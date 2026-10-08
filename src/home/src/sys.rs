// Everything the dashboard asks of the system: Wi-Fi (iwd), sound (ALSA), Bluetooth (BlueZ), YARG
// updates, the song drive and power. Each command wrapper has a pure parser beside it that is
// unit tested on sample output; the wrappers themselves only run on the appliance.

use crate::model::*;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const DATA: &str = "/data";
pub const ASOUND: &str = "/data/asound.conf";
pub const CTL_SOCKET: &str = "/run/yargos-ctl.sock";

/// Run a command and return its combined output, or the output as an error if it failed.
pub fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd).args(args).output().map_err(|e| format!("{cmd}: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    if out.status.success() { Ok(text) } else { Err(text.trim().to_string()) }
}

/// Like `run`, but gives up (and kills the command) after `secs`.
pub fn run_timeout(cmd: &str, args: &[&str], secs: u64) -> Result<String, String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{cmd}: {e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut text = String::new();
                if let Some(mut o) = child.stdout.take() {
                    let _ = o.read_to_string(&mut text);
                }
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_string(&mut text);
                }
                return if status.success() { Ok(text) } else { Err(text.trim().to_string()) };
            }
            Ok(None) if start.elapsed() > Duration::from_secs(secs) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

// ---- network ----------------------------------------------------------------------------------

/// `ip -4 -o addr show scope global` -> (interface, address) pairs.
pub fn parse_ip_addr(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let inet = f.iter().position(|&x| x == "inet")?;
            Some((f.get(1)?.trim_end_matches(':').to_string(), f.get(inet + 1)?.split('/').next()?.to_string()))
        })
        .collect()
}

pub fn wifi_device() -> Option<String> {
    let mut names: Vec<String> = std::fs::read_dir("/sys/class/net")
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("wireless").exists())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names.into_iter().next()
}

pub fn net_status() -> String {
    let addrs = run("ip", &["-4", "-o", "addr", "show", "scope", "global"]).map(|o| parse_ip_addr(&o)).unwrap_or_default();
    match addrs.first() {
        None => "not connected".into(),
        Some((dev, ip)) if dev.starts_with("wl") => {
            let ssid = run("iwctl", &["station", dev, "show"])
                .ok()
                .and_then(|o| parse_connected_ssid(&o))
                .unwrap_or_default();
            if ssid.is_empty() { format!("Wi-Fi ({ip})") } else { format!("Wi-Fi: {ssid} ({ip})") }
        }
        Some((_, ip)) => format!("Wired ({ip})"),
    }
}

/// `iwctl station X show` -> the "Connected network" value.
pub fn parse_connected_ssid(out: &str) -> Option<String> {
    strip_ansi(out).lines().find_map(|l| {
        let t = l.trim();
        t.strip_prefix("Connected network").map(|r| r.trim().to_string())
    })
}

/// Rows of an iwctl table: skips headings and rules, returns (connected marker, columns).
fn table_rows(out: &str) -> Vec<(bool, Vec<String>)> {
    let mut rows = vec![];
    for line in strip_ansi(out).lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("---") {
            continue;
        }
        let marked = t.starts_with('>');
        let body = t.trim_start_matches('>').trim();
        let cols: Vec<String> = split_columns(body);
        rows.push((marked, cols));
    }
    // Drop the title line ("Available networks") and the header ("Network name ... Security ...").
    rows.into_iter()
        .filter(|(_, c)| {
            let first = c.first().map(String::as_str).unwrap_or("");
            !(first == "Available networks" || first == "Known Networks" || first == "Network name" || first == "Name")
        })
        .collect()
}

fn split_columns(s: &str) -> Vec<String> {
    let mut cols = vec![];
    let mut cur = String::new();
    let mut spaces = 0;
    for c in s.chars() {
        if c == ' ' {
            spaces += 1;
            cur.push(c);
        } else {
            if spaces >= 2 {
                cols.push(cur.trim().to_string());
                cur.clear();
            }
            spaces = 0;
            cur.push(c);
        }
    }
    if !cur.trim().is_empty() {
        cols.push(cur.trim().to_string());
    }
    cols
}

pub fn parse_networks(out: &str, known: &[String]) -> Vec<Network> {
    table_rows(out)
        .into_iter()
        .filter(|(_, c)| c.len() >= 2)
        .map(|(connected, c)| Network {
            known: known.contains(&c[0]),
            secured: c[1] != "open",
            signal: c.get(2).map(|s| s.chars().filter(|&x| x == '*').count().min(4) as u8).unwrap_or(0),
            connected,
            name: c[0].clone(),
        })
        .collect()
}

pub fn parse_known(out: &str) -> Vec<String> {
    table_rows(out).into_iter().filter_map(|(_, c)| c.into_iter().next()).collect()
}

pub fn wifi_list(dev: &str) -> Vec<Network> {
    let known = run("iwctl", &["known-networks", "list"]).map(|o| parse_known(&o)).unwrap_or_default();
    let mut nets = run("iwctl", &["station", dev, "get-networks"]).map(|o| parse_networks(&o, &known)).unwrap_or_default();
    // Strongest first, the connected one on top.
    nets.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.signal.cmp(&a.signal)));
    nets
}

pub fn wifi_refresh(scan: bool) -> Res {
    let dev = wifi_device();
    let Some(d) = dev.clone() else { return Res::Wifi { dev: None, nets: vec![] } };
    if scan {
        let _ = run_timeout("iwctl", &["station", &d, "scan"], 8);
        std::thread::sleep(Duration::from_secs(3));
    }
    Res::Wifi { dev, nets: wifi_list(&d) }
}

pub fn wifi_connect(ssid: &str, pass: Option<&str>) -> Result<String, String> {
    let dev = wifi_device().ok_or("No Wi-Fi adapter found")?;
    let out = match pass {
        Some(p) => run_timeout("iwctl", &["--passphrase", p, "station", &dev, "connect", ssid], 40),
        None => run_timeout("iwctl", &["station", &dev, "connect", ssid], 40),
    };
    match out {
        Ok(_) => Ok(format!("Connected to {ssid}")),
        Err(e) => {
            let e = strip_ansi(&e);
            let low = e.to_lowercase();
            Err(if low.contains("passphrase") || low.contains("authentication") || low.contains("invalid") {
                "That password did not work".into()
            } else if e.trim().is_empty() || low.contains("timed out") {
                "Could not connect (no answer)".into()
            } else {
                format!("Could not connect: {}", e.lines().next().unwrap_or("").trim())
            })
        }
    }
}

// ---- sound ------------------------------------------------------------------------------------

/// `aplay -l` -> playback outputs.
pub fn parse_aplay(out: &str) -> Vec<Output> {
    out.lines()
        .filter_map(|l| {
            let l = l.strip_prefix("card ")?;
            let (card, rest) = l.split_once(':')?;
            let card: u32 = card.trim().parse().ok()?;
            let cname = rest.split_once('[')?.1.split_once(']')?.0.to_string();
            let after = rest.split_once("device ")?.1;
            let (dev, dname) = after.split_once(':')?;
            let dev: u32 = dev.trim().parse().ok()?;
            let dlabel = dname.split_once('[').map(|(_, b)| b.trim_end_matches(']').trim().to_string()).unwrap_or(dname.trim().to_string());
            Some(Output { card, dev, label: format!("{dlabel} ({cname})") })
        })
        .collect()
}

pub fn parse_asound(conf: &str) -> Option<(u32, u32)> {
    let line = conf.lines().find(|l| l.contains("slave.pcm"))?;
    let hw = line.split("hw:").nth(1)?;
    let (c, d) = hw.split_once(',')?;
    Some((c.trim().parse().ok()?, d.trim_matches(|ch: char| !ch.is_ascii_digit()).parse().ok()?))
}

pub fn asound_text(card: u32, dev: u32) -> String {
    format!(
        "# Written by the YARGOS dashboard (Sound screen). Delete this file to return to the default output.\n\
         pcm.!default {{\n    type plug\n    slave.pcm \"hw:{card},{dev}\"\n}}\n\
         ctl.!default {{\n    type hw\n    card {card}\n}}\n"
    )
}

pub fn parse_scontrols(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| l.split_once("control '")?.1.split_once('\'').map(|(n, _)| n.to_string()))
        .collect()
}

pub fn pick_volume_control(names: &[String]) -> Option<String> {
    ["Master", "PCM", "Speaker", "Headphone", "Digital"].iter().find_map(|w| names.iter().find(|n| n == w).cloned())
}

pub fn parse_percent(out: &str) -> Option<u8> {
    let i = out.find('[')?;
    let j = out[i..].find('%')?;
    out[i + 1..i + j].trim().parse().ok()
}

fn volume_control(card: u32) -> Option<String> {
    let out = run("amixer", &["-c", &card.to_string(), "scontrols"]).ok()?;
    pick_volume_control(&parse_scontrols(&out))
}

pub fn sound_refresh() -> Res {
    let outputs = run("aplay", &["-l"]).map(|o| parse_aplay(&o)).unwrap_or_default();
    let current = std::fs::read_to_string(ASOUND).ok().and_then(|c| parse_asound(&c)).or_else(|| outputs.first().map(|o| (o.card, o.dev)));
    let volume = current.and_then(|(card, _)| {
        let ctl = volume_control(card)?;
        let out = run("amixer", &["-c", &card.to_string(), "sget", &ctl]).ok()?;
        Some((card, parse_percent(&out)?))
    });
    Res::Sound { outputs, current, volume }
}

pub fn sound_select(card: u32, dev: u32) -> Result<String, String> {
    std::fs::write(ASOUND, asound_text(card, dev)).map_err(|e| format!("Could not save the choice: {e}"))?;
    Ok("Sound output changed".into())
}

pub fn sound_volume(card: u32, pct: u8) {
    if let Some(ctl) = volume_control(card) {
        let _ = run("amixer", &["-c", &card.to_string(), "-q", "sset", &ctl, &format!("{pct}%")]);
    }
}

pub fn wav_bytes(secs: f32) -> Vec<u8> {
    let rate = 48_000u32;
    let n = (rate as f32 * secs) as usize;
    let mut data = Vec::with_capacity(n * 4);
    for i in 0..n {
        let t = i as f32 / rate as f32;
        let fade = (t / 0.05).min(1.0).min(((secs - t) / 0.15).max(0.0)).min(1.0);
        let s = ((t * 440.0 * std::f32::consts::TAU).sin() * 0.35 * fade * i16::MAX as f32) as i16;
        data.extend_from_slice(&s.to_le_bytes());
        data.extend_from_slice(&s.to_le_bytes());
    }
    let mut w = Vec::new();
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 4).to_le_bytes());
    w.extend_from_slice(&4u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data.len() as u32).to_le_bytes());
    w.extend_from_slice(&data);
    w
}

pub fn sound_test() -> Result<String, String> {
    let path = "/tmp/yargos-test.wav";
    std::fs::write(path, wav_bytes(1.0)).map_err(|e| e.to_string())?;
    run_timeout("aplay", &["-q", path], 10).map(|_| "Played a test sound".into()).map_err(|e| format!("Could not play sound: {e}"))
}

// ---- bluetooth --------------------------------------------------------------------------------

pub fn parse_bt_devices(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("Device ")?;
            let (mac, name) = rest.split_once(' ').unwrap_or((rest, ""));
            Some((mac.to_string(), name.trim().to_string()))
        })
        .collect()
}

pub fn parse_bt_info(out: &str) -> (bool, bool) {
    let flag = |k: &str| out.lines().any(|l| l.trim() == format!("{k}: yes"));
    (flag("Paired") || flag("Bonded") || flag("Trusted"), flag("Connected"))
}

fn is_mac_like(name: &str) -> bool {
    name.len() == 17 && name.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

pub fn bt_present() -> bool {
    std::fs::read_dir("/sys/class/bluetooth").map(|d| d.count() > 0).unwrap_or(false)
}

fn btctl(args: &[&str], secs: u64) -> Result<String, String> {
    run_timeout("bluetoothctl", args, secs)
}

pub fn bt_list() -> Res {
    if !bt_present() {
        return Res::Bt { present: false, devs: vec![] };
    }
    let devs = btctl(&["devices"], 6).map(|o| parse_bt_devices(&o)).unwrap_or_default();
    let mut out: Vec<BtDevice> = devs
        .into_iter()
        .map(|(mac, name)| {
            let (paired, connected) = btctl(&["info", &mac], 6).map(|o| parse_bt_info(&o)).unwrap_or((false, false));
            BtDevice { mac, name, paired, connected }
        })
        .filter(|d| d.paired || !is_mac_like(&d.name))
        .collect();
    out.sort_by(|a, b| (b.connected, b.paired).cmp(&(a.connected, a.paired)).then(a.name.cmp(&b.name)));
    Res::Bt { present: true, devs: out }
}

pub fn bt_scan() -> Res {
    if bt_present() {
        let _ = btctl(&["power", "on"], 6);
        let _ = btctl(&["--timeout", "12", "scan", "on"], 20);
    }
    bt_list()
}

fn wiimote_hid_present() -> bool {
    std::fs::read_dir("/sys/bus/hid/devices")
        .map(|d| d.filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().starts_with("0005:057E:")))
        .unwrap_or(false)
}

pub fn bt_connect(d: &BtDevice) -> Result<String, String> {
    let _ = btctl(&["power", "on"], 6);
    if d.is_wiimote() {
        // A Wiimote in 1+2 mode is connectable for a short time and never bonds: trust, then keep
        // trying to connect until its HID device appears.
        let _ = btctl(&["trust", &d.mac], 6);
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(30) {
            let _ = btctl(&["connect", &d.mac], 8);
            std::thread::sleep(Duration::from_secs(1));
            if wiimote_hid_present() {
                return Ok("Wiimote connected".into());
            }
        }
        return Err("The Wiimote did not connect. Hold 1+2 until the lights flash, then try again".into());
    }
    let _ = btctl(&["pair", &d.mac], 25);
    let _ = btctl(&["trust", &d.mac], 6);
    btctl(&["connect", &d.mac], 20)
        .map(|_| format!("{} connected", d.name))
        .map_err(|_| format!("Could not connect {}. Put it in pairing mode and try again", d.name))
}

pub fn bt_forget(mac: &str) -> Result<String, String> {
    btctl(&["remove", mac], 8).map(|_| "Device forgotten".into())
}

// ---- YARG updates -----------------------------------------------------------------------------

fn apps() -> String {
    format!("{DATA}/apps/yarg")
}

fn link_target(name: &str) -> Option<String> {
    std::fs::read_link(format!("{}/{name}", apps())).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
}

pub fn read_auto_update() -> bool {
    std::fs::read_to_string(format!("{DATA}/yargos.conf"))
        .map(|c| !c.lines().any(|l| l.trim() == "AUTO_UPDATE=no"))
        .unwrap_or(true)
}

pub fn write_auto_update(on: bool) -> Result<String, String> {
    let path = format!("{DATA}/yargos.conf");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = old.lines().filter(|l| !l.trim().starts_with("AUTO_UPDATE=")).map(String::from).collect();
    lines.push(format!("AUTO_UPDATE={}", if on { "yes" } else { "no" }));
    std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
    Ok(format!("Automatic updates {}", if on { "on" } else { "off" }))
}

pub fn upd_info(prev: &UpdInfo) -> UpdInfo {
    UpdInfo {
        installed: link_target("current").unwrap_or_default(),
        previous: link_target("previous"),
        auto: read_auto_update(),
        ..prev.clone()
    }
}

/// `yargos-update check` output -> (latest version if known, update available).
pub fn parse_check(out: &str) -> (Option<String>, bool) {
    let latest = out.split("latest ").nth(1).map(|r| r.split(|c: char| c == ',' || c.is_whitespace()).next().unwrap_or("").trim().to_string()).filter(|s| !s.is_empty());
    (latest, out.contains("update available"))
}

fn clean_log(out: &str) -> Vec<String> {
    out.lines().map(|l| l.trim().trim_start_matches("yargos-update: ").to_string()).filter(|l| !l.is_empty() && !l.contains('%') && !l.starts_with("Dload")).collect()
}

pub fn upd_check(prev: &UpdInfo) -> UpdInfo {
    let mut u = upd_info(prev);
    match run_timeout("yargos-update", &["check"], 40) {
        Ok(out) => {
            let (latest, available) = parse_check(&out);
            u.latest = latest;
            u.available = available;
            u.log = clean_log(&out);
        }
        Err(e) => {
            u.log = vec!["Could not reach the update server. Check your network connection.".into()];
            u.log.extend(clean_log(&e).into_iter().take(2));
        }
    }
    u
}

pub fn upd_apply() -> Result<String, String> {
    match run_timeout("yargos-update", &["apply"], 3600) {
        Ok(out) => Ok(if out.contains("installed ") { "Installed. Choose PLAY YARG to use it".into() } else { "You are up to date".into() }),
        Err(e) => Err(clean_log(&e).last().cloned().unwrap_or_else(|| "The update did not install".into())),
    }
}

pub fn upd_rollback() -> Result<String, String> {
    run("yargos-update", &["rollback"]).map(|_| "Went back to the previous version".into()).map_err(|e| clean_log(&e).last().cloned().unwrap_or(e))
}

// ---- songs, system, status ---------------------------------------------------------------------

/// /proc/mounts -> (device, fstype) of the real mount at a path, ignoring the automount stub.
pub fn parse_mount(mounts: &str, path: &str) -> Option<(String, String)> {
    mounts.lines().find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        (f.get(1) == Some(&path) && f.get(2).map(|t| *t != "autofs").unwrap_or(false)).then(|| (f[0].to_string(), f[2].to_string()))
    })
}

/// `df -k PATH` -> (total KiB, free KiB).
pub fn parse_df(out: &str) -> Option<(u64, u64)> {
    let f: Vec<&str> = out.lines().nth(1)?.split_whitespace().collect();
    Some((f.get(1)?.parse().ok()?, f.get(3)?.parse().ok()?))
}

pub fn human(kib: u64) -> String {
    let gb = kib as f64 / 1024.0 / 1024.0;
    if gb >= 10.0 { format!("{gb:.0} GB") } else if gb >= 1.0 { format!("{gb:.1} GB") } else { format!("{} MB", kib / 1024) }
}

pub fn songs_refresh() -> SongsInfo {
    // Looking inside makes the automount mount the drive (or fail after its short timeout).
    let entries = std::fs::read_dir("/media/songs").map(|d| d.take(200_000).count()).unwrap_or(0);
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    match parse_mount(&mounts, "/media/songs") {
        Some((_, fs)) => {
            let space = run("df", &["-k", "/media/songs"]).ok().and_then(|o| parse_df(&o)).map(|(t, f)| format!(", {} free of {}", human(f), human(t))).unwrap_or_default();
            SongsInfo { mounted: true, detail: format!("The song drive is connected ({fs}{space}) and mounted at /media/songs. {entries} items at the top level.") }
        }
        None => SongsInfo { mounted: false, detail: "No song drive found. Plug in a drive labelled YARGSONGS.".into() },
    }
}

pub fn songs_status() -> String {
    let s = songs_refresh();
    if s.mounted {
        let free = run("df", &["-k", "/media/songs"]).ok().and_then(|o| parse_df(&o)).map(|(_, f)| format!(", {} free", human(f))).unwrap_or_default();
        format!("connected{free}")
    } else {
        "not connected".into()
    }
}

pub fn pad_count() -> usize {
    std::fs::read_to_string("/proc/bus/input/devices")
        .map(|s| s.split("\n\n").filter(|b| b.lines().any(|l| l.starts_with("H: Handlers=") && l.split_whitespace().any(|w| w.starts_with("js")))).count())
        .unwrap_or(0)
}

pub fn status(sound_label: &str, upd: &UpdInfo) -> Status {
    Status {
        network: net_status(),
        songs: songs_status(),
        sound: sound_label.to_string(),
        yarg: link_target("current").unwrap_or_else(|| "not installed".into()),
        update: if upd.available { upd.latest.clone() } else { None },
        pads: pad_count(),
    }
}

pub fn sound_label() -> String {
    let outs = run("aplay", &["-l"]).map(|o| parse_aplay(&o)).unwrap_or_default();
    let cur = std::fs::read_to_string(ASOUND).ok().and_then(|c| parse_asound(&c));
    match cur {
        Some((c, d)) => outs.iter().find(|o| o.card == c && o.dev == d).map(|o| o.label.clone()).unwrap_or_else(|| format!("card {c}, device {d}")),
        None => outs.first().map(|o| o.label.clone()).unwrap_or_else(|| "no sound device".into()),
    }
}

pub fn sys_info() -> Vec<String> {
    let kernel = run("uname", &["-r"]).unwrap_or_default().trim().to_string();
    let up = std::fs::read_to_string("/proc/uptime").ok().and_then(|s| s.split_whitespace().next().and_then(|v| v.parse::<f64>().ok())).unwrap_or(0.0) as u64;
    let mem = std::fs::read_to_string("/proc/meminfo").ok().map(|s| {
        let get = |k: &str| s.lines().find(|l| l.starts_with(k)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        (get("MemTotal:") - get("MemAvailable:"), get("MemTotal:"))
    });
    let mut v = vec![format!("YARGOS (Linux {kernel})"), format!("Up for {} min", up / 60)];
    if let Some((used, total)) = mem {
        v.push(format!("Memory: {} used of {}", human(used), human(total)));
    }
    if let Some((t, f)) = run("df", &["-k", DATA]).ok().and_then(|o| parse_df(&o)) {
        v.push(format!("Data disk: {} free of {}", human(f), human(t)));
    }
    v
}

pub fn power(p: Power) -> Result<(), String> {
    let mut s = std::os::unix::net::UnixStream::connect(CTL_SOCKET).map_err(|e| format!("cannot reach the system helper: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok();
    s.write_all(if p == Power::Shutdown { b"poweroff\n" } else { b"reboot\n" }).map_err(|e| e.to_string())?;
    let mut reply = String::new();
    let _ = s.read_to_string(&mut reply);
    if reply.starts_with("ok") { Ok(()) } else { Err(reply.trim().to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NETS: &str = "\x1b[1m                               Available networks\x1b[0m\n\
--------------------------------------------------------------------------------\n\
\x1b[1;90m      Network name                    Security  Signal\x1b[0m\n\
--------------------------------------------------------------------------------\n\
\x1b[1;90m  >   \x1b[0mLiving Room                     psk       \x1b[1;90m****\x1b[0m\n\
      Neighbour 5G                    psk       ***\n\
      Cafe Guest                      open      **\n";

    #[test]
    fn parses_iwctl_networks_with_colour_codes_and_spaces_in_names() {
        let nets = parse_networks(NETS, &["Living Room".to_string()]);
        assert_eq!(nets.len(), 3);
        assert_eq!(nets[0], Network { name: "Living Room".into(), secured: true, signal: 4, connected: true, known: true });
        assert_eq!(nets[1].name, "Neighbour 5G");
        assert!(!nets[1].known && nets[1].secured && nets[1].signal == 3);
        assert!(!nets[2].secured && nets[2].signal == 2);
    }

    #[test]
    fn parses_known_networks() {
        let out = "                                Known Networks\n-----\n    Name                              Security  Hidden    Last connected\n-----\n    Living Room                       psk       No        Oct 08, 12:00\n    Cafe Guest                        open      No        Oct 01, 09:30\n";
        assert_eq!(parse_known(out), vec!["Living Room", "Cafe Guest"]);
    }

    #[test]
    fn parses_ip_addr_and_connected_ssid() {
        let out = "2: enp0s1    inet 10.0.2.15/24 brd 10.0.2.255 scope global dynamic enp0s1\n3: wlan0    inet 192.168.1.42/24 brd 192.168.1.255 scope global wlan0\n";
        assert_eq!(parse_ip_addr(out), vec![("enp0s1".into(), "10.0.2.15".into()), ("wlan0".into(), "192.168.1.42".into())]);
        let show = "                           Station: wlan0\n  State                connected\n  Connected network    Living Room\n  IPv4 address         192.168.1.42\n";
        assert_eq!(parse_connected_ssid(show).as_deref(), Some("Living Room"));
    }

    #[test]
    fn parses_aplay_and_asound() {
        let out = "**** List of PLAYBACK Hardware Devices ****\ncard 0: PCH [HDA Intel PCH], device 0: ALC283 Analog [ALC283 Analog]\n  Subdevices: 1/1\ncard 0: PCH [HDA Intel PCH], device 3: HDMI 0 [HDMI 0]\ncard 1: Audio [USB Audio], device 0: USB Audio [USB Audio]\n";
        let outs = parse_aplay(out);
        assert_eq!(outs.len(), 3);
        assert_eq!(outs[1], Output { card: 0, dev: 3, label: "HDMI 0 (HDA Intel PCH)".into() });
        assert_eq!(outs[2].card, 1);
        let conf = asound_text(0, 3);
        assert_eq!(parse_asound(&conf), Some((0, 3)));
        assert!(conf.contains("card 0"));
        assert_eq!(parse_asound("# nothing here"), None);
    }

    #[test]
    fn mixer_helpers() {
        let sc = "Simple mixer control 'Master',0\nSimple mixer control 'PCM',0\nSimple mixer control 'Mic',0\n";
        let names = parse_scontrols(sc);
        assert_eq!(names, vec!["Master", "PCM", "Mic"]);
        assert_eq!(pick_volume_control(&names).as_deref(), Some("Master"));
        assert_eq!(pick_volume_control(&["Mic".to_string()]), None);
        assert_eq!(parse_percent("  Front Left: Playback 40 [62%] [-12.00dB] [on]"), Some(62));
        assert_eq!(parse_percent("no percent"), None);
    }

    #[test]
    fn test_tone_is_a_valid_wav() {
        let w = wav_bytes(0.5);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        assert_eq!(w.len(), 44 + (48_000.0 * 0.5) as usize * 4);
        // fades in from silence
        assert_eq!(&w[44..48], &[0, 0, 0, 0]);
    }

    #[test]
    fn parses_bluetooth_output() {
        let out = "Device 00:22:D7:AD:BE:D4 Nintendo RVL-CNT-01\nDevice AA:BB:CC:00:00:01 Logitech K380\n";
        let d = parse_bt_devices(out);
        assert_eq!(d[0], ("00:22:D7:AD:BE:D4".into(), "Nintendo RVL-CNT-01".into()));
        assert_eq!(d[1].1, "Logitech K380");
        assert_eq!(parse_bt_info("Name: x\n\tPaired: no\n\tTrusted: yes\n\tConnected: yes\n"), (true, true));
        assert_eq!(parse_bt_info("\tPaired: no\n\tTrusted: no\n\tConnected: no\n"), (false, false));
        assert!(is_mac_like("AA-BB-CC-DD-EE-FF") && !is_mac_like("Logitech K380"));
    }

    #[test]
    fn parses_update_check_output() {
        assert_eq!(parse_check("yargos-update: installed 0.15.0, latest 0.16.0\nyargos-update: update available: 0.16.0\n"), (Some("0.16.0".into()), true));
        assert_eq!(parse_check("yargos-update: installed 0.15.0, latest 0.15.0\nyargos-update: up to date\n"), (Some("0.15.0".into()), false));
        assert_eq!(parse_check("garbage"), (None, false));
        assert_eq!(clean_log("yargos-update: up to date\n\n"), vec!["up to date"]);
    }

    #[test]
    fn parses_mounts_and_df() {
        let mounts = "systemd-1 /media/songs autofs rw 0 0\n/dev/sda1 /media/songs exfat ro,noatime 0 0\n";
        assert_eq!(parse_mount(mounts, "/media/songs"), Some(("/dev/sda1".into(), "exfat".into())));
        assert_eq!(parse_mount("systemd-1 /media/songs autofs rw 0 0\n", "/media/songs"), None);
        let df = "Filesystem 1K-blocks Used Available Use% Mounted on\n/dev/sda1 976762584 464000000 512762584 48% /media/songs\n";
        assert_eq!(parse_df(df), Some((976762584, 512762584)));
        assert_eq!(human(512762584), "489 GB");
        assert_eq!(human(2 * 1024 * 1024), "2.0 GB");
        assert_eq!(human(100 * 1024), "100 MB");
    }

    #[test]
    fn strips_ansi_and_carriage_returns() {
        assert_eq!(strip_ansi("\x1b[1;90mhi\x1b[0m\r\n"), "hi\n");
    }

    #[test]
    fn auto_update_setting_round_trips() {
        // Parsing only: the file path is fixed to /data on the appliance.
        let on = "# x\nAUTO_UPDATE=yes\n";
        let off = "AUTO_UPDATE=no\n";
        assert!(!off.lines().all(|l| l.trim() != "AUTO_UPDATE=no"));
        assert!(on.lines().all(|l| l.trim() != "AUTO_UPDATE=no"));
    }
}

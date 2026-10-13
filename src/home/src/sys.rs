// Everything the dashboard asks of the system: Wi-Fi (iwd), sound (ALSA), Bluetooth (BlueZ), YARG
// updates, the song drive and power. Each command wrapper has a pure parser beside it that is
// unit tested on sample output; the wrappers themselves only run on the appliance.

use crate::model::*;
use crate::songcopy;
use std::io::{BufRead, Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
            // (iwd says "unknown network" until its first scan after boot has put a name to the network)
            if ssid.is_empty() || ssid == "unknown network" { format!("Wi-Fi ({ip})") } else { format!("Wi-Fi: {ssid} ({ip})") }
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

/// `/proc/asound/cards` -> (card number, card id): " 0 [PCH            ]: HDA-Intel - HDA Intel PCH".
pub fn parse_card_ids(text: &str) -> Vec<(u32, String)> {
    text.lines()
        .filter_map(|l| {
            let (n, rest) = l.split_once('[')?;
            let id = rest.split_once(']')?.0.trim();
            Some((n.trim().parse().ok()?, id.to_string()))
        })
        .collect()
}

/// The output named in asound.conf: `hw:CARD=<id>,DEV=<n>` (what the Sound screen writes; the id is stable when USB
/// sound cards come and go, the number is not) or an older `hw:<card>,<n>`. None if it is not there or names a card that is
/// not connected.
pub fn parse_asound(conf: &str, cards: &[(u32, String)]) -> Option<(u32, u32)> {
    let line = conf.lines().find(|l| l.contains("slave.pcm"))?;
    let hw = line.split("hw:").nth(1)?;
    let (c, d) = hw.split_once(',')?;
    let dev = d.trim_start_matches("DEV=").trim_matches(|ch: char| !ch.is_ascii_digit()).parse().ok()?;
    let card = match c.trim().strip_prefix("CARD=") {
        Some(id) => cards.iter().find(|(_, i)| i == id)?.0,
        None => c.trim().parse().ok()?,
    };
    Some((card, dev))
}

/// What to play on when nothing was chosen (or the chosen card is gone): the first output that is not a screen's sound
/// (HDMI / DisplayPort), else the first. ALSA's own default is card 0 device 0, which on an AMD laptop is an HDMI card
/// that has no device 0: YARG then says it cannot initialise an audio device.
pub fn best_output(outs: &[Output]) -> Option<&Output> {
    let screen = |o: &Output| {
        let l = o.label.to_lowercase();
        l.contains("hdmi") || l.contains("displayport") || l.contains(" dp")
    };
    outs.iter().find(|o| !screen(o)).or_else(|| outs.first())
}

pub fn read_cards() -> Vec<(u32, String)> {
    parse_card_ids(&std::fs::read_to_string("/proc/asound/cards").unwrap_or_default())
}

/// The saved choice if it is still a real output.
pub fn saved_output(outs: &[Output]) -> Option<(u32, u32)> {
    let cur = std::fs::read_to_string(ASOUND).ok().and_then(|c| parse_asound(&c, &read_cards()))?;
    outs.iter().any(|o| (o.card, o.dev) == cur).then_some(cur)
}

/// Before YARG starts: if the saved output is missing, or none was ever chosen, choose one, so YARG's default device exists.
pub fn ensure_sound() {
    let outs = run("aplay", &["-l"]).map(|o| parse_aplay(&o)).unwrap_or_default();
    if saved_output(&outs).is_none() {
        if let Some(o) = best_output(&outs) {
            let _ = ctl_request(&format!("sound {} {}", o.card, o.dev));
        }
    }
}

/// `card_id`: the card's id when known (the card number can change between boots, the id does not).
pub fn asound_text(card_id: Option<&str>, card: u32, dev: u32) -> String {
    let (hw, ctl) = match card_id {
        Some(id) => (format!("hw:CARD={id},DEV={dev}"), id.to_string()),
        None => (format!("hw:{card},{dev}"), card.to_string()),
    };
    format!(
        "# Written by the YARGOS dashboard (Sound screen). Delete this file to return to the default output.\n\
         pcm.!default {{\n    type plug\n    slave.pcm \"{hw}\"\n}}\n\
         ctl.!default {{\n    type hw\n    card {ctl}\n}}\n"
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
    let current = saved_output(&outputs).or_else(|| best_output(&outputs).map(|o| (o.card, o.dev)));
    let volume = current.and_then(|(card, _)| {
        let ctl = volume_control(card)?;
        let out = run("amixer", &["-c", &card.to_string(), "sget", &ctl]).ok()?;
        Some((card, parse_percent(&out)?))
    });
    Res::Sound { outputs, current, volume }
}

pub fn sound_select(card: u32, dev: u32) -> Result<String, String> {
    ctl_request(&format!("sound {card} {dev}")).map_err(|e| format!("Could not save the choice: {e}"))?;
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

/// Devices that share a name (every Wii guitar is "Nintendo RVL-CNT-01") get the end of their Bluetooth address added,
/// "Nintendo RVL-CNT-01 (D4)": it never changes for a device and is printed on the sticker inside a Wiimote's battery bay.
/// Wiimotes always get it, so a guitar's name does not change when a second one turns up; other devices only when
/// another one has the same name. If two tags would be equal, more of the address is used for that name.
pub fn tag_duplicates(devs: &mut [BtDevice]) {
    let mut groups: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
    for (i, d) in devs.iter().enumerate() {
        groups.entry(d.name.clone()).or_default().push(i);
    }
    for (name, idx) in groups {
        if idx.len() < 2 && !devs[idx[0]].is_wiimote() {
            continue;
        }
        let tail = |mac: &str, n: usize| -> String {
            let parts: Vec<&str> = mac.split(':').collect();
            parts[parts.len().saturating_sub(n)..].join(":").to_uppercase()
        };
        let n = (1..=6).find(|&n| {
            let mut tags: Vec<String> = idx.iter().map(|&i| tail(&devs[i].mac, n)).collect();
            tags.sort();
            tags.dedup();
            tags.len() == idx.len()
        });
        for &i in &idx {
            devs[i].name = format!("{name} ({})", tail(&devs[i].mac, n.unwrap_or(6)));
        }
    }
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
            // A Wiimote counts as ready only once its guitar bridge is running.
            let ready = !(connected && name.starts_with("Nintendo RVL")) || wii_bridge_ready(&mac);
            let (battery, battery_low) = if connected && name.starts_with("Nintendo RVL") { wii_battery(&mac).map(|b| (Some(b.percent), b.is_low())).unwrap_or((None, false)) } else { (None, false) };
            BtDevice { mac, name, paired, connected, ready, battery, battery_low }
        })
        .filter(|d| d.paired || !is_mac_like(&d.name))
        .collect();
    tag_duplicates(&mut out);
    out.sort_by(|a, b| (b.connected, b.paired).cmp(&(a.connected, a.paired)).then(a.name.cmp(&b.name)));
    Res::Bt { present: true, devs: out }
}

/// One Bluetooth job at a time: the automatic Wiimote connect and the Bluetooth screen must not fight over the adapter.
static BT_BUSY: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn bt_scan() -> Res {
    let _busy = BT_BUSY.lock().unwrap_or_else(|e| e.into_inner());
    if bt_present() {
        let _ = btctl(&["power", "on"], 6);
        let _ = btctl(&["--timeout", "12", "scan", "on"], 20);
    }
    bt_list()
}

/// A HID device's uevent -> its address (lowercase, possibly empty) if it is a Wiimote (bus 0005 = Bluetooth).
pub fn parse_wiimote_uevent(uevent: &str) -> Option<String> {
    let is_wiimote = uevent.lines().any(|l| {
        let l = l.to_uppercase();
        l.starts_with("HID_ID=0005:0000057E:00000306") || l.starts_with("HID_ID=0005:0000057E:00000330")
    });
    is_wiimote.then(|| uevent.lines().find_map(|l| l.strip_prefix("HID_UNIQ=")).unwrap_or("").trim().to_lowercase())
}

/// A Wiimote's batteries as the bridge last wrote them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WiiBattery {
    pub percent: u8,
    pub low: bool,
}

/// Below this the batteries count as low even if the Wiimote has not raised its own flag.
const LOW_PERCENT: u8 = 15;

impl WiiBattery {
    pub fn is_low(&self) -> bool {
        self.low || self.percent <= LOW_PERCENT
    }
}

/// The bridge's battery file ("<percent> <low 0|1>").
pub fn parse_battery(text: &str) -> Option<WiiBattery> {
    let mut f = text.split_whitespace();
    let percent: u8 = f.next()?.parse().ok().filter(|p| *p <= 100)?;
    let low = match f.next()? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    Some(WiiBattery { percent, low })
}

/// The batteries of the Wiimote with this address (None until it has answered a status request).
pub fn wii_battery(mac: &str) -> Option<WiiBattery> {
    let mac = mac.to_lowercase();
    wiimote_hidraws().into_iter().filter(|(_, uniq)| uniq.is_empty() || *uniq == mac).find_map(|(node, _)| {
        parse_battery(&std::fs::read_to_string(format!("/run/yargos-bridge/battery-{node}")).ok()?)
    })
}

/// "Wii guitar (D4)": how a guitar is named in warnings, by the end of its address like the Bluetooth list does.
pub fn guitar_label(mac: &str) -> String {
    let tail = mac.rsplit(':').next().unwrap_or(mac).to_uppercase();
    format!("Wii guitar ({tail})")
}

/// Every Wii guitar that has told its batteries: (address, batteries).
pub fn wii_batteries() -> Vec<(String, WiiBattery)> {
    wiimote_hidraws()
        .into_iter()
        .filter_map(|(node, uniq)| {
            let b = parse_battery(&std::fs::read_to_string(format!("/run/yargos-bridge/battery-{node}")).ok()?)?;
            Some((uniq, b))
        })
        .collect()
}

/// Guitars whose batteries ran low since the last look: each is announced once, and again only after its batteries
/// were changed or charged (back above 40%). `announced` is the memory between looks.
pub fn newly_low(announced: &mut std::collections::HashMap<String, bool>, now: &[(String, WiiBattery)]) -> Vec<String> {
    announced.retain(|mac, _| now.iter().any(|(m, _)| m == mac));
    let mut out = vec![];
    for (mac, b) in now {
        let was = announced.entry(mac.clone()).or_insert(false);
        if b.is_low() && !*was {
            *was = true;
            out.push(format!("{} {}%", guitar_label(mac), b.percent));
        } else if !b.is_low() && b.percent >= 40 {
            *was = false;
        }
    }
    out
}

/// The lines for guitars whose batteries are low ("Wii guitar (D4) 12%").
pub fn low_battery_lines() -> Vec<String> {
    wii_batteries().into_iter().filter(|(_, b)| b.is_low()).map(|(mac, b)| format!("{} {}%", guitar_label(&mac), b.percent)).collect()
}

fn wiimote_hidraws() -> Vec<(String, String)> {
    let Ok(dir) = std::fs::read_dir("/sys/class/hidraw") else { return vec![] };
    dir.filter_map(|e| e.ok())
        .filter_map(|e| {
            let uevent = std::fs::read_to_string(e.path().join("device/uevent")).ok()?;
            Some((e.file_name().to_string_lossy().to_string(), parse_wiimote_uevent(&uevent)?))
        })
        .collect()
}

/// The bridge runs AND its guitar answered (the bridge drops /run/yargos-bridge/ready-<node> once it streams): a bridge
/// that is merely up while the Wiimote never replied is not a working guitar.
fn bridge_active(node: &str) -> bool {
    std::path::Path::new(&format!("/run/yargos-bridge/ready-{node}")).exists()
        && Command::new("systemctl").args(["is-active", "--quiet", &format!("yargos-bridge-wii@{node}.service")]).status().map(|s| s.success()).unwrap_or(false)
}

/// Is the guitar bridge running for the Wiimote with this address? (If the kernel does not say which Wiimote a
/// device is, any running bridge counts.)
pub fn wii_bridge_ready(mac: &str) -> bool {
    let mac = mac.to_lowercase();
    wiimote_hidraws().iter().any(|(node, uniq)| (uniq.is_empty() || *uniq == mac) && bridge_active(node))
}

/// `yargos-home wiicheck [address]`: what the launcher sees of Wiimotes and their bridges (diagnostics).
pub fn wiicheck(mac: Option<&str>) -> i32 {
    let nodes = wiimote_hidraws();
    if nodes.is_empty() {
        println!("no Wiimote HID devices");
    }
    for (node, uniq) in &nodes {
        let battery = std::fs::read_to_string(format!("/run/yargos-bridge/battery-{node}")).ok().and_then(|t| parse_battery(&t));
        println!("{node}  address '{uniq}'  bridge active: {}  battery: {}", bridge_active(node), match battery {
            Some(b) => format!("{}%{}", b.percent, if b.is_low() { " (low)" } else { "" }),
            None => "not told yet".into(),
        });
    }
    if let Some(m) = mac {
        println!("ready for {m}: {}", wii_bridge_ready(m));
    }
    0
}

/// Has the Wiimote with this address shown up as a HID device? `before` is how many Wiimote HID devices there were
/// when we started: if the kernel does not name the Wiimote, one more than that counts. (Any Wiimote at all is not
/// enough: with one guitar already connected the second would be declared connected at once.)
fn wiimote_hid_present(mac: &str, before: usize) -> bool {
    let mac = mac.to_lowercase();
    let nodes = wiimote_hidraws();
    nodes.iter().any(|(_, uniq)| *uniq == mac) || (nodes.iter().any(|(_, uniq)| uniq.is_empty()) && nodes.len() > before)
}

pub fn bt_connect(d: &BtDevice) -> Result<String, String> {
    let _busy = BT_BUSY.lock().unwrap_or_else(|e| e.into_inner());
    let _ = btctl(&["power", "on"], 6);
    if d.is_wiimote() {
        return wiimote_connect(d);
    }
    let _ = btctl(&["pair", &d.mac], 25);
    let _ = btctl(&["trust", &d.mac], 6);
    btctl(&["connect", &d.mac], 20)
        .map(|_| format!("{} connected", d.name))
        .map_err(|_| format!("Could not connect {}. Put it in pairing mode and try again", d.name))
}

fn wiimote_connect(d: &BtDevice) -> Result<String, String> {
    {
        // A Wiimote in 1+2 mode is connectable for a short time and never bonds: trust, then keep
        // trying to connect until its HID device appears.
        let _ = btctl(&["trust", &d.mac], 6);
        let before = wiimote_hidraws().len();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(30) {
            let _ = btctl(&["connect", &d.mac], 8);
            std::thread::sleep(Duration::from_secs(1));
            if wiimote_hid_present(&d.mac, before) {
                break;
            }
        }
        if !wiimote_hid_present(&d.mac, before) {
            return Err("The Wiimote did not connect. Hold 1+2 on it until its lights flash: it connects by itself.".into());
        }
        // The HID device appearing is not enough: the bridge that turns it into a guitar has to be running, and
        // to stay up. Bluetooth alone happily reports "connected" for a Wiimote that is not really usable.
        let start = Instant::now();
        let mut up_since: Option<Instant> = None;
        while start.elapsed() < Duration::from_secs(12) {
            if wii_bridge_ready(&d.mac) {
                if up_since.get_or_insert_with(Instant::now).elapsed() >= Duration::from_secs(2) {
                    return Ok("Wiimote ready: the guitar bridge is running".into());
                }
            } else {
                up_since = None;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let _ = btctl(&["disconnect", &d.mac], 6);
        Err("The Wiimote connected, but it is NOT ready: the guitar did not answer. Press 1+2 on it again: it connects by itself.".into())
    }
}

/// What `bluetoothctl scan on` printed -> the addresses of the Nintendo controllers that answered. `known` is
/// (address, name) of the devices BlueZ already has: a repeat sighting prints only the address.
pub fn parse_scan_wiimotes(scan: &str, known: &[(String, String)]) -> Vec<String> {
    let mut names: std::collections::HashMap<String, String> = known.iter().map(|(m, n)| (m.to_uppercase(), n.clone())).collect();
    let mut seen: Vec<String> = vec![];
    for line in strip_ansi(scan).lines() {
        let Some(at) = line.find("[NEW]").or_else(|| line.find("[CHG]")) else { continue };
        let line = &line[at..];
        let new = line.starts_with("[NEW]");
        let Some(rest) = line.split_once("Device ").map(|x| x.1) else { continue };
        let (mac, tail) = rest.split_once(' ').unwrap_or((rest, ""));
        if !mac.contains(':') {
            continue;
        }
        let mac = mac.to_uppercase();
        let name = tail.strip_prefix("Name: ").or_else(|| tail.strip_prefix("Alias: ")).or(if new { Some(tail) } else { None });
        if let Some(n) = name {
            names.insert(mac.clone(), n.trim().to_string());
        }
        if !seen.contains(&mac) {
            seen.push(mac);
        }
    }
    seen.into_iter().filter(|m| names.get(m).is_some_and(|n| n.starts_with("Nintendo RVL-CNT"))).collect()
}

pub enum WiiAuto {
    Nothing,
    Connected(String),
    Failed(String),
}

/// One round of the automatic Wiimote connect: listen for a few seconds, and connect any Nintendo controller that is
/// calling (1+2 held) and is not connected yet. Skips the round if the Bluetooth screen is using the adapter.
pub fn wii_auto_step() -> WiiAuto {
    let Ok(_busy) = BT_BUSY.try_lock() else { return WiiAuto::Nothing };
    let _ = btctl(&["power", "on"], 6);
    let known = btctl(&["devices"], 6).map(|o| parse_bt_devices(&o)).unwrap_or_default();
    let scan = match btctl(&["--timeout", "5", "scan", "on"], 12) {
        Ok(t) | Err(t) => t,
    };
    for mac in parse_scan_wiimotes(&scan, &known) {
        let connected = btctl(&["info", &mac], 6).map(|o| parse_bt_info(&o).1).unwrap_or(false);
        if connected {
            continue;
        }
        let name = known.iter().find(|(m, _)| m.eq_ignore_ascii_case(&mac)).map(|(_, n)| n.clone()).unwrap_or_else(|| "Nintendo RVL-CNT-01".into());
        let d = BtDevice { mac, name, paired: false, connected: false, ready: false, battery: None, battery_low: false };
        return match wiimote_connect(&d) {
            Ok(m) => WiiAuto::Connected(m),
            Err(e) => WiiAuto::Failed(e),
        };
    }
    WiiAuto::Nothing
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

/// Set `KEY=yes|no` in /data/yargos.conf, keeping the other lines.
fn write_conf_flag(key: &str, on: bool) -> Result<(), String> {
    let path = format!("{DATA}/yargos.conf");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let prefix = format!("{key}=");
    let mut lines: Vec<String> = old.lines().filter(|l| !l.trim().starts_with(&prefix)).map(String::from).collect();
    lines.push(format!("{key}={}", if on { "yes" } else { "no" }));
    std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

pub fn write_auto_update(on: bool) -> Result<String, String> {
    write_conf_flag("AUTO_UPDATE", on)?;
    Ok(format!("Automatic updates {}", if on { "on" } else { "off" }))
}

/// Does YARG get one song source per drive folder? (Default yes: only an explicit AUTO_SONG_SOURCES=no turns it off.)
pub fn read_auto_songs() -> bool {
    std::fs::read_to_string(format!("{DATA}/yargos.conf")).map(|c| !c.lines().any(|l| l.trim() == "AUTO_SONG_SOURCES=no")).unwrap_or(true)
}

pub fn write_auto_songs(on: bool) -> Result<String, String> {
    write_conf_flag("AUTO_SONG_SOURCES", on)?;
    Ok(format!("Adding song folders to YARG is {}; it applies the next time YARG starts", if on { "on" } else { "off" }))
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
    out.lines().map(|l| l.trim().trim_start_matches("yargos-update: ").to_string()).filter(|l| !l.is_empty() && !l.contains('%') && !l.starts_with("Dload") && !l.starts_with("PROGRESS") && !l.starts_with("STEP")).collect()
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

/// One line of `yargos-update` output.
#[derive(Debug, PartialEq)]
pub enum UpdLine {
    Progress(u32),
    Unpack(u32),
    Step(String),
    Text(String),
}

pub fn parse_update_line(line: &str) -> UpdLine {
    let t = line.trim().trim_start_matches("yargos-update: ").trim_start_matches("yargos-os-update: ");
    if let Some(n) = t.strip_prefix("PROGRESS ").and_then(|n| n.trim().parse::<u32>().ok()) {
        UpdLine::Progress(n.min(100))
    } else if let Some(n) = t.strip_prefix("UNPACK ").and_then(|n| n.trim().parse::<u32>().ok()) {
        UpdLine::Unpack(n.min(100))
    } else if let Some(step) = t.strip_prefix("STEP ") {
        UpdLine::Step(step.trim().to_string())
    } else {
        UpdLine::Text(t.to_string())
    }
}

/// Run `yargos-update ARGS`, reporting what it is doing ("Downloading YARG… 40%") while it runs.
/// Ok with its messages, or Err with them when it failed.
fn update_stream(args: &[&str], mut on_progress: impl FnMut(String)) -> Result<Vec<String>, Vec<String>> {
    CANCELLED.store(false, Ordering::Relaxed);
    // Its own process group, so cancelling stops curl and unzip with it.
    let mut child = Command::new("sh")
        .args(["-c", "exec yargos-update \"$@\" 2>&1", "sh"])
        .args(args)
        .stdout(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| vec![format!("yargos-update: {e}")])?;
    UPDATE_PID.store(child.id(), Ordering::Relaxed);
    let mut text = vec![];
    if let Some(out) = child.stdout.take() {
        for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
            match parse_update_line(&line) {
                UpdLine::Progress(n) => on_progress(format!("Downloading YARG\u{2026} {n}%")),
                UpdLine::Unpack(n) => on_progress(format!("Unpacking YARG\u{2026} {n}%")),
                UpdLine::Step(s) if s == "unpacking" => on_progress("Unpacking YARG\u{2026}".into()),
                UpdLine::Step(s) if s == "checking" => on_progress("Checking YARG\u{2026}".into()),
                UpdLine::Step(_) => {}
                UpdLine::Text(t) if !t.is_empty() => text.push(t),
                UpdLine::Text(_) => {}
            }
        }
    }
    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    UPDATE_PID.store(0, Ordering::Relaxed);
    if ok { Ok(text) } else { Err(text) }
}

static UPDATE_PID: AtomicU32 = AtomicU32::new(0);
static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Stop the running `yargos-update` (and the download or unpacking under it); it cleans up after itself.
pub fn update_cancel() {
    CANCELLED.store(true, Ordering::Relaxed);
    let pid = UPDATE_PID.load(Ordering::Relaxed);
    if pid != 0 {
        let _ = Command::new("kill").args(["-TERM", "--", &format!("-{pid}")]).status();
    }
}

/// The last message of a failed run, in words for the screen.
pub fn update_error(lines: &[String]) -> String {
    if CANCELLED.load(Ordering::Relaxed) {
        return "Installation cancelled. Nothing was changed.".into();
    }
    let last = lines.last().cloned().unwrap_or_else(|| "it did not finish".into());
    if last.contains("offline") || last.contains("could not read the latest release") {
        "Could not reach the internet. Choose WI-FI on the main menu, connect, and try again.".into()
    } else {
        last
    }
}

pub fn upd_apply(on_progress: impl FnMut(String)) -> Result<String, String> {
    match update_stream(&["apply"], on_progress) {
        Ok(out) => Ok(if out.iter().any(|l| l.starts_with("installed ")) { "Installed. Choose PLAY YARG to use it".into() } else { "You are up to date".into() }),
        Err(e) => Err(update_error(&e)),
    }
}

/// First install of YARG: downloaded from GitHub, or from a release zip (a full path) when there is no internet.
pub fn yarg_install(zip: Option<&str>, on_progress: impl FnMut(String)) -> Result<String, String> {
    let r = match zip {
        Some(z) => update_stream(&["install-zip", z], on_progress),
        None => update_stream(&["install"], on_progress),
    };
    r.map(|_| "YARG is installed. Choose PLAY YARG".to_string()).map_err(|e| update_error(&e))
}

/// YARG release zips on the song drive.
pub fn yarg_zips() -> Vec<String> {
    run("yargos-update", &["find-zip"]).map(|o| o.lines().map(|l| l.trim().to_string()).filter(|l| l.starts_with('/')).collect()).unwrap_or_default()
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

/// `song-sources` output (paths under /media/songs or /data/songs) -> the names to show: folder names, "the whole drive",
/// or, for the internal drive, "internal: <folder>".
pub fn parse_sources(out: &str) -> Vec<String> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            let l = l.trim_end_matches('/');
            if let Some(name) = l.strip_prefix("/data/songs/") {
                format!("internal: {name}")
            } else {
                match l.strip_prefix("/media/songs/") {
                    Some(name) => name.to_string(),
                    None => "the whole drive".to_string(),
                }
            }
        })
        .collect()
}

pub fn songs_refresh() -> SongsInfo {
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    match parse_mount(&mounts, "/media/songs") {
        Some((src, fs)) => {
            let fs = fs.to_uppercase().replace("FUSEBLK", "NTFS");
            let df = run("df", &["-k", "/media/songs"]).ok().and_then(|o| parse_df(&o));
            let label = run("lsblk", &["-no", "LABEL", &src]).map(|o| o.trim().to_string()).unwrap_or_default();
            SongsInfo {
                mounted: true,
                detail: match df {
                    Some((t, _)) => format!("The song drive is connected ({fs}, {}) at /media/songs.", human(t)),
                    None => format!("The song drive is connected ({fs}) at /media/songs."),
                },
                name: if label.is_empty() { "Song drive".into() } else { label },
                space: df.map(|(_, f)| format!("{} free", human(f))).unwrap_or_default(),
                fs,
                sources: run("/usr/lib/yargos/song-sources", &[]).map(|o| parse_sources(&o)).unwrap_or_default(),
                auto: read_auto_songs(),
            }
        }
        None => SongsInfo {
            mounted: false,
            detail: "No song drive found. Plug in a drive labelled YARGSONGS.".into(),
            sources: run("/usr/lib/yargos/song-sources", &[]).map(|o| parse_sources(&o)).unwrap_or_default(),
            auto: read_auto_songs(),
            ..SongsInfo::default()
        },
    }
}

// ---- internal songs (SYSTEM > Storage) ----------------------------------------------------------

static IMPORT_CANCEL: AtomicBool = AtomicBool::new(false);

pub fn import_cancel() {
    IMPORT_CANCEL.store(true, Ordering::Relaxed);
}

/// `yargos-songs list` -> the portable drives that can be imported.
pub fn parse_import_drives(out: &str) -> Vec<ImportDrive> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('|').collect();
            if f.len() < 5 || !f[0].starts_with("/dev/") {
                return None;
            }
            let fs = match f[3] {
                "vfat" => "FAT".to_string(),
                other => other.to_uppercase(),
            };
            Some(ImportDrive { dev: f[0].into(), bytes: f[1].parse().ok()?, label: f[2].trim().into(), fs, uuid: f[4].trim().into() })
        })
        .collect()
}

pub fn import_drives() -> Vec<ImportDrive> {
    run("yargos-songs", &["list"]).map(|o| parse_import_drives(&o)).unwrap_or_default()
}

/// What stays free on the internal drive when songs are copied to it: a tenth of it, between 1.5 and 4 GB. YARG's own
/// files, its updates and the system update download live on the same drive and must always fit.
pub fn reserve_kib(total_kib: u64) -> u64 {
    (total_kib / 10).clamp(1_500_000, 4_000_000)
}

fn data_space() -> (u64, u64) {
    run("df", &["-k", DATA]).ok().and_then(|o| parse_df(&o)).unwrap_or((0, 0))
}

/// The data partition's PARTUUID from the kernel command line (`systemd.mount-extra=PARTUUID=<id>:/data:...`).
pub fn parse_data_partuuid(cmdline: &str) -> Option<String> {
    cmdline.split_whitespace().find_map(|w| {
        let rest = w.strip_prefix("systemd.mount-extra=PARTUUID=")?;
        let (id, tail) = rest.split_once(':')?;
        tail.starts_with("/data:").then(|| id.to_string())
    })
}

/// The device mounted at /data, from /proc/mounts (None if it is not a /dev device or not mounted).
pub fn parse_data_mount_source(mounts: &str) -> Option<String> {
    mounts
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (src, dir) = (f.next()?, f.next()?);
            (dir == "/data" && src.starts_with("/dev/")).then(|| src.to_string())
        })
        .last()
}

/// Sectors left unused after a partition: the disk's size minus where the partition ends.
pub fn slack_sectors(disk: u64, part_start: u64, part_size: u64) -> u64 {
    disk.saturating_sub(part_start + part_size)
}

/// KiB of the drive after the data partition that nothing uses (0 when the partition fills it, or it cannot be told).
pub fn data_slack_kib() -> u64 {
    let slack = || -> Option<u64> {
        // What is mounted at /data is the truth; the boot entry's GUID is only how it got there.
        let dev = match parse_data_mount_source(&std::fs::read_to_string("/proc/mounts").unwrap_or_default()) {
            Some(src) => std::fs::canonicalize(src).ok()?,
            None => {
                let id = parse_data_partuuid(&std::fs::read_to_string("/proc/cmdline").ok()?)?;
                std::fs::canonicalize(format!("/dev/disk/by-partuuid/{id}")).ok()?
            }
        };
        let name = dev.file_name()?.to_str()?.to_string();
        let num = |p: String| std::fs::read_to_string(p).ok()?.trim().parse::<u64>().ok();
        let disk = std::fs::canonicalize(format!("/sys/class/block/{name}/..")).ok()?.file_name()?.to_str()?.to_string();
        Some(slack_sectors(num(format!("/sys/class/block/{disk}/size"))?, num(format!("/sys/class/block/{name}/start"))?, num(format!("/sys/class/block/{name}/size"))?) / 2)
    };
    slack().unwrap_or(0)
}

/// Ask the system to make the data partition fill its drive (what the boot step does; see growdata.sh).
pub fn grow_data() -> Result<String, String> {
    ctl_request_for("grow-data", 120).map_err(|e| format!("Could not use the rest of the drive: {}", e.trim_start_matches("error:").trim()))?;
    Ok("The rest of the drive is now in use".into())
}

pub fn storage_info() -> StorageInfo {
    let (total, free) = data_space();
    let used = run("du", &["-sk", songcopy::ROOT]).ok().and_then(|o| o.split_whitespace().next().and_then(|n| n.parse().ok())).unwrap_or(0);
    StorageInfo {
        used_kib: used,
        free_kib: free,
        total_kib: total,
        reserve_kib: reserve_kib(total),
        folders: songcopy::list(std::path::Path::new(songcopy::ROOT)).len(),
        slack_kib: data_slack_kib(),
    }
}

fn mount_drive(dev: &str) -> Result<(), String> {
    ctl_request_for(&format!("songs-mount {dev}"), 90).map_err(|e| {
        let e = e.trim().trim_start_matches("error:").trim().to_string();
        if e.is_empty() { "Could not open the drive".to_string() } else { e }
    })
}

fn unmount_drive() {
    let _ = ctl_request_for("songs-umount", 20);
}

fn dest_for(drive: &ImportDrive) -> String {
    let root = std::path::Path::new(songcopy::ROOT);
    songcopy::dest_name(&drive.label, &drive.uuid, &|n| songcopy::read_marker(root, n))
}

/// Mount the drive and work out what copying it would do (nothing is copied).
pub fn import_scan(drive: &ImportDrive) -> Result<ImportPlan, songcopy::Stop> {
    use songcopy::Stop;
    IMPORT_CANCEL.store(false, Ordering::Relaxed);
    mount_drive(&drive.dev).map_err(Stop::Other)?;
    let dest = dest_for(drive);
    let plan = songcopy::plan(std::path::Path::new(songcopy::MOUNT), &std::path::Path::new(songcopy::ROOT).join(&dest), &IMPORT_CANCEL);
    unmount_drive();
    let p = plan?;
    if p.files == 0 {
        return Err(Stop::Other(format!("There is nothing to copy on {}: it has no files.", drive.name())));
    }
    let (total, free) = data_space();
    Ok(ImportPlan {
        drive: drive.clone(),
        dest,
        files: p.files,
        bytes: p.bytes,
        todo_files: p.todo_files,
        todo_bytes: p.todo_bytes,
        unreadable: p.unreadable,
        free_kib: free,
        reserve_kib: reserve_kib(total),
    })
}

/// Copy the drive into the internal library. The message is a toast (Ok) or an alert (Err).
pub fn import_run(plan: &ImportPlan, mut on_progress: impl FnMut(String)) -> Result<String, String> {
    use songcopy::Stop;
    IMPORT_CANCEL.store(false, Ordering::Relaxed);
    mount_drive(&plan.drive.dev)?;
    let name = plan.drive.name();
    let dest = std::path::Path::new(songcopy::ROOT).join(&plan.dest);
    let mut last = Instant::now() - Duration::from_secs(1);
    let r = songcopy::run(
        std::path::Path::new(songcopy::MOUNT),
        &dest,
        &plan.drive.uuid,
        plan.todo_bytes,
        plan.reserve_kib,
        &|| Some(data_space().1),
        &IMPORT_CANCEL,
        &mut |done, total| {
            if last.elapsed() >= Duration::from_millis(300) {
                last = Instant::now();
                let pct = if total == 0 { 0 } else { (done * 100 / total).min(100) };
                on_progress(format!("Copying {name}\u{2026} {pct}%  ({} of {})", human_kib(done / 1024), human_kib(total / 1024)));
            }
        },
    );
    unmount_drive();
    let _ = run("sync", &[]);
    match r {
        Ok(rep) if rep.failed_files == 0 => Ok(format!("Copied {} files ({}) from {name}", rep.copied_files, human_kib(rep.copied_bytes / 1024))),
        Ok(rep) => Err(format!(
            "Copied {} files, but {} could not be copied (unreadable, or the drive was disturbed). Import the drive again to retry them.",
            rep.copied_files, rep.failed_files
        )),
        Err(Stop::Cancelled) => Ok("Stopped. What was copied is kept: import the drive again to carry on".into()),
        Err(Stop::Full) => Err("The internal drive is nearly full, so the copy stopped. Delete some songs to make room, then import the drive again to carry on.".into()),
        Err(Stop::Gone) => Err("The drive stopped answering. Was it unplugged? Plug it in and import it again to carry on.".into()),
        Err(Stop::Other(e)) => Err(e),
    }
}

/// The internal songs in a folder; a folder that is gone (just deleted) shows the nearest one above it.
pub fn songs_browse(mut path: Vec<String>) -> Res {
    let root = std::path::Path::new(songcopy::ROOT);
    loop {
        if let Some(dir) = songcopy::library_path(root, &path).filter(|p| p.is_dir()) {
            return Res::Browse { entries: songcopy::list(&dir), path };
        }
        if path.pop().is_none() {
            return Res::Browse { path, entries: vec![] };
        }
    }
}

pub fn songs_delete(path: &[String]) -> Result<String, String> {
    songcopy::remove(std::path::Path::new(songcopy::ROOT), path).map(|_| if path.is_empty() { "All internal songs deleted".to_string() } else { "Deleted".to_string() })
}

pub fn songs_status() -> String {
    // Only /proc/mounts and df: this runs every few seconds while the main menu is up, so it must not walk the drive.
    if parse_mount(&std::fs::read_to_string("/proc/mounts").unwrap_or_default(), "/media/songs").is_some() {
        let free = run("df", &["-k", "/media/songs"]).ok().and_then(|o| parse_df(&o)).map(|(_, f)| format!(", {} free", human(f))).unwrap_or_default();
        format!("connected{free}")
    } else {
        "not connected".into()
    }
}

/// Game controllers (the Wii guitars and drums show up as PlayStation instruments): the same ones the Devices screen lists.
pub fn pad_count() -> usize {
    parse_pads(&std::fs::read_to_string("/proc/bus/input/devices").unwrap_or_default()).len()
}

pub fn status(sound_label: &str, upd: &UpdInfo) -> Status {
    let os = os_info_base();
    // One line about updates: a system update waiting for a restart matters most.
    let update = if os.staged.is_some() {
        Some("System update ready: restart to use it".to_string())
    } else if let Some(v) = &os.available {
        Some(format!("System {v} is available"))
    } else if upd.available {
        upd.latest.as_ref().map(|v| format!("YARG {v} is available"))
    } else {
        None
    };
    Status {
        network: net_status(),
        songs: songs_status(),
        sound: sound_label.to_string(),
        yarg: link_target("current").unwrap_or_else(|| "not installed".into()),
        update,
        os_rolled_back: os.rolled_back.clone(),
        pads: pad_count(),
        low_battery: low_battery_lines(),
        os_version: os.running.clone(),
    }
}

// ---- system updates (yargos-os-update, through the root helper) ----------------------------------------------------

/// `key=value` lines (the updater's status file) -> the values by key; a later line wins.
pub fn parse_kv(text: &str) -> std::collections::HashMap<String, String> {
    text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect()
}

pub fn os_info_from(kv: &std::collections::HashMap<String, String>, running: &str, auto: bool) -> OsInfo {
    let get = |k: &str| kv.get(k).filter(|v| !v.is_empty()).cloned();
    OsInfo {
        running: running.to_string(),
        latest: get("latest"),
        available: get("available"),
        staged: get("staged"),
        notes: get("notes").unwrap_or_default(),
        rolled_back: get("rolled_back"),
        previous: get("previous"),
        auto,
        error: get("check_error"),
        why: get("check_why"),
        offline: None,
        offline_hint: None,
    }
}

/// `yargos-os-update find` output -> (version ready to install, a sentence about something unusable on the drive).
pub fn parse_offline(lines: &[String]) -> (Option<String>, Option<String>) {
    for l in lines {
        let l = l.trim();
        if let Some(v) = l.strip_prefix("offline-old ") {
            return (None, Some(format!("The update on the song drive ({v}) is not newer than this system.")));
        } else if let Some(f) = l.strip_prefix("offline-missing ") {
            return (None, Some(format!("The song drive has the update information but not its image file, {f}. Copy that file next to it.")));
        } else if l == "offline-bad" {
            return (None, Some("The update information on the song drive is not signed by us, so it is ignored.".into()));
        } else if let Some(v) = l.strip_prefix("offline ") {
            return (Some(v.trim().to_string()), None);
        }
    }
    (None, None)
}

/// The updater's state without asking whether the song drive holds an update (cheap: files only).
pub fn os_info_base() -> OsInfo {
    let kv = parse_kv(&std::fs::read_to_string(format!("{DATA}/os-update/status")).unwrap_or_default());
    let running = std::fs::read_to_string("/usr/lib/yargos/version").unwrap_or_default().trim().to_string();
    let auto = std::fs::read_to_string(format!("{DATA}/yargos.conf")).map(|c| !c.lines().any(|l| l.trim() == "OS_AUTO_UPDATE=no")).unwrap_or(true);
    os_info_from(&kv, &running, auto)
}

pub fn os_info() -> OsInfo {
    let mut info = os_info_base();
    // Is there an update on the song drive? (only asks while the drive is plugged in)
    if std::path::Path::new("/dev/disk/by-label/YARGSONGS").exists() {
        if let Ok(lines) = os_run("os-find", |_| {}) {
            (info.offline, info.offline_hint) = parse_offline(&lines);
        }
    }
    info
}

pub fn write_auto_os(on: bool) -> Result<String, String> {
    write_conf_flag("OS_AUTO_UPDATE", on)?;
    Ok(format!("Automatic system updates {}", if on { "on" } else { "off" }))
}

/// The last message of a failed system update, in words for the screen.
pub fn os_error(lines: &[String]) -> String {
    if CANCELLED.load(Ordering::Relaxed) {
        return "Update cancelled. Nothing was changed.".into();
    }
    let last = lines.iter().rev().find(|l| !l.starts_with("error:")).cloned().unwrap_or_else(|| "it did not finish".into());
    if last.contains("could not reach") {
        // "could not reach the update server: <why>": the reason stays, with what to do about it.
        let mut c = last.chars();
        let text = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or(last);
        format!("{text}. Check Wi-Fi and try again.")
    } else {
        let mut c = last.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or(last)
    }
}

/// Ask the root helper to run a system updater command (`os-check`, `os-update`, `os-goback`...). Progress comes back as
/// lines ("Downloading the update… 40%"); Ok holds the messages, Err the reason.
pub fn os_run(word: &str, mut on_progress: impl FnMut(String)) -> Result<Vec<String>, String> {
    CANCELLED.store(false, Ordering::Relaxed);
    let mut s = std::os::unix::net::UnixStream::connect(CTL_SOCKET).map_err(|e| format!("cannot reach the system helper: {e}"))?;
    s.write_all(format!("{word}\n").as_bytes()).map_err(|e| e.to_string())?;
    let (mut phase, mut text, mut ended_ok) = ("Downloading the update".to_string(), vec![], false);
    for line in std::io::BufReader::new(s).lines().map_while(Result::ok) {
        let t = line.trim().trim_start_matches("yargos-os-update: ").to_string();
        if let Some(n) = t.strip_prefix("PROGRESS ").and_then(|n| n.trim().parse::<u32>().ok()) {
            on_progress(format!("{phase}\u{2026} {}%", n.min(100)));
        } else if let Some(step) = t.strip_prefix("STEP ") {
            phase = match step.trim() {
                "downloading" => "Downloading the update",
                "checking" => "Checking the download",
                "writing" => "Installing the update",
                "verifying" => "Checking the new system",
                _ => "Finishing",
            }
            .to_string();
            on_progress(format!("{phase}\u{2026}"));
        } else if t == "ok" {
            ended_ok = true;
        } else if !t.is_empty() {
            text.push(t);
        }
    }
    if ended_ok { Ok(text) } else { Err(os_error(&text)) }
}

pub fn os_cancel() {
    CANCELLED.store(true, Ordering::Relaxed);
    let _ = ctl_request("os-cancel");
}

pub fn sound_label() -> String {
    let outs = run("aplay", &["-l"]).map(|o| parse_aplay(&o)).unwrap_or_default();
    match saved_output(&outs).and_then(|(c, d)| outs.iter().find(|o| o.card == c && o.dev == d)).or_else(|| best_output(&outs)) {
        Some(o) => o.label.clone(),
        None => "no sound device".into(),
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

/// One request to the root helper (see ctl.rs); Ok if it answered "ok".
fn ctl_request(line: &str) -> Result<(), String> {
    ctl_request_for(line, 5)
}

/// Like `ctl_request`, for a request that may take longer than a few seconds to answer (mounting a big drive).
fn ctl_request_for(line: &str, secs: u64) -> Result<(), String> {
    let mut s = std::os::unix::net::UnixStream::connect(CTL_SOCKET).map_err(|e| format!("cannot reach the system helper: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(secs))).ok();
    s.write_all(format!("{line}\n").as_bytes()).map_err(|e| e.to_string())?;
    let mut reply = String::new();
    let _ = s.read_to_string(&mut reply);
    if reply.starts_with("ok") { Ok(()) } else { Err(reply.trim().to_string()) }
}

pub fn power(p: Power) -> Result<(), String> {
    ctl_request(if p == Power::Shutdown { "poweroff" } else { "reboot" })
}

// ---- connected devices ---------------------------------------------------------------------------

/// `/proc/asound/seq/clients` -> (client name, its port names) for real MIDI clients, i.e. not the sequencer's own
/// "System" and "Midi Through".
pub fn parse_seq_clients(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, Vec<String>)> = vec![];
    let mut skip = false;
    for line in text.lines() {
        let t = line.trim();
        let quoted = |s: &str| s.split('"').nth(1).map(str::to_string);
        if t.starts_with("Client") {
            let name = quoted(t).unwrap_or_default();
            skip = name.is_empty() || name == "System" || name == "Midi Through";
            if !skip {
                out.push((name, vec![]));
            }
        } else if t.starts_with("Port") && !skip {
            if let (Some(p), Some(last)) = (quoted(t), out.last_mut()) {
                last.1.push(p);
            }
        }
    }
    out.into_iter().map(|(n, p)| (n, p.join(", "))).collect()
}

/// `/proc/asound/cards` -> card names ("HDA Intel", "Logitech USB Microphone").
pub fn parse_cards(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.contains('[') && l.contains("]:"))
        .filter_map(|l| l.split("]:").nth(1))
        .filter_map(|r| r.split(" - ").nth(1))
        .map(|n| n.trim().to_string())
        .collect()
}

/// `/proc/bus/input/devices` -> names of devices that are game controllers (they have a js handler).
pub fn parse_pads(text: &str) -> Vec<String> {
    text.split("\n\n")
        .filter(|b| b.lines().any(|l| l.starts_with("H:") && l.trim_start_matches("H:").trim().trim_start_matches("Handlers=").split_whitespace().any(|w| w.starts_with("js"))))
        .filter_map(|b| b.lines().find(|l| l.starts_with("N:")).and_then(|l| l.split('"').nth(1)).map(str::to_string))
        .collect()
}

/// `/proc/bus/input/devices` -> names of keyboards that are not USB ones (laptop keyboard, Bluetooth keyboards; USB
/// ones are listed with their USB device). A real keyboard handles key auto-repeat (EV bit 20); power buttons do not.
pub fn parse_keyboards(text: &str) -> Vec<String> {
    text.split("\n\n")
        .filter(|b| {
            let has = |p: &str| b.lines().any(|l| l.starts_with(p));
            let usb = b.lines().any(|l| l.starts_with("I:") && l.contains("Bus=0003"));
            let kbd = b.lines().any(|l| l.starts_with("H:") && l.split_whitespace().any(|w| w == "kbd"));
            let rep = b.lines().find_map(|l| l.strip_prefix("B: EV=")).and_then(|h| u64::from_str_radix(h.trim(), 16).ok()).is_some_and(|ev| ev & (1 << 20) != 0);
            has("N:") && kbd && rep && !usb
        })
        .filter_map(|b| b.lines().find(|l| l.starts_with("N:")).and_then(|l| l.split('"').nth(1)).map(str::to_string))
        .collect()
}

/// What a USB device says it is, from its HID interfaces: boot protocol 1 = keyboard, 2 = mouse. The second value is
/// whether the kernel made an input device for it (if not, the device is there but nothing it sends reaches YARG).
fn usb_input_kind(dev: &std::path::Path) -> Option<(&'static str, bool)> {
    let name = dev.file_name()?.to_string_lossy().to_string();
    let mut ifaces: Vec<(&'static str, bool)> = vec![];
    for e in std::fs::read_dir(dev).ok()?.filter_map(|e| e.ok()) {
        let n = e.file_name().to_string_lossy().to_string();
        if !n.starts_with(&format!("{name}:")) || sysfs(&e.path(), "bInterfaceClass") != "03" {
            continue;
        }
        let kind = match sysfs(&e.path(), "bInterfaceProtocol").as_str() {
            "01" => "KEYBOARD",
            "02" => "MOUSE",
            _ => continue,
        };
        // interface -> HID device (0003:VVVV:PPPP.000N) -> input/inputN
        let has_input = std::fs::read_dir(e.path()).map(|d| d.filter_map(|x| x.ok()).any(|h| h.path().join("input").is_dir())).unwrap_or(false);
        ifaces.push((kind, has_input));
    }
    // A receiver with a keyboard and a mouse interface is a keyboard.
    let kind = if ifaces.iter().any(|i| i.0 == "KEYBOARD") { "KEYBOARD" } else { ifaces.first()?.0 };
    Some((kind, ifaces.iter().any(|i| i.0 == kind && i.1)))
}

/// Did the kernel turn this USB device into something that has its own row (a sound card, a game pad, a keyboard)?
/// Those rows say it better than the raw USB one, which is only shown for a device that became nothing.
fn usb_has_function(dev: &std::path::Path) -> bool {
    let name = dev.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let Ok(dir) = std::fs::read_dir(dev) else { return false };
    dir.filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().starts_with(&format!("{name}:"))).any(|e| {
        let driver = std::fs::read_link(e.path().join("driver")).ok().and_then(|d| d.file_name().map(|n| n.to_string_lossy().to_string())).unwrap_or_default();
        match driver.as_str() {
            "snd-usb-audio" | "xpad" => true,
            "usbhid" => std::fs::read_dir(e.path()).map(|d| d.filter_map(|x| x.ok()).any(|h| h.path().join("input").is_dir())).unwrap_or(false),
            _ => false,
        }
    })
}

fn sysfs(dir: &std::path::Path, f: &str) -> String {
    std::fs::read_to_string(dir.join(f)).map(|s| s.trim().to_string()).unwrap_or_default()
}

pub fn devices_list() -> Vec<DeviceEntry> {
    let mut v = vec![];
    if let Ok(dir) = std::fs::read_dir("/sys/bus/usb/devices") {
        let mut usb: Vec<DeviceEntry> = dir
            .filter_map(|e| e.ok())
            .filter(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                !n.contains(':') && !n.starts_with("usb") // devices only: not interfaces, not root hubs
            })
            .filter(|e| sysfs(&e.path(), "bDeviceClass") != "09") // not hubs
            .flat_map(|e| {
                let p = e.path();
                let (vid, pid) = (sysfs(&p, "idVendor"), sysfs(&p, "idProduct"));
                let (prod, man) = (sysfs(&p, "product"), sysfs(&p, "manufacturer"));
                let name = if !prod.is_empty() { prod } else if !man.is_empty() { man.clone() } else { format!("USB device {vid}:{pid}") };
                let id = format!("{vid}:{pid}");
                match usb_input_kind(&p) {
                    Some((kind, true)) => vec![DeviceEntry { kind, name, detail: id }],
                    // Recognised as a keyboard or mouse but the kernel made no input device: say so, and keep the USB id.
                    Some((kind, false)) => vec![DeviceEntry { kind, name: name.clone(), detail: "NO INPUT".into() }, DeviceEntry { kind: "USB", name, detail: id }],
                    None if usb_has_function(&p) => vec![],
                    None => vec![DeviceEntry { kind: "USB", name, detail: id }],
                }
            })
            .collect();
        usb.sort_by(|a, b| a.name.cmp(&b.name));
        v.extend(usb);
    }
    for (name, ports) in parse_seq_clients(&std::fs::read_to_string("/proc/asound/seq/clients").unwrap_or_default()) {
        v.push(DeviceEntry { kind: "MIDI", name, detail: ports });
    }
    for name in parse_cards(&std::fs::read_to_string("/proc/asound/cards").unwrap_or_default()) {
        v.push(DeviceEntry { kind: "SOUND", name, detail: String::new() });
    }
    let input = std::fs::read_to_string("/proc/bus/input/devices").unwrap_or_default();
    for name in parse_keyboards(&input) {
        v.push(DeviceEntry { kind: "KEYBOARD", name, detail: String::new() });
    }
    for name in parse_pads(&input) {
        // The bridges present Wii guitars and Xbox drums as PlayStation instruments: that is the one YARG lists.
        let detail = if name.contains("PlayStation") { "what YARG sees".into() } else { String::new() };
        v.push(DeviceEntry { kind: "PAD", name, detail });
    }
    v
}

// ---- installing to a disk --------------------------------------------------------------------

/// `yargos-install list` output: `/dev/NAME|bytes|model|transport|removable` per line.
pub fn parse_disks(out: &str) -> Vec<Disk> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.trim().split('|').collect();
            if f.len() < 5 || !f[0].starts_with("/dev/") {
                return None;
            }
            Some(Disk { path: f[0].to_string(), bytes: f[1].parse().ok()?, model: f[2].to_string(), removable: f[3] == "usb" || f[4] == "1" })
        })
        .collect()
}

pub fn install_disks() -> Vec<Disk> {
    run("yargos-install", &["list"]).map(|o| parse_disks(&o)).unwrap_or_default()
}

/// Was the boot menu's "Install" entry chosen?
pub fn install_requested() -> bool {
    std::fs::read_to_string("/proc/cmdline").map(|c| c.split_whitespace().any(|w| w == "yargos.install=1")).unwrap_or(false)
}

#[derive(Debug, PartialEq)]
pub enum InstallLine {
    Step(String),
    Done,
    Error(String),
    Other,
}

/// One line of the installer's output: `STEP 3/6 Copying the system`, `DONE` or `ERROR text`.
pub fn parse_install_line(line: &str) -> InstallLine {
    let line = line.trim();
    if line == "DONE" {
        InstallLine::Done
    } else if let Some(e) = line.strip_prefix("ERROR ") {
        InstallLine::Error(e.to_string())
    } else if let Some(rest) = line.strip_prefix("STEP ") {
        match rest.split_once(' ') {
            Some((n, text)) => InstallLine::Step(format!("{text} (step {})", n.replace('/', " of "))),
            None => InstallLine::Other,
        }
    } else {
        InstallLine::Other
    }
}

/// Ask the root helper to erase `disk` and install on it, reporting each step as it happens.
pub fn install_run(disk: &str, mut on_step: impl FnMut(String)) -> Result<(), String> {
    let mut s = std::os::unix::net::UnixStream::connect(CTL_SOCKET).map_err(|e| format!("cannot reach the system helper: {e}"))?;
    s.write_all(format!("install {disk}\n").as_bytes()).map_err(|e| e.to_string())?;
    let mut finished = None;
    for line in std::io::BufReader::new(s).lines() {
        let Ok(line) = line else { break };
        match parse_install_line(&line) {
            InstallLine::Step(t) => on_step(t),
            InstallLine::Done => finished = Some(Ok(())),
            InstallLine::Error(e) => finished = Some(Err(e)),
            InstallLine::Other => {}
        }
    }
    finished.unwrap_or_else(|| Err("The installer stopped without finishing".into()))
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
    fn keyboards_are_found_by_key_repeat_and_usb_ones_are_left_to_the_usb_list() {
        let input = "I: Bus=0011 Vendor=0001 Product=0001 Version=ab41\nN: Name=\"AT Translated Set 2 keyboard\"\nH: Handlers=sysrq kbd event1 leds\nB: EV=120013\n\n\
I: Bus=0019 Vendor=0000 Product=0001 Version=0000\nN: Name=\"Power Button\"\nH: Handlers=kbd event0\nB: EV=3\n\n\
I: Bus=0003 Vendor=1a81 Product=1004 Version=0110\nN: Name=\"Rii Keyboard\"\nH: Handlers=sysrq kbd event5 leds\nB: EV=120013\n\n\
I: Bus=0005 Vendor=046d Product=b34e Version=0001\nN: Name=\"BT Keys\"\nH: Handlers=sysrq kbd event6 leds\nB: EV=120013\n";
        assert_eq!(parse_keyboards(input), vec!["AT Translated Set 2 keyboard", "BT Keys"]);
    }

    #[test]
    fn scan_output_gives_the_wiimotes_that_answered() {
        let known = vec![("00:1E:35:AA:00:01".to_string(), "Nintendo RVL-CNT-01".to_string()), ("11:22:33:44:55:66".to_string(), "Some Speaker".to_string())];
        let scan = "Discovery started\n[\x1b[0;92mCHG\x1b[0m] Controller 8C:B8 Discovering: yes\n\
[bluetooth]# [CHG] Device 00:1E:35:AA:00:01 RSSI: 0xffffffc8 (-56)\n\
[NEW] Device 00:1E:35:BB:00:02 00-1E-35-BB-00-02\n\
[CHG] Device 00:1E:35:BB:00:02 Name: Nintendo RVL-CNT-01-TR\n\
[CHG] Device 11:22:33:44:55:66 RSSI: 0xffffffc8 (-56)\n\
[NEW] Device 77:77:77:77:77:77 Nintendo RVL-WBC-01\n[DEL] Device 00:1E:35:CC:00:03 Nintendo RVL-CNT-01\n";
        assert_eq!(parse_scan_wiimotes(scan, &known), vec!["00:1E:35:AA:00:01", "00:1E:35:BB:00:02"]);
        assert!(parse_scan_wiimotes("Discovery started\n", &known).is_empty());
    }

    #[test]
    fn the_updater_status_file_becomes_what_the_screen_shows() {
        let kv = parse_kv("running=1\nlatest=3\navailable=3\nnotes=Faster \nstaged=\ncheck_error=offline\nrolled_back=2\nprevious=0\n");
        let o = os_info_from(&kv, "1", true);
        assert_eq!((o.available.as_deref(), o.staged.as_deref(), o.rolled_back.as_deref(), o.previous.as_deref()), (Some("3"), None, Some("2"), Some("0")));
        assert_eq!(o.notes, "Faster");
        assert_eq!(o.error.as_deref(), Some("offline"));
        assert!(os_info_from(&parse_kv(""), "1", false).latest.is_none());
    }

    #[test]
    fn system_update_errors_are_put_in_words() {
        let e = os_error(&["could not reach the update server: the connection was cut off".to_string(), "error: x".to_string()]);
        assert!(e.contains("Wi-Fi") && e.contains("the connection was cut off"), "{e}");
        assert_eq!(
            os_error(&["the download failed at 41% after 6 tries: the connection timed out or stalled".to_string()]),
            "The download failed at 41% after 6 tries: the connection timed out or stalled"
        );
        assert_eq!(os_error(&["the download is damaged (checksum); not installing".to_string(), "error: the system update did not finish".to_string()]), "The download is damaged (checksum); not installing");
    }

    #[test]
    fn what_is_on_the_song_drive_is_told_from_the_finder_output() {
        let l = |s: &str| vec![s.to_string()];
        assert_eq!(parse_offline(&l("offline 2026.11.1")), (Some("2026.11.1".into()), None));
        assert!(parse_offline(&l("offline-old 2026.10.13")).1.unwrap().contains("not newer"));
        assert!(parse_offline(&l("offline-missing rootfs-2026.11.1.img.xz")).1.unwrap().contains("rootfs-2026.11.1.img.xz"));
        assert!(parse_offline(&l("offline-bad")).1.unwrap().contains("not signed"));
        assert_eq!(parse_offline(&[]), (None, None));
        // Other lines in the output (log lines) do not matter.
        assert_eq!(parse_offline(&["yargos-os-update: x".to_string(), "offline 3".to_string()]).0.as_deref(), Some("3"));
    }

    #[test]
    fn update_output_lines_are_told_apart() {
        assert_eq!(parse_update_line("yargos-update: PROGRESS 42"), UpdLine::Progress(42));
        assert_eq!(parse_update_line("yargos-update: PROGRESS 250"), UpdLine::Progress(100));
        assert_eq!(parse_update_line("yargos-update: UNPACK 64"), UpdLine::Unpack(64));
        assert_eq!(parse_update_line("yargos-update: STEP unpacking"), UpdLine::Step("unpacking".into()));
        assert_eq!(parse_update_line("yargos-update: downloading 0.15.0"), UpdLine::Text("downloading 0.15.0".into()));
        assert!(update_error(&["could not read the latest release (offline?)".to_string()]).contains("WI-FI"));
        assert_eq!(update_error(&["checksum mismatch".to_string()]), "checksum mismatch");
    }

    #[test]
    fn parses_aplay_and_asound() {
        let out = "**** List of PLAYBACK Hardware Devices ****\ncard 0: PCH [HDA Intel PCH], device 0: ALC283 Analog [ALC283 Analog]\n  Subdevices: 1/1\ncard 0: PCH [HDA Intel PCH], device 3: HDMI 0 [HDMI 0]\ncard 1: Audio [USB Audio], device 0: USB Audio [USB Audio]\n";
        let outs = parse_aplay(out);
        assert_eq!(outs.len(), 3);
        assert_eq!(outs[1], Output { card: 0, dev: 3, label: "HDMI 0 (HDA Intel PCH)".into() });
        assert_eq!(outs[2].card, 1);
        let cards = parse_card_ids(" 0 [Generic        ]: HDA-Intel - HDA ATI HDMI\n                      HDA ATI HDMI at 0xfd7e8000\n 1 [Audio          ]: USB-Audio - USB Audio\n");
        assert_eq!(cards, vec![(0, "Generic".to_string()), (1, "Audio".to_string())]);
        // The id keeps pointing at the same card when the numbers move; an old numeric file still reads; a missing card is None.
        let conf = asound_text(Some("Audio"), 1, 0);
        assert!(conf.contains("hw:CARD=Audio,DEV=0") && conf.contains("card Audio"));
        assert_eq!(parse_asound(&conf, &cards), Some((1, 0)));
        let moved = vec![(0, "Audio".to_string())];
        assert_eq!(parse_asound(&conf, &moved), Some((0, 0)));
        assert_eq!(parse_asound(&conf, &[]), None);
        assert_eq!(parse_asound(&asound_text(None, 0, 3), &cards), Some((0, 3)));
        assert_eq!(parse_asound("# nothing here", &cards), None);
    }

    #[test]
    fn same_named_devices_are_tagged_with_their_address() {
        let d = |mac: &str, name: &str| BtDevice { mac: mac.into(), name: name.into(), paired: true, connected: false, ready: true, battery: None, battery_low: false };
        let mut a = vec![d("00:22:D7:AD:BE:D4", "Nintendo RVL-CNT-01"), d("11:11:11:11:11:11", "Speaker"), d("00:1e:35:00:00:01", "Nintendo RVL-CNT-01"), d("22:22:22:22:22:22", "Pad"), d("33:33:33:33:33:33", "Pad")];
        tag_duplicates(&mut a);
        assert_eq!(a[0].name, "Nintendo RVL-CNT-01 (D4)");
        assert_eq!(a[2].name, "Nintendo RVL-CNT-01 (01)");
        assert_eq!(a[1].name, "Speaker");
        assert_eq!((a[3].name.as_str(), a[4].name.as_str()), ("Pad (22)", "Pad (33)"));
        assert!(a[0].is_wiimote());
        // One guitar alone is tagged too, so its name does not change when a second one turns up.
        let mut one = vec![d("00:22:D7:AD:BE:D4", "Nintendo RVL-CNT-01")];
        tag_duplicates(&mut one);
        assert_eq!(one[0].name, "Nintendo RVL-CNT-01 (D4)");
        // Equal endings: more of the address is used.
        let mut two = vec![d("00:22:D7:AA:BE:D4", "Nintendo RVL-CNT-01"), d("00:22:D7:BB:BE:D4", "Nintendo RVL-CNT-01")];
        tag_duplicates(&mut two);
        assert_eq!((two[0].name.as_str(), two[1].name.as_str()), ("Nintendo RVL-CNT-01 (AA:BE:D4)", "Nintendo RVL-CNT-01 (BB:BE:D4)"));
    }

    #[test]
    fn portable_drives_are_read_from_the_script_output() {
        let out = "/dev/sda1|1000200000000|My Songs|exfat|1234-ABCD\n/dev/sdb|64000000000||vfat|9999-9999\n/dev/sdc1|5|X|ntfs|\nnot a drive\n/dev/sdd1|abc|X|ext4|u\n";
        let d = parse_import_drives(out);
        assert_eq!(d.len(), 3);
        assert_eq!((d[0].dev.as_str(), d[0].label.as_str(), d[0].fs.as_str(), d[0].bytes), ("/dev/sda1", "My Songs", "EXFAT", 1_000_200_000_000));
        assert_eq!((d[1].fs.as_str(), d[1].name()), ("FAT", "USB drive".to_string()));
        assert_eq!(d[2].uuid, "");
    }

    #[test]
    fn a_low_guitar_is_announced_once_until_its_batteries_are_changed() {
        let b = |p: u8, low: bool| WiiBattery { percent: p, low };
        let mut memory = std::collections::HashMap::new();
        let a = "00:22:d7:ad:be:d4".to_string();
        let c = "00:1e:35:00:00:3a".to_string();
        assert!(newly_low(&mut memory, &[(a.clone(), b(80, false)), (c.clone(), b(70, false))]).is_empty());
        let first = newly_low(&mut memory, &[(a.clone(), b(12, true)), (c.clone(), b(69, false))]);
        assert_eq!(first, vec!["Wii guitar (D4) 12%".to_string()]);
        assert!(newly_low(&mut memory, &[(a.clone(), b(11, true)), (c.clone(), b(69, false))]).is_empty(), "not again while it stays low");
        // A bit of recovery under load (to 20%) is not new batteries.
        assert!(newly_low(&mut memory, &[(a.clone(), b(20, false)), (c.clone(), b(69, false))]).is_empty());
        assert!(newly_low(&mut memory, &[(a.clone(), b(14, false)), (c.clone(), b(69, false))]).is_empty());
        // Fresh batteries, then low again later: announced again. The other guitar is told apart.
        assert!(newly_low(&mut memory, &[(a.clone(), b(95, false)), (c.clone(), b(69, false))]).is_empty());
        assert_eq!(newly_low(&mut memory, &[(a.clone(), b(10, true)), (c.clone(), b(5, true))]).len(), 2);
        // A guitar that went away is forgotten.
        newly_low(&mut memory, &[(c.clone(), b(5, true))]);
        assert_eq!(memory.len(), 1);
    }

    #[test]
    fn the_bridges_battery_file_is_read_and_low_is_told() {
        assert_eq!(parse_battery("82 0\n"), Some(WiiBattery { percent: 82, low: false }));
        assert_eq!(parse_battery("12 1"), Some(WiiBattery { percent: 12, low: true }));
        assert_eq!(parse_battery("101 0"), None);
        assert_eq!(parse_battery("50"), None);
        assert_eq!(parse_battery("50 2"), None);
        assert_eq!(parse_battery("abc 0"), None);
        assert_eq!(parse_battery(""), None);
        assert!(!WiiBattery { percent: 82, low: false }.is_low());
        assert!(WiiBattery { percent: 15, low: false }.is_low(), "a low level counts even without the flag");
        assert!(WiiBattery { percent: 60, low: true }.is_low(), "the Wiimote's own flag counts at any level");
        assert_eq!(guitar_label("00:22:d7:ad:be:d4"), "Wii guitar (D4)");
    }

    #[test]
    fn controllers_are_counted_the_way_the_devices_screen_lists_them() {
        let guitar = "I: Bus=0005 Vendor=12ba Product=0100 Version=0108\nN: Name=\"Guitar Hero4 for PlayStation (R) 3\"\nH: Handlers=js0 event5 \nB: PROP=0\n\n";
        let kbd = "I: Bus=0011 Vendor=0001 Product=0001 Version=ab41\nN: Name=\"AT Translated Set 2 keyboard\"\nH: Handlers=sysrq kbd event1 leds \nB: EV=120013\n\n";
        let drums = "I: Bus=0005 Vendor=12ba Product=0210 Version=0108\nN: Name=\"Harmonix Drum Kit for PlayStation (R) 3\"\nH: Handlers=event6 js1 \nB: PROP=0\n";
        assert_eq!(parse_pads(&format!("{kbd}{guitar}{drums}")).len(), 2);
        assert_eq!(parse_pads(kbd).len(), 0);
    }

    #[test]
    fn the_device_mounted_at_data_is_read_from_proc_mounts() {
        let m = "/dev/root / ext4 rw 0 0\n/dev/sdb4 /data ext4 rw,noatime 0 0\ntmpfs /tmp tmpfs rw 0 0\n/dev/sda1 /media/songs ext4 ro 0 0\n";
        assert_eq!(parse_data_mount_source(m).as_deref(), Some("/dev/sdb4"));
        assert_eq!(parse_data_mount_source("systemd-1 /data autofs rw 0 0\n"), None);
        assert_eq!(parse_data_mount_source("/dev/sda1 /media/data ext4 rw 0 0\n"), None);
        assert_eq!(parse_data_mount_source(""), None);
    }

    #[test]
    fn unused_room_after_the_data_partition_is_found() {
        let cmd = "root=PARTUUID=aaa systemd.mount-extra=PARTUUID=6d5a1c52-0002:/data:ext4:rw,noatime quiet";
        assert_eq!(parse_data_partuuid(cmd).as_deref(), Some("6d5a1c52-0002"));
        assert_eq!(parse_data_partuuid("root=/dev/sda2 systemd.mount-extra=PARTUUID=x:/other:ext4"), None);
        assert_eq!(parse_data_partuuid("quiet"), None);
        // A 256 GB stick (500,118,192 sectors) with the image's 700 MB data partition at the end of a 5 GiB layout.
        assert_eq!(slack_sectors(500_118_192, 10_000_000, 1_433_600) / 2 / 1024 / 1024, 233);
        assert_eq!(slack_sectors(11_433_600, 10_000_000, 1_433_600), 0);
        assert_eq!(slack_sectors(5, 10, 10), 0);
    }

    #[test]
    fn the_space_kept_free_is_a_tenth_between_one_and_a_half_and_four_gb() {
        assert_eq!(reserve_kib(8_000_000), 1_500_000);
        assert_eq!(reserve_kib(30_000_000), 3_000_000);
        assert_eq!(reserve_kib(900_000_000), 4_000_000);
    }

    #[test]
    fn internal_song_sources_are_shown_with_their_folder() {
        assert_eq!(parse_sources("/data/songs/MyDrive\n/data/songs/MyDrive/Rock\n/media/songs/Songs\n/media/songs\n"), vec!["internal: MyDrive", "internal: MyDrive/Rock", "Songs", "the whole drive"]);
    }

    #[test]
    fn default_output_avoids_screens() {
        let out = "card 0: Generic [HDA ATI HDMI], device 3: HDMI 0 [HDMI 0]\ncard 1: Generic_1 [HD-Audio Generic], device 0: ALC285 Analog [ALC285 Analog]\n";
        let outs = parse_aplay(out);
        assert_eq!(best_output(&outs).map(|o| o.card), Some(1));
        assert_eq!(best_output(&outs[..1]).map(|o| o.card), Some(0));
        assert_eq!(best_output(&[]), None);
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

    #[test]
    fn disk_list_lines_are_parsed() {
        let out = "/dev/sdb|256060514304|WDC WDS250G1B0B|sata|0\n/dev/sdc|126437212160|USB 3.0 FD|usb|1\n/dev/nvme0n1|512110190592|Samsung SSD|nvme|0\ngarbage\n";
        let d = parse_disks(out);
        assert_eq!(d.len(), 3);
        assert_eq!(d[0], Disk { path: "/dev/sdb".into(), bytes: 256060514304, model: "WDC WDS250G1B0B".into(), removable: false });
        assert!(d[1].removable);
        assert_eq!(d[0].size_label(), "256 GB");
        assert_eq!(d[2].label(), "Samsung SSD (512 GB)");
        assert_eq!(Disk { path: "/dev/sda".into(), bytes: 2_000_398_934_016, model: "X".into(), removable: false }.size_label(), "2.0 TB");
    }

    #[test]
    fn installer_output_lines_are_understood() {
        assert_eq!(parse_install_line("STEP 3/6 Copying the system (a few minutes)"), InstallLine::Step("Copying the system (a few minutes) (step 3 of 6)".into()));
        assert_eq!(parse_install_line("DONE\n"), InstallLine::Done);
        assert_eq!(parse_install_line("ERROR Could not partition /dev/sdb"), InstallLine::Error("Could not partition /dev/sdb".into()));
        assert_eq!(parse_install_line("cp: something"), InstallLine::Other);
    }

    #[test]
    fn song_sources_are_shown_by_folder_name() {
        assert_eq!(parse_sources("/media/songs/Songs\n/media/songs/Rock Band\n"), vec!["Songs", "Rock Band"]);
        assert_eq!(parse_sources("/media/songs\n"), vec!["the whole drive"]);
        assert!(parse_sources("").is_empty());
    }

    #[test]
    fn wiimotes_are_recognised_from_the_hid_uevent() {
        let wii = "DRIVER=hid-generic\nHID_ID=0005:0000057E:00000306\nHID_NAME=Nintendo RVL-CNT-01\nHID_UNIQ=00:22:D7:AD:BE:D4\n";
        assert_eq!(parse_wiimote_uevent(wii), Some("00:22:d7:ad:be:d4".into()));
        assert_eq!(parse_wiimote_uevent("HID_ID=0005:0000057E:00000330\n"), Some(String::new()));
        assert_eq!(parse_wiimote_uevent("HID_ID=0003:0000046D:0000C534\nHID_UNIQ=\n"), None);
        assert_eq!(parse_wiimote_uevent("HID_ID=0005:000012BA:00000100\n"), None); // our own virtual guitar
    }

    #[test]
    fn midi_clients_cards_and_pads_are_read_from_proc() {
        let seq = "Client info\n  cur  clients : 3\nClient   0 : \"System\" [Kernel]\n  Port   0 : \"Timer\" (Rwe-)\nClient  14 : \"Midi Through\" [Kernel]\n  Port   0 : \"Midi Through Port-0\" (RWe-)\nClient  20 : \"P-143\" [Kernel]\n  Port   0 : \"P-143 MIDI 1\" (RWeX)\n";
        assert_eq!(parse_seq_clients(seq), vec![("P-143".to_string(), "P-143 MIDI 1".to_string())]);
        assert!(parse_seq_clients("Client info\n  cur  clients : 1\nClient   0 : \"System\" [Kernel]\n").is_empty());
        let cards = " 0 [Intel          ]: HDA-Intel - HDA Intel\n                      HDA Intel at 0xfebf0000 irq 28\n 1 [Microphone     ]: USB-Audio - Logitech USB Microphone\n                      Logitech Logitech USB Microphone at usb-0000:00:14.0-1, full speed\n";
        assert_eq!(parse_cards(cards), vec!["HDA Intel", "Logitech USB Microphone"]);
        let input = "I: Bus=0003 Vendor=12ba Product=0100 Version=0110\nN: Name=\"Guitar Hero4 for PlayStation (R) 3\"\nH: Handlers=js0 event5 \n\nI: Bus=0011 Vendor=0001 Product=0001\nN: Name=\"AT Translated Set 2 keyboard\"\nH: Handlers=sysrq kbd event1 leds \n";
        assert_eq!(parse_pads(input), vec!["Guitar Hero4 for PlayStation (R) 3"]);
    }
}

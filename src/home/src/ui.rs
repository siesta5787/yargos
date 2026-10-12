// Drawing for every dashboard screen. Layout is in units of u = height / 720 so it suits any resolution.

use crate::gfx::{rgb, Canvas, Fonts, Rgb};
use crate::logo::{draw_logo, GOLD};
use crate::model::{human_kib, Key, Model, Power, Screen, SongKind, HOME_ITEMS};

const TEXT: Rgb = rgb(232, 236, 244);
const DIM: Rgb = rgb(160, 172, 190);
const GREEN: Rgb = rgb(23, 230, 140);
const BLUE: Rgb = rgb(90, 150, 220);
const AMBER: Rgb = rgb(245, 175, 50);

/// A clickable area and the list row it stands for.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub row: usize,
}

pub fn hit_test(hits: &[Hit], x: i32, y: i32) -> Option<usize> {
    hits.iter().find(|h| x >= h.x && x < h.x + h.w && y >= h.y && y < h.y + h.h).map(|h| h.row)
}

enum Kind {
    Plain,
    Signal(u8),
    Slider(u8),
    Radio(bool),
}

struct Row {
    label: String,
    value: String,
    tag: Option<(&'static str, Rgb)>,
    enabled: bool,
    kind: Kind,
}

fn row(label: impl Into<String>) -> Row {
    Row { label: label.into(), value: String::new(), tag: None, enabled: true, kind: Kind::Plain }
}

fn rows(m: &Model) -> Vec<Row> {
    match m.screen {
        Screen::Wifi => {
            let mut v = vec![Row { value: if m.wifi_dev.is_some() { String::new() } else { "no adapter".into() }, ..row("Scan again") }];
            for n in &m.nets {
                let mut r = row(n.name.clone());
                r.kind = Kind::Signal(n.signal);
                r.value = if n.secured { "secured".into() } else { "open".into() };
                r.tag = if n.connected { Some(("CONNECTED", GREEN)) } else if n.known { Some(("SAVED", BLUE)) } else { None };
                v.push(r);
            }
            v
        }
        Screen::Sound => {
            let mut v: Vec<Row> = m
                .outputs
                .iter()
                .map(|o| {
                    let mut r = row(o.label.clone());
                    r.kind = Kind::Radio(m.cur_card_dev == Some((o.card, o.dev)));
                    r
                })
                .collect();
            if let Some((_, pct)) = m.volume {
                let mut r = row("Volume");
                r.kind = Kind::Slider(pct);
                v.push(r);
            }
            v.push(row("Play a test sound"));
            v
        }
        Screen::Bluetooth => {
            let mut v = vec![row("Scan for devices")];
            for d in &m.bt {
                let mut r = row(if d.name.is_empty() { d.mac.clone() } else { d.name.clone() });
                r.tag = if d.connected && d.is_wiimote() {
                    Some(if d.ready { ("READY", GREEN) } else { ("NOT READY", AMBER) })
                } else if d.connected {
                    Some(("CONNECTED", GREEN))
                } else if d.paired {
                    Some(("PAIRED", BLUE))
                } else {
                    None
                };
                v.push(r);
            }
            v
        }
        Screen::Updates => vec![
            row("Check for updates"),
            Row { enabled: m.upd.available, value: if m.upd.available { m.upd.latest.clone().unwrap_or_default() } else { String::new() }, ..row("Install the update") },
            Row { value: if m.upd.auto { "ON".into() } else { "OFF".into() }, ..row("Automatic updates") },
            Row { enabled: m.upd.previous.is_some(), value: m.upd.previous.clone().unwrap_or_default(), ..row("Go back to the previous version") },
        ],
        Screen::Songs => {
            let mut v = vec![row("Look for the song drive again")];
            if m.songs.mounted {
                let mut r = row(m.songs.name.clone());
                r.value = m.songs.space.clone();
                r.tag = Some(("CONNECTED", GREEN));
                v.push(r);
            }
            v.push(Row { value: if m.songs.auto { "ON".into() } else { "OFF".into() }, ..row("Add song folders to YARG") });
            v
        }
        Screen::System => vec![
            row("Restart"),
            row("Shut down"),
            row("Install on this computer's disk"),
            row("Connected devices"),
            Row { tag: m.os.staged.as_ref().map(|_| ("READY", GREEN)).or(m.os.available.as_ref().map(|_| ("NEW", AMBER))), ..row("System update") },
            row("Storage"),
        ],
        Screen::Storage => vec![
            row("Import from a portable drive"),
            Row { value: if m.storage.used_kib > 0 { human_kib(m.storage.used_kib) } else { String::new() }, ..row("Manage internal songs") },
        ],
        Screen::ImportPick => {
            let mut v = vec![row("Look for drives again")];
            for d in &m.drives {
                let mut r = row(d.name());
                r.value = human_kib(d.bytes / 1024);
                r.tag = Some((fs_tag(&d.fs), BLUE));
                v.push(r);
            }
            v
        }
        Screen::Manage => {
            let top = m.browse.path.is_empty();
            let mut v = vec![Row { enabled: !(top && m.browse.entries.is_empty()), ..row(if top { "Delete ALL internal songs" } else { "Delete this whole folder" }) }];
            for e in &m.browse.entries {
                let mut r = row(e.name.clone());
                r.tag = Some(match e.kind {
                    SongKind::Folder => ("FOLDER", BLUE),
                    SongKind::Song => ("SONG", GREEN),
                    SongKind::File => ("FILE", AMBER),
                });
                v.push(r);
            }
            v
        }
        Screen::OsUpdate => vec![
            row("Check for a system update"),
            Row { enabled: m.os.available.is_some(), value: m.os.available.clone().unwrap_or_default(), ..row("Install the system update") },
            Row { enabled: m.os.staged.is_some(), value: m.os.staged.clone().unwrap_or_default(), ..row("Restart to use the update") },
            Row { value: if m.os.auto { "ON".into() } else { "OFF".into() }, ..row("Automatic system updates") },
            Row { enabled: m.os.previous.is_some(), value: m.os.previous.clone().unwrap_or_default(), ..row("Go back to the earlier system") },
        ],
        Screen::Devices => {
            let mut v = vec![row("Look again")];
            for d in &m.devices {
                let mut r = row(d.name.clone());
                r.value = d.detail.clone();
                r.tag = Some((d.kind, match d.kind { "MIDI" => GREEN, "USB" | "KEYBOARD" | "MOUSE" => BLUE, "SOUND" => AMBER, _ => GREEN }));
                v.push(r);
            }
            v
        }
        Screen::Yarg => {
            let zip = m.yarg_zips.first().map(|z| z.rsplit('/').next().unwrap_or(z).to_string());
            vec![
                Row { value: "from GitHub".into(), ..row("Download YARG") },
                Row { enabled: zip.is_some(), value: if zip.is_some() { "found".into() } else { "no file".into() }, ..row("Install from the song drive") },
                row("Set up Wi-Fi first"),
            ]
        }
        Screen::Install if m.installed.is_some() => vec![row("Restart"), row("Shut down")],
        Screen::Install => {
            let mut v = vec![row("Look for disks again")];
            for d in &m.disks {
                let mut r = row(d.model.clone());
                r.value = d.size_label();
                r.tag = Some(if d.removable { ("USB", BLUE) } else { ("INTERNAL", GREEN) });
                v.push(r);
            }
            v
        }
        _ => vec![],
    }
}

fn fs_tag(fs: &str) -> &'static str {
    match fs {
        "EXFAT" => "EXFAT",
        "NTFS" => "NTFS",
        "FAT" => "FAT",
        "EXT4" => "EXT4",
        "EXT3" => "EXT3",
        "EXT2" => "EXT2",
        _ => "DRIVE",
    }
}

fn panel(m: &Model) -> (&'static str, Vec<String>) {
    match m.screen {
        Screen::Wifi => (
            "WI-FI",
            if m.wifi_dev.is_none() && !m.loading {
                vec!["No Wi-Fi adapter found.".into(), "Plug in a USB Wi-Fi adapter, or use the Ethernet port.".into()]
            } else {
                vec![
                    m.status.network.clone(),
                    "Pick a network. Saved networks reconnect without a password.".into(),
                    "Typing works with a keyboard, or use the on-screen keys with the strum bar and frets.".into(),
                ]
            },
        ),
        Screen::Sound => (
            "SOUND",
            vec![
                "Pick where YARG plays its sound: your TV or receiver over HDMI, the headphone jack, or a USB audio device.".into(),
                "Left / Right changes the volume. HDMI volume is set on your TV.".into(),
            ],
        ),
        Screen::Bluetooth if !m.bt_present && !m.loading => (
            "BLUETOOTH",
            vec!["No Bluetooth adapter found.".into(), "Plug in a USB Bluetooth adapter to connect Wii guitars, controllers, keyboards and mice.".into()],
        ),
        Screen::Bluetooth => (
            "BLUETOOTH",
            vec![
                "Connect Wii guitars, controllers, keyboards and mice.".into(),
                "Wii guitar: hold 1+2 on the Wiimote until the lights flash. It connects by itself within a few seconds.".into(),
                "Left forgets a paired device.".into(),
            ],
        ),
        Screen::Updates => {
            let mut v = vec![
                format!("Installed: {}", if m.upd.installed.is_empty() { "?" } else { &m.upd.installed }),
                format!("Previous: {}", m.upd.previous.as_deref().unwrap_or("none")),
                format!("Latest known: {}", m.upd.latest.as_deref().unwrap_or("not checked")),
            ];
            if m.upd.available {
                v.push("An update is ready to install.".into());
            }
            v.extend(m.upd.log.iter().cloned());
            ("UPDATES", v)
        }
        Screen::Songs => ("SONGS", {
            let mut v = vec![m.songs.detail.clone()];
            if m.songs.mounted {
                v.push(if !m.songs.auto {
                    "Automatic is OFF: you manage YARG's song folders yourself, in YARG's settings.".to_string()
                } else if m.songs.sources.is_empty() {
                    "Each folder on the drive that has songs becomes its own source in YARG.".to_string()
                } else {
                    format!("Becomes a source in YARG each time it starts: {}.", m.songs.sources.join(", "))
                });
            }
            v.push("Name your song drive YARGSONGS (ext4, exFAT, NTFS or FAT).".into());
            v
        }),
        Screen::System => ("SYSTEM", m.sysinfo.clone()),
        Screen::Storage => {
            let s = &m.storage;
            let mut v = vec!["Songs kept on the drive YARG OS runs from. YARG scans them each time it starts, like the song drive.".to_string()];
            if s.total_kib > 0 {
                v.push(format!(
                    "Internal songs: {} in {} folder{}. Free: {} of {}.",
                    human_kib(s.used_kib),
                    s.folders,
                    if s.folders == 1 { "" } else { "s" },
                    human_kib(s.free_kib),
                    human_kib(s.total_kib)
                ));
                v.push(format!("{} is always kept free for YARG and for system updates.", human_kib(s.reserve_kib)));
            }
            v.push("Import copies a portable drive as it is. Nothing on that drive is changed.".into());
            ("INTERNAL DRIVE", v)
        }
        Screen::ImportPick => {
            let mut v = vec!["Pick a portable drive. Everything on it is copied, folders exactly as they are. A stopped copy carries on where it left off.".to_string()];
            if m.drives.is_empty() && !m.loading {
                v.push("No portable drive found. Plug one in, then Look for drives again.".into());
            }
            if m.storage.total_kib > 0 {
                v.push(format!("Room for songs: {}.", human_kib(m.storage.free_kib.saturating_sub(m.storage.reserve_kib))));
            }
            ("IMPORT SONGS", v)
        }
        Screen::Manage => {
            let mut where_ = vec!["Internal songs".to_string()];
            where_.extend(m.browse.path.iter().cloned());
            let mut v = vec![where_.join(" / ")];
            if m.browse.entries.is_empty() && !m.loading {
                v.push(if m.browse.path.is_empty() { "No internal songs yet. Use Import from a portable drive.".into() } else { "This folder is empty.".to_string() });
            }
            v.push("OK opens a folder (or asks to delete a song). LEFT deletes the highlighted song or folder: it always asks first. BACK goes up one level.".into());
            v.push(if m.browse.path.is_empty() { "The top row deletes everything in the internal library. Songs on portable drives are never touched.".into() } else { "The top row deletes this whole folder.".to_string() });
            ("INTERNAL SONGS", v)
        }
        Screen::OsUpdate => {
            let mut v = vec![format!("Running: {}", if m.os.running.is_empty() { "?" } else { &m.os.running })];
            v.push(format!("Latest known: {}", m.os.latest.as_deref().unwrap_or("not checked")));
            if let Some(s) = &m.os.staged {
                v.push(format!("{s} is installed. Restart to use it."));
            } else if let Some(a) = &m.os.available {
                v.push(format!("{a} is available."));
            }
            if !m.os.notes.trim().is_empty() {
                v.push(m.os.notes.trim().to_string());
            }
            match m.os.error.as_deref() {
                Some("offline") => v.push("The update server could not be reached. Check Wi-Fi and try again.".into()),
                Some("signature") => v.push("The update information was not signed by us, so it was ignored.".into()),
                _ => {}
            }
            v.push("It never restarts without asking. A new system that does not start is replaced by the earlier one by itself.".into());
            ("SYSTEM UPDATE", v)
        }
        Screen::Devices => (
            "DEVICES",
            vec![
                "What is plugged in and usable right now. A plain USB row means the system could not make anything of that device.".to_string(),
                "Wii guitars and Xbox drums show as PlayStation instruments: that is what YARG lists.".to_string(),
            ],
        ),
        Screen::Install if m.installed.is_some() => (
            "INSTALLED",
            vec![
                format!("YARG OS is installed on {}.", m.installed.clone().unwrap_or_default()),
                "Choose Restart, and unplug the USB drive when the screen goes dark. From now on the computer starts YARG OS from its own disk.".into(),
                "Your YARG, its settings and your paired Bluetooth devices were copied across.".into(),
            ],
        ),
        Screen::Yarg => {
            let mut v = vec![
                "YARG is the free game this system runs. It is not part of YARG OS: it is downloaded from the official YARG release on GitHub (YARC-Official/YARG), about 120 MB.".to_string(),
                format!("Network: {}", if m.status.network.is_empty() { "checking\u{2026}" } else { &m.status.network }),
            ];
            if m.status.network == "not connected" {
                v.push("Not connected: choose Set up Wi-Fi first (or plug in a network cable).".into());
            }
            v.push(match m.yarg_zips.first() {
                Some(z) => format!("No internet? A YARG zip was found on the song drive: {}.", z.rsplit('/').next().unwrap_or(z)),
                None => "No internet? Put the YARG Linux zip (YARG_v...-Linux-x86_64.zip) on the song drive and look again.".into(),
            });
            ("INSTALL YARG", v)
        }
        Screen::Install => {
            let mut v = vec![
                "Install YARG OS on this computer's own disk, so it starts without the USB drive.".to_string(),
                "Everything on the disk you choose is erased.".to_string(),
            ];
            if m.disks.is_empty() && !m.loading {
                v.push("No other disk was found. The disk must be at least 8 GB and not be in use.".into());
            }
            if let Some(e) = &m.install_error {
                v.push(format!("The install did not finish: {e}"));
            }
            ("INSTALL", v)
        }
        _ => ("", vec![]),
    }
}

fn title_of(s: Screen) -> &'static str {
    match s {
        Screen::Home => "",
        Screen::Wifi | Screen::Password => "WI-FI",
        Screen::Sound => "SOUND",
        Screen::Bluetooth => "BLUETOOTH",
        Screen::Updates => "UPDATES",
        Screen::Songs => "SONGS",
        Screen::System => "SYSTEM",
        Screen::Devices => "DEVICES",
        Screen::Install => "INSTALL",
        Screen::Yarg => "INSTALL YARG",
        Screen::OsUpdate => "SYSTEM UPDATE",
        Screen::Storage => "STORAGE",
        Screen::ImportPick => "IMPORT SONGS",
        Screen::Manage => "INTERNAL SONGS",
    }
}

fn wrap(fonts: &mut Fonts, text: &str, px: f32, max: f32) -> Vec<String> {
    let mut lines = vec![];
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let trial = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if fonts.width(&trial, px, false) > max && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = trial;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

pub fn draw(cv: &mut Canvas, fonts: &mut Fonts, m: &Model, hits: &mut Vec<Hit>) {
    hits.clear();
    cv.backdrop();
    let u = cv.h as f32 / 720.0;
    let w = cv.w as f32;

    if m.screen == Screen::Home {
        draw_home(cv, fonts, m, u, hits);
    } else if m.screen == Screen::Password {
        draw_password(cv, fonts, m, u, hits);
    } else {
        draw_list_screen(cv, fonts, m, u, hits);
    }

    // Controls hint.
    let hint = match m.screen {
        Screen::Password => "Arrows: move    OK: type the key    BACK: cancel    A keyboard works too",
        Screen::Manage => "\u{25B2}\u{25BC} Move     OK Open     \u{25C0} Delete     BACK Up",
        _ => "\u{25B2}\u{25BC} Move     OK Select     BACK Return",
    };
    fonts.draw_centered(cv, hint, w / 2.0, cv.h as f32 - 50.0 * u, 18.0 * u, false, DIM, 0.8);

    if let Some(msg) = m.live_toast() {
        let tw = fonts.width(msg, 22.0 * u, false) + 56.0 * u;
        let (x, y) = ((w - tw) / 2.0, cv.h as f32 - 118.0 * u);
        cv.rrect(x as i32, y as i32, tw as i32, (50.0 * u) as i32, 25.0 * u, rgb(10, 16, 30), 0.92);
        fonts.draw_centered(cv, msg, w / 2.0, y + 12.0 * u, 22.0 * u, false, TEXT, 1.0);
    }
    if let Some(p) = m.confirm {
        hits.clear();
        let (q, go) = if p == Power::Shutdown { ("Shut down now?", "Shut down") } else { ("Restart now?", "Restart") };
        draw_confirm(cv, fonts, m, q, "", go, true, u, hits);
    }
    if let Some(a) = &m.ask {
        hits.clear();
        draw_confirm(cv, fonts, m, &a.question, &a.note, &a.go, a.danger, u, hits);
    }
    if let Some(d) = m.erase.and_then(|i| m.disks.get(i)) {
        hits.clear();
        draw_confirm(cv, fonts, m, "Erase this disk and install?", &format!("{}\nEverything on it will be lost.", d.label()), "Erase and install", true, u, hits);
    }
    if let Some(msg) = &m.busy {
        hits.clear();
        draw_busy(cv, fonts, msg, m.tick, m.cancellable(), u);
    }
    if let Some(msg) = &m.alert {
        hits.clear();
        draw_alert(cv, fonts, msg, u, hits);
    }
}

fn header(cv: &mut Canvas, fonts: &mut Fonts, title: &str, u: f32) {
    draw_logo(cv, fonts, 64.0 * u + 150.0 * u, 70.0 * u, 40.0 * u);
    if !title.is_empty() {
        fonts.draw(cv, title, 64.0 * u, 118.0 * u, 34.0 * u, true, GOLD, 1.0);
    }
}

fn draw_home(cv: &mut Canvas, fonts: &mut Fonts, m: &Model, u: f32, hits: &mut Vec<Hit>) {
    header(cv, fonts, "", u);
    let (x, y0, step) = (96.0 * u, 168.0 * u, 62.0 * u);
    for i in 0..HOME_ITEMS.len() {
        let label = m.home_label(i);
        let y = y0 + i as f32 * step;
        let sel = i == m.sel;
        if sel {
            cv.rect((x - 34.0 * u) as i32, (y + 4.0 * u) as i32, (12.0 * u) as i32, (46.0 * u) as i32, GOLD, 1.0);
        }
        let color = if sel { rgb(255, 255, 255) } else { rgb(190, 198, 212) };
        fonts.draw(cv, label, x, y, 46.0 * u, true, color, if sel { 1.0 } else { 0.72 });
        hits.push(Hit { x: (x - 40.0 * u) as i32, y: y as i32, w: (460.0 * u) as i32, h: step as i32, row: i });
    }

    // Status card, sized to its content.
    let (cx, cy, cw) = (cv.w as f32 * 0.58, 168.0 * u, cv.w as f32 * 0.36);
    let st = &m.status;
    let mut items: Vec<(&str, String)> = vec![
        ("NETWORK", st.network.clone()),
        ("SONG DRIVE", st.songs.clone()),
        ("SOUND", st.sound.clone()),
        ("YARG", st.yarg.clone()),
        ("CONTROLLERS", if st.pads == 0 { "none connected".into() } else { format!("{} connected", st.pads) }),
    ];
    if let Some(v) = &st.update {
        items.push(("UPDATE", v.clone()));
    }
    let wrapped: Vec<(&str, Vec<String>)> = items
        .into_iter()
        .map(|(k, v)| {
            let v = if v.is_empty() { "\u{2026}".to_string() } else { v };
            (k, wrap(fonts, &v, 22.0 * u, cw - 56.0 * u).into_iter().take(2).collect())
        })
        .collect();
    let ch = 70.0 * u + wrapped.iter().map(|(_, l)| 38.0 * u + l.len() as f32 * 26.0 * u).sum::<f32>();
    cv.rrect(cx as i32, cy as i32, cw as i32, ch as i32, 18.0 * u, rgb(0, 0, 0), 0.34);
    fonts.draw(cv, "STATUS", cx + 28.0 * u, cy + 22.0 * u, 20.0 * u, true, GOLD, 1.0);
    let mut y = cy + 66.0 * u;
    for (k, lines) in &wrapped {
        fonts.draw(cv, k, cx + 28.0 * u, y, 15.0 * u, true, DIM, 0.9);
        for (j, line) in lines.iter().enumerate() {
            fonts.draw(cv, line, cx + 28.0 * u, y + 20.0 * u + j as f32 * 26.0 * u, 22.0 * u, false, TEXT, 1.0);
        }
        y += 38.0 * u + lines.len() as f32 * 26.0 * u;
    }
}

fn draw_list_screen(cv: &mut Canvas, fonts: &mut Fonts, m: &Model, u: f32, hits: &mut Vec<Hit>) {
    header(cv, fonts, title_of(m.screen), u);
    let rs = rows(m);
    let (lx, top, lw) = (64.0 * u, 172.0 * u, cv.w as f32 * 0.52);
    let (rh, gap) = (58.0 * u, 8.0 * u);
    let visible = (((cv.h as f32 - 92.0 * u - top) / (rh + gap)) as usize).max(1);
    let first = if m.sel >= visible { m.sel + 1 - visible } else { 0 };
    for (i, r) in rs.iter().enumerate().skip(first).take(visible) {
        let y = top + (i - first) as f32 * (rh + gap);
        let sel = i == m.sel;
        cv.rrect(lx as i32, y as i32, lw as i32, rh as i32, 14.0 * u, rgb(255, 255, 255), if sel { 0.15 } else { 0.06 });
        if sel {
            cv.rect(lx as i32, (y + 10.0 * u) as i32, (7.0 * u) as i32, (rh - 20.0 * u) as i32, GOLD, 1.0);
        }
        let a = if r.enabled { 1.0 } else { 0.4 };
        let mut tx = lx + 26.0 * u;
        if let Kind::Radio(on) = r.kind {
            cv.circle(tx + 9.0 * u, y + rh / 2.0, 10.0 * u, rgb(255, 255, 255), 0.35 * a);
            if on {
                cv.circle(tx + 9.0 * u, y + rh / 2.0, 6.0 * u, GOLD, 1.0);
            }
            tx += 36.0 * u;
        }
        let mut reserve = 36.0 * u;
        reserve += match r.kind { Kind::Signal(_) => 70.0 * u, Kind::Slider(_) => 330.0 * u, _ => 0.0 };
        if !r.value.is_empty() {
            reserve += fonts.width(&r.value, 20.0 * u, false) + 16.0 * u;
        }
        if let Some((t, _)) = r.tag {
            reserve += fonts.width(t, 14.0 * u, true) + 40.0 * u;
        }
        let label = fit_bold(fonts, &r.label, 26.0 * u, lw - (tx - lx) - reserve, sel);
        fonts.draw(cv, &label, tx, y + (rh - 26.0 * u) / 2.0 - 3.0 * u, 26.0 * u, sel, if sel { rgb(255, 255, 255) } else { TEXT }, a);
        let mut rx = lx + lw - 24.0 * u;
        match r.kind {
            Kind::Signal(s) => {
                for b in 0..4 {
                    let bh = (8.0 + b as f32 * 6.0) * u;
                    let on = (b as u8) < s;
                    cv.rect((rx - (4 - b) as f32 * 11.0 * u) as i32, (y + rh / 2.0 + 12.0 * u - bh) as i32, (7.0 * u) as i32, bh as i32, rgb(255, 255, 255), if on { 0.9 } else { 0.2 });
                }
                rx -= 56.0 * u;
            }
            Kind::Slider(p) => {
                let (bw, bh) = (200.0 * u, 10.0 * u);
                let bx = rx - bw - 60.0 * u;
                cv.rrect(bx as i32, (y + rh / 2.0 - bh / 2.0) as i32, bw as i32, bh as i32, bh / 2.0, rgb(255, 255, 255), 0.18);
                cv.rrect(bx as i32, (y + rh / 2.0 - bh / 2.0) as i32, ((bw * p as f32 / 100.0).max(bh)) as i32, bh as i32, bh / 2.0, GOLD, 1.0);
                let t = format!("{p}%");
                let tw = fonts.width(&t, 20.0 * u, false);
                fonts.draw(cv, &t, rx - tw, y + (rh - 20.0 * u) / 2.0 - 2.0 * u, 20.0 * u, false, TEXT, 1.0);
                rx = bx;
            }
            _ => {}
        }
        if !r.value.is_empty() {
            let vw = fonts.width(&r.value, 20.0 * u, false);
            fonts.draw(cv, &r.value, rx - vw, y + (rh - 20.0 * u) / 2.0 - 2.0 * u, 20.0 * u, false, DIM, a);
            rx -= vw + 16.0 * u;
        }
        if let Some((t, c)) = r.tag {
            let tw = fonts.width(t, 14.0 * u, true) + 22.0 * u;
            cv.rrect((rx - tw) as i32, (y + rh / 2.0 - 12.0 * u) as i32, tw as i32, (24.0 * u) as i32, 12.0 * u, c, 0.22);
            fonts.draw(cv, t, rx - tw + 11.0 * u, y + rh / 2.0 - 9.0 * u, 14.0 * u, true, c, 1.0);
        }
        hits.push(Hit { x: lx as i32, y: y as i32, w: lw as i32, h: rh as i32, row: i });
    }
    if first > 0 {
        fonts.draw_centered(cv, "\u{25B2}", lx + lw / 2.0, top - 26.0 * u, 18.0 * u, false, DIM, 0.8);
    }
    if first + visible < rs.len() {
        fonts.draw_centered(cv, "\u{25BC}", lx + lw / 2.0, top + visible as f32 * (rh + gap), 18.0 * u, false, DIM, 0.8);
    }

    // Side panel.
    let (pt, lines) = panel(m);
    let (px, pw) = (lx + lw + 40.0 * u, cv.w as f32 - (lx + lw + 40.0 * u) - 64.0 * u);
    cv.rrect(px as i32, top as i32, pw as i32, (visible as f32 * (rh + gap) - gap) as i32, 18.0 * u, rgb(0, 0, 0), 0.30);
    fonts.draw(cv, pt, px + 26.0 * u, top + 20.0 * u, 18.0 * u, true, GOLD, 1.0);
    let mut y = top + 58.0 * u;
    if m.loading {
        lines_draw(cv, fonts, &["Loading…".to_string()], px, pw, &mut y, u);
    } else {
        lines_draw(cv, fonts, &lines, px, pw, &mut y, u);
    }
}

fn lines_draw(cv: &mut Canvas, fonts: &mut Fonts, lines: &[String], px: f32, pw: f32, y: &mut f32, u: f32) {
    for line in lines {
        for l in wrap(fonts, line, 20.0 * u, pw - 52.0 * u) {
            if *y > cv.h as f32 - 190.0 * u {
                return;
            }
            fonts.draw(cv, &l, px + 26.0 * u, *y, 20.0 * u, false, TEXT, 0.95);
            *y += 27.0 * u;
        }
        *y += 10.0 * u;
    }
}

/// Like `fit`, measuring in the weight the text will be drawn in (the selected row is bold and wider).
fn fit_bold(fonts: &mut Fonts, text: &str, px: f32, max: f32, bold: bool) -> String {
    let width = |f: &mut Fonts, t: &str| f.width(t, px, bold);
    if width(fonts, text) <= max {
        return text.to_string();
    }
    let mut s: String = text.to_string();
    while !s.is_empty() && width(fonts, &format!("{s}\u{2026}")) > max {
        s.pop();
    }
    format!("{s}\u{2026}")
}

fn fit(fonts: &mut Fonts, text: &str, px: f32, max: f32) -> String {
    if fonts.width(text, px, false) <= max {
        return text.to_string();
    }
    let mut s: String = text.to_string();
    while !s.is_empty() && fonts.width(&format!("{s}…"), px, false) > max {
        s.pop();
    }
    format!("{s}…")
}

fn draw_password(cv: &mut Canvas, fonts: &mut Fonts, m: &Model, u: f32, hits: &mut Vec<Hit>) {
    header(cv, fonts, "WI-FI", u);
    let w = cv.w as f32;
    fonts.draw_centered(cv, &format!("Password for {}", m.osk.ssid), w / 2.0, 156.0 * u, 30.0 * u, true, TEXT, 1.0);
    let (bw, bh) = (620.0 * u, 58.0 * u);
    let bx = (w - bw) / 2.0;
    cv.rrect(bx as i32, (200.0 * u) as i32, bw as i32, bh as i32, 14.0 * u, rgb(0, 0, 0), 0.45);
    let shown = if m.osk.show { m.osk.text.clone() } else { "\u{25CF}".repeat(m.osk.text.chars().count()) };
    let blink = if (m.tick / 5) % 2 == 0 { "|" } else { "" };
    fonts.draw(cv, &format!("{shown}{blink}"), bx + 20.0 * u, 200.0 * u + 12.0 * u, 28.0 * u, false, TEXT, 1.0);

    let rows = m.osk.rows();
    let (kw, kh, kg) = (62.0 * u, 58.0 * u, 8.0 * u);
    let mut y = 290.0 * u;
    for (ri, r) in rows.iter().enumerate() {
        let special = ri == rows.len() - 1;
        let widths: Vec<f32> = r
            .iter()
            .map(|k| if special { match k { Key::Space => 3.0 * kw, Key::Done => 1.8 * kw, _ => 1.5 * kw } } else { kw })
            .collect();
        let total: f32 = widths.iter().sum::<f32>() + kg * (r.len() as f32 - 1.0);
        let mut x = (w - total) / 2.0;
        for (ci, k) in r.iter().enumerate() {
            let sel = ri == m.osk.row && ci == m.osk.col.min(r.len() - 1);
            let label = match k {
                Key::Ch(c) => c.to_string(),
                Key::Shift => ["abc", "ABC", "#+="][m.osk.layer].to_string(),
                Key::Space => "space".into(),
                Key::Back => "\u{232B}".into(),
                Key::Show => if m.osk.show { "hide".into() } else { "show".into() },
                Key::Done => "OK".into(),
            };
            let fill = if *k == Key::Done { GREEN } else { rgb(255, 255, 255) };
            cv.rrect(x as i32, y as i32, widths[ci] as i32, kh as i32, 10.0 * u, fill, if sel { 0.95 } else if *k == Key::Done { 0.35 } else { 0.10 });
            let color = if sel { rgb(10, 16, 30) } else { TEXT };
            fonts.draw_centered(cv, &label, x + widths[ci] / 2.0, y + (kh - 26.0 * u) / 2.0 - 2.0 * u, 26.0 * u, true, color, 1.0);
            hits.push(Hit { x: x as i32, y: y as i32, w: widths[ci] as i32, h: kh as i32, row: ri * 16 + ci });
            x += widths[ci] + kg;
        }
        y += kh + kg;
    }
}

fn draw_confirm(cv: &mut Canvas, fonts: &mut Fonts, m: &Model, question: &str, note: &str, go: &str, danger: bool, u: f32, hits: &mut Vec<Hit>) {
    let (w, h) = (cv.w as f32, cv.h as f32);
    cv.rect(0, 0, cv.w as i32, cv.h as i32, rgb(0, 0, 0), 0.6);
    let (cw, ch) = (640.0 * u, 290.0 * u);
    let (cx, cy) = ((w - cw) / 2.0, (h - ch) / 2.0);
    cv.rrect(cx as i32, cy as i32, cw as i32, ch as i32, 22.0 * u, rgb(12, 20, 38), 0.98);
    let q = fit(fonts, question, 32.0 * u, cw - 60.0 * u);
    fonts.draw_centered(cv, &q, w / 2.0, cy + 48.0 * u, 32.0 * u, true, TEXT, 1.0);
    for (i, line) in note.lines().enumerate() {
        let line = fit(fonts, line, 20.0 * u, cw - 60.0 * u);
        fonts.draw_centered(cv, &line, w / 2.0, cy + 92.0 * u + i as f32 * 26.0 * u, 20.0 * u, false, if i == 0 { TEXT } else { DIM }, 1.0);
    }
    for (i, label) in ["Cancel", go].iter().enumerate() {
        let (bw, bh) = (270.0 * u, 62.0 * u);
        let bx = cx + 40.0 * u + i as f32 * (bw + 20.0 * u);
        let sel = m.confirm_sel == i;
        cv.rrect(bx as i32, (cy + 170.0 * u) as i32, bw as i32, bh as i32, 14.0 * u, if i == 1 && danger { rgb(220, 60, 60) } else if i == 1 { GREEN } else { rgb(255, 255, 255) }, if sel { 0.95 } else { 0.14 });
        fonts.draw_centered(cv, label, bx + bw / 2.0, cy + 170.0 * u + 16.0 * u, 26.0 * u, true, if sel { rgb(10, 16, 30) } else { TEXT }, 1.0);
        hits.push(Hit { x: bx as i32, y: (cy + 170.0 * u) as i32, w: bw as i32, h: bh as i32, row: i });
    }
}

/// A message that stays until OK is pressed: for failures that must not be mistaken for success.
fn draw_alert(cv: &mut Canvas, fonts: &mut Fonts, msg: &str, u: f32, hits: &mut Vec<Hit>) {
    let (w, h) = (cv.w as f32, cv.h as f32);
    cv.rect(0, 0, cv.w as i32, cv.h as i32, rgb(0, 0, 0), 0.7);
    let lines = wrap(fonts, msg, 24.0 * u, 640.0 * u);
    let (cw, ch) = (720.0 * u, (190.0 + lines.len().min(7) as f32 * 32.0) * u);
    let (cx, cy) = ((w - cw) / 2.0, (h - ch) / 2.0);
    cv.rrect(cx as i32, cy as i32, cw as i32, ch as i32, 22.0 * u, rgb(12, 20, 38), 0.98);
    cv.rect((cx + 24.0 * u) as i32, (cy + 24.0 * u) as i32, (8.0 * u) as i32, (34.0 * u) as i32, AMBER, 1.0);
    fonts.draw(cv, "ATTENTION", cx + 46.0 * u, cy + 26.0 * u, 26.0 * u, true, AMBER, 1.0);
    for (i, l) in lines.iter().take(7).enumerate() {
        fonts.draw(cv, l, cx + 40.0 * u, cy + 78.0 * u + i as f32 * 32.0 * u, 24.0 * u, false, TEXT, 1.0);
    }
    let (bw, bh) = (200.0 * u, 58.0 * u);
    let (bx, by) = (cx + (cw - bw) / 2.0, cy + ch - bh - 22.0 * u);
    cv.rrect(bx as i32, by as i32, bw as i32, bh as i32, 14.0 * u, rgb(255, 255, 255), 0.95);
    fonts.draw_centered(cv, "OK", bx + bw / 2.0, by + 14.0 * u, 26.0 * u, true, rgb(10, 16, 30), 1.0);
    hits.push(Hit { x: bx as i32, y: by as i32, w: bw as i32, h: bh as i32, row: 0 });
}

fn draw_busy(cv: &mut Canvas, fonts: &mut Fonts, msg: &str, tick: u64, cancellable: bool, u: f32) {
    let (w, h) = (cv.w as f32, cv.h as f32);
    cv.rect(0, 0, cv.w as i32, cv.h as i32, rgb(0, 0, 0), 0.62);
    let (cw, ch) = (680.0 * u, 230.0 * u);
    let (cx, cy) = ((w - cw) / 2.0, (h - ch) / 2.0);
    cv.rrect(cx as i32, cy as i32, cw as i32, ch as i32, 22.0 * u, rgb(12, 20, 38), 0.98);
    for i in 0..8 {
        let a = (i as f32 / 8.0) * std::f32::consts::TAU;
        let lead = ((tick as i64 - i as i64).rem_euclid(8)) as f32 / 8.0;
        cv.circle(w / 2.0 + a.cos() * 26.0 * u, cy + 66.0 * u + a.sin() * 26.0 * u, 5.0 * u, GOLD, 1.0 - lead * 0.85);
    }
    let lines = wrap(fonts, msg, 24.0 * u, cw - 80.0 * u);
    for (i, l) in lines.iter().take(3).enumerate() {
        fonts.draw_centered(cv, l, w / 2.0, cy + 120.0 * u + i as f32 * 32.0 * u, 24.0 * u, false, TEXT, 1.0);
    }
    if cancellable {
        fonts.draw_centered(cv, "BACK  Cancel", w / 2.0, cy + ch - 38.0 * u, 18.0 * u, false, DIM, 0.9);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_testing_finds_the_row_under_a_point() {
        let hits = [Hit { x: 0, y: 0, w: 100, h: 50, row: 0 }, Hit { x: 0, y: 60, w: 100, h: 50, row: 1 }];
        assert_eq!(hit_test(&hits, 10, 10), Some(0));
        assert_eq!(hit_test(&hits, 10, 70), Some(1));
        assert_eq!(hit_test(&hits, 10, 55), None);
        assert_eq!(hit_test(&hits, 200, 10), None);
    }
}

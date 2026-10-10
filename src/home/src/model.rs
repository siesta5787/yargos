// The dashboard's state and behaviour, free of any drawing or system calls so it can be tested.
// `handle` turns an input into state changes plus `Cmd`s for the runtime to run in the background;
// `apply` folds a finished job's `Res` back in.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    Ok,
    Back,
    /// Enter on a physical keyboard: submits text entry, otherwise behaves like Ok.
    Submit,
    Char(char),
    Backspace,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Screen {
    Home,
    Wifi,
    Password,
    Sound,
    Bluetooth,
    Updates,
    Songs,
    System,
    Install,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Network {
    pub name: String,
    pub secured: bool,
    pub signal: u8, // 0..=4
    pub connected: bool,
    pub known: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub card: u32,
    pub dev: u32,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BtDevice {
    pub mac: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
}

impl BtDevice {
    pub fn is_wiimote(&self) -> bool {
        self.name.starts_with("Nintendo RVL")
    }
}

/// A disk the system can be installed on (see `yargos-install list`).
#[derive(Clone, Debug, PartialEq)]
pub struct Disk {
    pub path: String,
    pub bytes: u64,
    pub model: String,
    pub removable: bool,
}

impl Disk {
    /// Drive sizes the way they are sold: decimal gigabytes.
    pub fn size_label(&self) -> String {
        let gb = self.bytes as f64 / 1e9;
        if gb >= 1000.0 { format!("{:.1} TB", gb / 1000.0) } else { format!("{gb:.0} GB") }
    }

    pub fn label(&self) -> String {
        format!("{} ({})", self.model, self.size_label())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Power {
    Restart,
    Shutdown,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    LaunchYarg,
    Refresh(Screen),
    WifiScan,
    WifiConnect { ssid: String, pass: Option<String> },
    SoundSelect { card: u32, dev: u32 },
    SoundVolume { card: u32, pct: u8 },
    SoundTest,
    BtScan,
    BtConnect(BtDevice),
    BtForget(String),
    UpdCheck,
    UpdApply,
    UpdRollback,
    UpdAuto(bool),
    SongsRefresh,
    /// Erase this disk (a /dev path) and install the system on it.
    Install(String),
    Power(Power),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub network: String,
    pub songs: String,
    pub sound: String,
    pub yarg: String,
    pub update: Option<String>,
    pub pads: usize,
}

#[derive(Clone, Debug, Default)]
pub struct UpdInfo {
    pub installed: String,
    pub previous: Option<String>,
    pub latest: Option<String>,
    pub available: bool,
    pub auto: bool,
    pub log: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SongsInfo {
    pub mounted: bool,
    pub detail: String,
    /// The drive's name (its label) and a short "52 GB free of 224 GB", shown as a row once it is connected.
    pub name: String,
    pub space: String,
    pub fs: String,
    /// What YARG will scan: the drive's top-level folders that have songs (or "the whole drive").
    pub sources: Vec<String>,
}

/// Results of background jobs.
#[derive(Clone, Debug)]
pub enum Res {
    Status(Status),
    Wifi { dev: Option<String>, nets: Vec<Network> },
    Sound { outputs: Vec<Output>, current: Option<(u32, u32)>, volume: Option<(u32, u8)> },
    Bt { present: bool, devs: Vec<BtDevice> },
    Upd(UpdInfo),
    Songs(SongsInfo),
    SysInfo(Vec<String>),
    Disks(Vec<Disk>),
    /// One progress line from the installer.
    InstallStep(String),
    InstallDone(Result<(), String>),
    /// A finished action: clear the busy overlay and show this message.
    Done(String),
    /// A finished action whose screen data should also be reloaded.
    DoneRefresh(String, Screen),
    Log(String),
}

// ---- on-screen keyboard ----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Ch(char),
    Shift,
    Space,
    Back,
    Show,
    Done,
}

#[derive(Clone, Debug)]
pub struct Osk {
    pub ssid: String,
    pub text: String,
    pub layer: usize, // 0 abc, 1 ABC, 2 symbols
    pub row: usize,
    pub col: usize,
    pub show: bool,
}

const LAYERS: [[&str; 4]; 3] = [
    ["1234567890", "qwertyuiop", "asdfghjkl@", "zxcvbnm._-!"],
    ["1234567890", "QWERTYUIOP", "ASDFGHJKL@", "ZXCVBNM._-!"],
    ["1234567890", "!@#$%^&*()", "-_=+[]{};:", "'\"\\|,.<>/?~`"],
];

impl Osk {
    pub fn new(ssid: &str) -> Self {
        Self { ssid: ssid.to_string(), text: String::new(), layer: 0, row: 1, col: 0, show: false }
    }

    pub fn rows(&self) -> Vec<Vec<Key>> {
        let mut rows: Vec<Vec<Key>> = LAYERS[self.layer].iter().map(|r| r.chars().map(Key::Ch).collect()).collect();
        rows.push(vec![Key::Shift, Key::Space, Key::Back, Key::Show, Key::Done]);
        rows
    }

    pub fn key(&self) -> Key {
        let rows = self.rows();
        rows[self.row][self.col.min(rows[self.row].len() - 1)]
    }

    fn mv(&mut self, dr: i32, dc: i32) {
        let rows = self.rows();
        let (old_len, old_col) = (rows[self.row].len(), self.col.min(rows[self.row].len() - 1));
        let r = (self.row as i32 + dr).clamp(0, rows.len() as i32 - 1) as usize;
        if dr != 0 && r != self.row {
            // Keep roughly the same horizontal position when rows differ in length.
            let len = rows[r].len();
            self.col = ((old_col as f32 + 0.5) / old_len as f32 * len as f32) as usize;
            self.col = self.col.min(len - 1);
            self.row = r;
        } else if dc != 0 {
            let len = rows[self.row].len() as i32;
            self.col = (old_col as i32 + dc).rem_euclid(len) as usize;
        }
    }
}

// ---- model -----------------------------------------------------------------------------------

pub const HOME_ITEMS: [&str; 7] = ["PLAY YARG", "WI-FI", "SOUND", "BLUETOOTH", "UPDATES", "SONGS", "SYSTEM"];

pub struct Model {
    pub screen: Screen,
    pub sel: usize,
    pub status: Status,
    pub wifi_dev: Option<String>,
    pub nets: Vec<Network>,
    pub osk: Osk,
    pub outputs: Vec<Output>,
    pub cur_card_dev: Option<(u32, u32)>,
    pub volume: Option<(u32, u8)>,
    pub bt: Vec<BtDevice>,
    pub bt_present: bool,
    pub upd: UpdInfo,
    pub songs: SongsInfo,
    pub sysinfo: Vec<String>,
    pub confirm: Option<Power>,
    pub confirm_sel: usize,
    pub disks: Vec<Disk>,
    /// The disk the user is being asked to confirm erasing (index into `disks`).
    pub erase: Option<usize>,
    /// Set once the install finished: what it was installed on.
    pub installed: Option<String>,
    pub install_error: Option<String>,
    /// Label of the disk being installed on while the install runs.
    target: Option<String>,
    pub busy: Option<String>,
    pub toast: Option<(String, Instant)>,
    pub loading: bool,
    pub tick: u64,
}

impl Model {
    pub fn new() -> Self {
        Self {
            screen: Screen::Home,
            sel: 0,
            status: Status::default(),
            wifi_dev: None,
            nets: vec![],
            osk: Osk::new(""),
            outputs: vec![],
            cur_card_dev: None,
            volume: None,
            bt: vec![],
            bt_present: true,
            upd: UpdInfo::default(),
            songs: SongsInfo::default(),
            sysinfo: vec![],
            confirm: None,
            confirm_sel: 0,
            disks: vec![],
            erase: None,
            installed: None,
            install_error: None,
            target: None,
            busy: None,
            toast: None,
            loading: false,
            tick: 0,
        }
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    pub fn live_toast(&self) -> Option<&str> {
        self.toast.as_ref().filter(|(_, t)| t.elapsed() < Duration::from_secs(4)).map(|(m, _)| m.as_str())
    }

    /// Number of selectable rows on the current list screen.
    pub fn row_count(&self) -> usize {
        match self.screen {
            Screen::Home => HOME_ITEMS.len(),
            Screen::Wifi => 1 + self.nets.len(),
            Screen::Sound => self.outputs.len() + usize::from(self.volume.is_some()) + 1,
            Screen::Bluetooth => 1 + self.bt.len(),
            Screen::Updates => 4,
            Screen::Songs => 1 + usize::from(self.songs.mounted),
            Screen::System => 3,
            Screen::Install => if self.installed.is_some() { 2 } else { 1 + self.disks.len() },
            Screen::Password => 0,
        }
    }

    /// Go straight to a screen (the "Install" boot menu entry opens the installer).
    pub fn open(&mut self, screen: Screen) -> Vec<Cmd> {
        self.enter(screen)
    }

    fn enter(&mut self, screen: Screen) -> Vec<Cmd> {
        self.screen = screen;
        self.sel = 0;
        self.loading = screen != Screen::Home && screen != Screen::Password;
        if screen == Screen::Home { vec![Cmd::Refresh(Screen::Home)] } else { vec![Cmd::Refresh(screen)] }
    }

    /// Move the highlight to a row without activating it (mouse hover).
    pub fn hover(&mut self, id: usize) {
        if self.busy.is_some() {
            return;
        }
        if self.confirm.is_some() || self.erase.is_some() {
            self.confirm_sel = id.min(1);
        } else if self.screen == Screen::Password {
            let (r, c) = (id / 16, id % 16);
            let rows = self.osk.rows();
            if r < rows.len() && c < rows[r].len() {
                self.osk.row = r;
                self.osk.col = c;
            }
        } else if id < self.row_count() {
            self.sel = id;
        }
    }

    /// A mouse click on a row, key or button (ids as given by the drawing layer's hit areas).
    pub fn click(&mut self, id: usize) -> Vec<Cmd> {
        self.hover(id);
        self.handle(Nav::Ok)
    }

    pub fn handle(&mut self, nav: Nav) -> Vec<Cmd> {
        if self.busy.is_some() {
            return vec![]; // ignore input while an action runs
        }
        if let Some(p) = self.confirm {
            return self.handle_confirm(p, nav);
        }
        if let Some(i) = self.erase {
            return self.handle_erase(i, nav);
        }
        match self.screen {
            Screen::Password => self.handle_password(nav),
            _ => self.handle_list(nav),
        }
    }

    fn handle_confirm(&mut self, p: Power, nav: Nav) -> Vec<Cmd> {
        match nav {
            Nav::Left | Nav::Right | Nav::Up | Nav::Down => self.confirm_sel = 1 - self.confirm_sel,
            Nav::Back => self.confirm = None,
            Nav::Ok | Nav::Submit => {
                self.confirm = None;
                if self.confirm_sel == 1 {
                    self.busy = Some(if p == Power::Shutdown { "Shutting down…".into() } else { "Restarting…".into() });
                    return vec![Cmd::Power(p)];
                }
            }
            _ => {}
        }
        vec![]
    }

    /// The "erase this disk?" question: Cancel is the default.
    fn handle_erase(&mut self, i: usize, nav: Nav) -> Vec<Cmd> {
        match nav {
            Nav::Left | Nav::Right | Nav::Up | Nav::Down => self.confirm_sel = 1 - self.confirm_sel,
            Nav::Back => self.erase = None,
            Nav::Ok | Nav::Submit => {
                self.erase = None;
                if self.confirm_sel == 1 {
                    let disk = self.disks[i].clone();
                    self.install_error = None;
                    self.target = Some(disk.label());
                    self.busy = Some(format!("Installing on {}\u{2026} do not unplug or switch off", disk.label()));
                    return vec![Cmd::Install(disk.path)];
                }
            }
            _ => {}
        }
        vec![]
    }

    fn handle_password(&mut self, nav: Nav) -> Vec<Cmd> {
        match nav {
            Nav::Up => self.osk.mv(-1, 0),
            Nav::Down => self.osk.mv(1, 0),
            Nav::Left => self.osk.mv(0, -1),
            Nav::Right => self.osk.mv(0, 1),
            Nav::Back => {
                self.screen = Screen::Wifi;
                self.sel = 0;
            }
            Nav::Char(c) => self.osk.text.push(c),
            Nav::Backspace => {
                self.osk.text.pop();
            }
            Nav::Submit => return self.osk_done(),
            Nav::Ok => match self.osk.key() {
                Key::Ch(c) => self.osk.text.push(c),
                Key::Space => self.osk.text.push(' '),
                Key::Back => {
                    self.osk.text.pop();
                }
                Key::Shift => self.osk.layer = (self.osk.layer + 1) % 3,
                Key::Show => self.osk.show = !self.osk.show,
                Key::Done => return self.osk_done(),
            },
        }
        vec![]
    }

    fn osk_done(&mut self) -> Vec<Cmd> {
        if self.osk.text.len() < 8 {
            self.toast("A Wi-Fi password is at least 8 characters");
            return vec![];
        }
        self.busy = Some(format!("Connecting to {}…", self.osk.ssid));
        self.screen = Screen::Wifi;
        self.sel = 0;
        vec![Cmd::WifiConnect { ssid: self.osk.ssid.clone(), pass: Some(self.osk.text.clone()) }]
    }

    fn handle_list(&mut self, nav: Nav) -> Vec<Cmd> {
        let n = self.row_count();
        match nav {
            Nav::Up if n > 0 => self.sel = (self.sel + n - 1) % n,
            Nav::Down if n > 0 => self.sel = (self.sel + 1) % n,
            Nav::Back => {
                if self.screen != Screen::Home {
                    self.screen = Screen::Home;
                    self.sel = 0;
                    return vec![Cmd::Refresh(Screen::Home)];
                }
            }
            Nav::Left | Nav::Right => return self.adjust(nav == Nav::Right),
            Nav::Ok | Nav::Submit => return self.activate(),
            _ => {}
        }
        vec![]
    }

    /// Left/Right on a row: volume, or forgetting a Bluetooth device.
    fn adjust(&mut self, right: bool) -> Vec<Cmd> {
        match self.screen {
            Screen::Sound if self.volume.is_some() && self.sel == self.outputs.len() => {
                let (card, v) = self.volume.unwrap();
                let nv = (v as i32 + if right { 5 } else { -5 }).clamp(0, 100) as u8;
                self.volume = Some((card, nv));
                vec![Cmd::SoundVolume { card, pct: nv }]
            }
            Screen::Bluetooth if !right && self.sel >= 1 => {
                let d = self.bt[self.sel - 1].clone();
                if d.paired {
                    self.busy = Some(format!("Forgetting {}…", d.name));
                    return vec![Cmd::BtForget(d.mac)];
                }
                vec![]
            }
            _ => vec![],
        }
    }

    fn activate(&mut self) -> Vec<Cmd> {
        match self.screen {
            Screen::Home => match self.sel {
                0 => vec![Cmd::LaunchYarg],
                i => {
                    let s = [Screen::Wifi, Screen::Sound, Screen::Bluetooth, Screen::Updates, Screen::Songs, Screen::System][i - 1];
                    self.enter(s)
                }
            },
            Screen::Wifi => {
                if self.sel == 0 {
                    self.busy = Some("Scanning for networks…".into());
                    return vec![Cmd::WifiScan];
                }
                let net = self.nets[self.sel - 1].clone();
                if net.connected {
                    self.toast(format!("Already connected to {}", net.name));
                    vec![]
                } else if net.secured && !net.known {
                    self.osk = Osk::new(&net.name);
                    self.screen = Screen::Password;
                    vec![]
                } else {
                    self.busy = Some(format!("Connecting to {}…", net.name));
                    vec![Cmd::WifiConnect { ssid: net.name, pass: None }]
                }
            }
            Screen::Sound => {
                if self.sel < self.outputs.len() {
                    let o = self.outputs[self.sel].clone();
                    self.cur_card_dev = Some((o.card, o.dev));
                    vec![Cmd::SoundSelect { card: o.card, dev: o.dev }]
                } else if self.volume.is_some() && self.sel == self.outputs.len() {
                    vec![]
                } else {
                    vec![Cmd::SoundTest]
                }
            }
            Screen::Bluetooth => {
                if self.sel == 0 {
                    if !self.bt_present {
                        self.toast("No Bluetooth adapter found");
                        return vec![];
                    }
                    self.busy = Some("Scanning… hold 1+2 on a Wiimote, or put a device in pairing mode".into());
                    return vec![Cmd::BtScan];
                }
                let d = self.bt[self.sel - 1].clone();
                self.busy = Some(if d.is_wiimote() {
                    format!("Connecting {}… keep holding 1+2", d.name)
                } else {
                    format!("Connecting {}…", d.name)
                });
                vec![Cmd::BtConnect(d)]
            }
            Screen::Updates => match self.sel {
                0 => {
                    self.busy = Some("Checking for updates…".into());
                    vec![Cmd::UpdCheck]
                }
                1 => {
                    if !self.upd.available {
                        self.toast("You are up to date");
                        return vec![];
                    }
                    self.busy = Some("Downloading the update… this can take a few minutes".into());
                    vec![Cmd::UpdApply]
                }
                2 => {
                    self.upd.auto = !self.upd.auto;
                    vec![Cmd::UpdAuto(self.upd.auto)]
                }
                _ => {
                    if self.upd.previous.is_none() {
                        self.toast("There is no earlier version to go back to");
                        return vec![];
                    }
                    self.busy = Some("Going back to the previous version…".into());
                    vec![Cmd::UpdRollback]
                }
            },
            Screen::Songs if self.sel == 0 => {
                self.busy = Some("Looking for the song drive\u{2026}".into());
                vec![Cmd::SongsRefresh]
            }
            Screen::Songs => {
                self.toast("The song drive is at /media/songs; YARG scans it each time it starts");
                vec![]
            }
            Screen::System if self.sel == 2 => self.enter(Screen::Install),
            Screen::System => {
                self.confirm = Some(if self.sel == 0 { Power::Restart } else { Power::Shutdown });
                self.confirm_sel = 0;
                vec![]
            }
            Screen::Install if self.installed.is_some() => {
                self.confirm = Some(if self.sel == 0 { Power::Restart } else { Power::Shutdown });
                self.confirm_sel = 0;
                vec![]
            }
            Screen::Install if self.sel == 0 => {
                self.busy = Some("Looking for disks\u{2026}".into());
                vec![Cmd::Refresh(Screen::Install)]
            }
            Screen::Install => {
                self.erase = Some(self.sel - 1);
                self.confirm_sel = 0;
                vec![]
            }
            Screen::Password => vec![],
        }
    }

    pub fn apply(&mut self, res: Res) {
        match res {
            Res::Status(s) => self.status = s,
            Res::Wifi { dev, nets } => {
                self.wifi_dev = dev;
                self.nets = nets;
                self.loading = false;
                self.busy = None;
                self.sel = self.sel.min(self.row_count().saturating_sub(1));
            }
            Res::Sound { outputs, current, volume } => {
                self.outputs = outputs;
                self.cur_card_dev = current;
                self.volume = volume;
                self.loading = false;
                self.sel = self.sel.min(self.row_count().saturating_sub(1));
            }
            Res::Bt { present, devs } => {
                self.bt_present = present;
                self.bt = devs;
                self.loading = false;
                self.busy = None;
                self.sel = self.sel.min(self.row_count().saturating_sub(1));
            }
            Res::Upd(u) => {
                self.upd = u;
                self.loading = false;
                self.busy = None;
            }
            Res::Songs(s) => {
                self.songs = s;
                self.loading = false;
                self.busy = None;
            }
            Res::SysInfo(lines) => {
                self.sysinfo = lines;
                self.loading = false;
            }
            Res::Disks(d) => {
                self.disks = d;
                self.loading = false;
                self.busy = None;
                self.sel = self.sel.min(self.row_count().saturating_sub(1));
            }
            Res::InstallStep(text) => {
                if self.busy.is_some() {
                    self.busy = Some(text);
                }
            }
            Res::InstallDone(Ok(())) => {
                self.busy = None;
                self.installed = Some(self.target.take().unwrap_or_else(|| "the disk".into()));
                self.sel = 0;
            }
            Res::InstallDone(Err(e)) => {
                self.busy = None;
                self.install_error = Some(e);
            }
            Res::Done(msg) => {
                self.busy = None;
                self.toast(msg);
            }
            Res::DoneRefresh(msg, _) => {
                self.busy = None;
                self.toast(msg);
            }
            Res::Log(line) => {
                self.upd.log.push(line);
                if self.upd.log.len() > 8 {
                    self.upd.log.remove(0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_wifi() -> Model {
        let mut m = Model::new();
        m.screen = Screen::Wifi;
        m.nets = vec![
            Network { name: "Home".into(), secured: true, signal: 4, connected: false, known: false },
            Network { name: "Cafe".into(), secured: false, signal: 2, connected: false, known: false },
            Network { name: "Saved".into(), secured: true, signal: 3, connected: false, known: true },
            Network { name: "Now".into(), secured: true, signal: 4, connected: true, known: true },
        ];
        m
    }

    #[test]
    fn home_navigation_wraps_and_play_launches() {
        let mut m = Model::new();
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::LaunchYarg]);
        m.handle(Nav::Up);
        assert_eq!(m.sel, HOME_ITEMS.len() - 1);
        m.handle(Nav::Down);
        assert_eq!(m.sel, 0);
    }

    #[test]
    fn entering_a_screen_asks_for_its_data_and_back_returns_home() {
        let mut m = Model::new();
        m.handle(Nav::Down);
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Wifi)]);
        assert_eq!(m.screen, Screen::Wifi);
        assert_eq!(m.handle(Nav::Back), vec![Cmd::Refresh(Screen::Home)]);
        assert_eq!(m.screen, Screen::Home);
    }

    #[test]
    fn secured_unknown_network_opens_the_password_screen() {
        let mut m = at_wifi();
        m.sel = 1; // Home (row 0 is "scan")
        assert!(m.handle(Nav::Ok).is_empty());
        assert_eq!(m.screen, Screen::Password);
        assert_eq!(m.osk.ssid, "Home");
    }

    #[test]
    fn open_and_saved_networks_connect_without_a_password() {
        let mut m = at_wifi();
        m.sel = 2;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::WifiConnect { ssid: "Cafe".into(), pass: None }]);
        let mut m = at_wifi();
        m.sel = 3;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::WifiConnect { ssid: "Saved".into(), pass: None }]);
    }

    #[test]
    fn connected_network_is_not_reconnected() {
        let mut m = at_wifi();
        m.sel = 4;
        assert!(m.handle(Nav::Ok).is_empty());
        assert!(m.live_toast().unwrap().contains("Already connected"));
    }

    #[test]
    fn typing_a_password_on_a_physical_keyboard_and_submitting() {
        let mut m = at_wifi();
        m.sel = 1;
        m.handle(Nav::Ok);
        for c in "hunter22".chars() {
            m.handle(Nav::Char(c));
        }
        let cmds = m.handle(Nav::Submit);
        assert_eq!(cmds, vec![Cmd::WifiConnect { ssid: "Home".into(), pass: Some("hunter22".into()) }]);
        assert!(m.busy.is_some());
    }

    #[test]
    fn short_passwords_are_rejected_before_connecting() {
        let mut m = at_wifi();
        m.sel = 1;
        m.handle(Nav::Ok);
        m.handle(Nav::Char('a'));
        assert!(m.handle(Nav::Submit).is_empty());
        assert_eq!(m.screen, Screen::Password);
    }

    #[test]
    fn onscreen_keyboard_can_type_with_only_a_d_pad_and_ok() {
        let mut m = at_wifi();
        m.sel = 1;
        m.handle(Nav::Ok);
        // Starts on row 1 (qwerty), col 0 = 'q'
        m.handle(Nav::Ok);
        m.handle(Nav::Right);
        m.handle(Nav::Ok); // 'w'
        assert_eq!(m.osk.text, "qw");
        m.handle(Nav::Down);
        m.handle(Nav::Down);
        m.handle(Nav::Down); // special row
        assert!(matches!(m.osk.key(), Key::Shift | Key::Space | Key::Back | Key::Show | Key::Done));
    }

    #[test]
    fn shift_cycles_layers_and_back_leaves_the_keyboard() {
        let mut m = at_wifi();
        m.sel = 1;
        m.handle(Nav::Ok);
        m.osk.row = 4;
        m.osk.col = 0;
        assert_eq!(m.osk.key(), Key::Shift);
        m.handle(Nav::Ok);
        assert_eq!(m.osk.layer, 1);
        m.handle(Nav::Back);
        assert_eq!(m.screen, Screen::Wifi);
    }

    #[test]
    fn clicking_a_row_selects_and_activates_it() {
        let mut m = Model::new();
        assert_eq!(m.click(0), vec![Cmd::LaunchYarg]);
        assert_eq!(m.click(2), vec![Cmd::Refresh(Screen::Sound)]);
        assert_eq!(m.screen, Screen::Sound);
    }

    #[test]
    fn clicking_onscreen_keys_types_them_and_clicking_confirm_buttons_works() {
        let mut m = at_wifi();
        m.sel = 1;
        m.handle(Nav::Ok);
        m.click(2 * 16 + 4); // row 2 ("asdf..."), col 4 = 'g'
        assert_eq!(m.osk.text, "g");
        let mut m = Model::new();
        m.screen = Screen::System;
        m.confirm = Some(Power::Restart);
        assert_eq!(m.click(1), vec![Cmd::Power(Power::Restart)]);
    }

    #[test]
    fn input_is_ignored_while_busy() {
        let mut m = Model::new();
        m.busy = Some("working".into());
        assert!(m.handle(Nav::Ok).is_empty());
        assert_eq!(m.sel, 0);
    }

    #[test]
    fn power_actions_need_confirmation() {
        let mut m = Model::new();
        m.screen = Screen::System;
        m.sel = 1;
        assert!(m.handle(Nav::Ok).is_empty());
        assert_eq!(m.confirm, Some(Power::Shutdown));
        // Default choice is Cancel.
        assert!(m.handle(Nav::Ok).is_empty());
        assert!(m.confirm.is_none() && m.busy.is_none());
        m.handle(Nav::Ok);
        m.handle(Nav::Right);
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Power(Power::Shutdown)]);
    }

    #[test]
    fn volume_row_adjusts_with_left_and_right() {
        let mut m = Model::new();
        m.screen = Screen::Sound;
        m.outputs = vec![Output { card: 0, dev: 3, label: "HDMI".into() }];
        m.volume = Some((0, 50));
        m.sel = 1;
        assert_eq!(m.handle(Nav::Right), vec![Cmd::SoundVolume { card: 0, pct: 55 }]);
        m.volume = Some((0, 2));
        assert_eq!(m.handle(Nav::Left), vec![Cmd::SoundVolume { card: 0, pct: 0 }]);
    }

    #[test]
    fn choosing_an_output_selects_it() {
        let mut m = Model::new();
        m.screen = Screen::Sound;
        m.outputs = vec![
            Output { card: 0, dev: 3, label: "HDMI".into() },
            Output { card: 1, dev: 0, label: "USB".into() },
        ];
        m.sel = 1;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::SoundSelect { card: 1, dev: 0 }]);
        assert_eq!(m.cur_card_dev, Some((1, 0)));
    }

    #[test]
    fn bluetooth_wiimote_hint_and_forgetting() {
        let mut m = Model::new();
        m.screen = Screen::Bluetooth;
        m.bt = vec![
            BtDevice { mac: "00:22:D7:AD:BE:D4".into(), name: "Nintendo RVL-CNT-01".into(), paired: false, connected: false },
            BtDevice { mac: "AA".into(), name: "Keyboard".into(), paired: true, connected: true },
        ];
        m.sel = 1;
        assert!(matches!(m.handle(Nav::Ok)[0], Cmd::BtConnect(_)));
        assert!(m.busy.as_ref().unwrap().contains("1+2"));
        m.busy = None;
        m.sel = 2;
        assert_eq!(m.handle(Nav::Left), vec![Cmd::BtForget("AA".into())]);
        m.busy = None;
        m.sel = 1;
        assert!(m.handle(Nav::Left).is_empty()); // unpaired devices cannot be forgotten
    }

    #[test]
    fn updates_only_install_when_one_is_available() {
        let mut m = Model::new();
        m.screen = Screen::Updates;
        m.sel = 1;
        assert!(m.handle(Nav::Ok).is_empty());
        m.upd.available = true;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::UpdApply]);
        m.busy = None;
        m.sel = 2;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::UpdAuto(true)]);
        m.sel = 3;
        assert!(m.handle(Nav::Ok).is_empty()); // nothing to go back to
        m.upd.previous = Some("0.14.0".into());
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::UpdRollback]);
    }

    #[test]
    fn results_clear_the_busy_overlay_and_clamp_the_selection() {
        let mut m = at_wifi();
        m.sel = 4;
        m.busy = Some("x".into());
        m.apply(Res::Wifi { dev: Some("wlan0".into()), nets: vec![] });
        assert!(m.busy.is_none());
        assert_eq!(m.sel, 0);
    }

    fn disks() -> Vec<Disk> {
        vec![
            Disk { path: "/dev/sda".into(), bytes: 256_000_000_000, model: "Internal SSD".into(), removable: false },
            Disk { path: "/dev/sdb".into(), bytes: 1_000_000_000_000, model: "Portable".into(), removable: true },
        ]
    }

    #[test]
    fn the_system_screen_leads_to_the_installer() {
        let mut m = Model::new();
        m.screen = Screen::System;
        m.sel = 2;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Install)]);
        assert_eq!(m.screen, Screen::Install);
    }

    #[test]
    fn choosing_a_disk_asks_first_and_cancel_is_the_default() {
        let mut m = Model::new();
        m.screen = Screen::Install;
        m.disks = disks();
        m.sel = 2; // second disk (row 0 is "look again")
        assert!(m.handle(Nav::Ok).is_empty());
        assert_eq!(m.erase, Some(1));
        assert_eq!(m.confirm_sel, 0);
        assert!(m.handle(Nav::Ok).is_empty()); // OK on Cancel
        assert!(m.erase.is_none() && m.busy.is_none());
    }

    #[test]
    fn confirming_erase_starts_the_install_and_input_is_locked_until_it_ends() {
        let mut m = Model::new();
        m.screen = Screen::Install;
        m.disks = disks();
        m.sel = 1;
        m.handle(Nav::Ok);
        m.handle(Nav::Right);
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Install("/dev/sda".into())]);
        assert!(m.busy.is_some());
        assert!(m.handle(Nav::Back).is_empty()); // cannot back out mid-install
        assert_eq!(m.screen, Screen::Install);
        m.apply(Res::InstallStep("Formatting (step 2 of 6)".into()));
        assert_eq!(m.busy.as_deref(), Some("Formatting (step 2 of 6)"));
        m.apply(Res::InstallDone(Ok(())));
        assert!(m.busy.is_none());
        assert_eq!(m.installed.as_deref(), Some("Internal SSD (256 GB)"));
        assert_eq!(m.row_count(), 2); // Restart / Shut down
        assert!(m.handle(Nav::Ok).is_empty());
        assert_eq!(m.confirm, Some(Power::Restart));
    }

    #[test]
    fn a_failed_install_keeps_the_disk_list_and_shows_why() {
        let mut m = Model::new();
        m.screen = Screen::Install;
        m.disks = disks();
        m.busy = Some("x".into());
        m.apply(Res::InstallDone(Err("Could not partition /dev/sda".into())));
        assert!(m.busy.is_none() && m.installed.is_none());
        assert_eq!(m.install_error.as_deref(), Some("Could not partition /dev/sda"));
        assert_eq!(m.row_count(), 3);
    }

    #[test]
    fn escape_cancels_the_erase_question() {
        let mut m = Model::new();
        m.screen = Screen::Install;
        m.disks = disks();
        m.sel = 1;
        m.handle(Nav::Ok);
        m.handle(Nav::Back);
        assert!(m.erase.is_none());
        assert_eq!(m.screen, Screen::Install);
    }

    #[test]
    fn a_connected_song_drive_gets_its_own_row() {
        let mut m = Model::new();
        m.screen = Screen::Songs;
        assert_eq!(m.row_count(), 1);
        m.apply(Res::Songs(SongsInfo { mounted: true, name: "YARGSONGS".into(), space: "52 GB free of 224 GB".into(), ..SongsInfo::default() }));
        assert_eq!(m.row_count(), 2);
        m.sel = 1;
        assert!(m.handle(Nav::Ok).is_empty()); // only explains where it is
        assert!(m.live_toast().unwrap().contains("/media/songs"));
        m.sel = 0;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::SongsRefresh]);
    }
}

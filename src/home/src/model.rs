// The dashboard's state and behaviour, free of any drawing or system calls so it can be tested.
// `handle` turns an input into state changes plus `Cmd`s for the runtime to run in the background;
// `apply` folds a finished job's `Res` back in.

use std::time::{Duration, Instant};

pub use crate::songcopy::{Entry as SongEntry, Kind as SongKind};

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
    /// YARG's own updates (the game): check, install, automatic on/off, go back.
    Updates,
    /// The song drive (the portable drive labelled YARGSONGS); it lives under Storage.
    Songs,
    System,
    /// SYSTEM > Updates: the choice between the system's updates and YARG's.
    UpdatesMenu,
    Install,
    Devices,
    /// The system (YARG OS itself): check for, install and go back from an over-the-air update.
    OsUpdate,
    /// First install of YARG itself (an image without it): download, or install from a zip on the song drive.
    Yarg,
    /// The internal drive: songs kept on the drive the system runs from.
    Storage,
    /// Pick a portable drive to copy its songs to the internal drive.
    ImportPick,
    /// What the system saw: Bluetooth, sound, YARG and system reports to read (or photograph) instead of typing commands.
    Diag,
    /// Browse the internal songs and delete a song, a folder or everything.
    Manage,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Network {
    pub name: String,
    pub secured: bool,
    pub signal: u8, // 0..=4
    pub connected: bool,
    pub known: bool,
}

/// In `Model::volume`, "the card" of a Bluetooth headset's level (it has no sound card number).
pub const BT_VOLUME: u32 = u32::MAX;

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub card: u32,
    pub dev: u32,
    pub label: String,
    /// A connected Bluetooth headset (its address): played through bluealsa, not a sound card.
    pub bt: Option<String>,
}

/// A portable drive that can be imported (see `yargos-songs list`).
#[derive(Clone, Debug, PartialEq)]
pub struct ImportDrive {
    pub dev: String,
    pub label: String,
    pub fs: String,
    pub bytes: u64,
    pub uuid: String,
}

impl ImportDrive {
    pub fn name(&self) -> String {
        if self.label.is_empty() { "USB drive".into() } else { self.label.clone() }
    }
}

/// What copying a drive would do: found by looking at the drive and the internal drive, shown before anything is copied.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportPlan {
    pub drive: ImportDrive,
    /// The folder it goes into, below /data/songs.
    pub dest: String,
    pub files: u64,
    pub bytes: u64,
    pub todo_files: u64,
    pub todo_bytes: u64,
    pub unreadable: u64,
    /// Space on the internal drive and the part of it that is kept free for the system (KiB).
    pub free_kib: u64,
    pub reserve_kib: u64,
}

impl ImportPlan {
    pub fn fits(&self) -> bool {
        self.todo_bytes / 1024 + self.reserve_kib <= self.free_kib
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StorageInfo {
    pub used_kib: u64,
    pub free_kib: u64,
    pub total_kib: u64,
    pub reserve_kib: u64,
    /// Imported drives (folders directly in the library).
    pub folders: usize,
    /// Room on the drive after the data partition that is not in use (the partition did not grow to fill it).
    pub slack_kib: u64,
    /// The system runs from a USB stick or another removable drive (so "Install on this computer's disk" makes sense). False
    /// once it runs from a disk built into the computer.
    pub on_stick: bool,
    /// Venue files in YARG's venue folder (/data/yarg/venue).
    pub venues: u64,
}

/// The rows of the Storage screen, in order: some only exist when they have something to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StorageRow {
    Import,
    Manage,
    SongDrive,
    /// Copy venue files from the song drive to YARG's venue folder.
    Venues,
    /// Delete the venue files that were added (only while there are some).
    RemoveVenues,
    /// Unused room after the data partition.
    UseRest,
    /// Copy this system to the computer's own disk (only while running from a stick).
    Install,
}

/// Unused room worth offering to grow into.
pub const SLACK_WORTH_KIB: u64 = 512 * 1024;

/// Where the song browser is: names below /data/songs, and what is in that folder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Browse {
    pub path: Vec<String>,
    pub entries: Vec<SongEntry>,
}

/// A question with Cancel (the default) and a button that runs `cmd`.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub question: String,
    pub note: String,
    pub go: String,
    /// The second button is drawn red (it throws something away).
    pub danger: bool,
    /// What the busy overlay says while `cmd` runs, and what BACK does there (if it can be stopped).
    pub busy: String,
    pub cancel: Option<Cmd>,
    pub cmd: Cmd,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BtDevice {
    pub mac: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
    /// A connected Wiimote is only usable once the guitar bridge runs for it; everything else is ready when connected.
    pub ready: bool,
    /// A connected Wiimote's batteries, 0..=100, once it has told (see `sys::wii_batteries`).
    pub battery: Option<u8>,
    /// Its batteries are nearly empty (its own flag, or a low level).
    pub battery_low: bool,
    /// Headphones, earbuds or a speaker: something that can be a sound output.
    pub audio: bool,
}

impl BtDevice {
    pub fn is_wiimote(&self) -> bool {
        self.name.starts_with("Nintendo RVL")
    }
}

/// One thing the kernel sees plugged in or built in (USB device, MIDI port, sound card, controller).
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceEntry {
    pub kind: &'static str,
    pub name: String,
    pub detail: String,
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
    /// Switch the Wi-Fi adapter on (true) or off (false).
    WifiPower(bool),
    WifiConnect { ssid: String, pass: Option<String> },
    SoundSelect { card: u32, dev: u32 },
    /// Play on this Bluetooth headset (its address).
    SoundSelectBt(String),
    SoundVolume { card: u32, pct: u8 },
    SoundTest,
    BtScan,
    BtConnect(BtDevice),
    BtForget(String),
    UpdCheck,
    UpdApply,
    /// Download and install YARG from its GitHub release (nothing installed yet).
    YargInstall,
    /// Install YARG from a release zip on the song drive (full path).
    YargInstallZip(String),
    /// Stop the YARG download/install that is running (BACK on the busy overlay).
    YargCancel,
    OsCheck,
    OsInstall,
    /// Install the update found on the song drive (no internet).
    OsInstallOffline,
    /// Look at the venue files on the song drive (the answer is a question: copy them?).
    VenuesScan,
    /// Copy the venue files from the song drive to YARG's venue folder.
    VenuesRun,
    /// Delete the venue files in YARG's venue folder.
    VenuesRemove,
    /// Follow the beta channel (true: test versions) or only the stable one (false).
    OsChannel(bool),
    /// Look on the song drive again for a system update (the dashboard asks again every few seconds while the screen is open).
    OsFindOffline,
    OsCancel,
    OsGoBack,
    OsAck,
    OsAuto(bool),
    UpdRollback,
    UpdAuto(bool),
    SongsRefresh,
    SongsAuto(bool),
    /// Erase this disk (a /dev path) and install the system on it.
    Install(String),
    /// Read this page of SYSTEM > Diagnostics again.
    DiagLoad(usize),
    /// Mount this portable drive and work out what copying it would do.
    ImportScan(ImportDrive),
    /// Copy it (the plan the user agreed to).
    ImportRun(ImportPlan),
    /// Stop the looking or the copying that is running.
    ImportCancel,
    /// Make the data partition fill the drive (it should have at boot).
    GrowData,
    /// List this folder of the internal songs (names below /data/songs).
    SongsBrowse(Vec<String>),
    /// Delete this song or folder of the internal songs; no names = everything.
    SongsDelete(Vec<String>),
    Power(Power),
}

/// What the system updater says (read from /data/os-update/status).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OsInfo {
    pub running: String,
    pub latest: Option<String>,
    /// A newer system is published and not yet installed.
    pub available: Option<String>,
    /// A newer system is installed and starts at the next restart.
    pub staged: Option<String>,
    pub notes: String,
    /// An update that did not start properly and was undone (shown until it is acknowledged).
    pub rolled_back: Option<String>,
    /// The earlier system that "go back" returns to.
    pub previous: Option<String>,
    pub auto: bool,
    /// Why the last check failed: "offline" or "signature".
    pub error: Option<String>,
    /// The reason in words (what the network said), when the check could not reach the server.
    pub why: Option<String>,
    /// An update (this version) is on the song drive, ready to install without internet.
    pub offline: Option<String>,
    /// Something is on the song drive but cannot be used: a sentence for the side panel.
    pub offline_hint: Option<String>,
    /// This device follows the beta channel: test versions before everyone else.
    pub beta: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub network: String,
    pub songs: String,
    pub sound: String,
    pub yarg: String,
    /// A line for the main menu card about an update that is available or waiting for a restart.
    pub update: Option<String>,
    /// A system update that did not start properly and was undone: the version (until the notice is acknowledged).
    pub os_rolled_back: Option<String>,
    pub pads: usize,
    /// Connected Wii guitars whose batteries are low, like "Wii guitar (D4) 12%".
    pub low_battery: Vec<String>,
    /// The version of the system that is running (YARG OS itself, not YARG).
    pub os_version: String,
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
    /// Add each top-level song folder as its own source in YARG (AUTO_SONG_SOURCES in /data/yargos.conf).
    pub auto: bool,
}

/// Results of background jobs.
#[derive(Clone, Debug)]
pub enum Res {
    Status(Status),
    Wifi { dev: Option<String>, nets: Vec<Network> },
    /// Is the Wi-Fi adapter on?
    WifiPower(bool),
    /// The venue files found on the song drive, or why there are none.
    VenuesPlan(Result<crate::venues::VenuePlan, String>),
    /// Venues were added or removed: the message to show, or what went wrong.
    VenuesDone(Result<String, String>),
    /// What the song drive holds as a system update (or why not), see Cmd::OsFindOffline.
    OsOffline { offline: Option<String>, hint: Option<String> },
    Sound { outputs: Vec<Output>, current: Option<(u32, u32)>, current_bt: Option<String>, volume: Option<(u32, u8)> },
    Bt { present: bool, devs: Vec<BtDevice> },
    Upd(UpdInfo),
    Songs(SongsInfo),
    SysInfo(Vec<String>),
    Disks(Vec<Disk>),
    Devices(Vec<DeviceEntry>),
    Storage(StorageInfo),
    Drives(Vec<ImportDrive>),
    /// What copying a drive would do, or why that could not be worked out.
    ImportPlan(Result<ImportPlan, String>),
    /// A copy finished: what was copied (a toast), or why it stopped / what went wrong (an alert).
    ImportDone(Result<String, String>),
    Browse { path: Vec<String>, entries: Vec<SongEntry> },
    /// A page of the diagnostics report.
    Diag { page: usize, lines: Vec<String> },
    Os(OsInfo),
    /// A system update (or going back) finished: a message that asks for a restart, or why it failed.
    OsDone(Result<String, String>),
    /// YARG release zips found on the song drive.
    YargZips(Vec<String>),
    /// Progress text of a long job (only shown while the busy overlay is up).
    Progress(String),
    /// A YARG install finished: the message to show, or why it failed.
    YargInstalled(Result<String, String>),
    /// One progress line from the installer.
    InstallStep(String),
    InstallDone(Result<(), String>),
    /// Something failed in a way the user must not miss: shown as a message that waits for OK.
    Alert(String),
    /// A finished action: clear the busy overlay and show this message.
    Done(String),
    /// A finished action whose screen data should also be reloaded.
    DoneRefresh(String, Screen),
    Log(String),
}

/// 18400 -> "18,400".
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Sizes in the units drives are sold in (decimal), from KiB.
pub fn human_kib(kib: u64) -> String {
    let b = kib as f64 * 1024.0;
    if b >= 1e12 {
        format!("{:.1} TB", b / 1e12)
    } else if b >= 1e10 {
        format!("{:.0} GB", b / 1e9)
    } else if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else {
        format!("{:.0} KB", (b / 1e3).ceil())
    }
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

pub const HOME_ITEMS: [&str; 5] = ["PLAY YARG", "SOUND", "BLUETOOTH", "CONNECTED DEVICES", "SYSTEM"];

/// SYSTEM: four rows (Wi-Fi, Updates, Storage, Diagnostics), then the two power icons: Restart, Shut down.
pub const SYSTEM_ROWS: usize = 4;

/// The busy text while "Install from the song drive" looks at the drive again.
pub const LOOKING_ON_DRIVE: &str = "Looking on the song drive\u{2026}";
const INSTALL_YARG: &str = "INSTALL YARG";

pub struct Model {
    pub screen: Screen,
    pub sel: usize,
    pub status: Status,
    pub wifi_dev: Option<String>,
    /// The Wi-Fi adapter is on (it can be switched off on the Wi-Fi screen).
    pub wifi_on: bool,
    pub nets: Vec<Network>,
    pub osk: Osk,
    pub outputs: Vec<Output>,
    pub cur_card_dev: Option<(u32, u32)>,
    /// The Bluetooth headset the sound goes to (when it is not a sound card).
    pub cur_bt: Option<String>,
    pub volume: Option<(u32, u8)>,
    pub bt: Vec<BtDevice>,
    pub bt_present: bool,
    pub upd: UpdInfo,
    pub songs: SongsInfo,
    pub sysinfo: Vec<String>,
    pub confirm: Option<Power>,
    pub confirm_sel: usize,
    pub disks: Vec<Disk>,
    pub devices: Vec<DeviceEntry>,
    /// YARG release zips on the song drive (full paths).
    pub yarg_zips: Vec<String>,
    pub storage: StorageInfo,
    /// The Wi-Fi screen was opened from the YARG install screen (BACK returns there, not to SYSTEM).
    wifi_from_yarg: bool,
    pub drives: Vec<ImportDrive>,
    pub browse: Browse,
    /// SYSTEM > Diagnostics: which report, its lines, and how far it is scrolled.
    pub diag_page: usize,
    pub diag: Vec<String>,
    pub diag_top: usize,
    /// The folder just left with BACK: highlighted again when the list above it comes back.
    back_to: Option<String>,
    pub ask: Option<Ask>,
    /// The disk the user is being asked to confirm erasing (index into `disks`).
    pub erase: Option<usize>,
    /// Set once the install finished: what it was installed on.
    pub installed: Option<String>,
    pub install_error: Option<String>,
    pub alert: Option<String>,
    /// Label of the disk being installed on while the install runs.
    target: Option<String>,
    pub busy: Option<String>,
    /// What BACK does on the busy overlay, if the work can be stopped (a download or install).
    pub cancel: Option<Cmd>,
    pub os: OsInfo,
    /// The alert being shown is a rollback notice: dismissing it tells the updater.
    alert_ack: bool,
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
            wifi_on: true,
            nets: vec![],
            osk: Osk::new(""),
            outputs: vec![],
            cur_card_dev: None,
            cur_bt: None,
            volume: None,
            bt: vec![],
            bt_present: true,
            upd: UpdInfo::default(),
            songs: SongsInfo::default(),
            sysinfo: vec![],
            confirm: None,
            confirm_sel: 0,
            disks: vec![],
            devices: vec![],
            yarg_zips: vec![],
            storage: StorageInfo::default(),
            wifi_from_yarg: false,
            drives: vec![],
            browse: Browse::default(),
            back_to: None,
            diag_page: 0,
            diag: vec![],
            diag_top: 0,
            ask: None,
            erase: None,
            installed: None,
            install_error: None,
            alert: None,
            target: None,
            busy: None,
            cancel: None,
            os: OsInfo::default(),
            alert_ack: false,
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

    pub fn cancellable(&self) -> bool {
        self.cancel.is_some()
    }

    /// Nothing to play yet: the image has no YARG until it is installed from the dashboard.
    pub fn yarg_missing(&self) -> bool {
        self.status.yarg == "not installed"
    }

    /// The label of a main menu row (the first one becomes INSTALL YARG while there is no YARG).
    pub fn home_label(&self, i: usize) -> &'static str {
        if i == 0 && self.yarg_missing() { INSTALL_YARG } else { HOME_ITEMS[i] }
    }

    /// Number of selectable rows on the current list screen.
    pub fn row_count(&self) -> usize {
        match self.screen {
            Screen::Home => HOME_ITEMS.len(),
            Screen::Wifi => 2 + self.nets.len(), // the on/off switch, scan again, the networks
            Screen::Sound => self.outputs.len() + 2, // the outputs, the volume row (always there), the test sound
            Screen::Bluetooth => 1 + self.bt.len(),
            Screen::Updates => 4,
            Screen::UpdatesMenu => 2,
            Screen::Songs => 2 + usize::from(self.songs.mounted),
            Screen::System => SYSTEM_ROWS + 2,
            Screen::Diag => 0,
            Screen::Storage => self.storage_rows().len(),
            Screen::ImportPick => 1 + self.drives.len(),
            Screen::Manage => 1 + self.browse.entries.len(),
            Screen::OsUpdate => 7,
            Screen::Devices => 1 + self.devices.len(),
            Screen::Yarg => 3,
            Screen::Install => if self.installed.is_some() { 2 } else { 1 + self.disks.len() },
            Screen::Password => 0,
        }
    }

    /// The rows of the Storage screen now.
    pub fn storage_rows(&self) -> Vec<StorageRow> {
        let mut v = vec![StorageRow::Import, StorageRow::Manage, StorageRow::SongDrive, StorageRow::Venues];
        if self.storage.venues > 0 {
            v.push(StorageRow::RemoveVenues);
        }
        if self.storage.slack_kib >= SLACK_WORTH_KIB {
            v.push(StorageRow::UseRest);
        }
        if self.storage.on_stick {
            v.push(StorageRow::Install);
        }
        v
    }

    /// Go straight to a screen (the "Install" boot menu entry opens the installer).
    pub fn open(&mut self, screen: Screen) -> Vec<Cmd> {
        self.enter(screen)
    }

    fn enter(&mut self, screen: Screen) -> Vec<Cmd> {
        self.screen = screen;
        self.sel = 0;
        if screen == Screen::UpdatesMenu {
            // two choices, shown at once; both updaters are asked so that their NEW tags are right
            self.loading = false;
            return vec![Cmd::Refresh(Screen::OsUpdate), Cmd::Refresh(Screen::Updates)];
        }
        self.loading = screen != Screen::Home && screen != Screen::Password;
        if screen == Screen::Home { vec![Cmd::Refresh(Screen::Home)] } else { vec![Cmd::Refresh(screen)] }
    }

    /// Move the highlight to a row without activating it (mouse hover).
    pub fn hover(&mut self, id: usize) {
        if self.busy.is_some() || self.alert.is_some() {
            return;
        }
        if self.confirm.is_some() || self.erase.is_some() || self.ask.is_some() {
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
        if self.alert.is_some() {
            if matches!(nav, Nav::Ok | Nav::Submit | Nav::Back) {
                self.alert = None;
                if std::mem::take(&mut self.alert_ack) {
                    return vec![Cmd::OsAck];
                }
            }
            return vec![];
        }
        if self.busy.is_some() {
            if nav == Nav::Back {
                if let Some(c) = self.cancel.take() {
                    self.busy = Some("Cancelling\u{2026}".into());
                    return vec![c];
                }
            }
            return vec![]; // ignore input while an action runs
        }
        if self.ask.is_some() {
            return self.handle_ask(nav);
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

    /// A question with Cancel as the default: only OK on the second button runs it.
    fn handle_ask(&mut self, nav: Nav) -> Vec<Cmd> {
        match nav {
            Nav::Left | Nav::Right | Nav::Up | Nav::Down => self.confirm_sel = 1 - self.confirm_sel,
            Nav::Back => self.ask = None,
            Nav::Ok | Nav::Submit => {
                if let Some(a) = self.ask.take() {
                    if self.confirm_sel == 1 {
                        self.busy = Some(a.busy);
                        self.cancel = a.cancel;
                        return vec![a.cmd];
                    }
                }
            }
            _ => {}
        }
        vec![]
    }

    fn ask(&mut self, question: impl Into<String>, note: impl Into<String>, go: &str, danger: bool, busy: &str, cmd: Cmd) {
        self.ask = Some(Ask { question: question.into(), note: note.into(), go: go.into(), danger, busy: busy.into(), cancel: None, cmd });
        self.confirm_sel = 0;
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
        // SYSTEM: the two power icons are one line under the rows: DOWN from the last row reaches them, LEFT/RIGHT choose
        // between them, UP goes back to the last row.
        if self.screen == Screen::System {
            let on_icons = self.sel >= SYSTEM_ROWS;
            match nav {
                Nav::Down => {
                    self.sel = if on_icons { 0 } else { self.sel + 1 };
                    return vec![];
                }
                Nav::Up => {
                    self.sel = if self.sel == 0 { SYSTEM_ROWS } else if on_icons { SYSTEM_ROWS - 1 } else { self.sel - 1 };
                    return vec![];
                }
                Nav::Left | Nav::Right if on_icons => {
                    self.sel = if self.sel == SYSTEM_ROWS { SYSTEM_ROWS + 1 } else { SYSTEM_ROWS };
                    return vec![];
                }
                _ => {}
            }
        }
        match nav {
            Nav::Up if n > 0 => self.sel = (self.sel + n - 1) % n,
            Nav::Down if n > 0 => self.sel = (self.sel + 1) % n,
            Nav::Left | Nav::Right | Nav::Up | Nav::Down | Nav::Ok | Nav::Submit if self.screen == Screen::Diag => return self.handle_diag(nav),
            Nav::Back => match self.screen {
                Screen::Home => {}
                Screen::Diag => return self.back_to(Screen::System, 3),
                Screen::ImportPick => return self.enter(Screen::Storage),
                Screen::Manage => return self.manage_up(),
                Screen::Storage => return self.back_to(Screen::System, 2),
                Screen::Songs => return self.back_to(Screen::Storage, 2),
                Screen::Install => {
                    let cmds = self.back_to(Screen::Storage, 0);
                    self.sel = self.storage_rows().iter().position(|r| *r == StorageRow::Install).unwrap_or(0);
                    return cmds;
                }
                Screen::UpdatesMenu => return self.back_to(Screen::System, 1),
                Screen::OsUpdate => return self.back_to(Screen::UpdatesMenu, 0),
                Screen::Updates => return self.back_to(Screen::UpdatesMenu, 1),
                Screen::Wifi if std::mem::take(&mut self.wifi_from_yarg) => return self.enter(Screen::Yarg),
                Screen::Wifi => return self.back_to(Screen::System, 0),
                Screen::System => {
                    self.screen = Screen::Home;
                    self.sel = HOME_ITEMS.len() - 1;
                    return vec![Cmd::Refresh(Screen::Home)];
                }
                Screen::Sound | Screen::Bluetooth | Screen::Devices => {
                    // (back on the main menu with the row we came from highlighted)
                    let sel = match self.screen { Screen::Sound => 1, Screen::Bluetooth => 2, _ => 3 };
                    self.screen = Screen::Home;
                    self.sel = sel;
                    return vec![Cmd::Refresh(Screen::Home)];
                }
                _ => {
                    self.screen = Screen::Home;
                    self.sel = 0;
                    return vec![Cmd::Refresh(Screen::Home)];
                }
            },
            Nav::Left | Nav::Right => return self.adjust(nav == Nav::Right),
            Nav::Ok | Nav::Submit => return self.activate(),
            _ => {}
        }
        vec![]
    }

    /// BACK to a screen, with the row we came from highlighted.
    fn back_to(&mut self, screen: Screen, sel: usize) -> Vec<Cmd> {
        let cmds = self.enter(screen);
        self.sel = sel;
        cmds
    }

    /// Diagnostics: LEFT/RIGHT change the report, UP/DOWN scroll it, OK reads it again.
    fn handle_diag(&mut self, nav: Nav) -> Vec<Cmd> {
        let n = crate::sys::DIAG_PAGES.len();
        match nav {
            Nav::Left | Nav::Right => {
                self.diag_page = if nav == Nav::Right { (self.diag_page + 1) % n } else { (self.diag_page + n - 1) % n };
                self.diag = vec![];
                self.diag_top = 0;
                self.loading = true;
                return vec![Cmd::DiagLoad(self.diag_page)];
            }
            Nav::Up => self.diag_top = self.diag_top.saturating_sub(5),
            Nav::Down => self.diag_top = (self.diag_top + 5).min(self.diag.len().saturating_sub(5)),
            _ => {
                self.loading = true;
                return vec![Cmd::DiagLoad(self.diag_page)];
            }
        }
        vec![]
    }

    /// BACK in the song browser: up one folder (the one left is highlighted again), or out to Storage from the top.
    fn manage_up(&mut self) -> Vec<Cmd> {
        match self.browse.path.pop() {
            Some(name) => {
                self.back_to = Some(name);
                self.loading = true;
                vec![Cmd::SongsBrowse(self.browse.path.clone())]
            }
            None => self.enter(Screen::Storage),
        }
    }

    /// Ask before deleting `name` (a song or folder inside the folder being browsed), or, with None, the folder being
    /// browsed (everything, at the top).
    fn ask_delete(&mut self, name: Option<usize>) {
        let mut target = self.browse.path.clone();
        let (question, note) = match name.and_then(|i| self.browse.entries.get(i)).cloned() {
            Some(e) => {
                target.push(e.name.clone());
                match e.kind {
                    SongKind::Song => ("Delete this song?".to_string(), e.name),
                    SongKind::Folder => ("Delete this folder?".to_string(), format!("{}\nEverything inside it is deleted.", e.name)),
                    SongKind::File => ("Delete this file?".to_string(), e.name),
                }
            }
            None if target.is_empty() => ("Delete ALL internal songs?".to_string(), "The whole internal library is deleted.\nSongs on portable drives are not touched.".to_string()),
            None => ("Delete this whole folder?".to_string(), format!("{}\nEverything inside it is deleted.", target.join(" / "))),
        };
        self.ask(question, note, "Delete", true, "Deleting\u{2026}", Cmd::SongsDelete(target));
    }

    /// Left/Right on a row: volume, forgetting a Bluetooth device, deleting an internal song or folder.
    fn adjust(&mut self, right: bool) -> Vec<Cmd> {
        match self.screen {
            Screen::Manage if !right && self.sel >= 1 => {
                self.ask_delete(Some(self.sel - 1));
                vec![]
            }
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
                0 if self.yarg_missing() => self.enter(Screen::Yarg),
                0 => vec![Cmd::LaunchYarg],
                i => {
                    let s = [Screen::Sound, Screen::Bluetooth, Screen::Devices, Screen::System][i - 1];
                    self.enter(s)
                }
            },
            Screen::Wifi => {
                if self.sel == 0 {
                    self.busy = Some(if self.wifi_on { "Turning Wi-Fi off\u{2026}" } else { "Turning Wi-Fi on\u{2026}" }.into());
                    return vec![Cmd::WifiPower(!self.wifi_on)];
                }
                if self.sel == 1 {
                    if !self.wifi_on {
                        self.toast("Wi-Fi is off: turn it on first");
                        return vec![];
                    }
                    self.busy = Some("Scanning for networks…".into());
                    return vec![Cmd::WifiScan];
                }
                let net = self.nets[self.sel - 2].clone();
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
                    if let Some(mac) = o.bt.clone() {
                        self.cur_bt = Some(mac.clone());
                        self.cur_card_dev = None;
                        self.volume = None; // read again with the next refresh (it is the headset's level now)
                        return vec![Cmd::SoundSelectBt(mac)];
                    }
                    self.cur_card_dev = Some((o.card, o.dev));
                    self.cur_bt = None;
                    vec![Cmd::SoundSelect { card: o.card, dev: o.dev }]
                } else if self.sel == self.outputs.len() {
                    if self.volume.is_none() {
                        self.toast("This output has no volume control here: use the TV, receiver or headphones");
                    }
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
                } else if d.audio {
                    format!("Connecting {}… it must be in pairing mode (or switched on). This takes up to 30 seconds", d.name)
                } else {
                    format!("Connecting {}…", d.name)
                });
                vec![Cmd::BtConnect(d)]
            }
            Screen::Yarg => match self.sel {
                0 => {
                    self.busy = Some("Downloading YARG\u{2026} this can take a few minutes".into());
                    self.cancel = Some(Cmd::YargCancel);
                    vec![Cmd::YargInstall]
                }
                1 => match self.yarg_zips.first().cloned() {
                    Some(zip) => {
                        self.busy = Some("Installing YARG\u{2026}".into());
                        self.cancel = Some(Cmd::YargCancel);
                        vec![Cmd::YargInstallZip(zip)]
                    }
                    None => {
                        self.toast("No YARG zip found on the song drive");
                        vec![]
                    }
                },
                _ => {
                    self.wifi_from_yarg = true;
                    self.enter(Screen::Wifi)
                }
            },
            Screen::UpdatesMenu => match self.sel {
                0 => self.enter(Screen::OsUpdate),
                _ if self.yarg_missing() => self.enter(Screen::Yarg), // there is no YARG to update yet: install it
                _ => self.enter(Screen::Updates),
            },
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
                    self.cancel = Some(Cmd::YargCancel);
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
            Screen::Songs if self.songs.mounted && self.sel == 1 => {
                self.toast("The song drive is at /media/songs; YARG scans it each time it starts");
                vec![]
            }
            Screen::Songs => {
                self.songs.auto = !self.songs.auto;
                vec![Cmd::SongsAuto(self.songs.auto)]
            }
            Screen::System if self.sel == 0 => {
                self.wifi_from_yarg = false;
                self.enter(Screen::Wifi)
            }
            Screen::System if self.sel == 1 => self.enter(Screen::UpdatesMenu),
            Screen::System if self.sel == 2 => self.enter(Screen::Storage),
            Screen::System if self.sel == 3 => {
                self.diag = vec![];
                self.diag_page = 0;
                self.diag_top = 0;
                self.enter(Screen::Diag)
            }
            Screen::Storage => match self.storage_rows().get(self.sel).copied() {
                Some(StorageRow::Import) => self.enter(Screen::ImportPick),
                Some(StorageRow::Manage) => {
                    self.browse = Browse::default();
                    self.enter(Screen::Manage)
                }
                Some(StorageRow::SongDrive) => self.enter(Screen::Songs),
                Some(StorageRow::Venues) => {
                    self.busy = Some("Looking for venues on the song drive\u{2026}".into());
                    vec![Cmd::VenuesScan]
                }
                Some(StorageRow::RemoveVenues) => {
                    let n = self.storage.venues;
                    self.ask(
                        format!("Remove {} venue file{}?", thousands(n), if n == 1 { "" } else { "s" }),
                        "Everything in YARG's venue folder is deleted. YARG uses its built-in venue again.",
                        "Remove",
                        true,
                        "Removing venues\u{2026}",
                        Cmd::VenuesRemove,
                    );
                    vec![]
                }
                Some(StorageRow::UseRest) => {
                    self.busy = Some("Using the rest of the drive\u{2026}".into());
                    vec![Cmd::GrowData]
                }
                Some(StorageRow::Install) => self.enter(Screen::Install),
                None => vec![],
            },
            Screen::ImportPick if self.sel == 0 => {
                self.busy = Some("Looking for drives\u{2026}".into());
                vec![Cmd::Refresh(Screen::ImportPick)]
            }
            Screen::ImportPick => {
                let d = self.drives[self.sel - 1].clone();
                self.busy = Some(format!("Reading {}\u{2026} a big drive can take a minute", d.name()));
                self.cancel = Some(Cmd::ImportCancel);
                vec![Cmd::ImportScan(d)]
            }
            Screen::Manage if self.sel == 0 => {
                if self.browse.path.is_empty() && self.browse.entries.is_empty() {
                    self.toast("There are no internal songs to delete");
                } else {
                    self.ask_delete(None);
                }
                vec![]
            }
            Screen::Manage => {
                let i = self.sel - 1;
                match self.browse.entries[i].kind {
                    SongKind::Folder => {
                        self.browse.path.push(self.browse.entries[i].name.clone());
                        self.sel = 0;
                        self.loading = true;
                        vec![Cmd::SongsBrowse(self.browse.path.clone())]
                    }
                    _ => {
                        self.ask_delete(Some(i));
                        vec![]
                    }
                }
            }
            Screen::OsUpdate => match self.sel {
                0 => {
                    self.busy = Some("Checking for a system update\u{2026}".into());
                    vec![Cmd::OsCheck]
                }
                1 => {
                    if self.os.available.is_none() {
                        self.toast(if self.os.staged.is_some() { "An update is already waiting for a restart" } else { "The system is up to date" });
                        return vec![];
                    }
                    self.busy = Some("Downloading the update\u{2026} this can take a few minutes".into());
                    self.cancel = Some(Cmd::OsCancel);
                    vec![Cmd::OsInstall]
                }
                2 => {
                    if self.os.staged.is_some() {
                        self.toast("An update is already waiting for a restart");
                        return vec![];
                    }
                    if self.os.offline.is_none() {
                        // nothing known yet: look at the drive again (it may have been plugged in after this screen opened)
                        self.busy = Some(LOOKING_ON_DRIVE.into());
                        return vec![Cmd::OsFindOffline];
                    }
                    self.busy = Some("Copying the update from the song drive\u{2026}".into());
                    self.cancel = Some(Cmd::OsCancel);
                    vec![Cmd::OsInstallOffline]
                }
                3 => {
                    if self.os.staged.is_none() {
                        self.toast("There is no update waiting");
                        return vec![];
                    }
                    self.confirm = Some(Power::Restart);
                    self.confirm_sel = 0;
                    vec![]
                }
                4 => {
                    self.os.auto = !self.os.auto;
                    vec![Cmd::OsAuto(self.os.auto)]
                }
                5 => {
                    if self.os.previous.is_none() {
                        self.toast("There is no earlier system to go back to");
                        return vec![];
                    }
                    self.busy = Some("Going back to the earlier system\u{2026}".into());
                    vec![Cmd::OsGoBack]
                }
                _ => {
                    self.os.beta = !self.os.beta;
                    vec![Cmd::OsChannel(self.os.beta)]
                }
            },
            Screen::Devices if self.sel == 0 => {
                self.busy = Some("Looking at what is plugged in\u{2026}".into());
                vec![Cmd::Refresh(Screen::Devices)]
            }
            Screen::Devices => {
                let d = self.devices[self.sel - 1].clone();
                self.toast(format!("{}: {}", d.name, d.detail));
                vec![]
            }
            Screen::System => {
                // the icons: Restart, then Shut down
                self.confirm = Some(if self.sel == SYSTEM_ROWS { Power::Restart } else { Power::Shutdown });
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
            Screen::Password | Screen::Diag => vec![],
        }
    }

    pub fn apply(&mut self, res: Res) {
        match res {
            Res::Status(s) => {
                // A system update that did not start properly (and was undone) is never shown only as a status line.
                if let (Some(v), None) = (&s.os_rolled_back, &self.alert) {
                    self.alert = Some(format!("The system update to {v} did not start properly, so the earlier system was put back. Nothing was lost. The update will not be tried again."));
                    self.alert_ack = true;
                }
                self.status = s;
            }
            Res::OsOffline { offline, hint } => {
                let looking = self.busy.as_deref() == Some(LOOKING_ON_DRIVE);
                self.os.offline = offline.clone();
                self.os.offline_hint = hint.clone();
                if looking {
                    self.busy = None;
                    match offline {
                        Some(v) => self.toast(format!("Found {v} on the song drive: choose Install from the song drive again to install it")),
                        None => self.toast(hint.unwrap_or_else(|| "No update found on the song drive".into())),
                    }
                }
            }
            Res::WifiPower(on) => {
                self.wifi_on = on;
                self.busy = None;
                if !on {
                    self.nets.clear();
                }
                self.sel = self.sel.min(self.row_count().saturating_sub(1));
            }
            Res::Wifi { dev, nets } => {
                self.wifi_dev = dev;
                self.nets = nets;
                self.loading = false;
                self.busy = None;
                self.sel = self.sel.min(self.row_count().saturating_sub(1));
            }
            Res::Sound { outputs, current, current_bt, volume } => {
                self.outputs = outputs;
                self.cur_card_dev = current;
                self.cur_bt = current_bt;
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
            Res::Devices(d) => {
                self.devices = d;
                self.loading = false;
                self.busy = None;
                self.sel = self.sel.min(self.row_count().saturating_sub(1));
            }
            Res::Storage(info) => {
                self.storage = info;
                self.loading = false;
            }
            Res::Drives(d) => {
                self.drives = d;
                self.loading = false;
                self.busy = None;
                self.sel = self.sel.min(self.row_count().saturating_sub(1));
            }
            Res::ImportPlan(r) => {
                self.busy = None;
                self.cancel = None;
                match r {
                    Err(e) => self.alert = Some(e),
                    Ok(p) if p.todo_files == 0 => self.toast(format!("Everything on {} is already on the internal drive", p.drive.name())),
                    Ok(p) if !p.fits() => {
                        let usable = p.free_kib.saturating_sub(p.reserve_kib);
                        self.alert = Some(format!(
                            "There is not enough room on the internal drive. Copying needs {}, but only {} can be used ({} is kept free for the system).",
                            human_kib(p.todo_bytes / 1024),
                            human_kib(usable),
                            human_kib(p.reserve_kib)
                        ));
                    }
                    Ok(p) => {
                        let mut note = format!("{} in {} files", human_kib(p.todo_bytes / 1024), thousands(p.todo_files));
                        if p.todo_files < p.files {
                            note.push_str(&format!(" ({} already there)", thousands(p.files - p.todo_files)));
                        }
                        note.push_str(&format!("\n{} left for songs afterwards", human_kib((p.free_kib - p.todo_bytes / 1024).saturating_sub(p.reserve_kib))));
                        let q = format!("Copy \"{}\"?", p.drive.name());
                        self.ask(q, note, "Copy", false, &format!("Copying {}\u{2026} 0%", p.drive.name()), Cmd::ImportRun(p));
                        if let Some(a) = &mut self.ask {
                            a.cancel = Some(Cmd::ImportCancel);
                        }
                    }
                }
            }
            Res::VenuesPlan(r) => {
                self.busy = None;
                match r {
                    Err(e) => self.toast(e),
                    Ok(p) if p.todo == 0 => self.toast(if p.found == 1 {
                        "The venue file on the song drive is already added".to_string()
                    } else {
                        format!("All {} venue files on the song drive are already added", thousands(p.found))
                    }),
                    Ok(p) if !p.fits() => {
                        self.alert = Some(format!(
                            "There is not enough room on the internal drive. The venues need {}, but only {} can be used.",
                            human_kib(p.todo_bytes / 1024),
                            human_kib(p.free_kib.saturating_sub(p.reserve_kib))
                        ));
                    }
                    Ok(p) => {
                        let note = format!(
                            "{}. YARG picks one at random for each song",
                            human_kib(p.todo_bytes / 1024)
                        );
                        self.ask(format!("Add {} venue file{}?", thousands(p.todo), if p.todo == 1 { "" } else { "s" }), note, "Add", false, "Adding venues\u{2026}", Cmd::VenuesRun);
                    }
                }
            }
            Res::VenuesDone(r) => {
                self.busy = None;
                match r {
                    Ok(msg) => self.toast(msg),
                    Err(e) => self.alert = Some(e),
                }
            }
            Res::ImportDone(r) => {
                self.busy = None;
                self.cancel = None;
                match r {
                    Ok(msg) => self.toast(msg),
                    Err(e) => self.alert = Some(e),
                }
            }
            Res::Diag { page, lines } => {
                if page == self.diag_page {
                    self.diag = lines;
                    self.diag_top = 0;
                    self.loading = false;
                }
            }
            Res::Browse { path, entries } => {
                let back = self.back_to.take();
                self.browse = Browse { path, entries };
                self.loading = false;
                self.busy = None;
                self.sel = match back.and_then(|n| self.browse.entries.iter().position(|e| e.name == n)) {
                    Some(i) => i + 1,
                    None => self.sel.min(self.row_count().saturating_sub(1)),
                };
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
            Res::Alert(msg) => {
                self.busy = None;
                self.alert = Some(msg);
            }
            Res::Os(info) => {
                self.os = info;
                self.loading = false;
                self.busy = None;
                self.cancel = None;
                self.sel = self.sel.min(self.row_count().saturating_sub(1));
            }
            Res::OsDone(r) => {
                self.busy = None;
                self.cancel = None;
                match r {
                    Ok(msg) => {
                        self.toast(msg);
                        self.confirm = Some(Power::Restart);
                        self.confirm_sel = 0;
                    }
                    Err(e) => self.alert = Some(e),
                }
            }
            Res::YargZips(z) => {
                self.yarg_zips = z;
                self.loading = false;
            }
            Res::Progress(t) => {
                if self.busy.is_some() {
                    self.busy = Some(t);
                }
            }
            Res::YargInstalled(r) => {
                self.busy = None;
                self.cancel = None;
                match r {
                    Ok(msg) => {
                        self.screen = Screen::Home;
                        self.sel = 0;
                        self.toast(msg);
                    }
                    Err(e) => self.alert = Some(e),
                }
            }
            Res::Done(msg) => {
                self.busy = None;
                self.cancel = None;
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
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Sound)]);
        assert_eq!(m.screen, Screen::Sound);
        assert_eq!(m.handle(Nav::Back), vec![Cmd::Refresh(Screen::Home)]);
        assert_eq!((m.screen, m.sel), (Screen::Home, 1), "back on the row it came from");
    }

    #[test]
    fn the_main_menu_has_play_sound_bluetooth_devices_and_system() {
        assert_eq!(HOME_ITEMS, ["PLAY YARG", "SOUND", "BLUETOOTH", "CONNECTED DEVICES", "SYSTEM"]);
        let mut m = Model::new();
        m.sel = 3;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Devices)]);
        assert_eq!(m.handle(Nav::Back), vec![Cmd::Refresh(Screen::Home)]);
        assert_eq!((m.screen, m.sel), (Screen::Home, 3));
        m.sel = 4;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::System)]);
        assert_eq!(m.handle(Nav::Back), vec![Cmd::Refresh(Screen::Home)]);
        assert_eq!((m.screen, m.sel), (Screen::Home, 4));
    }

    #[test]
    fn wifi_is_the_first_row_of_system_and_back_returns_there() {
        let mut m = Model::new();
        m.screen = Screen::System;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Wifi)]);
        assert_eq!(m.screen, Screen::Wifi);
        assert_eq!(m.handle(Nav::Back), vec![Cmd::Refresh(Screen::System)]);
        assert_eq!((m.screen, m.sel), (Screen::System, 0));
    }

    #[test]
    fn wifi_opened_from_the_yarg_install_screen_goes_back_to_it() {
        let mut m = without_yarg();
        m.screen = Screen::Yarg;
        m.sel = 2;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Wifi)]);
        assert_eq!(m.handle(Nav::Back), vec![Cmd::Refresh(Screen::Yarg)]);
        assert_eq!(m.screen, Screen::Yarg);
        // and the flag does not stick: Wi-Fi from SYSTEM goes back to SYSTEM
        m.screen = Screen::System;
        m.sel = 0;
        m.handle(Nav::Ok);
        assert_eq!(m.handle(Nav::Back), vec![Cmd::Refresh(Screen::System)]);
    }

    #[test]
    fn the_power_icons_are_one_stop_below_the_system_rows() {
        let mut m = Model::new();
        m.screen = Screen::System;
        assert_eq!(m.row_count(), 6);
        for want in [1, 2, 3, 4] {
            m.handle(Nav::Down);
            assert_eq!(m.sel, want);
        }
        // on the icons: Left/Right choose, Up returns to the last row, Down wraps to the top
        m.handle(Nav::Right);
        assert_eq!(m.sel, 5);
        m.handle(Nav::Right);
        assert_eq!(m.sel, 4, "it toggles between the two");
        m.handle(Nav::Right);
        m.handle(Nav::Up);
        assert_eq!(m.sel, 3, "from either icon UP is the last row");
        m.handle(Nav::Down);
        m.handle(Nav::Down);
        assert_eq!(m.sel, 0, "DOWN from the icons wraps to the first row");
        m.handle(Nav::Up);
        assert_eq!(m.sel, 4, "UP from the first row reaches the icons");
        // OK on an icon asks first
        assert!(m.handle(Nav::Ok).is_empty());
        assert_eq!(m.confirm, Some(Power::Restart));
        m.confirm = None;
        m.sel = 5;
        m.handle(Nav::Ok);
        assert_eq!(m.confirm, Some(Power::Shutdown));
        // a click on an icon does the same
        m.confirm = None;
        assert!(m.click(4).is_empty());
        assert_eq!(m.confirm, Some(Power::Restart));
    }

    #[test]
    fn updates_is_a_menu_of_system_updates_and_yarg_updates() {
        let mut m = Model::new();
        m.screen = Screen::System;
        m.sel = 1;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::OsUpdate), Cmd::Refresh(Screen::Updates)]);
        assert_eq!((m.screen, m.loading), (Screen::UpdatesMenu, false));
        assert_eq!(m.row_count(), 2);
        // System updates, and back to the menu with that row highlighted
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::OsUpdate)]);
        assert_eq!(m.screen, Screen::OsUpdate);
        m.apply(Res::Os(OsInfo { running: "1".into(), auto: true, ..Default::default() }));
        assert_eq!(m.handle(Nav::Back), vec![Cmd::Refresh(Screen::OsUpdate), Cmd::Refresh(Screen::Updates)]);
        assert_eq!((m.screen, m.sel), (Screen::UpdatesMenu, 0));
        // YARG updates (the screen that used to be on the main menu), and back
        m.sel = 1;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Updates)]);
        assert_eq!(m.screen, Screen::Updates);
        m.apply(Res::Upd(UpdInfo { installed: "0.15.0".into(), auto: true, ..Default::default() }));
        m.handle(Nav::Back);
        assert_eq!((m.screen, m.sel), (Screen::UpdatesMenu, 1));
        m.handle(Nav::Back);
        assert_eq!((m.screen, m.sel), (Screen::System, 1));
    }

    #[test]
    fn secured_unknown_network_opens_the_password_screen() {
        let mut m = at_wifi();
        m.sel = 2; // Home (row 0 is "scan")
        assert!(m.handle(Nav::Ok).is_empty());
        assert_eq!(m.screen, Screen::Password);
        assert_eq!(m.osk.ssid, "Home");
    }

    #[test]
    fn open_and_saved_networks_connect_without_a_password() {
        let mut m = at_wifi();
        m.sel = 3;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::WifiConnect { ssid: "Cafe".into(), pass: None }]);
        let mut m = at_wifi();
        m.sel = 4;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::WifiConnect { ssid: "Saved".into(), pass: None }]);
    }

    #[test]
    fn connected_network_is_not_reconnected() {
        let mut m = at_wifi();
        m.sel = 5;
        assert!(m.handle(Nav::Ok).is_empty());
        assert!(m.live_toast().unwrap().contains("Already connected"));
    }

    #[test]
    fn typing_a_password_on_a_physical_keyboard_and_submitting() {
        let mut m = at_wifi();
        m.sel = 2;
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
        m.sel = 2;
        m.handle(Nav::Ok);
        m.handle(Nav::Char('a'));
        assert!(m.handle(Nav::Submit).is_empty());
        assert_eq!(m.screen, Screen::Password);
    }

    #[test]
    fn onscreen_keyboard_can_type_with_only_a_d_pad_and_ok() {
        let mut m = at_wifi();
        m.sel = 2;
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
        m.sel = 2;
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
        assert_eq!(m.click(1), vec![Cmd::Refresh(Screen::Sound)]);
        assert_eq!(m.screen, Screen::Sound);
    }

    #[test]
    fn clicking_onscreen_keys_types_them_and_clicking_confirm_buttons_works() {
        let mut m = at_wifi();
        m.sel = 2;
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
        m.sel = 5;
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
        m.outputs = vec![Output { card: 0, dev: 3, label: "HDMI".into(), bt: None }];
        m.volume = Some((0, 50));
        m.sel = 1;
        assert_eq!(m.handle(Nav::Right), vec![Cmd::SoundVolume { card: 0, pct: 55 }]);
        m.volume = Some((0, 2));
        assert_eq!(m.handle(Nav::Left), vec![Cmd::SoundVolume { card: 0, pct: 0 }]);
    }

    fn without_yarg() -> Model {
        let mut m = Model::new();
        m.status.yarg = "not installed".into();
        m
    }

    #[test]
    fn with_no_yarg_the_first_row_installs_it_instead_of_playing() {
        let mut m = Model::new();
        m.status.yarg = "0.15.0".into();
        assert_eq!(m.home_label(0), "PLAY YARG");
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::LaunchYarg]);

        let mut m = without_yarg();
        assert_eq!(m.home_label(0), "INSTALL YARG");
        assert_eq!(m.home_label(1), "SOUND");
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Yarg)]);
        assert_eq!(m.screen, Screen::Yarg);
    }

    #[test]
    fn yarg_updates_with_no_yarg_leads_to_the_install_screen() {
        let mut m = without_yarg();
        m.screen = Screen::UpdatesMenu;
        m.sel = 1;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Yarg)]);
        assert_eq!(m.screen, Screen::Yarg);
    }

    #[test]
    fn the_install_screen_downloads_installs_from_a_zip_or_goes_to_wifi() {
        let mut m = without_yarg();
        m.screen = Screen::Yarg;
        m.sel = 1;
        assert!(m.handle(Nav::Ok).is_empty(), "no zip yet");
        assert!(m.busy.is_none());
        m.apply(Res::YargZips(vec!["/media/songs/YARG_v0.15.0-Linux-x86_64.zip".into()]));
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::YargInstallZip("/media/songs/YARG_v0.15.0-Linux-x86_64.zip".into())]);
        assert!(m.busy.is_some());
        m.busy = None;
        m.sel = 0;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::YargInstall]);
        assert!(m.busy.as_ref().unwrap().contains("Downloading"));
        m.apply(Res::Progress("Downloading YARG\u{2026} 40%".into()));
        assert!(m.busy.as_ref().unwrap().contains("40%"));
        m.busy = None;
        m.apply(Res::Progress("late line".into()));
        assert!(m.busy.is_none(), "progress after the end does not bring the overlay back");
        m.sel = 2;
        m.handle(Nav::Ok);
        assert_eq!(m.screen, Screen::Wifi);
    }

    #[test]
    fn a_finished_install_returns_home_and_a_failed_one_raises_an_alert() {
        let mut m = without_yarg();
        m.screen = Screen::Yarg;
        m.busy = Some("x".into());
        m.apply(Res::YargInstalled(Err("Could not reach the internet.".into())));
        assert!(m.busy.is_none());
        assert_eq!(m.alert.as_deref(), Some("Could not reach the internet."));
        assert_eq!(m.screen, Screen::Yarg);
        m.alert = None;
        m.busy = Some("x".into());
        m.apply(Res::YargInstalled(Ok("YARG is installed. Choose PLAY YARG".into())));
        assert_eq!(m.screen, Screen::Home);
        assert!(m.live_toast().is_some());
    }

    #[test]
    fn back_cancels_a_yarg_download_but_not_other_busy_work() {
        let mut m = without_yarg();
        m.screen = Screen::Yarg;
        m.sel = 0;
        m.handle(Nav::Ok);
        assert!(m.cancellable());
        assert_eq!(m.handle(Nav::Back), vec![Cmd::YargCancel]);
        assert!(m.busy.as_ref().unwrap().contains("Cancelling"));
        assert!(m.handle(Nav::Back).is_empty(), "only once");
        m.apply(Res::YargInstalled(Err("Installation cancelled.".into())));
        assert!(m.busy.is_none() && !m.cancellable());
        // Some other kind of busy: BACK does nothing.
        m.alert = None;
        m.busy = Some("Scanning\u{2026}".into());
        assert!(m.handle(Nav::Back).is_empty());
        assert!(m.busy.is_some());
    }

    fn os_screen() -> Model {
        let mut m = Model::new();
        m.screen = Screen::OsUpdate;
        m.os = OsInfo { running: "1".into(), auto: true, ..Default::default() };
        m
    }

    #[test]
    fn test_versions_are_a_switch_on_the_system_updates_screen() {
        let mut m = os_screen();
        assert_eq!(m.row_count(), 7);
        assert!(!m.os.beta);
        m.sel = 6;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::OsChannel(true)]);
        assert!(m.os.beta);
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::OsChannel(false)]);
        assert!(!m.os.beta);
    }

    #[test]
    fn the_updates_menu_leads_to_system_updates() {
        let mut m = Model::new();
        m.screen = Screen::UpdatesMenu;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::OsUpdate)]);
        assert_eq!(m.screen, Screen::OsUpdate);
    }

    #[test]
    fn system_update_rows_only_act_when_there_is_something_to_do() {
        let mut m = os_screen();
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::OsCheck]);
        m.busy = None;
        m.sel = 1;
        assert!(m.handle(Nav::Ok).is_empty(), "nothing available");
        m.sel = 2;
        // nothing known on the song drive: OK looks at the drive again, and says what it found
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::OsFindOffline]);
        assert_eq!(m.busy.as_deref(), Some(LOOKING_ON_DRIVE));
        m.apply(Res::OsOffline { offline: None, hint: None });
        assert!(m.busy.is_none());
        assert!(m.live_toast().unwrap().contains("No update found"));
        m.sel = 3;
        assert!(m.handle(Nav::Ok).is_empty(), "nothing waiting");
        assert!(m.confirm.is_none());
        m.sel = 5;
        assert!(m.handle(Nav::Ok).is_empty(), "no earlier system");
        m.sel = 4;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::OsAuto(false)]);
        assert!(!m.os.auto);
    }

    #[test]
    fn an_update_on_the_song_drive_installs_without_internet_and_can_be_cancelled() {
        let mut m = os_screen();
        m.os.offline = Some("2".into());
        m.sel = 2;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::OsInstallOffline]);
        assert!(m.busy.as_deref().unwrap().contains("song drive") && m.cancellable());
        assert_eq!(m.handle(Nav::Back), vec![Cmd::OsCancel]);
        m.apply(Res::OsDone(Ok("The update is installed. Restart to use it".into())));
        assert_eq!(m.confirm, Some(Power::Restart));
        // Something on the drive that cannot be used says why instead of doing anything.
        let mut m = os_screen();
        m.os.offline_hint = Some("The file on the song drive is not signed by us.".into());
        m.sel = 2;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::OsFindOffline]);
        m.apply(Res::OsOffline { offline: None, hint: Some("The file on the song drive is not signed by us.".into()) });
        assert!(m.live_toast().unwrap().contains("not signed"));
        // A drive plugged in after the screen opened is found when OK looks again, and a second OK installs it.
        let mut m = os_screen();
        m.sel = 2;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::OsFindOffline]);
        m.apply(Res::OsOffline { offline: Some("2026.11.1".into()), hint: None });
        assert!(m.busy.is_none() && m.live_toast().unwrap().contains("Found 2026.11.1"));
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::OsInstallOffline]);
        // The screen's own look every few seconds changes the row quietly: it never clears something else that is busy.
        let mut m = os_screen();
        m.busy = Some("Downloading the update".into());
        m.apply(Res::OsOffline { offline: Some("3".into()), hint: None });
        assert_eq!(m.busy.as_deref(), Some("Downloading the update"));
        assert_eq!(m.os.offline.as_deref(), Some("3"));
        assert!(m.live_toast().is_none());
        // An update already waiting for a restart is not stacked up with another.
        let mut m = os_screen();
        m.os.offline = Some("2".into());
        m.os.staged = Some("2".into());
        m.sel = 2;
        assert!(m.handle(Nav::Ok).is_empty() && m.busy.is_none());
    }

    #[test]
    fn installing_a_system_update_can_be_cancelled_and_ends_in_a_restart_question() {
        let mut m = os_screen();
        m.os.available = Some("2".into());
        m.sel = 1;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::OsInstall]);
        assert!(m.cancellable());
        assert_eq!(m.handle(Nav::Back), vec![Cmd::OsCancel]);
        m.apply(Res::OsDone(Err("Update cancelled. Nothing was changed.".into())));
        assert!(m.alert.is_some() && m.busy.is_none() && m.confirm.is_none());
        m.alert = None;
        m.busy = Some("x".into());
        m.apply(Res::OsDone(Ok("The update is installed. Restart to use it".into())));
        assert_eq!(m.confirm, Some(Power::Restart));
        m.confirm = None;
        m.os.staged = Some("2".into());
        m.os.available = None;
        m.sel = 3;
        m.handle(Nav::Ok);
        assert_eq!(m.confirm, Some(Power::Restart), "restart to use it asks first");
    }

    #[test]
    fn going_back_needs_an_earlier_system() {
        let mut m = os_screen();
        m.os.previous = Some("0".into());
        m.sel = 5;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::OsGoBack]);
    }

    #[test]
    fn a_rolled_back_update_raises_an_alert_that_is_acknowledged_once() {
        let mut m = Model::new();
        m.apply(Res::Status(Status { os_rolled_back: Some("2".into()), ..Default::default() }));
        assert!(m.alert.as_ref().unwrap().contains("2"));
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::OsAck]);
        assert!(m.alert.is_none());
        // Another alert (no rollback) does not ask for an acknowledgement.
        m.alert = Some("Something else".into());
        assert!(m.handle(Nav::Ok).is_empty());
    }

    #[test]
    fn choosing_an_output_selects_it() {
        let mut m = Model::new();
        m.screen = Screen::Sound;
        m.outputs = vec![
            Output { card: 0, dev: 3, label: "HDMI".into(), bt: None },
            Output { card: 1, dev: 0, label: "USB".into(), bt: None },
        ];
        m.sel = 1;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::SoundSelect { card: 1, dev: 0 }]);
        assert_eq!(m.cur_card_dev, Some((1, 0)));
    }

    #[test]
    fn diagnostics_pages_are_read_changed_scrolled_and_left_with_back() {
        let mut m = Model::new();
        m.screen = Screen::System;
        m.sel = 3;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Diag)]);
        assert_eq!(m.screen, Screen::Diag);
        m.apply(Res::Diag { page: 0, lines: (0..40).map(|i| format!("line {i}")).collect() });
        assert!(!m.loading && m.diag.len() == 40);
        assert_eq!(m.handle(Nav::Down), vec![]);
        assert_eq!(m.diag_top, 5);
        assert_eq!(m.handle(Nav::Up), vec![]);
        m.handle(Nav::Up);
        assert_eq!(m.diag_top, 0);
        // The next report; a late answer for the page left is ignored.
        assert_eq!(m.handle(Nav::Right), vec![Cmd::DiagLoad(1)]);
        m.apply(Res::Diag { page: 0, lines: vec!["old".into()] });
        assert!(m.diag.is_empty());
        m.apply(Res::Diag { page: 1, lines: vec!["sound".into()] });
        assert_eq!(m.diag, vec!["sound".to_string()]);
        assert_eq!(m.handle(Nav::Left), vec![Cmd::DiagLoad(0)]);
        assert_eq!(m.handle(Nav::Left), vec![Cmd::DiagLoad(4)], "it wraps round");
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::DiagLoad(4)], "OK reads it again");
        assert_eq!(m.handle(Nav::Back), vec![Cmd::Refresh(Screen::System)]);
        assert_eq!((m.screen, m.sel), (Screen::System, 3));
    }

    #[test]
    fn the_volume_row_is_always_there_and_says_so_when_the_output_has_none() {
        let mut m = Model::new();
        m.screen = Screen::Sound;
        m.outputs = vec![Output { card: 0, dev: 3, label: "HDMI".into(), bt: None }];
        assert_eq!(m.row_count(), 3, "the output, the volume row and the test sound");
        m.sel = 1; // the volume row with no volume control (HDMI)
        assert!(m.handle(Nav::Ok).is_empty());
        assert!(m.live_toast().unwrap().contains("no volume control"));
        assert!(m.handle(Nav::Right).is_empty(), "nothing to move");
        m.sel = 2;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::SoundTest]);
        // A headset's level moves the same way as a card's, marked with BT_VOLUME.
        m.volume = Some((BT_VOLUME, 50));
        m.sel = 1;
        assert_eq!(m.handle(Nav::Right), vec![Cmd::SoundVolume { card: BT_VOLUME, pct: 55 }]);
    }

    #[test]
    fn a_bluetooth_headset_is_an_output_and_choosing_it_drops_the_volume_slider() {
        let mut m = Model::new();
        m.screen = Screen::Sound;
        m.outputs = vec![
            Output { card: 1, dev: 0, label: "Analog".into(), bt: None },
            Output { card: 0, dev: 0, label: "Liberty 4 (Bluetooth)".into(), bt: Some("08:DF:1F:AA:BB:CC".into()) },
        ];
        m.cur_card_dev = Some((1, 0));
        m.volume = Some((1, 70));
        m.sel = 1;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::SoundSelectBt("08:DF:1F:AA:BB:CC".into())]);
        assert_eq!((m.cur_bt.as_deref(), m.cur_card_dev, m.volume), (Some("08:DF:1F:AA:BB:CC"), None, None));
        // Back to a sound card forgets the headset.
        m.sel = 0;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::SoundSelect { card: 1, dev: 0 }]);
        assert!(m.cur_bt.is_none() && m.cur_card_dev == Some((1, 0)));
        // The refresh after a change carries the headset as the current output.
        m.apply(Res::Sound { outputs: m.outputs.clone(), current: None, current_bt: Some("08:DF:1F:AA:BB:CC".into()), volume: None });
        assert_eq!(m.cur_bt.as_deref(), Some("08:DF:1F:AA:BB:CC"));
    }

    #[test]
    fn connecting_headphones_says_they_must_be_in_pairing_mode_and_takes_a_while() {
        let mut m = Model::new();
        m.screen = Screen::Bluetooth;
        m.bt = vec![BtDevice { mac: "08:DF:1F:AA:BB:CC".into(), name: "Liberty 4".into(), paired: false, connected: false, ready: true, battery: None, battery_low: false, audio: true }];
        m.sel = 1;
        let cmds = m.handle(Nav::Ok);
        assert!(matches!(cmds.as_slice(), [Cmd::BtConnect(d)] if d.audio));
        assert!(m.busy.as_deref().unwrap().contains("pairing mode"));
    }

    #[test]
    fn bluetooth_wiimote_hint_and_forgetting() {
        let mut m = Model::new();
        m.screen = Screen::Bluetooth;
        m.bt = vec![
            BtDevice { mac: "00:22:D7:AD:BE:D4".into(), name: "Nintendo RVL-CNT-01".into(), paired: false, connected: false, ready: true, battery: None, battery_low: false, audio: false },
            BtDevice { mac: "AA".into(), name: "Keyboard".into(), paired: true, connected: true, ready: true, battery: None, battery_low: false, audio: false },
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
        m.sel = 5;
        m.busy = Some("x".into());
        m.apply(Res::Wifi { dev: Some("wlan0".into()), nets: vec![] });
        assert!(m.busy.is_none());
        assert_eq!(m.sel, 1, "the on/off row and the scan row are always there");
    }

    #[test]
    fn wifi_has_an_on_off_row_and_off_means_no_networks_and_no_scanning() {
        let mut m = at_wifi();
        assert_eq!(m.row_count(), 6, "on/off, scan again, four networks");
        // OK on the first row turns Wi-Fi off
        m.sel = 0;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::WifiPower(false)]);
        assert!(m.busy.is_some());
        m.apply(Res::WifiPower(false));
        assert!(!m.wifi_on && m.busy.is_none() && m.nets.is_empty());
        assert_eq!(m.row_count(), 2);
        // scanning while it is off only says so
        m.sel = 1;
        assert!(m.handle(Nav::Ok).is_empty());
        assert!(m.live_toast().unwrap().contains("turn it on first"));
        // and OK on the first row turns it on again
        m.sel = 0;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::WifiPower(true)]);
        m.apply(Res::WifiPower(true));
        assert!(m.wifi_on);
        m.sel = 1;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::WifiScan]);
    }

    fn disks() -> Vec<Disk> {
        vec![
            Disk { path: "/dev/sda".into(), bytes: 256_000_000_000, model: "Internal SSD".into(), removable: false },
            Disk { path: "/dev/sdb".into(), bytes: 1_000_000_000_000, model: "Portable".into(), removable: true },
        ]
    }

    #[test]
    fn storage_leads_to_the_installer_only_while_running_from_a_stick() {
        let mut m = Model::new();
        m.screen = Screen::Storage;
        assert!(!m.storage_rows().contains(&StorageRow::Install), "no row until we know it is a stick");
        m.apply(Res::Storage(StorageInfo { on_stick: true, ..Default::default() }));
        assert_eq!(m.row_count(), 5);
        m.sel = 4;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Install)]);
        assert_eq!(m.screen, Screen::Install);
        assert_eq!(m.handle(Nav::Back), vec![Cmd::Refresh(Screen::Storage)]);
        assert_eq!((m.screen, m.sel), (Screen::Storage, 4), "back on the install row");
        // installed on the computer's own disk: the row is gone
        m.apply(Res::Storage(StorageInfo { on_stick: false, ..Default::default() }));
        assert_eq!(m.row_count(), 4);
        assert!(!m.storage_rows().contains(&StorageRow::Install));
        // with room to grow as well: import, manage, song drive, venues, use the rest, install
        m.apply(Res::Storage(StorageInfo { on_stick: true, slack_kib: 233_000_000, ..Default::default() }));
        assert_eq!(m.storage_rows(), [StorageRow::Import, StorageRow::Manage, StorageRow::SongDrive, StorageRow::Venues, StorageRow::UseRest, StorageRow::Install]);
        m.sel = 5;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Install)]);
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
    fn a_connected_song_drive_gets_its_own_row_and_the_switch_stays_last() {
        let mut m = Model::new();
        m.screen = Screen::Songs;
        assert_eq!(m.row_count(), 2); // look again, automatic switch
        m.apply(Res::Songs(SongsInfo { mounted: true, name: "YARGSONGS".into(), space: "52 GB free of 224 GB".into(), auto: true, ..SongsInfo::default() }));
        assert_eq!(m.row_count(), 3);
        m.sel = 1;
        assert!(m.handle(Nav::Ok).is_empty()); // only explains where it is
        assert!(m.live_toast().unwrap().contains("/media/songs"));
        m.sel = 0;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::SongsRefresh]);
        m.busy = None;
        m.sel = 2;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::SongsAuto(false)]);
        assert!(!m.songs.auto);
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::SongsAuto(true)]);
    }

    #[test]
    fn an_alert_blocks_everything_until_ok_and_a_failed_connect_raises_one() {
        let mut m = Model::new();
        m.screen = Screen::Bluetooth;
        m.busy = Some("Connecting".into());
        m.apply(Res::Alert("Not ready".into()));
        assert!(m.busy.is_none());
        assert_eq!(m.alert.as_deref(), Some("Not ready"));
        assert!(m.handle(Nav::Down).is_empty());
        assert!(m.alert.is_some()); // arrows do not dismiss it
        m.hover(0);
        assert!(m.alert.is_some());
        assert!(m.handle(Nav::Ok).is_empty());
        assert!(m.alert.is_none());
        m.apply(Res::Alert("again".into()));
        m.click(0); // the OK button
        assert!(m.alert.is_none());
    }

    #[test]
    fn the_devices_screen_is_reached_from_the_main_menu_and_lists_what_is_plugged_in() {
        let mut m = Model::new();
        m.sel = 3;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Devices)]);
        assert_eq!(m.screen, Screen::Devices);
        m.apply(Res::Devices(vec![DeviceEntry { kind: "MIDI", name: "P-143".into(), detail: "P-143 MIDI 1".into() }]));
        assert_eq!(m.row_count(), 2);
        m.sel = 1;
        assert!(m.handle(Nav::Ok).is_empty());
        assert!(m.live_toast().unwrap().contains("P-143"));
        m.sel = 0;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Devices)]);
    }

    fn drive(label: &str) -> ImportDrive {
        ImportDrive { dev: "/dev/sdb1".into(), label: label.into(), fs: "EXFAT".into(), bytes: 500_000_000_000, uuid: "AB-CD".into() }
    }

    fn plan(todo_gb: u64, free_gb: u64) -> ImportPlan {
        ImportPlan {
            drive: drive("MySongs"),
            dest: "MySongs".into(),
            files: 120,
            bytes: todo_gb * 1_000_000_000,
            todo_files: 120,
            todo_bytes: todo_gb * 1_000_000_000,
            unreadable: 0,
            free_kib: free_gb * 1_000_000_000 / 1024,
            reserve_kib: 4_000_000,
        }
    }

    fn entry(name: &str, kind: SongKind) -> SongEntry {
        SongEntry { name: name.into(), kind }
    }

    #[test]
    fn storage_is_a_row_of_the_system_screen_and_back_returns_to_it() {
        let mut m = Model::new();
        m.screen = Screen::System;
        m.sel = 2;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Storage)]);
        assert_eq!(m.screen, Screen::Storage);
        assert_eq!(m.handle(Nav::Back), vec![Cmd::Refresh(Screen::System)]);
        assert_eq!((m.screen, m.sel), (Screen::System, 2));
    }

    #[test]
    fn the_song_drive_is_a_row_of_storage_and_back_returns_there() {
        let mut m = Model::new();
        m.screen = Screen::Storage;
        assert_eq!(m.row_count(), 4);
        m.sel = 2;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::Songs)]);
        assert_eq!(m.screen, Screen::Songs);
        assert_eq!(m.handle(Nav::Back), vec![Cmd::Refresh(Screen::Storage)]);
        assert_eq!((m.screen, m.sel), (Screen::Storage, 2));
    }

    #[test]
    fn unused_room_on_the_drive_is_offered_and_growing_runs_once_asked() {
        let mut m = Model::new();
        m.screen = Screen::Storage;
        assert_eq!(m.row_count(), 4);
        m.apply(Res::Storage(StorageInfo { slack_kib: 233_000_000, ..Default::default() }));
        assert_eq!(m.row_count(), 5);
        m.sel = 4;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::GrowData]);
        assert!(m.busy.is_some());
        m.apply(Res::Done("The rest of the drive is now in use".into()));
        m.apply(Res::Storage(StorageInfo::default()));
        assert_eq!(m.row_count(), 4, "the row goes away once the room is used");
        // A little leftover (a few MB) is not worth a row.
        m.apply(Res::Storage(StorageInfo { slack_kib: 4096, ..Default::default() }));
        assert_eq!(m.row_count(), 4);
    }

    fn venue_plan(todo: u64, mb: u64) -> crate::venues::VenuePlan {
        crate::venues::VenuePlan { found: 5, todo, todo_bytes: mb << 20, free_kib: 50_000_000, reserve_kib: 1_500_000 }
    }

    #[test]
    fn venues_are_scanned_asked_about_then_copied_and_can_be_removed() {
        let mut m = Model::new();
        m.screen = Screen::Storage;
        assert_eq!(m.storage_rows()[3], StorageRow::Venues);
        m.sel = 3;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::VenuesScan]);
        assert!(m.busy.is_some());
        // nothing found: the answer says what to do
        m.apply(Res::VenuesPlan(Err("No folder called venues on the song drive.".into())));
        assert!(m.busy.is_none() && m.live_toast().unwrap().contains("No folder called venues"));
        // all already there
        m.apply(Res::VenuesPlan(Ok(venue_plan(0, 0))));
        assert!(m.live_toast().unwrap().contains("already added"));
        // found some: a question, Cancel is the default, the second button copies
        m.apply(Res::VenuesPlan(Ok(venue_plan(2, 30))));
        let a = m.ask.clone().unwrap();
        assert!(a.question.contains("Add 2 venue files?") && a.note.contains(" MB"), "{a:?}");
        assert_eq!(m.handle(Nav::Ok), vec![], "Cancel is the default");
        m.apply(Res::VenuesPlan(Ok(venue_plan(1, 3))));
        assert!(m.ask.as_ref().unwrap().question.contains("Add 1 venue file?"));
        m.handle(Nav::Right);
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::VenuesRun]);
        assert!(m.busy.is_some());
        m.apply(Res::VenuesDone(Ok("1 venue file added".into())));
        assert!(m.busy.is_none() && m.live_toast().unwrap().contains("added"));
        // not enough room: a message, no question
        m.apply(Res::VenuesPlan(Ok(crate::venues::VenuePlan { free_kib: 1_600_000, ..venue_plan(1, 900) })));
        assert!(m.ask.is_none() && m.alert.as_ref().unwrap().contains("not enough room"));
        m.alert = None;
        // a failure is an alert
        m.apply(Res::VenuesDone(Err("1 venue file added, 1 could not be copied".into())));
        assert!(m.alert.is_some());
        m.alert = None;
        // the remove row only exists while there are venue files, and asks first (red button)
        assert!(!m.storage_rows().contains(&StorageRow::RemoveVenues));
        m.apply(Res::Storage(StorageInfo { venues: 3, ..Default::default() }));
        assert_eq!(m.storage_rows()[4], StorageRow::RemoveVenues);
        m.sel = 4;
        assert!(m.handle(Nav::Ok).is_empty());
        let a = m.ask.clone().unwrap();
        assert!(a.danger && a.question.contains("Remove 3 venue files?"));
        m.handle(Nav::Right);
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::VenuesRemove]);
    }

    #[test]
    fn importing_picks_a_drive_reads_it_then_asks_before_copying() {
        let mut m = Model::new();
        m.screen = Screen::Storage;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::Refresh(Screen::ImportPick)]);
        m.apply(Res::Drives(vec![drive("MySongs")]));
        // Back from the drive list goes to Storage, not Home.
        m.handle(Nav::Down);
        let cmds = m.handle(Nav::Ok);
        assert_eq!(cmds, vec![Cmd::ImportScan(drive("MySongs"))]);
        assert!(m.busy.is_some() && m.cancellable());
        // BACK while reading stops it.
        assert_eq!(m.handle(Nav::Back), vec![Cmd::ImportCancel]);
        m.apply(Res::Done("Cancelled".into()));
        assert!(m.busy.is_none() && !m.cancellable());
        // The plan comes back: a question, Cancel first, and nothing runs until the second button.
        m.apply(Res::ImportPlan(Ok(plan(40, 300))));
        let a = m.ask.clone().expect("a question is asked");
        assert!(a.question.contains("MySongs") && a.note.contains("40 GB") && !a.danger);
        assert_eq!(m.confirm_sel, 0);
        assert!(m.handle(Nav::Ok).is_empty() && m.ask.is_none() && m.busy.is_none());
        m.apply(Res::ImportPlan(Ok(plan(40, 300))));
        m.handle(Nav::Right);
        let cmds = m.handle(Nav::Ok);
        assert_eq!(cmds, vec![Cmd::ImportRun(plan(40, 300))]);
        assert!(m.busy.as_deref().unwrap().contains("Copying MySongs") && m.cancellable());
        m.apply(Res::Progress("Copying MySongs\u{2026} 50%".into()));
        assert!(m.busy.as_deref().unwrap().contains("50%"));
        assert_eq!(m.handle(Nav::Back), vec![Cmd::ImportCancel]);
        m.apply(Res::ImportDone(Ok("Copied 120 files".into())));
        assert!(m.busy.is_none() && m.live_toast().unwrap().contains("Copied"));
    }

    #[test]
    fn a_copy_that_does_not_fit_or_has_nothing_new_is_not_offered() {
        let mut m = Model::new();
        m.screen = Screen::ImportPick;
        m.apply(Res::ImportPlan(Ok(plan(100, 50))));
        assert!(m.ask.is_none());
        assert!(m.alert.as_deref().unwrap().contains("not enough room"));
        m.alert = None;
        // Exactly the free space minus the part kept for the system fits; one byte more does not.
        let mut p = plan(10, 20);
        p.todo_bytes = (p.free_kib - p.reserve_kib) * 1024;
        assert!(p.fits());
        p.todo_bytes += 1024;
        assert!(!p.fits());
        let mut same = plan(10, 300);
        same.todo_files = 0;
        same.todo_bytes = 0;
        m.apply(Res::ImportPlan(Ok(same)));
        assert!(m.ask.is_none() && m.live_toast().unwrap().contains("already on the internal drive"));
        m.apply(Res::ImportPlan(Err("The drive would not open".into())));
        assert_eq!(m.alert.as_deref(), Some("The drive would not open"));
    }

    #[test]
    fn a_copy_that_went_wrong_is_an_alert_not_a_toast() {
        let mut m = Model::new();
        m.busy = Some("Copying".into());
        m.apply(Res::ImportDone(Err("The internal drive is nearly full".into())));
        assert!(m.busy.is_none() && m.alert.is_some());
    }

    fn at_browser() -> Model {
        let mut m = Model::new();
        m.screen = Screen::Manage;
        m.apply(Res::Browse { path: vec![], entries: vec![entry("Drive A", SongKind::Folder), entry("Drive B", SongKind::Folder)] });
        m
    }

    #[test]
    fn deleting_can_be_asked_for_a_song_a_folder_or_everything() {
        let mut m = at_browser();
        // Everything: the top row at the top level.
        m.sel = 0;
        assert!(m.handle(Nav::Ok).is_empty());
        let a = m.ask.clone().unwrap();
        assert!(a.question.contains("ALL internal songs") && a.danger && a.cmd == Cmd::SongsDelete(vec![]));
        // Cancel is the default and does nothing.
        assert!(m.handle(Nav::Ok).is_empty() && m.ask.is_none() && m.busy.is_none());
        // A folder: LEFT on its row.
        m.sel = 2;
        m.handle(Nav::Left);
        assert_eq!(m.ask.as_ref().unwrap().cmd, Cmd::SongsDelete(vec!["Drive B".into()]));
        assert!(m.ask.as_ref().unwrap().question.contains("folder"));
        m.handle(Nav::Back);
        assert!(m.ask.is_none());
        // Inside a folder: a song is asked about with OK, the whole folder with the top row.
        m.sel = 1;
        assert_eq!(m.handle(Nav::Ok), vec![Cmd::SongsBrowse(vec!["Drive A".into()])]);
        m.apply(Res::Browse { path: vec!["Drive A".into()], entries: vec![entry("Band - Song", SongKind::Song), entry("notes.txt", SongKind::File)] });
        m.sel = 1;
        assert!(m.handle(Nav::Ok).is_empty());
        let a = m.ask.clone().unwrap();
        assert!(a.question.contains("song") && a.cmd == Cmd::SongsDelete(vec!["Drive A".into(), "Band - Song".into()]));
        m.handle(Nav::Right);
        let cmds = m.handle(Nav::Ok);
        assert_eq!(cmds, vec![Cmd::SongsDelete(vec!["Drive A".into(), "Band - Song".into()])]);
        assert!(m.busy.is_some() && !m.cancellable());
        m.apply(Res::Done("Deleted".into()));
        m.sel = 0;
        m.handle(Nav::Ok);
        let a = m.ask.clone().unwrap();
        assert!(a.question.contains("whole folder") && a.cmd == Cmd::SongsDelete(vec!["Drive A".into()]));
        // A file is asked about with the file wording.
        m.ask = None;
        m.sel = 2;
        m.handle(Nav::Left);
        assert!(m.ask.as_ref().unwrap().question.contains("file"));
    }

    #[test]
    fn back_in_the_browser_goes_up_and_finds_the_folder_again() {
        let mut m = at_browser();
        m.sel = 2;
        m.handle(Nav::Ok); // into Drive B
        m.apply(Res::Browse { path: vec!["Drive B".into()], entries: vec![entry("x", SongKind::Song)] });
        assert_eq!(m.sel, 0);
        let cmds = m.handle(Nav::Back);
        assert_eq!(cmds, vec![Cmd::SongsBrowse(vec![])]);
        m.apply(Res::Browse { path: vec![], entries: vec![entry("Drive A", SongKind::Folder), entry("Drive B", SongKind::Folder)] });
        assert_eq!(m.sel, 2, "Drive B is highlighted again");
        // From the top, BACK leaves the browser for Storage.
        assert_eq!(m.handle(Nav::Back), vec![Cmd::Refresh(Screen::Storage)]);
        assert_eq!(m.screen, Screen::Storage);
    }

    #[test]
    fn an_empty_library_has_nothing_to_delete_and_the_list_follows_the_session() {
        let mut m = Model::new();
        m.screen = Screen::Manage;
        m.apply(Res::Browse { path: vec![], entries: vec![] });
        assert!(m.handle(Nav::Ok).is_empty());
        assert!(m.ask.is_none() && m.live_toast().unwrap().contains("no internal songs"));
        // After a delete the session sends the folder above: the model follows its path.
        let mut m = at_browser();
        m.browse.path = vec!["Drive A".into()];
        m.apply(Res::Browse { path: vec![], entries: vec![entry("Drive B", SongKind::Folder)] });
        assert!(m.browse.path.is_empty() && m.row_count() == 2);
    }
}

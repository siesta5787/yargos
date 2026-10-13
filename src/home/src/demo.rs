// Fake data for previews (`yargos-home screenshot <screen> out.png`) so every screen can be seen
// without a display or any hardware.

use crate::model::*;

pub fn model(name: &str) -> Model {
    let mut m = Model::new();
    m.status = Status {
        network: "Wi-Fi: Living Room  (192.168.1.42)".into(),
        songs: "YARGSONGS connected, 412 GB free".into(),
        sound: "HDMI 0 (HDA ATI HDMI)".into(),
        yarg: "0.15.0".into(),
        update: Some("YARG 0.16.0 is available".into()),
        os_rolled_back: None,
        pads: 3,
        low_battery: vec![],
        os_version: "2026.10.16".into(),
    };
    m.wifi_dev = Some("wlan0".into());
    m.nets = vec![
        Network { name: "Living Room".into(), secured: true, signal: 4, connected: true, known: true },
        Network { name: "Neighbour 5G".into(), secured: true, signal: 3, connected: false, known: false },
        Network { name: "Cafe Guest".into(), secured: false, signal: 2, connected: false, known: false },
        Network { name: "Garage".into(), secured: true, signal: 1, connected: false, known: true },
    ];
    m.outputs = vec![
        Output { card: 0, dev: 3, label: "HDMI 0 (HDA ATI HDMI)".into() },
        Output { card: 0, dev: 7, label: "HDMI 1 (HDA ATI HDMI)".into() },
        Output { card: 1, dev: 0, label: "Analog (HDA Generic)".into() },
        Output { card: 2, dev: 0, label: "USB Audio (Focusrite)".into() },
    ];
    m.cur_card_dev = Some((0, 3));
    m.volume = Some((1, 70));
    m.bt = vec![
        BtDevice { mac: "00:22:D7:AD:BE:D4".into(), name: "Nintendo RVL-CNT-01".into(), paired: false, connected: false, ready: true, battery: None, battery_low: false },
        BtDevice { mac: "AA:BB:CC:00:00:01".into(), name: "Logitech K380".into(), paired: true, connected: true, ready: true, battery: None, battery_low: false },
        BtDevice { mac: "AA:BB:CC:00:00:02".into(), name: "Polybar Guitar".into(), paired: true, connected: false, ready: true, battery: None, battery_low: false },
    ];
    m.upd = UpdInfo {
        installed: "0.15.0".into(),
        previous: Some("0.14.0".into()),
        latest: Some("0.16.0".into()),
        available: true,
        auto: true,
        log: vec!["yargos-update: installed 0.15.0, latest 0.16.0".into(), "yargos-update: update available: 0.16.0".into()],
    };
    m.songs = SongsInfo { mounted: true, detail: "The song drive is connected (EXFAT, 931 GB) at /media/songs.".into(), name: "YARGSONGS".into(), space: "412 GB free".into(), fs: "EXFAT".into(), sources: vec!["Songs".into(), "Clone Hero Extras".into(), "Rock Band".into()], auto: true };
    m.sysinfo = vec![
        "YARGOS 0.1  (Linux 6.19.14)".into(),
        "Up for 2 hours, 14 minutes".into(),
        "Memory: 1.8 GB used of 7.2 GB".into(),
        "Data disk: 1.2 GB free of 1.9 GB".into(),
    ];
    m.disks = vec![
        Disk { path: "/dev/sda".into(), bytes: 256_060_514_304, model: "WDC WDS250G1B0B".into(), removable: false },
        Disk { path: "/dev/sdb".into(), bytes: 1_000_204_886_016, model: "Samsung Portable SSD".into(), removable: true },
    ];
    m.tick = 3;
    m.storage = StorageInfo { used_kib: 46_000_000, free_kib: 180_000_000, total_kib: 235_000_000, reserve_kib: 4_000_000, folders: 2, slack_kib: 0 };
    m.drives = vec![
        ImportDrive { dev: "/dev/sdb1".into(), label: "YARGSONGS".into(), fs: "EXFAT".into(), bytes: 931_000_000_000, uuid: "1234-ABCD".into() },
        ImportDrive { dev: "/dev/sdc1".into(), label: String::new(), fs: "NTFS".into(), bytes: 64_000_000_000, uuid: "ZZ".into() },
    ];
    let browse_top = vec![
        SongEntry { name: "YARGSONGS".into(), kind: SongKind::Folder },
        SongEntry { name: "Rock Band DLC".into(), kind: SongKind::Folder },
    ];
    let plan = ImportPlan {
        drive: m.drives[0].clone(),
        dest: "YARGSONGS".into(),
        files: 18_400,
        bytes: 46_000_000_000,
        todo_files: 18_400,
        todo_bytes: 46_000_000_000,
        unreadable: 0,
        free_kib: 180_000_000,
        reserve_kib: 4_000_000,
    };
    match name {
        "storage" => { m.screen = Screen::Storage; m.sel = 1; }
        "storage-slack" => {
            m.screen = Screen::Storage;
            m.sel = 2;
            m.storage = StorageInfo { used_kib: 0, free_kib: 20_000, total_kib: 700_000, reserve_kib: 1_500_000, folders: 0, slack_kib: 233_000_000 };
        }
        "import" => { m.screen = Screen::ImportPick; m.sel = 1; }
        "import-none" => { m.screen = Screen::ImportPick; m.drives.clear(); }
        "import-confirm" => {
            m.screen = Screen::ImportPick;
            m.sel = 1;
            m.apply(Res::ImportPlan(Ok(plan)));
        }
        "import-busy" => {
            m.screen = Screen::ImportPick;
            m.busy = Some("Copying YARGSONGS\u{2026} 37%  (17 GB of 46 GB)".into());
            m.cancel = Some(Cmd::ImportCancel);
        }
        "import-full" => {
            m.screen = Screen::ImportPick;
            let mut p = plan;
            p.free_kib = 30_000_000;
            m.apply(Res::ImportPlan(Ok(p)));
        }
        "manage" => {
            m.screen = Screen::Manage;
            m.sel = 1;
            m.browse = Browse { path: vec![], entries: browse_top };
        }
        "manage-empty" => { m.screen = Screen::Manage; }
        "manage-songs" => {
            m.screen = Screen::Manage;
            m.sel = 2;
            m.browse = Browse {
                path: vec!["YARGSONGS".into(), "Rock".into()],
                entries: vec![
                    SongEntry { name: "AC-DC - Back in Black".into(), kind: SongKind::Song },
                    SongEntry { name: "Aerosmith - Walk This Way".into(), kind: SongKind::Song },
                    SongEntry { name: "Extras".into(), kind: SongKind::Folder },
                    SongEntry { name: "readme.txt".into(), kind: SongKind::File },
                ],
            };
        }
        "manage-confirm" => {
            m.screen = Screen::Manage;
            m.sel = 1;
            m.browse = Browse { path: vec![], entries: browse_top };
            m.handle(Nav::Left);
            m.confirm_sel = 0;
        }
        "manage-confirm-all" => {
            m.screen = Screen::Manage;
            m.browse = Browse { path: vec![], entries: browse_top };
            m.handle(Nav::Ok);
        }
        "system" => {
            m.screen = Screen::System;
            m.sel = 4;
            m.os = OsInfo { running: "2026.10.11.2".into(), latest: Some("2026.11.1".into()), available: Some("2026.11.1".into()), notes: "Faster start-up and a fix for Wii guitars.".into(), previous: Some("2026.10.11.1".into()), auto: true, ..Default::default() };
        }
        "os-update" => {
            m.screen = Screen::OsUpdate;
            m.sel = 1;
            m.os = OsInfo { running: "2026.10.11.2".into(), latest: Some("2026.11.1".into()), available: Some("2026.11.1".into()), notes: "Faster start-up and a fix for Wii guitars.".into(), previous: Some("2026.10.11.1".into()), auto: true, ..Default::default() };
        }
        "os-offline" => {
            m.screen = Screen::OsUpdate;
            m.sel = 2;
            m.os = OsInfo { running: "2026.10.13".into(), offline: Some("2026.10.14".into()), previous: None, auto: true, error: Some("offline".into()), why: Some("the name of the update server could not be looked up (no internet, or the network gives no DNS)".into()), ..Default::default() };
        }
        "os-offline-missing" => {
            m.screen = Screen::OsUpdate;
            m.sel = 2;
            m.os = OsInfo { running: "2026.10.13".into(), offline_hint: Some("The song drive has the update information but not its image file, rootfs-2026.10.14.img.xz. Copy that file next to it.".into()), auto: true, ..Default::default() };
        }
        "os-failed" => {
            m.screen = Screen::OsUpdate;
            m.sel = 1;
            m.os = OsInfo { running: "2026.10.13".into(), available: Some("2026.10.14".into()), auto: true, ..Default::default() };
            m.alert = Some("The download failed at 41% after 6 tries: the connection timed out or stalled".into());
        }
        "os-busy" => {
            m.screen = Screen::OsUpdate;
            m.os = OsInfo { running: "2026.10.11.2".into(), available: Some("2026.11.1".into()), auto: true, ..Default::default() };
            m.busy = Some("Installing the update\u{2026} 63%".into());
            m.cancel = Some(Cmd::OsCancel);
        }
        "os-rolledback" => {
            m.alert = Some("The system update to 2026.11.1 did not start properly, so the earlier system was put back. Nothing was lost. The update will not be tried again.".into());
        }
        "yarg" => {
            m.status.yarg = "not installed".into();
            m.status.network = "Wi-Fi: Living Room (192.168.0.47)".into();
            m.screen = Screen::Yarg;
            m.yarg_zips = vec!["/media/songs/YARG_v0.15.0-Linux-x86_64.zip".into()];
        }
        "yarg-home" => m.status.yarg = "not installed".into(),
        "yarg-busy" => {
            m.status.yarg = "not installed".into();
            m.screen = Screen::Yarg;
            m.busy = Some("Downloading YARG\u{2026} 43%".into());
        }
        "devices" => {
            m.screen = Screen::Devices;
            m.sel = 3;
            m.devices = vec![
                DeviceEntry { kind: "USB", name: "P-143".into(), detail: "0499:171d".into() },
                DeviceEntry { kind: "USB", name: "Logitech USB Microphone".into(), detail: "046d:0a03".into() },
                DeviceEntry { kind: "KEYBOARD", name: "2.4G Wireless Receiver".into(), detail: "NO INPUT".into() },
                DeviceEntry { kind: "USB", name: "2.4G Wireless Receiver".into(), detail: "1a81:1004".into() },
                DeviceEntry { kind: "MIDI", name: "P-143".into(), detail: "P-143 MIDI 1".into() },
                DeviceEntry { kind: "SOUND", name: "HDA Intel".into(), detail: String::new() },
                DeviceEntry { kind: "PAD", name: "Guitar Hero4 for PlayStation (R) 3".into(), detail: String::new() },
            ];
        }
        "install" => { m.screen = Screen::Install; m.sel = 1; }
        "install-none" => { m.screen = Screen::Install; m.disks.clear(); }
        "install-confirm" => { m.screen = Screen::Install; m.sel = 1; m.erase = Some(0); m.confirm_sel = 1; }
        "install-busy" => { m.screen = Screen::Install; m.busy = Some("Copying the system (a few minutes) (step 3 of 6)".into()); }
        "install-done" => { m.screen = Screen::Install; m.installed = Some("WDC WDS250G1B0B (256 GB)".into()); }
        "install-failed" => { m.screen = Screen::Install; m.install_error = Some("Could not partition /dev/sda".into()); }
        "home" => {}
        "home-select" => m.sel = 3,
        "wifi" => { m.screen = Screen::Wifi; m.sel = 2; }
        "wifi-none" => { m.screen = Screen::Wifi; m.wifi_dev = None; m.nets.clear(); }
        "password" => {
            m.screen = Screen::Password;
            m.osk = Osk::new("Neighbour 5G");
            m.osk.text = "correct horse".into();
            m.osk.row = 2;
            m.osk.col = 4;
        }
        "sound" => { m.screen = Screen::Sound; m.sel = 4; }
        "bluetooth" => { m.screen = Screen::Bluetooth; m.sel = 1; }
        "updates" => { m.screen = Screen::Updates; m.sel = 1; }
        "songs" => { m.screen = Screen::Songs; }
        "system" => { m.screen = Screen::System; m.sel = 2; }
        "busy" => { m.screen = Screen::Wifi; m.busy = Some("Connecting to Neighbour 5G…".into()); }
        "confirm" => { m.screen = Screen::System; m.sel = 1; m.confirm = Some(Power::Shutdown); m.confirm_sel = 1; }
        "bt-battery" => {
            m.screen = Screen::Bluetooth;
            m.sel = 1;
            m.status.low_battery = vec!["Wii guitar (3A) 12%".into()];
            m.bt = vec![
                BtDevice { mac: "00:22:D7:AD:BE:D4".into(), name: "Nintendo RVL-CNT-01 (D4)".into(), paired: false, connected: true, ready: true, battery: Some(82), battery_low: false },
                BtDevice { mac: "00:1E:35:00:00:3A".into(), name: "Nintendo RVL-CNT-01 (3A)".into(), paired: false, connected: true, ready: true, battery: Some(12), battery_low: true },
                BtDevice { mac: "AA:BB:CC:00:00:01".into(), name: "Logitech K380".into(), paired: true, connected: true, ready: true, battery: None, battery_low: false },
            ];
        }
        "home-battery" => m.status.low_battery = vec!["Wii guitar (3A) 12%".into()],
        "bt-notready" => {
            m.screen = Screen::Bluetooth;
            m.bt[0].connected = true;
            m.bt[0].ready = false;
        }
        "alert" => {
            m.screen = Screen::Bluetooth;
            m.alert = Some("The Wiimote connected, but it is NOT ready: the guitar did not answer. Press 1+2 on it again: it connects by itself.".into());
        }
        "toast" => { m.screen = Screen::Sound; m.toast("Sound output changed to HDMI 0"); }
        _ => {}
    }
    m
}

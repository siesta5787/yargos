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
        update: Some("0.16.0".into()),
        pads: 3,
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
        BtDevice { mac: "00:22:D7:AD:BE:D4".into(), name: "Nintendo RVL-CNT-01".into(), paired: false, connected: false },
        BtDevice { mac: "AA:BB:CC:00:00:01".into(), name: "Logitech K380".into(), paired: true, connected: true },
        BtDevice { mac: "AA:BB:CC:00:00:02".into(), name: "Polybar Guitar".into(), paired: true, connected: false },
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
    match name {
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
        "toast" => { m.screen = Screen::Sound; m.toast("Sound output changed to HDMI 0"); }
        _ => {}
    }
    m
}

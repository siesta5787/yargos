// Test tool: a wireless-keyboard receiver as two USB HID devices (uhid), the way the real ones show up: "bjx.cn 2.4G Receiver"
// (a mouse and media keys) and "bjx.cn 2.4G Receiver Keyboard" (the keys), vendor 24ae product 9db6. Plugging one in and out
// again can be tried in QEMU without the hardware. Build it static so it runs in the guest:
//   rustc --edition 2021 -O -C target-feature=+crt-static tools/fake-dongle.rs -o /tmp/fakedongle
//   fakedongle [ORDER] [SECONDS]
// ORDER    "ab" (default): the receiver, then the keyboard; "ba" the other way round (the order udev announces them in is
//          not fixed, and the real one came back in the other order after a replug); SECONDS how long it stays "plugged in".
// While it runs, a file /tmp/dongle.key with a name in it makes the keyboard press that key once (and the file is removed):
//   echo down > /tmp/dongle.key      (also: up left right enter esc a)
use std::fs::OpenOptions;
use std::io::Write;
use std::time::{Duration, Instant};

const EV_CREATE2: u32 = 11;
const EV_INPUT2: u32 = 12;

const KEYBOARD: &[u8] = &[
    0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x05, 0x07, 0x19, 0xe0, 0x29, 0xe7, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02, 0x95, 0x01,
    0x75, 0x08, 0x81, 0x01, 0x95, 0x05, 0x75, 0x01, 0x05, 0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02, 0x95, 0x01, 0x75, 0x03, 0x91, 0x01, 0x95, 0x06,
    0x75, 0x08, 0x15, 0x00, 0x25, 0x65, 0x05, 0x07, 0x19, 0x00, 0x29, 0x65, 0x81, 0x00, 0xc0,
];
const MOUSE_AND_MEDIA: &[u8] = &[
    0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x85, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03,
    0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x05, 0x81, 0x03, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x81, 0x25, 0x7f, 0x75, 0x08, 0x95, 0x02,
    0x81, 0x06, 0xc0, 0xc0, 0x05, 0x0c, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x02, 0x19, 0x00, 0x2a, 0xff, 0x03, 0x15, 0x00, 0x26, 0xff, 0x03, 0x75, 0x10,
    0x95, 0x01, 0x81, 0x00, 0xc0,
];

fn create(name: &str, rd: &[u8]) -> std::fs::File {
    let f = OpenOptions::new().read(true).write(true).open("/dev/uhid").expect("open /dev/uhid");
    // struct uhid_event { u32 type; union { create2 { name[128], phys[64], uniq[64], u16 rd_size, u16 bus, u32 vendor, product, version, country, u8 rd_data[4096] } } }
    let mut ev = vec![0u8; 4 + 128 + 64 + 64 + 2 + 2 + 4 * 4 + 4096];
    ev[0..4].copy_from_slice(&EV_CREATE2.to_le_bytes());
    ev[4..4 + name.len()].copy_from_slice(name.as_bytes());
    let o = 4 + 128 + 64 + 64;
    ev[o..o + 2].copy_from_slice(&(rd.len() as u16).to_le_bytes());
    ev[o + 2..o + 4].copy_from_slice(&3u16.to_le_bytes()); // BUS_USB
    ev[o + 4..o + 8].copy_from_slice(&0x24aeu32.to_le_bytes());
    ev[o + 8..o + 12].copy_from_slice(&0x9db6u32.to_le_bytes());
    ev[o + 20..o + 20 + rd.len()].copy_from_slice(rd);
    (&f).write_all(&ev).expect("create");
    f
}

fn send(f: &std::fs::File, data: &[u8]) {
    let mut e = vec![0u8; 4 + 2 + 4096];
    e[0..4].copy_from_slice(&EV_INPUT2.to_le_bytes());
    e[4..6].copy_from_slice(&(data.len() as u16).to_le_bytes());
    e[6..6 + data.len()].copy_from_slice(data);
    let _ = (&*f).write_all(&e);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let order = args.get(1).cloned().unwrap_or_else(|| "ab".into());
    let secs: u64 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(120);
    let (mut recv, mut kbd) = (None, None);
    for c in order.chars() {
        match c {
            'a' => recv = Some(create("bjx.cn 2.4G Receiver", MOUSE_AND_MEDIA)),
            'b' => kbd = Some(create("bjx.cn 2.4G Receiver Keyboard", KEYBOARD)),
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    let _ = &recv;
    eprintln!("fakedongle: plugged in ({order})");
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_millis(200));
        let Ok(k) = std::fs::read_to_string("/tmp/dongle.key") else { continue };
        let _ = std::fs::remove_file("/tmp/dongle.key");
        let code = match k.trim() {
            "down" => 0x51,
            "up" => 0x52,
            "left" => 0x50,
            "right" => 0x4f,
            "enter" => 0x28,
            "esc" => 0x29,
            "a" => 0x04,
            _ => continue,
        };
        if let Some(f) = &kbd {
            send(f, &[0, 0, code, 0, 0, 0, 0, 0]);
            std::thread::sleep(Duration::from_millis(80));
            send(f, &[0; 8]);
            eprintln!("fakedongle: pressed {}", k.trim());
        }
    }
    drop(kbd);
    drop(recv);
    eprintln!("fakedongle: unplugged");
    std::thread::sleep(Duration::from_millis(500));
}

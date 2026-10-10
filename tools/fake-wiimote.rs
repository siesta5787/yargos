// Test tool: a uhid device that udev sees as a Wiimote (Bluetooth bus, 057E:0306) and that answers like a Wiimote
// with a Guitar Hero guitar plugged in, so the udev rule, yargos-bridge and the launcher's "ready" check can be tried in
// QEMU without hardware. Build it static so it runs in the guest:
//   rustc --edition 2021 -O -C target-feature=+crt-static tools/fake-wiimote.rs -o /tmp/fakehid
// Serve it (python3 -m http.server in its folder), fetch it in the guest (curl http://10.0.2.2:8000/fakehid), then
//   fakehid [ADDRESS] [IGNORE] [SECONDS]
// ADDRESS  the Bluetooth address it claims (default 00:1e:35:aa:bb:01); IGNORE  how many status requests to drop
// before answering (a slow Wiimote: the bridge must ask again); SECONDS  how long to stay (default 120).
// Expect /dev/hidrawN, `systemctl list-units "yargos-bridge*"` to show the bridge, and /run/yargos-bridge/ready-hidrawN
// to appear once the "guitar" answers.
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const EV_CREATE2: u32 = 11;
const EV_INPUT2: u32 = 12;
const EV_OUTPUT: u32 = 6;

// A report descriptor with the Wiimote's report ids and sizes (the kernel drops reports it has no description for).
fn descriptor() -> Vec<u8> {
    let mut d = vec![0x06, 0x00, 0xff, 0x09, 0x01, 0xa1, 0x01];
    let mut report = |id: u8, input: bool, bytes: u8| {
        d.extend_from_slice(&[0x85, id, 0x09, 0x01, 0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08, 0x95, bytes, if input { 0x81 } else { 0x91 }, 0x00]);
    };
    for (id, n) in [(0x20u8, 6u8), (0x21, 21), (0x22, 4), (0x30, 2), (0x35, 21)] {
        report(id, true, n);
    }
    for (id, n) in [(0x11u8, 1u8), (0x12, 2), (0x15, 1), (0x16, 21), (0x17, 6)] {
        report(id, false, n);
    }
    d.push(0xc0);
    d
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let addr = args.get(1).cloned().unwrap_or_else(|| "00:1e:35:aa:bb:01".into());
    let mut ignore: u32 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(0);
    let secs: u64 = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(120);
    let f = OpenOptions::new().read(true).write(true).open("/dev/uhid").expect("open /dev/uhid");
    let rd = descriptor();
    // struct uhid_event { u32 type; union { create2 { name[128], phys[64], uniq[64], u16 rd_size, u16 bus, u32 vendor, product, version, country, u8 rd_data[4096] } } }
    let mut ev = vec![0u8; 4 + 128 + 64 + 64 + 2 + 2 + 4 * 4 + 4096];
    ev[0..4].copy_from_slice(&EV_CREATE2.to_le_bytes());
    let name = b"Nintendo RVL-CNT-01";
    ev[4..4 + name.len()].copy_from_slice(name);
    ev[4 + 128 + 64..4 + 128 + 64 + addr.len()].copy_from_slice(addr.as_bytes());
    let o = 4 + 128 + 64 + 64;
    ev[o..o + 2].copy_from_slice(&(rd.len() as u16).to_le_bytes());
    ev[o + 2..o + 4].copy_from_slice(&5u16.to_le_bytes()); // BUS_BLUETOOTH
    ev[o + 4..o + 8].copy_from_slice(&0x057eu32.to_le_bytes());
    ev[o + 8..o + 12].copy_from_slice(&0x0306u32.to_le_bytes());
    ev[o + 20..o + 20 + rd.len()].copy_from_slice(&rd);
    (&f).write_all(&ev).expect("create");

    let out = Arc::new(Mutex::new(f.try_clone().unwrap()));
    let send = move |data: &[u8]| {
        let mut e = vec![0u8; 4 + 2 + 4096];
        e[0..4].copy_from_slice(&EV_INPUT2.to_le_bytes());
        e[4..6].copy_from_slice(&(data.len() as u16).to_le_bytes());
        e[6..6 + data.len()].copy_from_slice(data);
        let _ = out.lock().unwrap().write_all(&e);
    };
    let streaming = Arc::new(Mutex::new(false));
    {
        let (send, streaming) = (send.clone(), streaming.clone());
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(50));
            if *streaming.lock().unwrap() {
                // buttons, accelerometer, then the guitar's 6 bytes: nothing pressed, touch bar and whammy at rest.
                let mut r = [0u8; 22];
                r[0] = 0x35;
                r[4] = 0x80;
                r[6..12].copy_from_slice(&[0x80, 0x80, 0x0f, 0x0f, 0xff, 0xff]);
                send(&r);
            }
        });
    }

    let start = Instant::now();
    let mut reader = &f;
    let mut buf = vec![0u8; 4400];
    while start.elapsed() < Duration::from_secs(secs) {
        let n = match reader.read(&mut buf) {
            Ok(n) if n > 0 => n,
            _ => break,
        };
        if n < 4 || u32::from_le_bytes(buf[0..4].try_into().unwrap()) != EV_OUTPUT {
            continue;
        }
        // struct uhid_output_req { u8 data[4096]; u16 size; u8 rtype; }
        let size = u16::from_le_bytes(buf[4 + 4096..4 + 4096 + 2].try_into().unwrap()) as usize;
        let data = buf[4..4 + size.min(4096)].to_vec();
        match data.first() {
            Some(0x15) => {
                if ignore > 0 {
                    ignore -= 1;
                    eprintln!("fakehid: ignoring a status request");
                } else {
                    eprintln!("fakehid: status -> extension plugged in");
                    send(&[0x20, 0, 0, 0x02, 0, 0, 0xc0]);
                }
            }
            Some(0x17) if data.len() >= 5 && data[1..5] == [0x04, 0xa4, 0x00, 0xfa] => {
                let mut r = [0u8; 22];
                r[0] = 0x21;
                r[3] = 0x50;
                r[4..6].copy_from_slice(&[0x00, 0xfa]);
                r[6..12].copy_from_slice(&[0x00, 0x00, 0xa4, 0x20, 0x01, 0x03]);
                send(&r);
            }
            Some(0x12) if data.len() >= 3 => {
                *streaming.lock().unwrap() = data[2] == 0x35;
                eprintln!("fakehid: reporting mode {:#x}", data[2]);
            }
            _ => {}
        }
    }
}

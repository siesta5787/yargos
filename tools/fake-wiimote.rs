// Test tool: makes a uhid device that udev sees as a Wiimote (Bluetooth bus, 057E:0306) so the rule that starts
// yargos-bridge-wii@ can be checked without hardware. Build it static so it runs in the guest:
//   rustc -O -C target-feature=+crt-static tools/fake-wiimote.rs -o /tmp/fakehid
// Serve it (python3 -m http.server in its folder), fetch it in the guest (curl http://10.0.2.2:8000/fakehid),
// run it there, then expect /dev/hidrawN and `systemctl list-units "yargos-bridge*"` to show the bridge running.
use std::fs::OpenOptions;
use std::io::Write;
fn main() {
    let mut f = OpenOptions::new().read(true).write(true).open("/dev/uhid").expect("open /dev/uhid");
    // struct uhid_event { u32 type; union { create2 { name[128], phys[64], uniq[64], u16 rd_size, u16 bus, u32 vendor, product, version, country, u8 rd_data[4096] } } }
    let desc: [u8; 25] = [0x06, 0x00, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x30, 0x09, 0x01, 0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08, 0x95, 0x02, 0x81, 0x02, 0xc0, 0, 0];
    let rd = &desc[..23];
    let mut ev = vec![0u8; 4 + 128 + 64 + 64 + 2 + 2 + 4 * 5 + 4096];
    ev[0..4].copy_from_slice(&11u32.to_le_bytes()); // UHID_CREATE2
    let name = b"Nintendo RVL-CNT-01 (fake)";
    ev[4..4 + name.len()].copy_from_slice(name);
    let o = 4 + 128 + 64 + 64;
    ev[o..o + 2].copy_from_slice(&(rd.len() as u16).to_le_bytes());
    ev[o + 2..o + 4].copy_from_slice(&5u16.to_le_bytes()); // BUS_BLUETOOTH
    ev[o + 4..o + 8].copy_from_slice(&0x057eu32.to_le_bytes());
    ev[o + 8..o + 12].copy_from_slice(&0x0306u32.to_le_bytes());
    let d = o + 4 + 16;
    ev[d..d + rd.len()].copy_from_slice(rd);
    f.write_all(&ev).expect("create");
    std::thread::sleep(std::time::Duration::from_secs(120));
}

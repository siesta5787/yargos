use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::thread;

const UHID_DESTROY: u32 = 1;
const UHID_GET_REPORT: u32 = 9;
const UHID_GET_REPORT_REPLY: u32 = 10;
const UHID_CREATE2: u32 = 11;
const UHID_INPUT2: u32 = 12;
const UHID_SET_REPORT: u32 = 13;
const UHID_SET_REPORT_REPLY: u32 = 14;

const EVENT_SIZE: usize = 4 + 4372;
const BUS_BLUETOOTH: u16 = 0x05;

pub struct DeviceInfo<'a> {
    pub name: &'a str,
    pub vendor: u32,
    pub product: u32,
    pub version: u32,
    pub descriptor: &'a [u8],
}

pub struct UhidDevice {
    file: File,
}

fn put_u16(buf: &mut [u8], off: usize, v: u16) {
    buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

fn put_u32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

impl UhidDevice {
    pub fn create(info: &DeviceInfo) -> io::Result<Self> {
        let mut file = OpenOptions::new().read(true).write(true).open("/dev/uhid")?;
        let mut ev = [0u8; EVENT_SIZE];
        put_u32(&mut ev, 0, UHID_CREATE2);
        let name = info.name.as_bytes();
        let n = name.len().min(127);
        ev[4..4 + n].copy_from_slice(&name[..n]);
        put_u16(&mut ev, 4 + 256, info.descriptor.len() as u16);
        put_u16(&mut ev, 4 + 258, BUS_BLUETOOTH);
        put_u32(&mut ev, 4 + 260, info.vendor);
        put_u32(&mut ev, 4 + 264, info.product);
        put_u32(&mut ev, 4 + 268, info.version);
        put_u32(&mut ev, 4 + 272, 0);
        ev[4 + 276..4 + 276 + info.descriptor.len()].copy_from_slice(info.descriptor);
        file.write_all(&ev)?;

        let reader = file.try_clone()?;
        let responder = file.try_clone()?;
        thread::spawn(move || drain_events(reader, responder));
        Ok(Self { file })
    }

    pub fn send_input(&mut self, report: &[u8]) -> io::Result<()> {
        let mut ev = [0u8; EVENT_SIZE];
        put_u32(&mut ev, 0, UHID_INPUT2);
        put_u16(&mut ev, 4, report.len() as u16);
        ev[6..6 + report.len()].copy_from_slice(report);
        self.file.write_all(&ev)
    }
}

impl Drop for UhidDevice {
    fn drop(&mut self) {
        let mut ev = [0u8; EVENT_SIZE];
        put_u32(&mut ev, 0, UHID_DESTROY);
        let _ = self.file.write_all(&ev);
    }
}

// The kernel queues feature-report requests and waits for an answer; reply
// with zeros so nothing stalls on the virtual guitar's unused feature report.
fn drain_events(mut reader: File, mut responder: File) {
    let mut ev = [0u8; EVENT_SIZE];
    loop {
        match reader.read(&mut ev) {
            Ok(n) if n >= 4 => {}
            _ => return,
        }
        let ty = u32::from_le_bytes([ev[0], ev[1], ev[2], ev[3]]);
        let mut reply = [0u8; EVENT_SIZE];
        match ty {
            UHID_GET_REPORT => {
                put_u32(&mut reply, 0, UHID_GET_REPORT_REPLY);
                reply[4..8].copy_from_slice(&ev[4..8]);
                put_u16(&mut reply, 8, 0);
                put_u16(&mut reply, 10, 8);
                let _ = responder.write_all(&reply);
            }
            UHID_SET_REPORT => {
                put_u32(&mut reply, 0, UHID_SET_REPORT_REPLY);
                reply[4..8].copy_from_slice(&ev[4..8]);
                put_u16(&mut reply, 8, 0);
                let _ = responder.write_all(&reply);
            }
            _ => {}
        }
    }
}

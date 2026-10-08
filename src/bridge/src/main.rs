mod ps3;
mod ps3drums;
mod uhid;
mod wiimote;
mod xdrums;

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::process::ExitCode;

const SLOT_DIR: &str = "/run/yargos-bridge";
const MAX_SLOTS: u8 = 4;

struct Slot(String);

impl Drop for Slot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn claim_slot() -> (u8, Option<Slot>) {
    let _ = fs::create_dir_all(SLOT_DIR);
    for n in 1..=MAX_SLOTS {
        let path = format!("{SLOT_DIR}/slot{n}");
        if OpenOptions::new().write(true).create_new(true).open(&path).is_ok() {
            return (n, Some(Slot(path)));
        }
    }
    (1, None)
}

fn run_wii(path: &str) -> std::io::Result<()> {
    let dev = OpenOptions::new().read(true).write(true).open(path)?;
    let mut reader: File = dev.try_clone()?;
    let (slot, _guard) = claim_slot();

    let mut virtual_guitar = uhid::UhidDevice::create(&uhid::DeviceInfo {
        name: ps3::NAME,
        vendor: ps3::VENDOR,
        product: ps3::PRODUCT,
        version: ps3::VERSION,
        descriptor: ps3::DESCRIPTOR,
    })?;

    let mut wm = wiimote::Wiimote::new(dev);
    wm.start(slot)?;

    let mut last = ps3::Guitar::idle();
    virtual_guitar.send_input(&last.to_report())?;

    let mut buf = [0u8; 64];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        if let Some(g) = wm.handle(&buf[..n])? {
            if g != last {
                virtual_guitar.send_input(&g.to_report())?;
                last = g;
            }
        }
    }
}

fn run_drums(path: &str) -> std::io::Result<()> {
    let mut dev = OpenOptions::new().read(true).open(path)?;
    xdrums::grab(&dev)?;

    let mut virtual_drums = uhid::UhidDevice::create(&uhid::DeviceInfo {
        name: ps3drums::NAME,
        vendor: ps3drums::VENDOR,
        product: ps3drums::PRODUCT,
        version: ps3drums::VERSION,
        descriptor: ps3drums::DESCRIPTOR,
    })?;

    let mut kit = xdrums::XDrums::new();
    let mut last = ps3drums::Drums::idle();
    virtual_drums.send_input(&last.to_report())?;

    let mut buf = [0u8; 24 * 64];
    loop {
        let n = xdrums::read_events(&mut dev, &mut buf)?;
        if n == 0 {
            return Ok(());
        }
        for chunk in buf[..n].chunks_exact(24) {
            let (ty, code, value) = xdrums::parse_event(chunk);
            if let Some(d) = kit.handle(ty, code, value) {
                if d != last {
                    virtual_drums.send_input(&d.to_report())?;
                    last = d;
                }
            }
        }
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(mode), Some(path)) = (args.next(), args.next()) else {
        eprintln!("usage: yargos-bridge wii /dev/hidrawN | drums /dev/input/eventN");
        return ExitCode::from(2);
    };
    let result = match mode.as_str() {
        "wii" => run_wii(&path),
        "drums" => run_drums(&path),
        _ => {
            eprintln!("yargos-bridge: unknown mode '{mode}'");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("yargos-bridge: {mode} {path}: {e}");
            ExitCode::FAILURE
        }
    }
}

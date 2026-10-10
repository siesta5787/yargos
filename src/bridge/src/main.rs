mod ps3;
mod ps3drums;
mod uhid;
mod wiimote;
mod xdrums;

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const SLOT_DIR: &str = "/run/yargos-bridge";
const MAX_SLOTS: u8 = 4;

struct Slot(String);

impl Drop for Slot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// A slot file holds the pid of the bridge that owns it, so one left behind by a bridge that was killed (systemd
/// stops it with SIGTERM when the Wiimote goes away) does not use up a player number for good.
fn claim_slot() -> (u8, Option<Slot>) {
    claim_slot_in(SLOT_DIR)
}

fn claim_slot_in(dir: &str) -> (u8, Option<Slot>) {
    let _ = fs::create_dir_all(dir);
    for n in 1..=MAX_SLOTS {
        let path = format!("{dir}/slot{n}");
        for _ in 0..2 {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    let _ = write!(f, "{}", std::process::id());
                    return (n, Some(Slot(path)));
                }
                Err(_) => {
                    let owner = fs::read_to_string(&path).ok().and_then(|t| t.trim().parse::<u32>().ok());
                    if owner.is_some_and(|pid| fs::metadata(format!("/proc/{pid}")).is_ok()) {
                        break;
                    }
                    let _ = fs::remove_file(&path);
                }
            }
        }
    }
    (1, None)
}

/// While this file exists the guitar answered and is streaming: the launcher shows "ready" only then.
fn ready_path(dev: &str) -> String {
    format!("{SLOT_DIR}/ready-{}", dev.rsplit('/').next().unwrap_or(dev))
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

    let ready = ready_path(path);
    let _ = fs::remove_file(&ready);
    let streaming = Arc::new(AtomicBool::new(false));
    // A Wiimote that is slow to wake, or that drops the first request, is asked again until the guitar answers:
    // the one status request in start() is not enough.
    let mut nudge: File = dev.try_clone()?;
    let flag = streaming.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(3));
        if !flag.load(Ordering::Relaxed) && nudge.write_all(&[0x15, 0x00]).is_err() {
            return;
        }
    });

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
        let report = wm.handle(&buf[..n])?;
        let on = wm.streaming();
        if on != streaming.swap(on, Ordering::Relaxed) {
            if on {
                let _ = fs::write(&ready, "");
            } else {
                let _ = fs::remove_file(&ready);
            }
        }
        if let Some(g) = report {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_left_by_a_dead_bridge_is_reused_but_a_live_one_is_not() {
        let dir = std::env::temp_dir().join(format!("yargos-slots-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let d = dir.to_str().unwrap();
        let (a, keep_a) = claim_slot_in(d);
        let (b, _keep_b) = claim_slot_in(d);
        assert_eq!((a, b), (1, 2));
        // Slot 1 is abandoned without cleanup (as when the process is killed), its owner no longer exists.
        std::mem::forget(keep_a);
        fs::write(dir.join("slot1"), "4294967295").unwrap();
        let (again, _keep) = claim_slot_in(d);
        assert_eq!(again, 1);
        // Slots 1 and 2 belong to this very process, which is alive, so a new claim goes to 3.
        assert_eq!(claim_slot_in(d).0, 3);
        let _ = fs::remove_dir_all(&dir);
    }
}

// Turning inputs into dashboard navigation: keyboard keysyms, and joystick/gamepad/instrument events
// read straight from the kernel's evdev nodes (the same devices YARG sees).

use crate::event::Event;
use crate::model::Nav;
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::raw::{c_int, c_ulong};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---- keyboard ---------------------------------------------------------------------------------

pub fn keysym_to_nav(sym: u32) -> Option<Nav> {
    Some(match sym {
        0xff52 | 0xff97 => Nav::Up,
        0xff54 | 0xff99 => Nav::Down,
        0xff51 | 0xff96 => Nav::Left,
        0xff53 | 0xff98 => Nav::Right,
        0xff0d | 0xff8d => Nav::Submit,
        0xff1b => Nav::Back,
        0xff08 | 0xffff => Nav::Backspace,
        0x20..=0x7e => Nav::Char(char::from_u32(sym)?),
        _ => return None,
    })
}

// ---- pads and instruments -----------------------------------------------------------------------

const BTN_SOUTH: u16 = 0x130;
const BTN_EAST: u16 = 0x131;
const BTN_C: u16 = 0x132;
const BTN_NORTH: u16 = 0x133;
const BTN_START: u16 = 0x13b;
const BTN_DPAD_UP: u16 = 0x220;
const BTN_DPAD_DOWN: u16 = 0x221;
const BTN_DPAD_LEFT: u16 = 0x222;
const BTN_DPAD_RIGHT: u16 = 0x223;
const ABS_HAT0X: u16 = 0x10;
const ABS_HAT0Y: u16 = 0x11;

/// The PS3 instruments the bridge creates (and real ones) use the PlayStation vendor id.
pub const INSTRUMENT_VENDOR: u16 = 0x12BA;

/// One evdev event from a pad -> a navigation action. Instruments use the same face buttons
/// differently from a normal gamepad: green = OK and red = Back like in YARG's own menus, the two
/// outer frets/pads move left and right, and strum / cymbals move up and down through the hat.
pub fn pad_nav(instrument: bool, ty: u16, code: u16, value: i32) -> Option<Nav> {
    match ty {
        1 if value == 1 => match (instrument, code) {
            (_, BTN_DPAD_UP) => Some(Nav::Up),
            (_, BTN_DPAD_DOWN) => Some(Nav::Down),
            (_, BTN_DPAD_LEFT) => Some(Nav::Left),
            (_, BTN_DPAD_RIGHT) => Some(Nav::Right),
            (true, BTN_EAST) => Some(Nav::Ok),
            (true, BTN_C) => Some(Nav::Back),
            (true, BTN_SOUTH) => Some(Nav::Left),
            (true, BTN_NORTH) => Some(Nav::Right),
            (false, BTN_SOUTH) | (false, BTN_START) => Some(Nav::Ok),
            (false, BTN_EAST) => Some(Nav::Back),
            _ => None,
        },
        3 => match (code, value) {
            (ABS_HAT0X, -1) => Some(Nav::Left),
            (ABS_HAT0X, 1) => Some(Nav::Right),
            (ABS_HAT0Y, -1) => Some(Nav::Up),
            (ABS_HAT0Y, 1) => Some(Nav::Down),
            _ => None,
        },
        _ => None,
    }
}

// Linux ioctl numbers.
fn ioc(dir: c_ulong, nr: c_ulong, size: c_ulong) -> c_ulong {
    (dir << 30) | (size << 16) | ((b'E' as c_ulong) << 8) | nr
}

extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

fn has_bit(map: &[u8], bit: usize) -> bool {
    map.get(bit / 8).map(|b| b & (1 << (bit % 8)) != 0).unwrap_or(false)
}

struct PadInfo {
    vendor: u16,
}

/// Is this evdev node a pad / joystick / instrument (as opposed to a keyboard, mouse, button)?
fn probe(f: &File) -> Option<PadInfo> {
    let fd = f.as_raw_fd();
    let mut keys = [0u8; 96];
    let mut abs = [0u8; 8];
    unsafe {
        if ioctl(fd, ioc(2, 0x20 + 1, keys.len() as c_ulong), keys.as_mut_ptr()) < 0 {
            return None;
        }
        let _ = ioctl(fd, ioc(2, 0x20 + 3, abs.len() as c_ulong), abs.as_mut_ptr());
    }
    let pad_buttons = (0x120..=0x13e).any(|b| has_bit(&keys, b));
    let hat = has_bit(&abs, ABS_HAT0X as usize);
    let keyboard = has_bit(&keys, 30); // KEY_A
    if keyboard || !(pad_buttons || hat) {
        return None;
    }
    let mut id = [0u16; 4]; // bustype, vendor, product, version
    unsafe {
        if ioctl(fd, ioc(2, 0x02, 8), id.as_mut_ptr()) < 0 {
            return None;
        }
    }
    Some(PadInfo { vendor: id[1] })
}

/// Watch for pads (also ones plugged in later) and forward their navigation while `active`.
pub fn spawn_pads(tx: Sender<Event>, active: Arc<AtomicBool>) {
    let watched: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    std::thread::spawn(move || loop {
        if let Ok(dir) = std::fs::read_dir("/dev/input") {
            for e in dir.filter_map(|e| e.ok()) {
                let name = e.file_name().to_string_lossy().to_string();
                if !name.starts_with("event") || watched.lock().unwrap().contains(&name) {
                    continue;
                }
                let Ok(f) = File::open(e.path()) else { continue };
                let Some(info) = probe(&f) else { continue };
                watched.lock().unwrap().insert(name.clone());
                let (tx, active, watched) = (tx.clone(), active.clone(), watched.clone());
                std::thread::spawn(move || {
                    let mut f = f;
                    let instrument = info.vendor == INSTRUMENT_VENDOR;
                    let mut buf = [0u8; 24 * 16];
                    while let Ok(n) = f.read(&mut buf) {
                        for chunk in buf[..n / 24 * 24].chunks_exact(24) {
                            let ty = u16::from_le_bytes([chunk[16], chunk[17]]);
                            let code = u16::from_le_bytes([chunk[18], chunk[19]]);
                            let value = i32::from_le_bytes([chunk[20], chunk[21], chunk[22], chunk[23]]);
                            if active.load(Ordering::Relaxed) {
                                if let Some(nav) = pad_nav(instrument, ty, code, value) {
                                    let _ = tx.send(Event::Nav(nav));
                                }
                            }
                        }
                    }
                    watched.lock().unwrap().remove(&name);
                });
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_keys_map_to_navigation_and_text() {
        assert_eq!(keysym_to_nav(0xff52), Some(Nav::Up));
        assert_eq!(keysym_to_nav(0xff0d), Some(Nav::Submit));
        assert_eq!(keysym_to_nav(0xff1b), Some(Nav::Back));
        assert_eq!(keysym_to_nav(0xff08), Some(Nav::Backspace));
        assert_eq!(keysym_to_nav('a' as u32), Some(Nav::Char('a')));
        assert_eq!(keysym_to_nav('@' as u32), Some(Nav::Char('@')));
        assert_eq!(keysym_to_nav(0xffe1), None); // Shift_L alone does nothing
    }

    #[test]
    fn guitar_strum_and_frets_navigate() {
        // strum up/down arrive as hat Y; green = OK, red = Back, yellow/blue = left/right
        assert_eq!(pad_nav(true, 3, ABS_HAT0Y, -1), Some(Nav::Up));
        assert_eq!(pad_nav(true, 3, ABS_HAT0Y, 1), Some(Nav::Down));
        assert_eq!(pad_nav(true, 3, ABS_HAT0Y, 0), None); // returning to neutral
        assert_eq!(pad_nav(true, 1, BTN_EAST, 1), Some(Nav::Ok));
        assert_eq!(pad_nav(true, 1, BTN_C, 1), Some(Nav::Back));
        assert_eq!(pad_nav(true, 1, BTN_SOUTH, 1), Some(Nav::Left));
        assert_eq!(pad_nav(true, 1, BTN_NORTH, 1), Some(Nav::Right));
    }

    #[test]
    fn releases_and_unmapped_buttons_do_nothing() {
        assert_eq!(pad_nav(true, 1, BTN_EAST, 0), None);
        assert_eq!(pad_nav(true, 1, 0x136, 1), None); // a fret we do not use
        assert_eq!(pad_nav(false, 0, 0, 0), None);
    }

    #[test]
    fn a_normal_gamepad_uses_a_for_ok_and_b_for_back() {
        assert_eq!(pad_nav(false, 1, BTN_SOUTH, 1), Some(Nav::Ok));
        assert_eq!(pad_nav(false, 1, BTN_EAST, 1), Some(Nav::Back));
        assert_eq!(pad_nav(false, 1, BTN_START, 1), Some(Nav::Ok));
        assert_eq!(pad_nav(false, 3, ABS_HAT0X, 1), Some(Nav::Right));
        assert_eq!(pad_nav(false, 1, BTN_DPAD_UP, 1), Some(Nav::Up));
    }
}

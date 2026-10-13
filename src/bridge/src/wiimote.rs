use crate::ps3::Guitar;
use std::fs::File;
use std::io::{self, Write};

const REG_EXT_INIT1: u32 = 0x04A4_00F0;
const REG_EXT_INIT2: u32 = 0x04A4_00FB;
const REG_EXT_IDENT: u32 = 0x04A4_00FA;
const GUITAR_IDENT: [u8; 6] = [0x00, 0x00, 0xA4, 0x20, 0x01, 0x03];

const MODE_CORE_ACCEL_EXT16: u8 = 0x35;
const MODE_CORE: u8 = 0x30;

// Resting/maximum raw 5-bit whammy values; to be confirmed on real guitars.
const WHAMMY_REST_DEFAULT: u8 = 0x0F;
const WHAMMY_MAX: u8 = 0x1D;

#[derive(PartialEq)]
enum State {
    WaitStatus,
    WaitIdent,
    Streaming,
}

// The accelerometer jitters by a unit or two at rest; ignore changes smaller than this (of 0..=255).
const TILT_DEADBAND: u16 = 8;

/// What a status report says about the Wiimote's batteries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Battery {
    /// 0..=100, from the report's level byte (0xC8 = full).
    pub percent: u8,
    /// The Wiimote's own "batteries nearly empty" flag.
    pub low: bool,
}

pub struct Wiimote {
    dev: File,
    state: State,
    battery: Option<Battery>,
    whammy_rest: u8,
    last_tilt: u16,
    // Touch-bar value seen when streaming started (nothing touched). World Tour guitars idle at
    // 0x0F; guitars without a touch bar (Les Paul) read a constant 0x1F, i.e. "orange" if taken literally.
    slider_rest: Option<u8>,
}

impl Wiimote {
    pub fn new(dev: File) -> Self {
        Self { dev, state: State::WaitStatus, battery: None, whammy_rest: WHAMMY_REST_DEFAULT, last_tilt: 0x200, slider_rest: None }
    }

    /// The batteries as the last status report said (None until one arrived).
    pub fn battery(&self) -> Option<Battery> {
        self.battery
    }


    pub fn start(&mut self, slot: u8) -> io::Result<()> {
        let leds = 0x10u8 << (slot.saturating_sub(1).min(3));
        self.dev.write_all(&[0x11, leds])?;
        self.request_status()
    }

    fn request_status(&mut self) -> io::Result<()> {
        self.dev.write_all(&[0x15, 0x00])
    }

    /// Has the guitar been identified and its reports are flowing?
    pub fn streaming(&self) -> bool {
        self.state == State::Streaming
    }

    fn set_mode(&mut self, mode: u8) -> io::Result<()> {
        self.dev.write_all(&[0x12, 0x00, mode])
    }

    fn write_register(&mut self, addr: u32, value: u8) -> io::Result<()> {
        let a = addr.to_be_bytes();
        let mut r = [0u8; 22];
        r[0] = 0x16;
        r[1..5].copy_from_slice(&a);
        r[5] = 1;
        r[6] = value;
        self.dev.write_all(&r)
    }

    fn read_register(&mut self, addr: u32, len: u16) -> io::Result<()> {
        let a = addr.to_be_bytes();
        let l = len.to_be_bytes();
        self.dev.write_all(&[0x17, a[0], a[1], a[2], a[3], l[0], l[1]])
    }

    pub fn handle(&mut self, buf: &[u8]) -> io::Result<Option<Guitar>> {
        match buf.first() {
            Some(0x20) if buf.len() >= 4 => {
                let ext_connected = buf[3] & 0x02 != 0;
                if buf.len() >= 7 {
                    self.battery = Some(Battery { percent: (buf[6] as u32 * 100 / 200).min(100) as u8, low: buf[3] & 0x01 != 0 });
                }
                // An answer to a status request made while the guitar is already streaming: note the battery and keep
                // going. (Starting the setup again would recalibrate the touch bar under the player's fingers.)
                if self.state == State::Streaming && ext_connected {
                    self.set_mode(MODE_CORE_ACCEL_EXT16)?;
                    return Ok(None);
                }
                self.state = State::WaitStatus;
                if ext_connected {
                    self.write_register(REG_EXT_INIT1, 0x55)?;
                    self.write_register(REG_EXT_INIT2, 0x00)?;
                    self.read_register(REG_EXT_IDENT, 6)?;
                    self.state = State::WaitIdent;
                } else {
                    self.set_mode(MODE_CORE)?;
                }
                Ok(None)
            }
            Some(0x21) if buf.len() >= 12 && self.state == State::WaitIdent => {
                let error = buf[3] & 0x0F;
                if error == 0 && buf[6..12] == GUITAR_IDENT {
                    self.whammy_rest = WHAMMY_REST_DEFAULT;
                    self.slider_rest = None;
                    self.set_mode(MODE_CORE_ACCEL_EXT16)?;
                    self.state = State::Streaming;
                } else {
                    eprintln!("yargos-bridge: unsupported extension, ident {:02x?}", &buf[6..12.min(buf.len())]);
                }
                Ok(None)
            }
            Some(0x35) if buf.len() >= 12 && self.state == State::Streaming => {
                Ok(Some(self.decode_guitar(buf)))
            }
            _ => Ok(None),
        }
    }

    fn decode_guitar(&mut self, buf: &[u8]) -> Guitar {
        let core = [buf[1], buf[2]];
        let accel_y = buf[4];
        let ext = &buf[6..12];

        let touch = ext[2] & 0x1F;
        let whammy_raw = ext[3] & 0x1F;
        let b4 = ext[4];
        let b5 = ext[5];

        let mut g = Guitar::idle();
        g.green = b5 & 0x10 == 0;
        g.red = b5 & 0x40 == 0;
        g.yellow = b5 & 0x08 == 0;
        g.blue = b5 & 0x20 == 0;
        g.orange = b5 & 0x80 == 0;
        g.strum_up = b5 & 0x01 == 0;
        g.strum_down = b4 & 0x40 == 0;
        g.start = b4 & 0x04 == 0;
        g.select = b4 & 0x10 == 0;
        g.guide = core[1] & 0x80 != 0;

        self.whammy_rest = self.whammy_rest.min(whammy_raw);
        let span = WHAMMY_MAX.saturating_sub(self.whammy_rest).saturating_sub(1).max(1) as u16;
        // One step of whammy noise at rest should not register as movement.
        let pressed = whammy_raw.saturating_sub(self.whammy_rest).saturating_sub(1) as u16;
        g.whammy = 0x80 + ((pressed.min(span) * 0x7F) / span) as u8;

        let slider_rest = *self.slider_rest.get_or_insert(touch);
        if touch != slider_rest {
            g.slider = ((touch as u16 * 0xFF) / 0x1F) as u8;
        }

        let tilt = 0x180 + (accel_y as u16 * 0x100) / 0xFF;
        if tilt.abs_diff(self.last_tilt) >= TILT_DEADBAND {
            self.last_tilt = tilt;
        }
        g.tilt = self.last_tilt;
        g
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wm() -> Wiimote {
        let f = std::fs::OpenOptions::new().write(true).open("/dev/null").unwrap();
        let mut w = Wiimote::new(f);
        w.state = State::Streaming;
        w
    }

    fn report(b4: u8, b5: u8) -> [u8; 22] {
        let mut r = [0u8; 22];
        r[0] = 0x35;
        r[6 + 2] = 0x0F;
        r[6 + 3] = 0x0F;
        r[6 + 4] = b4;
        r[6 + 5] = b5;
        r
    }

    #[test]
    fn nothing_pressed() {
        let g = wm().handle(&report(0xFF, 0xFF)).unwrap().unwrap();
        assert!(!g.green && !g.strum_up && !g.strum_down && !g.start);
        assert_eq!(g.whammy, 0x80);
        assert_eq!(g.slider, 0x80);
    }

    #[test]
    fn green_and_strum_down() {
        let g = wm().handle(&report(0xFF & !0x40, 0xFF & !0x10)).unwrap().unwrap();
        assert!(g.green && g.strum_down && !g.strum_up);
    }

    #[test]
    fn whammy_full() {
        let mut r = report(0xFF, 0xFF);
        r[6 + 3] = WHAMMY_MAX;
        let g = wm().handle(&r).unwrap().unwrap();
        assert_eq!(g.whammy, 0xFF);
    }

    #[test]
    fn accelerometer_jitter_does_not_change_the_report() {
        let mut w = wm();
        let mut r = report(0xFF, 0xFF);
        r[4] = 128;
        let first = w.handle(&r).unwrap().unwrap();
        for y in [127u8, 129, 126, 130, 128, 131] {
            r[4] = y;
            assert!(w.handle(&r).unwrap().unwrap() == first, "y={y}");
        }
        r[4] = 200; // a real tilt
        assert!(w.handle(&r).unwrap().unwrap() != first);
    }

    #[test]
    fn whammy_noise_of_one_step_is_ignored() {
        let mut w = wm();
        let mut r = report(0xFF, 0xFF);
        r[6 + 3] = WHAMMY_REST_DEFAULT + 1;
        assert_eq!(w.handle(&r).unwrap().unwrap().whammy, 0x80);
        r[6 + 3] = WHAMMY_MAX;
        assert_eq!(w.handle(&r).unwrap().unwrap().whammy, 0xFF);
    }

    #[test]
    fn guitar_without_touch_bar_never_reports_a_slider_touch() {
        let mut w = wm();
        let mut r = report(0xFF, 0xFF);
        r[6 + 2] = 0x1F; // Les Paul: constant 0x1F
        for _ in 0..3 {
            assert_eq!(w.handle(&r).unwrap().unwrap().slider, 0x80);
        }
    }

    #[test]
    fn touch_bar_reports_a_touch_relative_to_its_rest_value() {
        let mut w = wm();
        let mut r = report(0xFF, 0xFF);
        assert_eq!(w.handle(&r).unwrap().unwrap().slider, 0x80); // rest 0x0F
        r[6 + 2] = 0x04; // green
        assert_eq!(w.handle(&r).unwrap().unwrap().slider, 0x20);
        r[6 + 2] = 0x1F; // orange
        assert_eq!(w.handle(&r).unwrap().unwrap().slider, 0xFF);
    }

    #[test]
    fn a_status_report_gives_the_battery_and_a_streaming_guitar_keeps_streaming() {
        let mut w = wm();
        assert!(w.battery().is_none());
        // 0xC8 is full, 0x64 half, flag bit 0 is "low", bit 1 "extension connected".
        assert!(w.handle(&[0x20, 0, 0, 0x02, 0, 0, 0xC8]).unwrap().is_none());
        assert_eq!(w.battery(), Some(Battery { percent: 100, low: false }));
        assert!(w.streaming(), "an answer while streaming does not start the setup again");
        w.handle(&[0x20, 0, 0, 0x03, 0, 0, 0x14]).unwrap();
        assert_eq!(w.battery(), Some(Battery { percent: 10, low: true }));
        assert!(w.streaming());
        w.handle(&[0x20, 0, 0, 0x02, 0, 0, 0xFF]).unwrap();
        assert_eq!(w.battery().unwrap().percent, 100, "never above 100");
        // The guitar being unplugged is not an answer to keep streaming through: the setup restarts.
        w.handle(&[0x20, 0, 0, 0x00, 0, 0, 0x64]).unwrap();
        assert!(!w.streaming());
        assert_eq!(w.battery(), Some(Battery { percent: 50, low: false }));
        // A short report (no battery byte) changes nothing.
        let before = w.battery();
        w.handle(&[0x20, 0, 0, 0x00]).unwrap();
        assert_eq!(w.battery(), before);
    }

    #[test]
    fn ignores_data_before_ext_init() {
        let mut w = wm();
        w.state = State::WaitStatus;
        assert!(w.handle(&report(0xFF, 0xFF)).unwrap().is_none());
    }
}

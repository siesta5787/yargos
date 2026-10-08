// Reads a wired Xbox 360 drum kit (ION Drum Rocker) through the kernel's xpad
// evdev node and turns it into a PS3-style drum state.
//
// Xbox 360 layout, per PlasticBand "Xbox 360 Rock Band Drums":
//   colours  A/B/X/Y     pad flag = right stick click   cymbal flag = right bumper
//   yellow/blue cymbals  also send d-pad up/down
//   kicks    left bumper / left stick click
//   velocity left X = red, left Y = yellow, right X = blue, right Y = green,
//            low 15 bits, 0 = hardest hit
// xpad reports the Y axes inverted (~raw), so they are un-inverted here.

use crate::ps3drums::Drums;
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::raw::{c_int, c_ulong};

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;
const SYN_REPORT: u16 = 0;

const BTN_A: u16 = 0x130;
const BTN_B: u16 = 0x131;
const BTN_X: u16 = 0x133;
const BTN_Y: u16 = 0x134;
const BTN_TL: u16 = 0x136;
const BTN_TR: u16 = 0x137;
const BTN_SELECT: u16 = 0x13A;
const BTN_START: u16 = 0x13B;
const BTN_MODE: u16 = 0x13C;
const BTN_THUMBL: u16 = 0x13D;
const BTN_THUMBR: u16 = 0x13E;
const BTN_DPAD_UP: u16 = 0x220;
const BTN_DPAD_DOWN: u16 = 0x221;

const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_RX: u16 = 0x03;
const ABS_RY: u16 = 0x04;

const EVIOCGRAB: c_ulong = 0x4004_4590;
const EVENT_SIZE: usize = 24;

extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

fn velocity(raw: i16) -> u8 {
    (((raw as u16) & 0x7FFF) >> 7) as u8
}

pub struct XDrums {
    state: Drums,
    // Latest velocity per axis: red, yellow, blue, green. The kit's piezo sensors
    // report small spikes from desk vibration with no pad flag, so these are only
    // forwarded while a colour flag is actually set.
    raw_vel: [u8; 4],
}

impl XDrums {
    pub fn new() -> Self {
        Self { state: Drums::idle(), raw_vel: [0xFF; 4] }
    }

    fn frame(&self) -> Drums {
        let mut out = self.state;
        if out.red || out.yellow || out.blue || out.green {
            out.vel_red = self.raw_vel[0];
            out.vel_yellow = self.raw_vel[1];
            out.vel_blue = self.raw_vel[2];
            out.vel_green = self.raw_vel[3];
        }
        out
    }

    // Returns the new state when a SYN_REPORT completes a frame.
    pub fn handle(&mut self, ty: u16, code: u16, value: i32) -> Option<Drums> {
        let s = &mut self.state;
        let on = value != 0;
        match ty {
            EV_KEY => match code {
                BTN_A => s.green = on,
                BTN_B => s.red = on,
                BTN_X => s.blue = on,
                BTN_Y => s.yellow = on,
                BTN_THUMBR => s.pad = on,
                BTN_TR => s.cymbal = on,
                BTN_TL => s.kick1 = on,
                BTN_THUMBL => s.kick2 = on,
                BTN_DPAD_UP => s.dpad_up = on,
                BTN_DPAD_DOWN => s.dpad_down = on,
                BTN_START => s.start = on,
                BTN_SELECT => s.select = on,
                BTN_MODE => s.guide = on,
                _ => {}
            },
            EV_ABS => {
                let v = value as i16;
                match code {
                    ABS_X => self.raw_vel[0] = velocity(v),
                    ABS_Y => self.raw_vel[1] = velocity(!v),
                    ABS_RX => self.raw_vel[2] = velocity(v),
                    ABS_RY => self.raw_vel[3] = velocity(!v),
                    _ => {}
                }
            }
            EV_SYN if code == SYN_REPORT => return Some(self.frame()),
            _ => {}
        }
        None
    }
}

// Take the device exclusively so Unity doesn't also see a duplicate gamepad.
pub fn grab(dev: &File) -> io::Result<()> {
    let one: c_int = 1;
    let rc = unsafe { ioctl(dev.as_raw_fd(), EVIOCGRAB, one) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn read_events(dev: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let n = dev.read(buf)?;
    Ok(n / EVENT_SIZE * EVENT_SIZE)
}

pub fn parse_event(chunk: &[u8]) -> (u16, u16, i32) {
    let ty = u16::from_le_bytes([chunk[16], chunk[17]]);
    let code = u16::from_le_bytes([chunk[18], chunk[19]]);
    let value = i32::from_le_bytes([chunk[20], chunk[21], chunk[22], chunk[23]]);
    (ty, code, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(x: &mut XDrums, evs: &[(u16, u16, i32)]) -> Drums {
        for &(t, c, v) in evs {
            x.handle(t, c, v);
        }
        x.handle(EV_SYN, SYN_REPORT, 0).unwrap()
    }

    #[test]
    fn red_pad_with_velocity() {
        let mut x = XDrums::new();
        let d = frame(&mut x, &[(EV_KEY, BTN_B, 1), (EV_KEY, BTN_THUMBR, 1), (EV_ABS, ABS_X, 0x1000)]);
        assert!(d.red && d.pad && !d.cymbal);
        assert_eq!(d.vel_red, 0x20);
    }

    #[test]
    fn yellow_cymbal_uninverts_y_axis() {
        let mut x = XDrums::new();
        // raw 0x8100 (top bit set, as the kit sends for yellow) arrives as ~raw
        let raw: u16 = 0x8100;
        let evdev = !(raw as i16);
        let d = frame(
            &mut x,
            &[(EV_KEY, BTN_Y, 1), (EV_KEY, BTN_TR, 1), (EV_KEY, BTN_DPAD_UP, 1), (EV_ABS, ABS_Y, evdev as i32)],
        );
        assert!(d.yellow && d.cymbal && d.dpad_up && !d.pad);
        assert_eq!(d.vel_yellow, 0x02);
    }

    #[test]
    fn hardest_and_softest_hit() {
        assert_eq!(velocity(0), 0x00);
        assert_eq!(velocity(0x7FFF), 0xFF);
        assert_eq!(velocity(i16::MIN), 0x00); // top bit ignored
    }

    #[test]
    fn kicks_release() {
        let mut x = XDrums::new();
        let d = frame(&mut x, &[(EV_KEY, BTN_TL, 1), (EV_KEY, BTN_THUMBL, 1)]);
        assert!(d.kick1 && d.kick2);
        let d = frame(&mut x, &[(EV_KEY, BTN_TL, 0)]);
        assert!(!d.kick1 && d.kick2);
    }

    #[test]
    fn velocity_noise_without_a_hit_is_ignored() {
        let mut x = XDrums::new();
        let idle = frame(&mut x, &[]);
        let noisy = frame(&mut x, &[(EV_ABS, ABS_X, 0x7FFF), (EV_ABS, ABS_RX, 0x0001)]);
        assert!(idle == noisy);
        assert_eq!(noisy.vel_red, 0xFF);
    }

    #[test]
    fn no_report_until_syn() {
        let mut x = XDrums::new();
        assert!(x.handle(EV_KEY, BTN_A, 1).is_none());
    }

    #[test]
    fn parses_raw_event() {
        let mut e = [0u8; EVENT_SIZE];
        e[16..18].copy_from_slice(&EV_KEY.to_le_bytes());
        e[18..20].copy_from_slice(&BTN_A.to_le_bytes());
        e[20..24].copy_from_slice(&1i32.to_le_bytes());
        assert_eq!(parse_event(&e), (EV_KEY, BTN_A, 1));
    }
}

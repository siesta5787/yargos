// Virtual PS3 Guitar Hero World Tour guitar (12BA:0100), per PlasticBand's
// "PS3 Guitar Hero Guitars" and "PS3 Base Report Format" docs.

pub const VENDOR: u32 = 0x12BA;
pub const PRODUCT: u32 = 0x0100;
pub const VERSION: u32 = 0x0108;
pub const NAME: &str = "Guitar Hero4 for PlayStation (R) 3";

pub const DESCRIPTOR: &[u8] = &[
    0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0x15, 0x00, 0x25, 0x01, 0x35, 0x00, 0x45, 0x01, 0x75, 0x01,
    0x95, 0x0D, 0x05, 0x09, 0x19, 0x01, 0x29, 0x0D, 0x81, 0x02, 0x95, 0x03, 0x81, 0x01, 0x05, 0x01,
    0x25, 0x07, 0x46, 0x3B, 0x01, 0x75, 0x04, 0x95, 0x01, 0x65, 0x14, 0x09, 0x39, 0x81, 0x42, 0x65,
    0x00, 0x95, 0x01, 0x81, 0x01, 0x26, 0xFF, 0x00, 0x46, 0xFF, 0x00, 0x09, 0x30, 0x09, 0x31, 0x09,
    0x32, 0x09, 0x35, 0x75, 0x08, 0x95, 0x04, 0x81, 0x02, 0x06, 0x00, 0xFF, 0x09, 0x20, 0x09, 0x21,
    0x09, 0x22, 0x09, 0x23, 0x09, 0x24, 0x09, 0x25, 0x09, 0x26, 0x09, 0x27, 0x09, 0x28, 0x09, 0x29,
    0x09, 0x2A, 0x09, 0x2B, 0x95, 0x0C, 0x81, 0x02, 0x0A, 0x21, 0x26, 0x95, 0x08, 0xB1, 0x02, 0x0A,
    0x21, 0x26, 0x91, 0x02, 0x26, 0xFF, 0x03, 0x46, 0xFF, 0x03, 0x09, 0x2C, 0x09, 0x2D, 0x09, 0x2E,
    0x09, 0x2F, 0x75, 0x10, 0x95, 0x04, 0x81, 0x02, 0xC0,
];

pub const REPORT_LEN: usize = 27;
const DPAD_NEUTRAL: u8 = 0x1F;
const DPAD_UP: u8 = 0;
const DPAD_DOWN: u8 = 4;
const ACCEL_REST: u16 = 0x200;

#[derive(Default, Clone, Copy, PartialEq)]
pub struct Guitar {
    pub green: bool,
    pub red: bool,
    pub yellow: bool,
    pub blue: bool,
    pub orange: bool,
    pub strum_up: bool,
    pub strum_down: bool,
    pub start: bool,
    pub select: bool,
    pub guide: bool,
    pub sp_pedal: bool,
    // 0x80 at rest, up to 0xFF fully pressed
    pub whammy: u8,
    // 0x80 at rest
    pub slider: u8,
    // nominal 0x180..=0x280, 0x200 at rest
    pub tilt: u16,
}

impl Guitar {
    pub fn idle() -> Self {
        Self { whammy: 0x80, slider: 0x80, tilt: ACCEL_REST, ..Self::default() }
    }

    pub fn to_report(&self) -> [u8; REPORT_LEN] {
        let mut r = [0u8; REPORT_LEN];
        let mut b0 = 0u8;
        if self.yellow { b0 |= 1 << 0; }
        if self.green { b0 |= 1 << 1; }
        if self.red { b0 |= 1 << 2; }
        if self.blue { b0 |= 1 << 3; }
        if self.orange { b0 |= 1 << 4; }
        if self.sp_pedal { b0 |= 1 << 5; }
        let mut b1 = 0u8;
        if self.select { b1 |= 1 << 0; }
        if self.start { b1 |= 1 << 1; }
        if self.guide { b1 |= 1 << 4; }
        r[0] = b0;
        r[1] = b1;
        r[2] = match (self.strum_up, self.strum_down) {
            (true, false) => DPAD_UP,
            (false, true) => DPAD_DOWN,
            _ => DPAD_NEUTRAL,
        };
        r[3] = 0x80;
        r[4] = 0x80;
        r[5] = self.whammy;
        r[6] = self.slider;
        let tilt = self.tilt.clamp(0x180, 0x280);
        r[19..21].copy_from_slice(&tilt.to_le_bytes());
        r[21..23].copy_from_slice(&0x180u16.to_le_bytes());
        r[23..25].copy_from_slice(&ACCEL_REST.to_le_bytes());
        r[25..27].copy_from_slice(&ACCEL_REST.to_le_bytes());
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_report_layout() {
        let r = Guitar::idle().to_report();
        assert_eq!(r.len(), 27);
        assert_eq!(r[2], 0x1F);
        assert_eq!(r[5], 0x80);
        assert_eq!(r[6], 0x80);
        assert_eq!(u16::from_le_bytes([r[19], r[20]]), 0x200);
    }

    #[test]
    fn frets_and_strum() {
        let g = Guitar { green: true, orange: true, strum_down: true, start: true, ..Guitar::idle() };
        let r = g.to_report();
        assert_eq!(r[0], 0b0001_0010);
        assert_eq!(r[1], 0b10);
        assert_eq!(r[2], 4);
    }

    #[test]
    fn descriptor_ends_collection() {
        assert_eq!(*DESCRIPTOR.last().unwrap(), 0xC0);
        assert_eq!(DESCRIPTOR.len(), 137);
    }
}

// Virtual PS3 Rock Band 2 drum kit (12BA:0210), per PlasticBand's
// "PS3 and Wii Rock Band Drums" and "PS3 Base Report Format" docs.

pub const VENDOR: u32 = 0x12BA;
pub const PRODUCT: u32 = 0x0210;
pub const VERSION: u32 = 0x0200;
pub const NAME: &str = "Harmonix Drum Kit for PlayStation(R)3";

// Report descriptor dumped from a real RB2 PS3 kit (PlasticBand).
pub const DESCRIPTOR: &[u8] = &[
    0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0x05, 0x09, 0x19, 0x01, 0x29, 0x0D, 0x15, 0x00, 0x25, 0x01,
    0x75, 0x01, 0x95, 0x0D, 0x81, 0x02, 0x75, 0x03, 0x95, 0x01, 0x81, 0x03, 0x05, 0x01, 0x09, 0x39,
    0x15, 0x00, 0x25, 0x07, 0x35, 0x00, 0x46, 0x3B, 0x01, 0x65, 0x14, 0x75, 0x04, 0x95, 0x01, 0x81,
    0x42, 0x75, 0x04, 0x95, 0x01, 0x81, 0x03, 0x09, 0x30, 0x09, 0x31, 0x09, 0x32, 0x09, 0x35, 0x15,
    0x00, 0x26, 0xFF, 0x00, 0x35, 0x00, 0x46, 0xFF, 0x00, 0x65, 0x00, 0x75, 0x08, 0x95, 0x04, 0x81,
    0x02, 0x06, 0x00, 0xFF, 0x09, 0x20, 0x09, 0x21, 0x09, 0x22, 0x09, 0x23, 0x09, 0x24, 0x09, 0x25,
    0x09, 0x26, 0x09, 0x27, 0x09, 0x28, 0x09, 0x29, 0x09, 0x2A, 0x09, 0x2B, 0x15, 0x00, 0x26, 0xFF,
    0x00, 0x75, 0x08, 0x95, 0x0C, 0x81, 0x02, 0x09, 0x2C, 0x09, 0x2D, 0x09, 0x2E, 0x09, 0x2F, 0x15,
    0x00, 0x26, 0xFF, 0x03, 0x35, 0x00, 0x46, 0xFF, 0x03, 0x75, 0x10, 0x95, 0x04, 0x81, 0x02, 0x0A,
    0x21, 0x26, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x35, 0x00, 0x46, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x08,
    0x91, 0x02, 0x0A, 0x21, 0x26, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x08, 0xB1, 0x02,
    0xC0,
];

pub const REPORT_LEN: usize = 27;
const HAT_NEUTRAL: u8 = 8;
const HAT_UP: u8 = 0;
const HAT_DOWN: u8 = 4;
const ACCEL_REST: u16 = 0x200;

#[derive(Clone, Copy, PartialEq)]
pub struct Drums {
    pub red: bool,
    pub yellow: bool,
    pub blue: bool,
    pub green: bool,
    // Pad/cymbal flags, kept as the kit sends them; YARG decodes the combination.
    pub pad: bool,
    pub cymbal: bool,
    pub kick1: bool,
    pub kick2: bool,
    pub dpad_up: bool,
    pub dpad_down: bool,
    pub start: bool,
    pub select: bool,
    pub guide: bool,
    // 0x00 = hardest hit, 0xFF = softest
    pub vel_red: u8,
    pub vel_yellow: u8,
    pub vel_blue: u8,
    pub vel_green: u8,
}

impl Drums {
    pub fn idle() -> Self {
        Self {
            red: false,
            yellow: false,
            blue: false,
            green: false,
            pad: false,
            cymbal: false,
            kick1: false,
            kick2: false,
            dpad_up: false,
            dpad_down: false,
            start: false,
            select: false,
            guide: false,
            vel_red: 0xFF,
            vel_yellow: 0xFF,
            vel_blue: 0xFF,
            vel_green: 0xFF,
        }
    }

    pub fn to_report(&self) -> [u8; REPORT_LEN] {
        let mut r = [0u8; REPORT_LEN];
        let mut b0 = 0u8;
        if self.blue { b0 |= 1 << 0; }
        if self.green { b0 |= 1 << 1; }
        if self.red { b0 |= 1 << 2; }
        if self.yellow { b0 |= 1 << 3; }
        if self.kick1 { b0 |= 1 << 4; }
        if self.kick2 { b0 |= 1 << 5; }
        let mut b1 = 0u8;
        if self.select { b1 |= 1 << 0; }
        if self.start { b1 |= 1 << 1; }
        if self.pad { b1 |= 1 << 2; }
        if self.cymbal { b1 |= 1 << 3; }
        if self.guide { b1 |= 1 << 4; }
        r[0] = b0;
        r[1] = b1;
        r[2] = match (self.dpad_up, self.dpad_down) {
            (true, false) => HAT_UP,
            (false, true) => HAT_DOWN,
            _ => HAT_NEUTRAL,
        };
        r[3..7].fill(0x80);
        r[11] = self.vel_yellow;
        r[12] = self.vel_red;
        r[13] = self.vel_green;
        r[14] = self.vel_blue;
        for i in 0..4 {
            r[19 + i * 2..21 + i * 2].copy_from_slice(&ACCEL_REST.to_le_bytes());
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_report() {
        let r = Drums::idle().to_report();
        assert_eq!(r[0], 0);
        assert_eq!(r[1], 0);
        assert_eq!(r[2], 8);
        assert_eq!(&r[11..15], &[0xFF; 4]);
    }

    #[test]
    fn red_pad_hit_layout() {
        let d = Drums { red: true, pad: true, vel_red: 0x10, ..Drums::idle() };
        let r = d.to_report();
        assert_eq!(r[0], 0b0000_0100); // circle
        assert_eq!(r[1], 0b0000_0100); // L3
        assert_eq!(r[12], 0x10);
    }

    #[test]
    fn yellow_cymbal_uses_dpad_up_and_r3() {
        let d = Drums { yellow: true, cymbal: true, dpad_up: true, ..Drums::idle() };
        let r = d.to_report();
        assert_eq!(r[0], 0b0000_1000); // triangle
        assert_eq!(r[1], 0b0000_1000); // R3
        assert_eq!(r[2], 0);
    }

    #[test]
    fn kicks_and_blue_green() {
        let d = Drums { blue: true, green: true, kick1: true, kick2: true, dpad_down: true, ..Drums::idle() };
        let r = d.to_report();
        assert_eq!(r[0], 0b0011_0011);
        assert_eq!(r[2], 4);
    }

    #[test]
    fn descriptor_shape() {
        assert_eq!(*DESCRIPTOR.last().unwrap(), 0xC0);
        assert_eq!(&DESCRIPTOR[..4], &[0x05, 0x01, 0x09, 0x05]);
    }
}

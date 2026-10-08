// Early-boot splash: draw the logo straight onto the Linux framebuffer (/dev/fb0) before X starts.
// The picture stays up until Xorg takes the display over, and this program exits right after drawing.

use crate::gfx::{Canvas, Fonts};
use crate::logo::draw_splash;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::raw::{c_int, c_ulong};

extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

const FBIOGET_VSCREENINFO: c_ulong = 0x4600;
const FBIOGET_FSCREENINFO: c_ulong = 0x4602;

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// Fit the 0x00RRGGBB canvas to the framebuffer's channel layout and line length.
pub fn to_fb(cv: &Canvas, line_length: usize, shift: (u32, u32, u32)) -> Vec<u8> {
    let mut out = vec![0u8; line_length * cv.h];
    for y in 0..cv.h {
        for x in 0..cv.w {
            let p = cv.px[y * cv.w + x];
            let v = (((p >> 16) & 255) << shift.0) | (((p >> 8) & 255) << shift.1) | ((p & 255) << shift.2);
            let o = y * line_length + x * 4;
            if o + 4 <= out.len() {
                out[o..o + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
    }
    out
}

pub fn run() -> i32 {
    let Ok(mut fb) = OpenOptions::new().read(true).write(true).open("/dev/fb0") else { return 0 };
    let fd = fb.as_raw_fd();
    let mut var = [0u8; 160];
    let mut fix = [0u8; 80];
    unsafe {
        if ioctl(fd, FBIOGET_VSCREENINFO, var.as_mut_ptr()) < 0 || ioctl(fd, FBIOGET_FSCREENINFO, fix.as_mut_ptr()) < 0 {
            return 0;
        }
    }
    let (w, h, bpp) = (u32_at(&var, 0) as usize, u32_at(&var, 4) as usize, u32_at(&var, 24));
    let shift = (u32_at(&var, 32), u32_at(&var, 44), u32_at(&var, 56)); // red, green, blue offsets
    let line = u32_at(&fix, 48) as usize;
    if bpp != 32 || w == 0 || h == 0 || line < w * 4 {
        return 0;
    }
    let Some(mut fonts) = Fonts::load() else { return 0 };
    let mut cv = Canvas::new(w, h);
    draw_splash(&mut cv, &mut fonts, "Starting\u{2026}", None);
    let frame = to_fb(&cv, line, shift);
    if fb.seek(SeekFrom::Start(0)).is_ok() {
        let _ = fb.write_all(&frame);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framebuffer_conversion_honours_channel_offsets_and_line_length() {
        let mut cv = Canvas::new(2, 1);
        cv.px = vec![0x112233, 0xAABBCC];
        // BGRX layout (red at 16, green at 8, blue at 0), lines padded to 12 bytes
        let b = to_fb(&cv, 12, (16, 8, 0));
        assert_eq!(&b[0..4], &[0x33, 0x22, 0x11, 0x00]);
        assert_eq!(&b[4..8], &[0xCC, 0xBB, 0xAA, 0x00]);
        assert_eq!(b.len(), 12);
        // RGBX layout
        let b = to_fb(&cv, 8, (0, 8, 16));
        assert_eq!(&b[0..4], &[0x11, 0x22, 0x33, 0x00]);
    }
}

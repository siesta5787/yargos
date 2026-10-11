// Software drawing: a 0x00RRGGBB canvas (what X11 and the Linux framebuffer both take as 32 bpp),
// shapes, anti-aliased polygons, text and a PNG writer for previews.

use fontdue::{Font, FontSettings};
use std::collections::HashMap;
use std::io::Write;

pub type Rgb = u32;

pub const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

pub fn lerp_rgb(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let ch = |s: u32| {
        let (x, y) = (((a >> s) & 255) as f32, ((b >> s) & 255) as f32);
        (x + (y - x) * t).round() as u32
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
    /// The backdrop is the same every frame: made once, then copied.
    bg: Vec<u32>,
}

impl Canvas {
    pub fn new(w: usize, h: usize) -> Self {
        Self { w, h, px: vec![0; w * h], bg: vec![] }
    }

    fn blend(&mut self, x: i32, y: i32, color: Rgb, alpha: f32) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h || alpha <= 0.0 {
            return;
        }
        let i = y as usize * self.w + x as usize;
        self.px[i] = if alpha >= 1.0 { color } else { lerp_rgb(self.px[i], color, alpha) };
    }

    pub fn fill(&mut self, color: Rgb) {
        self.px.iter_mut().for_each(|p| *p = color);
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgb, alpha: f32) {
        for yy in y.max(0)..(y + h).min(self.h as i32) {
            for xx in x.max(0)..(x + w).min(self.w as i32) {
                self.blend(xx, yy, color, alpha);
            }
        }
    }

    /// Rounded rectangle with a one-pixel anti-aliased edge.
    pub fn rrect(&mut self, x: i32, y: i32, w: i32, h: i32, r: f32, color: Rgb, alpha: f32) {
        let (cx, cy) = (x as f32 + w as f32 / 2.0, y as f32 + h as f32 / 2.0);
        let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
        for yy in y.max(0)..(y + h).min(self.h as i32) {
            for xx in x.max(0)..(x + w).min(self.w as i32) {
                let (px, py) = (xx as f32 + 0.5 - cx, yy as f32 + 0.5 - cy);
                let (qx, qy) = (px.abs() - (hw - r), py.abs() - (hh - r));
                let d = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
                self.blend(xx, yy, color, alpha * (0.5 - d).clamp(0.0, 1.0));
            }
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, color: Rgb, alpha: f32) {
        for yy in (cy - r - 1.0).floor() as i32..=(cy + r + 1.0).ceil() as i32 {
            for xx in (cx - r - 1.0).floor() as i32..=(cx + r + 1.0).ceil() as i32 {
                let d = (xx as f32 + 0.5 - cx).hypot(yy as f32 + 0.5 - cy) - r;
                self.blend(xx, yy, color, alpha * (0.5 - d).clamp(0.0, 1.0));
            }
        }
    }

    /// The YARG-like backdrop: teal top-left, near-black navy in the middle, dark red bottom-right.
    pub fn backdrop(&mut self) {
        if self.bg.len() == self.px.len() {
            self.px.copy_from_slice(&self.bg);
            return;
        }
        let (teal, navy, red) = (rgb(8, 104, 124), rgb(4, 9, 20), rgb(92, 14, 22));
        for y in 0..self.h {
            for x in 0..self.w {
                let t = (x as f32 / self.w as f32 * 0.55 + y as f32 / self.h as f32 * 0.45).clamp(0.0, 1.0);
                // Hold the dark middle longer so the corners read as soft glows.
                let c = if t < 0.5 {
                    lerp_rgb(teal, navy, (t * 2.0).powf(0.7))
                } else {
                    lerp_rgb(navy, red, ((t - 0.5) * 2.0).powf(1.5))
                };
                self.px[y * self.w + x] = c;
            }
        }
        self.bg = self.px.clone();
    }

    /// Fill polygons (even-odd, so holes work) with 4x4 supersampling. `color_at` gets the pixel's
    /// position relative to the shape's bounding box, 0..1 on both axes.
    pub fn polygons(&mut self, polys: &[Vec<(f32, f32)>], color_at: &dyn Fn(f32, f32) -> Rgb, alpha: f32) {
        const SS: usize = 4;
        let pts = || polys.iter().flatten();
        let minx = pts().map(|p| p.0).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
        let miny = pts().map(|p| p.1).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
        let maxx = pts().map(|p| p.0).fold(f32::MIN, f32::max).ceil().min(self.w as f32) as i32;
        let maxy = pts().map(|p| p.1).fold(f32::MIN, f32::max).ceil().min(self.h as f32) as i32;
        if maxx <= minx || maxy <= miny {
            return;
        }
        let (bw, bh) = ((maxx - minx) as usize, (maxy - miny) as usize);
        let mut cov = vec![0u8; bw * bh];
        for sy in 0..bh * SS {
            let y = miny as f32 + (sy as f32 + 0.5) / SS as f32;
            let mut xs: Vec<f32> = Vec::new();
            for poly in polys {
                for i in 0..poly.len() {
                    let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                    if (a.1 <= y) != (b.1 <= y) {
                        xs.push(a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0));
                    }
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for pair in xs.chunks(2) {
                if pair.len() < 2 {
                    continue;
                }
                let x0 = ((pair[0] - minx as f32) * SS as f32).round().max(0.0) as usize;
                let x1 = (((pair[1] - minx as f32) * SS as f32).round().max(0.0) as usize).min(bw * SS);
                for sx in x0..x1 {
                    cov[(sy / SS) * bw + sx / SS] += 1;
                }
            }
        }
        for py in 0..bh {
            for px in 0..bw {
                let c = cov[py * bw + px];
                if c > 0 {
                    let (u, v) = (px as f32 / bw as f32, py as f32 / bh as f32);
                    let a = (c as f32 / (SS * SS) as f32).min(1.0) * alpha;
                    self.blend(minx + px as i32, miny + py as i32, color_at(u, v), a);
                }
            }
        }
    }

    pub fn save_png(&self, path: &str) -> std::io::Result<()> {
        write_png(path, self.w, self.h, &self.px)
    }
}

// ---- text -----------------------------------------------------------------------------------

pub struct Fonts {
    pub regular: Font,
    pub bold: Font,
    cache: HashMap<(bool, char, u32), (fontdue::Metrics, Vec<u8>)>,
}

const FONT_DIRS: &[&str] = &[
    "/usr/share/fonts/dejavu",
    "/usr/share/fonts/truetype/dejavu",
    "/usr/share/fonts/TTF",
    "/usr/share/fonts/truetype",
];

fn load_font(name: &str) -> Option<Font> {
    for dir in FONT_DIRS {
        if let Ok(bytes) = std::fs::read(format!("{dir}/{name}")) {
            return Font::from_bytes(bytes, FontSettings::default()).ok();
        }
    }
    None
}

impl Fonts {
    pub fn load() -> Option<Self> {
        let regular = load_font("DejaVuSans.ttf")?;
        let bold = load_font("DejaVuSans-Bold.ttf").or_else(|| load_font("DejaVuSans.ttf"))?;
        Some(Self { regular, bold, cache: HashMap::new() })
    }

    fn glyph(&mut self, bold: bool, ch: char, px: f32) -> &(fontdue::Metrics, Vec<u8>) {
        let key = (bold, ch, px.round() as u32);
        let font = if bold { &self.bold } else { &self.regular };
        self.cache.entry(key).or_insert_with(|| font.rasterize(ch, px))
    }

    pub fn width(&mut self, text: &str, px: f32, bold: bool) -> f32 {
        text.chars().map(|c| self.glyph(bold, c, px).0.advance_width).sum()
    }

    /// Draw text with its baseline-top at `y` (y is the top of the line box).
    pub fn draw(&mut self, cv: &mut Canvas, text: &str, x: f32, y: f32, px: f32, bold: bool, color: Rgb, alpha: f32) {
        let ascent = {
            let font = if bold { &self.bold } else { &self.regular };
            font.horizontal_line_metrics(px).map(|m| m.ascent).unwrap_or(px * 0.8)
        };
        let mut pen = x;
        for ch in text.chars() {
            let (m, bitmap) = {
                let g = self.glyph(bold, ch, px);
                (g.0, g.1.clone())
            };
            let gx = pen.round() as i32 + m.xmin;
            let gy = (y + ascent).round() as i32 - m.height as i32 - m.ymin;
            for row in 0..m.height {
                for col in 0..m.width {
                    let a = bitmap[row * m.width + col] as f32 / 255.0;
                    cv.blend(gx + col as i32, gy + row as i32, color, a * alpha);
                }
            }
            pen += m.advance_width;
        }
    }

    pub fn draw_centered(&mut self, cv: &mut Canvas, text: &str, cx: f32, y: f32, px: f32, bold: bool, color: Rgb, alpha: f32) {
        let w = self.width(text, px, bold);
        self.draw(cv, text, cx - w / 2.0, y, px, bold, color, alpha);
    }
}

// ---- PNG (uncompressed deflate blocks; only for previews) -------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

pub fn write_png(path: &str, w: usize, h: usize, px: &[u32]) -> std::io::Result<()> {
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let p = px[y * w + x];
            raw.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
    }
    let mut z = vec![0x78, 0x01];
    for (i, chunk) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(last as u8);
        z.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes());
        z.extend_from_slice(chunk);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = std::fs::File::create(path)?;
    out.write_all(b"\x89PNG\r\n\x1a\n")?;
    let mut chunk = |kind: &[u8; 4], body: &[u8]| -> std::io::Result<()> {
        out.write_all(&(body.len() as u32).to_be_bytes())?;
        let mut tagged = kind.to_vec();
        tagged.extend_from_slice(body);
        out.write_all(&tagged)?;
        out.write_all(&crc32(&tagged).to_be_bytes())
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr)?;
    chunk(b"IDAT", &z)?;
    chunk(b"IEND", &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_endpoints_and_middle() {
        assert_eq!(lerp_rgb(0x000000, 0xFFFFFF, 0.0), 0x000000);
        assert_eq!(lerp_rgb(0x000000, 0xFFFFFF, 1.0), 0xFFFFFF);
        assert_eq!(lerp_rgb(0x000000, 0xFEFEFE, 0.5), 0x7F7F7F);
    }

    #[test]
    fn polygon_fill_with_a_hole_is_even_odd() {
        let mut c = Canvas::new(20, 20);
        let outer = vec![(2.0, 2.0), (18.0, 2.0), (18.0, 18.0), (2.0, 18.0)];
        let hole = vec![(8.0, 8.0), (12.0, 8.0), (12.0, 12.0), (8.0, 12.0)];
        c.polygons(&[outer, hole], &|_, _| 0xFFFFFF, 1.0);
        assert_eq!(c.px[5 * 20 + 5], 0xFFFFFF);
        assert_eq!(c.px[10 * 20 + 10], 0x000000);
        assert_eq!(c.px[0], 0x000000);
    }

    #[test]
    fn png_has_a_signature_and_chunks() {
        let path = std::env::temp_dir().join("yargos-gfx-test.png");
        write_png(path.to_str().unwrap(), 3, 2, &[0xFF0000; 6]).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert!(bytes.windows(4).any(|w| w == b"IDAT"));
        assert!(bytes.ends_with(&[0xAE, 0x42, 0x60, 0x82]));
    }
}

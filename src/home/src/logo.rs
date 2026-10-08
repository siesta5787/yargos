// The YARGOS mark: an angular italic "YARG" wordmark (a nod to the game's own logo, drawn from
// scratch) with an "OS" badge in the gold of YARG's menu selector. Resolution independent.

use crate::gfx::{rgb, Canvas, Fonts, Rgb};

type Poly = Vec<(f32, f32)>;

pub const GOLD: Rgb = rgb(232, 184, 36);
pub const INK: Rgb = rgb(5, 10, 22);

const SKEW: f32 = 0.20;
const GAP: f32 = 16.0;

// Letters on a 100-unit-tall grid (y down); each is a list of polygons, extra ones are holes.
fn letter(c: char) -> (f32, Vec<Poly>) {
    match c {
        'Y' => (92.0, vec![vec![
            (0.0, 0.0), (27.0, 0.0), (46.0, 30.0), (65.0, 0.0), (92.0, 0.0),
            (59.0, 52.0), (59.0, 100.0), (33.0, 100.0), (33.0, 52.0),
        ]]),
        'A' => (100.0, vec![
            vec![(0.0, 100.0), (33.0, 0.0), (67.0, 0.0), (100.0, 100.0), (74.0, 100.0),
                 (67.0, 80.0), (33.0, 80.0), (26.0, 100.0)],
            vec![(39.0, 58.0), (50.0, 24.0), (61.0, 58.0)],
        ]),
        'R' => (94.0, vec![
            vec![(0.0, 0.0), (72.0, 0.0), (92.0, 17.0), (92.0, 45.0), (78.0, 59.0), (94.0, 100.0),
                 (66.0, 100.0), (53.0, 66.0), (27.0, 66.0), (27.0, 100.0), (0.0, 100.0)],
            vec![(27.0, 23.0), (62.0, 23.0), (66.0, 29.0), (66.0, 38.0), (62.0, 43.0), (27.0, 43.0)],
        ]),
        'G' => (100.0, vec![vec![
            (0.0, 16.0), (16.0, 0.0), (100.0, 0.0), (100.0, 24.0), (30.0, 24.0), (30.0, 76.0),
            (72.0, 76.0), (72.0, 62.0), (54.0, 62.0), (54.0, 44.0), (100.0, 44.0), (100.0, 84.0),
            (84.0, 100.0), (16.0, 100.0), (0.0, 84.0),
        ]]),
        _ => (0.0, vec![]),
    }
}

/// Total width of the mark at a given letter height, and the badge geometry.
fn layout(height: f32) -> (f32, f32) {
    let s = height / 100.0;
    let word: f32 = "YARG".chars().map(|c| letter(c).0 + GAP).sum::<f32>() * s;
    let badge = 118.0 * s;
    (word + badge + 8.0 * s + SKEW * height, word)
}

pub fn draw_logo(cv: &mut Canvas, fonts: &mut Fonts, cx: f32, cy: f32, height: f32) {
    let s = height / 100.0;
    let (_, word_w) = layout(height);
    let shear = |x: f32, y: f32| (x * s + (100.0 - y) * SKEW * s, y * s);

    // Place everything at the origin first so the mark can be centred on its real bounds.
    let mut letters: Vec<Vec<Poly>> = Vec::new();
    let mut pen = 0.0;
    for c in "YARG".chars() {
        let (w, polys) = letter(c);
        letters.push(polys.iter().map(|p| p.iter().map(|&(x, y)| shear(x + pen, y)).collect()).collect());
        pen += w + GAP;
    }
    let bx = word_w / s + 4.0;
    let (bw, top) = (118.0, 6.0);
    let badge: Poly = [(0.0, top), (bw, top), (bw - 22.0, 100.0 - top), (-22.0, 100.0 - top)]
        .iter()
        .map(|&(x, y)| shear(x + bx + 22.0, y))
        .collect();
    let all = letters.iter().flatten().flatten().chain(badge.iter());
    let (minx, maxx) = all.clone().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
    let (dx, dy) = (cx - (minx + maxx) / 2.0, cy - height / 2.0);
    let mv = |p: &Poly, ox: f32, oy: f32| -> Poly { p.iter().map(|&(x, y)| (x + dx + ox, y + dy + oy)).collect() };

    // Soft drop shadow, then the letters with a light-to-steel gradient.
    let shadow = (5.0 * s, 7.0 * s);
    for polys in &letters {
        let placed: Vec<Poly> = polys.iter().map(|p| mv(p, shadow.0, shadow.1)).collect();
        cv.polygons(&placed, &|_, _| rgb(0, 0, 0), 0.40);
    }
    for polys in &letters {
        let placed: Vec<Poly> = polys.iter().map(|p| mv(p, 0.0, 0.0)).collect();
        cv.polygons(&placed, &|_, v| crate::gfx::lerp_rgb(rgb(246, 249, 253), rgb(150, 164, 184), v), 1.0);
    }

    // "OS" badge: a sheared gold parallelogram with dark lettering.
    cv.polygons(&[mv(&badge, shadow.0, shadow.1)], &|_, _| rgb(0, 0, 0), 0.40);
    cv.polygons(&[mv(&badge, 0.0, 0.0)], &|_, v| crate::gfx::lerp_rgb(rgb(250, 208, 70), GOLD, v), 1.0);
    let px = 62.0 * s;
    let tw = fonts.width("OS", px, true);
    let (tx, ty) = shear(bx + 22.0 + bw / 2.0 - 11.0, 50.0);
    fonts.draw(cv, "OS", tx + dx - tw / 2.0, ty + dy - px * 0.62, px, true, INK, 1.0);
}

/// The boot/launch splash: backdrop, the mark, and a line of status text.
pub fn draw_splash(cv: &mut Canvas, fonts: &mut Fonts, status: &str, progress: Option<f32>) {
    cv.backdrop();
    let (w, h) = (cv.w as f32, cv.h as f32);
    draw_logo(cv, fonts, w / 2.0, h * 0.46, (h * 0.17).max(48.0));
    let px = (h * 0.032).max(14.0);
    fonts.draw_centered(cv, status, w / 2.0, h * 0.70, px, false, rgb(190, 200, 214), 0.85);
    if let Some(p) = progress {
        let (bw, bh) = ((w * 0.26) as i32, (h * 0.006).max(4.0) as i32);
        let (bx, by) = ((w / 2.0) as i32 - bw / 2, (h * 0.77) as i32);
        cv.rrect(bx, by, bw, bh, bh as f32 / 2.0, rgb(255, 255, 255), 0.14);
        cv.rrect(bx, by, ((bw as f32) * p.clamp(0.0, 1.0)).max(bh as f32) as i32, bh, bh as f32 / 2.0, GOLD, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_scales_with_height() {
        let (w1, _) = layout(100.0);
        let (w2, _) = layout(200.0);
        assert!((w2 / w1 - 2.0).abs() < 0.05);
    }

    #[test]
    fn letters_are_closed_polygons_with_at_least_three_points() {
        for c in "YARG".chars() {
            let (w, polys) = letter(c);
            assert!(w > 0.0);
            assert!(polys.iter().all(|p| p.len() >= 3));
        }
    }
}

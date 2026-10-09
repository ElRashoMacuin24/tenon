//! Drawing sheets to an image (PNG output without a GPU): lines with their widths and dashes,
//! filled arrowheads, and text in the single-stroke drafting font, on white paper.

use tenon_drawing::graphics::dashed;
use tenon_drawing::{Graphics, stroke};
use tenon_geom::Vec2;

use crate::raster::{Image, MAX_SIDE};

struct Canvas {
    w: u32,
    h: u32,
    /// Coverage of black per pixel, 0..1.
    ink: Vec<f32>,
    px: f64,
    height_mm: f64,
}

impl Canvas {
    fn to_px(&self, p: Vec2) -> (f64, f64) {
        (p.x * self.px, (self.height_mm - p.y) * self.px)
    }
    fn plot(&mut self, x: i64, y: i64, a: f32) {
        if x < 0 || y < 0 || x >= i64::from(self.w) || y >= i64::from(self.h) {
            return;
        }
        let i = (y as usize) * (self.w as usize) + x as usize;
        if let Some(v) = self.ink.get_mut(i) {
            *v = v.max(a);
        }
    }
    /// A segment `width` pixels wide, anti-aliased by distance.
    fn segment(&mut self, a: Vec2, b: Vec2, width: f64) {
        let ((x0, y0), (x1, y1)) = (self.to_px(a), self.to_px(b));
        let r = (width / 2.0).max(0.5);
        let (lo_x, hi_x) = ((x0.min(x1) - r - 1.0).floor() as i64, (x0.max(x1) + r + 1.0).ceil() as i64);
        let (lo_y, hi_y) = ((y0.min(y1) - r - 1.0).floor() as i64, (y0.max(y1) + r + 1.0).ceil() as i64);
        if (hi_x - lo_x) * (hi_y - lo_y) > 4_000_000 {
            return;
        }
        let (dx, dy) = (x1 - x0, y1 - y0);
        let l2 = dx * dx + dy * dy;
        for y in lo_y..=hi_y {
            for x in lo_x..=hi_x {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let t = if l2 > 0.0 { (((px - x0) * dx + (py - y0) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let d = ((px - x0 - dx * t).powi(2) + (py - y0 - dy * t).powi(2)).sqrt();
                let cover = (r + 0.5 - d).clamp(0.0, 1.0);
                if cover > 0.0 {
                    self.plot(x, y, cover as f32);
                }
            }
        }
    }
    /// A filled convex polygon.
    fn fill(&mut self, pts: &[Vec2]) {
        let p: Vec<(f64, f64)> = pts.iter().map(|q| self.to_px(*q)).collect();
        if p.len() < 3 {
            return;
        }
        let (lo_y, hi_y) =
            (p.iter().map(|q| q.1).fold(f64::MAX, f64::min).floor() as i64, p.iter().map(|q| q.1).fold(f64::MIN, f64::max).ceil() as i64);
        let (lo_x, hi_x) =
            (p.iter().map(|q| q.0).fold(f64::MAX, f64::min).floor() as i64, p.iter().map(|q| q.0).fold(f64::MIN, f64::max).ceil() as i64);
        if (hi_x - lo_x) * (hi_y - lo_y) > 1_000_000 {
            return;
        }
        for y in lo_y..=hi_y {
            for x in lo_x..=hi_x {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let mut sign = 0.0f64;
                let mut inside = true;
                for i in 0..p.len() {
                    let (a, b) = (p[i], p[(i + 1) % p.len()]);
                    let c = (b.0 - a.0) * (py - a.1) - (b.1 - a.1) * (px - a.0);
                    if c != 0.0 {
                        if sign != 0.0 && c.signum() != sign {
                            inside = false;
                            break;
                        }
                        sign = c.signum();
                    }
                }
                if inside {
                    self.plot(x, y, 1.0);
                }
            }
        }
    }
}

/// A sheet as an image, `px_per_mm` pixels per millimetre of paper.
pub fn rasterize(g: &Graphics, px_per_mm: f64) -> Image {
    let px = if px_per_mm.is_finite() && px_per_mm > 0.0 { px_per_mm } else { 4.0 };
    let w = ((g.width * px).ceil() as u32).clamp(1, MAX_SIDE);
    let h = ((g.height * px).ceil() as u32).clamp(1, MAX_SIDE);
    let mut c = Canvas { w, h, ink: vec![0.0; (w as usize) * (h as usize)], px, height_mm: g.height };
    for (_, pen, pts) in &g.strokes {
        let width = (pen.width() * px).max(1.0);
        for run in dashed(pts, pen.dashes()) {
            for s in run.windows(2) {
                c.segment(s[0], s[1], width);
            }
        }
    }
    for (_, pts) in &g.fills {
        c.fill(pts);
    }
    for (_, t) in &g.texts {
        let width = (t.height * 0.09 * px).max(1.0);
        for s in stroke::text_strokes(&t.text, t.left(), t.height) {
            for w2 in s.windows(2) {
                c.segment(w2[0], w2[1], width);
            }
        }
    }
    let mut rgba = Vec::with_capacity(c.ink.len() * 4);
    for v in &c.ink {
        let shade = (255.0 * (1.0 - f64::from(*v))).round().clamp(0.0, 255.0) as u8;
        rgba.extend_from_slice(&[shade, shade, shade, 255]);
    }
    Image { width: w, height: h, rgba }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use tenon_drawing::{Align, Owner, Pen};

    #[test]
    fn lines_dashes_and_text_land_on_white_paper() {
        let mut g = Graphics { width: 100.0, height: 50.0, ..Graphics::default() };
        g.line(Owner::Frame, Pen::Visible, Vec2::new(10.0, 40.0), Vec2::new(90.0, 40.0));
        g.line(Owner::Frame, Pen::Hidden, Vec2::new(10.0, 25.0), Vec2::new(90.0, 25.0));
        g.text(Owner::Frame, Vec2::new(50.0, 5.0), 5.0, "TENON", Align::Center);
        let img = rasterize(&g, 4.0);
        assert_eq!((img.width, img.height), (400, 200));
        let shade = |x: u32, y_mm: f64| img.rgba[(((50.0 - y_mm) * 4.0) as usize * 400 + x as usize) * 4];
        // Paper is white; the visible line is dark all along; the hidden one has gaps.
        assert_eq!(shade(200, 32.0), 255);
        assert!((40..360).all(|x| shade(x, 40.0) < 128));
        let hidden: Vec<bool> = (40..360).map(|x| shade(x, 25.0) < 128).collect();
        assert!(hidden.iter().any(|d| *d) && hidden.iter().any(|d| !*d));
        // The text is ink near the bottom middle.
        assert!((150..250).any(|x| (4..10).any(|y| shade(x, f64::from(y)) < 128)));
    }
}

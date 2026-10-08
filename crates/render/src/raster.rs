//! A small software rasteriser: shaded triangles with a depth buffer plus edge lines. Used for
//! PNG renders without a GPU (CLI, MCP, CI), with the same camera as the interactive viewport.

use tenon_geom::Vec3;
use tenon_kernel::Mesh;

use crate::Camera;

/// RGBA colour, 0..=255.
pub type Rgba = [u8; 4];

/// What to draw and how.
#[derive(Clone, Debug)]
pub struct Style {
    pub background_top: Rgba,
    pub background_bottom: Rgba,
    pub body: Rgba,
    pub edge: Rgba,
    /// Per body and face, an override colour (e.g. a selection).
    pub face_colors: Vec<(usize, u32, Rgba)>,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            background_top: [0x4a, 0x55, 0x63, 255],
            background_bottom: [0x1f, 0x24, 0x2b, 255],
            body: [0xb8, 0xc0, 0xc8, 255],
            edge: [0x15, 0x18, 0x1c, 255],
            face_colors: Vec::new(),
        }
    }
}

/// An RGBA image, rows top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Largest image side rendered (hostile-input cap).
pub const MAX_SIDE: u32 = 8192;

fn v3(p: [f32; 3]) -> Vec3 {
    Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
}

fn lerp(a: Rgba, b: Rgba, t: f64) -> Rgba {
    let m = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2]), m(a[3], b[3])]
}

/// Renders `meshes` seen by `camera` into a `width x height` image.
pub fn render(meshes: &[&Mesh], camera: &Camera, width: u32, height: u32, style: &Style) -> Image {
    let (w, h) = (width.clamp(1, MAX_SIDE), height.clamp(1, MAX_SIDE));
    let (wf, hf) = (f64::from(w), f64::from(h));
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let c = lerp(style.background_top, style.background_bottom, f64::from(y) / hf.max(1.0));
        for _ in 0..w {
            rgba.extend_from_slice(&c);
        }
    }
    let mut depth = vec![f64::INFINITY; (w * h) as usize];
    let light = camera.forward();

    for (bi, m) in meshes.iter().enumerate() {
        for range in &m.faces {
            let base = style.face_colors.iter().find(|(b, f, _)| *b == bi && *f == range.face).map_or(style.body, |c| c.2);
            let tris = m.indices.get(range.first as usize..(range.first as usize).saturating_add(range.count as usize)).unwrap_or(&[]);
            for t in tris.as_chunks::<3>().0 {
                let pts: Option<Vec<Vec3>> = t.iter().map(|i| m.positions.get(*i as usize).map(|p| v3(*p))).collect();
                let Some(pts) = pts else { continue };
                let n = (pts[1] - pts[0]).cross(pts[2] - pts[0]).normalized();
                let shade = 0.35 + 0.65 * n.dot(light).abs();
                let color = [(f64::from(base[0]) * shade) as u8, (f64::from(base[1]) * shade) as u8, (f64::from(base[2]) * shade) as u8, 255];
                let proj: Option<Vec<(f64, f64, f64)>> = pts.iter().map(|p| camera.project(*p, wf, hf)).collect();
                let Some(s) = proj else { continue };
                fill_triangle(&mut rgba, &mut depth, w, h, [s[0], s[1], s[2]], color);
            }
        }
    }
    // Edges on top of their faces (small depth slack), hidden behind other faces.
    for m in meshes {
        for e in &m.edges {
            for seg in e.points.windows(2) {
                let (Some(a), Some(b)) = (camera.project(v3(seg[0]), wf, hf), camera.project(v3(seg[1]), wf, hf)) else { continue };
                draw_line(&mut rgba, &depth, w, h, a, b, style.edge, 1e-3 * camera.distance.max(1.0));
            }
        }
    }
    Image { width: w, height: h, rgba }
}

fn fill_triangle(rgba: &mut [u8], depth: &mut [f64], w: u32, h: u32, s: [(f64, f64, f64); 3], color: Rgba) {
    let (minx, maxx) = (s.iter().map(|p| p.0).fold(f64::INFINITY, f64::min), s.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max));
    let (miny, maxy) = (s.iter().map(|p| p.1).fold(f64::INFINITY, f64::min), s.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max));
    if !(minx.is_finite() && maxx.is_finite() && miny.is_finite() && maxy.is_finite()) {
        return;
    }
    let x0 = minx.floor().max(0.0) as u32;
    let x1 = (maxx.ceil().min(f64::from(w) - 1.0)).max(0.0) as u32;
    let y0 = miny.floor().max(0.0) as u32;
    let y1 = (maxy.ceil().min(f64::from(h) - 1.0)).max(0.0) as u32;
    let area = (s[1].0 - s[0].0) * (s[2].1 - s[0].1) - (s[2].0 - s[0].0) * (s[1].1 - s[0].1);
    if area.abs() < 1e-12 || x0 > x1 || y0 > y1 {
        return;
    }
    for py in y0..=y1 {
        for px in x0..=x1 {
            let (x, y) = (f64::from(px) + 0.5, f64::from(py) + 0.5);
            let w0 = ((s[1].0 - x) * (s[2].1 - y) - (s[2].0 - x) * (s[1].1 - y)) / area;
            let w1 = ((s[2].0 - x) * (s[0].1 - y) - (s[0].0 - x) * (s[2].1 - y)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let z = w0 * s[0].2 + w1 * s[1].2 + w2 * s[2].2;
            let i = (py * w + px) as usize;
            if let Some(d) = depth.get_mut(i)
                && z < *d
            {
                *d = z;
                if let Some(px) = rgba.get_mut(i * 4..i * 4 + 4) {
                    px.copy_from_slice(&color);
                }
            }
        }
    }
}

fn draw_line(rgba: &mut [u8], depth: &[f64], w: u32, h: u32, a: (f64, f64, f64), b: (f64, f64, f64), color: Rgba, slack: f64) {
    let steps = ((b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil() as usize).clamp(1, 20_000);
    for k in 0..=steps {
        let t = k as f64 / steps as f64;
        let (x, y, z) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t);
        if x < 0.0 || y < 0.0 || x >= f64::from(w) || y >= f64::from(h) {
            continue;
        }
        let i = (y as u32 * w + x as u32) as usize;
        if depth.get(i).is_some_and(|d| z <= d + slack)
            && let Some(px) = rgba.get_mut(i * 4..i * 4 + 4)
        {
            px.copy_from_slice(&color);
        }
    }
}

/// PNG bytes of an image.
pub fn encode_png(img: &Image) -> Result<Vec<u8>, String> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::write_buffer_with_format(&mut out, &img.rgba, img.width, img.height, image::ExtendedColorType::Rgba8, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

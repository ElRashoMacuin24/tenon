//! Picking faces (ray against mesh triangles) and edges (screen distance to edge polylines).

use tenon_geom::Vec3;
use tenon_kernel::Mesh;

use crate::Camera;

/// A face under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceHit {
    pub body: usize,
    pub face: u32,
    pub point: Vec3,
    /// Distance along the ray.
    pub t: f64,
}

/// An edge near the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeHit {
    pub body: usize,
    pub edge: u32,
    pub point: Vec3,
    /// Distance from the pointer in pixels.
    pub pixels: f64,
}

fn v3(p: [f32; 3]) -> Vec3 {
    Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
}

/// Moller-Trumbore ray/triangle intersection; distance along the ray.
fn ray_triangle(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f64> {
    let (e1, e2) = (b - a, c - a);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

/// Nearest face hit by the ray `(o, d)` (unit direction) among `meshes`.
pub fn pick_face(meshes: &[&Mesh], o: Vec3, d: Vec3) -> Option<FaceHit> {
    let mut best: Option<FaceHit> = None;
    for (bi, m) in meshes.iter().enumerate() {
        for range in &m.faces {
            let tris = m.indices.get(range.first as usize..(range.first as usize).saturating_add(range.count as usize)).unwrap_or(&[]);
            for t in tris.as_chunks::<3>().0 {
                let (Some(a), Some(b), Some(c)) = (m.positions.get(t[0] as usize), m.positions.get(t[1] as usize), m.positions.get(t[2] as usize))
                else {
                    continue;
                };
                if let Some(dist) = ray_triangle(o, d, v3(*a), v3(*b), v3(*c))
                    && best.is_none_or(|h| dist < h.t)
                {
                    best = Some(FaceHit { body: bi, face: range.face, point: o + d * dist, t: dist });
                }
            }
        }
    }
    best
}

fn seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (cx, cy) = (a.0 + dx * t, a.1 + dy * t);
    (((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt(), t)
}

/// Nearest edge within `radius` pixels of `(x, y)` that is not hidden behind a face.
pub fn pick_edge(meshes: &[&Mesh], camera: &Camera, w: f64, h: f64, x: f64, y: f64, radius: f64) -> Option<EdgeHit> {
    let (o, d) = camera.ray(x, y, w, h);
    let face = pick_face(meshes, o, d);
    let mut best: Option<EdgeHit> = None;
    for (bi, m) in meshes.iter().enumerate() {
        for e in &m.edges {
            for seg in e.points.windows(2) {
                let (pa, pb) = (v3(seg[0]), v3(seg[1]));
                let (Some(a), Some(b)) = (camera.project(pa, w, h), camera.project(pb, w, h)) else { continue };
                let (dist, t) = seg_dist((x, y), (a.0, a.1), (b.0, b.1));
                if dist > radius || best.is_some_and(|b| dist >= b.pixels) {
                    continue;
                }
                let point = pa.lerp(pb, t);
                // Hidden if a face is clearly in front of it along the ray.
                if let Some(f) = face {
                    let along = (point - o).dot(d);
                    let slack = 1e-3 * camera.distance.max(1.0);
                    if f.t + slack < along {
                        continue;
                    }
                }
                best = Some(EdgeHit { body: bi, edge: e.edge, point, pixels: dist });
            }
        }
    }
    best
}

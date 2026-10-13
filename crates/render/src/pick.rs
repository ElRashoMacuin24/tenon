//! Picking faces (ray against mesh triangles) and edges (screen distance to edge polylines).
//!
//! Neither looks at all of every mesh: each mesh keeps a box around itself, a tree of boxes
//! over its triangles and a box around each edge ([`Mesh::index`]), and what the ray or the
//! pointer is clear of is passed over. The answers are those of trying everything.

use std::sync::Arc;

use tenon_geom::Vec3;
use tenon_kernel::Mesh;
use tenon_kernel::index::{Box3, MeshIndex};

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

/// Nearest face hit by the ray `(o, d)` (unit direction) among `meshes`. Of two hit at the
/// same distance, the one in the earlier mesh, and in it the earlier triangle.
pub fn pick_face(meshes: &[&Mesh], o: Vec3, d: Vec3) -> Option<FaceHit> {
    let indices: Vec<Arc<MeshIndex>> = meshes.iter().map(|m| m.index()).collect();
    face_along(meshes, &indices, o, d)
}

fn face_along(meshes: &[&Mesh], indices: &[Arc<MeshIndex>], o: Vec3, d: Vec3) -> Option<FaceHit> {
    let mut best: Option<FaceHit> = None;
    for (bi, (m, index)) in meshes.iter().zip(indices).enumerate() {
        let within = best.map_or(f64::INFINITY, |h| h.t);
        if let Some((triangle, dist)) = index.ray(m, o, d, within)
            && best.is_none_or(|h| dist < h.t)
            && let Some(face) = m.face_of_triangle(triangle)
        {
            best = Some(FaceHit { body: bi, face, point: o + d * dist, t: dist });
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

/// The point of segment `ab` nearest to the line through `o` along `d`.
fn nearest_on_segment(o: Vec3, d: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let u = b - a;
    let w0 = a - o;
    let (uu, ud, dd, uw, dw) = (u.dot(u), u.dot(d), d.dot(d), u.dot(w0), d.dot(w0));
    let den = uu * dd - ud * ud;
    let s = if uu <= 0.0 || den.abs() < 1e-12 * uu * dd { 0.0 } else { ((ud * dw - dd * uw) / den).clamp(0.0, 1.0) };
    a + u * s
}

/// Nearest edge within `radius` pixels of `(x, y)` that is not hidden behind a face.
pub fn pick_edge(meshes: &[&Mesh], camera: &Camera, w: f64, h: f64, x: f64, y: f64, radius: f64) -> Option<EdgeHit> {
    let (o, d) = camera.ray(x, y, w, h);
    let indices: Vec<Arc<MeshIndex>> = meshes.iter().map(|m| m.index()).collect();
    // Whether anything in a box can be within reach of the pointer: not when the pointer is
    // clear of where the corners of the box are on the screen. (A box that is partly behind
    // the eye is not ruled out.)
    let reach = radius + 1.0;
    let near_box = |b: &Box3| {
        if b.is_empty() {
            return false;
        }
        let (mut low, mut high) = ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
        for corner in b.corners() {
            let Some((px, py, _)) = camera.project(corner, w, h) else { return true };
            low = (low.0.min(px), low.1.min(py));
            high = (high.0.max(px), high.1.max(py));
        }
        x >= low.0 - reach && x <= high.0 + reach && y >= low.1 - reach && y <= high.1 + reach
    };
    // Every segment near the pointer (cheap: projection only), nearest first.
    let mut near: Vec<EdgeHit> = Vec::new();
    for (bi, (m, index)) in meshes.iter().zip(&indices).enumerate() {
        if !near_box(&index.bounds()) {
            continue;
        }
        for (e, bounds) in m.edges.iter().zip(index.edge_bounds(m)) {
            if e.points.len() > 4 && !near_box(bounds) {
                continue;
            }
            for seg in e.points.windows(2) {
                let (pa, pb) = (v3(seg[0]), v3(seg[1]));
                let (Some(a), Some(b)) = (camera.project(pa, w, h), camera.project(pb, w, h)) else { continue };
                let (dist, _) = seg_dist((x, y), (a.0, a.1), (b.0, b.1));
                if dist <= radius {
                    // The 3D point under the pointer (a screen fraction is not a 3D fraction in
                    // perspective).
                    near.push(EdgeHit { body: bi, edge: e.edge, point: nearest_on_segment(o, d, pa, pb), pixels: dist });
                }
            }
        }
    }
    near.sort_by(|a, b| a.pixels.total_cmp(&b.pixels));
    // The nearest one not hidden by a face clearly in front of it along its own ray.
    let slack = 1e-3 * camera.distance.max(1.0);
    near.into_iter().find(|hit| match camera.project(hit.point, w, h) {
        Some((px, py, _)) => {
            let (o2, d2) = camera.ray(px, py, w, h);
            !face_along(meshes, &indices, o2, d2).is_some_and(|f| f.t + slack < (hit.point - o2).dot(d2))
        }
        None => true,
    })
}

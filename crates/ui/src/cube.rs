//! The orientation cube (top right of the viewport). Each face is split into a 3 x 3 grid:
//! the middle cell looks at that face, the side strips at the edge shared with the neighbour, the
//! corner cells at the corner, so the cube offers all 26 standard directions. Dragging the cube
//! orbits; when the view is square to a face, arrows turn it by quarter turns.

use egui::{Mesh, Pos2, Rect, Sense, Shape, Stroke, Ui, vec2};
use tenon_geom::Vec3;
use tenon_render::Camera;

use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::workbench::Workbench;

/// A view direction from the cube: each component -1, 0 or 1 (eye side).
pub(crate) type Dir = [i8; 3];

struct Face {
    n: Dir,
    right: Dir,
    up: Dir,
    label: &'static str,
}

/// The faces with the directions their labels read in (`right = up x n`).
const FACES: [Face; 6] = [
    Face { n: [0, -1, 0], right: [1, 0, 0], up: [0, 0, 1], label: "FRONT" },
    Face { n: [0, 1, 0], right: [-1, 0, 0], up: [0, 0, 1], label: "BACK" },
    Face { n: [1, 0, 0], right: [0, 1, 0], up: [0, 0, 1], label: "RIGHT" },
    Face { n: [-1, 0, 0], right: [0, -1, 0], up: [0, 0, 1], label: "LEFT" },
    Face { n: [0, 0, 1], right: [1, 0, 0], up: [0, 1, 0], label: "TOP" },
    Face { n: [0, 0, -1], right: [1, 0, 0], up: [0, -1, 0], label: "BOTTOM" },
];

/// Face coordinates beyond this (of 1) belong to an edge or corner zone.
const ZONE: f64 = 0.64;
/// Half the cube's edge in screen points.
pub(crate) const CUBE_SCALE: f32 = 28.0;

pub(crate) fn vec(d: Dir) -> Vec3 {
    Vec3::new(f64::from(d[0]), f64::from(d[1]), f64::from(d[2]))
}

fn add(a: Dir, b: Dir, k: i8) -> Dir {
    [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k]
}

fn zone(t: f64) -> i8 {
    if t > ZONE {
        1
    } else if t < -ZONE {
        -1
    } else {
        0
    }
}

/// The cube as seen by a camera, drawn around `center`.
pub(crate) struct CubeView {
    center: Pos2,
    scale: f32,
    right: Vec3,
    up: Vec3,
    eye: Vec3,
    light: Vec3,
}

impl CubeView {
    pub(crate) fn new(cam: &Camera, center: Pos2, scale: f32) -> CubeView {
        CubeView { center, scale, right: cam.right(), up: cam.up(), eye: cam.eye_dir(), light: cam.key_light() }
    }

    fn to_screen(&self, p: Vec3) -> Pos2 {
        self.center + vec2((p.dot(self.right) as f32) * self.scale, -(p.dot(self.up) as f32) * self.scale)
    }

    fn visible(&self) -> impl Iterator<Item = &'static Face> + '_ {
        FACES.iter().filter(|f| vec(f.n).dot(self.eye) > 1e-3)
    }

    /// Face coordinates `(u, v)` of a screen point on a face's plane.
    fn face_coords(&self, f: &Face, p: Pos2) -> Option<(f64, f64)> {
        let c = self.to_screen(vec(f.n));
        let a = self.to_screen(vec(f.n) + vec(f.right)) - c;
        let b = self.to_screen(vec(f.n) + vec(f.up)) - c;
        let q = p - c;
        let det = f64::from(a.x * b.y - a.y * b.x);
        if det.abs() < 1e-6 {
            return None;
        }
        let u = f64::from(q.x * b.y - q.y * b.x) / det;
        let v = f64::from(a.x * q.y - a.y * q.x) / det;
        Some((u, v))
    }

    /// The direction under a screen point, if the point is on the cube.
    pub(crate) fn hit(&self, p: Pos2) -> Option<Dir> {
        self.visible().find_map(|f| {
            let (u, v) = self.face_coords(f, p)?;
            (u.abs() <= 1.0 && v.abs() <= 1.0).then(|| add(add(f.n, f.right, zone(u)), f.up, zone(v)))
        })
    }

    /// The middle of the zone of direction `d` on screen (for tests).
    #[cfg(test)]
    pub(crate) fn point_of(&self, d: Dir) -> Option<Pos2> {
        // The centre of the zone of `d` on the visible face most square to the viewer.
        let f = self.visible().filter(|f| vec(f.n).dot(vec(d)) > 0.5).max_by(|a, b| vec(a.n).dot(self.eye).total_cmp(&vec(b.n).dot(self.eye)))?;
        let (r, u) = (vec(f.right).dot(vec(d)), vec(f.up).dot(vec(d)));
        let mid = (1.0 + ZONE) / 2.0;
        Some(self.to_screen(vec(f.n) + vec(f.right) * (r * mid) + vec(f.up) * (u * mid)))
    }

    /// The 9 cells of a face: direction and corners (in model units).
    fn cells(f: &Face) -> impl Iterator<Item = (Dir, [Vec3; 4])> {
        let bands = [(-1.0, -ZONE, -1i8), (-ZONE, ZONE, 0), (ZONE, 1.0, 1)];
        let (n, r, u) = (vec(f.n), vec(f.right), vec(f.up));
        let (fn_, fr, fu) = (f.n, f.right, f.up);
        bands.into_iter().flat_map(move |(u0, u1, cu)| {
            bands.into_iter().map(move |(v0, v1, cv)| {
                let at = |a: f64, b: f64| n + r * a + u * b;
                (add(add(fn_, fr, cu), fu, cv), [at(u0, v0), at(u1, v0), at(u1, v1), at(u0, v1)])
            })
        })
    }

    /// Square to a face: that face's direction.
    pub(crate) fn face_on(&self) -> Option<Dir> {
        FACES.iter().find(|f| vec(f.n).dot(self.eye) > 0.9995).map(|f| f.n)
    }

    fn paint(&self, ui: &Ui, hover: Option<Dir>, t: &Tokens) {
        // Galley meshes carry texture coordinates in font-atlas pixels; the painter normalises
        // them only for text shapes, so do it here.
        let atlas = ui.ctx().fonts(|f| f.font_image_size());
        let norm = vec2(1.0 / atlas[0].max(1) as f32, 1.0 / atlas[1].max(1) as f32);
        let p = ui.painter();
        for f in self.visible() {
            let facing = vec(f.n).dot(self.eye);
            let corners: Vec<Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .iter()
                .map(|(a, b)| self.to_screen(vec(f.n) + vec(f.right) * *a + vec(f.up) * *b))
                .collect();
            // Lit like the model, so the cube reads as a solid.
            let shade = (0.78 + 0.22 * vec(f.n).dot(self.light).abs()) as f32;
            p.add(Shape::convex_polygon(corners.clone(), t.cube_face.gamma_multiply(shade), Stroke::NONE));
            if let Some(h) = hover {
                for (d, quad) in Self::cells(f) {
                    if d == h {
                        let pts: Vec<Pos2> = quad.iter().map(|q| self.to_screen(*q)).collect();
                        p.add(Shape::convex_polygon(pts, t.cube_face_hover, Stroke::NONE));
                    }
                }
            }
            p.add(Shape::closed_line(corners, Stroke::new(1.0, t.cube_edge)));
            // The label is painted onto the face, so it foreshortens with it.
            let alpha = ((facing - 0.12) / 0.3).clamp(0.0, 1.0) as f32;
            if alpha > 0.0 {
                let color = t.cube_text.gamma_multiply(alpha);
                let galley = p.layout_no_wrap(f.label.to_owned(), egui::FontId::proportional(13.0), color);
                let size = galley.size();
                let s = (1.45 / f64::from(size.x.max(1.0))).min(0.62 / f64::from(size.y.max(1.0)));
                let mut mesh = Mesh::default();
                for row in &galley.rows {
                    let mut m = row.row.visuals.mesh.clone();
                    for v in &mut m.vertices {
                        let (x, y) = (f64::from(v.pos.x + row.pos.x - size.x / 2.0), f64::from(v.pos.y + row.pos.y - size.y / 2.0));
                        v.pos = self.to_screen(vec(f.n) + vec(f.right) * (x * s) - vec(f.up) * (y * s));
                        v.uv = (v.uv.to_vec2() * norm).to_pos2();
                        v.color = color;
                    }
                    mesh.append(m);
                }
                p.add(Shape::mesh(mesh));
            }
        }
    }
}

/// Hot spots around the cube when it is square to a face: a quarter turn towards each
/// neighbour, and a quarter roll either way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Arrow {
    Turn(Dir),
    Roll(f64),
}

fn arrows(cv: &CubeView, cam: &Camera) -> Vec<(Rect, Arrow)> {
    if cv.face_on().is_none() {
        return Vec::new();
    }
    let reach = cv.scale + 16.0;
    let snap = |v: Vec3| -> Dir {
        let r = |x: f64| x.round().clamp(-1.0, 1.0) as i8;
        [r(v.x), r(v.y), r(v.z)]
    };
    let mut out = vec![
        (Rect::from_center_size(cv.center + vec2(reach, 0.0), vec2(14.0, 14.0)), Arrow::Turn(snap(cam.right()))),
        (Rect::from_center_size(cv.center - vec2(reach, 0.0), vec2(14.0, 14.0)), Arrow::Turn(snap(-cam.right()))),
        (Rect::from_center_size(cv.center - vec2(0.0, reach), vec2(14.0, 14.0)), Arrow::Turn(snap(cam.up()))),
        (Rect::from_center_size(cv.center + vec2(0.0, reach), vec2(14.0, 14.0)), Arrow::Turn(snap(-cam.up()))),
    ];
    let corner = cv.center + vec2(reach + 4.0, -reach - 4.0);
    out.push((Rect::from_center_size(corner + vec2(-9.0, 0.0), vec2(14.0, 14.0)), Arrow::Roll(std::f64::consts::FRAC_PI_2)));
    out.push((Rect::from_center_size(corner + vec2(9.0, 0.0), vec2(14.0, 14.0)), Arrow::Roll(-std::f64::consts::FRAC_PI_2)));
    out
}

fn paint_arrow(ui: &Ui, r: Rect, a: Arrow, center: Pos2, hot: bool, t: &Tokens) {
    let color = if hot { t.accent } else { t.cube_edge };
    let p = ui.painter();
    match a {
        Arrow::Turn(_) => {
            // A triangle pointing away from the cube.
            let d = (r.center() - center).normalized();
            let n = vec2(-d.y, d.x);
            let tip = r.center() + d * 6.0;
            p.add(Shape::convex_polygon(vec![tip, r.center() - d * 4.0 + n * 6.0, r.center() - d * 4.0 - n * 6.0], color, Stroke::NONE));
        }
        Arrow::Roll(angle) => {
            // A quarter-circle arrow, counter-clockwise for a positive roll.
            let (c, rad) = (r.center(), 5.5f32);
            let dir = if angle > 0.0 { -1.0f32 } else { 1.0 };
            let pts: Vec<Pos2> =
                (0..=10).map(|i| c + vec2((0.3 + dir * i as f32 * 0.16).cos() * rad, -(0.3 + dir * i as f32 * 0.16).sin() * rad)).collect();
            let end = pts[pts.len() - 1];
            p.add(Shape::line(pts, Stroke::new(1.6, color)));
            p.circle_filled(end, 2.2, color);
        }
    }
}

impl Workbench {
    pub(crate) fn cube(&mut self, ui: &Ui, center: Pos2, t: &Tokens) {
        let cam = self.view.camera;
        let cv = CubeView::new(&cam, center, CUBE_SCALE);
        let area = Rect::from_center_size(center, vec2(CUBE_SCALE * 3.7, CUBE_SCALE * 3.7));
        let resp = ui.interact(area, ui.id().with("cube"), Sense::click_and_drag());
        let pointer = resp.hover_pos();
        let hover = pointer.and_then(|p| cv.hit(p));
        cv.paint(ui, if resp.dragged() { None } else { hover }, t);

        // Home, top left of the cube.
        let home = Rect::from_center_size(center + vec2(-CUBE_SCALE * 1.75, -CUBE_SCALE * 1.75), vec2(16.0, 16.0));
        let hresp = ui.interact(home, ui.id().with("cube-home"), Sense::click());
        icons::paint(ui.painter(), home, Icon::Home, if hresp.hovered() { t.accent } else { t.cube_edge });
        let mut home_clicked = hresp.on_hover_text("Home view").clicked();

        let mut chosen: Option<Arrow> = None;
        for (i, (r, a)) in arrows(&cv, &cam).into_iter().enumerate() {
            let ar = ui.interact(r.expand(2.0), ui.id().with(("cube-arrow", i)), Sense::click());
            paint_arrow(ui, r, a, center, ar.hovered(), t);
            if ar.clicked() {
                chosen = Some(a);
            }
        }

        if resp.dragged() {
            let d = resp.drag_delta();
            self.view.anim = None;
            self.view.camera.orbit(f64::from(d.x), f64::from(d.y));
        } else if resp.clicked()
            && let Some(d) = hover
        {
            self.look_from(vec(d));
        }
        match chosen {
            Some(Arrow::Turn(d)) => self.look_from(vec(d)),
            Some(Arrow::Roll(a)) => {
                let mut to = self.view.camera;
                to.roll += a;
                self.animate_to(to);
            }
            None => {}
        }
        resp.context_menu(|ui| {
            if ui.button("Home View").clicked() {
                home_clicked = true;
                ui.close();
            }
            if ui.button("Set Current View as Home").clicked() {
                self.view.home = (self.view.camera.yaw, self.view.camera.pitch, self.view.camera.roll);
                self.set_status("The current view is now the home view.");
                ui.close();
            }
            ui.separator();
            let ortho = self.view.camera.projection == tenon_render::Projection::Orthographic;
            if ui.radio(!ortho, "Perspective").clicked() {
                self.view.camera.projection = tenon_render::Projection::Perspective;
                ui.close();
            }
            if ui.radio(ortho, "Orthographic").clicked() {
                self.view.camera.projection = tenon_render::Projection::Orthographic;
                ui.close();
            }
            ui.separator();
            if ui.button("Fit to View").clicked() {
                self.zoom_all();
                ui.close();
            }
        });
        if home_clicked {
            self.home_view();
        }
        if let Some(d) = hover
            && !resp.dragged()
        {
            let what = match d.iter().filter(|c| **c != 0).count() {
                1 => "face",
                2 => "edge",
                _ => "corner",
            };
            let _ = resp.on_hover_text(format!("Look at this {what}; drag to orbit; right-click for options"));
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use std::collections::BTreeSet;

    use egui::pos2;
    use tenon_render::StdView;

    use super::*;

    #[test]
    fn every_one_of_the_26_directions_can_be_picked() {
        // From eight corner views the visible faces together show every direction.
        let mut seen = BTreeSet::new();
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                for sz in [-1.0, 1.0] {
                    let mut cam = Camera::default();
                    cam.look_along(Vec3::new(sx, sy, sz));
                    let cv = CubeView::new(&cam, pos2(100.0, 100.0), 30.0);
                    for f in cv.visible() {
                        for (d, quad) in CubeView::cells(f) {
                            let mid = quad.iter().fold(Vec3::ZERO, |a, q| a + *q) * 0.25;
                            assert_eq!(cv.hit(cv.to_screen(mid)), Some(d), "cell centre picks its own direction");
                            seen.insert(d);
                        }
                    }
                }
            }
        }
        assert_eq!(seen.len(), 26);
        assert!(!seen.contains(&[0, 0, 0]));
    }

    #[test]
    fn faces_edges_and_corners_from_the_home_view() {
        let mut cam = Camera::default();
        cam.set_view(StdView::Home);
        let cv = CubeView::new(&cam, pos2(0.0, 0.0), 30.0);
        // Home looks from front-right-top: those three faces are visible.
        let visible: Vec<Dir> = cv.visible().map(|f| f.n).collect();
        assert_eq!(visible.len(), 3);
        for d in [[0, -1, 0], [1, 0, 0], [0, 0, 1], [1, -1, 0], [0, -1, 1], [1, 0, 1], [1, -1, 1]] {
            let p = cv.point_of(d).unwrap();
            assert_eq!(cv.hit(p), Some(d), "{d:?}");
        }
        assert_eq!(cv.hit(pos2(500.0, 500.0)), None);
        assert_eq!(cv.face_on(), None);
        cam.look_along(Vec3::Z);
        assert_eq!(CubeView::new(&cam, pos2(0.0, 0.0), 30.0).face_on(), Some([0, 0, 1]));
    }

    #[test]
    fn quarter_turn_arrows_point_at_the_neighbours() {
        let mut cam = Camera::default();
        cam.set_view(StdView::Front);
        let cv = CubeView::new(&cam, pos2(0.0, 0.0), 30.0);
        let turns: Vec<Dir> = arrows(&cv, &cam)
            .iter()
            .filter_map(|(_, a)| match a {
                Arrow::Turn(d) => Some(*d),
                Arrow::Roll(_) => None,
            })
            .collect();
        assert_eq!(turns, [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]], "right, left, top, bottom of the front view");
    }
}

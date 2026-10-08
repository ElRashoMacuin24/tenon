//! Orbit camera (Z up), projections, standard views, picking rays and screen projection.
//!
//! Screen coordinates are pixels from the top-left corner; clip space follows wgpu (depth 0..1).

use std::f64::consts::{FRAC_PI_2, PI};

use tenon_geom::{Aabb3, Vec3};

/// Column-major 4x4 matrix, as WGSL expects.
pub type Mat4 = [[f32; 4]; 4];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    Perspective,
    Orthographic,
}

/// The six face-on views and the default three-quarter view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StdView {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
    /// Front-right-top three-quarter view.
    Home,
}

impl StdView {
    pub fn from_name(name: &str) -> Option<StdView> {
        Some(match name.to_ascii_lowercase().as_str() {
            "front" => StdView::Front,
            "back" => StdView::Back,
            "left" => StdView::Left,
            "right" => StdView::Right,
            "top" => StdView::Top,
            "bottom" => StdView::Bottom,
            "home" | "iso" => StdView::Home,
            _ => return None,
        })
    }
    /// `(yaw, pitch)` of the eye direction.
    fn angles(self) -> (f64, f64) {
        match self {
            StdView::Front => (-FRAC_PI_2, 0.0),
            StdView::Back => (FRAC_PI_2, 0.0),
            StdView::Right => (0.0, 0.0),
            StdView::Left => (PI, 0.0),
            StdView::Top => (-FRAC_PI_2, FRAC_PI_2),
            StdView::Bottom => (-FRAC_PI_2, -FRAC_PI_2),
            StdView::Home => (-std::f64::consts::FRAC_PI_4, (1.0f64 / 3.0f64.sqrt()).asin()),
        }
    }
}

/// An orbit camera around `target`. `yaw` turns about world Z, `pitch` lifts above the XY plane,
/// `roll` turns the image about the view axis (counter-clockwise).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub target: Vec3,
    pub distance: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
    /// Vertical field of view (radians) for perspective; also sets the orthographic scale.
    pub fov_y: f64,
    pub projection: Projection,
}

impl Default for Camera {
    fn default() -> Self {
        let (yaw, pitch) = StdView::Home.angles();
        Camera { target: Vec3::ZERO, distance: 200.0, yaw, pitch, roll: 0.0, fov_y: 0.6, projection: Projection::Perspective }
    }
}

fn to_mat(m: [[f64; 4]; 4]) -> Mat4 {
    m.map(|c| c.map(|v| v as f32))
}

/// `a * b` for column-major matrices.
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0f32; 4]; 4];
    for (c, col) in out.iter_mut().enumerate() {
        for (r, cell) in col.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    out
}

impl Camera {
    /// Unit vector from the target towards the eye.
    pub fn eye_dir(&self) -> Vec3 {
        let (cp, sp) = (self.pitch.cos(), self.pitch.sin());
        Vec3::new(cp * self.yaw.cos(), cp * self.yaw.sin(), sp)
    }
    pub fn eye(&self) -> Vec3 {
        self.target + self.eye_dir() * self.distance
    }
    pub fn forward(&self) -> Vec3 {
        -self.eye_dir()
    }
    /// Screen right and up before roll; right is defined from the yaw so it stays valid looking
    /// straight down.
    fn unrolled(&self) -> (Vec3, Vec3) {
        let r = Vec3::new(-self.yaw.sin(), self.yaw.cos(), 0.0);
        (r, r.cross(self.forward()).normalized())
    }
    pub fn right(&self) -> Vec3 {
        let (r, u) = self.unrolled();
        r * self.roll.cos() - u * self.roll.sin()
    }
    pub fn up(&self) -> Vec3 {
        let (r, u) = self.unrolled();
        r * self.roll.sin() + u * self.roll.cos()
    }
    /// Direction the key light travels: from above and a little to the left of the eye, so
    /// faces at different angles shade differently even in the isometric view (a headlight
    /// along `forward` lights all three faces of an iso cube equally).
    pub fn key_light(&self) -> Vec3 {
        (self.forward() - self.up() * 0.5 + self.right() * 0.15).normalized()
    }

    /// Half the visible height at the target's depth.
    fn half_height(&self) -> f64 {
        self.distance * (self.fov_y / 2.0).tan()
    }

    /// World -> view (right-handed, camera looking down -Z).
    pub fn view(&self) -> [[f64; 4]; 4] {
        let (r, u, f, e) = (self.right(), self.up(), self.forward(), self.eye());
        [[r.x, u.x, -f.x, 0.0], [r.y, u.y, -f.y, 0.0], [r.z, u.z, -f.z, 0.0], [-r.dot(e), -u.dot(e), f.dot(e), 1.0]]
    }

    /// Near and far planes covering a scene of radius `radius` around the target.
    fn depth_range(&self, radius: f64) -> (f64, f64) {
        let r = radius.max(1e-3);
        let near = (self.distance - 2.0 * r).max(self.distance * 1e-3).max(1e-3);
        (near, self.distance + 2.0 * r)
    }

    /// View -> clip (wgpu depth 0..1).
    pub fn projection_matrix(&self, aspect: f64, radius: f64) -> [[f64; 4]; 4] {
        let aspect = if aspect.is_finite() && aspect > 1e-6 { aspect } else { 1.0 };
        let (n, f) = self.depth_range(radius);
        match self.projection {
            Projection::Perspective => {
                let t = 1.0 / (self.fov_y / 2.0).tan();
                [[t / aspect, 0.0, 0.0, 0.0], [0.0, t, 0.0, 0.0], [0.0, 0.0, f / (n - f), -1.0], [0.0, 0.0, n * f / (n - f), 0.0]]
            }
            Projection::Orthographic => {
                let h = self.half_height();
                let w = h * aspect;
                [[1.0 / w, 0.0, 0.0, 0.0], [0.0, 1.0 / h, 0.0, 0.0], [0.0, 0.0, 1.0 / (n - f), 0.0], [0.0, 0.0, n / (n - f), 1.0]]
            }
        }
    }

    /// World -> clip as f32 (for the GPU).
    pub fn view_proj(&self, aspect: f64, radius: f64) -> Mat4 {
        mul(&to_mat(self.projection_matrix(aspect, radius)), &to_mat(self.view()))
    }

    /// Orbits by a pointer movement in pixels (screen axes, so a rolled view orbits the way the
    /// pointer moves).
    pub fn orbit(&mut self, dx: f64, dy: f64) {
        let (s, c) = self.roll.sin_cos();
        let (dx, dy) = (dx * c - dy * s, dx * s + dy * c);
        self.yaw -= dx * 0.008;
        self.pitch = (self.pitch + dy * 0.008).clamp(-FRAC_PI_2, FRAC_PI_2);
    }

    /// Looks along `-eye_dir` (from the `eye_dir` side) without roll. Straight down or up, the
    /// front (-Y) is at the bottom of the screen.
    pub fn look_along(&mut self, eye_dir: Vec3) {
        let d = eye_dir.normalized();
        if !d.is_finite() || d.len() < 0.5 {
            return;
        }
        self.pitch = d.z.clamp(-1.0, 1.0).asin();
        self.yaw = if d.x.abs() < 1e-9 && d.y.abs() < 1e-9 { -FRAC_PI_2 } else { d.y.atan2(d.x) };
        self.roll = 0.0;
    }

    /// The camera a fraction `t` (0..=1) of the way from `a` to `b`: angles along the shorter
    /// way round, distance geometrically (zoom feels even), target linearly.
    pub fn lerp(a: &Camera, b: &Camera, t: f64) -> Camera {
        let t = t.clamp(0.0, 1.0);
        if t >= 1.0 {
            return *b;
        }
        let turn = |x: f64, y: f64| {
            let d = (y - x + PI).rem_euclid(2.0 * PI) - PI;
            x + d * t
        };
        let distance =
            if a.distance > 0.0 && b.distance > 0.0 { (a.distance.ln() + (b.distance.ln() - a.distance.ln()) * t).exp() } else { b.distance };
        Camera {
            target: a.target + (b.target - a.target) * t,
            distance,
            yaw: turn(a.yaw, b.yaw),
            pitch: a.pitch + (b.pitch - a.pitch) * t,
            roll: turn(a.roll, b.roll),
            fov_y: a.fov_y + (b.fov_y - a.fov_y) * t,
            projection: a.projection,
        }
    }

    /// Pans by a pointer movement in pixels in a viewport `height` pixels tall.
    pub fn pan(&mut self, dx: f64, dy: f64, height: f64) {
        let per_px = 2.0 * self.half_height() / height.max(1.0);
        self.target = self.target - self.right() * (dx * per_px) + self.up() * (dy * per_px);
    }

    /// Zooms by `factor` (< 1 closer), towards the world point under the pointer when given.
    pub fn zoom(&mut self, factor: f64, towards: Option<Vec3>) {
        let factor = if factor.is_finite() { factor.clamp(0.2, 5.0) } else { 1.0 };
        let new = (self.distance * factor).clamp(1e-3, 1e7);
        if let Some(p) = towards.filter(|p| p.is_finite()) {
            // Keep `p` under the pointer: move the target along the line to it.
            self.target = p + (self.target - p) * (new / self.distance);
        }
        self.distance = new;
    }

    pub fn set_view(&mut self, v: StdView) {
        (self.yaw, self.pitch) = v.angles();
        self.roll = 0.0;
    }

    /// Frames the box (keeps the view direction).
    pub fn fit(&mut self, b: &Aabb3) {
        let r = (b.diagonal() / 2.0).max(1.0);
        self.target = b.center();
        self.distance = r / (self.fov_y / 2.0).sin() * 1.15;
    }

    /// Ray through pixel `(x, y)` of a `w x h` viewport: origin and unit direction.
    pub fn ray(&self, x: f64, y: f64, w: f64, h: f64) -> (Vec3, Vec3) {
        let (w, h) = (w.max(1.0), h.max(1.0));
        let nx = (2.0 * x / w - 1.0) * (w / h);
        let ny = 1.0 - 2.0 * y / h;
        let hh = self.half_height() / self.distance; // tan(fov/2)
        match self.projection {
            Projection::Perspective => {
                let d = (self.forward() + self.right() * (nx * hh) + self.up() * (ny * hh)).normalized();
                (self.eye(), d)
            }
            Projection::Orthographic => {
                let o = self.eye() + self.right() * (nx * self.half_height()) + self.up() * (ny * self.half_height());
                (o, self.forward())
            }
        }
    }

    /// Pixel position and view depth of a world point, if it is in front of the camera.
    pub fn project(&self, p: Vec3, w: f64, h: f64) -> Option<(f64, f64, f64)> {
        let rel = p - self.eye();
        let depth = rel.dot(self.forward());
        let (x, y) = (rel.dot(self.right()), rel.dot(self.up()));
        let (sx, sy) = match self.projection {
            Projection::Perspective => {
                if depth <= 1e-9 {
                    return None;
                }
                let hh = (self.fov_y / 2.0).tan() * depth;
                (x / hh, y / hh)
            }
            Projection::Orthographic => (x / self.half_height(), y / self.half_height()),
        };
        let aspect = w.max(1.0) / h.max(1.0);
        Some(((sx / aspect + 1.0) * w / 2.0, (1.0 - sy) * h / 2.0, depth))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(m: &Mat4, p: Vec3) -> [f32; 4] {
        let v = [p.x as f32, p.y as f32, p.z as f32, 1.0];
        let mut out = [0.0f32; 4];
        for (r, o) in out.iter_mut().enumerate() {
            *o = (0..4).map(|c| m[c][r] * v[c]).sum();
        }
        out
    }

    #[test]
    fn front_view_shows_x_right_and_z_up() {
        let mut c = Camera { target: Vec3::ZERO, distance: 100.0, ..Default::default() };
        c.set_view(StdView::Front);
        assert!(c.right().near(Vec3::X, 1e-12) && c.up().near(Vec3::Z, 1e-12));
        let (x, y, _) = c.project(Vec3::new(10.0, 0.0, 0.0), 800.0, 600.0).unwrap();
        assert!(x > 400.0 && (y - 300.0).abs() < 1e-6);
        let (_, y, _) = c.project(Vec3::new(0.0, 0.0, 10.0), 800.0, 600.0).unwrap();
        assert!(y < 300.0);
        c.set_view(StdView::Top);
        assert!(c.up().near(Vec3::Y, 1e-12) && c.forward().near(-Vec3::Z, 1e-12));
    }

    #[test]
    fn roll_turns_the_image_and_orbit_follows_the_screen() {
        let mut c = Camera { target: Vec3::ZERO, distance: 100.0, ..Default::default() };
        c.set_view(StdView::Front);
        c.roll = FRAC_PI_2;
        // Rolled a quarter turn counter-clockwise: world up points to screen left.
        assert!(c.right().near(-Vec3::Z, 1e-12) && c.up().near(Vec3::X, 1e-12), "{:?} {:?}", c.right(), c.up());
        let (x, _, _) = c.project(Vec3::new(0.0, 0.0, 10.0), 800.0, 600.0).unwrap();
        assert!(x < 400.0);
        // Dragging right on screen orbits about the axis that is vertical on screen (world X).
        let before = c;
        c.orbit(50.0, 0.0);
        assert!((c.yaw - before.yaw).abs() < 1e-12 && (c.pitch - before.pitch).abs() > 0.1);
        // Every view axis is orthonormal.
        for roll in [0.0, 0.3, -2.0] {
            let c = Camera { yaw: 0.7, pitch: 0.4, roll, ..Default::default() };
            let (r, u, f) = (c.right(), c.up(), c.forward());
            assert!(r.dot(u).abs() < 1e-12 && r.dot(f).abs() < 1e-12 && u.dot(f).abs() < 1e-12);
            assert!(r.cross(u).near(-f, 1e-12), "right-handed: right x up points at the viewer");
        }
    }

    #[test]
    fn look_along_and_lerp() {
        let mut c = Camera::default();
        for d in [Vec3::X, -Vec3::Y, Vec3::new(1.0, -1.0, 1.0), Vec3::new(-1.0, 0.0, -1.0)] {
            c.look_along(d);
            assert!(c.eye_dir().near(d.normalized(), 1e-12), "{d:?}");
        }
        c.look_along(Vec3::Z);
        assert!(c.up().near(Vec3::Y, 1e-12), "top view: front at the bottom");
        let a = Camera { yaw: 3.0, roll: 0.0, distance: 10.0, ..Default::default() };
        let b = Camera { yaw: -3.0, roll: 0.0, distance: 1000.0, ..Default::default() };
        let m = Camera::lerp(&a, &b, 0.5);
        assert!((m.yaw.rem_euclid(2.0 * PI) - PI).abs() < 1e-9, "the short way round, through pi: {}", m.yaw);
        assert!((m.distance - 100.0).abs() < 1e-9, "geometric mean");
        assert_eq!(Camera::lerp(&a, &b, 1.0).distance, b.distance);
    }

    #[test]
    fn matrices_agree_with_projection_and_rays() {
        for proj in [Projection::Perspective, Projection::Orthographic] {
            let c = Camera { target: Vec3::new(5.0, 6.0, 7.0), distance: 80.0, projection: proj, ..Default::default() };
            let m = c.view_proj(800.0 / 600.0, 50.0);
            let p = Vec3::new(12.0, -3.0, 9.0);
            let clip = apply(&m, p);
            let (ndx, ndy) = (clip[0] / clip[3], clip[1] / clip[3]);
            let (x, y, _) = c.project(p, 800.0, 600.0).unwrap();
            assert!(((ndx as f64 + 1.0) * 400.0 - x).abs() < 1e-2, "{proj:?}");
            assert!(((1.0 - ndy as f64) * 300.0 - y).abs() < 1e-2, "{proj:?}");
            let depth = clip[2] / clip[3];
            assert!((0.0..=1.0).contains(&depth), "{proj:?}: depth {depth}");
            // The ray through the projected pixel passes through the point.
            let (o, d) = c.ray(x, y, 800.0, 600.0);
            let t = (p - o).dot(d);
            assert!((o + d * t).near(p, 1e-6), "{proj:?}");
        }
    }

    #[test]
    fn fit_zoom_pan_orbit() {
        let mut c = Camera::default();
        c.fit(&Aabb3::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 50.0, 20.0)));
        assert!(c.target.near(Vec3::new(50.0, 25.0, 10.0), 1e-9));
        let d = c.distance;
        c.zoom(0.5, None);
        assert!((c.distance - d / 2.0).abs() < 1e-9);
        let p = c.target + Vec3::new(10.0, 0.0, 0.0);
        let before = c.project(p, 800.0, 600.0).unwrap();
        c.zoom(0.5, Some(p));
        let after = c.project(p, 800.0, 600.0).unwrap();
        assert!((before.0 - after.0).abs() < 1e-6 && (before.1 - after.1).abs() < 1e-6, "the point under the pointer stays put");
        let t = c.target;
        c.pan(100.0, 0.0, 600.0);
        assert!((c.target - t).dot(c.right()) < 0.0, "dragging right moves the scene right");
        c.orbit(0.0, 1e6);
        assert!((c.pitch - FRAC_PI_2).abs() < 1e-12, "pitch clamps at straight down");
        c.zoom(f64::NAN, None);
        assert!(c.distance.is_finite());
    }
}

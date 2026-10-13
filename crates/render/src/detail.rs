//! Level of detail: how much of each body is drawn, from where it is on the screen.
//!
//! An assembly of hundreds of components shows most of them a few dozen pixels across, or not
//! at all. A body that is off the screen is not drawn; one that is drawn small is drawn from
//! its far mesh, which is within a pixel of the fine one at that size (`MeshTol::far`). What is
//! picked is always the fine mesh.

use tenon_geom::Vec3;

use crate::{Camera, Projection};

/// How a body is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Detail {
    /// Not at all: no part of it is on the screen.
    Hidden,
    /// From its far mesh, if it has one.
    Far,
    /// In full.
    Near,
}

/// A body whose bounding ball is drawn with a radius under this many pixels changes to its far
/// mesh. The far mesh is off the surface by at most a hundredth of the body's size (the
/// diameter of that ball), so by under a pixel here.
pub const FAR_BELOW: f64 = 45.0;

/// A body drawn from its far mesh changes back when its ball is drawn larger than this. The gap
/// keeps a body near the limit from changing back and forth as the view moves.
pub const NEAR_ABOVE: f64 = 55.0;

/// How a body inside the ball of `radius` about `center` is drawn in a view `w` by `h` pixels.
/// `was` is how it was drawn last.
pub fn detail(camera: &Camera, w: f64, h: f64, center: Vec3, radius: f64, was: Option<Detail>) -> Detail {
    let depth = (center - camera.eye()).dot(camera.forward());
    let perspective = camera.projection == Projection::Perspective;
    if perspective && depth + radius <= 0.0 {
        return Detail::Hidden;
    }
    // Round the eye, or nearly: in full.
    if perspective && depth < 2.0 * radius {
        return Detail::Near;
    }
    let (Some((x, y, _)), Some((_, above, _))) = (camera.project(center, w, h), camera.project(center + camera.up() * radius.max(1e-9), w, h)) else {
        return Detail::Near;
    };
    let pixels = (y - above).abs();
    // A ball away from the middle of a perspective view is drawn stretched: by less than twice.
    let reach = 2.0 * pixels + 2.0;
    if x + reach < 0.0 || x - reach > w || y + reach < 0.0 || y - reach > h {
        return Detail::Hidden;
    }
    let limit = if was == Some(Detail::Far) { NEAR_ABOVE } else { FAR_BELOW };
    if pixels < limit { Detail::Far } else { Detail::Near }
}

//! A mesh and where it stands.
//!
//! The components of one part in an assembly are the same body in different places. So that
//! they can share one mesh (in memory, in what picking keeps about it, and on the graphics
//! card), a body's mesh stays in the coordinates it was made in and says where it stands, and
//! whoever draws it or looks for what is under the pointer takes the two together.

use tenon_geom::{Frame, Vec3};

use crate::Mesh;
use crate::index::Box3;

/// A mesh, and the frame its coordinates are in (None: the coordinates of whoever asks).
#[derive(Clone, Copy, Debug)]
pub struct Placed<'a> {
    pub mesh: &'a Mesh,
    pub at: Option<&'a Frame>,
}

impl<'a> From<&'a Mesh> for Placed<'a> {
    fn from(mesh: &'a Mesh) -> Placed<'a> {
        Placed { mesh, at: None }
    }
}

impl<'a> Placed<'a> {
    pub fn new(mesh: &'a Mesh, at: Option<&'a Frame>) -> Placed<'a> {
        Placed { mesh, at }
    }

    /// A point of the mesh, where it stands.
    pub fn point(&self, p: [f32; 3]) -> Vec3 {
        let p = Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
        self.at.map_or(p, |f| f.to_world(p))
    }

    /// A direction of the mesh (a normal), as it stands.
    pub fn dir(&self, d: [f32; 3]) -> Vec3 {
        let d = Vec3::new(f64::from(d[0]), f64::from(d[1]), f64::from(d[2]));
        self.at.map_or(d, |f| f.dir_to_world(d))
    }

    /// A ray from `o` along `d`, in the mesh's own coordinates. Distances along it are the
    /// same: the mesh is moved, not stretched.
    pub fn ray_in(&self, o: Vec3, d: Vec3) -> (Vec3, Vec3) {
        self.at.map_or((o, d), |f| (f.to_local(o), f.dir_to_local(d)))
    }

    /// The corners of a box of the mesh, where they stand.
    pub fn corners(&self, b: &Box3) -> [Vec3; 8] {
        let c = b.corners();
        match self.at {
            Some(f) => c.map(|p| f.to_world(p)),
            None => c,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placed_mesh_is_where_its_frame_puts_it() {
        let m = Mesh::default();
        // Turned a quarter about Z and moved.
        let f = Frame::new(Vec3::new(10.0, 20.0, 30.0), Vec3::Z, Vec3::Y).unwrap_or(Frame::WORLD);
        let p = Placed::new(&m, Some(&f));
        let near = |a: Vec3, b: Vec3| (a - b).len() < 1e-12;
        assert!(near(p.point([1.0, 0.0, 0.0]), Vec3::new(10.0, 21.0, 30.0)));
        assert!(near(p.dir([1.0, 0.0, 0.0]), Vec3::Y));
        let (o, d) = p.ray_in(Vec3::new(10.0, 21.0, 35.0), -Vec3::Z);
        assert!(near(o, Vec3::new(1.0, 0.0, 5.0)) && near(d, -Vec3::Z));
        let unit = Box3 { min: [0.0; 3], max: [1.0; 3] };
        assert!(near(p.corners(&unit)[1], Vec3::new(10.0, 21.0, 30.0)));
        // Without a frame: as it is.
        let plain = Placed::from(&m);
        assert!(near(plain.point([1.0, 2.0, 3.0]), Vec3::new(1.0, 2.0, 3.0)));
        assert_eq!(plain.ray_in(Vec3::X, Vec3::Y), (Vec3::X, Vec3::Y));
    }
}

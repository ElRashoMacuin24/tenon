//! How a part would print on the usual desktop printer: is its surface closed, what overhangs,
//! how thin is its thinnest wall, does it fit the bed. An aid, not a slicer's verdict: it reads
//! the part as triangles, as the printer's software will.

use serde_json::{Value, json};
use tenon_geom::Vec3;
use tenon_kernel::{Kernel, MeshTol, ShapeHandle};

use crate::meshcheck::{Closure, Welded};

/// Which way is up on the printer, in the part's coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Up {
    X,
    Y,
    #[default]
    Z,
    NegX,
    NegY,
    NegZ,
}

impl Up {
    pub const ALL: [Up; 6] = [Up::Z, Up::NegZ, Up::X, Up::NegX, Up::Y, Up::NegY];

    pub fn name(self) -> &'static str {
        match self {
            Up::X => "x",
            Up::Y => "y",
            Up::Z => "z",
            Up::NegX => "-x",
            Up::NegY => "-y",
            Up::NegZ => "-z",
        }
    }

    pub fn parse(s: &str) -> Option<Up> {
        Some(match s {
            "x" | "+x" => Up::X,
            "y" | "+y" => Up::Y,
            "z" | "+z" => Up::Z,
            "-x" => Up::NegX,
            "-y" => Up::NegY,
            "-z" => Up::NegZ,
            _ => return None,
        })
    }

    pub fn dir(self) -> Vec3 {
        match self {
            Up::X => Vec3::new(1.0, 0.0, 0.0),
            Up::Y => Vec3::new(0.0, 1.0, 0.0),
            Up::Z => Vec3::new(0.0, 0.0, 1.0),
            Up::NegX => Vec3::new(-1.0, 0.0, 0.0),
            Up::NegY => Vec3::new(0.0, -1.0, 0.0),
            Up::NegZ => Vec3::new(0.0, 0.0, -1.0),
        }
    }
}

/// What the part is checked against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Setup {
    /// The thinnest wall that prints, mm.
    pub min_wall: f64,
    /// How far from vertical a face may lean out over air, degrees.
    pub overhang: f64,
    pub up: Up,
    /// The printer's bed and height, mm: [x, y, height]. Not checked when absent.
    pub bed: Option<[f64; 3]>,
}

impl Default for Setup {
    fn default() -> Self {
        Setup { min_wall: 0.8, overhang: 45.0, up: Up::Z, bed: None }
    }
}

impl Setup {
    /// The values make sense.
    pub fn check(&self) -> Result<(), String> {
        if !(self.min_wall > 0.0 && self.min_wall <= 100.0) {
            return Err("`min_wall` must be more than 0 and at most 100 mm".into());
        }
        if !(self.overhang > 0.0 && self.overhang <= 90.0) {
            return Err("`overhang` must be more than 0 and at most 90 degrees".into());
        }
        if self.bed.is_some_and(|b| b.iter().any(|x| !(x.is_finite() && *x > 0.0))) {
            return Err("`bed` must be three sizes in mm: [x, y, height]".into());
        }
        Ok(())
    }
}

/// A face that leans out past the limit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverhangAt {
    pub body: usize,
    pub face: u32,
    /// Area past the limit, mm^2.
    pub area: f64,
    /// Its flattest part, degrees from vertical (a ceiling is 90).
    pub steepest: f64,
}

/// A face with too little material behind it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThinAt {
    pub body: usize,
    pub face: u32,
    /// The least thickness behind it, mm.
    pub thinnest: f64,
    /// Area with less than the wanted thickness behind it, mm^2.
    pub area: f64,
}

/// What the check found.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub setup: Setup,
    pub closure: Closure,
    pub overhangs: Vec<OverhangAt>,
    pub overhang_area: f64,
    pub thin: Vec<ThinAt>,
    /// The thinnest wall found anywhere, mm (none: nothing to measure through).
    pub thinnest: Option<f64>,
    /// The part's extents along x, y and z, mm.
    pub size: Vec3,
    /// Its height standing as `setup.up` says, mm.
    pub height: f64,
    /// Whether it fits the bed (turned either way on it); none when no bed was given.
    pub fits: Option<bool>,
    /// What the triangles enclose, mm^3.
    pub volume: f64,
    pub triangles: usize,
}

impl Report {
    pub fn watertight(&self) -> bool {
        self.closure.watertight()
    }

    /// Nothing was found against printing it as it stands.
    pub fn ok(&self) -> bool {
        self.watertight() && self.overhangs.is_empty() && self.thin.is_empty() && self.fits != Some(false)
    }

    pub fn json(&self) -> Value {
        let overhangs: Vec<Value> =
            self.overhangs.iter().map(|o| json!({ "body": o.body, "face": o.face, "area": o.area, "steepest": o.steepest })).collect();
        let thin: Vec<Value> = self.thin.iter().map(|t| json!({ "body": t.body, "face": t.face, "thinnest": t.thinnest, "area": t.area })).collect();
        let c = &self.closure;
        json!({
            "ok": self.ok(),
            "watertight": { "ok": self.watertight(), "open_edges": c.open_edges, "crowded_edges": c.crowded_edges, "flipped_edges": c.flipped_edges },
            "overhangs": { "ok": overhangs.is_empty(), "limit": self.setup.overhang, "area": self.overhang_area, "faces": overhangs },
            "walls": { "ok": thin.is_empty(), "min_wall": self.setup.min_wall, "thinnest": self.thinnest, "faces": thin },
            "size": { "x": self.size.x, "y": self.size.y, "z": self.size.z, "height": self.height, "bed": self.setup.bed, "ok": self.fits },
            "up": self.setup.up.name(),
            "volume": self.volume,
            "triangles": self.triangles,
        })
    }
}

/// Checks the solids `shapes` (the bodies of a part, in order) against `setup`.
pub fn check(k: &mut dyn Kernel, shapes: &[ShapeHandle], setup: &Setup) -> Result<Report, String> {
    setup.check()?;
    let up = setup.up.dir();
    let mut r = Report {
        setup: *setup,
        closure: Closure::default(),
        overhangs: Vec::new(),
        overhang_area: 0.0,
        thin: Vec::new(),
        thinnest: None,
        size: Vec3::ZERO,
        height: 0.0,
        fits: None,
        volume: 0.0,
        triangles: 0,
    };
    let mut bounds: Option<(Vec3, Vec3)> = None;
    for (body, shape) in shapes.iter().enumerate() {
        // The mesh the part is shown with: its faces are numbered as the part's are, and what is
        // looked for here (holes, slopes, thickness) does not turn on the last chord.
        let w = Welded::of(&k.tessellate(*shape, &MeshTol::default()).map_err(|e| e.to_string())?);
        let c = w.closure();
        r.closure.open_edges += c.open_edges;
        r.closure.crowded_edges += c.crowded_edges;
        r.closure.flipped_edges += c.flipped_edges;
        r.volume += w.volume();
        r.triangles += w.triangles.len();
        for o in w.overhangs(up, setup.overhang.to_radians()) {
            r.overhang_area += o.area;
            r.overhangs.push(OverhangAt { body, face: o.face, area: o.area, steepest: o.steepest.to_degrees() });
        }
        let (least, faces) = w.walls(setup.min_wall);
        r.thinnest = match (r.thinnest, least) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        r.thin.extend(faces.iter().map(|t| ThinAt { body, face: t.face, thinnest: t.thinnest, area: t.area }));
        if let Some((lo, hi)) = w.bounds() {
            bounds = Some(match bounds {
                Some((a, b)) => (Vec3::new(a.x.min(lo.x), a.y.min(lo.y), a.z.min(lo.z)), Vec3::new(b.x.max(hi.x), b.y.max(hi.y), b.z.max(hi.z))),
                None => (lo, hi),
            });
        }
    }
    r.size = bounds.map_or(Vec3::ZERO, |(lo, hi)| hi - lo);
    // Standing as `up` says: its height, and the two sizes of its footprint (which may be turned
    // on the bed).
    let size = r.size;
    r.height = (size.x * up.x).abs() + (size.y * up.y).abs() + (size.z * up.z).abs();
    let mut foot: Vec<f64> = [(size.x, up.x), (size.y, up.y), (size.z, up.z)].iter().filter(|(_, u)| *u == 0.0).map(|(s, _)| *s).collect();
    foot.sort_by(f64::total_cmp);
    r.fits = setup.bed.map(|[x, y, z]| {
        let (small, large) = (x.min(y), x.max(y));
        foot.first().is_some_and(|f| *f <= small + 1e-9) && foot.last().is_some_and(|f| *f <= large + 1e-9) && r.height <= z + 1e-9
    });
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn up_names_round_trip_and_bad_setups_are_refused() {
        for up in Up::ALL {
            assert_eq!(Up::parse(up.name()), Some(up));
            assert!((up.dir().len() - 1.0).abs() < 1e-12);
        }
        assert_eq!((Up::parse("+z"), Up::parse("up")), (Some(Up::Z), None));
        assert!(Setup::default().check().is_ok());
        for bad in [
            Setup { min_wall: 0.0, ..Default::default() },
            Setup { min_wall: f64::NAN, ..Default::default() },
            Setup { min_wall: 101.0, ..Default::default() },
            Setup { overhang: 0.0, ..Default::default() },
            Setup { overhang: 91.0, ..Default::default() },
            Setup { bed: Some([220.0, 0.0, 250.0]), ..Default::default() },
            Setup { bed: Some([220.0, f64::INFINITY, 250.0]), ..Default::default() },
        ] {
            assert!(bad.check().is_err(), "{bad:?}");
        }
    }
}

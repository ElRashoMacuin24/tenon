//! Punch Tool: shapes stamped into a flat face of the sheet, from a library built into Tenon.
//!
//! Each shape is written here, with its own parameters: what it is called, what it takes out or
//! forms, how much sheet that comes to by hand. A cut is the shape's outline through the sheet,
//! and in the flat pattern it is that outline with the punch's name. A formed shape (a dimple, a
//! louver) is the sheet moved out of its plane: in the flat pattern it is a centre mark and the
//! line it bends at, with the punch's name: the press makes the rest in one stroke.

use std::f64::consts::{PI, TAU};

use serde::{Deserialize, Serialize};
use tenon_geom::Vec2;
use tenon_kernel::{Curve2, Loop, Region, TaggedCurve2};
use tenon_sketch::EntityId;

use crate::FeatureId;
use crate::naming::sub_key;

/// A shape of the library, with its sizes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PunchShape {
    /// A round hole.
    Round { diameter: f64 },
    /// A square hole.
    Square { side: f64 },
    /// A rectangular hole, `length` along the punch's X.
    Rectangle { length: f64, width: f64 },
    /// A slot with round ends, `length` overall along the punch's X, `width` across.
    Obround { length: f64, width: f64 },
    /// A round hole with one flat side: `across` from the flat to the far side of the circle
    /// (the flat is on the punch's +Y side).
    DShape { diameter: f64, across: f64 },
    /// A round dome: `diameter` where it leaves the far side of the sheet, standing `depth` out
    /// of the near side.
    Dimple { diameter: f64, depth: f64 },
    /// A flap lanced on three sides and bent up along the fourth: `length` along the hinge,
    /// `width` from the hinge to the lip laid flat, its lip's top `height` above the face.
    Louver { length: f64, width: f64, height: f64 },
}

/// What the library shows of a shape, and the fields it takes: the name it goes by in commands,
/// what it is called, what kind it is, what it does, and each size's key, label and tip.
#[derive(Clone, Copy, Debug)]
pub struct PunchFamily {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub about: &'static str,
    pub sizes: &'static [(&'static str, &'static str, &'static str)],
    /// What it starts as.
    pub starts: PunchShape,
}

pub const LIBRARY: [PunchFamily; 7] = [
    PunchFamily {
        id: "round",
        name: "Round",
        category: "Cuts",
        about: "A round hole through the sheet.",
        sizes: &[("diameter", "Diameter", "Across the hole")],
        starts: PunchShape::Round { diameter: 10.0 },
    },
    PunchFamily {
        id: "square",
        name: "Square",
        category: "Cuts",
        about: "A square hole through the sheet.",
        sizes: &[("side", "Side", "The length of a side")],
        starts: PunchShape::Square { side: 10.0 },
    },
    PunchFamily {
        id: "rectangle",
        name: "Rectangle",
        category: "Cuts",
        about: "A rectangular hole through the sheet.",
        sizes: &[("length", "Length", "Along the punch's X, which the rotation turns"), ("width", "Width", "Across it")],
        starts: PunchShape::Rectangle { length: 20.0, width: 10.0 },
    },
    PunchFamily {
        id: "obround",
        name: "Obround Slot",
        category: "Cuts",
        about: "A slot with round ends through the sheet.",
        sizes: &[
            ("length", "Length", "Overall, along the punch's X, which the rotation turns"),
            ("width", "Width", "Across it: the diameter of its ends"),
        ],
        starts: PunchShape::Obround { length: 24.0, width: 8.0 },
    },
    PunchFamily {
        id: "d_shape",
        name: "D-Shape",
        category: "Cuts",
        about: "A round hole with one flat side, so that what goes in it cannot turn.",
        sizes: &[("diameter", "Diameter", "Of the round part"), ("across", "Across Flat", "From the flat to the far side of the circle")],
        starts: PunchShape::DShape { diameter: 12.0, across: 10.0 },
    },
    PunchFamily {
        id: "dimple",
        name: "Dimple",
        category: "Formed",
        about: "A round dome pressed out of the sheet, to stiffen it or to keep what lies on it clear.",
        sizes: &[
            ("diameter", "Diameter", "Where the dome leaves the far side of the sheet"),
            ("depth", "Depth", "How far its top stands out of the near side"),
        ],
        starts: PunchShape::Dimple { diameter: 20.0, depth: 3.0 },
    },
    PunchFamily {
        id: "louver",
        name: "Louver",
        category: "Formed",
        about: "A flap lanced on three sides and bent up along the fourth: a vent. Open at its two ends.",
        sizes: &[
            ("length", "Length", "Along the hinge"),
            ("width", "Width", "From the hinge to the lip, laid flat"),
            ("height", "Height", "How far the top of the lip stands out of the face"),
        ],
        starts: PunchShape::Louver { length: 30.0, width: 15.0, height: 5.0 },
    },
];

/// The family of a shape.
pub fn family_of(shape: &PunchShape) -> Option<&'static PunchFamily> {
    let id = match shape {
        PunchShape::Round { .. } => "round",
        PunchShape::Square { .. } => "square",
        PunchShape::Rectangle { .. } => "rectangle",
        PunchShape::Obround { .. } => "obround",
        PunchShape::DShape { .. } => "d_shape",
        PunchShape::Dimple { .. } => "dimple",
        PunchShape::Louver { .. } => "louver",
    };
    LIBRARY.iter().find(|f| f.id == id)
}

/// A size as a name has it: as short as it can be.
fn size(v: f64) -> String {
    crate::explain::mm((v * 1000.0).round() / 1000.0)
}

impl PunchShape {
    pub fn is_formed(&self) -> bool {
        matches!(self, PunchShape::Dimple { .. } | PunchShape::Louver { .. })
    }

    /// What it is called in the flat pattern: "Dimple 20 x 3".
    pub fn name(&self) -> String {
        match *self {
            PunchShape::Round { diameter } => format!("Round {}", size(diameter)),
            PunchShape::Square { side } => format!("Square {}", size(side)),
            PunchShape::Rectangle { length, width } => format!("Rectangle {} x {}", size(length), size(width)),
            PunchShape::Obround { length, width } => format!("Obround {} x {}", size(length), size(width)),
            PunchShape::DShape { diameter, across } => format!("D-Shape {} x {}", size(diameter), size(across)),
            PunchShape::Dimple { diameter, depth } => format!("Dimple {} x {}", size(diameter), size(depth)),
            PunchShape::Louver { length, width, height } => format!("Louver {} x {} x {}", size(length), size(width), size(height)),
        }
    }

    /// Says what it cannot be.
    pub fn check(&self) -> Result<(), String> {
        let positive = |what: &str, v: f64| {
            if v.is_finite() && (0.01..=10_000.0).contains(&v) { Ok(()) } else { Err(format!("the punch's {what} must be from 0.01 to 10000 mm")) }
        };
        match *self {
            PunchShape::Round { diameter } => positive("diameter", diameter),
            PunchShape::Square { side } => positive("side", side),
            PunchShape::Rectangle { length, width } => {
                positive("length", length)?;
                positive("width", width)
            }
            PunchShape::Obround { length, width } => {
                positive("length", length)?;
                positive("width", width)?;
                if length <= width {
                    return Err("An obround slot is longer than it is wide: make its length more than its width, or use Round.".into());
                }
                Ok(())
            }
            PunchShape::DShape { diameter, across } => {
                positive("diameter", diameter)?;
                positive("across flat", across)?;
                if !(across > diameter / 2.0 && across < diameter) {
                    return Err("A D-shape's flat is between the middle of its circle and the far side: make Across Flat more than half the diameter and less than all of it.".into());
                }
                Ok(())
            }
            PunchShape::Dimple { diameter, depth } => {
                positive("diameter", diameter)?;
                positive("depth", depth)?;
                if depth > diameter / 2.0 {
                    return Err("A dimple is a dome of at most half a sphere: make its depth at most half its diameter.".into());
                }
                Ok(())
            }
            PunchShape::Louver { length, width, height } => {
                positive("length", length)?;
                positive("width", width)?;
                positive("height", height)
            }
        }
    }

    /// The area of a cut's outline (mm²); a formed shape takes nothing out.
    pub fn area(&self) -> f64 {
        match *self {
            PunchShape::Round { diameter } => PI * diameter * diameter / 4.0,
            PunchShape::Square { side } => side * side,
            PunchShape::Rectangle { length, width } => length * width,
            PunchShape::Obround { length, width } => (length - width) * width + PI * width * width / 4.0,
            PunchShape::DShape { diameter, across } => {
                let (r, gone) = (diameter / 2.0, diameter - across);
                let half = ((r - gone) / r).clamp(-1.0, 1.0).acos();
                PI * r * r - (r * r * half - (r - gone) * (2.0 * r * gone - gone * gone).max(0.0).sqrt())
            }
            PunchShape::Dimple { .. } | PunchShape::Louver { .. } => 0.0,
        }
    }

    /// The outline of a cut, about its centre: tags come from `tag(part)`.
    pub fn outline(&self, tag: impl Fn(u32) -> u64) -> Region {
        let line = |n: u32, a: Vec2, b: Vec2| TaggedCurve2 { tag: tag(n), curve: Curve2::Line { start: a, end: b } };
        let box_of = |l: f64, w: f64| {
            let (x, y) = (l / 2.0, w / 2.0);
            let c = [Vec2::new(-x, -y), Vec2::new(x, -y), Vec2::new(x, y), Vec2::new(-x, y)];
            (0..4).map(|i| line(i as u32, c[i], c[(i + 1) % 4])).collect::<Vec<_>>()
        };
        let curves = match *self {
            PunchShape::Round { diameter } => {
                vec![TaggedCurve2 { tag: tag(0), curve: Curve2::Circle { center: Vec2::ZERO, radius: diameter / 2.0 } }]
            }
            PunchShape::Square { side } => box_of(side, side),
            PunchShape::Rectangle { length, width } => box_of(length, width),
            PunchShape::Obround { length, width } => {
                let (r, x) = (width / 2.0, (length - width) / 2.0);
                let arc = |n: u32, cx: f64, from: f64| TaggedCurve2 {
                    tag: tag(n),
                    curve: Curve2::Arc { center: Vec2::new(cx, 0.0), radius: r, start_angle: from, end_angle: from + PI },
                };
                vec![
                    line(0, Vec2::new(-x, -r), Vec2::new(x, -r)),
                    arc(1, x, -PI / 2.0),
                    line(2, Vec2::new(x, r), Vec2::new(-x, r)),
                    arc(3, -x, PI / 2.0),
                ]
            }
            PunchShape::DShape { diameter, across } => {
                // The flat is `across` from the circle's far side: on the +Y side of the centre.
                let r = diameter / 2.0;
                let y = across - r;
                let half = (y / r).clamp(-1.0, 1.0).asin();
                // Counter-clockwise round the circle from the flat's right-hand end to its left.
                let (from, to) = (half, PI - half);
                let (right, left) = (Vec2::polar(Vec2::ZERO, r, from), Vec2::polar(Vec2::ZERO, r, to));
                vec![
                    TaggedCurve2 { tag: tag(0), curve: Curve2::Arc { center: Vec2::ZERO, radius: r, start_angle: to, end_angle: from + TAU } },
                    line(1, right, left),
                ]
            }
            // (What a formed shape takes out is worked out with what it forms.)
            PunchShape::Dimple { diameter, .. } => {
                vec![TaggedCurve2 { tag: tag(0), curve: Curve2::Circle { center: Vec2::ZERO, radius: diameter / 2.0 } }]
            }
            PunchShape::Louver { length, width, .. } => box_of(length, width),
        };
        Region { outer: Loop { curves }, holes: vec![] }
    }

    /// Points in order round the whole of what the stamp takes up on the face it is on, about its
    /// centre, for a sheet `t` thick: to see that it is all on the face.
    pub fn reach(&self, t: f64) -> Vec<Vec2> {
        let round = |r: f64, cx: f64, from: f64, sweep: f64, n: usize| -> Vec<Vec2> {
            (0..=n).map(|i| Vec2::polar(Vec2::new(cx, 0.0), r, from + sweep * i as f64 / n as f64)).collect()
        };
        let box_of = |l: f64, w: f64| {
            let (x, y) = (l / 2.0, w / 2.0);
            let corners = [Vec2::new(-x, -y), Vec2::new(x, -y), Vec2::new(x, y), Vec2::new(-x, y)];
            let mut out = Vec::with_capacity(32);
            for i in 0..4 {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                out.extend((0..8).map(|k| a + (b - a) * (f64::from(k) / 8.0)));
            }
            out
        };
        match *self {
            PunchShape::Round { diameter } => round(diameter / 2.0, 0.0, 0.0, TAU, 48),
            PunchShape::Square { side } => box_of(side, side),
            PunchShape::Rectangle { length, width } | PunchShape::Louver { length, width, .. } => box_of(length, width),
            PunchShape::Obround { length, width } => {
                let (r, x) = (width / 2.0, (length - width) / 2.0);
                let mut out = round(r, x, -PI / 2.0, PI, 24);
                out.extend(round(r, -x, PI / 2.0, PI, 24));
                out
            }
            PunchShape::DShape { diameter, across } => {
                let r = diameter / 2.0;
                let half = ((across - r) / r).clamp(-1.0, 1.0).asin();
                round(r, 0.0, PI - half, TAU - (PI - 2.0 * half), 48)
            }
            PunchShape::Dimple { diameter, depth } => {
                let r = diameter / 2.0;
                round((r * r + 2.0 * t * depth).sqrt(), 0.0, 0.0, TAU, 48)
            }
        }
    }

    /// What a formed shape comes to in the sheet, beside what its faces on the sheet's sides
    /// account for: how much sheet is in it (mm³), for a sheet `t` thick with bends of inside
    /// radius `r`. Nothing, for a cut.
    pub fn formed_volume(&self, t: f64, r: f64, k: f64) -> f64 {
        match *self {
            PunchShape::Dimple { diameter, depth } => {
                let base = diameter / 2.0;
                PI * (base * base + t * depth) * t + PI * depth * depth * t
            }
            PunchShape::Louver { length, width, height } => {
                let (turn, straight) = louver_flap(width, height, t, r, k).unwrap_or((0.0, width));
                length * t * (straight + turn * (r + t / 2.0))
            }
            _ => 0.0,
        }
    }
}

/// A louver's flap for a lip `height` above the face: how far it turns at its hinge, and how long
/// it runs straight after that. It is `width` long laid flat. An error says what cannot be.
pub fn louver_flap(width: f64, height: f64, t: f64, r: f64, k: f64) -> Result<(f64, f64), String> {
    let neutral = r + k * t;
    let straight = |turn: f64| width - turn * neutral;
    // How high the top of the lip stands, for a turn at the hinge: the top of the sheet is the
    // inside of the bend, so the path is that face's own.
    let rise = |turn: f64| r * (1.0 - turn.cos()) + straight(turn) * turn.sin();
    let (mut lo, mut hi) = (0.0f64, std::f64::consts::FRAC_PI_2);
    // (The bend alone may take all of the flap before it stands upright.)
    while straight(hi) < 0.0 && hi > 1e-6 {
        hi /= 2.0;
    }
    if rise(hi) < height {
        return Err(format!(
            "A louver {} mm wide stands at most {} mm high with this sheet's bends: make it wider or lower, or the bend radius smaller.",
            size(width),
            size(rise(hi))
        ));
    }
    // Whether the underside of the flap, turned this far, clears the far wall of its opening: the
    // wall is `width` from the hinge and goes down from the face. (Below that height the lip of a
    // flap as long as its opening runs into that wall, wherever on the sheet it is.)
    let clears = |turn: f64| {
        let (sin, cos) = turn.sin_cos();
        let reach = r * sin + straight(turn) * cos + t * sin;
        let top = r * (1.0 - cos) + straight(turn) * sin - t * cos;
        reach <= width || top - (reach - width) * sin / cos.max(1e-9) >= 1e-3 * t
    };
    if !clears(hi) {
        return Err(format!(
            "A louver {} mm wide runs into the far side of its own opening however high it stands, with this sheet's thickness and bends: make it wider, or the bend radius smaller.",
            size(width)
        ));
    }
    let (mut under, mut over) = (0.0f64, hi);
    for _ in 0..60 {
        let mid = (under + over) / 2.0;
        if clears(mid) {
            over = mid;
        } else {
            under = mid;
        }
    }
    if rise(over) > height {
        return Err(format!(
            "A louver on this sheet must stand at least {} mm high, or the lip of its flap runs into the far side of its own opening: make it higher.",
            size((rise(over) * 1000.0).ceil() / 1000.0)
        ));
    }
    for _ in 0..80 {
        let mid = (lo + hi) / 2.0;
        if rise(mid) < height {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok((hi, straight(hi)))
}

/// What a dimple is seen in a cut through it, on a sheet from 0 down to `-t`, about its axis: the
/// sphere's centre height and its radius on the far side of the sheet, and how wide it is where
/// it leaves the near side.
pub fn dimple_section(diameter: f64, depth: f64, t: f64) -> (f64, f64, f64) {
    let base = diameter / 2.0;
    let radius = (base * base + depth * depth) / (2.0 * depth);
    (depth - t - radius, radius, (base * base + 2.0 * t * depth).sqrt())
}

/// Stamps a shape from the library at sketch points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Punch {
    /// The sketch its centre points are in: on a flat side of the sheet.
    pub sketch: FeatureId,
    /// The points; none: every centre point of the sketch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<EntityId>,
    pub shape: PunchShape,
    /// How far the shape is turned, about the sketch's normal from its X (radians).
    #[serde(default, skip_serializing_if = "is_nothing")]
    pub rotation: f64,
    /// A formed shape stands out of the sheet's other side.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_nothing(v: &f64) -> bool {
    *v == 0.0
}

impl Punch {
    pub fn check(&self) -> Result<(), String> {
        self.shape.check()?;
        if !self.rotation.is_finite() {
            return Err("the punch's rotation must be a number".into());
        }
        if self.points.len() > 1000 {
            return Err("a punch takes at most 1000 points".into());
        }
        Ok(())
    }
}

/// What a face made by a punch at a point is named after.
pub(crate) fn tag(point: EntityId, part: u32) -> u64 {
    sub_key(sub_key(0x5055_4e43, point.0), part)
}

/// Parts of a stamp past those of its outline: where a cut's curves take numbers from 0.
pub(crate) const PART_FLAP: u32 = 16;
pub(crate) const PART_DOME: u32 = 48;
pub(crate) const PARTS: u32 = 64;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn every_shape_of_the_library_comes_to_what_it_should_by_hand() {
        for f in &LIBRARY {
            f.starts.check().unwrap();
            assert_eq!(family_of(&f.starts).unwrap().id, f.id);
            assert!(!f.sizes.is_empty() && !f.about.is_empty());
        }
        let area = |s: PunchShape| s.area();
        assert!((area(PunchShape::Round { diameter: 10.0 }) - 25.0 * PI).abs() < 1e-12);
        assert!((area(PunchShape::Square { side: 10.0 }) - 100.0).abs() < 1e-12);
        assert!((area(PunchShape::Rectangle { length: 20.0, width: 10.0 }) - 200.0).abs() < 1e-12);
        // 24 x 8: 16 x 8 and two half circles of radius 4.
        assert!((area(PunchShape::Obround { length: 24.0, width: 8.0 }) - (128.0 + 16.0 * PI)).abs() < 1e-12);
        // D 12 across 10: a circle less a segment 2 high. The segment: half angle acos(4/6).
        let half = (4.0f64 / 6.0).acos();
        let segment = 36.0 * half - 4.0 * 20f64.sqrt();
        assert!((area(PunchShape::DShape { diameter: 12.0, across: 10.0 }) - (36.0 * PI - segment)).abs() < 1e-9);
        assert_eq!(PunchShape::Dimple { diameter: 20.0, depth: 3.0 }.name(), "Dimple 20 x 3");
        assert_eq!(PunchShape::Louver { length: 30.0, width: 15.0, height: 5.0 }.name(), "Louver 30 x 15 x 5");
        // What cannot be is said.
        assert!(PunchShape::Obround { length: 8.0, width: 8.0 }.check().unwrap_err().contains("longer than it is wide"));
        assert!(PunchShape::DShape { diameter: 10.0, across: 4.0 }.check().unwrap_err().contains("Across Flat"));
        assert!(PunchShape::Dimple { diameter: 10.0, depth: 6.0 }.check().unwrap_err().contains("half its diameter"));
        assert!(PunchShape::Round { diameter: f64::NAN }.check().unwrap_err().contains("diameter"));
    }

    #[test]
    fn a_louvers_flap_is_as_long_as_its_opening_and_stands_as_high_as_it_was_asked() {
        // A flap 15 long, 2 thick, bends of 3: a turn t takes t (3 + 0.44 x 2) of it.
        let (turn, straight) = louver_flap(15.0, 5.0, 2.0, 3.0, 0.44).unwrap();
        assert!((straight + turn * 3.88 - 15.0).abs() < 1e-12);
        assert!((3.0 * (1.0 - turn.cos()) + straight * turn.sin() - 5.0).abs() < 1e-9);
        // Higher than it can stand: said.
        assert!(louver_flap(15.0, 50.0, 2.0, 3.0, 0.44).unwrap_err().contains("stands at most"));
    }

    #[test]
    fn a_dimples_dome_is_two_spheres_about_one_centre() {
        // 20 across, 3 deep, 2 thick: the far side's sphere goes through (10, -2) and (0, 1).
        let (centre, radius, wide) = dimple_section(20.0, 3.0, 2.0);
        assert!((radius - 109.0 / 6.0).abs() < 1e-12);
        assert!((centre - (1.0 - radius)).abs() < 1e-12);
        assert!((100.0 + (centre + 2.0) * (centre + 2.0) - radius * radius).abs() < 1e-9);
        // The near side's goes through (0, 3) and leaves the face at sqrt(100 + 2 x 2 x 3).
        assert!((wide - 112f64.sqrt()).abs() < 1e-12);
        assert!((wide * wide + centre * centre - (radius + 2.0) * (radius + 2.0)).abs() < 1e-9);
    }
}

//! Sheet metal: a part of one thickness, made of flat faces joined by bends, that is cut from
//! a flat sheet and folded.
//!
//! - The **rule** of the part ([`SheetMetal`]) says how reliefs are shaped; its sizes are
//!   parameters of the part with fixed names ([`THICKNESS`], [`BEND_RADIUS`], ...), so they take
//!   equations and design table columns like any other and the features follow them.
//! - The features: [`Face`] (a sketch profile one thickness thick), [`Flange`] (a bend and a
//!   wall along edges of the sheet, mitred and relieved where two meet), [`Hem`] (an edge folded
//!   back on itself), [`ContourFlange`] (sheet along an open profile), [`CornerSeam`] and
//!   [`CutAcrossBend`] (a profile cut round the bends, as drawn on the sheet laid flat: [`wrap`]).
//!   Other cuts, holes and rounded corners are the ordinary features.
//! - The **flat pattern** ([`flat`], [`unfold`]) is worked out from the solid, not from the
//!   features, so whatever made the part, what is cut is what the model shows.

pub mod flange;
pub mod flat;
pub mod outline;
pub mod region;
pub mod strip;
pub mod unfold;
pub mod wrap;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tenon_kernel::Kernel;

use crate::document::{Operation, RegionSel};
use crate::naming::{EdgeRef, FaceRef};
use crate::params::{ParamUnit, UserParam};
use crate::{Document, FeatureId};

pub use flat::{Bend, FlatCurve, FlatLoop, FlatPattern};
pub use unfold::{Unfold, unfold};

/// The names of the part's sheet metal parameters.
pub const THICKNESS: &str = "Thickness";
pub const BEND_RADIUS: &str = "BendRadius";
pub const RELIEF_WIDTH: &str = "BendReliefWidth";
pub const RELIEF_DEPTH: &str = "BendReliefDepth";
pub const MIN_REMNANT: &str = "MinimumRemnant";
pub const CORNER_SIZE: &str = "CornerReliefSize";
pub const K_FACTOR: &str = "KFactor";
pub const GAP_SIZE: &str = "GapSize";

/// Every sheet metal parameter: its name, what it starts as, its unit and what it is.
pub const PARAMETERS: [(&str, &str, ParamUnit, &str); 8] = [
    (THICKNESS, "1 mm", ParamUnit::Mm, "Thickness of the sheet"),
    (BEND_RADIUS, "Thickness", ParamUnit::Mm, "Radius of the inside of a bend"),
    (RELIEF_WIDTH, "Thickness", ParamUnit::Mm, "Width of the relief cut beside a bend"),
    (RELIEF_DEPTH, "Thickness * 0.5", ParamUnit::Mm, "How far a bend relief goes past the bend"),
    (MIN_REMNANT, "Thickness * 2", ParamUnit::Mm, "A strip left beside a bend relief narrower than this is cut away"),
    (CORNER_SIZE, "Thickness * 4", ParamUnit::Mm, "Size of the relief where two bends meet at a corner"),
    (K_FACTOR, "0.44", ParamUnit::Ul, "Where the neutral surface lies in the thickness: 0 inside of the bend, 1 outside"),
    (GAP_SIZE, "Thickness", ParamUnit::Mm, "Gap left between flanges that meet at a corner"),
];

/// The shape of the cut beside a bend that is narrower than the edge it is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReliefShape {
    /// A slot with square corners.
    #[default]
    Straight,
    /// A slot with a round end.
    Round,
}

/// The shape of the cut where two bends meet at a corner of the sheet. It is laid out in the
/// flat sheet, centred where the centre lines of the two bends cross, so it takes the corner of
/// the sheet between the bends and, when it is large enough, the ends of the bends and the
/// feet of the walls beside it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CornerShape {
    /// A round hole, `CornerReliefSize` across.
    #[default]
    Round,
    /// A square hole with sides of `CornerReliefSize` along the two bend lines.
    Square,
    /// None: the bends simply end where their two lines cross.
    Trim,
}

/// What makes a part a sheet metal part: how its reliefs are shaped, and which face its flat
/// pattern is laid out from. Its sizes are the parameters named in [`PARAMETERS`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SheetMetal {
    #[serde(default)]
    pub relief: ReliefShape,
    #[serde(default)]
    pub corner: CornerShape,
    /// The face that is up when the pattern lies on the table (the A side). Without one it is
    /// the largest flat face.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a_side: Option<FaceRef>,
    /// The part has a flat pattern: it is shown in the browser and kept up to date.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flat: bool,
}

/// The sizes of the rule as they now stand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizes {
    pub thickness: f64,
    pub bend_radius: f64,
    pub relief_width: f64,
    pub relief_depth: f64,
    pub min_remnant: f64,
    pub corner_size: f64,
    pub k_factor: f64,
    /// The gap left between flanges that meet at a corner.
    pub gap: f64,
}

impl Sizes {
    /// Reads the sizes from the part's parameter values, and says which is missing or cannot be.
    pub fn from_values(values: &BTreeMap<String, f64>) -> Result<Sizes, String> {
        let get = |name: &str| -> Result<f64, String> {
            values
                .get(name)
                .copied()
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("The part has no parameter `{name}`: open Sheet Metal Defaults and click OK to put it back."))
        };
        let s = Sizes {
            thickness: get(THICKNESS)?,
            bend_radius: get(BEND_RADIUS)?,
            relief_width: get(RELIEF_WIDTH)?,
            relief_depth: get(RELIEF_DEPTH)?,
            min_remnant: get(MIN_REMNANT)?,
            corner_size: get(CORNER_SIZE)?,
            k_factor: get(K_FACTOR)?,
            // (Parts made before there was one: a thickness, as it starts out.)
            gap: values.get(GAP_SIZE).copied().filter(|v| v.is_finite()).unwrap_or(get(THICKNESS)?),
        };
        s.check()?;
        Ok(s)
    }

    pub fn check(&self) -> Result<(), String> {
        if !(self.thickness >= MIN_THICKNESS && self.thickness <= MAX_THICKNESS) {
            return Err(format!("{THICKNESS} must be from {MIN_THICKNESS} to {MAX_THICKNESS} mm"));
        }
        if !(self.bend_radius >= MIN_BEND_RADIUS && self.bend_radius <= 1000.0) {
            return Err(format!("{BEND_RADIUS} must be from {MIN_BEND_RADIUS} to 1000 mm (a bend is never perfectly sharp)"));
        }
        if !(0.0..=1.0).contains(&self.k_factor) {
            return Err(format!("{K_FACTOR} must be from 0 to 1"));
        }
        for (name, v) in [
            (RELIEF_WIDTH, self.relief_width),
            (RELIEF_DEPTH, self.relief_depth),
            (MIN_REMNANT, self.min_remnant),
            (CORNER_SIZE, self.corner_size),
            (GAP_SIZE, self.gap),
        ] {
            if !(0.0..=1000.0).contains(&v) {
                return Err(format!("{name} must be from 0 to 1000 mm"));
            }
        }
        Ok(())
    }
}

pub const MIN_THICKNESS: f64 = 0.05;
pub const MAX_THICKNESS: f64 = 100.0;
pub const MIN_BEND_RADIUS: f64 = 0.01;

/// A flat piece of sheet: a sketch profile made one thickness thick.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub sketch: FeatureId,
    #[serde(default)]
    pub regions: RegionSel,
    /// Put the thickness behind the sketch plane instead of in front of it.
    #[serde(default)]
    pub flip: bool,
    /// Join (to the sheet it touches) or a new solid.
    #[serde(default)]
    pub operation: Operation,
}

impl Face {
    pub fn check(&self) -> Result<(), String> {
        match self.operation {
            Operation::Join | Operation::NewBody => Ok(()),
            _ => Err("a sheet metal face is joined to the part or starts a new solid; to remove material use Cut".into()),
        }
    }
}

/// What a flange's height is measured from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeightDatum {
    /// Where the outer faces of the sheet and of the flange would meet (the overall height).
    #[default]
    Outer,
    /// Where their inner faces would meet.
    Inner,
    /// The plane that touches the outside of the bend, square to the flange.
    Tangent,
}

/// Where the bend lies relative to the edge the flange is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BendPosition {
    /// The flange's outer face goes through the edge: the part gets no bigger. (The sheet is
    /// cut back to make room for the bend.)
    #[default]
    Inside,
    /// The bend starts at the edge: nothing is cut, and the part grows by the bend.
    Adjacent,
    /// The flange's inner face goes through the edge: the part grows by the thickness.
    Outside,
    /// The outside of the bend touches the plane of the edge's face.
    Tangent,
}

/// How two walls that meet at a corner are brought together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Seam {
    /// Both stop the same way short of one another, with the gap between them.
    #[default]
    Symmetric,
    /// The first runs on across the end of the second, which stops the gap short of it.
    Overlap,
    /// The second runs on across the end of the first.
    ReverseOverlap,
}

fn one() -> f64 {
    1.0
}
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_one(v: &f64) -> bool {
    *v == 1.0
}
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_symmetric(s: &Seam) -> bool {
    *s == Seam::Symmetric
}

/// What a corner may have of its own in place of the rule's: its seam, its gap and its relief.
/// Shared by a flange (for all the corners it makes) and a corner seam (for the one it closes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CornerOptions {
    #[serde(default, skip_serializing_if = "is_symmetric")]
    pub seam: Seam,
    /// In an overlap seam, how much of the other wall's thickness is covered: 0 to 1.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub overlap: f64,
    /// The gap between the walls; without it, the part's `GapSize`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<f64>,
    /// The relief's shape and size; without them, the rule's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relief: Option<CornerShape>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relief_size: Option<f64>,
}

impl Default for CornerOptions {
    fn default() -> Self {
        CornerOptions { seam: Seam::Symmetric, overlap: 1.0, gap: None, relief: None, relief_size: None }
    }
}

impl CornerOptions {
    /// Nothing of its own: everything is the rule's.
    pub fn is_plain(&self) -> bool {
        *self == CornerOptions::default()
    }

    pub fn check(&self) -> Result<(), String> {
        if !(self.overlap.is_finite() && (0.0..=1.0).contains(&self.overlap)) {
            return Err("the overlap of a corner seam is a part of the thickness, from 0 to 1".into());
        }
        if let Some(g) = self.gap
            && !(g.is_finite() && (0.0..=1000.0).contains(&g))
        {
            return Err("the gap of a corner seam must be from 0 to 1000 mm".into());
        }
        if let Some(s) = self.relief_size
            && !(s.is_finite() && (0.0..=1000.0).contains(&s))
        {
            return Err("the size of a corner relief must be from 0 to 1000 mm".into());
        }
        Ok(())
    }

    /// The corner as it is to be made, with the rule's shape and sizes where it has none of its
    /// own. `joined`: the walls are brought together at all.
    pub fn corners(&self, rule: &SheetMetal, sizes: &Sizes, joined: bool) -> flange::Corners {
        flange::Corners {
            shape: self.relief.unwrap_or(rule.corner),
            size: self.relief_size.unwrap_or(sizes.corner_size),
            joint: joined.then_some(flange::Joint { seam: self.seam, gap: self.gap.unwrap_or(sizes.gap), overlap: self.overlap }),
        }
    }
}

/// The corner between two faces of the sheet closed: the two brought together to a seam, their
/// bends ended where the two bend lines cross, and the corner relieved. For faces that were not
/// made by one flange, which closes its own corners.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CornerSeam {
    /// The edge at the end of each of the two faces, where they come together: the first face's,
    /// then the second's.
    pub edges: [EdgeRef; 2],
    #[serde(flatten)]
    pub corner: CornerOptions,
}

impl CornerSeam {
    pub fn check(&self) -> Result<(), String> {
        self.corner.check()
    }
}

/// A profile cut out of the sheet as it lies flat: laid out from the face the profile's sketch
/// is on, every face and every bend the profile lies on loses its part of it, so the cut goes
/// round the bends with the sheet (Cut with Cut Across Bend). In the flat pattern it is the
/// profile as drawn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CutAcrossBend {
    pub sketch: FeatureId,
    #[serde(default)]
    pub regions: RegionSel,
    /// How deep the cut goes, from the side of the sheet the sketch is on; without it (or at
    /// the thickness or more) it goes through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
}

impl CutAcrossBend {
    pub fn check(&self) -> Result<(), String> {
        match self.distance {
            Some(d) if !(d.is_finite() && d > 0.0) => Err("the depth of a cut must be more than 0".into()),
            _ => Ok(()),
        }
    }
}

/// How much of its edge a flange takes up.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlangeWidth {
    /// All of it.
    #[default]
    Edge,
    /// A width, in the middle of the edge.
    Centered { width: f64 },
    /// From `start` after the edge begins to `end` before it ends.
    Offset { start: f64, end: f64 },
}

/// Walls bent up from edges of the sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Flange {
    /// Straight edges of the sheet's rim. The flange bends towards the side each edge is on.
    pub edges: Vec<EdgeRef>,
    /// How far the sheet turns (radians): a quarter turn stands the flange upright.
    pub angle: f64,
    pub height: f64,
    #[serde(default)]
    pub datum: HeightDatum,
    #[serde(default)]
    pub position: BendPosition,
    /// Bend towards the other side of the sheet.
    #[serde(default)]
    pub flip: bool,
    /// The inside radius of the bend; without it, the part's `BendRadius`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    #[serde(default)]
    pub width: FlangeWidth,
    /// Where the flange meets another of the same feature at a corner, the two walls are
    /// brought together until the part's `GapSize` is left between them.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub miter: bool,
    /// What its corners have of their own in place of the rule's: the seam, the gap, the relief.
    #[serde(flatten)]
    pub corner: CornerOptions,
}

fn yes() -> bool {
    true
}
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_yes(b: &bool) -> bool {
    *b
}

/// The smallest and largest angle a flange turns through.
pub const MIN_FLANGE_ANGLE: f64 = 1.0 * std::f64::consts::PI / 180.0;
pub const MAX_FLANGE_ANGLE: f64 = 179.0 * std::f64::consts::PI / 180.0;
/// Most edges one flange feature takes (hostile-input cap).
pub const MAX_FLANGE_EDGES: usize = 200;

impl Flange {
    pub fn check(&self) -> Result<(), String> {
        if self.edges.is_empty() {
            return Err("select at least one edge for the flange".into());
        }
        if self.edges.len() > MAX_FLANGE_EDGES {
            return Err(format!("a flange takes at most {MAX_FLANGE_EDGES} edges"));
        }
        if !(self.angle.is_finite() && (MIN_FLANGE_ANGLE..=MAX_FLANGE_ANGLE).contains(&self.angle)) {
            return Err("the flange angle must be from 1 to 179 degrees".into());
        }
        if !(self.height.is_finite() && self.height > 0.0 && self.height <= 1e5) {
            return Err("the flange height must be more than 0".into());
        }
        if let Some(r) = self.radius
            && !(r.is_finite() && (MIN_BEND_RADIUS..=1000.0).contains(&r))
        {
            return Err(format!("the bend radius must be from {MIN_BEND_RADIUS} to 1000 mm"));
        }
        self.corner.check()?;
        match self.width {
            FlangeWidth::Edge => {}
            FlangeWidth::Centered { width } if !(width.is_finite() && width > 0.0) => return Err("the flange width must be more than 0".into()),
            FlangeWidth::Offset { start, end } if !(start.is_finite() && end.is_finite() && start >= 0.0 && end >= 0.0) => {
                return Err("the flange's offsets from the ends of its edge cannot be negative".into());
            }
            _ => {}
        }
        Ok(())
    }
}

/// How a hem is folded.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HemShape {
    /// Folded flat back over the sheet, `gap` between the two faces. `length` is overall: from
    /// the outside of the fold to the hem's end.
    Single { gap: f64, length: f64 },
    /// Folded over twice: the hem's own end is folded in under it, with `gap` between each two
    /// layers. `length` is overall, from the outside of the outer fold to that of the inner.
    Double { gap: f64, length: f64 },
    /// Rolled: a bend of inside `radius` through `angle`, and nothing after it.
    Rolled { radius: f64, angle: f64 },
    /// Rolled through more than half a turn (`angle`) and then run straight back down to the
    /// face of the sheet.
    Teardrop { radius: f64, angle: f64 },
}

/// How far short of the sheet a teardrop hem stops (mm): touching it, the part would be a solid
/// that meets itself along a line.
pub const TEARDROP_CLEAR: f64 = 0.01;

/// The edge of the sheet folded back on itself: stiffer, and no longer sharp.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hem {
    /// Straight edges of the sheet's rim. The hem folds towards the side each edge is on.
    pub edges: Vec<EdgeRef>,
    pub shape: HemShape,
    /// Fold towards the other side of the sheet.
    #[serde(default)]
    pub flip: bool,
    #[serde(default)]
    pub width: FlangeWidth,
}

/// The largest angle a rolled hem turns through, and the range of a teardrop's.
pub const MAX_ROLL_ANGLE: f64 = 350.0 * std::f64::consts::PI / 180.0;
pub const MIN_TEARDROP_ANGLE: f64 = 181.0 * std::f64::consts::PI / 180.0;
pub const MAX_TEARDROP_ANGLE: f64 = 270.0 * std::f64::consts::PI / 180.0;

impl Hem {
    pub fn check(&self) -> Result<(), String> {
        if self.edges.is_empty() {
            return Err("select at least one edge for the hem".into());
        }
        if self.edges.len() > MAX_FLANGE_EDGES {
            return Err(format!("a hem takes at most {MAX_FLANGE_EDGES} edges"));
        }
        let size = |what: &str, v: f64, least: f64| {
            if v.is_finite() && (least..=1000.0).contains(&v) { Ok(()) } else { Err(format!("the hem's {what} must be from {least} to 1000 mm")) }
        };
        match self.shape {
            HemShape::Single { gap, length } | HemShape::Double { gap, length } => {
                // The fold's inside radius is half the gap.
                size("gap", gap, 2.0 * MIN_BEND_RADIUS)?;
                size("length", length, 0.001)?;
            }
            HemShape::Rolled { radius, angle } => {
                size("radius", radius, MIN_BEND_RADIUS)?;
                if !(angle.is_finite() && (MIN_FLANGE_ANGLE..=MAX_ROLL_ANGLE).contains(&angle)) {
                    return Err("a rolled hem turns through 1 to 350 degrees".into());
                }
            }
            HemShape::Teardrop { radius, angle } => {
                size("radius", radius, MIN_BEND_RADIUS)?;
                if !(angle.is_finite() && (MIN_TEARDROP_ANGLE..=MAX_TEARDROP_ANGLE).contains(&angle)) {
                    return Err("a teardrop hem turns through 181 to 270 degrees (a rolled hem takes any angle)".into());
                }
            }
        }
        match self.width {
            FlangeWidth::Centered { width } if !(width.is_finite() && width > 0.0) => Err("the hem's width must be more than 0".into()),
            FlangeWidth::Offset { start, end } if !(start.is_finite() && end.is_finite() && start >= 0.0 && end >= 0.0) => {
                Err("the hem's offsets from the ends of its edge cannot be negative".into())
            }
            _ => Ok(()),
        }
    }

    /// The hem seen end on, as steps from the edge: the path is the face of the sheet the hem
    /// folds towards (the inside of every fold), setting off straight out of the sheet.
    pub fn steps(&self, t: f64) -> Result<Vec<strip::Step>, String> {
        use strip::Step::{Bend, Straight};
        let half = std::f64::consts::PI;
        match self.shape {
            HemShape::Single { gap, length } => {
                let fold = gap / 2.0 + t;
                if length < fold - 1e-9 {
                    return Err(format!(
                        "The hem is {} mm long and its fold alone takes {} mm: make it longer, or the gap smaller.",
                        crate::explain::mm(length),
                        crate::explain::mm(fold)
                    ));
                }
                Ok(vec![Bend { radius: gap / 2.0, angle: half }, Straight(length - fold)])
            }
            HemShape::Double { gap, length } => {
                // The outer fold goes round the inner layer and a gap either side of it.
                let (outer, inner) = (gap + t / 2.0, gap / 2.0);
                let flat = length - (outer + t) - (inner + t);
                if flat < 1e-6 {
                    return Err(format!(
                        "The hem is {} mm long and its two folds alone take {} mm: make it longer, or the gap smaller.",
                        crate::explain::mm(length),
                        crate::explain::mm(outer + inner + 2.0 * t)
                    ));
                }
                Ok(vec![Bend { radius: outer, angle: half }, Straight(flat), Bend { radius: inner, angle: half }, Straight(flat)])
            }
            HemShape::Rolled { radius, angle } => Ok(vec![Bend { radius, angle }]),
            HemShape::Teardrop { radius, angle } => {
                // From where the roll ends, straight down to just short of the sheet.
                let (sin, cos) = angle.sin_cos();
                let down = (radius * (1.0 - cos) - TEARDROP_CLEAR) / -sin;
                if !(down.is_finite() && down > 0.0) {
                    return Err("The teardrop hem's angle leaves it no way back down to the sheet.".into());
                }
                Ok(vec![Bend { radius, angle }, Straight(down)])
            }
        }
    }
}

/// One of two sides, or both alike.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sides {
    #[default]
    Default,
    Flipped,
    Symmetric,
}

/// A strip of sheet that follows an open profile: lines and arcs drawn end to end in a sketch,
/// pushed out of the sketch plane. Corners between lines become bends; an arc is a bend as
/// drawn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContourFlange {
    pub sketch: FeatureId,
    /// How wide the strip is (how far the profile is pushed).
    pub distance: f64,
    /// In front of the sketch plane, behind it, or half each way.
    #[serde(default)]
    pub direction: Sides,
    /// Which side of the profile the sheet's thickness goes: the inside of its first corner
    /// (the profile is the outside of the part there), the outside, or half each way.
    #[serde(default)]
    pub position: Sides,
    /// The inside radius of the bends made at corners; without it, the part's `BendRadius`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    /// Join (to the sheet it touches) or a new solid.
    #[serde(default)]
    pub operation: Operation,
}

impl ContourFlange {
    pub fn check(&self) -> Result<(), String> {
        if !(self.distance.is_finite() && self.distance > 0.0 && self.distance <= 1e5) {
            return Err("the contour flange's distance must be more than 0".into());
        }
        if let Some(r) = self.radius
            && !(r.is_finite() && (MIN_BEND_RADIUS..=1000.0).contains(&r))
        {
            return Err(format!("the bend radius must be from {MIN_BEND_RADIUS} to 1000 mm"));
        }
        match self.operation {
            Operation::Join | Operation::NewBody => Ok(()),
            _ => Err("a contour flange is joined to the part or starts a new solid".into()),
        }
    }

    /// How far the sheet reaches to the left of the profile and to its right (negative), for a
    /// sheet `t` thick.
    pub fn reach(&self, t: f64) -> (f64, f64) {
        match self.position {
            Sides::Default => (0.0, t),
            Sides::Flipped => (-t, 0.0),
            Sides::Symmetric => (-t / 2.0, t / 2.0),
        }
    }
}

/// How closely the points of a curve that runs across a bend follow it in a flat pattern (mm).
pub const FLAT_DEFLECTION: f64 = 0.005;

/// The flat pattern of the part as `regen` has it: of solid `body` (the only one, when it is not
/// said), laid out from `face` (else the part's A side, else its largest flat face).
pub fn pattern_of(doc: &Document, regen: &crate::Regen, k: &dyn Kernel, body: Option<usize>, face: Option<&FaceRef>) -> Result<FlatPattern, String> {
    let sizes = doc.sheet_sizes()?;
    let bi = match (body, regen.bodies.len()) {
        (_, 0) => return Err("The part has no solid to lay flat yet: start with a Face.".into()),
        (Some(b), _) => b,
        (None, 1) => 0,
        (None, n) => return Err(format!("The part has {n} solids and a flat pattern is of one: combine them, or keep each in a part of its own.")),
    };
    let shape = regen.bodies.get(bi).ok_or("there is no such solid")?.shape;
    let base = match face.or(doc.sheet_metal().and_then(|r| r.a_side.as_ref())) {
        Some(f) => {
            let (b, i) = regen.resolve(f, k).map_err(|e| {
                format!("The face the flat pattern is laid out from (the A side) cannot be found: {e}. Choose another with Define A-Side.")
            })?;
            if b != bi {
                return Err("The face the flat pattern is laid out from is on another solid.".into());
            }
            Some(i)
        }
        None => None,
    };
    unfold(k, shape, &Unfold { thickness: sizes.thickness, k_factor: sizes.k_factor, base, deflection: FLAT_DEFLECTION })
}

/// How a sheet is laid flat by the part's rule.
pub(crate) fn unfold_opts(doc: &Document) -> Result<Unfold, String> {
    let sizes = doc.sheet_sizes()?;
    Ok(Unfold { thickness: sizes.thickness, k_factor: sizes.k_factor, base: None, deflection: FLAT_DEFLECTION })
}

/// The side of the sheet that sketch `sketch` (whose plane is `frame`) lies on, as `regen` has
/// the part: the solid and its face. That is the face the sketch was put on when it is one;
/// else, of the sides of the sheet in the sketch's plane, the one with most of `profile` on it,
/// or the nearest. None when the sketch's plane holds no side of the sheet.
pub(crate) fn seat_of(
    doc: &Document,
    regen: &crate::Regen,
    k: &dyn Kernel,
    sketch: FeatureId,
    frame: &tenon_geom::Frame,
    profile: &[tenon_kernel::Region],
) -> Result<Option<(usize, u32)>, String> {
    let opts = unfold_opts(doc)?;
    if let Some(crate::FeatureKind::Sketch { plane: crate::PlaneRef::Face(on), .. }) = doc.feature(sketch).map(|f| &f.kind)
        && let Ok((b, face)) = regen.resolve(on, k)
        && let Some(body) = regen.bodies.get(b)
        && wrap::is_seat(k, body.shape, face, frame, &opts)?
    {
        return Ok(Some((b, face)));
    }
    let mut best: Option<(usize, u32, f64)> = None;
    for (b, body) in regen.bodies.iter().enumerate() {
        if let Some((face, share)) = wrap::seat(k, body.shape, frame, profile, &opts)?
            && best.is_none_or(|(_, _, most)| share > most)
        {
            best = Some((b, face, share));
        }
    }
    Ok(best.map(|(b, face, _)| (b, face)))
}

/// Where edge `edge` of the part lies in sketch `sketch` with the sheet laid flat from the face
/// the sketch is on (Project Flat Pattern).
pub(crate) fn edge_laid_flat(
    doc: &Document,
    regen: &crate::Regen,
    k: &dyn Kernel,
    sketch: FeatureId,
    frame: &tenon_geom::Frame,
    edge: &EdgeRef,
) -> Result<crate::project::Projected, String> {
    if doc.sheet_metal().is_none() {
        return Err("the part is not sheet metal, so it has no flat pattern to show".into());
    }
    let (b, e) = regen.resolve_edge(edge, k)?;
    let (seat, base) =
        seat_of(doc, regen, k, sketch, frame, &[])?.ok_or("the sketch is not on a side of the sheet, so there is nothing to lay flat from")?;
    if seat != b {
        return Err("it is on another solid than the one the sketch is on".into());
    }
    let shape = regen.bodies.get(b).ok_or("no such body")?.shape;
    wrap::flat_edge(k, shape, base, frame, e, &unfold_opts(doc)?)
}

impl Document {
    /// The part's sheet metal rule, when it is a sheet metal part.
    pub fn sheet_metal(&self) -> Option<&SheetMetal> {
        self.sheet.as_deref()
    }
    pub fn sheet_metal_mut(&mut self) -> Option<&mut SheetMetal> {
        self.sheet.as_deref_mut()
    }

    /// Makes the part a sheet metal part (or keeps it one), with the rule given, and gives it
    /// whichever of the sheet metal parameters it does not have yet.
    pub fn set_sheet_metal(&mut self, rule: SheetMetal) -> Result<(), String> {
        for (name, equation, unit, comment) in PARAMETERS {
            if self.parameters().user.iter().any(|u| u.name == name) {
                continue;
            }
            if self.parameters().model.iter().any(|m| m.name == name) {
                return Err(format!("A dimension of the part is already called `{name}`: rename it, since sheet metal uses that name."));
            }
            self.add_user_parameter(UserParam { name: name.to_owned(), equation: equation.to_owned(), unit, comment: comment.to_owned() })?;
        }
        self.sheet = Some(Box::new(rule));
        Ok(())
    }

    /// The sizes of the sheet metal rule as the parameters now have them.
    pub fn sheet_sizes(&self) -> Result<Sizes, String> {
        if self.sheet.is_none() {
            return Err("This is not a sheet metal part yet: set its thickness in Sheet Metal Defaults first.".into());
        }
        Sizes::from_values(&self.parameter_values())
    }

    /// Whether `name` is a parameter the sheet metal rule needs: it cannot be renamed or deleted
    /// while the part is a sheet metal part.
    pub fn is_sheet_parameter(&self, name: &str) -> bool {
        self.sheet.is_some() && PARAMETERS.iter().any(|p| p.0 == name)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_part_becomes_sheet_metal_by_getting_the_rules_parameters() {
        let mut doc = Document::default();
        assert!(doc.sheet_sizes().unwrap_err().contains("not a sheet metal part"));
        doc.set_sheet_metal(SheetMetal::default()).unwrap();
        doc.sync_parameters().unwrap();
        let s = doc.sheet_sizes().unwrap();
        assert_eq!(
            (s.thickness, s.bend_radius, s.relief_width, s.relief_depth, s.min_remnant, s.corner_size, s.k_factor, s.gap),
            (1.0, 1.0, 1.0, 0.5, 2.0, 4.0, 0.44, 1.0)
        );
        // The sizes are parameters: the thickness changed, the rest follow by their equations.
        doc.set_equation(THICKNESS, Some("2 mm")).unwrap();
        doc.sync_parameters().unwrap();
        let s = doc.sheet_sizes().unwrap();
        assert_eq!((s.thickness, s.bend_radius, s.relief_depth, s.corner_size), (2.0, 2.0, 1.0, 8.0));
        // A radius of its own stays when the rule is set again; what cannot be is said.
        doc.set_equation(BEND_RADIUS, Some("3 mm")).unwrap();
        doc.set_sheet_metal(SheetMetal { corner: CornerShape::Square, ..Default::default() }).unwrap();
        doc.sync_parameters().unwrap();
        assert_eq!(doc.sheet_sizes().unwrap().bend_radius, 3.0);
        assert_eq!(doc.sheet_metal().unwrap().corner, CornerShape::Square);
        doc.set_equation(K_FACTOR, Some("1.5")).unwrap();
        doc.sync_parameters().unwrap();
        assert!(doc.sheet_sizes().unwrap_err().contains("KFactor must be from 0 to 1"));
        assert!(doc.is_sheet_parameter("Thickness") && !doc.is_sheet_parameter("d0"));
    }

    #[test]
    fn flanges_say_what_they_cannot_be() {
        let ok = Flange {
            edges: vec![],
            angle: std::f64::consts::FRAC_PI_2,
            height: 10.0,
            datum: HeightDatum::Outer,
            position: BendPosition::Inside,
            flip: false,
            radius: None,
            width: FlangeWidth::Edge,
            miter: true,
            corner: CornerOptions::default(),
        };
        assert!(ok.check().unwrap_err().contains("at least one edge"));
        let face = crate::naming::FaceOrigin::Cap { feature: FeatureId(1), end: crate::naming::CapEnd::End };
        let side = crate::naming::FaceOrigin::Side { feature: FeatureId(1), curve: tenon_sketch::EntityId(6) };
        let edge = EdgeRef::new(face, side, crate::naming::EdgeFingerprint { mid: tenon_geom::Vec3::ZERO, length: 1.0 });
        let ok = Flange { edges: vec![edge], ..ok };
        ok.check().unwrap();
        assert!(Flange { angle: 0.0, ..ok.clone() }.check().unwrap_err().contains("1 to 179"));
        assert!(Flange { angle: std::f64::consts::PI, ..ok.clone() }.check().unwrap_err().contains("1 to 179"));
        assert!(Flange { height: 0.0, ..ok.clone() }.check().unwrap_err().contains("height"));
        assert!(Flange { radius: Some(0.0), ..ok.clone() }.check().unwrap_err().contains("bend radius"));
        assert!(Flange { width: FlangeWidth::Centered { width: -1.0 }, ..ok.clone() }.check().unwrap_err().contains("width"));
        assert!(Flange { width: FlangeWidth::Offset { start: -1.0, end: 0.0 }, ..ok }.check().unwrap_err().contains("offsets"));
    }
}

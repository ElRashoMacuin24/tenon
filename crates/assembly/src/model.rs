//! The assembly document: components (placed part files), the relationships between them
//! (constraints and joints), the patterns and mirrors that repeat components, and the steps of
//! the exploded view. Saved as `.tenonasm` (docs/file-format.md, DEC-024, DEC-130).

use serde::{Deserialize, Serialize};
use tenon_geom::{Frame, Vec3, tol};
use tenon_model::{EdgeRef, FaceRef, FeatureId, OriginAxis, OriginPlane};

/// Most components one assembly may have.
pub const MAX_COMPONENTS: usize = 5_000;
/// Most relationships one assembly may have.
pub const MAX_RELATIONSHIPS: usize = 20_000;
/// Most exploded-view steps.
pub const MAX_TWEAKS: usize = 5_000;
/// Most patterns and mirrors one assembly may have.
pub const MAX_PATTERNS: usize = 5_000;
/// Most occurrences a pattern may have in one direction (hostile-input cap: an assembly holds
/// [`MAX_COMPONENTS`] in all).
pub const MAX_COUNT: u32 = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ComponentId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RelationshipId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PatternId(pub u32);

impl std::fmt::Display for PatternId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pattern {}", self.0)
    }
}

impl std::fmt::Display for ComponentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "component {}", self.0)
    }
}

impl std::fmt::Display for RelationshipId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "relationship {}", self.0)
    }
}

fn yes() -> bool {
    true
}

/// One placed occurrence of a part file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub id: ComponentId,
    /// "Bracket:1": the part's name and an occurrence number.
    pub name: String,
    /// The part file. In memory the full path; in the file relative to the assembly's folder.
    pub part: String,
    /// Part coordinates to assembly coordinates.
    pub placement: Frame,
    /// A grounded component never moves.
    #[serde(default)]
    pub grounded: bool,
    /// The row of the part's design table this component uses: the part in that size. Without
    /// one, the part as its file has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<String>,
    #[serde(default = "yes")]
    pub visible: bool,
    /// The component holds the mirror image of its part (in the part's XY plane), not the part:
    /// a mirrored occurrence of the part file (DEC-130). `placement` stays a rigid frame.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mirrored: bool,
}

/// The key of a part among an assembly's loaded parts: its file's path, and for a part in one
/// row of its design table, the path and the row (parted by a character no path can hold).
pub fn part_key(part: &str, row: Option<&str>) -> String {
    match row {
        Some(row) => format!("{part}\0{row}"),
        None => part.to_owned(),
    }
}

/// The key of a part as a mirrored occurrence holds it, the key's row empty when it has none (a
/// row is never empty, and holds no control character: the two cannot be taken for each other).
pub fn mirrored_key(part: &str, row: Option<&str>) -> String {
    format!("{part}\0{}\0mirrored", row.unwrap_or(""))
}

/// The file of a part key (see [`part_key`]).
pub fn key_file(key: &str) -> &str {
    key.split('\0').next().unwrap_or(key)
}

impl Component {
    /// The key of this component's part among the assembly's loaded parts.
    pub fn key(&self) -> String {
        if self.mirrored { mirrored_key(&self.part, self.row.as_deref()) } else { part_key(&self.part, self.row.as_deref()) }
    }
}

/// Geometry of a part that a relationship refers to, in the part's coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Geom {
    /// A face: a planar face gives its plane, a cylinder or cone its axis, a sphere its centre.
    Face { face: FaceRef },
    /// An edge: a straight edge gives its line, a circle its centre and axis.
    Edge { edge: EdgeRef },
    /// An origin plane.
    Plane { plane: OriginPlane },
    /// An origin axis.
    Axis { axis: OriginAxis },
    /// The origin point.
    Origin,
    /// A work plane, axis or point of the part.
    Work { feature: FeatureId },
}

/// Geometry of a component, or of the assembly itself (its origin planes, axes and point) when
/// `component` is absent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<ComponentId>,
    pub geom: Geom,
}

/// The joint types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JointKind {
    /// No motion.
    Rigid,
    /// Turns about the joint axis.
    Revolute,
    /// Slides along the joint axis.
    Slider,
    /// Turns about and slides along the axis.
    Cylindrical,
    /// Slides and turns on the joint plane.
    Planar,
    /// Turns about the joint point.
    Ball,
}

impl JointKind {
    pub const ALL: [JointKind; 6] =
        [JointKind::Rigid, JointKind::Revolute, JointKind::Slider, JointKind::Cylindrical, JointKind::Planar, JointKind::Ball];

    pub fn label(self) -> &'static str {
        match self {
            JointKind::Rigid => "Rigid",
            JointKind::Revolute => "Rotational",
            JointKind::Slider => "Slider",
            JointKind::Cylindrical => "Cylindrical",
            JointKind::Planar => "Planar",
            JointKind::Ball => "Ball",
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            JointKind::Rigid => "rigid",
            JointKind::Revolute => "revolute",
            JointKind::Slider => "slider",
            JointKind::Cylindrical => "cylindrical",
            JointKind::Planar => "planar",
            JointKind::Ball => "ball",
        }
    }
    pub fn from_id(s: &str) -> Option<JointKind> {
        JointKind::ALL.into_iter().find(|k| k.id() == s || k.label().eq_ignore_ascii_case(s))
    }
    /// Degrees of freedom the joint leaves between its two components.
    pub fn freedom(self) -> usize {
        match self {
            JointKind::Rigid => 0,
            JointKind::Revolute | JointKind::Slider => 1,
            JointKind::Cylindrical => 2,
            JointKind::Planar | JointKind::Ball => 3,
        }
    }
}

/// What a relationship holds together.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RelKind {
    /// Faces against each other: planes facing opposite ways `offset` apart, axes in line,
    /// points together, a point or line on a plane, a point on an axis.
    Mate {
        a: Target,
        b: Target,
        #[serde(default)]
        offset: f64,
    },
    /// Planes side by side, facing the same way, `offset` apart.
    Flush {
        a: Target,
        b: Target,
        #[serde(default)]
        offset: f64,
    },
    /// The angle (radians) from A's plane normal or line to B's, turning about `reference`, a
    /// direction in A's coordinates fixed when the relationship was made.
    Angle { a: Target, b: Target, angle: f64, reference: Vec3 },
    /// Circular edges: axes in line, the circles' planes facing each other `offset` apart
    /// (`aligned`: facing the same way).
    Insert {
        a: Target,
        b: Target,
        #[serde(default)]
        offset: f64,
        #[serde(default)]
        aligned: bool,
    },
    /// A round face against a plane or another round face (DEC-110): a cylinder's or sphere's
    /// side touching a plane (`inside`: on its far side), or two touching each other, outside or
    /// one inside the other, `offset` apart.
    Tangent {
        a: Target,
        b: Target,
        #[serde(default)]
        offset: f64,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        inside: bool,
    },
    /// A and B mirror images of each other in `plane` (DEC-115): two planes, axes or points.
    Symmetry { a: Target, b: Target, plane: Target },
    /// A joint between origins on A and B. The origins' Z axes face each other (`flip`: the same
    /// way), `offset` apart along A's Z; rigid and slider joints also turn B by `angle` about it.
    Joint {
        joint: JointKind,
        a: Target,
        b: Target,
        #[serde(default)]
        flip: bool,
        #[serde(default)]
        offset: f64,
        #[serde(default)]
        angle: f64,
    },
}

impl RelKind {
    /// The same relationship between the components `map` says stand for those it joins, if it
    /// is all between those: it touches no other component and not the assembly's own origin.
    pub fn between(&self, map: &std::collections::BTreeMap<ComponentId, ComponentId>) -> Option<RelKind> {
        if self.all_targets().iter().any(|t| t.component.is_none_or(|c| !map.contains_key(&c))) {
            return None;
        }
        let mut kind = self.clone();
        let mapped = |t: &mut Target| t.component = t.component.and_then(|c| map.get(&c).copied());
        for t in kind.targets_mut() {
            mapped(t);
        }
        if let RelKind::Symmetry { plane, .. } = &mut kind {
            mapped(plane);
        }
        Some(kind)
    }
    pub fn targets(&self) -> [&Target; 2] {
        match self {
            RelKind::Mate { a, b, .. }
            | RelKind::Flush { a, b, .. }
            | RelKind::Angle { a, b, .. }
            | RelKind::Insert { a, b, .. }
            | RelKind::Tangent { a, b, .. }
            | RelKind::Symmetry { a, b, .. }
            | RelKind::Joint { a, b, .. } => [a, b],
        }
    }
    /// A symmetry's plane: the third thing it is between.
    pub fn plane(&self) -> Option<&Target> {
        match self {
            RelKind::Symmetry { plane, .. } => Some(plane),
            _ => None,
        }
    }
    /// Every target, a symmetry's plane included.
    pub fn all_targets(&self) -> Vec<&Target> {
        let [a, b] = self.targets();
        [a, b].into_iter().chain(self.plane()).collect()
    }
    fn targets_mut(&mut self) -> [&mut Target; 2] {
        match self {
            RelKind::Mate { a, b, .. }
            | RelKind::Flush { a, b, .. }
            | RelKind::Angle { a, b, .. }
            | RelKind::Insert { a, b, .. }
            | RelKind::Tangent { a, b, .. }
            | RelKind::Symmetry { a, b, .. }
            | RelKind::Joint { a, b, .. } => [a, b],
        }
    }
    /// The components it connects (the assembly origin is not one).
    pub fn components(&self) -> Vec<ComponentId> {
        let mut v: Vec<ComponentId> = self.all_targets().iter().filter_map(|t| t.component).collect();
        v.dedup();
        v
    }
    /// "Mate", "Flush", "Angle", "Insert" or the joint's name.
    pub fn label(&self) -> &'static str {
        match self {
            RelKind::Mate { .. } => "Mate",
            RelKind::Flush { .. } => "Flush",
            RelKind::Angle { .. } => "Angle",
            RelKind::Insert { .. } => "Insert",
            RelKind::Tangent { .. } => "Tangent",
            RelKind::Symmetry { .. } => "Symmetry",
            RelKind::Joint { joint, .. } => joint.label(),
        }
    }
    pub fn is_joint(&self) -> bool {
        matches!(self, RelKind::Joint { .. })
    }
}

/// One direction of a rectangular pattern.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// What gives the direction: a straight edge or an axis, a flat face's normal, a circular
    /// edge's axis.
    pub along: Target,
    /// The other way along it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reverse: bool,
    /// How many, the source counted.
    pub count: u32,
    /// From one to the next (mm).
    pub spacing: f64,
}

/// A source of a mirror placed again in the mirrored position, not reflected: for a part that is
/// the same as its mirror image.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reused {
    pub component: ComponentId,
    /// Square to the part's own plane of symmetry, which goes through the middle of its box.
    pub about: OriginAxis,
}

/// The rule that says where the elements of a pattern stand.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PatternKind {
    /// Rows along one direction, or rows of rows along two.
    Rectangular {
        first: Run,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        second: Option<Run>,
    },
    /// Round an axis (a straight edge, a round face's or a circular edge's axis). `angle` is
    /// over how much of a turn they spread evenly, as in a part's circular pattern: a whole turn
    /// has them `1 / count` of it apart, less than that from the first to the last.
    Circular {
        axis: Target,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reverse: bool,
        count: u32,
        angle: f64,
    },
    /// Reflected in a plane or flat face (DEC-130): each source a mirrored occurrence, but for
    /// those in `reuse`.
    Mirror {
        plane: Target,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        reuse: Vec<Reused>,
    },
}

impl PatternKind {
    /// Where each element stands in the pattern. [0, 0], the sources themselves, is not one.
    pub fn places(&self) -> Vec<[u32; 2]> {
        match self {
            PatternKind::Rectangular { first, second } => {
                let rows = second.as_ref().map_or(1, |s| s.count);
                (0..rows).flat_map(|j| (0..first.count).map(move |i| [i, j])).filter(|at| *at != [0, 0]).collect()
            }
            PatternKind::Circular { count, .. } => (1..*count).map(|i| [i, 0]).collect(),
            PatternKind::Mirror { .. } => vec![[1, 0]],
        }
    }

    /// What the rule takes from the assembly: its directions, axis or plane.
    pub fn targets(&self) -> Vec<&Target> {
        match self {
            PatternKind::Rectangular { first, second } => std::iter::once(&first.along).chain(second.as_ref().map(|s| &s.along)).collect(),
            PatternKind::Circular { axis, .. } => vec![axis],
            PatternKind::Mirror { plane, .. } => vec![plane],
        }
    }

    fn targets_mut(&mut self) -> Vec<&mut Target> {
        match self {
            PatternKind::Rectangular { first, second } => std::iter::once(&mut first.along).chain(second.as_mut().map(|s| &mut s.along)).collect(),
            PatternKind::Circular { axis, .. } => vec![axis],
            PatternKind::Mirror { plane, .. } => vec![plane],
        }
    }

    /// "Pattern" or "Mirror": the record's name, before its number.
    pub fn label(&self) -> &'static str {
        match self {
            PatternKind::Mirror { .. } => "Mirror",
            _ => "Pattern",
        }
    }

    /// What is wrong with its numbers, in words.
    pub fn check(&self) -> Result<(), String> {
        let count_ok = |count: u32| (1..=MAX_COUNT).contains(&count);
        match self {
            PatternKind::Rectangular { first, second } => {
                for run in std::iter::once(first).chain(second) {
                    if !count_ok(run.count) {
                        return Err(format!("the counts must be 1 to {MAX_COUNT}"));
                    }
                    if !run.spacing.is_finite() || run.spacing.abs() > tol::MAX_SIZE || (run.count > 1 && run.spacing <= 0.0) {
                        return Err("the spacing must be positive".into());
                    }
                }
                let all = first.count.saturating_mul(second.as_ref().map_or(1, |s| s.count));
                if all < 2 {
                    return Err("a pattern needs at least two occurrences".into());
                }
                if all as usize > MAX_COMPONENTS {
                    return Err(format!("that would make more than {MAX_COMPONENTS} components"));
                }
            }
            PatternKind::Circular { count, angle, .. } => {
                if *count < 2 || !count_ok(*count) {
                    return Err(format!("the count must be 2 to {MAX_COUNT}"));
                }
                if !(angle.is_finite() && *angle > 0.0 && *angle <= std::f64::consts::TAU + 1e-9) {
                    return Err("the angle must be more than 0 and at most 360 degrees".into());
                }
            }
            PatternKind::Mirror { .. } => {}
        }
        Ok(())
    }
}

/// One element of a pattern: what stands for its sources at one place of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Element {
    /// Its place along the first direction and along the second (a circular pattern's and a
    /// mirror's second is 0): what keeps it the same element when the counts change.
    pub at: [u32; 2],
    /// A component for each source, in the order of the sources.
    pub components: Vec<ComponentId>,
}

/// Components repeated by a rule, or reflected: one record (DEC-130). Each element's components
/// are ordinary components of the assembly, drawn, listed, checked and exported as any other, and
/// held by the rule: they stand where it puts them and follow what it depends on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub id: PatternId,
    /// "Pattern:1", "Mirror:2", ...
    pub name: String,
    /// Its elements are out of the assembly (hidden, and not in the parts list) until it is
    /// brought back; the sources stay.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
    /// The components repeated.
    pub sources: Vec<ComponentId>,
    pub kind: PatternKind,
    pub elements: Vec<Element>,
}

/// A constraint or joint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub id: RelationshipId,
    /// "Mate:1", "Rigid:2", ...
    pub name: String,
    #[serde(default)]
    pub suppressed: bool,
    pub kind: RelKind,
}

/// One step of the exploded view: components moved `distance` along `direction` (assembly
/// coordinates, a unit vector).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tweak {
    pub components: Vec<ComponentId>,
    pub direction: Vec3,
    pub distance: f64,
}

/// An assembly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Assembly {
    pub name: String,
    pub components: Vec<Component>,
    pub relationships: Vec<Relationship>,
    /// Patterns and mirrors (DEC-130). A later one may repeat the elements of an earlier one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<Pattern>,
    /// The exploded view, step by step.
    #[serde(default)]
    pub explode: Vec<Tweak>,
    pub next_component: u32,
    pub next_relationship: u32,
    #[serde(default = "first_pattern", skip_serializing_if = "is_first_pattern")]
    pub next_pattern: u32,
}

fn first_pattern() -> u32 {
    1
}

fn is_first_pattern(n: &u32) -> bool {
    *n == 1
}

impl Default for Assembly {
    fn default() -> Self {
        Assembly {
            name: "Assembly1".into(),
            components: Vec::new(),
            relationships: Vec::new(),
            patterns: Vec::new(),
            explode: Vec::new(),
            next_component: 1,
            next_relationship: 1,
            next_pattern: 1,
        }
    }
}

fn finite(v: Vec3) -> bool {
    v.is_finite() && v.x.abs() <= tol::MAX_SIZE && v.y.abs() <= tol::MAX_SIZE && v.z.abs() <= tol::MAX_SIZE
}

/// Whether a stored frame is a usable placement: finite and orthonormal.
fn frame_ok(f: &Frame) -> bool {
    let unit = |v: Vec3| (v.len() - 1.0).abs() < tol::UNIT;
    finite(f.origin())
        && [f.x(), f.y(), f.z()].iter().all(|v| v.is_finite() && unit(*v))
        && f.x().dot(f.y()).abs() < tol::UNIT
        && f.x().cross(f.y()).near(f.z(), tol::UNIT)
}

impl Assembly {
    /// Where the component `id` is among the components. They are kept in the order they were
    /// placed, which is the order of their ids, so it is looked for by halving first: every
    /// body drawn asks for its component every frame, and looking through all of them for
    /// each made a frame of a thousand components cost a million comparisons.
    fn component_at(&self, id: ComponentId) -> Option<usize> {
        match self.components.binary_search_by_key(&id, |c| c.id) {
            Ok(i) => Some(i),
            // (Not found so: not there, or the components are not in the order of their ids.)
            Err(_) => self.components.iter().position(|c| c.id == id),
        }
    }
    pub fn component(&self, id: ComponentId) -> Option<&Component> {
        self.component_at(id).and_then(|i| self.components.get(i))
    }
    pub fn component_mut(&mut self, id: ComponentId) -> Option<&mut Component> {
        self.component_at(id).and_then(|i| self.components.get_mut(i))
    }
    pub fn relationship(&self, id: RelationshipId) -> Option<&Relationship> {
        self.relationships.iter().find(|r| r.id == id)
    }
    pub fn relationship_mut(&mut self, id: RelationshipId) -> Option<&mut Relationship> {
        self.relationships.iter_mut().find(|r| r.id == id)
    }
    /// Relationships that involve `id`.
    pub fn relationships_of(&self, id: ComponentId) -> impl Iterator<Item = &Relationship> {
        self.relationships.iter().filter(move |r| r.kind.components().contains(&id))
    }

    /// A new component id.
    pub fn take_component_id(&mut self) -> ComponentId {
        let id = ComponentId(self.next_component);
        self.next_component = self.next_component.saturating_add(1);
        id
    }
    /// A new relationship id.
    pub fn take_relationship_id(&mut self) -> RelationshipId {
        let id = RelationshipId(self.next_relationship);
        self.next_relationship = self.next_relationship.saturating_add(1);
        id
    }

    /// "Base:3" for the next occurrence of a part named `base`.
    pub fn occurrence_name(&self, base: &str) -> String {
        let n = self
            .components
            .iter()
            .filter_map(|c| c.name.rsplit_once(':').filter(|(b, _)| *b == base).and_then(|(_, n)| n.parse::<u32>().ok()))
            .max()
            .unwrap_or(0);
        format!("{base}:{}", n.saturating_add(1))
    }
    /// "Mate:2" for the next relationship labelled `label`.
    pub fn relationship_name(&self, label: &str) -> String {
        let n = self
            .relationships
            .iter()
            .filter_map(|r| r.name.rsplit_once(':').filter(|(b, _)| *b == label).and_then(|(_, n)| n.parse::<u32>().ok()))
            .max()
            .unwrap_or(0);
        format!("{label}:{}", n.saturating_add(1))
    }

    /// The pattern or mirror at `id`.
    pub fn pattern(&self, id: PatternId) -> Option<&Pattern> {
        self.patterns.iter().find(|p| p.id == id)
    }
    pub fn pattern_mut(&mut self, id: PatternId) -> Option<&mut Pattern> {
        self.patterns.iter_mut().find(|p| p.id == id)
    }
    /// The pattern a component is an element of. (The sources are not elements.)
    pub fn pattern_of(&self, id: ComponentId) -> Option<&Pattern> {
        self.patterns.iter().find(|p| p.elements.iter().any(|e| e.components.contains(&id)))
    }
    /// A new pattern id.
    pub fn take_pattern_id(&mut self) -> PatternId {
        let id = PatternId(self.next_pattern);
        self.next_pattern = self.next_pattern.saturating_add(1);
        id
    }
    /// "Pattern:2" for the next pattern labelled `label`.
    pub fn pattern_name(&self, label: &str) -> String {
        let n = self
            .patterns
            .iter()
            .filter_map(|p| p.name.rsplit_once(':').filter(|(b, _)| *b == label).and_then(|(_, n)| n.parse::<u32>().ok()))
            .max()
            .unwrap_or(0);
        format!("{label}:{}", n.saturating_add(1))
    }
    /// Every component that is an element of a pattern.
    pub fn elements(&self) -> std::collections::BTreeSet<ComponentId> {
        self.patterns.iter().flat_map(|p| p.elements.iter()).flat_map(|e| e.components.iter().copied()).collect()
    }
    /// The elements of suppressed patterns: components that are out of the assembly for now.
    pub fn suppressed_elements(&self) -> std::collections::BTreeSet<ComponentId> {
        self.patterns.iter().filter(|p| p.suppressed).flat_map(|p| p.elements.iter()).flat_map(|e| e.components.iter().copied()).collect()
    }

    /// Removes a component with its relationships and exploded-view steps, and with what stands
    /// for it in a pattern. A pattern with nothing left to repeat goes, and so does one whose
    /// rule took its direction, axis or plane from the component, or that has lost an element
    /// (its counts cannot say that): the elements it had left stay, as components of their own.
    pub fn remove_component(&mut self, id: ComponentId) -> bool {
        let existed = self.component(id).is_some();
        let mut going = vec![id];
        while let Some(id) = going.pop() {
            if self.component(id).is_none() {
                continue;
            }
            self.components.retain(|c| c.id != id);
            self.relationships.retain(|r| !r.kind.components().contains(&id));
            for t in &mut self.explode {
                t.components.retain(|c| *c != id);
            }
            self.explode.retain(|t| !t.components.is_empty());
            for p in &mut self.patterns {
                if let Some(i) = p.sources.iter().position(|s| *s == id) {
                    p.sources.remove(i);
                    for e in &mut p.elements {
                        if i < e.components.len() {
                            going.push(e.components.remove(i));
                        }
                    }
                    if let PatternKind::Mirror { reuse, .. } = &mut p.kind {
                        reuse.retain(|r| r.component != id);
                    }
                }
            }
            let (broken, whole): (Vec<Pattern>, Vec<Pattern>) = std::mem::take(&mut self.patterns)
                .into_iter()
                .partition(|p| p.kind.targets().iter().any(|t| t.component == Some(id)) || p.elements.iter().any(|e| e.components.contains(&id)));
            self.patterns = whole;
            self.patterns.retain(|p| !p.sources.is_empty());
            for c in broken.iter().filter(|p| p.suppressed).flat_map(|p| p.elements.iter()).flat_map(|e| e.components.iter()) {
                if let Some(c) = self.component_mut(*c) {
                    c.visible = true;
                }
            }
        }
        existed
    }

    /// Takes a pattern out of the assembly, and its elements with it unless `keep` leaves them
    /// as components of their own, where they stand.
    pub fn remove_pattern(&mut self, id: PatternId, keep: bool) -> bool {
        let Some(i) = self.patterns.iter().position(|p| p.id == id) else { return false };
        let p = self.patterns.remove(i);
        for c in p.elements.iter().flat_map(|e| e.components.iter()) {
            if !keep {
                self.remove_component(*c);
            } else if let Some(c) = self.component_mut(*c)
                && p.suppressed
            {
                c.visible = true;
            }
        }
        true
    }

    /// Rewrites every component's part path (used to store paths relative to the file and read
    /// them back).
    pub fn map_parts(&mut self, f: impl Fn(&str) -> String) {
        for c in &mut self.components {
            c.part = f(&c.part);
        }
    }

    /// Checks a document read from a file (or built by commands): ids, references, numbers.
    pub fn validate(&self) -> Result<(), String> {
        if self.components.len() > MAX_COMPONENTS {
            return Err(format!("more than {MAX_COMPONENTS} components"));
        }
        if self.relationships.len() > MAX_RELATIONSHIPS {
            return Err(format!("more than {MAX_RELATIONSHIPS} relationships"));
        }
        if self.explode.len() > MAX_TWEAKS {
            return Err(format!("more than {MAX_TWEAKS} exploded-view steps"));
        }
        if self.patterns.len() > MAX_PATTERNS {
            return Err(format!("more than {MAX_PATTERNS} patterns"));
        }
        let mut ids = std::collections::BTreeSet::new();
        for c in &self.components {
            if c.id.0 == 0 || c.id.0 >= self.next_component || !ids.insert(c.id) {
                return Err(format!("{} has a bad or repeated id", c.id));
            }
            if c.part.is_empty() || c.part.len() > 4096 || c.part.contains('\0') {
                return Err(format!("{} has a bad part path", c.name));
            }
            if c.row.as_ref().is_some_and(|r| r.is_empty() || r.chars().count() > 64 || r.chars().any(char::is_control)) {
                return Err(format!("{} names a bad design table row", c.name));
            }
            if c.name.len() > 256 {
                return Err(format!("{} has too long a name", c.id));
            }
            if !frame_ok(&c.placement) {
                return Err(format!("{} has a bad placement", c.name));
            }
        }
        let mut rids = std::collections::BTreeSet::new();
        for r in &self.relationships {
            if r.id.0 == 0 || r.id.0 >= self.next_relationship || !rids.insert(r.id) {
                return Err(format!("{} has a bad or repeated id", r.id));
            }
            for t in r.kind.all_targets() {
                match t.component {
                    Some(c) if !ids.contains(&c) => return Err(format!("{} refers to a missing {c}", r.name)),
                    None if !matches!(t.geom, Geom::Plane { .. } | Geom::Axis { .. } | Geom::Origin) => {
                        return Err(format!("{}: the assembly origin has only planes, axes and a point", r.name));
                    }
                    _ => {}
                }
            }
            let [a, b] = r.kind.targets();
            if a.component.is_some() && a.component == b.component {
                return Err(format!("{} joins a component to itself", r.name));
            }
            let numbers_ok = match &r.kind {
                RelKind::Mate { offset, .. } | RelKind::Flush { offset, .. } | RelKind::Insert { offset, .. } | RelKind::Tangent { offset, .. } => {
                    offset.abs() <= tol::MAX_SIZE
                }
                RelKind::Angle { angle, reference, .. } => angle.is_finite() && (reference.len() - 1.0).abs() < tol::UNIT,
                RelKind::Symmetry { .. } => true,
                RelKind::Joint { offset, angle, .. } => offset.abs() <= tol::MAX_SIZE && angle.is_finite(),
            };
            if !numbers_ok {
                return Err(format!("{} has a value out of range", r.name));
            }
        }
        for t in &self.explode {
            if !finite(t.direction) || (t.direction.len() - 1.0).abs() > tol::UNIT || t.distance.abs() > tol::MAX_SIZE || !t.distance.is_finite() {
                return Err("an exploded-view step has a bad direction or distance".into());
            }
            if let Some(c) = t.components.iter().find(|c| !ids.contains(c)) {
                return Err(format!("an exploded-view step moves a missing {c}"));
            }
        }
        self.validate_patterns(&ids)
    }

    /// Checks the patterns: ids, what they refer to, numbers. A pattern may lean on the elements
    /// of those before it and on no others, so that they can be worked out in order.
    fn validate_patterns(&self, ids: &std::collections::BTreeSet<ComponentId>) -> Result<(), String> {
        let mut pids = std::collections::BTreeSet::new();
        let mut taken = std::collections::BTreeSet::new();
        let all = self.elements();
        for p in &self.patterns {
            if p.id.0 == 0 || p.id.0 >= self.next_pattern || !pids.insert(p.id) {
                return Err(format!("{} has a bad or repeated id", p.id));
            }
            if p.name.is_empty() || p.name.len() > 256 {
                return Err(format!("{} has a bad name", p.id));
            }
            p.kind.check().map_err(|e| format!("{}: {e}", p.name))?;
            if p.sources.is_empty() {
                return Err(format!("{} repeats nothing", p.name));
            }
            let places = p.kind.places();
            if p.elements.len() != places.len() || places.iter().any(|at| !p.elements.iter().any(|e| e.at == *at)) {
                return Err(format!("{} does not have the elements its counts say", p.name));
            }
            let mut mine = std::collections::BTreeSet::new();
            for e in &p.elements {
                if e.components.len() != p.sources.len() {
                    return Err(format!("{} has an element that does not stand for all its sources", p.name));
                }
                for c in &e.components {
                    if !ids.contains(c) {
                        return Err(format!("{} refers to a missing {c}", p.name));
                    }
                    if !mine.insert(*c) || taken.contains(c) {
                        return Err(format!("{c} is an element of more than one place"));
                    }
                }
            }
            let mut sources = std::collections::BTreeSet::new();
            for s in &p.sources {
                if !ids.contains(s) {
                    return Err(format!("{} refers to a missing {s}", p.name));
                }
                if !sources.insert(*s) {
                    return Err(format!("{} repeats {s} twice", p.name));
                }
            }
            // (Those of the patterns after it, and its own, are not to lean on.)
            let later = |c: ComponentId| mine.contains(&c) || (all.contains(&c) && !taken.contains(&c));
            if let Some(s) = p.sources.iter().find(|s| later(**s)) {
                return Err(format!("{} repeats {s}, which depends on it", p.name));
            }
            for t in p.kind.targets() {
                match t.component {
                    Some(c) if !ids.contains(&c) => return Err(format!("{} refers to a missing {c}", p.name)),
                    Some(c) if later(c) => return Err(format!("{} takes its rule from {c}, which depends on it", p.name)),
                    None if !matches!(t.geom, Geom::Plane { .. } | Geom::Axis { .. } | Geom::Origin) => {
                        return Err(format!("{}: the assembly origin has only planes, axes and a point", p.name));
                    }
                    _ => {}
                }
            }
            if let PatternKind::Mirror { reuse, .. } = &p.kind
                && let Some(r) = reuse.iter().find(|r| !sources.contains(&r.component))
            {
                return Err(format!("{}: {} is not among what it mirrors", p.name, r.component));
            }
            taken.extend(mine);
        }
        Ok(())
    }

    /// Fixes small drift in stored placements (re-orthonormalises them).
    pub fn tidy(&mut self) {
        for c in &mut self.components {
            if let Some(f) = Frame::new(c.placement.origin(), c.placement.z(), c.placement.x()) {
                c.placement = f;
            }
        }
    }

    /// Target references to `from` now point to `to` (a component replaced by another).
    pub fn retarget(&mut self, from: ComponentId, to: ComponentId) {
        for r in &mut self.relationships {
            for t in r.kind.targets_mut() {
                if t.component == Some(from) {
                    t.component = Some(to);
                }
            }
            if let RelKind::Symmetry { plane, .. } = &mut r.kind
                && plane.component == Some(from)
            {
                plane.component = Some(to);
            }
        }
        for p in &mut self.patterns {
            for t in p.kind.targets_mut() {
                if t.component == Some(from) {
                    t.component = Some(to);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn comp(asm: &mut Assembly, name: &str) -> ComponentId {
        let id = asm.take_component_id();
        let name = asm.occurrence_name(name);
        asm.components.push(Component {
            id,
            name,
            part: "p.tenon".into(),
            placement: Frame::WORLD,
            grounded: false,
            row: None,
            visible: true,
            mirrored: false,
        });
        id
    }

    #[test]
    fn names_count_occurrences_and_validation_catches_damage() {
        let mut asm = Assembly::default();
        let a = comp(&mut asm, "Plate");
        let b = comp(&mut asm, "Plate");
        let c = comp(&mut asm, "Pin");
        assert_eq!(asm.component(b).unwrap().name, "Plate:2");
        assert_eq!(asm.component(c).unwrap().name, "Pin:1");
        let id = asm.take_relationship_id();
        let name = asm.relationship_name("Mate");
        let origin = |component| Target { component, geom: Geom::Plane { plane: OriginPlane::XY } };
        asm.relationships.push(Relationship {
            id,
            name,
            suppressed: false,
            kind: RelKind::Mate { a: origin(Some(a)), b: origin(Some(b)), offset: 0.0 },
        });
        assert_eq!(asm.relationship_name("Mate"), "Mate:2");
        asm.validate().unwrap();

        let json = serde_json::to_string(&asm).unwrap();
        let back: Assembly = serde_json::from_str(&json).unwrap();
        assert_eq!(back, asm);

        let mut bad = asm.clone();
        bad.relationships[0].kind = RelKind::Mate { a: origin(Some(a)), b: origin(Some(a)), offset: 0.0 };
        assert!(bad.validate().unwrap_err().contains("itself"));
        let mut bad = asm.clone();
        bad.relationships[0].kind = RelKind::Mate { a: origin(Some(ComponentId(99))), b: origin(None), offset: 0.0 };
        assert!(bad.validate().unwrap_err().contains("missing"));
        let mut bad = asm.clone();
        bad.relationships[0].kind =
            RelKind::Mate { a: Target { component: None, geom: Geom::Work { feature: FeatureId(3) } }, b: origin(Some(a)), offset: 0.0 };
        assert!(bad.validate().unwrap_err().contains("origin"));
        let mut bad = asm.clone();
        bad.components[1].id = a;
        assert!(bad.validate().unwrap_err().contains("repeated"));
        let mut bad: serde_json::Value = serde_json::to_value(&asm).unwrap();
        bad["components"][0]["placement"]["x"]["x"] = serde_json::json!(2.0);
        let bad: Assembly = serde_json::from_value(bad).unwrap();
        assert!(bad.validate().unwrap_err().contains("placement"));

        // Removing a component takes its relationships with it.
        assert!(asm.remove_component(b));
        assert!(asm.relationships.is_empty());
        assert!(!asm.remove_component(b));
    }

    /// An assembly of a source and a pattern of it along the assembly's X axis.
    fn patterned(count: u32) -> (Assembly, ComponentId, PatternId) {
        let mut asm = Assembly::default();
        let source = comp(&mut asm, "Plate");
        let elements: Vec<Element> = (1..count)
            .map(|i| {
                let c = comp(&mut asm, "Plate");
                Element { at: [i, 0], components: vec![c] }
            })
            .collect();
        let id = asm.take_pattern_id();
        let along = Target { component: None, geom: Geom::Axis { axis: OriginAxis::X } };
        asm.patterns.push(Pattern {
            id,
            name: "Pattern:1".into(),
            suppressed: false,
            sources: vec![source],
            kind: PatternKind::Rectangular { first: Run { along, reverse: false, count, spacing: 10.0 }, second: None },
            elements,
        });
        (asm, source, id)
    }

    #[test]
    fn a_pattern_is_checked_as_a_record_and_its_elements_go_with_it() {
        let (asm, source, id) = patterned(3);
        asm.validate().unwrap();
        assert_eq!(asm.pattern_name("Pattern"), "Pattern:2");
        assert_eq!(asm.pattern_name("Mirror"), "Mirror:1");
        assert_eq!(asm.elements().len(), 2);
        assert_eq!(asm.pattern_of(ComponentId(2)).map(|p| p.id), Some(id));
        assert!(asm.pattern_of(source).is_none(), "a source is not an element");

        // Damage is told in words, and a file with it is refused.
        let mut bad = asm.clone();
        bad.patterns[0].elements.pop();
        assert!(bad.validate().unwrap_err().contains("counts"), "{:?}", bad.validate());
        let mut bad = asm.clone();
        bad.patterns[0].sources[0] = ComponentId(99);
        assert!(bad.validate().unwrap_err().contains("missing"));
        let mut bad = asm.clone();
        bad.patterns[0].sources[0] = ComponentId(2);
        assert!(bad.validate().unwrap_err().contains("depends on it"), "{:?}", bad.validate());
        let mut bad = asm.clone();
        if let PatternKind::Rectangular { first, .. } = &mut bad.patterns[0].kind {
            first.along.component = Some(ComponentId(3));
            first.along.geom = Geom::Axis { axis: OriginAxis::X };
        }
        assert!(bad.validate().unwrap_err().contains("depends on it"), "{:?}", bad.validate());
        let mut bad = asm.clone();
        if let PatternKind::Rectangular { first, .. } = &mut bad.patterns[0].kind {
            first.spacing = f64::NAN;
        }
        assert!(bad.validate().unwrap_err().contains("spacing"));
        let mut bad = asm.clone();
        bad.patterns[0].elements[1].components[0] = ComponentId(2);
        assert!(bad.validate().unwrap_err().contains("more than one"), "{:?}", bad.validate());

        // Without a pattern nothing of them is written, so a file is as it was.
        let plain = serde_json::to_value(Assembly::default()).unwrap();
        assert!(plain.get("patterns").is_none() && plain.get("next_pattern").is_none(), "{plain}");
        let back: Assembly = serde_json::from_value(serde_json::to_value(&asm).unwrap()).unwrap();
        assert_eq!(back, asm);

        // Without an element it is no pattern (the commands refuse this: they change its count);
        // the other elements stay. The source takes the pattern with it, with all that stands
        // for it.
        let mut less = asm.clone();
        assert!(less.remove_component(ComponentId(3)));
        assert!(less.patterns.is_empty());
        assert_eq!(less.components.len(), 2);
        less.validate().unwrap();
        let mut none = asm.clone();
        assert!(none.remove_component(source));
        assert!(none.patterns.is_empty() && none.components.is_empty(), "{none:?}");

        // A pattern taken away takes its elements; kept, they are components of their own.
        let mut gone = asm.clone();
        assert!(gone.remove_pattern(id, false));
        assert_eq!(gone.components.len(), 1);
        let mut kept = asm;
        assert!(kept.remove_pattern(id, true));
        assert_eq!(kept.components.len(), 3);
        assert!(kept.elements().is_empty());
        kept.validate().unwrap();
    }

    #[test]
    fn mirrored_occurrences_have_a_part_key_of_their_own() {
        let mut asm = Assembly::default();
        let a = comp(&mut asm, "Plate");
        let plain = asm.component(a).unwrap().clone();
        let mirrored = Component { mirrored: true, ..plain.clone() };
        let (k0, k1) = (plain.key(), mirrored.key());
        assert_ne!(k0, k1);
        assert_eq!(key_file(&k1), "p.tenon");
        let row = Component { row: Some("A".into()), ..mirrored.clone() };
        assert_ne!(row.key(), k1, "each size has its mirror image");
        assert_eq!(key_file(&row.key()), "p.tenon");
        // A row named as the word in a mirrored key is still only a row.
        let word = Component { row: Some("mirrored".into()), ..plain };
        assert_ne!(word.key(), k1);
        // In a file it is written only when it is so.
        assert!(serde_json::to_value(&plain_of(&asm, a)).unwrap().get("mirrored").is_none());
        assert_eq!(serde_json::to_value(&mirrored).unwrap()["mirrored"], true);
    }

    fn plain_of(asm: &Assembly, id: ComponentId) -> Component {
        asm.component(id).unwrap().clone()
    }

    #[test]
    fn the_places_of_a_pattern_leave_out_the_sources() {
        let along = Target { component: None, geom: Geom::Axis { axis: OriginAxis::X } };
        let run = |count| Run { along: along.clone(), reverse: false, count, spacing: 5.0 };
        let two = PatternKind::Rectangular { first: run(3), second: Some(run(2)) };
        assert_eq!(two.places(), [[1, 0], [2, 0], [0, 1], [1, 1], [2, 1]]);
        let round = PatternKind::Circular { axis: along.clone(), reverse: false, count: 4, angle: 1.0 };
        assert_eq!(round.places(), [[1, 0], [2, 0], [3, 0]]);
        assert_eq!(PatternKind::Mirror { plane: along.clone(), reuse: Vec::new() }.places(), [[1, 0]]);
        assert_eq!(PatternKind::Rectangular { first: run(1), second: None }.check().unwrap_err(), "a pattern needs at least two occurrences");
        assert!(PatternKind::Rectangular { first: run(MAX_COUNT + 1), second: None }.check().is_err());
        assert!(PatternKind::Circular { axis: along.clone(), reverse: false, count: 2, angle: 0.0 }.check().is_err());
        assert!(PatternKind::Circular { axis: along, reverse: false, count: 1, angle: 1.0 }.check().is_err());
    }
}

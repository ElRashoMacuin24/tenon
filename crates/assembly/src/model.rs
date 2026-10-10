//! The assembly document: components (placed part files), the relationships between them
//! (constraints and joints) and the steps of the exploded view. Saved as `.tenonasm`
//! (docs/file-format.md, DEC-024).

use serde::{Deserialize, Serialize};
use tenon_geom::{Frame, Vec3, tol};
use tenon_model::{EdgeRef, FaceRef, FeatureId, OriginAxis, OriginPlane};

/// Most components one assembly may have.
pub const MAX_COMPONENTS: usize = 5_000;
/// Most relationships one assembly may have.
pub const MAX_RELATIONSHIPS: usize = 20_000;
/// Most exploded-view steps.
pub const MAX_TWEAKS: usize = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ComponentId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RelationshipId(pub u32);

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
}

/// The key of a part among an assembly's loaded parts: its file's path, and for a part in one
/// row of its design table, the path and the row (parted by a character no path can hold).
pub fn part_key(part: &str, row: Option<&str>) -> String {
    match row {
        Some(row) => format!("{part}\0{row}"),
        None => part.to_owned(),
    }
}

/// The file of a part key (see [`part_key`]).
pub fn key_file(key: &str) -> &str {
    key.split('\0').next().unwrap_or(key)
}

impl Component {
    /// The key of this component's part among the assembly's loaded parts.
    pub fn key(&self) -> String {
        part_key(&self.part, self.row.as_deref())
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
    pub fn targets(&self) -> [&Target; 2] {
        match self {
            RelKind::Mate { a, b, .. }
            | RelKind::Flush { a, b, .. }
            | RelKind::Angle { a, b, .. }
            | RelKind::Insert { a, b, .. }
            | RelKind::Joint { a, b, .. } => [a, b],
        }
    }
    fn targets_mut(&mut self) -> [&mut Target; 2] {
        match self {
            RelKind::Mate { a, b, .. }
            | RelKind::Flush { a, b, .. }
            | RelKind::Angle { a, b, .. }
            | RelKind::Insert { a, b, .. }
            | RelKind::Joint { a, b, .. } => [a, b],
        }
    }
    /// The components it connects (the assembly origin is not one).
    pub fn components(&self) -> Vec<ComponentId> {
        let mut v: Vec<ComponentId> = self.targets().iter().filter_map(|t| t.component).collect();
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
            RelKind::Joint { joint, .. } => joint.label(),
        }
    }
    pub fn is_joint(&self) -> bool {
        matches!(self, RelKind::Joint { .. })
    }
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
    /// The exploded view, step by step.
    #[serde(default)]
    pub explode: Vec<Tweak>,
    pub next_component: u32,
    pub next_relationship: u32,
}

impl Default for Assembly {
    fn default() -> Self {
        Assembly {
            name: "Assembly1".into(),
            components: Vec::new(),
            relationships: Vec::new(),
            explode: Vec::new(),
            next_component: 1,
            next_relationship: 1,
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
    pub fn component(&self, id: ComponentId) -> Option<&Component> {
        self.components.iter().find(|c| c.id == id)
    }
    pub fn component_mut(&mut self, id: ComponentId) -> Option<&mut Component> {
        self.components.iter_mut().find(|c| c.id == id)
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

    /// Removes a component with its relationships and exploded-view steps.
    pub fn remove_component(&mut self, id: ComponentId) -> bool {
        let before = self.components.len();
        self.components.retain(|c| c.id != id);
        self.relationships.retain(|r| !r.kind.components().contains(&id));
        for t in &mut self.explode {
            t.components.retain(|c| *c != id);
        }
        self.explode.retain(|t| !t.components.is_empty());
        self.components.len() != before
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
            for t in r.kind.targets() {
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
                RelKind::Mate { offset, .. } | RelKind::Flush { offset, .. } | RelKind::Insert { offset, .. } => offset.abs() <= tol::MAX_SIZE,
                RelKind::Angle { angle, reference, .. } => angle.is_finite() && (reference.len() - 1.0).abs() < tol::UNIT,
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
        asm.components.push(Component { id, name, part: "p.tenon".into(), placement: Frame::WORLD, grounded: false, row: None, visible: true });
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
}

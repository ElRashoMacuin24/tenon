//! Operation lineage: where each sub-shape of a result came from.
//!
//! Every modelling operation returns an [`Op`]: the new shape plus a [`History`]. The model layer
//! uses the history to keep persistent references alive across regeneration
//! (docs/persistent-naming.md); kernels never invent names themselves.

use serde::{Deserialize, Serialize};

use crate::{ShapeHandle, TopoId};

/// Result of a modelling operation.
#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    pub shape: ShapeHandle,
    pub history: History,
}

/// A sub-shape of the `input`-th input of an operation. For booleans input 0 is the target and
/// inputs 1.. are the tools, in call order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InputRef {
    pub input: u32,
    pub id: TopoId,
}

/// Where one input sub-shape ended up: the result sub-shapes it became (unchanged, modified or
/// split). An empty `result` means it was deleted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Image {
    pub source: InputRef,
    pub result: Vec<TopoId>,
}

/// What created a new sub-shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Origin {
    /// Generated from an input sub-shape (a fillet face from an edge, a section edge from a face).
    Input(InputRef),
    /// Swept from a profile curve; `tag` is the caller's tag (usually a sketch entity id).
    ProfileCurve { tag: u64 },
}

/// Sub-shapes created by the operation rather than carried over from an input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Generated {
    pub origin: Origin,
    pub result: Vec<TopoId>,
}

/// Named faces of primitives and sweeps, in the operation's own frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrimitiveRole {
    BoxXMin,
    BoxXMax,
    BoxYMin,
    BoxYMax,
    BoxZMin,
    BoxZMax,
    /// Curved face of a cylinder or cone.
    Lateral,
    /// Cap at the axis origin (cylinder, cone).
    Bottom,
    /// Cap at the far end of the axis (cylinder, cone).
    Top,
    /// Face(s) where an extrusion or partial revolution starts (the swept profile).
    StartCap,
    /// Face(s) where an extrusion or partial revolution ends.
    EndCap,
}

/// Lineage of one operation's result.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    /// For each input face and edge: its image in the result.
    pub images: Vec<Image>,
    /// Result sub-shapes created by the operation.
    pub generated: Vec<Generated>,
    /// For primitives: which result face plays which role.
    pub roles: Vec<(PrimitiveRole, TopoId)>,
}

impl History {
    /// The result sub-shapes `source` became, or `None` if the history does not mention it.
    pub fn image_of(&self, source: InputRef) -> Option<&[TopoId]> {
        self.images.iter().find(|i| i.source == source).map(|i| i.result.as_slice())
    }
    /// True when the history records `source` as deleted.
    pub fn is_deleted(&self, source: InputRef) -> bool {
        self.image_of(source).is_some_and(<[TopoId]>::is_empty)
    }
    /// Result face playing `role`, for primitives (the first one when several faces share it).
    pub fn role(&self, role: PrimitiveRole) -> Option<TopoId> {
        self.roles.iter().find(|(r, _)| *r == role).map(|(_, t)| *t)
    }
    /// Every result face playing `role` (a multi-region extrusion has several start caps).
    pub fn roles_of(&self, role: PrimitiveRole) -> impl Iterator<Item = TopoId> + '_ {
        self.roles.iter().filter(move |(r, _)| *r == role).map(|(_, t)| *t)
    }
    /// Result sub-shapes generated from `origin`.
    pub fn generated_from(&self, origin: Origin) -> impl Iterator<Item = TopoId> + '_ {
        self.generated.iter().filter(move |g| g.origin == origin).flat_map(|g| g.result.iter().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups() {
        let src = InputRef { input: 0, id: TopoId::face(3) };
        let gone = InputRef { input: 1, id: TopoId::face(0) };
        let h = History {
            images: vec![Image { source: src, result: vec![TopoId::face(5)] }, Image { source: gone, result: vec![] }],
            generated: vec![Generated { origin: Origin::Input(gone), result: vec![TopoId::edge(9), TopoId::edge(10)] }],
            roles: vec![(PrimitiveRole::Top, TopoId::face(2))],
        };
        assert_eq!(h.image_of(src), Some(&[TopoId::face(5)][..]));
        assert!(h.is_deleted(gone) && !h.is_deleted(src));
        assert!(h.image_of(InputRef { input: 7, id: TopoId::face(0) }).is_none());
        assert_eq!(h.role(PrimitiveRole::Top), Some(TopoId::face(2)));
        assert_eq!(h.role(PrimitiveRole::Bottom), None);
        assert_eq!(h.generated_from(Origin::Input(gone)).count(), 2);
    }
}

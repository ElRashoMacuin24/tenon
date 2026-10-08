//! Opaque shape handles and sub-shape ids.

use serde::{Deserialize, Serialize};

/// Opaque handle to a shape stored inside a [`Kernel`](crate::Kernel) instance.
///
/// Handles are generational: after [`Kernel::release`](crate::Kernel::release) the old handle
/// reports [`KernelError::InvalidHandle`](crate::KernelError::InvalidHandle) even if the slot is
/// reused. Handles are **not persistent**: documents never store them, nor the sub-shape indices
/// below (docs/persistent-naming.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ShapeHandle {
    index: u32,
    generation: u32,
}

impl ShapeHandle {
    /// For backends: a handle for arena slot `index` at `generation`.
    pub const fn from_parts(index: u32, generation: u32) -> Self {
        ShapeHandle { index, generation }
    }
    pub const fn index(self) -> u32 {
        self.index
    }
    pub const fn generation(self) -> u32 {
        self.generation
    }
    pub fn face(self, index: u32) -> FaceId {
        FaceId { shape: self, index }
    }
    pub fn edge(self, index: u32) -> EdgeId {
        EdgeId { shape: self, index }
    }
    pub fn vertex(self, index: u32) -> VertexId {
        VertexId { shape: self, index }
    }
}

/// Sub-shape dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TopoKind {
    Vertex,
    Edge,
    Face,
}

/// A sub-shape of one particular shape: its kind and its 0-based index in that shape's
/// deterministic enumeration (see [`Topology`](crate::Topology)). Meaningful only together with
/// the shape it was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TopoId {
    pub kind: TopoKind,
    pub index: u32,
}

impl TopoId {
    pub const fn face(index: u32) -> Self {
        TopoId { kind: TopoKind::Face, index }
    }
    pub const fn edge(index: u32) -> Self {
        TopoId { kind: TopoKind::Edge, index }
    }
    pub const fn vertex(index: u32) -> Self {
        TopoId { kind: TopoKind::Vertex, index }
    }
}

macro_rules! sub_shape_id {
    ($(#[$doc:meta])* $name:ident, $kind:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name {
            pub shape: ShapeHandle,
            pub index: u32,
        }
        impl $name {
            pub const fn topo(self) -> TopoId {
                TopoId { kind: TopoKind::$kind, index: self.index }
            }
        }
    };
}

sub_shape_id!(
    /// A face of a shape (0-based index into its face enumeration).
    FaceId,
    Face
);
sub_shape_id!(
    /// An edge of a shape (0-based index into its edge enumeration).
    EdgeId,
    Edge
);
sub_shape_id!(
    /// A vertex of a shape (0-based index into its vertex enumeration).
    VertexId,
    Vertex
);

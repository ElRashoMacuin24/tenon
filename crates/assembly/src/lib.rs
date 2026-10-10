//! Tenon assemblies: components (placed part files), constraints and joints, the solver and
//! degrees of freedom, the parts list, interference and the exploded view.
//!
//! - [`Assembly`]: the document (saved as `.tenonasm`, DEC-024).
//! - [`AsmSession`]: an open assembly with undo, its part documents (edited in place) and their
//!   geometry.
//! - [`solve`]: the rigid-body solver and degrees-of-freedom analysis (DEC-025).
//! - [`cmd`]: the `asm.*` commands.
#![forbid(unsafe_code)]

pub mod cmd;
pub mod geometry;
pub mod interfere;
pub mod math;
mod model;
pub mod session;
pub mod solve;

pub use geometry::Prim;
pub use model::{
    Assembly, Component, ComponentId, Geom, JointKind, MAX_COMPONENTS, MAX_RELATIONSHIPS, RelKind, Relationship, RelationshipId, Target, Tweak,
    key_file, part_key,
};
pub use session::{AsmSession, BomRow, Part, Parts, Solved, Variant};

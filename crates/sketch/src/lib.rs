//! Tenon sketches: 2D geometry on a plane, constraints, the solver, editing tools and the
//! closed regions that become kernel profiles.
//!
//! - [`Sketch`]: points, lines, circles, arcs, splines (points are shared between joined
//!   curves) and constraints.
//! - [`SketchSolver`] / [`GaussNewton`]: solving, drag, degrees of freedom, conflict and
//!   redundancy detection.
//! - Tools: rectangle, polygon, three-point arc, fillet, trim, offset, mirror.
//! - [`regions`] / [`profile`]: closed regions and their kernel [`Profile`](tenon_kernel::Profile),
//!   with entity ids as curve tags.
#![forbid(unsafe_code)]

mod constraint;
mod entity;
pub mod lm;

mod modify;
mod profile;
mod sketch;
mod solve;
pub mod text;
mod tools;

pub use constraint::Constraint;
pub use entity::{ConstraintId, Entity, EntityId, Geometry, PointRef};
pub use modify::{MAX_PATTERN, Motion};
pub use profile::{Piece, RegionKey, SketchRegion, default_regions, find, key_of, outlines, profile, regions, text_regions};
pub use sketch::{ArcSlot, MAX_CONSTRAINTS, MAX_ENTITIES, Sketch, SketchError, SketchResult, Slot};
pub use solve::{Dof, GaussNewton, SketchSolver, SolveOptions};

#[cfg(test)]
mod tests;

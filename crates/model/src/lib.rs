//! Tenon part model: the feature history, regeneration through the kernel, persistent face
//! references, the command registry with undo/redo, and the regeneration worker thread.
//!
//! Status: M1 scope. Features are sketches, extrudes and revolves; parameters/expressions,
//! rollback and the full persistent-naming resolver are M2 (docs/persistent-naming.md).
#![forbid(unsafe_code)]

pub mod cmd;
mod document;
pub mod naming;
pub mod regen;
pub mod worker;

pub use cmd::{CmdError, CmdResult, CommandSpec, Run, Session};
pub use document::{
    AxisRef, Chamfer, ChamferSize, DRILL_POINT, Document, Extrude, ExtrudeExtent, Feature, FeatureId, FeatureKind, Fillet, Hole, HoleExtent,
    HoleType, MAX_FEATURES, Operation, OriginAxis, OriginPlane, PlaneRef, RegionSel, Revolve, RevolveAngle, Shell, hole_centres,
};
pub use naming::{CapEnd, EdgeFingerprint, EdgeRef, FaceOrigin, FaceRef, Fingerprint, HoleFace};
pub use regen::{Body, BodyView, FeatureStatus, Regen, Scene, regenerate, scene};

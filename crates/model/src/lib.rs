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
    AxisRef, Document, Extrude, ExtrudeExtent, Feature, FeatureId, FeatureKind, MAX_FEATURES, Operation, OriginAxis, OriginPlane, PlaneRef,
    RegionSel, Revolve, RevolveAngle,
};
pub use naming::{CapEnd, FaceOrigin, FaceRef, Fingerprint};
pub use regen::{Body, BodyView, FeatureStatus, Regen, Scene, regenerate, scene};

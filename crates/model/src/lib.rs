//! Tenon part model: the feature history, regeneration through the kernel (incremental),
//! persistent face and edge references, parameters and equations, measuring, the command registry
//! with undo/redo, and the regeneration worker thread.
//!
//! Status: M2. Features: sketches, extrude, revolve, fillet, chamfer, shell, hole, rib, patterns,
//! mirror, work planes/axes/points; End of Part and reordering (docs/persistent-naming.md,
//! docs/architecture.md).
#![forbid(unsafe_code)]

pub mod cmd;
mod document;
mod explain;
pub mod expr;
pub mod measure;
pub mod naming;
pub mod params;
pub mod regen;
pub mod repair;
pub mod worker;

pub use cmd::{CmdError, CmdResult, CommandSpec, Run, Session};
pub use document::{
    AxisRef, AxisSel, Chamfer, ChamferSize, CircPattern, Coil, DRILL_POINT, DirectionRef, Document, Extrude, ExtrudeExtent, Feature, FeatureId,
    FeatureKind, Fillet, Hole, HoleExtent, HoleType, Loft, MAX_COPIES, MAX_FEATURES, Mirror, Operation, OriginAxis, OriginPlane, PlaneRef,
    RectPattern, RegionSel, Revolve, RevolveAngle, Rib, RibExtent, Shell, SketchCurves, Sweep, WorkAxis, WorkPlane, WorkPoint, hole_centres,
    open_lines,
};
pub use naming::{CapEnd, EdgeFingerprint, EdgeRef, FaceOrigin, FaceRef, Fingerprint, HoleFace};
pub use params::{ModelParam, ParamUnit, Parameters, UserParam, ValuePath};
pub use regen::{Body, BodyView, FeatureStatus, Regen, RegenCache, Scene, Tool, WorkGeom, regenerate, regenerate_with, scene};

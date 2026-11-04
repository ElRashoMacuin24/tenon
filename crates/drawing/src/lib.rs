//! Tenon drawings: sheets of views of a part or assembly (hidden-line removal, sections,
//! details, isometric views), associative dimensions, baseline and ordinate sets, centrelines,
//! hole tables, balloons and parts lists, title blocks, and PDF/SVG/DXF output.
//!
//! - [`Drawing`]: the document (saved as `.tenondrw`, DEC-026).
//! - [`views`]: frames and view geometry through the kernel.
//! - [`annotate`]: a sheet as [`Graphics`], with every annotation worked out from the model now.
//! - [`dimsets`]: how a baseline or ordinate set of dimensions is laid out.
//! - [`export`]: PDF, SVG, DXF.
//! - [`DrwSession`] and [`cmd`]: the open drawing and the `drw.*` commands.
#![forbid(unsafe_code)]

pub mod annotate;
pub mod clean;
pub mod cmd;
pub mod dimsets;
pub mod export;
pub mod graphics;
mod model;
pub mod modify;
pub mod schemas;
pub mod session;
pub mod sheets;
pub mod stroke;
pub mod suggest;
pub mod views;

pub use graphics::{Align, Graphics, Owner, Pen, Text};
pub use model::{
    AnnotId, AnnotKind, Annotation, Boundary, BreakLines, DimKind, Drawing, GeomPick, MAX_ANNOTATIONS, MAX_SET_PLACES, MAX_SHEETS, MAX_VIEWS, ModKind,
    Orientation, PickPoint, Props, SET_SPACING, SetAxis, Sheet, SheetId, SheetSize, Side, Standard, TitleBlock, TitleField, TitleLine, View, ViewId,
    ViewKind, ViewMod,
};
pub use session::{DrwModel, DrwSession};
pub use views::{Evaluation, ModelGeometry, ModelSource, ViewGeometry};

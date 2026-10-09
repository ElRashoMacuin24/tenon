//! Tenon drawings: sheets of views of a part or assembly (hidden-line removal, sections,
//! details, isometric views), associative dimensions, centrelines, hole tables, balloons and
//! parts lists, title blocks, and PDF/SVG/DXF output.
//!
//! - [`Drawing`]: the document (saved as `.tenondrw`, DEC-026).
//! - [`views`]: frames and view geometry through the kernel.
//! - [`annotate`]: a sheet as [`Graphics`], with every annotation worked out from the model now.
//! - [`export`]: PDF, SVG, DXF.
//! - [`DrwSession`] and [`cmd`]: the open drawing and the `drw.*` commands.
#![forbid(unsafe_code)]

pub mod annotate;
pub mod clean;
pub mod cmd;
pub mod export;
pub mod graphics;
mod model;
pub mod session;
pub mod sheets;
pub mod stroke;
pub mod views;

pub use graphics::{Align, Graphics, Owner, Pen, Text};
pub use model::{
    AnnotId, AnnotKind, Annotation, DimKind, Drawing, GeomPick, MAX_ANNOTATIONS, MAX_SHEETS, MAX_VIEWS, Orientation, PickPoint, Props, Sheet,
    SheetId, SheetSize, Side, Standard, TitleBlock, TitleField, TitleLine, View, ViewId, ViewKind,
};
pub use session::{DrwModel, DrwSession};
pub use views::{Evaluation, ModelGeometry, ModelSource, ViewGeometry};

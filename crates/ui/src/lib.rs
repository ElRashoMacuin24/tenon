//! Tenon egui front end. The only library crate that may know about egui (`cargo xtask layers`).
//!
//! Layout follows the established mechanical-CAD workflow (ribbon, model browser on the left,
//! orientation cube and navigation bar in the viewport, document tabs and status bar at the
//! bottom). Colours, icons and wording are Tenon's own.
#![forbid(unsafe_code)]

pub mod commands;
pub mod icons;
mod shell;
pub mod theme;

pub use shell::Shell;

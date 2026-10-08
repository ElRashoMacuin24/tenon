//! Tenon egui front end. The only library crate that may know about egui (`cargo xtask layers`).
//!
//! [`Workbench`] is the part-modelling window: ribbon, model browser, 3D viewport (wgpu or a
//! software fallback), sketch mode and feature dialogs. Every model edit goes through the shared
//! command registry, so what the UI can do, the CLI and MCP server can do too.
//!
//! Layout follows the established mechanical-CAD workflow; colours, icons and wording are Tenon's
//! own.
#![forbid(unsafe_code)]

mod browser;
mod chrome;
pub mod commands;
mod cube;
pub mod icons;
mod panels;
mod radial;
mod sketcher;
pub mod theme;
mod viewport;
mod workbench;

pub use theme::ThemeName;
pub use workbench::{Services, Workbench};

#[cfg(test)]
mod tests;

//! Tenon egui front end. The only library crate that may know about egui (`cargo xtask layers`).
//!
//! [`Workbench`] is the modelling window: ribbon, model browser, 3D viewport (wgpu or a software
//! fallback), sketch mode and feature dialogs for parts; components, relationships and the
//! exploded view for assemblies (with parts edited in place). Every model edit goes through the shared
//! command registry, so what the UI can do, the CLI and MCP server can do too.
//!
//! Layout follows the established mechanical-CAD workflow; colours, icons and wording are Tenon's
//! own.
#![forbid(unsafe_code)]

mod asm_browser;
mod asm_panel;
mod assembly;
mod browser;
mod chrome;
pub mod commands;
mod cube;
pub mod icons;
mod modify;
mod panels;
mod params_dialog;
mod properties;
mod radial;
mod sketcher;
pub mod theme;
mod viewport;
mod work;
mod workbench;

pub use theme::ThemeName;
pub use workbench::{Services, Workbench};

#[cfg(test)]
mod asm_tests;
#[cfg(test)]
mod tests;

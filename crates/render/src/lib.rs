//! Tenon 3D viewport rendering, without any UI toolkit.
//!
//! - [`Camera`]: orbit/pan/zoom, perspective or orthographic, standard views, picking rays.
//! - [`pick`]: face and edge picking on kernel meshes.
//! - [`raster`]: a software renderer for PNG output without a GPU (CLI, MCP, CI).
//! - [`gpu`]: the interactive wgpu renderer (offscreen texture the UI displays).
//! - [`sheet`]: drawing sheets to images (PNG output of drawings).
#![forbid(unsafe_code)]

mod camera;
pub mod gpu;
pub mod pick;
pub mod raster;
pub mod sheet;

pub use camera::{Camera, Mat4, Projection, StdView, mul};

#[cfg(test)]
mod tests;

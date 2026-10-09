//! Tenon file formats.
//!
//! - [`project`]: the native `.tenon` part file and what every Tenon document shares: text from
//!   format version 2 (`text`, laid out by `layout`), zips before (docs/file-format.md).
//! - [`stl`]: STL output from a kernel mesh.
//! - STEP goes through the [`Kernel`](tenon_kernel::Kernel) trait (the kernel owns B-rep exchange).
//! - [`cmd`]: file commands (save, open, export) for the shared command registry.
//! - [`asm`]: assembly files (`.tenonasm`) and the assembly file commands.
//! - [`drw`]: drawing files (`.tenondrw`) and the drawing file and export commands.
//! - [`diff`]: what changed between two documents (`tenon-cli diff`).
#![forbid(unsafe_code)]

pub mod asm;
pub mod cmd;
pub mod diff;
pub mod drw;
mod layout;
pub mod project;
pub mod stl;
mod text;

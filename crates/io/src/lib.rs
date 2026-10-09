//! Tenon file formats.
//!
//! - [`project`]: the native `.tenon` project file (zip + versioned JSON, docs/file-format.md).
//! - [`stl`]: STL output from a kernel mesh.
//! - STEP goes through the [`Kernel`](tenon_kernel::Kernel) trait (the kernel owns B-rep exchange).
//! - [`cmd`]: file commands (save, open, export) for the shared command registry.
//! - [`asm`]: assembly files (`.tenonasm`) and the assembly file commands.
//! - [`drw`]: drawing files (`.tenondrw`) and the drawing file and export commands.
#![forbid(unsafe_code)]

pub mod asm;
pub mod cmd;
pub mod drw;
pub mod project;
pub mod stl;

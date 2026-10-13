//! Tenon file formats.
//!
//! - [`project`]: the native `.tenon` part file and what every Tenon document shares: text from
//!   format version 2 (`text`, laid out by `layout`), zips before (docs/file-format.md).
//! - [`stl`]: STL output from a kernel mesh.
//! - [`threemf`]: 3MF output (a welded mesh per body, with its colour, in an OPC zip).
//! - [`meshcheck`]: what a mesh is like to print: closed or not, overhangs, wall thickness.
//! - STEP goes through the [`Kernel`](tenon_kernel::Kernel) trait (the kernel owns B-rep exchange).
//! - [`cmd`]: file commands (save, open, export) for the shared command registry.
//! - [`asm`]: assembly files (`.tenonasm`) and the assembly file commands.
//! - [`drw`]: drawing files (`.tenondrw`) and the drawing file and export commands.
//! - [`diff`]: what changed between two documents (`tenon-cli diff`).
#![forbid(unsafe_code)]

/// Standard base64 (RFC 4648, with padding): how a picture travels inside a command's result.
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = |i: usize| u32::from(c.get(i).copied().unwrap_or(0));
        let n = (b(0) << 16) | (b(1) << 8) | b(2);
        for i in 0..4 {
            out.push(if i <= c.len() { char::from(T[((n >> (18 - 6 * i)) & 63) as usize]) } else { '=' });
        }
    }
    out
}

pub mod asm;
pub mod cmd;
pub mod control;
pub mod diff;
pub mod drw;
pub mod flat;
mod layout;
pub mod meshcheck;
pub mod print;
pub mod project;
pub mod scenes;
pub mod schemas;
pub mod steps;
pub mod stl;
mod text;
pub mod threemf;

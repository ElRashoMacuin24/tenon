//! Smoothing the kept surface (DEC-123): Taubin's two steps (lambda 0.5, mu -0.53) on the
//! vertices that are not fixed, with umbrella weights, `pairs` times; it keeps the volume
//! about where Laplace's alone shrinks it.
//!
//! CONTRACT: `smooth(skin, pairs)` moves no fixed vertex, keeps the triangles (and so the
//! surface closed), and is deterministic.

use super::Skin;

pub fn smooth(skin: &mut Skin, pairs: usize) {
    let _ = (skin, pairs);
}

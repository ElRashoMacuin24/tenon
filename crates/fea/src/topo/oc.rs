//! The optimization loop (DEC-123): SIMP with an optimality-criteria update.
//!
//! CONTRACT (filled in by the loop's implementer):
//! - Start: x_e = (V* - V_P) / (V - V_P) outside the passive set P, 1 in P (V* = fraction V,
//!   V the total volume, V_P the passive volume; V_P >= V* is refused in words). One solve with
//!   every element at level 1000 gives `solid_compliance`.
//! - Each round: x~ = A x (the filter); with `sharpen`, x- = P_beta(x~) (tanh projection about
//!   PROJECT_AT, slope s_e), else x- = x~ and s = 1; x- = 1 on P. Levels q = level_of(x-); the
//!   energies U from the solver; compliance c = 2 sum U. Sensitivity
//!   dc/dx-_e = -PENALTY x-_e^(PENALTY-1) (1 - FLOOR) 2 U_e / modulus(q_e, 1), 0 on P;
//!   dc/dx = A^T (s . dc/dx-); dV/dx = A^T (s . V . free).
//! - OC update by bisection on lambda: x'_j = clamp(x_j (max(0, -dc_j) / (lambda dV_j))^DAMPING,
//!   max(X_MIN, x_j - m), min(1, x_j + m)), x' = 1 on P; m = MOVE / sqrt(beta) when sharpening;
//!   the volume met to 1e-10 of the total (a dot product without sharpening; one filter pass
//!   per bisection with it), at most 200 passes.
//! - Stages: beta from STAGES, the next after STAGE_ITERATIONS rounds or when the change is
//!   below SETTLED. Stop rules (without sharpening, or at the last stage): change < SETTLED is
//!   Settled; |c_k - c_k-1| < LEVEL_OFF c_k for LEVEL_OFF_RUN rounds is LevelledOff; the round
//!   limit is Limit; the time budget is Budget; `cancel` or the solver saying STOPPED is
//!   Stopped once a round is done (Ok), and Err(STOPPED) before.
//! - `progress` is told each round. Sums run in a fixed order: the same inputs give the same
//!   design bit for bit.

use super::{Design, Goal, Iteration, Optimized, Stiffness};
use crate::tools::Cancel;

/// Finds the design: see the module's contract.
pub fn optimize(d: &Design, g: &Goal, s: &mut dyn Stiffness, cancel: &Cancel, progress: &mut dyn FnMut(&Iteration)) -> Result<Optimized, String> {
    let _ = (d, g, s, cancel, progress);
    Err("the optimizer is not written yet".into())
}

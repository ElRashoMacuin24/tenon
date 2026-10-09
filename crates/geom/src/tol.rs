//! Central tolerances.
//!
//! Every geometric comparison in Tenon takes its tolerance from this module, so tolerances can be
//! audited and tuned in one place. Do not introduce ad hoc epsilons elsewhere (AGENTS.md).
//!
//! Units: lengths in millimetres, angles in radians.

/// Two points closer than this are the same point (mm). Equal to OpenCASCADE's
/// `Precision::Confusion()`, so Tenon and the kernel agree on coincidence.
pub const LINEAR: f64 = 1e-7;

/// Two directions closer than this angle are parallel (rad). Equal to OpenCASCADE's
/// `Precision::Angular()`.
pub const ANGULAR: f64 = 1e-12;

/// 2D coincidence tolerance of the drafting geometry inherited from CADCraft (drawing units).
pub const EPS_2D: f64 = 1e-9;

/// Smallest dimension accepted for a modelling input such as a box side or a radius (mm).
/// Smaller values are rejected before they reach the kernel.
pub const MIN_SIZE: f64 = 1e-6;

/// Largest absolute coordinate or dimension accepted for modelling input (mm): one kilometre.
/// Larger values are treated as hostile input or a unit mistake.
pub const MAX_SIZE: f64 = 1e6;

/// Relative tolerance for comparing volumes, areas or lengths computed by different routes, for
/// example before and after a STEP round trip, or by two kernel backends.
pub const MEASURE_REL: f64 = 1e-6;

/// Stored unit vectors and placement frames (from files or commands) are accepted within this of
/// unit length and of perpendicular.
pub const UNIT: f64 = 1e-6;

/// An assembly constraint or joint holds when its residual is below this (mm, radians, or the
/// difference of unit vectors).
pub const ASSEMBLY: f64 = 1e-9;

/// After a solve, a relationship further than this from holding is reported as failing.
pub const ASSEMBLY_BROKEN: f64 = 1e-6;

/// Computed coordinates (solved sketch positions and component placements, reference
/// fingerprints) are written to files rounded to this many decimals of a millimetre: 1e-9 mm, a
/// hundredth of [`LINEAR`]. Last-digit noise from recomputing then never shows as a change in a
/// file.
pub const FILE_DECIMALS: usize = 9;

/// Two solids overlapping by less than this volume (mm^3) only touch; they do not interfere.
pub const CLASH_VOLUME: f64 = 1e-6;

/// When counting degrees of freedom, a motion is free when the constraints resist it by less than
/// this fraction of the stiffest direction (relative singular value).
pub const DOF_REL: f64 = 1e-7;

/// Default chord deviation for display tessellation (mm).
pub const MESH_LINEAR: f64 = 0.01;

/// Default angular deviation for display tessellation (rad, about 11.5 degrees).
pub const MESH_ANGULAR: f64 = 0.2;

/// True when `a` and `b` agree to within `rel` of the larger magnitude. Values below
/// [`LINEAR`] in magnitude compare absolutely, so zero equals zero.
pub fn rel_eq(a: f64, b: f64, rel: f64) -> bool {
    if !(a.is_finite() && b.is_finite()) {
        return false;
    }
    let scale = a.abs().max(b.abs());
    (a - b).abs() <= (rel * scale).max(LINEAR)
}

/// A finite, positive size within [`MIN_SIZE`]..=[`MAX_SIZE`].
pub fn is_valid_size(v: f64) -> bool {
    v.is_finite() && (MIN_SIZE..=MAX_SIZE).contains(&v)
}

/// A finite coordinate within ±[`MAX_SIZE`].
pub fn is_valid_coord(v: f64) -> bool {
    v.is_finite() && v.abs() <= MAX_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rel_eq_scales_with_magnitude() {
        assert!(rel_eq(1000.0, 1000.0005, MEASURE_REL));
        assert!(!rel_eq(1000.0, 1000.01, MEASURE_REL));
        assert!(rel_eq(0.0, 0.0, MEASURE_REL));
        assert!(rel_eq(0.0, LINEAR * 0.5, MEASURE_REL));
        assert!(!rel_eq(f64::NAN, f64::NAN, MEASURE_REL));
        assert!(!rel_eq(f64::INFINITY, f64::INFINITY, MEASURE_REL));
    }

    #[test]
    fn size_and_coord_ranges() {
        assert!(is_valid_size(10.0));
        for bad in [0.0, -1.0, MIN_SIZE * 0.5, MAX_SIZE * 2.0, f64::NAN, f64::INFINITY] {
            assert!(!is_valid_size(bad), "{bad}");
        }
        assert!(is_valid_coord(-MAX_SIZE) && is_valid_coord(0.0));
        assert!(!is_valid_coord(f64::NEG_INFINITY) && !is_valid_coord(MAX_SIZE * 1.5));
    }
}

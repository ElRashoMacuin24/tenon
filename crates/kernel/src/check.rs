//! Input validation shared by all backends, so every backend rejects the same hostile input the
//! same way, before it reaches native code.

use tenon_geom::{Vec3, tol};

use crate::{KResult, KernelError, MeshTol};

/// A positive finite size within [`tol::MIN_SIZE`]..=[`tol::MAX_SIZE`].
pub fn size(what: &str, v: f64) -> KResult<f64> {
    if tol::is_valid_size(v) {
        Ok(v)
    } else {
        Err(KernelError::InvalidInput(format!("{what} must be between {} and {} mm, got {v}", tol::MIN_SIZE, tol::MAX_SIZE)))
    }
}

/// A finite point within the modelling range.
pub fn point(what: &str, p: Vec3) -> KResult<Vec3> {
    if tol::is_valid_coord(p.x) && tol::is_valid_coord(p.y) && tol::is_valid_coord(p.z) {
        Ok(p)
    } else {
        Err(KernelError::InvalidInput(format!("{what} must be finite and within {} mm of the origin, got {p:?}", tol::MAX_SIZE)))
    }
}

/// A density for mass properties: finite and non-negative.
pub fn density(v: f64) -> KResult<f64> {
    if v.is_finite() && v >= 0.0 { Ok(v) } else { Err(KernelError::InvalidInput(format!("density must be finite and >= 0, got {v}"))) }
}

/// Tessellation tolerances: linear within the size range, angular in (0, pi/2].
pub fn mesh_tol(t: &MeshTol) -> KResult<()> {
    size("mesh linear deflection", t.linear)?;
    if t.angular.is_finite() && t.angular > tol::ANGULAR && t.angular <= std::f64::consts::FRAC_PI_2 {
        Ok(())
    } else {
        Err(KernelError::InvalidInput(format!("mesh angular deflection must be in (0, pi/2], got {}", t.angular)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_hostile_numbers() {
        assert!(size("width", 10.0).is_ok());
        for v in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e300] {
            assert!(matches!(size("width", v), Err(KernelError::InvalidInput(_))), "{v}");
        }
        assert!(point("origin", Vec3::new(1.0, 2.0, 3.0)).is_ok());
        assert!(point("origin", Vec3::new(f64::NAN, 0.0, 0.0)).is_err());
        assert!(point("origin", Vec3::new(0.0, 0.0, 1e12)).is_err());
        assert!(density(7.85e-6).is_ok() && density(0.0).is_ok());
        assert!(density(-1.0).is_err() && density(f64::NAN).is_err());
        assert!(mesh_tol(&MeshTol::default()).is_ok());
        assert!(mesh_tol(&MeshTol { linear: 0.01, angular: 0.0 }).is_err());
        assert!(mesh_tol(&MeshTol { linear: 0.0, angular: 0.3 }).is_err());
        assert!(mesh_tol(&MeshTol { linear: 0.01, angular: 4.0 }).is_err());
    }
}

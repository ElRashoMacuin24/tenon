//! Small linear algebra for the assembly solver: rotations and symmetric eigenvalues.

use tenon_geom::{Frame, Vec3};

/// A 3x3 matrix, row-major. Rotations map part directions to assembly directions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct M3(pub [[f64; 3]; 3]);

impl M3 {
    pub const IDENTITY: M3 = M3([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);

    /// The rotation whose columns are the frame's axes.
    pub fn of_frame(f: &Frame) -> M3 {
        M3::from_columns(f.x(), f.y(), f.z())
    }
    pub fn from_columns(x: Vec3, y: Vec3, z: Vec3) -> M3 {
        M3([[x.x, y.x, z.x], [x.y, y.y, z.y], [x.z, y.z, z.z]])
    }
    pub fn col(&self, i: usize) -> Vec3 {
        let m = &self.0;
        match i {
            0 => Vec3::new(m[0][0], m[1][0], m[2][0]),
            1 => Vec3::new(m[0][1], m[1][1], m[2][1]),
            _ => Vec3::new(m[0][2], m[1][2], m[2][2]),
        }
    }
    pub fn apply(&self, v: Vec3) -> Vec3 {
        let m = &self.0;
        Vec3::new(
            m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
            m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
            m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
        )
    }
    pub fn mul(&self, o: &M3) -> M3 {
        let mut r = [[0.0; 3]; 3];
        for (i, row) in r.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = (0..3).map(|k| self.0[i][k] * o.0[k][j]).sum();
            }
        }
        M3(r)
    }
    pub fn transpose(&self) -> M3 {
        let m = &self.0;
        M3([[m[0][0], m[1][0], m[2][0]], [m[0][1], m[1][1], m[2][1]], [m[0][2], m[1][2], m[2][2]]])
    }
    /// The rotation `|w|` radians about `w` (Rodrigues).
    pub fn exp(w: Vec3) -> M3 {
        let t = w.len();
        if t < 1e-12 {
            return M3([[1.0, -w.z, w.y], [w.z, 1.0, -w.x], [-w.y, w.x, 1.0]]);
        }
        let k = w * (1.0 / t);
        let (s, c) = t.sin_cos();
        let v = 1.0 - c;
        M3([
            [c + k.x * k.x * v, k.x * k.y * v - k.z * s, k.x * k.z * v + k.y * s],
            [k.y * k.x * v + k.z * s, c + k.y * k.y * v, k.y * k.z * v - k.x * s],
            [k.z * k.x * v - k.y * s, k.z * k.y * v + k.x * s, c + k.z * k.z * v],
        ])
    }
    /// Axis (unit) and angle of a rotation; the angle is 0 for the identity.
    pub fn axis_angle(&self) -> (Vec3, f64) {
        let m = &self.0;
        let cos = ((m[0][0] + m[1][1] + m[2][2] - 1.0) * 0.5).clamp(-1.0, 1.0);
        let angle = cos.acos();
        let v = Vec3::new(m[2][1] - m[1][2], m[0][2] - m[2][0], m[1][0] - m[0][1]);
        if v.len() > 1e-9 {
            return (v.normalized(), angle);
        }
        if cos > 0.0 {
            return (Vec3::Z, 0.0);
        }
        // Half a turn: the axis is the column of (R + I) / 2 with the largest diagonal.
        let i = (0..3).max_by(|a, b| m[*a][*a].total_cmp(&m[*b][*b])).unwrap_or(0);
        let col =
            Vec3::new(m[0][i] + if i == 0 { 1.0 } else { 0.0 }, m[1][i] + if i == 1 { 1.0 } else { 0.0 }, m[2][i] + if i == 2 { 1.0 } else { 0.0 });
        (col.normalized(), std::f64::consts::PI)
    }
}

/// The smallest rotation taking unit `from` to unit `to` (half a turn about a perpendicular
/// when they are opposite).
pub fn rotation_between(from: Vec3, to: Vec3) -> M3 {
    let c = from.cross(to);
    let d = from.dot(to).clamp(-1.0, 1.0);
    if c.len() < 1e-12 {
        if d > 0.0 {
            return M3::IDENTITY;
        }
        let p = if from.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        let axis = from.cross(p).normalized();
        return M3::exp(axis * std::f64::consts::PI);
    }
    M3::exp(c.normalized() * c.len().atan2(d))
}

/// A frame from a rotation and an origin (the rotation's columns are re-orthonormalised).
pub fn frame_of(r: &M3, origin: Vec3) -> Option<Frame> {
    Frame::new(origin, r.col(2), r.col(0))
}

/// Eigenvalues (ascending) and unit eigenvectors of a symmetric `n x n` matrix (row-major), by
/// cyclic Jacobi rotations.
pub fn sym_eigen(a: &[f64], n: usize) -> (Vec<f64>, Vec<Vec<f64>>) {
    let mut m: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| a.get(i * n + j).copied().unwrap_or(0.0)).collect()).collect();
    let mut v: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect()).collect();
    let get = |m: &Vec<Vec<f64>>, i: usize, j: usize| m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
    for _sweep in 0..100 {
        let off: f64 = (0..n).flat_map(|i| (0..n).filter(move |j| *j != i).map(move |j| (i, j))).map(|(i, j)| get(&m, i, j).powi(2)).sum();
        let scale: f64 = (0..n).map(|i| get(&m, i, i).powi(2)).sum::<f64>().max(1e-300);
        if off <= scale * 1e-30 {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = get(&m, p, q);
                if apq.abs() < 1e-300 {
                    continue;
                }
                let (app, aqq) = (get(&m, p, p), get(&m, q, q));
                let theta = (aqq - app) / (2.0 * apq);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (mkp, mkq) = (get(&m, k, p), get(&m, k, q));
                    if let Some(row) = m.get_mut(k) {
                        row[p] = c * mkp - s * mkq;
                        row[q] = s * mkp + c * mkq;
                    }
                }
                for k in 0..n {
                    let (mpk, mqk) = (get(&m, p, k), get(&m, q, k));
                    if let Some(row) = m.get_mut(p) {
                        row[k] = c * mpk - s * mqk;
                    }
                    if let Some(row) = m.get_mut(q) {
                        row[k] = s * mpk + c * mqk;
                    }
                }
                for row in v.iter_mut() {
                    let (vp, vq) = (row[p], row[q]);
                    row[p] = c * vp - s * vq;
                    row[q] = s * vp + c * vq;
                }
            }
        }
    }
    let mut pairs: Vec<(f64, Vec<f64>)> = (0..n).map(|i| (get(&m, i, i), v.iter().map(|row| row[i]).collect())).collect();
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    pairs.into_iter().unzip()
}

/// An orthonormal basis of the span of `vs` (Gram-Schmidt; `rel` drops vectors shorter than
/// that fraction of the longest).
pub fn span_basis(vs: &[Vec3], rel: f64) -> Vec<Vec3> {
    let longest = vs.iter().map(|v| v.len()).fold(0.0f64, f64::max);
    if longest <= 0.0 {
        return Vec::new();
    }
    let mut out: Vec<Vec3> = Vec::new();
    let mut rest: Vec<Vec3> = vs.to_vec();
    // Longest first, so the basis follows the dominant directions.
    rest.sort_by(|a, b| b.len().total_cmp(&a.len()));
    for v in rest {
        let mut u = v;
        for b in &out {
            u = u - *b * u.dot(*b);
        }
        if u.len() > rel * longest && out.len() < 3 {
            out.push(u.normalized());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn rotations() {
        let r = M3::exp(Vec3::Z * std::f64::consts::FRAC_PI_2);
        assert!(r.apply(Vec3::X).near(Vec3::Y, 1e-12));
        let (axis, angle) = r.axis_angle();
        assert!(axis.near(Vec3::Z, 1e-12) && (angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        let half = M3::exp(Vec3::new(1.0, 1.0, 0.0).normalized() * std::f64::consts::PI);
        let (axis, angle) = half.axis_angle();
        assert!((angle - std::f64::consts::PI).abs() < 1e-9);
        assert!(axis.cross(Vec3::new(1.0, 1.0, 0.0).normalized()).len() < 1e-9);
        for (a, b) in [(Vec3::X, Vec3::Y), (Vec3::Z, -Vec3::Z), (Vec3::X, Vec3::X), (Vec3::new(1.0, 2.0, 3.0).normalized(), -Vec3::Y)] {
            assert!(rotation_between(a, b).apply(a).near(b, 1e-12), "{a:?} {b:?}");
        }
        let r = M3::exp(Vec3::new(0.3, -0.2, 0.9));
        assert!(r.mul(&r.transpose()).0.iter().flatten().zip(M3::IDENTITY.0.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-12));
    }

    #[test]
    fn eigen_of_symmetric() {
        let a = [4.0, 1.0, 0.0, 1.0, 3.0, 0.0, 0.0, 0.0, 0.0];
        let (vals, vecs) = sym_eigen(&a, 3);
        assert!(vals[0].abs() < 1e-12);
        assert!(vecs[0][2].abs() > 1.0 - 1e-12);
        let (l1, l2) = ((7.0 - 5f64.sqrt()) / 2.0, (7.0 + 5f64.sqrt()) / 2.0);
        assert!((vals[1] - l1).abs() < 1e-12 && (vals[2] - l2).abs() < 1e-12);
        assert_eq!(span_basis(&[Vec3::X, Vec3::X * 2.0, Vec3::new(1.0, 1e-12, 0.0)], 1e-6).len(), 1);
        assert_eq!(span_basis(&[Vec3::X, Vec3::Y, Vec3::new(1.0, 1.0, 0.0)], 1e-6).len(), 2);
    }
}

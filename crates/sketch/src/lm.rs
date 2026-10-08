//! Damped, weighted minimum-norm Gauss-Newton (Levenberg-Marquardt style) with a numerical
//! Jacobian.
//!
//! Ported from CADCraft (crates/constraints/src/lm.rs at 14e143b, MIT OR Apache-2.0); see NOTICE.
//!
//! Each step solves `min ‖W^{1/2} Δx‖` subject to `J Δx = −r` (damped):
//! `Δx = −W⁻¹ Jᵀ (J W⁻¹ Jᵀ + μ I)⁻¹ r`. Under-constrained systems therefore move as little as
//! possible, and heavily weighted parameters (geometry the user just edited, or the first object
//! picked) move least.

/// Most residual rows one solve may have (the normal matrix is rows × rows).
pub const MAX_ROWS: usize = 800;

#[derive(Debug)]
pub(crate) struct Outcome {
    pub x: Vec<f64>,
    pub converged: bool,
}

pub(crate) fn max_abs(v: &[f64]) -> f64 {
    v.iter().fold(0.0f64, |m, a| if a.is_finite() { m.max(a.abs()) } else { f64::INFINITY })
}

fn norm2(v: &[f64]) -> f64 {
    v.iter().map(|a| a * a).sum::<f64>()
}

/// Numerical Jacobian over the free parameters (row-major, rows × free.len()).
pub(crate) fn jacobian(f: &dyn Fn(&[f64], &mut Vec<f64>), x: &[f64], free: &[usize], rows: usize) -> Vec<f64> {
    let nf = free.len();
    let mut jac = vec![0.0; rows * nf];
    let mut xp = x.to_vec();
    let (mut rp, mut rm) = (Vec::with_capacity(rows), Vec::with_capacity(rows));
    for (k, &j) in free.iter().enumerate() {
        let Some(&xj) = x.get(j) else { continue };
        let h = 1e-7 * xj.abs().max(1.0);
        if let Some(v) = xp.get_mut(j) {
            *v = xj + h;
        }
        rp.clear();
        f(&xp, &mut rp);
        if let Some(v) = xp.get_mut(j) {
            *v = xj - h;
        }
        rm.clear();
        f(&xp, &mut rm);
        if let Some(v) = xp.get_mut(j) {
            *v = xj;
        }
        for i in 0..rows {
            let d = (rp.get(i).copied().unwrap_or(0.0) - rm.get(i).copied().unwrap_or(0.0)) / (2.0 * h);
            if let Some(slot) = jac.get_mut(i * nf + k) {
                *slot = if d.is_finite() { d } else { 0.0 };
            }
        }
    }
    jac
}

/// Solve the dense system `a y = b` (n × n, row-major) by Gaussian elimination with partial
/// pivoting. `None` when singular.
fn solve_dense(mut a: Vec<f64>, mut b: Vec<f64>, n: usize) -> Option<Vec<f64>> {
    let scale = a.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-300);
    for col in 0..n {
        let mut piv = col;
        let mut best = 0.0;
        for r in col..n {
            let v = a.get(r * n + col)?.abs();
            if v > best {
                best = v;
                piv = r;
            }
        }
        if best <= scale * 1e-15 {
            return None;
        }
        if piv != col {
            for c in 0..n {
                a.swap(piv * n + c, col * n + c);
            }
            b.swap(piv, col);
        }
        let p = *a.get(col * n + col)?;
        for r in col + 1..n {
            let f = *a.get(r * n + col)? / p;
            if f == 0.0 {
                continue;
            }
            for c in col..n {
                let v = *a.get(col * n + c)?;
                *a.get_mut(r * n + c)? -= f * v;
            }
            let bv = *b.get(col)?;
            *b.get_mut(r)? -= f * bv;
        }
    }
    let mut y = vec![0.0; n];
    for r in (0..n).rev() {
        let mut s = *b.get(r)?;
        for c in r + 1..n {
            s -= a.get(r * n + c)? * y.get(c)?;
        }
        *y.get_mut(r)? = s / a.get(r * n + r)?;
    }
    y.iter().all(|v| v.is_finite()).then_some(y)
}

/// Numerical rank of a row-major `rows × cols` matrix (rows normalised first).
pub(crate) fn rank(m: &[f64], rows: usize, cols: usize) -> usize {
    let mut a: Vec<Vec<f64>> = (0..rows)
        .map(|i| {
            let r: Vec<f64> = m.get(i * cols..(i + 1) * cols).map(<[f64]>::to_vec).unwrap_or_default();
            let n = norm2(&r).sqrt();
            if n > 1e-12 { r.iter().map(|v| v / n).collect() } else { vec![0.0; cols] }
        })
        .collect();
    let mut rank = 0;
    let mut used = vec![false; rows];
    for c in 0..cols {
        let mut piv = None;
        let mut best = 1e-7;
        for (i, row) in a.iter().enumerate() {
            if used.get(i).copied().unwrap_or(true) {
                continue;
            }
            let v = row.get(c).copied().unwrap_or(0.0).abs();
            if v > best {
                best = v;
                piv = Some(i);
            }
        }
        let Some(p) = piv else { continue };
        if let Some(u) = used.get_mut(p) {
            *u = true;
        }
        rank += 1;
        let prow = a.get(p).cloned().unwrap_or_default();
        let pv = prow.get(c).copied().unwrap_or(1.0);
        for (i, row) in a.iter_mut().enumerate() {
            if i == p || used.get(i).copied().unwrap_or(true) {
                continue;
            }
            let f = row.get(c).copied().unwrap_or(0.0) / pv;
            if f != 0.0 {
                for (v, pr) in row.iter_mut().zip(&prow) {
                    *v -= f * pr;
                }
            }
        }
    }
    rank
}

/// Run the solver from `x0`. Only `free` parameters change.
pub(crate) fn solve(f: &dyn Fn(&[f64], &mut Vec<f64>), x0: &[f64], free: &[usize], weights: &[f64], tol: f64, max_iter: usize) -> Outcome {
    let mut x = x0.to_vec();
    let mut r = Vec::new();
    f(&x, &mut r);
    let rows = r.len();
    if rows == 0 || max_abs(&r) <= tol {
        return Outcome { x, converged: true };
    }
    if free.is_empty() || rows > MAX_ROWS {
        return Outcome { x, converged: false };
    }
    let nf = free.len();
    let winv: Vec<f64> = free.iter().map(|&j| 1.0 / weights.get(j).copied().unwrap_or(1.0).max(1e-12)).collect();
    let mut mu = 1e-10;
    let mut cost = norm2(&r);
    let mut iter = 0;
    let mut rn = Vec::with_capacity(rows);
    while iter < max_iter {
        iter += 1;
        let jac = jacobian(f, &x, free, rows);
        // A = J W⁻¹ Jᵀ
        let mut a = vec![0.0; rows * rows];
        for i in 0..rows {
            for k in i..rows {
                let mut s = 0.0;
                for (c, w) in winv.iter().enumerate() {
                    s += jac.get(i * nf + c).copied().unwrap_or(0.0) * jac.get(k * nf + c).copied().unwrap_or(0.0) * w;
                }
                if let Some(v) = a.get_mut(i * rows + k) {
                    *v = s;
                }
                if let Some(v) = a.get_mut(k * rows + i) {
                    *v = s;
                }
            }
        }
        let diag_max = (0..rows).map(|i| a.get(i * rows + i).copied().unwrap_or(0.0)).fold(0.0f64, f64::max).max(1e-12);
        let mut improved = false;
        for _ in 0..12 {
            let mut ad = a.clone();
            for i in 0..rows {
                if let Some(v) = ad.get_mut(i * rows + i) {
                    *v += mu * diag_max;
                }
            }
            let Some(y) = solve_dense(ad, r.clone(), rows) else {
                mu *= 10.0;
                continue;
            };
            let mut xn = x.clone();
            for (c, &j) in free.iter().enumerate() {
                let mut s = 0.0;
                for (i, yi) in y.iter().enumerate() {
                    s += jac.get(i * nf + c).copied().unwrap_or(0.0) * yi;
                }
                if let Some(v) = xn.get_mut(j) {
                    *v -= winv.get(c).copied().unwrap_or(1.0) * s;
                }
            }
            rn.clear();
            f(&xn, &mut rn);
            let cn = norm2(&rn);
            if cn.is_finite() && cn < cost {
                x = xn;
                std::mem::swap(&mut r, &mut rn);
                cost = cn;
                mu = (mu * 0.1).max(1e-15);
                improved = true;
                break;
            }
            mu *= 10.0;
        }
        if max_abs(&r) <= tol {
            return Outcome { x, converged: true };
        }
        if !improved {
            break;
        }
    }
    let converged = max_abs(&r) <= tol;
    Outcome { x, converged }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_and_rank() {
        let y = solve_dense(vec![2.0, 1.0, 1.0, 3.0], vec![3.0, 5.0], 2).unwrap();
        assert!((y[0] - 0.8).abs() < 1e-12 && (y[1] - 1.4).abs() < 1e-12);
        assert!(solve_dense(vec![1.0, 2.0, 2.0, 4.0], vec![1.0, 1.0], 2).is_none());
        assert_eq!(rank(&[1.0, 0.0, 0.0, 1.0, 1.0, 1.0], 3, 2), 2);
        assert_eq!(rank(&[1.0, 2.0, 2.0, 4.0], 2, 2), 1);
        assert_eq!(rank(&[0.0; 4], 2, 2), 0);
    }

    #[test]
    fn minimum_norm_step() {
        // One equation x + y = 2 from (0,0): the minimum-norm solution is (1,1).
        let f = |x: &[f64], out: &mut Vec<f64>| out.push(x[0] + x[1] - 2.0);
        let o = solve(&f, &[0.0, 0.0], &[0, 1], &[1.0, 1.0], 1e-12, 50);
        assert!(o.converged);
        assert!((o.x[0] - 1.0).abs() < 1e-9 && (o.x[1] - 1.0).abs() < 1e-9);
        // Heavy weight on x keeps it in place.
        let o = solve(&f, &[0.0, 0.0], &[0, 1], &[1e6, 1.0], 1e-12, 50);
        assert!(o.x[0].abs() < 1e-5 && (o.x[1] - 2.0).abs() < 1e-5);
    }

    #[test]
    fn inconsistent_does_not_converge() {
        let f = |x: &[f64], out: &mut Vec<f64>| {
            out.push(x[0] - 1.0);
            out.push(x[0] - 2.0);
        };
        let o = solve(&f, &[0.0], &[0], &[1.0], 1e-10, 100);
        assert!(!o.converged);
        assert!(o.x[0].is_finite());
    }
}

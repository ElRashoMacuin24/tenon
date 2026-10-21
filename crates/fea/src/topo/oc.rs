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
//!
//! Sharpening is not done yet: a goal that asks for it is refused.

use std::sync::atomic::Ordering;
use std::time::Instant;

use super::{
    DAMPING, Design, FLOOR, Filter, Goal, Iteration, LEVEL_OFF, LEVEL_OFF_RUN, LEVELS, MOVE, Optimized, PENALTY, SETTLED, Stiffness, Stop, X_MIN,
    level_of, modulus,
};
use crate::tools::{Cancel, STOPPED};

/// How near each update brings the volume kept to the volume asked, as a share of the whole.
const VOLUME_TOLERANCE: f64 = 1e-10;
/// The most times one update reckons the volume it would keep.
const PASSES: usize = 200;

/// Finds the design: see the module's contract.
pub fn optimize(d: &Design, g: &Goal, s: &mut dyn Stiffness, cancel: &Cancel, progress: &mut dyn FnMut(&Iteration)) -> Result<Optimized, String> {
    let n = d.volumes.len();
    if d.centroids.len() != n || d.passive.len() != n {
        return Err(format!(
            "each element needs a volume, a middle and whether it stays solid: there are {} volumes, {} middles and {} of the last",
            n,
            d.centroids.len(),
            d.passive.len()
        ));
    }
    if n == 0 {
        return Err("there are no elements to shape".into());
    }
    if d.volumes.iter().any(|v| !(v.is_finite() && *v > 0.0)) {
        return Err("every element's volume must be more than 0".into());
    }
    if !(g.fraction > 0.0 && g.fraction < 1.0) {
        return Err(format!("the share of the part to keep must be more than 0 and less than 1 (it is {})", g.fraction));
    }
    if g.max_iterations == 0 {
        return Err("at least one round must be allowed".into());
    }
    if g.sharpen {
        return Err("sharpening the design is not done yet".into());
    }
    let total: f64 = d.volumes.iter().sum();
    let solid: f64 = d.volumes.iter().zip(&d.passive).filter(|(_, p)| **p).map(|(v, _)| *v).sum();
    let target = g.fraction * total;
    if solid >= target {
        return Err(format!(
            "the elements that must stay solid (held, loaded or kept) are already {} of the part, and only {} is to be kept: keep a larger share, or keep fewer faces",
            percent(solid / total),
            percent(g.fraction)
        ));
    }
    let filter = Filter::new(&d.centroids, &d.volumes, g.radius, &g.mirrors)?;
    let tolerance = VOLUME_TOLERANCE * total;
    let start = (target - solid) / (total - solid);
    let mut x: Vec<f64> = d.passive.iter().map(|p| if *p { 1.0 } else { start }).collect();
    let free: Vec<f64> = d.volumes.iter().zip(&d.passive).map(|(v, p)| if *p { 0.0 } else { *v }).collect();
    let free_total: f64 = free.iter().sum();
    // The volume goes straight with the design: its slope is the same each round.
    let straight = filter.back(&free);

    let began = Instant::now();
    if cancel.load(Ordering::Relaxed) {
        return Err(STOPPED.into());
    }
    let solid_compliance = 2.0 * energies(s, &vec![LEVELS; n])?.iter().sum::<f64>();
    if solid_compliance <= 0.0 {
        return Err("the loads do no work on the part, so there is nothing to shape it for".into());
    }

    let mut history: Vec<Iteration> = Vec::new();
    // The densities the last solve saw.
    let mut last: Vec<f64> = Vec::new();
    // The rounds running the work has held steady.
    let mut steady = 0;
    loop {
        let round = Instant::now();
        let seen = see(&filter, &d.passive, &x);
        let levels: Vec<u16> = seen.iter().map(|v| level_of(*v)).collect();
        if cancel.load(Ordering::Relaxed) {
            return stopped(last, solid_compliance, history);
        }
        let u = match energies(s, &levels) {
            Err(e) if e == STOPPED => return stopped(last, solid_compliance, history),
            u => u?,
        };
        let compliance = 2.0 * u.iter().sum::<f64>();
        let dc = work_slope(&filter, &d.passive, &seen, &levels, &u);
        let next = update(&x, &dc, &straight, &d.passive, MOVE, target, tolerance, &mut |y| solid + dot(&straight, y));
        let change = next.iter().zip(&x).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        let grey = seen.iter().zip(&free).map(|(x, v)| 4.0 * x * (1.0 - x) * v).sum::<f64>() / free_total;
        let it = Iteration {
            number: history.len() + 1,
            compliance,
            fraction: dot(&d.volumes, &seen) / total,
            change,
            sharpness: 1.0,
            grey,
            seconds: round.elapsed().as_secs_f64(),
        };
        let before = history.last().map(|i| i.compliance);
        history.push(it);
        progress(&it);
        last = seen;
        x = next;

        steady = match before {
            Some(c) if (compliance - c).abs() < LEVEL_OFF * compliance => steady + 1,
            _ => 0,
        };
        let stop = if change < SETTLED {
            Some(Stop::Settled)
        } else if steady >= LEVEL_OFF_RUN {
            Some(Stop::LevelledOff)
        } else if history.len() >= g.max_iterations {
            Some(Stop::Limit)
        } else if began.elapsed() >= g.budget {
            Some(Stop::Budget)
        } else {
            None
        };
        if let Some(stop) = stop {
            return Ok(Optimized { density: last, solid_compliance, history, stop });
        }
    }
}

/// A run that was asked to stop: the design so far once there is one.
fn stopped(density: Vec<f64>, solid_compliance: f64, history: Vec<Iteration>) -> Result<Optimized, String> {
    if history.is_empty() { Err(STOPPED.into()) } else { Ok(Optimized { density, solid_compliance, history, stop: Stop::Stopped }) }
}

/// The solver's energies, one for each element and each a number.
fn energies(s: &mut dyn Stiffness, levels: &[u16]) -> Result<Vec<f64>, String> {
    let u = s.energies(levels)?;
    if u.len() != levels.len() {
        return Err(format!("the solver told the energy of {} elements, not of {}", u.len(), levels.len()));
    }
    if u.iter().any(|v| !v.is_finite()) {
        return Err("the solver told an energy that is not a number".into());
    }
    Ok(u)
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// A share as a percentage, to a tenth.
fn percent(share: f64) -> String {
    format!("{}%", (share * 1000.0).round() / 10.0)
}

/// What a solve is given from the design: each element's density, filtered, solid where it
/// must be.
fn see(filter: &Filter, passive: &[bool], x: &[f64]) -> Vec<f64> {
    filter.apply(x).iter().zip(passive).map(|(v, p)| if *p { 1.0 } else { v.clamp(0.0, 1.0) }).collect()
}

/// The volume a design keeps: its densities as a solve would see them, by volume (what the
/// straight slope gives without filtering again).
#[cfg(test)]
fn kept(filter: &Filter, d: &Design, x: &[f64]) -> f64 {
    dot(&d.volumes, &see(filter, &d.passive, x))
}

/// How the work of the loads goes with each design value: through the solved densities and
/// the filter in turn. An element's stiffness goes with its density cubed; the
/// energy it holds was worked out at its level's stiffness.
fn work_slope(filter: &Filter, passive: &[bool], density: &[f64], levels: &[u16], energies: &[f64]) -> Vec<f64> {
    let by_density: Vec<f64> = (0..density.len())
        .map(|e| {
            if passive.get(e).copied().unwrap_or(true) {
                return 0.0;
            }
            let x = density[e];
            let (q, u) = (levels.get(e).copied().unwrap_or(LEVELS), energies.get(e).copied().unwrap_or(0.0));
            -PENALTY * x.powf(PENALTY - 1.0) * (1.0 - FLOOR) * 2.0 * u / modulus(q, 1.0)
        })
        .collect();
    filter.back(&by_density)
}

/// One update of the design by the optimality criterion: each value moved toward where the
/// work it saves for the volume it costs is the same for all, by at most `m`, between
/// [`X_MIN`] and 1 (1 where solid); that common rate (lambda) found by halving, until
/// `volume` of the design is within `tolerance` of `target`.
fn update(
    x: &[f64],
    dc: &[f64],
    dv: &[f64],
    passive: &[bool],
    m: f64,
    target: f64,
    tolerance: f64,
    volume: &mut dyn FnMut(&[f64]) -> f64,
) -> Vec<f64> {
    // The work each value saves for the volume it costs.
    let gain: Vec<f64> = dc.iter().zip(dv).map(|(c, v)| if *v > 0.0 { (-c).max(0.0) / v } else { 0.0 }).collect();
    let at = |lambda: f64| -> Vec<f64> {
        x.iter()
            .zip(&gain)
            .zip(passive)
            .map(|((x, b), p)| {
                if *p {
                    return 1.0;
                }
                let grown = if *b > 0.0 { x * (b / lambda).powf(DAMPING) } else { 0.0 };
                grown.max((x - m).max(X_MIN)).min((x + m).min(1.0))
            })
            .collect()
    };
    let most = gain.iter().zip(passive).filter(|(_, p)| !**p).map(|(b, _)| *b).fold(0.0, f64::max);
    let (mut low, mut high) = (0.0, if most > 0.0 { 1e4 * most } else { 1.0 });
    let mut best = at(high);
    let mut kept = volume(&best);
    let mut passes = 1;
    // The upper rate raised until it keeps less than the target (or no less can be kept).
    while kept > target && passes < PASSES {
        let fewer = at(2.0 * high);
        if fewer == best {
            break;
        }
        (low, high) = (high, 2.0 * high);
        kept = volume(&fewer);
        best = fewer;
        passes += 1;
    }
    while (kept - target).abs() > tolerance && passes < PASSES {
        let mid = 0.5 * (low + high);
        if mid <= low || mid >= high {
            break;
        }
        best = at(mid);
        kept = volume(&best);
        passes += 1;
        if kept > target {
            low = mid;
        } else {
            high = mid;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    use tenon_geom::Vec3;

    use super::*;
    use crate::topo::Mirror;
    use crate::topo::filter::tests::{Lcg, block};

    /// A block's elements, none of them held solid.
    fn design(cells: [usize; 3], size: Vec3) -> Design {
        let (centroids, volumes) = block(cells, size);
        Design { passive: vec![false; volumes.len()], volumes, centroids }
    }

    fn goal(fraction: f64, radius: f64) -> Goal {
        Goal { fraction, radius, sharpen: false, max_iterations: 60, budget: Duration::from_secs(3600), mirrors: Vec::new() }
    }

    fn run(d: &Design, g: &Goal, s: &mut dyn Stiffness) -> Result<Optimized, String> {
        optimize(d, g, s, &Cancel::default(), &mut |_| {})
    }

    /// Every other element twice as stiff as the rest.
    fn twice(n: usize) -> Vec<f64> {
        (0..n).map(|e| if e % 2 == 0 { 2.0 } else { 1.0 }).collect()
    }

    /// Springs side by side, all pulled by one load: spring e is a_e times as stiff as its
    /// level's modulus, and holds k_e u^2 / 2 when the load stretches them all by u.
    struct Springs {
        a: Vec<f64>,
        load: f64,
        calls: usize,
        /// The levels of each call that was answered.
        levels: Vec<Vec<u16>>,
        /// The call that says it was stopped, the call that sets `cancel`, and the call that
        /// fails (0: none).
        stop_on: usize,
        cancel_on: usize,
        fail_on: usize,
        cancel: Cancel,
        sleep: Duration,
    }

    impl Springs {
        fn new(a: Vec<f64>) -> Springs {
            Springs {
                a,
                load: 10.0,
                calls: 0,
                levels: Vec::new(),
                stop_on: 0,
                cancel_on: 0,
                fail_on: 0,
                cancel: Cancel::default(),
                sleep: Duration::ZERO,
            }
        }

        /// The work of the load with each spring at exactly its density, not rounded to a level.
        fn work(&self, x: &[f64]) -> f64 {
            let k: f64 = self.a.iter().zip(x).map(|(a, x)| a * (FLOOR + x.powi(3) * (1.0 - FLOOR))).sum();
            self.load * self.load / k
        }
    }

    impl Stiffness for Springs {
        fn energies(&mut self, levels: &[u16]) -> Result<Vec<f64>, String> {
            self.calls += 1;
            if !self.sleep.is_zero() {
                std::thread::sleep(self.sleep);
            }
            if self.calls == self.cancel_on {
                self.cancel.store(true, Ordering::Relaxed);
            }
            if self.calls == self.stop_on {
                return Err(STOPPED.into());
            }
            if self.calls == self.fail_on {
                return Err("the solver ran out of memory".into());
            }
            self.levels.push(levels.to_vec());
            let k: Vec<f64> = self.a.iter().zip(levels).map(|(a, q)| a * modulus(*q, 1.0)).collect();
            let u = self.load / k.iter().sum::<f64>();
            Ok(k.iter().map(|k| 0.5 * k * u * u).collect())
        }
    }

    /// A solver that says one half of the elements works hard on one call and the other half on
    /// the next, the total the same (or `scale` times it on call k): the design keeps moving
    /// while the work does not.
    struct Swapping {
        calls: i32,
        scale: fn(i32) -> f64,
    }

    fn swapping(scale: fn(i32) -> f64) -> Swapping {
        Swapping { calls: 0, scale }
    }

    impl Stiffness for Swapping {
        fn energies(&mut self, levels: &[u16]) -> Result<Vec<f64>, String> {
            self.calls += 1;
            let n = levels.len();
            let scale = (self.scale)(self.calls);
            Ok((0..n).map(|e| scale * if (2 * e < n) == (self.calls % 2 == 1) { 1.0 } else { 1e-3 }).collect())
        }
    }

    /// A solver that answers as it is told.
    struct Told<F>(F);

    impl<F: FnMut(&[u16]) -> Result<Vec<f64>, String>> Stiffness for Told<F> {
        fn energies(&mut self, levels: &[u16]) -> Result<Vec<f64>, String> {
            (self.0)(levels)
        }
    }

    /// The design values each element may take in one update, at the least and at the most.
    fn bounds(x: &[f64], passive: &[bool], m: f64) -> (Vec<f64>, Vec<f64>) {
        let at = |f: &dyn Fn(f64) -> f64| x.iter().zip(passive).map(|(x, p)| if *p { 1.0 } else { f(*x) }).collect::<Vec<f64>>();
        (at(&|x| (x - m).max(X_MIN)), at(&|x| (x + m).min(1.0)))
    }

    fn solid_volume(d: &Design) -> f64 {
        d.volumes.iter().zip(&d.passive).filter(|(_, p)| **p).map(|(v, _)| *v).sum()
    }

    fn free_volume(d: &Design) -> Vec<f64> {
        d.volumes.iter().zip(&d.passive).map(|(v, p)| if *p { 0.0 } else { *v }).collect()
    }

    #[test]
    fn the_update_meets_the_volume_and_its_bounds() {
        // Through the loop: from the second round on, each design keeps the share asked.
        let mut d = design([4, 3, 2], Vec3::new(4.0, 3.0, 2.0));
        for e in [0, 1, 2, 30, 31] {
            d.passive[e] = true;
        }
        let n = d.volumes.len();
        let mut rng = Lcg(17);
        let o = run(&d, &goal(0.35, 1.4), &mut Springs::new(rng.many(n, 0.5, 2.0))).unwrap();
        assert!(o.history.len() > 5, "{:?}", o.stop);
        for it in &o.history[1..] {
            assert!((it.fraction - 0.35).abs() < 1e-6 * 0.35, "{it:?}");
        }
        assert!(o.density.iter().zip(&d.passive).all(|(x, p)| if *p { *x == 1.0 } else { (0.0..=1.0).contains(x) }));

        // One update by itself, from designs anywhere, to volumes it can reach.
        let filter = Filter::new(&d.centroids, &d.volumes, 1.4, &[]).unwrap();
        let dv = filter.back(&free_volume(&d));
        let (solid, total) = (solid_volume(&d), d.volumes.iter().sum::<f64>());
        let linear = |y: &[f64]| solid + dot(&dv, y);
        for trial in 0..5 {
            let x: Vec<f64> = d.passive.iter().map(|p| if *p { 1.0 } else { X_MIN + (1.0 - X_MIN) * rng.next() }).collect();
            let dc = rng.many(n, -5.0, 0.0);
            let (lo, hi) = bounds(&x, &d.passive, MOVE);
            let target = linear(&lo) + (0.2 + 0.15 * trial as f64) * (linear(&hi) - linear(&lo));
            let y = update(&x, &dc, &dv, &d.passive, MOVE, target, VOLUME_TOLERANCE * total, &mut |y| linear(y));
            assert!((linear(&y) - target).abs() <= 1e-10 * total, "{} {target}", linear(&y));
            assert!((kept(&filter, &d, &y) - target).abs() < 1e-6 * target);
            for e in 0..n {
                if d.passive[e] {
                    assert_eq!(y[e], 1.0);
                } else {
                    assert!((X_MIN..=1.0).contains(&y[e]) && (y[e] - x[e]).abs() <= MOVE + 1e-15, "{} from {}", y[e], x[e]);
                }
            }
            // Some held at a limit, and some moved to where the criterion puts them, between.
            let free: Vec<usize> = (0..n).filter(|e| !d.passive[*e]).collect();
            let held = free.iter().filter(|e| y[**e] == lo[**e] || y[**e] == hi[**e]).count();
            assert!(held > 0 && held < free.len(), "{held} of {}", free.len());
        }
        // Wanting more than can be kept keeps as much as can be; wanting less, as little.
        let x: Vec<f64> = d.passive.iter().map(|p| if *p { 1.0 } else { 0.5 }).collect();
        let dc = rng.many(n, -5.0, 0.0);
        let (lo, hi) = bounds(&x, &d.passive, MOVE);
        assert_eq!(update(&x, &dc, &dv, &d.passive, MOVE, total, 1e-10 * total, &mut |y| linear(y)), hi);
        assert_eq!(update(&x, &dc, &dv, &d.passive, MOVE, 0.0, 1e-10 * total, &mut |y| linear(y)), lo);
    }

    #[test]
    fn the_linear_volume_equals_the_filtered_volume() {
        let mut d = design([4, 3, 3], Vec3::new(8.0, 6.0, 6.0));
        let mut rng = Lcg(23);
        d.volumes.iter_mut().for_each(|v| *v *= 0.5 + rng.next());
        for e in [3, 50, 51, 52, 200] {
            d.passive[e] = true;
        }
        for mirrors in [vec![], vec![Mirror { point: Vec3::new(8.0, 0.0, 0.0), normal: Vec3::X }]] {
            let filter = Filter::new(&d.centroids, &d.volumes, 2.5, &mirrors).unwrap();
            let dv = filter.back(&free_volume(&d));
            for _ in 0..5 {
                let x: Vec<f64> = d.passive.iter().map(|p| if *p { 1.0 } else { X_MIN + (1.0 - X_MIN) * rng.next() }).collect();
                let (linear, filtered) = (solid_volume(&d) + dot(&dv, &x), kept(&filter, &d, &x));
                assert!((linear - filtered).abs() < 1e-12 * filtered, "{linear} {filtered}");
            }
        }
    }

    #[test]
    fn more_kept_than_the_target_is_refused_in_words() {
        let n = 100;
        let mut d = Design {
            volumes: vec![2.0; n],
            centroids: (0..n).map(|i| Vec3::new(i as f64, 0.0, 0.0)).collect(),
            passive: (0..n).map(|i| i < 41).collect(),
        };
        let mut s = Springs::new(vec![1.0; n]);
        assert_eq!(
            run(&d, &goal(0.3, 0.5), &mut s).unwrap_err(),
            "the elements that must stay solid (held, loaded or kept) are already 41% of the part, and only 30% is to be kept: keep a larger share, or keep fewer faces"
        );
        assert_eq!(s.calls, 0, "refused before anything is solved");
        // Just as much is refused too; a little less is not.
        d.passive = (0..n).map(|i| i < 30).collect();
        assert!(run(&d, &goal(0.3, 0.5), &mut s).unwrap_err().contains("already 30% of the part, and only 30% is to be kept"));
        d.passive = (0..n).map(|i| i < 29).collect();
        // Then the others start with what is left: 60 - 58 mm^3 over the 142 of them.
        let mut t = Springs::new(vec![1.0; n]);
        let o = run(&d, &goal(0.3, 0.5), &mut t).unwrap();
        assert!((o.history[0].fraction - 0.3).abs() < 1e-15, "{:?}", o.history[0]);
        assert_eq!(t.levels[1], (0..n).map(|i| if i < 29 { 1000 } else { level_of(2.0 / 142.0) }).collect::<Vec<_>>());
        // And what is asked must make sense.
        for f in [0.0, 1.0, -0.2, f64::NAN] {
            assert!(run(&d, &goal(f, 0.5), &mut s).unwrap_err().starts_with("the share of the part to keep must be more than 0 and less than 1"));
        }
        assert_eq!(run(&d, &Goal { max_iterations: 0, ..goal(0.5, 0.5) }, &mut s).unwrap_err(), "at least one round must be allowed");
        assert_eq!(run(&d, &Goal { sharpen: true, ..goal(0.5, 0.5) }, &mut s).unwrap_err(), "sharpening the design is not done yet");
        assert!(run(&d, &goal(0.5, 0.0), &mut s).unwrap_err().starts_with("the radius to smooth the densities over must be more than 0"));
        d.passive.pop();
        assert!(run(&d, &goal(0.5, 0.5), &mut s).unwrap_err().starts_with("each element needs a volume, a middle and whether it stays solid"));
    }

    #[test]
    fn material_goes_to_the_stiffer_springs() {
        // 48 elements of one volume; a radius too small to take in a neighbour, so each is on
        // its own.
        let d = design([2, 2, 2], Vec3::new(2.0, 2.0, 2.0));
        let a = twice(48);
        let o = run(&d, &goal(0.5, 0.05), &mut Springs::new(a.clone())).unwrap();
        assert_eq!(o.stop, Stop::Settled);
        for (e, x) in o.density.iter().enumerate() {
            assert!(if a[e] == 2.0 { *x > 0.95 } else { *x < 0.05 }, "element {e} of stiffness {}: {x}", a[e]);
        }
        // All solid, the 72 of stiffness add up; half full, an eighth of it; at the end, nearly
        // the 48 of the stiffer half, the most half the material can be.
        assert!((o.solid_compliance - 100.0 / 72.0).abs() < 1e-12, "{}", o.solid_compliance);
        assert!((o.history[0].compliance - 100.0 / (72.0 * modulus(500, 1.0))).abs() < 1e-9);
        let last = o.history.last().unwrap().compliance;
        assert!(last > 100.0 / 48.0 && last < 1.02 * 100.0 / 48.0, "{last}");
    }

    #[test]
    fn the_sensitivity_matches_a_finite_difference_of_the_fake_work() {
        let mut d = design([3, 2, 2], Vec3::new(3.0, 2.0, 2.0));
        for e in [0, 5, 40] {
            d.passive[e] = true;
        }
        let n = d.volumes.len();
        let mut rng = Lcg(29);
        let mut s = Springs::new(rng.many(n, 0.5, 2.0));
        let filter = Filter::new(&d.centroids, &d.volumes, 1.3, &[]).unwrap();
        let x: Vec<f64> = d.passive.iter().map(|p| if *p { 1.0 } else { 0.3 + 0.6 * rng.next() }).collect();
        let seen = see(&filter, &d.passive, &x);
        let levels: Vec<u16> = seen.iter().map(|v| level_of(*v)).collect();
        let u = s.energies(&levels).unwrap();
        let dc = work_slope(&filter, &d.passive, &seen, &levels, &u);
        let work = |x: &[f64]| s.work(&see(&filter, &d.passive, x));
        let h = 1e-6;
        let fd: Vec<f64> = (0..n)
            .map(|j| {
                let (mut up, mut down) = (x.clone(), x.clone());
                up[j] += h;
                down[j] -= h;
                (work(&up) - work(&down)) / (2.0 * h)
            })
            .collect();
        let largest = fd.iter().fold(0.0, |a: f64, b| a.max(b.abs()));
        assert!(largest > 0.1, "{largest}");
        let mut checked = 0;
        for j in 0..n {
            if fd[j].abs() > 1e-4 * largest {
                assert!((dc[j] - fd[j]).abs() <= 0.01 * fd[j].abs(), "element {j}: {} against {}", dc[j], fd[j]);
                checked += 1;
            }
        }
        assert!(checked > n / 2, "{checked}");
        // And the volume kept, the same way round.
        let dv = filter.back(&free_volume(&d));
        for j in 0..n {
            let (mut up, mut down) = (x.clone(), x.clone());
            up[j] += h;
            down[j] -= h;
            let fd = (kept(&filter, &d, &up) - kept(&filter, &d, &down)) / (2.0 * h);
            assert!((dv[j] - fd).abs() <= 1e-6 * fd.abs().max(1e-3), "element {j}: {} against {fd}", dv[j]);
        }
        // Each element on its own, every fifth nearly empty, where its level is a long way from
        // its density (0.0137 is solved as 0.014): the slope goes with the density, the
        // energy was held at the level's stiffness.
        let filter = Filter::new(&d.centroids, &d.volumes, 0.05, &[]).unwrap();
        let x: Vec<f64> = (0..n)
            .map(|e| {
                if d.passive[e] {
                    1.0
                } else if e % 5 == 1 {
                    0.01 + 0.03 * rng.next()
                } else {
                    0.6 + 0.3 * rng.next()
                }
            })
            .collect();
        let seen = see(&filter, &d.passive, &x);
        let levels: Vec<u16> = seen.iter().map(|v| level_of(*v)).collect();
        let u = s.energies(&levels).unwrap();
        let dc = work_slope(&filter, &d.passive, &seen, &levels, &u);
        let mut worst: f64 = 0.0;
        for j in (0..n).filter(|j| *j % 5 == 1 && !d.passive[*j]) {
            let h = 1e-7;
            let (mut up, mut down) = (x.clone(), x.clone());
            up[j] += h;
            down[j] -= h;
            let fd = (s.work(&up) - s.work(&down)) / (2.0 * h);
            assert!((dc[j] - fd).abs() <= 0.01 * fd.abs(), "element {j} at {}: {} against {fd}", x[j], dc[j]);
            // (Where rounding to the level is a large share, the two differ much.)
            worst = worst.max((f64::from(levels[j]) / f64::from(LEVELS) / x[j] - 1.0).abs());
        }
        assert!(worst > 0.02, "{worst}");
    }

    #[test]
    fn compliance_never_rises_much() {
        let d = design([6, 3, 2], Vec3::new(12.0, 6.0, 4.0));
        // Stiffer toward one end, and a little this way and that.
        let a: Vec<f64> = d.centroids.iter().map(|c| 1.0 + 0.3 * c.x + 0.2 * (c.y * c.z).sin()).collect();
        let o = run(&d, &goal(0.4, 3.0), &mut Springs::new(a)).unwrap();
        assert!(o.history.len() > 5, "{:?}", o.stop);
        for w in o.history.windows(2) {
            assert!(w[1].compliance <= 1.02 * w[0].compliance, "{:?} then {:?}", w[0], w[1]);
        }
        let (first, last) = (o.history[0].compliance, o.history.last().unwrap().compliance);
        assert!(last < 0.5 * first && last > o.solid_compliance, "{first} {last} {}", o.solid_compliance);
    }

    #[test]
    fn the_loop_stops_when_the_work_levels_off() {
        let d = design([2, 2, 1], Vec3::new(2.0, 2.0, 1.0));
        let o = run(&d, &goal(0.5, 0.05), &mut swapping(|_| 1.0)).unwrap();
        assert_eq!((o.stop, o.history.len()), (Stop::LevelledOff, 1 + LEVEL_OFF_RUN));
        assert!(o.history.iter().all(|i| i.change > 0.1), "the design was still moving: {:?}", o.history);
        // Work that keeps changing by a little more than the share does not level off; a little
        // less does.
        let o = run(&d, &goal(0.5, 0.05), &mut swapping(|k| (1.0 + 1.1 * LEVEL_OFF).powi(k))).unwrap();
        assert_eq!((o.stop, o.history.len()), (Stop::Limit, 60));
        let o = run(&d, &goal(0.5, 0.05), &mut swapping(|k| (1.0 + 0.9 * LEVEL_OFF).powi(k))).unwrap();
        assert_eq!((o.stop, o.history.len()), (Stop::LevelledOff, 1 + LEVEL_OFF_RUN));
        // The rounds must run: level for rounds 1 to 4, a jump at the fifth, then level from it
        // to the tenth.
        let o = run(&d, &goal(0.5, 0.05), &mut swapping(|k| if k <= 5 { 1.0 } else { 2.0 })).unwrap();
        assert_eq!((o.stop, o.history.len()), (Stop::LevelledOff, 5 + LEVEL_OFF_RUN));
    }

    #[test]
    fn stopping_keeps_the_design_so_far() {
        let d = design([3, 2, 2], Vec3::new(3.0, 2.0, 2.0));
        let g = goal(0.4, 1.3);
        // The solver says it was stopped on its fourth call: the solid, two rounds, then this.
        let mut s = Springs::new(twice(72));
        s.stop_on = 4;
        let o = run(&d, &g, &mut s).unwrap();
        assert_eq!((o.stop, o.history.len()), (Stop::Stopped, 2));
        assert_eq!(o.density.iter().map(|x| level_of(*x)).collect::<Vec<_>>(), s.levels[2], "the design the last solve saw");
        assert!((o.solid_compliance - 100.0 / 108.0).abs() < 1e-12);
        // Stopped from outside while the second round is solved: that round is kept.
        let mut s = Springs::new(twice(72));
        s.cancel_on = 3;
        let cancel = s.cancel.clone();
        let p = optimize(&d, &g, &mut s, &cancel, &mut |_| {}).unwrap();
        assert_eq!((p.stop, p.history.len(), s.calls), (Stop::Stopped, 2, 3));
        assert_eq!(p.density, o.density);
    }

    #[test]
    fn stopping_before_the_first_iteration_says_stopped() {
        let d = design([2, 2, 1], Vec3::new(2.0, 2.0, 1.0));
        let g = goal(0.5, 0.05);
        let mut s = Springs::new(twice(24));
        assert_eq!(optimize(&d, &g, &mut s, &Arc::new(AtomicBool::new(true)), &mut |_| {}).unwrap_err(), STOPPED);
        assert_eq!(s.calls, 0, "nothing was solved");
        // The solver stopped while the part is solved solid, or while the first round is.
        for at in [1, 2] {
            let mut s = Springs::new(twice(24));
            s.stop_on = at;
            assert_eq!(run(&d, &g, &mut s).unwrap_err(), STOPPED);
            assert_eq!(s.calls, at);
        }
        // Asked to stop while the part is solved solid: the first round is not begun.
        let mut s = Springs::new(twice(24));
        s.cancel_on = 1;
        let cancel = s.cancel.clone();
        assert_eq!(optimize(&d, &g, &mut s, &cancel, &mut |_| {}).unwrap_err(), STOPPED);
        assert_eq!(s.calls, 1);
        // Asked while the first round is solved, that round is done and kept.
        let mut s = Springs::new(twice(24));
        s.cancel_on = 2;
        let cancel = s.cancel.clone();
        let o = optimize(&d, &g, &mut s, &cancel, &mut |_| {}).unwrap();
        assert_eq!((o.stop, o.history.len(), s.calls), (Stop::Stopped, 1, 2));
        assert_eq!(o.density, vec![0.5; 24]);
    }

    #[test]
    fn a_solver_that_fails_is_said_in_words() {
        let d = design([2, 2, 1], Vec3::new(2.0, 2.0, 1.0));
        let g = goal(0.5, 0.05);
        // A failure is not a stop, even once rounds are done.
        let mut s = Springs::new(twice(24));
        s.fail_on = 3;
        assert_eq!(run(&d, &g, &mut s).unwrap_err(), "the solver ran out of memory");
        let short = run(&d, &g, &mut Told(|_: &[u16]| Ok(vec![1.0; 3]))).unwrap_err();
        assert_eq!(short, "the solver told the energy of 3 elements, not of 24");
        let nan = run(&d, &g, &mut Told(|l: &[u16]| Ok(vec![f64::NAN; l.len()]))).unwrap_err();
        assert_eq!(nan, "the solver told an energy that is not a number");
        let none = run(&d, &g, &mut Told(|l: &[u16]| Ok(vec![0.0; l.len()]))).unwrap_err();
        assert_eq!(none, "the loads do no work on the part, so there is nothing to shape it for");
    }

    #[test]
    fn a_budget_that_runs_out_stops_the_loop() {
        let d = design([2, 2, 1], Vec3::new(2.0, 2.0, 1.0));
        let o = run(&d, &Goal { budget: Duration::ZERO, ..goal(0.5, 0.05) }, &mut Springs::new(twice(24))).unwrap();
        assert_eq!((o.stop, o.history.len()), (Stop::Budget, 1));
        // A solver that takes 20 ms a call: 50 ms are spent within two rounds.
        let mut s = Springs::new(twice(24));
        s.sleep = Duration::from_millis(20);
        let o = run(&d, &Goal { budget: Duration::from_millis(50), ..goal(0.5, 0.05) }, &mut s).unwrap();
        assert!(o.stop == Stop::Budget && o.history.len() <= 2, "{:?} after {}", o.stop, o.history.len());
        assert!(o.history.iter().all(|i| i.seconds >= 0.02));
        // With time enough, the design settles.
        assert_eq!(run(&d, &goal(0.5, 0.05), &mut Springs::new(twice(24))).unwrap().stop, Stop::Settled);
    }

    #[test]
    fn progress_is_told_each_iteration() {
        let d = design([2, 2, 1], Vec3::new(2.0, 2.0, 1.0));
        let mut told = Vec::new();
        let o = optimize(&d, &goal(0.5, 0.05), &mut Springs::new(twice(24)), &Cancel::default(), &mut |i| told.push(*i)).unwrap();
        assert!(told.len() > 3);
        assert_eq!(told, o.history);
        assert!(told.iter().enumerate().all(|(k, i)| i.number == k + 1 && i.sharpness == 1.0 && i.seconds >= 0.0));
        // The first round is the start, every element a half; the last nearly all solid or
        // empty.
        assert!((told[0].fraction - 0.5).abs() < 1e-15 && (told[0].grey - 1.0).abs() < 1e-15, "{:?}", told[0]);
        assert!(told.last().unwrap().grey < 0.05, "{:?}", told.last());
        assert!(told.windows(2).all(|w| w[1].grey < w[0].grey));
        // The first update: the stiffer half to sqrt 2 times the rest (the criterion's square
        // root of their stiffness ratio), half the volume kept.
        assert!((told[0].change - (2f64.sqrt() / (1.0 + 2f64.sqrt()) - 0.5)).abs() < 1e-9, "{}", told[0].change);
        assert!(told.last().unwrap().change < SETTLED);
    }

    #[test]
    fn the_same_inputs_give_the_same_design_bit_for_bit() {
        // A block and its mirror image, with the plane between them.
        let (half, hv) = block([3, 2, 2], Vec3::new(3.0, 2.0, 2.0));
        let m = Mirror { point: Vec3::ZERO, normal: Vec3::X };
        let n = half.len();
        let d = Design {
            centroids: half.iter().copied().chain(half.iter().map(|c| m.image(*c))).collect(),
            volumes: hv.iter().chain(&hv).copied().collect(),
            passive: (0..2 * n).map(|e| e % n == 4).collect(),
        };
        let a: Vec<f64> = (0..2 * n).map(|e| 1.0 + 0.5 * d.centroids[e % n].y + 0.1 * (e % n % 3) as f64).collect();
        let g = Goal { mirrors: vec![m], max_iterations: 45, ..goal(0.4, 1.3) };
        let (p, q) = (run(&d, &g, &mut Springs::new(a.clone())).unwrap(), run(&d, &g, &mut Springs::new(a)).unwrap());
        let bits = |v: &[f64]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&p.density), bits(&q.density));
        assert_eq!((p.solid_compliance.to_bits(), p.stop), (q.solid_compliance.to_bits(), q.stop));
        let rounds = |o: &Optimized| {
            o.history.iter().map(|i| (i.number, bits(&[i.compliance, i.fraction, i.change, i.sharpness, i.grey]))).collect::<Vec<_>>()
        };
        assert_eq!(rounds(&p), rounds(&q));
        assert!(p.history.len() > 5);
        // And the same on both sides of the plane.
        assert!((0..n).all(|e| (p.density[e] - p.density[e + n]).abs() < 1e-9));
    }
}

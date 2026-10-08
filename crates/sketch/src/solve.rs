//! Constraint solving behind the [`SketchSolver`] trait, plus degrees-of-freedom analysis.
//!
//! The default solver, [`GaussNewton`], follows CADCraft's design (damped, weighted
//! minimum-norm Gauss-Newton, conflict probing by removal, redundancy by Jacobian rank) on
//! Tenon's own sketch model. Parameters: two per point, one per circle radius. Arcs have no
//! parameters of their own; an implicit row keeps their end on the circle through their start.

use std::collections::{BTreeSet, HashMap};

use tenon_geom::Vec2;

use crate::lm;
use crate::sketch::{SketchError, SketchResult};
use crate::{Constraint, ConstraintId, EntityId, Geometry, Sketch};

/// Most iterations of one solve.
const MAX_ITER: usize = 100;
/// Constraints tried one by one when looking for the cause of a conflict.
const MAX_CONFLICT_PROBES: usize = 48;
/// Weight of a dragged point: it moves least, so it ends where the pointer is when it can.
const DRAG_WEIGHT: f64 = 1e4;

/// What to keep still while solving.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SolveOptions {
    /// Move this point to this position and keep it as close to it as the constraints allow.
    pub drag: Option<(EntityId, Vec2)>,
    /// Points (or circles) to move as little as possible.
    pub keep: Vec<EntityId>,
}

/// Degrees of freedom of a sketch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dof {
    /// Remaining degrees of freedom (0 = fully constrained).
    pub dof: usize,
    /// Entities whose every parameter is determined by the constraints.
    pub fully_constrained: BTreeSet<EntityId>,
}

/// A sketch constraint solver. Implementations must leave the sketch unchanged on error.
pub trait SketchSolver {
    /// Solves all constraints and writes the geometry back.
    fn solve(&self, sketch: &mut Sketch, opts: &SolveOptions) -> SketchResult<()>;
    /// Degrees of freedom at the current geometry.
    fn dof(&self, sketch: &Sketch) -> SketchResult<Dof>;
    /// Error if `new` adds no independent equation to the others (redundant).
    fn check_redundant(&self, sketch: &Sketch, new: ConstraintId) -> SketchResult<()>;
}

/// The default solver.
#[derive(Clone, Copy, Debug, Default)]
pub struct GaussNewton;

fn dir_n(a: Vec2, b: Vec2) -> Vec2 {
    let d = b - a;
    let l = d.len();
    if l > 1e-12 { d * (1.0 / l) } else { d }
}

fn sgn(v: f64) -> f64 {
    if v < 0.0 { -1.0 } else { 1.0 }
}

fn wrap(a: f64) -> f64 {
    let r = (a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
    if r.is_finite() { r } else { 0.0 }
}

/// A constraint prepared for evaluation: its residual row count and the orientation chosen from
/// the starting geometry.
#[derive(Clone, Debug)]
struct Built {
    id: ConstraintId,
    c: Constraint,
    rows: usize,
    /// Orientation (+-1) or, for circle tangency, +1 external / -1 internal.
    sign: f64,
}

/// Parameter layout and residual system of one sketch.
struct System<'s> {
    sketch: &'s Sketch,
    point_at: HashMap<EntityId, usize>,
    radius_at: HashMap<EntityId, usize>,
    x0: Vec<f64>,
    arcs: Vec<(EntityId, EntityId, EntityId)>,
    built: Vec<Built>,
    locked: Vec<bool>,
}

impl<'s> System<'s> {
    fn new(sketch: &'s Sketch) -> SketchResult<Self> {
        let mut sys = System {
            sketch,
            point_at: HashMap::new(),
            radius_at: HashMap::new(),
            x0: Vec::new(),
            arcs: Vec::new(),
            built: Vec::new(),
            locked: Vec::new(),
        };
        for (id, e) in sketch.entities() {
            match &e.geometry {
                Geometry::Point { pos } => {
                    sys.point_at.insert(id, sys.x0.len());
                    sys.x0.extend([pos.x, pos.y]);
                }
                Geometry::Circle { radius, .. } => {
                    sys.radius_at.insert(id, sys.x0.len());
                    sys.x0.push(*radius);
                }
                Geometry::Arc { center, start, end } => sys.arcs.push((*center, *start, *end)),
                _ => {}
            }
        }
        sys.locked = vec![false; sys.x0.len()];
        for (id, c) in sketch.constraints() {
            sketch.check_constraint(c)?;
            if let Constraint::Fix { point } = c {
                if let Some(&o) = sys.point_at.get(point) {
                    for j in [o, o + 1] {
                        if let Some(l) = sys.locked.get_mut(j) {
                            *l = true;
                        }
                    }
                }
                continue;
            }
            let b = sys.build(id, c.clone());
            sys.built.push(b);
        }
        Ok(sys)
    }

    /// A copy sharing the sketch, for solving variants (e.g. with extra locks).
    fn shallow(&self) -> System<'s> {
        System {
            sketch: self.sketch,
            point_at: self.point_at.clone(),
            radius_at: self.radius_at.clone(),
            x0: self.x0.clone(),
            arcs: self.arcs.clone(),
            built: self.built.clone(),
            locked: self.locked.clone(),
        }
    }

    fn pt(&self, x: &[f64], id: EntityId) -> Option<Vec2> {
        let o = *self.point_at.get(&id)?;
        Some(Vec2::new(*x.get(o)?, *x.get(o + 1)?))
    }
    fn line(&self, x: &[f64], id: EntityId) -> Option<(Vec2, Vec2)> {
        match self.sketch.geometry(id)? {
            Geometry::Line { start, end } => Some((self.pt(x, *start)?, self.pt(x, *end)?)),
            _ => None,
        }
    }
    fn circ(&self, x: &[f64], id: EntityId) -> Option<(Vec2, f64)> {
        match self.sketch.geometry(id)? {
            Geometry::Circle { center, .. } => Some((self.pt(x, *center)?, *x.get(*self.radius_at.get(&id)?)?)),
            Geometry::Arc { center, start, .. } => {
                let c = self.pt(x, *center)?;
                Some((c, self.pt(x, *start)?.dist(c)))
            }
            _ => None,
        }
    }

    fn build(&self, id: ConstraintId, c: Constraint) -> Built {
        use Constraint::*;
        let x = &self.x0;
        let rows = match &c {
            Coincident { .. } | Concentric { .. } | Symmetric { .. } | Midpoint { .. } | Collinear { .. } => 2,
            Fix { .. } => 0,
            _ => 1,
        };
        let mut sign = 1.0;
        match &c {
            Tangent { a, b } => {
                if let (Some((p, q)), Some((cc, _))) = (self.line(x, *a), self.circ(x, *b)) {
                    sign = sgn(dir_n(p, q).cross(cc - p));
                } else if let (Some((cc, _)), Some((p, q))) = (self.circ(x, *a), self.line(x, *b)) {
                    sign = sgn(dir_n(p, q).cross(cc - p));
                } else if let (Some((c1, r1)), Some((c2, r2))) = (self.circ(x, *a), self.circ(x, *b)) {
                    sign = if c1.dist(c2) >= r1.max(r2) { 1.0 } else { -1.0 };
                }
            }
            Distance { a, b, .. } => {
                if let (Some(p), Some((la, lb))) = (self.pt(x, *a), self.line(x, *b)) {
                    sign = sgn(dir_n(la, lb).cross(p - la));
                } else if let (Some((la, lb)), Some(p)) = (self.line(x, *a), self.pt(x, *b)) {
                    sign = sgn(dir_n(la, lb).cross(p - la));
                }
            }
            Angle { a, b, .. } => {
                if let (Some((a1, b1)), Some((a2, b2))) = (self.line(x, *a), self.line(x, *b)) {
                    let (d1, d2) = (dir_n(a1, b1), dir_n(a2, b2));
                    sign = sgn(d1.cross(d2).atan2(d1.dot(d2)));
                }
            }
            _ => {}
        }
        Built { id, c, rows, sign }
    }

    /// Appends exactly `b.rows` residuals.
    fn eval_one(&self, b: &Built, x: &[f64], out: &mut Vec<f64>) {
        let start = out.len();
        let _ = self.eval_inner(b, x, out);
        out.resize(start + b.rows, 0.0);
    }

    fn eval_inner(&self, b: &Built, x: &[f64], out: &mut Vec<f64>) -> Option<()> {
        use Constraint::*;
        let pt = |id| self.pt(x, id);
        let ln = |id| self.line(x, id);
        let ci = |id| self.circ(x, id);
        match &b.c {
            Coincident { a, b } => {
                let (p, q) = (pt(*a)?, pt(*b)?);
                out.extend([p.x - q.x, p.y - q.y]);
            }
            PointOnCurve { point, curve } => {
                let p = pt(*point)?;
                if let Some((a, bb)) = ln(*curve) {
                    out.push(dir_n(a, bb).cross(p - a));
                } else {
                    let (c, r) = ci(*curve)?;
                    out.push(p.dist(c) - r);
                }
            }
            Horizontal { line } => {
                let (a, bb) = ln(*line)?;
                out.push(a.y - bb.y);
            }
            Vertical { line } => {
                let (a, bb) = ln(*line)?;
                out.push(a.x - bb.x);
            }
            Parallel { a, b } => {
                let ((a1, b1), (a2, b2)) = (ln(*a)?, ln(*b)?);
                out.push(dir_n(a1, b1).cross(dir_n(a2, b2)));
            }
            Perpendicular { a, b } => {
                let ((a1, b1), (a2, b2)) = (ln(*a)?, ln(*b)?);
                out.push(dir_n(a1, b1).dot(dir_n(a2, b2)));
            }
            Collinear { a, b } => {
                let ((a1, b1), (a2, b2)) = (ln(*a)?, ln(*b)?);
                let d = dir_n(a1, b1);
                out.extend([d.cross(a2 - a1), d.cross(b2 - a1)]);
            }
            Tangent { a, b: bb } => {
                if let (Some((p, q)), Some((c, r))) = (ln(*a), ci(*bb)) {
                    out.push(b.sign * dir_n(p, q).cross(c - p) - r);
                } else if let (Some((c, r)), Some((p, q))) = (ci(*a), ln(*bb)) {
                    out.push(b.sign * dir_n(p, q).cross(c - p) - r);
                } else {
                    let ((c1, r1), (c2, r2)) = (ci(*a)?, ci(*bb)?);
                    let d = c1.dist(c2);
                    out.push(if b.sign > 0.0 { d - (r1 + r2) } else { d - (r1 - r2).abs() });
                }
            }
            Concentric { a, b } => {
                let ((c1, _), (c2, _)) = (ci(*a)?, ci(*b)?);
                out.extend([c1.x - c2.x, c1.y - c2.y]);
            }
            Equal { a, b } => {
                if let (Some((a1, b1)), Some((a2, b2))) = (ln(*a), ln(*b)) {
                    out.push(a1.dist(b1) - a2.dist(b2));
                } else {
                    let ((_, r1), (_, r2)) = (ci(*a)?, ci(*b)?);
                    out.push(r1 - r2);
                }
            }
            Symmetric { a, b, axis } => {
                let (p, q, (la, lb)) = (pt(*a)?, pt(*b)?, ln(*axis)?);
                let d = dir_n(la, lb);
                out.extend([d.cross(p.mid(q) - la), d.dot(q - p)]);
            }
            Midpoint { point, line } => {
                let (p, (a, bb)) = (pt(*point)?, ln(*line)?);
                let m = a.mid(bb);
                out.extend([p.x - m.x, p.y - m.y]);
            }
            Fix { .. } => {}
            Distance { a, b: bb, value } => {
                if let (Some(p), Some(q)) = (pt(*a), pt(*bb)) {
                    out.push(p.dist(q) - value);
                } else if let (Some(p), Some((la, lb))) = (pt(*a), ln(*bb)) {
                    out.push(b.sign * dir_n(la, lb).cross(p - la) - value);
                } else {
                    let ((la, lb), p) = (ln(*a)?, pt(*bb)?);
                    out.push(b.sign * dir_n(la, lb).cross(p - la) - value);
                }
            }
            HorizontalDistance { a, b, value } => out.push(pt(*b)?.x - pt(*a)?.x - value),
            VerticalDistance { a, b, value } => out.push(pt(*b)?.y - pt(*a)?.y - value),
            Length { line, value } => {
                let (a, bb) = ln(*line)?;
                out.push(a.dist(bb) - value);
            }
            Angle { a, b: bb, value } => {
                let ((a1, b1), (a2, b2)) = (ln(*a)?, ln(*bb)?);
                let (d1, d2) = (dir_n(a1, b1), dir_n(a2, b2));
                out.push(wrap(d1.cross(d2).atan2(d1.dot(d2)) - b.sign * value));
            }
            Radius { curve, value } => out.push(ci(*curve)?.1 - value),
            Diameter { curve, value } => out.push(2.0 * ci(*curve)?.1 - value),
        }
        Some(())
    }

    /// All residuals, skipping constraint index `skip`. Arc rows come first.
    fn eval(&self, x: &[f64], out: &mut Vec<f64>, skip: Option<usize>) {
        for (c, s, e) in &self.arcs {
            let v = match (self.pt(x, *c), self.pt(x, *s), self.pt(x, *e)) {
                (Some(c), Some(s), Some(e)) => e.dist(c) - s.dist(c),
                _ => 0.0,
            };
            out.push(v);
        }
        for (i, b) in self.built.iter().enumerate() {
            if Some(i) != skip {
                self.eval_one(b, x, out);
            }
        }
    }

    fn rows(&self, skip: Option<usize>) -> usize {
        self.arcs.len() + self.built.iter().enumerate().filter(|(i, _)| Some(*i) != skip).map(|(_, b)| b.rows).sum::<usize>()
    }

    fn free(&self) -> Vec<usize> {
        self.locked.iter().enumerate().filter(|(_, l)| !**l).map(|(i, _)| i).collect()
    }

    /// Convergence tolerance on residuals: relative to the sketch's size and well below the
    /// kernel's coincidence tolerance (`tol::LINEAR`), so solved joints stay joined in 3D.
    fn tolerance(&self) -> f64 {
        1e-11 * self.x0.iter().fold(1.0f64, |m, v| m.max(v.abs()))
    }

    fn weights(&self, opts: &SolveOptions) -> Vec<f64> {
        let mut w = vec![1.0; self.x0.len()];
        let mut heavy = |id: &EntityId, weight: f64| {
            for o in [self.point_at.get(id).map(|o| (*o, 2)), self.radius_at.get(id).map(|o| (*o, 1))].into_iter().flatten() {
                for j in o.0..o.0 + o.1 {
                    if let Some(v) = w.get_mut(j) {
                        *v = weight;
                    }
                }
            }
        };
        for id in &opts.keep {
            heavy(id, 1e3);
        }
        if let Some((id, _)) = &opts.drag {
            heavy(id, DRAG_WEIGHT);
        }
        w
    }

    fn run(&self, x0: &[f64], weights: &[f64], skip: Option<usize>) -> SketchResult<lm::Outcome> {
        let rows = self.rows(skip);
        let free = self.free();
        if rows > lm::MAX_ROWS || (rows as f64) * (rows as f64) * (free.len() as f64) > 4e8 {
            return Err(SketchError::TooLarge);
        }
        let f = |x: &[f64], out: &mut Vec<f64>| self.eval(x, out, skip);
        Ok(lm::solve(&f, x0, &free, weights, self.tolerance(), MAX_ITER))
    }

    /// Constraints whose removal makes the rest solvable.
    fn conflicts(&self, x0: &[f64], weights: &[f64], x_failed: &[f64]) -> Vec<ConstraintId> {
        let mut ids = Vec::new();
        if self.built.len() <= MAX_CONFLICT_PROBES {
            for (i, b) in self.built.iter().enumerate() {
                if self.run(x0, weights, Some(i)).is_ok_and(|o| o.converged) {
                    ids.push(b.id);
                }
            }
        }
        if ids.is_empty() {
            let tol = self.tolerance() * 10.0;
            for b in &self.built {
                let mut out = Vec::new();
                self.eval_one(b, x_failed, &mut out);
                if lm::max_abs(&out) > tol {
                    ids.push(b.id);
                }
            }
        }
        ids
    }

    fn jacobian(&self, x: &[f64]) -> (Vec<f64>, usize, Vec<usize>) {
        let rows = self.rows(None);
        let free = self.free();
        let f = |x: &[f64], out: &mut Vec<f64>| self.eval(x, out, None);
        (lm::jacobian(&f, x, &free, rows), rows, free)
    }
}

/// Indices of parameters lying in the row space of `jac` (rows x cols), i.e. determined by the
/// equations: modified Gram-Schmidt on the rows, then the squared projection of each unit
/// vector onto the basis.
fn determined_columns(jac: &[f64], rows: usize, cols: usize) -> Vec<bool> {
    let mut basis: Vec<Vec<f64>> = Vec::new();
    for i in 0..rows {
        let Some(row) = jac.get(i * cols..(i + 1) * cols) else { continue };
        let mut v = row.to_vec();
        let n0 = v.iter().map(|a| a * a).sum::<f64>().sqrt();
        if n0 <= 1e-12 {
            continue;
        }
        v.iter_mut().for_each(|a| *a /= n0);
        for q in &basis {
            let d: f64 = v.iter().zip(q).map(|(a, b)| a * b).sum();
            v.iter_mut().zip(q).for_each(|(a, b)| *a -= d * b);
        }
        let n = v.iter().map(|a| a * a).sum::<f64>().sqrt();
        if n > 1e-7 {
            v.iter_mut().for_each(|a| *a /= n);
            basis.push(v);
        }
    }
    (0..cols).map(|j| basis.iter().map(|q| q.get(j).map_or(0.0, |v| v * v)).sum::<f64>() > 1.0 - 1e-6).collect()
}

fn write_back(sketch: &mut Sketch, sys_point_at: &HashMap<EntityId, usize>, sys_radius_at: &HashMap<EntityId, usize>, x: &[f64]) -> SketchResult<()> {
    for (id, &o) in sys_point_at {
        if let (Some(&px), Some(&py)) = (x.get(o), x.get(o + 1)) {
            sketch.set_point(*id, Vec2::new(px, py))?;
        }
    }
    for (id, &o) in sys_radius_at {
        if let Some(&r) = x.get(o) {
            sketch.set_radius(*id, r)?;
        }
    }
    Ok(())
}

impl SketchSolver for GaussNewton {
    fn solve(&self, sketch: &mut Sketch, opts: &SolveOptions) -> SketchResult<()> {
        let (point_at, radius_at, x) = {
            let sys = System::new(sketch)?;
            let mut x0 = sys.x0.clone();
            if let Some((id, target)) = opts.drag {
                let o = *sys.point_at.get(&id).ok_or_else(|| SketchError::Invalid(format!("{id} is not a point")))?;
                if !(target.x.is_finite() && target.y.is_finite()) {
                    return Err(SketchError::Invalid("drag target must be finite".into()));
                }
                x0[o] = target.x;
                x0[o + 1] = target.y;
            }
            let weights = sys.weights(opts);
            // A dragged point goes exactly where the pointer is whenever the constraints allow:
            // first solve with it pinned there, and only if that fails let it trail behind.
            let pinned = opts.drag.and_then(|(id, _)| {
                let o = *sys.point_at.get(&id)?;
                let mut pinned = sys.shallow();
                for j in [o, o + 1] {
                    if let Some(l) = pinned.locked.get_mut(j) {
                        *l = true;
                    }
                }
                pinned.run(&x0, &weights, None).ok().filter(|r| r.converged)
            });
            let o = match pinned {
                Some(o) => o,
                None => sys.run(&x0, &weights, None)?,
            };
            if !o.converged {
                return Err(SketchError::Conflict(sys.conflicts(&x0, &weights, &o.x)));
            }
            (sys.point_at.clone(), sys.radius_at.clone(), o.x)
        };
        // Write back on a copy so a failure leaves the sketch untouched.
        let mut next = sketch.clone();
        write_back(&mut next, &point_at, &radius_at, &x)?;
        *sketch = next;
        Ok(())
    }

    fn dof(&self, sketch: &Sketch) -> SketchResult<Dof> {
        let sys = System::new(sketch)?;
        let (jac, rows, free) = sys.jacobian(&sys.x0);
        let rank = lm::rank(&jac, rows, free.len());
        let det = determined_columns(&jac, rows, free.len());
        let mut fixed = sys.locked.clone();
        for (k, &j) in free.iter().enumerate() {
            if det.get(k).copied().unwrap_or(false)
                && let Some(f) = fixed.get_mut(j)
            {
                *f = true;
            }
        }
        let param_fixed = |o: usize, n: usize| (o..o + n).all(|j| fixed.get(j).copied().unwrap_or(false));
        let point_fixed = |id: &EntityId| sys.point_at.get(id).is_some_and(|o| param_fixed(*o, 2));
        let mut fully = BTreeSet::new();
        for (id, e) in sketch.entities() {
            let done = match &e.geometry {
                Geometry::Point { .. } => point_fixed(&id),
                Geometry::Circle { center, .. } => point_fixed(center) && sys.radius_at.get(&id).is_some_and(|o| param_fixed(*o, 1)),
                g => g.points().iter().all(point_fixed),
            };
            if done {
                fully.insert(id);
            }
        }
        Ok(Dof { dof: free.len().saturating_sub(rank), fully_constrained: fully })
    }

    fn check_redundant(&self, sketch: &Sketch, new: ConstraintId) -> SketchResult<()> {
        let sys = System::new(sketch)?;
        let Some(pos) = sys.built.iter().position(|b| b.id == new) else {
            return Ok(()); // Fix and unknown ids add no rows.
        };
        let nb = &sys.built[pos];
        if nb.rows == 0 {
            return Ok(());
        }
        let free = sys.free();
        let rows_all = sys.rows(None);
        let rows_old = rows_all - nb.rows;
        let f_old = |x: &[f64], out: &mut Vec<f64>| sys.eval(x, out, Some(pos));
        let j_old = lm::jacobian(&f_old, &sys.x0, &free, rows_old);
        let (j_all, _, _) = sys.jacobian(&sys.x0);
        let (r_old, r_all) = (lm::rank(&j_old, rows_old, free.len()), lm::rank(&j_all, rows_all, free.len()));
        if r_all < r_old + nb.rows {
            let mine = nb.c.refs();
            let related = sys.built.iter().filter(|b| b.id != new && b.c.refs().iter().any(|r| mine.contains(r))).map(|b| b.id).collect();
            return Err(SketchError::Redundant(related));
        }
        Ok(())
    }
}

impl Sketch {
    /// Solves with the default solver.
    pub fn solve(&mut self) -> SketchResult<()> {
        GaussNewton.solve(self, &SolveOptions::default())
    }

    /// Degrees of freedom with the default solver.
    pub fn dof(&self) -> SketchResult<Dof> {
        GaussNewton.dof(self)
    }

    /// Adds a constraint, solves, and rejects it (leaving the sketch unchanged) if it conflicts
    /// with the others or is redundant.
    pub fn add_constraint(&mut self, c: Constraint) -> SketchResult<ConstraintId> {
        self.add_constraint_with(&GaussNewton, c)
    }

    pub fn add_constraint_with(&mut self, solver: &dyn SketchSolver, c: Constraint) -> SketchResult<ConstraintId> {
        let mut trial = self.clone();
        let id = trial.push_constraint(c)?;
        match solver.solve(&mut trial, &SolveOptions::default()) {
            Ok(()) => {}
            Err(SketchError::Conflict(mut ids)) => {
                ids.retain(|i| *i != id);
                ids.insert(0, id);
                return Err(SketchError::Conflict(ids));
            }
            Err(e) => return Err(e),
        }
        solver.check_redundant(&trial, id)?;
        *self = trial;
        Ok(id)
    }

    /// Changes a dimension and solves; unchanged on failure.
    pub fn set_dimension(&mut self, id: ConstraintId, value: f64) -> SketchResult<()> {
        let mut trial = self.clone();
        let c = trial.constraint_mut(id).ok_or(SketchError::NoConstraint(id))?;
        if !c.set_value(value) {
            return Err(SketchError::Invalid(format!("{id} is not a dimension")));
        }
        let c = c.clone();
        trial.check_constraint(&c)?;
        GaussNewton.solve(&mut trial, &SolveOptions::default())?;
        *self = trial;
        Ok(())
    }

    /// Drags a point towards `target` while keeping every constraint; unchanged on failure.
    pub fn drag(&mut self, point: EntityId, target: Vec2) -> SketchResult<()> {
        GaussNewton.solve(self, &SolveOptions { drag: Some((point, target)), keep: vec![] })
    }

    /// The current value of a dimensional constraint's measured quantity (radians for angles).
    pub fn measure(&self, c: &Constraint) -> Option<f64> {
        use Constraint::*;
        let pt = |id| self.point(id);
        Some(match c {
            Distance { a, b, .. } => match (pt(*a), pt(*b), self.line(*a), self.line(*b)) {
                (Some(p), Some(q), _, _) => p.dist(q),
                (Some(p), _, _, Some((la, lb))) | (_, Some(p), Some((la, lb)), _) => dir_n(la, lb).cross(p - la).abs(),
                _ => return None,
            },
            HorizontalDistance { a, b, .. } => pt(*b)?.x - pt(*a)?.x,
            VerticalDistance { a, b, .. } => pt(*b)?.y - pt(*a)?.y,
            Length { line, .. } => {
                let (a, b) = self.line(*line)?;
                a.dist(b)
            }
            Angle { a, b, .. } => {
                let ((a1, b1), (a2, b2)) = (self.line(*a)?, self.line(*b)?);
                let (d1, d2) = (dir_n(a1, b1), dir_n(a2, b2));
                d1.cross(d2).atan2(d1.dot(d2)).abs()
            }
            Radius { curve, .. } => self.circle(*curve)?.1,
            Diameter { curve, .. } => 2.0 * self.circle(*curve)?.1,
            _ => return None,
        })
    }
}

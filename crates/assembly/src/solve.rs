//! The assembly solver: places components so their relationships hold, and counts the degrees of
//! freedom each component has left.
//!
//! Every free component has six unknowns: a translation and a rotation vector, applied about its
//! centre to where it stands now. The equations are the residuals of the relationships, solved by
//! the damped minimum-norm Gauss-Newton of the sketch solver ([`tenon_sketch::lm`]): components
//! move as little as possible, measured as the motion of their points, and a component being
//! dragged moves least of all.

use std::f64::consts::PI;

use tenon_geom::{Frame, Vec3, tol};
use tenon_sketch::lm;

use crate::geometry::Prim;
use crate::math::{M3, frame_of, rotation_between, span_basis, sym_eigen};
use crate::model::JointKind;

/// Most iterations of one solve.
const MAX_ITER: usize = 200;
/// Weight of a dragged component's unknowns: it stays as near the pointer as it may.
const DRAG_WEIGHT: f64 = 1e4;

/// A component as the solver sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    /// Part coordinates to assembly coordinates.
    pub frame: Frame,
    /// Grounded: never moves.
    pub fixed: bool,
    /// Where it turns about (assembly coordinates): the centre of its bounding box.
    pub center: Vec3,
    /// Its size (bounding-box diagonal, mm), to weigh turning against sliding.
    pub size: f64,
}

/// What a relationship requires.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Law {
    Mate {
        offset: f64,
    },
    Flush {
        offset: f64,
    },
    /// `reference` in A's part coordinates.
    Angle {
        angle: f64,
        reference: Vec3,
    },
    Insert {
        offset: f64,
        aligned: bool,
    },
    Joint {
        joint: JointKind,
        flip: bool,
        offset: f64,
        angle: f64,
    },
}

/// One side of a relationship: a body (`None`: the assembly itself) and geometry in its part
/// coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct End {
    pub body: Option<usize>,
    pub prim: Prim,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rel {
    pub law: Law,
    pub a: End,
    pub b: End,
}

/// A rigid placement during solving.
#[derive(Clone, Copy, Debug)]
struct Pose {
    r: M3,
    t: Vec3,
}

impl Pose {
    fn of(f: &Frame) -> Pose {
        Pose { r: M3::of_frame(f), t: f.origin() }
    }
    fn frame(&self) -> Option<Frame> {
        frame_of(&self.r, self.t)
    }
}

/// Whether two kinds of geometry can take this law; the message says what fits.
pub fn check(law: &Law, a: &Prim, b: &Prim) -> Result<(), String> {
    let mate = |p: &Prim| match p {
        Prim::Circle { center, normal, .. } => Prim::Line { point: *center, dir: *normal },
        x => *x,
    };
    match law {
        Law::Mate { .. } => match (mate(a), mate(b)) {
            (Prim::Line { .. } | Prim::Plane { .. } | Prim::Point(_), _) => Ok(()),
            _ => Err("mate needs planes, axes or points".into()),
        },
        Law::Flush { .. } => match (a, b) {
            (Prim::Plane { .. }, Prim::Plane { .. }) => Ok(()),
            _ => Err("flush needs two planar faces or planes".into()),
        },
        Law::Angle { .. } => match (a.direction(), b.direction()) {
            (Some(_), Some(_)) => Ok(()),
            _ => Err("angle needs planes, axes or edges, not points".into()),
        },
        Law::Insert { .. } => match (a, b) {
            (Prim::Circle { .. }, Prim::Circle { .. }) => Ok(()),
            _ => Err("insert needs two circular edges".into()),
        },
        Law::Joint { .. } => Ok(()),
    }
}

fn perp(v: Vec3, d: Vec3) -> Vec3 {
    v - d * v.dot(d)
}

fn push3(out: &mut Vec<f64>, v: Vec3) {
    out.extend_from_slice(&[v.x, v.y, v.z]);
}

/// `a` wrapped into (-pi, pi].
fn wrap(a: f64) -> f64 {
    let w = (a + PI).rem_euclid(2.0 * PI) - PI;
    if w <= -PI { w + 2.0 * PI } else { w }
}

/// Angle from `xa` to `xb` turning about `axis`.
fn turn(xa: Vec3, xb: Vec3, axis: Vec3) -> f64 {
    xa.cross(xb).dot(axis).atan2(xa.dot(xb))
}

fn mate_prim(p: Prim) -> Prim {
    match p {
        Prim::Circle { center, normal, .. } => Prim::Line { point: center, dir: normal },
        x => x,
    }
}

/// A joint frame in assembly coordinates: origin, X, Z.
fn joint_frame(prim: &Prim, pose: &Pose) -> (Vec3, Vec3, Vec3) {
    let f = prim.joint_frame();
    (pose.r.apply(f.origin()) + pose.t, pose.r.apply(f.x()), pose.r.apply(f.z()))
}

/// Appends the residual rows of one relationship.
fn residual(rel: &Rel, poses: &[Pose], out: &mut Vec<f64>) {
    let world = Pose { r: M3::IDENTITY, t: Vec3::ZERO };
    let pose = |e: &End| e.body.and_then(|i| poses.get(i).copied()).unwrap_or(world);
    let (qa, qb) = (pose(&rel.a), pose(&rel.b));
    let (pa, pb) = (rel.a.prim.placed(&qa.r, qa.t), rel.b.prim.placed(&qb.r, qb.t));
    match rel.law {
        Law::Mate { offset } => match (mate_prim(pa), mate_prim(pb)) {
            (Prim::Plane { point: a, normal: na }, Prim::Plane { point: b, normal: nb }) => {
                push3(out, na + nb);
                out.push((b - a).dot(na) - offset);
            }
            (Prim::Line { point: a, dir: da }, Prim::Line { point: b, dir: db }) => {
                push3(out, da.cross(db));
                push3(out, perp(b - a, da));
            }
            (Prim::Point(a), Prim::Point(b)) => push3(out, b - a),
            (Prim::Plane { point, normal }, Prim::Point(q)) => out.push((q - point).dot(normal) - offset),
            (Prim::Point(q), Prim::Plane { point, normal }) => out.push((q - point).dot(normal) - offset),
            (Prim::Line { point, dir }, Prim::Point(q)) | (Prim::Point(q), Prim::Line { point, dir }) => push3(out, perp(q - point, dir)),
            (Prim::Plane { point: pp, normal }, Prim::Line { point: pl, dir })
            | (Prim::Line { point: pl, dir }, Prim::Plane { point: pp, normal }) => {
                out.push(dir.dot(normal));
                out.push((pl - pp).dot(normal) - offset);
            }
            _ => {}
        },
        Law::Flush { offset } => {
            if let (Prim::Plane { point: a, normal: na }, Prim::Plane { point: b, normal: nb }) = (pa, pb) {
                push3(out, na - nb);
                out.push((b - a).dot(na) - offset);
            }
        }
        Law::Angle { angle, reference } => {
            if let (Some(ua), Some(ub)) = (pa.direction(), pb.direction()) {
                let r = qa.r.apply(reference);
                out.push(wrap(turn(ua, ub, r) - angle));
            }
        }
        Law::Insert { offset, aligned } => {
            if let (Prim::Circle { center: ca, normal: na, .. }, Prim::Circle { center: cb, normal: nb, .. }) = (pa, pb) {
                push3(out, if aligned { na - nb } else { na + nb });
                push3(out, cb - ca - na * offset);
            }
        }
        Law::Joint { joint, flip, offset, angle } => {
            let (oa, xa, za) = joint_frame(&rel.a.prim, &qa);
            let (ob, xb, zb) = joint_frame(&rel.b.prim, &qb);
            let s = if flip { 1.0 } else { -1.0 };
            if joint != JointKind::Ball {
                push3(out, zb - za * s);
            }
            match joint {
                JointKind::Rigid | JointKind::Revolute => push3(out, ob - oa - za * offset),
                JointKind::Slider | JointKind::Cylindrical => push3(out, perp(ob - oa, za)),
                JointKind::Planar => out.push((ob - oa).dot(za) - offset),
                JointKind::Ball => push3(out, ob - oa),
            }
            if matches!(joint, JointKind::Rigid | JointKind::Slider) {
                out.push(wrap(turn(xa, xb, za) - angle));
            }
        }
    }
}

/// The unknowns: for each body, its parameter offset (`None`: it does not move).
fn layout(bodies: &[Body], rels: &[Rel]) -> Vec<Option<usize>> {
    let used: std::collections::BTreeSet<usize> = rels.iter().flat_map(|r| [r.a.body, r.b.body]).flatten().collect();
    let mut next = 0;
    bodies
        .iter()
        .enumerate()
        .map(|(i, b)| {
            (!b.fixed && used.contains(&i)).then(|| {
                let k = next;
                next += 6;
                k
            })
        })
        .collect()
}

fn poses_at(x: &[f64], bodies: &[Body], index: &[Option<usize>]) -> Vec<Pose> {
    bodies
        .iter()
        .zip(index)
        .map(|(b, k)| {
            let base = Pose::of(&b.frame);
            let Some(k) = *k else { return base };
            let v = |i: usize| x.get(k + i).copied().unwrap_or(0.0);
            let (t, w) = (Vec3::new(v(0), v(1), v(2)), Vec3::new(v(3), v(4), v(5)));
            let rw = M3::exp(w);
            Pose { r: rw.mul(&base.r), t: b.center + rw.apply(base.t - b.center) + t }
        })
        .collect()
}

fn residuals(rels: &[Rel], poses: &[Pose], out: &mut Vec<f64>) {
    for r in rels {
        residual(r, poses, out);
    }
}

/// The result of a solve.
#[derive(Clone, Debug, PartialEq)]
pub struct Solution {
    /// New frames, one per body (fixed bodies unchanged).
    pub frames: Vec<Frame>,
    pub converged: bool,
    /// For each relationship, its largest remaining residual.
    pub errors: Vec<f64>,
}

fn rel_errors(rels: &[Rel], poses: &[Pose]) -> Vec<f64> {
    let mut out = Vec::new();
    rels.iter()
        .map(|r| {
            out.clear();
            residual(r, poses, &mut out);
            lm::max_abs(&out)
        })
        .collect()
}

fn half_size(b: &Body) -> f64 {
    (b.size * 0.5).max(1.0)
}

/// Solves all relationships. `drag`: a body the user moved, which stays as near where it was put
/// as the relationships allow.
pub fn solve(bodies: &[Body], rels: &[Rel], drag: Option<usize>) -> Solution {
    let index = layout(bodies, rels);
    let n = index.iter().flatten().count() * 6;
    let mut weights = vec![1.0; n];
    for (i, k) in index.iter().enumerate() {
        let (Some(k), Some(b)) = (k, bodies.get(i)) else { continue };
        let scale = if drag == Some(i) { DRAG_WEIGHT } else { 1.0 };
        let l2 = half_size(b).powi(2);
        for j in 0..6 {
            if let Some(w) = weights.get_mut(k + j) {
                *w = scale * if j < 3 { 1.0 } else { l2 };
            }
        }
    }
    let f = |x: &[f64], out: &mut Vec<f64>| residuals(rels, &poses_at(x, bodies, &index), out);
    let free: Vec<usize> = (0..n).collect();
    let mut x0 = vec![0.0; n];
    let mut outcome = lm::solve(&f, &x0, &free, &weights, tol::ASSEMBLY, MAX_ITER);
    if !outcome.converged && n > 0 {
        // A start exactly half a turn from the answer is a saddle; try again from a slight turn.
        for (i, v) in x0.iter_mut().enumerate() {
            if i % 6 >= 3 {
                *v = [0.05, -0.03, 0.07][i % 3];
            }
        }
        let retry = lm::solve(&f, &x0, &free, &weights, tol::ASSEMBLY, MAX_ITER);
        let cost = |x: &[f64]| {
            let mut r = Vec::new();
            f(x, &mut r);
            lm::max_abs(&r)
        };
        if retry.converged || cost(&retry.x) < cost(&outcome.x) {
            outcome = retry;
        }
    }
    let poses = poses_at(&outcome.x, bodies, &index);
    let frames = poses.iter().zip(bodies).map(|(p, b)| p.frame().unwrap_or(b.frame)).collect();
    Solution { frames, converged: outcome.converged, errors: rel_errors(rels, &poses) }
}

/// A rigid motion `x -> r x + t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub r: M3,
    pub t: Vec3,
}

impl Motion {
    pub const IDENTITY: Motion = Motion { r: M3::IDENTITY, t: Vec3::ZERO };
    pub fn apply_frame(&self, f: &Frame) -> Frame {
        let p = Pose { r: self.r.mul(&M3::of_frame(f)), t: self.r.apply(f.origin()) + self.t };
        p.frame().unwrap_or(*f)
    }
    pub fn inverse(&self) -> Motion {
        let rt = self.r.transpose();
        Motion { r: rt, t: -rt.apply(self.t) }
    }
    /// Turning by `r` about `center`, then moving by `t`.
    fn about(r: M3, center: Vec3, t: Vec3) -> Motion {
        Motion { r, t: center - r.apply(center) + t }
    }
}

/// A motion of B's body that makes one relationship hold by itself, as small as it can be: B's
/// geometry is turned onto A's, then slid into place. Used before solving a new relationship,
/// so the solver starts near the answer.
pub fn snap(bodies: &[Body], rel: &Rel) -> Motion {
    let world = Pose { r: M3::IDENTITY, t: Vec3::ZERO };
    let pose = |e: &End| e.body.and_then(|i| bodies.get(i)).map_or(world, |b| Pose::of(&b.frame));
    let (qa, qb) = (pose(&rel.a), pose(&rel.b));
    let center = rel.b.body.and_then(|i| bodies.get(i)).map_or(Vec3::ZERO, |b| b.center);
    let (pa, pb) = (rel.a.prim.placed(&qa.r, qa.t), rel.b.prim.placed(&qb.r, qb.t));
    // Turn first.
    let target_dir = |da: Vec3, db: Vec3| -> Option<Vec3> {
        match rel.law {
            Law::Mate { .. } => match (mate_prim(pa), mate_prim(pb)) {
                (Prim::Plane { .. }, Prim::Plane { .. }) => Some(-da),
                (Prim::Line { .. }, Prim::Line { .. }) => Some(if da.dot(db) >= 0.0 { da } else { -da }),
                _ => None,
            },
            Law::Flush { .. } => Some(da),
            Law::Insert { aligned, .. } => Some(if aligned { da } else { -da }),
            Law::Joint { joint: JointKind::Ball, .. } | Law::Angle { .. } => None,
            Law::Joint { flip, .. } => Some(if flip { da } else { -da }),
        }
    };
    let (ja, jb) = (joint_frame(&rel.a.prim, &qa), joint_frame(&rel.b.prim, &qb));
    let (da, db) = match rel.law {
        Law::Joint { .. } => (Some(ja.2), Some(jb.2)),
        _ => (pa.direction(), pb.direction()),
    };
    let mut r = match (da, db) {
        (Some(da), Some(db)) => target_dir(da, db).map_or(M3::IDENTITY, |to| rotation_between(db, to)),
        _ => M3::IDENTITY,
    };
    if let Law::Joint { joint: JointKind::Rigid | JointKind::Slider, angle, .. } = rel.law {
        // Then about the joint axis, to the joint's angle.
        let (xa, za) = (ja.1, ja.2);
        let xb = r.apply(jb.1);
        let now = turn(xa, xb, za);
        r = M3::exp(za * (angle - now)).mul(&r);
    }
    let turned = Motion::about(r, center, Vec3::ZERO);
    let moved = |p: Vec3| turned.r.apply(p) + turned.t;
    // Then slide.
    let pb2 = rel.b.prim.placed(&turned.r.mul(&qb.r), moved(qb.t));
    let slide = match rel.law {
        Law::Mate { offset } => match (mate_prim(pa), mate_prim(pb2)) {
            (Prim::Plane { point: a, normal: na }, Prim::Plane { point: b, .. }) => na * (offset - (b - a).dot(na)),
            (Prim::Line { point: a, dir }, Prim::Line { point: b, .. }) => perp(a - b, dir),
            (Prim::Point(a), Prim::Point(b)) => a - b,
            (Prim::Plane { point, normal }, Prim::Point(q)) => normal * (offset - (q - point).dot(normal)),
            (Prim::Point(q), Prim::Plane { point, normal }) => normal * ((q - point).dot(normal) - offset),
            (Prim::Line { point, dir }, Prim::Point(q)) => -perp(q - point, dir),
            (Prim::Point(q), Prim::Line { point, dir }) => perp(q - point, dir),
            (Prim::Plane { point: pp, normal }, Prim::Line { point: pl, .. }) => normal * (offset - (pl - pp).dot(normal)),
            (Prim::Line { point: pl, .. }, Prim::Plane { point: pp, normal }) => normal * ((pl - pp).dot(normal) - offset),
            _ => Vec3::ZERO,
        },
        Law::Flush { offset } => match (pa, pb2) {
            (Prim::Plane { point: a, normal: na }, Prim::Plane { point: b, .. }) => na * (offset - (b - a).dot(na)),
            _ => Vec3::ZERO,
        },
        Law::Insert { offset, .. } => match (pa, pb2) {
            (Prim::Circle { center: ca, normal: na, .. }, Prim::Circle { center: cb, .. }) => ca + na * offset - cb,
            _ => Vec3::ZERO,
        },
        Law::Angle { .. } => Vec3::ZERO,
        Law::Joint { joint, offset, .. } => {
            let (oa, _, za) = ja;
            let ob = moved(jb.0);
            match joint {
                JointKind::Rigid | JointKind::Revolute | JointKind::Ball => oa + za * offset - ob,
                JointKind::Slider | JointKind::Cylindrical => perp(oa - ob, za),
                JointKind::Planar => za * (offset - (ob - oa).dot(za)),
            }
        }
    };
    Motion { r: turned.r, t: turned.t + slide }
}

/// The motions a component has left.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BodyDof {
    /// Directions it may slide in (assembly coordinates, orthonormal).
    pub translations: Vec<Vec3>,
    /// Axes it may turn about: a direction and a point on the axis.
    pub rotations: Vec<(Vec3, Vec3)>,
}

impl BodyDof {
    pub fn count(&self) -> usize {
        self.translations.len() + self.rotations.len()
    }
}

/// Degrees of freedom of every body, each counted with the other bodies held still (as a
/// component's own symbol shows them), and of the whole assembly.
pub fn dof(bodies: &[Body], rels: &[Rel]) -> (Vec<BodyDof>, usize) {
    let poses: Vec<Pose> = bodies.iter().map(|b| Pose::of(&b.frame)).collect();
    let mut per = Vec::with_capacity(bodies.len());
    for (i, b) in bodies.iter().enumerate() {
        if b.fixed {
            per.push(BodyDof::default());
            continue;
        }
        let mine: Vec<Rel> = rels.iter().filter(|r| r.a.body == Some(i) || r.b.body == Some(i)).copied().collect();
        let l = half_size(b);
        let f = |x: &[f64], out: &mut Vec<f64>| {
            let mut p = poses.clone();
            let base = Pose::of(&b.frame);
            let (t, w) = (Vec3::new(x[0], x[1], x[2]), Vec3::new(x[3] / l, x[4] / l, x[5] / l));
            let rw = M3::exp(w);
            if let Some(slot) = p.get_mut(i) {
                *slot = Pose { r: rw.mul(&base.r), t: b.center + rw.apply(base.t - b.center) + t };
            }
            residuals(&mine, &p, out);
        };
        let mut r = Vec::new();
        f(&[0.0; 6], &mut r);
        let rows = r.len();
        let jac = lm::jacobian(&f, &[0.0; 6], &[0, 1, 2, 3, 4, 5], rows);
        per.push(body_dof(&jac, rows, b.center, l));
    }
    // The whole assembly: unknowns of all free bodies at once.
    let index = layout(bodies, rels);
    let n = index.iter().flatten().count() * 6;
    let f = |x: &[f64], out: &mut Vec<f64>| {
        // Turning unknowns scaled by each body's size, as above.
        let mut xs = x.to_vec();
        for (i, k) in index.iter().enumerate() {
            if let (Some(k), Some(b)) = (k, bodies.get(i)) {
                for j in 3..6 {
                    if let Some(v) = xs.get_mut(k + j) {
                        *v /= half_size(b);
                    }
                }
            }
        }
        residuals(rels, &poses_at(&xs, bodies, &index), out);
    };
    let mut r = Vec::new();
    f(&vec![0.0; n], &mut r);
    let rows = r.len();
    let jac = lm::jacobian(&f, &vec![0.0; n], &(0..n).collect::<Vec<_>>(), rows);
    let rank = rank_rel(&jac, rows, n);
    let unused = bodies.iter().zip(&index).filter(|(b, k)| !b.fixed && k.is_none()).count() * 6;
    (per, n - rank.min(n) + unused)
}

/// Numerical rank with the relative tolerance [`tol::DOF_REL`].
fn rank_rel(jac: &[f64], rows: usize, cols: usize) -> usize {
    if rows == 0 || cols == 0 {
        return 0;
    }
    let mut a = vec![0.0; cols * cols];
    for i in 0..cols {
        for j in 0..cols {
            a[i * cols + j] = (0..rows).map(|r| jac.get(r * cols + i).copied().unwrap_or(0.0) * jac.get(r * cols + j).copied().unwrap_or(0.0)).sum();
        }
    }
    let (vals, _) = sym_eigen(&a, cols);
    let top = vals.iter().copied().fold(0.0f64, f64::max).sqrt();
    if top <= 0.0 {
        return 0;
    }
    vals.iter().filter(|v| v.max(0.0).sqrt() > tol::DOF_REL * top).count()
}

/// Free motions of one body from its Jacobian (`rows x 6`; turning columns scaled by `l`).
fn body_dof(jac: &[f64], rows: usize, center: Vec3, l: f64) -> BodyDof {
    let col = |r: usize, c: usize| jac.get(r * 6 + c).copied().unwrap_or(0.0);
    let mut a = [0.0; 36];
    for i in 0..6 {
        for j in 0..6 {
            a[i * 6 + j] = (0..rows).map(|r| col(r, i) * col(r, j)).sum();
        }
    }
    let (vals, vecs) = sym_eigen(&a, 6);
    let top = vals.iter().copied().fold(0.0f64, f64::max).sqrt();
    let free = |v: f64| top <= 0.0 || v.max(0.0).sqrt() <= tol::DOF_REL * top;
    let null: Vec<&Vec<f64>> = vals.iter().zip(&vecs).filter(|(v, _)| free(**v)).map(|(_, e)| e).collect();
    let g = |e: &Vec<f64>, i: usize| e.get(i).copied().unwrap_or(0.0);
    // Turning: the span of the rotation parts of the free motions.
    let turns: Vec<Vec3> = null.iter().map(|e| Vec3::new(g(e, 3), g(e, 4), g(e, 5))).collect();
    let axes = span_basis(&turns, 1e-6);
    let rotations = axes
        .iter()
        .map(|axis| {
            // The free motion turning most about this axis, and the line it turns about.
            let best = null.iter().max_by(|p, q| {
                let w = |e: &Vec<f64>| Vec3::new(g(e, 3), g(e, 4), g(e, 5)).dot(*axis).abs();
                w(p).total_cmp(&w(q))
            });
            let point = best.map_or(center, |e| {
                let (t, w) = (Vec3::new(g(e, 0), g(e, 1), g(e, 2)), Vec3::new(g(e, 3), g(e, 4), g(e, 5)) * (1.0 / l));
                let w2 = w.dot(w);
                if w2 > 0.0 { center + w.cross(t) * (1.0 / w2) } else { center }
            });
            (*axis, point)
        })
        .collect::<Vec<_>>();
    // Sliding: pure translations the constraints allow.
    let mut t = [0.0; 9];
    for i in 0..3 {
        for j in 0..3 {
            t[i * 3 + j] = a[i * 6 + j];
        }
    }
    let (tv, te) = sym_eigen(&t, 3);
    let translations: Vec<Vec3> = tv.iter().zip(&te).filter(|(v, _)| free(**v)).map(|(_, e)| Vec3::new(g(e, 0), g(e, 1), g(e, 2))).collect();
    // Free motions that neither slide purely nor turn are screw-like; count them as sliding.
    let mut translations = span_basis(&translations, 1e-6);
    while translations.len() + rotations.len() < null.len() && translations.len() < 3 {
        let extra = null
            .iter()
            .map(|e| Vec3::new(g(e, 0), g(e, 1), g(e, 2)))
            .find(|v| translations.iter().all(|u| v.normalized().dot(*u).abs() < 0.99) && v.len() > 1e-9);
        match extra {
            Some(v) => translations.push(v.normalized()),
            None => break,
        }
    }
    BodyDof { translations, rotations }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn body(origin: Vec3, fixed: bool) -> Body {
        Body { frame: Frame::WORLD.with_origin(origin).unwrap(), fixed, center: origin + Vec3::new(5.0, 5.0, 5.0), size: 17.3 }
    }
    /// The faces of a 10 mm cube in its part coordinates.
    fn top() -> Prim {
        Prim::Plane { point: Vec3::new(5.0, 5.0, 10.0), normal: Vec3::Z }
    }
    fn bottom() -> Prim {
        Prim::Plane { point: Vec3::new(5.0, 5.0, 0.0), normal: -Vec3::Z }
    }
    fn front() -> Prim {
        Prim::Plane { point: Vec3::new(5.0, 0.0, 5.0), normal: -Vec3::Y }
    }
    fn left() -> Prim {
        Prim::Plane { point: Vec3::new(0.0, 5.0, 5.0), normal: -Vec3::X }
    }
    fn end(body: usize, prim: Prim) -> End {
        End { body: Some(body), prim }
    }

    #[test]
    fn mates_and_flushes_place_a_block_on_a_block_and_count_what_is_left() {
        let bodies = vec![body(Vec3::ZERO, true), body(Vec3::new(30.0, 4.0, -7.0), false)];
        let mut rels = vec![Rel { law: Law::Mate { offset: 0.0 }, a: end(0, top()), b: end(1, bottom()) }];
        let s = solve(&bodies, &rels, None);
        assert!(s.converged, "{s:?}");
        let moved = s.frames[1];
        assert!((moved.origin().z - 10.0).abs() < 1e-7, "{moved:?}");
        assert!(moved.z().near(Vec3::Z, 1e-7));
        // It moved as little as it could: straight up, not sideways.
        assert!((moved.origin().x - 30.0).abs() < 1e-6 && (moved.origin().y - 4.0).abs() < 1e-6);

        let placed: Vec<Body> =
            bodies.iter().zip(&s.frames).map(|(b, f)| Body { frame: *f, center: f.to_world(Vec3::new(5.0, 5.0, 5.0)), ..*b }).collect();
        let (per, total) = dof(&placed, &rels);
        assert_eq!((per[1].translations.len(), per[1].rotations.len(), total), (2, 1, 3));
        assert!(per[1].rotations[0].0.cross(Vec3::Z).len() < 1e-6);

        rels.push(Rel { law: Law::Flush { offset: 0.0 }, a: end(0, front()), b: end(1, front()) });
        rels.push(Rel { law: Law::Flush { offset: 2.5 }, a: end(0, left()), b: end(1, left()) });
        let s = solve(&placed, &rels, None);
        assert!(s.converged);
        assert!(s.frames[1].origin().near(Vec3::new(-2.5, 0.0, 10.0), 1e-6), "{:?}", s.frames[1]);
        let placed: Vec<Body> = placed.iter().zip(&s.frames).map(|(b, f)| Body { frame: *f, ..*b }).collect();
        let (per, total) = dof(&placed, &rels);
        assert_eq!((per[1].count(), total), (0, 0));
    }

    #[test]
    fn a_conflict_does_not_converge() {
        let bodies = vec![body(Vec3::ZERO, true), body(Vec3::new(0.0, 0.0, 12.0), false)];
        let rels = vec![
            Rel { law: Law::Mate { offset: 0.0 }, a: end(0, top()), b: end(1, bottom()) },
            Rel { law: Law::Mate { offset: 5.0 }, a: end(0, top()), b: end(1, bottom()) },
        ];
        let s = solve(&bodies, &rels, None);
        assert!(!s.converged);
        assert!(s.errors.iter().any(|e| *e > 1.0));
    }

    #[test]
    fn joints_leave_their_motions() {
        let axis_a = Prim::Circle { center: Vec3::new(5.0, 5.0, 10.0), normal: Vec3::Z, radius: 2.0 };
        let axis_b = Prim::Circle { center: Vec3::new(5.0, 5.0, 0.0), normal: -Vec3::Z, radius: 2.0 };
        for (joint, slides, turns) in [
            (JointKind::Rigid, 0, 0),
            (JointKind::Revolute, 0, 1),
            (JointKind::Slider, 1, 0),
            (JointKind::Cylindrical, 1, 1),
            (JointKind::Planar, 2, 1),
            (JointKind::Ball, 0, 3),
        ] {
            let bodies = vec![body(Vec3::ZERO, true), body(Vec3::new(13.0, -6.0, 31.0), false)];
            let rels = vec![Rel { law: Law::Joint { joint, flip: false, offset: 0.0, angle: 0.0 }, a: end(0, axis_a), b: end(1, axis_b) }];
            let s = solve(&bodies, &rels, None);
            assert!(s.converged, "{joint:?}");
            let placed: Vec<Body> =
                bodies.iter().zip(&s.frames).map(|(b, f)| Body { frame: *f, center: f.to_world(Vec3::new(5.0, 5.0, 5.0)), ..*b }).collect();
            let (per, total) = dof(&placed, &rels);
            assert_eq!((per[1].translations.len(), per[1].rotations.len()), (slides, turns), "{joint:?}: {:?}", per[1]);
            assert_eq!(total, joint.freedom(), "{joint:?}");
            if joint == JointKind::Revolute {
                // It turns about the joint axis, through the joint origin.
                let (dir, point) = per[1].rotations[0];
                assert!(dir.cross(Vec3::Z).len() < 1e-6);
                assert!((point - Vec3::new(5.0, 5.0, 10.0)).cross(Vec3::Z).len() < 1e-5, "{point:?}");
            }
            if joint == JointKind::Rigid {
                // Turned so the origins face each other: B sits on A, unturned.
                assert!(s.frames[1].origin().near(Vec3::new(0.0, 0.0, 10.0), 1e-6), "{:?}", s.frames[1]);
                assert!(s.frames[1].x().near(Vec3::X, 1e-6));
            }
        }
    }

    #[test]
    fn snapping_turns_a_part_over_before_solving() {
        // B upside down, far away: one snap puts its bottom on A's top, then the solve has
        // nothing left to do.
        let flipped = Frame::new(Vec3::new(40.0, 0.0, 50.0), -Vec3::Z, Vec3::X).unwrap();
        let mut bodies =
            vec![body(Vec3::ZERO, true), Body { frame: flipped, fixed: false, center: flipped.to_world(Vec3::new(5.0, 5.0, 5.0)), size: 17.3 }];
        let rel = Rel { law: Law::Mate { offset: 1.0 }, a: end(0, top()), b: end(1, bottom()) };
        let m = snap(&bodies, &rel);
        bodies[1].frame = m.apply_frame(&bodies[1].frame);
        let mut out = Vec::new();
        residual(&rel, &bodies.iter().map(|b| Pose::of(&b.frame)).collect::<Vec<_>>(), &mut out);
        assert!(lm::max_abs(&out) < 1e-9, "{out:?}");
        let m2 = m.inverse();
        assert!(m2.apply_frame(&bodies[1].frame).origin().near(flipped.origin(), 1e-9));
    }

    #[test]
    fn angle_and_insert() {
        // A hinge: a pin's circle inserted into a hole, then an angle between two faces.
        let hole = Prim::Circle { center: Vec3::new(5.0, 5.0, 10.0), normal: Vec3::Z, radius: 2.0 };
        let pin = Prim::Circle { center: Vec3::new(5.0, 5.0, 0.0), normal: -Vec3::Z, radius: 2.0 };
        let bodies = vec![body(Vec3::ZERO, true), body(Vec3::new(3.0, 1.0, 20.0), false)];
        let mut rels = vec![Rel { law: Law::Insert { offset: 0.0, aligned: false }, a: end(0, hole), b: end(1, pin) }];
        let s = solve(&bodies, &rels, None);
        assert!(s.converged);
        let placed: Vec<Body> =
            bodies.iter().zip(&s.frames).map(|(b, f)| Body { frame: *f, center: f.to_world(Vec3::new(5.0, 5.0, 5.0)), ..*b }).collect();
        assert_eq!(dof(&placed, &rels).1, 1);
        rels.push(Rel { law: Law::Angle { angle: PI / 6.0, reference: Vec3::Z }, a: end(0, front()), b: end(1, front()) });
        let s = solve(&placed, &rels, None);
        assert!(s.converged, "{s:?}");
        let (axis, angle) = M3::of_frame(&s.frames[1]).axis_angle();
        assert!((angle - PI / 6.0).abs() < 1e-7 && axis.near(Vec3::Z, 1e-6), "{axis:?} {angle}");
        let placed: Vec<Body> = placed.iter().zip(&s.frames).map(|(b, f)| Body { frame: *f, ..*b }).collect();
        assert_eq!(dof(&placed, &rels).1, 0);
    }

    #[test]
    fn dragging_keeps_the_dragged_body_near_the_pointer() {
        // A slider along Z: dragging it sideways and up keeps only the upward part.
        let a = Prim::Line { point: Vec3::new(5.0, 5.0, 0.0), dir: Vec3::Z };
        let bodies = vec![body(Vec3::ZERO, true), body(Vec3::new(0.0, 0.0, 10.0), false)];
        let rels = vec![Rel { law: Law::Joint { joint: JointKind::Slider, flip: true, offset: 0.0, angle: 0.0 }, a: end(0, a), b: end(1, a) }];
        let s = solve(&bodies, &rels, None);
        assert!(s.converged);
        let mut placed: Vec<Body> =
            bodies.iter().zip(&s.frames).map(|(b, f)| Body { frame: *f, center: f.to_world(Vec3::new(5.0, 5.0, 5.0)), ..*b }).collect();
        let z0 = placed[1].frame.origin().z;
        let pushed = placed[1].frame.origin() + Vec3::new(7.0, 3.0, 12.0);
        placed[1].frame = placed[1].frame.with_origin(pushed).unwrap();
        placed[1].center = placed[1].frame.to_world(Vec3::new(5.0, 5.0, 5.0));
        let s = solve(&placed, &rels, Some(1));
        assert!(s.converged);
        let o = s.frames[1].origin();
        assert!((o.z - (z0 + 12.0)).abs() < 1e-6 && o.x.abs() < 1e-6 && o.y.abs() < 1e-6, "{o:?}");
    }
}

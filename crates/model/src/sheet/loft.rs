//! Lofted Flange: a sheet between two open profiles.
//!
//! Each profile is taken as points, and the strip between them is the set of triangles of least
//! area that joins one to the other from end to end (for two parallel curves that is close to
//! the developable surface between them, the one a sheet can really take). From there:
//!
//! - **Press-brake facets.** The profiles' arcs are taken as chords, to the chord tolerance, and
//!   the triangles (joined where they lie in one plane) are the flat facets, with a real bend at
//!   every line between two. The solid is made of those, as a flange's is, so the flat pattern
//!   is the ordinary unfolding of flat faces and bends, and exact as they are.
//! - **Die formed.** The same matching, made smooth, is a smooth ruled surface: the kernel makes
//!   the solid between it and its offset by the thickness ([`Kernel::ruled_sheet`]). Such a
//!   surface has no flat faces or bends to unfold, and need not be developable at all, so it is
//!   laid flat by its triangles ([`Skin`]), and the feature says how close that is.
//!
//! [`Kernel::ruled_sheet`]: tenon_kernel::Kernel::ruled_sheet

use std::f64::consts::{PI, TAU};

use serde::{Deserialize, Serialize};
use tenon_geom::{Frame, Vec2, Vec3};

use super::strip::Run;
use super::{MIN_BEND_RADIUS, Sides};
use crate::FeatureId;
use crate::document::Operation;

/// How a lofted flange is formed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Forming {
    /// One smooth surface, pressed in a die: laid flat by triangles.
    DieFormed,
    /// Flat facets with bends between them, no further from the profiles' arcs than `chord`.
    PressBrake { chord: f64 },
}

impl Default for Forming {
    fn default() -> Self {
        Forming::DieFormed
    }
}

/// The smallest chord tolerance (mm), and the most facets a lofted flange is made of.
pub const MIN_CHORD: f64 = 0.001;
pub const MAX_FACETS: usize = 120;

/// A sheet between two open profiles, each in a sketch of its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoftedFlange {
    /// The two profiles: lines and arcs end to end.
    pub sketches: [FeatureId; 2],
    #[serde(default)]
    pub forming: Forming,
    /// Which side of the profiles the sheet's thickness goes: left of the first profile (the
    /// inside of its first corner), right of it, or half each way.
    #[serde(default)]
    pub position: Sides,
    /// The inside radius of the bends; without it, the part's `BendRadius`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    /// Join (to the sheet it touches) or a new solid.
    #[serde(default)]
    pub operation: Operation,
}

impl LoftedFlange {
    pub fn check(&self) -> Result<(), String> {
        if self.sketches[0] == self.sketches[1] {
            return Err("a lofted flange is between two profiles, in two sketches: pick another for the second".into());
        }
        if let Forming::PressBrake { chord } = self.forming
            && !(chord.is_finite() && (MIN_CHORD..=1000.0).contains(&chord))
        {
            return Err(format!("the chord tolerance must be from {MIN_CHORD} to 1000 mm"));
        }
        if let Some(r) = self.radius
            && !(r.is_finite() && (MIN_BEND_RADIUS..=1000.0).contains(&r))
        {
            return Err(format!("the bend radius must be from {MIN_BEND_RADIUS} to 1000 mm"));
        }
        match self.operation {
            Operation::Join | Operation::NewBody => Ok(()),
            _ => Err("a lofted flange is joined to the part or starts a new solid".into()),
        }
    }

    /// How far the sheet reaches along the normal the profile's left gives it, for a sheet `t`
    /// thick.
    pub fn reach(&self, t: f64) -> (f64, f64) {
        match self.position {
            Sides::Default => (0.0, t),
            Sides::Flipped => (-t, 0.0),
            Sides::Symmetric => (-t / 2.0, t / 2.0),
        }
    }
}

/// The runs of a profile as points in space: a line's ends (and as many between as `line_step`
/// says), an arc by `arc_step` radians at most.
pub(crate) fn points_of(runs: &[Run], frame: &Frame, arc_step: impl Fn(f64) -> f64, line_pieces: impl Fn(f64) -> usize) -> Vec<Vec3> {
    let mut out: Vec<Vec3> = Vec::new();
    for run in runs {
        let pieces: Vec<Vec2> = match *run {
            Run::Line { from, to } => {
                let n = line_pieces(from.dist(to)).max(1);
                (0..=n).map(|i| from + (to - from) * (i as f64 / n as f64)).collect()
            }
            Run::Arc { center, radius, from, sweep } => {
                let n = ((sweep.abs() / arc_step(radius)).ceil() as usize).max(1);
                (0..=n).map(|i| Vec2::polar(center, radius, from + sweep * i as f64 / n as f64)).collect()
            }
        };
        let skip = usize::from(!out.is_empty());
        out.extend(pieces.into_iter().skip(skip).map(|p| frame.plane_point(p)));
    }
    out
}

/// The angle of a chord of an arc of `radius` that stays within `tolerance` of it.
pub(crate) fn chord_step(radius: f64, tolerance: f64) -> f64 {
    let c = (1.0 - tolerance / radius).clamp(-1.0, 1.0);
    (2.0 * c.acos()).clamp(1e-3, PI / 2.0)
}

fn triangle_area(a: Vec3, b: Vec3, c: Vec3) -> f64 {
    (b - a).cross(c - a).len() / 2.0
}

/// The direction of a run of points at each of them, as the neighbours on either side have it.
fn tangents(p: &[Vec3]) -> Vec<Vec3> {
    (0..p.len())
        .map(|i| {
            let (x, y) = (p[i.saturating_sub(1)], p[(i + 1).min(p.len() - 1)]);
            let d = y - x;
            if d.len() > 1e-12 { d.normalized() } else { Vec3::ZERO }
        })
        .collect()
}

/// The strip of triangles of least area between two runs of points, from their first points to
/// their last: the pairs of points it visits in order, each step going on along one of the two.
pub(crate) fn matched(a: &[Vec3], b: &[Vec3]) -> Vec<(usize, usize)> {
    matched_by(a, b, 0.0)
}

/// [`matched`], the area of a triangle counting for more the more the sheet twists along the
/// line it starts from, by `twist` x (the sine of the angle between the sheet's two normals
/// there, at the two ends of the line) squared. A sheet with no twist in it is one that can be
/// laid flat: along each line between the two curves it has one tangent plane.
fn matched_by(a: &[Vec3], b: &[Vec3], twist: f64) -> Vec<(usize, usize)> {
    let (n, m) = (a.len(), b.len());
    if n == 0 || m == 0 {
        return Vec::new();
    }
    let (ta, tb) = (tangents(a), tangents(b));
    let weight = |i: usize, j: usize| -> f64 {
        if twist == 0.0 {
            return 1.0;
        }
        let g = b[j] - a[i];
        let (p, q) = (ta[i].cross(g), tb[j].cross(g));
        let (lp, lq) = (p.len(), q.len());
        if lp < 1e-12 || lq < 1e-12 {
            return 1.0;
        }
        // (A sheet that doubles back is as bad as one at a right angle.)
        let sin = p.cross(q).len() / (lp * lq);
        let sin = if p.dot(q) < 0.0 { 1.0 } else { sin };
        1.0 + twist * sin * sin
    };
    let at = |i: usize, j: usize| i * m + j;
    let mut cost = vec![f64::INFINITY; n * m];
    // From where each cell was come to: true along `a`.
    let mut from_a = vec![false; n * m];
    cost[0] = 0.0;
    for i in 0..n {
        for j in 0..m {
            if i > 0 {
                let c = cost[at(i - 1, j)] + triangle_area(a[i - 1], a[i], b[j]) * weight(i, j);
                if c < cost[at(i, j)] {
                    cost[at(i, j)] = c;
                    from_a[at(i, j)] = true;
                }
            }
            if j > 0 {
                let c = cost[at(i, j - 1)] + triangle_area(a[i], b[j - 1], b[j]) * weight(i, j);
                if c < cost[at(i, j)] {
                    cost[at(i, j)] = c;
                    from_a[at(i, j)] = false;
                }
            }
        }
    }
    let (mut i, mut j) = (n - 1, m - 1);
    let mut out = vec![(i, j)];
    while i > 0 || j > 0 {
        if from_a[at(i, j)] {
            i -= 1;
        } else {
            j -= 1;
        }
        out.push((i, j));
    }
    out.reverse();
    out
}

/// One flat facet of a press-brake sheet: its corners along the first profile, then back along
/// the second, in the plane of the sheet's reference surface.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Facet {
    pub corners: Vec<Vec3>,
    /// The normal the sheet's thickness is measured along.
    pub normal: Vec3,
    /// The line it shares with the next facet: on the first profile, on the second.
    pub to_next: Option<[Vec3; 2]>,
}

/// How far from flat two triangles may be and still be one facet (radians).
const SAME_PLANE: f64 = 1e-7;

/// The facets of the strip between `a` and `b`, `sign` turning their normals to the side the
/// sheet's thickness is measured from. An error says why there are none.
pub(crate) fn facets(a: &[Vec3], b: &[Vec3], sign: f64) -> Result<Vec<Facet>, String> {
    let path = matched(a, b);
    // The triangles, with the normal each has and the line it ends on.
    struct Tri {
        normal: Vec3,
        from: (usize, usize),
        to: (usize, usize),
    }
    let mut tris: Vec<Tri> = Vec::with_capacity(path.len());
    for w in path.windows(2) {
        let ((i0, j0), (i1, j1)) = (w[0], w[1]);
        let across = b[j0] - a[i0];
        let along = if i1 > i0 { a[i1] - a[i0] } else { b[j1] - b[j0] };
        let normal = along.cross(across);
        // (Two points in one place: a triangle with no area, which is nothing to fold.)
        if normal.len() < 1e-12 {
            continue;
        }
        tris.push(Tri { normal: normal.normalized() * sign, from: (i0, j0), to: (i1, j1) });
    }
    if tris.is_empty() {
        return Err("The two profiles lie on one line: there is no sheet between them. Put them apart.".into());
    }
    // Triangles that lie in one plane are one facet.
    let mut out: Vec<Facet> = Vec::new();
    let mut start = 0;
    for k in 0..tris.len() {
        let last = k + 1 == tris.len();
        let turn = if last { f64::INFINITY } else { tris[k].normal.cross(tris[k + 1].normal).len() };
        // (And the two must face one way: a sheet that doubles back has a fold of half a turn.)
        if turn > SAME_PLANE || (!last && tris[k].normal.dot(tris[k + 1].normal) < 0.0) {
            let (i0, j0) = tris[start].from;
            let (i1, j1) = tris[k].to;
            let mut corners: Vec<Vec3> = a[i0..=i1].to_vec();
            corners.extend(b[j0..=j1].iter().rev().copied());
            out.push(Facet { corners, normal: tris[k].normal, to_next: (!last).then_some([a[i1], b[j1]]) });
            start = k + 1;
        }
    }
    Ok(out)
}

/// One bend of a press-brake sheet: everything worked out from the two facets it joins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fold {
    /// The line the two facets' reference surfaces meet along, from the first profile to the
    /// second, and its direction.
    pub from: Vec3,
    pub along: Vec3,
    /// A point of the bend's axis level with `from`.
    pub axis: Vec3,
    /// How far the sheet turns.
    pub angle: f64,
    /// In each of the two facets: the way into it, square to the line.
    pub into: [Vec3; 2],
    /// How far into each facet (on the reference surface) the bend reaches from the line.
    pub back: [f64; 2],
    /// From the axis towards each of the two faces of the bend's start and end.
    pub out: [Vec3; 2],
}

/// The bend between facets `f` and `g` of a sheet reaching from `lo` to `hi` along their normals,
/// with inside radius `r`. None when the two make no fold.
pub(crate) fn fold(f: &Facet, g: &Facet, line: [Vec3; 2], lo: f64, hi: f64, r: f64) -> Option<Fold> {
    let along = (line[1] - line[0]).normalized();
    if !(along.is_finite()) || (line[1] - line[0]).len() < 1e-9 {
        return None;
    }
    let (nf, ng) = (f.normal, g.normal);
    let cos = nf.dot(ng).clamp(-1.0, 1.0);
    let angle = nf.cross(ng).len().atan2(cos);
    if angle < SAME_PLANE {
        return None;
    }
    // Into each facet, square to the line: towards the middle of its corners.
    let into_of = |x: &Facet, n: Vec3| {
        let middle = x.corners.iter().fold(Vec3::ZERO, |s, p| s + *p) * (1.0 / x.corners.len() as f64);
        let w = n.cross(along).normalized();
        if (middle - line[0]).dot(w) >= 0.0 { w } else { -w }
    };
    let into = [into_of(f, nf), into_of(g, ng)];
    // The sheet folds towards the side the two facets lean to.
    let lean = (into[0] + into[1]).dot(nf);
    let (inside, sign) = if lean > 0.0 { (hi, 1.0) } else { (lo, -1.0) };
    // Where the two facets' inside faces meet, from the line; the axis is out along the bisector.
    let meet = (nf + ng) * (inside / (1.0 + cos));
    let bisect = (into[0] + into[1]).normalized();
    let offset = meet + bisect * (r / (angle / 2.0).cos());
    let back = [offset.dot(into[0]), offset.dot(into[1])];
    Some(Fold { from: line[0], along, axis: line[0] + offset, angle, into, back, out: [nf * -sign, ng * -sign] })
}

/// A polygon cut by the plane through `at` square to `keep`: what lies on the side `keep` points
/// to.
pub(crate) fn clipped(corners: &[Vec3], at: Vec3, keep: Vec3) -> Vec<Vec3> {
    let side = |p: Vec3| (p - at).dot(keep);
    let n = corners.len();
    let mut out = Vec::with_capacity(n + 1);
    for i in 0..n {
        let (p, q) = (corners[i], corners[(i + 1) % n]);
        let (sp, sq) = (side(p), side(q));
        if sp >= 0.0 {
            out.push(p);
        }
        if (sp > 0.0 && sq < 0.0) || (sp < 0.0 && sq > 0.0) {
            out.push(p + (q - p) * (sp / (sp - sq)));
        }
    }
    out
}

/// How many columns a die-formed sheet is taken in.
const MOST_COLUMNS: usize = 240;
const LEAST_COLUMNS: usize = 33;

/// The distance along a run of points to each of them.
fn lengths(p: &[Vec3]) -> Vec<f64> {
    let mut out = Vec::with_capacity(p.len());
    let mut s = 0.0;
    for (i, q) in p.iter().enumerate() {
        if i > 0 {
            s += q.dist(p[i - 1]);
        }
        out.push(s);
    }
    out
}

/// The point `s` along a run of points whose lengths are `at`.
fn point_at(p: &[Vec3], at: &[f64], s: f64) -> Vec3 {
    let i = at.partition_point(|v| *v < s).clamp(1, p.len().saturating_sub(1).max(1));
    let (Some(&a), Some(&b)) = (p.get(i - 1), p.get(i)) else { return p.first().copied().unwrap_or(Vec3::ZERO) };
    let (s0, s1) = (at.get(i - 1).copied().unwrap_or(0.0), at.get(i).copied().unwrap_or(0.0));
    a + (b - a) * if s1 > s0 { (s - s0) / (s1 - s0) } else { 0.0 }
}

/// A run of numbers from 0 to 1 averaged over `half` either side, its ends held: it goes up
/// smoothly from 0 to 1 where it went up in steps.
fn smoothed(v: &[f64], half: usize) -> Vec<f64> {
    let n = v.len();
    let value = |i: isize| -> f64 {
        // Beyond an end the run is carried on by turning it over about that end.
        let last = n as isize - 1;
        if i < 0 {
            2.0 * v[0] - v[(-i).min(last) as usize]
        } else if i > last {
            2.0 * v[n - 1] - v[(2 * last - i).max(0) as usize]
        } else {
            v[i as usize]
        }
    };
    (0..n)
        .map(|i| {
            let sum: f64 = (-(half as isize)..=half as isize).map(|d| value(i as isize + d)).sum();
            sum / (2 * half + 1) as f64
        })
        .collect()
}

/// The columns of a smooth sheet between two runs of points: for each, a point on each run. The
/// matching of least area, made smooth, and never standing still on either run.
pub(crate) fn columns(a: &[Vec3], b: &[Vec3]) -> Vec<[Vec3; 2]> {
    let path = matched_by(a, b, 25.0);
    let (la, lb) = (lengths(a), lengths(b));
    let (ta, tb) = (la.last().copied().unwrap_or(0.0).max(1e-12), lb.last().copied().unwrap_or(0.0).max(1e-12));
    let ua: Vec<f64> = path.iter().map(|p| la[p.0] / ta).collect();
    let ub: Vec<f64> = path.iter().map(|p| lb[p.1] / tb).collect();
    let half = (path.len() / 60).max(1);
    let (sa, sb) = (smoothed(&ua, half), smoothed(&ub, half));
    let last = path.len().saturating_sub(1).max(1) as f64;
    // (A little of an even step on both, so that neither stands still.)
    const EVEN: f64 = 0.01;
    let even = |s: f64, k: usize| (1.0 - EVEN) * s + EVEN * k as f64 / last;
    let wa: Vec<f64> = sa.iter().enumerate().map(|(k, s)| even(*s, k)).collect();
    let wb: Vec<f64> = sb.iter().enumerate().map(|(k, s)| even(*s, k)).collect();
    let count = path.len().clamp(LEAST_COLUMNS, MOST_COLUMNS);
    (0..count)
        .map(|c| {
            let at = c as f64 / (count - 1) as f64 * last;
            let k = (at.floor() as usize).min(path.len().saturating_sub(2));
            let f = at - k as f64;
            let (u, w) = (wa[k] + (wa[k + 1] - wa[k]) * f, wb[k] + (wb[k + 1] - wb[k]) * f);
            [point_at(a, &la, u * ta), point_at(b, &lb, w * tb)]
        })
        .collect()
}

/// The normal of the surface of `columns` at each end of each column, `sign` turning it to the
/// side the thickness is measured along.
pub(crate) fn normals(columns: &[[Vec3; 2]], sign: f64) -> Vec<[Vec3; 2]> {
    let n = columns.len();
    (0..n)
        .map(|c| {
            let (p, q) = (columns[c.saturating_sub(1)], columns[(c + 1).min(n.saturating_sub(1))]);
            let across = columns[c][1] - columns[c][0];
            [0, 1].map(|end| {
                let w = (q[end] - p[end]).cross(across);
                // (A column that stands still on this rim: as the other end has it.)
                let w = if w.len() > 1e-12 { w } else { (q[1 - end] - p[1 - end]).cross(across) };
                w.normalized() * sign
            })
        })
        .collect()
}

/// A die-formed sheet as it is laid flat: its two sides and its neutral surface in the same
/// columns, and where each point of them goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Skin {
    /// The faces of the solid that are each side.
    pub faces: [Vec<u32>; 2],
    /// Each side's columns.
    pub sides: [Vec<[Vec3; 2]>; 2],
    /// Where the columns go in the flat, seen from the side the normals of the surface look to.
    pub flat: Vec<[Vec2; 2]>,
    /// Each piece between two columns is two triangles: true when the line between them goes
    /// from the start of the first column to the end of the next.
    pub rising: Vec<bool>,
    /// Which sides see the flat turned over, for the pattern to be seen from that side.
    pub mirrored: [bool; 2],
    /// How close the pattern is, in words.
    pub words: String,
    /// How much the sheet stretches across its triangles, at most (a share).
    pub stretch: f64,
    /// How far the far end moves if it is laid out by the other diagonals (mm).
    pub drift: f64,
}

/// A triangle in the flat from its neighbour's edge `a` to `b`: the point on the right of that
/// edge at distances `to_a` and `to_b` from its ends.
fn beside(a: Vec2, b: Vec2, to_a: f64, to_b: f64) -> Vec2 {
    let d = b - a;
    let l = d.len();
    if l < 1e-12 {
        return a + Vec2::X * to_a;
    }
    let x = (to_a * to_a - to_b * to_b + l * l) / (2.0 * l);
    let y = (to_a * to_a - x * x).max(0.0).sqrt();
    let u = d * (1.0 / l);
    // To the right of a to b.
    a + u * x - u.perp() * y
}

/// The columns laid out in the flat, each piece by the two triangles `rising` says.
fn laid_out(neutral: &[[Vec3; 2]], rising: impl Fn(usize) -> bool) -> Vec<[Vec2; 2]> {
    let mut flat: Vec<[Vec2; 2]> = Vec::with_capacity(neutral.len());
    let Some(first) = neutral.first() else { return flat };
    flat.push([Vec2::ZERO, Vec2::new(0.0, first[0].dist(first[1]))]);
    for c in 1..neutral.len() {
        let (p, q) = (neutral[c - 1], neutral[c]);
        let [a, b] = flat[c - 1];
        flat.push(if rising(c - 1) {
            // Start to the next end, then the next start.
            let d = beside(a, b, p[0].dist(q[1]), p[1].dist(q[1]));
            [beside(a, d, p[0].dist(q[0]), q[0].dist(q[1])), d]
        } else {
            // The next start, then from it the next end.
            let e = beside(a, b, p[0].dist(q[0]), p[1].dist(q[0]));
            [e, beside(e, b, q[0].dist(q[1]), p[1].dist(q[1]))]
        });
    }
    flat
}

impl Skin {
    /// A sheet whose columns are `columns`, reaching from `lo` to `hi` along `normals` (which
    /// are `sign` times the surface's own), by the part's K factor.
    pub(crate) fn new(columns: &[[Vec3; 2]], normals: &[[Vec3; 2]], lo: f64, hi: f64, k_factor: f64, sign: f64) -> Skin {
        let t = hi - lo;
        let n = columns.len();
        let offset = |d: f64| -> Vec<[Vec3; 2]> { columns.iter().zip(normals).map(|(c, m)| [c[0] + m[0] * d, c[1] + m[1] * d]).collect() };
        // The neutral surface is a share of the way through from the inside of the curve, column
        // by column: the sheet stretches outside it and shrinks inside it.
        // (Where a rim runs straight it has no inside, and the neutral surface stays as it was.)
        let mut last = [lo + k_factor * t; 2];
        let neutral: Vec<[Vec3; 2]> = (0..n)
            .map(|c| {
                let (p, q) = (columns[c.saturating_sub(1)], columns[(c + 1).min(n.saturating_sub(1))]);
                let here = columns[c];
                [0, 1].map(|end| {
                    let curve = (q[end] + p[end] - here[end] * 2.0).dot(normals[c][end]);
                    if curve.abs() > 1e-6 * (q[end].dist(p[end]) + 1e-9) {
                        last[end] = if curve > 0.0 { hi - k_factor * t } else { lo + k_factor * t };
                    }
                    here[end] + normals[c][end] * last[end]
                })
            })
            .collect();
        // By the shorter diagonal of each piece.
        let rising: Vec<bool> =
            (0..n.saturating_sub(1)).map(|c| neutral[c][0].dist(neutral[c + 1][1]) < neutral[c][1].dist(neutral[c + 1][0])).collect();
        let flat = laid_out(&neutral, |c| rising[c]);
        let other = laid_out(&neutral, |c| !rising[c]);
        // How long the other diagonal of each piece is, against how long it is in the flat.
        let mut stretch = 0.0f64;
        for c in 0..n.saturating_sub(1) {
            let (true_length, flat_length) = if rising[c] {
                (neutral[c][1].dist(neutral[c + 1][0]), flat[c][1].dist(flat[c + 1][0]))
            } else {
                (neutral[c][0].dist(neutral[c + 1][1]), flat[c][0].dist(flat[c + 1][1]))
            };
            if true_length > 1e-9 {
                stretch = stretch.max((flat_length - true_length).abs() / true_length);
            }
        }
        let drift = flat.iter().zip(&other).map(|(a, b)| a[0].dist(b[0]).max(a[1].dist(b[1]))).fold(0.0, f64::max);
        let triangles = 2 * n.saturating_sub(1);
        let words = format!(
            "Die formed: laid flat by {triangles} triangles of its neutral surface, not by unfolding. Its rims and the lines across it keep their lengths; between them the sheet is up to {:.2} % stretched or shrunk, and laying the triangles out the other way moves the pattern by up to {} mm. That is how close this pattern is: try a part before cutting many.",
            stretch * 100.0,
            crate::explain::mm((drift * 1000.0).round() / 1000.0)
        );
        // The flat is seen from the side the surface's own normal looks to.
        let mirrored = [sign > 0.0, sign < 0.0];
        Skin { faces: [Vec::new(), Vec::new()], sides: [offset(lo), offset(hi)], flat, rising, mirrored, words, stretch, drift }
    }

    /// The whole of a rim of side `side` in the flat, when `start` and `end` are its two ends.
    pub fn rim(&self, start: Vec3, end: Vec3, side: usize) -> Option<Vec<Vec2>> {
        let grid = self.sides.get(side)?;
        let (first, last) = (grid.first()?, grid.last()?);
        let turned = |v: Vec2| if self.mirrored.get(side).copied().unwrap_or(false) { Vec2::new(-v.x, v.y) } else { v };
        for rim in [0, 1] {
            let (a, b) = (first[rim], last[rim]);
            let points = || self.flat.iter().map(|c| turned(c[rim]));
            if a.dist(start) < 1e-6 && b.dist(end) < 1e-6 {
                return Some(points().collect());
            }
            if b.dist(start) < 1e-6 && a.dist(end) < 1e-6 {
                return Some(points().rev().collect());
            }
        }
        None
    }

    /// Where point `p` of side `side` goes in the flat: by the triangle of that side it is
    /// nearest.
    pub fn map(&self, p: Vec3, side: usize) -> Vec2 {
        let Some(grid) = self.sides.get(side) else { return Vec2::ZERO };
        let mut best: Option<(f64, Vec2)> = None;
        for c in 0..grid.len().saturating_sub(1) {
            let (g0, g1) = (grid[c], grid[c + 1]);
            let (f0, f1) = (self.flat[c], self.flat[c + 1]);
            let tris: [([Vec3; 3], [Vec2; 3]); 2] = if self.rising.get(c).copied().unwrap_or(true) {
                [([g0[0], g0[1], g1[1]], [f0[0], f0[1], f1[1]]), ([g0[0], g1[1], g1[0]], [f0[0], f1[1], f1[0]])]
            } else {
                [([g0[0], g0[1], g1[0]], [f0[0], f0[1], f1[0]]), ([g1[0], g0[1], g1[1]], [f1[0], f0[1], f1[1]])]
            };
            for (t3, t2) in tris {
                let [u, v, w] = nearest_in(t3, p);
                let d = (t3[0] * u + t3[1] * v + t3[2] * w).dist(p);
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, t2[0] * u + t2[1] * v + t2[2] * w));
                }
            }
        }
        let at = best.map_or(Vec2::ZERO, |b| b.1);
        if self.mirrored.get(side).copied().unwrap_or(false) { Vec2::new(-at.x, at.y) } else { at }
    }
}

/// How much of each corner of a triangle makes the point of it nearest `p`.
fn nearest_in(t: [Vec3; 3], p: Vec3) -> [f64; 3] {
    let (ab, ac, ap) = (t[1] - t[0], t[2] - t[0], p - t[0]);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return [1.0, 0.0, 0.0];
    }
    let bp = p - t[1];
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return [0.0, 1.0, 0.0];
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = if d1 - d3 != 0.0 { d1 / (d1 - d3) } else { 0.0 };
        return [1.0 - v, v, 0.0];
    }
    let cp = p - t[2];
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return [0.0, 0.0, 1.0];
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = if d2 - d6 != 0.0 { d2 / (d2 - d6) } else { 0.0 };
        return [1.0 - w, 0.0, w];
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        let denom = (d4 - d3) + (d5 - d6);
        let w = if denom != 0.0 { (d4 - d3) / denom } else { 0.0 };
        return [0.0, 1.0 - w, w];
    }
    let denom = va + vb + vc;
    if denom.abs() < 1e-300 {
        return [1.0, 0.0, 0.0];
    }
    let (v, w) = (vb / denom, vc / denom);
    [1.0 - v - w, v, w]
}

/// An even step, for what is sampled round an arc (radians).
pub(crate) fn smooth_step(_: f64) -> f64 {
    TAU / 72.0
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn the_strip_of_least_area_between_a_line_and_a_point_is_a_fan_and_between_parallel_lines_a_plate() {
        // Three points on a line, one point across from their middle: two triangles.
        let a = [Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 0.0, 0.0)];
        let b = [Vec3::new(10.0, 10.0, 0.0)];
        assert_eq!(matched(&a, &b), vec![(0, 0), (1, 0), (2, 0)]);
        // Two parallel lines in one plane are one facet whatever the points along them.
        let b = [Vec3::new(0.0, 10.0, 0.0), Vec3::new(5.0, 10.0, 0.0), Vec3::new(20.0, 10.0, 0.0)];
        let f = facets(&a, &b, 1.0).unwrap();
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].normal.near(-Vec3::Z, 1e-12) || f[0].normal.near(Vec3::Z, 1e-12));
        // A fold: a roof of two slopes.
        let a = [Vec3::ZERO, Vec3::new(10.0, 0.0, 3.0), Vec3::new(20.0, 0.0, 0.0)];
        let b = [Vec3::new(0.0, 30.0, 0.0), Vec3::new(10.0, 30.0, 3.0), Vec3::new(20.0, 30.0, 0.0)];
        let f = facets(&a, &b, 1.0).unwrap();
        assert_eq!(f.len(), 2, "{f:?}");
        assert_eq!(f[0].to_next, Some([a[1], b[1]]));
        assert_eq!(f[1].to_next, None);
    }

    #[test]
    fn a_fold_is_worked_out_as_a_bend_tangent_to_its_two_facets() {
        // Two flats that meet at a right angle along the Y axis, seen from its end: one along
        // +X, one along +Z. Their reference surfaces are the plane z = 0 and the plane x = 0;
        // the sheet is 2 thick on the side they lean to.
        let line = [Vec3::ZERO, Vec3::new(0.0, 10.0, 0.0)];
        let f = Facet { corners: vec![line[0], line[1], Vec3::new(20.0, 10.0, 0.0), Vec3::new(20.0, 0.0, 0.0)], normal: Vec3::Z, to_next: None };
        let g = Facet { corners: vec![line[0], line[1], Vec3::new(0.0, 10.0, 20.0), Vec3::new(0.0, 0.0, 20.0)], normal: Vec3::X, to_next: None };
        let b = fold(&f, &g, line, 0.0, 2.0, 3.0).unwrap();
        assert!((b.angle - FRAC_PI_2_).abs() < 1e-12);
        // The inside of the bend is the side at 2; those planes meet at (2, y, 2); the axis is
        // a further 3 along each.
        assert!(b.axis.near(Vec3::new(5.0, 0.0, 5.0), 1e-12), "{:?}", b.axis);
        assert!((b.back[0] - 5.0).abs() < 1e-12 && (b.back[1] - 5.0).abs() < 1e-12, "{:?}", b.back);
        assert!(b.out[0].near(-Vec3::Z, 1e-12) && b.out[1].near(-Vec3::X, 1e-12));
        // The sheet on the other side of the surfaces: the inside is the surfaces themselves.
        let b = fold(&f, &g, line, -2.0, 0.0, 3.0).unwrap();
        assert!(b.axis.near(Vec3::new(3.0, 0.0, 3.0), 1e-12), "{:?}", b.axis);
        assert!((b.back[0] - 3.0).abs() < 1e-12);
    }

    const FRAC_PI_2_: f64 = std::f64::consts::FRAC_PI_2;

    #[test]
    fn a_polygon_is_cut_by_a_plane() {
        let square = [Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(0.0, 10.0, 0.0)];
        let cut = clipped(&square, Vec3::new(3.0, 0.0, 0.0), Vec3::X);
        assert_eq!(cut.len(), 4);
        let xs: Vec<f64> = cut.iter().map(|p| p.x).collect();
        assert!(xs.iter().all(|x| *x >= 3.0 - 1e-12) && xs.iter().any(|x| (*x - 3.0).abs() < 1e-12));
        // Across a corner it takes the corner off; beyond it, nothing is left.
        assert_eq!(clipped(&square, Vec3::new(5.0, 0.0, 0.0), (Vec3::X + Vec3::Y).normalized()).len(), 5);
        assert!(clipped(&square, Vec3::new(30.0, 0.0, 0.0), Vec3::X).is_empty());
    }

    #[test]
    fn a_developable_skin_lays_flat_with_nothing_stretched() {
        // A quarter of a cone frustum: radii 40 and 20, 30 high, so a slant of sqrt(20^2 + 30^2).
        let on = |r: f64, z: f64, a: f64| Vec3::new(r * a.cos(), r * a.sin(), z);
        let turn = FRAC_PI_2_;
        let n = 60;
        let cols: Vec<[Vec3; 2]> = (0..=n)
            .map(|i| {
                let a = turn * f64::from(i) / f64::from(n);
                [on(40.0, 0.0, a), on(20.0, 30.0, a)]
            })
            .collect();
        let sign = 1.0;
        let m = normals(&cols, sign);
        let skin = Skin::new(&cols, &m, 0.0, 0.1, 0.5, sign);
        // Nearly nothing: a cone is laid flat by triangles to within the facets' own chords.
        assert!(skin.stretch < 1e-3, "{}", skin.stretch);
        // Its rims keep their lengths, as chords of the two circles at the neutral surface.
        let rim = |end: usize| skin.flat.windows(2).map(|w| w[0][end].dist(w[1][end])).sum::<f64>();
        let want = |r: f64| f64::from(n) * 2.0 * r * (turn / f64::from(n) / 2.0).sin();
        // (The neutral surface here is 0.05 along the normal: 0.05 cos of the half angle in radius.)
        let slant = (400.0f64 + 900.0).sqrt();
        let out = 0.05 * 30.0 / slant;
        assert!((rim(0) - want(40.0 + out)).abs() < 0.02 || (rim(0) - want(40.0 - out)).abs() < 0.02, "{} {}", rim(0), want(40.0));
        assert!((rim(1) - want(20.0 + out)).abs() < 0.02 || (rim(1) - want(20.0 - out)).abs() < 0.02, "{} {}", rim(1), want(20.0));
        // A point of the surface goes where its column does.
        let grid = &skin.sides[0];
        let mirrored = |v: Vec2| if skin.mirrored[0] { Vec2::new(-v.x, v.y) } else { v };
        assert!(skin.map(grid[10][0], 0).near(mirrored(skin.flat[10][0]), 1e-9));
        assert!(skin.map(grid[30][1], 0).near(mirrored(skin.flat[30][1]), 1e-9));
        assert!(skin.words.contains("120 triangles") && skin.words.contains("how close"), "{}", skin.words);
    }

    #[test]
    fn a_smooth_matching_never_stands_still_and_ends_where_the_runs_end() {
        // A long line against a short one, in a fan-like way.
        let a: Vec<Vec3> = (0..=20).map(|i| Vec3::new(f64::from(i), 0.0, 0.0)).collect();
        let b: Vec<Vec3> = (0..=4).map(|i| Vec3::new(8.0 + f64::from(i), 0.0, 10.0)).collect();
        let c = columns(&a, &b);
        assert!(c.len() >= LEAST_COLUMNS);
        assert!(c[0][0].near(a[0], 1e-9) && c[0][1].near(b[0], 1e-9));
        assert!(c.last().unwrap()[0].near(a[20], 1e-9) && c.last().unwrap()[1].near(b[4], 1e-9));
        for w in c.windows(2) {
            assert!(w[1][0].x > w[0][0].x && w[1][1].x > w[0][1].x, "{w:?}");
        }
    }

    #[test]
    fn a_chord_stays_within_its_tolerance() {
        // R 50, 0.1: half a chord's angle has cosine 1 - 0.1 / 50.
        let step = chord_step(50.0, 0.1);
        assert!(((step / 2.0).cos() - (1.0 - 0.1 / 50.0)).abs() < 1e-12);
        // A tolerance as large as the arc gives the coarsest there is.
        assert_eq!(chord_step(1.0, 5.0), PI / 2.0);
    }
}

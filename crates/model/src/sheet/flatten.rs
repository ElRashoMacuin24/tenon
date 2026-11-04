//! What Unfold and Refold read off the solid.
//!
//! A bend laid flat is a strip of sheet as wide as the bend's neutral surface is long: its round
//! faces unrolled, with whatever was cut out of them. A [`FlatBend`] says where such a strip now
//! lies and how to roll it up again, which is all a refold needs: what was done to the strip
//! meanwhile is read off the solid when the time comes ([`strip_outlines`]).

use std::f64::consts::TAU;

use tenon_geom::{Axis, Vec2, Vec3};
use tenon_kernel::{Kernel, Loop, ShapeHandle, SurfaceKind};

use super::flat::{FlatCurve, FlatLoop, chain};
use super::outline::Piece;
use super::region;
use super::unfold::{Chart, Read, Unfold, bend_side, flatten, radial, read};
use super::wrap::has_other_side;
use crate::FeatureId;
use crate::naming::{FaceOrigin, sub_key};

/// A bend an unfold has laid flat, where it now lies.
#[derive(Clone, Debug, PartialEq)]
pub struct FlatBend {
    /// The unfold that laid it flat.
    pub unfold: FeatureId,
    /// What the names of what it makes are made from.
    pub key: u64,
    /// Where the strip starts, on the side of the sheet that is the inside of the bend.
    pub origin: Vec3,
    /// Across the strip, away from the sheet on the side towards the stationary face.
    pub across: Vec3,
    /// Along the strip: seen with `across` to the right and `along` up, `up` looks at the eye.
    pub along: Vec3,
    /// Through the sheet, from the side that is the inside of the bend.
    pub up: Vec3,
    /// How far it reaches along the strip from `origin`.
    pub run: (f64, f64),
    /// Its width: `sweep` x `rho`.
    pub allowance: f64,
    /// How far the sheet turned at it (radians).
    pub sweep: f64,
    /// The radius of the inside of the bend, and of its neutral surface.
    pub inside: f64,
    pub rho: f64,
    /// What the bend's round faces were called: its inside, its outside.
    pub names: [Option<FaceOrigin>; 2],
    /// There was sheet beyond it when it was laid flat.
    pub beyond: bool,
    /// What the faces of the strip are called, each with `unfold`.
    pub sources: Vec<u64>,
}

impl FlatBend {
    /// Whether the face with this name is one of the strip's.
    pub fn holds(&self, name: &FaceOrigin) -> bool {
        matches!(name, FaceOrigin::From { feature, source, .. } if *feature == self.unfold && self.sources.contains(source))
    }

    /// Where the strip's axis lies: the axis the bend turned about.
    pub fn center(&self) -> Vec3 {
        self.origin - self.up * self.inside
    }

    /// Moved with the sheet it is on.
    pub(crate) fn moved(&mut self, by: &Rigid) {
        self.origin = by.point(self.origin);
        self.across = by.direction(self.across);
        self.along = by.direction(self.along);
        self.up = by.direction(self.up);
    }
}

/// `v` turned `angle` radians about the unit direction `k` (right-hand rule).
pub(crate) fn turned(v: Vec3, k: Vec3, angle: f64) -> Vec3 {
    let (s, c) = angle.sin_cos();
    v * c + k.cross(v) * s + k * (k.dot(v) * (1.0 - c))
}

/// A turn about an axis, then a step: how the sheet beyond a bend moves when the bend is laid
/// flat or rolled up.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rigid {
    pub axis: Axis,
    pub angle: f64,
    pub shift: Vec3,
}

impl Rigid {
    pub(crate) fn point(&self, p: Vec3) -> Vec3 {
        self.axis.origin() + turned(p - self.axis.origin(), self.axis.dir(), self.angle) + self.shift
    }
    pub(crate) fn direction(&self, v: Vec3) -> Vec3 {
        turned(v, self.axis.dir(), self.angle)
    }
}

/// One connected piece of a strip in the flat: its outer boundary and its holes. Each curve has
/// a tag of its own; a curve that came as points has its tag in `sampled`.
#[derive(Clone, Debug)]
pub(crate) struct Outline {
    pub outer: Vec<Piece>,
    pub holes: Vec<Vec<Piece>>,
    sampled: Vec<u64>,
}

impl Outline {
    /// A closed run of pieces of this outline as the kernel takes it.
    pub(crate) fn as_loop(&self, pieces: &[Piece]) -> Loop {
        region::to_loop_smoothing(pieces, |tag| self.sampled.contains(&tag))
    }

    /// A closed run of pieces of this outline as the kernel rolls it: it takes a round hole as
    /// its two halves, not as one circle.
    pub(crate) fn as_rolled_loop(&self, pieces: &[Piece]) -> Loop {
        if let [p, q] = pieces
            && p.arc.is_some()
            && q.arc.is_some()
        {
            return super::outline::to_loop(pieces);
        }
        self.as_loop(pieces)
    }

    /// Whether it reaches from one side of a strip of this width to the other.
    pub(crate) fn spans(&self, width: f64) -> bool {
        let at = || self.outer.iter().flat_map(|p| [p.a.x, p.b.x]);
        let (lo, hi) = (at().fold(f64::INFINITY, f64::min), at().fold(f64::NEG_INFINITY, f64::max));
        lo <= 1e-6 && hi >= width - 1e-6
    }

    /// The tags of its curves.
    pub(crate) fn tags(&self) -> Vec<u64> {
        let mut tags: Vec<u64> = self.outer.iter().chain(self.holes.iter().flatten()).map(|p| p.tag).collect();
        tags.sort_unstable();
        tags.dedup();
        tags
    }

    /// The same outline with its points moved by `to`, which does not turn it over.
    pub(crate) fn moved(&self, to: impl Fn(Vec2) -> Vec2) -> Outline {
        Outline {
            outer: region::moved(&self.outer, &to, false),
            holes: self.holes.iter().map(|h| region::moved(h, &to, false)).collect(),
            sampled: self.sampled.clone(),
        }
    }
}

/// The curves of the face `f` as `chart` lays them out, as an outline tagged from `tag`.
fn outline_of_face(k: &dyn Kernel, shape: ShapeHandle, solid: &Read, f: u32, chart: &Chart, tag: u64, opts: &Unfold) -> Result<Outline, String> {
    let mut curves = Vec::new();
    for &e in solid.topo.face_edges.get(f as usize).map_or(&[][..], Vec::as_slice) {
        let Some(info) = solid.edges.get(e as usize).filter(|i| !i.degenerate) else { continue };
        curves.push(flatten(k, shape.edge(e), info, chart, opts.deflection)?);
    }
    let (outer, inner) = chain(curves).map_err(|_| "What is cut out of this bend leaves an outline that cannot be followed.".to_owned())?;
    let mut next = 0;
    let mut sampled = Vec::new();
    let mut pieces = |l: &FlatLoop| -> Vec<Piece> {
        let mut out = Vec::new();
        for c in &l.curves {
            let own = sub_key(tag, next);
            next += 1;
            if matches!(c, FlatCurve::Polyline { .. }) {
                sampled.push(own);
            }
            out.extend(region::of_flat(&FlatLoop { curves: vec![c.clone()] }, own));
        }
        out
    };
    let outer = pieces(&outer);
    let holes = inner.iter().map(&mut pieces).collect();
    Ok(Outline { outer, holes, sampled })
}

/// A bend of the sheet as the solid has it.
#[derive(Clone, Debug)]
pub(crate) struct BendRead {
    /// A point of its axis, and the way along the axis.
    pub center: Vec3,
    pub dir: Vec3,
    /// From the axis to where it starts: it turns from there about `dir`, right-handed.
    pub start: Vec3,
    pub sweep: f64,
    pub inside: f64,
    pub rho: f64,
    /// The stretches of the axis it takes up, from `center` along `dir`, in order.
    pub runs: Vec<(f64, f64)>,
    /// What it is laid flat: x round the bend from `start` at the neutral surface, y along `dir`.
    pub outlines: Vec<Outline>,
    /// The round faces it is made of.
    pub faces: Vec<u32>,
    /// What its faces are called: its inside, its outside.
    pub names: [Option<FaceOrigin>; 2],
}

impl BendRead {
    /// Its width in the flat.
    pub(crate) fn allowance(&self) -> f64 {
        self.sweep * self.rho
    }

    /// From the axis to the bend's other end.
    pub(crate) fn finish(&self) -> Vec3 {
        turned(self.start, self.dir, self.sweep)
    }
}

/// How far round a chart a round face reaches, and how far along.
fn rim_of(
    k: &dyn Kernel,
    shape: ShapeHandle,
    solid: &Read,
    f: u32,
    chart: &Chart,
    axis: &Axis,
    opts: &Unfold,
) -> Result<((f64, f64), (f64, f64)), String> {
    let (mut turn, mut run) = ((f64::INFINITY, f64::NEG_INFINITY), (f64::INFINITY, f64::NEG_INFINITY));
    for &e in solid.topo.face_edges.get(f as usize).map_or(&[][..], Vec::as_slice) {
        let Some(info) = solid.edges.get(e as usize).filter(|i| !i.degenerate) else { continue };
        let straight = match &info.curve {
            tenon_kernel::CurveKind::Line { .. } => true,
            tenon_kernel::CurveKind::Circle { axis: about, .. } => about.dir().cross(axis.dir()).len() < 1e-6,
            _ => false,
        };
        let points =
            if straight { vec![info.start, info.mid, info.end] } else { k.edge_points(shape.edge(e), opts.deflection).map_err(|e| e.to_string())? };
        for p in points {
            let (angle, along) = chart.round(p);
            turn = (turn.0.min(angle), turn.1.max(angle));
            run = (run.0.min(along), run.1.max(along));
        }
    }
    Ok((turn, run))
}

/// Reads the bend face `face` of `shape` is one side of. A bend in pieces along one line (a slot
/// through it, a hole) is taken whole when `whole_line` says so; otherwise only the faces that
/// reach the same stretch of the axis are, so that two tabs on one edge are two bends. The
/// curves of its outlines are tagged from `tag`.
pub(crate) fn read_bend(
    k: &dyn Kernel,
    shape: ShapeHandle,
    names: &[Option<FaceOrigin>],
    face: u32,
    whole_line: bool,
    tag: u64,
    opts: &Unfold,
) -> Result<BendRead, String> {
    let t = opts.thickness;
    let solid = read(k, shape)?;
    let info = solid.faces.get(face as usize).ok_or("no such face")?;
    let not_one = "Unfold lays bends flat, and a face picked for it is not one: pick a round face of a bend, its inside or its outside. (A bend that is flat already has no round faces.)";
    let SurfaceKind::Cylinder { axis, radius } = &info.surface else { return Err(not_one.into()) };
    let inner = bend_side(face as usize, &solid.faces, t).ok_or(not_one)?;
    let inside = if inner { *radius } else { radius - t };
    let a = axis.dir();
    let c0 = radial(axis, info.centroid)
        .ok_or("This bend goes all the way round (a tube), so there is nothing for it to turn from. Open a seam in it first with Rip.")?;
    let rho = inside + opts.k_factor * t;
    let middle = |radius: f64| Chart::Bend {
        q: axis.origin() + c0 * radius,
        axis: *axis,
        r0: c0,
        mid: 0.0,
        rho,
        o2: Vec2::ZERO,
        ea: Vec2::Y,
        eg: Vec2::X,
        inner: true,
        radius,
    };
    // The round faces on the same axis: their radius, how far round they reach and how far along.
    let mut line: Vec<(u32, bool, (f64, f64), (f64, f64))> = Vec::new();
    for (i, g) in solid.faces.iter().enumerate() {
        let SurfaceKind::Cylinder { axis: b, radius: rb } = &g.surface else { continue };
        let d = b.origin() - axis.origin();
        let tol = 1e-6 * (1.0 + inside + t);
        let in_side = (rb - inside).abs() < tol;
        if a.cross(b.dir()).len() > 1e-6
            || (d - a * d.dot(a)).len() > tol
            || !(in_side || (rb - inside - t).abs() < tol)
            || bend_side(i, &solid.faces, t).is_none()
        {
            continue;
        }
        let Ok(i) = u32::try_from(i) else { continue };
        let (turn, run) = rim_of(k, shape, &solid, i, &middle(*rb), axis, opts)?;
        line.push((i, in_side, turn, run));
    }
    // Those that reach the same stretch as this one, and those that reach the same as they do.
    let mut taken: Vec<(u32, bool, (f64, f64), (f64, f64))> = line.iter().filter(|g| g.0 == face).copied().collect();
    loop {
        let more: Vec<_> = line
            .iter()
            .filter(|g| !taken.iter().any(|h| h.0 == g.0))
            .filter(|g| whole_line || taken.iter().any(|h| g.3.0.max(h.3.0) < g.3.1.min(h.3.1) - 1e-9))
            .copied()
            .collect();
        if more.is_empty() {
            break;
        }
        taken.extend(more);
    }
    taken.sort_by_key(|g| g.0);
    let turn = taken.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |s, g| (s.0.min(g.2.0), s.1.max(g.2.1)));
    let sweep = turn.1 - turn.0;
    if !(sweep > 1e-9 && sweep < TAU - 1e-9) {
        return Err("This bend goes all the way round (a tube), so there is nothing for it to turn from. Open a seam in it first with Rip.".into());
    }
    let start = turned(c0, a, turn.0);
    // Laid out from its inside, where it has one (what is left of its outside is the same).
    let side: Vec<u32> = {
        let inside_faces: Vec<u32> = taken.iter().filter(|g| g.1).map(|g| g.0).collect();
        if inside_faces.is_empty() { taken.iter().map(|g| g.0).collect() } else { inside_faces }
    };
    let on_inside = taken.iter().any(|g| g.1);
    let radius_of_side = if on_inside { inside } else { inside + t };
    let chart = Chart::Bend {
        q: axis.origin() + start * radius_of_side,
        axis: *axis,
        r0: start,
        mid: sweep / 2.0,
        rho,
        o2: Vec2::ZERO,
        ea: Vec2::Y,
        eg: Vec2::X,
        inner: on_inside,
        radius: radius_of_side,
    };
    let mut outlines = Vec::with_capacity(side.len());
    for (n, f) in side.iter().enumerate() {
        outlines.push(outline_of_face(k, shape, &solid, *f, &chart, sub_key(tag, u32::try_from(n).unwrap_or(u32::MAX)), opts)?);
    }
    // The stretches of the axis, as one where they overlap.
    let mut spans: Vec<(f64, f64)> = taken.iter().map(|g| g.3).collect();
    spans.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut runs: Vec<(f64, f64)> = Vec::new();
    for s in spans {
        match runs.last_mut() {
            Some(last) if s.0 <= last.1 + 1e-9 => last.1 = last.1.max(s.1),
            _ => runs.push(s),
        }
    }
    let name_of = |in_side: bool| taken.iter().find(|g| g.1 == in_side).and_then(|g| names.get(g.0 as usize).copied().flatten());
    Ok(BendRead {
        center: axis.origin(),
        dir: a,
        start,
        sweep,
        inside,
        rho,
        runs,
        outlines,
        faces: taken.iter().map(|g| g.0).collect(),
        names: [name_of(true), name_of(false)],
    })
}

/// The round faces of the sheet that are the inside of a bend, in the kernel's order.
pub(crate) fn inner_bends(k: &dyn Kernel, shape: ShapeHandle, opts: &Unfold) -> Result<Vec<u32>, String> {
    let solid = read(k, shape)?;
    Ok((0..solid.faces.len()).filter(|i| bend_side(*i, &solid.faces, opts.thickness) == Some(true)).filter_map(|i| u32::try_from(i).ok()).collect())
}

/// A point of flat face `face` of `shape` that is a side of the sheet: the middle of its longest
/// edge. None when the face is no flat side of the sheet.
pub(crate) fn point_on_side(k: &dyn Kernel, shape: ShapeHandle, face: u32, opts: &Unfold) -> Result<Option<Vec3>, String> {
    let solid = read(k, shape)?;
    if !has_other_side(&solid, face as usize, opts.thickness) {
        return Ok(None);
    }
    let edges = solid.topo.face_edges.get(face as usize).map_or(&[][..], Vec::as_slice);
    let longest = edges.iter().filter_map(|e| solid.edges.get(*e as usize)).filter(|i| !i.degenerate).max_by(|x, y| x.length.total_cmp(&y.length));
    Ok(longest.map(|i| i.mid))
}

/// What is left of a flat strip, given as the solid `strip`: each piece's outline in the strip's
/// coordinates (x across it, y along it), tagged from `tag`. Whatever is not sheet of one
/// thickness, cut straight through, cannot be rolled up, and is said to be so.
pub(crate) fn strip_outlines(k: &dyn Kernel, strip: ShapeHandle, bend: &FlatBend, tag: u64, opts: &Unfold) -> Result<Vec<Outline>, String> {
    let t = opts.thickness;
    let solid = read(k, strip)?;
    let chart = Chart::Plane { o3: bend.origin, u3: bend.across, v3: bend.along, o2: Vec2::ZERO, u2: Vec2::X, v2: Vec2::Y };
    let not_sheet = "A bend cannot be folded back while what is in its flat strip is not sheet of one thickness cut straight through: a cut that does not go right through, a slanted or a chamfered edge, or anything raised on the strip, cannot go round a bend. Make the cut go through, or do that work where the sheet stays flat.";
    let slack = 1e-6 * (1.0 + t);
    let mut outlines = Vec::new();
    let mut area = 0.0;
    for (i, f) in solid.faces.iter().enumerate() {
        match &f.surface {
            SurfaceKind::Plane { normal, .. } => {
                let (n, height) = (normal.normalized(), (f.centroid - bend.origin).dot(bend.up));
                if n.cross(bend.up).len() > 1e-6 {
                    if n.dot(bend.up).abs() > 1e-6 {
                        return Err(not_sheet.into());
                    }
                    continue;
                }
                // A side of the sheet, or the floor of something that does not go through.
                if height.abs() > slack && (height - t).abs() > slack {
                    return Err(not_sheet.into());
                }
                if n.dot(bend.up) > 0.0 || height.abs() > slack {
                    continue;
                }
                let Ok(i) = u32::try_from(i) else { continue };
                area += f.area;
                outlines.push(outline_of_face(k, strip, &solid, i, &chart, sub_key(tag, i), opts)?);
            }
            SurfaceKind::Cylinder { axis, .. } if axis.dir().cross(bend.up).len() <= 1e-6 => {}
            // The wall of a cut whose outline came as points: a curve pushed straight.
            SurfaceKind::Cylinder { .. } | SurfaceKind::Cone { .. } | SurfaceKind::Sphere { .. } | SurfaceKind::Torus { .. } => {
                return Err(not_sheet.into());
            }
            _ => {}
        }
    }
    // (What a face with such a wall says of its own area is only so close.)
    let volume = k.mass_properties(strip, 1.0).map_err(|e| e.to_string())?.volume;
    if (area * t - volume).abs() > 2e-3 * (1.0 + volume) {
        return Err(not_sheet.into());
    }
    Ok(outlines)
}

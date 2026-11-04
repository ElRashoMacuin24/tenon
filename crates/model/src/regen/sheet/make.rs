//! Regenerating the sheet metal features that make sheet of their own or stamp it: Contour Roll,
//! Lofted Flange and Punch.

use tenon_geom::{Axis, Frame, Vec2, Vec3};
use tenon_kernel::{AngleExtent, BoolOp, Curve2, Extent, Loop, Op, Profile, Region, ShapeHandle, TaggedCurve2};
use tenon_sketch::Sketch;

use super::super::{Ctx, Stop};
use super::{Named, contour_path};
use crate::document::{AxisRef, Document};
use crate::naming::sub_key;
use crate::sheet::flange::Part;
use crate::sheet::loft::{self, Forming, LoftedFlange, Skin};
use crate::sheet::outline;
use crate::sheet::punch::{self, Punch, PunchShape};
use crate::sheet::strip::{self, Run};
use crate::sheet::unfold::Unfold;
use crate::sheet::wrap;
use crate::sheet::{ContourRoll, SKIN_SIDES, Sides, Sizes};
use crate::{FeatureId, FeatureKind};

/// What a press-brake facet's and bend's faces are named after: its number and the part.
fn facet_key(i: usize, part: u32) -> u64 {
    sub_key(sub_key(0x4641_4345, u32::try_from(i).unwrap_or(u32::MAX)), part)
}
fn fold_key(i: usize, part: u32) -> u64 {
    sub_key(sub_key(0x464f_4c44, u32::try_from(i).unwrap_or(u32::MAX)), part)
}

/// A direction of a sketch's plane in space.
fn in_space(frame: &Frame, v: Vec2) -> Vec3 {
    frame.x() * v.x + frame.y() * v.y
}

/// The sheet between two profiles as the two rims it starts from: their runs, each in its frame,
/// running the same way, and the sign that turns the surface's normal to the left of the first
/// (the side the sheet reaches to, in `Sides::Default`).
struct Rims {
    runs: [Vec<Run>; 2],
    frames: [Frame; 2],
    /// `Run` by `Run`: the second profile's reach from its own left is its reach times this.
    second_left: f64,
    sign: f64,
}

/// A sketch as a regeneration has it, and its plane.
fn sketch_of<'a>(doc: &'a Document, regen: &'a crate::Regen, id: FeatureId) -> Result<(&'a Sketch, Frame), String> {
    let sketch = match regen.sketches.get(&id) {
        Some(s) => &**s,
        None => doc.sketch(id).ok_or("a profile of the lofted flange is not a sketch")?,
    };
    let frame = *regen.sketch_frames.get(&id).ok_or("a profile of the lofted flange is suppressed or failed")?;
    Ok((sketch, frame))
}

fn rims_of(doc: &Document, regen: &crate::Regen, l: &LoftedFlange) -> Result<Rims, String> {
    rims_between([sketch_of(doc, regen, l.sketches[0])?, sketch_of(doc, regen, l.sketches[1])?])
}

fn rims_between(sketches: [(&Sketch, Frame); 2]) -> Result<Rims, String> {
    let [(s0, f0), (s1, f1)] = sketches;
    let (r0, _) =
        contour_path(s0, &[]).map_err(|e| e.replace("A contour flange", "A lofted flange").replace("a contour flange", "a lofted flange"))?;
    let (mut r1, _) =
        contour_path(s1, &[]).map_err(|e| e.replace("A contour flange", "A lofted flange").replace("a contour flange", "a lofted flange"))?;
    let ends = |runs: &[Run], f: &Frame| -> Result<[Vec3; 2], String> {
        match (runs.first(), runs.last()) {
            (Some(a), Some(b)) => Ok([f.plane_point(a.start()), f.plane_point(b.end())]),
            _ => Err("a profile of the lofted flange is empty".into()),
        }
    };
    let (a, b) = (ends(&r0, &f0)?, ends(&r1, &f1)?);
    // Each end to the one nearest.
    if a[0].dist(b[0]) + a[1].dist(b[1]) > a[0].dist(b[1]) + a[1].dist(b[0]) {
        r1 = r1.iter().rev().map(Run::reversed).collect();
    }
    let b0 = ends(&r1, &f1)?[0];
    let across = b0 - a[0];
    if across.len() < 1e-6 {
        return Err("The two profiles of the lofted flange start at one point: put them apart.".into());
    }
    let heading = |runs: &[Run], f: &Frame| runs.first().map_or(Vec3::X, |r| in_space(f, r.headings().0));
    let (h0, h1) = (heading(&r0, &f0), heading(&r1, &f1));
    let raw = h0.cross(across);
    if raw.len() < 1e-9 {
        return Err("The first profile of the lofted flange runs straight at the second where it starts: there is no sheet between them. Put them side by side.".into());
    }
    let sign = if raw.dot(f0.z().cross(h0)) >= 0.0 { 1.0 } else { -1.0 };
    let second_left = if f1.z().cross(h1).dot(raw * sign) >= 0.0 { 1.0 } else { -1.0 };
    Ok(Rims { runs: [r0, r1], frames: [f0, f1], second_left, sign })
}

/// The columns of a die-formed sheet, and their normals, with the sign and how far it reaches.
fn smooth_sheet(rims: &Rims, l: &LoftedFlange, sizes: &Sizes) -> Result<(Vec<[Vec3; 2]>, Vec<[Vec3; 2]>), String> {
    let (lo, hi) = l.reach(sizes.thickness);
    let r = l.radius.unwrap_or(sizes.bend_radius);
    let rounded =
        |runs: &[Run], lo: f64, hi: f64| -> Result<Vec<Run>, String> { Ok(strip::rounded(runs, r, lo, hi)?.into_iter().map(|p| p.0).collect()) };
    let first = rounded(&rims.runs[0], lo, hi)?;
    let second = if rims.second_left > 0.0 { rounded(&rims.runs[1], lo, hi)? } else { rounded(&rims.runs[1], -hi, -lo)? };
    fn rim<'a>(runs: &'a [Run], frame: &'a Frame) -> loft::Rim<'a> {
        let total: f64 = runs.iter().map(Run::length).sum();
        loft::Rim::of(runs, frame, loft::smooth_step, |len| ((len / (total / 60.0)).ceil() as usize).clamp(1, 60))
    }
    let (a, b) = (rim(&first, &rims.frames[0]), rim(&second, &rims.frames[1]));
    loft::not_too_many(&a.points, &b.points)?;
    let (columns, ways) = loft::columns(&a, &b);
    let normals = loft::normals(&columns, &ways, rims.sign);
    let t = hi - lo;
    for (end, (runs, frame)) in [(&first, &rims.frames[0]), (&second, &rims.frames[1])].into_iter().enumerate() {
        if let Some(radius) = centre_passed(runs, frame, &columns, &normals, end, lo, hi) {
            return Err(format!(
                "A curve of these profiles has a radius of {} mm, and a die-formed sheet {} mm thick on the inside of it goes past its centre and crosses itself. Make the curve wider or the sheet thinner, or put the sheet on the outside of the curve with Position.",
                crate::explain::mm((radius * 1000.0).round() / 1000.0),
                crate::explain::mm(t)
            ));
        }
    }
    let (twist, thick) = loft::thickness_of(&columns, &normals, lo, hi);
    if thick < t * (1.0 - loft::THIN) {
        return Err(format!(
            "These profiles twist a die-formed sheet by up to {:.0} degrees from one rim to the other, and it would come to only {} mm thick on the whole, where the rule says {} mm. Turn the profiles less, one against the other, or make it as Press Brake facets, which keep their thickness.",
            twist.to_degrees(),
            crate::explain::mm((thick * 1000.0).round() / 1000.0),
            crate::explain::mm((t * 1000.0).round() / 1000.0)
        ));
    }
    Ok((columns, normals))
}

/// The radius of an arc of `runs` (the rim `end` of `columns`) whose centre the sheet reaches past,
/// from `lo` to `hi` along `normals`.
fn centre_passed(runs: &[Run], frame: &Frame, columns: &[[Vec3; 2]], normals: &[[Vec3; 2]], end: usize, lo: f64, hi: f64) -> Option<f64> {
    runs.iter().find_map(|run| {
        let Run::Arc { center, radius, .. } = *run else { return None };
        let middle = frame.plane_point(center);
        let past = columns.iter().zip(normals).any(|(column, normal)| {
            let to = middle - column[end];
            // (A point of the rim on this arc, or on a chord of it.)
            (to.len() - radius).abs() <= 2e-3 * radius && {
                let inward = to.normalized().dot(normal[end]);
                (lo * inward).max(hi * inward) >= radius - 1e-6
            }
        });
        past.then_some(radius)
    })
}

/// How a die-formed sheet is laid flat: what the flat pattern goes by, and what the feature says
/// of how close that is.
pub(crate) fn lofted_skin(doc: &Document, regen: &crate::Regen, l: &LoftedFlange, sizes: &Sizes) -> Result<Skin, String> {
    let rims = rims_of(doc, regen, l)?;
    let (columns, normals) = smooth_sheet(&rims, l, sizes)?;
    let (lo, hi) = l.reach(sizes.thickness);
    Ok(Skin::new(&columns, &normals, lo, hi, sizes.k_factor, rims.sign))
}

/// What a lofted flange's panel says of it: how it is laid flat, and how close that is.
pub fn lofted_words(l: &LoftedFlange, sketches: [(&Sketch, Frame); 2], sizes: &Sizes) -> Result<String, String> {
    let rims = rims_between(sketches)?;
    match l.forming {
        Forming::DieFormed => {
            let (columns, normals) = smooth_sheet(&rims, l, sizes)?;
            let (lo, hi) = l.reach(sizes.thickness);
            Ok(Skin::new(&columns, &normals, lo, hi, sizes.k_factor, rims.sign).words)
        }
        Forming::PressBrake { chord } => {
            let points = |runs: &[Run], frame: &Frame| loft::points_of(runs, frame, |radius| loft::chord_step(radius, chord), |_| 1);
            let (a, b) = (points(&rims.runs[0], &rims.frames[0]), points(&rims.runs[1], &rims.frames[1]));
            let n = loft::facets(&a, &b, rims.sign)?.len();
            Ok(format!(
                "Press brake: {n} flat facet{} with {} bend{} between them, each within {} mm of the profiles' arcs. Laid flat by unfolding them, which is exact. Where folds meet at a corner of a profile they stop short of it, as a relief.",
                if n == 1 { "" } else { "s" },
                n.saturating_sub(1),
                if n == 2 { "" } else { "s" },
                crate::explain::mm(chord)
            ))
        }
    }
}

/// The profile of a contour roll: its curves in order, with lines that run straight on from one
/// another taken as one line, so that it is the surface it looks like. (Two lines in one straight
/// run are one cylinder or one cone, which unroll exactly; as two faces the unrolling would have to
/// go across a rim between them.)
fn roll_path(sketch: &Sketch, skip: &[tenon_sketch::EntityId]) -> Result<(Vec<Run>, Vec<tenon_sketch::EntityId>), String> {
    let (runs, ids) =
        contour_path(sketch, skip).map_err(|e| e.replace("A contour flange", "A contour roll").replace("a contour flange", "a contour roll"))?;
    let mut merged: (Vec<Run>, Vec<tenon_sketch::EntityId>) = (Vec::with_capacity(runs.len()), Vec::with_capacity(ids.len()));
    for (run, id) in runs.into_iter().zip(ids) {
        if let (Some(Run::Line { from, to }), Run::Line { from: next, to: end }) = (merged.0.last().copied(), run)
            && to.dist(next) <= 1e-6
            && (to - from).cross(end - next).abs() <= 1e-9 * (to - from).len() * (end - next).len()
            && (to - from).dot(end - next) > 0.0
        {
            if let Some(last) = merged.0.last_mut() {
                *last = Run::Line { from, to: end };
            }
            continue;
        }
        merged.0.push(run);
        merged.1.push(id);
    }
    Ok(merged)
}

/// What a contour roll's panel says of it: whether it unrolls exactly.
pub fn roll_words(c: &ContourRoll, sketch: &Sketch) -> Result<String, String> {
    let skip: Vec<tenon_sketch::EntityId> = if let AxisRef::SketchLine(l) = c.axis { vec![l] } else { Vec::new() };
    let (runs, _) = roll_path(sketch, &skip)?;
    Ok(if matches!(runs.as_slice(), [Run::Line { .. }]) {
        "One straight line: a cylinder, a cone or a flat ring, which unrolls exactly.".to_owned()
    } else {
        "This profile does not unroll exactly: only one straight line does. Each line and arc is unrolled row by row at its neutral surface; the flat pattern says how far out that is.".to_owned()
    })
}

impl Ctx<'_> {
    /// A revolve whose flat faces look out of the solid, as every sheet metal feature takes a
    /// flat face's normal to. (The ordinary Revolve keeps what it gives: a sketch put on such a
    /// face takes its direction from that, and old files must come out as they did.)
    fn revolved(&mut self, profile: &Profile, axis: &Axis, turn: &AngleExtent) -> Result<Op, Stop> {
        let op = self.k.revolve(profile, axis, turn)?;
        let shape = self.k.direct_faces(op.shape);
        self.k.release(op.shape);
        Ok(Op { shape: shape?, history: op.history })
    }

    /// On a part with a die-formed sheet, that what a round or a chamfer of `size` on `edges` of
    /// `before` left can be measured. On a ruled surface the kernel now and then gives, at some
    /// sizes and not at others, a solid that passes every check with a face of the wrong area, even
    /// a negative one, and a box past the part. A round takes off at most a strip of its size along
    /// each face at each edge, and a corner of its size at each end.
    pub(in crate::regen) fn die_formed_measures(
        &mut self,
        before: ShapeHandle,
        after: ShapeHandle,
        edges: &[u32],
        size: f64,
        what: &str,
    ) -> Result<(), Stop> {
        if !self.doc.features().iter().any(|f| matches!(&f.kind, FeatureKind::LoftedFlange(l) if l.forming == Forming::DieFormed)) {
            return Ok(());
        }
        let mut area = [0.0f64; 2];
        let mut negative = false;
        for (i, shape) in [before, after].into_iter().enumerate() {
            for f in 0..self.k.topology(shape)?.faces {
                let a = self.k.face_info(shape.face(f))?.area;
                area[i] += a;
                negative |= a < 0.0;
            }
        }
        let mut most = 0.0;
        for e in edges {
            most += 2.0 * size * self.k.edge_info(before.edge(*e))?.length + 4.0 * size * size;
        }
        if negative || area[0] - area[1] > 1.5 * most {
            self.k.release(after);
            return Err(format!(
                "This {what} gives a face the kernel cannot measure: its area comes out wrong. That happens now and then on the ruled surface of a die-formed sheet, at some sizes and not at others. Try a slightly different size, or shape the corner in the profiles."
            )
            .into());
        }
        Ok(())
    }

    pub(in crate::regen) fn contour_roll(&mut self, id: FeatureId, c: &ContourRoll) -> Result<(), Stop> {
        c.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let t = sizes.thickness;
        let (held, frame) = self.sketch_and_frame(c.sketch)?;
        let sketch: &Sketch = &held;
        let mut skip = Vec::new();
        let axis = match &c.axis {
            AxisRef::Origin(a) => a.axis(),
            AxisRef::SketchLine(line) => {
                skip.push(*line);
                let (a, b) = sketch.line(*line).ok_or("the axis of the contour roll is not a line of the sketch")?;
                let (a3, b3) = (frame.plane_point(a), frame.plane_point(b));
                Axis::new(a3, b3 - a3).ok_or("the axis of the contour roll has no length")?
            }
            AxisRef::Work(w) => self.work_axis(*w)?,
        };
        if axis.dir().dot(frame.z()).abs() > 1e-6 || (axis.origin() - frame.origin()).dot(frame.z()).abs() > 1e-6 {
            return Err("The axis of the contour roll is not in the sketch's plane, so the profile cannot be rolled about it. Choose an axis in that plane: a line of the sketch, or an origin axis that lies in it.".into());
        }
        let (drawn, ids) = roll_path(sketch, &skip)?;
        let (lo, hi) = c.reach(t);
        let path = strip::rounded(&drawn, c.radius.unwrap_or(sizes.bend_radius), lo, hi)?;
        let runs: Vec<Run> = path.iter().map(|p| p.0).collect();
        if strip::crosses(&runs, lo, hi) {
            return Err("The contour roll folds into itself: its profile comes back through its own path. Shorten the part that does, or draw it clear of the rest.".into());
        }
        // It must lie all on one side of the axis, clear of it: what touches the axis has no roll.
        let across = {
            let (a0, a2) = (frame.to_local(axis.origin()), frame.to_local(axis.origin() + axis.dir()));
            let (origin, along) = (Vec2::new(a0.x, a0.y), (Vec2::new(a2.x, a2.y) - Vec2::new(a0.x, a0.y)).normalized());
            move |p: Vec2| (p - origin).cross(along)
        };
        let mut sides = (f64::INFINITY, f64::NEG_INFINITY);
        for d in [lo, hi] {
            for run in runs.iter().filter_map(|r| r.offset(d)) {
                let points: Vec<Vec2> = match run {
                    Run::Line { from, to } => vec![from, to],
                    Run::Arc { center, radius, from, sweep } => {
                        (0..=16).map(|i| Vec2::polar(center, radius, from + sweep * f64::from(i) / 16.0)).collect()
                    }
                };
                for p in points {
                    let a = across(p);
                    sides = (sides.0.min(a), sides.1.max(a));
                }
            }
        }
        if sides.0 < 1e-6 && sides.1 > -1e-6 {
            return Err("The sheet reaches the axis of the contour roll: it would pinch to nothing there. Draw the profile clear of the axis, with the sheet's thickness too, all on one side of it.".into());
        }
        let key = |i: usize| sub_key(u64::from(ids.get(i).map_or(0, |e| e.0)), 1);
        let tag = |n: usize, left: bool| {
            let [a, b] = path.get(n).map_or([0, 0], |p| p.1);
            sub_key(if a == b { key(a) } else { crate::naming::pair_key(key(a), key(b)) }, 2 + u32::from(left))
        };
        let region = strip::outline(&runs, lo, hi, tag, [sub_key(0, Part::Root as u32), sub_key(0, Part::Tip as u32)])?;
        let turn = match c.direction {
            Sides::Default => AngleExtent::Angle(c.angle),
            Sides::Flipped => AngleExtent::Angle(-c.angle),
            Sides::Symmetric => AngleExtent::Symmetric(c.angle),
        };
        let op = self.revolved(&Profile { frame, regions: vec![region] }, &axis, &turn)?;
        let made = self.sheet_named(id, op, Some([sub_key(0, Part::Start as u32), sub_key(0, Part::End as u32)]))?;
        self.combine(id, vec![made], c.operation)
    }

    pub(in crate::regen) fn lofted_flange(&mut self, id: FeatureId, l: &LoftedFlange) -> Result<(), Stop> {
        l.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let t = sizes.thickness;
        let rims = rims_of(self.doc, &self.regen, l)?;
        let (lo, hi) = l.reach(t);
        let made = match l.forming {
            Forming::DieFormed => {
                let (columns, normals) = smooth_sheet(&rims, l, &sizes)?;
                let rim = |end: usize, d: f64| -> Vec<Vec3> { columns.iter().zip(&normals).map(|(c, n)| c[end] + n[end] * d).collect() };
                let (a0, a1, b0, b1) = (rim(0, lo), rim(1, lo), rim(0, hi), rim(1, hi));
                let walls = [Part::Root, Part::Tip, Part::Start, Part::End].map(|p| sub_key(0, p as u32));
                let op = self.k.ruled_sheet([&a0, &a1], [&b0, &b1], walls)?;
                self.sheet_named(id, op, Some(SKIN_SIDES))?
            }
            Forming::PressBrake { chord } => self.press_brake(id, &rims, l, chord, &sizes)?,
        };
        self.combine(id, vec![made], l.operation)
    }

    /// A sheet between two rims as flat facets with bends between them: each facet a prism cut
    /// back to where its bends begin, each bend rolled between them, all joined.
    fn press_brake(&mut self, id: FeatureId, rims: &Rims, l: &LoftedFlange, chord: f64, sizes: &Sizes) -> Result<Named, Stop> {
        let t = sizes.thickness;
        let (lo, hi) = l.reach(t);
        let r = l.radius.unwrap_or(sizes.bend_radius);
        let rho = r + sizes.k_factor * t;
        let points = |runs: &[Run], frame: &Frame| loft::points_of(runs, frame, |radius| loft::chord_step(radius, chord), |_| 1);
        let (a, b) = (points(&rims.runs[0], &rims.frames[0]), points(&rims.runs[1], &rims.frames[1]));
        let facets = loft::facets(&a, &b, rims.sign)?;
        if facets.len() > loft::MAX_FACETS {
            return Err(format!(
                "A chord tolerance of {} mm makes {} facets, and a lofted flange has at most {}: make the tolerance larger.",
                crate::explain::mm(chord),
                facets.len(),
                loft::MAX_FACETS
            )
            .into());
        }
        // Every facet cut back to where its bends begin.
        let mut outlines: Vec<Vec<Vec3>> = facets.iter().map(|f| f.corners.clone()).collect();
        let mut folds = Vec::with_capacity(facets.len());
        for (i, pair) in facets.windows(2).enumerate() {
            let line = pair[0].to_next.ok_or("internal: a facet with none after it has one after it")?;
            let fold = loft::fold(&pair[0], &pair[1], line, lo, hi, r);
            if let Some(b) = &fold {
                for (j, side) in [(i, 0), (i + 1, 1)] {
                    outlines[j] = loft::clipped(&outlines[j], b.from + b.into[side] * b.back[side], b.into[side]);
                }
            }
            folds.push(fold);
        }
        let too_tight = || {
            Stop::Failed(format!(
                "The bends take up all of a facet: {} mm of inside radius is too much for facets this small. Make the bend radius smaller, or the chord tolerance larger.",
                crate::explain::mm(r)
            ))
        };
        if outlines.iter().any(|o| o.len() < 3) {
            return Err(too_tight());
        }
        // Where each bend's ends are, on each of its two facets. An end runs straight across from
        // where one facet stops to where the other does. Where one stops well short of the other
        // (at a corner of a profile, where the bends beside it stop it) the bend ends square where
        // the shorter stops, and the longer is cut back to there too: that is the relief at the
        // corner. (Laid flat, bends are wider than the corners they round: what is left of two
        // facets at a corner would otherwise lie across one another.)
        let mut ends: Vec<Option<[(f64, f64); 2]>> = Vec::with_capacity(folds.len());
        for (i, fold) in folds.iter().enumerate() {
            let Some(b) = fold else {
                ends.push(None);
                continue;
            };
            let reach = |corners: &[Vec3], side: usize| -> Option<(f64, f64)> {
                let at_line = corners.iter().filter(|q| ((**q - b.from).dot(b.into[side]) - b.back[side]).abs() < 1e-7);
                let along: Vec<f64> = at_line.map(|q| (*q - b.from).dot(b.along)).collect();
                let (least, most) = along.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, m), v| (l.min(*v), m.max(*v)));
                (most - least > 1e-6).then_some((least, most))
            };
            let (Some(mut first), Some(mut second)) = (reach(&outlines[i], 0), reach(&outlines[i + 1], 1)) else { return Err(too_tight()) };
            let wide = 2.0 * b.angle * rho;
            if (first.0 - second.0).abs() > wide {
                let at = first.0.max(second.0);
                let longer = if first.0 < second.0 { i } else { i + 1 };
                outlines[longer] = loft::clipped(&outlines[longer], b.from + b.along * at, b.along);
                (first.0, second.0) = (at, at);
            }
            if (first.1 - second.1).abs() > wide {
                let at = first.1.min(second.1);
                let longer = if first.1 > second.1 { i } else { i + 1 };
                outlines[longer] = loft::clipped(&outlines[longer], b.from + b.along * at, -b.along);
                (first.1, second.1) = (at, at);
            }
            if first.1 - first.0 < 1e-6 || second.1 - second.0 < 1e-6 || outlines[i].len() < 3 || outlines[i + 1].len() < 3 {
                return Err(too_tight());
            }
            ends.push(Some([first, second]));
        }
        let mut pieces: Vec<Named> = Vec::with_capacity(2 * facets.len());
        let built = (|| -> Result<(), Stop> {
            for (i, (facet, corners)) in facets.iter().zip(&outlines).enumerate() {
                let (p0, p1) = (corners[0], corners.get(1).copied().unwrap_or(corners[0]));
                let x = corners.iter().map(|q| *q - p0).find(|d| d.len() > 1e-9).unwrap_or(p1 - p0);
                let frame = Frame::new(p0 + facet.normal * lo, facet.normal, x).ok_or_else(too_tight)?;
                let flat: Vec<(Vec2, u64)> = corners
                    .iter()
                    .enumerate()
                    .map(|(e, q)| {
                        let at = frame.to_local(*q);
                        (Vec2::new(at.x, at.y), facet_key(i, u32::try_from(e).unwrap_or(0)))
                    })
                    .collect();
                let around = outline::polygon(&flat);
                if around.len() < 3 {
                    return Err(too_tight());
                }
                let profile = Profile { frame, regions: vec![Region { outer: outline::to_loop(&around), holes: vec![] }] };
                pieces.push(self.sheet_prism(id, &profile, t, Some([facet_key(i, 1000), facet_key(i, 1001)]))?);
            }
            for (i, (fold, reach)) in folds.iter().zip(&ends).enumerate() {
                let (Some(b), Some([first, second])) = (fold, reach) else { continue };
                let turn = b.out[0].cross(b.out[1]).dot(b.along);
                let (z, way) = if turn >= 0.0 { (b.along, 1.0) } else { (-b.along, -1.0) };
                let frame = Frame::new(b.axis, z, b.out[0]).ok_or_else(too_tight)?;
                let w = b.angle * rho;
                let name = |part: u32| fold_key(i, part);
                let corners = [
                    (Vec2::new(0.0, first.0 * way), name(0)),
                    (Vec2::new(w, second.0 * way), name(1)),
                    (Vec2::new(w, second.1 * way), name(2)),
                    (Vec2::new(0.0, first.1 * way), name(3)),
                ];
                let mut around = corners.to_vec();
                // (Counter-clockwise, whichever way along the line it runs.)
                if way < 0.0 {
                    around.reverse();
                }
                let around = outline::polygon(&around);
                let profile = Profile { frame, regions: vec![Region { outer: outline::to_loop(&around), holes: vec![] }] };
                pieces.push(self.sheet_roll(id, &profile, rho, r, r + t, [name(4), name(5)])?);
            }
            Ok(())
        })();
        if let Err(e) = built {
            for (s, _) in pieces {
                self.k.release(s);
            }
            return Err(e);
        }
        // All of them joined: nothing lost, nothing apart.
        let mut pending = pieces.into_iter();
        let Some(first) = pending.next() else { return Err("the lofted flange has no facets".into()) };
        let rest: Vec<Named> = pending.collect();
        let volume = |ctx: &Self, s: tenon_kernel::ShapeHandle| ctx.k.mass_properties(s, 1.0).map(|m| m.volume);
        let added: f64 = std::iter::once(&first).chain(&rest).map(|p| volume(self, p.0).unwrap_or(0.0)).sum();
        if rest.is_empty() {
            return Ok(first);
        }
        let joined = self.sheet_step(BoolOp::Union, first, false, rest)?;
        let (whole, solids) = (volume(self, joined.0), self.k.topology(joined.0).map(|s| s.solids));
        // (To a thousandth: where folds meet at a corner their bends overlap by a hair at their ends.)
        if !matches!(solids, Ok(1)) || !matches!(whole, Ok(v) if (v - added).abs() <= 1e-3 * added) {
            self.k.release(joined.0);
            return Err(
                "The facets and bends of the lofted flange could not be joined into one sheet. Try another chord tolerance or bend radius, or move the profiles a little."
                    .into(),
            );
        }
        Ok(joined)
    }

    pub(in crate::regen) fn punch(&mut self, id: FeatureId, p: &Punch) -> Result<(), Stop> {
        p.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let t = sizes.thickness;
        let (held, frame) = self.sketch_and_frame(p.sketch)?;
        let sketch: &Sketch = &held;
        let points = if p.points.is_empty() { crate::document::hole_centres(sketch) } else { p.points.clone() };
        if points.is_empty() {
            return Err("The punch's sketch has no centre points: put a point where each stamp goes, or click the face in the Punch panel.".into());
        }
        if self.regen.bodies.is_empty() {
            return Err("There is no sheet to punch yet: start with a Face.".into());
        }
        let opts = Unfold { thickness: t, k_factor: sizes.k_factor, base: None, deflection: crate::sheet::FLAT_DEFLECTION };
        let (sin, cos) = p.rotation.sin_cos();
        let (cuts, pieces, bi) = {
            let mut cuts: Vec<Named> = Vec::new();
            let mut pieces: Vec<Named> = Vec::new();
            let mut on: Option<usize> = None;
            let built = (|| -> Result<(), Stop> {
                for point in &points {
                    let at = sketch.point(*point).ok_or_else(|| format!("The punch's point {} no longer exists in its sketch.", point.0))?;
                    let mut found = None;
                    for (b, body) in self.regen.bodies.iter().enumerate() {
                        if let Some(side) = wrap::side_at(&*self.k, body.shape, &frame, at, &opts)? {
                            found = Some((b, side));
                            break;
                        }
                    }
                    let Some((b, side)) = found else {
                        return Err(format!(
                            "The punch at ({}, {}) is not on a flat side of the sheet: its sketch's plane must hold a flat face of the part, with the point on it. Put the sketch on the face to be punched, or use Cut for a cut that goes anywhere.",
                            crate::explain::mm(at.x),
                            crate::explain::mm(at.y)
                        )
                        .into());
                    };
                    if on.is_some_and(|o| o != b) {
                        return Err("The punch's points are on different solids: make a punch for each.".into());
                    }
                    on = Some(b);
                    // Is all of it on the face? What it takes up, turned, about the point.
                    let turn = |v: Vec2| Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
                    let reach: Vec<Vec2> = p.shape.reach(t).into_iter().map(|v| at + turn(v)).collect();
                    if let Some((off, bend)) = side.off_at(&reach) {
                        let (x, y) = (crate::explain::mm((at.x * 100.0).round() / 100.0), crate::explain::mm((at.y * 100.0).round() / 100.0));
                        return Err(if bend {
                            format!(
                                "The punch at ({x}, {y}) goes across a bend, near ({}, {}): a punch stays on one flat face. Move it clear of the bend, or use Cut with Cut Across Bend.",
                                crate::explain::mm((off.x * 100.0).round() / 100.0),
                                crate::explain::mm((off.y * 100.0).round() / 100.0)
                            )
                        } else {
                            format!(
                                "The punch at ({x}, {y}) goes over the edge of its face, near ({}, {}): a punch stays on one flat face, clear of its edges and holes. Move it in, or make it smaller.",
                                crate::explain::mm((off.x * 100.0).round() / 100.0),
                                crate::explain::mm((off.y * 100.0).round() / 100.0)
                            )
                        }
                        .into());
                    }
                    if p.shape.is_formed()
                        && let Some(hole) = side.over_hole(&reach)
                    {
                        return Err(format!(
                            "The punch at ({}, {}) would close the hole near ({}, {}): a formed shape needs whole sheet under it. Move it clear of the hole, or make it smaller.",
                            crate::explain::mm((at.x * 100.0).round() / 100.0),
                            crate::explain::mm((at.y * 100.0).round() / 100.0),
                            crate::explain::mm((hole.x * 100.0).round() / 100.0),
                            crate::explain::mm((hole.y * 100.0).round() / 100.0)
                        )
                        .into());
                    }
                    // The stamp's own frame: X as turned, on the face it looks out of.
                    let x = frame.x() * cos + frame.y() * sin;
                    let centre = frame.plane_point(at);
                    let name = |part: u32| punch::tag(*point, part);
                    if !p.shape.is_formed() {
                        let site = Frame::new(centre, side.normal, x).ok_or("a face of the sheet has no direction")?;
                        let region = p.shape.outline(name);
                        cuts.push(self.sheet_prism(id, &Profile { frame: site, regions: vec![region] }, -t, None)?);
                        continue;
                    }
                    // A formed shape is worked out seen from the face it stands out of: that one,
                    // or with `flip` the sheet's other side.
                    let (up, near) = if p.flip { (-side.normal, centre - side.normal * t) } else { (side.normal, centre) };
                    let y = up.cross(x);
                    match p.shape {
                        PunchShape::Dimple { diameter, depth } => {
                            let (middle, radius, wide) = punch::dimple_section(diameter, depth, t);
                            let base = diameter / 2.0;
                            let site = Frame::new(near, up, x).ok_or("a face of the sheet has no direction")?;
                            let hole = Region {
                                outer: Loop {
                                    curves: vec![TaggedCurve2 { tag: name(0), curve: Curve2::Circle { center: Vec2::ZERO, radius: wide } }],
                                },
                                holes: vec![],
                            };
                            cuts.push(self.sheet_prism(id, &Profile { frame: site, regions: vec![hole] }, -t, None)?);
                            // In a plane through its axis: out along X, up along the way it stands.
                            let section = Frame::new(near, x.cross(up), x).ok_or("a face of the sheet has no direction")?;
                            let dome = punch::PART_DOME;
                            let line = |part: u32, a: (f64, f64), b: (f64, f64)| TaggedCurve2 {
                                tag: name(part),
                                curve: Curve2::Line { start: Vec2::new(a.0, a.1), end: Vec2::new(b.0, b.1) },
                            };
                            let arc = |part: u32, r: f64, from: (f64, f64)| TaggedCurve2 {
                                tag: name(part),
                                curve: Curve2::Arc {
                                    center: Vec2::new(0.0, middle),
                                    radius: r,
                                    start_angle: (from.1 - middle).atan2(from.0),
                                    end_angle: std::f64::consts::FRAC_PI_2,
                                },
                            };
                            let curves = vec![
                                arc(dome, radius, (base, -t)),
                                line(dome + 2, (0.0, depth - t), (0.0, depth)),
                                arc(dome + 1, radius + t, (wide, 0.0)),
                                line(dome + 3, (wide, 0.0), (wide, -t)),
                                line(dome + 4, (wide, -t), (base, -t)),
                            ];
                            let axis = Axis::new(near, up).ok_or("a face of the sheet has no direction")?;
                            let profile = Profile { frame: section, regions: vec![Region { outer: Loop { curves }, holes: vec![] }] };
                            let op = self.revolved(&profile, &axis, &AngleExtent::Full)?;
                            pieces.push(self.sheet_named(id, op, None)?);
                        }
                        PunchShape::Louver { length, width, height } => {
                            let r = sizes.bend_radius;
                            let (turned, straight) = punch::louver_flap(width, height, t, r, sizes.k_factor)?;
                            let site = Frame::new(near, up, x).ok_or("a face of the sheet has no direction")?;
                            let opening = p.shape.outline(name);
                            cuts.push(self.sheet_prism(id, &Profile { frame: site, regions: vec![opening] }, -t, None)?);
                            // Seen along the hinge: across the opening, and up. The bend's inside is
                            // the face the flap stands out of.
                            let path = strip::walk(
                                Vec2::new(-width / 2.0, 0.0),
                                Vec2::X,
                                &[strip::Step::Bend { radius: r, angle: turned }, strip::Step::Straight(straight)],
                            );
                            let flap = punch::PART_FLAP;
                            let region = strip::outline(
                                &path,
                                -t,
                                0.0,
                                |n, left| name(flap + 2 * u32::try_from(n).unwrap_or(0) + u32::from(left)),
                                [name(flap + 8), name(flap + 9)],
                            )?;
                            let section = Frame::new(near - x * (length / 2.0), x, y).ok_or("a face of the sheet has no direction")?;
                            let caps = [name(flap + 10), name(flap + 11)];
                            pieces.push(self.sheet_strip(id, &Profile { frame: section, regions: vec![region] }, &Extent::Distance(length), caps)?);
                        }
                        _ => {}
                    }
                }
                Ok(())
            })();
            if let Err(e) = built {
                for (s, _) in cuts.into_iter().chain(pieces) {
                    self.k.release(s);
                }
                return Err(e);
            }
            (cuts, pieces, on.unwrap_or(0))
        };
        let body = self.body_shape(bi)?;
        self.sheet_assemble(
            bi,
            body,
            cuts,
            pieces,
            Vec::new(),
            "punch",
            ["Move it clear of what it runs into, or make it smaller.", "the stamp does not meet the sheet where it should."],
        )?;
        // What a stamp leaves in the plane of the sheet is the sheet.
        if p.shape.is_formed() {
            self.sheet_unify(bi, id);
        }
        Ok(())
    }
}

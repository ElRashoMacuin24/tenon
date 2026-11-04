//! Regenerating what opens a sheet up: a rip, an unfold and a refold (see `crate::sheet::rip` and
//! `crate::sheet::flatten`).

use tenon_geom::{Axis, Frame, Vec2, Vec3};
use tenon_kernel::{BoolOp, Profile, Region, ShapeHandle, SubShape, Transform};

use super::Named;
use crate::FeatureId;
use crate::naming::{FaceOrigin, names_of_boolean, sub_key};
use crate::regen::{Ctx, Stop};
use crate::sheet::flange::Part;
use crate::sheet::flatten::{self, BendRead, FlatBend, Outline, Rigid, turned};
use crate::sheet::outline;
use crate::sheet::unfold::Unfold;
use crate::sheet::{Refold, Rip, RipWay, UnfoldBends, rip, unfold_opts};

/// How far past a bend, or a strip, sheet is looked for (mm).
const JUST_PAST: f64 = 0.01;

/// What came of trying to lay a bend flat.
enum Tried {
    Done,
    /// The stationary face's sheet reaches both ends of it: something else joins them.
    StillJoined,
    /// The stationary face's sheet reaches neither: the bend is on another piece of the sheet.
    Elsewhere,
}

/// The sheet with a bend laid flat or rolled up: the new solid, its names, and how what came round
/// with the bend moved, with the names of its faces.
struct Moved {
    shape: ShapeHandle,
    names: Vec<Option<FaceOrigin>>,
    by: Rigid,
    moved: Vec<Option<FaceOrigin>>,
}

/// What came of laying out the pieces a bend's going left.
enum Laid {
    Done(Moved),
    StillJoined,
    Elsewhere,
}

impl Ctx<'_> {
    /// Whether sheet of the solid `s` is at `p`.
    fn sheet_at(&mut self, s: ShapeHandle, p: Vec3, t: f64) -> Result<bool, Stop> {
        let probe = self.k.make_sphere(p, t * 0.001)?.shape;
        let near = self.k.min_distance(SubShape::Shape(s), SubShape::Shape(probe)).map(|d| d.value < 1e-6);
        self.k.release(probe);
        Ok(near?)
    }

    /// A thin box in the middle of the sheet, just past an edge of it: from `at` (on that edge, in
    /// the middle of the sheet), `away` from the edge and from `run.0` to `run.1` along it. It goes
    /// into `temp`.
    fn band(&mut self, at: Vec3, away: Vec3, along: Vec3, run: (f64, f64), t: f64, temp: &mut Vec<ShapeHandle>) -> Result<ShapeHandle, Stop> {
        let past = JUST_PAST.min(t);
        let up = away.cross(along);
        let frame = Frame::new(at + along * run.0 + away * (past / 2.0) - up * (t / 4.0), up, away).ok_or("an edge of the sheet has no direction")?;
        let band = self.k.make_box(&frame, Vec3::new(past / 2.0, run.1 - run.0, t / 2.0))?.shape;
        temp.push(band);
        Ok(band)
    }

    /// Whether the solid `s` reaches any of `bands`: sheet goes on past that edge, if only by a
    /// neck, wherever along it that is.
    fn reaches(&mut self, s: ShapeHandle, bands: &[ShapeHandle]) -> Result<bool, Stop> {
        for b in bands {
            if self.k.min_distance(SubShape::Shape(s), SubShape::Shape(*b))?.value < 1e-6 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Each solid of `shape`, with its names.
    fn solids_of(&mut self, shape: ShapeHandle, names: &[Option<FaceOrigin>]) -> Result<Vec<Named>, Stop> {
        let count = self.k.topology(shape)?.solids;
        let mut out: Vec<Named> = Vec::with_capacity(count as usize);
        let mut result = Ok(());
        for i in 0..count {
            match self.k.solid(shape, i) {
                Ok(s) => match self.k.topology(s.shape) {
                    Ok(topo) => out.push((s.shape, names_of_boolean(&s.history, &[names], topo.faces))),
                    Err(e) => {
                        self.k.release(s.shape);
                        result = Err(e);
                        break;
                    }
                },
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        match result {
            Ok(()) => Ok(out),
            Err(e) => {
                for (s, _) in out {
                    self.k.release(s);
                }
                Err(e.into())
            }
        }
    }

    /// A seam opened in a face of the sheet (`crate::sheet::rip`).
    pub(in crate::regen) fn rip(&mut self, id: FeatureId, r: &Rip) -> Result<(), Stop> {
        r.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let opts = unfold_opts(self.doc)?;
        let (bi, face) = self.regen.resolve(&r.face, &*self.k).map_err(|e| self.describe_ref(&r.face, &e))?;
        let body = self.body_shape(bi)?;
        let along = match &r.way {
            RipWay::Point { along: Some(e), .. } => {
                let (b, edge) = self.regen.resolve_edge(e, &*self.k)?;
                let shape = self.body_shape(b)?;
                let info = self.k.edge_info(shape.edge(edge))?;
                if !matches!(info.curve, tenon_kernel::CurveKind::Line { .. }) {
                    return Err("A slit runs alongside a straight edge, and the edge picked for it is curved.".into());
                }
                Some(info.end - info.start)
            }
            _ => None,
        };
        let tools = rip::tools(&*self.k, body, face, &r.way, along, r.gap.unwrap_or(sizes.gap), &opts)?;
        // A rip that is refused leaves the part as it was.
        let (were, names) = (self.k.topology(body)?.solids, self.regen.bodies.get(bi).map(|b| b.names.clone()).unwrap_or_default());
        let kept = self.k.duplicate(body)?;
        if let Err(stop) = self.sheet_tools(id, bi, body, &tools, false) {
            self.k.release(kept);
            return Err(match stop {
                Stop::Failed(m) => Stop::Failed(m.replacen("The cut could not be cut", "The rip could not be cut", 1)),
                other => other,
            });
        }
        let left = self.body_shape(bi)?;
        let refused = match self.k.topology(left).map(|topo| topo.solids) {
            Ok(0) => Some("Taking that out leaves nothing of the part: open a seam with a slit, or pick a smaller face."),
            Ok(now) if now > were => Some(
                "A rip opens a seam and leaves the sheet in one piece, and this one would cut it in two, which could not be laid flat together. Click a straight edge for a slit to run alongside, stop it short of an edge with Point to Point, or use Cut if a piece is to come off.",
            ),
            _ => None,
        };
        let Some(why) = refused else {
            self.k.release(kept);
            return Ok(());
        };
        self.k.release(left);
        if let Some(b) = self.regen.bodies.get_mut(bi) {
            (b.shape, b.names) = (kept, names);
        }
        Err(why.into())
    }

    /// Chosen bends laid flat, one after the other, from a stationary face.
    pub(in crate::regen) fn unfold_bends(&mut self, id: FeatureId, u: &UnfoldBends) -> Result<(), Stop> {
        u.check()?;
        let opts = unfold_opts(self.doc)?;
        let (bi, base) = self.regen.resolve(&u.face, &*self.k).map_err(|e| self.describe_ref(&u.face, &e))?;
        let body = self.body_shape(bi)?;
        let Some(stays) = flatten::point_on_side(&*self.k, body, base, &opts)? else {
            return Err(
                "The stationary face of an unfold is a flat side of the sheet, and the face picked is not one (it is round, or the rim of the sheet). Pick the flat face that is to stay where it is."
                    .into(),
            );
        };
        if u.bends.is_empty() {
            // All of them: until none is left of those on this piece of the sheet.
            let (mut done, mut elsewhere) = (0, Vec::new());
            loop {
                let shape = self.body_shape(bi)?;
                let name = |bodies: &[crate::regen::Body], f: u32| bodies.get(bi).and_then(|b| b.names.get(f as usize).copied().flatten());
                let left = flatten::inner_bends(&*self.k, shape, &opts)?;
                let Some(&face) = left.iter().find(|f| !elsewhere.contains(&name(&self.regen.bodies, **f))) else { break };
                if self.flatten_bend(id, bi, face, stays, &opts)? {
                    done += 1;
                } else {
                    elsewhere.push(name(&self.regen.bodies, face));
                }
            }
            if done == 0 {
                return Err("The sheet has no bend to unfold: it is flat already.".into());
            }
            return Ok(());
        }
        for b in &u.bends {
            // A second face of a bend that has gone is that bend again.
            if self.regen.flat_bends.iter().any(|f| f.unfold == id && f.names.contains(&Some(b.origin))) && self.regen.resolve(b, &*self.k).is_err() {
                continue;
            }
            let (on, face) = self.regen.resolve(b, &*self.k).map_err(|e| self.describe_ref(b, &e))?;
            if on != bi {
                return Err("A bend picked for the unfold is on another solid than its stationary face.".into());
            }
            if !self.flatten_bend(id, bi, face, stays, &opts)? {
                return Err(
                    "A bend picked for the unfold is not on the piece of sheet its stationary face is on: it would have nothing to turn from.".into(),
                );
            }
        }
        Ok(())
    }

    /// Lays the bend face `face` is a side of flat. False when it is on another piece of the sheet
    /// than the stationary face.
    fn flatten_bend(&mut self, id: FeatureId, bi: usize, face: u32, stays: Vec3, opts: &Unfold) -> Result<bool, Stop> {
        let body = self.body_shape(bi)?;
        let names = self.regen.bodies.get(bi).map(|b| b.names.clone()).unwrap_or_default();
        // The bend's key: from its name, and which of that name this is.
        let named = names.get(face as usize).copied().flatten().map_or(u64::from(face), |n| n.key());
        let nth = self.regen.flat_bends.iter().filter(|f| f.unfold == id && f.names.iter().flatten().any(|n| n.key() == named)).count();
        let key = sub_key(named, u32::try_from(nth).unwrap_or(u32::MAX));
        let tag = sub_key(key, 64);
        let bend = flatten::read_bend(&*self.k, body, &names, face, false, tag, opts)?;
        match self.try_flatten(id, bi, body, &names, &bend, stays, key, opts)? {
            Tried::Done => return Ok(true),
            Tried::Elsewhere => return Ok(false),
            Tried::StillJoined => {}
        }
        // A bend in pieces along one line (a slot through it, a hole): they go together.
        let whole = flatten::read_bend(&*self.k, body, &names, face, true, tag, opts)?;
        if whole.faces != bend.faces
            && let Tried::Done = self.try_flatten(id, bi, body, &names, &whole, stays, key, opts)?
        {
            return Ok(true);
        }
        Err("This bend is not all that joins the sheet on its two sides: the sheet closes round on itself (a tube, a box), so it cannot be laid flat. Open a seam in it first with Rip, or Corner Seam > Corner Rip at a corner.".into())
    }

    /// Lays one bend flat: what it is goes, leaving the sheet in pieces; those on the far side
    /// turn back about its axis and come along by its allowance, and the bend's outline, flat,
    /// joins them again.
    #[allow(clippy::too_many_arguments)]
    fn try_flatten(
        &mut self,
        id: FeatureId,
        bi: usize,
        body: ShapeHandle,
        names: &[Option<FaceOrigin>],
        bend: &BendRead,
        stays: Vec3,
        key: u64,
        opts: &Unfold,
    ) -> Result<Tried, Stop> {
        let t = opts.thickness;
        let r = bend.inside;
        let were = self.k.topology(body)?.solids.max(1);
        // What the bend is: as far round as it goes, for each stretch it takes up, clear of its
        // round faces either way.
        let frame = Frame::new(bend.center, bend.dir, bend.start).ok_or("a bend has no direction")?;
        let margin = r.min(t) / 2.0;
        let allowance = bend.allowance();
        let cut_tag = sub_key(key, Part::Cut as u32);
        let mut tools: Vec<Named> = Vec::with_capacity(bend.runs.len());
        for (lo, hi) in &bend.runs {
            let at = |x: f64, y: f64| (Vec2::new(x, y), cut_tag);
            let shape = outline::polygon(&[at(0.0, *lo), at(allowance, *lo), at(allowance, *hi), at(0.0, *hi)]);
            let profile = Profile { frame, regions: vec![Region { outer: outline::to_loop(&shape), holes: vec![] }] };
            match self.sheet_roll(id, &profile, bend.rho, r - margin, r + t + margin, [cut_tag, cut_tag]) {
                Ok(made) => tools.push(made),
                Err(e) => {
                    for (s, _) in tools {
                        self.k.release(s);
                    }
                    return Err(e);
                }
            }
        }
        let cut = self.sheet_cut((body, names.to_vec()), true, tools, "bend")?;
        let mut temp: Vec<ShapeHandle> = vec![cut.0];
        let outcome = self.flatten_pieces(id, &cut, bend, stays, key, were, opts, &mut temp);
        for s in temp {
            self.k.release(s);
        }
        let done = match outcome? {
            Laid::Done(done) => done,
            Laid::StillJoined => return Ok(Tried::StillJoined),
            Laid::Elsewhere => return Ok(Tried::Elsewhere),
        };
        self.k.release(body);
        self.moved_with_it(bi, done);
        Ok(Tried::Done)
    }

    /// The body as it now is, and the strips still flat where what they are on has gone.
    fn moved_with_it(&mut self, bi: usize, done: Moved) {
        if let Some(b) = self.regen.bodies.get_mut(bi) {
            (b.shape, b.names) = (done.shape, done.names);
        }
        for f in &mut self.regen.flat_bends {
            if done.moved.iter().flatten().any(|n| f.holds(n)) {
                f.moved(&done.by);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn flatten_pieces(
        &mut self,
        id: FeatureId,
        cut: &Named,
        bend: &BendRead,
        stays: Vec3,
        key: u64,
        were: u32,
        opts: &Unfold,
        temp: &mut Vec<ShapeHandle>,
    ) -> Result<Laid, Stop> {
        let t = opts.thickness;
        let r = bend.inside;
        let allowance = bend.allowance();
        let lumps = self.solids_of(cut.0, &cut.1)?;
        temp.extend(lumps.iter().map(|l| l.0));
        // Just past each end of the bend, along the stretches it takes.
        let (e0, e1) = (bend.start, bend.finish());
        let mut past = |ctx: &mut Self, e: Vec3, away: Vec3| -> Result<Vec<ShapeHandle>, Stop> {
            bend.runs.iter().map(|run| ctx.band(bend.center + e * (r + t / 2.0), away, bend.dir, *run, t, temp)).collect()
        };
        // Round the bend from its start is along `dir x e`: past its start is back along that.
        let past_start = past(self, e0, -bend.dir.cross(e0))?;
        let past_end = past(self, e1, bend.dir.cross(e1))?;
        // The piece the stationary face is on, and which end of the bend it comes up to.
        let mut stay: Option<usize> = None;
        for (i, l) in lumps.iter().enumerate() {
            if self.sheet_at(l.0, stays, t)? {
                stay = Some(i);
                break;
            }
        }
        let Some(stay) = stay else {
            return Err("The stationary face could not be found on the sheet once the bend was out of it: pick another.".into());
        };
        let at_start = self.reaches(lumps[stay].0, &past_start)?;
        let at_end = self.reaches(lumps[stay].0, &past_end)?;
        let near_start = match (at_start, at_end) {
            (true, true) => return Ok(Laid::StillJoined),
            (true, false) => true,
            (false, true) => false,
            (false, false) => return Ok(Laid::Elsewhere),
        };
        let far_places = if near_start { &past_end } else { &past_start };
        let mut far: Vec<usize> = Vec::new();
        for (i, l) in lumps.iter().enumerate() {
            if i != stay && self.reaches(l.0, far_places)? {
                far.push(i);
            }
        }
        // Seen from the near end: the way it turned, from the axis to there, and the way on.
        let (turn_about, near_e) = if near_start { (bend.dir, e0) } else { (-bend.dir, e1) };
        let on = turn_about.cross(near_e);
        let axis = Axis::new(bend.center, turn_about).ok_or("a bend has no axis")?;
        let by = Rigid { axis, angle: -bend.sweep, shift: on * allowance };
        // The strip: as the bend's outline lies from that end.
        let strip_frame = Frame::new(bend.center + near_e * r, near_e, on).ok_or("a bend has no direction")?;
        let from_near = |o: &Outline| if near_start { o.clone() } else { o.moved(|p| Vec2::new(allowance - p.x, -p.y)) };
        let (inner_tag, outer_tag) = (sub_key(key, Part::BendInner as u32), sub_key(key, Part::BendOuter as u32));
        let mut strips: Vec<Named> = Vec::with_capacity(bend.outlines.len());
        let mut sources = vec![inner_tag, outer_tag];
        // Each hole is pushed through on its own, the way a hole is rolled when folding back: the
        // outline of one that came as points runs whichever way round the solid gave it.
        let margin = r.min(t) / 2.0;
        let through = Frame::new(strip_frame.origin() - near_e * margin, near_e, on).ok_or("a bend has no direction")?;
        for o in &bend.outlines {
            let o = from_near(o);
            sources.extend(o.tags());
            let profile = Profile { frame: strip_frame, regions: vec![Region { outer: o.as_loop(&o.outer), holes: vec![] }] };
            let mut made = self.sheet_prism(id, &profile, t, Some([inner_tag, outer_tag]))?;
            temp.push(made.0);
            for h in &o.holes {
                let tool = Profile { frame: through, regions: vec![Region { outer: o.as_loop(h), holes: vec![] }] };
                let cutter = self.sheet_prism(id, &tool, t + 2.0 * margin, Some([inner_tag, outer_tag]))?;
                made = self.with_hole(&made, cutter, temp)?;
            }
            strips.push(made);
        }
        // The far side, brought round.
        let mut moved: Vec<Named> = Vec::with_capacity(far.len());
        let mut moved_names: Vec<Option<FaceOrigin>> = Vec::new();
        for i in &far {
            let l = &lumps[*i];
            let shape = self.moved_solid(l.0, &by, temp)?;
            moved.push((shape, l.1.clone()));
            moved_names.extend(l.1.iter().copied());
        }
        let others: Vec<&Named> = lumps.iter().enumerate().filter(|(i, _)| *i != stay && !far.contains(i)).map(|(_, l)| l).collect();
        let parts: Vec<&Named> = others.into_iter().chain(strips.iter()).chain(moved.iter()).collect();
        let (shape, whole_names) = self.joined(
            &lumps[stay],
            &parts,
            were,
            "laid flat",
            ["Laid flat, what comes round with this bend runs into the sheet", "Refold it, or unfold the bend it runs into as well."],
        )?;
        let beyond = !far.is_empty();
        self.regen.flat_bends.push(FlatBend {
            unfold: id,
            key,
            origin: bend.center + near_e * r,
            across: on,
            along: turn_about,
            up: near_e,
            run: {
                let lo = bend.runs.first().map_or(0.0, |s| s.0);
                let hi = bend.runs.last().map_or(0.0, |s| s.1);
                if near_start { (lo, hi) } else { (-hi, -lo) }
            },
            allowance,
            sweep: bend.sweep,
            inside: r,
            rho: bend.rho,
            names: bend.names,
            beyond,
            solids: were,
            sources,
        });
        Ok(Laid::Done(Moved { shape, names: whole_names, by, moved: moved_names }))
    }

    /// `made` with `cutter` taken out of it. What is made on the way goes into `temp`.
    fn with_hole(&mut self, made: &Named, cutter: Named, temp: &mut Vec<ShapeHandle>) -> Result<Named, Stop> {
        temp.push(cutter.0);
        let inputs: [&[Option<FaceOrigin>]; 2] = [&made.1, &cutter.1];
        let op = self.k.boolean(BoolOp::Cut, made.0, &[cutter.0])?;
        temp.push(op.shape);
        let faces = self.k.topology(op.shape)?.faces;
        Ok((op.shape, names_of_boolean(&op.history, &inputs, faces)))
    }

    /// A solid moved as `by` says. What is made on the way goes into `temp`.
    fn moved_solid(&mut self, s: ShapeHandle, by: &Rigid, temp: &mut Vec<ShapeHandle>) -> Result<ShapeHandle, Stop> {
        let turned = self.k.transform(s, &Transform::Rotate { axis: by.axis, angle: by.angle })?.shape;
        temp.push(turned);
        let shifted = self.k.transform(turned, &Transform::Translate(by.shift))?.shape;
        temp.push(shifted);
        Ok(shifted)
    }

    /// `first` and `rest` one body again, of what they were: none of them may share space with
    /// another, and they must come to no more than `were` pieces. The shape returned is new.
    fn joined(
        &mut self,
        first: &Named,
        rest: &[&Named],
        were: u32,
        what: &str,
        into_itself: [&str; 2],
    ) -> Result<(ShapeHandle, Vec<Option<FaceOrigin>>), Stop> {
        let volume = |ctx: &Self, s: ShapeHandle| ctx.k.mass_properties(s, 1.0).map(|m| m.volume);
        let mut parts = volume(self, first.0)?;
        for r in rest {
            parts += volume(self, r.0)?;
        }
        let tools: Vec<ShapeHandle> = rest.iter().map(|r| r.0).collect();
        let op = self.k.boolean(BoolOp::Union, first.0, &tools)?;
        let mut inputs: Vec<&[Option<FaceOrigin>]> = vec![first.1.as_slice()];
        inputs.extend(rest.iter().map(|r| r.1.as_slice()));
        let checked = self.k.topology(op.shape).map_err(Stop::from).and_then(|topo| {
            let whole = volume(self, op.shape)?;
            if (whole - parts).abs() > 1e-6 * (1.0 + parts) {
                return Err(format!("{}: {} mm³ of it would be where the sheet already is. {}", into_itself[0], crate::explain::mm((parts - whole).abs()), into_itself[1]).into());
            }
            if topo.solids > were {
                return Err(format!(
                    "The sheet does not hold together {what}: what is beyond the bend would no longer be joined to the rest, because nothing of the bend is left between them. Leave some of the bend whole."
                )
                .into());
            }
            if !self.k.is_valid(op.shape)? {
                return Err(format!(
                    "The sheet {what} does not make a sound solid: something in it cannot be carried across the bend. Suppress the last cut made in or near this bend, or change its size a little."
                )
                .into());
            }
            Ok(names_of_boolean(&op.history, &inputs, topo.faces))
        });
        match checked {
            Ok(names) => Ok((op.shape, names)),
            Err(e) => {
                self.k.release(op.shape);
                Err(e)
            }
        }
    }

    /// Bends an unfold laid flat folded back, the last laid flat first.
    pub(in crate::regen) fn refold(&mut self, id: FeatureId, r: &Refold) -> Result<(), Stop> {
        r.check()?;
        let opts = unfold_opts(self.doc)?;
        let name = crate::regen::label(self.doc, r.unfold);
        if !self.doc.feature(r.unfold).is_some_and(|f| matches!(f.kind, crate::FeatureKind::Unfold(_))) {
            return Err(format!("{name} is not an unfold: a refold folds back the bends an unfold laid flat.").into());
        }
        let flat: Vec<usize> = self.regen.flat_bends.iter().enumerate().filter(|(_, b)| b.unfold == r.unfold).map(|(i, _)| i).collect();
        if flat.is_empty() {
            let why =
                if self.doc.feature(r.unfold).is_some_and(|f| f.suppressed) { "it is suppressed" } else { "they have all been folded back already" };
            return Err(format!("{name} has no bend laid flat to fold back: {why}.").into());
        }
        // Which of them, the last laid flat first.
        let mut chosen: Vec<u64> = Vec::new();
        for b in &r.bends {
            let found = flat.iter().filter_map(|i| self.regen.flat_bends.get(*i)).find(|f| f.holds(&b.origin));
            let Some(f) = found else {
                return Err(format!("A face picked for the refold is not on a flat strip of {name}: pick a face of a strip that bend became.").into());
            };
            if !chosen.contains(&f.key) {
                chosen.push(f.key);
            }
        }
        let keys: Vec<u64> = flat
            .iter()
            .rev()
            .filter_map(|i| self.regen.flat_bends.get(*i))
            .filter(|f| r.bends.is_empty() || chosen.contains(&f.key))
            .map(|f| f.key)
            .collect();
        for key in keys {
            self.fold_back(id, r.unfold, key, &opts)?;
        }
        Ok(())
    }

    /// Whether anything of the solid `s` is in the box of that `size` from `height` up `frame`'s Z.
    fn body_within(&mut self, s: ShapeHandle, frame: &Frame, height: f64, size: Vec3) -> Result<bool, Stop> {
        let corner = Frame::new(frame.origin() + frame.z() * height, frame.z(), frame.x()).ok_or("a flat strip has no direction")?;
        let probe = self.k.make_box(&corner, size)?.shape;
        let inside = self.k.boolean(BoolOp::Intersect, s, &[probe]);
        self.k.release(probe);
        // (The kernel has no common solid to give when there is none.)
        let Ok(op) = inside else { return Ok(false) };
        let solids = self.k.topology(op.shape).map(|topo| topo.solids);
        self.k.release(op.shape);
        Ok(solids? > 0)
    }

    /// Folds one flat strip back: what lies in it is taken out, leaving the sheet in pieces; it is
    /// rolled up, holes and all; and the sheet beyond turns back with it.
    fn fold_back(&mut self, id: FeatureId, unfold: FeatureId, key: u64, opts: &Unfold) -> Result<(), Stop> {
        let t = opts.thickness;
        let Some(at) = self.regen.flat_bends.iter().position(|f| f.unfold == unfold && f.key == key) else { return Ok(()) };
        let bend = self.regen.flat_bends[at].clone();
        let gone = "Nothing is left of a bend's flat strip, so there is nothing to fold back: something cut all of it away. Delete or suppress that cut, or leave some of the strip.";
        let Some(bi) = self.regen.bodies.iter().position(|b| b.names.iter().flatten().any(|n| bend.holds(n))) else {
            return Err(gone.into());
        };
        let body = self.body_shape(bi)?;
        let names = self.regen.bodies.get(bi).map(|b| b.names.clone()).unwrap_or_default();
        let were = self.k.topology(body)?.solids.max(1);
        let tag = sub_key(key, 96);
        let cut_tag = sub_key(tag, Part::Cut as u32);
        let at_xy = |x: f64, y: f64| (Vec2::new(x, y), cut_tag);
        let (lo, hi) = bend.run;
        // Nothing on either face of the strip, raised over it from what lies beside it or not.
        let frame = Frame::new(bend.origin + bend.along * lo, bend.up, bend.across).ok_or("a flat strip has no direction")?;
        let skin = JUST_PAST.min(t);
        for height in [-skin, t] {
            if self.body_within(body, &frame, height, Vec3::new(bend.allowance, hi - lo, skin))? {
                return Err(flatten::NOT_SHEET.into());
            }
        }
        let shape = outline::polygon(&[at_xy(0.0, lo), at_xy(bend.allowance, lo), at_xy(bend.allowance, hi), at_xy(0.0, hi)]);
        let frame = Frame::new(bend.origin, bend.up, bend.across).ok_or("a flat strip has no direction")?;
        let profile = Profile { frame, regions: vec![Region { outer: outline::to_loop(&shape), holes: vec![] }] };
        let zone = self.sheet_prism(id, &profile, t, Some([cut_tag, cut_tag]))?;
        // What is in the strip now, and what is not.
        let strip = match self.k.boolean(BoolOp::Intersect, body, &[zone.0]) {
            Ok(op) => op.shape,
            Err(_) => {
                self.k.release(zone.0);
                return Err(gone.into());
            }
        };
        let mut temp: Vec<ShapeHandle> = vec![strip];
        let outlines =
            if self.k.topology(strip)?.solids == 0 { Err(gone.to_owned()) } else { flatten::strip_outlines(&*self.k, strip, &bend, tag, opts) };
        let outlines = match outlines {
            Ok(o) if !o.is_empty() => o,
            Ok(_) => {
                self.k.release(zone.0);
                self.k.release(strip);
                return Err(gone.into());
            }
            Err(e) => {
                self.k.release(zone.0);
                self.k.release(strip);
                return Err(e.into());
            }
        };
        let cut = match self.sheet_cut((body, names), true, vec![zone], "bend's strip") {
            Ok(c) => c,
            Err(e) => {
                self.k.release(strip);
                return Err(e);
            }
        };
        temp.push(cut.0);
        let result = self.fold_pieces(id, &bend, &cut, &outlines, were, opts, &mut temp);
        for s in temp {
            self.k.release(s);
        }
        let done = result?;
        self.k.release(body);
        self.regen.flat_bends.remove(at);
        self.moved_with_it(bi, done);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn fold_pieces(
        &mut self,
        id: FeatureId,
        bend: &FlatBend,
        cut: &Named,
        outlines: &[Outline],
        were: u32,
        opts: &Unfold,
        temp: &mut Vec<ShapeHandle>,
    ) -> Result<Moved, Stop> {
        let t = opts.thickness;
        let r = bend.inside;
        let lumps = self.solids_of(cut.0, &cut.1)?;
        temp.extend(lumps.iter().map(|l| l.0));
        // Which side of the strip each piece is on.
        let middle = bend.origin + bend.up * (t / 2.0);
        let near_band = [self.band(middle, -bend.across, bend.along, bend.run, t, temp)?];
        let far_band = [self.band(middle + bend.across * bend.allowance, bend.across, bend.along, bend.run, t, temp)?];
        let (mut near, mut far, mut others): (Vec<usize>, Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new(), Vec::new());
        for (i, l) in lumps.iter().enumerate() {
            let (n, f) = (self.reaches(l.0, &near_band)?, self.reaches(l.0, &far_band)?);
            match (n, f) {
                (true, true) => {
                    return Err(
                        "A bend cannot be folded back while something else joins the sheet on its two sides: sheet added across its flat strip, or beside it. Delete that, or do it after the refold."
                            .into(),
                    );
                }
                (true, false) => near.push(i),
                (false, true) => far.push(i),
                (false, false) => others.push(i),
            }
        }
        // What was on its own before stays on its own; anything more stood on the strip.
        if others.len() as u32 + u32::from(!near.is_empty() || !far.is_empty()) > were {
            return Err(
                "A bend cannot be folded back with something joined to its flat strip alone, such as a flange on the strip's edge or a boss on it: only the sheet goes round a bend. Delete that, or make it after the refold."
                    .into(),
            );
        }
        let Some(&first) = near.first() else {
            return Err("The sheet does not hold together round this bend: nothing is left on the side of it that stays. Leave some of the sheet beside the bend's strip.".into());
        };
        // A cut right across the strip, or right along it, has left nothing to turn what is
        // beyond it: it would be left where it lies, or fall apart at the bend.
        if (bend.beyond && far.is_empty() && !others.is_empty()) || (!far.is_empty() && !outlines.iter().any(|o| o.spans(bend.allowance))) {
            return Err(
                "The sheet does not hold together round this bend: a cut goes right across its flat strip, or along it, so what is beyond the bend would no longer be joined to the rest. Leave some of the strip whole from one side to the other."
                    .into(),
            );
        }
        // A piece cut off since the bend was laid flat, beyond the strip: nothing turns it, and it
        // would be left lying where it is as the rest folds back.
        if were > bend.solids {
            for i in &others {
                let at = self.k.mass_properties(lumps[*i].0, 1.0)?.center_of_mass;
                if (at - bend.origin).dot(bend.across) > bend.allowance / 2.0 {
                    return Err(
                        "The sheet does not hold together round this bend: a cut has parted a piece from the sheet beyond its flat strip, so that piece would be left lying where it is. Delete that cut, or make it after the refold."
                            .into(),
                    );
                }
            }
        }
        // Rolled up about the axis it turned about, from the side that stays.
        let center = bend.center();
        let turn_frame = Frame::new(center, bend.along, bend.up).ok_or("a flat strip has no direction")?;
        let (inner_tag, outer_tag) = (sub_key(bend.key, Part::BendInner as u32 + 96), sub_key(bend.key, Part::BendOuter as u32 + 96));
        let margin = r.min(t) / 2.0;
        let mut rolled: Vec<Named> = Vec::with_capacity(outlines.len());
        for o in outlines {
            let profile = Profile { frame: turn_frame, regions: vec![Region { outer: o.as_rolled_loop(&o.outer), holes: vec![] }] };
            let mut made = self.sheet_roll(id, &profile, bend.rho, r, r + t, [inner_tag, outer_tag])?;
            temp.push(made.0);
            // A hole in it: rolled too, and cut out.
            for h in &o.holes {
                let tool = Profile { frame: turn_frame, regions: vec![Region { outer: o.as_rolled_loop(h), holes: vec![] }] };
                let cutter = self.sheet_roll(id, &tool, bend.rho, r - margin, r + t + margin, [inner_tag, outer_tag])?;
                made = self.with_hole(&made, cutter, temp)?;
            }
            // Its round faces are called what they were before it was laid flat.
            for n in &mut made.1 {
                match n {
                    Some(FaceOrigin::From { feature, source, .. }) if *feature == id && *source == inner_tag => {
                        if let Some(old) = bend.names[0] {
                            *n = Some(old);
                        }
                    }
                    Some(FaceOrigin::From { feature, source, .. }) if *feature == id && *source == outer_tag => {
                        if let Some(old) = bend.names[1] {
                            *n = Some(old);
                        }
                    }
                    _ => {}
                }
            }
            rolled.push(made);
        }
        // The far side turned back with it. (Turned, then moved: the step is what the turn makes of
        // the step along the strip.)
        let axis = Axis::new(center, bend.along).ok_or("a flat strip has no axis")?;
        let by = Rigid { axis, angle: bend.sweep, shift: -turned(bend.across, bend.along, bend.sweep) * bend.allowance };
        let mut moved: Vec<Named> = Vec::with_capacity(far.len());
        let mut moved_names: Vec<Option<FaceOrigin>> = Vec::new();
        for i in &far {
            let l = &lumps[*i];
            let shape = self.moved_solid(l.0, &by, temp)?;
            moved.push((shape, l.1.clone()));
            moved_names.extend(l.1.iter().copied());
        }
        let parts: Vec<&Named> = near.iter().skip(1).chain(others.iter()).map(|i| &lumps[*i]).chain(rolled.iter()).chain(moved.iter()).collect();
        let (shape, names) = self.joined(
            &lumps[first],
            &parts,
            were,
            "folded back",
            [
                "Folded back, what comes round with this bend runs into the sheet",
                "Something made while it was flat may be in the way: move it, or fold the bend back first.",
            ],
        )?;
        Ok(Moved { shape, names, by, moved: moved_names })
    }
}

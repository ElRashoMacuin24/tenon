//! Regenerating sheet metal features: faces and flanges (see `crate::sheet`).

use tenon_geom::Vec2;
use tenon_kernel::{BoolOp, CurveKind, Extent, Op, Origin, PrimitiveRole, Profile, Region, ShapeHandle, SurfaceKind, TopoKind, Topology};
use tenon_sketch::{Sketch, profile, regions};

use super::{Ctx, Stop, label, select};
use crate::FeatureId;
use crate::naming::{FaceOrigin, edge_key, names_of_boolean, names_of_sweep, pair_key, resolve_edge, sub_key};
use crate::sheet::flange::{self, Notch, Part, Site};
use crate::sheet::outline::{self, Cutter};
use crate::sheet::strip;
use crate::sheet::{ContourFlange, Face, Flange, Hem, Sides};

/// A solid on its way into the part, and the names of its faces.
type Named = (ShapeHandle, Vec<Option<FaceOrigin>>);

/// One edge of a flange feature.
struct Wall {
    site: Site,
    /// The key of the edge, for the names of what the flange makes on it.
    key: u64,
    /// The stretch of the edge the flange takes.
    span: (f64, f64),
    /// The ends where the flange meets that of another edge of the feature at a corner of the
    /// sheet: the angle between the two edges there.
    meets: [Option<f64>; 2],
}

/// Where two walls of one feature meet at a corner of the sheet: what the corner's relief takes
/// out of the sheet, seen from above the first wall (out of the sheet, along its edge), and the
/// corner's key.
struct Corner {
    wall: usize,
    cutter: Cutter,
    key: u64,
}

impl Ctx<'_> {
    pub(super) fn sheet_face(&mut self, id: FeatureId, s: &Face) -> Result<(), Stop> {
        s.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let (held, frame) = self.sketch_and_frame(s.sketch)?;
        let sketch: &Sketch = &held;
        let all = regions(sketch);
        let profile = profile(sketch, frame, &select(sketch, &all, &s.regions)?);
        let distance = if s.flip { -sizes.thickness } else { sizes.thickness };
        let op = self.k.extrude(&profile, &Extent::Distance(distance), None)?;
        let faces = self.k.topology(op.shape)?.faces;
        let names = names_of_sweep(&op.history, id, faces);
        self.combine(id, vec![(op.shape, names)], s.operation)
    }

    /// Where a flange grows from edge `e` of `body`: the edge must be straight, between a face
    /// of the sheet and the rim (the face one thickness deep beside it).
    fn flange_site(&self, body: ShapeHandle, topo: &Topology, e: u32, t: f64, flip: bool) -> Result<Site, Stop> {
        let info = self.k.edge_info(body.edge(e))?;
        if !matches!(info.curve, CurveKind::Line { .. }) {
            return Err("A flange goes on a straight edge, and an edge picked for it is curved.".into());
        }
        let adjacent = topo.edge_faces.get(e as usize).map_or(&[][..], Vec::as_slice);
        let &[fa, fb] = adjacent else { return Err("A flange goes on an edge between two faces of the sheet.".into()) };
        let d = (info.end - info.start).normalized();
        // The rim is the face that has, one thickness away, the matching edge of the sheet's
        // other side.
        let mut sides = Vec::with_capacity(2);
        for f in [fa, fb] {
            let face = self.k.face_info(body.face(f))?;
            let SurfaceKind::Plane { normal, .. } = face.surface else {
                return Err("A flange goes on an edge between two flat faces of the sheet, and a face beside this edge is not flat.".into());
            };
            let mut rim = false;
            for &other in topo.face_edges.get(f as usize).map_or(&[][..], Vec::as_slice) {
                if other == e {
                    continue;
                }
                let o = self.k.edge_info(body.edge(other))?;
                if !matches!(o.curve, CurveKind::Line { .. }) || (o.end - o.start).normalized().cross(d).len() > 1e-6 {
                    continue;
                }
                let off = o.start - info.start;
                if ((off - d * off.dot(d)).len() - t).abs() > 1e-6 * (1.0 + t) {
                    continue;
                }
                let (b0, b1) = ((o.start - info.start).dot(d), (o.end - info.start).dot(d));
                rim |= b0.max(b1).min(info.length) - b0.min(b1).max(0.0) > 1e-6;
            }
            sides.push((normal.normalized(), face.area, rim));
        }
        let skin = match (sides[0].2, sides[1].2) {
            (false, true) => 0,
            (true, false) => 1,
            // A strip as wide as it is thick: the larger face is the sheet's.
            (true, true) if (sides[0].1 - sides[1].1).abs() > 1e-9 * sides[0].1.max(1.0) => usize::from(sides[1].1 > sides[0].1),
            (true, true) => return Err("The sheet is as wide as it is thick here, so it is not clear which face the flange bends from.".into()),
            (false, false) => {
                return Err(format!(
                    "This edge is not on the rim of a sheet {} mm thick: a flange goes on an edge of the sheet's own outline. (Check the thickness in Sheet Metal Defaults.)",
                    crate::explain::mm(t)
                )
                .into());
            }
        };
        let (up, out) = (sides[skin].0, sides[1 - skin].0);
        let along = out.cross(up);
        if (along.len() - 1.0).abs() > 1e-6 {
            return Err("The rim is not square to the sheet at this edge, so a flange cannot start there.".into());
        }
        let origin = if (info.end - info.start).dot(along) > 0.0 { info.start } else { info.end };
        let site = Site { origin, out, up, along, length: info.length };
        // Bent the other way, it is the same edge seen from the sheet's other side.
        Ok(if flip { Site { origin: site.at(0.0, -t, info.length), out, up: -up, along: -along, length: info.length } } else { site })
    }

    /// One boolean on the way to the new body. The tools are used up; `target` is released when
    /// it is not the part's own body (`keep`).
    fn sheet_step(&mut self, kind: BoolOp, target: Named, keep: bool, tools: Vec<Named>) -> Result<Named, Stop> {
        let shapes: Vec<ShapeHandle> = tools.iter().map(|t| t.0).collect();
        let done = self.k.boolean(kind, target.0, &shapes).map_err(Stop::from).and_then(|op| match self.k.topology(op.shape) {
            Ok(topo) => {
                let inputs: Vec<&[Option<FaceOrigin>]> = std::iter::once(target.1.as_slice()).chain(tools.iter().map(|t| t.1.as_slice())).collect();
                Ok((op.shape, names_of_boolean(&op.history, &inputs, topo.faces)))
            }
            Err(e) => {
                self.k.release(op.shape);
                Err(e.into())
            }
        });
        for s in shapes {
            self.k.release(s);
        }
        if !keep {
            self.k.release(target.0);
        }
        done
    }

    /// A profile pushed `distance` along its frame's Z, every face named after the curve of
    /// the profile that made it and its two ends by `caps`.
    fn sheet_prism(&mut self, id: FeatureId, profile: &Profile, distance: f64, caps: Option<[u64; 2]>) -> Result<Named, Stop> {
        let op = self.k.extrude(profile, &Extent::Distance(distance), None)?;
        self.sheet_named(id, op, caps)
    }

    /// A flat outline rolled into a bend (`Kernel::roll`), named as a prism is: its inside is
    /// the first of `caps`, its outside the second.
    fn sheet_roll(&mut self, id: FeatureId, profile: &Profile, rho: f64, inner: f64, outer: f64, caps: [u64; 2]) -> Result<Named, Stop> {
        let op = self.k.roll(profile, rho, inner, outer)?;
        self.sheet_named(id, op, Some(caps))
    }

    fn sheet_named(&mut self, id: FeatureId, op: Op, caps: Option<[u64; 2]>) -> Result<Named, Stop> {
        let faces = match self.k.topology(op.shape) {
            Ok(t) => t.faces,
            Err(e) => {
                self.k.release(op.shape);
                return Err(e.into());
            }
        };
        let mut names = vec![None; faces as usize];
        let mut name = |i: u32, source: u64| {
            if let Some(slot) = names.get_mut(i as usize) {
                *slot = Some(FaceOrigin::From { feature: id, source, ordinal: 0 });
            }
        };
        for g in &op.history.generated {
            if let Origin::ProfileCurve { tag } = g.origin {
                for t in g.result.iter().filter(|t| t.kind == TopoKind::Face) {
                    name(t.index, tag);
                }
            }
        }
        if let Some([start, end]) = caps {
            for t in op.history.roles_of(PrimitiveRole::StartCap) {
                name(t.index, start);
            }
            for t in op.history.roles_of(PrimitiveRole::EndCap) {
                name(t.index, end);
            }
        }
        Ok((op.shape, names))
    }

    pub(super) fn flange(&mut self, id: FeatureId, fl: &Flange) -> Result<(), Stop> {
        fl.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let rule = self.doc.sheet_metal().cloned().unwrap_or_default();
        let dims = flange::Shape::new(&sizes, fl.radius, fl.angle, fl.height, fl.datum, fl.position)?;
        let t = sizes.thickness;
        // The radius of the bend's neutral surface: what the flat pattern unrolls the bend by, and
        // so what places a corner relief in the folded part.
        let rho = dims.r + sizes.k_factor * t;
        let corner_rule = flange::Corners { shape: rule.corner, size: sizes.corner_size, gap: fl.miter.then_some(sizes.gap) };

        let (bi, body, found) = self.sheet_edges(&fl.edges, "flange")?;
        let topo = self.k.topology(body)?;
        let mut walls: Vec<Wall> = Vec::with_capacity(found.len());
        for (e, key) in &found {
            let site = self.flange_site(body, &topo, *e, t, fl.flip)?;
            let span = flange::span(&fl.width, site.length)?;
            walls.push(Wall { site, key: *key, span, meets: [None, None] });
        }

        // Where two of the walls meet at a corner of the sheet, each bend starts where the other's
        // bend line crosses its own, and the corner gets the rule's relief.
        let mut corners: Vec<Corner> = Vec::new();
        for i in 0..walls.len() {
            for j in i + 1..walls.len() {
                let (a, b) = (walls[i].site, walls[j].site);
                if a.up.dot(b.up) < 0.999 {
                    continue;
                }
                for (ea, eb) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                    let end = |s: &Site, e: usize| s.at(0.0, 0.0, if e == 0 { 0.0 } else { s.length });
                    if end(&a, ea).dist(end(&b, eb)) > 1e-6 * (1.0 + t) {
                        continue;
                    }
                    // Only when both flanges come all the way to the corner.
                    let reaches = |w: &Wall, e: usize| if e == 0 { w.span.0 < 1e-9 } else { w.site.length - w.span.1 < 1e-9 };
                    if !(reaches(&walls[i], ea) && reaches(&walls[j], eb)) {
                        continue;
                    }
                    // Along each edge away from the corner.
                    let (da, db) = (if ea == 0 { a.along } else { -a.along }, if eb == 0 { b.along } else { -b.along });
                    let (sin, cos) = (da.cross(db).len(), da.dot(db));
                    if sin < 1e-6 {
                        // One straight side in two pieces: the flanges simply run on.
                        continue;
                    }
                    if a.out.dot(db) > -1e-9 || b.out.dot(da) > -1e-9 {
                        return Err(
                            "Two of the flange's edges meet in an inside corner of the sheet, where their bends would run into each other: give the flange offsets from that end, or make them two flanges."
                                .into(),
                        );
                    }
                    let corner = cos.clamp(-1.0, 1.0).acos();
                    let back = dims.back(corner);
                    for (w, e) in [(i, ea), (j, eb)] {
                        let wall = &mut walls[w];
                        if e == 0 {
                            wall.span.0 = wall.span.0.max(back);
                        } else {
                            wall.span.1 = wall.span.1.min(wall.site.length - back);
                        }
                        wall.meets[e] = Some(corner);
                    }
                    if let Some(cutter) = dims.relief(rho, corner, &corner_rule) {
                        // From the flat of the first wall (out from where its bend starts) to the
                        // sheet seen from above (out from its edge).
                        let cutter = flange::at_end(&cutter, ea, walls[i].site.length).moved(Vec2::new(-dims.setback, 0.0));
                        corners.push(Corner { wall: i, cutter, key: pair_key(walls[i].key, walls[j].key) });
                    }
                }
            }
        }
        if walls.iter().any(|w| w.span.1 - w.span.0 < 1e-6) {
            return Err(
                "The bends of the flanges beside it leave nothing of one of the edges: use a smaller bend radius, or leave that edge out.".into()
            );
        }

        // The solids: what is cut from the sheet for the bends and at the corners, and the bends
        // and walls themselves. On a failure everything made so far is released.
        let margin = t;
        let mut cuts: Vec<Named> = Vec::new();
        let mut prisms: Vec<Named> = Vec::new();
        let built = (|| -> Result<(), Stop> {
            for w in &walls {
                if dims.setback > 1e-9 {
                    let shared = [w.meets[0].is_some(), w.meets[1].is_some()];
                    let notch = Notch::new(w.span, w.site.length, &dims, &sizes, rule.relief, shared);
                    let frame = w.site.plan_frame(margin).ok_or("the flange's edge has no direction")?;
                    let made =
                        self.sheet_prism(id, &flange::profile(frame, notch.outline(sub_key(w.key, Part::Cut as u32))), t + 2.0 * margin, None)?;
                    cuts.push(made);
                }
                let name = |p: Part| sub_key(w.key, p as u32);
                if w.meets != [None, None] {
                    // A flange that meets another: its bend and its wall each from its outline in
                    // the flat, with the corner's relief out of both.
                    let pieces = flange::pieces(&dims, rho, w.span, w.site.length, w.meets, &corner_rule, name)?;
                    let frame = w.site.bend_frame(&dims).ok_or("the flange's edge has no direction")?;
                    let bend = Profile { frame, regions: vec![Region { outer: outline::to_loop(&pieces.bend), holes: vec![] }] };
                    let made = self.sheet_roll(id, &bend, rho, dims.r, dims.r + t, [name(Part::BendInner), name(Part::BendOuter)])?;
                    prisms.push(made);
                    if !pieces.wall.is_empty() {
                        let frame = w.site.wall_frame(&dims).ok_or("the flange's edge has no direction")?;
                        let up: Vec<outline::Piece> = pieces.wall.iter().map(|p| p.moved(Vec2::new(-dims.allowance(rho), 0.0))).collect();
                        let wall = Profile { frame, regions: vec![Region { outer: outline::to_loop(&up), holes: vec![] }] };
                        let made = self.sheet_prism(id, &wall, t, Some([name(Part::Inner), name(Part::Outer)]))?;
                        prisms.push(made);
                    }
                    continue;
                }
                let frame = w.site.section_frame(w.span.0).ok_or("the flange's edge has no direction")?;
                let section = Profile { frame, regions: vec![dims.section(|p| sub_key(w.key, p as u32))] };
                let caps = [sub_key(w.key, Part::Start as u32), sub_key(w.key, Part::End as u32)];
                let made = self.sheet_prism(id, &section, w.span.1 - w.span.0, Some(caps))?;
                prisms.push(made);
            }
            for c in &corners {
                let outline = c.cutter.outline(sub_key(c.key, Part::Corner as u32));
                let frame = walls[c.wall].site.plan_frame(margin).ok_or("the flange's edge has no direction")?;
                let made = self.sheet_prism(id, &flange::profile(frame, outline), t + 2.0 * margin, None)?;
                cuts.push(made);
            }
            Ok(())
        })();
        if let Err(e) = built {
            for (s, _) in cuts.into_iter().chain(prisms) {
                self.k.release(s);
            }
            return Err(e);
        }

        self.sheet_assemble(
            bi,
            body,
            cuts,
            prisms,
            "flange",
            [
                "Make it lower or narrower, or bend it the other way.",
                "its bend starts further in than the sheet reaches there. Use a smaller bend radius or another bend position.",
            ],
        )
    }

    /// The edges a flange or a hem is on: the body they are all on (its index and shape), and
    /// each edge's number in it with the key its faces are named after.
    fn sheet_edges(&mut self, edges: &[crate::naming::EdgeRef], what: &str) -> Result<(usize, ShapeHandle, Vec<(u32, u64)>), Stop> {
        let mut found: Vec<(usize, u32, u64)> = Vec::with_capacity(edges.len());
        {
            let views = self.regen.views();
            for e in edges {
                let (b, i) = resolve_edge(e, &views, &*self.k).map_err(|m| {
                    let [a, c] = &e.faces;
                    format!(
                        "{m} (the edge between the {} and the {})",
                        a.describe(&label(self.doc, a.feature())),
                        c.describe(&label(self.doc, c.feature()))
                    )
                })?;
                if found.iter().any(|f| f.0 == b && f.1 == i) {
                    return Err(format!("An edge is picked twice for the {what}.").into());
                }
                found.push((b, i, edge_key(&e.faces[0], &e.faces[1])));
            }
        }
        let bi = found.first().map(|f| f.0).ok_or_else(|| format!("select at least one edge for the {what}"))?;
        if found.iter().any(|f| f.0 != bi) {
            return Err("the edges are on different bodies".into());
        }
        let body = self.body_shape(bi)?;
        Ok((bi, body, found.into_iter().map(|f| (f.1, f.2)).collect()))
    }

    /// Makes the part's body `bi` what it is with `cuts` taken out of it and `pieces` joined on.
    /// The pieces must sit on the sheet without sharing space with it or with one another, and
    /// leave it in one piece: `advice` says what to do about each when they do not. Everything
    /// given is used up or released.
    fn sheet_assemble(
        &mut self,
        bi: usize,
        body: ShapeHandle,
        cuts: Vec<Named>,
        pieces: Vec<Named>,
        what: &str,
        advice: [&str; 2],
    ) -> Result<(), Stop> {
        let volume = |ctx: &Self, s: ShapeHandle| ctx.k.mass_properties(s, 1.0).map(|m| m.volume);
        let added: f64 = pieces.iter().map(|p| volume(self, p.0).unwrap_or(0.0)).sum();
        let Some(names) = self.regen.bodies.get(bi).map(|b| b.names.clone()) else {
            for (s, _) in cuts.into_iter().chain(pieces) {
                self.k.release(s);
            }
            return Err("no such body".into());
        };
        let mut now: Named = (body, names);
        let mut own = true;
        let steps: [(BoolOp, Vec<Named>); 2] = [(BoolOp::Cut, cuts), (BoolOp::Union, pieces)];
        let mut pending = steps.into_iter();
        let mut before_join = 0.0;
        let result = (|| -> Result<(), Stop> {
            for (n, (kind, tools)) in pending.by_ref().enumerate() {
                if tools.is_empty() {
                    continue;
                }
                if n == 1 {
                    before_join = volume(self, now.0)?;
                }
                now = self.sheet_step(kind, now.clone(), own, tools)?;
                own = false;
                if n == 1 {
                    let after = volume(self, now.0)?;
                    let lost = before_join + added - after;
                    if lost > 1e-6 * (1.0 + after) {
                        return Err(format!(
                            "The {what} runs into the part: {} mm³ of it would be where the part already is. {}",
                            crate::explain::mm(lost),
                            advice[0]
                        )
                        .into());
                    }
                    if self.k.topology(now.0)?.solids != 1 {
                        return Err(format!("The {what} does not join the sheet: {}", advice[1]).into());
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            // What was not used yet, and what was made on the way.
            for (s, _) in pending.flat_map(|(_, tools)| tools) {
                self.k.release(s);
            }
            if !own {
                self.k.release(now.0);
            }
            return Err(e);
        }
        if !own {
            self.k.release(body);
            if let Some(b) = self.regen.bodies.get_mut(bi) {
                (b.shape, b.names) = now;
            }
        }
        Ok(())
    }

    /// A strip's cross-section pushed along its profile's normal as far as `extent` says, named
    /// as a prism is.
    fn sheet_strip(&mut self, id: FeatureId, profile: &Profile, extent: &Extent, caps: [u64; 2]) -> Result<Named, Stop> {
        let op = self.k.extrude(profile, extent, None)?;
        self.sheet_named(id, op, Some(caps))
    }

    pub(super) fn hem(&mut self, id: FeatureId, h: &Hem) -> Result<(), Stop> {
        h.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let t = sizes.thickness;
        // Seen end on: out of the sheet along its face, which is the inside of every fold.
        let path = strip::walk(Vec2::ZERO, Vec2::X, &h.steps(t)?);
        if strip::crosses(&path, -t, 0.0) {
            return Err("The hem folds into itself: make it shorter, or its gap wider.".into());
        }
        let (bi, body, found) = self.sheet_edges(&h.edges, "hem")?;
        let topo = self.k.topology(body)?;
        let mut pieces: Vec<Named> = Vec::new();
        let built = (|| -> Result<(), Stop> {
            for (e, key) in &found {
                let site = self.flange_site(body, &topo, *e, t, h.flip)?;
                let span = flange::span(&h.width, site.length).map_err(|m| m.replace("The flange", "The hem"))?;
                let name = |part: u32| sub_key(*key, part);
                // Past the parts of a flange: two faces to every step of the hem.
                let region = strip::outline(
                    &path,
                    -t,
                    0.0,
                    |i, left| name(32 + 2 * u32::try_from(i).unwrap_or(0) + u32::from(left)),
                    [name(Part::Root as u32), name(Part::Tip as u32)],
                )?;
                let frame = site.section_frame(span.0).ok_or("the hem's edge has no direction")?;
                let profile = Profile { frame, regions: vec![region] };
                let width = span.1 - span.0;
                let caps = [name(Part::Start as u32), name(Part::End as u32)];
                let made = self.sheet_strip(id, &profile, &Extent::Distance(width), caps)?;
                pieces.push(made);
            }
            Ok(())
        })();
        if let Err(e) = built {
            for (s, _) in pieces {
                self.k.release(s);
            }
            return Err(e);
        }
        self.sheet_assemble(
            bi,
            body,
            Vec::new(),
            pieces,
            "hem",
            [
                "Give it a larger gap or radius, make it shorter, or fold it to the other side.",
                "the edge picked for it is not on the rim of the sheet.",
            ],
        )
    }

    pub(super) fn contour_flange(&mut self, id: FeatureId, c: &ContourFlange) -> Result<(), Stop> {
        c.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let t = sizes.thickness;
        let (held, frame) = self.sketch_and_frame(c.sketch)?;
        let sketch: &Sketch = &held;
        let (drawn, ids) = contour_path(sketch)?;
        let (lo, hi) = c.reach(t);
        let path = strip::rounded(&drawn, c.radius.unwrap_or(sizes.bend_radius), lo, hi)?;
        // A face is named after the curve of the sketch it follows (a bend made at a corner:
        // after the two lines it joins) and the side of the sheet it is.
        let key = |i: usize| sub_key(u64::from(ids.get(i).map_or(0, |e| e.0)), 1);
        let tag = |n: usize, left: bool| {
            let [a, b] = path.get(n).map_or([0, 0], |p| p.1);
            sub_key(if a == b { key(a) } else { pair_key(key(a), key(b)) }, 2 + u32::from(left))
        };
        let runs: Vec<strip::Run> = path.iter().map(|p| p.0).collect();
        if strip::crosses(&runs, lo, hi) {
            return Err("The contour flange folds into itself: its profile comes back through its own path. Shorten the part that does, or draw it clear of the rest.".into());
        }
        let region = strip::outline(&runs, lo, hi, tag, [sub_key(0, Part::Root as u32), sub_key(0, Part::Tip as u32)])?;
        let extent = match c.direction {
            Sides::Default => Extent::Distance(c.distance),
            Sides::Flipped => Extent::Distance(-c.distance),
            Sides::Symmetric => Extent::Symmetric(c.distance),
        };
        let profile = Profile { frame, regions: vec![region] };
        let caps = [sub_key(0, Part::Start as u32), sub_key(0, Part::End as u32)];
        let made = self.sheet_strip(id, &profile, &extent, caps)?;
        self.combine(id, vec![made], c.operation)
    }
}

/// The open profile a sketch holds: its lines and arcs (not construction or reference geometry)
/// in order from one end to the other, with the sketch entity each is. The profile runs so that
/// its first turn is to the left: the left of it is then the inside of its first corner.
fn contour_path(sketch: &Sketch) -> Result<(Vec<strip::Run>, Vec<tenon_sketch::EntityId>), String> {
    use tenon_sketch::{EntityId, Geometry};
    // Each curve by where it starts and ends: curves join where their ends are at one place
    // (one point of the sketch, or two that coincide).
    let mut left: Vec<(EntityId, Vec2, Vec2)> = Vec::new();
    for (id, e) in sketch.entities() {
        if e.construction || e.reference {
            continue;
        }
        match &e.geometry {
            Geometry::Point { .. } => {}
            Geometry::Line { start, end } | Geometry::Arc { start, end, .. } => match (sketch.point(*start), sketch.point(*end)) {
                (Some(a), Some(b)) => left.push((id, a, b)),
                _ => return Err(format!("the profile's e{} cannot be read", id.0)),
            },
            other => {
                return Err(format!(
                    "A contour flange follows lines and arcs, and the sketch has a {} in it: make it construction geometry, or draw the profile without it.",
                    other.kind_name()
                ));
            }
        }
    }
    if left.is_empty() {
        return Err("The sketch has no profile: draw the lines and arcs the sheet is to follow, end to end.".into());
    }
    let same = |a: Vec2, b: Vec2| a.dist(b) <= 1e-6;
    let meeting = |p: Vec2| left.iter().filter(|s| same(s.1, p)).count() + left.iter().filter(|s| same(s.2, p)).count();
    if left.iter().any(|s| meeting(s.1) > 2 || meeting(s.2) > 2) {
        return Err("The profile branches: more than two of its curves meet at one point. A contour flange follows one run of lines and arcs.".into());
    }
    // From the free end of the curve with the lowest number.
    let Some(mut at) = left.iter().find_map(|s| [s.1, s.2].into_iter().find(|p| meeting(*p) == 1)) else {
        return Err("The profile is closed, and a contour flange follows an open one: leave a gap in it (a closed profile makes a Face).".into());
    };
    let mut runs = Vec::with_capacity(left.len());
    let mut ids = Vec::with_capacity(left.len());
    while let Some(i) = left.iter().position(|s| same(s.1, at) || same(s.2, at)) {
        let (id, a, b) = left.remove(i);
        let forward = same(a, at);
        at = if forward { b } else { a };
        let run = if let Some((p, q)) = sketch.line(id) {
            strip::Run::Line { from: p, to: q }
        } else if let Some(arc) = sketch.arc(id) {
            strip::Run::Arc { center: arc.center, radius: arc.radius, from: arc.start, sweep: arc.sweep() }
        } else {
            return Err(format!("the profile's e{} cannot be read", id.0));
        };
        if run.length() < 1e-7 {
            return Err("A curve of the profile has no length.".into());
        }
        runs.push(if forward { run } else { run.reversed() });
        ids.push(id);
    }
    if !left.is_empty() {
        return Err("The profile is in more than one piece: a contour flange follows one run of lines and arcs joined end to end.".into());
    }
    // Which way it first turns.
    let mut turn = 0.0;
    for (i, run) in runs.iter().enumerate() {
        if let strip::Run::Arc { sweep, .. } = run {
            turn = *sweep;
        } else if let Some(next) = runs.get(i + 1) {
            let (d0, d1) = (run.headings().1, next.headings().0);
            turn = d0.cross(d1);
        }
        if turn.abs() > 1e-9 {
            break;
        }
    }
    if turn < 0.0 {
        runs = runs.iter().rev().map(strip::Run::reversed).collect();
        ids.reverse();
    }
    Ok((runs, ids))
}

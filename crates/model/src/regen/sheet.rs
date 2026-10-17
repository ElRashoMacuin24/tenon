//! Regenerating sheet metal features: faces and flanges (see `crate::sheet`).

use tenon_geom::{Axis, Frame, Vec2, Vec3};
use tenon_kernel::{
    BoolOp, CurveKind, Extent, Op, Origin, PrimitiveRole, Profile, Region, ShapeHandle, SubShape, SurfaceKind, TopoKind, Topology, Transform,
};
use tenon_sketch::Sketch;

use super::{Ctx, Stop, label, profile_of};
use crate::FeatureId;
use crate::naming::{FaceOrigin, edge_key, names_of_boolean, names_of_sweep, pair_key, resolve_edge, sub_key};
use crate::sheet::flange::{self, Notch, Part, Site};
use crate::sheet::join;
use crate::sheet::outline::{self, Cutter};
use crate::sheet::region;
use crate::sheet::strip;
use crate::sheet::unfold::Unfold;
use crate::sheet::wrap::{self, Tool};
use crate::sheet::{
    BendBetween, ContourFlange, CornerRip, CornerSeam, CutAcrossBend, CutNormal, Face, Flange, Fold, FoldLocation, Hem, Seam, SharpBend, Sides,
};

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
    /// sheet: the angle between the two edges there, and which of the two it is.
    meets: [Option<flange::Meet>; 2],
    /// The ends where it comes up to the flange of an edge on another face of the sheet, and
    /// is mitred back from the plane between the two faces.
    rims: [Option<flange::Rim>; 2],
}

/// Where two walls of one feature meet at a corner of the sheet: what the corner's relief takes
/// out of the sheet, seen from above the first wall (out of the sheet, along its edge), and the
/// corner's key.
struct Corner {
    wall: usize,
    cutter: Cutter,
    key: u64,
}

/// A sharp corner between two faces of the sheet, seen along its edge: the first face runs
/// off to the right, with its inner side (the side the corner closes on) up.
struct Sharp {
    /// The outside of the corner, at the end of its edge it is seen from.
    origin: Vec3,
    /// Along the edge, away from the eye.
    axis: Vec3,
    length: f64,
    /// The way each face runs from the corner, and the way through it to its inner side.
    d: [Vec3; 2],
    inward: [Vec3; 2],
    /// How far the sheet turns at the corner (radians).
    turn: f64,
    /// How far from the outside of the corner the bend begins, on the outer side of either face.
    outer_back: f64,
    /// The key of the edge, for the names of what the bend makes.
    key: u64,
}

impl Sharp {
    /// The way the second face runs, as it is seen.
    fn across(&self) -> Vec2 {
        Vec2::new(self.d[1].dot(self.d[0]), self.d[1].dot(self.inward[0]))
    }
    /// The way through the second face to its inner side, as it is seen.
    fn up(&self) -> Vec2 {
        Vec2::new(self.inward[1].dot(self.d[0]), self.inward[1].dot(self.inward[0]))
    }
}

/// One of the two faces a corner seam brings together, as the solid has it: the face, the bend
/// along its foot and the flat that comes up from.
#[derive(Clone, Copy, Debug)]
struct Flap {
    /// Where its bend starts on the flat (a point of the bend line, then where the two bend lines
    /// cross): out of the sheet across the line, up to the side the face is bent to, and along
    /// the line. Its length is the face's height.
    site: Site,
    shape: flange::Shape,
    /// Along the bend into the flange, away from the end that was picked.
    into: Vec3,
    /// How far along that the face and its bend now begin (from a point on the bend's axis, then
    /// from the crossing): less than nothing when they run on past it.
    wall_from: f64,
    bend_from: f64,
    /// The end that was picked is square to the bend.
    square: bool,
}

impl Ctx<'_> {
    pub(super) fn sheet_face(&mut self, id: FeatureId, s: &Face) -> Result<(), Stop> {
        s.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let (held, frame) = self.sketch_and_frame(s.sketch)?;
        let sketch: &Sketch = &held;
        let profile = profile_of(sketch, frame, &s.regions)?;
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

    /// Takes `tools` out of `target` one at a time, and sees each time that what went is what the
    /// tool shared with the part: a cut that took anything else is refused. (Tools that lie
    /// against one another, as the pieces of a relief do along a bend line, are not safe to take
    /// out in one boolean: the kernel then gives a sound solid of the wrong volume on some
    /// platforms, DEC-067.) The tools are used up; `target` is released when it is not the part's
    /// own body (`keep`).
    fn sheet_cut(&mut self, target: Named, keep: bool, tools: Vec<Named>, what: &str) -> Result<Named, Stop> {
        let volume = |ctx: &Self, s: ShapeHandle| ctx.k.mass_properties(s, 1.0).map(|m| m.volume);
        let (mut now, mut own) = (target, keep);
        let mut pending = tools.into_iter();
        let result = (|| -> Result<(), Stop> {
            for tool in pending.by_ref() {
                let before = volume(self, now.0);
                // What the tool can take: what it shares with the part, and no more than itself.
                let most = volume(self, tool.0);
                let shared = self.k.boolean(BoolOp::Intersect, now.0, &[tool.0]).ok().and_then(|op| {
                    let v = volume(self, op.shape);
                    self.k.release(op.shape);
                    v.ok()
                });
                now = self.sheet_step(BoolOp::Cut, now.clone(), own, vec![tool])?;
                own = false;
                let (Ok(before), Ok(most)) = (before, most) else { continue };
                let took = before - volume(self, now.0)?;
                // (To a millionth of the part, and a hundred thousandth of the tool for one with
                // walls of free curves, as a cut seen at an angle has: what this looks for is a
                // tool that was not taken, or took something else.)
                let slack = 1e-6 * (1.0 + before) + 1e-5 * most;
                let (wrong, due) = match shared {
                    Some(shared) => ((took - shared).abs() > slack, shared),
                    None => (took < -slack || took > most + slack, most),
                };
                if wrong {
                    return Err(format!(
                        "The {what} could not be cut out of the part: taking a piece of it out removed {} mm³ where {} mm³ was to go. Change its size or its place a little.",
                        crate::explain::mm(took),
                        crate::explain::mm(due)
                    )
                    .into());
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(now),
            Err(e) => {
                for (s, _) in pending {
                    self.k.release(s);
                }
                if !own {
                    self.k.release(now.0);
                }
                Err(e)
            }
        }
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
        let corner_rule = fl.corner.corners(&rule, &sizes, fl.miter);

        let (bi, body, found) = self.sheet_edges(&fl.edges, "flange")?;
        let topo = self.k.topology(body)?;
        let mut walls: Vec<Wall> = Vec::with_capacity(found.len());
        for (e, key) in &found {
            let site = self.flange_site(body, &topo, *e, t, fl.flip)?;
            let span = flange::span(&fl.width, site.length)?;
            walls.push(Wall { site, key: *key, span, meets: [None, None], rims: [None, None] });
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
                    for (w, e, first) in [(i, ea, true), (j, eb, false)] {
                        let wall = &mut walls[w];
                        if e == 0 {
                            wall.span.0 = wall.span.0.max(back);
                        } else {
                            wall.span.1 = wall.span.1.min(wall.site.length - back);
                        }
                        wall.meets[e] = Some(flange::Meet { corner, first });
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
        // Where two of the walls are on different faces of the sheet, whose planes cross along a
        // line both edges are square to, and both set off along that line from one level (the
        // rims of two walls of a box, each turned in over it), their flanges would share the
        // corner between them: each is mitred back from the plane that halves the two faces, as
        // far up it as it comes near, so that their inner sides are the gap apart.
        if let Some(joint) = corner_rule.joint {
            let far = dims.allowance(rho) + dims.straight;
            for i in 0..walls.len() {
                for j in i + 1..walls.len() {
                    let (a, b) = (walls[i].site, walls[j].site);
                    let line = a.up.cross(b.up);
                    let square = |s: &Site| s.along.dot(line).abs() < 1e-6 * line.len();
                    if line.len() < 1e-6
                        || !square(&a)
                        || !square(&b)
                        || a.out.dot(b.out) < 0.999
                        || (a.origin - b.origin).dot(a.out).abs() > 1e-6 * (1.0 + t)
                    {
                        continue;
                    }
                    // The end of a wall that is towards the other face, when the wall is all on
                    // the side of that face it looks to: and where that face's plane crosses
                    // the wall's edge.
                    let towards = |w: &Wall, other: &Site| -> Option<(usize, f64)> {
                        let across = w.site.along.dot(other.up);
                        let at = (other.origin - w.site.origin).dot(other.up) / across;
                        if across > 0.0 && at <= w.span.0 + 1e-9 {
                            Some((0, at))
                        } else if across < 0.0 && at >= w.span.1 - 1e-9 {
                            Some((1, at))
                        } else {
                            None
                        }
                    };
                    let (Some((ea, at_a)), Some((eb, at_b))) = (towards(&walls[i], &b), towards(&walls[j], &a)) else { continue };
                    let turn = a.up.dot(b.up).clamp(-1.0, 1.0).acos();
                    for (w, e, at) in [(i, ea, at_a), (j, eb, at_b)] {
                        let rim = flange::Rim::new(at, turn, joint.gap);
                        let wall = &mut walls[w];
                        // Only where the mitre takes anything of the flange at all, and of two
                        // that would, the one that takes more.
                        let takes = |r: &flange::Rim| {
                            let reach = dims.rim_reach(rho, r, far);
                            if e == 0 { r.at + reach - wall.span.0 } else { wall.span.1 - (r.at - reach) }
                        };
                        if takes(&rim) > 1e-9 && wall.meets[e].is_none() && wall.rims[e].is_none_or(|r| takes(&r) < takes(&rim)) {
                            wall.rims[e] = Some(rim);
                        }
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
                if w.meets != [None, None] || w.rims != [None, None] {
                    // A flange that meets another: its bend and its wall each from its outline in
                    // the flat, with the corner's relief out of both.
                    // (At a rim the end of the bend is a curve, given piece by piece.)
                    let pieces = flange::pieces_at_rims(&dims, rho, w.span, w.site.length, w.meets, w.rims, &corner_rule, name)?;
                    let frame = w.site.bend_frame(&dims).ok_or("the flange's edge has no direction")?;
                    let mitred = |tag: u64| tag == name(Part::MiterStart) || tag == name(Part::MiterEnd);
                    let outer = if w.rims == [None, None] { outline::to_loop(&pieces.bend) } else { region::to_loop_smoothing(&pieces.bend, mitred) };
                    let bend = Profile { frame, regions: vec![Region { outer, holes: vec![] }] };
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
            Vec::new(),
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
    /// each be joined to it: `advice` says what to do about each when they do not. Everything
    /// given is used up or released.
    fn sheet_assemble(
        &mut self,
        bi: usize,
        body: ShapeHandle,
        cuts: Vec<Named>,
        pieces: Vec<Named>,
        after: Vec<Named>,
        what: &str,
        advice: [&str; 2],
    ) -> Result<(), Stop> {
        self.sheet_assemble_more(bi, body, cuts, pieces, after, 0, what, advice)
    }

    /// As [`Self::sheet_assemble`], for pieces that bring `more` separate solids of their own
    /// (another piece of sheet that the rest joins to this one): the body may come out in that
    /// many more pieces than it was, and no more.
    #[allow(clippy::too_many_arguments)]
    fn sheet_assemble_more(
        &mut self,
        bi: usize,
        body: ShapeHandle,
        cuts: Vec<Named>,
        pieces: Vec<Named>,
        after: Vec<Named>,
        more: u32,
        what: &str,
        advice: [&str; 2],
    ) -> Result<(), Stop> {
        // (A sheet may be in several pieces: faces made apart, before a bend joins them.)
        let were = self.k.topology(body).map_or(1, |t| t.solids.max(1));
        let volume = |ctx: &Self, s: ShapeHandle| ctx.k.mass_properties(s, 1.0).map(|m| m.volume);
        let added: f64 = pieces.iter().map(|p| volume(self, p.0).unwrap_or(0.0)).sum();
        let Some(names) = self.regen.bodies.get(bi).map(|b| b.names.clone()) else {
            for (s, _) in cuts.into_iter().chain(pieces).chain(after) {
                self.k.release(s);
            }
            return Err("no such body".into());
        };
        let mut now: Named = (body, names);
        let mut own = true;
        // What is taken out first, what is joined on, and what is taken out of the whole.
        let steps: [(BoolOp, Vec<Named>); 3] = [(BoolOp::Cut, cuts), (BoolOp::Union, pieces), (BoolOp::Cut, after)];
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
                now = match kind {
                    BoolOp::Cut => self.sheet_cut(now.clone(), own, tools, what)?,
                    _ => self.sheet_step(kind, now.clone(), own, tools)?,
                };
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
                    // More than went in is no join at all.
                    if lost < -1e-6 * (1.0 + after) {
                        return Err(format!(
                            "The {what} could not be joined to the part: the whole came out {} mm³ more than its pieces. Change its size or its place a little.",
                            crate::explain::mm(-lost)
                        )
                        .into());
                    }
                    if self.k.topology(now.0)?.solids > were + more {
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

    /// Makes the faces feature `id` added one with the faces of the part they lie in the plane of
    /// (a wall, and the piece a corner seam put on its end). Such a face keeps the name the part
    /// had for it. Left as it is when the kernel cannot, or gives anything but the same solid.
    fn sheet_unify(&mut self, bi: usize, id: FeatureId) {
        let Some((shape, names)) = self.regen.bodies.get(bi).map(|b| (b.shape, b.names.clone())) else { return };
        let own = |n: &Option<FaceOrigin>| matches!(n, Some(FaceOrigin::From { feature, .. }) if *feature == id);
        let faces: Vec<tenon_kernel::FaceId> =
            names.iter().enumerate().filter(|(_, n)| own(n)).filter_map(|(i, _)| u32::try_from(i).ok()).map(|i| shape.face(i)).collect();
        if faces.is_empty() {
            return;
        }
        let Ok(op) = self.k.unify(shape, &faces) else { return };
        // The same solid with fewer faces, or it is not taken.
        let volume = |ctx: &Self, s: ShapeHandle| ctx.k.mass_properties(s, 1.0).map(|m| m.volume);
        let same = matches!((volume(self, shape), volume(self, op.shape)), (Ok(a), Ok(b)) if (a - b).abs() <= 1e-6 * (1.0 + a));
        let Some(topo) = self.k.topology(op.shape).ok().filter(|t| same && t.solids == 1) else {
            self.k.release(op.shape);
            return;
        };
        let mut joined: Vec<Option<FaceOrigin>> = vec![None; topo.faces as usize];
        for img in op.history.images.iter().filter(|i| i.source.id.kind == TopoKind::Face) {
            let name = names.get(img.source.id.index as usize).copied().flatten();
            for r in img.result.iter().filter(|r| r.kind == TopoKind::Face) {
                if let Some(slot) = joined.get_mut(r.index as usize)
                    && (slot.is_none() || (own(slot) && !own(&name)))
                    && name.is_some()
                {
                    *slot = name;
                }
            }
        }
        self.k.release(shape);
        if let Some(b) = self.regen.bodies.get_mut(bi) {
            (b.shape, b.names) = (op.shape, joined);
        }
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
            Vec::new(),
            "hem",
            [
                "Give it a larger gap or radius, make it shorter, or fold it to the other side.",
                "the edge picked for it is not on the rim of the sheet.",
            ],
        )
    }

    /// What the solid says of the face whose end edge `e` is: the bends along its foot, each with
    /// the flat it comes up from.
    fn flaps(&self, body: ShapeHandle, topo: &Topology, faces: &[tenon_kernel::FaceInfo], e: u32, t: f64) -> Result<Vec<Flap>, Stop> {
        let end = self.flange_site(body, topo, e, t, false).map_err(|stop| match stop {
            Stop::Failed(_) => Stop::Failed(
                "A corner seam is between the ends of two faces of the sheet: pick, on each, the straight edge at its end where the two come together."
                    .to_owned(),
            ),
            other => other,
        })?;
        // The face itself: of the two at the edge, the one that looks the way the site does.
        let skin = topo
            .edge_faces
            .get(e as usize)
            .into_iter()
            .flatten()
            .copied()
            .find(|f| matches!(faces.get(*f as usize).map(|i| &i.surface), Some(SurfaceKind::Plane { normal, .. }) if normal.normalized().dot(end.up) > 0.999))
            .ok_or("the face at an edge of the corner seam could not be read")?;
        let edges_of = |f: u32| topo.face_edges.get(f as usize).map_or(&[][..], Vec::as_slice);
        let across = |edge: u32, from: u32| topo.edge_faces.get(edge as usize).into_iter().flatten().copied().find(|f| *f != from);
        let mut found = Vec::new();
        for &g in edges_of(skin) {
            if g == e {
                continue;
            }
            let joint = self.k.edge_info(body.edge(g))?;
            // A bend runs along the face, square to its end.
            if !matches!(joint.curve, CurveKind::Line { .. }) || (joint.end - joint.start).normalized().cross(end.out).len() > 1e-6 {
                continue;
            }
            let Some(c) = across(g, skin) else { continue };
            let Some(SurfaceKind::Cylinder { axis, radius }) = faces.get(c as usize).map(|i| &i.surface) else { continue };
            let Some(inner) = crate::sheet::unfold::bend_side(c as usize, faces, t) else { continue };
            if axis.dir().cross(end.out).len() > 1e-6 {
                continue;
            }
            let radial = |p: Vec3| {
                let d = p - axis.origin();
                d - axis.dir() * d.dot(axis.dir())
            };
            // The flat the bend comes up from: across the bend's other straight edge.
            let mut floor = None;
            let mut reach = (f64::INFINITY, f64::NEG_INFINITY);
            for &k in edges_of(c) {
                let info = self.k.edge_info(body.edge(k))?;
                for p in [info.start, info.end] {
                    let s = (p - axis.origin()).dot(axis.dir());
                    reach = (reach.0.min(s), reach.1.max(s));
                }
                if k == g || !matches!(info.curve, CurveKind::Line { .. }) || (info.end - info.start).normalized().cross(axis.dir()).len() > 1e-6 {
                    continue;
                }
                if let Some(f) = across(k, c)
                    && matches!(faces.get(f as usize).map(|i| &i.surface), Some(SurfaceKind::Plane { .. }))
                {
                    floor = Some(radial(info.mid));
                }
            }
            let Some(to_floor) = floor.filter(|v| v.len() > 1e-9) else { continue };
            let (down, to_wall) = (to_floor.normalized(), radial(joint.mid).normalized());
            // The way the bend turns from the flat: out of the sheet, across its bend line.
            let side = to_wall - down * to_wall.dot(down);
            if side.len() < 1e-6 {
                // Folded right back (a hem) or not at all: no corner to close.
                continue;
            }
            let out = side.normalized();
            let r = if inner { *radius } else { radius - t };
            // Along the bend into the flange, away from the end that was picked.
            let into = if axis.dir().dot(end.out) < 0.0 { axis.dir() } else { -axis.dir() };
            let begins = |s: f64| s * axis.dir().dot(into);
            found.push(Flap {
                // (The crossing of the two bend lines is put in once both flaps are known.)
                site: Site { origin: axis.origin() + down * r, out, up: -down, along: out.cross(-down), length: end.length },
                shape: flange::Shape { t, r, angle: to_wall.dot(out).atan2(to_wall.dot(down)), setback: 0.0, straight: end.length },
                into,
                wall_from: (end.origin - axis.origin()).dot(into),
                bend_from: begins(reach.0).min(begins(reach.1)),
                square: end.along.dot(axis.dir()).abs() < 1e-6,
            });
        }
        Ok(found)
    }

    pub(super) fn corner_seam(&mut self, id: FeatureId, cs: &CornerSeam) -> Result<(), Stop> {
        cs.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let rule = self.doc.sheet_metal().cloned().unwrap_or_default();
        let t = sizes.thickness;
        let (bi, body, found) = self.sheet_edges(&cs.edges, "corner seam")?;
        let &[(e1, key1), (e2, key2)] = found.as_slice() else { return Err("a corner seam takes two edges".into()) };
        let topo = self.k.topology(body)?;
        let faces: Vec<tenon_kernel::FaceInfo> = (0..topo.faces).map(|i| self.k.face_info(body.face(i))).collect::<Result<_, _>>()?;
        let (first, second) = (self.flaps(body, &topo, &faces, e1, t)?, self.flaps(body, &topo, &faces, e2, t)?);
        if first.is_empty() || second.is_empty() {
            return Err(
                "A corner seam closes the corner between two faces that are bent up from the sheet, and one of the faces picked has no bend along its foot: pick the edges at the ends of two flanges."
                    .into(),
            );
        }
        // The two that come up from one flat.
        let floor = |f: &Flap| f.site.origin;
        let pair = first
            .iter()
            .flat_map(|a| second.iter().map(move |b| (a, b)))
            .find(|(a, b)| a.site.up.dot(b.site.up) > 0.999 && (floor(a) - floor(b)).dot(a.site.up).abs() < 1e-6 * (1.0 + t));
        let Some((a, b)) = pair else {
            return Err("The two faces of the corner seam are not bent up from the same flat of the sheet.".into());
        };
        let (mut a, mut b) = (*a, *b);
        if a.into.dot(b.into).abs() > 1e-6 {
            return Err(
                "The two faces meet at a corner that is not square, and a corner seam closes square corners. (Edges of one flange close their own corners, at any angle.)"
                    .into(),
            );
        }
        if a.site.out.dot(b.into) > -0.999 || b.site.out.dot(a.into) > -0.999 {
            return Err(
                "The two faces do not stand at an outside corner of the sheet: a corner seam closes the corner where two flanges come together from its two sides.".into(),
            );
        }
        if !(a.square && b.square) {
            return Err("An end picked for the corner seam is not square to its bend: pick the edges at the ends of two flanges.".into());
        }
        // Where the two bend lines cross: everything is measured from there.
        let (pa, pb) = (floor(&a), floor(&b));
        let crossing = pa + a.into * (pb - pa).dot(a.into);
        for f in [&mut a, &mut b] {
            let moved = (f.site.origin - crossing).dot(f.into);
            f.wall_from += moved;
            f.bend_from += moved;
            f.site.origin = crossing;
        }
        let corner = std::f64::consts::FRAC_PI_2;
        let corners = cs.corner.corners(&rule, &sizes, true);
        let Some(joint) = corners.joint else { return Err("a corner seam has a seam".into()) };
        let flaps = [a, b];
        let rho = |f: &Flap| f.shape.r + sizes.k_factor * t;
        let widths = flaps.map(|f| f.shape.allowance(rho(&f)));
        // Along the flange from the crossing is this way along its site.
        let way = |f: &Flap| f.site.along.dot(f.into).signum();
        let base = pair_key(key1, key2);

        let (mut cuts, mut pieces, mut reliefs): (Vec<Named>, Vec<Named>, Vec<Named>) = (Vec::new(), Vec::new(), Vec::new());
        let built = (|| -> Result<(), Stop> {
            for (i, f) in flaps.iter().enumerate() {
                let other = &flaps[1 - i];
                let name = |part: u32| sub_key(sub_key(base, u32::try_from(i).unwrap_or(0)), part);
                let high = f.shape.straight;
                // What of the flange runs on past the crossing goes: its bend, and its wall with it.
                let past = f.wall_from.min(f.bend_from);
                if past < -1e-7 {
                    let path = strip::walk(
                        Vec2::ZERO,
                        Vec2::X,
                        &[strip::Step::Bend { radius: f.shape.r, angle: f.shape.angle }, strip::Step::Straight(high + t)],
                    );
                    let region = strip::outline(&path, -t, 0.0, |_, _| name(Part::Cut as u32), [name(Part::Cut as u32); 2])?;
                    let frame = f.site.section_frame(0.0).ok_or("the corner seam's bend has no direction")?;
                    let reach = -(t - past) * way(f);
                    let caps = [name(Part::BendEnd as u32); 2];
                    let made = self.sheet_strip(id, &Profile { frame, regions: vec![region] }, &Extent::Distance(reach), caps)?;
                    cuts.push(made);
                }
                // The wall, to where the seam ends it: more of it where that is nearer the corner
                // than it now reaches, less where it is further.
                let now = f.wall_from.max(0.0);
                let to = |h: f64| flange::seam_end(&f.shape, &other.shape, corner, &joint, i == 0, h);
                let at = |h: f64, along: f64| Vec2::new(h, along * way(f));
                let (g0, g1) = (to(0.0) - now, to(high) - now);
                let frame = f.site.wall_frame(&f.shape).ok_or("the corner seam's face has no direction")?;
                let mut wall = |ctx: &mut Self, corners: [(f64, f64); 4], add: bool| -> Result<(), Stop> {
                    let tag = name(if add { Part::End as u32 } else { Part::Cut as u32 });
                    let outline = outline::polygon(&corners.map(|(h, along)| (at(h, along), tag)));
                    if outline.len() < 3 {
                        return Ok(());
                    }
                    let profile = Profile { frame, regions: vec![Region { outer: outline::to_loop(&outline), holes: vec![] }] };
                    let made = ctx.sheet_prism(id, &profile, t, Some([name(Part::Inner as u32), name(Part::Outer as u32)]))?;
                    if add {
                        pieces.push(made)
                    } else {
                        cuts.push(made)
                    }
                    Ok(())
                };
                if high > 1e-9 && (g0.abs() > 1e-9 || g1.abs() > 1e-9) {
                    if g0 * g1 >= 0.0 {
                        wall(self, [(0.0, to(0.0)), (0.0, now), (high, now), (high, to(high))], g0 + g1 < 0.0)?;
                    } else {
                        // The seam's end crosses the end the wall has: more on one side of where
                        // it does, less on the other.
                        let turn = high * g0 / (g0 - g1);
                        wall(self, [(0.0, to(0.0)), (0.0, now), (turn, now), (turn, now)], g0 < 0.0)?;
                        wall(self, [(turn, now), (turn, now), (high, now), (high, to(high))], g1 < 0.0)?;
                    }
                }
                // The relief, out of the bend and out of the wall as the seam leaves it.
                let Some(cutter) = flange::relief([widths[i], widths[1 - i]], corner, &corners) else { continue };
                let cutter = if way(f) < 0.0 { flange::at_end(&cutter, 1, 0.0) } else { cutter };
                let far = corners.size + widths[0] + widths[1] + 4.0 * t;
                let tag = name(Part::Corner as u32);
                let bend = outline::polygon(&[(at(0.0, 0.0), tag), (at(widths[i], 0.0), tag), (at(widths[i], far), tag), (at(0.0, far), tag)]);
                let taken = outline::keep(&bend, &cutter, tag)?;
                if !taken.is_empty() {
                    let frame = f.site.bend_frame(&f.shape).ok_or("the corner seam's bend has no direction")?;
                    let profile = Profile { frame, regions: vec![Region { outer: outline::to_loop(&taken), holes: vec![] }] };
                    let margin = f.shape.r.min(t) / 2.0;
                    let made = self.sheet_roll(id, &profile, rho(f), f.shape.r - margin, f.shape.r + t + margin, [tag, tag])?;
                    reliefs.push(made);
                }
                if high > 1e-9 {
                    let width = widths[i];
                    let foot = outline::polygon(&[
                        (at(width, to(0.0)), tag),
                        (at(width + high, to(high)), tag),
                        (at(width + high, far), tag),
                        (at(width, far), tag),
                    ]);
                    let taken: Vec<outline::Piece> = outline::keep(&foot, &cutter, tag)?.iter().map(|p| p.moved(Vec2::new(-width, 0.0))).collect();
                    if !taken.is_empty() {
                        let profile = Profile { frame, regions: vec![Region { outer: outline::to_loop(&taken), holes: vec![] }] };
                        let made = self.sheet_prism(id, &profile, t, Some([tag, tag]))?;
                        reliefs.push(made);
                    }
                }
                // Out of the flat, once: between the two bend lines.
                if i == 0 {
                    let flat = outline::polygon(&[(at(-far, 0.0), tag), (at(0.0, 0.0), tag), (at(0.0, far), tag), (at(-far, far), tag)]);
                    let taken = outline::keep(&flat, &cutter, tag)?;
                    if !taken.is_empty() {
                        let frame = f.site.plan_frame(t).ok_or("the corner seam's bend has no direction")?;
                        let made = self.sheet_prism(id, &flange::profile(frame, outline::to_loop(&taken)), 3.0 * t, None)?;
                        reliefs.push(made);
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = built {
            for (s, _) in cuts.into_iter().chain(pieces).chain(reliefs) {
                self.k.release(s);
            }
            return Err(e);
        }
        self.sheet_assemble(
            bi,
            body,
            cuts,
            pieces,
            reliefs,
            "corner seam",
            ["Give it a wider gap, or another kind of seam.", "the faces picked for it do not come together at one corner."],
        )?;
        // The piece put on the end of a wall is the wall: one face, with the wall's name.
        self.sheet_unify(bi, id);
        Ok(())
    }

    /// A bend between two faces of the sheet that do not touch (`crate::sheet::join`): each face
    /// is cut back where the strip between them begins short of its edge, and the strip, with
    /// its bends, is joined to both.
    pub(super) fn bend_between(&mut self, id: FeatureId, b: &BendBetween) -> Result<(), Stop> {
        b.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let t = sizes.thickness;
        let r = b.radius.unwrap_or(sizes.bend_radius);
        // The two edges, each on a piece of the sheet: one solid of the part, or two.
        let mut ends: Vec<(usize, u32, u64)> = Vec::with_capacity(2);
        {
            let views = self.regen.views();
            for e in &b.edges {
                let (body, edge) = resolve_edge(e, &views, &*self.k).map_err(|m| {
                    let [a, c] = &e.faces;
                    format!(
                        "{m} (the edge between the {} and the {})",
                        a.describe(&label(self.doc, a.feature())),
                        c.describe(&label(self.doc, c.feature()))
                    )
                })?;
                ends.push((body, edge, edge_key(&e.faces[0], &e.faces[1])));
            }
        }
        let &[(b1, e1, key1), (b2, e2, key2)] = ends.as_slice() else { return Err("a bend takes two edges".into()) };
        if (b1, e1) == (b2, e2) {
            return Err("An edge is picked twice for the bend: pick an edge of each of the two faces.".into());
        }
        let (body1, body2) = (self.body_shape(b1)?, self.body_shape(b2)?);
        let site = |ctx: &Self, body: ShapeHandle, e: u32| -> Result<Site, Stop> {
            let topo = ctx.k.topology(body)?;
            ctx.flange_site(body, &topo, e, t, false).map_err(|stop| match stop {
                Stop::Failed(_) => Stop::Failed(
                    "A bend joins two faces of the sheet along a straight edge of each, on the rim of the sheet: pick, on each face, the edge the bend is to start from."
                        .to_owned(),
                ),
                other => other,
            })
        };
        let (s1, s2) = (site(self, body1, e1)?, site(self, body2, e2)?);
        if s1.along.cross(s2.along).len() > 1e-6 {
            return Err("The two edges are not parallel: a bend joins two faces along edges that run alongside one another.".into());
        }
        // As much of the first edge as the second runs alongside.
        let along = |p: Vec3| (p - s1.origin).dot(s1.along);
        let (from, to) = (along(s2.origin), along(s2.at(0.0, 0.0, s2.length)));
        let span = (from.min(to).max(0.0), from.max(to).min(s1.length));
        let width = span.1 - span.0;
        if width < 1e-6 {
            return Err(
                "The two edges do not run alongside one another: seen along them, neither is beside the other. Pick edges that face each other."
                    .into(),
            );
        }
        // The second face in the cross-section of the first, and the strip from one to the other.
        let seen = |v: Vec3| Vec2::new(v.dot(s1.out), v.dot(s1.up));
        let other = join::Other::of(seen(s2.origin - s1.origin), seen(s2.out), seen(s2.up), t);
        let joined = join::join(&other, t, r, b.double, b.flip_fixed)?;
        let path = strip::walk(Vec2::new(-joined.trims[0], 0.0), Vec2::X, &joined.steps);
        if strip::crosses(&path, -t, 0.0) {
            return Err("The bend folds into itself between the two faces: use a smaller bend radius, or another kind of double bend.".into());
        }
        let name = |part: u32| sub_key(pair_key(key1, key2), part);
        let frame = s1.section_frame(span.0).ok_or("the bend's edge has no direction")?;
        // What each face loses to make room: the sheet from its edge back to where the strip
        // begins, as wide as the bend.
        let right = -other.into.perp();
        let lost = [
            [Vec2::new(-joined.trims[0], 0.0), Vec2::ZERO, Vec2::new(0.0, -t), Vec2::new(-joined.trims[0], -t)],
            [other.at, other.at + other.into * joined.trims[1], other.at + other.into * joined.trims[1] + right * t, other.at + right * t],
        ];
        let (mut pieces, mut cuts): (Vec<Named>, [Option<Named>; 2]) = (Vec::new(), [None, None]);
        let built = (|| -> Result<(), Stop> {
            let region = strip::outline(
                &path,
                -t,
                0.0,
                |i, left| name(32 + 2 * u32::try_from(i).unwrap_or(0) + u32::from(left)),
                [name(Part::Root as u32), name(Part::Tip as u32)],
            )?;
            let caps = [name(Part::Start as u32), name(Part::End as u32)];
            pieces.push(self.sheet_strip(id, &Profile { frame, regions: vec![region] }, &Extent::Distance(width), caps)?);
            for (i, corners) in lost.iter().enumerate() {
                if joined.trims[i] <= 1e-9 {
                    continue;
                }
                let tag = name(Part::Cut as u32 + 16 * u32::try_from(i).unwrap_or(0));
                let outline = outline::polygon(&corners.map(|c| (c, tag)));
                let profile = Profile { frame, regions: vec![Region { outer: outline::to_loop(&outline), holes: vec![] }] };
                cuts[i] = Some(self.sheet_prism(id, &profile, width, Some([tag, tag]))?);
            }
            Ok(())
        })();
        let [cut1, cut2] = cuts;
        if let Err(e) = built {
            for (s, _) in pieces.into_iter().chain(cut1).chain(cut2) {
                self.k.release(s);
            }
            return Err(e);
        }
        let advice = [
            "Use a smaller bend radius, or another kind of double bend.",
            "the strip between the two edges does not reach both faces: pick the edges that face each other.",
        ];
        // How many pieces the sheet is in before, to see that the bend made two of them one.
        let lumps = |ctx: &Self, s: ShapeHandle| ctx.k.topology(s).map_or(1, |t| t.solids.max(1));
        let before = lumps(self, body1) + if b1 == b2 { 0 } else { lumps(self, body2) };
        let bi = if b1 == b2 {
            self.sheet_assemble(b1, body1, cut1.into_iter().chain(cut2).collect(), pieces, Vec::new(), "bend", advice)?;
            b1
        } else {
            // The second face is on a solid of its own: that solid, less what the bend takes of
            // it, is joined on with the strip, and is then part of the first.
            let names2 = self.regen.bodies.get(b2).map(|b| b.names.clone()).unwrap_or_default();
            let second: Result<Named, Stop> = match cut2 {
                Some(tool) => self.sheet_cut((body2, names2), true, vec![tool], "bend"),
                None => self.k.transform(body2, &Transform::Translate(Vec3::ZERO)).map(|op| (op.shape, names2)).map_err(Stop::from),
            };
            let second = match second {
                Ok(s) => s,
                Err(e) => {
                    for (s, _) in pieces.into_iter().chain(cut1) {
                        self.k.release(s);
                    }
                    return Err(e);
                }
            };
            let more = lumps(self, second.0);
            pieces.push(second);
            self.sheet_assemble_more(b1, body1, cut1.into_iter().collect(), pieces, Vec::new(), more, "bend", advice)?;
            let gone = self.regen.bodies.remove(b2);
            self.k.release(gone.shape);
            if b2 < b1 { b1 - 1 } else { b1 }
        };
        let after = self.regen.bodies.get(bi).map_or(before, |b| lumps(self, b.shape));
        if after + 1 != before {
            // (The part is left as the bend made it: the feature fails, and regeneration stops.)
            return Err(format!("The bend does not join the two faces: {}", advice[1]).into());
        }
        // The piece put on the edge of a face is the face: one face, with the face's name.
        self.sheet_unify(bi, id);
        Ok(())
    }

    /// A face of the sheet folded along a line drawn across it: the bend's allowance of sheet is
    /// taken out at the line, which leaves the sheet in two; the piece on the side that folds is
    /// brought up to the cut and turned about the bend's axis, and the bend joins the two again.
    pub(super) fn fold(&mut self, id: FeatureId, f: &Fold) -> Result<(), Stop> {
        f.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let t = sizes.thickness;
        let r = f.radius.unwrap_or(sizes.bend_radius);
        let (held, frame) = self.sketch_and_frame(f.sketch)?;
        let sketch: &Sketch = &held;
        // The line: the one named, or the sketch's only line of its own.
        let line = match f.line {
            Some(l) => l,
            None => {
                let own: Vec<_> = sketch
                    .entities()
                    .filter(|(_, e)| !e.reference && !e.construction && matches!(e.geometry, tenon_sketch::Geometry::Line { .. }))
                    .map(|(id, _)| id)
                    .collect();
                match own.as_slice() {
                    [one] => *one,
                    [] => {
                        return Err("A fold is along a straight line drawn across a face of the sheet, and the sketch has no line of its own.".into());
                    }
                    _ => return Err("The sketch has more than one line: say which one the fold is along.".into()),
                }
            }
        };
        let (p, q) = sketch.line(line).ok_or("the fold's line is not a line of its sketch")?;
        let (p0, p1) = (frame.plane_point(p), frame.plane_point(q));
        let length = p0.dist(p1);
        if length < 1e-6 {
            return Err("The fold's line has no length.".into());
        }
        // The face the line is drawn on: the side of the sheet the sketch is on, under the line.
        let across = (q - p).normalized().perp() * 0.01;
        let sliver = outline::polygon(&[(p - across, 0), (q - across, 0), (q + across, 0), (p + across, 0)]);
        let under = [Region { outer: outline::to_loop(&sliver), holes: vec![] }];
        let Some((bi, base)) = crate::sheet::seat_of(self.doc, &self.regen, &*self.k, f.sketch, &frame, &under)? else {
            return Err("A fold is along a line drawn on the sheet: its sketch must be on a flat face of the sheet, and this one's plane has no face of the part in it.".into());
        };
        let body = self.body_shape(bi)?;
        let SurfaceKind::Plane { normal, .. } = self.k.face_info(body.face(base))?.surface else {
            return Err("the face under the fold's line is not flat".into());
        };
        let n = normal.normalized();
        let along = (p1 - p0).normalized();
        // The side that folds: to the left of the line as drawn, seen from outside the face.
        let side = n.cross(along) * if f.flip_side { -1.0 } else { 1.0 };
        // The bend's width of sheet in the flat, and where it starts across the line.
        let allowance = f.angle * (r + sizes.k_factor * t);
        let start = match f.location {
            FoldLocation::Centerline => -allowance / 2.0,
            FoldLocation::Start => 0.0,
            FoldLocation::End => -allowance,
        };
        let up = !f.flip_direction;
        let name = |part: u32| sub_key(u64::from(line.0), part);

        // The strip of sheet the bend takes: across the face from `start`, as long as the line.
        let zone_frame = Frame::new(p0, n, side).ok_or("the fold's line has no direction")?;
        let reach = (p1 - p0).dot(zone_frame.y());
        let tag = name(Part::Cut as u32);
        let corner = |x: f64, y: f64| (Vec2::new(x, y), tag);
        let zone = outline::polygon(&[
            corner(start, reach.min(0.0)),
            corner(start + allowance, reach.min(0.0)),
            corner(start + allowance, reach.max(0.0)),
            corner(start, reach.max(0.0)),
        ]);
        let zone = self.sheet_prism(
            id,
            &Profile { frame: zone_frame, regions: vec![Region { outer: outline::to_loop(&zone), holes: vec![] }] },
            -t,
            Some([tag, tag]),
        )?;
        let names = self.regen.bodies.get(bi).map(|b| b.names.clone()).unwrap_or_default();
        let lumps = self.k.topology(body)?.solids.max(1);
        let cut = self.sheet_cut((body, names), true, vec![zone], "fold")?;

        // Everything made on the way, to be released whatever happens.
        let mut temp: Vec<ShapeHandle> = vec![cut.0];
        let mut made = || -> Result<Named, Stop> {
            let topo = self.k.topology(cut.0)?;
            if topo.solids != lumps + 1 {
                return Err("The fold's line does not cut the face in two: draw it from edge to edge of the face it is on.".into());
            }
            // The piece that folds is the one just past the bend's strip of sheet.
            let middle = (p0 + p1) * 0.5 + side * (start + allowance + (0.01f64).min(t)) - n * (t / 2.0);
            let probe = self.k.make_sphere(middle, t * 0.001)?.shape;
            temp.push(probe);
            let (mut moving, mut staying): (Option<Named>, Vec<Named>) = (None, Vec::new());
            for i in 0..topo.solids {
                let s = self.k.solid(cut.0, i)?;
                temp.push(s.shape);
                let faces = self.k.topology(s.shape)?.faces;
                let named = (s.shape, names_of_boolean(&s.history, &[&cut.1], faces));
                if moving.is_none() && self.k.min_distance(SubShape::Shape(s.shape), SubShape::Shape(probe))?.value < 1e-6 {
                    moving = Some(named);
                } else {
                    staying.push(named);
                }
            }
            let (Some(moving), false) = (moving, staying.is_empty()) else {
                return Err("The fold's line does not leave sheet on both sides of it to fold: draw it across the face.".into());
            };
            // Brought up to the cut, then turned about the bend's axis: above the face for a
            // fold up, below the sheet for a fold down.
            let foot = p0 + side * start;
            let (centre, turn) = if up { (foot + n * r, side.cross(n)) } else { (foot - n * (t + r), n.cross(side)) };
            let axis = Axis::new(centre, turn).ok_or("the fold has no axis")?;
            let shifted = self.k.transform(moving.0, &Transform::Translate(-side * allowance))?.shape;
            temp.push(shifted);
            let turned = self.k.transform(shifted, &Transform::Rotate { axis, angle: f.angle })?.shape;
            temp.push(turned);
            // The bend: a strip from the edge of what stays, as a hem's is.
            let (out, lift) = (side, if up { n } else { -n });
            let run = out.cross(lift);
            let origin = (if run.dot(along) > 0.0 { p0 } else { p1 }) + side * start - if up { Vec3::ZERO } else { n * t };
            let site = Site { origin, out, up: lift, along: run, length };
            let path = strip::walk(Vec2::ZERO, Vec2::X, &[strip::Step::Bend { radius: r, angle: f.angle }]);
            let region = strip::outline(
                &path,
                -t,
                0.0,
                |i, left| name(32 + 2 * u32::try_from(i).unwrap_or(0) + u32::from(left)),
                [name(Part::Root as u32), name(Part::Tip as u32)],
            )?;
            let section = site.section_frame(0.0).ok_or("the fold's line has no direction")?;
            let bend = self.sheet_strip(
                id,
                &Profile { frame: section, regions: vec![region] },
                &Extent::Distance(length),
                [name(Part::Start as u32), name(Part::End as u32)],
            )?;
            temp.push(bend.0);
            // What stays, the bend and what folded, one solid again.
            let volume = |ctx: &Self, s: ShapeHandle| ctx.k.mass_properties(s, 1.0).map(|m| m.volume);
            let parts = volume(self, cut.0)? + volume(self, bend.0)?;
            let mut inputs: Vec<&[Option<FaceOrigin>]> = staying.iter().map(|s| s.1.as_slice()).collect();
            inputs.push(&bend.1);
            inputs.push(&moving.1);
            let tools: Vec<ShapeHandle> = staying.iter().skip(1).map(|s| s.0).chain([bend.0, turned]).collect();
            let joined = self.k.boolean(BoolOp::Union, staying[0].0, &tools)?;
            let topo = match self.k.topology(joined.shape) {
                Ok(topo) => topo,
                Err(e) => {
                    self.k.release(joined.shape);
                    return Err(e.into());
                }
            };
            let whole = volume(self, joined.shape).unwrap_or(f64::NAN);
            if (whole - parts).abs() > 1e-6 * (1.0 + parts) || topo.solids != lumps {
                self.k.release(joined.shape);
                return Err(if topo.solids != lumps {
                    "The fold does not hold together: what folds is not joined to the rest by the bend. Draw the line from edge to edge of the face."
                        .into()
                } else {
                    format!(
                        "The fold runs into the part: {} mm³ of what folds would be where the part already is. Fold it less, or the other way.",
                        crate::explain::mm((parts - whole).abs())
                    )
                    .into()
                });
            }
            Ok((joined.shape, names_of_boolean(&joined.history, &inputs, topo.faces)))
        };
        let result = made();
        for s in temp {
            self.k.release(s);
        }
        let (shape, names) = result?;
        self.k.release(body);
        if let Some(b) = self.regen.bodies.get_mut(bi) {
            (b.shape, b.names) = (shape, names);
        }
        self.sheet_unify(bi, id);
        Ok(())
    }

    /// A bend on sharp edges: each edge given is the outside or the inside edge of a corner
    /// where two faces of the sheet meet with no bend between them. Every corner is first
    /// cleared back to where its bend begins on either face, for the length of its edge;
    /// then each gets its bend, tangent to both faces, as far along the edge as both faces
    /// still reach it. So edges that meet at a corner of the sheet, bent in one feature, have
    /// their bends end where their bend lines cross, and the walls hang on past them.
    pub(super) fn sharp_bend(&mut self, id: FeatureId, b: &SharpBend) -> Result<(), Stop> {
        b.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let t = sizes.thickness;
        let r = b.radius.unwrap_or(sizes.bend_radius);
        let (bi, body, found) = self.sheet_edges(&b.edges, "bend")?;
        let topo = self.k.topology(body)?;
        let faces: Vec<tenon_kernel::FaceInfo> = (0..topo.faces).map(|i| self.k.face_info(body.face(i))).collect::<Result<_, _>>()?;
        // A flat face of the sheet with its other side one thickness behind it: its normal.
        let side = |f: u32| -> Option<Vec3> {
            let face = faces.get(f as usize)?;
            let SurfaceKind::Plane { normal, .. } = &face.surface else { return None };
            let n = normal.normalized();
            faces
                .iter()
                .any(|g| matches!(&g.surface, SurfaceKind::Plane { normal: m, .. } if m.normalized().dot(n) < -1.0 + 1e-9 && ((g.centroid - face.centroid).dot(n) + t).abs() < 1e-6 * (1.0 + t)))
                .then_some(n)
        };

        // Each corner, seen along its edge.
        let mut corners: Vec<Sharp> = Vec::with_capacity(found.len());
        for (e, key) in &found {
            let info = self.k.edge_info(body.edge(*e))?;
            if !matches!(info.curve, CurveKind::Line { .. }) || info.length < 1e-6 {
                return Err("A bend goes on a straight edge, and an edge picked for it is curved.".into());
            }
            let &[fa, fb] = topo.edge_faces.get(*e as usize).map_or(&[][..], Vec::as_slice) else {
                return Err("A bend goes on an edge between two faces of the sheet.".into());
            };
            let (Some(na), Some(nb)) = (side(fa), side(fb)) else {
                return Err(
                    "A bend on one edge goes on a sharp edge where two faces of the sheet meet, and an edge picked is not one: the faces either side of it are not both flat sides of the sheet. (An edge of the sheet's rim takes a flange, or a bend to an edge of another face; a corner with a bend round it is bent already.)"
                        .into(),
                );
            };
            let along = (info.end - info.start).normalized();
            if na.cross(nb).len() < 1e-6 {
                return Err("The two faces at an edge picked lie in one plane: there is no corner there to bend.".into());
            }
            // The outside edge of the corner, or the inside one: beside the inside edge there
            // is sheet a little way up either face, beside the outside edge none.
            let middle = (info.start + info.end) * 0.5;
            let mut inside = [false; 2];
            for (slot, way) in inside.iter_mut().zip([na - nb, nb - na]) {
                let probe = self.k.make_sphere(middle + way * (t / 2.0), t * 0.001)?.shape;
                let near = self.k.min_distance(SubShape::Shape(body), SubShape::Shape(probe)).map(|d| d.value < 1e-6);
                self.k.release(probe);
                *slot = near?;
            }
            let outside = match inside {
                [false, false] => true,
                [true, true] => false,
                _ => {
                    return Err(
                        "The faces at an edge picked are too short to tell which side of the corner it is on: a bend needs faces longer than they are thick.".into()
                    );
                }
            };
            // Each face runs away from the corner in its own plane, and has its inner side
            // (the side the corner closes on) to one side of it.
            let away = |n: Vec3, other: Vec3| {
                let d = n.cross(along).normalized();
                if (d.dot(other) < 0.0) == outside { d } else { -d }
            };
            let d = [away(na, nb), away(nb, na)];
            let inward = if outside { [-na, -nb] } else { [na, nb] };
            let turn = (-d[0].dot(d[1])).clamp(-1.0, 1.0).acos();
            if !(1f64.to_radians()..=179f64.to_radians()).contains(&turn) {
                return Err(
                    "The two faces at an edge picked meet at too sharp or too flat an angle to bend: a bend turns the sheet by 1 to 179 degrees."
                        .into(),
                );
            }
            let outer_back = (r + t) * (turn / 2.0).tan();
            // The outside of the corner, where the edge starts.
            let start = if outside { info.start } else { info.start - (d[0] + d[1]) * (t / d[0].cross(d[1]).len()) };
            // Seen with the first face running off to the right and its inner side up.
            let axis = d[0].cross(inward[0]);
            let origin = if axis.dot(along) > 0.0 { start } else { start + along * info.length };
            if corners.iter().any(|c| c.axis.cross(axis).len() < 1e-9 && (c.origin - origin).cross(axis).len() < 1e-6 * (1.0 + t)) {
                return Err("A corner is picked twice for the bend: its outside edge and its inside edge are the one corner.".into());
            }
            corners.push(Sharp { origin, axis, length: info.length, d, inward, turn, outer_back, key: *key });
        }

        // What each corner loses: all of it back to where the bend begins on either face.
        let mut zones: Vec<Named> = Vec::with_capacity(corners.len());
        let cleared = (|| -> Result<(), Stop> {
            for c in &corners {
                let tag = sub_key(c.key, Part::Cut as u32);
                let (across, up) = (c.across(), c.up());
                let at = |p: Vec2| (p, tag);
                let outline = outline::polygon(&[
                    at(Vec2::ZERO),
                    at(Vec2::new(c.outer_back, 0.0)),
                    at(Vec2::new(c.outer_back, t)),
                    at(across * c.outer_back + up * t),
                    at(across * c.outer_back),
                ]);
                let frame = Frame::new(c.origin, c.axis, c.d[0]).ok_or("the corner's edge has no direction")?;
                let profile = Profile { frame, regions: vec![Region { outer: outline::to_loop(&outline), holes: vec![] }] };
                zones.push(self.sheet_prism(id, &profile, c.length, Some([tag, tag]))?);
            }
            Ok(())
        })();
        if let Err(e) = cleared {
            for (s, _) in zones {
                self.k.release(s);
            }
            return Err(e);
        }
        let names = self.regen.bodies.get(bi).map(|b| b.names.clone()).unwrap_or_default();
        let lumps = topo.solids.max(1);
        let mut now = self.sheet_cut((body, names), true, zones, "bend")?;

        // The bends, each as long as both of its faces reach it, joined on one at a time.
        let mut pieces: Vec<Named> = Vec::new();
        let joined = (|| -> Result<(), Stop> {
            let cut = self.k.topology(now.0)?;
            let ends: Vec<tenon_kernel::FaceInfo> = (0..cut.faces).map(|i| self.k.face_info(now.0.face(i))).collect::<Result<_, _>>()?;
            // The ends of the bends, which must stay free: their names and their size.
            let mut caps: Vec<(u64, f64)> = Vec::new();
            for c in &corners {
                // Where face `k` now ends at the bend: the stretches of the edge it reaches.
                let reach = |k: usize| -> Result<Vec<(f64, f64)>, Stop> {
                    let mut spans: Vec<(f64, f64)> = Vec::new();
                    let at_end = c.origin + c.d[k] * c.outer_back;
                    for (f, face) in ends.iter().enumerate() {
                        let SurfaceKind::Plane { normal, .. } = &face.surface else { continue };
                        if normal.normalized().dot(c.d[k]) > -1.0 + 1e-9 || (face.centroid - at_end).dot(c.d[k]).abs() > 1e-6 * (1.0 + t) {
                            continue;
                        }
                        let (mut through, mut run) = ((f64::INFINITY, f64::NEG_INFINITY), (f64::INFINITY, f64::NEG_INFINITY));
                        for &e in cut.face_edges.get(f).map_or(&[][..], Vec::as_slice) {
                            let info = self.k.edge_info(now.0.edge(e))?;
                            for p in [info.start, info.mid, info.end] {
                                let (deep, far) = ((p - at_end).dot(c.inward[k]), (p - c.origin).dot(c.axis));
                                through = (through.0.min(deep), through.1.max(deep));
                                run = (run.0.min(far), run.1.max(far));
                            }
                        }
                        // The face's whole thickness, within the corner's own length.
                        let slack = 1e-6 * (1.0 + t);
                        if through.0.abs() <= slack
                            && (through.1 - t).abs() <= slack
                            && run.0 >= -slack
                            && run.1 <= c.length + slack
                            && run.1 - run.0 > slack
                        {
                            spans.push(run);
                        }
                    }
                    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
                    let mut merged: Vec<(f64, f64)> = Vec::new();
                    for s in spans {
                        match merged.last_mut() {
                            Some(last) if s.0 <= last.1 + 1e-7 => last.1 = last.1.max(s.1),
                            _ => merged.push(s),
                        }
                    }
                    Ok(merged)
                };
                let (first, second) = (reach(0)?, reach(1)?);
                let both: Vec<(f64, f64)> =
                    first.iter().flat_map(|a| second.iter().map(move |b| (a.0.max(b.0), a.1.min(b.1)))).filter(|s| s.1 - s.0 > 1e-6).collect();
                if both.is_empty() {
                    return Err(
                        "A bend could not be put on an edge: once the corner is cleared for it, the two faces do not both reach the bend anywhere along the edge. A face may be shorter than the bend takes of it: use a smaller bend radius."
                            .into(),
                    );
                }
                for (j, (from, to)) in both.iter().enumerate() {
                    let base = if j == 0 { c.key } else { sub_key(c.key, 64 + u32::try_from(j).unwrap_or(u32::MAX - 64)) };
                    let name = |part: Part| sub_key(base, part as u32);
                    // From where the bend begins on the first face's inner side, towards the
                    // corner and round to the right, with the sheet to the left of the way.
                    let path = strip::walk(Vec2::new(c.outer_back, t), -Vec2::X, &[strip::Step::Bend { radius: r, angle: -c.turn }]);
                    let region = strip::outline(
                        &path,
                        0.0,
                        t,
                        |_, left| name(if left { Part::BendOuter } else { Part::BendInner }),
                        [name(Part::Root), name(Part::Tip)],
                    )?;
                    let frame = Frame::new(c.origin + c.axis * *from, c.axis, c.d[0]).ok_or("the corner's edge has no direction")?;
                    let caps_of = [name(Part::Start), name(Part::End)];
                    pieces.push(self.sheet_strip(id, &Profile { frame, regions: vec![region] }, &Extent::Distance(to - from), caps_of)?);
                    let size = c.turn / 2.0 * ((r + t) * (r + t) - r * r);
                    caps.extend(caps_of.map(|cap| (cap, size)));
                }
            }
            let volume = |ctx: &Self, s: ShapeHandle| ctx.k.mass_properties(s, 1.0).map(|m| m.volume);
            while let Some(piece) = pieces.pop() {
                let step = (|| -> Result<Named, Stop> {
                    let parts = volume(self, now.0)? + volume(self, piece.0)?;
                    let op = self.k.boolean(BoolOp::Union, now.0, &[piece.0])?;
                    let checked = self.k.topology(op.shape).map_err(Stop::from).and_then(|topo| {
                        let whole = volume(self, op.shape)?;
                        if (whole - parts).abs() > 1e-6 * (1.0 + parts) {
                            return Err(format!(
                                "A bend runs into the part: {} mm³ of it would be where the part already is. Use a smaller bend radius, or bend the edges that meet at a corner in one Bend.",
                                crate::explain::mm((parts - whole).abs())
                            )
                            .into());
                        }
                        Ok(names_of_boolean(&op.history, &[&now.1, &piece.1], topo.faces))
                    });
                    match checked {
                        Ok(names) => Ok((op.shape, names)),
                        Err(e) => {
                            self.k.release(op.shape);
                            Err(e)
                        }
                    }
                })();
                self.k.release(piece.0);
                let next = step?;
                self.k.release(now.0);
                now = next;
            }
            // A bend whose end lies against sheet that runs on beside it is joined to that
            // sheet sideways, and the end is no longer all there.
            let whole = self.k.topology(now.0)?;
            for (cap, size) in caps {
                let mut left = 0.0;
                for (f, name) in now.1.iter().enumerate() {
                    if matches!(name, Some(FaceOrigin::From { feature, source, .. }) if *feature == id && *source == cap) {
                        left += self.k.face_info(now.0.face(u32::try_from(f).map_err(|_| "too many faces")?))?.area;
                    }
                }
                if left < size * (1.0 - 1e-6) {
                    return Err(
                        "The sheet runs on beside the end of a bend, so the bend would be joined to it sideways. Open the corner there first (Corner Seam > Corner Rip), or cut the sheet back to the bend line beside it, or bend the edges that meet there in one Bend."
                            .into(),
                    );
                }
            }
            if whole.solids > lumps {
                return Err(
                    "The bends would leave a piece of the sheet on its own, where two of them meet at a corner of it. Rip the corners by their outside edges, so that the floor's corners go with them, or cut the piece off first."
                        .into(),
                );
            }
            Ok(())
        })();
        if let Err(e) = joined {
            for (s, _) in pieces {
                self.k.release(s);
            }
            self.k.release(now.0);
            return Err(e);
        }
        self.k.release(body);
        if let Some(view) = self.regen.bodies.get_mut(bi) {
            (view.shape, view.names) = now;
        }
        self.sheet_unify(bi, id);
        Ok(())
    }

    /// A corner rip: each edge given is the outside or the inside edge of a square corner
    /// where two walls of the sheet meet with no seam, and the walls are cut back from it as
    /// the seam says, for the length of the edge. Seen along the edge with the corner's
    /// outside at the origin, the first wall runs out along `u` (one thickness deep in `v`)
    /// and the second along `v`:
    /// - symmetric: both stop where their inner faces are the gap apart, and the corner
    ///   between them goes;
    /// - overlap: the first keeps the corner (as far into the second's thickness as the
    ///   overlap says) and the second stops the gap short of its inner face; in a reverse
    ///   overlap the second keeps it.
    pub(super) fn corner_rip(&mut self, id: FeatureId, rip: &CornerRip) -> Result<(), Stop> {
        rip.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let t = sizes.thickness;
        let gap = rip.corner.gap.unwrap_or(sizes.gap);
        if gap < 1e-6 {
            return Err(
                "A corner rip with no gap leaves the corner as it is: give it a gap, or set the part's Miter Gap in Sheet Metal Defaults.".into()
            );
        }
        let (bi, body, found) = self.sheet_edges(&rip.edges, "corner rip")?;
        let topo = self.k.topology(body)?;
        let faces: Vec<tenon_kernel::FaceInfo> = (0..topo.faces).map(|i| self.k.face_info(body.face(i))).collect::<Result<_, _>>()?;
        let names = self.regen.bodies.get(bi).map(|b| b.names.clone()).unwrap_or_default();
        // A flat face of the sheet with its other side one thickness behind it: its normal.
        let side = |f: u32| -> Option<Vec3> {
            let face = faces.get(f as usize)?;
            let SurfaceKind::Plane { normal, .. } = &face.surface else { return None };
            let n = normal.normalized();
            faces
                .iter()
                .any(|g| matches!(&g.surface, SurfaceKind::Plane { normal: m, .. } if m.normalized().dot(n) < -1.0 + 1e-9 && ((g.centroid - face.centroid).dot(n) + t).abs() < 1e-6 * (1.0 + t)))
                .then_some(n)
        };
        let mut cuts: Vec<Named> = Vec::with_capacity(found.len());
        let built = (|| -> Result<(), Stop> {
            for ((e, key), picked) in found.iter().zip(&rip.edges) {
                let info = self.k.edge_info(body.edge(*e))?;
                if !matches!(info.curve, CurveKind::Line { .. }) || info.length < 1e-6 {
                    return Err("A corner rip opens a straight corner, and an edge picked for it is curved.".into());
                }
                let &[fa, fb] = topo.edge_faces.get(*e as usize).map_or(&[][..], Vec::as_slice) else {
                    return Err("A corner rip goes on an edge between two faces of the sheet.".into());
                };
                // The first wall is the one whose face the edge is named by first.
                let named = |f: u32| names.get(f as usize).copied().flatten();
                let (fa, fb) = if named(fb) == Some(picked.faces[0]) && named(fa) != Some(picked.faces[0]) { (fb, fa) } else { (fa, fb) };
                let (Some(na), Some(nb)) = (side(fa), side(fb)) else {
                    return Err(
                        "A corner rip opens the corner where two walls of the sheet meet, and an edge picked is not one: the faces either side of it are not both flat sides of the sheet. (An edge of the sheet's rim is no corner, and a corner with a bend round it is open already.)"
                            .into(),
                    );
                };
                if na.dot(nb).abs() > 1e-6 {
                    return Err("The two walls at an edge picked do not meet squarely, and a corner rip opens square corners.".into());
                }
                // The outside edge of the corner, or the inside one: beside the inside edge
                // there is sheet a little way along either wall, beside the outside edge none.
                let middle = (info.start + info.end) * 0.5;
                let mut inside = [false; 2];
                for (slot, way) in inside.iter_mut().zip([na - nb, nb - na]) {
                    let probe = self.k.make_sphere(middle + way * (t / 2.0), t * 0.001)?.shape;
                    let near = self.k.min_distance(SubShape::Shape(body), SubShape::Shape(probe)).map(|d| d.value < 1e-6);
                    self.k.release(probe);
                    *slot = near?;
                }
                let (u, v, corner) = match inside {
                    [false, false] => (-nb, -na, info.start),
                    [true, true] => (nb, na, info.start - (na + nb) * t),
                    _ => return Err("The walls at an edge picked are too short to tell which side of the corner it is on: a corner rip needs walls longer than they are thick.".into()),
                };
                let along = (info.end - info.start).normalized();
                // Seen with `u` to the right and `v` up, from the end the prism grows from.
                let (origin, axis) = if along.cross(u).dot(v) > 0.0 { (corner, along) } else { (corner + along * info.length, -along) };
                let frame = Frame::new(origin, axis, u).ok_or("the corner's edge has no direction")?;
                let tag = sub_key(*key, 1);
                let at = |x: f64, y: f64| (Vec2::new(x, y), tag);
                let keeps = t * (1.0 - rip.corner.overlap);
                // (The wall that keeps the corner loses none of it at a whole overlap.)
                let outline = match (rip.corner.seam, keeps > 1e-9) {
                    (Seam::Symmetric, _) => {
                        let reach = t + gap / 2f64.sqrt();
                        outline::polygon(&[at(0.0, 0.0), at(reach, 0.0), at(reach, reach), at(0.0, reach)])
                    }
                    (Seam::Overlap, true) => {
                        outline::polygon(&[at(0.0, 0.0), at(keeps, 0.0), at(keeps, t), at(t, t), at(t, t + gap), at(0.0, t + gap)])
                    }
                    (Seam::Overlap, false) => outline::polygon(&[at(0.0, t), at(t, t), at(t, t + gap), at(0.0, t + gap)]),
                    (Seam::ReverseOverlap, true) => {
                        outline::polygon(&[at(0.0, 0.0), at(0.0, keeps), at(t, keeps), at(t, t), at(t + gap, t), at(t + gap, 0.0)])
                    }
                    (Seam::ReverseOverlap, false) => outline::polygon(&[at(t, 0.0), at(t + gap, 0.0), at(t + gap, t), at(t, t)]),
                };
                let tool = self.sheet_prism(
                    id,
                    &Profile { frame, regions: vec![Region { outer: outline::to_loop(&outline), holes: vec![] }] },
                    info.length,
                    Some([tag, tag]),
                )?;
                cuts.push(tool);
            }
            Ok(())
        })();
        if let Err(e) = built {
            for (s, _) in cuts {
                self.k.release(s);
            }
            return Err(e);
        }
        self.sheet_assemble(bi, body, cuts, Vec::new(), Vec::new(), "corner rip", ["", ""])
    }

    /// A profile cut out of the sheet round its bends (`crate::sheet::wrap`): each face and bend
    /// the profile lies on, with the sheet laid flat from the face its sketch is on, loses its
    /// part of it.
    pub(super) fn cut_across_bend(&mut self, id: FeatureId, c: &CutAcrossBend) -> Result<(), Stop> {
        c.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let t = sizes.thickness;
        let (held, frame) = self.sketch_and_frame(c.sketch)?;
        let sketch: &Sketch = &held;
        let profile = profile_of(sketch, frame, &c.regions)?;
        let opts = Unfold { thickness: t, k_factor: sizes.k_factor, base: None, deflection: crate::sheet::FLAT_DEFLECTION };
        // The face the sketch is on.
        let seat = crate::sheet::seat_of(self.doc, &self.regen, &*self.k, c.sketch, &frame, &profile.regions)?;
        let Some((bi, base)) = seat else {
            return Err(
                "A cut across bends is drawn on the sheet: its sketch must be on a flat face of the sheet, and this one's plane has no face of the part in it. Put the sketch on the face the cut starts from."
                    .into(),
            );
        };
        let body = self.regen.bodies.get(bi).ok_or("no such body")?.shape;
        let tools = wrap::wrap(&*self.k, body, base, &frame, &profile.regions, c.distance.unwrap_or(t), &opts)?;
        self.sheet_tools(id, bi, body, &tools, false)
    }

    /// A cut normal: the profile is taken out of every face and bend of the sheet it is seen
    /// on, looking along its sketch's normal, each cut square to itself. (What is seen at an
    /// angle or on a bend is not the profile's own lines and arcs: those outlines go to the
    /// kernel as smooth curves through points of them.)
    pub(super) fn cut_normal(&mut self, id: FeatureId, c: &CutNormal) -> Result<(), Stop> {
        c.check()?;
        let sizes = self.doc.sheet_sizes()?;
        let t = sizes.thickness;
        let (held, frame) = self.sketch_and_frame(c.sketch)?;
        let sketch: &Sketch = &held;
        let profile = profile_of(sketch, frame, &c.regions)?;
        let opts = Unfold { thickness: t, k_factor: sizes.k_factor, base: None, deflection: crate::sheet::FLAT_DEFLECTION };
        let Some((bi, base, toward)) = crate::sheet::seen_on(self.doc, &self.regen, &*self.k, &frame, &profile.regions)? else {
            return Err(
                "No side of the sheet looks at the sketch: its plane is edge on to every face of the sheet. Put the sketch on a plane that looks at the face to be cut."
                    .into(),
            );
        };
        let body = self.regen.bodies.get(bi).ok_or("no such body")?.shape;
        let tools = wrap::project(&*self.k, body, base, toward, &frame, &profile.regions, c.distance.unwrap_or(t), &opts)?;
        self.sheet_tools(id, bi, body, &tools, true)
    }

    /// What a profile takes out of the faces and bends of body `bi`, cut out of it. `smooth`:
    /// the outlines were worked out point by point, and are curves through the points.
    fn sheet_tools(&mut self, id: FeatureId, bi: usize, body: ShapeHandle, tools: &[Tool], smooth: bool) -> Result<(), Stop> {
        // A wall of the cut is named after the curve of the profile that cuts it and the face of
        // the sheet it is cut in; its floor, where it does not go through, after that face.
        let names = self.regen.bodies.get(bi).map(|b| b.names.clone()).unwrap_or_default();
        let on = |face: u32| names.get(face as usize).copied().flatten().map_or(0, |n| n.key());
        // (A curve that comes to the kernel in more than one piece, as a closed one worked out
        // point by point does, has a wall to each: they are told apart by their order.)
        let named = |mut closed: tenon_kernel::Loop, face: u32| {
            let mut met: Vec<u64> = Vec::new();
            for c in &mut closed.curves {
                if c.tag == 0 {
                    c.tag = sub_key(on(face), 0);
                    continue;
                }
                let key = pair_key(sub_key(c.tag, 1), on(face));
                let nth = met.iter().filter(|k| **k == key).count() as u32;
                met.push(key);
                c.tag = if smooth && nth > 0 { sub_key(key, 16 + nth) } else { key };
            }
            closed
        };
        let flat = |pieces: &[outline::Piece], face: u32| named(if smooth { region::to_loop_smooth(pieces) } else { region::to_loop(pieces) }, face);
        let mut cuts: Vec<Named> = Vec::new();
        let built = (|| -> Result<(), Stop> {
            for tool in tools {
                let floor = sub_key(on(tool.face()), 2);
                let made = match tool {
                    Tool::Flat { face, frame, reach, shapes } => {
                        let regions = shapes
                            .iter()
                            .map(|s| Region { outer: flat(&s.outer, *face), holes: s.holes.iter().map(|h| flat(h, *face)).collect() })
                            .collect();
                        self.sheet_prism(id, &Profile { frame: *frame, regions }, *reach, Some([floor, floor]))?
                    }
                    Tool::Bend { face, frame, rho, inner, outer, outline } => {
                        let rolled = named(if smooth { region::to_loop_smooth(outline) } else { outline::to_loop(outline) }, *face);
                        let regions = vec![Region { outer: rolled, holes: vec![] }];
                        self.sheet_roll(id, &Profile { frame: *frame, regions }, *rho, *inner, *outer, [floor, floor])?
                    }
                };
                cuts.push(made);
            }
            Ok(())
        })();
        if let Err(e) = built {
            for (s, _) in cuts {
                self.k.release(s);
            }
            return Err(e);
        }
        self.sheet_assemble(bi, body, cuts, Vec::new(), Vec::new(), "cut", ["", ""])
    }

    /// A contour flange along edges of the sheet: its profile, drawn square to the first edge
    /// and starting at it, is taken as the cross-section at every edge given. The side of the
    /// sheet the edge is on runs on into the profile, so where the profile leaves the edge at
    /// an angle a bend joins the two, and the sheet is cut back to where that bend begins.
    fn contour_along(&mut self, id: FeatureId, c: &ContourFlange) -> Result<(), Stop> {
        let sizes = self.doc.sheet_sizes()?;
        let t = sizes.thickness;
        let (held, frame) = self.sketch_and_frame(c.sketch)?;
        let sketch: &Sketch = &held;
        let (drawn, mut ids) = contour_path(sketch)?;
        let (bi, body, found) = self.sheet_edges(&c.edges, "contour flange")?;
        let topo = self.k.topology(body)?;
        let mut sites: Vec<(Site, u64)> = Vec::with_capacity(found.len());
        for (e, key) in &found {
            sites.push((
                self.flange_site(body, &topo, *e, t, false).map_err(|stop| match stop {
                    Stop::Failed(m) => Stop::Failed(m.replace("A flange goes", "A contour flange goes").replace("a flange", "a contour flange")),
                    other => other,
                })?,
                *key,
            ));
        }
        let Some(&(first, _)) = sites.first() else { return Err("select an edge for the contour flange".into()) };
        // The profile seen in the cross-section of the first edge: out of the sheet and up from
        // the side the edge is on.
        if frame.z().cross(first.along).len() > 1e-6 {
            return Err(
                "The profile's sketch is not square to the edge: for a contour flange along an edge, draw the profile on a plane the edge runs straight through (the end of the sheet there, or a plane across it)."
                    .into(),
            );
        }
        let seen = |p: Vec2| {
            let from_edge = frame.plane_point(p) - first.origin;
            Vec2::new(from_edge.dot(first.out), from_edge.dot(first.up))
        };
        let across = |v: Vec3| Vec2::new(v.dot(first.out), v.dot(first.up));
        let mirrored = across(frame.x()).cross(across(frame.y())) < 0.0;
        let mut path: Vec<strip::Run> = drawn
            .iter()
            .map(|run| match *run {
                strip::Run::Line { from, to } => strip::Run::Line { from: seen(from), to: seen(to) },
                strip::Run::Arc { center, radius, from, sweep } => {
                    let (middle, start) = (seen(center), seen(Vec2::polar(center, radius, from)));
                    strip::Run::Arc { center: middle, radius, from: (start - middle).angle(), sweep: if mirrored { -sweep } else { sweep } }
                }
            })
            .collect();
        // It starts at the edge, whichever way round it was drawn.
        let at_edge = |p: Vec2| p.len() < 1e-6;
        let (Some(head), Some(tail)) = (path.first().map(strip::Run::start), path.last().map(strip::Run::end)) else {
            return Err("the profile is empty".into());
        };
        if !at_edge(head) {
            if !at_edge(tail) {
                return Err(format!(
                    "The profile does not start at the edge: its ends are {} mm and {} mm from it. Start it at the edge (shown as a point in a sketch square to it: project the edge or the face it bounds).",
                    crate::explain::mm(head.len()),
                    crate::explain::mm(tail.len())
                )
                .into());
            }
            path.reverse();
            for run in &mut path {
                *run = run.reversed();
            }
            ids.reverse();
        }
        // The side of the sheet leads up to the edge, and every corner from there on is a bend.
        let lead = 1.0e4;
        let mut whole = vec![strip::Run::Line { from: Vec2::new(-lead, 0.0), to: Vec2::ZERO }];
        whole.extend(path);
        let rounded = strip::rounded(&whole, c.radius.unwrap_or(sizes.bend_radius), -t, 0.0)?;
        let Some((strip::Run::Line { to: begins, .. }, _)) = rounded.first().copied() else {
            return Err("the contour flange has no profile".into());
        };
        // What the bend at the edge takes of the sheet.
        let trim = (-begins.x).max(0.0);
        let runs: Vec<strip::Run> = rounded.iter().skip(1).map(|p| p.0).collect();
        if runs.is_empty() {
            return Err("The profile only runs back along the sheet: draw it out of the sheet from the edge.".into());
        }
        if strip::crosses(&runs, -t, 0.0) {
            return Err("The contour flange folds into itself: its profile comes back through its own path. Shorten the part that does, or draw it clear of the rest.".into());
        }
        // A face is named after the edge, the curve of the profile it follows (a bend: the two
        // it joins; the sheet itself counts as one) and the side of the sheet it is.
        let curve = |i: usize| if i == 0 { sub_key(0, 7) } else { sub_key(u64::from(ids.get(i - 1).map_or(0, |e| e.0)), 1) };
        let (mut pieces, mut cuts): (Vec<Named>, Vec<Named>) = (Vec::new(), Vec::new());
        let built = (|| -> Result<(), Stop> {
            for (site, edge) in &sites {
                let span = flange::span(&c.width, site.length).map_err(|m| m.replace("The flange", "The contour flange"))?;
                let width = span.1 - span.0;
                let section = site.section_frame(span.0).ok_or("the contour flange's edge has no direction")?;
                let tag = |n: usize, left: bool| {
                    let [a, b] = rounded.get(n + 1).map_or([0, 0], |p| p.1);
                    sub_key(pair_key(*edge, if a == b { curve(a) } else { pair_key(curve(a), curve(b)) }), 2 + u32::from(left))
                };
                let name = |part: u32| sub_key(*edge, part);
                let region = strip::outline(&runs, -t, 0.0, tag, [name(Part::Root as u32), name(Part::Tip as u32)])?;
                let caps = [name(Part::Start as u32), name(Part::End as u32)];
                pieces.push(self.sheet_strip(id, &Profile { frame: section, regions: vec![region] }, &Extent::Distance(width), caps)?);
                if trim > 1e-9 {
                    let cut = name(Part::Cut as u32);
                    let back =
                        outline::polygon(&[(Vec2::new(-trim, 0.0), cut), (Vec2::ZERO, cut), (Vec2::new(0.0, -t), cut), (Vec2::new(-trim, -t), cut)]);
                    let profile = Profile { frame: section, regions: vec![Region { outer: outline::to_loop(&back), holes: vec![] }] };
                    cuts.push(self.sheet_prism(id, &profile, width, Some([cut, cut]))?);
                }
            }
            Ok(())
        })();
        if let Err(e) = built {
            for (s, _) in pieces.into_iter().chain(cuts) {
                self.k.release(s);
            }
            return Err(e);
        }
        self.sheet_assemble(
            bi,
            body,
            cuts,
            pieces,
            Vec::new(),
            "contour flange",
            [
                "Where it runs along edges that meet at a corner, give it a width or offsets so that it stops short of the corner.",
                "the edge picked for it is not on the rim of the sheet.",
            ],
        )?;
        // What runs straight on from the sheet is the sheet: one face, with the sheet's name.
        self.sheet_unify(bi, id);
        Ok(())
    }

    pub(super) fn contour_flange(&mut self, id: FeatureId, c: &ContourFlange) -> Result<(), Stop> {
        c.check()?;
        if !c.edges.is_empty() {
            return self.contour_along(id, c);
        }
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

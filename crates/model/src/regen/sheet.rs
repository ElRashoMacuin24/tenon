//! Regenerating sheet metal features: faces and flanges (see `crate::sheet`).

use tenon_geom::Vec2;
use tenon_kernel::{BoolOp, CurveKind, Extent, Origin, PrimitiveRole, Profile, ShapeHandle, SurfaceKind, TopoKind, Topology};
use tenon_sketch::{Sketch, profile, regions};

use super::{Ctx, Stop, label, select};
use crate::FeatureId;
use crate::naming::{FaceOrigin, edge_key, names_of_boolean, names_of_sweep, pair_key, resolve_edge, sub_key};
use crate::sheet::flange::{self, Notch, Part, Site};
use crate::sheet::{Face, Flange};

/// A solid on its way into the part, and the names of its faces.
type Named = (ShapeHandle, Vec<Option<FaceOrigin>>);

/// One edge of a flange feature.
struct Wall {
    site: Site,
    /// The key of the edge, for the names of what the flange makes on it.
    key: u64,
    /// The stretch of the edge the flange takes.
    span: (f64, f64),
    /// The ends that are at a corner shared with another edge of the feature.
    shared: [bool; 2],
}

/// Where two walls of one feature meet at a corner of the sheet, seen from above the first
/// wall (out of the sheet, along its edge): where their bend lines cross, the way each runs
/// on from there, and the corner's key.
struct Corner {
    wall: usize,
    at: Vec2,
    lines: [Vec2; 2],
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

        // The edges, on one body.
        let mut found: Vec<(usize, u32, u64)> = Vec::with_capacity(fl.edges.len());
        {
            let views = self.regen.views();
            for e in &fl.edges {
                let (b, i) = resolve_edge(e, &views, &*self.k).map_err(|m| {
                    let [a, c] = &e.faces;
                    format!(
                        "{m} (the edge between the {} and the {})",
                        a.describe(&label(self.doc, a.feature())),
                        c.describe(&label(self.doc, c.feature()))
                    )
                })?;
                if found.iter().any(|f| f.0 == b && f.1 == i) {
                    return Err("An edge is picked twice for the flange.".into());
                }
                found.push((b, i, edge_key(&e.faces[0], &e.faces[1])));
            }
        }
        let bi = found.first().map(|f| f.0).ok_or("select at least one edge for the flange")?;
        if found.iter().any(|f| f.0 != bi) {
            return Err("the edges are on different bodies".into());
        }
        let body = self.body_shape(bi)?;
        let topo = self.k.topology(body)?;
        let mut walls: Vec<Wall> = Vec::with_capacity(found.len());
        for (_, e, key) in &found {
            let site = self.flange_site(body, &topo, *e, t, fl.flip)?;
            let span = flange::span(&fl.width, site.length)?;
            walls.push(Wall { site, key: *key, span, shared: [false, false] });
        }

        // Where two of the walls meet at a corner of the sheet, each starts where the other's
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
                    let back = dims.setback * (1.0 + cos) / sin;
                    for (w, e) in [(i, ea), (j, eb)] {
                        let wall = &mut walls[w];
                        if e == 0 {
                            wall.span.0 = wall.span.0.max(back);
                        } else {
                            wall.span.1 = wall.span.1.min(wall.site.length - back);
                        }
                        wall.shared[e] = true;
                    }
                    let along = if ea == 0 { back } else { walls[i].site.length - back };
                    let lines = [Vec2::new(0.0, if ea == 0 { 1.0 } else { -1.0 }), Vec2::new(db.dot(a.out), db.dot(a.along)).normalized()];
                    corners.push(Corner { wall: i, at: Vec2::new(-dims.setback, along), lines, key: pair_key(walls[i].key, walls[j].key) });
                }
            }
        }
        if walls.iter().any(|w| w.span.1 - w.span.0 < 1e-6) {
            return Err(
                "The bends of the flanges beside it leave nothing of one of the edges: use a smaller bend radius, or leave that edge out.".into()
            );
        }

        // The solids: what is cut from the sheet for the bends, the bends and walls themselves,
        // and the reliefs at the corners. On a failure everything made so far is released.
        let margin = t;
        let mut cuts: Vec<Named> = Vec::new();
        let mut prisms: Vec<Named> = Vec::new();
        let mut reliefs: Vec<Named> = Vec::new();
        let built = (|| -> Result<(), Stop> {
            for w in &walls {
                if dims.setback > 1e-9 {
                    let notch = Notch::new(w.span, w.site.length, &dims, &sizes, rule.relief, w.shared);
                    let frame = w.site.plan_frame(margin).ok_or("the flange's edge has no direction")?;
                    let made =
                        self.sheet_prism(id, &flange::profile(frame, notch.outline(sub_key(w.key, Part::Cut as u32))), t + 2.0 * margin, None)?;
                    cuts.push(made);
                }
                let frame = w.site.section_frame(w.span.0).ok_or("the flange's edge has no direction")?;
                let section = Profile { frame, regions: vec![dims.section(|p| sub_key(w.key, p as u32))] };
                let caps = [sub_key(w.key, Part::Start as u32), sub_key(w.key, Part::End as u32)];
                let made = self.sheet_prism(id, &section, w.span.1 - w.span.0, Some(caps))?;
                prisms.push(made);
            }
            for c in &corners {
                let tag = sub_key(c.key, Part::Corner as u32);
                let Some(outline) = flange::corner_outline(rule.corner, c.at, c.lines[0], c.lines[1], sizes.corner_size, tag) else { continue };
                let frame = walls[c.wall].site.plan_frame(margin).ok_or("the flange's edge has no direction")?;
                let made = self.sheet_prism(id, &flange::profile(frame, outline), t + 2.0 * margin, None)?;
                reliefs.push(made);
            }
            Ok(())
        })();
        if let Err(e) = built {
            for (s, _) in cuts.into_iter().chain(prisms).chain(reliefs) {
                self.k.release(s);
            }
            return Err(e);
        }

        let volume = |ctx: &Self, s: ShapeHandle| ctx.k.mass_properties(s, 1.0).map(|m| m.volume);
        let added: f64 = prisms.iter().map(|p| volume(self, p.0).unwrap_or(0.0)).sum();
        let names = self.regen.bodies.get(bi).map(|b| b.names.clone()).ok_or("no such body")?;
        let mut now: Named = (body, names);
        let mut own = true;
        let steps: [(BoolOp, Vec<Named>); 3] = [(BoolOp::Cut, cuts), (BoolOp::Union, prisms), (BoolOp::Cut, reliefs)];
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
                    // The bends and walls must sit on the sheet without sharing space with it,
                    // and leave it in one piece.
                    let after = volume(self, now.0)?;
                    let lost = before_join + added - after;
                    if lost > 1e-6 * (1.0 + after) {
                        return Err(format!(
                            "The flange runs into the part: {} mm³ of it would be where the part already is. Make it lower or narrower, or bend it the other way.",
                            crate::explain::mm(lost)
                        )
                        .into());
                    }
                    if self.k.topology(now.0)?.solids != 1 {
                        return Err(
                            "The flange does not join the sheet: its bend starts further in than the sheet reaches there. Use a smaller bend radius or another bend position."
                                .into(),
                        );
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
}

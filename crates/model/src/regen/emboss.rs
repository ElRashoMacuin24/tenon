//! Regenerating Emboss and Derive (DEC-133).
//!
//! An emboss on the part's faces is the layer between those faces and the same faces a depth
//! along their normals, as far as the profile is seen on them along the sketch's normal: what
//! looks that way first is what it goes on. Laid round a cylinder or a cone instead (Wrap to Face),
//! it is the outline rolled round the surface with its lengths kept. From the sketch plane, it is
//! the prism on the one side of the plane taken out of the part and the prism on the other added.
//!
//! A derived part is the solids of another part file, rebuilt here, scaled and mirrored.

use std::collections::BTreeSet;

use tenon_geom::{Frame, Vec2, Vec3};
use tenon_kernel::{BoolOp, Curve2, Extent, Loop, Profile, Region, ShapeHandle, SubShape, SurfaceKind, TaggedCurve2, TopoKind, Transform};
use tenon_sketch::Sketch;

use super::{Ctx, Stop, corners, label, profile_centre, profile_of, profile_points, regenerate};
use crate::FeatureId;
use crate::document::{Derive, Emboss, EmbossStyle, Operation};
use crate::naming::{CapEnd, FaceOrigin, FaceRef, names_of_boolean, names_of_modify, names_of_sweep};

/// A solid on its way into the part, and the names of its faces.
type Named = (ShapeHandle, Vec<Option<FaceOrigin>>);

/// How far from square to the sketch's normal a face must look its way to be what an emboss goes
/// on: about a degree. A wall along the normal, or one that leans back, is not.
const FACING: f64 = 0.02;

/// How far a tool laid round a surface reaches past the surface on the other side from where it
/// goes, at most, so that it is not a face on a face for the kernel to judge.
const SUNK: f64 = 0.05;

/// The faces an emboss goes on, thickened: the solid of them, and how far it may reach from the
/// sketch plane.
struct Floors {
    solid: Named,
    /// Along the way the faces look.
    above: f64,
    /// The other way.
    below: f64,
}

/// A plane's sketch coordinates laid on another's: turned and moved, perhaps turned over, and
/// nothing stretched. A point of the sketch is `(q - from) . x` across and `(q - from) . y` up.
struct Laid {
    from: Vec2,
    x: Vec2,
    y: Vec2,
}

impl Laid {
    fn at(&self, q: Vec2) -> Vec2 {
        let d = q - self.from;
        Vec2::new(d.dot(self.x), d.dot(self.y))
    }

    /// A direction, not a place.
    fn way(&self, v: Vec2) -> Vec2 {
        Vec2::new(v.dot(self.x), v.dot(self.y))
    }

    fn turned_over(&self) -> bool {
        self.x.cross(self.y) < 0.0
    }

    /// A curve as a rolled outline takes it: a whole circle or ellipse is made two arcs, since a
    /// curve of an outline runs from one corner to another.
    fn curve(&self, c: &Curve2) -> Vec<Curve2> {
        use std::f64::consts::{PI, TAU};
        let over = self.turned_over();
        match c {
            Curve2::Line { start, end } => vec![Curve2::Line { start: self.at(*start), end: self.at(*end) }],
            Curve2::Arc { center, radius, start_angle, end_angle } => {
                let sweep = (end_angle - start_angle).rem_euclid(TAU);
                let sweep = if sweep <= 0.0 { TAU } else { sweep };
                let c2 = self.at(*center);
                // (Turned over, what ran counter-clockwise runs clockwise: from its end.)
                let from = if over { *start_angle + sweep } else { *start_angle };
                let start_angle = (self.at(Vec2::polar(*center, *radius, from)) - c2).angle();
                vec![Curve2::Arc { center: c2, radius: *radius, start_angle, end_angle: start_angle + sweep }]
            }
            Curve2::Circle { center, radius } => {
                let center = self.at(*center);
                vec![
                    Curve2::Arc { center, radius: *radius, start_angle: 0.0, end_angle: PI },
                    Curve2::Arc { center, radius: *radius, start_angle: PI, end_angle: TAU },
                ]
            }
            Curve2::BSpline { poles, degree } => vec![Curve2::BSpline { poles: poles.iter().map(|p| self.at(*p)).collect(), degree: *degree }],
            Curve2::Ellipse { center, major, minor } => {
                let (center, major) = (self.at(*center), self.way(*major));
                vec![
                    Curve2::EllipseArc { center, major, minor: *minor, start: 0.0, end: PI },
                    Curve2::EllipseArc { center, major, minor: *minor, start: PI, end: TAU },
                ]
            }
            // (Turned over, the point at t is the one at -t of the ellipse as it is then.)
            Curve2::EllipseArc { center, major, minor, start, end } => {
                let (start, end) = if over { (-*end, -*start) } else { (*start, *end) };
                vec![Curve2::EllipseArc { center: self.at(*center), major: self.way(*major), minor: *minor, start, end }]
            }
        }
    }

    fn outline(&self, frame: Frame, l: &Loop) -> Profile {
        let curves = l.curves.iter().flat_map(|c| self.curve(&c.curve).into_iter().map(|curve| TaggedCurve2 { tag: c.tag, curve })).collect();
        Profile { frame, regions: vec![Region { outer: Loop { curves }, holes: Vec::new() }] }
    }
}

/// How a tool is laid round a surface: its axis's frame, where the sketch lies on it, and what a
/// flat outline is rolled to, from `inner` to `outer`.
enum Round {
    Cylinder { rho: f64, inner: f64, outer: f64 },
    Cone { half_angle: f64, inner: f64, outer: f64 },
}

impl<'a> Ctx<'a> {
    pub(super) fn emboss(&mut self, id: FeatureId, e: &Emboss) -> Result<(), Stop> {
        e.check()?;
        let (held, frame) = self.sketch_and_frame(e.sketch)?;
        let sketch: &Sketch = &held;
        let profile = profile_of(sketch, frame, &e.regions)?;
        if self.regen.bodies.is_empty() {
            return Err("There is no part to emboss: make a solid first.".into());
        }
        match (&e.wrap, e.style) {
            (Some(face), _) => self.emboss_round(id, e, &profile, face),
            (None, EmbossStyle::FromPlane) => self.emboss_from_plane(id, e, &profile),
            (None, _) => self.emboss_on_faces(id, e, &profile),
        }
    }

    /// The profile as a prism, `extent` along the sketch's normal. Its sides are named after the
    /// sketch's curves, and its ends not at all: they are not what stays. With `lean` (the way it
    /// narrows, and by how much), its sides are tilted about the sketch plane.
    fn emboss_prism(&mut self, id: FeatureId, profile: &Profile, extent: &Extent, lean: Option<(Vec3, f64)>) -> Result<Named, Stop> {
        let op = self.k.extrude(profile, extent, None)?;
        let faces = self.k.topology(op.shape)?.faces;
        let names = names_of_sweep(&op.history, id, faces);
        let sided = |names: Vec<Option<FaceOrigin>>| -> Vec<Option<FaceOrigin>> { names.into_iter().map(|n| n.filter(|o| matches!(o, FaceOrigin::Side { .. }))).collect() };
        let Some((pull, angle)) = lean else { return Ok((op.shape, sided(names))) };
        let tilted = (|| -> Result<Named, Stop> {
            let topo = self.k.topology(op.shape)?;
            let sides: Vec<_> =
                (0..topo.faces).filter(|i| matches!(names.get(*i as usize), Some(Some(FaceOrigin::Side { .. })))).map(|i| op.shape.face(i)).collect();
            let drafted = self.k.draft(op.shape, &sides, pull, angle, &profile.frame)?;
            match self.k.topology(drafted.shape) {
                Ok(new) => Ok((drafted.shape, names_of_modify(&drafted.history, &names, &topo, id, new.faces))),
                Err(err) => {
                    self.k.release(drafted.shape);
                    Err(err.into())
                }
            }
        })();
        self.k.release(op.shape);
        let (shape, names) = tilted?;
        Ok((shape, sided(names)))
    }

    /// Taper as a [`Self::emboss_prism`] takes it: what it narrows along when the taper is
    /// positive, which is the way that lets the shape out of a mould.
    fn lean(e: &Emboss, narrows: Vec3) -> Option<(Vec3, f64)> {
        let t = e.taper.unwrap_or(0.0);
        (t != 0.0).then(|| (if t > 0.0 { narrows } else { -narrows }, t.abs()))
    }

    /// From the sketch plane: on the way the feature looks the prism is taken out of the part, on
    /// the other it is added, each as far as the depth. So where the plane lies off the part the
    /// shape stands up to it, and where it lies in the part the shape goes down to it.
    fn emboss_from_plane(&mut self, id: FeatureId, e: &Emboss, profile: &Profile) -> Result<(), Stop> {
        let sign = if e.reverse { -1.0 } else { 1.0 };
        let look = profile.frame.z() * sign;
        let bi = self.regen.bodies.len() - 1;
        let target = self.body_shape(bi)?;
        // A positive taper opens toward the way out of a mould from the plane both ways.
        let added = self.emboss_prism(id, profile, &Extent::Distance(-sign * e.depth), Self::lean(e, look))?;
        let cut = match self.emboss_prism(id, profile, &Extent::Distance(sign * e.depth), Self::lean(e, -look)) {
            Ok(c) => c,
            Err(err) => {
                self.k.release(added.0);
                return Err(err);
            }
        };
        // What is added must reach the part: a solid left floating is not an emboss.
        let reached = (|| -> Result<bool, Stop> {
            let topo = self.k.topology(added.0)?;
            for i in 0..topo.solids {
                let s = self.k.solid(added.0, i)?;
                let d = self.k.min_distance(SubShape::Shape(s.shape), SubShape::Shape(target));
                self.k.release(s.shape);
                if d?.value > tenon_geom::tol::LINEAR * 10.0 {
                    return Ok(false);
                }
            }
            Ok(true)
        })();
        match reached {
            Ok(true) => {}
            Ok(false) => {
                self.k.release(added.0);
                self.k.release(cut.0);
                return Err(format!(
                    "The profile does not reach the part from the sketch plane in {} mm: give it more depth, or flip the direction.",
                    crate::explain::mm(e.depth)
                )
                .into());
            }
            Err(err) => {
                self.k.release(added.0);
                self.k.release(cut.0);
                return Err(err);
            }
        }
        let before = self.k.mass_properties(target, 1.0)?.volume;
        if let Err(err) = self.combine_at(id, vec![added], Operation::Join, Some(bi)) {
            self.k.release(cut.0);
            return Err(err);
        }
        let joined = self.k.mass_properties(self.body_shape(bi)?, 1.0)?.volume;
        // (The prism on the far side may miss the part altogether: nothing to take away.)
        let meets = self.k.min_distance(SubShape::Shape(cut.0), SubShape::Shape(self.body_shape(bi)?))?.value <= tenon_geom::tol::LINEAR * 10.0;
        let open = meets && self.k.boolean(BoolOp::Intersect, cut.0, &[self.body_shape(bi)?]).ok().map(|op| {
            let v = self.k.mass_properties(op.shape, 1.0).map(|m| m.volume).unwrap_or(0.0);
            self.k.release(op.shape);
            v
        }).unwrap_or(0.0) > tenon_geom::tol::MIN_SIZE;
        if open {
            self.combine_at(id, vec![cut], Operation::Cut, Some(bi))?;
        } else {
            self.k.release(cut.0);
            if (joined - before).abs() <= tenon_geom::tol::MIN_SIZE {
                return Err("Nothing is added or taken away: the sketch plane lies on the part's surface here. Move the sketch off the face, or raise or recess the profile from it instead.".into());
            }
        }
        Ok(())
    }

    /// Raised from, or recessed into, what the profile is seen on along the sketch's normal.
    fn emboss_on_faces(&mut self, id: FeatureId, e: &Emboss, profile: &Profile) -> Result<(), Stop> {
        let frame = profile.frame;
        let sign = if e.reverse { -1.0 } else { 1.0 };
        let look = frame.z() * sign;
        let raise = e.style == EmbossStyle::Raise;
        // The layer is cut to the profile afterwards: first the faces under a box round it, wide
        // enough that the layer's own sides, which are square to the faces, fall outside it.
        let points: Vec<Vec2> = profile_points(profile).iter().map(|p| frame.to_local(*p).xy()).collect();
        let first = points.first().copied().ok_or("the profile is empty")?;
        let (lo, hi) = points.iter().fold((first, first), |(a, b), p| (a.min(*p), b.max(*p)));
        let across = self.target_bbox().map_or(0.0, |b| b.diagonal());
        let room = 2.0 * e.depth + 1.0 + across * e.taper.unwrap_or(0.0).abs().tan().min(1.0);
        let quad = [Vec2::new(lo.x - room, lo.y - room), Vec2::new(hi.x + room, lo.y - room), Vec2::new(hi.x + room, hi.y + room), Vec2::new(lo.x - room, hi.y + room)];
        let curves = (0..4).map(|i| TaggedCurve2 { tag: i as u64 + 1, curve: Curve2::Line { start: quad[i], end: quad[(i + 1) % 4] } }).collect();
        let wide = Profile { frame, regions: vec![Region { outer: Loop { curves }, holes: Vec::new() }] };
        // The solid it goes on: the last that it is seen on, as a feature goes on the last solid.
        let mut found = None;
        for bi in (0..self.regen.bodies.len()).rev() {
            if let Some(floors) = self.floors_under(id, bi, &wide, sign, e.depth, raise)? {
                found = Some((bi, floors));
                break;
            }
        }
        let Some((bi, floors)) = found else {
            return Err("The profile is seen on no face of the part from this side of the sketch: flip the direction, or move the sketch.".into());
        };
        let (forward, backward) = if e.reverse { (floors.below, floors.above) } else { (floors.above, floors.below) };
        // Raised, it narrows going up; recessed, going down, as a mould would have it.
        let narrows = if raise { look } else { -look };
        let (thick, thick_names) = floors.solid;
        let prism = self.emboss_prism(id, profile, &Extent::TwoSided { forward, backward }, Self::lean(e, narrows));
        let (prism, prism_names) = match prism {
            Ok(p) => p,
            Err(err) => {
                self.k.release(thick);
                return Err(err);
            }
        };
        let cut = self.k.boolean(BoolOp::Intersect, thick, &[prism]);
        self.k.release(thick);
        self.k.release(prism);
        let op = match cut {
            Ok(op) => op,
            Err(err) if err.to_string().contains("empty shape") => return Err("The profile does not fall on the faces it is seen on: move the sketch.".into()),
            Err(err) => return Err(err.into()),
        };
        let names = match self.k.topology(op.shape) {
            Ok(t) => names_of_boolean(&op.history, &[&thick_names, &prism_names], t.faces),
            Err(err) => {
                self.k.release(op.shape);
                return Err(err.into());
            }
        };
        self.combine_at(id, vec![(op.shape, names)], if raise { Operation::Join } else { Operation::Cut }, Some(bi))
    }

    /// The faces of body `bi` the box `wide` is seen on, looking along the sketch's normal, those
    /// that look `sign` of the way along it, thickened by `depth` out of the part (`raise`) or
    /// into it. None when it is seen on none of that body.
    ///
    /// What is seen from the sketch plane looking the other way: the first faces met. A sketch
    /// plane in the material (on a plane through a shaft, say) looks from where the material
    /// ends over it. A sketch on a face looks at that face.
    fn floors_under(&mut self, id: FeatureId, bi: usize, wide: &Profile, sign: f64, depth: f64, raise: bool) -> Result<Option<Floors>, Stop> {
        let body = self.body_shape(bi)?;
        let frame = wide.frame;
        let look = frame.z() * sign;
        let Some(bb) = self.k.bounding_box(body)? else { return Ok(None) };
        let along: Vec<f64> = corners(&bb).into_iter().map(|c| (c - frame.origin()).dot(look)).collect();
        let (low, high) = along.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), t| (a.min(*t), b.max(*t)));
        let mut temp: Vec<ShapeHandle> = Vec::new();
        let mut layers: Vec<Named> = Vec::new();
        let result = (|| -> Result<Option<(f64, f64)>, Stop> {
            // Where the material goes on above the sketch plane, from the plane itself.
            let mut top = 0.0f64;
            if high > tenon_geom::tol::LINEAR {
                let up = self.k.extrude(wide, &Extent::Distance(sign * (high + 1.0)), None)?.shape;
                temp.push(up);
                match self.k.boolean(BoolOp::Intersect, up, &[body]) {
                    Ok(common) => {
                        temp.push(common.shape);
                        let plane = self.k.make_face(wide)?.shape;
                        temp.push(plane);
                        for i in 0..self.k.topology(common.shape)?.solids {
                            let s = self.k.solid(common.shape, i)?;
                            temp.push(s.shape);
                            if self.k.min_distance(SubShape::Shape(s.shape), SubShape::Shape(plane))?.value < 1e-6
                                && let Some(b) = self.k.bounding_box(s.shape)?
                            {
                                let far = corners(&b).into_iter().map(|c| (c - frame.origin()).dot(look)).fold(f64::NEG_INFINITY, f64::max);
                                top = top.max(far);
                            }
                        }
                    }
                    Err(err) if err.to_string().contains("empty shape") => {}
                    Err(err) => return Err(err.into()),
                }
            }
            if low >= -tenon_geom::tol::LINEAR && top <= tenon_geom::tol::LINEAR {
                return Ok(None);
            }
            // The space from just above all that, down through the part.
            let above = top + depth + 1.0;
            let below = (-low).max(0.0) + 1.0;
            let start_frame = frame.with_origin(frame.origin() + look * above).ok_or("the sketch's plane is out of range")?;
            let start = Profile { frame: start_frame, regions: wide.regions.clone() };
            let shaft = self.k.extrude(&start, &Extent::Distance(-sign * (above + below)), None)?.shape;
            temp.push(shaft);
            let free = match self.k.boolean(BoolOp::Cut, shaft, &[body]) {
                Ok(op) => op,
                Err(err) if err.to_string().contains("empty shape") => return Ok(None),
                Err(err) => return Err(err.into()),
            };
            temp.push(free.shape);
            let from_body: BTreeSet<u32> = free
                .history
                .images
                .iter()
                .filter(|i| i.source.input == 1 && i.source.id.kind == TopoKind::Face)
                .flat_map(|i| i.result.iter().filter(|r| r.kind == TopoKind::Face).map(|r| r.index))
                .collect();
            let cap = self.k.make_face(&start)?.shape;
            temp.push(cap);
            for i in 0..self.k.topology(free.shape)?.solids {
                let s = self.k.solid(free.shape, i)?;
                temp.push(s.shape);
                if self.k.min_distance(SubShape::Shape(s.shape), SubShape::Shape(cap))?.value > 1e-6 {
                    continue;
                }
                let faces: Vec<u32> = s
                    .history
                    .images
                    .iter()
                    .filter(|m| m.source.id.kind == TopoKind::Face && from_body.contains(&m.source.id.index))
                    .flat_map(|m| m.result.iter().filter(|r| r.kind == TopoKind::Face).map(|r| r.index))
                    .collect();
                for f in faces {
                    // (Of the space above the part, a face looks into the part.)
                    if self.k.face_normal(s.shape.face(f))?.dot(look) > -FACING {
                        continue;
                    }
                    let t = self.k.thicken(s.shape.face(f), if raise { -depth } else { depth })?;
                    let named = self.k.topology(t.shape).map(|topo| names_of_sweep(&t.history, id, topo.faces));
                    match named {
                        Ok(names) => layers.push((t.shape, names.into_iter().map(|n| n.filter(|o| matches!(o, FaceOrigin::Cap { end: CapEnd::End, .. }))).collect())),
                        Err(err) => {
                            self.k.release(t.shape);
                            return Err(err.into());
                        }
                    }
                }
            }
            Ok(Some((above, below)))
        })();
        for t in temp {
            self.k.release(t);
        }
        let reach = match result {
            Ok(Some(reach)) if !layers.is_empty() => reach,
            other => {
                for (s, _) in layers {
                    self.k.release(s);
                }
                return other.map(|_| None);
            }
        };
        let solid = self.joined(layers)?;
        Ok(Some(Floors { solid, above: reach.0, below: reach.1 }))
    }

    /// Solids as one, with the names of their faces. They are let go.
    fn joined(&mut self, mut solids: Vec<Named>) -> Result<Named, Stop> {
        if solids.len() < 2 {
            return solids.pop().ok_or_else(|| Stop::from("there is nothing to join"));
        }
        let tools: Vec<ShapeHandle> = solids.iter().skip(1).map(|s| s.0).collect();
        let op = self.k.boolean(BoolOp::Union, solids[0].0, &tools);
        let names = op.as_ref().ok().map(|op| {
            let inputs: Vec<&[Option<FaceOrigin>]> = solids.iter().map(|s| s.1.as_slice()).collect();
            self.k.topology(op.shape).map(|t| names_of_boolean(&op.history, &inputs, t.faces))
        });
        for (s, _) in solids {
            self.k.release(s);
        }
        let op = op?;
        match names {
            Some(Ok(names)) => Ok((op.shape, names)),
            Some(Err(err)) => {
                self.k.release(op.shape);
                Err(err.into())
            }
            None => Err("the layers could not be joined".into()),
        }
    }

    /// The profile laid round a cylinder or a cone, its lengths kept, and raised from it or
    /// recessed into it.
    fn emboss_round(&mut self, id: FeatureId, e: &Emboss, profile: &Profile, face: &FaceRef) -> Result<(), Stop> {
        let (bi, fi) = self.regen.resolve(face, &*self.k).map_err(|m| self.describe_ref(face, &m))?;
        let info = self.k.face_info(self.body_shape(bi)?.face(fi))?;
        let raise = e.style == EmbossStyle::Raise;
        let sunk = SUNK.min(e.depth * 0.2);
        // Which way the layer goes from the surface, as distances along its outward normal: a
        // hole's wall has its material outside it.
        let (inner, outer) = match (raise, info.reversed) {
            (true, false) => (-sunk, e.depth),
            (false, false) => (-e.depth, sunk),
            (true, true) => (-e.depth, sunk),
            (false, true) => (-sunk, e.depth),
        };
        let frame = profile.frame;
        let n = frame.z();
        // The direction along the surface's axis, and out from it toward the sketch.
        let lay = |axis_dir: Vec3, on_axis: Vec3| -> Result<(Vec3, Vec3), Stop> {
            if axis_dir.dot(n).abs() > std::f64::consts::FRAC_1_SQRT_2 {
                return Err("Wrap to Face lays the profile along the surface from a sketch plane that runs along its axis, and this one looks along the axis: sketch on a plane along it.".into());
            }
            let square = (n - axis_dir * axis_dir.dot(n)).normalized();
            let height = (frame.origin() - on_axis).dot(square);
            // The side of the axis the sketch plane is on; through the axis, the way it looks.
            let out = if height < -tenon_geom::tol::LINEAR { -square } else { square };
            Ok((if e.reverse { -out } else { out }, axis_dir))
        };
        let sketch_way = |v: Vec3| Vec2::new(v.dot(frame.x()), v.dot(frame.y()));
        let sketch_at = |p: Vec3| frame.to_local(p).xy();
        let (on_axis, round, laid) = match info.surface {
            SurfaceKind::Cylinder { axis, radius } => {
                let (out, up) = lay(axis.dir(), axis.origin())?;
                if radius + inner <= tenon_geom::tol::MIN_SIZE {
                    return Err(format!("A {} mm depth goes past the axis of this {} mm radius: use a smaller depth.", crate::explain::mm(e.depth), crate::explain::mm(radius)).into());
                }
                let across = up.cross(out);
                let laid = Laid { from: sketch_at(axis.origin()), x: sketch_way(across), y: sketch_way(up).normalized() };
                let f = Frame::new(axis.origin(), up, out).ok_or("the cylinder's axis has no direction")?;
                (f, Round::Cylinder { rho: radius, inner: radius + inner, outer: radius + outer }, laid)
            }
            SurfaceKind::Cone { axis, half_angle, ref_radius } => {
                // (Its half angle is negative when it widens against its axis' direction.)
                let (wide, alpha) = if half_angle < 0.0 { (-axis.dir(), -half_angle) } else { (axis.dir(), half_angle) };
                let apex = axis.origin() - wide * (ref_radius / alpha.tan());
                let (out, up) = lay(wide, apex)?;
                let (along, across) = (up * alpha.cos() + out * alpha.sin(), up.cross(out));
                let way = sketch_way(along);
                if way.len() < tenon_geom::tol::LINEAR {
                    return Err("Wrap to Face lays the profile along the cone from a sketch plane that runs along it, and this one looks along it: sketch on a plane along it.".into());
                }
                let y = way.normalized();
                // The profile's middle stays where it is seen: the cone's point under it, as far
                // from the apex in the sketch's plane as that point is along the cone.
                let middle = (profile_centre(profile) - apex).dot(frame.x() * y.x + frame.y() * y.y);
                let from_apex = middle / along.dot(frame.x() * y.x + frame.y() * y.y);
                if from_apex <= tenon_geom::tol::MIN_SIZE {
                    return Err("The profile lies past the cone's apex: move the sketch along the cone.".into());
                }
                let laid = Laid { from: sketch_at(apex) - y * (from_apex - middle), x: sketch_way(across), y };
                let f = Frame::new(apex, up, out).ok_or("the cone's axis has no direction")?;
                (f, Round::Cone { half_angle: alpha, inner, outer }, laid)
            }
            _ => return Err("Wrap to Face lays the profile round a cylinder or a cone: pick one of those.".into()),
        };
        let mut tools: Vec<Named> = Vec::new();
        let mut made = || -> Result<(), Stop> {
            for region in &profile.regions {
                let outer_loop = self.rolled(id, &laid.outline(on_axis, &region.outer), &round)?;
                if region.holes.is_empty() {
                    tools.push(outer_loop);
                    continue;
                }
                let mut holes: Vec<Named> = Vec::new();
                let mut rolled_holes = || -> Result<(), Stop> {
                    for hole in &region.holes {
                        holes.push(self.rolled(id, &laid.outline(on_axis, hole), &round)?);
                    }
                    Ok(())
                };
                let done = rolled_holes();
                let shapes: Vec<ShapeHandle> = holes.iter().map(|h| h.0).collect();
                let cut = match done {
                    Ok(()) => self.k.boolean(BoolOp::Cut, outer_loop.0, &shapes).map_err(Stop::from),
                    Err(err) => Err(err),
                };
                let names = cut.as_ref().ok().map(|op| {
                    let inputs: Vec<&[Option<FaceOrigin>]> = std::iter::once(outer_loop.1.as_slice()).chain(holes.iter().map(|h| h.1.as_slice())).collect();
                    self.k.topology(op.shape).map(|t| names_of_boolean(&op.history, &inputs, t.faces))
                });
                self.k.release(outer_loop.0);
                for s in shapes {
                    self.k.release(s);
                }
                let op = cut?;
                match names {
                    Some(Ok(names)) => tools.push((op.shape, names)),
                    Some(Err(err)) => {
                        self.k.release(op.shape);
                        return Err(err.into());
                    }
                    None => return Err("the holes of the profile could not be cut from it".into()),
                }
            }
            Ok(())
        };
        if let Err(err) = made() {
            for (s, _) in tools {
                self.k.release(s);
            }
            return Err(err);
        }
        self.combine_at(id, tools, if raise { Operation::Join } else { Operation::Cut }, Some(bi))
    }

    /// One outline rolled round the surface. Its caps and sides have their names.
    fn rolled(&mut self, id: FeatureId, outline: &Profile, round: &Round) -> Result<Named, Stop> {
        let op = match round {
            Round::Cylinder { rho, inner, outer } => self.k.roll(outline, *rho, *inner, *outer)?,
            Round::Cone { half_angle, inner, outer } => self.k.roll_cone(outline, *half_angle, *inner, *outer)?,
        };
        match self.k.topology(op.shape) {
            Ok(t) => Ok((op.shape, names_of_sweep(&op.history, id, t.faces))),
            Err(err) => {
                self.k.release(op.shape);
                Err(err.into())
            }
        }
    }

    /// The solids of another part file, rebuilt here: scaled about the origin, then mirrored, and
    /// each face named after the face it was there.
    pub(super) fn derive(&mut self, id: FeatureId, d: &Derive) -> Result<(), Stop> {
        d.check()?;
        let name = d.file_name();
        let source = match self.doc.links().sources.get(&d.path) {
            Some(s) => s.clone(),
            None => return Err(format!("{name} has not been read: save or rebuild the part so that it is, or edit the feature and choose the file again.").into()),
        };
        let other = source.document.map_err(Stop::Failed)?;
        let mirror = match &d.mirror {
            Some(p) => Some(self.plane_frame(p)?),
            None => None,
        };
        let mut made = regenerate(&other, self.k);
        if made.cancelled {
            made.release(self.k);
            return Err(Stop::Cancelled);
        }
        if let Some((f, why)) = made.first_error() {
            let said = format!("{name} does not rebuild: {}: {why}", label(&other, f));
            made.release(self.k);
            return Err(said.into());
        }
        let bodies = std::mem::take(&mut made.bodies);
        made.release(self.k);
        if bodies.is_empty() {
            return Err(format!("{name} has no solid to bring in.").into());
        }
        let mut pieces: Vec<Named> = Vec::new();
        let mut kept: Vec<ShapeHandle> = bodies.iter().map(|b| b.shape).collect();
        let mut go = || -> Result<(), Stop> {
            for (ordinal, b) in bodies.iter().enumerate() {
                let mut shape = b.shape;
                let mut names = b.names.clone();
                let mut moves = Vec::new();
                if (d.scale - 1.0).abs() > f64::EPSILON {
                    moves.push(Transform::Scale { center: Vec3::ZERO, factor: d.scale });
                }
                if let Some(plane) = mirror {
                    moves.push(Transform::Mirror { plane });
                }
                for m in &moves {
                    let op = self.k.transform(shape, m)?;
                    kept.push(op.shape);
                    let faces = self.k.topology(op.shape)?.faces;
                    names = names_of_boolean(&op.history, &[&names], faces);
                    shape = op.shape;
                }
                let copy = self.k.duplicate(shape)?;
                let ordinal = u32::try_from(ordinal).map_err(|_| "too many solids")?;
                let named = names.into_iter().map(|o| o.map(|o| FaceOrigin::From { feature: id, source: o.key(), ordinal })).collect();
                pieces.push((copy, named));
            }
            Ok(())
        };
        let done = go();
        for s in kept {
            self.k.release(s);
        }
        if let Err(err) = done {
            for (s, _) in pieces {
                self.k.release(s);
            }
            return Err(err);
        }
        self.combine(id, pieces, d.operation)
    }
}

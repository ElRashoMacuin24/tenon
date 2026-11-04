//! An open assembly: the document with undo, its parts (each a part [`Session`] with its own
//! undo, for editing in place) and their geometry, and solving.

use std::collections::BTreeMap;
use std::sync::Arc;

use tenon_geom::{Aabb3, Frame, Vec3, tol};
use tenon_kernel::{Kernel, MeshTol};
use tenon_model::{CmdError, Document, Fingerprint, OriginAxis, Scene, Session};

use crate::geometry::{self, Prim};
use crate::math::M3;
use crate::model::{
    Assembly, Component, ComponentId, Element, Geom, MAX_COMPONENTS, Pattern, PatternId, PatternKind, RelKind, Relationship, RelationshipId, Reused,
    Run, Target, Tweak,
};
use crate::solve::{self, Body, BodyDof, End, Law, Motion, Rel};

/// Undo depth.
const MAX_UNDO: usize = 200;

/// A part file an assembly uses.
pub struct Part {
    /// The part document, with its own undo (edited in place from the assembly).
    pub session: Session,
    /// Its geometry and the document revision it was made from.
    pub scene: Option<(u64, Arc<Scene>)>,
    /// Why the file could not be read (or, for a part in a row of its design table, why that
    /// row cannot be had); its components are shown as missing.
    pub missing: Option<String>,
    /// Set for a part in one row of its design table, or a part's mirror image: worked out from
    /// the part file's own entry, never saved, and worked out again when that changes.
    pub variant: Option<Variant>,
}

impl Part {
    pub fn new(doc: Document) -> Part {
        Part { session: Session::new(doc), scene: None, missing: None, variant: None }
    }
    pub fn missing(why: impl Into<String>) -> Part {
        Part { session: Session::default(), scene: None, missing: Some(why.into()), variant: None }
    }
    /// The geometry is from the current document revision.
    pub fn is_current(&self) -> bool {
        self.missing.is_some() || self.scene.as_ref().is_some_and(|(rev, _)| *rev == self.session.revision())
    }
}

/// What a part in one row of its design table, or a part's mirror image, was worked out from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    /// The key of the part file's own entry.
    pub base: String,
    pub row: Option<String>,
    /// The part's mirror image, for mirrored occurrences (DEC-130).
    pub mirrored: bool,
    /// That entry's document revision when this was worked out (none: it was not loaded).
    pub base_revision: Option<u64>,
}

/// Parts by key ([`Component::key`]): a part file's path, or path and design table row.
pub type Parts = BTreeMap<String, Part>;

/// The geometry of a component's part, if there is any yet.
pub fn scene_of<'p>(parts: &'p Parts, c: &Component) -> Option<&'p Scene> {
    parts.get(&c.key()).and_then(|p| p.scene.as_ref()).map(|(_, s)| s.as_ref())
}

/// A component's bounding box in its part's coordinates.
pub fn local_bbox(parts: &Parts, c: &Component) -> Option<Aabb3> {
    scene_of(parts, c).and_then(Scene::bbox)
}

/// A box in part coordinates, placed.
pub fn placed_bbox(b: &Aabb3, f: &Frame) -> Aabb3 {
    let (l, h) = (b.min, b.max);
    let corners = (0..8)
        .map(|i| f.to_world(Vec3::new(if i & 1 == 0 { l.x } else { h.x }, if i & 2 == 0 { l.y } else { h.y }, if i & 4 == 0 { l.z } else { h.z })));
    Aabb3::from_points(corners).unwrap_or(*b)
}

/// The assembly's bounding box at its components' placements.
pub fn assembly_bbox(asm: &Assembly, parts: &Parts) -> Option<Aabb3> {
    asm.components.iter().filter_map(|c| local_bbox(parts, c).map(|b| placed_bbox(&b, &c.placement))).reduce(|a, b| a.union(&b))
}

/// What a solve found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Solved {
    pub converged: bool,
    /// Relationships that do not hold, and why.
    pub failing: Vec<(RelationshipId, String)>,
}

/// The solver's view of the assembly: bodies (one per component, in order) and the
/// relationships it can resolve, with the ids of those, plus the ones it cannot.
pub struct System {
    pub bodies: Vec<Body>,
    pub rels: Vec<Rel>,
    pub ids: Vec<RelationshipId>,
    pub unresolved: Vec<(RelationshipId, String)>,
}

/// An element of a pattern is held by the pattern's rule: to the solver it stands still, and
/// `place_elements` moves it after its source.
fn body_of(parts: &Parts, c: &Component, element: bool) -> Body {
    let (center, size) = match local_bbox(parts, c) {
        Some(b) => (c.placement.to_world(b.center()), b.diagonal()),
        None => (c.placement.origin(), 10.0),
    };
    Body { frame: c.placement, fixed: c.grounded || element, center, size }
}

/// The law of a relationship, with a symmetry's plane found in `asm` (DEC-115).
pub fn law_for(asm: &Assembly, parts: &Parts, kind: &RelKind) -> Result<Law, String> {
    match kind {
        RelKind::Symmetry { plane, .. } => Ok(Law::Symmetry { plane: resolve_target(asm, parts, plane)? }),
        _ => Ok(law_of(kind)),
    }
}

/// The law of a relationship kind (a symmetry's plane is at the world's origin: see [`law_for`]).
pub fn law_of(kind: &RelKind) -> Law {
    match kind {
        RelKind::Mate { offset, .. } => Law::Mate { offset: *offset },
        RelKind::Flush { offset, .. } => Law::Flush { offset: *offset },
        RelKind::Angle { angle, reference, .. } => Law::Angle { angle: *angle, reference: *reference },
        RelKind::Insert { offset, aligned, .. } => Law::Insert { offset: *offset, aligned: *aligned },
        RelKind::Tangent { offset, inside, .. } => Law::Tangent { offset: *offset, inside: *inside },
        RelKind::Symmetry { .. } => Law::Symmetry {
            plane: End { body: None, prim: geometry::Prim::Plane { point: Vec3::ZERO, normal: Vec3::Z }, radius: 0.0, mirrored: false },
        },
        RelKind::Joint { joint, flip, offset, angle, .. } => Law::Joint { joint: *joint, flip: *flip, offset: *offset, angle: *angle },
    }
}

/// The geometry a target means, in its component's part coordinates (assembly coordinates for
/// the assembly's own origin), and that component's index.
pub fn resolve_target(asm: &Assembly, parts: &Parts, t: &Target) -> Result<End, String> {
    match t.component {
        None => Ok(End { body: None, prim: geometry::resolve(&t.geom, None, false)?, radius: 0.0, mirrored: false }),
        Some(id) => {
            let i = asm.components.iter().position(|c| c.id == id).ok_or_else(|| format!("{id} does not exist"))?;
            let c = &asm.components[i];
            if let Some(why) = parts.get(&c.key()).and_then(|p| p.missing.as_ref()) {
                return Err(format!("{} is missing: {why}", c.name));
            }
            let prim = geometry::resolve(&t.geom, scene_of(parts, c), c.mirrored).map_err(|e| format!("{}: {e}", c.name))?;
            let radius = geometry::radius(&t.geom, scene_of(parts, c));
            Ok(End { body: Some(i), prim, radius, mirrored: c.mirrored })
        }
    }
}

/// The solver's view of `asm`.
pub fn system(asm: &Assembly, parts: &Parts) -> System {
    let elements = asm.elements();
    let bodies = asm.components.iter().map(|c| body_of(parts, c, elements.contains(&c.id))).collect();
    let (mut rels, mut ids, mut unresolved) = (Vec::new(), Vec::new(), Vec::new());
    for r in asm.relationships.iter().filter(|r| !r.suppressed) {
        let [a, b] = r.kind.targets();
        let ends = resolve_target(asm, parts, a).and_then(|a| Ok((a, resolve_target(asm, parts, b)?)));
        match ends.and_then(|(a, b)| {
            let law = law_for(asm, parts, &r.kind)?;
            solve::check(&law, &a.prim, &b.prim)?;
            if matches!(law, Law::Tangent { .. }) {
                solve::check_tangent(&a, &b)?;
            }
            Ok(Rel { law, a, b })
        }) {
            Ok(rel) => {
                rels.push(rel);
                ids.push(r.id);
            }
            Err(e) => unresolved.push((r.id, e)),
        }
    }
    System { bodies, rels, ids, unresolved }
}

/// How many times a solve and the patterns may answer each other before they are given up on: a
/// relationship on an element moves what is held by it, which may move the element's source.
const SOLVE_ROUNDS: usize = 8;

/// Solves `asm` in place: free components move so the relationships hold, and the elements of
/// patterns follow what they repeat. `drag` is a component the user moved, which stays as near
/// where it was put as it may.
pub fn solve_assembly(asm: &mut Assembly, parts: &Parts, drag: Option<ComponentId>) -> Solved {
    let elements = asm.elements();
    let held = asm.relationships.iter().filter(|r| !r.suppressed && r.kind.components().iter().any(|c| elements.contains(c))).map(|r| r.id);
    let held: Vec<RelationshipId> = held.collect();
    let dragged = drag.and_then(|d| asm.components.iter().position(|c| c.id == d));
    let mut solved = Solved::default();
    for _ in 0..SOLVE_ROUNDS {
        let sys = system(asm, parts);
        let sol = solve::solve(&sys.bodies, &sys.rels, dragged);
        for (c, f) in asm.components.iter_mut().zip(&sol.frames) {
            if !c.grounded && !elements.contains(&c.id) {
                c.placement = *f;
            }
        }
        let mut failing = sys.unresolved;
        if !sol.converged {
            for (id, e) in sys.ids.iter().zip(&sol.errors) {
                if *e > tol::ASSEMBLY_BROKEN {
                    failing.push((*id, "cannot hold together with the other relationships".into()));
                }
            }
        }
        solved = Solved { converged: sol.converged, failing };
        // (Nothing that holds on to an element, or no element moved: what was solved stands.)
        if !place_elements(asm, parts) || held.is_empty() {
            return solved;
        }
    }
    solved.converged = false;
    solved.failing.extend(held.into_iter().map(|id| (id, "holds on to an element of a pattern that moves with it".to_owned())));
    solved
}

// ---- patterns and mirrors (DEC-130) -----------------------------------------------------------

/// A target's geometry where it stands now, in assembly coordinates.
pub fn world_prim(asm: &Assembly, parts: &Parts, t: &Target) -> Result<Prim, String> {
    let end = resolve_target(asm, parts, t)?;
    Ok(match end.body.and_then(|i| asm.components.get(i)) {
        Some(c) => end.prim.placed(&M3::of_frame(&c.placement), c.placement.origin()),
        None => end.prim,
    })
}

/// What a pattern's rule comes to with things as they stand.
#[derive(Clone, Copy, Debug)]
enum Rule {
    /// One step along each direction: its way and its spacing.
    Rows { first: Vec3, second: Vec3 },
    /// Round an axis, by so much a step.
    Round { point: Vec3, axis: Vec3, step: f64 },
    /// Reflected in a plane.
    Reflect { point: Vec3, normal: Vec3 },
}

impl Rule {
    /// What takes a source to its element at `at`, for a rule that moves things.
    fn motion(&self, at: [u32; 2]) -> Motion {
        let [i, j] = at.map(f64::from);
        match *self {
            Rule::Rows { first, second } => Motion { r: M3::IDENTITY, t: first * i + second * j },
            Rule::Round { point, axis, step } => {
                let r = M3::exp(axis * (step * i));
                Motion { r, t: point - r.apply(point) }
            }
            Rule::Reflect { .. } => Motion::IDENTITY,
        }
    }
}

/// One step along a direction of a rectangular pattern.
fn step_along(asm: &Assembly, parts: &Parts, run: &Run) -> Result<Vec3, String> {
    let way = world_prim(asm, parts, &run.along)?.direction().ok_or("a point gives no direction: pick a straight edge, an axis or a flat face")?;
    Ok(way.normalized() * (if run.reverse { -run.spacing } else { run.spacing }))
}

fn rule_of(asm: &Assembly, parts: &Parts, kind: &PatternKind) -> Result<Rule, String> {
    match kind {
        PatternKind::Rectangular { first, second } => {
            let along = step_along(asm, parts, first)?;
            let across = match second {
                Some(s) if s.count > 1 => step_along(asm, parts, s)?,
                _ => Vec3::ZERO,
            };
            // Rows of rows along one line would stand on one another, and be counted.
            if first.count > 1 && across != Vec3::ZERO && along.normalized().cross(across.normalized()).len() < tol::UNIT {
                return Err("the two directions are along one line: pick a second direction across the first".into());
            }
            Ok(Rule::Rows { first: along, second: across })
        }
        PatternKind::Circular { axis, reverse, count, angle } => {
            let (point, dir) = match world_prim(asm, parts, axis)? {
                Prim::Line { point, dir } => (point, dir),
                Prim::Circle { center, normal, .. } => (center, normal),
                _ => return Err("that gives no axis: pick a straight edge, a round face or a circular edge".into()),
            };
            // As a part's circular pattern: a whole turn shared out, or from the first to the last.
            let full = (*angle - std::f64::consts::TAU).abs() < 1e-9;
            let step = if full { *angle / f64::from(*count) } else { *angle / f64::from(count.saturating_sub(1).max(1)) };
            Ok(Rule::Round { point, axis: dir.normalized(), step: if *reverse { -step } else { step } })
        }
        PatternKind::Mirror { plane, .. } => match world_prim(asm, parts, plane)? {
            Prim::Plane { point, normal } => Ok(Rule::Reflect { point, normal: normal.normalized() }),
            _ => Err("that is not flat: pick a flat face or a plane to mirror in".into()),
        },
    }
}

/// A frame standing for `f` reflected in a plane (`point`, `normal`, a unit vector) and, before
/// that, in the part's own coordinates in the plane through `within` square to `across`: two
/// reflections make a rigid motion. So with the part's XY plane for the second, the frame of a
/// mirrored occurrence, whose part is reflected in that plane; with the part's plane of symmetry,
/// the frame of the part placed again in the mirrored position.
pub fn reflected_frame(f: &Frame, point: Vec3, normal: Vec3, within: Vec3, across: Vec3) -> Option<Frame> {
    let way = |v: Vec3| {
        let w = f.dir_to_world(v - across * (2.0 * v.dot(across)));
        w - normal * (2.0 * w.dot(normal))
    };
    let origin = f.to_world(across * (2.0 * within.dot(across)));
    Frame::new(origin - normal * (2.0 * (origin - point).dot(normal)), way(Vec3::Z), way(Vec3::X))
}

fn axis_unit(a: OriginAxis) -> Vec3 {
    match a {
        OriginAxis::X => Vec3::X,
        OriginAxis::Y => Vec3::Y,
        OriginAxis::Z => Vec3::Z,
    }
}

/// Whether a part is the same on both sides of its middle plane square to `axis`, the plane
/// through the middle of its box: every face has its image there, of the same kind and area.
pub fn symmetric_about(scene: &Scene, axis: OriginAxis) -> bool {
    let Some(b) = scene.bbox() else { return false };
    let (middle, across, near) = (b.center(), axis_unit(axis), b.diagonal() * 1e-5 + 1e-6);
    let image = |p: Vec3| p - across * (2.0 * (p - middle).dot(across));
    let image_way = |v: Vec3| v - across * (2.0 * v.dot(across));
    let faces: Vec<Fingerprint> = scene.bodies.iter().flat_map(|x| x.faces.iter()).map(|(_, info)| Fingerprint::of(info)).collect();
    faces.iter().all(|f| {
        faces.iter().any(|g| {
            g.surface == f.surface
                && (g.area - f.area).abs() <= f.area.abs() * 1e-4 + 1e-6
                && g.centroid.dist(image(f.centroid)) <= near
                && image_way(f.direction).cross(g.direction).len() <= 1e-5
        })
    })
}

/// How to place `source` again in its position mirrored in `plane`, for a part that is the same
/// as its mirror image: `about` its own middle plane square to that axis or, with none given, the
/// one that turns it least (of those it is the same about).
pub fn reused(asm: &Assembly, parts: &Parts, source: ComponentId, plane: &Target, about: Option<OriginAxis>) -> Result<Reused, String> {
    let c = asm.component(source).ok_or_else(|| format!("{source} does not exist"))?;
    let scene = scene_of(parts, c).ok_or_else(|| format!("{}: the part's geometry is not available", c.name))?;
    let same: Vec<OriginAxis> = [OriginAxis::X, OriginAxis::Y, OriginAxis::Z].into_iter().filter(|a| symmetric_about(scene, *a)).collect();
    if let Some(a) = about {
        if same.contains(&a) {
            return Ok(Reused { component: source, about: a });
        }
        return Err(format!(
            "{} is not the same on both sides of its middle plane square to its {a:?} axis, so it cannot be placed again about it",
            c.name
        ));
    }
    let Prim::Plane { normal, .. } = world_prim(asm, parts, plane)? else {
        return Err("that is not flat: pick a flat face or a plane to mirror in".into());
    };
    let seen = c.placement.dir_to_local(normal);
    let square = |a: OriginAxis| axis_unit(a).dot(seen).abs();
    same.into_iter().max_by(|a, b| square(*a).total_cmp(&square(*b))).map(|about| Reused { component: source, about }).ok_or_else(|| {
        format!("{} is not the same as its mirror image, so it cannot be placed again in the mirrored position: leave it to be mirrored", c.name)
    })
}

/// A mirror's rule: reflected in `plane`, those in `reuse` placed again in the mirrored position.
pub fn mirror_kind(asm: &Assembly, parts: &Parts, plane: Target, reuse: &[ComponentId]) -> Result<PatternKind, String> {
    let reuse = reuse.iter().map(|c| reused(asm, parts, *c, &plane, None)).collect::<Result<Vec<_>, _>>()?;
    Ok(PatternKind::Mirror { plane, reuse })
}

/// What keeps a pattern's rule from being worked out now, in words: its elements then stay where
/// they are. None when all is well.
pub fn pattern_problem(asm: &Assembly, parts: &Parts, p: &Pattern) -> Option<String> {
    if let Err(e) = rule_of(asm, parts, &p.kind) {
        return Some(e);
    }
    if let PatternKind::Mirror { reuse, .. } = &p.kind {
        for r in reuse {
            let c = asm.component(r.component)?;
            if let Some(scene) = scene_of(parts, c)
                && !symmetric_about(scene, r.about)
            {
                return Some(format!("{} is no longer the same as its mirror image: mirror it instead of placing it again", c.name));
            }
        }
    }
    None
}

/// Puts every element where its pattern's rule says and makes it of what its source is of,
/// pattern by pattern (a later one may repeat an earlier one's elements). True when anything
/// moved or changed.
pub fn place_elements(asm: &mut Assembly, parts: &Parts) -> bool {
    let mut changed = false;
    for i in 0..asm.patterns.len() {
        let Some(p) = asm.patterns.get(i).cloned() else { continue };
        // (A rule that cannot be worked out leaves its elements where they are:
        // `pattern_problem` says why.)
        let Ok(rule) = rule_of(asm, parts, &p.kind) else { continue };
        for e in &p.elements {
            for (s, c) in p.sources.iter().zip(&e.components) {
                let Some(source) = asm.component(*s).cloned() else { continue };
                let (frame, mirrored) = match rule {
                    Rule::Reflect { point, normal } => {
                        let again = match &p.kind {
                            PatternKind::Mirror { reuse, .. } => reuse.iter().find(|r| r.component == *s),
                            _ => None,
                        };
                        match again {
                            Some(r) => {
                                // (Until its part's geometry is there it stays where it is.)
                                let Some(b) = local_bbox(parts, &source) else { continue };
                                (reflected_frame(&source.placement, point, normal, b.center(), axis_unit(r.about)), source.mirrored)
                            }
                            None => (reflected_frame(&source.placement, point, normal, Vec3::ZERO, Vec3::Z), !source.mirrored),
                        }
                    }
                    _ => (Some(rule.motion(e.at).apply_frame(&source.placement)), source.mirrored),
                };
                let Some(frame) = frame else { continue };
                let Some(c) = asm.component_mut(*c) else { continue };
                // (Nearer than what a file keeps of a placement is where it is: what is read back
                // is not moved, and so not changed.)
                let near = 1e-8;
                let moved = !c.placement.origin().near(frame.origin(), near)
                    || !c.placement.x().near(frame.x(), near)
                    || !c.placement.z().near(frame.z(), near);
                if moved || c.part != source.part || c.row != source.row || c.mirrored != mirrored {
                    changed = true;
                    c.placement = frame;
                    c.part = source.part;
                    c.row = source.row;
                    c.mirrored = mirrored;
                }
            }
        }
    }
    changed
}

/// A new component standing for `source`, in the same part: `place_elements` puts it.
fn new_element(asm: &mut Assembly, source: ComponentId) -> Result<ComponentId, CmdError> {
    let s = asm.component(source).cloned().ok_or_else(|| CmdError(format!("{source} does not exist")))?;
    let id = asm.take_component_id();
    let name = asm.occurrence_name(s.name.rsplit_once(':').map_or(s.name.as_str(), |(base, _)| base));
    asm.components.push(Component { id, name, grounded: false, visible: true, ..s });
    Ok(id)
}

/// What is wrong with a pattern about to be made or changed, in words.
fn check_pattern(asm: &Assembly, parts: &Parts, sources: &[ComponentId], kind: &PatternKind) -> Result<(), CmdError> {
    kind.check().map_err(CmdError)?;
    if sources.is_empty() {
        return Err("choose at least one component to repeat".into());
    }
    for (i, s) in sources.iter().enumerate() {
        asm.component(*s).ok_or_else(|| CmdError(format!("{s} does not exist")))?;
        if sources[..i].contains(s) {
            return Err(CmdError(format!("{s} is chosen twice")));
        }
    }
    if kind.places().len().saturating_mul(sources.len()).saturating_add(asm.components.len()) > MAX_COMPONENTS.saturating_add(asm.elements().len()) {
        return Err(CmdError(format!("that would make more than {MAX_COMPONENTS} components")));
    }
    rule_of(asm, parts, kind).map(|_| ()).map_err(CmdError)
}

/// Gives a pattern the sources and elements its rule asks for: elements that are still wanted stay
/// the same components, so what holds on to them holds; those no longer wanted go, with their
/// relationships; the rest are made. Each then stands where the rule says.
fn rebuild_elements(asm: &mut Assembly, parts: &Parts, id: PatternId, sources: Vec<ComponentId>) -> Result<(), CmdError> {
    let p = asm.pattern(id).cloned().ok_or_else(|| CmdError(format!("{id} does not exist")))?;
    let mut old: BTreeMap<([u32; 2], ComponentId), ComponentId> = BTreeMap::new();
    for e in &p.elements {
        old.extend(p.sources.iter().zip(&e.components).map(|(s, c)| ((e.at, *s), *c)));
    }
    let mut elements = Vec::new();
    for at in p.kind.places() {
        let mut components = Vec::with_capacity(sources.len());
        for s in &sources {
            components.push(match old.remove(&(at, *s)) {
                Some(c) => c,
                None => new_element(asm, *s)?,
            });
        }
        elements.push(Element { at, components });
    }
    if let Some(p) = asm.pattern_mut(id) {
        p.sources = sources;
        p.elements = elements;
    }
    // (They are no longer its elements: each goes alone.)
    for c in old.into_values() {
        asm.remove_component(c);
    }
    place_elements(asm, parts);
    Ok(())
}

/// Makes a pattern or mirror of `sources`, with its elements where its rule says. The caller
/// solves after it.
pub fn add_pattern(asm: &mut Assembly, parts: &Parts, sources: Vec<ComponentId>, kind: PatternKind) -> Result<PatternId, CmdError> {
    check_pattern(asm, parts, &sources, &kind)?;
    let id = asm.take_pattern_id();
    let name = asm.pattern_name(kind.label());
    asm.patterns.push(Pattern { id, name, suppressed: false, sources: Vec::new(), kind, elements: Vec::new() });
    rebuild_elements(asm, parts, id, sources)?;
    Ok(id)
}

/// Gives a pattern a new rule (a count, a spacing, a direction) or new sources: its elements are
/// moved, added or taken away. The caller solves after it.
pub fn change_pattern(asm: &mut Assembly, parts: &Parts, id: PatternId, sources: Vec<ComponentId>, kind: PatternKind) -> Result<(), CmdError> {
    check_pattern(asm, parts, &sources, &kind)?;
    let p = asm.pattern_mut(id).ok_or_else(|| CmdError(format!("{id} does not exist")))?;
    p.kind = kind;
    rebuild_elements(asm, parts, id, sources)
}

/// Takes a pattern's elements out of the assembly for now (not drawn, listed or checked), or brings
/// them back. Whether each is hidden on its own stays as it was.
pub fn suppress_pattern(asm: &mut Assembly, id: PatternId, on: bool) -> Result<(), CmdError> {
    asm.pattern_mut(id).ok_or_else(|| CmdError(format!("{id} does not exist")))?.suppressed = on;
    Ok(())
}

/// How far copies of `ids` go by default: along X by the width of what is copied and a gap, so
/// that they lie beside what they are copied from and not on it.
pub fn copy_offset(asm: &Assembly, parts: &Parts, ids: &[ComponentId]) -> Vec3 {
    let boxes = ids.iter().filter_map(|c| asm.component(*c)).filter_map(|c| local_bbox(parts, c).map(|b| placed_bbox(&b, &c.placement)));
    match boxes.reduce(|a, b| a.union(&b)) {
        Some(b) => Vec3::new(b.max.x - b.min.x + (b.diagonal() * 0.1).max(5.0), 0.0, 0.0),
        None => Vec3::new(10.0, 0.0, 0.0),
    }
}

/// Independent copies of `ids`, moved by `by`: further occurrences of their parts, or of the
/// part files `files` says stand for those (file by file). The relationships among them come too;
/// those that reach anything else stay with what they hold. Not grounded, so that they can be
/// moved. Returns the new components, in the order of `ids`, and relationships.
pub fn copy_components(
    asm: &mut Assembly,
    ids: &[ComponentId],
    by: Vec3,
    files: &BTreeMap<String, String>,
) -> Result<(Vec<ComponentId>, Vec<RelationshipId>), CmdError> {
    let mut map = BTreeMap::new();
    let mut made = Vec::with_capacity(ids.len());
    for c in ids {
        if map.contains_key(c) {
            continue;
        }
        let s = asm.component(*c).cloned().ok_or_else(|| CmdError(format!("{c} does not exist")))?;
        let placement = s.placement.with_origin(s.placement.origin() + by).ok_or("the copy would be out of range")?;
        let id = asm.take_component_id();
        let name = match files.get(&s.part) {
            Some(file) => {
                let file = file.rsplit(['/', '\\']).next().unwrap_or(file);
                asm.occurrence_name(file.strip_suffix(".tenon").unwrap_or(file))
            }
            None => asm.occurrence_name(s.name.rsplit_once(':').map_or(s.name.as_str(), |(base, _)| base)),
        };
        let part = files.get(&s.part).cloned().unwrap_or(s.part);
        asm.components.push(Component { id, name, part, placement, grounded: false, row: s.row, visible: true, mirrored: s.mirrored });
        map.insert(*c, id);
        made.push(id);
    }
    let copied: Vec<(RelKind, bool, &'static str)> =
        asm.relationships.iter().filter_map(|r| r.kind.between(&map).map(|k| (k, r.suppressed, r.kind.label()))).collect();
    let mut rels = Vec::with_capacity(copied.len());
    for (kind, suppressed, label) in copied {
        let id = asm.take_relationship_id();
        let name = asm.relationship_name(label);
        asm.relationships.push(Relationship { id, name, suppressed, kind });
        rels.push(id);
    }
    Ok((made, rels))
}

/// Remaining degrees of freedom of every component (in order) and of the whole assembly.
pub fn dof(asm: &Assembly, parts: &Parts) -> (Vec<BodyDof>, usize) {
    let sys = system(asm, parts);
    solve::dof(&sys.bodies, &sys.rels)
}

/// Where each component is in the exploded view: its placement moved by every step that moves it.
pub fn exploded(asm: &Assembly) -> BTreeMap<ComponentId, Frame> {
    let mut out: BTreeMap<ComponentId, Frame> = asm.components.iter().map(|c| (c.id, c.placement)).collect();
    for t in &asm.explode {
        for id in &t.components {
            if let Some(f) = out.get_mut(id) {
                *f = f.with_origin(f.origin() + t.direction * t.distance).unwrap_or(*f);
            }
        }
    }
    out
}

/// An exploded view made from the relationships: starting from grounded components, each other
/// component (and everything attached beyond it) moves `spacing` away from the component it is
/// attached to, along the direction of the relationship that holds it.
pub fn auto_explode(asm: &Assembly, parts: &Parts, spacing: f64) -> Vec<Tweak> {
    let n = asm.components.len();
    let index = |id: ComponentId| asm.components.iter().position(|c| c.id == id);
    // Neighbours with the direction pointing from the first to the second, and whether that
    // direction's sign means anything (a face normal does, an axis does not).
    let mut edges: Vec<Vec<(usize, Vec3, bool)>> = vec![Vec::new(); n];
    for r in asm.relationships.iter().filter(|r| !r.suppressed) {
        let [ta, tb] = r.kind.targets();
        let (Some(a), Some(b)) = (ta.component.and_then(index), tb.component.and_then(index)) else { continue };
        // A's side: a face normal points out of A's material, towards B.
        let (dir, signed) = match resolve_target(asm, parts, ta) {
            Ok(ea) => {
                let fa = &asm.components[a].placement;
                let pa = ea.prim.placed(&M3::of_frame(fa), fa.origin());
                match pa {
                    Prim::Plane { normal, .. } | Prim::Circle { normal, .. } => (normal, true),
                    Prim::Line { dir, .. } => (dir, false),
                    Prim::Point(_) => (Vec3::ZERO, false),
                }
            }
            Err(_) => (Vec3::ZERO, false),
        };
        edges[a].push((b, dir, signed));
        edges[b].push((a, -dir, signed));
    }
    // Breadth first from the grounded components (or the first one).
    let mut parent: Vec<Option<(usize, Vec3, bool)>> = vec![None; n];
    let mut seen = vec![false; n];
    let mut order = Vec::new();
    let mut queue: std::collections::VecDeque<usize> = asm.components.iter().enumerate().filter(|(_, c)| c.grounded).map(|(i, _)| i).collect();
    if queue.is_empty() && n > 0 {
        queue.push_back(0);
    }
    for i in &queue {
        seen[*i] = true;
    }
    loop {
        while let Some(i) = queue.pop_front() {
            order.push(i);
            for (j, dir, signed) in edges[i].clone() {
                if !seen[j] {
                    seen[j] = true;
                    parent[j] = Some((i, dir, signed));
                    queue.push_back(j);
                }
            }
        }
        // Components attached to nothing explode from the assembly's middle.
        match (0..n).find(|i| !seen[*i]) {
            Some(i) => {
                seen[i] = true;
                parent[i] = Some((usize::MAX, Vec3::ZERO, false));
                queue.push_back(i);
            }
            None => break,
        }
    }
    let middle = assembly_bbox(asm, parts).map_or(Vec3::ZERO, |b| b.center());
    let center_of = |i: usize| {
        let c = &asm.components[i];
        local_bbox(parts, c).map_or(c.placement.origin(), |b| c.placement.to_world(b.center()))
    };
    // Every component carries the ones attached beyond it.
    let descendants = |i: usize| {
        let mut out = vec![i];
        let mut k = 0;
        while k < out.len() {
            let p = out[k];
            let children: Vec<usize> = (0..n).filter(|j| parent[*j].is_some_and(|(q, _, _)| q == p) && !out.contains(j)).collect();
            out.extend(children);
            k += 1;
        }
        out
    };
    let mut tweaks = Vec::new();
    for i in order {
        let Some((p, dir, signed)) = parent[i] else { continue };
        let from = if p == usize::MAX { middle } else { center_of(p) };
        let mut d = dir;
        if d.len() < tol::LINEAR {
            d = center_of(i) - from;
        }
        if d.len() < tol::LINEAR {
            d = Vec3::Z;
        }
        // An axis has no side: go away from the component it is attached to.
        if !signed && (center_of(i) - from).dot(d) < 0.0 {
            d = -d;
        }
        let components = descendants(i).into_iter().map(|j| asm.components[j].id).collect();
        tweaks.push(Tweak { components, direction: d.normalized(), distance: spacing });
    }
    tweaks
}

/// A bill of materials row: one per part file, or per size of one (a row of its design table).
#[derive(Clone, Debug, PartialEq)]
pub struct BomRow {
    pub item: usize,
    /// The part file's name without its folder or extension, and the size when a row is named:
    /// "bolt (M8x50)".
    pub part: String,
    /// The part's key among the assembly's parts ([`Component::key`]).
    pub key: String,
    /// The part document's name, and the size when a row is named: "Bolt, M8x50".
    pub name: String,
    /// The row of the part's design table these components use.
    pub row: Option<String>,
    /// These are mirrored occurrences of the part: its mirror image, a part of its own to make.
    pub mirrored: bool,
    pub quantity: usize,
    /// What the part is made of, when it has a material.
    pub material: Option<String>,
    /// Of one part (grams), at its material's density, when its geometry is known.
    pub mass: Option<f64>,
    /// Of one part (mm^3), when its geometry is known.
    pub volume: Option<f64>,
    /// The part's own number and description, when it says them (parts lists show the file's
    /// name and the part's name in their place).
    pub part_number: Option<String>,
    pub description: Option<String>,
    pub components: Vec<String>,
}

impl BomRow {
    /// What a parts list says the part's number is: its own, with the size when it comes in
    /// several, or its file's name.
    pub fn number(&self) -> String {
        let number = match (&self.part_number, &self.row) {
            (Some(n), Some(row)) => format!("{n} ({row})"),
            (Some(n), None) => n.clone(),
            (None, _) => return self.part.clone(),
        };
        if self.mirrored { format!("{number} (mirrored)") } else { number }
    }

    /// What a parts list says the part is: its description, or its name.
    pub fn what(&self) -> String {
        match &self.description {
            Some(d) if self.mirrored => format!("{d}, mirrored"),
            Some(d) => d.clone(),
            None => self.name.clone(),
        }
    }
}

/// The parts list, in the order parts were first placed.
pub fn bom(asm: &Assembly, parts: &Parts) -> Vec<BomRow> {
    let mut rows = bom_with(
        asm,
        |key| parts.get(key).filter(|p| p.missing.is_none()).map(|p| p.session.document().name.clone()),
        |c| scene_of(parts, c).map(|s| s.bodies.iter().map(|b| b.volume).sum()),
    );
    // What each part is made of and what one of it weighs (a cubic centimetre is 1000 mm^3).
    for r in &mut rows {
        let Some(doc) = parts.get(&r.key).filter(|p| p.missing.is_none()).map(|p| p.session.document()) else { continue };
        r.material = doc.material().map(|m| m.name.clone());
        r.mass = r.volume.map(|v| v * doc.density() / 1000.0);
        r.part_number.clone_from(&doc.part_number);
        r.description.clone_from(&doc.description);
    }
    rows
}

/// The parts list from what is known of the parts: `name` gives a part's document name (none:
/// the file name stands in), `volume` a component's part volume. Every parts list in Tenon (the
/// assembly's, a drawing's) is made here, so they number parts alike. The elements of a
/// suppressed pattern are out of it; a mirrored occurrence has a row of its own.
pub fn bom_with(asm: &Assembly, name: impl Fn(&str) -> Option<String>, volume: impl Fn(&Component) -> Option<f64>) -> Vec<BomRow> {
    let mut rows: Vec<BomRow> = Vec::new();
    let out = asm.suppressed_elements();
    for c in asm.components.iter().filter(|c| !out.contains(&c.id)) {
        let key = c.key();
        if let Some(r) = rows.iter_mut().find(|r| r.key == key) {
            r.quantity += 1;
            r.components.push(c.name.clone());
            continue;
        }
        let file = c.part.rsplit(['/', '\\']).next().unwrap_or(&c.part);
        let stem = file.strip_suffix(".tenon").unwrap_or(file).to_owned();
        let name = name(&c.part).unwrap_or_else(|| stem.clone());
        let (stem, name) = match &c.row {
            Some(row) => (format!("{stem} ({row})"), format!("{name}, {row}")),
            None => (stem, name),
        };
        let (stem, name) = if c.mirrored { (format!("{stem} (mirrored)"), format!("{name}, mirrored")) } else { (stem, name) };
        rows.push(BomRow {
            item: rows.len() + 1,
            part: stem,
            key,
            name,
            row: c.row.clone(),
            mirrored: c.mirrored,
            quantity: 1,
            material: None,
            mass: None,
            volume: volume(c),
            part_number: None,
            description: None,
            components: vec![c.name.clone()],
        });
    }
    rows
}

/// Keeps the parts that are a part file in one row of its design table, or a part's mirror
/// image, in step with the assemblies given (the document and what a panel is showing) and with
/// the part files' own entries: makes those components need, works them out again when their part
/// file's document has changed, and drops those no component uses. Returns true when anything
/// changed.
pub fn sync_variants(assemblies: &[&Assembly], parts: &mut Parts) -> bool {
    let wanted: BTreeMap<String, (&str, Option<&str>, bool)> = assemblies
        .iter()
        .flat_map(|a| a.components.iter())
        .filter(|c| c.row.is_some() || c.mirrored)
        .map(|c| (c.key(), (c.part.as_str(), c.row.as_deref(), c.mirrored)))
        .collect();
    let before = parts.len();
    parts.retain(|k, p| p.variant.is_none() || wanted.contains_key(k));
    let mut changed = parts.len() != before;
    for (key, (base, row, mirrored)) in wanted {
        let base_revision = parts.get(base).filter(|b| b.missing.is_none()).map(|b| b.session.revision());
        let variant = Variant { base: base.to_owned(), row: row.map(str::to_owned), mirrored, base_revision };
        if parts.get(&key).is_some_and(|p| p.variant.as_ref() == Some(&variant)) {
            continue;
        }
        // The part file's document, put at the row and reflected.
        let made: Result<Document, String> = match parts.get(base) {
            None => Err("its part file is not loaded".into()),
            Some(b) => match &b.missing {
                Some(why) => Err(why.clone()),
                None => {
                    let mut doc = b.session.document().clone();
                    let file = base.rsplit(['/', '\\']).next().unwrap_or(base);
                    doc.set_reflected(mirrored);
                    match row {
                        None => Ok(doc),
                        Some(row) => match doc.table_activate(row).and_then(|()| doc.sync_parameters()) {
                            Ok(()) => Ok(doc),
                            Err(e) => Err(format!("{file} cannot be had at row `{row}`: {e}")),
                        },
                    }
                }
            },
        };
        match (parts.get_mut(&key), made) {
            // (The entry is kept, so whoever regenerates it sees its revision move on.)
            (Some(p), Ok(doc)) => {
                p.session.replace_document(doc, None);
                p.missing = None;
                p.variant = Some(variant);
            }
            (Some(p), Err(why)) => {
                p.scene = None;
                p.missing = Some(why);
                p.variant = Some(variant);
            }
            (None, made) => {
                let mut p = match made {
                    Ok(doc) => Part::new(doc),
                    Err(why) => Part::missing(why),
                };
                p.variant = Some(variant);
                parts.insert(key, p);
            }
        }
        changed = true;
    }
    changed
}

/// An open assembly.
pub struct AsmSession {
    asm: Assembly,
    undo: Vec<Assembly>,
    /// How many steps have been put on `undo`, less those taken off: where a group of steps
    /// began is told by this even when the oldest steps have been dropped.
    steps: u64,
    redo: Vec<Assembly>,
    revision: u64,
    saved_revision: u64,
    pub parts: Parts,
    /// Relationships that failed in the last solve, and why.
    pub failing: Vec<(RelationshipId, String)>,
    /// The assembly as a panel would leave it, being shown: its parts are kept ready too.
    preview: Option<Box<Assembly>>,
}

impl Default for AsmSession {
    fn default() -> Self {
        AsmSession::new(Assembly::default())
    }
}

impl AsmSession {
    pub fn new(asm: Assembly) -> AsmSession {
        AsmSession {
            asm,
            undo: Vec::new(),
            steps: 0,
            redo: Vec::new(),
            revision: 1,
            saved_revision: 1,
            parts: Parts::new(),
            failing: Vec::new(),
            preview: None,
        }
    }
    pub fn assembly(&self) -> &Assembly {
        &self.asm
    }
    /// The assembly as an open panel would leave it, when one is being shown.
    pub fn preview(&self) -> Option<&Assembly> {
        self.preview.as_deref()
    }
    /// Shows the assembly as a panel would leave it (the parts it needs are made ready), or (none)
    /// stops. It is no change to the assembly.
    pub fn set_preview(&mut self, preview: Option<Assembly>) {
        self.preview = preview.map(Box::new);
        self.sync_variants();
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// The assembly or one of its parts has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision || self.parts.values().any(|p| p.variant.is_none() && p.session.is_dirty())
    }
    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Replaces the assembly and its parts (after opening a file); undo history is cleared.
    pub fn replace(&mut self, asm: Assembly, parts: Parts) {
        self.asm = asm;
        self.parts = parts;
        self.undo.clear();
        self.steps = 0;
        self.redo.clear();
        self.revision += 1;
        self.saved_revision = self.revision;
        self.failing.clear();
        self.preview = None;
        self.sync_variants();
    }

    /// Applies `f` as one undoable step; on error nothing changes.
    pub fn edit<T>(&mut self, f: impl FnOnce(&mut Assembly, &Parts) -> Result<T, CmdError>) -> Result<T, CmdError> {
        let before = self.asm.clone();
        match f(&mut self.asm, &self.parts).and_then(|v| self.asm.check_reach().and_then(|()| self.asm.validate()).map(|()| v).map_err(CmdError)) {
            Ok(v) => {
                if self.asm != before {
                    self.undo.push(before);
                    self.steps += 1;
                    if self.undo.len() > MAX_UNDO {
                        self.undo.remove(0);
                    }
                    self.redo.clear();
                    self.revision += 1;
                    self.sync_variants();
                }
                Ok(v)
            }
            Err(e) => {
                self.asm = before;
                Err(e)
            }
        }
    }

    /// A mark to come back to: everything done after it can be made one undo step
    /// ([`Self::squash_since`]) or taken back ([`Self::roll_back_to`]).
    pub fn mark(&self) -> u64 {
        self.steps
    }

    /// Where on `undo` the document as it was at `mark` is, if steps were added since.
    fn at_mark(&self, mark: u64) -> Option<usize> {
        let since = usize::try_from(self.steps.checked_sub(mark)?).ok()?;
        (since > 0 && since <= self.undo.len()).then(|| self.undo.len() - since)
    }

    /// Makes everything done since `mark` one undo step: a command made of several edits, or
    /// the commands an agent ran as one request, is undone as a whole.
    pub fn squash_since(&mut self, mark: u64) {
        if let Some(first) = self.at_mark(mark) {
            self.undo.truncate(first + 1);
            self.steps = mark + 1;
        }
    }

    /// Takes back everything done since `mark`, as if it had not been done (it is not there
    /// to redo): for a run of commands of which one failed.
    pub fn roll_back_to(&mut self, mark: u64) {
        if let Some(first) = self.at_mark(mark)
            && let Some(before) = self.undo.get(first).cloned()
        {
            self.asm = before;
            self.undo.truncate(first);
            self.redo.clear();
            self.steps = mark;
            self.revision += 1;
            self.failing.clear();
            self.sync_variants();
        }
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(a) => {
                self.redo.push(std::mem::replace(&mut self.asm, a));
                self.steps = self.steps.saturating_sub(1);
                self.revision += 1;
                self.sync_variants();
                true
            }
            None => false,
        }
    }
    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(a) => {
                self.undo.push(std::mem::replace(&mut self.asm, a));
                self.steps += 1;
                self.revision += 1;
                self.sync_variants();
                true
            }
            None => false,
        }
    }

    /// Closes the part files nothing can name any more: no component uses them, and no step of
    /// undo or redo brings back one that does (or undo would bring back components with nothing
    /// behind them). One with unsaved changes stays until it is saved.
    pub fn close_unused(&mut self) {
        let shown = std::iter::once(&self.asm).chain(self.preview.as_deref());
        let named: std::collections::BTreeSet<&str> =
            shown.chain(&self.undo).chain(&self.redo).flat_map(|a| a.components.iter()).map(|c| c.part.as_str()).collect();
        self.parts.retain(|k, p| named.contains(crate::model::key_file(k)) || p.session.is_dirty());
    }

    /// Works out the parts that are a part file in one row of its design table (see
    /// [`sync_variants`]). Called after every change here; call it after changing a part's
    /// document directly. True when anything changed.
    pub fn sync_variants(&mut self) -> bool {
        let shown: Vec<&Assembly> = std::iter::once(&self.asm).chain(self.preview.as_deref()).collect();
        sync_variants(&shown, &mut self.parts)
    }

    /// Adds a part file (if it is not already there).
    pub fn add_part(&mut self, key: &str, part: Part) {
        self.parts.entry(key.to_owned()).or_insert(part);
        self.sync_variants();
    }

    /// Sets the geometry of a part, made from document revision `revision`.
    pub fn set_scene(&mut self, key: &str, revision: u64, scene: Arc<Scene>) {
        if let Some(p) = self.parts.get_mut(key) {
            p.scene = Some((revision, scene));
        }
    }

    /// Regenerates every part whose geometry is out of date, with `k`.
    pub fn refresh(&mut self, k: &mut dyn Kernel) -> Result<(), String> {
        self.refresh_as(k, false)
    }

    /// Like [`AsmSession::refresh`], and gives the parts their meshes for drawing from far
    /// away ([`tenon_model::far_meshes`]): for an assembly that is shown.
    pub fn refresh_shown(&mut self, k: &mut dyn Kernel) -> Result<(), String> {
        self.refresh_as(k, true)
    }

    fn refresh_as(&mut self, k: &mut dyn Kernel, shown: bool) -> Result<(), String> {
        self.sync_variants();
        for (key, p) in self.parts.iter_mut() {
            if p.is_current() {
                // (Made ready before it was to be shown: only the far meshes are still to make.)
                if shown && p.missing.is_none() && p.scene.as_ref().is_some_and(|(_, s)| !s.far) {
                    let regen = p.session.regen(k).clone();
                    if let Some((_, s)) = p.scene.as_mut() {
                        tenon_model::far_meshes(Arc::make_mut(s), &regen, k);
                    }
                }
                continue;
            }
            let rev = p.session.revision();
            // A part with a failing feature still shows what it has (its status says what fails).
            let regen = p.session.regen(k).clone();
            let mut s = tenon_model::scene(&regen, k, &MeshTol::default()).map_err(|e| format!("{key}: {e}"))?;
            if shown {
                tenon_model::far_meshes(&mut s, &regen, k);
            }
            p.scene = Some((rev, Arc::new(s)));
        }
        Ok(())
    }

    /// Re-solves after parts changed (their geometry moved), as one undo step when anything moved.
    pub fn update(&mut self) -> Result<Solved, CmdError> {
        let mut solved = Solved::default();
        self.edit(|asm, parts| {
            solved = solve_assembly(asm, parts, None);
            Ok(())
        })?;
        self.failing = solved.failing.clone();
        Ok(solved)
    }

    /// The component at `id`.
    pub fn component(&self, id: ComponentId) -> Result<&Component, CmdError> {
        self.asm.component(id).ok_or_else(|| CmdError(format!("{id} does not exist")))
    }

    /// The relationship at `id`.
    pub fn relationship(&self, id: RelationshipId) -> Result<&Relationship, CmdError> {
        self.asm.relationship(id).ok_or_else(|| CmdError(format!("{id} does not exist")))
    }

    /// The part session of a component, to edit in place.
    pub fn part_of(&mut self, id: ComponentId) -> Result<&mut Part, CmdError> {
        let key = self.component(id)?.part.clone();
        let p = self.parts.get_mut(&key).ok_or("the component's part is not loaded")?;
        if let Some(why) = &p.missing {
            return Err(CmdError(format!("the part is missing: {why}")));
        }
        Ok(p)
    }
}

/// Places a new component of part `key`: grounded at the origin when it is the first, otherwise
/// beside the others (or at `at`).
pub fn insert(asm: &mut Assembly, parts: &Parts, key: &str, at: Option<Frame>, grounded: Option<bool>) -> ComponentId {
    let first = asm.components.is_empty();
    let base = {
        let file = key.rsplit(['/', '\\']).next().unwrap_or(key);
        file.strip_suffix(".tenon").unwrap_or(file).to_owned()
    };
    let placement = at.unwrap_or_else(|| {
        if first {
            return Frame::WORLD;
        }
        // To the right of what is there, with a gap.
        let all = assembly_bbox(asm, parts);
        let mine = parts.get(key).and_then(|p| p.scene.as_ref()).and_then(|(_, s)| s.bbox());
        match (all, mine) {
            (Some(a), Some(m)) => {
                let gap = (a.diagonal() * 0.1).max(5.0);
                Frame::WORLD.with_origin(Vec3::new(a.max.x + gap - m.min.x, a.center().y - m.center().y, a.min.z - m.min.z)).unwrap_or(Frame::WORLD)
            }
            _ => Frame::WORLD,
        }
    });
    let id = asm.take_component_id();
    let name = asm.occurrence_name(&base);
    asm.components.push(Component {
        id,
        name,
        part: key.to_owned(),
        placement,
        grounded: grounded.unwrap_or(first),
        row: None,
        visible: true,
        mirrored: false,
    });
    id
}

/// Adds a relationship: the component that may move is turned and slid so the new relationship
/// holds, then everything is solved. Fails (leaving `asm` to be restored by the caller) when the
/// relationship cannot hold with the others.
pub fn relate(asm: &mut Assembly, parts: &Parts, mut kind: RelKind) -> Result<RelationshipId, CmdError> {
    let [ta, tb] = kind.targets();
    let (ea, eb) = (resolve_target(asm, parts, ta).map_err(CmdError)?, resolve_target(asm, parts, tb).map_err(CmdError)?);
    if ea.body.is_some() && ea.body == eb.body {
        return Err("pick geometry on two different components".into());
    }
    // An angle measures about the axis the two directions turn about now.
    if let RelKind::Angle { reference, .. } = &mut kind {
        let world = |e: &End| {
            let f = e.body.and_then(|i| asm.components.get(i)).map_or(Frame::WORLD, |c| c.placement);
            (M3::of_frame(&f), e.prim.placed(&M3::of_frame(&f), f.origin()))
        };
        let ((ra, pa), (_, pb)) = (world(&ea), world(&eb));
        if let (Some(ua), Some(ub)) = (pa.direction(), pb.direction()) {
            let c = ua.cross(ub);
            let axis = if c.len() > 1e-9 {
                c.normalized()
            } else {
                let p = if ua.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
                ua.cross(p).normalized()
            };
            *reference = ra.transpose().apply(axis);
        }
    }
    let law = law_for(asm, parts, &kind).map_err(CmdError)?;
    solve::check(&law, &ea.prim, &eb.prim).map_err(CmdError)?;
    // Snap the free side into place first.
    let sys = system(asm, parts);
    let rel = Rel { law, a: ea, b: eb };
    let movable = |e: &End| e.body.filter(|i| sys.bodies.get(*i).is_some_and(|b| !b.fixed));
    let motion = solve::snap(&sys.bodies, &rel);
    // Moving A by the inverse motion gives the same relative placement as moving B.
    let (moving, m) = match (movable(&eb), movable(&ea)) {
        (Some(i), _) => (Some(i), motion),
        (None, Some(i)) => (Some(i), motion.inverse()),
        (None, None) => (None, motion),
    };
    if let Some(c) = moving.and_then(|i| asm.components.get_mut(i)) {
        c.placement = m.apply_frame(&c.placement);
    }
    let id = asm.take_relationship_id();
    let name = asm.relationship_name(kind.label());
    asm.relationships.push(Relationship { id, name: name.clone(), suppressed: false, kind });
    let solved = solve_assembly(asm, parts, None);
    if !solved.converged {
        let why =
            solved.failing.iter().filter(|(r, _)| *r != id).filter_map(|(r, _)| asm.relationship(*r)).map(|r| r.name.clone()).collect::<Vec<_>>();
        return Err(CmdError(if why.is_empty() {
            format!("{name} cannot hold with the components where they are")
        } else {
            format!("{name} conflicts with {}", why.join(", "))
        }));
    }
    Ok(id)
}

/// A target on a component's face, by the face's index in the part's geometry.
pub fn face_target(scene: &Scene, component: ComponentId, body: usize, face: usize) -> Result<Target, String> {
    let (name, info) = scene.bodies.get(body).and_then(|b| b.faces.get(face)).ok_or("no such face")?;
    let origin = name.ok_or("this face cannot be referenced yet")?;
    let face = tenon_model::FaceRef { origin, fingerprint: tenon_model::Fingerprint::of(info) };
    geometry::face_prim(scene, body, geometry::find_face(scene, &face)?.1)?;
    Ok(Target { component: Some(component), geom: Geom::Face { face } })
}

/// A target on a component's edge, by the edge's index in the part's geometry.
pub fn edge_target(scene: &Scene, component: ComponentId, body: usize, edge: usize) -> Result<Target, String> {
    let b = scene.bodies.get(body).ok_or("no such body")?;
    let (names, fp) = b.edges.get(edge).ok_or("no such edge")?;
    let [x, y] = names.ok_or("this edge cannot be referenced yet")?;
    geometry::edge_prim(scene, body, edge)?;
    Ok(Target { component: Some(component), geom: Geom::Edge { edge: tenon_model::EdgeRef::new(x, y, fp.clone()) } })
}

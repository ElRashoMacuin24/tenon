//! An open assembly: the document with undo, its parts (each a part [`Session`] with its own
//! undo, for editing in place) and their geometry, and solving.

use std::collections::BTreeMap;
use std::sync::Arc;

use tenon_geom::{Aabb3, Frame, Vec3, tol};
use tenon_kernel::{Kernel, MeshTol};
use tenon_model::{CmdError, Document, Scene, Session};

use crate::geometry::{self, Prim};
use crate::math::M3;
use crate::model::{Assembly, Component, ComponentId, Geom, RelKind, Relationship, RelationshipId, Target, Tweak};
use crate::solve::{self, Body, BodyDof, End, Law, Rel};

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
    /// Set for a part in one row of its design table: worked out from the part file's own entry,
    /// never saved, and worked out again when that changes.
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

/// What a part in one row of its design table was worked out from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    /// The key of the part file's own entry.
    pub base: String,
    pub row: String,
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

fn body_of(parts: &Parts, c: &Component) -> Body {
    let (center, size) = match local_bbox(parts, c) {
        Some(b) => (c.placement.to_world(b.center()), b.diagonal()),
        None => (c.placement.origin(), 10.0),
    };
    Body { frame: c.placement, fixed: c.grounded, center, size }
}

/// The law of a relationship kind.
pub fn law_of(kind: &RelKind) -> Law {
    match kind {
        RelKind::Mate { offset, .. } => Law::Mate { offset: *offset },
        RelKind::Flush { offset, .. } => Law::Flush { offset: *offset },
        RelKind::Angle { angle, reference, .. } => Law::Angle { angle: *angle, reference: *reference },
        RelKind::Insert { offset, aligned, .. } => Law::Insert { offset: *offset, aligned: *aligned },
        RelKind::Joint { joint, flip, offset, angle, .. } => Law::Joint { joint: *joint, flip: *flip, offset: *offset, angle: *angle },
    }
}

/// The geometry a target means, in its component's part coordinates (assembly coordinates for
/// the assembly's own origin), and that component's index.
pub fn resolve_target(asm: &Assembly, parts: &Parts, t: &Target) -> Result<End, String> {
    match t.component {
        None => Ok(End { body: None, prim: geometry::resolve(&t.geom, None)? }),
        Some(id) => {
            let i = asm.components.iter().position(|c| c.id == id).ok_or_else(|| format!("{id} does not exist"))?;
            let c = &asm.components[i];
            if let Some(why) = parts.get(&c.key()).and_then(|p| p.missing.as_ref()) {
                return Err(format!("{} is missing: {why}", c.name));
            }
            let prim = geometry::resolve(&t.geom, scene_of(parts, c)).map_err(|e| format!("{}: {e}", c.name))?;
            Ok(End { body: Some(i), prim })
        }
    }
}

/// The solver's view of `asm`.
pub fn system(asm: &Assembly, parts: &Parts) -> System {
    let bodies = asm.components.iter().map(|c| body_of(parts, c)).collect();
    let (mut rels, mut ids, mut unresolved) = (Vec::new(), Vec::new(), Vec::new());
    for r in asm.relationships.iter().filter(|r| !r.suppressed) {
        let [a, b] = r.kind.targets();
        let ends = resolve_target(asm, parts, a).and_then(|a| Ok((a, resolve_target(asm, parts, b)?)));
        match ends.and_then(|(a, b)| {
            let law = law_of(&r.kind);
            solve::check(&law, &a.prim, &b.prim)?;
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

/// Solves `asm` in place: free components move so the relationships hold. `drag` is a component
/// the user moved, which stays as near where it was put as it may.
pub fn solve_assembly(asm: &mut Assembly, parts: &Parts, drag: Option<ComponentId>) -> Solved {
    let sys = system(asm, parts);
    let drag = drag.and_then(|d| asm.components.iter().position(|c| c.id == d));
    let sol = solve::solve(&sys.bodies, &sys.rels, drag);
    for (c, f) in asm.components.iter_mut().zip(&sol.frames) {
        if !c.grounded {
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
    Solved { converged: sol.converged, failing }
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
    pub quantity: usize,
    /// What the part is made of, when it has a material.
    pub material: Option<String>,
    /// Of one part (grams), at its material's density, when its geometry is known.
    pub mass: Option<f64>,
    /// Of one part (mm^3), when its geometry is known.
    pub volume: Option<f64>,
    pub components: Vec<String>,
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
    }
    rows
}

/// The parts list from what is known of the parts: `name` gives a part's document name (none:
/// the file name stands in), `volume` a component's part volume. Every parts list in Tenon (the
/// assembly's, a drawing's) is made here, so they number parts alike.
pub fn bom_with(asm: &Assembly, name: impl Fn(&str) -> Option<String>, volume: impl Fn(&Component) -> Option<f64>) -> Vec<BomRow> {
    let mut rows: Vec<BomRow> = Vec::new();
    for c in &asm.components {
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
        rows.push(BomRow {
            item: rows.len() + 1,
            part: stem,
            key,
            name,
            row: c.row.clone(),
            quantity: 1,
            material: None,
            mass: None,
            volume: volume(c),
            components: vec![c.name.clone()],
        });
    }
    rows
}

/// Keeps the parts that are a part file in one row of its design table in step with the
/// assembly and with the part files' own entries: makes those components need, works them out
/// again when their part file's document has changed, and drops those no component uses.
/// Returns true when anything changed.
pub fn sync_variants(asm: &Assembly, parts: &mut Parts) -> bool {
    let wanted: BTreeMap<String, (&str, &str)> =
        asm.components.iter().filter_map(|c| c.row.as_deref().map(|row| (c.key(), (c.part.as_str(), row)))).collect();
    let before = parts.len();
    parts.retain(|k, p| p.variant.is_none() || wanted.contains_key(k));
    let mut changed = parts.len() != before;
    for (key, (base, row)) in wanted {
        let base_revision = parts.get(base).filter(|b| b.missing.is_none()).map(|b| b.session.revision());
        let variant = Variant { base: base.to_owned(), row: row.to_owned(), base_revision };
        if parts.get(&key).is_some_and(|p| p.variant.as_ref() == Some(&variant)) {
            continue;
        }
        // The part file's document, put at the row.
        let made: Result<Document, String> = match parts.get(base) {
            None => Err("its part file is not loaded".into()),
            Some(b) => match &b.missing {
                Some(why) => Err(why.clone()),
                None => {
                    let mut doc = b.session.document().clone();
                    let file = base.rsplit(['/', '\\']).next().unwrap_or(base);
                    match doc.table_activate(row).and_then(|()| doc.sync_parameters()) {
                        Ok(()) => Ok(doc),
                        Err(e) => Err(format!("{file} cannot be had at row `{row}`: {e}")),
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
    redo: Vec<Assembly>,
    revision: u64,
    saved_revision: u64,
    pub parts: Parts,
    /// Relationships that failed in the last solve, and why.
    pub failing: Vec<(RelationshipId, String)>,
}

impl Default for AsmSession {
    fn default() -> Self {
        AsmSession::new(Assembly::default())
    }
}

impl AsmSession {
    pub fn new(asm: Assembly) -> AsmSession {
        AsmSession { asm, undo: Vec::new(), redo: Vec::new(), revision: 1, saved_revision: 1, parts: Parts::new(), failing: Vec::new() }
    }
    pub fn assembly(&self) -> &Assembly {
        &self.asm
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
        self.redo.clear();
        self.revision += 1;
        self.saved_revision = self.revision;
        self.failing.clear();
        self.sync_variants();
    }

    /// Applies `f` as one undoable step; on error nothing changes.
    pub fn edit<T>(&mut self, f: impl FnOnce(&mut Assembly, &Parts) -> Result<T, CmdError>) -> Result<T, CmdError> {
        let before = self.asm.clone();
        match f(&mut self.asm, &self.parts).and_then(|v| self.asm.validate().map(|()| v).map_err(CmdError)) {
            Ok(v) => {
                if self.asm != before {
                    self.undo.push(before);
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

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(a) => {
                self.redo.push(std::mem::replace(&mut self.asm, a));
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
                self.revision += 1;
                self.sync_variants();
                true
            }
            None => false,
        }
    }

    /// Works out the parts that are a part file in one row of its design table (see
    /// [`sync_variants`]). Called after every change here; call it after changing a part's
    /// document directly. True when anything changed.
    pub fn sync_variants(&mut self) -> bool {
        sync_variants(&self.asm, &mut self.parts)
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
        self.sync_variants();
        for (key, p) in self.parts.iter_mut() {
            if p.is_current() {
                continue;
            }
            let rev = p.session.revision();
            // A part with a failing feature still shows what it has (its status says what fails).
            let regen = p.session.regen(k).clone();
            let s = tenon_model::scene(&regen, k, &MeshTol::default()).map_err(|e| format!("{key}: {e}"))?;
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
    asm.components.push(Component { id, name, part: key.to_owned(), placement, grounded: grounded.unwrap_or(first), row: None, visible: true });
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
    let law = law_of(&kind);
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

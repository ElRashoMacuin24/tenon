//! The assembly's Pattern group: Pattern (rows along one or two directions, or round an axis),
//! Mirror (reflected in a plane or flat face) and Copy (independent copies), each a tool of the
//! assembly panel (DEC-130). What they need is picked in the view; while the panel is open the view
//! shows the assembly as it would leave it.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use egui::{Color32, Rect, RichText, Ui, vec2};
use serde_json::{Value, json};
use tenon_assembly::session::{self as asm_session, copy_components, copy_offset, mirror_kind, solve_assembly, world_prim};
use tenon_assembly::{Assembly, ComponentId, Pattern, PatternId, PatternKind, Prim, Run, Target};
use tenon_geom::Vec3;
use tenon_render::pick::{pick_edge_in, pick_face_in};

use crate::asm_panel::{AsmPanel, AsmTool};
use crate::assembly::AsmDoc;
use crate::panels::Panel;
use crate::properties::{eq_field, section};
use crate::theme::Tokens;
use crate::viewport::{HOVER, Pick};
use crate::workbench::Workbench;

/// What a click in the view fills, in these tools: the components.
pub(crate) const SLOT_COMPONENTS: usize = 0;
/// The first direction, the axis, or the plane.
pub(crate) const SLOT_FIRST: usize = 1;
/// The second direction.
pub(crate) const SLOT_SECOND: usize = 2;
/// Copy: where the copies go, following the pointer until a click puts them down.
pub(crate) const SLOT_WHERE: usize = 3;
/// Nothing: clicks in the view do nothing until a selection is chosen again.
pub(crate) const SLOT_NONE: usize = 4;

/// The rule a panel describes, once what it needs is picked.
fn panel_rule(p: &AsmPanel, asm: &Assembly, parts: &asm_session::Parts) -> Result<Option<PatternKind>, String> {
    let Some((first, _)) = &p.a else { return Ok(None) };
    let first = first.clone();
    let whole = |n: f64| n.round().clamp(0.0, f64::from(u32::MAX)) as u32;
    Ok(Some(match (p.tool, p.circular) {
        (AsmTool::Mirror, _) => mirror_kind(asm, parts, first, &p.reuse)?,
        (_, true) => PatternKind::Circular { axis: first, reverse: p.reverse, count: whole(p.count), angle: p.degrees.to_radians() },
        (_, false) => PatternKind::Rectangular {
            first: Run { along: first, reverse: p.reverse, count: whole(p.count), spacing: p.spacing },
            second: p.b.as_ref().map(|(along, _)| Run { along: along.clone(), reverse: p.reverse2, count: whole(p.count2), spacing: p.spacing2 }),
        },
    }))
}

/// The assembly as the panel would leave it: none until what it needs is picked.
fn as_left(p: &AsmPanel, a: &AsmDoc) -> Result<Option<Assembly>, String> {
    if p.components.is_empty() {
        return Ok(None);
    }
    let parts = &a.session.parts;
    let mut asm = a.session.assembly().clone();
    if p.tool == AsmTool::Copy {
        let by = Vec3::new(p.by[0], p.by[1], p.by[2]);
        copy_components(&mut asm, &p.components, by, &BTreeMap::new()).map_err(|e| e.0)?;
    } else {
        let Some(kind) = panel_rule(p, &asm, parts)? else { return Ok(None) };
        match p.pattern {
            Some(id) => asm_session::change_pattern(&mut asm, parts, id, p.components.clone(), kind).map_err(|e| e.0)?,
            None => {
                asm_session::add_pattern(&mut asm, parts, p.components.clone(), kind).map_err(|e| e.0)?;
            }
        }
    }
    if !solve_assembly(&mut asm, parts, None).converged {
        return Err("the relationships cannot all hold with this".into());
    }
    asm.validate()?;
    Ok(Some(asm))
}

/// Whether geometry can do what a pick is asked for, and if not, what to pick instead.
fn fits(p: &AsmPanel, prim: &Prim) -> Result<(), &'static str> {
    match (p.tool, p.circular, prim) {
        (AsmTool::Mirror, _, Prim::Plane { .. }) => Ok(()),
        (AsmTool::Mirror, ..) => Err("Mirror: pick a flat face, or a plane under Origin in the browser."),
        (_, true, Prim::Line { .. } | Prim::Circle { .. }) => Ok(()),
        (_, true, _) => Err("Pattern: pick a round face, a circular edge or a straight edge for the axis."),
        (_, false, Prim::Point(_)) => Err("Pattern: pick a straight edge, a flat face or an axis for the direction."),
        (_, false, _) => Ok(()),
    }
}

impl AsmPanel {
    /// Pattern, Mirror or Copy.
    pub(crate) fn is_repeat(&self) -> bool {
        matches!(self.tool, AsmTool::Pattern | AsmTool::Mirror | AsmTool::Copy)
    }

    /// What the view's preview depends on, for these tools.
    fn repeat_key(&self, revision: u64) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}{:?}{:?}{:?}{:?}", self.tool, self.components, self.reuse, self.a.as_ref().map(|x| &x.0), self.b.as_ref().map(|x| &x.0)).hash(&mut h);
        (self.circular, self.reverse, self.reverse2, self.pattern.map(|p| p.0), revision).hash(&mut h);
        for n in [self.count, self.spacing, self.count2, self.spacing2, self.degrees, self.by[0], self.by[1], self.by[2]] {
            n.to_bits().hash(&mut h);
        }
        h.finish()
    }

    /// Clicked in the view or chosen in the browser: a component in or out of the selection.
    pub(crate) fn toggle_component(&mut self, c: ComponentId, add_only: bool) {
        if let Some(i) = self.components.iter().position(|x| *x == c) {
            if !add_only {
                self.components.remove(i);
                self.reuse.retain(|x| *x != c);
            }
        } else {
            self.components.push(c);
        }
    }
}

/// What a browser row says of a pattern: "Pattern:1 (3 x 2)".
pub(crate) fn pattern_label(p: &Pattern) -> String {
    match &p.kind {
        PatternKind::Rectangular { first, second: Some(s) } => format!("{} ({} x {})", p.name, first.count, s.count),
        PatternKind::Rectangular { first, second: None } => format!("{} ({})", p.name, first.count),
        PatternKind::Circular { count, .. } => format!("{} ({})", p.name, count),
        PatternKind::Mirror { .. } => p.name.clone(),
    }
}

impl Workbench {
    /// Opens Pattern, Mirror or Copy on what is selected.
    pub(crate) fn open_repeat_panel(&mut self, tool: AsmTool) {
        let mut p = AsmPanel::new(tool);
        p.components = self.asm.as_ref().map_or_else(Vec::new, |a| a.selected.iter().copied().filter(|c| a.session.assembly().component(*c).is_some()).collect());
        p.slot = match (p.components.is_empty(), tool) {
            (true, _) => SLOT_COMPONENTS,
            (false, AsmTool::Copy) => SLOT_NONE,
            (false, _) => SLOT_FIRST,
        };
        self.panel = Some(Panel::Asm(Box::new(p)));
        self.view.hover = None;
        self.set_status(match tool {
            AsmTool::Mirror => "Mirror: click the components, then the flat face or plane to mirror them in.",
            AsmTool::Copy => "Copy: click the components. The view shows the copies; type how far, or put them down with a click.",
            _ => "Pattern: click the components, then an edge, face or axis to repeat them along or round.",
        });
    }

    /// Opens a pattern or mirror's panel, to change it.
    pub(crate) fn edit_pattern(&mut self, id: PatternId) -> Result<(), String> {
        let a = self.asm.as_ref().ok_or("no assembly")?;
        let r = a.session.assembly().pattern(id).ok_or_else(|| format!("{id} does not exist"))?.clone();
        let mut p = AsmPanel::new(if matches!(r.kind, PatternKind::Mirror { .. }) { AsmTool::Mirror } else { AsmTool::Pattern });
        p.pattern = Some(id);
        p.components.clone_from(&r.sources);
        p.slot = SLOT_NONE;
        match &r.kind {
            PatternKind::Rectangular { first, second } => {
                p.a = Some(self.labelled(&first.along));
                (p.reverse, p.count, p.spacing) = (first.reverse, f64::from(first.count), first.spacing);
                if let Some(s) = second {
                    p.b = Some(self.labelled(&s.along));
                    (p.reverse2, p.count2, p.spacing2) = (s.reverse, f64::from(s.count), s.spacing);
                }
            }
            PatternKind::Circular { axis, reverse, count, angle } => {
                p.circular = true;
                p.a = Some(self.labelled(axis));
                (p.reverse, p.count, p.degrees) = (*reverse, f64::from(*count), angle.to_degrees());
            }
            PatternKind::Mirror { plane, reuse } => {
                p.a = Some(self.labelled(plane));
                p.reuse = reuse.iter().map(|x| x.component).collect();
            }
        }
        self.panel = Some(Panel::Asm(Box::new(p)));
        self.set_status(format!("Editing {}: change it, then OK.", r.name));
        Ok(())
    }

    /// Keeps the view showing the assembly as an open Pattern, Mirror or Copy panel would leave
    /// it, and takes that away when no such panel is open. True when one is open.
    pub(crate) fn repeat_preview(&mut self) -> bool {
        let Some(a) = self.asm.as_mut() else { return false };
        let Some(Panel::Asm(p)) = self.panel.as_mut().filter(|x| matches!(x, Panel::Asm(p) if p.is_repeat())) else {
            if a.session.preview().is_some() {
                a.session.set_preview(None);
                a.shown = None;
            }
            return false;
        };
        // Copy goes beside what is copied until it is told otherwise.
        if p.tool == AsmTool::Copy && p.by_auto {
            let by = copy_offset(a.session.assembly(), &a.session.parts, &p.components);
            p.by = [by.x, by.y, by.z];
        }
        let key = p.repeat_key(a.session.revision());
        if p.preview.as_ref().is_some_and(|(k, _)| *k == key) {
            return true;
        }
        let left = as_left(p, a);
        p.preview = Some((key, left.as_ref().map(|_| BTreeMap::new()).map_err(Clone::clone)));
        a.session.set_preview(left.unwrap_or_default());
        a.shown = None;
        a.dof = None;
        true
    }

    /// The bodies a preview adds, to tell them from what is there.
    pub(crate) fn repeat_highlights(&self) -> Vec<(Pick, Color32)> {
        let Some(a) = self.asm.as_ref() else { return Vec::new() };
        if a.session.preview().is_none() {
            return Vec::new();
        }
        let asm = a.session.assembly();
        let mut out = Vec::new();
        for (i, (c, _)) in a.map.iter().enumerate() {
            if asm.component(*c).is_none() {
                let faces = self.scene.bodies.get(i).map_or(0, |b| b.faces.len());
                out.extend((0..faces).filter_map(|f| u32::try_from(f).ok()).map(|face| (Pick::Face { body: i, face }, HOVER)));
            }
        }
        out
    }

    /// The pointer over the view with one of these panels open.
    pub(crate) fn repeat_pointer(&mut self, resp: &egui::Response, rect: Rect) {
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let Some(Panel::Asm(p)) = &self.panel else { return };
        let slot = p.slot;
        if slot == SLOT_WHERE {
            self.view.hover = None;
            self.copies_follow(resp, rect);
            return;
        }
        if slot == SLOT_NONE {
            self.view.hover = None;
            return;
        }
        // (Only what is there: what the preview adds is not yet anything to pick.)
        let is_there = |wb: &Self, body: usize| {
            wb.component_of_body(body).is_some_and(|c| wb.asm.as_ref().is_some_and(|a| a.session.assembly().component(c).is_some()))
        };
        let (meshes, tree) = (self.scene.placed(), self.body_tree());
        let hover = resp.hover_pos().and_then(|q| {
            let (x, y) = (f64::from(q.x - rect.left()), f64::from(q.y - rect.top()));
            if slot != SLOT_COMPONENTS
                && let Some(e) = pick_edge_in(tree.as_deref(), &meshes, &self.view.camera, w, h, x, y, 5.0)
                && is_there(self, e.body)
            {
                return Some(Pick::Edge { body: e.body, edge: e.edge });
            }
            let (o, d) = self.view.camera.ray(x, y, w, h);
            pick_face_in(tree.as_deref(), &meshes, o, d).filter(|f| is_there(self, f.body)).map(|f| Pick::Face { body: f.body, face: f.face })
        });
        drop(meshes);
        self.view.hover = hover;
        if !resp.clicked() {
            return;
        }
        let Some(pick) = hover else { return };
        if slot == SLOT_COMPONENTS {
            let body = match pick {
                Pick::Face { body, .. } | Pick::Edge { body, .. } => body,
            };
            if let (Some(c), Some(Panel::Asm(p))) = (self.component_of_body(body), &mut self.panel) {
                p.toggle_component(c, false);
            }
            return;
        }
        match self.target_of_pick(pick) {
            Ok((target, text)) => self.put_repeat_pick(target, text),
            Err(e) => self.set_error(e),
        }
    }

    /// A pick, from the view or the browser's Origin, into the selection being filled.
    pub(crate) fn put_repeat_pick(&mut self, target: Target, text: String) {
        let Some(Panel::Asm(p)) = &self.panel else { return };
        let told = self.asm.as_ref().ok_or_else(String::new).and_then(|a| world_prim(a.session.assembly(), &a.session.parts, &target));
        let wrong = match &told {
            Ok(prim) => fits(p, prim).err().map(str::to_owned),
            Err(e) => Some(e.clone()),
        };
        if let Some(e) = wrong {
            self.set_error(e);
            return;
        }
        if let Some(Panel::Asm(p)) = &mut self.panel {
            p.put((target, text));
        }
    }

    /// Copy: the copies follow the pointer, the middle of what is copied under it in the view's
    /// plane through that middle (as Place, DEC-107), until a click puts them down.
    fn copies_follow(&mut self, resp: &egui::Response, rect: Rect) {
        let Some(pointer) = resp.hover_pos().or_else(|| resp.interact_pointer_pos()) else { return };
        let cam = self.view.camera;
        let (o, d) = cam.ray(f64::from(pointer.x - rect.left()), f64::from(pointer.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
        let Some(a) = self.asm.as_ref() else { return };
        let Some(Panel::Asm(p)) = &self.panel else { return };
        let asm = a.session.assembly();
        let middle = p
            .components
            .iter()
            .filter_map(|c| asm.component(*c))
            .filter_map(|c| asm_session::local_bbox(&a.session.parts, c).map(|b| asm_session::placed_bbox(&b, &c.placement)))
            .reduce(|x, y| x.union(&y))
            .map(|b| b.center());
        let Some(middle) = middle else { return };
        let (n, across) = (cam.forward(), d.dot(cam.forward()));
        if across.abs() < 1e-9 {
            return;
        }
        let by = o + d * ((middle - o).dot(n) / across) - middle;
        if let Some(Panel::Asm(p)) = &mut self.panel {
            p.by = [by.x, by.y, by.z];
            p.by_auto = false;
            if resp.clicked() {
                p.slot = SLOT_NONE;
            }
        }
    }

    /// The controls of these panels.
    pub(crate) fn repeat_panel_ui(&self, ui: &mut Ui, p: &mut AsmPanel, t: &Tokens) {
        let names = |c: &ComponentId| self.asm.as_ref().map_or_else(String::new, |a| a.name(*c));
        let slot_button = |ui: &mut Ui, label: &str, key: &str, active: bool, text: &str| -> bool {
            let mut clicked = false;
            ui.horizontal(|ui| {
                ui.label(label);
                let b = egui::Button::new(RichText::new(text).color(if active { t.accent_text } else { t.text })).fill(if active { t.accent } else { t.field });
                let r = ui.add(b.min_size(vec2(170.0, 22.0))).on_hover_text("Click, then pick in the view");
                crate::drawing::remember(ui, key, r.rect);
                clicked = r.clicked();
            });
            clicked
        };
        section(ui, "Components", true, |ui| {
            let text = if p.components.is_empty() { "click the components".to_string() } else { p.components.iter().map(names).collect::<Vec<_>>().join(", ") };
            if slot_button(ui, "", "tn_repeat_components", p.slot == SLOT_COMPONENTS, &text) {
                p.slot = SLOT_COMPONENTS;
            }
            if !p.components.is_empty() && ui.small_button("Clear").clicked() {
                p.components.clear();
                p.reuse.clear();
                p.slot = SLOT_COMPONENTS;
            }
        });
        let value = |ui: &mut Ui, label: &str, field: &str, value: &mut f64, unit: &str, range: std::ops::RangeInclusive<f64>| {
            ui.label(label);
            eq_field(ui, egui::Id::new(field), value, None, unit, range, 90.0, t);
            ui.end_row();
        };
        match p.tool {
            AsmTool::Pattern => {
                // (A pattern is rows or round an axis: not, once made, the other.)
                if p.pattern.is_none() {
                    ui.horizontal(|ui| {
                        for (circular, label, key) in [(false, "Rectangular", "tn_pattern_rectangular"), (true, "Circular", "tn_pattern_circular")] {
                            let r = ui.add(egui::Button::selectable(p.circular == circular, label));
                            crate::drawing::remember(ui, key, r.rect);
                            if r.clicked() && p.circular != circular {
                                p.circular = circular;
                                p.a = None;
                                p.b = None;
                                if p.slot == SLOT_SECOND {
                                    p.slot = SLOT_FIRST;
                                }
                            }
                        }
                    });
                }
                let pick = |p: &Option<(Target, String)>, hint: &str| p.as_ref().map_or_else(|| hint.to_string(), |(_, s)| s.clone());
                if p.circular {
                    section(ui, "Round an axis", true, |ui| {
                        if slot_button(ui, "Axis", "tn_pattern_first", p.slot == SLOT_FIRST, &pick(&p.a, "click a round face or an edge")) {
                            p.slot = SLOT_FIRST;
                        }
                        egui::Grid::new("tn_pattern_round").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                            value(ui, "Count", "tn_pattern_count", &mut p.count, "", 2.0..=5000.0);
                            value(ui, "Angle", "tn_pattern_angle", &mut p.degrees, "deg", 0.001..=360.0);
                        });
                        let r = ui.checkbox(&mut p.reverse, "the other way round");
                        crate::drawing::remember(ui, "tn_pattern_reverse", r.rect);
                        ui.label(RichText::new("They spread evenly over the angle: 360 is all round.").color(t.text_dim));
                    });
                } else {
                    section(ui, "Direction 1", true, |ui| {
                        if slot_button(ui, "Along", "tn_pattern_first", p.slot == SLOT_FIRST, &pick(&p.a, "click an edge, face or axis")) {
                            p.slot = SLOT_FIRST;
                        }
                        egui::Grid::new("tn_pattern_dir1").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                            value(ui, "Count", "tn_pattern_count", &mut p.count, "", 1.0..=5000.0);
                            value(ui, "Spacing", "tn_pattern_spacing", &mut p.spacing, "mm", 0.001..=100_000.0);
                        });
                        let r = ui.checkbox(&mut p.reverse, "the other way");
                        crate::drawing::remember(ui, "tn_pattern_reverse", r.rect);
                    });
                    section(ui, "Direction 2", true, |ui| {
                        if slot_button(ui, "Along", "tn_pattern_second", p.slot == SLOT_SECOND, &pick(&p.b, "none: click an edge, face or axis")) {
                            p.slot = SLOT_SECOND;
                        }
                        if p.b.is_some() {
                            egui::Grid::new("tn_pattern_dir2").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                value(ui, "Count", "tn_pattern_count2", &mut p.count2, "", 1.0..=5000.0);
                                value(ui, "Spacing", "tn_pattern_spacing2", &mut p.spacing2, "mm", 0.001..=100_000.0);
                            });
                            ui.checkbox(&mut p.reverse2, "the other way");
                            let r = ui.small_button("No second direction");
                            crate::drawing::remember(ui, "tn_pattern_no_second", r.rect);
                            if r.clicked() {
                                p.b = None;
                            }
                        }
                    });
                }
            }
            AsmTool::Mirror => {
                section(ui, "Mirror in", true, |ui| {
                    let text = p.a.as_ref().map_or_else(|| "click a flat face or a plane".to_string(), |(_, s)| s.clone());
                    if slot_button(ui, "Plane", "tn_pattern_first", p.slot == SLOT_FIRST, &text) {
                        p.slot = SLOT_FIRST;
                    }
                });
                section(ui, "Each one is", true, |ui| {
                    egui::Grid::new("tn_mirror_how").num_columns(3).spacing([10.0, 4.0]).show(ui, |ui| {
                        for c in p.components.clone() {
                            ui.label(names(&c));
                            let again = p.reuse.contains(&c);
                            let a = ui.add(egui::Button::selectable(!again, "Mirrored")).on_hover_text("A mirrored occurrence, holding its part reflected: a row of its own in the parts list");
                            let b = ui.add(egui::Button::selectable(again, "Placed again")).on_hover_text("For a part that is the same as its mirror image: the same part again in the mirrored position");
                            crate::drawing::remember(ui, &format!("tn_mirror_reuse_{}", c.0), b.rect);
                            if a.clicked() {
                                p.reuse.retain(|x| *x != c);
                            }
                            if b.clicked() && !again {
                                p.reuse.push(c);
                            }
                            ui.end_row();
                        }
                    });
                });
            }
            AsmTool::Copy => {
                section(ui, "Where", true, |ui| {
                    if slot_button(ui, "", "tn_copy_where", p.slot == SLOT_WHERE, "click to put them down") {
                        p.slot = SLOT_WHERE;
                    }
                    egui::Grid::new("tn_copy_by").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        let before = p.by;
                        for (i, (label, field)) in [("X", "tn_copy_x"), ("Y", "tn_copy_y"), ("Z", "tn_copy_z")].into_iter().enumerate() {
                            value(ui, label, field, &mut p.by[i], "mm", -100_000.0..=100_000.0);
                        }
                        if p.by != before {
                            p.by_auto = false;
                        }
                    });
                });
                section(ui, "Each copy is", true, |ui| {
                    let (a, b) = (
                        ui.add(egui::Button::selectable(!p.new_files, "The same part")).on_hover_text("Further occurrences of the same part files"),
                        ui.add(egui::Button::selectable(p.new_files, "A new part file")).on_hover_text("A part file of its own for each one copied, saved beside the assembly"),
                    );
                    crate::drawing::remember(ui, "tn_copy_new_files", b.rect);
                    if a.clicked() {
                        p.new_files = false;
                    }
                    if b.clicked() {
                        p.new_files = true;
                    }
                });
            }
            _ => {}
        }
        match &p.preview {
            Some((_, Err(e))) => {
                ui.label(RichText::new(e).color(Color32::from_rgb(0xd8, 0x44, 0x38)));
            }
            Some((_, Ok(_))) if !p.components.is_empty() && (p.a.is_some() || p.tool == AsmTool::Copy) => {
                ui.label(RichText::new("The view shows what this makes.").color(t.text_dim));
            }
            _ => {}
        }
    }

    /// OK on one of these panels: makes or changes it. True when done.
    pub(crate) fn commit_repeat_panel(&mut self, p: &AsmPanel) -> bool {
        match self.run_repeat_panel(p) {
            Ok(note) => {
                if let Some(a) = self.asm.as_mut() {
                    a.session.set_preview(None);
                    a.shown = None;
                }
                self.set_status(note);
                true
            }
            Err(e) => {
                self.set_error(e);
                false
            }
        }
    }

    fn run_repeat_panel(&mut self, p: &AsmPanel) -> Result<String, String> {
        if p.components.is_empty() {
            return Err("Click at least one component first.".into());
        }
        let ids: Vec<u32> = p.components.iter().map(|c| c.0).collect();
        if p.tool == AsmTool::Copy {
            let mut params = json!({ "components": ids, "by": p.by, "new_files": p.new_files });
            if p.new_files {
                let folder = self.asm.as_ref().and_then(|a| a.path.as_ref()).and_then(|f| f.parent()).map(|f| f.to_string_lossy().into_owned());
                params["folder"] = json!(folder.ok_or("Save the assembly first: the new part files go beside it.")?);
            }
            let r = self.asm_exec("asm.copy", params)?;
            let n = r["components"].as_array().map_or(0, Vec::len);
            let m = r["relationships"].as_array().map_or(0, Vec::len);
            if let Some(a) = self.asm.as_mut() {
                a.selected = r["components"].as_array().into_iter().flatten().filter_map(|v| v.as_u64()).filter_map(|v| u32::try_from(v).ok()).map(ComponentId).collect();
            }
            return Ok(format!("Copied {n} component(s), with {m} relationship(s) among them. Constrain (C) them to the others, or drag them."));
        }
        let (first, _) = p.a.as_ref().ok_or(if p.tool == AsmTool::Mirror {
            "Mirror: click the flat face or plane to mirror in."
        } else if p.circular {
            "Pattern: click the axis to repeat them round."
        } else {
            "Pattern: click the edge, face or axis to repeat them along."
        })?;
        let whole = |n: f64| n.round().clamp(0.0, f64::from(u32::MAX)) as u32;
        let mut params = if p.tool == AsmTool::Mirror {
            json!({ "components": ids, "plane": first, "reuse": p.reuse.iter().map(|c| c.0).collect::<Vec<_>>() })
        } else if p.circular {
            json!({ "components": ids, "type": "circular", "axis": first, "count": whole(p.count), "angle": p.degrees.to_radians(), "reverse": p.reverse })
        } else {
            let mut v = json!({ "components": ids, "type": "rectangular", "direction": first, "count": whole(p.count), "spacing": p.spacing, "reverse": p.reverse });
            if let Some((second, _)) = &p.b {
                v["direction2"] = json!(second);
                v["count2"] = json!(whole(p.count2));
                v["spacing2"] = json!(p.spacing2);
                v["reverse2"] = json!(p.reverse2);
            }
            v
        };
        let r: Value = match p.pattern {
            Some(id) => {
                params["pattern"] = json!(id.0);
                if p.tool == AsmTool::Pattern && !p.circular && p.b.is_none() {
                    params["direction2"] = Value::Null;
                }
                self.asm_exec("asm.edit_pattern", params)?
            }
            None => self.asm_exec(if p.tool == AsmTool::Mirror { "asm.mirror" } else { "asm.pattern" }, params)?,
        };
        Ok(format!("{}: {} element(s).", r["name"].as_str().unwrap_or(""), r["elements"]))
    }
}

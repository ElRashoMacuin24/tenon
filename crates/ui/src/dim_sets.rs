//! The Baseline and Ordinate tools (DEC-132): a panel that stays open while the datum or origin
//! and the places to measure are clicked in a view, with the set shown where it would go. Making
//! one and changing one are the same panel; every change goes through `drw.baseline`,
//! `drw.ordinate` and `drw.annotation.edit`, as scripts do.

use egui::{Color32, Pos2, Rect};
use serde_json::json;
use tenon_drawing::dimsets;
use tenon_drawing::views::Evaluation;
use tenon_drawing::{AnnotId, AnnotKind, Annotation, Drawing, GeomPick, Owner, SET_SPACING, SetAxis, ViewId};
use tenon_geom::Vec2;

use crate::drawing::{DrwDoc, DrwTool, SheetCam, button, paint_plain, remember};
use crate::workbench::Workbench;

/// What the panel holds: a set being made, or a set being changed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SetPanel {
    /// An ordinate set; else a baseline set.
    pub ordinate: bool,
    /// The set being changed; none while one is being made.
    pub annotation: Option<AnnotId>,
    pub view: Option<ViewId>,
    /// Which way it measures; none: by where it goes.
    pub axis: Option<SetAxis>,
    /// What it is measured from: a baseline's datum, an ordinate's origin.
    pub from: Option<GeomPick>,
    pub targets: Vec<GeomPick>,
    /// The next click on an edge is another datum or origin.
    pub choosing_from: bool,
    /// As typed; a baseline set's.
    pub spacing: String,
    pub precision: u8,
    /// Where it goes on the sheet, once that is said.
    pub at: Option<Vec2>,
}

impl SetPanel {
    pub(crate) fn new(ordinate: bool) -> SetPanel {
        SetPanel {
            ordinate,
            annotation: None,
            view: None,
            axis: None,
            from: None,
            targets: Vec::new(),
            choosing_from: false,
            spacing: format!("{SET_SPACING}"),
            precision: 2,
            at: None,
        }
    }

    fn of(id: AnnotId, kind: &AnnotKind, center: Vec2) -> Option<SetPanel> {
        let set = dimsets::parts(kind)?;
        Some(SetPanel {
            ordinate: set.spacing.is_none(),
            annotation: Some(id),
            view: Some(set.view),
            axis: Some(set.axis),
            from: Some(set.from.clone()),
            targets: set.targets.to_vec(),
            choosing_from: false,
            spacing: set.spacing.map_or_else(|| format!("{SET_SPACING}"), |x| format!("{x}")),
            precision: set.precision,
            at: Some(center + set.offset),
        })
    }

    fn datum_word(&self) -> &'static str {
        if self.ordinate { "Origin" } else { "Datum" }
    }

    /// What to do next.
    pub(crate) fn prompt(&self) -> &'static str {
        match (self.ordinate, self.annotation, &self.from) {
            (false, None, None) => "Baseline: click the datum, an edge or a point (near a line's end: that end).",
            (true, None, None) => "Ordinate: click the origin, an edge or a point (near a line's end: that end).",
            _ if self.choosing_from && self.ordinate => "Click the new origin.",
            _ if self.choosing_from => "Click the new datum.",
            (_, Some(_), _) => "Click edges or points to add or take away, or clear of them to move the set; then OK.",
            _ if self.targets.is_empty() => "Click the edges or points to measure (a second click takes one away).",
            _ => "Click more edges or points, or click clear of them where the set goes. Esc cancels.",
        }
    }

    /// The set as it would be: at `at`, or where it has been put.
    pub(crate) fn annotation_at(&self, d: &Drawing, ev: &Evaluation, at: Option<Vec2>) -> Option<Annotation> {
        let view = self.view?;
        let from = self.from.clone()?;
        if self.targets.is_empty() {
            return None;
        }
        let v = d.view(view)?;
        let at = self.at.or(at)?;
        let axis = match self.axis {
            Some(a) => a,
            None => dimsets::axis_for(d, ev, view, Some(at), &from, &self.targets).ok()?,
        };
        let (targets, offset, precision) = (self.targets.clone(), at - v.center, self.precision);
        let kind = if self.ordinate {
            AnnotKind::Ordinate { view, axis, origin: from, targets, offset, precision }
        } else {
            AnnotKind::Baseline { view, axis, datum: from, targets, offset, spacing: self.spacing_value().unwrap_or(SET_SPACING), precision }
        };
        Some(Annotation { id: self.annotation.unwrap_or(AnnotId(u32::MAX)), kind })
    }

    fn spacing_value(&self) -> Option<f64> {
        self.spacing.trim().parse().ok().filter(|x: &f64| x.is_finite() && *x > 0.0 && *x <= 1000.0)
    }
}

/// One edge and one point of it, whatever its fingerprint was rounded to in a file.
fn same_place(a: &GeomPick, b: &GeomPick) -> bool {
    a.component == b.component
        && a.point == b.point
        && a.edge.faces == b.edge.faces
        && a.edge.fingerprint.mid.dist(b.edge.fingerprint.mid) <= 1e-6
        && (a.edge.fingerprint.length - b.edge.fingerprint.length).abs() <= 1e-6
}

/// Whether the annotation is a set of dimensions.
pub(crate) fn is_set(d: &Drawing, a: AnnotId) -> bool {
    d.annotation(a).is_some_and(|x| dimsets::parts(&x.kind).is_some())
}

/// What the tool shows on the sheet: what has been picked, and the set where it would go.
pub(crate) fn paint_tool(p: &egui::Painter, rect: Rect, cam: &SheetCam, d: &DrwDoc, panel: &SetPanel, at: Option<Vec2>, ghost: Color32) {
    let (dr, Some(ev)) = (d.session.drawing(), d.shown_evaluation()) else { return };
    let Some(view) = panel.view else { return };
    if let Some(a) = panel.annotation_at(dr, ev, at) {
        paint_plain(p, rect, cam, &tenon_drawing::annotate::one(dr, ev, &a), ghost);
    }
    if let Some(q) = panel.from.as_ref().and_then(|pick| dimsets::pick_on_sheet(dr, ev, view, pick)) {
        let c: Pos2 = cam.screen(rect, q);
        p.circle_filled(c, 5.0, ghost);
        p.circle_stroke(c, 8.0, egui::Stroke::new(1.4, ghost));
    }
    for q in panel.targets.iter().filter_map(|pick| dimsets::pick_on_sheet(dr, ev, view, pick)) {
        p.circle_filled(cam.screen(rect, q), 4.0, ghost);
    }
}

impl Workbench {
    /// Opens the panel on a set already made. Clicks then add to it and take from it.
    pub(crate) fn edit_set(&mut self, a: AnnotId) -> Result<(), String> {
        let d = self.drw.as_mut().ok_or("no drawing is open")?;
        let dr = d.session.drawing();
        let note = dr.annotation(a).ok_or("the set is gone")?;
        let view = note.kind.view().and_then(|v| dr.view(v)).ok_or("the view is gone")?;
        let (sheet, panel) = (view.sheet, SetPanel::of(a, &note.kind, view.center).ok_or("that is not a set of dimensions")?);
        let prompt = panel.prompt();
        d.selected = Some(Owner::Annotation(a));
        if sheet != d.sheet {
            d.sheet = sheet;
            d.fitted = false;
        }
        d.tool = Some(DrwTool::Set(Box::new(panel)));
        self.set_status(prompt);
        Ok(())
    }

    /// A click on the sheet with the panel open (`over`: the view under it).
    pub(crate) fn set_click(&mut self, mut panel: SetPanel, at: Vec2, over: Option<ViewId>) -> Result<(), String> {
        // An edge where the click is: picked; elsewhere, the set goes there.
        let picked = match panel.view.or(over) {
            Some(v) if panel.view.is_none() || over == panel.view => match self.drw_exec("drw.pick", json!({ "view": v.0, "at": [at.x, at.y] })) {
                Ok(pick) => {
                    let pick = self.near_a_line_end(v, at, pick)?;
                    Some((v, serde_json::from_value::<GeomPick>(pick).map_err(|e| e.to_string())?))
                }
                Err(_) => None,
            },
            _ => None,
        };
        if let Some((v, pick)) = picked {
            panel.view = Some(v);
            self.set_pick(&mut panel, pick)?;
            return self.keep_panel(panel);
        }
        if panel.annotation.is_some() {
            panel.at = Some(at);
            return self.keep_panel(panel);
        }
        if panel.from.is_none() || panel.targets.is_empty() {
            return Err(format!("click an edge or a point of a view: {}", panel.prompt()));
        }
        panel.at = Some(at);
        self.finish_set(&panel)
    }

    /// A pick while the panel is open: another datum or origin, or a place, added or taken away.
    fn set_pick(&mut self, panel: &mut SetPanel, pick: GeomPick) -> Result<(), String> {
        if panel.from.is_none() || panel.choosing_from {
            if let Some(i) = panel.targets.iter().position(|t| same_place(t, &pick)) {
                panel.targets.remove(i);
            }
            panel.from = Some(pick);
            panel.choosing_from = false;
            return Ok(());
        }
        if panel.from.as_ref().is_some_and(|f| same_place(f, &pick)) {
            return Err(format!("that is the {}: click what to measure from it", panel.datum_word().to_lowercase()));
        }
        if let Some(i) = panel.targets.iter().position(|t| same_place(t, &pick)) {
            panel.targets.remove(i);
            return Ok(());
        }
        // With its direction known, what cannot be measured is said now and not at the end.
        let mut with = panel.clone();
        with.targets.push(pick);
        if with.axis.is_some() {
            with.at = with.at.or(Some(Vec2::new(0.0, 0.0)));
            let d = self.drw.as_ref().ok_or("no drawing")?;
            let ev = d.session.evaluation().ok_or("the views are still being computed; try again in a moment")?;
            if let Some(a) = with.annotation_at(d.session.drawing(), ev, None) {
                dimsets::check(d.session.drawing(), ev, &a)?;
            }
        }
        panel.targets = with.targets;
        Ok(())
    }

    fn keep_panel(&mut self, panel: SetPanel) -> Result<(), String> {
        let prompt = panel.prompt();
        if let Some(d) = self.drw.as_mut() {
            d.tool = Some(DrwTool::Set(Box::new(panel)));
        }
        self.set_status(prompt);
        Ok(())
    }

    /// Makes the set, or changes it; the panel closes and it is selected.
    fn finish_set(&mut self, panel: &SetPanel) -> Result<(), String> {
        let word = if panel.ordinate { "Ordinate" } else { "Baseline" };
        let spacing = panel.spacing_value().ok_or("the spacing is a number of millimetres on the sheet, more than 0 and at most 1000")?;
        let from = panel.from.as_ref().ok_or_else(|| format!("click the {} first", panel.datum_word().to_lowercase()))?;
        if panel.targets.is_empty() {
            return Err("click at least one edge or point to measure".into());
        }
        let picks = |v: &[GeomPick]| serde_json::to_value(v).map_err(|e| e.to_string());
        let from_value = serde_json::to_value(from).map_err(|e| e.to_string())?;
        let r = match panel.annotation {
            None => {
                let view = panel.view.ok_or("click an edge of a view first")?;
                let mut p = json!({ "view": view.0, "targets": picks(&panel.targets)?, "precision": panel.precision });
                p[if panel.ordinate { "origin" } else { "datum" }] = from_value;
                if !panel.ordinate {
                    p["spacing"] = json!(spacing);
                }
                if let Some(a) = panel.axis {
                    p["axis"] = json!(a.id());
                }
                if let Some(at) = panel.at {
                    p["at"] = json!([at.x, at.y]);
                }
                self.drw_exec(if panel.ordinate { "drw.ordinate" } else { "drw.baseline" }, p)?
            }
            Some(id) => {
                let (old, by) = {
                    let d = self.drw.as_ref().ok_or("no drawing")?;
                    let dr = d.session.drawing();
                    let note = dr.annotation(id).ok_or("the set is gone")?;
                    let center = note.kind.view().and_then(|v| dr.view(v)).ok_or("the view is gone")?.center;
                    let old = SetPanel::of(id, &note.kind, center).ok_or("that is not a set of dimensions")?;
                    let by = panel.at.zip(old.at).map(|(new, was)| new - was);
                    (old, by)
                };
                let mut p = json!({ "annotation": id.0, "precision": panel.precision });
                if let Some(by) = by {
                    p["by"] = json!([by.x, by.y]);
                }
                if panel.targets != old.targets {
                    p["remove"] = json!((0..old.targets.len()).collect::<Vec<_>>());
                    p["add"] = picks(&panel.targets)?;
                }
                if panel.from != old.from {
                    p["datum"] = from_value;
                }
                if panel.axis != old.axis {
                    p["axis"] = json!(panel.axis.map(SetAxis::id));
                }
                if !panel.ordinate {
                    p["spacing"] = json!(spacing);
                }
                self.drw_exec("drw.annotation.edit", p)?
            }
        };
        let id = panel.annotation.or_else(|| r["annotation"].as_u64().and_then(|n| u32::try_from(n).ok()).map(AnnotId));
        if let Some(d) = self.drw.as_mut() {
            d.tool = None;
            d.selected = id.map(Owner::Annotation);
        }
        let rows = r["rows"].as_array().map_or(panel.targets.len(), Vec::len);
        self.set_status(format!("{word} set: {rows} place(s). Drag it to move it; double-click it to change it."));
        Ok(())
    }

    /// The panel, while the Baseline or Ordinate tool runs.
    pub(crate) fn set_panel(&mut self, ctx: &egui::Context) {
        let Some(d) = self.drw.as_ref() else { return };
        let Some(DrwTool::Set(panel)) = d.tool.clone() else { return };
        let mut dlg = *panel;
        let asked = dlg.prompt();
        // What each place measures now, for the list.
        let shown: Vec<String> = {
            let ev = d.session.evaluation().or(d.session.last_evaluation());
            let at = dlg.at.or(d.pointer).or(Some(Vec2::new(0.0, 0.0)));
            let rows =
                ev.and_then(|ev| dlg.annotation_at(d.session.drawing(), ev, at).and_then(|a| dimsets::layout(d.session.drawing(), ev, &a).ok()));
            (0..dlg.targets.len())
                .map(|i| {
                    let row = rows.as_ref().and_then(|l| l.rows.iter().find(|r| r.place == Some(i)));
                    row.map_or_else(|| "not found".to_string(), |r| r.shown.clone())
                })
                .collect()
        };
        let title = if dlg.ordinate { "Ordinate Dimensions" } else { "Baseline Dimensions" };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let mut gone = None;
        let corner = [(self.view.rect.right() - 300.0).max(self.view.rect.left()), self.view.rect.top() + 40.0];
        egui::Window::new(title).open(&mut open).collapsible(false).resizable(false).default_width(270.0).default_pos(corner).show(ctx, |ui| {
            egui::Grid::new("tn_set_panel").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(dlg.datum_word());
                ui.horizontal(|ui| {
                    ui.label(if dlg.from.is_some() && !dlg.choosing_from { "picked" } else { "click it in a view" });
                    if dlg.from.is_some() && button(ui, "Change", "tn_set_change").clicked() {
                        dlg.choosing_from = true;
                    }
                });
                ui.end_row();
                ui.label("Direction");
                ui.vertical(|ui| {
                    if dlg.annotation.is_none() {
                        let r = ui.radio_value(&mut dlg.axis, None, "By where it goes");
                        remember(ui, "tn_set_axis_auto", r.rect);
                    }
                    let r = ui.radio_value(&mut dlg.axis, Some(SetAxis::Horizontal), "Horizontal");
                    remember(ui, "tn_set_axis_h", r.rect);
                    let r = ui.radio_value(&mut dlg.axis, Some(SetAxis::Vertical), "Vertical");
                    remember(ui, "tn_set_axis_v", r.rect);
                });
                ui.end_row();
                if !dlg.ordinate {
                    ui.label("Spacing");
                    ui.add(egui::TextEdit::singleline(&mut dlg.spacing).id(egui::Id::new("tn_set_spacing")).desired_width(60.0));
                    ui.end_row();
                }
                ui.label("Decimals");
                let combo = egui::ComboBox::from_id_salt("tn_set_precision").selected_text(dlg.precision.to_string()).show_ui(ui, |ui| {
                    for n in 0..=6u8 {
                        let r = ui.selectable_value(&mut dlg.precision, n, n.to_string());
                        remember(ui, &format!("tn_set_precision_{n}"), r.rect);
                    }
                });
                remember(ui, "tn_set_precision", combo.response.rect);
                ui.end_row();
            });
            ui.add_space(4.0);
            if dlg.targets.is_empty() {
                ui.label("Click what to measure, in the view.");
            } else {
                egui::ScrollArea::vertical().max_height(180.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (i, value) in shown.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}:  {value}", i + 1));
                            if button(ui, "Remove", &format!("tn_set_remove_{i}")).clicked() {
                                gone = Some(i);
                            }
                        });
                    }
                });
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if button(ui, "OK", "tn_set_ok").clicked() {
                    ok = true;
                }
                if button(ui, "Cancel", "tn_set_cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if let Some(i) = gone
            && i < dlg.targets.len()
        {
            dlg.targets.remove(i);
        }
        if let Some(d) = self.drw.as_mut()
            && let Some(DrwTool::Set(p)) = d.tool.as_mut()
        {
            **p = dlg.clone();
        }
        if ok {
            // (Refused, it stays open, to put right.)
            if let Err(e) = self.finish_set(&dlg) {
                self.set_error(e);
            }
        } else if cancel || !open {
            if let Some(d) = self.drw.as_mut() {
                d.tool = None;
            }
            self.set_status("Ready");
        } else if dlg.prompt() != asked {
            self.set_status(dlg.prompt());
        }
    }
}

/// What the browser row of a set says, if the annotation is one.
pub(crate) fn browser_label(a: &Annotation) -> Option<(bool, String)> {
    match &a.kind {
        AnnotKind::Baseline { targets, .. } => Some((false, format!("Baseline {}: {} dimension(s)", a.id.0, targets.len()))),
        AnnotKind::Ordinate { targets, .. } => Some((true, format!("Ordinate {}: {} place(s)", a.id.0, targets.len()))),
        _ => None,
    }
}

/// Whether something of the set cannot be found, for marking its browser row.
pub(crate) fn has_problem(d: &Drawing, ev: Option<&Evaluation>, a: &Annotation) -> bool {
    ev.is_some_and(|ev| dimsets::layout(d, ev, a).map_or(true, |l| l.problem().is_some()))
}

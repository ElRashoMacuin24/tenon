//! The model browser: the part, its bodies, the origin, then the features in history order, with
//! each feature's sketch tucked under it, and the end-of-part marker.

use std::collections::BTreeMap;

use egui::{Align2, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;
use tenon_model::{FeatureId, FeatureKind, FeatureStatus, OriginPlane};

use crate::icons::{self, Icon};
use crate::panels::Panel;
use crate::theme::{self, Tokens};
use crate::workbench::{Mode, Workbench};

pub(crate) const ROW_H: f32 = 21.0;

pub(crate) enum BrowserAction {
    SketchOn(OriginPlane),
    Edit(FeatureId),
    Rename(FeatureId),
    Suppress(FeatureId, bool),
    Delete(FeatureId),
    /// A single click: picks the feature for a pattern being set up.
    Pick(FeatureId),
    /// A single click on an origin plane or axis: picks it for the open panel.
    Reference(crate::work::Reference),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowStyle {
    Normal,
    Active,
    Dim,
    Error,
}

/// One browser row. `expand` draws a +/- box; returns the row response and whether the box was
/// clicked.
fn row(ui: &mut Ui, t: &Tokens, depth: u8, icon: Icon, label: &str, expand: Option<bool>, style: RowStyle) -> (egui::Response, bool) {
    let (rr, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::CLICK);
    if style == RowStyle::Active {
        ui.painter().rect_filled(rr, 0.0, t.pressed);
    } else if resp.hovered() {
        ui.painter().rect_filled(rr, 0.0, t.hover);
    }
    let indent = rr.left() + 6.0 + f32::from(depth) * 16.0;
    let mut toggled = false;
    if let Some(open) = expand {
        let b = Rect::from_center_size(pos2(indent + 5.0, rr.center().y), vec2(9.0, 9.0));
        let p = ui.painter();
        p.rect_stroke(b, 1.0, Stroke::new(1.0, t.text_dim), egui::StrokeKind::Inside);
        p.hline((b.left() + 2.0)..=(b.right() - 2.0), b.center().y, Stroke::new(1.0, t.text));
        if !open {
            p.vline(b.center().x, (b.top() + 2.0)..=(b.bottom() - 2.0), Stroke::new(1.0, t.text));
        }
        toggled = resp.clicked() && resp.interact_pointer_pos().is_some_and(|q| b.expand(4.0).contains(q));
    }
    let ir = Rect::from_min_size(pos2(indent + 14.0, rr.center().y - 8.0), vec2(16.0, 16.0));
    let dim = matches!(style, RowStyle::Dim);
    icons::paint_colored(ui.painter(), ir, icon, if dim { t.icon_disabled } else { t.icon }, t, !dim);
    let color = match style {
        RowStyle::Dim => t.text_disabled,
        RowStyle::Error => t.history_marker,
        _ => t.text,
    };
    ui.painter().text(pos2(ir.right() + 6.0, rr.center().y), Align2::LEFT_CENTER, label, theme::body(), color);
    (resp, toggled)
}

fn header_button(ui: &Ui, at: Pos2, icon: Icon, tip: &str, t: &Tokens, key: &str) -> bool {
    let r = Rect::from_center_size(at, vec2(18.0, 18.0));
    let resp = ui.interact(r, ui.id().with(("browser-head", key)), Sense::CLICK);
    if resp.hovered() {
        ui.painter().rect_filled(r, 2.0, t.hover);
    }
    icons::paint(ui.painter(), r.shrink(3.0), icon, t.icon);
    resp.on_hover_text(tip).clicked()
}

fn feature_icon(kind: &FeatureKind) -> Icon {
    match kind {
        FeatureKind::Sketch { .. } => Icon::NewSketch,
        FeatureKind::Extrude(_) => Icon::Extrude,
        FeatureKind::Revolve(_) => Icon::Revolve,
        FeatureKind::Fillet(_) => Icon::Fillet,
        FeatureKind::Chamfer(_) => Icon::Chamfer,
        FeatureKind::Shell(_) => Icon::Shell,
        FeatureKind::Hole(_) => Icon::Hole,
        FeatureKind::PatternRect(_) => Icon::PatternRect,
        FeatureKind::PatternCircular(_) => Icon::PatternCircular,
        FeatureKind::Mirror(_) => Icon::Mirror,
        FeatureKind::WorkPlane(_) => Icon::Plane,
        FeatureKind::WorkAxis(_) => Icon::Axis,
        FeatureKind::WorkPoint(_) => Icon::Point,
    }
}

impl Workbench {
    pub(crate) fn browser(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        let header = Rect::from_min_size(r.min, vec2(r.width(), 26.0));
        let p = ui.painter();
        p.rect_filled(header, 0.0, t.panel_header);
        let tab = Rect::from_min_size(header.min, vec2(70.0, header.height()));
        p.rect_filled(tab, 0.0, t.panel);
        p.text(pos2(tab.left() + 8.0, tab.center().y), Align2::LEFT_CENTER, "Model", theme::body(), t.text);
        p.hline(header.x_range(), header.bottom(), Stroke::new(1.0, t.border));
        if header_button(ui, pos2(tab.right() - 12.0, tab.center().y), Icon::Close, "Hide the browser", t, "close") {
            self.chrome.show_browser = false;
        }
        let _ = header_button(ui, pos2(tab.right() + 12.0, tab.center().y), Icon::Plus, "More browsers arrive with assemblies (M3)", t, "plus");
        if header_button(ui, pos2(header.right() - 38.0, header.center().y), Icon::Search, "Find in the browser", t, "search") {
            self.chrome.browser_filter = if self.chrome.browser_filter.is_some() { None } else { Some(String::new()) };
        }
        let menu_at = pos2(header.right() - 14.0, header.center().y);
        if header_button(ui, menu_at, Icon::Menu, "Browser options", t, "menu") {
            self.chrome.browser_menu = if self.chrome.browser_menu.is_some() { None } else { Some(menu_at + vec2(-130.0, 12.0)) };
        }
        let mut expand_all = None;
        if let Some(at) = self.chrome.browser_menu {
            let area = egui::Area::new(egui::Id::new("tn_browser_menu")).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
                egui::Frame::menu(ui.style()).fill(t.panel).show(ui, |ui| {
                    ui.set_min_width(130.0);
                    if ui.button("Expand All").clicked() {
                        expand_all = Some(true);
                    }
                    if ui.button("Collapse All").clicked() {
                        expand_all = Some(false);
                    }
                });
            });
            let outside = ui.input(|i| i.pointer.any_pressed())
                && !area.response.contains_pointer()
                && !ui.rect_contains_pointer(Rect::from_center_size(menu_at, vec2(18.0, 18.0)));
            if expand_all.is_some() || outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.chrome.browser_menu = None;
            }
        }
        ui.add_space(header.height() + 2.0);
        if let Some(filter) = &mut self.chrome.browser_filter {
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                let resp = ui.add(egui::TextEdit::singleline(filter).hint_text("Find").desired_width(ui.available_width() - 10.0));
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    filter.clear();
                }
            });
        }
        let filter = self.chrome.browser_filter.clone().unwrap_or_default().to_lowercase();

        // Each sketch sits under the first feature that uses it.
        let mut owner: BTreeMap<FeatureId, FeatureId> = BTreeMap::new();
        for f in self.document().features() {
            if matches!(f.kind, FeatureKind::Sketch { .. }) {
                continue;
            }
            for dep in f.kind.depends_on() {
                if matches!(self.document().feature(dep).map(|d| &d.kind), Some(FeatureKind::Sketch { .. })) {
                    owner.entry(dep).or_insert(f.id);
                }
            }
        }
        if let Some(open) = expand_all {
            self.chrome.origin_open = open;
            self.chrome.bodies_open = open;
            self.chrome.expanded = if open { owner.values().copied().collect() } else { Default::default() };
        }

        let mut action: Option<BrowserAction> = None;
        egui::ScrollArea::vertical().id_salt("tn_browser_scroll").auto_shrink([false, false]).show(ui, |ui| {
            let name = self.document().name.clone();
            row(ui, t, 0, Icon::Part, &name, None, RowStyle::Normal);
            let bodies = self.scene.bodies.len();
            let (resp, toggled) = row(ui, t, 1, Icon::Folder, &format!("Solid Bodies({bodies})"), Some(self.chrome.bodies_open), RowStyle::Normal);
            if toggled || resp.double_clicked() {
                self.chrome.bodies_open = !self.chrome.bodies_open;
            }
            if self.chrome.bodies_open {
                for i in 0..bodies {
                    row(ui, t, 2, Icon::Body, &format!("Solid{}", i + 1), None, RowStyle::Normal);
                }
            }
            let (resp, toggled) = row(ui, t, 1, Icon::Folder, "Origin", Some(self.chrome.origin_open), RowStyle::Normal);
            if toggled || resp.double_clicked() {
                self.chrome.origin_open = !self.chrome.origin_open;
            }
            if self.chrome.origin_open {
                use crate::work::Reference;
                for (icon, label, r) in [
                    (Icon::Plane, "YZ Plane", Some(Reference::Plane(OriginPlane::YZ))),
                    (Icon::Plane, "XZ Plane", Some(Reference::Plane(OriginPlane::XZ))),
                    (Icon::Plane, "XY Plane", Some(Reference::Plane(OriginPlane::XY))),
                    (Icon::Axis, "X Axis", Some(Reference::Axis(tenon_model::OriginAxis::X))),
                    (Icon::Axis, "Y Axis", Some(Reference::Axis(tenon_model::OriginAxis::Y))),
                    (Icon::Axis, "Z Axis", Some(Reference::Axis(tenon_model::OriginAxis::Z))),
                    (Icon::Point, "Center Point", None),
                ] {
                    let (resp, _) = row(ui, t, 2, icon, label, None, RowStyle::Normal);
                    match r {
                        Some(Reference::Plane(p)) => {
                            let resp = resp.on_hover_text("Click while starting a sketch, or double-click, to sketch on this plane");
                            if resp.double_clicked() || (resp.clicked() && self.pick_plane) {
                                action = Some(BrowserAction::SketchOn(p));
                            } else if resp.clicked() {
                                action = Some(BrowserAction::Reference(Reference::Plane(p)));
                            }
                        }
                        Some(r) if resp.clicked() => action = Some(BrowserAction::Reference(r)),
                        _ => {}
                    }
                }
            }
            let editing = match &self.mode {
                Mode::Sketch(s) => Some(s.feature),
                Mode::Model => None,
            };
            let features: Vec<(FeatureId, String, Icon, bool, bool)> = self
                .document()
                .features()
                .iter()
                .map(|f| (f.id, f.name.clone(), feature_icon(&f.kind), f.suppressed, matches!(f.kind, FeatureKind::Sketch { .. })))
                .collect();
            for (id, name, icon, suppressed, is_sketch) in &features {
                if *is_sketch && owner.contains_key(id) {
                    continue; // shown under its feature
                }
                let kids: Vec<_> = features.iter().filter(|c| owner.get(&c.0) == Some(id)).collect();
                let visible = filter.is_empty() || name.to_lowercase().contains(&filter) || kids.iter().any(|k| k.1.to_lowercase().contains(&filter));
                if !visible {
                    continue;
                }
                let open = self.chrome.expanded.contains(id) || !filter.is_empty();
                let mut draw = |ui: &mut Ui, id: FeatureId, name: &str, icon: Icon, suppressed: bool, depth: u8, expand: Option<bool>| {
                    let status = self.scene.status.iter().find(|(f, _)| *f == id).map(|(_, s)| s.clone());
                    let style = match (&status, suppressed, editing == Some(id)) {
                        (_, _, true) => RowStyle::Active,
                        (_, true, _) => RowStyle::Dim,
                        (Some(FeatureStatus::Error { .. }), _, _) => RowStyle::Error,
                        (Some(FeatureStatus::NotComputed), _, _) => RowStyle::Dim,
                        _ => RowStyle::Normal,
                    };
                    let (mut resp, toggled) = row(ui, t, depth, icon, name, expand, style);
                    if let Some(FeatureStatus::Error { message }) = &status {
                        resp = resp.on_hover_text(message.clone());
                    }
                    if toggled {
                        if !self.chrome.expanded.remove(&id) {
                            self.chrome.expanded.insert(id);
                        }
                    } else if resp.double_clicked() {
                        action = Some(BrowserAction::Edit(id));
                    } else if resp.clicked() {
                        action = Some(BrowserAction::Pick(id));
                    }
                    let sketch_child = features.iter().find(|c| owner.get(&c.0) == Some(&id)).map(|c| c.0);
                    resp.context_menu(|ui| {
                        if ui.button("Edit Feature").clicked() {
                            action = Some(BrowserAction::Edit(id));
                            ui.close();
                        }
                        if let Some(s) = sketch_child
                            && ui.button("Edit Sketch").clicked()
                        {
                            action = Some(BrowserAction::Edit(s));
                            ui.close();
                        }
                        if ui.button("Rename").clicked() {
                            action = Some(BrowserAction::Rename(id));
                            ui.close();
                        }
                        if ui.button(if suppressed { "Unsuppress Features" } else { "Suppress Features" }).clicked() {
                            action = Some(BrowserAction::Suppress(id, !suppressed));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Delete").clicked() {
                            action = Some(BrowserAction::Delete(id));
                            ui.close();
                        }
                    });
                };
                draw(ui, *id, name, *icon, *suppressed, 1, (!kids.is_empty()).then_some(open));
                if open {
                    for k in kids {
                        draw(ui, k.0, &k.1, k.2, k.3, 2, None);
                    }
                }
            }
            let (marker, _) = row(ui, t, 1, Icon::EndOfPart, "End of Part", None, RowStyle::Normal);
            let _ = marker.on_hover_text("Rollback marker: features below it are not computed. Dragging it arrives later in M2.");
        });
        if let Some(a) = action {
            self.browser_action(a);
        }
    }

    pub(crate) fn browser_action(&mut self, a: BrowserAction) {
        let result = match a {
            BrowserAction::SketchOn(p) => self.create_sketch(json!({ "plane": format!("{p:?}").to_lowercase() })),
            BrowserAction::Edit(id) => self.edit_feature(id),
            BrowserAction::Rename(id) => {
                self.panel = Some(Panel::Rename { feature: id, name: self.feature_name(id) });
                Ok(())
            }
            BrowserAction::Suppress(id, on) => self.exec("feature.suppress", json!({ "feature": id.0, "suppressed": on })).map(|_| ()),
            BrowserAction::Delete(id) => {
                if matches!(&self.mode, Mode::Sketch(s) if s.feature == id) {
                    self.mode = Mode::Model;
                }
                self.exec("feature.delete", json!({ "feature": id.0 })).map(|_| ())
            }
            BrowserAction::Pick(id) => {
                // A work feature goes into a plane or axis selector; others into pattern features.
                let work = self
                    .document()
                    .feature(id)
                    .is_some_and(|f| matches!(f.kind, FeatureKind::WorkPlane(_) | FeatureKind::WorkAxis(_) | FeatureKind::WorkPoint(_)));
                if work {
                    self.pick_reference(crate::work::Reference::Work(id));
                } else if matches!(self.panel, Some(crate::panels::Panel::Pattern(_))) {
                    self.toggle_pattern_feature(id);
                }
                Ok(())
            }
            BrowserAction::Reference(r) => {
                self.pick_reference(r);
                Ok(())
            }
        };
        if let Err(e) = result {
            self.set_error(e);
        }
    }
}

//! The model browser of an assembly: the assembly, its relationships, its origin, then the
//! components in the order they were placed, each with the relationships that hold it.

use egui::{Align2, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;
use tenon_assembly::{ComponentId, Geom, RelationshipId, Target};
use tenon_model::{OriginAxis, OriginPlane};

use crate::icons::{self, Icon};
use crate::panels::Panel;
use crate::theme::{self, Tokens};
use crate::workbench::Workbench;

enum Action {
    Select(ComponentId, bool),
    Edit(ComponentId),
    Ground(ComponentId, bool),
    Visible(ComponentId, bool),
    /// The size a component is: a row of its part's design table, or the part as its file has it.
    Size(ComponentId, Option<String>),
    DeleteComponent(ComponentId),
    SelectRelationship(RelationshipId),
    EditRelationship(RelationshipId),
    Suppress(RelationshipId, bool),
    DeleteRelationship(RelationshipId),
    /// An origin plane or axis of the assembly, for the open panel.
    Origin(Target),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Normal,
    Selected,
    Dim,
    Error,
}

fn row(ui: &mut Ui, t: &Tokens, depth: u8, icon: Icon, label: &str, expand: Option<bool>, style: Style, pin: bool) -> (egui::Response, bool) {
    let h = crate::browser::ROW_H;
    let (rr, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::CLICK);
    if style == Style::Selected {
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
    let dim = style == Style::Dim;
    icons::paint_colored(ui.painter(), ir, icon, if dim { t.icon_disabled } else { t.icon }, t, !dim);
    if pin {
        // Grounded: a small pin on the icon.
        icons::paint(ui.painter(), Rect::from_min_size(ir.right_bottom() + vec2(-6.0, -8.0), vec2(9.0, 9.0)), Icon::Ground, t.accent);
    }
    let color = match style {
        Style::Dim => t.text_disabled,
        Style::Error => t.history_marker,
        _ => t.text,
    };
    ui.painter().text(pos2(ir.right() + 6.0, rr.center().y), Align2::LEFT_CENTER, label, theme::body(), color);
    #[cfg(test)]
    ui.data_mut(|d| d.get_temp_mut_or_default::<Vec<(String, Rect)>>(egui::Id::new("tn_browser_rows")).push((label.to_owned(), rr)));
    (resp, toggled)
}

/// A row of the browser.
#[derive(Clone, Copy)]
enum Line {
    /// The assembly itself.
    Title,
    /// The folder of all relationships.
    Relationships,
    /// A relationship, by its place among the assembly's.
    Relationship(usize),
    /// The folder of the assembly's own planes, axes and centre point.
    Origin,
    /// One of those ([`ORIGIN`]).
    OriginItem(usize),
    /// A component, by its place among the assembly's.
    Component(usize),
}

/// The assembly's own planes, axes and centre point, as the Origin folder lists them.
const ORIGIN: [(Icon, &str, Geom); 7] = [
    (Icon::Plane, "YZ Plane", Geom::Plane { plane: OriginPlane::YZ }),
    (Icon::Plane, "XZ Plane", Geom::Plane { plane: OriginPlane::XZ }),
    (Icon::Plane, "XY Plane", Geom::Plane { plane: OriginPlane::XY }),
    (Icon::Axis, "X Axis", Geom::Axis { axis: OriginAxis::X }),
    (Icon::Axis, "Y Axis", Geom::Axis { axis: OriginAxis::Y }),
    (Icon::Axis, "Z Axis", Geom::Axis { axis: OriginAxis::Z }),
    (Icon::Point, "Center Point", Geom::Origin),
];

fn rel_icon(joint: bool) -> Icon {
    if joint { Icon::Joint } else { Icon::Constrain }
}

impl Workbench {
    pub(crate) fn asm_browser(&mut self, ui: &mut Ui, t: &Tokens) {
        #[cfg(test)]
        ui.data_mut(|d| d.remove::<Vec<(String, Rect)>>(egui::Id::new("tn_browser_rows")));
        let r = ui.max_rect();
        let header = Rect::from_min_size(r.min, vec2(r.width(), 26.0));
        let p = ui.painter();
        p.rect_filled(header, 0.0, t.panel_header);
        let tab = Rect::from_min_size(header.min, vec2(70.0, header.height()));
        p.rect_filled(tab, 0.0, t.panel);
        p.text(pos2(tab.left() + 8.0, tab.center().y), Align2::LEFT_CENTER, "Model", theme::body(), t.text);
        p.hline(header.x_range(), header.bottom(), Stroke::new(1.0, t.border));
        ui.add_space(header.height() + 2.0);
        let Some(a) = self.asm.as_ref() else { return };
        let asm = a.session.assembly();
        let origin_open = self.chrome.origin_open;
        let dof = self.asm_dof().map(|(per, _)| per.iter().map(|d| d.count()).collect::<Vec<_>>()).unwrap_or_default();
        // The relationships that hold each component, found once: not for every component in turn.
        let mut held: std::collections::HashMap<ComponentId, Vec<usize>> = std::collections::HashMap::new();
        for (i, rel) in asm.relationships.iter().enumerate() {
            for c in rel.kind.components() {
                let of = held.entry(c).or_default();
                if of.last() != Some(&i) {
                    of.push(i);
                }
            }
        }
        // Every row there is, top to bottom; only those in sight are laid out and drawn, so that
        // an assembly of thousands of components costs a frame no more than one of a dozen.
        let mut lines: Vec<Line> = vec![Line::Title, Line::Relationships];
        if a.relationships_open {
            lines.extend((0..asm.relationships.len()).map(Line::Relationship));
        }
        lines.push(Line::Origin);
        if origin_open {
            lines.extend((0..ORIGIN.len()).map(Line::OriginItem));
        }
        for (i, c) in asm.components.iter().enumerate() {
            lines.push(Line::Component(i));
            if a.expanded.contains(&c.id) {
                lines.extend(held.get(&c.id).into_iter().flatten().map(|r| Line::Relationship(*r)));
            }
        }
        let mut action: Option<Action> = None;
        let mut toggle_rel = false;
        let mut toggle_origin = false;
        let mut toggle_comp: Option<ComponentId> = None;
        egui::ScrollArea::vertical().id_salt("tn_asm_browser").auto_shrink([false, false]).show_viewport(ui, |ui, sight| {
            let pitch = crate::browser::ROW_H + ui.spacing().item_spacing.y;
            let first = ((sight.min.y / pitch).floor().max(0.0) as usize).min(lines.len());
            let last = ((sight.max.y / pitch).ceil().max(0.0) as usize + 1).min(lines.len());
            ui.add_space(first as f32 * pitch);
            let relationship_row = |ui: &mut Ui, depth: u8, rel: &tenon_assembly::Relationship, action: &mut Option<Action>| {
                let fail = a.session.failing.iter().find(|(id, _)| *id == rel.id).map(|(_, m)| m.clone());
                let style = if rel.suppressed {
                    Style::Dim
                } else if fail.is_some() {
                    Style::Error
                } else {
                    Style::Normal
                };
                let (mut resp, _) = row(ui, t, depth, rel_icon(rel.kind.is_joint()), &rel.name, None, style, false);
                if let Some(m) = fail {
                    resp = resp.on_hover_text(m);
                }
                if resp.double_clicked() {
                    *action = Some(Action::EditRelationship(rel.id));
                } else if resp.clicked() {
                    *action = Some(Action::SelectRelationship(rel.id));
                }
                resp.context_menu(|ui| {
                    if ui.button("Edit").clicked() {
                        *action = Some(Action::EditRelationship(rel.id));
                        ui.close();
                    }
                    if ui.button(if rel.suppressed { "Unsuppress" } else { "Suppress" }).clicked() {
                        *action = Some(Action::Suppress(rel.id, !rel.suppressed));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Delete").clicked() {
                        *action = Some(Action::DeleteRelationship(rel.id));
                        ui.close();
                    }
                });
            };
            for line in lines.get(first..last).unwrap_or(&[]) {
                match *line {
                    Line::Title => {
                        row(ui, t, 0, Icon::Assembly, &asm.name, None, Style::Normal, false);
                    }
                    Line::Relationships => {
                        let n = asm.relationships.len();
                        let (resp, toggled) =
                            row(ui, t, 1, Icon::Folder, &format!("Relationships ({n})"), Some(a.relationships_open), Style::Normal, false);
                        if toggled || resp.double_clicked() {
                            toggle_rel = true;
                        }
                    }
                    Line::Relationship(i) => {
                        if let Some(rel) = asm.relationships.get(i) {
                            relationship_row(ui, 2, rel, &mut action);
                        }
                    }
                    Line::Origin => {
                        let (resp, toggled) = row(ui, t, 1, Icon::Folder, "Origin", Some(origin_open), Style::Normal, false);
                        if toggled || resp.double_clicked() {
                            toggle_origin = true;
                        }
                    }
                    Line::OriginItem(i) => {
                        let Some((icon, label, geom)) = ORIGIN.get(i) else { continue };
                        let (resp, _) = row(ui, t, 2, *icon, label, None, Style::Normal, false);
                        if resp.on_hover_text("Click while constraining to use the assembly's own origin").clicked() {
                            action = Some(Action::Origin(Target { component: None, geom: geom.clone() }));
                        }
                    }
                    Line::Component(i) => {
                        let Some(c) = asm.components.get(i) else { continue };
                        let has = held.get(&c.id).is_some_and(|of| !of.is_empty());
                        let open = a.expanded.contains(&c.id);
                        let missing = a.session.parts.get(&c.key()).and_then(|p| p.missing.clone());
                        let style = if missing.is_some() {
                            Style::Error
                        } else if a.selected.contains(&c.id) {
                            Style::Selected
                        } else if !c.visible {
                            Style::Dim
                        } else {
                            Style::Normal
                        };
                        // A component in a size of its own says which.
                        let label = match &c.row {
                            Some(size) => format!("{} ({size})", c.name),
                            None => c.name.clone(),
                        };
                        let (mut resp, toggled) = row(ui, t, 1, Icon::Part, &label, has.then_some(open), style, c.grounded);
                        resp = resp.on_hover_ui(|ui| {
                            ui.label(match &missing {
                                Some(m) => format!("{} is missing: {m}", c.part),
                                None => {
                                    let d = dof.get(i).copied().unwrap_or(6);
                                    format!("{}\n{}", c.part, if c.grounded { "Grounded".to_string() } else { format!("{d} degrees of freedom") })
                                }
                            });
                        });
                        if toggled {
                            toggle_comp = Some(c.id);
                        } else if resp.double_clicked() {
                            action = Some(Action::Edit(c.id));
                        } else if resp.clicked() {
                            action = Some(Action::Select(c.id, ui.input(|i| i.modifiers.command || i.modifiers.shift)));
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Edit").clicked() {
                                action = Some(Action::Edit(c.id));
                                ui.close();
                            }
                            if ui.button(if c.grounded { "Unground" } else { "Grounded" }).clicked() {
                                action = Some(Action::Ground(c.id, !c.grounded));
                                ui.close();
                            }
                            // The sizes of a part with a design table (the rows of the table): the
                            // one this component is, ticked.
                            let rows = tenon_assembly::cmd::rows_of(&a.session.parts, &c.part);
                            if !rows.is_empty() {
                                ui.menu_button("Size", |ui| {
                                    if ui
                                        .radio(c.row.is_none(), "As the part file")
                                        .on_hover_text("Whichever size the part's own file is at")
                                        .clicked()
                                    {
                                        action = Some(Action::Size(c.id, None));
                                        ui.close();
                                    }
                                    for size in &rows {
                                        if ui.radio(c.row.as_deref() == Some(size.as_str()), size).clicked() {
                                            action = Some(Action::Size(c.id, Some(size.clone())));
                                            ui.close();
                                        }
                                    }
                                });
                            }
                            if ui.button(if c.visible { "Hide" } else { "Show" }).clicked() {
                                action = Some(Action::Visible(c.id, !c.visible));
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Delete").clicked() {
                                action = Some(Action::DeleteComponent(c.id));
                                ui.close();
                            }
                        });
                    }
                }
            }
            ui.add_space((lines.len() - last) as f32 * pitch);
        });
        if toggle_origin {
            self.chrome.origin_open = !self.chrome.origin_open;
        }
        if let Some(a) = self.asm.as_mut() {
            if toggle_rel {
                a.relationships_open = !a.relationships_open;
            }
            if let Some(c) = toggle_comp
                && !a.expanded.remove(&c)
            {
                a.expanded.insert(c);
            }
        }
        if let Some(act) = action {
            self.asm_browser_action(act);
        }
    }

    fn asm_browser_action(&mut self, act: Action) {
        let result: Result<(), String> = match act {
            Action::Select(c, add) => {
                if let Some(a) = self.asm.as_mut() {
                    if add {
                        if let Some(i) = a.selected.iter().position(|x| *x == c) {
                            a.selected.remove(i);
                        } else {
                            a.selected.push(c);
                        }
                    } else {
                        a.selected = vec![c];
                    }
                }
                // A component picked for a tweak.
                if let Some(Panel::Asm(p)) = &mut self.panel
                    && p.tool == crate::asm_panel::AsmTool::Tweak
                    && !p.components.contains(&c)
                {
                    p.components.push(c);
                }
                Ok(())
            }
            Action::Edit(c) => self.edit_in_place(c),
            Action::Ground(c, on) => self.asm_exec("asm.ground", json!({ "component": c.0, "grounded": on })).map(|_| ()),
            Action::Visible(c, on) => self.asm_exec("asm.visible", json!({ "component": c.0, "visible": on })).map(|_| ()),
            Action::Size(c, size) => self.asm_exec("asm.set_row", json!({ "component": c.0, "row": size })).map(|_| ()),
            Action::DeleteComponent(c) => {
                if let Some(a) = self.asm.as_mut() {
                    a.selected.retain(|x| *x != c);
                }
                self.asm_exec("asm.delete", json!({ "component": c.0 })).map(|_| ())
            }
            Action::SelectRelationship(r) => {
                if let Some(a) = self.asm.as_mut() {
                    a.selected = a.session.assembly().relationship(r).map(|x| x.kind.components()).unwrap_or_default();
                }
                Ok(())
            }
            Action::EditRelationship(r) => self.edit_relationship(r),
            Action::Suppress(r, on) => self.asm_exec("asm.suppress", json!({ "relationship": r.0, "suppressed": on })).map(|_| ()),
            Action::DeleteRelationship(r) => self.asm_exec("asm.delete", json!({ "relationship": r.0 })).map(|_| ()),
            Action::Origin(target) => {
                if let Some(Panel::Asm(p)) = &mut self.panel
                    && p.editing.is_none()
                    && p.tool != crate::asm_panel::AsmTool::Tweak
                {
                    let text = format!("Assembly: {:?}", target.geom).replace("Plane { plane: ", "").replace("Axis { axis: ", "").replace(" }", "");
                    if p.slot == 0 {
                        p.a = Some((target, text));
                        p.slot = 1;
                    } else {
                        p.b = Some((target, text));
                    }
                }
                Ok(())
            }
        };
        if let Err(e) = result {
            self.set_error(e);
        }
    }
}

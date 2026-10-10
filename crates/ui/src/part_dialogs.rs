//! Two dialogs about the part as a whole (M6): Properties (what it is made of, its colour, and
//! its mass, volume and centre of gravity) and the Design Table (the sizes it comes in). Every
//! change goes through the command registry.

use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::params_dialog::edit_cell;
use crate::properties::fmt_value;
use crate::theme::{self, Tokens};
use crate::workbench::Workbench;

/// Colours to pick an appearance from.
const SWATCHES: [&str; 12] =
    ["#b8c0c8", "#3c3f44", "#f2f0ea", "#d04030", "#e8952c", "#e6c84a", "#4c9a4f", "#3fa7a0", "#4f8fd8", "#6a5acd", "#c0588f", "#8a6a4a"];

/// A mass in grams as it reads: grams below a kilogram, kilograms from there.
pub(crate) fn mass_text(grams: f64) -> String {
    if grams.abs() >= 1000.0 { format!("{} kg", fmt_value(grams / 1000.0)) } else { format!("{} g", fmt_value(grams)) }
}

/// A new row's name: "Size N", the first that is free.
fn free_row_name(rows: &[Value]) -> String {
    (1..).map(|n| format!("Size {n}")).find(|name| !rows.iter().any(|r| r["name"] == name.as_str())).unwrap_or_else(|| "Size".into())
}

impl Workbench {
    /// The density (g/cm³) and the colour of the part the shown body `i` is of: the open part's,
    /// or in an assembly its component's.
    pub(crate) fn body_part(&self, i: usize) -> (f64, Option<[u8; 3]>) {
        let doc = match self.asm.as_ref().filter(|a| a.editing.is_none()) {
            Some(a) => a
                .map
                .get(i)
                .and_then(|(id, _)| a.shown_assembly().component(*id))
                .and_then(|c| a.session.parts.get(&c.key()))
                .map(|p| p.session.document()),
            None => Some(self.document()),
        };
        doc.map_or((tenon_model::materials::DEFAULT_DENSITY, None), |d| (d.density(), d.color()))
    }

    /// Properties: material, density and appearance, and what follows from them.
    pub(crate) fn part_properties_window(&mut self, ui: &mut Ui, t: &Tokens) {
        if !self.chrome.mass {
            return;
        }
        let mut open = true;
        let material = self.document().material().cloned();
        // In an assembly the window is a summary of what is shown; a part's material is set in the part.
        let part = !self.in_assembly();
        let density = self.document().density();
        // Each shown component's name, material and mass in grams.
        let mut components: Vec<(tenon_assembly::ComponentId, String, String, f64)> = Vec::new();
        if let Some(a) = self.asm.as_ref().filter(|_| !part) {
            for (i, b) in self.scene.bodies.iter().enumerate() {
                let Some((id, _)) = a.map.get(i) else { continue };
                let grams = b.mass.volume * self.body_part(i).0 / 1000.0;
                match components.iter_mut().find(|r| r.0 == *id) {
                    Some(r) => r.3 += grams,
                    None => {
                        let doc = a.shown_assembly().component(*id).and_then(|c| a.session.parts.get(&c.key())).map(|p| p.session.document());
                        let material = doc.and_then(|d| d.material()).map_or("Generic", |m| m.name.as_str());
                        components.push((*id, a.name(*id), material.to_owned(), grams));
                    }
                }
            }
        }
        let appearance = self.document().appearance().map(str::to_owned);
        let shown = self.document().color().map(tenon_model::materials::color_text);
        let mut changes: Vec<(&'static str, Value)> = Vec::new();
        egui::Window::new("Properties").open(&mut open).collapsible(false).resizable(false).default_width(360.0).default_pos([300.0, 170.0]).show(
            ui.ctx(),
            |ui| {
                if part {
                    ui.label(RichText::new("Physical").font(theme::body()).strong());
                    egui::Grid::new("tn_partprops_physical").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label("Material");
                        let current = material.as_ref().map_or("Generic", |m| m.name.as_str());
                        egui::ComboBox::from_id_salt("tn_partprops_material").selected_text(current).width(190.0).show_ui(ui, |ui| {
                            if ui.selectable_label(material.is_none(), "Generic").on_hover_text("No material: 1 g/cm³, the standard colour").clicked()
                            {
                                changes.push(("document.material", json!({ "name": null })));
                            }
                            for (name, d, _) in tenon_model::materials::LIBRARY {
                                let on = material.as_ref().is_some_and(|m| m.name == name);
                                if ui.selectable_label(on, name).on_hover_text(format!("{d} g/cm³")).clicked() {
                                    changes.push(("document.material", json!({ "name": name })));
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("Density");
                        ui.horizontal(|ui| {
                            if let Some(typed) = edit_cell(ui, egui::Id::new("tn_partprops_density"), &fmt_value(density), 70.0, t) {
                                match typed.parse::<f64>() {
                                    // A density of one's own keeps the material's name, or names it Custom.
                                    Ok(d) => changes.push((
                                        "document.material",
                                        json!({ "name": material.as_ref().map_or("Custom", |m| m.name.as_str()), "density": d }),
                                    )),
                                    Err(_) => changes.push(("", json!(format!("`{typed}` is not a density: type a number of g/cm³")))),
                                }
                            }
                            ui.label("g/cm³");
                        });
                        ui.end_row();
                        ui.label("Appearance");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
                            for hex in SWATCHES {
                                let Some([r, g, b]) = tenon_model::materials::parse_color(hex) else { continue };
                                let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::CLICK);
                                ui.painter().rect_filled(rect, 3.0, egui::Color32::from_rgb(r, g, b));
                                let on = appearance.as_deref() == Some(hex);
                                ui.painter().rect_stroke(
                                    rect,
                                    3.0,
                                    egui::Stroke::new(if on { 2.0 } else { 1.0 }, if on { t.accent } else { t.border }),
                                    egui::StrokeKind::Inside,
                                );
                                crate::drawing::remember(ui, &format!("tn_partprops_swatch_{hex}"), rect);
                                if resp.on_hover_text(hex).clicked() {
                                    changes.push(("document.appearance", json!({ "color": hex })));
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("");
                        ui.horizontal(|ui| {
                            // Typed as #rrggbb; empty goes back to the material's colour.
                            let now = appearance.clone().unwrap_or_default();
                            if let Some(typed) = edit_cell(ui, egui::Id::new("tn_partprops_color"), &now, 70.0, t) {
                                changes.push(("document.appearance", json!({ "color": if typed.is_empty() { Value::Null } else { json!(typed) } })));
                            }
                            if appearance.is_some() {
                                if ui.small_button("As Material").on_hover_text("Show the part in its material's colour again").clicked() {
                                    changes.push(("document.appearance", json!({ "color": null })));
                                }
                            } else {
                                let note = match &shown {
                                    Some(c) => format!("the material's colour, {c}"),
                                    None => "the standard colour".to_owned(),
                                };
                                ui.label(RichText::new(note).font(theme::small()).color(t.text_dim));
                            }
                        });
                        ui.end_row();
                    });
                    ui.separator();
                }
                if self.scene.bodies.is_empty() {
                    ui.label(if part { "There is no solid yet." } else { "No component is shown." });
                } else if !part {
                    // The whole assembly, each component at its own part's density.
                    let (mut grams, mut volume, mut area, mut moment) = (0.0, 0.0, 0.0, tenon_geom::Vec3::new(0.0, 0.0, 0.0));
                    for (i, b) in self.scene.bodies.iter().enumerate() {
                        let g = b.mass.volume * self.body_part(i).0 / 1000.0;
                        grams += g;
                        volume += b.mass.volume;
                        area += b.mass.area;
                        moment = moment + b.mass.center_of_mass * g;
                    }
                    let c = if grams > 0.0 { moment * (1.0 / grams) } else { moment };
                    egui::Grid::new("tn_asmprops_whole").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                        ui.label("Mass");
                        ui.label(RichText::new(mass_text(grams)).strong());
                        ui.end_row();
                        ui.label("Volume");
                        ui.label(format!("{} mm³", fmt_value(volume)));
                        ui.end_row();
                        ui.label("Surface area");
                        ui.label(format!("{} mm²", fmt_value(area)));
                        ui.end_row();
                        ui.label("Centre of gravity");
                        ui.label(format!("{}, {}, {} mm", fmt_value(c.x), fmt_value(c.y), fmt_value(c.z)));
                        ui.end_row();
                    });
                    ui.separator();
                    egui::ScrollArea::vertical().id_salt("tn_asmprops_scroll").max_height(180.0).show(ui, |ui| {
                        egui::Grid::new("tn_asmprops_components").num_columns(3).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                            for head in ["Component", "Material", "Mass"] {
                                ui.label(RichText::new(head).font(theme::small()).color(t.text_dim));
                            }
                            ui.end_row();
                            for (_, name, material, g) in &components {
                                ui.label(name);
                                ui.label(material);
                                ui.label(mass_text(*g));
                                ui.end_row();
                            }
                        });
                    });
                    ui.label(
                        RichText::new("Each component at its part's material: open the part (or edit it in place) to set one.")
                            .font(theme::small())
                            .color(t.text_dim),
                    );
                }
                let many = self.scene.bodies.len() > 1;
                let (mut total_mass, mut total_volume) = (0.0, 0.0);
                for (i, b) in self.scene.bodies.iter().enumerate().filter(|_| part) {
                    // (The scene measures at unit density: a cubic centimetre is 1000 mm³.)
                    let grams = b.mass.volume * density / 1000.0;
                    total_mass += grams;
                    total_volume += b.mass.volume;
                    let c = b.mass.center_of_mass;
                    if many {
                        ui.label(RichText::new(format!("Solid{}", i + 1)).font(theme::body()).strong());
                    }
                    egui::Grid::new(("tn_partprops_body", i)).num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                        ui.label("Mass");
                        ui.label(RichText::new(mass_text(grams)).strong());
                        ui.end_row();
                        ui.label("Volume");
                        ui.label(format!("{} mm³", fmt_value(b.mass.volume)));
                        ui.end_row();
                        ui.label("Surface area");
                        ui.label(format!("{} mm²", fmt_value(b.mass.area)));
                        ui.end_row();
                        ui.label("Centre of gravity");
                        ui.label(format!("{}, {}, {} mm", fmt_value(c.x), fmt_value(c.y), fmt_value(c.z)));
                        ui.end_row();
                    });
                    ui.add_space(4.0);
                }
                if many && part {
                    ui.label(RichText::new(format!("All solids: {}, {} mm³", mass_text(total_mass), fmt_value(total_volume))).strong());
                }
            },
        );
        self.chrome.mass = open;
        for (id, p) in changes {
            if id.is_empty() {
                self.set_error(p.as_str().unwrap_or("not a number").to_owned());
            } else {
                let _ = self.exec_status(id, p);
            }
        }
    }

    /// Design Table: the part's sizes, a row each; the ticked row is the one the part is at.
    pub(crate) fn design_table_window(&mut self, ui: &mut Ui, t: &Tokens) {
        if !self.chrome.table {
            return;
        }
        let mut open = true;
        if self.params_list.0 != self.session.revision() {
            self.params_list = (self.session.revision(), self.exec("param.list", json!({})).unwrap_or(Value::Null));
        }
        // Parameters a table can set: plain values, not ones an equation works out.
        let list = self.params_list.1.clone();
        let mut free: Vec<(String, String)> = Vec::new();
        for key in ["user", "model"] {
            for row in list[key].as_array().into_iter().flatten() {
                let plain = if key == "user" {
                    row["equation"].as_str().is_some_and(|e| e.split_whitespace().next().is_some_and(|n| n.parse::<f64>().is_ok()))
                } else {
                    row["equation"].is_null()
                };
                if plain && let Some(name) = row["name"].as_str() {
                    free.push((name.to_owned(), row["of"].as_str().unwrap_or("").to_owned()));
                }
            }
        }
        let table = self.document().table().cloned();
        let mut changes: Vec<(&'static str, Value)> = Vec::new();
        let mut picked = std::mem::take(&mut self.chrome.table_pick);
        egui::Window::new("Design Table")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([520.0, 300.0])
            .default_pos([320.0, 190.0])
            .show(ui.ctx(), |ui| match &table {
                None => {
                    ui.label("A design table gives one part several sizes: a row for each size, a column for each parameter that differs.");
                    ui.add_space(4.0);
                    ui.label(RichText::new("Parameters to put in the table").font(theme::body()).strong());
                    if free.is_empty() {
                        ui.label(RichText::new("The part has no dimensions or parameters yet.").color(t.text_dim));
                    }
                    egui::ScrollArea::vertical().id_salt("tn_table_pick").max_height(200.0).show(ui, |ui| {
                        for (name, of) in &free {
                            let mut on = picked.contains(name);
                            let label = if of.is_empty() { name.clone() } else { format!("{name}   ({of})") };
                            let r = ui.checkbox(&mut on, label);
                            crate::drawing::remember(ui, &format!("tn_table_pick_{name}"), r.rect);
                            if r.changed() {
                                if on {
                                    picked.insert(name.clone());
                                } else {
                                    picked.remove(name);
                                }
                            }
                        }
                    });
                    ui.separator();
                    let ready = !picked.is_empty();
                    if crate::drawing::button(ui, "Create Table", "tn_table_create")
                        .on_hover_text("The part as it stands becomes the first row")
                        .clicked()
                        && ready
                    {
                        // In the order the parameters are listed.
                        let columns: Vec<&String> = free.iter().map(|f| &f.0).filter(|n| picked.contains(*n)).collect();
                        changes.push(("table.create", json!({ "columns": columns, "row": "Size 1" })));
                    }
                }
                Some(tb) => {
                    egui::ScrollArea::both().id_salt("tn_table_scroll").max_height(ui.available_height() - 40.0).show(ui, |ui| {
                        egui::Grid::new("tn_table_grid").num_columns(tb.columns.len() + 3).striped(true).spacing([8.0, 4.0]).show(ui, |ui| {
                            ui.label("");
                            ui.label(RichText::new("Row").font(theme::small()).color(t.text_dim));
                            for c in &tb.columns {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(c).font(theme::small()).strong());
                                    if ui.small_button("✖").on_hover_text("Take this parameter out of the table").clicked() {
                                        changes.push(("table.remove_column", json!({ "name": c })));
                                    }
                                });
                            }
                            ui.label("");
                            ui.end_row();
                            // What each parameter is: "Coil1 pitch" under "d0".
                            ui.label("");
                            ui.label("");
                            for c in &tb.columns {
                                let of = free.iter().find(|f| &f.0 == c).map_or("", |f| f.1.as_str());
                                ui.label(RichText::new(of).font(theme::small()).color(t.text_dim));
                            }
                            ui.label("");
                            ui.end_row();
                            for r in &tb.rows {
                                let active = r.name == tb.active;
                                let radio = ui.radio(active, "").on_hover_text("The part is at the ticked row");
                                crate::drawing::remember(ui, &format!("tn_table_row_{}", r.name), radio.rect);
                                if radio.clicked() && !active {
                                    changes.push(("table.activate", json!({ "row": r.name })));
                                }
                                if let Some(new) = edit_cell(ui, egui::Id::new(("tn_table_name", &r.name)), &r.name, 90.0, t) {
                                    changes.push(("table.rename_row", json!({ "name": r.name, "to": new })));
                                }
                                for (c, v) in tb.columns.iter().zip(&r.values) {
                                    if let Some(new) = edit_cell(ui, egui::Id::new(("tn_table_cell", &r.name, c)), &fmt_value(*v), 64.0, t) {
                                        match new.parse::<f64>() {
                                            Ok(v) => changes.push(("table.set", json!({ "row": r.name, "column": c, "value": v }))),
                                            Err(_) => changes.push(("", json!(format!("`{new}` is not a number")))),
                                        }
                                    }
                                }
                                if ui.add_enabled(!active, egui::Button::new("✖").small()).on_hover_text("Remove this row").clicked() {
                                    changes.push(("table.remove_row", json!({ "name": r.name })));
                                }
                                ui.end_row();
                            }
                        });
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if crate::drawing::button(ui, "Add Row", "tn_table_add_row").on_hover_text("A new size, starting as the active one").clicked()
                        {
                            let rows: Vec<Value> = tb.rows.iter().map(|r| json!({ "name": r.name })).collect();
                            changes.push(("table.add_row", json!({ "name": free_row_name(&rows) })));
                        }
                        let more: Vec<&(String, String)> = free.iter().filter(|f| !tb.columns.contains(&f.0)).collect();
                        egui::ComboBox::from_id_salt("tn_table_add_column").selected_text("Add Parameter").width(130.0).show_ui(ui, |ui| {
                            if more.is_empty() {
                                ui.label(RichText::new("No other plain parameter").color(t.text_dim));
                            }
                            for (name, of) in more {
                                let label = if of.is_empty() { name.clone() } else { format!("{name}   ({of})") };
                                if ui.selectable_label(false, label).clicked() {
                                    changes.push(("table.add_column", json!({ "name": name })));
                                }
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Delete Table").on_hover_text("The part keeps the size it is at").clicked() {
                                changes.push(("table.delete", json!({})));
                            }
                        });
                    });
                    ui.label(
                        RichText::new("Lengths in mm, angles in degrees. The ticked row follows the part: edit a dimension and that size has it.")
                            .font(theme::small())
                            .color(t.text_dim),
                    );
                }
            });
        self.chrome.table = open;
        if changes.iter().any(|c| c.0 == "table.create") {
            picked.clear();
        }
        self.chrome.table_pick = picked;
        for (id, p) in changes {
            if id.is_empty() {
                self.set_error(p.as_str().unwrap_or("not a number").to_owned());
            } else {
                let _ = self.exec_status(id, p);
            }
        }
    }
}

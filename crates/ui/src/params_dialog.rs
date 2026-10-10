//! The Parameters dialog (fx): model parameters (every named dimension and feature value) and
//! user parameters, with their equations, values and comments. Edits go through the registry.

use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::properties::fmt_value;
use crate::theme::{self, Tokens};
use crate::workbench::Workbench;

/// A text cell that commits on Enter or when it loses focus. Returns the new text.
pub(crate) fn edit_cell(ui: &mut Ui, id: egui::Id, current: &str, width: f32, t: &Tokens) -> Option<String> {
    let focused = ui.memory(|m| m.has_focus(id));
    let mut text = if focused { ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.to_owned()) } else { current.to_owned() };
    let r = ui.add(egui::TextEdit::singleline(&mut text).id(id).desired_width(width).font(theme::body()).text_color(t.text));
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        if text.trim() != current {
            return Some(text.trim().to_owned());
        }
    }
    None
}

/// A change asked for in the dialog, run after it is drawn.
enum Change {
    Rename(String, String),
    Equation(String, String),
    Comment(String, String),
    Delete(String),
    Add,
}

const NAME_W: f32 = 90.0;
const OF_W: f32 = 170.0;
const EQ_W: f32 = 150.0;
const COMMENT_W: f32 = 130.0;

impl Workbench {
    pub(crate) fn parameters_window(&mut self, ui: &mut Ui, t: &Tokens) {
        if !self.chrome.params {
            return;
        }
        let mut open = true;
        // Listed again only when the document changes.
        if self.params_list.0 != self.session.revision() {
            self.params_list = (self.session.revision(), self.exec("param.list", json!({})).unwrap_or(Value::Null));
        }
        let list = self.params_list.1.clone();
        let mut changes: Vec<Change> = Vec::new();
        let mut done = false;
        egui::Window::new("Parameters")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([760.0, 420.0])
            // First shown over the viewport, clear of the ribbon and the browser.
            .default_pos([300.0, 170.0])
            .show(ui.ctx(), |ui| {
                let header = |ui: &mut Ui| {
                    for (label, w) in [
                        ("Parameter Name", NAME_W),
                        ("Consumed by", OF_W),
                        ("Unit", 34.0),
                        ("Equation", EQ_W),
                        ("Nominal Value", 90.0),
                        ("Comment", COMMENT_W),
                    ] {
                        ui.add_sized([w, 18.0], egui::Label::new(RichText::new(label).font(theme::small()).color(t.text_dim)));
                    }
                    ui.end_row();
                };
                egui::ScrollArea::vertical().id_salt("tn_params_scroll").max_height(ui.available_height() - 40.0).show(ui, |ui| {
                    for (group, key) in [("Model Parameters", "model"), ("User Parameters", "user")] {
                        egui::CollapsingHeader::new(RichText::new(group).font(theme::body()).strong()).default_open(true).show(ui, |ui| {
                            egui::Grid::new(("tn_params_grid", key)).num_columns(6).striped(true).spacing([6.0, 4.0]).show(ui, |ui| {
                                header(ui);
                                let rows = list[key].as_array().cloned().unwrap_or_default();
                                if rows.is_empty() {
                                    ui.label(
                                        RichText::new(if key == "model" {
                                            "No dimensions or feature values yet."
                                        } else {
                                            "None yet: Add Numeric makes one."
                                        })
                                        .color(t.text_dim),
                                    );
                                    ui.end_row();
                                }
                                for row in rows {
                                    let name = row["name"].as_str().unwrap_or("").to_owned();
                                    if let Some(new) = edit_cell(ui, egui::Id::new(("tn_param_name", &name)), &name, NAME_W, t) {
                                        changes.push(Change::Rename(name.clone(), new));
                                    }
                                    let of = row["of"].as_str().unwrap_or("");
                                    ui.add_sized([OF_W, 18.0], egui::Label::new(RichText::new(of).font(theme::small())).truncate());
                                    ui.label(row["unit"].as_str().unwrap_or(""));
                                    let value = row["value"].as_f64();
                                    // A model value without an equation shows its number as the equation.
                                    let eq = row["equation"].as_str().map(str::to_owned).unwrap_or_else(|| value.map(fmt_value).unwrap_or_default());
                                    if let Some(new) = edit_cell(ui, egui::Id::new(("tn_param_eq", &name)), &eq, EQ_W, t) {
                                        changes.push(Change::Equation(name.clone(), new));
                                    }
                                    ui.label(
                                        value.map_or("?".into(), |v| format!("{:.6}", v).trim_end_matches('0').trim_end_matches('.').to_owned()),
                                    );
                                    ui.horizontal(|ui| {
                                        let comment = row["comment"].as_str().unwrap_or("");
                                        if let Some(new) = edit_cell(ui, egui::Id::new(("tn_param_comment", &name)), comment, COMMENT_W, t) {
                                            changes.push(Change::Comment(name.clone(), new));
                                        }
                                        if key == "user" && ui.small_button("✖").on_hover_text("Delete this parameter").clicked() {
                                            changes.push(Change::Delete(name.clone()));
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                        });
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Add Numeric").on_hover_text("A new user parameter").clicked() {
                        changes.push(Change::Add);
                    }
                    ui.label(
                        RichText::new("Equations use other names, + - * / ^, units (mm, cm, in, deg) and sin, cos, sqrt, min, max...")
                            .font(theme::small())
                            .color(t.text_dim),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Done").clicked() {
                            done = true;
                        }
                    });
                });
            });
        self.chrome.params = open && !done;
        for c in changes {
            let (id, params) = match c {
                Change::Rename(from, to) => ("param.rename", json!({ "name": from, "to": to })),
                Change::Equation(name, eq) => ("param.set", json!({ "name": name, "equation": eq })),
                Change::Comment(name, comment) => ("param.set", json!({ "name": name, "comment": comment })),
                Change::Delete(name) => ("param.delete", json!({ "name": name })),
                Change::Add => {
                    let taken = |n: &str| list["model"].as_array().into_iter().chain(list["user"].as_array()).flatten().any(|r| r["name"] == n);
                    let name = (0..).map(|i| format!("d{i}")).find(|n| !taken(n)).unwrap_or_else(|| "d".into());
                    ("param.add", json!({ "name": name, "equation": "0 mm" }))
                }
            };
            if let Err(e) = self.exec(id, params) {
                self.set_error(e);
            }
        }
    }
}

//! The Standard Parts dialog (M7): fasteners, bearings and sections from the library built into
//! Tenon. In an assembly a part is placed in the size picked; in a part, a new part is started
//! from it. Either way the work is done by commands.

use egui::{RichText, Ui};
use serde_json::json;
use tenon_assembly::ComponentId;

use crate::properties::fmt_value;
use crate::theme::{self, Tokens};
use crate::workbench::Workbench;

/// The folder standard parts are kept in, beside the assembly that uses them.
pub(crate) const STANDARD_FOLDER: &str = "standard";

impl Workbench {
    pub(crate) fn library_window(&mut self, ui: &mut Ui, t: &Tokens) {
        if !self.chrome.library {
            return;
        }
        let families = tenon_model::library::families();
        let (mut part, mut size) = self.chrome.library_pick.clone();
        // Something is always picked: the first part, and a size it comes in.
        let family = families.iter().find(|f| f.id == part).or(families.first());
        let Some(family) = family else { return };
        part = family.id.to_owned();
        let rows = family.rows();
        // A part newly picked starts at the size it is drawn at (a middling one), and the list
        // is brought to it.
        let fresh = !rows.iter().any(|r| r.0 == size);
        // (Asked for over a few frames: a new window is still finding its size in the first.)
        if fresh {
            self.chrome.library_scroll = 4;
            size = default_size(family.id).filter(|d| rows.iter().any(|r| r.0 == *d)).or(rows.first().map(|r| r.0.clone())).unwrap_or_default();
        }
        let bring = self.chrome.library_scroll > 0;
        self.chrome.library_scroll = self.chrome.library_scroll.saturating_sub(1);
        let in_assembly = self.in_assembly();
        let mut open = true;
        let mut act = false;
        egui::Window::new("Standard Parts").open(&mut open).collapsible(false).resizable(false).default_pos([300.0, 150.0]).show(ui.ctx(), |ui| {
            ui.horizontal_top(|ui| {
                // What there is, by kind.
                ui.vertical(|ui| {
                    ui.set_width(190.0);
                    for category in ["Fasteners", "Bearings", "Sections"] {
                        ui.label(RichText::new(category).font(theme::small()).color(t.text_dim));
                        for f in families.iter().filter(|f| f.category == category) {
                            let r = ui.selectable_label(f.id == part, f.name);
                            crate::drawing::remember(ui, &format!("tn_library_part_{}", f.id), r.rect);
                            if r.clicked() {
                                part = f.id.to_owned();
                                size.clear();
                            }
                        }
                        ui.add_space(4.0);
                    }
                });
                // (No separator here: in a row it takes all the height there is, and the window with it.)
                ui.add_space(10.0);
                // The part picked: what it is, and its sizes.
                ui.vertical(|ui| {
                    ui.set_width(300.0);
                    ui.label(RichText::new(family.name).font(theme::heading()).strong());
                    let standard = if family.standard.is_empty() { "Generic shape".to_owned() } else { format!("Sizes to {}", family.standard) };
                    ui.label(RichText::new(standard).font(theme::small()).color(t.text_dim));
                    ui.add(egui::Label::new(RichText::new(family.about).font(theme::small())).wrap());
                    ui.add_space(4.0);
                    ui.label(RichText::new("Size").font(theme::small()).color(t.text_dim));
                    egui::ScrollArea::vertical().id_salt(("tn_library_sizes", family.id)).max_height(190.0).show(ui, |ui| {
                        ui.set_width(280.0);
                        for (name, _) in &rows {
                            let r = ui.selectable_label(*name == size, name);
                            crate::drawing::remember(ui, &format!("tn_library_size_{name}"), r.rect);
                            if bring && *name == size {
                                r.scroll_to_me_animation(Some(egui::Align::Center), egui::style::ScrollAnimation::none());
                            }
                            if r.clicked() {
                                size = name.clone();
                            }
                        }
                    });
                    // What the size picked comes to.
                    if let Some((_, values)) = rows.iter().find(|r| r.0 == size) {
                        let dims: Vec<String> = family.columns.iter().zip(values).map(|(c, v)| format!("{c} {}", fmt_value(*v))).collect();
                        ui.label(RichText::new(dims.join("   ")).font(theme::small()).color(t.text_dim));
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                let (label, key, tip) = if in_assembly {
                    ("Place", "tn_library_place", "Place this size in the assembly. The part's file is kept in a folder beside the assembly")
                } else {
                    ("New Part", "tn_library_new", "Start a new part from this one, at this size")
                };
                if crate::drawing::button(ui, label, key).on_hover_text(tip).clicked() {
                    act = true;
                }
                let note = if in_assembly {
                    "Every size comes from one file; right-click a component to change its size."
                } else {
                    "The new part keeps every size in its design table."
                };
                ui.label(RichText::new(note).font(theme::small()).color(t.text_dim));
            });
        });
        self.chrome.library = open;
        self.chrome.library_pick = (part.clone(), size.clone());
        if act && !size.is_empty() {
            let done = if in_assembly { self.place_standard(&part, &size) } else { self.run_ui("library.new") };
            if let Err(e) = done {
                self.set_error(e);
            }
        }
    }

    /// Places standard part `part` in `size` in the open assembly, beside what is there.
    pub(crate) fn place_standard(&mut self, part: &str, size: &str) -> Result<(), String> {
        let a = self.asm.as_ref().ok_or("no assembly is open")?;
        let folder = a
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|dir| dir.join(STANDARD_FOLDER))
            .ok_or("Save the assembly first: standard parts are kept in a folder named `standard` beside it.")?;
        let at = (!a.session.assembly().components.is_empty())
            .then(|| tenon_assembly::session::assembly_bbox(a.session.assembly(), &a.session.parts))
            .flatten()
            .map(|b| json!([b.max.x + (b.diagonal() * 0.15).max(10.0), b.min.y, b.min.z]));
        let mut p = json!({ "part": part, "size": size, "path": folder.to_string_lossy() });
        if let Some(at) = at {
            p["at"] = at;
        }
        let r = self.asm_exec("asm.insert_standard", p)?;
        let name = tenon_model::library::family(part).map_or(part, |f| f.name);
        self.set_status(format!("Placed {name}, {size}. Constrain (C) or Joint (J) it to the others."));
        if let Some(a) = self.asm.as_mut()
            && let Some(id) = r["component"].as_u64().and_then(|v| u32::try_from(v).ok())
        {
            a.selected = vec![ComponentId(id)];
        }
        Ok(())
    }
}

/// The size a standard part is drawn at, which is where a choice starts.
fn default_size(part: &str) -> Option<String> {
    Some(
        match part {
            "hex-screw-iso-4017" | "socket-screw-iso-4762" => "M8x30",
            "hex-nut-iso-4032" | "washer-iso-7089" => "M8",
            "ball-bearing-iso-15" => "608",
            _ => return None,
        }
        .to_owned(),
    )
}

//! Window chrome: title bar with quick-access tools, ribbon tabs and panels, document tabs, status
//! bar, file menu and small windows (about, mass properties).

use egui::{Align2, Color32, Frame, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::Workbench;
use crate::commands::{FILE_MENU, QUICK_ACCESS, RIBBON, Size, UiCommand};
use crate::icons;
use crate::theme::{self, Tokens};

pub(crate) const TITLE_H: f32 = 30.0;
pub(crate) const TABS_H: f32 = 26.0;
pub(crate) const RIBBON_H: f32 = 96.0;
pub(crate) const DOC_TABS_H: f32 = 26.0;
pub(crate) const STATUS_H: f32 = 24.0;

/// Window-level state.
#[derive(Debug, Clone)]
pub(crate) struct Chrome {
    pub tab: usize,
    pub show_browser: bool,
    pub show_cube: bool,
    pub origin_open: bool,
    pub file_menu: Option<Pos2>,
    pub about: bool,
    pub mass: bool,
}

impl Default for Chrome {
    fn default() -> Self {
        Chrome { tab: 0, show_browser: true, show_cube: true, origin_open: false, file_menu: None, about: false, mass: false }
    }
}

/// Text width in `font`.
pub(crate) fn text_width(ui: &Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter().layout_no_wrap(text.to_owned(), font, Color32::WHITE).size().x
}

pub(crate) fn tooltip(cmd: &UiCommand) -> String {
    if cmd.available() {
        format!("{}  [{}]\n{}", cmd.label, cmd.id, cmd.tip)
    } else {
        format!("{}  [{}]\n{}\nArrives in milestone M{}", cmd.label, cmd.id, cmd.tip, cmd.milestone)
    }
}

/// A ribbon button (large: icon over label; small: icon then label). Returns true when clicked.
fn command_button(ui: &Ui, rect: Rect, cmd: &UiCommand, active: bool, t: &Tokens) -> bool {
    let resp = ui.interact(rect, ui.id().with(cmd.id), Sense::click());
    let p = ui.painter();
    if active || resp.is_pointer_button_down_on() {
        p.rect_filled(rect, 3.0, t.pressed);
    } else if resp.hovered() {
        p.rect_filled(rect, 3.0, t.hover);
    }
    let (icon_color, text_color) = if cmd.available() { (t.icon, t.text) } else { (t.icon_disabled, t.text_disabled) };
    match cmd.size {
        Size::Large => {
            let ir = Rect::from_center_size(pos2(rect.center().x, rect.top() + 20.0), vec2(28.0, 28.0));
            icons::paint(p, ir, cmd.icon, icon_color);
            p.text(pos2(rect.center().x, rect.top() + 40.0), Align2::CENTER_TOP, cmd.label, theme::small(), text_color);
        }
        Size::Small => {
            let ir = Rect::from_min_size(pos2(rect.left() + 4.0, rect.center().y - 8.0), vec2(16.0, 16.0));
            icons::paint(p, ir, cmd.icon, icon_color);
            p.text(pos2(ir.right() + 6.0, rect.center().y), Align2::LEFT_CENTER, cmd.label, theme::small(), text_color);
        }
    }
    resp.on_hover_text(tooltip(cmd)).clicked()
}

/// An icon-only button (quick access, navigation bar). Returns true when clicked.
pub(crate) fn icon_button(ui: &Ui, rect: Rect, cmd: &UiCommand, active: bool, t: &Tokens) -> bool {
    let resp = ui.interact(rect, ui.id().with(cmd.id), Sense::click());
    if active {
        ui.painter().rect_filled(rect, 3.0, t.pressed);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, t.hover);
    }
    icons::paint(ui.painter(), rect.shrink(rect.width() * 0.18), cmd.icon, if cmd.available() { t.icon } else { t.icon_disabled });
    resp.on_hover_text(tooltip(cmd)).clicked()
}

impl Workbench {
    pub(crate) fn title_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        let mark = Rect::from_min_size(pos2(r.left() + 10.0, r.center().y - 7.0), vec2(14.0, 14.0));
        ui.painter().rect_filled(mark, 3.0, t.accent);
        ui.painter().rect_filled(Rect::from_center_size(mark.center(), vec2(4.0, 8.0)), 1.0, t.title_bar);
        let mut x = mark.right() + 14.0;
        let mut clicked = None;
        for cmd in QUICK_ACCESS {
            let br = Rect::from_min_size(pos2(x, r.center().y - 11.0), vec2(22.0, 22.0));
            if icon_button(ui, br, cmd, false, t) {
                clicked = Some(cmd.id);
            }
            x += 25.0;
        }
        let dirty = if self.session.is_dirty() { " *" } else { "" };
        let file = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.session.document().name.clone());
        ui.painter().text(r.center(), Align2::CENTER_CENTER, format!("Tenon   {file}{dirty}"), theme::body(), t.text_dim);
        ui.painter().text(
            pos2(r.right() - 10.0, r.center().y),
            Align2::RIGHT_CENTER,
            concat!("v", env!("CARGO_PKG_VERSION")),
            theme::small(),
            t.text_disabled,
        );
        if let Some(id) = clicked {
            self.command(id);
        }
    }

    pub(crate) fn ribbon_tabs(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        let file = Rect::from_min_size(pos2(r.left() + 6.0, r.top() + 3.0), vec2(52.0, r.height() - 6.0));
        let resp = ui.interact(file, ui.id().with("file"), Sense::click());
        ui.painter().rect_filled(file, 3.0, if resp.hovered() { t.accent.gamma_multiply(1.2) } else { t.accent });
        ui.painter().text(file.center(), Align2::CENTER_CENTER, "File", theme::body(), t.accent_text);
        if resp.clicked() {
            self.chrome.file_menu = if self.chrome.file_menu.is_some() { None } else { Some(file.left_bottom() + vec2(0.0, 2.0)) };
        }
        let mut x = file.right() + 10.0;
        for (i, tab) in RIBBON.iter().enumerate() {
            let w = text_width(ui, tab.name, theme::body()) + 24.0;
            let tr = Rect::from_min_size(pos2(x, r.top() + 2.0), vec2(w, r.height() - 2.0));
            let resp = ui.interact(tr, ui.id().with(("tab", i)), Sense::click());
            let active = i == self.chrome.tab;
            if active {
                ui.painter().rect_filled(tr, egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, t.ribbon);
                ui.painter().hline(tr.x_range(), tr.top() + 1.0, Stroke::new(2.0, t.accent));
            } else if resp.hovered() {
                ui.painter().rect_filled(tr, 3.0, t.hover);
            }
            ui.painter().text(tr.center(), Align2::CENTER_CENTER, tab.name, theme::body(), if active { t.text } else { t.text_dim });
            if resp.clicked() {
                self.chrome.tab = i;
            }
            x += w + 2.0;
        }
    }

    pub(crate) fn ribbon(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        let Some(tab) = RIBBON.get(self.chrome.tab) else {
            return;
        };
        let title_h = 18.0;
        let top = r.top() + 4.0;
        let content_h = r.height() - title_h - 8.0;
        let mut x = r.left() + 8.0;
        let mut clicked = None;
        let active_tool = self.active_tool_id();
        for panel in tab.panels {
            let start = x;
            for cmd in panel.commands.iter().filter(|c| c.size == Size::Large) {
                let w = (text_width(ui, cmd.label, theme::small()) + 14.0).max(50.0);
                if command_button(ui, Rect::from_min_size(pos2(x, top), vec2(w, content_h)), cmd, active_tool == Some(cmd.id), t) {
                    clicked = Some(cmd.id);
                }
                x += w + 2.0;
            }
            let smalls: Vec<&UiCommand> = panel.commands.iter().filter(|c| c.size == Size::Small).collect();
            let row_h = content_h / 3.0;
            for column in smalls.chunks(3) {
                let w = column.iter().map(|c| text_width(ui, c.label, theme::small()) + 34.0).fold(0.0, f32::max);
                for (row, cmd) in column.iter().enumerate() {
                    let br = Rect::from_min_size(pos2(x, top + row as f32 * row_h), vec2(w, row_h - 1.0));
                    if command_button(ui, br, cmd, active_tool == Some(cmd.id), t) {
                        clicked = Some(cmd.id);
                    }
                }
                x += w + 4.0;
            }
            let title = Rect::from_min_max(pos2(start, r.bottom() - title_h - 2.0), pos2(x, r.bottom() - 2.0));
            ui.painter().text(title.center(), Align2::CENTER_CENTER, panel.title, theme::small(), t.text_dim);
            x += 6.0;
            ui.painter().vline(x, (r.top() + 6.0)..=(r.bottom() - 6.0), Stroke::new(1.0, t.border));
            x += 8.0;
        }
        if let Some(id) = clicked {
            self.command(id);
        }
    }

    pub(crate) fn doc_tabs(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        icons::paint(ui.painter(), Rect::from_min_size(pos2(r.left() + 8.0, r.top() + 5.0), vec2(16.0, 16.0)), icons::Icon::Home, t.icon_disabled);
        let label = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{}.tenon", self.session.document().name));
        let w = text_width(ui, &label, theme::small()) + 24.0;
        let tab = Rect::from_min_size(pos2(r.left() + 32.0, r.top()), vec2(w, r.height()));
        ui.painter().rect_filled(tab, 0.0, t.panel);
        ui.painter().hline(tab.x_range(), tab.bottom() - 1.0, Stroke::new(2.0, t.accent));
        ui.painter().text(pos2(tab.left() + 10.0, tab.center().y), Align2::LEFT_CENTER, label, theme::small(), t.text);
    }

    pub(crate) fn status_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        let color = if self.status_error { t.history_marker } else { t.text_dim };
        ui.painter().text(pos2(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, &self.status, theme::small(), color);
        let mut right = vec!["Units: mm".to_owned()];
        if self.waiting {
            right.insert(0, "Regenerating...".into());
        } else if self.scene_seq > 0 {
            right.insert(0, format!("Regenerated in {:.0} ms", self.scene.regen_ms + self.scene.mesh_ms));
        }
        if let Some(d) = self.sketch_dof_text() {
            right.insert(0, d);
        }
        ui.painter().text(pos2(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER, right.join("     "), theme::small(), t.text_dim);
    }

    pub(crate) fn file_menu(&mut self, ui: &mut Ui, t: &Tokens) {
        let Some(at) = self.chrome.file_menu else {
            return;
        };
        let mut chosen = None;
        egui::Area::new(egui::Id::new("tn_file_menu")).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
            Frame::menu(ui.style()).fill(t.panel).show(ui, |ui| {
                ui.set_min_width(190.0);
                for (label, id) in FILE_MENU {
                    if ui.button(*label).clicked() {
                        chosen = Some(*id);
                    }
                }
                ui.separator();
                if ui.button("About Tenon").clicked() {
                    chosen = Some("app.about");
                }
                if ui.button("Exit").clicked() {
                    chosen = Some("app.exit");
                }
            });
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.chrome.file_menu = None;
        }
        if let Some(id) = chosen {
            self.chrome.file_menu = None;
            self.command(id);
        }
    }

    pub(crate) fn windows(&mut self, ui: &mut Ui) {
        let mut about = self.chrome.about;
        egui::Window::new("About Tenon").open(&mut about).collapsible(false).resizable(false).default_width(420.0).show(ui.ctx(), |ui| {
            ui.heading(concat!("Tenon ", env!("CARGO_PKG_VERSION")));
            ui.label("Free, open-source parametric 3D CAD. Milestone M1: sketch to solid.");
            ui.add_space(6.0);
            ui.label("Licence: MIT OR Apache-2.0.");
            ui.label("Geometry kernel: OpenCASCADE Technology 8 (LGPL-2.1 with the Open CASCADE exception), dynamically linked.");
            ui.label("Forked from CADCraft (MIT OR Apache-2.0). See NOTICE.");
            ui.add_space(6.0);
            ui.small("Tenon is not affiliated with Autodesk, Inc. or any other CAD vendor.");
        });
        self.chrome.about = about;

        let mut mass = self.chrome.mass;
        egui::Window::new("Mass Properties").open(&mut mass).collapsible(false).resizable(false).default_width(340.0).show(ui.ctx(), |ui| {
            if self.scene.bodies.is_empty() {
                ui.label("There is no solid yet.");
            }
            for (i, b) in self.scene.bodies.iter().enumerate() {
                let c = b.mass.center_of_mass;
                ui.strong(format!("Body {}", i + 1));
                egui::Grid::new(("mass", i)).num_columns(2).show(ui, |ui| {
                    ui.label("Volume");
                    ui.label(format!("{:.3} mm^3", b.mass.volume));
                    ui.end_row();
                    ui.label("Surface area");
                    ui.label(format!("{:.3} mm^2", b.mass.area));
                    ui.end_row();
                    ui.label("Centre of mass");
                    ui.label(format!("{:.3}, {:.3}, {:.3} mm", c.x, c.y, c.z));
                    ui.end_row();
                });
                ui.add_space(4.0);
            }
            ui.small("Mass equals volume at unit density; materials arrive in M5.");
        });
        self.chrome.mass = mass;
    }
}

/// Every ribbon id with a handler in this milestone (for tests).
#[cfg(test)]
pub(crate) fn available_ids() -> Vec<&'static str> {
    crate::commands::all().filter(|c| c.available()).map(|c| c.id).collect()
}

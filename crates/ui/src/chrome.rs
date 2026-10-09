//! Window chrome: title bar (quick-access tools, title, command search), ribbon tabs and panels,
//! document tabs, status bar, file menu and small windows (about, mass properties, options).

use std::collections::BTreeSet;

use egui::{Align2, Color32, Frame, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use tenon_model::FeatureId;

use crate::Workbench;
use crate::commands::{self, ASM_RIBBON, DRW_RIBBON, FILE_MENU, QUICK_ACCESS, RIBBON, RibbonTab, Size, UiCommand};
use crate::icons::{self, Icon};
use crate::theme::{self, ThemeName, Tokens};

pub(crate) const TITLE_H: f32 = 32.0;
pub(crate) const TABS_H: f32 = 26.0;
pub(crate) const RIBBON_H: f32 = 98.0;
pub(crate) const DOC_TABS_H: f32 = 26.0;
pub(crate) const STATUS_H: f32 = 22.0;

/// Window-level state.
#[derive(Debug, Clone)]
pub(crate) struct Chrome {
    pub tab: usize,
    pub show_browser: bool,
    pub show_cube: bool,
    pub show_navbar: bool,
    pub origin_open: bool,
    pub bodies_open: bool,
    /// Browser rows opened to show their children (a feature's sketch).
    pub expanded: BTreeSet<FeatureId>,
    pub browser_filter: Option<String>,
    pub browser_menu: Option<Pos2>,
    pub file_menu: Option<Pos2>,
    pub about: bool,
    pub mass: bool,
    pub options: bool,
    /// The Parameters dialog is open.
    pub params: bool,
    /// A browser row being dragged.
    pub browser_drag: Option<crate::browser::BrowserDrag>,
    pub radial: Option<crate::radial::Radial>,
    pub theme: ThemeName,
    /// The theme egui's own widgets were last styled with.
    pub applied_theme: Option<ThemeName>,
    pub search: String,
    pub search_pick: usize,
    /// A split button's drop-down: the button's id and where to open the list.
    pub dropdown: Option<(&'static str, Pos2)>,
}

impl Default for Chrome {
    fn default() -> Self {
        Chrome {
            tab: 0,
            show_browser: true,
            show_cube: true,
            show_navbar: true,
            origin_open: false,
            bodies_open: false,
            expanded: BTreeSet::new(),
            browser_filter: None,
            browser_menu: None,
            file_menu: None,
            about: false,
            mass: false,
            options: false,
            params: false,
            browser_drag: None,
            radial: None,
            theme: ThemeName::default(),
            applied_theme: None,
            search: String::new(),
            search_pick: 0,
            dropdown: None,
        }
    }
}

/// Text width in `font`.
pub(crate) fn text_width(ui: &Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter().layout_no_wrap(text.to_owned(), font, Color32::WHITE).size().x
}

pub(crate) fn tooltip(cmd: &UiCommand) -> String {
    let key = cmd.key.map(|k| format!(" ({k})")).unwrap_or_default();
    let mut s = format!("{}{key}\n{}", cmd.name(), cmd.tip);
    if !cmd.available() {
        s.push_str(&format!("\nArrives in {}", cmd.arrives()));
    }
    s
}

/// A small downward triangle (drop-down marker).
fn caret(ui: &Ui, at: Pos2, color: Color32) {
    ui.painter().add(egui::Shape::convex_polygon(vec![at + vec2(-3.0, -1.5), at + vec2(3.0, -1.5), at + vec2(0.0, 2.0)], color, Stroke::NONE));
}

/// What a click on a split button hit.
#[derive(PartialEq)]
enum Hit {
    None,
    Main,
    Arrow(Pos2),
}

/// A ribbon button. Large: icon over one or two label lines; small: icon then label; icon: icon
/// only. Buttons with a drop-down list have an arrow part (below for large, right for small).
fn ribbon_button(ui: &Ui, rect: Rect, cmd: &UiCommand, active: bool, t: &Tokens) -> Hit {
    let has_more = !cmd.more.is_empty();
    let arrow_zone = match cmd.size {
        Size::Large if has_more => Some(Rect::from_min_max(pos2(rect.left(), rect.bottom() - 18.0), rect.max)),
        Size::Small if has_more => Some(Rect::from_min_max(pos2(rect.right() - 13.0, rect.top()), rect.max)),
        _ => None,
    };
    let resp = ui.interact(rect, ui.id().with(cmd.id), Sense::CLICK);
    let p = ui.painter();
    if active || resp.is_pointer_button_down_on() {
        p.rect_filled(rect, 3.0, t.pressed);
    } else if resp.hovered() {
        p.rect_filled(rect, 3.0, t.hover);
        if let Some(z) = arrow_zone {
            p.rect_stroke(rect, 3.0, Stroke::new(1.0, t.pressed), egui::StrokeKind::Inside);
            let line = if cmd.size == Size::Large { [z.left_top(), z.right_top()] } else { [z.left_top(), z.left_bottom()] };
            p.line_segment(line, Stroke::new(1.0, t.pressed));
        }
    }
    let enabled = cmd.available();
    let (icon_color, text_color) = if enabled { (t.icon, t.text) } else { (t.icon_disabled, t.text_disabled) };
    match cmd.size {
        Size::Large => {
            let ir = Rect::from_center_size(pos2(rect.center().x, rect.top() + 19.0), vec2(30.0, 30.0));
            icons::paint_colored(p, ir, cmd.icon, icon_color, t, enabled);
            let lines: Vec<&str> = cmd.label.split('\n').collect();
            for (i, line) in lines.iter().enumerate() {
                let y = rect.top() + 38.0 + i as f32 * 13.0;
                let last = i + 1 == lines.len();
                if last && has_more {
                    let w = text_width(ui, line, theme::small());
                    p.text(pos2(rect.center().x - 5.0, y), Align2::CENTER_TOP, *line, theme::small(), text_color);
                    caret(ui, pos2(rect.center().x + w / 2.0, y + 7.0), text_color);
                } else {
                    p.text(pos2(rect.center().x, y), Align2::CENTER_TOP, *line, theme::small(), text_color);
                }
            }
            if has_more && lines.len() == 1 {
                caret(ui, pos2(rect.center().x, rect.top() + 58.0), text_color);
            }
        }
        Size::Small => {
            let ir = Rect::from_min_size(pos2(rect.left() + 3.0, rect.center().y - 8.0), vec2(16.0, 16.0));
            icons::paint_colored(p, ir, cmd.icon, icon_color, t, enabled);
            p.text(pos2(ir.right() + 5.0, rect.center().y), Align2::LEFT_CENTER, cmd.name(), theme::small(), text_color);
            if has_more {
                caret(ui, pos2(rect.right() - 6.0, rect.center().y), text_color);
            }
        }
        Size::Icon => {
            icons::paint_colored(p, rect.shrink(3.0), cmd.icon, icon_color, t, enabled);
        }
    }
    let resp = resp.on_hover_text(tooltip(cmd));
    if !resp.clicked() {
        return Hit::None;
    }
    match (arrow_zone, resp.interact_pointer_pos()) {
        (Some(z), Some(at)) if z.contains(at) => Hit::Arrow(pos2(rect.left(), rect.bottom() + 2.0)),
        _ => Hit::Main,
    }
}

/// An icon-only button (quick access, navigation bar). Returns true when clicked.
pub(crate) fn icon_button(ui: &Ui, rect: Rect, cmd: &UiCommand, active: bool, t: &Tokens) -> bool {
    let resp = ui.interact(rect, ui.id().with(cmd.id), Sense::CLICK);
    if active {
        ui.painter().rect_filled(rect, 3.0, t.pressed);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, t.hover);
    }
    let enabled = cmd.available();
    icons::paint_colored(ui.painter(), rect.shrink(rect.width() * 0.18), cmd.icon, if enabled { t.icon } else { t.icon_disabled }, t, enabled);
    resp.on_hover_text(tooltip(cmd)).clicked()
}

impl Workbench {
    /// The ribbon of the environment: drawing, assembly, or part (also while a part is edited in
    /// place or from a drawing).
    pub(crate) fn ribbon_def(&self) -> &'static [RibbonTab] {
        if self.in_drawing() {
            DRW_RIBBON
        } else if self.in_assembly() {
            ASM_RIBBON
        } else {
            RIBBON
        }
    }

    /// The Return button at the end of the ribbon: back to the assembly from a part edited in
    /// place, or back to the drawing from its model.
    fn return_command(&self) -> Option<&'static UiCommand> {
        if self.editing_in_place() {
            Some(&commands::RETURN)
        } else if self.editing_from_drawing() {
            Some(&commands::DRW_RETURN)
        } else {
            None
        }
    }

    /// The open document's file name and whether it has unsaved changes.
    fn document_label(&self) -> (String, bool) {
        if let Some(d) = &self.drw {
            let file = d
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map_or_else(|| format!("{}.{}", d.session.drawing().name, tenon_io::drw::EXTENSION), |n| n.to_string_lossy().into_owned());
            let models_dirty = d.session.models.values().any(tenon_drawing::DrwModel::is_dirty);
            return match &d.editing {
                Some(key) => (
                    format!("{file} > {}", tenon_drawing::views::file_name(key)),
                    self.session.is_dirty() || self.asm.as_ref().is_some_and(|a| a.session.is_dirty()),
                ),
                None => (file, d.session.is_dirty() || models_dirty),
            };
        }
        if let Some(a) = &self.asm {
            let file = a
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map_or_else(|| format!("{}.tenonasm", a.session.assembly().name), |n| n.to_string_lossy().into_owned());
            let file = match &a.editing {
                Some(e) => format!("{file} > {}", a.name(e.component)),
                None => file,
            };
            return (file, a.session.is_dirty() || self.session.is_dirty());
        }
        let file = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{}.tenon", self.session.document().name));
        (file, self.session.is_dirty())
    }

    pub(crate) fn title_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        // Tenon's mark: a tenon in its mortise.
        let mark = Rect::from_min_size(pos2(r.left() + 8.0, r.center().y - 10.0), vec2(20.0, 20.0));
        ui.painter().rect_filled(mark, 4.0, t.accent);
        ui.painter().rect_filled(Rect::from_center_size(mark.center() + vec2(0.0, 2.0), vec2(6.0, 12.0)), 1.0, t.title_bar);
        let mut x = mark.right() + 12.0;
        let mut clicked = None;
        for (gi, group) in QUICK_ACCESS.iter().enumerate() {
            if gi > 0 {
                ui.painter().vline(x + 2.0, (r.top() + 8.0)..=(r.bottom() - 8.0), Stroke::new(1.0, t.separator));
                x += 7.0;
            }
            for cmd in *group {
                let br = Rect::from_min_size(pos2(x, r.center().y - 12.0), vec2(24.0, 24.0));
                if icon_button(ui, br, cmd, false, t) {
                    clicked = Some(cmd.id);
                }
                x += 26.0;
            }
        }
        let (file, dirty) = self.document_label();
        let dirty = if dirty { " *" } else { "" };
        ui.painter().text(r.center(), Align2::CENTER_CENTER, format!("Tenon     {file}{dirty}"), theme::body(), t.text_dim);

        // Help, then command search to its left.
        let help = Rect::from_min_size(pos2(r.right() - 32.0, r.center().y - 11.0), vec2(22.0, 22.0));
        let hresp = ui.interact(help, ui.id().with("help"), Sense::CLICK);
        if hresp.hovered() {
            ui.painter().rect_filled(help, 3.0, t.hover);
        }
        ui.painter().circle_stroke(help.center(), 8.0, Stroke::new(1.3, t.icon));
        ui.painter().text(help.center(), Align2::CENTER_CENTER, "?", theme::small(), t.icon);
        if hresp.on_hover_text("About Tenon").clicked() {
            clicked = Some("app.about");
        }
        let search = Rect::from_min_size(pos2(help.left() - 246.0, r.center().y - 11.0), vec2(238.0, 22.0));
        self.command_search(ui, search, t);
        if let Some(id) = clicked {
            self.command(id);
        }
    }

    /// The search box: type part of a command's name; arrows choose, Enter runs.
    fn command_search(&mut self, ui: &mut Ui, rect: Rect, t: &Tokens) {
        let id = egui::Id::new("tn_search");
        let focused = ui.memory(|m| m.has_focus(id));
        let results: Vec<&'static UiCommand> = commands::search(&self.chrome.search).into_iter().take(12).collect();
        if focused && !results.is_empty() {
            let n = results.len();
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
                self.chrome.search_pick = (self.chrome.search_pick + 1) % n;
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
                self.chrome.search_pick = (self.chrome.search_pick + n - 1) % n;
            }
        }
        self.chrome.search_pick = self.chrome.search_pick.min(results.len().saturating_sub(1));
        ui.painter().rect_filled(rect, 3.0, t.field);
        ui.painter().rect_stroke(rect, 3.0, Stroke::new(1.0, if focused { t.accent } else { t.border }), egui::StrokeKind::Inside);
        icons::paint(ui.painter(), Rect::from_min_size(rect.left_top() + vec2(5.0, 4.0), vec2(14.0, 14.0)), Icon::Search, t.text_dim);
        let edit = egui::TextEdit::singleline(&mut self.chrome.search)
            .id(id)
            .hint_text("Search commands...")
            .frame(Frame::NONE)
            .font(theme::body())
            .text_color(t.text)
            .desired_width(rect.width() - 28.0);
        let resp = ui.put(Rect::from_min_max(rect.min + vec2(22.0, 3.0), rect.max - vec2(4.0, 1.0)), edit);
        // Only a click puts the cursor in the search box: Tab is for the value boxes.
        if resp.gained_focus() && !resp.clicked() && !ui.input(|i| i.pointer.any_pressed()) {
            resp.surrender_focus();
        }
        if resp.changed() {
            self.chrome.search_pick = 0;
        }
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let mut run: Option<&'static str> = None;
        if enter {
            run = results.get(self.chrome.search_pick).map(|c| c.id);
        }
        if (focused || resp.has_focus()) && !results.is_empty() {
            egui::Area::new(egui::Id::new("tn_search_results")).order(egui::Order::Foreground).fixed_pos(rect.left_bottom() + vec2(0.0, 2.0)).show(
                ui.ctx(),
                |ui| {
                    Frame::menu(ui.style()).fill(t.panel).show(ui, |ui| {
                        ui.set_width(rect.width() + 60.0);
                        for (i, c) in results.iter().enumerate() {
                            let (row, rresp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::CLICK);
                            if i == self.chrome.search_pick || rresp.hovered() {
                                ui.painter().rect_filled(row, 3.0, t.hover);
                            }
                            let enabled = c.available();
                            icons::paint_colored(
                                ui.painter(),
                                Rect::from_min_size(row.left_top() + vec2(4.0, 8.0), vec2(18.0, 18.0)),
                                c.icon,
                                if enabled { t.icon } else { t.icon_disabled },
                                t,
                                enabled,
                            );
                            let key = c.key.map(|k| format!("   {k}")).unwrap_or_default();
                            ui.painter().text(
                                row.left_top() + vec2(28.0, 3.0),
                                Align2::LEFT_TOP,
                                format!("{}{key}", c.name()),
                                theme::body(),
                                if enabled { t.text } else { t.text_disabled },
                            );
                            let place = commands::location(c.id).unwrap_or_else(|| "Menus and toolbars".into());
                            let place = if enabled { place } else { format!("{place} ({})", c.arrives()) };
                            ui.painter().text(row.left_top() + vec2(28.0, 18.0), Align2::LEFT_TOP, place, theme::small(), t.text_dim);
                            if rresp.clicked() {
                                run = Some(c.id);
                            }
                        }
                    });
                },
            );
        }
        if let Some(id) = run {
            self.chrome.search.clear();
            ui.memory_mut(|m| m.surrender_focus(egui::Id::new("tn_search")));
            self.command(id);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) && focused {
            self.chrome.search.clear();
        }
    }

    pub(crate) fn ribbon_tabs(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        let file = Rect::from_min_size(pos2(r.left() + 6.0, r.top() + 3.0), vec2(48.0, r.height() - 3.0));
        let resp = ui.interact(file, ui.id().with("file"), Sense::CLICK);
        crate::drawing::remember(ui, "tn_file_tab", file);
        ui.painter().rect_filled(
            file,
            egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 },
            if resp.hovered() { t.accent.gamma_multiply(1.15) } else { t.accent },
        );
        ui.painter().text(file.center(), Align2::CENTER_CENTER, "File", theme::body(), t.accent_text);
        if resp.clicked() {
            self.chrome.file_menu = if self.chrome.file_menu.is_some() { None } else { Some(file.left_bottom() + vec2(0.0, 2.0)) };
        }
        let mut x = file.right() + 6.0;
        let sketching = self.is_sketching();
        for (i, tab) in self.ribbon_def().iter().enumerate() {
            let w = text_width(ui, tab.name, theme::body()) + 22.0;
            let tr = Rect::from_min_size(pos2(x, r.top() + 3.0), vec2(w, r.height() - 3.0));
            let resp = ui.interact(tr, ui.id().with(("tab", i)), Sense::CLICK);
            let active = i == self.chrome.tab;
            if active {
                ui.painter().rect_filled(tr, egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, t.ribbon);
                let s = Stroke::new(1.0, t.border);
                ui.painter().line_segment([tr.left_bottom(), tr.left_top()], s);
                ui.painter().line_segment([tr.left_top(), tr.right_top()], s);
                ui.painter().line_segment([tr.right_top(), tr.right_bottom()], s);
            } else if resp.hovered() {
                ui.painter().rect_filled(tr, 3.0, t.hover);
            }
            // While sketching, the Sketch tab is the contextual one.
            let color = if active || (sketching && i == commands::SKETCH_TAB && !self.in_assembly()) { t.text } else { t.text_dim };
            ui.painter().text(tr.center(), Align2::CENTER_CENTER, tab.name, theme::body(), color);
            if resp.clicked() {
                self.chrome.tab = i;
            }
            x += w + 1.0;
        }
        ui.painter().hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, t.border));
    }

    pub(crate) fn ribbon(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        let Some(tab) = self.ribbon_def().get(self.chrome.tab).or_else(|| self.ribbon_def().first()) else {
            return;
        };
        let back = self.return_command();
        let return_panel = back.map(|c| [commands::RibbonPanel { title: "Return", commands: std::slice::from_ref(c) }]);
        let return_panel: &[commands::RibbonPanel] = return_panel.as_ref().map_or(&[], |p| &p[..]);
        let editing = back.is_some();
        let title_h = 17.0;
        let top = r.top() + 4.0;
        let content_h = r.height() - title_h - 8.0;
        let mut x = r.left() + 6.0;
        let mut clicked = None;
        let active_tool = self.active_tool_id();
        let panels: Vec<&commands::RibbonPanel> = tab.panels.iter().chain(if editing { return_panel } else { &[] }).collect();
        for panel in panels {
            let start = x;
            let mut hit = |h: Hit, id: &'static str| match h {
                Hit::Main => clicked = Some((id, None)),
                Hit::Arrow(at) => clicked = Some((id, Some(at))),
                Hit::None => {}
            };
            for cmd in panel.commands.iter().filter(|c| c.size == Size::Large) {
                let w = cmd.label.split('\n').map(|l| text_width(ui, l, theme::small())).fold(0.0, f32::max)
                    + if cmd.more.is_empty() { 12.0 } else { 20.0 };
                let w = w.max(44.0);
                hit(ribbon_button(ui, Rect::from_min_size(pos2(x, top), vec2(w, content_h)), cmd, active_tool == Some(cmd.id), t), cmd.id);
                x += w + 2.0;
            }
            let row_h = content_h / 3.0;
            let smalls: Vec<&UiCommand> = panel.commands.iter().filter(|c| c.size == Size::Small).collect();
            for column in smalls.chunks(3) {
                let w = column
                    .iter()
                    .map(|c| text_width(ui, &c.name(), theme::small()) + if c.more.is_empty() { 30.0 } else { 42.0 })
                    .fold(0.0, f32::max);
                for (row, cmd) in column.iter().enumerate() {
                    let br = Rect::from_min_size(pos2(x, top + row as f32 * row_h), vec2(w, row_h - 1.0));
                    hit(ribbon_button(ui, br, cmd, active_tool == Some(cmd.id), t), cmd.id);
                }
                x += w + 3.0;
            }
            let glyphs: Vec<&UiCommand> = panel.commands.iter().filter(|c| c.size == Size::Icon).collect();
            for column in glyphs.chunks(3) {
                for (row, cmd) in column.iter().enumerate() {
                    let br = Rect::from_min_size(pos2(x, top + row as f32 * row_h + (row_h - 22.0) / 2.0), vec2(22.0, 22.0));
                    hit(ribbon_button(ui, br, cmd, active_tool == Some(cmd.id), t), cmd.id);
                }
                x += 24.0;
            }
            let title = Rect::from_min_max(pos2(start, r.bottom() - title_h - 1.0), pos2(x, r.bottom() - 1.0));
            ui.painter().text(title.center(), Align2::CENTER_CENTER, panel.title, theme::small(), t.text_dim);
            x += 5.0;
            ui.painter().vline(x, (r.top() + 6.0)..=(r.bottom() - 4.0), Stroke::new(1.0, t.separator));
            x += 6.0;
        }
        ui.painter().hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, t.border));
        match clicked {
            Some((id, Some(at))) => self.chrome.dropdown = Some((id, at)),
            Some((id, None)) => self.command(id),
            None => {}
        }
    }

    /// The list under a split button's arrow.
    pub(crate) fn dropdown(&mut self, ui: &Ui, t: &Tokens) {
        let Some((id, at)) = self.chrome.dropdown else { return };
        let Some(cmd) = commands::find(id) else {
            self.chrome.dropdown = None;
            return;
        };
        let mut chosen = None;
        let area = egui::Area::new(egui::Id::new("tn_dropdown")).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
            Frame::menu(ui.style()).fill(t.panel).show(ui, |ui| {
                ui.set_min_width(170.0);
                for m in cmd.more {
                    let Some(c) = commands::find(m) else { continue };
                    let enabled = c.available();
                    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(170.0), 26.0), Sense::CLICK);
                    if resp.hovered() {
                        ui.painter().rect_filled(row, 3.0, t.hover);
                    }
                    icons::paint_colored(
                        ui.painter(),
                        Rect::from_min_size(row.left_top() + vec2(4.0, 4.0), vec2(18.0, 18.0)),
                        c.icon,
                        if enabled { t.icon } else { t.icon_disabled },
                        t,
                        enabled,
                    );
                    ui.painter().text(
                        row.left_center() + vec2(28.0, 0.0),
                        Align2::LEFT_CENTER,
                        c.name(),
                        theme::body(),
                        if enabled { t.text } else { t.text_disabled },
                    );
                    if resp.on_hover_text(tooltip(c)).clicked() {
                        chosen = Some(c.id);
                    }
                }
            });
        });
        let outside = ui.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer();
        if let Some(id) = chosen {
            self.chrome.dropdown = None;
            self.command(id);
        } else if outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.chrome.dropdown = None;
        }
    }

    pub(crate) fn doc_tabs(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        icons::paint(ui.painter(), Rect::from_min_size(pos2(r.left() + 8.0, r.top() + 5.0), vec2(16.0, 16.0)), Icon::Home, t.icon_disabled);
        let (label, _) = self.document_label();
        let w = text_width(ui, &label, theme::small()) + 34.0;
        let tab = Rect::from_min_size(pos2(r.left() + 32.0, r.top()), vec2(w, r.height()));
        ui.painter().rect_filled(tab, 0.0, t.panel);
        ui.painter().hline(tab.x_range(), tab.top() + 0.5, Stroke::new(2.0, t.accent));
        icons::paint_colored(
            ui.painter(),
            Rect::from_min_size(pos2(tab.left() + 6.0, tab.center().y - 7.0), vec2(14.0, 14.0)),
            if self.in_drawing() {
                Icon::Drawing
            } else if self.asm.is_some() {
                Icon::Assembly
            } else {
                Icon::Part
            },
            t.icon,
            t,
            true,
        );
        ui.painter().text(pos2(tab.left() + 24.0, tab.center().y), Align2::LEFT_CENTER, label, theme::small(), t.text);
        icons::paint(ui.painter(), Rect::from_min_size(pos2(r.right() - 24.0, r.top() + 5.0), vec2(16.0, 16.0)), Icon::Menu, t.icon_disabled);
        ui.painter().hline(r.x_range(), r.top() + 0.5, Stroke::new(1.0, t.border));
    }

    pub(crate) fn status_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        let color = if self.status_error { t.history_marker } else { t.text_dim };
        let status = if self.status.is_empty() { "Ready" } else { &self.status };
        ui.painter().text(pos2(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, status, theme::small(), color);
        let mut right = Vec::new();
        if let Some(d) = self.drw_status() {
            right.push(d);
        }
        if let Some(a) = self.asm_status() {
            right.push(a);
        }
        if let Some(d) = self.sketch_dof_text() {
            right.push(d);
        }
        if self.waiting {
            right.push("Updating...".into());
        } else if self.scene_seq > 0 && !self.in_assembly() && !self.in_drawing() {
            right.push(format!("{:.0} ms", self.scene.regen_ms + self.scene.mesh_ms));
        }
        right.push("mm".to_owned());
        ui.painter().text(pos2(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER, right.join("      "), theme::small(), t.text_dim);
    }

    pub(crate) fn file_menu(&mut self, ui: &mut Ui, t: &Tokens) {
        let Some(at) = self.chrome.file_menu else {
            return;
        };
        let mut chosen = None;
        let entries = if self.in_drawing() { commands::DRW_FILE_MENU } else { FILE_MENU };
        egui::Area::new(egui::Id::new("tn_file_menu")).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
            Frame::menu(ui.style()).fill(t.panel).show(ui, |ui| {
                ui.set_min_width(190.0);
                for (label, id) in entries {
                    if crate::drawing::button(ui, label, id).clicked() {
                        chosen = Some(*id);
                    }
                }
                ui.separator();
                for (label, id) in [("Options...", "tools.options"), ("About Tenon", "app.about"), ("Exit", "app.exit")] {
                    if crate::drawing::button(ui, label, id).clicked() {
                        chosen = Some(id);
                    }
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
            ui.label("Free, open-source parametric 3D CAD.");
            ui.add_space(6.0);
            ui.label("Licence: MIT OR Apache-2.0.");
            ui.label("Geometry kernel: OpenCASCADE Technology 8 (LGPL-2.1 with the Open CASCADE exception), dynamically linked.");
            ui.label("Forked from CADCraft (MIT OR Apache-2.0). See NOTICE.");
            ui.add_space(6.0);
            ui.small("Tenon is not affiliated with Autodesk, Inc. or any other CAD vendor.");
        });
        self.chrome.about = about;

        let mut options = self.chrome.options;
        egui::Window::new("Application Options").open(&mut options).collapsible(false).resizable(false).default_width(320.0).show(ui.ctx(), |ui| {
            ui.strong("Colors");
            ui.horizontal(|ui| {
                ui.label("UI theme");
                ui.radio_value(&mut self.chrome.theme, ThemeName::Light, "Light");
                ui.radio_value(&mut self.chrome.theme, ThemeName::Dark, "Dark");
            });
            ui.add_space(6.0);
            ui.strong("Display");
            ui.checkbox(&mut self.chrome.show_cube, "Orientation cube");
            ui.checkbox(&mut self.chrome.show_navbar, "Navigation bar");
            ui.checkbox(&mut self.chrome.show_browser, "Model browser");
        });
        self.chrome.options = options;

        let mut mass = self.chrome.mass;
        egui::Window::new("Mass Properties").open(&mut mass).collapsible(false).resizable(false).default_width(340.0).show(ui.ctx(), |ui| {
            if self.scene.bodies.is_empty() {
                ui.label("There is no solid yet.");
            }
            for (i, b) in self.scene.bodies.iter().enumerate() {
                let c = b.mass.center_of_mass;
                ui.strong(format!("Solid{}", i + 1));
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
            ui.small("Mass equals volume at unit density; materials arrive in M6.");
        });
        self.chrome.mass = mass;
        let t = crate::theme::Tokens::of(self.chrome.theme);
        self.parameters_window(ui, &t);
    }
}

/// Every ribbon id with a handler in this milestone (for tests).
#[cfg(test)]
pub(crate) fn available_ids() -> Vec<&'static str> {
    crate::commands::all().filter(|c| c.available()).map(|c| c.id).collect()
}

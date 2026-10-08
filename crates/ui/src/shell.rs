//! The application window: title bar with quick-access tools, ribbon (tabs + panels), model
//! browser, viewport (gradient, orientation cube, navigation bar, axis triad), document tabs and
//! status bar.
//!
//! M0 is the layout shell only: commands that do not work yet say which milestone brings them.

use egui::{Align2, Color32, Frame, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};

use crate::commands::{self, NAV_BAR, QUICK_ACCESS, RIBBON, Size, UiCommand};
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// Window state of the shell.
#[derive(Debug, Clone)]
pub struct Shell {
    tab: usize,
    show_browser: bool,
    show_cube: bool,
    origin_open: bool,
    file_menu: Option<Pos2>,
    about: bool,
    exit_requested: bool,
    status: String,
    document: String,
}

impl Default for Shell {
    fn default() -> Self {
        Shell {
            tab: 0,
            show_browser: true,
            show_cube: true,
            origin_open: true,
            file_menu: None,
            about: false,
            exit_requested: false,
            status: "Ready".into(),
            document: "Part1".into(),
        }
    }
}

const TITLE_H: f32 = 30.0;
const TABS_H: f32 = 26.0;
const RIBBON_H: f32 = 96.0;
const DOC_TABS_H: f32 = 26.0;
const STATUS_H: f32 = 24.0;

/// Text width in `font`.
fn text_width(ui: &Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter().layout_no_wrap(text.to_owned(), font, Color32::WHITE).size().x
}

fn tooltip(cmd: &UiCommand) -> String {
    if cmd.available() {
        format!("{}  [{}]\n{}", cmd.label, cmd.id, cmd.tip)
    } else {
        format!("{}  [{}]\n{}\nArrives in milestone M{}", cmd.label, cmd.id, cmd.tip, cmd.milestone)
    }
}

/// A ribbon button (large: icon over label; small: icon then label). Returns true when clicked.
fn command_button(ui: &Ui, rect: Rect, cmd: &UiCommand, t: &Tokens) -> bool {
    let resp = ui.interact(rect, ui.id().with(cmd.id), Sense::click());
    let p = ui.painter();
    if resp.is_pointer_button_down_on() {
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
fn icon_button(ui: &Ui, rect: Rect, cmd: &UiCommand, t: &Tokens) -> bool {
    let resp = ui.interact(rect, ui.id().with(cmd.id), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, t.hover);
    }
    icons::paint(ui.painter(), rect.shrink(rect.width() * 0.18), cmd.icon, if cmd.available() { t.icon } else { t.icon_disabled });
    resp.on_hover_text(tooltip(cmd)).clicked()
}

/// One row of the model browser.
struct Row {
    depth: u8,
    icon: Icon,
    label: String,
    /// `Some(open)` for rows that expand.
    expand: Option<bool>,
    key: &'static str,
    marker: bool,
}

// Orientation cube: an isometric view from front-right-top (Z up, front = -Y).
const VIEW: [f32; 3] = [0.577_350_3, -0.577_350_3, 0.577_350_3];
const SCREEN_RIGHT: [f32; 3] = [std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2, 0.0];
const SCREEN_UP: [f32; 3] = [-0.408_248_3, 0.408_248_3, 0.816_496_6];

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn project(p: [f32; 3], center: Pos2, scale: f32) -> Pos2 {
    pos2(center.x + dot(p, SCREEN_RIGHT) * scale, center.y - dot(p, SCREEN_UP) * scale)
}

/// Cube faces: outward normal, corners (counter-clockwise from outside), label.
const CUBE_FACES: [([f32; 3], [[f32; 3]; 4], &str); 6] = [
    ([0.0, 0.0, 1.0], [[-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]], "TOP"),
    ([0.0, 0.0, -1.0], [[-1.0, -1.0, -1.0], [-1.0, 1.0, -1.0], [1.0, 1.0, -1.0], [1.0, -1.0, -1.0]], "BOTTOM"),
    ([0.0, -1.0, 0.0], [[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, -1.0, 1.0], [-1.0, -1.0, 1.0]], "FRONT"),
    ([0.0, 1.0, 0.0], [[1.0, 1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, 1.0, 1.0], [1.0, 1.0, 1.0]], "BACK"),
    ([1.0, 0.0, 0.0], [[1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [1.0, 1.0, 1.0], [1.0, -1.0, 1.0]], "RIGHT"),
    ([-1.0, 0.0, 0.0], [[-1.0, 1.0, -1.0], [-1.0, -1.0, -1.0], [-1.0, -1.0, 1.0], [-1.0, 1.0, 1.0]], "LEFT"),
];

fn inside_convex(poly: &[Pos2], p: Pos2) -> bool {
    let n = poly.len();
    let mut sign = 0.0f32;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let c = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        if c != 0.0 {
            if sign != 0.0 && c.signum() != sign {
                return false;
            }
            sign = c.signum();
        }
    }
    true
}

impl Shell {
    pub fn new() -> Self {
        Self::default()
    }

    /// The user chose File > Exit.
    pub fn exit_requested(&self) -> bool {
        self.exit_requested
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn browser_visible(&self) -> bool {
        self.show_browser
    }
    pub fn cube_visible(&self) -> bool {
        self.show_cube
    }
    pub fn active_tab(&self) -> &'static str {
        RIBBON.get(self.tab).map_or("", |t| t.name)
    }

    /// Runs a shell command by id. Commands that are not implemented yet report the milestone that
    /// brings them and return an error, as every command will once the registry lands (M1).
    pub fn run(&mut self, id: &str) -> Result<(), String> {
        match id {
            "view.browser" => {
                self.show_browser = !self.show_browser;
                self.status = format!("Model browser {}", if self.show_browser { "shown" } else { "hidden" });
            }
            "view.cube" => {
                self.show_cube = !self.show_cube;
                self.status = format!("Orientation cube {}", if self.show_cube { "shown" } else { "hidden" });
            }
            "app.about" => self.about = true,
            "app.exit" => self.exit_requested = true,
            _ => {
                let msg = match commands::find(id) {
                    Some(c) => format!("{} is not available yet: it arrives in milestone M{}.", c.label, c.milestone),
                    None => format!("unknown command `{id}`"),
                };
                self.status.clone_from(&msg);
                return Err(msg);
            }
        }
        Ok(())
    }

    /// Selects a ribbon tab by name.
    pub fn select_tab(&mut self, name: &str) -> Result<(), String> {
        let i = RIBBON.iter().position(|t| t.name.eq_ignore_ascii_case(name)).ok_or_else(|| format!("no ribbon tab `{name}`"))?;
        self.tab = i;
        Ok(())
    }

    /// Draws the whole window.
    pub fn ui(&mut self, ui: &mut Ui) {
        let t = Tokens::DARK;
        egui::Panel::top("tn_title").exact_size(TITLE_H).frame(Frame::NONE.fill(t.title_bar)).show(ui, |ui| self.title_bar(ui, &t));
        egui::Panel::top("tn_tabs").exact_size(TABS_H).frame(Frame::NONE.fill(t.tab_strip)).show(ui, |ui| self.ribbon_tabs(ui, &t));
        egui::Panel::top("tn_ribbon").exact_size(RIBBON_H).frame(Frame::NONE.fill(t.ribbon)).show(ui, |ui| self.ribbon(ui, &t));
        egui::Panel::bottom("tn_status").exact_size(STATUS_H).frame(Frame::NONE.fill(t.title_bar)).show(ui, |ui| self.status_bar(ui, &t));
        egui::Panel::bottom("tn_docs").exact_size(DOC_TABS_H).frame(Frame::NONE.fill(t.tab_strip)).show(ui, |ui| self.doc_tabs(ui, &t));
        if self.show_browser {
            egui::Panel::left("tn_browser")
                .default_size(250.0)
                .size_range(180.0..=480.0)
                .resizable(true)
                .frame(Frame::NONE.fill(t.panel))
                .show(ui, |ui| self.browser(ui, &t));
        }
        egui::CentralPanel::default().frame(Frame::NONE.fill(t.viewport_bottom)).show(ui, |ui| self.viewport(ui, &t));
        self.file_menu(ui, &t);
        self.about_window(ui);
    }

    fn title_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        let mark = Rect::from_min_size(pos2(r.left() + 10.0, r.center().y - 7.0), vec2(14.0, 14.0));
        ui.painter().rect_filled(mark, 3.0, t.accent);
        ui.painter().rect_filled(Rect::from_center_size(mark.center(), vec2(4.0, 8.0)), 1.0, t.title_bar);
        let mut x = mark.right() + 14.0;
        let mut clicked = None;
        for cmd in QUICK_ACCESS {
            let br = Rect::from_min_size(pos2(x, r.center().y - 11.0), vec2(22.0, 22.0));
            if icon_button(ui, br, cmd, t) {
                clicked = Some(cmd.id);
            }
            x += 25.0;
        }
        ui.painter().text(r.center(), Align2::CENTER_CENTER, format!("Tenon   {}", self.document), theme::body(), t.text_dim);
        ui.painter().text(
            pos2(r.right() - 10.0, r.center().y),
            Align2::RIGHT_CENTER,
            concat!("v", env!("CARGO_PKG_VERSION"), "  M0 preview"),
            theme::small(),
            t.text_disabled,
        );
        if let Some(id) = clicked {
            let _ = self.run(id);
        }
    }

    fn ribbon_tabs(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        let file = Rect::from_min_size(pos2(r.left() + 6.0, r.top() + 3.0), vec2(52.0, r.height() - 6.0));
        let resp = ui.interact(file, ui.id().with("file"), Sense::click());
        ui.painter().rect_filled(file, 3.0, if resp.hovered() { t.accent.gamma_multiply(1.2) } else { t.accent });
        ui.painter().text(file.center(), Align2::CENTER_CENTER, "File", theme::body(), t.accent_text);
        if resp.clicked() {
            self.file_menu = if self.file_menu.is_some() { None } else { Some(file.left_bottom() + vec2(0.0, 2.0)) };
        }
        let mut x = file.right() + 10.0;
        for (i, tab) in RIBBON.iter().enumerate() {
            let w = text_width(ui, tab.name, theme::body()) + 24.0;
            let tr = Rect::from_min_size(pos2(x, r.top() + 2.0), vec2(w, r.height() - 2.0));
            let resp = ui.interact(tr, ui.id().with(("tab", i)), Sense::click());
            let active = i == self.tab;
            if active {
                ui.painter().rect_filled(tr, egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, t.ribbon);
                ui.painter().hline(tr.x_range(), tr.top() + 1.0, Stroke::new(2.0, t.accent));
            } else if resp.hovered() {
                ui.painter().rect_filled(tr, 3.0, t.hover);
            }
            ui.painter().text(tr.center(), Align2::CENTER_CENTER, tab.name, theme::body(), if active { t.text } else { t.text_dim });
            if resp.clicked() {
                self.tab = i;
            }
            x += w + 2.0;
        }
    }

    fn ribbon(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        let Some(tab) = RIBBON.get(self.tab) else {
            return;
        };
        let title_h = 18.0;
        let top = r.top() + 4.0;
        let content_h = r.height() - title_h - 8.0;
        let mut x = r.left() + 8.0;
        let mut clicked = None;
        for panel in tab.panels {
            let start = x;
            for cmd in panel.commands.iter().filter(|c| c.size == Size::Large) {
                let w = (text_width(ui, cmd.label, theme::small()) + 14.0).max(50.0);
                if command_button(ui, Rect::from_min_size(pos2(x, top), vec2(w, content_h)), cmd, t) {
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
                    if command_button(ui, br, cmd, t) {
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
            let _ = self.run(id);
        }
    }

    fn tree_rows(&self) -> Vec<Row> {
        let row = |depth, icon, label: &str, key| Row { depth, icon, label: label.to_owned(), expand: None, key, marker: false };
        let mut rows = vec![
            Row { expand: Some(true), ..row(0, Icon::Part, &self.document, "part") },
            row(1, Icon::Folder, "Solid Bodies (0)", "bodies"),
            Row { expand: Some(self.origin_open), ..row(1, Icon::Folder, "Origin", "origin") },
        ];
        if self.origin_open {
            for (icon, label, key) in [
                (Icon::Plane, "YZ Plane", "origin.yz"),
                (Icon::Plane, "XZ Plane", "origin.xz"),
                (Icon::Plane, "XY Plane", "origin.xy"),
                (Icon::Axis, "X Axis", "origin.x"),
                (Icon::Axis, "Y Axis", "origin.y"),
                (Icon::Axis, "Z Axis", "origin.z"),
                (Icon::Point, "Center Point", "origin.center"),
            ] {
                rows.push(row(2, icon, label, key));
            }
        }
        rows.push(Row { marker: true, ..row(1, Icon::FinishSketch, "End of history", "end") });
        rows
    }

    fn browser(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        let header = Rect::from_min_size(r.min, vec2(r.width(), 26.0));
        ui.painter().rect_filled(header, 0.0, t.panel_header);
        ui.painter().text(pos2(header.left() + 10.0, header.center().y), Align2::LEFT_CENTER, "Model", theme::heading(), t.text);
        ui.painter().hline(header.x_range(), header.bottom(), Stroke::new(1.0, t.border));
        let row_h = 21.0;
        let mut y = header.bottom() + 6.0;
        let mut toggle = false;
        for row in self.tree_rows() {
            let rr = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), row_h));
            let resp = ui.interact(rr, ui.id().with(("row", row.key)), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(rr, 0.0, t.hover);
            }
            let indent = r.left() + 8.0 + f32::from(row.depth) * 16.0;
            if let Some(open) = row.expand {
                let c = pos2(indent + 5.0, rr.center().y);
                let tri = if open {
                    vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)]
                } else {
                    vec![c + vec2(-2.0, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 4.0)]
                };
                ui.painter().add(Shape::convex_polygon(tri, t.text_dim, Stroke::NONE));
            }
            let x = indent + 14.0;
            if row.marker {
                ui.painter().rect_filled(Rect::from_min_size(pos2(x, rr.center().y - 3.0), vec2(16.0, 6.0)), 1.0, t.history_marker);
                ui.painter().text(pos2(x + 22.0, rr.center().y), Align2::LEFT_CENTER, &row.label, theme::small(), t.history_marker);
                let _ = resp.on_hover_text("Rollback marker: features below it are not computed. Dragging it arrives in milestone M2.");
            } else {
                icons::paint(ui.painter(), Rect::from_min_size(pos2(x, rr.center().y - 8.0), vec2(16.0, 16.0)), row.icon, t.icon);
                ui.painter().text(pos2(x + 22.0, rr.center().y), Align2::LEFT_CENTER, &row.label, theme::small(), t.text);
                if row.key == "origin" && resp.clicked() {
                    toggle = true;
                }
            }
            y += row_h;
        }
        if toggle {
            self.origin_open = !self.origin_open;
        }
    }

    fn viewport(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        let mut mesh = egui::Mesh::default();
        for (p, c) in [
            (r.left_top(), t.viewport_top),
            (r.right_top(), t.viewport_top),
            (r.right_bottom(), t.viewport_bottom),
            (r.left_bottom(), t.viewport_bottom),
        ] {
            mesh.colored_vertex(p, c);
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        ui.painter().add(Shape::mesh(mesh));
        ui.painter().text(
            r.center() - vec2(0.0, 12.0),
            Align2::CENTER_CENTER,
            "The 3D viewport arrives in milestone M1.",
            theme::heading(),
            t.viewport_text,
        );
        ui.painter().text(
            r.center() + vec2(0.0, 12.0),
            Align2::CENTER_CENTER,
            "The modelling kernel already works headless:  tenon-cli demo m0",
            theme::small(),
            t.text_dim,
        );
        self.triad(ui, pos2(r.left() + 46.0, r.bottom() - 46.0), t);
        if self.show_cube {
            self.cube(ui, pos2(r.right() - 92.0, r.top() + 82.0), t);
        }
        self.nav_bar(ui, r, t);
    }

    fn triad(&self, ui: &Ui, origin: Pos2, t: &Tokens) {
        for (axis, color, label) in [([1.0, 0.0, 0.0], t.axis_x, "X"), ([0.0, 1.0, 0.0], t.axis_y, "Y"), ([0.0, 0.0, 1.0], t.axis_z, "Z")] {
            let end = project(axis, origin, 28.0);
            ui.painter().line_segment([origin, end], Stroke::new(2.0, color));
            let label_pos = origin + (end - origin) * 1.3;
            ui.painter().text(label_pos, Align2::CENTER_CENTER, label, theme::small(), color);
        }
    }

    fn cube(&mut self, ui: &Ui, center: Pos2, t: &Tokens) {
        let scale = 30.0;
        let area = Rect::from_center_size(center, vec2(scale * 3.4, scale * 3.4));
        let resp = ui.interact(area, ui.id().with("cube"), Sense::click());
        let hover = resp.hover_pos();
        let mut picked = None;
        for (normal, corners, label) in CUBE_FACES {
            if dot(normal, VIEW) <= 0.0 {
                continue;
            }
            let poly: Vec<Pos2> = corners.iter().map(|c| project(*c, center, scale)).collect();
            let hot = hover.is_some_and(|h| inside_convex(&poly, h));
            if hot {
                picked = Some(label);
            }
            ui.painter().add(Shape::convex_polygon(poly.clone(), if hot { t.cube_face_hover } else { t.cube_face }, Stroke::new(1.0, t.cube_edge)));
            let mid = poly.iter().fold(Pos2::ZERO, |a, p| a + p.to_vec2() / 4.0);
            ui.painter().text(mid, Align2::CENTER_CENTER, label, theme::small(), t.cube_text);
        }
        let home = Rect::from_min_size(area.left_top() + vec2(-2.0, -4.0), vec2(18.0, 18.0));
        icons::paint(ui.painter(), home, Icon::Home, t.icon_disabled);
        if resp.clicked()
            && let Some(face) = picked
        {
            self.status = format!("{face} view selected. View changes arrive with the 3D viewport in milestone M1.");
        }
        let _ = resp.on_hover_text("Orientation cube: click a face to look at it (works from M1)");
    }

    fn nav_bar(&mut self, ui: &Ui, viewport: Rect, t: &Tokens) {
        let size = 28.0;
        let h = NAV_BAR.len() as f32 * (size + 2.0) + 8.0;
        let bar = Rect::from_min_size(pos2(viewport.right() - size - 14.0, viewport.center().y - h / 2.0), vec2(size + 8.0, h));
        ui.painter().rect_filled(bar, 6.0, t.panel.gamma_multiply(0.85));
        let mut clicked = None;
        for (i, cmd) in NAV_BAR.iter().enumerate() {
            let br = Rect::from_min_size(pos2(bar.left() + 4.0, bar.top() + 4.0 + i as f32 * (size + 2.0)), vec2(size, size));
            if icon_button(ui, br, cmd, t) {
                clicked = Some(cmd.id);
            }
        }
        if let Some(id) = clicked {
            let _ = self.run(id);
        }
    }

    fn doc_tabs(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        icons::paint(ui.painter(), Rect::from_min_size(pos2(r.left() + 8.0, r.top() + 5.0), vec2(16.0, 16.0)), Icon::Home, t.icon_disabled);
        let label = format!("{}.tenon", self.document);
        let w = text_width(ui, &label, theme::small()) + 40.0;
        let tab = Rect::from_min_size(pos2(r.left() + 32.0, r.top()), vec2(w, r.height()));
        ui.painter().rect_filled(tab, 0.0, t.panel);
        ui.painter().hline(tab.x_range(), tab.bottom() - 1.0, Stroke::new(2.0, t.accent));
        ui.painter().text(pos2(tab.left() + 10.0, tab.center().y), Align2::LEFT_CENTER, label, theme::small(), t.text);
        ui.painter().text(pos2(tab.right() - 12.0, tab.center().y), Align2::CENTER_CENTER, "x", theme::small(), t.text_disabled);
    }

    fn status_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        ui.painter().text(pos2(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, &self.status, theme::small(), t.text_dim);
        ui.painter().text(pos2(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER, "Units: mm", theme::small(), t.text_dim);
    }

    fn file_menu(&mut self, ui: &mut Ui, t: &Tokens) {
        let Some(at) = self.file_menu else {
            return;
        };
        let mut chosen = None;
        egui::Area::new(egui::Id::new("tn_file_menu")).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
            Frame::menu(ui.style()).fill(t.panel).show(ui, |ui| {
                ui.set_min_width(180.0);
                for (label, id) in [("New Part", "file.new"), ("Open...", "file.open"), ("Save", "file.save")] {
                    if ui.button(label).on_hover_text(tooltip_for(id)).clicked() {
                        chosen = Some(id);
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
            self.file_menu = None;
        }
        if let Some(id) = chosen {
            self.file_menu = None;
            let _ = self.run(id);
        }
    }

    fn about_window(&mut self, ui: &mut Ui) {
        let mut open = self.about;
        egui::Window::new("About Tenon").open(&mut open).collapsible(false).resizable(false).default_width(420.0).show(ui.ctx(), |ui| {
            ui.heading(concat!("Tenon ", env!("CARGO_PKG_VERSION")));
            ui.label("Free, open-source parametric 3D CAD. Milestone M0: foundations.");
            ui.add_space(6.0);
            ui.label("Licence: MIT OR Apache-2.0.");
            ui.label("Geometry kernel: OpenCASCADE Technology 8 (LGPL-2.1 with the Open CASCADE exception), dynamically linked. In M0 it is driven by tenon-cli; the desktop UI connects to it in M1.");
            ui.label("Forked from CADCraft (MIT OR Apache-2.0). See NOTICE.");
            ui.add_space(6.0);
            ui.small("Tenon is not affiliated with Autodesk, Inc. or any other CAD vendor.");
        });
        self.about = open;
    }
}

fn tooltip_for(id: &str) -> String {
    commands::find(id).map(tooltip).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(shell: &mut Shell, ctx: &egui::Context, input: egui::RawInput) {
        // Headless: no renderer consumes the texture updates.
        ctx.run_ui(input, |ui| shell.ui(ui)).drop_without_applying_deltas();
    }

    fn screen() -> egui::RawInput {
        egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 860.0))), ..Default::default() }
    }

    #[test]
    fn renders_every_tab_and_toggle_without_panicking() {
        let ctx = egui::Context::default();
        let mut shell = Shell::new();
        for tab in RIBBON {
            shell.select_tab(tab.name).unwrap();
            frame(&mut shell, &ctx, screen());
        }
        shell.run("view.browser").unwrap();
        shell.run("view.cube").unwrap();
        shell.run("app.about").unwrap();
        frame(&mut shell, &ctx, screen());
        assert!(!shell.browser_visible() && !shell.cube_visible());
        // Degenerate window sizes must not panic either.
        for size in [vec2(1.0, 1.0), vec2(0.0, 0.0), vec2(5000.0, 3000.0)] {
            frame(&mut shell, &ctx, egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)), ..Default::default() });
        }
    }

    #[test]
    fn unavailable_commands_say_when_they_arrive() {
        let mut shell = Shell::new();
        let e = shell.run("model.extrude").unwrap_err();
        assert!(e.contains("M1"), "{e}");
        assert_eq!(shell.status(), e);
        assert!(shell.run("no.such").unwrap_err().contains("unknown"));
        assert!(shell.select_tab("nope").is_err());
        assert!(shell.select_tab("sketch").is_ok());
        assert_eq!(shell.active_tab(), "Sketch");
        shell.run("app.exit").unwrap();
        assert!(shell.exit_requested());
    }

    #[test]
    fn cube_shows_top_front_right() {
        let visible: Vec<_> = CUBE_FACES.iter().filter(|(n, _, _)| dot(*n, VIEW) > 0.0).map(|(_, _, l)| *l).collect();
        assert_eq!(visible, ["TOP", "FRONT", "RIGHT"]);
        // Z projects straight up, X to the lower right, Y to the upper right.
        let c = Pos2::ZERO;
        assert!(project([0.0, 0.0, 1.0], c, 1.0).y < 0.0 && project([0.0, 0.0, 1.0], c, 1.0).x.abs() < 1e-6);
        let x = project([1.0, 0.0, 0.0], c, 1.0);
        let y = project([0.0, 1.0, 0.0], c, 1.0);
        assert!(x.x > 0.0 && x.y > 0.0 && y.x > 0.0 && y.y < 0.0);
        let square = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
        assert!(inside_convex(&square, pos2(0.5, 0.5)) && !inside_convex(&square, pos2(1.5, 0.5)));
    }
}

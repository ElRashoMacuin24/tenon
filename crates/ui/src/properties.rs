//! The properties panel (docked above the browser while a feature command runs), the
//! mini-toolbar next to the preview, and the drag arrow that sets a distance in the viewport.

use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use tenon_geom::Vec3;
use tenon_model::{AxisSel, DirectionRef, Operation, OriginAxis, OriginPlane, PlaneRef, RegionSel};
use tenon_sketch::EntityId;

use crate::panels::{AxisChoice, ChamferMethod, CopyKind, Direction, ExtentChoice, Panel, PanelRequest, Seat, Slot};
use crate::theme::{self, Tokens};
use crate::work::{WorkMethod, WorkSlot};
use crate::workbench::Workbench;

/// Formats a value for a field: up to 3 decimals, no trailing zeros.
pub(crate) fn fmt_value(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// Parses what was typed into a value field: a number with an optional unit.
pub(crate) fn parse_value(text: &str, unit: &str) -> Option<f64> {
    let s = text.trim();
    let s = s.strip_suffix(unit).unwrap_or(s).trim();
    let s = s.strip_suffix("deg").or_else(|| s.strip_suffix("mm")).unwrap_or(s).trim();
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// What a value field did this frame.
#[derive(Default)]
pub(crate) struct FieldOut {
    pub changed: bool,
    /// Enter was pressed in the field.
    pub entered: bool,
}

/// Parameter values by name, for fields that take equations (set every frame).
pub(crate) fn set_param_env(ctx: &egui::Context, env: std::sync::Arc<std::collections::BTreeMap<String, f64>>) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("tn_param_env"), env));
}

/// What was typed: a number (with no equation), or an equation and its value.
fn parse_entry(ui: &Ui, text: &str, unit: &str) -> Option<(f64, Option<String>)> {
    if let Some(v) = parse_value(text, unit) {
        return Some((v, None));
    }
    let env: std::sync::Arc<std::collections::BTreeMap<String, f64>> = ui.data(|d| d.get_temp(egui::Id::new("tn_param_env"))).unwrap_or_default();
    let v = tenon_model::expr::parse(text).ok()?.eval(&env).ok()?;
    Some((v, Some(text.trim().to_owned())))
}

/// Equations typed into a panel's fields, by field key.
pub(crate) type Equations = std::collections::BTreeMap<&'static str, String>;

/// A value box that keeps the equation typed into it (in `eqs` under `key`), shown in place of
/// the number until a plain number replaces it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn eq_value_field(
    ui: &mut Ui,
    id: egui::Id,
    value: &mut f64,
    eqs: &mut Equations,
    key: &'static str,
    unit: &str,
    range: std::ops::RangeInclusive<f64>,
    width: f32,
    t: &Tokens,
) -> FieldOut {
    let mut slot = eqs.get(key).cloned();
    let out = eq_field(ui, id, value, Some(&mut slot), unit, range, width, t);
    match slot {
        Some(e) => {
            eqs.insert(key, e);
        }
        None => {
            eqs.remove(key);
        }
    }
    out
}

/// A value box with its unit, as in the properties panel and mini-toolbar. Typing edits the text;
/// a valid number applies at once (so the preview follows), an invalid one shows red. An
/// equation (`d0 / 2`) is evaluated too, and kept in `eq` when given.
#[allow(clippy::too_many_arguments)]
pub(crate) fn eq_field(
    ui: &mut Ui,
    id: egui::Id,
    value: &mut f64,
    mut eq: Option<&mut Option<String>>,
    unit: &str,
    range: std::ops::RangeInclusive<f64>,
    width: f32,
    t: &Tokens,
) -> FieldOut {
    let mut out = FieldOut::default();
    let focused = ui.memory(|m| m.has_focus(id));
    let equation = eq.as_deref().cloned().flatten();
    let mut text: String = if focused {
        ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| equation.clone().unwrap_or_else(|| fmt_value(*value)))
    } else {
        match &equation {
            Some(e) => format!("fx: {e}"),
            None => format!("{} {unit}", fmt_value(*value)),
        }
    };
    let entry = if focused { parse_entry(ui, &text, unit) } else { None };
    let valid = !focused || entry.as_ref().is_some_and(|(v, _)| range.contains(v));
    let resp = ui.add(egui::TextEdit::singleline(&mut text).id(id).desired_width(width).font(theme::body()).text_color(if valid {
        t.text
    } else {
        t.history_marker
    }));
    if resp.gained_focus() {
        // Select the whole value (or equation), so typing replaces it.
        text = equation.clone().unwrap_or_else(|| fmt_value(*value));
        if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), id) {
            let all = egui::text_selection::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()));
            state.cursor.set_char_range(Some(all));
            state.store(ui.ctx(), id);
        }
    }
    if resp.has_focus() || resp.gained_focus() {
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
    }
    if resp.changed()
        && let Some((v, e)) = parse_entry(ui, &text, unit).filter(|(v, _)| range.contains(v))
    {
        if (v - *value).abs() > 0.0 {
            *value = v;
            out.changed = true;
        }
        if let Some(slot) = &mut eq
            && **slot != e
        {
            **slot = e;
            out.changed = true;
        }
    }
    if resp.lost_focus() {
        out.entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.data_mut(|d| d.remove::<String>(id));
    }
    let border = if !valid {
        t.history_marker
    } else if resp.has_focus() {
        t.accent
    } else {
        t.border
    };
    ui.painter().rect_stroke(resp.rect.expand(1.0), 2.0, Stroke::new(1.0, border), egui::StrokeKind::Outside);
    out
}

/// The small pictures on the direction and output buttons.
#[derive(Clone, Copy)]
enum Glyph {
    Default,
    Flipped,
    Symmetric,
    Asymmetric,
    Join,
    Cut,
    Intersect,
    NewSolid,
}

fn paint_glyph(ui: &Ui, r: Rect, g: Glyph, color: Color32, fill: Color32) {
    let p = ui.painter();
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    let s = Stroke::new(1.4, color);
    let arrow = |from: (f32, f32), to: (f32, f32)| {
        p.line_segment([at(from.0, from.1), at(to.0, to.1)], s);
        let d = (at(to.0, to.1) - at(from.0, from.1)).normalized();
        let n = vec2(-d.y, d.x);
        let tip = at(to.0, to.1);
        p.add(Shape::convex_polygon(vec![tip, tip - d * 5.0 + n * 3.5, tip - d * 5.0 - n * 3.5], color, Stroke::NONE));
    };
    let plate = || p.rect_filled(Rect::from_min_max(at(0.15, 0.62), at(0.85, 0.78)), 1.0, fill);
    match g {
        Glyph::Default => {
            plate();
            arrow((0.5, 0.6), (0.5, 0.12));
        }
        Glyph::Flipped => {
            p.rect_filled(Rect::from_min_max(at(0.15, 0.22), at(0.85, 0.38)), 1.0, fill);
            arrow((0.5, 0.4), (0.5, 0.88));
        }
        Glyph::Symmetric => {
            p.rect_filled(Rect::from_min_max(at(0.15, 0.42), at(0.85, 0.58)), 1.0, fill);
            arrow((0.5, 0.4), (0.5, 0.1));
            arrow((0.5, 0.6), (0.5, 0.9));
        }
        Glyph::Asymmetric => {
            p.rect_filled(Rect::from_min_max(at(0.15, 0.55), at(0.85, 0.7)), 1.0, fill);
            arrow((0.5, 0.53), (0.5, 0.1));
            arrow((0.5, 0.72), (0.5, 0.92));
        }
        Glyph::Join => {
            p.rect_filled(Rect::from_min_max(at(0.12, 0.35), at(0.6, 0.85)), 1.0, fill);
            p.rect_filled(Rect::from_min_max(at(0.4, 0.15), at(0.88, 0.65)), 1.0, fill);
        }
        Glyph::Cut => {
            p.rect_filled(Rect::from_min_max(at(0.12, 0.35), at(0.6, 0.85)), 1.0, fill);
            p.rect_stroke(Rect::from_min_max(at(0.4, 0.15), at(0.88, 0.65)), 1.0, s, egui::StrokeKind::Inside);
        }
        Glyph::Intersect => {
            p.rect_stroke(Rect::from_min_max(at(0.12, 0.35), at(0.6, 0.85)), 1.0, s, egui::StrokeKind::Inside);
            p.rect_stroke(Rect::from_min_max(at(0.4, 0.15), at(0.88, 0.65)), 1.0, s, egui::StrokeKind::Inside);
            p.rect_filled(Rect::from_min_max(at(0.4, 0.35), at(0.6, 0.65)), 0.0, fill);
        }
        Glyph::NewSolid => {
            p.rect_filled(Rect::from_min_max(at(0.18, 0.3), at(0.62, 0.82)), 1.0, fill);
            p.line_segment([at(0.78, 0.12), at(0.78, 0.42)], s);
            p.line_segment([at(0.63, 0.27), at(0.93, 0.27)], s);
        }
    }
}

/// A row of picture buttons; returns the index clicked.
fn glyph_row(ui: &mut Ui, items: &[(Glyph, &str, bool)], selected: usize, t: &Tokens) -> Option<usize> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (i, (g, tip, enabled)) in items.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(28.0, 26.0), if *enabled { Sense::CLICK } else { Sense::hover() });
            let on = i == selected;
            ui.painter().rect_filled(
                r,
                3.0,
                if on {
                    t.pressed
                } else if resp.hovered() && *enabled {
                    t.hover
                } else {
                    t.field
                },
            );
            ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, if on { t.accent } else { t.border }), egui::StrokeKind::Inside);
            let color = if *enabled { t.icon } else { t.icon_disabled };
            let fill = if *enabled { t.tint_solid.gamma_multiply(0.8) } else { t.icon_disabled.gamma_multiply(0.4) };
            paint_glyph(ui, r.shrink(4.0), *g, color, fill);
            if resp.on_hover_text(*tip).clicked() && *enabled {
                clicked = Some(i);
            }
        }
    });
    clicked
}

/// A selector: shows what is chosen; pressed while it takes clicks in the part. Returns true when
/// clicked (to make it the active one).
fn slot_button(ui: &mut Ui, active: bool, label: &str, t: &Tokens) -> bool {
    let text = egui::RichText::new(label).color(if active { t.accent_text } else { t.text });
    let b = egui::Button::new(text).fill(if active { t.accent } else { t.field }).min_size(vec2(96.0, 22.0));
    ui.add(b).on_hover_text("Click, then pick in the part").clicked()
}

/// The flip toggle beside a direction.
fn flip_button(ui: &mut Ui, flipped: &mut bool, t: &Tokens) {
    let items = [(Glyph::Default, "Default direction", true), (Glyph::Flipped, "Flipped", true)];
    if let Some(i) = glyph_row(ui, &items, usize::from(*flipped), t) {
        *flipped = i == 1;
    }
}

/// A direction selector: the chosen direction, a list of origin axes (and None when
/// `optional`). Returns true when the selector was clicked to pick an edge.
type WorkNames<'a> = [(tenon_model::FeatureId, String, &'a str)];

fn work_name(work: &WorkNames, id: tenon_model::FeatureId) -> String {
    work.iter().find(|x| x.0 == id).map_or_else(|| format!("{id}"), |x| x.1.clone())
}

/// An axis selector: what is chosen (or a hint), and a list of origin and work axes. Returns
/// true when the selector was clicked to pick in the part.
fn axis_picker(ui: &mut Ui, id: &str, a: &mut Option<AxisSel>, active: bool, work: &WorkNames, t: &Tokens) -> bool {
    let label = match a {
        None => "click an axis or edge".into(),
        Some(AxisSel::Origin(o)) => format!("{o:?} Axis"),
        Some(AxisSel::Edge(_)) => "Edge".into(),
        Some(AxisSel::Face(_)) => "Cylinder".into(),
        Some(AxisSel::Work(w)) => work_name(work, *w),
    };
    let clicked = slot_button(ui, active, &label, t);
    egui::ComboBox::from_id_salt(id).selected_text("").width(18.0).show_ui(ui, |ui| {
        for o in [OriginAxis::X, OriginAxis::Y, OriginAxis::Z] {
            if ui.selectable_label(*a == Some(AxisSel::Origin(o)), format!("{o:?} Axis")).clicked() {
                *a = Some(AxisSel::Origin(o));
            }
        }
        for (w, n, _) in work.iter().filter(|x| x.2 == "axis") {
            if ui.selectable_label(*a == Some(AxisSel::Work(*w)), n).clicked() {
                *a = Some(AxisSel::Work(*w));
            }
        }
    });
    clicked
}

/// A plane selector: what is chosen (or a hint), and a list of origin and work planes.
fn plane_picker(ui: &mut Ui, id: &str, p: &mut Option<PlaneRef>, active: bool, work: &WorkNames, t: &Tokens) -> bool {
    let label = match p {
        None => "click a plane".into(),
        Some(PlaneRef::Origin(o)) => format!("{o:?} Plane"),
        Some(PlaneRef::Face(_)) => "Face".into(),
        Some(PlaneRef::Work(w)) => work_name(work, *w),
    };
    let clicked = slot_button(ui, active, &label, t);
    egui::ComboBox::from_id_salt(id).selected_text("").width(18.0).show_ui(ui, |ui| {
        for o in [OriginPlane::YZ, OriginPlane::XZ, OriginPlane::XY] {
            if ui.selectable_label(*p == Some(PlaneRef::Origin(o)), format!("{o:?} Plane")).clicked() {
                *p = Some(PlaneRef::Origin(o));
            }
        }
        for (w, n, _) in work.iter().filter(|x| x.2 == "plane") {
            if ui.selectable_label(*p == Some(PlaneRef::Work(*w)), n).clicked() {
                *p = Some(PlaneRef::Work(*w));
            }
        }
    });
    clicked
}

#[allow(clippy::too_many_arguments)]
fn direction_picker(
    ui: &mut Ui,
    id: &str,
    d: &mut Option<DirectionRef>,
    optional: bool,
    active: bool,
    work: &[(tenon_model::FeatureId, String, &str)],
    t: &Tokens,
) -> bool {
    let name = |w: tenon_model::FeatureId| work.iter().find(|x| x.0 == w).map_or("Work Axis".to_string(), |x| x.1.clone());
    let label = match d {
        Some(DirectionRef::Origin(a)) => format!("{a:?} Axis"),
        Some(DirectionRef::Edge(_)) => "Edge".into(),
        Some(DirectionRef::Work(w)) => name(*w),
        None => "None".into(),
    };
    let clicked = slot_button(ui, active, &label, t);
    egui::ComboBox::from_id_salt(id).selected_text("").width(18.0).show_ui(ui, |ui| {
        if optional && ui.selectable_label(d.is_none(), "None").clicked() {
            *d = None;
        }
        for a in [OriginAxis::X, OriginAxis::Y, OriginAxis::Z] {
            if ui.selectable_label(*d == Some(DirectionRef::Origin(a)), format!("{a:?} Axis")).clicked() {
                *d = Some(DirectionRef::Origin(a));
            }
        }
        for (w, n, _) in work.iter().filter(|x| x.2 == "axis") {
            if ui.selectable_label(*d == Some(DirectionRef::Work(*w)), n).clicked() {
                *d = Some(DirectionRef::Work(*w));
            }
        }
    });
    clicked
}

/// "N selected" with a button that clears the picks; returns true when it was clicked.
fn picked_row(ui: &mut Ui, n: usize, hint: &str) -> bool {
    let mut clear = false;
    ui.horizontal(|ui| {
        ui.label(if n == 0 { hint.to_string() } else { format!("{n} selected") });
        if n > 0 && ui.small_button("Clear").clicked() {
            clear = true;
        }
    });
    clear
}

fn section(ui: &mut Ui, title: &str, open: bool, body: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(egui::RichText::new(title).font(theme::body()).strong()).default_open(open).show(ui, body);
}

const OPS: [(Glyph, &str, Operation); 4] = [
    (Glyph::Join, "Join: add material", Operation::Join),
    (Glyph::Cut, "Cut: remove material", Operation::Cut),
    (Glyph::Intersect, "Intersect: keep what is common", Operation::Intersect),
    (Glyph::NewSolid, "New Solid: a separate body", Operation::NewBody),
];

impl Workbench {
    /// Number of profile regions a feature will use.
    fn profile_count(&self, sketch: tenon_model::FeatureId, sel: &RegionSel) -> usize {
        match sel {
            RegionSel::Keys(k) => k.len(),
            RegionSel::Default => self.document().sketch(sketch).map(|s| tenon_sketch::default_regions(&tenon_sketch::regions(s)).len()).unwrap_or(0),
        }
    }

    /// True while a feature command shows the properties panel.
    pub(crate) fn has_properties(&self) -> bool {
        matches!(
            self.panel,
            Some(
                Panel::Extrude(_)
                    | Panel::Revolve(_)
                    | Panel::Fillet(_)
                    | Panel::Chamfer(_)
                    | Panel::Shell(_)
                    | Panel::Hole(_)
                    | Panel::Pattern(_)
                    | Panel::Work(_)
            )
        )
    }

    /// The properties panel for the running feature command.
    pub(crate) fn properties(&mut self, ui: &mut Ui, t: &Tokens) {
        let Some(mut panel) = self.panel.clone() else { return };
        let r = ui.max_rect();
        let header = Rect::from_min_size(r.min, vec2(r.width(), 26.0));
        ui.painter().rect_filled(header, 0.0, t.panel_header);
        let tab = Rect::from_min_size(header.min, vec2(92.0, header.height()));
        ui.painter().rect_filled(tab, 0.0, t.panel);
        ui.painter().text(pos2(tab.left() + 8.0, tab.center().y), Align2::LEFT_CENTER, "Properties", theme::body(), t.text);
        ui.painter().hline(header.x_range(), header.bottom(), Stroke::new(1.0, t.border));
        ui.add_space(header.height() + 4.0);
        let sketches = self.sketches();
        // Work planes and axes, for the lists: (id, name, "plane" | "axis" | "point").
        let work_names: Vec<(tenon_model::FeatureId, String, &str)> = self
            .document()
            .features()
            .iter()
            .filter_map(|f| match f.kind {
                tenon_model::FeatureKind::WorkPlane(_) => Some((f.id, f.name.clone(), "plane")),
                tenon_model::FeatureKind::WorkAxis(_) => Some((f.id, f.name.clone(), "axis")),
                _ => None,
            })
            .filter(|w| p_before(&panel, self, w.0))
            .collect();
        let mut request = None;
        let mut enter = ui.input(|i| i.key_pressed(egui::Key::Enter)) && !ui.ctx().egui_wants_keyboard_input();
        let mut eqs = self.panel_eqs.clone();
        let (kind, name) = match &panel {
            Panel::Extrude(p) => ("Extrusion", p.editing.map(|f| self.feature_name(f))),
            Panel::Revolve(p) => ("Revolution", p.editing.map(|f| self.feature_name(f))),
            Panel::Fillet(p) => ("Fillet", p.editing.map(|f| self.feature_name(f))),
            Panel::Chamfer(p) => ("Chamfer", p.editing.map(|f| self.feature_name(f))),
            Panel::Shell(p) => ("Shell", p.editing.map(|f| self.feature_name(f))),
            Panel::Hole(p) => ("Hole", p.editing.map(|f| self.feature_name(f))),
            Panel::Pattern(p) => (
                match p.kind {
                    CopyKind::Rect => "Rectangular Pattern",
                    CopyKind::Circular => "Circular Pattern",
                    CopyKind::Mirror => "Mirror",
                },
                p.editing.map(|f| self.feature_name(f)),
            ),
            Panel::Work(w) => (w.title(), w.editing.map(|f| self.feature_name(f))),
            _ => return,
        };
        let mut clear = false;
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(kind).font(theme::heading()).color(t.text));
            ui.label(egui::RichText::new(format!("> {}", name.unwrap_or_else(|| "new".into()))).font(theme::body()).color(t.text_dim));
        });
        ui.add_space(4.0);
        egui::ScrollArea::vertical().id_salt("tn_props_scroll").max_height((r.height() - 110.0).max(80.0)).auto_shrink([false, true]).show(
            ui,
            |ui| {
                ui.spacing_mut().indent = 10.0;
                match &mut panel {
                    Panel::Extrude(p) => {
                        section(ui, "Input Geometry", true, |ui| {
                            egui::Grid::new("tn_props_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Profiles");
                                ui.label(format!("{} selected", self.profile_count(p.sketch, &p.regions)));
                                ui.end_row();
                                ui.label("Sketch");
                                egui::ComboBox::from_id_salt("tn_props_sketch")
                                    .selected_text(sketches.iter().find(|s| s.0 == p.sketch).map_or("?".into(), |s| s.1.clone()))
                                    .show_ui(ui, |ui| {
                                        for (id, name) in &sketches {
                                            if ui.selectable_value(&mut p.sketch, *id, name).clicked() {
                                                p.regions = RegionSel::Default;
                                            }
                                        }
                                    });
                                ui.end_row();
                            });
                        });
                        section(ui, "Behavior", true, |ui| {
                            let through = p.extent == ExtentChoice::ThroughAll;
                            let dirs = [
                                (Glyph::Default, "Default direction", true, Direction::Default),
                                (Glyph::Flipped, "Flipped", true, Direction::Flipped),
                                (Glyph::Symmetric, "Symmetric: half each way", !through, Direction::Symmetric),
                                (Glyph::Asymmetric, "Asymmetric: two distances", !through, Direction::Asymmetric),
                            ];
                            egui::Grid::new("tn_props_behavior").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Direction");
                                let items: Vec<(Glyph, &str, bool)> = dirs.iter().map(|d| (d.0, d.1, d.2)).collect();
                                let sel = dirs.iter().position(|d| d.3 == p.direction).unwrap_or(0);
                                if let Some(i) = glyph_row(ui, &items, sel, t) {
                                    p.direction = dirs[i].3;
                                }
                                ui.end_row();
                                ui.label("Extents");
                                egui::ComboBox::from_id_salt("tn_props_extents")
                                    .selected_text(if through { "Through All" } else { "Distance" })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut p.extent, ExtentChoice::Distance, "Distance");
                                        if ui.selectable_value(&mut p.extent, ExtentChoice::ThroughAll, "Through All").clicked()
                                            && matches!(p.direction, Direction::Symmetric | Direction::Asymmetric)
                                        {
                                            p.direction = Direction::Default;
                                        }
                                        ui.add_enabled(false, egui::Button::selectable(false, "To (M5)"));
                                        ui.add_enabled(false, egui::Button::selectable(false, "Between (M5)"));
                                    });
                                ui.end_row();
                                if p.extent == ExtentChoice::Distance {
                                    ui.label(if p.direction == Direction::Asymmetric { "Distance A" } else { "Distance" });
                                    enter |= eq_value_field(
                                        ui,
                                        egui::Id::new("tn_props_dist"),
                                        &mut p.distance,
                                        &mut eqs,
                                        "distance",
                                        "mm",
                                        0.001..=100_000.0,
                                        110.0,
                                        t,
                                    )
                                    .entered;
                                    ui.end_row();
                                    if p.direction == Direction::Asymmetric {
                                        ui.label("Distance B");
                                        enter |= eq_value_field(
                                            ui,
                                            egui::Id::new("tn_props_dist_b"),
                                            &mut p.distance_b,
                                            &mut eqs,
                                            "distance_b",
                                            "mm",
                                            0.0..=100_000.0,
                                            110.0,
                                            t,
                                        )
                                        .entered;
                                        ui.end_row();
                                    }
                                }
                            });
                        });
                        section(ui, "Output", true, |ui| {
                            egui::Grid::new("tn_props_output").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Boolean");
                                let items: Vec<(Glyph, &str, bool)> = OPS.iter().map(|o| (o.0, o.1, true)).collect();
                                let sel = OPS.iter().position(|o| o.2 == p.operation).unwrap_or(0);
                                if let Some(i) = glyph_row(ui, &items, sel, t) {
                                    p.operation = OPS[i].2;
                                }
                                ui.end_row();
                            });
                        });
                        section(ui, "Advanced Properties", false, |ui| {
                            ui.add_enabled(false, egui::Label::new("Taper (milestone M5)"));
                        });
                    }
                    Panel::Revolve(p) => {
                        let lines: Vec<EntityId> = self
                            .document()
                            .sketch(p.sketch)
                            .map(|s| s.entities().filter(|(id, _)| s.is_line(*id)).map(|(id, _)| id).collect())
                            .unwrap_or_default();
                        section(ui, "Input Geometry", true, |ui| {
                            egui::Grid::new("tn_props_rev_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Profiles");
                                ui.label(format!("{} selected", self.profile_count(p.sketch, &p.regions)));
                                ui.end_row();
                                ui.label("Sketch");
                                egui::ComboBox::from_id_salt("tn_props_rev_sketch")
                                    .selected_text(sketches.iter().find(|s| s.0 == p.sketch).map_or("?".into(), |s| s.1.clone()))
                                    .show_ui(ui, |ui| {
                                        for (id, name) in &sketches {
                                            ui.selectable_value(&mut p.sketch, *id, name);
                                        }
                                    });
                                ui.end_row();
                                ui.label("Axis");
                                let axis_name = |a: &AxisChoice| match a {
                                    AxisChoice::Origin(o) => format!("{o:?} Axis"),
                                    AxisChoice::Line(l) => format!("Sketch line {}", l.0),
                                    AxisChoice::Work(w) => work_names.iter().find(|x| x.0 == *w).map_or("Work Axis".into(), |x| x.1.clone()),
                                };
                                egui::ComboBox::from_id_salt("tn_props_axis").selected_text(axis_name(&p.axis)).show_ui(ui, |ui| {
                                    for o in [OriginAxis::X, OriginAxis::Y, OriginAxis::Z] {
                                        ui.selectable_value(&mut p.axis, AxisChoice::Origin(o), format!("{o:?} Axis"));
                                    }
                                    for l in &lines {
                                        ui.selectable_value(&mut p.axis, AxisChoice::Line(*l), format!("Sketch line {}", l.0));
                                    }
                                    for (w, name, _) in work_names.iter().filter(|x| x.2 == "axis") {
                                        ui.selectable_value(&mut p.axis, AxisChoice::Work(*w), name);
                                    }
                                });
                                ui.end_row();
                            });
                        });
                        section(ui, "Behavior", true, |ui| {
                            egui::Grid::new("tn_props_rev_behavior").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Extents");
                                egui::ComboBox::from_id_salt("tn_props_rev_extents").selected_text(if p.full { "Full" } else { "Angle" }).show_ui(
                                    ui,
                                    |ui| {
                                        ui.selectable_value(&mut p.full, false, "Angle");
                                        ui.selectable_value(&mut p.full, true, "Full");
                                    },
                                );
                                ui.end_row();
                                if !p.full {
                                    ui.label("Direction");
                                    let items = [(Glyph::Default, "Default direction", true), (Glyph::Symmetric, "Symmetric: half each way", true)];
                                    if let Some(i) = glyph_row(ui, &items, usize::from(p.symmetric), t) {
                                        p.symmetric = i == 1;
                                    }
                                    ui.end_row();
                                    ui.label("Angle");
                                    enter |= eq_value_field(
                                        ui,
                                        egui::Id::new("tn_props_angle"),
                                        &mut p.degrees,
                                        &mut eqs,
                                        "degrees",
                                        "deg",
                                        0.1..=360.0,
                                        110.0,
                                        t,
                                    )
                                    .entered;
                                    ui.end_row();
                                }
                            });
                        });
                        section(ui, "Output", true, |ui| {
                            egui::Grid::new("tn_props_rev_output").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Boolean");
                                let items: Vec<(Glyph, &str, bool)> = OPS.iter().map(|o| (o.0, o.1, true)).collect();
                                let sel = OPS.iter().position(|o| o.2 == p.operation).unwrap_or(0);
                                if let Some(i) = glyph_row(ui, &items, sel, t) {
                                    p.operation = OPS[i].2;
                                }
                                ui.end_row();
                            });
                        });
                    }
                    Panel::Fillet(p) => {
                        section(ui, "Input Geometry", true, |ui| {
                            egui::Grid::new("tn_props_fillet_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Edges");
                                clear |= picked_row(ui, p.edges.len(), "click edges");
                                ui.end_row();
                            });
                        });
                        section(ui, "Behavior", true, |ui| {
                            egui::Grid::new("tn_props_fillet_behavior").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Radius");
                                enter |= eq_value_field(
                                    ui,
                                    egui::Id::new("tn_props_radius"),
                                    &mut p.radius,
                                    &mut eqs,
                                    "radius",
                                    "mm",
                                    0.001..=100_000.0,
                                    110.0,
                                    t,
                                )
                                .entered;
                                ui.end_row();
                            });
                        });
                    }
                    Panel::Chamfer(p) => {
                        section(ui, "Input Geometry", true, |ui| {
                            egui::Grid::new("tn_props_chamfer_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Edges");
                                clear |= picked_row(ui, p.edges.len(), "click edges");
                                ui.end_row();
                                if p.method != ChamferMethod::Distance {
                                    ui.label("Face");
                                    ui.label(if p.reference.is_some() { "1 selected" } else { "click a face" })
                                        .on_hover_text("The face the first distance is measured on");
                                    ui.end_row();
                                }
                            });
                        });
                        section(ui, "Behavior", true, |ui| {
                            egui::Grid::new("tn_props_chamfer_behavior").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Method");
                                let label = |m: ChamferMethod| match m {
                                    ChamferMethod::Distance => "Distance",
                                    ChamferMethod::TwoDistances => "Two Distances",
                                    ChamferMethod::DistanceAngle => "Distance and Angle",
                                };
                                egui::ComboBox::from_id_salt("tn_props_chamfer_method").selected_text(label(p.method)).show_ui(ui, |ui| {
                                    for m in [ChamferMethod::Distance, ChamferMethod::TwoDistances, ChamferMethod::DistanceAngle] {
                                        ui.selectable_value(&mut p.method, m, label(m));
                                    }
                                });
                                ui.end_row();
                                ui.label(if p.method == ChamferMethod::TwoDistances { "Distance 1" } else { "Distance" });
                                enter |= eq_value_field(
                                    ui,
                                    egui::Id::new("tn_props_chamfer_d1"),
                                    &mut p.d1,
                                    &mut eqs,
                                    "d1",
                                    "mm",
                                    0.001..=100_000.0,
                                    110.0,
                                    t,
                                )
                                .entered;
                                ui.end_row();
                                match p.method {
                                    ChamferMethod::TwoDistances => {
                                        ui.label("Distance 2");
                                        enter |= eq_value_field(
                                            ui,
                                            egui::Id::new("tn_props_chamfer_d2"),
                                            &mut p.d2,
                                            &mut eqs,
                                            "d2",
                                            "mm",
                                            0.001..=100_000.0,
                                            110.0,
                                            t,
                                        )
                                        .entered;
                                        ui.end_row();
                                    }
                                    ChamferMethod::DistanceAngle => {
                                        ui.label("Angle");
                                        enter |= eq_value_field(
                                            ui,
                                            egui::Id::new("tn_props_chamfer_angle"),
                                            &mut p.degrees,
                                            &mut eqs,
                                            "degrees",
                                            "deg",
                                            0.1..=89.9,
                                            110.0,
                                            t,
                                        )
                                        .entered;
                                        ui.end_row();
                                    }
                                    ChamferMethod::Distance => {}
                                }
                            });
                        });
                    }
                    Panel::Shell(p) => {
                        section(ui, "Input Geometry", true, |ui| {
                            egui::Grid::new("tn_props_shell_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Remove Faces");
                                clear |= picked_row(ui, p.faces.len(), "click faces");
                                ui.end_row();
                            });
                        });
                        section(ui, "Behavior", true, |ui| {
                            egui::Grid::new("tn_props_shell_behavior").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Direction");
                                egui::ComboBox::from_id_salt("tn_props_shell_dir")
                                    .selected_text(if p.outside { "Outside" } else { "Inside" })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut p.outside, false, "Inside");
                                        ui.selectable_value(&mut p.outside, true, "Outside");
                                    });
                                ui.end_row();
                                ui.label("Thickness");
                                enter |= eq_value_field(
                                    ui,
                                    egui::Id::new("tn_props_thickness"),
                                    &mut p.thickness,
                                    &mut eqs,
                                    "thickness",
                                    "mm",
                                    0.001..=100_000.0,
                                    110.0,
                                    t,
                                )
                                .entered;
                                ui.end_row();
                            });
                        });
                    }
                    Panel::Hole(p) => {
                        section(ui, "Input Geometry", true, |ui| {
                            egui::Grid::new("tn_props_hole_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Positions");
                                clear |= picked_row(ui, p.points.len(), "click sketch points");
                                ui.end_row();
                                ui.label("Sketch");
                                ui.label(sketches.iter().find(|s| s.0 == p.sketch).map_or("?".into(), |s| s.1.clone()));
                                ui.end_row();
                            });
                        });
                        section(ui, "Type", true, |ui| {
                            egui::Grid::new("tn_props_hole_type").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Hole");
                                egui::ComboBox::from_id_salt("tn_props_hole_kind").selected_text("Simple").show_ui(ui, |ui| {
                                    let _ = ui.selectable_label(true, "Simple");
                                    ui.add_enabled(false, egui::Button::selectable(false, "Clearance (M5)"));
                                    ui.add_enabled(false, egui::Button::selectable(false, "Tapped (M5)"));
                                });
                                ui.end_row();
                                ui.label("Seat");
                                let seat_name = |s: Seat| match s {
                                    Seat::None => "None",
                                    Seat::Counterbore => "Counterbore",
                                    Seat::Countersink => "Countersink",
                                };
                                egui::ComboBox::from_id_salt("tn_props_hole_seat").selected_text(seat_name(p.seat)).show_ui(ui, |ui| {
                                    for s in [Seat::None, Seat::Counterbore, Seat::Countersink] {
                                        ui.selectable_value(&mut p.seat, s, seat_name(s));
                                    }
                                });
                                ui.end_row();
                            });
                        });
                        section(ui, "Behavior", true, |ui| {
                            egui::Grid::new("tn_props_hole_behavior").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Termination");
                                egui::ComboBox::from_id_salt("tn_props_hole_term")
                                    .selected_text(if p.through { "Through All" } else { "Distance" })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut p.through, false, "Distance");
                                        ui.selectable_value(&mut p.through, true, "Through All");
                                    });
                                ui.end_row();
                                ui.label("Direction");
                                let items =
                                    [(Glyph::Flipped, "Default: into the part, against the sketch normal", true), (Glyph::Default, "Flipped", true)];
                                if let Some(i) = glyph_row(ui, &items, usize::from(p.reverse), t) {
                                    p.reverse = i == 1;
                                }
                                ui.end_row();
                                if !p.through {
                                    ui.label("Drill Point");
                                    egui::ComboBox::from_id_salt("tn_props_hole_point").selected_text(if p.flat { "Flat" } else { "Angle" }).show_ui(
                                        ui,
                                        |ui| {
                                            ui.selectable_value(&mut p.flat, true, "Flat");
                                            ui.selectable_value(&mut p.flat, false, "Angle");
                                        },
                                    );
                                    ui.end_row();
                                }
                            });
                        });
                        section(ui, "Dimensions", true, |ui| {
                            egui::Grid::new("tn_props_hole_dims").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Diameter");
                                enter |= eq_value_field(
                                    ui,
                                    egui::Id::new("tn_props_hole_dia"),
                                    &mut p.diameter,
                                    &mut eqs,
                                    "diameter",
                                    "mm",
                                    0.001..=100_000.0,
                                    110.0,
                                    t,
                                )
                                .entered;
                                ui.end_row();
                                if !p.through {
                                    ui.label("Depth");
                                    enter |= eq_value_field(
                                        ui,
                                        egui::Id::new("tn_props_hole_depth"),
                                        &mut p.depth,
                                        &mut eqs,
                                        "depth",
                                        "mm",
                                        0.001..=100_000.0,
                                        110.0,
                                        t,
                                    )
                                    .entered;
                                    ui.end_row();
                                }
                                match p.seat {
                                    Seat::None => {}
                                    Seat::Counterbore => {
                                        ui.label("Bore Diameter");
                                        enter |= eq_value_field(
                                            ui,
                                            egui::Id::new("tn_props_seat_dia"),
                                            &mut p.seat_diameter,
                                            &mut eqs,
                                            "seat_diameter",
                                            "mm",
                                            0.001..=100_000.0,
                                            110.0,
                                            t,
                                        )
                                        .entered;
                                        ui.end_row();
                                        ui.label("Bore Depth");
                                        enter |= eq_value_field(
                                            ui,
                                            egui::Id::new("tn_props_bore_depth"),
                                            &mut p.bore_depth,
                                            &mut eqs,
                                            "bore_depth",
                                            "mm",
                                            0.001..=100_000.0,
                                            110.0,
                                            t,
                                        )
                                        .entered;
                                        ui.end_row();
                                    }
                                    Seat::Countersink => {
                                        ui.label("Sink Diameter");
                                        enter |= eq_value_field(
                                            ui,
                                            egui::Id::new("tn_props_seat_dia"),
                                            &mut p.seat_diameter,
                                            &mut eqs,
                                            "seat_diameter",
                                            "mm",
                                            0.001..=100_000.0,
                                            110.0,
                                            t,
                                        )
                                        .entered;
                                        ui.end_row();
                                        ui.label("Sink Angle");
                                        enter |= eq_value_field(
                                            ui,
                                            egui::Id::new("tn_props_sink_angle"),
                                            &mut p.sink_degrees,
                                            &mut eqs,
                                            "sink_degrees",
                                            "deg",
                                            1.0..=179.0,
                                            110.0,
                                            t,
                                        )
                                        .entered;
                                        ui.end_row();
                                    }
                                }
                                if !p.through && !p.flat {
                                    ui.label("Point Angle");
                                    enter |= eq_value_field(
                                        ui,
                                        egui::Id::new("tn_props_tip_angle"),
                                        &mut p.tip_degrees,
                                        &mut eqs,
                                        "tip_degrees",
                                        "deg",
                                        1.0..=179.0,
                                        110.0,
                                        t,
                                    )
                                    .entered;
                                    ui.end_row();
                                }
                            });
                        });
                    }
                    Panel::Pattern(p) => {
                        section(ui, "Input Geometry", true, |ui| {
                            egui::Grid::new("tn_props_pattern_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Features");
                                ui.horizontal(|ui| {
                                    let label =
                                        if p.features.is_empty() { "click features".to_string() } else { format!("{} selected", p.features.len()) };
                                    if slot_button(ui, p.slot == Slot::Features, &label, t) {
                                        p.slot = Slot::Features;
                                    }
                                    if !p.features.is_empty() && ui.small_button("Clear").clicked() {
                                        clear = true;
                                    }
                                })
                                .response
                                .on_hover_text(p.features.iter().map(|f| self.feature_name(*f)).collect::<Vec<_>>().join(", "));
                                ui.end_row();
                            });
                        });
                        match p.kind {
                            CopyKind::Rect => {
                                section(ui, "Direction 1", true, |ui| {
                                    egui::Grid::new("tn_props_dir1").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                        ui.label("Direction");
                                        ui.horizontal(|ui| {
                                            let mut d = Some(p.dir1.clone());
                                            if direction_picker(ui, "tn_dir1", &mut d, false, p.slot == Slot::Dir1, &work_names, t) {
                                                p.slot = Slot::Dir1;
                                            }
                                            if let Some(d) = d {
                                                p.dir1 = d;
                                            }
                                            flip_button(ui, &mut p.reverse1, t);
                                        });
                                        ui.end_row();
                                        ui.label("Count");
                                        enter |= eq_value_field(
                                            ui,
                                            egui::Id::new("tn_props_count1"),
                                            &mut p.count1,
                                            &mut eqs,
                                            "count1",
                                            "",
                                            1.0..=10_000.0,
                                            110.0,
                                            t,
                                        )
                                        .entered;
                                        ui.end_row();
                                        ui.label("Spacing");
                                        enter |= eq_value_field(
                                            ui,
                                            egui::Id::new("tn_props_spacing1"),
                                            &mut p.spacing1,
                                            &mut eqs,
                                            "spacing1",
                                            "mm",
                                            0.001..=100_000.0,
                                            110.0,
                                            t,
                                        )
                                        .entered;
                                        ui.end_row();
                                    });
                                });
                                section(ui, "Direction 2", p.dir2.is_some(), |ui| {
                                    egui::Grid::new("tn_props_dir2").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                        ui.label("Direction");
                                        ui.horizontal(|ui| {
                                            let had = p.dir2.is_some();
                                            if direction_picker(ui, "tn_dir2", &mut p.dir2, true, p.slot == Slot::Dir2, &work_names, t) {
                                                p.slot = Slot::Dir2;
                                            }
                                            if !had && p.dir2.is_some() && p.count2 < 2.0 {
                                                p.count2 = 2.0;
                                            }
                                            flip_button(ui, &mut p.reverse2, t);
                                        });
                                        ui.end_row();
                                        if p.dir2.is_some() {
                                            ui.label("Count");
                                            enter |= eq_value_field(
                                                ui,
                                                egui::Id::new("tn_props_count2"),
                                                &mut p.count2,
                                                &mut eqs,
                                                "count2",
                                                "",
                                                1.0..=10_000.0,
                                                110.0,
                                                t,
                                            )
                                            .entered;
                                            ui.end_row();
                                            ui.label("Spacing");
                                            enter |= eq_value_field(
                                                ui,
                                                egui::Id::new("tn_props_spacing2"),
                                                &mut p.spacing2,
                                                &mut eqs,
                                                "spacing2",
                                                "mm",
                                                0.001..=100_000.0,
                                                110.0,
                                                t,
                                            )
                                            .entered;
                                            ui.end_row();
                                        }
                                    });
                                });
                            }
                            CopyKind::Circular => {
                                section(ui, "Placement", true, |ui| {
                                    egui::Grid::new("tn_props_circ").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                        ui.label("Axis");
                                        ui.horizontal(|ui| {
                                            let mut a = Some(p.axis.clone());
                                            if axis_picker(ui, "tn_props_axis_origin", &mut a, p.slot == Slot::Axis, &work_names, t) {
                                                p.slot = Slot::Axis;
                                            }
                                            if let Some(a) = a {
                                                p.axis = a;
                                            }
                                            flip_button(ui, &mut p.reverse, t);
                                        });
                                        ui.end_row();
                                        ui.label("Count");
                                        enter |= eq_value_field(
                                            ui,
                                            egui::Id::new("tn_props_count"),
                                            &mut p.count,
                                            &mut eqs,
                                            "count",
                                            "",
                                            2.0..=10_000.0,
                                            110.0,
                                            t,
                                        )
                                        .entered;
                                        ui.end_row();
                                        ui.label("Angle");
                                        enter |= eq_value_field(
                                            ui,
                                            egui::Id::new("tn_props_pattern_angle"),
                                            &mut p.degrees,
                                            &mut eqs,
                                            "degrees",
                                            "deg",
                                            0.1..=360.0,
                                            110.0,
                                            t,
                                        )
                                        .entered;
                                        ui.end_row();
                                    });
                                });
                            }
                            CopyKind::Mirror => {
                                section(ui, "Mirror Plane", true, |ui| {
                                    egui::Grid::new("tn_props_mirror").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                        ui.label("Plane");
                                        ui.horizontal(|ui| {
                                            let mut pl = Some(p.plane.clone());
                                            if plane_picker(ui, "tn_props_plane_origin", &mut pl, p.slot == Slot::Plane, &work_names, t) {
                                                p.slot = Slot::Plane;
                                            }
                                            if let Some(pl) = pl {
                                                p.plane = pl;
                                            }
                                        });
                                        ui.end_row();
                                    });
                                });
                            }
                        }
                    }
                    Panel::Work(w) => {
                        section(ui, "Placement", true, |ui| {
                            egui::Grid::new("tn_props_work").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                ui.label("Method");
                                egui::ComboBox::from_id_salt("tn_props_work_method").selected_text(w.method.label()).show_ui(ui, |ui| {
                                    for m in w.method.family() {
                                        if ui.selectable_label(w.method == *m, m.label()).clicked() && w.method != *m {
                                            w.method = *m;
                                            w.slot = m.slots()[0];
                                            w.advance();
                                        }
                                    }
                                });
                                ui.end_row();
                                let two = w.method.slots().contains(&WorkSlot::B);
                                for slot in w.method.slots() {
                                    match slot {
                                        WorkSlot::A => {
                                            ui.label(if two { "Plane 1" } else { "Plane" });
                                            if plane_picker(ui, "tn_work_a", &mut w.a, w.slot == WorkSlot::A, &work_names, t) {
                                                w.slot = WorkSlot::A;
                                            }
                                        }
                                        WorkSlot::B => {
                                            ui.label("Plane 2");
                                            if plane_picker(ui, "tn_work_b", &mut w.b, w.slot == WorkSlot::B, &work_names, t) {
                                                w.slot = WorkSlot::B;
                                            }
                                        }
                                        WorkSlot::Axis => {
                                            ui.label("Axis");
                                            if axis_picker(ui, "tn_work_axis", &mut w.axis, w.slot == WorkSlot::Axis, &work_names, t) {
                                                w.slot = WorkSlot::Axis;
                                            }
                                        }
                                        WorkSlot::Edge => {
                                            ui.label("Edge");
                                            let label = if w.edge.is_some() { "1 selected" } else { "click a circular edge" };
                                            if slot_button(ui, w.slot == WorkSlot::Edge, label, t) {
                                                w.slot = WorkSlot::Edge;
                                            }
                                        }
                                    }
                                    ui.end_row();
                                }
                                match w.method {
                                    WorkMethod::Offset => {
                                        ui.label("Offset");
                                        enter |= eq_value_field(
                                            ui,
                                            egui::Id::new("tn_props_work_offset"),
                                            &mut w.distance,
                                            &mut eqs,
                                            "distance",
                                            "mm",
                                            -100_000.0..=100_000.0,
                                            110.0,
                                            t,
                                        )
                                        .entered;
                                        ui.end_row();
                                    }
                                    WorkMethod::Angle => {
                                        ui.label("Angle");
                                        enter |= eq_value_field(
                                            ui,
                                            egui::Id::new("tn_props_work_angle"),
                                            &mut w.degrees,
                                            &mut eqs,
                                            "degrees",
                                            "deg",
                                            -360.0..=360.0,
                                            110.0,
                                            t,
                                        )
                                        .entered;
                                        ui.end_row();
                                    }
                                    _ => {}
                                }
                            });
                        });
                    }
                    _ => {}
                }
            },
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let ok = ui.add(egui::Button::new(egui::RichText::new("OK").color(t.accent_text)).fill(t.accent).min_size(vec2(64.0, 24.0)));
            if ok.clicked() {
                request = Some(PanelRequest::Ok);
            }
            if ui.add(egui::Button::new("Cancel").min_size(vec2(64.0, 24.0))).clicked() {
                request = Some(PanelRequest::Cancel);
            }
            if ui.add(egui::Button::new("+").min_size(vec2(28.0, 24.0))).on_hover_text("Apply, then start another").clicked() {
                request = Some(PanelRequest::Apply);
            }
        });
        if enter && request.is_none() {
            request = Some(PanelRequest::Ok);
        }
        // Keep edits unless the panel changed underneath (e.g. it was closed this frame).
        if self.panel.is_some() {
            self.panel = Some(panel);
            self.panel_eqs = eqs;
        }
        if clear {
            self.clear_panel_picks();
        }
        if request.is_some() {
            self.panel_request = request;
        }
    }

    /// Where the preview's profile sits: its centre and outward direction in 3D.
    pub(crate) fn profile_anchor(&self, sketch: tenon_model::FeatureId) -> Option<(Vec3, Vec3)> {
        let frame = self.sketch_frame(sketch)?;
        let sk = self.document().sketch(sketch)?;
        let pts: Vec<_> = sk.entities().filter_map(|(id, _)| sk.point(id)).collect();
        if pts.is_empty() {
            return None;
        }
        let c = pts.iter().fold(tenon_geom::Vec2::new(0.0, 0.0), |a, p| a + *p) * (1.0 / pts.len() as f64);
        Some((frame.plane_point(c), frame.z()))
    }

    /// The distance arrow on the extrusion preview: drag its tip to change the distance.
    pub(crate) fn manipulator(&mut self, ui: &Ui, rect: Rect, t: &Tokens) {
        let Some(Panel::Extrude(p)) = &self.panel else { return };
        if p.extent != ExtentChoice::Distance {
            return;
        }
        let Some((base, n)) = self.profile_anchor(p.sketch) else { return };
        let dir = if p.direction == Direction::Flipped { -n } else { n };
        let shown = match p.direction {
            Direction::Symmetric => p.distance / 2.0,
            _ => p.distance,
        };
        let tip = base + dir * shown;
        let cam = self.view.camera;
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let screen = |q: Vec3| cam.project(q, w, h).map(|(x, y, _)| rect.min + vec2(x as f32, y as f32));
        let (Some(a), Some(b)) = (screen(base), screen(tip)) else { return };
        let along = b - a;
        let len = along.length();
        let handle = Rect::from_center_size(b, vec2(16.0, 16.0));
        let resp = ui.interact(handle, ui.id().with("tn_manipulator"), Sense::DRAG);
        let hot = resp.hovered() || resp.dragged();
        let color = if hot { t.accent } else { Color32::from_rgb(0xf0, 0xb4, 0x3c) };
        let p = ui.painter();
        p.line_segment([a, b], Stroke::new(2.0, color));
        if len > 1.0 {
            let d = along / len;
            let nrm = vec2(-d.y, d.x);
            p.add(Shape::convex_polygon(vec![b + d * 9.0, b - d * 3.0 + nrm * 6.0, b - d * 3.0 - nrm * 6.0], color, Stroke::NONE));
        }
        p.circle_filled(a, 3.0, color);
        if resp.dragged() && len > 1.0 {
            let mm_per_px = shown / f64::from(len);
            let moved = f64::from(resp.drag_delta().dot(along / len)) * mm_per_px;
            let k = if p_direction_symmetric(&self.panel) { 2.0 } else { 1.0 };
            if let Some(Panel::Extrude(p)) = &mut self.panel {
                p.distance = (p.distance + moved * k).clamp(0.001, 100_000.0);
            }
        }
        let _ = resp.on_hover_text("Drag to change the distance");
    }

    /// The mini-toolbar beside the preview: the main value, flip, OK, Cancel and Apply.
    pub(crate) fn mini_toolbar(&mut self, ui: &Ui, rect: Rect, t: &Tokens) {
        let at3 = match &self.panel {
            Some(Panel::Extrude(p)) => self.profile_anchor(p.sketch).map(|a| a.0),
            Some(Panel::Revolve(p)) => self.profile_anchor(p.sketch).map(|a| a.0),
            Some(Panel::Fillet(_) | Panel::Chamfer(_) | Panel::Shell(_)) => self.panel_anchor().or_else(|| self.scene.bbox().map(|b| b.center())),
            Some(Panel::Hole(p)) => p
                .points
                .first()
                .and_then(|e| Some(self.sketch_frame(p.sketch)?.plane_point(self.document().sketch(p.sketch)?.point(*e)?)))
                .or_else(|| self.scene.bbox().map(|b| b.center())),
            Some(Panel::Pattern(_) | Panel::Work(_)) => self.scene.bbox().map(|b| Vec3::new(b.max.x, b.min.y, b.max.z)),
            _ => return,
        };
        let is_extrude = matches!(self.panel, Some(Panel::Extrude(_)));
        let anchor: Pos2 = at3
            .and_then(|c| {
                let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
                self.view.camera.project(c, w, h).map(|(x, y, _)| rect.min + vec2(x as f32 + 70.0, y as f32 + 45.0))
            })
            .unwrap_or(rect.center());
        let at = pos2(anchor.x.clamp(rect.left() + 8.0, rect.right() - 260.0), anchor.y.clamp(rect.top() + 8.0, rect.bottom() - 40.0));
        let mut request = None;
        let mut panel = self.panel.clone();
        let mut eqs = self.panel_eqs.clone();
        egui::Area::new(egui::Id::new("tn_minibar")).order(egui::Order::Middle).fixed_pos(at).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).fill(t.panel).inner_margin(4.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    match &mut panel {
                        Some(Panel::Extrude(p)) if p.extent == ExtentChoice::Distance => {
                            if eq_value_field(
                                ui,
                                egui::Id::new("tn_mini_dist"),
                                &mut p.distance,
                                &mut eqs,
                                "distance",
                                "mm",
                                0.001..=100_000.0,
                                80.0,
                                t,
                            )
                            .entered
                            {
                                request = Some(PanelRequest::Ok);
                            }
                        }
                        Some(Panel::Extrude(_)) => {
                            ui.label("Through All");
                        }
                        Some(Panel::Revolve(p)) if !p.full => {
                            if eq_value_field(ui, egui::Id::new("tn_mini_angle"), &mut p.degrees, &mut eqs, "degrees", "deg", 0.1..=360.0, 80.0, t)
                                .entered
                            {
                                request = Some(PanelRequest::Ok);
                            }
                        }
                        Some(Panel::Revolve(_)) => {
                            ui.label("Full");
                        }
                        Some(Panel::Fillet(p)) => {
                            if eq_value_field(
                                ui,
                                egui::Id::new("tn_mini_radius"),
                                &mut p.radius,
                                &mut eqs,
                                "radius",
                                "mm",
                                0.001..=100_000.0,
                                80.0,
                                t,
                            )
                            .entered
                            {
                                request = Some(PanelRequest::Ok);
                            }
                        }
                        Some(Panel::Chamfer(p)) => {
                            if eq_value_field(ui, egui::Id::new("tn_mini_chamfer"), &mut p.d1, &mut eqs, "d1", "mm", 0.001..=100_000.0, 80.0, t)
                                .entered
                            {
                                request = Some(PanelRequest::Ok);
                            }
                        }
                        Some(Panel::Shell(p)) => {
                            if eq_value_field(
                                ui,
                                egui::Id::new("tn_mini_thickness"),
                                &mut p.thickness,
                                &mut eqs,
                                "thickness",
                                "mm",
                                0.001..=100_000.0,
                                80.0,
                                t,
                            )
                            .entered
                            {
                                request = Some(PanelRequest::Ok);
                            }
                        }
                        Some(Panel::Hole(p)) => {
                            if eq_value_field(
                                ui,
                                egui::Id::new("tn_mini_hole_dia"),
                                &mut p.diameter,
                                &mut eqs,
                                "diameter",
                                "mm",
                                0.001..=100_000.0,
                                80.0,
                                t,
                            )
                            .entered
                            {
                                request = Some(PanelRequest::Ok);
                            }
                        }
                        Some(Panel::Work(w)) if w.method == WorkMethod::Offset => {
                            if eq_value_field(
                                ui,
                                egui::Id::new("tn_mini_work"),
                                &mut w.distance,
                                &mut eqs,
                                "distance",
                                "mm",
                                -100_000.0..=100_000.0,
                                80.0,
                                t,
                            )
                            .entered
                            {
                                request = Some(PanelRequest::Ok);
                            }
                        }
                        Some(Panel::Work(w)) if w.method == WorkMethod::Angle => {
                            if eq_value_field(ui, egui::Id::new("tn_mini_work"), &mut w.degrees, &mut eqs, "degrees", "deg", -360.0..=360.0, 80.0, t)
                                .entered
                            {
                                request = Some(PanelRequest::Ok);
                            }
                        }
                        Some(Panel::Pattern(p)) if p.kind != CopyKind::Mirror => {
                            let (count, key) = if p.kind == CopyKind::Rect { (&mut p.count1, "count1") } else { (&mut p.count, "count") };
                            if eq_value_field(ui, egui::Id::new("tn_mini_count"), count, &mut eqs, key, "", 1.0..=10_000.0, 60.0, t).entered {
                                request = Some(PanelRequest::Ok);
                            }
                        }
                        _ => {}
                    }
                    if is_extrude
                        && let Some(Panel::Extrude(p)) = &mut panel
                        && ui.small_button("Flip").on_hover_text("Flip direction").clicked()
                    {
                        p.direction = if p.direction == Direction::Flipped { Direction::Default } else { Direction::Flipped };
                    }
                    if ui.add(egui::Button::new(egui::RichText::new("✔").color(t.ok)).small()).on_hover_text("OK (Enter)").clicked() {
                        request = Some(PanelRequest::Ok);
                    }
                    if ui.add(egui::Button::new(egui::RichText::new("✖").color(t.history_marker)).small()).on_hover_text("Cancel (Esc)").clicked() {
                        request = Some(PanelRequest::Cancel);
                    }
                    if ui.small_button("+").on_hover_text("Apply, then start another").clicked() {
                        request = Some(PanelRequest::Apply);
                    }
                });
            });
        });
        if self.panel.is_some() {
            self.panel = panel;
            self.panel_eqs = eqs;
        }
        if request.is_some() {
            self.panel_request = request;
        }
    }
}

/// True if feature `id` comes before the feature being edited (so the panel may refer to it).
fn p_before(panel: &Panel, wb: &Workbench, id: tenon_model::FeatureId) -> bool {
    let editing = match panel {
        Panel::Revolve(p) => p.editing,
        Panel::Pattern(p) => p.editing,
        Panel::Work(w) => w.editing,
        _ => None,
    };
    editing.is_none_or(|e| wb.document().index_of(id) < wb.document().index_of(e))
}

fn p_direction_symmetric(panel: &Option<Panel>) -> bool {
    matches!(panel, Some(Panel::Extrude(p)) if p.direction == Direction::Symmetric)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_parse_with_or_without_units() {
        assert_eq!(parse_value("12.5", "mm"), Some(12.5));
        assert_eq!(parse_value(" 12.5 mm ", "mm"), Some(12.5));
        assert_eq!(parse_value("90 deg", "deg"), Some(90.0));
        assert_eq!(parse_value("abc", "mm"), None);
        assert_eq!(parse_value("inf", "mm"), None);
        assert_eq!(fmt_value(10.0), "10");
        assert_eq!(fmt_value(2.5), "2.5");
        assert_eq!(fmt_value(1.0 / 3.0), "0.333");
    }
}

//! The properties panel (docked above the browser while a feature command runs), the
//! mini-toolbar next to the preview, and the drag arrow that sets a distance in the viewport.

use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use tenon_geom::Vec3;
use tenon_model::{Operation, OriginAxis, RegionSel};
use tenon_sketch::EntityId;

use crate::panels::{AxisChoice, Direction, ExtentChoice, Panel, PanelRequest};
use crate::theme::{self, Tokens};
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

/// A value box with its unit, as in the properties panel and mini-toolbar. Typing edits the text;
/// a valid number applies at once (so the preview follows), an invalid one shows red.
pub(crate) fn value_field(
    ui: &mut Ui,
    id: egui::Id,
    value: &mut f64,
    unit: &str,
    range: std::ops::RangeInclusive<f64>,
    width: f32,
    t: &Tokens,
) -> FieldOut {
    let mut out = FieldOut::default();
    let focused = ui.memory(|m| m.has_focus(id));
    let mut text: String =
        if focused { ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| fmt_value(*value)) } else { format!("{} {unit}", fmt_value(*value)) };
    let valid = !focused || parse_value(&text, unit).is_some_and(|v| range.contains(&v));
    let resp = ui.add(egui::TextEdit::singleline(&mut text).id(id).desired_width(width).font(theme::body()).text_color(if valid {
        t.text
    } else {
        t.history_marker
    }));
    if resp.gained_focus() {
        // Select the whole value, so typing replaces it.
        text = fmt_value(*value);
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
        && let Some(v) = parse_value(&text, unit).filter(|v| range.contains(v))
        && (v - *value).abs() > 0.0
    {
        *value = v;
        out.changed = true;
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
        matches!(self.panel, Some(Panel::Extrude(_) | Panel::Revolve(_)))
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
        let mut request = None;
        let mut enter = ui.input(|i| i.key_pressed(egui::Key::Enter)) && !ui.ctx().egui_wants_keyboard_input();
        let (kind, name) = match &panel {
            Panel::Extrude(p) => ("Extrusion", p.editing.map(|f| self.feature_name(f))),
            Panel::Revolve(p) => ("Revolution", p.editing.map(|f| self.feature_name(f))),
            _ => return,
        };
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
                                    enter |=
                                        value_field(ui, egui::Id::new("tn_props_dist"), &mut p.distance, "mm", 0.001..=100_000.0, 110.0, t).entered;
                                    ui.end_row();
                                    if p.direction == Direction::Asymmetric {
                                        ui.label("Distance B");
                                        enter |=
                                            value_field(ui, egui::Id::new("tn_props_dist_b"), &mut p.distance_b, "mm", 0.0..=100_000.0, 110.0, t)
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
                                };
                                egui::ComboBox::from_id_salt("tn_props_axis").selected_text(axis_name(&p.axis)).show_ui(ui, |ui| {
                                    for o in [OriginAxis::X, OriginAxis::Y, OriginAxis::Z] {
                                        ui.selectable_value(&mut p.axis, AxisChoice::Origin(o), format!("{o:?} Axis"));
                                    }
                                    for l in &lines {
                                        ui.selectable_value(&mut p.axis, AxisChoice::Line(*l), format!("Sketch line {}", l.0));
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
                                    enter |= value_field(ui, egui::Id::new("tn_props_angle"), &mut p.degrees, "deg", 0.1..=360.0, 110.0, t).entered;
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
        let (sketch, is_extrude) = match &self.panel {
            Some(Panel::Extrude(p)) => (p.sketch, true),
            Some(Panel::Revolve(p)) => (p.sketch, false),
            _ => return,
        };
        let anchor: Pos2 = self
            .profile_anchor(sketch)
            .and_then(|(c, _)| {
                let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
                self.view.camera.project(c, w, h).map(|(x, y, _)| rect.min + vec2(x as f32 + 70.0, y as f32 + 45.0))
            })
            .unwrap_or(rect.center());
        let at = pos2(anchor.x.clamp(rect.left() + 8.0, rect.right() - 260.0), anchor.y.clamp(rect.top() + 8.0, rect.bottom() - 40.0));
        let mut request = None;
        let mut panel = self.panel.clone();
        egui::Area::new(egui::Id::new("tn_minibar")).order(egui::Order::Middle).fixed_pos(at).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).fill(t.panel).inner_margin(4.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    match &mut panel {
                        Some(Panel::Extrude(p)) if p.extent == ExtentChoice::Distance => {
                            if value_field(ui, egui::Id::new("tn_mini_dist"), &mut p.distance, "mm", 0.001..=100_000.0, 80.0, t).entered {
                                request = Some(PanelRequest::Ok);
                            }
                        }
                        Some(Panel::Extrude(_)) => {
                            ui.label("Through All");
                        }
                        Some(Panel::Revolve(p)) if !p.full => {
                            if value_field(ui, egui::Id::new("tn_mini_angle"), &mut p.degrees, "deg", 0.1..=360.0, 80.0, t).entered {
                                request = Some(PanelRequest::Ok);
                            }
                        }
                        Some(Panel::Revolve(_)) => {
                            ui.label("Full");
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
        }
        if request.is_some() {
            self.panel_request = request;
        }
    }
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

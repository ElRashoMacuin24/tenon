//! Sweep, Coil and Loft (M6): their properties panels, what a new one starts with, and what each
//! makes of its fields.

use egui::Ui;
use tenon_model::{AxisRef, Coil, Document, FeatureId, FeatureKind, Loft, Operation, OriginAxis, RegionSel, SketchCurves, Sweep};
use tenon_sketch::Geometry;

use crate::Workbench;
use crate::panels::{AxisChoice, Panel};
use crate::properties::{Equations, OPS, eq_value_field, glyph_row, section};
use crate::theme::Tokens;
use crate::workbench::Mode;

/// A sweep being made or edited.
#[derive(Clone, Debug)]
pub(crate) struct SweepPanel {
    pub editing: Option<FeatureId>,
    pub sketch: FeatureId,
    pub regions: RegionSel,
    /// The path's sketch and curves; None until there is one.
    pub path: Option<SketchCurves>,
    pub fixed: bool,
    pub operation: Operation,
}

impl SweepPanel {
    pub(crate) fn kind(&self) -> Option<FeatureKind> {
        let path = self.path.clone()?;
        Some(FeatureKind::Sweep(Sweep { sketch: self.sketch, regions: self.regions.clone(), path, fixed: self.fixed, operation: self.operation }))
    }
}

/// A coil being made or edited.
#[derive(Clone, Debug)]
pub(crate) struct CoilPanel {
    pub editing: Option<FeatureId>,
    pub sketch: FeatureId,
    pub regions: RegionSel,
    pub axis: AxisChoice,
    pub pitch: f64,
    pub turns: f64,
    pub left: bool,
    pub operation: Operation,
}

impl CoilPanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        let axis = match self.axis {
            AxisChoice::Origin(a) => AxisRef::Origin(a),
            AxisChoice::Line(l) => AxisRef::SketchLine(l),
            AxisChoice::Work(w) => AxisRef::Work(w),
        };
        FeatureKind::Coil(Coil {
            sketch: self.sketch,
            regions: self.regions.clone(),
            axis,
            pitch: self.pitch,
            turns: self.turns,
            left: self.left,
            operation: self.operation,
        })
    }
}

/// A loft being made or edited.
#[derive(Clone, Debug)]
pub(crate) struct LoftPanel {
    pub editing: Option<FeatureId>,
    /// In order.
    pub sections: Vec<FeatureId>,
    pub ruled: bool,
    pub operation: Operation,
}

impl LoftPanel {
    pub(crate) fn kind(&self) -> Option<FeatureKind> {
        (self.sections.len() >= 2).then(|| FeatureKind::Loft(Loft { sections: self.sections.clone(), ruled: self.ruled, operation: self.operation }))
    }
}

/// The lines and arcs of a sketch a sweep runs along: those that are not construction.
pub(crate) fn path_curves(doc: &Document, sketch: FeatureId) -> Option<SketchCurves> {
    let sk = doc.sketch(sketch)?;
    let curves: Vec<_> = sk
        .entities()
        .filter(|(_, e)| !e.construction && matches!(e.geometry, Geometry::Line { .. } | Geometry::Arc { .. }))
        .map(|(id, _)| id)
        .collect();
    (!curves.is_empty()).then_some(SketchCurves { sketch, curves })
}

fn has_profile(doc: &Document, sketch: FeatureId) -> bool {
    doc.sketch(sketch).is_some_and(|sk| !tenon_sketch::regions(sk).is_empty())
}

impl Workbench {
    /// The sketch a new sweep or coil takes its profile from: the one being edited when it has a
    /// closed profile (else it is likely the path), otherwise the last that has one.
    fn profile_sketch(&mut self) -> Result<FeatureId, String> {
        if let Mode::Sketch(s) = &self.mode {
            let id = s.feature;
            self.finish_sketch();
            if has_profile(self.document(), id) {
                return Ok(id);
            }
        }
        let doc = self.document();
        self.sketches().iter().rev().map(|s| s.0).find(|id| has_profile(doc, *id)).ok_or_else(|| "draw a closed profile in a sketch first".into())
    }

    pub(crate) fn open_sweep(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Sweep(s) => SweepPanel {
                    editing,
                    sketch: s.sketch,
                    regions: s.regions.clone(),
                    path: Some(s.path.clone()),
                    fixed: s.fixed,
                    operation: s.operation,
                },
                _ => return Err("not a sweep".into()),
            },
            None => {
                let sketch = self.profile_sketch()?;
                // The path: the last other sketch drawn with lines and arcs, preferring one with no
                // closed profile of its own.
                let doc = self.document();
                let others: Vec<FeatureId> = self.sketches().iter().rev().map(|s| s.0).filter(|id| *id != sketch).collect();
                let path = others
                    .iter()
                    .find(|id| !has_profile(doc, **id) && path_curves(doc, **id).is_some())
                    .or_else(|| others.iter().find(|id| path_curves(doc, **id).is_some()))
                    .and_then(|id| path_curves(doc, *id))
                    .ok_or("Sweep needs a path: draw it with lines and arcs in a sketch of its own first")?;
                // A profile in a plane parallel to the path's cannot be swept along it: prefer the
                // last one that can.
                let parallel = |a: FeatureId| match (self.sketch_frame(a), self.sketch_frame(path.sketch)) {
                    (Some(x), Some(y)) => x.z().dot(y.z()).abs() > 1.0 - 1e-9,
                    _ => false,
                };
                let sketch = match parallel(sketch) {
                    true => others.iter().copied().find(|id| *id != path.sketch && has_profile(doc, *id) && !parallel(*id)).unwrap_or(sketch),
                    false => sketch,
                };
                SweepPanel { editing: None, sketch, regions: RegionSel::Default, path: Some(path), fixed: false, operation: Operation::Join }
            }
        };
        self.panel = Some(Panel::Sweep(panel));
        self.start_equations();
        self.set_status("Sweep: choose the profile and the path, then OK.");
        Ok(())
    }

    pub(crate) fn open_coil(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Coil(c) => CoilPanel {
                    editing,
                    sketch: c.sketch,
                    regions: c.regions.clone(),
                    axis: match c.axis {
                        AxisRef::Origin(a) => AxisChoice::Origin(a),
                        AxisRef::SketchLine(l) => AxisChoice::Line(l),
                        AxisRef::Work(w) => AxisChoice::Work(w),
                    },
                    pitch: c.pitch,
                    turns: c.turns,
                    left: c.left,
                    operation: c.operation,
                },
                _ => return Err("not a coil".into()),
            },
            None => {
                let sketch = self.profile_sketch()?;
                // The axis: a construction line of the sketch, else the origin axis lying in its
                // plane (Z first).
                let doc = self.document();
                let line = doc.sketch(sketch).and_then(|s| s.entities().find(|(id, e)| e.construction && s.is_line(*id)).map(|(id, _)| id));
                let normal = self.sketch_frame(sketch).map(|f| f.z());
                let axis = match (line, normal) {
                    (Some(l), _) => AxisChoice::Line(l),
                    (None, Some(n)) => {
                        let lies = |a: OriginAxis| a.axis().dir().dot(n).abs() < 1e-6;
                        AxisChoice::Origin([OriginAxis::Z, OriginAxis::Y, OriginAxis::X].into_iter().find(|a| lies(*a)).unwrap_or(OriginAxis::Z))
                    }
                    (None, None) => AxisChoice::Origin(OriginAxis::Z),
                };
                CoilPanel {
                    editing: None,
                    sketch,
                    regions: RegionSel::Default,
                    axis,
                    pitch: 10.0,
                    turns: 5.0,
                    left: false,
                    operation: Operation::Join,
                }
            }
        };
        self.panel = Some(Panel::Coil(panel));
        self.start_equations();
        self.set_status("Coil: choose the axis, pitch and turns, then OK.");
        Ok(())
    }

    pub(crate) fn open_loft(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Loft(l) => LoftPanel { editing, sections: l.sections.clone(), ruled: l.ruled, operation: l.operation },
                _ => return Err("not a loft".into()),
            },
            None => {
                if matches!(self.mode, Mode::Sketch(_)) {
                    self.finish_sketch();
                }
                // Every sketch with a closed profile that nothing uses yet, in order.
                let doc = self.document();
                let used: std::collections::BTreeSet<FeatureId> = doc.features().iter().flat_map(|f| f.kind.depends_on()).collect();
                let sections: Vec<FeatureId> = self.sketches().iter().map(|s| s.0).filter(|id| has_profile(doc, *id) && !used.contains(id)).collect();
                if sections.len() < 2 {
                    return Err("Loft needs two or more sketches with closed profiles, on different planes".into());
                }
                LoftPanel { editing: None, sections, ruled: false, operation: Operation::Join }
            }
        };
        self.panel = Some(Panel::Loft(panel));
        self.start_equations();
        self.set_status("Loft: tick the sections, in order, then OK.");
        Ok(())
    }

    /// The fields of a sweep, coil or loft panel. Returns whether Enter was pressed in a value.
    pub(crate) fn sweeps_properties(
        &self,
        ui: &mut Ui,
        panel: &mut Panel,
        eqs: &mut Equations,
        sketches: &[(FeatureId, String)],
        work_names: &[(FeatureId, String, &str)],
        t: &Tokens,
    ) -> bool {
        let mut enter = false;
        let doc = self.document();
        let name = |id: FeatureId| sketches.iter().find(|s| s.0 == id).map_or_else(|| "?".to_string(), |s| s.1.clone());
        let output = |ui: &mut Ui, op: &mut Operation| {
            section(ui, "Output", true, |ui| {
                egui::Grid::new("tn_props_sweeps_output").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("Boolean");
                    let items: Vec<_> = OPS.iter().map(|o| (o.0, o.1, true)).collect();
                    let sel = OPS.iter().position(|o| o.2 == *op).unwrap_or(0);
                    if let Some(i) = glyph_row(ui, &items, sel, t) {
                        *op = OPS[i].2;
                    }
                    ui.end_row();
                });
            });
        };
        match panel {
            Panel::Sweep(p) => {
                section(ui, "Input Geometry", true, |ui| {
                    egui::Grid::new("tn_props_sweep_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Profiles");
                        ui.label(format!("{} selected", self.profile_count(p.sketch, &p.regions)));
                        ui.end_row();
                        ui.label("Sketch");
                        egui::ComboBox::from_id_salt("tn_props_sweep_sketch").selected_text(name(p.sketch)).show_ui(ui, |ui| {
                            for (id, n) in sketches.iter().filter(|s| has_profile(doc, s.0)) {
                                if ui.selectable_value(&mut p.sketch, *id, n).changed() {
                                    p.regions = RegionSel::Default;
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("Path");
                        let shown = p.path.as_ref().map_or_else(|| "Choose a sketch".to_string(), |c| name(c.sketch));
                        egui::ComboBox::from_id_salt("tn_props_sweep_path").selected_text(shown).show_ui(ui, |ui| {
                            for (id, n) in sketches.iter().filter(|s| s.0 != p.sketch && path_curves(doc, s.0).is_some()) {
                                let on = p.path.as_ref().is_some_and(|c| c.sketch == *id);
                                if ui.selectable_label(on, n).clicked() {
                                    p.path = path_curves(doc, *id);
                                }
                            }
                        });
                        ui.end_row();
                    });
                });
                section(ui, "Behavior", true, |ui| {
                    egui::Grid::new("tn_props_sweep_behavior").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Orientation");
                        egui::ComboBox::from_id_salt("tn_props_sweep_orient")
                            .selected_text(if p.fixed { "Fixed" } else { "Follow the path" })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut p.fixed, false, "Follow the path");
                                ui.selectable_value(&mut p.fixed, true, "Fixed");
                            });
                        ui.end_row();
                    });
                });
                output(ui, &mut p.operation);
            }
            Panel::Coil(p) => {
                let lines: Vec<tenon_sketch::EntityId> =
                    doc.sketch(p.sketch).map(|s| s.entities().filter(|(id, _)| s.is_line(*id)).map(|(id, _)| id).collect()).unwrap_or_default();
                section(ui, "Input Geometry", true, |ui| {
                    egui::Grid::new("tn_props_coil_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Profiles");
                        ui.label(format!("{} selected", self.profile_count(p.sketch, &p.regions)));
                        ui.end_row();
                        ui.label("Sketch");
                        egui::ComboBox::from_id_salt("tn_props_coil_sketch").selected_text(name(p.sketch)).show_ui(ui, |ui| {
                            for (id, n) in sketches.iter().filter(|s| has_profile(doc, s.0)) {
                                if ui.selectable_value(&mut p.sketch, *id, n).changed() {
                                    p.regions = RegionSel::Default;
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("Axis");
                        let axis_name = |a: &AxisChoice| match a {
                            AxisChoice::Origin(o) => format!("{o:?} Axis"),
                            AxisChoice::Line(l) => format!("Sketch line {}", l.0),
                            AxisChoice::Work(w) => work_names.iter().find(|x| x.0 == *w).map_or("Work Axis".into(), |x| x.1.clone()),
                        };
                        egui::ComboBox::from_id_salt("tn_props_coil_axis").selected_text(axis_name(&p.axis)).show_ui(ui, |ui| {
                            for o in [OriginAxis::X, OriginAxis::Y, OriginAxis::Z] {
                                ui.selectable_value(&mut p.axis, AxisChoice::Origin(o), format!("{o:?} Axis"));
                            }
                            for l in &lines {
                                ui.selectable_value(&mut p.axis, AxisChoice::Line(*l), format!("Sketch line {}", l.0));
                            }
                            for (w, n, _) in work_names.iter().filter(|x| x.2 == "axis") {
                                ui.selectable_value(&mut p.axis, AxisChoice::Work(*w), n);
                            }
                        });
                        ui.end_row();
                    });
                });
                section(ui, "Behavior", true, |ui| {
                    egui::Grid::new("tn_props_coil_behavior").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Pitch");
                        enter |= eq_value_field(ui, egui::Id::new("tn_props_pitch"), &mut p.pitch, eqs, "pitch", "mm", 0.001..=1e6, 110.0, t).entered;
                        ui.end_row();
                        ui.label("Turns");
                        enter |=
                            eq_value_field(ui, egui::Id::new("tn_props_turns"), &mut p.turns, eqs, "turns", "", 0.01..=10_000.0, 110.0, t).entered;
                        ui.end_row();
                        ui.label("Rotation");
                        egui::ComboBox::from_id_salt("tn_props_coil_hand")
                            .selected_text(if p.left { "Left-handed" } else { "Right-handed" })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut p.left, false, "Right-handed");
                                ui.selectable_value(&mut p.left, true, "Left-handed");
                            });
                        ui.end_row();
                    });
                });
                output(ui, &mut p.operation);
            }
            Panel::Loft(p) => {
                section(ui, "Sections", true, |ui| {
                    ui.label(egui::RichText::new("In the order they are ticked, bottom to top of the list.").small());
                    for (id, n) in sketches.iter().filter(|s| has_profile(doc, s.0) && p.editing.is_none_or(|e| doc.index_of(s.0) < doc.index_of(e)))
                    {
                        let mut on = p.sections.contains(id);
                        if ui.checkbox(&mut on, n).changed() {
                            if on {
                                p.sections.push(*id);
                            } else {
                                p.sections.retain(|s| s != id);
                            }
                        }
                    }
                    ui.label(format!("{} sections", p.sections.len()));
                });
                section(ui, "Behavior", true, |ui| {
                    ui.checkbox(&mut p.ruled, "Ruled (flat sides between sections)");
                });
                output(ui, &mut p.operation);
            }
            _ => {}
        }
        enter
    }
}

impl Workbench {
    /// The sketches drawn over the part outside sketch mode, and whether each is one an open
    /// Sweep, Coil or Loft panel takes its shape from (drawn brighter): those, and the sketches
    /// no feature uses yet. A sweep's path and a loft's sections can then be seen while they are
    /// chosen.
    pub(crate) fn shown_sketches(&self) -> Vec<(FeatureId, bool)> {
        let doc = self.document();
        // What is past End of Part or suppressed is not in the part: it neither shows nor uses
        // anything.
        let live: Vec<&tenon_model::Feature> = doc.features().iter().take(doc.end_of_part()).filter(|f| !f.suppressed).collect();
        let used: std::collections::BTreeSet<FeatureId> = live.iter().flat_map(|f| f.kind.depends_on()).collect();
        let chosen: Vec<FeatureId> = match &self.panel {
            Some(Panel::Sweep(p)) => std::iter::once(p.sketch).chain(p.path.as_ref().map(|c| c.sketch)).collect(),
            Some(Panel::Coil(p)) => vec![p.sketch],
            // (Its lines can be clicked to turn about.)
            Some(Panel::Revolve(p)) => vec![p.sketch],
            // (Its closed regions can be clicked to choose what is extruded.)
            Some(Panel::Extrude(p)) => vec![p.sketch],
            Some(Panel::Loft(p)) => p.sections.clone(),
            _ => vec![],
        };
        live.iter()
            .filter(|f| matches!(f.kind, FeatureKind::Sketch { .. }))
            .map(|f| (f.id, chosen.contains(&f.id)))
            .filter(|(id, on)| *on || !used.contains(id))
            .collect()
    }

    /// Draws [`Workbench::shown_sketches`].
    pub(crate) fn sketch_wires(&self, ui: &Ui, rect: egui::Rect, t: &Tokens) {
        let doc = self.document();
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let painter = ui.painter().with_clip_rect(rect);
        for (id, on) in self.shown_sketches() {
            let (Some(sk), Some(frame)) = (doc.sketch(id), self.sketch_frame(id)) else { continue };
            let stroke = if on { egui::Stroke::new(2.0, t.accent) } else { egui::Stroke::new(1.2, t.text_dim) };
            for (e, entity) in sk.entities() {
                if entity.geometry.is_point() {
                    continue;
                }
                let pts: Vec<egui::Pos2> = sk
                    .tessellate(e, 0.05)
                    .into_iter()
                    .filter_map(|q| self.view.camera.project(frame.plane_point(q), w, h))
                    .map(|(x, y, _)| rect.min + egui::vec2(x as f32, y as f32))
                    .collect();
                if pts.len() < 2 {
                    continue;
                }
                if entity.construction {
                    painter.extend(egui::Shape::dashed_line(&pts, egui::Stroke::new(1.0, stroke.color), 6.0, 4.0));
                } else {
                    painter.add(egui::Shape::line(pts, stroke));
                }
            }
        }
    }
}

/// The axis of a revolution or coil: choosing a sensible one, picking it in the viewport, and
/// showing it.
impl Workbench {
    /// The sketch and axis of the open Revolve or Coil panel.
    fn axis_panel(&self) -> Option<(FeatureId, AxisChoice)> {
        match &self.panel {
            Some(Panel::Revolve(p)) => Some((p.sketch, p.axis)),
            Some(Panel::Coil(p)) => Some((p.sketch, p.axis)),
            _ => None,
        }
    }

    /// Puts an axis into the open Revolve or Coil panel. Returns false when neither is open.
    pub(crate) fn set_panel_axis(&mut self, axis: AxisChoice) -> bool {
        match &mut self.panel {
            Some(Panel::Revolve(p)) => p.axis = axis,
            Some(Panel::Coil(p)) => p.axis = axis,
            _ => return false,
        }
        true
    }

    /// The axis a new revolution of `sketch` starts with:
    /// - a centre line of the sketch (a construction line), if it has one;
    /// - else the one line drawn in the sketch that bounds no profile (it was drawn to turn about);
    /// - else an origin axis lying in the sketch's plane, the upright one first, and one the
    ///   profile is beside before one that runs through it.
    pub(crate) fn default_revolve_axis(&self, sketch: FeatureId) -> AxisChoice {
        let doc = self.document();
        let Some(sk) = doc.sketch(sketch) else { return AxisChoice::Origin(OriginAxis::Y) };
        let lines: Vec<(tenon_sketch::EntityId, bool)> =
            sk.entities().filter(|(id, _)| sk.is_line(*id)).map(|(id, e)| (id, e.construction)).collect();
        if let Some((id, _)) = lines.iter().find(|l| l.1) {
            return AxisChoice::Line(*id);
        }
        let regions = tenon_sketch::regions(sk);
        let bounding: std::collections::BTreeSet<tenon_sketch::EntityId> =
            regions.iter().flat_map(|r| r.outer.iter().chain(r.holes.iter().flatten())).copied().collect();
        let loose: Vec<tenon_sketch::EntityId> = lines.iter().filter(|l| !bounding.contains(&l.0)).map(|l| l.0).collect();
        if let [only] = loose.as_slice() {
            return AxisChoice::Line(*only);
        }
        let Some(frame) = self.sketch_frame(sketch) else { return AxisChoice::Origin(OriginAxis::Y) };
        let in_plane: Vec<OriginAxis> =
            [OriginAxis::Z, OriginAxis::Y, OriginAxis::X].into_iter().filter(|a| a.axis().dir().dot(frame.z()).abs() < 1e-6).collect();
        // Across an axis: profile points both sides of it.
        let across = |a: &OriginAxis| {
            let axis = a.axis();
            let side = frame.z().cross(axis.dir());
            let s: Vec<f64> = regions.iter().flat_map(|r| r.outline.iter()).map(|q| (frame.plane_point(*q) - axis.origin()).dot(side)).collect();
            s.iter().any(|x| *x > 1e-6) && s.iter().any(|x| *x < -1e-6)
        };
        let axis = in_plane.iter().find(|a| !across(a)).or(in_plane.first()).copied().unwrap_or(OriginAxis::Y);
        AxisChoice::Origin(axis)
    }

    /// The line of the open Revolve or Coil panel's sketch under `pos`: a click makes it the axis.
    pub(crate) fn axis_line_at(&self, pos: Option<egui::Pos2>, rect: egui::Rect) -> Option<tenon_sketch::EntityId> {
        let pos = pos?;
        let (sketch, _) = self.axis_panel()?;
        let (sk, frame) = (self.document().sketch(sketch)?, self.sketch_frame(sketch)?);
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let screen =
            |q: tenon_geom::Vec2| self.view.camera.project(frame.plane_point(q), w, h).map(|(x, y, _)| rect.min + egui::vec2(x as f32, y as f32));
        sk.entities()
            .filter_map(|(id, _)| {
                let (a, b) = sk.line(id)?;
                let (a, b) = (screen(a)?, screen(b)?);
                // Distance from the pointer to the segment on screen.
                let ab = b - a;
                let t = if ab.length_sq() > 0.0 { ((pos - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
                Some((id, (a + ab * t).distance(pos)))
            })
            .filter(|(_, d)| *d <= 6.0)
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .map(|(id, _)| id)
    }

    /// The open Revolve or Coil panel's axis in space: a point on it and its direction.
    fn panel_axis_line(&self) -> Option<(tenon_geom::Vec3, tenon_geom::Vec3)> {
        let (sketch, axis) = self.axis_panel()?;
        match axis {
            AxisChoice::Origin(o) => Some((o.axis().origin(), o.axis().dir())),
            AxisChoice::Line(l) => {
                let (sk, frame) = (self.document().sketch(sketch)?, self.sketch_frame(sketch)?);
                let (a, b) = sk.line(l)?;
                let (a, b) = (frame.plane_point(a), frame.plane_point(b));
                Some((a, (b - a).normalized())).filter(|x| x.1.len() > 0.0)
            }
            AxisChoice::Work(w) => self.scene.work.iter().find(|x| x.0 == w).and_then(|x| match x.1 {
                tenon_model::WorkGeom::Axis(a) => Some((a.origin(), a.dir())),
                _ => None,
            }),
        }
    }

    /// While a Revolve or Coil panel is open: its axis as a centre line through the part, and
    /// the sketch line under the pointer lit up (a click makes it the axis).
    pub(crate) fn axis_overlay(&self, ui: &Ui, rect: egui::Rect, t: &Tokens) {
        let Some((sketch, _)) = self.axis_panel() else { return };
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let screen = |p: tenon_geom::Vec3| self.view.camera.project(p, w, h).map(|(x, y, _)| rect.min + egui::vec2(x as f32, y as f32));
        let painter = ui.painter().with_clip_rect(rect);
        if let Some((origin, dir)) = self.panel_axis_line() {
            // As long as the part and the sketch reach along it, and a little more.
            let mut along: Vec<f64> = Vec::new();
            if let Some(b) = self.scene.bbox() {
                for x in [b.min.x, b.max.x] {
                    for y in [b.min.y, b.max.y] {
                        for z in [b.min.z, b.max.z] {
                            along.push((tenon_geom::Vec3::new(x, y, z) - origin).dot(dir));
                        }
                    }
                }
            }
            if let (Some(sk), Some(frame)) = (self.document().sketch(sketch), self.sketch_frame(sketch)) {
                along.extend(sk.entities().filter_map(|(id, _)| sk.point(id)).map(|q| (frame.plane_point(q) - origin).dot(dir)));
            }
            let (lo, hi) = along.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
            let (lo, hi) = if lo.is_finite() { (lo, hi) } else { (-10.0, 10.0) };
            let margin = ((hi - lo) * 0.15).max(5.0);
            if let (Some(a), Some(b)) = (screen(origin + dir * (lo - margin)), screen(origin + dir * (hi + margin))) {
                // Long dash, short dash: a centre line.
                painter.extend(egui::Shape::dashed_line_with_offset(&[a, b], egui::Stroke::new(1.5, t.tint_work), &[16.0, 4.0], &[4.0, 4.0], 0.0));
            }
        }
        let hover = self.axis_line_at(ui.input(|i| i.pointer.hover_pos()).filter(|q| rect.contains(*q)), rect);
        if let Some(id) = hover
            && let (Some(sk), Some(frame)) = (self.document().sketch(sketch), self.sketch_frame(sketch))
            && let Some((a, b)) = sk.line(id)
            && let (Some(a), Some(b)) = (screen(frame.plane_point(a)), screen(frame.plane_point(b)))
        {
            painter.line_segment([a, b], egui::Stroke::new(3.0, t.tint_work));
        }
    }
}

/// Choosing which closed regions of a sketch a feature is made from, by clicking them in the
/// viewport while its panel is open.
impl Workbench {
    /// The sketch and regions of the open panel, when it makes a feature from a sketch profile.
    fn profile_panel(&self) -> Option<(FeatureId, &RegionSel)> {
        match &self.panel {
            Some(Panel::Extrude(p)) => Some((p.sketch, &p.regions)),
            Some(Panel::Revolve(p)) => Some((p.sketch, &p.regions)),
            Some(Panel::Sweep(p)) => Some((p.sketch, &p.regions)),
            Some(Panel::Coil(p)) => Some((p.sketch, &p.regions)),
            _ => None,
        }
    }

    /// The regions of sketch `sk` the open panel uses.
    fn chosen_regions(&self, sk: &tenon_sketch::Sketch) -> Vec<tenon_sketch::SketchRegion> {
        let all = tenon_sketch::regions(sk);
        match self.profile_panel().map(|p| p.1) {
            Some(RegionSel::Keys(keys)) => all.into_iter().filter(|r| keys.contains(&r.key)).collect(),
            _ => tenon_sketch::default_regions(&all).into_iter().cloned().collect(),
        }
    }

    /// A click at `pos` in the viewport while a profile panel is open: the closed region of the
    /// profile's sketch under it joins the feature, or leaves it if it was in. Returns false
    /// when the click is on no region.
    pub(crate) fn toggle_profile_at(&mut self, pos: egui::Pos2, rect: egui::Rect) -> bool {
        let Some((sketch, sel)) = self.profile_panel() else { return false };
        let (Some(sk), Some(frame)) = (self.document().sketch(sketch), self.sketch_frame(sketch)) else { return false };
        // Where the pointer's ray meets the sketch plane.
        let (o, d) =
            self.view.camera.ray(f64::from(pos.x - rect.left()), f64::from(pos.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
        let along = d.dot(frame.z());
        if along.abs() < 1e-9 {
            return false;
        }
        let local = frame.to_local(o + d * ((frame.origin() - o).dot(frame.z()) / along));
        let at = tenon_geom::Vec2::new(local.x, local.y);
        let all = tenon_sketch::regions(sk);
        // The innermost region there: a hole's disc before the plate round it.
        let Some(hit) = all.iter().filter(|r| r.contains(at)).max_by_key(|r| r.depth) else { return false };
        let mut keys: Vec<Vec<tenon_sketch::EntityId>> = match sel {
            RegionSel::Keys(k) => k.clone(),
            RegionSel::Default => tenon_sketch::default_regions(&all).iter().map(|r| r.key.clone()).collect(),
        };
        let mut note = None;
        match keys.iter().position(|k| *k == hit.key) {
            Some(_) if keys.len() == 1 => note = Some("A feature needs at least one profile: click another region to add it first."),
            Some(i) => {
                keys.remove(i);
            }
            None => keys.push(hit.key.clone()),
        }
        let regions = match &mut self.panel {
            Some(Panel::Extrude(p)) => &mut p.regions,
            Some(Panel::Revolve(p)) => &mut p.regions,
            Some(Panel::Sweep(p)) => &mut p.regions,
            Some(Panel::Coil(p)) => &mut p.regions,
            _ => return false,
        };
        *regions = RegionSel::Keys(keys);
        if let Some(n) = note {
            self.set_status(n);
        }
        true
    }

    /// The outlines of the regions the open profile panel uses, drawn boldly over the sketch so
    /// it is plain what the feature is made from.
    pub(crate) fn profile_overlay(&self, ui: &Ui, rect: egui::Rect, t: &Tokens) {
        let Some((sketch, _)) = self.profile_panel() else { return };
        let (Some(sk), Some(frame)) = (self.document().sketch(sketch), self.sketch_frame(sketch)) else { return };
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let painter = ui.painter().with_clip_rect(rect);
        for r in self.chosen_regions(sk) {
            for outline in std::iter::once(&r.outline).chain(&r.hole_outlines) {
                let mut pts: Vec<egui::Pos2> = outline
                    .iter()
                    .filter_map(|q| self.view.camera.project(frame.plane_point(*q), w, h))
                    .map(|(x, y, _)| rect.min + egui::vec2(x as f32, y as f32))
                    .collect();
                if let Some(first) = pts.first().copied() {
                    pts.push(first);
                }
                painter.add(egui::Shape::line(pts, egui::Stroke::new(3.0, t.accent)));
            }
        }
    }
}

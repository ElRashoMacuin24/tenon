//! Sheet metal in the app (M8): the Sheet Metal tab's commands, the Face and Flange panels,
//! Sheet Metal Defaults, and the flat pattern, shown in place of the folded part.
//!
//! The tab is a sheet metal part's home tab and shows only for one; any part becomes one with
//! Convert to Sheet Metal on the 3D Model tab, which is Sheet Metal Defaults by another door.

use std::path::{Path, PathBuf};

use egui::{Align2, Color32, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2};
use serde_json::json;
use tenon_geom::Vec2;
use tenon_io::flat::Marks;
use tenon_model::sheet::{self, BendPosition, CornerShape, FlangeWidth, FlatPattern, HeightDatum, ReliefShape};
use tenon_model::{EdgeRef, FeatureId, FeatureKind, Operation, RegionSel};

use crate::commands::{FLAT_TAB, MODEL_TAB, SHEET_TAB};
use crate::panels::{Direction, ExtentChoice, ExtrudePanel, Panel};
use crate::print_dialog::PartJob;
use crate::properties::{Equations, eq_value_field, flip_button, fmt_value, picked_row, section};
use crate::theme::{self, Tokens};
use crate::workbench::{Geo, Workbench};

/// A flat piece of sheet from a sketch profile.
#[derive(Clone, Debug)]
pub(crate) struct FacePanel {
    pub editing: Option<FeatureId>,
    pub sketch: FeatureId,
    pub regions: RegionSel,
    pub flip: bool,
    pub new_solid: bool,
}

impl FacePanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        let operation = if self.new_solid { Operation::NewBody } else { Operation::Join };
        FeatureKind::SheetFace(sheet::Face { sketch: self.sketch, regions: self.regions.clone(), flip: self.flip, operation })
    }
}

/// How much of its edge a flange takes, as the panel offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WidthChoice {
    Edge,
    Centered,
    Offset,
}

/// Walls bent up from the picked edges.
#[derive(Clone, Debug)]
pub(crate) struct FlangePanel {
    pub editing: Option<FeatureId>,
    pub edges: Vec<EdgeRef>,
    pub degrees: f64,
    pub height: f64,
    pub datum: HeightDatum,
    pub position: BendPosition,
    pub flip: bool,
    /// A bend radius of this flange's own, in place of the rule's.
    pub own_radius: bool,
    pub radius: f64,
    pub width: WidthChoice,
    pub width_value: f64,
    pub offset_a: f64,
    pub offset_b: f64,
}

impl FlangePanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        let width = match self.width {
            WidthChoice::Edge => FlangeWidth::Edge,
            WidthChoice::Centered => FlangeWidth::Centered { width: self.width_value },
            WidthChoice::Offset => FlangeWidth::Offset { start: self.offset_a, end: self.offset_b },
        };
        FeatureKind::Flange(sheet::Flange {
            edges: self.edges.clone(),
            angle: self.degrees.to_radians(),
            height: self.height,
            datum: self.datum,
            position: self.position,
            flip: self.flip,
            radius: self.own_radius.then_some(self.radius),
            width,
        })
    }

    /// The value fields that take equations as the panel stands.
    pub(crate) fn eq_pointers(&self) -> Vec<(&'static str, &'static str)> {
        let mut v = vec![("height", "/height"), ("degrees", "/angle")];
        if self.own_radius {
            v.push(("radius", "/radius"));
        }
        match self.width {
            WidthChoice::Edge => {}
            WidthChoice::Centered => v.push(("width", "/width/centered/width")),
            WidthChoice::Offset => v.extend([("offset_a", "/width/offset/start"), ("offset_b", "/width/offset/end")]),
        }
        v
    }
}

/// What is typed into Sheet Metal Defaults: each size as its equation, and the two shapes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DefaultsEdit {
    /// In the order of [`FIELDS`].
    pub sizes: [String; 7],
    pub relief: ReliefShape,
    pub corner: CornerShape,
}

/// The sizes of Sheet Metal Defaults: the command's key, the parameter, the label, a hint.
const FIELDS: [(&str, &str, &str, &str); 7] = [
    ("thickness", sheet::THICKNESS, "Thickness", "How thick the sheet is"),
    ("bend_radius", sheet::BEND_RADIUS, "Bend Radius", "The radius of the inside of every bend, unless a flange has its own"),
    ("relief_width", sheet::RELIEF_WIDTH, "Relief Width", "The width of the slot cut beside a bend that is narrower than its edge"),
    ("relief_depth", sheet::RELIEF_DEPTH, "Relief Depth", "How far that slot goes in past the bend"),
    ("min_remnant", sheet::MIN_REMNANT, "Minimum Remnant", "A strip left beside a relief that is narrower than this is cut away with it"),
    ("corner_size", sheet::CORNER_SIZE, "Relief Size", "The size of the relief where two bends meet at a corner"),
    (
        "k_factor",
        sheet::K_FACTOR,
        "K Factor",
        "Where the sheet neither stretches nor shrinks in a bend: 0 at its inside, 1 at its outside. It sets how wide a bend is in the flat pattern",
    ),
];

/// What the app keeps about sheet metal between frames.
#[derive(Clone, Debug)]
pub(crate) struct SheetUi {
    /// Sheet Metal Defaults, while it is open.
    pub defaults: Option<DefaultsEdit>,
    /// The flat pattern is shown in place of the folded part.
    pub flat_view: bool,
    /// The last pattern worked out: the document revision it is of, and it or why there is none.
    pub flat: Option<(u64, Result<FlatPattern, String>)>,
    /// The triangles that fill it, for the painter.
    fill: Vec<[Vec2; 3]>,
    /// A pattern being worked out: the revision it is of.
    pub pending: Option<u64>,
    /// A file to write once the pattern being worked out arrives.
    export: Option<(PathBuf, bool)>,
    pub show_bends: bool,
    pub show_extents: bool,
    /// The view of the pattern: the point of it at the middle of the window, and pixels to a
    /// millimetre. None until it has been fitted.
    cam: Option<(Vec2, f32)>,
    /// Define A-Side waits for a click on a flat face.
    pub pick_a_side: bool,
}

impl Default for SheetUi {
    fn default() -> Self {
        SheetUi {
            defaults: None,
            flat_view: false,
            flat: None,
            fill: Vec::new(),
            pending: None,
            export: None,
            show_bends: true,
            show_extents: true,
            cam: None,
            pick_a_side: false,
        }
    }
}

/// Orange and blue enough to tell from the sheet in either theme: bends up and bends down.
const BEND_UP: Color32 = Color32::from_rgb(0x1f, 0x9d, 0x4a);
const BEND_DOWN: Color32 = Color32::from_rgb(0xd0, 0x45, 0x35);

const NOT_SHEET: &str = "This is not a sheet metal part yet: use Convert to Sheet Metal on the 3D Model tab, or File > New Sheet Metal Part.";

impl Workbench {
    pub(crate) fn is_sheet_metal(&self) -> bool {
        !self.in_assembly() && !self.in_drawing() && self.document().sheet_metal().is_some()
    }

    /// The tab a part comes back to when a sketch is finished: Sheet Metal for a sheet metal
    /// part, 3D Model for any other.
    pub(crate) fn home_tab(&self) -> usize {
        if self.is_sheet_metal() { SHEET_TAB } else { MODEL_TAB }
    }

    /// The ribbon tabs of the part environment that show, in order: Sheet Metal first for a
    /// sheet metal part and not at all for another; while the flat pattern is shown, its own tab
    /// and the ones that have nothing to do with features.
    pub(crate) fn part_tabs(&self) -> Vec<usize> {
        let all = 0..crate::commands::RIBBON.len();
        let named = |n: &str| crate::commands::RIBBON.iter().position(|t| t.name == n);
        if self.flat_shown() {
            return std::iter::once(FLAT_TAB).chain(["Tools", "View"].iter().filter_map(|n| named(n))).collect();
        }
        let sheet = self.is_sheet_metal();
        let rest = all.filter(|i| *i != SHEET_TAB && *i != FLAT_TAB);
        if sheet { std::iter::once(SHEET_TAB).chain(rest).collect() } else { rest.collect() }
    }

    /// The flat pattern is what the window shows.
    pub(crate) fn flat_shown(&self) -> bool {
        self.chrome.sheet.flat_view && self.is_sheet_metal() && !self.is_sketching()
    }

    /// Keeps the open tab among those that show (a part that stopped being sheet metal by an
    /// undo, a flat pattern left).
    pub(crate) fn keep_tab_shown(&mut self) {
        if self.in_assembly() || self.in_drawing() {
            return;
        }
        if self.chrome.sheet.flat_view && !self.is_sheet_metal() {
            self.chrome.sheet.flat_view = false;
        }
        let tabs = self.part_tabs();
        if !tabs.contains(&self.chrome.tab) {
            self.chrome.tab = tabs.first().copied().unwrap_or(MODEL_TAB);
        }
    }

    /// The label of a ribbon button that changes with the part: Create Flat Pattern goes to the
    /// pattern once there is one.
    pub(crate) fn ribbon_label(&self, id: &str, label: &'static str) -> &'static str {
        match id {
            "sheet.flat_view" if self.document().sheet_metal().is_some_and(|r| r.flat) => "Go to\nFlat Pattern",
            _ => label,
        }
    }

    /// The commands of the Sheet Metal and Flat Pattern tabs. `None` when `id` is not one.
    pub(crate) fn sheet_command(&mut self, id: &str) -> Option<Result<(), String>> {
        if !(id.starts_with("sheet.") || id == "file.new_sheet_metal") {
            return None;
        }
        if id != "file.new_sheet_metal" && (self.in_assembly() || self.in_drawing()) {
            return Some(Err("that works on a part: open the part, or edit it in place".into()));
        }
        Some(self.run_sheet(id))
    }

    fn run_sheet(&mut self, id: &str) -> Result<(), String> {
        match id {
            "file.new_sheet_metal" => {
                self.leave_drawing();
                self.leave_assembly();
                let mut doc = tenon_model::Document::default();
                doc.set_sheet_metal(Default::default())?;
                doc.sync_parameters()?;
                self.session.replace_document(doc, None);
                self.path = None;
                self.mode = crate::workbench::Mode::Model;
                self.panel = None;
                self.view = Default::default();
                self.chrome.sheet = SheetUi::default();
                self.chrome.tab = SHEET_TAB;
                self.set_status("New sheet metal part, 1 mm thick: Sheet Metal Defaults changes that. Start with a sketch, then Face.");
            }
            "sheet.defaults" | "sheet.convert" => self.open_sheet_defaults(),
            "sheet.face" => self.open_sheet_face(None)?,
            "sheet.flange" => self.open_flange(None)?,
            "sheet.cut" => self.open_sheet_cut()?,
            "sheet.corner_round" => {
                self.open_modify("fillet", None)?;
                self.set_status("Corner Round: click the upright edges at the corners of the sheet, set the radius, then OK.");
            }
            "sheet.corner_chamfer" => {
                self.open_modify("chamfer", None)?;
                self.set_status("Corner Chamfer: click the upright edges at the corners of the sheet, set the distance, then OK.");
            }
            "sheet.flat_view" => {
                if self.document().sheet_metal().is_none() {
                    return Err(NOT_SHEET.into());
                }
                if self.is_sketching() {
                    self.finish_sketch();
                }
                if !self.document().sheet_metal().is_some_and(|r| r.flat) {
                    self.exec("sheet.flat_pattern", json!({}))?;
                }
                self.panel = None;
                self.chrome.sheet.flat_view = true;
                self.chrome.sheet.cam = None;
                self.chrome.tab = FLAT_TAB;
                self.set_status("Flat pattern. Go to Folded Part returns to the part.");
            }
            "sheet.folded" => {
                self.chrome.sheet.flat_view = false;
                self.chrome.tab = self.home_tab();
                self.set_status("Folded part.");
            }
            "sheet.delete_flat" => {
                self.exec("sheet.flat_pattern", json!({ "on": false }))?;
                self.chrome.sheet.flat_view = false;
                self.chrome.tab = self.home_tab();
                self.set_status("Flat pattern deleted. (Create Flat Pattern makes it again.)");
            }
            "sheet.a_side" => {
                if self.document().sheet_metal().is_none() {
                    return Err(NOT_SHEET.into());
                }
                match self.selected_face_ref() {
                    Some(face) => self.set_a_side(&face)?,
                    None => {
                        self.chrome.sheet.pick_a_side = true;
                        self.chrome.sheet.flat_view = false;
                        self.set_status("Define A-Side: click the flat face that is up when the pattern lies on the table. Esc cancels.");
                    }
                }
            }
            "sheet.show_bends" => self.chrome.sheet.show_bends = !self.chrome.sheet.show_bends,
            "sheet.show_extents" => self.chrome.sheet.show_extents = !self.chrome.sheet.show_extents,
            "sheet.export_dxf" | "sheet.export_svg" => {
                if self.document().sheet_metal().is_none() {
                    return Err(NOT_SHEET.into());
                }
                let ext = if id == "sheet.export_dxf" { "dxf" } else { "svg" };
                let name = format!("{} flat.{ext}", self.document().name);
                let path = self.services.pick_save.as_ref().and_then(|f| f(&name, ext)).ok_or("no file chosen")?;
                self.export_flat(&path, ext == "svg")?;
            }
            _ => {
                return Err(match crate::commands::find(id) {
                    Some(c) if !c.available() => c.not_yet(),
                    _ => format!("unknown command `{id}`"),
                });
            }
        }
        Ok(())
    }

    /// The face clicked for Define A-Side.
    pub(crate) fn set_a_side(&mut self, face: &tenon_model::FaceRef) -> Result<(), String> {
        self.chrome.sheet.pick_a_side = false;
        self.exec("sheet.a_side", json!({ "face": face }))?;
        self.set_status("The flat pattern is laid out from that face: it is the side that is up.");
        Ok(())
    }

    // ---- panels ---------------------------------------------------------------------------------

    pub(crate) fn open_sheet_face(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::SheetFace(f) => {
                    FacePanel { editing, sketch: f.sketch, regions: f.regions.clone(), flip: f.flip, new_solid: f.operation == Operation::NewBody }
                }
                _ => return Err("not a sheet metal face".into()),
            },
            None => {
                if self.document().sheet_metal().is_none() {
                    return Err(NOT_SHEET.into());
                }
                let sketch = self.profile_sketch()?;
                FacePanel { editing: None, sketch, regions: RegionSel::Default, flip: false, new_solid: false }
            }
        };
        self.panel = Some(Panel::SheetFace(panel));
        self.start_equations();
        self.set_status("Face: the profile is made one thickness thick. Click regions of the sketch to choose them, then OK.");
        Ok(())
    }

    pub(crate) fn open_flange(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Flange(f) => {
                    let (width, width_value, offset_a, offset_b) = match f.width {
                        FlangeWidth::Edge => (WidthChoice::Edge, 10.0, 0.0, 0.0),
                        FlangeWidth::Centered { width } => (WidthChoice::Centered, width, 0.0, 0.0),
                        FlangeWidth::Offset { start, end } => (WidthChoice::Offset, 10.0, start, end),
                    };
                    FlangePanel {
                        editing,
                        edges: f.edges.clone(),
                        degrees: f.angle.to_degrees(),
                        height: f.height,
                        datum: f.datum,
                        position: f.position,
                        flip: f.flip,
                        own_radius: f.radius.is_some(),
                        radius: f.radius.or_else(|| self.document().sheet_sizes().ok().map(|s| s.bend_radius)).unwrap_or(1.0),
                        width,
                        width_value,
                        offset_a,
                        offset_b,
                    }
                }
                _ => return Err("not a flange".into()),
            },
            None => {
                let sizes =
                    self.document().sheet_sizes().map_err(|e| if self.document().sheet_metal().is_none() { NOT_SHEET.to_owned() } else { e })?;
                if !self.document().features().iter().any(|f| matches!(f.kind, FeatureKind::SheetFace(_) | FeatureKind::Extrude(_))) {
                    return Err("there is no sheet yet: make a Face from a sketch first".into());
                }
                // High enough to stand clear of its own bend.
                let height = (4.0 * (sizes.bend_radius + sizes.thickness)).max(10.0);
                FlangePanel {
                    editing: None,
                    edges: self.picked_edges(),
                    degrees: 90.0,
                    height,
                    datum: HeightDatum::Outer,
                    position: BendPosition::Inside,
                    flip: false,
                    own_radius: false,
                    radius: sizes.bend_radius,
                    width: WidthChoice::Edge,
                    width_value: 10.0,
                    offset_a: 0.0,
                    offset_b: 0.0,
                }
            }
        };
        self.panel = Some(Panel::Flange(panel));
        self.start_equations();
        self.view.selection.clear();
        self.set_status("Flange: click edges of the sheet's outline (it bends towards the side the edge is on), set the height, then OK.");
        Ok(())
    }

    /// Cut: an extrusion that removes its profile through the sheet, one thickness each way
    /// from the sketch, so that it goes through whichever side the sketch is on and no further.
    fn open_sheet_cut(&mut self) -> Result<(), String> {
        let sizes = self.document().sheet_sizes().map_err(|e| if self.document().sheet_metal().is_none() { NOT_SHEET.to_owned() } else { e })?;
        let sketch = self.profile_sketch()?;
        self.panel = Some(Panel::Extrude(ExtrudePanel {
            editing: None,
            sketch,
            extent: ExtentChoice::Distance,
            direction: Direction::Asymmetric,
            distance: sizes.thickness,
            distance_b: sizes.thickness,
            operation: Operation::Cut,
            regions: RegionSel::Default,
            keep_taper: false,
            taper: 0.0,
        }));
        self.start_equations();
        self.panel_eqs.insert("distance", sheet::THICKNESS.to_owned());
        self.panel_eqs.insert("distance_b", sheet::THICKNESS.to_owned());
        self.set_status("Cut: the profile is cut through the sheet (one Thickness each way from the sketch). OK to cut.");
        Ok(())
    }

    /// The fields of the Face and Flange panels. Returns whether Enter was pressed in a value,
    /// and whether Clear was pressed on the picked edges.
    pub(crate) fn sheet_properties(
        &self,
        ui: &mut Ui,
        panel: &mut Panel,
        eqs: &mut Equations,
        sketches: &[(FeatureId, String)],
        t: &Tokens,
    ) -> (bool, bool) {
        let (mut enter, mut clear) = (false, false);
        let sizes = self.document().sheet_sizes().ok();
        // A list to choose one of. The list is remembered under `key` and each entry under
        // `key` and its own name; an entry's tip says what it does.
        fn choice<T: Copy + PartialEq>(ui: &mut Ui, key: &str, value: &mut T, options: &[(T, &str, &str, &str)]) {
            let shown = options.iter().find(|o| o.0 == *value).map_or("?", |o| o.2);
            let list = egui::ComboBox::from_id_salt(key).selected_text(shown).width(118.0).show_ui(ui, |ui| {
                for (v, name, label, tip) in options {
                    let r = ui.selectable_label(*value == *v, *label);
                    crate::drawing::remember(ui, &format!("{key}_{name}"), r.rect);
                    if r.on_hover_text(*tip).clicked() {
                        *value = *v;
                    }
                }
            });
            crate::drawing::remember(ui, key, list.response.rect);
        }
        match panel {
            Panel::SheetFace(p) => {
                section(ui, "Input Geometry", true, |ui| {
                    egui::Grid::new("tn_props_face_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Profiles");
                        ui.label(format!("{} selected", self.profile_count(p.sketch, &p.regions)));
                        ui.end_row();
                        ui.label("Sketch");
                        egui::ComboBox::from_id_salt("tn_props_face_sketch")
                            .selected_text(sketches.iter().find(|s| s.0 == p.sketch).map_or("?".into(), |s| s.1.clone()))
                            .show_ui(ui, |ui| {
                                for (id, name) in sketches {
                                    if ui.selectable_value(&mut p.sketch, *id, name).clicked() {
                                        p.regions = RegionSel::Default;
                                    }
                                }
                            });
                        ui.end_row();
                    });
                });
                section(ui, "Behavior", true, |ui| {
                    egui::Grid::new("tn_props_face_behavior").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Direction").on_hover_text("Which side of the sketch the sheet's thickness goes");
                        flip_button(ui, &mut p.flip, t);
                        ui.end_row();
                        ui.label("Thickness");
                        ui.label(RichText::new(sizes.map_or("?".into(), |s| format!("{} mm", fmt_value(s.thickness)))).color(t.text_dim))
                            .on_hover_text("The part's Thickness: change it in Sheet Metal Defaults");
                        ui.end_row();
                    });
                });
                if self.scene.bodies.len() + usize::from(p.editing.is_some()) > 1 || p.new_solid {
                    section(ui, "Output", true, |ui| {
                        ui.checkbox(&mut p.new_solid, "New solid").on_hover_text("A sheet of its own, not joined to the one it touches");
                    });
                }
            }
            Panel::Flange(p) => {
                section(ui, "Input Geometry", true, |ui| {
                    egui::Grid::new("tn_props_flange_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Edges");
                        clear |= picked_row(ui, p.edges.len(), "click edges");
                        ui.end_row();
                    });
                });
                section(ui, "Height Extents", true, |ui| {
                    egui::Grid::new("tn_props_flange_height").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Height");
                        enter |=
                            eq_value_field(ui, egui::Id::new("tn_props_flange_h"), &mut p.height, eqs, "height", "mm", 0.001..=100_000.0, 110.0, t)
                                .entered;
                        ui.end_row();
                        ui.label("Measured from").on_hover_text("What the height is measured from");
                        choice(
                            ui,
                            "tn_flange_datum",
                            &mut p.datum,
                            &[
                                (
                                    HeightDatum::Outer,
                                    "outer",
                                    "Outer Faces",
                                    "From where the outer faces of the sheet and the flange would meet: the overall height",
                                ),
                                (HeightDatum::Inner, "inner", "Inner Faces", "From where their inner faces would meet"),
                                (HeightDatum::Tangent, "tangent", "Tangent Plane", "From the plane that touches the outside of the bend"),
                            ],
                        );
                        ui.end_row();
                        ui.label("Direction").on_hover_text("Which side of the sheet the flange bends to");
                        flip_button(ui, &mut p.flip, t);
                        ui.end_row();
                    });
                });
                section(ui, "Angle and Placement", true, |ui| {
                    egui::Grid::new("tn_props_flange_angle").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Angle").on_hover_text("How far the sheet turns: 90 stands the flange upright");
                        enter |= eq_value_field(
                            ui,
                            egui::Id::new("tn_props_flange_angle_v"),
                            &mut p.degrees,
                            eqs,
                            "degrees",
                            "deg",
                            1.0..=179.0,
                            110.0,
                            t,
                        )
                        .entered;
                        ui.end_row();
                        ui.label("Bend Position").on_hover_text("Where the bend lies relative to the edge");
                        choice(
                            ui,
                            "tn_flange_position",
                            &mut p.position,
                            &[
                                (BendPosition::Inside, "inside", "Inside", "The flange's outer face goes through the edge: the part gets no bigger"),
                                (BendPosition::Adjacent, "adjacent", "From Edge", "The bend starts at the edge: nothing is cut from the sheet"),
                                (BendPosition::Outside, "outside", "Outside", "The flange's inner face goes through the edge"),
                                (BendPosition::Tangent, "tangent", "Tangent", "The outside of the bend touches the plane of the edge"),
                            ],
                        );
                        ui.end_row();
                    });
                });
                section(ui, "Width Extents", true, |ui| {
                    egui::Grid::new("tn_props_flange_width").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Type");
                        choice(
                            ui,
                            "tn_flange_width",
                            &mut p.width,
                            &[
                                (WidthChoice::Edge, "edge", "Edge", "The whole edge"),
                                (WidthChoice::Centered, "centered", "Width", "A width, in the middle of the edge"),
                                (WidthChoice::Offset, "offset", "Offset", "In from each end of the edge"),
                            ],
                        );
                        ui.end_row();
                        match p.width {
                            WidthChoice::Edge => {}
                            WidthChoice::Centered => {
                                ui.label("Width");
                                enter |= eq_value_field(
                                    ui,
                                    egui::Id::new("tn_props_flange_w"),
                                    &mut p.width_value,
                                    eqs,
                                    "width",
                                    "mm",
                                    0.001..=100_000.0,
                                    110.0,
                                    t,
                                )
                                .entered;
                                ui.end_row();
                            }
                            WidthChoice::Offset => {
                                ui.label("Offset 1").on_hover_text(
                                    "From the start of the edge (edges run clockwise round the sheet, seen from the side the flange bends to)",
                                );
                                enter |= eq_value_field(
                                    ui,
                                    egui::Id::new("tn_props_flange_o1"),
                                    &mut p.offset_a,
                                    eqs,
                                    "offset_a",
                                    "mm",
                                    0.0..=100_000.0,
                                    110.0,
                                    t,
                                )
                                .entered;
                                ui.end_row();
                                ui.label("Offset 2").on_hover_text("From the end of the edge");
                                enter |= eq_value_field(
                                    ui,
                                    egui::Id::new("tn_props_flange_o2"),
                                    &mut p.offset_b,
                                    eqs,
                                    "offset_b",
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
                section(ui, "Bend", true, |ui| {
                    egui::Grid::new("tn_props_flange_bend").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Bend Radius");
                        let mut by_rule = !p.own_radius;
                        let r = ui.checkbox(&mut by_rule, "By rule");
                        crate::drawing::remember(ui, "tn_flange_by_rule", r.rect);
                        r.on_hover_text("The part's BendRadius, from Sheet Metal Defaults");
                        if by_rule == p.own_radius {
                            p.own_radius = !by_rule;
                            if let (false, Some(s)) = (p.own_radius, sizes) {
                                p.radius = s.bend_radius;
                            }
                        }
                        ui.end_row();
                        ui.label("");
                        if p.own_radius {
                            enter |=
                                eq_value_field(ui, egui::Id::new("tn_props_flange_r"), &mut p.radius, eqs, "radius", "mm", 0.01..=1000.0, 110.0, t)
                                    .entered;
                        } else {
                            ui.label(RichText::new(sizes.map_or("?".into(), |s| format!("{} mm", fmt_value(s.bend_radius)))).color(t.text_dim));
                        }
                        ui.end_row();
                    });
                });
            }
            _ => {}
        }
        (enter, clear)
    }

    // ---- Sheet Metal Defaults ---------------------------------------------------------------

    /// Opens Sheet Metal Defaults with the part's rule as it stands (or the usual one, for a
    /// part that is not sheet metal yet: OK then makes it one).
    pub(crate) fn open_sheet_defaults(&mut self) {
        let doc = self.document();
        let rule = doc.sheet_metal().cloned().unwrap_or_default();
        let sizes = FIELDS.map(|(_, name, _, _)| {
            doc.parameters()
                .user
                .iter()
                .find(|u| u.name == name)
                .map(|u| u.equation.clone())
                .or_else(|| sheet::PARAMETERS.iter().find(|p| p.0 == name).map(|p| p.1.to_owned()))
                .unwrap_or_default()
        });
        self.chrome.sheet.defaults = Some(DefaultsEdit { sizes, relief: rule.relief, corner: rule.corner });
    }

    pub(crate) fn sheet_defaults_window(&mut self, ui: &mut Ui, t: &Tokens) {
        let Some(mut edit) = self.chrome.sheet.defaults.clone() else { return };
        if self.in_assembly() || self.in_drawing() {
            self.chrome.sheet.defaults = None;
            return;
        }
        let converting = self.document().sheet_metal().is_none();
        let values = self.param_env.1.clone();
        let mut open = true;
        let (mut ok, mut cancel) = (false, false);
        egui::Window::new("Sheet Metal Defaults").open(&mut open).collapsible(false).resizable(false).default_pos([320.0, 170.0]).show(
            ui.ctx(),
            |ui| {
                ui.set_width(330.0);
                if converting {
                    ui.add(
                        egui::Label::new(
                            RichText::new("OK makes this a sheet metal part. To lay it flat it must be one thickness all over.")
                                .font(theme::small())
                                .color(t.text_dim),
                        )
                        .wrap(),
                    );
                    ui.add_space(4.0);
                }
                // Rows laid out by hand, so that every group has its fields in one column (a grid
                // sizes its columns to what is in it, group by group).
                const LABEL_W: f32 = 124.0;
                fn label(ui: &mut Ui, text: &str, tip: &str) {
                    ui.allocate_ui_with_layout(vec2(LABEL_W, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.set_min_width(LABEL_W);
                        ui.label(text).on_hover_text(tip);
                    });
                }
                // A value, or an equation of the part's parameters; what it comes to beside it.
                let mut field = |ui: &mut Ui, i: usize| {
                    let (key, name, text, tip) = FIELDS[i];
                    ui.horizontal(|ui| {
                        label(ui, text, tip);
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut edit.sizes[i])
                                .id(egui::Id::new(format!("tn_sheet_{key}")))
                                .desired_width(120.0)
                                .font(theme::body()),
                        );
                        crate::drawing::remember(ui, &format!("tn_sheet_{key}"), r.rect);
                        // What it comes to, when it is an equation of other parameters.
                        let plain = edit.sizes[i].trim().trim_end_matches("mm").trim().parse::<f64>().is_ok();
                        if let Some(v) = values.get(name).filter(|_| !plain) {
                            ui.label(RichText::new(format!("= {}", fmt_value(*v))).font(theme::small()).color(t.text_dim));
                        }
                    });
                };
                let heading = |ui: &mut Ui, text: &str| {
                    ui.add_space(6.0);
                    ui.label(RichText::new(text).font(theme::body()).strong());
                };
                heading(ui, "Sheet");
                field(ui, 0);
                heading(ui, "Bend");
                field(ui, 1);
                ui.horizontal(|ui| {
                    label(ui, "Relief Shape", "The cut beside a bend that is narrower than its edge");
                    for (v, key, text) in [(ReliefShape::Straight, "straight", "Straight"), (ReliefShape::Round, "round", "Round")] {
                        let r = ui.selectable_label(edit.relief == v, text);
                        crate::drawing::remember(ui, &format!("tn_sheet_relief_{key}"), r.rect);
                        if r.clicked() {
                            edit.relief = v;
                        }
                    }
                });
                field(ui, 2);
                field(ui, 3);
                field(ui, 4);
                heading(ui, "Corner");
                ui.horizontal(|ui| {
                    label(ui, "Relief Shape", "The cut where two bends meet at a corner of the sheet");
                    for (v, key, text) in [
                        (CornerShape::Round, "round", "Round"),
                        (CornerShape::Square, "square", "Square"),
                        (CornerShape::Trim, "trim", "Trim to Bend"),
                    ] {
                        let r = ui.selectable_label(edit.corner == v, text);
                        crate::drawing::remember(ui, &format!("tn_sheet_corner_{key}"), r.rect);
                        if r.clicked() {
                            edit.corner = v;
                        }
                    }
                });
                field(ui, 5);
                heading(ui, "Unfold");
                field(ui, 6);
                ui.add_space(4.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ok = crate::drawing::button(ui, "OK", "tn_sheet_ok").clicked();
                    cancel = crate::drawing::button(ui, "Cancel", "tn_sheet_cancel").clicked();
                });
            },
        );
        if cancel || !open {
            self.chrome.sheet.defaults = None;
            return;
        }
        self.chrome.sheet.defaults = Some(edit.clone());
        if ok {
            let mut p = json!({ "relief": edit.relief, "corner": edit.corner });
            for (i, (key, ..)) in FIELDS.iter().enumerate() {
                // A plain number is a size in millimetres (the K factor is a plain number); anything
                // else is an equation.
                let typed = edit.sizes[i].trim();
                p[*key] = typed.parse::<f64>().map_or_else(|_| json!(typed), |n| json!(n));
            }
            match self.exec("sheet.defaults", p) {
                Ok(_) => {
                    self.chrome.sheet.defaults = None;
                    if converting {
                        self.chrome.tab = SHEET_TAB;
                        self.set_status(
                            "This is now a sheet metal part: the Sheet Metal tab has its commands, and Create Flat Pattern lays it flat.",
                        );
                    } else {
                        self.set_status("Sheet metal defaults changed: every face, bend and relief follows.");
                    }
                }
                Err(e) => self.set_error(e),
            }
        }
    }

    // ---- the flat pattern ---------------------------------------------------------------------

    /// Works the flat pattern of the part out, wherever the kernel is; it comes back to
    /// [`Workbench::flat_done`].
    fn request_flat(&mut self) {
        let revision = self.session.revision();
        let sheet = &self.chrome.sheet;
        if sheet.pending == Some(revision) || sheet.flat.as_ref().is_some_and(|(r, _)| *r == revision) {
            return;
        }
        let doc = self.session.document().clone();
        let done: Result<FlatPattern, String> = match &mut self.geo {
            Geo::Off => Err("there is no geometry kernel".into()),
            Geo::Sync(k) => {
                let regen = self.session.regen(k.as_mut()).clone();
                sheet::pattern_of(&doc, &regen, k.as_ref(), None, None)
            }
            Geo::Worker(w) => {
                self.step_seq += 1;
                self.part_jobs.insert(self.step_seq, PartJob::Flat { revision });
                self.chrome.sheet.pending = Some(revision);
                w.job(
                    self.step_seq,
                    Box::new(move |k, _| {
                        // The part as it is saved, not as the view may be showing it.
                        let mut regen = tenon_model::regenerate(&doc, k);
                        let done = match regen.first_error() {
                            Some((_, msg)) => Err(format!("The part does not regenerate, so it cannot be laid flat: {msg}")),
                            None => sheet::pattern_of(&doc, &regen, &*k, None, None),
                        };
                        regen.release(k);
                        Box::new(done)
                    }),
                );
                return;
            }
        };
        self.flat_done(revision, Box::new(done));
    }

    /// A flat pattern arrived.
    pub(crate) fn flat_done(&mut self, revision: u64, result: Box<dyn std::any::Any + Send>) {
        let flat = result.downcast::<Result<FlatPattern, String>>().map_or_else(|_| Err("internal: unexpected result".into()), |b| *b);
        let sheet = &mut self.chrome.sheet;
        if sheet.pending == Some(revision) {
            sheet.pending = None;
        }
        sheet.fill = match &flat {
            Ok(f) => {
                let holes: Vec<Vec<Vec2>> = f.inner.iter().map(|l| l.polygon(0.05)).collect();
                crate::polyfill::triangulate(&f.outer.polygon(0.05), &holes)
            }
            Err(_) => Vec::new(),
        };
        sheet.flat = Some((revision, flat));
        if let Some((path, svg)) = self.chrome.sheet.export.take() {
            self.write_flat(&path, svg);
        }
    }

    /// The flat pattern of the part as it is now, if it has been worked out.
    pub(crate) fn flat_now(&self) -> Option<&Result<FlatPattern, String>> {
        self.chrome.sheet.flat.as_ref().filter(|(r, _)| *r == self.session.revision()).map(|(_, f)| f)
    }

    /// Writes the flat pattern as a DXF (or SVG) file: now when it is worked out already, else
    /// when it arrives.
    pub(crate) fn export_flat(&mut self, path: &Path, svg: bool) -> Result<(), String> {
        if self.flat_now().is_some() {
            self.write_flat(path, svg);
            return Ok(());
        }
        self.chrome.sheet.export = Some((path.to_path_buf(), svg));
        self.set_status("Laying the part flat...");
        self.request_flat();
        Ok(())
    }

    fn write_flat(&mut self, path: &Path, svg: bool) {
        let marks = Marks { bends: self.chrome.sheet.show_bends, extents: self.chrome.sheet.show_extents };
        let written = match self.flat_now() {
            Some(Ok(f)) => {
                let data = if svg { tenon_io::flat::svg(f, marks) } else { tenon_io::flat::dxf(f, marks).0 };
                std::fs::write(path, data)
                    .map(|()| {
                        format!(
                            "Exported {} ({} x {} mm, {} to cut)",
                            path.display(),
                            fmt_value(f.length),
                            fmt_value(f.width),
                            mm_length(f.cut_length)
                        )
                    })
                    .map_err(|e| format!("cannot write {}: {e}", path.display()))
            }
            Some(Err(e)) => Err(e.clone()),
            None => Err("the flat pattern is not worked out yet".into()),
        };
        match written {
            Ok(s) => self.set_status(s),
            Err(e) => self.set_error(format!("Flat pattern export failed: {e}")),
        }
    }

    /// The flat pattern in the window, in place of the folded part: the sheet filled, its
    /// outline, its bend lines, and what size of sheet it is cut from.
    pub(crate) fn flat_canvas(&mut self, ui: &mut Ui, t: &Tokens) {
        let rect = ui.max_rect();
        self.view.rect = rect;
        let mut mesh = egui::Mesh::default();
        for (p, c) in [
            (rect.left_top(), t.viewport_top),
            (rect.right_top(), t.viewport_top),
            (rect.right_bottom(), t.viewport_bottom),
            (rect.left_bottom(), t.viewport_bottom),
        ] {
            mesh.colored_vertex(p, c);
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        ui.painter().add(Shape::mesh(mesh));
        // Worked out again whenever the part changes (not while it is still being rebuilt).
        if !self.waiting {
            self.request_flat();
        }
        let resp = ui.interact(rect, ui.id().with("flat"), Sense::CLICK | Sense::DRAG);
        let painter = ui.painter().with_clip_rect(rect);
        let flat = match self.flat_now() {
            Some(Ok(f)) => f.clone(),
            Some(Err(e)) => {
                let text = format!("No flat pattern\n\n{e}");
                let galley = painter.layout(text, theme::body(), t.history_marker, (rect.width() - 80.0).clamp(120.0, 640.0));
                painter.galley(rect.center() - galley.size() / 2.0, galley, t.history_marker);
                return;
            }
            None => {
                painter.text(rect.center(), Align2::CENTER_CENTER, "Laying the part flat...", theme::body(), t.text_dim);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                return;
            }
        };
        // The view: fitted at first, then dragged and wheeled about.
        let fit = |r: Rect| -> (Vec2, f32) {
            let scale = ((r.width() - 120.0) / flat.length as f32).min((r.height() - 150.0) / flat.width as f32).max(0.01);
            (Vec2::new(flat.length / 2.0, flat.width / 2.0), scale)
        };
        let (mut centre, mut scale) = self.chrome.sheet.cam.unwrap_or_else(|| fit(rect));
        if resp.dragged() {
            let d = resp.drag_delta();
            centre -= Vec2::new(f64::from(d.x / scale), f64::from(-d.y / scale));
        }
        if let Some(p) = resp.hover_pos() {
            let wheel = ui.input(|i| i.smooth_scroll_delta.y);
            if wheel != 0.0 {
                // About the pointer: what is under it stays under it.
                let factor = (wheel * 0.004).exp();
                let under = centre + Vec2::new(f64::from((p.x - rect.center().x) / scale), f64::from(-(p.y - rect.center().y) / scale));
                scale = (scale * factor).clamp(0.01, 2000.0);
                centre = under - Vec2::new(f64::from((p.x - rect.center().x) / scale), f64::from(-(p.y - rect.center().y) / scale));
            }
        }
        if resp.double_clicked() {
            (centre, scale) = fit(rect);
        }
        self.chrome.sheet.cam = Some((centre, scale));
        let to_screen =
            |p: Vec2| -> Pos2 { pos2(rect.center().x + (p.x - centre.x) as f32 * scale, rect.center().y - (p.y - centre.y) as f32 * scale) };

        // The sheet, in the part's colour.
        let body = self.body_palette(0).0;
        let mut fill = egui::Mesh::default();
        for tri in &self.chrome.sheet.fill {
            let at = fill.vertices.len() as u32;
            for p in tri {
                fill.colored_vertex(to_screen(*p), body);
            }
            fill.add_triangle(at, at + 1, at + 2);
        }
        painter.add(Shape::mesh(fill));
        let edge = Stroke::new(1.5, t.text);
        for l in flat.loops() {
            let pts: Vec<Pos2> = l.polygon(0.03).into_iter().map(to_screen).collect();
            painter.add(Shape::closed_line(pts, edge));
        }
        // Bend lines: where each bend begins and ends, thin; its centre line, dash-dotted, green
        // for a bend up (towards whoever looks) and red for one down.
        for b in &flat.bends {
            if self.chrome.sheet.show_extents {
                for seg in &b.extents {
                    painter.line_segment([to_screen(seg[0]), to_screen(seg[1])], Stroke::new(1.0, t.text_dim.gamma_multiply(0.7)));
                }
            }
            if self.chrome.sheet.show_bends {
                let color = if b.up { BEND_UP } else { BEND_DOWN };
                for seg in &b.lines {
                    painter.extend(Shape::dashed_line(&[to_screen(seg[0]), to_screen(seg[1])], Stroke::new(1.5, color), 14.0, 5.0));
                    let mid = to_screen((seg[0] + seg[1]) * 0.5);
                    let label = format!("{} {}°  R{}", if b.up { "Up" } else { "Down" }, fmt_value(b.angle.to_degrees()), fmt_value(b.radius));
                    let galley = painter.layout_no_wrap(label, theme::small(), color);
                    let at = mid + vec2(6.0, -galley.size().y - 2.0);
                    painter.rect_filled(Rect::from_min_size(at - vec2(2.0, 1.0), galley.size() + vec2(4.0, 2.0)), 2.0, t.panel.gamma_multiply(0.85));
                    painter.galley(at, galley, color);
                }
            }
        }
        // What it is cut from.
        let lines = [
            format!("Flat pattern  {} x {} mm", fmt_value(flat.length), fmt_value(flat.width)),
            format!("Sheet {} mm, K factor {}", fmt_value(flat.thickness), fmt_value(flat.k_factor)),
            format!(
                "{} bends, {} cut-outs, {} to cut, {} cm²",
                flat.bends.len(),
                flat.inner.len(),
                mm_length(flat.cut_length),
                fmt_value(flat.area / 100.0)
            ),
        ];
        let mut y = rect.top() + 12.0;
        for (i, line) in lines.iter().enumerate() {
            let font = if i == 0 { theme::heading() } else { theme::small() };
            let galley = painter.layout_no_wrap(line.clone(), font, if i == 0 { t.text } else { t.text_dim });
            painter.galley(pos2(rect.left() + 14.0, y), galley.clone(), t.text);
            y += galley.size().y + 3.0;
        }
        // The size of the sheet, along the bottom and up the left of the pattern.
        let (lo, hi) = (to_screen(Vec2::ZERO), to_screen(Vec2::new(flat.length, flat.width)));
        let dim = Stroke::new(1.0, t.text_dim);
        let (below, beside) = (lo.y + 22.0, lo.x - 22.0);
        painter.line_segment([pos2(lo.x, below), pos2(hi.x, below)], dim);
        painter.line_segment([pos2(beside, lo.y), pos2(beside, hi.y)], dim);
        for x in [lo.x, hi.x] {
            painter.line_segment([pos2(x, lo.y + 6.0), pos2(x, below + 4.0)], dim);
        }
        for yy in [lo.y, hi.y] {
            painter.line_segment([pos2(lo.x - 6.0, yy), pos2(beside - 4.0, yy)], dim);
        }
        painter.text(pos2((lo.x + hi.x) / 2.0, below + 3.0), Align2::CENTER_TOP, fmt_value(flat.length), theme::small(), t.text);
        painter.text(pos2(beside - 4.0, (lo.y + hi.y) / 2.0), Align2::RIGHT_CENTER, fmt_value(flat.width), theme::small(), t.text);
    }

    /// Fits the flat pattern in the window again.
    pub(crate) fn fit_flat(&mut self) {
        self.chrome.sheet.cam = None;
    }
}

/// A length to cut, in the unit that reads best.
fn mm_length(mm: f64) -> String {
    if mm >= 1000.0 { format!("{} m", fmt_value(mm / 1000.0)) } else { format!("{} mm", fmt_value(mm)) }
}

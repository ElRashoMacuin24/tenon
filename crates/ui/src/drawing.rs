//! The drawing environment: an open drawing (`.tenondrw`) on a sheet that pans and zooms;
//! placing views (base, projected, section, detail), annotating (dimensions picked from the
//! views' edges, text, parts lists, balloons, hole tables), moving and deleting what is placed,
//! editing a view's model from the drawing (Return comes back), and PDF, SVG and DXF export.
//!
//! Every change goes through the `drw.*` commands (`tenon_io::drw::run`), as scripts do. The
//! views are computed on the worker, one job at a time; the sheet keeps showing the last ones
//! meanwhile.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};
use tenon_drawing::graphics::dashed;
use tenon_drawing::views::{Evaluation, base_frame, projected_frame, seen_size};
use tenon_drawing::{AnnotKind, DrwModel, DrwSession, Graphics, Orientation, Owner, SheetId, Side, Standard, ViewId, ViewKind};
use tenon_geom::Vec2;

use crate::dim_sets::SetPanel;
use crate::drw_views::{ModPanel, ModPick, ModWhat, OverlayPanel, draw_panels, source_view};
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};
use crate::viewport::{HOVER, Nav, SELECTED};
use crate::workbench::{Geo, Mode, Workbench};

/// How near the pointer something must be drawn to be under it (pixels).
pub(crate) const PICK_PX: f64 = 6.0;
/// How often the model files are checked for changes made outside the drawing (seconds).
const MODEL_CHECK_SECONDS: f64 = 1.0;
/// Ink on the paper.
const INK: Color32 = Color32::from_rgb(0x1e, 0x22, 0x28);
/// Previews of what a tool is about to place.
pub(crate) const GHOST: Color32 = Color32::from_rgb(0x2f, 0x7f, 0xd8);

/// The sheet canvas's camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SheetCam {
    /// The sheet point at the middle of the canvas (mm).
    pub center: Vec2,
    /// Pixels per millimetre of paper.
    pub px: f64,
}

impl Default for SheetCam {
    fn default() -> Self {
        SheetCam { center: Vec2::new(215.9, 139.7), px: 2.0 }
    }
}

impl SheetCam {
    pub(crate) fn screen(&self, rect: Rect, p: Vec2) -> Pos2 {
        let c = rect.center();
        pos2(c.x + ((p.x - self.center.x) * self.px) as f32, c.y - ((p.y - self.center.y) * self.px) as f32)
    }
    pub(crate) fn sheet(&self, rect: Rect, s: Pos2) -> Vec2 {
        let c = rect.center();
        Vec2::new(self.center.x + f64::from(s.x - c.x) / self.px, self.center.y - f64::from(s.y - c.y) / self.px)
    }
    /// Zooms by `factor`, the sheet point under `at` staying there.
    fn zoom(&mut self, rect: Rect, factor: f64, at: Pos2) {
        let before = self.sheet(rect, at);
        self.px = (self.px * factor).clamp(0.2, 200.0);
        let after = self.sheet(rect, at);
        self.center += before - after;
    }
    fn pan(&mut self, d: egui::Vec2) {
        self.center += Vec2::new(-f64::from(d.x) / self.px, f64::from(d.y) / self.px);
    }
    /// The whole sheet in `rect`, with a margin.
    fn fit(&mut self, rect: Rect, w: f64, h: f64) {
        self.center = Vec2::new(w / 2.0, h / 2.0);
        self.px = (f64::from(rect.width() - 48.0) / w).min(f64::from(rect.height() - 48.0) / h).max(0.2);
    }
}

/// A placing tool and what it has been given so far (sheet points in mm).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DrwTool {
    /// Each click places a view projected from `parent` (the first click picks it).
    Projected {
        parent: Option<ViewId>,
    },
    /// The view to cut, the two ends of the section line, then where the section goes.
    Section {
        parent: Option<ViewId>,
        line: Vec<Vec2>,
    },
    /// The view, the detail's centre, a point on its circle, then where the detail goes.
    Detail {
        parent: Option<ViewId>,
        center: Option<Vec2>,
        radius: Option<f64>,
    },
    /// One or two edges (picks from `drw.pick`), then where the dimension goes.
    Dimension {
        view: Option<ViewId>,
        picks: Vec<Value>,
    },
    /// A view, for its suggested dimensions.
    AutoDimension,
    /// An edge of a part in an assembly view, then where the balloon goes.
    Balloon {
        view: Option<ViewId>,
        attach: Option<Vec2>,
    },
    AutoBalloon,
    HoleTable {
        view: Option<ViewId>,
    },
    PartsList {
        view: Option<ViewId>,
    },
    Text,
    /// Each click on a circle or an arc puts a centre mark on it.
    CenterMark,
    /// Two places (a circle's centre, a line's middle or end), then the centreline through them;
    /// with `bisector`, two lines, then the centreline midway between them.
    Centerline {
        bisector: bool,
        view: Option<ViewId>,
        a: Option<Value>,
    },
    /// The view, a straight edge of it, then where the view square to that edge goes. With
    /// `changing`: the auxiliary view that takes the edge, instead of a new one.
    Auxiliary {
        parent: Option<ViewId>,
        edge: Option<Value>,
        changing: Option<ViewId>,
    },
    /// The view, then what the open Crop, Break or Break Out panel wants clicked (`first`: its first
    /// point, view coordinates).
    Modify {
        what: ModWhat,
        pick: ModPick,
        view: Option<ViewId>,
        first: Option<Vec2>,
    },
    /// A view of an assembly, for the Overlay panel.
    Overlay,
    /// A baseline or ordinate set and its panel: a datum or origin, the places to measure, then
    /// where it goes; or a set already made, being changed.
    Set(Box<SetPanel>),
}

impl DrwTool {
    pub(crate) fn id(&self) -> &'static str {
        match self {
            DrwTool::Projected { .. } => "drw.projected",
            DrwTool::Section { .. } => "drw.section",
            DrwTool::Detail { .. } => "drw.detail",
            DrwTool::Dimension { .. } => "drw.dimension",
            DrwTool::AutoDimension => "drw.dimension.auto",
            DrwTool::Balloon { .. } => "drw.balloon",
            DrwTool::AutoBalloon => "drw.balloon.auto",
            DrwTool::HoleTable { .. } => "drw.hole_table",
            DrwTool::PartsList { .. } => "drw.parts_list",
            DrwTool::Text => "drw.text",
            DrwTool::CenterMark => "drw.center_mark",
            DrwTool::Centerline { bisector: false, .. } => "drw.centerline",
            DrwTool::Centerline { bisector: true, .. } => "drw.centerline.bisector",
            DrwTool::Auxiliary { .. } => "drw.auxiliary",
            DrwTool::Modify { what, .. } => what.id(),
            DrwTool::Overlay => "drw.overlay",
            DrwTool::Set(p) if p.ordinate => "drw.ordinate",
            DrwTool::Set(_) => "drw.baseline",
        }
    }

    /// What to do next.
    pub(crate) fn prompt(&self) -> &'static str {
        match self {
            DrwTool::Projected { parent: None } => "Projected view: click the view to project from.",
            DrwTool::Projected { .. } => {
                "Click where each projected view goes: beside, above or below, or at a corner for an isometric view. Esc or right-click ends."
            }
            DrwTool::Section { parent: None, .. } => "Section view: click the view to cut.",
            DrwTool::Section { line, .. } if line.is_empty() => "Click the first end of the section line.",
            DrwTool::Section { line, .. } if line.len() == 1 => "Click the other end of the section line.",
            DrwTool::Section { .. } => "Click where the section view goes; it is seen from the other side of the line.",
            DrwTool::Detail { parent: None, .. } => "Detail view: click the view to enlarge.",
            DrwTool::Detail { center: None, .. } => "Click the centre of the detail.",
            DrwTool::Detail { radius: None, .. } => "Click to set the size of the detail's circle.",
            DrwTool::Detail { .. } => "Click where the detail view goes.",
            DrwTool::Dimension { picks, .. } if picks.is_empty() => "Dimension: click an edge, or two (Esc ends).",
            DrwTool::Dimension { picks, .. } if picks.len() == 1 => "Click a second edge, or where the dimension goes.",
            DrwTool::Dimension { .. } => "Click where the dimension goes.",
            DrwTool::AutoDimension => "Auto dimension: click a view.",
            DrwTool::Balloon { attach: None, .. } => "Balloon: click an edge of a part in an assembly view.",
            DrwTool::Balloon { .. } => "Click where the balloon goes.",
            DrwTool::AutoBalloon => "Auto balloon: click an assembly view.",
            DrwTool::HoleTable { view: None } => "Hole table: click a part view whose holes are seen end-on.",
            DrwTool::PartsList { view: None } => "Parts list: click an assembly view.",
            DrwTool::HoleTable { .. } | DrwTool::PartsList { .. } => "Click where the table's top-left corner goes.",
            DrwTool::Text => "Text: click where the text goes.",
            DrwTool::CenterMark => "Centre mark: click a circle or an arc (Esc ends).",
            DrwTool::Centerline { bisector: false, a: None, .. } => {
                "Centreline: click a circle (its centre), a line (its middle) or near a line's end."
            }
            DrwTool::Centerline { bisector: false, .. } => "Click the second circle, line or line end.",
            DrwTool::Centerline { a: None, .. } => "Centreline bisector: click a line.",
            DrwTool::Centerline { .. } => "Click the second line: parallel, or meeting the first.",
            DrwTool::Auxiliary { parent: None, .. } => "Auxiliary view: click the view to project from.",
            DrwTool::Auxiliary { edge: None, .. } => "Click a straight edge of the view: the new view looks square to it.",
            DrwTool::Auxiliary { .. } => "Click where the auxiliary view goes: it slides square to the edge, on the side it is seen from.",
            DrwTool::Modify { what: ModWhat::Crop, view: None, .. } => "Crop: click the view to crop.",
            DrwTool::Modify { what: ModWhat::Break, view: None, .. } => "Break: click the view to shorten.",
            DrwTool::Modify { what: ModWhat::BreakOut, view: None, .. } => "Break out: click the view to cut into.",
            DrwTool::Modify { pick: ModPick::Rect, first: None, .. } => "Click one corner of the rectangle, or type it in the panel.",
            DrwTool::Modify { pick: ModPick::Rect, .. } => "Click the opposite corner.",
            DrwTool::Modify { pick: ModPick::Circle, first: None, .. } => "Click the middle of the circle, or type it in the panel.",
            DrwTool::Modify { pick: ModPick::Circle, .. } => "Click a point on the circle.",
            DrwTool::Modify { pick: ModPick::Lines, first: None, .. } => "Click where the first break line goes, or type it in the panel.",
            DrwTool::Modify { pick: ModPick::Lines, .. } => "Click where the second break line goes.",
            DrwTool::Overlay => "Overlay: click a view of an assembly.",
            DrwTool::Set(p) => p.prompt(),
        }
    }
}

/// The Drawing View dialog (a base view).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BaseDialog {
    pub file: String,
    pub orientation: Orientation,
    /// "" fits the sheet; else "1:2", "2:1", or a number.
    pub scale: String,
    pub hidden: bool,
}

/// The Edit View dialog.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ViewDialog {
    pub view: ViewId,
    pub scale: String,
    pub hidden: bool,
    pub centerlines: bool,
    pub label: bool,
    /// Of an auxiliary view: from which side of its edge it is seen.
    pub flip: Option<bool>,
}

/// The Text dialog: new text at `at`, or a note's text being changed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextDialog {
    pub at: Vec2,
    pub text: String,
    pub note: Option<tenon_drawing::AnnotId>,
}

/// The Suggested Dimensions dialog: a view's suggestions, each kept or not, with what each would
/// draw (for the preview on the sheet).
#[derive(Clone, Debug)]
pub(crate) struct SuggestDialog {
    pub view: ViewId,
    /// Label and whether it is kept.
    pub items: Vec<(String, bool)>,
    pub preview: Vec<Arc<Graphics>>,
    /// The drawing's revision they were made for.
    pub revision: u64,
}

/// What a job sent to the worker is for.
pub(crate) enum DrwJob {
    /// The views, for the drawing as it was (its evaluation key).
    Evaluate(u64),
    /// A model's size, to pick the scale of a base view about to be placed with these parameters.
    Base { model: String, params: Value },
}

/// An open drawing.
pub(crate) struct DrwDoc {
    pub session: DrwSession,
    pub path: Option<PathBuf>,
    /// The model that does not build that the status bar has said so of.
    failing_said: Option<String>,
    /// The sheet shown.
    pub sheet: SheetId,
    pub cam: SheetCam,
    pub fitted: bool,
    pub selected: Option<Owner>,
    pub hover: Option<Owner>,
    /// A view or annotation being dragged, and how far so far (sheet mm).
    pub drag: Option<(Owner, Vec2)>,
    pub tool: Option<DrwTool>,
    pub base_dialog: Option<BaseDialog>,
    pub view_dialog: Option<ViewDialog>,
    pub text_dialog: Option<TextDialog>,
    pub suggest_dialog: Option<SuggestDialog>,
    pub mod_panel: Option<ModPanel>,
    pub overlay_panel: Option<OverlayPanel>,
    /// A view that has just been modified: what computing it finds is said once, when it is computed.
    pub report: Option<ViewId>,
    pub jobs: BTreeMap<u64, DrwJob>,
    /// The model being edited from the drawing (its key), while it is.
    pub editing: Option<String>,
    /// Where the pointer is on the sheet.
    pub pointer: Option<Vec2>,
    /// When the model files were last checked for changes on disk (UI time, seconds).
    checked_at: f64,
    /// The sheet as drawn, its problems, and what it was drawn from.
    graphics: Option<(u64, Arc<Graphics>, Arc<Vec<String>>)>,
    /// Bumped whenever computed views arrive.
    eval_seq: u64,
}

impl DrwDoc {
    pub(crate) fn new(session: DrwSession, path: Option<PathBuf>) -> DrwDoc {
        let sheet = session.drawing().sheets[0].id;
        DrwDoc {
            session,
            path,
            failing_said: None,
            sheet,
            cam: SheetCam::default(),
            fitted: false,
            selected: None,
            hover: None,
            drag: None,
            tool: None,
            base_dialog: None,
            view_dialog: None,
            text_dialog: None,
            suggest_dialog: None,
            mod_panel: None,
            overlay_panel: None,
            report: None,
            jobs: BTreeMap::new(),
            editing: None,
            pointer: None,
            checked_at: 0.0,
            graphics: None,
            eval_seq: 0,
        }
    }

    /// The computed views shown: current ones, or the last while new ones are computed.
    pub(crate) fn shown_evaluation(&self) -> Option<&Evaluation> {
        self.session.evaluation().or(self.session.last_evaluation())
    }

    /// The view drawn under a sheet point (its outline, with a margin), the most recent on top. A
    /// click on an overlay means the view it is over.
    fn view_at(&self, g: &Graphics, at: Vec2) -> Option<ViewId> {
        let d = self.session.drawing();
        d.views
            .iter()
            .rev()
            .filter(|v| v.sheet == self.sheet && !matches!(v.kind, ViewKind::Overlay { .. }))
            .map(|v| v.id)
            .find(|v| over_view(g, *v, at))
    }
}

/// Whether a sheet point is over a view: inside its outline, with a margin.
pub(crate) fn over_view(g: &Graphics, v: ViewId, at: Vec2) -> bool {
    let m = 3.0;
    g.bounds_of(Owner::View(v)).is_some_and(|(lo, hi)| at.x >= lo.x - m && at.x <= hi.x + m && at.y >= lo.y - m && at.y <= hi.y + m)
}

/// The side of a parent view a point is on, and where a view projected there lines up with it.
fn projected_place(parent: Vec2, at: Vec2) -> (Side, Vec2) {
    let d = at - parent;
    let (ax, ay) = (d.x.abs(), d.y.abs());
    if ax > 2.0 * ay {
        (if d.x > 0.0 { Side::Right } else { Side::Left }, Vec2::new(at.x, parent.y))
    } else if ay > 2.0 * ax {
        (if d.y > 0.0 { Side::Above } else { Side::Below }, Vec2::new(parent.x, at.y))
    } else {
        let side = match (d.x > 0.0, d.y > 0.0) {
            (true, true) => Side::AboveRight,
            (false, true) => Side::AboveLeft,
            (true, false) => Side::BelowRight,
            (false, false) => Side::BelowLeft,
        };
        (side, at)
    }
}

fn v2(p: Vec2) -> Value {
    json!([p.x, p.y])
}

fn pick_at(p: &Value) -> Option<Vec2> {
    Some(Vec2::new(p["at"][0].as_f64()?, p["at"][1].as_f64()?))
}

/// The kind of dimension two picks (or one) and a placement point make: a circle alone its
/// diameter (radius for an arc); a line alone its length along its direction; two lines that
/// meet an angle; else the distance across or along, by where it is placed.
fn dimension_type(picks: &[Value], at: Vec2) -> &'static str {
    let kind = |p: &Value| p["kind"].as_str().unwrap_or("").to_owned();
    match picks {
        [a] if kind(a) == "circle" => {
            let d = a["diameter"].as_f64().unwrap_or(0.0);
            let length = a["edge"]["fingerprint"]["length"].as_f64().unwrap_or(0.0);
            if length >= 0.99 * std::f64::consts::PI * d { "diameter" } else { "radius" }
        }
        [a] => {
            let ends = (a["ends"][0][0].as_f64(), a["ends"][0][1].as_f64(), a["ends"][1][0].as_f64(), a["ends"][1][1].as_f64());
            match ends {
                (Some(x0), Some(y0), Some(x1), Some(y1)) if (y1 - y0).abs() <= 1e-6 * (x1 - x0).abs().max(1.0) => "horizontal",
                (Some(x0), Some(y0), Some(x1), Some(y1)) if (x1 - x0).abs() <= 1e-6 * (y1 - y0).abs().max(1.0) => "vertical",
                _ => "aligned",
            }
        }
        [a, b] => {
            if kind(a) == "line" && kind(b) == "line" {
                let dir = |p: &Value| {
                    let e = &p["ends"];
                    Vec2::new(
                        e[1][0].as_f64().unwrap_or(0.0) - e[0][0].as_f64().unwrap_or(0.0),
                        e[1][1].as_f64().unwrap_or(0.0) - e[0][1].as_f64().unwrap_or(0.0),
                    )
                    .normalized()
                };
                let (u, w) = (dir(a), dir(b));
                if (u.x * w.y - u.y * w.x).abs() > 1e-6 {
                    return "angle";
                }
            }
            let (Some(p), Some(q)) = (pick_at(a), pick_at(b)) else { return "aligned" };
            let between = |v: f64, a: f64, b: f64| v >= a.min(b) && v <= a.max(b);
            if between(at.x, p.x, q.x) {
                "horizontal"
            } else if between(at.y, p.y, q.y) {
                "vertical"
            } else {
                "aligned"
            }
        }
        _ => "aligned",
    }
}

impl Workbench {
    /// A drawing is open and no model is being edited from it.
    pub(crate) fn in_drawing(&self) -> bool {
        self.drw.as_ref().is_some_and(|d| d.editing.is_none())
    }

    /// A model of the open drawing is being edited.
    pub(crate) fn editing_from_drawing(&self) -> bool {
        self.drw.as_ref().is_some_and(|d| d.editing.is_some())
    }

    /// The open drawing's session (tests and tools).
    pub fn drawing(&self) -> Option<&DrwSession> {
        self.drw.as_ref().map(|d| &d.session)
    }

    /// Runs a drawing command (`drw.*`) on the open drawing.
    pub fn drw_exec(&mut self, id: &str, params: Value) -> Result<Value, String> {
        let d = self.drw.as_mut().ok_or("no drawing is open")?;
        let kernel: Option<&mut dyn tenon_kernel::Kernel> = match &mut self.geo {
            Geo::Sync(k) => Some(k.as_mut()),
            _ => None,
        };
        let r = tenon_io::drw::run(&mut d.session, id, &params, kernel).map_err(|e| e.to_string());
        // What was selected or shown may be gone (deleted, undone).
        let dr = d.session.drawing();
        if dr.sheet(d.sheet).is_none() {
            d.sheet = dr.sheets[0].id;
        }
        let gone = |o: &Owner| match o {
            Owner::View(v) => dr.view(*v).is_none(),
            Owner::Annotation(a) => !dr.annotations.iter().any(|x| x.id == *a),
            Owner::Mod(v, m) => dr.view(*v).is_none_or(|x| !x.mods.iter().any(|y| y.id == *m)),
            Owner::Frame => false,
        };
        if d.selected.as_ref().is_some_and(gone) {
            d.selected = None;
        }
        if d.hover.as_ref().is_some_and(gone) {
            d.hover = None;
        }
        r
    }

    fn enter_drawing(&mut self, doc: DrwDoc) {
        self.leave_drawing();
        self.leave_assembly();
        self.session.replace_document(tenon_model::Document::default(), None);
        self.drw = Some(Box::new(doc));
        self.path = None;
        self.mode = Mode::Model;
        self.panel = None;
        self.scene = tenon_model::Scene::default();
        self.scene_seq += 1;
        self.shown = None;
        self.shown_key = None;
        self.chrome.tab = 0;
    }

    /// Closes the drawing (after bringing back a model being edited from it).
    pub(crate) fn leave_drawing(&mut self) {
        if self.editing_from_drawing() {
            self.return_to_drawing();
        }
        self.drw = None;
    }

    pub(crate) fn new_drawing(&mut self) {
        let mut s = DrwSession::default();
        let _ = tenon_io::drw::run(&mut s, "drw.new", &json!({ "name": "Drawing1" }), None);
        self.enter_drawing(DrwDoc::new(s, None));
        self.set_status("New drawing: place a base view of a part or assembly file (Place Views > Base).");
    }

    /// A new drawing starting from a template: its standard, properties, sheets and notes.
    pub fn new_drawing_from(&mut self, template: &Path) -> Result<(), String> {
        let mut s = DrwSession::default();
        tenon_io::drw::run(&mut s, "drw.new", &json!({ "name": "Drawing1", "template": template.to_string_lossy() }), None)
            .map_err(|e| e.to_string())?;
        self.enter_drawing(DrwDoc::new(s, None));
        let name = template.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.set_status(format!("New drawing from {name}: place a base view (Place Views > Base)."));
        Ok(())
    }

    /// Opens a drawing and the model files its views show.
    pub fn open_drawing(&mut self, path: &Path) -> Result<(), String> {
        let (d, models) = tenon_io::drw::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let missing: Vec<String> =
            DrwSession::missing_of(&models).into_iter().map(|(file, why)| format!("{}: {why}", tenon_drawing::views::file_name(&file))).collect();
        let mut s = DrwSession::default();
        s.replace(d, models);
        tenon_io::drw::stamp_all(&mut s);
        self.enter_drawing(DrwDoc::new(s, Some(path.to_path_buf())));
        match missing.first() {
            Some(m) => self.set_error(format!("Opened {} ({} model file(s) missing: {m})", path.display(), missing.len())),
            None => self.set_status(format!("Opened {}", path.display())),
        }
        Ok(())
    }

    /// Saves the drawing, and the models changed from it.
    pub(crate) fn save_drawing(&mut self, path: &Path) -> Result<(), String> {
        let r = self.drw_exec("drw.save", json!({ "path": path.to_string_lossy() }))?;
        if let Some(d) = self.drw.as_mut() {
            d.path = Some(path.to_path_buf());
        }
        let models = r["models_saved"].as_array().map_or(0, Vec::len);
        let note = crate::workbench::kept_note(&r);
        self.set_status(if models > 0 {
            format!("Saved {} and {models} model file(s){note}", path.display())
        } else {
            format!("Saved {}{note}", path.display())
        });
        Ok(())
    }

    /// Save (or Save As) of the drawing: asks where when it was never saved. A model being edited
    /// from it, and a part edited in place in that model, are saved with it.
    pub(crate) fn save_drawing_ui(&mut self, save_as: bool) -> Result<(), String> {
        let (saved, name) = self.drw.as_ref().map(|d| (d.path.clone(), d.session.drawing().name.clone())).unwrap_or_default();
        let path = match saved.filter(|_| !save_as) {
            Some(p) => p,
            None => {
                let file = format!("{name}.{}", tenon_io::drw::EXTENSION);
                self.pick_where_to_save(&file, tenon_io::drw::EXTENSION)?
            }
        };
        self.with_parts_home(|wb| wb.with_model_home(|wb| wb.save_drawing(&path)))
    }

    /// Runs `f` with the model edited from the drawing back in it (for saving).
    pub(crate) fn with_model_home<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let swap = |wb: &mut Self| {
            let Some(d) = wb.drw.as_mut() else { return };
            let Some(key) = d.editing.clone() else { return };
            match d.session.models.get_mut(&key) {
                Some(DrwModel::Part(s)) => std::mem::swap(&mut wb.session, &mut **s),
                Some(DrwModel::Assembly(a)) => {
                    if let Some(doc) = wb.asm.as_mut() {
                        std::mem::swap(&mut doc.session, &mut **a);
                    }
                }
                _ => {}
            }
        };
        swap(self);
        let r = f(self);
        swap(self);
        r
    }

    /// Computes the views when they are out of date: here, or on the worker (one job at a time).
    pub(crate) fn sync_drawing(&mut self) {
        let Some(d) = self.drw.as_mut() else { return };
        // A model being edited is not in the drawing's hands until Return.
        if d.editing.is_some() {
            return;
        }
        // Model files saved by another program or window: about once a second.
        if self.view.now - d.checked_at >= MODEL_CHECK_SECONDS || self.view.now < d.checked_at {
            d.checked_at = self.view.now;
            let changed = tenon_io::drw::reload_changed(&mut d.session);
            if !changed.kept.is_empty() {
                self.set_error(format!(
                    "{} changed on disk, but the drawing has unsaved changes to it: they are kept (Update reads the file instead)",
                    changed.kept.join(", ")
                ));
            } else if !changed.reloaded.is_empty() {
                self.set_status(format!("{} changed on disk: the drawing follows.", changed.reloaded.join(", ")));
            }
        }
        let Some(d) = self.drw.as_mut() else { return };
        if d.session.evaluation().is_some() {
            return;
        }
        let key = d.session.eval_key();
        match &mut self.geo {
            Geo::Sync(k) => {
                d.session.refresh(k.as_mut());
                d.eval_seq += 1;
                self.say_model_failing();
            }
            Geo::Worker(w) => {
                if d.jobs.values().any(|j| matches!(j, DrwJob::Evaluate(_))) {
                    return;
                }
                self.step_seq += 1;
                let request = self.step_seq;
                let (sources, drawing) = (d.session.sources(), d.session.drawing().clone());
                d.jobs.insert(request, DrwJob::Evaluate(key));
                w.job(request, Box::new(move |k, _| Box::new(tenon_drawing::views::evaluate(k, &sources, &drawing))));
            }
            Geo::Off => {}
        }
    }

    /// A model that does not build is said in the status bar when it is found, and then not again
    /// until something else is the matter. (Its views say so for as long as it lasts.)
    fn say_model_failing(&mut self) {
        let Some(d) = self.drw.as_mut() else { return };
        let failing = d.session.evaluation().and_then(|ev| ev.models.values().find_map(|m| m.error.clone()));
        if failing != d.failing_said {
            d.failing_said.clone_from(&failing);
            if let Some(said) = failing {
                self.set_error(said);
            }
        }
    }

    /// True when the job is the drawing's.
    pub(crate) fn owns_drw_job(&self, request: u64) -> bool {
        self.drw.as_ref().is_some_and(|d| d.jobs.contains_key(&request))
    }

    /// A drawing job finished.
    pub(crate) fn drw_job(&mut self, request: u64, result: Box<dyn std::any::Any + Send>) {
        let Some(d) = self.drw.as_mut() else { return };
        let Some(job) = d.jobs.remove(&request) else { return };
        let Ok(ev) = result.downcast::<Evaluation>() else {
            self.set_error("internal: unexpected result from the geometry thread");
            return;
        };
        let mut ev = *ev;
        d.session.explain_missing(&mut ev);
        match job {
            DrwJob::Evaluate(key) => {
                d.session.set_evaluation(key, ev);
                d.eval_seq += 1;
                self.say_model_failing();
            }
            DrwJob::Base { model, mut params } => {
                let size = d.session.drawing().sheet(d.sheet).map(|s| Vec2::new(s.size.width, s.size.height));
                let o = params["orientation"].as_str().and_then(Orientation::from_id).unwrap_or(Orientation::Front);
                if let Some(e) = ev.models.get(&model).and_then(|m| m.error.clone()) {
                    self.set_error(format!("The model does not build: {e}"));
                    return;
                }
                let scale =
                    ev.models.get(&model).and_then(|m| seen_size(m, &base_frame(o))).zip(size).map(|(z, s)| tenon_drawing::cmd::fitting_scale(z, s));
                params["scale"] = json!(scale.unwrap_or(1.0));
                let r = tenon_drawing::cmd::add_base_view(&mut d.session, &model, &params).map_err(|e| e.to_string());
                self.after_base_view(r);
            }
        }
    }

    /// Places a base view of a part or assembly file on the sheet shown, at `scale` (None: the
    /// largest standard scale that fits), then starts placing views projected from it.
    pub fn place_base_view(&mut self, model: &Path, orientation: Orientation, scale: Option<f64>, hidden: bool) -> Result<(), String> {
        let d = self.drw.as_mut().ok_or("no drawing is open")?;
        let sh = d.session.drawing().sheet(d.sheet).ok_or("no sheet")?.clone();
        let key = tenon_io::asm::part_key(model);
        let mut params = json!({
            "model": key,
            "orientation": orientation.id(),
            "sheet": sh.id.0,
            "at": [sh.size.width * 0.3, sh.size.height * 0.55],
            "hidden": hidden && orientation != Orientation::Iso,
        });
        if let Some(s) = scale {
            params["scale"] = json!(s);
        }
        match (&mut self.geo, scale) {
            (Geo::Worker(w), None) => {
                // The scale needs the model's size: build it on the worker first.
                if !d.session.models.contains_key(&key) {
                    let m = tenon_io::drw::load_model(model);
                    if let DrwModel::Missing(why) = &m {
                        return Err(format!("cannot open {}: {why}", model.display()));
                    }
                    d.session.models.insert(key.clone(), m);
                    tenon_io::drw::restamp(&mut d.session, &key);
                }
                let (sources, probe) = tenon_io::drw::probe(&d.session, &key);
                self.step_seq += 1;
                let request = self.step_seq;
                d.jobs.insert(request, DrwJob::Base { model: key, params });
                w.job(request, Box::new(move |k, _| Box::new(tenon_drawing::views::evaluate(k, &sources, &probe))));
                self.set_status("Placing the view...");
                Ok(())
            }
            _ => {
                let r = self.drw_exec("drw.view.base", params);
                self.after_base_view(r);
                Ok(())
            }
        }
    }

    fn after_base_view(&mut self, r: Result<Value, String>) {
        match r {
            Ok(v) => {
                let id = v["view"].as_u64().and_then(|n| u32::try_from(n).ok()).map(ViewId);
                let tool = id.map(|p| DrwTool::Projected { parent: Some(p) });
                self.set_status(tool.as_ref().map_or("Ready", DrwTool::prompt));
                if let Some(d) = self.drw.as_mut() {
                    d.selected = id.map(Owner::View);
                    d.tool = tool;
                }
            }
            Err(e) => self.set_error(e),
        }
    }

    fn selected_view(&self) -> Option<ViewId> {
        match self.drw.as_ref()?.selected {
            Some(Owner::View(v)) => Some(v),
            _ => None,
        }
    }

    fn start_tool(&mut self, tool: DrwTool) {
        let prompt = tool.prompt();
        if let Some(d) = self.drw.as_mut() {
            d.tool = Some(tool);
            // The clicks are this tool's now, and a panel left open would take none.
            d.mod_panel = None;
            d.overlay_panel = None;
        }
        self.set_status(prompt);
    }

    /// Drawing commands from the ribbon, menus and keys.
    pub(crate) fn run_drw_ui(&mut self, id: &str) -> Result<(), String> {
        if id == "file.new_drawing" {
            self.new_drawing();
            return Ok(());
        }
        if id == "file.new_drawing_template" {
            let path = self.services.pick_open_ext.as_ref().and_then(|f| f(tenon_io::drw::EXTENSION)).ok_or("no template chosen")?;
            return self.new_drawing_from(&path);
        }
        if id == "drw.return" {
            if !self.editing_from_drawing() {
                return Err("no model is being edited from a drawing".into());
            }
            // A part edited in place in that assembly goes back to it first.
            if self.editing_in_place() {
                self.return_to_assembly();
            }
            self.return_to_drawing();
            return Ok(());
        }
        if !self.in_drawing() {
            return Err("open or start a drawing first (File > New Drawing)".into());
        }
        let selected = self.selected_view();
        // The view a new one is made from: what is drawn over a view stands for that view.
        let source = self.drw.as_ref().and_then(|d| source_view(d, d.selected));
        match id {
            "drw.base" => {
                let file = self.drw.as_ref().and_then(|d| d.session.drawing().views.last().map(|v| v.model.clone())).unwrap_or_default();
                if let Some(d) = self.drw.as_mut() {
                    d.tool = None;
                    d.base_dialog = Some(BaseDialog { file, orientation: Orientation::Front, scale: String::new(), hidden: true });
                }
                self.set_status("Choose the file, orientation and scale of the view.");
            }
            "drw.projected" => self.start_tool(DrwTool::Projected { parent: source }),
            "drw.section" => self.start_tool(DrwTool::Section { parent: source, line: Vec::new() }),
            "drw.detail" => self.start_tool(DrwTool::Detail { parent: source, center: None, radius: None }),
            "drw.auxiliary" => self.start_tool(DrwTool::Auxiliary { parent: source, edge: None, changing: None }),
            "drw.crop" => self.open_mod_panel(ModWhat::Crop, source)?,
            "drw.break" => self.open_mod_panel(ModWhat::Break, source)?,
            "drw.break_out" => self.open_mod_panel(ModWhat::BreakOut, source)?,
            "drw.overlay" => match source {
                Some(v) => self.open_overlay_panel(v, None)?,
                None => self.start_tool(DrwTool::Overlay),
            },
            "drw.edit_modifier" => match self.drw.as_ref().and_then(|d| d.selected) {
                Some(Owner::Mod(v, m)) => self.edit_modifier(v, m)?,
                _ => return Err("select a crop, a break or a break out first".into()),
            },
            "drw.dimension" => self.start_tool(DrwTool::Dimension { view: None, picks: Vec::new() }),
            "drw.dimension.auto" => match selected {
                Some(v) => self.open_suggestions(v)?,
                None => self.start_tool(DrwTool::AutoDimension),
            },
            "drw.balloon" => self.start_tool(DrwTool::Balloon { view: None, attach: None }),
            "drw.balloon.auto" => match selected {
                Some(v) => {
                    let r = self.drw_exec("drw.balloon.auto", json!({ "view": v.0 }))?;
                    self.set_status(format!("{} balloon(s) added.", r["added"]));
                }
                None => self.start_tool(DrwTool::AutoBalloon),
            },
            "drw.hole_table" => self.start_tool(DrwTool::HoleTable { view: selected }),
            "drw.parts_list" => self.start_tool(DrwTool::PartsList { view: selected }),
            "drw.text" => self.start_tool(DrwTool::Text),
            "drw.center_mark" => self.start_tool(DrwTool::CenterMark),
            "drw.centerline" | "drw.centerline.bisector" => {
                self.start_tool(DrwTool::Centerline { bisector: id == "drw.centerline.bisector", view: None, a: None })
            }
            "drw.baseline" | "drw.ordinate" => self.start_tool(DrwTool::Set(Box::new(SetPanel::new(id == "drw.ordinate")))),
            "drw.sheet.new" => {
                let size = self.drw.as_ref().and_then(|d| d.session.drawing().sheet(d.sheet).map(|s| s.size.name.clone())).unwrap_or_default();
                let r = self.drw_exec("drw.sheet.add", json!({ "size": size }))?;
                if let (Some(d), Some(n)) = (self.drw.as_mut(), r["sheet"].as_u64().and_then(|n| u32::try_from(n).ok())) {
                    d.sheet = SheetId(n);
                    d.fitted = false;
                    d.selected = None;
                }
                self.set_status(format!("Sheet:{} added.", r["sheet"]));
            }
            "drw.update" => {
                let r = self.drw_exec("drw.update", json!({}))?;
                self.set_status(format!(
                    "Read again: {}",
                    r["models"].as_array().map(|m| m.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default()
                ));
            }
            "drw.edit_view" => {
                let v = selected.ok_or("select a view first")?;
                let view = self.drw.as_ref().and_then(|d| d.session.drawing().view(v).cloned()).ok_or("the view is gone")?;
                // An overlay's panel is its position.
                if let ViewKind::Overlay { parent, .. } = &view.kind {
                    return self.open_overlay_panel(*parent, Some(v));
                }
                let flip = match &view.kind {
                    ViewKind::Auxiliary { flip, .. } => Some(*flip),
                    _ => None,
                };
                if let Some(d) = self.drw.as_mut() {
                    d.view_dialog = Some(ViewDialog {
                        view: v,
                        scale: tenon_drawing::annotate::scale_text(view.scale),
                        hidden: view.hidden,
                        centerlines: view.centerlines,
                        label: view.label,
                        flip,
                    });
                }
            }
            "drw.template.apply" => {
                let path = self.services.pick_open_ext.as_ref().and_then(|f| f("json")).ok_or("no file chosen")?;
                let r = self.drw_exec("drw.template.apply", json!({ "path": path.to_string_lossy() }))?;
                self.set_status(format!("Title block {} on {} sheet(s).", r["name"], r["sheets"]));
            }
            "drw.template.save" => {
                let (name, sheet) = self
                    .drw
                    .as_ref()
                    .map(|d| (d.session.drawing().sheet(d.sheet).map(|s| s.title_block.name.clone()).unwrap_or_default(), d.sheet.0))
                    .unwrap_or_default();
                let path = self.services.pick_save.as_ref().and_then(|f| f(&format!("{name}.json"), "json")).ok_or("no file chosen")?;
                self.drw_exec("drw.template.save", json!({ "path": path.to_string_lossy(), "sheet": sheet }))?;
                self.set_status(format!("Saved {}", path.display()));
            }
            "drw.save_template" => {
                let name = self.drw.as_ref().map(|d| d.session.drawing().name.clone()).unwrap_or_default();
                let ext = tenon_io::drw::EXTENSION;
                let path = self.services.pick_save.as_ref().and_then(|f| f(&format!("{name} template.{ext}"), ext)).ok_or("no file chosen")?;
                self.drw_exec("drw.save_template", json!({ "path": path.to_string_lossy() }))?;
                self.set_status(format!("Saved the template {}", path.display()));
            }
            "drw.edit_model" => self.edit_model_from_drawing()?,
            "drw.delete" => self.delete_selected()?,
            "export.pdf" | "export.svg" | "export.dxf" => self.export_drawing(&id["export.".len()..])?,
            _ => return Err(format!("unknown command `{id}`")),
        }
        Ok(())
    }

    /// Opens the Suggested Dimensions dialog for a view, with what each suggestion would draw.
    pub(crate) fn open_suggestions(&mut self, view: ViewId) -> Result<(), String> {
        let d = self.drw.as_mut().ok_or("no drawing is open")?;
        let ev = d.session.evaluation().ok_or("the views are still being computed; try again in a moment")?;
        let dr = d.session.drawing();
        let list = tenon_drawing::suggest::suggest(dr, ev, view)?;
        if list.is_empty() {
            self.set_status("This view already has the dimensions Tenon would suggest.");
            return Ok(());
        }
        // What they would draw: the drawing with all of them added, each one's own lines.
        let v = dr.view(view).ok_or("the view is gone")?.clone();
        let mut copy = dr.clone();
        let ids: Vec<tenon_drawing::AnnotId> = list
            .iter()
            .map(|s| {
                let id = copy.take_annotation_id();
                copy.annotations.push(tenon_drawing::Annotation { id, kind: s.kind(view, &v) });
                id
            })
            .collect();
        let (g, _) = tenon_drawing::annotate::build(&copy, v.sheet, ev);
        let only = |o: Owner| Graphics {
            sheet: g.sheet,
            width: g.width,
            height: g.height,
            strokes: g.strokes.iter().filter(|x| x.0 == o).cloned().collect(),
            fills: g.fills.iter().filter(|x| x.0 == o).cloned().collect(),
            texts: g.texts.iter().filter(|x| x.0 == o).cloned().collect(),
        };
        let preview = ids.iter().map(|id| Arc::new(only(Owner::Annotation(*id)))).collect();
        let items = list.iter().map(|s| (format!("{}   {}", s.shown, s.why), true)).collect();
        let n = list.len();
        if let Some(d) = self.drw.as_mut() {
            d.suggest_dialog = Some(SuggestDialog { view, items, preview, revision: d.session.revision() });
        }
        self.set_status(format!("{n} dimension(s) suggested: untick any not wanted, then Add."));
        Ok(())
    }

    /// Undo or redo in the drawing.
    pub(crate) fn drw_undo(&mut self, undo: bool) -> Result<(), String> {
        // (Nothing to undo is not a fault: it is said, plainly, as in a part.)
        if let Err(e) = self.drw_exec(if undo { "drw.undo" } else { "drw.redo" }, json!({})) {
            if e.starts_with("nothing to ") {
                self.set_status(if undo { "Nothing to undo." } else { "Nothing to redo." });
                return Ok(());
            }
            return Err(e);
        }
        if let Some(d) = self.drw.as_mut() {
            d.tool = None;
            d.mod_panel = None;
            d.overlay_panel = None;
        }
        self.set_status(if undo { "Undone." } else { "Redone." });
        Ok(())
    }

    fn delete_selected(&mut self) -> Result<(), String> {
        let sel = self.drw.as_ref().and_then(|d| d.selected).ok_or("select a view or an annotation first")?;
        match sel {
            Owner::View(v) => {
                self.drw_exec("drw.view.delete", json!({ "view": v.0 }))?;
                self.set_status("View deleted, with the views made from it.");
            }
            Owner::Annotation(a) => {
                self.drw_exec("drw.delete", json!({ "annotation": a.0 }))?;
                self.set_status("Deleted.");
            }
            Owner::Mod(v, m) => {
                self.drw_exec("drw.modifier.delete", json!({ "view": v.0, "modifier": m }))?;
                self.set_status("Deleted: the view is drawn without it.");
            }
            Owner::Frame => return Err("the border and title block stay".into()),
        }
        if let Some(d) = self.drw.as_mut() {
            d.selected = None;
            d.hover = None;
        }
        Ok(())
    }

    fn export_drawing(&mut self, kind: &str) -> Result<(), String> {
        let name = self.drw.as_ref().map(|d| d.session.drawing().name.clone()).unwrap_or_else(|| "Drawing1".into());
        let path = self.services.pick_save.as_ref().and_then(|f| f(&format!("{name}.{kind}"), kind)).ok_or("no file chosen")?;
        let mut params = json!({ "path": path.to_string_lossy() });
        if kind != "pdf"
            && let Some(d) = self.drw.as_ref()
        {
            params["sheet"] = json!(d.sheet.0);
        }
        self.drw_exec(&format!("drw.export.{kind}"), params)
            .map_err(|e| if e.contains("not computed") { "the views are still being computed; try again in a moment".to_string() } else { e })?;
        self.set_status(format!("Exported {}", path.display()));
        Ok(())
    }

    // ---- editing a model from the drawing -------------------------------------------------

    /// Opens the selected view's part or assembly for editing; Return brings it back.
    pub(crate) fn edit_model_from_drawing(&mut self) -> Result<(), String> {
        let v = self.selected_view().ok_or("select a view of the model to edit")?;
        let d = self.drw.as_mut().ok_or("no drawing is open")?;
        let key = d.session.drawing().view(v).ok_or("the view is gone")?.model.clone();
        let file = tenon_drawing::views::file_name(&key).to_owned();
        match d.session.models.get_mut(&key) {
            Some(DrwModel::Part(s)) => {
                std::mem::swap(&mut self.session, &mut **s);
                d.editing = Some(key.clone());
                d.tool = None;
                self.path = Some(PathBuf::from(&key));
                self.mode = Mode::Model;
                self.panel = None;
                self.view = crate::viewport::View::default();
                self.scene = tenon_model::Scene::default();
                self.scene_seq += 1;
                self.shown = None;
                self.shown_key = None;
                self.chrome.tab = 0;
            }
            Some(DrwModel::Assembly(a)) => {
                let session = std::mem::take(a.as_mut());
                d.editing = Some(key.clone());
                d.tool = None;
                self.asm = Some(Box::new(crate::assembly::AsmDoc::new(session, Some(PathBuf::from(&key)))));
                self.mode = Mode::Model;
                self.panel = None;
                self.view = crate::viewport::View::default();
                self.scene = tenon_model::Scene::default();
                self.scene_seq += 1;
                self.chrome.tab = 0;
            }
            Some(DrwModel::Missing(why)) => return Err(format!("{file} could not be read: {why}")),
            None => return Err(format!("{file} is not loaded")),
        }
        self.set_status(format!("Editing {file}. Return goes back to the drawing, whose views follow the changes."));
        Ok(())
    }

    /// Brings the model edited from the drawing back to it.
    pub(crate) fn return_to_drawing(&mut self) {
        let Some(key) = self.drw.as_mut().and_then(|d| d.editing.take()) else { return };
        let assembly = self.take_assembly();
        let Some(d) = self.drw.as_mut() else { return };
        match d.session.models.get_mut(&key) {
            Some(DrwModel::Part(s)) => std::mem::swap(&mut self.session, &mut **s),
            Some(DrwModel::Assembly(a)) => {
                if let Some(s) = assembly {
                    **a = s;
                }
            }
            _ => {}
        }
        // Saved while it was edited, the model's files are as the drawing holds them.
        tenon_io::drw::restamp(&mut d.session, &key);
        self.session.replace_document(tenon_model::Document::default(), None);
        self.path = None;
        self.mode = Mode::Model;
        self.panel = None;
        self.scene = tenon_model::Scene::default();
        self.scene_seq += 1;
        self.shown = None;
        self.shown_key = None;
        self.chrome.tab = 0;
        self.set_status(format!("Back in the drawing: the views follow the changes to {}.", tenon_drawing::views::file_name(&key)));
    }

    // ---- the sheet ------------------------------------------------------------------------

    /// The shown sheet as graphics, from the views computed so far (cached).
    fn sheet_graphics(&mut self) -> Option<(Arc<Graphics>, Arc<Vec<String>>)> {
        let d = self.drw.as_mut()?;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (d.session.revision(), d.session.eval_key(), d.session.evaluation().is_some(), d.eval_seq, d.sheet.0).hash(&mut h);
        let key = h.finish();
        if let Some((k, g, p)) = &d.graphics
            && *k == key
        {
            return Some((g.clone(), p.clone()));
        }
        let empty = Evaluation::default();
        let ev = d.shown_evaluation().unwrap_or(&empty);
        let (g, problems) = tenon_drawing::annotate::build(d.session.drawing(), d.sheet, ev);
        let problems: Vec<String> = problems.into_iter().map(|(id, m)| format!("{id}: {m}")).collect();
        let (g, problems) = (Arc::new(g), Arc::new(problems));
        d.graphics = Some((key, g.clone(), problems.clone()));
        Some((g, problems))
    }

    /// Where a sheet point is on the screen in the last frame (tests).
    #[cfg(test)]
    pub(crate) fn sheet_on_screen(&self, p: Vec2) -> Pos2 {
        self.drw.as_ref().map_or(Pos2::ZERO, |d| d.cam.screen(self.view.rect, p))
    }

    /// The sheet canvas: the paper, the views and annotations, the tool being used; panning,
    /// zooming, picking, dragging.
    pub(crate) fn drw_canvas(&mut self, ui: &mut Ui, t: &Tokens) {
        let rect = ui.max_rect();
        self.view.rect = rect;
        ui.painter().rect_filled(rect, 0.0, t.viewport_bottom);
        let resp = ui.interact(rect, ui.id().with("sheet"), Sense::CLICK | Sense::DRAG);
        let Some((g, problems)) = self.sheet_graphics() else { return };
        let tool_nav = self.left_drag_tool(ui);
        let Some(d) = self.drw.as_mut() else { return };
        let (w, h) = (g.width, g.height);
        if !d.fitted && rect.width() > 60.0 {
            d.cam.fit(rect, w, h);
            d.fitted = true;
        }
        // Navigation: middle drag (or the Pan tool) pans, the wheel (or the Zoom tool) zooms.
        let delta = resp.drag_delta();
        if resp.dragged_by(egui::PointerButton::Middle)
            || (resp.dragged_by(egui::PointerButton::Primary) && matches!(tool_nav, Nav::Pan | Nav::Orbit))
        {
            d.cam.pan(delta);
        } else if resp.dragged_by(egui::PointerButton::Primary) && tool_nav == Nav::Zoom {
            d.cam.zoom(rect, (-f64::from(delta.y) * 0.01).exp(), rect.center());
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y + (i.zoom_delta() - 1.0) * 200.0);
            if scroll.abs() > 0.01
                && let Some(p) = resp.hover_pos()
            {
                d.cam.zoom(rect, (f64::from(scroll) * 0.0015).exp(), p);
            }
        }
        let cam = d.cam;
        let pointer = resp.hover_pos().or_else(|| resp.interact_pointer_pos()).filter(|p| rect.contains(*p)).map(|p| cam.sheet(rect, p));
        d.pointer = pointer;
        let tol = PICK_PX / cam.px;
        if d.drag.is_none() {
            d.hover = pointer.and_then(|p| g.pick(p, tol)).filter(|o| !matches!(o, Owner::Frame));
        }
        let selecting = d.tool.is_none() && tool_nav == Nav::Select;

        // Dragging a view or an annotation moves it; centre marks and lines stay on their geometry.
        if selecting && resp.drag_started_by(egui::PointerButton::Primary) {
            let grab = ui.input(|i| i.pointer.press_origin()).map(|p| cam.sheet(rect, p)).and_then(|p| g.pick(p, tol));
            if let Some(o @ (Owner::View(_) | Owner::Annotation(_))) = grab {
                let fixed = matches!(o, Owner::Annotation(a) if d.session.drawing().annotation(a).is_some_and(|x| x.kind.on_geometry()));
                if !fixed {
                    d.drag = Some((o, Vec2::new(0.0, 0.0)));
                }
                d.selected = Some(o);
            }
        }
        // From where the button went down: what moved before the drag was recognised counts.
        if let Some((_, by)) = &mut d.drag
            && let (Some(from), Some(now)) = (ui.input(|i| i.pointer.press_origin()), resp.interact_pointer_pos())
        {
            *by = cam.sheet(rect, now) - cam.sheet(rect, from);
        }
        let mut finished_drag = None;
        if resp.drag_stopped() {
            finished_drag = d.drag.take();
        }
        let mut click = None;
        if resp.clicked() {
            click = pointer;
        }
        let double = resp.double_clicked();
        let right = resp.secondary_clicked();
        let keys = !ui.ctx().egui_wants_keyboard_input();
        let (esc, del) = if keys { ui.input(|i| (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Delete))) } else { (false, false) };

        // ---- paint ----
        let p = ui.painter_at(rect);
        let paper = Rect::from_two_pos(cam.screen(rect, Vec2::new(0.0, 0.0)), cam.screen(rect, Vec2::new(w, h)));
        p.rect_filled(paper.translate(vec2(4.0, 4.0)), 0.0, Color32::from_black_alpha(70));
        p.rect_filled(paper, 0.0, Color32::WHITE);
        let moving = d.drag.map(|(o, by)| (moving_owners(&d.session, o), in_line(&d.session, o, by)));
        let color_of = |o: Owner| {
            if d.selected == Some(o) {
                SELECTED
            } else if d.hover == Some(o) {
                HOVER
            } else {
                INK
            }
        };
        let shift_of = |o: Owner| moving.as_ref().filter(|(set, _)| set.contains(&o)).map_or(Vec2::new(0.0, 0.0), |(_, by)| *by);
        // A set being changed is shown as it would be, not as it was.
        let changing = match &d.tool {
            Some(DrwTool::Set(panel)) => panel.annotation.map(Owner::Annotation),
            _ => None,
        };
        for (o, pen, pts) in g.strokes.iter().filter(|s| Some(s.0) != changing) {
            let (color, shift) = (color_of(*o), shift_of(*o));
            let width = ((pen.width() * cam.px) as f32).max(1.0);
            for run in dashed(pts, pen.dashes()) {
                let line: Vec<Pos2> = run.iter().map(|q| cam.screen(rect, *q + shift)).collect();
                if line.len() >= 2 {
                    p.add(Shape::line(line, Stroke::new(width, color)));
                }
            }
        }
        for (o, pts) in g.fills.iter().filter(|f| Some(f.0) != changing) {
            let (color, shift) = (color_of(*o), shift_of(*o));
            p.add(Shape::convex_polygon(pts.iter().map(|q| cam.screen(rect, *q + shift)).collect(), color, Stroke::NONE));
        }
        for (o, text) in g.texts.iter().filter(|t| Some(t.0) != changing) {
            let (color, shift) = (color_of(*o), shift_of(*o));
            let width = ((text.height * tenon_drawing::stroke::PEN * cam.px) as f32).max(1.0);
            for s in tenon_drawing::stroke::text_strokes(&text.text, text.left() + shift, text.height) {
                let line: Vec<Pos2> = s.iter().map(|q| cam.screen(rect, *q)).collect();
                if line.len() >= 2 {
                    p.add(Shape::line(line, Stroke::new(width, color)));
                }
            }
        }
        let tool = d.tool.clone();
        let ghost = Stroke::new(1.4, GHOST);
        if let (Some(tool), Some(at)) = (&tool, pointer) {
            draw_tool(&p, rect, &cam, d, tool, at, ghost);
        }
        draw_panels(&p, rect, &cam, d, pointer, ghost);
        // (With the pointer on its panel too: what is being changed stays in sight.)
        if let Some(DrwTool::Set(panel)) = &tool {
            crate::dim_sets::paint_tool(&p, rect, &cam, d, panel, pointer, GHOST);
        }
        // Suggested dimensions still ticked, as they would be drawn.
        if let Some(sd) = &d.suggest_dialog {
            for (g, _) in sd.preview.iter().zip(&sd.items).filter(|(_, (_, on))| *on) {
                paint_plain(&p, rect, &cam, g, GHOST);
            }
        }
        // What a tool asks for, and what does not draw.
        let mut note_y = rect.top() + 10.0;
        if let Some(tool) = &tool {
            p.text(pos2(rect.left() + 12.0, note_y), Align2::LEFT_TOP, tool.prompt(), theme::body(), t.viewport_text);
            note_y += 20.0;
        }
        if !d.jobs.is_empty() {
            p.text(pos2(rect.left() + 12.0, note_y), Align2::LEFT_TOP, "Computing views...", theme::small(), t.viewport_text);
            note_y += 18.0;
        }
        for msg in problems.iter().take(4) {
            p.text(pos2(rect.left() + 12.0, note_y), Align2::LEFT_TOP, msg, theme::small(), Color32::from_rgb(0xd8, 0x44, 0x38));
            note_y += 16.0;
        }
        let hovered_view = pointer.and_then(|q| d.view_at(&g, q));

        // ---- act ----
        if let Some((o, by)) = finished_drag
            && by.len() > 1e-9
        {
            let r = match o {
                Owner::View(v) => self.drw_exec("drw.view.edit", json!({ "view": v.0, "by": v2(by) })),
                Owner::Annotation(a) => self.drw_exec("drw.annotation.edit", json!({ "annotation": a.0, "by": v2(by) })),
                Owner::Frame | Owner::Mod(..) => Ok(Value::Null),
            };
            if let Err(e) = r {
                self.set_error(e);
            }
        }
        if right && let Some(d) = self.drw.as_mut() {
            d.mod_panel = None;
            if d.tool.take().is_some() {
                self.set_status("Ready");
            } else {
                d.selected = d.hover;
            }
        }
        if esc && let Some(d) = self.drw.as_mut() {
            d.mod_panel = None;
            d.overlay_panel = None;
            if d.tool.take().is_none() {
                d.selected = None;
            }
            self.set_status("Ready");
        }
        if del
            && self.drw.as_ref().is_some_and(|d| d.selected.is_some() && d.tool.is_none())
            && let Err(e) = self.delete_selected()
        {
            self.set_error(e);
        }
        if let Some(at) = click {
            if tool.is_some() {
                if let Err(e) = self.drw_tool_click(at, hovered_view, &g) {
                    self.set_error(e);
                }
            } else if let Some(d) = self.drw.as_mut() {
                d.selected = d.hover;
            }
        }
        if double
            && tool.is_none()
            && let Some(d) = self.drw.as_ref()
        {
            match d.hover {
                Some(Owner::View(_)) => {
                    if let Err(e) = self.run_drw_ui("drw.edit_view") {
                        self.set_error(e);
                    }
                }
                Some(Owner::Mod(v, m)) => {
                    if let Err(e) = self.edit_modifier(v, m) {
                        self.set_error(e);
                    }
                }
                Some(Owner::Annotation(a)) if crate::dim_sets::is_set(d.session.drawing(), a) => {
                    if let Err(e) = self.edit_set(a) {
                        self.set_error(e);
                    }
                }
                Some(Owner::Annotation(a)) => {
                    let note = d.session.drawing().annotations.iter().find(|x| x.id == a).and_then(|x| match &x.kind {
                        AnnotKind::Note { at, text, .. } => Some((*at, text.clone())),
                        _ => None,
                    });
                    if let (Some((at, text)), Some(d)) = (note, self.drw.as_mut()) {
                        d.text_dialog = Some(TextDialog { at, text, note: Some(a) });
                    }
                }
                _ => {}
            }
        }
        // The context menu of what was right-clicked.
        let target = self.drw.as_ref().and_then(|d| d.selected);
        let set = match (target, self.drw.as_ref()) {
            (Some(Owner::Annotation(a)), Some(d)) if crate::dim_sets::is_set(d.session.drawing(), a) => Some(a),
            _ => None,
        };
        let mut chosen: Option<&'static str> = None;
        let mut change_set = None;
        resp.context_menu(|ui| {
            if set.is_some() && button(ui, "Change Set...", "tn_ctx_edit_set").clicked() {
                change_set = set;
                ui.close();
            }
            if let Some(Owner::View(_)) = target {
                if button(ui, "Edit View...", "tn_ctx_edit_view").clicked() {
                    chosen = Some("drw.edit_view");
                    ui.close();
                }
                if button(ui, "Open Model", "tn_ctx_open_model").clicked() {
                    chosen = Some("drw.edit_model");
                    ui.close();
                }
                if button(ui, "Projected View", "tn_ctx_projected").clicked() {
                    chosen = Some("drw.projected");
                    ui.close();
                }
            }
            if let Some(Owner::Mod(..)) = target
                && button(ui, "Edit...", "tn_ctx_edit_modifier").clicked()
            {
                chosen = Some("drw.edit_modifier");
                ui.close();
            }
            if matches!(target, Some(Owner::View(_) | Owner::Annotation(_) | Owner::Mod(..))) && button(ui, "Delete", "tn_ctx_delete").clicked() {
                chosen = Some("drw.delete");
                ui.close();
            }
            if button(ui, "Zoom All", "tn_ctx_zoom_all").clicked() {
                chosen = Some("view.fit");
                ui.close();
            }
        });
        if let Some(id) = chosen {
            self.command(id);
        }
        if let Some(a) = change_set
            && let Err(e) = self.edit_set(a)
        {
            self.set_error(e);
        }
        if let Some(d) = self.drw.as_ref()
            && (d.tool.is_some() || d.drag.is_some())
        {
            ui.ctx().request_repaint();
        }
        // Idle, a frame a second still notices model files saved elsewhere.
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(MODEL_CHECK_SECONDS));
    }

    /// A click on the sheet while a tool runs (`over`: the view under it).
    fn drw_tool_click(&mut self, at: Vec2, over: Option<ViewId>, g: &Graphics) -> Result<(), String> {
        let d = self.drw.as_mut().ok_or("no drawing")?;
        let Some(tool) = d.tool.clone() else { return Ok(()) };
        if matches!(tool, DrwTool::Modify { .. }) {
            return self.modifier_click(at, over);
        }
        let standard = d.session.drawing().standard;
        let need_view = || over.ok_or_else(|| "click inside a view".to_string());
        let next = |wb: &mut Self, t: Option<DrwTool>| {
            let prompt = t.as_ref().map(DrwTool::prompt);
            if let Some(d) = wb.drw.as_mut() {
                d.tool = t;
            }
            match prompt {
                Some(p) => wb.set_status(p),
                None => wb.set_status("Ready"),
            }
        };
        // A view's geometry: for turning sheet points into the view's own coordinates.
        let view_point = |wb: &Self, v: ViewId, q: Vec2| -> Result<Vec2, String> {
            let d = wb.drw.as_ref().ok_or("no drawing")?;
            let view = d.session.drawing().view(v).ok_or("the view is gone")?;
            let geo = d.shown_evaluation().and_then(|e| e.view(v)).ok_or("the view is not computed yet")?;
            Ok(tenon_drawing::annotate::from_sheet(view, geo, q))
        };
        match tool {
            DrwTool::Projected { parent: None } => next(self, Some(DrwTool::Projected { parent: Some(need_view()?) })),
            DrwTool::Projected { parent: Some(p) } => {
                let center = d.session.drawing().view(p).ok_or("the view is gone")?.center;
                let (side, place) = projected_place(center, at);
                // (A view on top of another shows nothing new, whatever the ghost showed.)
                if let Some(under) = d.view_at(g, place).and_then(|v| d.session.drawing().view(v)) {
                    return Err(format!(
                        "{} is already there. Click beside the view for the view from that side, or off at a corner for a slanted one.",
                        under.name
                    ));
                }
                let r = self.drw_exec("drw.view.projected", json!({ "parent": p.0, "side": side.id(), "at": v2(place) }))?;
                self.set_status(format!("{} placed. Click for another, or Esc to end.", r["name"].as_str().unwrap_or("View")));
            }
            DrwTool::Section { parent: None, .. } => next(self, Some(DrwTool::Section { parent: Some(need_view()?), line: Vec::new() })),
            DrwTool::Section { parent: Some(p), mut line } if line.len() < 2 => {
                if line.first().is_some_and(|a| a.dist(at) < 1.0) {
                    return Err("click the other end somewhere else".into());
                }
                line.push(at);
                next(self, Some(DrwTool::Section { parent: Some(p), line }));
            }
            DrwTool::Section { parent: Some(p), line } => {
                let (a, b) = (line[0], line[1]);
                let dir = (b - a).normalized();
                let right = Vec2::new(dir.y, -dir.x);
                let side = right.dot(at - (a + b) * 0.5);
                // Third-angle: the view goes where it is seen from (against the arrows).
                let look_right = if standard == Standard::Ansi { side < 0.0 } else { side > 0.0 };
                let (va, vb) = (view_point(self, p, a)?, view_point(self, p, b)?);
                let r = self.drw_exec("drw.view.section", json!({ "parent": p.0, "a": v2(va), "b": v2(vb), "flip": !look_right, "at": v2(at) }))?;
                next(self, None);
                self.set_status(format!("Section {0}-{0} placed.", r["name"].as_str().unwrap_or("")));
            }
            DrwTool::Detail { parent: None, .. } => next(self, Some(DrwTool::Detail { parent: Some(need_view()?), center: None, radius: None })),
            DrwTool::Detail { parent: Some(p), center: None, .. } => {
                next(self, Some(DrwTool::Detail { parent: Some(p), center: Some(at), radius: None }))
            }
            DrwTool::Detail { parent: Some(p), center: Some(c), radius: None } => {
                let r = c.dist(at);
                if r < 0.5 {
                    return Err("click farther from the centre".into());
                }
                next(self, Some(DrwTool::Detail { parent: Some(p), center: Some(c), radius: Some(r) }));
            }
            DrwTool::Detail { parent: Some(p), center: Some(c), radius: Some(r) } => {
                let scale = d.session.drawing().view(p).ok_or("the view is gone")?.scale;
                let vc = view_point(self, p, c)?;
                let out = self.drw_exec("drw.view.detail", json!({ "parent": p.0, "center": v2(vc), "radius": r / scale, "at": v2(at) }))?;
                next(self, None);
                self.set_status(format!("Detail {} placed.", out["name"].as_str().unwrap_or("")));
            }
            DrwTool::Dimension { view, mut picks } => {
                // An edge where the click is: picked; elsewhere, the dimension goes there.
                let target = view.or(over);
                if picks.len() < 2
                    && let Some(v) = target
                    && (view.is_none() || over == view)
                    && let Ok(mut pick) = self.drw_exec("drw.pick", json!({ "view": v.0, "at": v2(at) }))
                {
                    // (Of a curve, its chord.)
                    if (pick["kind"] == "line" || pick["kind"] == "curve")
                        && let Some(ends) = line_ends(self, v, &pick)
                    {
                        pick["ends"] = json!([v2(ends.0), v2(ends.1)]);
                    }
                    picks.push(pick);
                    next(self, Some(DrwTool::Dimension { view: Some(v), picks }));
                    return Ok(());
                }
                let v = view.ok_or("click an edge of a view")?;
                let kind = dimension_type(&picks, at);
                let mut params = json!({ "view": v.0, "type": kind, "a": picks[0], "at": v2(at) });
                if let Some(b) = picks.get(1) {
                    params["b"] = b.clone();
                }
                let r = self.drw_exec("drw.dimension", params)?;
                next(self, Some(DrwTool::Dimension { view: None, picks: Vec::new() }));
                self.set_status(format!("Dimension {} placed. Click the next edge, or Esc to end.", r["shown"].as_str().unwrap_or("")));
            }
            DrwTool::AutoDimension => {
                let v = need_view()?;
                next(self, None);
                self.open_suggestions(v)?;
            }
            DrwTool::Balloon { attach: None, .. } => {
                let v = need_view()?;
                let pick = self.drw_exec("drw.pick", json!({ "view": v.0, "at": v2(at) })).map_err(|_| "click an edge of a part".to_string())?;
                if pick.get("component").is_none_or(Value::is_null) {
                    return Err("balloons are for assembly views".into());
                }
                next(self, Some(DrwTool::Balloon { view: Some(v), attach: Some(at) }));
            }
            DrwTool::Balloon { view, attach: Some(q) } => {
                let v = view.ok_or("click an edge of a part first")?;
                let r = self.drw_exec("drw.balloon", json!({ "view": v.0, "attach_at": v2(q), "at": v2(at) }))?;
                next(self, Some(DrwTool::Balloon { view: None, attach: None }));
                self.set_status(format!("Balloon {} placed.", r["item"]));
            }
            DrwTool::AutoBalloon => {
                let v = need_view()?;
                let r = self.drw_exec("drw.balloon.auto", json!({ "view": v.0 }))?;
                next(self, None);
                self.set_status(format!("{} balloon(s) added.", r["added"]));
            }
            DrwTool::HoleTable { view: None } => next(self, Some(DrwTool::HoleTable { view: Some(need_view()?) })),
            DrwTool::HoleTable { view: Some(v) } => {
                let r = self.drw_exec("drw.hole_table", json!({ "view": v.0, "at": v2(at) }))?;
                next(self, None);
                self.set_status(format!("Hole table: {} hole(s).", r["rows"].as_array().map_or(0, Vec::len)));
            }
            DrwTool::PartsList { view: None } => next(self, Some(DrwTool::PartsList { view: Some(need_view()?) })),
            DrwTool::PartsList { view: Some(v) } => {
                let r = self.drw_exec("drw.parts_list", json!({ "view": v.0, "at": v2(at) }))?;
                next(self, None);
                self.set_status(format!("Parts list: {} part(s).", r["rows"].as_array().map_or(0, Vec::len)));
            }
            DrwTool::Text => {
                if let Some(d) = self.drw.as_mut() {
                    d.text_dialog = Some(TextDialog { at, text: String::new(), note: None });
                    d.tool = None;
                }
            }
            DrwTool::CenterMark => {
                let v = need_view()?;
                let pick = self.drw_exec("drw.pick", json!({ "view": v.0, "at": v2(at) })).map_err(|_| "click a circle or an arc".to_string())?;
                self.drw_exec("drw.center_mark", json!({ "view": v.0, "a": pick }))?;
                self.set_status("Centre mark placed. Click the next circle or arc, or Esc to end.");
            }
            DrwTool::Set(panel) => return self.set_click(*panel, at, over, g),
            DrwTool::Centerline { bisector, view, a } => {
                let v = need_view()?;
                if view.is_some_and(|w| w != v) {
                    return Err("click in the same view".into());
                }
                let what = if bisector { "click a line" } else { "click a circle, a line or a line's end" };
                let mut pick = self.drw_exec("drw.pick", json!({ "view": v.0, "at": v2(at) })).map_err(|_| what.to_string())?;
                if bisector && pick["kind"] != "line" {
                    return Err(what.into());
                }
                if !bisector {
                    pick = self.near_a_line_end(v, at, pick)?;
                }
                let Some(first) = a else {
                    next(self, Some(DrwTool::Centerline { bisector, view: Some(v), a: Some(pick) }));
                    return Ok(());
                };
                let id = if bisector { "drw.centerline.bisector" } else { "drw.centerline" };
                self.drw_exec(id, json!({ "view": v.0, "a": first, "b": pick }))?;
                next(self, Some(DrwTool::Centerline { bisector, view: None, a: None }));
                self.set_status(if bisector {
                    "Centreline bisector placed. Click the next line, or Esc to end."
                } else {
                    "Centreline placed. Click the next place, or Esc to end."
                });
            }
            DrwTool::Auxiliary { parent: None, changing, .. } => {
                next(self, Some(DrwTool::Auxiliary { parent: Some(need_view()?), edge: None, changing }))
            }
            DrwTool::Auxiliary { parent: Some(p), edge: None, changing } => {
                let pick = self.drw_exec("drw.pick", json!({ "view": p.0, "at": v2(at) })).map_err(|e| {
                    if e.contains("computed") { STILL_COMPUTING.to_string() } else { "click a straight edge of the view".to_string() }
                })?;
                if pick["kind"] != "line" {
                    return Err("click a straight edge: a circle, an arc or a curve has no one direction to look square to".into());
                }
                if let Some(v) = changing {
                    self.drw_exec("drw.view.edit", json!({ "view": v.0, "edge": pick }))
                        .map_err(|e| if e.contains("computed") { STILL_COMPUTING.to_string() } else { e })?;
                    next(self, None);
                    self.set_status("The auxiliary view looks square to that edge now.");
                } else {
                    next(self, Some(DrwTool::Auxiliary { parent: Some(p), edge: Some(pick), changing: None }));
                }
            }
            DrwTool::Auxiliary { parent: Some(p), edge: Some(edge), .. } => {
                // (A view on top of another shows nothing new, as with Projected.)
                if let Some(under) = d.view_at(g, at).and_then(|v| d.session.drawing().view(v)) {
                    return Err(format!("{} is already there. Click off to the side the view is to be seen from.", under.name));
                }
                let r = self
                    .drw_exec("drw.auxiliary", json!({ "parent": p.0, "edge": edge, "at": v2(at) }))
                    .map_err(|e| if e.contains("computed") { STILL_COMPUTING.to_string() } else { e })?;
                next(self, None);
                self.set_status(format!("View {} placed.", r["name"].as_str().unwrap_or("")));
            }
            DrwTool::Modify { .. } => {}
            DrwTool::Overlay => {
                let v = need_view()?;
                self.open_overlay_panel(v, None)?;
            }
        }
        let _ = g;
        Ok(())
    }

    /// A pick clicked at `at`: near a line's end (within a quarter of its length), that end and not
    /// its middle.
    pub(crate) fn near_a_line_end(&mut self, v: ViewId, at: Vec2, pick: Value) -> Result<Value, String> {
        if pick["kind"] == "line"
            && let Some((s, e)) = line_ends(self, v, &pick)
        {
            let near = s.dist(e) * 0.25;
            let end = if at.dist(s) < near {
                Some("start")
            } else if at.dist(e) < near {
                Some("end")
            } else {
                None
            };
            if let Some(end) = end {
                return self.drw_exec("drw.pick", json!({ "view": v.0, "at": v2(at), "point": end }));
            }
        }
        Ok(pick)
    }

    /// Fits the sheet in the window.
    pub(crate) fn fit_sheet(&mut self) {
        if let Some(d) = self.drw.as_mut() {
            d.fitted = false;
        }
    }

    /// The Drawing View, Edit View, Text and Suggested Dimensions dialogs, and the panel of a set.
    pub(crate) fn drw_windows(&mut self, ui: &Ui) {
        let Some(d) = self.drw.as_mut() else { return };
        let mut action: Option<Result<(), String>> = None;
        let ctx = ui.ctx().clone();
        if let Some(mut dlg) = d.base_dialog.clone() {
            let mut open = true;
            let mut ok = false;
            let mut cancel = false;
            let mut browse = false;
            egui::Window::new("Drawing View")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(360.0)
                .default_pos([320.0, 170.0])
                .show(&ctx, |ui| {
                    egui::Grid::new("tn_base_view").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("File");
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut dlg.file).id(egui::Id::new("tn_base_file")).desired_width(220.0));
                            if ui.button("...").on_hover_text("Choose a part or assembly file").clicked() {
                                browse = true;
                            }
                        });
                        ui.end_row();
                        ui.label("Orientation");
                        egui::ComboBox::from_id_salt("tn_base_orientation").selected_text(dlg.orientation.id()).show_ui(ui, |ui| {
                            for o in Orientation::ALL {
                                ui.selectable_value(&mut dlg.orientation, o, o.id());
                            }
                        });
                        ui.end_row();
                        ui.label("Scale");
                        egui::ComboBox::from_id_salt("tn_base_scale")
                            .selected_text(if dlg.scale.is_empty() { "Fit the sheet" } else { &dlg.scale })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut dlg.scale, String::new(), "Fit the sheet");
                                for s in ["5:1", "2:1", "1:1", "1:2", "1:4", "1:5", "1:10", "1:20"] {
                                    ui.selectable_value(&mut dlg.scale, s.to_owned(), s);
                                }
                            });
                        ui.end_row();
                        ui.label("Style");
                        ui.checkbox(&mut dlg.hidden, "Hidden lines");
                        ui.end_row();
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if button(ui, "OK", "tn_base_ok").clicked() {
                            ok = true;
                        }
                        if button(ui, "Cancel", "tn_base_cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            if browse && let Some(path) = self.services.pick_open.as_ref().and_then(|f| f()) {
                dlg.file = path.to_string_lossy().into_owned();
            }
            let d = self.drw.as_mut();
            if let Some(d) = d {
                d.base_dialog = (open && !ok && !cancel).then_some(dlg.clone());
            }
            if ok {
                let scale = parse_scale(&dlg.scale);
                action = Some(match scale {
                    Err(e) => Err(e),
                    Ok(scale) if dlg.file.trim().is_empty() => {
                        let _ = scale;
                        Err("choose a part or assembly file".into())
                    }
                    Ok(scale) => self.place_base_view(Path::new(dlg.file.trim()), dlg.orientation, scale, dlg.hidden),
                });
            }
        }
        let Some(d) = self.drw.as_mut() else { return };
        if let Some(mut dlg) = d.view_dialog.clone() {
            let mut open = true;
            let mut ok = false;
            let mut cancel = false;
            let mut another_edge = false;
            egui::Window::new("Edit View").open(&mut open).collapsible(false).resizable(false).default_width(300.0).default_pos([320.0, 170.0]).show(
                &ctx,
                |ui| {
                    egui::Grid::new("tn_edit_view").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Scale");
                        ui.add(egui::TextEdit::singleline(&mut dlg.scale).id(egui::Id::new("tn_view_scale")).desired_width(90.0));
                        ui.end_row();
                        ui.label("Style");
                        ui.checkbox(&mut dlg.hidden, "Hidden lines");
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut dlg.centerlines, "Centrelines");
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut dlg.label, "Label");
                        ui.end_row();
                        if let Some(flip) = &mut dlg.flip {
                            ui.label("Edge");
                            check(ui, flip, "Seen from its other side", "tn_view_flip");
                            ui.end_row();
                            ui.label("");
                            if button(ui, "Pick Another Edge", "tn_view_edge").clicked() {
                                another_edge = true;
                            }
                            ui.end_row();
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if button(ui, "OK", "tn_view_ok").clicked() {
                            ok = true;
                        }
                        if button(ui, "Cancel", "tn_view_cancel").clicked() {
                            cancel = true;
                        }
                    });
                },
            );
            d.view_dialog = (open && !ok && !cancel && !another_edge).then_some(dlg.clone());
            if another_edge && let Some(ViewKind::Auxiliary { parent, .. }) = d.session.drawing().view(dlg.view).map(|v| &v.kind) {
                let tool = DrwTool::Auxiliary { parent: Some(*parent), edge: None, changing: Some(dlg.view) };
                let prompt = tool.prompt();
                d.tool = Some(tool);
                self.set_status(prompt);
            }
            if ok {
                action = Some(parse_scale(&dlg.scale).and_then(|scale| {
                    let mut p = json!({ "view": dlg.view.0, "hidden": dlg.hidden, "centerlines": dlg.centerlines, "label": dlg.label });
                    if let Some(s) = scale {
                        p["scale"] = json!(s);
                    }
                    if let Some(f) = dlg.flip {
                        p["flip"] = json!(f);
                    }
                    self.drw_exec("drw.view.edit", p).map(|_| ())
                }));
            }
        }
        let Some(d) = self.drw.as_mut() else { return };
        if let Some(mut dlg) = d.text_dialog.clone() {
            let mut open = true;
            let mut ok = false;
            let mut cancel = false;
            egui::Window::new("Text").open(&mut open).collapsible(false).resizable(false).default_width(340.0).default_pos([320.0, 170.0]).show(
                &ctx,
                |ui| {
                    let r = ui.add(egui::TextEdit::multiline(&mut dlg.text).id(egui::Id::new("tn_drw_text")).desired_rows(3).desired_width(310.0));
                    if dlg.text.is_empty() && !r.has_focus() {
                        r.request_focus();
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if button(ui, "OK", "tn_text_ok").clicked() {
                            ok = true;
                        }
                        if button(ui, "Cancel", "tn_text_cancel").clicked() {
                            cancel = true;
                        }
                    });
                },
            );
            let sheet = d.sheet;
            d.text_dialog = (open && !ok && !cancel).then_some(dlg.clone());
            if ok && !dlg.text.trim().is_empty() {
                action = Some(match dlg.note {
                    Some(a) => self.drw_exec("drw.annotation.edit", json!({ "annotation": a.0, "text": dlg.text })).map(|_| ()),
                    None => self.drw_exec("drw.note", json!({ "text": dlg.text, "at": v2(dlg.at), "sheet": sheet.0 })).map(|_| ()),
                });
            }
        }
        let Some(d) = self.drw.as_mut() else { return };
        if let Some(mut dlg) = d.suggest_dialog.clone() {
            let mut open = true;
            let (mut add, mut cancel) = (false, false);
            egui::Window::new("Suggested Dimensions")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(320.0)
                .default_pos([320.0, 170.0])
                .show(&ctx, |ui| {
                    ui.label("Ticked dimensions are added; their outlines show on the sheet.");
                    ui.add_space(4.0);
                    for (i, (label, on)) in dlg.items.iter_mut().enumerate() {
                        check(ui, on, label, &format!("tn_suggest_{i}"));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let n = dlg.items.iter().filter(|x| x.1).count();
                        if button(ui, &format!("Add {n}"), "tn_suggest_add").clicked() {
                            add = true;
                        }
                        if button(ui, "Cancel", "tn_suggest_cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            let stale = dlg.revision != d.session.revision();
            d.suggest_dialog = (open && !add && !cancel && !stale).then_some(dlg.clone());
            if stale {
                action = Some(Err("the drawing changed: suggest again".into()));
            } else if add {
                let accept: Vec<usize> = dlg.items.iter().enumerate().filter(|(_, x)| x.1).map(|(i, _)| i).collect();
                let r = self.drw_exec("drw.dimension.auto", json!({ "view": dlg.view.0, "accept": accept }));
                action = Some(r.map(|r| self.set_status(format!("{} dimension(s) added.", r["added"]))));
            }
        }
        if let Some(Err(e)) = action {
            self.set_error(e);
        }
        self.drw_view_windows(ui);
        self.set_panel(&ctx);
    }

    /// Right-hand side of the status bar in a drawing.
    pub(crate) fn drw_status(&self) -> Option<String> {
        let d = self.drw.as_ref().filter(|d| d.editing.is_none())?;
        let dr = d.session.drawing();
        let i = dr.sheets.iter().position(|s| s.id == d.sheet).map_or(1, |i| i + 1);
        let size = dr.sheet(d.sheet).map(|s| s.size.name.clone()).unwrap_or_default();
        let views = dr.views.iter().filter(|v| v.sheet == d.sheet).count();
        let busy = if d.jobs.is_empty() { "" } else { "      Computing views..." };
        Some(format!("Sheet {i} of {}   {size}      {views} views{busy}", dr.sheets.len()))
    }

    /// The model browser of a drawing: its sheets and, on each, the views (made-from views under
    /// the view they come from).
    pub(crate) fn drw_browser(&mut self, ui: &mut Ui, t: &Tokens) {
        #[cfg(test)]
        ui.data_mut(|x| x.remove::<Vec<(String, Rect)>>(egui::Id::new("tn_browser_rows")));
        let r = ui.max_rect();
        let header = Rect::from_min_size(r.min, vec2(r.width(), 26.0));
        let p = ui.painter();
        p.rect_filled(header, 0.0, t.panel_header);
        let tab = Rect::from_min_size(header.min, vec2(70.0, header.height()));
        p.rect_filled(tab, 0.0, t.panel);
        p.text(pos2(tab.left() + 8.0, tab.center().y), Align2::LEFT_CENTER, "Model", theme::body(), t.text);
        p.hline(header.x_range(), header.bottom(), Stroke::new(1.0, t.border));
        ui.add_space(header.height() + 2.0);
        let Some(d) = self.drw.as_ref() else { return };
        let dr = d.session.drawing().clone();
        let (active, selected) = (d.sheet, d.selected);
        let errors: BTreeMap<ViewId, String> = d
            .shown_evaluation()
            .map(|e| {
                dr.views.iter().filter_map(|v| e.view(v.id).and_then(|g| tenon_drawing::annotate::view_problem(v, g)).map(|m| (v.id, m))).collect()
            })
            .unwrap_or_default();
        let mut pick_sheet: Option<SheetId> = None;
        let mut pick_view: Option<(SheetId, ViewId)> = None;
        let mut pick_mod: Option<(SheetId, ViewId, u32)> = None;
        let mut suppress: Option<(ViewId, u32, bool, ModWhat)> = None;
        let mut pick_set: Option<(SheetId, tenon_drawing::AnnotId)> = None;
        let mut change_set = None;
        let mut menu: Option<&'static str> = None;
        egui::ScrollArea::vertical().id_salt("tn_drw_browser").auto_shrink([false, false]).show(ui, |ui| {
            browser_row(ui, t, 0, Icon::Drawing, &dr.name, false, false);
            for sh in &dr.sheets {
                let label = if sh.id == active { format!("{} (active)", sh.name) } else { sh.name.clone() };
                let resp = browser_row(ui, t, 1, Icon::NewSheet, &label, false, false);
                if resp.clicked() || resp.double_clicked() {
                    pick_sheet = Some(sh.id);
                }
                // Views in the order made, each under the view it comes from.
                let mut stack: Vec<(ViewId, u8)> =
                    dr.views.iter().rev().filter(|v| v.sheet == sh.id && v.kind.parent().is_none()).map(|v| (v.id, 2)).collect();
                while let Some((id, depth)) = stack.pop() {
                    let Some(v) = dr.view(id) else { continue };
                    let (icon, label) = match &v.kind {
                        ViewKind::Base { .. } => (Icon::BaseView, format!("{}: {}", v.name, tenon_drawing::views::file_name(&v.model))),
                        ViewKind::Projected { .. } => (Icon::ProjectedView, v.name.clone()),
                        ViewKind::Section { .. } => (Icon::SectionView, format!("Section {0}-{0}", v.name)),
                        ViewKind::Detail { .. } => (Icon::DetailView, format!("Detail {}", v.name)),
                        ViewKind::Auxiliary { .. } => (Icon::ProjectedView, format!("Auxiliary {}", v.name)),
                        ViewKind::Overlay { .. } => (Icon::Explode, format!("Overlay {}", v.name)),
                    };
                    let resp = browser_row(ui, t, depth, icon, &label, selected == Some(Owner::View(id)), errors.contains_key(&id));
                    let resp = match errors.get(&id) {
                        Some(m) => resp.on_hover_text(m),
                        None => resp,
                    };
                    if resp.clicked() {
                        pick_view = Some((sh.id, id));
                    }
                    if resp.double_clicked() {
                        pick_view = Some((sh.id, id));
                        menu = Some("drw.edit_view");
                    }
                    resp.context_menu(|ui| {
                        pick_view = Some((sh.id, id));
                        for (label, cmd) in [("Edit View...", "drw.edit_view"), ("Open Model", "drw.edit_model"), ("Delete", "drw.delete")] {
                            if button(ui, label, cmd).clicked() {
                                menu = Some(cmd);
                                ui.close();
                            }
                        }
                    });
                    // What modifies it, each with its own panel.
                    for m in &v.mods {
                        let (icon, what) = match &m.kind {
                            tenon_drawing::ModKind::Crop { .. } => (Icon::Rectangle, ModWhat::Crop),
                            tenon_drawing::ModKind::Break { .. } => (Icon::Split, ModWhat::Break),
                            tenon_drawing::ModKind::BreakOut { .. } => (Icon::Trim, ModWhat::BreakOut),
                        };
                        // Numbered among those of its kind on the view.
                        let nth = v.mods.iter().take_while(|x| x.id != m.id).filter(|x| x.kind.label() == m.kind.label()).count() + 1;
                        let name = if what == ModWhat::Crop { m.kind.label().to_owned() } else { format!("{} {nth}", m.kind.label()) };
                        let label = if m.suppressed { format!("{name} (suppressed)") } else { name };
                        let resp = browser_row(ui, t, depth + 1, icon, &label, selected == Some(Owner::Mod(id, m.id)), false);
                        if resp.clicked() {
                            pick_mod = Some((sh.id, id, m.id));
                        }
                        if resp.double_clicked() {
                            pick_mod = Some((sh.id, id, m.id));
                            menu = Some("drw.edit_modifier");
                        }
                        resp.context_menu(|ui| {
                            pick_mod = Some((sh.id, id, m.id));
                            if button(ui, "Edit...", "tn_mod_menu_edit").clicked() {
                                menu = Some("drw.edit_modifier");
                                ui.close();
                            }
                            if button(ui, if m.suppressed { "Use Again" } else { "Suppress" }, "tn_mod_menu_suppress").clicked() {
                                suppress = Some((id, m.id, !m.suppressed, what));
                                ui.close();
                            }
                            if button(ui, "Delete", "tn_mod_menu_delete").clicked() {
                                menu = Some("drw.delete");
                                ui.close();
                            }
                        });
                    }
                    // Its sets of dimensions, each one record.
                    for a in dr.annotations.iter().filter(|a| a.kind.view() == Some(id)) {
                        let Some((ordinate, label)) = crate::dim_sets::browser_label(a) else { continue };
                        let icon = if ordinate { Icon::Ordinate } else { Icon::Baseline };
                        let problem = crate::dim_sets::has_problem(&dr, d.shown_evaluation(), a);
                        let resp = browser_row(ui, t, depth + 1, icon, &label, selected == Some(Owner::Annotation(a.id)), problem);
                        if resp.clicked() {
                            pick_set = Some((sh.id, a.id));
                        }
                        if resp.double_clicked() {
                            pick_set = Some((sh.id, a.id));
                            change_set = Some(a.id);
                        }
                        resp.context_menu(|ui| {
                            pick_set = Some((sh.id, a.id));
                            if button(ui, "Change Set...", "tn_set_row_edit").clicked() {
                                change_set = Some(a.id);
                                ui.close();
                            }
                            if button(ui, "Delete", "tn_set_row_delete").clicked() {
                                menu = Some("drw.delete");
                                ui.close();
                            }
                        });
                    }
                    stack.extend(dr.views.iter().rev().filter(|c| c.kind.parent() == Some(id)).map(|c| (c.id, depth + 1)));
                }
            }
        });
        if let Some(d) = self.drw.as_mut() {
            if let Some(s) = pick_sheet
                && s != d.sheet
            {
                d.sheet = s;
                d.fitted = false;
                d.selected = None;
            }
            if let Some((s, v)) = pick_view {
                if s != d.sheet {
                    d.sheet = s;
                    d.fitted = false;
                }
                d.selected = Some(Owner::View(v));
            }
            if let Some((s, v, m)) = pick_mod {
                if s != d.sheet {
                    d.sheet = s;
                    d.fitted = false;
                }
                d.selected = Some(Owner::Mod(v, m));
            }
            if let Some((s, a)) = pick_set {
                if s != d.sheet {
                    d.sheet = s;
                    d.fitted = false;
                }
                d.selected = Some(Owner::Annotation(a));
            }
        }
        if let Some((v, m, on, what)) = suppress
            && let Err(e) = self.drw_exec(what.id(), json!({ "view": v.0, "modifier": m, "suppressed": on }))
        {
            self.set_error(e);
        }
        if let Some(id) = menu {
            self.command(id);
        }
        if let Some(a) = change_set
            && let Err(e) = self.edit_set(a)
        {
            self.set_error(e);
        }
    }
}

/// The owners that move with a dragged one: a view takes the views projected from it and the
/// dimensions, balloons, centre marks and centrelines on them.
fn moving_owners(s: &DrwSession, o: Owner) -> Vec<Owner> {
    let Owner::View(v) = o else { return vec![o] };
    let d = s.drawing();
    let views: Vec<ViewId> = d.family(v).into_iter().filter(|f| *f == v || d.view(*f).is_some_and(|x| x.kind.follows_parent())).collect();
    let mut out: Vec<Owner> = views.iter().map(|x| Owner::View(*x)).collect();
    for x in views.iter().filter_map(|x| d.view(*x)) {
        out.extend(x.mods.iter().map(|m| Owner::Mod(x.id, m.id)));
    }
    for a in &d.annotations {
        let on = match &a.kind {
            AnnotKind::Dimension { view, .. }
            | AnnotKind::Balloon { view, .. }
            | AnnotKind::CenterMark { view, .. }
            | AnnotKind::Centerline { view, .. }
            | AnnotKind::CenterlineBisector { view, .. }
            | AnnotKind::Baseline { view, .. }
            | AnnotKind::Ordinate { view, .. } => Some(*view),
            _ => None,
        };
        if on.is_some_and(|x| views.contains(&x)) {
            out.push(Owner::Annotation(a.id));
        }
    }
    out
}

/// What a click that needs the views is told while they are being computed.
const STILL_COMPUTING: &str = "the views are still being computed; try again in a moment";

/// How far a dragged view or set moves: a view that stays lined up with its parent only slides in
/// line with it (as `drw.view.edit` moves it), and a set of dimensions only across what it measures
/// (as `drw.annotation.edit` moves it).
fn in_line(s: &DrwSession, o: Owner, by: Vec2) -> Vec2 {
    if let Owner::Annotation(a) = o
        && let Some(set) = s.drawing().annotation(a).and_then(|x| tenon_drawing::dimsets::parts(&x.kind))
    {
        return set.axis.point(0.0, set.axis.across(by));
    }
    let Owner::View(v) = o else { return by };
    s.drawing().view(v).map_or(by, |x| tenon_drawing::cmd::lined_up(&x.kind, by))
}

/// A picked line's two ends in its view (view coordinates).
fn line_in_view(d: &DrwDoc, v: ViewId, pick: &Value) -> Option<(Vec2, Vec2)> {
    let ev = d.shown_evaluation()?;
    let view = d.session.drawing().view(v)?;
    let geo = ev.view(v)?;
    let m = ev.models.get(&view.model)?;
    let gp: tenon_drawing::GeomPick = serde_json::from_value(pick.clone()).ok()?;
    match tenon_drawing::annotate::resolve_pick(m, &gp).ok()? {
        tenon_drawing::annotate::Resolved::Line(a, b) => {
            Some((tenon_drawing::views::project(&geo.frame, a), tenon_drawing::views::project(&geo.frame, b)))
        }
        _ => None,
    }
}

/// A picked line's two ends on the sheet.
fn line_ends(wb: &Workbench, v: ViewId, pick: &Value) -> Option<(Vec2, Vec2)> {
    let d = wb.drw.as_ref()?;
    let (a, b) = line_in_view(d, v, pick)?;
    let view = d.session.drawing().view(v)?;
    let geo = d.shown_evaluation()?.view(v)?;
    Some((tenon_drawing::annotate::to_sheet(view, geo, a), tenon_drawing::annotate::to_sheet(view, geo, b)))
}

/// "1:2", "2:1", "0.5" or "" (fit): the scale, or None to fit.
fn parse_scale(s: &str) -> Result<Option<f64>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let v = match s.split_once(':') {
        Some((a, b)) => {
            let (a, b): (f64, f64) = (a.trim().parse().map_err(|_| "bad scale")?, b.trim().parse().map_err(|_| "bad scale")?);
            a / b
        }
        None => s.parse().map_err(|_| "the scale is a number or a ratio like 1:2")?,
    };
    if v.is_finite() && (1e-4..=1e4).contains(&v) { Ok(Some(v)) } else { Err("the scale is out of range".into()) }
}

/// In tests, keeps where something to press was drawn under `key`, so tests can press it as a
/// user would.
pub(crate) fn remember(ui: &Ui, key: &str, rect: Rect) {
    #[cfg(test)]
    ui.data_mut(|d| {
        d.get_temp_mut_or_default::<BTreeMap<String, Rect>>(egui::Id::new("tn_buttons")).insert(key.to_owned(), rect);
    });
    #[cfg(not(test))]
    let _ = (ui, key, rect);
}

/// A dialog or menu button; in tests its rectangle is kept under `key` (see [`remember`]).
pub(crate) fn button(ui: &mut Ui, label: &str, key: &str) -> egui::Response {
    let r = ui.button(label);
    remember(ui, key, r.rect);
    r
}

/// A checkbox; in tests its rectangle is kept under `key`, as for [`button`].
pub(crate) fn check(ui: &mut Ui, on: &mut bool, label: &str, key: &str) -> egui::Response {
    let r = ui.checkbox(on, label);
    remember(ui, key, r.rect);
    r
}

/// One row of the drawing browser.
fn browser_row(ui: &mut Ui, t: &Tokens, depth: u8, icon: Icon, label: &str, selected: bool, error: bool) -> egui::Response {
    let (rr, resp) = ui.allocate_exact_size(vec2(ui.available_width(), crate::browser::ROW_H), Sense::CLICK);
    if selected {
        ui.painter().rect_filled(rr, 0.0, t.pressed);
    } else if resp.hovered() {
        ui.painter().rect_filled(rr, 0.0, t.hover);
    }
    let ir = Rect::from_min_size(pos2(rr.left() + 20.0 + f32::from(depth) * 16.0, rr.center().y - 8.0), vec2(16.0, 16.0));
    icons::paint_colored(ui.painter(), ir, icon, t.icon, t, true);
    ui.painter().text(
        pos2(ir.right() + 6.0, rr.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::body(),
        if error { t.history_marker } else { t.text },
    );
    #[cfg(test)]
    ui.data_mut(|d| d.get_temp_mut_or_default::<Vec<(String, Rect)>>(egui::Id::new("tn_browser_rows")).push((label.to_owned(), rr)));
    resp
}

/// Graphics in one colour (previews).
pub(crate) fn paint_plain(p: &egui::Painter, rect: Rect, cam: &SheetCam, g: &Graphics, color: Color32) {
    for (_, pen, pts) in &g.strokes {
        let width = ((pen.width() * cam.px) as f32).max(1.0);
        for run in dashed(pts, pen.dashes()) {
            p.add(Shape::line(run.iter().map(|q| cam.screen(rect, *q)).collect(), Stroke::new(width, color)));
        }
    }
    for (_, pts) in &g.fills {
        p.add(Shape::convex_polygon(pts.iter().map(|q| cam.screen(rect, *q)).collect(), color, Stroke::NONE));
    }
    for (_, t) in &g.texts {
        let width = ((t.height * tenon_drawing::stroke::PEN * cam.px) as f32).max(1.0);
        for s in tenon_drawing::stroke::text_strokes(&t.text, t.left(), t.height) {
            p.add(Shape::line(s.iter().map(|q| cam.screen(rect, *q)).collect(), Stroke::new(width, color)));
        }
    }
}

/// What a tool would place, where the pointer is.
fn draw_tool(p: &egui::Painter, rect: Rect, cam: &SheetCam, d: &DrwDoc, tool: &DrwTool, at: Vec2, ghost: Stroke) {
    let s = |q: Vec2| cam.screen(rect, q);
    let box_at = |c: Vec2, half: Vec2| {
        let r = Rect::from_two_pos(s(c - half), s(c + half));
        p.extend(Shape::dashed_line(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()], ghost, 6.0, 4.0));
    };
    let dr = d.session.drawing();
    let ev = d.shown_evaluation();
    let model_size = |v: ViewId, f: &tenon_geom::Frame, scale: f64| -> Option<Vec2> {
        let m = ev?.models.get(&dr.view(v)?.model)?;
        seen_size(m, f).map(|z| z * (0.5 * scale))
    };
    match tool {
        DrwTool::Projected { parent: Some(pv) } => {
            let (Some(v), Some(pf)) = (dr.view(*pv), ev.and_then(|e| e.view(*pv)).map(|g| g.frame)) else { return };
            let (side, place) = projected_place(v.center, at);
            let f = projected_frame(&pf, side, dr.standard == Standard::Ansi);
            if let Some(half) = model_size(*pv, &f, v.scale) {
                box_at(place, half);
            }
            p.extend(Shape::dashed_line(&[s(v.center), s(place)], Stroke::new(1.0, GHOST.gamma_multiply(0.6)), 4.0, 4.0));
        }
        DrwTool::Section { line, .. } if !line.is_empty() => {
            let end = line.get(1).copied().unwrap_or(at);
            p.line_segment([s(line[0]), s(end)], Stroke::new(2.0, GHOST));
            if line.len() == 2 {
                p.circle_stroke(s(at), 5.0, ghost);
            }
        }
        // Where the view would go, and the edge it looks square to.
        DrwTool::Auxiliary { parent: Some(pv), edge: Some(edge), .. } => {
            let (Some(v), Some(g), Some((a, b))) = (dr.view(*pv), ev.and_then(|e| e.view(*pv)), line_in_view(d, *pv, edge)) else { return };
            let third = dr.standard == Standard::Ansi;
            let to_sheet = |q: Vec2| tenon_drawing::annotate::to_sheet(v, g, q);
            let flip = tenon_drawing::cmd::auxiliary_flip(third, a, b, to_sheet((a + b) * 0.5), at);
            let toward = tenon_drawing::cmd::auxiliary_toward(third, a, b, flip);
            let place = v.center + toward * (at - v.center).dot(toward);
            let (f, _, _) = tenon_drawing::views::section_frame(&g.frame, a, b, flip);
            if let Some(half) = model_size(*pv, &f, v.scale) {
                // Its middle is lined up with its parent's along the edge: what it draws lies
                // off that by as far as the two boxes' middles are apart along it.
                let u = (b - a).normalized();
                let apart = ev
                    .and_then(|e| e.models.get(&v.model))
                    .and_then(|m| tenon_drawing::views::seen_middle(m, &f))
                    .map_or(0.0, |c| (g.center - c).dot(u));
                box_at(place - u * (apart * v.scale), half);
            }
            p.line_segment([s(to_sheet(a)), s(to_sheet(b))], Stroke::new(2.0, GHOST));
            p.extend(Shape::dashed_line(&[s(v.center), s(place)], Stroke::new(1.0, GHOST.gamma_multiply(0.6)), 4.0, 4.0));
        }
        DrwTool::Detail { center: Some(c), radius, .. } => {
            let r = radius.unwrap_or_else(|| c.dist(at));
            p.circle_stroke(s(*c), (r * cam.px) as f32, ghost);
            if let Some(r) = radius {
                p.circle_stroke(s(at), (2.0 * r * cam.px) as f32, Stroke::new(1.0, GHOST.gamma_multiply(0.6)));
            }
        }
        DrwTool::Dimension { picks, .. } => {
            for pk in picks {
                if let Some(q) = pick_at(pk) {
                    p.circle_filled(s(q), 4.0, GHOST);
                }
            }
            if !picks.is_empty() {
                p.circle_stroke(s(at), 4.0, ghost);
            }
        }
        DrwTool::Balloon { attach: Some(q), .. } => {
            p.line_segment([s(*q), s(at)], ghost);
            p.circle_stroke(s(at), (4.0 * cam.px) as f32, ghost);
        }
        DrwTool::Centerline { bisector, a: Some(pk), .. } => {
            if let Some(q) = pick_at(pk) {
                p.circle_filled(s(q), 4.0, GHOST);
                if !bisector {
                    p.extend(Shape::dashed_line(&[s(q), s(at)], ghost, 10.0, 4.0));
                }
            }
        }
        _ => {}
    }
}

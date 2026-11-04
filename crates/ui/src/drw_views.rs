//! The panels of a drawing's view modifiers (Crop, Break, Break Out) and of Overlay: what they need
//! is clicked on the sheet or typed, and shows there as it would be before OK puts it in through
//! the `drw.*` commands. The tools that fill them are in `drawing.rs`, with the others.

use std::collections::BTreeMap;

use egui::{Pos2, Rect, Shape, Stroke, Ui};
use serde_json::{Value, json};
use tenon_drawing::annotate::{format_number, from_sheet, to_sheet};
use tenon_drawing::modify::outline;
use tenon_drawing::{Boundary, BreakLines, ModKind, Owner, ViewId, ViewKind};
use tenon_geom::Vec2;

use crate::drawing::{DrwDoc, DrwTool, GHOST, SheetCam, button, check, remember};
use crate::workbench::Workbench;

/// Which modifier a panel makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModWhat {
    Crop,
    Break,
    BreakOut,
}

impl ModWhat {
    pub(crate) fn id(self) -> &'static str {
        match self {
            ModWhat::Crop => "drw.crop",
            ModWhat::Break => "drw.break",
            ModWhat::BreakOut => "drw.break_out",
        }
    }
    fn title(self) -> &'static str {
        match self {
            ModWhat::Crop => "Crop",
            ModWhat::Break => "Break",
            ModWhat::BreakOut => "Break Out",
        }
    }
    /// One of its kind, to ask a view whether it may have it.
    fn probe(self) -> ModKind {
        let boundary = Boundary::rect(Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0));
        match self {
            ModWhat::Crop => ModKind::Crop { boundary, outline: true },
            ModWhat::Break => ModKind::Break { lines: BreakLines::Vertical, from: 0.0, to: 1.0, gap: 0.0 },
            ModWhat::BreakOut => ModKind::BreakOut { boundary, depth: Some(1.0) },
        }
    }
}

/// What the clicks on a view give a modifier's panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModPick {
    Rect,
    Circle,
    Lines,
}

/// The panel of a crop, break or break out being made or changed. Its numbers are as typed; what
/// the sheet is clicked for fills them in (view coordinates, model mm).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ModPanel {
    pub what: ModWhat,
    pub view: Option<ViewId>,
    /// The modifier being changed, if it is not a new one.
    pub id: Option<u32>,
    pub circle: bool,
    pub corners: [String; 4],
    pub center: [String; 2],
    pub radius: String,
    pub outline: bool,
    pub vertical: bool,
    pub from: String,
    pub to: String,
    pub gap: String,
    pub depth: String,
    pub through: bool,
    pub suppressed: bool,
}

fn n(v: f64) -> String {
    format_number(v, 3)
}

fn number(s: &str) -> Option<f64> {
    s.trim().parse().ok().filter(|v: &f64| v.is_finite())
}

impl ModPanel {
    fn new(what: ModWhat, view: Option<ViewId>) -> ModPanel {
        ModPanel {
            what,
            view,
            id: None,
            circle: false,
            corners: Default::default(),
            center: Default::default(),
            radius: String::new(),
            outline: true,
            vertical: true,
            from: String::new(),
            to: String::new(),
            gap: n(tenon_drawing::cmd::BREAK_GAP),
            depth: "5".into(),
            through: false,
            suppressed: false,
        }
    }

    /// What a modifier there is says.
    fn of(view: ViewId, id: u32, suppressed: bool, kind: &ModKind) -> ModPanel {
        let what = match kind {
            ModKind::Crop { .. } => ModWhat::Crop,
            ModKind::Break { .. } => ModWhat::Break,
            ModKind::BreakOut { .. } => ModWhat::BreakOut,
        };
        let mut p = ModPanel { id: Some(id), suppressed, ..ModPanel::new(what, Some(view)) };
        match kind {
            ModKind::Crop { boundary, outline } => {
                p.outline = *outline;
                p.set_boundary(boundary);
            }
            ModKind::Break { lines, from, to, gap } => {
                p.vertical = *lines == BreakLines::Vertical;
                (p.from, p.to, p.gap) = (n(*from), n(*to), n(*gap));
            }
            ModKind::BreakOut { boundary, depth } => {
                p.set_boundary(boundary);
                p.through = depth.is_none();
                if let Some(d) = depth {
                    p.depth = n(*d);
                }
            }
        }
        p
    }

    fn set_boundary(&mut self, b: &Boundary) {
        match b {
            Boundary::Rect { lo, hi } => {
                self.circle = false;
                self.corners = [n(lo.x), n(lo.y), n(hi.x), n(hi.y)];
            }
            Boundary::Circle { center, radius } => {
                self.circle = true;
                self.center = [n(center.x), n(center.y)];
                self.radius = n(*radius);
            }
        }
    }

    pub(crate) fn pick(&self) -> ModPick {
        match (self.what, self.circle) {
            (ModWhat::Break, _) => ModPick::Lines,
            (_, true) => ModPick::Circle,
            (_, false) => ModPick::Rect,
        }
    }

    /// The boundary as typed so far, if it makes one.
    fn boundary(&self) -> Option<Boundary> {
        if self.circle {
            let (x, y, r) = (number(&self.center[0])?, number(&self.center[1])?, number(&self.radius)?);
            return (r > 0.0).then_some(Boundary::Circle { center: Vec2::new(x, y), radius: r });
        }
        let [x0, y0, x1, y1] = [number(&self.corners[0])?, number(&self.corners[1])?, number(&self.corners[2])?, number(&self.corners[3])?];
        Some(Boundary::rect(Vec2::new(x0, y0), Vec2::new(x1, y1)))
    }

    /// The command that puts it in, or what is still wanted.
    fn command(&self) -> Result<Value, String> {
        let view = self.view.ok_or("click the view first")?;
        let mut p = json!({ "view": view.0, "suppressed": self.suppressed });
        if let Some(id) = self.id {
            p["modifier"] = json!(id);
        }
        let boundary = |p: &mut Value| -> Result<(), String> {
            match self.boundary() {
                Some(Boundary::Rect { lo, hi }) => p["rect"] = json!([[lo.x, lo.y], [hi.x, hi.y]]),
                Some(Boundary::Circle { center, radius }) => {
                    p["center"] = json!([center.x, center.y]);
                    p["radius"] = json!(radius);
                }
                None if self.circle => return Err("click the circle on the view, or type its middle and radius".into()),
                None => return Err("click the rectangle's two corners on the view, or type them".into()),
            }
            Ok(())
        };
        match self.what {
            ModWhat::Crop => {
                boundary(&mut p)?;
                p["outline"] = json!(self.outline);
            }
            ModWhat::Break => {
                let (from, to) = (number(&self.from), number(&self.to));
                let (Some(from), Some(to)) = (from, to) else {
                    return Err("click where the two break lines go on the view, or type them".into());
                };
                let gap = number(&self.gap).ok_or("the gap is a number: mm of paper")?;
                p["lines"] = json!(if self.vertical { "vertical" } else { "horizontal" });
                p["from"] = json!(from);
                p["to"] = json!(to);
                p["gap"] = json!(gap);
            }
            ModWhat::BreakOut => {
                boundary(&mut p)?;
                if self.through {
                    p["through"] = json!(true);
                } else {
                    p["depth"] = json!(number(&self.depth).ok_or("the depth is a number: mm of the model, from what is nearest the eye")?);
                }
            }
        }
        Ok(p)
    }
}

/// One relationship's row in the Overlay panel: blank is as the assembly has it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OverlayRow {
    pub relationship: u32,
    pub name: String,
    /// What it is now (mm, degrees), if a position can set it.
    pub offset_now: Option<f64>,
    pub angle_now: Option<f64>,
    pub offset: String,
    pub angle: String,
}

/// The Overlay panel: the position, with what it comes to shown on the sheet.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OverlayPanel {
    pub parent: ViewId,
    /// The overlay being changed, if it is not a new one.
    pub view: Option<ViewId>,
    pub rows: Vec<OverlayRow>,
    /// What is wrong with the position, in words.
    pub problem: Option<String>,
    /// Where the components that move go, as lines on the sheet.
    pub preview: Vec<Vec<Vec2>>,
    /// What the problem and the preview were worked out for.
    worked_out: Option<Value>,
}

impl OverlayPanel {
    /// The values typed, as a command takes them.
    fn values(&self) -> Result<Value, String> {
        let mut out = Vec::new();
        for r in &self.rows {
            let read = |s: &str, what: &str| -> Result<Option<f64>, String> {
                match s.trim() {
                    "" => Ok(None),
                    t => t.parse::<f64>().ok().filter(|v| v.is_finite()).map(Some).ok_or_else(|| format!("the {what} of {} is not a number", r.name)),
                }
            };
            let (offset, angle) = (read(&r.offset, "offset")?, read(&r.angle, "angle")?.map(f64::to_radians));
            if offset.is_some() || angle.is_some() {
                out.push(json!({ "relationship": r.relationship, "offset": offset, "angle": angle }));
            }
        }
        Ok(Value::Array(out))
    }
}

/// The values as the assembly crate has them.
fn joint_values(values: &Value) -> Vec<tenon_assembly::position::JointValue> {
    values.as_array().map_or_else(Vec::new, |a| a.iter().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect())
}

impl Workbench {
    /// Crop, Break or Break Out from the ribbon: its panel, and the tool that fills it from clicks.
    pub(crate) fn open_mod_panel(&mut self, what: ModWhat, view: Option<ViewId>) -> Result<(), String> {
        let d = self.drw.as_mut().ok_or("no drawing is open")?;
        if let Some(v) = view.and_then(|v| d.session.drawing().view(v)) {
            v.may_have(&what.probe())?;
        }
        // A view has one crop: Crop on a cropped view changes it.
        let existing = view.and_then(|v| d.session.drawing().view(v)).and_then(|v| {
            (what == ModWhat::Crop).then(|| v.mods.iter().find(|m| matches!(m.kind, ModKind::Crop { .. }))).flatten().map(|m| (v.id, m.clone()))
        });
        let panel = match existing {
            Some((v, m)) => ModPanel::of(v, m.id, m.suppressed, &m.kind),
            None => ModPanel::new(what, view),
        };
        let tool = DrwTool::Modify { what, pick: panel.pick(), view, first: None };
        let prompt = tool.prompt();
        d.mod_panel = Some(panel);
        d.overlay_panel = None;
        d.tool = Some(tool);
        self.set_status(prompt);
        Ok(())
    }

    /// Opens the panel of a modifier that is there.
    pub(crate) fn edit_modifier(&mut self, view: ViewId, id: u32) -> Result<(), String> {
        let d = self.drw.as_mut().ok_or("no drawing is open")?;
        let v = d.session.drawing().view(view).ok_or("the view is gone")?;
        let m = v.mods.iter().find(|m| m.id == id).ok_or("the modifier is gone")?;
        let panel = ModPanel::of(view, id, m.suppressed, &m.kind);
        let tool = DrwTool::Modify { what: panel.what, pick: panel.pick(), view: Some(view), first: None };
        let prompt = tool.prompt();
        d.mod_panel = Some(panel);
        d.overlay_panel = None;
        d.tool = Some(tool);
        self.set_status(prompt);
        Ok(())
    }

    /// A click on the sheet with a modifier's panel open: the view, then what the panel wants.
    pub(crate) fn modifier_click(&mut self, at: Vec2, over: Option<ViewId>) -> Result<(), String> {
        let d = self.drw.as_mut().ok_or("no drawing")?;
        let Some(DrwTool::Modify { what, pick, view, first }) = d.tool.clone() else { return Ok(()) };
        let Some(v) = view else {
            let v = over.ok_or("click inside a view")?;
            let target = d.session.drawing().view(v).ok_or("the view is gone")?;
            target.may_have(&what.probe())?;
            // A view with a crop: its crop, to change.
            if what == ModWhat::Crop && target.mods.iter().any(|m| matches!(m.kind, ModKind::Crop { .. })) {
                return self.open_mod_panel(what, Some(v));
            }
            if let Some(panel) = d.mod_panel.as_mut() {
                panel.view = Some(v);
            }
            let tool = DrwTool::Modify { what, pick, view: Some(v), first: None };
            let prompt = tool.prompt();
            d.tool = Some(tool);
            self.set_status(prompt);
            return Ok(());
        };
        let dr = d.session.drawing();
        let target = dr.view(v).ok_or("the view is gone")?;
        let geo = d.session.evaluation().or(d.session.last_evaluation()).and_then(|e| e.view(v)).ok_or("the view is not computed yet")?;
        let q = from_sheet(target, geo, at);
        let mut first = first;
        match (pick, first) {
            (_, None) => first = Some(q),
            (_, Some(a)) => {
                let panel = d.mod_panel.as_mut().ok_or("the panel is closed")?;
                match pick {
                    ModPick::Rect => {
                        if (a.x - q.x).abs() < 1e-3 || (a.y - q.y).abs() < 1e-3 {
                            return Err("click the opposite corner somewhere else".into());
                        }
                        panel.set_boundary(&Boundary::rect(a, q));
                    }
                    ModPick::Circle => {
                        if a.dist(q) < 1e-3 {
                            return Err("click farther from the middle".into());
                        }
                        panel.set_boundary(&Boundary::Circle { center: a, radius: a.dist(q) });
                    }
                    ModPick::Lines => {
                        let (s, t) = if panel.vertical { (a.x, q.x) } else { (a.y, q.y) };
                        if (s - t).abs() < 1e-3 {
                            return Err("click the second line somewhere else".into());
                        }
                        (panel.from, panel.to) = (n(s.min(t)), n(s.max(t)));
                    }
                }
                first = None;
            }
        }
        let tool = DrwTool::Modify { what, pick, view, first };
        let prompt = if first.is_none() && d.mod_panel.as_ref().is_some_and(|p| p.command().is_ok()) {
            "Press OK to put it on the view, or click again to place it again."
        } else {
            tool.prompt()
        };
        d.tool = Some(tool);
        self.set_status(prompt);
        Ok(())
    }

    /// Opens the Overlay panel over a view of an assembly (`overlay`: to change one that is there).
    pub(crate) fn open_overlay_panel(&mut self, parent: ViewId, overlay: Option<ViewId>) -> Result<(), String> {
        let d = self.drw.as_mut().ok_or("no drawing is open")?;
        let dr = d.session.drawing();
        let pv = dr.view(parent).ok_or("the view is gone")?;
        if !matches!(pv.kind, ViewKind::Base { .. } | ViewKind::Projected { .. } | ViewKind::Auxiliary { .. }) {
            return Err("an overlay goes over a base, projected or auxiliary view: click one of those".into());
        }
        let ev = d.session.evaluation().ok_or("the views are still being computed; try again in a moment")?;
        let m = ev.models.get(&pv.model).ok_or("the model is not loaded")?;
        let asm = m.assembly.as_ref().ok_or("an overlay shows an assembly in another position: click a view of an assembly")?;
        let readings = tenon_assembly::position::readings(asm, &m.parts());
        if readings.is_empty() {
            return Err(
                "nothing holds this assembly's components: a position sets the values of its joints and constraints, and it has none to set".into()
            );
        }
        let was: Vec<tenon_assembly::position::JointValue> = match overlay.and_then(|o| dr.view(o)).map(|v| &v.kind) {
            Some(ViewKind::Overlay { values, .. }) => values.clone(),
            _ => Vec::new(),
        };
        let rows = readings
            .into_iter()
            .map(|r| {
                let given = was.iter().find(|v| v.relationship == r.relationship);
                OverlayRow {
                    relationship: r.relationship.0,
                    name: r.name,
                    offset_now: r.offset,
                    angle_now: r.angle.map(f64::to_degrees),
                    offset: given.and_then(|v| v.offset).map(n).unwrap_or_default(),
                    angle: given.and_then(|v| v.angle).map(|a| n(a.to_degrees())).unwrap_or_default(),
                }
            })
            .collect();
        d.overlay_panel = Some(OverlayPanel { parent, view: overlay, rows, problem: None, preview: Vec::new(), worked_out: None });
        d.mod_panel = None;
        d.tool = None;
        self.set_status("Overlay: type a position: the values of the joints and offsets that change. It shows on the sheet as you type.");
        Ok(())
    }

    /// Says what computing a view that has just been modified found, once it is computed.
    fn report_modified(&mut self) {
        let Some(d) = self.drw.as_mut() else { return };
        let Some(id) = d.report else { return };
        let Some(ev) = d.session.evaluation() else { return };
        d.report = None;
        let dr = d.session.drawing();
        let said = dr.view(id).zip(ev.view(id)).and_then(|(v, g)| tenon_drawing::annotate::view_problem(v, g).map(|e| format!("{}: {e}", v.name)));
        if let Some(e) = said {
            self.set_error(e);
        }
    }

    /// Closes a panel whose work is over: a modifier's panel is filled by its tool, so another tool
    /// taking over ends it, and what either fills in is on its view's sheet.
    fn close_stale_panels(&mut self) {
        let Some(d) = self.drw.as_mut() else { return };
        let dr = d.session.drawing();
        let elsewhere = |v: Option<ViewId>| v.and_then(|v| dr.view(v)).is_some_and(|v| v.sheet != d.sheet);
        let mod_stale = d.mod_panel.as_ref().is_some_and(|p| elsewhere(p.view) || !matches!(d.tool, Some(DrwTool::Modify { .. })));
        let overlay_stale = d.overlay_panel.as_ref().is_some_and(|p| elsewhere(Some(p.parent)));
        let off_sheet = d.mod_panel.as_ref().is_some_and(|p| elsewhere(p.view)) || overlay_stale;
        if mod_stale {
            d.mod_panel = None;
            if matches!(d.tool, Some(DrwTool::Modify { .. })) {
                d.tool = None;
            }
        }
        if overlay_stale {
            d.overlay_panel = None;
        }
        if off_sheet {
            self.set_status("The panel is closed: its view is on another sheet.");
        }
    }

    /// The panels of this file.
    pub(crate) fn drw_view_windows(&mut self, ui: &Ui) {
        self.report_modified();
        self.close_stale_panels();
        let at = [self.view.rect.right() - 350.0, self.view.rect.top() + 36.0];
        self.mod_window(ui, at);
        self.overlay_window(ui, at);
    }

    fn mod_window(&mut self, ui: &Ui, at: [f32; 2]) {
        let Some(d) = self.drw.as_mut() else { return };
        let Some(mut panel) = d.mod_panel.clone() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let before = panel.pick();
        let text = |ui: &mut Ui, value: &mut String, key: &str| {
            ui.add(egui::TextEdit::singleline(value).id(egui::Id::new(key)).desired_width(60.0));
        };
        egui::Window::new(panel.what.title()).open(&mut open).collapsible(false).resizable(false).default_width(270.0).default_pos(at).show(
            ui.ctx(),
            |ui| {
                egui::Grid::new("tn_mod_panel").num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
                    if panel.what == ModWhat::Break {
                        ui.label("Lines");
                        let r = ui.radio_value(&mut panel.vertical, true, "Up the sheet");
                        remember(ui, "tn_mod_vertical", r.rect);
                        let r = ui.radio_value(&mut panel.vertical, false, "Across");
                        remember(ui, "tn_mod_horizontal", r.rect);
                        ui.end_row();
                        ui.label("Between");
                        text(ui, &mut panel.from, "tn_mod_from");
                        text(ui, &mut panel.to, "tn_mod_to");
                        ui.end_row();
                        ui.label("Gap (paper)");
                        text(ui, &mut panel.gap, "tn_mod_gap");
                        ui.end_row();
                    } else {
                        ui.label("Shape");
                        let r = ui.radio_value(&mut panel.circle, false, "Rectangle");
                        remember(ui, "tn_mod_rect", r.rect);
                        let r = ui.radio_value(&mut panel.circle, true, "Circle");
                        remember(ui, "tn_mod_circle", r.rect);
                        ui.end_row();
                        if panel.circle {
                            ui.label("Middle");
                            text(ui, &mut panel.center[0], "tn_mod_cx");
                            text(ui, &mut panel.center[1], "tn_mod_cy");
                            ui.end_row();
                            ui.label("Radius");
                            text(ui, &mut panel.radius, "tn_mod_radius");
                            ui.end_row();
                        } else {
                            ui.label("Corner");
                            text(ui, &mut panel.corners[0], "tn_mod_x0");
                            text(ui, &mut panel.corners[1], "tn_mod_y0");
                            ui.end_row();
                            ui.label("Opposite");
                            text(ui, &mut panel.corners[2], "tn_mod_x1");
                            text(ui, &mut panel.corners[3], "tn_mod_y1");
                            ui.end_row();
                        }
                    }
                    if panel.what == ModWhat::BreakOut {
                        ui.label("Depth");
                        ui.add_enabled_ui(!panel.through, |ui| text(ui, &mut panel.depth, "tn_mod_depth"));
                        check(ui, &mut panel.through, "Through", "tn_mod_through");
                        ui.end_row();
                    }
                    if panel.what == ModWhat::Crop {
                        ui.label("");
                        check(ui, &mut panel.outline, "Show its outline", "tn_mod_outline");
                        ui.end_row();
                    }
                    if panel.id.is_some() {
                        ui.label("");
                        check(ui, &mut panel.suppressed, "Suppressed", "tn_mod_suppressed");
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if button(ui, "OK", "tn_mod_ok").clicked() {
                        ok = true;
                    }
                    if button(ui, "Cancel", "tn_mod_cancel").clicked() {
                        cancel = true;
                    }
                });
            },
        );
        // The shape or the way of the lines changed: what clicks give changes too.
        let mut reset = None;
        if panel.pick() != before
            && let Some(DrwTool::Modify { what, view, .. }) = d.tool.clone()
        {
            let t = DrwTool::Modify { what, pick: panel.pick(), view, first: None };
            reset = Some(t.prompt());
            d.tool = Some(t);
        }
        let mut result = Ok(());
        if ok {
            let what = panel.what.title();
            match panel.command() {
                Ok(p) => {
                    let id = panel.what.id();
                    d.mod_panel = Some(panel.clone());
                    match self.drw_exec(id, p) {
                        Ok(_) => {
                            if let Some(d) = self.drw.as_mut() {
                                d.report = d.mod_panel.as_ref().and_then(|p| p.view);
                                d.mod_panel = None;
                                d.tool = None;
                                d.selected = None;
                            }
                            self.set_status(format!("{what} done."));
                            return;
                        }
                        Err(e) => result = Err(e),
                    }
                }
                Err(e) => result = Err(e),
            }
        }
        let Some(d) = self.drw.as_mut() else { return };
        if !open || cancel {
            d.mod_panel = None;
            d.tool = None;
            self.set_status("Ready");
            return;
        }
        d.mod_panel = Some(panel);
        if let Some(p) = reset {
            self.set_status(p);
        }
        if let Err(e) = result {
            self.set_error(e);
        }
    }

    fn overlay_window(&mut self, ui: &Ui, at: [f32; 2]) {
        let Some(d) = self.drw.as_mut() else { return };
        let Some(mut panel) = d.overlay_panel.clone() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        egui::Window::new("Overlay").open(&mut open).collapsible(false).resizable(false).default_width(330.0).default_pos(at).show(ui.ctx(), |ui| {
            ui.label("The values that change; blank is as the assembly has it.");
            ui.add_space(4.0);
            egui::Grid::new("tn_overlay_panel").num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("");
                ui.label("Offset (mm)");
                ui.label("Angle (deg)");
                ui.end_row();
                for (i, row) in panel.rows.iter_mut().enumerate() {
                    ui.label(row.name.as_str());
                    let field = |ui: &mut Ui, value: &mut String, now: Option<f64>, key: String| {
                        ui.add_enabled_ui(now.is_some(), |ui| {
                            let hint = now.map(n).unwrap_or_default();
                            ui.add(egui::TextEdit::singleline(value).id(egui::Id::new(key)).desired_width(70.0).hint_text(hint));
                        });
                    };
                    field(ui, &mut row.offset, row.offset_now, format!("tn_overlay_offset_{i}"));
                    field(ui, &mut row.angle, row.angle_now, format!("tn_overlay_angle_{i}"));
                    ui.end_row();
                }
            });
            if let Some(p) = &panel.problem {
                ui.add_space(4.0);
                ui.colored_label(egui::Color32::from_rgb(0xd8, 0x44, 0x38), p.as_str());
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if button(ui, "OK", "tn_overlay_ok").clicked() {
                    ok = true;
                }
                if button(ui, "Cancel", "tn_overlay_cancel").clicked() {
                    cancel = true;
                }
            });
        });
        let values = panel.values();
        // What it comes to, when what is typed has changed.
        let now = values.clone().unwrap_or(Value::Null);
        if panel.worked_out.as_ref() != Some(&now) {
            work_out(d, &mut panel, &values);
            panel.worked_out = Some(now);
        }
        if !open || cancel {
            d.overlay_panel = None;
            self.set_status("Ready");
            return;
        }
        d.overlay_panel = Some(panel.clone());
        if ok {
            let run = values.and_then(|v| {
                if v.as_array().is_none_or(Vec::is_empty) {
                    return Err("type at least one value: a position is what it changes".to_string());
                }
                let p = match panel.view {
                    Some(o) => json!({ "view": o.0, "values": v }),
                    None => json!({ "parent": panel.parent.0, "values": v }),
                };
                self.drw_exec("drw.overlay", p)
            });
            match run {
                Ok(_) => {
                    if let Some(d) = self.drw.as_mut() {
                        d.overlay_panel = None;
                    }
                    self.set_status("Overlay done: the assembly in that position is drawn over the view in phantom lines.");
                }
                Err(e) => self.set_error(e),
            }
        }
    }
}

/// Asks the solver where a panel's position goes, and draws the components that move there as lines
/// on the sheet. The solver needs no kernel: what it says shows while typing.
fn work_out(d: &DrwDoc, panel: &mut OverlayPanel, values: &Result<Value, String>) {
    panel.preview.clear();
    panel.problem = None;
    let values = match values {
        Ok(v) => joint_values(v),
        Err(e) => {
            panel.problem = Some(e.clone());
            return;
        }
    };
    if values.is_empty() {
        return;
    }
    let (Some(ev), Some(pv)) = (d.session.evaluation(), d.session.drawing().view(panel.parent)) else { return };
    let (Some(m), Some(g)) = (ev.models.get(&pv.model), ev.view(panel.parent)) else { return };
    let frames: BTreeMap<_, _> = match m.position_frames(&values) {
        Ok(f) => f,
        Err(e) => {
            panel.problem = Some(e);
            return;
        }
    };
    for inst in &m.instances {
        let (Some(c), Some(scene)) = (inst.component, m.scenes.get(&inst.part)) else { continue };
        let Some(to) = frames.get(&c) else { continue };
        let same = to.origin().near(inst.frame.origin(), 1e-9) && to.x().near(inst.frame.x(), 1e-9) && to.y().near(inst.frame.y(), 1e-9);
        if same {
            continue;
        }
        for body in &scene.bodies {
            for poly in &body.mesh.edges {
                let line = poly.points.iter().map(|p| {
                    let world = to.to_world(tenon_geom::Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])));
                    to_sheet(pv, g, tenon_drawing::views::project(&g.frame, world))
                });
                panel.preview.push(line.collect());
            }
        }
    }
}

/// What the open panel would make, on the sheet.
pub(crate) fn draw_panels(p: &egui::Painter, rect: Rect, cam: &SheetCam, d: &DrwDoc, pointer: Option<Vec2>, ghost: Stroke) {
    let s = |q: Vec2| cam.screen(rect, q);
    if let Some(panel) = &d.overlay_panel {
        for line in &panel.preview {
            let pts: Vec<Pos2> = line.iter().map(|q| s(*q)).collect();
            p.add(Shape::line(pts, ghost));
        }
    }
    let Some(panel) = &d.mod_panel else { return };
    let ev = d.session.evaluation().or(d.session.last_evaluation());
    let Some((v, g)) = panel.view.and_then(|id| d.session.drawing().view(id).zip(ev.and_then(|e| e.view(id)))) else { return };
    let on = |q: Vec2| s(to_sheet(v, g, q));
    let shape = |b: &Boundary| -> Vec<Pos2> { outline(b).into_iter().map(on).collect() };
    let band_lines = |vertical: bool, t: f64, stroke: Stroke| {
        let (lo, hi) = g.bounds.unwrap_or((g.center, g.center));
        let (a, b) = if vertical { (Vec2::new(t, lo.y), Vec2::new(t, hi.y)) } else { (Vec2::new(lo.x, t), Vec2::new(hi.x, t)) };
        p.extend(Shape::dashed_line(&[on(a), on(b)], stroke, 8.0, 4.0));
    };
    // What the panel says now.
    if panel.what == ModWhat::Break {
        for t in [number(&panel.from), number(&panel.to)].into_iter().flatten() {
            band_lines(panel.vertical, t, ghost);
        }
    } else if let Some(b) = panel.boundary() {
        p.extend(Shape::dashed_line(&shape(&b), ghost, 8.0, 4.0));
    }
    // What the next click would make of it.
    let Some(DrwTool::Modify { pick, first: Some(a), .. }) = &d.tool else { return };
    let Some(q) = pointer.map(|q| from_sheet(v, g, q)) else { return };
    match pick {
        ModPick::Rect => {
            p.extend(Shape::dashed_line(&shape(&Boundary::rect(*a, q)), ghost, 8.0, 4.0));
        }
        ModPick::Circle => {
            p.extend(Shape::dashed_line(&shape(&Boundary::Circle { center: *a, radius: a.dist(q).max(1e-6) }), ghost, 8.0, 4.0));
        }
        ModPick::Lines => {
            band_lines(panel.vertical, if panel.vertical { a.x } else { a.y }, ghost);
            band_lines(panel.vertical, if panel.vertical { q.x } else { q.y }, Stroke::new(1.0, GHOST.gamma_multiply(0.6)));
        }
    }
}

/// The view a click on what a view owns stands for.
pub(crate) fn source_view(d: &DrwDoc, o: Option<Owner>) -> Option<ViewId> {
    match o? {
        Owner::Mod(v, _) => Some(v),
        Owner::View(v) => match d.session.drawing().view(v).map(|x| &x.kind) {
            Some(ViewKind::Overlay { parent, .. }) => Some(*parent),
            _ => Some(v),
        },
        _ => None,
    }
}

//! The drawing document: sheets, views of a part or assembly, and annotations. Saved as
//! `.tenondrw` (DEC-026). Lengths on the sheet are millimetres of paper; lengths in a view are
//! millimetres of the model.

use serde::{Deserialize, Serialize};
use tenon_assembly::ComponentId;
use tenon_geom::{Vec2, Vec3, tol};
use tenon_model::EdgeRef;

/// Most sheets, views and annotations one drawing may have.
pub const MAX_SHEETS: usize = 500;
pub const MAX_VIEWS: usize = 5_000;
pub const MAX_ANNOTATIONS: usize = 50_000;

macro_rules! id_type {
    ($name:ident, $what:literal) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u32);
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!($what, " {}"), self.0)
            }
        }
    };
}
id_type!(SheetId, "sheet");
id_type!(ViewId, "view");
id_type!(AnnotId, "annotation");

/// The drafting standard (DEC-027): projection angle, sheet sizes, title block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Standard {
    /// Third-angle projection, ANSI sheets.
    #[default]
    Ansi,
    /// First-angle projection, ISO sheets.
    Iso,
}

/// A sheet size, landscape, in millimetres.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetSize {
    pub name: String,
    pub width: f64,
    pub height: f64,
}

/// A line of a title block, from the sheet's bottom-right corner (x <= 0, y >= 0).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TitleLine {
    pub a: Vec2,
    pub b: Vec2,
}

/// A text field of a title block: its label, and where the value goes (bottom-left of the text,
/// from the sheet's bottom-right corner).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TitleField {
    /// What fills it: a drawing property (`title`, `number`, `revision`, `company`, `drawn_by`,
    /// `date`) or a computed value (`scale`, `sheet`, `size`, `units`).
    pub key: String,
    pub label: String,
    pub at: Vec2,
    pub height: f64,
}

/// A title block template: lines and fields. A drawing keeps its own copy, so it can be
/// changed per drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TitleBlock {
    pub name: String,
    pub lines: Vec<TitleLine>,
    pub fields: Vec<TitleField>,
    /// Where the projection symbol goes (its centre, from the bottom-right corner), if shown.
    #[serde(default)]
    pub projection_symbol: Option<Vec2>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub id: SheetId,
    pub name: String,
    pub size: SheetSize,
    pub title_block: TitleBlock,
    #[serde(default = "yes")]
    pub border: bool,
}

fn yes() -> bool {
    true
}

/// Which way a base view looks at the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    Front,
    Back,
    Top,
    Bottom,
    Left,
    Right,
    /// From above, in front and to the right.
    Iso,
}

impl Orientation {
    pub const ALL: [Orientation; 7] =
        [Orientation::Front, Orientation::Top, Orientation::Right, Orientation::Back, Orientation::Bottom, Orientation::Left, Orientation::Iso];

    /// The name commands take.
    pub fn id(self) -> &'static str {
        match self {
            Orientation::Front => "front",
            Orientation::Back => "back",
            Orientation::Top => "top",
            Orientation::Bottom => "bottom",
            Orientation::Left => "left",
            Orientation::Right => "right",
            Orientation::Iso => "iso",
        }
    }

    pub fn from_id(s: &str) -> Option<Orientation> {
        Some(match s.to_ascii_lowercase().as_str() {
            "front" => Orientation::Front,
            "back" => Orientation::Back,
            "top" => Orientation::Top,
            "bottom" => Orientation::Bottom,
            "left" => Orientation::Left,
            "right" => Orientation::Right,
            "iso" | "isometric" => Orientation::Iso,
            _ => return None,
        })
    }
}

/// Where a projected view sits relative to its parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Right,
    Left,
    Above,
    Below,
    /// Diagonal positions give isometric views.
    AboveRight,
    AboveLeft,
    BelowRight,
    BelowLeft,
}

impl Side {
    /// The name commands take.
    pub fn id(self) -> &'static str {
        match self {
            Side::Right => "right",
            Side::Left => "left",
            Side::Above => "above",
            Side::Below => "below",
            Side::AboveRight => "above_right",
            Side::AboveLeft => "above_left",
            Side::BelowRight => "below_right",
            Side::BelowLeft => "below_left",
        }
    }

    pub fn from_id(s: &str) -> Option<Side> {
        Some(match s.to_ascii_lowercase().as_str() {
            "right" => Side::Right,
            "left" => Side::Left,
            "above" | "top" | "up" => Side::Above,
            "below" | "bottom" | "down" => Side::Below,
            "above_right" => Side::AboveRight,
            "above_left" => Side::AboveLeft,
            "below_right" => Side::BelowRight,
            "below_left" => Side::BelowLeft,
            _ => return None,
        })
    }
    pub fn diagonal(self) -> bool {
        matches!(self, Side::AboveRight | Side::AboveLeft | Side::BelowRight | Side::BelowLeft)
    }
    /// The unit step on the sheet from the parent towards this side.
    pub fn step(self) -> Vec2 {
        match self {
            Side::Right => Vec2::new(1.0, 0.0),
            Side::Left => Vec2::new(-1.0, 0.0),
            Side::Above => Vec2::new(0.0, 1.0),
            Side::Below => Vec2::new(0.0, -1.0),
            Side::AboveRight => Vec2::new(1.0, 1.0),
            Side::AboveLeft => Vec2::new(-1.0, 1.0),
            Side::BelowRight => Vec2::new(1.0, -1.0),
            Side::BelowLeft => Vec2::new(-1.0, -1.0),
        }
    }
}

/// What a view shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewKind {
    /// Looks at the model from one of the standard directions.
    Base { orientation: Orientation },
    /// Looks at the model from beside its parent (right, above, ...), by the drawing's projection
    /// angle; diagonal sides give isometric views.
    Projected { parent: ViewId, side: Side },
    /// The model cut along the line `a`-`b` of the parent view (parent view coordinates, model mm),
    /// seen in the direction of the section arrows (to the right of `a`-`b`; `flip` the other way).
    Section {
        parent: ViewId,
        a: Vec2,
        b: Vec2,
        #[serde(default)]
        flip: bool,
    },
    /// A circle of the parent view (`center`, `radius` in parent view coordinates) drawn at its
    /// own scale.
    Detail { parent: ViewId, center: Vec2, radius: f64 },
}

impl ViewKind {
    pub fn parent(&self) -> Option<ViewId> {
        match self {
            ViewKind::Base { .. } => None,
            ViewKind::Projected { parent, .. } | ViewKind::Section { parent, .. } | ViewKind::Detail { parent, .. } => Some(*parent),
        }
    }
}

/// A view of a model on a sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub id: ViewId,
    pub sheet: SheetId,
    /// "VIEW1", or the section or detail letter.
    pub name: String,
    /// The part or assembly file. In memory the full path; in the file relative to the drawing.
    pub model: String,
    pub kind: ViewKind,
    /// Paper mm per model mm.
    pub scale: f64,
    /// Where the middle of the view's geometry is on the sheet (mm).
    pub center: Vec2,
    /// Hidden lines drawn dashed.
    #[serde(default)]
    pub hidden: bool,
    /// Edges between tangent faces drawn.
    #[serde(default)]
    pub tangent: bool,
    /// Centre marks on circles and centrelines on cylinders, made automatically.
    #[serde(default = "yes")]
    pub centerlines: bool,
    /// The name and scale written under the view.
    #[serde(default = "yes")]
    pub label: bool,
}

/// A point of the model a dimension or balloon is attached to: an edge (by persistent name) of a
/// component (or of the part), and which point of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeomPick {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<ComponentId>,
    pub edge: EdgeRef,
    pub point: PickPoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickPoint {
    Start,
    End,
    Mid,
    /// The centre of a circular edge.
    Center,
    /// The whole edge (a line's length, a circle's size).
    Whole,
}

impl PickPoint {
    pub fn from_id(s: &str) -> Option<PickPoint> {
        Some(match s {
            "start" => PickPoint::Start,
            "end" => PickPoint::End,
            "mid" => PickPoint::Mid,
            "center" | "centre" => PickPoint::Center,
            "whole" | "edge" => PickPoint::Whole,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimKind {
    Horizontal,
    Vertical,
    Aligned,
    Diameter,
    Radius,
    Angle,
}

impl DimKind {
    pub fn from_id(s: &str) -> Option<DimKind> {
        Some(match s {
            "horizontal" => DimKind::Horizontal,
            "vertical" => DimKind::Vertical,
            "aligned" | "linear" => DimKind::Aligned,
            "diameter" => DimKind::Diameter,
            "radius" => DimKind::Radius,
            "angle" | "angular" => DimKind::Angle,
            _ => return None,
        })
    }
}

/// What an annotation is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnnotKind {
    /// A dimension of a view: between `a` and `b` (or of `a` alone), its line or text placed
    /// `offset` from the view's centre on the sheet (so it moves with the view).
    Dimension {
        view: ViewId,
        dim: DimKind,
        a: GeomPick,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        b: Option<GeomPick>,
        offset: Vec2,
        /// Replaces the measured value (`<>` stands for it).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        /// Decimal places at most (trailing zeros dropped).
        #[serde(default = "two")]
        precision: u8,
    },
    /// A table of a view's holes, with their positions from `origin` (default: the bottom-left of
    /// the view) and tags on the holes. `at` is the table's top-left corner on the sheet.
    HoleTable {
        view: ViewId,
        at: Vec2,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<GeomPick>,
    },
    /// An item number on a component of an assembly view, with a leader to `attach` (a point in
    /// the component's part coordinates); the balloon is `offset` from the view's centre.
    Balloon { view: ViewId, component: ComponentId, attach: Vec3, offset: Vec2 },
    /// The assembly's parts list (from its bill of materials); `at` is its bottom-right corner.
    PartsList { view: ViewId, at: Vec2 },
    /// Text on a sheet.
    Note {
        sheet: SheetId,
        at: Vec2,
        text: String,
        #[serde(default = "note_height")]
        height: f64,
    },
}

fn two() -> u8 {
    2
}

fn note_height() -> f64 {
    3.5
}

impl AnnotKind {
    /// The view the annotation belongs to, if it belongs to one.
    pub fn view(&self) -> Option<ViewId> {
        match self {
            AnnotKind::Dimension { view, .. }
            | AnnotKind::HoleTable { view, .. }
            | AnnotKind::Balloon { view, .. }
            | AnnotKind::PartsList { view, .. } => Some(*view),
            AnnotKind::Note { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: AnnotId,
    pub kind: AnnotKind,
}

/// What fills the title block.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Props {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub number: String,
    #[serde(default)]
    pub revision: String,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub drawn_by: String,
    #[serde(default)]
    pub date: String,
}

/// A drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    pub name: String,
    #[serde(default)]
    pub standard: Standard,
    #[serde(default)]
    pub props: Props,
    pub sheets: Vec<Sheet>,
    pub views: Vec<View>,
    pub annotations: Vec<Annotation>,
    pub next_sheet: u32,
    pub next_view: u32,
    pub next_annotation: u32,
}

fn finite2(v: Vec2) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.x.abs() <= tol::MAX_SIZE && v.y.abs() <= tol::MAX_SIZE
}

impl Drawing {
    /// An empty drawing with one sheet of the standard's middle size.
    pub fn new(name: &str, standard: Standard) -> Drawing {
        let size = crate::sheets::default_size(standard);
        let mut d = Drawing {
            name: name.to_owned(),
            standard,
            props: Props { title: name.to_owned(), ..Props::default() },
            sheets: Vec::new(),
            views: Vec::new(),
            annotations: Vec::new(),
            next_sheet: 1,
            next_view: 1,
            next_annotation: 1,
        };
        d.add_sheet(size);
        d
    }

    pub fn add_sheet(&mut self, size: SheetSize) -> SheetId {
        let id = SheetId(self.next_sheet);
        self.next_sheet += 1;
        let title_block = crate::sheets::title_block(self.standard, &size);
        self.sheets.push(Sheet { id, name: format!("Sheet:{}", id.0), size, title_block, border: true });
        id
    }

    pub fn sheet(&self, id: SheetId) -> Option<&Sheet> {
        self.sheets.iter().find(|s| s.id == id)
    }
    pub fn view(&self, id: ViewId) -> Option<&View> {
        self.views.iter().find(|v| v.id == id)
    }
    pub fn view_mut(&mut self, id: ViewId) -> Option<&mut View> {
        self.views.iter_mut().find(|v| v.id == id)
    }
    pub fn annotation(&self, id: AnnotId) -> Option<&Annotation> {
        self.annotations.iter().find(|a| a.id == id)
    }

    pub fn take_view_id(&mut self) -> ViewId {
        let id = ViewId(self.next_view);
        self.next_view += 1;
        id
    }
    pub fn take_annotation_id(&mut self) -> AnnotId {
        let id = AnnotId(self.next_annotation);
        self.next_annotation += 1;
        id
    }

    /// "VIEW3" for the next plain view; the next free capital letter for a section or detail.
    pub fn next_view_name(&self, lettered: bool) -> String {
        if lettered {
            let used: Vec<&str> = self.views.iter().map(|v| v.name.as_str()).collect();
            for c in 'A'..='Z' {
                let s = c.to_string();
                if !used.contains(&s.as_str()) && c != 'I' && c != 'O' {
                    return s;
                }
            }
            return format!("Z{}", self.next_view);
        }
        let n = self.views.iter().filter_map(|v| v.name.strip_prefix("VIEW").and_then(|n| n.parse::<u32>().ok())).max().unwrap_or(0);
        format!("VIEW{}", n + 1)
    }

    /// Views on a sheet, in the order they were made.
    pub fn views_on(&self, sheet: SheetId) -> impl Iterator<Item = &View> {
        self.views.iter().filter(move |v| v.sheet == sheet)
    }

    /// A view and every view made from it, at any depth.
    pub fn family(&self, id: ViewId) -> Vec<ViewId> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            let p = out[i];
            out.extend(self.views.iter().filter(|v| v.kind.parent() == Some(p)).map(|v| v.id));
            i += 1;
        }
        out
    }

    /// Removes a view, the views made from it and their annotations.
    pub fn remove_view(&mut self, id: ViewId) -> bool {
        let gone = self.family(id);
        let before = self.views.len();
        self.views.retain(|v| !gone.contains(&v.id));
        self.annotations.retain(|a| a.kind.view().is_none_or(|v| !gone.contains(&v)));
        self.views.len() != before
    }

    /// Rewrites every view's model path.
    pub fn map_models(&mut self, f: impl Fn(&str) -> String) {
        for v in &mut self.views {
            v.model = f(&v.model);
        }
    }

    /// The model files the drawing uses, each once.
    pub fn models(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for v in &self.views {
            if !out.contains(&v.model) {
                out.push(v.model.clone());
            }
        }
        out
    }

    /// Checks a document read from a file: ids, references, numbers.
    pub fn validate(&self) -> Result<(), String> {
        if self.sheets.is_empty() || self.sheets.len() > MAX_SHEETS {
            return Err(format!("a drawing has 1 to {MAX_SHEETS} sheets"));
        }
        if self.views.len() > MAX_VIEWS || self.annotations.len() > MAX_ANNOTATIONS {
            return Err("too many views or annotations".into());
        }
        let mut sheets = std::collections::BTreeSet::new();
        for s in &self.sheets {
            if s.id.0 == 0 || s.id.0 >= self.next_sheet || !sheets.insert(s.id) {
                return Err(format!("{} has a bad or repeated id", s.id));
            }
            let size_ok = |v: f64| v.is_finite() && (10.0..=10_000.0).contains(&v);
            if !size_ok(s.size.width) || !size_ok(s.size.height) {
                return Err(format!("{} has a bad size", s.name));
            }
            if s.title_block.lines.len() > 1000 || s.title_block.fields.len() > 200 {
                return Err(format!("{}: the title block is too big", s.name));
            }
            let lines_ok = s.title_block.lines.iter().all(|l| finite2(l.a) && finite2(l.b));
            let fields_ok = s.title_block.fields.iter().all(|f| finite2(f.at) && f.height.is_finite() && f.height > 0.0 && f.height < 100.0);
            if !lines_ok || !fields_ok {
                return Err(format!("{}: the title block has a value out of range", s.name));
            }
        }
        let mut views = std::collections::BTreeSet::new();
        for v in &self.views {
            if v.id.0 == 0 || v.id.0 >= self.next_view || !views.insert(v.id) {
                return Err(format!("{} has a bad or repeated id", v.id));
            }
            if !sheets.contains(&v.sheet) {
                return Err(format!("{} is on a missing {}", v.name, v.sheet));
            }
            if !(v.scale.is_finite() && (1e-4..=1e4).contains(&v.scale)) || !finite2(v.center) {
                return Err(format!("{} has a bad scale or position", v.name));
            }
            if v.model.is_empty() || v.model.len() > 4096 || v.model.contains('\0') {
                return Err(format!("{} has a bad model path", v.name));
            }
            match &v.kind {
                ViewKind::Base { .. } => {}
                ViewKind::Projected { parent, .. } => {
                    // Parents come first, so there are no cycles.
                    if !views.contains(parent) || parent == &v.id {
                        return Err(format!("{} is projected from a missing view", v.name));
                    }
                }
                ViewKind::Section { parent, a, b, .. } => {
                    if !views.contains(parent) || !finite2(*a) || !finite2(*b) || a.dist(*b) < tol::LINEAR {
                        return Err(format!("{} has a bad section line", v.name));
                    }
                }
                ViewKind::Detail { parent, center, radius } => {
                    if !views.contains(parent) || !finite2(*center) || !(radius.is_finite() && *radius > tol::LINEAR && *radius < tol::MAX_SIZE) {
                        return Err(format!("{} has a bad detail circle", v.name));
                    }
                }
            }
            if let Some(p) = v.kind.parent().and_then(|p| self.view(p))
                && p.model != v.model
            {
                return Err(format!("{} shows another model than its parent", v.name));
            }
        }
        let mut ids = std::collections::BTreeSet::new();
        for a in &self.annotations {
            if a.id.0 == 0 || a.id.0 >= self.next_annotation || !ids.insert(a.id) {
                return Err(format!("{} has a bad or repeated id", a.id));
            }
            if let Some(v) = a.kind.view()
                && !views.contains(&v)
            {
                return Err(format!("{} belongs to a missing {v}", a.id));
            }
            let ok = match &a.kind {
                AnnotKind::Dimension { offset, text, .. } => finite2(*offset) && text.as_ref().is_none_or(|t| t.len() <= 1000),
                AnnotKind::HoleTable { at, .. } | AnnotKind::PartsList { at, .. } => finite2(*at),
                AnnotKind::Balloon { attach, offset, .. } => finite2(*offset) && attach.is_finite(),
                AnnotKind::Note { sheet, at, text, height } => {
                    sheets.contains(sheet) && finite2(*at) && text.len() <= 10_000 && height.is_finite() && *height > 0.0 && *height < 100.0
                }
            };
            if !ok {
                return Err(format!("{} has a value out of range", a.id));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn names_families_and_validation() {
        let mut d = Drawing::new("Plate", Standard::Ansi);
        assert_eq!(d.sheets[0].size.name, "B");
        let sheet = d.sheets[0].id;
        let add = |d: &mut Drawing, kind: ViewKind, lettered: bool| {
            let id = d.take_view_id();
            let name = d.next_view_name(lettered);
            d.views.push(View {
                id,
                sheet,
                name,
                model: "plate.tenon".into(),
                kind,
                scale: 1.0,
                center: Vec2::new(100.0, 100.0),
                hidden: true,
                tangent: false,
                centerlines: true,
                label: true,
            });
            id
        };
        let base = add(&mut d, ViewKind::Base { orientation: Orientation::Front }, false);
        let right = add(&mut d, ViewKind::Projected { parent: base, side: Side::Right }, false);
        let section = add(&mut d, ViewKind::Section { parent: base, a: Vec2::new(0.0, 5.0), b: Vec2::new(10.0, 5.0), flip: false }, true);
        assert_eq!(d.view(right).unwrap().name, "VIEW2");
        assert_eq!(d.view(section).unwrap().name, "A");
        d.validate().unwrap();
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<Drawing>(&json).unwrap(), d);
        assert_eq!(d.family(base), [base, right, section]);
        let mut bad = d.clone();
        bad.views[2].kind = ViewKind::Section { parent: base, a: Vec2::new(1.0, 1.0), b: Vec2::new(1.0, 1.0), flip: false };
        assert!(bad.validate().unwrap_err().contains("section"));
        let mut bad = d.clone();
        bad.views[1].scale = 0.0;
        assert!(bad.validate().is_err());
        let mut bad = d.clone();
        bad.views[1].kind = ViewKind::Projected { parent: ViewId(77), side: Side::Left };
        assert!(bad.validate().is_err());
        assert!(d.remove_view(base));
        assert!(d.views.is_empty());
    }
}

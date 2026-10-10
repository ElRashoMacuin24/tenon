//! Tenon's icon set, drawn in code (no image assets). Each icon is a few strokes in a unit square;
//! all designs are original.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    // part modelling
    NewSketch,
    Extrude,
    Revolve,
    Sweep,
    Loft,
    Coil,
    Fillet,
    Chamfer,
    Shell,
    Hole,
    Plane,
    Axis,
    Point,
    PatternRect,
    PatternCircular,
    Mirror,
    // sketching
    Line,
    Circle,
    Arc,
    Rectangle,
    Polygon,
    Spline,
    Trim,
    Offset,
    Dimension,
    Coincident,
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Tangent,
    FinishSketch,
    // inspect and tools
    Measure,
    MassProps,
    Parameters,
    Settings,
    // view and navigation
    Home,
    Orbit,
    Pan,
    Zoom,
    ZoomFit,
    LookAt,
    Browser,
    Cube,
    // file
    New,
    Open,
    Save,
    Undo,
    Redo,
    // browser tree
    Part,
    Folder,
    Body,
    Info,
    EndOfPart,
    // more modelling
    Rib,
    Draft,
    Thread,
    Combine,
    Split,
    Ucs,
    // more sketching
    Collinear,
    Symmetric,
    Fix,
    Equal,
    Concentric,
    Construction,
    Driven,
    Text,
    Move,
    Copy,
    Rotate,
    Extend,
    Scale,
    Stretch,
    // chrome
    Update,
    Search,
    Menu,
    Close,
    Plus,
    VisualStyle,
    Window,
    PreviousView,
    Projection,
    Delete,
    // assemblies
    Assembly,
    Place,
    CreateComponent,
    Ground,
    Joint,
    Constrain,
    Explode,
    Interference,
    Bom,
    Dof,
    Return,
    // drawings
    Drawing,
    BaseView,
    ProjectedView,
    SectionView,
    DetailView,
    NewSheet,
    Balloon,
    HoleTable,
    CenterMark,
    Centerline,
    CenterlineBisector,
}

/// What an icon depicts, for its fill colour.
fn category(icon: Icon) -> Option<Category> {
    use Icon::*;
    Some(match icon {
        Extrude | Revolve | Sweep | Loft | Coil | Fillet | Chamfer | Shell | Hole | Rib | Draft | Thread | Combine | Split | Part | Body | Cube
        | MassProps | PatternRect | PatternCircular | Mirror | Assembly | Place | CreateComponent | Explode | BaseView | ProjectedView
        | DetailView => Category::Solid,
        Plane | Axis | Point | Ucs | LookAt => Category::Work,
        NewSketch | FinishSketch => Category::Sketch,
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Category {
    Solid,
    Work,
    Sketch,
}

struct Pen<'a> {
    p: &'a Painter,
    r: Rect,
    s: Stroke,
}

impl Pen<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.r.left() + x * self.r.width(), self.r.top() + y * self.r.height())
    }
    fn line(&self, pts: &[(f32, f32)]) {
        self.p.add(Shape::line(pts.iter().map(|&(x, y)| self.at(x, y)).collect(), self.s));
    }
    fn closed(&self, pts: &[(f32, f32)]) {
        self.p.add(Shape::closed_line(pts.iter().map(|&(x, y)| self.at(x, y)).collect(), self.s));
    }
    fn fill(&self, pts: &[(f32, f32)], color: Color32) {
        self.p.add(Shape::convex_polygon(pts.iter().map(|&(x, y)| self.at(x, y)).collect(), color, Stroke::NONE));
    }
    fn circle(&self, c: (f32, f32), rad: f32) {
        self.p.circle_stroke(self.at(c.0, c.1), rad * self.r.width(), self.s);
    }
    fn dot(&self, c: (f32, f32), rad: f32) {
        self.p.circle_filled(self.at(c.0, c.1), rad * self.r.width(), self.s.color);
    }
    /// Elliptical arc from angle `a0` to `a1` (radians, y down).
    fn arc(&self, c: (f32, f32), rx: f32, ry: f32, a0: f32, a1: f32) {
        let n = 18;
        let pts = (0..=n)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / n as f32;
                self.at(c.0 + rx * a.cos(), c.1 + ry * a.sin())
            })
            .collect();
        self.p.add(Shape::line(pts, self.s));
    }
    fn arrow_head(&self, tip: (f32, f32), from: (f32, f32)) {
        let (dx, dy) = (tip.0 - from.0, tip.1 - from.1);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let (ux, uy) = (dx / len * 0.16, dy / len * 0.16);
        self.line(&[(tip.0 - ux - uy * 0.6, tip.1 - uy + ux * 0.6), tip, (tip.0 - ux + uy * 0.6, tip.1 - uy - ux * 0.6)]);
    }
}

const TAU: f32 = std::f32::consts::TAU;
const PI: f32 = std::f32::consts::PI;

/// Paints `icon` into `rect` in `color`, with translucent fills.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    draw(painter, rect, icon, color, color.gamma_multiply(0.35));
}

/// Paints `icon` with its category colour as the fill (ribbon and browser): solids blue, work
/// features amber, sketches green. Disabled icons stay grey.
pub fn paint_colored(painter: &Painter, rect: Rect, icon: Icon, color: Color32, t: &crate::theme::Tokens, enabled: bool) {
    let fill = match (enabled, category(icon)) {
        (true, Some(Category::Solid)) => t.tint_solid.gamma_multiply(0.85),
        (true, Some(Category::Work)) => t.tint_work.gamma_multiply(0.85),
        (true, Some(Category::Sketch)) => t.tint_sketch.gamma_multiply(0.85),
        _ => color.gamma_multiply(0.35),
    };
    let color = if enabled && icon == Icon::FinishSketch { t.ok } else { color };
    draw(painter, rect, icon, color, fill);
}

fn draw(painter: &Painter, rect: Rect, icon: Icon, color: Color32, soft: Color32) {
    let width = (rect.width() / 14.0).clamp(1.0, 2.2);
    let pen = Pen { p: painter, r: rect, s: Stroke::new(width, color) };
    let accent = soft;
    match icon {
        Icon::NewSketch => {
            pen.fill(&[(0.1, 0.25), (0.75, 0.25), (0.75, 0.9), (0.1, 0.9)], soft);
            pen.closed(&[(0.1, 0.25), (0.75, 0.25), (0.75, 0.9), (0.1, 0.9)]);
            pen.line(&[(0.45, 0.6), (0.9, 0.12)]);
            pen.line(&[(0.38, 0.68), (0.45, 0.6)]);
        }
        Icon::Extrude => {
            pen.fill(&[(0.1, 0.75), (0.55, 0.75), (0.85, 0.58), (0.4, 0.58)], soft);
            pen.closed(&[(0.1, 0.75), (0.55, 0.75), (0.85, 0.58), (0.4, 0.58)]);
            pen.line(&[(0.1, 0.75), (0.1, 0.4), (0.55, 0.4), (0.55, 0.75)]);
            pen.line(&[(0.1, 0.4), (0.4, 0.23), (0.85, 0.23), (0.85, 0.58)]);
            pen.line(&[(0.55, 0.4), (0.85, 0.23)]);
        }
        Icon::Revolve => {
            pen.line(&[(0.5, 0.05), (0.5, 0.95)]);
            pen.arc((0.5, 0.55), 0.38, 0.16, 0.2, TAU - 0.4);
            pen.arrow_head((0.84, 0.47), (0.7, 0.42));
            pen.fill(&[(0.5, 0.25), (0.75, 0.3), (0.75, 0.55), (0.5, 0.55)], soft);
        }
        Icon::Sweep => {
            pen.arc((0.15, 0.8), 0.7, 0.62, -PI / 2.0, 0.0);
            pen.circle((0.15, 0.18), 0.12);
            pen.dot((0.85, 0.8), 0.06);
        }
        Icon::Loft => {
            pen.arc((0.5, 0.78), 0.36, 0.12, 0.0, TAU);
            pen.arc((0.5, 0.22), 0.18, 0.07, 0.0, TAU);
            pen.line(&[(0.14, 0.78), (0.32, 0.22)]);
            pen.line(&[(0.86, 0.78), (0.68, 0.22)]);
        }
        Icon::Coil => {
            for i in 0..4 {
                let y = 0.2 + i as f32 * 0.18;
                pen.arc((0.5, y), 0.32, 0.08, 0.0, PI);
            }
        }
        Icon::Fillet => {
            pen.line(&[(0.15, 0.1), (0.15, 0.5)]);
            pen.arc((0.55, 0.5), 0.4, 0.4, PI / 2.0, PI);
            pen.line(&[(0.55, 0.9), (0.9, 0.9)]);
            pen.line(&[(0.15, 0.9), (0.15, 0.7)]);
            pen.line(&[(0.15, 0.9), (0.35, 0.9)]);
        }
        Icon::Chamfer => {
            pen.line(&[(0.15, 0.1), (0.15, 0.5), (0.55, 0.9), (0.9, 0.9)]);
            pen.line(&[(0.15, 0.9), (0.15, 0.7)]);
            pen.line(&[(0.15, 0.9), (0.35, 0.9)]);
        }
        Icon::Shell => {
            pen.closed(&[(0.1, 0.15), (0.9, 0.15), (0.9, 0.85), (0.1, 0.85)]);
            pen.line(&[(0.25, 0.15), (0.25, 0.7), (0.75, 0.7), (0.75, 0.15)]);
        }
        Icon::Hole => {
            painter.circle_filled(pen.at(0.5, 0.5), 0.4 * rect.width(), soft);
            pen.circle((0.5, 0.5), 0.4);
            pen.circle((0.5, 0.5), 0.2);
            pen.line(&[(0.5, 0.02), (0.5, 0.98)]);
            pen.line(&[(0.02, 0.5), (0.98, 0.5)]);
        }
        Icon::Plane => {
            pen.fill(&[(0.05, 0.75), (0.65, 0.75), (0.95, 0.25), (0.35, 0.25)], soft);
            pen.closed(&[(0.05, 0.75), (0.65, 0.75), (0.95, 0.25), (0.35, 0.25)]);
        }
        Icon::Axis => {
            pen.line(&[(0.1, 0.9), (0.9, 0.1)]);
            pen.dot((0.3, 0.7), 0.06);
            pen.dot((0.7, 0.3), 0.06);
        }
        Icon::Point => {
            pen.dot((0.5, 0.5), 0.12);
            pen.line(&[(0.5, 0.1), (0.5, 0.3)]);
            pen.line(&[(0.5, 0.7), (0.5, 0.9)]);
            pen.line(&[(0.1, 0.5), (0.3, 0.5)]);
            pen.line(&[(0.7, 0.5), (0.9, 0.5)]);
        }
        Icon::PatternRect => {
            for (x, y) in [(0.1, 0.1), (0.58, 0.1), (0.1, 0.58), (0.58, 0.58)] {
                pen.fill(&[(x, y), (x + 0.32, y), (x + 0.32, y + 0.32), (x, y + 0.32)], soft);
                pen.closed(&[(x, y), (x + 0.32, y), (x + 0.32, y + 0.32), (x, y + 0.32)]);
            }
        }
        Icon::PatternCircular => {
            for i in 0..6 {
                let a = i as f32 * TAU / 6.0;
                pen.dot((0.5 + 0.36 * a.cos(), 0.5 + 0.36 * a.sin()), 0.08);
            }
            pen.dot((0.5, 0.5), 0.04);
        }
        Icon::Mirror => {
            for i in 0..5 {
                let y = 0.05 + i as f32 * 0.2;
                pen.line(&[(0.5, y), (0.5, y + 0.1)]);
            }
            pen.closed(&[(0.4, 0.25), (0.4, 0.75), (0.08, 0.75)]);
            pen.fill(&[(0.6, 0.25), (0.6, 0.75), (0.92, 0.75)], soft);
            pen.closed(&[(0.6, 0.25), (0.6, 0.75), (0.92, 0.75)]);
        }
        Icon::Line => {
            pen.line(&[(0.15, 0.85), (0.85, 0.15)]);
            pen.dot((0.15, 0.85), 0.07);
            pen.dot((0.85, 0.15), 0.07);
        }
        Icon::Circle => {
            pen.circle((0.5, 0.5), 0.38);
            pen.dot((0.5, 0.5), 0.05);
        }
        Icon::Arc => {
            pen.arc((0.5, 0.75), 0.4, 0.5, PI, TAU);
            pen.dot((0.1, 0.75), 0.06);
            pen.dot((0.9, 0.75), 0.06);
        }
        Icon::Rectangle => {
            pen.closed(&[(0.1, 0.25), (0.9, 0.25), (0.9, 0.75), (0.1, 0.75)]);
            pen.dot((0.1, 0.25), 0.06);
            pen.dot((0.9, 0.75), 0.06);
        }
        Icon::Polygon => {
            let pts: Vec<(f32, f32)> = (0..6).map(|i| i as f32 * TAU / 6.0).map(|a| (0.5 + 0.4 * a.cos(), 0.5 + 0.4 * a.sin())).collect();
            pen.closed(&pts);
        }
        Icon::Spline => {
            pen.arc((0.3, 0.55), 0.2, 0.3, PI, TAU);
            pen.arc((0.7, 0.55), 0.2, 0.3, 0.0, PI);
        }
        Icon::Trim => {
            pen.line(&[(0.1, 0.5), (0.5, 0.5)]);
            for i in 0..3 {
                let x = 0.55 + i as f32 * 0.14;
                pen.line(&[(x, 0.5), (x + 0.07, 0.5)]);
            }
            pen.line(&[(0.5, 0.1), (0.5, 0.9)]);
        }
        Icon::Offset => {
            pen.arc((0.5, 0.9), 0.4, 0.6, PI, TAU);
            pen.arc((0.5, 0.9), 0.22, 0.35, PI, TAU);
        }
        Icon::Dimension => {
            pen.line(&[(0.1, 0.3), (0.1, 0.9)]);
            pen.line(&[(0.9, 0.3), (0.9, 0.9)]);
            pen.line(&[(0.1, 0.5), (0.9, 0.5)]);
            pen.arrow_head((0.1, 0.5), (0.5, 0.5));
            pen.arrow_head((0.9, 0.5), (0.5, 0.5));
        }
        Icon::Coincident => {
            pen.line(&[(0.1, 0.8), (0.5, 0.5)]);
            pen.line(&[(0.9, 0.8), (0.5, 0.5)]);
            pen.dot((0.5, 0.5), 0.1);
        }
        Icon::Horizontal => {
            pen.line(&[(0.1, 0.5), (0.9, 0.5)]);
            pen.line(&[(0.1, 0.35), (0.1, 0.65)]);
            pen.line(&[(0.9, 0.35), (0.9, 0.65)]);
        }
        Icon::Vertical => {
            pen.line(&[(0.5, 0.1), (0.5, 0.9)]);
            pen.line(&[(0.35, 0.1), (0.65, 0.1)]);
            pen.line(&[(0.35, 0.9), (0.65, 0.9)]);
        }
        Icon::Parallel => {
            pen.line(&[(0.15, 0.85), (0.6, 0.15)]);
            pen.line(&[(0.4, 0.85), (0.85, 0.15)]);
        }
        Icon::Perpendicular => {
            pen.line(&[(0.1, 0.85), (0.9, 0.85)]);
            pen.line(&[(0.5, 0.85), (0.5, 0.1)]);
            pen.line(&[(0.5, 0.65), (0.7, 0.65), (0.7, 0.85)]);
        }
        Icon::Tangent => {
            pen.circle((0.5, 0.6), 0.28);
            pen.line(&[(0.05, 0.32), (0.95, 0.32)]);
        }
        Icon::FinishSketch => {
            pen.line(&[(0.12, 0.55), (0.4, 0.82), (0.9, 0.2)]);
        }
        Icon::Measure => {
            pen.closed(&[(0.05, 0.35), (0.95, 0.35), (0.95, 0.65), (0.05, 0.65)]);
            for i in 1..6 {
                let x = 0.05 + i as f32 * 0.15;
                pen.line(&[(x, 0.35), (x, if i % 2 == 0 { 0.55 } else { 0.47 })]);
            }
        }
        Icon::MassProps => {
            pen.fill(&[(0.2, 0.9), (0.8, 0.9), (0.7, 0.4), (0.3, 0.4)], soft);
            pen.closed(&[(0.2, 0.9), (0.8, 0.9), (0.7, 0.4), (0.3, 0.4)]);
            pen.circle((0.5, 0.25), 0.13);
        }
        Icon::Parameters => {
            pen.line(&[(0.45, 0.15), (0.35, 0.15), (0.3, 0.25), (0.25, 0.85), (0.15, 0.9)]);
            pen.line(&[(0.15, 0.45), (0.45, 0.45)]);
            pen.line(&[(0.55, 0.45), (0.9, 0.85)]);
            pen.line(&[(0.9, 0.45), (0.55, 0.85)]);
        }
        Icon::Settings => {
            pen.circle((0.5, 0.5), 0.22);
            for i in 0..8 {
                let a = i as f32 * TAU / 8.0;
                pen.line(&[(0.5 + 0.3 * a.cos(), 0.5 + 0.3 * a.sin()), (0.5 + 0.42 * a.cos(), 0.5 + 0.42 * a.sin())]);
            }
        }
        Icon::Home => {
            pen.line(&[(0.1, 0.5), (0.5, 0.12), (0.9, 0.5)]);
            pen.line(&[(0.22, 0.42), (0.22, 0.88), (0.78, 0.88), (0.78, 0.42)]);
        }
        Icon::Orbit => {
            pen.arc((0.5, 0.5), 0.4, 0.4, -PI * 0.9, PI * 0.6);
            pen.arrow_head((0.5 + 0.4 * (PI * 0.6).cos(), 0.5 + 0.4 * (PI * 0.6).sin()), (0.62, 0.86));
            pen.dot((0.5, 0.5), 0.07);
        }
        Icon::Pan => {
            pen.line(&[(0.5, 0.08), (0.5, 0.92)]);
            pen.line(&[(0.08, 0.5), (0.92, 0.5)]);
            pen.arrow_head((0.5, 0.08), (0.5, 0.5));
            pen.arrow_head((0.5, 0.92), (0.5, 0.5));
            pen.arrow_head((0.08, 0.5), (0.5, 0.5));
            pen.arrow_head((0.92, 0.5), (0.5, 0.5));
        }
        Icon::Zoom => {
            pen.circle((0.42, 0.42), 0.28);
            pen.line(&[(0.62, 0.62), (0.9, 0.9)]);
            pen.line(&[(0.3, 0.42), (0.54, 0.42)]);
            pen.line(&[(0.42, 0.3), (0.42, 0.54)]);
        }
        Icon::ZoomFit => {
            pen.closed(&[(0.3, 0.3), (0.7, 0.3), (0.7, 0.7), (0.3, 0.7)]);
            for (a, b) in [((0.05, 0.05), (0.22, 0.22)), ((0.95, 0.05), (0.78, 0.22)), ((0.05, 0.95), (0.22, 0.78)), ((0.95, 0.95), (0.78, 0.78))] {
                pen.line(&[a, b]);
                pen.arrow_head(a, b);
            }
        }
        Icon::LookAt => {
            pen.fill(&[(0.1, 0.6), (0.6, 0.6), (0.9, 0.3), (0.4, 0.3)], soft);
            pen.closed(&[(0.1, 0.6), (0.6, 0.6), (0.9, 0.3), (0.4, 0.3)]);
            pen.line(&[(0.5, 0.95), (0.5, 0.5)]);
            pen.arrow_head((0.5, 0.5), (0.5, 0.95));
        }
        Icon::Browser => {
            pen.line(&[(0.15, 0.2), (0.85, 0.2)]);
            pen.line(&[(0.3, 0.45), (0.85, 0.45)]);
            pen.line(&[(0.3, 0.7), (0.85, 0.7)]);
            pen.line(&[(0.15, 0.2), (0.15, 0.7), (0.3, 0.7)]);
            pen.line(&[(0.15, 0.45), (0.3, 0.45)]);
        }
        Icon::Cube | Icon::Part | Icon::Body => {
            let fill = if icon == Icon::Body { accent.gamma_multiply(0.6) } else { soft };
            pen.fill(&[(0.5, 0.1), (0.9, 0.3), (0.5, 0.5), (0.1, 0.3)], fill);
            pen.closed(&[(0.5, 0.1), (0.9, 0.3), (0.9, 0.72), (0.5, 0.92), (0.1, 0.72), (0.1, 0.3)]);
            pen.line(&[(0.1, 0.3), (0.5, 0.5), (0.9, 0.3)]);
            pen.line(&[(0.5, 0.5), (0.5, 0.92)]);
        }
        Icon::New => {
            pen.closed(&[(0.2, 0.08), (0.62, 0.08), (0.82, 0.28), (0.82, 0.92), (0.2, 0.92)]);
            pen.line(&[(0.62, 0.08), (0.62, 0.28), (0.82, 0.28)]);
        }
        Icon::Open | Icon::Folder => {
            pen.fill(&[(0.08, 0.3), (0.92, 0.3), (0.92, 0.85), (0.08, 0.85)], soft);
            pen.closed(&[(0.08, 0.2), (0.4, 0.2), (0.48, 0.3), (0.92, 0.3), (0.92, 0.85), (0.08, 0.85)]);
        }
        Icon::Save => {
            pen.closed(&[(0.1, 0.1), (0.75, 0.1), (0.9, 0.25), (0.9, 0.9), (0.1, 0.9)]);
            pen.closed(&[(0.28, 0.1), (0.68, 0.1), (0.68, 0.36), (0.28, 0.36)]);
            pen.closed(&[(0.25, 0.55), (0.75, 0.55), (0.75, 0.9), (0.25, 0.9)]);
        }
        Icon::Undo => {
            pen.arc((0.55, 0.6), 0.3, 0.28, -PI / 2.0, PI / 2.0);
            pen.line(&[(0.55, 0.32), (0.15, 0.32)]);
            pen.arrow_head((0.15, 0.32), (0.4, 0.32));
        }
        Icon::Redo => {
            pen.arc((0.45, 0.6), 0.3, 0.28, PI / 2.0, PI * 1.5);
            pen.line(&[(0.45, 0.32), (0.85, 0.32)]);
            pen.arrow_head((0.85, 0.32), (0.6, 0.32));
        }
        Icon::Info => {
            pen.circle((0.5, 0.5), 0.42);
            pen.dot((0.5, 0.3), 0.06);
            pen.line(&[(0.5, 0.45), (0.5, 0.75)]);
        }
        Icon::EndOfPart => {
            let red = Color32::from_rgb(0xd8, 0x44, 0x38);
            painter.circle_filled(pen.at(0.5, 0.5), 0.42 * rect.width(), red);
            let white = Pen { p: painter, r: rect, s: Stroke::new(width, Color32::WHITE) };
            white.line(&[(0.32, 0.32), (0.68, 0.68)]);
            white.line(&[(0.68, 0.32), (0.32, 0.68)]);
        }
        Icon::Rib => {
            pen.fill(&[(0.1, 0.85), (0.9, 0.85), (0.9, 0.7), (0.1, 0.7)], soft);
            pen.closed(&[(0.1, 0.85), (0.9, 0.85), (0.9, 0.7), (0.1, 0.7)]);
            pen.fill(&[(0.42, 0.7), (0.58, 0.7), (0.58, 0.15), (0.42, 0.15)], soft);
            pen.closed(&[(0.42, 0.7), (0.58, 0.7), (0.58, 0.15), (0.42, 0.15)]);
        }
        Icon::Draft => {
            pen.fill(&[(0.25, 0.9), (0.75, 0.9), (0.65, 0.15), (0.35, 0.15)], soft);
            pen.closed(&[(0.25, 0.9), (0.75, 0.9), (0.65, 0.15), (0.35, 0.15)]);
        }
        Icon::Thread => {
            pen.closed(&[(0.3, 0.1), (0.7, 0.1), (0.7, 0.9), (0.3, 0.9)]);
            for i in 0..4 {
                let y = 0.2 + i as f32 * 0.18;
                pen.line(&[(0.3, y + 0.1), (0.7, y)]);
            }
        }
        Icon::Combine => {
            pen.fill(&[(0.1, 0.35), (0.6, 0.35), (0.6, 0.85), (0.1, 0.85)], soft);
            pen.closed(&[(0.1, 0.35), (0.6, 0.35), (0.6, 0.85), (0.1, 0.85)]);
            pen.closed(&[(0.4, 0.15), (0.9, 0.15), (0.9, 0.65), (0.4, 0.65)]);
        }
        Icon::Split => {
            pen.fill(&[(0.1, 0.2), (0.45, 0.2), (0.45, 0.8), (0.1, 0.8)], soft);
            pen.closed(&[(0.1, 0.2), (0.45, 0.2), (0.45, 0.8), (0.1, 0.8)]);
            pen.closed(&[(0.55, 0.2), (0.9, 0.2), (0.9, 0.8), (0.55, 0.8)]);
        }
        Icon::Ucs => {
            pen.line(&[(0.2, 0.8), (0.85, 0.8)]);
            pen.line(&[(0.2, 0.8), (0.2, 0.15)]);
            pen.line(&[(0.2, 0.8), (0.55, 0.5)]);
            pen.dot((0.2, 0.8), 0.07);
        }
        Icon::Collinear => {
            pen.line(&[(0.05, 0.75), (0.4, 0.55)]);
            pen.line(&[(0.6, 0.45), (0.95, 0.25)]);
            for i in 0..2 {
                let t = 0.42 + i as f32 * 0.1;
                pen.line(&[(t, 0.54 - i as f32 * 0.05), (t + 0.04, 0.52 - i as f32 * 0.05)]);
            }
        }
        Icon::Symmetric => {
            pen.line(&[(0.5, 0.05), (0.5, 0.95)]);
            pen.closed(&[(0.12, 0.3), (0.38, 0.5), (0.12, 0.7)]);
            pen.closed(&[(0.88, 0.3), (0.62, 0.5), (0.88, 0.7)]);
        }
        Icon::Fix => {
            pen.closed(&[(0.25, 0.45), (0.75, 0.45), (0.75, 0.9), (0.25, 0.9)]);
            pen.arc((0.5, 0.45), 0.17, 0.22, PI, TAU);
            pen.dot((0.5, 0.66), 0.06);
        }
        Icon::Equal => {
            pen.line(&[(0.15, 0.38), (0.85, 0.38)]);
            pen.line(&[(0.15, 0.62), (0.85, 0.62)]);
        }
        Icon::Concentric => {
            pen.circle((0.5, 0.5), 0.4);
            pen.circle((0.5, 0.5), 0.2);
        }
        // A dimension in parentheses.
        Icon::Driven => {
            pen.arc((0.3, 0.5), 0.2, 0.38, 2.2, 4.08);
            pen.arc((0.7, 0.5), 0.2, 0.38, -0.94, 0.94);
            pen.line(&[(0.3, 0.5), (0.7, 0.5)]);
            pen.arrow_head((0.3, 0.5), (0.5, 0.5));
            pen.arrow_head((0.7, 0.5), (0.5, 0.5));
        }
        Icon::Construction => {
            for i in 0..4 {
                let x = 0.08 + i as f32 * 0.23;
                pen.line(&[(x, 0.92 - i as f32 * 0.23), (x + 0.13, 0.79 - i as f32 * 0.23)]);
            }
        }
        Icon::Text => {
            pen.line(&[(0.15, 0.15), (0.85, 0.15)]);
            pen.line(&[(0.5, 0.15), (0.5, 0.88)]);
            pen.line(&[(0.38, 0.88), (0.62, 0.88)]);
        }
        Icon::Move => {
            pen.closed(&[(0.1, 0.5), (0.4, 0.5), (0.4, 0.85), (0.1, 0.85)]);
            pen.line(&[(0.45, 0.4), (0.85, 0.15)]);
            pen.arrow_head((0.85, 0.15), (0.45, 0.4));
        }
        Icon::Copy => {
            pen.closed(&[(0.1, 0.4), (0.55, 0.4), (0.55, 0.9), (0.1, 0.9)]);
            pen.closed(&[(0.35, 0.1), (0.85, 0.1), (0.85, 0.6), (0.65, 0.6)]);
        }
        Icon::Rotate => {
            pen.arc((0.5, 0.55), 0.34, 0.34, -PI * 0.95, PI * 0.35);
            pen.arrow_head((0.5 + 0.34 * (PI * 0.35).cos(), 0.55 + 0.34 * (PI * 0.35).sin()), (0.85, 0.6));
            pen.dot((0.5, 0.55), 0.05);
        }
        Icon::Extend => {
            pen.line(&[(0.1, 0.5), (0.55, 0.5)]);
            for i in 0..2 {
                let x = 0.6 + i as f32 * 0.12;
                pen.line(&[(x, 0.5), (x + 0.06, 0.5)]);
            }
            pen.line(&[(0.9, 0.1), (0.9, 0.9)]);
        }
        Icon::Scale => {
            pen.closed(&[(0.1, 0.55), (0.45, 0.55), (0.45, 0.9), (0.1, 0.9)]);
            pen.closed(&[(0.1, 0.1), (0.9, 0.1), (0.9, 0.9), (0.1, 0.9)]);
            pen.line(&[(0.5, 0.5), (0.8, 0.2)]);
        }
        Icon::Stretch => {
            pen.closed(&[(0.1, 0.3), (0.55, 0.3), (0.55, 0.7), (0.1, 0.7)]);
            pen.line(&[(0.55, 0.5), (0.92, 0.5)]);
            pen.arrow_head((0.92, 0.5), (0.6, 0.5));
        }
        Icon::Update => {
            pen.arc((0.5, 0.5), 0.36, 0.36, -PI * 0.8, PI * 0.3);
            pen.arrow_head((0.5 + 0.36 * (PI * 0.3).cos(), 0.5 + 0.36 * (PI * 0.3).sin()), (0.88, 0.5));
            pen.arc((0.5, 0.5), 0.36, 0.36, PI * 0.2, PI * 1.3);
            pen.arrow_head((0.5 + 0.36 * (PI * 1.3).cos(), 0.5 + 0.36 * (PI * 1.3).sin()), (0.12, 0.5));
        }
        Icon::Search => {
            pen.circle((0.42, 0.42), 0.27);
            pen.line(&[(0.62, 0.62), (0.9, 0.9)]);
        }
        Icon::Menu => {
            for y in [0.25, 0.5, 0.75] {
                pen.line(&[(0.15, y), (0.85, y)]);
            }
        }
        Icon::Close => {
            pen.line(&[(0.25, 0.25), (0.75, 0.75)]);
            pen.line(&[(0.75, 0.25), (0.25, 0.75)]);
        }
        Icon::Plus => {
            pen.line(&[(0.5, 0.2), (0.5, 0.8)]);
            pen.line(&[(0.2, 0.5), (0.8, 0.5)]);
        }
        Icon::VisualStyle => {
            painter.circle_filled(pen.at(0.5, 0.5), 0.4 * rect.width(), soft);
            pen.circle((0.5, 0.5), 0.4);
            pen.arc((0.5, 0.5), 0.4, 0.14, 0.0, PI);
        }
        Icon::Window => {
            pen.closed(&[(0.1, 0.15), (0.9, 0.15), (0.9, 0.85), (0.1, 0.85)]);
            pen.line(&[(0.1, 0.3), (0.9, 0.3)]);
            pen.line(&[(0.35, 0.3), (0.35, 0.85)]);
        }
        Icon::PreviousView => {
            pen.line(&[(0.85, 0.5), (0.15, 0.5)]);
            pen.arrow_head((0.15, 0.5), (0.5, 0.5));
            pen.closed(&[(0.55, 0.25), (0.85, 0.25), (0.85, 0.75), (0.55, 0.75)]);
        }
        Icon::Projection => {
            pen.closed(&[(0.1, 0.25), (0.45, 0.25), (0.45, 0.75), (0.1, 0.75)]);
            pen.closed(&[(0.6, 0.35), (0.9, 0.2), (0.9, 0.8), (0.6, 0.65)]);
        }
        Icon::Delete => {
            pen.closed(&[(0.25, 0.3), (0.75, 0.3), (0.7, 0.9), (0.3, 0.9)]);
            pen.line(&[(0.15, 0.3), (0.85, 0.3)]);
            pen.line(&[(0.4, 0.3), (0.42, 0.15), (0.58, 0.15), (0.6, 0.3)]);
        }
        Icon::Assembly => {
            // Two blocks, one standing on the other.
            pen.fill(&[(0.08, 0.6), (0.62, 0.6), (0.62, 0.9), (0.08, 0.9)], soft);
            pen.closed(&[(0.08, 0.6), (0.62, 0.6), (0.62, 0.9), (0.08, 0.9)]);
            pen.closed(&[(0.42, 0.18), (0.92, 0.18), (0.92, 0.6), (0.42, 0.6)]);
        }
        Icon::Place => {
            pen.fill(&[(0.1, 0.5), (0.55, 0.5), (0.55, 0.9), (0.1, 0.9)], soft);
            pen.closed(&[(0.1, 0.5), (0.55, 0.5), (0.55, 0.9), (0.1, 0.9)]);
            pen.line(&[(0.75, 0.1), (0.75, 0.45)]);
            pen.arrow_head((0.75, 0.48), (0.75, 0.1));
            pen.line(&[(0.62, 0.55), (0.9, 0.55)]);
        }
        Icon::CreateComponent => {
            pen.fill(&[(0.1, 0.4), (0.6, 0.4), (0.6, 0.9), (0.1, 0.9)], soft);
            pen.closed(&[(0.1, 0.4), (0.6, 0.4), (0.6, 0.9), (0.1, 0.9)]);
            pen.line(&[(0.78, 0.08), (0.78, 0.4)]);
            pen.line(&[(0.62, 0.24), (0.94, 0.24)]);
        }
        Icon::Ground => {
            // A pin pushed into a base line.
            pen.circle((0.5, 0.25), 0.15);
            pen.line(&[(0.5, 0.4), (0.5, 0.75)]);
            pen.line(&[(0.15, 0.8), (0.85, 0.8)]);
            for i in 0..4 {
                let x = 0.2 + i as f32 * 0.18;
                pen.line(&[(x, 0.8), (x - 0.08, 0.94)]);
            }
        }
        Icon::Joint => {
            // Two links turning about a shared pin.
            pen.closed(&[(0.08, 0.62), (0.5, 0.62), (0.5, 0.86), (0.08, 0.86)]);
            pen.closed(&[(0.42, 0.66), (0.86, 0.14), (0.96, 0.26), (0.54, 0.8)]);
            pen.circle((0.48, 0.73), 0.07);
        }
        Icon::Constrain => {
            // Two faces brought together, arrows meeting.
            pen.fill(&[(0.08, 0.1), (0.36, 0.1), (0.36, 0.9), (0.08, 0.9)], soft);
            pen.closed(&[(0.08, 0.1), (0.36, 0.1), (0.36, 0.9), (0.08, 0.9)]);
            pen.closed(&[(0.64, 0.1), (0.92, 0.1), (0.92, 0.9), (0.64, 0.9)]);
            pen.line(&[(0.4, 0.5), (0.6, 0.5)]);
            pen.arrow_head((0.4, 0.5), (0.6, 0.5));
            pen.arrow_head((0.6, 0.5), (0.4, 0.5));
        }
        Icon::Explode => {
            pen.fill(&[(0.32, 0.38), (0.68, 0.38), (0.68, 0.62), (0.32, 0.62)], soft);
            pen.closed(&[(0.32, 0.38), (0.68, 0.38), (0.68, 0.62), (0.32, 0.62)]);
            for (a, b) in [((0.3, 0.32), (0.08, 0.1)), ((0.7, 0.32), (0.92, 0.1)), ((0.3, 0.68), (0.08, 0.9)), ((0.7, 0.68), (0.92, 0.9))] {
                pen.line(&[a, b]);
                pen.arrow_head(b, a);
            }
        }
        Icon::Interference => {
            let red = Color32::from_rgb(0xd8, 0x44, 0x38);
            pen.closed(&[(0.08, 0.3), (0.6, 0.3), (0.6, 0.85), (0.08, 0.85)]);
            pen.closed(&[(0.4, 0.12), (0.92, 0.12), (0.92, 0.66), (0.4, 0.66)]);
            pen.fill(&[(0.4, 0.3), (0.6, 0.3), (0.6, 0.66), (0.4, 0.66)], red);
        }
        Icon::Bom => {
            pen.closed(&[(0.12, 0.08), (0.88, 0.08), (0.88, 0.92), (0.12, 0.92)]);
            for y in [0.3, 0.5, 0.7] {
                pen.line(&[(0.12, y), (0.88, y)]);
            }
            pen.line(&[(0.32, 0.08), (0.32, 0.92)]);
        }
        Icon::Dof => {
            // A sliding arrow and a turning arrow.
            pen.line(&[(0.1, 0.3), (0.9, 0.3)]);
            pen.arrow_head((0.1, 0.3), (0.5, 0.3));
            pen.arrow_head((0.9, 0.3), (0.5, 0.3));
            pen.arc((0.5, 0.7), 0.32, 0.14, PI * 0.1, PI * 1.6);
            pen.arrow_head((0.5 + 0.32 * (PI * 1.6).cos(), 0.7 + 0.14 * (PI * 1.6).sin()), (0.4, 0.55));
        }
        Icon::Return => {
            pen.line(&[(0.85, 0.2), (0.85, 0.6), (0.2, 0.6)]);
            pen.arrow_head((0.15, 0.6), (0.6, 0.6));
        }
        Icon::Drawing => {
            // A sheet with a folded corner and a title strip.
            pen.fill(&[(0.14, 0.08), (0.66, 0.08), (0.86, 0.28), (0.86, 0.92), (0.14, 0.92)], soft);
            pen.closed(&[(0.14, 0.08), (0.66, 0.08), (0.86, 0.28), (0.86, 0.92), (0.14, 0.92)]);
            pen.line(&[(0.66, 0.08), (0.66, 0.28), (0.86, 0.28)]);
            pen.line(&[(0.48, 0.78), (0.86, 0.78)]);
            pen.line(&[(0.48, 0.78), (0.48, 0.92)]);
        }
        Icon::BaseView => {
            // A sheet with one view on it.
            pen.closed(&[(0.08, 0.12), (0.92, 0.12), (0.92, 0.88), (0.08, 0.88)]);
            pen.fill(&[(0.24, 0.36), (0.6, 0.36), (0.6, 0.7), (0.24, 0.7)], soft);
            pen.closed(&[(0.24, 0.36), (0.6, 0.36), (0.6, 0.7), (0.24, 0.7)]);
        }
        Icon::ProjectedView => {
            // A view and one projected beside it.
            pen.fill(&[(0.08, 0.5), (0.45, 0.5), (0.45, 0.9), (0.08, 0.9)], soft);
            pen.closed(&[(0.08, 0.5), (0.45, 0.5), (0.45, 0.9), (0.08, 0.9)]);
            pen.closed(&[(0.6, 0.5), (0.92, 0.5), (0.92, 0.9), (0.6, 0.9)]);
            pen.closed(&[(0.08, 0.1), (0.45, 0.1), (0.45, 0.36), (0.08, 0.36)]);
            pen.line(&[(0.48, 0.7), (0.58, 0.7)]);
            pen.arrow_head((0.6, 0.7), (0.48, 0.7));
        }
        Icon::SectionView => {
            // A cut view, hatched, and its cutting line.
            pen.closed(&[(0.12, 0.32), (0.88, 0.32), (0.88, 0.78), (0.12, 0.78)]);
            for i in 0..4 {
                let x = 0.2 + 0.18 * i as f32;
                pen.line(&[(x, 0.76), (x + 0.16, 0.34)]);
            }
            pen.line(&[(0.04, 0.16), (0.96, 0.16)]);
            pen.arrow_head((0.06, 0.04), (0.06, 0.16));
            pen.arrow_head((0.94, 0.04), (0.94, 0.16));
        }
        Icon::DetailView => {
            // A circle on a corner, and the corner larger.
            pen.closed(&[(0.06, 0.4), (0.4, 0.4), (0.4, 0.9), (0.06, 0.9)]);
            pen.circle((0.36, 0.44), 0.12);
            pen.fill(&[(0.56, 0.1), (0.94, 0.1), (0.94, 0.62), (0.56, 0.62)], soft);
            pen.circle((0.75, 0.36), 0.22);
            pen.line(&[(0.75, 0.36), (0.94, 0.36)]);
            pen.line(&[(0.75, 0.36), (0.75, 0.58)]);
        }
        Icon::NewSheet => {
            pen.closed(&[(0.12, 0.2), (0.7, 0.2), (0.7, 0.92), (0.12, 0.92)]);
            pen.line(&[(0.3, 0.08), (0.88, 0.08), (0.88, 0.8)]);
            pen.line(&[(0.41, 0.42), (0.41, 0.7)]);
            pen.line(&[(0.27, 0.56), (0.55, 0.56)]);
        }
        Icon::Balloon => {
            pen.circle((0.62, 0.32), 0.22);
            pen.line(&[(0.46, 0.48), (0.14, 0.86)]);
            pen.dot((0.14, 0.86), 0.06);
            pen.line(&[(0.62, 0.22), (0.62, 0.42)]);
        }
        Icon::HoleTable => {
            pen.circle((0.24, 0.24), 0.12);
            pen.line(&[(0.24, 0.06), (0.24, 0.42)]);
            pen.line(&[(0.06, 0.24), (0.42, 0.24)]);
            pen.closed(&[(0.5, 0.12), (0.92, 0.12), (0.92, 0.9), (0.5, 0.9)]);
            pen.closed(&[(0.08, 0.52), (0.5, 0.52), (0.5, 0.9), (0.08, 0.9)]);
            for y in [0.38, 0.64] {
                pen.line(&[(0.5, y), (0.92, y)]);
            }
            pen.line(&[(0.08, 0.71), (0.5, 0.71)]);
        }
        Icon::CenterMark => {
            pen.circle((0.5, 0.5), 0.28);
            pen.line(&[(0.42, 0.5), (0.58, 0.5)]);
            pen.line(&[(0.5, 0.42), (0.5, 0.58)]);
            for (a, b) in [(0.06, 0.34), (0.66, 0.94)] {
                pen.line(&[(a, 0.5), (b, 0.5)]);
                pen.line(&[(0.5, a), (0.5, b)]);
            }
        }
        Icon::Centerline => {
            pen.circle((0.24, 0.76), 0.17);
            pen.circle((0.76, 0.24), 0.17);
            // A chain line through both centres: long dash, short dash, long dash.
            for (a, b) in [(0.02, 0.36), (0.45, 0.55), (0.64, 0.98)] {
                pen.line(&[(a, 1.0 - a), (b, 1.0 - b)]);
            }
        }
        Icon::CenterlineBisector => {
            pen.line(&[(0.06, 0.16), (0.94, 0.16)]);
            pen.line(&[(0.06, 0.84), (0.94, 0.84)]);
            for (a, b) in [(0.0, 0.34), (0.44, 0.56), (0.66, 1.0)] {
                pen.line(&[(a, 0.5), (b, 0.5)]);
            }
        }
    }
}

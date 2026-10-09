//! Sheet sizes and the built-in title block (Tenon's own layout) for each standard.

use tenon_geom::Vec2;

use crate::model::{SheetSize, Standard, TitleBlock, TitleField, TitleLine};

/// Margin between the sheet edge and the border (mm).
pub const BORDER: f64 = 10.0;

fn size(name: &str, width: f64, height: f64) -> SheetSize {
    SheetSize { name: name.into(), width, height }
}

/// The sheet sizes of a standard, landscape, smallest first.
pub fn sizes(standard: Standard) -> Vec<SheetSize> {
    match standard {
        Standard::Ansi => vec![size("A", 279.4, 215.9), size("B", 431.8, 279.4), size("C", 558.8, 431.8), size("D", 863.6, 558.8)],
        Standard::Iso => vec![size("A4", 297.0, 210.0), size("A3", 420.0, 297.0), size("A2", 594.0, 420.0), size("A1", 841.0, 594.0)],
    }
}

/// The size new drawings start on: ANSI B or ISO A3.
pub fn default_size(standard: Standard) -> SheetSize {
    sizes(standard).into_iter().nth(1).unwrap_or_else(|| size("B", 431.8, 279.4))
}

/// A size by name (`"B"`, `"A3"`, ...) in either standard.
pub fn size_named(name: &str) -> Option<SheetSize> {
    sizes(Standard::Ansi).into_iter().chain(sizes(Standard::Iso)).find(|s| s.name.eq_ignore_ascii_case(name))
}

/// The title block: 170 x 40 mm in the bottom-right corner, inside the border.
pub fn title_block(standard: Standard, _size: &SheetSize) -> TitleBlock {
    let (x0, x1, y0, y1) = (-(BORDER + 170.0), -BORDER, BORDER, BORDER + 40.0);
    let line = |ax: f64, ay: f64, bx: f64, by: f64| TitleLine { a: Vec2::new(ax, ay), b: Vec2::new(bx, by) };
    let mut lines = vec![line(x0, y0, x1, y0), line(x1, y0, x1, y1), line(x1, y1, x0, y1), line(x0, y1, x0, y0)];
    // Rows: 10..26 (size, scale, sheet, units, projection), 26..38 (drawn, date, number,
    // revision), 38..50 (company, title).
    let (r1, r2) = (y0 + 16.0, y0 + 28.0);
    lines.push(line(x0, r1, x1, r1));
    lines.push(line(x0, r2, x1, r2));
    for (x, from, to) in [
        (x0 + 80.0, r2, y1),
        (x0 + 40.0, r1, r2),
        (x0 + 80.0, r1, r2),
        (x1 - 25.0, r1, r2),
        (x0 + 20.0, y0, r1),
        (x0 + 60.0, y0, r1),
        (x0 + 100.0, y0, r1),
        (x0 + 125.0, y0, r1),
    ] {
        lines.push(line(x, from, x, to));
    }
    let field =
        |key: &str, label: &str, x: f64, y: f64, height: f64| TitleField { key: key.into(), label: label.into(), at: Vec2::new(x, y), height };
    let fields = vec![
        field("company", "COMPANY", x0 + 2.0, r2 + 2.0, 3.5),
        field("title", "TITLE", x0 + 82.0, r2 + 2.0, 5.0),
        field("drawn_by", "DRAWN", x0 + 2.0, r1 + 2.0, 3.0),
        field("date", "DATE", x0 + 42.0, r1 + 2.0, 3.0),
        field("number", "DWG NO", x0 + 82.0, r1 + 2.0, 3.5),
        field("revision", "REV", x1 - 23.0, r1 + 2.0, 3.5),
        field("size", "SIZE", x0 + 2.0, y0 + 2.0, 3.5),
        field("scale", "SCALE", x0 + 22.0, y0 + 2.0, 3.5),
        field("sheet", "SHEET", x0 + 62.0, y0 + 2.0, 3.5),
        field("units", "UNITS", x0 + 102.0, y0 + 2.0, 3.5),
    ];
    TitleBlock {
        name: match standard {
            Standard::Ansi => "ANSI".into(),
            Standard::Iso => "ISO".into(),
        },
        lines,
        fields,
        projection_symbol: Some(Vec2::new(x0 + 147.5, y0 + 8.0)),
    }
}

/// The projection symbol (a truncated cone in two views) centred at `c`: lines, and the two
/// circles as (centre, radius). Third-angle puts the end view to the right of the side view;
/// first-angle to the left.
pub fn projection_symbol(c: Vec2, third_angle: bool) -> (Vec<[Vec2; 2]>, Vec<(Vec2, f64)>) {
    let (big, small, len) = (4.0, 2.4, 9.0);
    let side = if third_angle { -6.5 } else { 6.5 };
    let s = Vec2::new(c.x + side, c.y);
    let (l, r) = (s.x - len / 2.0, s.x + len / 2.0);
    // Side view: wide end at the left, narrowing to the right.
    let lines = vec![
        [Vec2::new(l, s.y - big), Vec2::new(l, s.y + big)],
        [Vec2::new(l, s.y + big), Vec2::new(r, s.y + small)],
        [Vec2::new(r, s.y + small), Vec2::new(r, s.y - small)],
        [Vec2::new(r, s.y - small), Vec2::new(l, s.y - big)],
    ];
    let e = Vec2::new(c.x - side, c.y);
    (lines, vec![(e, big), (e, small)])
}

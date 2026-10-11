//! A flat pattern as files for whoever cuts it: DXF for a laser, plasma or waterjet cutter and
//! its software, SVG to look at or print.
//!
//! The DXF is R12, the version every cutting program reads, in millimetres, with the pattern's
//! lower left corner at the origin. What is cut and what is not are on separate layers, so that
//! bend lines can be turned off, etched or left as they are:
//!
//! | Layer | Holds | Colour |
//! |---|---|---|
//! | `OUTER` | the outline | 7 (black or white) |
//! | `INTERIOR` | holes and cut-outs | 4 (cyan) |
//! | `BEND_UP` | centre lines of bends towards the viewer | 3 (green) |
//! | `BEND_DOWN` | centre lines of bends away from the viewer | 1 (red) |
//! | `BEND_EXTENT` | where each bend begins and ends | 8 (grey) |

use std::f64::consts::TAU;

use serde_json::{Value, json};
use tenon_dxf::Tag;
use tenon_geom::Vec2;
use tenon_kernel::Kernel;
use tenon_model::sheet::{FlatCurve, FlatLoop, FlatPattern};
use tenon_model::{CmdError, CmdResult, Session};

pub const OUTER: &str = "OUTER";
pub const INTERIOR: &str = "INTERIOR";
pub const BEND_UP: &str = "BEND_UP";
pub const BEND_DOWN: &str = "BEND_DOWN";
pub const BEND_EXTENT: &str = "BEND_EXTENT";

/// The layers of a flat pattern's DXF: name, colour, line type.
pub const LAYERS: [(&str, i64, &str); 5] =
    [(OUTER, 7, "CONTINUOUS"), (INTERIOR, 4, "CONTINUOUS"), (BEND_UP, 3, "CENTER"), (BEND_DOWN, 1, "CENTER"), (BEND_EXTENT, 8, "CONTINUOUS")];

/// What goes into the file besides what is cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marks {
    /// The centre line of every bend.
    pub bends: bool,
    /// Where every bend begins and ends.
    pub extents: bool,
}

impl Default for Marks {
    fn default() -> Self {
        Marks { bends: true, extents: true }
    }
}

/// What a written flat pattern holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub lines: usize,
    pub arcs: usize,
    pub circles: usize,
    pub polylines: usize,
}

/// A number as the file has it: to a millionth (a nanometre of a length), so that the file is
/// the same whatever machine wrote it and no longer than it need be.
fn real(code: i32, v: f64) -> Tag {
    let r = (v * 1e6).round() / 1e6;
    Tag::f(code, if r == 0.0 { 0.0 } else { r })
}

fn curve_tags(t: &mut Vec<Tag>, c: &FlatCurve, layer: &str, n: &mut Counts) {
    let (s, f, i) = (|code: i32, v: &str| Tag::s(code, v), real, Tag::i);
    match c {
        FlatCurve::Line { start, end } => {
            n.lines += 1;
            t.extend([s(0, "LINE"), s(8, layer), f(10, start.x), f(20, start.y), f(30, 0.0), f(11, end.x), f(21, end.y), f(31, 0.0)]);
        }
        FlatCurve::Arc { center, radius, start, end, ccw } => {
            n.arcs += 1;
            // A DXF arc runs counter-clockwise from its first angle to its second.
            let (from, to) = if *ccw { (start, end) } else { (end, start) };
            let degrees = |p: &Vec2| (*p - *center).angle().rem_euclid(TAU).to_degrees();
            t.extend([
                s(0, "ARC"),
                s(8, layer),
                f(10, center.x),
                f(20, center.y),
                f(30, 0.0),
                f(40, *radius),
                f(50, degrees(from)),
                f(51, degrees(to)),
            ]);
        }
        FlatCurve::Circle { center, radius } => {
            n.circles += 1;
            t.extend([s(0, "CIRCLE"), s(8, layer), f(10, center.x), f(20, center.y), f(30, 0.0), f(40, *radius)]);
        }
        FlatCurve::Polyline { points } => {
            n.polylines += 1;
            t.extend([s(0, "POLYLINE"), s(8, layer), i(66, 1), f(10, 0.0), f(20, 0.0), f(30, 0.0), i(70, 0)]);
            for p in points {
                t.extend([s(0, "VERTEX"), s(8, layer), f(10, p.x), f(20, p.y), f(30, 0.0)]);
            }
            t.extend([s(0, "SEQEND"), s(8, layer)]);
        }
    }
}

/// The pattern as a DXF file (R12, millimetres), and what was written of its outlines.
pub fn dxf(flat: &FlatPattern, marks: Marks) -> (String, Counts) {
    let (s, f, i) = (|code: i32, v: &str| Tag::s(code, v), real, Tag::i);
    let mut t: Vec<Tag> = Vec::new();
    t.extend([s(0, "SECTION"), s(2, "HEADER"), s(9, "$ACADVER"), s(1, "AC1009"), s(9, "$INSUNITS"), i(70, 4), s(9, "$MEASUREMENT"), i(70, 1)]);
    t.extend([s(9, "$EXTMIN"), f(10, 0.0), f(20, 0.0), f(30, 0.0), s(9, "$EXTMAX"), f(10, flat.length), f(20, flat.width), f(30, 0.0)]);
    t.extend([s(9, "$LIMMIN"), f(10, 0.0), f(20, 0.0), s(9, "$LIMMAX"), f(10, flat.length), f(20, flat.width), s(0, "ENDSEC")]);
    t.extend([s(0, "SECTION"), s(2, "TABLES"), s(0, "TABLE"), s(2, "LTYPE"), i(70, 2)]);
    t.extend([s(0, "LTYPE"), s(2, "CONTINUOUS"), i(70, 0), s(3, "Solid line"), i(72, 65), i(73, 0), f(40, 0.0)]);
    // Long, gap, short, gap: a centre line.
    let center = [12.0, -3.0, 3.0, -3.0];
    t.extend([s(0, "LTYPE"), s(2, "CENTER"), i(70, 0), s(3, "Center ____ _ ____"), i(72, 65), i(73, 4), f(40, 21.0)]);
    t.extend(center.iter().map(|d| f(49, *d)));
    t.extend([s(0, "ENDTAB"), s(0, "TABLE"), s(2, "LAYER"), i(70, LAYERS.len() as i64)]);
    for (name, color, ltype) in LAYERS {
        t.extend([s(0, "LAYER"), s(2, name), i(70, 0), i(62, color), s(6, ltype)]);
    }
    t.extend([s(0, "ENDTAB"), s(0, "ENDSEC"), s(0, "SECTION"), s(2, "ENTITIES")]);
    let mut n = Counts::default();
    for c in &flat.outer.curves {
        curve_tags(&mut t, c, OUTER, &mut n);
    }
    for c in flat.inner.iter().flat_map(|l| &l.curves) {
        curve_tags(&mut t, c, INTERIOR, &mut n);
    }
    let mut marked = Counts::default();
    for b in &flat.bends {
        let mut seg = |l: &[Vec2; 2], layer: &str| curve_tags(&mut t, &FlatCurve::Line { start: l[0], end: l[1] }, layer, &mut marked);
        if marks.bends {
            b.lines.iter().for_each(|l| seg(l, if b.up { BEND_UP } else { BEND_DOWN }));
        }
        if marks.extents {
            b.extents.iter().for_each(|l| seg(l, BEND_EXTENT));
        }
    }
    t.extend([s(0, "ENDSEC"), s(0, "EOF")]);
    (tenon_dxf::write_ascii(&t), n)
}

fn svg_path(l: &FlatLoop, height: f64) -> String {
    // SVG's Y runs down the page: the pattern is turned over top to bottom, which also turns
    // its arcs the other way round.
    let p = |v: &Vec2| format!("{:.4} {:.4}", v.x, height - v.y);
    let mut d = String::new();
    for (i, c) in l.curves.iter().enumerate() {
        if let (0, Some((start, _))) = (i, c.ends()) {
            d.push_str(&format!("M{} ", p(&start)));
        }
        match c {
            FlatCurve::Line { end, .. } => d.push_str(&format!("L{} ", p(end))),
            FlatCurve::Arc { radius, end, ccw, .. } => {
                let large = u8::from(c.sweep().abs() > std::f64::consts::PI);
                d.push_str(&format!("A{radius:.4} {radius:.4} 0 {large} {} {} ", u8::from(!ccw), p(end)));
            }
            FlatCurve::Circle { center, radius } => {
                let (a, b) = (*center - Vec2::new(*radius, 0.0), *center + Vec2::new(*radius, 0.0));
                d.push_str(&format!("M{} A{radius:.4} {radius:.4} 0 1 0 {} A{radius:.4} {radius:.4} 0 1 0 {} ", p(&a), p(&b), p(&a)));
            }
            FlatCurve::Polyline { points } => points.iter().skip(1).for_each(|q| d.push_str(&format!("L{} ", p(q)))),
        }
    }
    d.push('Z');
    d
}

/// The pattern as an SVG picture at full size (a millimetre is a millimetre): the sheet filled,
/// bend lines dash-dotted (green up, red down), bend extents thin.
pub fn svg(flat: &FlatPattern, marks: Marks) -> String {
    let (w, h) = (flat.length, flat.width);
    let margin = 5.0;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.3}mm\" height=\"{:.3}mm\" viewBox=\"{:.3} {:.3} {:.3} {:.3}\">\n",
        w + 2.0 * margin,
        h + 2.0 * margin,
        -margin,
        -margin,
        w + 2.0 * margin,
        h + 2.0 * margin
    );
    let d: Vec<String> = flat.loops().map(|l| svg_path(l, h)).collect();
    out.push_str(&format!("  <path d=\"{}\" fill=\"#c8ced6\" fill-rule=\"evenodd\" stroke=\"#1a1d21\" stroke-width=\"0.25\"/>\n", d.join(" ")));
    let mut line = |l: &[Vec2; 2], style: &str| {
        out.push_str(&format!("  <line x1=\"{:.4}\" y1=\"{:.4}\" x2=\"{:.4}\" y2=\"{:.4}\" {style}/>\n", l[0].x, h - l[0].y, l[1].x, h - l[1].y));
    };
    for b in &flat.bends {
        if marks.extents {
            b.extents.iter().for_each(|l| line(l, "stroke=\"#7a828c\" stroke-width=\"0.12\""));
        }
        if marks.bends {
            let color = if b.up { "#1a8a3a" } else { "#c0392b" };
            b.lines.iter().for_each(|l| line(l, &format!("stroke=\"{color}\" stroke-width=\"0.2\" stroke-dasharray=\"4 1 1 1\"")));
        }
    }
    out.push_str("</svg>\n");
    out
}

fn marks(p: &Value) -> Marks {
    let flag = |key: &str| p.get(key).and_then(Value::as_bool).unwrap_or(true);
    Marks { bends: flag("bends"), extents: flag("extents") }
}

fn written(path: &std::path::Path, data: &str, flat: &FlatPattern) -> CmdResult {
    std::fs::write(path, data).map_err(|e| CmdError(format!("cannot write {}: {e}", path.display())))?;
    Ok(json!({
        "path": path.display().to_string(),
        "bytes": data.len(),
        "length": flat.length,
        "width": flat.width,
        "area": flat.area,
        "cut_length": flat.cut_length,
        "holes": flat.inner.len(),
        "bends": flat.bends.len(),
    }))
}

pub(crate) fn export_dxf(s: &mut Session, k: &mut dyn Kernel, p: &Value) -> CmdResult {
    let path = crate::cmd::path(p)?;
    let flat = tenon_model::cmd::flat_pattern(s, k, p)?;
    let (data, n) = dxf(&flat, marks(p));
    let mut v = written(&path, &data, &flat)?;
    v["entities"] = json!({ "lines": n.lines, "arcs": n.arcs, "circles": n.circles, "polylines": n.polylines });
    Ok(v)
}

pub(crate) fn export_svg(s: &mut Session, k: &mut dyn Kernel, p: &Value) -> CmdResult {
    let path = crate::cmd::path(p)?;
    let flat = tenon_model::cmd::flat_pattern(s, k, p)?;
    written(&path, &svg(&flat, marks(p)), &flat)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use tenon_model::sheet::Bend;
    use tenon_model::sheet::flat::chain;

    /// A 60 x 40 sheet with a slot (two lines and two half circles), a round hole, and a bend
    /// up along x = 20 and one down along x = 45, each 6 wide.
    fn pattern() -> FlatPattern {
        let line = |a: (f64, f64), b: (f64, f64)| FlatCurve::Line { start: Vec2::new(a.0, a.1), end: Vec2::new(b.0, b.1) };
        let half = |c: (f64, f64), from: (f64, f64), to: (f64, f64)| FlatCurve::Arc {
            center: Vec2::new(c.0, c.1),
            radius: 3.0,
            start: Vec2::new(from.0, from.1),
            end: Vec2::new(to.0, to.1),
            ccw: true,
        };
        let (outer, inner) = chain(vec![
            line((0.0, 0.0), (60.0, 0.0)),
            line((60.0, 0.0), (60.0, 40.0)),
            line((60.0, 40.0), (0.0, 40.0)),
            line((0.0, 40.0), (0.0, 0.0)),
            line((5.0, 7.0), (12.0, 7.0)),
            half((12.0, 10.0), (12.0, 7.0), (12.0, 13.0)),
            line((12.0, 13.0), (5.0, 13.0)),
            half((5.0, 10.0), (5.0, 13.0), (5.0, 7.0)),
            FlatCurve::Circle { center: Vec2::new(52.0, 30.0), radius: 4.0 },
        ])
        .unwrap();
        let bend = |x: f64, up: bool| Bend {
            lines: vec![[Vec2::new(x, 0.0), Vec2::new(x, 40.0)]],
            extents: vec![[Vec2::new(x - 3.0, 0.0), Vec2::new(x - 3.0, 40.0)], [Vec2::new(x + 3.0, 0.0), Vec2::new(x + 3.0, 40.0)]],
            angle: std::f64::consts::FRAC_PI_2,
            radius: 3.0,
            up,
            allowance: 6.0,
            face: 0,
        };
        let area = 2400.0 - (7.0 * 6.0 + 9.0 * std::f64::consts::PI) - 16.0 * std::f64::consts::PI;
        FlatPattern {
            outer,
            inner,
            bends: vec![bend(20.0, true), bend(45.0, false)],
            thickness: 2.0,
            k_factor: 0.44,
            length: 60.0,
            width: 40.0,
            area,
            cut_length: 0.0,
        }
    }

    /// Reads the cut layers of a DXF back as curves, as a cutting program would.
    fn read_back(text: &str) -> (Vec<FlatCurve>, Vec<(String, String)>) {
        let tags = tenon_dxf::parse(text.as_bytes()).unwrap();
        let sections = tenon_dxf::sections(&tags);
        assert_eq!(sections.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["HEADER", "TABLES", "ENTITIES"]);
        let mut curves = Vec::new();
        let mut on: Vec<(String, String)> = Vec::new();
        for (kind, r) in tenon_dxf::records(&sections[2].tags) {
            let num = |code: i32| r.iter().find(|t| t.code == code).map(Tag::f64).unwrap();
            let layer = r.iter().find(|t| t.code == 8).map(Tag::str).unwrap();
            on.push((kind.clone(), layer.clone()));
            if layer != OUTER && layer != INTERIOR {
                continue;
            }
            match kind.as_str() {
                "LINE" => curves.push(FlatCurve::Line { start: Vec2::new(num(10), num(20)), end: Vec2::new(num(11), num(21)) }),
                "CIRCLE" => curves.push(FlatCurve::Circle { center: Vec2::new(num(10), num(20)), radius: num(40) }),
                "ARC" => {
                    let (c, r) = (Vec2::new(num(10), num(20)), num(40));
                    curves.push(FlatCurve::Arc {
                        center: c,
                        radius: r,
                        start: Vec2::polar(c, r, num(50).to_radians()),
                        end: Vec2::polar(c, r, num(51).to_radians()),
                        ccw: true,
                    });
                }
                other => panic!("an unexpected {other}"),
            }
        }
        (curves, on)
    }

    #[test]
    fn the_dxf_holds_the_outline_holes_and_bend_lines_on_their_layers() {
        let flat = pattern();
        let (text, n) = dxf(&flat, Marks::default());
        assert_eq!(n, Counts { lines: 6, arcs: 2, circles: 1, polylines: 0 });
        assert!(text.contains("AC1009") && text.contains("$INSUNITS"), "R12, in millimetres");
        // Read back and put together again, it is the same sheet: the same area to the last
        // digits a file keeps, with its two holes. (The clockwise half circles of the slot are
        // written the way DXF has arcs, counter-clockwise.)
        let (curves, on) = read_back(&text);
        let (outer, inner) = chain(curves).unwrap();
        let area = outer.signed_area() - inner.iter().map(|l| l.signed_area().abs()).sum::<f64>();
        assert!((area - flat.area).abs() < 1e-6, "{area} {}", flat.area);
        assert_eq!(inner.len(), 2);
        let count = |kind: &str, layer: &str| on.iter().filter(|(k, l)| k == kind && l == layer).count();
        assert_eq!((count("LINE", OUTER), count("LINE", INTERIOR), count("ARC", INTERIOR), count("CIRCLE", INTERIOR)), (4, 2, 2, 1));
        assert_eq!((count("LINE", BEND_UP), count("LINE", BEND_DOWN), count("LINE", BEND_EXTENT)), (1, 1, 4));
        // Every layer used is defined, with a line type that is defined.
        let tags = tenon_dxf::parse(text.as_bytes()).unwrap();
        let tables = tenon_dxf::records(&tenon_dxf::sections(&tags)[1].tags);
        let named = |kind: &str| -> Vec<String> {
            tables.iter().filter(|(k, _)| k == kind).filter_map(|(_, r)| r.iter().find(|t| t.code == 2).map(Tag::str)).collect()
        };
        assert!(on.iter().all(|(_, layer)| named("LAYER").contains(layer)), "{on:?}");
        for (_, r) in tables.iter().filter(|(k, _)| k == "LAYER") {
            assert!(named("LTYPE").contains(&r.iter().find(|t| t.code == 6).map(Tag::str).unwrap()));
        }
        // Without the marks only what is cut is written.
        let (bare, _) = dxf(&flat, Marks { bends: false, extents: false });
        assert!(read_back(&bare).1.iter().all(|(_, layer)| layer == OUTER || layer == INTERIOR));
    }

    #[test]
    fn the_svg_is_the_sheet_at_full_size_with_its_bend_lines() {
        let flat = pattern();
        let text = svg(&flat, Marks::default());
        assert!(
            text.starts_with(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"70.000mm\" height=\"50.000mm\" viewBox=\"-5.000 -5.000 70.000 50.000\">"
            ),
            "{text}"
        );
        // One path for the sheet (its outline and its holes, so that the holes show through),
        // two centre lines and four extent lines.
        assert_eq!((text.matches("<path").count(), text.matches("fill-rule=\"evenodd\"").count(), text.matches("<line").count()), (1, 1, 6));
        assert_eq!((text.matches("#1a8a3a").count(), text.matches("#c0392b").count()), (1, 1), "one bend up, one down");
        assert_eq!(text.matches('Z').count(), 3, "the outline, the slot and the hole are each closed");
        assert!(svg(&flat, Marks { bends: false, extents: false }).matches("<line").count() == 0);
    }
}

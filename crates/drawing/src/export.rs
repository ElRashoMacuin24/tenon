//! Writing sheets: SVG, PDF (one page per sheet) and DXF. All three come from the same
//! [`Graphics`], so they match the screen:
//!
//! - strokes are cleaned first ([`crate::clean`]): true arcs and circles, polylines without
//!   needless points, no stroke written twice;
//! - text is drawn in the drafting font, as on screen. PDF and SVG also carry the words as
//!   invisible text over it, so they can be searched and selected. DXF writes TEXT entities.
//!
//! [`check_pdf`] and [`check_dxf`] read the files back the way other programs do; tests use
//! them on every output.

use std::f64::consts::{FRAC_PI_2, TAU};
use std::fmt::Write as _;

use tenon_dxf::Tag;
use tenon_geom::Vec2;

use crate::clean::{self, Item};
use crate::graphics::{Align, Graphics, Pen, Text};
use crate::stroke;

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// ---- SVG ----------------------------------------------------------------------------------------

/// One sheet as SVG (millimetres, white paper), a group per pen.
pub fn svg(g: &Graphics) -> String {
    let (w, h) = (g.width, g.height);
    let p = |q: Vec2| format!("{:.3},{:.3}", q.x, h - q.y);
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(s, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="{h}mm" viewBox="0 0 {w} {h}">"#);
    let _ = writeln!(s, r#"<rect width="{w}" height="{h}" fill="white"/>"#);
    let items = clean::items(g);
    for pen in Pen::ALL {
        let mine: Vec<&Item> = items.iter().filter(|(x, _)| *x == pen).map(|(_, i)| i).collect();
        if mine.is_empty() {
            continue;
        }
        let dash = pen.dashes().iter().map(|d| format!("{d}")).collect::<Vec<_>>().join(" ");
        let dash = if dash.is_empty() { String::new() } else { format!(r#" stroke-dasharray="{dash}""#) };
        let _ = writeln!(
            s,
            r#"<g id="{}" fill="none" stroke="black" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round"{dash}>"#,
            pen.name(),
            pen.width()
        );
        for item in mine {
            match item {
                Item::Polyline(pts) => {
                    let _ = writeln!(s, r#"<polyline points="{}"/>"#, pts.iter().map(|q| p(*q)).collect::<Vec<_>>().join(" "));
                }
                Item::Circle { c, r } => {
                    let _ = writeln!(s, r#"<circle cx="{:.3}" cy="{:.3}" r="{r:.4}"/>"#, c.x, h - c.y);
                }
                Item::Arc { c, r, a0, a1 } => {
                    let at = |a: f64| Vec2::new(c.x + r * a.cos(), c.y + r * a.sin());
                    // Counter-clockwise on the sheet is the negative sweep in SVG's y-down space.
                    let large = u8::from(a1 - a0 > std::f64::consts::PI);
                    let _ = writeln!(s, r#"<path d="M {} A {r:.4} {r:.4} 0 {large} 0 {}"/>"#, p(at(*a0)), p(at(*a1)));
                }
            }
        }
        s.push_str("</g>\n");
    }
    s.push_str("<g id=\"ARROWS\" fill=\"black\" stroke=\"none\">\n");
    for (_, pts) in &g.fills {
        let _ = writeln!(s, r#"<polygon points="{}"/>"#, pts.iter().map(|q| p(*q)).collect::<Vec<_>>().join(" "));
    }
    s.push_str("</g>\n<g id=\"TEXT\" fill=\"none\" stroke=\"black\" stroke-linecap=\"round\" stroke-linejoin=\"round\">\n");
    for (_, t) in &g.texts {
        let left = t.left();
        let _ = writeln!(s, r#"<g stroke-width="{:.3}">"#, t.height * stroke::PEN);
        for run in stroke::text_strokes(&t.text, left, t.height) {
            let _ = writeln!(s, r#"<polyline points="{}"/>"#, run.iter().map(|q| p(*q)).collect::<Vec<_>>().join(" "));
        }
        // The words, invisible, over the strokes: for searching and selecting.
        let width = stroke::text_width(&t.text, t.height);
        if width > 0.0 {
            let _ = writeln!(
                s,
                r#"<text x="{:.3}" y="{:.3}" font-family="Helvetica, Arial, sans-serif" font-size="{:.3}" textLength="{width:.3}" lengthAdjust="spacingAndGlyphs" fill="black" fill-opacity="0" stroke="none">{}</text>"#,
                left.x,
                h - left.y,
                t.height / 0.72,
                xml(&t.text)
            );
        }
        s.push_str("</g>\n");
    }
    s.push_str("</g>\n</svg>\n");
    s
}

// ---- PDF ----------------------------------------------------------------------------------------

/// PDF points per millimetre.
const PT: f64 = 72.0 / 25.4;

/// Text for a PDF string in WinAnsi encoding (Helvetica): escapes, and the few non-ASCII
/// characters drawings use.
fn pdf_text(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            'Ø' => out.push_str("\\330"),
            'ø' | '⌀' => out.push_str("\\370"),
            '°' => out.push_str("\\260"),
            '±' => out.push_str("\\261"),
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            _ => out.push('?'),
        }
    }
    out
}

/// Width of text in Helvetica (approximate, average glyph widths), in mm at cap height `h`.
fn helvetica_width(s: &str, h: f64) -> f64 {
    let size = h / 0.72;
    s.chars()
        .map(|c| {
            if c.is_ascii_digit() {
                0.556
            } else if c == ' ' {
                0.278
            } else if c.is_ascii_uppercase() {
                0.667
            } else {
                0.5
            }
        })
        .sum::<f64>()
        * size
}

/// An arc as cubic Béziers (`m` already done at its start), at most a quarter turn each.
fn pdf_arc(c: &mut String, centre: Vec2, r: f64, a0: f64, a1: f64) {
    let pieces = ((a1 - a0) / FRAC_PI_2).ceil().max(1.0) as usize;
    let step = (a1 - a0) / pieces as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan() * r;
    for i in 0..pieces {
        let (s, e) = (a0 + step * i as f64, a0 + step * (i + 1) as f64);
        let (ps, pe) = (centre + Vec2::new(s.cos(), s.sin()) * r, centre + Vec2::new(e.cos(), e.sin()) * r);
        let (c1, c2) = (ps + Vec2::new(-s.sin(), s.cos()) * k, pe - Vec2::new(-e.sin(), e.cos()) * k);
        let _ = writeln!(c, "{:.3} {:.3} {:.3} {:.3} {:.3} {:.3} c", c1.x * PT, c1.y * PT, c2.x * PT, c2.y * PT, pe.x * PT, pe.y * PT);
    }
}

/// One page's drawing operators.
fn pdf_page(g: &Graphics) -> String {
    let mut c = String::from("1 J 1 j 0 0 0 RG 0 0 0 rg\n");
    let mv = |c: &mut String, q: Vec2, op: &str| {
        let _ = writeln!(c, "{:.3} {:.3} {op}", q.x * PT, q.y * PT);
    };
    let items = clean::items(g);
    for pen in Pen::ALL {
        let mine: Vec<&Item> = items.iter().filter(|(x, _)| *x == pen).map(|(_, i)| i).collect();
        if mine.is_empty() {
            continue;
        }
        let dash: Vec<String> = pen.dashes().iter().map(|d| format!("{:.3}", d * PT)).collect();
        let _ = writeln!(c, "{:.3} w [{}] 0 d", pen.width() * PT, dash.join(" "));
        for item in mine {
            match item {
                Item::Polyline(pts) => {
                    for (k, q) in pts.iter().enumerate() {
                        mv(&mut c, *q, if k == 0 { "m" } else { "l" });
                    }
                    c.push_str("S\n");
                }
                Item::Arc { c: centre, r, a0, a1 } => {
                    mv(&mut c, *centre + Vec2::new(a0.cos(), a0.sin()) * *r, "m");
                    pdf_arc(&mut c, *centre, *r, *a0, *a1);
                    c.push_str("S\n");
                }
                Item::Circle { c: centre, r } => {
                    mv(&mut c, *centre + Vec2::new(*r, 0.0), "m");
                    pdf_arc(&mut c, *centre, *r, 0.0, TAU);
                    c.push_str("h S\n");
                }
            }
        }
    }
    c.push_str("[] 0 d\n");
    for (_, pts) in &g.fills {
        for (k, q) in pts.iter().enumerate() {
            mv(&mut c, *q, if k == 0 { "m" } else { "l" });
        }
        c.push_str("h f\n");
    }
    for (_, t) in &g.texts {
        pdf_text_strokes(&mut c, t);
    }
    c
}

/// Text in the drafting font, with the words as invisible Helvetica stretched over it.
fn pdf_text_strokes(c: &mut String, t: &Text) {
    let left = t.left();
    let _ = writeln!(c, "{:.3} w", t.height * stroke::PEN * PT);
    for run in stroke::text_strokes(&t.text, left, t.height) {
        for (k, q) in run.iter().enumerate() {
            let _ = writeln!(c, "{:.3} {:.3} {}", q.x * PT, q.y * PT, if k == 0 { "m" } else { "l" });
        }
        c.push_str("S\n");
    }
    let (ours, theirs) = (stroke::text_width(&t.text, t.height), helvetica_width(&t.text, t.height));
    if ours > 0.0 && theirs > 0.0 {
        let _ = writeln!(
            c,
            "BT 3 Tr /F1 {:.3} Tf {:.2} Tz {:.3} {:.3} Td ({}) Tj ET",
            t.height / 0.72 * PT,
            100.0 * ours / theirs,
            left.x * PT,
            left.y * PT,
            pdf_text(&t.text)
        );
    }
}

/// Sheets as a PDF document, one page each.
pub fn pdf(sheets: &[Graphics]) -> Vec<u8> {
    let mut objects: Vec<String> = Vec::new();
    // 1: catalog, 2: pages, 3: font; then per page: page, contents.
    let n = sheets.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".into());
    objects.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")));
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into());
    for (i, g) in sheets.iter().enumerate() {
        let c = pdf_page(g);
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.3} {:.3}] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            g.width * PT,
            g.height * PT,
            5 + 2 * i
        ));
        // The stream's bytes, then an end of line before `endstream` (not counted in /Length).
        objects.push(format!("<< /Length {} >>\nstream\n{c}\nendstream", c.len()));
    }
    let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    let mut x = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for o in offsets {
        let _ = writeln!(x, "{o:010} 00000 n ");
    }
    let _ = write!(x, "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1);
    out.extend_from_slice(x.as_bytes());
    out
}

/// Reads a PDF's structure back as a reader does: `startxref` leads to the cross-reference
/// table, every entry to its `N 0 obj`, every stream holds `/Length` bytes, and the page tree
/// counts its pages. Returns the page count.
pub fn check_pdf(data: &[u8]) -> Result<usize, String> {
    // One character per byte, so offsets in the text are offsets in the file.
    let text: String = data.iter().map(|&b| if b.is_ascii() { char::from(b) } else { '?' }).collect();
    if !data.starts_with(b"%PDF-") || !text.trim_end().ends_with("%%EOF") {
        return Err("no %PDF- header or %%EOF".into());
    }
    let sx = text.rfind("startxref").ok_or("no startxref")?;
    let xref: usize = text[sx + 9..].split_whitespace().next().and_then(|v| v.parse().ok()).ok_or("bad startxref")?;
    if !data.get(xref..).is_some_and(|d| d.starts_with(b"xref")) {
        return Err(format!("startxref {xref} does not point at the xref table"));
    }
    let mut lines = text[xref..].lines();
    lines.next();
    let header: Vec<usize> = lines.next().ok_or("no xref section")?.split_whitespace().filter_map(|v| v.parse().ok()).collect();
    let [first, count] = header[..] else { return Err("bad xref section header".into()) };
    for i in first..first + count {
        let entry = lines.next().ok_or("xref table ends early")?;
        if entry.len() != 19 {
            return Err(format!("xref entry {i} is {} bytes, not 20 with its end of line", entry.len() + 1));
        }
        if i == 0 {
            continue;
        }
        let offset: usize = entry[..10].parse().map_err(|_| format!("bad xref entry {i}"))?;
        let head = format!("{i} 0 obj");
        if !data.get(offset..).is_some_and(|d| d.starts_with(head.as_bytes())) {
            return Err(format!("xref entry {i} points at byte {offset}, not at `{head}`"));
        }
    }
    let size: usize = text.split("/Size ").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|v| v.parse().ok()).ok_or("no trailer /Size")?;
    if size != first + count {
        return Err(format!("trailer /Size {size}, xref has {}", first + count));
    }
    // Streams: /Length bytes after `stream` and its end of line, then an end of line and `endstream`.
    let mut at = 0;
    while let Some(pos) = text[at..].find("stream\n").map(|p| p + at) {
        if text[..pos].ends_with("end") {
            at = pos + 7;
            continue;
        }
        let dict = &text[text[..pos].rfind("<<").ok_or("a stream without a dictionary")?..pos];
        let len: usize =
            dict.split("/Length ").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|v| v.parse().ok()).ok_or("a stream without /Length")?;
        let end = pos + 7 + len;
        if !data.get(end..).is_some_and(|d| d.starts_with(b"\nendstream")) {
            return Err(format!("the stream at byte {pos} is not {len} bytes long"));
        }
        at = end;
    }
    let pages = text.matches("/Type /Page ").count();
    let declared: usize =
        text.split("/Count ").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|v| v.parse().ok()).ok_or("no page count")?;
    if pages != declared {
        return Err(format!("{pages} pages, the page tree says {declared}"));
    }
    Ok(pages)
}

// ---- DXF ----------------------------------------------------------------------------------------

fn ltype(pen: Pen) -> &'static str {
    match pen {
        Pen::Hidden => "HIDDEN",
        Pen::Center | Pen::Cutting => "CENTER",
        _ => "CONTINUOUS",
    }
}

/// One sheet as DXF (R12, millimetres): a layer per pen; lines, polylines, arcs and circles;
/// filled arrowheads (SOLID) and text.
pub fn dxf(g: &Graphics) -> String {
    let mut t: Vec<Tag> = Vec::new();
    let s = |c: i32, v: &str| Tag::s(c, v);
    let f = Tag::f;
    let i = Tag::i;
    let (w, h) = (g.width, g.height);
    t.extend([s(0, "SECTION"), s(2, "HEADER"), s(9, "$ACADVER"), s(1, "AC1009"), s(9, "$INSUNITS"), i(70, 4), s(9, "$MEASUREMENT"), i(70, 1)]);
    t.extend([s(9, "$EXTMIN"), f(10, 0.0), f(20, 0.0), f(30, 0.0), s(9, "$EXTMAX"), f(10, w), f(20, h), f(30, 0.0)]);
    t.extend([s(9, "$LIMMIN"), f(10, 0.0), f(20, 0.0), s(9, "$LIMMAX"), f(10, w), f(20, h), s(0, "ENDSEC")]);
    t.extend([s(0, "SECTION"), s(2, "TABLES")]);
    t.extend([s(0, "TABLE"), s(2, "LTYPE"), i(70, 3)]);
    for (name, desc, pattern) in [
        ("CONTINUOUS", "Solid line", vec![]),
        ("HIDDEN", "Hidden __ __", Pen::Hidden.dashes().to_vec()),
        ("CENTER", "Center ____ _ ____", Pen::Center.dashes().to_vec()),
    ] {
        t.extend([s(0, "LTYPE"), s(2, name), i(70, 0), s(3, desc), i(72, 65), i(73, pattern.len() as i64), f(40, pattern.iter().sum())]);
        // Dashes positive, gaps negative.
        for (k, p) in pattern.iter().enumerate() {
            t.push(f(49, if k % 2 == 0 { *p } else { -*p }));
        }
    }
    t.push(s(0, "ENDTAB"));
    t.extend([s(0, "TABLE"), s(2, "LAYER"), i(70, (Pen::ALL.len() + 1) as i64)]);
    for pen in Pen::ALL {
        t.extend([s(0, "LAYER"), s(2, pen.name()), i(70, 0), i(62, 7), s(6, ltype(pen))]);
    }
    t.extend([s(0, "LAYER"), s(2, "TEXT"), i(70, 0), i(62, 7), s(6, "CONTINUOUS")]);
    t.extend([s(0, "ENDTAB"), s(0, "ENDSEC")]);
    t.extend([s(0, "SECTION"), s(2, "ENTITIES")]);
    for (pen, item) in clean::items(g) {
        let layer = pen.name();
        match item {
            Item::Polyline(pts) if pts.len() == 2 => {
                t.extend([s(0, "LINE"), s(8, layer), f(10, pts[0].x), f(20, pts[0].y), f(30, 0.0), f(11, pts[1].x), f(21, pts[1].y), f(31, 0.0)]);
            }
            Item::Polyline(pts) => {
                let closed = pts.len() > 2 && pts[0].dist(pts[pts.len() - 1]) < 1e-9;
                let pts = if closed { &pts[..pts.len() - 1] } else { &pts[..] };
                // 128: line type pattern continues through the vertices.
                t.extend([s(0, "POLYLINE"), s(8, layer), i(66, 1), f(10, 0.0), f(20, 0.0), f(30, 0.0), i(70, 128 + i64::from(closed))]);
                for q in pts {
                    t.extend([s(0, "VERTEX"), s(8, layer), f(10, q.x), f(20, q.y), f(30, 0.0)]);
                }
                t.extend([s(0, "SEQEND"), s(8, layer)]);
            }
            Item::Circle { c, r } => t.extend([s(0, "CIRCLE"), s(8, layer), f(10, c.x), f(20, c.y), f(30, 0.0), f(40, r)]),
            Item::Arc { c, r, a0, a1 } => {
                t.extend([s(0, "ARC"), s(8, layer), f(10, c.x), f(20, c.y), f(30, 0.0), f(40, r), f(50, a0.to_degrees()), f(51, a1.to_degrees())]);
            }
        }
    }
    for (_, pts) in &g.fills {
        // SOLID corners go 1, 2, 4, 3; a triangle repeats its last corner.
        if pts.len() == 3 {
            let (a, b, c) = (pts[0], pts[1], pts[2]);
            t.extend([
                s(0, "SOLID"),
                s(8, "THIN"),
                f(10, a.x),
                f(20, a.y),
                f(30, 0.0),
                f(11, b.x),
                f(21, b.y),
                f(31, 0.0),
                f(12, c.x),
                f(22, c.y),
                f(32, 0.0),
                f(13, c.x),
                f(23, c.y),
                f(33, 0.0),
            ]);
        }
    }
    for (_, tx) in &g.texts {
        let just = match tx.align {
            Align::Left => 0,
            Align::Center => 1,
            Align::Right => 2,
        };
        t.extend([
            s(0, "TEXT"),
            s(8, "TEXT"),
            f(10, tx.at.x),
            f(20, tx.at.y),
            f(30, 0.0),
            f(40, tx.height),
            s(1, &tx.text.replace('Ø', "%%c").replace('°', "%%d")),
        ]);
        if just != 0 {
            t.extend([i(72, just), f(11, tx.at.x), f(21, tx.at.y), f(31, 0.0)]);
        }
    }
    t.extend([s(0, "ENDSEC"), s(0, "EOF")]);
    tenon_dxf::write_ascii(&t)
}

/// What a DXF holds, by entity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DxfStats {
    pub lines: usize,
    pub polylines: usize,
    pub arcs: usize,
    pub circles: usize,
    pub solids: usize,
    pub texts: usize,
}

/// Reads a DXF back as other programs do: sections in order, every entity on a layer the LAYER
/// table defines with a line type the LTYPE table defines, polylines closed by SEQEND, every
/// point inside the header's extents, every arc and circle with a radius.
pub fn check_dxf(text: &str) -> Result<DxfStats, String> {
    let tags = tenon_dxf::parse(text.as_bytes()).map_err(|e| e.to_string())?;
    let sections = tenon_dxf::sections(&tags);
    let names: Vec<&str> = sections.iter().map(|s| s.name.as_str()).collect();
    if names != ["HEADER", "TABLES", "ENTITIES"] {
        return Err(format!("sections {names:?}"));
    }
    let point = |var: &str| -> Option<(f64, f64)> {
        let i = sections[0].tags.iter().position(|t| t.code == 9 && t.str() == var)?;
        let x = sections[0].tags.get(i + 1).filter(|t| t.code == 10)?.f64();
        let y = sections[0].tags.get(i + 2).filter(|t| t.code == 20)?.f64();
        Some((x, y))
    };
    let (lo, hi) = (point("$EXTMIN").ok_or("no $EXTMIN")?, point("$EXTMAX").ok_or("no $EXTMAX")?);
    let tables = tenon_dxf::records(&sections[1].tags);
    let ltypes: Vec<String> = tables.iter().filter(|(k, _)| k == "LTYPE").filter_map(|(_, r)| r.iter().find(|t| t.code == 2).map(Tag::str)).collect();
    let mut layers: Vec<String> = Vec::new();
    for (_, r) in tables.iter().filter(|(k, _)| k == "LAYER") {
        let name = r.iter().find(|t| t.code == 2).map(Tag::str).ok_or("a layer without a name")?;
        let lt = r.iter().find(|t| t.code == 6).map(Tag::str).ok_or("a layer without a line type")?;
        if !ltypes.contains(&lt) {
            return Err(format!("layer {name} uses line type {lt}, which is not defined"));
        }
        layers.push(name);
    }
    let mut stats = DxfStats::default();
    let mut in_polyline = false;
    for (kind, r) in tenon_dxf::records(&sections[2].tags) {
        let layer = r.iter().find(|t| t.code == 8).map(Tag::str).ok_or_else(|| format!("a {kind} without a layer"))?;
        if !layers.contains(&layer) {
            return Err(format!("a {kind} on layer {layer}, which is not defined"));
        }
        for (cx, cy) in [(10, 20), (11, 21), (12, 22), (13, 23)] {
            let (x, y) = (r.iter().find(|t| t.code == cx).map(Tag::f64), r.iter().find(|t| t.code == cy).map(Tag::f64));
            if let (Some(x), Some(y)) = (x, y)
                && kind != "POLYLINE"
                && !(x >= lo.0 - 1e-6 && x <= hi.0 + 1e-6 && y >= lo.1 - 1e-6 && y <= hi.1 + 1e-6)
            {
                return Err(format!("a {kind} at ({x}, {y}) is outside the extents"));
            }
        }
        match (kind.as_str(), in_polyline) {
            ("VERTEX", true) => continue,
            ("SEQEND", true) => {
                in_polyline = false;
                continue;
            }
            (_, true) => return Err(format!("a polyline not closed by SEQEND before a {kind}")),
            ("POLYLINE", false) => {
                in_polyline = true;
                stats.polylines += 1;
            }
            ("LINE", _) => stats.lines += 1,
            ("ARC" | "CIRCLE", _) => {
                let r = r.iter().find(|t| t.code == 40).map(Tag::f64).ok_or_else(|| format!("a {kind} without a radius"))?;
                if r.is_nan() || r <= 0.0 {
                    return Err(format!("a {kind} of radius {r}"));
                }
                if kind == "ARC" {
                    stats.arcs += 1;
                } else {
                    stats.circles += 1;
                }
            }
            ("SOLID", _) => stats.solids += 1,
            ("TEXT", _) => stats.texts += 1,
            (other, _) => return Err(format!("an unexpected {other}")),
        }
    }
    if in_polyline {
        return Err("the last polyline is not closed by SEQEND".into());
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::graphics::Owner;

    fn sample() -> Graphics {
        let mut g = Graphics { width: 100.0, height: 50.0, ..Graphics::default() };
        g.line(Owner::Frame, Pen::Visible, Vec2::new(10.0, 10.0), Vec2::new(90.0, 10.0));
        g.line(Owner::Frame, Pen::Hidden, Vec2::new(10.0, 20.0), Vec2::new(90.0, 20.0));
        g.circle(Owner::Frame, Pen::Visible, Vec2::new(20.0, 35.0), 4.0);
        // A half circle, as views draw arcs: a run of points.
        let arc: Vec<Vec2> =
            (0..=30).map(|i| f64::from(i) / 30.0 * std::f64::consts::PI).map(|a| Vec2::new(70.0 + 6.0 * a.cos(), 30.0 + 6.0 * a.sin())).collect();
        g.polyline(Owner::Frame, Pen::Center, arc);
        g.polyline(Owner::Frame, Pen::Thin, vec![Vec2::new(5.0, 45.0), Vec2::new(15.0, 45.0), Vec2::new(15.0, 48.0)]);
        g.arrow(Owner::Frame, Vec2::new(50.0, 30.0), Vec2::new(1.0, 0.0));
        g.text(Owner::Frame, Vec2::new(50.0, 40.0), 3.5, "Ø12 (A)", Align::Center);
        g
    }

    /// The elements of an SVG, read by an XML parser (which refuses anything not well formed).
    fn svg_elements(s: &str) -> Vec<String> {
        use quick_xml::events::Event;
        let mut r = quick_xml::Reader::from_str(s);
        let (mut names, mut depth) = (Vec::new(), 0);
        loop {
            match r.read_event() {
                Ok(Event::Start(e)) => {
                    depth += 1;
                    names.push(String::from_utf8_lossy(e.name().as_ref()).into_owned());
                }
                Ok(Event::Empty(e)) => names.push(String::from_utf8_lossy(e.name().as_ref()).into_owned()),
                Ok(Event::End(_)) => depth -= 1,
                Ok(Event::Eof) => break,
                Err(e) => panic!("not well-formed XML: {e}"),
                _ => {}
            }
        }
        assert_eq!(depth, 0, "every element closed");
        names
    }

    #[test]
    fn svg_pdf_and_dxf_hold_the_sheet() {
        let g = sample();
        let s = svg(&g);
        assert!(s.starts_with("<?xml") && s.contains("stroke-dasharray=\"3 1.5\"") && s.contains("Ø12 (A)"));
        let names = svg_elements(&s);
        assert_eq!(names[0], "svg");
        let count = |n: &str| names.iter().filter(|x| *x == n).count();
        assert_eq!((count("circle"), count("path"), count("polygon"), count("text")), (1, 1, 1, 1));
        assert!(s.contains("<circle cx=\"20.000\" cy=\"15.000\" r=\"4.0000\"/>"), "the circle is a circle");
        assert!(s.contains(" A 6.0000 6.0000 0 0 0 "), "the arc is an arc");
        let p = pdf(&[g.clone(), g.clone()]);
        assert_eq!(check_pdf(&p), Ok(2));
        let text = String::from_utf8_lossy(&p);
        assert!(text.contains("\\330") && text.contains("\\(A\\)"), "Ø in WinAnsi, parentheses escaped");
        assert!(text.contains("BT 3 Tr"), "the words are there, invisible");
        assert!(text.matches(" c\n").count() >= 4 + 2, "circle and arc as curves");
        let d = dxf(&g);
        let stats = check_dxf(&d).unwrap();
        assert_eq!(stats, DxfStats { lines: 2, polylines: 1, arcs: 1, circles: 1, solids: 1, texts: 1 });
        assert!(d.contains("%%c12"));
    }

    #[test]
    fn broken_files_are_caught_by_the_checks() {
        let p = pdf(&[sample()]);
        let insert = |needle: &[u8], extra: &[u8]| {
            let at = p.windows(needle.len()).position(|w| w == needle).unwrap();
            let mut q = p.clone();
            q.splice(at..at, extra.iter().copied());
            q
        };
        assert_eq!(check_pdf(&p), Ok(1));
        // Where object 2 should start, something else: its xref entry points at the wrong thing.
        let mut renumbered = p.clone();
        let at = p.windows(7).position(|w| w == b"2 0 obj").unwrap();
        renumbered[at] = b'9';
        assert!(check_pdf(&renumbered).unwrap_err().contains("xref entry 2"));
        // Everything after a byte added moves; startxref no longer finds the table.
        assert!(check_pdf(&insert(b"2 0 obj", b" ")).unwrap_err().contains("startxref"));
        // A stream whose /Length is wrong.
        let longer = insert(b"/Length ", b"1");
        assert!(check_pdf(&longer).is_err());
        let d = dxf(&sample());
        let edited = |edit: &dyn Fn(&mut Vec<Tag>)| {
            let mut tags = tenon_dxf::parse(d.as_bytes()).unwrap();
            edit(&mut tags);
            check_dxf(&tenon_dxf::write_ascii(&tags))
        };
        assert!(edited(&|_| {}).is_ok());
        // The THIN layer's definition renamed: the arrowheads are on a layer that does not exist.
        let err = edited(&|t| {
            if let Some(x) = t.iter_mut().find(|x| x.code == 2 && x.str() == "THIN") {
                *x = Tag::s(2, "GONE");
            }
        });
        assert!(err.unwrap_err().contains("layer THIN"));
        // A layer with a line type the table lacks.
        let err = edited(&|t| {
            if let Some(x) = t.iter_mut().find(|x| x.code == 6 && x.str() == "HIDDEN") {
                *x = Tag::s(6, "DOTTED");
            }
        });
        assert!(err.unwrap_err().contains("DOTTED"));
        // A polyline left open; a point outside the extents.
        let err = edited(&|t| {
            if let Some(x) = t.iter_mut().find(|x| x.code == 0 && x.str() == "SEQEND") {
                *x = Tag::s(0, "VERTEX");
            }
        });
        assert!(err.unwrap_err().contains("SEQEND"));
        let err = edited(&|t| {
            if let Some(x) = t.iter_mut().rev().find(|x| x.code == 10) {
                *x = Tag::f(10, 500.0);
            }
        });
        assert!(err.unwrap_err().contains("outside the extents"));
    }
}

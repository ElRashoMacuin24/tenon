//! Writing sheets: SVG, PDF (one page per sheet) and DXF. All three come from the same
//! [`Graphics`], so they match the screen.

use std::fmt::Write as _;

use tenon_dxf::Tag;

use crate::graphics::{Align, Graphics, Pen};

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// One sheet as SVG (millimetres, white paper).
pub fn svg(g: &Graphics) -> String {
    let (w, h) = (g.width, g.height);
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="{h}mm" viewBox="0 0 {w} {h}"><rect width="{w}" height="{h}" fill="white"/>"#
    );
    for pen in Pen::ALL {
        let dash = pen.dashes().iter().map(|d| format!("{d}")).collect::<Vec<_>>().join(" ");
        let dash = if dash.is_empty() { String::new() } else { format!(r#" stroke-dasharray="{dash}""#) };
        let _ = write!(s, r#"<g fill="none" stroke="black" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round"{dash}>"#, pen.width());
        for (_, p, pts) in g.strokes.iter().filter(|x| x.1 == pen) {
            let _ = p;
            let path: Vec<String> = pts.iter().map(|q| format!("{:.3},{:.3}", q.x, h - q.y)).collect();
            let _ = write!(s, r#"<polyline points="{}"/>"#, path.join(" "));
        }
        s.push_str("</g>");
    }
    s.push_str(r#"<g fill="black" stroke="none">"#);
    for (_, pts) in &g.fills {
        let path: Vec<String> = pts.iter().map(|q| format!("{:.3},{:.3}", q.x, h - q.y)).collect();
        let _ = write!(s, r#"<polygon points="{}"/>"#, path.join(" "));
    }
    s.push_str("</g>");
    s.push_str(r#"<g fill="black" font-family="Helvetica, Arial, sans-serif">"#);
    for (_, t) in &g.texts {
        let anchor = match t.align {
            Align::Left => "start",
            Align::Center => "middle",
            Align::Right => "end",
        };
        // Cap height is about 0.72 of the font size.
        let _ = write!(
            s,
            r#"<text x="{:.3}" y="{:.3}" font-size="{:.3}" text-anchor="{anchor}">{}</text>"#,
            t.at.x,
            h - t.at.y,
            t.height / 0.72,
            xml(&t.text)
        );
    }
    s.push_str("</g></svg>");
    s
}

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
        let mut c = String::new();
        c.push_str("1 J 1 j 0 0 0 RG 0 0 0 rg\n");
        for pen in Pen::ALL {
            let dash: Vec<String> = pen.dashes().iter().map(|d| format!("{:.3}", d * PT)).collect();
            let _ = writeln!(c, "{:.3} w [{}] 0 d", pen.width() * PT, dash.join(" "));
            for (_, _, pts) in g.strokes.iter().filter(|x| x.1 == pen) {
                for (k, p) in pts.iter().enumerate() {
                    let _ = write!(c, "{:.3} {:.3} {} ", p.x * PT, p.y * PT, if k == 0 { "m" } else { "l" });
                }
                c.push_str("S\n");
            }
        }
        c.push_str("[] 0 d\n");
        for (_, pts) in &g.fills {
            for (k, p) in pts.iter().enumerate() {
                let _ = write!(c, "{:.3} {:.3} {} ", p.x * PT, p.y * PT, if k == 0 { "m" } else { "l" });
            }
            c.push_str("h f\n");
        }
        for (_, t) in &g.texts {
            let w = helvetica_width(&t.text, t.height);
            let x = match t.align {
                Align::Left => t.at.x,
                Align::Center => t.at.x - w / 2.0,
                Align::Right => t.at.x - w,
            };
            let _ = writeln!(c, "BT /F1 {:.3} Tf {:.3} {:.3} Td ({}) Tj ET", t.height / 0.72 * PT, x * PT, t.at.y * PT, pdf_text(&t.text));
        }
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.3} {:.3}] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            g.width * PT,
            g.height * PT,
            5 + 2 * i
        ));
        objects.push(format!("<< /Length {} >>\nstream\n{c}endstream", c.len()));
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

fn ltype(pen: Pen) -> &'static str {
    match pen {
        Pen::Hidden => "HIDDEN",
        Pen::Center | Pen::Cutting => "CENTER",
        _ => "CONTINUOUS",
    }
}

/// One sheet as DXF (R12 entities, millimetres): a layer per pen, lines, filled triangles
/// (SOLID) and text.
pub fn dxf(g: &Graphics) -> String {
    let mut t: Vec<Tag> = Vec::new();
    let s = |c: i32, v: &str| Tag::s(c, v);
    let f = Tag::f;
    let i = Tag::i;
    t.extend([s(0, "SECTION"), s(2, "HEADER"), s(9, "$ACADVER"), s(1, "AC1009"), s(9, "$INSUNITS"), i(70, 4), s(0, "ENDSEC")]);
    t.extend([s(0, "SECTION"), s(2, "TABLES")]);
    t.extend([s(0, "TABLE"), s(2, "LTYPE"), i(70, 3)]);
    for (name, desc, pattern) in [
        ("CONTINUOUS", "Solid line", vec![]),
        ("HIDDEN", "Hidden __ __", vec![3.0, -1.5]),
        ("CENTER", "Center ____ _ ____", vec![12.0, -1.5, 2.0, -1.5]),
    ] {
        t.extend([
            s(0, "LTYPE"),
            s(2, name),
            i(70, 0),
            s(3, desc),
            i(72, 65),
            i(73, pattern.len() as i64),
            f(40, pattern.iter().map(|x: &f64| x.abs()).sum()),
        ]);
        for p in pattern {
            t.push(f(49, p));
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
    for (_, pen, pts) in &g.strokes {
        for w in pts.windows(2) {
            t.extend([s(0, "LINE"), s(8, pen.name()), f(10, w[0].x), f(20, w[0].y), f(30, 0.0), f(11, w[1].x), f(21, w[1].y), f(31, 0.0)]);
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::graphics::Owner;
    use tenon_geom::Vec2;

    fn sample() -> Graphics {
        let mut g = Graphics { width: 100.0, height: 50.0, ..Graphics::default() };
        g.line(Owner::Frame, Pen::Visible, Vec2::new(10.0, 10.0), Vec2::new(90.0, 10.0));
        g.line(Owner::Frame, Pen::Hidden, Vec2::new(10.0, 20.0), Vec2::new(90.0, 20.0));
        g.arrow(Owner::Frame, Vec2::new(50.0, 30.0), Vec2::new(1.0, 0.0));
        g.text(Owner::Frame, Vec2::new(50.0, 40.0), 3.5, "Ø12 (A)", Align::Center);
        g
    }

    #[test]
    fn svg_pdf_and_dxf_hold_the_sheet() {
        let g = sample();
        let s = svg(&g);
        assert!(s.starts_with("<svg") && s.contains("stroke-dasharray=\"3 1.5\"") && s.contains("Ø12 (A)"));
        let p = pdf(&[g.clone(), g.clone()]);
        let text = String::from_utf8_lossy(&p);
        assert!(text.starts_with("%PDF-1.4") && text.contains("/Count 2") && text.ends_with("%%EOF\n"));
        assert!(text.contains("\\330") && text.contains("\\(A\\)"), "Ø in WinAnsi, parentheses escaped");
        // The cross-reference table points at each object.
        let xref = text.find("xref").unwrap();
        assert!(text[xref..].contains("0000000015 00000 n"), "object 1 after the 15-byte header");
        let d = dxf(&g);
        let tags = tenon_dxf::parse(d.as_bytes()).unwrap();
        let ents: Vec<String> = tenon_dxf::records(&tags).into_iter().map(|r| r.0).collect();
        assert_eq!(ents.iter().filter(|e| *e == "LINE").count(), 2);
        assert_eq!(ents.iter().filter(|e| *e == "SOLID").count(), 1);
        assert_eq!(ents.iter().filter(|e| *e == "TEXT").count(), 1);
        assert!(d.contains("%%c12"));
    }
}

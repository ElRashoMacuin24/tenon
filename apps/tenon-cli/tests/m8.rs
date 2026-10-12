//! M8: the sheet metal demo. The script checks the folded part and its flat pattern against
//! hand-worked numbers as it runs (examples/m8-tray/README.md has the working); here the files
//! it writes are read back as a cutter's program would read them.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI};
use std::path::{Path, PathBuf};

use serde_json::json;
use tenon_cli::Engine;
use tenon_dxf::Tag;
use tenon_geom::Vec2;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;
use tenon_model::sheet::FlatCurve;
use tenon_model::sheet::flat::{bounds, chain};

fn kernel() -> Box<dyn Kernel> {
    Box::new(OcctKernel::new())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-cli-m8-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn near(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

/// The entities of a DXF file: kind, layer, and the numbers of each by group code.
fn entities(path: &Path) -> Vec<(String, String, Vec<(i32, f64)>)> {
    let text = std::fs::read_to_string(path).unwrap();
    let tags = tenon_dxf::parse(text.as_bytes()).unwrap();
    let sections = tenon_dxf::sections(&tags);
    assert_eq!(sections.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["HEADER", "TABLES", "ENTITIES"], "{}", path.display());
    tenon_dxf::records(&sections[2].tags)
        .into_iter()
        .map(|(kind, r)| {
            let layer = r.iter().find(|t| t.code == 8).map(Tag::str).unwrap_or_default();
            let numbers = r.iter().filter(|t| (10..60).contains(&t.code)).map(|t| (t.code, t.f64())).collect();
            (kind, layer, numbers)
        })
        .collect()
}

/// What is cut, as curves: the entities on the OUTER and INTERIOR layers.
fn cut_curves(all: &[(String, String, Vec<(i32, f64)>)]) -> Vec<FlatCurve> {
    let mut curves = Vec::new();
    for (kind, layer, n) in all {
        if layer != "OUTER" && layer != "INTERIOR" {
            continue;
        }
        let num = |code: i32| n.iter().find(|t| t.0 == code).map(|t| t.1).unwrap_or_else(|| panic!("a {kind} without group {code}"));
        match kind.as_str() {
            "LINE" => curves.push(FlatCurve::Line { start: Vec2::new(num(10), num(20)), end: Vec2::new(num(11), num(21)) }),
            "CIRCLE" => curves.push(FlatCurve::Circle { center: Vec2::new(num(10), num(20)), radius: num(40) }),
            "ARC" => {
                let (c, r) = (Vec2::new(num(10), num(20)), num(40));
                let at = |code: i32| Vec2::polar(c, r, num(code).to_radians());
                curves.push(FlatCurve::Arc { center: c, radius: r, start: at(50), end: at(51), ccw: true });
            }
            other => panic!("an unexpected {other} on {layer}"),
        }
    }
    curves
}

/// The sizes the tray is made to, worked out as in its README: sheet 1.5 thick, bends of
/// inside radius 1.5, K factor 0.44.
struct Tray {
    /// The width of a bend in the flat.
    allowance: f64,
    /// The width of the hem's fold in the flat, and of the whole hem.
    hem_fold: f64,
    hem: f64,
    area: f64,
    volume: f64,
}

fn tray(k: f64) -> Tray {
    let (t, r) = (1.5, 1.5);
    let allowance = FRAC_PI_2 * (r + k * t);
    let set_back = r + t;
    let (floor_x, floor_y) = (120.0 - 2.0 * set_back, 80.0 - 2.0 * set_back);
    // The walls are mitred: each ends its thickness and (gap / root 2) from the corner.
    let end = t + t / 2f64.sqrt();
    let walls = 2.0 * (120.0 + 80.0) - 8.0 * end;
    // A corner relief: a disc 6 across, its centre half an allowance out from each bend line.
    // What it takes of the floor, of each of the two bends, and of the foot of each wall.
    let (d, radius) = (allowance / 2.0, 3.0f64);
    let b = (radius * radius - d * d).sqrt();
    let from = end - (set_back - d);
    let part = |s: f64| (s * (radius * radius - s * s).sqrt() + radius * radius * (s / radius).asin()) / 2.0;
    let off_floor = part(b) - part(d) - d * (b - d);
    let off_bend = 2.0 * part(d) - 2.0 * d * d;
    let off_wall = part(b) - part(from) - d * (b - from);
    let floor = floor_x * floor_y - 4.0 * off_floor - 4.0 * PI * 4.0;
    let round = 2.0 * (floor_x + floor_y);
    let wall = 25.0 - set_back;
    let (window, tab_cut) = (30.0 * 8.0, set_back * 40.0 + 2.0 * 1.5 * (set_back + 0.75));
    let tab = 12.0 - set_back;
    // The hem: a gap of 0.75 (a fold of inside radius 0.375), 6 long over all, along the top of
    // the back wall.
    let (fold_radius, hem_flat, hem_along) = (0.375, 6.0 - 0.375 - t, 120.0 - 2.0 * end);
    let hem_fold = PI * (fold_radius + k * t);
    let bends = round * allowance - 8.0 * off_bend;
    let flat_walls = walls * wall - 8.0 * off_wall - window - tab_cut;
    let area = floor + bends + flat_walls + (allowance + tab) * 40.0 + (hem_fold + hem_flat) * hem_along;
    // A bend's volume for the area of it in the flat: the ring over its neutral width.
    let ring = |angle: f64, inside: f64| angle / 2.0 * ((inside + t) * (inside + t) - inside * inside);
    let volume = (floor + flat_walls + tab * 40.0) * t
        + bends * ring(FRAC_PI_2, r) / allowance
        + ring(FRAC_PI_2, r) * 40.0
        + (ring(PI, fold_radius) + hem_flat * t) * hem_along;
    Tray { allowance, hem_fold, hem: hem_fold + hem_flat, area, volume }
}

#[test]
fn the_m8_demo_script_folds_a_tray_and_lays_it_flat_for_a_cutter() {
    let dir = scratch("demo");
    let r = tenon_cli::demo_m8(kernel(), &dir).unwrap();
    assert_eq!(r.json["ok"], true, "{}", r.text);
    let want = tray(0.44);

    // The DXF, read back: what is cut joins up into one outline with five cut-outs, and encloses
    // the sheet's area to the last digit the file keeps.
    let all = entities(&dir.join("tray-flat.dxf"));
    let (outer, inner) = chain(cut_curves(&all)).unwrap();
    let area = outer.signed_area() - inner.iter().map(|l| l.signed_area().abs()).sum::<f64>();
    assert!(near(area, want.area, 1e-7), "{area} vs {}", want.area);
    assert_eq!(inner.len(), 5, "four holes and the window (the corner reliefs are bites out of the outline)");
    // It lies in the corner of the sheet: from (0, 0) to its length and width.
    let (lo, hi) = bounds(std::slice::from_ref(&outer));
    let (length, width) = (114.0 + 2.0 * (want.allowance + 22.0), 74.0 + 2.0 * (want.allowance + 22.0) + want.allowance + 9.0 - 3.0 + want.hem);
    assert!(lo.near(Vec2::ZERO, 1e-6) && near(hi.x, length, 1e-7) && near(hi.y, width, 1e-7), "{lo:?} {hi:?} vs {length} x {width}");
    // Lines, arcs and circles only: nothing a cutter would have to approximate.
    let count = |kind: &str, layer: &str| all.iter().filter(|e| e.0 == kind && e.1 == layer).count();
    // The outline: at each corner the two walls' ends and one arc between them; the tops of
    // the walls, the front one with the tab and its slots (nine lines), the back one moved out
    // by the hem.
    assert_eq!((count("LINE", "OUTER"), count("ARC", "OUTER"), count("POLYLINE", "OUTER") + count("POLYLINE", "INTERIOR")), (20, 4, 0));
    assert_eq!((count("CIRCLE", "INTERIOR"), count("ARC", "INTERIOR"), count("LINE", "INTERIOR")), (4, 0, 4));
    for arc in all.iter().filter(|e| e.0 == "ARC") {
        let radius = arc.2.iter().find(|t| t.0 == 40).unwrap().1;
        assert!((radius - 3.0).abs() < 2e-6, "a relief 6 across: {radius}");
    }
    // The bend lines are on layers of their own: four walls up, the tab and the hem down, and
    // the two lines each bend lies between.
    assert_eq!((count("LINE", "BEND_UP"), count("LINE", "BEND_DOWN"), count("LINE", "BEND_EXTENT")), (4, 2, 12));
    let centre = |layer: &str| -> Vec<f64> {
        let mut at: Vec<f64> = all
            .iter()
            .filter(|e| e.1 == layer)
            .map(|e| {
                let n = |code: i32| e.2.iter().find(|t| t.0 == code).unwrap().1;
                // A bend line is level or upright: where it is across the sheet.
                if (n(10) - n(11)).abs() < 1e-9 { n(10) } else { n(20) }
            })
            .collect();
        at.sort_by(f64::total_cmp);
        at
    };
    let half = want.allowance / 2.0;
    // (Level lines by their height, upright ones by how far across they are: the walls' bends
    // are half an allowance outside the floor on every side.)
    let ups = centre("BEND_UP");
    let tab_rise = want.allowance + 9.0 - 3.0;
    let expected = {
        let mut v = vec![22.0 + half, 22.0 + want.allowance + 114.0 + half, tab_rise + 22.0 + half, tab_rise + 22.0 + want.allowance + 74.0 + half];
        v.sort_by(f64::total_cmp);
        v
    };
    for (got, want) in ups.iter().zip(&expected) {
        assert!((got - want).abs() < 2e-6, "{ups:?} vs {expected:?}");
    }
    let downs = centre("BEND_DOWN");
    assert!((downs[0] - (9.0 + half)).abs() < 2e-6, "the tab's bend, 9 in from the tab's end: {downs:?}");
    // The hem's fold is past the top of the back wall, 4.125 of hem beyond it.
    assert!((downs[1] - (width - 4.125 - want.hem_fold / 2.0)).abs() < 2e-6, "{downs:?}");

    // The picture of it, at full size.
    let svg = std::fs::read_to_string(dir.join("tray-flat.svg")).unwrap();
    assert!(
        svg.starts_with(&format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.3}mm\" height=\"{:.3}mm\"", length + 10.0, width + 10.0)),
        "{}",
        &svg[..120]
    );
    assert_eq!((svg.matches("#1a8a3a").count(), svg.matches("#c0392b").count()), (4, 2));
    assert!(std::fs::read(dir.join("tray.png")).unwrap().starts_with(&[0x89, b'P', b'N', b'G']));
    assert!(std::fs::read_to_string(dir.join("tray.step")).unwrap().starts_with("ISO-10303-21"));

    // The part reopens as what it was: a sheet metal part with its rule, its flat pattern kept,
    // folding to the same solid and laying flat to the same sheet.
    let mut e = Engine::new(kernel(), &dir);
    e.exec("file.open", &json!({ "path": "tray.tenon" })).unwrap();
    let rule = e.exec("sheet.info", &json!({})).unwrap();
    assert_eq!(
        (rule["sheet_metal"].as_bool(), rule["thickness"].as_f64(), rule["corner"].as_str(), rule["flat_pattern"].as_bool()),
        (Some(true), Some(1.5), Some("round"), Some(true)),
        "{rule}"
    );
    let m = e.exec("model.mass", &json!({})).unwrap();
    // (To what the kernel keeps of a relief's cut through a bend, which is a curve of its own.)
    assert!(near(m["bodies"][0]["volume"].as_f64().unwrap(), want.volume, 1e-7), "{m} vs {}", want.volume);
    assert!(near(m["mass"].as_f64().unwrap(), want.volume / 1000.0 * 2.68, 1e-7), "{m}");
    let flat = e.exec("sheet.flat", &json!({})).unwrap();
    assert!(near(flat["area"].as_f64().unwrap(), want.area, 1e-7) && near(flat["length"].as_f64().unwrap(), length, 1e-9), "{flat}");
    // A different K factor is a different sheet: every bend is narrower in the flat. The folded
    // part changes only where its corner reliefs sit, which are laid out in the flat.
    e.exec("sheet.defaults", &json!({ "k_factor": 0.33 })).unwrap();
    let other = e.exec("sheet.flat", &json!({})).unwrap();
    let narrower = FRAC_PI_2 * (0.44 - 0.33) * 1.5;
    assert!(near(other["length"].as_f64().unwrap(), length - 2.0 * narrower, 1e-9), "{other}");
    let folded = e.exec("model.mass", &json!({})).unwrap()["bodies"][0]["volume"].as_f64().unwrap();
    assert!(near(folded, tray(0.33).volume, 1e-7) && (folded - want.volume).abs() < 20.0, "{folded} vs {}", tray(0.33).volume);
    assert!(near(other["area"].as_f64().unwrap(), tray(0.33).area, 1e-7), "{other}");
}

/// The numbers and the words of a text, apart: files written on different machines may differ
/// in the last digit of a number that was rounded to be written, and a checkout may have
/// changed the ends of the lines.
fn tokens(text: &str) -> (Vec<f64>, String) {
    let (mut numbers, mut words) = (Vec::new(), String::new());
    for token in text.split(|c: char| c.is_whitespace() || c == ',' || c == '"').filter(|t| !t.is_empty()) {
        match token.parse::<f64>() {
            Ok(n) => numbers.push(n),
            Err(_) => {
                words.push_str(token);
                words.push(' ');
            }
        }
    }
    (numbers, words)
}

#[test]
fn the_checked_in_m8_example_is_what_the_script_writes() {
    let dir = scratch("same");
    tenon_cli::demo_m8(kernel(), &dir).unwrap();
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m8-tray");
    // The part, compared as `tenon-cli diff` compares files (see the M6 example's test for why
    // not byte for byte).
    let d = tenon_io::diff::diff_files(&example.join("tray.tenon"), &dir.join("tray.tenon")).unwrap();
    assert!(d.same(), "tray.tenon: regenerate the example (see its README):\n{}", d.text());
    // The flat pattern's files: the same words, and the same numbers to what a file keeps.
    for (file, tolerance) in [("tray-flat.dxf", 2e-6), ("tray-flat.svg", 2e-4)] {
        let (kept, made) = (std::fs::read_to_string(example.join(file)).unwrap(), std::fs::read_to_string(dir.join(file)).unwrap());
        let ((a, words_a), (b, words_b)) = (tokens(&kept), tokens(&made));
        assert_eq!(words_a, words_b, "{file}: regenerate the example (see its README)");
        assert_eq!(a.len(), b.len(), "{file}");
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() <= tolerance, "{file}: {x} vs {y}");
        }
    }
}

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
    area: f64,
    volume: f64,
}

fn tray() -> Tray {
    let (t, r, k) = (1.5, 1.5, 0.44);
    let allowance = FRAC_PI_2 * (r + k * t);
    let set_back = r + t;
    let (floor_x, floor_y) = (120.0 - 2.0 * set_back, 80.0 - 2.0 * set_back);
    // A corner relief: a hole 6 across touching both bend lines, opened to the corner.
    let relief = 9.0 + 0.75 * PI * 9.0;
    let floor = floor_x * floor_y - 4.0 * relief - 4.0 * PI * 4.0;
    let round = 2.0 * (floor_x + floor_y);
    let wall = 25.0 - set_back;
    let (window, tab_cut) = (30.0 * 8.0, set_back * 40.0 + 2.0 * 1.5 * (set_back + 0.75));
    let tab = 12.0 - set_back;
    let area = floor + round * (allowance + wall) - window - tab_cut + (allowance + tab) * 40.0;
    let bend = FRAC_PI_2 / 2.0 * ((r + t) * (r + t) - r * r);
    let volume = (floor + round * wall - window - tab_cut + tab * 40.0) * t + bend * (round + 40.0);
    Tray { allowance, area, volume }
}

#[test]
fn the_m8_demo_script_folds_a_tray_and_lays_it_flat_for_a_cutter() {
    let dir = scratch("demo");
    let r = tenon_cli::demo_m8(kernel(), &dir).unwrap();
    assert_eq!(r.json["ok"], true, "{}", r.text);
    let want = tray();

    // The DXF, read back: what is cut joins up into one outline with nine cut-outs, and encloses
    // the sheet's area to the last digit the file keeps.
    let all = entities(&dir.join("tray-flat.dxf"));
    let (outer, inner) = chain(cut_curves(&all)).unwrap();
    let area = outer.signed_area() - inner.iter().map(|l| l.signed_area().abs()).sum::<f64>();
    assert!(near(area, want.area, 1e-7), "{area} vs {}", want.area);
    assert_eq!(inner.len(), 9, "four holes, four reliefs, the window");
    // It lies in the corner of the sheet: from (0, 0) to its length and width.
    let (lo, hi) = bounds(std::slice::from_ref(&outer));
    let (length, width) = (114.0 + 2.0 * (want.allowance + 22.0), 74.0 + 2.0 * (want.allowance + 22.0) + want.allowance + 9.0 - 3.0);
    assert!(lo.near(Vec2::ZERO, 1e-6) && near(hi.x, length, 1e-7) && near(hi.y, width, 1e-7), "{lo:?} {hi:?} vs {length} x {width}");
    // Lines, arcs and circles only: nothing a cutter would have to approximate.
    let count = |kind: &str, layer: &str| all.iter().filter(|e| e.0 == kind && e.1 == layer).count();
    assert_eq!((count("LINE", "OUTER"), count("ARC", "OUTER"), count("POLYLINE", "OUTER") + count("POLYLINE", "INTERIOR")), (20, 0, 0));
    assert_eq!((count("CIRCLE", "INTERIOR"), count("ARC", "INTERIOR"), count("LINE", "INTERIOR")), (4, 4, 4 * 2 + 4));
    // The bend lines are on layers of their own: four walls up, the tab down, and the two lines
    // each bend lies between.
    assert_eq!((count("LINE", "BEND_UP"), count("LINE", "BEND_DOWN"), count("LINE", "BEND_EXTENT")), (4, 1, 10));
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
    assert!((centre("BEND_DOWN")[0] - (9.0 + half)).abs() < 2e-6, "the tab's bend, 9 in from the tab's end: {:?}", centre("BEND_DOWN"));

    // The picture of it, at full size.
    let svg = std::fs::read_to_string(dir.join("tray-flat.svg")).unwrap();
    assert!(
        svg.starts_with(&format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.3}mm\" height=\"{:.3}mm\"", length + 10.0, width + 10.0)),
        "{}",
        &svg[..120]
    );
    assert_eq!((svg.matches("#1a8a3a").count(), svg.matches("#c0392b").count()), (4, 1));
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
    assert!(near(m["bodies"][0]["volume"].as_f64().unwrap(), want.volume, 1e-9), "{m} vs {}", want.volume);
    assert!(near(m["mass"].as_f64().unwrap(), want.volume / 1000.0 * 2.68, 1e-9), "{m}");
    let flat = e.exec("sheet.flat", &json!({})).unwrap();
    assert!(near(flat["area"].as_f64().unwrap(), want.area, 1e-9) && near(flat["length"].as_f64().unwrap(), length, 1e-9), "{flat}");
    // A different K factor is a different sheet; the folded part is the same.
    e.exec("sheet.defaults", &json!({ "k_factor": 0.33 })).unwrap();
    let other = e.exec("sheet.flat", &json!({})).unwrap();
    let narrower = FRAC_PI_2 * (0.44 - 0.33) * 1.5;
    assert!(near(other["length"].as_f64().unwrap(), length - 2.0 * narrower, 1e-9), "{other}");
    assert!(near(e.exec("model.mass", &json!({})).unwrap()["bodies"][0]["volume"].as_f64().unwrap(), want.volume, 1e-9));
}

/// The numbers and the words of a text, apart: files written on different machines may differ
/// in the last digit of a number that was rounded to be written.
fn tokens(text: &str) -> (Vec<f64>, String) {
    let (mut numbers, mut words) = (Vec::new(), String::new());
    for token in text.split(|c: char| c.is_whitespace() || c == ',' || c == '"') {
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

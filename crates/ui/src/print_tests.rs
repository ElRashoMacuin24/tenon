//! 3D printing through the UI (M7), with real pointer and key input: the 3D Print Check window,
//! what it marks in the view, and STL and 3MF files of parts and assemblies.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use egui::vec2;
use serde_json::json;
use tenon_kernel_occt::OcctKernel;

use crate::Workbench;
use crate::print_dialog::{OVERHANG, THIN};
use crate::tests::{Driver, pressable, type_into};
use crate::viewport::Pick;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-ui-print-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A table: a 10 x 10 leg 20 high under the middle of a 40 x 20 top 2 thick.
fn table(wb: &mut Workbench) {
    let leg = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].clone();
    wb.exec("sketch.rectangle", json!({ "sketch": leg, "x1": 15, "y1": 5, "x2": 25, "y2": 15 })).unwrap();
    wb.exec("model.extrude", json!({ "sketch": leg, "distance": 20 })).unwrap();
    let plane = wb.exec("work.plane", json!({ "base": "xy", "distance": 20 })).unwrap()["feature"].clone();
    let top = wb.exec("sketch.create", json!({ "work_plane": plane })).unwrap()["feature"].clone();
    wb.exec("sketch.rectangle", json!({ "sketch": top, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    wb.exec("model.extrude", json!({ "sketch": top, "distance": 2 })).unwrap();
}

fn frames(d: &mut Driver, wb: &mut Workbench) {
    for _ in 0..5 {
        d.frame(wb, vec![]);
    }
}

fn shows(d: &Driver, text: &str) -> bool {
    d.texts().iter().any(|t| t == text)
}

/// What the triangles of a binary STL file enclose, and how many there are.
fn stl_volume(path: &Path) -> (f64, usize) {
    let data = std::fs::read(path).unwrap();
    let count = tenon_io::stl::binary_triangle_count(&data).expect("a well-formed binary STL") as usize;
    let corner = |t: usize, v: usize| -> [f64; 3] {
        let o = 84 + 50 * t + 12 + 12 * v;
        std::array::from_fn(|k| f64::from(f32::from_le_bytes(data[o + 4 * k..o + 4 * k + 4].try_into().unwrap())))
    };
    let six: f64 = (0..count)
        .map(|t| {
            let (a, b, c) = (corner(t, 0), corner(t, 1), corner(t, 2));
            a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2]) + a[2] * (b[0] * c[1] - b[1] * c[0])
        })
        .sum();
    (six / 6.0, count)
}

/// Sends the save dialog's answer to `path`.
fn save_to(wb: &mut Workbench, path: &Path) {
    let to = path.to_path_buf();
    wb.services.pick_save = Some(Box::new(move |_: &str, _: &str| Some(to.clone())));
}

#[test]
fn the_print_check_window_reads_the_part_and_marks_what_it_finds() {
    let dir = scratch("window");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    table(&mut wb);
    d.settle(&mut wb);
    assert!(wb.highlights().is_empty());

    // Environments > 3D Print Check: the part is read at once. Standing on its leg, the underside of
    // the top hangs over air, all but where the leg is.
    wb.run_ui("inspect.print").unwrap();
    frames(&mut d, &mut wb);
    let report = wb.print_report().expect("the part is checked when the window opens").clone();
    assert!(!report.ok() && report.watertight() && report.thin.is_empty(), "{report:?}");
    assert_eq!(report.overhangs.len(), 1);
    assert!((report.overhang_area - 700.0).abs() < 1e-6 && (report.overhangs[0].steepest - 90.0).abs() < 1e-6, "{report:?}");
    let texts = d.texts();
    for shown in [
        "Watertight",
        "Overhangs",
        "Walls",
        "Size",
        "1 face, 700 mm² past 45° (to 90°): wants support",
        "Thinnest 2 mm",
        "40 x 20 x 22 mm, 22 high",
        "It prints, with what is marked above looked after (supports, another way up, thicker walls).",
    ] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    assert!(texts.iter().any(|t| t.starts_with("Closed surface, ") && t.ends_with(" triangles")), "{texts:?}");
    // The window is where it opened, clear of the ribbon (its first, unseen frame measures its
    // contents: a wrapped line measured a letter wide once sent it to the top of the screen).
    let title = d.text_pos("3D Print Check").expect("the window's title");
    assert!(title.y > 160.0 && title.y < 210.0, "{title:?}");
    // The face is orange in the view, and it is the underside of the top.
    let under = Pick::Face { body: report.overhangs[0].body, face: report.overhangs[0].face };
    assert_eq!(wb.highlights(), vec![(under, OVERHANG)]);
    let (_, face) = &wb.scene().bodies[0].faces[report.overhangs[0].face as usize];
    assert!((face.centroid.z - 20.0).abs() < 1e-9 && (face.area - 700.0).abs() < 1e-6, "{face:?}");

    // Turned over (-Z up) the top lies on the bed: nothing against printing it.
    d.click(&mut wb, pressable(&d, "tn_print_up_-z"));
    frames(&mut d, &mut wb);
    assert_eq!(wb.chrome.print.setup.up, tenon_io::print::Up::NegZ);
    assert!(wb.print_report().is_some_and(tenon_io::print::Report::ok), "{:?}", wb.chrome.print.report);
    assert!(shows(&d, "None past 45°") && shows(&d, "Nothing found against printing it as it stands."), "{:?}", d.texts());
    assert!(wb.highlights().is_empty());

    // A printer that wants 3 mm walls: the top, 2 thick, is too thin, seen from both its faces.
    type_into(&mut d, &mut wb, "tn_print_wall", "3");
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert_eq!(wb.chrome.print.setup.min_wall, 3.0);
    let report = wb.print_report().expect("checked again at the new thickness").clone();
    assert_eq!(report.thin.len(), 2, "{report:?}");
    assert!(shows(&d, "Thinnest 2 mm: 2 faces under 3 mm"), "{:?}", d.texts());
    let red: Vec<Pick> = wb.highlights().into_iter().filter(|(_, c)| *c == THIN).map(|(p, _)| p).collect();
    assert_eq!(red.len(), 2);
    // Show in the model, unticked, leaves the part as it is drawn.
    d.click(&mut wb, pressable(&d, "tn_print_show"));
    frames(&mut d, &mut wb);
    assert!(wb.highlights().is_empty() && wb.print_report().is_some());
    d.click(&mut wb, pressable(&d, "tn_print_show"));
    frames(&mut d, &mut wb);
    assert_eq!(wb.highlights().len(), 2);
    // What is not a thickness or an angle is refused, and says so.
    type_into(&mut d, &mut wb, "tn_print_wall", "thick");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.status_error && wb.status().contains("is not a wall thickness"), "{}", wb.status());
    type_into(&mut d, &mut wb, "tn_print_overhang", "120");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.status_error && wb.status().contains("is not an overhang limit"), "{}", wb.status());
    assert_eq!((wb.chrome.print.setup.min_wall, wb.chrome.print.setup.overhang), (3.0, 45.0));

    // The bed: it fits the usual one, and not one 30 square (it is 40 long).
    d.click(&mut wb, pressable(&d, "tn_print_bed"));
    frames(&mut d, &mut wb);
    assert!(shows(&d, "40 x 20 x 22 mm, 22 high: fits the bed"), "{:?}", d.texts());
    for axis in ["x", "y"] {
        let at = d.ctx.read_response(egui::Id::new(("tn_print_bed", axis))).expect("the bed's size").rect.center();
        d.click(&mut wb, at);
        d.frame(
            &mut wb,
            vec![egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }],
        );
        d.frame(&mut wb, vec![egui::Event::Text("30".into())]);
        d.tap(&mut wb, egui::Key::Enter);
        frames(&mut d, &mut wb);
    }
    assert_eq!(wb.chrome.print.bed, [30.0, 30.0, 250.0]);
    assert!(shows(&d, "40 x 20 x 22 mm, 22 high: does not fit the bed"), "{:?}", d.texts());
    assert_eq!(wb.print_report().map(|r| r.fits), Some(Some(false)));

    // The part changes; the check follows it. Without its top it is a post: nothing thin.
    wb.run_ui("edit.undo").unwrap();
    d.settle(&mut wb);
    frames(&mut d, &mut wb);
    let report = wb.print_report().expect("checked again as the part changed").clone();
    assert!(report.ok() && report.thin.is_empty() && (report.height - 20.0).abs() < 1e-9, "{report:?}");
    assert!(shows(&d, "10 x 10 x 20 mm, 20 high: fits the bed"), "{:?}", d.texts());
    wb.run_ui("edit.redo").unwrap();
    d.settle(&mut wb);
    frames(&mut d, &mut wb);
    assert_eq!(wb.print_report().map(|r| r.thin.len()), Some(2));

    // The window writes the part out, for the slicer.
    let (stl, mf3) = (dir.join("table.stl"), dir.join("table.3mf"));
    save_to(&mut wb, &stl);
    d.click(&mut wb, pressable(&d, "tn_print_stl"));
    frames(&mut d, &mut wb);
    assert!(!wb.status_error && wb.status().contains("table.stl") && wb.status().ends_with("triangles)"), "{}", wb.status());
    let (volume, triangles) = stl_volume(&stl);
    assert!((volume - (10.0 * 10.0 * 20.0 + 40.0 * 20.0 * 2.0)).abs() < 1e-3 && triangles >= 24, "{volume} in {triangles} triangles");
    save_to(&mut wb, &mf3);
    d.click(&mut wb, pressable(&d, "tn_print_3mf"));
    frames(&mut d, &mut wb);
    assert!(!wb.status_error && wb.status().contains("table.3mf"), "{}", wb.status());
    assert!(std::fs::read(&mf3).unwrap().starts_with(b"PK"), "a 3MF file is a zip archive");

    // Closed with its own button; nothing stays marked.
    wb.run_ui("inspect.print").unwrap();
    frames(&mut d, &mut wb);
    assert!(!wb.chrome.print.open && wb.highlights().is_empty());
    assert!(!shows(&d, "Overhang limit"));
}

#[test]
fn a_part_with_nothing_in_it_says_why_it_is_not_checked() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.run_ui("inspect.print").unwrap();
    frames(&mut d, &mut wb);
    assert!(wb.print_report().is_none());
    assert!(shows(&d, "Not checked: there is no solid"), "{:?}", d.texts());
    // And there is nothing to write out.
    save_to(&mut wb, &scratch("empty").join("nothing.stl"));
    d.click(&mut wb, pressable(&d, "tn_print_stl"));
    frames(&mut d, &mut wb);
    assert!(wb.status_error && wb.status().contains("there is no solid"), "{}", wb.status());
    // While a sketch is open the part is about to change: nothing is checked, nothing marked.
    table(&mut wb);
    d.settle(&mut wb);
    frames(&mut d, &mut wb);
    assert_eq!(wb.highlights().len(), 1);
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    frames(&mut d, &mut wb);
    assert!(wb.highlights().iter().all(|(_, c)| *c != OVERHANG));
}

#[test]
fn files_for_the_slicer_come_from_the_file_menu_for_parts_and_assemblies() {
    let dir = scratch("menu");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    // A part with a round hole: curved, so the file is as close as its tolerance says.
    let sk = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].clone();
    wb.exec("sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    wb.exec("sketch.circle", json!({ "sketch": sk, "cx": 20, "cy": 10, "r": 6 })).unwrap();
    wb.exec("model.extrude", json!({ "sketch": sk, "distance": 10 })).unwrap();
    d.settle(&mut wb);
    let exact = (800.0 - std::f64::consts::PI * 36.0) * 10.0;
    for (entry, file) in [("export.stl", "plate.stl"), ("export.3mf", "plate.3mf")] {
        save_to(&mut wb, &dir.join(file));
        d.click(&mut wb, pressable(&d, "tn_file_tab"));
        d.frame(&mut wb, vec![]);
        d.click(&mut wb, pressable(&d, entry));
        frames(&mut d, &mut wb);
        assert!(!wb.status_error && wb.status().starts_with("Exported ") && wb.status().contains(file), "{entry}: {}", wb.status());
    }
    let (volume, _) = stl_volume(&dir.join("plate.stl"));
    assert!((volume - exact).abs() < 0.002 * exact && volume > exact, "{volume} vs {exact}: chords across a hole leave a little more");
    assert!(std::fs::read(dir.join("plate.3mf")).unwrap().starts_with(b"PK"));

    // The assembly of the M3 example: every component where it is, in one file.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m3-pivot");
    for f in ["pivot.tenonasm", "base.tenon", "arm.tenon", "pin.tenon", "block.tenon"] {
        std::fs::copy(src.join(f), dir.join(f)).unwrap();
    }
    wb.open(&dir.join("pivot.tenonasm")).unwrap();
    d.settle(&mut wb);
    let shown: f64 = wb.scene().bodies.iter().map(|b| b.volume).sum();
    let solids = wb.scene().bodies.len();
    assert!(solids >= 3 && shown > 0.0);
    for (entry, file) in [("export.stl", "pivot.stl"), ("export.3mf", "pivot.3mf")] {
        save_to(&mut wb, &dir.join(file));
        d.click(&mut wb, pressable(&d, "tn_file_tab"));
        d.frame(&mut wb, vec![]);
        d.click(&mut wb, pressable(&d, entry));
        frames(&mut d, &mut wb);
        assert!(!wb.status_error && wb.status().starts_with("Exported ") && wb.status().contains(file), "{entry}: {}", wb.status());
    }
    let (volume, triangles) = stl_volume(&dir.join("pivot.stl"));
    assert!((volume - shown).abs() < 0.01 * shown && triangles >= 12 * solids, "{volume} vs {shown}");
    assert!(std::fs::read(dir.join("pivot.3mf")).unwrap().starts_with(b"PK"));
    // The print check is a part's: in an assembly it says where to go.
    let err = wb.run_ui("inspect.print").unwrap_err();
    assert!(err.contains("double-click a component"), "{err}");
}

#[test]
fn the_print_check_and_the_exports_run_on_the_geometry_thread() {
    let _quiet = crate::tests::timing_lock();
    let dir = scratch("worker");
    let mut wb = Workbench::new(|| Box::new(OcctKernel::new()), None, crate::Services::default());
    let mut d = Driver::new(vec2(1400.0, 860.0));
    let settle = |d: &mut Driver, wb: &mut Workbench| {
        for _ in 0..1200 {
            d.frame(wb, vec![]);
            if !wb.is_busy() {
                d.frame(wb, vec![]);
                if !wb.is_busy() {
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the geometry thread never finished");
    };
    table(&mut wb);
    settle(&mut d, &mut wb);
    wb.run_ui("inspect.print").unwrap();
    settle(&mut d, &mut wb);
    let report = wb.print_report().expect("checked on the worker").clone();
    assert_eq!((report.overhangs.len(), report.watertight()), (1, true), "{report:?}");
    // The face it names is the one the view shows there.
    let under = Pick::Face { body: 0, face: report.overhangs[0].face };
    assert_eq!(wb.highlights(), vec![(under, OVERHANG)]);
    let (_, face) = &wb.scene().bodies[0].faces[report.overhangs[0].face as usize];
    assert!((face.centroid.z - 20.0).abs() < 1e-9 && (face.area - 700.0).abs() < 1e-6, "{face:?}");
    // A change of the part is checked again once it is rebuilt.
    wb.run_ui("edit.undo").unwrap();
    settle(&mut d, &mut wb);
    assert!(wb.print_report().is_some_and(|r| r.ok() && (r.height - 20.0).abs() < 1e-9), "{:?}", wb.chrome.print.report);
    wb.run_ui("edit.redo").unwrap();
    settle(&mut d, &mut wb);

    for (entry, file) in [("export.stl", "table.stl"), ("export.3mf", "table.3mf")] {
        save_to(&mut wb, &dir.join(file));
        wb.run_ui(entry).unwrap();
        assert!(wb.is_busy() && wb.status().starts_with("Exporting"), "{}", wb.status());
        settle(&mut d, &mut wb);
        assert!(!wb.status_error && wb.status().starts_with("Exported ") && wb.status().contains(file), "{entry}: {}", wb.status());
    }
    let (volume, _) = stl_volume(&dir.join("table.stl"));
    assert!((volume - 3600.0).abs() < 1e-3, "{volume}");
    assert!(std::fs::read(dir.join("table.3mf")).unwrap().starts_with(b"PK"));

    // An assembly, each part regenerated in its own slot of the worker.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m3-pivot");
    for f in ["pivot.tenonasm", "base.tenon", "arm.tenon", "pin.tenon", "block.tenon"] {
        std::fs::copy(src.join(f), dir.join(f)).unwrap();
    }
    wb.open(&dir.join("pivot.tenonasm")).unwrap();
    settle(&mut d, &mut wb);
    let shown: f64 = wb.scene().bodies.iter().map(|b| b.volume).sum();
    for (entry, file) in [("export.stl", "pivot.stl"), ("export.3mf", "pivot.3mf")] {
        save_to(&mut wb, &dir.join(file));
        wb.run_ui(entry).unwrap();
        settle(&mut d, &mut wb);
        assert!(!wb.status_error && wb.status().starts_with("Exported ") && wb.status().contains(file), "{entry}: {}", wb.status());
    }
    let (volume, _) = stl_volume(&dir.join("pivot.stl"));
    assert!(shown > 0.0 && (volume - shown).abs() < 0.01 * shown, "{volume} vs {shown}");
    assert!(std::fs::read(dir.join("pivot.3mf")).unwrap().starts_with(b"PK"));
}

//! What is on the menu does something: every entry of every ribbon, run with a suitable document
//! open on a real geometry thread. The owner, 2026-11-03: "Make sure all the features on the menu
//! are available not grayed out".
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use egui::vec2;
use serde_json::json;
use tenon_geom::Vec3;
use tenon_kernel_occt::OcctKernel;

use crate::commands::{ANALYSIS_TAB, ASM_RIBBON, DRW_RIBBON, FLAT_TAB, RIBBON, RibbonTab, SHEET_TAB, SKETCH_TAB, SKETCH3D_TAB};
use crate::tests::Driver;
use crate::workbench::Mode;
use crate::{Services, Workbench};

/// What no command on the menu may answer.
const NOT_THERE: &[&str] = &["unknown command", "not available", "arrives in", "not there yet", "milestone", "no solid"];

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-menu-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The window, with its file dialogs answered as someone would.
fn window(dir: &Path) -> Workbench {
    let (other, named, like, name, of) =
        (dir.join("other.tenon"), dir.join("other.tenon"), dir.join("plan"), dir.join("written"), dir.join("written"));
    std::fs::create_dir_all(&name).unwrap();
    let services = Services {
        pick_open: Some(Box::new(move || Some(other.clone()))),
        pick_open_ext: Some(Box::new(move |ext| Some(if ext == "tenon" { named.clone() } else { like.with_extension(ext) }))),
        pick_save: Some(Box::new(move |file, _| Some(name.join(file)))),
        pick_save_of: Some(Box::new(move |file, kinds| Some(of.join(format!("{file}.{}", kinds.first().copied().unwrap_or("stl")))))),
        pick_program: None,
    };
    Workbench::new(|| Box::new(OcctKernel::new()), None, services)
}

/// Until the geometry thread has nothing to do and the view has come to rest.
fn idle(d: &mut Driver, wb: &mut Workbench) {
    // (The view's animation runs only where the view is drawn.)
    let rest = |wb: &Workbench| !wb.is_busy() && (wb.view.anim.is_none() || wb.flat_shown() || wb.in_drawing());
    for _ in 0..4000 {
        d.frame(wb, vec![]);
        if rest(wb) {
            d.frame(wb, vec![]);
            if rest(wb) {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the geometry thread never finished: {}", wb.status());
}

/// A block with a sketch on it that nothing uses, of a circle and a centre point, and one to edit.
fn part_file(path: &Path) {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let sketch = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].clone();
    wb.exec("sketch.rectangle", json!({ "sketch": sketch, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    wb.exec("model.extrude", json!({ "sketch": sketch, "distance": 10 })).unwrap();
    let on_it = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].clone();
    wb.exec("sketch.circle", json!({ "sketch": on_it, "cx": 20, "cy": 10, "r": 3 })).unwrap();
    wb.exec("sketch.point", json!({ "sketch": on_it, "x": 5, "y": 5 })).unwrap();
    wb.save(path).unwrap();
}

/// A plate of sheet with a flange on it.
fn sheet_file(path: &Path) {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3 })).unwrap();
    let sketch = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].clone();
    let lines = wb.exec("sketch.rectangle", json!({ "sketch": sketch, "x1": 0, "y1": 0, "x2": 40, "y2": 30 })).unwrap()["lines"].clone();
    let face = wb.exec("sheet.face", json!({ "sketch": sketch })).unwrap()["feature"].clone();
    let edge = json!({
        "faces": [{ "type": "cap", "feature": face, "end": "end" }, { "type": "side", "feature": face, "curve": lines[1] }],
        "fingerprint": { "mid": { "x": 40.0, "y": 15.0, "z": 2.0 }, "length": 30.0 },
    });
    wb.exec("sheet.flange", json!({ "edges": [edge], "height": 20 })).unwrap();
    wb.save(path).unwrap();
}

/// Two of the part, the second free to be moved, and a relationship between them.
fn assembly_file(path: &Path, part: &Path) {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.run_ui("file.new_assembly").unwrap();
    for at in [[0.0, 0.0, 0.0], [60.0, 0.0, 0.0]] {
        wb.place_file(part).unwrap();
        d.settle(&mut wb);
        let last = wb.assembly().unwrap().assembly().components.len();
        wb.asm_exec("asm.place", json!({ "component": last, "origin": at })).unwrap_or_else(|e| {
            assert!(e.contains("grounded"), "{e}");
            json!({})
        });
    }
    wb.panel = None;
    wb.save_assembly(path).unwrap();
}

/// A drawing of the part: a view from above and one beside it.
fn drawing_file(path: &Path, part: &Path) {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.run_ui("file.new_drawing").unwrap();
    let view = wb.drw_exec("drw.view.base", json!({ "model": part.to_string_lossy(), "orientation": "top", "scale": 1, "at": [100, 150] })).unwrap();
    wb.drw_exec("drw.view.projected", json!({ "parent": view["view"], "side": "right", "at": [200, 150] })).unwrap();
    d.frame(&mut wb, vec![]);
    wb.save_drawing(path).unwrap();
}

/// A place in the program: its document, how to get to where the tab's commands are for, and
/// the tabs.
struct Stage {
    name: &'static str,
    file: PathBuf,
    enter: fn(&mut Workbench, &mut Driver),
    ribbon: &'static [RibbonTab],
    tabs: Vec<usize>,
}

/// What a command has to work on that a document just opened does not have: without it the window
/// has no cause to change.
fn prepare(wb: &mut Workbench, d: &mut Driver, id: &str) {
    match id {
        "view.style.shaded_edges" => wb.run_ui("view.style.wireframe").unwrap(),
        "view.perspective" => wb.run_ui("view.orthographic").unwrap(),
        "asm.explode.clear" => wb.run_ui("asm.explode.auto").unwrap(),
        "view.fit" if wb.flat_shown() || wb.in_drawing() => {
            d.frame(wb, vec![egui::Event::PointerMoved(egui::pos2(700.0, 500.0))]);
            let phase = egui::TouchPhase::Move;
            let wheel =
                egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, 120.0), phase, modifiers: egui::Modifiers::NONE };
            d.frame(wb, vec![wheel]);
        }
        _ => {}
    }
    idle(d, wb);
}

/// Runs `id` with the window as it should be for it. What comes of it, and whether the window then
/// shows anything other than it did.
fn press(wb: &mut Workbench, d: &mut Driver, stage: &Stage, tab: usize, id: &str) -> (Result<(), String>, bool) {
    wb.chrome = crate::chrome::Chrome::default();
    wb.open(&stage.file).unwrap_or_else(|e| panic!("{}: {e}", stage.name));
    idle(d, wb);
    (stage.enter)(wb, d);
    idle(d, wb);
    wb.chrome.tab = tab;
    // (A view of its own, so that a command to look at the part has something to change.)
    if !wb.flat_shown() && !wb.in_drawing() {
        wb.look_from(Vec3::new(1.0, -2.0, 0.5));
        idle(d, wb);
    }
    prepare(wb, d, id);
    wb.set_status("Before");
    d.frame(wb, vec![]);
    let before = d.drawn();
    let said = wb.run_ui(id);
    if let Err(e) = &said {
        wb.set_error(e.clone());
    }
    idle(d, wb);
    (said, d.drawn() != before)
}

#[test]
fn every_command_on_the_menu_does_something() {
    let _quiet = crate::tests::timing_lock();
    let dir = scratch("every");
    let (part, sheet, assembly, drawing) = (dir.join("part.tenon"), dir.join("sheet.tenon"), dir.join("pair.tenonasm"), dir.join("plan.tenondrw"));
    part_file(&part);
    part_file(&dir.join("other.tenon"));
    sheet_file(&sheet);
    assembly_file(&assembly, &part);
    drawing_file(&drawing, &part);

    let special = [SKETCH_TAB, SHEET_TAB, FLAT_TAB, ANALYSIS_TAB, SKETCH3D_TAB];
    let stages = [
        Stage {
            name: "a part",
            file: part.clone(),
            enter: |_, _| {},
            ribbon: RIBBON,
            tabs: (0..RIBBON.len()).filter(|i| !special.contains(i)).collect(),
        },
        Stage {
            name: "a sketch",
            file: part.clone(),
            enter: |wb, _| {
                wb.edit_sketch_named("Sketch2").unwrap();
                // (The circle, for the tools that work on what is selected.)
                let Mode::Sketch(s) = &mut wb.mode else { panic!("not in the sketch") };
                let sketch = wb.session.document().sketch(s.feature).unwrap();
                s.selection =
                    sketch.entities().filter(|(_, e)| matches!(e.geometry, tenon_sketch::Geometry::Circle { .. })).map(|(id, _)| id).collect();
            },
            ribbon: RIBBON,
            tabs: vec![SKETCH_TAB],
        },
        Stage {
            name: "a 3D sketch",
            file: part.clone(),
            enter: |wb, _| wb.run_ui("sketch.new_3d").unwrap(),
            ribbon: RIBBON,
            tabs: vec![SKETCH3D_TAB],
        },
        Stage { name: "a sheet metal part", file: sheet.clone(), enter: |_, _| {}, ribbon: RIBBON, tabs: vec![SHEET_TAB] },
        Stage {
            name: "a flat pattern",
            file: sheet.clone(),
            enter: |wb, _| wb.run_ui("sheet.flat_view").unwrap(),
            ribbon: RIBBON,
            tabs: vec![FLAT_TAB],
        },
        Stage { name: "a study", file: part.clone(), enter: |wb, _| wb.run_ui("analysis.begin").unwrap(), ribbon: RIBBON, tabs: vec![ANALYSIS_TAB] },
        Stage {
            name: "an assembly",
            file: assembly.clone(),
            enter: |wb, _| {
                if let Some(a) = wb.asm.as_mut() {
                    a.selected = vec![tenon_assembly::ComponentId(2)];
                }
            },
            ribbon: ASM_RIBBON,
            tabs: (0..ASM_RIBBON.len()).collect(),
        },
        Stage { name: "a drawing", file: drawing.clone(), enter: |_, _| {}, ribbon: DRW_RIBBON, tabs: (0..DRW_RIBBON.len()).collect() },
    ];

    let mut wb = window(&dir);
    let mut d = Driver::new(vec2(1400.0, 860.0));
    let mut ran = std::collections::BTreeSet::new();
    let mut wrong = Vec::new();
    let mut nothing = Vec::new();
    for stage in &stages {
        for tab in &stage.tabs {
            let t = &stage.ribbon[*tab];
            for c in t.panels.iter().flat_map(|p| p.commands.iter()) {
                let more = c.more.iter().copied().filter(|m| *m != c.id);
                for id in std::iter::once(c.id).chain(more) {
                    ran.insert(id);
                    let (said, changed) = press(&mut wb, &mut d, stage, *tab, id);
                    let words = said.as_ref().err().cloned().unwrap_or_else(|| wb.status().to_owned());
                    if NOT_THERE.iter().any(|w| words.contains(w)) {
                        wrong.push(format!("{} > {} > {id}: {words}", stage.name, t.name));
                    }
                    if !changed {
                        nothing.push(format!("{} > {} > {id}", stage.name, t.name));
                    }
                }
            }
        }
    }
    assert!(wrong.is_empty(), "these answer that they are not there:\n{}", wrong.join("\n"));
    assert!(nothing.is_empty(), "the window is the same after these:\n{}", nothing.join("\n"));
    // And that went through the lot.
    let on_ribbons = RIBBON.iter().chain(ASM_RIBBON).chain(DRW_RIBBON).flat_map(|t| t.panels.iter()).flat_map(|p| p.commands.iter()).map(|c| c.id);
    let missed: Vec<&str> = on_ribbons.filter(|id| !ran.contains(id)).collect();
    assert!(missed.is_empty(), "{missed:?}");
    let _ = std::fs::remove_dir_all(dir);
}

/// The disabled colours of a theme, as a frame's shapes are written out.
fn disabled_colours(theme: crate::theme::ThemeName) -> [String; 2] {
    let t = crate::theme::Tokens::of(theme);
    [format!("{:?}", t.icon_disabled), format!("{:?}", t.text_disabled)]
}

#[test]
fn nothing_in_the_ribbon_or_the_strips_is_painted_as_if_out_of_use() {
    use crate::chrome::{DOC_TABS_H, RIBBON_H, STATUS_H, TABS_H, TITLE_H};
    use crate::theme::ThemeName;
    let size = vec2(1400.0, 860.0);
    let top = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(size.x, TITLE_H + TABS_H + RIBBON_H));
    let bottom = egui::Rect::from_min_max(egui::pos2(0.0, size.y - STATUS_H - DOC_TABS_H), size.to_pos2());
    let documents: [Option<&str>; 3] = [None, Some("file.new_assembly"), Some("file.new_drawing")];
    for theme in [ThemeName::Light, ThemeName::Dark] {
        for new in documents {
            let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
            let mut d = Driver::new(size);
            wb.chrome.theme = theme;
            if let Some(id) = new {
                wb.run_ui(id).unwrap();
            }
            d.frame(&mut wb, vec![]);
            for tab in 0..wb.ribbon_def().len() {
                wb.chrome.tab = tab;
                d.frame(&mut wb, vec![]);
                let (above, below) = (d.drawn_in(top), d.drawn_in(bottom));
                assert!(above.len() > 1000 && below.len() > 100, "{new:?}: the frame has drawn the strips");
                for colour in disabled_colours(theme) {
                    assert!(!above.contains(&colour), "{new:?}, tab {tab}: something on the ribbon is painted disabled");
                    assert!(!below.contains(&colour), "{new:?}: something in the strip of documents is painted disabled");
                }
            }
        }
    }
}

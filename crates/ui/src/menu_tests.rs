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

/// Where the last frame drew something to press (see `drawing::remember`).
fn drawn_at(d: &Driver, key: &str) -> Option<egui::Rect> {
    d.ctx.data(|x| x.get_temp::<std::collections::BTreeMap<String, egui::Rect>>(egui::Id::new("tn_buttons"))).unwrap_or_default().get(key).copied()
}

/// Opens the list of button `id` by the pointer, going along the ribbon by its arrows as far as
/// the button lies beyond the window.
fn open_list(d: &mut Driver, wb: &mut Workbench, id: &str, width: f32) {
    d.frame(wb, vec![]);
    for _ in 0..40 {
        let on = drawn_at(d, &format!("ribbon {id}")).expect("the button is on this tab");
        if on.right() < width - 24.0 && (on.left() > 24.0 || wb.chrome.ribbon_scroll <= 0.0) {
            break;
        }
        let way = if on.right() >= width - 24.0 { "tn_ribbon_on" } else { "tn_ribbon_back" };
        let at = crate::tests::pressable(d, way);
        d.click(wb, at);
        d.frame(wb, vec![]);
    }
    let on = drawn_at(d, &format!("ribbon {id}")).expect("the button is on this tab");
    let (x, y) = match crate::commands::find(id).expect("a command").size {
        crate::commands::Size::Large => (on.center().x, on.bottom() - 6.0),
        _ => (on.right() - 5.0, on.center().y),
    };
    d.click(wb, egui::pos2(x, y));
    // (A list new on the screen is measured in its first frame and drawn in its second.)
    d.frame(wb, vec![]);
    d.frame(wb, vec![]);
    assert!(wb.chrome.dropdown.is_some(), "{id}: the arrow opens the list");
}

/// The menu at any width: a list that the window's edge pushes to the left takes a click on its
/// rows as one that stays under its button does. (The rows lit up under the pointer, and a press on
/// their words closed the list with nothing run.)
#[test]
fn every_row_of_every_list_takes_a_click_at_any_width() {
    let _quiet = crate::tests::timing_lock();
    let dir = scratch("lists");
    let (part, sheet, assembly, drawing) = (dir.join("part.tenon"), dir.join("sheet.tenon"), dir.join("pair.tenonasm"), dir.join("plan.tenondrw"));
    part_file(&part);
    part_file(&dir.join("other.tenon"));
    sheet_file(&sheet);
    assembly_file(&assembly, &part);
    drawing_file(&drawing, &part);
    type Enter = fn(&mut Workbench);
    let places: [(&PathBuf, Enter, &[RibbonTab], Vec<usize>); 6] = [
        (&part, |_| {}, RIBBON, vec![0, SKETCH_TAB, 5]),
        (&part, |wb| wb.edit_sketch_named("Sketch2").unwrap(), RIBBON, vec![SKETCH_TAB]),
        (&sheet, |_| {}, RIBBON, vec![SHEET_TAB]),
        (&part, |wb| wb.run_ui("analysis.begin").unwrap(), RIBBON, vec![ANALYSIS_TAB]),
        (&assembly, |_| {}, ASM_RIBBON, vec![4]),
        (&drawing, |_| {}, DRW_RIBBON, vec![1]),
    ];
    let mut tried = std::collections::BTreeSet::new();
    let mut wrong = Vec::new();
    for width in [900.0, 1440.0] {
        let mut wb = window(&dir);
        let mut d = Driver::new(vec2(width, 900.0));
        for (file, enter, ribbon, tabs) in &places {
            for tab in tabs {
                for c in ribbon[*tab].panels.iter().flat_map(|p| p.commands.iter()).filter(|c| !c.more.is_empty()) {
                    for row in c.more {
                        // On the row's words, and at its far end.
                        for along in [36.0, -6.0] {
                            wb.chrome = crate::chrome::Chrome::default();
                            wb.open(file).unwrap();
                            idle(&mut d, &mut wb);
                            enter(&mut wb);
                            idle(&mut d, &mut wb);
                            wb.chrome.tab = *tab;
                            open_list(&mut d, &mut wb, c.id, width);
                            let on = drawn_at(&d, &format!("list {row}")).unwrap_or_else(|| panic!("{row} is not in the list of {}", c.id));
                            let x = if along > 0.0 { on.left() + along } else { on.right() + along };
                            wb.ran.clear();
                            d.click(&mut wb, egui::pos2(x, on.center().y));
                            tried.insert(*row);
                            if wb.ran != [*row] {
                                wrong.push(format!("at {width} wide, {} > {row} at x={x}: ran {:?}", c.id, wb.ran));
                            }
                            assert!(wb.chrome.dropdown.is_none(), "{row}: a choice closes the list");
                        }
                    }
                }
            }
        }
    }
    assert!(wrong.is_empty(), "a click on these rows ran nothing, or something else:\n{}", wrong.join("\n"));
    assert!(tried.contains("analysis.deform.actual") && tried.contains("drw.balloon.auto") && tried.contains("model.face_fillet"), "{tried:?}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A list at the window's edge is drawn further left than it was asked to open, and that is where
/// it takes a click, and where a press is not outside it.
#[test]
fn a_list_the_window_pushes_left_takes_a_click_on_its_words() {
    let mut wb = Workbench::without_kernel();
    let mut d = Driver::new(vec2(520.0, 900.0));
    open_list(&mut d, &mut wb, "model.fillet", 520.0);
    let button = drawn_at(&d, "ribbon model.fillet").unwrap();
    let on = drawn_at(&d, "list model.face_fillet").unwrap();
    assert!(on.left() < button.left(), "the list has moved: {on:?} from under {button:?}");
    let at = egui::pos2(on.left() + 36.0, on.center().y);
    assert!(at.x < button.left(), "this is left of where it was asked to open");
    d.click(&mut wb, at);
    assert_eq!(wb.ran, ["model.face_fillet"]);
    // Pressed beside it, it closes and nothing runs.
    open_list(&mut d, &mut wb, "model.fillet", 520.0);
    wb.ran.clear();
    let on = drawn_at(&d, "list model.face_fillet").unwrap();
    d.click(&mut wb, egui::pos2(on.left() - 20.0, on.bottom() + 40.0));
    assert!(wb.chrome.dropdown.is_none() && wb.ran.is_empty());
}

/// A list of one is as wide as what is on it, and leaves the browser beside it alone.
#[test]
fn a_list_is_as_wide_as_its_longest_row() {
    let mut wb = Workbench::without_kernel();
    let mut d = Driver::new(vec2(1440.0, 900.0));
    open_list(&mut d, &mut wb, "sketch.new", 1440.0);
    let one = drawn_at(&d, "list sketch.new_3d").unwrap();
    assert!(one.width() < 200.0, "one short row: {} wide", one.width());
    wb.chrome.dropdown = None;
    wb.chrome.tab = SKETCH_TAB;
    open_list(&mut d, &mut wb, "sketch.rectangle", 1440.0);
    let long = drawn_at(&d, "list sketch.rectangle_center").unwrap();
    let words = d.text_rect("Rectangle Two Point Center").unwrap();
    assert!(words.right() <= long.right(), "the longest row holds its words: {words:?} in {long:?}");
    assert!(long.width() < 300.0, "{long:?}");
}

/// A block 40 x 20 x 10 with a round boss on its top, and a sketch on nothing yet with a circle in it.
fn part_with_a_boss(path: &Path) {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let sketch = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].clone();
    wb.exec("sketch.rectangle", json!({ "sketch": sketch, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    wb.exec("model.extrude", json!({ "sketch": sketch, "distance": 10 })).unwrap();
    let boss = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].clone();
    wb.exec("sketch.circle", json!({ "sketch": boss, "cx": 30, "cy": 10, "r": 4 })).unwrap();
    wb.exec("model.extrude", json!({ "sketch": boss, "distance": 20 })).unwrap();
    let open = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].clone();
    wb.exec("sketch.circle", json!({ "sketch": open, "cx": 10, "cy": 10, "r": 3 })).unwrap();
    wb.save(path).unwrap();
}

/// Presses \`id\`: its button on the tab shown, or its row under the arrow it is under.
fn press_it(d: &mut Driver, wb: &mut Workbench, id: &str, width: f32) {
    d.frame(wb, vec![]);
    if let Some(on) = drawn_at(d, &format!("ribbon {id}")) {
        d.click(wb, egui::pos2(on.center().x, on.top() + 8.0));
        return;
    }
    let above = wb.ribbon_def()[wb.chrome.tab].panels.iter().flat_map(|p| p.commands.iter()).find(|c| c.more.contains(&id));
    let above = above.unwrap_or_else(|| panic!("{id} is not on this tab")).id;
    open_list(d, wb, above, width);
    let row = drawn_at(d, &format!("list {id}")).unwrap();
    d.click(wb, row.center());
}

/// Fillet and the rest in a 2D sketch said "click edges", and no click reached them: they leave the
/// sketch first, as they do a 3D sketch, and a click on the part then does what they say.
#[test]
fn what_picks_on_the_part_leaves_a_sketch_and_takes_clicks() {
    let _quiet = crate::tests::timing_lock();
    let dir = scratch("in-sketch");
    let part = dir.join("boss.tenon");
    part_with_a_boss(&part);
    part_with_a_boss(&dir.join("other.tenon"));
    let mut wb = window(&dir);
    let mut d = Driver::new(vec2(1400.0, 860.0));
    let ids = [
        "model.fillet",
        "model.face_fillet",
        "model.chamfer",
        "model.shell",
        "model.draft",
        "model.thread",
        "model.split",
        "model.box",
        "model.cylinder",
        "model.sphere",
        "model.torus",
        "model.pattern.rect",
        "model.pattern.circular",
        "model.mirror",
        "work.plane",
        "work.axis",
        "work.point",
        "work.ucs",
        "inspect.measure",
    ];
    for id in ids {
        wb.chrome = crate::chrome::Chrome::default();
        wb.open(&part).unwrap();
        idle(&mut d, &mut wb);
        wb.edit_sketch_named("Sketch3").unwrap();
        idle(&mut d, &mut wb);
        wb.chrome.tab = if id == "inspect.measure" { 2 } else { 0 };
        press_it(&mut d, &mut wb, id, 1400.0);
        idle(&mut d, &mut wb);
        assert!(!wb.is_sketching(), "{id}: the sketch is left");
        assert!(wb.panel.is_some(), "{id}: its panel is open, and said: {}", wb.status());
        assert!(!wb.status().starts_with("Finished"), "{id} says what it wants: {}", wb.status());
        assert_eq!(wb.chrome.tab, if id == "inspect.measure" { 2 } else { 0 }, "{id}: the tab stays");
        // (A UCS is typed until Three Points is chosen.)
        if id == "work.ucs" {
            continue;
        }
        wb.look_from(Vec3::new(1.0, -2.0, 1.5));
        idle(&mut d, &mut wb);
        // What they take between them: a straight edge, a flat face, the round of the boss, and a
        // round edge of it. (Each takes something among them.)
        let before = format!("{:?}", wb.panel);
        let mut took = false;
        for at in [Vec3::new(10.0, 0.0, 10.0), Vec3::new(8.0, 10.0, 10.0), Vec3::new(30.0, 6.0, 20.0), Vec3::new(30.0, 6.0, 15.0)] {
            let p = crate::tests::on_screen(&wb, at);
            d.click(&mut wb, p);
            idle(&mut d, &mut wb);
            took |= format!("{:?}", wb.panel) != before;
        }
        assert!(took, "{id}: a click on the part changed nothing, and it said: {}", wb.status());
    }
    // And on a sheet metal part, by the Sheet Metal tab.
    let sheet = dir.join("sheet.tenon");
    sheet_file(&sheet);
    for id in ["sheet.flange", "sheet.hem", "sheet.corner_round", "sheet.corner_chamfer", "sheet.rip", "sheet.unfold"] {
        wb.chrome = crate::chrome::Chrome::default();
        wb.open(&sheet).unwrap();
        idle(&mut d, &mut wb);
        wb.edit_sketch_named("Sketch1").unwrap();
        idle(&mut d, &mut wb);
        wb.chrome.tab = SHEET_TAB;
        press_it(&mut d, &mut wb, id, 1400.0);
        idle(&mut d, &mut wb);
        assert!(!wb.is_sketching() && wb.panel.is_some(), "{id}: {}", wb.status());
        wb.look_from(Vec3::new(1.0, -2.0, 1.5));
        idle(&mut d, &mut wb);
        let before = format!("{:?}", wb.panel);
        let mut took = false;
        for at in [Vec3::new(20.0, 0.0, 2.0), Vec3::new(0.0, 0.0, 1.0), Vec3::new(20.0, 15.0, 2.0)] {
            let p = crate::tests::on_screen(&wb, at);
            d.click(&mut wb, p);
            idle(&mut d, &mut wb);
            took |= format!("{:?}", wb.panel) != before;
        }
        assert!(took, "{id}: a click on the part changed nothing, and it said: {}", wb.status());
    }
    // Refused, it stays in the sketch.
    wb.run_unprompted("file.new").unwrap();
    let sk = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].clone();
    idle(&mut d, &mut wb);
    wb.edit_sketch_named(wb.document().feature(tenon_model::FeatureId(sk.as_u64().unwrap() as u32)).unwrap().name.clone().as_str()).unwrap();
    assert!(wb.run_unprompted("model.fillet").is_err(), "there is nothing to fillet");
    assert!(wb.is_sketching(), "a command refused leaves things as they were");
    let _ = std::fs::remove_dir_all(dir);
}

/// The same from the browser: a feature's row double-clicked in a sketch.
#[test]
fn a_feature_opened_from_the_browser_leaves_a_sketch() {
    let _quiet = crate::tests::timing_lock();
    let dir = scratch("in-sketch-browser");
    let part = dir.join("boss.tenon");
    part_with_a_boss(&part);
    let mut wb = window(&dir);
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.open(&part).unwrap();
    idle(&mut d, &mut wb);
    wb.edit_sketch_named("Sketch3").unwrap();
    idle(&mut d, &mut wb);
    let extrusion = wb.document().features().iter().find(|f| f.name == "Extrusion2").unwrap().id;
    wb.edit_feature(extrusion).unwrap();
    assert!(!wb.is_sketching() && wb.panel.is_some());
    let _ = std::fs::remove_dir_all(dir);
}

/// Start 2D Sketch in a sketch said "Select a plane (Esc cancels)", and neither Esc nor a click did
/// anything: it waited on through Finish Sketch, and took the next click on the part for a new sketch.
#[test]
fn start_2d_sketch_in_a_sketch_finishes_it_and_then_asks_for_a_plane() {
    let _quiet = crate::tests::timing_lock();
    let dir = scratch("s-in-sketch");
    let part = dir.join("boss.tenon");
    part_with_a_boss(&part);
    let mut wb = window(&dir);
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.open(&part).unwrap();
    idle(&mut d, &mut wb);
    wb.edit_sketch_named("Sketch3").unwrap();
    idle(&mut d, &mut wb);
    let count = wb.document().features().len();
    let middle = wb.view.rect.center();
    d.frame(&mut wb, vec![egui::Event::PointerMoved(middle)]);
    d.tap(&mut wb, egui::Key::S);
    idle(&mut d, &mut wb);
    assert!(!wb.is_sketching() && wb.pick_plane, "{}", wb.status());
    assert!(wb.status().contains("Select a plane"), "{}", wb.status());
    // Esc cancels, as it says, and the next click on the part is only a click.
    d.tap(&mut wb, egui::Key::Escape);
    idle(&mut d, &mut wb);
    assert!(!wb.pick_plane);
    wb.look_from(Vec3::new(1.0, -2.0, 1.5));
    idle(&mut d, &mut wb);
    let top = crate::tests::on_screen(&wb, Vec3::new(8.0, 10.0, 10.0));
    d.click(&mut wb, top);
    idle(&mut d, &mut wb);
    assert!(!wb.is_sketching() && wb.document().features().len() == count, "{}", wb.status());
    // Asked again, a click on the part is the plane.
    wb.view.selection.clear();
    d.tap(&mut wb, egui::Key::S);
    idle(&mut d, &mut wb);
    d.click(&mut wb, top);
    idle(&mut d, &mut wb);
    assert!(wb.is_sketching() && wb.document().features().len() == count + 1, "{}", wb.status());
    // A plane being asked for goes with what it was asked of.
    wb.finish_sketch();
    idle(&mut d, &mut wb);
    wb.view.selection.clear();
    wb.run_ui("sketch.new").unwrap();
    assert!(wb.pick_plane);
    wb.edit_sketch_named("Sketch3").unwrap();
    d.frame(&mut wb, vec![]);
    assert!(!wb.pick_plane, "a sketch opened from the browser takes it away");
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

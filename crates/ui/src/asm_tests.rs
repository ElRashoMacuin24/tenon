//! Assemblies through the UI, with real pointer and key input: opening, dragging along joints,
//! constraining and jointing by clicking faces, the degrees of freedom, interference, the parts
//! list, the exploded view and editing a part in place.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use egui::vec2;
use serde_json::json;
use tenon_assembly::ComponentId;
use tenon_geom::Vec3;
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;

use crate::Workbench;
use crate::asm_panel::{AsmTool, ConstraintType};
use crate::panels::Panel;
use crate::tests::{Driver, browser_row, on_screen};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-ui-asm-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The M3 demo assembly (examples/m3-pivot), copied so tests may change it.
fn pivot(name: &str) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m3-pivot");
    let dir = scratch(name);
    for f in ["pivot.tenonasm", "base.tenon", "arm.tenon", "pin.tenon", "block.tenon"] {
        std::fs::copy(src.join(f), dir.join(f)).unwrap();
    }
    dir.join("pivot.tenonasm")
}

/// A box part file, `x` by `y` by `z`, with a corner at the origin.
fn box_part(path: &Path, name: &str, x: f64, y: f64, z: f64) {
    let mut s = Session::default();
    let run = |s: &mut Session, id: &str, p: serde_json::Value| tenon_io::cmd::run(s, id, &p, None).unwrap();
    run(&mut s, "document.rename", json!({ "name": name }));
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": x, "y2": y }));
    run(&mut s, "model.extrude", json!({ "sketch": sk, "distance": z }));
    tenon_io::project::save(path, s.document(), &serde_json::Map::new()).unwrap();
}

fn placement(wb: &Workbench, id: u32) -> tenon_geom::Frame {
    wb.assembly().unwrap().assembly().component(ComponentId(id)).unwrap().placement
}

#[test]
fn an_assembly_opens_and_components_drag_along_their_joints() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.open(&pivot("drag")).unwrap();
    d.settle(&mut wb);
    assert!(wb.in_assembly());
    assert_eq!(wb.scene().bodies.len(), 5, "one body per component");
    for row in ["Pivot", "Relationships (5)", "Rotational:1", "Slider:1", "base:1", "block:1"] {
        browser_row(&d, row);
    }
    assert_eq!(wb.asm_status().unwrap(), "5 components      3 DOF");

    // Hovering the block highlights it whole.
    let top = on_screen(&wb, Vec3::new(40.0, 20.0, 20.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(top)]);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.asm.as_ref().unwrap().hover, Some(ComponentId(5)));

    // Dragging the block: its slider keeps it on the base's front edge; only X changes.
    let to = on_screen(&wb, Vec3::new(58.0, 26.0, 24.0));
    d.drag(&mut wb, top, to, egui::PointerButton::Primary);
    d.frame(&mut wb, vec![]);
    let f = placement(&wb, 5);
    assert!(f.origin().x > 35.0, "{f:?}");
    assert!(f.origin().y.abs() < 1e-6 && (f.origin().z - 10.0).abs() < 1e-6, "{f:?}");
    assert!(f.x().near(Vec3::X, 1e-6), "not turned");
    // One drag is one undo step.
    wb.run_ui("edit.undo").unwrap();
    assert!((placement(&wb, 5).origin().x - 30.0).abs() < 1e-6);

    // The grounded base does not move.
    let base = on_screen(&wb, Vec3::new(70.0, 30.0, 10.0));
    d.drag(&mut wb, base, base + vec2(60.0, 10.0), egui::PointerButton::Primary);
    d.frame(&mut wb, vec![]);
    assert!(wb.status().contains("grounded"), "{}", wb.status());
    assert!(placement(&wb, 1).origin().near(Vec3::ZERO, 1e-12));

    // Free Rotate turns the second pin about its own axis only (its insert holds the rest).
    wb.run_ui("asm.free_rotate").unwrap();
    let head = on_screen(&wb, Vec3::new(60.0, 20.0, 13.0));
    d.drag(&mut wb, head, head + vec2(40.0, 25.0), egui::PointerButton::Primary);
    d.frame(&mut wb, vec![]);
    let f = placement(&wb, 4);
    assert!(f.z().near(Vec3::Z, 1e-6) && f.origin().near(Vec3::new(60.0, 20.0, 10.0), 1e-6), "{f:?}");
}

#[test]
fn constrain_and_joint_by_clicking_faces_in_the_view() {
    let dir = scratch("relate");
    box_part(&dir.join("base.tenon"), "Base", 40.0, 40.0, 10.0);
    box_part(&dir.join("cube.tenon"), "Cube", 10.0, 10.0, 10.0);
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.run_ui("file.new_assembly").unwrap();
    wb.place_file(&dir.join("base.tenon")).unwrap();
    d.settle(&mut wb);
    wb.place_file(&dir.join("cube.tenon")).unwrap();
    d.settle(&mut wb);
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);
    let cube0 = placement(&wb, 2);
    assert!(cube0.origin().x > 40.0, "placed beside the base: {cube0:?}");

    // C opens Constrain. Flush, 5 apart: click the base's top, then the cube's top.
    d.tap(&mut wb, egui::Key::C);
    assert!(matches!(&wb.panel, Some(Panel::Asm(p)) if p.tool == AsmTool::Constrain));
    if let Some(Panel::Asm(p)) = &mut wb.panel {
        p.constraint = ConstraintType::Flush;
        p.offset = 5.0;
    }
    let at = on_screen(&wb, Vec3::new(20.0, 20.0, 10.0));
    d.click(&mut wb, at);
    let c = cube0.origin();
    let at = on_screen(&wb, c + Vec3::new(5.0, 5.0, 10.0));
    d.click(&mut wb, at);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Asm(p)) = &wb.panel else { panic!("panel closed") };
    assert!(p.a.as_ref().unwrap().1.starts_with("base:1"), "{:?}", p.a);
    assert!(p.b.as_ref().unwrap().1.starts_with("cube:1"), "{:?}", p.b);
    // The view already shows the cube 5 up.
    assert!(matches!(&p.preview, Some((_, Ok(f))) if (f[&ComponentId(2)].origin().z - 5.0).abs() < 1e-6), "{:?}", p.preview);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!((placement(&wb, 2).origin().z - 5.0).abs() < 1e-6);
    assert_eq!(wb.asm_dof().unwrap().0[1].count(), 3, "two slides and a turn left");
    browser_row(&d, "Flush:1");

    // Undo, then a rotational joint from the base's top to the cube's top: the cube turns over
    // onto the base and can only spin.
    wb.run_ui("edit.undo").unwrap();
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::J);
    if let Some(Panel::Asm(p)) = &mut wb.panel {
        p.joint = tenon_assembly::JointKind::Revolute;
    }
    let at = on_screen(&wb, Vec3::new(20.0, 20.0, 10.0));
    d.click(&mut wb, at);
    let at = on_screen(&wb, c + Vec3::new(5.0, 5.0, 10.0));
    d.click(&mut wb, at);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let f = placement(&wb, 2);
    assert!(f.z().near(-Vec3::Z, 1e-6), "upside down: {f:?}");
    let corners: Vec<Vec3> =
        (0..8).map(|k| f.to_world(Vec3::new(f64::from(k & 1) * 10.0, f64::from((k >> 1) & 1) * 10.0, f64::from((k >> 2) & 1) * 10.0))).collect();
    let (lo, hi) = corners.iter().fold((f64::MAX, f64::MIN), |(l, h), p| (l.min(p.z), h.max(p.z)));
    assert!((lo - 10.0).abs() < 1e-6 && (hi - 20.0).abs() < 1e-6, "sits on the base: {lo} {hi}");
    let (per, _) = wb.asm_dof().unwrap();
    assert_eq!((per[1].translations.len(), per[1].rotations.len()), (0, 1));
    browser_row(&d, "Rotational:1");

    // A second joint that cannot hold with the first is refused, and nothing changes.
    let before = wb.assembly().unwrap().assembly().clone();
    d.tap(&mut wb, egui::Key::C);
    if let Some(Panel::Asm(p)) = &mut wb.panel {
        p.constraint = ConstraintType::Flush;
        p.offset = 30.0;
    }
    let at = on_screen(&wb, Vec3::new(30.0, 30.0, 10.0));
    d.click(&mut wb, at);
    let ft = placement(&wb, 2).to_world(Vec3::new(5.0, 5.0, 0.0));
    let at = on_screen(&wb, ft);
    d.click(&mut wb, at);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.status_error, "{}", wb.status());
    assert_eq!(wb.assembly().unwrap().assembly(), &before);
    d.tap(&mut wb, egui::Key::Escape);
}

#[test]
fn dof_interference_parts_list_and_explode_from_the_ribbon() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.open(&pivot("inspect")).unwrap();
    d.settle(&mut wb);
    wb.command("asm.dof");
    d.frame(&mut wb, vec![]);
    assert!(wb.asm.as_ref().unwrap().show_dof);
    let (per, total) = wb.asm_dof().unwrap();
    assert_eq!(total, 3);
    assert_eq!(per.iter().map(|d| d.count()).collect::<Vec<_>>(), [0, 0, 1, 1, 1]);

    // No interference where the block is; moved over the second pin's head, 108 pi of it.
    wb.command("asm.interference");
    d.frame(&mut wb, vec![]);
    assert!(matches!(&wb.asm.as_ref().unwrap().clashes, Some(Ok(c)) if c.is_empty()), "{}", wb.status());
    wb.asm_exec("asm.place", json!({ "component": 5, "origin": [50, 0, 10] })).unwrap();
    wb.command("asm.interference");
    d.frame(&mut wb, vec![]);
    let clashes = match &wb.asm.as_ref().unwrap().clashes {
        Some(Ok(c)) => c.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(clashes.len(), 1);
    assert!((clashes[0].volume - 108.0 * PI).abs() < 1e-6, "{}", clashes[0].volume);
    assert!(wb.status().contains("1 interference"), "{}", wb.status());

    // The parts list.
    wb.command("asm.bom");
    d.frame(&mut wb, vec![]);
    let rows = tenon_assembly::session::bom(wb.assembly().unwrap().assembly(), &wb.assembly().unwrap().parts);
    assert_eq!(rows.iter().map(|r| (r.name.as_str(), r.quantity)).collect::<Vec<_>>(), [("Base", 1), ("Arm", 1), ("Pin", 2), ("Block", 1)]);

    // Auto Explode shows the exploded view; the toggle puts it back together.
    let assembled = wb.scene().bbox().unwrap();
    wb.command("asm.explode.auto");
    d.frame(&mut wb, vec![]);
    let exploded = wb.scene().bbox().unwrap();
    assert!(exploded.max.z > assembled.max.z + 20.0, "{exploded:?} vs {assembled:?}");
    wb.command("asm.explode.toggle");
    d.frame(&mut wb, vec![]);
    assert!((wb.scene().bbox().unwrap().max.z - assembled.max.z).abs() < 1e-6);
    assert!(!wb.status_error, "{}", wb.status());
}

#[test]
fn editing_a_part_in_place_and_returning_updates_the_assembly() {
    let path = pivot("edit");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.open(&path).unwrap();
    d.settle(&mut wb);
    let pin_z = placement(&wb, 3).origin().z;
    assert!((pin_z - 18.0).abs() < 1e-6);

    // Double-click the arm (near its far end, clear of the pin): its part opens in place.
    let at = on_screen(&wb, Vec3::new(20.0, 60.0, 18.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(at)]);
    d.click(&mut wb, at);
    d.click(&mut wb, at);
    d.frame(&mut wb, vec![]);
    assert!(wb.editing_in_place(), "{}", wb.status());
    assert_eq!(wb.document().name, "Arm");
    assert!(!wb.context_meshes().is_empty(), "the rest of the assembly stays in view");
    assert!(crate::commands::find("asm.return").is_some());
    // The part environment: its features in the browser, the part ribbon.
    browser_row(&d, "Extrusion1");

    // Make the arm thinner again (t = 5) and go back: the pin on it drops 3.
    wb.exec("param.set", json!({ "name": "t", "equation": "5" })).unwrap();
    d.frame(&mut wb, vec![]);
    wb.command("asm.return");
    d.settle(&mut wb);
    assert!(wb.in_assembly(), "{}", wb.status());
    assert!((placement(&wb, 3).origin().z - 15.0).abs() < 1e-6, "{:?}", placement(&wb, 3));

    // Saving the assembly saves the arm's file too.
    wb.run_ui("file.save").unwrap();
    let (doc, _) = tenon_io::project::open(&path.with_file_name("arm.tenon")).unwrap();
    assert_eq!(doc.parameter_values()["t"], 5.0);
    assert!(wb.status().contains("1 part file"), "{}", wb.status());
}

/// Dragging a component (solving its relationships, moving its bodies, picking) stays well inside
/// a 60 Hz frame. Numbers: `cargo test --release -p tenon-ui assembly_drag -- --nocapture`.
///
/// The median frame is what is held to the budget: tests run in parallel, and on a small CI
/// machine a few frames can wait for the processor behind other tests' geometry work.
#[test]
fn assembly_drag_frame_time_stays_within_budget() {
    use std::time::Instant;
    let _quiet = crate::tests::timing_lock();
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1440.0, 900.0));
    wb.open(&pivot("timing")).unwrap();
    d.settle(&mut wb);
    let from = on_screen(&wb, Vec3::new(40.0, 20.0, 20.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(from)]);
    d.frame(&mut wb, vec![Driver::button(from, egui::PointerButton::Primary, true)]);
    let mut frames: Vec<f64> = (0..60)
        .map(|i| {
            let t = Instant::now();
            d.frame(&mut wb, vec![egui::Event::PointerMoved(from + vec2(i as f32 * 2.0, i as f32))]);
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    d.frame(&mut wb, vec![Driver::button(from + vec2(120.0, 60.0), egui::PointerButton::Primary, false)]);
    frames.sort_by(f64::total_cmp);
    let (median, p90, mean) = (frames[30], frames[54], frames.iter().sum::<f64>() / 60.0);
    println!("dragging a component of a 5-component assembly: median {median:.2} ms, 90th percentile {p90:.2} ms, mean {mean:.2} ms per frame");
    assert!(placement(&wb, 5).origin().x > 31.0, "the block moved");
    assert!(median < 16.0, "over the 16 ms frame budget: median {median:.2} ms (90th percentile {p90:.2} ms)");
}

#[test]
fn an_assembly_weighs_and_shows_each_component_in_its_parts_material() {
    let path = pivot("material");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.open(&path).unwrap();
    d.settle(&mut wb);
    let standard = wb.body_palette(0).0;

    // The arm is made of brass: set in its part, edited in place.
    let at = on_screen(&wb, Vec3::new(20.0, 60.0, 18.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(at)]);
    d.click(&mut wb, at);
    d.click(&mut wb, at);
    d.frame(&mut wb, vec![]);
    assert!(wb.editing_in_place(), "{}", wb.status());
    wb.exec("document.material", json!({ "name": "Brass" })).unwrap();
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.body_palette(0).0, egui::Color32::from_rgb(0xc9, 0xa6, 0x4a), "the part being edited is brass-coloured");
    wb.command("asm.return");
    d.settle(&mut wb);
    assert!(wb.in_assembly(), "{}", wb.status());

    // In the assembly only the arm is brass-coloured.
    let a = wb.asm.as_ref().unwrap();
    let arm = a.session.assembly().components.iter().find(|c| a.name(c.id).starts_with("arm")).expect("the arm").id;
    let bodies: Vec<(bool, f64)> = wb.scene().bodies.iter().enumerate().map(|(i, b)| (a.map[i].0 == arm, b.volume)).collect();
    assert_eq!(bodies.iter().filter(|b| b.0).count(), 1);
    for (i, (is_arm, _)) in bodies.iter().enumerate() {
        let want = if *is_arm { egui::Color32::from_rgb(0xc9, 0xa6, 0x4a) } else { standard };
        assert_eq!(wb.body_palette(i).0, want, "body {i}");
    }

    // Properties adds the components up, the arm at 8.5 g/cm^3 and the rest at 1.
    wb.command("inspect.mass");
    for _ in 0..5 {
        d.frame(&mut wb, vec![]);
    }
    let grams: f64 = bodies.iter().map(|(is_arm, v)| v * if *is_arm { 8.5 } else { 1.0 } / 1000.0).sum();
    let texts = d.texts();
    assert!(texts.contains(&crate::part_dialogs::mass_text(grams)), "{grams}: {texts:?}");
    assert_eq!(texts.iter().filter(|t| *t == "Brass").count(), 1, "{texts:?}");
    assert_eq!(texts.iter().filter(|t| *t == "Generic").count(), 4, "{texts:?}");
    // (No material is chosen for an assembly: that is done in the parts.)
    assert!(!texts.iter().any(|t| t == "Appearance"));

    // The parts list names the material and weighs one of each part; the scripted total agrees.
    let arm_volume = bodies.iter().find(|b| b.0).unwrap().1;
    let rows = tenon_assembly::session::bom(wb.assembly().unwrap().assembly(), &wb.assembly().unwrap().parts);
    assert_eq!(rows.iter().map(|r| r.material.as_deref()).collect::<Vec<_>>(), [None, Some("Brass"), None, None]);
    assert!((rows[1].mass.unwrap() - arm_volume * 8.5 / 1000.0).abs() < 1e-9, "{:?}", rows[1]);
    assert!((rows[2].mass.unwrap() - rows[2].volume.unwrap() / 1000.0).abs() < 1e-9, "one pin, not both: {:?}", rows[2]);
    let csv = tenon_io::asm::bom_csv(wb.assembly().unwrap().assembly(), &wb.assembly().unwrap().parts);
    assert!(csv.lines().nth(2).unwrap().ends_with(&format!(",Brass,{:.3}", arm_volume * 8.5 / 1000.0)), "{csv}");
    let total = wb.asm_exec("asm.mass", json!({})).unwrap();
    assert!((total["mass"].as_f64().unwrap() - grams).abs() < 1e-9, "{total}");
    assert_eq!(total["components"][1]["material"], "Brass");
    wb.command("asm.bom");
    for _ in 0..5 {
        d.frame(&mut wb, vec![]);
    }
    assert!(d.texts().contains(&crate::part_dialogs::mass_text(arm_volume * 8.5 / 1000.0)), "{:?}", d.texts());

    // Saving the assembly saves the arm's file, material and all.
    wb.run_ui("file.save").unwrap();
    let (doc, _) = tenon_io::project::open(&path.with_file_name("arm.tenon")).unwrap();
    assert_eq!(doc.material().map(|m| m.name.as_str()), Some("Brass"));
    assert!(!wb.status_error, "{}", wb.status());
}

#[test]
fn a_newly_opened_assembly_is_fitted_again_as_its_parts_arrive() {
    // Parts regenerate one after another on the worker. A view fitted to the first of them
    // would leave the rest off screen, so it is fitted with each arrival until all are in.
    let path = pivot("fit");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.open(&path).unwrap();
    d.settle(&mut wb);
    assert!(wb.view.fitted);
    let whole = wb.view.camera.distance;
    let asm = wb.assembly().unwrap().assembly().clone();
    assert_eq!(crate::assembly::arrived(&asm, &wb.assembly().unwrap().parts), (5, true));

    // As if only the block had arrived so far: the view is fitted to it, and is not final.
    let scenes: Vec<(String, Option<(u64, std::sync::Arc<tenon_model::Scene>)>)> = wb
        .asm
        .as_mut()
        .unwrap()
        .session
        .parts
        .iter_mut()
        .filter(|(k, _)| !k.ends_with("block.tenon"))
        .map(|(k, p)| (k.clone(), p.scene.take()))
        .collect();
    assert_eq!(crate::assembly::arrived(&asm, &wb.assembly().unwrap().parts), (1, false));
    wb.view.fitted = false;
    wb.asm.as_mut().unwrap().fitted_parts = 0;
    wb.asm.as_mut().unwrap().shown = None;
    wb.flatten();
    assert_eq!(wb.scene().bodies.len(), 1);
    assert!(!wb.view.fitted, "more parts are to come");
    let close = wb.view.camera.distance;
    assert!(close < 0.7 * whole, "fitted to the block alone: {close} vs {whole}");
    // Nothing new: the view is left alone (a part that never arrives must not keep it moving).
    wb.view.camera.distance = close * 3.0;
    wb.asm.as_mut().unwrap().shown = None;
    wb.flatten();
    assert_eq!(wb.view.camera.distance, close * 3.0);

    // The rest arrive: fitted to the whole assembly, for the last time.
    for (k, s) in scenes {
        wb.asm.as_mut().unwrap().session.parts.get_mut(&k).unwrap().scene = s;
    }
    wb.flatten();
    assert_eq!(wb.scene().bodies.len(), 5);
    assert!(wb.view.fitted);
    assert!((wb.view.camera.distance - whole).abs() < 1e-6 * whole, "{} vs {whole}", wb.view.camera.distance);
    wb.view.camera.distance = whole * 2.0;
    wb.asm.as_mut().unwrap().shown = None;
    wb.flatten();
    assert_eq!(wb.view.camera.distance, whole * 2.0, "the user's view is theirs from then on");

    // A part whose file is missing is not waited for.
    let mut parts = tenon_assembly::Parts::new();
    for (k, p) in wb.asm.as_mut().unwrap().session.parts.iter_mut() {
        let mut copy = tenon_assembly::Part::missing("gone");
        if !k.ends_with("pin.tenon") {
            copy = tenon_assembly::Part::new(p.session.document().clone());
            copy.scene = p.scene.clone();
        }
        parts.insert(k.clone(), copy);
    }
    assert_eq!(crate::assembly::arrived(&asm, &parts), (3, true));
}

/// An assembly file of three pins from one part file whose length comes in three sizes: the
/// first as the file has it (20), the others at the row "8x50".
fn sized_pins(name: &str) -> PathBuf {
    let dir = scratch(name);
    let mut s = Session::default();
    let run = |s: &mut Session, id: &str, p: serde_json::Value| tenon_io::cmd::run(s, id, &p, None).unwrap();
    run(&mut s, "document.rename", json!({ "name": "Pin" }));
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(&mut s, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 4 }));
    run(&mut s, "model.extrude", json!({ "sketch": sk, "distance": 20 }));
    run(&mut s, "table.create", json!({ "columns": ["d0"], "row": "8x20" }));
    run(&mut s, "table.add_row", json!({ "name": "8x30", "values": { "d0": 30 } }));
    run(&mut s, "table.add_row", json!({ "name": "8x50", "values": { "d0": 50 } }));
    tenon_io::project::save(&dir.join("pin.tenon"), s.document(), &serde_json::Map::new()).unwrap();
    let mut a = tenon_assembly::AsmSession::default();
    let mut k = OcctKernel::new();
    let pin = dir.join("pin.tenon").to_string_lossy().into_owned();
    let mut place = |p: serde_json::Value| tenon_io::asm::run(&mut a, "asm.insert", &p, Some(&mut k)).unwrap();
    place(json!({ "path": pin }));
    place(json!({ "path": pin, "at": [20, 0, 0], "row": "8x50" }));
    place(json!({ "path": pin, "at": [40, 0, 0], "row": "8x50" }));
    let file = dir.join("pins.tenonasm");
    tenon_io::asm::run(&mut a, "asm.save", &json!({ "path": file.to_string_lossy() }), None).unwrap();
    file
}

#[test]
fn a_component_is_shown_in_its_size_and_changes_size_from_the_browser() {
    let path = sized_pins("sizes");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.open(&path).unwrap();
    d.settle(&mut wb);
    let per_mm = PI * 16.0;
    let volume = |wb: &Workbench| wb.scene().bodies.iter().map(|b| b.volume).sum::<f64>();
    // Three pins: 20, 50 and 50 long. The browser says which are in a size of their own.
    assert_eq!(wb.scene().bodies.len(), 3);
    assert!((volume(&wb) - per_mm * 120.0).abs() < 1e-6, "{}", volume(&wb));
    browser_row(&d, "pin:1");
    browser_row(&d, "pin:2 (8x50)");
    let third = browser_row(&d, "pin:3 (8x50)");

    // Right-click the third pin, Size, 8x30: it is 30 long now.
    let at = third.center();
    d.frame(&mut wb, vec![egui::Event::PointerMoved(at)]);
    d.frame(&mut wb, vec![Driver::button(at, egui::PointerButton::Secondary, true)]);
    d.frame(&mut wb, vec![Driver::button(at, egui::PointerButton::Secondary, false)]);
    d.frame(&mut wb, vec![]);
    let size = d.text_pos("Size").expect("the Size menu");
    d.click(&mut wb, size);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(d.text_pos("As the part file").is_some() && d.text_pos("8x20").is_some(), "{:?}", d.texts());
    let small = d.text_pos("8x30").expect("the 8x30 row");
    d.click(&mut wb, small);
    d.settle(&mut wb);
    assert!(!wb.status_error, "{}", wb.status());
    let rows: Vec<Option<String>> = wb.assembly().unwrap().assembly().components.iter().map(|c| c.row.clone()).collect();
    assert_eq!(rows, [None, Some("8x50".to_owned()), Some("8x30".to_owned())]);
    assert!((volume(&wb) - per_mm * 100.0).abs() < 1e-6, "{}", volume(&wb));
    browser_row(&d, "pin:3 (8x30)");
    // The parts list has an item for each size.
    let bom = tenon_assembly::session::bom(wb.assembly().unwrap().assembly(), &wb.assembly().unwrap().parts);
    assert_eq!(bom.iter().map(|r| (r.name.as_str(), r.quantity)).collect::<Vec<_>>(), [("Pin", 1), ("Pin, 8x50", 1), ("Pin, 8x30", 1)]);

    // The part edited in place (the pin made thicker): on return every size has followed.
    let first = ComponentId(1);
    wb.asm_exec("asm.edit_part", json!({ "component": first.0, "run": "sketch.info", "with": { "sketch": 1 } })).unwrap();
    let info = wb.asm_exec("asm.edit_part", json!({ "component": first.0, "run": "sketch.info", "with": { "sketch": 1 } })).unwrap();
    let ring = info["result"]["entities"].as_array().unwrap().iter().find(|x| x["type"] == "circle").unwrap()["id"].clone();
    let thicker = json!({ "sketch": 1, "constraint": { "type": "radius", "curve": ring, "value": 5.0 } });
    wb.asm_exec("asm.edit_part", json!({ "component": first.0, "run": "sketch.constrain", "with": thicker })).unwrap();
    d.settle(&mut wb);
    d.frame(&mut wb, vec![]);
    assert!((volume(&wb) - PI * 25.0 * 100.0).abs() < 1e-6, "{}", volume(&wb));
    // Saving writes the part file and the assembly, and nothing for the sizes.
    wb.run_ui("file.save").unwrap();
    assert!(wb.status().contains("1 part file"), "{}", wb.status());
    let files = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
    assert_eq!(files, 2);
    assert!(!wb.status_error, "{}", wb.status());
}

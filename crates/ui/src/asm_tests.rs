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
use crate::tests::{Driver, browser_row, on_screen, pressable};

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

    // C opens Constrain, on Mate. Its second solution, Flush, and 5 typed as the offset; then
    // click the base's top, and the cube's top.
    d.tap(&mut wb, egui::Key::C);
    d.frame(&mut wb, vec![]);
    assert!(matches!(&wb.panel, Some(Panel::Asm(p)) if p.tool == AsmTool::Constrain && p.constraint == ConstraintType::Mate));
    d.click(&mut wb, crate::tests::pressable(&d, "tn_asm_solution_flush"));
    crate::tests::type_into(&mut d, &mut wb, "tn_asm_offset", "5");
    let label = d.text_pos("Offset").expect("the Offset label");
    d.click(&mut wb, label);
    d.frame(&mut wb, vec![]);
    assert!(
        matches!(&wb.panel, Some(Panel::Asm(p)) if p.constraint == ConstraintType::Flush && p.offset == 5.0),
        "{:?}",
        wb.panel.as_ref().map(|_| ())
    );
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
    assert!(csv.lines().nth(2).unwrap().ends_with(&format!(",Brass,{:.3},arm,Arm", arm_volume * 8.5 / 1000.0)), "{csv}");
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

#[test]
fn standard_parts_are_placed_from_the_library_in_the_size_picked() {
    let dir = scratch("library");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.run_ui("file.new_assembly").unwrap();
    d.settle(&mut wb);
    // Standard Parts on the ribbon opens the library, on a screw at a middling size.
    wb.run_ui("tools.library").unwrap();
    for _ in 0..5 {
        d.frame(&mut wb, vec![]);
    }
    let texts = d.texts();
    for shown in ["Fasteners", "Bearings", "Sections", "Hex Head Screw", "Deep Groove Ball Bearing", "T-Slot Profile", "Sizes to ISO 4017", "M8x30"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    assert_eq!(wb.chrome.library_pick, ("hex-screw-iso-4017".to_owned(), "M8x30".to_owned()));

    // Place, in an assembly that has no folder yet: refused, saying what to do.
    d.click(&mut wb, pressable(&d, "tn_library_place"));
    d.frame(&mut wb, vec![]);
    assert!(wb.status_error && wb.status().starts_with("Save the assembly first"), "{}", wb.status());
    assert!(wb.assembly().unwrap().assembly().components.is_empty());
    let file = dir.join("stack.tenonasm");
    let to = file.clone();
    wb.services.pick_save = Some(Box::new(move |_: &str, _: &str| Some(to.clone())));
    wb.run_ui("file.save").unwrap();
    d.frame(&mut wb, vec![]);

    // The washer for it: its row in the list, the M8 size, Place.
    d.click(&mut wb, pressable(&d, "tn_library_part_washer-iso-7089"));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t == "Sizes to ISO 7089"), "{:?}", d.texts());
    assert_eq!(wb.chrome.library_pick, ("washer-iso-7089".to_owned(), "M8".to_owned()));
    d.click(&mut wb, pressable(&d, "tn_library_place"));
    d.settle(&mut wb);
    assert!(!wb.status_error && wb.status().starts_with("Placed Plain Washer, M8"), "{}", wb.status());
    // Then the screw, in M8x30 and again in M8x16 (picked from the list of sizes).
    d.click(&mut wb, pressable(&d, "tn_library_part_hex-screw-iso-4017"));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "tn_library_place"));
    d.settle(&mut wb);
    d.click(&mut wb, pressable(&d, "tn_library_size_M8x16"));
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t.contains("l 16")), "the size's dimensions show: {:?}", d.texts());
    d.click(&mut wb, pressable(&d, "tn_library_place"));
    d.settle(&mut wb);
    assert!(!wb.status_error, "{}", wb.status());

    // Three components from two files in the folder beside the assembly; each in its size.
    let rows: Vec<(String, Option<String>)> =
        wb.assembly().unwrap().assembly().components.iter().map(|c| (c.part.replace('\\', "/"), c.row.clone())).collect();
    assert_eq!(rows.len(), 3);
    assert!(rows[0].0.ends_with("standard/washer-iso-7089.tenon") && rows[0].1.as_deref() == Some("M8"), "{rows:?}");
    assert!(rows[1].0.ends_with("standard/hex-screw-iso-4017.tenon") && rows[1].1.as_deref() == Some("M8x30"), "{rows:?}");
    assert_eq!(rows[2].1.as_deref(), Some("M8x16"));
    assert_eq!(std::fs::read_dir(dir.join("standard")).unwrap().count(), 2);
    let hexagon = 3f64.sqrt() / 2.0 * 169.0;
    let want = (PI * (256.0 - 70.56) / 4.0) * 1.6 + 2.0 * hexagon * 5.3 + PI * 16.0 * (30.0 + 16.0);
    let got: f64 = wb.scene().bodies.iter().map(|b| b.volume).sum();
    assert!((got - want).abs() < 1e-6 * want, "{got} vs {want}");
    browser_row(&d, "hex-screw-iso-4017:2 (M8x16)");
    // They do not land on one another: each is placed beside what is there.
    let xs: Vec<f64> = wb.assembly().unwrap().assembly().components.iter().map(|c| c.placement.origin().x).collect();
    assert!(xs[0] < xs[1] && xs[1] < xs[2], "{xs:?}");
    // Saving writes the assembly; the standard files were written when they were first used.
    wb.run_ui("file.save").unwrap();
    assert!(std::fs::read_to_string(&file).unwrap().contains("row = \"M8x16\""));
    assert!(!wb.status_error, "{}", wb.status());
}

/// An assembly file of `side` by `side` components, bolts (a modelled thread) and nozzles (a
/// loft) in turn: the fittings of examples/m6-fittings.
fn grid(name: &str, side: u32) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m6-fittings");
    let dir = scratch(name);
    for f in ["bolt.tenon", "nozzle.tenon"] {
        std::fs::copy(src.join(f), dir.join(f)).unwrap();
    }
    let mut a = tenon_assembly::AsmSession::default();
    let mut k = OcctKernel::new();
    for i in 0..side {
        for j in 0..side {
            let part = dir.join(if (i + j) % 2 == 0 { "bolt.tenon" } else { "nozzle.tenon" }).to_string_lossy().into_owned();
            tenon_io::asm::run(&mut a, "asm.insert", &json!({ "path": part, "at": [f64::from(i) * 40.0, f64::from(j) * 40.0, 0.0] }), Some(&mut k))
                .unwrap();
        }
    }
    let file = dir.join("grid.tenonasm");
    tenon_io::asm::run(&mut a, "asm.save", &json!({ "path": file.to_string_lossy() }), None).unwrap();
    file
}

/// Level of detail: components drawn small are drawn from their far meshes, components off the
/// screen are not drawn, and what is under the pointer is found without trying every triangle.
///
/// The medians are what is held to the budgets (tests run in parallel; see
/// `assembly_drag_frame_time_stays_within_budget`). Numbers for larger assemblies:
/// `cargo test -p tenon-ui large_assembly -- --ignored --nocapture`.
#[test]
fn a_large_assembly_is_drawn_by_level_of_detail() {
    use std::time::Instant;
    use tenon_render::detail::Detail;
    let _quiet = crate::tests::timing_lock();
    let path = grid("detail", 10);
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1440.0, 900.0));
    wb.open(&path).unwrap();
    d.settle(&mut wb);
    assert_eq!(wb.scene().bodies.len(), 100);
    let (w, h) = (f64::from(wb.view.rect.width()), f64::from(wb.view.rect.height()));
    let all: usize = wb.scene().bodies.iter().map(|b| b.mesh.triangle_count()).sum();
    // Every part came with a far mesh: the same faces and edges, far fewer triangles.
    for b in &wb.scene().bodies {
        let far = b.far.as_ref().expect("a far mesh");
        assert!(far.triangle_count() * 3 < b.mesh.triangle_count(), "{} of {}", far.triangle_count(), b.mesh.triangle_count());
        assert_eq!((far.faces.len(), far.edges.len()), (b.mesh.faces.len(), b.mesh.edges.len()));
    }

    // The whole assembly in view: a hundred components, each a few dozen pixels across.
    // (All but the nearest of the larger ones, the nozzles.)
    let whole = wb.body_detail(w, h);
    assert!(whole.iter().filter(|l| **l == Detail::Far).count() >= 60 && !whole.contains(&Detail::Hidden), "{whole:?}");
    let (bodies, triangles, _) = wb.drawn(w, h);
    assert_eq!(bodies, 100);
    assert!(triangles * 5 < all * 2, "{triangles} of {all} triangles drawn");

    // Close in: the components in front of the eye in full, those off the screen not at all.
    let fitted = wb.view.camera.distance;
    wb.view.camera.distance = fitted / 6.0;
    let levels = wb.body_detail(w, h);
    let count = |l: Detail| levels.iter().filter(|x| **x == l).count();
    assert!(count(Detail::Near) >= 4 && count(Detail::Hidden) >= 20, "{} near, {} hidden", count(Detail::Near), count(Detail::Hidden));
    let (bodies, _, _) = wb.drawn(w, h);
    assert_eq!(bodies, 100 - count(Detail::Hidden));
    // A component that is drawn is on the screen, and one that is not is off it.
    for (b, level) in wb.scene().bodies.iter().zip(&levels) {
        let bb = b.bbox.unwrap();
        let mid = wb.view.camera.project((bb.min + bb.max) * 0.5, w, h);
        let on_screen = mid.is_some_and(|(x, y, _)| (0.0..=w).contains(&x) && (0.0..=h).contains(&y));
        assert!(!(on_screen && *level == Detail::Hidden), "a component in view is drawn");
    }
    // Back out, and it is drawn from afar again.
    wb.view.camera.distance = fitted;
    assert_eq!(wb.body_detail(w, h), whole);

    // What is under the pointer is still the component itself, found in the full mesh.
    let bolt = wb.scene().bodies[0].bbox.unwrap();
    let over = on_screen(&wb, (bolt.min + bolt.max) * 0.5);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(over)]);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(over)]);
    assert!(wb.asm.as_ref().unwrap().hover.is_some(), "the component under the pointer is found");

    // Finding it takes well under a frame, where trying every triangle of the half million
    // took 20 ms for a face and 130 ms for an edge on the machine this was written on.
    // (The components are drawn and picked from their parts' own meshes: a hundred
    // components, the two meshes of the bolt and the nozzle.)
    let own: std::collections::BTreeSet<*const tenon_kernel::Mesh> = wb.scene().bodies.iter().map(|b| std::sync::Arc::as_ptr(&b.mesh)).collect();
    assert_eq!((wb.scene().bodies.len(), own.len()), (100, 2));
    let (meshes, tree) = (wb.scene().placed(), wb.body_tree());
    let middles: Vec<(f64, f64)> = wb
        .scene()
        .bodies
        .iter()
        .filter_map(|b| b.bbox)
        .filter_map(|bb| wb.view.camera.project((bb.min + bb.max) * 0.5, w, h))
        .map(|(x, y, _)| (x, y))
        .collect();
    let (mut faces, mut edges) = (Vec::new(), Vec::new());
    let mut found = 0;
    for round in 0..2 {
        for (x, y) in &middles {
            let t = Instant::now();
            let (o, dir) = wb.view.camera.ray(*x, *y, w, h);
            let face = tenon_render::pick::pick_face_in(tree.as_deref(), &meshes, o, dir);
            let took = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            tenon_render::pick::pick_edge_in(tree.as_deref(), &meshes, &wb.view.camera, w, h, *x, *y, 5.0);
            // (The first round makes the indices of the bodies the pointer comes over.)
            if round == 1 {
                faces.push(took);
                edges.push(t.elapsed().as_secs_f64() * 1000.0);
                found += usize::from(face.is_some());
            }
        }
    }
    assert!(found > 50, "{found} of {} components found under the pointer", middles.len());
    faces.sort_by(f64::total_cmp);
    edges.sort_by(f64::total_cmp);
    let (face, edge) = (faces[faces.len() / 2], edges[edges.len() / 2]);
    let mut frames: Vec<f64> = (0..40)
        .map(|_| {
            let t = Instant::now();
            d.frame(&mut wb, vec![]);
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    frames.sort_by(f64::total_cmp);
    let idle = frames[20];
    println!("100 components: finding a face {face:.3} ms, an edge {edge:.3} ms; an idle frame {idle:.2} ms; {triangles} of {all} triangles drawn");
    assert!(face < 4.0 && edge < 8.0, "finding what is under the pointer: a face {face:.2} ms, an edge {edge:.2} ms");
    assert!(idle < 16.0, "over the 16 ms frame budget: {idle:.2} ms");
}

/// The browser of an assembly lays out only the rows in sight: a thousand components cost a
/// frame no more than a dozen.
#[test]
fn the_browser_of_a_large_assembly_lays_out_only_the_rows_in_sight() {
    let path = grid("rows", 10);
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1440.0, 900.0));
    wb.open(&path).unwrap();
    d.settle(&mut wb);
    d.frame(&mut wb, vec![]);
    let rows: Vec<(String, egui::Rect)> = d.ctx.data(|data| data.get_temp(egui::Id::new("tn_browser_rows"))).unwrap_or_default();
    // The assembly, two folders, and the first components: not all hundred.
    assert!(rows.len() > 10 && rows.len() < 60, "{} rows laid out", rows.len());
    let name = wb.assembly().unwrap().assembly().name.clone();
    assert_eq!(rows[0].0, name);
    assert!(rows.iter().any(|(label, _)| label == "bolt:1"), "{:?}", rows.iter().map(|r| &r.0).collect::<Vec<_>>());
    assert!(!rows.iter().any(|(label, _)| label == "nozzle:100" || label == "bolt:99"));
    // Rows are where they would be if all were laid out: evenly apart, in order.
    let pitch = rows[1].1.top() - rows[0].1.top();
    assert!(pitch >= crate::browser::ROW_H);
    for pair in rows.windows(2) {
        assert!((pair[1].1.top() - pair[0].1.top() - pitch).abs() < 0.01, "{pair:?}");
    }
    // Scrolled to the end, the last components are there and the first are not.
    let at = rows[5].1.center();
    d.frame(&mut wb, vec![egui::Event::PointerMoved(at)]);
    for _ in 0..40 {
        d.frame(
            &mut wb,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -400.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
    d.frame(&mut wb, vec![]);
    let rows: Vec<(String, egui::Rect)> = d.ctx.data(|data| data.get_temp(egui::Id::new("tn_browser_rows"))).unwrap_or_default();
    let labels: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert!(labels.len() < 60 && !labels.contains(&name.as_str()), "{labels:?}");
    let last = wb.assembly().unwrap().assembly().components.last().unwrap().name.clone();
    assert!(labels.contains(&last.as_str()), "{last} in {labels:?}");
    // Clicking a row there selects that component.
    let (_, rect) = rows.iter().find(|(label, _)| *label == last).unwrap();
    d.click(&mut wb, rect.center());
    let picked = wb.asm.as_ref().unwrap().selected.clone();
    assert_eq!(picked.len(), 1);
    assert_eq!(wb.assembly().unwrap().assembly().component(picked[0]).unwrap().name, last);
}

/// Timing, not a check: assemblies of a hundred, four hundred and a thousand components in the
/// view. The numbers in docs/performance.md are from here.
/// `cargo test -p tenon-ui large_assembly_in_the_view -- --ignored --nocapture`
#[test]
#[ignore]
fn large_assembly_in_the_view() {
    use std::time::Instant;
    use tenon_render::detail::Detail;
    let _quiet = crate::tests::timing_lock();
    for side in [10, 20, 32] {
        let path = grid(&format!("grid-{side}"), side);
        let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
        let mut d = Driver::new(vec2(1440.0, 900.0));
        wb.open(&path).unwrap();
        d.settle(&mut wb);
        let (w, h) = (f64::from(wb.view.rect.width()), f64::from(wb.view.rect.height()));
        let all: usize = wb.scene().bodies.iter().map(|b| b.mesh.triangle_count()).sum();
        // What the meshes hold in memory: points, normals, corners and edge points.
        let bytes =
            |m: &tenon_kernel::Mesh| 24 * m.positions.len() + 4 * m.indices.len() + 12 * m.edges.iter().map(|e| e.points.len()).sum::<usize>();
        // (Each mesh once, however many components are drawn from it.)
        let mut seen = std::collections::BTreeSet::new();
        let mut once = |m: &std::sync::Arc<tenon_kernel::Mesh>| if seen.insert(std::sync::Arc::as_ptr(m)) { bytes(m) } else { 0 };
        let held: usize = wb.scene().bodies.iter().map(|b| once(&b.mesh)).sum();
        let held_far: usize = wb.scene().bodies.iter().filter_map(|b| b.far.as_ref()).map(&mut once).sum();
        let (held_mb, far_mb) = (held as f64 / 1e6, held_far as f64 / 1e6);
        // What is said of each component's faces and edges, which is still moved for each.
        let said = |b: &tenon_model::BodyView| {
            std::mem::size_of_val(b.faces.as_slice())
                + std::mem::size_of_val(b.edges.as_slice())
                + std::mem::size_of_val(b.curves.as_slice())
                + std::mem::size_of_val(b.ends.as_slice())
                + std::mem::size_of_val(b.mids.as_slice())
        };
        let said_mb = wb.scene().bodies.iter().map(said).sum::<usize>() as f64 / 1e6;
        let whole = wb.drawn(w, h);
        let fitted = wb.view.camera.distance;
        wb.view.camera.distance = fitted / 6.0;
        let close = wb.drawn(w, h);
        let hidden = wb.body_detail(w, h).iter().filter(|l| **l == Detail::Hidden).count();
        wb.view.camera.distance = fitted;
        // The pointer over the middle of every component, twice (the first round makes the
        // indices of the bodies it comes over).
        let (meshes, tree) = (wb.scene().placed(), wb.body_tree());
        let middles: Vec<(f64, f64)> = wb
            .scene()
            .bodies
            .iter()
            .filter_map(|b| b.bbox)
            .filter_map(|bb| wb.view.camera.project((bb.min + bb.max) * 0.5, w, h))
            .map(|(x, y, _)| (x, y))
            .collect();
        let mut took = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for round in 0..2 {
            for (x, y) in &middles {
                let t = Instant::now();
                let (o, dir) = wb.view.camera.ray(*x, *y, w, h);
                tenon_render::pick::pick_face_in(tree.as_deref(), &meshes, o, dir);
                took[round].push(t.elapsed().as_secs_f64() * 1000.0);
                let t = Instant::now();
                tenon_render::pick::pick_edge_in(tree.as_deref(), &meshes, &wb.view.camera, w, h, *x, *y, 5.0);
                took[2 + round].push(t.elapsed().as_secs_f64() * 1000.0);
            }
        }
        let median = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        let [face_first, face, edge_first, edge] = &mut took;
        let mut frames: Vec<f64> = (0..40)
            .map(|_| {
                let t = Instant::now();
                d.frame(&mut wb, vec![]);
                t.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        println!(
            "{} components, {all} triangles ({held_mb:.2} MB of meshes and {far_mb:.2} MB of far meshes, each held once; {said_mb:.2} MB said of faces and edges): drawn with all in view {} of them; close in {} in {} bodies ({hidden} off the screen); \
             finding a face {:.3} ms (the first time over a body {:.3}), an edge {:.3} ms (the first time {:.3}); an idle frame {:.2} ms",
            side * side,
            whole.1,
            close.1,
            close.0,
            median(face),
            median(face_first),
            median(edge),
            median(edge_first),
            median(&mut frames),
        );
    }
}

fn block<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    let mut cx = Context::from_waker(Waker::noop());
    let mut f = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

/// Timing, not a check: a large assembly drawn on the graphics card, every body in full and
/// then at each body's level of detail.
/// `cargo test --release -p tenon-ui large_assembly_on_the_graphics_card -- --ignored --nocapture`
#[test]
#[ignore]
fn large_assembly_on_the_graphics_card() {
    use egui_wgpu::wgpu;
    use std::time::Instant;
    use tenon_render::detail::Detail;
    use tenon_render::gpu::{Body, BodyColors, Viewport};
    let _quiet = crate::tests::timing_lock();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let Ok(adapter) = block(instance.request_adapter(&wgpu::RequestAdapterOptions { force_fallback_adapter: false, ..Default::default() })) else {
        println!("no graphics adapter");
        return;
    };
    let info = adapter.get_info();
    println!("adapter: {:?} ({:?})", info.device_type, info.backend);
    let Ok((device, queue)) = block(adapter.request_device(&wgpu::DeviceDescriptor::default())) else { return };
    let (w, h) = (1440_u32, 900_u32);
    for side in [10, 20, 32] {
        let path = grid(&format!("gpu-{side}"), side);
        let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
        let mut d = Driver::new(vec2(w as f32, h as f32));
        wb.open(&path).unwrap();
        d.settle(&mut wb);
        let plain = BodyColors { face: [0.6; 3], edge: [0.02; 3], ..Default::default() };
        let fitted = wb.view.camera.distance;
        for (view, zoom) in [("the whole assembly", 1.0), ("close in", 1.0 / 6.0)] {
            wb.view.camera.distance = fitted * zoom;
            let levels = wb.body_detail(f64::from(w), f64::from(h));
            let radius = wb.scene_radius();
            for with in [false, true] {
                let bodies: Vec<Body> = wb
                    .scene()
                    .bodies
                    .iter()
                    .zip(&levels)
                    .filter(|(_, level)| !with || **level != Detail::Hidden)
                    .map(|(b, level)| {
                        let placed = b.placed_at(with && *level == Detail::Far);
                        Body::new(placed.mesh, placed.at, &plain)
                    })
                    .collect();
                let mut vp = Viewport::new(&device);
                let t = Instant::now();
                vp.set_bodies(&device, &bodies);
                vp.render(&device, &queue, &wb.view.camera, w, h, radius).unwrap();
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                let first = t.elapsed().as_secs_f64() * 1000.0;
                let mut frames: Vec<f64> = (0..60)
                    .map(|i| {
                        let mut c = wb.view.camera;
                        c.orbit(f64::from(i) * 0.002, 0.0);
                        let t = Instant::now();
                        vp.render(&device, &queue, &c, w, h, radius).unwrap();
                        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                        t.elapsed().as_secs_f64() * 1000.0
                    })
                    .collect();
                frames.sort_by(f64::total_cmp);
                let (n, triangles, segments) = vp.drawn();
                println!(
                    "{} components, {view}, {}: {n} bodies of {} meshes, {triangles} triangles, {segments} edge segments, {:.2} MB on the card; sent and first drawn in {first:.1} ms; a frame {:.2} ms (90th {:.2})",
                    side * side,
                    if with { "with level of detail" } else { "every body in full" },
                    vp.held_meshes().0,
                    vp.held_bytes().0 as f64 / 1e6,
                    frames[30],
                    frames[54]
                );
            }
        }
    }
}

/// The app as it runs (the kernel on its own thread), keeping scenes in `cache`, with `file`
/// opened and everything it started done.
fn opened_keeping_scenes(cache: &Path, file: &Path) -> (Workbench, Driver) {
    let mut wb = Workbench::new(|| Box::new(OcctKernel::new()) as Box<dyn tenon_kernel::Kernel>, None, crate::Services::default());
    let mut d = Driver::new(vec2(1200.0, 800.0));
    wb.keep_scenes(cache);
    wb.open(file).unwrap();
    for _ in 0..60_000 {
        d.frame(&mut wb, vec![]);
        if wb.is_settled() {
            return (wb, d);
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("{} was never done", file.display());
}

fn kept_scenes(cache: &Path) -> usize {
    std::fs::read_dir(cache).map_or(0, |d| d.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "scene")).count())
}

#[test]
fn an_assembly_opened_again_is_shown_from_what_was_kept_of_its_parts() {
    let file = pivot("kept-pivot");
    let cache = scratch("kept-pivot-scenes");
    // The first time every part is rebuilt, and what each comes to is kept.
    let (wb, _) = opened_keeping_scenes(&cache, &file);
    assert_eq!(wb.early_scenes, 0);
    let first = wb.scene().bodies.clone();
    assert_eq!(first.len(), 5);
    drop(wb);
    assert_eq!(kept_scenes(&cache), 4, "five components of four parts");

    // Opened again in another run of the program: each part is shown from what was kept of
    // it, and the assembly is the same to the last number once the parts are rebuilt (which
    // is what `is_settled` waits for: until then commands that need the solids wait too).
    let (wb, _) = opened_keeping_scenes(&cache, &file);
    assert_eq!(wb.early_scenes, 4);
    assert_eq!(wb.scene().bodies, first);
    assert!(wb.asm.as_ref().unwrap().unconfirmed.is_empty() && !wb.is_busy());
    // (The two pins are still one mesh.)
    let meshes: std::collections::BTreeSet<*const tenon_kernel::Mesh> = wb.scene().bodies.iter().map(|b| std::sync::Arc::as_ptr(&b.mesh)).collect();
    assert_eq!(meshes.len(), 4);
    drop(wb);

    // A part that was changed since is not found among what was kept: it is rebuilt, the
    // others are shown as before.
    let block = file.with_file_name("block.tenon");
    let (doc, extra) = tenon_io::project::open(&block).unwrap();
    let mut s = Session::new(doc);
    s.exec("document.rename", &json!({ "name": "Block, renamed" }), None).unwrap();
    tenon_io::project::save(&block, s.document(), &extra).unwrap();
    let (wb, _) = opened_keeping_scenes(&cache, &file);
    assert_eq!(wb.early_scenes, 3);
    assert_eq!(wb.scene().bodies, first, "a new name is the same solid");
    drop(wb);
    assert_eq!(kept_scenes(&cache), 5, "the block as it is now is kept too");

    // A part on its own likewise: rebuilt and kept the first time, shown at once the second.
    let arm = file.with_file_name("arm.tenon");
    let (wb, _) = opened_keeping_scenes(&cache, &arm);
    assert_eq!(wb.early_scenes, 0, "a part on its own is kept apart from the part of an assembly (no mesh for drawing from far away)");
    let alone = wb.scene().bodies.clone();
    drop(wb);
    let (mut wb, mut d) = opened_keeping_scenes(&cache, &arm);
    assert_eq!(wb.early_scenes, 1);
    assert_eq!(wb.scene().bodies, alone);
    assert!(!wb.waiting);
    // Once it is changed it is no longer the part on disk: rebuilt, with nothing shown early
    // and nothing kept of what is only being tried.
    let kept = kept_scenes(&cache);
    wb.exec("document.rename", json!({ "name": "Arm, being worked on" })).unwrap();
    for _ in 0..60_000 {
        d.frame(&mut wb, vec![]);
        if wb.is_settled() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!((wb.early_scenes, kept_scenes(&cache)), (1, kept));
    assert_eq!(wb.scene().bodies, alone);
}

#[test]
fn placed_parts_follow_the_pointer_and_are_put_down_by_clicks() {
    let dir = scratch("place");
    box_part(&dir.join("base.tenon"), "Base", 40.0, 40.0, 10.0);
    box_part(&dir.join("cube.tenon"), "Cube", 10.0, 10.0, 10.0);
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.run_ui("file.new_assembly").unwrap();
    // Place on an empty assembly: the base goes to the origin, grounded, and another base
    // follows the pointer at once.
    let pick = std::sync::Arc::new(std::sync::Mutex::new(dir.join("base.tenon")));
    let chosen = pick.clone();
    wb.services.pick_open = Some(Box::new(move || Some(chosen.lock().unwrap().clone())));
    d.tap(&mut wb, egui::Key::P);
    d.settle(&mut wb);
    assert!(wb.status().contains("Click where"), "{}", wb.status());
    assert!(placement(&wb, 1).origin().near(Vec3::ZERO, 1e-12));
    d.tap(&mut wb, egui::Key::Escape);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.assembly().unwrap().assembly().components.len(), 1, "the follower is taken back: {}", wb.status());
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);

    // Place the cube: it follows the pointer, its middle under it; two clicks put two down.
    *pick.lock().unwrap() = dir.join("cube.tenon");
    d.tap(&mut wb, egui::Key::P);
    d.frame(&mut wb, vec![]);
    let spots = [wb.view.rect.center() + vec2(-150.0, 60.0), wb.view.rect.center() + vec2(160.0, -40.0)];
    for (i, at) in spots.iter().enumerate() {
        d.frame(&mut wb, vec![egui::Event::PointerMoved(*at)]);
        d.frame(&mut wb, vec![]);
        // The cube's middle is under the pointer before the click.
        let a = wb.asm.as_ref().unwrap();
        let p = a.placing.as_ref().expect("placing");
        let f = p.working.component(p.component).unwrap().placement;
        let middle = f.to_world(Vec3::new(5.0, 5.0, 5.0));
        let seen = crate::tests::on_screen(&wb, middle);
        assert!((seen - *at).length() < 1.0, "instance {i}: middle seen at {seen:?}, pointer at {at:?}");
        d.click(&mut wb, *at);
        d.frame(&mut wb, vec![]);
    }
    // A right-click ends it: two cubes beside the base, each put down where it was clicked.
    let at = spots[1];
    d.frame(&mut wb, vec![Driver::button(at, egui::PointerButton::Secondary, true)]);
    d.frame(&mut wb, vec![Driver::button(at, egui::PointerButton::Secondary, false)]);
    d.frame(&mut wb, vec![]);
    let asm = wb.assembly().unwrap().assembly().clone();
    assert_eq!(asm.components.len(), 3, "{}", wb.status());
    assert!(wb.status().contains("Placed 2 of cube"), "{}", wb.status());
    let (c2, c3) = (placement(&wb, asm.components[1].id.0), placement(&wb, asm.components[2].id.0));
    assert!(c2.origin().dist(c3.origin()) > 20.0, "put down apart: {c2:?} {c3:?}");
    // Each one is one step to undo.
    wb.run_ui("edit.undo").unwrap();
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.assembly().unwrap().assembly().components.len(), 2);
}

#[test]
fn constrain_offers_tangent_with_its_outside_and_inside_solutions() {
    let dir = scratch("tangent-ui");
    box_part(&dir.join("base.tenon"), "Base", 40.0, 40.0, 10.0);
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.run_ui("file.new_assembly").unwrap();
    wb.place_file(&dir.join("base.tenon")).unwrap();
    d.settle(&mut wb);
    d.tap(&mut wb, egui::Key::C);
    d.frame(&mut wb, vec![]);
    // Inventor's order of types: Mate, Angle, Tangent, Insert.
    let x = |key: &str| pressable(&d, key).x;
    assert!(x("tn_asm_type_mate") < x("tn_asm_type_angle") && x("tn_asm_type_angle") < x("tn_asm_type_tangent"));
    assert!(x("tn_asm_type_tangent") < x("tn_asm_type_insert"));
    d.click(&mut wb, pressable(&d, "tn_asm_type_tangent"));
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t == "click a cylinder, sphere or plane"), "{:?}", d.texts());
    d.click(&mut wb, pressable(&d, "tn_asm_solution_inside"));
    d.frame(&mut wb, vec![]);
    let Some(Panel::Asm(p)) = &wb.panel else { panic!("no panel") };
    assert_eq!((p.constraint, p.inside), (ConstraintType::Tangent, true));
    d.click(&mut wb, pressable(&d, "tn_asm_solution_outside"));
    d.frame(&mut wb, vec![]);
    assert!(matches!(&wb.panel, Some(Panel::Asm(p)) if !p.inside));
}

#[test]
fn a_symmetry_takes_its_plane_as_a_third_pick() {
    let dir = scratch("symmetry-ui");
    box_part(&dir.join("block.tenon"), "Block", 10.0, 10.0, 10.0);
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.run_ui("file.new_assembly").unwrap();
    wb.place_file(&dir.join("block.tenon")).unwrap();
    wb.place_file(&dir.join("block.tenon")).unwrap();
    // The second off to the back: its front 7 behind the first's.
    wb.asm_exec("asm.move", json!({ "component": 2, "by": [0, 7, 0] })).unwrap();
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);
    let b = placement(&wb, 2).origin();
    // C, Symmetry: the first's front face, the second's front face, then the YZ plane from the
    // browser's Origin folder.
    d.tap(&mut wb, egui::Key::C);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "tn_asm_type_symmetry"));
    d.frame(&mut wb, vec![]);
    let front_a = on_screen(&wb, Vec3::new(5.0, 0.0, 5.0));
    d.click(&mut wb, front_a);
    let front_b = on_screen(&wb, b + Vec3::new(5.0, 0.0, 5.0));
    d.click(&mut wb, front_b);
    d.frame(&mut wb, vec![]);
    assert!(matches!(&wb.panel, Some(Panel::Asm(p)) if p.a.is_some() && p.b.is_some() && p.slot == 2), "the plane is next: {}", wb.status());
    if !d.texts().iter().any(|t| t == "YZ Plane") {
        d.click(&mut wb, browser_row(&d, "Origin").left_center() + vec2(27.0, 0.0));
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, browser_row(&d, "YZ Plane").center());
    d.frame(&mut wb, vec![]);
    assert!(matches!(&wb.panel, Some(Panel::Asm(p)) if p.c.is_some()), "{}", wb.status());
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    // The second's front is back on the first's (a front face mirrored in YZ stays at its y).
    assert!(placement(&wb, 2).origin().y.abs() < 1e-6, "{:?}", placement(&wb, 2));
    browser_row(&d, "Symmetry:1");
}

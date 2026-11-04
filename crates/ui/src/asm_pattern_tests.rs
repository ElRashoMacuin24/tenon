//! Pattern, Mirror and Copy of an assembly's components through the UI, with real pointer and key
//! input: the panels, picking in the view, the preview, the browser's rows (DEC-130).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use egui::vec2;
use serde_json::json;
use tenon_assembly::ComponentId;
use tenon_geom::Vec3;
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;

use crate::Workbench;
use crate::asm_panel::AsmTool;
use crate::panels::Panel;
use crate::tests::{Driver, browser_row, on_screen, pressable, type_into};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-ui-asm-pattern-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

/// A foot 40 x 20 x 10 with a 10 x 10 post on its corner at the origin, up to 30: 10000 mm^3, not
/// the same as its mirror image however it is turned.
fn post_part(path: &Path) {
    let mut s = Session::default();
    let run = |s: &mut Session, id: &str, p: serde_json::Value| tenon_io::cmd::run(s, id, &p, None).unwrap();
    run(&mut s, "document.rename", json!({ "name": "Post" }));
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 }));
    run(&mut s, "model.extrude", json!({ "sketch": sk, "distance": 10 }));
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 10, "y2": 10 }));
    run(&mut s, "model.extrude", json!({ "sketch": sk, "distance": 30, "operation": "join" }));
    tenon_io::project::save(path, s.document(), &serde_json::Map::new()).unwrap();
}

fn placement(wb: &Workbench, id: u32) -> tenon_geom::Frame {
    wb.assembly().unwrap().assembly().component(ComponentId(id)).unwrap().placement
}

fn near(a: Vec3, b: Vec3) -> bool {
    a.near(b, 1e-6)
}

/// A new assembly of `files`, each placed where it is told, seen from the front, right and above.
fn opened(wb: &mut Workbench, d: &mut Driver, files: &[(&Path, [f64; 3])]) {
    wb.run_ui("file.new_assembly").unwrap();
    for (i, (file, at)) in files.iter().enumerate() {
        wb.place_file(file).unwrap();
        d.settle(wb);
        wb.asm_exec("asm.place", json!({ "component": i + 1, "origin": at })).unwrap_or_else(|e| {
            assert!(e.contains("grounded") || i == 0, "{e}");
            json!({})
        });
    }
    if let Some(a) = wb.asm.as_mut() {
        a.selected.clear();
    }
    d.settle(wb);
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    wb.run_ui("view.fit").unwrap();
    d.settle(wb);
}

/// Clicks a panel's value box, types `text` and clicks away, so that it counts.
fn typed(d: &mut Driver, wb: &mut Workbench, field: &str, text: &str) {
    type_into(d, wb, field, text);
    d.tap(wb, egui::Key::Tab);
    d.frame(wb, vec![]);
    d.frame(wb, vec![]);
}

/// Presses the panel's OK.
fn press_ok(d: &mut Driver, wb: &mut Workbench) {
    let ok = d.text_pos("OK").expect("the panel's OK");
    d.click(wb, ok);
    d.settle(wb);
}

/// A right-click on a browser row, for its menu.
fn menu_on(d: &mut Driver, wb: &mut Workbench, row: &str) {
    let at = browser_row(d, row).center();
    d.frame(wb, vec![egui::Event::PointerMoved(at)]);
    d.frame(wb, vec![Driver::button(at, egui::PointerButton::Secondary, true)]);
    d.frame(wb, vec![Driver::button(at, egui::PointerButton::Secondary, false)]);
    d.frame(wb, vec![]);
    d.frame(wb, vec![]);
}

#[test]
fn a_pattern_is_made_by_clicking_seen_before_ok_and_listed_with_its_elements() {
    let dir = scratch("rows");
    box_part(&dir.join("plate.tenon"), "Plate", 200.0, 100.0, 10.0);
    box_part(&dir.join("block.tenon"), "Block", 10.0, 10.0, 10.0);
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    opened(&mut wb, &mut d, &[(&dir.join("plate.tenon"), [0.0, 0.0, 0.0]), (&dir.join("block.tenon"), [10.0, 10.0, 10.0])]);

    // The button works and says what it does.
    let c = crate::commands::find("asm.pattern").unwrap();
    assert!(c.available(), "{}", c.not_yet());
    d.click(&mut wb, pressable(&d, "ribbon asm.pattern"));
    d.frame(&mut wb, vec![]);
    assert!(matches!(&wb.panel, Some(Panel::Asm(p)) if p.tool == AsmTool::Pattern), "{}", wb.status());

    // OK with nothing picked says what to do.
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.status_error && wb.status().contains("Click at least one component"), "{}", wb.status());
    assert!(wb.panel.is_some());

    // The block, by a click; then the plate's long front edge for the first direction.
    let block = on_screen(&wb, Vec3::new(15.0, 15.0, 20.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(block)]);
    d.click(&mut wb, block);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Asm(p)) = &wb.panel else { panic!() };
    assert_eq!(p.components, [ComponentId(2)]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.status_error && wb.status().contains("edge, face or axis"), "{}", wb.status());

    d.click(&mut wb, pressable(&d, "tn_pattern_first"));
    d.frame(&mut wb, vec![]);
    let edge = on_screen(&wb, Vec3::new(100.0, 0.0, 10.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(edge)]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, edge);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Asm(p)) = &wb.panel else { panic!() };
    assert!(p.a.as_ref().is_some_and(|(_, s)| s.contains("straight edge")), "{:?}", p.a);

    // Three, 30 apart: before OK the view shows them, and nothing is made yet.
    typed(&mut d, &mut wb, "tn_pattern_count", "3");
    typed(&mut d, &mut wb, "tn_pattern_spacing", "30");
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.scene().bodies.len(), 4, "the plate and three blocks");
    assert_eq!(wb.assembly().unwrap().assembly().components.len(), 2);
    let top = |wb: &Workbench| wb.scene().bbox().unwrap();
    assert!((wb.scene().bodies.iter().map(|b| b.volume).sum::<f64>() - (200000.0 + 3000.0)).abs() < 1e-6);
    let _ = top;

    // A second direction, along the plate's right edge: two rows 20 apart.
    d.click(&mut wb, pressable(&d, "tn_pattern_second"));
    d.frame(&mut wb, vec![]);
    let edge = on_screen(&wb, Vec3::new(200.0, 50.0, 10.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(edge)]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, edge);
    d.frame(&mut wb, vec![]);
    typed(&mut d, &mut wb, "tn_pattern_count2", "2");
    typed(&mut d, &mut wb, "tn_pattern_spacing2", "20");
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.scene().bodies.len(), 7, "the plate and six blocks");

    // OK: an element wherever i and j say.
    press_ok(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!(!wb.status_error && wb.status().contains("Pattern:1"), "{}", wb.status());
    let asm = wb.assembly().unwrap().assembly().clone();
    assert_eq!(asm.components.len(), 7);
    assert_eq!(asm.patterns.len(), 1);
    // (The edge may run either way along: the elements then go from the far side of the source.)
    let along = placement(&wb, 3).origin() - placement(&wb, 2).origin();
    assert!((along.len() - 30.0).abs() < 1e-6 && along.y.abs() < 1e-6 && along.z.abs() < 1e-6, "{along:?}");
    let up = placement(&wb, 5).origin() - placement(&wb, 2).origin();
    assert!((up.len() - 20.0).abs() < 1e-6 && up.x.abs() < 1e-6 && up.z.abs() < 1e-6, "{up:?}");

    // The browser: the pattern's row, its elements under it, what is left of the rest.
    browser_row(&d, "Pattern:1 (3 x 2)");
    for row in ["block:1", "block:2", "block:6", "plate:1"] {
        browser_row(&d, row);
    }
    let rows = tenon_assembly::session::bom(&asm, &wb.assembly().unwrap().parts);
    assert_eq!(rows.iter().map(|r| (r.name.as_str(), r.quantity)).collect::<Vec<_>>(), [("Plate", 1), ("Block", 6)]);

    // An element is the pattern's to place: a drag leaves it, and says why.
    let f = placement(&wb, 3);
    let at = on_screen(&wb, f.to_world(Vec3::new(5.0, 5.0, 10.0)));
    d.drag(&mut wb, at, at + vec2(40.0, 10.0), egui::PointerButton::Primary);
    d.frame(&mut wb, vec![]);
    assert!(wb.status().contains("an element of Pattern:1"), "{}", wb.status());
    assert!(near(placement(&wb, 3).origin(), f.origin()));
    // Its source drags them all.
    let from = placement(&wb, 2);
    let at = on_screen(&wb, from.to_world(Vec3::new(5.0, 5.0, 10.0)));
    d.drag(&mut wb, at, at + vec2(0.0, -30.0), egui::PointerButton::Primary);
    d.frame(&mut wb, vec![]);
    let moved = placement(&wb, 2).origin() - from.origin();
    assert!(moved.len() > 1.0, "{moved:?}");
    assert!(near(placement(&wb, 3).origin(), f.origin() + moved), "{:?}", placement(&wb, 3));
    wb.run_ui("edit.undo").unwrap();
    d.frame(&mut wb, vec![]);

    // Double-click its row: its panel again, with what it has. Count 2 takes the third along each
    // row away; what is left stays the same components.
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    let row = browser_row(&d, "Pattern:1 (3 x 2)").center();
    d.frame(&mut wb, vec![egui::Event::PointerMoved(row)]);
    d.click(&mut wb, row);
    d.click(&mut wb, row);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Asm(p)) = &wb.panel else { panic!("{}", wb.status()) };
    assert_eq!((p.tool, p.count, p.count2, p.spacing, p.spacing2), (AsmTool::Pattern, 3.0, 2.0, 30.0, 20.0));
    assert!(p.pattern.is_some() && p.b.is_some());
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t == "> Pattern:1"), "{:?}", d.texts());
    typed(&mut d, &mut wb, "tn_pattern_count", "2");
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.scene().bodies.len(), 5, "the preview has four blocks");
    press_ok(&mut d, &mut wb);
    browser_row(&d, "Pattern:1 (2 x 2)");
    assert_eq!(wb.assembly().unwrap().assembly().components.len(), 5);
    assert!(wb.assembly().unwrap().assembly().component(ComponentId(3)).is_some());

    // Suppress from its menu: its elements are out; back again; then delete.
    menu_on(&mut d, &mut wb, "Pattern:1 (2 x 2)");
    d.click(&mut wb, d.text_pos("Suppress").expect("the menu"));
    d.settle(&mut wb);
    assert_eq!(wb.scene().bodies.len(), 2, "the plate and the source");
    let rows = tenon_assembly::session::bom(wb.assembly().unwrap().assembly(), &wb.assembly().unwrap().parts);
    assert_eq!(rows[1].quantity, 1);
    menu_on(&mut d, &mut wb, "Pattern:1 (2 x 2)");
    d.click(&mut wb, d.text_pos("Unsuppress").expect("the menu"));
    d.settle(&mut wb);
    assert_eq!(wb.scene().bodies.len(), 5);
    menu_on(&mut d, &mut wb, "Pattern:1 (2 x 2)");
    d.click(&mut wb, d.text_pos("Delete").expect("the menu"));
    d.settle(&mut wb);
    assert_eq!(wb.assembly().unwrap().assembly().components.len(), 2);
    assert!(wb.assembly().unwrap().assembly().patterns.is_empty());
    wb.run_ui("edit.undo").unwrap();
    d.settle(&mut wb);
    assert_eq!(wb.assembly().unwrap().assembly().components.len(), 5);
    assert!(!wb.status_error, "{}", wb.status());
}

#[test]
fn a_circular_pattern_takes_its_axis_from_a_round_face_and_esc_leaves_nothing() {
    let dir = scratch("round");
    let mut s = Session::default();
    let run = |s: &mut Session, id: &str, p: serde_json::Value| tenon_io::cmd::run(s, id, &p, None).unwrap();
    run(&mut s, "document.rename", json!({ "name": "Hub" }));
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(&mut s, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 10 }));
    run(&mut s, "model.extrude", json!({ "sketch": sk, "distance": 40 }));
    tenon_io::project::save(&dir.join("hub.tenon"), s.document(), &serde_json::Map::new()).unwrap();
    box_part(&dir.join("lug.tenon"), "Lug", 10.0, 6.0, 6.0);
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    opened(&mut wb, &mut d, &[(&dir.join("hub.tenon"), [0.0, 0.0, 0.0]), (&dir.join("lug.tenon"), [30.0, -3.0, 10.0])]);

    // With the lug selected first, Pattern starts on it and asks for the axis.
    let lug = on_screen(&wb, Vec3::new(35.0, -3.0, 13.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(lug)]);
    d.click(&mut wb, lug);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.asm.as_ref().unwrap().selected, [ComponentId(2)]);
    d.click(&mut wb, pressable(&d, "ribbon asm.pattern"));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "tn_pattern_circular"));
    d.frame(&mut wb, vec![]);
    let Some(Panel::Asm(p)) = &wb.panel else { panic!() };
    assert!(p.circular && p.components == [ComponentId(2)]);
    assert_eq!(p.degrees, 360.0, "all round");

    // A flat face gives no axis: said in words, and the selection is still open.
    let flat = on_screen(&wb, Vec3::new(0.0, 0.0, 40.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(flat)]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, flat);
    d.frame(&mut wb, vec![]);
    assert!(wb.status_error && wb.status().contains("round face"), "{}", wb.status());
    let Some(Panel::Asm(p)) = &wb.panel else { panic!() };
    assert!(p.a.is_none());

    // The hub's round side: the axis. Four all round.
    let side = on_screen(&wb, Vec3::new(10.0 * 0.5f64.sqrt(), -10.0 * 0.5f64.sqrt(), 20.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(side)]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, side);
    d.frame(&mut wb, vec![]);
    typed(&mut d, &mut wb, "tn_pattern_count", "4");
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.scene().bodies.len(), 5, "the hub and four lugs");

    // Esc: nothing is made, and the view shows what is there again.
    d.tap(&mut wb, egui::Key::Escape);
    d.settle(&mut wb);
    assert!(wb.panel.is_none());
    assert_eq!(wb.scene().bodies.len(), 2);
    assert!(wb.assembly().unwrap().preview().is_none());

    // Again, to OK: a quarter turn from one to the next, each turned with it. Whichever way the
    // face's axis runs, the second is on the Y axis at 30.
    wb.command("asm.pattern");
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "tn_pattern_circular"));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(side)]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, side);
    d.frame(&mut wb, vec![]);
    // (Enter in a value box is OK, as in every panel.)
    type_into(&mut d, &mut wb, "tn_pattern_count", "4");
    d.tap(&mut wb, egui::Key::Enter);
    d.settle(&mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    browser_row(&d, "Pattern:1 (4)");
    let f = placement(&wb, 3);
    assert!((f.origin().x.abs() - 3.0).abs() < 1e-6 && (f.origin().y.abs() - 30.0).abs() < 1e-6, "{f:?}");
    assert!((f.x().y.abs() - 1.0).abs() < 1e-6, "turned with it: {f:?}");
    assert!((wb.scene().bodies.iter().map(|b| b.volume).sum::<f64>() - (std::f64::consts::PI * 100.0 * 40.0 + 4.0 * 360.0)).abs() < 1e-6);
}

#[test]
fn a_mirror_is_made_by_clicking_and_its_part_is_reflected_in_the_view() {
    let dir = scratch("mirror");
    post_part(&dir.join("post.tenon"));
    box_part(&dir.join("wall.tenon"), "Wall", 4.0, 60.0, 40.0);
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    let parts = [(dir.join("wall.tenon"), [-2.0, -20.0, 0.0]), (dir.join("post.tenon"), [20.0, 0.0, 0.0])];
    opened(&mut wb, &mut d, &[(&parts[0].0, parts[0].1), (&parts[1].0, parts[1].1)]);
    wb.asm_exec("asm.ground", json!({ "component": 1, "grounded": false })).unwrap();
    wb.asm_exec("asm.place", json!({ "component": 1, "origin": [-2, -20, 0] })).unwrap();
    wb.asm_exec("asm.ground", json!({ "component": 1, "grounded": true })).unwrap();
    d.settle(&mut wb);
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);
    let post = placement(&wb, 2);
    assert!(near(post.origin(), Vec3::new(20.0, 0.0, 0.0)), "{post:?}");

    // Mirror: the post by a click; the plane slot; the wall's near face, at x = 2.
    d.click(&mut wb, pressable(&d, "ribbon asm.mirror"));
    d.frame(&mut wb, vec![]);
    assert!(matches!(&wb.panel, Some(Panel::Asm(p)) if p.tool == AsmTool::Mirror));
    let on_post = on_screen(&wb, Vec3::new(45.0, 10.0, 10.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(on_post)]);
    d.click(&mut wb, on_post);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "tn_pattern_first"));
    d.frame(&mut wb, vec![]);
    let face = on_screen(&wb, Vec3::new(2.0, 35.0, 20.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(face)]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, face);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Asm(p)) = &wb.panel else { panic!() };
    assert!(p.a.as_ref().is_some_and(|(_, s)| s.starts_with("wall:1")), "{:?}: {}", p.a, wb.status());
    // The view already shows it, in the part's mirror image.
    assert_eq!(wb.scene().bodies.len(), 3, "{}", wb.status());

    // A part not the same as its mirror image cannot be placed again: said in words, in the panel.
    d.click(&mut wb, pressable(&d, "tn_mirror_reuse_2"));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Asm(p)) = &wb.panel else { panic!() };
    assert!(matches!(&p.preview, Some((_, Err(e))) if e.contains("not the same as its mirror image")), "{:?}", p.preview);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.status_error && wb.status().contains("mirror image"), "{}", wb.status());
    assert!(wb.panel.is_some());
    assert_eq!(wb.assembly().unwrap().assembly().components.len(), 2);
    let Some(Panel::Asm(p)) = &mut wb.panel else { panic!() };
    p.reuse.clear();

    // OK. Its origin 18 from the plane on the far side: at x = 2 - 18 = -16, and the post, which
    // on the source runs x 20 to 30, y 0 to 10, runs x -26 to -16 on it.
    d.tap(&mut wb, egui::Key::Enter);
    d.settle(&mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let asm = wb.assembly().unwrap().assembly().clone();
    let image = asm.component(ComponentId(3)).unwrap();
    assert!(image.mirrored);
    assert_eq!(image.part, asm.component(ComponentId(2)).unwrap().part);
    assert!(near(image.placement.origin(), Vec3::new(-16.0, 0.0, 0.0)), "{:?}", image.placement);

    // Drawn so: its centre of mass (17, 9, 8) on the source's own coordinates, reflected: 20 + 17
    // = 37 goes to 2 - 35 = -33. Its normals are outward, as the mesh encloses the part.
    let a = wb.asm.as_ref().unwrap();
    let body = a.map.iter().position(|(c, _)| *c == ComponentId(3)).expect("drawn");
    let b = &wb.scene().bodies[body];
    assert!(near(b.mass.center_of_mass, Vec3::new(-33.0, 9.0, 8.0)), "{:?}", b.mass.center_of_mass);
    assert!((b.volume - 10000.0).abs() < 1e-6);
    let p = b.placed();
    let enclosed: f64 = b
        .mesh
        .indices
        .chunks_exact(3)
        .map(|t| {
            let at = |i: u32| p.point(b.mesh.positions[i as usize]);
            at(t[0]).dot(at(t[1]).cross(at(t[2]))) / 6.0
        })
        .sum();
    assert!((enclosed - 10000.0).abs() < 1e-1, "{enclosed}");

    // Picked where the mirror image is, not where the part would be unreflected: a click on the
    // post's top selects it, and over the foot's far end (x -56 to -46) there is nothing above z 10.
    d.settle(&mut wb);
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);
    let top = on_screen(&wb, Vec3::new(-21.0, 5.0, 30.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(top)]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, top);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.asm.as_ref().unwrap().selected, [ComponentId(3)], "{}", wb.status());
    assert!(wb.status().contains("an element of Mirror:1"), "{}", wb.status());

    // Listed as a mirrored part, on its own row, and in the browser as one.
    browser_row(&d, "Mirror:1");
    browser_row(&d, "post:2 (mirrored)");
    let rows = tenon_assembly::session::bom(&asm, &wb.assembly().unwrap().parts);
    assert_eq!(
        rows.iter().map(|r| (r.name.as_str(), r.quantity, r.mirrored)).collect::<Vec<_>>(),
        [("Wall", 1, false), ("Post", 1, false), ("Post, mirrored", 1, true)]
    );

    // The source dragged: its image goes the other way. The wall stays.
    let from = placement(&wb, 2);
    let at = on_screen(&wb, from.to_world(Vec3::new(30.0, 10.0, 10.0)));
    d.drag(&mut wb, at, at + vec2(60.0, 0.0), egui::PointerButton::Primary);
    d.settle(&mut wb);
    let moved = placement(&wb, 2).origin() - from.origin();
    assert!(moved.x.abs() > 1.0, "{moved:?}");
    let want = Vec3::new(-16.0 - moved.x, moved.y, moved.z);
    assert!(near(placement(&wb, 3).origin(), want), "{:?} against {want:?}", placement(&wb, 3).origin());

    // Double-click its row: the panel again, on what it has. Esc leaves it as it was.
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    let row = browser_row(&d, "Mirror:1").center();
    d.frame(&mut wb, vec![egui::Event::PointerMoved(row)]);
    d.click(&mut wb, row);
    d.click(&mut wb, row);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Asm(p)) = &wb.panel else { panic!("{}", wb.status()) };
    assert_eq!((p.tool, p.components.as_slice()), (AsmTool::Mirror, [ComponentId(2)].as_slice()));
    assert!(p.a.is_some());
    d.tap(&mut wb, egui::Key::Escape);
    d.settle(&mut wb);

    // A double-click on the mirrored occurrence opens its part, as itself: the source is what is
    // edited, and the image follows on the way back.
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    let row = browser_row(&d, "post:2 (mirrored)").center();
    d.frame(&mut wb, vec![egui::Event::PointerMoved(row)]);
    d.click(&mut wb, row);
    d.click(&mut wb, row);
    d.frame(&mut wb, vec![]);
    assert!(wb.editing_in_place(), "{}", wb.status());
    assert_eq!(wb.document().name, "Post");
    assert!(!wb.context_meshes().is_empty());
    wb.exec("model.box", json!({ "x": 30, "y": 0, "width": 10, "depth": 10, "height": 20 })).unwrap();
    d.frame(&mut wb, vec![]);
    wb.command("asm.return");
    d.settle(&mut wb);
    assert!(wb.in_assembly());
    let a = wb.asm.as_ref().unwrap();
    let body = a.map.iter().position(|(c, _)| *c == ComponentId(3)).expect("drawn");
    assert!((wb.scene().bodies[body].volume - 11000.0).abs() < 1e-6, "another 10 x 10 above the foot: {}", wb.scene().bodies[body].volume);
    assert!(!wb.status_error, "{}", wb.status());

    // Standing alone, made independent and its source gone, it opens its part just the same, with
    // nothing of the rest round it: that would be reflected.
    wb.asm_exec("asm.delete_pattern", json!({ "pattern": 1, "keep_elements": true })).unwrap();
    wb.asm_exec("asm.delete", json!({ "component": 2 })).unwrap();
    d.settle(&mut wb);
    wb.edit_in_place(ComponentId(3)).unwrap();
    d.frame(&mut wb, vec![]);
    assert!(wb.editing_in_place() && wb.context_meshes().is_empty(), "{}", wb.status());
    assert!(wb.status().contains("without the rest"), "{}", wb.status());
    wb.command("asm.return");
    d.settle(&mut wb);
    assert!(wb.in_assembly());
    assert_eq!(wb.scene().bodies.len(), 2);
}

#[test]
fn a_symmetric_part_is_placed_again_from_the_mirror_panel() {
    let dir = scratch("again");
    box_part(&dir.join("base.tenon"), "Base", 10.0, 10.0, 10.0);
    box_part(&dir.join("cube.tenon"), "Cube", 10.0, 10.0, 10.0);
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    opened(&mut wb, &mut d, &[(&dir.join("base.tenon"), [0.0, 0.0, 0.0]), (&dir.join("cube.tenon"), [30.0, 0.0, 0.0])]);
    // The base's right face, x = 10, for the plane; the cube, 20 from it, then lands 20 on the
    // other side: from x = -20 to -10.
    let cube = on_screen(&wb, Vec3::new(35.0, 5.0, 10.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(cube)]);
    d.click(&mut wb, cube);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "ribbon asm.mirror"));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let face = on_screen(&wb, Vec3::new(10.0, 5.0, 5.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(face)]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, face);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "tn_mirror_reuse_2"));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::Enter);
    d.settle(&mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let asm = wb.assembly().unwrap().assembly().clone();
    let image = asm.component(ComponentId(3)).unwrap();
    assert!(!image.mirrored, "the same part");
    assert!(near(image.placement.origin(), Vec3::new(-20.0, 0.0, 0.0)) && near(image.placement.x(), Vec3::X), "{:?}", image.placement);
    browser_row(&d, "cube:2");
    let rows = tenon_assembly::session::bom(&asm, &wb.assembly().unwrap().parts);
    assert_eq!(rows.iter().map(|r| (r.name.as_str(), r.quantity)).collect::<Vec<_>>(), [("Base", 1), ("Cube", 2)]);
    // The file says how it is to be placed again: read back, it is what it was.
    let file = dir.join("again.tenonasm");
    let to = file.clone();
    wb.services.pick_save = Some(Box::new(move |_: &str, _: &str| Some(to.clone())));
    wb.run_ui("file.save").unwrap();
    wb.open(&file).unwrap();
    d.settle(&mut wb);
    let back = wb.assembly().unwrap().assembly();
    assert_eq!(back.patterns.len(), 1);
    assert!(near(back.component(ComponentId(3)).unwrap().placement.origin(), Vec3::new(-20.0, 0.0, 0.0)));
    browser_row(&d, "Mirror:1");
}

#[test]
fn copy_makes_independent_copies_beside_their_source_or_where_a_click_puts_them() {
    let dir = scratch("copy");
    box_part(&dir.join("base.tenon"), "Base", 40.0, 40.0, 10.0);
    box_part(&dir.join("cube.tenon"), "Cube", 10.0, 10.0, 10.0);
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    opened(&mut wb, &mut d, &[(&dir.join("base.tenon"), [0.0, 0.0, 0.0]), (&dir.join("cube.tenon"), [5.0, 5.0, 10.0])]);

    // Copy, the cube by a click: the view shows a copy beside it, 10 wide and a gap of 5.
    d.click(&mut wb, pressable(&d, "ribbon asm.copy"));
    d.frame(&mut wb, vec![]);
    assert!(matches!(&wb.panel, Some(Panel::Asm(p)) if p.tool == AsmTool::Copy));
    let cube = on_screen(&wb, Vec3::new(10.0, 10.0, 20.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(cube)]);
    d.click(&mut wb, cube);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Asm(p)) = &wb.panel else { panic!() };
    assert_eq!(p.by, [15.0, 0.0, 0.0]);
    assert_eq!(wb.scene().bodies.len(), 3);
    assert_eq!(wb.assembly().unwrap().assembly().components.len(), 2);

    // Told how far, it goes there: 12 along Y, up 3.
    typed(&mut d, &mut wb, "tn_copy_y", "12");
    typed(&mut d, &mut wb, "tn_copy_z", "3");
    d.frame(&mut wb, vec![]);
    let Some(Panel::Asm(p)) = &wb.panel else { panic!() };
    assert_eq!(p.by, [15.0, 12.0, 3.0]);
    assert!(!p.by_auto);
    press_ok(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let f = placement(&wb, 3);
    assert!(near(f.origin(), Vec3::new(20.0, 17.0, 13.0)), "{f:?}");
    browser_row(&d, "cube:2");
    assert!(wb.assembly().unwrap().assembly().elements().is_empty(), "independent: it is no element of anything");
    assert_eq!(wb.asm.as_ref().unwrap().selected, [ComponentId(3)], "and ready to be moved");

    // Undo takes it back; it is free to drag, unlike the grounded base it was not copied from.
    wb.run_ui("edit.undo").unwrap();
    d.settle(&mut wb);
    assert_eq!(wb.assembly().unwrap().assembly().components.len(), 2);

    // With what is selected already; put down by a click: it follows the pointer, in the view's
    // plane through its middle, until the click.
    let a = wb.asm.as_mut().unwrap();
    a.selected = vec![ComponentId(2)];
    d.click(&mut wb, pressable(&d, "ribbon asm.copy"));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "tn_copy_where"));
    d.frame(&mut wb, vec![]);
    let middle = Vec3::new(10.0, 10.0, 15.0);
    let here = on_screen(&wb, middle + wb.view.camera.right() * 30.0);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(here)]);
    d.frame(&mut wb, vec![]);
    let by = |wb: &Workbench| match &wb.panel {
        Some(Panel::Asm(p)) => Vec3::new(p.by[0], p.by[1], p.by[2]),
        _ => panic!(),
    };
    let want = wb.view.camera.right() * 30.0;
    assert!(by(&wb).near(want, 0.2), "{:?} against {want:?}", by(&wb));
    let there = on_screen(&wb, middle - wb.view.camera.right() * 20.0);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(there)]);
    d.click(&mut wb, there);
    d.frame(&mut wb, vec![]);
    let put = by(&wb);
    let want = wb.view.camera.right() * -20.0;
    assert!(put.near(want, 0.2), "{put:?} against {want:?}");
    // (Put down: moving the pointer on leaves it.)
    let away = on_screen(&wb, middle);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(away)]);
    d.frame(&mut wb, vec![]);
    assert!(by(&wb).near(put, 1e-9));

    // New part files want a folder: an assembly not saved yet says so.
    d.click(&mut wb, pressable(&d, "tn_copy_new_files"));
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.status_error && wb.status().contains("Save the assembly first"), "{}", wb.status());
    assert!(wb.panel.is_some());
    let file = dir.join("copies.tenonasm");
    let to = file.clone();
    wb.services.pick_save = Some(Box::new(move |_: &str, _: &str| Some(to.clone())));
    wb.save_assembly(&file).unwrap();
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::Enter);
    d.settle(&mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!(dir.join("cube-copy.tenon").exists());
    let asm = wb.assembly().unwrap().assembly().clone();
    assert_eq!(asm.component(ComponentId(3)).unwrap().name, "cube-copy:1");
    browser_row(&d, "cube-copy:1");
    let rows = tenon_assembly::session::bom(&asm, &wb.assembly().unwrap().parts);
    assert_eq!(rows.iter().map(|r| (r.name.as_str(), r.quantity)).collect::<Vec<_>>(), [("Base", 1), ("Cube", 1), ("Cube Copy", 1)]);
    assert!(!wb.status_error, "{}", wb.status());
}

#[test]
fn every_button_of_the_pattern_group_is_there_to_press() {
    for id in ["asm.pattern", "asm.mirror", "asm.copy"] {
        let c = crate::commands::find(id).unwrap();
        assert_eq!(c.milestone, 0, "{id}");
        assert!(c.available(), "{id}");
    }
    // In a part they do not run, and say what to do.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let e = wb.run_ui("asm.pattern").unwrap_err();
    assert!(e.contains("assembly"), "{e}");
}

#[test]
fn a_mirrored_part_comes_from_the_worker_in_a_slot_of_its_own() {
    use std::time::{Duration, Instant};
    let dir = scratch("worker");
    post_part(&dir.join("post.tenon"));
    // The assembly on disk, as the app opens it: a post, and its mirror image in the YZ plane.
    let mut s = tenon_assembly::AsmSession::default();
    let mut k = OcctKernel::new();
    let post = dir.join("post.tenon").to_string_lossy().into_owned();
    tenon_io::asm::run(&mut s, "asm.insert", &json!({ "path": post, "at": [100, 0, 0], "grounded": false }), Some(&mut k)).unwrap();
    let plane = tenon_io::asm::run(&mut s, "asm.geom", &json!({ "plane": "yz" }), Some(&mut k)).unwrap();
    tenon_io::asm::run(&mut s, "asm.mirror", &json!({ "components": [1], "plane": plane }), Some(&mut k)).unwrap();
    let file = dir.join("pair.tenonasm");
    tenon_io::asm::run(&mut s, "asm.save", &json!({ "path": file.to_string_lossy() }), None).unwrap();

    let mut wb = Workbench::new(|| Box::new(OcctKernel::new()) as Box<dyn tenon_kernel::Kernel>, None, crate::Services::default());
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.open(&file).unwrap();
    let wait = |wb: &mut Workbench, d: &mut Driver, want: &dyn Fn(&Workbench) -> bool| {
        let t = Instant::now();
        while !want(wb) && t.elapsed() < Duration::from_secs(60) {
            d.frame(wb, vec![]);
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    wait(&mut wb, &mut d, &|wb| wb.scene().bodies.len() == 2);
    let a = wb.asm.as_ref().unwrap();
    let body = a.map.iter().position(|(c, _)| *c == ComponentId(2)).expect("drawn");
    assert!(near(wb.scene().bodies[body].mass.center_of_mass, Vec3::new(-117.0, 9.0, 8.0)), "{:?}", wb.scene().bodies[body].mass.center_of_mass);
    // Opening it changed nothing: there is nothing to save, and nothing to undo.
    for _ in 0..10 {
        d.frame(&mut wb, vec![]);
    }
    assert!(!wb.assembly().unwrap().is_dirty(), "it has only been opened");
    assert!(!wb.assembly().unwrap().can_undo());

    // The mirror of another part, through the panel's preview: its image arrives from the worker
    // for the preview, is dropped with the panel, and comes again for what is made.
    let a = wb.asm.as_mut().unwrap();
    a.selected = vec![ComponentId(1)];
    wb.command("asm.mirror");
    d.frame(&mut wb, vec![]);
    let Some(Panel::Asm(p)) = &mut wb.panel else { panic!() };
    let target = tenon_assembly::Target { component: None, geom: tenon_assembly::Geom::Plane { plane: tenon_model::OriginPlane::XZ } };
    p.a = Some((target, "Assembly: XZ".to_owned()));
    wait(&mut wb, &mut d, &|wb| wb.scene().bodies.len() == 3);
    assert_eq!(wb.scene().bodies.len(), 3, "{}", wb.status());
    wb.panel = None;
    wait(&mut wb, &mut d, &|wb| wb.scene().bodies.len() == 2);
    assert_eq!(wb.scene().bodies.len(), 2);
    assert!(wb.assembly().unwrap().preview().is_none());
    assert!(!wb.status_error, "{}", wb.status());
}

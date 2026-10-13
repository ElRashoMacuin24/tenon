//! Primitives (3D Model > Primitives), by real input: the ribbon button and its list, clicks on
//! a plane and a face, the pointer sizing the solid, values typed into the panel.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use egui::vec2;
use serde_json::json;
use tenon_geom::Vec3;
use tenon_kernel_occt::OcctKernel;
use tenon_model::{FeatureKind, Operation, OriginPlane, PlaneRef};

use crate::Workbench;
use crate::panels::Panel;
use crate::primitive::{PrimitivePanel, Shape};
use crate::tests::{Driver, browser_row, on_screen, type_into};

fn volume(wb: &Workbench) -> f64 {
    wb.scene().bodies.iter().map(|b| b.volume).sum()
}

fn panel(wb: &Workbench) -> &PrimitivePanel {
    match &wb.panel {
        Some(Panel::Primitive(p)) => p,
        other => panic!("no primitive panel: {other:?}"),
    }
}

fn names(wb: &Workbench) -> Vec<String> {
    wb.document().features().iter().map(|f| f.name.clone()).collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * b.abs().max(1.0)
}

/// Moves the pointer there and lets the preview follow.
fn hover(d: &mut Driver, wb: &mut Workbench, at: egui::Pos2) {
    d.frame(wb, vec![egui::Event::PointerMoved(at)]);
    d.frame(wb, vec![]);
}

#[test]
fn a_box_is_made_by_a_plane_its_centre_a_corner_and_its_height() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);

    // The button on the 3D Model tab: the panel opens and waits for a plane, with the origin
    // planes shown.
    let button = d.text_pos("Box").expect("the Box button on the ribbon");
    d.click(&mut wb, button);
    assert!(panel(&wb).on.is_none() && wb.primitive_wants_plane(), "{}", wb.status());
    assert!(wb.status().starts_with("Box: select a plane"), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t == "XY Plane"), "the origin planes are shown");

    // A click on the XY plane (where no other plane is in front of it).
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 0.0));
    let plane = at(&wb, 15.0, -10.0);
    hover(&mut d, &mut wb, plane);
    assert_eq!(wb.view.plane_hover, Some(OriginPlane::XY), "the plane under the pointer is shown as such");
    d.click(&mut wb, plane);
    assert!(matches!(panel(&wb).on, Some((PlaneRef::Origin(OriginPlane::XY), _))), "{:?}", panel(&wb).on);
    assert!(wb.status().starts_with("Box: click where its centre is"), "{}", wb.status());
    assert!(wb.document().features().is_empty() && volume(&wb) == 0.0, "nothing is made yet");

    // Its centre, then the pointer sizes it: the solid is shown as it will be.
    let centre = at(&wb, 20.0, -15.0);
    d.click(&mut wb, centre);
    assert_eq!(panel(&wb).center.map(|c| (c.x, c.y)), Some((20.0, -15.0)));
    assert_eq!(wb.status(), "Box: click a corner.");
    let corner = at(&wb, 32.0, -7.0);
    hover(&mut d, &mut wb, corner);
    assert_eq!((panel(&wb).width, panel(&wb).depth), (24.0, 16.0), "twice the way from the centre to the pointer, each way");
    assert!(near(volume(&wb), 24.0 * 16.0 * 10.0), "the preview: {}", volume(&wb));
    assert!(wb.document().features().is_empty(), "still only a preview");
    let other = at(&wb, 26.0, -12.0);
    hover(&mut d, &mut wb, other);
    assert!(near(volume(&wb), 12.0 * 6.0 * 10.0), "it follows the pointer: {}", volume(&wb));
    d.click(&mut wb, corner);
    assert_eq!((panel(&wb).sizing, panel(&wb).width, panel(&wb).depth), (0, 24.0, 16.0));
    assert_eq!(wb.status(), "Box: set its height, then OK.");
    // The corner is set: the pointer no longer moves it.
    hover(&mut d, &mut wb, other);
    assert_eq!((panel(&wb).width, panel(&wb).depth), (24.0, 16.0));

    // Its height typed; Enter is OK. A sketch and its extrusion, as drawn by hand, in one step.
    type_into(&mut d, &mut wb, "tn_props_prim_h", "6");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(names(&wb), ["Sketch1", "Extrusion1"]);
    assert!(near(volume(&wb), 24.0 * 16.0 * 6.0), "{}", volume(&wb));
    let bb = wb.scene().bbox().unwrap();
    assert!(near(bb.min.x, 8.0) && near(bb.max.x, 32.0) && near(bb.min.y, -23.0) && near(bb.max.y, -7.0) && near(bb.max.z, 6.0), "{bb:?}");
    // The sketch is held by its dimensions: nothing in it is free to move.
    let sketch = wb.document().features()[0].id;
    let info = wb.exec("sketch.info", json!({ "sketch": sketch.0 })).unwrap();
    assert_eq!(info["dof"], 0, "{info}");
    wb.exec("edit.undo", json!({})).unwrap();
    assert!(wb.document().features().is_empty(), "one step, taken back as one");
    wb.exec("edit.redo", json!({})).unwrap();
    d.frame(&mut wb, vec![]);

    // A cylinder on the box's top face, from the list under the button.
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    let button = d.text_pos("Box").expect("the Box button");
    d.click(&mut wb, button + vec2(0.0, 16.0));
    d.frame(&mut wb, vec![]);
    let cylinder = d.text_pos("Cylinder").expect("Cylinder in the list under the button");
    assert!(d.text_pos("Sphere").is_some() && d.text_pos("Torus").is_some());
    d.click(&mut wb, cylinder);
    assert_eq!(panel(&wb).shape, Shape::Cylinder);
    let top = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 6.0));
    let on_top = top(&wb, 12.0, -20.0);
    hover(&mut d, &mut wb, on_top);
    assert!(wb.view.hover.is_some(), "the face under the pointer is shown as such");
    d.click(&mut wb, on_top);
    assert!(matches!(panel(&wb).on, Some((PlaneRef::Face(_), _))), "{:?}", panel(&wb).on);
    let middle = top(&wb, 20.0, -15.0);
    d.click(&mut wb, middle);
    let rim = top(&wb, 25.0, -15.0);
    hover(&mut d, &mut wb, rim);
    assert_eq!(panel(&wb).width, 10.0);
    d.click(&mut wb, rim);
    let block = 24.0 * 16.0 * 6.0;
    assert!(near(volume(&wb), block + PI * 25.0 * 10.0), "it stands on the face, out of the part: {}", volume(&wb));
    type_into(&mut d, &mut wb, "tn_props_prim_h", "4");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(names(&wb), ["Sketch1", "Extrusion1", "Sketch2", "Extrusion2"]);
    assert!(near(volume(&wb), block + PI * 25.0 * 4.0), "{}", volume(&wb));
    let bb = wb.scene().bbox().unwrap();
    assert!(near(bb.max.z, 10.0), "{bb:?}");
    assert!(matches!(&wb.document().features()[3].kind, FeatureKind::Extrude(e) if e.operation == Operation::Join));
}

#[test]
fn a_sphere_and_a_torus_are_made_from_the_browsers_planes_and_typed_sizes() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    d.frame(&mut wb, vec![]);

    // Sphere: the plane clicked in the browser's Origin folder, the centre at the plane's
    // origin (a click near it is on it), the size by the pointer and then typed.
    wb.command("model.sphere");
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let row = browser_row(&d, "XZ Plane").center();
    d.click(&mut wb, row);
    assert!(matches!(panel(&wb).on, Some((PlaneRef::Origin(OriginPlane::XZ), _))), "{}", wb.status());
    let origin = on_screen(&wb, Vec3::ZERO);
    d.click(&mut wb, origin + vec2(3.0, -2.0));
    assert_eq!(panel(&wb).center.map(|c| (c.x, c.y)), Some((0.0, 0.0)), "a click near the plane's origin is on it");
    let out = on_screen(&wb, Vec3::new(9.0, 0.0, 0.0));
    hover(&mut d, &mut wb, out);
    assert_eq!(panel(&wb).width, 18.0);
    assert!(near(volume(&wb), PI * 18.0_f64.powi(3) / 6.0), "{}", volume(&wb));
    d.click(&mut wb, out);
    type_into(&mut d, &mut wb, "tn_props_prim_w", "30");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(names(&wb), ["Sketch1", "Revolution1"]);
    assert!(near(volume(&wb), PI * 30.0_f64.powi(3) / 6.0), "{}", volume(&wb));
    wb.exec("edit.undo", json!({})).unwrap();
    // (The view turned to the first solid when it was made: let it come to rest.)
    d.settle(&mut wb);

    // Torus: three clicks after the plane (its centre, the middle of its ring, how thick),
    // then both sizes typed. Nothing is made while its ring is as thick as it is wide.
    wb.command("model.torus");
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let row = browser_row(&d, "XY Plane").center();
    d.click(&mut wb, row);
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 0.0));
    let centre = at(&wb, 10.0, -20.0);
    d.click(&mut wb, centre);
    assert_eq!(wb.status(), "Torus: click where the middle of its ring is.");
    let ring = at(&wb, 30.0, -20.0);
    hover(&mut d, &mut wb, ring);
    assert_eq!((panel(&wb).width, panel(&wb).depth), (40.0, 10.0), "a ring a quarter as thick as it is wide, until that is clicked");
    d.click(&mut wb, ring);
    assert_eq!(wb.status(), "Torus: click how thick its ring is.");
    let thick = at(&wb, 33.0, -20.0);
    hover(&mut d, &mut wb, thick);
    assert_eq!((panel(&wb).width, panel(&wb).depth), (40.0, 6.0));
    assert!(near(volume(&wb), 2.0 * PI * PI * 20.0 * 9.0), "{}", volume(&wb));
    // (Far out, it is still thinner than the ring is wide.)
    let far = at(&wb, 80.0, -20.0);
    hover(&mut d, &mut wb, far);
    assert!(panel(&wb).depth < panel(&wb).width);
    d.click(&mut wb, thick);
    type_into(&mut d, &mut wb, "tn_props_prim_d", "50");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_some() && wb.document().features().is_empty(), "a ring thicker than it is wide is not made");
    type_into(&mut d, &mut wb, "tn_props_prim_d", "8");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!(near(volume(&wb), 2.0 * PI * PI * 20.0 * 16.0), "{}", volume(&wb));
    let bb = wb.scene().bbox().unwrap();
    assert!(near((bb.min.x + bb.max.x) / 2.0, 10.0) && near((bb.min.y + bb.max.y) / 2.0, -20.0), "about where it was clicked: {bb:?}");

    // Esc leaves nothing behind; the Plane button starts the solid over on another plane.
    d.settle(&mut wb);
    wb.command("model.cylinder");
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let row = browser_row(&d, "XY Plane").center();
    d.click(&mut wb, row);
    let centre = at(&wb, -30.0, -40.0);
    d.click(&mut wb, centre);
    let rim = at(&wb, -25.0, -40.0);
    d.click(&mut wb, rim);
    assert!(panel(&wb).params().is_some());
    let again = crate::tests::pressable(&d, "tn_props_prim_plane");
    d.click(&mut wb, again);
    assert!(panel(&wb).on.is_none() && panel(&wb).center.is_none(), "the plane is asked for again");
    let before = names(&wb);
    d.tap(&mut wb, egui::Key::Escape);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none());
    assert_eq!(names(&wb), before);
    assert!(near(volume(&wb), 2.0 * PI * PI * 20.0 * 16.0), "the preview went with it: {}", volume(&wb));
}

#[test]
fn a_primitive_cuts_goes_the_other_way_and_stands_on_a_work_plane_or_a_face_picked_before() {
    use crate::tests::pressable;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    // A block 40 by 30 by 10 with a corner at the origin, and a work plane 25 above its base.
    wb.exec("model.box", json!({ "width": 40, "depth": 30, "height": 10 })).unwrap();
    wb.exec("work.plane", json!({ "by": "offset", "base": "xy", "distance": 25 })).unwrap();
    d.frame(&mut wb, vec![]);
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    let block = 40.0 * 30.0 * 10.0;
    assert!(near(volume(&wb), block));

    // The top face clicked first, then Cylinder: it stands on that face at once.
    let top = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 10.0));
    let face = top(&wb, 8.0, 6.0);
    d.click(&mut wb, face);
    assert!(matches!(wb.view.selection.as_slice(), [crate::viewport::Pick::Face { .. }]), "{:?}", wb.view.selection);
    wb.command("model.cylinder");
    d.frame(&mut wb, vec![]);
    assert!(matches!(panel(&wb).on, Some((PlaneRef::Face(_), _))), "{}", wb.status());
    assert!(wb.status().starts_with("Cylinder: click where its centre is"), "{}", wb.status());
    let centre = top(&wb, 20.0, 15.0);
    d.click(&mut wb, centre);
    let rim = top(&wb, 26.0, 15.0);
    d.click(&mut wb, rim);
    assert_eq!(panel(&wb).width, 12.0);
    assert!(near(volume(&wb), block + PI * 36.0 * 10.0), "joined, out of the face: {}", volume(&wb));
    // Cut, and the other way: into the part, a hole 10 deep, which is all the way through.
    d.frame(&mut wb, vec![]);
    let cut = pressable(&d, "tn_glyph Cut: remove material");
    d.click(&mut wb, cut);
    d.frame(&mut wb, vec![]);
    assert_eq!(panel(&wb).operation, Operation::Cut);
    assert!(near(volume(&wb), block), "a cut out of the face takes nothing: {}", volume(&wb));
    let flip = pressable(&d, "tn_glyph Flipped");
    d.click(&mut wb, flip);
    d.frame(&mut wb, vec![]);
    assert!(panel(&wb).reverse);
    assert!(near(volume(&wb), block - PI * 36.0 * 10.0), "{}", volume(&wb));
    let ok = d.text_pos("OK").expect("the OK button");
    d.click(&mut wb, ok);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!(near(volume(&wb), block - PI * 36.0 * 10.0), "{}", volume(&wb));
    assert!(matches!(&wb.document().features().last().unwrap().kind, FeatureKind::Extrude(e) if e.operation == Operation::Cut));

    // A sphere on the work plane, picked in the browser: a body of its own, up where the plane is.
    wb.command("model.sphere");
    // (The Origin folder is opened for the planes in it; folded, the work plane's row is in
    // sight below the properties panel.)
    wb.chrome.origin_open = false;
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let row = browser_row(&d, "Work Plane1").center();
    d.click(&mut wb, row);
    assert!(matches!(panel(&wb).on, Some((PlaneRef::Work(_), _))), "{}", wb.status());
    let up = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 25.0));
    let centre = up(&wb, 20.0, 15.0);
    d.click(&mut wb, centre);
    let rim = up(&wb, 24.0, 15.0);
    d.click(&mut wb, rim);
    d.frame(&mut wb, vec![]);
    let new_solid = pressable(&d, "tn_glyph New Solid: a separate body");
    d.click(&mut wb, new_solid);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(wb.scene().bodies.len(), 2);
    let ball = wb.scene().bodies.iter().find(|b| near(b.volume, PI * 8.0_f64.powi(3) / 6.0)).expect("the sphere, 8 across");
    let bb = ball.bbox.unwrap();
    assert!(near(bb.min.z, 21.0) && near(bb.max.z, 29.0) && near((bb.min.x + bb.max.x) / 2.0, 20.0), "{bb:?}");
}

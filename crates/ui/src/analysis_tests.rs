//! The Stress Analysis environment (Environments > Stress Analysis), by real input: the ribbon's
//! buttons, clicks on the part's faces, values typed into the panel, the study's rows in the
//! browser.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use egui::vec2;
use serde_json::json;
use tenon_geom::Vec3;
use tenon_kernel_occt::OcctKernel;
use tenon_model::study::{LoadKind, Study};

use crate::Workbench;
use crate::commands::{ANALYSIS_TAB, RIBBON};
use crate::panels::Panel;
use crate::tests::{Driver, browser_row, ctrl, on_screen, pressable, type_into};
use crate::viewport::Pick;

/// A bar 100 long (x), 20 wide (y) and 10 high (z), shown.
fn bar() -> (Workbench, Driver) {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    wb.exec("model.box", json!({ "plane": "xy", "width": 100, "depth": 20, "height": 10 })).unwrap();
    let mut d = Driver::new(vec2(1400.0, 860.0));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.scene().bodies.len(), 1, "{}", wb.status());
    (wb, d)
}

fn study(wb: &Workbench) -> &Study {
    wb.document().studies().first().expect("a study")
}

/// The ribbon's tabs, as they are drawn.
fn tabs(wb: &Workbench) -> Vec<&'static str> {
    wb.part_tabs().into_iter().map(|i| RIBBON[i].name).collect()
}

/// Where model point `at` (the middle of a face of the bar) is on screen.
fn face(wb: &Workbench, at: Vec3) -> egui::Pos2 {
    on_screen(wb, at)
}

/// The colour the face under model point `at` is picked out in, if it is.
fn lit(wb: &Workbench, at: Vec3) -> Option<egui::Color32> {
    let scene = wb.scene();
    wb.highlights().into_iter().find_map(|(p, c)| match p {
        Pick::Face { body, face } => {
            let info = &scene.bodies[body].faces[face as usize].1;
            ((info.centroid - at).len() < 1e-6).then_some(c)
        }
        Pick::Edge { .. } => None,
    })
}

/// Whether a mark of a support ("held") or of a load ("load") is drawn where model point `at`
/// is on screen.
fn marked(d: &Driver, wb: &Workbench, what: &str, at: Vec3) -> bool {
    let marks: Vec<(&'static str, egui::Pos2)> = d.ctx.data(|x| x.get_temp(egui::Id::new("tn_analysis_marks"))).unwrap_or_default();
    let there = on_screen(wb, at);
    marks.iter().any(|(kind, p)| *kind == what && (*p - there).length() < 1.0)
}

fn click_text(d: &mut Driver, wb: &mut Workbench, text: &str) {
    let at = d.text_pos(text).unwrap_or_else(|| panic!("no `{text}` on screen: {:?}", d.texts()));
    d.click(wb, at);
    d.frame(wb, vec![]);
}

/// Two clicks at one place, after a pause: clicks soon after earlier ones would count as a
/// triple click.
fn double_click(d: &mut Driver, wb: &mut Workbench, at: egui::Pos2) {
    d.frame(wb, vec![egui::Event::PointerMoved(at)]);
    for _ in 0..45 {
        d.frame(wb, vec![]);
    }
    d.click(wb, at);
    d.click(wb, at);
}

const LEFT: Vec3 = Vec3 { x: 0.0, y: 10.0, z: 5.0 };
const RIGHT: Vec3 = Vec3 { x: 100.0, y: 10.0, z: 5.0 };
const TOP: Vec3 = Vec3 { x: 50.0, y: 10.0, z: 10.0 };

#[test]
fn a_bar_is_held_at_one_end_and_loaded_at_the_other_by_clicks() {
    let (mut wb, mut d) = bar();

    // Environments > Stress Analysis: the ribbon is the environment's, the browser the study's.
    // (The 3D Model tab's Simulation panel has the same button.)
    assert_eq!(crate::commands::location("analysis.begin").as_deref(), Some("3D Model > Simulation"));
    assert!(d.text_pos("Stress").is_some(), "on the 3D Model tab");
    click_text(&mut d, &mut wb, "Environments");
    click_text(&mut d, &mut wb, "Stress");
    assert!(wb.in_analysis(), "{}", wb.status());
    assert_eq!(wb.chrome.tab, ANALYSIS_TAB);
    assert_eq!(tabs(&wb), ["Analysis", "Inspect", "Tools", "View"]);
    assert!(wb.status().starts_with("Stress Analysis: Create Study"), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    browser_row(&d, "No study yet: Create Study");
    assert!(wb.document().studies().is_empty());

    // Fixed, with no study yet: the study is made, and the panel waits for faces.
    wb.look_from(Vec3::new(-1.0, -1.0, 1.0));
    d.settle(&mut wb);
    click_text(&mut d, &mut wb, "Fixed");
    assert!(matches!(wb.panel, Some(Panel::Support(_))), "{}", wb.status());
    assert_eq!(wb.status(), "Fixed: click the faces that are held still, then OK.");
    assert_eq!(study(&wb).name, "Study1");
    // OK with no face says what is missing, and the panel stays.
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.status().contains("click at least one face"), "{}", wb.status());
    assert!(matches!(wb.panel, Some(Panel::Support(_))));
    // The left end, clicked: it is the panel's. Clicked again it is not; a third time it is.
    let left = face(&wb, LEFT);
    for expected in [1, 0, 1] {
        d.click(&mut wb, left);
        let Some(Panel::Support(p)) = &wb.panel else { panic!("no panel: {}", wb.status()) };
        assert_eq!(p.faces.len(), expected);
    }
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!((study(&wb).supports.len(), study(&wb).supports[0].name.as_str(), study(&wb).supports[0].faces.len()), (1, "Fixed1", 1));
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Constraints(1)");
    browser_row(&d, "Fixed1");
    browser_row(&d, "Loads(0)");
    assert!(marked(&d, &wb, "held", LEFT), "the ground mark is drawn on the held face");
    assert_eq!(lit(&wb, LEFT), None, "faces are not coloured in: the part stays to be seen");
    // Its row, clicked: its face is picked out. Clicked again, it is not.
    let row = browser_row(&d, "Fixed1").center();
    d.click(&mut wb, row);
    d.frame(&mut wb, vec![]);
    assert_eq!(lit(&wb, LEFT), Some(crate::viewport::SELECTED));
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, row);
    assert_eq!(lit(&wb, LEFT), None);

    // A force on the other end: 250 N typed, Enter is OK. Square to the face and into it.
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    click_text(&mut d, &mut wb, "Force");
    assert!(wb.status().starts_with("Force: click the faces it is on"), "{}", wb.status());
    let right = face(&wb, RIGHT);
    d.click(&mut wb, right);
    type_into(&mut d, &mut wb, "tn_props_load_mag", "250");
    // (As it will be, while it is set up.)
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t == "250 N"), "the load is drawn on its face: {:?}", d.texts());
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let load = &study(&wb).loads[0];
    assert_eq!((load.name.as_str(), load.faces.len(), load.kind), ("Force1", 1, LoadKind::Force { newtons: 250.0, direction: None }));
    assert!(study(&wb).missing().is_empty(), "held and loaded: nothing is missing");
    browser_row(&d, "Loads(1)");
    assert!(marked(&d, &wb, "load", RIGHT) && marked(&d, &wb, "held", LEFT), "both are drawn on the part");
    assert!(d.texts().iter().any(|t| t == "250 N"), "{:?}", d.texts());

    // Its row, double-clicked: the panel opens on it. Along Z, the other way, 300 N.
    let row = browser_row(&d, "Force1 (250 N)").center();
    double_click(&mut d, &mut wb, row);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Load(p)) = &wb.panel else { panic!("no load panel: {}", wb.status()) };
    assert_eq!((p.editing, p.magnitude, p.faces.len()), (Some(load_id(&wb, "Force1")), 250.0, 1));
    d.frame(&mut wb, vec![]);
    click_text(&mut d, &mut wb, "Normal to Face");
    click_text(&mut d, &mut wb, "Along Z");
    d.frame(&mut wb, vec![]);
    let flipped = pressable(&d, "tn_glyph Flipped");
    d.click(&mut wb, flipped);
    type_into(&mut d, &mut wb, "tn_props_load_mag", "300");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(study(&wb).loads.len(), 1, "changed, not added");
    assert_eq!(study(&wb).loads[0].kind, LoadKind::Force { newtons: 300.0, direction: Some(Vec3::new(0.0, 0.0, -1.0)) });

    // A pressure on the top.
    click_text(&mut d, &mut wb, "Pressure");
    let top = face(&wb, TOP);
    d.click(&mut wb, top);
    type_into(&mut d, &mut wb, "tn_props_load_mag", "0.5");
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(study(&wb).loads[1].kind, LoadKind::Pressure { megapascals: 0.5 });
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Pressure1 (0.5 MPa)");
    browser_row(&d, "Loads(2)");

    // Its row clicked: its face is picked out as selected. Deleted from its menu; undone.
    let row = browser_row(&d, "Pressure1 (0.5 MPa)").center();
    d.click(&mut wb, row);
    d.frame(&mut wb, vec![]);
    assert_eq!(lit(&wb, TOP), Some(crate::viewport::SELECTED));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(row)]);
    d.frame(&mut wb, vec![Driver::button(row, egui::PointerButton::Secondary, true)]);
    d.frame(&mut wb, vec![Driver::button(row, egui::PointerButton::Secondary, false)]);
    d.frame(&mut wb, vec![]);
    click_text(&mut d, &mut wb, "Delete");
    assert_eq!(study(&wb).loads.len(), 1, "{}", wb.status());
    ctrl(&mut d, &mut wb, egui::Key::Z);
    assert_eq!(study(&wb).loads.len(), 2, "one step, taken back");

    // The part is not changed from here: Extrude's key says so.
    d.tap(&mut wb, egui::Key::E);
    assert!(wb.panel.is_none() && wb.status().contains("Finish Analysis first"), "{}", wb.status());
    // What cannot be worked out yet says when it comes.
    let simulate = wb.run_ui("analysis.simulate").unwrap_err();
    assert!(simulate.contains("milestone M11"), "{simulate}");

    // Finish Analysis: the part's ribbon and browser again; the study stays with the part.
    click_text(&mut d, &mut wb, "Finish");
    assert!(!wb.in_analysis());
    assert_eq!(tabs(&wb)[0], "3D Model");
    assert!(!tabs(&wb).contains(&"Analysis"));
    d.frame(&mut wb, vec![]);
    assert!(!marked(&d, &wb, "held", LEFT) && !d.texts().iter().any(|t| t == "300 N"), "supports and loads are shown in the environment only");
    browser_row(&d, "Extrusion1");
    assert_eq!((study(&wb).supports.len(), study(&wb).loads.len()), (1, 2));

    // Entered again, it is the study that was there.
    wb.run_ui("analysis.begin").unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Study1");
    browser_row(&d, "Fixed1");
    assert!(wb.status().contains("say where the part is held"), "{}", wb.status());
}

fn load_id(wb: &Workbench, name: &str) -> u32 {
    study(wb).loads.iter().find(|l| l.name == name).map(|l| l.id).unwrap_or_else(|| panic!("no load {name}"))
}

#[test]
fn the_environment_is_of_a_part_with_a_solid_and_is_left_with_it() {
    // Nothing to study.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    d.frame(&mut wb, vec![]);
    let refused = wb.run_ui("analysis.begin").unwrap_err();
    assert!(refused.contains("no solid to study"), "{refused}");
    assert!(!wb.in_analysis());

    // A study made from the ribbon, and how large its elements are.
    let (mut wb, mut d) = bar();
    wb.run_ui("analysis.begin").unwrap();
    d.frame(&mut wb, vec![]);
    click_text(&mut d, &mut wb, "Create");
    click_text(&mut d, &mut wb, "Create");
    let names: Vec<&str> = wb.document().studies().iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Study1", "Study2"]);
    assert_eq!(wb.active_study().map(|s| s.name.as_str()), Some("Study2"), "the new study is the one worked on");
    // The other, double-clicked in the browser, is.
    d.frame(&mut wb, vec![]);
    let row = browser_row(&d, "Study1").center();
    double_click(&mut d, &mut wb, row);
    assert_eq!(wb.active_study().map(|s| s.name.as_str()), Some("Study1"));
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Mesh: sized from the part");
    click_text(&mut d, &mut wb, "Settings");
    assert!(matches!(wb.panel, Some(Panel::Value(_))), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    // (What is typed takes the place of what was there; Enter is OK.)
    d.frame(&mut wb, vec![egui::Event::Text("4".into())]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(wb.active_study().and_then(|s| s.mesh_size), Some(4.0));
    // Opened again it shows the 4; 2.5 typed takes its place.
    click_text(&mut d, &mut wb, "Settings");
    assert!(matches!(&wb.panel, Some(Panel::Value(v)) if v.value == 4.0), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![egui::Event::Text("2.5".into())]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.active_study().and_then(|s| s.mesh_size), Some(2.5), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Mesh: 2.5 mm");
    // The part has no material that says how strong it is: the browser says so.
    browser_row(&d, "Material: none (Assign)");
    wb.exec("document.material", json!({ "name": "Oak" })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Material: Oak (no strength known)");
    wb.exec("document.material", json!({ "name": "Brass" })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Material: Brass");

    // A new part: the environment was the other's.
    wb.run_unprompted("file.new").unwrap();
    d.frame(&mut wb, vec![]);
    assert!(!wb.in_analysis());
    assert_eq!(tabs(&wb)[0], "3D Model");

    // An assembly has none.
    wb.run_unprompted("file.new_assembly").unwrap();
    d.frame(&mut wb, vec![]);
    let refused = wb.run_ui("analysis.begin").unwrap_err();
    assert!(refused.contains("of a part"), "{refused}");
}

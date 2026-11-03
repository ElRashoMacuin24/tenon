//! The Stress Analysis environment (Environments > Stress Analysis), by real input: the ribbon's
//! buttons, clicks on the part's faces, values typed into the panel, the study's rows in the
//! browser.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use egui::vec2;
use serde_json::json;
use tenon_geom::Vec3;
use tenon_kernel_occt::OcctKernel;
use tenon_model::study::{LoadKind, Study, SupportKind};

use crate::Workbench;
use crate::analysis::{LoadPanel, Way};
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
    assert_eq!(wb.status(), "Fixed: click the faces, or the edges, that are held still, then OK.");
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
    let list = pressable(&d, "tn_props_load_way");
    d.click(&mut wb, list);
    d.frame(&mut wb, vec![]);
    click_text(&mut d, &mut wb, "Z Axis");
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
    // Simulate, with a part of no material: it says what the study still needs.
    wb.run_ui("analysis.simulate").unwrap();
    assert!(wb.status_error && wb.status().contains("cannot be worked out yet. It needs a material that says how strong it is"), "{}", wb.status());

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

/// The part's strength as the document has it: Young's modulus (GPa), Poisson's ratio, yield
/// strength (MPa).
fn strength(wb: &Workbench) -> Option<(f64, f64, f64)> {
    wb.document().strength().map(|s| (s.youngs_modulus, s.poisson, s.yield_strength))
}

/// Types a value into a cell of the Properties window; Enter.
fn cell(d: &mut Driver, wb: &mut Workbench, id: &str, text: &str) {
    type_into(d, wb, id, text);
    d.tap(wb, egui::Key::Enter);
    d.frame(wb, vec![]);
    d.frame(wb, vec![]);
}

#[test]
fn assign_opens_properties_where_a_material_says_how_strong_it_is() {
    let (mut wb, mut d) = bar();
    wb.run_ui("analysis.begin").unwrap();
    d.frame(&mut wb, vec![]);
    click_text(&mut d, &mut wb, "Assign");
    d.frame(&mut wb, vec![]);
    assert!(wb.chrome.mass, "{}", wb.status());
    let shows = |d: &Driver, text: &str| d.texts().iter().any(|t| t == text);
    assert!(shows(&d, "a material says how strong the part is"), "{:?}", d.texts());

    // A material from the list brings what the library says of it.
    click_text(&mut d, &mut wb, "Generic");
    click_text(&mut d, &mut wb, "Brass");
    d.frame(&mut wb, vec![]);
    assert_eq!(strength(&wb), Some((110.0, 0.34, 125.0)), "{}", wb.status());
    assert!(shows(&d, "Young's modulus") && shows(&d, "110") && shows(&d, "0.34") && shows(&d, "125"), "{:?}", d.texts());
    // One of them of one's own: the others stay. A density of one's own keeps all three.
    cell(&mut d, &mut wb, "tn_partprops_yield", "200");
    assert_eq!(strength(&wb), Some((110.0, 0.34, 200.0)), "{}", wb.status());
    cell(&mut d, &mut wb, "tn_partprops_density", "8.4");
    assert_eq!((wb.document().density(), strength(&wb)), (8.4, Some((110.0, 0.34, 200.0))), "{}", wb.status());
    assert_eq!(wb.document().material().map(|m| (m.name.as_str(), m.color.as_str())), Some(("Brass", "#c9a64a")));
    // What cannot be is refused in words, and nothing changes.
    cell(&mut d, &mut wb, "tn_partprops_poisson", "0.7");
    assert!(wb.status_error && wb.status().contains("Poisson"), "{}", wb.status());
    assert_eq!(strength(&wb), Some((110.0, 0.34, 200.0)));
    cell(&mut d, &mut wb, "tn_partprops_youngs", "stiff");
    assert!(wb.status_error && wb.status().contains("is not a number"), "{}", wb.status());

    // A material of one's own that says nothing of its strength: the study's browser says so.
    wb.exec("document.material", json!({ "name": "Carbon PETG", "density": 1.3, "color": "#202428" })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert_eq!(strength(&wb), None);
    assert!(shows(&d, "not given: a stress check needs all three"), "{:?}", d.texts());
    wb.run_ui("study.new").unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Material: Carbon PETG (no strength known)");
    // Its three values typed: they are the material's once all three are there.
    cell(&mut d, &mut wb, "tn_partprops_youngs", "6");
    assert_eq!(strength(&wb), None, "one of three is not yet a strength");
    cell(&mut d, &mut wb, "tn_partprops_poisson", "0.35");
    assert_eq!(strength(&wb), None);
    cell(&mut d, &mut wb, "tn_partprops_yield", "50");
    assert_eq!(strength(&wb), Some((6.0, 0.35, 50.0)), "{}", wb.status());
    let m = wb.document().material().unwrap();
    assert_eq!((m.name.as_str(), m.density, m.color.as_str()), ("Carbon PETG", 1.3, "#202428"), "it is otherwise as it was");
    browser_row(&d, "Material: Carbon PETG");
}

/// A bar 100 x 20 x 10 with a hole of 8 through its middle, in the environment.
fn bar_with_a_hole() -> (Workbench, Driver) {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    wb.exec("model.box", json!({ "plane": "xy", "width": 100, "depth": 20, "height": 10 })).unwrap();
    wb.exec("model.cylinder", json!({ "plane": "xy", "x": 50, "y": 10, "diameter": 8, "height": 10, "operation": "cut" })).unwrap();
    let mut d = Driver::new(vec2(1400.0, 860.0));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.scene().bodies.len(), 1, "{}", wb.status());
    wb.run_ui("analysis.begin").unwrap();
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    (wb, d)
}

fn right_click(d: &mut Driver, wb: &mut Workbench, at: egui::Pos2) {
    d.frame(wb, vec![egui::Event::PointerMoved(at)]);
    d.frame(wb, vec![Driver::button(at, egui::PointerButton::Secondary, true)]);
    d.frame(wb, vec![Driver::button(at, egui::PointerButton::Secondary, false)]);
    d.frame(wb, vec![]);
}

fn load_panel(wb: &Workbench) -> &LoadPanel {
    match &wb.panel {
        Some(Panel::Load(p)) => p,
        _ => panic!("no load panel: {}", wb.status()),
    }
}

#[test]
fn a_forces_direction_is_picked_in_the_part_or_given_by_its_parts() {
    // A work axis along Y, made before the study, to push along later.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    wb.exec("model.box", json!({ "plane": "xy", "x": 10, "width": 100, "depth": 20, "height": 10 })).unwrap();
    let mut d = Driver::new(vec2(1400.0, 860.0));
    d.frame(&mut wb, vec![]);
    wb.run_ui("work.axis").unwrap();
    wb.browser_action(crate::browser::BrowserAction::Reference(crate::work::Reference::Axis(tenon_model::OriginAxis::Y)));
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none() && wb.scene().work.len() == 1, "{}", wb.status());
    wb.run_ui("analysis.begin").unwrap();
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);

    // Force on the right end. Its faces take the clicks until the direction is asked for.
    click_text(&mut d, &mut wb, "Force");
    let right = on_screen(&wb, Vec3::new(110.0, 10.0, 5.0));
    d.click(&mut wb, right);
    assert_eq!((load_panel(&wb).faces.len(), load_panel(&wb).way, load_panel(&wb).picking), (1, Way::Normal, false));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);

    // The direction selector, then a straight edge: along that edge. The faces stay as they were.
    let selector = pressable(&d, "tn_props_load_pick");
    d.click(&mut wb, selector);
    assert!(load_panel(&wb).picking);
    let long_edge = on_screen(&wb, Vec3::new(60.0, 0.0, 10.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(long_edge)]);
    d.frame(&mut wb, vec![]);
    assert!(matches!(wb.view.hover, Some(Pick::Edge { .. })), "the edge under the pointer is shown as what would be taken: {:?}", wb.view.hover);
    d.click(&mut wb, long_edge);
    assert_eq!((load_panel(&wb).way, load_panel(&wb).picking, load_panel(&wb).faces.len()), (Way::X, false, 1), "{}", wb.status());
    // A flat face: square to it, into it.
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, selector);
    let top = on_screen(&wb, Vec3::new(40.0, 10.0, 10.0));
    d.click(&mut wb, top);
    assert_eq!((load_panel(&wb).way, load_panel(&wb).flip, load_panel(&wb).faces.len()), (Way::Z, true, 1), "down, into the top");
    // A work axis (drawn over the part): along it.
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, selector);
    let on_axis = on_screen(&wb, Vec3::new(0.0, 12.0, 0.0));
    d.click(&mut wb, on_axis);
    assert_eq!((load_panel(&wb).way, load_panel(&wb).picking), (Way::Y, false), "{}", wb.status());
    // A round edge gives no direction: the selector goes on waiting.
    // (Chosen from the list instead: an axis of the part.)
    d.frame(&mut wb, vec![]);
    let list = pressable(&d, "tn_props_load_way");
    d.click(&mut wb, list);
    d.frame(&mut wb, vec![]);
    click_text(&mut d, &mut wb, "Z Axis");
    assert_eq!((load_panel(&wb).way, load_panel(&wb).flip), (Way::Z, false));

    // Use Vector Components: the same force by its parts. 100 N along Z is 0, 0, 100.
    click_text(&mut d, &mut wb, "Use Vector Components");
    assert_eq!(load_panel(&wb).components.map(|v| (v.x, v.y, v.z)), Some((0.0, 0.0, 100.0)));
    type_into(&mut d, &mut wb, "tn_props_load_fx", "30");
    type_into(&mut d, &mut wb, "tn_props_load_fz", "-40");
    d.frame(&mut wb, vec![]);
    assert_eq!(load_panel(&wb).shown(), LoadKind::Force { newtons: 50.0, direction: Some(Vec3::new(0.6, 0.0, -0.8)) });
    assert!(d.texts().iter().any(|t| t == "50 N"), "drawn as it will be: {:?}", d.texts());
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let LoadKind::Force { newtons, direction: Some(way) } = study(&wb).loads[0].kind else { panic!("{:?}", study(&wb).loads) };
    assert!((newtons - 50.0).abs() < 1e-9 && (way - Vec3::new(0.6, 0.0, -0.8)).len() < 1e-9, "{newtons} {way:?}");

    // Opened again it is that direction; by its parts it is 30, 0, -40. Unticked, 50 N that way.
    let row = browser_row(&d, "Force1 (50 N)").center();
    double_click(&mut d, &mut wb, row);
    d.frame(&mut wb, vec![]);
    assert!(matches!(load_panel(&wb).way, Way::Other(_)) && load_panel(&wb).magnitude == 50.0, "{:?}", load_panel(&wb));
    assert!(d.texts().iter().any(|t| t == "0.6, 0, -0.8"), "the selector says which way: {:?}", d.texts());
    click_text(&mut d, &mut wb, "Use Vector Components");
    assert_eq!(load_panel(&wb).components.map(|v| (v.x, v.y, v.z)), Some((30.0, 0.0, -40.0)));
    type_into(&mut d, &mut wb, "tn_props_load_fx", "0");
    click_text(&mut d, &mut wb, "Use Vector Components");
    assert_eq!((load_panel(&wb).components, load_panel(&wb).magnitude, load_panel(&wb).way, load_panel(&wb).flip), (None, 40.0, Way::Z, true));
    // No force at all is refused in words, and the panel stays.
    click_text(&mut d, &mut wb, "Use Vector Components");
    type_into(&mut d, &mut wb, "tn_props_load_fz", "0");
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.status_error && wb.status().contains("other than 0"), "{}", wb.status());
    assert!(matches!(wb.panel, Some(Panel::Load(_))));
}

#[test]
fn a_pin_a_sliding_support_and_the_parts_own_weight() {
    let (mut wb, mut d) = bar_with_a_hole();
    // (The wall of the hole that is seen through its mouth.)
    let bore = Vec3::new(50.0 - 2.828, 10.0 + 2.828, 8.0);
    let (in_bore, on_top) = (on_screen(&wb, bore), on_screen(&wb, Vec3::new(20.0, 10.0, 10.0)));
    let middle = Vec3::new(50.0, 10.0, 5.0);

    // Pin: round faces only. A flat face under the pointer is not offered, and a click on it
    // takes nothing.
    click_text(&mut d, &mut wb, "Pin");
    assert_eq!(wb.status(), "Pin: click the round faces that are on the pin, then OK.");
    d.frame(&mut wb, vec![egui::Event::PointerMoved(on_top)]);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.view.hover, None, "a flat face is not a pin's");
    d.click(&mut wb, on_top);
    d.click(&mut wb, in_bore);
    let Some(Panel::Support(p)) = &wb.panel else { panic!("no panel: {}", wb.status()) };
    assert_eq!((p.faces.len(), p.kind), (1, SupportKind::PIN));
    // Free to move along the pin too.
    d.frame(&mut wb, vec![]);
    click_text(&mut d, &mut wb, "Fix Axial Direction");
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.panel.is_none(), "{}", wb.status());
    let pin = &study(&wb).supports[0];
    assert_eq!((pin.name.as_str(), pin.kind, pin.faces.len()), ("Pin1", SupportKind::Pin { radial: true, axial: false, tangential: false }, 1));
    d.frame(&mut wb, vec![]);
    assert!(marked(&d, &wb, "held", middle), "the pin's mark is in the middle of the hole");

    // Frictionless: flat or round faces (the hole's wall is taken, and given back).
    click_text(&mut d, &mut wb, "Frictionless");
    d.click(&mut wb, on_top);
    d.click(&mut wb, in_bore);
    let Some(Panel::Support(p)) = &wb.panel else { panic!("no panel: {}", wb.status()) };
    assert_eq!(p.faces.len(), 2);
    d.click(&mut wb, in_bore);
    click_text(&mut d, &mut wb, "OK");
    let slide = &study(&wb).supports[1];
    assert_eq!((slide.name.as_str(), slide.kind, slide.faces.len()), ("Frictionless1", SupportKind::Frictionless, 1), "{}", wb.status());

    // Gravity: on no faces, down at the Earth's pull, and drawn from the part's centre.
    click_text(&mut d, &mut wb, "Gravity");
    assert!(wb.status().starts_with("Gravity: the part's own weight"), "{}", wb.status());
    assert_eq!(load_panel(&wb).shown(), LoadKind::Gravity { direction: Vec3::new(0.0, 0.0, -1.0), acceleration: 9810.0 });
    d.click(&mut wb, on_top);
    assert!(load_panel(&wb).faces.is_empty(), "it is on all of the part: a click on a face takes nothing");
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t == "g 9.81 m/s²"), "{:?}", d.texts());
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.panel.is_none(), "{}", wb.status());
    let weight = &study(&wb).loads[0];
    assert_eq!(
        (weight.name.as_str(), weight.faces.len(), weight.kind),
        ("Gravity1", 0, LoadKind::Gravity { direction: Vec3::new(0.0, 0.0, -1.0), acceleration: 9810.0 })
    );
    assert!(study(&wb).missing().is_empty());
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Loads(1)");
    // There is one: the button says so, and its row opens it. Along X, on the Moon.
    click_text(&mut d, &mut wb, "Gravity");
    assert!(wb.status_error && wb.status().contains("the part's weight already (Gravity1)"), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    let row = browser_row(&d, "Gravity1 (9.81 m/s²)").center();
    double_click(&mut d, &mut wb, row);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let list = pressable(&d, "tn_props_load_way");
    d.click(&mut wb, list);
    d.frame(&mut wb, vec![]);
    assert!(!d.texts().iter().any(|t| t == "Normal to Face"), "weight has no faces to be square to");
    click_text(&mut d, &mut wb, "X Axis");
    type_into(&mut d, &mut wb, "tn_props_load_mag", "1620");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert_eq!(study(&wb).loads[0].kind, LoadKind::Gravity { direction: Vec3::X, acceleration: 1620.0 }, "{}", wb.status());

    // Suppressed from its menu: kept, written dim, not drawn, and the study lacks a load again.
    let row = browser_row(&d, "Pin1").center();
    right_click(&mut d, &mut wb, row);
    click_text(&mut d, &mut wb, "Suppress");
    assert!(study(&wb).supports[0].suppressed, "{}", wb.status());
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let quiet = browser_row(&d, "Pin1 (suppressed)").center();
    assert!(!marked(&d, &wb, "held", middle), "the pin's mark is gone");
    assert!(d.texts().iter().all(|t| t != "Pin1"), "{:?}", d.texts());
    right_click(&mut d, &mut wb, quiet);
    click_text(&mut d, &mut wb, "Unsuppress");
    assert!(!study(&wb).supports[0].suppressed);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(marked(&d, &wb, "held", middle), "and back");

    // Renamed from its menu: what is typed takes the place of the name; Enter.
    d.frame(&mut wb, vec![]);
    let row = browser_row(&d, "Frictionless1").center();
    right_click(&mut d, &mut wb, row);
    click_text(&mut d, &mut wb, "Rename");
    assert!(matches!(wb.panel, Some(Panel::RenameStudy { item: Some(2), .. })), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![egui::Event::Text("Table".into())]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(study(&wb).supports[1].name, "Table");
    d.frame(&mut wb, vec![]);
    let row = browser_row(&d, "Study1").center();
    right_click(&mut d, &mut wb, row);
    click_text(&mut d, &mut wb, "Rename");
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![egui::Event::Text("Bench".into())]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert_eq!(study(&wb).name, "Bench", "{}", wb.status());

    // Copy Study: the same again, which becomes the one worked on.
    d.frame(&mut wb, vec![]);
    let row = browser_row(&d, "Bench").center();
    right_click(&mut d, &mut wb, row);
    click_text(&mut d, &mut wb, "Copy Study");
    let names: Vec<&str> = wb.document().studies().iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Bench", "Bench copy"], "{}", wb.status());
    assert_eq!(wb.active_study().map(|s| (s.name.as_str(), s.supports.len(), s.loads.len())), Some(("Bench copy", 2, 1)));
    assert!(wb.status().starts_with("Bench copy is a copy of the study"), "{}", wb.status());
    ctrl(&mut d, &mut wb, egui::Key::Z);
    assert_eq!(wb.document().studies().len(), 1, "one step, taken back");

    // Boundary Conditions hides what is drawn on the part, and shows it again.
    d.frame(&mut wb, vec![]);
    assert!(marked(&d, &wb, "held", middle));
    click_text(&mut d, &mut wb, "Boundary");
    d.frame(&mut wb, vec![]);
    assert!(!marked(&d, &wb, "held", middle) && !d.texts().iter().any(|t| t.starts_with("g ")), "{}", wb.status());
    click_text(&mut d, &mut wb, "Boundary");
    d.frame(&mut wb, vec![]);
    assert!(marked(&d, &wb, "held", middle));
}

#[test]
fn a_feature_is_renamed_by_typing_its_name_and_enter() {
    let (mut wb, mut d) = bar();
    let row = browser_row(&d, "Extrusion1").center();
    right_click(&mut d, &mut wb, row);
    click_text(&mut d, &mut wb, "Rename");
    assert!(matches!(wb.panel, Some(Panel::Rename { .. })), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    // (What is typed takes the place of the name that is shown; Enter is OK.)
    d.frame(&mut wb, vec![egui::Event::Text("Bar".into())]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(wb.document().features().last().map(|f| f.name.as_str()), Some("Bar"));
}

#[test]
fn a_support_whose_face_the_part_no_longer_has_says_so_and_is_mended() {
    let (mut wb, mut d) = bar_with_a_hole();
    let find = |wb: &mut Workbench, with: serde_json::Value| wb.exec("model.find", with).unwrap()["found"][0]["reference"].clone();
    let bore = find(&mut wb, json!({ "what": "faces", "kind": "cylinder" }));
    let right = find(&mut wb, json!({ "what": "faces", "facing": "right" }));
    wb.run_ui("study.new").unwrap();
    wb.exec("study.pin", json!({ "faces": [bore] })).unwrap();
    wb.exec("study.force", json!({ "faces": [right, bore], "newtons": 100, "direction": "down" })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Pin1");
    browser_row(&d, "Force1 (100 N)");

    // The hole goes (its feature suppressed): what was on its wall says so, in red.
    let cut = wb.document().features().last().unwrap().id;
    wb.exec("feature.suppress", json!({ "feature": cut.0, "suppressed": true })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let pin = browser_row(&d, "Pin1 (face lost)").center();
    let force = browser_row(&d, "Force1 (100 N) (1 of 2 faces lost)").center();

    // Opened, it is without the faces that are gone, and says which: the force is put right by OK.
    double_click(&mut d, &mut wb, force);
    d.frame(&mut wb, vec![]);
    assert_eq!(load_panel(&wb).faces.len(), 1);
    assert!(wb.status_error && wb.status().starts_with("Force1: 1 of its 2 faces are no longer on the part, and left out"), "{}", wb.status());
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(study(&wb).loads[0].faces.len(), 1);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Force1 (100 N)");
    // The pin has no face left: OK asks for one, and Cancel leaves it as it was.
    double_click(&mut d, &mut wb, pin);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Support(p)) = &wb.panel else { panic!("no panel: {}", wb.status()) };
    assert!(p.faces.is_empty());
    assert!(wb.status().starts_with("Pin1: its face is no longer on the part"), "{}", wb.status());
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.status().contains("click at least one face"), "{}", wb.status());
    click_text(&mut d, &mut wb, "Cancel");
    assert_eq!(study(&wb).supports[0].faces.len(), 1, "kept: the hole may come back");

    // The hole comes back: the pin is on its wall again, by the wall's name.
    wb.exec("feature.suppress", json!({ "feature": cut.0, "suppressed": false })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Pin1");
    assert!(marked(&d, &wb, "held", Vec3::new(50.0, 10.0, 5.0)));
}

/// Whether the edge with its middle at model point `mid` is picked out.
fn lit_edge(wb: &Workbench, mid: Vec3) -> bool {
    let scene = wb.scene();
    wb.highlights()
        .into_iter()
        .any(|(p, _)| matches!(p, Pick::Edge { body, edge } if (scene.bodies[body].edges[edge as usize].1.mid - mid).len() < 1e-6))
}

/// How many faces and edges the open support panel has.
fn support_on(wb: &Workbench) -> (usize, usize) {
    match &wb.panel {
        Some(Panel::Support(p)) => (p.faces.len(), p.edges.len()),
        _ => panic!("no support panel: {}", wb.status()),
    }
}

#[test]
fn a_bar_is_held_along_an_edge_and_pushed_along_another() {
    let (mut wb, mut d) = bar();
    wb.run_ui("analysis.begin").unwrap();
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    // (Seen from the right, the front and above: the upright edge and the upper edge of the far
    // end, the upper edge of the near end, the middle of the front.)
    let (upright, far_top, near_top) = (Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 10.0, 10.0), Vec3::new(100.0, 10.0, 10.0));
    let (on_upright, on_far_top, on_near_top) = (on_screen(&wb, upright), on_screen(&wb, far_top), on_screen(&wb, near_top));
    let on_front = on_screen(&wb, Vec3::new(50.0, 0.0, 5.0));

    // Fixed: an edge is offered as a face is. Once it is on an edge it takes no face: a support
    // is on the one or on the other.
    click_text(&mut d, &mut wb, "Fixed");
    d.frame(&mut wb, vec![egui::Event::PointerMoved(on_upright)]);
    d.frame(&mut wb, vec![]);
    assert!(matches!(wb.view.hover, Some(Pick::Edge { .. })), "the edge under the pointer is offered: {:?}", wb.view.hover);
    d.click(&mut wb, on_upright);
    assert_eq!(support_on(&wb), (0, 1), "{}", wb.status());
    d.frame(&mut wb, vec![egui::Event::PointerMoved(on_front)]);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.view.hover, None, "on edges, a face is not offered");
    d.click(&mut wb, on_front);
    assert_eq!(support_on(&wb), (0, 1));
    // Another edge; clicked again it is given back.
    d.click(&mut wb, on_far_top);
    assert_eq!(support_on(&wb), (0, 2));
    d.click(&mut wb, on_far_top);
    assert_eq!(support_on(&wb), (0, 1));
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t == "1 edge"), "the panel says what it is on: {:?}", d.texts());
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.panel.is_none(), "{}", wb.status());
    let fixed = &study(&wb).supports[0];
    assert_eq!((fixed.name.as_str(), fixed.faces.len(), fixed.edges.len()), ("Fixed1", 0, 1));
    d.frame(&mut wb, vec![]);
    assert!(marked(&d, &wb, "held", upright), "its mark is at the middle of its edge");

    // Force, along the upper edge of the near end. An edge has no face to be square to: it
    // pushes down until it is told which way, and says so.
    click_text(&mut d, &mut wb, "Force");
    assert!(wb.status().starts_with("Force: click the faces it is on (or the edges it is along)"), "{}", wb.status());
    d.click(&mut wb, on_near_top);
    let p = load_panel(&wb);
    assert_eq!((p.faces.len(), p.edges.len(), p.way, p.flip), (0, 1, Way::Z, true), "{}", wb.status());
    assert!(wb.status().contains("along edges it has no face to be square to"), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let list = pressable(&d, "tn_props_load_way");
    d.click(&mut wb, list);
    d.frame(&mut wb, vec![]);
    assert!(!d.texts().iter().any(|t| t == "Normal to Face"), "not offered along edges: {:?}", d.texts());
    click_text(&mut d, &mut wb, "Y Axis");
    type_into(&mut d, &mut wb, "tn_props_load_mag", "60");
    d.frame(&mut wb, vec![]);
    assert!(
        marked(&d, &wb, "load", near_top) && d.texts().iter().any(|t| t == "60 N"),
        "drawn as it will be, at the middle of its edge: {:?}",
        d.texts()
    );
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let force = &study(&wb).loads[0];
    assert_eq!((force.faces.len(), force.edges.len(), force.kind), (0, 1, LoadKind::Force { newtons: 60.0, direction: Some(Vec3::Y) }));
    assert!(study(&wb).missing().is_empty(), "held and loaded: nothing is missing");

    // Its row, clicked: its edge is picked out. Double-clicked: the panel opens on it.
    let row = browser_row(&d, "Force1 (60 N)").center();
    d.click(&mut wb, row);
    d.frame(&mut wb, vec![]);
    assert!(lit_edge(&wb, near_top));
    double_click(&mut d, &mut wb, row);
    d.frame(&mut wb, vec![]);
    let p = load_panel(&wb);
    assert_eq!((p.edges.len(), p.faces.len(), p.way, p.magnitude), (1, 0, Way::Y, 60.0));
    click_text(&mut d, &mut wb, "Cancel");

    // Fixed1, opened and cleared: a face is offered again, and with a face it takes no edge.
    d.frame(&mut wb, vec![]);
    let row = browser_row(&d, "Fixed1").center();
    double_click(&mut d, &mut wb, row);
    d.frame(&mut wb, vec![]);
    assert_eq!(support_on(&wb), (0, 1));
    click_text(&mut d, &mut wb, "✖");
    assert_eq!(support_on(&wb), (0, 0));
    d.click(&mut wb, on_front);
    assert_eq!(support_on(&wb), (1, 0), "{}", wb.status());
    d.frame(&mut wb, vec![egui::Event::PointerMoved(on_far_top)]);
    d.frame(&mut wb, vec![]);
    assert!(!matches!(wb.view.hover, Some(Pick::Edge { .. })), "on faces, an edge is not offered: {:?}", wb.view.hover);
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.panel.is_none(), "{}", wb.status());
    let fixed = &study(&wb).supports[0];
    assert_eq!((fixed.faces.len(), fixed.edges.len()), (1, 0), "the face took the place of the edge");

    // An edge that was selected beforehand is what a new force is along.
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, on_far_top);
    assert!(matches!(wb.view.selection.as_slice(), [Pick::Edge { .. }]), "{:?}", wb.view.selection);
    click_text(&mut d, &mut wb, "Force");
    let p = load_panel(&wb);
    assert_eq!((p.faces.len(), p.edges.len(), p.way, p.flip), (0, 1, Way::Z, true), "{}", wb.status());
    click_text(&mut d, &mut wb, "Cancel");

    // The edge the force is along goes (it is rounded off): its row says so, and opened it is
    // without it. OK asks for something to be on; Cancel leaves it as it was.
    let edges = wb.exec("model.find", json!({ "what": "edges" })).unwrap();
    let there = |e: &&serde_json::Value| [100.0, 10.0, 10.0].iter().enumerate().all(|(i, v)| (e["mid"][i].as_f64().unwrap() - v).abs() < 1e-6);
    let edge = edges["found"].as_array().unwrap().iter().find(there).expect("the edge")["reference"].clone();
    wb.exec("model.fillet", json!({ "edges": [edge], "radius": 2 })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let row = browser_row(&d, "Force1 (60 N) (edge lost)").center();
    double_click(&mut d, &mut wb, row);
    d.frame(&mut wb, vec![]);
    assert!(load_panel(&wb).edges.is_empty());
    assert!(
        wb.status_error && wb.status().starts_with("Force1: its edge is no longer on the part, and left out. Click its edges again"),
        "{}",
        wb.status()
    );
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.status().contains("click at least one face or edge"), "{}", wb.status());
    click_text(&mut d, &mut wb, "Cancel");
    assert_eq!(study(&wb).loads[0].edges.len(), 1, "kept: the edge may come back");
}

#[test]
fn a_moment_a_bearing_load_a_remote_force_and_the_parts_turning() {
    let (mut wb, mut d) = bar_with_a_hole();
    // (The wall of the hole that is seen through its mouth.)
    let wall = Vec3::new(50.0 - 2.828, 10.0 + 2.828, 8.0);
    let (in_bore, on_top, right) = (on_screen(&wb, wall), on_screen(&wb, Vec3::new(20.0, 10.0, 10.0)), on_screen(&wb, RIGHT));
    let middle = Vec3::new(50.0, 10.0, 5.0);

    // Moment: on the right end, about that face's own axis until another is said. 2 N m.
    click_text(&mut d, &mut wb, "Moment");
    assert!(wb.status().starts_with("Moment: click the faces it turns"), "{}", wb.status());
    d.click(&mut wb, right);
    type_into(&mut d, &mut wb, "tn_props_load_mag", "2000");
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t == "Face's Own Axis"), "{:?}", d.texts());
    assert!(marked(&d, &wb, "load", RIGHT) && d.texts().iter().any(|t| t == "2000 N mm"), "drawn round its face: {:?}", d.texts());
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let moment = &study(&wb).loads[0];
    assert_eq!((moment.name.as_str(), moment.faces.len(), moment.kind), ("Moment1", 1, LoadKind::Moment { newton_millimetres: 2000.0, axis: None }));
    // Opened again: its axis is clicked in the part (the top: the way that face looks), and it
    // is turned the other way round.
    let row = browser_row(&d, "Moment1 (2000 N mm)").center();
    double_click(&mut d, &mut wb, row);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let selector = pressable(&d, "tn_props_load_pick");
    d.click(&mut wb, selector);
    assert!(load_panel(&wb).picking);
    d.click(&mut wb, on_top);
    let p = load_panel(&wb);
    assert_eq!((p.way, p.flip, p.picking, p.faces.len()), (Way::Z, false, false, 1), "an axis is the way the face looks: {}", wb.status());
    d.frame(&mut wb, vec![]);
    let flipped = pressable(&d, "tn_glyph Flipped");
    d.click(&mut wb, flipped);
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(study(&wb).loads[0].kind, LoadKind::Moment { newton_millimetres: 2000.0, axis: Some(Vec3::new(0.0, 0.0, -1.0)) });

    // Bearing Load: round faces only. It presses across its bore, and this bore stands up: so
    // along X, and it says so.
    d.frame(&mut wb, vec![]);
    click_text(&mut d, &mut wb, "Bearing");
    assert!(wb.status().starts_with("Bearing Load: click the round faces"), "{}", wb.status());
    d.frame(&mut wb, vec![egui::Event::PointerMoved(on_top)]);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.view.hover, None, "a flat face is not a bearing's");
    d.click(&mut wb, on_top);
    assert!(load_panel(&wb).faces.is_empty());
    d.click(&mut wb, in_bore);
    let p = load_panel(&wb);
    assert_eq!((p.faces.len(), p.way, p.flip), (1, Way::X, false), "{}", wb.status());
    assert!(wb.status().contains("presses across its bore"), "{}", wb.status());
    type_into(&mut d, &mut wb, "tn_props_load_mag", "500");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let bearing = &study(&wb).loads[1];
    assert_eq!((bearing.name.as_str(), bearing.kind), ("Bearing1", LoadKind::Bearing { newtons: 500.0, direction: Vec3::X }));
    assert!(marked(&d, &wb, "load", middle), "its arrow is in the bore");
    browser_row(&d, "Bearing1 (500 N)");

    // Remote Force: carried by the right end, from a point beyond it.
    click_text(&mut d, &mut wb, "Remote Force");
    assert!(wb.status().starts_with("Remote Force: click the faces that carry it"), "{}", wb.status());
    d.click(&mut wb, right);
    // Its point picked in the part: the middle of the top face. (Then typed, below.)
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let pick = pressable(&d, "tn_props_load_point_pick");
    d.click(&mut wb, pick);
    assert!(load_panel(&wb).picking_point && !load_panel(&wb).picking);
    d.click(&mut wb, on_top);
    let p = load_panel(&wb);
    assert_eq!((p.point, p.picking_point, p.faces.len()), (Vec3::new(50.0, 10.0, 10.0), false, 1), "the faces stay as they were: {}", wb.status());
    type_into(&mut d, &mut wb, "tn_props_load_mag", "80");
    type_into(&mut d, &mut wb, "tn_props_load_px", "130");
    type_into(&mut d, &mut wb, "tn_props_load_py", "10");
    type_into(&mut d, &mut wb, "tn_props_load_pz", "5");
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let beyond = Vec3::new(130.0, 10.0, 5.0);
    assert!(marked(&d, &wb, "load", beyond) && marked(&d, &wb, "load", RIGHT), "drawn at its point, with a line to the face that carries it");
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.panel.is_none(), "{}", wb.status());
    let remote = &study(&wb).loads[2];
    assert_eq!(
        (remote.name.as_str(), remote.faces.len(), remote.kind),
        ("RemoteForce1", 1, LoadKind::RemoteForce { newtons: 80.0, direction: Vec3::new(0.0, 0.0, -1.0), point: beyond })
    );

    // Body: the part turning, on no faces. Its axis is clicked in the part: the wall of the hole
    // gives the hole's axis, and a point that axis goes through.
    click_text(&mut d, &mut wb, "Body");
    assert!(wb.status().starts_with("Body: the part turning"), "{}", wb.status());
    assert_eq!(load_panel(&wb).shown(), LoadKind::Body { axis: Vec3::Z, point: Vec3::ZERO, rpm: 1000.0 });
    d.click(&mut wb, right);
    assert!(load_panel(&wb).faces.is_empty(), "it is on all of the part: a click on a face takes nothing");
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let selector = pressable(&d, "tn_props_load_pick");
    d.click(&mut wb, selector);
    d.click(&mut wb, in_bore);
    let p = load_panel(&wb);
    assert!(!p.picking && p.way == Way::Z, "{p:?}");
    assert_eq!((p.point.x, p.point.y), (50.0, 10.0), "the axis goes through the middle of the hole");
    type_into(&mut d, &mut wb, "tn_props_load_mag", "3000");
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t == "3000 rpm"), "drawn round its axis: {:?}", d.texts());
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let LoadKind::Body { axis, point, rpm } = study(&wb).loads[3].kind else { panic!("{:?}", study(&wb).loads) };
    assert!(rpm == 3000.0 && axis.z.abs() == 1.0 && (point.x, point.y) == (50.0, 10.0), "{axis:?} {point:?} {rpm}");
    assert!(study(&wb).loads[3].faces.is_empty());
    browser_row(&d, "Body1 (3000 rpm)");
    browser_row(&d, "Loads(4)");
    // There is one: the button says so.
    click_text(&mut d, &mut wb, "Body");
    assert!(wb.status_error && wb.status().contains("the part's turning already (Body1)"), "{}", wb.status());
}

#[test]
fn a_force_given_backwards_is_shown_the_way_it_pushes() {
    // By a command a size may be negative: 100 N "up" the other way round is 100 N down.
    let (mut wb, mut d) = bar();
    wb.run_ui("analysis.begin").unwrap();
    let right = wb.exec("model.find", json!({ "what": "faces", "facing": "right" })).unwrap()["found"][0]["reference"].clone();
    wb.run_ui("study.new").unwrap();
    wb.exec("study.force", json!({ "faces": [right], "newtons": -100, "direction": "up" })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let row = browser_row(&d, "Force1 (-100 N)").center();
    double_click(&mut d, &mut wb, row);
    d.frame(&mut wb, vec![]);
    let p = load_panel(&wb);
    assert_eq!((p.magnitude, p.way, p.flip), (100.0, Way::Z, true), "its panel shows the way it pushes: down");
    let down = LoadKind::Force { newtons: 100.0, direction: Some(Vec3::new(0.0, 0.0, -1.0)) };
    assert_eq!(p.shown(), down);
    // OK leaves it pushing the way it did.
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(study(&wb).loads[0].kind, down);
}

/// True when the mesher and the solver are there. When they are not: a failure where they are
/// required (the gate sets TENON_FEA_REQUIRED), and otherwise a line saying the test did not run.
fn programs(test: &str) -> bool {
    match tenon_fea::Tools::find() {
        Ok(_) => true,
        Err(why) => {
            let required = std::env::var("TENON_FEA_REQUIRED").is_ok_and(|v| !v.is_empty() && v != "0");
            assert!(!required, "{test} needs the mesher and the solver, and they are required here: {why}");
            eprintln!("NOT RUN: {test}: {why}");
            false
        }
    }
}

/// How many triangles and how many mesh lines the last frame drew of a study's outcome.
fn drawn(d: &Driver) -> Option<(usize, usize)> {
    d.ctx.data(|x| x.get_temp(egui::Id::new("tn_results_drawn")))
}

#[test]
fn a_study_is_worked_out_and_shown_in_colour_on_the_part() {
    if !programs("Simulate in the app") {
        return;
    }
    // A steel bar held at its left end with 100 N down on its right.
    let (mut wb, mut d) = bar();
    let find =
        |wb: &mut Workbench, way: &str| wb.exec("model.find", json!({ "what": "faces", "facing": way })).unwrap()["found"][0]["reference"].clone();
    let (left, right) = (find(&mut wb, "left"), find(&mut wb, "right"));
    wb.exec("document.material", json!({ "name": "Steel, mild" })).unwrap();
    wb.run_ui("analysis.begin").unwrap();
    wb.run_ui("study.new").unwrap();
    wb.exec("study.force", json!({ "faces": [right], "newtons": 100, "direction": "down" })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Results: none yet (Simulate)");
    assert_eq!(drawn(&d), None);

    // Simulate with nothing holding the part: said in the study's own words, and nothing is shown.
    click_text(&mut d, &mut wb, "Simulate");
    assert!(wb.status_error && wb.status().starts_with("Study1 cannot be worked out yet. It needs a support"), "{}", wb.status());
    assert!(wb.outcome().is_none());

    // Held: worked out. The status bar says what it comes to, and that it is an aid.
    wb.exec("study.fix", json!({ "faces": [left] })).unwrap();
    click_text(&mut d, &mut wb, "Simulate");
    let solved = wb.outcome().and_then(|o| o.solved.clone()).unwrap_or_else(|| panic!("not worked out: {}", wb.status()));
    let (stress, moved) = (solved.results.max_von_mises.value, solved.results.max_displacement.value);
    assert!((moved - 0.1008).abs() < 0.004, "beam theory gives 0.1 mm, and a little for shear: {moved}");
    assert!(stress > 25.0 && stress < 60.0, "bending gives 30 MPa at the root: {stress}");
    assert!(!wb.status_error && wb.status().starts_with("Study1: at most "), "{}", wb.status());
    assert!(
        wb.status().contains("Steel, mild yields at 250") && wb.status().contains("it holds") && wb.status().contains("not a certified analysis"),
        "{}",
        wb.status()
    );

    // Shown on the part: the mesh's boundary in colour, the scale, where the most is.
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let (triangles, lines) = drawn(&d).expect("the results are drawn");
    assert!(triangles > 200 && lines == 0, "{triangles} triangles, {lines} lines");
    let said = |d: &Driver, what: &str| d.texts().iter().any(|t| t == what);
    let figures = crate::results::figures;
    assert!(said(&d, "Von Mises Stress (MPa)") && said(&d, &format!("Max: {} MPa", figures(stress))), "{:?}", d.texts());
    assert!(said(&d, "An engineering aid,") && said(&d, "not a certified analysis."), "{:?}", d.texts());
    assert!(d.texts().iter().any(|t| t.starts_with("Bending shown ")), "{:?}", d.texts());
    // In the browser: the mesh, and each result as a row.
    assert!(d.texts().iter().any(|t| t.starts_with("Mesh: ") && t.ends_with(" elements")), "{:?}", d.texts());
    browser_row(&d, &format!("Von Mises Stress: {} MPa", figures(stress)));
    let factor = browser_row(&d, &format!("Safety Factor: {}", figures(250.0 / stress))).center();
    let displacement = browser_row(&d, &format!("Displacement: {} mm", figures(moved))).center();

    // A row clicked shows that result: the scale is its own, and so is where the most is.
    d.click(&mut wb, displacement);
    d.frame(&mut wb, vec![]);
    assert!(said(&d, "Displacement (mm)") && said(&d, &format!("Max: {} mm", figures(moved))), "{:?}", d.texts());
    // Seen from the other side of the bar, the most is behind it, and the mark says so.
    let (peak, centre) = (solved.results.max_displacement.at, Vec3::new(50.0, 10.0, 5.0));
    wb.look_from(centre - peak);
    d.settle(&mut wb);
    d.frame(&mut wb, vec![]);
    assert!(said(&d, &format!("Max: {} mm (behind)", figures(moved))), "{:?}", d.texts());
    wb.look_from(peak - centre);
    d.settle(&mut wb);
    d.frame(&mut wb, vec![]);
    assert!(said(&d, &format!("Max: {} mm", figures(moved))), "{:?}", d.texts());
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    d.click(&mut wb, factor);
    d.frame(&mut wb, vec![]);
    assert!(said(&d, "Safety Factor") && said(&d, &format!("Min: {}", figures(250.0 / stress))) && said(&d, "15 or more"), "{:?}", d.texts());

    // Displacement Display goes round: as far as it moves, not at all, large enough to see.
    for now in ["Bent as far as it moves", "Shown as it is, not bent"] {
        click_text(&mut d, &mut wb, "Displacement");
        d.frame(&mut wb, vec![]);
        assert!(said(&d, now), "{now}: {:?}", d.texts());
    }
    click_text(&mut d, &mut wb, "Displacement");

    // Mesh View: the elements' lines over it. Again, and they are gone.
    click_text(&mut d, &mut wb, "Mesh");
    d.frame(&mut wb, vec![]);
    let (_, lines) = drawn(&d).unwrap();
    assert!(lines > 300, "{lines} lines");
    wb.run_ui("analysis.mesh_view").unwrap();
    d.frame(&mut wb, vec![]);
    assert_eq!(drawn(&d).map(|(_, lines)| lines), Some(0));

    // The load changed: the results are of what was, and say so; nothing is drawn of them.
    wb.exec("study.set", json!({ "item": 1, "newtons": 200 })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.outcome().is_none() && wb.outcome_is_stale());
    browser_row(&d, "Results: out of date (Simulate)");
    assert_eq!(drawn(&d), None);
    // Worked out again: twice the load, twice the movement.
    click_text(&mut d, &mut wb, "Simulate");
    let twice = wb.outcome().and_then(|o| o.solved.clone()).unwrap_or_else(|| panic!("{}", wb.status())).results.max_displacement.value;
    assert!((twice / moved - 2.0).abs() < 1e-3, "{twice} for twice the load, {moved} for once");

    // Too much for it: 30 kN. It says that it yields, in the colour of a fault.
    wb.exec("study.set", json!({ "item": 1, "newtons": 30000 })).unwrap();
    click_text(&mut d, &mut wb, "Simulate");
    assert!(wb.status_error && wb.status().contains("IT YIELDS: the safety factor is 0."), "{}", wb.status());

    // Left and entered again, the results are gone with the environment.
    wb.run_ui("analysis.finish").unwrap();
    d.frame(&mut wb, vec![]);
    assert_eq!(drawn(&d), None);
}

#[test]
fn mesh_view_divides_the_part_and_shows_its_elements() {
    if !programs("Mesh View") {
        return;
    }
    // A study with nothing in it yet: the mesh can be looked at all the same.
    let (mut wb, mut d) = bar();
    wb.run_ui("analysis.begin").unwrap();
    wb.run_ui("study.new").unwrap();
    d.frame(&mut wb, vec![]);
    click_text(&mut d, &mut wb, "Mesh");
    let o = wb.outcome().unwrap_or_else(|| panic!("no mesh: {}", wb.status()));
    assert!(o.solved.is_none() && o.mesh().is_some_and(|m| m.tets.len() > 20));
    assert!(wb.status().starts_with("Study1: divided into ") && wb.status().contains("elements of about 5.12 mm"), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let (triangles, lines) = drawn(&d).expect("the mesh is drawn");
    assert!(triangles > 100 && lines > 100, "{triangles} triangles, {lines} lines");
    assert!(!d.texts().iter().any(|t| t.contains("MPa")), "there is no result to give a scale to: {:?}", d.texts());
    assert!(d.texts().iter().any(|t| t.starts_with("Mesh: ") && t.ends_with(" elements")), "{:?}", d.texts());
    browser_row(&d, "Results: none yet (Simulate)");

    // Smaller elements: the mesh is of the study as it was, so Mesh View divides the part again.
    wb.exec("study.mesh_size", json!({ "size": 2.5 })).unwrap();
    d.frame(&mut wb, vec![]);
    assert!(wb.outcome().is_none());
    let before = triangles;
    wb.run_ui("analysis.mesh_view").unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let (triangles, _) = drawn(&d).expect("the finer mesh is drawn");
    assert!(triangles > before * 2, "{triangles} triangles of 2.5 mm, {before} of 5");
}

/// Frames until the geometry thread has nothing left to do.
fn settle(d: &mut Driver, wb: &mut Workbench) {
    for _ in 0..6000 {
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
}

/// The app as it runs (the kernel on a thread of its own): a bar of steel in the environment,
/// held at its left end and pushed down at its right.
fn bar_on_the_geometry_thread() -> (Workbench, Driver) {
    let mut wb = Workbench::new(|| Box::new(OcctKernel::new()) as Box<dyn tenon_kernel::Kernel>, None, crate::Services::default());
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.exec("model.box", json!({ "plane": "xy", "width": 100, "depth": 20, "height": 10 })).unwrap();
    wb.exec("document.material", json!({ "name": "Steel, mild" })).unwrap();
    settle(&mut d, &mut wb);
    // (The faces by where they are on the part that is shown.)
    let on = |wb: &Workbench, at: Vec3| {
        let b = &wb.scene().bodies[0];
        let face = b.faces.iter().position(|(_, info)| (info.centroid - at).len() < 1e-6).expect("a face there");
        serde_json::to_value(wb.face_ref_of(0, face as u32).expect("a face that can be named")).unwrap()
    };
    let (left, right) = (on(&wb, LEFT), on(&wb, RIGHT));
    wb.run_ui("analysis.begin").unwrap();
    wb.run_ui("study.new").unwrap();
    wb.exec("study.fix", json!({ "faces": [left] })).unwrap();
    wb.exec("study.force", json!({ "faces": [right], "newtons": 100, "direction": "down" })).unwrap();
    settle(&mut d, &mut wb);
    (wb, d)
}

#[test]
fn a_study_is_worked_out_on_the_geometry_thread_and_esc_stops_it() {
    if !programs("Simulate on the geometry thread") {
        return;
    }
    let _quiet = crate::tests::timing_lock();
    let (mut wb, mut d) = bar_on_the_geometry_thread();

    // Simulate: set going, and the window goes on while it is worked out.
    wb.run_ui("analysis.simulate").unwrap();
    assert!(wb.is_busy() && wb.outcome().is_none() && wb.status().starts_with("Simulating"), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Results: being worked out...");
    // A second one meanwhile is refused.
    assert!(wb.run_ui("analysis.simulate").unwrap_err().contains("is being worked out"));
    settle(&mut d, &mut wb);
    let solved = wb.outcome().and_then(|o| o.solved.clone()).unwrap_or_else(|| panic!("not worked out: {}", wb.status()));
    assert!((solved.results.max_displacement.value - 0.1008).abs() < 0.004, "{}", solved.results.max_displacement.value);
    assert!(wb.status().starts_with("Study1: at most "), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    assert!(drawn(&d).is_some_and(|(triangles, _)| triangles > 200));

    // A finer mesh takes longer: Esc stops it, and what was shown before is out of date.
    wb.exec("study.mesh_size", json!({ "size": 0.6 })).unwrap();
    wb.run_ui("analysis.simulate").unwrap();
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::Escape);
    assert_eq!(wb.status(), "Stopping...");
    settle(&mut d, &mut wb);
    assert_eq!(wb.status(), "Study1: stopped.");
    assert!(!wb.status_error && wb.outcome().is_none());
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Results: out of date (Simulate)");
}

/// How many triangles of a shape the last frame drew.
fn shape_drawn(d: &Driver) -> Option<usize> {
    d.ctx.data(|x| x.get_temp(egui::Id::new("tn_shape_drawn")))
}

/// Frames until a shape on the geometry thread has come to round `n`.
fn to_round(d: &mut Driver, wb: &mut Workbench, n: usize) {
    for _ in 0..6000 {
        d.frame(wb, vec![]);
        let run = wb.shaping().unwrap_or_else(|| panic!("the run ended before round {n}: {}", wb.status()));
        let at = run.progress.lock().unwrap().last.map_or(0, |i| i.number);
        if at >= n {
            d.frame(wb, vec![]);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("round {n} never came: {}", wb.status());
}

/// What the last frame's browser says of the study's shape.
fn shape_row(d: &Driver) -> String {
    let rows: Vec<(String, egui::Rect)> = d.ctx.data(|x| x.get_temp(egui::Id::new("tn_browser_rows"))).unwrap_or_default();
    rows.into_iter().map(|(label, _)| label).find(|l| l.starts_with("Shape: ")).expect("a row for the shape")
}

#[test]
fn a_shape_on_the_geometry_thread_says_where_it_is_and_stops_when_asked_or_left() {
    use std::sync::atomic::Ordering::Relaxed;
    if !programs("Optimize Shape on the geometry thread") {
        return;
    }
    let _quiet = crate::tests::timing_lock();
    const BUSY: &str = "A study is being worked out: wait for it, or press Esc to stop it.";
    const STOPPING: &str = "The last run is still stopping: try again in a moment.";
    let (mut wb, mut d) = bar_on_the_geometry_thread();
    // (Large elements, for rounds that are quick; rounds enough that none ends by itself here.)
    wb.exec("study.shape", json!({ "element_size": 5, "iterations": 300 })).unwrap();

    // Set going: the window goes on, and nothing else that needs the geometry thread starts.
    wb.run_ui("analysis.optimize.now").unwrap();
    assert!(wb.is_busy() && wb.status().starts_with("Optimizing the shape"), "{}", wb.status());
    for id in ["analysis.optimize.now", "analysis.simulate", "analysis.mesh_view"] {
        assert_eq!(wb.run_ui(id).unwrap_err(), BUSY, "{id}");
    }
    assert!(wb.analysis.as_ref().unwrap().running.is_none());
    // Where it is, round by round, in the browser and over the part.
    to_round(&mut d, &mut wb, 2);
    let row = shape_row(&d);
    assert!(row.starts_with("Shape: round ") && row.ends_with(" of at most 300..."), "{row}");
    assert!(d.texts().iter().any(|t| t.starts_with("Optimize Shape: round ") && t.contains("Esc stops it")), "{:?}", d.texts());

    // Esc: the shape so far is kept, shown and said.
    let cancel = wb.shaping().unwrap().cancel.clone();
    d.tap(&mut wb, egui::Key::Escape);
    assert!(cancel.load(Relaxed));
    settle(&mut d, &mut wb);
    assert!(wb.status().contains("the shape so far") && wb.status().contains("Export Shape keeps it"), "{}", wb.status());
    let v = wb.shape_view().unwrap_or_else(|| panic!("no shape: {}", wb.status()));
    assert!(matches!(v.shaped.optimized.stop, tenon_fea::topo::Stop::Stopped) && v.visible);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Shape: 50% removed (stopped early)");
    assert!(shape_drawn(&d).is_some_and(|n| n > 50));
    // What there is to know of it is written over the view, where all of it can be read, and is
    // not run off the end of the status bar.
    assert!(d.texts().iter().any(|t| t == "It was stopped: this is the shape so far."), "{:?}", d.texts());
    assert!(!wb.status().contains("It was stopped: this is the shape so far."), "{}", wb.status());

    // Finish Analysis stops one, and nothing is said of it after.
    wb.run_ui("analysis.optimize.now").unwrap();
    to_round(&mut d, &mut wb, 1);
    let cancel = wb.shaping().unwrap().cancel.clone();
    wb.run_ui("analysis.finish").unwrap();
    assert!(cancel.load(Relaxed));
    settle(&mut d, &mut wb);
    assert_eq!(wb.status(), "Ready");
    assert!(!wb.status_error);

    // Left and come back while it stops: no new run yet, and what it came to goes unsaid and
    // unkept.
    wb.run_ui("analysis.begin").unwrap();
    wb.run_ui("analysis.optimize.now").unwrap();
    to_round(&mut d, &mut wb, 1);
    wb.run_ui("analysis.finish").unwrap();
    wb.run_ui("analysis.begin").unwrap();
    let said = wb.status().to_owned();
    for id in ["analysis.optimize.now", "analysis.simulate", "analysis.mesh_view"] {
        assert_eq!(wb.run_ui(id).unwrap_err(), STOPPING, "{id}");
    }
    settle(&mut d, &mut wb);
    assert_eq!(wb.status(), said);
    assert!(wb.analysis.as_ref().unwrap().shape.is_none() && wb.shape_view().is_none());
    d.frame(&mut wb, vec![]);
    assert_eq!(shape_row(&d), "Shape: none yet (Optimize Shape)");

    // The study changed while it went on: the shape is of what was, so it is stopped there and
    // then, and it is said; it is not kept, and is not promised.
    wb.run_ui("analysis.optimize.now").unwrap();
    to_round(&mut d, &mut wb, 1);
    let cancel = wb.shaping().unwrap().cancel.clone();
    wb.exec("study.set", json!({ "item": 2, "newtons": 150 })).unwrap();
    d.frame(&mut wb, vec![]);
    assert!(cancel.load(Relaxed) && wb.status().contains("out of date") && wb.status().contains("being stopped"), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    assert!(!d.texts().iter().any(|t| t.contains("keeps the shape so far")), "{:?}", d.texts());
    assert!(wb.shaping().is_none() || shape_row(&d) == "Shape: stopping...", "{}", shape_row(&d));
    settle(&mut d, &mut wb);
    assert!(wb.status().contains("out of date") && wb.status().contains("not kept") && !wb.status().contains("Export Shape"), "{}", wb.status());
    assert!(wb.analysis.as_ref().unwrap().shape.is_none());
    assert!(wb.run_ui("analysis.export_shape").unwrap_err().contains("Optimize Shape first"));

    // A Simulate left behind is treated alike.
    wb.exec("study.mesh_size", json!({ "size": 0.6 })).unwrap();
    wb.run_ui("analysis.simulate").unwrap();
    let cancel = wb.analysis.as_ref().unwrap().running.as_ref().unwrap().cancel.clone();
    wb.run_ui("analysis.finish").unwrap();
    assert!(cancel.load(Relaxed));
    wb.run_ui("analysis.begin").unwrap();
    let said = wb.status().to_owned();
    assert_eq!(wb.run_ui("analysis.optimize.now").unwrap_err(), STOPPING);
    settle(&mut d, &mut wb);
    assert_eq!(wb.status(), said);
    assert!(wb.analysis.as_ref().unwrap().running.is_none() && wb.outcome().is_none());
    wb.exec("study.mesh_size", json!({ "size": null })).unwrap();

    // File New stops it, and the new part's geometry comes.
    wb.run_ui("analysis.optimize.now").unwrap();
    to_round(&mut d, &mut wb, 1);
    let cancel = wb.shaping().unwrap().cancel.clone();
    wb.run_unprompted("file.new").unwrap();
    assert!(cancel.load(Relaxed) && wb.analysis.is_none());
    wb.exec("model.box", json!({ "plane": "xy", "width": 30, "depth": 30, "height": 30 })).unwrap();
    settle(&mut d, &mut wb);
    assert_eq!(wb.status(), "New part.");
    let volume: f64 = wb.scene().bodies.iter().map(|b| b.volume).sum();
    assert!((volume - 27000.0).abs() < 1.0, "{volume}");

    // Closing stops one too: the window does not wait for it.
    let (mut wb, mut d) = bar_on_the_geometry_thread();
    wb.exec("study.shape", json!({ "element_size": 5, "iterations": 300 })).unwrap();
    wb.run_ui("analysis.optimize.now").unwrap();
    to_round(&mut d, &mut wb, 1);
    let cancel = wb.shaping().unwrap().cancel.clone();
    wb.shutdown();
    assert!(cancel.load(Relaxed));
    let closing = std::time::Instant::now();
    drop(wb);
    assert!(closing.elapsed().as_secs_f64() < 20.0, "{:?} to close", closing.elapsed());
}

/// Goes as far as round 1 of a run on the geometry thread, opens `open`, and says whether one
/// press of Esc stopped the run; the run is stopped either way, and the screen is left as Esc left
/// it, with `open`'s closing to the caller.
fn esc_with(d: &mut Driver, wb: &mut Workbench, open: &mut dyn FnMut(&mut Driver, &mut Workbench)) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    wb.run_ui("analysis.optimize.now").unwrap();
    to_round(d, wb, 1);
    open(d, wb);
    d.frame(wb, vec![]);
    let cancel = wb.shaping().unwrap().cancel.clone();
    d.tap(wb, egui::Key::Escape);
    d.frame(wb, vec![]);
    let stopped = cancel.load(Relaxed);
    cancel.store(true, Relaxed);
    settle(d, wb);
    stopped
}

#[test]
fn esc_meant_for_something_else_does_not_stop_a_run() {
    if !programs("Esc meant for something else") {
        return;
    }
    let _quiet = crate::tests::timing_lock();
    let (mut wb, mut d) = bar_on_the_geometry_thread();
    wb.exec("study.shape", json!({ "element_size": 5, "iterations": 300 })).unwrap();
    // With nothing open, it stops the run.
    assert!(esc_with(&mut d, &mut wb, &mut |_, _| {}));
    // A dialog, which keeps the Esc it does not use.
    for (name, set) in [
        ("About", (|c| c.about = true) as fn(&mut crate::chrome::Chrome)),
        ("Properties", |c| c.mass = true),
        ("Application Options", |c| c.options = true),
    ] {
        assert!(!esc_with(&mut d, &mut wb, &mut |_, wb| set(&mut wb.chrome)), "{name}");
        (wb.chrome.about, wb.chrome.mass, wb.chrome.options) = (false, false, false);
    }
    // The File menu: closed by it.
    let stopped = esc_with(&mut d, &mut wb, &mut |_, wb| wb.chrome.file_menu = Some(egui::pos2(10.0, 30.0)));
    assert!(!stopped && wb.chrome.file_menu.is_none());
    // The search box, with the focus: it goes back to the view.
    let stopped = esc_with(&mut d, &mut wb, &mut |d, wb| {
        d.ctx.memory_mut(|m| m.request_focus(egui::Id::new("tn_search")));
        d.frame(wb, vec![]);
        assert!(d.ctx.egui_wants_keyboard_input(), "the search box has the keys");
    });
    assert!(!stopped);
    // A panel: closed by it, the run going on; the next Esc is the run's.
    let stopped = esc_with(&mut d, &mut wb, &mut |_, wb| wb.run_ui("analysis.keep_faces").unwrap());
    assert!(!stopped && wb.panel.is_none(), "the panel closes");
    let stopped = esc_with(&mut d, &mut wb, &mut |_, wb| wb.run_ui("analysis.symmetry").unwrap());
    assert!(!stopped && wb.panel.is_none(), "the panel closes");
    // A field of a panel, with the focus.
    let stopped = esc_with(&mut d, &mut wb, &mut |d, wb| {
        wb.run_ui("study.mesh_size").unwrap();
        d.frame(wb, vec![]);
    });
    assert!(!stopped);
    wb.panel = None;
    // A menu of the browser.
    let stopped = esc_with(&mut d, &mut wb, &mut |d, wb| {
        d.frame(wb, vec![]);
        let row = browser_row(d, &shape_row(d)).center();
        right_click(d, wb, row);
        assert!(egui::Popup::is_any_open(&d.ctx), "a menu is open");
    });
    assert!(!stopped);
    // ui.cancel, as the radial menu has it, goes the same way.
    wb.run_ui("analysis.optimize.now").unwrap();
    to_round(&mut d, &mut wb, 1);
    wb.run_ui("analysis.keep_faces").unwrap();
    let cancel = wb.shaping().unwrap().cancel.clone();
    wb.run_ui("ui.cancel").unwrap();
    assert!(!cancel.load(std::sync::atomic::Ordering::Relaxed));
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none());
    wb.run_ui("ui.cancel").unwrap();
    assert!(cancel.load(std::sync::atomic::Ordering::Relaxed));
    settle(&mut d, &mut wb);
}

#[test]
fn a_shape_says_whose_it_is_when_another_study_is_being_worked_on() {
    if !programs("a shape when another study is worked on") {
        return;
    }
    let _quiet = crate::tests::timing_lock();
    let (mut wb, mut d) = bar_on_the_geometry_thread();
    wb.exec("study.shape", json!({ "element_size": 8, "iterations": 2, "sharpen": false })).unwrap();
    wb.run_ui("analysis.optimize.now").unwrap();
    settle(&mut d, &mut wb);
    assert!(wb.status().contains("Export Shape keeps it"), "{}", wb.status());
    let id = study(&wb).id;
    let shaped = wb.shape_view().unwrap().shaped.clone();
    // What comes back while another is active is kept for its own, and not promised to Export.
    // (Another study is made here, and the run's result given to it by hand: a study made is an
    // edit of the part's file, which a run going on would stop for.)
    wb.exec("study.copy", json!({ "study": id.0 })).unwrap();
    let revision_now = wb.session.revision();
    let made = wb.document().studies().iter().find(|s| s.id != id).unwrap().id;
    wb.analysis.as_mut().unwrap().study = Some(made);
    wb.step_seq += 1;
    let run = wb.step_seq;
    wb.analysis.as_mut().unwrap().shaping = Some(crate::shape::Shaping {
        run,
        study: id,
        revision: revision_now,
        cancel: tenon_fea::Cancel::default(),
        since: std::time::Instant::now(),
        progress: std::sync::Arc::new(std::sync::Mutex::new(tenon_fea::topo::Report::default())),
    });
    wb.shape_job_done(run, id, revision_now, Box::new(Ok::<_, String>((*shaped).clone())));
    assert!(wb.status().contains("Work on Study1 to see it and export it.") && !wb.status().contains("Export Shape keeps it"), "{}", wb.status());
    assert_eq!(wb.run_ui("analysis.export_shape").unwrap_err(), "There is no shape to export: Optimize Shape first.");
    wb.analysis.as_mut().unwrap().study = Some(id);
    assert!(wb.shape_view().is_some());
    // What it came to is what is said, when that is not what was asked: stopped early, or with
    // pieces left out, the surface can be well off the share asked.
    let mut off = (*shaped).clone();
    off.shown.share = 0.767;
    wb.step_seq += 1;
    let run = wb.step_seq;
    wb.analysis.as_mut().unwrap().shaping = Some(crate::shape::Shaping {
        run,
        study: id,
        revision: revision_now,
        cancel: tenon_fea::Cancel::default(),
        since: std::time::Instant::now(),
        progress: std::sync::Arc::new(std::sync::Mutex::new(tenon_fea::topo::Report::default())),
    });
    wb.shape_job_done(run, id, revision_now, Box::new(Ok::<_, String>(off)));
    assert!(wb.status().contains("(23% removed, 50% asked)") && wb.status().contains("Export Shape keeps it"), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert_eq!(shape_row(&d), "Shape: 23% removed (stopped early)");
    assert!(d.texts().iter().any(|t| t == "Optimized for 50%: it came to 77%, stopped early or with pieces left out."), "{:?}", d.texts());
    assert!(!d.texts().iter().any(|t| t == "The share it was optimized for."), "{:?}", d.texts());
}

#[test]
fn a_share_typed_and_then_a_click_on_export_writes_that_share() {
    if !programs("a share typed and exported") {
        return;
    }
    let (mut wb, mut d) = bar();
    let find =
        |wb: &mut Workbench, way: &str| wb.exec("model.find", json!({ "what": "faces", "facing": way })).unwrap()["found"][0]["reference"].clone();
    let (left, right) = (find(&mut wb, "left"), find(&mut wb, "right"));
    wb.exec("document.material", json!({ "name": "Steel, mild" })).unwrap();
    wb.run_ui("analysis.begin").unwrap();
    wb.run_ui("study.new").unwrap();
    wb.exec("study.fix", json!({ "faces": [left] })).unwrap();
    wb.exec("study.force", json!({ "faces": [right], "newtons": 100, "direction": "down" })).unwrap();
    wb.exec("study.shape", json!({ "element_size": 5, "iterations": 3, "sharpen": false })).unwrap();
    wb.run_ui("analysis.optimize.now").unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.shape_view().is_some(), "{}", wb.status());
    let to = std::env::temp_dir().join(format!("tenon-shape-typed-{}.stl", std::process::id()));
    let chosen = to.clone();
    wb.services.pick_save = Some(Box::new(move |_: &str, _: &str| Some(chosen.clone())));
    // Typed, and straight to the button: no click anywhere between.
    type_into(&mut d, &mut wb, "tn_shape_keep", "80");
    d.frame(&mut wb, vec![]);
    let button = pressable(&d, "ribbon analysis.export_shape");
    d.click(&mut wb, button);
    let data = std::fs::read(&to).unwrap_or_else(|e| panic!("{e}: {}", wb.status()));
    std::fs::remove_file(&to).ok();
    d.frame(&mut wb, vec![]);
    let v = wb.shape_view().unwrap();
    assert!((v.percent - 80.0).abs() < 1e-9 && (v.shown.share - 0.8).abs() < 0.03, "{} {}", v.percent, v.shown.share);
    assert_eq!(tenon_io::stl::binary_triangle_count(&data).map(|n| n as usize), Some(v.shown.skin.triangles.len()));
    // A surface that is not closed: written, and said so; and said in the view before that.
    let env = wb.analysis.as_mut().unwrap();
    let view = env.shape.as_mut().unwrap();
    view.shown.skin.triangles.pop();
    view.closed = false;
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t == tenon_io::shape::NOT_CLOSED), "{:?}", d.texts());
    wb.run_ui("analysis.export_shape").unwrap();
    assert!(wb.status_error && wb.status().contains("its surface is not closed"), "{}", wb.status());
    std::fs::remove_file(&to).ok();
}

#[test]
fn the_panel_says_the_part_is_being_built_until_its_mass_is_known() {
    let mut p = crate::shape::ShapePanel {
        study: tenon_model::study::StudyId(1),
        by_grams: true,
        remove: 0.0,
        grams: 50.0,
        auto_member: true,
        member: 5.0,
        resolution: tenon_model::study::Resolution::Coarse,
        element_size: None,
        iterations: None,
        sharpen: true,
        part_grams: 0.0,
    };
    // The part comes: what goes by its mass follows, and what was asked stays.
    p.part_is(100.0);
    assert_eq!((p.part_grams, p.remove, p.grams), (100.0, 50.0, 50.0));
    p.part_is(200.0);
    assert_eq!((p.part_grams, p.remove, p.grams), (200.0, 75.0, 50.0));
    p.part_is(0.0);
    assert_eq!((p.part_grams, p.remove, p.grams), (0.0, 75.0, 50.0));
    let mut by_share = crate::shape::ShapePanel { by_grams: false, remove: 25.0, grams: 0.0, ..p };
    by_share.part_is(80.0);
    assert_eq!((by_share.remove, by_share.grams), (25.0, 60.0));
    // On a real worker: opened before the part's first scene has come, and right once it has.
    let mut wb = Workbench::new(|| Box::new(OcctKernel::new()) as Box<dyn tenon_kernel::Kernel>, None, crate::Services::default());
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.exec("model.box", json!({ "plane": "xy", "width": 100, "depth": 20, "height": 10 })).unwrap();
    wb.exec("document.material", json!({ "name": "Steel, mild" })).unwrap();
    wb.run_ui("analysis.begin").unwrap();
    wb.run_ui("study.new").unwrap();
    wb.exec("study.shape", json!({ "grams": 50 })).unwrap();
    wb.run_ui("analysis.optimize").unwrap();
    d.frame(&mut wb, vec![]);
    let Some(Panel::Shape(p)) = &wb.panel else { panic!("{}", wb.status()) };
    assert!(p.remove.is_finite() && (-100.0..=100.0).contains(&p.remove), "{}", p.remove);
    if wb.scene().bodies.is_empty() {
        assert!(d.texts().iter().any(|t| t == "The part is being built"), "{:?}", d.texts());
    }
    settle(&mut d, &mut wb);
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t == "Part 157 g, about 50 g kept"), "{:?}", d.texts());
    let Some(Panel::Shape(p)) = &wb.panel else { panic!("{}", wb.status()) };
    assert!((p.remove - 100.0 * (1.0 - 50.0 / 157.0)).abs() < 0.01, "{}", p.remove);
}

#[test]
fn a_long_status_can_be_read_under_the_pointer() {
    let (mut wb, mut d) = bar();
    let hover = egui::Id::new("tn_status_said");
    wb.set_status("Ready");
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(d.ctx.read_response(hover).is_none(), "what fits needs nothing more");
    wb.set_status("A long thing to say. ".repeat(40));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(d.ctx.read_response(hover).is_some(), "what does not fit comes up under the pointer");
}

#[test]
fn finish_analysis_closes_mesh_settings_too() {
    let (mut wb, mut d) = bar();
    wb.run_ui("analysis.begin").unwrap();
    wb.run_ui("study.new").unwrap();
    wb.run_ui("study.mesh_size").unwrap();
    d.frame(&mut wb, vec![]);
    assert!(matches!(wb.panel, Some(Panel::Value(_))), "{}", wb.status());
    wb.run_ui("analysis.finish").unwrap();
    assert!(wb.panel.is_none());
}

#[test]
fn faces_are_kept_and_planes_of_symmetry_chosen_by_clicks() {
    // Keep Faces and Symmetry need no programs: they are kept with the study (DEC-123).
    let (mut wb, mut d) = bar();
    wb.run_ui("analysis.begin").unwrap();
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    d.click(&mut wb, pressable(&d, "ribbon analysis.keep_faces"));
    assert!(matches!(wb.panel, Some(Panel::Keep(_))), "{}", wb.status());
    assert!(wb.status().starts_with("Keep Faces: click the faces"), "{}", wb.status());
    // OK with no face is refused in words; the right end clicked, as deep as 3 mm.
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.status().contains("click at least one face") && matches!(wb.panel, Some(Panel::Keep(_))), "{}", wb.status());
    let right = face(&wb, RIGHT);
    d.click(&mut wb, right);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "tn_keep_deep"));
    type_into(&mut d, &mut wb, "tn_props_keep_depth", "3");
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.panel.is_none(), "{}", wb.status());
    let kept = study(&wb).optimize.clone().unwrap().kept;
    assert_eq!((kept.len(), kept[0].name.as_str(), kept[0].faces.len(), kept[0].depth), (1, "Keep1", 1, Some(3.0)));
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Keep Faces(1)");
    browser_row(&d, "Keep1 (3 mm deep)");
    browser_row(&d, "Shape: none yet (Optimize Shape)");
    assert!(marked(&d, &wb, "kept", RIGHT), "a mark on the kept face");
    // Double-clicked, its panel opens again: one layer now.
    let row = browser_row(&d, "Keep1 (3 mm deep)").center();
    double_click(&mut d, &mut wb, row);
    let Some(Panel::Keep(p)) = &wb.panel else { panic!("no panel: {}", wb.status()) };
    assert_eq!((p.editing, p.faces.len(), p.deep), (Some(kept[0].id), 1, true));
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "tn_keep_deep"));
    click_text(&mut d, &mut wb, "OK");
    assert_eq!(study(&wb).optimize.clone().unwrap().kept[0].depth, None, "{}", wb.status());

    // Symmetry: the part's work planes are listed by name beside the origin planes. (Its work
    // axis is not a plane, and is not.)
    wb.exec("work.plane", json!({ "base": "yz", "distance": 50 })).unwrap();
    let plane = wb.document().features().last().unwrap().clone();
    wb.exec("work.axis", json!({ "a": "xz", "b": "yz" })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "ribbon analysis.symmetry"));
    let Some(Panel::Symmetry(p)) = &wb.panel else { panic!("no panel: {}", wb.status()) };
    assert_eq!((p.work.len(), p.work[0].0, p.lost.len()), (1, plane.id, 0));
    assert_eq!(wb.status(), "Symmetry: tick the origin planes or work planes the part is the same on both sides of, then OK.");
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(d.texts().contains(&plane.name), "{:?}", d.texts());
    // A face of the part is no plane of symmetry: a click on one takes nothing.
    assert!(wb.panel_wants().is_none());
    let top = face(&wb, TOP);
    d.click(&mut wb, top);
    assert!(wb.panel_picks().is_empty());
    // The XZ plane ticked (the bar lies from y = 0 to 20, so it is not symmetric about it: that
    // is said when a shape is worked out, not here), and the work plane.
    d.click(&mut wb, pressable(&d, "tn_symmetry_XZ"));
    d.click(&mut wb, pressable(&d, &format!("tn_symmetry_work_{}", plane.id.0)));
    click_text(&mut d, &mut wb, "OK");
    assert!(wb.panel.is_none(), "{}", wb.status());
    let symmetry = |wb: &Workbench| study(wb).optimize.clone().unwrap().symmetry;
    let xz = tenon_model::PlaneRef::Origin(tenon_model::OriginPlane::XZ);
    assert_eq!(symmetry(&wb), vec![xz.clone(), tenon_model::PlaneRef::Work(plane.id)]);
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Symmetry: 2 planes");
    let red = crate::theme::Tokens::of(wb.theme()).history_marker;
    assert_ne!(d.text_color("Symmetry: 2 planes"), Some(red));

    // The work plane deleted: the row says so, in red, and the panel shows it as lost, to untick.
    wb.exec("feature.delete", json!({ "feature": plane.id.0 })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let lost = "Symmetry: 2 planes (1 of 2 planes lost)";
    assert_eq!(d.text_color(lost), Some(red));
    let row = browser_row(&d, lost).center();
    double_click(&mut d, &mut wb, row);
    let Some(Panel::Symmetry(p)) = &wb.panel else { panic!("no panel: {}", wb.status()) };
    assert_eq!((p.work.len(), p.lost.len(), p.origin), (0, 1, [false, false, true]));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    // (Its name went with it.)
    assert!(d.texts().contains(&format!("Work plane {} (lost)", plane.id.0)), "{:?}", d.texts());
    d.click(&mut wb, pressable(&d, "tn_symmetry_lost_0"));
    click_text(&mut d, &mut wb, "OK");
    assert_eq!(symmetry(&wb), vec![xz.clone()], "{}", wb.status());
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Symmetry: 1 plane");

    // A face held as a plane in a file is lost too; OK with it still ticked is refused in words.
    let bottom = find_face(&mut wb, "bottom");
    let mut as_filed = serde_json::to_value(wb.document()).unwrap();
    as_filed["studies"][0]["optimize"]["symmetry"].as_array_mut().unwrap().push(json!({ "face": bottom }));
    let as_filed: tenon_model::Document = serde_json::from_value(as_filed).unwrap();
    wb.session
        .edit(|doc| {
            *doc = as_filed;
            Ok(())
        })
        .unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let lost = "Symmetry: 2 planes (1 of 2 planes lost)";
    assert_eq!(d.text_color(lost), Some(red));
    wb.run_ui("analysis.symmetry").unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t == "A face of the part (lost)"), "{:?}", d.texts());
    click_text(&mut d, &mut wb, "OK");
    assert!(matches!(wb.panel, Some(Panel::Symmetry(_))));
    assert!(wb.status_error && wb.status() == tenon_model::study::FACE_PLANE_REFUSED, "{}", wb.status());
    d.click(&mut wb, pressable(&d, "tn_symmetry_lost_0"));
    click_text(&mut d, &mut wb, "OK");
    assert_eq!(symmetry(&wb), vec![xz], "{}", wb.status());
    // Export Shape with no shape says so.
    assert!(wb.run_ui("analysis.export_shape").unwrap_err().contains("Optimize Shape first"));
}

/// The reference of the part's face that looks `way`.
fn find_face(wb: &mut Workbench, way: &str) -> serde_json::Value {
    wb.exec("model.find", json!({ "what": "faces", "facing": way })).unwrap()["found"][0]["reference"].clone()
}

#[test]
fn kept_faces_are_picked_out_named_and_said_to_be_lost_as_a_support_is() {
    let (mut wb, mut d) = bar_with_a_hole();
    let bore = wb.exec("model.find", json!({ "what": "faces", "kind": "cylinder" })).unwrap()["found"][0]["reference"].clone();
    let (left, right) = (find_face(&mut wb, "left"), find_face(&mut wb, "right"));
    wb.exec("document.material", json!({ "name": "Steel, mild" })).unwrap();
    wb.run_ui("study.new").unwrap();
    wb.exec("study.fix", json!({ "faces": [left] })).unwrap();
    wb.exec("study.force", json!({ "faces": [right], "newtons": 100, "direction": "down" })).unwrap();
    wb.exec("study.keep", json!({ "faces": [bore] })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);

    // Clicked, its faces are picked out; clicked again, no longer.
    let row = browser_row(&d, "Keep1").center();
    d.click(&mut wb, row);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.analysis_highlights().len(), 1);
    d.frame(&mut wb, vec![]);
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, row);
    d.frame(&mut wb, vec![]);
    assert!(wb.analysis_highlights().is_empty());
    // Opened, its panel says its name.
    double_click(&mut d, &mut wb, row);
    assert!(matches!(wb.panel, Some(Panel::Keep(_))), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t == "> Keep1"), "{:?}", d.texts());
    click_text(&mut d, &mut wb, "Cancel");
    wb.analysis.as_mut().unwrap().selected = None;

    // The hole goes: the row says so, in red.
    let cut = wb.document().features().last().unwrap().id;
    wb.exec("feature.suppress", json!({ "feature": cut.0, "suppressed": true })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let lost = "Keep1 (face lost)";
    assert_eq!(d.text_color(lost), Some(crate::theme::Tokens::of(wb.theme()).history_marker));
    // Opened, it is without the face that is gone, and says so; Cancel leaves it as it was.
    let row = browser_row(&d, lost).center();
    double_click(&mut d, &mut wb, row);
    let Some(Panel::Keep(p)) = &wb.panel else { panic!("no panel: {}", wb.status()) };
    assert!(p.faces.is_empty());
    assert!(wb.status_error && wb.status().starts_with("Keep1: its face is no longer on the part"), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    click_text(&mut d, &mut wb, "Cancel");
    assert_eq!(study(&wb).optimize.clone().unwrap().kept[0].faces.len(), 1, "kept: the hole may come back");
    // A shape asked for all the same is refused in words, before anything starts.
    wb.run_ui("analysis.optimize.now").unwrap();
    assert!(
        wb.status_error && wb.status().starts_with("A shape of Study1 cannot be worked out yet. Keep1: its face is no longer on the part"),
        "{}",
        wb.status()
    );
    assert!(wb.shaping().is_none() && wb.shape_view().is_none());
    // The hole comes back: the row says nothing more.
    wb.exec("feature.suppress", json!({ "feature": cut.0, "suppressed": false })).unwrap();
    wb.analysis.as_mut().unwrap().selected = None;
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Keep1");
}

#[test]
fn optimize_shapes_panel_shows_what_the_study_holds_of_element_size_and_rounds() {
    use tenon_model::study::Resolution;
    let (mut wb, mut d) = bar();
    wb.run_ui("analysis.begin").unwrap();
    wb.run_ui("study.new").unwrap();
    wb.exec("study.shape", json!({ "element_size": 3, "iterations": 4 })).unwrap();
    let panel = |wb: &Workbench| match &wb.panel {
        Some(Panel::Shape(p)) => p.clone(),
        _ => panic!("no panel: {}", wb.status()),
    };
    let stored = |wb: &Workbench| {
        let o = study(wb).optimize.clone().unwrap();
        (o.element_size, o.iterations, o.resolution)
    };

    // Shown, and OK with no clicks keeps them.
    wb.run_ui("analysis.optimize").unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    for said in ["Element size", "3 mm", "Most rounds", "4"] {
        assert!(d.texts().iter().any(|t| t == said), "{said}: {:?}", d.texts());
    }
    let asked = panel(&wb).command();
    assert_eq!((&asked["element_size"], &asked["iterations"]), (&json!(3.0), &json!(4)));
    wb.exec("study.shape", asked).unwrap();
    assert_eq!(stored(&wb), (Some(3.0), Some(4), Resolution::Coarse));

    // A resolution picked takes the place of the size, which goes from the panel.
    d.click(&mut wb, pressable(&d, "tn_shape_fine"));
    let p = panel(&wb);
    assert_eq!((p.element_size, p.iterations, p.resolution), (None, Some(4), Resolution::Fine));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(!d.texts().iter().any(|t| t == "Element size"), "{:?}", d.texts());
    // The rounds cleared with their button: both are now said to be nothing.
    d.click(&mut wb, pressable(&d, "tn_shape_clear_rounds"));
    let asked = panel(&wb).command();
    assert_eq!((&asked["element_size"], &asked["iterations"]), (&json!(null), &json!(null)));
    wb.exec("study.shape", asked).unwrap();
    assert_eq!(stored(&wb), (None, None, Resolution::Fine));
    wb.panel = None;

    // A size cleared with its button leaves the resolution as it was.
    wb.exec("study.shape", json!({ "element_size": 7 })).unwrap();
    wb.run_ui("analysis.optimize").unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "tn_shape_clear_size"));
    let p = panel(&wb);
    assert_eq!((p.element_size, p.resolution), (None, Resolution::Fine));
}

#[test]
fn every_way_out_of_the_environment_stops_what_is_being_worked_out() {
    use std::sync::atomic::Ordering::Relaxed;
    let file = std::env::temp_dir().join(format!("tenon-ways-out-{}.tenon", std::process::id()));
    for way in ["analysis.finish", "analysis.begin", "file.new", "file.new_sheet_metal", "library.new", "file.open", "file.new_assembly", "shutdown"]
    {
        let (mut wb, mut d) = bar();
        wb.save(&file).unwrap();
        wb.run_ui("analysis.begin").unwrap();
        wb.run_ui("study.new").unwrap();
        let (solving, shaping) = (tenon_fea::Cancel::default(), tenon_fea::Cancel::default());
        let (id, since) = (study(&wb).id, std::time::Instant::now());
        let env = wb.analysis.as_mut().unwrap();
        env.running = Some(crate::results::Running { run: 1, study: id, solving: true, cancel: solving.clone(), since });
        env.shaping = Some(crate::shape::Shaping { run: 2, study: id, revision: 0, cancel: shaping.clone(), since, progress: Default::default() });
        match way {
            "file.open" => wb.open(&file).unwrap(),
            "shutdown" => wb.shutdown(),
            "library.new" => {
                wb.chrome.library_pick = ("washer-iso-7089".to_owned(), "M8".to_owned());
                wb.run_unprompted(way).unwrap();
            }
            _ => wb.run_unprompted(way).unwrap(),
        }
        // (An assembly or a drawing leaves the environment on the next frame.)
        d.frame(&mut wb, vec![]);
        assert!(solving.load(Relaxed) && shaping.load(Relaxed), "{way}");
    }
    std::fs::remove_file(&file).ok();
}

#[test]
fn what_comes_back_is_taken_only_by_the_run_that_waits_for_it() {
    type Came = Box<dyn std::any::Any + Send>;
    let (mut wb, _d) = bar();
    wb.run_ui("analysis.begin").unwrap();
    wb.run_ui("study.new").unwrap();
    let (id, since, revision) = (study(&wb).id, std::time::Instant::now(), wb.session.revision());
    let a_shape = || -> Came { Box::new(Err::<tenon_io::shape::Shaped, String>("it failed".into())) };
    let a_study = || -> Came { Box::new(Err::<tenon_io::stress::Solved, String>("it failed".into())) };

    // A shape: another run's comes back to nothing, its own is taken.
    wb.analysis.as_mut().unwrap().shaping =
        Some(crate::shape::Shaping { run: 7, study: id, revision, cancel: Default::default(), since, progress: Default::default() });
    wb.set_status("as it was");
    wb.shape_job_done(6, id, revision, a_shape());
    assert!(wb.shaping().is_some() && wb.status() == "as it was", "{}", wb.status());
    wb.shape_job_done(7, id, revision, a_shape());
    assert!(wb.shaping().is_none() && wb.status() == "it failed", "{}", wb.status());

    // Simulate: the same.
    wb.analysis.as_mut().unwrap().running = Some(crate::results::Running { run: 9, study: id, solving: true, cancel: Default::default(), since });
    wb.set_status("as it was");
    wb.study_job_done(8, id, revision, true, a_study());
    assert!(wb.analysis.as_ref().unwrap().running.is_some() && wb.status() == "as it was", "{}", wb.status());
    wb.study_job_done(9, id, revision, true, a_study());
    assert!(wb.analysis.as_ref().unwrap().running.is_none() && wb.status() == "it failed", "{}", wb.status());

    // With no run waiting, or no environment, nothing is said.
    wb.set_status("as it was");
    wb.shape_job_done(7, id, revision, a_shape());
    wb.study_job_done(9, id, revision, true, a_study());
    wb.analysis = None;
    wb.shape_job_done(7, id, revision, a_shape());
    wb.study_job_done(9, id, revision, true, a_study());
    assert!(wb.status() == "as it was" && !wb.status_error, "{}", wb.status());
}

#[test]
fn a_shape_gives_way_to_what_is_asked_for_and_its_row_brings_it_back() {
    if !programs("A shape and Simulate") {
        return;
    }
    let (mut wb, mut d) = bar();
    let (left, right) = (find_face(&mut wb, "left"), find_face(&mut wb, "right"));
    wb.exec("document.material", json!({ "name": "Steel, mild" })).unwrap();
    wb.run_ui("analysis.begin").unwrap();
    wb.run_ui("study.new").unwrap();
    wb.exec("study.fix", json!({ "faces": [left] })).unwrap();
    wb.exec("study.force", json!({ "faces": [right], "newtons": 100, "direction": "down" })).unwrap();
    wb.exec("study.shape", json!({ "element_size": 5, "iterations": 3, "sharpen": false })).unwrap();
    let frames = |d: &mut Driver, wb: &mut Workbench| {
        d.frame(wb, vec![]);
        d.frame(wb, vec![]);
    };
    wb.run_ui("analysis.optimize.now").unwrap();
    frames(&mut d, &mut wb);
    assert!(wb.shape_shown() && shape_drawn(&d).is_some(), "{}", wb.status());
    let shown = "Shape: 50% removed (stopped early)";
    let hidden = "Shape: 50% removed (stopped early) (hidden)";

    // Simulate: its results are drawn, and the shape waits on its row.
    wb.run_ui("analysis.simulate").unwrap();
    frames(&mut d, &mut wb);
    assert!(!wb.shape_shown() && wb.results_shown(), "{}", wb.status());
    assert!(drawn(&d).is_some() && shape_drawn(&d).is_none());
    let row = browser_row(&d, hidden).center();
    d.click(&mut wb, row);
    frames(&mut d, &mut wb);
    assert!(wb.shape_shown() && !wb.results_shown() && shape_drawn(&d).is_some() && drawn(&d).is_none());

    // A result's row clicked: that result is drawn.
    let rows: Vec<(String, egui::Rect)> = d.ctx.data(|x| x.get_temp(egui::Id::new("tn_browser_rows"))).unwrap_or_default();
    let result = rows.iter().find(|(label, _)| label.starts_with("Displacement: ")).expect("its row").1.center();
    d.click(&mut wb, result);
    frames(&mut d, &mut wb);
    assert_eq!(wb.analysis.as_ref().unwrap().field, crate::results::Field::Displacement);
    assert!(!wb.shape_shown() && wb.results_shown() && drawn(&d).is_some());

    // Mesh View turned on: the lines are drawn. (Turned off, what is drawn stays.)
    let row = browser_row(&d, hidden).center();
    d.click(&mut wb, row);
    frames(&mut d, &mut wb);
    assert!(wb.shape_shown());
    wb.run_ui("analysis.mesh_view").unwrap();
    frames(&mut d, &mut wb);
    assert!(!wb.shape_shown() && drawn(&d).is_some_and(|(_, lines)| lines > 0));

    // Double-clicked, the Shape row opens the panel with the shape as it was: hidden here, and
    // shown below. Cancel leaves it so.
    for (row, visible, after) in [(hidden, false, shown), (shown, true, hidden)] {
        let at = browser_row(&d, row).center();
        double_click(&mut d, &mut wb, at);
        assert!(matches!(wb.panel, Some(Panel::Shape(_))), "{}", wb.status());
        assert_eq!(wb.shape_view().unwrap().visible, visible, "{row}");
        frames(&mut d, &mut wb);
        click_text(&mut d, &mut wb, "Cancel");
        assert_eq!(wb.shape_view().unwrap().visible, visible, "{row}");
        // (And one click on it changes it.)
        frames(&mut d, &mut wb);
        let at = browser_row(&d, row).center();
        d.frame(&mut wb, vec![egui::Event::PointerMoved(at)]);
        for _ in 0..45 {
            d.frame(&mut wb, vec![]);
        }
        d.click(&mut wb, at);
        frames(&mut d, &mut wb);
        browser_row(&d, after);
    }
}

#[test]
fn a_shape_can_be_written_as_stl_or_3mf_from_one_dialog() {
    if !programs("Export Shape") {
        return;
    }
    let (mut wb, mut d) = bar();
    let (left, right) = (find_face(&mut wb, "left"), find_face(&mut wb, "right"));
    wb.exec("document.material", json!({ "name": "Steel, mild" })).unwrap();
    wb.run_ui("analysis.begin").unwrap();
    wb.run_ui("study.new").unwrap();
    wb.exec("study.fix", json!({ "faces": [left] })).unwrap();
    wb.exec("study.force", json!({ "faces": [right], "newtons": 100, "direction": "down" })).unwrap();
    wb.exec("study.shape", json!({ "element_size": 5, "iterations": 3, "sharpen": false })).unwrap();
    wb.run_ui("analysis.optimize.now").unwrap();
    d.frame(&mut wb, vec![]);
    assert!(wb.shape_view().is_some(), "{}", wb.status());

    // The dialog is asked for both kinds, STL first; the file is of the kind chosen in it.
    let name = format!("{} shape.stl", wb.document().name);
    let base = std::env::temp_dir().join(format!("tenon-shape-export-{}", std::process::id()));
    let (stl, mf3) = (base.with_extension("stl"), base.with_extension("3mf"));
    let chosen = std::rc::Rc::new(std::cell::RefCell::new(stl.clone()));
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(String, Vec<String>)>::new()));
    let (from, told) = (chosen.clone(), asked.clone());
    wb.services.pick_save_of = Some(Box::new(move |file: &str, kinds: &[&str]| {
        told.borrow_mut().push((file.to_owned(), kinds.iter().map(|k| (*k).to_owned()).collect()));
        Some(from.borrow().clone())
    }));
    wb.run_ui("analysis.export_shape").unwrap();
    let triangles = wb.shape_view().unwrap().shown.skin.triangles.len();
    let data = std::fs::read(&stl).unwrap_or_else(|e| panic!("{e}: {}", wb.status()));
    assert_eq!(tenon_io::stl::binary_triangle_count(&data).map(|n| n as usize), Some(triangles));
    *chosen.borrow_mut() = mf3.clone();
    wb.run_ui("analysis.export_shape").unwrap();
    let data = std::fs::read(&mf3).unwrap_or_else(|e| panic!("{e}: {}", wb.status()));
    assert_eq!(&data[..2], b"PK", "a 3MF is a zip");
    assert!(wb.status().contains(&format!("({triangles} triangles)")), "{}", wb.status());
    let both = vec!["stl".to_owned(), "3mf".to_owned()];
    assert_eq!(*asked.borrow(), vec![(name.clone(), both.clone()), (name, both)]);
    std::fs::remove_file(&stl).ok();
    std::fs::remove_file(&mf3).ok();
}

#[test]
fn esc_and_finish_analysis_stop_a_shape_being_worked_out() {
    let (mut wb, mut d) = bar();
    wb.run_ui("analysis.begin").unwrap();
    wb.run_ui("study.new").unwrap();
    // A run going on, as the worker would have it.
    let cancel = tenon_fea::Cancel::default();
    let progress = std::sync::Arc::new(std::sync::Mutex::new(tenon_fea::topo::Report {
        phase: tenon_fea::topo::Phase::Iterating,
        last: Some(tenon_fea::topo::Iteration { number: 12, seconds: 1.1, ..Default::default() }),
        most: 100,
    }));
    let revision = wb.session.revision();
    let shaping =
        crate::shape::Shaping { run: 0, study: study(&wb).id, revision, cancel: cancel.clone(), since: std::time::Instant::now(), progress };
    wb.analysis.as_mut().unwrap().shaping = Some(shaping.clone());
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Shape: round 12 of at most 100...");
    assert!(d.texts().iter().any(|t| t.starts_with("Optimize Shape: round 12 of at most 100 (1.1 s each)")), "{:?}", d.texts());
    // A second run is refused while it goes on.
    assert!(wb.optimize_shape().unwrap_err().contains("being worked out"));
    d.tap(&mut wb, egui::Key::Escape);
    assert!(cancel.load(std::sync::atomic::Ordering::Relaxed) && wb.status() == "Stopping...", "{}", wb.status());
    // Finish Analysis stops one too.
    let other = tenon_fea::Cancel::default();
    wb.analysis.as_mut().unwrap().shaping = Some(crate::shape::Shaping { cancel: other.clone(), ..shaping });
    wb.run_ui("analysis.finish").unwrap();
    assert!(other.load(std::sync::atomic::Ordering::Relaxed) && !wb.in_analysis());
}

#[test]
fn a_shape_is_worked_out_shown_on_a_slider_and_exported() {
    if !programs("Optimize Shape in the app") {
        return;
    }
    let (mut wb, mut d) = bar();
    let find =
        |wb: &mut Workbench, way: &str| wb.exec("model.find", json!({ "what": "faces", "facing": way })).unwrap()["found"][0]["reference"].clone();
    let (left, right) = (find(&mut wb, "left"), find(&mut wb, "right"));
    wb.exec("document.material", json!({ "name": "Steel, mild" })).unwrap();
    wb.run_ui("analysis.begin").unwrap();
    wb.run_ui("study.new").unwrap();
    wb.exec("study.fix", json!({ "faces": [left] })).unwrap();
    wb.exec("study.force", json!({ "faces": [right], "newtons": 100, "direction": "down" })).unwrap();
    // (Quick settings by command, for a quick test: the panel keeps what it does not show.)
    wb.exec("study.shape", json!({ "element_size": 3, "iterations": 4, "sharpen": false })).unwrap();
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "ribbon analysis.optimize"));
    assert!(matches!(wb.panel, Some(Panel::Shape(_))), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    // The part is 100 x 20 x 10 of steel: 157 g. Remove 60 %.
    assert!(d.texts().iter().any(|t| t == "Part 157 g, about 78.5 g kept"), "{:?}", d.texts());
    type_into(&mut d, &mut wb, "tn_props_shape_remove", "60");
    d.frame(&mut wb, vec![]);
    click_text(&mut d, &mut wb, "OK");
    // Worked out (here at once: the kernel is on this thread), kept with the study, shown.
    assert!(wb.panel.is_none(), "{}", wb.status());
    let o = study(&wb).optimize.clone().unwrap();
    assert_eq!((o.target, o.element_size, o.iterations, o.sharpen), (tenon_model::study::Target::Remove(60.0), Some(3.0), Some(4), false));
    assert!(wb.status().starts_with("Study1: a shape of ") && wb.status().contains("for a part of 157 g (60% removed)"), "{}", wb.status());
    assert!(wb.shape_shown() && !wb.results_shown(), "the shape is drawn, the stress colours are not");
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Shape: 60% removed (stopped early)");
    let first = shape_drawn(&d).expect("the shape is drawn");
    assert!(first > 50, "{first} triangles");
    // Another share on the slider: another surface, and a note that it is not an optimum.
    type_into(&mut d, &mut wb, "tn_shape_keep", "70");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let v = wb.shape_view().unwrap();
    assert!((v.percent - 70.0).abs() < 1e-9 && (v.shown.share - 0.7).abs() < 0.02, "{} {}", v.percent, v.shown.share);
    assert!(d.texts().iter().any(|t| t.starts_with("Optimized for 40%: other shares show where material ranks")), "{:?}", d.texts());
    assert_ne!(shape_drawn(&d), Some(first));
    // Typed, the surface waits while the field has it; left by a click elsewhere, it follows.
    let at_70 = shape_drawn(&d);
    type_into(&mut d, &mut wb, "tn_shape_keep", "55");
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let v = wb.shape_view().unwrap();
    assert!((v.percent - 55.0).abs() < 1e-9 && (v.shown.share - 0.7).abs() < 0.02, "{} {}", v.percent, v.shown.share);
    let away = on_screen(&wb, TOP);
    d.click(&mut wb, away);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let v = wb.shape_view().unwrap();
    assert!((v.percent - 55.0).abs() < 1e-9 && (v.shown.share - 0.55).abs() < 0.02, "{} {}", v.percent, v.shown.share);
    assert_ne!(shape_drawn(&d), at_70);
    // Export Shape: the shape shown, written where the user chose.
    let to = std::env::temp_dir().join(format!("tenon-shape-ui-{}.stl", std::process::id()));
    let chosen = to.clone();
    wb.services.pick_save = Some(Box::new(move |_: &str, _: &str| Some(chosen.clone())));
    d.click(&mut wb, pressable(&d, "ribbon analysis.export_shape"));
    let data = std::fs::read(&to).unwrap_or_else(|e| panic!("{e}: {}", wb.status()));
    assert_eq!(tenon_io::stl::binary_triangle_count(&data).map(|n| n as usize), Some(wb.shape_view().unwrap().shown.skin.triangles.len()));
    std::fs::remove_file(&to).ok();
    // Its row clicked: hidden, and the part is drawn as it was.
    let row = browser_row(&d, "Shape: 60% removed (stopped early)").center();
    d.click(&mut wb, row);
    d.frame(&mut wb, vec![]);
    assert!(!wb.shape_shown() && shape_drawn(&d).is_none());
    browser_row(&d, "Shape: 60% removed (stopped early) (hidden)");
    // The study changed: the shape is out of date.
    wb.exec("study.set", json!({ "item": 2, "newtons": 150 })).unwrap();
    d.frame(&mut wb, vec![]);
    browser_row(&d, "Shape: out of date (Optimize Shape)");
}

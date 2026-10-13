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

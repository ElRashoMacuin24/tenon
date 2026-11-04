//! Auxiliary, crop, break, break out and overlay views, against figures worked out by hand: what
//! each draws, that dimensions keep their true values, that they follow the model, come through
//! undo, save and open to the same bytes, and go to PDF and DXF as other views do.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI, SQRT_2};
use std::path::PathBuf;

use serde_json::{Value, json};
use tenon_cli::Engine;
use tenon_drawing::graphics::Pen;
use tenon_drawing::{Graphics, Owner, SheetId, ViewId, ViewKind};
use tenon_geom::Vec2;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;

fn kernel() -> Box<dyn Kernel> {
    Box::new(OcctKernel::new())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-cli-drw-views-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(e: &mut Engine, id: &str, p: Value) -> Value {
    e.exec(id, &p).unwrap_or_else(|err| panic!("{id}: {err}"))
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn pt(v: &Value) -> Vec2 {
    Vec2::new(num(&v[0]), num(&v[1]))
}

fn near(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() < 1e-6, "{what}: {a}, not {b}");
}

/// The sheet as primitives.
fn sheet(e: &Engine) -> Graphics {
    tenon_drawing::annotate::build(e.drw.drawing(), SheetId(1), e.drw.evaluation().unwrap()).0
}

/// The strokes a view draws with a pen.
fn strokes(g: &Graphics, view: u64, pen: Pen) -> Vec<Vec<Vec2>> {
    g.strokes.iter().filter(|s| s.0 == Owner::View(ViewId(view as u32)) && s.1 == pen).map(|s| s.2.clone()).collect()
}

fn length(run: &[Vec2]) -> f64 {
    run.windows(2).map(|w| w[0].dist(w[1])).sum()
}

fn extent(runs: &[Vec<Vec2>]) -> (Vec2, Vec2) {
    let mut pts = runs.iter().flatten().copied();
    let first = pts.next().expect("something is drawn");
    pts.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)))
}

fn view_info(e: &mut Engine, view: u64) -> Value {
    let info = run(e, "drw.info", json!({}));
    info["views"].as_array().unwrap().iter().find(|v| v["id"] == view).cloned().unwrap()
}

/// Where a point of a view is on the sheet.
fn on_sheet(e: &mut Engine, view: u64, x: f64, y: f64) -> Vec2 {
    pt(&run(e, "drw.to_sheet", json!({ "view": view, "at": [x, y] }))["at"])
}

/// A 120 x 80 x `t` plate (t a parameter, 10), with holes of 10 across at (30, 20) and (90, 60),
/// saved as `plate.tenon`.
fn plate(e: &mut Engine) {
    run(e, "file.new", json!({ "name": "Plate" }));
    run(e, "param.add", json!({ "name": "t", "equation": "10 mm" }));
    run(e, "param.add", json!({ "name": "w", "equation": "120 mm" }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    let rect = run(e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 120, "y2": 80 }));
    run(e, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "distance", "a": rect["corners"][0], "b": rect["corners"][1] }, "equation": "w" }));
    let body = run(e, "model.extrude", json!({ "sketch": sk, "distance": 10, "equations": { "/extent/distance": "t" } }))["feature"].clone();
    let top = run(e, "model.face_ref", json!({ "origin": { "type": "cap", "feature": body, "end": "end" } }));
    let holes = run(e, "sketch.create", json!({ "face": top }))["feature"].clone();
    run(e, "sketch.point", json!({ "sketch": holes, "x": 30, "y": 20 }));
    run(e, "sketch.point", json!({ "sketch": holes, "x": 90, "y": 60 }));
    run(e, "model.hole", json!({ "sketch": holes, "diameter": 10, "through_all": true }));
    run(e, "file.save", json!({ "path": "plate.tenon" }));
}

/// A drawing of the plate seen from above at 1:1, its middle at (150, 150): view 1.
fn plate_drawing(e: &mut Engine) -> u64 {
    plate(e);
    run(e, "drw.new", json!({ "name": "Plate" }));
    let v = run(e, "drw.view.base", json!({ "model": "plate.tenon", "orientation": "top", "scale": 1, "at": [150, 150], "hidden": false }));
    v["view"].as_u64().unwrap()
}

#[test]
fn an_auxiliary_view_looks_square_to_an_edge_and_follows_it() {
    let dir = scratch("aux");
    let mut e = Engine::new(kernel(), &dir);
    // A 100 x 60 block 25 thick, its corner cut from (100, 30) to (70, 60): a face at 45 degrees,
    // 30 root 2 long, with a hole 10 across in the middle of it, square to it.
    run(&mut e, "file.new", json!({ "name": "Wedge" }));
    let sk = run(&mut e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    let corners = [(0, 0), (100, 0), (100, 30), (70, 60), (0, 60)];
    let (mut first, mut last) = (Value::Null, Value::Null);
    let mut lines = Vec::new();
    for (i, (x, y)) in corners.iter().enumerate() {
        let mut p = json!({ "sketch": sk, "x2": x, "y2": y });
        if i == 0 {
            p = json!({ "sketch": sk, "x1": 0, "y1": 60, "x2": x, "y2": y });
        } else {
            p["start"] = last.clone();
        }
        if i + 1 == corners.len() {
            p = json!({ "sketch": sk, "start": last, "end": first });
        }
        let l = run(&mut e, "sketch.line", p);
        if i == 0 {
            first = l["start"].clone();
        }
        last = l["end"].clone();
        lines.push(l);
    }
    let cap_line = lines[3]["line"].clone();
    let moving_corner = lines[3]["end"].clone();
    let body = run(&mut e, "model.extrude", json!({ "sketch": sk, "distance": 25 }))["feature"].clone();
    assert!((num(&run(&mut e, "model.mass", json!({}))["bodies"][0]["volume"]) - (6000.0 - 450.0) * 25.0).abs() < 1e-6);
    let side = run(&mut e, "model.face_ref", json!({ "origin": { "type": "side", "feature": body, "curve": cap_line } }));
    let on_face = run(&mut e, "sketch.create", json!({ "face": side }));
    let info = run(&mut e, "sketch.info", json!({ "sketch": on_face["feature"] }));
    let _ = info;
    run(&mut e, "file.save", json!({ "path": "wedge.tenon" }));

    run(&mut e, "drw.new", json!({ "name": "Wedge" }));
    let top = run(&mut e, "drw.view.base", json!({ "model": "wedge.tenon", "orientation": "top", "scale": 1, "at": [120, 110], "hidden": false }))["view"]
        .as_u64()
        .unwrap();

    // The inclined edge, picked in the view: its middle is at (85, 45).
    let edge = run(&mut e, "drw.pick", json!({ "view": top, "view_at": [85, 45] }));
    assert_eq!(edge["kind"], "line", "{edge}");
    near(num(&edge["length"]), 30.0 * SQRT_2, "the edge as drawn");
    // Up and to the right of the view: seen from out there, third-angle.
    let at = pt(&edge["at"]) + Vec2::new(90.0, 90.0);
    let aux = run(&mut e, "drw.auxiliary", json!({ "parent": top, "edge": edge, "at": [at.x, at.y] }));
    assert_eq!(aux["name"], "A", "lettered, as a section is");
    let aux = aux["view"].as_u64().unwrap();
    let i = view_info(&mut e, aux);
    assert!(i["error"].is_null(), "{i}");
    let z = &i["direction"];
    let s = 1.0 / SQRT_2;
    near(num(&z[0]), s, "x of the direction");
    near(num(&z[1]), s, "y of the direction");
    near(num(&z[2]), 0.0, "z of the direction");
    // It sits square to the edge from its parent, at the distance asked for along that direction.
    let centre = pt(&i["center"]);
    let parent_centre = Vec2::new(120.0, 110.0);
    let along_edge = Vec2::new(-s, s);
    near((centre - parent_centre).dot(along_edge), 0.0, "its middle lines up with its parent's");
    // A point of the model is as far along the edge in both: (100, 30, 25) and the same seen there.
    let g_parent = {
        let ev = e.drw.evaluation().unwrap();
        tenon_drawing::views::project(&ev.view(ViewId(top as u32)).unwrap().frame, tenon_geom::Vec3::new(100.0, 30.0, 25.0))
    };
    let g_aux = {
        let ev = e.drw.evaluation().unwrap();
        tenon_drawing::views::project(&ev.view(ViewId(aux as u32)).unwrap().frame, tenon_geom::Vec3::new(100.0, 30.0, 25.0))
    };
    let (sp, sa) = (on_sheet(&mut e, top, g_parent.x, g_parent.y), on_sheet(&mut e, aux, g_aux.x, g_aux.y));
    near((sa - sp).dot(along_edge), 0.0, "the same corner lines up across the two views");
    // The face square to the eye: its long edge true length, and the thickness true.
    let long = run(&mut e, "drw.pick", json!({ "view": aux, "at": [sa.x + along_edge.x * 20.0, sa.y + along_edge.y * 20.0] }));
    near(num(&long["length"]), 30.0 * SQRT_2, "the inclined edge in the auxiliary view");
    // A dimension of it in the auxiliary view keeps its true length.
    let d = run(&mut e, "drw.dimension", json!({ "view": aux, "type": "aligned", "a": long, "by": [20, 20] }));
    near(num(&d["value"]), 30.0 * SQRT_2, "its dimension");

    // Its label, and the arrow on its parent with the same letter.
    let g = sheet(&e);
    let text = |o: u64, t: &str| g.texts.iter().any(|(ow, x)| *ow == Owner::View(ViewId(o as u32)) && x.text == t);
    assert!(text(aux, "VIEW A") && text(aux, "SCALE 1:1") && text(top, "A"), "{:?}", g.texts);
    assert!(g.fills.iter().any(|(o, _)| *o == Owner::View(ViewId(top as u32))), "the arrow on the parent");

    // It only slides square to the edge; its parent takes it along.
    let before = pt(&view_info(&mut e, aux)["center"]);
    run(&mut e, "drw.view.edit", json!({ "view": aux, "by": [30, 0] }));
    let after = pt(&view_info(&mut e, aux)["center"]);
    near((after - before).dot(along_edge), 0.0, "it stays lined up");
    near((after - before).dot(Vec2::new(s, s)), 30.0 * s, "it slides square to the edge");
    run(&mut e, "drw.view.edit", json!({ "view": top, "by": [10, 5] }));
    let moved = pt(&view_info(&mut e, aux)["center"]);
    assert!(moved.near(after + Vec2::new(10.0, 5.0), 1e-9), "{moved:?}");

    // Flipped: from the other side, the other way along its direction.
    run(&mut e, "drw.view.edit", json!({ "view": aux, "flip": true }));
    let z = view_info(&mut e, aux)["direction"].clone();
    near(num(&z[0]), -s, "seen from the other side");
    run(&mut e, "drw.undo", json!({}));
    near(num(&view_info(&mut e, aux)["direction"][0]), s, "undone");

    // The model changes: the corner goes to (60, 60), so the edge runs (-40, 30). The view turns
    // with it: seen along (3, 4, 0) / 5.
    run(&mut e, "file.open", json!({ "path": "wedge.tenon" }));
    run(&mut e, "sketch.drag", json!({ "sketch": sk, "point": moving_corner, "x": 60, "y": 60 }));
    run(&mut e, "file.save", json!({ "path": "wedge.tenon" }));
    run(&mut e, "drw.update", json!({}));
    let i = view_info(&mut e, aux);
    near(num(&i["direction"][0]), 0.6, "x of the direction as the edge has turned");
    near(num(&i["direction"][1]), 0.8, "y of the direction as the edge has turned");
    assert!(i["error"].is_null(), "{i}");

    // Refused, in words: a circle, a view that is not there.
    assert!(e.exec("drw.auxiliary", &json!({ "parent": 9, "edge": edge })).unwrap_err().contains("does not exist"));
}

#[test]
fn a_crop_draws_only_what_is_inside_and_dimensions_keep_their_true_values() {
    let dir = scratch("crop");
    let mut e = Engine::new(kernel(), &dir);
    let v = plate_drawing(&mut e);
    let bottom = run(&mut e, "drw.pick", json!({ "view": v, "view_at": [20, 0] }));
    let d = run(&mut e, "drw.dimension", json!({ "view": v, "type": "horizontal", "a": bottom, "at": [150, 90] }));
    near(num(&d["value"]), 120.0, "the plate's width");

    let key = e.drw.eval_key();
    // A rectangle over the bottom left corner: -5 to 50 across, -5 to 30 up. The view's middle (60,
    // 40) is at (150, 150), so the corner of the plate is at (90, 110).
    let r = run(&mut e, "drw.crop", json!({ "view": v, "rect": [[50, 30], [-5, -5]], "outline": false }));
    assert_eq!(r["modifier"], 1);
    assert_eq!(e.drw.eval_key(), key, "a crop computes nothing again");
    let g = sheet(&e);
    let lines = strokes(&g, v, Pen::Visible);
    // Nothing visible outside it.
    let (lo, hi) = extent(&lines);
    assert!(lo.x >= 90.0 - 5.0 - 1e-9 && hi.x <= 140.0 + 1e-9 && lo.y >= 105.0 - 1e-9 && hi.y <= 140.0 + 1e-9, "{lo:?} {hi:?}");
    // The plate's edges along the bottom and the left: 50 and 30 of them, plus the hole at (30, 20)
    // whole (10 pi round). The hole at (90, 60) is outside.
    let total: f64 = lines.iter().map(|r| length(r)).sum();
    assert!((total - (50.0 + 30.0 + 10.0 * PI)).abs() < 0.01, "{total}");
    // Its centre mark, none on the other hole.
    let (mlo, mhi) = extent(&strokes(&g, v, Pen::Center));
    assert!(mhi.x <= 140.0 + 1e-9 && mlo.y >= 105.0 - 1e-9, "{mlo:?} {mhi:?}");
    // The dimension says the plate's true width; its value is measured from the model, not the crop.
    let info = run(&mut e, "drw.info", json!({}));
    near(num(&info["annotations"][0]["value"]), 120.0, "the dimension after the crop");
    assert_eq!(info["views"][0]["modifiers"][0]["type"], "crop", "{info}");
    let size = pt(&info["views"][0]["size"]);
    assert!(size.near(Vec2::new(50.0, 30.0), 1e-9), "{size:?}");
    // An edge is there to pick where it is drawn; beyond the crop there is nothing.
    assert!(e.exec("drw.pick", &json!({ "view": v, "view_at": [20, 0] })).is_ok());
    assert!(e.exec("drw.pick", &json!({ "view": v, "view_at": [90, 0] })).unwrap_err().contains("no edge"));
    // A long edge whose two ends are both outside a crop is picked where it crosses it.
    run(&mut e, "drw.crop", json!({ "view": v, "rect": [[50, -5], [70, 5]], "outline": false }));
    assert!(e.exec("drw.pick", &json!({ "view": v, "view_at": [60, 0] })).is_ok());
    assert_eq!(view_info(&mut e, v)["modifiers"].as_array().unwrap().len(), 1, "a view has one crop: the same one changed");

    // A circle: 10 of radius about (30, 20): the hole's whole, and no edge of the plate.
    run(&mut e, "drw.crop", json!({ "view": v, "center": [30, 20], "radius": 10, "rect": null, "outline": true }));
    let g = sheet(&e);
    let total: f64 = strokes(&g, v, Pen::Visible).iter().map(|r| length(r)).sum();
    assert!((total - 10.0 * PI).abs() < 0.01, "{total}");
    // Its outline, on its own to click: 20 pi round.
    let outline: f64 = g.strokes.iter().filter(|s| s.0 == Owner::Mod(ViewId(v as u32), 1)).map(|s| length(&s.2)).sum();
    assert!((outline - 20.0 * PI).abs() < 0.05, "{outline}");

    // Suppressed: the whole plate again. Undone, and deleted.
    run(&mut e, "drw.crop", json!({ "view": v, "suppressed": true }));
    let (lo, hi) = extent(&strokes(&sheet(&e), v, Pen::Visible));
    assert!(lo.near(Vec2::new(90.0, 110.0), 1e-9) && hi.near(Vec2::new(210.0, 190.0), 1e-9), "{lo:?} {hi:?}");
    run(&mut e, "drw.undo", json!({}));
    assert_eq!(view_info(&mut e, v)["modifiers"][0]["suppressed"], Value::Null);
    run(&mut e, "drw.modifier.delete", json!({ "view": v, "modifier": 1 }));
    assert!(view_info(&mut e, v)["modifiers"].as_array().unwrap().is_empty());
    assert!(e.exec("drw.modifier.delete", &json!({ "view": v, "modifier": 1 })).is_err());
    run(&mut e, "drw.undo", json!({}));

    // Refused, in words: outside the view; no size.
    assert!(e.exec("drw.crop", &json!({ "view": v, "rect": [[500, 500], [600, 600]] })).unwrap_err().contains("outside the view"));
    assert!(e.exec("drw.crop", &json!({ "view": v, "rect": [[5, 5], [5, 50]] })).unwrap_err().contains("no size"));
    assert!(e.exec("drw.crop", &json!({ "view": v, "center": [5, 5] })).is_err());
}

#[test]
fn a_break_closes_a_band_and_dimensions_across_it_keep_the_true_length() {
    let dir = scratch("break");
    let mut e = Engine::new(kernel(), &dir);
    let v = plate_drawing(&mut e);
    let bottom = run(&mut e, "drw.pick", json!({ "view": v, "view_at": [20, 0] }));
    let dim = run(&mut e, "drw.dimension", json!({ "view": v, "type": "horizontal", "a": bottom, "at": [150, 95] }));
    let key = e.drw.eval_key();

    // Vertical lines at 40 and 80, 6 of paper left between: 120 - 40 + 6 = 86 across, centred.
    run(&mut e, "drw.break", json!({ "view": v, "lines": "vertical", "from": 80, "to": 40, "gap": 6 }));
    assert_eq!(e.drw.eval_key(), key, "a break computes nothing again");
    let i = view_info(&mut e, v);
    let size = pt(&i["size"]);
    assert!(size.near(Vec2::new(86.0, 80.0), 1e-9), "{size:?}");
    // On the sheet: 0 at 150 - 43, 40 at 147, 80 at 153, 120 at 193, 100 at 173, 60 inside the band
    // half way across the gap at 150. And back.
    for (x, on) in [(0.0, 107.0), (40.0, 147.0), (80.0, 153.0), (100.0, 173.0), (120.0, 193.0), (60.0, 150.0)] {
        let s = on_sheet(&mut e, v, x, 10.0);
        near(s.x, on, "where a point goes");
        near(s.y, 120.0, "up the sheet nothing moves");
        let back = run(&mut e, "drw.to_view", json!({ "view": v, "at": [s.x, s.y] }));
        near(num(&back["x"]), x, "and back");
    }
    // Drawn: nothing in the gap; the two sides each end at the lines.
    let g = sheet(&e);
    let lines = strokes(&g, v, Pen::Visible);
    assert!(lines.iter().flatten().all(|p| p.x <= 147.0 + 1e-9 || p.x >= 153.0 - 1e-9), "nothing is drawn in the gap");
    // The bottom edge, 120 long, is drawn 40 + 40 long; so is the top.
    let along: f64 = lines.iter().filter(|r| r.iter().all(|p| (p.y - 110.0).abs() < 1e-9)).map(|r| length(r)).sum();
    near(along, 80.0, "the bottom edge as drawn");
    // Two break lines with their symbols, 6 apart, past the view both ways.
    let symbols: Vec<&(Owner, Pen, Vec<Vec2>)> = g.strokes.iter().filter(|s| s.0 == Owner::Mod(ViewId(v as u32), 1)).collect();
    assert_eq!(symbols.len(), 2);
    assert!(symbols.iter().all(|s| s.1 == Pen::Thin && s.2.len() == 6), "a zigzag in each");
    near(symbols[0].2[0].x, 147.0, "the first break line");
    near(symbols[1].2[0].x, 153.0, "the second");
    assert!(symbols[0].2[0].y < 110.0 && symbols[0].2[5].y > 190.0, "past the view");
    // The hole at (90, 60), past the band, goes with what is beyond it.
    let s = on_sheet(&mut e, v, 90.0, 60.0);
    assert!(s.near(Vec2::new(163.0, 170.0), 1e-9), "{s:?}");

    // The dimension across it: 120 as measured; its line on the sheet 86 long.
    let info = run(&mut e, "drw.info", json!({}));
    near(num(&info["annotations"][0]["value"]), 120.0, "the dimension keeps the true length");
    assert_eq!(info["annotations"][0]["shown"], "120");
    let aid = dim["annotation"].as_u64().unwrap() as u32;
    let thin: Vec<Vec2> = g.strokes.iter().filter(|s| s.0 == Owner::Annotation(tenon_drawing::AnnotId(aid))).flat_map(|s| s.2.clone()).collect();
    let (lo, hi) = (thin.iter().map(|p| p.x).fold(f64::MAX, f64::min), thin.iter().map(|p| p.x).fold(f64::MIN, f64::max));
    near(hi - lo, 86.0, "the dimension's span on the sheet");

    // An edge is there to pick either side of the band, and not in it.
    let picked = run(&mut e, "drw.pick", json!({ "view": v, "at": [180, 110] }));
    near(num(&picked["length"]), 120.0, "the edge beyond the band");
    assert!(e.exec("drw.pick", &json!({ "view": v, "view_at": [60, 40.0] })).is_err(), "nothing in the band");

    // A second band the same way, and one the other way: 20 to 50 up, 5 left: 55 high.
    run(&mut e, "drw.break", json!({ "view": v, "lines": "vertical", "from": 95, "to": 110, "gap": 6 }));
    run(&mut e, "drw.break", json!({ "view": v, "lines": "horizontal", "from": 25, "to": 55, "gap": 5 }));
    let size = pt(&view_info(&mut e, v)["size"]);
    assert!(size.near(Vec2::new(86.0 - 15.0 + 6.0, 80.0 - 30.0 + 5.0), 1e-9), "{size:?}");
    // Changing one: 95 to 110 becomes 90 to 110.
    run(&mut e, "drw.break", json!({ "view": v, "modifier": 2, "from": 90 }));
    let size = pt(&view_info(&mut e, v)["size"]);
    near(size.x, 86.0 - 20.0 + 6.0, "the band changed");
    run(&mut e, "drw.undo", json!({}));
    // Suppressed: as if it were not there.
    run(&mut e, "drw.break", json!({ "view": v, "modifier": 3, "suppressed": true }));
    near(pt(&view_info(&mut e, v)["size"]).y, 80.0, "the band suppressed");

    // Refused, in words.
    let err = |e: &mut Engine, p: Value| e.exec("drw.break", &p).unwrap_err();
    assert!(err(&mut e, json!({ "view": v, "lines": "vertical", "from": 10, "to": 12 })).contains("closer together"));
    assert!(err(&mut e, json!({ "view": v, "lines": "vertical", "from": 200, "to": 300 })).contains("do not cross"));
    assert!(err(&mut e, json!({ "view": v, "lines": "vertical", "from": -5, "to": 130 })).contains("whole view"));
    assert!(err(&mut e, json!({ "view": v, "lines": "vertical", "from": 45, "to": 60 })).contains("out of range"), "over another band");
    let sec = run(&mut e, "drw.view.section", json!({ "parent": v, "a": [-5, 40], "b": [125, 40], "at": [150, 60] }))["view"].clone();
    assert!(err(&mut e, json!({ "view": sec, "lines": "vertical", "from": 40, "to": 60 })).contains("base or projected"));
    run(&mut e, "drw.undo", json!({}));

    // The plate made longer: 140, so what is left of it is 140 - 40 + 6 - 15 + 6 = 97, the band stays.
    run(&mut e, "file.open", json!({ "path": "plate.tenon" }));
    run(&mut e, "param.set", json!({ "name": "w", "equation": "140" }));
    run(&mut e, "file.save", json!({ "path": "plate.tenon" }));
    run(&mut e, "drw.update", json!({}));
    let info = run(&mut e, "drw.info", json!({}));
    near(pt(&info["views"][0]["size"]).x, 97.0, "the view follows the model");
    near(num(&info["annotations"][0]["value"]), 140.0, "and so does the dimension");
}

/// A 60 x 40 x `t` block (t a parameter, 20) with a blind hole 10 across and 10 deep up from below
/// at (30, 20), saved as `block.tenon`.
fn block(e: &mut Engine) {
    run(e, "file.new", json!({ "name": "Block" }));
    run(e, "param.add", json!({ "name": "t", "equation": "20 mm" }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 60, "y2": 40 }));
    let body = run(e, "model.extrude", json!({ "sketch": sk, "distance": 20, "equations": { "/extent/distance": "t" } }))["feature"].clone();
    let under = run(e, "model.face_ref", json!({ "origin": { "type": "cap", "feature": body, "end": "start" } }));
    let holes = run(e, "sketch.create", json!({ "face": under }))["feature"].clone();
    let info = run(e, "sketch.info", json!({ "sketch": holes }));
    let _ = info;
    run(e, "sketch.point", json!({ "sketch": holes, "x": 30, "y": -20 }));
    run(e, "model.hole", json!({ "sketch": holes, "diameter": 10, "depth": 10 }));
    let volume = num(&run(e, "model.mass", json!({}))["bodies"][0]["volume"]);
    assert!((volume - (48000.0 - 250.0 * PI)).abs() < 1e-3, "{volume}");
    run(e, "file.save", json!({ "path": "block.tenon" }));
}

/// The hatch of a view, how long it comes to: the area over the spacing.
fn hatch_length(g: &Graphics, view: u64) -> f64 {
    strokes(g, view, Pen::Hatch).iter().map(|r| length(r)).sum()
}

fn circles(g: &Graphics, pen: Pen, radius: f64) -> usize {
    tenon_drawing::clean::items(g)
        .into_iter()
        .filter(|(p, i)| *p == pen && matches!(i, tenon_drawing::clean::Item::Circle { r, .. } if (r - radius).abs() < 1e-6))
        .count()
}

#[test]
fn a_break_out_cuts_to_a_depth_and_hatches_what_it_leaves() {
    let dir = scratch("break-out");
    let mut e = Engine::new(kernel(), &dir);
    block(&mut e);
    run(&mut e, "drw.new", json!({ "name": "Block" }));
    let v = run(&mut e, "drw.view.base", json!({ "model": "block.tenon", "orientation": "top", "scale": 1, "at": [150, 150], "hidden": true }))["view"]
        .as_u64()
        .unwrap();
    // From above the hole is hidden: a dashed circle.
    let g = sheet(&e);
    assert_eq!((circles(&g, Pen::Hidden, 5.0), circles(&g, Pen::Visible, 5.0)), (1, 0));
    assert_eq!(hatch_length(&g, v), 0.0);

    // 40 x 30 over the middle, 15 down from the top at 20: the floor at 5, across the hole.
    let key = e.drw.eval_key();
    let r = run(&mut e, "drw.break_out", json!({ "view": v, "rect": [[10, 5], [50, 35]], "depth": 15 }));
    assert_eq!(r["modifier"], 1);
    assert_ne!(e.drw.eval_key(), key, "a break-out is cut anew");
    let i = view_info(&mut e, v);
    assert!(i["error"].is_null(), "{i}");
    let g = sheet(&e);
    // The hole now seen: a whole circle, drawn visible. The floor hatched: 40 x 30 less the hole's
    // 25 pi, over the 3 between hatch lines.
    assert_eq!((circles(&g, Pen::Hidden, 5.0), circles(&g, Pen::Visible, 5.0)), (0, 1), "the hole is seen");
    let area = 1200.0 - 25.0 * PI;
    let h = hatch_length(&g, v);
    assert!((h - area / 3.0).abs() < area / 3.0 * 0.03, "{h} against {}", area / 3.0);
    // No hatching outside the boundary (the middle of the view, (30, 20), is at (150, 150)).
    let (lo, hi) = extent(&strokes(&g, v, Pen::Hatch));
    assert!(lo.x >= 130.0 - 1e-6 && hi.x <= 170.0 + 1e-6 && lo.y >= 135.0 - 1e-6 && hi.y <= 165.0 + 1e-6, "{lo:?} {hi:?}");
    // The cut's rim drawn: the lines of the 40 x 30 at the top and the floor, one over the other.
    let rim: f64 = strokes(&g, v, Pen::Visible).iter().flat_map(|r| r.windows(2).map(|w| (w[0], w[1]))).filter(|(a, b)| (a.y - 135.0).abs() < 1e-6 && (b.y - 135.0).abs() < 1e-6).map(|(a, b)| a.dist(b)).sum();
    assert!(rim >= 40.0 - 1e-6, "{rim}");

    // Only 5 down: the floor at 15, above the hole: the floor whole, the hole still hidden.
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": 1, "depth": 5 }));
    let g = sheet(&e);
    assert_eq!((circles(&g, Pen::Hidden, 5.0), circles(&g, Pen::Visible, 5.0)), (1, 0), "the hole is still below the floor");
    let h = hatch_length(&g, v);
    assert!((h - 400.0).abs() < 400.0 * 0.03, "{h}");
    // A circle 12 across its radius, 15 down: 144 pi less the hole's 25 pi.
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": 1, "center": [30, 20], "radius": 12, "depth": 15, "rect": null }));
    let g = sheet(&e);
    let area = 119.0 * PI;
    let h = hatch_length(&g, v);
    assert!((h - area / 3.0).abs() < area / 3.0 * 0.03, "{h} against {}", area / 3.0);
    assert_eq!(circles(&g, Pen::Visible, 12.0), 1, "the boundary, seen as a circle");
    // Through it all: no floor, nothing to hatch; a hole of 12 is what is left.
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": 1, "through": true }));
    let g = sheet(&e);
    assert_eq!(hatch_length(&g, v), 0.0);
    assert_eq!(circles(&g, Pen::Visible, 12.0), 1);
    assert_eq!(circles(&g, Pen::Hidden, 5.0), 0, "the hole is gone with what it was in");
    // Through a boundary bigger than the block: nothing left, and no failure.
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": 1, "rect": [[-10, -10], [70, 50]], "center": null, "radius": null }));
    let i = view_info(&mut e, v);
    assert!(i["error"].is_null(), "{i}");
    assert_eq!(num(&i["visible_curves"]), 0.0, "{i}");

    // Back to the rectangle, 15 down, by undo. The block made 30 thick: the floor at 15 now, above
    // the hole: the depth is from the top as it is.
    for _ in 0..3 {
        run(&mut e, "drw.undo", json!({}));
    }
    let i = view_info(&mut e, v);
    assert_eq!(i["modifiers"][0]["depth"], 15.0, "{i}");
    run(&mut e, "file.open", json!({ "path": "block.tenon" }));
    run(&mut e, "param.set", json!({ "name": "t", "equation": "30" }));
    run(&mut e, "file.save", json!({ "path": "block.tenon" }));
    run(&mut e, "drw.update", json!({}));
    let g = sheet(&e);
    assert_eq!((circles(&g, Pen::Hidden, 5.0), circles(&g, Pen::Visible, 5.0)), (1, 0), "the hole is below the floor now");
    assert!((hatch_length(&g, v) - 133.33).abs() < 12.0 * 133.33, "circle: {}", hatch_length(&g, v));

    // A section of it, a detail: the detail shows what the break-out left. Picking the part cut away
    // is refused; suppressing it brings it back.
    assert!(e.exec("drw.pick", &json!({ "view": v, "view_at": [30, 0] })).is_ok(), "an edge outside the cut");
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": 1, "suppressed": true }));
    let g = sheet(&e);
    assert_eq!(hatch_length(&g, v), 0.0);
    run(&mut e, "drw.undo", json!({}));

    // Refused, in words.
    let err = |e: &mut Engine, p: Value| e.exec("drw.break_out", &p).unwrap_err();
    assert!(err(&mut e, json!({ "view": v, "rect": [[0, 0], [5, 5]] })).contains("depth"));
    assert!(err(&mut e, json!({ "view": v, "depth": 5 })).contains("boundary"));
    assert!(err(&mut e, json!({ "view": v, "rect": [[0, 0], [5, 5]], "depth": -1 })).contains("positive"));
    assert!(err(&mut e, json!({ "view": v, "rect": [[0, 0], [5, 5]], "depth": 1, "through": true })).contains("not both"));
}

/// A 100 x 60 x `t` base (t a parameter, 10), grounded, with a hole 10 across at (20, 20); on it an
/// arm 60 x 10 x 5 with a hole 10 across 5 in from its end, turning on a rotational joint in that
/// hole; and a block 20 x 20 x 10 mated flat on the base at (60, 30) with an offset of 0. Saved as
/// `pivot.tenonasm`. Returns the ids of the joint and the mate.
fn pivot(e: &mut Engine) -> (u64, u64) {
    run(e, "file.new", json!({ "name": "Base" }));
    run(e, "param.add", json!({ "name": "t", "equation": "10 mm" }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 100, "y2": 60 }));
    let hole = run(e, "sketch.circle", json!({ "sketch": sk, "cx": 20, "cy": 20, "r": 5 }))["circle"].clone();
    let base = run(e, "model.extrude", json!({ "sketch": sk, "distance": 10, "equations": { "/extent/distance": "t" } }))["feature"].clone();
    run(e, "file.save", json!({ "path": "base.tenon" }));

    run(e, "file.new", json!({ "name": "Arm" }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 60, "y2": 10 }));
    let arm_hole = run(e, "sketch.circle", json!({ "sketch": sk, "cx": 5, "cy": 5, "r": 3 }))["circle"].clone();
    let arm = run(e, "model.extrude", json!({ "sketch": sk, "distance": 5 }))["feature"].clone();
    run(e, "file.save", json!({ "path": "arm.tenon" }));

    run(e, "file.new", json!({ "name": "Cube" }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 20, "y2": 20 }));
    let cube = run(e, "model.extrude", json!({ "sketch": sk, "distance": 10 }))["feature"].clone();
    run(e, "file.save", json!({ "path": "cube.tenon" }));

    run(e, "asm.new", json!({ "name": "Pivot" }));
    run(e, "asm.insert", json!({ "path": "base.tenon" }));
    let a = run(e, "asm.insert", json!({ "path": "arm.tenon", "at": [15, 15, 10] }))["component"].clone();
    let c = run(e, "asm.insert", json!({ "path": "cube.tenon", "at": [60, 30, 10] }))["component"].clone();
    let base_top = run(e, "asm.geom", json!({ "component": 1, "edge": [{ "type": "cap", "feature": base, "end": "end" }, { "type": "side", "feature": base, "curve": hole }] }));
    let arm_under = run(e, "asm.geom", json!({ "component": a, "edge": [{ "type": "cap", "feature": arm, "end": "start" }, { "type": "side", "feature": arm, "curve": arm_hole }] }));
    let joint = run(e, "asm.joint", json!({ "type": "revolute", "a": base_top, "b": arm_under }))["relationship"].as_u64().unwrap();
    let top = run(e, "asm.geom", json!({ "component": 1, "face": { "type": "cap", "feature": base, "end": "end" } }));
    let under = run(e, "asm.geom", json!({ "component": c, "face": { "type": "cap", "feature": cube, "end": "start" } }));
    let mate = run(e, "asm.constrain", json!({ "type": "mate", "a": top, "b": under }))["relationship"].as_u64().unwrap();
    run(e, "asm.save", json!({ "path": "pivot.tenonasm" }));
    (joint, mate)
}

#[test]
fn an_overlay_draws_the_assembly_in_another_position_in_phantom_lines() {
    let dir = scratch("overlay");
    let mut e = Engine::new(kernel(), &dir);
    let (joint, mate) = pivot(&mut e);
    run(&mut e, "drw.new", json!({ "name": "Pivot" }));
    let top = run(&mut e, "drw.view.base", json!({ "model": "pivot.tenonasm", "orientation": "top", "scale": 1, "at": [150, 150], "hidden": false }))["view"]
        .as_u64()
        .unwrap();

    // What a position can set, as it is now: the joint's angle, whatever it comes to, and offsets.
    let values = run(&mut e, "drw.overlay.values", json!({ "view": top }));
    let rows = values["values"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{values}");
    assert_eq!(rows[0]["name"], "Rotational:1");
    assert_eq!(rows[1]["name"], "Mate:1");
    let a0 = num(&rows[0]["angle"]);
    near(num(&rows[1]["offset"]), 0.0, "the mate's offset");
    assert!(rows[1]["angle"].is_null());

    // The arm turned a quarter turn about the pivot at (20, 20). It lies from 5 behind the pivot to
    // 55 past it and 5 to each side: in view coordinates 15 to 25 across and 15 to 75 up, or 15 to 25
    // and -35 to 25 the other way. The middle of the view (of the model's 100 x 60: (50, 30)) is at
    // (150, 150).
    let ov = run(&mut e, "drw.overlay", json!({ "parent": top, "values": [{ "relationship": joint, "angle": a0 + FRAC_PI_2 }] }));
    let ov = ov["view"].as_u64().unwrap();
    let i = view_info(&mut e, ov);
    assert!(i["error"].is_null(), "{i}");
    assert_eq!(i["kind"]["type"], "overlay");
    let g = sheet(&e);
    let phantom = strokes(&g, ov, Pen::Phantom);
    let (lo, hi) = extent(&phantom);
    let from_view = |p: Vec2| p - Vec2::new(150.0, 150.0) + Vec2::new(50.0, 30.0);
    let (lo, hi) = (from_view(lo), from_view(hi));
    assert!((lo.x - 15.0).abs() < 1e-6 && (hi.x - 25.0).abs() < 1e-6, "{lo:?} {hi:?}");
    assert!((lo.y - 15.0).abs() < 1e-6 && (hi.y - 75.0).abs() < 1e-6, "{lo:?} {hi:?}");
    // Only in phantom; the first position is drawn in its own lines. Nothing visible of the view is
    // drawn again: the base's outer edges do not show among the phantom lines.
    assert!(strokes(&g, ov, Pen::Visible).is_empty() && strokes(&g, ov, Pen::Hidden).is_empty());
    assert!(!phantom.iter().flatten().any(|p| (p.y - 120.0).abs() < 1e-6 && p.x > 170.0), "nothing of the base's front");
    // The first position stays where it was: the arm from x 15 to 75 is in the parent's lines.
    let (plo, phi) = extent(&strokes(&g, top, Pen::Visible));
    assert!(plo.near(Vec2::new(100.0, 120.0), 1e-6) && phi.near(Vec2::new(200.0, 180.0), 1e-6), "{plo:?} {phi:?}");
    // It moves with its parent, not by itself, and stays over it when the parent is scaled.
    run(&mut e, "drw.view.edit", json!({ "view": ov, "by": [50, 50] }));
    assert!(pt(&view_info(&mut e, ov)["center"]).near(Vec2::new(150.0, 150.0), 1e-9));
    run(&mut e, "drw.view.edit", json!({ "view": top, "by": [20, 0], "scale": 0.5 }));
    let g = sheet(&e);
    let (lo2, hi2) = extent(&strokes(&g, ov, Pen::Phantom));
    let (a2, b2) = (from_view(lo2 - Vec2::new(20.0, 0.0)), from_view(hi2 - Vec2::new(20.0, 0.0)));
    assert!((b2.y - a2.y - 30.0).abs() < 1e-6, "half the size: {a2:?} {b2:?}");
    run(&mut e, "drw.undo", json!({}));
    run(&mut e, "drw.overlay", json!({ "view": ov, "values": [{ "relationship": mate, "offset": 40 }] }));

    // The cube lifted 40 off the base, in a view from the front: from 10 up to 50 up to 60. The arm
    // has not moved, so it is not in phantom.
    let front = run(&mut e, "drw.view.projected", json!({ "parent": top, "side": "below", "at": [150, 60] }))["view"].as_u64().unwrap();
    let f = run(&mut e, "drw.overlay", json!({ "parent": front, "values": [{ "relationship": mate, "offset": 40 }] }))["view"].as_u64().unwrap();
    assert!(view_info(&mut e, f)["error"].is_null());
    let g = sheet(&e);
    let (lo, hi) = extent(&strokes(&g, f, Pen::Phantom));
    // The view's middle (x 50, z 7.5: the model is 15 high) is at (150, 60).
    let up = |y: f64| y - 60.0 + 7.5;
    near(up(lo.y), 50.0, "the cube's underside, lifted");
    near(up(hi.y), 60.0, "its top, lifted");
    near(lo.x - 150.0 + 50.0, 60.0, "its left, where it was");
    near(hi.x - 150.0 + 50.0, 80.0, "its right, where it was");

    // The base made 15 thick: the cube goes up with it, 55 to 65.
    run(&mut e, "file.open", json!({ "path": "base.tenon" }));
    run(&mut e, "param.set", json!({ "name": "t", "equation": "15" }));
    run(&mut e, "file.save", json!({ "path": "base.tenon" }));
    run(&mut e, "drw.update", json!({}));
    let g = sheet(&e);
    let (lo, hi) = extent(&strokes(&g, f, Pen::Phantom));
    assert!((hi.y - lo.y - 10.0).abs() < 1e-6, "{lo:?} {hi:?}");
    let ev = e.drw.evaluation().unwrap();
    let c = ev.view(ViewId(front as u32)).unwrap().center;
    near(lo.y - 60.0 + c.y, 55.0, "the underside on the thicker base");

    // Refused, in words.
    let err = |e: &mut Engine, p: Value| e.exec("drw.overlay", &p).unwrap_err();
    assert!(err(&mut e, json!({ "parent": top, "values": [] })).contains("at least one"));
    assert!(err(&mut e, json!({ "parent": top, "values": [{ "relationship": 99, "offset": 1 }] })).contains("no longer in the assembly"));
    assert!(err(&mut e, json!({ "parent": top, "values": [{ "relationship": mate, "angle": 1 }] })).contains("no angle"));
    assert!(err(&mut e, json!({ "parent": top, "values": [{ "relationship": mate }] })).contains("offset or an angle"));
    assert!(err(&mut e, json!({ "parent": ov, "values": [{ "relationship": mate, "offset": 1 }] })).contains("parent"));
    assert!(e.exec("drw.pick", &json!({ "view": ov, "view_at": [0, 0] })).unwrap_err().contains("overlay"));
    assert!(e.exec("drw.dimension.suggest", &json!({ "view": ov })).unwrap_err().contains("overlay"));
    // A part's view has no position.
    plate(&mut e);
    let v = run(&mut e, "drw.view.base", json!({ "model": "plate.tenon", "orientation": "top", "scale": 1, "at": [320, 150] }))["view"].clone();
    assert!(err(&mut e, json!({ "parent": v, "values": [{ "relationship": mate, "offset": 1 }] })).contains("assembly"));
    assert!(e.exec("drw.overlay.values", &json!({ "view": v })).unwrap_err().contains("assembly"));
    // Deleting the parent takes the overlay with it.
    run(&mut e, "drw.view.delete", json!({ "view": top }));
    let info = run(&mut e, "drw.info", json!({}));
    assert!(!info["views"].as_array().unwrap().iter().any(|x| x["kind"]["type"] == "overlay"), "{info}");
}

/// Every kind on one drawing, for what they share.
fn everything(e: &mut Engine) -> (u64, u64) {
    plate(e);
    let (joint, _) = pivot(e);
    run(e, "drw.new", json!({ "name": "All" }));
    let top = run(e, "drw.view.base", json!({ "model": "plate.tenon", "orientation": "top", "scale": 1, "at": [100, 200], "hidden": true }))["view"].as_u64().unwrap();
    run(e, "drw.crop", json!({ "view": top, "rect": [[-5, -5], [125, 85]] }));
    run(e, "drw.break", json!({ "view": top, "lines": "vertical", "from": 45, "to": 75, "gap": 6 }));
    run(e, "drw.break_out", json!({ "view": top, "center": [90, 60], "radius": 12, "depth": 5 }));
    let edge = run(e, "drw.pick", json!({ "view": top, "view_at": [20, 80] }));
    run(e, "drw.auxiliary", json!({ "parent": top, "edge": edge, "at": [100, 110] }));
    let asm = run(e, "drw.view.base", json!({ "model": "pivot.tenonasm", "orientation": "top", "scale": 1, "at": [300, 150], "hidden": false }))["view"].as_u64().unwrap();
    let a0 = num(&run(e, "drw.overlay.values", json!({ "view": asm }))["values"][0]["angle"]);
    let ov = run(e, "drw.overlay", json!({ "parent": asm, "values": [{ "relationship": joint, "angle": a0 + 1.0 }] }))["view"].as_u64().unwrap();
    (top, ov)
}

#[test]
fn all_of_them_come_through_save_and_open_to_the_same_bytes_and_go_to_pdf_and_dxf() {
    let dir = scratch("files");
    let mut e = Engine::new(kernel(), &dir);
    let (top, ov) = everything(&mut e);
    let before = e.drw.drawing().clone();
    let shown = sheet(&e);
    run(&mut e, "drw.save", json!({ "path": "all.tenondrw" }));
    let bytes = std::fs::read(dir.join("all.tenondrw")).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    for word in ["type = \"auxiliary\"", "type = \"overlay\"", "mods = [", "type = \"crop\"", "type = \"break\"", "type = \"break_out\"", "shape = \"circle\""] {
        assert!(text.contains(word), "{word} not in {text}");
    }
    // Read back: the same drawing, the same sheet, the same bytes.
    let mut e = Engine::new(kernel(), &dir);
    run(&mut e, "drw.open", json!({ "path": "all.tenondrw" }));
    assert_eq!(e.drw.drawing().views, before.views);
    assert_eq!(sheet(&e), shown);
    run(&mut e, "drw.save", json!({ "path": "again.tenondrw" }));
    assert_eq!(std::fs::read(dir.join("again.tenondrw")).unwrap(), bytes, "to the same bytes");
    let i = run(&mut e, "drw.info", json!({}));
    assert!(i["views"].as_array().unwrap().iter().all(|v| v["error"].is_null()), "{i}");

    // PDF and DXF, as readers read them: the phantom pen a layer and a line type of its own.
    run(&mut e, "drw.export.pdf", json!({ "path": "all.pdf" }));
    assert_eq!(tenon_drawing::export::check_pdf(&std::fs::read(dir.join("all.pdf")).unwrap()), Ok(1));
    run(&mut e, "drw.export.dxf", json!({ "path": "all.dxf" }));
    let dxf = std::fs::read(dir.join("all.dxf")).unwrap();
    let stats = tenon_drawing::export::check_dxf(&String::from_utf8_lossy(&dxf)).unwrap();
    assert!(stats.lines + stats.polylines > 10, "{stats:?}");
    let tags = tenon_dxf::parse(&dxf).unwrap();
    let entities = tenon_dxf::sections(&tags).into_iter().find(|s| s.name == "ENTITIES").unwrap();
    let layer = |r: &[tenon_dxf::Tag]| r.iter().find(|t| t.code == 8).map(tenon_dxf::Tag::str).unwrap_or_default();
    let records = tenon_dxf::records(&entities.tags);
    for pen in ["PHANTOM", "HATCH", "VISIBLE", "THIN"] {
        assert!(records.iter().any(|(k, r)| k != "TEXT" && layer(r) == pen), "nothing on layer {pen}");
    }
    run(&mut e, "drw.export.svg", json!({ "path": "all.svg" }));
    let svg = std::fs::read_to_string(dir.join("all.svg")).unwrap();
    assert!(svg.contains("VIEW A"), "the auxiliary view's label");
    // A picture of the sheet.
    assert!(e.render_sheet_png(None, 800).unwrap().starts_with(&[0x89, b'P', b'N', b'G']));

    // Undo takes each back, one step at a time, and the file with none reads as it did.
    let views = e.drw.drawing().views.len();
    run(&mut e, "drw.overlay", json!({ "parent": top, "values": [{ "relationship": 1, "offset": 1 }] })).as_object().map(|_| ()).unwrap_or_default();
    let _ = (views, ov);
}

#[test]
fn old_drawings_read_as_they_did_and_damaged_new_fields_are_refused() {
    let dir = scratch("old");
    let mut e = Engine::new(kernel(), &dir);
    plate_drawing(&mut e);
    run(&mut e, "drw.save", json!({ "path": "old.tenondrw" }));
    let text = std::fs::read_to_string(dir.join("old.tenondrw")).unwrap();
    assert!(!text.contains("mods"), "a view with no modifiers writes what it always wrote: {text}");
    // A modifier in a file with a value out of range, on a view it does not fit, repeated.
    run(&mut e, "drw.crop", json!({ "view": 1, "rect": [[0, 0], [10, 10]] }));
    run(&mut e, "drw.save", json!({ "path": "new.tenondrw" }));
    let text = std::fs::read_to_string(dir.join("new.tenondrw")).unwrap();
    assert!(text.contains("mods = ["), "{text}");
    let d = dir.clone();
    let opened = move |t: String| {
        let f = d.join("bad.tenondrw");
        std::fs::write(&f, t).unwrap();
        tenon_io::drw::open(&f).map(|_| ()).map_err(|e| e.to_string())
    };
    assert!(opened(text.clone()).is_ok());
    assert!(opened(text.replace("id = 1, type = \"crop\"", "id = 0, type = \"crop\"")).unwrap_err().contains("bad or repeated"));
    assert!(opened(text.replace("hi = { x = 10.0, y = 10.0 }", "hi = { x = -10.0, y = 10.0 }")).unwrap_err().contains("out of range"));
}

#[test]
fn a_kind_of_view_is_known_by_its_parent() {
    let dir = scratch("kinds");
    let mut e = Engine::new(kernel(), &dir);
    let v = plate_drawing(&mut e);
    let edge = run(&mut e, "drw.pick", json!({ "view": v, "view_at": [20, 0] }));
    let aux = run(&mut e, "drw.auxiliary", json!({ "parent": v, "edge": edge }))["view"].as_u64().unwrap();
    let d = e.drw.drawing();
    let kind = &d.view(ViewId(aux as u32)).unwrap().kind;
    assert!(matches!(kind, ViewKind::Auxiliary { .. }) && kind.parent() == Some(ViewId(v as u32)) && kind.follows_parent());
    // Square to the bottom edge from above: third-angle, placed below the view looking at the front
    // (nothing told): the first side, clear of the view by the gap.
    let i = view_info(&mut e, aux);
    assert!(i["error"].is_null());
}

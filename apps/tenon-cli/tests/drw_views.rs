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
use tenon_geom::{Frame, Vec2, Vec3};
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
    run(e, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "fix", "point": rect["corners"][0] } }));
    run(e, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "length", "line": rect["lines"][0], "value": 120 }, "equation": "w" }));
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

    let side = run(&mut e, "model.face_ref", json!({ "origin": { "type": "side", "feature": body, "curve": cap_line } }));
    // The sketch on the face has the part's origin projected onto it as its own; the hole goes in
    // the middle of the face, at (85, 45, 12.5).
    let on_face = run(&mut e, "sketch.create", json!({ "face": side }))["feature"].clone();
    let n = Vec3::new(1.0, 1.0, 0.0) * (1.0 / SQRT_2);
    let onto = Frame::from_normal(n * (130.0 / SQRT_2), n).unwrap();
    let middle = Vec3::new(85.0, 45.0, 12.5);
    let there = middle - onto.origin();
    run(&mut e, "sketch.point", json!({ "sketch": on_face, "x": there.dot(onto.x()), "y": there.dot(onto.y()) }));
    run(&mut e, "model.hole", json!({ "sketch": on_face, "diameter": 10, "depth": 8, "flat_bottom": true }));
    near(num(&run(&mut e, "model.mass", json!({}))["bodies"][0]["volume"]), (6000.0 - 450.0) * 25.0 - 200.0 * PI, "the part");
    run(&mut e, "file.save", json!({ "path": "wedge.tenon" }));

    run(&mut e, "drw.new", json!({ "name": "Wedge" }));
    let top =
        run(&mut e, "drw.view.base", json!({ "model": "wedge.tenon", "orientation": "top", "scale": 1, "at": [120, 110], "hidden": false }))["view"]
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
        tenon_drawing::views::project(&ev.view(ViewId(top as u32)).unwrap().frame, Vec3::new(100.0, 30.0, 25.0))
    };
    let g_aux = {
        let ev = e.drw.evaluation().unwrap();
        tenon_drawing::views::project(&ev.view(ViewId(aux as u32)).unwrap().frame, Vec3::new(100.0, 30.0, 25.0))
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
    // The hole in that face, square to the eye: a true circle of 10 across, where it is. From above
    // it shows as an edge-on slot, not a circle.
    let round: Vec<Vec2> = tenon_drawing::clean::items(&g)
        .into_iter()
        .filter_map(|(p, i)| match i {
            tenon_drawing::clean::Item::Circle { c, r } if p == Pen::Visible && (r - 5.0).abs() < 1e-6 => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(round.len(), 1, "{round:?}");
    let seen = {
        let ev = e.drw.evaluation().unwrap();
        tenon_drawing::views::project(&ev.view(ViewId(aux as u32)).unwrap().frame, middle)
    };
    assert!(round[0].near(on_sheet(&mut e, aux, seen.x, seen.y), 1e-6), "{round:?}");
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
    let was = view_info(&mut e, aux)["kind"]["flip"].as_bool().unwrap();
    run(&mut e, "drw.view.edit", json!({ "view": aux, "flip": !was }));
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
    // Moved, it lines up with its parent again, square to the edge as it is now.
    run(&mut e, "drw.view.edit", json!({ "view": aux, "by": [1, 1] }));
    let (c, pc) = (pt(&view_info(&mut e, aux)["center"]), pt(&view_info(&mut e, top)["center"]));
    near((c - pc).dot(Vec2::new(0.8, -0.6)), 0.0, "lined up with its parent, along the edge as it is now");

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
    assert!((total - (50.0 + 30.0 + 10.0 * PI)).abs() < 0.05, "{total}");
    // Its centre mark, none on the other hole.
    let (mlo, mhi) = extent(&strokes(&g, v, Pen::Center));
    assert!(mhi.x <= 140.0 + 1e-9 && mlo.y >= 105.0 - 1e-9, "{mlo:?} {mhi:?}");
    // The dimension says the plate's true width; its value is measured from the model, not the crop.
    let info = run(&mut e, "drw.info", json!({}));
    near(num(&info["annotations"][0]["value"]), 120.0, "the dimension after the crop");
    assert_eq!(info["views"][0]["modifiers"][0]["type"], "crop", "{info}");
    let size = pt(&info["views"][0]["size"]);
    assert!(size.near(Vec2::new(50.0, 30.0), 1e-9), "{size:?}");
    // A hole table lists both holes, as they are; only the hole drawn has its tag on the view.
    let rows = run(&mut e, "drw.hole_table", json!({ "view": v, "at": [260, 250] }));
    assert_eq!(rows["rows"].as_array().unwrap().len(), 2, "{rows}");
    let tags = |g: &Graphics, t: &str| g.texts.iter().filter(|(_, x)| x.text == t).count();
    let g = sheet(&e);
    assert_eq!((tags(&g, "A1"), tags(&g, "A2")), (2, 1), "A1 in the table and on the hole; A2 in the table");
    run(&mut e, "drw.undo", json!({}));
    // Suggested dimensions are of what is drawn: the one hole's size, once, no overall sizes.
    let suggested = run(&mut e, "drw.dimension.suggest", json!({ "view": v }));
    let shown: Vec<&str> = suggested["suggestions"].as_array().unwrap().iter().map(|s| s["shown"].as_str().unwrap()).collect();
    assert_eq!(shown, ["Ø10"], "{suggested}");
    // An edge is there to pick where it is drawn; beyond the crop there is nothing.
    assert!(e.exec("drw.pick", &json!({ "view": v, "view_at": [20, 0] })).is_ok());
    assert!(e.exec("drw.pick", &json!({ "view": v, "view_at": [90, 0] })).unwrap_err().contains("no edge"));
    // A long edge whose two ends are both outside a crop is picked where it crosses it.
    run(&mut e, "drw.crop", json!({ "view": v, "rect": [[50, -5], [70, 5]], "outline": false }));
    assert!(e.exec("drw.pick", &json!({ "view": v, "view_at": [60, 0] })).is_ok());
    assert_eq!(view_info(&mut e, v)["modifiers"].as_array().unwrap().len(), 1, "a view has one crop: the same one changed");
    // So is one across a detail's circle, with both ends outside it.
    let det = run(&mut e, "drw.view.detail", json!({ "parent": v, "center": [60, 0], "radius": 8, "at": [320, 60] }))["view"].clone();
    assert!(e.exec("drw.pick", &json!({ "view": det, "view_at": [60, 0] })).is_ok());
    assert!(e.exec("drw.pick", &json!({ "view": det, "view_at": [10, 0] })).unwrap_err().contains("no edge"));
    run(&mut e, "drw.undo", json!({}));

    // A circle: 10 of radius about (30, 20): the hole's whole, and no edge of the plate.
    run(&mut e, "drw.crop", json!({ "view": v, "center": [30, 20], "radius": 10, "rect": null, "outline": true }));
    let g = sheet(&e);
    let total: f64 = strokes(&g, v, Pen::Visible).iter().map(|r| length(r)).sum();
    assert!((total - 10.0 * PI).abs() < 0.05, "{total}");
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
    assert!(err(&mut e, json!({ "view": v, "lines": "vertical", "from": 45, "to": 60 })).contains("elsewhere"), "over another band");
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
    run(e, "model.hole", json!({ "sketch": holes, "diameter": 10, "depth": 10, "flat_bottom": true }));
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
    let v =
        run(&mut e, "drw.view.base", json!({ "model": "block.tenon", "orientation": "top", "scale": 1, "at": [150, 150], "hidden": true }))["view"]
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
    let rim: f64 = strokes(&g, v, Pen::Visible)
        .iter()
        .flat_map(|r| r.windows(2).map(|w| (w[0], w[1])))
        .filter(|(a, b)| (a.y - 135.0).abs() < 1e-6 && (b.y - 135.0).abs() < 1e-6)
        .map(|(a, b)| a.dist(b))
        .sum();
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
    // A depth deeper than the block goes through it, no farther.
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": 1, "depth": 5000 }));
    let g = sheet(&e);
    assert_eq!(hatch_length(&g, v), 0.0);
    assert!(view_info(&mut e, v)["error"].is_null());
    run(&mut e, "drw.undo", json!({}));
    // Through a boundary bigger than the block: nothing left, and no failure.
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": 1, "rect": [[-10, -10], [70, 50]], "center": null, "radius": null }));
    let i = view_info(&mut e, v);
    assert!(i["error"].is_null(), "{i}");
    assert_eq!(num(&i["visible_curves"]), 0.0, "{i}");

    // Back to the rectangle, 15 down, by undo. The block made 30 thick: the floor at 15 now, above
    // the hole: the depth is from the top as it is.
    for _ in 0..4 {
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
    assert!((hatch_length(&g, v) - 400.0).abs() < 400.0 * 0.03, "the floor whole: {}", hatch_length(&g, v));

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
    let base_top = run(
        e,
        "asm.geom",
        json!({ "component": 1, "edge": [{ "type": "cap", "feature": base, "end": "end" }, { "type": "side", "feature": base, "curve": hole }] }),
    );
    let arm_under = run(
        e,
        "asm.geom",
        json!({ "component": a, "edge": [{ "type": "cap", "feature": arm, "end": "start" }, { "type": "side", "feature": arm, "curve": arm_hole }] }),
    );
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
    let top = run(&mut e, "drw.view.base", json!({ "model": "pivot.tenonasm", "orientation": "top", "scale": 1, "at": [150, 150], "hidden": false }))
        ["view"]
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
    // The view's middle (x 50, z 10: the model is 20 high, to the cube's top) is at (150, 60).
    let up = |y: f64| y - 60.0 + 10.0;
    near(up(lo.y), 50.0, "the cube's underside, lifted");
    near(up(hi.y), 60.0, "its top, lifted");
    near(lo.x - 150.0 + 50.0, 60.0, "its left, where it was");
    near(hi.x - 150.0 + 50.0, 80.0, "its right, where it was");

    // The base made 15 thick: the cube goes up with it, 55 to 65.
    run(&mut e, "file.open", json!({ "path": "base.tenon" }));
    run(&mut e, "param.set", json!({ "name": "t", "equation": "15" }));
    run(&mut e, "file.save", json!({ "path": "base.tenon" }));
    run(&mut e, "asm.open", json!({ "path": "pivot.tenonasm" }));
    run(&mut e, "asm.update", json!({}));
    run(&mut e, "asm.save", json!({ "path": "pivot.tenonasm" }));
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
    // The mate deleted from the assembly: the overlay stays, drawn empty, and says what to do.
    run(&mut e, "asm.open", json!({ "path": "pivot.tenonasm" }));
    run(&mut e, "asm.delete", json!({ "relationship": mate }));
    run(&mut e, "asm.save", json!({ "path": "pivot.tenonasm" }));
    run(&mut e, "drw.update", json!({}));
    let i = view_info(&mut e, f);
    assert!(i["error"].as_str().is_some_and(|m| m.contains("no longer in the assembly")), "{i}");
    assert!(strokes(&sheet(&e), f, Pen::Phantom).is_empty());
    // Deleting the parent takes the overlay with it.
    run(&mut e, "drw.view.delete", json!({ "view": top }));
    let info = run(&mut e, "drw.info", json!({}));
    assert!(!info["views"].as_array().unwrap().iter().any(|x| x["kind"]["type"] == "overlay"), "{info}");
}

/// Every kind on one drawing, for what they share.
fn everything(e: &mut Engine) {
    plate(e);
    let (joint, _) = pivot(e);
    run(e, "drw.new", json!({ "name": "All" }));
    let top = run(e, "drw.view.base", json!({ "model": "plate.tenon", "orientation": "top", "scale": 1, "at": [100, 200], "hidden": true }))["view"]
        .as_u64()
        .unwrap();
    run(e, "drw.crop", json!({ "view": top, "rect": [[-5, -5], [125, 85]] }));
    run(e, "drw.break", json!({ "view": top, "lines": "vertical", "from": 45, "to": 75, "gap": 6 }));
    run(e, "drw.break_out", json!({ "view": top, "center": [90, 60], "radius": 12, "depth": 5 }));
    let edge = run(e, "drw.pick", json!({ "view": top, "view_at": [20, 80] }));
    run(e, "drw.auxiliary", json!({ "parent": top, "edge": edge, "at": [100, 110] }));
    let asm =
        run(e, "drw.view.base", json!({ "model": "pivot.tenonasm", "orientation": "top", "scale": 1, "at": [300, 150], "hidden": false }))["view"]
            .as_u64()
            .unwrap();
    let a0 = num(&run(e, "drw.overlay.values", json!({ "view": asm }))["values"][0]["angle"]);
    run(e, "drw.overlay", json!({ "parent": asm, "values": [{ "relationship": joint, "angle": a0 + 1.0 }] }));
}

#[test]
fn all_of_them_come_through_save_and_open_to_the_same_bytes_and_go_to_pdf_and_dxf() {
    let dir = scratch("files");
    let mut e = Engine::new(kernel(), &dir);
    everything(&mut e);
    let before = e.drw.drawing().clone();
    let shown = sheet(&e);
    run(&mut e, "drw.save", json!({ "path": "all.tenondrw" }));
    let bytes = std::fs::read(dir.join("all.tenondrw")).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    for word in [
        "type = \"auxiliary\"",
        "type = \"overlay\"",
        "mods = [",
        "type = \"crop\"",
        "type = \"break\"",
        "type = \"break_out\"",
        "shape = \"circle\"",
    ] {
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
fn an_auxiliary_view_is_seen_as_the_drawing_projects() {
    let dir = scratch("angles");
    let mut e = Engine::new(kernel(), &dir);
    plate(&mut e);
    // Below a view from above, square to its bottom edge. Third-angle: seen from the front, the side
    // it is on. First-angle: seen from behind, the side away from it.
    for (standard, seen) in [("ansi", -1.0), ("iso", 1.0)] {
        run(&mut e, "drw.new", json!({ "name": "Plate", "standard": standard }));
        let v = run(&mut e, "drw.view.base", json!({ "model": "plate.tenon", "orientation": "top", "scale": 1, "at": [150, 170], "hidden": false }))
            ["view"]
            .clone();
        let edge = run(&mut e, "drw.pick", json!({ "view": v, "view_at": [20, 0] }));
        let aux = run(&mut e, "drw.auxiliary", json!({ "parent": v, "edge": edge, "at": [170, 60] }))["view"].as_u64().unwrap();
        let i = view_info(&mut e, aux);
        near(num(&i["direction"][1]), seen, standard);
        // It slid straight down: (170, 60) is 20 off to the side of the line from its parent.
        assert!(pt(&i["center"]).near(Vec2::new(150.0, 60.0), 1e-9), "{i}");
    }
}

#[test]
fn an_auxiliary_view_told_nowhere_goes_clear_of_its_parent() {
    let dir = scratch("kinds");
    let mut e = Engine::new(kernel(), &dir);
    let v = plate_drawing(&mut e);
    let edge = run(&mut e, "drw.pick", json!({ "view": v, "view_at": [20, 0] }));
    let aux = run(&mut e, "drw.auxiliary", json!({ "parent": v, "edge": edge }))["view"].as_u64().unwrap();
    let d = e.drw.drawing();
    let kind = &d.view(ViewId(aux as u32)).unwrap().kind;
    assert!(matches!(kind, ViewKind::Auxiliary { .. }) && kind.parent() == Some(ViewId(v as u32)) && kind.follows_parent());
    // The plate from above is 120 x 80; square to its bottom edge it is 120 x 10. Half of 80, half of
    // 10 and the gap of 20 from its parent's middle at (150, 150), up or down the sheet.
    let i = view_info(&mut e, aux);
    assert!(i["error"].is_null());
    let c = pt(&i["center"]);
    near(c.x, 150.0, "across");
    near((c.y - 150.0).abs(), 40.0 + 5.0 + 20.0, "up or down the sheet");
    // On the side it is seen from: from behind, up the sheet; from in front, down it.
    let z = num(&i["direction"][1]);
    near(z.abs(), 1.0, "it looks along Y");
    assert_eq!(c.y > 150.0, z > 0.0, "{i}");

    // The plate's extrusion suppressed: the edge is
    // gone. The view stays, looking the way it did, and says what to do.
    run(&mut e, "file.open", json!({ "path": "plate.tenon" }));
    let tree = run(&mut e, "model.tree", json!({}));
    let extrude = tree["features"].as_array().unwrap().iter().find(|f| f["type"] == "Extrude").unwrap()["id"].clone();
    run(&mut e, "feature.suppress", json!({ "feature": extrude }));
    run(&mut e, "file.save", json!({ "path": "plate.tenon" }));
    run(&mut e, "drw.update", json!({}));
    let i = view_info(&mut e, aux);
    assert!(i["error"].as_str().is_some_and(|m| m.contains("pick another edge")), "{i}");
    near(num(&i["direction"][1]), z, "it looks the way it did");
}

/// A 100 x 60 x 25 block with its corner cut from (100, 30) to (70, 60): a face at 45 degrees, 30
/// root 2 long and 25 wide. In a drawing: seen from above (at (120, 110)), and an auxiliary view
/// square to that face, up and to the right of it. Returns the two views.
fn wedge_drawing(e: &mut Engine) -> (u64, u64) {
    run(e, "file.new", json!({ "name": "Wedge" }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    let first = run(e, "sketch.line", json!({ "sketch": sk, "x1": 0, "y1": 60, "x2": 0, "y2": 0 }));
    let mut last = first["end"].clone();
    for (x, y) in [(100, 0), (100, 30), (70, 60)] {
        last = run(e, "sketch.line", json!({ "sketch": sk, "start": last, "x2": x, "y2": y }))["end"].clone();
    }
    run(e, "sketch.line", json!({ "sketch": sk, "start": last, "end": first["start"] }));
    run(e, "model.extrude", json!({ "sketch": sk, "distance": 25 }));
    near(num(&run(e, "model.mass", json!({}))["bodies"][0]["volume"]), 5550.0 * 25.0, "the part");
    run(e, "file.save", json!({ "path": "wedge.tenon" }));
    run(e, "drw.new", json!({ "name": "Wedge" }));
    let top = run(e, "drw.view.base", json!({ "model": "wedge.tenon", "orientation": "top", "scale": 1, "at": [120, 110], "hidden": false }))["view"]
        .as_u64()
        .unwrap();
    let edge = run(e, "drw.pick", json!({ "view": top, "view_at": [85, 45] }));
    let at = pt(&edge["at"]) + Vec2::new(90.0, 90.0);
    let aux = run(e, "drw.auxiliary", json!({ "parent": top, "edge": edge, "at": [at.x, at.y] }))["view"].as_u64().unwrap();
    (top, aux)
}

/// Where a point of the model is in a view's own coordinates.
fn seen_in(e: &Engine, view: u64, p: Vec3) -> Vec2 {
    let ev = e.drw.evaluation().unwrap();
    tenon_drawing::views::project(&ev.view(ViewId(view as u32)).unwrap().frame, p)
}

/// A view's hatch against a floor's area by hand: its length is the area over the 3 between lines.
fn hatched(g: &Graphics, view: u64, area: f64, what: &str) {
    let h = hatch_length(g, view);
    assert!((h - area / 3.0).abs() < area / 3.0 * 0.03, "{what}: {h} against {}", area / 3.0);
}

#[test]
fn a_break_out_goes_down_from_the_model_itself_and_says_when_it_cuts_nothing() {
    let dir = scratch("break-out-near");
    let mut e = Engine::new(kernel(), &dir);
    let (top, aux) = wedge_drawing(&mut e);
    // Seen along (1, 1, 0) / root 2 the nearest of the block is the inclined face at 130 / root 2 =
    // 91.92; the corner of its box, at (100, 60), is at 160 / root 2 = 113.14, 21.2 clear of it. A
    // 10 x 10 square in the middle of the face is cut square to it: 100 of floor, however deep.
    let c = seen_in(&e, aux, Vec3::new(85.0, 45.0, 12.5));
    let rect = json!([[c.x - 5.0, c.y - 5.0], [c.x + 5.0, c.y + 5.0]]);
    let before = num(&view_info(&mut e, aux)["visible_curves"]);
    let m = run(&mut e, "drw.break_out", json!({ "view": aux, "rect": rect, "depth": 5 }))["modifier"].clone();
    let i = view_info(&mut e, aux);
    assert!(i["error"].is_null(), "{i}");
    assert!(num(&i["visible_curves"]) > before, "the cut's rim is drawn: {i}");
    hatched(&sheet(&e), aux, 100.0, "5 down");
    for depth in [0.5, 10.0, 21.2, 22.0] {
        run(&mut e, "drw.break_out", json!({ "view": aux, "modifier": m, "depth": depth }));
        assert!(view_info(&mut e, aux)["error"].is_null());
        hatched(&sheet(&e), aux, 100.0, &format!("{depth} down"));
    }
    // The hatching lies over the square, where the view draws it.
    let (lo, hi) = extent(&strokes(&sheet(&e), aux, Pen::Hatch));
    let (p, q) = (on_sheet(&mut e, aux, c.x - 5.0, c.y - 5.0), on_sheet(&mut e, aux, c.x + 5.0, c.y + 5.0));
    assert!(lo.x >= p.x - 1e-6 && lo.y >= p.y - 1e-6 && hi.x <= q.x + 1e-6 && hi.y <= q.y + 1e-6, "{lo:?} {hi:?} {p:?} {q:?}");

    // From above, the corner taken off the block is inside the view's box and has nothing in it: the
    // cut takes nothing away and the view says so.
    let m = run(&mut e, "drw.break_out", json!({ "view": top, "rect": [[92, 52], [99, 59]], "depth": 5 }))["modifier"].clone();
    let i = view_info(&mut e, top);
    assert!(i["error"].as_str().is_some_and(|t| t.contains("cuts nothing") && t.contains("Break Out 1")), "{i}");
    assert_eq!(hatch_length(&sheet(&e), top), 0.0);
    // Through, the same. Moved over the part it says nothing, and hatches.
    run(&mut e, "drw.break_out", json!({ "view": top, "modifier": m, "through": true }));
    assert!(view_info(&mut e, top)["error"].as_str().is_some_and(|t| t.contains("cuts nothing")));
    run(&mut e, "drw.break_out", json!({ "view": top, "modifier": m, "rect": [[10, 10], [30, 30]], "depth": 5 }));
    assert!(view_info(&mut e, top)["error"].is_null());
    hatched(&sheet(&e), top, 400.0, "moved over the part");
}

/// A step, 100 x 60: 30 high for x under 50, 10 high beyond. Its volume is 120000. In a drawing, seen
/// from above at 1:1, the middle of the view (50, 30) at (150, 150).
fn step_drawing(e: &mut Engine) -> u64 {
    run(e, "file.new", json!({ "name": "Step" }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 100, "y2": 60 }));
    run(e, "model.extrude", json!({ "sketch": sk, "distance": 10 }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 50, "y2": 60 }));
    run(e, "model.extrude", json!({ "sketch": sk, "distance": 30 }));
    near(num(&run(e, "model.mass", json!({}))["bodies"][0]["volume"]), 120_000.0, "the step");
    run(e, "file.save", json!({ "path": "step.tenon" }));
    run(e, "drw.new", json!({ "name": "Step" }));
    run(e, "drw.view.base", json!({ "model": "step.tenon", "orientation": "top", "scale": 1, "at": [150, 150], "hidden": false }))["view"]
        .as_u64()
        .unwrap()
}

#[test]
fn a_break_out_hatches_what_it_cut_and_not_what_was_already_there() {
    let dir = scratch("break-out-floor");
    let mut e = Engine::new(kernel(), &dir);
    let v = step_drawing(&mut e);
    // Over the step, x 30 to 70, y 10 to 50. 5 down the floor is at 25, and only the high part (x 30 to
    // 50) reaches it: 20 x 40.
    let m = run(&mut e, "drw.break_out", json!({ "view": v, "rect": [[30, 10], [70, 50]], "depth": 5 }))["modifier"].clone();
    let g = sheet(&e);
    hatched(&g, v, 800.0, "5 down");
    assert!(extent(&strokes(&g, v, Pen::Hatch)).1.x <= 150.0 + 1e-3, "up to x = 50");
    // 20 down the floor is level with the step's own top at 10: that was never cut, and is not hatched.
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": m, "depth": 20 }));
    let g = sheet(&e);
    hatched(&g, v, 800.0, "20 down, level with the step's top");
    let (lo, hi) = extent(&strokes(&g, v, Pen::Hatch));
    assert!(lo.x >= 130.0 - 1e-6 && hi.x <= 150.0 + 1e-3, "{lo:?} {hi:?}");
    // Nor is a line left on it where the cut met it: over the low part, x 50 to 100 is at the sheet's 150
    // to 200, the rim of the cut on it would run along y = 10, at the sheet's 130. It stops at the wall.
    let on_rim = |g: &Graphics| -> f64 {
        strokes(g, v, Pen::Visible)
            .iter()
            .flat_map(|r| r.windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>())
            .filter(|(a, b)| (a.y - 130.0).abs() < 1e-6 && (b.y - 130.0).abs() < 1e-6 && a.x.min(b.x) >= 150.0 - 1e-6)
            .map(|(a, b)| a.dist(b))
            .sum()
    };
    assert!(on_rim(&g) < 1e-6, "{}", on_rim(&g));
    // 25 down it is under that top too: all of the 40 x 40.
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": m, "depth": 25 }));
    let g = sheet(&e);
    hatched(&g, v, 1600.0, "25 down");
    let (lo, hi) = extent(&strokes(&g, v, Pen::Hatch));
    assert!(lo.x >= 130.0 - 1e-6 && hi.x <= 170.0 + 1e-6, "{lo:?} {hi:?}");

    // Over the low part alone, x 60 to 90: down to the high part's depth it is over air; level with
    // the low part's top it only touches it; below that it cuts.
    let rect = json!([[60, 10], [90, 50]]);
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": m, "rect": rect, "depth": 5 }));
    assert!(view_info(&mut e, v)["error"].as_str().is_some_and(|t| t.contains("cuts nothing")), "over air");
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": m, "depth": 20 }));
    assert!(view_info(&mut e, v)["error"].as_str().is_some_and(|t| t.contains("cuts nothing")), "touching");
    assert_eq!(hatch_length(&sheet(&e), v), 0.0, "what it only touches is not hatched");
    let (lines, whole) = (num(&view_info(&mut e, v)["visible_curves"]), {
        run(&mut e, "drw.break_out", json!({ "view": v, "modifier": m, "suppressed": true }));
        let n = num(&view_info(&mut e, v)["visible_curves"]);
        run(&mut e, "drw.undo", json!({}));
        n
    });
    assert_eq!(lines, whole, "and leaves no line where it touched");
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": m, "depth": 25 }));
    assert!(view_info(&mut e, v)["error"].is_null());
    hatched(&sheet(&e), v, 1200.0, "25 down, in the low part");
}

#[test]
fn an_auxiliary_view_is_of_a_straight_edge_and_a_curve_is_refused_in_words() {
    let dir = scratch("aux-curve");
    let mut e = Engine::new(kernel(), &dir);
    // A plate whose top edge is a curve from (100, 50) to (0, 50), rising in the middle.
    run(&mut e, "file.new", json!({ "name": "Curvy" }));
    let sk = run(&mut e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    let a = run(&mut e, "sketch.line", json!({ "sketch": sk, "x1": 0, "y1": 50, "x2": 0, "y2": 0 }));
    let b = run(&mut e, "sketch.line", json!({ "sketch": sk, "start": a["end"], "x2": 100, "y2": 0 }));
    let c = run(&mut e, "sketch.line", json!({ "sketch": sk, "start": b["end"], "x2": 100, "y2": 50 }));
    run(&mut e, "sketch.spline", json!({ "sketch": sk, "points": [c["end"], [75, 62], [25, 62], a["start"]] }));
    run(&mut e, "model.extrude", json!({ "sketch": sk, "distance": 10 }));
    run(&mut e, "file.save", json!({ "path": "curvy.tenon" }));
    run(&mut e, "drw.new", json!({ "name": "Curvy" }));
    let v =
        run(&mut e, "drw.view.base", json!({ "model": "curvy.tenon", "orientation": "top", "scale": 1, "at": [150, 150], "hidden": false }))["view"]
            .as_u64()
            .unwrap();
    // Find where the curve runs: above the chord, at the middle of the view.
    let curve = (0..200)
        .map(|i| 52.0 + f64::from(i) * 0.1)
        .find_map(|y| e.exec("drw.pick", &json!({ "view": v, "view_at": [50, y] })).ok())
        .expect("the curve is there to pick");
    assert_eq!(curve["kind"], "curve", "{curve}");
    let refused = e.exec("drw.auxiliary", &json!({ "parent": v, "edge": curve })).unwrap_err();
    assert!(refused.contains("curve") && refused.contains("straight"), "{refused}");
    // The straight one beside it is fine, and it cannot be changed for the curve.
    let bottom = run(&mut e, "drw.pick", json!({ "view": v, "view_at": [50, 0] }));
    assert_eq!(bottom["kind"], "line");
    let aux = run(&mut e, "drw.auxiliary", json!({ "parent": v, "edge": bottom }))["view"].clone();
    let refused = e.exec("drw.view.edit", &json!({ "view": aux, "edge": curve })).unwrap_err();
    assert!(refused.contains("curve"), "{refused}");
    // Its chord can still be measured, as it always could.
    near(num(&run(&mut e, "drw.dimension", json!({ "view": v, "type": "horizontal", "a": curve, "by": [0, 20] }))["value"]), 100.0, "the chord");
}

#[test]
fn a_slanted_edge_across_a_break_is_picked_where_it_is_drawn() {
    let dir = scratch("break-slant");
    let mut e = Engine::new(kernel(), &dir);
    // A triangle (0, 0), (120, 0), (120, 80): its long edge is 144.22 and slants across the band.
    run(&mut e, "file.new", json!({ "name": "Tri" }));
    let sk = run(&mut e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    let a = run(&mut e, "sketch.line", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 120, "y2": 0 }));
    let b = run(&mut e, "sketch.line", json!({ "sketch": sk, "start": a["end"], "x2": 120, "y2": 80 }));
    run(&mut e, "sketch.line", json!({ "sketch": sk, "start": b["end"], "end": a["start"] }));
    run(&mut e, "model.extrude", json!({ "sketch": sk, "distance": 10 }));
    run(&mut e, "file.save", json!({ "path": "tri.tenon" }));
    run(&mut e, "drw.new", json!({ "name": "Tri" }));
    let v =
        run(&mut e, "drw.view.base", json!({ "model": "tri.tenon", "orientation": "top", "scale": 1, "at": [150, 150], "hidden": false }))["view"]
            .as_u64()
            .unwrap();
    let long = (120.0_f64 * 120.0 + 80.0 * 80.0).sqrt();
    near(num(&run(&mut e, "drw.pick", json!({ "view": v, "view_at": [20, 13.333_333] }))["length"]), long, "before the break");
    run(&mut e, "drw.break", json!({ "view": v, "lines": "vertical", "from": 40, "to": 80, "gap": 6 }));
    // On each stroke as drawn, on both sides of the band, near it and far from it.
    for x in [20.0, 35.0, 85.0, 100.0] {
        let p = run(&mut e, "drw.pick", json!({ "view": v, "view_at": [x, x * 2.0 / 3.0] }));
        near(num(&p["length"]), long, "the edge, by a click on what is drawn");
    }
    // And by the sheet: the middle of the stroke beyond the band, which runs (80, 53.33) to (120, 80).
    let on = on_sheet(&mut e, v, 100.0, 200.0 / 3.0);
    near(num(&run(&mut e, "drw.pick", json!({ "view": v, "at": [on.x, on.y] }))["length"]), long, "by the sheet");
    // In the band nothing is drawn, so nothing is there to pick: the two strokes end 6 across and 26.67
    // up apart, either end well out of reach of the gap's middle.
    assert!(e.exec("drw.pick", &json!({ "view": v, "view_at": [60, 40] })).unwrap_err().contains("no edge"));
    // A dimension of it from there keeps its true length.
    let p = run(&mut e, "drw.pick", json!({ "view": v, "view_at": [100, 200.0 / 3.0] }));
    near(num(&run(&mut e, "drw.dimension", json!({ "view": v, "type": "aligned", "a": p, "by": [-10, 20] }))["value"]), long, "its dimension");
}

#[test]
fn what_is_projected_from_an_auxiliary_view_goes_with_it_when_it_turns_or_flips() {
    let dir = scratch("aux-child");
    let mut e = Engine::new(kernel(), &dir);
    let v = plate_drawing(&mut e);
    let bottom = run(&mut e, "drw.pick", json!({ "view": v, "view_at": [20, 0] }));
    let aux = run(&mut e, "drw.auxiliary", json!({ "parent": v, "edge": bottom, "at": [150, 70] }))["view"].as_u64().unwrap();
    let child = run(&mut e, "drw.view.projected", json!({ "parent": aux, "side": "right", "at": [300, 70] }))["view"].as_u64().unwrap();
    let centre = |e: &mut Engine, id: u64| pt(&view_info(e, id)["center"]);
    assert!(centre(&mut e, aux).near(Vec2::new(150.0, 70.0), 1e-9));
    assert!(centre(&mut e, child).near(Vec2::new(300.0, 70.0), 1e-9));
    // Seen from the other side: 80 below its parent's middle becomes 80 above; the view projected from
    // it stays beside it.
    run(&mut e, "drw.view.edit", json!({ "view": aux, "flip": true }));
    assert!(centre(&mut e, aux).near(Vec2::new(150.0, 230.0), 1e-9), "{:?}", centre(&mut e, aux));
    assert!(centre(&mut e, child).near(Vec2::new(300.0, 230.0), 1e-9), "{:?}", centre(&mut e, child));
    // Dragged, it still only slides along.
    run(&mut e, "drw.view.edit", json!({ "view": child, "by": [10, 25] }));
    assert!(centre(&mut e, child).near(Vec2::new(310.0, 230.0), 1e-9));
    run(&mut e, "drw.undo", json!({}));
    run(&mut e, "drw.undo", json!({}));
    assert!(centre(&mut e, child).near(Vec2::new(300.0, 70.0), 1e-9), "one undo brings both back");
    // Another edge: the left one, x = 0. The view goes the same 80 from its parent, now across the
    // sheet, and the view from it goes by as much: 150 to the right of it still.
    let left = run(&mut e, "drw.pick", json!({ "view": v, "view_at": [0, 20] }));
    run(&mut e, "drw.view.edit", json!({ "view": aux, "edge": left }));
    let (a, c) = (centre(&mut e, aux), centre(&mut e, child));
    near((a - Vec2::new(150.0, 150.0)).len(), 80.0, "as far from its parent");
    near(a.y, 150.0, "across the sheet from it");
    assert!((c - a).near(Vec2::new(150.0, 0.0), 1e-9), "{a:?} {c:?}");
}

#[test]
fn an_overlay_needs_no_part_shown_to_solve_its_position() {
    let dir = scratch("overlay-hidden");
    let mut e = Engine::new(kernel(), &dir);
    let (joint, mate) = pivot(&mut e);
    run(&mut e, "drw.new", json!({ "name": "Pivot" }));
    let top = run(&mut e, "drw.view.base", json!({ "model": "pivot.tenonasm", "orientation": "top", "scale": 1, "at": [150, 150], "hidden": false }))
        ["view"]
        .as_u64()
        .unwrap();
    let front = run(&mut e, "drw.view.projected", json!({ "parent": top, "side": "below", "at": [150, 60] }))["view"].as_u64().unwrap();
    // The cube lifted 40 off the base, seen from the front.
    let ov = run(&mut e, "drw.overlay", json!({ "parent": front, "values": [{ "relationship": mate, "offset": 40 }] }))["view"].as_u64().unwrap();
    let (lo0, hi0) = extent(&strokes(&sheet(&e), ov, Pen::Phantom));

    // The base, which the cube is mated to, hidden in the assembly: the cube still goes there.
    run(&mut e, "asm.open", json!({ "path": "pivot.tenonasm" }));
    run(&mut e, "asm.visible", json!({ "component": 1, "visible": false }));
    run(&mut e, "asm.save", json!({ "path": "pivot.tenonasm" }));
    run(&mut e, "drw.update", json!({}));
    let i = view_info(&mut e, ov);
    assert!(i["error"].is_null(), "{i}");
    let g = sheet(&e);
    let (lo, hi) = extent(&strokes(&g, ov, Pen::Phantom));
    near(hi.y - lo.y, hi0.y - lo0.y, "the cube, its 10 high");
    near(hi.x - lo.x, 20.0, "the cube, its 20 across");
    // The base is hidden, so nothing of it is drawn: from above, the arm from x 15 to 75 and the cube
    // from 60 to 80.
    let (plo, phi) = extent(&strokes(&g, top, Pen::Visible));
    near(phi.x - plo.x, 80.0 - 15.0, "the arm and the cube, seen from above");

    // The arm hidden as well, and a new position of its joint: read and made as it would be with it
    // shown, with nothing of it to draw.
    run(&mut e, "asm.open", json!({ "path": "pivot.tenonasm" }));
    run(&mut e, "asm.visible", json!({ "component": 2, "visible": false }));
    run(&mut e, "asm.save", json!({ "path": "pivot.tenonasm" }));
    run(&mut e, "drw.update", json!({}));
    let rows = run(&mut e, "drw.overlay.values", json!({ "view": top }));
    let a0 = num(&rows["values"][0]["angle"]);
    let turned =
        run(&mut e, "drw.overlay", json!({ "parent": top, "values": [{ "relationship": joint, "angle": a0 + 1.0 }] }))["view"].as_u64().unwrap();
    assert!(view_info(&mut e, turned)["error"].is_null());
    assert!(strokes(&sheet(&e), turned, Pen::Phantom).is_empty(), "the arm is hidden");
    // Shown again, it is drawn.
    run(&mut e, "asm.open", json!({ "path": "pivot.tenonasm" }));
    run(&mut e, "asm.visible", json!({ "component": 2, "visible": true }));
    run(&mut e, "asm.save", json!({ "path": "pivot.tenonasm" }));
    run(&mut e, "drw.update", json!({}));
    assert!(view_info(&mut e, turned)["error"].is_null());
    assert!(!strokes(&sheet(&e), turned, Pen::Phantom).is_empty());
}

#[test]
fn breaks_that_clash_are_refused_saying_which() {
    let dir = scratch("break-clash");
    let mut e = Engine::new(kernel(), &dir);
    let v = plate_drawing(&mut e);
    let m = run(&mut e, "drw.break", json!({ "view": v, "lines": "vertical", "from": 40, "to": 80, "gap": 6 }))["modifier"].clone();
    let err = e.exec("drw.break", &json!({ "view": v, "lines": "vertical", "from": 60, "to": 100 })).unwrap_err();
    assert!(err.contains("Break 1") && err.contains("40") && err.contains("80") && err.contains("elsewhere"), "{err}");
    // The other way is another matter; and a change of its own does not clash with itself.
    run(&mut e, "drw.break", json!({ "view": v, "lines": "horizontal", "from": 20, "to": 60, "gap": 6 }));
    run(&mut e, "drw.break", json!({ "view": v, "modifier": m, "to": 90 }));
    run(&mut e, "drw.undo", json!({}));
    // Suppressed it still holds its place, and the words say why that is in the way.
    run(&mut e, "drw.break", json!({ "view": v, "modifier": m, "suppressed": true }));
    let err = e.exec("drw.break", &json!({ "view": v, "lines": "vertical", "from": 60, "to": 100 })).unwrap_err();
    assert!(err.contains("Break 1") && err.contains("suppressed"), "{err}");
}

#[test]
fn a_break_never_makes_a_view_longer() {
    let dir = scratch("break-scale");
    let mut e = Engine::new(kernel(), &dir);
    let v = plate_drawing(&mut e);
    run(&mut e, "drw.break", json!({ "view": v, "lines": "vertical", "from": 40, "to": 80, "gap": 6 }));
    // 120 wide, a band of 40 and a gap of 6 of paper. At 1:1: 120 - 40 + 6. At 1:5 the band is 8 of paper:
    // 24 - 8 + 6. At 1:10 the band is 4 of paper, under its gap: it stays as it is, and the view is
    // as long as it was whole, not longer; at 1:20 too.
    for (scale, size) in [(1.0, 86.0), (0.2, 22.0), (0.1, 12.0), (0.05, 6.0)] {
        run(&mut e, "drw.view.edit", json!({ "view": v, "scale": scale }));
        near(pt(&view_info(&mut e, v)["size"]).x, size, &format!("across at {scale}"));
        // Nothing of it runs wider than that.
        let (lo, hi) = extent(&strokes(&sheet(&e), v, Pen::Visible));
        assert!(hi.x - lo.x <= size + 1e-9, "{lo:?} {hi:?} at {scale}");
    }
    // At 1:20 its band is under its gap: it can still be suppressed and used again, as it stands.
    run(&mut e, "drw.break", json!({ "view": v, "modifier": 1, "suppressed": true }));
    near(pt(&view_info(&mut e, v)["size"]).x, 6.0, "suppressed at 1:20");
    run(&mut e, "drw.break", json!({ "view": v, "modifier": 1, "suppressed": false }));
    assert_eq!(view_info(&mut e, v)["modifiers"][0]["from"], 40.0);
    // And back: the gap is kept.
    run(&mut e, "drw.view.edit", json!({ "view": v, "scale": 1 }));
    near(pt(&view_info(&mut e, v)["size"]).x, 86.0, "back at 1:1");
    run(&mut e, "drw.save", json!({ "path": "scaled.tenondrw" }));
    assert!(std::fs::read_to_string(dir.join("scaled.tenondrw")).unwrap().contains("gap = 6.0"));
}

#[test]
fn an_overlay_is_told_plainly_what_it_takes_from_its_parent() {
    let dir = scratch("overlay-words");
    let mut e = Engine::new(kernel(), &dir);
    let (joint, _) = pivot(&mut e);
    run(&mut e, "drw.new", json!({ "name": "Pivot" }));
    let top = run(&mut e, "drw.view.base", json!({ "model": "pivot.tenonasm", "orientation": "top", "scale": 1, "at": [150, 150], "hidden": false }))
        ["view"]
        .as_u64()
        .unwrap();
    let a0 = num(&run(&mut e, "drw.overlay.values", json!({ "view": top }))["values"][0]["angle"]);
    let ov = run(&mut e, "drw.overlay", json!({ "parent": top, "values": [{ "relationship": joint, "angle": a0 + 1.0 }] }))["view"].clone();
    // Whatever the shape, and wherever: the same words as a break out gets.
    for (id, p) in [
        ("drw.crop", json!({ "view": ov, "rect": [[0, 0], [10, 10]] })),
        ("drw.crop", json!({ "view": ov, "rect": [[900, 900], [910, 910]] })),
        ("drw.break", json!({ "view": ov, "lines": "vertical", "from": 0, "to": 900 })),
        ("drw.break", json!({ "view": ov, "lines": "vertical", "from": 900, "to": 950 })),
        ("drw.break_out", json!({ "view": ov, "rect": [[0, 0], [10, 10]], "depth": 5 })),
    ] {
        let err = e.exec(id, &p).unwrap_err();
        assert!(err.contains("modify the parent view"), "{id}: {err}");
    }
    // What it is drawn with is its parent's.
    for key in
        [json!({ "scale": 2 }), json!({ "label": true }), json!({ "hidden": true }), json!({ "tangent": true }), json!({ "centerlines": true })]
    {
        let mut p = key.clone();
        p["view"] = ov.clone();
        let err = e.exec("drw.view.edit", &p).unwrap_err();
        assert!(err.contains("parent"), "{key}: {err}");
    }
    // Nor does it take a set of dimensions: those are of what a view shows of its own.
    let edge = run(&mut e, "drw.pick", json!({ "view": top, "view_at": [100, 30] }));
    let err = e.exec("drw.baseline", &json!({ "view": ov, "datum": edge, "targets": [edge] })).unwrap_err();
    assert!(err.contains("use the parent view"), "{err}");
    let i = view_info(&mut e, ov.as_u64().unwrap());
    assert_eq!(i["scale"], 1.0);
    // Nothing is written with it.
    let g = sheet(&e);
    assert!(!g.texts.iter().any(|(o, _)| *o == Owner::View(ViewId(ov.as_u64().unwrap() as u32))));
    // The parent's scale still goes to it.
    run(&mut e, "drw.view.edit", json!({ "view": top, "scale": 2 }));
    assert_eq!(view_info(&mut e, ov.as_u64().unwrap())["scale"], 2.0);
}

#[test]
fn a_break_out_away_from_the_view_is_refused_and_through_is_not_undone_without_a_depth() {
    let dir = scratch("break-out-words");
    let mut e = Engine::new(kernel(), &dir);
    let v = plate_drawing(&mut e);
    let err = e.exec("drw.break_out", &json!({ "view": v, "rect": [[500, 500], [600, 600]], "depth": 5 })).unwrap_err();
    assert!(err.contains("outside the view"), "{err}");
    let err = e.exec("drw.break_out", &json!({ "view": v, "center": [500, 500], "radius": 5, "through": true })).unwrap_err();
    assert!(err.contains("outside the view"), "{err}");
    // Through false on one that goes through wants a depth; on one with a depth it keeps it.
    let m = run(&mut e, "drw.break_out", json!({ "view": v, "rect": [[10, 10], [50, 50]], "through": true }))["modifier"].clone();
    let err = e.exec("drw.break_out", &json!({ "view": v, "modifier": m, "through": false })).unwrap_err();
    assert!(err.contains("depth"), "{err}");
    assert!(view_info(&mut e, v)["modifiers"][0]["depth"].is_null());
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": m, "depth": 4 }));
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": m, "through": false }));
    assert_eq!(view_info(&mut e, v)["modifiers"][0]["depth"], 4.0);
    assert!(e.exec("drw.break_out", &json!({ "view": v, "rect": [[0, 0], [5, 5]], "through": false })).unwrap_err().contains("depth"));
}

/// How many times a word is written on the sheet.
fn written(g: &Graphics, t: &str) -> usize {
    g.texts.iter().filter(|(_, x)| x.text == t).count()
}

#[test]
fn what_a_view_leaves_out_takes_its_marks_with_it() {
    let dir = scratch("marks");
    let mut e = Engine::new(kernel(), &dir);
    let v = plate_drawing(&mut e);
    // A centre mark by hand on the hole at (90, 60), which is at (180, 170) on the sheet: 5 + 2 each way.
    let hole = run(&mut e, "drw.pick", json!({ "view": v, "view_at": [95, 60] }));
    let mark = run(&mut e, "drw.center_mark", json!({ "view": v, "a": hole }))["annotation"].as_u64().unwrap() as u32;
    let of_mark = |g: &Graphics| -> Vec<Vec<Vec2>> {
        g.strokes.iter().filter(|s| s.0 == Owner::Annotation(tenon_drawing::AnnotId(mark))).map(|s| s.2.clone()).collect()
    };
    let (lo, hi) = extent(&of_mark(&sheet(&e)));
    assert!(lo.near(Vec2::new(173.0, 163.0), 1e-6) && hi.near(Vec2::new(187.0, 177.0), 1e-6), "{lo:?} {hi:?}");
    // Its hole cropped away: the mark goes with it. drw.info tells the same.
    let c = run(&mut e, "drw.crop", json!({ "view": v, "rect": [[-5, -5], [50, 85]], "outline": false }))["modifier"].clone();
    assert!(of_mark(&sheet(&e)).is_empty());
    let info = run(&mut e, "drw.info", json!({}));
    assert_eq!(info["annotations"][0]["segments"], json!([]), "{info}");
    // A crop down the side of the hole: of the mark, what lies inside: to x = 93, sheet 183.
    run(&mut e, "drw.crop", json!({ "view": v, "rect": [[-5, -5], [93, 85]], "outline": false }));
    let (lo, hi) = extent(&of_mark(&sheet(&e)));
    assert!(lo.near(Vec2::new(173.0, 163.0), 1e-6) && hi.near(Vec2::new(183.0, 177.0), 1e-6), "{lo:?} {hi:?}");
    // Suppressed: whole. Then deleted.
    run(&mut e, "drw.crop", json!({ "view": v, "suppressed": true }));
    assert!((extent(&of_mark(&sheet(&e))).1.x - 187.0).abs() < 1e-6);
    run(&mut e, "drw.modifier.delete", json!({ "view": v, "modifier": c }));
    // The hole in the band of a break: gone too.
    let b = run(&mut e, "drw.break", json!({ "view": v, "lines": "vertical", "from": 80, "to": 100, "gap": 6 }))["modifier"].clone();
    assert!(of_mark(&sheet(&e)).is_empty());
    // Beyond the band, it goes with what it marks: the band 60 to 80 takes 14 out.
    run(&mut e, "drw.break", json!({ "view": v, "modifier": b, "from": 60, "to": 80 }));
    let s = on_sheet(&mut e, v, 90.0, 60.0);
    let (lo, hi) = extent(&of_mark(&sheet(&e)));
    assert!(lo.near(s - Vec2::new(7.0, 7.0), 1e-6) && hi.near(s + Vec2::new(7.0, 7.0), 1e-6), "{lo:?} {hi:?} {s:?}");
    run(&mut e, "drw.modifier.delete", json!({ "view": v, "modifier": b }));
    // A centre line by hand between the two holes: across a band it is broken as the view's lines are.
    let other = run(&mut e, "drw.pick", json!({ "view": v, "view_at": [35, 20] }));
    let line = run(&mut e, "drw.centerline", json!({ "view": v, "a": other, "b": hole }))["annotation"].as_u64().unwrap() as u32;
    let band = run(&mut e, "drw.break", json!({ "view": v, "lines": "vertical", "from": 50, "to": 70, "gap": 6 }))["modifier"].clone();
    let g = sheet(&e);
    let pieces: Vec<Vec<Vec2>> = g.strokes.iter().filter(|s| s.0 == Owner::Annotation(tenon_drawing::AnnotId(line))).map(|s| s.2.clone()).collect();
    assert_eq!(pieces.len(), 2, "{pieces:?}");
    // The sides of the band are 6 apart on the sheet: nothing of the line in there.
    let (s0, s1) = (on_sheet(&mut e, v, 50.0, 0.0).x, on_sheet(&mut e, v, 70.0, 0.0).x);
    near(s1 - s0, 6.0, "the gap");
    assert!(pieces.iter().flatten().all(|p| p.x <= s0 + 1e-6 || p.x >= s1 - 1e-6), "{pieces:?}");
    run(&mut e, "drw.modifier.delete", json!({ "view": v, "modifier": band }));

    // A hole table's tag: a through cut takes a hole away, and its tag; a shallow one leaves it, as
    // the hole goes on down below the floor.
    run(&mut e, "drw.hole_table", json!({ "view": v, "at": [260, 250] }));
    assert_eq!(written(&sheet(&e), "A1"), 2, "in the table and on the hole");
    let cut = run(&mut e, "drw.break_out", json!({ "view": v, "center": [30, 20], "radius": 15, "through": true }))["modifier"].clone();
    assert_eq!(written(&sheet(&e), "A1"), 1, "in the table");
    assert_eq!(written(&sheet(&e), "A2"), 2, "the other hole is as it was");
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": cut, "depth": 5 }));
    assert_eq!(written(&sheet(&e), "A1"), 2, "the hole goes on below the floor");
}

#[test]
fn a_cropped_detail_keeps_its_circle_inside_and_a_break_left_behind_says_so() {
    let dir = scratch("detail-crop");
    let mut e = Engine::new(kernel(), &dir);
    let v = plate_drawing(&mut e);
    // A detail of radius 20 about the hole at (30, 20), at 2:1, at (320, 60): its circle 280 to 360.
    let det = run(&mut e, "drw.view.detail", json!({ "parent": v, "center": [30, 20], "radius": 20, "scale": 2, "at": [320, 60] }))["view"]
        .as_u64()
        .unwrap();
    let (lo, hi) = extent(&strokes(&sheet(&e), det, Pen::Thin));
    assert!(lo.near(Vec2::new(280.0, 20.0), 0.02) && hi.near(Vec2::new(360.0, 100.0), 0.02), "{lo:?} {hi:?}");
    // Cropped to its left half, to x = 30: on the sheet to x = 320; so is what is left of the circle.
    run(&mut e, "drw.crop", json!({ "view": det, "rect": [[5, -5], [30, 45]], "outline": false }));
    let (lo, hi) = extent(&strokes(&sheet(&e), det, Pen::Thin));
    assert!(lo.near(Vec2::new(280.0, 20.0), 0.02) && hi.near(Vec2::new(320.0, 100.0), 0.02), "{lo:?} {hi:?}");
    run(&mut e, "drw.undo", json!({}));
    run(&mut e, "drw.view.delete", json!({ "view": det }));

    // A plate made 35 wide leaves a band 40 to 80 beyond its end: no lines stand out there, and the
    // view says what to do.
    let b = run(&mut e, "drw.break", json!({ "view": v, "lines": "vertical", "from": 40, "to": 80, "gap": 6 }))["modifier"].as_u64().unwrap();
    assert!(view_info(&mut e, v)["error"].is_null());
    let symbols = |g: &Graphics| g.strokes.iter().filter(|s| s.0 == Owner::Mod(ViewId(v as u32), b as u32)).count();
    assert_eq!(symbols(&sheet(&e)), 2);
    run(&mut e, "file.open", json!({ "path": "plate.tenon" }));
    run(&mut e, "param.set", json!({ "name": "w", "equation": "35" }));
    run(&mut e, "file.save", json!({ "path": "plate.tenon" }));
    run(&mut e, "drw.update", json!({}));
    assert_eq!(symbols(&sheet(&e)), 0);
    let i = view_info(&mut e, v);
    assert!(i["error"].as_str().is_some_and(|t| t.contains("Break 1") && t.contains("no longer crosses")), "{i}");
    // It can be suppressed as it stands, which ends the word; its lines do not have to cross anything.
    run(&mut e, "drw.break", json!({ "view": v, "modifier": b, "suppressed": true }));
    assert!(view_info(&mut e, v)["error"].is_null());
    run(&mut e, "drw.break", json!({ "view": v, "modifier": b, "suppressed": false }));
    // Back at 120 it is at work again and says nothing.
    run(&mut e, "file.open", json!({ "path": "plate.tenon" }));
    run(&mut e, "param.set", json!({ "name": "w", "equation": "120" }));
    run(&mut e, "file.save", json!({ "path": "plate.tenon" }));
    run(&mut e, "drw.update", json!({}));
    assert!(view_info(&mut e, v)["error"].is_null());
    assert_eq!(symbols(&sheet(&e)), 2);
}

#[test]
fn a_break_out_goes_down_from_a_curved_face_and_from_the_top_of_an_assembly() {
    let dir = scratch("break-out-round");
    let mut e = Engine::new(kernel(), &dir);
    // A round bar 40 across and 60 long, its axis up Z, seen from the front. Its nearest is the line
    // down its front, 20 from the axis. 5 down, the floor cuts it 2 root(400 - 15 x 15) = 26.46 wide.
    run(&mut e, "file.new", json!({ "name": "Bar" }));
    let sk = run(&mut e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(&mut e, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 20 }));
    run(&mut e, "model.extrude", json!({ "sketch": sk, "distance": 60 }));
    run(&mut e, "file.save", json!({ "path": "bar.tenon" }));
    run(&mut e, "drw.new", json!({ "name": "Bar" }));
    let v =
        run(&mut e, "drw.view.base", json!({ "model": "bar.tenon", "orientation": "front", "scale": 1, "at": [150, 150], "hidden": false }))["view"]
            .as_u64()
            .unwrap();
    // 40 long, wider than the bar.
    run(&mut e, "drw.break_out", json!({ "view": v, "rect": [[-30, 10], [30, 50]], "depth": 5 }));
    assert!(view_info(&mut e, v)["error"].is_null());
    hatched(&sheet(&e), v, 40.0 * 2.0 * (400.0_f64 - 225.0).sqrt(), "5 down from the front of the bar");

    // An assembly: the base's top at 10, the arm on it from 10 to 15 (x 15 to 75, y 15 to 25), the cube
    // on it from 10 to 20 (x 60 to 80). From above the nearest is the cube's top at 20. 7 down is 13:
    // through the arm's top only, 20 x 6 of it, the parts are placed and not at their origins.
    let _ = pivot(&mut e);
    run(&mut e, "drw.new", json!({ "name": "Pivot" }));
    let top = run(&mut e, "drw.view.base", json!({ "model": "pivot.tenonasm", "orientation": "top", "scale": 1, "at": [150, 150], "hidden": false }))
        ["view"]
        .as_u64()
        .unwrap();
    let m = run(&mut e, "drw.break_out", json!({ "view": top, "rect": [[30, 17], [50, 23]], "depth": 7 }))["modifier"].clone();
    assert!(view_info(&mut e, top)["error"].is_null());
    hatched(&sheet(&e), top, 120.0, "7 down, in the arm");
    // 5 down is 15: level with the arm's top, which is no cut face, and it takes nothing away.
    run(&mut e, "drw.break_out", json!({ "view": top, "modifier": m, "depth": 5 }));
    assert!(view_info(&mut e, top)["error"].as_str().is_some_and(|t| t.contains("cuts nothing")));
    assert_eq!(hatch_length(&sheet(&e), top), 0.0);
    // 12 down is 8: through the arm, into the base.
    run(&mut e, "drw.break_out", json!({ "view": top, "modifier": m, "depth": 12 }));
    assert!(view_info(&mut e, top)["error"].is_null());
    hatched(&sheet(&e), top, 120.0, "12 down, in the base");
}

#[test]
fn a_break_out_can_be_moved_over_what_it_cut_away_and_a_mark_stays_on_a_hole_left_below_it() {
    let dir = scratch("break-out-moved");
    let mut e = Engine::new(kernel(), &dir);
    let v = plate_drawing(&mut e);
    // Through, all of the plate above y = 40: the view's lines stop there, the model does not.
    let m = run(&mut e, "drw.break_out", json!({ "view": v, "rect": [[-5, 40], [125, 85]], "through": true }))["modifier"].clone();
    // Moved to where it has cut already: not off the view, as the view is the model's.
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": m, "rect": [[10, 50], [50, 70]] }));
    assert!(view_info(&mut e, v)["error"].is_null());
    run(&mut e, "drw.modifier.delete", json!({ "view": v, "modifier": m }));

    // A centre mark by hand on the hole at (90, 60). A cut 5 deep takes the circle it is on off the top;
    // the hole goes on below the floor, so the mark stays. Through, there is no hole there.
    let hole = run(&mut e, "drw.pick", json!({ "view": v, "view_at": [95, 60] }));
    let mark = run(&mut e, "drw.center_mark", json!({ "view": v, "a": hole }))["annotation"].as_u64().unwrap() as u32;
    let drawn = |e: &Engine| sheet(e).strokes.iter().filter(|s| s.0 == Owner::Annotation(tenon_drawing::AnnotId(mark))).count();
    assert_eq!(drawn(&e), 2);
    let c = run(&mut e, "drw.break_out", json!({ "view": v, "center": [90, 60], "radius": 12, "depth": 5 }))["modifier"].clone();
    assert_eq!(drawn(&e), 2, "the hole goes on");
    run(&mut e, "drw.break_out", json!({ "view": v, "modifier": c, "through": true }));
    assert_eq!(drawn(&e), 0, "the hole is gone");
}

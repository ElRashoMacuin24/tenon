//! The standard parts library (M7): fasteners, bearings and structural sections, each a part
//! built here by the same commands a user gives, fully dimensioned, with a design table of its
//! standard sizes. Nothing is read from a file or a catalogue: the sizes are the boundary
//! dimensions the standards publish (facts, typed in below; see ATTRIBUTION.md), and the shapes
//! are Tenon's own simple ones. A size is a row of the part's design table, so an assembly
//! places "M8x30" of the one hex screw file (DEC-048).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::Session;
use crate::document::Document;
use crate::expr;

/// One kind of standard part.
#[derive(Clone, Copy, Debug)]
pub struct Family {
    /// A stable identifier, also the part file's name: "hex-screw-iso-4017".
    pub id: &'static str,
    pub name: &'static str,
    /// "Fasteners", "Bearings" or "Sections".
    pub category: &'static str,
    /// The standard the sizes follow, or "" for Tenon's own generic shape.
    pub standard: &'static str,
    /// What the part is and what is simplified about it.
    pub about: &'static str,
    /// The parameters a size sets, in the order of a row's values.
    pub columns: &'static [&'static str],
    rows: fn() -> Vec<(String, Vec<f64>)>,
    build: fn(&mut Build) -> Result<(), String>,
}

impl Family {
    /// The sizes: each a name and a value for every column.
    pub fn rows(&self) -> Vec<(String, Vec<f64>)> {
        (self.rows)()
    }
    /// The names of the sizes.
    pub fn sizes(&self) -> Vec<String> {
        self.rows().into_iter().map(|r| r.0).collect()
    }
}

// ---- sizes ------------------------------------------------------------------------------------

/// ISO metric coarse threads M3 to M20 with their hexagon and socket head sizes.
/// (d, pitch, hex across flats s, hex head height k, nut height m, socket head diameter dk,
/// socket size, socket depth, washer inner d1, washer outer d2, washer thickness h)
const METRIC: [(f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64); 9] = [
    (3.0, 0.5, 5.5, 2.0, 2.4, 5.5, 2.5, 1.3, 3.2, 7.0, 0.5),
    (4.0, 0.7, 7.0, 2.8, 3.2, 7.0, 3.0, 2.0, 4.3, 9.0, 0.8),
    (5.0, 0.8, 8.0, 3.5, 4.7, 8.5, 4.0, 2.5, 5.3, 10.0, 1.0),
    (6.0, 1.0, 10.0, 4.0, 5.2, 10.0, 5.0, 3.0, 6.4, 12.0, 1.6),
    (8.0, 1.25, 13.0, 5.3, 6.8, 13.0, 6.0, 4.0, 8.4, 16.0, 1.6),
    (10.0, 1.5, 16.0, 6.4, 8.4, 16.0, 8.0, 5.0, 10.5, 20.0, 2.0),
    (12.0, 1.75, 18.0, 7.5, 10.8, 18.0, 10.0, 6.0, 13.0, 24.0, 2.5),
    (16.0, 2.0, 24.0, 10.0, 14.8, 24.0, 14.0, 8.0, 17.0, 30.0, 3.0),
    (20.0, 2.5, 30.0, 12.5, 18.0, 30.0, 17.0, 10.0, 21.0, 37.0, 3.0),
];

/// Preferred screw lengths, mm: eight for each diameter, from about twice the diameter up.
const LENGTHS: [f64; 16] = [6.0, 8.0, 10.0, 12.0, 16.0, 20.0, 25.0, 30.0, 40.0, 50.0, 60.0, 80.0, 100.0, 120.0, 150.0, 200.0];

fn lengths_for(d: f64) -> impl Iterator<Item = f64> {
    LENGTHS.into_iter().filter(move |l| *l >= 2.0 * d).take(8)
}

fn number(x: f64) -> String {
    let s = format!("{x:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn hex_screw_rows() -> Vec<(String, Vec<f64>)> {
    METRIC.iter().flat_map(|m| lengths_for(m.0).map(move |l| (format!("M{}x{}", number(m.0), number(l)), vec![m.0, m.2, m.3, l]))).collect()
}

fn socket_screw_rows() -> Vec<(String, Vec<f64>)> {
    METRIC.iter().flat_map(|m| lengths_for(m.0).map(move |l| (format!("M{}x{}", number(m.0), number(l)), vec![m.0, m.5, m.6, m.7, l]))).collect()
}

fn nut_rows() -> Vec<(String, Vec<f64>)> {
    METRIC.iter().map(|m| (format!("M{}", number(m.0)), vec![m.0, m.1, m.2, m.4])).collect()
}

fn washer_rows() -> Vec<(String, Vec<f64>)> {
    METRIC.iter().map(|m| (format!("M{}", number(m.0)), vec![m.8, m.9, m.10])).collect()
}

/// Single-row deep groove ball bearings: designation, bore d, outside diameter D, width B.
const BEARINGS: [(&str, f64, f64, f64); 17] = [
    ("623", 3.0, 10.0, 4.0),
    ("624", 4.0, 13.0, 5.0),
    ("625", 5.0, 16.0, 5.0),
    ("626", 6.0, 19.0, 6.0),
    ("608", 8.0, 22.0, 7.0),
    ("6000", 10.0, 26.0, 8.0),
    ("6001", 12.0, 28.0, 8.0),
    ("6002", 15.0, 32.0, 9.0),
    ("6003", 17.0, 35.0, 10.0),
    ("6004", 20.0, 42.0, 12.0),
    ("6005", 25.0, 47.0, 12.0),
    ("6200", 10.0, 30.0, 9.0),
    ("6201", 12.0, 32.0, 10.0),
    ("6202", 15.0, 35.0, 11.0),
    ("6203", 17.0, 40.0, 12.0),
    ("6204", 20.0, 47.0, 14.0),
    ("6205", 25.0, 52.0, 15.0),
];

fn bearing_rows() -> Vec<(String, Vec<f64>)> {
    BEARINGS.iter().map(|b| (b.0.to_owned(), vec![b.1, b.2, b.3])).collect()
}

fn named(sizes: &[&[f64]]) -> Vec<(String, Vec<f64>)> {
    sizes.iter().map(|s| (s.iter().map(|x| number(*x)).collect::<Vec<_>>().join("x"), s.to_vec())).collect()
}

fn rect_tube_rows() -> Vec<(String, Vec<f64>)> {
    named(&[
        &[20.0, 20.0, 2.0],
        &[25.0, 25.0, 2.0],
        &[30.0, 30.0, 2.0],
        &[40.0, 40.0, 2.0],
        &[40.0, 40.0, 3.0],
        &[50.0, 50.0, 3.0],
        &[30.0, 20.0, 2.0],
        &[40.0, 20.0, 2.0],
        &[50.0, 30.0, 3.0],
        &[60.0, 40.0, 3.0],
    ])
}

fn round_tube_rows() -> Vec<(String, Vec<f64>)> {
    named(&[&[10.0, 1.0], &[12.0, 1.0], &[16.0, 1.5], &[20.0, 2.0], &[25.0, 2.0], &[30.0, 2.0], &[40.0, 3.0]])
}

fn angle_rows() -> Vec<(String, Vec<f64>)> {
    named(&[
        &[20.0, 20.0, 3.0],
        &[25.0, 25.0, 3.0],
        &[30.0, 30.0, 3.0],
        &[40.0, 40.0, 4.0],
        &[50.0, 50.0, 5.0],
        &[40.0, 20.0, 3.0],
        &[60.0, 30.0, 5.0],
    ])
}

fn flat_bar_rows() -> Vec<(String, Vec<f64>)> {
    named(&[&[20.0, 3.0], &[25.0, 5.0], &[30.0, 5.0], &[40.0, 5.0], &[50.0, 6.0], &[60.0, 8.0]])
}

/// Tenon's own simplified T-slot sections: side a, slot opening w, lip e, cavity width c, cavity
/// depth g, centre bore b. The cavities are rectangles kept clear of one another (half a cavity's
/// width is less than what is left of the half side under the lip and the cavity), so a web
/// joins each corner to the core.
fn t_slot_rows() -> Vec<(String, Vec<f64>)> {
    [(20.0, 6.0, 1.8, 8.0, 3.0, 4.2), (30.0, 8.0, 2.0, 13.0, 5.5, 6.8), (40.0, 8.0, 4.5, 16.0, 6.5, 6.8)]
        .iter()
        .map(|s| (format!("{}x{} slot {}", number(s.0), number(s.0), number(s.1)), vec![s.0, s.1, s.2, s.3, s.4, s.5]))
        .collect()
}

// ---- building ---------------------------------------------------------------------------------

/// A part being built by commands.
pub struct Build {
    s: Session,
}

impl Build {
    fn run(&mut self, id: &str, p: Value) -> Result<Value, String> {
        self.s.exec(id, &p, None).map_err(|e| format!("{id}: {}", e.0))
    }

    /// A user parameter, in millimetres.
    fn param(&mut self, name: &str, value: f64, comment: &str) -> Result<(), String> {
        self.run("param.add", json!({ "name": name, "equation": format!("{} mm", number(value)), "comment": comment })).map(|_| ())
    }

    /// What `equation` comes to now.
    fn value(&self, equation: &str) -> Result<f64, String> {
        let env: BTreeMap<String, f64> = self.s.document().parameter_values();
        expr::parse(equation).and_then(|e| e.eval(&env)).map_err(|e| format!("`{equation}`: {e}"))
    }

    /// A sketch on an origin plane ("xy") or a work plane; returns it and its origin point.
    fn sketch(&mut self, on: Value) -> Result<(u64, u64), String> {
        let p = match on {
            Value::String(plane) => json!({ "plane": plane }),
            work => json!({ "work_plane": work }),
        };
        let r = self.run("sketch.create", p)?;
        Ok((id(&r, "feature")?, id(&r, "origin")?))
    }

    /// A geometric constraint.
    fn hold(&mut self, sketch: u64, constraint: Value) -> Result<(), String> {
        self.run("sketch.constrain", json!({ "sketch": sketch, "constraint": constraint })).map(|_| ())
    }

    /// A dimension that follows `equation` (the geometry is drawn at that size already).
    fn dim(&mut self, sketch: u64, mut constraint: Value, equation: &str) -> Result<(), String> {
        constraint["value"] = json!(self.value(equation)?);
        self.run("sketch.constrain", json!({ "sketch": sketch, "constraint": constraint, "equation": equation })).map(|_| ())
    }

    /// A circle about `center` (a point) whose diameter follows `equation`.
    fn circle(&mut self, sketch: u64, center: u64, equation: &str) -> Result<u64, String> {
        let r = self.value(equation)? / 2.0;
        let c = id(&self.run("sketch.circle", json!({ "sketch": sketch, "center": center, "r": r }))?, "circle")?;
        self.dim(sketch, json!({ "type": "diameter", "curve": c }), equation)?;
        Ok(c)
    }

    /// A rectangle `w` by `h` (equations) about `origin`, fully dimensioned; returns its lines
    /// (bottom, right, top, left).
    fn rectangle(&mut self, sketch: u64, origin: u64, w: &str, h: &str) -> Result<Vec<u64>, String> {
        let (wv, hv) = (self.value(w)?, self.value(h)?);
        let r = self.run("sketch.rectangle", json!({ "sketch": sketch, "x1": -wv / 2.0, "y1": -hv / 2.0, "x2": wv / 2.0, "y2": hv / 2.0 }))?;
        let (lines, corner) = (ids(&r, "lines")?, ids(&r, "corners")?[0]);
        self.dim(sketch, json!({ "type": "horizontal_distance", "a": origin, "b": corner }), &format!("-({w}) / 2"))?;
        self.dim(sketch, json!({ "type": "vertical_distance", "a": origin, "b": corner }), &format!("-({h}) / 2"))?;
        self.dim(sketch, json!({ "type": "length", "line": lines[0] }), w)?;
        self.dim(sketch, json!({ "type": "length", "line": lines[1] }), h)?;
        Ok(lines)
    }

    /// A regular hexagon about `origin`, `across` (an equation) between opposite sides, with a
    /// side level at the top.
    fn hexagon(&mut self, sketch: u64, origin: u64, across: &str) -> Result<(), String> {
        let circumradius = self.value(across)? / 3f64.sqrt();
        let lines = ids(&self.run("sketch.polygon", json!({ "sketch": sketch, "cx": 0, "cy": 0, "x": circumradius, "y": 0, "sides": 6 }))?, "lines")?;
        // The polygon's corners lie on a construction circle about its own centre point.
        let info = self.run("sketch.info", json!({ "sketch": sketch }))?;
        let ring = info["entities"]
            .as_array()
            .into_iter()
            .flatten()
            .rfind(|e| e["type"] == "circle" && e["construction"] == true)
            .ok_or("the polygon has no circle")?
            .clone();
        self.hold(sketch, json!({ "type": "coincident", "a": id(&ring, "center")?, "b": origin }))?;
        self.hold(sketch, json!({ "type": "horizontal", "line": lines[1] }))?;
        self.dim(sketch, json!({ "type": "diameter", "curve": id(&ring, "id")? }), &format!("({across}) * 2 / sqrt(3)"))
    }

    /// An extrusion of `sketch` by `distance` (an equation).
    fn extrude(&mut self, sketch: u64, distance: &str, reverse: bool, operation: &str) -> Result<u64, String> {
        let d = self.value(distance)?;
        let p = json!({ "sketch": sketch, "distance": d, "reverse": reverse, "operation": operation, "equations": { "/extent/distance": distance } });
        id(&self.run("model.extrude", p)?, "feature")
    }

    /// A cosmetic thread on the round wall `curve` of `feature` made.
    fn thread(&mut self, feature: u64, curve: u64) -> Result<(), String> {
        // (The face is found by its name; the rest only tells apart pieces of one face.)
        let face = json!({
            "origin": { "type": "side", "feature": feature, "curve": curve },
            "fingerprint": { "surface": "cylinder", "direction": { "x": 0.0, "y": 0.0, "z": 1.0 }, "centroid": { "x": 0.0, "y": 0.0, "z": 0.0 }, "area": 0.0 },
        });
        self.run("model.thread", json!({ "face": face })).map(|_| ())
    }
}

fn id(v: &Value, key: &str) -> Result<u64, String> {
    v[key].as_u64().ok_or_else(|| format!("no `{key}` in {v}"))
}

fn ids(v: &Value, key: &str) -> Result<Vec<u64>, String> {
    v[key]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_u64).collect::<Vec<_>>())
        .filter(|a| !a.is_empty())
        .ok_or_else(|| format!("no `{key}` in {v}"))
}

fn hex_screw(b: &mut Build) -> Result<(), String> {
    b.param("d", 8.0, "thread diameter")?;
    b.param("s", 13.0, "across the head's flats")?;
    b.param("k", 5.3, "head height")?;
    b.param("l", 30.0, "length under the head")?;
    let (head, o) = b.sketch(json!("xy"))?;
    b.hexagon(head, o, "s")?;
    b.extrude(head, "k", false, "join")?;
    let (shank, o) = b.sketch(json!("xy"))?;
    let c = b.circle(shank, o, "d")?;
    let x = b.extrude(shank, "l", true, "join")?;
    b.thread(x, c)
}

fn socket_screw(b: &mut Build) -> Result<(), String> {
    b.param("d", 8.0, "thread diameter")?;
    b.param("dk", 13.0, "head diameter")?;
    b.param("s", 6.0, "across the socket's flats")?;
    b.param("t", 4.0, "socket depth")?;
    b.param("l", 30.0, "length under the head")?;
    let (head, o) = b.sketch(json!("xy"))?;
    b.circle(head, o, "dk")?;
    // (The head is as high as the thread is wide.)
    b.extrude(head, "d", false, "join")?;
    let top = id(&b.run("work.plane", json!({ "base": "xy", "distance": 8.0, "equations": { "/distance": "d" } }))?, "feature")?;
    let (socket, o) = b.sketch(json!(top))?;
    b.hexagon(socket, o, "s")?;
    b.extrude(socket, "t", true, "cut")?;
    let (shank, o) = b.sketch(json!("xy"))?;
    let c = b.circle(shank, o, "d")?;
    let x = b.extrude(shank, "l", true, "join")?;
    b.thread(x, c)
}

fn hex_nut(b: &mut Build) -> Result<(), String> {
    b.param("d", 8.0, "thread diameter")?;
    b.param("p", 1.25, "thread pitch")?;
    b.param("s", 13.0, "across flats")?;
    b.param("height", 6.8, "height (m in the standard: a unit's name here)")?;
    let (sk, o) = b.sketch(json!("xy"))?;
    b.hexagon(sk, o, "s")?;
    // The hole is the thread's minor diameter: d less 1.0825 pitches.
    let hole = b.circle(sk, o, "d - 1.0825 * p")?;
    let x = b.extrude(sk, "height", false, "join")?;
    b.thread(x, hole)
}

fn washer(b: &mut Build) -> Result<(), String> {
    b.param("d1", 8.4, "hole")?;
    b.param("d2", 16.0, "outside diameter")?;
    b.param("h", 1.6, "thickness")?;
    let (sk, o) = b.sketch(json!("xy"))?;
    b.circle(sk, o, "d1")?;
    b.circle(sk, o, "d2")?;
    b.extrude(sk, "h", false, "join").map(|_| ())
}

fn bearing(b: &mut Build) -> Result<(), String> {
    b.param("d", 8.0, "bore")?;
    b.param("D", 22.0, "outside diameter")?;
    b.param("B", 7.0, "width")?;
    let (sk, o) = b.sketch(json!("xy"))?;
    b.circle(sk, o, "d")?;
    b.circle(sk, o, "D")?;
    b.extrude(sk, "B", false, "join")?;
    // Between the rings, each face is set in a little, where the cage or a shield shows.
    let far = id(&b.run("work.plane", json!({ "base": "xy", "distance": 7.0, "equations": { "/distance": "B" } }))?, "feature")?;
    for (on, reverse) in [(json!("xy"), false), (json!(far), true)] {
        let (recess, o) = b.sketch(on)?;
        b.circle(recess, o, "d + 0.3 * (D - d)")?;
        b.circle(recess, o, "D - 0.3 * (D - d)")?;
        b.extrude(recess, "0.08 * B", reverse, "cut")?;
    }
    Ok(())
}

fn rect_tube(b: &mut Build) -> Result<(), String> {
    b.param("w", 20.0, "width")?;
    b.param("h", 20.0, "height")?;
    b.param("t", 2.0, "wall thickness")?;
    b.param("L", 100.0, "length")?;
    let (sk, o) = b.sketch(json!("xy"))?;
    b.rectangle(sk, o, "w", "h")?;
    b.rectangle(sk, o, "w - 2 * t", "h - 2 * t")?;
    b.extrude(sk, "L", false, "join").map(|_| ())
}

fn round_tube(b: &mut Build) -> Result<(), String> {
    b.param("D", 20.0, "outside diameter")?;
    b.param("t", 2.0, "wall thickness")?;
    b.param("L", 100.0, "length")?;
    let (sk, o) = b.sketch(json!("xy"))?;
    b.circle(sk, o, "D")?;
    b.circle(sk, o, "D - 2 * t")?;
    b.extrude(sk, "L", false, "join").map(|_| ())
}

fn flat_bar(b: &mut Build) -> Result<(), String> {
    b.param("w", 20.0, "width")?;
    b.param("t", 3.0, "thickness")?;
    b.param("L", 100.0, "length")?;
    let (sk, o) = b.sketch(json!("xy"))?;
    b.rectangle(sk, o, "w", "t")?;
    b.extrude(sk, "L", false, "join").map(|_| ())
}

/// A closed chain of lines through `points`, each line level or upright as it is drawn; returns
/// the lines and the points' ids.
fn chain(b: &mut Build, sketch: u64, points: &[(f64, f64)]) -> Result<(Vec<u64>, Vec<u64>), String> {
    let (mut lines, mut nodes): (Vec<u64>, Vec<u64>) = (Vec::new(), Vec::new());
    for (i, (x, y)) in points.iter().enumerate() {
        let (nx, ny) = points[(i + 1) % points.len()];
        let mut p = json!({ "sketch": sketch });
        match nodes.last() {
            Some(start) => p["start"] = json!(start),
            None => {
                p["x1"] = json!(x);
                p["y1"] = json!(y);
            }
        }
        if i + 1 == points.len() {
            p["end"] = json!(nodes[0]);
        } else {
            p["x2"] = json!(nx);
            p["y2"] = json!(ny);
        }
        let r = b.run("sketch.line", p)?;
        if nodes.is_empty() {
            nodes.push(id(&r, "start")?);
        }
        if i + 1 < points.len() {
            nodes.push(id(&r, "end")?);
        }
        let line = id(&r, "line")?;
        let level = (ny - y).abs() < 1e-9;
        b.hold(sketch, json!({ "type": if level { "horizontal" } else { "vertical" }, "line": line }))?;
        lines.push(line);
    }
    Ok((lines, nodes))
}

fn angle(b: &mut Build) -> Result<(), String> {
    b.param("a", 20.0, "one leg")?;
    b.param("b", 20.0, "the other leg")?;
    b.param("t", 3.0, "thickness")?;
    b.param("L", 100.0, "length")?;
    let (sk, o) = b.sketch(json!("xy"))?;
    let (lines, nodes) = chain(b, sk, &[(0.0, 0.0), (20.0, 0.0), (20.0, 3.0), (3.0, 3.0), (3.0, 20.0), (0.0, 20.0)])?;
    b.hold(sk, json!({ "type": "coincident", "a": nodes[0], "b": o }))?;
    b.dim(sk, json!({ "type": "length", "line": lines[0] }), "a")?;
    b.dim(sk, json!({ "type": "length", "line": lines[1] }), "t")?;
    b.dim(sk, json!({ "type": "length", "line": lines[4] }), "t")?;
    b.dim(sk, json!({ "type": "length", "line": lines[5] }), "b")?;
    b.extrude(sk, "L", false, "join").map(|_| ())
}

fn t_slot(b: &mut Build) -> Result<(), String> {
    b.param("a", 20.0, "side")?;
    b.param("w", 6.0, "slot opening")?;
    b.param("e", 1.8, "lip thickness")?;
    b.param("c", 8.0, "cavity width")?;
    b.param("g", 3.0, "cavity depth")?;
    b.param("bore", 4.2, "centre bore")?;
    b.param("L", 100.0, "length")?;
    let (sk, o) = b.sketch(json!("xy"))?;
    b.rectangle(sk, o, "a", "a")?;
    b.extrude(sk, "L", false, "join")?;
    // One slot, in the top side: an opening through the lip, then the wider cavity. The other
    // three are copies of it round the bar's axis.
    let (slot, o) = b.sketch(json!("xy"))?;
    let (lines, nodes) = chain(b, slot, &[(-3.0, 10.0), (3.0, 10.0), (3.0, 8.2), (4.0, 8.2), (4.0, 5.2), (-4.0, 5.2), (-4.0, 8.2), (-3.0, 8.2)])?;
    b.dim(slot, json!({ "type": "horizontal_distance", "a": o, "b": nodes[0] }), "-w / 2")?;
    b.dim(slot, json!({ "type": "vertical_distance", "a": o, "b": nodes[0] }), "a / 2")?;
    b.dim(slot, json!({ "type": "length", "line": lines[0] }), "w")?;
    b.dim(slot, json!({ "type": "length", "line": lines[1] }), "e")?;
    b.dim(slot, json!({ "type": "horizontal_distance", "a": o, "b": nodes[3] }), "c / 2")?;
    b.dim(slot, json!({ "type": "length", "line": lines[3] }), "g")?;
    b.dim(slot, json!({ "type": "length", "line": lines[4] }), "c")?;
    // (Both sides of the lip end at one level.)
    b.hold(slot, json!({ "type": "point_on_curve", "point": nodes[6], "curve": lines[2] }))?;
    let cut = id(&b.run("model.extrude", json!({ "sketch": slot, "through_all": true, "operation": "cut" }))?, "feature")?;
    b.run("model.pattern.circular", json!({ "features": [cut], "axis": "z", "count": 4 }))?;
    let (hole, o) = b.sketch(json!("xy"))?;
    b.circle(hole, o, "bore")?;
    b.run("model.extrude", json!({ "sketch": hole, "through_all": true, "operation": "cut" }))?;
    Ok(())
}

// ---- the catalogue ----------------------------------------------------------------------------

const FAMILIES: &[Family] = &[
    Family {
        id: "hex-screw-iso-4017",
        name: "Hex Head Screw",
        category: "Fasteners",
        standard: "ISO 4017",
        about: "Hexagon head screw threaded to the head, coarse thread, M3 to M20. The thread is cosmetic; the head has no chamfer or washer face.",
        columns: &["d", "s", "k", "l"],
        rows: hex_screw_rows,
        build: hex_screw,
    },
    Family {
        id: "socket-screw-iso-4762",
        name: "Socket Head Cap Screw",
        category: "Fasteners",
        standard: "ISO 4762",
        about: "Hexagon socket head cap screw, coarse thread, M3 to M20. The thread is cosmetic and runs the whole length.",
        columns: &["d", "dk", "s", "t", "l"],
        rows: socket_screw_rows,
        build: socket_screw,
    },
    Family {
        id: "hex-nut-iso-4032",
        name: "Hex Nut",
        category: "Fasteners",
        standard: "ISO 4032",
        about: "Hexagon nut, style 1, coarse thread, M3 to M20. The thread is cosmetic; the corners are not chamfered.",
        columns: &["d", "p", "s", "height"],
        rows: nut_rows,
        build: hex_nut,
    },
    Family {
        id: "washer-iso-7089",
        name: "Plain Washer",
        category: "Fasteners",
        standard: "ISO 7089",
        about: "Plain washer, normal series, for M3 to M20.",
        columns: &["d1", "d2", "h"],
        rows: washer_rows,
        build: washer,
    },
    Family {
        id: "ball-bearing-iso-15",
        name: "Deep Groove Ball Bearing",
        category: "Bearings",
        standard: "ISO 15",
        about: "Single-row deep groove ball bearing by its boundary dimensions (bore, outside diameter, width). One solid: the rings are not separate and there are no balls.",
        columns: &["d", "D", "B"],
        rows: bearing_rows,
        build: bearing,
    },
    Family {
        id: "tube-rectangular",
        name: "Square and Rectangular Tube",
        category: "Sections",
        standard: "",
        about: "Hollow section with square corners, by width, height and wall. Its length is the parameter L.",
        columns: &["w", "h", "t"],
        rows: rect_tube_rows,
        build: rect_tube,
    },
    Family {
        id: "tube-round",
        name: "Round Tube",
        category: "Sections",
        standard: "",
        about: "Round tube by outside diameter and wall. Its length is the parameter L.",
        columns: &["D", "t"],
        rows: round_tube_rows,
        build: round_tube,
    },
    Family {
        id: "angle",
        name: "Angle",
        category: "Sections",
        standard: "",
        about: "L section with square corners, by its two legs and thickness. Its length is the parameter L.",
        columns: &["a", "b", "t"],
        rows: angle_rows,
        build: angle,
    },
    Family {
        id: "flat-bar",
        name: "Flat Bar",
        category: "Sections",
        standard: "",
        about: "Flat bar by width and thickness. Its length is the parameter L.",
        columns: &["w", "t"],
        rows: flat_bar_rows,
        build: flat_bar,
    },
    Family {
        id: "t-slot",
        name: "T-Slot Profile",
        category: "Sections",
        standard: "",
        about: "A generic T-slot framing profile with one slot a side and a centre bore. Tenon's own simplified section, not a maker's drawing: check the supplier's before relying on a fit. Its length is the parameter L.",
        columns: &["a", "w", "e", "c", "g", "bore"],
        rows: t_slot_rows,
        build: t_slot,
    },
];

/// Every kind of standard part.
pub fn families() -> &'static [Family] {
    FAMILIES
}

/// The family with this id.
pub fn family(id: &str) -> Option<&'static Family> {
    FAMILIES.iter().find(|f| f.id == id)
}

/// The material a family's part is made of.
fn material(f: &Family) -> &'static str {
    if f.category == "Sections" { "Aluminium 6061" } else { "Steel, mild" }
}

/// The part of family `id`: a document with a design table of every size in the standard's
/// order, at the size it is drawn at (a middling one: an M8 screw, a 608 bearing).
pub fn build(id: &str) -> Result<Document, String> {
    let f = family(id)
        .ok_or_else(|| format!("there is no standard part `{id}`: there are {}", FAMILIES.iter().map(|f| f.id).collect::<Vec<_>>().join(", ")))?;
    let mut b = Build { s: Session::default() };
    let made = (|| -> Result<(), String> {
        b.run("document.rename", json!({ "name": f.name }))?;
        (f.build)(&mut b)?;
        b.run("document.material", json!({ "name": material(f) }))?;
        // The sizes: the part was drawn at one of them, which starts the table; the others are
        // added, and the rows put in the standard's order below.
        let rows = f.rows();
        let now = b.s.document().parameter_values();
        let drawn = rows
            .iter()
            .position(|r| f.columns.iter().zip(&r.1).all(|(c, v)| now.get(*c).is_some_and(|x| (x - v).abs() < 1e-9)))
            .ok_or("the part was not drawn at one of its sizes")?;
        b.run("table.create", json!({ "columns": f.columns, "row": rows[drawn].0 }))?;
        for (name, values) in rows.iter().enumerate().filter(|(i, _)| *i != drawn).map(|(_, r)| r) {
            let set: serde_json::Map<String, Value> = f.columns.iter().zip(values).map(|(c, v)| ((*c).to_owned(), json!(v))).collect();
            b.run("table.add_row", json!({ "name": name, "values": set }))?;
        }
        Ok(())
    })();
    made.map_err(|e| format!("{}: {e}", f.name))?;
    let mut doc = b.s.document().clone();
    if let Some(t) = doc.table.as_mut() {
        let order: Vec<String> = f.sizes();
        t.rows.sort_by_key(|r| order.iter().position(|n| *n == r.name));
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn every_family_builds_fully_dimensioned_with_a_table_of_its_sizes() {
        let mut ids = std::collections::BTreeSet::new();
        for f in families() {
            assert!(ids.insert(f.id), "{} is listed twice", f.id);
            assert!(f.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "{} is a file name", f.id);
            let doc = build(f.id).unwrap_or_else(|e| panic!("{e}"));
            doc.validate().unwrap_or_else(|e| panic!("{}: {e}", f.id));
            assert_eq!(doc.name, f.name);
            // Every sketch is held completely: a size changes it one way only.
            for feature in doc.features() {
                if let Some(sk) = doc.sketch(feature.id) {
                    assert_eq!(sk.dof().unwrap().dof, 0, "{} {} is not fully dimensioned", f.id, feature.name);
                }
            }
            let table = doc.table().unwrap_or_else(|| panic!("{} has no table", f.id));
            assert_eq!(table.columns, f.columns);
            let rows = f.rows();
            assert!(!rows.is_empty() && table.rows.len() == rows.len(), "{}", f.id);
            let mut names = std::collections::BTreeSet::new();
            for (name, values) in &rows {
                assert!(names.insert(name.clone()), "{} has two sizes named {name}", f.id);
                assert_eq!(values.len(), f.columns.len(), "{} {name}", f.id);
                assert!(values.iter().all(|v| *v > 0.0), "{} {name}", f.id);
                assert_eq!(&table.row(name).unwrap().values, values, "{} {name}", f.id);
            }
            assert!(doc.material().is_some(), "{}", f.id);
            // The table reads as the standard does (M3 before M10, not as names sort), though
            // the part is drawn at a size from the middle of it, which is the active row.
            let listed: Vec<&str> = table.rows.iter().map(|r| r.name.as_str()).collect();
            assert_eq!(listed, rows.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), "{}", f.id);
            assert!(table.row(&table.active).is_some(), "{}: the active row {} is one of them", f.id, table.active);
        }
        let screws = family("hex-screw-iso-4017").unwrap().sizes();
        let at = |name: &str| screws.iter().position(|s| s == name).unwrap_or_else(|| panic!("no {name}"));
        assert!(at("M3x6") == 0 && at("M8x30") < at("M10x20") && at("M10x100") < at("M12x25"), "{screws:?}");
        assert_eq!(build("hex-screw-iso-4017").unwrap().table().unwrap().active, "M8x30");
        assert!(build("left-handed-widget").unwrap_err().contains("there is no standard part"));
    }

    #[test]
    fn the_sizes_are_the_standards() {
        // A few sizes anyone can check against a bolt chart or a bearing's box.
        let size = |id: &str, name: &str| family(id).unwrap().rows().into_iter().find(|r| r.0 == name).unwrap_or_else(|| panic!("no {name}")).1;
        assert_eq!(size("hex-screw-iso-4017", "M8x30"), [8.0, 13.0, 5.3, 30.0]);
        assert_eq!(size("hex-screw-iso-4017", "M6x12"), [6.0, 10.0, 4.0, 12.0]);
        assert_eq!(size("socket-screw-iso-4762", "M5x20"), [5.0, 8.5, 4.0, 2.5, 20.0]);
        assert_eq!(size("hex-nut-iso-4032", "M10"), [10.0, 1.5, 16.0, 8.4]);
        assert_eq!(size("washer-iso-7089", "M6"), [6.4, 12.0, 1.6]);
        assert_eq!(size("ball-bearing-iso-15", "608"), [8.0, 22.0, 7.0]);
        assert_eq!(size("ball-bearing-iso-15", "6204"), [20.0, 47.0, 14.0]);
        assert_eq!(family("hex-screw-iso-4017").unwrap().sizes().len(), 72);
        // No screw is shorter than twice its diameter.
        assert!(family("hex-screw-iso-4017").unwrap().rows().iter().all(|r| r.1[3] >= 2.0 * r.1[0]));
    }
}

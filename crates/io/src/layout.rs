//! Where a document's fields go in a version-2 file (docs/file-format.md), and back.
//!
//! In memory (and in version-1 files) a document is `{format, version, <body>: {...}}`. A
//! version-2 file puts the body's fields at the top, writes each list of things as records under
//! a singular name (`features` as `[[feature]]`), flattens each record's `kind` into the record,
//! writes sketch entities and constraints as records with their ids, and rounds computed values
//! (solved sketch positions and component placements, reference fingerprints) to
//! `tol::FILE_DECIMALS`.

use serde_json::{Map, Value};
use tenon_geom::tol;

/// The layout of one kind of document.
pub(crate) struct Shape {
    /// "part", "assembly" or "drawing", for messages.
    pub what: &'static str,
    /// The field of a version-1 file holding the document.
    pub body: &'static str,
    /// The document's fields in file order: name in memory, name in the file.
    pub fields: &'static [(&'static str, &'static str)],
    /// Lists whose records have their `kind` flattened into them, with each record's own fields
    /// (everything else in a record belongs to its kind).
    pub kinds: &'static [(&'static str, &'static [&'static str])],
    /// Computed fields of records, rounded: (list, field).
    pub computed: &'static [(&'static str, &'static str)],
    /// Objects written as sub-tables, by path in the file (`sheet.title_block`).
    pub tables: &'static [&'static str],
}

pub(crate) const PART: Shape = Shape {
    what: "part",
    body: "document",
    fields: &[
        ("name", "name"),
        ("next_feature", "next_feature"),
        ("end_before", "end_before"),
        ("appearance", "appearance"),
        ("material", "material"),
        ("table", "table"),
        ("params", "parameters"),
        ("features", "feature"),
    ],
    kinds: &[("features", &["id", "name", "suppressed"])],
    computed: &[],
    tables: &[],
};

pub(crate) const ASSEMBLY: Shape = Shape {
    what: "assembly",
    body: "assembly",
    fields: &[
        ("name", "name"),
        ("next_component", "next_component"),
        ("next_relationship", "next_relationship"),
        ("components", "component"),
        ("relationships", "relationship"),
        ("explode", "explode"),
    ],
    kinds: &[("relationships", &["id", "name", "suppressed"])],
    computed: &[("components", "placement")],
    tables: &[],
};

pub(crate) const DRAWING: Shape = Shape {
    what: "drawing",
    body: "drawing",
    fields: &[
        ("name", "name"),
        ("standard", "standard"),
        ("next_sheet", "next_sheet"),
        ("next_view", "next_view"),
        ("next_annotation", "next_annotation"),
        ("props", "props"),
        ("sheets", "sheet"),
        ("views", "view"),
        ("annotations", "annotation"),
    ],
    kinds: &[("annotations", &["id"])],
    computed: &[],
    tables: &["sheet.title_block"],
};

/// Fields a version-1 file has beside the document; they are not kept.
const HEAD: &[&str] = &["format", "version", "generator"];
/// A sketch entity's own fields; the rest are its geometry's.
const ENTITY_OWN: &[&str] = &["construction"];
/// A sketch's fields, in file order.
const SKETCH: &[&str] = &["next_entity", "next_constraint", "entities", "constraints", "places"];

impl Shape {
    /// The file's name for a list (`"features"` gives `"feature"`).
    pub(crate) fn file_name(&self, field: &str) -> &'static str {
        self.fields.iter().find(|(m, _)| *m == field).map_or("", |(_, f)| *f)
    }
}

/// The version-2 file fields of a document held as `head` (`{format, version, <body>, extra
/// fields...}`), written as `version`. Unknown fields of `head` follow, sorted.
pub(crate) fn to_file(head: &Map<String, Value>, shape: &Shape, version: u32) -> Result<Map<String, Value>, String> {
    let body = head.get(shape.body).and_then(Value::as_object).ok_or_else(|| format!("no {} to write", shape.what))?;
    if let Some(k) = body.keys().find(|k| !shape.fields.iter().any(|(m, _)| m == k)) {
        return Err(format!("the {} field `{k}` has no place in the file", shape.what));
    }
    let mut out = Map::new();
    out.insert("format".into(), head.get("format").cloned().unwrap_or(Value::Null));
    out.insert("version".into(), Value::from(version));
    for (mem, name) in shape.fields {
        let Some(v) = body.get(*mem) else { continue };
        let mut v = v.clone();
        if let Some(own) = shape.kinds.iter().find(|(l, _)| l == mem).map(|(_, o)| *o) {
            for r in v.as_array_mut().into_iter().flatten() {
                *r = flatten(r, own).map_err(|e| format!("{name}: {e}"))?;
                if shape.what == "part" {
                    sketch_out(r).map_err(|e| format!("{name}: {e}"))?;
                }
            }
        }
        for (_, field) in shape.computed.iter().filter(|(l, _)| l == mem) {
            for r in v.as_array_mut().into_iter().flatten() {
                if let Some(x) = r.get_mut(*field) {
                    round(x);
                }
            }
        }
        if *mem == "params" {
            v = Value::Object(first(&v, &["next", "user", "model"]));
        }
        out.insert((*name).into(), v);
    }
    let mut extra: Vec<(&String, &Value)> =
        head.iter().filter(|(k, _)| !HEAD.contains(&k.as_str()) && k.as_str() != shape.body && !out.contains_key(*k)).collect();
    extra.sort_by(|a, b| a.0.cmp(b.0));
    out.extend(extra.into_iter().map(|(k, v)| (k.clone(), v.clone())));
    let mut file = Value::Object(out);
    round_fingerprints(&mut file);
    source_keys(&mut file, true);
    match file {
        Value::Object(o) => Ok(o),
        _ => Err("not an object".into()),
    }
}

/// The document held by version-2 file fields, as `{format, version, <body>, unknown fields...}`.
pub(crate) fn from_file(file: Map<String, Value>, shape: &Shape) -> Map<String, Value> {
    let mut head = Map::new();
    let mut body = Map::new();
    for (k, mut v) in file {
        source_keys(&mut v, false);
        if k == "format" || k == "version" {
            head.insert(k, v);
            continue;
        }
        let Some((mem, _)) = shape.fields.iter().find(|(_, f)| *f == k) else {
            head.insert(k, v);
            continue;
        };
        let mut v = v;
        if let Some(own) = shape.kinds.iter().find(|(l, _)| l == mem).map(|(_, o)| *o) {
            for r in v.as_array_mut().into_iter().flatten() {
                if shape.what == "part" {
                    sketch_in(r);
                }
                *r = unflatten(r, own);
            }
        }
        body.insert((*mem).into(), v);
    }
    head.insert(shape.body.into(), Value::Object(body));
    head
}

/// A record with the fields of its `kind` in place of it.
fn flatten(r: &Value, own: &[&str]) -> Result<Value, String> {
    let r = r.as_object().ok_or("a record is not an object")?;
    let id = r.get("id").cloned().unwrap_or(Value::Null);
    let mut out = Map::new();
    for (k, v) in r {
        if k == "kind" {
            for (kk, vv) in v.as_object().ok_or("a kind is not an object")? {
                if own.contains(&kk.as_str()) || out.contains_key(kk) {
                    return Err(format!("record {id}: its kind's field `{kk}` would clash"));
                }
                out.insert(kk.clone(), vv.clone());
            }
        } else if own.contains(&k.as_str()) {
            out.insert(k.clone(), v.clone());
        } else {
            return Err(format!("record {id}: the field `{k}` has no place in the file"));
        }
    }
    Ok(Value::Object(out))
}

/// A record with the fields that are not its own gathered back into `kind`.
fn unflatten(r: &Value, own: &[&str]) -> Value {
    let Some(r) = r.as_object() else { return r.clone() };
    let (mut out, mut kind) = (Map::new(), Map::new());
    for (k, v) in r {
        if own.contains(&k.as_str()) {
            out.insert(k.clone(), v.clone())
        } else {
            kind.insert(k.clone(), v.clone())
        };
    }
    out.insert("kind".into(), Value::Object(kind));
    Value::Object(out)
}

/// A sketch feature (flattened) with its sketch's fields in place of it: entities and
/// constraints as records with their ids.
fn sketch_out(r: &mut Value) -> Result<(), String> {
    let Some(o) = r.as_object() else { return Ok(()) };
    if o.get("type").and_then(Value::as_str) != Some("sketch") {
        return Ok(());
    }
    let mut out = Map::new();
    for (k, v) in o {
        if k != "sketch" {
            out.insert(k.clone(), v.clone());
            continue;
        }
        let s = v.as_object().ok_or("a sketch is not an object")?;
        if let Some(k) = s.keys().find(|k| !SKETCH.contains(&k.as_str())) {
            return Err(format!("the sketch field `{k}` has no place in the file"));
        }
        // Where a dimension's value was placed goes on the dimension's own record, as `at`.
        let mut places = match s.get("places") {
            Some(x) => pairs(x)?,
            None => Vec::new(),
        };
        for f in SKETCH {
            let Some(x) = s.get(*f) else { continue };
            let x = match *f {
                "entities" => Value::Array(pairs(x)?.into_iter().map(entity_out).collect::<Result<_, _>>()?),
                "constraints" => {
                    let mut records = Vec::new();
                    for (id, mut c) in pairs(x)? {
                        if let Some(i) = places.iter().position(|p| p.0 == id) {
                            if c.contains_key("at") {
                                return Err(format!("{id}: the field `at` would clash"));
                            }
                            let mut at = Value::Object(places.remove(i).1);
                            round(&mut at);
                            c.insert("at".into(), at);
                        }
                        records.push(with_id(id, c)?);
                    }
                    Value::Array(records)
                }
                "places" => continue,
                _ => x.clone(),
            };
            out.insert((*f).into(), x);
        }
        if let Some((id, _)) = places.first() {
            return Err(format!("a place is given for dimension {id}, which the sketch does not have"));
        }
    }
    *r = Value::Object(out);
    Ok(())
}

/// The reverse of [`sketch_out`]. Anything malformed is left for the reader to refuse.
fn sketch_in(r: &mut Value) {
    let Some(o) = r.as_object() else { return };
    if o.get("type").and_then(Value::as_str) != Some("sketch") {
        return;
    }
    let (mut out, mut sketch) = (Map::new(), Map::new());
    let mut places = Vec::new();
    for (k, v) in o {
        match k.as_str() {
            "entities" => {
                sketch.insert(k.clone(), Value::Array(v.as_array().into_iter().flatten().map(entity_in).collect()));
            }
            "constraints" => {
                let mut items = Vec::new();
                for c in v.as_array().into_iter().flatten() {
                    let mut c = c.as_object().cloned().unwrap_or_default();
                    let id = c.shift_remove("id").unwrap_or(Value::Null);
                    if let Some(at) = c.shift_remove("at") {
                        places.push(Value::Array(vec![id.clone(), at]));
                    }
                    items.push(Value::Array(vec![id, Value::Object(c)]));
                }
                sketch.insert(k.clone(), Value::Array(items));
            }
            "next_entity" | "next_constraint" => {
                sketch.insert(k.clone(), v.clone());
            }
            _ => {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    if !places.is_empty() {
        sketch.insert("places".into(), Value::Array(places));
    }
    out.insert("sketch".into(), Value::Object(sketch));
    *r = Value::Object(out);
}

/// `[[id, {...}], ...]`.
fn pairs(v: &Value) -> Result<Vec<(Value, Map<String, Value>)>, String> {
    let mut out = Vec::new();
    for p in v.as_array().ok_or("not a list")? {
        match p.as_array().map(Vec::as_slice) {
            Some([id, Value::Object(o)]) => out.push((id.clone(), o.clone())),
            _ => return Err("not an [id, value] pair".into()),
        }
    }
    Ok(out)
}

fn with_id(id: Value, o: Map<String, Value>) -> Result<Value, String> {
    if o.contains_key("id") {
        return Err(format!("{id}: the field `id` would clash"));
    }
    let mut out = Map::new();
    out.insert("id".into(), id);
    out.extend(o);
    Ok(Value::Object(out))
}

/// `[id, {geometry: {type, ...}, construction}]` as `{id, type, ..., construction}`, its
/// solved coordinates rounded.
fn entity_out((id, e): (Value, Map<String, Value>)) -> Result<Value, String> {
    let mut flat = Map::new();
    for (k, v) in e {
        if k == "geometry" {
            for (gk, gv) in v.as_object().cloned().ok_or("a geometry is not an object")? {
                if ENTITY_OWN.contains(&gk.as_str()) || flat.contains_key(&gk) {
                    return Err(format!("entity {id}: the geometry field `{gk}` would clash"));
                }
                flat.insert(gk, gv);
            }
        } else if flat.contains_key(&k) {
            return Err(format!("entity {id}: the field `{k}` would clash"));
        } else {
            flat.insert(k, v);
        }
    }
    let mut out = with_id(id, flat)?;
    round(&mut out);
    Ok(out)
}

fn entity_in(e: &Value) -> Value {
    let (mut own, mut geometry) = (Map::new(), Map::new());
    let mut id = Value::Null;
    for (k, v) in e.as_object().into_iter().flatten() {
        if k == "id" {
            id = v.clone();
        } else if ENTITY_OWN.contains(&k.as_str()) {
            own.insert(k.clone(), v.clone());
        } else {
            geometry.insert(k.clone(), v.clone());
        }
    }
    let mut out = Map::new();
    out.insert("geometry".into(), Value::Object(geometry));
    out.extend(own);
    Value::Array(vec![id, Value::Object(out)])
}

/// `v` with the fields named in `order` first.
fn first(v: &Value, order: &[&str]) -> Map<String, Value> {
    let Some(o) = v.as_object() else { return Map::new() };
    let mut out = Map::new();
    for k in order {
        if let Some(x) = o.get(*k) {
            out.insert((*k).into(), x.clone());
        }
    }
    out.extend(o.iter().filter(|(k, _)| !order.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())));
    out
}

/// A face made from a sub-shape (`type = "from"`) names it by a 64-bit key, `source`. TOML
/// integers are signed, so a key above `i64::MAX` is written as the same 64 bits read as signed
/// (it shows as negative): `to_signed` on writing, the other way on reading.
fn source_keys(v: &mut Value, to_signed: bool) {
    match v {
        Value::Object(o) => {
            if o.get("type").and_then(Value::as_str) == Some("from")
                && let Some(s) = o.get_mut("source")
            {
                let other = if to_signed {
                    s.as_u64().filter(|k| *k > i64::MAX.unsigned_abs()).map(|k| Value::from(i64::from_ne_bytes(k.to_ne_bytes())))
                } else {
                    s.as_i64().filter(|k| *k < 0).map(|k| Value::from(u64::from_ne_bytes(k.to_ne_bytes())))
                };
                if let Some(x) = other {
                    *s = x;
                }
            }
            o.values_mut().for_each(|x| source_keys(x, to_signed));
        }
        Value::Array(a) => a.iter_mut().for_each(|x| source_keys(x, to_signed)),
        _ => {}
    }
}

/// Every reference fingerprint in `v` rounded.
fn round_fingerprints(v: &mut Value) {
    match v {
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                if k == "fingerprint" { round(x) } else { round_fingerprints(x) }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(round_fingerprints),
        _ => {}
    }
}

/// Every float in `v` rounded to `tol::FILE_DECIMALS`; integers stay as they are.
fn round(v: &mut Value) {
    match v {
        Value::Number(n) if n.is_f64() => {
            if let Some(x) = n.as_f64().map(rounded).and_then(serde_json::Number::from_f64) {
                *n = x;
            }
        }
        Value::Object(o) => o.values_mut().for_each(round),
        Value::Array(a) => a.iter_mut().for_each(round),
        _ => {}
    }
}

fn rounded(x: f64) -> f64 {
    let r: f64 = format!("{x:.prec$}", prec = tol::FILE_DECIMALS).parse().unwrap_or(x);
    if r == 0.0 { 0.0 } else { r }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_part_lays_out_and_comes_back() {
        let head = json!({
            "format": "tenon", "version": 1, "generator": "tenon 0.1.0", "future": { "a": 1 },
            "document": {
                "name": "P",
                "features": [
                    { "id": 1, "name": "Sketch1", "suppressed": false, "kind": { "type": "sketch", "plane": { "origin": "XY" }, "sketch": {
                        "entities": [[1, { "geometry": { "type": "point", "pos": { "x": 29.999999999999996, "y": -1e-12 } }, "construction": true }]],
                        "constraints": [[1, { "type": "fix", "point": 1 }]],
                        "next_entity": 2, "next_constraint": 2 } } },
                    { "id": 2, "name": "Fillet1", "suppressed": false, "kind": { "type": "fillet", "radius": 0.1, "edges": [
                        { "faces": [], "fingerprint": { "mid": { "x": 4769.097335529232, "y": 1, "z": 0.0 }, "length": 30.0 } }] } },
                    { "id": 3, "name": "Hole1", "suppressed": true, "kind": { "type": "hole", "kind": "simple", "diameter": 5.0 } },
                ],
                "next_feature": 4,
                "params": { "model": [], "user": [], "next": 0 },
            },
        });
        let file = to_file(head.as_object().unwrap(), &PART, 2).unwrap();
        let keys: Vec<&str> = file.keys().map(String::as_str).collect();
        assert_eq!(keys, ["format", "version", "name", "next_feature", "parameters", "feature", "future"]);
        assert_eq!(file["version"], 2);
        let sketch = &file["feature"][0];
        let keys: Vec<&str> = sketch.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["id", "name", "suppressed", "type", "plane", "next_entity", "next_constraint", "entities", "constraints"]);
        assert_eq!(sketch["entities"][0], json!({ "id": 1, "type": "point", "pos": { "x": 30.0, "y": 0.0 }, "construction": true }));
        assert_eq!(sketch["constraints"][0], json!({ "id": 1, "type": "fix", "point": 1 }));
        let fillet = &file["feature"][1];
        assert_eq!(fillet["radius"], 0.1, "values the user gave are not rounded");
        assert_eq!(file["feature"][2], json!({ "id": 3, "name": "Hole1", "suppressed": true, "type": "hole", "kind": "simple", "diameter": 5.0 }));
        assert_eq!(fillet["edges"][0]["fingerprint"]["mid"], json!({ "x": 4769.097335529, "y": 1, "z": 0.0 }));
        let keys: Vec<&str> = file["parameters"].as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["next", "user", "model"]);

        // Back: the same document (rounded), without the generator, the version as the file's.
        let back = from_file(file, &PART);
        let mut expected = head.clone();
        expected.as_object_mut().unwrap().shift_remove("generator");
        expected["version"] = json!(2);
        expected["document"]["features"][0]["kind"]["sketch"]["entities"][0][1]["geometry"]["pos"] = json!({ "x": 30.0, "y": 0.0 });
        expected["document"]["features"][1]["kind"]["edges"][0]["fingerprint"]["mid"]["x"] = json!(4769.097335529);
        assert_eq!(Value::Object(back), expected);
    }

    #[test]
    fn keys_above_the_signed_range_come_back() {
        let big = u64::MAX - 5;
        let face = |source: u64| json!({ "origin": { "type": "from", "feature": 1, "source": source, "ordinal": 0 } });
        let head = json!({ "format": "tenon", "document": { "name": "P", "features": [
            { "id": 1, "name": "Shell1", "suppressed": false, "kind": { "type": "shell", "remove": [face(big), face(517)], "thickness": 1.0 } },
        ] } });
        let file = to_file(head.as_object().unwrap(), &PART, 2).unwrap();
        let remove = &file["feature"][0]["remove"];
        assert_eq!((remove[0]["origin"]["source"].clone(), remove[1]["origin"]["source"].clone()), (json!(-6), json!(517)));
        let back = from_file(file, &PART);
        assert_eq!(back["document"]["features"][0]["kind"]["remove"][0]["origin"]["source"], json!(big));
    }

    #[test]
    fn clashes_and_strays_are_caught_when_writing() {
        let doc = |f: Value| json!({ "format": "tenon", "document": { "name": "P", "features": [f] } });
        let clash = doc(json!({ "id": 1, "name": "A", "kind": { "type": "x", "name": "B" } }));
        assert!(to_file(clash.as_object().unwrap(), &PART, 2).unwrap_err().contains("clash"));
        let stray = doc(json!({ "id": 1, "name": "A", "colour": 3, "kind": { "type": "x" } }));
        assert!(to_file(stray.as_object().unwrap(), &PART, 2).unwrap_err().contains("`colour`"));
        let unknown = json!({ "format": "tenon", "document": { "name": "P", "layers": [] } });
        assert!(to_file(unknown.as_object().unwrap(), &PART, 2).unwrap_err().contains("`layers`"));
    }
}

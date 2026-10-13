//! Command parameters as data: what every command takes, said in a form a program can read.
//!
//! A command's help text says its parameters in words. An agent that drives Tenon needs them
//! exactly: which names there are, which must be given, what type each is. Each registry (the
//! part's here, the assembly's, the drawing's, the file commands') declares a
//! [`CommandSchema`] for every command it has; from those come the published JSON Schemas
//! (`docs/schemas/commands.json`, `tenon-cli commands --schema`), what the MCP server tells its
//! clients, and the check that parameters are what they should be before a command runs
//! ([`check`]).
//!
//! **They are kept true by the tests, not by care.** In builds with debug assertions (every
//! test run) each command run is checked both ways: a parameter it is *given* must be declared,
//! and a parameter it *reads* must be declared ([`Watch`]; parameters are read through [`arg`]).
//! So a schema that leaves a parameter out fails whichever test exercises the command.
//!
//! **Versions.** The schemas have one version number ([`VERSION`]). Within a version they only
//! grow: new commands, new optional parameters, wider types. Anything that could break a caller
//! (a command or parameter removed or renamed, an optional parameter made required, a type
//! narrowed) needs a new version; [`breaking`] tells the two apart.

use std::cell::RefCell;

use serde_json::{Map, Value, json};

/// The version of the command schemas (see the module text).
pub const VERSION: u32 = 1;

/// The type of a parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ty {
    /// A number with no unit (a factor, a ratio, a density).
    Number,
    /// A length in millimetres.
    Length,
    /// An angle in radians.
    Angle,
    /// A whole number: a count, an index.
    Int,
    /// The id of a feature, a sketch entity, a constraint, a component and so on: a whole
    /// number, zero or more, as the commands that make or list such things return it.
    Id,
    Bool,
    Text,
    /// A file or folder. Relative paths are taken from the working folder.
    Path,
    /// One of these words.
    Word(&'static [&'static str]),
    /// A list of these.
    List(&'static Ty),
    /// A list of exactly this many numbers: `[x, y]`, `[x, y, z]`.
    Vector(usize),
    /// A reference to a face, as `model.face_ref` returns it.
    Face,
    /// A reference to an edge, as `model.edge_ref` returns it.
    Edge,
    /// An object, described in the parameter's text.
    Object,
    /// Null: said where it means something (no material, no value).
    Null,
    /// Any one of these types.
    Either(&'static [Ty]),
    /// Anything.
    Any,
}

/// A parameter of a command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Param {
    pub name: &'static str,
    pub ty: Ty,
    /// Must be given. (Where one of several must be, each is optional and
    /// [`CommandSchema::one_of`] says so.)
    pub required: bool,
    pub doc: &'static str,
}

/// A required parameter.
pub const fn req(name: &'static str, ty: Ty, doc: &'static str) -> Param {
    Param { name, ty, required: true, doc }
}

/// An optional parameter.
pub const fn opt(name: &'static str, ty: Ty, doc: &'static str) -> Param {
    Param { name, ty, required: false, doc }
}

/// Parameters of which one set must be given: `[["start"], ["x1", "y1"]]` is "`start`, or
/// `x1` and `y1`".
pub type OneOf = &'static [&'static [&'static str]];

/// What a command takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CommandSchema {
    pub id: &'static str,
    pub params: &'static [Param],
    /// Each entry is a choice that must be made (see [`OneOf`]).
    pub one_of: &'static [OneOf],
}

impl CommandSchema {
    pub const fn new(id: &'static str, params: &'static [Param]) -> CommandSchema {
        CommandSchema { id, params, one_of: &[] }
    }

    pub const fn with(id: &'static str, params: &'static [Param], one_of: &'static [OneOf]) -> CommandSchema {
        CommandSchema { id, params, one_of }
    }

    pub fn param(&self, name: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.name == name)
    }
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "true or false",
        Value::Number(_) => "a number",
        Value::String(_) => "text",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

impl Ty {
    /// What is wrong with `v` as a value of this type, if anything.
    fn wrong(&self, v: &Value) -> Option<String> {
        let whole = |v: &Value| v.as_i64().is_some() || v.as_u64().is_some();
        let expected = |what: &str| Some(format!("must be {what}, not {}", kind(v)));
        match self {
            Ty::Number | Ty::Length | Ty::Angle => match v.as_f64() {
                Some(x) if x.is_finite() => None,
                _ => expected("a number"),
            },
            Ty::Int => (!whole(v)).then(|| format!("must be a whole number, not {}", if v.is_number() { "a fraction" } else { kind(v) })),
            Ty::Id => match v.as_u64() {
                Some(x) if u32::try_from(x).is_ok() => None,
                _ => expected("an id (a whole number, 0 or more)"),
            },
            Ty::Bool => (!v.is_boolean()).then(|| format!("must be true or false, not {}", kind(v))),
            Ty::Text | Ty::Path => (!v.is_string()).then(|| format!("must be text, not {}", kind(v))),
            Ty::Word(words) => match v.as_str() {
                // (Whatever the case: the commands that take a plane or an axis by name do.)
                Some(s) if words.iter().any(|w| w.eq_ignore_ascii_case(s)) => None,
                Some(s) => Some(format!("must be one of {}, not \"{s}\"", words.join(" | "))),
                None => expected(&format!("one of {}", words.join(" | "))),
            },
            Ty::List(of) => match v.as_array() {
                Some(items) => items.iter().enumerate().find_map(|(i, item)| of.wrong(item).map(|w| format!("item {i} {w}"))),
                None => expected("a list"),
            },
            Ty::Vector(n) => match v.as_array() {
                Some(items) if items.len() == *n && items.iter().all(|x| x.as_f64().is_some_and(f64::is_finite)) => None,
                _ => expected(&format!("a list of {n} numbers")),
            },
            Ty::Face => (!v.is_object()).then(|| format!("must be a face reference (as model.face_ref returns), not {}", kind(v))),
            Ty::Edge => (!v.is_object()).then(|| format!("must be an edge reference (as model.edge_ref returns), not {}", kind(v))),
            Ty::Object => (!v.is_object()).then(|| format!("must be an object, not {}", kind(v))),
            Ty::Null => (!v.is_null()).then(|| format!("must be null, not {}", kind(v))),
            Ty::Either(types) => {
                if types.iter().any(|t| t.wrong(v).is_none()) {
                    None
                } else {
                    // The complaint of the type of the same JSON kind says most.
                    types.iter().filter_map(|t| t.wrong(v)).find(|w| !w.ends_with(kind(v))).or_else(|| types.first().and_then(|t| t.wrong(v)))
                }
            }
            Ty::Any => None,
        }
    }

    /// The JSON Schema of the type.
    pub fn json(&self) -> Value {
        match self {
            Ty::Number => json!({ "type": "number" }),
            Ty::Length => json!({ "type": "number", "x-unit": "mm" }),
            Ty::Angle => json!({ "type": "number", "x-unit": "rad" }),
            Ty::Int => json!({ "type": "integer" }),
            Ty::Id => json!({ "type": "integer", "minimum": 0 }),
            Ty::Bool => json!({ "type": "boolean" }),
            Ty::Text => json!({ "type": "string" }),
            Ty::Path => json!({ "type": "string", "x-path": true }),
            Ty::Word(words) => json!({ "type": "string", "enum": words }),
            Ty::List(of) => json!({ "type": "array", "items": of.json() }),
            Ty::Vector(n) => json!({ "type": "array", "items": { "type": "number" }, "minItems": n, "maxItems": n }),
            Ty::Face => json!({ "$ref": "#/$defs/face_ref" }),
            Ty::Edge => json!({ "$ref": "#/$defs/edge_ref" }),
            Ty::Object => json!({ "type": "object" }),
            Ty::Null => json!({ "type": "null" }),
            Ty::Either(types) => json!({ "anyOf": types.iter().map(Ty::json).collect::<Vec<_>>() }),
            Ty::Any => json!({}),
        }
    }
}

/// How many single-letter edits turn `a` into `b`.
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut last = row.first().copied().unwrap_or(0);
        if let Some(first) = row.first_mut() {
            *first = i + 1;
        }
        for (j, cb) in b.iter().enumerate() {
            let here = row.get(j + 1).copied().unwrap_or(0);
            let left = row.get(j).copied().unwrap_or(0);
            let best = (last + usize::from(ca != cb)).min(here + 1).min(left + 1);
            if let Some(cell) = row.get_mut(j + 1) {
                *cell = best;
            }
            last = here;
        }
    }
    row.last().copied().unwrap_or(0)
}

/// Whether a parameter counts as given: there, and not null.
fn given(params: &Map<String, Value>, name: &str) -> bool {
    params.get(name).is_some_and(|v| !v.is_null())
}

/// Checks `params` against the schema: nothing it does not know, everything it needs, each
/// value of its type. The error says what to change, in the words of the parameters.
///
/// An optional parameter that is null counts as not given, as the commands take it.
pub fn check(schema: &CommandSchema, params: &Value) -> Result<(), String> {
    let empty = Map::new();
    let map = match params {
        Value::Null => &empty,
        Value::Object(map) => map,
        _ => return Err("parameters must be a JSON object".into()),
    };
    let id = schema.id;
    for (name, value) in map {
        let Some(param) = schema.param(name) else {
            // (A slip of the hand, not another word: one letter out in a short name, two in a long
            // one, or the same letters in another order.)
            let letters = |s: &str| {
                let mut l: Vec<char> = s.chars().collect();
                l.sort_unstable();
                l
            };
            let near = schema
                .params
                .iter()
                .map(|p| (distance(name, p.name), p.name))
                .filter(|(d, n)| *d <= if name.len().max(n.len()) > 4 { 2 } else { 1 } || letters(name) == letters(n))
                .min();
            let known: Vec<&str> = schema.params.iter().map(|p| p.name).collect();
            return Err(match near {
                Some((_, near)) => format!("{id} has no parameter `{name}`: did you mean `{near}`?"),
                None if known.is_empty() => format!("{id} takes no parameters, so not `{name}`"),
                None => format!("{id} has no parameter `{name}`: it takes {}", known.join(", ")),
            });
        };
        if value.is_null() && !param.required {
            continue;
        }
        if let Some(wrong) = param.ty.wrong(value) {
            return Err(format!("{id}: `{name}` {wrong} ({})", param.doc));
        }
    }
    if let Some(missing) = schema.params.iter().find(|p| p.required && !map.contains_key(p.name)) {
        return Err(format!("{id} needs `{}`: {}", missing.name, missing.doc));
    }
    for choice in schema.one_of {
        if !choice.iter().any(|set| set.iter().all(|name| given(map, name))) {
            let sets: Vec<String> = choice.iter().map(|set| set.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(" and ")).collect();
            return Err(format!("{id} needs {}", sets.join(", or ")));
        }
    }
    Ok(())
}

/// What is wrong with a registry's schemas as such, whatever its commands do: an id or a
/// parameter declared twice, a parameter without a word of explanation, a choice between
/// parameters that are not declared, a required parameter in a choice. For each registry's test.
pub fn sound(schemas: &[CommandSchema]) -> Result<(), String> {
    for (i, s) in schemas.iter().enumerate() {
        if schemas.iter().skip(i + 1).any(|other| other.id == s.id) {
            return Err(format!("{} has two schemas", s.id));
        }
        for (j, p) in s.params.iter().enumerate() {
            if s.params.iter().skip(j + 1).any(|other| other.name == p.name) {
                return Err(format!("{} declares `{}` twice", s.id, p.name));
            }
            if p.doc.trim().is_empty() {
                return Err(format!("{}: `{}` is not explained", s.id, p.name));
            }
        }
        for name in s.one_of.iter().flat_map(|choice| choice.iter()).flat_map(|set| set.iter()) {
            match s.param(name) {
                None => return Err(format!("{} chooses between parameters, of which `{name}` is not declared", s.id)),
                Some(p) if p.required => return Err(format!("{}: `{name}` is required and in a choice", s.id)),
                Some(_) => {}
            }
        }
    }
    Ok(())
}

/// The JSON Schema of a command's parameters (draft 2020-12; `$ref`s point into the
/// `$defs` of [`definitions`]).
pub fn parameters(schema: &CommandSchema) -> Value {
    let mut properties = Map::new();
    for p in schema.params {
        let mut ty = p.ty.json();
        if let Some(o) = ty.as_object_mut() {
            o.insert("description".into(), json!(p.doc));
        }
        properties.insert(p.name.to_owned(), ty);
    }
    let required: Vec<&str> = schema.params.iter().filter(|p| p.required).map(|p| p.name).collect();
    let mut out = json!({ "type": "object", "properties": properties, "additionalProperties": false });
    if let Some(o) = out.as_object_mut() {
        if !required.is_empty() {
            o.insert("required".into(), json!(required));
        }
        let choices: Vec<Value> =
            schema.one_of.iter().map(|choice| json!({ "anyOf": choice.iter().map(|set| json!({ "required": set })).collect::<Vec<_>>() })).collect();
        if !choices.is_empty() {
            o.insert("allOf".into(), json!(choices));
        }
    }
    out
}

/// What `$ref`s in the schemas point to.
pub fn definitions() -> Value {
    json!({
        "face_ref": {
            "type": "object",
            "description": "A persistent reference to a face: what model.face_ref returns. It names the face by the feature and sketch curve that made it, and keeps a fingerprint to find it again when the name is lost.",
        },
        "edge_ref": {
            "type": "object",
            "description": "A persistent reference to an edge: what model.edge_ref returns. It names the edge by the two faces it lies between.",
        },
    })
}

/// What would break a caller written against `old` if it met `new` instead: commands and
/// parameters gone, optional parameters now required, types that no longer take what they took.
/// Both are documents as `tenon-cli commands --schema` writes them. Nothing in the list means
/// `new` may keep `old`'s version.
pub fn breaking(old: &Value, new: &Value) -> Vec<String> {
    let commands = |doc: &Value| -> Vec<(String, Value)> {
        doc["commands"]
            .as_array()
            .map(|c| c.iter().map(|c| (c["id"].as_str().unwrap_or_default().to_owned(), c["params"].clone())).collect())
            .unwrap_or_default()
    };
    let now = commands(new);
    let mut out = Vec::new();
    for (id, was) in commands(old) {
        let Some((_, is)) = now.iter().find(|(i, _)| *i == id) else {
            out.push(format!("{id} is gone"));
            continue;
        };
        let required = |s: &Value| -> Vec<String> {
            s["required"].as_array().map(|r| r.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default()
        };
        let none = Map::new();
        let props = |s: &Value| s["properties"].as_object().cloned().unwrap_or_else(|| none.clone());
        let (was_props, is_props) = (props(&was), props(is));
        for (name, was_ty) in &was_props {
            match is_props.get(name) {
                None => out.push(format!("{id}: `{name}` is gone")),
                Some(is_ty) if !takes_all(is_ty, was_ty) => out.push(format!("{id}: `{name}` no longer takes what it took")),
                Some(_) => {}
            }
        }
        for name in required(is) {
            if !required(&was).contains(&name) {
                out.push(format!("{id}: `{name}` is now required"));
            }
        }
        if was["allOf"] != is["allOf"] && !is["allOf"].is_null() {
            out.push(format!("{id}: which parameters must be given together has changed"));
        }
    }
    out
}

/// Whether a value that fits the type schema `old` fits `new` too, as far as can be told from
/// the two: the same schema, or `new` with more words, more alternatives, or no limits.
fn takes_all(new: &Value, old: &Value) -> bool {
    let strip = |v: &Value| {
        let mut v = v.clone();
        if let Some(o) = v.as_object_mut() {
            o.remove("description");
        }
        v
    };
    let (new, old) = (strip(new), strip(old));
    if new == old || new.as_object().is_some_and(Map::is_empty) {
        return true;
    }
    let alternatives = |v: &Value| v["anyOf"].as_array().cloned().unwrap_or_else(|| vec![v.clone()]);
    if new.get("anyOf").is_some() || old.get("anyOf").is_some() {
        return alternatives(&old).iter().all(|o| alternatives(&new).iter().any(|n| takes_all(n, o)));
    }
    if new["type"] != old["type"] {
        // A whole number is a number.
        return new["type"] == "number" && old["type"] == "integer";
    }
    if let (Some(n), Some(o)) = (new["enum"].as_array(), old["enum"].as_array()) {
        return o.iter().all(|w| n.contains(w));
    }
    if old.get("enum").is_some() != new.get("enum").is_some() {
        return new.get("enum").is_none();
    }
    if new["type"] == "array" {
        return takes_all(&new["items"], &old["items"]) && new["minItems"] == old["minItems"] && new["maxItems"] == old["maxItems"];
    }
    true
}

thread_local! {
    /// The parameters of the commands being run on this thread, innermost last: where each
    /// object is, and the names read from it so far.
    static WATCHED: RefCell<Vec<(usize, Vec<String>)>> = const { RefCell::new(Vec::new()) };
}

/// `p.get(key)`. Commands read their parameters through this, so that what a command reads
/// can be held against what its schema declares ([`Watch`]). Reading from any other object (a
/// constraint inside the parameters, an item of a list) is not noted.
pub fn arg<'v>(p: &'v Value, key: &str) -> Option<&'v Value> {
    if cfg!(debug_assertions) {
        let at = std::ptr::from_ref(p) as usize;
        WATCHED.with(|w| {
            if let Some((_, read)) = w.borrow_mut().iter_mut().rev().find(|(object, _)| *object == at)
                && !read.iter().any(|k| k == key)
            {
                read.push(key.to_owned());
            }
        });
    }
    p.get(key)
}

/// Runs a command held to its schema, in builds with debug assertions (every test run): what
/// it is given must be declared, and so must what it reads. A command without a schema is
/// refused there. In a release build this only runs it: the app's own calls are trusted, and
/// whoever takes commands from outside checks them first ([`check`]).
pub fn held<T, E: From<String>>(id: &str, schema: Option<&CommandSchema>, params: &Value, run: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    if !cfg!(debug_assertions) {
        return run();
    }
    let schema = schema.ok_or_else(|| E::from(format!("{id} has no schema (a mistake in Tenon: every command declares its parameters)")))?;
    check(schema, params).map_err(E::from)?;
    let watch = Watch::of(params);
    let done = run()?;
    watch.finish(schema).map_err(E::from)?;
    Ok(done)
}

/// While this lives, the names a command reads from its parameters are noted (in builds with
/// debug assertions; otherwise it does nothing).
pub struct Watch {
    on: bool,
}

impl Watch {
    /// Starts noting what is read from `params` (the very object the command is handed).
    pub fn of(params: &Value) -> Watch {
        let on = cfg!(debug_assertions);
        if on {
            WATCHED.with(|w| w.borrow_mut().push((std::ptr::from_ref(params) as usize, Vec::new())));
        }
        Watch { on }
    }

    /// What the command read that `schema` does not declare, as an error. A command that
    /// failed is not held to this: it may have stopped at a parameter it was about to refuse.
    pub fn finish(mut self, schema: &CommandSchema) -> Result<(), String> {
        if !self.on {
            return Ok(());
        }
        self.on = false;
        let read = WATCHED.with(|w| w.borrow_mut().pop()).map(|(_, read)| read).unwrap_or_default();
        match read.iter().find(|name| schema.param(name).is_none()) {
            Some(name) => Err(format!(
                "{} read the parameter `{name}`, which its schema does not declare. This is a mistake in Tenon (crates/*/src/schemas.rs), not in what was asked.",
                schema.id
            )),
            None => Ok(()),
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        if self.on {
            WATCHED.with(|w| {
                w.borrow_mut().pop();
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: CommandSchema = CommandSchema::with(
        "sketch.line",
        &[
            req("sketch", Ty::Id, "the sketch"),
            opt("start", Ty::Id, "a point to start at"),
            opt("x1", Ty::Length, "where it starts"),
            opt("y1", Ty::Length, "where it starts"),
            opt("style", Ty::Word(&["solid", "dashed"]), "how it is drawn"),
            opt("through", Ty::List(&Ty::Vector(2)), "points on the way"),
            opt("name", Ty::Either(&[Ty::Text, Ty::Null]), "its name, or null for none"),
        ],
        &[&[&["start"], &["x1", "y1"]]],
    );

    #[test]
    fn parameters_are_checked_for_names_presence_and_types() {
        let ok = |p: Value| check(&LINE, &p);
        assert_eq!(ok(json!({ "sketch": 1, "start": 4 })), Ok(()));
        assert_eq!(ok(json!({ "sketch": 1, "x1": 0, "y1": 2.5, "style": "dashed", "through": [[1, 2], [3, 4.5]] })), Ok(()));
        // A null optional parameter is one not given.
        assert_eq!(ok(json!({ "sketch": 1, "start": 4, "style": null, "name": null })), Ok(()));
        // What is wrong is said in the parameter's own words.
        let err = |p: Value| ok(p).unwrap_err();
        assert_eq!(err(json!({ "start": 4 })), "sketch.line needs `sketch`: the sketch");
        assert_eq!(err(json!({ "sketch": 1 })), "sketch.line needs `start`, or `x1` and `y1`");
        assert_eq!(err(json!({ "sketch": 1, "x1": 0, "y1": null })), "sketch.line needs `start`, or `x1` and `y1`");
        assert_eq!(err(json!({ "sketch": 1, "strat": 4 })), "sketch.line has no parameter `strat`: did you mean `start`?");
        assert_eq!(err(json!({ "sketch": 1, "start": 4, "x_1": 0 })), "sketch.line has no parameter `x_1`: did you mean `x1`?");
        assert_eq!(
            err(json!({ "sketch": 1, "start": 4, "colour": "red" })),
            "sketch.line has no parameter `colour`: it takes sketch, start, x1, y1, style, through, name"
        );
        assert_eq!(
            err(json!({ "sketch": "one", "start": 4 })),
            "sketch.line: `sketch` must be an id (a whole number, 0 or more), not text (the sketch)"
        );
        assert_eq!(
            err(json!({ "sketch": -1, "start": 4 })),
            "sketch.line: `sketch` must be an id (a whole number, 0 or more), not a number (the sketch)"
        );
        assert_eq!(err(json!({ "sketch": 1, "x1": "2", "y1": 0 })), "sketch.line: `x1` must be a number, not text (where it starts)");
        assert_eq!(
            err(json!({ "sketch": 1, "start": 4, "style": "dotted" })),
            "sketch.line: `style` must be one of solid | dashed, not \"dotted\" (how it is drawn)"
        );
        assert_eq!(
            err(json!({ "sketch": 1, "start": 4, "through": [[1, 2], [3]] })),
            "sketch.line: `through` item 1 must be a list of 2 numbers, not a list (points on the way)"
        );
        assert_eq!(err(json!({ "sketch": 1, "start": 4, "name": 7 })), "sketch.line: `name` must be text, not a number (its name, or null for none)");
        assert_eq!(err(json!([1, 2])), "parameters must be a JSON object");
        assert_eq!(
            check(&CommandSchema::new("edit.undo", &[]), &json!({ "steps": 2 })).unwrap_err(),
            "edit.undo takes no parameters, so not `steps`"
        );
        assert_eq!(check(&CommandSchema::new("edit.undo", &[]), &Value::Null), Ok(()));
        assert!(Ty::Int.wrong(&json!(2.5)).unwrap().contains("a fraction"));
        assert!(Ty::Int.wrong(&json!(-3)).is_none());
    }

    #[test]
    fn the_json_schema_says_the_same() {
        let s = parameters(&LINE);
        assert_eq!(s["type"], "object");
        assert_eq!(s["additionalProperties"], false);
        assert_eq!(s["required"], json!(["sketch"]));
        assert_eq!(s["properties"]["sketch"], json!({ "type": "integer", "minimum": 0, "description": "the sketch" }));
        assert_eq!(s["properties"]["x1"]["x-unit"], "mm");
        assert_eq!(s["properties"]["style"]["enum"], json!(["solid", "dashed"]));
        assert_eq!(s["properties"]["through"]["items"], json!({ "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 }));
        assert_eq!(s["properties"]["name"]["anyOf"], json!([{ "type": "string" }, { "type": "null" }]));
        assert_eq!(s["allOf"], json!([{ "anyOf": [{ "required": ["start"] }, { "required": ["x1", "y1"] }] }]));
        assert_eq!(parameters(&CommandSchema::new("edit.undo", &[])), json!({ "type": "object", "properties": {}, "additionalProperties": false }));
        assert_eq!(Ty::Face.json(), json!({ "$ref": "#/$defs/face_ref" }));
        assert!(definitions()["face_ref"].is_object() && definitions()["edge_ref"].is_object());
    }

    #[test]
    fn what_a_command_reads_is_held_against_its_schema() {
        let params = json!({ "sketch": 1, "start": 4, "through": [{ "inner": 1 }] });
        // Everything read is declared, present or not.
        let watch = Watch::of(&params);
        assert_eq!(arg(&params, "sketch"), Some(&json!(1)));
        assert_eq!(arg(&params, "x1"), None);
        assert_eq!(watch.finish(&LINE), Ok(()));
        // A name the schema does not have is told, whether or not it was given.
        let watch = Watch::of(&params);
        arg(&params, "sketch");
        arg(&params, "angle");
        let told = watch.finish(&LINE).unwrap_err();
        assert!(told.starts_with("sketch.line read the parameter `angle`, which its schema does not declare"), "{told}");
        // Reading inside the parameters is no business of the schema's, and nor is a read of
        // some other object while the command runs.
        let watch = Watch::of(&params);
        let inner = &params["through"][0];
        arg(inner, "inner");
        arg(&json!({ "other": 1 }), "other");
        assert_eq!(watch.finish(&LINE), Ok(()));
        // A command run inside a command has its own parameters and its own list.
        let outer = Watch::of(&params);
        let nested = json!({ "feature": 2 });
        let inner = Watch::of(&nested);
        arg(&nested, "feature");
        arg(&params, "start");
        assert!(inner.finish(&LINE).unwrap_err().contains("`feature`"));
        assert_eq!(outer.finish(&LINE), Ok(()));
        // Dropped without being finished (the command failed), it leaves nothing behind.
        {
            let _left = Watch::of(&params);
            arg(&params, "nonsense");
        }
        let watch = Watch::of(&params);
        assert_eq!(watch.finish(&LINE), Ok(()));
    }

    #[test]
    fn a_change_that_would_break_a_caller_is_told_from_one_that_would_not() {
        let doc = |schemas: &[CommandSchema]| json!({ "version": VERSION, "commands": schemas.iter().map(|s| json!({ "id": s.id, "params": parameters(s) })).collect::<Vec<_>>() });
        let old = doc(&[LINE, CommandSchema::new("edit.undo", &[])]);
        assert!(breaking(&old, &old).is_empty());
        // More commands, more optional parameters, more words, wider types: all the same to a
        // caller written before.
        const WIDER: CommandSchema = CommandSchema::with(
            "sketch.line",
            &[
                req("sketch", Ty::Id, "the sketch, said otherwise"),
                opt("start", Ty::Either(&[Ty::Id, Ty::Text]), "a point to start at, or its name"),
                opt("x1", Ty::Length, "where it starts"),
                opt("y1", Ty::Number, "where it starts"),
                opt("style", Ty::Word(&["solid", "dashed", "dotted"]), "how it is drawn"),
                opt("through", Ty::List(&Ty::Vector(2)), "points on the way"),
                opt("name", Ty::Any, "its name"),
                opt("layer", Ty::Text, "new"),
            ],
            &[&[&["start"], &["x1", "y1"]]],
        );
        let new = doc(&[WIDER, CommandSchema::new("edit.undo", &[]), CommandSchema::new("edit.redo", &[])]);
        assert_eq!(breaking(&old, &new), Vec::<String>::new());
        // The other way round every one of those is a break.
        let back = breaking(&new, &old);
        for told in [
            "edit.redo is gone",
            "sketch.line: `layer` is gone",
            "sketch.line: `start` no longer takes what it took",
            "sketch.line: `style` no longer takes what it took",
            "sketch.line: `name` no longer takes what it took",
        ] {
            assert!(back.iter().any(|b| b == told), "{told} in {back:?}");
        }
        // Required where it was not; a number where anything whole was is fine, the reverse not.
        const STRICT: CommandSchema = CommandSchema::new(
            "sketch.line",
            &[
                req("sketch", Ty::Number, ""),
                req("start", Ty::Id, ""),
                opt("x1", Ty::Int, ""),
                opt("y1", Ty::Length, ""),
                opt("style", Ty::Text, ""),
                opt("through", Ty::List(&Ty::Vector(3)), ""),
                opt("name", Ty::Either(&[Ty::Text, Ty::Null]), ""),
            ],
        );
        let told = breaking(&old, &doc(&[STRICT, CommandSchema::new("edit.undo", &[])]));
        assert_eq!(
            told,
            [
                "sketch.line: `x1` no longer takes what it took",
                "sketch.line: `through` no longer takes what it took",
                "sketch.line: `start` is now required"
            ]
        );
    }

    #[test]
    fn near_names_are_told_apart_from_far_ones() {
        assert_eq!(distance("start", "strat"), 2);
        assert_eq!(distance("x1", "x_1"), 1);
        assert_eq!(distance("", "abc"), 3);
        assert_eq!(distance("same", "same"), 0);
    }
}

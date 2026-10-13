//! The steps of a command script, shared by whatever runs commands in a row: `tenon-cli run`,
//! `batch.run`, the MCP server, the app's control port.
//!
//! A step is `{"run": id, "with": parameters, "as": a name for its result, "expect": {path:
//! value}}`. A string `"$name.path"` in `with` is replaced by that part of the result of the
//! step saved `as` `name` (path segments are object keys or array indices; `"$$"` escapes a
//! dollar sign). `expect` checks parts of a step's result: numbers within a relative 1e-6 (or
//! the step's own `within`), anything else exactly. The format is documented in docs/scripts.md.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Map, Value};

/// Relative tolerance for numeric `expect` checks.
pub const EXPECT_REL: f64 = 1e-6;
/// The loosest tolerance a step may ask for with `within`: beyond it a check says little.
pub const MAX_WITHIN: f64 = 0.1;
/// Most steps a script may have (hostile-input cap).
pub const MAX_STEPS: usize = 100_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub run: String,
    #[serde(default)]
    pub with: Value,
    #[serde(default, rename = "as")]
    pub save_as: Option<String>,
    #[serde(default)]
    pub expect: Map<String, Value>,
    /// The relative tolerance of this step's numeric expectations, in place of [`EXPECT_REL`]:
    /// for values the kernel approximates (a coil's volume, a thread's groove).
    #[serde(default)]
    pub within: Option<f64>,
    /// A note for readers; ignored.
    #[serde(default)]
    pub note: Option<String>,
}

/// Commands that all-or-nothing steps may not hold: they replace a document, which clears what
/// could have been taken back, or step through the history themselves.
pub const NOT_AS_ONE: &[&str] = &[
    "file.new",
    "file.open",
    "library.new",
    "asm.new",
    "asm.open",
    "drw.new",
    "drw.open",
    "edit.undo",
    "edit.redo",
    "asm.undo",
    "asm.redo",
    "drw.undo",
    "drw.redo",
];

/// Replaces `"$name.path"` strings with saved results.
pub fn substitute(v: &Value, saved: &BTreeMap<String, Value>) -> Result<Value, String> {
    Ok(match v {
        Value::String(s) if s.starts_with("$$") => Value::String(s[1..].to_owned()),
        Value::String(s) if s.starts_with('$') => {
            let mut parts = s[1..].split('.');
            let name = parts.next().unwrap_or_default();
            let root = saved.get(name).ok_or_else(|| format!("`{s}`: no earlier step is saved as `{name}`"))?;
            lookup(root, parts).ok_or_else(|| format!("`{s}`: no such value in the result of `{name}` ({root})"))?.clone()
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| substitute(x, saved)).collect::<Result<_, _>>()?),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| Ok((k.clone(), substitute(x, saved)?))).collect::<Result<_, String>>()?),
        other => other.clone(),
    })
}

fn lookup<'v, 'p>(mut v: &'v Value, path: impl Iterator<Item = &'p str>) -> Option<&'v Value> {
    for seg in path.filter(|s| !s.is_empty()) {
        v = match v {
            Value::Array(a) => a.get(seg.parse::<usize>().ok()?)?,
            Value::Object(o) => o.get(seg)?,
            _ => return None,
        };
    }
    Some(v)
}

/// Checks `expect` against a result, numbers within the relative tolerance `rel`.
pub fn check(result: &Value, expect: &Map<String, Value>, rel: f64) -> Result<(), String> {
    for (path, want) in expect {
        let got = lookup(result, path.split('.')).ok_or_else(|| format!("expected `{path}` in the result, which is {result}"))?;
        let ok = match (got.as_f64(), want.as_f64()) {
            (Some(g), Some(w)) => (g - w).abs() <= rel * w.abs().max(1.0),
            _ => got == want,
        };
        if !ok {
            let note = if rel == EXPECT_REL { String::new() } else { format!(" (within {rel})") };
            return Err(format!("expected `{path}` = {want}{note}, got {got}"));
        }
    }
    Ok(())
}

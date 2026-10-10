//! Command scripts: a JSON list of registry commands, run in order on one document.
//!
//! ```json
//! { "name": "...", "steps": [
//!   { "run": "sketch.create", "with": { "plane": "xz" }, "as": "s1" },
//!   { "run": "sketch.line", "with": { "sketch": "$s1.feature", "x1": 0, "y1": 0, "x2": 60, "y2": 0 }, "as": "l1" },
//!   { "run": "model.mass", "expect": { "bodies.0.volume": 28800.0 } }
//! ] }
//! ```
//!
//! A string `"$name.path"` in `with` is replaced by that part of the result of the step saved
//! `as` `name` (path segments are object keys or array indices; `"$$"` escapes a dollar sign).
//! `expect` checks parts of a step's result: numbers within a relative 1e-6 (or the step's own
//! `within`, for values the kernel approximates), anything else exactly.
//! The format is documented in docs/scripts.md.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::engine::Engine;

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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub steps: Vec<Step>,
}

impl Script {
    /// Parses a script: an object with `steps`, or a bare list of steps.
    pub fn parse(text: &str) -> Result<Script, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
        let script = if v.is_array() {
            Script { name: None, description: None, steps: serde_json::from_value(v).map_err(|e| format!("invalid steps: {e}"))? }
        } else {
            serde_json::from_value(v).map_err(|e| format!("invalid script: {e}"))?
        };
        if script.steps.len() > MAX_STEPS {
            return Err(format!("a script may have at most {MAX_STEPS} steps"));
        }
        for (i, step) in script.steps.iter().enumerate() {
            if step.within.is_some_and(|w| !(w.is_finite() && w > 0.0 && w <= MAX_WITHIN)) {
                return Err(format!("step {} ({}): `within` must be more than 0 and at most {MAX_WITHIN}", i + 1, step.run));
            }
        }
        Ok(script)
    }
}

/// What one step did.
#[derive(Debug, Clone)]
pub struct StepResult {
    pub index: usize,
    pub run: String,
    pub result: Value,
}

/// A failed step: its number (from 1), command and message.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptError {
    pub step: usize,
    pub run: String,
    pub message: String,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "step {} ({}): {}", self.step, self.run, self.message)
    }
}

/// Runs every step in order; stops at the first failure.
pub fn run(engine: &mut Engine, script: &Script) -> Result<Vec<StepResult>, (Vec<StepResult>, ScriptError)> {
    let mut saved: BTreeMap<String, Value> = BTreeMap::new();
    let mut done = Vec::with_capacity(script.steps.len());
    for (i, step) in script.steps.iter().enumerate() {
        let fail = |message: String| ScriptError { step: i + 1, run: step.run.clone(), message };
        let outcome = substitute(&step.with, &saved)
            .and_then(|params| engine.exec(&step.run, &if params.is_null() { json!({}) } else { params }))
            .and_then(|result| check(&result, &step.expect, step.within.unwrap_or(EXPECT_REL)).map(|()| result));
        match outcome {
            Ok(result) => {
                if let Some(name) = &step.save_as {
                    saved.insert(name.clone(), result.clone());
                }
                done.push(StepResult { index: i + 1, run: step.run.clone(), result });
            }
            Err(e) => return Err((done, fail(e))),
        }
    }
    Ok(done)
}

/// Replaces `"$name.path"` strings with saved results.
fn substitute(v: &Value, saved: &BTreeMap<String, Value>) -> Result<Value, String> {
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
fn check(result: &Value, expect: &Map<String, Value>, rel: f64) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn references_resolve_into_nested_values() {
        let mut saved = BTreeMap::new();
        saved.insert("r".to_owned(), json!({ "lines": [7, 8], "feature": 2 }));
        let v = substitute(&json!({ "sketch": "$r.feature", "c": { "line": "$r.lines.1" }, "n": "$$money" }), &saved).unwrap();
        assert_eq!(v, json!({ "sketch": 2, "c": { "line": 8 }, "n": "$money" }));
        assert!(substitute(&json!("$nope.x"), &saved).unwrap_err().contains("no earlier step"));
        assert!(substitute(&json!("$r.lines.5"), &saved).unwrap_err().contains("no such value"));
    }

    #[test]
    fn expectations_compare_numbers_relatively() {
        let r = json!({ "bodies": [{ "volume": 1000.0004, "valid": true }] });
        let mut e = Map::new();
        e.insert("bodies.0.volume".into(), json!(1000.0));
        e.insert("bodies.0.valid".into(), json!(true));
        assert!(check(&r, &e, EXPECT_REL).is_ok());
        e.insert("bodies.0.volume".into(), json!(1001.0));
        assert!(check(&r, &e, EXPECT_REL).unwrap_err().contains("expected `bodies.0.volume`"));
        // A step may ask for a looser check, and the failure says how loose it was.
        assert!(check(&r, &e, 2e-3).is_ok());
        e.insert("bodies.0.volume".into(), json!(1010.0));
        assert!(check(&r, &e, 2e-3).unwrap_err().contains("= 1010.0 (within 0.002), got 1000.0004"));
        assert_eq!(Script::parse(r#"[{"run": "model.mass", "within": 0.01}]"#).unwrap().steps[0].within, Some(0.01));
        for bad in ["0", "-1", "0.5"] {
            let e = Script::parse(&format!(r#"[{{"run": "model.tree"}}, {{"run": "model.mass", "within": {bad}}}]"#)).unwrap_err();
            assert!(e.contains("step 2 (model.mass): `within`"), "{e}");
        }
    }

    #[test]
    fn scripts_parse_from_objects_and_lists() {
        assert_eq!(Script::parse(r#"[{"run": "model.tree"}]"#).unwrap().steps.len(), 1);
        let s = Script::parse(r#"{"name": "x", "steps": [{"run": "model.tree", "as": "t"}]}"#).unwrap();
        assert_eq!(s.steps[0].save_as.as_deref(), Some("t"));
        assert!(Script::parse(r#"{"steps": [{"run": "model.tree", "typo": 1}]}"#).unwrap_err().contains("typo"));
        assert!(Script::parse("nope").is_err());
    }
}

//! Parameters: every sketch dimension and numeric feature value has a name (d0, d1, ...) and may
//! be driven by an equation; user parameters add named values of their own. Equations are
//! evaluated after every edit, in dependency order, and their values written into the document.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tenon_sketch::ConstraintId;

use crate::FeatureId;
use crate::document::{Document, FeatureKind};
use crate::expr;

/// What a parameter's number means: a length (mm), an angle (degrees, stored in radians) or a
/// plain count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamUnit {
    Mm,
    Deg,
    Ul,
}

impl ParamUnit {
    pub fn label(self) -> &'static str {
        match self {
            ParamUnit::Mm => "mm",
            ParamUnit::Deg => "deg",
            ParamUnit::Ul => "ul",
        }
    }
}

/// Where a model parameter's value lives.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ValuePath {
    /// A number in a feature's definition, by JSON pointer (e.g. `/extent/distance`).
    Feature { feature: FeatureId, field: String },
    /// A driving dimension of a sketch.
    Dimension { sketch: FeatureId, constraint: ConstraintId },
}

/// A named document value, optionally driven by an equation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelParam {
    pub name: String,
    pub target: ValuePath,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equation: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

/// A value of the user's own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserParam {
    pub name: String,
    pub equation: String,
    pub unit: ParamUnit,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Parameters {
    #[serde(default)]
    pub model: Vec<ModelParam>,
    #[serde(default)]
    pub user: Vec<UserParam>,
    /// The number of the next `dN` name.
    #[serde(default)]
    pub next: u32,
}

/// Most user parameters (hostile-input cap).
pub const MAX_USER_PARAMS: usize = 10_000;

/// The numbers of each feature type that are parameters: JSON pointer and unit. Pointers that do
/// not exist in a given definition (another extent, another chamfer method) are skipped.
fn fields(kind: &FeatureKind) -> &'static [(&'static str, ParamUnit)] {
    use ParamUnit::*;
    match kind {
        FeatureKind::Extrude(_) => {
            &[("/extent/distance", Mm), ("/extent/symmetric", Mm), ("/extent/two_sided/forward", Mm), ("/extent/two_sided/backward", Mm)]
        }
        FeatureKind::Revolve(_) => &[("/angle/angle", Deg), ("/angle/symmetric", Deg)],
        FeatureKind::Fillet(_) => &[("/radius", Mm)],
        FeatureKind::Chamfer(_) => &[
            ("/size/equal", Mm),
            ("/size/two_distances/d1", Mm),
            ("/size/two_distances/d2", Mm),
            ("/size/distance_angle/distance", Mm),
            ("/size/distance_angle/angle", Deg),
        ],
        FeatureKind::Shell(_) => &[("/thickness", Mm)],
        FeatureKind::Hole(_) => &[
            ("/diameter", Mm),
            ("/extent/distance", Mm),
            ("/kind/counterbore/diameter", Mm),
            ("/kind/counterbore/depth", Mm),
            ("/kind/countersink/diameter", Mm),
            ("/kind/countersink/angle", Deg),
            ("/tip_angle", Deg),
        ],
        FeatureKind::PatternRect(_) => &[("/count1", Ul), ("/spacing1", Mm), ("/count2", Ul), ("/spacing2", Mm)],
        FeatureKind::PatternCircular(_) => &[("/count", Ul), ("/angle", Deg)],
        FeatureKind::WorkPlane(_) => &[("/distance", Mm), ("/angle", Deg)],
        FeatureKind::Rib(_) => &[("/thickness", Mm), ("/extent/distance", Mm)],
        FeatureKind::Coil(_) => &[("/pitch", Mm), ("/turns", Ul)],
        FeatureKind::Draft(_) => &[("/angle", Deg)],
        FeatureKind::Sketch { .. }
        | FeatureKind::Mirror(_)
        | FeatureKind::WorkAxis(_)
        | FeatureKind::WorkPoint(_)
        | FeatureKind::Sweep(_)
        | FeatureKind::Loft(_)
        | FeatureKind::Split(_)
        | FeatureKind::Combine(_) => &[],
    }
}

/// The unit of a feature value field (a JSON pointer), if it is a parameter.
pub fn field_unit(kind: &FeatureKind, field: &str) -> Option<ParamUnit> {
    fields(kind).iter().find(|(p, _)| *p == field).map(|(_, u)| *u)
}

fn shown(v: f64, unit: ParamUnit) -> f64 {
    if unit == ParamUnit::Deg { v.to_degrees() } else { v }
}

fn stored(v: f64, unit: ParamUnit) -> f64 {
    if unit == ParamUnit::Deg { v.to_radians() } else { v }
}

/// Replaces whole-word `from` by `to` in an equation.
fn rename_in(eq: &str, from: &str, to: &str) -> String {
    let cs: Vec<char> = eq.chars().collect();
    let mut out = String::with_capacity(eq.len());
    let mut i = 0;
    while i < cs.len() {
        if cs[i].is_alphabetic() || cs[i] == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            let word: String = cs[start..i].iter().collect();
            out.push_str(if word == from { to } else { &word });
        } else {
            out.push(cs[i]);
            i += 1;
        }
    }
    out
}

impl Document {
    pub fn parameters(&self) -> &Parameters {
        &self.params
    }

    /// Every value that is a parameter, in history order, with its unit.
    pub fn value_paths(&self) -> Vec<(ValuePath, ParamUnit)> {
        let mut out = Vec::new();
        for f in self.features() {
            match &f.kind {
                FeatureKind::Sketch { sketch, .. } => {
                    for (cid, c) in sketch.constraints() {
                        if c.is_dimensional() {
                            let unit = if c.is_angular() { ParamUnit::Deg } else { ParamUnit::Mm };
                            out.push((ValuePath::Dimension { sketch: f.id, constraint: cid }, unit));
                        }
                    }
                }
                k => {
                    let json = serde_json::to_value(k).unwrap_or(Value::Null);
                    for (ptr, unit) in fields(k) {
                        if json.pointer(ptr).is_some_and(Value::is_number) {
                            out.push((ValuePath::Feature { feature: f.id, field: (*ptr).to_owned() }, *unit));
                        }
                    }
                }
            }
        }
        out
    }

    /// The value at `path` as shown (degrees for angles) and its unit.
    pub fn value_at(&self, path: &ValuePath) -> Option<(f64, ParamUnit)> {
        match path {
            ValuePath::Dimension { sketch, constraint } => {
                let c = self.sketch(*sketch)?.constraint(*constraint)?;
                let unit = if c.is_angular() { ParamUnit::Deg } else { ParamUnit::Mm };
                Some((shown(c.value()?, unit), unit))
            }
            ValuePath::Feature { feature, field } => {
                let kind = &self.feature(*feature)?.kind;
                let unit = fields(kind).iter().find(|(p, _)| p == field)?.1;
                let v = serde_json::to_value(kind).ok()?.pointer(field)?.as_f64()?;
                Some((shown(v, unit), unit))
            }
        }
    }

    /// Sets the value at `path` (as shown: degrees for angles). Sketch dimensions re-solve.
    pub fn set_value_at(&mut self, path: &ValuePath, v: f64) -> Result<(), String> {
        match path {
            ValuePath::Dimension { sketch, constraint } => {
                let sk = self.sketch_mut(*sketch).ok_or("the sketch no longer exists")?;
                let angular = sk.constraint(*constraint).is_some_and(|c| c.is_angular());
                sk.set_dimension(*constraint, if angular { v.to_radians() } else { v }).map_err(|e| e.to_string())
            }
            ValuePath::Feature { feature, field } => {
                let f = self.feature_mut(*feature).ok_or("the feature no longer exists")?;
                let unit = fields(&f.kind).iter().find(|(p, _)| p == field).ok_or("not a parameter")?.1;
                let mut json = serde_json::to_value(&f.kind).map_err(|e| e.to_string())?;
                let slot = json.pointer_mut(field).ok_or("not a parameter")?;
                *slot = match unit {
                    // Counts are whole numbers; other unitless values (a coil's turns) need not be.
                    ParamUnit::Ul if slot.is_u64() => {
                        if v < 0.0 || v.fract().abs() > 1e-9 {
                            return Err(format!("{v} is not a whole number"));
                        }
                        Value::from(v.round() as u64)
                    }
                    u => serde_json::Number::from_f64(stored(v, u)).map(Value::Number).ok_or("not a finite number")?,
                };
                f.kind = serde_json::from_value(json).map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    }

    /// The parameter name of a value, if it has one.
    pub fn name_of(&self, path: &ValuePath) -> Option<&str> {
        self.params.model.iter().find(|m| m.target == *path).map(|m| m.name.as_str())
    }

    fn name_taken(&self, name: &str) -> bool {
        self.params.model.iter().any(|m| m.name == name) || self.params.user.iter().any(|u| u.name == name)
    }

    /// Gives every value a name and forgets names of values that are gone.
    pub fn name_values(&mut self) {
        let paths = self.value_paths();
        let live: BTreeSet<&ValuePath> = paths.iter().map(|p| &p.0).collect();
        self.params.model.retain(|m| live.contains(&m.target));
        let named: BTreeSet<ValuePath> = self.params.model.iter().map(|m| m.target.clone()).collect();
        for (p, _) in paths {
            if !named.contains(&p) {
                let mut name = format!("d{}", self.params.next);
                while self.name_taken(&name) {
                    self.params.next += 1;
                    name = format!("d{}", self.params.next);
                }
                self.params.next += 1;
                self.params.model.push(ModelParam { name, target: p, equation: None, comment: String::new() });
            }
        }
    }

    /// Current parameter values (as shown) by name.
    pub fn parameter_values(&self) -> BTreeMap<String, f64> {
        let mut env = BTreeMap::new();
        for m in &self.params.model {
            if let Some((v, _)) = self.value_at(&m.target) {
                env.insert(m.name.clone(), v);
            }
        }
        // User parameters as last evaluated: recompute them in order.
        for u in &self.params.user {
            if let Ok(v) = expr::parse(&u.equation).and_then(|e| e.eval(&env)) {
                env.insert(u.name.clone(), v);
            }
        }
        env
    }

    /// Names the values, then evaluates every equation in dependency order and writes the results.
    /// Fails on an unknown name, a cycle or a value the document refuses.
    pub fn sync_parameters(&mut self) -> Result<(), String> {
        self.name_values();
        // name -> (equation, target)
        let mut eqs: BTreeMap<String, (expr::Expr, Option<ValuePath>)> = BTreeMap::new();
        let mut env: BTreeMap<String, f64> = BTreeMap::new();
        for m in &self.params.model {
            match &m.equation {
                Some(e) => {
                    let parsed = expr::parse(e).map_err(|err| format!("{}: {err}", m.name))?;
                    eqs.insert(m.name.clone(), (parsed, Some(m.target.clone())));
                }
                None => {
                    if let Some((v, _)) = self.value_at(&m.target) {
                        env.insert(m.name.clone(), v);
                    }
                }
            }
        }
        for u in &self.params.user {
            let parsed = expr::parse(&u.equation).map_err(|err| format!("{}: {err}", u.name))?;
            eqs.insert(u.name.clone(), (parsed, None));
        }
        // Evaluate whatever is ready until nothing is left; what remains is a cycle or unknown.
        while !eqs.is_empty() {
            let ready: Vec<String> = eqs
                .iter()
                .filter(|(_, (e, _))| {
                    let mut names = Vec::new();
                    e.names(&mut names);
                    names.iter().all(|n| env.contains_key(n))
                })
                .map(|(n, _)| n.clone())
                .collect();
            if ready.is_empty() {
                let (name, (e, _)) = eqs.iter().next().ok_or("no equation")?;
                let mut names = Vec::new();
                e.names(&mut names);
                let all: BTreeSet<&String> = self.params.model.iter().map(|m| &m.name).chain(self.params.user.iter().map(|u| &u.name)).collect();
                return Err(match names.iter().find(|n| !all.contains(n)) {
                    Some(missing) => format!("{name}: there is no parameter `{missing}`"),
                    None => format!("{name}: the equations refer to each other in a circle"),
                });
            }
            for name in ready {
                let Some((e, target)) = eqs.remove(&name) else { continue };
                let v = e.eval(&env).map_err(|err| format!("{name}: {err}"))?;
                if let Some(path) = target
                    && self.value_at(&path).is_none_or(|(old, _)| (old - v).abs() > 1e-12 * (1.0 + v.abs()))
                {
                    self.set_value_at(&path, v).map_err(|err| format!("{name} = {v}: {err}"))?;
                }
                env.insert(name, v);
            }
        }
        Ok(())
    }

    /// Sets (or with `None` clears) the equation of a parameter. A model parameter without an
    /// equation keeps its plain value; a user parameter always has one.
    pub fn set_equation(&mut self, name: &str, equation: Option<&str>) -> Result<(), String> {
        if let Some(e) = equation {
            expr::parse(e).map_err(|err| format!("{name}: {err}"))?;
        }
        if let Some(m) = self.params.model.iter_mut().find(|m| m.name == name) {
            m.equation = equation.map(str::to_owned);
            return Ok(());
        }
        if let Some(u) = self.params.user.iter_mut().find(|u| u.name == name) {
            u.equation = equation.ok_or("a user parameter needs an equation")?.to_owned();
            return Ok(());
        }
        Err(format!("there is no parameter `{name}`"))
    }

    pub fn set_comment(&mut self, name: &str, comment: &str) -> Result<(), String> {
        if comment.len() > 1_000 {
            return Err("the comment is too long".into());
        }
        if let Some(m) = self.params.model.iter_mut().find(|m| m.name == name) {
            m.comment = comment.to_owned();
        } else if let Some(u) = self.params.user.iter_mut().find(|u| u.name == name) {
            u.comment = comment.to_owned();
        } else {
            return Err(format!("there is no parameter `{name}`"));
        }
        Ok(())
    }

    /// Clears the equation of the value at `path` (it was set directly).
    pub fn clear_equation_at(&mut self, path: &ValuePath) {
        if let Some(m) = self.params.model.iter_mut().find(|m| m.target == *path) {
            m.equation = None;
        }
    }

    pub fn add_user_parameter(&mut self, p: UserParam) -> Result<(), String> {
        if !expr::valid_name(&p.name) {
            return Err(format!("`{}` cannot be a parameter name: use letters, digits and _, starting with a letter", p.name));
        }
        if self.name_taken(&p.name) {
            return Err(format!("there is already a parameter `{}`", p.name));
        }
        if self.params.user.len() >= MAX_USER_PARAMS {
            return Err("too many parameters".into());
        }
        expr::parse(&p.equation).map_err(|err| format!("{}: {err}", p.name))?;
        self.params.user.push(p);
        Ok(())
    }

    /// Renames a parameter and every use of it in equations.
    pub fn rename_parameter(&mut self, from: &str, to: &str) -> Result<(), String> {
        if !expr::valid_name(to) {
            return Err(format!("`{to}` cannot be a parameter name: use letters, digits and _, starting with a letter"));
        }
        if from != to && self.name_taken(to) {
            return Err(format!("there is already a parameter `{to}`"));
        }
        let mut found = false;
        for m in &mut self.params.model {
            if m.name == from {
                m.name = to.to_owned();
                found = true;
            }
            if let Some(e) = &mut m.equation {
                *e = rename_in(e, from, to);
            }
        }
        for u in &mut self.params.user {
            if u.name == from {
                u.name = to.to_owned();
                found = true;
            }
            u.equation = rename_in(&u.equation, from, to);
        }
        if found { Ok(()) } else { Err(format!("there is no parameter `{from}`")) }
    }

    /// Deletes a user parameter that no equation uses.
    pub fn delete_user_parameter(&mut self, name: &str) -> Result<(), String> {
        let users: Vec<&str> = self
            .params
            .model
            .iter()
            .filter_map(|m| m.equation.as_deref().map(|e| (m.name.as_str(), e)))
            .chain(self.params.user.iter().map(|u| (u.name.as_str(), u.equation.as_str())))
            .filter(|(n, e)| {
                *n != name
                    && expr::parse(e).is_ok_and(|x| {
                        let mut v = Vec::new();
                        x.names(&mut v);
                        v.iter().any(|w| w == name)
                    })
            })
            .map(|(n, _)| n)
            .collect();
        if !users.is_empty() {
            return Err(format!("{name} is used by {}", users.join(", ")));
        }
        let i = self.params.user.iter().position(|u| u.name == name).ok_or_else(|| format!("there is no user parameter `{name}`"))?;
        self.params.user.remove(i);
        Ok(())
    }

    /// A readable name for where a value lives ("Extrusion1 distance", "Sketch1 dimension").
    pub fn describe_path(&self, path: &ValuePath) -> String {
        match path {
            ValuePath::Dimension { sketch, constraint } => {
                let s = self.feature(*sketch).map_or("?", |f| f.name.as_str());
                let what = self.sketch(*sketch).and_then(|sk| sk.constraint(*constraint)).map_or("dimension", |c| c.name());
                format!("{s} {what}")
            }
            ValuePath::Feature { feature, field } => {
                let f = self.feature(*feature).map_or("?", |f| f.name.as_str());
                let what = field.trim_start_matches('/').replace(['/', '_'], " ");
                format!("{f} {what}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn renames_whole_words_only() {
        assert_eq!(rename_in("d1 * 2 + d10 - d1x", "d1", "width"), "width * 2 + d10 - d1x");
    }
}

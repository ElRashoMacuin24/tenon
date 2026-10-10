//! What changed between two documents (`tenon-cli diff`, the `file.diff` command): the parameters
//! and features of parts, the components and relationships of assemblies, the sheets, views and
//! annotations of drawings. Either file may be of any format version. Computed values (reference
//! fingerprints) are not compared and numbers are compared to `tol::FILE_DECIMALS`, so a
//! version-1 file and its upgrade are the same.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Map, Value, json};
use tenon_geom::tol;

use crate::layout::{self, Shape};
use crate::project::{self, ProjectError};

/// One change.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// "Document", "Parameters", "Features", "Components", ...
    pub section: &'static str,
    /// `+` added, `-` removed, `~` changed, `>` moved.
    pub op: char,
    /// What changed: "Extrusion1 [2]", a parameter's name.
    pub item: String,
    /// How: "extent distance 30 -> 35 (from H)".
    pub detail: String,
}

/// The changes from one document to another of the same kind.
#[derive(Clone, Debug, PartialEq)]
pub struct Diff {
    /// "part", "assembly" or "drawing".
    pub what: &'static str,
    pub changes: Vec<Change>,
}

impl Diff {
    pub fn same(&self) -> bool {
        self.changes.is_empty()
    }

    /// One section heading, then one line per change, items lined up.
    pub fn text(&self) -> String {
        if self.same() {
            return "No changes.".into();
        }
        let mut out = String::new();
        let mut sections: Vec<&str> = Vec::new();
        for c in &self.changes {
            if !sections.contains(&c.section) {
                sections.push(c.section);
            }
        }
        for s in sections {
            let rows: Vec<&Change> = self.changes.iter().filter(|c| c.section == s).collect();
            let width = rows.iter().map(|c| c.item.chars().count()).max().unwrap_or(0);
            out.push_str(s);
            out.push('\n');
            for c in rows {
                let line = match (c.item.is_empty(), c.detail.is_empty()) {
                    (_, true) => format!("  {} {}", c.op, c.item),
                    (true, false) => format!("  {} {}", c.op, c.detail),
                    (false, false) => format!("  {} {:width$}  {}", c.op, c.item, c.detail),
                };
                out.push_str(line.trim_end());
                out.push('\n');
            }
        }
        out.trim_end().to_owned()
    }

    pub fn json(&self) -> Value {
        let op = |c: char| match c {
            '+' => "added",
            '-' => "removed",
            '>' => "moved",
            _ => "changed",
        };
        let changes: Vec<Value> =
            self.changes.iter().map(|c| json!({ "section": c.section, "op": op(c.op), "item": c.item, "detail": c.detail })).collect();
        json!({ "what": self.what, "same": self.same(), "changes": changes })
    }
}

/// The changes from the file at `a` to the file at `b`.
pub fn diff_files(a: &Path, b: &Path) -> Result<Diff, ProjectError> {
    let read = |p: &Path| -> Result<Vec<u8>, ProjectError> {
        if std::fs::metadata(p)?.len() > project::MAX_FILE {
            return Err(ProjectError::NotAProject(format!("{} is too large", p.display())));
        }
        Ok(std::fs::read(p)?)
    };
    let named = |p: &Path, e: ProjectError| match e {
        ProjectError::NotAProject(m) => ProjectError::NotAProject(format!("{}: {m}", p.display())),
        ProjectError::Damaged(m) => ProjectError::Damaged(format!("{}: {m}", p.display())),
        ProjectError::Io(e) => ProjectError::Io(std::io::Error::new(e.kind(), format!("{}: {e}", p.display()))),
        e => e,
    };
    let da = read(a).map_err(|e| named(a, e))?;
    let db = read(b).map_err(|e| named(b, e))?;
    let (wa, ba) = document(&da).map_err(|e| named(a, e))?;
    let (wb, bb) = document(&db).map_err(|e| named(b, e))?;
    if wa != wb {
        let an = |w: &str| if w.starts_with('a') { "an" } else { "a" };
        return Err(ProjectError::NotAProject(format!(
            "{} is {} {wa} and {} {} {wb}: only documents of one kind compare",
            a.display(),
            an(wa),
            b.display(),
            an(wb)
        )));
    }
    Ok(diff_documents(wa, &ba, &bb))
}

/// What a file holds and its document, in memory shape, read through the model so that fields a
/// version left out compare as their defaults.
fn document(data: &[u8]) -> Result<(&'static str, Value), ProjectError> {
    let kinds: [(&str, &Shape, u32); 3] = [
        (project::FORMAT, &layout::PART, project::VERSION),
        (crate::asm::FORMAT, &layout::ASSEMBLY, crate::asm::VERSION),
        (crate::drw::FORMAT, &layout::DRAWING, crate::drw::VERSION),
    ];
    for (format, shape, version) in kinds {
        match project::read_head(data, format, shape, version) {
            Ok((head, _)) => {
                let body = head[shape.body].clone();
                let through = |e: serde_json::Error| ProjectError::Damaged(e.to_string());
                let body = match shape.what {
                    "part" => serde_json::to_value(serde_json::from_value::<tenon_model::Document>(body).map_err(through)?),
                    "assembly" => serde_json::to_value(serde_json::from_value::<tenon_assembly::Assembly>(body).map_err(through)?),
                    _ => serde_json::to_value(serde_json::from_value::<tenon_drawing::Drawing>(body).map_err(through)?),
                };
                return Ok((shape.what, body.map_err(|e| ProjectError::Damaged(e.to_string()))?));
            }
            Err(ProjectError::NotAProject(m)) if m.starts_with("this is a") => {}
            Err(e) => return Err(e),
        }
    }
    Err(ProjectError::NotAProject("not a Tenon part, assembly or drawing".into()))
}

/// The changes from document `a` to document `b` (in memory shape) of kind `what`.
pub fn diff_documents(what: &'static str, a: &Value, b: &Value) -> Diff {
    let mut out = Vec::new();
    match what {
        "part" => part(a, b, &mut out),
        "assembly" => assembly(a, b, &mut out),
        _ => drawing(a, b, &mut out),
    }
    Diff { what, changes: out }
}

// ---- parts ------------------------------------------------------------------------------------

fn part(a: &Value, b: &Value, out: &mut Vec<Change>) {
    let doc = |c: &mut Vec<Change>, detail: String| c.push(Change { section: "Document", op: '~', item: String::new(), detail });
    if a["name"] != b["name"] {
        doc(out, format!("name {} -> {}", show(&a["name"]), show(&b["name"])));
    }
    if a["end_before"] != b["end_before"] {
        let at = |d: &Value| match d["end_before"].as_u64() {
            Some(id) => format!("before {}", feature_label(d, id)),
            None => "at the end".into(),
        };
        doc(out, format!("End of Part {} -> {}", at(a), at(b)));
    }
    let changed = parameters(&a["params"], &b["params"], out);
    let params = &b["params"]["model"];
    let label = |f: &Value| label(f);
    let summary = |f: &Value| feature_summary(f);
    let detail = |x: &Value, y: &Value| feature_detail(x, y, params, &changed);
    records("Features", &a["features"], &b["features"], &label, &summary, &detail, out);
}

/// Parameter changes; returns the names whose values may have changed.
fn parameters(a: &Value, b: &Value, out: &mut Vec<Change>) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();
    let by_name = |v: &Value| -> BTreeMap<String, Value> {
        v.as_array().into_iter().flatten().filter_map(|p| Some((p["name"].as_str()?.to_owned(), p.clone()))).collect()
    };
    let (ua, ub) = (by_name(&a["user"]), by_name(&b["user"]));
    let push = |out: &mut Vec<Change>, op, item: &str, detail: String| out.push(Change { section: "Parameters", op, item: item.into(), detail });
    let note = |p: &Value| p["comment"].as_str().filter(|c| !c.is_empty()).map_or(String::new(), |c| format!(" ({c})"));
    for name in ua.keys() {
        if !ub.contains_key(name) {
            push(out, '-', name, String::new());
            changed.insert(name.clone());
        }
    }
    for (name, q) in &ub {
        match ua.get(name) {
            None => {
                push(out, '+', name, format!("= {}{}", show(&q["equation"]), note(q)));
                changed.insert(name.clone());
            }
            Some(p) if !same(p, q) => {
                let mut parts = Vec::new();
                if !same(&p["equation"], &q["equation"]) {
                    parts.push(format!("{} -> {}{}", show(&p["equation"]), show(&q["equation"]), note(q)));
                    changed.insert(name.clone());
                }
                for f in ["unit", "comment"] {
                    if !same(&p[f], &q[f]) {
                        parts.push(format!("{f} {} -> {}", show(&p[f]), show(&q[f])));
                    }
                }
                push(out, '~', name, parts.join("; "));
            }
            Some(_) => {}
        }
    }
    // A named model value whose equation changed (added and removed ones come and go with
    // their features).
    let (ma, mb) = (by_name(&a["model"]), by_name(&b["model"]));
    for (name, q) in &mb {
        if let Some(p) = ma.get(name)
            && !same(&p["equation"], &q["equation"])
        {
            push(out, '~', name, format!("equation {} -> {}", show(&p["equation"]), show(&q["equation"])));
            changed.insert(name.clone());
        }
    }
    changed
}

fn feature_label(doc: &Value, id: u64) -> String {
    doc["features"].as_array().into_iter().flatten().find(|f| f["id"].as_u64() == Some(id)).map_or_else(|| format!("[{id}]"), label)
}

fn feature_summary(f: &Value) -> String {
    let k = &f["kind"];
    let ty = k["type"].as_str().unwrap_or("?").replace('_', " ");
    if ty == "sketch" {
        let n = |v: &Value| v.as_array().map_or(0, Vec::len);
        return format!(
            "sketch, {}, {}",
            count(n(&k["sketch"]["entities"]), "entity", "entities"),
            count(n(&k["sketch"]["constraints"]), "constraint", "constraints")
        );
    }
    let mut parts = vec![ty];
    for (key, v) in k.as_object().into_iter().flatten() {
        match v {
            Value::Number(_) => parts.push(format!("{} {}", words_of(key), show(v))),
            Value::Array(a) if !a.is_empty() => parts.push(format!("{} {}", a.len(), words_of(key))),
            _ => {}
        }
    }
    parts.join(", ")
}

fn feature_detail(a: &Value, b: &Value, params: &Value, changed: &BTreeSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    if a["name"] != b["name"] {
        out.push(format!("renamed {} -> {}", show(&a["name"]), show(&b["name"])));
    }
    if a["suppressed"] != b["suppressed"] {
        out.push(if b["suppressed"].as_bool() == Some(true) { "suppressed".into() } else { "no longer suppressed".into() });
    }
    let (ka, kb) = (&a["kind"], &b["kind"]);
    if ka["type"] != kb["type"] {
        out.push(format!("type {} -> {}", show(&ka["type"]), show(&kb["type"])));
        return out;
    }
    let id = b["id"].as_u64().unwrap_or(0);
    let from = |target: &Value| from_note(params, target, changed);
    if kb["type"] == "sketch" {
        let mut plane = Vec::new();
        leaves(&mut Vec::new(), &mut Vec::new(), &ka["plane"], &kb["plane"], &mut plane);
        if !plane.is_empty() {
            out.push(format!("plane {} -> {}", show(&ka["plane"]), show(&kb["plane"])));
        }
        out.extend(sketch_detail(&ka["sketch"], &kb["sketch"], id, params, changed));
        return out;
    }
    let mut found = Vec::new();
    leaves(&mut Vec::new(), &mut Vec::new(), ka, kb, &mut found);
    for (path, pointer, x, y) in found {
        let note = from(&json!({ "kind": "feature", "feature": id, "field": pointer }));
        out.push(format!("{path} {} -> {}{note}", show(&x), show(&y)));
    }
    out
}

fn sketch_detail(a: &Value, b: &Value, sketch: u64, params: &Value, changed: &BTreeSet<String>) -> Vec<String> {
    let from = |target: &Value| from_note(params, target, changed);
    let pairs = |v: &Value| -> BTreeMap<u64, Value> {
        v.as_array().into_iter().flatten().filter_map(|p| Some((p.get(0)?.as_u64()?, p.get(1)?.clone()))).collect()
    };
    let mut out = Vec::new();
    let (ea, eb) = (pairs(&a["entities"]), pairs(&b["entities"]));
    let added = eb.keys().filter(|k| !ea.contains_key(k)).count();
    let removed = ea.keys().filter(|k| !eb.contains_key(k)).count();
    let (mut moved, mut other) = (0, 0);
    for (k, y) in &eb {
        if let Some(x) = ea.get(k)
            && !same(x, y)
        {
            let point = x["geometry"]["type"] == "point" && y["geometry"]["type"] == "point" && x["construction"] == y["construction"];
            if point { moved += 1 } else { other += 1 }
        }
    }
    for (n, one, many, how) in [
        (added, "entity", "entities", "added"),
        (removed, "entity", "entities", "removed"),
        (other, "entity", "entities", "changed"),
        (moved, "point", "points", "moved"),
    ] {
        if n > 0 {
            out.push(format!("{n} {} {how}", if n == 1 { one } else { many }));
        }
    }
    let (ca, cb) = (pairs(&a["constraints"]), pairs(&b["constraints"]));
    let added = cb.keys().filter(|k| !ca.contains_key(k)).count();
    let removed = ca.keys().filter(|k| !cb.contains_key(k)).count();
    let mut other = 0;
    for (k, y) in &cb {
        let Some(x) = ca.get(k) else { continue };
        if same(x, y) {
            continue;
        }
        let mut xv = x.clone();
        if let (Some(o), Some(v)) = (xv.as_object_mut(), y.get("value")) {
            o.insert("value".into(), v.clone());
        }
        if x.get("value").is_some() && same(&xv, y) {
            let target = json!({ "kind": "dimension", "sketch": sketch, "constraint": k });
            let name = param(params, &target).and_then(|p| p["name"].as_str()).map_or_else(|| format!("#{k}"), str::to_owned);
            out.push(format!("dimension {name} {} -> {}{}", show(&x["value"]), show(&y["value"]), from(&target)));
        } else {
            other += 1;
        }
    }
    for (n, how) in [(added, "added"), (removed, "removed"), (other, "changed")] {
        if n > 0 {
            out.push(format!("{n} {} {how}", if n == 1 { "constraint" } else { "constraints" }));
        }
    }
    // Dimensions in both whose value is shown somewhere else (placed, moved, or back beside the
    // geometry).
    let (pa, pb) = (pairs(&a["places"]), pairs(&b["places"]));
    let moved =
        cb.keys().filter(|k| ca.contains_key(k)).filter(|k| !same(pa.get(k).unwrap_or(&Value::Null), pb.get(k).unwrap_or(&Value::Null))).count();
    if moved > 0 {
        out.push(format!("{moved} {} moved", if moved == 1 { "dimension" } else { "dimensions" }));
    }
    out
}

/// The named model value (`d3`) that sets `target`.
fn param<'a>(params: &'a Value, target: &Value) -> Option<&'a Value> {
    params.as_array().into_iter().flatten().find(|p| same(&p["target"], target))
}

/// " (from H)" when the value at `target` follows an equation naming a changed parameter, or
/// " (= EQ)" when its own equation is new.
fn from_note(params: &Value, target: &Value, changed: &BTreeSet<String>) -> String {
    let Some(p) = param(params, target) else { return String::new() };
    let name = p["name"].as_str().unwrap_or("");
    let Some(eq) = p["equation"].as_str() else { return String::new() };
    if changed.contains(name) {
        return format!(" (= {eq})");
    }
    let names: Vec<&str> = identifiers(eq).filter(|n| changed.contains(*n)).collect();
    if names.is_empty() { String::new() } else { format!(" (from {})", names.join(", ")) }
}

/// The names in an equation, each once, in order.
fn identifiers(eq: &str) -> impl Iterator<Item = &str> {
    let mut seen = BTreeSet::new();
    eq.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| w.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_'))
        .filter(move |w| seen.insert(*w))
}

// ---- assemblies and drawings ------------------------------------------------------------------

fn assembly(a: &Value, b: &Value, out: &mut Vec<Change>) {
    fields("Document", a, b, &["name"], out);
    let summary = |c: &Value| show(&c["part"]);
    let detail = |x: &Value, y: &Value| {
        let mut d = generic_detail(x, y, &["placement"]);
        if !same(&x["placement"], &y["placement"]) {
            d.push("moved".into());
        }
        d
    };
    records("Components", &a["components"], &b["components"], &label, &summary, &detail, out);
    let summary = |r: &Value| r["kind"]["type"].as_str().unwrap_or("?").replace('_', " ");
    let detail = |x: &Value, y: &Value| generic_detail(x, y, &[]);
    records("Relationships", &a["relationships"], &b["relationships"], &label, &summary, &detail, out);
    records("Explode", &a["explode"], &b["explode"], &label, &|_| String::new(), &detail, out);
}

fn drawing(a: &Value, b: &Value, out: &mut Vec<Change>) {
    fields("Document", a, b, &["name", "standard", "props"], out);
    let detail = |x: &Value, y: &Value| generic_detail(x, y, &[]);
    let summary = |s: &Value| show(&s["size"]["name"]);
    records("Sheets", &a["sheets"], &b["sheets"], &label, &summary, &detail, out);
    let summary = |v: &Value| format!("{}, {}", v["kind"]["type"].as_str().unwrap_or("?"), show(&v["model"]));
    let detail = |x: &Value, y: &Value| {
        let mut d = generic_detail(x, y, &["center"]);
        if !same(&x["center"], &y["center"]) {
            d.push("moved".into());
        }
        d
    };
    records("Views", &a["views"], &b["views"], &label, &summary, &detail, out);
    let summary = |n: &Value| n["kind"]["type"].as_str().unwrap_or("?").replace('_', " ");
    let detail = |x: &Value, y: &Value| generic_detail(x, y, &[]);
    records("Annotations", &a["annotations"], &b["annotations"], &label, &summary, &detail, out);
}

/// Top-level fields of a document that changed.
fn fields(section: &'static str, a: &Value, b: &Value, keys: &[&str], out: &mut Vec<Change>) {
    for k in keys {
        let mut found = Vec::new();
        leaves(&mut vec![(*k).to_owned()], &mut vec![(*k).to_owned()], &a[*k], &b[*k], &mut found);
        for (path, _, x, y) in found {
            out.push(Change { section, op: '~', item: String::new(), detail: format!("{path} {} -> {}", show(&x), show(&y)) });
        }
    }
}

/// A record's changed fields, `kind` read as part of the record, `skip` left to the caller.
fn generic_detail(a: &Value, b: &Value, skip: &[&str]) -> Vec<String> {
    let flat = |v: &Value| -> Value {
        let mut o = Map::new();
        for (k, x) in v.as_object().into_iter().flatten() {
            if k == "kind" {
                o.extend(x.as_object().into_iter().flatten().map(|(kk, vv)| (kk.clone(), vv.clone())));
            } else if k != "id" && !skip.contains(&k.as_str()) {
                o.insert(k.clone(), x.clone());
            }
        }
        Value::Object(o)
    };
    let mut found = Vec::new();
    leaves(&mut Vec::new(), &mut Vec::new(), &flat(a), &flat(b), &mut found);
    found.into_iter().map(|(path, _, x, y)| format!("{path} {} -> {}", show(&x), show(&y))).collect()
}

// ---- records, values ----------------------------------------------------------------------------

/// "Fillet1 [3]", or "[3]" for a record without a name.
fn label(r: &Value) -> String {
    match (r["name"].as_str(), r["id"].as_u64()) {
        (Some(n), Some(id)) => format!("{n} [{id}]"),
        (None, Some(id)) => format!("[{id}]"),
        (Some(n), None) => n.to_owned(),
        (None, None) => String::new(),
    }
}

/// Records matched by id (by position when they have none): removed, added, changed and moved.
fn records(
    section: &'static str,
    a: &Value,
    b: &Value,
    label: &dyn Fn(&Value) -> String,
    summary: &dyn Fn(&Value) -> String,
    detail: &dyn Fn(&Value, &Value) -> Vec<String>,
    out: &mut Vec<Change>,
) {
    let keyed = |v: &Value| -> Vec<(u64, Value)> {
        v.as_array().into_iter().flatten().enumerate().map(|(i, r)| (r["id"].as_u64().unwrap_or(i as u64 + 1), r.clone())).collect()
    };
    let (ra, rb) = (keyed(a), keyed(b));
    let ia: BTreeMap<u64, &Value> = ra.iter().map(|(k, v)| (*k, v)).collect();
    let ib: BTreeMap<u64, &Value> = rb.iter().map(|(k, v)| (*k, v)).collect();
    let item = |r: &Value, k: u64| {
        let l = label(r);
        if l.is_empty() { format!("{k}") } else { l }
    };
    for (k, r) in &ra {
        if !ib.contains_key(k) {
            out.push(Change { section, op: '-', item: item(r, *k), detail: String::new() });
        }
    }
    let common_a: Vec<u64> = ra.iter().map(|(k, _)| *k).filter(|k| ib.contains_key(k)).collect();
    let common_b: Vec<u64> = rb.iter().map(|(k, _)| *k).filter(|k| ia.contains_key(k)).collect();
    let kept = lcs(&common_a, &common_b);
    let mut previous: Option<&Value> = None;
    for (k, r) in &rb {
        match ia.get(k) {
            None => out.push(Change { section, op: '+', item: item(r, *k), detail: summary(r) }),
            Some(old) => {
                if !kept.contains(k) {
                    let after = previous.map_or_else(|| "to the start".to_owned(), |p| format!("after {}", label(p)));
                    out.push(Change { section, op: '>', item: item(r, *k), detail: format!("moved {after}") });
                }
                let d = detail(old, r);
                if !d.is_empty() {
                    out.push(Change { section, op: '~', item: item(r, *k), detail: d.join("; ") });
                }
            }
        }
        previous = Some(r);
    }
}

/// The ids of a longest common subsequence: what kept its order.
fn lcs(a: &[u64], b: &[u64]) -> BTreeSet<u64> {
    let (n, m) = (a.len(), b.len());
    let mut t = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i][j] = if a[i] == b[j] { t[i + 1][j + 1] + 1 } else { t[i + 1][j].max(t[i][j + 1]) };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, BTreeSet::new());
    while i < n && j < m {
        if a[i] == b[j] {
            out.insert(a[i]);
            i += 1;
            j += 1;
        } else if t[i + 1][j] >= t[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// The leaves that differ: (words, JSON pointer, before, after). Objects are walked; lists of
/// objects of the same length are walked item by item; anything else is a leaf. Fingerprints
/// are skipped.
fn leaves(words: &mut Vec<String>, pointer: &mut Vec<String>, a: &Value, b: &Value, out: &mut Vec<(String, String, Value, Value)>) {
    if same(a, b) {
        return;
    }
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let keys: Vec<&String> = x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))).collect();
            for k in keys {
                if k == "fingerprint" || (k == "type" && words.is_empty()) {
                    continue;
                }
                words.push(words_of(k));
                pointer.push(k.clone());
                leaves(words, pointer, x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null), out);
                words.pop();
                pointer.pop();
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() && x.iter().chain(y).all(Value::is_object) => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                words.push((i + 1).to_string());
                pointer.push(i.to_string());
                leaves(words, pointer, p, q, out);
                words.pop();
                pointer.pop();
            }
        }
        _ => out.push((words.join(" "), format!("/{}", pointer.join("/")), a.clone(), b.clone())),
    }
}

fn words_of(k: &str) -> String {
    k.replace('_', " ")
}

/// Equal, with numbers to `tol::FILE_DECIMALS` and fingerprints not compared.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(p), Some(q)) => rounded(p) == rounded(q),
            _ => x == y,
        },
        (Value::Object(x), Value::Object(y)) => {
            let keys: BTreeSet<&String> = x.keys().chain(y.keys()).filter(|k| *k != "fingerprint").collect();
            keys.into_iter().all(|k| same(x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null)))
        }
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        _ => a == b,
    }
}

fn rounded(x: f64) -> f64 {
    let r: f64 = format!("{x:.prec$}", prec = tol::FILE_DECIMALS).parse().unwrap_or(x);
    if r == 0.0 { 0.0 } else { r }
}

/// A value as a reader would write it: `30`, `0.1`, `cut`, `[7, 10]`, `none`.
fn show(v: &Value) -> String {
    let s = match v {
        Value::Null => "none".into(),
        Value::Bool(b) => if *b { "yes" } else { "no" }.into(),
        Value::Number(n) => match (n.as_u64(), n.as_i64(), n.as_f64()) {
            (Some(i), _, _) => i.to_string(),
            (_, Some(i), _) => i.to_string(),
            (_, _, Some(x)) => {
                let x = rounded(x);
                if x.fract() == 0.0 && x.abs() < 1e15 { format!("{x:.0}") } else { format!("{x}") }
            }
            _ => n.to_string(),
        },
        Value::String(s) => s.clone(),
        Value::Array(a) if a.iter().any(Value::is_object) => format!("{} items", a.len()),
        Value::Array(a) => format!("[{}]", a.iter().map(show).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => {
            let inner: Vec<String> = o.iter().filter(|(k, _)| *k != "fingerprint").map(|(k, x)| format!("{} {}", words_of(k), show(x))).collect();
            format!("{{{}}}", inner.join(", "))
        }
    };
    if s.chars().count() > 80 { format!("{}...", s.chars().take(77).collect::<String>()) } else { s }
}

/// "1 entity", "3 entities".
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

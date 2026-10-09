//! Format version 2 as text: TOML with a fixed layout (docs/file-format.md). Written here, so the
//! layout never changes with a library; read with `toml_edit`.
//!
//! Layout of a file (a JSON object in memory):
//! - plain values first, one per line (`name = "Plate"`);
//! - then each object as a `[section]`, then each list of objects as `[[record]]` tables;
//! - inside a section or record, one field per line. A list of objects puts one object per line,
//!   each ending in a comma, so adding one never touches its neighbours. The objects named as
//!   sub-tables (`sheet.title_block`) follow as `[sheet.title_block]`, laid out the same way.
//!   Anything else is written inline;
//! - empty optional values (`null`) are left out: TOML has no null.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::{Map, Number, Value};

/// Where each record starts in a file: (list name, index) to line number (from 1).
pub(crate) type Lines = BTreeMap<(String, usize), usize>;

/// A non-empty list of objects: records at the top, one per line inside them.
fn is_records(v: &Value) -> bool {
    v.as_array().is_some_and(|a| !a.is_empty() && a.iter().all(Value::is_object))
}

/// The text of `file`, after a first line of `header` (a comment). `tables` names the objects
/// inside sections and records written as sub-tables, by path (`sheet.title_block`).
pub(crate) fn write(file: &Map<String, Value>, header: &str, tables: &[&str]) -> Result<String, String> {
    let mut out = format!("# {header}\n");
    for (k, v) in file {
        if !v.is_null() && !v.is_object() && !is_records(v) {
            let _ = writeln!(out, "{} = {}", key(k), inline(v).map_err(|e| format!("{k}: {e}"))?);
        }
    }
    for (k, v) in file {
        if let Some(o) = v.as_object() {
            let _ = writeln!(out, "\n[{}]", key(k));
            fields(&mut out, o, &key(k), tables).map_err(|e| format!("{k}: {e}"))?;
        }
    }
    for (k, v) in file {
        if is_records(v) {
            for (i, r) in v.as_array().into_iter().flatten().enumerate() {
                let _ = writeln!(out, "\n[[{}]]", key(k));
                fields(&mut out, r.as_object().ok_or("not an object")?, &key(k), tables).map_err(|e| format!("{k} {}: {e}", i + 1))?;
            }
        }
    }
    Ok(out)
}

/// The fields of a section or record (at `path`), one per line; then its sub-tables.
fn fields(out: &mut String, o: &Map<String, Value>, path: &str, tables: &[&str]) -> Result<(), String> {
    let table = |k: &str, v: &Value| v.is_object() && tables.contains(&format!("{path}.{k}").as_str());
    for (k, v) in o {
        match v {
            Value::Null => {}
            v if table(k, v) => {}
            Value::Array(a) if is_records(v) => {
                let _ = writeln!(out, "{} = [", key(k));
                for item in a {
                    let _ = writeln!(out, "  {},", inline(item).map_err(|e| format!("{k}: {e}"))?);
                }
                out.push_str("]\n");
            }
            _ => {
                let _ = writeln!(out, "{} = {}", key(k), inline(v).map_err(|e| format!("{k}: {e}"))?);
            }
        }
    }
    for (k, v) in o {
        if let Some(t) = v.as_object().filter(|_| table(k, v)) {
            let sub = format!("{path}.{}", key(k));
            let _ = writeln!(out, "\n[{sub}]");
            fields(out, t, &sub, tables).map_err(|e| format!("{k}: {e}"))?;
        }
    }
    Ok(())
}

/// A value on one line.
fn inline(v: &Value) -> Result<String, String> {
    Ok(match v {
        Value::Null => return Err("an empty value inside a list".into()),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => number(n)?,
        Value::String(s) => string(s),
        Value::Array(a) => {
            let items: Result<Vec<String>, String> = a.iter().map(inline).collect();
            format!("[{}]", items?.join(", "))
        }
        Value::Object(o) => {
            let mut items = Vec::new();
            for (k, x) in o {
                if !x.is_null() {
                    items.push(format!("{} = {}", key(k), inline(x)?));
                }
            }
            if items.is_empty() { "{}".into() } else { format!("{{ {} }}", items.join(", ")) }
        }
    })
}

/// An integer as is; a float as the shortest text that reads back as the same number, always
/// with a point or an exponent (TOML tells floats from integers by that). `-0` is written `0.0`.
fn number(n: &Number) -> Result<String, String> {
    if let Some(i) = n.as_u64() {
        // TOML integers are signed 64-bit: what does not fit would not read back.
        if i > i64::MAX.unsigned_abs() {
            return Err(format!("{i} is too large for a file (2^63 - 1 at most)"));
        }
        return Ok(i.to_string());
    }
    if let Some(i) = n.as_i64() {
        return Ok(i.to_string());
    }
    let x = n.as_f64().ok_or("not a number")?;
    if !x.is_finite() {
        return Err("not a finite number".into());
    }
    Ok(if x == 0.0 { "0.0".into() } else { format!("{x:?}") })
}

/// A TOML basic string.
fn string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04X}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A key: bare when it can be, else quoted.
fn key(k: &str) -> String {
    if !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') { k.to_owned() } else { string(k) }
}

/// Reads a file's text: its fields, and where each record starts.
pub(crate) fn read(text: &str) -> Result<(Map<String, Value>, Lines), String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let doc = toml_edit::Document::parse(text).map_err(|e| {
        let at = e.span().map_or(String::new(), |s| {
            let (line, col) = position(text, s.start);
            format!("line {line}, column {col}: ")
        });
        format!("{at}{}", e.message().trim_end())
    })?;
    let mut lines = Lines::new();
    let mut out = Map::new();
    for (k, item) in doc.as_table().iter() {
        if let toml_edit::Item::ArrayOfTables(a) = item {
            for (i, t) in a.iter().enumerate() {
                if let Some(s) = t.span() {
                    lines.insert((k.to_owned(), i), position(text, s.start).0);
                }
            }
        }
        if let Some(v) = item_value(item).map_err(|e| format!("{k}: {e}"))? {
            out.insert(k.to_owned(), v);
        }
    }
    Ok((out, lines))
}

/// Line and column (from 1) of a byte offset.
fn position(text: &str, at: usize) -> (usize, usize) {
    let before = text.get(..at).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

fn item_value(item: &toml_edit::Item) -> Result<Option<Value>, String> {
    Ok(match item {
        toml_edit::Item::None => None,
        toml_edit::Item::Value(v) => Some(value(v)?),
        toml_edit::Item::Table(t) => Some(table(t.iter())?),
        toml_edit::Item::ArrayOfTables(a) => {
            let items: Result<Vec<Value>, String> = a.iter().map(|t| table(t.iter())).collect();
            Some(Value::Array(items?))
        }
    })
}

fn table<'a>(items: impl Iterator<Item = (&'a str, &'a toml_edit::Item)>) -> Result<Value, String> {
    let mut o = Map::new();
    for (k, item) in items {
        if let Some(v) = item_value(item).map_err(|e| format!("{k}: {e}"))? {
            o.insert(k.to_owned(), v);
        }
    }
    Ok(Value::Object(o))
}

fn value(v: &toml_edit::Value) -> Result<Value, String> {
    Ok(match v {
        toml_edit::Value::String(s) => Value::String(s.value().clone()),
        toml_edit::Value::Integer(i) => {
            let i = *i.value();
            if i >= 0 { Value::from(i.unsigned_abs()) } else { Value::from(i) }
        }
        toml_edit::Value::Float(f) => Value::Number(Number::from_f64(*f.value()).ok_or("not a finite number")?),
        toml_edit::Value::Boolean(b) => Value::Bool(*b.value()),
        toml_edit::Value::Datetime(_) => return Err("dates and times are not used in Tenon files".into()),
        toml_edit::Value::Array(a) => {
            let items: Result<Vec<Value>, String> = a.iter().map(value).collect();
            Value::Array(items?)
        }
        toml_edit::Value::InlineTable(t) => {
            let mut o = Map::new();
            for (k, x) in t.iter() {
                o.insert(k.to_owned(), value(x).map_err(|e| format!("{k}: {e}"))?);
            }
            Value::Object(o)
        }
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn layout_is_fixed_and_reads_back() {
        let file = obj(json!({
            "format": "tenon", "version": 2, "name": "Plate \"A\"\\1\n", "skip": null,
            "parameters": { "next": 3, "user": [{ "name": "L", "equation": "80 mm" }] },
            "feature": [
                { "id": 1, "type": "sketch", "plane": { "origin": "XY" }, "entities": [{ "id": 1, "pos": { "x": 0.5, "y": -0.0 } }], "none": null },
                { "id": 2, "points": [2, 3], "tip": null, "edges": [] },
            ],
            "empty": [],
        }));
        let text = write(&file, "Tenon part", &[]).unwrap();
        assert_eq!(
            text,
            "# Tenon part\nformat = \"tenon\"\nversion = 2\nname = \"Plate \\\"A\\\"\\\\1\\n\"\nempty = []\n\n[parameters]\nnext = 3\nuser = [\n  { name = \"L\", equation = \"80 mm\" },\n]\n\n[[feature]]\nid = 1\ntype = \"sketch\"\nplane = { origin = \"XY\" }\nentities = [\n  { id = 1, pos = { x = 0.5, y = 0.0 } },\n]\n\n[[feature]]\nid = 2\npoints = [2, 3]\nedges = []\n"
        );
        let (back, lines) = read(&text).unwrap();
        let mut expected = file.clone();
        expected.remove("skip");
        expected["feature"][0].as_object_mut().unwrap().remove("none");
        expected["feature"][1].as_object_mut().unwrap().remove("tip");
        expected["feature"][0]["entities"][0]["pos"]["y"] = json!(0.0);
        assert_eq!(Value::Object(back), Value::Object(expected));
        assert_eq!(lines.get(&("feature".into(), 0)), Some(&13));
        assert_eq!(lines.get(&("feature".into(), 1)), Some(&21));
    }

    #[test]
    fn objects_named_as_sub_tables_follow_their_record() {
        let file = obj(json!({ "sheet": [
            { "id": 1, "title_block": { "name": "ANSI", "lines": [{ "a": 1 }, { "a": 2 }], "deep": { "x": [{ "b": 1 }] } }, "border": true },
            { "id": 2, "border": false },
        ] }));
        let text = write(&file, "t", &["sheet.title_block", "sheet.title_block.deep"]).unwrap();
        assert_eq!(
            text,
            "# t\n\n[[sheet]]\nid = 1\nborder = true\n\n[sheet.title_block]\nname = \"ANSI\"\nlines = [\n  { a = 1 },\n  { a = 2 },\n]\n\n[sheet.title_block.deep]\nx = [\n  { b = 1 },\n]\n\n[[sheet]]\nid = 2\nborder = false\n"
        );
        let (back, lines) = read(&text).unwrap();
        assert_eq!(Value::Object(back), Value::Object(file));
        assert_eq!((lines.get(&("sheet".into(), 0)), lines.get(&("sheet".into(), 1))), (Some(&3), Some(&19)));
    }

    #[test]
    fn numbers_keep_their_kind_and_value() {
        for x in [80.0, 0.1, 1e-10, 1e16, -2.5, 29.999999999999996, 4769.097335529232, f64::MIN_POSITIVE] {
            let text = write(&obj(json!({ "x": x, "n": 7, "m": -3 })), "t", &[]).unwrap();
            let (back, _) = read(&text).unwrap();
            assert_eq!(back["x"].as_f64(), Some(x), "{text}");
            assert!(back["x"].is_f64() && back["n"].is_u64() && back["m"].is_i64(), "{text}");
        }
        assert!(write(&obj(json!({ "a": [1, null] })), "t", &[]).is_err());
        assert!(write(&obj(json!({ "big": u64::MAX })), "t", &[]).unwrap_err().contains("too large"));
    }

    #[test]
    fn crlf_a_byte_order_mark_and_odd_keys_read() {
        let (back, _) = read("\u{feff}# x\r\nformat = \"tenon\"\r\n\"odd key\" = 1\r\n[s]\r\na = { b = [1.5, 2] }\r\n").unwrap();
        assert_eq!(Value::Object(back), json!({ "format": "tenon", "odd key": 1, "s": { "a": { "b": [1.5, 2] } } }));
        let text = write(&obj(json!({ "odd key": "é ✓ \u{1}" })), "t", &[]).unwrap();
        assert_eq!(text, "# t\n\"odd key\" = \"é ✓ \\u0001\"\n");
        assert_eq!(read(&text).unwrap().0["odd key"], "é ✓ \u{1}");
    }

    #[test]
    fn bad_text_says_where() {
        let e = read("format = \"tenon\"\nversion = \n").unwrap_err();
        assert!(e.starts_with("line 2, column 11: "), "{e}");
        let e = read("a = 1\na = 2\n").unwrap_err();
        assert!(e.starts_with("line 2"), "{e}");
        assert!(read("when = 1979-05-27\n").unwrap_err().contains("dates"));
        assert!(read("x = nan\n").unwrap_err().contains("finite"));
    }
}

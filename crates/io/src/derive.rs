//! Parts brought into parts (Derive, DEC-133): the other file read, what it derives from read in
//! turn, and given to the document, which keeps none of it in its own file. The model crate reads
//! no files: what it rebuilds from is handed to it here, as a table on the document, the way an
//! assembly session is handed its parts.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use tenon_model::schema::arg;
use tenon_model::{CmdError, CmdResult, Derive, Document, FeatureKind, Links, Operation, Session, Source};

use crate::asm::{normalize, part_key, relative};
use crate::cmd::path_in;
use crate::project;

/// How deep parts may derive from parts: a chain longer than this is refused, in words.
const MAX_DEPTH: usize = 16;

/// What a part that leads back to itself is told, first of all.
const DERIVES_ITSELF: &str = "A part cannot derive itself, directly or through another";

/// The folder a file is in.
fn folder(file: &Path) -> PathBuf {
    let parent = file.parent().map(Path::to_path_buf).unwrap_or_default();
    if parent.as_os_str().is_empty() { PathBuf::from(".") } else { parent }
}

/// A file as a feature's path means it: whole already, or relative to the document's file (or, for
/// a document with no file, to where the program is).
fn full_path(path: &str, home: Option<&str>) -> String {
    let p = Path::new(path);
    if p.is_absolute() {
        return normalize(p).to_string_lossy().into_owned();
    }
    match home {
        Some(h) => normalize(&folder(Path::new(h)).join(p)).to_string_lossy().into_owned(),
        None => part_key(p),
    }
}

/// What tells two names of one file for one file.
fn identity(path: &str) -> String {
    std::fs::canonicalize(path).map_or_else(|_| part_key(Path::new(path)), |c| c.to_string_lossy().into_owned())
}

fn name_of(path: &str) -> String {
    Path::new(path).file_name().map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
}

/// Reads one part file, and what it derives from. `chain` holds the files that led here, for what
/// derives itself. A file that cannot be read is said to be so in words: the feature fails with
/// them, and the part that derives from it still opens.
fn source_of(path: &str, home: Option<&str>, chain: &mut Vec<String>) -> Source {
    let full = full_path(path, home);
    let said = |why: String| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        why.hash(&mut h);
        Source { document: Err(why), stamp: h.finish() }
    };
    // (Before anything looks there: a file does not send Tenon to another machine.)
    if let Err(why) = project::may_read_from(Path::new(&full), Path::new(home.unwrap_or_default())) {
        return said(format!("The file {} cannot be brought in: {why}.", name_of(&full)));
    }
    let me = identity(&full);
    if chain.contains(&me) {
        let line: Vec<String> = chain.iter().chain(std::iter::once(&me)).map(|f| name_of(f)).collect();
        return said(format!("{DERIVES_ITSELF}: {}. Choose another file.", line.join(" derives ")));
    }
    if chain.len() >= MAX_DEPTH {
        return said(format!(
            "{} is more than {MAX_DEPTH} parts down a chain of derived parts: bring the part in from nearer the end of it.",
            name_of(&full)
        ));
    }
    let bytes = match project::read_file(Path::new(&full)) {
        Ok(b) => b,
        Err(project::ProjectError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return said(format!(
                "The file {} is missing (looked for at {full}): put it back, or edit the feature and choose the file again.",
                name_of(&full)
            ));
        }
        Err(e) => return said(format!("The file {} cannot be read: {e}", name_of(&full))),
    };
    let mut doc = match project::from_bytes(&bytes) {
        Ok((doc, _)) => doc,
        Err(e) => return said(format!("The file {} cannot be brought in: {e}", name_of(&full))),
    };
    resolve(&mut doc, &full);
    chain.push(me);
    let links = read_links(&doc, Some(&full), chain);
    chain.pop();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    for s in links.sources.values() {
        s.stamp.hash(&mut h);
    }
    doc.set_links(Arc::new(links));
    Source { document: Ok(Arc::new(doc)), stamp: h.finish() }
}

/// What is wrong with a part read, or with what it derives from in turn.
fn trouble(source: &Source) -> Option<String> {
    match &source.document {
        Err(why) => Some(why.clone()),
        Ok(doc) => doc.links().sources.values().find_map(trouble),
    }
}

/// The table for a document at `home`.
fn read_links(doc: &Document, home: Option<&str>, chain: &mut Vec<String>) -> Links {
    let mut links = Links { home: home.map(str::to_owned), ..Links::default() };
    for path in doc.derived_paths() {
        let source = source_of(&path, home, chain);
        links.sources.insert(path, source);
    }
    links
}

/// The chain of a document at `home`: itself.
fn chain_from(home: Option<&str>) -> Vec<String> {
    home.map(identity).into_iter().collect()
}

/// A document's derived paths as the file has them made full paths, for one read from `file`.
pub(crate) fn resolve(doc: &mut Document, file: &str) {
    doc.map_derived(|p| full_path(p, Some(file)));
}

/// A copy of `doc` with its derived paths as a file at `file` holds them: relative to its folder.
pub fn written(doc: &Document, file: &Path) -> Document {
    let mut stored = doc.clone();
    let home = doc.links().home.clone();
    let folder = folder(&std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf()));
    stored.map_derived(|p| relative(&folder, Path::new(&full_path(p, home.as_deref()))));
    stored
}

/// Everything `doc`, read from `file`, derives from: read and given to it.
pub(crate) fn attach(doc: &mut Document, file: &Path) {
    let home = part_key(file);
    resolve(doc, &home);
    let links = read_links(doc, Some(&home), &mut chain_from(Some(&home)));
    doc.set_links(Arc::new(links));
}

/// Whether a part read, or what it derives from in turn, leads back to the file it was read for.
fn leads_back(source: &Source) -> bool {
    match &source.document {
        Err(why) => why.starts_with(DERIVES_ITSELF),
        Ok(doc) => doc.links().sources.values().any(leads_back),
    }
}

/// The document's file is now `file` (it was saved there). Saved where it already was, nothing
/// changes: what it derives from is taken on a rebuild or on opening, not on a save. Saved in a new
/// place the same holds, except where its paths now lead back to it: those fail from now on.
pub fn set_home(s: &mut Session, file: &Path) {
    let home = part_key(file);
    let known = s.document().links();
    if known.home.as_deref() == Some(home.as_str()) {
        return;
    }
    let mut links = Links { home: Some(home.clone()), ..Links::default() };
    let mut chain = chain_from(Some(&home));
    for path in s.document().derived_paths() {
        let found = source_of(&path, Some(&home), &mut chain);
        let source = known.sources.get(&path).filter(|_| !leads_back(&found)).cloned().unwrap_or(found);
        links.sources.insert(path, source);
    }
    s.set_links(Arc::new(links));
}

/// `params` of a command that writes a feature's definition with the file of a Derive made full,
/// as every other way of naming it does: a relative path is from this document's file. With no file
/// to start from, it is refused in words. None when there is nothing to change.
pub(crate) fn full_path_in_feature(s: &Session, id: &str, params: &Value) -> Result<Option<Value>, CmdError> {
    if !matches!(id, "feature.add" | "feature.update") {
        return Ok(None);
    }
    let Some(kind) = arg(params, "kind").filter(|k| k.get("type").and_then(Value::as_str) == Some("derive")) else { return Ok(None) };
    let Some(path) = kind.get("path").and_then(Value::as_str).filter(|p| !Path::new(p).is_absolute()) else { return Ok(None) };
    let Some(home) = s.document().links().home.clone() else {
        return Err(CmdError(format!(
            "`{path}` is relative, and this part has no file for it to be relative to: give the file's full path, or save the part first."
        )));
    };
    let full = full_path(path, Some(&home));
    let mut made = params.clone();
    if let Some(kind) = made.get_mut("kind").and_then(Value::as_object_mut) {
        kind.insert("path".to_owned(), Value::String(full));
    }
    Ok(Some(made))
}

/// Reads again every part file the document derives from: Rebuild takes what the files now hold.
pub fn reload(s: &mut Session) {
    let home = s.document().links().home.clone();
    if home.is_none() && s.document().derived_paths().is_empty() && s.document().links().sources.is_empty() {
        return;
    }
    let links = read_links(s.document(), home.as_deref(), &mut chain_from(home.as_deref()));
    s.set_links(Arc::new(links));
}

/// Reads what the document's features name and has not been read yet (one just added, by whatever
/// command, or changed), and forgets what is no longer named.
pub fn follow(s: &mut Session) {
    let doc = s.document();
    let paths = doc.derived_paths();
    if paths.is_empty() && doc.links().sources.is_empty() {
        return;
    }
    let known = doc.links();
    if paths.len() == known.sources.len() && paths.iter().all(|p| known.sources.contains_key(p)) {
        return;
    }
    let home = known.home.clone();
    let mut links = Links { home: home.clone(), ..Links::default() };
    let mut chain = chain_from(home.as_deref());
    for path in paths {
        let source = known.sources.get(&path).cloned().unwrap_or_else(|| source_of(&path, home.as_deref(), &mut chain));
        links.sources.insert(path, source);
    }
    s.set_links(Arc::new(links));
}

/// The file chosen for a panel being set up, read so that its preview can show it. What no feature
/// goes on to name is forgotten at the next command. `Err` says why it cannot be used.
pub fn read_for_panel(s: &mut Session, file: &Path) -> Result<String, String> {
    let home = s.document().links().home.clone();
    let path = full_path(&file.to_string_lossy(), home.as_deref());
    let source = source_of(&path, home.as_deref(), &mut chain_from(home.as_deref()));
    if let Some(why) = trouble(&source) {
        return Err(why);
    }
    let mut links = s.document().links().clone();
    links.sources.insert(path.clone(), source);
    s.set_links(Arc::new(links));
    Ok(path)
}

/// `model.derive`: the solid of another part file brought into this one.
pub(crate) fn model_derive(s: &mut Session, p: &Value) -> CmdResult {
    let file = path_in(p, "path")?;
    let home = s.document().links().home.clone();
    let path = full_path(&file.to_string_lossy(), home.as_deref());
    // Now, rather than when it is rebuilt: a command that cannot work says so.
    let source = source_of(&path, home.as_deref(), &mut chain_from(home.as_deref()));
    if let Some(why) = trouble(&source) {
        return Err(CmdError(why));
    }
    let scale = match arg(p, "scale") {
        None | Some(Value::Null) => 1.0,
        Some(v) => v.as_f64().ok_or("`scale` must be a number")?,
    };
    let mirror = if arg(p, "mirror").is_some_and(|v| !v.is_null()) { Some(tenon_model::cmd::plane_value(p, "mirror")?) } else { None };
    let operation = match arg(p, "operation").and_then(Value::as_str).unwrap_or("join") {
        "join" => Operation::Join,
        "new_body" => Operation::NewBody,
        other => return Err(CmdError(format!("unknown operation `{other}` (join, new_body)"))),
    };
    let derive = Derive { path: path.clone(), scale, mirror, operation };
    derive.check().map_err(CmdError)?;
    let mut links = s.document().links().clone();
    links.sources.insert(path, source);
    s.set_links(Arc::new(links));
    tenon_model::cmd::add_feature(s, FeatureKind::Derive(derive), p)
}

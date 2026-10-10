//! The native part file (`.tenon`, docs/file-format.md) and what all Tenon documents share:
//! version 2 and later are text ([`crate::text`], laid out by [`crate::layout`]); version 1 was a
//! zip holding `project.json`, still read and upgraded in memory.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tenon_model::Document;

use crate::layout::{self, Shape};
use crate::text::{self, Lines};

/// `format` field of every project file.
pub const FORMAT: &str = "tenon";
/// Current schema version. Bump with a migration step in [`read_head`] and a round-trip test.
pub const VERSION: u32 = 2;
/// Largest document accepted (text, or `project.json` uncompressed): a zip-bomb guard.
pub const MAX_PROJECT_JSON: u64 = 64 * 1024 * 1024;
/// Largest project file accepted.
pub const MAX_FILE: u64 = 256 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("cannot read or write the file: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a Tenon project: {0}")]
    NotAProject(String),
    #[error("this project was made by a newer Tenon (format version {0}; this build reads up to {VERSION})")]
    TooNew(u32),
    #[error("the project is damaged: {0}")]
    Damaged(String),
}

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    format: String,
    version: u32,
    document: Document,
    /// Top-level fields this version does not know, kept on save.
    #[serde(flatten)]
    extra: Map<String, Value>,
}

fn zip_err(e: zip::result::ZipError) -> ProjectError {
    ProjectError::NotAProject(e.to_string())
}

/// A version-1 file: a zip holding `project.json` with `json` in it (for tests of reading old
/// files).
pub fn zip_json(json: &[u8]) -> Result<Vec<u8>, ProjectError> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zw.start_file("project.json", opts).map_err(zip_err)?;
        zw.write_all(json)?;
        zw.finish().map_err(zip_err)?;
    }
    Ok(buf.into_inner())
}

/// Whether `data` is a zip (a version-1 file).
pub fn is_version_1(data: &[u8]) -> bool {
    data.starts_with(b"PK\x03\x04")
}

/// The file text of a document held as `head` (`{format, version, <body>, unknown fields}`).
pub(crate) fn to_text(head: &Value, shape: &Shape, version: u32) -> Result<Vec<u8>, ProjectError> {
    let head = head.as_object().ok_or_else(|| ProjectError::Damaged("not an object".into()))?;
    let file = layout::to_file(head, shape, version).map_err(ProjectError::Damaged)?;
    let header = format!("Tenon {} file, format version {version}. The format: docs/file-format.md in the Tenon sources.", shape.what);
    Ok(text::write(&file, &header, shape.tables).map_err(ProjectError::Damaged)?.into_bytes())
}

/// Part file bytes for `doc`. `extra` carries unknown top-level fields from the file it was
/// opened from.
pub fn to_bytes(doc: &Document, extra: &Map<String, Value>) -> Result<Vec<u8>, ProjectError> {
    let file = ProjectFile { format: FORMAT.into(), version: VERSION, document: doc.clone(), extra: extra.clone() };
    let head = serde_json::to_value(&file).map_err(|e| ProjectError::Damaged(e.to_string()))?;
    to_text(&head, &layout::PART, VERSION)
}

/// The document in a file of any version, in memory shape (`{format, version, <body>, unknown
/// fields}`, upgraded to `max_version`), with where each record starts (empty for version 1).
/// `format` must match; a file of another kind of document is named.
pub(crate) fn read_head(data: &[u8], format: &str, shape: &Shape, max_version: u32) -> Result<(Value, Lines), ProjectError> {
    if data.len() as u64 > MAX_FILE {
        return Err(ProjectError::NotAProject("the file is too large".into()));
    }
    let (mut head, lines) = if is_version_1(data) { (zip_head(data)?, Lines::new()) } else { text_head(data, shape)? };
    match head.get("format").and_then(Value::as_str) {
        Some(f) if f == format => {}
        Some(crate::asm::FORMAT) => return Err(ProjectError::NotAProject(format!("this is an assembly (.tenonasm), not a {}", shape.what))),
        Some(crate::drw::FORMAT) => return Err(ProjectError::NotAProject(format!("this is a drawing (.tenondrw), not a {}", shape.what))),
        Some(FORMAT) => return Err(ProjectError::NotAProject(format!("this is a part (.tenon), not a {}", shape.what))),
        _ => return Err(ProjectError::NotAProject(format!("the file has no \"format\": \"{format}\""))),
    }
    let version = head.get("version").and_then(Value::as_u64).ok_or_else(|| ProjectError::Damaged("no version".into()))?;
    let version = u32::try_from(version).map_err(|_| ProjectError::Damaged("bad version".into()))?;
    if version > max_version {
        return Err(ProjectError::TooNew(version));
    }
    match (is_version_1(data), version) {
        (true, 1) => {}
        (true, _) => return Err(ProjectError::Damaged(format!("a zip holds format version 1, not {version}"))),
        (false, 0 | 1) => return Err(ProjectError::Damaged("format version 1 files are zips, not text".into())),
        (false, _) => {}
    }
    // Migrations go here, one version at a time. 1 to 2 changed only the container and the
    // layout, so the document reads the same; version 1's `generator` is not kept.
    if let Some(o) = head.as_object_mut() {
        o.shift_remove("generator");
        o.insert("version".into(), Value::from(max_version));
    }
    Ok((head, lines))
}

/// `project.json` of a version-1 zip.
fn zip_head(data: &[u8]) -> Result<Value, ProjectError> {
    let mut za = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(zip_err)?;
    let entry = za.by_name("project.json").map_err(|_| ProjectError::NotAProject("project.json is missing".into()))?;
    if entry.size() > MAX_PROJECT_JSON {
        return Err(ProjectError::Damaged("project.json is too large".into()));
    }
    let mut json = Vec::new();
    entry.take(MAX_PROJECT_JSON + 1).read_to_end(&mut json)?;
    if json.len() as u64 > MAX_PROJECT_JSON {
        return Err(ProjectError::Damaged("project.json is too large".into()));
    }
    serde_json::from_slice(&json).map_err(|e| ProjectError::Damaged(e.to_string()))
}

/// A version-2 (or later) text file, in memory shape.
fn text_head(data: &[u8], shape: &Shape) -> Result<(Value, Lines), ProjectError> {
    if data.len() as u64 > MAX_PROJECT_JSON {
        return Err(ProjectError::Damaged("the file is too large".into()));
    }
    let text = std::str::from_utf8(data).map_err(|_| ProjectError::NotAProject("the file is neither text nor a zip".into()))?;
    let (file, lines) = text::read(text).map_err(|e| {
        // Text that never meant to be a Tenon file is not "damaged".
        if text.contains("format") { ProjectError::Damaged(e) } else { ProjectError::NotAProject(e) }
    })?;
    Ok((Value::Object(layout::from_file(file, shape)), lines))
}

/// For a document that does not read: the first record of `list` (in memory name) that does not
/// read as a `T`, and where it is in the file.
pub(crate) fn locate<T: serde::de::DeserializeOwned>(head: &Value, shape: &Shape, list: &str, lines: &Lines) -> Option<ProjectError> {
    let name = shape.file_name(list);
    for (i, r) in head[shape.body][list].as_array().into_iter().flatten().enumerate() {
        if let Err(e) = serde_json::from_value::<T>(r.clone()) {
            let at = lines.get(&(name.to_owned(), i)).map_or(String::new(), |l| format!("line {l}: "));
            let what = match (r.get("id"), r.get("name").and_then(Value::as_str)) {
                (Some(id), Some(n)) => format!("{name} {id} ({n})"),
                (Some(id), None) => format!("{name} {id}"),
                _ => format!("{name} {}", i + 1),
            };
            return Some(ProjectError::Damaged(format!("{at}{what}: {e}")));
        }
    }
    None
}

/// Reads project file bytes of any version: the document plus any unknown top-level fields.
pub fn from_bytes(data: &[u8]) -> Result<(Document, Map<String, Value>), ProjectError> {
    let (head, lines) = read_head(data, FORMAT, &layout::PART, VERSION)?;
    let file: ProjectFile = serde_json::from_value(head.clone()).map_err(|e| {
        locate::<tenon_model::Feature>(&head, &layout::PART, "features", &lines).unwrap_or_else(|| ProjectError::Damaged(e.to_string()))
    })?;
    let mut document = file.document;
    document.validate().map_err(ProjectError::Damaged)?;
    // (A design table's active row is the part as it stands: the part's own values are the ones
    // that count, should a file edited by hand say two things.)
    document.table_follow();
    Ok((document, file.extra))
}

/// Writes a file atomically (a temporary file, then a rename). A version-1 file about to be
/// replaced is first kept beside it as `name.v1.ext`, once (older Tenon builds cannot read
/// version 2); that copy is returned.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<Option<PathBuf>, ProjectError> {
    let mut kept = None;
    if let Ok(mut f) = std::fs::File::open(path) {
        let mut magic = [0u8; 4];
        if f.read_exact(&mut magic).is_ok() && is_version_1(&magic) {
            let backup = version_1_copy(path);
            if !backup.exists() {
                std::fs::copy(path, &backup)?;
                kept = Some(backup);
            }
        }
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(kept)
}

/// Where a version-1 file is kept: `plate.tenon` as `plate.v1.tenon`.
pub fn version_1_copy(path: &Path) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    match path.extension() {
        Some(ext) => path.with_file_name(format!("{stem}.v1.{}", ext.to_string_lossy())),
        None => path.with_file_name(format!("{stem}.v1")),
    }
}

/// Writes `doc` to `path` atomically; returns where a version-1 original was kept, if one was.
pub fn save(path: &Path, doc: &Document, extra: &Map<String, Value>) -> Result<Option<PathBuf>, ProjectError> {
    write_file(path, &to_bytes(doc, extra)?)
}

pub fn open(path: &Path) -> Result<(Document, Map<String, Value>), ProjectError> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_FILE {
        return Err(ProjectError::NotAProject("the file is too large".into()));
    }
    from_bytes(&std::fs::read(path)?)
}

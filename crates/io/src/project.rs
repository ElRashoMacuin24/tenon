//! The native project file: a zip (`.tenon`) holding `project.json` (docs/file-format.md).

use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tenon_model::Document;

/// `format` field of every project file.
pub const FORMAT: &str = "tenon";
/// Current schema version. Bump with a migration and a round-trip test.
pub const VERSION: u32 = 1;
/// Largest `project.json` accepted, uncompressed (zip-bomb guard).
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
    #[serde(default)]
    generator: String,
    document: Document,
    /// Top-level fields this version does not know, kept on save.
    #[serde(flatten)]
    extra: Map<String, Value>,
}

fn zip_err(e: zip::result::ZipError) -> ProjectError {
    ProjectError::NotAProject(e.to_string())
}

/// Project file bytes for `doc`. `extra` carries unknown top-level fields from the file it was
/// opened from.
pub fn to_bytes(doc: &Document, extra: &Map<String, Value>) -> Result<Vec<u8>, ProjectError> {
    let file = ProjectFile {
        format: FORMAT.into(),
        version: VERSION,
        generator: format!("tenon {}", env!("CARGO_PKG_VERSION")),
        document: doc.clone(),
        extra: extra.clone(),
    };
    let json = serde_json::to_vec_pretty(&file).map_err(|e| ProjectError::Damaged(e.to_string()))?;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zw.start_file("project.json", opts).map_err(zip_err)?;
        zw.write_all(&json)?;
        zw.finish().map_err(zip_err)?;
    }
    Ok(buf.into_inner())
}

/// Reads project file bytes: the document plus any unknown top-level fields.
pub fn from_bytes(data: &[u8]) -> Result<(Document, Map<String, Value>), ProjectError> {
    if data.len() as u64 > MAX_FILE {
        return Err(ProjectError::NotAProject("the file is too large".into()));
    }
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
    let head: Value = serde_json::from_slice(&json).map_err(|e| ProjectError::Damaged(e.to_string()))?;
    if head.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(ProjectError::NotAProject("project.json has no \"format\": \"tenon\"".into()));
    }
    let version = head.get("version").and_then(Value::as_u64).ok_or_else(|| ProjectError::Damaged("no version".into()))?;
    let version = u32::try_from(version).map_err(|_| ProjectError::Damaged("bad version".into()))?;
    if version > VERSION {
        return Err(ProjectError::TooNew(version));
    }
    let file: ProjectFile = serde_json::from_value(head).map_err(|e| ProjectError::Damaged(e.to_string()))?;
    file.document.validate().map_err(ProjectError::Damaged)?;
    Ok((file.document, file.extra))
}

/// Writes `doc` to `path` atomically (temporary file, then rename).
pub fn save(path: &Path, doc: &Document, extra: &Map<String, Value>) -> Result<(), ProjectError> {
    let bytes = to_bytes(doc, extra)?;
    let tmp = path.with_extension("tenon.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn open(path: &Path) -> Result<(Document, Map<String, Value>), ProjectError> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_FILE {
        return Err(ProjectError::NotAProject("the file is too large".into()));
    }
    from_bytes(&std::fs::read(path)?)
}

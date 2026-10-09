//! Drawing files (`.tenondrw`, DEC-026) and the drawing commands that touch files: new, open,
//! save (with models changed from the drawing), a base view of a model file, update from the
//! model files, PDF/SVG/DXF export.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tenon_assembly::AsmSession;
use tenon_drawing::cmd::{DrwCommand, DrwFn};
use tenon_drawing::{Drawing, DrwModel, DrwSession, Standard};
use tenon_kernel::Kernel;
use tenon_model::{CmdError, CmdResult, Session};

use crate::asm::{normalize, part_key, relative};
use crate::project::{self, ProjectError};

/// `format` of every drawing file.
pub const FORMAT: &str = "tenon-drawing";
/// Current drawing schema version.
pub const VERSION: u32 = 1;
/// The drawing file extension.
pub const EXTENSION: &str = "tenondrw";

#[derive(Serialize, Deserialize)]
struct DrwFile {
    format: String,
    version: u32,
    #[serde(default)]
    generator: String,
    drawing: Drawing,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

/// Drawing file bytes; model paths are written relative to `dir` (the file's folder).
pub fn to_bytes(d: &Drawing, dir: &Path) -> Result<Vec<u8>, ProjectError> {
    let mut stored = d.clone();
    stored.map_models(|p| relative(dir, Path::new(p)));
    let file = DrwFile {
        format: FORMAT.into(),
        version: VERSION,
        generator: format!("tenon {}", env!("CARGO_PKG_VERSION")),
        drawing: stored,
        extra: Map::new(),
    };
    let json = serde_json::to_vec_pretty(&file).map_err(|e| ProjectError::Damaged(e.to_string()))?;
    project::zip_json(&json)
}

/// Reads drawing file bytes; model paths are resolved against `dir`.
pub fn from_bytes(data: &[u8], dir: &Path) -> Result<Drawing, ProjectError> {
    let head = project::read_head(data, FORMAT, VERSION)?;
    let file: DrwFile = serde_json::from_value(head).map_err(|e| ProjectError::Damaged(e.to_string()))?;
    let mut d = file.drawing;
    d.validate().map_err(ProjectError::Damaged)?;
    d.map_models(|p| {
        let path = Path::new(p);
        if path.is_absolute() { normalize(path).to_string_lossy().into_owned() } else { part_key(&dir.join(path)) }
    });
    Ok(d)
}

fn folder_of(path: &Path) -> PathBuf {
    let full = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let parent = full.parent().map(Path::to_path_buf).unwrap_or_default();
    if parent.as_os_str().is_empty() { PathBuf::from(".") } else { parent }
}

/// Reads a model file (part or assembly).
pub fn load_model(path: &Path) -> DrwModel {
    if path.extension().is_some_and(|e| e == crate::asm::EXTENSION) {
        match crate::asm::open(path) {
            Ok((asm, parts)) => {
                let mut s = AsmSession::default();
                s.replace(asm, parts);
                DrwModel::Assembly(Box::new(s))
            }
            Err(e) => DrwModel::Missing(e.to_string()),
        }
    } else {
        match project::open(path) {
            Ok((doc, _)) => {
                let mut s = Session::default();
                s.replace_document(doc, None);
                DrwModel::Part(Box::new(s))
            }
            Err(e) => DrwModel::Missing(e.to_string()),
        }
    }
}

/// Reads a drawing and the model files its views show.
pub fn open(path: &Path) -> Result<(Drawing, BTreeMap<String, DrwModel>), ProjectError> {
    let len = std::fs::metadata(path)?.len();
    if len > project::MAX_FILE {
        return Err(ProjectError::NotAProject("the file is too large".into()));
    }
    let d = from_bytes(&std::fs::read(path)?, &folder_of(path))?;
    let models = d.models().into_iter().map(|k| (k.clone(), load_model(Path::new(&k)))).collect();
    Ok((d, models))
}

/// Writes a drawing atomically.
pub fn save(path: &Path, d: &Drawing) -> Result<(), ProjectError> {
    let bytes = to_bytes(d, &folder_of(path))?;
    let tmp = path.with_extension("tenondrw.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Saves the models changed from the drawing to their own files. Returns their paths.
pub fn save_models(s: &mut DrwSession) -> Result<Vec<String>, CmdError> {
    let mut saved = Vec::new();
    for (key, m) in s.models.iter_mut() {
        match m {
            DrwModel::Part(p) if p.is_dirty() => {
                project::save(Path::new(key), p.document(), &Map::new()).map_err(|e| CmdError(format!("cannot save {key}: {e}")))?;
                p.mark_saved();
                saved.push(key.clone());
            }
            DrwModel::Assembly(a) if a.is_dirty() => {
                for (pk, part) in a.parts.iter_mut() {
                    if part.missing.is_none() && part.session.is_dirty() {
                        project::save(Path::new(pk), part.session.document(), &Map::new()).map_err(|e| CmdError(format!("cannot save {pk}: {e}")))?;
                        part.session.mark_saved();
                        saved.push(pk.clone());
                    }
                }
                crate::asm::save(Path::new(key), a.assembly()).map_err(|e| CmdError(format!("cannot save {key}: {e}")))?;
                a.mark_saved();
                saved.push(key.clone());
            }
            _ => {}
        }
    }
    Ok(saved)
}

// ---- commands ---------------------------------------------------------------------------------

fn path(p: &Value) -> Result<PathBuf, CmdError> {
    let s = p.get("path").and_then(Value::as_str).ok_or("missing parameter `path`")?;
    if s.is_empty() || s.len() > 4096 || s.contains('\0') {
        return Err("invalid path".into());
    }
    Ok(PathBuf::from(s))
}

fn drw_new(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let name = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Drawing1");
    if name.len() > 256 {
        return Err("`name` is too long".into());
    }
    let standard = match p.get("standard").and_then(Value::as_str) {
        None => Standard::Ansi,
        Some("ansi") => Standard::Ansi,
        Some("iso") => Standard::Iso,
        Some(_) => return Err("`standard` must be ansi or iso".into()),
    };
    let mut d = Drawing::new(name, standard);
    if let Some(size) = p.get("size").and_then(Value::as_str) {
        let size = tenon_drawing::sheets::size_named(size).ok_or("unknown sheet size (A, B, C, D, A4, A3, A2, A1)")?;
        d.sheets[0].title_block = tenon_drawing::sheets::title_block(standard, &size);
        d.sheets[0].size = size;
    }
    s.replace(d, BTreeMap::new());
    let sh = &s.drawing().sheets[0];
    Ok(json!({ "name": name, "sheet": sh.id.0, "size": sh.size.name }))
}

fn drw_open(s: &mut DrwSession, k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let path = path(p)?;
    let (d, models) = open(&path).map_err(|e| CmdError(format!("cannot open {}: {e}", path.display())))?;
    let missing: Vec<String> = models
        .iter()
        .filter_map(|(k, m)| match m {
            DrwModel::Missing(why) => Some(format!("{k}: {why}")),
            _ => None,
        })
        .collect();
    s.replace(d, models);
    if let Some(k) = k {
        s.refresh(k);
    }
    Ok(json!({ "path": path.display().to_string(), "name": s.drawing().name, "views": s.drawing().views.len(), "missing": missing }))
}

fn drw_save(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let path = path(p)?;
    let saved = save_models(s)?;
    save(&path, s.drawing()).map_err(|e| CmdError(e.to_string()))?;
    s.mark_saved();
    Ok(json!({ "path": path.display().to_string(), "models_saved": saved }))
}

/// What to build to learn a loaded model's size: its source, and a drawing with only one view of
/// it. A base view's scale is picked from the result (`tenon_drawing::cmd::add_base_view`).
pub fn probe(s: &DrwSession, key: &str) -> (BTreeMap<String, tenon_drawing::ModelSource>, Drawing) {
    let mut probe = Drawing::new("probe", s.drawing().standard);
    let sheet = probe.sheets[0].id;
    let id = probe.take_view_id();
    probe.views.push(tenon_drawing::View {
        id,
        sheet,
        name: "probe".into(),
        model: key.to_owned(),
        kind: tenon_drawing::ViewKind::Base { orientation: tenon_drawing::Orientation::Front },
        scale: 1.0,
        center: tenon_geom::Vec2::new(0.0, 0.0),
        hidden: false,
        tangent: false,
        centerlines: false,
        label: false,
    });
    let sources = s.sources().into_iter().filter(|(k, _)| k == key).collect();
    (sources, probe)
}

/// A base view of a part or assembly file (read if the drawing does not have it yet).
fn drw_base(s: &mut DrwSession, k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let file = PathBuf::from(p.get("model").and_then(Value::as_str).ok_or("missing parameter `model` (a .tenon or .tenonasm file)")?);
    let key = part_key(&file);
    if !s.models.contains_key(&key) {
        let m = load_model(&file);
        if let DrwModel::Missing(why) = &m {
            return Err(CmdError(format!("cannot open {}: {why}", file.display())));
        }
        s.models.insert(key.clone(), m);
    }
    // The model's size picks the scale: build it now when there is a kernel.
    let mut k = k;
    if let Some(k) = k.as_deref_mut() {
        let (sources, probe) = probe(s, &key);
        let ev = tenon_drawing::views::evaluate(k, &sources, &probe);
        if let Some(m) = ev.models.get(&key)
            && let Some(e) = &m.error
        {
            return Err(CmdError(format!("the model does not build: {e}")));
        }
        s.set_evaluation(0, ev);
    }
    let r = tenon_drawing::cmd::add_base_view(s, &key, p)?;
    if let Some(k) = k {
        s.refresh(k);
    }
    Ok(r)
}

/// Reads again the model files that changed on disk (models changed from the drawing and not
/// saved keep their changes); the views and dimensions follow.
fn drw_update(s: &mut DrwSession, k: Option<&mut dyn Kernel>, _p: &Value) -> CmdResult {
    let mut reread = Vec::new();
    let keys: Vec<String> = s.models.keys().cloned().collect();
    for key in keys {
        let dirty = s.models.get(&key).is_some_and(DrwModel::is_dirty);
        if dirty {
            continue;
        }
        s.models.insert(key.clone(), load_model(Path::new(&key)));
        reread.push(Path::new(&key).file_name().map_or(key.clone(), |n| n.to_string_lossy().into_owned()));
    }
    s.generation += 1;
    if let Some(k) = k {
        s.refresh(k);
    }
    // File names: what was read again, without where it lives.
    Ok(json!({ "models": reread }))
}

// ---- title block templates ----------------------------------------------------------------------

/// `format` of a title block template file (plain JSON, `.json`).
pub const TEMPLATE_FORMAT: &str = "tenon-title-block";
/// Current template schema version.
pub const TEMPLATE_VERSION: u32 = 1;
/// Largest template file read.
const MAX_TEMPLATE: u64 = 4 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct TemplateFile {
    format: String,
    version: u32,
    title_block: tenon_drawing::TitleBlock,
}

/// A title block as template file text.
pub fn template_json(tb: &tenon_drawing::TitleBlock) -> String {
    let file = TemplateFile { format: TEMPLATE_FORMAT.into(), version: TEMPLATE_VERSION, title_block: tb.clone() };
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// Reads a title block template file.
pub fn read_template(path: &Path) -> Result<tenon_drawing::TitleBlock, ProjectError> {
    if std::fs::metadata(path)?.len() > MAX_TEMPLATE {
        return Err(ProjectError::NotAProject("the template is too large".into()));
    }
    let head: Value = serde_json::from_slice(&std::fs::read(path)?).map_err(|e| ProjectError::NotAProject(e.to_string()))?;
    if head.get("format").and_then(Value::as_str) != Some(TEMPLATE_FORMAT) {
        return Err(ProjectError::NotAProject(format!("not a title block template (\"format\": \"{TEMPLATE_FORMAT}\")")));
    }
    let version = head.get("version").and_then(Value::as_u64).ok_or_else(|| ProjectError::Damaged("no version".into()))?;
    if version > u64::from(TEMPLATE_VERSION) {
        return Err(ProjectError::TooNew(u32::try_from(version).unwrap_or(u32::MAX)));
    }
    let file: TemplateFile = serde_json::from_value(head).map_err(|e| ProjectError::Damaged(e.to_string()))?;
    file.title_block.validate().map_err(ProjectError::Damaged)?;
    Ok(file.title_block)
}

fn drw_template_save(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let path = path(p)?;
    let d = s.drawing();
    let sheet = match sheet_param(p)? {
        Some(n) => d.sheets.iter().find(|x| x.id.0 == n).ok_or_else(|| CmdError(format!("sheet {n} does not exist")))?,
        None => &d.sheets[0],
    };
    let text = template_json(&sheet.title_block);
    write(&path, text.as_bytes())?;
    Ok(json!({ "path": path.display().to_string(), "name": sheet.title_block.name, "fields": sheet.title_block.fields.len() }))
}

fn drw_template_apply(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let path = path(p)?;
    let tb = read_template(&path).map_err(|e| CmdError(format!("cannot read {}: {e}", path.display())))?;
    let only = sheet_param(p)?;
    if let Some(n) = only
        && !s.drawing().sheets.iter().any(|x| x.id.0 == n)
    {
        return Err(CmdError(format!("sheet {n} does not exist")));
    }
    let name = tb.name.clone();
    let n = s.edit(|d| {
        let mut n = 0;
        for sh in d.sheets.iter_mut().filter(|x| only.is_none_or(|o| x.id.0 == o)) {
            sh.title_block = tb.clone();
            n += 1;
        }
        Ok(n)
    })?;
    Ok(json!({ "name": name, "sheets": n }))
}

/// Every sheet (or one) as graphics, from the current views.
pub fn sheets_graphics(s: &DrwSession, sheet: Option<u32>) -> Result<Vec<tenon_drawing::Graphics>, CmdError> {
    let ev = s.current()?;
    let d = s.drawing();
    let ids: Vec<tenon_drawing::SheetId> = match sheet {
        Some(n) => vec![d.sheets.iter().find(|x| x.id.0 == n).ok_or_else(|| CmdError(format!("sheet {n} does not exist")))?.id],
        None => d.sheets.iter().map(|x| x.id).collect(),
    };
    Ok(ids.into_iter().map(|id| tenon_drawing::annotate::build(d, id, ev).0).collect())
}

fn sheet_param(p: &Value) -> Result<Option<u32>, CmdError> {
    match p.get("sheet") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().and_then(|n| u32::try_from(n).ok()).map(Some).ok_or_else(|| CmdError("`sheet` must be a sheet id".into())),
    }
}

fn write(path: &Path, data: &[u8]) -> Result<(), CmdError> {
    std::fs::write(path, data).map_err(|e| CmdError(format!("cannot write {}: {e}", path.display())))
}

fn drw_export_pdf(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let path = path(p)?;
    let sheets = sheets_graphics(s, sheet_param(p)?)?;
    let data = tenon_drawing::export::pdf(&sheets);
    write(&path, &data)?;
    Ok(json!({ "path": path.display().to_string(), "pages": sheets.len(), "bytes": data.len() }))
}

fn drw_export_svg(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let path = path(p)?;
    let sheets = sheets_graphics(s, Some(sheet_param(p)?.unwrap_or(s.drawing().sheets[0].id.0)))?;
    let g = sheets.first().ok_or("no sheet")?;
    let data = tenon_drawing::export::svg(g);
    write(&path, data.as_bytes())?;
    Ok(json!({ "path": path.display().to_string(), "bytes": data.len() }))
}

fn drw_export_dxf(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let path = path(p)?;
    let sheets = sheets_graphics(s, Some(sheet_param(p)?.unwrap_or(s.drawing().sheets[0].id.0)))?;
    let g = sheets.first().ok_or("no sheet")?;
    let data = tenon_drawing::export::dxf(g);
    write(&path, data.as_bytes())?;
    Ok(
        json!({ "path": path.display().to_string(), "bytes": data.len(), "lines": g.strokes.iter().map(|s| s.2.len().saturating_sub(1)).sum::<usize>() }),
    )
}

macro_rules! cmd {
    ($id:literal, $label:literal, $help:literal, $mutates:expr, $views:expr, $f:expr) => {
        DrwCommand { id: $id, label: $label, help: $help, mutates: $mutates, views: $views, run: $f as DrwFn }
    };
}

static COMMANDS: &[DrwCommand] = &[
    cmd!(
        "drw.new",
        "New Drawing",
        "name; standard: ansi (default, third-angle) | iso (first-angle); size: A | B | C | D | A4 | A3 | A2 | A1",
        false,
        false,
        drw_new
    ),
    cmd!("drw.open", "Open Drawing", "path (.tenondrw); reads the model files its views show; clears undo history", false, false, drw_open),
    cmd!("drw.save", "Save Drawing", "path (.tenondrw); models changed from the drawing are saved to their own files first", false, false, drw_save),
    cmd!(
        "drw.view.base",
        "Base View",
        "model: a part (.tenon) or assembly (.tenonasm) file; orientation: front | back | top | bottom | left | right | iso (default front); scale: a number or \"1:2\" (default: fits the sheet); at: [x, y] (sheet mm); hidden (default true, false for iso); sheet",
        true,
        false,
        drw_base
    ),
    cmd!(
        "drw.update",
        "Update",
        "reads the model files again (models changed from the drawing and not saved keep their changes); views and dimensions follow. Returns the file names read",
        false,
        false,
        drw_update
    ),
    cmd!("drw.export.pdf", "Export PDF", "path (.pdf); sheet (default: every sheet, one page each)", false, true, drw_export_pdf),
    cmd!("drw.export.svg", "Export SVG", "path (.svg); sheet (default the first)", false, true, drw_export_svg),
    cmd!("drw.export.dxf", "Export DXF", "path (.dxf); sheet (default the first)", false, true, drw_export_dxf),
    cmd!(
        "drw.template.save",
        "Save Title Block",
        "path (.json); sheet (default the first): its title block as a template file",
        false,
        false,
        drw_template_save
    ),
    cmd!(
        "drw.template.apply",
        "Apply Title Block",
        "path: a title block template (.json, from drw.template.save or written by hand, docs/file-format.md); sheet (default: every sheet)",
        true,
        false,
        drw_template_apply
    ),
];

/// The file commands for drawings.
pub fn commands() -> &'static [DrwCommand] {
    COMMANDS
}

/// Every drawing command: these and the ones in `tenon_drawing::cmd`.
pub fn all_commands() -> impl Iterator<Item = &'static DrwCommand> {
    COMMANDS.iter().chain(tenon_drawing::cmd::commands().iter())
}

/// Runs any drawing command by id.
pub fn run(s: &mut DrwSession, id: &str, params: &Value, kernel: Option<&mut dyn Kernel>) -> CmdResult {
    let spec =
        COMMANDS.iter().find(|c| c.id == id).or_else(|| tenon_drawing::cmd::find(id)).ok_or_else(|| CmdError(format!("unknown command `{id}`")))?;
    tenon_drawing::cmd::run(s, spec, params, kernel)
}

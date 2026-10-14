//! Assembly files (`.tenonasm`, DEC-024) and the assembly commands that touch files: new, open,
//! save (with the parts changed in place), insert a part file, export STEP and the parts list.

use std::path::{Component as PathPart, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tenon_assembly::cmd::{AsmCommand, AsmFn};
use tenon_assembly::{AsmSession, Assembly, Part, Parts};
use tenon_kernel::Kernel;
use tenon_model::schema::arg;
use tenon_model::{CmdError, CmdResult};

use crate::layout;
use crate::project::{self, ProjectError};

/// `format` of every assembly file.
pub const FORMAT: &str = "tenon-assembly";
/// Current assembly schema version.
pub const VERSION: u32 = 2;
/// The assembly file extension.
pub const EXTENSION: &str = "tenonasm";

#[derive(Serialize, Deserialize)]
struct AsmFile {
    format: String,
    version: u32,
    assembly: Assembly,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

/// `path` without `.` and with `..` taken out where it can be (no file system access).
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            PathPart::CurDir => {}
            PathPart::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The key a part file is known by in memory: its full, normalised path.
pub fn part_key(path: &Path) -> String {
    let full =
        if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().map(|d| d.join(path)).unwrap_or_else(|_| path.to_path_buf()) };
    normalize(&full).to_string_lossy().into_owned()
}

/// `to` relative to the folder `from`, with `/` separators; `to` itself when they share no root.
pub fn relative(from: &Path, to: &Path) -> String {
    let (from, to) = (normalize(from), normalize(to));
    let a: Vec<_> = from.components().collect();
    let b: Vec<_> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    // Different drives or roots: keep the full path.
    let rooted = |c: &[PathPart]| c.iter().take_while(|p| matches!(p, PathPart::Prefix(_) | PathPart::RootDir)).count();
    if common < rooted(&a).max(rooted(&b)) || common == 0 {
        return to.to_string_lossy().replace('\\', "/");
    }
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_owned(), a.len() - common).collect();
    parts.extend(b[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    parts.join("/")
}

/// Assembly file bytes; part paths are written relative to `dir` (the file's folder).
pub fn to_bytes(asm: &Assembly, dir: &Path) -> Result<Vec<u8>, ProjectError> {
    let mut stored = asm.clone();
    stored.map_parts(|p| relative(dir, Path::new(p)));
    let file = AsmFile { format: FORMAT.into(), version: VERSION, assembly: stored, extra: Map::new() };
    let head = serde_json::to_value(&file).map_err(|e| ProjectError::Damaged(e.to_string()))?;
    project::to_text(&head, &layout::ASSEMBLY, VERSION)
}

/// Reads assembly file bytes of any version; part paths are resolved against `dir` (the file's
/// folder).
pub fn from_bytes(data: &[u8], dir: &Path) -> Result<Assembly, ProjectError> {
    let (head, lines) = project::read_head(data, FORMAT, &layout::ASSEMBLY, VERSION)?;
    let file: AsmFile = serde_json::from_value(head.clone()).map_err(|e| {
        project::locate::<tenon_assembly::Component>(&head, &layout::ASSEMBLY, "components", &lines)
            .or_else(|| project::locate::<tenon_assembly::Relationship>(&head, &layout::ASSEMBLY, "relationships", &lines))
            .unwrap_or_else(|| ProjectError::Damaged(e.to_string()))
    })?;
    let mut asm = file.assembly;
    asm.validate().map_err(ProjectError::Damaged)?;
    asm.tidy();
    asm.map_parts(|p| {
        let path = Path::new(p);
        if path.is_absolute() { normalize(path).to_string_lossy().into_owned() } else { part_key(&dir.join(path)) }
    });
    Ok(asm)
}

fn folder_of(path: &Path) -> PathBuf {
    let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
    if parent.as_os_str().is_empty() { PathBuf::from(".") } else { parent }
}

/// Writes an assembly atomically; returns where a version-1 original was kept, if one was.
pub fn save(path: &Path, asm: &Assembly) -> Result<Option<PathBuf>, ProjectError> {
    let bytes = to_bytes(asm, &folder_of(&std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())))?;
    project::write_file(path, &bytes)
}

/// Reads an assembly and every part file it uses. A part that cannot be read is marked missing.
pub fn open(path: &Path) -> Result<(Assembly, Parts), ProjectError> {
    let len = std::fs::metadata(path)?.len();
    if len > project::MAX_FILE {
        return Err(ProjectError::NotAProject("the file is too large".into()));
    }
    let full = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let asm = from_bytes(&std::fs::read(path)?, &folder_of(&full))?;
    let mut parts = Parts::new();
    for c in &asm.components {
        if parts.contains_key(&c.part) {
            continue;
        }
        let part = match project::open(Path::new(&c.part)) {
            Ok((doc, _)) => {
                let mut p = Part::new(tenon_model::Document::default());
                p.session.replace_document(doc, None);
                p
            }
            Err(e) => Part::missing(e.to_string()),
        };
        parts.insert(c.part.clone(), part);
    }
    Ok((asm, parts))
}

// ---- commands ---------------------------------------------------------------------------------

fn path(p: &Value) -> Result<PathBuf, CmdError> {
    let s = arg(p, "path").and_then(Value::as_str).ok_or("missing parameter `path`")?;
    if s.is_empty() || s.len() > 4096 || s.contains('\0') {
        return Err("invalid path".into());
    }
    Ok(PathBuf::from(s))
}

fn asm_new(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let name = arg(p, "name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Assembly1");
    if name.len() > 256 {
        return Err("`name` is too long".into());
    }
    s.replace(Assembly { name: name.to_owned(), ..Assembly::default() }, Parts::new());
    Ok(json!({ "name": name }))
}

fn asm_open(s: &mut AsmSession, k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let path = path(p)?;
    let (asm, parts) = open(&path).map_err(|e| CmdError(format!("cannot open {}: {e}", path.display())))?;
    s.replace(asm, parts);
    let missing: Vec<String> =
        s.parts.iter().filter_map(|(k, p)| p.missing.as_ref().map(|m| format!("{}: {m}", tenon_assembly::key_file(k)))).collect();
    if let Some(k) = k {
        s.refresh(k).map_err(CmdError)?;
    }
    Ok(json!({ "path": path.display().to_string(), "name": s.assembly().name, "components": s.assembly().components.len(), "missing": missing }))
}

fn asm_save(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let path = path(p)?;
    // Parts changed in place are saved to their own files first.
    let (mut saved, mut kept) = (Vec::new(), Vec::new());
    for (key, part) in s.parts.iter_mut() {
        // (A part in a row of its design table is worked out from its file's entry: not a file.)
        if part.variant.is_none() && part.missing.is_none() && part.session.is_dirty() {
            let k = project::save(Path::new(key), part.session.document(), &Map::new()).map_err(|e| CmdError(format!("cannot save {key}: {e}")))?;
            kept.extend(k.map(|k| k.display().to_string()));
            part.session.mark_saved();
            saved.push(key.clone());
        }
    }
    kept.extend(save(&path, s.assembly()).map_err(|e| CmdError(e.to_string()))?.map(|k| k.display().to_string()));
    s.mark_saved();
    Ok(json!({ "path": path.display().to_string(), "parts_saved": saved, "kept_version_1": kept }))
}

/// Places a part file: its first component is grounded at the origin; later ones go beside the
/// others (or `at`).
fn asm_insert(s: &mut AsmSession, mut k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let path = path(p)?;
    let key = part_key(&path);
    if !s.parts.contains_key(&key) {
        let (doc, _) = project::open(&path).map_err(|e| CmdError(format!("cannot open {}: {e}", path.display())))?;
        let mut part = Part::new(tenon_model::Document::default());
        part.session.replace_document(doc, None);
        s.add_part(&key, part);
    }
    if let Some(k) = k.as_deref_mut() {
        s.refresh(k).map_err(CmdError)?;
    }
    let at = match arg(p, "at").filter(|v| !v.is_null()) {
        Some(v) => {
            let a = v.as_array().filter(|a| a.len() == 3).ok_or("`at` must be [x, y, z]")?;
            let c = |i: usize| {
                a.get(i).and_then(Value::as_f64).filter(|x| tenon_geom::tol::is_valid_coord(*x)).ok_or("`at` must be three numbers in range")
            };
            Some(tenon_geom::Frame::WORLD.with_origin(tenon_geom::Vec3::new(c(0)?, c(1)?, c(2)?)).ok_or("`at` is out of range")?)
        }
        None => None,
    };
    let grounded = arg(p, "grounded").and_then(Value::as_bool);
    // In one size of the part: a row of its design table.
    let row = match arg(p, "row") {
        None | Some(Value::Null) => None,
        Some(Value::String(r)) => Some(r.clone()),
        Some(_) => return Err("`row` must be the name of a row of the part's design table".into()),
    };
    if let Some(row) = &row {
        tenon_assembly::cmd::check_row(&s.parts, &key, row)?;
    }
    let id = s.edit(|asm, parts| {
        let id = tenon_assembly::session::insert(asm, parts, &key, at, grounded);
        if let Some(c) = asm.component_mut(id) {
            c.row = row.clone();
        }
        Ok(id)
    })?;
    // (The part in that size is another solid to make.)
    if row.is_some()
        && let Some(k) = k
    {
        s.refresh(k).map_err(CmdError)?;
    }
    let c = s.component(id)?;
    Ok(json!({ "component": id.0, "name": c.name, "grounded": c.grounded, "row": c.row }))
}

/// The file a standard part is kept in, in `folder`: "hex-screw-iso-4017.tenon".
pub fn standard_file(folder: &Path, part: &str) -> PathBuf {
    folder.join(format!("{part}.tenon"))
}

/// Places a standard part in one of its sizes. The part is one file for all its sizes, kept in
/// a folder (`path`) beside the assembly and written there the first time it is used; the
/// component names the size.
fn asm_insert_standard(s: &mut AsmSession, k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let folder = path(p)?;
    let part = arg(p, "part").and_then(Value::as_str).ok_or("missing parameter `part`: a standard part's id (see library.list)")?;
    let family = tenon_model::library::family(part).ok_or_else(|| {
        CmdError(format!(
            "there is no standard part `{part}`: there are {}",
            tenon_model::library::families().iter().map(|f| f.id).collect::<Vec<_>>().join(", ")
        ))
    })?;
    let size = match arg(p, "size").and_then(Value::as_str) {
        Some(size) if family.sizes().iter().any(|x| x == size) => size.to_owned(),
        Some(size) => return Err(CmdError(format!("{} does not come in `{size}`: it comes in {}", family.name, family.sizes().join(", ")))),
        None => return Err(CmdError(format!("missing parameter `size`: {} comes in {}", family.name, family.sizes().join(", ")))),
    };
    let file = standard_file(&folder, part);
    let written = !file.exists();
    if written {
        let doc = tenon_model::library::build(part).map_err(CmdError)?;
        std::fs::create_dir_all(&folder).map_err(|e| CmdError(format!("cannot make the folder {}: {e}", folder.display())))?;
        project::save(&file, &doc, &Map::new()).map_err(|e| CmdError(format!("cannot write {}: {e}", file.display())))?;
    }
    let mut place = json!({ "path": file.to_string_lossy(), "row": size });
    for key in ["at", "grounded"] {
        if let Some(v) = arg(p, key) {
            place[key] = v.clone();
        }
    }
    let mut out = asm_insert(s, k, &place)?;
    out["file"] = json!(file.display().to_string());
    out["written"] = json!(written);
    Ok(out)
}

fn asm_export_step(s: &mut AsmSession, k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let k = k.ok_or("asm.export_step needs the geometry kernel")?;
    let path = path(p)?;
    let exploded = arg(p, "exploded").and_then(Value::as_bool).unwrap_or(false);
    let asm = s.assembly().clone();
    let frames = exploded.then(|| tenon_assembly::session::exploded(&asm));
    let data = tenon_assembly::interfere::export_step(k, &asm, &mut s.parts, frames.as_ref()).map_err(CmdError)?;
    std::fs::write(&path, &data).map_err(|e| CmdError(format!("cannot write {}: {e}", path.display())))?;
    Ok(json!({ "path": path.display().to_string(), "bytes": data.len() }))
}

/// What a component's object in the assembly's 3MF model is: its part (and size), name,
/// material and colour. Components of the same part in the same size share one object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectInfo {
    pub key: String,
    pub name: String,
    pub material: String,
    pub color: [u8; 3],
}

/// The object of every visible component.
pub fn object_infos(asm: &Assembly, parts: &Parts) -> std::collections::BTreeMap<tenon_assembly::ComponentId, ObjectInfo> {
    asm.components
        .iter()
        .filter(|c| c.visible)
        .filter_map(|c| {
            let key = c.key();
            let doc = parts.get(&key)?.session.document();
            let name = match &c.row {
                Some(row) => format!("{}, {row}", doc.name),
                None => doc.name.clone(),
            };
            let material = doc.material().map_or_else(|| "Generic".to_owned(), |m| m.name.clone());
            Some((c.id, ObjectInfo { key, name, material, color: doc.color().unwrap_or(crate::cmd::PLAIN) }))
        })
        .collect()
}

/// Placed solids as a 3MF file: an object for each part (each size of one, when it comes in
/// several) and an item for each component, where it is. The solids are meshed afresh to `tol`:
/// a written mesh is the result itself, and the one on screen is coarser inside freeform faces.
pub fn placed_3mf(
    k: &mut dyn Kernel,
    title: &str,
    placed: &[tenon_assembly::interfere::Placed],
    infos: &std::collections::BTreeMap<tenon_assembly::ComponentId, ObjectInfo>,
    tol: &tenon_kernel::MeshTol,
) -> Result<crate::cmd::Written, String> {
    use crate::meshcheck::Welded;
    use crate::threemf::{Item, Model, Object};
    let mut model = Model { title: title.to_owned(), ..Default::default() };
    let mut object_of: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for p in placed {
        let info = infos.get(&p.component).ok_or("internal: a component without its part")?;
        let object = match object_of.get(info.key.as_str()) {
            Some(i) => *i,
            None => {
                if p.shapes.is_empty() {
                    return Err(format!("{} has no solid to export", info.name));
                }
                let mut meshes = Vec::with_capacity(p.shapes.len());
                for shape in &p.shapes {
                    meshes.push(k.tessellate(*shape, tol).map_err(|e| format!("{}: {e}", info.name))?);
                }
                model.objects.push(Object {
                    name: info.name.clone(),
                    mesh: Welded::of_all(&meshes.iter().collect::<Vec<_>>()),
                    material: info.material.clone(),
                    color: info.color,
                });
                object_of.insert(&info.key, model.objects.len() - 1);
                model.objects.len() - 1
            }
        };
        model.items.push(Item { object, frame: Some(p.frame) });
    }
    if model.items.is_empty() {
        return Err("there is no solid to export".into());
    }
    crate::cmd::written_3mf(&model)
}

fn asm_export_3mf(s: &mut AsmSession, k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let k = k.ok_or("asm.export_3mf needs the geometry kernel")?;
    let path = path(p)?;
    s.sync_variants();
    let exploded = arg(p, "exploded").and_then(Value::as_bool).unwrap_or(false);
    let asm = s.assembly().clone();
    let frames = exploded.then(|| tenon_assembly::session::exploded(&asm));
    let placed = tenon_assembly::interfere::placed_parts(k, &asm, &mut s.parts, frames.as_ref()).map_err(CmdError)?;
    let w = placed_3mf(k, &asm.name, &placed, &object_infos(&asm, &s.parts), &crate::cmd::mesh_tol(p)).map_err(CmdError)?;
    std::fs::write(&path, &w.data).map_err(|e| CmdError(format!("cannot write {}: {e}", path.display())))?;
    let items = placed.len();
    Ok(
        json!({ "path": path.display().to_string(), "objects": w.objects, "items": items, "triangles": w.triangles, "watertight": w.watertight, "bytes": w.data.len() }),
    )
}

/// Placed solids as one STL file, each where it is: the assembly as it would be printed in one
/// piece, or laid out on one bed.
pub fn placed_stl(
    k: &mut dyn Kernel,
    name: &str,
    placed: &[tenon_assembly::interfere::Placed],
    tol: &tenon_kernel::MeshTol,
    ascii: bool,
) -> Result<crate::cmd::Written, String> {
    let mut all = tenon_kernel::Mesh::default();
    let (mut solids, mut watertight) = (0, true);
    for p in placed {
        for shape in &p.shapes {
            let m = k.tessellate(*shape, tol).map_err(|e| e.to_string())?;
            watertight &= crate::meshcheck::Welded::of(&m).closure().watertight();
            crate::cmd::append(&mut all, &m, |q| {
                let w = p.frame.to_world(tenon_geom::Vec3::new(f64::from(q[0]), f64::from(q[1]), f64::from(q[2])));
                [w.x as f32, w.y as f32, w.z as f32]
            })
            .map_err(|e| e.0)?;
            solids += 1;
        }
    }
    if solids == 0 {
        return Err("there is no solid to export".into());
    }
    crate::cmd::stl_of(&all, name, solids, watertight, ascii).map_err(|e| e.0)
}

fn asm_export_stl(s: &mut AsmSession, k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let k = k.ok_or("asm.export_stl needs the geometry kernel")?;
    let path = path(p)?;
    s.sync_variants();
    let exploded = arg(p, "exploded").and_then(Value::as_bool).unwrap_or(false);
    let asm = s.assembly().clone();
    let frames = exploded.then(|| tenon_assembly::session::exploded(&asm));
    let placed = tenon_assembly::interfere::placed_parts(k, &asm, &mut s.parts, frames.as_ref()).map_err(CmdError)?;
    let ascii = arg(p, "ascii").and_then(Value::as_bool).unwrap_or(false);
    let w = placed_stl(k, &asm.name, &placed, &crate::cmd::mesh_tol(p), ascii).map_err(CmdError)?;
    std::fs::write(&path, &w.data).map_err(|e| CmdError(format!("cannot write {}: {e}", path.display())))?;
    Ok(json!({ "path": path.display().to_string(), "solids": w.objects, "triangles": w.triangles, "watertight": w.watertight }))
}

/// A CSV field, quoted when it needs to be.
fn csv(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_owned() }
}

/// The parts list as CSV text.
pub fn bom_csv(asm: &Assembly, parts: &Parts) -> String {
    // (Material and mass came with M6, part number and description with 0.1: each after the
    // columns scripts may already read by position.)
    let mut out = String::from("Item,Part,Name,Quantity,Volume (mm^3),Material,Mass (g),Part Number,Description\r\n");
    for r in tenon_assembly::session::bom(asm, parts) {
        let volume = r.volume.map(|v| format!("{v:.3}")).unwrap_or_default();
        let material = csv(r.material.as_deref().unwrap_or(""));
        let mass = r.mass.map(|m| format!("{m:.3}")).unwrap_or_default();
        let (number, what) = (csv(&r.number()), csv(&r.what()));
        out.push_str(&format!("{},{},{},{},{volume},{material},{mass},{number},{what}\r\n", r.item, csv(&r.part), csv(&r.name), r.quantity));
    }
    out
}

fn asm_export_bom(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let path = path(p)?;
    let text = bom_csv(s.assembly(), &s.parts);
    std::fs::write(&path, &text).map_err(|e| CmdError(format!("cannot write {}: {e}", path.display())))?;
    Ok(json!({ "path": path.display().to_string(), "rows": text.lines().count().saturating_sub(1) }))
}

macro_rules! cmd {
    ($id:literal, $label:literal, $help:literal, $mutates:expr, $kernel:expr, $f:expr) => {
        AsmCommand { id: $id, label: $label, help: $help, mutates: $mutates, kernel: $kernel, run: $f as AsmFn }
    };
}

static COMMANDS: &[AsmCommand] = &[
    cmd!("asm.new", "New Assembly", "name (default Assembly1); clears undo history", false, false, asm_new),
    cmd!("asm.open", "Open Assembly", "path (.tenonasm); reads every part file it uses; clears undo history", false, false, asm_open),
    cmd!("asm.save", "Save Assembly", "path (.tenonasm); parts changed in place are saved to their own files first", false, false, asm_save),
    cmd!(
        "asm.insert",
        "Place Component",
        "path (.tenon part file); at: [x, y, z] (default: grounded at the origin for the first, beside the others after); grounded; row: a row of the part's design table (the component is the part in that size)",
        true,
        false,
        asm_insert
    ),
    cmd!(
        "asm.insert_standard",
        "Place Standard Part",
        "part: a standard part's id (see library.list); size: one of its sizes; path: the folder its file is kept in, beside the assembly (the file is written there the first time the part is used; every size comes from the one file); at, grounded: as asm.insert",
        true,
        false,
        asm_insert_standard
    ),
    cmd!("asm.export_step", "Export Assembly STEP", "path (.step); exploded (default false)", false, true, asm_export_step),
    cmd!(
        "asm.export_stl",
        "Export Assembly STL",
        "path (.stl); exploded (default false); linear (mm), angular (rad) deflection; ascii (default binary). Every visible component's solids in one file, each where it is. Returns the solid and triangle counts and whether the surface is closed (watertight)",
        false,
        true,
        asm_export_stl
    ),
    cmd!(
        "asm.export_3mf",
        "Export Assembly 3MF",
        "path (.3mf); exploded (default false); linear (mm), angular (rad) deflection. An object for each part (each size of one), an item for each visible component where it is, with materials and colours. Returns the counts and whether every surface is closed (watertight)",
        false,
        true,
        asm_export_3mf
    ),
    cmd!("asm.export_bom", "Export Bill of Materials", "path (.csv)", false, false, asm_export_bom),
];

/// The file commands for assemblies.
pub fn commands() -> &'static [AsmCommand] {
    COMMANDS
}

/// Every assembly command: these and the ones in `tenon_assembly::cmd`.
pub fn all_commands() -> impl Iterator<Item = &'static AsmCommand> {
    COMMANDS.iter().chain(tenon_assembly::cmd::commands().iter())
}

/// Runs any assembly command by id.
pub fn run(s: &mut AsmSession, id: &str, params: &Value, kernel: Option<&mut dyn Kernel>) -> CmdResult {
    let spec =
        COMMANDS.iter().find(|c| c.id == id).or_else(|| tenon_assembly::cmd::find(id)).ok_or_else(|| CmdError(format!("unknown command `{id}`")))?;
    tenon_assembly::cmd::run_as(s, spec, crate::schemas::schema(id), params, kernel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        let base = if cfg!(windows) { "C:\\work\\asm" } else { "/work/asm" };
        let join = |s: &str| Path::new(base).join(s);
        assert_eq!(relative(Path::new(base), &join("plate.tenon")), "plate.tenon");
        assert_eq!(relative(Path::new(base), &join("parts/pin.tenon")), "parts/pin.tenon");
        assert_eq!(relative(Path::new(base), &join("../lib/bolt.tenon")), "../lib/bolt.tenon");
        assert_eq!(normalize(&join("a/./b/../c.tenon")), join("a/c.tenon"));
        if cfg!(windows) {
            assert_eq!(relative(Path::new("C:\\work"), Path::new("D:\\lib\\x.tenon")), "D:/lib/x.tenon");
        }
    }
}

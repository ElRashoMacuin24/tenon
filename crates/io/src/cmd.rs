//! File commands: save and open projects, export STEP and STL.

use std::path::PathBuf;

use serde_json::{Map, Value, json};
use tenon_kernel::{Kernel, Mesh, MeshTol};
use tenon_model::{CmdError, CmdResult, CommandSpec, Run, Session};

use crate::{project, stl};

fn path(p: &Value) -> Result<PathBuf, CmdError> {
    let s = p.get("path").and_then(Value::as_str).ok_or("missing parameter `path`")?;
    if s.is_empty() || s.len() > 4096 || s.contains('\0') {
        return Err("invalid path".into());
    }
    Ok(PathBuf::from(s))
}

fn file_save(s: &mut Session, p: &Value) -> CmdResult {
    let path = path(p)?;
    project::save(&path, s.document(), &Map::new()).map_err(|e| CmdError(e.to_string()))?;
    s.mark_saved();
    Ok(json!({ "path": path.display().to_string() }))
}

fn file_open(s: &mut Session, p: &Value) -> CmdResult {
    let path = path(p)?;
    let (doc, _) = project::open(&path).map_err(|e| CmdError(e.to_string()))?;
    let name = doc.name.clone();
    s.replace_document(doc, None);
    Ok(json!({ "path": path.display().to_string(), "name": name, "features": s.document().features().len() }))
}

/// Bodies of the current document, or an error naming the failing feature.
fn bodies(s: &mut Session, k: &mut dyn Kernel) -> Result<Vec<tenon_kernel::ShapeHandle>, CmdError> {
    let r = s.regen(k);
    if let Some((_, msg)) = r.first_error() {
        return Err(CmdError(format!("the part does not regenerate: {msg}")));
    }
    if r.bodies.is_empty() {
        return Err("there is no solid to export".into());
    }
    Ok(r.bodies.iter().map(|b| b.shape).collect())
}

fn export_step(s: &mut Session, k: &mut dyn Kernel, p: &Value) -> CmdResult {
    let path = path(p)?;
    let shapes = bodies(s, k)?;
    let data = k.export_step(&shapes).map_err(|e| CmdError(e.to_string()))?;
    std::fs::write(&path, &data).map_err(|e| CmdError(format!("cannot write {}: {e}", path.display())))?;
    Ok(json!({ "path": path.display().to_string(), "bytes": data.len(), "bodies": shapes.len() }))
}

/// One mesh of every body.
pub fn merged_mesh(k: &mut dyn Kernel, shapes: &[tenon_kernel::ShapeHandle], tol: &MeshTol) -> Result<Mesh, CmdError> {
    let mut all = Mesh::default();
    for s in shapes {
        let m = k.tessellate(*s, tol).map_err(|e| CmdError(e.to_string()))?;
        let base = u32::try_from(all.positions.len()).map_err(|_| "mesh too large")?;
        all.positions.extend(m.positions);
        all.normals.extend(m.normals);
        all.indices.extend(m.indices.iter().map(|i| i.saturating_add(base)));
    }
    Ok(all)
}

fn export_stl(s: &mut Session, k: &mut dyn Kernel, p: &Value) -> CmdResult {
    let path = path(p)?;
    let shapes = bodies(s, k)?;
    let mut tol = MeshTol::default();
    if let Some(l) = p.get("linear").and_then(Value::as_f64) {
        tol.linear = l;
    }
    if let Some(a) = p.get("angular").and_then(Value::as_f64) {
        tol.angular = a;
    }
    let mesh = merged_mesh(k, &shapes, &tol)?;
    let name = s.document().name.clone();
    let data = if p.get("ascii").and_then(Value::as_bool).unwrap_or(false) {
        stl::write_ascii(&mesh, &name).into_bytes()
    } else {
        stl::write_binary(&mesh, &format!("Tenon {name} (mm)"))
    };
    std::fs::write(&path, &data).map_err(|e| CmdError(format!("cannot write {}: {e}", path.display())))?;
    Ok(json!({ "path": path.display().to_string(), "triangles": mesh.triangle_count() }))
}

static COMMANDS: &[CommandSpec] = &[
    CommandSpec { id: "file.save", label: "Save", help: "path (.tenon)", mutates: false, run: Run::Doc(file_save) },
    CommandSpec { id: "file.open", label: "Open", help: "path (.tenon)", mutates: true, run: Run::Doc(file_open) },
    CommandSpec { id: "export.step", label: "Export STEP", help: "path (.step)", mutates: false, run: Run::Geo(export_step) },
    CommandSpec {
        id: "export.stl",
        label: "Export STL",
        help: "path (.stl); linear (mm), angular (rad) deflection; ascii (default binary)",
        mutates: false,
        run: Run::Geo(export_stl),
    },
];

/// File and export commands.
pub fn commands() -> &'static [CommandSpec] {
    COMMANDS
}

/// Looks a command up in the model's registry and this one.
pub fn find(id: &str) -> Option<&'static CommandSpec> {
    tenon_model::cmd::find(id).or_else(|| COMMANDS.iter().find(|c| c.id == id))
}

/// Every command: model and file.
pub fn all_commands() -> impl Iterator<Item = &'static CommandSpec> {
    tenon_model::cmd::commands().iter().chain(COMMANDS.iter())
}

/// Runs any command (model or file) by id.
pub fn run(s: &mut Session, id: &str, params: &Value, kernel: Option<&mut dyn Kernel>) -> CmdResult {
    let spec = find(id).ok_or_else(|| CmdError(format!("unknown command `{id}`")))?;
    s.run(spec, params, kernel)
}

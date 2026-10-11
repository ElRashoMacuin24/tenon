//! File commands: save, open and compare projects, export STEP and STL.

use std::path::PathBuf;

use serde_json::{Map, Value, json};
use tenon_kernel::{Kernel, Mesh, MeshTol};
use tenon_model::{CmdError, CmdResult, CommandSpec, Run, Session};

use crate::meshcheck::Welded;
use crate::{print, project, stl, threemf};

pub(crate) fn path(p: &Value) -> Result<PathBuf, CmdError> {
    path_in(p, "path")
}

fn path_in(p: &Value, key: &str) -> Result<PathBuf, CmdError> {
    let s = p.get(key).and_then(Value::as_str).ok_or_else(|| CmdError(format!("missing parameter `{key}`")))?;
    if s.is_empty() || s.len() > 4096 || s.contains('\0') {
        return Err(format!("invalid path in `{key}`").into());
    }
    Ok(PathBuf::from(s))
}

fn file_save(s: &mut Session, p: &Value) -> CmdResult {
    let path = path(p)?;
    let kept = project::save(&path, s.document(), &Map::new()).map_err(|e| CmdError(e.to_string()))?;
    s.mark_saved();
    let kept: Vec<String> = kept.map(|k| k.display().to_string()).into_iter().collect();
    Ok(json!({ "path": path.display().to_string(), "kept_version_1": kept }))
}

fn file_diff(_s: &mut Session, p: &Value) -> CmdResult {
    let d = crate::diff::diff_files(&path_in(p, "a")?, &path_in(p, "b")?).map_err(|e| CmdError(e.to_string()))?;
    let mut out = d.json();
    out["text"] = Value::String(d.text());
    Ok(out)
}

fn file_new(s: &mut Session, p: &Value) -> CmdResult {
    let mut doc = tenon_model::Document::default();
    if let Some(name) = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) {
        if name.len() > 256 {
            return Err("`name` is too long".into());
        }
        doc.name = name.to_owned();
    }
    let name = doc.name.clone();
    s.replace_document(doc, None);
    Ok(json!({ "name": name }))
}

/// The standard parts there are: each with its sizes.
fn library_list(_s: &mut Session, p: &Value) -> CmdResult {
    let category = p.get("category").and_then(Value::as_str);
    let parts: Vec<Value> = tenon_model::library::families()
        .iter()
        .filter(|f| category.is_none_or(|c| c.eq_ignore_ascii_case(f.category)))
        .map(|f| {
            json!({
                "part": f.id, "name": f.name, "category": f.category, "standard": f.standard, "about": f.about,
                "columns": f.columns, "sizes": f.sizes(),
            })
        })
        .collect();
    Ok(json!({ "parts": parts }))
}

/// The document of standard part `id`, at `size` when one is given.
pub fn standard_part(id: &str, size: Option<&str>) -> Result<tenon_model::Document, CmdError> {
    let mut doc = tenon_model::library::build(id).map_err(CmdError)?;
    if let Some(size) = size {
        let sizes = || doc.table().map(|t| t.rows.iter().map(|r| r.name.clone()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        if doc.table().is_none_or(|t| t.row(size).is_none()) {
            return Err(CmdError(format!("{} does not come in `{size}`: it comes in {}", doc.name, sizes())));
        }
        doc.table_activate(size).and_then(|()| doc.sync_parameters()).map_err(CmdError)?;
    }
    Ok(doc)
}

/// Starts a part from a standard one: a part like any other, with the standard's sizes in its
/// design table.
fn library_new(s: &mut Session, p: &Value) -> CmdResult {
    let id = p.get("part").and_then(Value::as_str).ok_or("missing parameter `part`: a standard part's id (see library.list)")?;
    let size = p.get("size").and_then(Value::as_str);
    let doc = standard_part(id, size)?;
    let (name, active) = (doc.name.clone(), doc.table().map(|t| t.active.clone()));
    s.replace_document(doc, None);
    Ok(json!({ "part": id, "name": name, "size": active }))
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

/// The mesh tolerances a command asks for (`linear`, `angular`), or the usual ones.
pub(crate) fn mesh_tol(p: &Value) -> MeshTol {
    let mut tol = MeshTol::exact();
    if let Some(l) = p.get("linear").and_then(Value::as_f64) {
        tol.linear = l;
    }
    if let Some(a) = p.get("angular").and_then(Value::as_f64) {
        tol.angular = a;
    }
    tol
}

/// What a written mesh came to.
#[derive(Clone, Debug, PartialEq)]
pub struct Written {
    pub data: Vec<u8>,
    pub objects: usize,
    pub triangles: usize,
    /// Every surface is closed: a slicer can tell inside from outside.
    pub watertight: bool,
}

/// The solids `shapes` as an STL file named `name`: binary, or text when `ascii`.
pub fn stl_data(k: &mut dyn Kernel, shapes: &[tenon_kernel::ShapeHandle], name: &str, tol: &MeshTol, ascii: bool) -> Result<Written, CmdError> {
    let mut all = Mesh::default();
    let mut watertight = true;
    for s in shapes {
        let m = k.tessellate(*s, tol).map_err(|e| CmdError(e.to_string()))?;
        watertight &= Welded::of(&m).closure().watertight();
        append(&mut all, &m, |p| p)?;
    }
    stl_of(&all, name, shapes.len(), watertight, ascii)
}

/// Adds `m` to `all`, each of its points where `place` puts it.
pub(crate) fn append(all: &mut Mesh, m: &Mesh, place: impl Fn([f32; 3]) -> [f32; 3]) -> Result<(), CmdError> {
    let base = u32::try_from(all.positions.len()).map_err(|_| "mesh too large")?;
    all.positions.extend(m.positions.iter().map(|p| place(*p)));
    all.indices.extend(m.indices.iter().map(|i| i.saturating_add(base)));
    Ok(())
}

/// A mesh of `solids` solids as an STL file named `name`. `watertight`: each solid's surface is
/// closed. (Judged solid by solid: two that touch share corners, and are no less printable.)
pub(crate) fn stl_of(mesh: &Mesh, name: &str, solids: usize, watertight: bool, ascii: bool) -> Result<Written, CmdError> {
    let triangles = stl::triangle_count(mesh);
    if triangles > u32::MAX as usize {
        return Err("the mesh has more triangles than an STL file can count: export with a larger `linear`".into());
    }
    let data = if ascii { stl::write_ascii(mesh, name).into_bytes() } else { stl::write_binary(mesh, &format!("Tenon {name} (mm)")) };
    Ok(Written { data, objects: solids, triangles, watertight })
}

fn export_stl(s: &mut Session, k: &mut dyn Kernel, p: &Value) -> CmdResult {
    let path = path(p)?;
    let shapes = bodies(s, k)?;
    let ascii = p.get("ascii").and_then(Value::as_bool).unwrap_or(false);
    let w = stl_data(k, &shapes, &s.document().name, &mesh_tol(p), ascii)?;
    std::fs::write(&path, &w.data).map_err(|e| CmdError(format!("cannot write {}: {e}", path.display())))?;
    Ok(json!({ "path": path.display().to_string(), "triangles": w.triangles, "watertight": w.watertight }))
}

/// The standard colour of a part with neither material nor appearance, as 3MF's display colour.
pub(crate) const PLAIN: [u8; 3] = [0xb8, 0xc0, 0xc8];

/// The solids `shapes` of the part `doc` as a 3MF file: an object for each, in millimetres, in
/// the part's material and colour.
pub fn part_3mf(k: &mut dyn Kernel, shapes: &[tenon_kernel::ShapeHandle], doc: &tenon_model::Document, tol: &MeshTol) -> Result<Written, CmdError> {
    let (color, material) = (doc.color().unwrap_or(PLAIN), doc.material().map_or_else(|| "Generic".to_owned(), |m| m.name.clone()));
    let many = shapes.len() > 1;
    let mut model = threemf::Model { title: doc.name.clone(), ..Default::default() };
    for (i, shape) in shapes.iter().enumerate() {
        let mesh = k.tessellate(*shape, tol).map_err(|e| CmdError(e.to_string()))?;
        let name = if many { format!("{} {}", doc.name, i + 1) } else { doc.name.clone() };
        model.objects.push(threemf::Object { name, mesh: Welded::of(&mesh), material: material.clone(), color });
        model.items.push(threemf::Item { object: i, frame: None });
    }
    written_3mf(&model).map_err(CmdError)
}

/// A 3MF model as its file, with what it holds.
pub(crate) fn written_3mf(model: &threemf::Model) -> Result<Written, String> {
    Ok(Written {
        data: threemf::write(model)?,
        objects: model.objects.len(),
        triangles: model.objects.iter().map(|o| o.mesh.triangles.len()).sum(),
        watertight: model.objects.iter().all(|o| o.mesh.closure().watertight()),
    })
}

fn export_3mf(s: &mut Session, k: &mut dyn Kernel, p: &Value) -> CmdResult {
    let path = path(p)?;
    let shapes = bodies(s, k)?;
    let w = part_3mf(k, &shapes, s.document(), &mesh_tol(p))?;
    std::fs::write(&path, &w.data).map_err(|e| CmdError(format!("cannot write {}: {e}", path.display())))?;
    Ok(
        json!({ "path": path.display().to_string(), "objects": w.objects, "triangles": w.triangles, "watertight": w.watertight, "bytes": w.data.len() }),
    )
}

/// What `print.check` is asked to check against.
fn print_setup(p: &Value) -> Result<print::Setup, CmdError> {
    let mut setup = print::Setup::default();
    // (A value that is not a number is out of range like any other: the check says how.)
    let number = |key: &str, into: &mut f64| {
        if let Some(v) = p.get(key).filter(|v| !v.is_null()) {
            *into = v.as_f64().unwrap_or(f64::NAN);
        }
    };
    number("min_wall", &mut setup.min_wall);
    number("overhang", &mut setup.overhang);
    if let Some(up) = p.get("up").and_then(Value::as_str) {
        setup.up = print::Up::parse(up).ok_or_else(|| CmdError(format!("`up` is `{up}`: it must be x, y, z, -x, -y or -z")))?;
    }
    setup.bed = match p.get("bed") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let b: Vec<f64> = v.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()).unwrap_or_default();
            Some(<[f64; 3]>::try_from(b).map_err(|_| CmdError("`bed` must be three sizes in mm: [x, y, height]".into()))?)
        }
    };
    setup.check().map_err(CmdError)?;
    Ok(setup)
}

/// How the part would print: is its surface closed, what overhangs, how thin is its thinnest
/// wall, does it fit the bed. An aid for the usual desktop printer, not a slicer's verdict.
fn print_check(s: &mut Session, k: &mut dyn Kernel, p: &Value) -> CmdResult {
    let setup = print_setup(p)?;
    let shapes = bodies(s, k)?;
    Ok(print::check(k, &shapes, &setup).map_err(CmdError)?.json())
}

static COMMANDS: &[CommandSpec] = &[
    CommandSpec { id: "file.new", label: "New Part", help: "name (default Part1); clears undo history", mutates: false, run: Run::Doc(file_new) },
    CommandSpec { id: "file.save", label: "Save", help: "path (.tenon)", mutates: false, run: Run::Doc(file_save) },
    CommandSpec { id: "file.open", label: "Open", help: "path (.tenon); clears undo history", mutates: false, run: Run::Doc(file_open) },
    CommandSpec {
        id: "library.list",
        label: "Standard Parts",
        help: "the standard parts there are (fasteners, bearings, sections), each with its id, what it is and its sizes; category: only that one",
        mutates: false,
        run: Run::Doc(library_list),
    },
    CommandSpec {
        id: "library.new",
        label: "New Standard Part",
        help: "part: a standard part's id (see library.list); size: one of its sizes (default: the one it is drawn at, such as M8x30 for a screw). Starts a part from it, with every size in its design table; clears undo history",
        mutates: false,
        run: Run::Doc(library_new),
    },
    CommandSpec {
        id: "file.diff",
        label: "Compare Files",
        help: "a, b: two parts, assemblies or drawings (any format version). Returns what changed from a to b: each change's section, op (added, removed, changed, moved), item and detail; same; and the text tenon-cli diff prints",
        mutates: false,
        run: Run::Doc(file_diff),
    },
    CommandSpec { id: "export.step", label: "Export STEP", help: "path (.step)", mutates: false, run: Run::Geo(export_step) },
    CommandSpec {
        id: "export.stl",
        label: "Export STL",
        help: "path (.stl); linear (mm, default 0.01), angular (rad, default 0.2) deflection, kept everywhere on the surface; ascii (default binary). Returns the triangle count and whether the surface is closed (watertight)",
        mutates: false,
        run: Run::Geo(export_stl),
    },
    CommandSpec {
        id: "export.3mf",
        label: "Export 3MF",
        help: "path (.3mf); linear (mm, default 0.01), angular (rad, default 0.2) deflection, kept everywhere on the surface. One object per solid, in millimetres, with the part's material name and colour. Returns the object and triangle counts and whether every surface is closed (watertight)",
        mutates: false,
        run: Run::Geo(export_3mf),
    },
    CommandSpec {
        id: "print.check",
        label: "3D Print Check",
        help: "how the part would print. min_wall (mm, default 0.8); overhang (degrees from vertical a face may lean out, default 45); up: x | y | z | -x | -y | -z (which way is up on the printer, default z); bed: [x, y, height] in mm (default: not checked). Returns ok and, each with its own ok: watertight (open, crowded and flipped edges), overhangs (area and the faces, each with its steepest angle), walls (the thinnest found and the faces under min_wall), size (the part's extents, its height standing that way, and whether it fits the bed)",
        mutates: false,
        run: Run::Geo(print_check),
    },
    CommandSpec {
        id: "sheet.export_dxf",
        label: "Export Flat Pattern DXF",
        help: "path (.dxf): the flat pattern for a cutter, R12 in millimetres with its lower left corner at the origin. Layers: OUTER (the outline), INTERIOR (holes and cut-outs), BEND_UP and BEND_DOWN (bend centre lines), BEND_EXTENT (where bends begin and end). bends (default true), extents (default true): leave those lines out with false. face, body: as for sheet.flat. Returns the sheet's length, width, area and cut_length, and what was written of the outlines (entities)",
        mutates: false,
        run: Run::Geo(crate::flat::export_dxf),
    },
    CommandSpec {
        id: "sheet.export_svg",
        label: "Export Flat Pattern SVG",
        help: "path (.svg): the flat pattern as a picture at full size; bends, extents, face, body as for sheet.export_dxf",
        mutates: false,
        run: Run::Geo(crate::flat::export_svg),
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

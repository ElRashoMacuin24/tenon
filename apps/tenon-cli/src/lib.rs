//! Headless Tenon: the logic behind `tenon-cli`. Every command returns a [`Report`] with a human
//! summary and the same data as JSON, so scripts and agents can verify results.
#![forbid(unsafe_code)]

pub mod engine;
pub mod mcp;
pub mod script;

use std::f64::consts::PI;
use std::path::Path;

use serde_json::{Value, json};
use tenon_geom::{Axis, Frame, Vec3};
use tenon_kernel::{BoolOp, KResult, Kernel, Mesh, MeshTol, ShapeHandle};

pub use engine::Engine;
pub use script::Script;

/// The M1 demo script (also in examples/m1-bracket).
pub const M1_BRACKET_SCRIPT: &str = include_str!("../../../examples/m1-bracket/bracket.json");

/// The M2 demo script, the parametric enclosure (also in examples/m2-enclosure).
pub const M2_ENCLOSURE_SCRIPT: &str = include_str!("../../../examples/m2-enclosure/enclosure.json");

/// The second M2 example, the parametric L-mount (also in examples/m2-mount).
pub const M2_MOUNT_SCRIPT: &str = include_str!("../../../examples/m2-mount/mount.json");

/// The M3 demo script, the pivot assembly (also in examples/m3-pivot).
pub const M3_PIVOT_SCRIPT: &str = include_str!("../../../examples/m3-pivot/pivot.json");

/// The M4 demo script, drawings of a plate and its assembly (also in examples/m4-plate).
pub const M4_PLATE_SCRIPT: &str = include_str!("../../../examples/m4-plate/plate.json");

/// The M6 demo script, four fittings and their assembly (also in examples/m6-fittings).
pub const M6_FITTINGS_SCRIPT: &str = include_str!("../../../examples/m6-fittings/fittings.json");

/// Output of a command.
#[derive(Debug, Clone)]
pub struct Report {
    pub text: String,
    pub json: Value,
}

fn v(p: Vec3) -> Value {
    json!([p.x, p.y, p.z])
}

/// Kernel and tool versions.
pub fn version(k: &dyn Kernel) -> Report {
    let json = json!({ "tenon": env!("CARGO_PKG_VERSION"), "kernel": k.name(), "kernel_version": k.version() });
    Report { text: format!("tenon-cli {}\nkernel: {} ({})", env!("CARGO_PKG_VERSION"), k.name(), k.version()), json }
}

/// Measurements of one shape, as JSON.
pub fn describe(k: &dyn Kernel, s: ShapeHandle) -> KResult<Value> {
    let m = k.mass_properties(s, 1.0)?;
    let t = k.topology(s)?;
    let bbox = k.bounding_box(s)?.map(|b| json!({ "min": v(b.min), "max": v(b.max) }));
    Ok(json!({
        "kind": format!("{:?}", t.kind),
        "valid": k.is_valid(s)?,
        "volume_mm3": m.volume,
        "area_mm2": m.area,
        "center_of_mass": v(m.center_of_mass),
        "bounding_box": bbox,
        "solids": t.solids,
        "faces": t.faces,
        "edges": t.edges,
        "vertices": t.vertices,
    }))
}

fn describe_text(i: usize, d: &Value) -> String {
    let f = |key: &str| d[key].as_f64().unwrap_or(f64::NAN);
    let corner =
        |c: &Value| format!("[{:.3}, {:.3}, {:.3}]", c[0].as_f64().unwrap_or(0.0), c[1].as_f64().unwrap_or(0.0), c[2].as_f64().unwrap_or(0.0));
    let bbox = match &d["bounding_box"] {
        Value::Null => "empty".to_owned(),
        b => format!("{} .. {}", corner(&b["min"]), corner(&b["max"])),
    };
    format!(
        "shape {i}: {}, {}\n  volume    {:.3} mm^3\n  area      {:.3} mm^2\n  bbox      {bbox}\n  topology  {} solid(s), {} faces, {} edges, {} vertices",
        d["kind"].as_str().unwrap_or("?"),
        if d["valid"].as_bool() == Some(true) { "valid" } else { "INVALID" },
        f("volume_mm3"),
        f("area_mm2"),
        d["solids"],
        d["faces"],
        d["edges"],
        d["vertices"],
    )
}

/// The M0 demo part: an L-bracket with three holes, built only from M0 operations (boxes,
/// cylinders, booleans). Base 60 x 40 x 8 mm, upright 60 x 8 x 30 mm, two 8 mm holes in the base,
/// one 10 mm hole in the upright.
pub struct Bracket {
    pub shape: ShapeHandle,
    /// Analytic volume: 60*40*8 + 60*8*30 - 2*pi*4^2*8 - pi*5^2*8 mm^3.
    pub expected_volume: f64,
}

pub fn m0_bracket(k: &mut dyn Kernel) -> KResult<Bracket> {
    let at = |o: Vec3| Frame::WORLD.with_origin(o).unwrap_or(Frame::WORLD);
    let base = k.make_box(&Frame::WORLD, Vec3::new(60.0, 40.0, 8.0))?.shape;
    let upright = k.make_box(&at(Vec3::new(0.0, 0.0, 8.0)), Vec3::new(60.0, 8.0, 30.0))?.shape;
    let body = k.boolean(BoolOp::Union, base, &[upright])?.shape;
    let z = |x, y| Axis::new(Vec3::new(x, y, -1.0), Vec3::Z);
    let y_axis = Axis::new(Vec3::new(30.0, -1.0, 25.0), Vec3::Y);
    let (Some(h1), Some(h2), Some(h3)) = (z(15.0, 26.0), z(45.0, 26.0), y_axis) else {
        return Err(tenon_kernel::KernelError::InvalidInput("demo axes".into()));
    };
    let hole1 = k.make_cylinder(&h1, 4.0, 10.0)?.shape;
    let hole2 = k.make_cylinder(&h2, 4.0, 10.0)?.shape;
    let hole3 = k.make_cylinder(&h3, 5.0, 10.0)?.shape;
    let shape = k.boolean(BoolOp::Cut, body, &[hole1, hole2, hole3])?.shape;
    for s in [base, upright, body, hole1, hole2, hole3] {
        k.release(s);
    }
    Ok(Bracket { shape, expected_volume: 60.0 * 40.0 * 8.0 + 60.0 * 8.0 * 30.0 - 2.0 * PI * 16.0 * 8.0 - PI * 25.0 * 8.0 })
}

/// One mesh from several shapes.
fn mesh_all(k: &mut dyn Kernel, shapes: &[ShapeHandle]) -> KResult<Mesh> {
    let mut all = Mesh::default();
    for s in shapes {
        let m = k.tessellate(*s, &MeshTol::default())?;
        let base = u32::try_from(all.positions.len()).map_err(|_| tenon_kernel::KernelError::InvalidInput("mesh too large".into()))?;
        all.positions.extend(m.positions);
        all.normals.extend(m.normals);
        all.indices.extend(m.indices.iter().map(|i| i.saturating_add(base)));
    }
    Ok(all)
}

fn write(path: &Path, data: &[u8]) -> Result<(), String> {
    std::fs::write(path, data).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Builds the M0 demo bracket and writes `bracket.step` and `bracket.stl` into `out_dir`.
pub fn demo_m0(k: &mut dyn Kernel, out_dir: &Path) -> Result<Report, String> {
    let err = |e: tenon_kernel::KernelError| e.to_string();
    std::fs::create_dir_all(out_dir).map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
    let b = m0_bracket(k).map_err(err)?;
    let step = k.export_step(&[b.shape]).map_err(err)?;
    let mesh = mesh_all(k, &[b.shape]).map_err(err)?;
    let stl = tenon_io::stl::write_binary(&mesh, "Tenon M0 demo bracket (mm)");
    let (step_path, stl_path) = (out_dir.join("bracket.step"), out_dir.join("bracket.stl"));
    write(&step_path, &step)?;
    write(&stl_path, &stl)?;
    let d = describe(k, b.shape).map_err(err)?;
    let volume = d["volume_mm3"].as_f64().unwrap_or(f64::NAN);
    let rel_error = (volume - b.expected_volume).abs() / b.expected_volume;
    let text = format!(
        "M0 demo: L-bracket with three holes\n{}\n  expected  {:.3} mm^3 (relative error {rel_error:.1e})\nwrote {} ({} bytes)\nwrote {} ({} triangles)",
        describe_text(0, &d),
        b.expected_volume,
        step_path.display(),
        step.len(),
        stl_path.display(),
        mesh.triangle_count()
    );
    let json = json!({
        "shape": d,
        "expected_volume_mm3": b.expected_volume,
        "relative_volume_error": rel_error,
        "step": step_path.display().to_string(),
        "stl": stl_path.display().to_string(),
        "triangles": mesh.triangle_count(),
    });
    Ok(Report { text, json })
}

fn import(k: &mut dyn Kernel, path: &Path) -> Result<Vec<ShapeHandle>, String> {
    let data = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    k.import_step(&data).map_err(|e| format!("{}: {e}", path.display()))
}

/// Imports a STEP file and reports each shape.
pub fn info(k: &mut dyn Kernel, path: &Path) -> Result<Report, String> {
    let shapes = import(k, path)?;
    let mut text = format!("{}: {} shape(s)", path.display(), shapes.len());
    let mut list = Vec::new();
    for (i, s) in shapes.iter().enumerate() {
        let d = describe(k, *s).map_err(|e| e.to_string())?;
        text.push('\n');
        text.push_str(&describe_text(i, &d));
        list.push(d);
    }
    Ok(Report { text, json: json!({ "file": path.display().to_string(), "shapes": list }) })
}

/// Standard base64 (RFC 4648, with padding).
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = |i: usize| u32::from(c.get(i).copied().unwrap_or(0));
        let n = (b(0) << 16) | (b(1) << 8) | b(2);
        for i in 0..4 {
            out.push(if i <= c.len() { char::from(T[((n >> (18 - 6 * i)) & 63) as usize]) } else { '=' });
        }
    }
    out
}

/// One line per step for the text report.
fn step_line(r: &script::StepResult) -> String {
    let mut s = r.result.to_string();
    if s.len() > 100 {
        let cut = (0..=97).rev().find(|i| s.is_char_boundary(*i)).unwrap_or(0);
        s.truncate(cut);
        s.push_str("...");
    }
    format!("{:>4}  {:<24} {s}", r.index, r.run)
}

/// Runs a command script on a new document. Relative paths in it resolve against `base`.
pub fn run_script(kernel: Box<dyn Kernel>, text: &str, base: &Path) -> Result<Report, String> {
    let s = Script::parse(text)?;
    std::fs::create_dir_all(base).map_err(|e| format!("cannot create {}: {e}", base.display()))?;
    let mut engine = Engine::new(kernel, base);
    let title = s.name.clone().unwrap_or_else(|| "script".into());
    match script::run(&mut engine, &s) {
        Ok(done) => {
            let mut text = format!("{title}: {} step(s) ok", done.len());
            for r in &done {
                text.push('\n');
                text.push_str(&step_line(r));
            }
            let steps: Vec<Value> = done.iter().map(|r| json!({ "step": r.index, "run": r.run, "result": r.result })).collect();
            Ok(Report { text, json: json!({ "name": s.name, "ok": true, "steps": steps }) })
        }
        Err((done, e)) => {
            let mut text = String::new();
            for r in &done {
                text.push_str(&step_line(r));
                text.push('\n');
            }
            text.push_str(&e.to_string());
            Err(text)
        }
    }
}

/// Builds the M1 demo bracket from its command script, writing the project, STEP, STL and PNG
/// files named in the script into `out_dir`.
pub fn demo_m1(kernel: Box<dyn Kernel>, out_dir: &Path) -> Result<Report, String> {
    run_script(kernel, M1_BRACKET_SCRIPT, out_dir)
}

/// Builds the M2 demo part (a parametric enclosure) and writes its files into `out_dir`.
pub fn demo_m2(kernel: Box<dyn Kernel>, out_dir: &Path) -> Result<Report, String> {
    run_script(kernel, M2_ENCLOSURE_SCRIPT, out_dir)
}

/// Builds the second M2 example (a parametric L-mount) into `out_dir`.
pub fn demo_m2_mount(kernel: Box<dyn Kernel>, out_dir: &Path) -> Result<Report, String> {
    run_script(kernel, M2_MOUNT_SCRIPT, out_dir)
}

/// Builds the M3 demo (four part files and the pivot assembly) into `out_dir`.
pub fn demo_m3(kernel: Box<dyn Kernel>, out_dir: &Path) -> Result<Report, String> {
    run_script(kernel, M3_PIVOT_SCRIPT, out_dir)
}

/// Builds the M4 demo (a plate, a pin, their assembly and a two-sheet drawing) into `out_dir`.
pub fn demo_m4(kernel: Box<dyn Kernel>, out_dir: &Path) -> Result<Report, String> {
    run_script(kernel, M4_PLATE_SCRIPT, out_dir)
}

/// Builds the M6 demo (a spring, a bolt, a handle, a nozzle and their assembly) into `out_dir`.
pub fn demo_m6(kernel: Box<dyn Kernel>, out_dir: &Path) -> Result<Report, String> {
    run_script(kernel, M6_FITTINGS_SCRIPT, out_dir)
}

/// Opens a project and renders it to PNG.
pub fn render(kernel: Box<dyn Kernel>, project: &Path, out: &Path, view: &str, size: [u32; 2]) -> Result<Report, String> {
    let mut engine = Engine::new(kernel, ".");
    engine.exec("file.open", &json!({ "path": project.to_string_lossy() }))?;
    let r = engine.exec("render.png", &json!({ "path": out.to_string_lossy(), "view": view, "width": size[0], "height": size[1] }))?;
    Ok(Report { text: format!("wrote {} ({} x {}, {view} view)", out.display(), size[0], size[1]), json: r })
}

/// Example of every constraint as `sketch.constrain` takes it.
fn constraint_examples() -> Vec<tenon_sketch::Constraint> {
    use tenon_sketch::{Constraint::*, EntityId as E};
    let (a, b, c) = (E(1), E(2), E(3));
    let all = vec![
        Coincident { a, b },
        PointOnCurve { point: a, curve: b },
        Horizontal { line: a },
        Vertical { line: a },
        Parallel { a, b },
        Perpendicular { a, b },
        Collinear { a, b },
        Tangent { a, b },
        Concentric { a, b },
        Equal { a, b },
        Symmetric { a, b, axis: c },
        Midpoint { point: a, line: b },
        Fix { point: a },
        Distance { a, b, value: 10.0 },
        HorizontalDistance { a, b, value: 10.0 },
        VerticalDistance { a, b, value: 10.0 },
        Length { line: a, value: 10.0 },
        Angle { a, b, value: 0.5 },
        Radius { curve: a, value: 5.0 },
        Diameter { curve: a, value: 10.0 },
    ];
    // Fails to compile when a constraint kind is added, so this list stays complete.
    for x in &all {
        match x {
            Coincident { .. }
            | PointOnCurve { .. }
            | Horizontal { .. }
            | Vertical { .. }
            | Parallel { .. }
            | Perpendicular { .. }
            | Collinear { .. }
            | Tangent { .. }
            | Concentric { .. }
            | Equal { .. }
            | Symmetric { .. }
            | Midpoint { .. }
            | Fix { .. }
            | Distance { .. }
            | HorizontalDistance { .. }
            | VerticalDistance { .. }
            | Length { .. }
            | Angle { .. }
            | Radius { .. }
            | Diameter { .. } => {}
        }
    }
    all
}

/// docs/commands.md: every command and constraint, generated from the registry.
pub fn commands_markdown() -> String {
    let esc = |s: &str| s.replace('|', "\\|");
    let mut md = String::from(
        "# Commands\n\n\
         <!-- Generated by `tenon-cli commands --markdown --out docs/commands.md`; a test fails when it is out of date. -->\n\n\
         Everything Tenon does to a part is a command with JSON parameters: the ribbon, scripts \
         (`tenon-cli run`, see [scripts.md](scripts.md)) and agents ([mcp.md](mcp.md)) all go \
         through the same registry. Units are millimetres and radians. *Undoable* commands \
         edit the part as one step that `edit.undo` reverts.\n\n\
         | Command | Name | Parameters | Undoable |\n|---|---|---|---|\n",
    );
    for (id, label, help, mutates) in engine::all_commands() {
        let help = if help.is_empty() { "none".to_owned() } else { esc(help) };
        md.push_str(&format!("| `{id}` | {label} | {help} | {} |\n", if mutates { "yes" } else { "no" }));
    }
    md.push_str(
        "\n## Constraints\n\n\
         `sketch.constrain` takes one of these as `constraint` (the numbers are entity ids from the \
         sketch commands). Dimensions (`distance` to `diameter`) drive the geometry; change them \
         with `sketch.set_dimension`.\n\n",
    );
    for c in constraint_examples() {
        md.push_str(&format!("- `{}`\n", serde_json::to_string(&c).unwrap_or_default()));
    }
    md
}

/// The command list as a report.
pub fn commands() -> Report {
    let all = engine::all_commands();
    let width = all.iter().map(|c| c.0.len()).max().unwrap_or(0);
    let text = all.iter().map(|(id, label, _, _)| format!("{id:<width$}  {label}")).collect::<Vec<_>>().join("\n");
    let json = json!(
        all.iter().map(|(id, label, help, mutates)| json!({ "id": id, "label": label, "params": help, "undoable": mutates })).collect::<Vec<_>>()
    );
    Report { text, json }
}

/// Converts a STEP file to binary STL.
pub fn convert(k: &mut dyn Kernel, input: &Path, output: &Path) -> Result<Report, String> {
    let shapes = import(k, input)?;
    let mesh = mesh_all(k, &shapes).map_err(|e| e.to_string())?;
    let name = input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    write(output, &tenon_io::stl::write_binary(&mesh, &format!("Tenon STL from {name} (mm)")))?;
    Ok(Report {
        text: format!("wrote {} ({} triangles from {} shape(s))", output.display(), mesh.triangle_count(), shapes.len()),
        json: json!({ "output": output.display().to_string(), "triangles": mesh.triangle_count(), "shapes": shapes.len() }),
    })
}

/// What changed from one Tenon document to another (`tenon-cli diff`).
pub fn diff(a: &Path, b: &Path) -> Result<tenon_io::diff::Diff, String> {
    tenon_io::diff::diff_files(a, b).map_err(|e| e.to_string())
}

/// Saves each file in the current format version (`tenon-cli upgrade`). A version-1 file is kept
/// beside it as `name.v1.ext`; files already current are left alone.
pub fn upgrade(files: &[&Path]) -> Result<Report, String> {
    let mut lines = Vec::new();
    let mut list = Vec::new();
    for path in files {
        let data = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if !tenon_io::project::is_version_1(&data) {
            lines.push(format!("{}: already current", path.display()));
            list.push(json!({ "file": path.display().to_string(), "upgraded": false }));
            continue;
        }
        let full = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let dir = full.parent().map(Path::to_path_buf).unwrap_or_default();
        let err = |e: tenon_io::project::ProjectError| format!("{}: {e}", path.display());
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let kept = match ext.as_str() {
            tenon_io::asm::EXTENSION => tenon_io::asm::save(path, &tenon_io::asm::from_bytes(&data, &dir).map_err(err)?).map_err(err)?,
            tenon_io::drw::EXTENSION => tenon_io::drw::save(path, &tenon_io::drw::from_bytes(&data, &dir).map_err(err)?).map_err(err)?,
            _ => {
                let (doc, extra) = tenon_io::project::from_bytes(&data).map_err(err)?;
                tenon_io::project::save(path, &doc, &extra).map_err(err)?
            }
        };
        let note =
            kept.as_ref().map_or_else(|| " (a version-1 copy was already kept)".to_owned(), |k| format!(" (version 1 kept as {})", k.display()));
        lines.push(format!("{}: upgraded{note}", path.display()));
        list.push(json!({ "file": path.display().to_string(), "upgraded": true, "kept_version_1": kept.map(|k| k.display().to_string()) }));
    }
    Ok(Report { text: lines.join("\n"), json: json!({ "files": list }) })
}

//! Headless Tenon: the logic behind `tenon-cli`. Every command returns a [`Report`] with a human
//! summary and the same data as JSON, so scripts and agents can verify results.
#![forbid(unsafe_code)]

use std::f64::consts::PI;
use std::path::Path;

use serde_json::{Value, json};
use tenon_geom::{Axis, Frame, Vec3};
use tenon_kernel::{BoolOp, KResult, Kernel, Mesh, MeshTol, ShapeHandle};

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

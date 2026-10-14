//! Working a stress study out (M11): the part rebuilt, handed to the mesher as a STEP file,
//! the study's supports and loads put onto the mesh, the solver's answer read back.
//!
//! The mesher and the solver are programs of their own that are started and exchange files
//! with Tenon (`tenon_fea::tools`, DEC-098). A quick check is an engineering aid, not a
//! certified analysis, and every result says so.

use std::path::Path;

use serde_json::{Value, json};
use tenon_fea::{Cancel, Held, Material, Pushed, Results, Scratch, Setup, Surface, Took, Tools};
use tenon_kernel::{Kernel, MeshTol, SurfaceKind};
use tenon_model::schema::arg;
use tenon_model::study::StudyId;
use tenon_model::{CmdError, CmdResult, Session};

/// What every result is to be read as.
pub const NOTE: &str = "An engineering aid, not a certified analysis: linear, static, one solid of one material.";

/// A study worked out.
#[derive(Clone, Debug)]
pub struct Solved {
    pub study: StudyId,
    pub results: Results,
    pub took: Took,
    /// How large the elements were asked to be, mm.
    pub element_size: f64,
    /// The stress at which the part's material stays bent, MPa.
    pub yield_strength: f64,
}

impl Solved {
    /// How many times over the part holds: yield strength over the most the material is
    /// worked. None when nothing works it.
    pub fn safety_factor(&self) -> Option<f64> {
        self.results.safety_factor(self.yield_strength)
    }

    /// The outcome in a few figures, as the command gives it.
    pub fn summary(&self, name: &str) -> Value {
        let r = &self.results;
        let at = |p: tenon_geom::Vec3| json!([p.x, p.y, p.z]);
        let factor = self.safety_factor();
        json!({
            "study": self.study.0,
            "name": name,
            "nodes": r.mesh.nodes.len(),
            "elements": r.mesh.tets.len(),
            "element_size": self.element_size,
            "max_von_mises": { "megapascals": r.max_von_mises.value, "at": at(r.max_von_mises.at) },
            "max_displacement": { "millimetres": r.max_displacement.value, "at": at(r.max_displacement.at) },
            "yield_strength": self.yield_strength,
            "safety_factor": factor,
            "holds": factor.is_none_or(|f| f >= 1.0),
            "seconds": { "meshing": self.took.meshing, "solving": self.took.solving },
            "note": NOTE,
        })
    }
}

/// Works study `id` of the session's part out. The mesher's and the solver's files are left
/// in `keep` when a folder is given, and otherwise go when it is done. `cancel` stops it.
pub fn work_out(s: &mut Session, k: &mut dyn Kernel, id: StudyId, keep: Option<&Path>, cancel: &Cancel) -> Result<Solved, String> {
    // Held against the part first: what a study lacks is said in its own words, before any
    // program is started.
    let check = s.exec("study.check", &json!({ "study": id.0 }), Some(&mut *k)).map_err(|e| e.0)?;
    if check["ready"] != true {
        let needs: Vec<&str> = check["needs"].as_array().map(|n| n.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        return Err(format!("{} cannot be worked out yet. It needs {}.", check["name"].as_str().unwrap_or("The study"), needs.join("; ")));
    }
    let tools = Tools::find()?;
    let r = s.regen(k).clone();
    let d = s.document();
    let (st, strength) = (d.study(id).ok_or("no such study")?, d.strength().ok_or("the part's material does not say how strong it is")?);
    let body = r.bodies.first().ok_or("the part has no solid")?;

    // What each face of the solid is, and where the study's supports and loads are on it.
    let count = u32::try_from(body.names.len()).map_err(|_| "the solid has too many faces")?;
    let mut surfaces = Vec::with_capacity(count as usize);
    for face in 0..count {
        let info = k.face_info(body.shape.face(face)).map_err(|e| e.to_string())?;
        surfaces.push(match info.surface {
            SurfaceKind::Plane { normal, .. } => Surface::Plane { normal },
            SurfaceKind::Cylinder { axis, .. } => Surface::Cylinder { origin: axis.origin(), dir: axis.dir() },
            _ => Surface::Other,
        });
    }
    let on_part = |faces: &[tenon_model::FaceRef], name: &str| -> Result<Vec<u32>, String> {
        faces.iter().map(|f| r.resolve(f, k).map(|(_, face)| face).map_err(|e| format!("{name}: {e}"))).collect()
    };
    let mut held = Vec::new();
    for x in st.supports.iter().filter(|x| !x.suppressed) {
        held.push(Held { name: x.name.clone(), kind: x.kind, faces: on_part(&x.faces, &x.name)? });
    }
    let mut pushed = Vec::new();
    for l in st.loads.iter().filter(|l| !l.suppressed) {
        pushed.push(Pushed { name: l.name.clone(), kind: l.kind, faces: on_part(&l.faces, &l.name)? });
    }
    let material = Material::of(strength.youngs_modulus, strength.poisson, d.density());
    let setup = Setup { held, pushed, surfaces, material };

    let (step, drawn, element_size) = for_the_mesher(k, body.shape, st.mesh_size)?;
    let scratch = match keep {
        Some(dir) => Scratch::in_folder(dir)?,
        None => Scratch::new()?,
    };
    let (results, took) = tenon_fea::work_out(&tools, &step, &drawn, &setup, element_size, &scratch.dir, cancel)?;
    Ok(Solved { study: id, results, took, element_size, yield_strength: strength.yield_strength })
}

/// The solid as the mesher reads it (a STEP file's bytes), as it is drawn (to tell its faces on
/// the mesh), and how large its elements are to be: `size`, or from the part's own size.
fn for_the_mesher(k: &mut dyn Kernel, shape: tenon_kernel::ShapeHandle, size: Option<f64>) -> Result<(Vec<u8>, tenon_kernel::Mesh, f64), String> {
    let step = k.export_step(&[shape]).map_err(|e| e.to_string())?;
    let drawn = k.tessellate(shape, &MeshTol::exact()).map_err(|e| e.to_string())?;
    let at = |p: &[f32; 3]| tenon_geom::Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
    let Some(first) = drawn.positions.first().map(at) else { return Err("the part has nothing to divide into elements".into()) };
    let (lo, hi) = drawn.positions.iter().map(at).fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
    let element_size = size.unwrap_or_else(|| tenon_fea::element_size_for((hi - lo).len()));
    Ok((step, drawn, element_size))
}

/// The part divided into elements as study `id` sizes them, and no more (Mesh View): the
/// study need not be ready to be solved. Returns the mesh and the size its elements were
/// asked to be.
pub fn mesh(s: &mut Session, k: &mut dyn Kernel, id: StudyId, cancel: &Cancel) -> Result<(tenon_fea::TetMesh, f64), String> {
    let tools = Tools::find()?;
    let r = s.regen(k).clone();
    if let Some((_, why)) = r.first_error() {
        return Err(format!("the part does not build: {why}"));
    }
    let body = match r.bodies.as_slice() {
        [one] => one,
        [] => return Err("the part has no solid to divide into elements".into()),
        many => return Err(format!("the part has {} solids, and a study is of one", many.len())),
    };
    let size = s.document().study(id).ok_or("no such study")?.mesh_size;
    let (step, _, element_size) = for_the_mesher(k, body.shape, size)?;
    let scratch = Scratch::new()?;
    Ok((tenon_fea::mesh_part(&tools, &step, element_size, &scratch.dir, cancel)?, element_size))
}

/// `study.solve`: works a study out and says what it comes to.
pub(crate) fn solve(s: &mut Session, k: &mut dyn Kernel, p: &Value) -> CmdResult {
    let id = match arg(p, "study").filter(|v| !v.is_null()) {
        Some(v) => StudyId(v.as_u64().and_then(|v| u32::try_from(v).ok()).ok_or("`study` must be an id")?),
        None => match s.document().studies() {
            [only] => only.id,
            [] => return Err("the part has no study yet: study.new makes one".into()),
            _ => return Err("the part has several studies: say which with `study` (study.list says which there are)".into()),
        },
    };
    let name = s
        .document()
        .study(id)
        .map(|st| st.name.clone())
        .ok_or_else(|| CmdError(format!("there is no study {}: study.list says which there are", id.0)))?;
    let keep = match arg(p, "keep").filter(|v| !v.is_null()) {
        Some(_) => Some(crate::cmd::path_in(p, "keep")?),
        None => None,
    };
    let solved = work_out(s, k, id, keep.as_deref(), &Cancel::default()).map_err(CmdError)?;
    let mut out = solved.summary(&name);
    if let Some(dir) = keep {
        out["kept"] = json!(dir.display().to_string());
    }
    Ok(out)
}

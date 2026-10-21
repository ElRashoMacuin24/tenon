//! Optimize Shape (DEC-123): where in a part material is needed to carry a stress study's loads
//! to its supports, for a share of its mass kept.
//!
//! The part is divided into elements by the mesher and the study put on them as for a stress
//! check (`crate::stress`); the optimizer (`tenon_fea::topo`) then asks the solver, round after
//! round, how hard each element works, and moves material toward those that work hardest. What
//! is kept is made into a closed surface to look at and to export. It is a guide to model over,
//! not a finished part: the part made from it is checked with Simulate.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tenon_fea::topo::{self, Ccx, Extracted, Field, Mirror, Optimized, Phase, Progress, Report, Stop, region};
use tenon_fea::{Cancel, Deck, Scratch, TetMesh, Took, Tools};
use tenon_kernel::{Kernel, SurfaceKind};
use tenon_model::PlaneRef;
use tenon_model::schema::arg;
use tenon_model::study::{Optimize, Resolution, StudyId, Target};
use tenon_model::{CmdError, CmdResult, Session, WorkGeom};

use crate::meshcheck::Welded;

/// What every shape is to be read as.
pub const NOTE: &str = "A guide to where material is needed for these supports and loads, not a finished part: model over it and check what you make with Simulate. Linear, static, one solid of one material.";

/// The most time a run may take.
pub const BUDGET: Duration = Duration::from_secs(60 * 60);
/// The most rounds when none are asked: with sharpening, and without.
pub const ROUNDS_SHARPENED: usize = 100;
pub const ROUNDS_QUICK: usize = 60;
/// How many pairs of smoothing passes the shape is given when none are asked.
pub const SMOOTHING: usize = 10;
/// The most smoothing passes taken.
pub const MAX_SMOOTHING: usize = 50;

/// A shape worked out.
#[derive(Clone, Debug)]
pub struct Shaped {
    pub study: StudyId,
    /// What was asked.
    pub settings: Optimize,
    /// The share of the part's volume kept.
    pub fraction: f64,
    /// How large the elements were asked to be, and the thinnest member the filter allows.
    pub element_size: f64,
    pub member_size: f64,
    /// The densities, node by node, on the mesh they were found on.
    pub field: Field,
    pub optimized: Optimized,
    /// The surface for the share kept.
    pub shown: Extracted,
    /// The work of the loads on the shape as it is shown, solid where it is kept and empty
    /// elsewhere (N mm); none when it was not solved (the run was stopped).
    pub shape_compliance: Option<f64>,
    /// The part's volume (mm^3) and density (g/cm^3).
    pub part_volume: f64,
    pub density: f64,
    pub took: Took,
    /// What the user should know of the shape.
    pub warnings: Vec<String>,
}

impl Shaped {
    /// The mass of a share of the part, g.
    pub fn grams(&self, share: f64) -> f64 {
        share * self.part_volume * self.density / 1000.0
    }

    /// A surface for another share of the part (the same densities, another threshold): it
    /// shows where material ranks, not what an optimization for that share would make.
    pub fn reshaped(&self, share: f64, smooth: usize) -> Extracted {
        topo::iso::extract(&self.field, share, smooth.min(MAX_SMOOTHING))
    }

    /// What the shape comes to, for the command and the agent that asked.
    pub fn summary(&self, name: &str) -> Value {
        let o = &self.optimized;
        let last = o.history.last();
        let skin = &self.shown.skin;
        json!({
            "study": self.study.0,
            "name": name,
            "nodes": self.field.mesh.nodes.len(),
            "elements": self.field.mesh.tets.len(),
            "element_size": self.element_size,
            "member_size": self.member_size,
            "target": { "fraction": self.fraction, "grams": self.grams(self.fraction) },
            "iterations": o.history.len(),
            "stopped": o.stop.word(),
            "history": o.history.iter().map(|i| json!({
                "iteration": i.number,
                "compliance": i.compliance,
                "fraction": i.fraction,
                "change": i.change,
                "sharpness": i.sharpness,
                "grey": i.grey,
                "seconds": i.seconds,
            })).collect::<Vec<_>>(),
            "compliance": { "solid": o.solid_compliance, "optimizer": last.map(|i| i.compliance), "shape": self.shape_compliance },
            "stiffness_kept": self.shape_compliance.filter(|c| *c > 0.0).map(|c| o.solid_compliance / c),
            "mass": { "part_grams": self.grams(1.0), "shape_grams": self.grams(self.shown.share) },
            "surface": {
                "share": self.shown.share,
                "threshold": self.shown.threshold,
                "triangles": skin.triangles.len(),
                "watertight": skin.is_closed(),
                "islands_dropped": self.shown.islands_dropped,
                "volume": skin.volume(),
            },
            "warnings": self.warnings,
            "seconds": { "meshing": self.took.meshing, "solving": self.took.solving },
            "note": NOTE,
        })
    }
}

/// Tells `progress`, if there is one, where the run is.
fn report(progress: Option<&Progress>, phase: Phase, most: usize, last: Option<topo::Iteration>) {
    if let Some(p) = progress {
        let mut r = p.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *r = Report { phase, last: last.or(r.last), most };
    }
}

/// The share of the part `target` keeps, of a part of `grams`.
fn fraction_of(target: Target, grams: f64) -> Result<f64, String> {
    match target {
        Target::Remove(r) => Ok(1.0 - r / 100.0),
        Target::Grams(g) if g >= grams => Err(format!("the part is {} g, so {} g is not less than it: ask for less", round(grams), round(g))),
        Target::Grams(g) => {
            let f = g / grams;
            if f < 1.0 - tenon_model::study::MAX_REMOVE / 100.0 {
                Err(format!("{} g is less than {}% of the part's {} g: ask for more", round(g), 100.0 - tenon_model::study::MAX_REMOVE, round(grams)))
            } else {
                Ok(f)
            }
        }
    }
}

/// A number to three figures after the point, for messages.
fn round(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// How large the elements are to be: as asked, or about the resolution's number of nodes for a
/// part of `volume`, and fine enough for three across the thinnest member asked.
pub fn element_size(o: &Optimize, volume: f64) -> f64 {
    if let Some(h) = o.element_size {
        return h;
    }
    // (A second-order mesh has about nine nodes to each cube of the element size.)
    let by_nodes = (9.0 * volume / o.resolution.nodes() as f64).cbrt();
    match o.min_member {
        Some(m) => by_nodes.min(m / 3.0),
        None => by_nodes,
    }
}

/// The plane `plane` of the rebuilt part, as a mirror, and its name for messages.
fn mirror_of(plane: &PlaneRef, r: &tenon_model::Regen, k: &dyn Kernel, doc: &tenon_model::Document) -> Result<(Mirror, String), String> {
    match plane {
        PlaneRef::Origin(o) => {
            let f = o.frame();
            Ok((Mirror { point: f.origin(), normal: f.z() }, format!("{o:?}")))
        }
        PlaneRef::Work(id) => {
            let name = doc.feature(*id).map_or_else(|| format!("work plane {}", id.0), |f| f.name.clone());
            match r.work.get(id) {
                Some(WorkGeom::Plane(f)) => Ok((Mirror { point: f.origin(), normal: f.z() }, name)),
                _ => Err(format!("{name} is not a work plane that is there: a plane of symmetry must be a plane")),
            }
        }
        PlaneRef::Face(fref) => {
            let (body, face) = r.resolve(fref, k)?;
            let b = r.bodies.get(body).ok_or("no such body")?;
            match k.face_info(b.shape.face(face)).map_err(|e| e.to_string())?.surface {
                SurfaceKind::Plane { origin, normal } => Ok((Mirror { point: origin, normal }, "the face".to_owned())),
                _ => Err("a plane of symmetry must be flat: the face chosen is not".into()),
            }
        }
    }
}

/// Works out the shape of study `id` of the session's part as `settings` ask, the surface for
/// it smoothed `smooth` pairs of passes. The mesher's and the solver's files are left in `keep`
/// when a folder is given. `cancel` stops it: once a round is done, with the shape so far.
/// `progress` is told where it is.
pub fn work_out(
    s: &mut Session,
    k: &mut dyn Kernel,
    id: StudyId,
    settings: &Optimize,
    smooth: usize,
    keep: Option<&Path>,
    cancel: &Cancel,
    progress: Option<&Progress>,
) -> Result<Shaped, String> {
    let started = Instant::now();
    settings.check()?;
    // What the study lacks, and what Optimize Shape cannot take, said before anything starts.
    let check = s.exec("study.check", &json!({ "study": id.0 }), Some(&mut *k)).map_err(|e| e.0)?;
    let name = check["name"].as_str().unwrap_or("The study").to_owned();
    if check["ready"] != true {
        let needs: Vec<&str> = check["needs"].as_array().map(|n| n.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        return Err(format!("{name} cannot be worked out yet. It needs {}.", needs.join("; ")));
    }
    let study = s.document().study(id).ok_or("no such study")?.clone();
    if let Some(l) = study.loads.iter().find(|l| !l.suppressed && l.kind.whole_part().is_some()) {
        return Err(format!("Optimize Shape does not take {} yet: its load depends on how much material is kept. Suppress it to optimize.", l.name));
    }
    let on = crate::stress::setup_of(s, k, id)?;
    let r = &on.regen;
    // Kept faces, and planes of symmetry, on the part as it now is.
    let mut kept_faces: Vec<(Vec<u32>, Option<f64>, String)> = Vec::new();
    for kept in &settings.kept {
        let faces =
            kept.faces.iter().map(|f| r.resolve(f, k).map(|(_, face)| face).map_err(|e| format!("{}: {e}", kept.name))).collect::<Result<_, _>>()?;
        kept_faces.push((faces, kept.depth, kept.name.clone()));
    }
    let doc = s.document().clone();
    let mut mirrors = Vec::new();
    for plane in &settings.symmetry {
        mirrors.push(mirror_of(plane, r, k, &doc)?);
    }
    topo::mirror_group(&mirrors.iter().map(|(m, _)| *m).collect::<Vec<_>>())?;
    // How much to keep, and how fine.
    let density = doc.density();
    let part_volume = k.mass_properties(on.shape, 1.0).map_err(|e| e.to_string())?.volume;
    if !(part_volume.is_finite() && part_volume > 0.0) {
        return Err("the part has no volume to shape".into());
    }
    let fraction = fraction_of(settings.target, part_volume * density / 1000.0)?;
    let h = element_size(settings, part_volume);

    let tools = Tools::find()?;
    let scratch = match keep {
        Some(dir) => Scratch::in_folder(dir)?,
        None => Scratch::new()?,
    };
    let most = settings.iterations.map_or(if settings.sharpen { ROUNDS_SHARPENED } else { ROUNDS_QUICK }, |n| n as usize);
    report(progress, Phase::Meshing, most, None);
    let (step, drawn, _) = crate::stress::for_the_mesher(k, on.shape, Some(h))?;
    let prepared = tenon_fea::prepare(&tools, &step, &drawn, &on.setup, h, &scratch.dir, cancel, topo::MAX_SHAPE_NODES)?;
    let problem = prepared.problem;
    let mesh: &TetMesh = &problem.mesh;

    // What must stay solid: what is held or loaded, and the kept faces as deep as asked.
    let kept_sets: Vec<(Vec<u32>, Option<f64>)> = kept_faces
        .iter()
        .map(|(faces, depth, _)| (prepared.on.iter().filter(|(_, face)| faces.contains(face)).map(|(surface, _)| *surface).collect(), *depth))
        .collect();
    let passive = region::passive(&problem, &kept_sets);
    let design = region::design(mesh, passive.clone());
    let extent = mesh.extent();
    for (m, plane) in &mirrors {
        let off = region::asymmetry(mesh, m);
        if off > 0.1 * h + 1e-6 * extent {
            return Err(format!(
                "The part is not the same on both sides of {plane} (a point of it is {} mm off its mirror image): take the plane away, or choose one it is the same on both sides of.",
                round(off)
            ));
        }
    }
    let total: f64 = design.volumes.iter().sum();
    let solid: f64 = design.volumes.iter().zip(&passive).filter(|(_, p)| **p).map(|(v, _)| *v).sum();
    if solid >= fraction * total {
        let names: Vec<&str> = kept_faces.iter().map(|(_, _, n)| n.as_str()).collect();
        let what = if names.is_empty() {
            "The layers under the supports and loads".to_owned()
        } else {
            format!("{} and the layers under the supports and loads", names.join(", "))
        };
        return Err(format!(
            "{what} are already {:.0}% of the part: more than the {:.0}% kept. Remove less, or keep less.",
            100.0 * solid / total,
            100.0 * fraction
        ));
    }

    // The thinnest member: as asked, or three elements; the filter's radius half of it, and
    // at least one and a half elements.
    let radius = (settings.min_member.unwrap_or(3.0 * h) / 2.0).max(1.5 * h);
    let goal = topo::Goal {
        fraction,
        radius,
        sharpen: settings.sharpen,
        max_iterations: most,
        budget: BUDGET.saturating_sub(started.elapsed()),
        mirrors: mirrors.iter().map(|(m, _)| *m).collect(),
    };
    let deck = Deck::new(&problem)?;
    let (e0, nu) = (problem.material.youngs_modulus, problem.material.poisson);
    let mut solver = Ccx { tools: &tools, deck: &deck, e0, nu, dir: &scratch.dir, cancel };
    report(progress, Phase::Solid, most, None);
    let optimized = topo::optimize(&design, &goal, &mut solver, cancel, &mut |it| report(progress, Phase::Iterating, most, Some(*it)))?;

    // The surface of what is kept.
    report(progress, Phase::Shaping, most, None);
    let node_set = |nodes: &mut dyn Iterator<Item = u32>| {
        let mut on = vec![false; mesh.nodes.len()];
        for n in nodes {
            if let Some(x) = on.get_mut(n as usize) {
                *x = true;
            }
        }
        on
    };
    let passive_node = node_set(&mut mesh.tets.iter().zip(&passive).filter(|(_, p)| **p).flat_map(|(t, _)| t.iter().copied()));
    let held_node = node_set(&mut problem.held.keys().copied());
    let loaded_node = node_set(&mut problem.forces.keys().copied());
    let field = Field { rho: topo::iso::node_density(mesh, &optimized.density, &passive), mesh: mesh.clone(), passive_node, held_node, loaded_node };
    let shown = topo::iso::extract(&field, fraction, smooth.min(MAX_SMOOTHING));

    // The shape solved once, solid where it is kept and empty elsewhere: how stiff it is against
    // the part. (Kept: the densest elements, those that must stay first, up to the share kept.
    // Densities averaged onto the nodes would thin a member one element thick until it broke.)
    report(progress, Phase::Checking, most, None);
    let levels = kept_levels(&optimized.density, &design.volumes, &passive, shown.share);
    let shape_compliance = match tenon_fea::solve_shape(&tools, &deck, &levels, e0, nu, &scratch.dir, cancel, false) {
        Ok((u, _)) => Some(2.0 * u.iter().sum::<f64>()),
        Err(e) if e == tenon_fea::tools::STOPPED => None,
        Err(e) => return Err(e),
    };

    let mut warnings = Vec::new();
    match optimized.stop {
        Stop::Limit => {
            warnings.push(format!("It stopped at the most rounds ({}) before it settled: the shape may still be changing.", optimized.history.len()))
        }
        Stop::Budget => warnings.push("It ran out of time (an hour) before it settled: the shape may still be changing.".to_owned()),
        Stop::Stopped => warnings.push("It was stopped: this is the shape so far.".to_owned()),
        Stop::Settled | Stop::LevelledOff => {}
    }
    if shown.islands_dropped > 0 {
        let n = shown.islands_dropped;
        warnings.push(format!(
            "{n} piece{} of material cut off from everything that must stay {} left out.",
            if n == 1 { "" } else { "s" },
            if n == 1 { "was" } else { "were" }
        ));
    }
    if shown.load_not_joined {
        warnings
            .push("The material kept does not join every load to a support: keep a larger share, or ask for a smaller smallest member.".to_owned());
    }
    let meshing = prepared.meshing;
    Ok(Shaped {
        study: id,
        settings: settings.clone(),
        fraction,
        element_size: h,
        member_size: 2.0 * radius,
        field,
        optimized,
        shown,
        shape_compliance,
        part_volume,
        density,
        took: Took { meshing, solving: (started.elapsed().as_secs_f64() - meshing).max(0.0) },
        warnings,
    })
}

/// Each element solid (the top level) or empty (0): those that must stay, then the densest, until
/// `share` of the volume is kept. Ties go by element order.
pub fn kept_levels(density: &[f64], volumes: &[f64], passive: &[bool], share: f64) -> Vec<u16> {
    let total: f64 = volumes.iter().sum();
    let mut order: Vec<usize> = (0..density.len()).collect();
    let rank = |i: usize| if passive.get(i).copied().unwrap_or(false) { f64::INFINITY } else { density.get(i).copied().unwrap_or(0.0) };
    order.sort_by(|a, b| rank(*b).total_cmp(&rank(*a)).then(a.cmp(b)));
    let mut levels = vec![0; density.len()];
    let mut kept = 0.0;
    for i in order {
        let solid = passive.get(i).copied().unwrap_or(false);
        if !solid && kept >= share * total {
            break;
        }
        levels[i] = topo::LEVELS;
        kept += volumes.get(i).copied().unwrap_or(0.0);
    }
    levels
}

/// Writes a shape's surface to `path`: STL (binary) or 3MF, by the file's ending.
pub fn write_shape(path: &Path, skin: &topo::Skin, name: &str, material: &str, color: [u8; 3]) -> Result<(), String> {
    let mesh = skin.to_mesh();
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    let data = match ext.as_str() {
        "stl" => crate::stl::write_binary(&mesh, &format!("Tenon {name} shape (mm)")),
        "3mf" => {
            let object = crate::threemf::Object { name: format!("{name} shape"), mesh: Welded::of(&mesh), material: material.to_owned(), color };
            let model = crate::threemf::Model {
                title: format!("{name} shape"),
                objects: vec![object],
                items: vec![crate::threemf::Item { object: 0, frame: None }],
            };
            crate::threemf::write(&model)?
        }
        _ => return Err(format!("a shape is written as .stl or .3mf, not as {}", path.display())),
    };
    std::fs::write(path, data).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// The settings a command asks for: the study's own, with what the command gives in their place
/// for this run.
fn asked(s: &Session, id: StudyId, p: &Value) -> Result<Optimize, CmdError> {
    let mut o = s.document().study(id).and_then(|st| st.optimize.clone()).unwrap_or_default();
    let num = |key: &str| -> Result<Option<f64>, CmdError> {
        match arg(p, key).filter(|v| !v.is_null()) {
            None => Ok(None),
            Some(v) => v.as_f64().filter(|v| v.is_finite()).map(Some).ok_or_else(|| CmdError(format!("`{key}` must be a number"))),
        }
    };
    match (num("remove")?, num("grams")?) {
        (Some(_), Some(_)) => return Err("give `remove` or `grams`, not both".into()),
        (Some(r), None) => o.target = Target::Remove(r),
        (None, Some(g)) => o.target = Target::Grams(g),
        (None, None) => {}
    }
    if let Some(m) = num("min_member")? {
        o.min_member = Some(m);
    }
    if let Some(h) = num("element_size")? {
        o.element_size = Some(h);
    }
    if let Some(r) = arg(p, "resolution").filter(|v| !v.is_null()) {
        o.resolution = serde_json::from_value::<Resolution>(r.clone()).map_err(|_| CmdError("`resolution` is coarse, medium or fine".into()))?;
    }
    if let Some(b) = arg(p, "sharpen").filter(|v| !v.is_null()) {
        o.sharpen = b.as_bool().ok_or("`sharpen` must be true or false")?;
    }
    if let Some(n) = arg(p, "iterations").filter(|v| !v.is_null()) {
        o.iterations = Some(n.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or("`iterations` must be a whole number")?);
    }
    o.check().map_err(CmdError)?;
    Ok(o)
}

/// `study.optimize`: works a study's shape out and says what it comes to.
pub(crate) fn optimize(s: &mut Session, k: &mut dyn Kernel, p: &Value) -> CmdResult {
    let id = crate::stress::study_arg(s, p)?;
    let name = s.document().study(id).map(|st| st.name.clone()).ok_or_else(|| CmdError(format!("there is no study {}", id.0)))?;
    let settings = asked(s, id, p)?;
    let smooth = match arg(p, "smooth").filter(|v| !v.is_null()) {
        Some(v) => v
            .as_u64()
            .filter(|n| *n as usize <= MAX_SMOOTHING)
            .ok_or_else(|| CmdError(format!("`smooth` is a whole number of passes, 0 to {MAX_SMOOTHING}")))? as usize,
        None => SMOOTHING,
    };
    let shown = match arg(p, "shown").filter(|v| !v.is_null()) {
        Some(v) => Some(v.as_f64().filter(|v| (0.05..=1.0).contains(v)).ok_or("`shown` is a share of the part from 0.05 to 1")?),
        None => None,
    };
    let path = |key: &str| -> Result<Option<PathBuf>, CmdError> {
        match arg(p, key).filter(|v| !v.is_null()) {
            Some(_) => Ok(Some(crate::cmd::path_in(p, key)?)),
            None => Ok(None),
        }
    };
    let (stl, threemf, keep) = (path("stl")?, path("threemf")?, path("keep")?);
    let shaped = Arc::new(work_out(s, k, id, &settings, smooth, keep.as_deref(), &Cancel::default(), None).map_err(CmdError)?);
    let mut out = shaped.summary(&name);
    let skin = match shown {
        Some(share) => shaped.reshaped(share, smooth).skin,
        None => shaped.shown.skin.clone(),
    };
    let doc = s.document();
    let (color, material) = (doc.color().unwrap_or(crate::cmd::PLAIN), doc.material().map_or_else(|| "Generic".to_owned(), |m| m.name.clone()));
    let mut written = json!({});
    for (key, file) in [("stl", stl), ("threemf", threemf)] {
        if let Some(file) = file {
            write_shape(&file, &skin, &doc.name, &material, color).map_err(CmdError)?;
            written[key] = json!(file.display().to_string());
        }
    }
    out["written"] = written;
    if let Some(dir) = keep {
        out["kept"] = json!(dir.display().to_string());
    }
    Ok(out)
}

//! Optimize Shape (DEC-123): where in a part material is needed to carry a stress study's loads
//! to its supports, for a share of its mass kept.
//!
//! The part is divided into elements by the mesher, coarser round small details than for a stress
//! check (`tenon_fea::mesh_for_shape`), and the study put on them as for one (`crate::stress`); the
//! optimizer (`tenon_fea::topo`) then asks the solver, round after round, how hard each element
//! works, and moves material toward those that work hardest. What is kept is made into a closed
//! surface to look at and to export. It is a guide to model over, not a finished part: the part
//! made from it is checked with Simulate.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tenon_fea::topo::{self, Ccx, Extracted, Field, Mirror, Optimized, Phase, Progress, Report, Stop, region};
use tenon_fea::{Cancel, Deck, Scratch, TetMesh, Took, Tools};
use tenon_kernel::Kernel;
use tenon_model::PlaneRef;
use tenon_model::schema::arg;
use tenon_model::study::{FACE_PLANE_REFUSED, Optimize, Resolution, StudyId, Target};
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
/// How many times a resolution's nodes a mesh may come to before its elements are made larger.
const NODES_OVER: f64 = 1.5;
/// How many more times a part is divided, each with larger elements, to come near them.
const MORE_TRIES: usize = 3;

/// A shape worked out.
#[derive(Clone, Debug)]
pub struct Shaped {
    pub study: StudyId,
    /// What was asked.
    pub settings: Optimize,
    /// The share of the part's volume kept.
    pub fraction: f64,
    /// How large the elements were asked to be (larger than the resolution first picks when the
    /// part's small details took too many nodes), and the thinnest member the filter allows.
    pub element_size: f64,
    pub member_size: f64,
    /// The densities, node by node, on the mesh they were found on.
    pub field: Field,
    pub optimized: Optimized,
    /// The surface for the share kept.
    pub shown: Extracted,
    /// The work of the loads on the shape as it is shown, solid where it is kept and empty
    /// elsewhere (N mm); none when it was not solved (the run was stopped, or the solve failed:
    /// the warnings say).
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

/// How large the elements are to be at first: as asked, or about the resolution's number of nodes
/// for a plain solid of `volume`, and fine enough for three across the thinnest member asked.
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

/// The plane `plane` of the rebuilt part, as a mirror, and its name for messages. (A stored plane
/// that cannot be mirrored in has been refused by `study.check` before this: what is said here
/// is for settings given to [`work_out`] that are not the study's own.)
fn mirror_of(plane: &PlaneRef, r: &tenon_model::Regen, doc: &tenon_model::Document) -> Result<(Mirror, String), String> {
    match plane {
        PlaneRef::Origin(o) => {
            let f = o.frame();
            Ok((Mirror { point: f.origin(), normal: f.z() }, format!("{o:?}")))
        }
        PlaneRef::Work(id) => {
            let name = doc.feature(*id).map_or_else(|| format!("work plane {}", id.0), |f| f.name.clone());
            match r.work.get(id) {
                Some(WorkGeom::Plane(f)) => Ok((Mirror { point: f.origin(), normal: f.z() }, name)),
                _ => Err(format!("{name} cannot be a plane of symmetry: it is not a plane that is on the part")),
            }
        }
        PlaneRef::Face(_) => Err(FACE_PLANE_REFUSED.into()),
    }
}

/// The part divided into elements for a shape, by `divide` at the size it is given, from `h` on,
/// and the size that came to.
///
/// A size that came from the resolution is only a guess from the part's volume, and the small
/// holes and rounds of a part take many more nodes than its volume says. So when the mesh comes
/// out well over the resolution's nodes, or past what a shape is worked out on, the elements are
/// made larger by the cube root of how far over it is, and the part divided again, a few times at
/// most and never past three elements across a smallest member asked. What is kept is the last
/// mesh a shape can be worked out on. A size that was asked for is left as it is.
fn divided(o: &Optimize, h: f64, divide: &mut dyn FnMut(f64) -> Result<TetMesh, String>) -> Result<(TetMesh, f64), String> {
    let mesh = divide(h)?;
    let (mut now, mut nodes) = (h, mesh.nodes.len());
    let mut fits = (nodes <= topo::MAX_SHAPE_NODES).then_some((mesh, h));
    if o.element_size.is_none() {
        let target = o.resolution.nodes() as f64;
        let most = o.min_member.map_or(f64::INFINITY, |m| m / 3.0);
        for _ in 0..MORE_TRIES {
            if nodes as f64 <= NODES_OVER * target && nodes <= topo::MAX_SHAPE_NODES {
                break;
            }
            let larger = (now * (nodes as f64 / target).cbrt()).min(most);
            if larger <= now {
                break;
            }
            now = larger;
            match divide(now) {
                Ok(mesh) => {
                    nodes = mesh.nodes.len();
                    if nodes <= topo::MAX_SHAPE_NODES {
                        fits = Some((mesh, now));
                    }
                }
                Err(e) if e == tenon_fea::tools::STOPPED => return Err(e),
                // (A larger size the mesher cannot do: what it did before stands.)
                Err(e) => {
                    if fits.is_none() {
                        return Err(e);
                    }
                    break;
                }
            }
        }
    }
    fits.ok_or_else(|| too_many_nodes(o, now, nodes))
}

/// Why a mesh is more than a shape is worked out on, and what helps for what was asked.
fn too_many_nodes(o: &Optimize, h: f64, nodes: usize) -> String {
    let helps = if o.element_size.is_some() {
        "Make the elements larger."
    } else if o.min_member.is_some_and(|m| h >= (1.0 - 1e-9) * m / 3.0) {
        "Ask for a larger smallest member."
    } else {
        match o.resolution {
            Resolution::Fine => "Choose Medium.",
            Resolution::Medium => "Choose Coarse.",
            Resolution::Coarse => "Suppress its small holes and rounds to optimize.",
        }
    };
    format!(
        "Divided into elements of {} mm the part has {nodes} nodes: more than a shape is worked out on ({}). {helps}",
        round(h),
        topo::MAX_SHAPE_NODES
    )
}

/// What is said when what must stay solid (`what`, `solid` of the part) leaves no room for the
/// share kept (`fraction`) of a part of `part_grams`: to a tenth where whole figures would read
/// the same, and for a mass asked, the mass that would leave room.
fn too_much_stays(what: &str, solid: f64, fraction: f64, target: Target, part_grams: f64) -> String {
    let (stays, kept) = (100.0 * solid, 100.0 * fraction);
    let (stays, kept) = if format!("{stays:.0}") == format!("{kept:.0}") {
        (format!("{stays:.1}"), format!("{kept:.1}"))
    } else {
        (format!("{stays:.0}"), format!("{kept:.0}"))
    };
    let helps = match target {
        Target::Remove(_) => "Remove less, or keep less.".to_owned(),
        Target::Grams(_) => format!("Ask for more than {} g, or keep less.", round(solid * part_grams)),
    };
    format!("{what} are already {stays}% of the part: more than the {kept}% kept. {helps}")
}

/// What the one solve of the shape comes to, `was_stopped` when the run had been stopped before
/// it: how much work the loads do, or none and what is to be said of that. The shape stands
/// either way, since only how much stiffness it keeps hangs on this solve.
fn checked(solved: Result<f64, String>, was_stopped: bool) -> (Option<f64>, Option<String>) {
    match solved {
        Ok(work) => (Some(work), None),
        Err(e) if e == tenon_fea::tools::STOPPED => {
            // (A run stopped before it has said so, and that is all there is to say.)
            let said = "It was stopped while the shape was being solved once, so how much stiffness it keeps was not worked out.";
            (None, (!was_stopped).then(|| said.to_owned()))
        }
        Err(e) => (None, Some(format!("The shape could not be solved once, so how much stiffness it keeps was not worked out: {e}"))),
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
    let shape_needs: Vec<&str> = check["shape_needs"].as_array().map(|n| n.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    if !shape_needs.is_empty() {
        // (Each its own sentence: some end in a full stop, and some do not.)
        let said: Vec<String> = shape_needs.iter().map(|n| format!("{}.", n.trim_end_matches('.'))).collect();
        return Err(format!("A shape of {name} cannot be worked out yet. {}", said.join(" ")));
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
        mirrors.push(mirror_of(plane, r, &doc)?);
    }
    topo::mirror_group(&mirrors.iter().map(|(m, _)| *m).collect::<Vec<_>>())?;
    // How much to keep, and how fine.
    let density = doc.density();
    let part_volume = k.mass_properties(on.shape, 1.0).map_err(|e| e.to_string())?.volume;
    if !(part_volume.is_finite() && part_volume > 0.0) {
        return Err("the part has no volume to shape".into());
    }
    let fraction = fraction_of(settings.target, part_volume * density / 1000.0)?;
    let first_size = element_size(settings, part_volume);

    let tools = Tools::find()?;
    let scratch = match keep {
        Some(dir) => Scratch::in_folder(dir)?,
        None => Scratch::new()?,
    };
    let most = settings.iterations.map_or(if settings.sharpen { ROUNDS_SHARPENED } else { ROUNDS_QUICK }, |n| n as usize);
    report(progress, Phase::Meshing, most, None);
    let (step, drawn, _) = crate::stress::for_the_mesher(k, on.shape, Some(first_size))?;
    let began = Instant::now();
    let (mesh, h) = divided(settings, first_size, &mut |size| tenon_fea::mesh_for_shape(&tools, &step, size, &scratch.dir, cancel))?;
    let prepared = tenon_fea::put_on(mesh, &drawn, &on.setup, began.elapsed().as_secs_f64())?;
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
    if topo::no_room(solid, total, fraction * total) {
        let names: Vec<&str> = kept_faces.iter().map(|(_, _, n)| n.as_str()).collect();
        let what = if names.is_empty() {
            "The layers under the supports and loads".to_owned()
        } else {
            format!("{} and the layers under the supports and loads", names.join(", "))
        };
        return Err(too_much_stays(&what, solid / total, fraction, settings.target, part_volume * density / 1000.0));
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
    let solved = tenon_fea::solve_shape(&tools, &deck, &levels, e0, nu, &scratch.dir, cancel, false).map(|(u, _)| 2.0 * u.iter().sum::<f64>());
    let (shape_compliance, check_said) = checked(solved, optimized.stop == Stop::Stopped);

    let mut warnings = Vec::new();
    if h > first_size {
        warnings.push(format!(
            "The part's small details take many elements, so the elements were made larger ({} mm) to stay near this resolution's nodes.",
            round(h)
        ));
    }
    match optimized.stop {
        Stop::Limit => {
            warnings.push(format!("It stopped at the most rounds ({}) before it settled: the shape may still be changing.", optimized.history.len()))
        }
        Stop::Budget => warnings.push("It ran out of time (an hour) before it settled: the shape may still be changing.".to_owned()),
        Stop::Stopped => warnings.push("It was stopped: this is the shape so far.".to_owned()),
        Stop::Settled | Stop::LevelledOff => {}
    }
    warnings.extend(check_said);
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
    // (Given as null, the one the study has is not used for this run.)
    let maybe = |key: &str| -> Result<Option<Option<f64>>, CmdError> {
        match arg(p, key) {
            None => Ok(None),
            Some(Value::Null) => Ok(Some(None)),
            Some(_) => num(key).map(Some),
        }
    };
    if let Some(m) = maybe("min_member")? {
        o.min_member = m;
    }
    if let Some(r) = arg(p, "resolution").filter(|v| !v.is_null()) {
        o.resolution = serde_json::from_value::<Resolution>(r.clone()).map_err(|_| CmdError("`resolution` is coarse, medium or fine".into()))?;
        // (Asking for a resolution asks for its size, unless a size is said with it.)
        o.element_size = None;
    }
    if let Some(h) = maybe("element_size")? {
        o.element_size = h;
    }
    if let Some(b) = arg(p, "sharpen").filter(|v| !v.is_null()) {
        o.sharpen = b.as_bool().ok_or("`sharpen` must be true or false")?;
    }
    match arg(p, "iterations") {
        None => {}
        Some(Value::Null) => o.iterations = None,
        Some(n) => o.iterations = Some(Optimize::rounds_of(n).map_err(CmdError)?),
    }
    o.check().map_err(CmdError)?;
    Ok(o)
}

/// Whether a shape can be written to `file` for the parameter `key`, which takes the ending `ext`:
/// said before a run that may take an hour, not after it. Nothing already there is touched: the
/// folder is tried with a file of its own, named to be no other, and removed at once.
fn can_write(key: &str, ext: &str, file: &Path) -> Result<(), CmdError> {
    if !file.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
        return Err(format!("`{key}` must be a file ending in .{ext}, not {}", file.display()).into());
    }
    if file.is_dir() {
        return Err(format!("`{key}`: {} is a folder", file.display()).into());
    }
    let folder = file.parent().filter(|f| !f.as_os_str().is_empty()).unwrap_or(Path::new("."));
    if !folder.is_dir() {
        return Err(format!("`{key}`: there is no folder {}", folder.display()).into());
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let trial = folder.join(format!(".tenon-can-write-{}-{n}", std::process::id()));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&trial) {
        Ok(_) => {
            let _ = std::fs::remove_file(&trial);
        }
        Err(e) => return Err(format!("`{key}`: a file cannot be written in {}: {e}", folder.display()).into()),
    }
    if std::fs::metadata(file).is_ok_and(|m| m.permissions().readonly()) {
        return Err(format!("`{key}`: {} is read-only", file.display()).into());
    }
    Ok(())
}

/// Writes the shape's files, as `write` does, and says in the result `out` what came of each. A
/// file that cannot be written after all does not lose the run: it goes under `not_written`, with
/// why, and among the warnings.
fn write_files(out: &mut Value, files: Vec<(&str, PathBuf)>, write: &dyn Fn(&Path) -> Result<(), String>) {
    let mut written = json!({});
    let mut not_written = serde_json::Map::new();
    for (key, file) in files {
        match write(&file) {
            Ok(()) => written[key] = json!(file.display().to_string()),
            Err(why) => {
                if let Some(warnings) = out["warnings"].as_array_mut() {
                    warnings.push(json!(format!("The shape was worked out, but it was not written: {why}")));
                }
                not_written.insert(key.to_owned(), json!(why));
            }
        }
    }
    out["written"] = written;
    if !not_written.is_empty() {
        out["not_written"] = Value::Object(not_written);
    }
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
    let files: Vec<(&str, PathBuf)> = [("stl", stl), ("threemf", threemf)].into_iter().filter_map(|(key, file)| Some((key, file?))).collect();
    for (key, file) in &files {
        can_write(key, if *key == "stl" { "stl" } else { "3mf" }, file)?;
    }
    let shaped = Arc::new(work_out(s, k, id, &settings, smooth, keep.as_deref(), &Cancel::default(), None).map_err(CmdError)?);
    let mut out = shaped.summary(&name);
    let skin = match shown {
        Some(share) => shaped.reshaped(share, smooth).skin,
        None => shaped.shown.skin.clone(),
    };
    let doc = s.document();
    let (color, material) = (doc.color().unwrap_or(crate::cmd::PLAIN), doc.material().map_or_else(|| "Generic".to_owned(), |m| m.name.clone()));
    write_files(&mut out, files, &|file: &Path| write_shape(file, &skin, &doc.name, &material, color));
    if let Some(dir) = keep {
        out["kept"] = json!(dir.display().to_string());
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// A mesh of so many nodes, for what looks only at how many.
    fn of(nodes: usize) -> TetMesh {
        TetMesh { nodes: vec![tenon_geom::Vec3::ZERO; nodes], ..TetMesh::default() }
    }

    /// What `divided` comes to when the mesher gives `counts` nodes in turn, and the sizes it
    /// asked for.
    fn dividing(o: &Optimize, h: f64, counts: &[usize]) -> (Result<(usize, f64), String>, Vec<f64>) {
        let mut asked = Vec::new();
        let done = divided(o, h, &mut |size| {
            let n = counts[asked.len()];
            asked.push(size);
            Ok(of(n))
        });
        (done.map(|(mesh, size)| (mesh.nodes.len(), size)), asked)
    }

    fn at(resolution: Resolution) -> Optimize {
        Optimize { resolution, ..Optimize::default() }
    }

    #[test]
    fn a_mesh_near_its_resolution_is_kept_as_it_is() {
        let o = at(Resolution::Coarse);
        // Up to one and a half times its nodes.
        assert_eq!(dividing(&o, 3.0, &[18_000]), (Ok((18_000, 3.0)), vec![3.0]));
        assert_eq!(dividing(&o, 3.0, &[9_000]), (Ok((9_000, 3.0)), vec![3.0]));
    }

    #[test]
    fn a_mesh_well_over_its_resolution_is_made_again_with_larger_elements() {
        let o = at(Resolution::Coarse);
        // Larger by the cube root of how far over it is: 96,000 is eight times 12,000.
        let (done, asked) = dividing(&o, 3.0, &[96_000, 15_000]);
        assert_eq!(done, Ok((15_000, 6.0)));
        assert_eq!(asked, vec![3.0, 6.0]);
        // At most three more tries: the last is kept, over or not.
        let (done, asked) = dividing(&o, 3.0, &[96_000, 96_000, 96_000, 96_000]);
        assert_eq!(done, Ok((96_000, 24.0)));
        assert_eq!(asked, vec![3.0, 6.0, 12.0, 24.0]);
        // Past what a shape is worked out on, with no more tries, the last that was within it.
        let (done, asked) = dividing(&o, 3.0, &[96_000, 150_000, 150_000, 150_000]);
        assert_eq!(done.unwrap().0, 96_000);
        assert_eq!(asked.len(), 4);
        // Past it at Fine, though not one and a half times over: larger, not refused.
        let (done, asked) = dividing(&at(Resolution::Fine), 2.0, &[108_000, 79_000]);
        assert_eq!(done.as_ref().map(|(n, _)| *n), Ok(79_000));
        assert!(asked[1] > 2.0, "{asked:?}");
    }

    #[test]
    fn elements_are_not_made_larger_than_a_smallest_member_or_a_size_asked_allows() {
        // Never past three elements across the smallest member asked.
        let o = Optimize { min_member: Some(9.0), ..Optimize::default() };
        let (done, asked) = dividing(&o, 2.0, &[96_000, 30_000]);
        assert_eq!(done, Ok((30_000, 3.0)));
        assert_eq!(asked, vec![2.0, 3.0]);
        // A smallest member that pinned the size leaves nothing to make larger.
        let o = Optimize { min_member: Some(6.0), ..Optimize::default() };
        assert_eq!(dividing(&o, 2.0, &[96_000]), (Ok((96_000, 2.0)), vec![2.0]));
        // Nor is a size that was asked for made larger, however many nodes it comes to.
        let o = Optimize { element_size: Some(2.0), ..Optimize::default() };
        assert_eq!(dividing(&o, 2.0, &[96_000]), (Ok((96_000, 2.0)), vec![2.0]));
    }

    #[test]
    fn too_many_nodes_says_what_helps_for_what_was_asked() {
        let said = |o: &Optimize, h: f64, n: usize| dividing(o, h, &[n; 4]).0.unwrap_err();
        let start = "Divided into elements of";
        let ends = |e: &str, tail: &str| assert!(e.starts_with(start) && e.ends_with(tail), "{e}");
        assert_eq!(
            said(&Optimize { element_size: Some(1.25), ..Optimize::default() }, 1.25, 140_000),
            "Divided into elements of 1.25 mm the part has 140000 nodes: more than a shape is worked out on (100000). Make the elements larger."
        );
        ends(&said(&Optimize { min_member: Some(3.0), ..Optimize::default() }, 1.0, 140_000), "Ask for a larger smallest member.");
        // A smallest member that let the size grow, until it stopped it.
        let o = Optimize { min_member: Some(9.0), ..Optimize::default() };
        ends(&said(&o, 2.0, 140_000), "Ask for a larger smallest member.");
        ends(&said(&at(Resolution::Fine), 1.0, 900_000), "Choose Medium.");
        ends(&said(&at(Resolution::Medium), 1.0, 900_000), "Choose Coarse.");
        let coarse = said(&at(Resolution::Coarse), 1.0, 900_000);
        ends(&coarse, "Suppress its small holes and rounds to optimize.");
        assert!(!coarse.contains("Coarse"), "{coarse}");
        // It is said of the last size tried.
        assert!(coarse.starts_with("Divided into elements of "), "{coarse}");
        assert!(!coarse.contains("of 1 mm"), "{coarse}");
    }

    #[test]
    fn a_mesher_that_fails_on_a_larger_size_leaves_what_it_did_before() {
        let o = at(Resolution::Coarse);
        let mut calls = 0;
        let mut failing = |why: &'static str| {
            calls = 0;
            divided(&o, 3.0, &mut |_| {
                calls += 1;
                if calls == 1 { Ok(of(50_000)) } else { Err(why.to_owned()) }
            })
            .map(|(mesh, size)| (mesh.nodes.len(), size))
        };
        assert_eq!(failing("the mesher fell over"), Ok((50_000, 3.0)));
        // Being stopped is being stopped, whatever there is so far.
        assert_eq!(failing(tenon_fea::tools::STOPPED), Err(tenon_fea::tools::STOPPED.to_owned()));
        // With nothing it can be worked out on, the failure is what is said.
        let mut calls = 0;
        let e = divided(&o, 3.0, &mut |_| {
            calls += 1;
            if calls == 1 { Ok(of(500_000)) } else { Err("the mesher fell over".to_owned()) }
        })
        .unwrap_err();
        assert_eq!(e, "the mesher fell over");
        // And one that fails at once, as it is.
        assert_eq!(divided(&o, 3.0, &mut |_| Err("no mesher".to_owned())).unwrap_err(), "no mesher");
    }

    #[test]
    fn too_much_kept_is_said_so_that_it_can_be_read() {
        let what = "The layers under the supports and loads";
        // Two whole figures that differ are enough.
        assert_eq!(
            too_much_stays(what, 0.7, 0.57, Target::Remove(43.0), 141.3),
            "The layers under the supports and loads are already 70% of the part: more than the 57% kept. Remove less, or keep less."
        );
        // Two that would read the same are given to a tenth.
        assert_eq!(
            too_much_stays(what, 0.503, 0.5, Target::Remove(50.0), 141.3),
            "The layers under the supports and loads are already 50.3% of the part: more than the 50.0% kept. Remove less, or keep less."
        );
        // A mass asked for: the mass that would leave room. (70% of 141.3 g.)
        assert_eq!(
            too_much_stays(what, 0.7, 0.57, Target::Grams(80.0), 141.3),
            "The layers under the supports and loads are already 70% of the part: more than the 57% kept. Ask for more than 98.91 g, or keep less."
        );
    }

    #[test]
    fn a_shape_stands_without_its_one_solve() {
        assert_eq!(checked(Ok(12.5), false), (Some(12.5), None));
        // Stopped during that solve: said, as nothing else says it.
        let (work, said) = checked(Err(tenon_fea::tools::STOPPED.to_owned()), false);
        assert_eq!(
            (work, said.as_deref()),
            (None, Some("It was stopped while the shape was being solved once, so how much stiffness it keeps was not worked out."))
        );
        // Stopped before it: the run's own warning says that.
        assert_eq!(checked(Err(tenon_fea::tools::STOPPED.to_owned()), true), (None, None));
        // A failure says why, and the shape is kept.
        let (work, said) = checked(Err("The shape's study could not be solved. ccx failed".to_owned()), false);
        assert_eq!(
            (work, said.as_deref()),
            (
                None,
                Some(
                    "The shape could not be solved once, so how much stiffness it keeps was not worked out: The shape's study could not be solved. ccx failed"
                )
            )
        );
    }

    /// A folder of its own for a test, gone when dropped.
    fn folder() -> tenon_fea::Scratch {
        tenon_fea::Scratch::new().unwrap()
    }

    #[test]
    fn where_a_shape_goes_is_tried_without_touching_what_is_there() {
        let scratch = folder();
        let dir = &scratch.dir;
        // Any case of the ending, and nothing there yet; nothing is left behind.
        assert!(can_write("stl", "stl", &dir.join("SHAPE.STL")).is_ok());
        assert!(can_write("threemf", "3mf", &dir.join("shape.3Mf")).is_ok());
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), 0);
        // A file already there is left as it is.
        let there = dir.join("there.stl");
        std::fs::write(&there, b"what was there").unwrap();
        assert!(can_write("stl", "stl", &there).is_ok());
        assert_eq!(std::fs::read(&there).unwrap(), b"what was there");
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), 1);
        // A name with no folder is in the working one.
        assert!(can_write("stl", "stl", Path::new("in-the-working-folder.stl")).is_ok());
        assert!(!Path::new("in-the-working-folder.stl").exists());
        // An ending of the other kind, or none.
        let refused = |key: &str, ext: &str, file: &Path| can_write(key, ext, file).unwrap_err().0;
        assert!(refused("stl", "stl", &dir.join("y.3mf")).starts_with("`stl` must be a file ending in .stl"));
        assert!(refused("threemf", "3mf", &dir.join("x.stl")).starts_with("`threemf` must be a file ending in .3mf"));
        assert!(refused("stl", "stl", &dir.join("shape")).starts_with("`stl` must be a file ending in .stl"));
        // A folder for a file, and a folder that is not there.
        std::fs::create_dir(dir.join("a folder.stl")).unwrap();
        assert!(refused("stl", "stl", &dir.join("a folder.stl")).ends_with("is a folder"));
        let e = refused("stl", "stl", &dir.join("no").join("such").join("z.stl"));
        assert!(e.starts_with("`stl`: there is no folder"), "{e}");
        // A target that is read-only.
        let mut read_only = std::fs::metadata(&there).unwrap().permissions();
        read_only.set_readonly(true);
        std::fs::set_permissions(&there, read_only.clone()).unwrap();
        let e = refused("stl", "stl", &there);
        #[allow(clippy::permissions_set_readonly_false)]
        read_only.set_readonly(false);
        std::fs::set_permissions(&there, read_only).unwrap();
        assert!(e.ends_with("is read-only"), "{e}");
    }

    #[test]
    fn a_file_that_cannot_be_written_does_not_lose_the_run() {
        let scratch = folder();
        let good = scratch.dir.join("good.stl");
        let bad = scratch.dir.join("gone").join("bad.3mf");
        // (A tetrahedron is enough of a surface to write.)
        let positions = vec![tenon_geom::Vec3::ZERO, tenon_geom::Vec3::X, tenon_geom::Vec3::Y, tenon_geom::Vec3::Z];
        let skin = topo::Skin { positions, triangles: vec![[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]], fixed: vec![false; 4] };
        let mut out = json!({ "warnings": ["what was said before"], "iterations": 3 });
        let files = vec![("stl", good.clone()), ("threemf", bad.clone())];
        write_files(&mut out, files, &|file: &Path| write_shape(file, &skin, "Part", "Steel", [1, 2, 3]));
        assert!(good.is_file(), "the other file is still written");
        assert_eq!(out["written"], json!({ "stl": good.display().to_string() }), "{out}");
        let why = out["not_written"]["threemf"].as_str().unwrap();
        assert!(why.starts_with("cannot write") && why.contains("bad.3mf"), "{why}");
        // The summary stands, and the warnings say it.
        assert_eq!(out["iterations"], 3);
        let warnings = out["warnings"].as_array().unwrap();
        assert_eq!(warnings[0], "what was said before");
        assert_eq!(warnings[1].as_str().unwrap(), format!("The shape was worked out, but it was not written: {why}"));
        // With every file written there is nothing under not_written.
        let mut out = json!({ "warnings": [] });
        write_files(&mut out, vec![("stl", good)], &|_: &Path| Ok(()));
        assert!(out.get("not_written").is_none() && out["warnings"] == json!([]), "{out}");
    }

    /// The settings a command asks for, of a study holding what `stored` says.
    fn asked_of(stored: Value, given: Value) -> Result<Optimize, String> {
        let mut s = Session::default();
        s.exec("study.new", &json!({}), None).unwrap();
        s.exec("study.shape", &stored, None).unwrap();
        asked(&s, StudyId(1), &given).map_err(|e| e.0)
    }

    #[test]
    fn what_a_command_gives_takes_the_place_of_what_is_stored_for_its_run() {
        let stored = json!({ "element_size": 3, "min_member": 12, "iterations": 7, "resolution": "medium", "remove": 40 });
        // Nothing given: what is stored.
        let o = asked_of(stored.clone(), json!({})).unwrap();
        assert_eq!((o.element_size, o.min_member, o.iterations, o.resolution), (Some(3.0), Some(12.0), Some(7), Resolution::Medium));
        // A resolution without a size takes the place of the size kept; with one, the size stands.
        let o = asked_of(stored.clone(), json!({ "resolution": "fine" })).unwrap();
        assert_eq!((o.element_size, o.resolution), (None, Resolution::Fine));
        let o = asked_of(stored.clone(), json!({ "resolution": "fine", "element_size": 5 })).unwrap();
        assert_eq!((o.element_size, o.resolution), (Some(5.0), Resolution::Fine));
        let o = asked_of(stored.clone(), json!({ "resolution": "fine", "element_size": null })).unwrap();
        assert_eq!((o.element_size, o.resolution), (None, Resolution::Fine));
        // A null resolution is not given.
        let o = asked_of(stored.clone(), json!({ "resolution": null })).unwrap();
        assert_eq!((o.element_size, o.resolution), (Some(3.0), Resolution::Medium));
        // Null clears for this run.
        let o = asked_of(stored.clone(), json!({ "element_size": null, "min_member": null, "iterations": null })).unwrap();
        assert_eq!((o.element_size, o.min_member, o.iterations), (None, None, None));
        assert_eq!(o.resolution, Resolution::Medium);
        // Numbers in their place; and the same words for rounds as study.shape gives.
        let o = asked_of(stored.clone(), json!({ "element_size": 4, "min_member": 8, "iterations": 9 })).unwrap();
        assert_eq!((o.element_size, o.min_member, o.iterations), (Some(4.0), Some(8.0), Some(9)));
        for bad in [json!(-1), json!(5_000_000_000u64), json!(0), json!(301), json!(1.5)] {
            let e = asked_of(stored.clone(), json!({ "iterations": bad })).unwrap_err();
            assert_eq!(e, "the most rounds must be from 1 to 300", "{bad}");
        }
        assert!(asked_of(stored, json!({ "element_size": -1 })).unwrap_err().contains("element size"));
    }
}

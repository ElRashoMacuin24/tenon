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
/// What is said of a surface that is not closed.
pub const NOT_CLOSED: &str = "The surface of the shape is not closed: a slicer may refuse it, and it is no solid to model from.";
/// What is said of a surface that leaves a load cut off from the supports.
pub const NOT_JOINED: &str = "The material kept does not join every load to a support: keep a larger share, or ask for a smaller smallest member.";
/// How many pairs of smoothing passes the shape is given when none are asked.
pub const SMOOTHING: usize = 10;
/// The most smoothing passes taken.
pub const MAX_SMOOTHING: usize = 50;
/// How many times a resolution's nodes a mesh may come to before its elements are made larger.
const NODES_OVER: f64 = 1.5;
/// How many more times a part is divided, each with larger elements, to come near them.
const MORE_TRIES: usize = 3;
/// How far a mesh's nodes must fall, as a share of the last mesh's, for its larger elements to be
/// worth having. (Where small details hold the nodes up, they do not fall by much, and all that
/// grows is the filter's radius.)
const WORTH_HAVING: f64 = 0.8;
/// How many sizes between are tried when the part is too many nodes at one size and too much for
/// the filter at the next.
const SIZES_BETWEEN: usize = 2;
/// How many times shorter than the elements a closed edge may be. The mesher must go down to its
/// length along all of the wall under it: a bore of 0.01 on a 10 mm plate took it a minute and one
/// of 0.005 nearly ten, and neither could be worked out on after that.
const SMALLEST_CLOSED_EDGE: f64 = 0.01;
/// How the filter's refusal of a table too large is known (`topo::filter`).
const TOO_MUCH_TO_SMOOTH: &str = " would take more than ";

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

/// The part divided into elements for a shape, and what `take` makes of the mesh, with the size
/// that came to: by `divide` at the size it is given, from `h` on.
///
/// A size that came from the resolution is only a guess from the part's volume, and the small
/// holes and rounds of a part take many more nodes than its volume says. So when the mesh comes
/// out well over the resolution's nodes, or past what a shape is worked out on, the elements are
/// made larger by the cube root of how far over it is, and the part divided again, a few times at
/// most and never past three elements across a smallest member asked.
///
/// What is kept is the last mesh a shape can be worked out on: one within the nodes that `take`
/// takes. Larger elements that bring the nodes down by little are not worth having, and the mesh
/// before them stands. So does it when `divide` or `take` cannot do with a larger size (the study
/// does not lie on it, say, or the layers it keeps are too thick, or it is too much for the
/// filter). When one size is too many nodes and the next will not do, a size between them is
/// tried. A size that was asked for is left as it is. At the first size what `take` says is said,
/// with one exception: too much for the filter on a mesh over the nodes, with a smallest member
/// setting the radius. There fewer elements are fewer to smooth, so the size is made larger as it
/// was going to be, and it is said only if nothing fits.
fn divided<T>(
    o: &Optimize,
    h: f64,
    closed_edge: Option<f64>,
    divide: &mut dyn FnMut(f64) -> Result<TetMesh, String>,
    take: &mut dyn FnMut(TetMesh, f64) -> Result<T, String>,
) -> Result<(T, f64), String> {
    use tenon_fea::tools::STOPPED;
    let target = o.resolution.nodes() as f64;
    let most = o.min_member.map_or(f64::INFINITY, |m| m / 3.0);
    let (mut fits, mut fit_nodes): (Option<(T, f64)>, Option<usize>) = (None, None);
    // The largest size that was too many nodes, and the smallest that would not do.
    let (mut too_fine, mut too_much): (Option<f64>, Option<f64>) = (None, None);
    let mut why = None;
    // The last size tried, and of the last mesh made, how large its elements and how many its nodes.
    let (mut now, mut made, mut nodes) = (h, h, 0);
    for tried in 0..=MORE_TRIES {
        let mesh = match divide(now) {
            Ok(mesh) => mesh,
            Err(e) if e == STOPPED => return Err(e),
            Err(e) if tried == 0 => return Err(cannot_divide(o, now, closed_edge, &e)),
            Err(e) => {
                why = Some(cannot_divide(o, now, closed_edge, &e));
                too_much = Some(now);
                break;
            }
        };
        (made, nodes) = (now, mesh.nodes.len());
        if nodes as f64 > NODES_OVER * target && fit_nodes.is_some_and(|n| nodes as f64 > WORTH_HAVING * n as f64) {
            break;
        }
        if nodes > topo::MAX_SHAPE_NODES {
            too_fine = Some(now);
        } else {
            match take(mesh, now) {
                Ok(taken) => (fits, fit_nodes) = (Some((taken, now)), Some(nodes)),
                Err(e) if e == STOPPED => return Err(e),
                Err(e) if fits.is_none() && nodes as f64 > NODES_OVER * target && fewer_would_smooth(o, now, &e) => why = Some(e),
                Err(e) if tried == 0 => return Err(e),
                Err(e) => {
                    why = Some(e);
                    too_much = Some(now);
                    break;
                }
            }
        }
        if o.element_size.is_some() || (nodes as f64 <= NODES_OVER * target && nodes <= topo::MAX_SHAPE_NODES) {
            break;
        }
        let larger = (now * (nodes as f64 / target).cbrt()).min(most);
        if larger <= now {
            break;
        }
        now = larger;
    }
    for _ in 0..SIZES_BETWEEN {
        let (None, Some(fine), Some(much)) = (&fits, too_fine, too_much) else { break };
        now = (fine * much).sqrt();
        match divide(now) {
            Ok(mesh) if mesh.nodes.len() > topo::MAX_SHAPE_NODES => too_fine = Some(now),
            Ok(mesh) => match take(mesh, now) {
                Ok(taken) => fits = Some((taken, now)),
                Err(e) if e == STOPPED => return Err(e),
                Err(e) => (why, too_much) = (Some(e), Some(now)),
            },
            Err(e) if e == STOPPED => return Err(e),
            Err(e) => (why, too_much) = (Some(cannot_divide(o, now, closed_edge, &e)), Some(now)),
        }
    }
    fits.ok_or_else(|| why.unwrap_or_else(|| too_many_nodes(o, made, nodes)))
}

/// What is said of a closed edge (`closed_edge`, its length) too short to divide a part round, with
/// elements of size `h`, before the mesher is asked.
fn closed_edge_too_short(h: f64, closed_edge: Option<f64>) -> Option<String> {
    closed_edge.filter(|c| *c < SMALLEST_CLOSED_EDGE * h).map(|c| {
        format!(
            "The part has a closed edge only {} mm long (a small hole or round), against elements of {} mm: dividing it would take many minutes, and likely come to nothing. Suppress its small holes and rounds to optimize.",
            round(c),
            round(h)
        )
    })
}

/// What the mesher's failing (`e`) at elements of size `h` comes to for what was asked, with what
/// helps when it did try: smaller elements hold a small hole open, unless it is the part's shortest
/// closed edge (`closed_edge`) that the mesher goes by, as no size of element then changes that.
fn cannot_divide(o: &Optimize, h: f64, closed_edge: Option<f64>, e: &str) -> String {
    if !(e.contains("It said") || e.contains("closed up")) {
        return e.to_owned();
    }
    let helps = if tenon_fea::only_the_closed_edge_matters(h, closed_edge) {
        "Suppress its small holes and rounds to optimize."
    } else if o.element_size.is_some() {
        "Make the elements smaller, or suppress its small holes and rounds to optimize."
    } else {
        match o.resolution {
            Resolution::Coarse => "Choose Medium, or suppress its small holes and rounds to optimize.",
            Resolution::Medium => "Choose Fine, or suppress its small holes and rounds to optimize.",
            Resolution::Fine => "Suppress its small holes and rounds to optimize.",
        }
    };
    format!("{}. {helps}", e.trim_end_matches('.'))
}

/// Each kept set's surfaces on the mesh (`on`: which face each surface is), with how deep it is
/// kept. A set with none of the mesh on its faces is said by name, as a support or a load is: left
/// to go on, its faces would not be kept, with no word of it.
fn kept_surfaces(
    kept: &[(Vec<u32>, Option<f64>, String)],
    on: &std::collections::BTreeMap<u32, u32>,
) -> Result<Vec<(Vec<u32>, Option<f64>)>, String> {
    kept.iter()
        .map(|(faces, depth, name)| {
            let surfaces: Vec<u32> = on.iter().filter(|(_, face)| faces.contains(face)).map(|(surface, _)| *surface).collect();
            if surfaces.is_empty() {
                return Err(format!("{name}: none of the mesh lies on its faces"));
            }
            Ok((surfaces, *depth))
        })
        .collect()
}

/// What the filter's refusing a table too large (`e`) comes to for what was asked, of elements of
/// size `h` and so many planes of symmetry. The table is the elements within the radius of each
/// other: a smaller smallest member helps while it sets the radius. When one and a half elements
/// set it, the size of the elements does not say which way it goes: larger ones are fewer but the
/// radius grows with them, and smaller ones can come out the same.
fn too_much_to_smooth(o: &Optimize, h: f64, planes: usize, e: String) -> String {
    let Some((what, _)) = e.split_once(": ask for") else { return e };
    let first = member_sets_the_radius(o, h).then_some("ask for a smaller smallest member");
    let last = (planes > 0).then_some("take fewer planes (each plane through the part doubles the memory)");
    let helps: Vec<&str> = first.into_iter().chain(Some("suppress the part's small holes and rounds")).chain(last).collect();
    format!("{what}: {}", helps.join(", or "))
}

/// Whether a smallest member asked, and not the size of the elements, sets the filter's radius.
fn member_sets_the_radius(o: &Optimize, h: f64) -> bool {
    o.min_member.is_some() && radius_for(o, h) > 1.5 * h
}

/// Whether the filter's refusal `e`, at elements of size `h`, can be got past by fewer elements:
/// for a size that may still be made larger, with the radius staying as it is.
fn fewer_would_smooth(o: &Optimize, h: f64, e: &str) -> bool {
    o.element_size.is_none() && member_sets_the_radius(o, h) && e.contains(TOO_MUCH_TO_SMOOTH)
}

/// The filter's radius for elements of size `h`: half the thinnest member (as asked, or three
/// elements), and at least one and a half elements.
fn radius_for(o: &Optimize, h: f64) -> f64 {
    (o.min_member.unwrap_or(3.0 * h) / 2.0).max(1.5 * h)
}

/// Whether a smallest member asked is what holds elements of size `h` down: three across it.
fn member_holds_the_size(o: &Optimize, h: f64) -> bool {
    o.min_member.is_some_and(|m| h >= (1.0 - 1e-9) * m / 3.0)
}

/// Why a mesh is more than a shape is worked out on, and what helps for what was asked.
fn too_many_nodes(o: &Optimize, h: f64, nodes: usize) -> String {
    let helps = if o.element_size.is_some() {
        "Make the elements larger."
    } else if member_holds_the_size(o, h) {
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

/// What makes the elements smaller, for what was asked and elements of size `h`, if anything does.
fn what_makes_elements_smaller(o: &Optimize, h: f64) -> Option<&'static str> {
    if o.element_size.is_some() {
        Some("make the elements smaller")
    } else if member_holds_the_size(o, h) {
        Some("ask for a smaller smallest member")
    } else {
        match o.resolution {
            Resolution::Coarse => Some("choose Medium"),
            Resolution::Medium => Some("choose Fine"),
            Resolution::Fine => None,
        }
    }
}

/// What is said when what must stay (`what`, with the least the rest can come to `least` of the
/// part: `topo::no_room`) leaves no room for the share kept (`fraction`) of a part of `part_grams`:
/// with as many figures as it takes for the two to read differently, and what would leave room.
/// A share or a mass is offered only if one can be asked that does (a mass just above the one said
/// does), `keep less` only if faces are kept (`any_kept`), and smaller elements (`smaller`: what
/// makes them so) as the layers go with them. When no share or mass will do, it is the faces held
/// and loaded that are too large for the part: all of a thin plate, say.
fn too_much_stays(what: &str, least: f64, fraction: f64, target: Target, part_grams: f64, any_kept: bool, smaller: Option<&str>) -> String {
    let (stays, kept) = (100.0 * least, 100.0 * fraction);
    let figures = (0..6).find(|n| format!("{stays:.n$}", n = *n) != format!("{kept:.n$}", n = *n)).unwrap_or(6);
    // (Up, so that what is said is not itself too little; the less is for the sums' dust.)
    let enough = (least * part_grams * 1000.0 - 1e-6).ceil() / 1000.0;
    let other = match target {
        Target::Remove(_) => (least < 1.0 - tenon_model::study::MIN_REMOVE / 100.0).then(|| "remove less".to_owned()),
        Target::Grams(_) => (enough < part_grams).then(|| format!("ask for more than {enough} g")),
    };
    let (said, first) = match other {
        Some(other) => (format!("more than the {kept:.figures$}% kept."), other),
        None => ("too little is left to take away.".to_owned(), "hold or load smaller faces".to_owned()),
    };
    let smaller = smaller.map(|s| format!("{s}: smaller elements make the layers thinner"));
    let helps: Vec<String> = Some(first).into_iter().chain(any_kept.then(|| "keep less".to_owned())).chain(smaller).collect();
    let helps = match helps.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{}, or {last}", rest.join(", ")),
        _ => helps.concat(),
    };
    let mut letters = helps.chars();
    let helps = letters.next().map_or_else(String::new, |c| c.to_uppercase().chain(letters).collect());
    format!("{what} are already {stays:.figures$}% of the part: {said} {helps}.")
}

/// What is said of a mesh of `nodes` nodes against the `target` of a resolution, at elements of
/// size `h`, with what would bring it down.
fn many_nodes(o: &Optimize, h: f64, nodes: usize, target: usize) -> String {
    let helps = if member_holds_the_size(o, h) { "ask for a larger smallest member" } else { "suppress the part's small holes and rounds" };
    format!("It comes to {nodes} nodes, against about {target} at this resolution, so each round takes that much longer: {helps} to go faster.")
}

/// What is to be said of a surface, for whatever share it was made: not closed (`closed` being
/// whether it is: to work that out takes a look at all of it), pieces left out, a load not joined.
pub fn said_of_surface(closed: bool, shown: &Extracted) -> Vec<String> {
    let mut said = Vec::new();
    if !closed {
        said.push(NOT_CLOSED.to_owned());
    }
    if shown.islands_dropped > 0 {
        let n = shown.islands_dropped;
        said.push(format!(
            "{n} piece{} of material cut off from everything that must stay {} left out.",
            if n == 1 { "" } else { "s" },
            if n == 1 { "was" } else { "were" }
        ));
    }
    if shown.load_not_joined {
        said.push(NOT_JOINED.to_owned());
    }
    said
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
    let (step, drawn, _) = crate::stress::for_the_mesher(k, on.shape, Some(first_size))?;
    let at = |p: &[f32; 3]| tenon_geom::Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
    for (m, plane) in &mirrors {
        if let Some((ahead, behind)) = region::off_centre(drawn.positions.iter().map(at), m) {
            return Err(format!(
                "The part is not the same on both sides of {plane} (it reaches {} mm to one side of it and {} mm to the other): take the plane away, or choose one it is the same on both sides of.",
                round(ahead.max(behind)),
                round(ahead.min(behind))
            ));
        }
    }
    let closed_edge = tenon_fea::faces::shortest_closed_edge(&drawn);
    if let Some(said) = closed_edge_too_short(first_size, closed_edge) {
        return Err(said);
    }

    let tools = Tools::find()?;
    let scratch = match keep {
        Some(dir) => Scratch::in_folder(dir)?,
        None => Scratch::new()?,
    };
    let most = settings.iterations.map_or(if settings.sharpen { ROUNDS_SHARPENED } else { ROUNDS_QUICK }, |n| n as usize);
    report(progress, Phase::Meshing, most, None);
    let own_lines = tenon_fea::faces::lines_of_no_thickness(&drawn);
    let only_mirrors: Vec<Mirror> = mirrors.iter().map(|(m, _)| *m).collect();
    let began = Instant::now();
    let (last_mesh, fit_mesh) = (scratch.dir.join("part.msh"), scratch.dir.join("part.fits.msh"));
    let part_grams = part_volume * density / 1000.0;
    let (taken, h) = divided(
        settings,
        first_size,
        closed_edge,
        &mut |size| tenon_fea::mesh_for_shape(&tools, &step, size, closed_edge, &own_lines, &scratch.dir, cancel),
        // The study put on the mesh, what must stay solid on it (what is held or loaded, and the
        // kept faces as deep as asked), that it leaves room for the share kept (the layers are as
        // thick as the elements), and whether the filter can take it.
        &mut |mesh, size| {
            let prepared = tenon_fea::put_on(mesh, &drawn, &on.setup, 0.0)?;
            let kept_sets = kept_surfaces(&kept_faces, &prepared.on)?;
            let passive = region::passive(&prepared.problem, &kept_sets);
            let design = region::design(&prepared.problem.mesh, passive.clone());
            let total: f64 = design.volumes.iter().sum();
            let solid: f64 = design.volumes.iter().zip(&passive).filter(|(_, p)| **p).map(|(v, _)| *v).sum();
            if topo::no_room(solid, total, fraction * total) {
                let names: Vec<&str> = kept_faces.iter().map(|(_, _, n)| n.as_str()).collect();
                let what = if names.is_empty() {
                    "The layers under the supports and loads".to_owned()
                } else {
                    format!("{} and the layers under the supports and loads", names.join(", "))
                };
                let least = (solid + topo::X_MIN * (total - solid)) / total;
                let smaller = what_makes_elements_smaller(settings, size);
                return Err(too_much_stays(&what, least, fraction, settings.target, part_grams, !names.is_empty(), smaller));
            }
            topo::filter::Filter::new(&design.centroids, &design.volumes, radius_for(settings, size), &only_mirrors)
                .map_err(|e| too_much_to_smooth(settings, size, only_mirrors.len(), e))?;
            // (The folder asked to keep the files gets the mesh that was used, not the last tried.)
            if keep.is_some() {
                let _ = std::fs::copy(&last_mesh, &fit_mesh);
            }
            Ok((prepared.problem, passive, design))
        },
    )?;
    let _ = std::fs::rename(&fit_mesh, &last_mesh);
    let meshing = began.elapsed().as_secs_f64();
    let (problem, passive, design) = taken;
    let mesh: &TetMesh = &problem.mesh;
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

    let radius = radius_for(settings, h);
    let goal = topo::Goal {
        fraction,
        radius,
        sharpen: settings.sharpen,
        max_iterations: most,
        budget: BUDGET.saturating_sub(started.elapsed()),
        mirrors: only_mirrors,
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
            "The part's thin walls or small details take many elements, so the elements were made larger ({} mm) to stay near this resolution's nodes.",
            round(h)
        ));
    }
    let (nodes, target) = (mesh.nodes.len(), settings.resolution.nodes());
    if settings.element_size.is_none() && nodes as f64 > NODES_OVER * target as f64 {
        warnings.push(many_nodes(settings, h, nodes, target));
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
    warnings.extend(said_of_surface(shown.skin.is_closed(), &shown));
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
    // (As `study.shape` reads them; `check`, at the end of this, says how large they may be.)
    let num = |key: &str| Optimize::number_of(p, key).map_err(CmdError);
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
        o.resolution = Resolution::of(r).map_err(CmdError)?;
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

/// Whether a file can be made in `folder`: one of a name of its own is tried, and removed at once.
/// (Made only if no such file is there, so that what is removed is only ever that one.)
fn folder_takes_a_file(folder: &Path) -> std::io::Result<()> {
    let mut last = std::io::Error::from(std::io::ErrorKind::AlreadyExists);
    for tried in 0..100 {
        let probe = folder.join(format!(".tenon-try-{}-{tried}.tmp", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
            Ok(made) => {
                drop(made);
                let _ = std::fs::remove_file(&probe);
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = e,
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

/// Whether a shape can be written to `file` for the parameter `key`, which takes the ending `ext`:
/// said before a run that may take an hour, not after it. A file that is there is opened to add to
/// and not to write over, so that it is not touched. No file of the name asked for is ever made
/// here, as one made would have to be removed, and what is under that name by then (a file saved
/// meanwhile, a link) need not be this one's: when nothing is there its folder is tried, and the
/// name goes by the words below.
fn can_write(key: &str, ext: &str, file: &Path) -> Result<(), CmdError> {
    if !file.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
        return Err(format!("`{key}` must be a file ending in .{ext}, not {}", file.display()).into());
    }
    if file.is_dir() {
        return Err(format!("`{key}`: {} is a folder", file.display()).into());
    }
    // (On Windows `a:b.stl` opens: as a stream hidden in a file `a`, which is not the file named.)
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if cfg!(windows) && name.chars().any(|c| "<>:\"|?*".contains(c) || c.is_control()) {
        return Err(format!("`{key}`: a file cannot be called {name}: the name cannot have any of < > : \" | ? * in it").into());
    }
    if (if cfg!(windows) { name.encode_utf16().count() } else { name.len() }) > 255 {
        return Err(format!("`{key}`: a file cannot be called {name}: the name is too long").into());
    }
    let folder = file.parent().filter(|f| !f.as_os_str().is_empty()).unwrap_or(Path::new("."));
    if !folder.is_dir() {
        return Err(format!("`{key}`: there is no folder {}", folder.display()).into());
    }
    if std::fs::metadata(file).is_ok_and(|m| m.permissions().readonly()) {
        return Err(format!("`{key}`: {} is read-only", file.display()).into());
    }
    let tried = match std::fs::OpenOptions::new().append(true).open(file) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => folder_takes_a_file(folder),
        Err(e) => Err(e),
    };
    tried.map_err(|e| format!("`{key}`: {} cannot be written: {e}", file.display()).into())
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
    let shaped = Arc::new(work_out(s, k, id, &settings, smooth, keep.as_deref(), &crate::stress::command_cancel(), None).map_err(CmdError)?);
    let mut out = shaped.summary(&name);
    let skin = match shown {
        Some(share) => {
            let skin = shaped.reshaped(share, smooth).skin;
            // (Said of the share it was worked out for only, if not of this one.)
            if !skin.is_closed()
                && shaped.shown.skin.is_closed()
                && let Some(warnings) = out["warnings"].as_array_mut()
            {
                warnings.push(json!(NOT_CLOSED));
            }
            skin
        }
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

    /// What `divided` comes to when the mesher gives `counts` nodes in turn, what it makes of each
    /// is `take`'s, and the sizes it asked for.
    fn dividing_with(
        o: &Optimize,
        h: f64,
        counts: &[usize],
        take: &mut dyn FnMut(TetMesh, f64) -> Result<TetMesh, String>,
    ) -> (Result<(usize, f64), String>, Vec<f64>) {
        let mut asked = Vec::new();
        let done = divided(
            o,
            h,
            None,
            &mut |size| {
                let n = counts[asked.len()];
                asked.push(size);
                Ok(of(n))
            },
            take,
        );
        (done.map(|(mesh, size)| (mesh.nodes.len(), size)), asked)
    }

    /// The same, with every mesh within the nodes taken.
    fn dividing(o: &Optimize, h: f64, counts: &[usize]) -> (Result<(usize, f64), String>, Vec<f64>) {
        dividing_with(o, h, counts, &mut |mesh, _| Ok(mesh))
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
        // At most three more tries, each worth having: the last is kept, over or not.
        let (done, asked) = dividing(&o, 3.0, &[96_000, 76_000, 60_000, 48_000]);
        assert_eq!(done.as_ref().map(|(n, _)| *n), Ok(48_000));
        assert_eq!(asked.len(), 4);
        // Past what a shape is worked out on, with no more tries, the last that was within it.
        let (done, asked) = dividing(&o, 3.0, &[96_000, 76_000, 150_000]);
        assert_eq!(done.as_ref().map(|(n, _)| *n), Ok(76_000));
        assert_eq!(asked.len(), 3);
        // Past it at Fine, though not one and a half times over: larger, not refused.
        let (done, asked) = dividing(&at(Resolution::Fine), 2.0, &[108_000, 79_000]);
        assert_eq!(done.as_ref().map(|(n, _)| *n), Ok(79_000));
        assert!(asked[1] > 2.0, "{asked:?}");
    }

    #[test]
    fn larger_elements_that_bring_the_nodes_down_by_little_are_not_kept() {
        let o = at(Resolution::Coarse);
        // Twice the size for nearly the nodes: small details hold them up. The first stands.
        let (done, asked) = dividing(&o, 3.0, &[96_000, 90_000]);
        assert_eq!(done, Ok((96_000, 3.0)));
        assert_eq!(asked, vec![3.0, 6.0]);
        // Falling little is no matter when that comes to what was asked.
        let (done, asked) = dividing(&o, 3.0, &[20_000, 17_500]);
        assert_eq!(done.as_ref().map(|(n, _)| *n), Ok(17_500));
        assert_eq!(asked.len(), 2);
        // The same at the third.
        let (done, asked) = dividing(&o, 3.0, &[96_000, 60_000, 58_000]);
        assert_eq!(done.as_ref().map(|(n, _)| *n), Ok(60_000));
        assert_eq!(asked.len(), 3);
        // But while nothing fits it goes on, however little they fall.
        let (done, asked) = dividing(&o, 3.0, &[140_000, 130_000, 120_000, 99_000]);
        assert_eq!(done.as_ref().map(|(n, _)| *n), Ok(99_000));
        assert_eq!(asked.len(), 4);
    }

    #[test]
    fn a_larger_size_that_will_not_do_leaves_the_mesh_before_it_or_one_between() {
        let o = at(Resolution::Coarse);
        // What `take` refuses at a larger size: the one before stands.
        let (done, asked) =
            dividing_with(&o, 3.0, &[60_000, 30_000], &mut |mesh, size| if size > 3.0 { Err("will not do".into()) } else { Ok(mesh) });
        assert_eq!(done, Ok((60_000, 3.0)));
        assert_eq!(asked.len(), 2);
        // What it refuses at the first size is said, whatever it is.
        let (done, asked) = dividing_with(&o, 3.0, &[60_000], &mut |_, _| Err("Fixed1: none of the mesh lies on its faces".into()));
        assert_eq!(done.unwrap_err(), "Fixed1: none of the mesh lies on its faces");
        assert_eq!(asked, vec![3.0]);
        // Too many nodes at one size and refused at the next: a size between.
        let (done, asked) = dividing_with(&o, 3.0, &[150_000, 90_000, 95_000], &mut |mesh, size| {
            if size > 5.0 { Err("too much for the filter".into()) } else { Ok(mesh) }
        });
        assert_eq!(done.as_ref().map(|(n, _)| *n), Ok(95_000), "{asked:?}");
        assert!(asked[2] > asked[0] && asked[2] < asked[1], "{asked:?}");
        // No size between will do: what was said of the last that was tried.
        let (done, asked) = dividing_with(&o, 3.0, &[150_000, 90_000, 90_000, 90_000], &mut |_, _| Err("too much for the filter".into()));
        assert_eq!(done.unwrap_err(), "too much for the filter");
        assert_eq!(asked.len(), 4);
        // Too many nodes in the middle too: the next between goes higher.
        let (done, asked) =
            dividing_with(&o, 3.0, &[150_000, 90_000, 150_000, 90_000], &mut |mesh, size| if size > 6.0 { Err("no".into()) } else { Ok(mesh) });
        assert!(asked[3] > asked[2], "{asked:?}");
        assert_eq!(done.as_ref().map(|(n, _)| *n), Ok(90_000), "{asked:?}");
        // Being stopped while taking is being stopped.
        let (done, _) =
            dividing_with(&o, 3.0, &[60_000, 30_000], &mut |mesh, size| if size > 3.0 { Err(tenon_fea::tools::STOPPED.into()) } else { Ok(mesh) });
        assert_eq!(done.unwrap_err(), tenon_fea::tools::STOPPED);
    }

    #[test]
    fn a_mesher_that_cannot_divide_it_is_said_with_what_helps() {
        let failed = "The part could not be divided into elements. gmsh failed (exit code: 1). It said: Invalid boundary mesh on surface 7";
        let refused = |o: &Optimize| divided(o, 3.0, None, &mut |_| Err(failed.to_owned()), &mut |mesh: TetMesh, _| Ok(mesh)).unwrap_err();
        assert_eq!(refused(&at(Resolution::Coarse)), format!("{failed}. Choose Medium, or suppress its small holes and rounds to optimize."));
        assert_eq!(refused(&at(Resolution::Medium)), format!("{failed}. Choose Fine, or suppress its small holes and rounds to optimize."));
        assert_eq!(refused(&at(Resolution::Fine)), format!("{failed}. Suppress its small holes and rounds to optimize."));
        let sized = Optimize { element_size: Some(3.0), ..Optimize::default() };
        assert_eq!(refused(&sized), format!("{failed}. Make the elements smaller, or suppress its small holes and rounds to optimize."));
        // Said of a hole closed up too, and not of a mesher that could not be started.
        let closed = "The part could not be divided into elements of 3 mm: a small hole or round closed up at that size.";
        assert_eq!(
            cannot_divide(&at(Resolution::Coarse), 3.0, None, closed),
            format!("{}. Choose Medium, or suppress its small holes and rounds to optimize.", closed.trim_end_matches('.'))
        );
        let not_started = "The part could not be divided into elements. cannot start gmsh";
        assert_eq!(cannot_divide(&at(Resolution::Coarse), 3.0, None, not_started), not_started);
        // Where the mesher goes by a closed edge, not by the size, no size helps: not said.
        for o in [at(Resolution::Coarse), at(Resolution::Medium), sized.clone()] {
            let e = cannot_divide(&o, 3.0, Some(0.3), closed);
            assert_eq!(e, format!("{}. Suppress its small holes and rounds to optimize.", closed.trim_end_matches('.')));
        }
        // A closed edge not far under the size: the size still has a say.
        assert!(cannot_divide(&at(Resolution::Coarse), 3.0, Some(2.0), closed).contains("Choose Medium"));
    }

    #[test]
    fn too_much_to_smooth_says_what_helps_for_what_was_asked() {
        let said = "smoothing the densities of 100 elements over 30 mm would take more than 512 MB: ask for a smaller smallest member, or suppress the part's small holes and rounds";
        let what = "smoothing the densities of 100 elements over 30 mm would take more than 512 MB: ";
        let holes = "suppress the part's small holes and rounds";
        // A smallest member that sets the radius.
        let o = Optimize { min_member: Some(60.0), ..Optimize::default() };
        assert_eq!(too_much_to_smooth(&o, 5.0, 0, said.into()), format!("{what}ask for a smaller smallest member, or {holes}"));
        // Elements asked for, with one and a half of them setting it: a smaller member would not
        // help, and smaller elements can come out the same.
        let o = Optimize { element_size: Some(12.0), ..Optimize::default() };
        assert_eq!(too_much_to_smooth(&o, 12.0, 0, said.into()), format!("{what}{holes}"));
        let o = Optimize { element_size: Some(12.0), min_member: Some(3.0), ..Optimize::default() };
        assert_eq!(too_much_to_smooth(&o, 12.0, 0, said.into()), format!("{what}{holes}"));
        // Sized from the resolution: the size has been made as it can be.
        assert_eq!(too_much_to_smooth(&Optimize::default(), 20.0, 0, said.into()), format!("{what}{holes}"));
        // With planes, those too; and what is not of this kind is left as it is.
        assert_eq!(
            too_much_to_smooth(&Optimize::default(), 20.0, 2, said.into()),
            format!("{what}{holes}, or take fewer planes (each plane through the part doubles the memory)")
        );
        assert_eq!(too_much_to_smooth(&Optimize::default(), 20.0, 0, "something else".into()), "something else");
    }

    #[test]
    fn a_closed_edge_far_shorter_than_the_elements_is_said_before_the_mesher_is_asked() {
        assert_eq!(closed_edge_too_short(3.0, None), None);
        assert_eq!(closed_edge_too_short(3.0, Some(0.031)), None);
        let said = closed_edge_too_short(3.0, Some(0.0157)).unwrap();
        assert!(said.starts_with("The part has a closed edge only 0.016 mm long (a small hole or round), against elements of 3 mm"), "{said}");
        assert!(said.ends_with("Suppress its small holes and rounds to optimize."), "{said}");
    }

    #[test]
    fn a_kept_set_with_none_of_the_mesh_on_its_faces_is_said_by_name() {
        let on: std::collections::BTreeMap<u32, u32> = [(1, 10), (2, 11), (3, 12), (4, 11)].into_iter().collect();
        let set = |faces: &[u32], depth: Option<f64>, name: &str| (faces.to_vec(), depth, name.to_owned());
        // Each set's surfaces, as many as lie on its faces.
        assert_eq!(
            kept_surfaces(&[set(&[11], Some(3.0), "Keep1"), set(&[10, 12, 99], None, "Keep2")], &on).unwrap(),
            [(vec![2, 4], Some(3.0)), (vec![1, 3], None)]
        );
        assert_eq!(kept_surfaces(&[], &on).unwrap(), []);
        // None of them: not passed over.
        assert_eq!(
            kept_surfaces(&[set(&[10], None, "Keep1"), set(&[99], None, "Keep2")], &on).unwrap_err(),
            "Keep2: none of the mesh lies on its faces"
        );
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
        // It is said of the last mesh made, at the size that mesh was made at.
        let (done, asked) = dividing(&at(Resolution::Coarse), 1.0, &[900_000, 500_000, 300_000, 200_000]);
        assert_eq!(asked.len(), 4);
        let e = done.unwrap_err();
        assert!(e.starts_with(&format!("Divided into elements of {} mm the part has 200000 nodes", round(asked[3]))), "{e} {asked:?}");
    }

    #[test]
    fn too_much_to_smooth_at_the_first_size_is_not_final_when_fewer_elements_would_do() {
        let said = "smoothing the densities of 10788 elements over 50 mm would take more than 512 MB: ask for a smaller smallest member";
        let refuses_below = |least: f64| move |mesh: TetMesh, size: f64| if size < least { Err(said.to_owned()) } else { Ok(mesh) };
        // A smallest member sets the radius, and the mesh is over its resolution's nodes: as it
        // was going to be made larger anyway, that is tried, and it fits.
        let o = Optimize { min_member: Some(100.0), ..Optimize::default() };
        let (done, asked) = dividing_with(&o, 3.0, &[20_473, 13_323], &mut refuses_below(3.5));
        assert_eq!(done.as_ref().map(|(n, _)| *n), Ok(13_323), "{asked:?}");
        assert_eq!(asked.len(), 2);
        // Nothing fits: what the filter said of the last.
        let (done, asked) = dividing_with(&o, 3.0, &[20_473, 11_000], &mut refuses_below(1e9));
        assert_eq!(done.unwrap_err(), said);
        assert_eq!(asked.len(), 2, "{asked:?}");
        // Within its resolution's nodes: not to be made larger, so it is said at once.
        let (done, asked) = dividing_with(&o, 3.0, &[11_000], &mut refuses_below(1e9));
        assert_eq!((done.unwrap_err().as_str(), asked.len()), (said, 1));
        // One and a half elements set the radius: it grows with the elements. Said at once.
        let (done, asked) = dividing_with(&Optimize::default(), 3.0, &[20_473], &mut refuses_below(1e9));
        assert_eq!((done.unwrap_err().as_str(), asked.len()), (said, 1));
        // Likewise a size asked for, which is not made larger.
        let sized = Optimize { element_size: Some(3.0), min_member: Some(100.0), ..Optimize::default() };
        let (done, asked) = dividing_with(&sized, 3.0, &[20_473], &mut refuses_below(1e9));
        assert_eq!((done.unwrap_err().as_str(), asked.len()), (said, 1));
        // What else `take` says at the first size is said at once, over the nodes or not.
        let (done, asked) = dividing_with(&o, 3.0, &[20_473], &mut |_, _| Err("Fixed1: none of the mesh lies on its faces".to_owned()));
        assert_eq!((done.unwrap_err().as_str(), asked.len()), ("Fixed1: none of the mesh lies on its faces", 1));
    }

    #[test]
    fn many_nodes_says_what_holds_them_up() {
        let tail = "to go faster.";
        let by_details = many_nodes(&Optimize::default(), 3.0, 29_986, 12_000);
        assert_eq!(
            by_details,
            format!(
                "It comes to 29986 nodes, against about 12000 at this resolution, so each round takes that much longer: suppress the part's small holes and rounds {tail}"
            )
        );
        // A smallest member that holds the size down: a block with no small details at all.
        let o = Optimize { min_member: Some(5.0), ..Optimize::default() };
        assert!(many_nodes(&o, 5.0 / 3.0, 29_986, 12_000).ends_with(&format!("ask for a larger smallest member {tail}")));
        // A smallest member with room to spare: the details again.
        assert!(many_nodes(&o, 1.0, 29_986, 12_000).ends_with(&format!("suppress the part's small holes and rounds {tail}")));
    }

    #[test]
    fn a_mesher_that_fails_on_a_larger_size_leaves_what_it_did_before() {
        let o = at(Resolution::Coarse);
        let mut calls = 0;
        let mut failing = |why: &'static str| {
            calls = 0;
            divided(
                &o,
                3.0,
                None,
                &mut |_| {
                    calls += 1;
                    if calls == 1 { Ok(of(50_000)) } else { Err(why.to_owned()) }
                },
                &mut |mesh, _| Ok(mesh),
            )
            .map(|(mesh, size)| (mesh.nodes.len(), size))
        };
        assert_eq!(failing("the mesher fell over"), Ok((50_000, 3.0)));
        // Being stopped is being stopped, whatever there is so far.
        assert_eq!(failing(tenon_fea::tools::STOPPED), Err(tenon_fea::tools::STOPPED.to_owned()));
        // With nothing it can be worked out on, the failure is what is said.
        let mut calls = 0;
        let e = divided(
            &o,
            3.0,
            None,
            &mut |_| {
                calls += 1;
                if calls == 1 { Ok(of(500_000)) } else { Err("the mesher fell over".to_owned()) }
            },
            &mut |mesh, _| Ok(mesh),
        )
        .unwrap_err();
        assert_eq!(e, "the mesher fell over");
        // And one that fails at once, as it is.
        assert_eq!(divided(&o, 3.0, None, &mut |_| Err("no mesher".to_owned()), &mut |mesh, _| Ok(mesh)).unwrap_err(), "no mesher");
    }

    #[test]
    fn too_much_kept_is_said_so_that_it_can_be_read() {
        let what = "The layers under the supports and loads";
        let said = |least: f64, fraction: f64, target: Target, grams: f64| too_much_stays(what, least, fraction, target, grams, true, None);
        // Two whole figures that differ are enough.
        assert_eq!(
            said(0.7, 0.57, Target::Remove(43.0), 141.3),
            "The layers under the supports and loads are already 70% of the part: more than the 57% kept. Remove less, or keep less."
        );
        // Two that would read the same are given to a tenth.
        assert_eq!(
            said(0.503, 0.5, Target::Remove(50.0), 141.3),
            "The layers under the supports and loads are already 50.3% of the part: more than the 50.0% kept. Remove less, or keep less."
        );
        // A mass asked for: the mass that would leave room. (70% of 141.3 g.)
        assert_eq!(
            said(0.7, 0.57, Target::Grams(80.0), 141.3),
            "The layers under the supports and loads are already 70% of the part: more than the 57% kept. Ask for more than 98.91 g, or keep less."
        );
        // Asked just above what was said, the two shares read differently, however close.
        assert_eq!(
            said(0.699555, 0.699545, Target::Grams(98.84), 141.3),
            "The layers under the supports and loads are already 69.96% of the part: more than the 69.95% kept. Ask for more than 98.848 g, or keep less."
        );
        // Up, not to the nearest: what is said is enough.
        assert!(said(0.123_456, 0.1, Target::Grams(1.0), 100.0).ends_with("Ask for more than 12.346 g, or keep less."));
        // The same to six figures: as they are.
        assert!(said(0.5, 0.5, Target::Remove(50.0), 100.0).contains("already 50.000000% of the part: more than the 50.000000% kept."));
    }

    #[test]
    fn too_much_kept_offers_only_what_can_be_asked_and_would_help() {
        let what = "The layers under the supports and loads";
        let medium = Some("choose Medium");
        // No faces kept: nothing to keep less of. Smaller elements make the layers thinner.
        assert_eq!(
            too_much_stays(what, 0.12, 0.1, Target::Remove(90.0), 776.0, false, medium),
            "The layers under the supports and loads are already 12% of the part: more than the 10% kept. Remove less, or choose Medium: smaller elements make the layers thinner."
        );
        assert_eq!(
            too_much_stays(what, 0.12, 0.1, Target::Remove(90.0), 776.0, true, medium),
            "The layers under the supports and loads are already 12% of the part: more than the 10% kept. Remove less, keep less, or choose Medium: smaller elements make the layers thinner."
        );
        assert_eq!(
            too_much_stays(what, 0.12, 0.1, Target::Remove(90.0), 776.0, false, None),
            "The layers under the supports and loads are already 12% of the part: more than the 10% kept. Remove less."
        );
        // All of a thin plate stays: 5% is the least that can be removed, so "remove less" is no use.
        let whole =
            "The layers under the supports and loads are already 100% of the part: too little is left to take away. Hold or load smaller faces";
        assert_eq!(
            too_much_stays(what, 1.0, 0.95, Target::Remove(5.0), 113.04, false, medium),
            format!("{whole}, or choose Medium: smaller elements make the layers thinner.")
        );
        assert_eq!(too_much_stays(what, 1.0, 0.5, Target::Remove(50.0), 113.04, false, None), format!("{whole}."));
        assert_eq!(too_much_stays(what, 1.0, 0.5, Target::Remove(50.0), 113.04, true, None), format!("{whole}, or keep less."));
        // Likewise a mass that would be the part's own, which is refused in its turn.
        assert_eq!(too_much_stays(what, 1.0, 0.44, Target::Grams(50.0), 113.04, false, None), format!("{whole}."));
        // Just under the most that can be kept: asking for that does leave room.
        assert!(too_much_stays(what, 0.949, 0.5, Target::Remove(50.0), 100.0, false, None).ends_with("Remove less."));
        assert!(too_much_stays(what, 0.951, 0.5, Target::Remove(50.0), 100.0, false, None).contains("too little is left"));
        // By mass, there is no such limit short of the part's own.
        assert!(too_much_stays(what, 0.97, 0.5, Target::Grams(50.0), 100.0, false, None).ends_with("Ask for more than 97 g."));
    }

    #[test]
    fn what_makes_the_elements_smaller_goes_by_what_was_asked() {
        assert_eq!(what_makes_elements_smaller(&at(Resolution::Coarse), 3.0), Some("choose Medium"));
        assert_eq!(what_makes_elements_smaller(&at(Resolution::Medium), 3.0), Some("choose Fine"));
        assert_eq!(what_makes_elements_smaller(&at(Resolution::Fine), 3.0), None);
        let sized = Optimize { element_size: Some(3.0), ..at(Resolution::Fine) };
        assert_eq!(what_makes_elements_smaller(&sized, 3.0), Some("make the elements smaller"));
        // A smallest member that holds the size down is what the resolution's size cannot get past.
        let member = Optimize { min_member: Some(9.0), ..at(Resolution::Coarse) };
        assert_eq!(what_makes_elements_smaller(&member, 3.0), Some("ask for a smaller smallest member"));
        assert_eq!(what_makes_elements_smaller(&member, 2.0), Some("choose Medium"));
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
        // A name longer than any the system takes, which the folder would have let through.
        let e = can_write("stl", "stl", &dir.join(format!("{}.stl", "x".repeat(252)))).unwrap_err().0;
        assert!(e.contains("a file cannot be called") && e.ends_with("the name is too long"), "{e}");
        assert!(can_write("stl", "stl", &dir.join(format!("{}.stl", "x".repeat(251)))).is_ok());
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
        // A name the system does not take is said, and nothing is left: on Windows `a:b.stl` opens
        // as a stream in a file `a`, and `a*b.stl` fails only when it is written.
        if cfg!(windows) {
            for bad in ["bad:name.stl", "bad*name.stl", "bad?name.stl"] {
                let e = refused("stl", "stl", &dir.join(bad));
                assert!(e.contains("a file cannot be called"), "{e}");
            }
            assert!(!dir.join("bad").exists());
        }
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), 2);
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
    fn trying_where_a_shape_goes_makes_no_file_of_that_name() {
        use std::sync::atomic::{AtomicBool, Ordering};
        // Something saving a file under that name while it is tried must not lose it: it would, if
        // the name itself were made and removed. So the name is never there, however closely it is
        // watched.
        let scratch = folder();
        let target = scratch.dir.join("being-saved.stl");
        let (seen, done) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
        let watcher = {
            let (target, seen, done) = (target.clone(), seen.clone(), done.clone());
            std::thread::spawn(move || {
                while !done.load(Ordering::Relaxed) {
                    if target.exists() {
                        seen.store(true, Ordering::Relaxed);
                    }
                }
            })
        };
        for _ in 0..500 {
            can_write("stl", "stl", &target).unwrap();
        }
        done.store(true, Ordering::Relaxed);
        watcher.join().unwrap();
        assert!(!seen.load(Ordering::Relaxed), "the name was made");
        assert_eq!(std::fs::read_dir(&scratch.dir).unwrap().count(), 0, "and what was tried in its place is gone");
        // A file there already is opened, and that is all.
        std::fs::write(&target, b"there").unwrap();
        can_write("stl", "stl", &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"there");
        // A name of its own that is taken is passed over, and left as it is.
        let taken = scratch.dir.join(format!(".tenon-try-{}-0.tmp", std::process::id()));
        std::fs::write(&taken, b"not this one's").unwrap();
        folder_takes_a_file(&scratch.dir).unwrap();
        assert_eq!(std::fs::read(&taken).unwrap(), b"not this one's");
        assert_eq!(std::fs::read_dir(&scratch.dir).unwrap().count(), 2);
        // A link that points nowhere is left as it is, and nothing is made where it points. (Where
        // the system lets a link be made at all: Windows may need a right for that.)
        let (link, nowhere) = (scratch.dir.join("link.stl"), scratch.dir.join("nowhere.stl"));
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&nowhere, &link);
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&nowhere, &link);
        if linked.is_ok() {
            can_write("stl", "stl", &link).unwrap();
            assert!(std::fs::symlink_metadata(&link).is_ok(), "the link is gone");
            assert!(!nowhere.exists(), "a file was made through the link");
        }
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
        assert!(asked_of(stored.clone(), json!({ "element_size": -1 })).unwrap_err().contains("element size"));
        // A resolution in any case, as the schema takes it.
        for word in ["Fine", "FINE", "fine"] {
            assert_eq!(asked_of(stored.clone(), json!({ "resolution": word })).unwrap().resolution, Resolution::Fine);
        }
        assert_eq!(asked_of(stored.clone(), json!({ "resolution": "huge" })).unwrap_err(), "`resolution` is coarse, medium or fine");
        // Past what any size is, or a share can be: in words with the range, as study.shape has.
        let e = asked_of(json!({ "remove": 40 }), json!({ "element_size": 1e300 })).unwrap_err();
        assert_eq!(e, "the element size must be more than 0 and at most 1000000 mm");
        let e = asked_of(json!({ "remove": 40 }), json!({ "min_member": 1e300 })).unwrap_err();
        assert_eq!(e, "the thinnest member must be more than 0 and at most 1000000 mm");
        let e = asked_of(stored, json!({ "element_size": 2_000_000 })).unwrap_err();
        assert_eq!(e, "the element size must be more than 0 and at most 1000000 mm");
        let e = asked_of(json!({ "remove": 40 }), json!({ "remove": 2_000_000 })).unwrap_err();
        assert_eq!(e, "the share taken away must be from 5 to 95 %");
        // A mass has no limit but the part's own, which the run says: a heavy part can be asked one.
        let o = asked_of(json!({ "remove": 40 }), json!({ "grams": 1_500_000 })).unwrap();
        assert_eq!(o.target, Target::Grams(1_500_000.0));
        assert_eq!(asked_of(json!({ "remove": 40 }), json!({ "grams": "much" })).unwrap_err(), "`grams` must be a number");
    }
}

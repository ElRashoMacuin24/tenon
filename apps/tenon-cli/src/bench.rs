//! Benchmarks on the example models: what `tenon-cli bench` prints, and what CI holds to
//! budgets (`cargo xtask ci` runs it with `--check`).
//!
//! Every row is one thing a user waits for: a part rebuilt from nothing, rebuilt after an
//! edit, meshed for the screen; a flat pattern; an assembly opened; what is drawn of a large
//! assembly and how long it takes to find what is under the pointer; a drawing's views worked
//! out. Each has a budget, set loosely enough that a slow shared runner passes and tightly
//! enough that a change which makes something several times slower does not. The numbers and
//! what they mean are in `docs/performance.md`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};
use tenon_kernel::{Kernel, MeshTol};
use tenon_model::{Document, FeatureId, RegenCache, regenerate, regenerate_with, scene};

use crate::{Engine, Report};

/// Makes a kernel of its own for a measurement.
pub type Make<'a> = &'a dyn Fn() -> Box<dyn Kernel>;

/// One measurement.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// What kind of thing was measured: `part`, `flat`, `assembly`, `view`, `drawing`, `memory`.
    pub group: &'static str,
    /// The model it was measured on.
    pub model: String,
    pub what: &'static str,
    pub value: f64,
    /// `ms`, `MB`, or `k` (thousands, of triangles).
    pub unit: &'static str,
    /// The most it may be before CI fails.
    pub budget: f64,
}

impl Row {
    pub fn over(&self) -> bool {
        self.value > self.budget
    }
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
}

/// How often something is timed: five times when it is quick, three when it is not.
fn repeats(first_ms: f64) -> usize {
    if first_ms > 250.0 { 3 } else { 5 }
}

/// The budgets of a part's rows, in milliseconds: rebuilt from nothing, meshed, rebuilt after
/// an edit of its last feature, and of a feature in the middle of its history.
#[derive(Clone, Copy, Debug)]
pub struct PartBudget {
    pub rebuild: f64,
    pub mesh: f64,
    pub edit_last: f64,
    pub edit_middle: f64,
}

/// The rows of one part: `doc` rebuilt from nothing on a new kernel, meshed and measured for
/// the screen, and rebuilt after edits that change nothing but must be followed through (a
/// feature renamed), as the app rebuilds it: with what it kept from the time before.
pub fn part(rows: &mut Vec<Row>, make: Make, model: &str, doc: &Document, budget: PartBudget) -> Result<(), String> {
    let mut row =
        |what: &'static str, value: f64, budget: f64| rows.push(Row { group: "part", model: model.to_owned(), what, value, unit: "ms", budget });
    let (mut rebuilt, mut meshed) = (Vec::new(), Vec::new());
    let mut times = 5;
    let mut i = 0;
    while i < times {
        let mut k = make();
        let t = Instant::now();
        let mut regen = regenerate(doc, k.as_mut());
        rebuilt.push(ms(t));
        if let Some((_, why)) = regen.first_error() {
            return Err(format!("{model} does not rebuild: {why}"));
        }
        let t = Instant::now();
        scene(&regen, k.as_mut(), &MeshTol::default())?;
        meshed.push(ms(t));
        regen.release(k.as_mut());
        if i == 0 {
            times = repeats(rebuilt[0]);
        }
        i += 1;
    }
    row("rebuild from nothing", median(rebuilt), budget.rebuild);
    row("mesh and measure for the screen", median(meshed), budget.mesh);

    // Edits, on one kernel, with what regeneration keeps between runs.
    let mut k = make();
    let mut cache = RegenCache::default();
    let mut doc = doc.clone();
    let ids: Vec<FeatureId> = doc.features().iter().map(|f| f.id).collect();
    let (Some(&last), Some(&middle)) = (ids.last(), ids.get(ids.len() / 2)) else { return Ok(()) };
    let mut renamed = 0;
    let mut touch = |doc: &mut Document, id: FeatureId| {
        renamed += 1;
        if let Some(f) = doc.feature_mut(id) {
            f.name = format!("Renamed{renamed}");
        }
    };
    let rebuild = |doc: &Document, cache: &mut RegenCache, k: &mut dyn Kernel| -> f64 {
        let t = Instant::now();
        let mut regen = regenerate_with(doc, k, Some(cache));
        let took = ms(t);
        regen.release(k);
        took
    };
    rebuild(&doc, &mut cache, k.as_mut());
    touch(&mut doc, last);
    let first = rebuild(&doc, &mut cache, k.as_mut());
    let times = repeats(first);
    let mut edits = Vec::new();
    for _ in 0..times {
        touch(&mut doc, last);
        edits.push(rebuild(&doc, &mut cache, k.as_mut()));
    }
    row("rebuild after an edit of the last feature", median(edits), budget.edit_last);
    // Meshed again after such an edit, on the same kernel: the faces the edit left alone
    // have their triangles still.
    let mut again = Vec::new();
    for _ in 0..times {
        touch(&mut doc, last);
        let mut regen = regenerate_with(&doc, k.as_mut(), Some(&mut cache));
        let t = Instant::now();
        scene(&regen, k.as_mut(), &MeshTol::default())?;
        again.push(ms(t));
        regen.release(k.as_mut());
    }
    row("mesh and measure again after that edit", median(again), budget.mesh);
    // An edit further back, each time after one of the last feature: the jump back is what
    // is timed.
    let mut jumps = Vec::new();
    for _ in 0..times {
        touch(&mut doc, last);
        rebuild(&doc, &mut cache, k.as_mut());
        touch(&mut doc, middle);
        jumps.push(rebuild(&doc, &mut cache, k.as_mut()));
    }
    row("rebuild after an edit half way up the history", median(jumps), budget.edit_middle);
    cache.release(k.as_mut());
    Ok(())
}

/// A plate with twenty pairs of holes and a boss: 42 features, each hole a feature of its own.
pub fn many_holes(make: Make, work: &Path) -> Result<Document, String> {
    let mut e = Engine::new(make(), work);
    let run = |e: &mut Engine, id: &str, p: Value| e.exec(id, &p).map_err(|err| format!("{id}: {err}"));
    let sk = run(&mut e, "sketch.create", json!({ "plane": "xy" }))?["feature"].clone();
    run(&mut e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 220, "y2": 100 }))?;
    run(&mut e, "model.extrude", json!({ "sketch": sk, "distance": 10 }))?;
    let top = run(&mut e, "model.face_ref", json!({ "at": [110, 50, 10] }))?;
    for i in 0..20 {
        let sk = run(&mut e, "sketch.create", json!({ "face": top, "project_edges": false }))?["feature"].clone();
        // Where the part's points are in the sketch on the top face.
        let id = FeatureId(u32::try_from(sk.as_u64().unwrap_or(0)).map_err(|_| "too many features")?);
        let frame = *e.session.regen(e.kernel.as_mut()).sketch_frames.get(&id).ok_or("the sketch on the top has no place")?;
        let mut points = Vec::new();
        for y in [20.0, 80.0] {
            let at = frame.to_local(tenon_geom::Vec3::new(10.0 + 10.0 * f64::from(i), y, 10.0));
            points.push(run(&mut e, "sketch.point", json!({ "sketch": sk, "x": at.x, "y": at.y }))?["point"].clone());
        }
        run(&mut e, "model.hole", json!({ "sketch": sk, "points": points, "diameter": 4, "through_all": true }))?;
    }
    let sk = run(&mut e, "sketch.create", json!({ "face": top, "project_edges": false }))?["feature"].clone();
    let id = FeatureId(u32::try_from(sk.as_u64().unwrap_or(0)).map_err(|_| "too many features")?);
    let at = e
        .session
        .regen(e.kernel.as_mut())
        .sketch_frames
        .get(&id)
        .ok_or("the sketch on the top has no place")?
        .to_local(tenon_geom::Vec3::new(110.0, 50.0, 10.0));
    run(&mut e, "sketch.circle", json!({ "sketch": sk, "cx": at.x, "cy": at.y, "r": 8 }))?;
    run(&mut e, "model.extrude", json!({ "sketch": sk, "distance": 5 }))?;
    Ok(e.session.document().clone())
}

/// A sheet metal tray with a lip turned in from each wall: 100 by 80 and 30 high.
pub const LIPPED_TRAY_SCRIPT: &str = r#"[
{ "run": "sheet.defaults", "with": { "thickness": 2, "bend_radius": 3, "gap": 1 } },
{ "run": "sketch.create", "with": { "plane": "xy" }, "as": "s" },
{ "run": "sketch.rectangle", "with": { "sketch": "$s.feature", "x1": 0, "y1": 0, "x2": 100, "y2": 80 } },
{ "run": "sheet.face", "with": { "sketch": "$s.feature" } },
{ "run": "model.edge_ref", "with": { "at": [50, 0, 2] }, "as": "e0" },
{ "run": "model.edge_ref", "with": { "at": [100, 40, 2] }, "as": "e1" },
{ "run": "model.edge_ref", "with": { "at": [50, 80, 2] }, "as": "e2" },
{ "run": "model.edge_ref", "with": { "at": [0, 40, 2] }, "as": "e3" },
{ "run": "sheet.flange", "with": { "edges": ["$e0", "$e1", "$e2", "$e3"], "height": 30 } },
{ "run": "model.edge_ref", "with": { "at": [50, 2, 30] }, "as": "t0" },
{ "run": "model.edge_ref", "with": { "at": [98, 40, 30] }, "as": "t1" },
{ "run": "model.edge_ref", "with": { "at": [50, 78, 30] }, "as": "t2" },
{ "run": "model.edge_ref", "with": { "at": [2, 40, 30] }, "as": "t3" },
{ "run": "sheet.flange", "with": { "edges": ["$t0", "$t1", "$t2", "$t3"], "height": 12 } }
]"#;

fn lipped_tray(make: Make, work: &Path) -> Result<Document, String> {
    let mut e = Engine::new(make(), work);
    let script = crate::Script::parse(LIPPED_TRAY_SCRIPT)?;
    crate::script::run(&mut e, &script).map_err(|(_, err)| err.to_string())?;
    Ok(e.session.document().clone())
}

/// How long laying `doc` flat takes, once it is rebuilt.
fn flat(rows: &mut Vec<Row>, make: Make, model: &str, doc: &Document, budget: f64) -> Result<(), String> {
    let mut k = make();
    let mut regen = regenerate(doc, k.as_mut());
    let mut took = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        tenon_model::sheet::pattern_of(doc, &regen, k.as_ref(), None, None).map_err(|e| format!("{model}: {e}"))?;
        took.push(ms(t));
    }
    regen.release(k.as_mut());
    rows.push(Row { group: "flat", model: model.to_owned(), what: "lay the part flat", value: median(took), unit: "ms", budget });
    Ok(())
}

/// Opening an assembly and making every part of it ready to show.
fn assembly(rows: &mut Vec<Row>, make: Make, model: &str, file: &Path, budget: f64) -> Result<usize, String> {
    let mut took = Vec::new();
    let mut components = 0;
    for _ in 0..3 {
        let mut e = Engine::new(make(), file.parent().unwrap_or(Path::new(".")));
        let t = Instant::now();
        e.exec("asm.open", &json!({ "path": file.to_string_lossy() }))?;
        e.asm.refresh(e.kernel.as_mut())?;
        took.push(ms(t));
        components = e.asm.assembly().components.len();
    }
    rows.push(Row { group: "assembly", model: model.to_owned(), what: "open, rebuild and mesh every part", value: median(took), unit: "ms", budget });
    Ok(components)
}

/// A large assembly in the view: its parts made ready with their meshes for drawing from far
/// away, what is then drawn of it with the whole assembly in sight, and how long it takes to
/// find the face and the edge under the pointer.
fn assembly_view(rows: &mut Vec<Row>, make: Make, model: &str, file: &Path, budget: [f64; 5]) -> Result<(), String> {
    use tenon_kernel::Placed;
    use tenon_render::detail::{Detail, detail};
    use tenon_render::pick::{Bodies, pick_edge_in, pick_face_in};
    let mut e = Engine::new(make(), file.parent().unwrap_or(Path::new(".")));
    e.exec("asm.open", &json!({ "path": file.to_string_lossy() }))?;
    e.asm.refresh(e.kernel.as_mut())?;
    // (Each part once, however many components are of it.)
    let t = Instant::now();
    e.asm.refresh_shown(e.kernel.as_mut())?;
    let far = ms(t);
    // Every body where its component is, as the app shows them: from its part's own mesh, in
    // full and (where it has one) from far away, with where it stands. Nothing is copied.
    let asm = e.asm.assembly();
    let mut bodies: Vec<(&tenon_kernel::Mesh, Option<&tenon_kernel::Mesh>, &tenon_geom::Frame)> = Vec::new();
    for c in asm.components.iter().filter(|c| c.visible) {
        let Some(scene) = tenon_assembly::session::scene_of(&e.asm.parts, c) else { continue };
        for b in &scene.bodies {
            bodies.push((&b.mesh, b.far.as_deref(), &c.placement));
        }
    }
    let meshes: Vec<Placed> = bodies.iter().map(|(m, _, at)| Placed::new(m, Some(at))).collect();
    // (Each body's middle and reach, where it stands.)
    let boxes: Vec<(tenon_geom::Vec3, f64)> = meshes
        .iter()
        .map(|m| {
            let b = m.mesh.index().bounds();
            (m.at.map_or(b.center(), |at| at.to_world(b.center())), b.radius())
        })
        .collect();
    let whole =
        tenon_geom::Aabb3::from_points(meshes.iter().flat_map(|m| m.corners(&m.mesh.index().bounds()))).ok_or("the assembly shows nothing")?;
    let mut camera = tenon_render::Camera::default();
    camera.set_view(tenon_render::StdView::Home);
    camera.fit(&whole);
    let tree = Bodies::of(&meshes);
    // (The view of a window 1440 by 900: what the ribbon and the browser leave of it.)
    let (w, h) = (1100.0, 700.0);
    let (mut full, mut drawn) = (0, 0);
    for ((mesh, far, _), (center, radius)) in bodies.iter().zip(&boxes) {
        full += mesh.triangle_count();
        drawn += match detail(&camera, w, h, *center, *radius, None) {
            Detail::Hidden => 0,
            Detail::Far => far.unwrap_or(mesh).triangle_count(),
            Detail::Near => mesh.triangle_count(),
        };
    }
    // The pointer over the middle of each body in turn (150 of them at most), twice: the first
    // round also makes the indices of the bodies it comes over, as the first movements of the
    // pointer do.
    let places: Vec<(f64, f64)> = boxes.iter().take(150).filter_map(|(center, _)| camera.project(*center, w, h)).map(|(x, y, _)| (x, y)).collect();
    let mut took = [Vec::new(), Vec::new()];
    let mut found = [0, 0];
    for round in 0..2 {
        for (x, y) in &places {
            let t = Instant::now();
            let (o, d) = camera.ray(*x, *y, w, h);
            let face = pick_face_in(Some(&tree), &meshes, o, d);
            let at = ms(t);
            let t = Instant::now();
            let edge = pick_edge_in(Some(&tree), &meshes, &camera, w, h, *x, *y, 5.0);
            if round == 1 {
                took[0].push(at);
                took[1].push(ms(t));
                found[0] += usize::from(face.is_some());
                found[1] += usize::from(edge.is_some());
            }
        }
    }
    if 2 * found[0] < places.len() || found[1] < 5 {
        return Err(format!("{model}: the pointer found {} faces and {} edges in {} places", found[0], found[1], places.len()));
    }
    let [faces, edges] = took;
    let row = |what, value, unit, budget| Row { group: "view", model: model.to_owned(), what, value, unit, budget };
    rows.push(row("mesh its parts again for drawing from far away", far, "ms", budget[0]));
    rows.push(row("triangles in all its bodies (thousands)", full as f64 / 1000.0, "k", budget[4]));
    rows.push(row("triangles drawn with all of it in view (thousands)", drawn as f64 / 1000.0, "k", budget[1]));
    rows.push(row("find the face under the pointer", median(faces), "ms", budget[2]));
    rows.push(row("find the edge under the pointer", median(edges), "ms", budget[3]));
    Ok(())
}

/// A grid of `side` by `side` components, alternately two of the example parts, written to
/// `dir` beside copies of those parts: an assembly large enough to show what many components
/// cost.
fn grid_assembly(make: Make, examples: &Path, dir: &Path, side: u32) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    for part in ["bolt.tenon", "nozzle.tenon"] {
        std::fs::copy(examples.join("m6-fittings").join(part), dir.join(part)).map_err(|e| format!("cannot copy {part}: {e}"))?;
    }
    let mut e = Engine::new(make(), dir);
    e.exec("asm.new", &json!({ "name": "Grid" }))?;
    for i in 0..side {
        for j in 0..side {
            let part = if (i + j) % 2 == 0 { "bolt.tenon" } else { "nozzle.tenon" };
            e.exec("asm.insert", &json!({ "path": part, "at": [f64::from(i) * 40.0, f64::from(j) * 40.0, 0.0] }))?;
        }
    }
    let file = dir.join("grid.tenonasm");
    e.exec("asm.save", &json!({ "path": file.to_string_lossy() }))?;
    Ok(file)
}

/// The views of a drawing worked out from its model: hidden lines removed, edges projected.
fn drawing(rows: &mut Vec<Row>, make: Make, model: &str, file: &Path, budget: f64) -> Result<(), String> {
    let mut took = Vec::new();
    for _ in 0..3 {
        let mut e = Engine::new(make(), file.parent().unwrap_or(Path::new(".")));
        let t = Instant::now();
        e.exec("drw.open", &json!({ "path": file.to_string_lossy() }))?;
        e.drw.refresh(e.kernel.as_mut());
        took.push(ms(t));
        e.drw.current().map_err(|err| format!("{model}: {err}"))?;
    }
    rows.push(Row { group: "drawing", model: model.to_owned(), what: "open and work out every view", value: median(took), unit: "ms", budget });
    Ok(())
}

/// The memory this process holds, in megabytes, where the system says (the resident set).
pub fn resident_mb() -> Option<f64> {
    let pid = std::process::id().to_string();
    if cfg!(target_os = "linux") {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let kb: f64 = status.lines().find(|l| l.starts_with("VmRSS:"))?.split_whitespace().nth(1)?.parse().ok()?;
        Some(kb / 1024.0)
    } else if cfg!(target_os = "windows") {
        // "tenon-cli.exe","1234","Console","1","123,456 K": the last field, digits only.
        let out = std::process::Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let field = text.lines().find(|l| l.contains(&format!("\"{pid}\"")))?.rsplit("\",\"").next()?.to_owned();
        let digits: String = field.chars().filter(char::is_ascii_digit).collect();
        Some(digits.parse::<f64>().ok()? / 1024.0)
    } else {
        let out = std::process::Command::new("ps").args(["-o", "rss=", "-p", &pid]).output().ok()?;
        Some(String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().ok()? / 1024.0)
    }
}

/// The example parts that are measured, and their budgets: four times what the slowest of CI's
/// three platforms took when they were set (a shared runner has slow minutes), and no less
/// than 100 ms. A change that makes something several times slower fails CI; one that makes
/// it half as slow again does not, and shows in the table that every run prints.
const PARTS: [(&str, &str, PartBudget); 8] = [
    ("m1-bracket", "bracket.tenon", PartBudget { rebuild: 100.0, mesh: 100.0, edit_last: 100.0, edit_middle: 100.0 }),
    ("m2-enclosure", "enclosure.tenon", PartBudget { rebuild: 300.0, mesh: 100.0, edit_last: 100.0, edit_middle: 150.0 }),
    ("m2-mount", "mount.tenon", PartBudget { rebuild: 250.0, mesh: 100.0, edit_last: 100.0, edit_middle: 200.0 }),
    ("m6-fittings", "nozzle.tenon", PartBudget { rebuild: 250.0, mesh: 100.0, edit_last: 100.0, edit_middle: 200.0 }),
    ("m6-fittings", "bolt.tenon", PartBudget { rebuild: 3200.0, mesh: 320.0, edit_last: 3400.0, edit_middle: 3000.0 }),
    ("m6-fittings", "spring.tenon", PartBudget { rebuild: 100.0, mesh: 1600.0, edit_last: 100.0, edit_middle: 100.0 }),
    ("m7-end-cap", "end-cap.tenon", PartBudget { rebuild: 100.0, mesh: 100.0, edit_last: 100.0, edit_middle: 100.0 }),
    ("m8-tray", "tray.tenon", PartBudget { rebuild: 2000.0, mesh: 120.0, edit_last: 100.0, edit_middle: 300.0 }),
];

/// Everything, in order. `examples` is the repository's `examples` folder; `work` takes what
/// is written on the way.
pub fn run(make: Make, examples: &Path, work: &Path) -> Result<Vec<Row>, String> {
    std::fs::create_dir_all(work).map_err(|e| format!("cannot create {}: {e}", work.display()))?;
    let mut rows = Vec::new();
    let open = |dir: &str, file: &str| -> Result<Document, String> {
        let path = examples.join(dir).join(file);
        tenon_io::project::open(&path).map(|(doc, _)| doc).map_err(|e| format!("cannot open {}: {e}", path.display()))
    };
    for (dir, file, budget) in PARTS {
        part(&mut rows, make, &format!("{dir}/{file}"), &open(dir, file)?, budget)?;
    }
    let holes = many_holes(make, work)?;
    part(
        &mut rows,
        make,
        "a plate with 40 holes (42 features)",
        &holes,
        PartBudget { rebuild: 1100.0, mesh: 480.0, edit_last: 100.0, edit_middle: 700.0 },
    )?;
    let lipped = lipped_tray(make, work)?;
    part(
        &mut rows,
        make,
        "a tray with four mitred lips",
        &lipped,
        PartBudget { rebuild: 4000.0, mesh: 150.0, edit_last: 3200.0, edit_middle: 4400.0 },
    )?;

    flat(&mut rows, make, "m8-tray/tray.tenon", &open("m8-tray", "tray.tenon")?, 100.0)?;
    flat(&mut rows, make, "a tray with four mitred lips", &lipped, 100.0)?;

    assembly(&mut rows, make, "m3-pivot/pivot.tenonasm", &examples.join("m3-pivot").join("pivot.tenonasm"), 100.0)?;
    assembly(&mut rows, make, "m6-fittings/fittings.tenonasm", &examples.join("m6-fittings").join("fittings.tenonasm"), 5000.0)?;
    let grid = grid_assembly(make, examples, &work.join("grid"), 10)?;
    let n = assembly(&mut rows, make, "a grid of 100 bolts and nozzles", &grid, 3400.0)?;
    if n != 100 {
        return Err(format!("the grid assembly has {n} components, not 100"));
    }

    assembly_view(&mut rows, make, "a grid of 100 bolts and nozzles", &grid, [400.0, 250.0, 5.0, 10.0, 700.0])?;

    drawing(&mut rows, make, "m4-plate/plate.tenondrw", &examples.join("m4-plate").join("plate.tenondrw"), 600.0)?;

    if let Some(mb) = resident_mb() {
        rows.push(Row {
            group: "memory",
            model: "this run".into(),
            what: "held at the end of all of the above",
            value: mb,
            unit: "MB",
            budget: 800.0,
        });
    }
    Ok(rows)
}

/// The rows as a table, and as data.
pub fn report(rows: &[Row]) -> Report {
    let wide = rows.iter().map(|r| r.model.len()).max().unwrap_or(0);
    let long = rows.iter().map(|r| r.what.len()).max().unwrap_or(0);
    let mut text = format!("{:<9} {:<wide$}  {:<long$}  {:>10}  {:>8}\n", "", "model", "what", "measured", "budget");
    for r in rows {
        let mark = if r.over() { "  OVER" } else { "" };
        // (What takes under ten of its unit is given to a hundredth.)
        let value = if r.value < 10.0 { format!("{:.2}", r.value) } else { format!("{:.1}", r.value) };
        text.push_str(&format!("{:<9} {:<wide$}  {:<long$}  {value:>7} {:<2}  {:>8.0}{mark}\n", r.group, r.model, r.what, r.unit, r.budget));
    }
    let over = rows.iter().filter(|r| r.over()).count();
    text.push_str(&if over == 0 {
        format!("{} measurements, all within budget", rows.len())
    } else {
        format!("{over} of {} measurements over budget", rows.len())
    });
    let data: Vec<Value> = rows
        .iter()
        .map(
            |r| json!({ "group": r.group, "model": r.model, "what": r.what, "value": r.value, "unit": r.unit, "budget": r.budget, "over": r.over() }),
        )
        .collect();
    Report { text, json: json!({ "rows": data, "over": over }) }
}

/// Where the time of rebuilding and meshing each measured part goes: the kernel's share of it,
/// call by call, and what is left for Tenon's own work. For `docs/performance.md`.
pub fn profile(make: Make, examples: &Path, work: &Path) -> Result<Report, String> {
    use tenon_kernel::timed::{Tally, Timed};
    std::fs::create_dir_all(work).map_err(|e| format!("cannot create {}: {e}", work.display()))?;
    let mut docs: Vec<(String, Document)> = Vec::new();
    for (dir, file, _) in PARTS {
        let path = examples.join(dir).join(file);
        let (doc, _) = tenon_io::project::open(&path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        docs.push((format!("{dir}/{file}"), doc));
    }
    docs.push(("a plate with 40 holes (42 features)".into(), many_holes(make, work)?));
    docs.push(("a tray with four mitred lips".into(), lipped_tray(make, work)?));
    let shares = |tally: &Tally, total: f64| -> (String, Value) {
        let mut calls: Vec<(&str, u64, f64)> = tally.iter().map(|(what, (n, ms))| (*what, *n, *ms)).collect();
        calls.sort_by(|a, b| b.2.total_cmp(&a.2));
        let inside: f64 = calls.iter().map(|c| c.2).sum();
        let mut text: Vec<String> = calls.iter().take(5).filter(|c| c.2 >= 0.05).map(|(what, n, ms)| format!("{what} {ms:.1} ms in {n}")).collect();
        text.push(format!("Tenon's own {:.1} ms", (total - inside).max(0.0)));
        let data: Vec<Value> = calls.iter().map(|(what, n, ms)| json!({ "call": what, "times": n, "ms": ms })).collect();
        (text.join(", "), json!({ "total_ms": total, "kernel_ms": inside, "calls": data }))
    };
    let (mut text, mut data) = (String::new(), Vec::new());
    for (model, doc) in &docs {
        let mut k = Timed::new(make());
        let t = Instant::now();
        let mut regen = regenerate(doc, &mut k);
        let rebuilt = ms(t);
        let (rebuild_text, rebuild_data) = shares(&k.tally(), rebuilt);
        k.reset();
        let t = Instant::now();
        scene(&regen, &mut k, &MeshTol::default())?;
        let meshed = ms(t);
        let (mesh_text, mesh_data) = shares(&k.tally(), meshed);
        // The same again at once: what the kernel keeps of a mesh it has made.
        k.reset();
        let t = Instant::now();
        scene(&regen, &mut k, &MeshTol::default())?;
        let again = ms(t);
        let (again_text, again_data) = shares(&k.tally(), again);
        regen.release(&mut k);
        text.push_str(&format!(
            "{model}\n  rebuild {rebuilt:.1} ms: {rebuild_text}\n  mesh and measure {meshed:.1} ms: {mesh_text}\n  the same again {again:.1} ms: {again_text}\n"
        ));
        data.push(json!({ "model": model, "rebuild": rebuild_data, "mesh": mesh_data, "mesh_again": again_data }));
    }
    Ok(Report { text, json: json!({ "parts": data }) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_over_its_budget_is_marked_and_counted() {
        let row = |value: f64| Row { group: "part", model: "a part".into(), what: "rebuild from nothing", value, unit: "ms", budget: 10.0 };
        assert!(!row(10.0).over() && row(10.5).over());
        let report = report(&[row(3.0), row(12.0)]);
        assert!(report.text.contains("OVER") && report.text.ends_with("1 of 2 measurements over budget"), "{}", report.text);
        assert_eq!(report.json["over"], 1);
        assert!(super::report(&[row(3.0)]).text.ends_with("1 measurements, all within budget"));
        assert_eq!(median(vec![5.0, 1.0, 3.0]), 3.0);
    }
}

//! A study worked out, in the app: Simulate, Mesh View, and what the study comes to shown in
//! colour on the part.
//!
//! The working out is `tenon_io::stress` (the part divided by one program and solved by
//! another, DEC-098), run where the kernel is: on its own thread in the app, here in tests.
//! What comes back is drawn over the part: the mesh's boundary as triangles coloured by the
//! result that is chosen, bent as the part bends, with a scale to read the colours by. It is
//! of the part and the study as they were: a change to either puts it out of date.

use std::sync::Arc;
use std::time::Instant;

use egui::{Align2, Color32, Pos2, Rect, Stroke, Ui, pos2, vec2};
use tenon_fea::{Cancel, TetMesh};
use tenon_io::stress::Solved;
use tenon_model::Session;
use tenon_model::study::StudyId;

use crate::print_dialog::PartJob;
use crate::theme::{self, Tokens};
use crate::workbench::{Geo, Workbench};

/// Which result is shown in colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Field {
    #[default]
    VonMises,
    Displacement,
    Safety,
}

impl Field {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Field::VonMises => "Von Mises Stress",
            Field::Displacement => "Displacement",
            Field::Safety => "Safety Factor",
        }
    }
    fn unit(self) -> &'static str {
        match self {
            Field::VonMises => "MPa",
            Field::Displacement => "mm",
            Field::Safety => "",
        }
    }
}

/// How the part's bending is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Deform {
    /// As the part is: no bending shown.
    Undeformed,
    /// Bent enough to be seen, whatever it really is.
    #[default]
    Adjusted,
    /// As far as it really moves.
    Actual,
}

/// A safety factor is drawn up to this: beyond it, "plenty".
const SAFETY_TOP: f64 = 15.0;

/// What is being worked out.
#[derive(Clone, Debug)]
pub(crate) struct Running {
    pub study: StudyId,
    /// True for Simulate; false for the mesh alone (Mesh View).
    pub solving: bool,
    pub cancel: Cancel,
    pub since: Instant,
}

/// What there is to show of a study: its mesh, and when it was solved, the answer.
#[derive(Clone, Debug)]
pub(crate) struct Outcome {
    pub study: StudyId,
    /// The revision of the part it is of.
    pub revision: u64,
    pub solved: Option<Arc<Solved>>,
    /// The mesh alone, when it was not solved.
    meshed: Option<Arc<TetMesh>>,
    pub element_size: f64,
    /// The nodes on the mesh's boundary, and the boundary as flat triangles between them:
    /// four to each six-node triangle (the last of the four is its middle), wound to face
    /// out of the part.
    nodes: Vec<u32>,
    tris: Vec<[u32; 3]>,
    /// The six sides of each six-node triangle, for Mesh View.
    sides: Vec<[u32; 2]>,
}

impl Outcome {
    fn new(study: StudyId, revision: u64, element_size: f64, solved: Option<Arc<Solved>>, meshed: Option<Arc<TetMesh>>) -> Result<Outcome, String> {
        let mut o = Outcome { study, revision, solved, meshed, element_size, nodes: Vec::new(), tris: Vec::new(), sides: Vec::new() };
        let mesh = o.mesh().ok_or("there is no mesh")?;
        let outward = mesh.outward()?;
        let mut place: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        let mut nodes = Vec::new();
        let mut at = |n: u32| {
            *place.entry(n).or_insert_with(|| {
                nodes.push(n);
                (nodes.len() - 1) as u32
            })
        };
        let (mut tris, mut sides) = (Vec::with_capacity(mesh.tris.len() * 4), Vec::with_capacity(mesh.tris.len() * 6));
        for (t, sign) in mesh.tris.iter().zip(&outward) {
            let [a, b, c, ab, bc, ca] = t.nodes.map(&mut at);
            for [x, y, z] in [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]] {
                tris.push(if *sign < 0.0 { [x, z, y] } else { [x, y, z] });
            }
            sides.extend([[a, ab], [ab, b], [b, bc], [bc, c], [c, ca], [ca, a]]);
        }
        (o.nodes, o.tris, o.sides) = (nodes, tris, sides);
        Ok(o)
    }

    pub(crate) fn mesh(&self) -> Option<&TetMesh> {
        self.solved.as_ref().map(|s| &s.results.mesh).or(self.meshed.as_deref())
    }

    /// The value of `field` at each node of the mesh (None when the study was not solved).
    fn values(&self, field: Field) -> Option<Vec<f64>> {
        let s = self.solved.as_ref()?;
        Some(match field {
            Field::VonMises => s.results.von_mises.clone(),
            Field::Displacement => s.results.displacement.iter().map(|d| d.len()).collect(),
            Field::Safety => s.results.von_mises.iter().map(|v| if *v > 0.0 { (s.yield_strength / v).min(SAFETY_TOP) } else { SAFETY_TOP }).collect(),
        })
    }
}

/// A number in three figures, as a scale is read.
pub(crate) fn figures(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return "0".into();
    }
    let digits = (2 - v.abs().log10().floor() as i32).clamp(0, 6) as usize;
    let text = format!("{v:.digits$}");
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_owned() } else { text }
}

/// The colour of a place along a scale from 0 (blue) through green to 1 (red).
fn ramp(t: f64) -> Color32 {
    let t = t.clamp(0.0, 1.0) as f32;
    let stops: [(f32, [f32; 3]); 5] =
        [(0.0, [0.10, 0.25, 0.85]), (0.25, [0.10, 0.75, 0.85]), (0.5, [0.20, 0.75, 0.25]), (0.75, [0.95, 0.85, 0.15]), (1.0, [0.88, 0.16, 0.12])];
    let i = stops.iter().rposition(|(at, _)| t >= *at).unwrap_or(0).min(stops.len() - 2);
    let ((a, ca), (b, cb)) = (stops[i], stops[i + 1]);
    let k = ((t - a) / (b - a)).clamp(0.0, 1.0);
    let mix = |j: usize| ((ca[j] + (cb[j] - ca[j]) * k) * 255.0).round() as u8;
    Color32::from_rgb(mix(0), mix(1), mix(2))
}

impl Workbench {
    /// What there is to show of the study worked on, if it is still of the part as it is.
    pub(crate) fn outcome(&self) -> Option<&Outcome> {
        let env = self.analysis.as_ref().filter(|_| self.in_analysis())?;
        let active = self.active_study()?.id;
        env.outcome.as_ref().filter(|o| o.study == active && o.revision == self.session.revision())
    }

    /// The study worked on has an outcome, and the part or the study has changed since.
    pub(crate) fn outcome_is_stale(&self) -> bool {
        let Some(env) = self.analysis.as_ref() else { return false };
        let active = self.active_study().map(|s| s.id);
        env.outcome.as_ref().is_some_and(|o| Some(o.study) == active && o.revision != self.session.revision())
    }

    /// Something of a study is drawn over the part: its results, or its mesh.
    pub(crate) fn results_shown(&self) -> bool {
        let lines = self.analysis.as_ref().is_some_and(|e| e.mesh_view);
        self.outcome().is_some_and(|o| o.solved.is_some() || lines)
    }

    /// Simulate, or (not `solving`) the mesh alone: sets it going where the kernel is.
    pub(crate) fn work_study_out(&mut self, solving: bool) -> Result<(), String> {
        if !self.in_analysis() {
            self.analysis_begin()?;
        }
        if self.analysis.as_ref().is_some_and(|e| e.running.is_some()) {
            return Err("A study is being worked out: wait for it, or press Esc to stop it.".into());
        }
        let Some(study) = self.active_study().map(|s| s.id) else {
            return Err("There is no study yet: Create Study, then say where the part is held and what pushes on it.".into());
        };
        self.panel = None;
        let (cancel, revision) = (Cancel::default(), self.session.revision());
        let running = Running { study, solving, cancel: cancel.clone(), since: Instant::now() };
        let job = PartJob::Study { study, revision, solving };
        // (The answer is boxed as what `study_job_done` takes it for.)
        let work = move |s: &mut Session, k: &mut dyn tenon_kernel::Kernel| -> Box<dyn std::any::Any + Send> {
            if solving {
                Box::new(tenon_io::stress::work_out(s, k, study, None, &cancel))
            } else {
                Box::new(tenon_io::stress::mesh(s, k, study, &cancel))
            }
        };
        match &mut self.geo {
            Geo::Off => return Err("there is no geometry kernel".into()),
            Geo::Sync(k) => {
                let done = work(&mut self.session, k.as_mut());
                if let Some(env) = &mut self.analysis {
                    env.running = Some(running);
                }
                self.study_job_done(study, revision, solving, done);
            }
            Geo::Worker(w) => {
                // On the kernel's thread, of the part as it is now: a session of its own there.
                let doc = self.session.document().clone();
                self.step_seq += 1;
                self.part_jobs.insert(self.step_seq, job);
                w.job(
                    self.step_seq,
                    Box::new(move |k, _| {
                        let mut s = Session::default();
                        s.replace_document(doc, None);
                        let done = work(&mut s, k);
                        s.release(k);
                        done
                    }),
                );
                if let Some(env) = &mut self.analysis {
                    env.running = Some(running);
                }
                self.set_status(if solving {
                    "Simulating: the part is divided into elements and solved. Esc stops it."
                } else {
                    "Dividing the part into elements. Esc stops it."
                });
            }
        }
        Ok(())
    }

    /// A study's job came back.
    pub(crate) fn study_job_done(&mut self, study: StudyId, revision: u64, solving: bool, result: Box<dyn std::any::Any + Send>) {
        let took = self.analysis.as_mut().and_then(|e| e.running.take()).map(|r| r.since.elapsed().as_secs_f64());
        let name = self.document().study(study).map_or_else(|| "The study".to_owned(), |s| s.name.clone());
        let outcome = if solving {
            let solved = result.downcast::<Result<Solved, String>>().map_or_else(|_| Err("internal: unexpected result".to_owned()), |b| *b);
            solved.and_then(|s| {
                let size = s.element_size;
                Outcome::new(study, revision, size, Some(Arc::new(s)), None)
            })
        } else {
            let meshed = result.downcast::<Result<(TetMesh, f64), String>>().map_or_else(|_| Err("internal: unexpected result".to_owned()), |b| *b);
            meshed.and_then(|(mesh, size)| Outcome::new(study, revision, size, None, Some(Arc::new(mesh))))
        };
        let outcome = match outcome {
            Ok(o) => o,
            Err(e) if e == tenon_fea::tools::STOPPED => {
                self.set_status(format!("{name}: stopped."));
                return;
            }
            Err(e) => {
                self.set_error(e);
                return;
            }
        };
        let said = match &outcome.solved {
            Some(s) => {
                let r = &s.results;
                let material = self.document().material().map_or("the material".to_owned(), |m| m.name.clone());
                let holds = match s.safety_factor() {
                    Some(f) if f >= 1.0 => format!("it holds {} times over", figures(f)),
                    Some(f) => format!("IT YIELDS: the safety factor is {}", figures(f)),
                    None => "nothing works it".to_owned(),
                };
                format!(
                    "{name}: at most {} MPa where {material} yields at {} ({holds}), and it moves {} mm at most. {} nodes{}. {}",
                    figures(r.max_von_mises.value),
                    figures(s.yield_strength),
                    figures(r.max_displacement.value),
                    r.mesh.nodes.len(),
                    took.map_or(String::new(), |t| format!(", {} s", figures(t))),
                    tenon_io::stress::NOTE
                )
            }
            None => {
                let m = outcome.mesh().map_or((0, 0), |m| (m.nodes.len(), m.tets.len()));
                format!("{name}: divided into {} elements of about {} mm, with {} nodes.", m.1, figures(outcome.element_size), m.0)
            }
        };
        let yields = outcome.solved.as_ref().and_then(|s| s.safety_factor()).is_some_and(|f| f < 1.0);
        if let Some(env) = &mut self.analysis {
            if outcome.solved.is_none() {
                env.mesh_view = true;
            }
            env.outcome = Some(outcome);
        }
        if yields { self.set_error(said) } else { self.set_status(said) }
    }

    /// Mesh View: the mesh's lines over the part, on or off. With no mesh of the part as it
    /// is, the part is divided first.
    pub(crate) fn toggle_mesh_view(&mut self) -> Result<(), String> {
        if !self.in_analysis() {
            self.analysis_begin()?;
        }
        // (While a study is being worked out the lines are asked for, and come with its mesh.)
        let running = self.analysis.as_ref().is_some_and(|e| e.running.is_some());
        if self.outcome().is_none() && !running {
            return self.work_study_out(false);
        }
        if let Some(env) = &mut self.analysis {
            env.mesh_view = !env.mesh_view;
        }
        Ok(())
    }

    /// The keys of a study being worked out: Esc stops it.
    pub(crate) fn study_keys(&mut self, ui: &Ui) {
        if self.analysis.as_ref().is_some_and(|e| e.running.is_some()) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.stop_study();
        }
    }

    /// Esc while a study is being worked out stops it. True when there was one to stop.
    pub(crate) fn stop_study(&mut self) -> bool {
        match self.analysis.as_ref().and_then(|e| e.running.as_ref()) {
            Some(r) => {
                r.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                self.set_status("Stopping...");
                true
            }
            None => false,
        }
    }

    /// How many times the part's bending is drawn larger than it is.
    fn deform_scale(&self, o: &Outcome) -> f64 {
        let (Some(s), Some(env)) = (o.solved.as_ref(), self.analysis.as_ref()) else { return 0.0 };
        match env.deform {
            Deform::Undeformed => 0.0,
            Deform::Actual => 1.0,
            Deform::Adjusted => {
                let most = s.results.max_displacement.value;
                if most > 0.0 { (s.results.mesh.extent() * 0.05 / most).max(1.0) } else { 1.0 }
            }
        }
    }

    /// The study's outcome drawn over the part: its boundary in the colours of the chosen
    /// result (or plain, for a mesh that was not solved), the mesh's lines for Mesh View, the
    /// scale, and where the most is.
    pub(crate) fn results_overlay(&self, ui: &Ui, rect: Rect, t: &Tokens) {
        #[cfg(test)]
        ui.data_mut(|d| d.remove::<(usize, usize)>(egui::Id::new("tn_results_drawn")));
        if !self.results_shown() {
            return;
        }
        let (Some(o), Some(env)) = (self.outcome(), self.analysis.as_ref()) else { return };
        let Some(mesh) = o.mesh() else { return };
        let values = o.values(env.field);
        let scale = self.deform_scale(o);
        let moved = |n: u32| {
            let p = mesh.nodes[n as usize];
            match &o.solved {
                Some(s) if scale != 0.0 => p + s.results.displacement[n as usize] * scale,
                _ => p,
            }
        };
        // The scale's ends: what is on the boundary (where the most always is).
        let on_boundary = |v: &Vec<f64>| o.nodes.iter().map(|n| v[*n as usize]).fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        let range = values.as_ref().map(on_boundary);
        let color_of = |n: u32| match (&values, range) {
            (Some(v), Some((lo, hi))) => {
                let k = if hi > lo { (v[n as usize] - lo) / (hi - lo) } else { 0.0 };
                // (A low safety factor is the red end.)
                ramp(if env.field == Field::Safety { 1.0 - k } else { k })
            }
            _ => Color32::from_rgb(0x9a, 0xa8, 0xb6),
        };

        let painter = ui.painter().with_clip_rect(rect);
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let cam = &self.view.camera;
        // Each boundary node on the screen, with how far away it is.
        let seen: Vec<Option<(Pos2, f64)>> =
            o.nodes.iter().map(|n| cam.project(moved(*n), w, h).map(|(x, y, depth)| (rect.min + vec2(x as f32, y as f32), depth))).collect();
        // The triangles that face the eye, the furthest first so that the nearer cover them.
        let mut facing: Vec<(f64, usize)> = Vec::with_capacity(o.tris.len() / 2);
        for (i, [a, b, c]) in o.tris.iter().enumerate() {
            let (Some((pa, da)), Some((pb, db)), Some((pc, dc))) = (seen[*a as usize], seen[*b as usize], seen[*c as usize]) else { continue };
            // (Wound to face out of the part: seen from outside, clockwise on a screen whose
            // y runs down.)
            let turn = (pb.x - pa.x) * (pc.y - pa.y) - (pb.y - pa.y) * (pc.x - pa.x);
            if turn < 0.0 {
                facing.push((da + db + dc, i));
            }
        }
        facing.sort_by(|x, y| y.0.total_cmp(&x.0));
        let mut drawn = egui::Mesh::default();
        drawn.reserve_vertices(o.nodes.len());
        drawn.reserve_triangles(facing.len());
        for (i, n) in o.nodes.iter().enumerate() {
            drawn.colored_vertex(seen[i].map_or(Pos2::ZERO, |s| s.0), color_of(*n));
        }
        for (_, i) in &facing {
            let [a, b, c] = o.tris[*i];
            drawn.add_triangle(a, b, c);
        }
        painter.add(egui::Shape::mesh(drawn));

        // Mesh View: the sides of the elements' faces, of those that face the eye (told by
        // the middle one of each face's four triangles).
        let mut lines = 0usize;
        if env.mesh_view {
            let ink = Color32::from_black_alpha(if values.is_some() { 110 } else { 170 });
            let mut wire = egui::Mesh::default();
            for (_, i) in facing.iter().filter(|(_, i)| i % 4 == 3) {
                for [a, b] in &o.sides[(i / 4) * 6..(i / 4) * 6 + 6] {
                    let (Some((pa, _)), Some((pb, _))) = (seen[*a as usize], seen[*b as usize]) else { continue };
                    let along = pb - pa;
                    if along.length() < 0.5 {
                        continue;
                    }
                    // A line a pixel wide, as two triangles.
                    let across = vec2(-along.y, along.x) / along.length() * 0.5;
                    let first = wire.vertices.len() as u32;
                    for p in [pa + across, pb + across, pb - across, pa - across] {
                        wire.colored_vertex(p, ink);
                    }
                    wire.add_triangle(first, first + 1, first + 2);
                    wire.add_triangle(first, first + 2, first + 3);
                    lines += 1;
                }
            }
            painter.add(egui::Shape::mesh(wire));
        }
        #[cfg(test)]
        ui.data_mut(|d| d.insert_temp(egui::Id::new("tn_results_drawn"), (facing.len(), lines)));
        #[cfg(not(test))]
        let _ = lines;

        let (Some(s), Some((lo, hi))) = (o.solved.as_ref(), range) else { return };
        // Where the most is (for a safety factor, the least), on the part as it is drawn.
        let peak = match env.field {
            Field::Displacement => s.results.max_displacement,
            Field::VonMises | Field::Safety => s.results.max_von_mises,
        };
        if let Some((x, y, _)) = cam.project(moved(peak.node), w, h) {
            let at = rect.min + vec2(x as f32, y as f32);
            let text = match env.field {
                Field::Safety => format!("Min: {}", figures(lo)),
                field => format!("Max: {} {}", figures(hi), field.unit()),
            };
            let edge = Color32::from_black_alpha(160);
            painter.circle_stroke(at, 5.0, Stroke::new(3.5, edge));
            painter.circle_stroke(at, 5.0, Stroke::new(1.5, Color32::WHITE));
            painter.line_segment([at + vec2(4.0, -4.0), at + vec2(22.0, -22.0)], Stroke::new(1.5, Color32::WHITE));
            let label = at + vec2(26.0, -26.0);
            painter.text(label + vec2(1.0, 1.0), Align2::LEFT_BOTTOM, &text, theme::body(), edge);
            painter.text(label, Align2::LEFT_BOTTOM, &text, theme::body(), Color32::WHITE);
        }

        // The scale: ten bands, the top one the most (the least, for a safety factor: the
        // colours are the other way up, and red is still the bad end).
        let title = if env.field.unit().is_empty() { env.field.title().to_owned() } else { format!("{} ({})", env.field.title(), env.field.unit()) };
        let bar = Rect::from_min_size(pos2(rect.left() + 18.0, rect.top() + 64.0), vec2(16.0, 220.0));
        let back = Rect::from_min_max(bar.min - vec2(10.0, 30.0), pos2(bar.right() + 170.0, bar.bottom() + 58.0));
        painter.rect_filled(back, 4.0, t.panel.gamma_multiply(0.88));
        painter.text(back.min + vec2(10.0, 8.0), Align2::LEFT_TOP, title, theme::body(), t.text);
        const BANDS: usize = 10;
        let band = bar.height() / BANDS as f32;
        for i in 0..BANDS {
            // (From the top: the value falls; for a safety factor it rises.)
            let k = 1.0 - (i as f64 + 0.5) / BANDS as f64;
            let r = Rect::from_min_size(pos2(bar.left(), bar.top() + band * i as f32), vec2(bar.width(), band + 0.5));
            painter.rect_filled(r, 0.0, ramp(k));
        }
        painter.rect_stroke(bar, 0.0, Stroke::new(1.0, t.border), egui::StrokeKind::Outside);
        for i in 0..=BANDS {
            let k = 1.0 - i as f64 / BANDS as f64;
            let value = if env.field == Field::Safety { hi - (hi - lo) * k } else { lo + (hi - lo) * k };
            let more = if env.field == Field::Safety && i == BANDS && hi >= SAFETY_TOP { " or more" } else { "" };
            let y = bar.top() + band * i as f32;
            painter.text(pos2(bar.right() + 8.0, y), Align2::LEFT_CENTER, format!("{}{more}", figures(value)), theme::small(), t.text);
        }
        let shape = match env.deform {
            Deform::Undeformed => "Shown as it is, not bent".to_owned(),
            Deform::Actual => "Bent as far as it moves".to_owned(),
            Deform::Adjusted => format!("Bending shown {} times over", figures(scale)),
        };
        painter.text(pos2(back.left() + 10.0, bar.bottom() + 12.0), Align2::LEFT_TOP, shape, theme::small(), t.text_dim);
        painter.text(pos2(back.left() + 10.0, bar.bottom() + 28.0), Align2::LEFT_TOP, "An engineering aid,", theme::small(), t.text_dim);
        painter.text(pos2(back.left() + 10.0, bar.bottom() + 41.0), Align2::LEFT_TOP, "not a certified analysis.", theme::small(), t.text_dim);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scale_is_read_in_three_figures_and_runs_from_blue_to_red() {
        assert_eq!(
            (figures(59.864), figures(0.200_63), figures(4.176), figures(1234.6)),
            ("59.9".into(), "0.201".into(), "4.18".into(), "1235".into())
        );
        assert_eq!((figures(0.0), figures(250.0), figures(15.0), figures(0.000_95)), ("0".into(), "250".into(), "15".into(), "0.00095".into()));
        let (low, mid, high) = (ramp(0.0), ramp(0.5), ramp(1.0));
        assert!(low.b() > 200 && low.r() < 60, "{low:?}");
        assert!(mid.g() > 170 && mid.r() < 80 && mid.b() < 90, "{mid:?}");
        assert!(high.r() > 200 && high.b() < 60, "{high:?}");
        assert_eq!((ramp(-3.0), ramp(7.0)), (low, high));
    }
}

//! The control port: a running Tenon driven from outside, command by command, while the person
//! at it watches.
//!
//! It is how an agent works *in the app*: every command it runs is a command of the registry,
//! run on the document that is open, so what it does appears in the window as it is done, is in
//! the undo history like anything else, and can be looked at and taken back by whoever sits
//! there. `tenon-cli mcp --attach` speaks MCP on one side and this on the other (docs/mcp.md).
//!
//! **Off unless asked for** (`tenon --control`). It listens on the loopback address only, and
//! every request must carry the token the app made up when it started: without it a request is
//! refused, whoever on the machine sends it.
//!
//! **The wire.** One JSON object a line each way:
//!
//! ```text
//! -> {"id": 7, "token": "...", "run": "model.box", "with": {"width": 60, "depth": 40, "height": 8}}
//! <- {"id": 7, "ok": true, "result": {"sketch": 1, "feature": 2, ...}}
//! <- {"id": 7, "ok": false, "error": "model.box needs `height`: out of the sketch"}
//! ```
//!
//! **Order.** Requests are done one at a time in the order they come. A command that needs the
//! geometry kernel is run on the geometry thread, on a copy of the part as it stands, and what
//! it changed is put into the part as one edit when it comes back; the window goes on drawing
//! meanwhile. `render.png` waits until what is shown is up to date.

use std::any::Any;
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use serde_json::{Value, json};
use tenon_drawing::views::{Evaluation, base_frame, seen_size};
use tenon_drawing::{DrwModel, Orientation, SheetId};
use tenon_geom::Vec2;
pub use tenon_io::control::{announce, withdraw};
use tenon_io::steps::{self, Step};
use tenon_model::schema::{CommandSchema, Ty, arg, opt, req};
use tenon_model::{Document, Run, Session};
use tenon_render::raster;
use tenon_render::{Camera, StdView};

use crate::workbench::{Geo, Workbench};

/// Longest request line taken (hostile-input cap).
pub const MAX_LINE: u64 = 16 * 1024 * 1024;
/// Largest image side `render.png` makes.
pub const MAX_RENDER_SIDE: u32 = 4096;

/// Called (from another thread) when a request has come in, so that the window takes it.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// What the control port's own commands take; the others have their schemas in their registries.
static SCHEMAS: &[CommandSchema] = &[
    CommandSchema::new("control.state", &[]),
    CommandSchema::new(
        "render.png",
        &[
            opt("path", Ty::Path, "the image to write (.png); without it the image is returned base64-encoded"),
            opt(
                "view",
                Ty::Word(&["current", "iso", "front", "back", "left", "right", "top", "bottom"]),
                "where it is seen from: a standard view fitted to the frame (default iso), or current: as the window shows it now",
            ),
            opt("width", Ty::Int, "pixels, 16 to 4096 (default 1024)"),
            opt("height", Ty::Int, "pixels, 16 to 4096 (default 768)"),
            opt("highlight_faces", Ty::List(&Ty::Int), "faces to pick out in colour, by their numbers"),
            opt("highlight_edges", Ty::List(&Ty::Int), "edges to pick out in colour, by their numbers"),
            opt("body", Ty::Int, "the body those faces and edges are of (default 0)"),
            opt("sheet", Ty::Id, "a drawing's sheet (default the one shown), drawn `width` pixels wide"),
        ],
    ),
    CommandSchema::new(
        "batch.run",
        &[
            req("steps", Ty::List(&Ty::Object), "the commands to run, in order, as a script's steps"),
            opt("as_one", Ty::Bool, "all of them as one undo step, or none of them (default true)"),
        ],
    ),
];

/// The schema of a command as the control port takes it.
pub fn schema(id: &str) -> Option<&'static CommandSchema> {
    SCHEMAS.iter().find(|s| s.id == id).or_else(|| tenon_io::schemas::schema(id))
}

/// A request off the wire, and where its answer goes.
struct Incoming {
    id: Value,
    run: String,
    with: Value,
    reply: Sender<Value>,
}

/// What a request is waiting for.
enum Waiting {
    Nothing,
    /// The geometry thread: the job asked of it, and the part's revision when it was asked.
    Job {
        request: u64,
        revision: u64,
    },
    /// The geometry thread, building a model to see how large it is: a base view of it is then
    /// placed with these parameters at the scale that fits its sheet.
    BaseView {
        request: u64,
        model: String,
        params: Value,
    },
    /// What is shown to be up to date, to make a picture of it with these parameters: asked
    /// for in the frame numbered so (the picture is made in a later one, once the part as it
    /// now is has been sent to be rebuilt and has come back).
    Settled(Value, u64),
}

impl Waiting {
    /// The job of the geometry thread that is waited for, if one is.
    fn job(&self) -> Option<u64> {
        match self {
            Waiting::Job { request, .. } | Waiting::BaseView { request, .. } => Some(*request),
            _ => None,
        }
    }
}

/// Where the open documents stood when steps to be run as one began.
struct Marks {
    part: u64,
    assembly: Option<u64>,
    drawing: Option<u64>,
}

/// A request being worked on.
struct Running {
    id: Value,
    reply: Sender<Value>,
    steps: Vec<Step>,
    /// It was `batch.run` (answered with every step's result), and whether as one.
    batch: Option<bool>,
    /// The step to do next.
    at: usize,
    saved: BTreeMap<String, Value>,
    done: Vec<Value>,
    marks: Option<Marks>,
    waiting: Waiting,
    /// How the open documents stood when the step now being done was begun.
    began_at: u64,
    /// What the step that was waited for came to.
    landed: Option<Result<Value, String>>,
}

/// The control port of a workbench.
pub(crate) struct Control {
    pub port: u16,
    pub token: String,
    rx: Receiver<Incoming>,
    stop: Arc<AtomicBool>,
    /// How many frames requests have been taken in.
    frames: u64,
    /// Requests taken and not answered yet, oldest first. Only the first is worked on.
    queue: VecDeque<Running>,
}

impl Drop for Control {
    fn drop(&mut self) {
        // The listening thread is in `accept`: tell it to stop, and knock so that it looks.
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port));
    }
}

/// A token nobody else on the machine can guess: 128 bits from the hasher keys the standard
/// library takes from the system's random numbers.
fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    (0..2u64)
        .map(|salt| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(salt);
            h.write_u128(now);
            h.write_u32(std::process::id());
            format!("{:016x}", h.finish())
        })
        .collect()
}

/// Whether two tokens are the same, taking as long whether or not they are.
fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn refused(id: &Value, why: impl Into<String>) -> Value {
    json!({ "id": id, "ok": false, "error": why.into() })
}

/// One connection: requests in, answers out, until it closes or the app has gone.
fn serve(stream: TcpStream, tx: &Sender<Incoming>, token: &str, waker: Option<&Waker>) {
    let Ok(mut out) = stream.try_clone() else { return };
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.by_ref().take(MAX_LINE + 1).read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let answer = if line.len() as u64 > MAX_LINE {
            // (What is left of the line is not read: the connection is closed.)
            let _ = writeln!(out, "{}", refused(&Value::Null, "request too long"));
            return;
        } else {
            match serde_json::from_slice::<Value>(&line) {
                Err(e) => refused(&Value::Null, format!("a request is one JSON object on a line: {e}")),
                Ok(msg) => {
                    let id = msg.get("id").cloned().unwrap_or(Value::Null);
                    match (msg.get("token").and_then(Value::as_str), msg.get("run").and_then(Value::as_str)) {
                        (given, _) if !given.is_some_and(|t| same_token(t, token)) => {
                            refused(&id, "wrong or missing `token`: it is in the file Tenon named when it was started with --control")
                        }
                        (_, None) => refused(&id, "a request needs `run`: a command id"),
                        (_, Some(run)) => {
                            let (reply, answered) = channel();
                            let with = msg.get("with").cloned().unwrap_or(Value::Null);
                            if tx.send(Incoming { id, run: run.to_owned(), with, reply }).is_err() {
                                return;
                            }
                            if let Some(wake) = waker {
                                wake();
                            }
                            match answered.recv() {
                                Ok(answer) => answer,
                                Err(_) => return,
                            }
                        }
                    }
                }
            }
        };
        if writeln!(out, "{answer}").and_then(|()| out.flush()).is_err() {
            return;
        }
    }
}

impl Control {
    /// Starts listening on the loopback address, on `port` (0: any that is free).
    fn open(port: u16, waker: Option<Waker>) -> std::io::Result<Control> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
        let port = listener.local_addr()?.port();
        let token = new_token();
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (stopped, secret) = (Arc::clone(&stop), token.clone());
        std::thread::Builder::new().name("tenon-control".into()).spawn(move || {
            for stream in listener.incoming() {
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let (tx, secret, waker) = (tx.clone(), secret.clone(), waker.clone());
                let _ = std::thread::Builder::new().name("tenon-control-client".into()).spawn(move || serve(stream, &tx, &secret, waker.as_ref()));
            }
        })?;
        Ok(Control { port, token, rx, stop, frames: 0, queue: VecDeque::new() })
    }
}

/// How a step began.
enum Begun {
    Done(Result<Value, String>),
    Wait(Waiting),
}

impl Running {
    fn of(incoming: Incoming) -> Result<Running, (Value, Sender<Value>, String)> {
        let Incoming { id, run, with, reply } = incoming;
        let with = if with.is_null() { json!({}) } else { with };
        let (steps, batch) = if run == "batch.run" {
            if let Err(e) = schema("batch.run").map_or(Ok(()), |s| tenon_model::schema::check(s, &with)) {
                return Err((id, reply, e));
            }
            let as_one = with.get("as_one").and_then(Value::as_bool).unwrap_or(true);
            let steps: Vec<Step> = match serde_json::from_value(with.get("steps").cloned().unwrap_or(Value::Null)) {
                Ok(steps) => steps,
                Err(e) => return Err((id, reply, format!("`steps` are not steps of a script: {e}"))),
            };
            if steps.len() > steps::MAX_STEPS {
                return Err((id, reply, format!("at most {} steps", steps::MAX_STEPS)));
            }
            if as_one && let Some((i, step)) = steps.iter().enumerate().find(|(_, s)| steps::NOT_AS_ONE.contains(&s.run.as_str())) {
                let why = format!(
                    "step {} ({}): {} cannot be one of several steps run as one: it replaces a document or steps through the history, and could not be taken back with the rest. Run it on its own",
                    i + 1,
                    step.run,
                    step.run
                );
                return Err((id, reply, why));
            }
            (steps, Some(as_one))
        } else {
            (vec![Step { run, with, save_as: None, expect: serde_json::Map::new(), within: None, note: None }], None)
        };
        Ok(Running {
            id,
            reply,
            steps,
            batch,
            at: 0,
            saved: BTreeMap::new(),
            done: Vec::new(),
            marks: None,
            waiting: Waiting::Nothing,
            began_at: 0,
            landed: None,
        })
    }
}

/// The colour of what is picked out in a picture.
const HIGHLIGHT: [u8; 4] = [0xff, 0x8c, 0x1a, 255];

impl Workbench {
    /// Opens the control port on the loopback address, on `port` (0: any that is free), and
    /// gives the port it is on and the token every request must carry. `waker` is called from
    /// another thread when a request comes in (to have the window repaint and take it).
    pub fn listen(&mut self, port: u16, waker: Option<Waker>) -> std::io::Result<(u16, String)> {
        let control = Control::open(port, waker)?;
        let at = (control.port, control.token.clone());
        self.control = Some(control);
        Ok(at)
    }

    /// The control port and its token, when it is open.
    pub fn control_address(&self) -> Option<(u16, &str)> {
        self.control.as_ref().map(|c| (c.port, c.token.as_str()))
    }

    /// Whether requests of the control port are still being worked on.
    pub fn control_busy(&self) -> bool {
        self.control.as_ref().is_some_and(|c| !c.queue.is_empty())
    }

    /// Takes what has come in over the control port and does it, as far as it can be done now.
    /// Called once a frame.
    pub(crate) fn serve_control(&mut self) {
        let Some(mut control) = self.control.take() else { return };
        control.frames += 1;
        self.control_frame = control.frames;
        while let Ok(incoming) = control.rx.try_recv() {
            match Running::of(incoming) {
                Ok(running) => control.queue.push_back(running),
                Err((id, reply, why)) => {
                    let _ = reply.send(refused(&id, why));
                }
            }
        }
        // Only the first is worked on: commands are done in the order they came.
        while let Some(mut running) = control.queue.pop_front() {
            match self.advance(&mut running) {
                Some(answer) => {
                    let _ = running.reply.send(answer);
                }
                None => {
                    control.queue.push_front(running);
                    break;
                }
            }
        }
        self.control = Some(control);
    }

    /// The geometry thread has answered job `request`. Ok when it was one of the control port's.
    pub(crate) fn control_job_done(&mut self, request: u64, result: Box<dyn Any + Send>) -> Result<(), Box<dyn Any + Send>> {
        let Some(mut control) = self.control.take() else { return Err(result) };
        let awaited = match control.queue.front_mut().map(|r| &mut r.waiting) {
            Some(w @ (Waiting::Job { .. } | Waiting::BaseView { .. })) if w.job() == Some(request) => std::mem::replace(w, Waiting::Nothing),
            _ => {
                self.control = Some(control);
                return Err(result);
            }
        };
        const UNEXPECTED: &str = "the geometry thread gave an answer that was not expected";
        let landed = match awaited {
            Waiting::Job { revision, .. } => match result.downcast::<(Result<Value, String>, Option<Document>)>() {
                Err(_) => Err(UNEXPECTED.to_owned()),
                Ok(answer) => {
                    let (result, changed) = *answer;
                    match (result, changed) {
                        (Ok(value), Some(after)) if self.session.revision() == revision => self
                            .session
                            .edit(|d| {
                                *d = after;
                                Ok(value)
                            })
                            .map_err(|e| e.to_string()),
                        (Ok(_), Some(_)) => Err("the part was changed in the app while the command ran: nothing was done, run it again".to_owned()),
                        (result, _) => result,
                    }
                }
            },
            Waiting::BaseView { model, mut params, .. } => match result.downcast::<Evaluation>() {
                Err(_) => Err(UNEXPECTED.to_owned()),
                Ok(ev) => match ev.models.get(&model).and_then(|m| m.error.clone()) {
                    Some(e) => Err(format!("the model does not build: {e}")),
                    None => {
                        // The scale that fits the sheet, as the command picks it when it has
                        // the kernel at hand. (What is wrong with the other parameters is for
                        // the command to say.)
                        let seen = params["orientation"].as_str().map_or(Some(Orientation::Front), Orientation::from_id);
                        let sheet = self.drw.as_ref().and_then(|d| {
                            let drawing = d.session.drawing();
                            let sheet = match params.get("sheet").and_then(Value::as_u64) {
                                Some(id) => u32::try_from(id).ok().and_then(|id| drawing.sheet(SheetId(id))),
                                None => drawing.sheets.first(),
                            };
                            sheet.map(|s| Vec2::new(s.size.width, s.size.height))
                        });
                        let size = seen.and_then(|o| ev.models.get(&model).and_then(|m| seen_size(m, &base_frame(o))));
                        if let Some((size, sheet)) = size.zip(sheet) {
                            params["scale"] = json!(tenon_drawing::cmd::fitting_scale(size, sheet));
                        }
                        self.drw_exec("drw.view.base", params)
                    }
                },
            },
            _ => Err(UNEXPECTED.to_owned()),
        };
        if let Some(front) = control.queue.front_mut() {
            front.landed = Some(landed);
        }
        self.control = Some(control);
        Ok(())
    }

    /// Does as much of a request as can be done now: its answer when it is finished, None while
    /// it waits (for the geometry thread, or for what is shown to be up to date).
    fn advance(&mut self, r: &mut Running) -> Option<Value> {
        loop {
            let outcome = if let Some(landed) = r.landed.take() {
                landed
            } else {
                match std::mem::replace(&mut r.waiting, Waiting::Nothing) {
                    waiting @ (Waiting::Job { .. } | Waiting::BaseView { .. }) => {
                        r.waiting = waiting;
                        return None;
                    }
                    Waiting::Settled(params, asked) => {
                        if asked >= self.control_frame || !self.is_settled() {
                            r.waiting = Waiting::Settled(params, asked);
                            return None;
                        }
                        self.control_render(&params)
                    }
                    Waiting::Nothing => {
                        let Some(step) = r.steps.get(r.at).cloned() else {
                            // Every step is done.
                            if r.batch == Some(true)
                                && let Some(marks) = r.marks.take()
                            {
                                self.squash_since(&marks);
                            }
                            let result = match r.batch {
                                Some(as_one) => json!({ "steps": std::mem::take(&mut r.done), "as_one": as_one }),
                                None => r.done.pop().map_or(Value::Null, |d| d["result"].clone()),
                            };
                            return Some(json!({ "id": r.id, "ok": true, "result": result }));
                        };
                        // An assembly's parts and a drawing's views are worked out on the
                        // geometry thread after each change: the next step waits for them, so
                        // that it sees what the one before it came to.
                        if self.behind_its_geometry() {
                            return None;
                        }
                        if r.batch == Some(true) && r.marks.is_none() {
                            r.marks = Some(self.marks());
                        }
                        r.began_at = self.control_revision();
                        match steps::substitute(&step.with, &r.saved) {
                            Err(e) => Err(e),
                            Ok(params) => match self.begin(&step.run, &if params.is_null() { json!({}) } else { params }) {
                                Begun::Done(outcome) => outcome,
                                Begun::Wait(waiting) => {
                                    r.waiting = waiting;
                                    // (Looked at again at once: a job may be answered on this thread
                                    // already; a picture waits for a later frame.)
                                    continue;
                                }
                            },
                        }
                    }
                }
            };
            let step = r.steps.get(r.at).cloned();
            let outcome = outcome.and_then(|result| match &step {
                Some(step) => steps::check(&result, &step.expect, step.within.unwrap_or(steps::EXPECT_REL)).map(|()| result),
                None => Ok(result),
            });
            match (outcome, step) {
                (Ok(result), Some(step)) => {
                    // What was done from outside is said in the window.
                    let opened = matches!(step.run.as_str(), "file.open" | "asm.open" | "drw.open");
                    if self.control_revision() != r.began_at {
                        self.set_status(format!("Remote: {}", step.run));
                    }
                    if (self.control_revision() != r.began_at || opened) && (self.in_assembly() || self.in_drawing()) {
                        self.control_touched = self.control_frame;
                    }
                    if let Some(name) = &step.save_as {
                        r.saved.insert(name.clone(), result.clone());
                    }
                    r.done.push(json!({ "step": r.at + 1, "run": step.run, "result": result }));
                    r.at += 1;
                }
                (Ok(_), None) => r.at += 1,
                (Err(e), step) => {
                    let run = step.map_or_else(String::new, |s| s.run);
                    let why = match r.batch {
                        None => e,
                        Some(true) => {
                            if let Some(marks) = r.marks.take() {
                                self.roll_back_to(&marks);
                            }
                            format!("step {} ({run}): {e}. Nothing was changed: the {} step(s) before it were taken back.", r.at + 1, r.done.len())
                        }
                        Some(false) => format!("step {} ({run}): {e}. The {} step(s) before it stand.", r.at + 1, r.done.len()),
                    };
                    return Some(refused(&r.id, why));
                }
            }
        }
    }

    /// Whether an open assembly or drawing has still to catch up with what was last done to it:
    /// its parts rebuilt, its views drawn. (They are sent to the geometry thread in the frame
    /// after a change, so that frame is waited for too.) A part needs no such wait: a command
    /// that needs its geometry rebuilds it where it runs.
    fn behind_its_geometry(&self) -> bool {
        if !matches!(self.geo, Geo::Worker(_)) || !(self.in_assembly() || self.in_drawing()) {
            return false;
        }
        let views_due = self.drw.as_ref().is_some_and(|d| d.editing.is_none() && d.session.evaluation().is_none());
        self.control_touched >= self.control_frame || self.is_busy() || views_due
    }

    fn marks(&self) -> Marks {
        Marks {
            part: self.session.mark(),
            assembly: self.asm.as_ref().map(|a| a.session.mark()),
            drawing: self.drw.as_ref().map(|d| d.session.mark()),
        }
    }

    fn squash_since(&mut self, marks: &Marks) {
        self.session.squash_since(marks.part);
        if let (Some(a), Some(at)) = (self.asm.as_mut(), marks.assembly) {
            a.session.squash_since(at);
        }
        if let (Some(d), Some(at)) = (self.drw.as_mut(), marks.drawing) {
            d.session.squash_since(at);
        }
    }

    fn roll_back_to(&mut self, marks: &Marks) {
        self.session.roll_back_to(marks.part);
        if let (Some(a), Some(at)) = (self.asm.as_mut(), marks.assembly) {
            a.session.roll_back_to(at);
            a.dof = None;
        }
        if let (Some(d), Some(at)) = (self.drw.as_mut(), marks.drawing) {
            d.session.roll_back_to(at);
        }
    }

    /// A number that changes whenever an open document does.
    fn control_revision(&self) -> u64 {
        let asm = self.asm.as_ref().map_or(0, |a| a.session.revision());
        let drw = self.drw.as_ref().map_or(0, |d| d.session.revision());
        self.session.revision().wrapping_add(asm.wrapping_mul(1_000_003)).wrapping_add(drw.wrapping_mul(1_000_033))
    }

    /// What is open and how it stands (`control.state`).
    fn control_state(&self) -> Value {
        let (kind, name) = if let Some(d) = &self.drw {
            ("drawing", d.session.drawing().name.clone())
        } else if let Some(a) = &self.asm {
            ("assembly", a.session.assembly().name.clone())
        } else {
            ("part", self.session.document().name.clone())
        };
        json!({
            "tenon": env!("CARGO_PKG_VERSION"),
            "schema_version": tenon_model::schema::VERSION,
            "document": kind,
            "name": name,
            "path": self.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
            "revision": self.control_revision(),
            "unsaved": self.unsaved_files(),
            "busy": self.is_busy(),
            "sketching": self.is_sketching(),
            "panel_open": self.panel.is_some(),
        })
    }

    /// A refusal for want of the kernel, said as what it is here: the kernel is on its own
    /// thread, and of an assembly's and a drawing's commands only the app's own buttons are run
    /// there so far.
    fn not_over_the_port(&self, id: &str, e: String) -> String {
        if matches!(self.geo, Geo::Worker(_)) && e.ends_with("needs the geometry kernel") {
            format!("{id} is not run over the control port yet: run it in the app, or on the saved file with tenon-cli (mcp, or a script)")
        } else {
            e
        }
    }

    /// What New and Open would lose, as a refusal.
    fn unsaved_refusal(&self, id: &str) -> Option<String> {
        let unsaved = self.unsaved_files();
        (!unsaved.is_empty()).then(|| {
            format!(
                "{id} would lose unsaved changes ({}): save first (file.save, asm.save or drw.save with a path), or leave it to the person at the app",
                unsaved.join(", ")
            )
        })
    }

    /// Begins a step: runs it if it can be run here and now, or sets it going and says what to
    /// wait for.
    fn begin(&mut self, id: &str, params: &Value) -> Begun {
        // What comes in here comes from outside: held to the command's schema first.
        if let Some(schema) = schema(id)
            && let Err(e) = tenon_model::schema::check(schema, params)
        {
            return Begun::Done(Err(e));
        }
        let path_of = |p: &Value| p.get("path").and_then(Value::as_str).map(str::to_owned);
        match id {
            "control.state" => Begun::Done(Ok(self.control_state())),
            "render.png" => Begun::Wait(Waiting::Settled(params.clone(), self.control_frame)),
            "batch.run" => Begun::Done(Err("batch.run cannot be a step of batch.run".into())),
            "file.open" | "asm.open" | "drw.open" => Begun::Done(match (self.unsaved_refusal(id), path_of(params)) {
                (Some(why), _) => Err(why),
                (None, None) => Err(format!("{id} needs `path`")),
                (None, Some(path)) => self.open(Path::new(&path)).map(|()| self.control_state()),
            }),
            "file.new" => Begun::Done(match self.unsaved_refusal(id) {
                Some(why) => Err(why),
                None => self.run_unprompted("file.new").map(|()| {
                    if let Some(name) = params.get("name").and_then(Value::as_str).filter(|n| !n.trim().is_empty()) {
                        let _ = self.exec("document.rename", json!({ "name": name }));
                        self.session.mark_saved();
                    }
                    self.control_state()
                }),
            }),
            "library.new" | "asm.new" | "drw.new" => {
                Begun::Done(Err(format!("{id} is not run over the control port: start it in the app (File > New), which sets up its window too")))
            }
            "file.save" => Begun::Done(match path_of(params) {
                _ if self.in_assembly() || self.in_drawing() => Err("a part is not what is open: asm.save or drw.save saves it".into()),
                Some(path) => self.save(Path::new(&path)).map(|()| json!({ "path": path })),
                None => Err("file.save needs `path`".into()),
            }),
            "asm.save" => Begun::Done(match path_of(params) {
                Some(path) => self.save_assembly(Path::new(&path)).map(|()| json!({ "path": path })),
                None => Err("asm.save needs `path`".into()),
            }),
            "drw.save" => Begun::Done(match path_of(params) {
                Some(path) => self.save_drawing(Path::new(&path)).map(|()| json!({ "path": path })),
                None => Err("drw.save needs `path`".into()),
            }),
            "drw.view.base" if matches!(self.geo, Geo::Worker(_)) && params.get("scale").is_none_or(Value::is_null) => self.begin_base_view(params),
            _ if id.starts_with("asm.") => Begun::Done(self.asm_exec(id, params.clone()).map_err(|e| self.not_over_the_port(id, e))),
            _ if id.starts_with("drw.") => Begun::Done(self.drw_exec(id, params.clone()).map_err(|e| self.not_over_the_port(id, e))),
            _ => self.begin_part_command(id, params),
        }
    }

    /// A base view with no scale said: the model is built on the geometry thread first, to see
    /// how large it is, as the Base View button does. The view is placed when that is back.
    fn begin_base_view(&mut self, params: &Value) -> Begun {
        let file = params.get("model").and_then(Value::as_str).map(Path::new);
        let (Some(d), Some(file), Geo::Worker(worker)) = (self.drw.as_mut(), file, &self.geo) else {
            // (No drawing, or no model named: the command says which.)
            return Begun::Done(self.drw_exec("drw.view.base", params.clone()));
        };
        let key = tenon_io::asm::part_key(file);
        if !d.session.models.contains_key(&key) {
            let model = tenon_io::drw::load_model(file);
            if let DrwModel::Missing(why) = &model {
                return Begun::Done(Err(format!("cannot open {}: {why}", file.display())));
            }
            d.session.models.insert(key.clone(), model);
            tenon_io::drw::restamp(&mut d.session, &key);
        }
        let (sources, probe) = tenon_io::drw::probe(&d.session, &key);
        self.step_seq += 1;
        let request = self.step_seq;
        worker.job(request, Box::new(move |k, _| Box::new(tenon_drawing::views::evaluate(k, &sources, &probe))));
        Begun::Wait(Waiting::BaseView { request, model: key, params: params.clone() })
    }

    /// A command on the part: here if it needs no kernel (or the kernel is on this thread), else
    /// on the geometry thread, on a copy of the part.
    fn begin_part_command(&mut self, id: &str, params: &Value) -> Begun {
        let Some(spec) = tenon_io::cmd::find(id) else {
            return Begun::Done(Err(format!("unknown command `{id}`")));
        };
        if self.in_assembly() || self.in_drawing() {
            return Begun::Done(Err(format!(
                "{id} works on a part, and {} is open: open the part, or use asm.edit_part to edit a component's part in place",
                if self.in_drawing() { "a drawing" } else { "an assembly" }
            )));
        }
        let changes = spec.mutates || matches!(id, "edit.undo" | "edit.redo");
        if changes && self.panel.is_some() {
            return Begun::Done(Err(format!(
                "{id} would change the part while a command's panel is open in the app: finish or cancel that first (control.state says `panel_open`)"
            )));
        }
        let on_this_thread = matches!(spec.run, Run::Doc(_)) || matches!(self.geo, Geo::Sync(_));
        if on_this_thread {
            return Begun::Done(self.exec(id, params.clone()));
        }
        let Geo::Worker(worker) = &self.geo else {
            return Begun::Done(Err(format!("{id} needs the geometry kernel, and there is none")));
        };
        let doc = self.session.document().clone();
        let revision = self.session.revision();
        let (id, params) = (id.to_owned(), params.clone());
        self.step_seq += 1;
        let request = self.step_seq;
        worker.job(
            request,
            Box::new(move |k, _| {
                let before = doc.clone();
                let mut session = Session::new(doc);
                let result = tenon_io::cmd::run(&mut session, &id, &params, Some(&mut *k)).map_err(|e| e.to_string());
                let after = session.document().clone();
                session.release(k);
                let changed = (after != before).then_some(after);
                Box::new((result, changed)) as Box<dyn Any + Send>
            }),
        );
        Begun::Wait(Waiting::Job { request, revision })
    }

    /// `render.png`: what is open as a picture (the software renderer, so that it is the same
    /// with any graphics card and with none).
    fn control_render(&mut self, p: &Value) -> Result<Value, String> {
        let side = |key: &str, default: u32| -> Result<u32, String> {
            match arg(p, key) {
                None | Some(Value::Null) => Ok(default),
                Some(v) => v
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| (16..=MAX_RENDER_SIDE).contains(n))
                    .ok_or_else(|| format!("`{key}` must be a whole number of pixels from 16 to {MAX_RENDER_SIDE}")),
            }
        };
        let (mut w, mut h) = (side("width", 1024)?, side("height", 768)?);
        let png = if let Some(d) = &self.drw {
            let sheet = arg(p, "sheet").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(d.sheet.0);
            let graphics = tenon_io::drw::sheets_graphics(&d.session, Some(sheet)).map_err(|e| e.to_string())?;
            let g = graphics.first().ok_or("no sheet")?;
            let img = tenon_render::sheet::rasterize(g, f64::from(w) / g.width.max(1.0));
            (w, h) = (img.width, img.height);
            raster::encode_png(&img)?
        } else {
            let scene = self.scene();
            if scene.bodies.is_empty() {
                return Err("there is nothing to draw yet".into());
            }
            let numbers = |key: &str| -> Result<Vec<u32>, String> {
                match arg(p, key) {
                    None | Some(Value::Null) => Ok(Vec::new()),
                    Some(v) => serde_json::from_value(v.clone()).map_err(|_| format!("`{key}` must be a list of face or edge numbers")),
                }
            };
            let body = arg(p, "body").and_then(Value::as_u64).unwrap_or(0) as usize;
            let (faces, edges) = (numbers("highlight_faces")?, numbers("highlight_edges")?);
            if let Some(b) = scene.bodies.get(body) {
                if let Some(f) = faces.iter().find(|f| **f as usize >= b.faces.len()) {
                    return Err(format!("body {body} has no face {f} to pick out: it has {}", b.faces.len()));
                }
                if let Some(e) = edges.iter().find(|e| **e as usize >= b.edges.len()) {
                    return Err(format!("body {body} has no edge {e} to pick out: it has {}", b.edges.len()));
                }
            } else if !(faces.is_empty() && edges.is_empty()) {
                return Err(format!("there is no body {body} to pick faces or edges out on"));
            }
            let camera = match arg(p, "view").and_then(Value::as_str).unwrap_or("iso") {
                "current" => self.view.camera,
                name => {
                    let view = StdView::from_name(name)
                        .ok_or_else(|| format!("unknown view `{name}` (current, iso, front, back, left, right, top, bottom)"))?;
                    let mut camera = Camera::default();
                    camera.set_view(view);
                    if let Some(b) = scene.bbox() {
                        camera.fit(&b);
                    }
                    camera
                }
            };
            let meshes = scene.placed();
            let rgba = |c: egui::Color32| [c.r(), c.g(), c.b(), 255];
            let style = raster::Style {
                body_colors: (0..meshes.len()).map(|bi| (bi, rgba(self.body_palette(bi).0))).collect(),
                face_colors: faces.iter().map(|f| (body, *f, HIGHLIGHT)).collect(),
                edge_colors: edges.iter().map(|e| (body, *e, HIGHLIGHT)).collect(),
                ..raster::Style::default()
            };
            raster::encode_png(&raster::render(&meshes, &camera, w, h, &style))?
        };
        match arg(p, "path").and_then(Value::as_str) {
            Some(path) => {
                std::fs::write(path, &png).map_err(|e| format!("cannot write {path}: {e}"))?;
                Ok(json!({ "path": path, "width": w, "height": h, "bytes": png.len() }))
            }
            None => Ok(json!({ "width": w, "height": h, "png_base64": tenon_io::base64(&png) })),
        }
    }
}

//! Regeneration off the UI thread. The worker owns the kernel; the UI sends document snapshots
//! and receives [`Scene`]s. A newer snapshot cancels the one in progress (between kernel
//! operations), and only the latest result is reported. On failure the UI keeps showing the last
//! good scene.
//!
//! Several documents can be kept at once, each in its own slot (the parts of an assembly); slot
//! 0 is the part being edited. Jobs run arbitrary kernel work against the slots' results
//! (interference, exporting an assembly) without the worker knowing what they are.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;

use tenon_kernel::{CancelToken, Kernel, MeshTol, ShapeHandle};

use crate::Document;
use crate::measure::{Entity, Measurement, measure};
use crate::regen::{Regen, RegenCache, Scene, regenerate, regenerate_with, scene};

/// Work a job does with the kernel and the slots' last results (by slot).
pub type Job = Box<dyn FnOnce(&mut dyn Kernel, &BTreeMap<u64, Regen>) -> Box<dyn Any + Send> + Send>;

enum Request {
    /// `fresh`: from scratch, not from the last checkpoint (Rebuild All).
    Regenerate {
        slot: u64,
        revision: u64,
        doc: Box<Document>,
        fresh: bool,
    },
    ExportStep {
        request: u64,
        doc: Box<Document>,
    },
    /// Measures on the part last regenerated in slot 0.
    Measure {
        request: u64,
        a: Entity,
        b: Option<Entity>,
    },
    Job {
        request: u64,
        job: Job,
    },
    /// Forgets a slot (a part no longer shown).
    Drop {
        slot: u64,
    },
    Shutdown,
}

/// What the worker reports.
pub enum Response {
    /// The scene of document revision `revision` in `slot` (feature errors are inside
    /// `scene.status`).
    Scene { slot: u64, revision: u64, scene: Box<Scene> },
    /// Regeneration of `revision` could not produce a scene at all.
    Failed { slot: u64, revision: u64, message: String },
    /// STEP data of the bodies of a document snapshot.
    Step { request: u64, result: Result<Vec<u8>, String> },
    /// A measurement on the part last regenerated.
    Measure { request: u64, result: Result<Measurement, String> },
    /// What a job returned.
    Job { request: u64, result: Box<dyn Any + Send> },
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Response::Scene { slot, revision, .. } => write!(f, "Scene(slot {slot}, revision {revision})"),
            Response::Failed { slot, revision, message } => write!(f, "Failed(slot {slot}, revision {revision}: {message})"),
            Response::Step { request, .. } => write!(f, "Step({request})"),
            Response::Measure { request, .. } => write!(f, "Measure({request})"),
            Response::Job { request, .. } => write!(f, "Job({request})"),
        }
    }
}

/// No regeneration is running.
const IDLE: u64 = u64::MAX;

/// Handle to the regeneration thread.
pub struct Worker {
    tx: Sender<Request>,
    rx: Receiver<Response>,
    cancel: CancelToken,
    /// The slot being regenerated (`IDLE`: none): a new request cancels only its own slot.
    running: Arc<AtomicU64>,
    handle: Option<JoinHandle<()>>,
}

/// Called (from the worker thread) whenever a response is ready, e.g. to wake the UI.
pub type Waker = Box<dyn Fn() + Send>;

fn bodies(r: &Regen) -> Vec<ShapeHandle> {
    r.bodies.iter().map(|b| b.shape).collect()
}

/// What the worker keeps per slot.
#[derive(Default)]
struct Slots {
    current: BTreeMap<u64, Regen>,
    caches: BTreeMap<u64, RegenCache>,
}

impl Slots {
    fn release(&mut self, k: &mut dyn Kernel) {
        for r in self.current.values_mut() {
            r.release(k);
        }
        for c in self.caches.values_mut() {
            c.release(k);
        }
        self.current.clear();
        self.caches.clear();
    }
    fn drop_slot(&mut self, k: &mut dyn Kernel, slot: u64) {
        if let Some(mut r) = self.current.remove(&slot) {
            r.release(k);
        }
        if let Some(mut c) = self.caches.remove(&slot) {
            c.release(k);
        }
    }
}

impl Worker {
    /// Starts the thread. `make_kernel` runs on it, so the kernel never crosses threads.
    pub fn spawn(make_kernel: impl FnOnce() -> Box<dyn Kernel> + Send + 'static, tol: MeshTol, waker: Option<Waker>) -> std::io::Result<Worker> {
        let (tx, worker_rx) = mpsc::channel::<Request>();
        let (worker_tx, rx) = mpsc::channel::<Response>();
        let cancel = CancelToken::new();
        let token = cancel.clone();
        let running = Arc::new(AtomicU64::new(IDLE));
        let watching = running.clone();
        let handle = std::thread::Builder::new().name("tenon-regen".into()).spawn(move || {
            let mut k = make_kernel();
            k.set_cancel(token.clone());
            // Checkpoints between regenerations: an edit recomputes from the feature it changed.
            let mut slots = Slots::default();
            let send = |r: Response| {
                let _ = worker_tx.send(r);
                if let Some(w) = &waker {
                    w();
                }
            };
            let mut pending: std::collections::VecDeque<Request> = std::collections::VecDeque::new();
            loop {
                if pending.is_empty() {
                    match worker_rx.recv() {
                        Ok(r) => pending.push_back(r),
                        Err(_) => break,
                    }
                }
                loop {
                    match worker_rx.try_recv() {
                        Ok(next) => pending.push_back(next),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            slots.release(&mut *k);
                            return;
                        }
                    }
                }
                let Some(req) = pending.pop_front() else { continue };
                match req {
                    Request::Shutdown => break,
                    Request::Regenerate { slot, revision, doc, fresh } => {
                        // Only the newest regeneration of a slot matters, unless something else
                        // was asked for in between (it must see this one).
                        let newer = pending.iter_mut().take_while(|r| matches!(r, Request::Regenerate { .. })).find_map(|r| match r {
                            Request::Regenerate { slot: s, fresh: f, .. } if *s == slot => Some(f),
                            _ => None,
                        });
                        if let Some(f) = newer {
                            // A pending Rebuild All survives being superseded.
                            *f |= fresh;
                            continue;
                        }
                        running.store(slot, Ordering::SeqCst);
                        let done = Self::run_regen(&mut *k, &mut slots, slot, revision, &doc, fresh, &tol, &token, &send);
                        running.store(IDLE, Ordering::SeqCst);
                        if !done {
                            // Cancelled: a newer request for this slot is coming; if not, this
                            // one runs again.
                            pending.push_front(Request::Regenerate { slot, revision, doc, fresh });
                        }
                    }
                    other => Self::handle(&mut *k, &mut slots, other, &token, &send),
                }
            }
            slots.release(&mut *k);
        })?;
        Ok(Worker { tx, rx, cancel, running: watching, handle: Some(handle) })
    }

    #[allow(clippy::too_many_arguments)]
    fn run_regen(
        k: &mut dyn Kernel,
        slots: &mut Slots,
        slot: u64,
        revision: u64,
        doc: &Document,
        fresh: bool,
        tol: &MeshTol,
        token: &CancelToken,
        send: &dyn Fn(Response),
    ) -> bool {
        token.reset();
        if let Some(mut old) = slots.current.remove(&slot) {
            old.release(k);
        }
        let cache = slots.caches.entry(slot).or_default();
        if fresh {
            cache.release(k);
        }
        let regen = regenerate_with(doc, k, Some(cache));
        if regen.cancelled {
            let mut r = regen;
            r.release(k);
            return false;
        }
        let result = scene(&regen, k, tol);
        slots.current.insert(slot, regen);
        match result {
            Ok(s) => send(Response::Scene { slot, revision, scene: Box::new(s) }),
            Err(message) => send(Response::Failed { slot, revision, message }),
        }
        true
    }

    fn handle(k: &mut dyn Kernel, slots: &mut Slots, req: Request, token: &CancelToken, send: &dyn Fn(Response)) {
        match req {
            Request::ExportStep { request, doc } => {
                token.reset();
                let mut regen = regenerate(&doc, k);
                let result = match regen.first_error() {
                    Some((_, msg)) => Err(format!("the part does not regenerate: {msg}")),
                    None if regen.bodies.is_empty() => Err("there is no solid to export".into()),
                    None => k.export_step(&bodies(&regen)).map_err(|e| e.to_string()),
                };
                regen.release(k);
                send(Response::Step { request, result });
            }
            Request::Measure { request, a, b } => {
                token.reset();
                let empty = Regen::default();
                let current = slots.current.get(&0).unwrap_or(&empty);
                send(Response::Measure { request, result: measure(k, current, a, b) });
            }
            Request::Job { request, job } => {
                token.reset();
                let result = job(k, &slots.current);
                send(Response::Job { request, result });
            }
            Request::Drop { slot } => slots.drop_slot(k, slot),
            Request::Regenerate { .. } | Request::Shutdown => {}
        }
    }

    /// Regenerates `doc` (as revision `revision`) in slot 0, cancelling any regeneration in
    /// progress. With `fresh`, every feature is recomputed (Rebuild All); otherwise the
    /// regeneration resumes from the last checkpoint where the history before it is unchanged.
    pub fn regenerate(&self, revision: u64, doc: Document, fresh: bool) {
        self.regenerate_slot(0, revision, doc, fresh);
    }

    /// Regenerates `doc` in `slot`, as [`Worker::regenerate`] does in slot 0.
    pub fn regenerate_slot(&self, slot: u64, revision: u64, doc: Document, fresh: bool) {
        // A regeneration of this slot in progress is out of date; other slots' are not.
        if self.running.load(Ordering::SeqCst) == slot {
            self.cancel.cancel();
        }
        let _ = self.tx.send(Request::Regenerate { slot, revision, doc: Box::new(doc), fresh });
    }

    /// Forgets what a slot holds.
    pub fn drop_slot(&self, slot: u64) {
        let _ = self.tx.send(Request::Drop { slot });
    }

    /// Measures on the part last regenerated in slot 0; the answer arrives as
    /// [`Response::Measure`].
    pub fn measure(&self, request: u64, a: Entity, b: Option<Entity>) {
        let _ = self.tx.send(Request::Measure { request, a, b });
    }

    /// Exports the bodies of `doc` as STEP; the answer arrives as [`Response::Step`].
    pub fn export_step(&self, request: u64, doc: Document) {
        let _ = self.tx.send(Request::ExportStep { request, doc: Box::new(doc) });
    }

    /// Runs a job after the requests before it; the answer arrives as [`Response::Job`].
    pub fn job(&self, request: u64, job: Job) {
        let _ = self.tx.send(Request::Job { request, job });
    }

    /// The next response, if one is ready.
    pub fn try_recv(&self) -> Option<Response> {
        self.rx.try_recv().ok()
    }

    /// Blocks for the next response (tests and headless use).
    pub fn recv_timeout(&self, d: std::time::Duration) -> Option<Response> {
        self.rx.recv_timeout(d).ok()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.cancel();
        let _ = self.tx.send(Request::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

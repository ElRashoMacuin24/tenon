//! Regeneration off the UI thread. The worker owns the kernel; the UI sends document snapshots
//! and receives [`Scene`]s. A newer snapshot cancels the one in progress (between kernel
//! operations), and only the latest result is reported. On failure the UI keeps showing the last
//! good scene.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;

use tenon_kernel::{CancelToken, Kernel, MeshTol, ShapeHandle};

use crate::Document;
use crate::measure::{Entity, Measurement, measure};
use crate::regen::{Regen, RegenCache, Scene, regenerate, regenerate_with, scene};

enum Request {
    /// `fresh`: from scratch, not from the last checkpoint (Rebuild All).
    Regenerate {
        revision: u64,
        doc: Box<Document>,
        fresh: bool,
    },
    ExportStep {
        request: u64,
        doc: Box<Document>,
    },
    /// Measures on the part last regenerated.
    Measure {
        request: u64,
        a: Entity,
        b: Option<Entity>,
    },
    Shutdown,
}

/// What the worker reports.
#[derive(Debug)]
pub enum Response {
    /// The scene of document revision `revision` (feature errors are inside `scene.status`).
    Scene { revision: u64, scene: Box<Scene> },
    /// Regeneration of `revision` could not produce a scene at all.
    Failed { revision: u64, message: String },
    /// STEP data of the bodies of a document snapshot.
    Step { request: u64, result: Result<Vec<u8>, String> },
    /// A measurement on the part last regenerated.
    Measure { request: u64, result: Result<Measurement, String> },
}

/// Handle to the regeneration thread.
pub struct Worker {
    tx: Sender<Request>,
    rx: Receiver<Response>,
    cancel: CancelToken,
    handle: Option<JoinHandle<()>>,
}

/// Called (from the worker thread) whenever a response is ready, e.g. to wake the UI.
pub type Waker = Box<dyn Fn() + Send>;

fn bodies(r: &Regen) -> Vec<ShapeHandle> {
    r.bodies.iter().map(|b| b.shape).collect()
}

impl Worker {
    /// Starts the thread. `make_kernel` runs on it, so the kernel never crosses threads.
    pub fn spawn(make_kernel: impl FnOnce() -> Box<dyn Kernel> + Send + 'static, tol: MeshTol, waker: Option<Waker>) -> std::io::Result<Worker> {
        let (tx, worker_rx) = mpsc::channel::<Request>();
        let (worker_tx, rx) = mpsc::channel::<Response>();
        let cancel = CancelToken::new();
        let token = cancel.clone();
        let handle = std::thread::Builder::new().name("tenon-regen".into()).spawn(move || {
            let mut k = make_kernel();
            k.set_cancel(token.clone());
            let mut current = Regen::default();
            // Checkpoints between regenerations: an edit recomputes from the feature it changed.
            let mut cache = RegenCache::default();
            let send = |r: Response| {
                let _ = worker_tx.send(r);
                if let Some(w) = &waker {
                    w();
                }
            };
            while let Ok(mut req) = worker_rx.recv() {
                // Only the newest regeneration request matters.
                loop {
                    match worker_rx.try_recv() {
                        Ok(Request::Regenerate { revision, doc, fresh }) if matches!(req, Request::Regenerate { .. }) => {
                            // A pending Rebuild All survives being superseded.
                            let was = matches!(req, Request::Regenerate { fresh: true, .. });
                            req = Request::Regenerate { revision, doc, fresh: fresh || was };
                        }
                        Ok(next) => {
                            // Handle the pending one first, then this one.
                            Self::handle(&mut *k, &mut current, &mut cache, req, &tol, &token, &send);
                            req = next;
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => return,
                    }
                }
                if matches!(req, Request::Shutdown) {
                    break;
                }
                Self::handle(&mut *k, &mut current, &mut cache, req, &tol, &token, &send);
            }
            current.release(&mut *k);
            cache.release(&mut *k);
        })?;
        Ok(Worker { tx, rx, cancel, handle: Some(handle) })
    }

    fn handle(
        k: &mut dyn Kernel,
        current: &mut Regen,
        cache: &mut RegenCache,
        req: Request,
        tol: &MeshTol,
        token: &CancelToken,
        send: &dyn Fn(Response),
    ) {
        match req {
            Request::Regenerate { revision, doc, fresh } => {
                token.reset();
                current.release(k);
                if fresh {
                    cache.release(k);
                }
                let regen = regenerate_with(&doc, k, Some(cache));
                if regen.cancelled {
                    let mut r = regen;
                    r.release(k);
                    return;
                }
                *current = regen;
                match scene(current, k, tol) {
                    Ok(s) => send(Response::Scene { revision, scene: Box::new(s) }),
                    Err(message) => send(Response::Failed { revision, message }),
                }
            }
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
                send(Response::Measure { request, result: measure(k, current, a, b) });
            }
            Request::Shutdown => {}
        }
    }

    /// Regenerates `doc` (as revision `revision`), cancelling any regeneration in progress.
    /// With `fresh`, every feature is recomputed (Rebuild All); otherwise the regeneration
    /// resumes from the last checkpoint where the history before it is unchanged.
    pub fn regenerate(&self, revision: u64, doc: Document, fresh: bool) {
        self.cancel.cancel();
        let _ = self.tx.send(Request::Regenerate { revision, doc: Box::new(doc), fresh });
    }

    /// Measures on the part last regenerated; the answer arrives as [`Response::Measure`].
    pub fn measure(&self, request: u64, a: Entity, b: Option<Entity>) {
        let _ = self.tx.send(Request::Measure { request, a, b });
    }

    /// Exports the bodies of `doc` as STEP; the answer arrives as [`Response::Step`].
    pub fn export_step(&self, request: u64, doc: Document) {
        let _ = self.tx.send(Request::ExportStep { request, doc: Box::new(doc) });
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

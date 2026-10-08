//! Regeneration off the UI thread. The worker owns the kernel; the UI sends document snapshots
//! and receives [`Scene`]s. A newer snapshot cancels the one in progress (between kernel
//! operations), and only the latest result is reported. On failure the UI keeps showing the last
//! good scene.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;

use tenon_kernel::{CancelToken, Kernel, MeshTol, ShapeHandle};

use crate::Document;
use crate::regen::{Regen, Scene, regenerate, scene};

enum Request {
    Regenerate { revision: u64, doc: Box<Document> },
    ExportStep { request: u64, doc: Box<Document> },
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
                        Ok(next @ Request::Regenerate { .. }) if matches!(req, Request::Regenerate { .. }) => req = next,
                        Ok(next) => {
                            // Handle the pending one first, then this one.
                            Self::handle(&mut *k, &mut current, req, &tol, &token, &send);
                            req = next;
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => return,
                    }
                }
                if matches!(req, Request::Shutdown) {
                    break;
                }
                Self::handle(&mut *k, &mut current, req, &tol, &token, &send);
            }
            current.release(&mut *k);
        })?;
        Ok(Worker { tx, rx, cancel, handle: Some(handle) })
    }

    fn handle(k: &mut dyn Kernel, current: &mut Regen, req: Request, tol: &MeshTol, token: &CancelToken, send: &dyn Fn(Response)) {
        match req {
            Request::Regenerate { revision, doc } => {
                token.reset();
                current.release(k);
                let regen = regenerate(&doc, k);
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
            Request::Shutdown => {}
        }
    }

    /// Regenerates `doc` (as revision `revision`), cancelling any regeneration in progress.
    pub fn regenerate(&self, revision: u64, doc: Document) {
        self.cancel.cancel();
        let _ = self.tx.send(Request::Regenerate { revision, doc: Box::new(doc) });
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

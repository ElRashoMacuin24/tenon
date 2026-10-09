//! An open drawing: the document with undo, the models its views show (each a part or assembly
//! session, so a model can be edited from the drawing), and the computed views.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use tenon_assembly::AsmSession;
use tenon_kernel::Kernel;
use tenon_model::{CmdError, Session};

use crate::model::Drawing;
use crate::views::{Evaluation, ModelSource, evaluate};

/// Undo depth.
const MAX_UNDO: usize = 200;

/// A model a drawing shows.
pub enum DrwModel {
    Part(Box<Session>),
    Assembly(Box<AsmSession>),
    /// The file could not be read.
    Missing(String),
}

impl DrwModel {
    /// What the kernel builds it from.
    pub fn source(&self) -> Option<ModelSource> {
        match self {
            DrwModel::Part(s) => Some(ModelSource::Part(s.document().clone())),
            DrwModel::Assembly(a) => Some(ModelSource::Assembly {
                asm: a.assembly().clone(),
                parts: a.parts.iter().filter(|(_, p)| p.missing.is_none()).map(|(k, p)| (k.clone(), p.session.document().clone())).collect(),
            }),
            DrwModel::Missing(_) => None,
        }
    }
    /// Changes whenever the model does.
    pub fn revision(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        match self {
            DrwModel::Part(s) => s.revision().hash(&mut h),
            DrwModel::Assembly(a) => {
                a.revision().hash(&mut h);
                for p in a.parts.values() {
                    p.session.revision().hash(&mut h);
                }
            }
            DrwModel::Missing(m) => m.hash(&mut h),
        }
        h.finish()
    }
    pub fn is_dirty(&self) -> bool {
        match self {
            DrwModel::Part(s) => s.is_dirty(),
            DrwModel::Assembly(a) => a.is_dirty(),
            DrwModel::Missing(_) => false,
        }
    }
}

/// An open drawing.
pub struct DrwSession {
    drawing: Drawing,
    undo: Vec<Drawing>,
    redo: Vec<Drawing>,
    revision: u64,
    saved_revision: u64,
    /// Models by path (as in the views).
    pub models: BTreeMap<String, DrwModel>,
    /// Bumped when models are read again from their files.
    pub generation: u64,
    /// Per model: a fingerprint of its files on disk when it was last read or saved (see
    /// `tenon_io::drw::reload_changed`).
    pub stamps: BTreeMap<String, u64>,
    /// The computed views and what they were computed from.
    eval: Option<(u64, Evaluation)>,
}

impl Default for DrwSession {
    fn default() -> Self {
        DrwSession::new(Drawing::new("Drawing1", crate::model::Standard::Ansi))
    }
}

impl DrwSession {
    pub fn new(drawing: Drawing) -> DrwSession {
        DrwSession {
            drawing,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 1,
            saved_revision: 1,
            models: BTreeMap::new(),
            generation: 0,
            stamps: BTreeMap::new(),
            eval: None,
        }
    }
    pub fn drawing(&self) -> &Drawing {
        &self.drawing
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }
    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }

    /// Replaces the drawing and its models (after opening a file); undo history is cleared.
    pub fn replace(&mut self, drawing: Drawing, models: BTreeMap<String, DrwModel>) {
        self.drawing = drawing;
        self.models = models;
        self.stamps.clear();
        self.undo.clear();
        self.redo.clear();
        self.revision += 1;
        self.saved_revision = self.revision;
        self.generation += 1;
        self.eval = None;
    }

    /// Applies `f` as one undoable step; on error nothing changes.
    pub fn edit<T>(&mut self, f: impl FnOnce(&mut Drawing) -> Result<T, CmdError>) -> Result<T, CmdError> {
        let before = self.drawing.clone();
        match f(&mut self.drawing).and_then(|v| self.drawing.validate().map(|()| v).map_err(CmdError)) {
            Ok(v) => {
                if self.drawing != before {
                    self.undo.push(before);
                    if self.undo.len() > MAX_UNDO {
                        self.undo.remove(0);
                    }
                    self.redo.clear();
                    self.revision += 1;
                }
                Ok(v)
            }
            Err(e) => {
                self.drawing = before;
                Err(e)
            }
        }
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(d) => {
                self.redo.push(std::mem::replace(&mut self.drawing, d));
                self.revision += 1;
                true
            }
            None => false,
        }
    }
    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(d) => {
                self.undo.push(std::mem::replace(&mut self.drawing, d));
                self.revision += 1;
                true
            }
            None => false,
        }
    }

    /// The model sources the kernel needs (models that could be read).
    pub fn sources(&self) -> BTreeMap<String, ModelSource> {
        self.models.iter().filter_map(|(k, m)| m.source().map(|s| (k.clone(), s))).collect()
    }

    /// What the views' geometry depends on: the views' kinds, models, scales and line options,
    /// and every model's revision. Moving a view or adding a dimension does not change it.
    pub fn eval_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.generation.hash(&mut h);
        format!("{:?}", self.drawing.standard).hash(&mut h);
        for v in &self.drawing.views {
            (v.id, &v.model, serde_json::to_string(&v.kind).unwrap_or_default(), v.scale.to_bits(), v.hidden, v.tangent).hash(&mut h);
        }
        for (k, m) in &self.models {
            (k, m.revision()).hash(&mut h);
        }
        h.finish()
    }

    /// The computed views, if they are current.
    pub fn evaluation(&self) -> Option<&Evaluation> {
        self.eval.as_ref().filter(|(k, _)| *k == self.eval_key()).map(|(_, e)| e)
    }
    /// The last computed views, current or not (what the screen keeps showing while new ones are
    /// computed).
    pub fn last_evaluation(&self) -> Option<&Evaluation> {
        self.eval.as_ref().map(|(_, e)| e)
    }

    /// Sets computed views (from a worker) made for `key`.
    pub fn set_evaluation(&mut self, key: u64, ev: Evaluation) {
        self.eval = Some((key, ev));
    }

    /// Computes the views with `k` if they are out of date.
    pub fn refresh(&mut self, k: &mut dyn Kernel) {
        let key = self.eval_key();
        if self.eval.as_ref().is_some_and(|(e, _)| *e == key) {
            return;
        }
        let mut ev = evaluate(k, &self.sources(), &self.drawing);
        self.explain_missing(&mut ev);
        self.eval = Some((key, ev));
    }

    /// Puts why each unreadable model file could not be read on it and its views.
    pub fn explain_missing(&self, ev: &mut Evaluation) {
        for (key, m) in &self.models {
            let DrwModel::Missing(why) = m else { continue };
            let msg = format!("{} could not be read: {why}", crate::views::file_name(key));
            if let Some(g) = ev.models.get_mut(key) {
                g.error = Some(msg.clone());
            }
            for v in self.drawing.views.iter().filter(|v| &v.model == key) {
                if let Some(g) = ev.views.get_mut(&v.id) {
                    g.error = Some(msg.clone());
                }
            }
        }
    }

    /// The current views, or an error saying they need a kernel.
    pub fn current(&self) -> Result<&Evaluation, CmdError> {
        self.evaluation().ok_or_else(|| CmdError("the views are not computed yet".into()))
    }
}

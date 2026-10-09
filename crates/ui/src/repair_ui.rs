//! Repairing a broken reference from the viewport (DEC-034): when a feature fails because a face
//! or edge it uses is gone, the failure banner offers Repair. Repair highlights the nearest faces
//! or edges, and one click on any face or edge of the right sort puts it in, as one undoable edit.
//! The browser's context menu offers the same for a failing feature.

use egui::{Color32, Rect, Ui, pos2, vec2};
use serde_json::json;
use tenon_model::repair::{self, Broken, RefKind};
use tenon_model::{FaceRef, FeatureId, FeatureStatus, Fingerprint};
use tenon_render::pick::{pick_edge, pick_face};

use crate::Workbench;
use crate::viewport::Pick;

/// The colour of the faces or edges offered as replacements.
pub(crate) const CANDIDATE: Color32 = Color32::from_rgb(0x5a, 0xd0, 0x6e);

/// A reference being repaired.
pub(crate) struct RepairMode {
    pub broken: Broken,
}

impl Workbench {
    /// The lost references of `feature`, or of the feature that failed.
    pub(crate) fn broken_references(&self, feature: Option<FeatureId>) -> Vec<Broken> {
        let failed = || self.scene.status.iter().find_map(|(f, s)| matches!(s, FeatureStatus::Error { .. }).then_some(*f));
        feature.or_else(failed).map(|f| repair::broken(self.document(), &self.scene, f)).unwrap_or_default()
    }

    /// The failure banner's Repair button, when what failed is a lost reference.
    pub(crate) fn repair_button(&mut self, ui: &mut Ui, banner: Rect) {
        if self.repair.is_some() || self.in_assembly() || self.in_drawing() || self.is_sketching() || self.broken_references(None).is_empty() {
            return;
        }
        let r = Rect::from_min_size(pos2(banner.right() + 6.0, banner.top()), vec2(72.0, banner.height()));
        let clicked = ui.put(r, egui::Button::new("Repair")).clicked();
        crate::drawing::remember(ui, "tn_repair", r);
        if clicked {
            self.start_repair(None);
        }
    }

    /// Starts repairing the first lost reference of `feature` (or of the feature that failed).
    pub(crate) fn start_repair(&mut self, feature: Option<FeatureId>) {
        let mut broken = self.broken_references(feature);
        if broken.is_empty() {
            self.set_status("Nothing to repair: that feature finds every face and edge it uses.");
            return;
        }
        let b = broken.remove(0);
        let (what, sort) = match b.kind {
            RefKind::Edge => ("an edge", "edge"),
            RefKind::Face => ("a face", "face"),
        };
        self.set_status(format!(
            "{} uses {what} that no longer exists. Click the {sort} to use instead (the nearest are highlighted), or Esc.",
            self.feature_name(b.feature)
        ));
        self.view.selection.clear();
        self.repair = Some(RepairMode { broken: b });
    }

    /// The faces or edges offered as replacements, to highlight.
    pub(crate) fn repair_candidates(&self) -> Vec<Pick> {
        let Some(m) = &self.repair else { return Vec::new() };
        m.broken
            .candidates
            .iter()
            .map(|c| match m.broken.kind {
                RefKind::Edge => Pick::Edge { body: c.body, edge: c.index },
                RefKind::Face => Pick::Face { body: c.body, face: c.index },
            })
            .collect()
    }

    /// While repairing: hovering shows what a click would take (edges or faces only), a click puts
    /// it in, Esc stops. Returns whether the pointer was taken.
    pub(crate) fn repair_pointer(&mut self, ui: &Ui, resp: &egui::Response, rect: Rect) -> bool {
        let Some(m) = &self.repair else { return false };
        let (feature, path, kind) = (m.broken.feature, m.broken.path.clone(), m.broken.kind);
        // Undone, repaired elsewhere, or the document replaced: nothing left to repair here.
        if !self.broken_references(Some(feature)).iter().any(|b| b.path == path) {
            self.repair = None;
            return false;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.repair = None;
            self.view.hover = None;
            self.set_status("Repair stopped.");
            return true;
        }
        let meshes: Vec<&tenon_kernel::Mesh> = self.scene.bodies.iter().map(|b| &b.mesh).collect();
        self.view.hover = resp.hover_pos().and_then(|p| {
            let (x, y, w, h) = (f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
            match kind {
                RefKind::Edge => pick_edge(&meshes, &self.view.camera, w, h, x, y, 5.0).map(|e| Pick::Edge { body: e.body, edge: e.edge }),
                RefKind::Face => {
                    let (o, d) = self.view.camera.ray(x, y, w, h);
                    pick_face(&meshes, o, d).map(|f| Pick::Face { body: f.body, face: f.face })
                }
            }
        });
        if !resp.clicked() {
            return true;
        }
        let Some(p) = self.view.hover else { return true };
        let reference = match p {
            Pick::Edge { body, edge } => self.scene.bodies.get(body).and_then(|b| b.edge_ref(edge)).and_then(|r| serde_json::to_value(r).ok()),
            Pick::Face { body, face } => self
                .scene
                .bodies
                .get(body)
                .and_then(|b| b.faces.get(face as usize))
                .and_then(|(name, info)| Some(FaceRef { origin: (*name)?, fingerprint: Fingerprint::of(info) }))
                .and_then(|r| serde_json::to_value(r).ok()),
        };
        let Some(with) = reference else {
            self.set_error("That one cannot be referenced yet: pick another.");
            return true;
        };
        let name = self.feature_name(feature);
        match self.exec("model.repair", json!({ "feature": feature.0, "path": path, "with": with })) {
            Ok(_) => {
                self.repair = None;
                self.view.hover = None;
                self.set_status(format!("{name} now uses the one you picked."));
            }
            Err(e) => self.set_error(e),
        }
        true
    }
}

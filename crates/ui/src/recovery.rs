//! Autosave and crash recovery (DEC-033): while documents have unsaved changes, copies are kept in
//! a folder of the app's own, never beside the user's files. Each running Tenon holds a lock on
//! its folder. A folder whose lock is free belongs to a Tenon that did not close properly, and its
//! copies are offered back when Tenon next starts.
//!
//! A folder holds `lock`, `recovery.json` (what each copy is) and the copies, in the current file
//! format.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// Seconds between autosaves while something has changed.
pub(crate) const AUTOSAVE_SECONDS: f64 = 30.0;
const MANIFEST: &str = "recovery.json";
const LOCK: &str = "lock";

/// One document kept for recovery.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Kept {
    /// "part", "assembly" or "drawing".
    pub kind: String,
    /// The document that was open (not a part or model it uses).
    pub top: bool,
    /// Changed since it was last saved (the open document is kept, unchanged, to know what to
    /// open again).
    pub changed: bool,
    /// The file it came from (its full path), if it was ever saved.
    pub original: Option<String>,
    /// For a part inside an assembly a drawing shows: that assembly (its full path). A file can
    /// be used in more than one place; the copy goes back where it was changed.
    pub within: Option<String>,
    /// The copy's file name in the folder.
    pub file: String,
}

/// What a folder holds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Manifest {
    /// When the copies were written (seconds since 1970).
    pub saved: u64,
    pub documents: Vec<Kept>,
}

impl Manifest {
    fn to_json(&self) -> Value {
        let docs: Vec<Value> = self
            .documents
            .iter()
            .map(|d| json!({ "kind": d.kind, "top": d.top, "changed": d.changed, "original": d.original, "within": d.within, "file": d.file }))
            .collect();
        json!({ "format": "tenon-recovery", "version": 1, "saved": self.saved, "documents": docs })
    }

    fn from_json(v: &Value) -> Option<Manifest> {
        if v["format"] != "tenon-recovery" || v["version"] != 1 {
            return None;
        }
        let mut documents = Vec::new();
        for d in v["documents"].as_array()? {
            let file = d["file"].as_str()?.to_owned();
            // Only plain file names: a damaged manifest must not point outside its folder.
            if file.is_empty() || file.contains(['/', '\\']) || file.starts_with('.') {
                return None;
            }
            documents.push(Kept {
                kind: d["kind"].as_str()?.to_owned(),
                top: d["top"].as_bool()?,
                changed: d["changed"].as_bool()?,
                original: d["original"].as_str().map(str::to_owned),
                within: d["within"].as_str().map(str::to_owned),
                file,
            });
        }
        (!documents.is_empty() && documents.iter().filter(|d| d.top).count() == 1).then_some(Manifest { saved: v["saved"].as_u64()?, documents })
    }

    /// The open document.
    pub fn top(&self) -> Option<&Kept> {
        self.documents.iter().find(|d| d.top)
    }
}

/// This Tenon's recovery folder, locked while it runs.
pub(crate) struct Recovery {
    pub dir: PathBuf,
    /// Held for as long as this Tenon runs: its folder is not offered to another one.
    _lock: File,
    /// What was last written (a hash of the copies), so unchanged work is not written again.
    written: Option<u64>,
    /// UI time of the last check.
    pub checked_at: f64,
}

impl Recovery {
    /// A new folder under `base`, locked.
    pub fn start(base: &Path) -> std::io::Result<Recovery> {
        let id = format!("{}-{}", std::process::id(), now_nanos());
        let dir = base.join(id);
        std::fs::create_dir_all(&dir)?;
        let lock = File::create(dir.join(LOCK))?;
        lock.try_lock().map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(Recovery { dir, _lock: lock, written: None, checked_at: 0.0 })
    }

    /// Keeps `docs` (what each is, and its bytes): writes them unless they are what was written
    /// last. Nothing to keep clears the folder.
    pub fn keep(&mut self, docs: Vec<(Kept, Vec<u8>)>) -> std::io::Result<bool> {
        if docs.is_empty() {
            self.clear()?;
            return Ok(false);
        }
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for (k, b) in &docs {
                (&k.kind, k.top, k.changed, &k.original, &k.file, b).hash(&mut h);
            }
            h.finish()
        };
        if self.written == Some(hash) {
            return Ok(false);
        }
        // The copies first, then the manifest that names them (each written whole, then renamed):
        // a crash in the middle leaves the previous manifest pointing at whole files.
        for (k, b) in &docs {
            write_whole(&self.dir.join(&k.file), b)?;
        }
        let manifest = Manifest { saved: now_secs(), documents: docs.iter().map(|(k, _)| k.clone()).collect() };
        write_whole(&self.dir.join(MANIFEST), manifest.to_json().to_string().as_bytes())?;
        // Copies no longer named go.
        for e in std::fs::read_dir(&self.dir)?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name != LOCK && name != MANIFEST && !manifest.documents.iter().any(|d| d.file == name) {
                let _ = std::fs::remove_file(e.path());
            }
        }
        self.written = Some(hash);
        Ok(true)
    }

    /// Nothing unsaved: removes the copies and the manifest (the lock stays).
    pub fn clear(&mut self) -> std::io::Result<()> {
        if self.written.take().is_some() || self.dir.join(MANIFEST).exists() {
            for e in std::fs::read_dir(&self.dir)?.flatten() {
                if e.file_name() != LOCK {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        Ok(())
    }

    /// Tenon is closing properly: the folder goes.
    pub fn finish(self) {
        let dir = self.dir.clone();
        drop(self);
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Folders under `base` left by Tenons that did not close properly (their lock is free) and that
/// hold copies, newest first. Empty or damaged ones are removed.
pub(crate) fn orphans(base: &Path) -> Vec<(PathBuf, Manifest)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(base).into_iter().flatten().flatten() {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        // No lock file yet: a Tenon just starting. A lock someone holds: a Tenon still running.
        let Ok(lock) = File::options().write(true).open(dir.join(LOCK)) else { continue };
        if lock.try_lock().is_err() {
            continue;
        }
        drop(lock);
        match read_manifest(&dir) {
            Some(m) if m.documents.iter().all(|d| dir.join(&d.file).is_file()) => out.push((dir, m)),
            _ => {
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
    }
    out.sort_by_key(|o| std::cmp::Reverse(o.1.saved));
    out
}

fn read_manifest(dir: &Path) -> Option<Manifest> {
    let text = std::fs::read_to_string(dir.join(MANIFEST)).ok()?;
    Manifest::from_json(&serde_json::from_str(&text).ok()?)
}

/// Writes `bytes` to a temporary file beside `path`, then renames it into place.
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn now_nanos() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos())
}

/// "2 minutes ago", for the recovery prompt.
pub(crate) fn ago(saved: u64) -> String {
    let s = now_secs().saturating_sub(saved);
    match s {
        0..=59 => "less than a minute ago".into(),
        60..=119 => "a minute ago".into(),
        120..=3599 => format!("{} minutes ago", s / 60),
        3600..=7199 => "an hour ago".into(),
        7200..=86_399 => format!("{} hours ago", s / 3600),
        86_400..=172_799 => "yesterday".into(),
        _ => format!("{} days ago", s / 86_400),
    }
}

// ---- the workbench's side ---------------------------------------------------------------------

/// Work offered back, and whether the prompt has taken the keyboard yet.
pub(crate) struct Offer {
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub focused: bool,
}

impl crate::Workbench {
    /// Keeps unsaved work under `base` against a crash, and offers back work a Tenon that did not
    /// close properly left there.
    pub fn enable_recovery(&mut self, base: &Path) -> Result<(), String> {
        let found = orphans(base);
        self.recovery = Some(Recovery::start(base).map_err(|e| format!("cannot keep unsaved work in {}: {e}", base.display()))?);
        self.recover_offer = found.into_iter().next().map(|(dir, manifest)| Offer { dir, manifest, focused: false });
        Ok(())
    }

    /// Tenon is closing properly: the copies of unsaved work go (the user saved it or chose not to).
    pub fn shutdown(&mut self) {
        if let Some(r) = self.recovery.take() {
            r.finish();
        }
    }

    /// Every so often, keeps copies of what has changed.
    pub(crate) fn autosave(&mut self) {
        let now = self.view.now;
        let Some(r) = self.recovery.as_mut() else { return };
        if now - r.checked_at < self.autosave_seconds && now >= r.checked_at {
            return;
        }
        r.checked_at = now;
        let dir = r.dir.clone();
        let docs = self.with_parts_home(|wb| wb.with_model_home(|wb| wb.unsaved_documents(&dir)));
        if let Some(r) = self.recovery.as_mut()
            && let Err(e) = r.keep(docs)
        {
            self.set_error(format!("Unsaved work could not be kept against a crash: {e}"));
        }
    }

    /// The documents with unsaved changes, as copies for `dir`: the open one first (kept even
    /// when unchanged, to know what to open again), then the parts and models it uses.
    fn unsaved_documents(&self, dir: &Path) -> Vec<(Kept, Vec<u8>)> {
        let mut n = 0;
        let mut kept = |kind: &str, top: bool, changed: bool, original: Option<String>, ext: &str| {
            n += 1;
            Kept { kind: kind.into(), top, changed, original, within: None, file: format!("{n}.{ext}") }
        };
        let full = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().into_owned();
        let part = |s: &tenon_model::Session| tenon_io::project::to_bytes(s.document(), &serde_json::Map::new()).ok();
        let mut used = Vec::new();
        // The changed parts of an assembly: the open one (`within` None) or one a drawing shows.
        let asm_parts = |a: &tenon_assembly::AsmSession,
                         within: Option<&String>,
                         used: &mut Vec<(Kept, Vec<u8>)>,
                         kept: &mut dyn FnMut(&str, bool, bool, Option<String>, &str) -> Kept| {
            for (key, p) in &a.parts {
                if p.missing.is_none()
                    && p.session.is_dirty()
                    && let Some(b) = part(&p.session)
                {
                    let mut k = kept("part", false, true, Some(key.clone()), "tenon");
                    k.within = within.cloned();
                    used.push((k, b));
                }
            }
        };
        let top = if let Some(d) = &self.drw {
            for (key, m) in &d.session.models {
                match m {
                    tenon_drawing::DrwModel::Part(s) if s.is_dirty() => {
                        if let Some(b) = part(s) {
                            used.push((kept("part", false, true, Some(key.clone()), "tenon"), b));
                        }
                    }
                    tenon_drawing::DrwModel::Assembly(a) => {
                        if a.is_dirty()
                            && let Ok(b) = tenon_io::asm::to_bytes(a.assembly(), dir)
                        {
                            used.push((kept("assembly", false, true, Some(key.clone()), "tenonasm"), b));
                        }
                        asm_parts(a, Some(key), &mut used, &mut kept);
                    }
                    _ => {}
                }
            }
            let changed = d.session.is_dirty();
            tenon_io::drw::to_bytes(d.session.drawing(), dir)
                .ok()
                .map(|b| (kept("drawing", true, changed, d.path.as_deref().map(full), "tenondrw"), b))
        } else if let Some(a) = &self.asm {
            asm_parts(&a.session, None, &mut used, &mut kept);
            let changed = a.session.is_dirty();
            tenon_io::asm::to_bytes(a.session.assembly(), dir)
                .ok()
                .map(|b| (kept("assembly", true, changed, a.path.as_deref().map(full), "tenonasm"), b))
        } else {
            let changed = self.session.is_dirty();
            part(&self.session).map(|b| (kept("part", true, changed, self.path.as_deref().map(full), "tenon"), b))
        };
        match top {
            Some(t) if t.0.changed || !used.is_empty() => std::iter::once(t).chain(used).collect(),
            _ => Vec::new(),
        }
    }

    /// Opens the work in a folder again: the open document from its file (or new, if it was never
    /// saved) with the kept changes applied as one edit each, so they show as unsaved and Undo
    /// goes back to the saved file. The folder goes afterwards.
    pub(crate) fn recover(&mut self, dir: &Path, m: &Manifest) -> Result<String, String> {
        let top = m.top().ok_or("nothing to recover")?;
        let read = |k: &Kept| std::fs::read(dir.join(&k.file)).map_err(|e| format!("cannot read the copy of {}: {e}", name(k)));
        let on_disk = top.original.as_deref().map(Path::new).filter(|p| p.is_file());
        match top.kind.as_str() {
            "assembly" => {
                match on_disk {
                    Some(p) => self.open_assembly(p)?,
                    None => self.new_assembly(),
                }
                if top.changed {
                    let asm = tenon_io::asm::from_bytes(&read(top)?, dir).map_err(|e| e.to_string())?;
                    let a = self.asm.as_mut().ok_or("no assembly")?;
                    put_assembly(&mut a.session, asm)?;
                    if top.original.is_some() {
                        a.path = top.original.as_deref().map(PathBuf::from);
                    }
                }
            }
            "drawing" => {
                match on_disk {
                    Some(p) => self.open_drawing(p)?,
                    None => self.new_drawing(),
                }
                if top.changed {
                    let d = tenon_io::drw::from_bytes(&read(top)?, dir).map_err(|e| e.to_string())?;
                    let doc = self.drw.as_mut().ok_or("no drawing")?;
                    for key in d.models() {
                        doc.session.models.entry(key.clone()).or_insert_with(|| tenon_io::drw::load_model(Path::new(&key)));
                    }
                    doc.session
                        .edit(|x| {
                            *x = d;
                            Ok(())
                        })
                        .map_err(|e| e.0)?;
                    if top.original.is_some() {
                        doc.path = top.original.as_deref().map(PathBuf::from);
                    }
                }
            }
            _ => {
                if let Some(p) = on_disk {
                    self.open(p)?;
                }
                if top.changed {
                    let (doc, _) = tenon_io::project::from_bytes(&read(top)?).map_err(|e| e.to_string())?;
                    self.session
                        .edit(|d| {
                            *d = doc;
                            Ok(())
                        })
                        .map_err(|e| e.0)?;
                    self.path = top.original.as_deref().map(PathBuf::from);
                }
            }
        }
        // The parts and models it uses, each where the open document holds it.
        let mut missed = Vec::new();
        for k in m.documents.iter().filter(|d| !d.top) {
            let Some(key) = k.original.clone() else { continue };
            let bytes = read(k)?;
            let done = match k.kind.as_str() {
                "assembly" => {
                    let asm = tenon_io::asm::from_bytes(&bytes, dir).map_err(|e| e.to_string())?;
                    match self.drw.as_mut().and_then(|d| d.session.models.get_mut(&key)) {
                        Some(tenon_drawing::DrwModel::Assembly(a)) => put_assembly(a, asm).is_ok(),
                        _ => false,
                    }
                }
                _ => {
                    let (doc, _) = tenon_io::project::from_bytes(&bytes).map_err(|e| e.to_string())?;
                    match self.used_part(&key, k.within.as_deref()) {
                        Some(s) => s
                            .edit(|d| {
                                *d = doc;
                                Ok(())
                            })
                            .is_ok(),
                        None => false,
                    }
                }
            };
            if !done {
                missed.push(name(k));
            }
        }
        let _ = std::fs::remove_dir_all(dir);
        let what = name(top);
        let used = m.documents.len() - 1 - missed.len();
        let mut msg = match used {
            0 => format!("Recovered {what} as it was {}.", ago(m.saved)),
            n => format!("Recovered {what} and {n} file(s) it uses, as they were {}.", ago(m.saved)),
        };
        msg.push_str(" Save to keep the changes; Undo goes back to the saved file.");
        if !missed.is_empty() {
            msg.push_str(&format!(" Not recovered (no longer used): {}.", missed.join(", ")));
        }
        Ok(msg)
    }

    /// A part the open assembly or drawing uses, by key: in a drawing, one it shows itself, or
    /// one inside the assembly `within` that it shows.
    fn used_part(&mut self, key: &str, within: Option<&str>) -> Option<&mut tenon_model::Session> {
        if let Some(d) = self.drw.as_mut() {
            return match (within, d.session.models.get_mut(within.unwrap_or(key))) {
                (Some(_), Some(tenon_drawing::DrwModel::Assembly(a))) => a.parts.get_mut(key).map(|p| &mut p.session),
                (None, Some(tenon_drawing::DrwModel::Part(s))) => Some(&mut **s),
                _ => None,
            };
        }
        self.asm.as_mut().and_then(|a| a.session.parts.get_mut(key)).map(|p| &mut p.session)
    }

    /// "Recover unsaved work?": Recover (the default), Discard or Not Now (Esc).
    pub(crate) fn recover_prompt(&mut self, ui: &egui::Ui) {
        let Some(o) = self.recover_offer.as_mut() else { return };
        let focus = !std::mem::replace(&mut o.focused, true);
        let (top, used, when) = (o.manifest.top().map(name).unwrap_or_default(), o.manifest.documents.len() - 1, ago(o.manifest.saved));
        let mut answer = None;
        egui::Modal::new(egui::Id::new("tn_recover_prompt")).show(ui.ctx(), |ui| {
            ui.set_width(380.0);
            ui.label(egui::RichText::new("Recover unsaved work?").font(crate::theme::body()).strong());
            ui.add_space(4.0);
            let files = if used > 0 { format!("{top} and {used} file(s) it uses") } else { top.clone() };
            ui.label(
                egui::RichText::new(format!("Tenon closed without saving changes to {files}. They were kept {when}.")).font(crate::theme::small()),
            );
            ui.add_space(12.0);
            let size = egui::vec2(88.0, 24.0);
            ui.horizontal(|ui| {
                ui.add_space(ui.available_width() - 3.0 * size.x - 2.0 * ui.spacing().item_spacing.x);
                for (label, key, a) in
                    [("Recover", "tn_recover_recover", 0), ("Discard", "tn_recover_discard", 1), ("Not Now", "tn_recover_later", 2)]
                {
                    let r = ui.add_sized(size, egui::Button::new(label));
                    crate::drawing::remember(ui, key, r.rect);
                    if focus && a == 0 {
                        r.request_focus();
                    }
                    if r.clicked() {
                        answer = Some(a);
                    }
                }
            });
        });
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            answer = Some(2);
        }
        let Some(a) = answer else { return };
        let Some(o) = self.recover_offer.take() else { return };
        match a {
            0 => match self.recover(&o.dir, &o.manifest) {
                Ok(msg) => self.set_status(msg),
                // The copies stay for another try.
                Err(e) => self.set_error(format!("Could not recover the unsaved work: {e}")),
            },
            1 => {
                let _ = std::fs::remove_dir_all(&o.dir);
                self.set_status("The unsaved work was discarded.");
            }
            _ => self.set_status("The unsaved work is kept: Tenon offers it again next time it starts."),
        }
    }
}

/// Replaces an assembly with a kept copy as one edit, first reading the part files it uses that
/// are not open.
fn put_assembly(s: &mut tenon_assembly::AsmSession, asm: tenon_assembly::Assembly) -> Result<(), String> {
    for c in &asm.components {
        if !s.parts.contains_key(&c.part) {
            let part = match tenon_io::project::open(Path::new(&c.part)) {
                Ok((doc, _)) => {
                    let mut p = tenon_assembly::Part::new(tenon_model::Document::default());
                    p.session.replace_document(doc, None);
                    p
                }
                Err(e) => tenon_assembly::Part::missing(e.to_string()),
            };
            s.add_part(&c.part, part);
        }
    }
    s.edit(|a, _| {
        *a = asm;
        Ok(())
    })
    .map_err(|e| e.0)
}

/// A kept document's file name, for messages.
fn name(k: &Kept) -> String {
    match &k.original {
        Some(o) => Path::new(o).file_name().map_or_else(|| o.clone(), |n| n.to_string_lossy().into_owned()),
        None => format!("a new {}", k.kind),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn base(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tenon-recovery-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn part(changed: bool) -> (Kept, Vec<u8>) {
        (
            Kept { kind: "part".into(), top: true, changed, original: Some("C:/x/plate.tenon".into()), within: None, file: "1.tenon".into() },
            b"format = 1".to_vec(),
        )
    }

    #[test]
    fn a_running_tenons_copies_are_not_offered_and_a_crashed_ones_are() {
        let base = base("offer");
        let mut r = Recovery::start(&base).unwrap();
        assert!(r.keep(vec![part(true)]).unwrap());
        assert!(!r.keep(vec![part(true)]).unwrap(), "unchanged work is not written again");
        // Still running (its lock held): not offered.
        assert!(orphans(&base).is_empty());
        // Gone without finishing (as in a crash, the lock goes with the process): offered.
        let dir = r.dir.clone();
        drop(r);
        let found = orphans(&base);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, dir);
        assert_eq!(found[0].1.documents, vec![part(true).0]);
        assert_eq!(std::fs::read(dir.join("1.tenon")).unwrap(), b"format = 1");
        // A Tenon that closes properly leaves nothing.
        let r = Recovery::start(&base).unwrap();
        let mine = r.dir.clone();
        r.finish();
        assert!(!mine.exists());
    }

    #[test]
    fn saved_work_clears_the_copies_and_damaged_folders_are_dropped() {
        let base = base("clear");
        let mut r = Recovery::start(&base).unwrap();
        r.keep(vec![part(true)]).unwrap();
        r.keep(Vec::new()).unwrap();
        let names: Vec<String> = std::fs::read_dir(&r.dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["lock"]);
        drop(r);
        assert!(orphans(&base).is_empty(), "nothing to offer");
        // A crashed Tenon's manifest naming a copy outside its folder, or one that is not there,
        // is not offered. (A folder without its lock file is a Tenon just starting: left alone.)
        for bad in [
            r#"{"format":"tenon-recovery","version":1,"saved":1,"documents":[{"kind":"part","top":true,"changed":true,"original":null,"file":"../x.tenon"}]}"#,
            r#"{"format":"tenon-recovery","version":1,"saved":1,"documents":[{"kind":"part","top":true,"changed":true,"original":null,"file":"gone.tenon"}]}"#,
            "not json",
        ] {
            let dir = base.join("damaged");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(LOCK), b"").unwrap();
            std::fs::write(dir.join(MANIFEST), bad).unwrap();
            assert!(orphans(&base).is_empty(), "{bad}");
            assert!(!dir.exists(), "damaged folders are removed");
        }
    }

    #[test]
    fn times_read_as_people_say_them() {
        let now = now_secs();
        assert_eq!(ago(now), "less than a minute ago");
        assert_eq!(ago(now - 300), "5 minutes ago");
        assert_eq!(ago(now - 3 * 3600), "3 hours ago");
        assert_eq!(ago(now - 3 * 86_400), "3 days ago");
    }
}

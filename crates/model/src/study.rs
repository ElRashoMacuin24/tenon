//! Stress studies (M11): where a part is held and what pushes on it.
//!
//! A study is the question "does this part hold?": faces that are held still (supports), and
//! forces and pressures on other faces (loads). It is kept with the part, refers to faces by
//! their persistent names like any feature does, and is edited by commands (`study.*`) with
//! undo. Working out the answer (dividing the part into elements, solving, showing stress) is
//! not in this module.
//!
//! Units are the part's: millimetres, newtons, and so megapascals (N/mm^2) for pressure and
//! stress.

use serde::{Deserialize, Serialize};
use tenon_geom::Vec3;

use crate::naming::FaceRef;

/// The most studies a part has, and the most supports or loads a study has (hostile-input
/// caps).
pub const MAX_STUDIES: usize = 100;
pub const MAX_ITEMS: usize = 1000;
/// The most faces one support or load is on.
pub const MAX_FACES: usize = 10_000;
/// The largest force (N) and pressure (MPa) taken: beyond any part a study is for.
pub const MAX_FORCE: f64 = 1.0e12;
pub const MAX_PRESSURE: f64 = 1.0e6;

/// A study's id within its part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StudyId(pub u32);

impl std::fmt::Display for StudyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "study {}", self.0)
    }
}

/// How faces are held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportKind {
    /// Held still in every direction, as if welded to something that does not move.
    Fixed,
}

/// Faces of the part that are held.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Support {
    pub id: u32,
    pub name: String,
    pub kind: SupportKind,
    pub faces: Vec<FaceRef>,
}

/// What pushes on faces.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LoadKind {
    /// A force of `newtons` in all, spread evenly over the faces. Along `direction` (in the
    /// part's coordinates; its length does not matter), or without one square to each face,
    /// pushing into it.
    Force {
        newtons: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<Vec3>,
    },
    /// A pressure of `megapascals` on the faces, square to them, pushing into them (a negative
    /// one pulls).
    Pressure { megapascals: f64 },
}

/// A load on faces of the part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Load {
    pub id: u32,
    pub name: String,
    pub faces: Vec<FaceRef>,
    #[serde(flatten)]
    pub kind: LoadKind,
}

/// A stress study of a part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Study {
    pub id: StudyId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supports: Vec<Support>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loads: Vec<Load>,
    /// How large the elements are that the part is divided into (mm). None: chosen from the
    /// part's size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_size: Option<f64>,
}

fn name_ok(name: &str) -> Result<(), String> {
    let n = name.trim().chars().count();
    if n == 0 || n > 64 { Err("a name needs 1 to 64 characters".into()) } else { Ok(()) }
}

fn faces_ok(faces: &[FaceRef], of: &str) -> Result<(), String> {
    if faces.is_empty() {
        return Err(format!("{of} needs at least one face"));
    }
    if faces.len() > MAX_FACES {
        return Err(format!("{of} is on too many faces (at most {MAX_FACES})"));
    }
    Ok(())
}

impl LoadKind {
    pub fn check(&self) -> Result<(), String> {
        match self {
            LoadKind::Force { newtons, direction } => {
                if !(newtons.is_finite() && newtons.abs() > 0.0 && newtons.abs() <= MAX_FORCE) {
                    return Err(format!("a force must be a number of newtons other than 0, at most {MAX_FORCE:e}"));
                }
                if direction.is_some_and(|d| !(d.is_finite() && d.len() > tenon_geom::tol::LINEAR)) {
                    return Err("a force's direction has no direction: give three numbers that are not all 0".into());
                }
                Ok(())
            }
            LoadKind::Pressure { megapascals } => {
                if megapascals.is_finite() && megapascals.abs() > 0.0 && megapascals.abs() <= MAX_PRESSURE {
                    Ok(())
                } else {
                    Err(format!("a pressure must be a number of megapascals other than 0, at most {MAX_PRESSURE:e}"))
                }
            }
        }
    }

    /// "Force" or "Pressure": what its name starts with.
    pub fn word(&self) -> &'static str {
        match self {
            LoadKind::Force { .. } => "Force",
            LoadKind::Pressure { .. } => "Pressure",
        }
    }
}

impl Study {
    pub fn new(id: StudyId, name: String) -> Study {
        Study { id, name, supports: Vec::new(), loads: Vec::new(), mesh_size: None }
    }

    /// Whether the study is whole in itself: names, sizes, ids that are not repeated. (Whether
    /// its faces are still on the part is for whoever solves it.)
    pub fn check(&self) -> Result<(), String> {
        name_ok(&self.name).map_err(|e| format!("{}: {e}", self.id))?;
        if self.supports.len() > MAX_ITEMS || self.loads.len() > MAX_ITEMS {
            return Err(format!("{}: at most {MAX_ITEMS} supports and {MAX_ITEMS} loads", self.name));
        }
        let mut ids: Vec<u32> = self.supports.iter().map(|s| s.id).chain(self.loads.iter().map(|l| l.id)).collect();
        ids.sort_unstable();
        if ids.windows(2).any(|w| w[0] == w[1]) {
            return Err(format!("{}: two of its supports and loads have the same id", self.name));
        }
        for s in &self.supports {
            name_ok(&s.name).and_then(|()| faces_ok(&s.faces, &s.name)).map_err(|e| format!("{}: {e}", self.name))?;
        }
        for l in &self.loads {
            name_ok(&l.name).and_then(|()| faces_ok(&l.faces, &l.name)).and_then(|()| l.kind.check()).map_err(|e| format!("{}: {e}", self.name))?;
        }
        if let Some(size) = self.mesh_size
            && !(size.is_finite() && size > 0.0)
        {
            return Err(format!("{}: the element size must be more than 0", self.name));
        }
        Ok(())
    }

    /// The id the next support or load gets, and the name of the next of `word`'s kind
    /// ("Fixed1", "Force2").
    fn next(&self, word: &str) -> (u32, String) {
        let id = self.supports.iter().map(|s| s.id).chain(self.loads.iter().map(|l| l.id)).max().map_or(1, |m| m.saturating_add(1));
        let taken = |n: &str| self.supports.iter().any(|s| s.name == n) || self.loads.iter().any(|l| l.name == n);
        let name = (1..=MAX_ITEMS as u32 + 1).map(|i| format!("{word}{i}")).find(|n| !taken(n)).unwrap_or_else(|| format!("{word}{id}"));
        (id, name)
    }

    /// Holds `faces`. Returns the support's id.
    pub fn add_support(&mut self, kind: SupportKind, faces: Vec<FaceRef>, name: Option<String>) -> Result<u32, String> {
        if self.supports.len() >= MAX_ITEMS {
            return Err(format!("a study has at most {MAX_ITEMS} supports"));
        }
        let (id, auto) = self.next("Fixed");
        let name = name.map_or(auto, |n| n.trim().to_owned());
        name_ok(&name)?;
        faces_ok(&faces, &name)?;
        self.supports.push(Support { id, name, kind, faces });
        Ok(id)
    }

    /// Puts a load on `faces`. Returns the load's id.
    pub fn add_load(&mut self, kind: LoadKind, faces: Vec<FaceRef>, name: Option<String>) -> Result<u32, String> {
        if self.loads.len() >= MAX_ITEMS {
            return Err(format!("a study has at most {MAX_ITEMS} loads"));
        }
        kind.check()?;
        let (id, auto) = self.next(kind.word());
        let name = name.map_or(auto, |n| n.trim().to_owned());
        name_ok(&name)?;
        faces_ok(&faces, &name)?;
        self.loads.push(Load { id, name, faces, kind });
        Ok(id)
    }

    /// Changes the support or load `id`: what is given takes the place of what it had. A
    /// force's `direction` is `Some(None)` for square to its faces. What does not belong to
    /// the kind of thing it is (newtons of a pressure, anything but faces and a name of a
    /// support) is refused.
    pub fn set(
        &mut self,
        id: u32,
        faces: Option<Vec<FaceRef>>,
        name: Option<String>,
        newtons: Option<f64>,
        direction: Option<Option<Vec3>>,
        megapascals: Option<f64>,
    ) -> Result<(), String> {
        let before = self.clone();
        let change = |this: &mut Study| -> Result<(), String> {
            if let Some(s) = this.supports.iter_mut().find(|s| s.id == id) {
                if newtons.is_some() || direction.is_some() || megapascals.is_some() {
                    return Err(format!("{} is a support: it has faces and a name, and nothing else to set", s.name));
                }
                if let Some(f) = faces {
                    s.faces = f;
                }
                if let Some(n) = name {
                    s.name = n.trim().to_owned();
                }
                return Ok(());
            }
            let Some(l) = this.loads.iter_mut().find(|l| l.id == id) else { return Err(format!("{} has no support or load {id}", this.name)) };
            match &mut l.kind {
                LoadKind::Force { newtons: n, direction: d } => {
                    if megapascals.is_some() {
                        return Err(format!("{} is a force: its size is in `newtons`", l.name));
                    }
                    if let Some(v) = newtons {
                        *n = v;
                    }
                    if let Some(v) = direction {
                        *d = v;
                    }
                }
                LoadKind::Pressure { megapascals: m } => {
                    if newtons.is_some() || direction.is_some() {
                        return Err(format!("{} is a pressure: its size is in `megapascals`, and it is always square to its faces", l.name));
                    }
                    if let Some(v) = megapascals {
                        *m = v;
                    }
                }
            }
            if let Some(f) = faces {
                l.faces = f;
            }
            if let Some(n) = name {
                l.name = n.trim().to_owned();
            }
            Ok(())
        };
        // (All of it or none: a change that leaves the study wrong is not made.)
        if let Err(e) = change(self).and_then(|()| self.check()) {
            *self = before;
            return Err(e);
        }
        Ok(())
    }

    /// Takes the support or load `id` away. False when there is none.
    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.supports.len() + self.loads.len();
        self.supports.retain(|s| s.id != id);
        self.loads.retain(|l| l.id != id);
        self.supports.len() + self.loads.len() < before
    }

    /// What the study still needs before it can be solved, in words; empty when it is ready.
    pub fn missing(&self) -> Vec<&'static str> {
        let mut m = Vec::new();
        if self.supports.is_empty() {
            m.push("a support: a part that is held nowhere moves off, however small the load");
        }
        if self.loads.is_empty() {
            m.push("a load: with nothing pushing on it there is no stress to find");
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naming::{FaceOrigin, Fingerprint};
    use crate::{CapEnd, FeatureId};

    fn face(feature: u32) -> FaceRef {
        let fingerprint = Fingerprint { surface: "plane".into(), direction: Vec3::Z, centroid: Vec3::ZERO, area: 1.0 };
        FaceRef { origin: FaceOrigin::Cap { feature: FeatureId(feature), end: CapEnd::End }, fingerprint }
    }

    #[test]
    fn a_study_holds_faces_and_loads_them_and_says_what_it_still_needs() {
        let mut s = Study::new(StudyId(1), "Study1".into());
        assert_eq!(s.missing().len(), 2);
        let fixed = s.add_support(SupportKind::Fixed, vec![face(2)], None).unwrap();
        let push = s.add_load(LoadKind::Force { newtons: 100.0, direction: Some(Vec3::new(0.0, 0.0, -2.0)) }, vec![face(3)], None).unwrap();
        let press = s.add_load(LoadKind::Pressure { megapascals: 0.5 }, vec![face(3), face(4)], Some(" Wind ".into())).unwrap();
        let more = s.add_load(LoadKind::Force { newtons: -5.0, direction: None }, vec![face(5)], None).unwrap();
        assert_eq!((fixed, push, press, more), (1, 2, 3, 4), "one run of ids for supports and loads");
        let names: Vec<&str> = s.supports.iter().map(|x| x.name.as_str()).chain(s.loads.iter().map(|l| l.name.as_str())).collect();
        assert_eq!(names, ["Fixed1", "Force1", "Wind", "Force2"]);
        assert!(s.missing().is_empty());
        s.check().unwrap();
        // Taken away by id; a name that is free again is given again.
        assert!(s.remove(push) && !s.remove(push) && !s.remove(99));
        assert_eq!(s.add_load(LoadKind::Force { newtons: 1.0, direction: None }, vec![face(2)], None).unwrap(), 5);
        assert_eq!(s.loads.last().unwrap().name, "Force1");
        s.check().unwrap();

        // What is not a support or a load is refused in words.
        let refused = |r: Result<u32, String>, what: &str| assert!(r.as_ref().is_err_and(|e| e.contains(what)), "{r:?}");
        refused(s.add_support(SupportKind::Fixed, vec![], None), "at least one face");
        refused(s.add_load(LoadKind::Force { newtons: 0.0, direction: None }, vec![face(2)], None), "other than 0");
        refused(s.add_load(LoadKind::Force { newtons: f64::NAN, direction: None }, vec![face(2)], None), "other than 0");
        refused(s.add_load(LoadKind::Force { newtons: 1.0, direction: Some(Vec3::ZERO) }, vec![face(2)], None), "no direction");
        refused(s.add_load(LoadKind::Pressure { megapascals: f64::INFINITY }, vec![face(2)], None), "megapascals");
        refused(s.add_load(LoadKind::Pressure { megapascals: 1.0 }, vec![face(2)], Some("  ".into())), "1 to 64 characters");
        assert_eq!(s.loads.len(), 3, "nothing was added by what was refused");

        // Changed in place: what is given takes the place of what was there.
        s.set(more, Some(vec![face(6), face(7)]), Some("Pull".into()), Some(-8.0), Some(Some(Vec3::X)), None).unwrap();
        let pull = s.loads.iter().find(|l| l.id == more).unwrap();
        assert_eq!((pull.name.as_str(), pull.faces.len(), pull.kind), ("Pull", 2, LoadKind::Force { newtons: -8.0, direction: Some(Vec3::X) }));
        s.set(more, None, None, None, Some(None), None).unwrap();
        assert_eq!(s.loads.iter().find(|l| l.id == more).unwrap().kind, LoadKind::Force { newtons: -8.0, direction: None });
        s.set(press, None, None, None, None, Some(0.75)).unwrap();
        s.set(fixed, Some(vec![face(9)]), None, None, None, None).unwrap();
        assert_eq!(s.supports[0].faces, vec![face(9)]);
        // What is not its to have is refused, and it is left as it was.
        let was = s.clone();
        let told = |r: Result<(), String>, what: &str| assert!(r.as_ref().is_err_and(|e| e.contains(what)), "{r:?}");
        told(s.set(fixed, None, None, Some(1.0), None, None), "is a support");
        told(s.set(press, None, None, Some(1.0), None, None), "is a pressure");
        told(s.set(more, None, None, None, None, Some(1.0)), "is a force");
        told(s.set(more, Some(vec![]), None, Some(3.0), None, None), "at least one face");
        told(s.set(more, None, None, Some(0.0), None, None), "other than 0");
        told(s.set(77, None, None, None, None, None), "no support or load 77");
        assert_eq!(s, was);
    }

    #[test]
    fn a_study_is_written_and_read_back_and_a_damaged_one_is_told() {
        let mut s = Study::new(StudyId(3), "Bracket under load".into());
        s.add_support(SupportKind::Fixed, vec![face(2)], None).unwrap();
        s.add_load(LoadKind::Force { newtons: 250.0, direction: Some(Vec3::new(0.0, 0.0, -1.0)) }, vec![face(4)], None).unwrap();
        s.add_load(LoadKind::Pressure { megapascals: 2.0 }, vec![face(4)], None).unwrap();
        s.mesh_size = Some(2.5);
        let text = serde_json::to_value(&s).unwrap();
        assert_eq!(text["loads"][0]["type"], "force");
        assert_eq!(text["loads"][0]["newtons"], 250.0);
        assert_eq!(
            text["loads"][1],
            serde_json::json!({ "id": 3, "name": "Pressure1", "faces": text["loads"][1]["faces"], "type": "pressure", "megapascals": 2.0 })
        );
        assert_eq!(text["supports"][0]["kind"], "fixed");
        let back: Study = serde_json::from_value(text.clone()).unwrap();
        assert_eq!(back, s);
        // A study with nothing in it says no more than its id and name.
        assert_eq!(serde_json::to_value(Study::new(StudyId(1), "Study1".into())).unwrap(), serde_json::json!({ "id": 1, "name": "Study1" }));

        // Read from a file that was changed by hand: checked as a whole.
        let mut twice = s.clone();
        twice.loads[1].id = twice.loads[0].id;
        assert!(twice.check().unwrap_err().contains("same id"));
        let mut sized = s.clone();
        sized.mesh_size = Some(0.0);
        assert!(sized.check().unwrap_err().contains("element size"));
        let mut heavy = s.clone();
        heavy.loads[0].kind = LoadKind::Force { newtons: 1.0e20, direction: None };
        assert!(heavy.check().unwrap_err().contains("at most"));
    }
}

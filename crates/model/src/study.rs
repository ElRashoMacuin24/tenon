//! Stress studies (M11): where a part is held and what pushes on it.
//!
//! A study is the question "does this part hold?": faces and edges that are held (supports:
//! fixed, on a pin, or free to slide), and what pushes on the part (loads: forces, pressures,
//! moments and bearing loads on faces, forces along edges, a force from a point away, the
//! part's own weight, and its being flung outward as it turns). It is kept with the part,
//! refers to faces and edges by their persistent names like any feature does, and is edited
//! by commands (`study.*`) with undo. Working out the answer (dividing the part into elements, solving, showing stress) is
//! not in this module.
//!
//! Units are the part's: millimetres, newtons, and so megapascals (N/mm^2) for pressure and
//! stress.

use serde::{Deserialize, Serialize};
use tenon_geom::Vec3;

use crate::naming::{EdgeRef, FaceRef};

/// The most studies a part has, and the most supports or loads a study has (hostile-input
/// caps).
pub const MAX_STUDIES: usize = 100;
pub const MAX_ITEMS: usize = 1000;
/// The most faces one support or load is on.
pub const MAX_FACES: usize = 10_000;
/// The largest force (N) and pressure (MPa) taken: beyond any part a study is for.
pub const MAX_FORCE: f64 = 1.0e12;
pub const MAX_PRESSURE: f64 = 1.0e6;
/// The pull of the Earth, mm/s^2, and the largest acceleration taken.
pub const EARTH: f64 = 9810.0;
pub const MAX_ACCELERATION: f64 = 1.0e9;
/// The largest moment (N mm) and the fastest turning (turns a minute) taken.
pub const MAX_MOMENT: f64 = 1.0e15;
pub const MAX_RPM: f64 = 1.0e7;

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
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SupportKind {
    /// Held still in every direction, as if welded to something that does not move.
    Fixed,
    /// Round faces on a pin. Each of the three ways such a face could move is held or left
    /// free: across the pin (`radial`), along it (`axial`), and round it (`tangential`).
    Pin { radial: bool, axial: bool, tangential: bool },
    /// Flat or round faces that may slide along themselves but not leave or enter what they
    /// rest on.
    Frictionless,
}

impl SupportKind {
    /// A pin as it usually is: held across and along, free to turn.
    pub const PIN: SupportKind = SupportKind::Pin { radial: true, axial: true, tangential: false };

    /// "Fixed", "Pin" or "Frictionless": what its name starts with.
    pub fn word(&self) -> &'static str {
        match self {
            SupportKind::Fixed => "Fixed",
            SupportKind::Pin { .. } => "Pin",
            SupportKind::Frictionless => "Frictionless",
        }
    }

    /// The same in the words of files and commands.
    pub fn key(&self) -> &'static str {
        match self {
            SupportKind::Fixed => "fixed",
            SupportKind::Pin { .. } => "pin",
            SupportKind::Frictionless => "frictionless",
        }
    }

    pub fn check(&self) -> Result<(), String> {
        match self {
            SupportKind::Pin { radial: false, axial: false, tangential: false } => {
                Err("a pin that holds nothing is no support: hold it across (radial), along (axial) or round (tangential)".into())
            }
            _ => Ok(()),
        }
    }

    /// Whether a face of that surface ("plane", "cylinder" ...) can be held this way.
    pub fn takes(&self, surface: &str) -> Result<(), String> {
        match self {
            SupportKind::Fixed => Ok(()),
            SupportKind::Pin { .. } if surface == "cylinder" => Ok(()),
            SupportKind::Pin { .. } => Err(format!("a pin holds round (cylindrical) faces, and one of these is a {surface}")),
            SupportKind::Frictionless if surface == "plane" || surface == "cylinder" => Ok(()),
            SupportKind::Frictionless => {
                Err(format!("a frictionless support is on flat or round (cylindrical) faces, and one of these is a {surface}"))
            }
        }
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Faces of the part that are held.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Support {
    pub id: u32,
    pub name: String,
    #[serde(flatten)]
    pub kind: SupportKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FaceRef>,
    /// The edges that are held (a fixed support may be on edges in place of faces).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<EdgeRef>,
    /// Left out of the study for now, and kept.
    #[serde(default, skip_serializing_if = "is_false")]
    pub suppressed: bool,
}

/// What pushes on the part.
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
    /// The part's own weight: all of it pulled along `direction` at `acceleration` (mm/s^2;
    /// 9810 on the Earth). It is on no faces, and a study has one at most.
    Gravity { direction: Vec3, acceleration: f64 },
    /// A turning load of `newton_millimetres` in all on the faces, about `axis` through their
    /// middle (in the part's coordinates; right-handed: seen from the axis's tip it turns
    /// anticlockwise). Without an axis, about the faces' own: a flat face's normal, a round
    /// face's axis.
    Moment {
        newton_millimetres: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        axis: Option<Vec3>,
    },
    /// The load of a shaft in its bore, or of a bearing on its shaft: `newtons` along
    /// `direction`, pressing on the half of each round face that it pushes against, most in
    /// the middle of that half and nothing at its sides.
    Bearing { newtons: f64, direction: Vec3 },
    /// A force of `newtons` along `direction` that acts at `point`, away from the faces that
    /// carry it: they get the force, and the turning it has about them.
    RemoteForce { newtons: f64, direction: Vec3, point: Vec3 },
    /// The part turning at `rpm` turns a minute about `axis` through `point`: every bit of it
    /// is flung outward. On no faces, and a study has one at most.
    Body { axis: Vec3, point: Vec3, rpm: f64 },
}

/// A load on the part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Load {
    pub id: u32,
    pub name: String,
    /// The faces it is on (none for gravity, which is on all of the part).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FaceRef>,
    /// The edges it is along (a force may be along edges in place of faces).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<EdgeRef>,
    #[serde(flatten)]
    pub kind: LoadKind,
    /// Left out of the study for now, and kept.
    #[serde(default, skip_serializing_if = "is_false")]
    pub suppressed: bool,
}

/// What a command changes of a support or a load: what is given takes the place of what it
/// had.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemChange {
    pub faces: Option<Vec<FaceRef>>,
    /// The edges of a fixed support or a force (in place of its faces: an item is on one or
    /// the other).
    pub edges: Option<Vec<EdgeRef>>,
    pub name: Option<String>,
    pub suppressed: Option<bool>,
    /// A force's size.
    pub newtons: Option<f64>,
    /// The way a force pushes (`Some(None)`: square to its faces), or gravity pulls.
    pub direction: Option<Option<Vec3>>,
    /// A pressure's size.
    pub megapascals: Option<f64>,
    /// Gravity's size, mm/s^2.
    pub acceleration: Option<f64>,
    /// What a pin holds.
    pub radial: Option<bool>,
    pub axial: Option<bool>,
    pub tangential: Option<bool>,
    /// A moment's size.
    pub newton_millimetres: Option<f64>,
    /// The axis a moment turns about (`Some(None)`: its faces' own), or a body spins about.
    pub axis: Option<Option<Vec3>>,
    /// Where a remote force acts, or a point the axis of a spin goes through.
    pub point: Option<Vec3>,
    /// How fast a body spins.
    pub rpm: Option<f64>,
}

impl ItemChange {
    /// The first of the named values that is given and is not for a thing of that kind.
    fn stray(&self, allowed: &[&str]) -> Option<&'static str> {
        let given = [
            ("faces", self.faces.is_some()),
            ("edges", self.edges.is_some()),
            ("newton_millimetres", self.newton_millimetres.is_some()),
            ("axis", self.axis.is_some()),
            ("point", self.point.is_some()),
            ("rpm", self.rpm.is_some()),
            ("newtons", self.newtons.is_some()),
            ("direction", self.direction.is_some()),
            ("megapascals", self.megapascals.is_some()),
            ("acceleration", self.acceleration.is_some()),
            ("radial", self.radial.is_some()),
            ("axial", self.axial.is_some()),
            ("tangential", self.tangential.is_some()),
        ];
        given.into_iter().find(|(key, given)| *given && !allowed.contains(key)).map(|(key, _)| key)
    }
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

/// Whether a support or load is somewhere it can be: on faces, or (when its kind may be) on
/// edges, and not on both.
fn places_ok(faces: &[FaceRef], edges: &[EdgeRef], of: &str, may_be_on_edges: bool) -> Result<(), String> {
    match (faces.is_empty(), edges.is_empty()) {
        (true, true) if may_be_on_edges => return Err(format!("{of} needs at least one face, or edge")),
        (true, true) => return Err(format!("{of} needs at least one face")),
        (_, false) if !may_be_on_edges => return Err(format!("{of} is on faces: only a fixed support and a force can be on edges")),
        (false, false) => return Err(format!("{of} is on faces or on edges, not on both: make another for the others")),
        _ => {}
    }
    if faces.len() > MAX_FACES || edges.len() > MAX_FACES {
        return Err(format!("{of} is on too many faces or edges (at most {MAX_FACES})"));
    }
    Ok(())
}

impl Support {
    fn check(&self) -> Result<(), String> {
        name_ok(&self.name)?;
        places_ok(&self.faces, &self.edges, &self.name, matches!(self.kind, SupportKind::Fixed))?;
        self.kind.check().map_err(|e| format!("{}: {e}", self.name))?;
        self.faces.iter().try_for_each(|f| self.kind.takes(&f.fingerprint.surface)).map_err(|e| format!("{}: {e}", self.name))
    }
}

impl Load {
    fn check(&self) -> Result<(), String> {
        name_ok(&self.name)?;
        if self.kind.on_faces() {
            places_ok(&self.faces, &self.edges, &self.name, matches!(self.kind, LoadKind::Force { .. }))?;
        } else if !self.faces.is_empty() || !self.edges.is_empty() {
            return Err(format!("{} is on the whole part: it has no faces", self.name));
        }
        self.kind.check()?;
        if matches!(self.kind, LoadKind::Force { direction: None, .. }) && !self.edges.is_empty() {
            return Err(format!("{}: a force along edges needs a direction (an edge has no face to be square to)", self.name));
        }
        self.faces.iter().try_for_each(|f| self.kind.takes(&f.fingerprint.surface)).map_err(|e| format!("{}: {e}", self.name))
    }
}

impl LoadKind {
    pub fn check(&self) -> Result<(), String> {
        let directed = |d: &Vec3, of: &str| {
            if d.is_finite() && d.len() > tenon_geom::tol::LINEAR {
                Ok(())
            } else {
                Err(format!("{of} direction has no direction: give three numbers that are not all 0"))
            }
        };
        let force = |newtons: f64, what: &str| {
            if newtons.is_finite() && newtons.abs() > 0.0 && newtons.abs() <= MAX_FORCE {
                Ok(())
            } else {
                Err(format!("{what} must be a number of newtons other than 0, at most {MAX_FORCE:e}"))
            }
        };
        match self {
            LoadKind::Force { newtons, direction } => {
                force(*newtons, "a force")?;
                direction.as_ref().map_or(Ok(()), |d| directed(d, "a force's"))
            }
            LoadKind::Pressure { megapascals } => {
                if megapascals.is_finite() && megapascals.abs() > 0.0 && megapascals.abs() <= MAX_PRESSURE {
                    Ok(())
                } else {
                    Err(format!("a pressure must be a number of megapascals other than 0, at most {MAX_PRESSURE:e}"))
                }
            }
            LoadKind::Gravity { direction, acceleration } => {
                if !(acceleration.is_finite() && *acceleration > 0.0 && *acceleration <= MAX_ACCELERATION) {
                    return Err(format!("gravity's acceleration must be more than 0 mm/s^2, at most {MAX_ACCELERATION:e} (the Earth's is {EARTH})"));
                }
                directed(direction, "gravity's")
            }
            LoadKind::Moment { newton_millimetres, axis } => {
                if !(newton_millimetres.is_finite() && newton_millimetres.abs() > 0.0 && newton_millimetres.abs() <= MAX_MOMENT) {
                    return Err(format!("a moment must be a number of newton millimetres other than 0, at most {MAX_MOMENT:e}"));
                }
                axis.as_ref().map_or(Ok(()), |a| directed(a, "a moment's axis as a"))
            }
            LoadKind::Bearing { newtons, direction } => {
                force(*newtons, "a bearing load")?;
                directed(direction, "a bearing load's")
            }
            LoadKind::RemoteForce { newtons, direction, point } => {
                force(*newtons, "a remote force")?;
                directed(direction, "a remote force's")?;
                if point.is_finite() { Ok(()) } else { Err("a remote force acts at a point: give three numbers".into()) }
            }
            LoadKind::Body { axis, point, rpm } => {
                if !(rpm.is_finite() && rpm.abs() > 0.0 && rpm.abs() <= MAX_RPM) {
                    return Err(format!("a body's turning must be a number of turns a minute other than 0, at most {MAX_RPM:e}"));
                }
                directed(axis, "a body's axis as a")?;
                if point.is_finite() { Ok(()) } else { Err("a body's axis goes through a point: give three numbers".into()) }
            }
        }
    }

    /// What its name starts with: "Force", "Pressure", "Gravity" ...
    pub fn word(&self) -> &'static str {
        match self {
            LoadKind::Force { .. } => "Force",
            LoadKind::Pressure { .. } => "Pressure",
            LoadKind::Gravity { .. } => "Gravity",
            LoadKind::Moment { .. } => "Moment",
            LoadKind::Bearing { .. } => "Bearing",
            LoadKind::RemoteForce { .. } => "RemoteForce",
            LoadKind::Body { .. } => "Body",
        }
    }

    /// Whether it is on faces or edges (the part's weight and its turning are on all of it).
    pub fn on_faces(&self) -> bool {
        self.whole_part().is_none()
    }

    /// For a load that is on all of the part, and so there once in a study: what it is.
    pub fn whole_part(&self) -> Option<&'static str> {
        match self {
            LoadKind::Gravity { .. } => Some("the part's weight"),
            LoadKind::Body { .. } => Some("the part's turning"),
            _ => None,
        }
    }

    /// Whether it can be on a face of that surface ("plane", "cylinder" ...).
    pub fn takes(&self, surface: &str) -> Result<(), String> {
        match self {
            LoadKind::Bearing { .. } if surface != "cylinder" => {
                Err(format!("a bearing load is on round (cylindrical) faces, and one of these is a {surface}"))
            }
            _ => Ok(()),
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
            s.check().map_err(|e| format!("{}: {e}", self.name))?;
        }
        for l in &self.loads {
            l.check().map_err(|e| format!("{}: {e}", self.name))?;
        }
        for (i, l) in self.loads.iter().enumerate() {
            if let Some(what) = l.kind.whole_part()
                && self.loads[..i].iter().any(|o| o.kind.whole_part() == Some(what))
            {
                return Err(format!("{}: a study has {what} once", self.name));
            }
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
        self.add_support_on(kind, faces, Vec::new(), name)
    }

    /// Holds `faces`, or (a fixed support) `edges`. Returns the support's id.
    pub fn add_support_on(&mut self, kind: SupportKind, faces: Vec<FaceRef>, edges: Vec<EdgeRef>, name: Option<String>) -> Result<u32, String> {
        if self.supports.len() >= MAX_ITEMS {
            return Err(format!("a study has at most {MAX_ITEMS} supports"));
        }
        let (id, auto) = self.next(kind.word());
        let support = Support { id, name: name.map_or(auto, |n| n.trim().to_owned()), kind, faces, edges, suppressed: false };
        support.check()?;
        self.supports.push(support);
        Ok(id)
    }

    /// Puts a load on `faces` (none, for a load on the whole part). Returns the load's id.
    pub fn add_load(&mut self, kind: LoadKind, faces: Vec<FaceRef>, name: Option<String>) -> Result<u32, String> {
        self.add_load_on(kind, faces, Vec::new(), name)
    }

    /// Puts a load on `faces`, or (a force) along `edges`. Returns the load's id.
    pub fn add_load_on(&mut self, kind: LoadKind, faces: Vec<FaceRef>, edges: Vec<EdgeRef>, name: Option<String>) -> Result<u32, String> {
        if self.loads.len() >= MAX_ITEMS {
            return Err(format!("a study has at most {MAX_ITEMS} loads"));
        }
        kind.check()?;
        if let Some(what) = kind.whole_part()
            && let Some(there) = self.loads.iter().find(|l| l.kind.whole_part() == Some(what))
        {
            return Err(format!("{} has {what} already ({}): change that one", self.name, there.name));
        }
        let (id, auto) = self.next(kind.word());
        let load = Load { id, name: name.map_or(auto, |n| n.trim().to_owned()), faces, edges, kind, suppressed: false };
        load.check()?;
        self.loads.push(load);
        Ok(id)
    }

    /// Changes the support or load `id`: what is given takes the place of what it had. What
    /// does not belong to the kind of thing it is (newtons of a pressure, faces of gravity) is
    /// refused, and so is a change that leaves it wrong: it is then as it was.
    pub fn set(&mut self, id: u32, change: ItemChange) -> Result<(), String> {
        let before = self.clone();
        let stray = |name: &str, what: &str, has: &str, allowed: &[&str]| match change.stray(allowed) {
            Some(key) => Err(format!("{name} is {what}: `{key}` is not its to have ({has})")),
            None => Ok(()),
        };
        let alter = |this: &mut Study| -> Result<(), String> {
            if let Some(s) = this.supports.iter_mut().find(|s| s.id == id) {
                match &mut s.kind {
                    SupportKind::Pin { radial, axial, tangential } => {
                        stray(
                            &s.name,
                            "a pin",
                            "it has faces, a name, and what it holds: radial, axial, tangential",
                            &["faces", "radial", "axial", "tangential"],
                        )?;
                        *radial = change.radial.unwrap_or(*radial);
                        *axial = change.axial.unwrap_or(*axial);
                        *tangential = change.tangential.unwrap_or(*tangential);
                    }
                    SupportKind::Fixed => stray(&s.name, "a support", "it has faces or edges, and a name", &["faces", "edges"])?,
                    SupportKind::Frictionless => stray(&s.name, "a support", "it has faces and a name", &["faces"])?,
                }
                // (Given faces it is on faces, given edges on edges: one takes the place of
                // the other unless both are said.)
                if let Some(f) = &change.faces {
                    s.faces = f.clone();
                    if change.edges.is_none() {
                        s.edges.clear();
                    }
                }
                if let Some(e) = &change.edges {
                    s.edges = e.clone();
                    if change.faces.is_none() {
                        s.faces.clear();
                    }
                }
                if let Some(n) = &change.name {
                    s.name = n.trim().to_owned();
                }
                s.suppressed = change.suppressed.unwrap_or(s.suppressed);
                return Ok(());
            }
            let Some(l) = this.loads.iter_mut().find(|l| l.id == id) else { return Err(format!("{} has no support or load {id}", this.name)) };
            match &mut l.kind {
                LoadKind::Force { newtons, direction } => {
                    stray(&l.name, "a force", "its size is in `newtons`, its way in `direction`", &["faces", "edges", "newtons", "direction"])?;
                    *newtons = change.newtons.unwrap_or(*newtons);
                    *direction = change.direction.unwrap_or(*direction);
                }
                LoadKind::Pressure { megapascals } => {
                    stray(&l.name, "a pressure", "its size is in `megapascals`, and it is always square to its faces", &["faces", "megapascals"])?;
                    *megapascals = change.megapascals.unwrap_or(*megapascals);
                }
                LoadKind::Gravity { direction, acceleration } => {
                    stray(
                        &l.name,
                        "the part's weight",
                        "it has the way it pulls, `direction`, and its `acceleration`",
                        &["direction", "acceleration"],
                    )?;
                    match change.direction {
                        Some(Some(d)) => *direction = d,
                        Some(None) => return Err(format!("{} pulls one way, whatever the faces: give its `direction`", l.name)),
                        None => {}
                    }
                    *acceleration = change.acceleration.unwrap_or(*acceleration);
                }
                LoadKind::Moment { newton_millimetres, axis } => {
                    stray(
                        &l.name,
                        "a moment",
                        "its size is in `newton_millimetres`, and it turns about `axis`",
                        &["faces", "newton_millimetres", "axis"],
                    )?;
                    *newton_millimetres = change.newton_millimetres.unwrap_or(*newton_millimetres);
                    *axis = change.axis.unwrap_or(*axis);
                }
                LoadKind::Bearing { newtons, direction } => {
                    stray(&l.name, "a bearing load", "its size is in `newtons`, its way in `direction`", &["faces", "newtons", "direction"])?;
                    *newtons = change.newtons.unwrap_or(*newtons);
                    match change.direction {
                        Some(Some(d)) => *direction = d,
                        Some(None) => return Err(format!("{} presses one way: give its `direction`", l.name)),
                        None => {}
                    }
                }
                LoadKind::RemoteForce { newtons, direction, point } => {
                    stray(
                        &l.name,
                        "a remote force",
                        "it has `newtons`, `direction`, and the `point` it acts at",
                        &["faces", "newtons", "direction", "point"],
                    )?;
                    *newtons = change.newtons.unwrap_or(*newtons);
                    match change.direction {
                        Some(Some(d)) => *direction = d,
                        Some(None) => return Err(format!("{} acts one way: give its `direction`", l.name)),
                        None => {}
                    }
                    *point = change.point.unwrap_or(*point);
                }
                LoadKind::Body { axis, point, rpm } => {
                    stray(&l.name, "the part's turning", "it has its `axis`, a `point` the axis goes through, and `rpm`", &["axis", "point", "rpm"])?;
                    match change.axis {
                        Some(Some(a)) => *axis = a,
                        Some(None) => return Err(format!("{} turns about one axis: give its `axis`", l.name)),
                        None => {}
                    }
                    *point = change.point.unwrap_or(*point);
                    *rpm = change.rpm.unwrap_or(*rpm);
                }
            }
            if let Some(f) = &change.faces {
                l.faces = f.clone();
                if change.edges.is_none() {
                    l.edges.clear();
                }
            }
            if let Some(e) = &change.edges {
                l.edges = e.clone();
                if change.faces.is_none() {
                    l.faces.clear();
                }
            }
            if let Some(n) = &change.name {
                l.name = n.trim().to_owned();
            }
            l.suppressed = change.suppressed.unwrap_or(l.suppressed);
            Ok(())
        };
        // (All of it or none: a change that leaves the study wrong is not made.)
        if let Err(e) = alter(self).and_then(|()| self.check()) {
            *self = before;
            return Err(e);
        }
        Ok(())
    }

    /// The same study under another id and name.
    pub fn copied(&self, id: StudyId, name: String) -> Study {
        Study { id, name, ..self.clone() }
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
        // (What is suppressed is not there, for now.)
        if self.supports.iter().all(|s| s.suppressed) {
            m.push("a support: a part that is held nowhere moves off, however small the load");
        }
        if self.loads.iter().all(|l| l.suppressed) {
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

    fn face_of(feature: u32, surface: &str) -> FaceRef {
        let fingerprint = Fingerprint { surface: surface.into(), direction: Vec3::Z, centroid: Vec3::ZERO, area: 1.0 };
        FaceRef { origin: FaceOrigin::Cap { feature: FeatureId(feature), end: CapEnd::End }, fingerprint }
    }

    fn face(feature: u32) -> FaceRef {
        face_of(feature, "plane")
    }

    /// (An edge between the two ends of a feature: no part has one, and a study does not look.)
    fn edge(feature: u32) -> EdgeRef {
        let end = |end| FaceOrigin::Cap { feature: FeatureId(feature), end };
        EdgeRef::new(end(CapEnd::Start), end(CapEnd::End), crate::naming::EdgeFingerprint { mid: Vec3::ZERO, length: 1.0 })
    }

    fn told<T: std::fmt::Debug>(r: Result<T, String>, what: &str) {
        assert!(r.as_ref().is_err_and(|e| e.contains(what)), "{r:?}");
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
        let refused = told::<u32>;
        refused(s.add_support(SupportKind::Fixed, vec![], None), "at least one face");
        refused(s.add_load(LoadKind::Force { newtons: 0.0, direction: None }, vec![face(2)], None), "other than 0");
        refused(s.add_load(LoadKind::Force { newtons: f64::NAN, direction: None }, vec![face(2)], None), "other than 0");
        refused(s.add_load(LoadKind::Force { newtons: 1.0, direction: Some(Vec3::ZERO) }, vec![face(2)], None), "no direction");
        refused(s.add_load(LoadKind::Pressure { megapascals: f64::INFINITY }, vec![face(2)], None), "megapascals");
        refused(s.add_load(LoadKind::Pressure { megapascals: 1.0 }, vec![face(2)], Some("  ".into())), "1 to 64 characters");
        assert_eq!(s.loads.len(), 3, "nothing was added by what was refused");

        // Changed in place: what is given takes the place of what was there.
        let pull = ItemChange {
            faces: Some(vec![face(6), face(7)]),
            name: Some("Pull".into()),
            newtons: Some(-8.0),
            direction: Some(Some(Vec3::X)),
            ..ItemChange::default()
        };
        s.set(more, pull).unwrap();
        let pull = s.loads.iter().find(|l| l.id == more).unwrap();
        assert_eq!((pull.name.as_str(), pull.faces.len(), pull.kind), ("Pull", 2, LoadKind::Force { newtons: -8.0, direction: Some(Vec3::X) }));
        s.set(more, ItemChange { direction: Some(None), ..ItemChange::default() }).unwrap();
        assert_eq!(s.loads.iter().find(|l| l.id == more).unwrap().kind, LoadKind::Force { newtons: -8.0, direction: None });
        s.set(press, ItemChange { megapascals: Some(0.75), ..ItemChange::default() }).unwrap();
        s.set(fixed, ItemChange { faces: Some(vec![face(9)]), ..ItemChange::default() }).unwrap();
        assert_eq!(s.supports[0].faces, vec![face(9)]);
        // What is not its to have is refused, and it is left as it was.
        let was = s.clone();
        let newtons = |n: f64| ItemChange { newtons: Some(n), ..ItemChange::default() };
        told(s.set(fixed, newtons(1.0)), "is a support: `newtons` is not its to have");
        told(s.set(press, newtons(1.0)), "is a pressure");
        told(s.set(more, ItemChange { megapascals: Some(1.0), ..ItemChange::default() }), "is a force");
        told(s.set(more, ItemChange { radial: Some(true), ..ItemChange::default() }), "is a force: `radial`");
        told(s.set(more, ItemChange { faces: Some(vec![]), ..newtons(3.0) }), "at least one face");
        told(s.set(more, newtons(0.0)), "other than 0");
        told(s.set(77, ItemChange::default()), "no support or load 77");
        assert_eq!(s, was);

        // Suppressed: kept, and for now not there. A study with nothing else lacks it again.
        s.set(fixed, ItemChange { suppressed: Some(true), ..ItemChange::default() }).unwrap();
        assert!(s.supports[0].suppressed);
        assert_eq!(s.missing().len(), 1, "its only support is suppressed");
        s.set(fixed, ItemChange { suppressed: Some(false), ..ItemChange::default() }).unwrap();
        assert!(s.missing().is_empty());
    }

    #[test]
    fn pins_sliding_supports_and_the_parts_weight() {
        let mut s = Study::new(StudyId(1), "Study1".into());
        let (flat, round, ball) = (face_of(2, "plane"), face_of(3, "cylinder"), face_of(4, "sphere"));

        // A pin is on round faces, and holds across and along unless told otherwise.
        let pin = s.add_support(SupportKind::PIN, vec![round.clone()], None).unwrap();
        assert_eq!((s.supports[0].name.as_str(), s.supports[0].kind), ("Pin1", SupportKind::Pin { radial: true, axial: true, tangential: false }));
        told(s.add_support(SupportKind::PIN, vec![round.clone(), flat.clone()], None), "a pin holds round");
        told(s.add_support(SupportKind::Pin { radial: false, axial: false, tangential: false }, vec![round.clone()], None), "holds nothing");
        s.set(pin, ItemChange { axial: Some(false), tangential: Some(true), ..ItemChange::default() }).unwrap();
        assert_eq!(s.supports[0].kind, SupportKind::Pin { radial: true, axial: false, tangential: true });
        told(s.set(pin, ItemChange { radial: Some(false), tangential: Some(false), ..ItemChange::default() }), "holds nothing");
        told(s.set(pin, ItemChange { faces: Some(vec![flat.clone()]), ..ItemChange::default() }), "a pin holds round");
        assert_eq!(s.supports[0].kind, SupportKind::Pin { radial: true, axial: false, tangential: true }, "left as it was");

        // A frictionless support is on flat or round faces.
        let slide = s.add_support(SupportKind::Frictionless, vec![flat.clone(), round.clone()], None).unwrap();
        assert_eq!(s.supports[1].name, "Frictionless1");
        told(s.add_support(SupportKind::Frictionless, vec![ball.clone()], None), "flat or round");
        told(s.set(slide, ItemChange { radial: Some(true), ..ItemChange::default() }), "is a support: `radial`");
        // Fixed takes any face.
        s.add_support(SupportKind::Fixed, vec![ball], None).unwrap();

        // The part's weight is on no faces, pulls one way, and is there once.
        let down = Vec3::new(0.0, 0.0, -1.0);
        let weight = s.add_load(LoadKind::Gravity { direction: down, acceleration: EARTH }, vec![], None).unwrap();
        assert_eq!(s.loads[0].name, "Gravity1");
        told(s.add_load(LoadKind::Gravity { direction: down, acceleration: EARTH }, vec![], None), "the part's weight already (Gravity1)");
        told(s.add_load(LoadKind::Gravity { direction: Vec3::ZERO, acceleration: EARTH }, vec![], None), "no direction");
        told(s.add_load(LoadKind::Gravity { direction: down, acceleration: 0.0 }, vec![], None), "more than 0");
        told(s.add_load(LoadKind::Force { newtons: 1.0, direction: None }, vec![], None), "at least one face");
        s.set(weight, ItemChange { direction: Some(Some(Vec3::X)), acceleration: Some(1620.0), ..ItemChange::default() }).unwrap();
        assert_eq!(s.loads[0].kind, LoadKind::Gravity { direction: Vec3::X, acceleration: 1620.0 });
        told(s.set(weight, ItemChange { faces: Some(vec![flat]), ..ItemChange::default() }), "is the part's weight: `faces`");
        told(s.set(weight, ItemChange { direction: Some(None), ..ItemChange::default() }), "pulls one way");
        told(s.set(weight, ItemChange { newtons: Some(1.0), ..ItemChange::default() }), "is the part's weight: `newtons`");
        s.check().unwrap();
        assert!(s.missing().is_empty(), "held and loaded");

        // As files have them, and read back.
        let text = serde_json::to_value(&s).unwrap();
        assert_eq!(text["supports"][0]["kind"], "pin");
        assert_eq!(
            (text["supports"][0]["radial"].clone(), text["supports"][0]["tangential"].clone()),
            (serde_json::json!(true), serde_json::json!(true))
        );
        assert_eq!(
            text["supports"][1],
            serde_json::json!({ "id": 2, "name": "Frictionless1", "kind": "frictionless", "faces": text["supports"][1]["faces"] })
        );
        assert_eq!(
            text["loads"][0],
            serde_json::json!({ "id": 4, "name": "Gravity1", "type": "gravity", "direction": { "x": 1.0, "y": 0.0, "z": 0.0 }, "acceleration": 1620.0 })
        );
        let back: Study = serde_json::from_value(text).unwrap();
        assert_eq!(back, s);
        // A copy is the same study under another id and name.
        let copy = s.copied(StudyId(2), "Study1 copy".into());
        assert_eq!((copy.id, copy.name.as_str(), &copy.supports, &copy.loads), (StudyId(2), "Study1 copy", &s.supports, &s.loads));
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

    #[test]
    fn supports_and_forces_along_edges() {
        let mut s = Study::new(StudyId(1), "Study1".into());
        // A fixed support on edges in place of faces; not on both, and not on neither.
        let held = s.add_support_on(SupportKind::Fixed, vec![], vec![edge(2), edge(3)], None).unwrap();
        assert_eq!((s.supports[0].name.as_str(), s.supports[0].faces.len(), s.supports[0].edges.len()), ("Fixed1", 0, 2));
        told(s.add_support_on(SupportKind::Fixed, vec![face(2)], vec![edge(2)], None), "on faces or on edges, not on both");
        told(s.add_support_on(SupportKind::Fixed, vec![], vec![], None), "at least one face, or edge");
        // Only a fixed support can be: a pin and a sliding support are of faces.
        told(s.add_support_on(SupportKind::PIN, vec![], vec![edge(2)], None), "only a fixed support and a force can be on edges");
        told(s.add_support_on(SupportKind::Frictionless, vec![], vec![edge(2)], None), "only a fixed support and a force");
        // Given faces it is on faces; given edges again, on edges.
        s.set(held, ItemChange { faces: Some(vec![face(4)]), ..ItemChange::default() }).unwrap();
        assert_eq!((s.supports[0].faces.len(), s.supports[0].edges.len()), (1, 0));
        s.set(held, ItemChange { edges: Some(vec![edge(5)]), ..ItemChange::default() }).unwrap();
        assert_eq!((s.supports[0].faces.len(), s.supports[0].edges.len()), (0, 1));
        told(s.set(held, ItemChange { faces: Some(vec![face(4)]), edges: Some(vec![edge(5)]), ..ItemChange::default() }), "not on both");

        // A force along edges needs a direction: an edge has no face to be square to.
        let along = LoadKind::Force { newtons: 50.0, direction: Some(Vec3::new(0.0, 0.0, -1.0)) };
        let push = s.add_load_on(along, vec![], vec![edge(6)], None).unwrap();
        told(s.add_load_on(LoadKind::Force { newtons: 50.0, direction: None }, vec![], vec![edge(6)], None), "needs a direction");
        told(s.set(push, ItemChange { direction: Some(None), ..ItemChange::default() }), "needs a direction");
        told(s.add_load_on(LoadKind::Pressure { megapascals: 1.0 }, vec![], vec![edge(6)], None), "only a fixed support and a force can be on edges");
        // As files have them: `edges` in place of `faces`.
        let text = serde_json::to_value(&s).unwrap();
        assert!(text["supports"][0].get("faces").is_none() && text["supports"][0]["edges"].as_array().is_some_and(|e| e.len() == 1), "{text}");
        assert!(text["loads"][0].get("faces").is_none() && text["loads"][0]["edges"].as_array().is_some_and(|e| e.len() == 1), "{text}");
        assert_eq!(serde_json::from_value::<Study>(text).unwrap(), s);
        s.check().unwrap();
    }

    #[test]
    fn moments_bearing_loads_remote_forces_and_the_parts_turning() {
        let mut s = Study::new(StudyId(1), "Study1".into());
        let (flat, round) = (face_of(2, "plane"), face_of(3, "cylinder"));
        s.add_support(SupportKind::Fixed, vec![flat.clone()], None).unwrap();

        // A moment: about its faces' own axis unless one is said.
        let turn = s.add_load(LoadKind::Moment { newton_millimetres: 2500.0, axis: None }, vec![flat.clone()], None).unwrap();
        assert_eq!(s.loads[0].name, "Moment1");
        told(s.add_load(LoadKind::Moment { newton_millimetres: 0.0, axis: None }, vec![flat.clone()], None), "newton millimetres other than 0");
        told(s.add_load(LoadKind::Moment { newton_millimetres: 1.0, axis: Some(Vec3::ZERO) }, vec![flat.clone()], None), "no direction");
        told(s.add_load(LoadKind::Moment { newton_millimetres: 1.0, axis: None }, vec![], None), "at least one face");
        s.set(turn, ItemChange { newton_millimetres: Some(-40.0), axis: Some(Some(Vec3::X)), ..ItemChange::default() }).unwrap();
        assert_eq!(s.loads[0].kind, LoadKind::Moment { newton_millimetres: -40.0, axis: Some(Vec3::X) });
        s.set(turn, ItemChange { axis: Some(None), ..ItemChange::default() }).unwrap();
        assert_eq!(s.loads[0].kind, LoadKind::Moment { newton_millimetres: -40.0, axis: None });
        told(s.set(turn, ItemChange { newtons: Some(1.0), ..ItemChange::default() }), "is a moment: `newtons` is not its to have");

        // A bearing load: on round faces, one way.
        let press = s.add_load(LoadKind::Bearing { newtons: 300.0, direction: Vec3::X }, vec![round.clone()], None).unwrap();
        assert_eq!(s.loads[1].name, "Bearing1");
        told(s.add_load(LoadKind::Bearing { newtons: 300.0, direction: Vec3::X }, vec![flat.clone()], None), "a bearing load is on round");
        told(s.set(press, ItemChange { direction: Some(None), ..ItemChange::default() }), "presses one way");
        told(s.set(press, ItemChange { faces: Some(vec![flat.clone()]), ..ItemChange::default() }), "a bearing load is on round");
        s.set(press, ItemChange { newtons: Some(450.0), direction: Some(Some(Vec3::Y)), ..ItemChange::default() }).unwrap();
        assert_eq!(s.loads[1].kind, LoadKind::Bearing { newtons: 450.0, direction: Vec3::Y });

        // A remote force: a force, a way, and where it acts.
        let far = LoadKind::RemoteForce { newtons: 80.0, direction: -Vec3::Z, point: Vec3::new(150.0, 5.0, 5.0) };
        let remote = s.add_load(far, vec![flat.clone()], None).unwrap();
        assert_eq!(s.loads[2].name, "RemoteForce1");
        told(
            s.add_load(LoadKind::RemoteForce { newtons: 80.0, direction: -Vec3::Z, point: Vec3::new(f64::NAN, 0.0, 0.0) }, vec![flat.clone()], None),
            "acts at a point",
        );
        s.set(remote, ItemChange { point: Some(Vec3::new(1.0, 2.0, 3.0)), ..ItemChange::default() }).unwrap();
        assert!(matches!(s.loads[2].kind, LoadKind::RemoteForce { point, newtons, .. } if point == Vec3::new(1.0, 2.0, 3.0) && newtons == 80.0));
        told(s.set(remote, ItemChange { rpm: Some(5.0), ..ItemChange::default() }), "is a remote force: `rpm`");

        // The part's turning: on no faces, once, beside its weight.
        let spin = s.add_load(LoadKind::Body { axis: Vec3::Z, point: Vec3::ZERO, rpm: 3000.0 }, vec![], None).unwrap();
        assert_eq!(s.loads[3].name, "Body1");
        s.add_load(LoadKind::Gravity { direction: -Vec3::Z, acceleration: EARTH }, vec![], None).unwrap();
        told(s.add_load(LoadKind::Body { axis: Vec3::Z, point: Vec3::ZERO, rpm: 10.0 }, vec![], None), "the part's turning already (Body1)");
        told(s.add_load(LoadKind::Gravity { direction: -Vec3::Z, acceleration: EARTH }, vec![], None), "the part's weight already (Gravity1)");
        told(s.set(spin, ItemChange { rpm: Some(0.0), ..ItemChange::default() }), "turns a minute other than 0");
        told(s.set(spin, ItemChange { axis: Some(None), ..ItemChange::default() }), "turns about one axis");
        told(s.set(spin, ItemChange { faces: Some(vec![flat]), ..ItemChange::default() }), "is the part's turning: `faces`");
        s.set(spin, ItemChange { rpm: Some(-1500.0), axis: Some(Some(Vec3::X)), point: Some(Vec3::new(0.0, 5.0, 5.0)), ..ItemChange::default() })
            .unwrap();
        assert_eq!(s.loads[3].kind, LoadKind::Body { axis: Vec3::X, point: Vec3::new(0.0, 5.0, 5.0), rpm: -1500.0 });
        s.check().unwrap();

        // As files have them, and read back.
        let text = serde_json::to_value(&s).unwrap();
        let kinds: Vec<&str> = text["loads"].as_array().unwrap().iter().map(|l| l["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["moment", "bearing", "remote_force", "body", "gravity"]);
        assert!(text["loads"][0].get("axis").is_none(), "a moment about its faces' own axis says no axis: {text}");
        assert_eq!(
            text["loads"][3],
            serde_json::json!({ "id": 5, "name": "Body1", "type": "body", "axis": { "x": 1.0, "y": 0.0, "z": 0.0 }, "point": { "x": 0.0, "y": 5.0, "z": 5.0 }, "rpm": -1500.0 })
        );
        assert_eq!(serde_json::from_value::<Study>(text).unwrap(), s);
    }
}

//! Topology optimization (DEC-123): where in a part material is needed to carry a study's
//! loads to its supports, for a given share of its mass kept.
//!
//! The method is the published one (SIMP: Bendsoe and Sigmund; the density filter: Bruns and
//! Tortorelli, Bourdin; the projection: Wang, Lazarov and Sigmund), written here from the
//! papers. Each element of the part's mesh has a density between [`X_MIN`] and 1; its stiffness
//! goes as the density cubed. Each round the solver is asked how much energy each element holds
//! under the loads ([`Stiffness`]); material moves toward the elements that work hardest, the
//! densities smoothed over a radius so that no member is thinner than about twice it
//! ([`filter`]), until the design settles ([`oc`]). What is kept is then turned into a closed
//! surface ([`iso`], [`smooth`]) to look at and to export.
//!
//! Nothing here starts a program: the solver is behind [`Stiffness`], so the loop is tried
//! with stand-ins in tests and run with CalculiX in use.

pub mod filter;
pub mod iso;
pub mod oc;
pub mod region;
pub mod smooth;
pub mod solver;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tenon_geom::Vec3;

pub use filter::{Filter, mirror_group};
pub use iso::{Extracted, Field, Skin};
pub use oc::optimize;
pub use solver::Ccx;

/// How many stiffness levels the solver is given: an element's density is rounded to the
/// nearest thousandth for it (level 0 is the floor, 1000 the material itself).
pub const LEVELS: u16 = 1000;
/// The stiffness of an element with no material, as a share of the material's: not nothing,
/// so that the solver always has an answer.
pub const FLOOR: f64 = 1e-9;
/// The least density the update gives an element (less is as good as nothing, and a density
/// of nothing could never grow again).
pub const X_MIN: f64 = 1e-3;
/// How much a part-full element is penalised: stiffness goes as density to this power.
pub const PENALTY: f64 = 3.0;
/// The power the optimality criterion is taken to (a half: the usual damping).
pub const DAMPING: f64 = 0.5;
/// The most an element's density may change in one round (without sharpening).
pub const MOVE: f64 = 0.2;
/// Sharpening's steepness, stage by stage.
pub const STAGES: [f64; 5] = [1.0, 2.0, 4.0, 8.0, 16.0];
/// The most rounds at one stage before the next.
pub const STAGE_ITERATIONS: usize = 20;
/// The density sharpening turns about.
pub const PROJECT_AT: f64 = 0.5;
/// The design has settled when no density changes by more than this in a round.
pub const SETTLED: f64 = 0.01;
/// ... or has levelled off when the work of the loads changes by less than this share ...
pub const LEVEL_OFF: f64 = 1e-3;
/// ... this many rounds running.
pub const LEVEL_OFF_RUN: usize = 5;
/// The most nodes a mesh may have to be optimized on: each round is a whole solve.
pub const MAX_SHAPE_NODES: usize = 100_000;

/// A plane the design is to be the same on both sides of: a point on it and its normal (unit).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mirror {
    pub point: Vec3,
    pub normal: Vec3,
}

impl Mirror {
    /// Where `p` is seen in the plane.
    pub fn image(&self, p: Vec3) -> Vec3 {
        p - self.normal * (2.0 * (p - self.point).dot(self.normal))
    }
}

/// What the optimizer works on, element by element (the mesh's tetrahedra in order): each
/// one's volume (mm^3), its middle, and whether it must stay solid (it is held, loaded, or
/// kept).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Design {
    pub volumes: Vec<f64>,
    pub centroids: Vec<Vec3>,
    pub passive: Vec<bool>,
}

/// What is asked of the optimizer.
#[derive(Clone, Debug, PartialEq)]
pub struct Goal {
    /// The share of the part's volume to keep, more than 0 and less than 1.
    pub fraction: f64,
    /// The filter's radius (mm): no member thinner than about twice it.
    pub radius: f64,
    /// Sharpen the design toward solid and empty as it goes (the projection, in stages).
    pub sharpen: bool,
    /// The most rounds.
    pub max_iterations: usize,
    /// The most time for the whole run.
    pub budget: Duration,
    /// Planes the design is the same on both sides of: none, or up to three at right angles.
    pub mirrors: Vec<Mirror>,
}

/// One round, as it is told.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Iteration {
    /// From 1.
    pub number: usize,
    /// The work of the loads (N mm) on this round's design: twice the energy it holds.
    pub compliance: f64,
    /// The share of the volume this round's design keeps (after filtering and sharpening).
    pub fraction: f64,
    /// The most any element's density changed in the update that followed.
    pub change: f64,
    /// The sharpening's steepness this round (1 without sharpening).
    pub sharpness: f64,
    /// How far from solid-or-empty the design is: 0 when every free element is 0 or 1, 1 when
    /// all are a half (the mean of 4 x (1 - x) by volume).
    pub grey: f64,
    /// How long the round took, the solve included.
    pub seconds: f64,
}

/// Why the loop stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// No density changed by more than [`SETTLED`] (at full sharpness).
    Settled,
    /// The work of the loads changed by less than [`LEVEL_OFF`] for [`LEVEL_OFF_RUN`] rounds.
    LevelledOff,
    /// The most rounds asked for were done.
    Limit,
    /// The time ran out.
    Budget,
    /// It was stopped; the design is the last one worked out.
    Stopped,
}

impl Stop {
    /// As commands and files say it.
    pub fn word(self) -> &'static str {
        match self {
            Stop::Settled => "settled",
            Stop::LevelledOff => "levelled_off",
            Stop::Limit => "limit",
            Stop::Budget => "budget",
            Stop::Stopped => "stopped",
        }
    }
}

/// What the optimizer found.
#[derive(Clone, Debug, PartialEq)]
pub struct Optimized {
    /// Each element's density as the last solve saw it (filtered and sharpened), 1 where
    /// passive: what the shape is drawn from.
    pub density: Vec<f64>,
    /// The work of the loads with every element solid (N mm): the stiffest the part can be.
    pub solid_compliance: f64,
    pub history: Vec<Iteration>,
    pub stop: Stop,
}

/// What tells the optimizer how hard each element works: given each element's stiffness level
/// (0 to [`LEVELS`]; see [`modulus`]), the energy each holds under the loads (N mm), in the
/// mesh's order. An error of [`crate::tools::STOPPED`] means it was stopped.
pub trait Stiffness {
    fn energies(&mut self, levels: &[u16]) -> Result<Vec<f64>, String>;
}

/// The stage of a run, as its progress tells it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Meshing,
    /// The part solved solid, once.
    Solid,
    Iterating,
    /// The surface of what is kept being made.
    Shaping,
    /// The kept shape solved, once.
    Checking,
}

/// Where a run is, for a window to show while it goes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Report {
    pub phase: Phase,
    pub last: Option<Iteration>,
    /// The most rounds it may take.
    pub most: usize,
}

/// A run's progress, shared with whoever watches it.
pub type Progress = Arc<Mutex<Report>>;

/// The Young's modulus of stiffness level `level` for a material of modulus `e0`: the floor
/// plus the level's density cubed of the rest.
pub fn modulus(level: u16, e0: f64) -> f64 {
    let x = f64::from(level.min(LEVELS)) / f64::from(LEVELS);
    e0 * (FLOOR + x.powi(3) * (1.0 - FLOOR))
}

/// The level a density is solved at: the nearest thousandth.
pub fn level_of(x: f64) -> u16 {
    (x.clamp(0.0, 1.0) * f64::from(LEVELS)).round() as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_thousandths_and_zero_is_the_floor() {
        assert_eq!(modulus(LEVELS, 200e3), 200e3);
        assert!((modulus(0, 200e3) - 200e3 * FLOOR).abs() < 1e-18);
        assert!((modulus(500, 1.0) - (FLOOR + 0.125 * (1.0 - FLOOR))).abs() < 1e-15);
        assert_eq!((level_of(0.0), level_of(0.4996), level_of(0.5004), level_of(1.0), level_of(2.0), level_of(-1.0)), (0, 500, 500, 1000, 1000, 0));
        let m = Mirror { point: Vec3::new(5.0, 0.0, 0.0), normal: Vec3::X };
        assert!(m.image(Vec3::new(7.0, 1.0, 2.0)).dist(Vec3::new(3.0, 1.0, 2.0)) < 1e-15);
    }
}

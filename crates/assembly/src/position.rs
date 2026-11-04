//! Positions of an assembly (DEC-131): values for some of its relationships other than those it
//! has, and where its components go when the solver is given them. Nothing of the assembly
//! changes: a drawing's overlay view keeps its values and asks again each time.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tenon_geom::{Frame, tol};

use crate::model::{Assembly, ComponentId, JointKind, RelKind, RelationshipId};
use crate::session::{Parts, solve_assembly, system};
use crate::solve;

/// A value of one relationship in a position. What is not given stays as the relationship has it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JointValue {
    pub relationship: RelationshipId,
    /// An offset (mm): of a mate, flush, insert or tangent; of a joint, how far its origins are
    /// apart along A's Z.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
    /// An angle (radians): of an angle constraint; of a joint, how far B is turned about A's Z.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
}

impl JointValue {
    /// Finite, and in the range of what a model holds.
    pub fn is_valid(&self) -> bool {
        self.offset.is_none_or(|o| o.is_finite() && o.abs() <= tol::MAX_SIZE)
            && self.angle.is_none_or(f64::is_finite)
            && (self.offset.is_some() || self.angle.is_some())
    }
}

/// What a relationship's values are now, for a panel to show and change.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub relationship: RelationshipId,
    /// "Slider:1".
    pub name: String,
    /// Its offset (mm) now, if a position can set one.
    pub offset: Option<f64>,
    /// Its angle (radians) now, if a position can set one.
    pub angle: Option<f64>,
}

/// Puts a value into a relationship. A value on a motion a joint leaves free holds that motion:
/// a slider at a distance is held there as a rigid joint is, a rotational joint at an angle too,
/// and the solver does the rest.
fn set(kind: &mut RelKind, value: &JointValue) -> Result<(), &'static str> {
    let (new_offset, new_angle) = (value.offset, value.angle);
    match kind {
        RelKind::Mate { offset, .. } | RelKind::Flush { offset, .. } | RelKind::Insert { offset, .. } | RelKind::Tangent { offset, .. } => {
            if new_angle.is_some() {
                return Err("it has an offset and no angle");
            }
            *offset = new_offset.unwrap_or(*offset);
        }
        RelKind::Angle { angle, .. } => {
            if new_offset.is_some() {
                return Err("it has an angle and no offset");
            }
            *angle = new_angle.unwrap_or(*angle);
        }
        RelKind::Symmetry { .. } => return Err("nothing of a symmetry is a value"),
        RelKind::Joint { joint, offset, angle, .. } => {
            *offset = new_offset.unwrap_or(*offset);
            *angle = new_angle.unwrap_or(*angle);
            *joint = match (*joint, new_offset.is_some(), new_angle.is_some()) {
                (JointKind::Ball, ..) => return Err("a ball joint has no value to set"),
                (JointKind::Planar, _, true) => return Err("a planar joint's turn is free and cannot be set: give its offset"),
                (JointKind::Planar, ..) => JointKind::Planar,
                (JointKind::Rigid, ..) => JointKind::Rigid,
                (JointKind::Revolute, _, true) | (JointKind::Slider, true, _) | (JointKind::Cylindrical, true, true) => JointKind::Rigid,
                (JointKind::Cylindrical, true, false) => JointKind::Revolute,
                (JointKind::Cylindrical, false, true) => JointKind::Slider,
                (j, ..) => j,
            };
        }
    }
    Ok(())
}

/// Where each component goes when the relationships take `values`: the assembly solved with them,
/// from where it stands. Fails, saying what to do, when a value names what is not there or the
/// assembly cannot go there.
pub fn position_frames(asm: &Assembly, parts: &Parts, values: &[JointValue]) -> Result<BTreeMap<ComponentId, Frame>, String> {
    let mut at = asm.clone();
    for value in values {
        let id = value.relationship;
        let r = at.relationship_mut(id).ok_or_else(|| format!("{id} is no longer in the assembly: take its value out of the position"))?;
        if r.suppressed {
            return Err(format!("{} is suppressed: take its value out of the position, or use it again in the assembly", r.name));
        }
        let name = r.name.clone();
        set(&mut r.kind, value).map_err(|why| format!("{name}: {why}"))?;
    }
    let solved = solve_assembly(&mut at, parts, None);
    let named = |id: RelationshipId| asm.relationship(id).map_or_else(|| id.to_string(), |r| r.name.clone());
    if !solved.converged {
        let names: Vec<String> = solved.failing.iter().map(|(id, _)| named(*id)).collect();
        return Err(format!("the assembly cannot go to this position ({} cannot hold): try other values", names.join(", ")));
    }
    if let Some((id, why)) = solved.failing.iter().find(|(id, _)| values.iter().any(|v| v.relationship == *id)) {
        return Err(format!("{}: {why}", named(*id)));
    }
    Ok(at.components.iter().map(|c| (c.id, c.placement)).collect())
}

/// The values a position can set, as they are now. A joint's free motions are read from where the
/// assembly stands.
pub fn readings(asm: &Assembly, parts: &Parts) -> Vec<Reading> {
    let sys = system(asm, parts);
    let at = |id: RelationshipId| sys.ids.iter().position(|x| *x == id).and_then(|i| sys.rels.get(i));
    asm.relationships
        .iter()
        .filter(|r| !r.suppressed)
        .filter_map(|r| {
            let (offset, angle) = match &r.kind {
                RelKind::Mate { offset, .. } | RelKind::Flush { offset, .. } | RelKind::Insert { offset, .. } | RelKind::Tangent { offset, .. } => {
                    (Some(*offset), None)
                }
                RelKind::Angle { angle, .. } => (None, Some(*angle)),
                RelKind::Symmetry { .. } => return None,
                RelKind::Joint { joint, offset, angle, .. } => {
                    let now = at(r.id).map(|rel| solve::joint_reading(rel, &sys.bodies));
                    let read_offset = now.map_or(*offset, |n| n.0);
                    let read_angle = now.map_or(*angle, |n| n.1);
                    match joint {
                        JointKind::Ball => return None,
                        JointKind::Planar => (Some(*offset), None),
                        JointKind::Rigid => (Some(*offset), Some(*angle)),
                        JointKind::Revolute => (Some(*offset), Some(read_angle)),
                        JointKind::Slider => (Some(read_offset), Some(*angle)),
                        JointKind::Cylindrical => (Some(read_offset), Some(read_angle)),
                    }
                }
            };
            Some(Reading { relationship: r.id, name: r.name.clone(), offset, angle })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Geom, Target};

    fn joint(joint: JointKind) -> RelKind {
        let t = Target { component: None, geom: Geom::Origin };
        RelKind::Joint { joint, a: t.clone(), b: t, flip: false, offset: 1.0, angle: 0.25 }
    }

    fn value(offset: Option<f64>, angle: Option<f64>) -> JointValue {
        JointValue { relationship: RelationshipId(1), offset, angle }
    }

    /// The joint a joint comes to with these values, its offset and its angle.
    fn set_on(j: JointKind, offset: Option<f64>, angle: Option<f64>) -> Result<(JointKind, f64, f64), &'static str> {
        let mut k = joint(j);
        set(&mut k, &value(offset, angle))?;
        match k {
            RelKind::Joint { joint, offset, angle, .. } => Ok((joint, offset, angle)),
            _ => Err("not a joint"),
        }
    }

    #[test]
    fn a_value_on_a_free_motion_holds_it() {
        use JointKind::{Ball, Cylindrical, Planar, Revolute, Rigid, Slider};
        // What each value does: the motion it names is held; what is not given stays free or as it is.
        assert_eq!(set_on(Revolute, None, Some(2.0)), Ok((Rigid, 1.0, 2.0)));
        assert_eq!(set_on(Revolute, Some(3.0), None), Ok((Revolute, 3.0, 0.25)), "its turn stays free");
        assert_eq!(set_on(Slider, Some(3.0), None), Ok((Rigid, 3.0, 0.25)));
        assert_eq!(set_on(Slider, None, Some(2.0)), Ok((Slider, 1.0, 2.0)), "its slide stays free");
        assert_eq!(set_on(Cylindrical, Some(3.0), Some(2.0)), Ok((Rigid, 3.0, 2.0)));
        assert_eq!(set_on(Cylindrical, Some(3.0), None), Ok((Revolute, 3.0, 0.25)));
        assert_eq!(set_on(Cylindrical, None, Some(2.0)), Ok((Slider, 1.0, 2.0)));
        assert_eq!(set_on(Rigid, Some(3.0), Some(2.0)), Ok((Rigid, 3.0, 2.0)));
        assert_eq!(set_on(Planar, Some(3.0), None), Ok((Planar, 3.0, 0.25)));
        assert!(set_on(Planar, None, Some(2.0)).is_err());
        assert!(set_on(Ball, Some(3.0), None).is_err());
    }

    #[test]
    fn a_value_a_relationship_does_not_have_is_refused() {
        let t = Target { component: None, geom: Geom::Origin };
        let mut mate = RelKind::Mate { a: t.clone(), b: t.clone(), offset: 0.0 };
        assert!(set(&mut mate, &value(None, Some(1.0))).is_err());
        set(&mut mate, &value(Some(4.0), None)).unwrap();
        assert!(matches!(mate, RelKind::Mate { offset, .. } if offset == 4.0));
        let mut angle = RelKind::Angle { a: t.clone(), b: t.clone(), angle: 0.0, reference: tenon_geom::Vec3::Z };
        assert!(set(&mut angle, &value(Some(1.0), None)).is_err());
        set(&mut angle, &value(None, Some(0.5))).unwrap();
        assert!(matches!(angle, RelKind::Angle { angle, .. } if angle == 0.5));
        let mut symmetry = RelKind::Symmetry { a: t.clone(), b: t.clone(), plane: t };
        assert!(set(&mut symmetry, &value(Some(1.0), None)).is_err());
    }

    #[test]
    fn a_value_is_finite_and_says_something() {
        assert!(value(Some(1.0), None).is_valid());
        assert!(value(None, Some(1.0)).is_valid());
        assert!(!value(None, None).is_valid());
        assert!(!value(Some(f64::NAN), None).is_valid());
        assert!(!value(Some(1e30), None).is_valid());
        assert!(!value(None, Some(f64::INFINITY)).is_valid());
    }
}

//! Kernel failures in plain words: what the feature could not do, the likely reason and what to
//! try, then the kernel's own words for reference. Shown in the browser and the viewport when a
//! feature fails.

use tenon_kernel::KernelError;

use crate::document::{FeatureKind, Operation};

/// The message for `kind` failing with `e` in the kernel.
pub(crate) fn explain(kind: &FeatureKind, e: &KernelError) -> String {
    let words = e.to_string();
    let emptied = words.contains("empty shape");
    let plain = match (kind, e) {
        (_, KernelError::Cancelled) => return "Stopped.".into(),
        (_, KernelError::InvalidInput(_)) => return words,
        (_, KernelError::Unsupported(op)) => return format!("This geometry kernel cannot do {op} yet."),
        (FeatureKind::Fillet(f), _) => format!(
            "The {} mm fillet could not be made on {}. The radius is probably too large for the edges or for the faces beside them: try a smaller radius, or fewer edges.",
            mm(f.radius),
            count(f.edges.len(), "this edge", "these edges")
        ),
        (FeatureKind::Chamfer(c), _) => format!(
            "The chamfer could not be made on {}. It is probably too large for the faces beside the edges: try a smaller size, or fewer edges.",
            count(c.edges.len(), "this edge", "these edges")
        ),
        (FeatureKind::Shell(s), _) => format!(
            "The part could not be hollowed out with {} mm walls. The walls are probably too thick for its rounds or thin places: try thinner walls, or remove other faces.",
            mm(s.thickness)
        ),
        (FeatureKind::Extrude(x), _) => match (x.operation, emptied) {
            (Operation::Cut, true) => "The cut removes the whole part: try a shorter distance, or the other direction.".into(),
            (Operation::Intersect, true) => "The extrusion and the part do not overlap, so nothing would be left: check the distance and direction.".into(),
            (Operation::Join | Operation::Cut | Operation::Intersect, false) if is_boolean(e) => format!(
                "The extrusion could not be {} the part. It may only touch the part along a face or an edge: try a slightly different distance, or move the sketch.",
                match x.operation {
                    Operation::Cut => "cut from",
                    Operation::Intersect => "intersected with",
                    _ => "joined to",
                }
            ),
            _ => "The extrusion could not be made from this sketch. Its profile may cross itself or not be closed: check the sketch.".into(),
        },
        (FeatureKind::Revolve(_), _) if is_boolean(e) => {
            "The revolution could not be combined with the part. It may only touch the part along a face or an edge: try a slightly different angle or profile.".into()
        }
        (FeatureKind::Revolve(_), _) => {
            "The revolution could not be made. The axis may cross the profile, or the profile may cross itself: put the axis on the profile's edge or outside it.".into()
        }
        (FeatureKind::Hole(h), _) if emptied => format!("The {} cut away the whole part: check the hole sizes.", count(h.points.len(), "hole", "holes")),
        (FeatureKind::Hole(h), _) => format!(
            "The {} could not be cut. A hole may miss the part, or break out of it at an edge: check where the hole centres are.",
            count(h.points.len(), "hole", "holes")
        ),
        (FeatureKind::Rib(_), _) => {
            "The rib could not be made. Its lines may not reach walls on both sides: check the sketch, or give the rib a distance.".into()
        }
        (FeatureKind::PatternRect(_) | FeatureKind::PatternCircular(_) | FeatureKind::Mirror(_), _) => {
            "The copies could not be joined to the part. A copy may touch the part only along an edge or at a point: change the spacing, count or plane.".into()
        }
        _ => "The geometry kernel could not compute this feature.".into(),
    };
    format!("{plain} (Kernel: {words})")
}

fn is_boolean(e: &KernelError) -> bool {
    matches!(e, KernelError::OperationFailed { op: "boolean", .. })
}

/// `3`, `2.5`: a length as typed.
pub(crate) fn mm(x: f64) -> String {
    let s = format!("{x:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// "this edge" for one, "these 4 edges" for more.
fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        one.to_owned()
    } else {
        let (first, rest) = many.split_once(' ').unwrap_or(("", many));
        if first.is_empty() { format!("{n} {rest}") } else { format!("{first} {n} {rest}") }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn kind(v: serde_json::Value) -> FeatureKind {
        serde_json::from_value(v).unwrap()
    }

    fn failed(op: &'static str, reason: &str) -> KernelError {
        KernelError::OperationFailed { op, reason: reason.into() }
    }

    #[test]
    fn each_feature_says_what_it_could_not_do_and_what_to_try() {
        let edge = json!({ "faces": [{ "type": "cap", "feature": 1, "end": "end" }, { "type": "side", "feature": 1, "curve": 2 }], "fingerprint": { "mid": { "x": 0.0, "y": 0.0, "z": 0.0 }, "length": 1.0 } });
        let occt = failed("fillet", "fillet: StdFail_NotDone: BRep_API: command not done");
        let fillet = kind(json!({ "type": "fillet", "edges": [edge.clone(), edge.clone()], "radius": 6.0 }));
        assert_eq!(
            explain(&fillet, &occt),
            "The 6 mm fillet could not be made on these 2 edges. The radius is probably too large for the edges or for the faces beside them: try a smaller radius, or fewer edges. (Kernel: fillet failed: fillet: StdFail_NotDone: BRep_API: command not done)"
        );
        let one = kind(json!({ "type": "fillet", "edges": [edge.clone()], "radius": 2.5 }));
        assert!(explain(&one, &occt).starts_with("The 2.5 mm fillet could not be made on this edge."));
        let shell = kind(json!({ "type": "shell", "remove": [], "thickness": 2.0, "outside": false }));
        assert!(explain(&shell, &failed("shell", "x")).starts_with("The part could not be hollowed out with 2 mm walls."));
        let cut = kind(json!({ "type": "extrude", "sketch": 1, "extent": { "distance": 5.0 }, "operation": "cut" }));
        assert!(explain(&cut, &failed("boolean", "the operation produced an empty shape")).starts_with("The cut removes the whole part"));
        assert!(explain(&cut, &failed("boolean", "boolean failed: ...")).starts_with("The extrusion could not be cut from the part."));
        let join = kind(json!({ "type": "extrude", "sketch": 1, "extent": { "distance": 5.0 } }));
        assert!(explain(&join, &failed("extrude", "x")).starts_with("The extrusion could not be made from this sketch."));
        assert!(explain(&join, &failed("boolean", "x")).starts_with("The extrusion could not be joined to the part."));
        let hole = kind(json!({ "type": "hole", "sketch": 1, "points": [2, 3, 4], "diameter": 5.0, "kind": "simple", "extent": "through_all" }));
        assert!(explain(&hole, &failed("boolean", "x")).starts_with("The 3 holes could not be cut."));
        let mirror = kind(json!({ "type": "mirror", "features": [2], "plane": { "origin": "XY" } }));
        assert!(explain(&mirror, &failed("boolean", "x")).starts_with("The copies could not be joined to the part."));
        // Messages that are already plain, and kernels that cannot do something, stay short.
        assert_eq!(
            explain(&join, &KernelError::InvalidInput("a profile line has zero length".into())),
            "invalid input: a profile line has zero length"
        );
        assert_eq!(explain(&fillet, &KernelError::Unsupported("fillet")), "This geometry kernel cannot do fillet yet.");
    }
}

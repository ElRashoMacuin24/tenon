//! Undo and redo through edits that make the part fail to rebuild: after every step, the rebuild
//! the app makes (resuming from its cache) is the same as a rebuild from scratch, and undoing a
//! failing edit gives back exactly the part from before it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use tenon_geom::tol;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;
use tenon_model::{Document, FeatureId, FeatureStatus, Regen, RegenCache, Session, regenerate, regenerate_with};

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// What a rebuild gives: each feature's status, and each body's volume and area.
fn outcome(r: &Regen, k: &OcctKernel) -> (Vec<(FeatureId, FeatureStatus)>, Vec<(f64, f64)>) {
    let bodies = r
        .bodies
        .iter()
        .map(|b| {
            let m = k.mass_properties(b.shape, 1.0).unwrap();
            (m.volume, m.area)
        })
        .collect();
    (r.status.clone(), bodies)
}

fn same(a: &(Vec<(FeatureId, FeatureStatus)>, Vec<(f64, f64)>), b: &(Vec<(FeatureId, FeatureStatus)>, Vec<(f64, f64)>)) -> bool {
    a.0 == b.0 && a.1.len() == b.1.len() && a.1.iter().zip(&b.1).all(|(x, y)| tol::rel_eq(x.0, y.0, 1e-9) && tol::rel_eq(x.1, y.1, 1e-9))
}

/// A 40 x 30 x 10 block with a rounded corner, hollowed out from the top, and a round cut.
fn part(s: &mut Session, k: &mut OcctKernel) -> (u64, u64, u64) {
    let sk = run(s, k, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    let rect = run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 30 }));
    let ex = run(s, k, "model.extrude", json!({ "sketch": sk, "distance": 10 }))["feature"].as_u64().unwrap();
    let side = |i: usize| json!({ "type": "side", "feature": ex, "curve": rect["lines"][i] });
    let edge = run(s, k, "model.edge_ref", json!({ "faces": [side(1), side(2)] }));
    let fillet = run(s, k, "model.fillet", json!({ "edges": [edge], "radius": 3 }))["feature"].as_u64().unwrap();
    let top = run(s, k, "model.face_ref", json!({ "origin": { "type": "cap", "feature": ex, "end": "end" } }));
    let shell = run(s, k, "model.shell", json!({ "remove": [top], "thickness": 2 }))["feature"].as_u64().unwrap();
    let hole = run(s, k, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(s, k, "sketch.circle", json!({ "sketch": hole, "cx": 12, "cy": 15, "r": 4 }));
    run(s, k, "model.extrude", json!({ "sketch": hole, "distance": 5, "operation": "cut" }));
    (ex, fillet, shell)
}

/// Sets one numeric field of a feature (`/radius`, `/thickness`, `/extent/distance`).
fn set(s: &mut Session, k: &mut OcctKernel, feature: u64, field: &str, value: f64) {
    let f = s.document().features().iter().find(|f| u64::from(f.id.0) == feature).unwrap();
    let mut kind = serde_json::to_value(&f.kind).unwrap();
    *kind.pointer_mut(field).unwrap() = json!(value);
    run(s, k, "feature.update", json!({ "feature": feature, "kind": kind }));
}

#[test]
fn undo_and_redo_through_failing_rebuilds_match_a_rebuild_from_scratch() {
    let mut k = OcctKernel::new();
    let mut s = Session::default();
    let (ex, fillet, shell) = part(&mut s, &mut k);
    let mut cache = RegenCache::default();
    let mut step = 0usize;
    let mut failures = 0;
    let mut undone_failures = 0;
    // A fixed walk (a linear congruential sequence, so every run takes the same steps).
    let mut seed: u64 = 0x5eed;
    let mut next = |n: u64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };
    let mut before_failing: Option<(Document, (Vec<(FeatureId, FeatureStatus)>, Vec<(f64, f64)>))> = None;
    while step < 160 {
        step += 1;
        let earlier = s.document().clone();
        let action = next(7);
        match action {
            0 => set(&mut s, &mut k, fillet, "/radius", [2.0, 3.0, 5.0, 25.0][next(4) as usize]),
            1 => set(&mut s, &mut k, shell, "/thickness", [1.0, 2.0, 20.0][next(3) as usize]),
            2 => set(&mut s, &mut k, ex, "/extent/distance", [8.0, 10.0, 14.0][next(3) as usize]),
            3 => {
                let suppressed = s.document().features().iter().any(|f| u64::from(f.id.0) == fillet && f.suppressed);
                run(&mut s, &mut k, "feature.suppress", json!({ "feature": fillet, "suppressed": !suppressed }));
            }
            4 | 5 => {
                s.undo();
            }
            _ => {
                s.redo();
            }
        }
        // The rebuild the app makes, resuming from its cache, against one from scratch.
        let mut cached = regenerate_with(s.document(), &mut k, Some(&mut cache));
        let mut fresh = regenerate(s.document(), &mut k);
        let (a, b) = (outcome(&cached, &k), outcome(&fresh, &k));
        assert!(same(&a, &b), "step {step}: the cached rebuild differs from a fresh one:\n{a:?}\n{b:?}");
        let failed = a.0.iter().any(|(_, st)| matches!(st, FeatureStatus::Error { .. }));
        // Undoing the edit that made it fail gives back the document and the part from before.
        // (After an edit or a redo, one undo goes back to the step before; after an undo it
        // goes further back.)
        if failed && action != 4 && action != 5 && before_failing.is_none() && s.document() != &earlier {
            failures += 1;
            let mut old = regenerate(&earlier, &mut k);
            before_failing = Some((earlier, outcome(&old, &k)));
            old.release(&mut k);
            s.undo();
            let (doc, part) = before_failing.take().unwrap();
            assert_eq!(s.document(), &doc, "step {step}: undo did not give back the document");
            let mut back = regenerate_with(s.document(), &mut k, Some(&mut cache));
            assert!(same(&outcome(&back, &k), &part), "step {step}: undo did not give back the part");
            back.release(&mut k);
            undone_failures += 1;
            // And redo brings the failure back, the same.
            s.redo();
            let mut again = regenerate_with(s.document(), &mut k, Some(&mut cache));
            assert!(same(&outcome(&again, &k), &a), "step {step}: redo did not bring the failure back");
            again.release(&mut k);
        }
        cached.release(&mut k);
        fresh.release(&mut k);
    }
    assert!(failures >= 5 && undone_failures == failures, "the walk failed {failures} times");
    cache.release(&mut k);
}

//! M8 kernel queries: points along an edge, which the flat pattern of a sheet metal part uses
//! for cuts that run across a bend.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use tenon_geom::{Axis, Frame, Vec3};
use tenon_kernel::{BoolOp, CurveKind, Kernel, KernelError};
use tenon_kernel_occt::OcctKernel;

#[test]
fn points_along_an_edge_run_from_its_start_to_its_end_and_stay_on_it() {
    let mut k = OcctKernel::new();
    // A straight edge is its two ends.
    let b = k.make_box(&Frame::WORLD, Vec3::new(10.0, 20.0, 30.0)).unwrap().shape;
    let topo = k.topology(b).unwrap();
    for e in 0..topo.edges {
        let (info, pts) = (k.edge_info(b.edge(e)).unwrap(), k.edge_points(b.edge(e), 0.01).unwrap());
        assert_eq!(pts.len(), 2, "edge {e}");
        assert!(pts[0].near(info.start, 1e-12) && pts[1].near(info.end, 1e-12), "edge {e}: {pts:?} {info:?}");
    }
    // A hole bored across a round bar: where the two cylinders meet is neither a line nor a
    // circle. Its points start and end where the edge does, lie on both cylinders, and are
    // close enough together to keep within the deflection asked for.
    let bar = k.make_cylinder(&Axis::Z, 10.0, 40.0).unwrap().shape;
    let across = Axis::new(Vec3::new(-20.0, 0.0, 20.0), Vec3::X).unwrap();
    let drill = k.make_cylinder(&across, 4.0, 40.0).unwrap().shape;
    let bored = k.boolean(BoolOp::Cut, bar, &[drill]).unwrap().shape;
    let topo = k.topology(bored).unwrap();
    let mut found = 0;
    for e in 0..topo.edges {
        let info = k.edge_info(bored.edge(e)).unwrap();
        if matches!(info.curve, CurveKind::Line { .. } | CurveKind::Circle { .. }) || info.degenerate {
            continue;
        }
        found += 1;
        for deflection in [0.05, 0.005] {
            let pts = k.edge_points(bored.edge(e), deflection).unwrap();
            assert!(pts.len() > 8, "{}", pts.len());
            assert!(pts[0].near(info.start, 1e-9) && pts[pts.len() - 1].near(info.end, 1e-9), "{:?} {:?} {info:?}", pts[0], pts[pts.len() - 1]);
            let mut length = 0.0;
            for w in pts.windows(2) {
                length += w[0].dist(w[1]);
            }
            for p in &pts {
                let on_bar = (p.x * p.x + p.y * p.y).sqrt();
                let on_drill = (p.y * p.y + (p.z - 20.0) * (p.z - 20.0)).sqrt();
                assert!((on_bar - 10.0).abs() < 1e-6 && (on_drill - 4.0).abs() < 1e-6, "{p:?}");
            }
            // The chords are a little shorter than the curve, never longer.
            assert!(length <= info.length + 1e-9 && length > info.length * (1.0 - deflection), "{length} {}", info.length);
        }
    }
    assert!(found >= 2, "the hole has a rim at each end: {found}");
    // What is not an edge, or not a size, is refused.
    assert!(matches!(k.edge_points(bored.edge(topo.edges), 0.01), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.edge_points(bored.edge(0), 0.0), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.edge_points(bored.edge(0), f64::NAN), Err(KernelError::InvalidInput(_))));
}

//! M2 demo: the parametric L-mount script (holes, pattern, rib, fillet, chamfer, work plane,
//! parameters, End of Part, measure) and the files it writes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;
use std::path::PathBuf;

use serde_json::json;
use tenon_cli::Engine;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;

fn kernel() -> Box<dyn Kernel> {
    Box::new(OcctKernel::new())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-cli-m2-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_m2_demo_script_builds_a_verified_parametric_mount() {
    // The script checks the analytic volume after every feature, the End of Part, the measured
    // height, and the whole part again after the thickness parameter changes; here we check the
    // files it writes.
    let dir = scratch("demo");
    let r = tenon_cli::demo_m2(kernel(), &dir).unwrap();
    assert_eq!(r.json["ok"], true, "{}", r.text);
    let expected = 29328.0 - 453.0 * PI;

    let mut k = OcctKernel::new();
    let info = tenon_cli::info(&mut k, &dir.join("mount.step")).unwrap();
    let v = info.json["shapes"][0]["volume_mm3"].as_f64().unwrap();
    assert!((v - expected).abs() < 1e-6 * expected, "STEP volume {v} vs {expected}");
    for png in ["mount.png", "mount-thick.png"] {
        assert!(std::fs::read(dir.join(png)).unwrap().starts_with(&[0x89, b'P', b'N', b'G']), "{png}");
    }

    // The saved project reopens with its parameters: changing t still drives the whole part.
    let mut e = Engine::new(kernel(), &dir);
    e.exec("file.open", &json!({ "path": "mount.tenon" })).unwrap();
    let m = e.exec("model.mass", &json!({})).unwrap();
    assert!((m["bodies"][0]["volume"].as_f64().unwrap() - expected).abs() < 1e-6 * expected);
    e.exec("param.set", &json!({ "name": "t", "equation": "10" })).unwrap();
    let m = e.exec("model.mass", &json!({})).unwrap();
    let thick = 34160.0 - 525.0 * PI;
    assert!((m["bodies"][0]["volume"].as_f64().unwrap() - thick).abs() < 1e-6 * thick, "{m}");
}

/// Timing, not a check: where a regeneration of the M2 demo mount spends its time.
/// `cargo test --release -p tenon-cli --test m2 scene_time_breakdown -- --ignored --nocapture`
#[test]
#[ignore]
fn scene_time_breakdown() {
    use std::time::Instant;
    use tenon_kernel::MeshTol;
    use tenon_model::{regenerate, scene};
    let mut k = OcctKernel::new();
    let (doc, _) =
        tenon_io::project::open(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/m2-mount/mount.tenon"))).unwrap();
    for _ in 0..3 {
        let t = Instant::now();
        let mut r = regenerate(&doc, &mut k);
        let regen = t.elapsed().as_secs_f64() * 1000.0;
        let b = r.bodies[0].shape;
        let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        let topo = k.topology(b).unwrap();
        let t_topo = ms(t);
        let t = Instant::now();
        let mesh = k.tessellate(b, &MeshTol::default()).unwrap();
        let t_mesh = ms(t);
        let t = Instant::now();
        for i in 0..topo.faces {
            k.face_info(b.face(i)).unwrap();
        }
        let t_faces = ms(t);
        let t = Instant::now();
        for i in 0..topo.edges {
            k.edge_info(b.edge(i)).unwrap();
        }
        let t_edges = ms(t);
        let t = Instant::now();
        k.mass_properties(b, 1.0).unwrap();
        let t_mass = ms(t);
        let t = Instant::now();
        let _ = scene(&r, &mut k, &MeshTol::default()).unwrap();
        let t_scene = ms(t);
        println!(
            "regen {regen:.1} ms | scene {t_scene:.1} ms = tessellate {t_mesh:.1} + {} face infos {t_faces:.1} + {} edge infos {t_edges:.1} + mass {t_mass:.1} + topology {t_topo:.1} | {} triangles",
            topo.faces,
            topo.edges,
            mesh.indices.len() / 3
        );
        r.release(&mut k);
    }
}

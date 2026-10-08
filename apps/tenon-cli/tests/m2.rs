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

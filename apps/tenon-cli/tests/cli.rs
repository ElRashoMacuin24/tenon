//! tenon-cli commands against the OpenCASCADE kernel: the M0 demo bracket, STEP info, STL
//! conversion, and failure reporting.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use tenon_geom::{Aabb3, Vec3, tol};
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;

/// A fresh scratch directory per test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-cli-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn m0_bracket_matches_analytic_volume() {
    let mut k = OcctKernel::new();
    let b = tenon_cli::m0_bracket(&mut k).unwrap();
    let m = k.mass_properties(b.shape, 1.0).unwrap();
    assert!(tol::rel_eq(m.volume, b.expected_volume, tol::MEASURE_REL), "{} vs {}", m.volume, b.expected_volume);
    assert!(k.is_valid(b.shape).unwrap());
    let t = k.topology(b.shape).unwrap();
    assert_eq!(t.solids, 1);
    let bb = k.bounding_box(b.shape).unwrap().unwrap();
    assert!(bb.near(&Aabb3::new(Vec3::ZERO, Vec3::new(60.0, 40.0, 38.0)), 1e-6), "{bb:?}");
    assert_eq!(k.live_shapes(), 1, "intermediate shapes are released");
}

#[test]
fn demo_writes_step_and_stl_that_read_back() {
    let dir = scratch("demo");
    let mut k = OcctKernel::new();
    let r = tenon_cli::demo_m0(&mut k, &dir).unwrap();
    assert!(r.json["relative_volume_error"].as_f64().unwrap() < tol::MEASURE_REL);
    let step = dir.join("bracket.step");
    let stl = std::fs::read(dir.join("bracket.stl")).unwrap();
    let tris = tenon_io::stl::binary_triangle_count(&stl).unwrap();
    assert_eq!(tris as u64, r.json["triangles"].as_u64().unwrap());
    assert!(tris > 100);

    let mut k2 = OcctKernel::new();
    let info = tenon_cli::info(&mut k2, &step).unwrap();
    let shapes = info.json["shapes"].as_array().unwrap();
    assert_eq!(shapes.len(), 1);
    let expected = r.json["expected_volume_mm3"].as_f64().unwrap();
    assert!(tol::rel_eq(shapes[0]["volume_mm3"].as_f64().unwrap(), expected, tol::MEASURE_REL));
    assert_eq!(shapes[0]["valid"], true);
    assert!(info.text.contains("valid"));

    let out = dir.join("again.stl");
    let c = tenon_cli::convert(&mut k2, &step, &out).unwrap();
    assert!(c.json["triangles"].as_u64().unwrap() > 100);
    assert!(tenon_io::stl::binary_triangle_count(&std::fs::read(&out).unwrap()).is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_files_are_reported_not_panicking() {
    let dir = scratch("bad");
    let mut k = OcctKernel::new();
    let missing = tenon_cli::info(&mut k, &dir.join("missing.step")).unwrap_err();
    assert!(missing.contains("cannot read"), "{missing}");
    let junk = dir.join("junk.step");
    std::fs::write(&junk, b"definitely not STEP").unwrap();
    let e = tenon_cli::info(&mut k, &junk).unwrap_err();
    assert!(e.contains("junk.step"), "{e}");
    assert!(tenon_cli::convert(&mut k, &junk, &dir.join("x.stl")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn json_output_is_clean_json() {
    // Regression: OCCT's STEP writer printed transfer statistics to stdout, corrupting --json.
    let dir = scratch("json");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_tenon-cli"))
        .args(["demo", "m0", "--json", "--out"])
        .arg(&dir)
        .output()
        .expect("run tenon-cli");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is exactly one JSON document");
    assert_eq!(v["shape"]["kind"], "Solid");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn version_names_the_kernel() {
    let r = tenon_cli::version(&OcctKernel::new());
    assert_eq!(r.json["kernel"], "occt");
    assert!(r.json["kernel_version"].as_str().unwrap().contains("8."));
}

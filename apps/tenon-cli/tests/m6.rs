//! M6 demo: the four-fittings script (coil, threads, sweep, loft, draft, tapered extrusion, split,
//! combine, materials, design tables, and an assembly whose parts list weighs them) and the files
//! it writes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use serde_json::json;
use tenon_cli::Engine;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;

fn kernel() -> Box<dyn Kernel> {
    Box::new(OcctKernel::new())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-cli-m6-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn near(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs()
}

#[test]
fn the_m6_demo_script_builds_four_verified_fittings_and_weighs_their_assembly() {
    // The script checks every volume and mass against its analytic value as it goes; here we
    // check the files it writes and that the saved parts reopen as what they were.
    let dir = scratch("demo");
    let r = tenon_cli::demo_m6(kernel(), &dir).unwrap();
    assert_eq!(r.json["ok"], true, "{}", r.text);

    // The nozzle: drafted flange + cone + tapered spout - bore, as STEP and in the project.
    let (t5, t3) = (5f64.to_radians().tan(), 3f64.to_radians().tan());
    let (b, r2) = (50.0 - 12.0 * t5, 8.0 - 20.0 * t3);
    let nozzle = 2.0 * (2500.0 + 50.0 * b + b * b) + 6240.0 * PI + PI * 20.0 / 3.0 * (64.0 + 8.0 * r2 + r2 * r2) - 896.0 * PI;
    let mut k = OcctKernel::new();
    let info = tenon_cli::info(&mut k, &dir.join("nozzle.step")).unwrap();
    let v = info.json["shapes"][0]["volume_mm3"].as_f64().unwrap();
    assert!(near(v, nozzle, 1e-6), "STEP volume {v} vs {nozzle}");
    assert!(std::fs::metadata(dir.join("nozzle.stl")).unwrap().len() > 1000);
    for png in ["spring.png", "spring-stiff.png", "bolt.png", "handle.png", "nozzle.png", "nozzle-section.png", "fittings.png"] {
        assert!(std::fs::read(dir.join(png)).unwrap().starts_with(&[0x89, b'P', b'N', b'G']), "{png}");
    }

    // The parts list: a material and a mass for each part.
    let csv = std::fs::read_to_string(dir.join("fittings-bom.csv")).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], "Item,Part,Name,Quantity,Volume (mm^3),Material,Mass (g)");
    assert_eq!(lines[1], "1,nozzle,Nozzle,1,34993.304,PLA,43.392", "{csv}");
    assert!(lines[2].starts_with("2,bolt,Bolt,1,21") && lines[2].contains(",\"Steel, mild\",16."), "{csv}");
    assert!(lines[3].starts_with("3,spring,Spring,1,947.48") && lines[3].ends_with(",Stainless steel,7.580"), "{csv}");
    assert_eq!(lines[4], "4,handle,Handle,1,1857.849,ABS,1.932", "{csv}");
    assert_eq!(lines.len(), 5);

    // The saved spring reopens with its material and its table; the other size is one command away.
    let mut e = Engine::new(kernel(), &dir);
    e.exec("file.open", &json!({ "path": "spring.tenon" })).unwrap();
    let t = e.exec("table.show", &json!({})).unwrap();
    assert_eq!(t["table"]["active"], "Soft");
    assert_eq!(t["table"]["rows"][1], json!({ "name": "Stiff", "values": [3.0, 10.0] }));
    let turn = PI * 16.0 * PI;
    let m = e.exec("model.mass", &json!({})).unwrap();
    assert!(near(m["bodies"][0]["volume"].as_f64().unwrap(), 6.0 * turn, 1e-4) && m["material"] == "Stainless steel", "{m}");
    e.exec("table.activate", &json!({ "row": "Stiff" })).unwrap();
    let m = e.exec("model.mass", &json!({})).unwrap();
    assert!(near(m["mass"].as_f64().unwrap(), 10.0 * turn * 8.0 / 1000.0, 1e-4), "{m}");

    // The bolt keeps its cut thread, sized from the shank.
    e.exec("file.open", &json!({ "path": "bolt.tenon" })).unwrap();
    let t = e.exec("model.threads", &json!({})).unwrap();
    assert_eq!((t["threads"][0]["designation"].as_str(), t["threads"][0]["modelled"].as_bool()), (Some("M8x1.25"), Some(true)), "{t}");
    // The handle is red, though ABS is not.
    e.exec("file.open", &json!({ "path": "handle.tenon" })).unwrap();
    let c = e.exec("document.materials", &json!({})).unwrap();
    assert_eq!(
        (c["material"]["name"].as_str(), c["material"]["color"].as_str(), c["color"].as_str()),
        (Some("ABS"), Some("#e8e4d8"), Some("#d04030")),
        "{c}"
    );

    // The assembly reopens and weighs the same.
    let opened = e.exec("asm.open", &json!({ "path": "fittings.tenonasm" })).unwrap();
    assert_eq!(opened["missing"].as_array().map(Vec::len), Some(0), "{opened}");
    let total = e.exec("asm.mass", &json!({})).unwrap();
    assert!(near(total["mass"].as_f64().unwrap(), 69.42, 5e-3), "{total}");
}

#[test]
fn the_checked_in_m6_example_is_what_the_script_writes() {
    // The copies in the repository are compared with a fresh run as `tenon-cli diff` compares
    // files: everything that was put in, to the stored precision. (Face fingerprints are worked
    // out by the kernel and may differ in the last digit from one machine to another; the STEP
    // file holds a timestamp; pictures depend on the renderer.)
    let dir = scratch("same");
    tenon_cli::demo_m6(kernel(), &dir).unwrap();
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m6-fittings");
    for f in ["spring.tenon", "bolt.tenon", "handle.tenon", "nozzle.tenon", "fittings.tenonasm"] {
        let d = tenon_io::diff::diff_files(&example.join(f), &dir.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert!(d.same(), "{f}: regenerate the example (see its README):\n{}", d.text());
    }
    // And the comparison is not blind: the other size of the spring is a different part.
    let mut e = Engine::new(kernel(), &dir);
    e.exec("file.open", &json!({ "path": "spring.tenon" })).unwrap();
    e.exec("table.activate", &json!({ "row": "Stiff" })).unwrap();
    e.exec("file.save", &json!({ "path": "spring-stiff.tenon" })).unwrap();
    let d = tenon_io::diff::diff_files(&example.join("spring.tenon"), &dir.join("spring-stiff.tenon")).unwrap();
    assert!(d.text().contains("active row  Soft -> Stiff"), "{}", d.text());
}

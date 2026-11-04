//! tenon-cli commands against the OpenCASCADE kernel: the M0 demo bracket, STEP info, STL
//! conversion, comparing and upgrading Tenon files, and failure reporting.
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

fn cli(args: &[&std::ffi::OsStr]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_tenon-cli")).args(args).output().expect("run tenon-cli")
}

#[test]
fn diff_says_what_changed_and_exits_like_diff() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let v1 = root.join("crates/io/tests/fixtures/v1/m2-enclosure/enclosure.tenon");
    let v2 = root.join("examples/m2-enclosure/enclosure.tenon");
    // A version-1 file and its upgrade are the same: exit 0.
    let out = cli(&["diff".as_ref(), v1.as_os_str(), v2.as_os_str()]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "No changes.");
    // The box made taller: exit 1, and what changed.
    let dir = scratch("diff");
    let taller = dir.join("taller.tenon");
    let text = std::fs::read_to_string(&v2).unwrap().replace("equation = \"30 mm\"", "equation = \"35 mm\"");
    std::fs::write(&taller, text).unwrap();
    let out = cli(&["diff".as_ref(), v2.as_os_str(), taller.as_os_str()]);
    assert_eq!(out.status.code(), Some(1));
    let shown = String::from_utf8_lossy(&out.stdout);
    assert!(shown.starts_with("Parameters\n  ~ H  30 mm -> 35 mm (outside height)"), "{shown}");
    let out = cli(&["diff".as_ref(), v2.as_os_str(), taller.as_os_str(), "--json".as_ref()]);
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).expect("one JSON document");
    assert_eq!((j["what"].as_str(), j["same"].as_bool()), (Some("part"), Some(false)));
    // A file that is not there: exit 2.
    let out = cli(&["diff".as_ref(), v2.as_os_str(), dir.join("missing.tenon").as_os_str()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("missing.tenon"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn diff_sees_what_optimize_shape_is_asked() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let part = std::fs::read_to_string(root.join("examples/m2-enclosure/enclosure.tenon")).unwrap().replace("\r\n", "\n");
    let dir = scratch("diff-shape");
    let file = |name: &str, target: &str| {
        let path = dir.join(name);
        std::fs::write(&path, format!("{part}\n[[study]]\nid = 1\nname = \"Study1\"\n\n[study.optimize]\n{target}\n")).unwrap();
        path
    };
    let (mass, share, deep) =
        (file("mass.tenon", "grams = 12.5"), file("share.tenon", "remove = 60.0"), file("fine.tenon", "grams = 12.5\nresolution = \"fine\""));
    // The same settings: exit 0.
    let out = cli(&["diff".as_ref(), mass.as_os_str(), mass.as_os_str()]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "No changes.");
    // Settings that are all that differ: exit 1, and what changed.
    let out = cli(&["diff".as_ref(), mass.as_os_str(), share.as_os_str()]);
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));
    let shown = String::from_utf8_lossy(&out.stdout);
    assert!(shown.contains("Study1 [1]  Optimize Shape target come to 12.5 g -> remove 60 %"), "{shown}");
    let out = cli(&["diff".as_ref(), mass.as_os_str(), deep.as_os_str(), "--json".as_ref()]);
    assert_eq!(out.status.code(), Some(1));
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).expect("one JSON document");
    assert_eq!(j["same"].as_bool(), Some(false), "{j}");
    assert_eq!(j["changes"][0]["detail"].as_str(), Some("Optimize Shape resolution coarse -> fine"), "{j}");
    // A file with both targets is refused in words: exit 2.
    let both = file("both.tenon", "grams = 12.5\nremove = 60.0");
    let out = cli(&["diff".as_ref(), mass.as_os_str(), both.as_os_str()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("give `remove` or `grams`, not both"), "{}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn upgrade_rewrites_version_1_files_and_keeps_them() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = scratch("upgrade");
    for f in ["plate.tenon", "pin.tenon", "plate-pins.tenonasm", "plate.tenondrw"] {
        std::fs::copy(root.join("crates/io/tests/fixtures/v1/m4-plate").join(f), dir.join(f)).unwrap();
    }
    let files: Vec<PathBuf> = ["plate.tenon", "plate-pins.tenonasm", "plate.tenondrw"].iter().map(|f| dir.join(f)).collect();
    let r = tenon_cli::upgrade(&files.iter().map(PathBuf::as_path).collect::<Vec<_>>()).unwrap();
    assert_eq!(r.json["files"].as_array().unwrap().iter().filter(|f| f["upgraded"] == true).count(), 3, "{}", r.text);
    for (f, kept) in files.iter().zip(["plate.v1.tenon", "plate-pins.v1.tenonasm", "plate.v1.tenondrw"]) {
        assert!(std::fs::read_to_string(f).unwrap().contains("\nversion = 2\n"), "{}", f.display());
        assert!(dir.join(kept).exists(), "{kept}");
        // The same document as its kept original.
        assert!(tenon_cli::diff(&dir.join(kept), f).unwrap().same(), "{}", f.display());
    }
    // Again: nothing to do.
    let r = tenon_cli::upgrade(&[files[0].as_path()]).unwrap();
    assert!(r.text.ends_with("already current"), "{}", r.text);
    let _ = std::fs::remove_dir_all(&dir);
}

/// True when the mesher and the solver are there. When they are not: a failure where they are
/// required, and otherwise a line saying that the test did not run.
fn programs(test: &str) -> bool {
    match tenon_fea::Tools::find() {
        Ok(_) => true,
        Err(why) => {
            let required = std::env::var("TENON_FEA_REQUIRED").is_ok_and(|v| !v.is_empty() && v != "0");
            assert!(!required, "{test} needs the mesher and the solver, and they are required here: {why}");
            eprintln!("NOT RUN: {test}: {why}");
            false
        }
    }
}

#[test]
fn a_shape_worked_out_twice_has_the_same_mesh_and_nearly_the_same_answer() {
    if !programs("a shape worked out twice") {
        return;
    }
    // A plate with bores, in two processes of their own. The mesher is on one thread, and what is
    // done with what comes back goes in a fixed order, so the mesh is the same to the byte; the
    // solver is on all of them and adds up in the order they finish, so the figures are the same
    // only as far as docs/topology.md says.
    let dir = scratch("twice");
    let script = dir.join("twice.json");
    let steps = serde_json::json!([
        { "run": "model.box", "with": { "width": 80, "depth": 40, "height": 10, "centered": true } },
        { "run": "model.cylinder", "with": { "x": -25, "y": 0, "diameter": 6, "height": 10, "operation": "cut" } },
        { "run": "model.cylinder", "with": { "x": 25, "y": 0, "diameter": 6, "height": 10, "operation": "cut" } },
        { "run": "document.material", "with": { "name": "Steel, mild" } },
        { "run": "model.find", "with": { "what": "faces", "facing": "left" }, "as": "left" },
        { "run": "model.find", "with": { "what": "faces", "facing": "right" }, "as": "right" },
        { "run": "study.new" },
        { "run": "study.fix", "with": { "faces": ["$left.found.0.reference"] } },
        { "run": "study.force", "with": { "faces": ["$right.found.0.reference"], "newtons": 100, "direction": "down" } },
        { "run": "study.optimize", "with": { "keep": "kept", "resolution": "coarse", "iterations": 6, "sharpen": false }, "as": "shape" }
    ]);
    std::fs::write(&script, steps.to_string()).unwrap();
    let once = |name: &str| -> (Vec<u8>, serde_json::Value) {
        let out = dir.join(name);
        let run = std::process::Command::new(env!("CARGO_BIN_EXE_tenon-cli"))
            .args(["run".as_ref(), script.as_os_str(), "--json".as_ref(), "--out".as_ref(), out.as_os_str()])
            .output()
            .expect("run tenon-cli");
        assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
        let said: serde_json::Value = serde_json::from_slice(&run.stdout).unwrap();
        let last = said.as_array().or_else(|| said["steps"].as_array()).unwrap().last().unwrap()["result"].clone();
        (std::fs::read(out.join("kept").join("part.msh")).unwrap(), last)
    };
    let ((mesh_a, a), (mesh_b, b)) = (once("a"), once("b"));
    assert!(mesh_a == mesh_b, "the mesh is the same to the byte");
    assert_eq!((&a["nodes"], &a["elements"], &a["element_size"]), (&b["nodes"], &b["elements"], &b["element_size"]));
    let near = |x: &serde_json::Value, y: &serde_json::Value, within: f64| {
        let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
        assert!((x - y).abs() <= within * x.abs().max(y.abs()), "{x} against {y}");
    };
    let (ha, hb) = (a["history"].as_array().unwrap(), b["history"].as_array().unwrap());
    assert_eq!(ha.len(), hb.len());
    // Each round's work to seven figures over these few rounds (over a whole run it can part by a
    // few in a million); what the shape keeps to about four, as a share rounded to elements can go
    // one element either way.
    for (x, y) in ha.iter().zip(hb) {
        near(&x["compliance"], &y["compliance"], 1e-7);
    }
    near(&a["compliance"]["shape"], &b["compliance"]["shape"], 1e-2);
    near(&a["stiffness_kept"], &b["stiffness_kept"], 1e-2);
    near(&a["surface"]["volume"], &b["surface"]["volume"], 1e-3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_to_keep_the_programs_files_in_is_under_out_as_a_file_is() {
    if !programs("the folders kept") {
        return;
    }
    // A bar held at its end and pushed on the other; a stress check and a shape of it, each
    // leaving the programs' files in a folder named without a path.
    let dir = scratch("keep");
    let (out, working) = (dir.join("out"), dir.join("working"));
    std::fs::create_dir_all(&working).unwrap();
    let script = dir.join("keep.json");
    let steps = serde_json::json!([
        { "run": "model.box", "with": { "width": 60, "depth": 10, "height": 30 } },
        { "run": "document.material", "with": { "name": "Steel, mild" } },
        { "run": "model.find", "with": { "what": "faces", "facing": "left" }, "as": "left" },
        { "run": "model.find", "with": { "what": "faces", "facing": "right" }, "as": "right" },
        { "run": "study.new" },
        { "run": "study.fix", "with": { "faces": ["$left.found.0.reference"] } },
        { "run": "study.force", "with": { "faces": ["$right.found.0.reference"], "newtons": 100, "direction": "down" } },
        { "run": "study.solve", "with": { "keep": "kept_solve" }, "as": "solved" },
        { "run": "study.optimize", "with": { "keep": "kept_shape", "stl": "shape.stl", "element_size": 8, "iterations": 1, "sharpen": false }, "as": "shape" }
    ]);
    std::fs::write(&script, steps.to_string()).unwrap();
    let run = std::process::Command::new(env!("CARGO_BIN_EXE_tenon-cli"))
        .args(["run".as_ref(), script.as_os_str(), "--json".as_ref(), "--out".as_ref(), out.as_os_str()])
        .current_dir(&working)
        .output()
        .expect("run tenon-cli");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    for name in ["kept_solve", "kept_shape"] {
        assert!(out.join(name).join("part.msh").is_file(), "{name} is under --out");
        assert!(!working.join(name).exists(), "{name} is not in the working folder");
    }
    assert!(out.join("shape.stl").is_file());
    assert_eq!(std::fs::read_dir(&working).unwrap().count(), 0, "nothing is left in the working folder");
    let said: serde_json::Value = serde_json::from_slice(&run.stdout).unwrap();
    let last = said.as_array().or_else(|| said["steps"].as_array()).unwrap().last().unwrap().clone();
    let kept = PathBuf::from(last["result"]["kept"].as_str().unwrap());
    assert!(kept.is_absolute() && kept.ends_with("kept_shape"), "{last}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_study_does_not_run_what_lies_in_the_working_folder() {
    // A downloaded folder with programs where a checkout keeps them.
    let dir = std::env::temp_dir().join(format!("tenon-cli-planted-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for programs in [".pixi/envs/fea/Library/bin", ".pixi/envs/fea/bin"] {
        let at = dir.join(programs);
        std::fs::create_dir_all(&at).unwrap();
        for name in ["gmsh", "gmsh.exe", "ccx", "ccx.exe"] {
            std::fs::write(at.join(name), b"not a program, and it could not be started").unwrap();
        }
    }
    let script = dir.join("study.json");
    let steps = r#"[
        { "run": "model.box", "with": { "width": 20, "depth": 10, "height": 10 } },
        { "run": "document.material", "with": { "name": "Aluminium 6061" } },
        { "run": "model.find", "with": { "what": "faces", "facing": "left" }, "as": "l" },
        { "run": "model.find", "with": { "what": "faces", "facing": "right" }, "as": "r" },
        { "run": "study.new", "with": {} },
        { "run": "study.fix", "with": { "faces": ["$l.found.0.reference"] } },
        { "run": "study.force", "with": { "faces": ["$r.found.0.reference"], "newtons": 10, "direction": "down" } },
        { "run": "study.solve", "with": {} }
    ]"#;
    std::fs::write(&script, steps).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_tenon-cli"))
        .args(["run", "study.json"])
        .current_dir(&dir)
        .env_remove("TENON_GMSH")
        .env_remove("TENON_CCX")
        .output()
        .unwrap();
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(!said.contains("cannot start"), "{said}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_step_file_the_reader_dies_on_is_said_to_be_unreadable() {
    let dir = scratch("step-damaged");
    let bracket = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m0-bracket/bracket.step")).unwrap();
    // One reference changed; and a line nested deeper than any stack.
    let changed = dir.join("changed.step");
    std::fs::write(&changed, bracket.replacen("#427", "#527", 1)).unwrap();
    let nested = dir.join("nested.step");
    let head = bracket.split("DATA;").next().unwrap();
    std::fs::write(&nested, format!("{head}DATA;\n#1=A{}{};\nENDSEC;\nEND-ISO-10303-21;\n", "(".repeat(100_000), ")".repeat(100_000))).unwrap();
    let stl = dir.join("out.stl");
    for file in [&changed, &nested] {
        for args in [vec!["info".as_ref(), file.as_os_str()], vec!["convert".as_ref(), file.as_os_str(), stl.as_os_str()]] {
            let out = cli(&args);
            let said = String::from_utf8_lossy(&out.stderr);
            // Read, or told in words: never ended by the fault with nothing said.
            assert!(matches!(out.status.code(), Some(0 | 1)), "{:?}: {} {said}", file.file_name(), out.status);
            if out.status.code() == Some(1) {
                assert!(said.starts_with("error: "), "{said}");
            }
        }
    }
    // A good one still is what it was, in the words it always was.
    let good = cli(&["info".as_ref(), std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m0-bracket/bracket.step").as_os_str()]);
    assert!(good.status.success() && String::from_utf8_lossy(&good.stdout).contains("volume"), "{good:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn upgrade_does_not_call_what_is_no_document_current() {
    let dir = scratch("upgrade-no-document");
    let note = dir.join("note.tenon");
    std::fs::write(&note, b"not a tenon file").unwrap();
    let out = cli(&["upgrade".as_ref(), note.as_os_str()]);
    let said = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains("not a Tenon project"), "{said}");
    // A real one still is.
    let good = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m1-bracket/bracket.tenon");
    let out = cli(&["upgrade".as_ref(), good.as_os_str()]);
    assert!(out.status.success() && String::from_utf8_lossy(&out.stdout).contains("already current"), "{out:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Run with nothing it says how, as the README in the archive tells a person to try, and ends well.
#[test]
fn with_no_arguments_it_says_how_and_no_error() {
    for args in [&[][..], &[std::ffi::OsStr::new("--help")][..]] {
        let out = cli(args);
        assert!(out.status.success(), "{:?}", out.status);
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("usage:") && text.contains("tenon-cli run"), "{text}");
        assert!(out.stderr.is_empty(), "{}", String::from_utf8_lossy(&out.stderr));
    }
    assert!(!cli(&[std::ffi::OsStr::new("nonsense")]).status.success(), "a command it does not know is still an error");
}

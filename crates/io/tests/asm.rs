//! Assembly files: round trip, relative part paths, damaged and mistaken files.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tenon_assembly::{AsmSession, Assembly, Geom, Part, RelKind, Target};
use tenon_io::asm;
use tenon_io::project::ProjectError;
use tenon_model::{Document, OriginPlane};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-io-asm-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn zip_with(body: &[u8]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut zw = zip::ZipWriter::new(&mut buf);
    zw.start_file("project.json", zip::write::SimpleFileOptions::default()).unwrap();
    zw.write_all(body).unwrap();
    zw.finish().unwrap();
    buf.into_inner()
}

/// An assembly of two parts in `dir` (one in a sub-folder), flush on their XY planes.
fn two_parts(dir: &Path) -> AsmSession {
    let mut s = AsmSession::default();
    for (key, name) in [(dir.join("plate.tenon"), "Plate"), (dir.join("parts").join("pin.tenon"), "Pin")] {
        let key = asm::part_key(&key);
        let mut doc = Document::default();
        doc.name = name.into();
        s.add_part(&key, Part::new(doc));
        s.edit(|a, parts| Ok(tenon_assembly::session::insert(a, parts, &key, None, None))).unwrap();
    }
    let xy = |c: u32| Target { component: Some(tenon_assembly::ComponentId(c)), geom: Geom::Plane { plane: OriginPlane::XY } };
    s.edit(|a, parts| tenon_assembly::session::relate(a, parts, RelKind::Flush { a: xy(1), b: xy(2), offset: 7.0 })).unwrap();
    s
}

#[test]
fn assemblies_round_trip_with_part_paths_relative_to_the_file() {
    let dir = scratch("round");
    let s = two_parts(&dir);
    let file = dir.join("pair.tenonasm");
    asm::save(&file, s.assembly()).unwrap();

    // Inside the file (text): relative paths with forward slashes.
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.contains(
            "
format = \"tenon-assembly\"
version = 2
"
        ),
        "{text}"
    );
    let parts: Vec<&str> = text.lines().filter(|l| l.starts_with("part = ")).collect();
    assert_eq!(parts, ["part = \"plate.tenon\"", "part = \"parts/pin.tenon\""], "{text}");

    // Read back elsewhere: the paths follow the file.
    let bytes = std::fs::read(&file).unwrap();
    let other = scratch("other");
    let back = asm::from_bytes(&bytes, &other).unwrap();
    assert_eq!(back.components[1].part, asm::part_key(&other.join("parts").join("pin.tenon")));
    let mut same = back.clone();
    same.map_parts(|p| p.replace(&*other.to_string_lossy(), &dir.to_string_lossy()));
    assert_eq!(same, *s.assembly());

    // The part files are not there: opening marks them missing but keeps the assembly.
    let (opened, parts) = asm::open(&file).unwrap();
    assert_eq!(opened.components.len(), 2);
    assert!(parts.values().all(|p| p.missing.is_some()));
    // The flush moved the pin 7 above the plate.
    assert!((opened.components[1].placement.origin().z - 7.0).abs() < 1e-9);
}

#[test]
fn damaged_and_mistaken_assembly_files_are_refused() {
    let dir = scratch("bad");
    let s = two_parts(&dir);
    let good = asm::to_bytes(s.assembly(), &dir).unwrap();
    assert!(asm::from_bytes(&good, &dir).is_ok());
    // A part file is not an assembly, and the other way round.
    let part = tenon_io::project::to_bytes(&Document::default(), &serde_json::Map::new()).unwrap();
    assert!(matches!(asm::from_bytes(&part, &dir), Err(ProjectError::NotAProject(m)) if m.contains("part")));
    assert!(matches!(tenon_io::project::from_bytes(&good), Err(ProjectError::NotAProject(m)) if m.contains("assembly")));
    // Newer than this build, as a zip or as text.
    let too_new = json!({ "format": "tenon-assembly", "version": 3, "assembly": Assembly::default() });
    assert!(matches!(asm::from_bytes(&zip_with(&serde_json::to_vec(&too_new).unwrap()), &dir), Err(ProjectError::TooNew(3))));
    let text = String::from_utf8(good.clone()).unwrap().replace(
        "
version = 2
",
        "
version = 3
",
    );
    assert!(matches!(asm::from_bytes(text.as_bytes(), &dir), Err(ProjectError::TooNew(3))));
    // A relationship pointing at a missing component, and a placement that is not a rotation.
    let mut v: Value = serde_json::to_value(s.assembly()).unwrap();
    v["relationships"][0]["kind"]["b"]["component"] = json!(42);
    let file = json!({ "format": "tenon-assembly", "version": 1, "assembly": v });
    assert!(matches!(asm::from_bytes(&zip_with(&serde_json::to_vec(&file).unwrap()), &dir), Err(ProjectError::Damaged(_))));
    let mut v: Value = serde_json::to_value(s.assembly()).unwrap();
    v["components"][0]["placement"]["z"] = json!({ "x": 0.0, "y": 0.0, "z": 3.0 });
    let file = json!({ "format": "tenon-assembly", "version": 1, "assembly": v });
    assert!(matches!(asm::from_bytes(&zip_with(&serde_json::to_vec(&file).unwrap()), &dir), Err(ProjectError::Damaged(_))));
    let mut v: Value = serde_json::to_value(s.assembly()).unwrap();
    v["components"][0]["placement"]["origin"]["x"] = json!(1e300);
    let file = json!({ "format": "tenon-assembly", "version": 1, "assembly": v });
    assert!(asm::from_bytes(&zip_with(&serde_json::to_vec(&file).unwrap()), &dir).is_err());
    // Truncated.
    assert!(asm::from_bytes(&good[..good.len() / 2], &dir).is_err());
    // Missing file through the command; the session is unchanged.
    let mut t = AsmSession::default();
    assert!(asm::run(&mut t, "asm.open", &json!({ "path": dir.join("nope.tenonasm").to_str().unwrap() }), None).is_err());
    assert_eq!(*t.assembly(), Assembly::default());
}

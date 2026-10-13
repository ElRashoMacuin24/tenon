//! Scenes kept on disk (DEC-091), with the worker and the real kernel: a part opened again is
//! shown at once from what was kept of it, and the rebuild behind that confirms it or puts it
//! right.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use tenon_io::scenes::SceneFiles;
use tenon_kernel::{Kernel, MeshTol};
use tenon_kernel_occt::OcctKernel;
use tenon_model::worker::{Response, SceneStore, Worker};
use tenon_model::{Document, Scene};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-io-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn example(file: &str) -> Document {
    tenon_io::project::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(file)).unwrap().0
}

/// A run of the program: a worker with a kernel of its own, keeping scenes in `dir`.
fn run(dir: &Path) -> Worker {
    let w = Worker::spawn(|| Box::new(OcctKernel::new()) as Box<dyn Kernel>, MeshTol::default(), None).unwrap();
    w.keep_scenes(Box::new(SceneFiles::at(dir)));
    w
}

fn next(w: &Worker) -> Response {
    w.recv_timeout(Duration::from_secs(300)).expect("the worker answers")
}

fn scene_of(r: Response, slot: u64, revision: u64) -> Scene {
    match r {
        Response::Scene { slot: s, revision: v, scene } if (s, v) == (slot, revision) => *scene,
        other => panic!("expected the scene of {slot} at {revision}, got {other:?}"),
    }
}

fn early_of(r: Response, slot: u64, revision: u64) -> Scene {
    match r {
        Response::Early { slot: s, revision: v, scene } if (s, v) == (slot, revision) => *scene,
        other => panic!("expected what was kept of {slot} at {revision}, got {other:?}"),
    }
}

fn kept_files(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |d| d.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "scene")).count())
}

#[test]
fn a_part_opened_again_is_shown_at_once_from_what_was_kept_and_the_rebuild_confirms_it() {
    let dir = scratch("scenes-kept");
    // A part with a shell, fillets, sketches on faces and work features.
    let doc = example("m2-enclosure/enclosure.tenon");

    // The first time there is nothing kept: the part is rebuilt, and what it comes to is kept.
    let w = run(&dir);
    w.regenerate_kept(1, 7, doc.clone());
    let first = scene_of(next(&w), 1, 7);
    assert!(!first.bodies.is_empty() && first.far && !first.sketch_frames.is_empty() && !first.work.is_empty() && !first.status.is_empty());
    assert!(first.bodies[0].far.is_some(), "a part of an assembly comes with its mesh for drawing from far away");
    drop(w);
    assert_eq!(kept_files(&dir), 1);
    // What was kept reads back as the very scene, to the last bit of every number.
    let kept = SceneFiles::at(&dir).get(&doc, &MeshTol::default(), true).unwrap();
    assert!(kept.same_as(&first));
    // So do the other examples': a modelled thread, a loft, a coil, sheet metal.
    let w = run(&scratch("scenes-others"));
    for (i, file) in ["m6-fittings/bolt.tenon", "m6-fittings/nozzle.tenon", "m6-fittings/spring.tenon", "m8-tray/tray.tenon"].iter().enumerate() {
        let part = example(file);
        w.regenerate_slot(1, i as u64, part, false);
        let scene = scene_of(next(&w), 1, i as u64);
        assert!(!scene.bodies.is_empty() && (i != 0 || !scene.threads.is_empty()), "{file}");
        let back = tenon_io::scenes::decode(&tenon_io::scenes::encode(&scene).unwrap_or_else(|| panic!("{file} is not written")))
            .unwrap_or_else(|| panic!("{file} does not read back"));
        assert!(back.same_as(&scene), "{file}");
    }
    drop(w);

    // Opened again in another run of the program: shown at once from what was kept, and then
    // the rebuild says that is what the part comes to.
    let w = run(&dir);
    w.regenerate_kept(1, 3, doc.clone());
    assert!(early_of(next(&w), 1, 3).same_as(&first));
    assert!(matches!(next(&w), Response::Confirmed { slot: 1, revision: 3 }), "the same part, rebuilt, is the same scene");
    // The slot shows the part now. A change of it is rebuilt as ever, with nothing shown
    // from what was kept; nor is anything shown early for a document asked for again.
    let mut renamed = doc.clone();
    renamed.name = "Another name".into();
    w.regenerate_slot(1, 4, renamed, false);
    assert!(scene_of(next(&w), 1, 4).same_as(&first));
    w.regenerate_kept(1, 5, doc.clone());
    assert!(scene_of(next(&w), 1, 5).same_as(&first));
    // The part on its own (slot 0) has no far meshes, and is kept apart from the above.
    w.regenerate_kept(0, 6, doc.clone());
    let alone = scene_of(next(&w), 0, 6);
    assert!(!alone.far && alone.bodies[0].far.is_none());
    drop(w);
    assert_eq!(kept_files(&dir), 2);
    let w = run(&dir);
    w.regenerate_kept(0, 1, doc.clone());
    assert!(early_of(next(&w), 0, 1).same_as(&alone));
    assert!(matches!(next(&w), Response::Confirmed { slot: 0, revision: 1 }));
    drop(w);

    // What was kept is never trusted. Say the wrong scene lies under this document (another
    // part's, as a fault in the program might leave it): it is shown, the rebuild finds the
    // part comes to something else, the right scene follows, and is what is kept from then on.
    let other = example("m1-bracket/bracket.tenon");
    let w = run(&dir);
    w.regenerate_slot(2, 1, other, false);
    let wrong = scene_of(next(&w), 2, 1);
    assert!(!wrong.same_as(&first));
    drop(w);
    SceneFiles::at(&dir).put(&doc, &MeshTol::default(), true, &wrong);
    let w = run(&dir);
    w.regenerate_kept(1, 9, doc.clone());
    assert!(early_of(next(&w), 1, 9).same_as(&wrong));
    assert!(scene_of(next(&w), 1, 9).same_as(&first), "put right by the rebuild");
    drop(w);
    let w = run(&dir);
    w.regenerate_kept(1, 2, doc.clone());
    assert!(early_of(next(&w), 1, 2).same_as(&first));
    assert!(matches!(next(&w), Response::Confirmed { slot: 1, revision: 2 }));
    drop(w);

    // Without a place to keep scenes, and for a part that is not as it is on disk, nothing is
    // kept and nothing shown early.
    let nowhere = scratch("scenes-nowhere");
    let w = Worker::spawn(|| Box::new(OcctKernel::new()) as Box<dyn Kernel>, MeshTol::default(), None).unwrap();
    w.regenerate_kept(1, 1, doc.clone());
    assert!(scene_of(next(&w), 1, 1).same_as(&first));
    drop(w);
    let w = run(&nowhere);
    w.regenerate_slot(1, 1, doc, false);
    assert!(scene_of(next(&w), 1, 1).same_as(&first));
    drop(w);
    assert_eq!(kept_files(&nowhere), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

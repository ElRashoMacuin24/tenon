//! The control port, driven as an agent drives it: over a TCP connection, a JSON object a
//! line, while the workbench goes on taking frames.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::time::Duration;

use egui::vec2;
use serde_json::{Value, json};
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;

use crate::tests::Driver;
use crate::{Services, Workbench};

/// A connection to a workbench's control port.
struct Client {
    out: TcpStream,
    reader: BufReader<TcpStream>,
    token: String,
    next: u64,
}

impl Client {
    fn to(wb: &Workbench) -> Client {
        let (port, token) = wb.control_address().expect("the port is open");
        Client::with_token(port, token)
    }

    fn with_token(port: u16, token: &str) -> Client {
        let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_millis(2))).unwrap();
        Client { out: stream.try_clone().unwrap(), reader: BufReader::new(stream), token: token.to_owned(), next: 0 }
    }

    /// Sends a line and takes frames until its answer has come.
    fn send(&mut self, wb: &mut Workbench, d: &mut Driver, line: &str) -> Value {
        writeln!(self.out, "{line}").unwrap();
        let mut answer = String::new();
        for _ in 0..20_000 {
            d.frame(wb, vec![]);
            // (What a read that ran out of time had already read stays in `answer`.)
            match self.reader.read_line(&mut answer) {
                Ok(n) if n > 0 && answer.ends_with('\n') => return serde_json::from_str(&answer).unwrap_or_else(|e| panic!("{answer}: {e}")),
                Ok(0) => panic!("the port closed the connection after {line}"),
                _ => {}
            }
        }
        panic!("no answer to {line}");
    }

    fn ask(&mut self, wb: &mut Workbench, d: &mut Driver, run: &str, with: Value) -> Result<Value, String> {
        self.next += 1;
        let answer = self.send(wb, d, &json!({ "id": self.next, "token": self.token, "run": run, "with": with }).to_string());
        assert_eq!(answer["id"], self.next, "{answer}");
        if answer["ok"] == true { Ok(answer["result"].clone()) } else { Err(answer["error"].as_str().unwrap().to_owned()) }
    }

    fn ok(&mut self, wb: &mut Workbench, d: &mut Driver, run: &str, with: Value) -> Value {
        self.ask(wb, d, run, with.clone()).unwrap_or_else(|e| panic!("{run} {with}: {e}"))
    }
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-ui-control-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn names(wb: &Workbench) -> Vec<String> {
    wb.document().features().iter().map(|f| f.name.clone()).collect()
}

/// The steps of "round the four upright corners 3 mm": the edges found by what they are.
fn round_the_corners() -> Value {
    json!([
        { "run": "model.find", "with": { "what": "edges", "kind": "line", "along": "z" }, "as": "up" },
        { "run": "model.fillet", "with": { "radius": 3, "edges": ["$up.found.0.reference", "$up.found.1.reference", "$up.found.2.reference", "$up.found.3.reference"] } },
    ])
}

#[test]
fn the_control_port_runs_commands_in_the_open_part_for_whoever_has_the_token() {
    let dir = scratch("sync");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1200.0, 800.0));
    assert!(wb.control_address().is_none(), "the port is closed unless it is asked for");
    let (port, token) = wb.listen(0, None).unwrap();
    assert!(port > 0 && token.len() == 32 && token.chars().all(|c| c.is_ascii_hexdigit()), "{token}");
    assert_eq!(wb.control_address(), Some((port, token.as_str())));
    // The window says that it is open.
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t.contains(&format!("Control port {port} open"))), "{:?}", d.texts());

    // Without the token nothing is done, whatever is asked; nor with half a request.
    let mut stranger = Client::with_token(port, "0123456789abcdef0123456789abcdef");
    let told = stranger.ask(&mut wb, &mut d, "model.box", json!({ "width": 1, "depth": 1, "height": 1 })).unwrap_err();
    assert!(told.contains("wrong or missing `token`"), "{told}");
    let none = stranger.send(&mut wb, &mut d, &json!({ "id": 2, "run": "control.state" }).to_string());
    assert_eq!((none["ok"].as_bool(), none["id"].as_u64()), (Some(false), Some(2)));
    assert!(names(&wb).is_empty());
    let mut c = Client::to(&wb);
    let garbled = c.send(&mut wb, &mut d, "not json at all");
    assert!(garbled["error"].as_str().unwrap().contains("one JSON object on a line"), "{garbled}");
    let no_run = c.send(&mut wb, &mut d, &json!({ "id": 1, "token": token }).to_string());
    assert!(no_run["error"].as_str().unwrap().contains("needs `run`"), "{no_run}");

    // What is open, and how it stands.
    let state = c.ok(&mut wb, &mut d, "control.state", json!({}));
    assert_eq!((state["document"].as_str(), state["panel_open"].as_bool(), state["sketching"].as_bool()), (Some("part"), Some(false), Some(false)));
    assert_eq!(state["schema_version"], tenon_model::schema::VERSION);
    assert!(state["unsaved"].as_array().unwrap().is_empty());

    // A command is run on the part that is open: it is there in the window, and says so.
    let made = c.ok(&mut wb, &mut d, "model.box", json!({ "width": 60, "depth": 40, "height": 8 }));
    assert_eq!(made["name"], "Extrusion1");
    assert_eq!(names(&wb), ["Sketch1", "Extrusion1"]);
    assert_eq!(wb.status(), "Remote: model.box");
    d.settle(&mut wb);
    assert_eq!(wb.scene().bodies.len(), 1);
    let later = c.ok(&mut wb, &mut d, "control.state", json!({}));
    assert_ne!(later["revision"], state["revision"]);
    assert_eq!(later["unsaved"].as_array().unwrap().len(), 1);
    // Held to its schema first, as anything from outside is.
    let told = c.ask(&mut wb, &mut d, "model.box", json!({ "width": 1, "depth": 1, "hight": 1 })).unwrap_err();
    assert_eq!(told, "model.box has no parameter `hight`: did you mean `height`?");
    assert!(c.ask(&mut wb, &mut d, "model.nonsense", json!({})).unwrap_err().contains("unknown command"));
    // A question that needs the kernel is answered too.
    let mass = c.ok(&mut wb, &mut d, "model.mass", json!({}));
    assert_eq!(mass["bodies"][0]["volume"].as_f64(), Some(60.0 * 40.0 * 8.0));

    // Several as one: one step in the window's history, which the person there can take back.
    let done = c.ok(&mut wb, &mut d, "batch.run", json!({ "steps": round_the_corners() }));
    assert_eq!((done["steps"].as_array().unwrap().len(), done["as_one"].as_bool()), (2, Some(true)));
    assert_eq!(names(&wb), ["Sketch1", "Extrusion1", "Fillet1"]);
    wb.exec("edit.undo", json!({})).unwrap();
    assert_eq!(names(&wb), ["Sketch1", "Extrusion1"]);
    wb.exec("edit.redo", json!({})).unwrap();
    // A step that fails takes the steps before it back.
    let told = c
        .ask(
            &mut wb,
            &mut d,
            "batch.run",
            json!({ "steps": [
                { "run": "feature.rename", "with": { "feature": 3, "name": "Corners" } },
                { "run": "model.cylinder", "with": { "diameter": 10, "height": 0 } },
            ] }),
        )
        .unwrap_err();
    assert!(
        told.contains("step 2 (model.cylinder)") && told.contains("Nothing was changed") && told.contains("1 step(s) before it were taken back"),
        "{told}"
    );
    assert_eq!(names(&wb), ["Sketch1", "Extrusion1", "Fillet1"]);
    let told = c.ask(&mut wb, &mut d, "batch.run", json!({ "steps": [{ "run": "edit.undo" }] })).unwrap_err();
    assert!(told.contains("edit.undo cannot be one of several steps run as one"), "{told}");
    let stand = c
        .ask(
            &mut wb,
            &mut d,
            "batch.run",
            json!({ "as_one": false, "steps": [{ "run": "feature.rename", "with": { "feature": 3, "name": "Corners" } }, { "run": "feature.rename", "with": { "feature": 99, "name": "x" } }] }),
        )
        .unwrap_err();
    assert!(stand.contains("The 1 step(s) before it stand"), "{stand}");
    assert_eq!(names(&wb)[2], "Corners");

    // A picture of what the window shows, with things picked out; it waits for the part to
    // have been rebuilt, so it shows the corners rounded.
    let plain = c.ok(&mut wb, &mut d, "render.png", json!({ "width": 240, "height": 180 }));
    assert!(plain["png_base64"].as_str().unwrap().starts_with("iVBORw0KGgo"), "base64 of the PNG signature");
    assert_eq!(wb.scene().bodies[0].faces.len(), 10, "six sides and four rounds");
    let marked = c.ok(&mut wb, &mut d, "render.png", json!({ "width": 240, "height": 180, "highlight_faces": [0], "highlight_edges": [0] }));
    assert_ne!(marked["png_base64"], plain["png_base64"]);
    let as_shown = c.ok(&mut wb, &mut d, "render.png", json!({ "view": "current", "width": 240, "height": 180 }));
    assert!(as_shown["png_base64"].as_str().is_some());
    let file = dir.join("shot.png");
    let written = c.ok(&mut wb, &mut d, "render.png", json!({ "view": "top", "path": file.to_string_lossy(), "width": 320, "height": 240 }));
    assert_eq!(written["bytes"].as_u64(), Some(std::fs::metadata(&file).unwrap().len()));
    assert!(c.ask(&mut wb, &mut d, "render.png", json!({ "highlight_faces": [99] })).unwrap_err().contains("has no face 99"));
    assert!(c.ask(&mut wb, &mut d, "render.png", json!({ "exploded": true })).unwrap_err().contains("has no parameter `exploded`"));

    // While a command's panel is open in the app, the part is looked at but not changed.
    wb.run_ui("model.fillet").unwrap();
    assert!(wb.panel.is_some());
    assert_eq!(c.ok(&mut wb, &mut d, "control.state", json!({}))["panel_open"], true);
    let told = c.ask(&mut wb, &mut d, "feature.rename", json!({ "feature": 3, "name": "No" })).unwrap_err();
    assert!(told.contains("while a command's panel is open"), "{told}");
    assert!(c.ok(&mut wb, &mut d, "model.tree", json!({}))["features"].as_array().is_some());
    wb.panel = None;

    // Unsaved work is not thrown away from outside: saved first, then a new part.
    let told = c.ask(&mut wb, &mut d, "file.new", json!({})).unwrap_err();
    assert!(told.contains("file.new would lose unsaved changes") && told.contains("save first"), "{told}");
    assert_eq!(names(&wb).len(), 3);
    let part = dir.join("plate.tenon");
    c.ok(&mut wb, &mut d, "file.save", json!({ "path": part.to_string_lossy() }));
    assert!(part.exists());
    let fresh = c.ok(&mut wb, &mut d, "file.new", json!({ "name": "Second" }));
    assert_eq!((fresh["name"].as_str(), fresh["unsaved"].as_array().map(Vec::len)), (Some("Second"), Some(0)));
    assert!(names(&wb).is_empty());
    let opened = c.ok(&mut wb, &mut d, "file.open", json!({ "path": part.to_string_lossy() }));
    assert_eq!(opened["document"], "part");
    assert_eq!(names(&wb), ["Sketch1", "Extrusion1", "Corners"]);
    assert!(c.ask(&mut wb, &mut d, "asm.new", json!({})).unwrap_err().contains("start it in the app"));
    assert!(c.ask(&mut wb, &mut d, "asm.tree", json!({})).unwrap_err().contains("no assembly is open"));

    // Requests are done in the order they come, each answered under its own id.
    let lines: String = (0..5)
        .map(|i| {
            json!({ "id": 100 + i, "token": token, "run": "feature.rename", "with": { "feature": 3, "name": format!("Name {i}") } }).to_string()
                + "\n"
        })
        .collect();
    c.out.write_all(lines.as_bytes()).unwrap();
    let mut got = String::new();
    for _ in 0..20_000 {
        d.frame(&mut wb, vec![]);
        let _ = c.reader.read_line(&mut got);
        if got.lines().count() == 5 && got.ends_with('\n') {
            break;
        }
    }
    let ids: Vec<u64> = got.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()["id"].as_u64().unwrap()).collect();
    assert_eq!(ids, [100, 101, 102, 103, 104]);
    assert_eq!(names(&wb)[2], "Name 4");
    assert!(!wb.control_busy());

    // The workbench gone, the port is gone with it.
    drop(wb);
    writeln!(c.out, "{}", json!({ "id": 1, "token": token, "run": "control.state" })).unwrap();
    c.reader.get_ref().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut rest = String::new();
    assert_eq!(c.reader.read_line(&mut rest).unwrap_or(0), 0, "{rest}");
}

#[test]
fn what_needs_the_kernel_runs_on_the_geometry_thread_and_comes_back_as_one_edit() {
    // As the app is: the kernel on its own thread, the window never waiting for it.
    let mut wb = Workbench::new(|| Box::new(OcctKernel::new()) as Box<dyn Kernel>, None, Services::default());
    let mut d = Driver::new(vec2(1200.0, 800.0));
    wb.listen(0, None).unwrap();
    let mut c = Client::to(&wb);

    // No kernel needed: done at once.
    c.ok(&mut wb, &mut d, "model.box", json!({ "width": 60, "depth": 40, "height": 8 }));
    assert_eq!(names(&wb), ["Sketch1", "Extrusion1"]);
    // Questions that need it are put to the geometry thread, about the part as it stands.
    let mass = c.ok(&mut wb, &mut d, "model.mass", json!({}));
    assert_eq!(mass["bodies"][0]["volume"].as_f64(), Some(60.0 * 40.0 * 8.0));
    let top = c.ok(&mut wb, &mut d, "model.find", json!({ "what": "faces", "facing": "top" }));
    assert_eq!(top["count"], 1);
    assert_eq!(names(&wb).len(), 2, "looking changed nothing");

    // A command that needs it and changes the part: worked out there, and put into the part
    // here as one edit when it comes back.
    let sketch = c.ok(&mut wb, &mut d, "sketch.create", json!({ "face": top["found"][0]["reference"], "project_edges": true }));
    assert_eq!(sketch["projected"].as_array().map(Vec::len), Some(4), "the face's four edges, looked up by the kernel: {sketch}");
    assert_eq!(names(&wb), ["Sketch1", "Extrusion1", "Sketch2"]);
    assert_eq!(wb.status(), "Remote: sketch.create");
    wb.exec("edit.undo", json!({})).unwrap();
    assert_eq!(names(&wb).len(), 2);

    // Steps as one, with one that needs the kernel among them.
    c.ok(&mut wb, &mut d, "batch.run", json!({ "steps": round_the_corners() }));
    assert_eq!(names(&wb), ["Sketch1", "Extrusion1", "Fillet1"]);
    let rounded = c.ok(&mut wb, &mut d, "model.mass", json!({}))["bodies"][0]["volume"].as_f64().unwrap();
    let expected = (60.0 * 40.0 - (4.0 - std::f64::consts::PI) * 9.0) * 8.0;
    assert!((rounded - expected).abs() < 1e-6 * expected, "{rounded} vs {expected}");
    wb.exec("edit.undo", json!({})).unwrap();
    assert_eq!(names(&wb), ["Sketch1", "Extrusion1"], "one step");
    wb.exec("edit.redo", json!({})).unwrap();
    // A failure among them, after a step that went to the geometry thread: all taken back.
    let mut steps = round_the_corners();
    steps.as_array_mut().unwrap().push(json!({ "run": "model.sphere", "with": { "diameter": -1 } }));
    steps[1]["with"]["radius"] = json!(1);
    steps[0]["with"]["most"] = json!("left");
    steps[1]["with"]["edges"] = json!(["$up.found.0.reference"]);
    let told = c.ask(&mut wb, &mut d, "batch.run", json!({ "steps": steps })).unwrap_err();
    assert!(told.contains("step 3 (model.sphere)") && told.contains("2 step(s) before it were taken back"), "{told}");
    assert_eq!(names(&wb), ["Sketch1", "Extrusion1", "Fillet1"]);

    // A picture waits for the window to have the part as it now is.
    let picture = c.ok(&mut wb, &mut d, "render.png", json!({ "view": "top", "width": 200, "height": 150 }));
    assert!(picture["png_base64"].as_str().unwrap().len() > 400);
    assert!(wb.is_settled());
    assert_eq!(wb.scene().bodies[0].faces.len(), 10, "the picture is of the part with its corners rounded");
}

/// An example's files, copied so the test may change them.
fn example(name: &str, files: &[&str], to: &std::path::Path) {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(name);
    std::fs::create_dir_all(to).unwrap();
    for f in files {
        std::fs::copy(src.join(f), to.join(f)).unwrap();
    }
}

#[test]
fn an_assembly_and_a_drawing_are_driven_once_their_geometry_has_come_back() {
    let dir = scratch("control-assembly-drawing");
    let (pivot, plate) = (dir.join("pivot"), dir.join("plate"));
    example("m3-pivot", &["pivot.tenonasm", "base.tenon", "arm.tenon", "pin.tenon", "block.tenon"], &pivot);
    example("m4-plate", &["plate.tenon", "pin.tenon", "plate-pins.tenonasm", "plate.tenondrw"], &plate);
    std::fs::copy(pivot.join("block.tenon"), pivot.join("weight.tenon")).unwrap();
    let path = |p: &std::path::Path| p.to_string_lossy().into_owned();

    // What they come to with the kernel at hand, to hold the app to.
    let mut plain = Workbench::headless(Box::new(OcctKernel::new()));
    plain.open(&pivot.join("pivot.tenonasm")).unwrap();
    let whole = plain.asm_exec("asm.mass", json!({})).unwrap();
    let block = whole["components"].as_array().unwrap().iter().find(|c| c["name"].as_str().unwrap().starts_with("block")).unwrap()["volume"]
        .as_f64()
        .unwrap();
    let whole = whole["volume"].as_f64().unwrap();
    assert!(whole > 0.0 && block > 0.0);

    // As the app is: the kernel on its own thread, an assembly's parts rebuilt there.
    let mut wb = Workbench::new(|| Box::new(OcctKernel::new()) as Box<dyn Kernel>, None, Services::default());
    let mut d = Driver::new(vec2(1200.0, 800.0));
    wb.listen(0, None).unwrap();
    let mut c = Client::to(&wb);
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9 * b.abs().max(1.0);

    // Opened and asked about at once: the question waits for the parts.
    let state = c.ok(&mut wb, &mut d, "asm.open", json!({ "path": path(&pivot.join("pivot.tenonasm")) }));
    assert_eq!(state["document"], "assembly");
    let mass = c.ok(&mut wb, &mut d, "asm.mass", json!({}));
    assert!(near(mass["volume"].as_f64().unwrap(), whole), "{mass} vs {whole}");
    // A part put in and weighed in the same breath, as one step: the weighing waits for it.
    let done = c.ok(
        &mut wb,
        &mut d,
        "batch.run",
        json!({ "steps": [
            { "run": "asm.insert", "with": { "path": path(&pivot.join("weight.tenon")), "at": [200, 0, 0] }, "as": "put" },
            { "run": "asm.mass" },
        ] }),
    );
    assert!(near(done["steps"][1]["result"]["volume"].as_f64().unwrap(), whole + block), "{done}");
    assert_eq!(wb.status(), "Remote: asm.insert");
    let count = wb.assembly().unwrap().assembly().components.len();
    wb.asm_exec("asm.undo", json!({})).unwrap();
    assert_eq!(wb.assembly().unwrap().assembly().components.len(), count - 1, "the person takes it back");
    // What is not run over the port yet says so, and where it can be run.
    let told = c.ask(&mut wb, &mut d, "asm.interference", json!({})).unwrap_err();
    assert!(told.contains("is not run over the control port yet"), "{told}");
    // A part's command is told what is open.
    let told = c.ask(&mut wb, &mut d, "model.box", json!({ "width": 1, "depth": 1, "height": 1 })).unwrap_err();
    assert!(told.contains("an assembly is open"), "{told}");

    // A drawing: its views are drawn on the geometry thread, and what asks about them waits.
    let mut plain = Workbench::headless(Box::new(OcctKernel::new()));
    plain.open(&plate.join("plate.tenondrw")).unwrap();
    let was = plain.drw_exec("drw.info", json!({})).unwrap();
    let views = was["views"].as_array().unwrap().len();
    let lines = |v: &Value| v["visible_curves"].as_u64().unwrap();
    assert!(views > 0 && lines(&was["views"][0]) > 0);

    // (The assembly has been changed and changed back, which the app counts as unsaved: it is
    // not thrown away from outside.)
    let told = c.ask(&mut wb, &mut d, "drw.open", json!({ "path": path(&plate.join("plate.tenondrw")) })).unwrap_err();
    assert!(told.contains("would lose unsaved changes (pivot.tenonasm)"), "{told}");
    c.ok(&mut wb, &mut d, "asm.save", json!({ "path": path(&pivot.join("pivot.tenonasm")) }));
    let state = c.ok(&mut wb, &mut d, "drw.open", json!({ "path": path(&plate.join("plate.tenondrw")) }));
    assert_eq!(state["document"], "drawing");
    let info = c.ok(&mut wb, &mut d, "drw.info", json!({}));
    assert_eq!(info["views"].as_array().unwrap().len(), views);
    assert_eq!(lines(&info["views"][0]), lines(&was["views"][0]), "the views were drawn before they were counted");
    // A view added and counted as one step, and the sheet written out after it.
    let parent = info["views"][0]["id"].clone();
    let svg = dir.join("sheet.svg");
    let done = c.ok(
        &mut wb,
        &mut d,
        "batch.run",
        json!({ "steps": [
            { "run": "drw.view.projected", "with": { "parent": parent, "side": "above_right", "at": [300, 200] }, "as": "v" },
            { "run": "drw.info" },
            { "run": "drw.export.svg", "with": { "path": path(&svg) } },
        ] }),
    );
    let after = &done["steps"][1]["result"]["views"];
    assert_eq!(after.as_array().unwrap().len(), views + 1);
    let added = after.as_array().unwrap().iter().find(|v| v["id"] == done["steps"][0]["result"]["view"]).unwrap_or_else(|| panic!("{done}"));
    assert!(lines(added) > 0 && added["error"].is_null(), "the new view was drawn before it was counted: {added}");
    assert!(std::fs::read_to_string(&svg).unwrap().contains("<svg"));
    wb.drw_exec("drw.undo", json!({})).unwrap();
    assert_eq!(wb.drawing().unwrap().drawing().views.len(), views, "one step, taken back");

    // A base view of a model the drawing has not seen, with no scale said: the model is built
    // on the geometry thread to see how large it is, and the view fits the sheet as it does
    // when the command has the kernel at hand.
    let model = path(&pivot.join("block.tenon"));
    let fits = plain.drw_exec("drw.view.base", json!({ "model": model, "orientation": "top" })).unwrap()["scale"].as_f64().unwrap();
    assert!(fits > 1.0, "a small part is drawn larger than life: {fits}");
    let placed = c.ok(&mut wb, &mut d, "drw.view.base", json!({ "model": model, "orientation": "top" }));
    assert_eq!(placed["scale"].as_f64(), Some(fits));
    assert_eq!(wb.status(), "Remote: drw.view.base");
    let told = c.ask(&mut wb, &mut d, "drw.view.base", json!({ "model": path(&dir.join("nothing.tenon")) })).unwrap_err();
    assert!(told.contains("cannot open"), "{told}");
    let told = c.ask(&mut wb, &mut d, "drw.view.base", json!({ "model": model, "orientation": "sideways" })).unwrap_err();
    assert!(told.contains("`orientation` must be"), "{told}");
}

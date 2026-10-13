//! M10: an agent attached to the running app. The MCP server speaks to a workbench's control
//! port as `tenon-cli mcp --attach` does, and what it does is done in that workbench: in its
//! document, in its window, in its undo history.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use tenon_cli::mcp::Server;
use tenon_cli::remote::Remote;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;
use tenon_ui::{Services, Workbench};

/// Calls a tool; its data, or (when the tool says it failed) the reason.
fn tool(server: &mut Server, name: &str, args: Value) -> Result<Value, String> {
    let r = server.handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": args } })).unwrap();
    assert!(r.get("error").is_none(), "{name}: {r}");
    let r = &r["result"];
    if r["isError"] == true {
        return Err(r["content"][0]["text"].as_str().unwrap_or_default().to_owned());
    }
    Ok(r.get("structuredContent").cloned().unwrap_or_else(|| r["content"][0].clone()))
}

#[test]
fn an_agent_attached_to_the_running_app_works_in_its_window() {
    // The app: a workbench with the kernel on its own thread, its control port open.
    let mut wb = Workbench::new(|| Box::new(OcctKernel::new()) as Box<dyn Kernel>, None, Services::default());
    let (port, token) = wb.listen(0, None).unwrap();

    // The agent, on a thread of its own as it is a process of its own: the MCP server, attached.
    let agent = std::thread::spawn(move || {
        assert!(Remote::connect(port, "not the token").is_err_and(|e| e.contains("wrong or missing `token`")));
        let mut server = Server::on(Box::new(Remote::connect(port, token).unwrap()));
        let ok = |server: &mut Server, name: &str, args: Value| tool(server, name, args.clone()).unwrap_or_else(|e| panic!("{name} {args}: {e}"));

        // A plate with a hole and rounded corners, in words, as one step.
        let done = ok(
            &mut server,
            "run_commands",
            json!({ "steps": [
                { "run": "model.box", "with": { "width": 60, "depth": 40, "height": 8 } },
                { "run": "model.cylinder", "with": { "x": 30, "y": 20, "diameter": 10, "height": 8, "operation": "cut" } },
                { "run": "model.find", "with": { "what": "edges", "kind": "line", "along": "z" }, "as": "up" },
                { "run": "model.fillet", "with": { "radius": 3, "edges": ["$up.found.0.reference", "$up.found.1.reference", "$up.found.2.reference", "$up.found.3.reference"] } },
            ] }),
        );
        assert_eq!(done["steps"].as_array().unwrap().len(), 4);
        // It looks at what it made: the tree, the measure, a picture of the window.
        let tree = ok(&mut server, "model_tree", json!({}));
        assert!(tree["error"].is_null(), "{tree}");
        let volume: f64 = ok(&mut server, "measure", json!({}))["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum();
        let picture = ok(&mut server, "render_png", json!({ "width": 200, "height": 150 }));
        assert_eq!(picture["mimeType"], "image/png");
        assert!(picture["data"].as_str().unwrap().starts_with("iVBORw0KGgo"));
        // A mistake comes back as one, by name, and changes nothing.
        let told = tool(&mut server, "run_command", json!({ "command": "model.shell", "params": { "thicknes": 1 } })).unwrap_err();
        assert!(told.contains("did you mean `thickness`?"), "{told}");
        let top = ok(&mut server, "find_geometry", json!({ "what": "faces", "facing": "top" }));
        ok(&mut server, "run_command", json!({ "command": "model.shell", "params": { "thickness": 1, "remove": [top["found"][0]["reference"]] } }));
        assert!(ok(&mut server, "broken_references", json!({}))["broken"].as_array().unwrap().is_empty());
        // What it did, as a script.
        let log = ok(&mut server, "session_log", json!({}));
        let ran: Vec<String> = log["steps"].as_array().unwrap().iter().map(|s| s["run"].as_str().unwrap().to_owned()).collect();
        (volume, ran)
    });

    // The window goes on taking frames meanwhile, as it does when someone is sitting at it.
    let ctx = egui::Context::default();
    let mut time = 0.0;
    let mut frame = |wb: &mut Workbench| {
        time += 1.0 / 60.0;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        let out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(time), ..Default::default() }, |ui| wb.ui(ui, None));
        out.drop_without_applying_deltas();
    };
    let started = std::time::Instant::now();
    while !agent.is_finished() {
        frame(&mut wb);
        std::thread::sleep(std::time::Duration::from_millis(1));
        assert!(started.elapsed().as_secs() < 120, "the agent never finished");
    }
    let (volume, ran) = agent.join().unwrap();

    // What the agent did is in the app's own document, said in its status bar, and in its
    // history: the person there takes it back a step at a time.
    let names = |wb: &Workbench| wb.document().features().iter().map(|f| f.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&wb), ["Sketch1", "Extrusion1", "Sketch2", "Extrusion2", "Fillet1", "Shell1"]);
    let net = (60.0 * 40.0 - std::f64::consts::PI * 25.0 - (4.0 - std::f64::consts::PI) * 9.0) * 8.0;
    assert!((volume - net).abs() < 1e-6 * net, "{volume} vs {net}");
    assert_eq!(ran, ["batch.run", "model.shell"]);
    assert_eq!(wb.status(), "Remote: model.shell");
    for _ in 0..600 {
        frame(&mut wb);
        if wb.is_settled() {
            break;
        }
    }
    assert_eq!(wb.scene().bodies.len(), 1, "the window shows the part");
    wb.exec("edit.undo", json!({})).unwrap();
    assert_eq!(names(&wb).len(), 5, "the shell, taken back");
    wb.exec("edit.undo", json!({})).unwrap();
    assert!(names(&wb).is_empty(), "the four steps run as one, taken back as one");
}

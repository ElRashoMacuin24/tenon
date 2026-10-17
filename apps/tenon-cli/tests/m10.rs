//! M10, the agent-native workflow: every command's parameters as a schema that is published,
//! kept true and held to; and the commands that no other test runs, run here so that their
//! schemas are held to what they do as well.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tenon_cli::Engine;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;

fn kernel() -> Box<dyn Kernel> {
    Box::new(OcctKernel::new())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-cli-m10-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn copy(from: &str, files: &[&str], to: &Path) {
    for f in files {
        std::fs::copy(examples().join(from).join(f), to.join(f)).unwrap();
    }
}

#[test]
fn every_command_has_a_schema_and_the_published_document_is_the_registrys() {
    let doc = tenon_cli::commands_schema();
    assert_eq!(doc["version"], tenon_model::schema::VERSION);
    let commands = doc["commands"].as_array().unwrap();
    let all = tenon_cli::engine::all_commands();
    assert_eq!(commands.len(), all.len());
    for ((id, label, _, undoable), c) in all.iter().zip(commands) {
        assert_eq!((c["id"].as_str(), c["label"].as_str(), c["undoable"].as_bool()), (Some(*id), Some(*label), Some(*undoable)));
        let params = &c["params"];
        assert_eq!(params["type"], "object", "{id} has a schema");
        assert_eq!(params["additionalProperties"], false, "{id}");
        // Every parameter says what it is, and what must be given is among them.
        let properties = params["properties"].as_object().unwrap();
        for (name, p) in properties {
            assert!(p["description"].as_str().is_some_and(|d| d.len() > 3), "{id}.{name} is explained");
            assert!(p.get("type").is_some() || p.get("anyOf").is_some() || p.get("$ref").is_some(), "{id}.{name} has a type");
        }
        for name in params["required"].as_array().into_iter().flatten() {
            assert!(properties.contains_key(name.as_str().unwrap()), "{id}: {name}");
        }
        // What the schema points to is in the document.
        let text = params.to_string();
        for (def, _) in doc["$defs"].as_object().unwrap() {
            let _ = text.contains(&format!("#/$defs/{def}"));
        }
        assert!(!text.contains("#/$defs/") || text.contains("#/$defs/face_ref") || text.contains("#/$defs/edge_ref"), "{id}");
    }

    // The file in the repository is this, to the letter: a change to any command's parameters
    // shows in review as a change to it.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/schemas/commands.json");
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert!(
        on_disk == tenon_cli::commands_schema_text(),
        "docs/schemas/commands.json is out of date; write it again with `pixi run cargo run -p tenon-cli -- commands --schema --out docs/schemas/commands.json`"
    );
    // It breaks nothing that was written against itself, and a change that would is refused.
    let dir = scratch("schema");
    let file = dir.join("commands.json");
    let wrote = tenon_cli::write_commands_schema(&file).unwrap();
    assert!(wrote.text.contains("198 commands") || wrote.text.contains(&format!("{} commands", all.len())), "{}", wrote.text);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), tenon_cli::commands_schema_text());
    tenon_cli::write_commands_schema(&file).unwrap();
    // As if a caller had been written against a command and a parameter that are no longer there.
    let mut older = doc.clone();
    older["commands"].as_array_mut().unwrap().push(json!({ "id": "sketch.hyperbola", "params": { "type": "object", "properties": {} } }));
    older["commands"][0]["params"]["properties"]["title"] = json!({ "type": "string" });
    std::fs::write(&file, older.to_string()).unwrap();
    let refused = tenon_cli::write_commands_schema(&file).unwrap_err();
    assert!(
        refused.contains("sketch.hyperbola is gone") && refused.contains("document.rename: `title` is gone") && refused.contains("VERSION"),
        "{refused}"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), older.to_string(), "nothing was written");
    // Of another version, it is simply replaced.
    older["version"] = json!(0);
    std::fs::write(&file, older.to_string()).unwrap();
    tenon_cli::write_commands_schema(&file).unwrap();
}

#[test]
fn what_comes_from_outside_is_held_to_the_schema_before_anything_runs() {
    let dir = scratch("held");
    let mut e = Engine::new(kernel(), &dir);
    let sk = e.exec("sketch.create", &json!({ "plane": "xy" })).unwrap()["feature"].as_u64().unwrap();
    e.exec("sketch.rectangle", &json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    let before = e.exec("model.tree", &json!({})).unwrap();

    // A slip in a name is told with the name that was meant; before, an optional parameter
    // spelt wrongly was passed over in silence and the feature came out at its default.
    let err = |e: &mut Engine, id: &str, p: Value| e.exec(id, &p).unwrap_err();
    assert_eq!(
        err(&mut e, "model.extrude", json!({ "sketch": sk, "distance": 10, "opperation": "cut" })),
        "model.extrude has no parameter `opperation`: did you mean `operation`?"
    );
    assert_eq!(
        err(&mut e, "model.extrude", json!({ "sketch": sk, "distance": 10, "revers": true })),
        "model.extrude has no parameter `revers`: did you mean `reverse`?"
    );
    let told = err(&mut e, "model.extrude", json!({ "sketch": sk, "distance": 10, "colour": "red" }));
    assert!(told.starts_with("model.extrude has no parameter `colour`: it takes sketch, distance, backward"), "{told}");
    // What is missing, and what is of the wrong kind, in the parameter's own words.
    assert_eq!(
        err(&mut e, "model.extrude", json!({ "distance": 10 })),
        "model.extrude needs `sketch`: the sketch: its feature id, as sketch.create returns it"
    );
    assert_eq!(
        err(&mut e, "model.extrude", json!({ "sketch": sk, "distance": "10 mm" })),
        "model.extrude: `distance` must be a number, not text (how far)"
    );
    assert_eq!(
        err(&mut e, "model.extrude", json!({ "sketch": sk, "distance": 10, "operation": "subtract" })),
        "model.extrude: `operation` must be one of join | cut | new_body | intersect, not \"subtract\" (what the feature does to the part (default join))"
    );
    assert_eq!(err(&mut e, "sketch.line", json!({ "sketch": sk, "x1": 0, "y1": 0 })), "sketch.line needs `end`, or `x2` and `y2`");
    assert_eq!(err(&mut e, "edit.undo", json!({ "steps": 2 })), "edit.undo takes no parameters, so not `steps`");
    assert_eq!(err(&mut e, "render.png", json!({ "veiw": "top" })), "render.png has no parameter `veiw`: did you mean `view`?");
    assert_eq!(err(&mut e, "asm.insert", json!({ "file": "a.tenon" })), "asm.insert has no parameter `file`: it takes path, at, grounded, row");
    assert_eq!(err(&mut e, "drw.note", json!({ "text": 7 })), "drw.note: `text` must be text, not a number (what it says)");
    // A command there is not is still said to be unknown, by the registry.
    assert!(err(&mut e, "model.extrood", json!({})).contains("unknown command `model.extrood`"));
    // Nothing ran: the part is as it was.
    assert_eq!(e.exec("model.tree", &json!({})).unwrap(), before);

    // A null where a parameter may be left out is the parameter left out.
    let ext = e.exec("model.extrude", &json!({ "sketch": sk, "distance": 10, "backward": null, "taper": null, "regions": null })).unwrap();
    assert!(ext["feature"].as_u64().is_some());
    // And case is not held against the names of planes and axes.
    e.exec("sketch.create", &json!({ "plane": "XZ" })).unwrap();
}

#[test]
fn the_sketch_commands_no_other_test_runs() {
    let dir = scratch("sketch");
    let mut e = Engine::new(kernel(), &dir);
    let run = |e: &mut Engine, id: &str, p: Value| e.exec(id, &p).unwrap_or_else(|err| panic!("{id} {p}: {err}"));
    let sk = run(&mut e, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    let entity = |e: &mut Engine, id: u64| {
        let info = run(e, "sketch.info", json!({ "sketch": sk }));
        info["entities"].as_array().unwrap().iter().find(|x| x["id"] == id).cloned().unwrap_or_else(|| panic!("no entity {id}"))
    };
    let at = |v: &Value| (v["x"].as_f64().unwrap(), v["y"].as_f64().unwrap());

    // An arc through three points: it starts and ends where it was told, and its centre is
    // where the circle through the three has it.
    let arc = run(&mut e, "sketch.arc3", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 5, "y2": 5, "x3": 10, "y3": 0 }));
    let (c, s, f) = (arc["center"].as_u64().unwrap(), arc["start"].as_u64().unwrap(), arc["end"].as_u64().unwrap());
    let pos = |e: &mut Engine, id: u64| at(&entity(e, id)["pos"]);
    // (An arc is kept counter-clockwise from its start, so the two ends may have changed places.)
    let mut ends = [pos(&mut e, s), pos(&mut e, f)];
    ends.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(ends[0].0.abs() < 1e-9 && ends[0].1.abs() < 1e-9 && (ends[1].0 - 10.0).abs() < 1e-9 && ends[1].1.abs() < 1e-9, "{ends:?}");
    let centre = pos(&mut e, c);
    assert!((centre.0 - 5.0).abs() < 1e-9 && centre.1.abs() < 1e-9, "{centre:?}");

    // A spline by its control points, of the degree asked for.
    let spline = run(&mut e, "sketch.spline", json!({ "sketch": sk, "points": [[0, 20], [5, 25], [10, 20], [15, 25]], "degree": 3 }));
    let s = entity(&mut e, spline["spline"].as_u64().unwrap());
    assert_eq!(s["type"], "spline", "{s}");
    assert!(e.exec("sketch.spline", &json!({ "sketch": sk, "points": [[0, 0]] })).is_err(), "one point is no spline");

    // A fillet at the corner of two lines: an arc of the radius, tangent to both.
    let l1 = run(&mut e, "sketch.line", json!({ "sketch": sk, "x1": 30, "y1": 0, "x2": 50, "y2": 0 }));
    let l2 = run(&mut e, "sketch.line", json!({ "sketch": sk, "start": l1["end"], "x2": 50, "y2": 20 }));
    let fillet = run(&mut e, "sketch.fillet", json!({ "sketch": sk, "point": l1["end"], "radius": 4 }));
    let a = entity(&mut e, fillet["arc"].as_u64().unwrap());
    assert_eq!(a["type"], "arc", "{a}");
    let centre = pos(&mut e, a["center"].as_u64().unwrap());
    assert!((centre.0 - 46.0).abs() < 1e-6 && (centre.1 - 4.0).abs() < 1e-6, "{centre:?}");

    // An offset of a line: a line beside it, the distance away.
    let off = run(&mut e, "sketch.offset", json!({ "sketch": sk, "curves": [l1["line"]], "distance": 3 }));
    let line = entity(&mut e, off["curves"][0].as_u64().unwrap());
    let y = pos(&mut e, line["start"].as_u64().unwrap()).1;
    assert!((y.abs() - 3.0).abs() < 1e-9, "{line}: {y}");

    // A mirror image across a line: as far the other side of it.
    let axis = run(&mut e, "sketch.line", json!({ "sketch": sk, "x1": 60, "y1": -10, "x2": 60, "y2": 30 }));
    let top = pos(&mut e, l2["end"].as_u64().unwrap());
    let mirrored = run(&mut e, "sketch.mirror", json!({ "sketch": sk, "entities": [l2["line"]], "axis": axis["line"] }));
    let copies: Vec<(f64, f64)> = mirrored["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| entity(&mut e, id.as_u64().unwrap()))
        .filter(|x| x["type"] == "point")
        .map(|x| at(&x["pos"]))
        .collect();
    assert!(copies.iter().any(|p| (p.0 - (120.0 - top.0)).abs() < 1e-9 && (p.1 - top.1).abs() < 1e-9), "{copies:?} mirrors {top:?}");
}

#[test]
fn the_assembly_drawing_and_render_commands_no_other_test_runs() {
    let dir = scratch("asm");
    copy("m3-pivot", &["pivot.tenonasm", "base.tenon", "arm.tenon", "pin.tenon", "block.tenon"], &dir);
    let mut e = Engine::new(kernel(), &dir);
    let run = |e: &mut Engine, id: &str, p: Value| e.exec(id, &p).unwrap_or_else(|err| panic!("{id} {p}: {err}"));
    run(&mut e, "asm.open", json!({ "path": "pivot.tenonasm" }));
    let tree = run(&mut e, "asm.tree", json!({}));
    let components = tree["components"].as_array().unwrap().clone();
    let named = |tree: &Value, id: u64| tree["components"].as_array().unwrap().iter().find(|c| c["id"] == id).cloned().unwrap();
    let block = components.iter().find(|c| c["name"].as_str().unwrap().starts_with("block")).unwrap()["id"].as_u64().unwrap();

    // Renamed: the component, a relationship, and (with neither) the assembly.
    run(&mut e, "asm.rename", json!({ "component": block, "name": "Slider" }));
    let rel = tree["relationships"][0]["id"].as_u64().unwrap();
    run(&mut e, "asm.rename", json!({ "relationship": rel, "name": "Hinge" }));
    run(&mut e, "asm.rename", json!({ "name": "Pivot, renamed" }));
    let tree = run(&mut e, "asm.tree", json!({}));
    assert_eq!(named(&tree, block)["name"], "Slider");
    assert_eq!(tree["relationships"][0]["name"], "Hinge");
    assert_eq!(tree["name"], "Pivot, renamed");

    // Grounded, it has nothing left to move; freed again, it has what it had.
    let free = named(&tree, block)["dof"].clone();
    run(&mut e, "asm.ground", json!({ "component": block }));
    let held = named(&run(&mut e, "asm.tree", json!({})), block);
    assert_eq!((held["grounded"].as_bool(), held["dof"].as_u64()), (Some(true), Some(0)), "{held}");
    run(&mut e, "asm.ground", json!({ "component": block, "grounded": false }));
    assert_eq!(named(&run(&mut e, "asm.tree", json!({})), block)["dof"], free);

    // Hidden, it is out of the mass; shown again, it is back in.
    let all = run(&mut e, "asm.mass", json!({}))["volume"].as_f64().unwrap();
    run(&mut e, "asm.visible", json!({ "component": block, "visible": false }));
    let without = run(&mut e, "asm.mass", json!({}))["volume"].as_f64().unwrap();
    assert!(without < all - 1.0, "{without} of {all}");
    run(&mut e, "asm.visible", json!({ "component": block }));
    assert!((run(&mut e, "asm.mass", json!({}))["volume"].as_f64().unwrap() - all).abs() < 1e-6);

    // A tweak moves its components that far that way in the exploded view, and nowhere else.
    let place = |e: &mut Engine, id: u64| -> Vec<f64> {
        let p = run(e, "asm.explode.positions", json!({}));
        let c = p["components"].as_array().unwrap().iter().find(|c| c["id"] == id).cloned().unwrap();
        c["origin"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect()
    };
    // (The example comes with an exploded view of its own: start from none.)
    run(&mut e, "asm.explode.clear", json!({}));
    let was = place(&mut e, block);
    run(&mut e, "asm.explode.add", json!({ "components": [block], "direction": "z", "distance": 25 }));
    run(&mut e, "asm.explode.add", json!({ "components": [block], "direction": [1, 0, 0], "distance": 10 }));
    let now = place(&mut e, block);
    assert!((now[0] - was[0] - 10.0).abs() < 1e-9 && (now[1] - was[1]).abs() < 1e-9 && (now[2] - was[2] - 25.0).abs() < 1e-9, "{was:?} to {now:?}");
    // One step taken out, then all of them.
    run(&mut e, "asm.explode.clear", json!({ "step": 1 }));
    let now = place(&mut e, block);
    assert!((now[0] - was[0]).abs() < 1e-9 && (now[2] - was[2] - 25.0).abs() < 1e-9, "{was:?} to {now:?}");
    run(&mut e, "asm.explode.clear", json!({}));
    assert_eq!(place(&mut e, block), was);

    // A picture of it, exploded or not, to a file or as data.
    let png = run(&mut e, "render.png", json!({ "path": "pivot.png", "view": "top", "width": 320, "height": 240 }));
    assert_eq!((png["width"].as_u64(), png["height"].as_u64()), (Some(320), Some(240)));
    let bytes = std::fs::read(dir.join("pivot.png")).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let data = run(&mut e, "render.png", json!({ "exploded": true, "width": 160, "height": 120 }));
    assert!(data["png_base64"].as_str().is_some_and(|s| s.len() > 200), "{data}");
    assert!(e.exec("render.png", &json!({ "width": 8 })).unwrap_err().contains("16 to 4096"));

    // A drawing: a point of the sheet is somewhere in a view, and that place in the view is
    // that point of the sheet.
    let dir = scratch("drw");
    copy("m4-plate", &["plate.tenondrw", "plate.tenon"], &dir);
    let mut e = Engine::new(kernel(), &dir);
    run(&mut e, "drw.open", json!({ "path": "plate.tenondrw" }));
    let info = run(&mut e, "drw.info", json!({}));
    let view = info["views"][0]["id"].as_u64().unwrap();
    let in_view = run(&mut e, "drw.to_view", json!({ "view": view, "at": [120, 150] }));
    let back = run(&mut e, "drw.to_sheet", json!({ "view": view, "at": [in_view["x"], in_view["y"]] }));
    let sheet: Vec<f64> = back["at"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((sheet[0] - 120.0).abs() < 1e-9 && (sheet[1] - 150.0).abs() < 1e-9, "{in_view} then {back}");
    let sheet_png = run(&mut e, "render.png", json!({ "sheet": info["sheets"][0]["id"], "width": 400 }));
    assert_eq!(sheet_png["width"].as_u64(), Some(400));
}

// ---- the MCP server ------------------------------------------------------------------------------

use tenon_cli::mcp::Server;

fn rpc(server: &mut Server, method: &str, params: Value) -> Value {
    let r = server.handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })).unwrap();
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

/// Calls a tool; its data, or (when the tool says it failed) the reason.
fn tool(server: &mut Server, name: &str, args: Value) -> Result<Value, String> {
    let r = rpc(server, "tools/call", json!({ "name": name, "arguments": args }));
    if r["isError"] == true {
        return Err(r["content"][0]["text"].as_str().unwrap_or_default().to_owned());
    }
    Ok(r.get("structuredContent").cloned().unwrap_or_else(|| r["content"][0].clone()))
}

fn ok(server: &mut Server, name: &str, args: Value) -> Value {
    tool(server, name, args.clone()).unwrap_or_else(|e| panic!("{name} {args}: {e}"))
}

fn tree_names(server: &mut Server) -> Vec<String> {
    ok(server, "model_tree", json!({}))["features"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap().to_owned()).collect()
}

fn volume_of(server: &mut Server) -> f64 {
    ok(server, "measure", json!({}))["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

#[test]
fn the_server_says_what_it_offers_and_what_every_command_takes() {
    let mut server = Server::new(Engine::new(kernel(), scratch("mcp-offers")));
    let hello = rpc(&mut server, "initialize", json!({ "protocolVersion": "2025-06-18" }));
    assert!(hello["capabilities"]["tools"].is_object() && hello["capabilities"]["resources"].is_object(), "{hello}");
    for word in ["describe_command", "run_commands", "find_geometry", "broken_references", "session_log", "model.box"] {
        assert!(hello["instructions"].as_str().unwrap().contains(word), "the instructions name {word}");
    }
    // The tools: to build, to inspect, to repair a reference, to render.
    let tools = rpc(&mut server, "tools/list", json!({}))["tools"].as_array().unwrap().clone();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "list_commands",
            "describe_command",
            "run_command",
            "run_commands",
            "model_tree",
            "query_topology",
            "find_geometry",
            "measure",
            "broken_references",
            "repair_reference",
            "render_png",
            "export",
            "session_log"
        ]
    );
    for t in &tools {
        assert!(t["description"].as_str().is_some_and(|d| d.len() > 40), "{t}");
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
    }
    // find_geometry takes what model.find takes, as that command's schema has it.
    let find = tools.iter().find(|t| t["name"] == "find_geometry").unwrap();
    assert_eq!(find["inputSchema"]["required"], json!(["what"]));
    assert_eq!(find["inputSchema"]["properties"]["what"]["enum"], json!(["faces", "edges"]));
    assert!(find["inputSchema"]["properties"]["most"]["anyOf"].is_array());

    // Every command by name, narrowed by a prefix; and one command exactly.
    let all = ok(&mut server, "list_commands", json!({}));
    assert_eq!(all["schema_version"], tenon_model::schema::VERSION);
    assert_eq!(all["commands"].as_array().unwrap().len(), tenon_cli::engine::all_commands().len());
    let sheet = ok(&mut server, "list_commands", json!({ "prefix": "sheet." }));
    assert!(sheet["commands"].as_array().unwrap().iter().all(|c| c["id"].as_str().unwrap().starts_with("sheet.")));
    assert!(sheet["commands"].as_array().unwrap().len() >= 18);
    let extrude = ok(&mut server, "describe_command", json!({ "command": "model.extrude" }));
    assert_eq!((extrude["label"].as_str(), extrude["undoable"].as_bool()), (Some("Extrude"), Some(true)));
    assert_eq!(extrude["params"]["required"], json!(["sketch"]));
    assert_eq!(extrude["params"]["properties"]["distance"]["x-unit"], "mm");
    assert_eq!(extrude["params"]["properties"]["operation"]["enum"], json!(["join", "cut", "new_body", "intersect"]));
    let fillet = ok(&mut server, "describe_command", json!({ "command": "model.fillet" }));
    assert_eq!(fillet["params"]["properties"]["edges"]["items"], json!({ "$ref": "#/$defs/edge_ref" }));
    assert!(fillet["$defs"]["edge_ref"]["description"].as_str().unwrap().contains("model.edge_ref"));
    assert!(ok(&mut server, "describe_command", json!({ "command": "batch.run" }))["params"]["properties"]["steps"].is_object());
    // A command that is not there is told with those near it.
    let told = tool(&mut server, "describe_command", json!({ "command": "extrude" })).unwrap_err();
    assert!(told.contains("there is no command `extrude`") && told.contains("model.extrude"), "{told}");
    assert!(tool(&mut server, "describe_command", json!({ "command": "nonsense.at.all" })).unwrap_err().contains("see list_commands"));

    // The schemas as a resource: the document of docs/schemas.
    let listed = rpc(&mut server, "resources/list", json!({}));
    assert_eq!(listed["resources"][0]["uri"], tenon_cli::mcp::SCHEMAS_URI);
    let read = rpc(&mut server, "resources/read", json!({ "uri": tenon_cli::mcp::SCHEMAS_URI }));
    let doc: Value = serde_json::from_str(read["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(doc, tenon_cli::commands_schema());
    let missing = server.handle(&json!({ "jsonrpc": "2.0", "id": 5, "method": "resources/read", "params": { "uri": "tenon://nothing" } })).unwrap();
    assert_eq!(missing["error"]["code"], -32002);
}

#[test]
fn an_agent_builds_in_words_as_one_step_and_reads_back_what_it_did() {
    let dir = scratch("mcp-build");
    let mut server = Server::new(Engine::new(kernel(), &dir));

    // "A plate 60 by 40 by 8 with a 10 mm hole in the middle and its four corners rounded 3 mm":
    // four commands, the edges found by saying what they are, all of it one step.
    let plate = json!([
        { "run": "model.box", "with": { "width": 60, "depth": 40, "height": 8 }, "as": "plate" },
        { "run": "model.cylinder", "with": { "x": 30, "y": 20, "diameter": 10, "height": 8, "operation": "cut" } },
        { "run": "model.find", "with": { "what": "edges", "kind": "line", "along": "z" }, "as": "corners" },
        { "run": "model.fillet", "with": { "radius": 3, "edges": ["$corners.found.0.reference", "$corners.found.1.reference", "$corners.found.2.reference", "$corners.found.3.reference"] } },
    ]);
    let done = ok(&mut server, "run_commands", json!({ "steps": plate }));
    assert_eq!(done["steps"].as_array().unwrap().len(), 4);
    assert_eq!(done["steps"][0]["result"]["name"], "Extrusion1");
    assert_eq!(tree_names(&mut server), ["Sketch1", "Extrusion1", "Sketch2", "Extrusion2", "Fillet1"]);
    let net = (60.0 * 40.0 - std::f64::consts::PI * 25.0 - (4.0 - std::f64::consts::PI) * 9.0) * 8.0;
    assert!((volume_of(&mut server) - net).abs() < 1e-6 * net, "{} vs {net}", volume_of(&mut server));
    // One undo takes all of it back; one redo brings all of it again.
    ok(&mut server, "run_command", json!({ "command": "edit.undo" }));
    assert!(tree_names(&mut server).is_empty());
    assert!(tool(&mut server, "run_command", json!({ "command": "edit.undo" })).is_err(), "there was one step");
    ok(&mut server, "run_command", json!({ "command": "edit.redo" }));
    assert_eq!(tree_names(&mut server).len(), 5);

    // A step that fails takes the steps before it back: nothing is left half made.
    let before = volume_of(&mut server);
    let told = tool(
        &mut server,
        "run_commands",
        json!({ "steps": [
            { "run": "model.box", "with": { "x": 100, "width": 10, "depth": 10, "height": 10, "operation": "new_body" } },
            { "run": "sketch.create", "with": { "plane": "xz" }, "as": "sk" },
            { "run": "sketch.line", "with": { "sketch": 99, "x1": 0, "y1": 0, "x2": 5, "y2": 5 } },
        ] }),
    )
    .unwrap_err();
    assert!(
        told.contains("step 3 (sketch.line)") && told.contains("Nothing was changed") && told.contains("2 step(s) before it were taken back"),
        "{told}"
    );
    assert_eq!(tree_names(&mut server).len(), 5);
    assert_eq!(volume_of(&mut server), before);
    // A mistake in a parameter is told by name, with the step it is in.
    let told = tool(&mut server, "run_commands", json!({ "steps": [{ "run": "model.box", "with": { "width": 10, "depth": 10, "hieght": 10 } }] }))
        .unwrap_err();
    assert!(told.contains("step 1 (model.box)") && told.contains("did you mean `height`?"), "{told}");
    // A reference to a step that was not saved, likewise.
    let told =
        tool(&mut server, "run_commands", json!({ "steps": [{ "run": "sketch.point", "with": { "sketch": "$nope.feature", "x": 0, "y": 0 } }] }))
            .unwrap_err();
    assert!(told.contains("no earlier step is saved as `nope`"), "{told}");
    // What cannot be taken back cannot be one of the steps.
    let told = tool(
        &mut server,
        "run_commands",
        json!({ "steps": [{ "run": "model.box", "with": { "width": 1, "depth": 1, "height": 1 } }, { "run": "file.new" }] }),
    )
    .unwrap_err();
    assert!(told.contains("file.new cannot be one of several steps run as one"), "{told}");
    assert_eq!(tree_names(&mut server).len(), 5);
    // Not as one: the steps before the failure stand.
    let told = tool(
        &mut server,
        "run_commands",
        json!({ "as_one": false, "steps": [{ "run": "feature.rename", "with": { "feature": 5, "name": "Corners" } }, { "run": "feature.rename", "with": { "feature": 77, "name": "x" } }] }),
    )
    .unwrap_err();
    assert!(told.contains("step 2 (feature.rename)") && told.contains("The 1 step(s) before it stand"), "{told}");
    assert_eq!(tree_names(&mut server)[4], "Corners");

    // One command at a time, held to its schema like any other.
    let told = tool(&mut server, "run_command", json!({ "command": "model.shell", "params": { "thicknes": 1 } })).unwrap_err();
    assert_eq!(told, "model.shell failed: model.shell has no parameter `thicknes`: did you mean `thickness`?");
    // "Open the top": the face found by where it looks.
    let top = ok(&mut server, "find_geometry", json!({ "what": "faces", "facing": "top" }));
    assert_eq!(top["count"], 1);
    ok(&mut server, "run_command", json!({ "command": "model.shell", "params": { "thickness": 1, "remove": [top["found"][0]["reference"]] } }));
    assert!(ok(&mut server, "model_tree", json!({}))["error"].is_null());
    assert!(tool(&mut server, "find_geometry", json!({ "what": "faces", "facing": "upwards" })).unwrap_err().contains("`facing`"));

    // What was done, as a script: only what changed something, in order, with what was given.
    let log = ok(&mut server, "session_log", json!({}));
    let ran: Vec<&str> = log["steps"].as_array().unwrap().iter().map(|s| s["run"].as_str().unwrap()).collect();
    // (Of the steps not run as one, the one that was done; of those taken back, nothing.)
    assert_eq!(ran, ["batch.run", "edit.undo", "edit.redo", "feature.rename", "model.shell"]);
    assert_eq!(log["steps"][0]["with"]["steps"][0], plate[0]);
    assert_eq!(log["steps"][0]["with"]["steps"][3]["with"]["edges"][0], "$$corners.found.0.reference", "a reference inside a batch is kept as one");
    assert_eq!(log["steps"][3]["with"], json!({ "feature": 5, "name": "Corners" }));
    let everything = ok(&mut server, "session_log", json!({ "all": true }));
    assert!(everything["steps"].as_array().unwrap().iter().any(|s| s["run"] == "model.find"));
    // Written out and run again from nothing, it makes the same part.
    let path = dir.join("session.json");
    let written = ok(&mut server, "session_log", json!({ "path": path.to_string_lossy() }));
    assert_eq!(written["steps"], 5);
    let script = tenon_cli::Script::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut again = Engine::new(kernel(), &dir);
    let replayed = tenon_cli::script::run(&mut again, &script).unwrap_or_else(|(_, e)| panic!("{e}"));
    assert_eq!(replayed.len(), 5);
    let names: Vec<String> = again.exec("model.tree", &json!({})).unwrap()["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, tree_names(&mut server));
    let made = again.exec("model.mass", &json!({})).unwrap()["bodies"][0]["volume"].as_f64().unwrap();
    let here = volume_of(&mut server);
    assert!((made - here).abs() < 1e-9 * here, "{made} vs {here}");

    // Files: the part saved and exported in each format.
    for (format, file) in [("tenon", "plate.tenon"), ("step", "plate.step"), ("stl", "plate.stl"), ("3mf", "plate.3mf")] {
        ok(&mut server, "export", json!({ "format": format, "path": file }));
        assert!(std::fs::metadata(dir.join(file)).unwrap().len() > 100, "{file}");
    }
    assert!(tool(&mut server, "export", json!({ "format": "iges", "path": "x.igs" })).is_err());
}

#[test]
fn an_agent_sees_a_broken_reference_and_repairs_it() {
    let mut server = Server::new(Engine::new(kernel(), scratch("mcp-repair")));
    // A block with one edge rounded.
    let sk =
        ok(&mut server, "run_command", json!({ "command": "sketch.create", "params": { "plane": "xy" } }))["result"]["feature"].as_u64().unwrap();
    let rect = ok(
        &mut server,
        "run_command",
        json!({ "command": "sketch.rectangle", "params": { "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 30 } }),
    )["result"]
        .clone();
    ok(&mut server, "run_command", json!({ "command": "model.extrude", "params": { "sketch": sk, "distance": 10 } }));
    let edge = ok(&mut server, "find_geometry", json!({ "what": "edges", "along": "y", "most": "right" }));
    let top_right = edge["found"].as_array().unwrap().iter().find(|e| e["mid"][2].as_f64() == Some(10.0)).unwrap().clone();
    let fillet =
        ok(&mut server, "run_command", json!({ "command": "model.fillet", "params": { "edges": [top_right["reference"]], "radius": 2 } }))["result"]
            ["feature"]
            .as_u64()
            .unwrap();
    assert!(ok(&mut server, "broken_references", json!({}))["broken"].as_array().unwrap().is_empty());
    assert!(tool(&mut server, "repair_reference", json!({ "feature": fillet, "path": "/edges/0" })).unwrap_err().contains("nothing is broken"));
    let whole = volume_of(&mut server);

    // The right-hand side of the sketch is deleted and drawn again: the face it swept is a new
    // face, and the fillet no longer finds its edge.
    let right = rect["lines"][1].as_u64().unwrap();
    let info = ok(&mut server, "run_command", json!({ "command": "sketch.info", "params": { "sketch": sk } }))["result"].clone();
    let line = info["entities"].as_array().unwrap().iter().find(|e| e["id"] == right).unwrap().clone();
    ok(&mut server, "run_command", json!({ "command": "sketch.delete", "params": { "sketch": sk, "entities": [right] } }));
    ok(&mut server, "run_command", json!({ "command": "sketch.line", "params": { "sketch": sk, "start": line["start"], "end": line["end"] } }));
    let tree = ok(&mut server, "model_tree", json!({}));
    assert_eq!(tree["error"]["feature"].as_u64(), Some(fillet), "{tree}");

    // What is broken, and what it could be: the nearest is where the old edge was, and is said
    // to be the one meant; each candidate says what it is and which edge of the body.
    let broken = ok(&mut server, "broken_references", json!({}));
    assert_eq!((broken["feature"].as_u64(), broken["name"].as_str()), (Some(fillet), Some("Fillet1")));
    let entry = &broken["broken"][0];
    assert_eq!((entry["path"].as_str(), entry["kind"].as_str(), entry["confident"].as_bool()), (Some("/edges/0"), Some("edge"), Some(true)));
    let nearest = &entry["candidates"][0];
    assert!(nearest["distance"].as_f64().unwrap() < 1e-6 && nearest["length"].as_f64() == Some(30.0), "{nearest}");
    assert_eq!(nearest["mid"], json!([40.0, 15.0, 10.0]));
    assert!(nearest["edge"].as_u64().is_some() && nearest["body"] == 0 && nearest["curve"].is_object(), "{nearest}");
    assert!(entry["candidates"].as_array().unwrap().len() > 1);
    // The part as far as it was rebuilt, with the candidate picked out, is a picture to look at.
    let picture = ok(&mut server, "render_png", json!({ "width": 240, "height": 180, "highlight_edges": [nearest["edge"]] }));
    assert_eq!(picture["mimeType"], "image/png");
    let plain = ok(&mut server, "render_png", json!({ "width": 240, "height": 180 }));
    assert_ne!(picture["data"], plain["data"], "the edge is picked out");
    assert!(tool(&mut server, "render_png", json!({ "highlight_edges": [999] })).unwrap_err().contains("has no edge 999"));

    // Repaired with the one meant, without having to say which: the part is whole again.
    let wrong = tool(&mut server, "repair_reference", json!({ "feature": fillet, "path": "/edges/3" })).unwrap_err();
    assert!(wrong.contains("`/edges/3` is not a broken reference") && wrong.contains("/edges/0"), "{wrong}");
    assert!(tool(&mut server, "repair_reference", json!({ "feature": sk, "path": "/edges/0" })).unwrap_err().contains("is not the one that fails"));
    assert!(
        tool(&mut server, "repair_reference", json!({ "feature": fillet, "path": "/edges/0", "candidate": 9 }))
            .unwrap_err()
            .contains("there is no candidate 9")
    );
    let mended = ok(&mut server, "repair_reference", json!({ "feature": fillet, "path": "/edges/0" }));
    assert!(mended["error"].is_null(), "{mended}");
    assert!((volume_of(&mut server) - whole).abs() < 1e-6 * whole);
    // It was an ordinary edit: undone, the reference is broken again; then mended by choosing.
    ok(&mut server, "run_command", json!({ "command": "edit.undo" }));
    assert_eq!(ok(&mut server, "broken_references", json!({}))["broken"].as_array().unwrap().len(), 1);
    ok(&mut server, "repair_reference", json!({ "feature": fillet, "path": "/edges/0", "candidate": 0 }));
    assert!((volume_of(&mut server) - whole).abs() < 1e-6 * whole);
    // Or with a reference of one's own choosing: another edge, and the fillet moves there.
    ok(&mut server, "run_command", json!({ "command": "edit.undo" }));
    let left = ok(&mut server, "find_geometry", json!({ "what": "edges", "along": "y", "most": "left" }));
    let top_left = left["found"].as_array().unwrap().iter().find(|e| e["mid"][2].as_f64() == Some(10.0)).unwrap().clone();
    ok(&mut server, "repair_reference", json!({ "feature": fillet, "path": "/edges/0", "with": top_left["reference"] }));
    assert!((volume_of(&mut server) - whole).abs() < 1e-6 * whole);
    let round = ok(&mut server, "find_geometry", json!({ "what": "faces", "kind": "cylinder" }));
    assert!(round["found"][0]["centroid"][0].as_f64().unwrap() < 2.0, "the fillet is on the left now: {round}");
    // The log has the repair as the command it was.
    let log = ok(&mut server, "session_log", json!({}));
    assert_eq!(log["steps"].as_array().unwrap().last().unwrap()["run"], "model.repair");
}

#[test]
fn an_agent_is_made_to_choose_when_no_replacement_is_clearly_the_one_meant() {
    let mut server = Server::new(Engine::new(kernel(), scratch("mcp-repair-choice")));
    let run = |server: &mut Server, command: &str, params: Value| {
        ok(server, "run_command", json!({ "command": command, "params": params }))["result"].clone()
    };
    // A block with its top right edge rounded.
    let sk = run(&mut server, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    let rect = run(&mut server, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 30 }));
    run(&mut server, "model.extrude", json!({ "sketch": sk, "distance": 10 }));
    let edge = ok(&mut server, "find_geometry", json!({ "what": "edges", "along": "y", "most": "right" }));
    let top_right = edge["found"].as_array().unwrap().iter().find(|e| e["mid"][2].as_f64() == Some(10.0)).unwrap().clone();
    let fillet = run(&mut server, "model.fillet", json!({ "edges": [top_right["reference"]], "radius": 2 }))["feature"].as_u64().unwrap();
    let whole = volume_of(&mut server);

    // The right-hand side is drawn again as two lines end to end: where there was one edge
    // there are two, each half of it, and neither is the old one more than the other.
    let right = rect["lines"][1].as_u64().unwrap();
    let info = run(&mut server, "sketch.info", json!({ "sketch": sk }));
    let line = info["entities"].as_array().unwrap().iter().find(|e| e["id"] == right).unwrap().clone();
    run(&mut server, "sketch.delete", json!({ "sketch": sk, "entities": [right] }));
    let lower = run(&mut server, "sketch.line", json!({ "sketch": sk, "start": line["start"], "x2": 40, "y2": 15 }));
    run(&mut server, "sketch.line", json!({ "sketch": sk, "start": lower["end"], "end": line["end"] }));
    assert_eq!(ok(&mut server, "model_tree", json!({}))["error"]["feature"].as_u64(), Some(fillet));

    let broken = ok(&mut server, "broken_references", json!({}));
    let entry = &broken["broken"][0];
    assert_eq!(entry["confident"], false, "{entry}");
    let halves: Vec<&Value> =
        entry["candidates"].as_array().unwrap().iter().filter(|c| c["length"].as_f64() == Some(15.0) && c["mid"][2] == 10.0).collect();
    assert_eq!(halves.len(), 2, "both halves are offered: {entry}");
    assert_eq!(halves[0]["distance"], halves[1]["distance"], "and are as near as each other");
    assert!(std::ptr::eq(halves[0], &entry["candidates"][0]) && std::ptr::eq(halves[1], &entry["candidates"][1]), "first: {entry}");

    // No guess is made: the agent is told to look and say which, and nothing is changed.
    let told = tool(&mut server, "repair_reference", json!({ "feature": fillet, "path": "/edges/0" })).unwrap_err();
    assert!(told.contains("which one?") && told.contains("`candidate`"), "{told}");
    assert_eq!(ok(&mut server, "model_tree", json!({}))["error"]["feature"].as_u64(), Some(fillet), "still broken");
    let log = ok(&mut server, "session_log", json!({}));
    assert_ne!(log["steps"].as_array().unwrap().last().unwrap()["run"], "model.repair", "nothing was repaired");
    // Having looked, it says which, and the part is whole.
    let picture = ok(&mut server, "render_png", json!({ "width": 240, "height": 180, "highlight_edges": [halves[0]["edge"], halves[1]["edge"]] }));
    assert_eq!(picture["mimeType"], "image/png");
    let mended = ok(&mut server, "repair_reference", json!({ "feature": fillet, "path": "/edges/0", "candidate": 1 }));
    assert!(mended["error"].is_null(), "{mended}");
    let now = volume_of(&mut server);
    assert!(now < 40.0 * 30.0 * 10.0 && now >= whole - 1e-6 * whole, "an edge of it is rounded again: {now} (all of it: {whole})");
}

//! M1 headless surface: command scripts, the MCP server, generated command docs, and the
//! performance budgets on the demo part.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Instant;

use serde_json::{Value, json};
use tenon_cli::mcp::{Server, serve};
use tenon_cli::{Engine, base64};
use tenon_kernel::{Kernel, MeshTol};
use tenon_kernel_occt::OcctKernel;

fn kernel() -> Box<dyn Kernel> {
    Box::new(OcctKernel::new())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-cli-m1-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_m1_demo_script_builds_a_verified_bracket_and_writes_its_files() {
    // The script itself checks DOF 0, the analytic volume before and after the thickness edit,
    // validity and undo; here we check the files it writes.
    let dir = scratch("demo");
    let r = tenon_cli::demo_m1(kernel(), &dir).unwrap();
    assert_eq!(r.json["ok"], true);

    let mut k = OcctKernel::new();
    let info = tenon_cli::info(&mut k, &dir.join("bracket.step")).unwrap();
    let v = info.json["shapes"][0]["volume_mm3"].as_f64().unwrap();
    let expected = 28800.0 - 456.0 * std::f64::consts::PI;
    assert!((v - expected).abs() < 1e-6 * expected, "STEP volume {v} vs {expected}");

    let stl = std::fs::read(dir.join("bracket.stl")).unwrap();
    assert!(tenon_io::stl::binary_triangle_count(&stl).unwrap() > 100);
    for png in ["bracket.png", "bracket-thick-base.png"] {
        assert!(std::fs::read(dir.join(png)).unwrap().starts_with(&[0x89, b'P', b'N', b'G']), "{png}");
    }

    // The saved project reopens and regenerates to the same part.
    let mut e = Engine::new(kernel(), &dir);
    e.exec("file.open", &json!({ "path": "bracket.tenon" })).unwrap();
    let m = e.exec("model.mass", &json!({})).unwrap();
    assert!((m["bodies"][0]["volume"].as_f64().unwrap() - expected).abs() < 1e-6 * expected);
    let tree = e.exec("model.regenerate", &json!({})).unwrap();
    assert_eq!(tree["features"].as_array().unwrap().len(), 6);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reopening_projects_does_not_leak_kernel_shapes() {
    // Regression: opening a file dropped the cached regeneration without releasing its shapes.
    let dir = scratch("leak");
    tenon_cli::demo_m1(kernel(), &dir).unwrap();
    let mut e = Engine::new(kernel(), &dir);
    let mut live = Vec::new();
    for _ in 0..4 {
        e.exec("file.open", &json!({ "path": "bracket.tenon" })).unwrap();
        e.exec("model.mass", &json!({})).unwrap();
        live.push(e.kernel.live_shapes());
    }
    assert!(live.windows(2).all(|w| w[0] == w[1]), "live shapes grow with every open: {live:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failing_script_names_the_step_and_keeps_going_no_further() {
    let dir = scratch("fail");
    let script = r#"{ "steps": [
        { "run": "sketch.create", "as": "s" },
        { "run": "sketch.rectangle", "with": { "sketch": "$s.feature", "x1": 0, "y1": 0, "x2": 10, "y2": 10 } },
        { "run": "model.extrude", "with": { "sketch": "$s.feature", "distance": 5 } },
        { "run": "model.mass", "expect": { "bodies.0.volume": 499 } },
        { "run": "file.save", "with": { "path": "never.tenon" } }
    ] }"#;
    let e = tenon_cli::run_script(kernel(), script, &dir).unwrap_err();
    assert!(e.contains("step 4 (model.mass)"), "{e}");
    assert!(e.contains("expected `bodies.0.volume` = 499"), "{e}");
    assert!(!dir.join("never.tenon").exists(), "steps after a failure do not run");

    let bad = tenon_cli::run_script(kernel(), r#"[{ "run": "sketch.line", "with": { "sketch": "$nope.feature" } }]"#, &dir).unwrap_err();
    assert!(bad.contains("step 1 (sketch.line)") && bad.contains("no earlier step"), "{bad}");
    let unknown = tenon_cli::run_script(kernel(), r#"[{ "run": "model.frobnicate" }]"#, &dir).unwrap_err();
    assert!(unknown.contains("unknown command"), "{unknown}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Sends messages through the stdio transport and returns the replies.
fn session(server: &mut Server, messages: &[Value]) -> Vec<Value> {
    let input: String = messages.iter().map(|m| format!("{m}\n")).collect();
    let mut out = Vec::new();
    serve(server, input.as_bytes(), &mut out).unwrap();
    String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).expect("every line is one JSON message")).collect()
}

fn call(id: u64, tool: &str, args: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": tool, "arguments": args } })
}

#[test]
fn mcp_server_builds_measures_and_renders_a_part() {
    let mut server = Server::new(Engine::new(kernel(), scratch("mcp")));
    let replies = session(
        &mut server,
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } } }),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            call(3, "run_command", json!({ "command": "sketch.create", "params": { "plane": "xy" } })),
            call(4, "run_command", json!({ "command": "sketch.rectangle", "params": { "sketch": 1, "x1": 0, "y1": 0, "x2": 20, "y2": 10 } })),
            call(5, "run_command", json!({ "command": "model.extrude", "params": { "sketch": 1, "distance": 5 } })),
            call(6, "measure", json!({})),
            call(7, "model_tree", json!({})),
            call(8, "query_topology", json!({ "body": 0 })),
            call(9, "render_png", json!({ "view": "iso", "width": 64, "height": 48 })),
            call(10, "export", json!({ "format": "step", "path": "box.step" })),
            call(11, "run_command", json!({ "command": "model.extrude", "params": { "sketch": 99, "distance": 5 } })),
            call(12, "no_such_tool", json!({})),
            json!({ "jsonrpc": "2.0", "id": 13, "method": "ping" }),
            json!({ "jsonrpc": "2.0", "id": 14, "method": "resources/list" }),
        ],
    );
    assert_eq!(replies.len(), 14, "one reply per request, none for the notification");
    let by_id = |id: u64| replies.iter().find(|r| r["id"] == id).unwrap_or_else(|| panic!("no reply {id}"));

    assert_eq!(by_id(1)["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(by_id(1)["result"]["serverInfo"]["name"], "tenon");
    let tools: Vec<&str> = by_id(2)["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for t in ["run_command", "list_commands", "model_tree", "query_topology", "measure", "render_png", "export"] {
        assert!(tools.contains(&t), "missing tool {t}");
    }
    for id in [3, 4, 5, 6, 7, 8, 9, 10] {
        assert_eq!(by_id(id)["result"]["isError"], false, "{}", by_id(id));
    }
    let volume = by_id(6)["result"]["structuredContent"]["bodies"][0]["volume"].as_f64().unwrap();
    assert!((volume - 1000.0).abs() < 1e-6, "{volume}");
    assert_eq!(by_id(7)["result"]["structuredContent"]["bodies"], 1);
    let faces = by_id(8)["result"]["structuredContent"]["faces"]["faces"].as_array().unwrap();
    assert_eq!(faces.len(), 6);
    assert!(faces.iter().all(|f| !f["name"].is_null()), "every face of an extrusion has a persistent name");
    let image = &by_id(9)["result"]["content"][0];
    assert_eq!(image["type"], "image");
    assert_eq!(image["mimeType"], "image/png");
    assert!(image["data"].as_str().unwrap().starts_with("iVBORw0KGgo"), "base64 of the PNG signature");
    assert!(server.engine.base.join("box.step").exists());

    // A failing command is a tool error the agent can read, not a protocol error.
    assert_eq!(by_id(11)["result"]["isError"], true);
    assert!(by_id(11)["result"]["content"][0]["text"].as_str().unwrap().contains("model.extrude failed"));
    assert_eq!(by_id(12)["error"]["code"], -32602);
    assert_eq!(by_id(13)["result"], json!({}));
    assert_eq!(by_id(14)["error"]["code"], -32601);
}

#[test]
fn mcp_transport_survives_garbage() {
    let mut server = Server::new(Engine::new(kernel(), "."));
    let mut out = Vec::new();
    let input = b"not json\n\n\xff\xfe\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n";
    serve(&mut server, &input[..], &mut out).unwrap();
    let replies: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(replies.len(), 3);
    assert_eq!(replies[0]["error"]["code"], -32700);
    assert_eq!(replies[1]["error"]["code"], -32700);
    assert_eq!(replies[2]["result"], json!({}));
    // Non-objects and requests without a method are invalid; client responses get no reply.
    assert_eq!(server.handle(&json!(42)).unwrap()["error"]["code"], -32600);
    assert_eq!(server.handle(&json!({ "jsonrpc": "2.0", "id": 7 })).unwrap()["error"]["code"], -32600);
    assert!(server.handle(&json!({ "jsonrpc": "2.0", "id": 7, "result": {} })).is_none());
    // Unknown protocol versions get the newest one we speak.
    let r = server.handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize", "params": { "protocolVersion": "1999-01-01" } })).unwrap();
    assert_eq!(r["result"]["protocolVersion"], tenon_cli::mcp::PROTOCOL_VERSIONS[0]);
}

#[test]
fn command_docs_are_generated_from_the_registry() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/commands.md");
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    let generated = tenon_cli::commands_markdown();
    assert!(
        on_disk == generated,
        "docs/commands.md is out of date; regenerate it with `pixi run cargo run -p tenon-cli -- commands --markdown --out docs/commands.md`"
    );
    for (id, ..) in tenon_cli::engine::all_commands() {
        assert!(generated.contains(&format!("`{id}`")), "{id}");
    }
}

#[test]
fn base64_matches_rfc_4648_vectors() {
    for (input, out) in
        [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]
    {
        assert_eq!(base64(input.as_bytes()), out);
    }
}

/// Performance budgets on the M1 demo part (docs/performance.md). Generous enough for slow CI
/// runners; they catch order-of-magnitude regressions, not noise.
const REGEN_BUDGET_MS: f64 = 500.0;
const TESSELLATE_BUDGET_MS: f64 = 500.0;

#[test]
fn m1_demo_part_regenerates_and_tessellates_within_budget() {
    let dir = scratch("perf");
    let mut e = Engine::new(kernel(), &dir);
    let script = tenon_cli::Script::parse(tenon_cli::M1_BRACKET_SCRIPT).unwrap();
    tenon_cli::script::run(&mut e, &script).map_err(|(_, err)| err.to_string()).unwrap();
    let doc = e.session.document().clone();

    let (mut regen, mut mesh) = (Vec::new(), Vec::new());
    for _ in 0..5 {
        let mut k = OcctKernel::new();
        let t = Instant::now();
        let mut r = tenon_model::regenerate(&doc, &mut k);
        regen.push(t.elapsed().as_secs_f64() * 1000.0);
        assert!(r.first_error().is_none());
        let t = Instant::now();
        let s = tenon_model::scene(&r, &mut k, &MeshTol::default()).unwrap();
        mesh.push(t.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(s.bodies.len(), 1);
        r.release(&mut k);
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let (r, m) = (median(&mut regen), median(&mut mesh));
    eprintln!("M1 demo part: regenerate {r:.1} ms, tessellate + measure {m:.1} ms (median of 5)");
    assert!(r <= REGEN_BUDGET_MS, "regeneration took {r:.1} ms; budget {REGEN_BUDGET_MS} ms");
    assert!(m <= TESSELLATE_BUDGET_MS, "tessellation took {m:.1} ms; budget {TESSELLATE_BUDGET_MS} ms");
    let _ = std::fs::remove_dir_all(&dir);
}

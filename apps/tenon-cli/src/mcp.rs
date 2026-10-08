//! A Model Context Protocol server over stdio (newline-delimited JSON-RPC 2.0), so agents can
//! build and inspect parts: run any command, read the model tree, query topology, measure,
//! render PNG images and export files. See docs/mcp.md.
//!
//! Stdout carries only protocol messages; diagnostics go to stderr.

use std::io::{BufRead, Read, Write};

use serde_json::{Value, json};

use crate::engine::{Engine, MAX_RENDER_SIDE, all_commands};

/// Protocol revisions this server speaks, newest first.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
/// Longest accepted message line (hostile-input cap).
pub const MAX_LINE: u64 = 16 * 1024 * 1024;

const INSTRUCTIONS: &str = "Tenon is a parametric CAD modeller (units: mm, radians). A part is a feature tree: sketches on \
planes or faces, then extrude/revolve features that join, cut or intersect. Typical flow: run_command sketch.create, \
add geometry and constraints with sketch.* commands (each returns the new ids), model.extrude, then check with \
model_tree, measure and render_png. list_commands shows every command and its parameters. To sketch on a face, get a \
reference with run_command model.face_ref (origin {type: cap|side, ...}) and pass it as `face` to sketch.create.";

pub struct Server {
    pub engine: Engine,
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

/// The tools this server offers.
pub fn tools() -> Vec<Value> {
    vec![
        tool("list_commands", "Every command run_command accepts, with its parameters.", json!({}), &[]),
        tool(
            "run_command",
            "Run one Tenon command on the open part, e.g. sketch.create, sketch.line, sketch.constrain, model.extrude, edit.undo, file.open. Returns the command's result (new ids etc.).",
            json!({
                "command": { "type": "string", "description": "Command id, e.g. \"sketch.rectangle\"" },
                "params": { "type": "object", "description": "Command parameters (see list_commands)" },
            }),
            &["command"],
        ),
        tool(
            "model_tree",
            "Regenerate the part and return its feature tree with each feature's status, the number of bodies and the first error.",
            json!({}),
            &[],
        ),
        tool(
            "query_topology",
            "Topology of the bodies (solids, faces, edges, vertices, validity, bounding box). With `body`, also lists that body's faces with their persistent names, surface type, area and centroid.",
            json!({ "body": { "type": "integer", "minimum": 0, "description": "Body index to list faces of" } }),
            &[],
        ),
        tool(
            "measure",
            "Mass properties of each body: volume (mm^3), surface area (mm^2), mass, centre of mass, inertia tensor; and the overall bounding box.",
            json!({ "density": { "type": "number", "description": "Mass per mm^3 (default 1)" } }),
            &[],
        ),
        tool(
            "render_png",
            "Render the part to a PNG image from a standard view, fitted to the frame.",
            json!({
                "view": { "type": "string", "enum": ["iso", "front", "back", "left", "right", "top", "bottom"] },
                "width": { "type": "integer", "minimum": 16, "maximum": MAX_RENDER_SIDE },
                "height": { "type": "integer", "minimum": 16, "maximum": MAX_RENDER_SIDE },
            }),
            &[],
        ),
        tool(
            "export",
            "Write the part to a file: a Tenon project (.tenon), STEP AP214 (.step) or binary STL (.stl).",
            json!({
                "format": { "type": "string", "enum": ["tenon", "step", "stl"] },
                "path": { "type": "string", "description": "Output file; relative paths resolve against the server's working directory" },
            }),
            &["format", "path"],
        ),
    ]
}

/// A tool's outcome: content blocks, structured data, and whether it failed.
struct ToolOut {
    content: Vec<Value>,
    structured: Option<Value>,
    error: bool,
}

impl ToolOut {
    fn data(v: Value) -> ToolOut {
        ToolOut { content: vec![json!({ "type": "text", "text": pretty(&v) })], structured: Some(v), error: false }
    }
    fn fail(message: impl Into<String>) -> ToolOut {
        ToolOut { content: vec![json!({ "type": "text", "text": message.into() })], structured: None, error: true }
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

fn rpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

impl Server {
    pub fn new(engine: Engine) -> Server {
        Server { engine }
    }

    /// Handles one parsed message; returns the response (none for notifications).
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        if let Value::Array(batch) = msg {
            let out: Vec<Value> = batch.iter().filter_map(|m| self.handle(m)).collect();
            return (!out.is_empty()).then_some(Value::Array(out));
        }
        let id = msg.get("id").cloned();
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            // A response from the client (we send no requests) or garbage.
            return id.map(|id| rpc_error(id, -32600, "invalid request"));
        };
        let id = id?; // notifications get no response
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result = match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
                let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[0]);
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "tenon", "title": "Tenon CAD", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                })
            }
            "ping" => json!({}),
            "tools/list" => json!({ "tools": tools() }),
            "tools/call" => {
                let Some(name) = params.get("name").and_then(Value::as_str) else {
                    return Some(rpc_error(id, -32602, "tools/call needs a tool `name`"));
                };
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let Some(out) = self.call(name, &args) else {
                    return Some(rpc_error(id, -32602, format!("unknown tool `{name}`")));
                };
                let mut r = json!({ "content": out.content, "isError": out.error });
                if let (Some(s), Some(o)) = (out.structured, r.as_object_mut()) {
                    o.insert("structuredContent".into(), s);
                }
                r
            }
            other => return Some(rpc_error(id, -32601, format!("method `{other}` not found"))),
        };
        Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    fn exec(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.engine.exec(id, &params)
    }

    /// Runs a tool; `None` if there is no such tool.
    fn call(&mut self, name: &str, args: &Value) -> Option<ToolOut> {
        let out = match name {
            "list_commands" => {
                let list: Vec<Value> = all_commands()
                    .into_iter()
                    .map(|(id, label, help, mutates)| json!({ "id": id, "label": label, "params": help, "undoable": mutates }))
                    .collect();
                ToolOut::data(json!({ "commands": list }))
            }
            "run_command" => match args.get("command").and_then(Value::as_str) {
                None => ToolOut::fail("run_command needs `command`"),
                Some(cmd) => {
                    let params = args.get("params").cloned().filter(|p| !p.is_null()).unwrap_or(json!({}));
                    match self.exec(cmd, params) {
                        Ok(v) => ToolOut::data(json!({ "command": cmd, "result": v })),
                        Err(e) => ToolOut::fail(format!("{cmd} failed: {e}")),
                    }
                }
            },
            "model_tree" => self.exec("model.regenerate", json!({})).map_or_else(ToolOut::fail, ToolOut::data),
            "query_topology" => match self.exec("model.topology", json!({})) {
                Err(e) => ToolOut::fail(e),
                Ok(mut topo) => match args.get("body").filter(|b| !b.is_null()) {
                    None => ToolOut::data(topo),
                    Some(b) => match self.exec("model.faces", json!({ "body": b })) {
                        Ok(faces) => {
                            if let Some(o) = topo.as_object_mut() {
                                o.insert("faces".into(), faces);
                            }
                            ToolOut::data(topo)
                        }
                        Err(e) => ToolOut::fail(e),
                    },
                },
            },
            "measure" => {
                let density = args.get("density").cloned().unwrap_or(Value::Null);
                match (self.exec("model.mass", json!({ "density": density })), self.exec("model.topology", json!({}))) {
                    (Ok(mut mass), Ok(topo)) => {
                        let boxes: Vec<Value> = topo["bodies"].as_array().map(|b| b.iter().map(|x| x["bbox"].clone()).collect()).unwrap_or_default();
                        if let Some(o) = mass.as_object_mut() {
                            o.insert("bounding_boxes".into(), Value::Array(boxes));
                            o.insert("units".into(), json!({ "length": "mm", "volume": "mm^3", "area": "mm^2" }));
                        }
                        ToolOut::data(mass)
                    }
                    (Err(e), _) | (_, Err(e)) => ToolOut::fail(e),
                }
            }
            "render_png" => {
                let view = args.get("view").and_then(Value::as_str).unwrap_or("iso");
                let side = |key: &str, d: u32| args.get(key).and_then(Value::as_u64).map_or(d, |v| v.clamp(16, u64::from(MAX_RENDER_SIDE)) as u32);
                match self.engine.render_png(view, side("width", 800), side("height", 600)) {
                    Ok(png) => ToolOut {
                        content: vec![json!({ "type": "image", "data": crate::base64(&png), "mimeType": "image/png" })],
                        structured: None,
                        error: false,
                    },
                    Err(e) => ToolOut::fail(e),
                }
            }
            "export" => {
                let cmd = match args.get("format").and_then(Value::as_str) {
                    Some("tenon") => "file.save",
                    Some("step") => "export.step",
                    Some("stl") => "export.stl",
                    _ => return Some(ToolOut::fail("`format` must be tenon, step or stl")),
                };
                let path = args.get("path").cloned().unwrap_or(Value::Null);
                self.exec(cmd, json!({ "path": path })).map_or_else(ToolOut::fail, ToolOut::data)
            }
            _ => return None,
        };
        Some(out)
    }
}

/// Serves requests from `input` until it closes.
pub fn serve(server: &mut Server, mut input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = input.by_ref().take(MAX_LINE).read_until(b'\n', &mut line)?;
        if n == 0 {
            return Ok(());
        }
        if line.last() != Some(&b'\n') && n as u64 >= MAX_LINE {
            skip_line(&mut input)?;
            write_msg(&mut output, &rpc_error(Value::Null, -32600, "message too large"))?;
            continue;
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let reply = match serde_json::from_slice::<Value>(&line) {
            Ok(msg) => server.handle(&msg),
            Err(e) => Some(rpc_error(Value::Null, -32700, format!("parse error: {e}"))),
        };
        if let Some(r) = reply {
            write_msg(&mut output, &r)?;
        }
    }
}

/// Discards input up to and including the next newline, without buffering it.
fn skip_line(input: &mut impl BufRead) -> std::io::Result<()> {
    loop {
        let buf = input.fill_buf()?;
        if buf.is_empty() {
            return Ok(());
        }
        match buf.iter().position(|b| *b == b'\n') {
            Some(i) => {
                input.consume(i + 1);
                return Ok(());
            }
            None => {
                let n = buf.len();
                input.consume(n);
            }
        }
    }
}

fn write_msg(out: &mut impl Write, v: &Value) -> std::io::Result<()> {
    serde_json::to_writer(&mut *out, v)?;
    out.write_all(b"\n")?;
    out.flush()
}

//! A Model Context Protocol server over stdio (newline-delimited JSON-RPC 2.0), so agents can
//! build, inspect, repair and render what Tenon makes: run any command (one, or several as
//! one step), read its schema, find faces and edges by description, see and repair broken
//! references, render PNG images with things picked out, and read back what was done as a
//! script. See docs/mcp.md.
//!
//! The server drives a [`Host`]: a headless [`Engine`], or a running Tenon through its control
//! port. Everything it does is a command of the registry, so every edit an agent makes is an
//! ordinary undoable step that a person can look at and take back.
//!
//! Stdout carries only protocol messages; diagnostics go to stderr.

use std::io::{BufRead, Read, Write};

use serde_json::{Value, json};

use crate::engine::{Engine, MAX_RENDER_SIDE, all_commands};

/// Protocol revisions this server speaks, newest first.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
/// Longest accepted message line (hostile-input cap).
pub const MAX_LINE: u64 = 16 * 1024 * 1024;
/// The schemas of every command, as a resource.
pub const SCHEMAS_URI: &str = "tenon://schemas/commands";

const INSTRUCTIONS: &str = "Tenon is a parametric CAD modeller (units: mm and radians). A part is a feature tree: sketches on \
planes or faces, then features (extrude, revolve, fillet, hole, ...) that join, cut or intersect. Everything is done by commands \
with JSON parameters: list_commands names them, describe_command gives one's exact parameters as JSON Schema, run_command runs \
one, run_commands runs several as one undo step (all of them, or none if one fails). Parameters are checked before anything \
runs, and a mistake is told by name. Quick starts: model.box, model.cylinder, model.sphere and model.torus make a dimensioned \
sketch and its feature in one go. To refer to a face or an edge (to fillet it, sketch on it, shell it open), use find_geometry: \
it finds them by what they are and where (the top face, the vertical edges, the wall of a hole) and returns a `reference` for \
each, which is what the commands take. Check the work with model_tree (each feature's status and the first error), measure and \
render_png (which can pick faces and edges out in colour). When a feature has lost a face or an edge it referred to, \
broken_references lists what is broken with the nearest replacements and repair_reference puts one in. session_log gives what \
was done so far as a script that `tenon-cli run` replays.";

/// What the server drives: something that runs Tenon commands.
pub trait Host {
    /// Runs a command by id and gives its result, or why it was refused.
    fn exec(&mut self, id: &str, params: &Value) -> Result<Value, String>;
    /// A number that changes whenever a document does: tells the commands that changed
    /// something from those that only looked.
    fn revision(&mut self) -> u64;
}

impl Host for Engine {
    fn exec(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        Engine::exec(self, id, params)
    }
    fn revision(&mut self) -> u64 {
        let parts: u64 = self.asm.parts.values().map(|p| p.session.revision()).sum();
        self.session
            .revision()
            .wrapping_add(self.asm.revision().wrapping_mul(1_000_003))
            .wrapping_add(self.drw.revision().wrapping_mul(1_000_033))
            .wrapping_add(parts)
    }
}

/// A command that was run through the server.
#[derive(Clone, Debug, PartialEq)]
pub struct Logged {
    pub run: String,
    pub with: Value,
    /// It changed a document (it did not only look).
    pub changed: bool,
}

pub struct Server {
    pub host: Box<dyn Host>,
    /// The commands run so far that succeeded, in order.
    pub log: Vec<Logged>,
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

/// A tool that is a command under another name: its input is the command's parameters.
fn command_tool(name: &str, command: &str, description: &str) -> Value {
    let mut schema = crate::engine::schema(command).map_or(json!({ "type": "object" }), tenon_model::schema::parameters);
    schema["$defs"] = tenon_model::schema::definitions();
    json!({ "name": name, "description": description, "inputSchema": schema })
}

/// The tools this server offers.
pub fn tools() -> Vec<Value> {
    vec![
        tool(
            "list_commands",
            "Every command there is, with its name, what it takes (in words) and whether it is an undoable edit. `prefix` narrows the list, e.g. \"sketch.\" or \"sheet.\". For a command's exact parameters use describe_command.",
            json!({ "prefix": { "type": "string", "description": "Only commands whose id starts with this" } }),
            &[],
        ),
        tool(
            "describe_command",
            "One command's parameters as JSON Schema (names, types, which are required, which go together), with its description.",
            json!({ "command": { "type": "string", "description": "Command id, e.g. \"model.extrude\"" } }),
            &["command"],
        ),
        tool(
            "run_command",
            "Run one Tenon command, e.g. sketch.create, sketch.line, sketch.constrain, model.extrude, model.box, edit.undo, file.open. Its parameters are checked against its schema first. Returns the command's result (new ids and names).",
            json!({
                "command": { "type": "string", "description": "Command id, e.g. \"sketch.rectangle\"" },
                "params": { "type": "object", "description": "The command's parameters (see describe_command)" },
            }),
            &["command"],
        ),
        tool(
            "run_commands",
            "Run several commands as one: all of them, as a single undo step, or (when one fails) none of them. Each step is {\"run\": command id, \"with\": parameters, \"as\": a name for its result}; a string \"$name.path\" in `with` is that part of an earlier step's result, e.g. \"$sk.feature\". Returns every step's result, or which step failed and why.",
            json!({
                "steps": { "type": "array", "items": { "type": "object" }, "description": "The steps, in order" },
                "as_one": { "type": "boolean", "description": "All or nothing, as one undo step (default true). false: the steps before a failure stand" },
            }),
            &["steps"],
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
        command_tool(
            "find_geometry",
            "model.find",
            "Find faces or edges of the part by what they are and where, each with a `reference` that commands take: the top face ({what: faces, facing: top}), the vertical edges ({what: edges, kind: line, along: z}), the walls of holes ({what: faces, hole: true}), the largest flat face ({what: faces, kind: plane, most: largest}), what a feature made ({what: faces, feature: id}).",
        ),
        tool(
            "measure",
            "Mass properties of each body: volume (mm^3), surface area (mm^2), mass, centre of mass, inertia tensor; and the overall bounding box.",
            json!({ "density": { "type": "number", "description": "g/cm^3 (default: the material of the part, or 1 without one)" } }),
            &[],
        ),
        tool(
            "broken_references",
            "The first failing feature's references that no longer find their face or edge, each with the nearest replacements (a `reference`, its number on the body, what it is, and how far it is from where the old one was) and `confident`: whether the nearest is clearly the one meant. Empty when nothing is broken.",
            json!({}),
            &[],
        ),
        tool(
            "repair_reference",
            "Put a face or an edge in place of a broken reference, as one undoable edit: `candidate` (0 is the nearest of broken_references), or `with` (a reference from find_geometry or model.face_ref / model.edge_ref). Returns the tree as it then is.",
            json!({
                "feature": { "type": "integer", "minimum": 0, "description": "The failing feature's id" },
                "path": { "type": "string", "description": "Which of its references, as broken_references lists it (e.g. \"/edges/0\")" },
                "candidate": { "type": "integer", "minimum": 0, "description": "Which replacement of broken_references to use, 0 the nearest" },
                "with": { "type": "object", "description": "Or the reference to use" },
            }),
            &["feature", "path"],
        ),
        tool(
            "render_png",
            "Render what is being worked on (the part, the assembly or the drawing sheet) to a PNG image from a standard view, fitted to the frame. On a part, faces and edges can be picked out in colour by their numbers (from find_geometry, query_topology or broken_references) to see which is which.",
            json!({
                "view": { "type": "string", "enum": ["iso", "front", "back", "left", "right", "top", "bottom"] },
                "width": { "type": "integer", "minimum": 16, "maximum": MAX_RENDER_SIDE },
                "height": { "type": "integer", "minimum": 16, "maximum": MAX_RENDER_SIDE },
                "highlight_faces": { "type": "array", "items": { "type": "integer" }, "description": "Faces of the part to pick out" },
                "highlight_edges": { "type": "array", "items": { "type": "integer" }, "description": "Edges of the part to pick out" },
                "body": { "type": "integer", "minimum": 0, "description": "The body those are of (default 0)" },
                "exploded": { "type": "boolean", "description": "An assembly as its exploded view has it" },
                "sheet": { "type": "integer", "minimum": 0, "description": "A drawing's sheet (default the first)" },
            }),
            &[],
        ),
        tool(
            "export",
            "Write the part to a file: a Tenon project (.tenon), STEP AP214 (.step), binary STL (.stl) or 3MF (.3mf).",
            json!({
                "format": { "type": "string", "enum": ["tenon", "step", "stl", "3mf"] },
                "path": { "type": "string", "description": "Output file; relative paths resolve against the server's working directory" },
            }),
            &["format", "path"],
        ),
        tool(
            "session_log",
            "What has been done through this server so far, as a script: the commands that changed something, in order, each with its parameters. `tenon-cli run` replays it from the same starting point. With `path`, the script is also written to that file; with `all`, the commands that only looked are included.",
            json!({
                "path": { "type": "string", "description": "Also write the script here (.json)" },
                "all": { "type": "boolean", "description": "Include commands that changed nothing (default false)" },
            }),
            &[],
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

/// One command as describe_command gives it.
pub fn described(id: &str) -> Option<Value> {
    let (_, label, help, undoable) = all_commands().into_iter().find(|c| c.0 == id)?;
    let schema = crate::engine::schema(id)?;
    Some(json!({
        "id": id,
        "label": label,
        "undoable": undoable,
        "description": help,
        "params": tenon_model::schema::parameters(schema),
        "$defs": tenon_model::schema::definitions(),
        "schema_version": tenon_model::schema::VERSION,
    }))
}

impl Server {
    /// A server driving a headless engine.
    pub fn new(engine: Engine) -> Server {
        Server::on(Box::new(engine))
    }

    /// A server driving `host`.
    pub fn on(host: Box<dyn Host>) -> Server {
        Server { host, log: Vec::new() }
    }

    /// Handles one parsed message; returns the response (none for notifications).
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        if let Value::Array(batch) = msg {
            let out: Vec<Value> = batch.iter().filter_map(|m| self.handle(m)).collect();
            return (!out.is_empty()).then_some(Value::Array(out));
        }
        let Value::Object(o) = msg else {
            return Some(rpc_error(Value::Null, -32600, "a message must be a JSON object"));
        };
        let id = o.get("id").cloned();
        let Some(method) = o.get("method").and_then(Value::as_str) else {
            // A response from the client needs no answer (we send no requests anyway).
            if o.contains_key("result") || o.contains_key("error") {
                return None;
            }
            return Some(rpc_error(id.unwrap_or(Value::Null), -32600, "missing `method`"));
        };
        let id = id?; // notifications get no response
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result = match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
                let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[0]);
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false }, "resources": { "listChanged": false, "subscribe": false } },
                    "serverInfo": { "name": "tenon", "title": "Tenon CAD", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                })
            }
            "ping" => json!({}),
            "tools/list" => json!({ "tools": tools() }),
            "resources/list" => json!({ "resources": [{
                "uri": SCHEMAS_URI,
                "name": "commands.json",
                "title": "Tenon command schemas",
                "description": "Every command's parameters as JSON Schema, in one document (docs/schemas/commands.json).",
                "mimeType": "application/json",
            }] }),
            "resources/templates/list" => json!({ "resourceTemplates": [] }),
            "resources/read" => match params.get("uri").and_then(Value::as_str) {
                Some(SCHEMAS_URI) => {
                    json!({ "contents": [{ "uri": SCHEMAS_URI, "mimeType": "application/json", "text": crate::commands_schema_text() }] })
                }
                Some(other) => return Some(rpc_error(id, -32002, format!("resource `{other}` not found"))),
                None => return Some(rpc_error(id, -32602, "resources/read needs a `uri`")),
            },
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

    /// Runs a command on the host, and notes it in the log when it succeeds.
    fn exec(&mut self, id: &str, params: Value) -> Result<Value, String> {
        let before = self.host.revision();
        let result = self.host.exec(id, &params)?;
        let changed = self.host.revision() != before;
        self.log.push(Logged { run: id.to_owned(), with: params, changed });
        Ok(result)
    }

    /// The log as a script (`tenon-cli run`): the commands that changed something, or all.
    /// (Text that begins with `$` is written `$$`, as a script has it when it is not a
    /// reference to an earlier step: the steps of a batch keep their own references.)
    pub fn script(&self, all: bool) -> Value {
        fn escaped(v: &Value) -> Value {
            match v {
                Value::String(s) if s.starts_with('$') => Value::String(format!("${s}")),
                Value::Array(a) => Value::Array(a.iter().map(escaped).collect()),
                Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), escaped(x))).collect()),
                other => other.clone(),
            }
        }
        let steps: Vec<Value> = self.log.iter().filter(|l| all || l.changed).map(|l| json!({ "run": l.run, "with": escaped(&l.with) })).collect();
        json!({ "name": "Session", "description": "What was done through the Tenon MCP server, in order.", "steps": steps })
    }

    /// Runs a tool; `None` if there is no such tool.
    fn call(&mut self, name: &str, args: &Value) -> Option<ToolOut> {
        let out = match name {
            "list_commands" => {
                let prefix = args.get("prefix").and_then(Value::as_str).unwrap_or("");
                let list: Vec<Value> = all_commands()
                    .into_iter()
                    .filter(|c| c.0.starts_with(prefix))
                    .map(|(id, label, help, mutates)| json!({ "id": id, "label": label, "params": help, "undoable": mutates }))
                    .collect();
                ToolOut::data(json!({ "schema_version": tenon_model::schema::VERSION, "commands": list }))
            }
            "describe_command" => match args.get("command").and_then(Value::as_str) {
                None => ToolOut::fail("describe_command needs `command`"),
                Some(id) => match described(id) {
                    Some(d) => ToolOut::data(d),
                    None => {
                        let near: Vec<&str> = all_commands()
                            .into_iter()
                            .map(|c| c.0)
                            .filter(|c| c.contains(id) || id.rsplit('.').next().is_some_and(|last| c.ends_with(last)))
                            .take(8)
                            .collect();
                        ToolOut::fail(if near.is_empty() {
                            format!("there is no command `{id}` (see list_commands)")
                        } else {
                            format!("there is no command `{id}`. Near it: {}", near.join(", "))
                        })
                    }
                },
            },
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
            "run_commands" => {
                let steps = args.get("steps").cloned().unwrap_or(Value::Null);
                if args.get("as_one").and_then(Value::as_bool).unwrap_or(true) {
                    self.exec("batch.run", json!({ "steps": steps })).map_or_else(ToolOut::fail, ToolOut::data)
                } else {
                    self.one_by_one(steps).map_or_else(ToolOut::fail, ToolOut::data)
                }
            }
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
            "find_geometry" => self.exec("model.find", args.clone()).map_or_else(ToolOut::fail, ToolOut::data),
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
            "broken_references" => self.exec("model.broken", json!({})).map_or_else(ToolOut::fail, ToolOut::data),
            "repair_reference" => self.repair(args).map_or_else(ToolOut::fail, ToolOut::data),
            "render_png" => {
                let mut params = json!({});
                for key in ["view", "highlight_faces", "highlight_edges", "body", "exploded", "sheet"] {
                    if let Some(v) = args.get(key).filter(|v| !v.is_null()) {
                        params[key] = v.clone();
                    }
                }
                let side = |key: &str, d: u32| args.get(key).and_then(Value::as_u64).map_or(d, |v| v.clamp(16, u64::from(MAX_RENDER_SIDE)) as u32);
                params["width"] = json!(side("width", 800));
                params["height"] = json!(side("height", 600));
                match self.exec("render.png", params) {
                    Ok(r) => match r["png_base64"].as_str() {
                        Some(data) => ToolOut {
                            content: vec![json!({ "type": "image", "data": data, "mimeType": "image/png" })],
                            structured: None,
                            error: false,
                        },
                        None => ToolOut::fail("the picture did not come back"),
                    },
                    Err(e) => ToolOut::fail(e),
                }
            }
            "export" => {
                let cmd = match args.get("format").and_then(Value::as_str) {
                    Some("tenon") => "file.save",
                    Some("step") => "export.step",
                    Some("stl") => "export.stl",
                    Some("3mf") => "export.3mf",
                    _ => return Some(ToolOut::fail("`format` must be tenon, step, stl or 3mf")),
                };
                let path = args.get("path").cloned().unwrap_or(Value::Null);
                self.exec(cmd, json!({ "path": path })).map_or_else(ToolOut::fail, ToolOut::data)
            }
            "session_log" => {
                let script = self.script(args.get("all").and_then(Value::as_bool).unwrap_or(false));
                match args.get("path").and_then(Value::as_str) {
                    None => ToolOut::data(script),
                    Some(path) => match std::fs::write(path, pretty(&script)) {
                        Ok(()) => ToolOut::data(json!({ "path": path, "steps": script["steps"].as_array().map_or(0, Vec::len), "script": script })),
                        Err(e) => ToolOut::fail(format!("cannot write {path}: {e}")),
                    },
                }
            }
            _ => return None,
        };
        Some(out)
    }

    /// Steps run one after another, each a command of its own in the log: those before a
    /// failure stand, and the log says exactly what was done.
    fn one_by_one(&mut self, steps: Value) -> Result<Value, String> {
        let steps: Vec<crate::script::Step> = serde_json::from_value(steps).map_err(|e| format!("`steps` are not steps of a script: {e}"))?;
        let mut saved = std::collections::BTreeMap::new();
        let mut done = Vec::new();
        for (i, step) in steps.iter().enumerate() {
            let outcome = crate::script::substitute(&step.with, &saved)
                .and_then(|params| self.exec(&step.run, if params.is_null() { json!({}) } else { params }))
                .and_then(|result| crate::script::check(&result, &step.expect, step.within.unwrap_or(crate::script::EXPECT_REL)).map(|()| result));
            match outcome {
                Ok(result) => {
                    if let Some(name) = &step.save_as {
                        saved.insert(name.clone(), result.clone());
                    }
                    done.push(json!({ "step": i + 1, "run": step.run, "result": result }));
                }
                Err(e) => return Err(format!("step {} ({}): {e}. The {} step(s) before it stand.", i + 1, step.run, done.len())),
            }
        }
        Ok(json!({ "steps": done, "as_one": false }))
    }

    /// `repair_reference`: the candidate asked for (looked up again, so that it is the one of
    /// the part as it now is) or the reference given, put in place of the broken one.
    fn repair(&mut self, args: &Value) -> Result<Value, String> {
        let feature = args.get("feature").and_then(Value::as_u64).ok_or("repair_reference needs `feature`: the failing feature's id")?;
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or("repair_reference needs `path`: which reference, as broken_references lists it")?
            .to_owned();
        let with = match (args.get("with").filter(|v| !v.is_null()), args.get("candidate").and_then(Value::as_u64)) {
            (Some(with), None) => with.clone(),
            (Some(_), Some(_)) => return Err("give `candidate` or `with`, not both".into()),
            (None, choice) => {
                let broken = self.exec("model.broken", json!({}))?;
                if broken["feature"].as_u64() != Some(feature) {
                    return Err(match broken["feature"].as_u64() {
                        Some(other) => format!("feature {feature} is not the one that fails: {other} ({}) is", broken["name"].as_str().unwrap_or("")),
                        None => "nothing is broken: every feature regenerates".to_owned(),
                    });
                }
                let entry = broken["broken"].as_array().and_then(|b| b.iter().find(|x| x["path"] == path.as_str())).ok_or_else(|| {
                    let paths: Vec<&str> = broken["broken"].as_array().into_iter().flatten().filter_map(|x| x["path"].as_str()).collect();
                    format!(
                        "`{path}` is not a broken reference of feature {feature}: its broken ones are {}",
                        if paths.is_empty() { "none".to_owned() } else { paths.join(", ") }
                    )
                })?;
                let candidates = entry["candidates"].as_array().cloned().unwrap_or_default();
                let index = match choice {
                    Some(i) => i as usize,
                    None if entry["confident"] == true => 0,
                    // No guess: a part that rebuilds with its fillet on the wrong edge is worse
                    // than one that says it is broken.
                    None => {
                        let far = |i: usize| candidates.get(i).and_then(|c| c["distance"].as_f64());
                        return Err(match (far(0), far(1)) {
                            (None, _) => {
                                "there is nothing like it on the part to put in its place: give `with`, a reference from find_geometry".to_owned()
                            }
                            (Some(a), next) => format!(
                                "which one? None of the {} replacements is clearly the one meant (the nearest is {a:.1} mm from where the old one was{}): look at them (broken_references; render_png with highlight_faces or highlight_edges) and give `candidate`, or `with`",
                                candidates.len(),
                                next.map_or_else(String::new, |b| format!(", the next {b:.1}"))
                            ),
                        });
                    }
                };
                candidates
                    .get(index)
                    .map(|c| c["reference"].clone())
                    .ok_or_else(|| format!("there is no candidate {index}: broken_references offers {}", candidates.len()))?
            }
        };
        self.exec("model.repair", json!({ "feature": feature, "path": path, "with": with }))?;
        self.exec("model.regenerate", json!({}))
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

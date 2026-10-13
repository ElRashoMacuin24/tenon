//! A document session with a kernel: what scripts and the MCP server drive. It runs every
//! registry command (model, file and assembly) plus the commands only a headless host offers.
//!
//! There is one part session, one assembly session and one drawing session. `asm.*` commands
//! work on the assembly, `drw.*` on the drawing, the others on the part. `render.png` shows
//! whichever was worked on last.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tenon_assembly::AsmSession;
use tenon_drawing::DrwSession;
use tenon_kernel::{Kernel, MeshTol};
use tenon_model::Session;
use tenon_model::schema::{CommandSchema, Ty, arg, opt};
use tenon_render::{Camera, StdView, raster};

/// Commands the engine adds to the registry: `(id, label, help)`.
pub const HOST_COMMANDS: &[(&str, &str, &str)] = &[
    (
        "render.png",
        "Render PNG",
        "path (.png, optional: without it the image is returned base64-encoded); view: iso | front | back | left | right | top | bottom (default iso); width, height (pixels, default 1024 x 768); exploded (assemblies: the exploded view); sheet (drawings: which sheet, drawn width pixels wide); highlight_faces, highlight_edges: [numbers] of a part's faces and edges to pick out in colour, on body (default 0). A part with a failing feature is drawn as far as it was rebuilt",
    ),
    (
        "batch.run",
        "Run as One",
        "steps: [{run: a command id, with: its parameters, as: a name for its result, expect: {path: value}}, ...], as a script's (docs/scripts.md): a string \"$name.path\" in `with` is that part of the result of the step saved as `name`. as_one (default true): every step is done and they are one undo step in each document they changed, or, when a step fails, none of them is (what the steps before it did is taken back; a file a step wrote stays written); such steps may not open or start a document, nor undo or redo. With as_one false the steps before a failure stand. Returns each step's result",
    ),
];

/// What the engine's own commands take (`tenon_model::schema`).
pub static HOST_SCHEMAS: &[CommandSchema] = &[
    CommandSchema::new(
        "render.png",
        &[
            opt("path", Ty::Path, "the image to write (.png); without it the image is returned base64-encoded"),
            opt("view", Ty::Word(&["iso", "front", "back", "left", "right", "top", "bottom"]), "where it is seen from (default iso)"),
            opt("width", Ty::Int, "pixels, 16 to 4096 (default 1024)"),
            opt("height", Ty::Int, "pixels, 16 to 4096 (default 768)"),
            opt("exploded", Ty::Bool, "an assembly as its exploded view has it (default false)"),
            opt("sheet", Ty::Id, "a drawing's sheet (default the first), drawn `width` pixels wide"),
            opt(
                "highlight_faces",
                Ty::List(&Ty::Int),
                "a part's faces to pick out in colour, by their numbers (as model.faces and model.find give them)",
            ),
            opt("highlight_edges", Ty::List(&Ty::Int), "a part's edges to pick out in colour, by their numbers"),
            opt("body", Ty::Int, "the body those faces and edges are of (default 0)"),
        ],
    ),
    CommandSchema::new(
        "batch.run",
        &[
            tenon_model::schema::req(
                "steps",
                Ty::List(&Ty::Object),
                "the commands to run, in order: {\"run\": id, \"with\": parameters, \"as\": a name for the result, \"expect\": {path: value}}",
            ),
            opt("as_one", Ty::Bool, "all of them as one undo step, or none of them (default true)"),
        ],
    ),
];

/// The schema of any command the engine runs.
pub fn schema(id: &str) -> Option<&'static CommandSchema> {
    HOST_SCHEMAS.iter().find(|s| s.id == id).or_else(|| tenon_io::schemas::schema(id))
}

/// What to pick out in a picture of the part: faces and edges of one body, by their numbers
/// (as model.faces, model.edges and model.find give them).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Highlight {
    pub body: usize,
    pub faces: Vec<u32>,
    pub edges: Vec<u32>,
}

/// The colour of what is picked out.
const HIGHLIGHT: [u8; 4] = [0xff, 0x8c, 0x1a, 255];

/// Where every document the engine holds stood at some moment: what is done after it can be
/// made one undo step in each document ([`Engine::squash_since`]) or taken back
/// ([`Engine::roll_back_to`]).
#[derive(Clone, Debug)]
pub struct Mark {
    part: u64,
    assembly: u64,
    /// The parts of the assembly, which are edited in place and have histories of their own.
    parts: Vec<(String, u64)>,
    drawing: u64,
}

/// Largest image side the engine renders.
pub const MAX_RENDER_SIDE: u32 = 4096;

/// Which document the last command worked on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Active {
    Part,
    Assembly,
    Drawing,
}

pub struct Engine {
    pub session: Session,
    pub asm: AsmSession,
    pub drw: DrwSession,
    pub active: Active,
    pub kernel: Box<dyn Kernel>,
    /// Relative paths in commands resolve against this directory.
    pub base: PathBuf,
}

impl Engine {
    pub fn new(kernel: Box<dyn Kernel>, base: impl Into<PathBuf>) -> Engine {
        Engine {
            session: Session::default(),
            asm: AsmSession::default(),
            drw: DrwSession::default(),
            active: Active::Part,
            kernel,
            base: base.into(),
        }
    }

    /// Where the part, the assembly (and its parts) and the drawing stand now.
    pub fn mark(&self) -> Mark {
        Mark {
            part: self.session.mark(),
            assembly: self.asm.mark(),
            parts: self.asm.parts.iter().map(|(key, p)| (key.clone(), p.session.mark())).collect(),
            drawing: self.drw.mark(),
        }
    }

    /// Makes what was done since `mark` one undo step in each document it changed.
    pub fn squash_since(&mut self, mark: &Mark) {
        self.session.squash_since(mark.part);
        self.asm.squash_since(mark.assembly);
        for (key, at) in &mark.parts {
            if let Some(p) = self.asm.parts.get_mut(key) {
                p.session.squash_since(*at);
            }
        }
        self.drw.squash_since(mark.drawing);
    }

    /// Takes back what was done since `mark` in every document, as if it had not been done.
    /// (A file that was written stays written.)
    pub fn roll_back_to(&mut self, mark: &Mark) {
        self.session.roll_back_to(mark.part);
        for (key, at) in &mark.parts {
            if let Some(p) = self.asm.parts.get_mut(key) {
                p.session.roll_back_to(*at);
            }
        }
        self.asm.roll_back_to(mark.assembly);
        self.drw.roll_back_to(mark.drawing);
    }

    /// Runs a command by id. `path` parameters are resolved against [`Engine::base`].
    ///
    /// What comes in here comes from outside (a script, an agent), so it is held to the
    /// command's schema first, in every build: a parameter that is not one of the command's, a
    /// missing one or one of the wrong type is told before anything runs, by name.
    pub fn exec(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        if let Some(schema) = schema(id) {
            tenon_model::schema::check(schema, params)?;
        }
        let params = self.resolve_paths(id, params);
        match id {
            "render.png" => tenon_model::schema::held(id, schema(id), &params, || self.render_command(&params)),
            "batch.run" => tenon_model::schema::held(id, schema(id), &params, || self.batch_command(&params)),
            _ if id.starts_with("asm.") => {
                self.active = Active::Assembly;
                tenon_io::asm::run(&mut self.asm, id, &params, Some(self.kernel.as_mut())).map_err(|e| e.to_string())
            }
            _ if id.starts_with("drw.") => {
                self.active = Active::Drawing;
                tenon_io::drw::run(&mut self.drw, id, &params, Some(self.kernel.as_mut())).map_err(|e| e.to_string())
            }
            _ => {
                self.active = Active::Part;
                tenon_io::cmd::run(&mut self.session, id, &params, Some(self.kernel.as_mut())).map_err(|e| e.to_string())
            }
        }
    }

    /// The parameters of the command `id` with its relative paths taken from the base folder.
    /// Which parameters are paths is for the command's schema to say: a parameter that is only
    /// called `path` may be something else (model.repair's is a place in a feature's
    /// definition, `/edges/0`, which on Windows reads as a relative path and was made one).
    fn resolve_paths(&self, id: &str, params: &Value) -> Value {
        let mut p = params.clone();
        let (Some(schema), Some(o)) = (schema(id), p.as_object_mut()) else { return p };
        for param in schema.params.iter().filter(|x| x.ty == Ty::Path) {
            let full = o
                .get(param.name)
                .and_then(Value::as_str)
                .filter(|path| !path.is_empty() && Path::new(path).is_relative())
                .map(|path| self.base.join(path).to_string_lossy().into_owned());
            if let Some(full) = full {
                o.insert(param.name.to_owned(), json!(full));
            }
        }
        p
    }

    fn batch_command(&mut self, p: &Value) -> Result<Value, String> {
        let steps = arg(p, "steps").cloned().unwrap_or(Value::Null);
        let as_one = arg(p, "as_one").and_then(Value::as_bool).unwrap_or(true);
        let steps: Vec<crate::script::Step> = serde_json::from_value(steps).map_err(|e| format!("`steps` are not steps of a script: {e}"))?;
        if steps.len() > crate::script::MAX_STEPS {
            return Err(format!("at most {} steps", crate::script::MAX_STEPS));
        }
        let script = crate::script::Script { name: None, description: None, steps };
        let run = if as_one { crate::script::run_as_one(self, &script) } else { crate::script::run(self, &script) };
        match run {
            Ok(done) => Ok(json!({
                "steps": done.iter().map(|s| json!({ "step": s.index, "run": s.run, "result": s.result })).collect::<Vec<_>>(),
                "as_one": as_one,
            })),
            Err((done, failed)) if as_one => Err(format!("{failed}. Nothing was changed: the {} step(s) before it were taken back.", done.len())),
            Err((done, failed)) => Err(format!("{failed}. The {} step(s) before it stand.", done.len())),
        }
    }

    fn render_command(&mut self, p: &Value) -> Result<Value, String> {
        let view = arg(p, "view").and_then(Value::as_str).unwrap_or("iso");
        let side = |key: &str, default: u32| -> Result<u32, String> {
            match arg(p, key) {
                None | Some(Value::Null) => Ok(default),
                Some(v) => v
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| (16..=MAX_RENDER_SIDE).contains(n))
                    .ok_or_else(|| format!("`{key}` must be a whole number of pixels from 16 to {MAX_RENDER_SIDE}")),
            }
        };
        let (mut w, mut h) = (side("width", 1024)?, side("height", 768)?);
        let exploded = arg(p, "exploded").and_then(Value::as_bool).unwrap_or(false);
        let png = match self.active {
            Active::Assembly => self.render_assembly_png(view, w, h, exploded)?,
            Active::Drawing => {
                // A sheet keeps its proportions: `width` sets the size.
                let sheet = arg(p, "sheet").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
                let (png, iw, ih) = self.render_sheet(sheet, w)?;
                (w, h) = (iw, ih);
                png
            }
            Active::Part => {
                let numbers = |key: &str| -> Result<Vec<u32>, String> {
                    match arg(p, key) {
                        None | Some(Value::Null) => Ok(Vec::new()),
                        Some(v) => serde_json::from_value(v.clone()).map_err(|_| format!("`{key}` must be a list of face or edge numbers")),
                    }
                };
                let body = arg(p, "body").and_then(Value::as_u64).unwrap_or(0) as usize;
                let highlight = Highlight { body, faces: numbers("highlight_faces")?, edges: numbers("highlight_edges")? };
                self.render_part(view, w, h, &highlight)?
            }
        };
        match arg(p, "path").and_then(Value::as_str) {
            Some(path) => {
                std::fs::write(path, &png).map_err(|e| format!("cannot write {path}: {e}"))?;
                Ok(json!({ "path": path, "width": w, "height": h, "bytes": png.len() }))
            }
            None => Ok(json!({ "width": w, "height": h, "png_base64": crate::base64(&png) })),
        }
    }

    /// `colors`: per mesh, the colour of the part it is of, where that has one.
    fn render_meshes(
        meshes: &[tenon_kernel::Placed],
        colors: &[Option<[u8; 3]>],
        view: &str,
        width: u32,
        height: u32,
        bbox: Option<tenon_geom::Aabb3>,
        highlight: &Highlight,
    ) -> Result<Vec<u8>, String> {
        let std_view = StdView::from_name(view).ok_or_else(|| format!("unknown view `{view}` (iso, front, back, left, right, top, bottom)"))?;
        let mut cam = Camera::default();
        cam.set_view(std_view);
        if let Some(b) = bbox {
            cam.fit(&b);
        }
        let body_colors = colors.iter().enumerate().filter_map(|(i, c)| c.map(|[r, g, b]| (i, [r, g, b, 255]))).collect();
        let style = raster::Style {
            body_colors,
            face_colors: highlight.faces.iter().map(|f| (highlight.body, *f, HIGHLIGHT)).collect(),
            edge_colors: highlight.edges.iter().map(|e| (highlight.body, *e, HIGHLIGHT)).collect(),
            ..raster::Style::default()
        };
        let img = raster::render(meshes, &cam, width, height, &style);
        raster::encode_png(&img)
    }

    /// The part as a PNG seen from a standard view, fitted to the image.
    pub fn render_png(&mut self, view: &str, width: u32, height: u32) -> Result<Vec<u8>, String> {
        self.render_part(view, width, height, &Highlight::default())
    }

    /// The part as a PNG, with some of its faces and edges picked out in colour.
    ///
    /// A part with a feature that fails is drawn as far as it was rebuilt: that is the part a
    /// broken reference has to be repaired on. Only a part with nothing to draw is an error.
    pub fn render_part(&mut self, view: &str, width: u32, height: u32, highlight: &Highlight) -> Result<Vec<u8>, String> {
        let regen = self.session.regen(self.kernel.as_mut()).clone();
        let scene = tenon_model::scene(&regen, self.kernel.as_mut(), &MeshTol::default())?;
        if scene.bodies.is_empty() {
            return Err(match regen.first_error() {
                Some((_, msg)) => format!("the part does not regenerate: {msg}"),
                None => "there is nothing to draw: the part has no solid yet".to_owned(),
            });
        }
        if let Some(body) = scene.bodies.get(highlight.body) {
            if let Some(f) = highlight.faces.iter().find(|f| **f as usize >= body.faces.len()) {
                return Err(format!("body {} has no face {f} to pick out: it has {}", highlight.body, body.faces.len()));
            }
            if let Some(e) = highlight.edges.iter().find(|e| **e as usize >= body.edges.len()) {
                return Err(format!("body {} has no edge {e} to pick out: it has {}", highlight.body, body.edges.len()));
            }
        } else if !(highlight.faces.is_empty() && highlight.edges.is_empty()) {
            return Err(format!("there is no body {} to pick faces or edges out on", highlight.body));
        }
        let meshes = scene.placed();
        let colors = vec![self.session.document().color(); meshes.len()];
        Self::render_meshes(&meshes, &colors, view, width, height, scene.bbox(), highlight)
    }

    /// A drawing sheet as a PNG, width pixels wide.
    pub fn render_sheet_png(&mut self, sheet: Option<u32>, width: u32) -> Result<Vec<u8>, String> {
        self.render_sheet(sheet, width).map(|(png, _, _)| png)
    }

    /// A drawing sheet as a PNG `width` pixels wide, with its width and height.
    fn render_sheet(&mut self, sheet: Option<u32>, width: u32) -> Result<(Vec<u8>, u32, u32), String> {
        self.drw.refresh(self.kernel.as_mut());
        let sheet = sheet.unwrap_or(self.drw.drawing().sheets[0].id.0);
        let graphics = tenon_io::drw::sheets_graphics(&self.drw, Some(sheet)).map_err(|e| e.to_string())?;
        let g = graphics.first().ok_or("no sheet")?;
        let img = tenon_render::sheet::rasterize(g, f64::from(width) / g.width.max(1.0));
        Ok((raster::encode_png(&img)?, img.width, img.height))
    }

    /// The assembly's visible components as a PNG (exploded or not).
    pub fn render_assembly_png(&mut self, view: &str, width: u32, height: u32, exploded: bool) -> Result<Vec<u8>, String> {
        self.asm.refresh(self.kernel.as_mut())?;
        let asm = self.asm.assembly();
        let frames = if exploded { tenon_assembly::session::exploded(asm) } else { asm.components.iter().map(|c| (c.id, c.placement)).collect() };
        // Each body from its part's own mesh, where its component stands: nothing is copied.
        let (mut meshes, mut colors) = (Vec::new(), Vec::new());
        for c in asm.components.iter().filter(|c| c.visible) {
            let (Some(scene), Some(f)) = (tenon_assembly::session::scene_of(&self.asm.parts, c), frames.get(&c.id)) else { continue };
            for b in &scene.bodies {
                meshes.push(tenon_kernel::Placed::new(&b.mesh, Some(f)));
                colors.push(self.asm.parts.get(&c.key()).and_then(|p| p.session.document().color()));
            }
        }
        let bbox = tenon_geom::Aabb3::from_points(meshes.iter().flat_map(|m| m.mesh.positions.iter().map(|p| m.point(*p))));
        Self::render_meshes(&meshes, &colors, view, width, height, bbox, &Highlight::default())
    }
}

/// Every command a script or agent can run: `(id, label, help, mutates)`.
pub fn all_commands() -> Vec<(&'static str, &'static str, &'static str, bool)> {
    let mut v: Vec<_> = tenon_io::cmd::all_commands().map(|c| (c.id, c.label, c.help, c.mutates)).collect();
    v.extend(tenon_io::asm::all_commands().map(|c| (c.id, c.label, c.help, c.mutates)));
    v.extend(tenon_io::drw::all_commands().map(|c| (c.id, c.label, c.help, c.mutates)));
    v.extend(HOST_COMMANDS.iter().map(|(id, label, help)| (*id, *label, *help, false)));
    v
}

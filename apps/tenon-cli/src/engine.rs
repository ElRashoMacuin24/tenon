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
use tenon_kernel::{Kernel, Mesh, MeshTol};
use tenon_model::Session;
use tenon_render::{Camera, StdView, raster};

/// Commands the engine adds to the registry: `(id, label, help)`.
pub const HOST_COMMANDS: &[(&str, &str, &str)] = &[(
    "render.png",
    "Render PNG",
    "path (.png, optional: without it the image is returned base64-encoded); view: iso | front | back | left | right | top | bottom (default iso); width, height (pixels, default 1024 x 768); exploded (assemblies: the exploded view); sheet (drawings: which sheet, drawn width pixels wide)",
)];

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

    /// Runs a command by id. `path` parameters are resolved against [`Engine::base`].
    pub fn exec(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        let params = self.resolve_path(params);
        match id {
            "render.png" => self.render_command(&params),
            _ if id.starts_with("asm.") => {
                self.active = Active::Assembly;
                tenon_io::asm::run(&mut self.asm, id, &params, Some(self.kernel.as_mut())).map_err(|e| e.to_string())
            }
            _ if id.starts_with("drw.") => {
                self.active = Active::Drawing;
                let params = self.resolve_key(&self.resolve_key(&params, "model"), "template");
                tenon_io::drw::run(&mut self.drw, id, &params, Some(self.kernel.as_mut())).map_err(|e| e.to_string())
            }
            _ => {
                self.active = Active::Part;
                tenon_io::cmd::run(&mut self.session, id, &params, Some(self.kernel.as_mut())).map_err(|e| e.to_string())
            }
        }
    }

    /// Resolves a relative path in key against the base folder.
    fn resolve_key(&self, params: &Value, key: &str) -> Value {
        let mut p = params.clone();
        let resolved = p
            .get(key)
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty() && Path::new(path).is_relative())
            .map(|path| self.base.join(path).to_string_lossy().into_owned());
        if let (Some(path), Some(o)) = (resolved, p.as_object_mut()) {
            o.insert(key.into(), json!(path));
        }
        p
    }

    fn resolve_path(&self, params: &Value) -> Value {
        let mut p = params.clone();
        let resolved = p
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty() && Path::new(path).is_relative())
            .map(|path| self.base.join(path).to_string_lossy().into_owned());
        if let (Some(path), Some(o)) = (resolved, p.as_object_mut()) {
            o.insert("path".into(), json!(path));
        }
        p
    }

    fn render_command(&mut self, p: &Value) -> Result<Value, String> {
        let view = p.get("view").and_then(Value::as_str).unwrap_or("iso");
        let side = |key: &str, default: u32| -> Result<u32, String> {
            match p.get(key) {
                None | Some(Value::Null) => Ok(default),
                Some(v) => v
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| (16..=MAX_RENDER_SIDE).contains(n))
                    .ok_or_else(|| format!("`{key}` must be a whole number of pixels from 16 to {MAX_RENDER_SIDE}")),
            }
        };
        let (mut w, mut h) = (side("width", 1024)?, side("height", 768)?);
        let exploded = p.get("exploded").and_then(Value::as_bool).unwrap_or(false);
        let png = match self.active {
            Active::Assembly => self.render_assembly_png(view, w, h, exploded)?,
            Active::Drawing => {
                // A sheet keeps its proportions: `width` sets the size.
                let sheet = p.get("sheet").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
                let (png, iw, ih) = self.render_sheet(sheet, w)?;
                (w, h) = (iw, ih);
                png
            }
            Active::Part => self.render_png(view, w, h)?,
        };
        match p.get("path").and_then(Value::as_str) {
            Some(path) => {
                std::fs::write(path, &png).map_err(|e| format!("cannot write {path}: {e}"))?;
                Ok(json!({ "path": path, "width": w, "height": h, "bytes": png.len() }))
            }
            None => Ok(json!({ "width": w, "height": h, "png_base64": crate::base64(&png) })),
        }
    }

    fn render_meshes(meshes: &[&Mesh], view: &str, width: u32, height: u32, bbox: Option<tenon_geom::Aabb3>) -> Result<Vec<u8>, String> {
        let std_view = StdView::from_name(view).ok_or_else(|| format!("unknown view `{view}` (iso, front, back, left, right, top, bottom)"))?;
        let mut cam = Camera::default();
        cam.set_view(std_view);
        if let Some(b) = bbox {
            cam.fit(&b);
        }
        let img = raster::render(meshes, &cam, width, height, &raster::Style::default());
        raster::encode_png(&img)
    }

    /// The part as a PNG seen from a standard view, fitted to the image.
    pub fn render_png(&mut self, view: &str, width: u32, height: u32) -> Result<Vec<u8>, String> {
        let regen = self.session.regen(self.kernel.as_mut()).clone();
        if let Some((_, msg)) = regen.first_error() {
            return Err(format!("the part does not regenerate: {msg}"));
        }
        let scene = tenon_model::scene(&regen, self.kernel.as_mut(), &MeshTol::default())?;
        let meshes: Vec<&Mesh> = scene.bodies.iter().map(|b| &b.mesh).collect();
        Self::render_meshes(&meshes, view, width, height, scene.bbox())
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
        let mut meshes = Vec::new();
        for c in asm.components.iter().filter(|c| c.visible) {
            let (Some(scene), Some(f)) = (tenon_assembly::session::scene_of(&self.asm.parts, c), frames.get(&c.id)) else { continue };
            for b in &scene.bodies {
                meshes.push(placed_mesh(&b.mesh, f));
            }
        }
        let bbox = tenon_geom::Aabb3::from_points(
            meshes.iter().flat_map(|m| m.positions.iter().map(|p| tenon_geom::Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])))),
        );
        let refs: Vec<&Mesh> = meshes.iter().collect();
        Self::render_meshes(&refs, view, width, height, bbox)
    }
}

/// A mesh moved by a placement.
pub fn placed_mesh(m: &Mesh, f: &tenon_geom::Frame) -> Mesh {
    use tenon_geom::Vec3;
    let p = |v: &[f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
    let out = |v: Vec3| [v.x as f32, v.y as f32, v.z as f32];
    let turn = |v: Vec3| f.x() * v.x + f.y() * v.y + f.z() * v.z;
    Mesh {
        positions: m.positions.iter().map(|v| out(f.to_world(p(v)))).collect(),
        normals: m.normals.iter().map(|v| out(turn(p(v)))).collect(),
        indices: m.indices.clone(),
        faces: m.faces.clone(),
        edges: m
            .edges
            .iter()
            .map(|e| tenon_kernel::EdgePolyline { edge: e.edge, points: e.points.iter().map(|v| out(f.to_world(p(v)))).collect() })
            .collect(),
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

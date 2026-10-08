//! A document session with a kernel: what scripts and the MCP server drive. It runs every
//! registry command (model and file) plus the commands only a headless host offers.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tenon_kernel::{Kernel, MeshTol};
use tenon_model::Session;
use tenon_render::{Camera, StdView, raster};

/// Commands the engine adds to the registry: `(id, label, help)`.
pub const HOST_COMMANDS: &[(&str, &str, &str)] = &[(
    "render.png",
    "Render PNG",
    "path (.png, optional: without it the image is returned base64-encoded); view: iso | front | back | left | right | top | bottom (default iso); width, height (pixels, default 1024 x 768)",
)];

/// Largest image side the engine renders.
pub const MAX_RENDER_SIDE: u32 = 4096;

pub struct Engine {
    pub session: Session,
    pub kernel: Box<dyn Kernel>,
    /// Relative paths in commands resolve against this directory.
    pub base: PathBuf,
}

impl Engine {
    pub fn new(kernel: Box<dyn Kernel>, base: impl Into<PathBuf>) -> Engine {
        Engine { session: Session::default(), kernel, base: base.into() }
    }

    /// Runs a command by id. `path` parameters are resolved against [`Engine::base`].
    pub fn exec(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        let params = self.resolve_path(params);
        match id {
            "render.png" => self.render_command(&params),
            _ => tenon_io::cmd::run(&mut self.session, id, &params, Some(self.kernel.as_mut())).map_err(|e| e.to_string()),
        }
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
        let (w, h) = (side("width", 1024)?, side("height", 768)?);
        let png = self.render_png(view, w, h)?;
        match p.get("path").and_then(Value::as_str) {
            Some(path) => {
                std::fs::write(path, &png).map_err(|e| format!("cannot write {path}: {e}"))?;
                Ok(json!({ "path": path, "width": w, "height": h, "bytes": png.len() }))
            }
            None => Ok(json!({ "width": w, "height": h, "png_base64": crate::base64(&png) })),
        }
    }

    /// The part as a PNG seen from a standard view, fitted to the image.
    pub fn render_png(&mut self, view: &str, width: u32, height: u32) -> Result<Vec<u8>, String> {
        let std_view = StdView::from_name(view).ok_or_else(|| format!("unknown view `{view}` (iso, front, back, left, right, top, bottom)"))?;
        let regen = self.session.regen(self.kernel.as_mut()).clone();
        if let Some((_, msg)) = regen.first_error() {
            return Err(format!("the part does not regenerate: {msg}"));
        }
        let scene = tenon_model::scene(&regen, self.kernel.as_mut(), &MeshTol::default())?;
        let mut cam = Camera::default();
        cam.set_view(std_view);
        if let Some(b) = scene.bbox() {
            cam.fit(&b);
        }
        let meshes: Vec<&tenon_kernel::Mesh> = scene.bodies.iter().map(|b| &b.mesh).collect();
        let img = raster::render(&meshes, &cam, width, height, &raster::Style::default());
        raster::encode_png(&img)
    }
}

/// Every command a script or agent can run: `(id, label, help, mutates)`.
pub fn all_commands() -> Vec<(&'static str, &'static str, &'static str, bool)> {
    let mut v: Vec<_> = tenon_io::cmd::all_commands().map(|c| (c.id, c.label, c.help, c.mutates)).collect();
    v.extend(HOST_COMMANDS.iter().map(|(id, label, help)| (*id, *label, *help, false)));
    v
}

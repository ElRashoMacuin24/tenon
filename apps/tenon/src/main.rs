//! Tenon desktop application.
//!
//! Usage: `tenon [FILE] [--theme light|dark] [--run COMMAND]... [--screenshot OUT.png] [--size WIDTHxHEIGHT] [--version]`
//!
//! FILE is a part (`.tenon`), an assembly (`.tenonasm`) or a drawing (`.tenondrw`). `--run` runs a
//! ribbon command (e.g. `model.extrude`) after it opens.
//!
//! `--screenshot` renders the window (after the model has regenerated), saves it as PNG and exits,
//! so agents and CI can check the UI without screen capture.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use tenon_kernel::Kernel;
use tenon_ui::{Services, ThemeName, Workbench};

/// Frames to wait for regeneration before taking a screenshot anyway.
const SCREENSHOT_MAX_FRAMES: u32 = 600;

struct App {
    wb: Workbench,
    screenshot: Option<PathBuf>,
    frames: u32,
    requested: bool,
}

/// Settings kept between sessions (eframe storage).
const THEME_KEY: &str = "tenon.theme";

fn parse_theme(s: &str) -> Option<ThemeName> {
    match s {
        "light" => Some(ThemeName::Light),
        "dark" => Some(ThemeName::Dark),
        _ => None,
    }
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Screenshot runs never change the user's settings.
        if self.screenshot.is_none() {
            storage.set_string(THEME_KEY, if self.wb.theme() == ThemeName::Light { "light".into() } else { "dark".into() });
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.wb.ui(ui, frame.wgpu_render_state());
        let ctx = ui.ctx().clone();
        if self.wb.exit_requested() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let Some(path) = self.screenshot.clone() else {
            return;
        };
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            match save_png(&path, &img) {
                Ok(()) => println!("wrote {}", path.display()),
                Err(e) => eprintln!("error: cannot write {}: {e}", path.display()),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        // Let layout settle and the model regenerate, then ask for the next frame's pixels.
        self.frames += 1;
        // Half a second lets window fade-ins and view glides finish.
        let ready = self.frames >= 3 && !self.wb.is_busy() && ctx.input(|i| i.time) >= 0.5;
        if (ready || self.frames >= SCREENSHOT_MAX_FRAMES) && !self.requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.requested = true;
        }
        ctx.request_repaint();
    }
}

fn save_png(path: &Path, img: &egui::ColorImage) -> Result<(), String> {
    let [w, h] = img.size;
    let (w, h) = (u32::try_from(w).map_err(|e| e.to_string())?, u32::try_from(h).map_err(|e| e.to_string())?);
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
    image::save_buffer(path, &bytes, w, h, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())
}

fn parse_size(s: &str) -> Option<[f32; 2]> {
    let (w, h) = s.split_once('x')?;
    let (w, h): (f32, f32) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (w.is_finite() && h.is_finite() && (320.0..=8192.0).contains(&w) && (240.0..=8192.0).contains(&h)).then_some([w, h])
}

/// Native file dialogs.
fn services() -> Services {
    fn label(ext: &str) -> &str {
        match ext {
            "tenon" => "Tenon part",
            "tenonasm" => "Tenon assembly",
            "tenondrw" => "Tenon drawing",
            "step" => "STEP",
            "stl" => "STL",
            "pdf" => "PDF",
            "svg" => "SVG",
            "dxf" => "DXF",
            "csv" => "CSV",
            "json" => "Title block template (JSON)",
            other => other,
        }
    }
    Services {
        // Parts, assemblies and drawings all open from File > Open.
        pick_open: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter("Tenon files", &["tenon", "tenonasm", "tenondrw"])
                .add_filter(label("tenon"), &["tenon"])
                .add_filter(label("tenonasm"), &["tenonasm"])
                .add_filter(label("tenondrw"), &["tenondrw"])
                .pick_file()
        })),
        pick_open_ext: Some(Box::new(|ext: &str| rfd::FileDialog::new().add_filter(label(ext), &[ext]).pick_file())),
        pick_save: Some(Box::new(|name: &str, ext: &str| {
            let path = rfd::FileDialog::new().set_file_name(name).add_filter(label(ext), &[ext]).save_file()?;
            // Some platforms return the name without the chosen filter's extension.
            Some(if path.extension().is_none() { path.with_extension(ext) } else { path })
        })),
    }
}

fn main() -> eframe::Result {
    let mut screenshot = None;
    let mut project = None;
    let mut theme = None;
    let mut run: Vec<String> = Vec::new();
    let mut size = [1440.0, 900.0];
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--version" | "-V" => {
                println!("tenon {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--run" => match args.next() {
                Some(id) => run.push(id),
                None => eprintln!("warning: --run expects a command id, e.g. model.extrude"),
            },
            "--theme" => match args.next().as_deref().and_then(parse_theme) {
                Some(t) => theme = Some(t),
                None => eprintln!("warning: --theme expects light or dark"),
            },
            "--size" => match args.next().as_deref().and_then(parse_size) {
                Some(s) => size = s,
                None => eprintln!("warning: --size expects WIDTHxHEIGHT, e.g. 1440x900; using the default"),
            },
            other if !other.starts_with('-') && project.is_none() => project = Some(PathBuf::from(other)),
            other => eprintln!("warning: ignoring unknown argument `{other}`"),
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Tenon").with_inner_size(size).with_min_inner_size([900.0, 560.0]),
        persist_window: screenshot.is_none(),
        ..Default::default()
    };
    eframe::run_native(
        "Tenon",
        options,
        Box::new(move |cc| {
            tenon_ui::theme::apply(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            let waker: Box<dyn Fn() + Send> = Box::new(move || ctx.request_repaint());
            let mut wb = Workbench::new(|| Box::new(tenon_kernel_occt::OcctKernel::new()) as Box<dyn Kernel>, Some(waker), services());
            let saved = cc.storage.and_then(|s| s.get_string(THEME_KEY)).and_then(|s| parse_theme(&s));
            if let Some(t) = theme.or(saved) {
                wb.set_theme(t);
            }
            if let Some(p) = project
                && let Err(e) = wb.open(&p)
            {
                eprintln!("error: {e}");
                wb.report_error(e);
            }
            for id in &run {
                if let Err(e) = wb.run_ui(id) {
                    eprintln!("error: {id}: {e}");
                    wb.report_error(e);
                }
            }
            Ok(Box::new(App { wb, screenshot, frames: 0, requested: false }))
        }),
    )
}

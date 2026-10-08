//! `tenon-cli`: headless Tenon.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;

const USAGE: &str = "\
tenon-cli: headless Tenon

usage:
  tenon-cli version [--json]                  Tenon and kernel versions
  tenon-cli run SCRIPT.json [--out DIR] [--json]
                                              run a command script (docs/scripts.md); relative
                                              paths in it resolve against DIR (default: .)
  tenon-cli commands [--json | --markdown [--out FILE]]
                                              list every command and its parameters
  tenon-cli render PROJECT.tenon OUT.png [--view iso|front|top|...] [--size WIDTHxHEIGHT]
                                              render a project to PNG without a GPU
  tenon-cli mcp [PROJECT.tenon]               Model Context Protocol server on stdin/stdout
  tenon-cli demo m0|m1 [--out DIR] [--json]   build a milestone demo part into DIR (default: out)
  tenon-cli info FILE.step [--json]           import STEP; report volume, area, bounding box, topology
  tenon-cli convert IN.step OUT.stl [--json]  tessellate STEP to binary STL (mm)
";

fn kernel() -> Box<dyn Kernel> {
    Box::new(OcctKernel::new())
}

fn parse_size(s: &str) -> Option<[u32; 2]> {
    let (w, h) = s.split_once('x')?;
    let (w, h): (u32, u32) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    let ok = |v: u32| (16..=tenon_cli::engine::MAX_RENDER_SIDE).contains(&v);
    (ok(w) && ok(h)).then_some([w, h])
}

fn main() -> ExitCode {
    let mut json = false;
    let mut markdown = false;
    let mut out: Option<PathBuf> = None;
    let mut view = "iso".to_owned();
    let mut size = [1024, 768];
    let mut pos = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--json" => json = true,
            "--markdown" => markdown = true,
            "--out" => match args.next() {
                Some(dir) => out = Some(PathBuf::from(dir)),
                None => return fail("--out needs a directory"),
            },
            "--view" => match args.next() {
                Some(v) => view = v,
                None => return fail("--view needs a view name"),
            },
            "--size" => match args.next().as_deref().and_then(parse_size) {
                Some(s) => size = s,
                None => return fail("--size expects WIDTHxHEIGHT, each 16 to 4096, e.g. 1024x768"),
            },
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ => pos.push(a),
        }
    }
    let pos: Vec<&str> = pos.iter().map(String::as_str).collect();
    let demo_dir = || out.clone().unwrap_or_else(|| PathBuf::from("out"));
    let result = match pos.as_slice() {
        ["version"] => Ok(tenon_cli::version(&OcctKernel::new())),
        ["run", file] => match std::fs::read_to_string(file) {
            Ok(text) => tenon_cli::run_script(kernel(), &text, &out.clone().unwrap_or_else(|| PathBuf::from("."))),
            Err(e) => Err(format!("cannot read {file}: {e}")),
        },
        ["commands"] if markdown => {
            let md = tenon_cli::commands_markdown();
            match &out {
                Some(file) => match std::fs::write(file, md) {
                    Ok(()) => println!("wrote {}", file.display()),
                    Err(e) => return fail(&format!("cannot write {}: {e}", file.display())),
                },
                None => print!("{md}"),
            }
            return ExitCode::SUCCESS;
        }
        ["commands"] => Ok(tenon_cli::commands()),
        ["render", project, png] => tenon_cli::render(kernel(), Path::new(project), Path::new(png), &view, size),
        ["mcp"] | ["mcp", _] => return mcp(pos.get(1).copied()),
        ["demo", "m0"] => tenon_cli::demo_m0(&mut OcctKernel::new(), &demo_dir()),
        ["demo", "m1"] => tenon_cli::demo_m1(kernel(), &demo_dir()),
        ["info", file] => tenon_cli::info(&mut OcctKernel::new(), Path::new(file)),
        ["convert", input, output] => tenon_cli::convert(&mut OcctKernel::new(), Path::new(input), Path::new(output)),
        _ => return fail(&format!("unrecognised command\n\n{USAGE}")),
    };
    match result {
        Ok(r) if json => {
            println!("{}", serde_json::to_string_pretty(&r.json).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Ok(r) => {
            println!("{}", r.text);
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e),
    }
}

fn mcp(project: Option<&str>) -> ExitCode {
    let mut engine = tenon_cli::Engine::new(kernel(), ".");
    if let Some(p) = project
        && let Err(e) = engine.exec("file.open", &serde_json::json!({ "path": p }))
    {
        return fail(&e);
    }
    let mut server = tenon_cli::mcp::Server::new(engine);
    eprintln!("tenon {} MCP server on stdio", env!("CARGO_PKG_VERSION"));
    match tenon_cli::mcp::serve(&mut server, std::io::stdin().lock(), std::io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&format!("MCP transport: {e}")),
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("error: {msg}");
    ExitCode::FAILURE
}

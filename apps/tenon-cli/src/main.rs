//! `tenon-cli`: headless Tenon.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tenon_kernel_occt::OcctKernel;

const USAGE: &str = "\
tenon-cli: headless Tenon

usage:
  tenon-cli version [--json]                 Tenon and kernel versions
  tenon-cli demo m0 [--out DIR] [--json]     build the M0 demo bracket; write DIR/bracket.step and .stl
  tenon-cli info FILE.step [--json]          import STEP; report volume, area, bounding box, topology
  tenon-cli convert IN.step OUT.stl [--json] tessellate STEP to binary STL (mm)
";

fn main() -> ExitCode {
    let mut json = false;
    let mut out = PathBuf::from("out");
    let mut pos = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--json" => json = true,
            "--out" => match args.next() {
                Some(dir) => out = PathBuf::from(dir),
                None => return fail("--out needs a directory"),
            },
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ => pos.push(a),
        }
    }
    let mut k = OcctKernel::new();
    let pos: Vec<&str> = pos.iter().map(String::as_str).collect();
    let result = match pos.as_slice() {
        ["version"] => Ok(tenon_cli::version(&k)),
        ["demo", "m0"] => tenon_cli::demo_m0(&mut k, &out),
        ["info", file] => tenon_cli::info(&mut k, Path::new(file)),
        ["convert", input, output] => tenon_cli::convert(&mut k, Path::new(input), Path::new(output)),
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

fn fail(msg: &str) -> ExitCode {
    eprintln!("error: {msg}");
    ExitCode::FAILURE
}

//! Compiles the C++ shim (cxx) and links OpenCASCADE 8 dynamically.
//!
//! OCCT is looked up, in order, at:
//! 1. `OCCT_ROOT`: an install prefix (pixi sets it to its environment),
//! 2. `CONDA_PREFIX`: any active conda or pixi environment,
//! 3. `<workspace>/.pixi/envs/default`: the pixi environment, when cargo runs outside `pixi run`.
//!
//! Headers are expected under `<prefix>/include/opencascade` (or `<prefix>/Library/include/opencascade`
//! for conda on Windows) and libraries under the matching `lib` directory.

use std::path::{Path, PathBuf};

/// OCCT toolkits the shim uses (linked dynamically).
const OCCT_LIBS: &[&str] = &[
    "TKernel",
    "TKMath",
    "TKG2d",
    "TKG3d",
    "TKGeomBase",
    "TKBRep",
    "TKGeomAlgo",
    "TKTopAlgo",
    "TKPrim",
    "TKBO",
    "TKBool",
    "TKFillet",
    "TKOffset",
    "TKFeat",
    "TKMesh",
    "TKShHealing",
    "TKXSBase",
    "TKDE",
    "TKDESTEP",
];

const REQUIRED_MAJOR: &str = "8";

fn fail(msg: &str) -> ! {
    eprintln!("\nerror: tenon-kernel-occt: {msg}\n");
    eprintln!("OpenCASCADE 8 is provided by pixi. From the workspace root run `pixi install`, then build");
    eprintln!("with `pixi run cargo build`, or set OCCT_ROOT to an OCCT 8 install prefix. See docs/setup.md.\n");
    std::process::exit(1)
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for var in ["OCCT_ROOT", "CONDA_PREFIX"] {
        if let Some(v) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            out.push(PathBuf::from(v));
        }
    }
    if let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR") {
        let ws = Path::new(&manifest).join("..").join("..");
        out.push(ws.join(".pixi").join("envs").join("default"));
    }
    out
}

/// `(include dir, lib dir)` of an OCCT install under `prefix`, if headers are there.
fn layout(prefix: &Path) -> Option<(PathBuf, PathBuf)> {
    [prefix.join("Library"), prefix.to_path_buf()].into_iter().find_map(|base| {
        let inc = base.join("include").join("opencascade");
        inc.join("Standard_Version.hxx").is_file().then(|| (inc, base.join("lib")))
    })
}

fn occt_major(inc: &Path) -> Option<String> {
    let text = std::fs::read_to_string(inc.join("Standard_Version.hxx")).ok()?;
    text.lines().find_map(|l| l.trim().strip_prefix("#define OCC_VERSION_MAJOR").map(|v| v.trim().to_owned()))
}

fn main() {
    println!("cargo:rerun-if-env-changed=OCCT_ROOT");
    println!("cargo:rerun-if-env-changed=CONDA_PREFIX");
    for f in ["build.rs", "src/ffi.rs", "shim/tenon_occt.h", "shim/tenon_occt.cpp"] {
        println!("cargo:rerun-if-changed={f}");
    }

    let tried = candidates();
    let Some((inc, lib)) = tried.iter().find_map(|p| layout(p)) else {
        let list: Vec<String> = tried.iter().map(|p| p.display().to_string()).collect();
        fail(&format!("OpenCASCADE headers not found (looked in: {})", list.join(", ")));
    };
    match occt_major(&inc) {
        Some(m) if m == REQUIRED_MAJOR => {}
        Some(m) => fail(&format!("found OpenCASCADE {m}.x at {}, but Tenon needs {REQUIRED_MAJOR}.x", inc.display())),
        None => fail(&format!("cannot read the OpenCASCADE version from {}", inc.display())),
    }

    println!("cargo:rustc-link-search=native={}", lib.display());
    for l in OCCT_LIBS {
        println!("cargo:rustc-link-lib=dylib={l}");
    }
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        // Lets this crate's own tests find the shared libraries without LD_LIBRARY_PATH.
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
    }

    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let mut build = cxx_build::bridge("src/ffi.rs");
    build.file("shim/tenon_occt.cpp").include(&inc).std("c++17").define("_USE_MATH_DEFINES", None);
    if msvc {
        build.define("NOMINMAX", None).flag("/EHsc").flag("/utf-8").flag("/bigobj").flag("/Zc:__cplusplus");
    } else {
        build.flag_if_supported("-Wno-deprecated-declarations").flag_if_supported("-Wno-unused-parameter");
    }
    build.compile("tenon_occt_shim");
}

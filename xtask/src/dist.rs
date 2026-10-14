//! `cargo xtask dist`: Tenon as a folder that runs on a machine with neither pixi nor Rust, and
//! an archive of it (DEC-101).
//!
//! The release builds of `tenon` and `tenon-cli` are put beside the shared libraries they need
//! from the pixi environment: OpenCASCADE, and what it needs in turn, found by reading each
//! binary's own list of imports. With them go the licence of every one of those libraries and
//! of every Rust crate built in, the example parts and the drawing templates. Then the folder
//! is tried as a user would have it: its programs are run with nothing of the environment on
//! their search paths.
//!
//! Layout:
//! - Windows: `tenon-<v>-windows-x64/`, the programs and their DLLs side by side;
//! - Linux: `tenon-<v>-linux-x64/bin` and `lib` (the programs look in `$ORIGIN/../lib`);
//! - macOS: `tenon-<v>-macos-<arch>/Tenon.app`, the libraries in `Contents/Frameworks`.
//!
//! Options: `--no-build` (package the release binaries already built), `--no-check`,
//! `--no-archive`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{cargo, root, run};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Os {
    Windows,
    Linux,
    Mac,
}

const OS: Os = if cfg!(windows) {
    Os::Windows
} else if cfg!(target_os = "macos") {
    Os::Mac
} else {
    Os::Linux
};

const PROGRAMS: [&str; 2] = ["tenon", "tenon-cli"];

/// The user's first page, put at the top of the folder (`{version}` is filled in).
const README: &str = include_str!("../dist/README.txt");

const INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Tenon</string>
  <key>CFBundleDisplayName</key><string>Tenon</string>
  <key>CFBundleIdentifier</key><string>io.github.elrashomacuin24.tenon</string>
  <key>CFBundleVersion</key><string>{version}</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleExecutable</key><string>tenon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#;

/// Where the pieces of the folder go.
struct Layout {
    top: PathBuf,
    programs: PathBuf,
    libraries: PathBuf,
}

impl Layout {
    fn new(top: PathBuf) -> Layout {
        let (programs, libraries) = match OS {
            Os::Windows => (top.clone(), top.clone()),
            Os::Linux => (top.join("bin"), top.join("lib")),
            Os::Mac => {
                let contents = top.join("Tenon.app").join("Contents");
                (contents.join("MacOS"), contents.join("Frameworks"))
            }
        };
        Layout { top, programs, libraries }
    }

    fn program(&self, name: &str) -> PathBuf {
        self.programs.join(exe(name))
    }
}

fn exe(name: &str) -> String {
    if OS == Os::Windows { format!("{name}.exe") } else { name.to_owned() }
}

pub fn run_dist(args: &[&str]) -> Result<(), String> {
    for a in args {
        if !["--no-build", "--no-check", "--no-archive"].contains(a) {
            return Err(format!("dist: unknown option `{a}` (--no-build, --no-check, --no-archive)"));
        }
    }
    let prefix = environment()?;
    let meta = full_metadata()?;
    let version = meta["packages"]
        .as_array()
        .and_then(|p| p.iter().find(|p| p["name"] == "tenon"))
        .and_then(|p| p["version"].as_str())
        .ok_or("dist: no version of the `tenon` package in cargo metadata")?
        .to_owned();
    let target = PathBuf::from(meta["target_directory"].as_str().ok_or("dist: no target directory in cargo metadata")?);

    if !args.contains(&"--no-build") {
        build()?;
    }
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    let os = match OS {
        Os::Windows => "windows",
        Os::Linux => "linux",
        Os::Mac => "macos",
    };
    let name = format!("tenon-{version}-{os}-{arch}");
    let dist = target.join("dist");
    let layout = Layout::new(dist.join(&name));
    if layout.top.exists() {
        std::fs::remove_dir_all(&layout.top).map_err(|e| format!("dist: cannot clear {}: {e}", layout.top.display()))?;
    }
    for dir in [&layout.top, &layout.programs, &layout.libraries] {
        std::fs::create_dir_all(dir).map_err(|e| format!("dist: cannot make {}: {e}", dir.display()))?;
    }

    // The programs.
    let mut roots = Vec::new();
    for p in PROGRAMS {
        let built = target.join("release").join(exe(p));
        if !built.is_file() {
            return Err(format!("dist: {} is not built: run without --no-build", built.display()));
        }
        let to = layout.program(p);
        copy(&built, &to)?;
        roots.push(to);
    }

    // The libraries they need, and the libraries those need, from the environment.
    let (bundled, system) = closure(&roots, &library_dirs(&prefix))?;
    for (name, from) in &bundled {
        copy(from, &layout.libraries.join(name))?;
    }
    eprintln!("dist: {} libraries from the environment", bundled.len());
    eprintln!("dist: left to the system: {}", system.iter().cloned().collect::<Vec<_>>().join(", "));

    // Licences: of the libraries, of the Rust crates built in, and Tenon's own.
    let licences = layout.top.join("licenses");
    let mut summary = String::from(concat!(
        "Third-party software in this folder\n",
        "===================================\n\n",
        "Tenon itself is MIT OR Apache-2.0 (LICENSE-MIT, LICENSE-APACHE, NOTICE). It is built\n",
        "with the software below. Each licence text is in the folder named beside it.\n\n",
    ));
    summary += &conda_licences(&prefix, &bundled, &licences)?;
    summary += &crate_licences(&meta, &licences)?;
    write(&licences.join("THIRD-PARTY.txt"), &summary)?;
    for f in ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ATTRIBUTION.md", "CHANGELOG.md"] {
        copy(&root().join(f), &layout.top.join(f))?;
    }
    write(&layout.top.join("README.txt"), &README.replace("{version}", &version))?;

    // What a user starts from: the example parts, assemblies and drawings, and the templates.
    copy_tree(&root().join("examples"), &layout.top.join("examples"), &["tenon", "tenonasm", "tenondrw", "json", "md", "csv"])?;
    copy_tree(&root().join("assets").join("templates"), &layout.top.join("templates"), &["tenondrw", "json"])?;

    if OS == Os::Mac {
        let contents = layout.top.join("Tenon.app").join("Contents");
        write(&contents.join("Info.plist"), &INFO_PLIST.replace("{version}", &version))?;
        // One signature over the whole bundle (ad hoc: Tenon has no certificate).
        let mut sign = Command::new("codesign");
        sign.args(["--force", "--deep", "--sign", "-"]).arg(layout.top.join("Tenon.app"));
        run(sign, "codesign --force --deep --sign - Tenon.app")?;
    }

    if !args.contains(&"--no-check") {
        check(&layout)?;
    }
    if !args.contains(&"--no-archive") {
        let archive = archive(&dist, &name)?;
        eprintln!("dist: {}", archive.display());
    }
    eprintln!("dist: {}", layout.top.display());
    Ok(())
}

/// The pixi environment the binaries were built against.
fn environment() -> Result<PathBuf, String> {
    let prefix = std::env::var_os("CONDA_PREFIX").map(PathBuf::from).unwrap_or_else(|| root().join(".pixi").join("envs").join("default"));
    if prefix.join("conda-meta").is_dir() {
        Ok(prefix)
    } else {
        Err(format!("dist: no pixi environment at {} (run `pixi install`, then `pixi run cargo xtask dist`)", prefix.display()))
    }
}

/// The folders of the environment that hold its shared libraries.
fn library_dirs(prefix: &Path) -> Vec<PathBuf> {
    match OS {
        Os::Windows => vec![prefix.join("Library").join("bin"), prefix.to_path_buf(), prefix.join("Library").join("mingw-w64").join("bin")],
        Os::Linux | Os::Mac => vec![prefix.join("lib")],
    }
}

/// The workspace with its dependencies, as built for this machine (crates for other
/// platforms are not in the programs).
fn full_metadata() -> Result<serde_json::Value, String> {
    let rustc = Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into())).arg("-vV").output().map_err(|e| format!("rustc -vV: {e}"))?;
    let host =
        String::from_utf8_lossy(&rustc.stdout).lines().find_map(|l| l.strip_prefix("host: ").map(str::to_owned)).ok_or("rustc -vV: no host")?;
    let out = cargo().args(["metadata", "--format-version", "1", "--filter-platform", &host]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

/// The release build, told where its libraries will be (Linux and macOS look where the
/// program says; Windows looks beside it).
fn build() -> Result<(), String> {
    let mut c = cargo();
    c.args(["build", "--release", "-p", "tenon", "-p", "tenon-cli"]);
    let rpath = match OS {
        Os::Windows => None,
        Os::Linux => Some("-C link-arg=-Wl,-rpath,$ORIGIN/../lib"),
        Os::Mac => Some("-C link-arg=-Wl,-rpath,@executable_path/../Frameworks"),
    };
    if let Some(rpath) = rpath {
        let flags = std::env::var("RUSTFLAGS").map(|f| format!("{f} {rpath}")).unwrap_or_else(|_| rpath.to_owned());
        c.env("RUSTFLAGS", flags);
    }
    run(c, "cargo build --release -p tenon -p tenon-cli")
}

// ---- imports

/// The libraries a program or library asks for by name.
pub fn imports(file: &Path) -> Result<Vec<String>, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let what = || format!("{}", file.display());
    match bytes.get(..4) {
        Some([b'M', b'Z', ..]) => pe_imports(&bytes).ok_or_else(|| format!("{}: not a PE file that can be read", what())),
        Some([0x7f, b'E', b'L', b'F']) => elf_imports(&bytes).ok_or_else(|| format!("{}: not a 64-bit ELF file that can be read", what())),
        Some([0xcf, 0xfa, 0xed, 0xfe]) => macho_imports(&bytes).ok_or_else(|| format!("{}: not a 64-bit Mach-O file that can be read", what())),
        _ => Err(format!("{}: neither PE, ELF nor 64-bit Mach-O", what())),
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

fn c_string(b: &[u8], at: usize) -> Option<String> {
    let rest = b.get(at..)?;
    let end = rest.iter().position(|c| *c == 0)?;
    String::from_utf8(rest[..end].to_vec()).ok()
}

/// A Windows program's imports: its import table and its delay-load table.
fn pe_imports(b: &[u8]) -> Option<Vec<String>> {
    let pe = u32_at(b, 0x3c)? as usize;
    if b.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let coff = pe + 4;
    let sections = u16_at(b, coff + 2)? as usize;
    let optional = coff + 20;
    let optional_size = u16_at(b, coff + 16)? as usize;
    // PE32+ (0x20b) has its data directories at 112; PE32 (0x10b) at 96.
    let directories = optional
        + match u16_at(b, optional)? {
            0x20b => 112,
            0x10b => 96,
            _ => return None,
        };
    let table = optional + optional_size;
    let offset_of = |rva: u32| -> Option<usize> {
        (0..sections).find_map(|i| {
            let s = table + i * 40;
            let (virtual_size, address, raw_size, raw) = (u32_at(b, s + 8)?, u32_at(b, s + 12)?, u32_at(b, s + 16)?, u32_at(b, s + 20)?);
            let size = virtual_size.max(raw_size);
            (rva >= address && rva < address + size).then(|| (rva - address + raw) as usize)
        })
    };
    let mut names = Vec::new();
    // (Directory 1: imports, 20 bytes each, the name at 12; 13: delay-load imports, 32 bytes
    // each, the name at 4.)
    for (directory, size, name_at) in [(1usize, 20usize, 12usize), (13, 32, 4)] {
        let rva = u32_at(b, directories + directory * 8)?;
        if rva == 0 {
            continue;
        }
        let mut at = offset_of(rva)?;
        loop {
            let name = u32_at(b, at + name_at)?;
            if name == 0 {
                break;
            }
            names.push(c_string(b, offset_of(name)?)?);
            at += size;
        }
    }
    Some(names)
}

/// A Linux program's imports: the DT_NEEDED entries of its dynamic section.
fn elf_imports(b: &[u8]) -> Option<Vec<String>> {
    // 64-bit, little-endian.
    if b.get(4) != Some(&2) || b.get(5) != Some(&1) {
        return None;
    }
    let (phoff, phentsize, phnum) = (u64_at(b, 0x20)? as usize, u16_at(b, 0x36)? as usize, u16_at(b, 0x38)? as usize);
    let headers: Vec<(u32, usize, u64, usize)> = (0..phnum)
        .filter_map(|i| {
            let h = phoff + i * phentsize;
            Some((u32_at(b, h)?, u64_at(b, h + 8)? as usize, u64_at(b, h + 16)?, u64_at(b, h + 32)? as usize))
        })
        .collect();
    let offset_of = |address: u64| -> Option<usize> {
        headers
            .iter()
            .find(|(kind, _, vaddr, size)| *kind == 1 && address >= *vaddr && address < vaddr + *size as u64)
            .map(|(_, off, vaddr, _)| off + (address - vaddr) as usize)
    };
    let &(_, dynamic, _, size) = headers.iter().find(|(kind, ..)| *kind == 2)?;
    let mut needed = Vec::new();
    let mut strings = None;
    for at in (dynamic..dynamic + size).step_by(16) {
        let (tag, value) = (u64_at(b, at)?, u64_at(b, at + 8)?);
        match tag {
            0 => break,
            1 => needed.push(value as usize),
            5 => strings = Some(offset_of(value)?),
            _ => {}
        }
    }
    let strings = strings?;
    needed.into_iter().map(|n| c_string(b, strings + n)).collect()
}

/// A macOS program's imports: its LC_LOAD_DYLIB commands (and the weak, re-exported and lazy
/// ones).
fn macho_imports(b: &[u8]) -> Option<Vec<String>> {
    let commands = u32_at(b, 16)? as usize;
    let mut at = 32;
    let mut names = Vec::new();
    for _ in 0..commands {
        let (kind, size) = (u32_at(b, at)?, u32_at(b, at + 4)? as usize);
        if [0xc, 0x8000_0018, 0x8000_001f, 0x20].contains(&kind) {
            names.push(c_string(b, at + u32_at(b, at + 8)? as usize)?);
        }
        at += size;
    }
    Some(names)
}

/// Every library the programs need that the environment has, by the name asked for, and the
/// names that are left to the system.
fn closure(roots: &[PathBuf], dirs: &[PathBuf]) -> Result<(BTreeMap<String, PathBuf>, BTreeSet<String>), String> {
    let mut bundled: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut system = BTreeSet::new();
    let mut todo: Vec<PathBuf> = roots.to_vec();
    while let Some(file) = todo.pop() {
        for asked in imports(&file)? {
            // macOS: "@rpath/libTKernel.8.0.dylib" is the environment's; "/usr/lib/..." the
            // system's.
            let name = match asked.rsplit_once('/') {
                Some((dir, name)) if dir.starts_with('@') => name.to_owned(),
                Some(_) => {
                    system.insert(asked);
                    continue;
                }
                None => asked,
            };
            // (Windows 10 and later have the C runtime's own DLLs: they are the system's even
            // where the environment has copies.)
            let lower = name.to_lowercase();
            if OS == Os::Windows && (lower.starts_with("api-ms-") || lower.starts_with("ext-ms-") || lower == "ucrtbase.dll") {
                system.insert(name);
                continue;
            }
            let key = if OS == Os::Windows { lower } else { name.clone() };
            if bundled.keys().any(|k| (if OS == Os::Windows { k.to_lowercase() } else { k.clone() }) == key) || system.contains(&name) {
                continue;
            }
            match dirs.iter().map(|d| d.join(&name)).find(|p| p.is_file()) {
                Some(found) => {
                    todo.push(found.clone());
                    bundled.insert(name, found);
                }
                None => {
                    system.insert(name);
                }
            }
        }
    }
    Ok((bundled, system))
}

// ---- licences

/// A path within the environment as it is compared: with forward slashes, and on Windows
/// without regard to case ("MSVCP140.dll" is asked for, "msvcp140.dll" installed).
fn same_path(p: &str) -> String {
    let p = p.replace('\\', "/");
    if OS == Os::Windows { p.to_lowercase() } else { p }
}

/// The conda packages the bundled libraries come from, each with its licence texts.
fn conda_licences(prefix: &Path, bundled: &BTreeMap<String, PathBuf>, licences: &Path) -> Result<String, String> {
    let wanted: BTreeSet<String> = bundled.values().filter_map(|p| p.strip_prefix(prefix).ok()).map(|p| same_path(&p.to_string_lossy())).collect();
    let mut found: BTreeSet<String> = BTreeSet::new();
    let mut text = String::from("Shared libraries, from conda-forge (https://conda-forge.org):\n\n");
    let meta = prefix.join("conda-meta");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&meta)
        .map_err(|e| format!("dist: cannot read {}: {e}", meta.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    let mut records = Vec::new();
    for path in paths {
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).map_err(|e| format!("dist: {}: {e}", path.display()))?)
            .map_err(|e| format!("dist: {}: {e}", path.display()))?;
        records.push(j);
    }
    let package_dir = |j: &serde_json::Value| PathBuf::from(j["extracted_package_dir"].as_str().unwrap_or_default());
    // The feedstock a package was built from: a package split off another (a runtime library
    // of its own) may carry no licence text, which the package it was split from has.
    let feedstock = |j: &serde_json::Value| -> Option<String> {
        let about: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(package_dir(j).join("info").join("about.json")).ok()?).ok()?;
        about["extra"]["remote_url"].as_str().map(str::to_owned)
    };
    let texts_of = |j: &serde_json::Value| Some(package_dir(j).join("info").join("licenses")).filter(|d| d.is_dir());
    for j in &records {
        let files: Vec<&str> = j["files"].as_array().map(|f| f.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
        let ours: Vec<&str> = files.iter().copied().filter(|f| wanted.contains(&same_path(f))).collect();
        if ours.is_empty() {
            continue;
        }
        let (name, version, licence) =
            (j["name"].as_str().unwrap_or("?"), j["version"].as_str().unwrap_or("?"), j["license"].as_str().unwrap_or("unknown"));
        let (from, note) = match texts_of(j) {
            Some(own) => (own, String::new()),
            None => {
                let source = feedstock(j);
                let sibling = records.iter().find(|r| r["name"] != j["name"] && source.is_some() && feedstock(r) == source && texts_of(r).is_some());
                match sibling {
                    Some(r) => {
                        (texts_of(r).unwrap_or_default(), format!(", the text of {}, built from the same source", r["name"].as_str().unwrap_or("?")))
                    }
                    None => {
                        return Err(format!(
                            "dist: the licence text of {name} {version} is not in the package cache ({}): reinstall the environment (`pixi clean`, then `pixi install`)",
                            package_dir(j).display()
                        ));
                    }
                }
            }
        };
        let to = licences.join("conda").join(name);
        if copy_tree(&from, &to, &[])? == 0 {
            return Err(format!("dist: no licence text in {}", from.display()));
        }
        text += &format!("  {name} {version}: {licence} (licenses/conda/{name}{note})\n");
        found.extend(ours.iter().map(|f| same_path(f)));
    }
    if let Some(missing) = wanted.iter().find(|w| !found.contains(*w)) {
        return Err(format!("dist: no conda package says it installed {missing}"));
    }
    Ok(text + "\n")
}

/// The Rust crates built into the programs (not the ones that only build them), each with the
/// licence files it is published with.
fn crate_licences(meta: &serde_json::Value, licences: &Path) -> Result<String, String> {
    let packages: BTreeMap<&str, &serde_json::Value> =
        meta["packages"].as_array().into_iter().flatten().filter_map(|p| Some((p["id"].as_str()?, p))).collect();
    let members: BTreeSet<&str> = meta["workspace_members"].as_array().into_iter().flatten().filter_map(|m| m.as_str()).collect();
    let nodes: BTreeMap<&str, &serde_json::Value> =
        meta["resolve"]["nodes"].as_array().into_iter().flatten().filter_map(|n| Some((n["id"].as_str()?, n))).collect();
    let proc_macro = |p: &serde_json::Value| {
        p["targets"].as_array().into_iter().flatten().any(|t| t["kind"].as_array().into_iter().flatten().any(|k| k == "proc-macro"))
    };
    let mut todo: Vec<&str> =
        packages.iter().filter(|(_, p)| PROGRAMS.contains(&p["name"].as_str().unwrap_or_default())).map(|(id, _)| *id).collect();
    let mut seen: BTreeSet<&str> = todo.iter().copied().collect();
    while let Some(id) = todo.pop() {
        for dep in nodes.get(id).and_then(|n| n["deps"].as_array()).into_iter().flatten() {
            // Normal dependencies only: build and dev dependencies are not in the programs.
            let normal = dep["dep_kinds"].as_array().into_iter().flatten().any(|k| k["kind"].is_null());
            let Some(dep_id) = dep["pkg"].as_str() else { continue };
            if normal && !packages.get(dep_id).is_some_and(|p| proc_macro(p)) && seen.insert(dep_id) {
                todo.push(dep_id);
            }
        }
    }
    let mut text = String::from("Rust crates, from crates.io (https://crates.io):\n\n");
    let mut by_licence: BTreeMap<String, usize> = BTreeMap::new();
    for id in seen.iter().filter(|id| !members.contains(*id)) {
        let p = packages[id];
        let (name, version) = (p["name"].as_str().unwrap_or("?"), p["version"].as_str().unwrap_or("?"));
        let licence =
            p["license"].as_str().map(str::to_owned).unwrap_or_else(|| format!("see {}", p["license_file"].as_str().unwrap_or("its repository")));
        *by_licence.entry(licence.clone()).or_default() += 1;
        let dir = Path::new(p["manifest_path"].as_str().unwrap_or_default()).parent().map(Path::to_path_buf).unwrap_or_default();
        let to = licences.join("rust").join(format!("{name}-{version}"));
        let mut copied = 0;
        for entry in std::fs::read_dir(&dir).map_err(|e| format!("dist: cannot read {}: {e}", dir.display()))?.filter_map(Result::ok) {
            let file = entry.file_name().to_string_lossy().to_uppercase();
            if entry.path().is_file() && ["LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT", "UNLICENSE"].iter().any(|w| file.starts_with(w)) {
                copy(&entry.path(), &to.join(entry.file_name()))?;
                copied += 1;
            }
        }
        let authors: Vec<&str> = p["authors"].as_array().into_iter().flatten().filter_map(|a| a.as_str()).collect();
        let source = p["repository"].as_str().unwrap_or("");
        if copied > 0 {
            text += &format!("  {name} {version}: {licence} (licenses/rust/{name}-{version})\n");
        } else {
            // (Published without a licence file: its licence by name, and who holds it.)
            let by = if authors.is_empty() { "its authors".to_owned() } else { authors.join(", ") };
            text += &format!("  {name} {version}: {licence}; by {by}; {source}\n");
        }
    }
    text += "\nBy licence:\n";
    for (licence, n) in by_licence {
        text += &format!("  {n:4}  {licence}\n");
    }
    Ok(text)
}

// ---- files

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("dist: cannot make {}: {e}", dir.display()))?;
    }
    // (A symbolic link is copied as the file it points to, under the name asked for.)
    std::fs::copy(from, to).map(|_| ()).map_err(|e| format!("dist: cannot copy {} to {}: {e}", from.display(), to.display()))
}

fn write(to: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("dist: cannot make {}: {e}", dir.display()))?;
    }
    std::fs::write(to, text).map_err(|e| format!("dist: cannot write {}: {e}", to.display()))
}

/// Copies the files of a folder and its folders whose extension is one of `kinds` (every file
/// when `kinds` is empty). Returns how many.
fn copy_tree(from: &Path, to: &Path, kinds: &[&str]) -> Result<usize, String> {
    let mut n = 0;
    let mut entries: Vec<_> =
        std::fs::read_dir(from).map_err(|e| format!("dist: cannot read {}: {e}", from.display()))?.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        let path = e.path();
        if path.is_dir() {
            n += copy_tree(&path, &to.join(e.file_name()), kinds)?;
        } else if kinds.is_empty() || path.extension().and_then(|x| x.to_str()).is_some_and(|x| kinds.contains(&x)) {
            copy(&path, &to.join(e.file_name()))?;
            n += 1;
        }
    }
    Ok(n)
}

// ---- trying it

/// Runs the folder's programs as a user would: with nothing of the build's environment on
/// their search paths.
fn check(layout: &Layout) -> Result<(), String> {
    let scratch = layout.top.parent().unwrap_or(&layout.top).join("check");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|e| format!("dist: cannot make {}: {e}", scratch.display()))?;
    let bare = |program: &Path| {
        let mut c = Command::new(program);
        c.env_clear().current_dir(&scratch);
        match OS {
            Os::Windows => {
                let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
                c.env("SystemRoot", &system).env("PATH", format!(r"{system}\System32;{system}"));
                for keep in ["TEMP", "TMP", "USERPROFILE", "APPDATA", "LOCALAPPDATA"] {
                    if let Ok(v) = std::env::var(keep) {
                        c.env(keep, v);
                    }
                }
            }
            Os::Linux | Os::Mac => {
                c.env("PATH", "/usr/bin:/bin");
                if let Ok(home) = std::env::var("HOME") {
                    c.env("HOME", home);
                }
            }
        }
        c
    };
    let cli = layout.program("tenon-cli");
    let tries: [(PathBuf, Vec<&str>, &str); 3] = [
        (cli.clone(), vec!["version"], "tenon-cli version"),
        (cli, vec!["demo", "m1", "--out", "m1"], "tenon-cli demo m1"),
        (layout.program("tenon"), vec!["--version"], "tenon --version"),
    ];
    for (program, args, what) in tries {
        let out = bare(&program).args(&args).output().map_err(|e| format!("dist: {what}: cannot start: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "dist: {what}, run from the folder alone, failed ({}):\n{}{}",
                out.status,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        eprintln!("dist: {what}: ok {}", String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or_default());
    }
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(())
}

fn archive(dist: &Path, name: &str) -> Result<PathBuf, String> {
    let file = dist.join(if OS == Os::Windows { format!("{name}.zip") } else { format!("{name}.tar.gz") });
    let _ = std::fs::remove_file(&file);
    let mut tar = Command::new("tar");
    tar.current_dir(dist);
    if OS == Os::Windows {
        // (Windows' own tar writes a zip when asked by the name's ending.)
        tar.args(["-a", "-c", "-f"]).arg(&file).arg(name);
    } else {
        tar.args(["-c", "-z", "-f"]).arg(&file).arg(name);
    }
    run(tar, &format!("tar {}", file.display()))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_programs_imports_are_read_from_it() {
        // This test's own program: it needs the system's libraries, by name.
        let me = std::env::current_exe().unwrap();
        let names = imports(&me).unwrap();
        let wanted = match OS {
            Os::Windows => "kernel32.dll",
            Os::Linux => "libc.so.6",
            Os::Mac => "/usr/lib/libSystem.B.dylib",
        };
        assert!(names.iter().any(|n| n.to_lowercase() == wanted.to_lowercase()), "{names:?}");
        assert!(imports(&root().join("Cargo.toml")).unwrap_err().contains("neither PE, ELF nor"));
    }
}

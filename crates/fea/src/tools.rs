//! The two programs a study is worked out by, found on the machine and started.
//!
//! Gmsh divides the part into elements and CalculiX solves them. Both are programs of their
//! own, each under its own licence (the GPL): Tenon does not contain them, link to them or
//! ship them. It starts them, hands each a file and reads the file it writes (DEC-098).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Where the two programs are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tools {
    pub gmsh: PathBuf,
    pub ccx: PathBuf,
}

/// Set to stop a program that is running: looked at while it runs.
pub type Cancel = Arc<AtomicBool>;

/// What a run that was asked to stop returns as its error: told apart from a failure.
pub const STOPPED: &str = "stopped";

/// The variables that say where the programs are, when they are not where they are looked for.
pub const GMSH_VAR: &str = "TENON_GMSH";
pub const CCX_VAR: &str = "TENON_CCX";

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() }
}

/// Where a pixi or conda environment keeps its programs.
fn programs_of(environment: &Path) -> PathBuf {
    if cfg!(windows) { environment.join("Library").join("bin") } else { environment.join("bin") }
}

/// The environment a checkout of Tenon keeps the programs in (`pixi install -e fea`), looked
/// for from `start` upward.
fn checkout_environment(start: &Path) -> Option<PathBuf> {
    start.ancestors().take(8).map(|dir| dir.join(".pixi").join("envs").join("fea")).find(|env| env.is_dir())
}

fn find_one(var: &str, name: &str) -> Option<PathBuf> {
    // Where the variable says, and nowhere else when it says.
    if let Some(said) = std::env::var_os(var).filter(|v| !v.is_empty()) {
        let said = PathBuf::from(said);
        return said.is_file().then_some(said);
    }
    let file = exe(name);
    // A checkout's own environment: from the working folder, and from the running program.
    let starts = [std::env::current_dir().ok(), std::env::current_exe().ok()];
    for start in starts.into_iter().flatten() {
        if let Some(found) = checkout_environment(&start).map(|env| programs_of(&env).join(&file)).filter(|p| p.is_file()) {
            return Some(found);
        }
    }
    // The search path.
    std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path).map(|dir| dir.join(&file)).find(|p| p.is_file()))
}

impl Tools {
    /// Finds both programs: where `TENON_GMSH` and `TENON_CCX` say, in a checkout's `fea`
    /// environment, or on the search path. The error says which is missing and what to do.
    pub fn find() -> Result<Tools, String> {
        let (gmsh, ccx) = (find_one(GMSH_VAR, "gmsh"), find_one(CCX_VAR, "ccx"));
        match (gmsh, ccx) {
            (Some(gmsh), Some(ccx)) => Ok(Tools { gmsh, ccx }),
            (gmsh, ccx) => {
                let missing: Vec<&str> =
                    [(gmsh.is_none(), "Gmsh (gmsh), which divides the part into elements"), (ccx.is_none(), "CalculiX (ccx), which solves them")]
                        .into_iter()
                        .filter(|(missing, _)| *missing)
                        .map(|(_, what)| what)
                        .collect();
                Err(format!(
                    "Working a study out takes two programs that are not part of Tenon, and {} not found: {}. Install them (docs/stress.md says how), or say where they are with {GMSH_VAR} and {CCX_VAR}.",
                    if missing.len() == 1 { "one was" } else { "they were" },
                    missing.join("; ")
                ))
            }
        }
    }
}

/// The folders of a conda environment whose libraries a program of it needs on Windows, when
/// it is started from outside the environment. (Elsewhere a program finds its own.)
fn library_folders(program: &Path) -> Vec<PathBuf> {
    // <environment>\Library\bin\program.exe
    let environment = program.parent().and_then(Path::parent).and_then(Path::parent);
    match environment {
        Some(env) if cfg!(windows) && env.join("Library").is_dir() => {
            let lib = env.join("Library");
            vec![env.to_path_buf(), lib.join("mingw-w64").join("bin"), lib.join("usr").join("bin"), lib.join("bin"), env.join("Scripts")]
        }
        _ => Vec::new(),
    }
}

/// Starts `program` in `dir` and waits for it, with all it says kept in the file `log` there.
/// Returns what it said. It is stopped, and an error returned, when `cancel` is set or it
/// runs longer than `limit`; an error too when it ends saying it failed.
pub fn run(program: &Path, args: &[&str], dir: &Path, log: &str, cancel: &Cancel, limit: Duration) -> Result<String, String> {
    let name = program.file_stem().map_or_else(|| "the program".to_owned(), |n| n.to_string_lossy().into_owned());
    let log_path = dir.join(log);
    let out = std::fs::File::create(&log_path).map_err(|e| format!("cannot write {}: {e}", log_path.display()))?;
    let err = out.try_clone().map_err(|e| format!("cannot write {}: {e}", log_path.display()))?;
    let mut command = Command::new(program);
    command.args(args).current_dir(dir).stdin(Stdio::null()).stdout(Stdio::from(out)).stderr(Stdio::from(err));
    let folders = library_folders(program);
    if !folders.is_empty() {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let all = folders.into_iter().chain(std::env::split_paths(&path));
        if let Ok(joined) = std::env::join_paths(all) {
            command.env("PATH", joined);
        }
    }
    // (As many threads as the machine has: both programs use what they are given.)
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    command.env("OMP_NUM_THREADS", threads.to_string());
    #[cfg(windows)]
    {
        // No console window flashing up beside the app.
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|e| format!("cannot start {}: {e}", program.display()))?;
    let started = Instant::now();
    let said = || std::fs::read_to_string(&log_path).unwrap_or_default();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(said()),
            Ok(Some(status)) => return Err(format!("{name} failed ({status}). It said:\n{}", tail(&said(), 12))),
            Ok(None) => {}
            Err(e) => return Err(format!("{name} could not be waited for: {e}")),
        }
        let stop = if cancel.load(Ordering::Relaxed) {
            Some(STOPPED.to_owned())
        } else if started.elapsed() > limit {
            Some(format!("{name} ran for more than {} minutes and was stopped: a quick check is of fewer, larger elements", limit.as_secs() / 60))
        } else {
            None
        };
        if let Some(why) = stop {
            let _ = child.kill();
            let _ = child.wait();
            return Err(why);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The last `lines` lines of `text` that say anything.
pub(crate) fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// A folder of its own for the files of one run, in the system's folder for such things.
/// It is removed with all in it when this is dropped, unless it was asked to be kept.
#[derive(Debug)]
pub struct Scratch {
    pub dir: PathBuf,
    keep: bool,
}

impl Scratch {
    pub fn new() -> Result<Scratch, String> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let dir = std::env::temp_dir().join(format!("tenon-study-{}-{n}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make the folder {}: {e}", dir.display()))?;
        Ok(Scratch { dir, keep: false })
    }

    /// A folder that is given, and left as it is afterwards.
    pub fn in_folder(dir: &Path) -> Result<Scratch, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot make the folder {}: {e}", dir.display()))?;
        Ok(Scratch { dir: dir.to_path_buf(), keep: true })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checkouts_environment_is_found_from_inside_it() {
        let scratch = Scratch::new().unwrap();
        let env = scratch.dir.join(".pixi").join("envs").join("fea");
        std::fs::create_dir_all(programs_of(&env)).unwrap();
        let deep = scratch.dir.join("crates").join("io").join("tests");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(checkout_environment(&deep), Some(env.clone()));
        assert_eq!(checkout_environment(&scratch.dir), Some(env.clone()));
        assert!(programs_of(&env).ends_with(if cfg!(windows) { "Library/bin" } else { "bin" }));
        // On Windows a program of an environment is given the environment's library folders.
        let program = programs_of(&env).join(exe("ccx"));
        let folders = library_folders(&program);
        assert_eq!(folders.is_empty(), !cfg!(windows));
        if cfg!(windows) {
            assert!(folders.contains(&env.join("Library").join("mingw-w64").join("bin")), "{folders:?}");
        }
        // The folder goes when it is dropped; one that was given stays.
        let (dir, given) = (scratch.dir.clone(), scratch.dir.join("kept"));
        let kept = Scratch::in_folder(&given).unwrap();
        drop(kept);
        assert!(given.is_dir());
        drop(scratch);
        assert!(!dir.exists());
    }

    #[test]
    fn a_program_is_run_and_what_it_says_is_kept() {
        // (This test's own program, asked for its list of tests: something that is surely there.)
        let me = std::env::current_exe().unwrap();
        let scratch = Scratch::new().unwrap();
        let cancel = Cancel::default();
        let said = run(&me, &["--list"], &scratch.dir, "out.log", &cancel, Duration::from_secs(60)).unwrap();
        assert!(said.contains("a_program_is_run_and_what_it_says_is_kept"), "{said}");
        assert!(scratch.dir.join("out.log").is_file());
        // Ending in failure is an error with what it said.
        let e = run(&me, &["--no-such-option"], &scratch.dir, "out.log", &cancel, Duration::from_secs(60)).unwrap_err();
        assert!(e.contains("failed") && e.contains("no-such-option"), "{e}");
        // Asked to stop before it starts: stopped. A program that is not there says so.
        cancel.store(true, Ordering::Relaxed);
        let e = run(&me, &["--list"], &scratch.dir, "out.log", &cancel, Duration::from_secs(60));
        assert!(e.is_ok() || e.as_ref().is_err_and(|e| e == STOPPED), "{e:?}");
        let e = run(&scratch.dir.join("nothing-here"), &[], &scratch.dir, "out.log", &Cancel::default(), Duration::from_secs(1)).unwrap_err();
        assert!(e.contains("cannot start"), "{e}");
        assert_eq!(tail("a\n\nb\nc\n \n", 2), "b\nc");
    }
}

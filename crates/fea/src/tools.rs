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
/// for from `start` upward. `start` is where the running program is, and never the working
/// folder: what lies there is whatever was downloaded or unpacked, and would be run.
fn checkout_environment(start: &Path) -> Option<PathBuf> {
    start.ancestors().take(8).map(|dir| dir.join(".pixi").join("envs").join("fea")).find(|env| env.is_dir())
}

/// Whether this program has a window, and so Tools > Application Options to choose them in.
static HAS_OPTIONS: AtomicBool = AtomicBool::new(false);

/// Says that this program has Tools > Application Options.
pub fn has_options() {
    HAS_OPTIONS.store(true, Ordering::Relaxed);
}

/// Where the user said the programs are (Tools > Application Options), if they said.
static CHOSEN: std::sync::RwLock<(Option<PathBuf>, Option<PathBuf>)> = std::sync::RwLock::new((None, None));

/// Says where the programs are, as the user chose them (None: look for it as usual). A
/// program that is not where it was chosen is looked for as usual.
pub fn choose(gmsh: Option<PathBuf>, ccx: Option<PathBuf>) {
    *CHOSEN.write().unwrap_or_else(std::sync::PoisonError::into_inner) = (gmsh, ccx);
}

/// Where each program would be taken from now, if it is found.
pub fn located() -> (Option<PathBuf>, Option<PathBuf>) {
    let chosen = CHOSEN.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    (find_one(GMSH_VAR, "gmsh", chosen.0.as_deref()), find_one(CCX_VAR, "ccx", chosen.1.as_deref()))
}

fn find_one(var: &str, name: &str, chosen: Option<&Path>) -> Option<PathBuf> {
    // Where the variable says, and nowhere else when it says.
    if let Some(said) = std::env::var_os(var).filter(|v| !v.is_empty()) {
        let said = PathBuf::from(said);
        return said.is_file().then_some(said);
    }
    // Where the user chose, if it is there.
    if let Some(chosen) = chosen.filter(|p| p.is_file()) {
        return Some(chosen.to_path_buf());
    }
    let file = exe(name);
    // A checkout's own environment, if the running program was built in one.
    if let Some(found) =
        std::env::current_exe().ok().and_then(|me| checkout_environment(&me)).map(|env| programs_of(&env).join(&file)).filter(|p| p.is_file())
    {
        return Some(found);
    }
    // The search path.
    std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path).map(|dir| dir.join(&file)).find(|p| p.is_file()))
}

impl Tools {
    /// Finds both programs: where `TENON_GMSH` and `TENON_CCX` say, where the user chose
    /// them, in a checkout's `fea` environment, or on the search path. The error says which
    /// is missing and what to do.
    pub fn find() -> Result<Tools, String> {
        let (gmsh, ccx) = located();
        match (gmsh, ccx) {
            (Some(gmsh), Some(ccx)) => Ok(Tools { gmsh, ccx }),
            (gmsh, ccx) => {
                let missing: Vec<&str> =
                    [(gmsh.is_none(), "Gmsh (gmsh), which divides the part into elements"), (ccx.is_none(), "CalculiX (ccx), which solves them")]
                        .into_iter()
                        .filter(|(missing, _)| *missing)
                        .map(|(_, what)| what)
                        .collect();
                Err(not_found(&missing, HAS_OPTIONS.load(Ordering::Relaxed)))
            }
        }
    }
}

/// What is said when programs are `missing`: what to do first, in the words of a program that
/// has Application Options or (`options` false) of one that has none.
fn not_found(missing: &[&str], options: bool) -> String {
    let what_to_do = if options {
        "Install Gmsh and CalculiX, or say where they are in Tools > Application Options."
    } else {
        "Install Gmsh and CalculiX and put them on the search path, or say where they are with TENON_GMSH and TENON_CCX."
    };
    format!(
        "{what_to_do} A study takes these two programs, which are not part of Tenon, and {} not found: {}. How to get them: https://github.com/ElRashoMacuin24/tenon/blob/main/docs/stress.md",
        if missing.len() == 1 { "one was" } else { "they were" },
        missing.join("; ")
    )
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

/// Why a program that ran longer than `limit` was stopped, with `slow`: what to try so that
/// it takes less.
fn too_long(name: &str, limit: Duration, slow: &str) -> String {
    format!("{name} ran for more than {} minutes and was stopped: {slow}", limit.as_secs() / 60)
}

/// Whether a program's failure (`e`, with whatever was said before it) was its running past its
/// limit: running it again, on the same limit, is only more of the same wait.
pub fn ran_too_long(e: &str) -> bool {
    e.contains(" ran for more than ") && e.contains(" minutes and was stopped: ")
}

/// Starts `program` in `dir` and waits for it, with all it says kept in the file `log` there.
/// Returns what it said. It is stopped, and an error returned, when `cancel` is set or it
/// runs longer than `limit` (the error then says `slow`: what to try so that it takes less);
/// an error too when it ends saying it failed.
pub fn run(program: &Path, args: &[&str], dir: &Path, log: &str, cancel: &Cancel, limit: Duration, slow: &str) -> Result<String, String> {
    // One at a time: each takes every thread the machine has, and several at once (the tests,
    // which run side by side) get in each other's way and take many times as long.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
            Some(too_long(&name, limit, slow))
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

/// What a folder of a run is called, before what tells it from another of the same program.
const SCRATCH_PREFIX: &str = "tenon-study-";

/// The program a folder of a run was made by, from its name.
fn scratch_owner(name: &str) -> Option<u32> {
    name.strip_prefix(SCRATCH_PREFIX)?.split('-').next()?.parse().ok()
}

/// Whether a program of that number is running. (Asked of the system: that is all this needs, and
/// it is asked once a folder has been found left behind. In doubt, it is taken to be.)
fn program_runs(id: u32) -> bool {
    if cfg!(windows) {
        let filter = format!("PID eq {id}");
        let mut command = Command::new("tasklist");
        command.args(["/FI", &filter, "/FO", "CSV", "/NH"]);
        #[cfg(windows)]
        {
            // No console window flashing up beside the app.
            use std::os::windows::process::CommandExt as _;
            command.creation_flags(0x0800_0000);
        }
        let asked = command.output();
        asked.map_or(true, |out| String::from_utf8_lossy(&out.stdout).contains(&format!("\"{id}\"")))
    } else {
        Command::new("ps").args(["-p", &id.to_string()]).output().map_or(true, |out| out.status.success())
    }
}

/// Takes away the folders of runs whose programs have gone: a crash, or the program being ended
/// from outside, leaves them for good.
fn clear_left_behind(base: &Path) {
    let Ok(entries) = std::fs::read_dir(base) else { return };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        if let Some(owner) = name.to_str().and_then(scratch_owner)
            && owner != std::process::id()
            && !program_runs(owner)
        {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

impl Scratch {
    pub fn new() -> Result<Scratch, String> {
        // (At the first run of this program.)
        static CLEARED: std::sync::Once = std::sync::Once::new();
        CLEARED.call_once(|| clear_left_behind(&std::env::temp_dir()));
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let dir = std::env::temp_dir().join(format!("{SCRATCH_PREFIX}{}-{n}-{stamp}", std::process::id()));
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
    fn what_a_program_that_has_gone_left_behind_is_taken_away() {
        let base = std::env::temp_dir().join(format!("tenon-left-behind-{}", std::process::id()));
        let (gone, here, other) =
            (base.join("tenon-study-4294967294-0-1"), base.join(format!("tenon-study-{}-0-1", std::process::id())), base.join("kept"));
        for dir in [&gone, &here, &other] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(gone.join("part.step"), b"left").unwrap();
        clear_left_behind(&base);
        assert!(!gone.exists(), "no program has that number");
        assert!(here.exists() && other.exists(), "a running program's stays, and what is no run's");
        assert_eq!(scratch_owner("tenon-study-44980-1-786432500"), Some(44980));
        assert_eq!(scratch_owner("tenon-study-x-1-2"), None);
        assert!(program_runs(std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn programs_not_found_are_told_in_the_words_of_the_program_that_says_so() {
        let window = not_found(&["CalculiX (ccx), which solves them"], true);
        assert!(window.starts_with("Install Gmsh and CalculiX, or say where they are in Tools > Application Options."), "{window}");
        assert!(window.contains("one was not found: CalculiX (ccx)") && window.contains("https://github.com/"), "{window}");
        let cli = not_found(&["Gmsh", "CalculiX"], false);
        assert!(cli.contains("TENON_GMSH and TENON_CCX") && cli.contains("they were not found"), "{cli}");
        assert!(!cli.contains("Application Options"), "{cli}");
    }

    #[test]
    fn a_program_is_run_and_what_it_says_is_kept() {
        // (This test's own program, asked for its list of tests: something that is surely there.)
        let me = std::env::current_exe().unwrap();
        let scratch = Scratch::new().unwrap();
        let cancel = Cancel::default();
        let said = run(&me, &["--list"], &scratch.dir, "out.log", &cancel, Duration::from_secs(60), "").unwrap();
        assert!(said.contains("a_program_is_run_and_what_it_says_is_kept"), "{said}");
        assert!(scratch.dir.join("out.log").is_file());
        // Ending in failure is an error with what it said.
        let e = run(&me, &["--no-such-option"], &scratch.dir, "out.log", &cancel, Duration::from_secs(60), "").unwrap_err();
        assert!(e.contains("failed") && e.contains("no-such-option"), "{e}");
        // Asked to stop before it starts: stopped. A program that is not there says so.
        cancel.store(true, Ordering::Relaxed);
        let e = run(&me, &["--list"], &scratch.dir, "out.log", &cancel, Duration::from_secs(60), "");
        assert!(e.is_ok() || e.as_ref().is_err_and(|e| e == STOPPED), "{e:?}");
        let e = run(&scratch.dir.join("nothing-here"), &[], &scratch.dir, "out.log", &Cancel::default(), Duration::from_secs(1), "").unwrap_err();
        assert!(e.contains("cannot start"), "{e}");
        assert_eq!(tail("a\n\nb\nc\n \n", 2), "b\nc");
    }

    #[test]
    fn ran_too_long_says_what_to_try() {
        assert_eq!(
            too_long("ccx", Duration::from_secs(30 * 60), "a shape is found sooner at Coarse"),
            "ccx ran for more than 30 minutes and was stopped: a shape is found sooner at Coarse"
        );
        assert_eq!(
            too_long("gmsh", Duration::from_secs(10 * 60 + 59), "a quick check is of fewer, larger elements"),
            "gmsh ran for more than 10 minutes and was stopped: a quick check is of fewer, larger elements"
        );
        assert!(ran_too_long(&format!("The part could not be divided. {}", too_long("gmsh", Duration::from_secs(600), "try less"))));
        assert!(!ran_too_long("gmsh failed (exit code: 1). It said: Invalid boundary mesh"));
        assert!(!ran_too_long(STOPPED));
    }
}

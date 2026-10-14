//! When Tenon fails: what failed is written to a report in the user's data folder, and the
//! next start says so (DEC-101).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where faults are reported.
pub const ISSUES: &str = "https://github.com/ElRashoMacuin24/tenon/issues";

/// How many reports are kept: the oldest go.
const KEPT: usize = 20;

/// Writes a report of every fault (panic) of Tenon's into `dir`, then does what was done
/// before (the message on the terminal).
pub fn install(dir: PathBuf) {
    let usual = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let said = payload.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_default();
        let at = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
        let thread = std::thread::current().name().unwrap_or("unnamed").to_owned();
        let trace = std::backtrace::Backtrace::force_capture().to_string();
        let _ = write(&dir, &report(&said, &at, &thread, &trace, now()));
        usual(info);
    }));
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The report of a fault: what, where in Tenon's code, when, on what.
pub(crate) fn report(said: &str, at: &str, thread: &str, trace: &str, time: u64) -> String {
    format!(
        concat!(
            "Tenon {version} met a fault in itself.\n\n",
            "When:   {when} (UTC)\n",
            "On:     {os} {arch}\n",
            "Thread: {thread}\n",
            "What:   {said}\n",
            "Where:  {at}\n\n",
            "Please report it at {issues}, with this file. It says what failed and where in\n",
            "Tenon's own code; read it before you send it.\n\n",
            "Backtrace:\n{trace}\n"
        ),
        version = env!("CARGO_PKG_VERSION"),
        thread = thread,
        said = said,
        at = at,
        trace = trace,
        when = date(time),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        issues = ISSUES,
    )
}

/// A time as a date and time of day (UTC).
pub(crate) fn date(secs: u64) -> String {
    // (Days to a civil date: H. Hinnant's algorithm.)
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rest = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}", rest / 3600, rest % 3600 / 60, rest % 60)
}

/// Writes a report into `dir`; returns where. The oldest reports beyond twenty go.
pub(crate) fn write(dir: &Path, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = now();
    let mut file = dir.join(format!("crash-{stamp}.txt"));
    let mut n = 1;
    while file.exists() {
        n += 1;
        file = dir.join(format!("crash-{stamp}-{n}.txt"));
    }
    std::fs::write(&file, text)?;
    let mut all = reports(dir);
    all.sort();
    let extra = all.len().saturating_sub(KEPT);
    for old in all.into_iter().take(extra) {
        let _ = std::fs::remove_file(old);
    }
    Ok(file)
}

fn reports(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt")))
        .collect()
}

/// The newest report not yet told of, if there is one; every report not yet told of is told
/// of now (it is renamed `...-seen.txt`, and the new name is returned).
pub(crate) fn unread(dir: &Path) -> Option<PathBuf> {
    let mut new: Vec<PathBuf> = reports(dir).into_iter().filter(|p| !p.to_string_lossy().ends_with("-seen.txt")).collect();
    new.sort();
    let mut newest = None;
    for p in new {
        let seen = p.with_file_name(p.file_name()?.to_string_lossy().replace(".txt", "-seen.txt"));
        newest = Some(if std::fs::rename(&p, &seen).is_ok() { seen } else { p });
    }
    newest
}

impl crate::Workbench {
    /// Says, once, that Tenon met a fault last time, if a report of one is in `dir`.
    pub fn enable_crash_reports(&mut self, dir: &Path) {
        self.chrome.crash = unread(dir);
    }

    /// The notice of last time's fault.
    pub(crate) fn crash_window(&mut self, ui: &egui::Ui) {
        let Some(file) = self.chrome.crash.clone() else { return };
        let mut open = true;
        let mut close = false;
        egui::Window::new("Tenon met a fault").open(&mut open).collapsible(false).resizable(false).default_width(460.0).show(ui.ctx(), |ui| {
            ui.label("The last time it ran, Tenon met a fault in itself. What failed was written to:");
            ui.add(egui::Label::new(egui::RichText::new(file.display().to_string()).monospace()).selectable(true));
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label("Please report it, with that file, at");
                ui.hyperlink_to("Tenon's issues", ISSUES);
            });
            ui.label("Work that was not saved is offered back when Tenon had kept a copy of it.");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Copy Path").clicked() {
                    ui.ctx().copy_text(file.display().to_string());
                }
                if crate::drawing::button(ui, "Close", "tn_crash_close").clicked() {
                    close = true;
                }
            });
        });
        if !open || close {
            self.chrome.crash = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_written_as_a_date() {
        assert_eq!(date(0), "1970-01-01 00:00:00");
        assert_eq!(date(1_760_000_000), "2025-10-09 08:53:20");
        assert_eq!(date(951_782_400), "2000-02-29 00:00:00");
    }

    #[test]
    fn a_fault_is_reported_and_told_of_once() {
        let dir = std::env::temp_dir().join(format!("tenon-crash-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(unread(&dir), None, "no folder, nothing to tell");
        let text = report("index out of bounds", "crates/model/src/x.rs:1:2", "tenon-regen", "0: frame", 0);
        assert!(text.starts_with(&format!("Tenon {} met a fault in itself.", env!("CARGO_PKG_VERSION"))), "{text}");
        assert!(text.contains("What:   index out of bounds\nWhere:  crates/model/src/x.rs:1:2"), "{text}");
        let written = write(&dir, &text).unwrap();
        assert_eq!(std::fs::read_to_string(&written).unwrap(), text);
        // Told of once: the report is still there, under its new name.
        let told = unread(&dir).expect("a report");
        assert!(told.to_string_lossy().ends_with("-seen.txt") && told.is_file(), "{told:?}");
        assert_eq!(unread(&dir), None);
        // No more than twenty are kept.
        for _ in 0..25 {
            write(&dir, "x").unwrap();
        }
        assert_eq!(reports(&dir).len(), KEPT);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_notice_of_a_fault_is_shown_and_closed() {
        let dir = std::env::temp_dir().join(format!("tenon-crash-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write(&dir, &report("boom", "here", "main", "", 0)).unwrap();
        let mut wb = crate::Workbench::headless(Box::new(tenon_kernel_occt::OcctKernel::new()));
        wb.enable_crash_reports(&dir);
        let mut d = crate::tests::Driver::new(egui::vec2(1400.0, 860.0));
        d.frame(&mut wb, vec![]);
        d.frame(&mut wb, vec![]);
        assert!(d.texts().iter().any(|t| t.starts_with("The last time it ran, Tenon met a fault")), "{:?}", d.texts());
        let close = crate::tests::pressable(&d, "tn_crash_close");
        d.click(&mut wb, close);
        d.frame(&mut wb, vec![]);
        assert!(!d.texts().iter().any(|t| t.starts_with("The last time it ran")), "closed");
        // Started again: nothing to tell.
        wb.enable_crash_reports(&dir);
        assert!(wb.chrome.crash.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

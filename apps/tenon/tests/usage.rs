//! `tenon --help` and `--version` answer without opening a window.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;
use std::time::{Duration, Instant};

/// Runs the program with one argument: what it printed, once it has ended of its own accord.
fn answers(arg: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tenon")).arg(arg).stdout(std::process::Stdio::piped()).spawn().unwrap();
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            panic!("tenon {arg} opened a window and did not end");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", out.status);
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn help_says_how_to_use_it_and_ends() {
    for arg in ["--help", "-h"] {
        let said = answers(arg);
        assert!(said.contains("tenon [FILE]") && said.contains("--screenshot"), "{said}");
    }
    assert!(answers("--version").starts_with("tenon "));
}

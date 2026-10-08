//! `cargo xtask unsafe-audit`: `unsafe` is allowed only in kernel backend (FFI) crates.
//!
//! For every other workspace package:
//! - every library and binary crate root must contain `#![forbid(unsafe_code)]`, and
//! - no `.rs` file in the package may use the `unsafe` keyword (comments and string literals are
//!   ignored).
//!
//! The workspace lint `unsafe_code = "forbid"` already stops compilation; this check makes the
//! rule visible per crate and catches crates that opt out of the workspace lints.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Packages allowed to contain `unsafe` (the cxx bridge).
pub const ALLOWED: &[&str] = &["tenon-kernel-occt"];

const FORBID: &str = "#![forbid(unsafe_code)]";

/// Source with comments and string/char literals blanked out, so keyword search sees code only.
pub fn code_only(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
    while i < chars.len() {
        let c = chars[i];
        // line comment
        if c == '/' && at(i + 1) == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        // block comment (nested)
        if c == '/' && at(i + 1) == '*' {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && at(i + 1) == '*' {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            out.push(' ');
            continue;
        }
        // raw string: r"..", r#".."#, br"..", and so on
        let raw_start = if c == 'r' {
            Some(i + 1)
        } else if c == 'b' && at(i + 1) == 'r' {
            Some(i + 2)
        } else {
            None
        };
        let prev_ident = i > 0 && is_ident(chars[i - 1]);
        if let Some(mut j) = raw_start.filter(|_| !prev_ident) {
            let mut hashes = 0;
            while at(j) == '#' {
                hashes += 1;
                j += 1;
            }
            if at(j) == '"' {
                j += 1;
                loop {
                    if j >= chars.len() {
                        break;
                    }
                    if chars[j] == '"' && (1..=hashes).all(|h| at(j + h) == '#') {
                        j += 1 + hashes;
                        break;
                    }
                    j += 1;
                }
                i = j;
                out.push_str("\"\"");
                continue;
            }
        }
        // string literal (also b"..")
        if c == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                i += if chars[i] == '\\' { 2 } else { 1 };
            }
            i += 1;
            out.push_str("\"\"");
            continue;
        }
        // char literal vs lifetime
        if c == '\'' {
            if at(i + 1) == '\\' {
                // skip the quote, the backslash and the escaped char, then up to the closing quote
                i += 3;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
                i += 1;
                out.push_str("' '");
                continue;
            }
            if at(i + 2) == '\'' {
                i += 3;
                out.push_str("' '");
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// 1-based line numbers where the `unsafe` keyword appears in code.
pub fn unsafe_lines(src: &str) -> Vec<usize> {
    let code = code_only(src);
    let chars: Vec<char> = code.chars().collect();
    let word: Vec<char> = "unsafe".chars().collect();
    let mut lines = Vec::new();
    let mut line = 1;
    for i in 0..chars.len() {
        if chars[i] == '\n' {
            line += 1;
        }
        if chars[i..].starts_with(&word) {
            let before = i.checked_sub(1).and_then(|p| chars.get(p)).is_some_and(|c| is_ident(*c));
            let after = chars.get(i + word.len()).is_some_and(|c| is_ident(*c));
            if !before && !after && lines.last() != Some(&line) {
                lines.push(line);
            }
        }
    }
    lines
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        if p.is_dir() {
            if name != "target" && !name.starts_with('.') {
                rust_files(&p, out);
            }
        } else if name.ends_with(".rs") {
            out.push(p);
        }
    }
}

pub fn run(meta: &Value) -> Result<(), String> {
    let mut problems = Vec::new();
    let mut checked = 0;
    for p in meta["packages"].as_array().into_iter().flatten() {
        let name = p["name"].as_str().unwrap_or("?");
        if ALLOWED.contains(&name) {
            println!("{name:<28} allowed (FFI backend)");
            continue;
        }
        checked += 1;
        for t in p["targets"].as_array().into_iter().flatten() {
            let kinds: Vec<&str> = t["kind"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            if !kinds.iter().any(|k| matches!(*k, "lib" | "rlib" | "bin" | "cdylib" | "staticlib" | "proc-macro")) {
                continue;
            }
            let Some(root) = t["src_path"].as_str() else {
                continue;
            };
            let src = std::fs::read_to_string(root).map_err(|e| format!("{root}: {e}"))?;
            if !src.contains(FORBID) {
                problems.push(format!("{name}: crate root {root} lacks {FORBID}"));
            }
        }
        let Some(dir) = p["manifest_path"].as_str().and_then(|m| Path::new(m).parent()) else {
            continue;
        };
        let mut files = Vec::new();
        rust_files(dir, &mut files);
        for f in files {
            let src = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            for line in unsafe_lines(&src) {
                problems.push(format!("{name}: `unsafe` at {}:{line}", f.display()));
            }
        }
        println!("{name:<28} ok");
    }
    if problems.is_empty() {
        println!("\nOK: {checked} crates free of unsafe; FFI allowed only in {}.", ALLOWED.join(", "));
        Ok(())
    } else {
        for p in &problems {
            println!("  - {p}");
        }
        Err(format!("{} unsafe-audit problem(s)", problems.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_real_unsafe_only() {
        let src = r##"
#![forbid(unsafe_code)]
// unsafe in a comment
/* block unsafe /* nested unsafe */ still comment */
fn a<'a>(x: &'a str) -> char { let _s = "unsafe { }"; let _r = r#"unsafe "quoted""#; let _c = '"'; 'u' }
fn b() { unsafe { core::hint::unreachable_unchecked() } }
let not_unsafe_fn = my_unsafe_helper;
unsafe impl Send for X {}
"##;
        assert_eq!(unsafe_lines(src), vec![6, 8]);
    }

    #[test]
    fn escapes_and_byte_strings() {
        assert!(unsafe_lines(r#"let s = "a \" unsafe \\"; let b = b"unsafe"; let c = '\''; let d = br"unsafe";"#).is_empty());
        assert_eq!(unsafe_lines("let s = \"x\";\nunsafe fn f() {}\n"), vec![2]);
    }

    #[test]
    fn identifiers_containing_the_word_are_fine() {
        assert!(unsafe_lines("#![forbid(unsafe_code)]\nlet unsafe_ok = 1; let not_unsafe = 2;").is_empty());
    }
}

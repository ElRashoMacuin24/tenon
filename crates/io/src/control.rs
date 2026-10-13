//! Where a running Tenon says how to reach its control port, for `tenon-cli mcp --attach` to
//! find without anything being copied by hand.
//!
//! The file holds the port and the token every request must carry, so it is as private as the
//! user's other settings: in their own settings folder, and on Unix readable by them alone.

use std::path::PathBuf;

use serde_json::{Value, json};

/// The file a running Tenon writes when its control port is open.
///
/// - Windows: `%APPDATA%\Tenon\control.json`
/// - macOS: `~/Library/Application Support/Tenon/control.json`
/// - elsewhere: `$XDG_RUNTIME_DIR/tenon/control.json`, or under `$XDG_CONFIG_HOME` (or
///   `~/.config`) when there is no runtime folder.
///
/// `TENON_CONTROL_FILE` names another (tests use it, and so can whoever runs two at once).
pub fn file() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(named) = var("TENON_CONTROL_FILE") {
        return Some(named);
    }
    if cfg!(target_os = "windows") {
        var("APPDATA").map(|d| d.join("Tenon").join("control.json"))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|d| d.join("Library").join("Application Support").join("Tenon").join("control.json"))
    } else {
        var("XDG_RUNTIME_DIR")
            .or_else(|| var("XDG_CONFIG_HOME"))
            .or_else(|| var("HOME").map(|d| d.join(".config")))
            .map(|d| d.join("tenon").join("control.json"))
    }
}

/// Writes the file: the port, the token, and which process it is.
pub fn announce(port: u16, token: &str) -> Result<PathBuf, String> {
    let path = file().ok_or("there is no settings folder to say where the control port is")?;
    announce_at(&path, port, token)?;
    Ok(path)
}

/// [`announce`], in the file `path`.
pub fn announce_at(path: &std::path::Path, port: u16, token: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&json!({ "port": port, "token": token, "pid": std::process::id() })).unwrap_or_default();
    write_private(path, &text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    // (Made anew, so that the mode is ours and not that of a file left there.)
    let _ = std::fs::remove_file(path);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    f.write_all(text.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    std::fs::write(path, text)
}

/// The port and token a running Tenon announced.
pub fn announced() -> Result<(u16, String), String> {
    let path = file().ok_or("there is no settings folder to look in for a running Tenon's control port")?;
    announced_at(&path)
}

/// [`announced`], from the file `path`.
pub fn announced_at(path: &std::path::Path) -> Result<(u16, String), String> {
    let text = std::fs::read_to_string(path).map_err(|_| {
        format!("no running Tenon has its control port open (start it with `tenon --control`; it would say so in {})", path.display())
    })?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{} is not what Tenon writes: {e}", path.display()))?;
    match (v["port"].as_u64().and_then(|p| u16::try_from(p).ok()), v["token"].as_str()) {
        (Some(port), Some(token)) => Ok((port, token.to_owned())),
        _ => Err(format!("{} is not what Tenon writes: it has no port and token", path.display())),
    }
}

/// Takes the file away, if it is still the one for `port` (a Tenon started later has written
/// its own, which stays).
pub fn withdraw(port: u16) {
    if let Some(path) = file() {
        withdraw_at(&path, port);
    }
}

/// [`withdraw`], of the file `path`.
pub fn withdraw_at(path: &std::path::Path, port: u16) {
    if announced_at(path).is_ok_and(|(announced, _)| announced == port) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_control_port_is_announced_found_and_withdrawn() {
        let dir = std::env::temp_dir().join(format!("tenon-io-control-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("control.json");
        // Nothing announced: said so, with how to have one.
        assert!(announced_at(&path).unwrap_err().contains("tenon --control"));
        announce_at(&path, 4711, "0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(announced_at(&path).unwrap(), (4711, "0123456789abcdef0123456789abcdef".to_owned()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600, "the token is the user's alone");
        }
        // A Tenon started later announces itself in the same place; the earlier one, closing,
        // leaves that alone.
        announce_at(&path, 4712, "ffffffffffffffffffffffffffffffff").unwrap();
        withdraw_at(&path, 4711);
        assert_eq!(announced_at(&path).unwrap().0, 4712);
        withdraw_at(&path, 4712);
        assert!(!path.exists());
        // Something else in its place is not taken for it.
        std::fs::write(&path, "{\"port\": \"x\"}").unwrap();
        assert!(announced_at(&path).unwrap_err().contains("no port and token"));
        std::fs::write(&path, "not json").unwrap();
        assert!(announced_at(&path).unwrap_err().contains("is not what Tenon writes"));
        // Where it is kept when nothing says otherwise: in the user's own settings.
        if std::env::var_os("TENON_CONTROL_FILE").is_none() {
            assert!(file().is_none_or(|f| f.ends_with("control.json")));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

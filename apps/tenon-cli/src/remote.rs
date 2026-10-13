//! A running Tenon as what the MCP server drives: commands go to the app's control port
//! (`tenon --control`), are run on the document open in its window, and the person there sees
//! each one as it is done and can undo it. The wire is in `tenon_ui`'s `control` module: one
//! JSON object a line each way.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpStream};

use serde_json::{Value, json};

use crate::mcp::Host;

/// A connection to a running Tenon's control port.
pub struct Remote {
    reader: BufReader<TcpStream>,
    out: TcpStream,
    token: String,
    next: u64,
}

impl Remote {
    /// Connects to the control port on this machine.
    pub fn connect(port: u16, token: impl Into<String>) -> Result<Remote, String> {
        let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .map_err(|e| format!("cannot reach Tenon's control port on 127.0.0.1:{port}: {e}. Is Tenon running, started with --control?"))?;
        let _ = stream.set_nodelay(true);
        let out = stream.try_clone().map_err(|e| e.to_string())?;
        let mut remote = Remote { reader: BufReader::new(stream), out, token: token.into(), next: 0 };
        // The token is tried at once: a wrong one is said now, not at the first command.
        let state = remote.run("control.state", &json!({}))?;
        let theirs = state["schema_version"].as_u64();
        if theirs != Some(u64::from(tenon_model::schema::VERSION)) {
            eprintln!(
                "warning: the running Tenon's commands are version {}, this tenon-cli's are version {}: what one says a command takes the other may not take",
                theirs.map_or_else(|| "?".to_owned(), |v| v.to_string()),
                tenon_model::schema::VERSION
            );
        }
        Ok(remote)
    }

    /// Connects to the Tenon that announced its control port (`tenon --control`).
    pub fn attach() -> Result<Remote, String> {
        let (port, token) = tenon_io::control::announced()?;
        Remote::connect(port, token)
    }

    /// Runs a command in the app and waits for its answer.
    pub fn run(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        self.next += 1;
        let request = json!({ "id": self.next, "token": self.token, "run": id, "with": params });
        writeln!(self.out, "{request}").and_then(|()| self.out.flush()).map_err(|e| format!("Tenon's control port has gone: {e}"))?;
        let mut line = String::new();
        loop {
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) => return Err("Tenon closed its control port (was the app closed?)".into()),
                Err(e) => return Err(format!("Tenon's control port has gone: {e}")),
                Ok(_) => {}
            }
            let Ok(answer) = serde_json::from_str::<Value>(&line) else { continue };
            // (An answer to an earlier request that was given up on is passed over.)
            if answer["id"].as_u64().is_some_and(|i| i != self.next) {
                continue;
            }
            return if answer["ok"] == true {
                Ok(answer["result"].clone())
            } else {
                Err(answer["error"].as_str().unwrap_or("the app refused, and did not say why").to_owned())
            };
        }
    }
}

impl Host for Remote {
    fn exec(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        self.run(id, params)
    }

    fn revision(&mut self) -> u64 {
        self.run("control.state", &json!({})).ok().and_then(|s| s["revision"].as_u64()).unwrap_or(0)
    }
}

//! `pillkit-host show | clear | remove | status`: talk to the running host
//! from a terminal or a shell script, the same way the SDKs do (open mode:
//! local only, no server).
//!
//!   pillkit-host show --title "Build passed" [--subtitle "main · 2m ago"]
//!                     [--app my-script] [--id build] [--label Sponsored]
//!                     [--bg "#1F7A4D"] [--fg "#FFFFFF"] [--url https://...]
//!                     [--icon logo.png] [--seconds 20]
//!   pillkit-host clear  [--app my-script]   remove this app's items
//!   pillkit-host remove [--app my-script]   forget the app entirely
//!   pillkit-host status [--app my-script]

use std::io::{BufRead, BufReader, Read, Write};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};

struct Conn {
    reader: BufReader<Box<dyn Read>>,
    writer: Box<dyn Write>,
}

impl Conn {
    fn request(&mut self, body: Value) -> Result<Value, String> {
        writeln!(self.writer, "{body}").map_err(|e| e.to_string())?;
        self.writer.flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        loop {
            line.clear();
            if self
                .reader
                .read_line(&mut line)
                .map_err(|e| e.to_string())?
                == 0
            {
                return Err("the host closed the connection".into());
            }
            let v: Value = serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
            if v.get("event").is_none() {
                return Ok(v);
            }
        }
    }
}

fn try_connect() -> std::io::Result<Conn> {
    let endpoint = crate::paths::ipc_endpoint();
    #[cfg(unix)]
    {
        let s = std::os::unix::net::UnixStream::connect(&endpoint)?;
        let r = s.try_clone()?;
        Ok(Conn {
            reader: BufReader::new(Box::new(r)),
            writer: Box::new(s),
        })
    }
    #[cfg(windows)]
    {
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&endpoint)?;
        let r = f.try_clone()?;
        Ok(Conn {
            reader: BufReader::new(Box::new(r)),
            writer: Box::new(f),
        })
    }
}

/// Connects to the host, starting it in the background if it isn't running.
fn connect() -> Result<Conn, String> {
    if let Ok(c) = try_connect() {
        return Ok(c);
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("run")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    }
    cmd.spawn()
        .map_err(|e| format!("could not start the host: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        std::thread::sleep(Duration::from_millis(150));
        if let Ok(c) = try_connect() {
            return Ok(c);
        }
        if Instant::now() > deadline {
            return Err("the host did not start; see host.log in the Pill Kit data folder".into());
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn check(v: Value) -> Result<Value, String> {
    if v.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(v)
    } else {
        Err(v
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("request failed")
            .to_string())
    }
}

pub fn main(args: &[String]) -> Result<(), String> {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let rest = &args[1..];
    let app = flag(rest, "--app").unwrap_or_else(|| "cli".into());
    let url = flag(rest, "--url").unwrap_or_default();
    // A custom app link (myapp://...) must be declared to be allowed.
    let schemes: Vec<String> = url
        .split_once("://")
        .map(|(s, _)| s.to_ascii_lowercase())
        .filter(|s| s != "https" && s != "http")
        .into_iter()
        .collect();

    let mut conn = connect()?;
    if cmd == "status" && flag(rest, "--app").is_none() {
        let v = check(conn.request(json!({"op": "hello"}))?)?;
        println!(
            "Pill Kit host {} is running",
            v["version"].as_str().unwrap_or("?")
        );
        return Ok(());
    }
    check(conn.request(json!({"op": "register", "app_id": app, "link_schemes": schemes}))?)?;

    match cmd {
        "show" => {
            let title = flag(rest, "--title").ok_or("show needs --title")?;
            let mut item = json!({
                "id": flag(rest, "--id").unwrap_or_else(|| "main".into()),
                "title": title,
                "subtitle": flag(rest, "--subtitle").unwrap_or_default(),
                "label": flag(rest, "--label").unwrap_or_default(),
                "theme": {
                    "background": flag(rest, "--bg").unwrap_or_else(|| "#E8F04A".into()),
                    "foreground": flag(rest, "--fg").unwrap_or_else(|| "#2A2C05".into()),
                },
                "duration_secs": flag(rest, "--seconds").and_then(|s| s.parse::<u32>().ok()).unwrap_or(20),
            });
            if !url.is_empty() {
                let kind = if url.to_ascii_lowercase().starts_with("https://") {
                    "open_url"
                } else {
                    "deep_link"
                };
                item["action"] = json!({"type": kind, "url": url});
            }
            if let Some(path) = flag(rest, "--icon") {
                let bytes = std::fs::read(&path).map_err(|e| format!("can't read {path}: {e}"))?;
                item["icon"] = json!({"type": "png", "data": base64::engine::general_purpose::STANDARD.encode(bytes)});
            }
            check(conn.request(json!({"op": "show", "item": item}))?)?;
            println!(
                "Showing \"{}\" for {app}.",
                flag(rest, "--title").unwrap_or_default()
            );
        }
        "clear" => {
            check(conn.request(json!({"op": "clear"}))?)?;
            println!("Cleared {app}'s items.");
        }
        "remove" => {
            check(conn.request(json!({"op": "unregister"}))?)?;
            println!("Removed {app}.");
        }
        "status" => {
            let v = check(conn.request(json!({"op": "status"}))?)?;
            println!(
                "{app}: {} item(s) showing",
                v["local_items"].as_u64().unwrap_or(0)
            );
        }
        _ => return Err(format!("unknown command {cmd}")),
    }
    Ok(())
}

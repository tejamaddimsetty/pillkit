//! Where the host keeps its files, and a tiny append-only log.

use std::io::Write;
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    let dir = if cfg!(target_os = "macos") {
        home().join("Library/Application Support/pillkit")
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(home)
            .join("pillkit")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"))
            .join("pillkit")
    };
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| ".".into())
}

/// Unix socket path, or the Windows named pipe name.
pub fn ipc_endpoint() -> String {
    if cfg!(windows) {
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".into());
        let user: String = user
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        format!(r"\\.\pipe\pillkit-host-{user}")
    } else {
        data_dir().join("host.sock").to_string_lossy().into_owned()
    }
}

pub fn log(msg: impl AsRef<str>) {
    let line = format!("{} {}\n", now(), msg.as_ref());
    eprint!("{line}");
    let path = data_dir().join("host.log");
    // Keep the log from growing forever: start over past 1 MB.
    if std::fs::metadata(&path)
        .map(|m| m.len() > 1_000_000)
        .unwrap_or(false)
    {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

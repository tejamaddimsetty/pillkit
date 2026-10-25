//! Local socket the SDKs talk to: a Unix socket on macOS/Linux, a named pipe on
//! Windows. Newline-delimited JSON both ways (docs/PROTOCOL.md §5).

use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::controller::Msg;
use crate::paths::log;

/// Writes go through this so the controller can push events to a client.
pub type Outbox = Arc<Mutex<Box<dyn Write + Send>>>;

pub fn send(out: &Outbox, v: &Value) {
    if let Ok(mut w) = out.lock() {
        let _ = writeln!(w, "{v}");
        let _ = w.flush();
    }
}

fn serve_client<R: std::io::Read>(conn_id: u64, reader: R, out: Outbox, tx: Sender<Msg>) {
    let _ = tx.send(Msg::Connected {
        conn_id,
        out: out.clone(),
    });
    for line in BufReader::new(reader).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > 512 * 1024 {
            send(
                &out,
                &serde_json::json!({"ok": false, "error": "message_too_large"}),
            );
            continue;
        }
        match serde_json::from_str::<Value>(&line) {
            Ok(v) => {
                let _ = tx.send(Msg::Request { conn_id, body: v });
            }
            Err(_) => send(&out, &serde_json::json!({"ok": false, "error": "bad_json"})),
        }
    }
    let _ = tx.send(Msg::Disconnected { conn_id });
}

#[cfg(unix)]
pub fn listen(tx: Sender<Msg>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};

    let path = crate::paths::ipc_endpoint();
    // A live host already owns the socket: refuse to start a second one.
    if UnixStream::connect(&path).is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            "another barkit-host is running",
        ));
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    log(format!("listening on {path}"));
    std::thread::spawn(move || {
        for (n, stream) in listener.incoming().enumerate() {
            let Ok(stream) = stream else { continue };
            let Ok(writer) = stream.try_clone() else {
                continue;
            };
            let tx = tx.clone();
            let out: Outbox = Arc::new(Mutex::new(Box::new(writer)));
            std::thread::spawn(move || serve_client(n as u64 + 1, stream, out, tx));
        }
    });
    Ok(())
}

#[cfg(windows)]
pub fn listen(tx: Sender<Msg>) -> std::io::Result<()> {
    use std::fs::File;
    use std::os::windows::io::FromRawHandle;
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
    };
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    let name = HSTRING::from(crate::paths::ipc_endpoint());
    let create = move |first: bool| unsafe {
        let mut flags: FILE_FLAGS_AND_ATTRIBUTES = PIPE_ACCESS_DUPLEX;
        if first {
            // Fails if anyone else already created this pipe: stops pipe squatting
            // and doubles as the "is another host running?" check.
            flags |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let h = CreateNamedPipeW(
            &name,
            flags,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            64 * 1024,
            64 * 1024,
            0,
            None,
        );
        if h.is_invalid() {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(h)
        }
    };

    let first = create(true).map_err(|e| std::io::Error::new(std::io::ErrorKind::AddrInUse, e))?;
    log(format!("listening on {}", crate::paths::ipc_endpoint()));
    // HANDLE wraps a raw pointer, which isn't Send; pass the value across.
    let first = first.0 as isize;
    std::thread::spawn(move || {
        let mut next = Some(windows::Win32::Foundation::HANDLE(first as _));
        let mut n = 0u64;
        loop {
            let h = match next.take() {
                Some(h) => h,
                None => match create(false) {
                    Ok(h) => h,
                    Err(e) => {
                        log(format!("pipe create failed: {e}"));
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        continue;
                    }
                },
            };
            // ERROR_PIPE_CONNECTED (client raced us) is still a connection.
            let connected = unsafe { ConnectNamedPipe(h, None) };
            if let Err(e) = connected {
                if e.code().0 as u32 & 0xFFFF != 535 {
                    continue;
                }
            }
            n += 1;
            let file = unsafe { File::from_raw_handle(h.0 as _) };
            let Ok(writer) = file.try_clone() else {
                continue;
            };
            let tx = tx.clone();
            let out: Outbox = Arc::new(Mutex::new(Box::new(writer)));
            let id = n;
            std::thread::spawn(move || serve_client(id, file, out, tx));
        }
    });
    Ok(())
}

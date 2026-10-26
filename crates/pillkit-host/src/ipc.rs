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
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                log(format!("connection {conn_id}: read failed, closing: {e}"));
                break;
            }
        };
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
            "another pillkit-host is running",
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

/// One connected pipe client. The handle is opened for overlapped I/O, so a
/// read blocked in one thread doesn't block replies written from another.
/// (With a plain synchronous pipe handle Windows serialises the two, and the
/// host deadlocks on its first reply.)
#[cfg(windows)]
struct Pipe(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
unsafe impl Send for Pipe {}
#[cfg(windows)]
unsafe impl Sync for Pipe {}

#[cfg(windows)]
impl Pipe {
    fn transfer(&self, write: bool, buf: &mut [u8]) -> std::io::Result<usize> {
        use windows::Win32::Foundation::{CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_PENDING};
        use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
        use windows::Win32::System::Threading::CreateEventW;
        use windows::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};

        unsafe {
            let event = CreateEventW(None, true, false, None).map_err(to_io)?;
            let mut ov = OVERLAPPED {
                hEvent: event,
                ..Default::default()
            };
            let started = if write {
                WriteFile(self.0, Some(&*buf), None, Some(&mut ov))
            } else {
                ReadFile(self.0, Some(buf), None, Some(&mut ov))
            };
            let mut n = 0u32;
            let result = match started {
                Ok(()) => GetOverlappedResult(self.0, &ov, &mut n, true),
                Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {
                    GetOverlappedResult(self.0, &ov, &mut n, true)
                }
                Err(e) => Err(e),
            };
            let _ = CloseHandle(event);
            match result {
                Ok(()) => Ok(n as usize),
                // The client went away: end of stream.
                Err(e) if e.code() == ERROR_BROKEN_PIPE.to_hresult() => Ok(0),
                Err(e) => Err(to_io(e)),
            }
        }
    }
}

#[cfg(windows)]
impl Drop for Pipe {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Pipes::DisconnectNamedPipe;
        unsafe {
            let _ = DisconnectNamedPipe(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn to_io(e: windows::core::Error) -> std::io::Error {
    std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF)
}

#[cfg(windows)]
struct PipeReader(Arc<Pipe>);
#[cfg(windows)]
struct PipeWriter(Arc<Pipe>);

#[cfg(windows)]
impl std::io::Read for PipeReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.transfer(false, buf)
    }
}

#[cfg(windows)]
impl Write for PipeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut copy = buf.to_vec();
        self.0.transfer(true, &mut copy)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
pub fn listen(tx: Sender<Msg>) -> std::io::Result<()> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED,
        PIPE_ACCESS_DUPLEX,
    };
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };
    use windows::Win32::System::Threading::CreateEventW;
    use windows::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};

    let name = HSTRING::from(crate::paths::ipc_endpoint());
    let create = move |first: bool| unsafe {
        let mut flags: FILE_FLAGS_AND_ATTRIBUTES = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
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

    // Waits for a client on an overlapped pipe handle.
    let wait_for_client = |h: HANDLE| -> bool {
        unsafe {
            let Ok(event) = CreateEventW(None, true, false, None) else {
                return false;
            };
            let mut ov = OVERLAPPED {
                hEvent: event,
                ..Default::default()
            };
            let ok = match ConnectNamedPipe(h, Some(&mut ov)) {
                Ok(()) => true,
                Err(e) if e.code() == ERROR_PIPE_CONNECTED.to_hresult() => true,
                Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {
                    let mut n = 0u32;
                    GetOverlappedResult(h, &ov, &mut n, true).is_ok()
                }
                Err(_) => false,
            };
            let _ = windows::Win32::Foundation::CloseHandle(event);
            ok
        }
    };

    let first = create(true).map_err(|e| std::io::Error::new(std::io::ErrorKind::AddrInUse, e))?;
    log(format!("listening on {}", crate::paths::ipc_endpoint()));
    // HANDLE wraps a raw pointer, which isn't Send; pass the value across.
    let first = first.0 as isize;
    std::thread::spawn(move || {
        let mut next = Some(HANDLE(first as _));
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
            let pipe = Arc::new(Pipe(h));
            if !wait_for_client(h) {
                continue; // dropping `pipe` closes the handle
            }
            n += 1;
            let tx = tx.clone();
            let out: Outbox = Arc::new(Mutex::new(Box::new(PipeWriter(pipe.clone()))));
            let id = n;
            std::thread::spawn(move || serve_client(id, PipeReader(pipe), out, tx));
        }
    });
    Ok(())
}

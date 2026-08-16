use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::cli::Command;

const PROTOCOL_VERSION: u16 = 1;
const MAX_REQUEST_BYTES: u64 = 64 * 1024;

#[derive(Debug, Serialize, Deserialize)]
struct Request {
    version: u16,
    command: Command,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Response {
    ok: bool,
    message: String,
}

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("XDG_RUNTIME_DIR is unset")]
    NoRuntimeDirectory,
    #[error("cannot connect to daemon at {path}: {source}; start it with `mousr daemon`")]
    Connect {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("daemon socket error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid daemon message: {0}")]
    Json(#[from] serde_json::Error),
    #[error("daemon rejected request: {0}")]
    Rejected(String),
    #[error("another daemon is already running at {0}")]
    AlreadyRunning(PathBuf),
}

pub fn socket_path() -> Result<PathBuf, IpcError> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or(IpcError::NoRuntimeDirectory)?;
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_owned());
    let safe_display: String = display
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok(PathBuf::from(runtime).join(format!("mousr-{safe_display}.sock")))
}

pub fn send_command(command: Command) -> Result<(), IpcError> {
    let path = socket_path()?;
    let mut stream =
        UnixStream::connect(&path).map_err(|source| IpcError::Connect { path, source })?;
    serde_json::to_writer(
        &mut stream,
        &Request {
            version: PROTOCOL_VERSION,
            command,
        },
    )?;
    stream.write_all(b"\n")?;
    let response: Response =
        serde_json::from_reader(BufReader::new(stream).take(MAX_REQUEST_BYTES))?;
    if response.ok {
        Ok(())
    } else {
        Err(IpcError::Rejected(response.message))
    }
}

pub(crate) fn bind_listener() -> Result<(UnixListener, SocketGuard), IpcError> {
    let path = socket_path()?;
    if UnixStream::connect(&path).is_ok() {
        return Err(IpcError::AlreadyRunning(path));
    }
    if path.exists() {
        fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    Ok((listener, SocketGuard(path)))
}

pub(crate) struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(crate) fn read_request(stream: &UnixStream) -> Result<Command, IpcError> {
    let mut reader = BufReader::new(stream).take(MAX_REQUEST_BYTES);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let request: Request = serde_json::from_str(&line)?;
    if request.version != PROTOCOL_VERSION {
        return Err(IpcError::Rejected(format!(
            "protocol version {} is unsupported; expected {PROTOCOL_VERSION}",
            request.version
        )));
    }
    Ok(request.command)
}

pub(crate) fn write_response(stream: &mut UnixStream, response: &Response) -> Result<(), IpcError> {
    serde_json::to_writer(&mut *stream, response)?;
    stream.write_all(b"\n")?;
    Ok(())
}

impl Response {
    pub(crate) fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
        }
    }
    pub(crate) fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
        }
    }
}

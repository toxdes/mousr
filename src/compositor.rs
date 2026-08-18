use std::{
    env,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
};

use log::debug;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug)]
pub struct Sway {
    socket: PathBuf,
}

#[derive(Debug, Error)]
pub enum CompositorError {
    #[error("SWAYSOCK is unset; Mousr currently requires Sway")]
    MissingSwaySocket,
    #[error("cannot read the monotonic clock: {0}")]
    Clock(std::io::Error),
    #[error("Sway IPC error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid Sway IPC response: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Sway returned an invalid IPC header")]
    InvalidHeader,
    #[error("Sway IPC response is too large")]
    ResponseTooLarge,
    #[error("Sway reported no focused workspace")]
    NoFocusedWorkspace,
    #[error("Sway rejected cursor command: {0}")]
    Command(String),
}

impl Sway {
    pub fn from_env() -> Result<Self, CompositorError> {
        env::var_os("SWAYSOCK")
            .map(|socket| {
                let socket = PathBuf::from(socket);
                debug!(target: "mousr::compositor", "using Sway IPC socket path={}", socket.display());
                socket
            })
            .map(|socket| Self { socket })
            .ok_or(CompositorError::MissingSwaySocket)
    }

    fn request(&self, message_type: u32, payload: &[u8]) -> Result<Vec<u8>, CompositorError> {
        let mut stream = UnixStream::connect(&self.socket)?;
        let mut header = Vec::with_capacity(14);
        header.extend_from_slice(b"i3-ipc");
        header.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
        header.extend_from_slice(&message_type.to_ne_bytes());
        stream.write_all(&header)?;
        stream.write_all(payload)?;

        let mut response_header = [0_u8; 14];
        stream.read_exact(&mut response_header)?;
        if &response_header[..6] != b"i3-ipc" {
            return Err(CompositorError::InvalidHeader);
        }
        let length = u32::from_ne_bytes(
            response_header[6..10]
                .try_into()
                .map_err(|_| CompositorError::InvalidHeader)?,
        ) as usize;
        if length > 16 * 1024 * 1024 {
            return Err(CompositorError::ResponseTooLarge);
        }
        let mut payload = vec![0; length];
        stream.read_exact(&mut payload)?;
        Ok(payload)
    }

    pub fn command(&self, command: &str) -> Result<(), CompositorError> {
        let replies: Vec<CommandReply> =
            serde_json::from_slice(&self.request(0, command.as_bytes())?)?;
        if let Some(reply) = replies.into_iter().find(|reply| !reply.success) {
            return Err(CompositorError::Command(
                reply.error.unwrap_or_else(|| "unknown Sway error".into()),
            ));
        }
        Ok(())
    }

    pub fn focused_output(&self) -> Result<String, CompositorError> {
        let workspaces: Vec<WorkspaceReply> = serde_json::from_slice(&self.request(1, &[])?)?;
        let output = workspaces
            .into_iter()
            .find(|workspace| workspace.focused)
            .map(|workspace| workspace.output)
            .ok_or(CompositorError::NoFocusedWorkspace)?;
        debug!(target: "mousr::compositor", "resolved focused output output={}", output);
        Ok(output)
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct CommandReply {
    success: bool,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceReply {
    focused: bool,
    output: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_workspace_fixture() {
        let replies: Vec<WorkspaceReply> = serde_json::from_str(
            r#"[{"focused":false,"output":"DP-1"},{"focused":true,"output":"eDP-1"}]"#,
        )
        .unwrap();
        assert_eq!(
            replies.iter().find(|reply| reply.focused).unwrap().output,
            "eDP-1"
        );
    }
}

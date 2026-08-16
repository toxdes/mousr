use std::{
    env,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub focused: bool,
}

pub trait Compositor {
    fn outputs(&mut self) -> Result<Vec<Output>, CompositorError>;
}

#[derive(Debug)]
pub struct Sway {
    socket: PathBuf,
}

#[derive(Debug, Error)]
pub enum CompositorError {
    #[error("SWAYSOCK is unset; Mousr currently requires Sway")]
    MissingSwaySocket,
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
            .map(PathBuf::from)
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
}

impl Compositor for Sway {
    fn outputs(&mut self) -> Result<Vec<Output>, CompositorError> {
        let workspaces: Vec<WorkspaceReply> = serde_json::from_slice(&self.request(1, &[])?)?;
        let focused_output = workspaces
            .into_iter()
            .find(|workspace| workspace.focused)
            .map(|workspace| workspace.output)
            .ok_or(CompositorError::NoFocusedWorkspace)?;
        let replies: Vec<OutputReply> = serde_json::from_slice(&self.request(3, &[])?)?;
        let mut outputs: Vec<Output> = replies
            .into_iter()
            .filter(|output| output.active)
            .map(|output| Output {
                focused: output.name == focused_output,
                name: output.name,
                x: output.rect.x,
                y: output.rect.y,
                width: output.rect.width,
                height: output.rect.height,
                scale: output.scale,
            })
            .collect();
        outputs.sort_by_key(|output| (output.y, output.x, output.name.clone()));
        Ok(outputs)
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

#[derive(Debug, Deserialize)]
struct OutputReply {
    name: String,
    active: bool,
    scale: f64,
    rect: RectReply,
}

#[derive(Debug, Deserialize)]
struct RectReply {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_output_fixture() {
        let replies: Vec<OutputReply> = serde_json::from_str(r#"[{"name":"DP-1","active":true,"scale":1.5,"rect":{"x":-1920,"y":0,"width":1920,"height":1080}}]"#).unwrap();
        assert_eq!(replies[0].name, "DP-1");
        assert_eq!(replies[0].rect.x, -1920);
        assert_eq!(replies[0].scale, 1.5);
    }
}

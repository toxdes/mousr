use std::{
    env,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
};

use log::debug;
use serde::Deserialize;
use thiserror::Error;

use crate::{
    cli::{Direction, MouseButton},
    compositor::{CompositorControl, CompositorError},
    mode::KeyState,
};

#[derive(Debug)]
pub struct Sway {
    socket: PathBuf,
}

#[derive(Debug, Error)]
pub enum SwayError {
    #[error("SWAYSOCK is unset; cannot connect to Sway")]
    MissingSocket,
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

impl From<SwayError> for CompositorError {
    fn from(error: SwayError) -> Self {
        Self::Backend(Box::new(error))
    }
}

impl Sway {
    pub fn from_env() -> Result<Self, SwayError> {
        let socket = env::var_os("SWAYSOCK")
            .map(PathBuf::from)
            .ok_or(SwayError::MissingSocket)?;
        debug!(target: "mousr::sway", "using Sway IPC socket path={}", socket.display());
        Ok(Self { socket })
    }

    fn request(&self, message_type: u32, payload: &[u8]) -> Result<Vec<u8>, SwayError> {
        let mut stream = UnixStream::connect(&self.socket)?;
        let mut header = [0_u8; 14];
        header[..6].copy_from_slice(b"i3-ipc");
        header[6..10].copy_from_slice(&(payload.len() as u32).to_ne_bytes());
        header[10..14].copy_from_slice(&message_type.to_ne_bytes());
        stream.write_all(&header)?;
        stream.write_all(payload)?;

        let mut response_header = [0_u8; 14];
        stream.read_exact(&mut response_header)?;
        if &response_header[..6] != b"i3-ipc" {
            return Err(SwayError::InvalidHeader);
        }
        let length = u32::from_ne_bytes(
            response_header[6..10]
                .try_into()
                .map_err(|_| SwayError::InvalidHeader)?,
        ) as usize;
        if length > 16 * 1024 * 1024 {
            return Err(SwayError::ResponseTooLarge);
        }
        let mut payload = vec![0; length];
        stream.read_exact(&mut payload)?;
        Ok(payload)
    }

    fn command(&self, command: &str) -> Result<(), SwayError> {
        let replies: Vec<CommandReply> =
            serde_json::from_slice(&self.request(0, command.as_bytes())?)?;
        if let Some(reply) = replies.into_iter().find(|reply| !reply.success) {
            return Err(SwayError::Command(
                reply.error.unwrap_or_else(|| "unknown Sway error".into()),
            ));
        }
        Ok(())
    }

    fn execute(&self, command: String) -> Result<(), CompositorError> {
        self.command(&command).map_err(Into::into)
    }
}

impl CompositorControl for Sway {
    fn focused_output(&self) -> Result<String, CompositorError> {
        let workspaces: Vec<WorkspaceReply> =
            serde_json::from_slice(&self.request(1, &[])?).map_err(SwayError::from)?;
        let output = workspaces
            .into_iter()
            .find(|workspace| workspace.focused)
            .map(|workspace| workspace.output)
            .ok_or(SwayError::NoFocusedWorkspace)?;
        debug!(target: "mousr::sway", "resolved focused output output={}", output);
        Ok(output)
    }

    fn warp_cursor(&self, seat: &str, x: i64, y: i64) -> Result<(), CompositorError> {
        self.execute(format!("seat {seat} cursor set {x} {y}"))
    }

    fn move_cursor(&self, seat: &str, x: f64, y: f64) -> Result<(), CompositorError> {
        self.execute(format!("seat {seat} cursor move {x:.3} {y:.3}"))
    }

    fn button(
        &self,
        seat: &str,
        button: MouseButton,
        state: KeyState,
    ) -> Result<(), CompositorError> {
        let action = match state {
            KeyState::Pressed => "press",
            KeyState::Released => "release",
        };
        self.execute(format!(
            "seat {seat} cursor {action} {}",
            button_number(button)
        ))
    }

    fn click(&self, seat: &str, button: MouseButton) -> Result<(), CompositorError> {
        let button = button_number(button);
        self.execute(format!(
            "seat {seat} cursor press {button}; seat {seat} cursor release {button}"
        ))
    }

    fn scroll(&self, seat: &str, direction: Direction, amount: f64) -> Result<(), CompositorError> {
        let button = match direction {
            Direction::Up => 4,
            Direction::Down => 5,
            Direction::Left => 6,
            Direction::Right => 7,
        };
        let notches = (amount / 15.0).round().max(1.0) as usize;
        let action = format!(
            "seat {seat} cursor press button{button}; seat {seat} cursor release button{button}"
        );
        self.execute(
            std::iter::repeat_n(action, notches)
                .collect::<Vec<_>>()
                .join("; "),
        )
    }
}

fn button_number(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "button1",
        MouseButton::Middle => "button2",
        MouseButton::Right => "button3",
    }
}

#[derive(Debug, Deserialize)]
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
        let workspaces: Vec<WorkspaceReply> = serde_json::from_str(
            r#"[{"focused":false,"output":"DP-1"},{"focused":true,"output":"eDP-1"}]"#,
        )
        .unwrap();
        assert_eq!(
            workspaces
                .iter()
                .find(|workspace| workspace.focused)
                .unwrap()
                .output,
            "eDP-1"
        );
    }
}

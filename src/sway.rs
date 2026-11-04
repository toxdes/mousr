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
    #[error("Sway reported no usable focused window for seat {0:?}")]
    NoFocusedWindow(String),
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

    fn warp_to_focused_window(&self, seat: &str) -> Result<(), CompositorError> {
        let seats: Vec<SeatReply> =
            serde_json::from_slice(&self.request(101, &[])?).map_err(SwayError::from)?;
        let focus = seats
            .iter()
            .find(|candidate| candidate.name == seat)
            .map(|seat| seat.focus)
            .filter(|id| *id != 0)
            .ok_or_else(|| SwayError::NoFocusedWindow(seat.to_owned()))?;
        let tree: TreeNode =
            serde_json::from_slice(&self.request(4, &[])?).map_err(SwayError::from)?;
        let (x, y) = focused_window_center(&tree, focus)
            .ok_or_else(|| SwayError::NoFocusedWindow(seat.to_owned()))?;
        // Unlike global window rectangles, `seat cursor set` coordinates start
        // at the output layout's top-left corner, which can have a negative origin.
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
        // Scroll buttons are axis events, not held buttons. Sway emits an axis
        // event for both press and release, so sending a pair doubles the wheel.
        let action = format!("seat {seat} cursor press button{button}");
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

#[derive(Debug, Deserialize)]
struct SeatReply {
    name: String,
    focus: i64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WindowRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Debug, Deserialize)]
struct TreeNode {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
    rect: WindowRect,
    #[serde(default)]
    window_rect: WindowRect,
    visible: Option<bool>,
    shell: Option<String>,
    app_id: Option<String>,
    window: Option<i64>,
    #[serde(default)]
    nodes: Vec<TreeNode>,
    #[serde(default)]
    floating_nodes: Vec<TreeNode>,
}

impl TreeNode {
    fn find(&self, id: i64) -> Option<&Self> {
        if self.id == id {
            return Some(self);
        }
        self.nodes
            .iter()
            .chain(&self.floating_nodes)
            .find_map(|node| node.find(id))
    }
}

fn focused_window_center(tree: &TreeNode, focus: i64) -> Option<(i64, i64)> {
    let node = tree.find(focus)?;
    if node.kind != "con"
        || node.visible == Some(false)
        || (node.shell.is_none() && node.app_id.is_none() && node.window.is_none())
        || node.window_rect.width == 0
        || node.window_rect.height == 0
    {
        return None;
    }
    let x = i64::from(node.rect.x)
        + i64::from(node.window_rect.x)
        + i64::from(node.window_rect.width / 2)
        - i64::from(tree.rect.x);
    let y = i64::from(node.rect.y)
        + i64::from(node.window_rect.y)
        + i64::from(node.window_rect.height / 2)
        - i64::from(tree.rect.y);
    if x < 0 || y < 0 || x >= i64::from(tree.rect.width) || y >= i64::from(tree.rect.height) {
        return None;
    }
    Some((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn with_ipc<T>(replies: Vec<Value>, run: impl FnOnce(&Sway) -> T) -> (T, Vec<(u32, String)>) {
        use std::{
            os::unix::net::UnixListener,
            thread,
            time::{Duration, Instant, SystemTime, UNIX_EPOCH},
        };
        let path = env::temp_dir().join(format!(
            "mousr-sway-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in replies {
                let deadline = Instant::now() + Duration::from_secs(3);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            thread::sleep(Duration::from_millis(1))
                        }
                        Err(error) => panic!("test IPC connection failed: {error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut header = [0; 14];
                stream.read_exact(&mut header).unwrap();
                assert_eq!(&header[..6], b"i3-ipc");
                let size = u32::from_ne_bytes(header[6..10].try_into().unwrap()) as usize;
                let message_type = u32::from_ne_bytes(header[10..14].try_into().unwrap());
                let mut payload = vec![0; size];
                stream.read_exact(&mut payload).unwrap();
                requests.push((message_type, String::from_utf8(payload).unwrap()));
                let payload = serde_json::to_vec(&reply).unwrap();
                header[6..10].copy_from_slice(&(payload.len() as u32).to_ne_bytes());
                stream.write_all(&header).unwrap();
                stream.write_all(&payload).unwrap();
            }
            requests
        });
        let result = run(&Sway {
            socket: path.clone(),
        });
        let requests = server.join().unwrap();
        std::fs::remove_file(path).unwrap();
        (result, requests)
    }

    fn window_tree() -> Value {
        json!({
            "id": 1, "type": "root", "rect": {"x": -1920, "y": -1080, "width": 3840, "height": 2160},
            "nodes": [{
                "id": 2, "type": "output", "rect": {"width": 1920, "height": 1080},
                "nodes": [{
                    "id": 3, "type": "workspace", "rect": {"width": 1920, "height": 1080},
                    "nodes": [{
                        "id": 4, "type": "con", "app_id": "terminal", "focused": true, "visible": true,
                        "rect": {"x": -1920, "y": -1080, "width": 808, "height": 624},
                        "window_rect": {"x": 4, "y": 24, "width": 800, "height": 600}
                    }],
                    "floating_nodes": [{
                        "id": 5, "type": "con", "window": 123, "focused": false, "visible": true,
                        "rect": {"x": 100, "y": 200, "width": 404, "height": 230},
                        "window_rect": {"x": 2, "y": 30, "width": 400, "height": 200}
                    }]
                }]
            }]
        })
    }

    #[test]
    fn fallback_scroll_sends_one_axis_command_per_notch_in_all_directions() {
        for (direction, button) in [
            (Direction::Up, 4),
            (Direction::Down, 5),
            (Direction::Left, 6),
            (Direction::Right, 7),
        ] {
            let (result, requests) = with_ipc(vec![json!([{"success": true}])], |sway| {
                sway.scroll("seat0", direction, 30.0)
            });
            result.unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].0, 0);
            let actions: Vec<_> = requests[0].1.split("; ").collect();
            assert_eq!(actions.len(), 2, "30 units should emit two wheel events");
            assert!(
                actions
                    .iter()
                    .all(|action| *action == format!("seat seat0 cursor press button{button}"))
            );
            assert!(!requests[0].1.contains("release"));
        }
    }

    #[test]
    fn focused_scroll_uses_the_requested_seat_and_floating_window_content() {
        let replies = vec![
            json!([{"name": "seat0", "focus": 4}, {"name": "seat1", "focus": 5}]),
            window_tree(),
            json!([{"success": true}]),
        ];
        let (result, requests) = with_ipc(replies, |sway| sway.warp_to_focused_window("seat1"));
        result.unwrap();
        assert_eq!(
            requests,
            [
                (101, String::new()),
                (4, String::new()),
                (0, "seat seat1 cursor set 2222 1410".into())
            ]
        );
    }

    #[test]
    fn focused_window_center_handles_negative_output_origins_and_window_decorations() {
        let tree: TreeNode = serde_json::from_value(window_tree()).unwrap();
        assert_eq!(focused_window_center(&tree, 4), Some((404, 324)));
        assert_eq!(focused_window_center(&tree, 5), Some((2222, 1410)));
        assert_eq!(focused_window_center(&tree, 3), None);
        assert_eq!(focused_window_center(&tree, 999), None);
    }

    #[test]
    fn missing_seat_or_focus_never_emits_a_cursor_command() {
        for seats in [
            json!([]),
            json!([{"name": "seat1", "focus": 4}]),
            json!([{"name": "seat0", "focus": 0}]),
        ] {
            let (result, requests) =
                with_ipc(vec![seats], |sway| sway.warp_to_focused_window("seat0"));
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("no usable focused window")
            );
            assert_eq!(requests, [(101, String::new())]);
        }
    }

    #[test]
    fn hidden_empty_and_offscreen_windows_are_rejected_before_warping() {
        for (field, value) in [
            ("visible", json!(false)),
            ("window_rect", json!({"width": 0, "height": 10})),
            ("rect", json!({"x": 10000, "width": 800, "height": 600})),
        ] {
            let mut tree = window_tree();
            tree["nodes"][0]["nodes"][0]["nodes"][0][field] = value;
            let (result, requests) =
                with_ipc(vec![json!([{"name": "seat0", "focus": 4}]), tree], |sway| {
                    sway.warp_to_focused_window("seat0")
                });
            assert!(result.is_err());
            assert_eq!(
                requests.iter().map(|(kind, _)| *kind).collect::<Vec<_>>(),
                [101, 4]
            );
        }
    }

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

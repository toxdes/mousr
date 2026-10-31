use std::{error::Error, time::Duration};

use crate::{
    cli::{DaemonOptions, Direction, GridOptions, MouseButton, Scope},
    grid::Region,
    mode::KeyState,
    render::Frame,
};

pub type BackendResult<T = ()> = Result<T, Box<dyn Error>>;

#[derive(Clone, Copy)]
pub struct OutputInfo<'a> {
    pub name: &'a str,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// The presentation surface is configured and may receive focus or frames.
    pub ready: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timer {
    Motion,
    Animation,
    Focus(u64),
    Capture(u64),
}

pub trait DisplayBackend {
    fn outputs(&self) -> impl Iterator<Item = OutputInfo<'_>>;
    fn focused_output(&self) -> BackendResult<String>;
    fn focus(&mut self, output: &str) -> BackendResult;
    fn park(&mut self) -> BackendResult;
    fn present(&mut self, output: &str, frame: &Frame) -> BackendResult;
    fn can_inhibit_shortcuts(&self) -> bool;
    fn can_capture(&self) -> bool;
    /// Hide the overlay before capturing the desktop, without releasing keyboard focus.
    /// Complete asynchronously through `App::captured`, preserving the request ID.
    fn capture(&mut self, id: u64, output: &str) -> BackendResult;
    fn cancel_captures(&mut self);
    /// Dispatch to `App::timer`; reschedule only when it returns a duration.
    fn schedule(&mut self, timer: Timer, delay: Duration) -> BackendResult;
    /// Position is local to the named output, in logical pixels.
    fn warp(&mut self, output: &str, x: u32, y: u32) -> BackendResult;
    fn move_cursor(&mut self, x: f64, y: f64) -> BackendResult;
    fn button(&mut self, button: MouseButton, state: KeyState) -> BackendResult;
    fn click(&mut self, button: MouseButton) -> BackendResult {
        self.button(button, KeyState::Pressed)?;
        self.button(button, KeyState::Released)
    }
    /// Amount is positive; direction carries the sign.
    fn scroll(&mut self, direction: Direction, amount: f64) -> BackendResult;
}

pub fn select_regions(
    outputs: &[Region],
    focused_output: &str,
    options: &GridOptions,
    default_scope: Scope,
) -> Result<Vec<Region>, String> {
    if let Some(name) = options.output.as_deref() {
        let output = outputs
            .iter()
            .find(|output| output.output == name)
            .ok_or_else(|| format!("output {name:?} is not active"))?;
        return Ok(vec![output.clone()]);
    }

    Ok(outputs
        .iter()
        .filter(|output| {
            options.scope.unwrap_or(default_scope) == Scope::All || output.output == focused_output
        })
        .cloned()
        .collect())
}

pub fn socket_name() -> String {
    #[cfg(feature = "wayland")]
    {
        std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_owned())
    }
    #[cfg(not(feature = "wayland"))]
    {
        "default".to_owned()
    }
}

pub fn run_daemon(options: DaemonOptions) -> BackendResult {
    #[cfg(all(feature = "wayland", feature = "sway"))]
    {
        let compositor = Box::new(crate::sway::Sway::from_env()?);
        crate::wayland::run_daemon(options, compositor).map_err(Into::into)
    }
    #[cfg(not(all(feature = "wayland", feature = "sway")))]
    {
        let _ = options;
        Err("no supported display/compositor pair is enabled in this build".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Rect;

    #[test]
    fn selects_requested_output_or_scope() {
        let outputs = ["eDP-1", "DP-1"].map(|name| Region {
            output: name.into(),
            bounds: Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
        });
        let options = GridOptions::default();
        assert_eq!(
            select_regions(&outputs, "DP-1", &options, Scope::Focused)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            select_regions(&outputs, "DP-1", &options, Scope::All)
                .unwrap()
                .len(),
            2
        );
        let options = GridOptions {
            output: Some("eDP-1".into()),
            ..options
        };
        assert_eq!(
            select_regions(&outputs, "DP-1", &options, Scope::All).unwrap()[0].output,
            "eDP-1"
        );
    }
}

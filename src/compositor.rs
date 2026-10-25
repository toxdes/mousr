use std::error::Error;

use thiserror::Error;

use crate::{
    cli::{Direction, MouseButton},
    mode::KeyState,
};

pub trait CompositorControl {
    /// The name must match the display backend's output names.
    fn focused_output(&self) -> Result<String, CompositorError>;
    /// Coordinates are global to the compositor, including negative output origins.
    fn warp_cursor(&self, seat: &str, x: i64, y: i64) -> Result<(), CompositorError>;
    /// Distances are relative to the current cursor position.
    fn move_cursor(&self, seat: &str, x: f64, y: f64) -> Result<(), CompositorError>;
    fn button(
        &self,
        seat: &str,
        button: MouseButton,
        state: KeyState,
    ) -> Result<(), CompositorError>;
    fn click(&self, seat: &str, button: MouseButton) -> Result<(), CompositorError>;
    /// Amount is positive; direction carries the sign.
    fn scroll(&self, seat: &str, direction: Direction, amount: f64) -> Result<(), CompositorError>;
}

#[derive(Debug, Error)]
pub enum CompositorError {
    #[error("output {0:?} disappeared")]
    MissingOutput(String),
    #[error(transparent)]
    Backend(Box<dyn Error + Send + Sync>),
}

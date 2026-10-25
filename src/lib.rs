#![forbid(unsafe_code)]

mod animation;
pub mod app;
pub mod backend;
pub mod cli;
pub mod compositor;
pub mod config;
mod font_data;
pub mod grid;
mod hints;
pub mod ipc;
pub mod logging;
pub mod mode;
mod motion;
pub mod render;
#[cfg(feature = "sway")]
pub mod sway;
#[cfg(feature = "wayland")]
pub mod wayland;

pub const APP_NAME: &str = "mousr";

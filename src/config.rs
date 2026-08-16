use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use thiserror::Error;

use crate::cli::Scope;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub general: General,
    pub grid: Grid,
    pub motion: Motion,
    pub scroll: Scroll,
    pub bindings: Bindings,
    pub ui: Ui,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct General {
    pub scope: Scope,
    pub require_shortcut_inhibit: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Grid {
    pub min_tile_width: u32,
    pub min_tile_height: u32,
    pub max_label_length: u8,
    pub max_depth: u8,
    pub max_cells: usize,
    pub auto_descend: bool,
    pub unmatched: Unmatched,
    pub unmatched_opacity: f32,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Unmatched {
    Keep,
    Dim,
    Hide,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Motion {
    pub initial_speed: f64,
    pub acceleration: f64,
    pub max_speed: f64,
    pub tick_hz: u16,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scroll {
    pub vertical_step: f64,
    pub horizontal_step: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bindings {
    pub grid: GridBindings,
    pub mouse: MouseBindings,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GridBindings {
    pub left_click: String,
    pub middle_click: String,
    pub right_click: String,
    pub scroll_up: String,
    pub scroll_down: String,
    pub scroll_left: String,
    pub scroll_right: String,
    pub enter_mouse: String,
    pub move_only: String,
    pub descend: String,
    pub back: String,
    pub cancel: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MouseBindings {
    pub left: String,
    pub down: String,
    pub up: String,
    pub right: String,
    pub left_button: String,
    pub middle_button: String,
    pub right_button: String,
    pub scroll_up: String,
    pub scroll_down: String,
    pub scroll_left: String,
    pub scroll_right: String,
    pub cancel: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ui {
    pub font_path: Option<PathBuf>,
    pub font_size: f32,
    pub overlay_background: Color,
    pub cell_background: Color,
    pub grid_border: Color,
    pub grid_border_width: f32,
    pub label_background: Color,
    pub label_foreground: Color,
    pub matched_background: Color,
    pub matched_foreground: Color,
    pub selected_background: Color,
    pub selected_border: Color,
    pub selected_border_width: f32,
    pub badge_background: Color,
    pub badge_foreground: Color,
    pub badge_border: Color,
    pub badge_border_width: f32,
    pub target_ring: Color,
    pub target_ring_width: f32,
    pub target_ring_radius: f32,
    pub show_badge: bool,
    pub show_target_ring: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub [u8; 4]);

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for Color {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let hex = value.strip_prefix('#').ok_or("color must start with #")?;
        if hex.len() != 6 && hex.len() != 8 {
            return Err("color must contain 6 or 8 hexadecimal digits");
        }
        let mut rgba = [0_u8; 4];
        for (index, component) in rgba.iter_mut().take(hex.len() / 2).enumerate() {
            *component = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
                .map_err(|_| "color contains a non-hexadecimal digit")?;
        }
        if hex.len() == 6 {
            rgba[3] = 255;
        }
        Ok(Self(rgba))
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot read config {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid config {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("invalid config: {0}")]
    Validation(String),
    #[error("HOME and XDG_CONFIG_HOME are both unset")]
    NoConfigDirectory,
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self, ConfigError> {
        let path = match path {
            Some(path) => path.to_owned(),
            None => default_path()?,
        };
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = fs::read_to_string(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        let config: Self =
            toml::from_str(&contents).map_err(|source| ConfigError::Parse { path, source })?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let grid = &self.grid;
        if grid.min_tile_width == 0 || grid.min_tile_height == 0 {
            return Err(ConfigError::Validation(
                "minimum tile dimensions must be positive".into(),
            ));
        }
        if !(1..=4).contains(&grid.max_label_length) {
            return Err(ConfigError::Validation(
                "max_label_length must be between 1 and 4".into(),
            ));
        }
        if grid.max_depth == 0 || grid.max_cells < 2 || grid.max_cells > 65_536 {
            return Err(ConfigError::Validation(
                "max_depth must be positive and max_cells must be between 2 and 65536".into(),
            ));
        }
        if !(0.0..=1.0).contains(&grid.unmatched_opacity) {
            return Err(ConfigError::Validation(
                "unmatched_opacity must be between 0 and 1".into(),
            ));
        }
        let motion = &self.motion;
        if !motion.initial_speed.is_finite()
            || !motion.acceleration.is_finite()
            || !motion.max_speed.is_finite()
            || motion.initial_speed <= 0.0
            || motion.acceleration < 0.0
            || motion.max_speed < motion.initial_speed
            || !(30..=1000).contains(&motion.tick_hz)
        {
            return Err(ConfigError::Validation(
                "invalid motion speed or tick rate".into(),
            ));
        }
        if !self.scroll.vertical_step.is_finite()
            || !self.scroll.horizontal_step.is_finite()
            || self.scroll.vertical_step <= 0.0
            || self.scroll.horizontal_step <= 0.0
        {
            return Err(ConfigError::Validation(
                "scroll steps must be positive finite values".into(),
            ));
        }
        Ok(())
    }
}

fn default_path() -> Result<PathBuf, ConfigError> {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path).join("mousr/config.toml"));
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| path.join(".config/mousr/config.toml"))
        .ok_or(ConfigError::NoConfigDirectory)
}

impl Default for General {
    fn default() -> Self {
        Self {
            scope: Scope::Focused,
            require_shortcut_inhibit: true,
        }
    }
}
impl Default for Grid {
    fn default() -> Self {
        Self {
            min_tile_width: 24,
            min_tile_height: 24,
            max_label_length: 2,
            max_depth: 2,
            max_cells: 4096,
            auto_descend: false,
            unmatched: Unmatched::Dim,
            unmatched_opacity: 0.18,
        }
    }
}
impl Default for Motion {
    fn default() -> Self {
        Self {
            initial_speed: 240.0,
            acceleration: 1400.0,
            max_speed: 1800.0,
            tick_hz: 120,
        }
    }
}
impl Default for Scroll {
    fn default() -> Self {
        Self {
            vertical_step: 15.0,
            horizontal_step: 15.0,
        }
    }
}
impl Default for GridBindings {
    fn default() -> Self {
        Self {
            left_click: "f".into(),
            middle_click: "d".into(),
            right_click: "s".into(),
            scroll_up: "u".into(),
            scroll_down: "e".into(),
            scroll_left: "y".into(),
            scroll_right: "o".into(),
            enter_mouse: "g".into(),
            move_only: "space".into(),
            descend: "Return".into(),
            back: "BackSpace".into(),
            cancel: "Escape".into(),
        }
    }
}
impl Default for MouseBindings {
    fn default() -> Self {
        Self {
            left: "h".into(),
            down: "j".into(),
            up: "k".into(),
            right: "l".into(),
            left_button: "f".into(),
            middle_button: "d".into(),
            right_button: "s".into(),
            scroll_up: "u".into(),
            scroll_down: "e".into(),
            scroll_left: "y".into(),
            scroll_right: "o".into(),
            cancel: "Escape".into(),
        }
    }
}

impl Default for Ui {
    fn default() -> Self {
        let color = |value: &str| value.parse().unwrap_or(Color([0, 0, 0, 255]));
        Self {
            font_path: None,
            font_size: 18.0,
            overlay_background: color("#00000040"),
            cell_background: color("#11182720"),
            grid_border: color("#94A3B8A0"),
            grid_border_width: 1.0,
            label_background: color("#111827E6"),
            label_foreground: color("#F8FAFCFF"),
            matched_background: color("#38BDF8FF"),
            matched_foreground: color("#082F49FF"),
            selected_background: color("#22C55E40"),
            selected_border: color("#4ADE80FF"),
            selected_border_width: 2.0,
            badge_background: color("#111827EE"),
            badge_foreground: color("#F8FAFCFF"),
            badge_border: color("#38BDF8FF"),
            badge_border_width: 1.0,
            target_ring: color("#F9E2AFFF"),
            target_ring_width: 3.0,
            target_ring_radius: 18.0,
            show_badge: true,
            show_target_ring: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_fields() {
        let result = toml::from_str::<Config>("[grid]\nmax_cheese = 3");
        assert!(result.is_err());
    }

    #[test]
    fn parses_alpha_colors() {
        assert_eq!(
            "#10203040".parse::<Color>().unwrap(),
            Color([16, 32, 48, 64])
        );
        assert_eq!(
            "#102030".parse::<Color>().unwrap(),
            Color([16, 32, 48, 255])
        );
    }

    #[test]
    fn validates_scroll_steps() {
        let mut config = Config::default();
        config.scroll.horizontal_step = 0.0;
        assert!(config.validate().is_err());
    }
}

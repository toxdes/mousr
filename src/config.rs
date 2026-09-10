use std::{
    collections::BTreeSet,
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
    pub root_min_tile_width: u32,
    pub root_min_tile_height: u32,
    pub min_tile_width: u32,
    pub min_tile_height: u32,
    pub refinement_zoom: f64,
    pub max_label_length: u8,
    pub max_depth: u8,
    pub max_cells: usize,
    pub auto_descend: bool,
    pub exit_on_scroll: bool,
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
    pub curve: MotionCurve,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MotionCurve {
    Linear,
    EaseIn,
    EaseOut,
    #[default]
    EaseInOut,
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
    pub double_click: String,
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
    pub double_click: String,
    pub button_lock: String,
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
    pub lens_scrim_opacity: f32,
    pub lens_border: Color,
    pub lens_border_width: f32,
    pub lens_animation_ms: u16,
    pub lens_cell_opacity: f32,
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
    pub show_action_hints: bool,
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
        let digits = hex.as_bytes();
        let mut rgba = [0_u8, 0, 0, 255];
        match digits.len() {
            3 | 4 => {
                for (index, digit) in digits.iter().copied().enumerate() {
                    rgba[index] = hex_digit(digit)? * 17;
                }
            }
            6 | 8 => {
                for (index, pair) in digits.as_chunks::<2>().0.iter().enumerate() {
                    rgba[index] = hex_digit(pair[0])? * 16 + hex_digit(pair[1])?;
                }
            }
            _ => return Err("color must contain 3, 4, 6, or 8 hexadecimal digits"),
        }
        Ok(Self(rgba))
    }
}

fn hex_digit(digit: u8) -> Result<u8, &'static str> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        b'A'..=b'F' => Ok(digit - b'A' + 10),
        _ => Err("color contains a non-hexadecimal digit"),
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
        if grid.root_min_tile_width == 0
            || grid.root_min_tile_height == 0
            || grid.min_tile_width == 0
            || grid.min_tile_height == 0
        {
            return Err(ConfigError::Validation(
                "minimum tile dimensions must be positive".into(),
            ));
        }
        if !(1..=4).contains(&grid.max_label_length) {
            return Err(ConfigError::Validation(
                "max_label_length must be between 1 and 4".into(),
            ));
        }
        if !grid.refinement_zoom.is_finite() || !(1.0..=32.0).contains(&grid.refinement_zoom) {
            return Err(ConfigError::Validation(
                "refinement_zoom must be a finite value between 1 and 32".into(),
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
        if !self.ui.lens_scrim_opacity.is_finite()
            || !(0.0..=1.0).contains(&self.ui.lens_scrim_opacity)
            || !self.ui.lens_border_width.is_finite()
            || self.ui.lens_border_width <= 0.0
            || self.ui.lens_animation_ms > 1_000
            || !self.ui.lens_cell_opacity.is_finite()
            || !(0.0..=1.0).contains(&self.ui.lens_cell_opacity)
        {
            return Err(ConfigError::Validation(
                "lens opacities must be between 0 and 1, lens_border_width must be positive, and lens_animation_ms must not exceed 1000".into(),
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
        let mouse = &self.bindings.mouse;
        let mouse_bindings = [
            mouse.left.as_str(),
            mouse.down.as_str(),
            mouse.up.as_str(),
            mouse.right.as_str(),
            mouse.left_button.as_str(),
            mouse.middle_button.as_str(),
            mouse.right_button.as_str(),
            mouse.double_click.as_str(),
            mouse.button_lock.as_str(),
            mouse.scroll_up.as_str(),
            mouse.scroll_down.as_str(),
            mouse.scroll_left.as_str(),
            mouse.scroll_right.as_str(),
            mouse.cancel.as_str(),
        ];
        if mouse_bindings.iter().any(|binding| binding.is_empty())
            || mouse_bindings
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                != mouse_bindings.len()
        {
            return Err(ConfigError::Validation(
                "mouse bindings must be non-empty and unique".into(),
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
            root_min_tile_width: 96,
            root_min_tile_height: 54,
            min_tile_width: 16,
            min_tile_height: 16,
            refinement_zoom: 1.0,
            max_label_length: 3,
            max_depth: 4,
            max_cells: 4096,
            auto_descend: false,
            exit_on_scroll: false,
            unmatched: Unmatched::Dim,
            unmatched_opacity: 0.18,
        }
    }
}
impl Default for Motion {
    fn default() -> Self {
        Self {
            initial_speed: 60.0,
            acceleration: 1400.0,
            max_speed: 1800.0,
            tick_hz: 120,
            curve: MotionCurve::EaseInOut,
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
            left_click: "s".into(),
            middle_click: "d".into(),
            right_click: "f".into(),
            double_click: "c".into(),
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
            left_button: "s".into(),
            middle_button: "d".into(),
            right_button: "f".into(),
            double_click: "c".into(),
            button_lock: "v".into(),
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
            font_size: 14.0,
            overlay_background: color("#02061759"),
            lens_scrim_opacity: 0.72,
            lens_border: color("#F59E0BFF"),
            lens_border_width: 3.0,
            lens_animation_ms: 120,
            lens_cell_opacity: 0.4,
            cell_background: color("#1E293B26"),
            grid_border: color("#CBD5E199"),
            grid_border_width: 1.0,
            label_background: color("#0F172A80"),
            label_foreground: color("#F8FAFCFF"),
            matched_background: color("#F59E0BCC"),
            matched_foreground: color("#1C1917FF"),
            selected_background: color("#22C55E38"),
            selected_border: color("#86EFACFF"),
            selected_border_width: 2.0,
            badge_background: color("#0F172AD9"),
            badge_foreground: color("#F8FAFCFF"),
            badge_border: color("#F59E0BFF"),
            badge_border_width: 1.0,
            target_ring: color("#F59E0BFF"),
            target_ring_width: 2.5,
            target_ring_radius: 16.0,
            show_badge: true,
            show_target_ring: true,
            show_action_hints: true,
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
    fn parses_short_colors() {
        assert_eq!("#fff".parse(), Ok(Color([255, 255, 255, 255])));
        assert_eq!("#0f08".parse(), Ok(Color([0, 255, 0, 136])));
        assert_eq!("#ffffff00".parse(), Ok(Color([255, 255, 255, 0])));
    }

    #[test]
    fn validates_scroll_steps() {
        let mut config = Config::default();
        config.scroll.horizontal_step = 0.0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn validates_refinement_zoom() {
        let mut config = Config::default();
        config.grid.refinement_zoom = f64::NAN;
        assert!(config.validate().is_err());
        config.grid.refinement_zoom = 0.5;
        assert!(config.validate().is_err());
        config.grid.refinement_zoom = 4.0;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validates_lens_appearance() {
        let mut config = Config::default();
        config.ui.lens_scrim_opacity = 1.5;
        assert!(config.validate().is_err());
        config.ui.lens_scrim_opacity = 0.72;
        config.ui.lens_border_width = 0.0;
        assert!(config.validate().is_err());
        config.ui.lens_border_width = 3.0;
        config.ui.lens_animation_ms = 1_001;
        assert!(config.validate().is_err());
        config.ui.lens_animation_ms = 120;
        config.ui.lens_cell_opacity = 1.1;
        assert!(config.validate().is_err());
    }

    #[test]
    fn defaults_include_double_click_bindings() {
        let config = Config::default();
        assert_eq!(config.bindings.grid.double_click, "c");
        assert_eq!(config.bindings.mouse.double_click, "c");
    }

    #[test]
    fn rejects_ambiguous_mouse_bindings() {
        let mut config = Config::default();
        config.bindings.mouse.right = config.bindings.mouse.left.clone();
        assert!(config.validate().is_err());
    }

    #[test]
    fn example_config_is_valid() {
        let config: Config = toml::from_str(include_str!("../mousr.example.toml")).unwrap();
        config.validate().unwrap();
    }

    #[test]
    fn default_layers_preserve_transparency() {
        let ui = Ui::default();
        assert!((1..255).contains(&ui.overlay_background.0[3]));
        assert!((1..255).contains(&ui.cell_background.0[3]));
        assert!((1..255).contains(&ui.label_background.0[3]));
    }
}

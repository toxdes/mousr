use std::{ffi::OsString, fmt, path::PathBuf, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct DaemonOptions {
    pub config: Option<PathBuf>,
    pub seat: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Daemon(DaemonOptionsWire),
    Reload,
    Cancel,
    Grid(GridOptions),
    Mouse,
    Click(MouseButton),
    Scroll {
        direction: Direction,
        step: Option<f64>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DaemonOptionsWire {
    pub config: Option<PathBuf>,
    pub seat: Option<String>,
}

impl From<DaemonOptionsWire> for DaemonOptions {
    fn from(value: DaemonOptionsWire) -> Self {
        Self {
            config: value.config,
            seat: value.seat,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridOptions {
    pub scope: Option<Scope>,
    pub output: Option<String>,
    pub action: GridAction,
    pub auto_descend: Option<bool>,
    pub max_depth: Option<u8>,
}

impl Default for GridOptions {
    fn default() -> Self {
        Self {
            scope: None,
            output: None,
            action: GridAction::Choose,
            auto_descend: None,
            max_depth: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    Focused,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GridAction {
    Choose,
    Move,
    Mouse,
    Left,
    Middle,
    Right,
    Scroll,
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

pub fn application_name() -> &'static str {
    application_name_from(std::env::args_os().next().as_deref())
}

fn application_name_from(path: Option<&std::ffi::OsStr>) -> &'static str {
    let invoked_as_dev = path.is_some_and(|path| {
        std::path::Path::new(path)
            .file_name()
            .is_some_and(|name| name == "mousr-dev")
    });
    if cfg!(debug_assertions) || invoked_as_dev {
        "mousr-dev"
    } else {
        "mousr"
    }
}

pub fn version() -> String {
    format!(
        "{} v{}({})",
        application_name(),
        env!("CARGO_PKG_VERSION"),
        env!("MOUSR_GIT_SHA")
    )
}

macro_rules! from_str_enum {
    ($type:ty, {$($text:literal => $value:expr),+ $(,)?}) => {
        impl FromStr for $type {
            type Err = ParseError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($text => Ok($value),)+
                    _ => Err(ParseError::InvalidValue(value.to_owned())),
                }
            }
        }
    };
}

from_str_enum!(Scope, { "focused" => Scope::Focused, "all" => Scope::All });
from_str_enum!(MouseButton, {
    "left" => MouseButton::Left,
    "middle" => MouseButton::Middle,
    "right" => MouseButton::Right,
});
from_str_enum!(Direction, {
    "up" => Direction::Up,
    "down" => Direction::Down,
    "left" => Direction::Left,
    "right" => Direction::Right,
});
from_str_enum!(GridAction, {
    "choose" => GridAction::Choose,
    "move" => GridAction::Move,
    "mouse" => GridAction::Mouse,
    "left" => GridAction::Left,
    "middle" => GridAction::Middle,
    "right" => GridAction::Right,
    "scroll" => GridAction::Scroll,
    "scroll-up" => GridAction::ScrollUp,
    "scroll-down" => GridAction::ScrollDown,
    "scroll-left" => GridAction::ScrollLeft,
    "scroll-right" => GridAction::ScrollRight,
});

#[derive(Debug, Error, PartialEq)]
pub enum ParseError {
    #[error("missing command; try `{0} --help`")]
    MissingCommand(&'static str),
    #[error("missing value for {0}")]
    MissingValue(String),
    #[error("unknown argument: {0}")]
    UnknownArgument(String),
    #[error("invalid value: {0}")]
    InvalidValue(String),
    #[error("argument is not valid UTF-8")]
    NonUtf8,
    #[error("{0}")]
    Help(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub command: Command,
    pub log_level: String,
    pub log_file: Option<PathBuf>,
}

pub fn parse<I>(args: I) -> Result<Command, ParseError>
where
    I: IntoIterator<Item = OsString>,
{
    Ok(parse_with_options(args)?.command)
}

pub fn parse_with_options<I>(args: I) -> Result<Parsed, ParseError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let program = application_name_from(args.next().as_deref());
    let mut remaining = Vec::new();
    let mut log_level = "info".to_owned();
    let mut log_file = None;
    while let Some(argument) = next_string(&mut args)? {
        match argument.as_str() {
            "--log-level" => log_level = required(&mut args, "--log-level")?,
            "--log-file" => log_file = Some(PathBuf::from(required(&mut args, "--log-file")?)),
            value if value.starts_with("--log-level=") => {
                log_level = value["--log-level=".len()..].to_owned()
            }
            value if value.starts_with("--log-file=") => {
                log_file = Some(PathBuf::from(&value["--log-file=".len()..]))
            }
            _ => remaining.push(OsString::from(argument)),
        }
    }
    let mut args = remaining.into_iter();
    let command = next_string(&mut args)?.ok_or(ParseError::MissingCommand(program))?;
    if command == "--help" || command == "-h" {
        return Err(ParseError::Help(help_for(program)));
    }
    if command == "--version" || command == "-V" {
        return Err(ParseError::Help(version()));
    }

    let command = match command.as_str() {
        "daemon" => parse_daemon(args),
        "reload" => ensure_empty(args, Command::Reload),
        "cancel" => ensure_empty(args, Command::Cancel),
        "grid" => parse_grid(args),
        "mouse" => ensure_empty(args, Command::Mouse),
        "click" => {
            let button = required(&mut args, "button")?.parse()?;
            ensure_empty(args, Command::Click(button))
        }
        "scroll" => parse_scroll(args),
        _ => Err(ParseError::UnknownArgument(command)),
    }?;
    log_level
        .parse::<crate::logging::LogLevel>()
        .map_err(ParseError::InvalidValue)?;
    Ok(Parsed {
        command,
        log_level,
        log_file,
    })
}

fn parse_daemon<I>(mut args: I) -> Result<Command, ParseError>
where
    I: Iterator<Item = OsString>,
{
    let mut options = DaemonOptionsWire {
        config: None,
        seat: None,
    };
    while let Some(argument) = next_string(&mut args)? {
        match argument.as_str() {
            "--config" => options.config = Some(PathBuf::from(required(&mut args, "--config")?)),
            "--seat" => options.seat = Some(required(&mut args, "--seat")?),
            _ => return Err(ParseError::UnknownArgument(argument)),
        }
    }
    Ok(Command::Daemon(options))
}

fn parse_grid<I>(mut args: I) -> Result<Command, ParseError>
where
    I: Iterator<Item = OsString>,
{
    let mut options = GridOptions::default();
    while let Some(argument) = next_string(&mut args)? {
        match argument.as_str() {
            "--scope" => options.scope = Some(required(&mut args, "--scope")?.parse()?),
            "--output" => options.output = Some(required(&mut args, "--output")?),
            "--action" => options.action = required(&mut args, "--action")?.parse()?,
            "--auto-descend" => options.auto_descend = Some(true),
            "--no-auto-descend" => options.auto_descend = Some(false),
            "--max-depth" => {
                options.max_depth = Some(
                    required(&mut args, "--max-depth")?
                        .parse()
                        .map_err(|_| ParseError::InvalidValue("--max-depth".to_owned()))?,
                );
            }
            _ => return Err(ParseError::UnknownArgument(argument)),
        }
    }
    if options.output.is_some() && options.scope.is_some() {
        return Err(ParseError::InvalidValue(
            "--output and --scope are mutually exclusive".to_owned(),
        ));
    }
    Ok(Command::Grid(options))
}

fn parse_scroll<I>(mut args: I) -> Result<Command, ParseError>
where
    I: Iterator<Item = OsString>,
{
    let direction = required(&mut args, "direction")?.parse()?;
    let mut step = None;
    while let Some(argument) = next_string(&mut args)? {
        match argument.as_str() {
            "--step" => {
                let value = required(&mut args, "--step")?;
                step = Some(value.parse().map_err(|_| ParseError::InvalidValue(value))?);
            }
            _ => return Err(ParseError::UnknownArgument(argument)),
        }
    }
    Ok(Command::Scroll { direction, step })
}

fn required<I>(args: &mut I, option: &str) -> Result<String, ParseError>
where
    I: Iterator<Item = OsString>,
{
    next_string(args)?.ok_or_else(|| ParseError::MissingValue(option.to_owned()))
}

fn next_string<I>(args: &mut I) -> Result<Option<String>, ParseError>
where
    I: Iterator<Item = OsString>,
{
    args.next()
        .map(|value| value.into_string().map_err(|_| ParseError::NonUtf8))
        .transpose()
}

fn ensure_empty<I>(mut args: I, command: Command) -> Result<Command, ParseError>
where
    I: Iterator<Item = OsString>,
{
    match next_string(&mut args)? {
        Some(argument) => Err(ParseError::UnknownArgument(argument)),
        None => Ok(command),
    }
}

pub fn help() -> String {
    help_for(application_name())
}

fn help_for(program: &str) -> String {
    format!(
        "{program} [--log-level LEVEL] [--log-file PATH] daemon [--config PATH] [--seat NAME]\n\
         {program} [--log-level LEVEL] [--log-file PATH] reload | cancel\n\
         {program} [--log-level LEVEL] [--log-file PATH] grid [--scope focused|all] [--output NAME] [--action ACTION]\n\
         {program} mouse\n\
         {program} click left|middle|right\n\
         {program} scroll up|down|left|right [--step AMOUNT]"
    )
}

impl fmt::Display for Command {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}",
            serde_json::to_string(self).map_err(|_| fmt::Error)?
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(arguments: &[&str]) -> Result<Command, ParseError> {
        parse(arguments.iter().map(OsString::from))
    }

    #[test]
    fn parses_grid_overrides() {
        let parsed = command(&[
            "mousr",
            "grid",
            "--scope",
            "all",
            "--action",
            "scroll-left",
            "--auto-descend",
            "--max-depth",
            "3",
        ])
        .unwrap();
        assert_eq!(
            parsed,
            Command::Grid(GridOptions {
                scope: Some(Scope::All),
                output: None,
                action: GridAction::ScrollLeft,
                auto_descend: Some(true),
                max_depth: Some(3),
            })
        );
    }

    #[test]
    fn rejects_conflicting_output_selection() {
        let result = command(&["mousr", "grid", "--scope", "all", "--output", "DP-1"]);
        assert!(matches!(result, Err(ParseError::InvalidValue(_))));
    }

    #[test]
    fn parses_horizontal_scroll() {
        assert_eq!(
            command(&["mousr", "scroll", "left", "--step", "12.5"]).unwrap(),
            Command::Scroll {
                direction: Direction::Left,
                step: Some(12.5)
            }
        );
    }

    #[test]
    fn parses_global_logging_options_before_and_after_command() {
        let parsed = parse_with_options(
            [
                "mousr",
                "--log-level",
                "debug",
                "grid",
                "--log-file",
                "/tmp/mousr.log",
            ]
            .iter()
            .map(OsString::from),
        )
        .unwrap();
        assert_eq!(parsed.log_level, "debug");
        assert_eq!(parsed.log_file, Some(PathBuf::from("/tmp/mousr.log")));
        assert_eq!(parsed.command, Command::Grid(GridOptions::default()));

        let parsed = parse_with_options(
            ["mousr", "reload", "--log-level=warn"]
                .iter()
                .map(OsString::from),
        )
        .unwrap();
        assert_eq!(parsed.log_level, "warn");
        assert_eq!(parsed.command, Command::Reload);
    }

    #[test]
    fn rejects_invalid_log_level() {
        let result = parse_with_options(
            ["mousr", "--log-level", "verbose", "cancel"]
                .iter()
                .map(OsString::from),
        );
        assert!(
            matches!(result, Err(ParseError::InvalidValue(value)) if value.contains("verbose"))
        );
    }

    #[test]
    fn dev_errors_and_help_use_dev_name() {
        let error = command(&["mousr-dev"]).unwrap_err();
        assert_eq!(error.to_string(), "missing command; try `mousr-dev --help`");

        let error = command(&["mousr-dev", "--help"]).unwrap_err();
        let ParseError::Help(help) = error else {
            panic!("expected help");
        };
        assert!(help.lines().all(|line| line.starts_with("mousr-dev ")));
    }
}

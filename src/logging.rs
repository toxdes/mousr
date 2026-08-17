use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub fn filter(self) -> LevelFilter {
        match self {
            Self::Error => LevelFilter::Error,
            Self::Warn => LevelFilter::Warn,
            Self::Info => LevelFilter::Info,
            Self::Debug => LevelFilter::Debug,
            Self::Trace => LevelFilter::Trace,
        }
    }
}

impl std::str::FromStr for LogLevel {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "error" => Ok(Self::Error),
            "warn" => Ok(Self::Warn),
            "info" => Ok(Self::Info),
            "debug" => Ok(Self::Debug),
            "trace" => Ok(Self::Trace),
            _ => Err(format!(
                "invalid log level {value:?}; expected error, warn, info, debug, or trace"
            )),
        }
    }
}

struct Logger {
    file: Option<Mutex<File>>,
    filter: LevelFilter,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.filter
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let prefix = if record.target() == "mousr::panic" {
            "PNC"
        } else {
            match record.level() {
                Level::Error => "ERR",
                Level::Warn => "WRN",
                Level::Info => "INF",
                Level::Debug => "DBG",
                Level::Trace => "TRC",
            }
        };
        let timestamp = timestamp();
        let message = format!("{}", record.args());
        let line = format!("{prefix} {timestamp} {message}\n");
        let mut stderr = io::stderr().lock();
        let _ = stderr.write_all(line.as_bytes());

        if let Some(file) = &self.file
            && let Ok(mut file) = file.lock()
        {
            let json = format!(
                "{{\"timestamp\":\"{timestamp}\",\"level\":\"{prefix}\",\"target\":{},\"message\":{}}}\n",
                json_string(record.target()),
                json_string(&message),
            );
            let _ = file.write_all(json.as_bytes());
        }
    }

    fn flush(&self) {}
}

pub fn init(level: LogLevel, file_path: Option<&Path>) -> Result<(), String> {
    let file = file_path
        .map(open_log_file)
        .transpose()
        .map_err(|error| format!("cannot open log file: {error}"))?;
    let logger = Logger {
        file: file.map(Mutex::new),
        filter: level.filter(),
    };
    log::set_boxed_logger(Box::new(logger)).map_err(set_logger_error)?;
    log::set_max_level(level.filter());
    std::panic::set_hook(Box::new(|panic| {
        log::error!(target: "mousr::panic", "{panic}");
    }));
    Ok(())
}

fn open_log_file(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new().create(true).append(true).open(path)
}

fn set_logger_error(_: SetLoggerError) -> String {
    "logging has already been initialized".into()
}

fn timestamp() -> String {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = elapsed.as_secs() % 86_400;
    format!(
        "{:02}:{:02}:{:02}.{:03}Z",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60,
        elapsed.subsec_millis()
    )
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"<invalid>\"".into())
}

use std::process::ExitCode;

use log::info;
use mousr::{cli, ipc, logging, wayland};

fn main() -> ExitCode {
    if let Err(mousr::cli::ParseError::Help(message)) = try_help() {
        println!("{message}");
        return ExitCode::SUCCESS;
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {error}", cli::application_name());
            ExitCode::FAILURE
        }
    }
}

fn try_help() -> Result<(), mousr::cli::ParseError> {
    let mut args = std::env::args_os();
    let _program = args.next();
    match args
        .next()
        .and_then(|value| value.into_string().ok())
        .as_deref()
    {
        Some("--help" | "-h") => Err(mousr::cli::ParseError::Help(mousr::cli::help())),
        Some("--version" | "-V") => Err(mousr::cli::ParseError::Help(mousr::cli::version())),
        _ => Ok(()),
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let parsed = cli::parse_with_options(std::env::args_os())?;
    let level = parsed.log_level.parse::<logging::LogLevel>()?;
    logging::init(level, parsed.log_file.as_deref())?;
    info!("starting {}", cli::version());
    match parsed.command {
        cli::Command::Daemon(options) => wayland::run_daemon(options)?,
        command => ipc::send_command(command)?,
    }
    Ok(())
}

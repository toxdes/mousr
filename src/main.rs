use std::process::ExitCode;

use mousr::{cli, ipc, wayland};

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
    let command = cli::parse(std::env::args_os())?;
    match command {
        cli::Command::Daemon(options) => wayland::run_daemon(options)?,
        command => ipc::send_command(command)?,
    }
    Ok(())
}

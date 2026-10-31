//! Argument parsing and dispatch.

use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::serve;

#[derive(Debug, Parser)]
#[command(version, about = "A profiler for code mass and structure")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Serve the web UI and the query API.
    Serve {
        /// Port to listen on; 0 picks a free one.
        #[arg(long, default_value_t = 7878)]
        port: u16,
    },
}

/// Parses the command line and runs it.
#[must_use]
pub fn run() -> ExitCode {
    match Cli::parse().command {
        Command::Serve { port } => match serve::run(port) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("codemap: {error}");
                ExitCode::FAILURE
            }
        },
    }
}

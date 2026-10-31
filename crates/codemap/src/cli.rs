//! Argument parsing and dispatch.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

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
    /// Bring the index under PATH up to date with the tree.
    Index {
        /// Root of the codebase.
        #[arg(default_value = ".")]
        path: PathBuf,
    },
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
        Command::Index { path } => {
            let started = Instant::now();
            match codemap_core::index(&path) {
                Ok(report) => {
                    println!(
                        "{} files, {} lines, {} re-extracted, {:.2}s",
                        report.file_count,
                        report.line_count,
                        report.extracted_count,
                        started.elapsed().as_secs_f64()
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("codemap: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Serve { port } => match serve::run(port) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("codemap: {error}");
                ExitCode::FAILURE
            }
        },
    }
}

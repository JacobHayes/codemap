//! Argument parsing and dispatch. Each verb builds one `Query` and prints its answer.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use clap::{Args, Parser, Subcommand, ValueEnum};
use codemap_core::{Answer, Envelope, Filter, Hierarchy, Index, MeasureRef, Query, answer, index};

use crate::{render, serve};

#[derive(Debug, Parser)]
#[command(version, about = "A profiler for code mass and structure")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Where the codebase is and how to print the answer.
#[derive(Debug, Args)]
struct Output {
    /// Root of the codebase.
    #[arg(long, short = 'C', default_value = ".")]
    root: PathBuf,
    /// Print the answer envelope as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum By {
    Path,
    Symbol,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Bring the index under PATH up to date with the tree.
    Index {
        /// Root of the codebase.
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Totals and shares by role, concern and language; counts of areas by detector.
    Overview(Output),
    /// A hierarchy with a size and a colour measure.
    Tree {
        #[command(flatten)]
        output: Output,
        /// Group by directory and file, or by file and symbol.
        #[arg(long, value_enum, default_value = "path")]
        by: By,
        /// Size measure, a sum from the catalog.
        #[arg(long, default_value = "lines")]
        size: String,
        /// Colour measure from the catalog.
        #[arg(long)]
        color: Option<String>,
        /// Levels below the root; deeper mass aggregates upward.
        #[arg(long)]
        depth: Option<u32>,
        /// Only this file or directory.
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        language: Option<String>,
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        concern: Option<String>,
        #[arg(long)]
        symbol_kind: Option<String>,
    },
    /// Ranked areas worth a closer look.
    Areas {
        #[command(flatten)]
        output: Output,
        /// How many to list.
        #[arg(long, default_value_t = 20)]
        top: u32,
        /// Only areas at or under this path.
        #[arg(long)]
        path: Option<String>,
        /// Only areas this detector found something in.
        #[arg(long)]
        detector: Option<String>,
    },
    /// One area: its findings, evidence and related areas.
    Area {
        #[command(flatten)]
        output: Output,
        /// A file or directory path, as `areas` prints it.
        subject: String,
    },
    /// Dimensions, measures and detectors available.
    Catalog(Output),
    /// Serve the web UI and the query API.
    Serve {
        /// Root of the codebase.
        #[arg(long, short = 'C', default_value = ".")]
        root: PathBuf,
        /// Port to listen on; 0 picks a free one.
        #[arg(long, default_value_t = 7878)]
        port: u16,
    },
}

type Failure = Box<dyn std::error::Error>;

/// Parses the command line and runs it.
#[must_use]
pub fn run() -> ExitCode {
    match dispatch(Cli::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("codemap: {error}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(command: Command) -> Result<(), Failure> {
    match command {
        Command::Index { path } => {
            let started = Instant::now();
            let live = Index::create(&path)?;
            let report = live.refresh()?;
            let semantic = live.refresh_semantic()?;
            println!(
                "{} files, {} lines, {} re-extracted, {:.2}s",
                report.file_count,
                report.line_count,
                report.extracted_count,
                started.elapsed().as_secs_f64()
            );
            println!("{}", semantic.summary());
            Ok(())
        }
        Command::Serve { root, port } => Ok(serve::run(&root, port)?),
        Command::Overview(output) => ask(&output, &Query::Overview),
        Command::Catalog(output) => ask(&output, &Query::Catalog),
        Command::Area { output, subject } => ask(&output, &Query::Area { subject }),
        Command::Areas {
            output,
            top,
            path,
            detector,
        } => ask(
            &output,
            &Query::Areas {
                top: Some(top),
                path,
                detector,
            },
        ),
        Command::Tree {
            output,
            by,
            size,
            color,
            depth,
            path,
            language,
            role,
            concern,
            symbol_kind,
        } => {
            let query = Query::Tree {
                hierarchy: match by {
                    By::Path => Hierarchy::Path,
                    By::Symbol => Hierarchy::Symbol,
                },
                size: Some(MeasureRef::Named(size)),
                color: color.map(MeasureRef::Named),
                filter: Filter {
                    path,
                    language,
                    role,
                    concern,
                    symbol_kind,
                    ..Filter::default()
                },
                depth,
            };
            ask(&output, &query)
        }
    }
}

/// Brings the index up to date (a hash-and-diff, cheap when nothing changed), then answers.
fn ask(output: &Output, query: &Query) -> Result<(), Failure> {
    let answer = answer_at(&output.root, query)?;
    if output.json {
        println!("{}", serde_json::to_string(&Envelope::from(answer))?);
    } else {
        print!("{}", render::render(&answer));
    }
    Ok(())
}

fn answer_at(root: &Path, query: &Query) -> Result<Answer, Failure> {
    index(root)?;
    Ok(answer(&Index::open(root)?, query)?)
}

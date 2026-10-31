//! Argument parsing and dispatch. Each verb builds one `Query` and prints its answer.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use clap::{Args, Parser, Subcommand};
use codemap_core::{Answer, Envelope, Index, Query, answer, index};

use crate::args::{AreaArgs, AreasArgs, SourceArgs, TreeArgs, XtabArgs};
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
        #[command(flatten)]
        args: TreeArgs,
    },
    /// Two dimensions crossed, with one measure in each cell.
    Xtab {
        #[command(flatten)]
        output: Output,
        #[command(flatten)]
        args: XtabArgs,
    },
    /// One file's lines with role, concern and the findings that touch them.
    Source {
        #[command(flatten)]
        output: Output,
        #[command(flatten)]
        args: SourceArgs,
    },
    /// Ranked areas worth a closer look.
    Areas {
        #[command(flatten)]
        output: Output,
        #[command(flatten)]
        args: AreasArgs,
    },
    /// One area: its findings, evidence and related areas.
    Area {
        #[command(flatten)]
        output: Output,
        #[command(flatten)]
        args: AreaArgs,
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
        Command::Area { output, args } => ask(&output, &args.query()),
        Command::Areas { output, args } => ask(&output, &args.query()),
        Command::Tree { output, args } => ask(&output, &args.query()),
        Command::Xtab { output, args } => ask(&output, &args.query()),
        Command::Source { output, args } => ask(&output, &args.query()?),
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

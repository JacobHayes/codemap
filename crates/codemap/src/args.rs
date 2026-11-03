//! The arguments of each query verb. The CLI parses them from flags (and MCP, from JSON) and
//! each builds the one `Query` it stands for, so a verb's arguments are defined once.

use clap::{Args, ValueEnum};
use codemap_core::{Axis, Filter, Hierarchy, MeasureRef, Query};
use rmcp::schemars::JsonSchema;
use serde::Deserialize;

/// How many areas `areas` lists unless told otherwise.
pub const TOP_DEFAULT: u32 = 20;
/// The measure a tile or cell is sized by unless told otherwise.
const MEASURE_DEFAULT: &str = "lines";

fn measure_default() -> String {
    MEASURE_DEFAULT.to_owned()
}

fn top_default() -> u32 {
    TOP_DEFAULT
}

/// Narrows lines on any dimension.
#[derive(Debug, Args, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct FilterArgs {
    /// Only this file or directory.
    #[arg(long)]
    pub path: Option<String>,
    #[arg(long)]
    pub language: Option<String>,
    #[arg(long)]
    pub role: Option<String>,
    #[arg(long)]
    pub concern: Option<String>,
    #[arg(long)]
    pub symbol_kind: Option<String>,
    /// Count generated and vendored code, which is left out otherwise.
    #[arg(long)]
    #[serde(default)]
    pub include_generated: bool,
}

impl FilterArgs {
    fn filter(self) -> Filter {
        Filter {
            path: self.path,
            language: self.language,
            role: self.role,
            concern: self.concern,
            symbol_kind: self.symbol_kind,
            include_generated: self.include_generated,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, ValueEnum, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(crate = "rmcp::schemars")]
pub enum By {
    #[default]
    Path,
    Symbol,
}

#[derive(Debug, Args, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct TreeArgs {
    /// Group by directory and file, or by file and symbol.
    #[arg(long, value_enum, default_value = "path")]
    #[serde(default)]
    pub by: By,
    /// Size measure, a sum from the catalog.
    #[arg(long, default_value = MEASURE_DEFAULT)]
    #[serde(default = "measure_default")]
    pub size: String,
    /// Colour measure from the catalog.
    #[arg(long)]
    pub color: Option<String>,
    /// Levels below the root; deeper mass aggregates upward.
    #[arg(long)]
    pub depth: Option<u32>,
    #[command(flatten)]
    #[serde(flatten)]
    pub filter: FilterArgs,
}

impl TreeArgs {
    #[must_use]
    pub fn query(self) -> Query {
        Query::Tree {
            hierarchy: match self.by {
                By::Path => Hierarchy::Path,
                By::Symbol => Hierarchy::Symbol,
            },
            size: Some(MeasureRef::Named(self.size)),
            color: self.color.map(MeasureRef::Named),
            filter: self.filter.filter(),
            depth: self.depth,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(crate = "rmcp::schemars")]
pub enum AxisArg {
    Role,
    Concern,
    Language,
    Kind,
    Path,
}

#[derive(Debug, Args, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct XtabArgs {
    /// Dimension down the side.
    #[arg(long, value_enum)]
    pub rows: AxisArg,
    /// Dimension across the top.
    #[arg(long, value_enum)]
    pub cols: AxisArg,
    /// Measure per cell, a sum from the catalog.
    #[arg(long, default_value = MEASURE_DEFAULT)]
    #[serde(default = "measure_default")]
    pub measure: String,
    /// Path components kept below `--path` when a dimension is `path`.
    #[arg(long)]
    pub path_depth: Option<u32>,
    #[command(flatten)]
    #[serde(flatten)]
    pub filter: FilterArgs,
}

impl XtabArgs {
    #[must_use]
    pub fn query(self) -> Query {
        let axis = |arg| match arg {
            AxisArg::Role => Axis::Role,
            AxisArg::Concern => Axis::Concern,
            AxisArg::Language => Axis::Language,
            AxisArg::Kind => Axis::Kind,
            AxisArg::Path => Axis::Path,
        };
        Query::Xtab {
            rows: axis(self.rows),
            cols: axis(self.cols),
            measure: Some(MeasureRef::Named(self.measure)),
            filter: self.filter.filter(),
            path_depth: self.path_depth,
        }
    }
}

#[derive(Debug, Args, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SourceArgs {
    /// File path relative to the root.
    pub path: String,
    /// Only these lines, as `a-b` (or `a` for one line).
    #[arg(long, value_parser = lines_range)]
    pub lines: Option<String>,
}

/// Checks `a-b` or `a` and keeps the text, so the same check serves flags and JSON.
fn lines_range(text: &str) -> Result<String, String> {
    parse_lines(text).map(|_| text.to_owned())
}

fn parse_lines(text: &str) -> Result<(u32, u32), String> {
    let number = |part: &str| {
        part.trim()
            .parse::<u32>()
            .map_err(|_| format!("{part:?} is not a line number"))
    };
    match text.split_once('-') {
        Some((start, end)) => Ok((number(start)?, number(end)?)),
        None => number(text).map(|line| (line, line)),
    }
}

impl SourceArgs {
    /// # Errors
    /// Fails when `lines` is not `a-b` or `a`.
    pub fn query(self) -> Result<Query, String> {
        let range = self.lines.as_deref().map(parse_lines).transpose()?;
        Ok(Query::Source {
            path: self.path,
            start_line: range.map(|(start, _)| start),
            end_line: range.map(|(_, end)| end),
        })
    }
}

#[derive(Debug, Args, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct AreasArgs {
    /// How many to list.
    #[arg(long, default_value_t = TOP_DEFAULT)]
    #[serde(default = "top_default")]
    pub top: u32,
    /// Only areas at or under this path; a group stays when any member is under it.
    #[arg(long)]
    pub path: Option<String>,
    /// Only areas this detector found something in.
    #[arg(long)]
    pub detector: Option<String>,
    /// Only areas whose findings touch lines of this role, such as `test`.
    #[arg(long)]
    pub role: Option<String>,
    /// Only areas whose findings touch lines with this concern.
    #[arg(long)]
    pub concern: Option<String>,
    /// Only areas whose findings touch lines in this language.
    #[arg(long)]
    pub language: Option<String>,
}

impl AreasArgs {
    #[must_use]
    pub fn query(self) -> Query {
        Query::Areas {
            top: Some(self.top),
            detector: self.detector,
            filter: Filter {
                path: self.path,
                language: self.language,
                role: self.role,
                concern: self.concern,
                ..Filter::default()
            },
        }
    }
}

#[derive(Debug, Args, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct AreaArgs {
    /// A file or directory path, or a group id (`group:...`), as `areas` prints it.
    pub subject: String,
}

impl AreaArgs {
    #[must_use]
    pub fn query(self) -> Query {
        Query::Area {
            subject: self.subject,
            filter: None,
        }
    }
}

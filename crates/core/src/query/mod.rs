//! The query layer: one [`Query`] in, one [`Answer`] out, in a versioned [`Envelope`]. Every
//! surface (CLI, HTTP, later MCP) is a shell over [`answer`]; none computes a number of its own.

mod overview;
mod scale;
mod tree;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::detect::{Area, DETECTORS, Finding};
use crate::measure::{self, Filter, MeasureDef, MeasureRef};
use crate::reader::{Index, QueryError};
use codemap_lang::{Concern, LANGUAGES, Role};

pub use overview::{AreaCounts, DetectorCount, Overview, Share, Totals};
pub use scale::{SCALE_STEPS, Scale, ScaleKind, Spacing};
pub use tree::{DEPTH_DEFAULT, Hierarchy, SizeInfo, Tree, TreeNode};

/// The schema name and version every serialized answer carries.
pub const SCHEMA: &str = "codemap.answer/v1";

/// A typed question. Adding a variant is a compile error in every surface until it is handled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[ts(export, export_to = "Query.ts", optional_fields)]
#[expect(
    clippy::large_enum_variant,
    reason = "a wire type built once per query"
)]
pub enum Query {
    /// Totals and shares by role, concern and language; counts of areas by detector.
    Overview,
    /// A hierarchy with a size measure and a colour measure, under filters.
    Tree {
        #[serde(default)]
        hierarchy: Hierarchy,
        /// A sum; `lines` when absent.
        #[serde(default)]
        size: Option<MeasureRef>,
        #[serde(default)]
        color: Option<MeasureRef>,
        #[serde(default)]
        filter: Filter,
        /// Levels drawn below the root; deeper mass aggregates upward. [`DEPTH_DEFAULT`] when absent.
        #[serde(default)]
        depth: Option<u32>,
    },
    /// Ranked areas, best first.
    Areas {
        /// How many to list; all when absent.
        #[serde(default)]
        top: Option<u32>,
        /// Only areas at or under this path.
        #[serde(default)]
        path: Option<String>,
        /// Only areas this detector found something in.
        #[serde(default)]
        detector: Option<String>,
    },
    /// One area: its findings, measures, evidence and related areas.
    Area {
        /// A file or directory path; a `path::function` subject names its file.
        subject: String,
    },
    /// Dimensions, measures and detectors available.
    Catalog,
}

/// One area with everything detectors said about it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct AreaDetail {
    pub area: Area,
    pub findings: Vec<Finding>,
    /// Areas above or below this one in the tree.
    pub related: Vec<Area>,
}

/// The ranked areas under a filter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct AreaList {
    /// Areas that match the filter, of which `areas` shows the first few.
    pub matching: u32,
    /// Matching areas that are acknowledged; they rank after the rest.
    pub acknowledged: u32,
    pub areas: Vec<Area>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct Dimension {
    pub name: String,
    pub description: String,
    /// The values a filter accepts; empty for free text such as paths.
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct DetectorInfo {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct Catalog {
    pub dimensions: Vec<Dimension>,
    pub measures: Vec<MeasureDef>,
    pub detectors: Vec<DetectorInfo>,
}

/// The answer to a [`Query`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "Answer.ts")]
pub enum Answer {
    Overview(Overview),
    Tree(Tree),
    Areas(AreaList),
    Area(AreaDetail),
    Catalog(Catalog),
}

/// An answer with its schema name, as every surface prints it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "Answer.ts")]
pub struct Envelope {
    pub schema: String,
    #[serde(flatten)]
    pub answer: Answer,
}

impl From<Answer> for Envelope {
    fn from(answer: Answer) -> Self {
        Self {
            schema: SCHEMA.to_owned(),
            answer,
        }
    }
}

/// Answers a query.
///
/// # Errors
/// Fails when the index cannot be read, a measure is unknown, or a subject has no area.
pub fn answer(index: &Index, query: &Query) -> Result<Answer, QueryError> {
    match query {
        Query::Overview => overview::overview(index).map(Answer::Overview),
        Query::Tree {
            hierarchy,
            size,
            color,
            filter,
            depth,
        } => {
            let size = size
                .clone()
                .unwrap_or_else(|| MeasureRef::Named("lines".to_owned()));
            validate(filter)?;
            let size_measure = tree::resolve(&size)?;
            let color = color
                .as_ref()
                .map(|reference| {
                    Ok::<_, QueryError>((tree::resolve(reference)?, measure::label(reference)))
                })
                .transpose()?;
            let args = tree::Args {
                hierarchy: *hierarchy,
                size: &size_measure,
                size_label: measure::label(&size),
                color: color.as_ref().map(|(m, label)| (m, label.clone())),
                filter,
                depth: depth.unwrap_or(DEPTH_DEFAULT),
            };
            for measure in [Some(&size_measure), color.as_ref().map(|(m, _)| m)]
                .into_iter()
                .flatten()
            {
                measure
                    .terms()
                    .iter()
                    .try_for_each(|term| validate(&term.filter))?;
            }
            tree::tree(index, &args).map(Answer::Tree)
        }
        Query::Areas {
            top,
            path,
            detector,
        } => areas(index, *top, path.as_deref(), detector.as_deref()),
        Query::Area { subject } => area(index, subject).map(Answer::Area),
        Query::Catalog => Ok(Answer::Catalog(catalog())),
    }
}

/// Rejects a filter value no line can have, so a typo is an error and not an empty answer.
fn validate(filter: &Filter) -> Result<(), QueryError> {
    let catalog = catalog();
    let given = [
        ("language", &filter.language),
        ("role", &filter.role),
        ("concern", &filter.concern),
        ("symbolKind", &filter.symbol_kind),
    ];
    let excluded = filter.exclude_roles.iter().map(|role| ("role", role));
    for (dimension, value) in given
        .into_iter()
        .filter_map(|(dimension, value)| Some((dimension, value.as_ref()?)))
        .chain(excluded)
    {
        let known = catalog
            .dimensions
            .iter()
            .any(|d| d.name == dimension && d.values.contains(value));
        if !known {
            return Err(QueryError::UnknownValue {
                dimension,
                value: value.clone(),
            });
        }
    }
    Ok(())
}

fn under(path: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_matches('/');
    prefix.is_empty()
        || path == prefix
        || (path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/'))
}

fn areas(
    index: &Index,
    top: Option<u32>,
    path: Option<&str>,
    detector: Option<&str>,
) -> Result<Answer, QueryError> {
    let matching: Vec<Area> = index
        .areas()?
        .into_iter()
        .filter(|area| path.is_none_or(|prefix| under(&area.subject, prefix)))
        .filter(|area| detector.is_none_or(|name| area.detectors.iter().any(|d| d == name)))
        .collect();
    let acknowledged = matching.iter().filter(|a| a.acknowledged.is_some()).count();
    let shown = top.map_or(matching.len(), |n| n as usize);
    Ok(Answer::Areas(AreaList {
        matching: u32::try_from(matching.len()).unwrap_or(u32::MAX),
        acknowledged: u32::try_from(acknowledged).unwrap_or(u32::MAX),
        areas: matching.into_iter().take(shown).collect(),
    }))
}

fn area(index: &Index, subject: &str) -> Result<AreaDetail, QueryError> {
    let path = subject
        .split_once("::")
        .map_or(subject, |(path, _)| path)
        .trim_matches('/');
    let all = index.areas()?;
    let found = all
        .iter()
        .find(|area| area.subject == path)
        .ok_or_else(|| QueryError::UnknownArea(subject.to_owned()))?
        .clone();
    let related = all
        .into_iter()
        .filter(|other| {
            other.subject != found.subject
                && (under(&other.subject, &found.subject) || under(&found.subject, &other.subject))
        })
        .collect();
    Ok(AreaDetail {
        findings: index.findings_of(&found.subject)?,
        area: found,
        related,
    })
}

fn catalog() -> Catalog {
    let names = |values: Vec<&str>| values.into_iter().map(str::to_owned).collect();
    let mut languages: Vec<&str> = LANGUAGES.iter().map(|language| language.name).collect();
    languages.push(measure::OTHER_LANGUAGE);
    Catalog {
        dimensions: vec![
            Dimension {
                name: "path".to_owned(),
                description:
                    "A file or directory relative to the root; matches everything under it"
                        .to_owned(),
                values: Vec::new(),
            },
            Dimension {
                name: "language".to_owned(),
                description: "The language of the file; `other` is every file without a grammar"
                    .to_owned(),
                values: names(languages),
            },
            Dimension {
                name: "role".to_owned(),
                description: "What the line is for".to_owned(),
                values: names(Role::ALL.iter().map(|r| r.as_str()).collect()),
            },
            Dimension {
                name: "concern".to_owned(),
                description: "What the line does".to_owned(),
                values: names(Concern::ALL.iter().map(|c| c.as_str()).collect()),
            },
            Dimension {
                name: "symbolKind".to_owned(),
                description: "The kind of the innermost symbol the line sits in".to_owned(),
                values: names(vec!["module", "type", "function", "test"]),
            },
        ],
        measures: measure::catalog(),
        detectors: DETECTORS
            .iter()
            .map(|d| DetectorInfo {
                name: d.name().to_owned(),
                description: d.describe().to_owned(),
            })
            .collect(),
    }
}

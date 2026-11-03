//! The query layer: one [`Query`] in, one [`Answer`] out, in a versioned [`Envelope`]. Every
//! surface (CLI, HTTP, later MCP) is a shell over [`answer`]; none computes a number of its own.

mod overview;
mod scale;
mod source;
mod tree;
mod xtab;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::detect::{
    Area, DETECTORS, Finding, GROUP_ID_PREFIX, SubjectKind, distinct_lines_by_path,
};
use crate::measure::{self, Base, Filter, Measure, MeasureDef, MeasureRef, Term};
use crate::reader::{Index, QueryError};
use codemap_lang::{Concern, LANGUAGES, Role};

pub use overview::{AreaCounts, DetectorCount, Overview, Share, Totals};
pub use scale::{SCALE_STEPS, Scale, Spacing};
pub use source::{SOURCE_LINES_MAX, Source, SourceFinding, SourceLine};
pub use tree::{DEPTH_DEFAULT, Hierarchy, RatioParts, SizeInfo, Tree, TreeNode};
pub use xtab::{Axis, PATH_DEPTH_DEFAULT, Xtab, XtabCell, XtabHeader};

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
    /// Two dimensions crossed, one measure per cell, under filters.
    Xtab {
        rows: Axis,
        cols: Axis,
        /// A sum; `lines` when absent.
        #[serde(default)]
        measure: Option<MeasureRef>,
        #[serde(default)]
        filter: Filter,
        /// Path components a path axis keeps below the filter's path. [`PATH_DEPTH_DEFAULT`]
        /// when absent.
        #[serde(default)]
        path_depth: Option<u32>,
    },
    /// Ranked areas, best first.
    Areas {
        /// How many to list; all when absent.
        #[serde(default)]
        top: Option<u32>,
        /// Only areas this detector found something in.
        #[serde(default)]
        detector: Option<String>,
        /// Only areas at or under `path` whose findings touch lines that pass the rest of the
        /// filter (language, role, concern, symbol kind). Generated and vendored lines count
        /// only when named; `include_generated` has no effect here.
        #[serde(default)]
        filter: Filter,
    },
    /// One area: its findings, measures, evidence and related areas. For a file or directory
    /// also the groups that touch it.
    Area {
        /// A file or directory path (a `path::function` subject names its file), or a group's
        /// id.
        subject: String,
        /// Narrows the lines `touching` counts, as it narrows a map's colour: the population of
        /// the filter, generated and vendored lines left out unless it includes them. Its path
        /// is ignored; the subject says where.
        #[serde(default)]
        filter: Option<Filter>,
    },
    /// One file's lines with their role and concern, and the finding ranges that touch them.
    Source {
        /// Relative to the root.
        path: String,
        /// First line to return, 1-based; the first line when absent.
        #[serde(default)]
        start_line: Option<u32>,
        /// Last line to return, inclusive; the last line when absent.
        #[serde(default)]
        end_line: Option<u32>,
    },
    /// Dimensions, measures and detectors available.
    Catalog,
}

/// One area with everything detectors said about it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct AreaDetail {
    /// Absent when a path has no area of its own and is only part of groups.
    pub area: Option<Area>,
    pub findings: Vec<Finding>,
    /// Areas above or below this one in the tree; for a group, the areas of the files it spans.
    pub related: Vec<Area>,
    /// For a file or directory: the groups with a member in it, best first.
    pub groups: Vec<GroupShare>,
    /// For a file or directory: every open area with evidence inside it, most lines first.
    pub touching: Vec<AreaShare>,
}

/// An open area and the lines of its evidence that lie inside a location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct AreaShare {
    pub area: Area,
    /// The area's evidence lines inside the location, each counted once.
    pub lines: u32,
}

/// A group touching a location, and how much of the group lies there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct GroupShare {
    pub group: Area,
    /// The group's evidence lines inside the location.
    pub lines: u32,
    /// Those lines as a share of all the group's evidence lines, 0 to 1.
    pub share: f64,
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
    Xtab(Xtab),
    Areas(AreaList),
    Area(AreaDetail),
    Source(Source),
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
        } => tree_answer(
            index,
            *hierarchy,
            size.as_ref(),
            color.as_ref(),
            filter,
            *depth,
        ),
        Query::Xtab {
            rows,
            cols,
            measure,
            filter,
            path_depth,
        } => xtab_answer(index, (*rows, *cols), measure.as_ref(), filter, *path_depth),
        Query::Source {
            path,
            start_line,
            end_line,
        } => source::source(index, path, *start_line, *end_line).map(Answer::Source),
        Query::Areas {
            top,
            detector,
            filter,
        } => areas(index, *top, detector.as_deref(), filter),
        Query::Area { subject, filter } => {
            area(index, subject, &filter.clone().unwrap_or_default()).map(Answer::Area)
        }
        Query::Catalog => Ok(Answer::Catalog(catalog())),
    }
}

fn tree_answer(
    index: &Index,
    hierarchy: Hierarchy,
    size: Option<&MeasureRef>,
    color: Option<&MeasureRef>,
    filter: &Filter,
    depth: Option<u32>,
) -> Result<Answer, QueryError> {
    let size = size
        .cloned()
        .unwrap_or_else(|| MeasureRef::Named("lines".to_owned()));
    validate(filter)?;
    let size_measure = tree::resolve(&size)?;
    let color = color
        .map(|reference| {
            Ok::<_, QueryError>((tree::resolve(reference)?, measure::label(reference)))
        })
        .transpose()?;
    validate_terms(&size_measure)?;
    if let Some((measure, _)) = &color {
        validate_terms(measure)?;
    }
    let args = tree::Args {
        hierarchy,
        size: &size_measure,
        size_label: measure::label(&size),
        color: color.as_ref().map(|(m, label)| (m, label.clone())),
        filter,
        depth: depth.unwrap_or(DEPTH_DEFAULT),
    };
    let mut tree = tree::tree(index, &args)?;
    tree.hidden = hidden(index, filter)?;
    Ok(Answer::Tree(tree))
}

fn xtab_answer(
    index: &Index,
    (rows, cols): (Axis, Axis),
    measure: Option<&MeasureRef>,
    filter: &Filter,
    path_depth: Option<u32>,
) -> Result<Answer, QueryError> {
    let measure = measure
        .cloned()
        .unwrap_or_else(|| MeasureRef::Named("lines".to_owned()));
    validate(filter)?;
    let resolved = tree::resolve(&measure)?;
    validate_terms(&resolved)?;
    let args = xtab::Args {
        row_axis: rows,
        col_axis: cols,
        measure: &resolved,
        label: measure::label(&measure),
        filter,
        path_depth: path_depth.unwrap_or(PATH_DEPTH_DEFAULT),
    };
    let mut xtab = xtab::xtab(index, &args)?;
    xtab.hidden = hidden(index, filter)?;
    Ok(Answer::Xtab(xtab))
}

/// What the default population left out of an answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct Hidden {
    /// The roles left out.
    pub roles: Vec<String>,
    /// Lines of those roles that match the rest of the filter.
    pub lines: u32,
}

/// The lines the filter's default population leaves out; `None` when it left none out.
fn hidden(index: &Index, filter: &Filter) -> Result<Option<Hidden>, QueryError> {
    let roles = filter.hidden_roles();
    if roles.is_empty() {
        return Ok(None);
    }
    let terms: Vec<Term> = roles
        .iter()
        .map(|role| {
            Term::new(
                Base::Lines,
                Filter {
                    role: Some((*role).to_owned()),
                    ..Filter::default()
                },
            )
        })
        .collect();
    let widened = Filter {
        include_generated: true,
        ..filter.clone()
    };
    let lines: f64 = index
        .grouped(&[], &widened, &terms)?
        .into_iter()
        .flat_map(|(_, sums)| sums)
        .sum();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "sums of line counts are whole and far below 2^53"
    )]
    let lines_whole = lines as u32;
    Ok((lines > 0.0).then(|| Hidden {
        roles: roles.iter().map(|role| (*role).to_owned()).collect(),
        lines: lines_whole,
    }))
}

/// Rejects a filter value in any term of the measure that no line can have.
fn validate_terms(measure: &Measure) -> Result<(), QueryError> {
    measure
        .terms()
        .iter()
        .try_for_each(|term| validate(&term.filter))
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
    for (dimension, value) in given
        .into_iter()
        .filter_map(|(dimension, value)| Some((dimension, value.as_ref()?)))
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
    detector: Option<&str>,
    filter: &Filter,
) -> Result<Answer, QueryError> {
    validate(filter)?;
    let path = filter
        .path
        .as_deref()
        .filter(|p| !p.trim_matches('/').is_empty());
    let touched = lines_filtered(filter)
        .then(|| index.touched_by(filter))
        .transpose()?;
    // A location area is found by its subject, a group area by its members.
    let mut findings: BTreeMap<String, Vec<Finding>> = BTreeMap::new();
    if touched.is_some() || path.is_some() {
        for finding in index.all_findings()? {
            findings
                .entry(finding.area.clone())
                .or_default()
                .push(finding);
        }
    }
    let own = |area: &Area| findings.get(&area.subject).map_or(&[][..], Vec::as_slice);
    let matching: Vec<Area> = index
        .areas()?
        .into_iter()
        .filter(|area| {
            path.is_none_or(|prefix| match area.subject_kind {
                SubjectKind::Group => own(area)
                    .iter()
                    .any(|f| f.evidence.iter().any(|e| under(&e.path, prefix))),
                _ => under(&area.subject, prefix),
            })
        })
        .filter(|area| detector.is_none_or(|name| area.detectors.iter().any(|d| d == name)))
        .filter(|area| {
            touched
                .as_ref()
                .is_none_or(|touched| own(area).iter().any(|finding| touched.touches(finding)))
        })
        .collect();
    let acknowledged = matching.iter().filter(|a| a.acknowledged.is_some()).count();
    let shown = top.map_or(matching.len(), |n| n as usize);
    Ok(Answer::Areas(AreaList {
        matching: u32::try_from(matching.len()).unwrap_or(u32::MAX),
        acknowledged: u32::try_from(acknowledged).unwrap_or(u32::MAX),
        areas: matching.into_iter().take(shown).collect(),
    }))
}

/// Whether the filter narrows lines, beyond the path that narrows areas themselves.
fn lines_filtered(filter: &Filter) -> bool {
    filter.language.is_some()
        || filter.role.is_some()
        || filter.concern.is_some()
        || filter.symbol_kind.is_some()
}

fn area(index: &Index, subject: &str, filter: &Filter) -> Result<AreaDetail, QueryError> {
    let all = index.areas()?;
    if subject.starts_with(GROUP_ID_PREFIX) {
        return group_detail(index, subject, all);
    }
    let path = subject
        .split_once("::")
        .map_or(subject, |(path, _)| path)
        .trim_matches('/');
    let found = all.iter().find(|area| area.subject == path).cloned();
    let groups = groups_touching(index, path, &all)?;
    let touching = areas_touching(index, path, filter, &all)?;
    if found.is_none() && groups.is_empty() && touching.is_empty() {
        return Err(QueryError::UnknownArea(subject.to_owned()));
    }
    let related = all
        .into_iter()
        .filter(|other| {
            other.subject_kind != SubjectKind::Group
                && other.subject != path
                && (under(&other.subject, path) || under(path, &other.subject))
        })
        .collect();
    Ok(AreaDetail {
        findings: index.findings_of(path)?,
        area: found,
        related,
        groups,
        touching,
    })
}

fn group_detail(index: &Index, id: &str, all: Vec<Area>) -> Result<AreaDetail, QueryError> {
    let found = all
        .iter()
        .find(|area| area.subject == id)
        .ok_or_else(|| QueryError::UnknownArea(id.to_owned()))?
        .clone();
    let findings = index.findings_of(id)?;
    let related = all
        .into_iter()
        .filter(|other| {
            other.subject_kind != SubjectKind::Group
                && findings
                    .iter()
                    .flat_map(|f| &f.evidence)
                    .any(|e| under(&e.path, &other.subject))
        })
        .collect();
    Ok(AreaDetail {
        area: Some(found),
        findings,
        related,
        groups: Vec::new(),
        touching: Vec::new(),
    })
}

/// The groups with evidence inside `path`, and how much of each lies there.
fn groups_touching(index: &Index, path: &str, all: &[Area]) -> Result<Vec<GroupShare>, QueryError> {
    let ranges = |finding: &Finding| -> Vec<(String, u32, u32)> {
        finding
            .evidence
            .iter()
            .map(|e| (e.path.clone(), e.start_line, e.end_line))
            .collect()
    };
    let lines = |ranges: &[(String, u32, u32)], inside: bool| -> u32 {
        distinct_lines_by_path(
            ranges
                .iter()
                .filter(|(p, _, _)| !inside || under(p, path))
                .map(|(p, start, end)| (p.as_str(), *start, *end)),
        )
        .values()
        .sum()
    };
    let mut shares = Vec::new();
    for finding in index.group_findings()? {
        let ranges = ranges(&finding);
        let (here, total) = (lines(&ranges, true), lines(&ranges, false));
        let group = all.iter().find(|area| area.subject == finding.area);
        if let (Some(group), true) = (group, here > 0) {
            shares.push(GroupShare {
                group: group.clone(),
                lines: here,
                share: f64::from(here) / f64::from(total.max(1)),
            });
        }
    }
    shares.sort_by_key(|share| share.group.rank);
    Ok(shares)
}

/// The open areas with evidence inside `path` on lines that pass `filter`, and how many distinct
/// lines of it lie there: the areas whose findings `finding_lines` counts on the map.
fn areas_touching(
    index: &Index,
    path: &str,
    filter: &Filter,
    all: &[Area],
) -> Result<Vec<AreaShare>, QueryError> {
    validate(filter)?;
    let flagged = index.flagged_lines(&Filter {
        path: Some(path.to_owned()),
        ..filter.clone()
    })?;
    let mut inside: BTreeMap<String, BTreeSet<(&str, u32)>> = BTreeMap::new();
    let findings = index.all_findings()?;
    for finding in findings.iter().filter(|f| f.acknowledged.is_none()) {
        if finding.evidence.is_empty() {
            // No evidence: everything under its area.
            let area = finding.area.trim_end_matches('/');
            let prefix = format!("{area}/");
            for (file, lines) in &flagged {
                if file == area || file.starts_with(&prefix) {
                    inside
                        .entry(finding.area.clone())
                        .or_default()
                        .extend(lines.iter().map(|line| (file.as_str(), *line)));
                }
            }
        }
        for e in &finding.evidence {
            let Some(lines) = flagged.get(&e.path) else {
                continue;
            };
            inside.entry(finding.area.clone()).or_default().extend(
                lines
                    .range(e.start_line..=e.end_line)
                    .map(|line| (e.path.as_str(), *line)),
            );
        }
    }
    let mut shares: Vec<AreaShare> = all
        .iter()
        .filter_map(|area| {
            let lines = u32::try_from(inside.get(&area.subject)?.len()).unwrap_or(u32::MAX);
            (lines > 0).then(|| AreaShare {
                area: area.clone(),
                lines,
            })
        })
        .collect();
    shares.sort_by(|a, b| b.lines.cmp(&a.lines).then(a.area.rank.cmp(&b.area.rank)));
    Ok(shares)
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
                values: names(vec![
                    "module",
                    "type",
                    "function",
                    "test",
                    measure::NO_SYMBOL,
                ]),
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

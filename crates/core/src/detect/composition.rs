//! Mass and composition: files and directories whose share of a concern or role is far above the
//! codebase's own share, weighted by mass.
//!
//! A subject is flagged when its share is at least [`SHARE_FACTOR`] times the codebase share and
//! [`SHARE_MARGIN`] above it, and the lines beyond the codebase share (the excess) number at least
//! [`MIN_EXCESS_LINES`] and a thousandth of the codebase. The excess, not the share, is the mass,
//! so a tiny file with a high share cannot dominate. A directory whose excess is mostly one
//! flagged descendant's is dropped in favour of the descendant. Roles that define their subject
//! (a tests directory is all tests) are not flagged. A concern is compared within the role where
//! it normally lives (handling, comments and logging in production code, assertions in tests),
//! so a test directory is not flagged for asserting. Configuration is not examined: it is data.

use codemap_lang::{Concern, Role};

use crate::role::role_for_path;

use super::corpus::Agg;
use super::{Context, Detector, Finding, SubjectKind, count, percent};
use crate::reader::QueryError;

const SHARE_FACTOR: f64 = 2.0;
const SHARE_MARGIN: f64 = 0.10;
const MIN_EXCESS_LINES: f64 = 50.0;
const MIN_EXCESS_FRACTION: f64 = 0.001;
/// Past this share a role is the subject's definition, not a finding about it.
const DEFINING_SHARE: f64 = 0.95;
/// A descendant that holds this much of an ancestor's excess replaces the ancestor.
const DESCENDANT_EXPLAINS: f64 = 0.8;

pub struct Composition;

#[derive(Clone, Copy)]
enum Dimension {
    /// A concern's share of the lines of one role.
    Concern(Concern, Role),
    /// A role's share of all lines.
    Role(Role),
}

struct Category {
    dimension: Dimension,
    /// How the lines are called in a sentence.
    words: &'static str,
}

const CATEGORIES: [Category; 6] = [
    Category {
        dimension: Dimension::Concern(Concern::Handler, Role::Prod),
        words: "error handling",
    },
    Category {
        dimension: Dimension::Concern(Concern::Comment, Role::Prod),
        words: "comments",
    },
    Category {
        dimension: Dimension::Concern(Concern::Assertion, Role::Test),
        words: "assertions",
    },
    Category {
        dimension: Dimension::Concern(Concern::Logging, Role::Prod),
        words: "logging",
    },
    Category {
        dimension: Dimension::Role(Role::Test),
        words: "test code",
    },
    Category {
        dimension: Dimension::Role(Role::Docs),
        words: "documentation",
    },
];

impl Dimension {
    /// The lines in the category and the lines it is a share of.
    fn parts(self, agg: &Agg) -> (f64, f64) {
        match self {
            Self::Concern(concern, role) => (agg.cell(role, concern), agg.role(role)),
            Self::Role(role) => (agg.role(role), agg.lines),
        }
    }

    fn column_value(self) -> (&'static str, &'static str) {
        match self {
            Self::Concern(concern, _) => ("concern", concern.as_str()),
            Self::Role(role) => ("role", role.as_str()),
        }
    }
}

struct Hit<'a> {
    path: &'a str,
    is_dir: bool,
    agg: &'a Agg,
    share: f64,
    /// The lines the share is of.
    of: f64,
    excess: f64,
}

impl Detector for Composition {
    fn name(&self) -> &'static str {
        "composition"
    }

    fn describe(&self) -> &'static str {
        "Files and directories where a concern or role takes far more of the lines than it does across the codebase"
    }

    fn detect(&self, context: &Context) -> Result<Vec<Finding>, QueryError> {
        let corpus = context.corpus;
        let mut findings = Vec::new();
        if corpus.total.lines <= 0.0 {
            return Ok(findings);
        }
        for category in &CATEGORIES {
            let (category_total, stratum_total) = category.dimension.parts(&corpus.total);
            if stratum_total <= 0.0 {
                continue;
            }
            let baseline = category_total / stratum_total;
            let subjects = corpus
                .dirs
                .iter()
                .map(|(path, agg)| (path.as_str(), true, agg))
                .chain(
                    corpus
                        .files
                        .iter()
                        .map(|(path, agg)| (path.as_str(), false, agg)),
                );
            let hits: Vec<Hit> = subjects
                .filter_map(|(path, is_dir, agg)| {
                    hit(context, category, baseline, path, is_dir, agg)
                })
                .collect();
            for hit in &hits {
                if hits.iter().any(|other| explains(other, hit)) {
                    continue;
                }
                findings.push(finding(context, category, baseline, hit)?);
            }
        }
        Ok(findings)
    }
}

fn hit<'a>(
    context: &Context,
    category: &Category,
    baseline: f64,
    path: &'a str,
    is_dir: bool,
    agg: &'a Agg,
) -> Option<Hit<'a>> {
    let (lines, of) = category.dimension.parts(agg);
    if of <= 0.0 {
        return None;
    }
    let share = lines / of;
    let excess = lines - baseline * of;
    let flagged = share >= SHARE_FACTOR * baseline
        && share >= baseline + SHARE_MARGIN
        && excess >= MIN_EXCESS_LINES.max(MIN_EXCESS_FRACTION * context.corpus.total.lines)
        && !defines(context, category, path, is_dir, share);
    flagged.then_some(Hit {
        path,
        is_dir,
        agg,
        share,
        of,
        excess,
    })
}

/// A role that is the reason the subject exists says nothing about it: a tests directory is
/// tests, a docs directory is docs.
fn defines(context: &Context, category: &Category, path: &str, is_dir: bool, share: f64) -> bool {
    let Dimension::Role(role) = category.dimension else {
        return false;
    };
    let by_path = if is_dir {
        role_for_path(&format!("{path}/_"), None, "") == role
    } else {
        context.corpus.file_roles.get(path) == Some(&role)
    };
    share >= DEFINING_SHARE || by_path
}

/// `inner` lies under `outer` and carries most of its excess.
fn explains(inner: &Hit, outer: &Hit) -> bool {
    outer.is_dir
        && inner.path.len() > outer.path.len()
        && inner.path.starts_with(outer.path)
        && inner.path.as_bytes()[outer.path.len()] == b'/'
        && inner.excess >= DESCENDANT_EXPLAINS * outer.excess
}

fn finding(
    context: &Context,
    category: &Category,
    baseline: f64,
    hit: &Hit,
) -> Result<Finding, QueryError> {
    let (category_lines, _) = category.dimension.parts(hit.agg);
    let what = if hit.is_dir { "directory" } else { "file" };
    let reason = format!(
        "{} of the {} lines in {} are {}, against {} across the codebase.",
        percent(hit.share),
        count(hit.of),
        hit.path,
        category.words,
        percent(baseline),
    );
    let (column, value) = category.dimension.column_value();
    let evidence = if hit.is_dir {
        let prefix = format!("{}/", hit.path);
        let mut files: Vec<_> = context
            .corpus
            .files
            .iter()
            .filter(|(path, _)| path.starts_with(&prefix))
            .map(|(path, agg)| (category.dimension.parts(agg).0, path))
            .collect();
        files.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(b.1)));
        let mut runs = Vec::new();
        for (_, path) in files.into_iter().take(3) {
            runs.push((path.clone(), context.index.runs(path, column, value, 1)?));
        }
        runs
    } else {
        vec![(
            hit.path.to_owned(),
            context.index.runs(hit.path, column, value, 3)?,
        )]
    };
    let mut found = Finding::new(
        "composition",
        if hit.is_dir {
            SubjectKind::Directory
        } else {
            SubjectKind::File
        },
        hit.path,
        reason,
        hit.excess,
        1.0 - baseline / hit.share,
    )
    .measure("categoryLines", category_lines)
    .measure("lines", hit.of)
    .measure("share", hit.share)
    .measure("codebaseShare", baseline)
    .measure("excessLines", hit.excess);
    found
        .labels
        .insert("category".to_owned(), category.words.to_owned());
    found.labels.insert("subject".to_owned(), what.to_owned());
    for (path, runs) in evidence {
        found = found.evidence(&path, &runs);
    }
    Ok(found)
}

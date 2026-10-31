//! Mass and composition: files and directories whose share of a concern or role is far above the
//! codebase's own share, weighted by mass.
//!
//! Every measure declares its population, the role whose lines the share is of: handling,
//! comments and logging among production lines, assertions among test lines, test code and
//! documentation against production lines. A subject with no lines in the population has no
//! share, so a tests directory is not flagged for being tests.
//!
//! A subject is flagged when its share is at least [`SHARE_FACTOR`] times the codebase share and
//! [`SHARE_MARGIN`] above it, and the lines beyond the codebase share (the excess) number at least
//! [`MIN_EXCESS_LINES`] and a thousandth of the codebase. The excess, not the share, is the mass,
//! so a tiny file with a high share cannot dominate. A directory whose excess is mostly one
//! flagged descendant's is dropped in favour of the descendant.

use codemap_lang::{Concern, Role};

use super::corpus::Agg;
use super::{Context, Detector, Finding, SubjectKind, count, percent};
use crate::reader::QueryError;

const SHARE_FACTOR: f64 = 2.0;
const SHARE_MARGIN: f64 = 0.10;
const MIN_EXCESS_LINES: f64 = 50.0;
const MIN_EXCESS_FRACTION: f64 = 0.001;
/// A descendant that holds this much of an ancestor's excess replaces the ancestor.
const DESCENDANT_EXPLAINS: f64 = 0.8;

pub struct Composition;

/// A measure: the lines of a role (or of one concern within it) as a share of the lines of its
/// population role. The population is the only place roles enter this detector; a subject with
/// no lines in it has no measure.
struct Category {
    role: Role,
    concern: Option<Concern>,
    population: Role,
    /// How the lines are called in a sentence.
    words: &'static str,
}

const fn concern(concern: Concern, role: Role, words: &'static str) -> Category {
    Category {
        role,
        concern: Some(concern),
        population: role,
        words,
    }
}

const fn against_prod(role: Role, words: &'static str) -> Category {
    Category {
        role,
        concern: None,
        population: Role::Prod,
        words,
    }
}

const CATEGORIES: [Category; 6] = [
    concern(Concern::Handler, Role::Prod, "error handling"),
    concern(Concern::Comment, Role::Prod, "comments"),
    concern(Concern::Assertion, Role::Test, "assertions"),
    concern(Concern::Logging, Role::Prod, "logging"),
    against_prod(Role::Test, "test code"),
    against_prod(Role::Docs, "documentation"),
];

impl Category {
    /// The lines in the category and the lines of its population.
    fn parts(&self, agg: &Agg) -> (f64, f64) {
        let lines = self
            .concern
            .map_or_else(|| agg.role(self.role), |c| agg.cell(self.role, c));
        (lines, agg.role(self.population))
    }

    /// What the category's share is of, in a sentence.
    fn share_fact(&self, lines: f64, of: f64, share: f64) -> String {
        let population = population_word(self.population);
        if self.concern.is_some() {
            format!(
                "{} of {} {population} lines are {} ({})",
                count(lines),
                count(of),
                self.words,
                percent(share),
            )
        } else {
            format!(
                "{} lines of {} against {} {population} lines ({})",
                count(lines),
                self.words,
                count(of),
                percent(share),
            )
        }
    }

    fn column_value(&self) -> (&'static str, &'static str) {
        self.concern
            .map_or(("role", self.role.as_str()), |c| ("concern", c.as_str()))
    }
}

/// A role as an adjective for lines.
fn population_word(role: Role) -> &'static str {
    match role {
        Role::Prod => "production",
        other => other.as_str(),
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
            let (category_total, stratum_total) = category.parts(&corpus.total);
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
    let (lines, of) = category.parts(agg);
    if of <= 0.0 {
        return None;
    }
    let share = lines / of;
    let excess = lines - baseline * of;
    let flagged = share >= SHARE_FACTOR * baseline
        && share >= baseline + SHARE_MARGIN
        && excess >= MIN_EXCESS_LINES.max(MIN_EXCESS_FRACTION * context.corpus.total.lines);
    flagged.then_some(Hit {
        path,
        is_dir,
        agg,
        share,
        of,
        excess,
    })
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
    let (lines, _) = category.parts(hit.agg);
    let what = if hit.is_dir { "directory" } else { "file" };
    let reason = format!(
        "{} has {} lines of {}, {} of its {} {} lines, against {} across the codebase.",
        hit.path,
        count(lines),
        category.words,
        percent(hit.share),
        count(hit.of),
        population_word(category.population),
        percent(baseline),
    );
    let (column, value) = category.column_value();
    let evidence = if hit.is_dir {
        let prefix = format!("{}/", hit.path);
        let mut files: Vec<_> = context
            .corpus
            .files
            .iter()
            .filter(|(path, _)| path.starts_with(&prefix))
            .map(|(path, agg)| (category.parts(agg).0, path))
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
    .measure("categoryLines", lines)
    .measure("lines", hit.of)
    .measure("share", hit.share)
    .measure("codebaseShare", baseline)
    .measure("excessLines", hit.excess)
    .fact(category.share_fact(lines, hit.of, hit.share))
    .fact(format!("{} across the codebase", percent(baseline)))
    .fact(format!(
        "{} lines more than the codebase's share",
        count(hit.excess)
    ));
    found
        .labels
        .insert("category".to_owned(), category.words.to_owned());
    found.labels.insert("subject".to_owned(), what.to_owned());
    for (path, runs) in evidence {
        found = found.evidence(&path, &runs);
    }
    Ok(found)
}

//! Mass and composition: files and directories whose share of a concern or role is unusual among
//! their peers, weighted by mass.
//!
//! Every measure declares its population, the role whose lines the share is of: handling,
//! comments and logging among production lines, assertions among test lines, test code and
//! documentation against production lines. A subject with no lines in the population has no
//! share, so a tests directory is not flagged for being tests.
//!
//! "Unusual" is judged against the distribution of the same measure across peers, never against
//! the codebase-wide share, which one big test-only folder or comment-heavy schema would drag for
//! everyone. Files are peers of files and directories of directories, among those with at least
//! [`PEER_MIN_LINES`] population lines (a share of a handful of lines means nothing), and only
//! when there are [`PEER_MIN_SAMPLE`] of them. When at least that many of them have some lines of
//! the category, the peers are those: most files have no inline tests and no logging, which says
//! nothing about how much a file that has some should have.
//!
//! A subject is flagged when its share is a robust outlier among its peers, its modified z-score
//! `0.6745 * (share - median) / MAD` reaching [`OUTLIER_Z`] (with the deviation floored at
//! [`SPREAD_FLOOR`]), and is at least [`SHARE_MARGIN`] above the peer median. The lines beyond
//! the median share (the excess) must number at least [`MIN_EXCESS_LINES`] and a thousandth of
//! the codebase. The excess, not the
//! share, is the mass, so a tiny file with a high share cannot dominate. A directory whose excess
//! is mostly one flagged descendant's is dropped in favour of the descendant.

use codemap_lang::{Concern, Role};

use super::corpus::Agg;
use super::robust::modified_z;
use super::{Context, Detector, Finding, SubjectKind, count, percent};
use crate::reader::QueryError;

const OUTLIER_Z: f64 = 3.5;
/// A floor under the median absolute deviation of shares, so a uniform codebase does not turn
/// small differences into outliers.
const SPREAD_FLOOR: f64 = 0.02;
const PEER_MIN_SAMPLE: usize = 20;
const PEER_MIN_LINES: f64 = 30.0;
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
    /// The lines in the category.
    lines: f64,
    /// The median share among the subject's peers.
    median: f64,
    z: f64,
    peers: f64,
    /// The lines the share is of.
    of: f64,
    excess: f64,
}

impl Detector for Composition {
    fn name(&self) -> &'static str {
        "composition"
    }

    fn describe(&self) -> &'static str {
        "Files and directories where a concern or role takes far more of the lines than it does in their peers"
    }

    fn detect(&self, context: &Context) -> Result<Vec<Finding>, QueryError> {
        let corpus = context.corpus;
        let mut findings = Vec::new();
        if corpus.total.lines <= 0.0 {
            return Ok(findings);
        }
        for category in &CATEGORIES {
            let dirs = corpus.dirs.iter().map(|(path, agg)| (path.as_str(), agg));
            let files = corpus.files.iter().map(|(path, agg)| (path.as_str(), agg));
            let mut hits = peer_outliers(context, category, dirs, true);
            hits.extend(peer_outliers(context, category, files, false));
            for hit in &hits {
                if hits.iter().any(|other| explains(other, hit)) {
                    continue;
                }
                findings.push(finding(context, category, hit)?);
            }
        }
        Ok(findings)
    }
}

/// The subjects of one kind whose share is an outlier among them.
fn peer_outliers<'a>(
    context: &Context,
    category: &Category,
    subjects: impl Iterator<Item = (&'a str, &'a Agg)>,
    is_dir: bool,
) -> Vec<Hit<'a>> {
    let peers: Vec<_> = subjects
        .filter_map(|(path, agg)| {
            let (lines, of) = category.parts(agg);
            (of >= PEER_MIN_LINES).then_some((path, agg, lines, of))
        })
        .collect();
    // Among those that have the category at all, when enough do: most files have no inline tests
    // and no logging, which says nothing about how much a file that has some should have.
    let having: Vec<_> = peers.iter().filter(|peer| peer.2 > 0.0).copied().collect();
    let peers = if having.len() >= PEER_MIN_SAMPLE {
        having
    } else {
        peers
    };
    if peers.len() < PEER_MIN_SAMPLE {
        return Vec::new();
    }
    let shares: Vec<f64> = peers.iter().map(|&(_, _, lines, of)| lines / of).collect();
    let (scores, median) = modified_z(&shares, SPREAD_FLOOR);
    let floor = MIN_EXCESS_LINES.max(MIN_EXCESS_FRACTION * context.corpus.total.lines);
    peers
        .iter()
        .zip(shares.iter().zip(scores))
        .filter_map(|(&(path, agg, lines, of), (&share, z))| {
            let excess = lines - median * of;
            (z >= OUTLIER_Z && share >= median + SHARE_MARGIN && excess >= floor).then_some(Hit {
                path,
                is_dir,
                agg,
                share,
                lines,
                median,
                z,
                peers: f64::from(u32::try_from(peers.len()).unwrap_or(u32::MAX)),
                of,
                excess,
            })
        })
        .collect()
}

/// `inner` lies under `outer` and carries most of its excess.
fn explains(inner: &Hit, outer: &Hit) -> bool {
    outer.is_dir
        && inner.path.len() > outer.path.len()
        && inner.path.starts_with(outer.path)
        && inner.path.as_bytes()[outer.path.len()] == b'/'
        // What the descendant holds beyond the ancestor's own median, as the ancestor's excess is.
        && inner.lines - outer.median * inner.of >= DESCENDANT_EXPLAINS * outer.excess
}

/// A file and the line runs of it that back a hit.
type Runs = (String, Vec<(u32, u32)>);

/// The runs of lines behind a hit: the biggest files of a directory, or the file's own.
fn evidence(context: &Context, category: &Category, hit: &Hit) -> Result<Vec<Runs>, QueryError> {
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
    Ok(evidence)
}

fn finding(context: &Context, category: &Category, hit: &Hit) -> Result<Finding, QueryError> {
    let baseline = hit.median;
    let (lines, _) = category.parts(hit.agg);
    let what = if hit.is_dir { "directory" } else { "file" };
    let reason = format!(
        "{} has {} lines of {}, {} of its {} {} lines, against a median of {} across {} {}.",
        hit.path,
        count(lines),
        category.words,
        percent(hit.share),
        count(hit.of),
        population_word(category.population),
        percent(baseline),
        count(hit.peers),
        if hit.is_dir { "directories" } else { "files" },
    );
    let evidence = evidence(context, category, hit)?;
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
    .measure("peerMedian", baseline)
    .measure("robustZ", hit.z)
    .measure("peers", hit.peers)
    .measure("excessLines", hit.excess)
    .fact(category.share_fact(lines, hit.of, hit.share))
    .fact(format!(
        "the median among {} {} is {}",
        count(hit.peers),
        if hit.is_dir { "directories" } else { "files" },
        percent(baseline)
    ))
    .fact(format!(
        "{} lines more than the median share",
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

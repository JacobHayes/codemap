//! Size and concentration: outlier files and functions against the codebase's own distribution,
//! and directories holding far more lines than their share of the files.
//!
//! Sizes are heavy-tailed, so outliers are judged on a log scale with robust statistics: a size
//! is an outlier when its modified z-score, `0.6745 * (log10(x) - median) / MAD`, reaches
//! [`OUTLIER_Z`] (the usual 3.5 cut for the median absolute deviation), it is above an absolute
//! floor, and the sample is big enough to have a distribution. The deviation is floored so a
//! uniform codebase does not turn small differences into outliers.
//!
//! The population is production and test code: a file is measured only if it has such lines, so
//! configuration, prose and the like are left out of both. Files and directories are sized
//! against peers of the same kind, production or test, since a suite of small fixtures says
//! nothing about how big source files should be.
//!
//! A directory is a concentration when its files are far larger than the files of its peer
//! directories: its mean file size is a robust outlier on the same log scale among directories of
//! at least [`CONCENTRATION_FILES_MIN`] files, and the lines beyond what its files would hold at
//! the peer median (the excess) are a real mass. Judging directories against directories, not
//! their share of the codebase's files, keeps one folder of tiny fixtures from making every
//! ordinary directory look heavy. A directory whose excess is mostly one flagged descendant's, or
//! mostly one file's (that file stands as its own finding), is dropped.

use std::collections::{BTreeMap, BTreeSet};

use codemap_lang::Role;

use super::corpus::{Agg, FunctionStat, ancestors};
use super::robust::modified_z;
use super::{Context, Detector, Finding, SubjectKind, count};
use crate::reader::QueryError;

const OUTLIER_Z: f64 = 3.5;
/// In log10 units: a floor under the deviation, about 12% size differences.
const MAD_FLOOR: f64 = 0.05;
const FILES_SAMPLE_MIN: usize = 20;
const FUNCTIONS_SAMPLE_MIN: usize = 30;
const FILE_LINES_MIN: f64 = 150.0;
const FILE_TOKENS_MIN: f64 = 1500.0;
const FUNCTION_LINES_MIN: f64 = 60.0;
const FUNCTION_TOKENS_MIN: f64 = 600.0;
const CONCENTRATION_FILES_MIN: f64 = 3.0;
const DIRECTORIES_SAMPLE_MIN: usize = 20;
const CONCENTRATION_EXCESS_MIN: f64 = 200.0;
const CONCENTRATION_EXCESS_FRACTION: f64 = 0.02;
/// A descendant that holds this much of an ancestor's excess replaces the ancestor.
const DESCENDANT_EXPLAINS: f64 = 0.8;

pub struct Size;

/// A line count as a line number.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "sums of line counts"
)]
fn whole_lines(lines: f64) -> u32 {
    lines as u32
}

/// Robust z-scores on a log scale; empty when the sample is too small.
fn robust_z(values: &[f64], sample_min: usize) -> (Vec<f64>, f64) {
    if values.len() < sample_min {
        return (Vec::new(), 0.0);
    }
    let logs: Vec<f64> = values.iter().map(|v| v.max(1.0).log10()).collect();
    let (scores, centre) = modified_z(&logs, MAD_FLOOR);
    (scores, 10_f64.powf(centre))
}

fn population_word(role: Role) -> &'static str {
    if role == Role::Test {
        "test"
    } else {
        "production"
    }
}

struct Sized {
    lines: f64,
    tokens: f64,
}

/// Indexes of the outliers among `sizes`, with each one's z-scores for lines and tokens.
fn outliers(
    sizes: &[Sized],
    sample_min: usize,
    lines_min: f64,
    tokens_min: f64,
) -> Vec<(usize, f64, f64)> {
    let lines: Vec<f64> = sizes.iter().map(|s| s.lines).collect();
    let tokens: Vec<f64> = sizes.iter().map(|s| s.tokens).collect();
    let (z_lines, _) = robust_z(&lines, sample_min);
    let (z_tokens, _) = robust_z(&tokens, sample_min);
    (0..sizes.len())
        .filter_map(|i| {
            let (zl, zt) = (*z_lines.get(i)?, *z_tokens.get(i)?);
            let by_lines = zl >= OUTLIER_Z && sizes[i].lines >= lines_min;
            let by_tokens = zt >= OUTLIER_Z && sizes[i].tokens >= tokens_min;
            (by_lines || by_tokens).then_some((i, zl, zt))
        })
        .collect()
}

impl Detector for Size {
    fn name(&self) -> &'static str {
        "size"
    }

    fn describe(&self) -> &'static str {
        "Files and functions far larger than the rest, and directories holding more lines than their share of the files"
    }

    fn detect(&self, context: &Context) -> Result<Vec<Finding>, QueryError> {
        let mut findings = files(context);
        findings.extend(functions(context));
        findings.extend(concentrations(context));
        Ok(findings)
    }
}

/// The population: files with production or test lines.
fn code_files<'a>(context: &'a Context) -> impl Iterator<Item = (&'a String, &'a Agg)> {
    context
        .corpus
        .files
        .iter()
        .filter(|(_, agg)| agg.role(Role::Prod) + agg.role(Role::Test) > 0.0)
}

/// Production and test code are sized separately: a test suite is not a peer of the code it tests.
const STRATA: [Role; 2] = [Role::Prod, Role::Test];

/// Code that is nearly all test is test code; a file with real production lines and tests beside
/// them is production code.
const TEST_STRATUM_PROD_MAX: f64 = 0.1;

fn stratum(prod: f64, test: f64) -> Role {
    if prod < TEST_STRATUM_PROD_MAX * (prod + test) {
        Role::Test
    } else {
        Role::Prod
    }
}

fn files(context: &Context) -> Vec<Finding> {
    STRATA
        .into_iter()
        .flat_map(|role| files_of(context, role))
        .collect()
}

fn files_of(context: &Context, role: Role) -> Vec<Finding> {
    let entries: Vec<_> = code_files(context)
        .filter(|(_, agg)| stratum(agg.role(Role::Prod), agg.role(Role::Test)) == role)
        .collect();
    let sizes: Vec<Sized> = entries
        .iter()
        .map(|(_, agg)| Sized {
            lines: agg.lines,
            tokens: agg.tokens,
        })
        .collect();
    let (_, median_lines) = robust_z(
        &sizes.iter().map(|s| s.lines).collect::<Vec<_>>(),
        FILES_SAMPLE_MIN,
    );
    let mut findings = Vec::new();
    for (i, z_lines, z_tokens) in
        outliers(&sizes, FILES_SAMPLE_MIN, FILE_LINES_MIN, FILE_TOKENS_MIN)
    {
        let (path, agg) = entries[i];
        let reason = format!(
            "{} is {} lines and {} tokens; the typical {} file here is about {} lines.",
            path,
            count(agg.lines),
            count(agg.tokens),
            population_word(role),
            count(median_lines),
        );
        let z = z_lines.max(z_tokens);
        findings.push(
            Finding::new(
                "size",
                SubjectKind::File,
                path,
                reason,
                agg.lines,
                z / (z + OUTLIER_Z),
            )
            .measure("lines", agg.lines)
            .measure("tokens", agg.tokens)
            .measure("typicalLines", median_lines)
            .measure("robustZ", z)
            .fact(format!(
                "{} lines and {} tokens",
                count(agg.lines),
                count(agg.tokens)
            ))
            .fact(format!(
                "the typical {} file is about {} lines",
                population_word(role),
                count(median_lines)
            ))
            .evidence(path, &[(1, whole_lines(agg.lines))]),
        );
    }
    findings
}

fn functions(context: &Context) -> Vec<Finding> {
    let stats: &[FunctionStat] = &context.corpus.functions;
    let sizes: Vec<Sized> = stats
        .iter()
        .map(|f| Sized {
            lines: f.lines,
            tokens: f.tokens,
        })
        .collect();
    let (_, median_lines) = robust_z(
        &sizes.iter().map(|s| s.lines).collect::<Vec<_>>(),
        FUNCTIONS_SAMPLE_MIN,
    );
    let mut findings = Vec::new();
    for (i, z_lines, z_tokens) in outliers(
        &sizes,
        FUNCTIONS_SAMPLE_MIN,
        FUNCTION_LINES_MIN,
        FUNCTION_TOKENS_MIN,
    ) {
        let stat = &stats[i];
        let symbol = &stat.symbol;
        let reason = format!(
            "{} in {} is {} lines and {} tokens; the typical function here is about {} lines.",
            stat.qualified,
            symbol.path,
            count(stat.lines),
            count(stat.tokens),
            count(median_lines),
        );
        let z = z_lines.max(z_tokens);
        let subject = format!("{}::{}", symbol.path, stat.qualified);
        findings.push(
            Finding::new(
                "size",
                SubjectKind::Function,
                &subject,
                reason,
                stat.lines,
                z / (z + OUTLIER_Z),
            )
            .measure("lines", stat.lines)
            .measure("tokens", stat.tokens)
            .measure("typicalLines", median_lines)
            .measure("robustZ", z)
            .fact(format!(
                "{} lines and {} tokens",
                count(stat.lines),
                count(stat.tokens)
            ))
            .fact(format!(
                "the typical function is about {} lines",
                count(median_lines)
            ))
            .evidence(&symbol.path, &[(symbol.start_line, symbol.end_line)]),
        );
    }
    findings
}

/// Files and lines of a directory's code files.
#[derive(Default)]
struct Mass<'a> {
    files: f64,
    lines: f64,
    /// The directory's biggest file and its lines.
    largest_path: &'a str,
    largest: f64,
    test_lines: f64,
}

fn concentrations(context: &Context) -> Vec<Finding> {
    let mut dirs: BTreeMap<&str, Mass> = BTreeMap::new();
    let mut total = Mass::default();
    for (path, agg) in code_files(context) {
        total.files += 1.0;
        total.lines += agg.lines;
        for dir in ancestors(path) {
            let mass = dirs.entry(dir).or_default();
            mass.files += 1.0;
            mass.lines += agg.lines;
            mass.test_lines += agg.role(Role::Test);
            if agg.lines > mass.largest {
                mass.largest = agg.lines;
                mass.largest_path = path;
            }
        }
    }
    let floor = CONCENTRATION_EXCESS_MIN.max(CONCENTRATION_EXCESS_FRACTION * total.lines);
    let mut hits = Vec::new();
    for role in STRATA {
        let peers: Vec<_> = dirs
            .iter()
            .filter(|(_, mass)| {
                mass.files >= CONCENTRATION_FILES_MIN
                    && stratum(mass.lines - mass.test_lines, mass.test_lines) == role
            })
            .collect();
        let means: Vec<f64> = peers
            .iter()
            .map(|(_, mass)| mass.lines / mass.files)
            .collect();
        let (scores, typical) = robust_z(&means, DIRECTORIES_SAMPLE_MIN);
        for (&(&path, mass), &z) in peers.iter().zip(&scores) {
            let excess = mass.lines - typical * mass.files;
            if z >= OUTLIER_Z && excess >= floor {
                hits.push(Hit {
                    path,
                    mass,
                    typical,
                    excess,
                    role,
                });
            }
        }
    }
    let flagged_files: BTreeSet<String> = files(context).into_iter().map(|f| f.subject).collect();
    let mut findings = Vec::new();
    for hit in &hits {
        // The flagged directories below it carry most of its excess: they are the findings.
        let inner: Vec<_> = hits
            .iter()
            .filter(|inner| inside(inner.path, hit.path))
            .collect();
        let covered: f64 = inner
            .iter()
            .filter(|a| !inner.iter().any(|b| inside(a.path, b.path)))
            .map(|a| a.excess)
            .sum();
        // So does one file, when it is a finding of its own.
        let one_file = flagged_files.contains(hit.mass.largest_path)
            && hit.mass.largest - hit.typical >= DESCENDANT_EXPLAINS * hit.excess;
        if covered >= DESCENDANT_EXPLAINS * hit.excess || one_file {
            continue;
        }
        findings.push(concentration(context, hit, total.files));
    }
    findings
}

/// A flagged directory.
struct Hit<'a> {
    path: &'a str,
    mass: &'a Mass<'a>,
    typical: f64,
    excess: f64,
    role: Role,
}

/// `path` lies under `ancestor`.
fn inside(path: &str, ancestor: &str) -> bool {
    path.len() > ancestor.len()
        && path.starts_with(ancestor)
        && path.as_bytes()[ancestor.len()] == b'/'
}

fn concentration(context: &Context, hit: &Hit, all_files: f64) -> Finding {
    let Hit {
        path,
        mass,
        typical,
        excess,
        role,
    } = *hit;
    let mean = mass.lines / mass.files;
    let reason = format!(
        "{} holds {} lines in {} files, {} lines a file where {} directories typically hold {}: about {} lines more than usual.",
        path,
        count(mass.lines),
        count(mass.files),
        count(mean),
        population_word(role),
        count(typical),
        count(excess),
    );
    let prefix = format!("{path}/");
    let mut biggest: Vec<_> = code_files(context)
        .filter(|(file, _)| file.starts_with(&prefix))
        .collect();
    biggest.sort_by(|a, b| b.1.lines.total_cmp(&a.1.lines).then(a.0.cmp(b.0)));
    let mut found = Finding::new(
        "size",
        SubjectKind::Directory,
        path,
        reason,
        excess,
        1.0 - typical / mean,
    )
    .measure("lines", mass.lines)
    .measure("files", mass.files)
    .measure("meanFileLines", mean)
    .measure("typicalFileLines", typical)
    .measure("fileShare", mass.files / all_files)
    .measure("excessLines", excess)
    .fact(format!(
        "{} lines in {} files",
        count(mass.lines),
        count(mass.files)
    ))
    .fact(format!(
        "{} lines a file, against a typical {} among {} directories",
        count(mean),
        count(typical),
        population_word(role)
    ))
    .fact(format!("{} lines more than usual", count(excess)));
    for (file, file_agg) in biggest.into_iter().take(3) {
        found = found.evidence(file, &[(1, whole_lines(file_agg.lines))]);
    }
    found
}

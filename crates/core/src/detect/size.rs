//! Size and concentration: outlier files and functions against the codebase's own distribution,
//! and directories holding far more lines than their share of the files.
//!
//! Sizes are heavy-tailed, so outliers are judged on a log scale with robust statistics: a size
//! is an outlier when its modified z-score, `0.6745 * (log10(x) - median) / MAD`, reaches
//! [`OUTLIER_Z`] (the usual 3.5 cut for the median absolute deviation), it is above an absolute
//! floor, and the sample is big enough to have a distribution. The deviation is floored so a
//! uniform codebase does not turn small differences into outliers.
//!
//! Files that are all configuration (JSON, TOML, YAML) are left out of both: they are mostly data
//! other tools write, and configuration is not what a size finding is about.
//!
//! A directory is a concentration when its share of the lines is at least [`CONCENTRATION_RATIO`]
//! times its share of the files and the lines beyond its file share (the excess) are a real mass.

use std::collections::BTreeMap;

use codemap_lang::Role;

use super::corpus::{Agg, FunctionStat, ancestors};
use super::{Context, Detector, Finding, SubjectKind, count};
use crate::reader::QueryError;

/// A file with this share of configuration lines is data.
const CONFIG_SHARE: f64 = 0.95;
const OUTLIER_Z: f64 = 3.5;
/// Scales the median absolute deviation to a standard deviation for normal data.
const MAD_SCALE: f64 = 0.6745;
/// In log10 units: a floor under the deviation, about 12% size differences.
const MAD_FLOOR: f64 = 0.05;
const FILES_SAMPLE_MIN: usize = 20;
const FUNCTIONS_SAMPLE_MIN: usize = 30;
const FILE_LINES_MIN: f64 = 150.0;
const FILE_TOKENS_MIN: f64 = 1500.0;
const FUNCTION_LINES_MIN: f64 = 60.0;
const FUNCTION_TOKENS_MIN: f64 = 600.0;
const CONCENTRATION_RATIO: f64 = 3.0;
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

fn median(sorted: &[f64]) -> f64 {
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        f64::midpoint(sorted[mid - 1], sorted[mid])
    } else {
        sorted[mid]
    }
}

/// Robust z-scores on a log scale; empty when the sample is too small.
fn robust_z(values: &[f64], sample_min: usize) -> (Vec<f64>, f64) {
    if values.len() < sample_min {
        return (Vec::new(), 0.0);
    }
    let mut logs: Vec<f64> = values.iter().map(|v| v.max(1.0).log10()).collect();
    let mut sorted = logs.clone();
    sorted.sort_by(f64::total_cmp);
    let centre = median(&sorted);
    let mut deviations: Vec<f64> = logs.iter().map(|v| (v - centre).abs()).collect();
    deviations.sort_by(f64::total_cmp);
    let mad = median(&deviations).max(MAD_FLOOR);
    for log in &mut logs {
        *log = MAD_SCALE * (*log - centre) / mad;
    }
    (logs, 10_f64.powf(centre))
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

/// Files that are not almost entirely configuration.
fn code_files<'a>(context: &'a Context) -> impl Iterator<Item = (&'a String, &'a Agg)> {
    context
        .corpus
        .files
        .iter()
        .filter(|(_, agg)| agg.role(Role::Config) < CONFIG_SHARE * agg.lines)
}

fn files(context: &Context) -> Vec<Finding> {
    let entries: Vec<_> = code_files(context).collect();
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
            "{} is {} lines and {} tokens; the typical file here is about {} lines.",
            path,
            count(agg.lines),
            count(agg.tokens),
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
            .evidence(&symbol.path, &[(symbol.start_line, symbol.end_line)]),
        );
    }
    findings
}

/// Files and lines of a directory's code files.
#[derive(Default)]
struct Mass {
    files: f64,
    lines: f64,
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
        }
    }
    if total.lines <= 0.0 {
        return Vec::new();
    }
    let hits: Vec<_> = dirs
        .iter()
        .filter_map(|(&path, mass)| {
            let line_share = mass.lines / total.lines;
            let file_share = mass.files / total.files;
            let excess = mass.lines - file_share * total.lines;
            let flagged = line_share >= CONCENTRATION_RATIO * file_share
                && excess
                    >= CONCENTRATION_EXCESS_MIN.max(CONCENTRATION_EXCESS_FRACTION * total.lines);
            flagged.then_some((path, mass, line_share, file_share, excess))
        })
        .collect();
    let mut findings = Vec::new();
    for &(path, mass, line_share, file_share, excess) in &hits {
        let explained = hits.iter().any(|&(other, _, _, _, other_excess)| {
            other.len() > path.len()
                && other.starts_with(path)
                && other.as_bytes()[path.len()] == b'/'
                && other_excess >= DESCENDANT_EXPLAINS * excess
        });
        if explained {
            continue;
        }
        let reason = format!(
            "{} holds {:.0}% of the lines in {:.0}% of the files: {} lines, about {} more than its share.",
            path,
            line_share * 100.0,
            file_share * 100.0,
            count(mass.lines),
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
            1.0 - file_share / line_share,
        )
        .measure("lines", mass.lines)
        .measure("files", mass.files)
        .measure("lineShare", line_share)
        .measure("fileShare", file_share)
        .measure("excessLines", excess);
        for (file, file_agg) in biggest.into_iter().take(3) {
            found = found.evidence(file, &[(1, whole_lines(file_agg.lines))]);
        }
        findings.push(found);
    }
    findings
}

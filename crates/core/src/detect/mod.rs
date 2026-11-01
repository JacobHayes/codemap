//! Detectors read the index and emit findings; ranking groups findings into areas. A detector
//! never sees a language, only lines, symbols and measures.

mod composition;
mod corpus;
mod duplication;
mod indirection;
mod parallel;
mod persist;
mod rank;
mod robust;
mod rules;
mod size;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::config::Config;
use crate::reader::{Index, QueryError};

pub use corpus::Corpus;
pub(crate) use corpus::is_ranked;
pub use rank::{Area, rank, test_shares};

/// What a finding is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "Answer.ts")]
pub enum SubjectKind {
    Directory,
    File,
    Function,
    /// A pattern spanning places (copies, a naming family, a repeated switch): its own area.
    Group,
}

impl SubjectKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::File => "file",
            Self::Function => "function",
            Self::Group => "group",
        }
    }
}

/// A line range of one file that backs a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct Evidence {
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
}

/// One detector's observation about a subject: the measures that triggered it, evidence, and a
/// reason in plain words. It carries no verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct Finding {
    pub detector: String,
    /// A directory or file path, `path::name` for a function, or a group's id.
    pub subject: String,
    pub subject_kind: SubjectKind,
    /// The area this finding belongs to: the file or directory it sits in, or for a group its
    /// id.
    pub area: String,
    pub reason: String,
    /// What weighs the area's opportunity: a group's lines counted once, a place's lines beyond
    /// what its peers' median would hold.
    pub mass_lines: f64,
    /// 0 to 1; breaks ties, never hides.
    pub confidence: f64,
    /// Detector-specific numbers by name.
    pub measures: BTreeMap<String, f64>,
    /// Detector-specific words by name.
    pub labels: BTreeMap<String, String>,
    /// The numbers behind the reason as short sentences, ready to show.
    pub facts: Vec<String>,
    pub evidence: Vec<Evidence>,
    /// The reason an acknowledgement gives, when one covers this finding.
    pub acknowledged: Option<String>,
}

impl Finding {
    pub(crate) fn new(
        detector: &str,
        kind: SubjectKind,
        subject: &str,
        reason: String,
        mass_lines: f64,
        confidence: f64,
    ) -> Self {
        let area = subject.split_once("::").map_or(subject, |(path, _)| path);
        Self {
            detector: detector.to_owned(),
            subject: subject.to_owned(),
            subject_kind: kind,
            area: area.to_owned(),
            reason,
            mass_lines,
            confidence: confidence.clamp(0.0, 1.0),
            measures: BTreeMap::new(),
            labels: BTreeMap::new(),
            facts: Vec::new(),
            evidence: Vec::new(),
            acknowledged: None,
        }
    }

    /// A finding about a group: the pattern is its own area, named by `id` and `title`, and
    /// its evidence lists every member.
    pub(crate) fn group(
        detector: &str,
        id: &str,
        title: String,
        reason: String,
        mass_lines: f64,
        confidence: f64,
    ) -> Self {
        let mut finding = Self::new(
            detector,
            SubjectKind::Group,
            id,
            reason,
            mass_lines,
            confidence,
        );
        id.clone_into(&mut finding.area);
        finding.labels.insert(TITLE_LABEL.to_owned(), title);
        finding
    }

    /// Names the group's members for acknowledgements that outlive the group's id.
    pub(crate) fn members(mut self, members: impl IntoIterator<Item = String>) -> Self {
        let members: BTreeSet<String> = members.into_iter().collect();
        self.labels.insert(
            MEMBERS_LABEL.to_owned(),
            members.into_iter().collect::<Vec<_>>().join("\n"),
        );
        self
    }

    pub(crate) fn measure(mut self, name: &str, value: f64) -> Self {
        self.measures.insert(name.to_owned(), value);
        self
    }

    pub(crate) fn fact(mut self, sentence: String) -> Self {
        self.facts.push(sentence);
        self
    }

    pub(crate) fn evidence(mut self, path: &str, runs: &[(u32, u32)]) -> Self {
        self.evidence
            .extend(runs.iter().map(|&(start_line, end_line)| Evidence {
                path: path.to_owned(),
                start_line,
                end_line,
            }));
        self
    }
}

/// The label a group finding carries its area's title in.
pub(crate) const TITLE_LABEL: &str = "title";

/// The label a group finding lists its members in, one per line: what an acknowledgement
/// matches by when the group's id no longer exists.
pub(crate) const MEMBERS_LABEL: &str = "members";

/// Prefix of a group's id, which tells it from a path.
pub const GROUP_ID_PREFIX: &str = "group:";

/// A group's id: the detector and a digest of the group's natural `key`, which each detector
/// chooses so that it survives what happens while a group is worked on (a member edited, one
/// added or gone). The key parts are sorted and deduplicated here, so order never matters.
#[must_use]
pub(crate) fn group_id(detector: &str, key: &[String]) -> String {
    let mut key: Vec<&str> = key.iter().map(String::as_str).collect();
    key.sort_unstable();
    key.dedup();
    let mut hasher = blake3::Hasher::new();
    for part in key {
        hasher.update(part.as_bytes());
        hasher.update(&[0]);
    }
    let digest = hasher.finalize().to_hex();
    format!("{GROUP_ID_PREFIX}{detector}:{}", &digest.as_str()[..12])
}

/// "`prefix`: 3 places in 2 files".
pub(crate) fn places_title(prefix: &str, places: usize, files: usize) -> String {
    let s = |n: usize| if n == 1 { "" } else { "s" };
    format!(
        "{prefix}: {places} place{} in {files} file{}",
        s(places),
        s(files)
    )
}

/// Distinct lines of line ranges per file, counting a line in nested ranges once.
pub(crate) fn distinct_lines_by_path<'a>(
    ranges: impl Iterator<Item = (&'a str, u32, u32)>,
) -> BTreeMap<&'a str, u32> {
    let mut by_path: BTreeMap<&str, Vec<(u32, u32)>> = BTreeMap::new();
    for (path, start, end) in ranges {
        by_path.entry(path).or_default().push((start, end));
    }
    by_path
        .into_iter()
        .map(|(path, mut ranges)| {
            ranges.sort_unstable();
            let (mut total, mut reached) = (0, 0);
            for (start, end) in ranges {
                let start = start.max(reached + 1);
                if end >= start {
                    total += end - start + 1;
                    reached = end;
                }
            }
            (path, total)
        })
        .collect()
}

/// Everything a detector may read.
pub struct Context<'a> {
    pub index: &'a Index,
    pub corpus: &'a Corpus,
    pub config: &'a Config,
}

/// An extension point: one file and one line in [`DETECTORS`].
pub trait Detector: Sync {
    fn name(&self) -> &'static str;
    /// What the detector looks for, in a sentence.
    fn describe(&self) -> &'static str;
    /// # Errors
    /// Fails when the index cannot be read.
    fn detect(&self, context: &Context) -> Result<Vec<Finding>, QueryError>;
}

/// Every shipped detector: one registry line each.
pub static DETECTORS: &[&dyn Detector] = &[
    &composition::Composition,
    &size::Size,
    &duplication::Duplication,
    &indirection::Indirection,
    &parallel::Parallel,
    &rules::Rules,
];

/// Runs every detector, in registry order.
///
/// # Errors
/// Fails when the index cannot be read.
pub fn detect(index: &Index, config: &Config) -> Result<Vec<Finding>, QueryError> {
    let corpus = Corpus::load(index)?;
    let context = Context {
        index,
        corpus: &corpus,
        config,
    };
    let mut findings = Vec::new();
    for detector in DETECTORS {
        findings.extend(detector.detect(&context)?);
    }
    Ok(findings)
}

/// A whole number with thousands separators.
pub(crate) fn count(value: f64) -> String {
    let digits = format!("{value:.0}");
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 && ch != '-' && digits.as_bytes()[i - 1] != b'-' {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub(crate) fn percent(share: f64) -> String {
    format!("{:.0}%", share * 100.0)
}

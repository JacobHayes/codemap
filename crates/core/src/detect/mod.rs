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

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::config::Config;
use crate::reader::{Index, QueryError};

pub use corpus::Corpus;
pub(crate) use corpus::is_ranked;
pub use rank::{Area, rank};

/// What a finding is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "Answer.ts")]
pub enum SubjectKind {
    Directory,
    File,
    Function,
    /// Copies of one function, as a duplicate group.
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
    /// A directory or file path, or `path::name` for a function.
    pub subject: String,
    pub subject_kind: SubjectKind,
    /// The file or directory whose area this finding belongs to.
    pub area: String,
    pub reason: String,
    /// The lines the finding points at; weighs the area's opportunity.
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

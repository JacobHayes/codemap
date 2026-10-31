//! Ranking: findings group into areas by the file or directory they sit in. An area's
//! opportunity is the number of distinct detectors that agree times the mass at stake (the
//! largest finding's lines); confidence only breaks ties. Acknowledged areas rank after the rest
//! and are counted, never hidden.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{Finding, SubjectKind};
use crate::config::{Acknowledgement, Config};

/// A file or directory with the findings that point at it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct Area {
    pub subject: String,
    pub subject_kind: SubjectKind,
    /// 1 is the first area to look at.
    pub rank: u32,
    pub opportunity: f64,
    pub mass_lines: f64,
    pub confidence: f64,
    /// The distinct detectors that agree, by name.
    pub detectors: Vec<String>,
    pub finding_count: u32,
    /// The biggest finding's reason: why the area stands out.
    pub reason: String,
    /// The acknowledgements' reasons, when every finding is acknowledged.
    pub acknowledged: Option<String>,
}

fn covers(acknowledgement: &Acknowledgement, finding: &Finding) -> bool {
    let path = acknowledgement.path.trim_matches('/');
    let inside = finding.area == path
        || (finding.area.starts_with(path)
            && finding.area.as_bytes().get(path.len()) == Some(&b'/'));
    inside
        && acknowledgement
            .detector
            .as_ref()
            .is_none_or(|detector| *detector == finding.detector)
}

/// Marks acknowledged findings and groups the findings into ranked areas.
#[must_use]
pub fn rank(findings: &mut [Finding], config: &Config) -> Vec<Area> {
    for finding in findings.iter_mut() {
        finding.acknowledged = config
            .acknowledgements
            .iter()
            .filter(|acknowledgement| covers(acknowledgement, finding))
            .map(|acknowledgement| acknowledgement.reason.as_str())
            .collect::<Vec<_>>()
            .first()
            .map(|reason| (*reason).to_owned());
    }
    let mut groups: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
    for finding in findings.iter() {
        groups.entry(&finding.area).or_default().push(finding);
    }
    let mut areas: Vec<Area> = groups
        .into_iter()
        .map(|(subject, group)| area_of(subject, &group))
        .collect();
    areas.sort_by(|a, b| {
        a.acknowledged
            .is_some()
            .cmp(&b.acknowledged.is_some())
            .then(b.opportunity.total_cmp(&a.opportunity))
            .then(b.confidence.total_cmp(&a.confidence))
            .then(a.subject.cmp(&b.subject))
    });
    for (i, area) in areas.iter_mut().enumerate() {
        area.rank = u32::try_from(i + 1).unwrap_or(u32::MAX);
    }
    areas
}

fn area_of(subject: &str, group: &[&Finding]) -> Area {
    let open: Vec<&&Finding> = group.iter().filter(|f| f.acknowledged.is_none()).collect();
    // Partly acknowledged areas count only what is still open.
    let counted: Vec<&Finding> = if open.is_empty() {
        group.to_vec()
    } else {
        open.into_iter().copied().collect()
    };
    let detectors: BTreeSet<&str> = counted.iter().map(|f| f.detector.as_str()).collect();
    let mass_lines = counted.iter().map(|f| f.mass_lines).fold(0.0, f64::max);
    let reason = counted
        .iter()
        .max_by(|a, b| a.mass_lines.total_cmp(&b.mass_lines))
        .map(|f| f.reason.clone())
        .unwrap_or_default();
    let confidence = counted.iter().map(|f| f.confidence).fold(0.0, f64::max);
    let acknowledged = group.iter().all(|f| f.acknowledged.is_some()).then(|| {
        let reasons: BTreeSet<&str> = group
            .iter()
            .filter_map(|f| f.acknowledged.as_deref())
            .collect();
        reasons.into_iter().collect::<Vec<_>>().join("; ")
    });
    let is_dir = group
        .iter()
        .any(|f| f.subject_kind == SubjectKind::Directory);
    Area {
        subject: subject.to_owned(),
        subject_kind: if is_dir {
            SubjectKind::Directory
        } else {
            SubjectKind::File
        },
        rank: 0,
        opportunity: f64::from(u32::try_from(detectors.len()).unwrap_or(u32::MAX)) * mass_lines,
        mass_lines,
        confidence,
        detectors: detectors.into_iter().map(str::to_owned).collect(),
        finding_count: u32::try_from(group.len()).unwrap_or(u32::MAX),
        reason,
        acknowledged,
    }
}

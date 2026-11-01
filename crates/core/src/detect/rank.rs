//! Ranking: findings group into areas by the file or directory they sit in, or by the group they
//! belong to (a group is its own area, however many files it spans). An area's
//! opportunity is the number of distinct detectors that agree times the mass at stake (the
//! largest finding's lines); confidence only breaks ties. Test code weighs less: an area's
//! opportunity is discounted by its test share. Acknowledged areas rank after the rest and are
//! counted, never hidden.

use std::collections::{BTreeMap, BTreeSet};

use codemap_lang::Role;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::corpus::Corpus;
use super::{Evidence, Finding, MEMBERS_LABEL, SubjectKind, TITLE_LABEL};
use crate::config::{Acknowledgement, Config};
use crate::reader::{Index, QueryError};

/// What an area made only of test lines keeps of its opportunity. Tests are the biggest and most
/// repetitive files of a codebase and a cleanup there pays less than the same lines of
/// production code, so test areas rank behind production areas of comparable mass; a discount
/// rather than a partition lets an extreme test outlier still outrank a weak production one.
/// A mixed area keeps a share in proportion to its test lines.
pub(crate) const TEST_OPPORTUNITY_KEPT: f64 = 0.4;

/// A location (file or directory) with the findings that point at it, or a group (copies, a
/// naming family, a repeated switch) with its one finding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct Area {
    /// A path, or a group's id.
    pub subject: String,
    /// What to call it: the path, or for a group what the pattern is and how many places and
    /// files it spans.
    pub title: String,
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

fn within(path: &str, prefix: &str) -> bool {
    path == prefix || (path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/'))
}

fn members_of(finding: &Finding) -> BTreeSet<&str> {
    finding
        .labels
        .get(MEMBERS_LABEL)
        .map(|members| members.split('\n').collect())
        .unwrap_or_default()
}

/// The group findings an acknowledgement by group names: the group with its id, or when none has
/// it any more, the one of the same detector that shares at least half of its members with the
/// acknowledged set (the best such, the first of equals).
fn group_targets(acknowledgement: &Acknowledgement, findings: &[Finding]) -> BTreeSet<usize> {
    let Some(id) = &acknowledgement.group else {
        return BTreeSet::new();
    };
    let groups = || {
        findings
            .iter()
            .enumerate()
            .filter(|(_, f)| f.subject_kind == SubjectKind::Group)
    };
    let exact: BTreeSet<usize> = groups()
        .filter(|(_, f)| f.area == *id)
        .map(|(i, _)| i)
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    let acknowledged: BTreeSet<&str> = acknowledgement.members.iter().map(String::as_str).collect();
    let detector = id.split(':').nth(1);
    groups()
        .filter(|(_, f)| detector == Some(f.detector.as_str()))
        .map(|(i, f)| {
            let own = members_of(f);
            (i, own.intersection(&acknowledged).count(), own.len())
        })
        .filter(|&(_, shared, own)| own > 0 && shared * 2 >= own)
        .min_by_key(|&(i, shared, _)| (std::cmp::Reverse(shared), i))
        .map(|(i, _, _)| i)
        .into_iter()
        .collect()
}

fn covers(
    acknowledgement: &Acknowledgement,
    targets: &BTreeSet<usize>,
    index: usize,
    finding: &Finding,
) -> bool {
    let target = match (&acknowledgement.group, &acknowledgement.path) {
        (Some(_), _) => targets.contains(&index),
        (None, Some(path)) => {
            let path = path.trim_matches('/');
            if finding.subject_kind == SubjectKind::Group {
                !finding.evidence.is_empty()
                    && finding.evidence.iter().all(|e| within(&e.path, path))
            } else {
                within(&finding.area, path)
            }
        }
        (None, None) => false,
    };
    target
        && acknowledgement
            .detector
            .as_ref()
            .is_none_or(|detector| *detector == finding.detector)
}

/// Marks acknowledged findings and groups the findings into ranked areas.
#[must_use]
pub fn rank(findings: &mut [Finding], test_shares: &[f64], config: &Config) -> Vec<Area> {
    let targets: Vec<BTreeSet<usize>> = config
        .acknowledgements
        .iter()
        .map(|acknowledgement| group_targets(acknowledgement, findings))
        .collect();
    for (index, finding) in findings.iter_mut().enumerate() {
        finding.acknowledged = config
            .acknowledgements
            .iter()
            .zip(&targets)
            .find(|(acknowledgement, targets)| covers(acknowledgement, targets, index, finding))
            .map(|(acknowledgement, _)| acknowledgement.reason.clone());
    }
    let mut groups: BTreeMap<&str, Vec<(&Finding, f64)>> = BTreeMap::new();
    for (finding, &share) in findings.iter().zip(test_shares) {
        groups
            .entry(&finding.area)
            .or_default()
            .push((finding, share));
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

fn area_of(subject: &str, shared: &[(&Finding, f64)]) -> Area {
    let group: Vec<&Finding> = shared.iter().map(|&(f, _)| f).collect();
    let open: Vec<(&Finding, f64)> = shared
        .iter()
        .filter(|(f, _)| f.acknowledged.is_none())
        .copied()
        .collect();
    // Partly acknowledged areas count only what is still open.
    let counted_shared = if open.is_empty() { shared } else { &open };
    let counted: Vec<&Finding> = counted_shared.iter().map(|&(f, _)| f).collect();
    let test_share = counted_shared.iter().map(|&(_, s)| s).sum::<f64>()
        / f64::from(u32::try_from(counted_shared.len()).unwrap_or(u32::MAX));
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
    let kind = if group.iter().any(|f| f.subject_kind == SubjectKind::Group) {
        SubjectKind::Group
    } else if group
        .iter()
        .any(|f| f.subject_kind == SubjectKind::Directory)
    {
        SubjectKind::Directory
    } else {
        SubjectKind::File
    };
    let title = group
        .iter()
        .find_map(|f| f.labels.get(TITLE_LABEL))
        .map_or_else(|| subject.to_owned(), Clone::clone);
    Area {
        subject: subject.to_owned(),
        title,
        subject_kind: kind,
        rank: 0,
        opportunity: f64::from(u32::try_from(detectors.len()).unwrap_or(u32::MAX))
            * mass_lines
            * (1.0 - (1.0 - TEST_OPPORTUNITY_KEPT) * test_share),
        mass_lines,
        confidence,
        detectors: detectors.into_iter().map(str::to_owned).collect(),
        finding_count: u32::try_from(group.len()).unwrap_or(u32::MAX),
        reason,
        acknowledged,
    }
}

/// The evidence's line ranges with overlaps merged per file, so a line counts once.
fn merged_ranges(evidence: &[Evidence]) -> Vec<(&str, u32, u32)> {
    let mut sorted: Vec<(&str, u32, u32)> = evidence
        .iter()
        .map(|e| (e.path.as_str(), e.start_line, e.end_line))
        .collect();
    sorted.sort_unstable();
    let mut merged: Vec<(&str, u32, u32)> = Vec::new();
    for (path, start, end) in sorted {
        match merged.last_mut() {
            Some(last) if last.0 == path && start <= last.2 => last.2 = last.2.max(end),
            _ => merged.push((path, start, end)),
        }
    }
    merged
}

/// Each finding's share of test lines (0 to 1), in the findings' order: a group's from the lines
/// of its evidence, a place's from the lines under it.
///
/// # Errors
/// Fails when the index cannot be read.
pub fn test_shares(index: &Index, findings: &[Finding]) -> Result<Vec<f64>, QueryError> {
    let corpus = Corpus::load(index)?;
    let mut by_role = index.store().connection().prepare(
        "SELECT CAST(count(*) FILTER (WHERE role = 'test') AS DOUBLE), CAST(count(*) AS DOUBLE)
         FROM lines WHERE path = ? AND line BETWEEN ? AND ?",
    )?;
    let mut shares = Vec::with_capacity(findings.len());
    for finding in findings {
        let (test, all) = if finding.subject_kind == SubjectKind::Group {
            let mut sum = (0.0, 0.0);
            for (path, start, end) in merged_ranges(&finding.evidence) {
                let (t, a) = by_role.query_row(duckdb::params![path, start, end], |row| {
                    Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?))
                })?;
                sum = (sum.0 + t, sum.1 + a);
            }
            sum
        } else {
            let agg = corpus
                .files
                .get(&finding.area)
                .or_else(|| corpus.dirs.get(&finding.area));
            agg.map_or((0.0, 0.0), |agg| (agg.role(Role::Test), agg.lines))
        };
        shares.push(if all > 0.0 { test / all } else { 0.0 });
    }
    Ok(shares)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_evidence_counts_each_line_once() {
        let at = |start_line, end_line| Evidence {
            path: "a".to_owned(),
            start_line,
            end_line,
        };
        let evidence = [at(4, 6), at(1, 6), at(9, 9)];
        let merged = merged_ranges(&evidence);
        assert_eq!(merged, vec![("a", 1, 6), ("a", 9, 9)]);
    }

    fn finding(area: &str, mass: f64) -> Finding {
        Finding::new("size", SubjectKind::File, area, String::new(), mass, 0.5)
    }

    #[test]
    fn test_areas_rank_behind_production_areas_of_comparable_mass() {
        let mut findings = vec![finding("a_test", 110.0), finding("b_prod", 100.0)];
        let areas = rank(&mut findings, &[1.0, 0.0], &Config::default());
        assert_eq!(areas[0].subject, "b_prod");
        assert!(areas[1].opportunity < areas[1].mass_lines);
    }

    #[test]
    fn an_extreme_test_outlier_still_outranks_a_weak_production_area() {
        let mut findings = vec![finding("a_test", 1000.0), finding("b_prod", 100.0)];
        let areas = rank(&mut findings, &[1.0, 0.0], &Config::default());
        assert_eq!(areas[0].subject, "a_test");
    }

    #[test]
    fn a_mixed_area_is_discounted_by_its_test_share() {
        let mut findings = vec![finding("half", 100.0), finding("all", 100.0)];
        let areas = rank(&mut findings, &[0.5, 1.0], &Config::default());
        assert_eq!(areas[0].subject, "half");
        assert!(areas[0].opportunity > areas[1].opportunity);
    }
}

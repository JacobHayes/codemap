//! Ranking: findings group into areas by the file or directory they sit in, or by the group they
//! belong to (a group is its own area, however many files it spans). An area's
//! opportunity is the number of distinct detectors that agree times the mass at stake (the
//! largest finding's lines); confidence only breaks ties. Areas whose evidence covers the same
//! lines agree: each keeps its own identity and lists the other, and the detectors of all of
//! them count once. Test code weighs less: an area's opportunity is discounted by its test
//! share. Acknowledged areas rank after the rest and are counted, never hidden.

use std::collections::{BTreeMap, BTreeSet};

use codemap_lang::Role;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::corpus::Corpus;
use super::{Evidence, Finding, MEMBERS_LABEL, SubjectKind, TITLE_LABEL, distinct_lines_by_path};
use crate::config::{Acknowledgement, Config};
use crate::reader::{Index, QueryError};

/// What an area made only of test lines keeps of its opportunity. Tests are the biggest and most
/// repetitive files of a codebase and a cleanup there pays less than the same lines of
/// production code, so test areas rank behind production areas of comparable mass; a discount
/// rather than a partition lets an extreme test outlier still outrank a weak production one.
/// A mixed area keeps a share in proportion to its test lines.
pub(crate) const TEST_OPPORTUNITY_KEPT: f64 = 0.4;

/// The share of lines two areas must have in common to agree: of the longer of two evidence
/// ranges for them to match, and of the smaller area's evidence lines for the matches to count.
/// Measured on a Rust, a TypeScript and a mixed codebase: pairs at a half or more were two
/// detectors on the same code; the pairs below it were a case set sharing a third of its lines
/// with a copied function.
pub(crate) const AGREEMENT_OVERLAP_SHARE: f64 = 0.5;

/// Another area whose evidence covers these lines and which a different detector found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct AgreeingArea {
    /// A path, or a group's id.
    pub subject: String,
    pub title: String,
    /// The detectors of that area.
    pub detectors: Vec<String>,
}

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
    /// Areas that flag the same lines for another reason, best first. Their detectors count
    /// toward `opportunity` once each.
    pub agreeing: Vec<AgreeingArea>,
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
    let mut evidence: Vec<Evidenced> = Vec::with_capacity(groups.len());
    let mut areas: Vec<Area> = groups
        .into_iter()
        .map(|(subject, group)| {
            evidence.push(open_evidence(&group));
            area_of(subject, &group)
        })
        .collect();
    agree(&mut areas, &evidence);
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
        agreeing: Vec::new(),
        reason,
        acknowledged,
    }
}

/// The line ranges an area's open findings point at, nested ones kept apart so that a function's
/// range is not lost in its file's, and the detectors that found them. An acknowledged area has
/// none: it ranks last and neither lends nor takes agreement. A finding without evidence (a rule
/// on a whole file or directory) names no lines and takes no part.
fn open_evidence<'a>(group: &[(&'a Finding, f64)]) -> Evidenced<'a> {
    let open = group
        .iter()
        .map(|&(f, _)| f)
        .filter(|f| f.acknowledged.is_none() && !f.evidence.is_empty());
    let mut ranges: Vec<Run> = Vec::new();
    let mut detectors = BTreeSet::new();
    for finding in open {
        detectors.insert(finding.detector.as_str());
        ranges.extend(
            finding
                .evidence
                .iter()
                .map(|e| (e.path.as_str(), e.start_line, e.end_line)),
        );
    }
    ranges.sort_unstable();
    ranges.dedup();
    Evidenced { ranges, detectors }
}

/// The lines an area points at and the detectors that point at them.
struct Evidenced<'a> {
    ranges: Vec<Run<'a>>,
    detectors: BTreeSet<&'a str>,
}

/// A line range of one file.
type Run<'a> = (&'a str, u32, u32);

/// For each pair of areas, the lines of their ranges that are mostly the same lines: a small
/// range inside a whole file or directory says nothing about it.
fn matching_lines<'a>(evidence: &[Evidenced<'a>]) -> BTreeMap<(usize, usize), Vec<Run<'a>>> {
    let mut by_path: BTreeMap<&str, Vec<(usize, u32, u32)>> = BTreeMap::new();
    for (i, found) in evidence.iter().enumerate() {
        for &(path, start, end) in &found.ranges {
            by_path.entry(path).or_default().push((i, start, end));
        }
    }
    let mut matched: BTreeMap<(usize, usize), Vec<Run>> = BTreeMap::new();
    for (&path, ranges) in &by_path {
        for (n, &(a, a_start, a_end)) in ranges.iter().enumerate() {
            for &(b, b_start, b_end) in &ranges[n + 1..] {
                let (start, end) = (a_start.max(b_start), a_end.min(b_end));
                let longer = (a_end - a_start).max(b_end - b_start) + 1;
                if a != b
                    && start <= end
                    && f64::from(end - start + 1) >= AGREEMENT_OVERLAP_SHARE * f64::from(longer)
                {
                    matched
                        .entry((a.min(b), a.max(b)))
                        .or_default()
                        .push((path, start, end));
                }
            }
        }
    }
    matched
}

fn line_total(ranges: &[Run]) -> u32 {
    distinct_lines_by_path(ranges.iter().copied())
        .values()
        .sum()
}

/// Links areas whose evidence covers the same lines, and counts the detectors of each area
/// together with those of the areas it agrees with once each. Two areas agree when the matching
/// lines cover [`AGREEMENT_OVERLAP_SHARE`] of the smaller one's lines, and each lists the other
/// only when it brings a detector the first lacks; areas never merge, so each keeps its identity.
fn agree(areas: &mut [Area], evidence: &[Evidenced]) {
    let totals: Vec<u32> = evidence.iter().map(|e| line_total(&e.ranges)).collect();
    let mut links: Vec<Vec<usize>> = vec![Vec::new(); areas.len()];
    for ((a, b), ranges) in matching_lines(evidence) {
        let smaller = f64::from(totals[a].min(totals[b]));
        if f64::from(line_total(&ranges)) < AGREEMENT_OVERLAP_SHARE * smaller {
            continue;
        }
        // Each area links to the other only when the other brings a detector it lacks.
        for (to, from) in [(a, b), (b, a)] {
            let brings = evidence[from]
                .detectors
                .iter()
                .any(|d| !areas[to].detectors.iter().any(|own| own == d));
            if brings {
                links[to].push(from);
            }
        }
    }
    let snapshot = areas.to_vec();
    for (area, others) in areas.iter_mut().zip(&mut links) {
        let mut detectors: BTreeSet<&str> = area.detectors.iter().map(String::as_str).collect();
        let own = u32::try_from(detectors.len()).unwrap_or(u32::MAX);
        for &other in &*others {
            detectors.extend(&evidence[other].detectors);
        }
        let all = u32::try_from(detectors.len()).unwrap_or(u32::MAX);
        area.opportunity *= f64::from(all) / f64::from(own);
        others.sort_by(|&x, &y| {
            snapshot[y]
                .mass_lines
                .total_cmp(&snapshot[x].mass_lines)
                .then(snapshot[x].subject.cmp(&snapshot[y].subject))
        });
        area.agreeing = others
            .iter()
            .map(|&other| AgreeingArea {
                subject: snapshot[other].subject.clone(),
                title: snapshot[other].title.clone(),
                detectors: evidence[other]
                    .detectors
                    .iter()
                    .map(|&d| d.to_owned())
                    .collect(),
            })
            .collect();
    }
}

/// The evidence's line ranges with overlaps merged per file, so a line counts once.
fn merged_ranges<'a>(evidence: impl IntoIterator<Item = &'a Evidence>) -> Vec<(&'a str, u32, u32)> {
    let mut sorted: Vec<(&str, u32, u32)> = evidence
        .into_iter()
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

    fn flagged(detector: &str, area: &str, path: &str, runs: &[(u32, u32)]) -> Finding {
        Finding::new(detector, SubjectKind::File, area, String::new(), 100.0, 0.5)
            .evidence(path, runs)
    }

    fn area<'a>(areas: &'a [Area], subject: &str) -> &'a Area {
        areas.iter().find(|a| a.subject == subject).unwrap()
    }

    #[test]
    fn areas_over_the_same_lines_agree_and_count_their_detectors_together() {
        let mut findings = vec![
            flagged("size", "a", "f.rs", &[(10, 60)]),
            flagged("parallel", "b", "f.rs", &[(12, 60)]),
            flagged("size", "lone", "g.rs", &[(1, 50)]),
        ];
        let areas = rank(&mut findings, &[0.0; 3], &Config::default());
        let (a, b) = (area(&areas, "a"), area(&areas, "b"));
        assert_eq!(a.agreeing.len(), 1);
        assert_eq!(a.agreeing[0].subject, "b");
        assert_eq!(b.agreeing[0].subject, "a");
        assert!(a.opportunity > 1.9 * area(&areas, "lone").opportunity);
        assert_eq!(a.detectors, ["size"], "areas keep their own detectors");
    }

    #[test]
    fn a_small_range_inside_a_large_one_is_not_agreement() {
        let mut findings = vec![
            flagged("size", "big", "f.rs", &[(1, 1000)]),
            flagged("parallel", "small", "f.rs", &[(40, 60)]),
        ];
        let areas = rank(&mut findings, &[0.0; 2], &Config::default());
        assert!(areas.iter().all(|a| a.agreeing.is_empty()));
    }

    #[test]
    fn one_detector_does_not_agree_with_itself_and_acknowledged_areas_stay_apart() {
        let mut findings = vec![
            flagged("size", "a", "f.rs", &[(1, 50)]),
            flagged("size", "b", "f.rs", &[(1, 50)]),
            flagged("parallel", "c", "f.rs", &[(1, 50)]),
        ];
        let config = Config {
            acknowledgements: vec![Acknowledgement {
                path: Some("c".to_owned()),
                group: None,
                detector: None,
                members: Vec::new(),
                reason: "essential".to_owned(),
            }],
            ..Config::default()
        };
        let areas = rank(&mut findings, &[0.0; 3], &config);
        assert!(areas.iter().all(|a| a.agreeing.is_empty()));
    }

    #[test]
    fn a_function_inside_a_flagged_file_still_agrees_by_its_own_range() {
        let mut findings = vec![
            flagged("size", "f.rs", "f.rs", &[(1, 500)]),
            flagged("size", "f.rs", "f.rs", &[(200, 292)]),
            flagged("duplication", "copies", "f.rs", &[(200, 290)]),
        ];
        let areas = rank(&mut findings, &[0.0; 3], &Config::default());
        assert_eq!(area(&areas, "f.rs").agreeing.len(), 1);
    }

    #[test]
    fn a_finding_without_evidence_lends_no_detector() {
        let rule = Finding::new(
            "rules",
            SubjectKind::File,
            "f.rs",
            String::new(),
            100.0,
            0.5,
        );
        let mut findings = vec![
            flagged("size", "f.rs", "f.rs", &[(1, 50)]),
            rule,
            flagged("duplication", "copies", "f.rs", &[(1, 50)]),
        ];
        let areas = rank(&mut findings, &[0.0; 3], &Config::default());
        let copies = area(&areas, "copies");
        assert_eq!(copies.agreeing[0].detectors, ["size"]);
        assert!((copies.opportunity - 2.0 * copies.mass_lines).abs() < 1e-9);
    }

    #[test]
    fn an_area_lists_only_areas_that_bring_a_detector_it_lacks() {
        let mut findings = vec![
            flagged("size", "wide", "f.rs", &[(1, 50)]),
            flagged("parallel", "wide", "f.rs", &[(1, 50)]),
            flagged("size", "narrow", "f.rs", &[(1, 50)]),
        ];
        let areas = rank(&mut findings, &[0.0; 3], &Config::default());
        assert_eq!(area(&areas, "wide").agreeing, []);
        assert_eq!(area(&areas, "narrow").agreeing.len(), 1);
    }
}

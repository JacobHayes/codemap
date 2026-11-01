//! The `findings` and `areas` tables.

use std::collections::{BTreeMap, BTreeSet};

use duckdb::params;

use super::{Area, Finding, SubjectKind};
use crate::reader::{Index, QueryError};

fn kind_of(name: &str) -> SubjectKind {
    match name {
        "directory" => SubjectKind::Directory,
        "function" => SubjectKind::Function,
        "group" => SubjectKind::Group,
        _ => SubjectKind::File,
    }
}

impl Index {
    /// Replaces every finding and area.
    pub(crate) fn save_analysis(
        &self,
        findings: &[Finding],
        areas: &[Area],
    ) -> Result<(), QueryError> {
        let connection = self.store().connection();
        connection
            .execute_batch("DELETE FROM findings; DELETE FROM areas; DELETE FROM flagged;")?;
        let mut finding_rows = connection.appender("findings")?;
        for finding in findings {
            finding_rows.append_row(params![
                finding.detector,
                finding.subject,
                finding.subject_kind.as_str(),
                finding.area,
                finding.reason,
                finding.mass_lines,
                finding.confidence,
                serde_json::to_string(&finding.measures)?,
                serde_json::to_string(&finding.labels)?,
                serde_json::to_string(&finding.facts)?,
                serde_json::to_string(&finding.evidence)?,
                finding.acknowledged
            ])?;
        }
        finding_rows.flush()?;
        let mut area_rows = connection.appender("areas")?;
        for area in areas {
            area_rows.append_row(params![
                area.subject,
                area.title,
                area.subject_kind.as_str(),
                area.rank,
                area.opportunity,
                area.mass_lines,
                area.confidence,
                area.detectors.join(","),
                area.finding_count,
                serde_json::to_string(&area.agreeing)?,
                area.reason,
                area.acknowledged
            ])?;
        }
        area_rows.flush()?;
        drop(area_rows);
        // The lines of every open finding's evidence, once each: what `finding_lines` sums. A
        // finding with no evidence (a rule on a file or directory) covers everything under its
        // area.
        let mut flagged: BTreeSet<(String, u32)> = BTreeSet::new();
        let mut under = connection
            .prepare("SELECT path, line FROM lines WHERE path = ?1 OR starts_with(path, ?2)")?;
        for finding in findings.iter().filter(|f| f.acknowledged.is_none()) {
            for e in &finding.evidence {
                for line in e.start_line..=e.end_line {
                    flagged.insert((e.path.clone(), line));
                }
            }
            if finding.evidence.is_empty() {
                let area = finding.area.trim_end_matches('/');
                let rows = under.query_map(params![area, format!("{area}/")], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
                })?;
                for row in rows {
                    flagged.insert(row?);
                }
            }
        }
        drop(under);
        let mut flagged_rows = connection.appender("flagged")?;
        for (path, line) in flagged {
            flagged_rows.append_row(params![path, line])?;
        }
        flagged_rows.flush()?;
        Ok(())
    }

    /// Every area, best first.
    pub(crate) fn areas(&self) -> Result<Vec<Area>, QueryError> {
        let mut statement = self.store().connection().prepare(
            "SELECT subject, title, subject_kind, rank, opportunity, mass_lines, confidence, detectors,
                    finding_count, agreeing, reason, acknowledged FROM areas ORDER BY rank",
        )?;
        let rows = statement.query_map([], |row| {
            let detectors: String = row.get(7)?;
            Ok((
                Area {
                    subject: row.get(0)?,
                    title: row.get(1)?,
                    subject_kind: kind_of(&row.get::<_, String>(2)?),
                    rank: row.get(3)?,
                    opportunity: row.get(4)?,
                    mass_lines: row.get(5)?,
                    confidence: row.get(6)?,
                    detectors: detectors.split(',').map(str::to_owned).collect(),
                    finding_count: row.get(8)?,
                    agreeing: Vec::new(),
                    reason: row.get(10)?,
                    acknowledged: row.get(11)?,
                },
                row.get::<_, String>(9)?,
            ))
        })?;
        let mut areas = Vec::new();
        for row in rows {
            let (mut area, agreeing) = row?;
            area.agreeing = serde_json::from_str(&agreeing)?;
            areas.push(area);
        }
        Ok(areas)
    }

    /// The findings of one area, biggest first.
    pub(crate) fn findings_of(&self, area: &str) -> Result<Vec<Finding>, QueryError> {
        self.findings_where("area = ?", &[area])
    }

    /// Every group finding, biggest first.
    pub(crate) fn group_findings(&self) -> Result<Vec<Finding>, QueryError> {
        self.findings_where("subject_kind = 'group'", &[])
    }

    /// Every finding, biggest first.
    pub(crate) fn all_findings(&self) -> Result<Vec<Finding>, QueryError> {
        self.findings_where("true", &[])
    }

    fn findings_where(&self, condition: &str, bound: &[&str]) -> Result<Vec<Finding>, QueryError> {
        let mut statement = self.store().connection().prepare(&format!(
            "SELECT detector, subject, subject_kind, area, reason, mass_lines, confidence,
                    measures, labels, facts, evidence, acknowledged
             FROM findings WHERE {condition} ORDER BY mass_lines DESC, detector, subject"
        ))?;
        let rows = statement.query_map(duckdb::params_from_iter(bound.iter()), |row| {
            Ok((
                Finding {
                    detector: row.get(0)?,
                    subject: row.get(1)?,
                    subject_kind: kind_of(&row.get::<_, String>(2)?),
                    area: row.get(3)?,
                    reason: row.get(4)?,
                    mass_lines: row.get(5)?,
                    confidence: row.get(6)?,
                    measures: BTreeMap::new(),
                    labels: BTreeMap::new(),
                    facts: Vec::new(),
                    evidence: Vec::new(),
                    acknowledged: row.get(11)?,
                },
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
            ))
        })?;
        let mut findings = Vec::new();
        for row in rows {
            let (mut finding, measures, labels, facts, evidence) = row?;
            finding.measures = serde_json::from_str(&measures)?;
            finding.labels = serde_json::from_str(&labels)?;
            finding.facts = serde_json::from_str(&facts)?;
            finding.evidence = serde_json::from_str(&evidence)?;
            findings.push(finding);
        }
        Ok(findings)
    }
}

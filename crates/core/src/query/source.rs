//! `Source`: one file's lines with the role and concern of each, and the finding ranges that
//! touch them. The index keeps no text, so the lines are read from the file under the root.
//! They come back plain: the index holds token counts per line, not token spans.

use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::facts::split_lines;
use crate::reader::{Index, QueryError};

/// Lines returned at most in one answer, so a whole large file never lands in one reply; `more`
/// says how many follow and the range arguments fetch them.
pub const SOURCE_LINES_MAX: u32 = 300;

/// One line of a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct SourceLine {
    /// 1-based.
    pub number: u32,
    pub text: String,
    pub role: String,
    pub concern: String,
}

/// A finding's range in this file, as the detector gave it: it may start before or end after the
/// lines returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct SourceFinding {
    pub detector: String,
    /// The area id, as `area` takes it.
    pub area: String,
    pub reason: String,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct Source {
    pub path: String,
    /// `None` for files without a grammar.
    pub language: Option<String>,
    /// Lines in the whole file.
    pub line_count: u32,
    /// The range returned, 1-based and inclusive.
    pub start_line: u32,
    pub end_line: u32,
    pub lines: Vec<SourceLine>,
    /// Lines of the file after `endLine` that were not returned.
    pub more: u32,
    /// Ranges that overlap the lines returned. A finding about a whole file with no narrower
    /// evidence covers every line.
    pub findings: Vec<SourceFinding>,
}

/// Whether `path` stays inside the root by its spelling alone.
fn is_inside(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

pub(crate) fn source(
    index: &Index,
    path: &str,
    start: Option<u32>,
    end: Option<u32>,
) -> Result<Source, QueryError> {
    if !is_inside(path) {
        return Err(QueryError::OutsideRoot(path.to_owned()));
    }
    let (language, binary, indexed_hash) = index
        .file_info(path)?
        .ok_or_else(|| QueryError::UnknownFile(path.to_owned()))?;
    let unreadable = |source| QueryError::Unreadable {
        path: path.to_owned(),
        source,
    };
    let root = std::fs::canonicalize(index.root()).map_err(unreadable)?;
    let full = std::fs::canonicalize(root.join(path)).map_err(unreadable)?;
    // A symlink inside the tree may point out of it.
    if !full.starts_with(&root) {
        return Err(QueryError::OutsideRoot(path.to_owned()));
    }
    if binary {
        return Err(QueryError::NotText(path.to_owned()));
    }
    let bytes = std::fs::read(&full).map_err(unreadable)?;
    // Lines, roles and findings describe the indexed bytes; mixing them with newer text would
    // misattribute lines until the next re-index.
    if blake3::hash(&bytes).to_hex().as_str() != indexed_hash {
        return Err(QueryError::Stale(path.to_owned()));
    }
    let text = String::from_utf8(bytes).map_err(|_| QueryError::NotText(path.to_owned()))?;
    let texts = split_lines(&text);
    let line_count = u32::try_from(texts.len()).unwrap_or(u32::MAX);
    let first = start.unwrap_or(1);
    if first == 0 || first > line_count.max(1) || end.is_some_and(|end| end < first) {
        return Err(QueryError::BadRange);
    }
    let last = end
        .unwrap_or(u32::MAX)
        .min(line_count)
        .min(first.saturating_add(SOURCE_LINES_MAX - 1));
    let attributes = index.line_attributes(path, first, last)?;
    let lines = (first..=last)
        .zip(attributes)
        .map(|(number, (role, concern))| SourceLine {
            number,
            // A file edited since the index was written may be shorter than the index says.
            text: texts
                .get(number as usize - 1)
                .map_or_else(String::new, |text| (*text).to_owned()),
            role,
            concern,
        })
        .collect();
    Ok(Source {
        path: path.to_owned(),
        language,
        line_count,
        start_line: first,
        end_line: last,
        lines,
        more: line_count - last,
        findings: touching(index, path, first, last, line_count)?,
    })
}

fn touching(
    index: &Index,
    path: &str,
    first: u32,
    last: u32,
    line_count: u32,
) -> Result<Vec<SourceFinding>, QueryError> {
    let mut found = Vec::new();
    for finding in index.all_findings()? {
        let ranges: Vec<(u32, u32)> = if finding.evidence.is_empty() && finding.subject == path {
            vec![(1, line_count)]
        } else {
            finding
                .evidence
                .iter()
                .filter(|evidence| evidence.path == path)
                .map(|evidence| (evidence.start_line, evidence.end_line))
                .collect()
        };
        found.extend(
            ranges
                .into_iter()
                .filter(|&(start, end)| start <= last && end >= first)
                .map(|(start_line, end_line)| SourceFinding {
                    detector: finding.detector.clone(),
                    area: finding.area.clone(),
                    reason: finding.reason.clone(),
                    start_line,
                    end_line,
                }),
        );
    }
    found.sort_by(|a, b| {
        (a.start_line, a.end_line, &a.detector).cmp(&(b.start_line, b.end_line, &b.detector))
    });
    Ok(found)
}

impl Index {
    /// A file's language, whether it is binary, and its indexed content hash; `None` when the index does not hold it.
    fn file_info(&self, path: &str) -> Result<Option<(Option<String>, bool, String)>, QueryError> {
        let mut statement = self
            .store()
            .connection()
            .prepare("SELECT language, is_binary, hash FROM files WHERE path = ?")?;
        let mut rows =
            statement.query_map([path], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        Ok(rows.next().transpose()?)
    }

    /// Role and concern of each line in `first..=last`, in line order.
    fn line_attributes(
        &self,
        path: &str,
        first: u32,
        last: u32,
    ) -> Result<Vec<(String, String)>, QueryError> {
        let mut statement = self.store().connection().prepare(
            "SELECT role, concern FROM lines WHERE path = ? AND line BETWEEN ? AND ? ORDER BY line",
        )?;
        let rows = statement.query_map(duckdb::params![path, first, last], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

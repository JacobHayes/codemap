//! `Overview`: aggregates over the whole index.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::measure::{Base, Filter, OTHER_LANGUAGE, Term};
use crate::reader::{Index, QueryError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct Totals {
    pub files: u64,
    pub lines: u64,
    pub tokens: u64,
    pub nodes: u64,
    pub symbols: u64,
}

/// One value of a dimension and its part of the whole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct Share {
    pub name: String,
    pub lines: u64,
    pub tokens: u64,
    /// Fraction of all lines, 0 to 1.
    pub share: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct DetectorCount {
    pub detector: String,
    /// Areas this detector found something in.
    pub areas: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct AreaCounts {
    pub total: u32,
    pub acknowledged: u32,
    /// One entry per detector, including those that found nothing.
    pub by_detector: Vec<DetectorCount>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct Overview {
    pub totals: Totals,
    pub by_role: Vec<Share>,
    pub by_concern: Vec<Share>,
    pub by_language: Vec<Share>,
    pub areas: AreaCounts,
    /// What the analysis that needs models did, in plain words: off and why, waiting, failed, or
    /// what it found.
    pub semantic: String,
}

/// Rounding is exact here: counts of lines are far below 2^53.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "sums of counts"
)]
fn whole(value: f64) -> u64 {
    value as u64
}

fn shares(index: &Index, key: &str, total_lines: f64) -> Result<Vec<Share>, QueryError> {
    let terms = [
        Term::new(Base::Lines, Filter::default()),
        Term::new(Base::Tokens, Filter::default()),
    ];
    let mut out: Vec<Share> = index
        .grouped(&[key], &Filter::everything(), &terms)?
        .into_iter()
        .map(|(key, sums)| Share {
            name: key.into_iter().next().flatten().unwrap_or_default(),
            lines: whole(sums[0]),
            tokens: whole(sums[1]),
            share: if total_lines > 0.0 {
                sums[0] / total_lines
            } else {
                0.0
            },
        })
        .collect();
    out.sort_by(|a, b| b.lines.cmp(&a.lines).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

pub(crate) fn overview(index: &Index) -> Result<Overview, QueryError> {
    let terms =
        [Base::Lines, Base::Tokens, Base::Nodes].map(|base| Term::new(base, Filter::default()));
    let sums = index
        .grouped(&[], &Filter::everything(), &terms)?
        .into_iter()
        .next()
        .map_or_else(|| vec![0.0; 3], |(_, sums)| sums);
    let count = |table: &str| -> Result<u64, QueryError> {
        Ok(index.store().connection().query_row(
            &format!("SELECT count(*) FROM {table}"),
            [],
            |row| row.get(0),
        )?)
    };
    let language = format!("coalesce(language, '{OTHER_LANGUAGE}')");
    let areas = index.areas()?;
    let by_detector = crate::detect::DETECTORS
        .iter()
        .map(|detector| DetectorCount {
            detector: detector.name().to_owned(),
            areas: u32::try_from(
                areas
                    .iter()
                    .filter(|a| a.detectors.iter().any(|d| d == detector.name()))
                    .count(),
            )
            .unwrap_or(u32::MAX),
        })
        .collect();
    Ok(Overview {
        totals: Totals {
            files: count("files")?,
            lines: whole(sums[0]),
            tokens: whole(sums[1]),
            nodes: whole(sums[2]),
            symbols: count("symbols")?,
        },
        by_role: shares(index, "role", sums[0])?,
        by_concern: shares(index, "concern", sums[0])?,
        by_language: shares(index, &language, sums[0])?,
        areas: AreaCounts {
            total: u32::try_from(areas.len()).unwrap_or(u32::MAX),
            acknowledged: u32::try_from(areas.iter().filter(|a| a.acknowledged.is_some()).count())
                .unwrap_or(u32::MAX),
            by_detector,
        },
        semantic: index.store().meta("semantic_status")?.unwrap_or_default(),
    })
}

//! `Xtab`: two dimensions crossed, one measure per cell, shaped for a heatmap and a text table.
//!
//! Rows and columns are the values of a dimension, ordered by their totals, largest first. Cells
//! hold the measure; margins hold the row and column totals. Cells with nothing in them are left
//! out. `colorStep` places each cell on the sequential scale over the cells that are present.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::Hidden;
use super::scale::{self, Scale};
use super::tree::SizeInfo;
use crate::measure::{Filter, Measure, NO_SYMBOL, OTHER_LANGUAGE};
use crate::reader::{Index, QueryError};

/// Path components kept in a path axis unless the query says otherwise.
pub const PATH_DEPTH_DEFAULT: u32 = 1;

/// A dimension a cross-tab can be laid out along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "Query.ts")]
pub enum Axis {
    Role,
    Concern,
    Language,
    /// The kind of the innermost symbol a line sits in.
    Kind,
    /// The first `pathDepth` components of the file's path.
    Path,
}

impl Axis {
    fn column(self) -> &'static str {
        match self {
            Self::Role => "role",
            Self::Concern => "concern",
            Self::Language => "language",
            Self::Kind => "symbol_kind",
            Self::Path => "path",
        }
    }

    fn value(self, raw: Option<String>, path_depth: usize) -> String {
        match (self, raw) {
            (Self::Language, None) => OTHER_LANGUAGE.to_owned(),
            (Self::Kind, None) => NO_SYMBOL.to_owned(),
            (Self::Path, Some(path)) => path
                .split('/')
                .take(path_depth)
                .collect::<Vec<_>>()
                .join("/"),
            (_, raw) => raw.unwrap_or_default(),
        }
    }
}

/// One row or column: a dimension value and its total across the other axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct XtabHeader {
    pub name: String,
    pub total: f64,
}

/// A non-empty cell, by position in `rows` and `cols`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct XtabCell {
    pub row: u32,
    pub col: u32,
    pub value: f64,
    /// 0 to `scale.steps - 1`.
    pub color_step: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct Xtab {
    pub row_axis: Axis,
    pub col_axis: Axis,
    /// The depth path axes were cut at.
    pub path_depth: u32,
    pub measure: SizeInfo,
    pub total: f64,
    /// Largest total first.
    pub rows: Vec<XtabHeader>,
    pub cols: Vec<XtabHeader>,
    /// Row-major.
    pub cells: Vec<XtabCell>,
    pub scale: Option<Scale>,
    /// What the default population left out.
    pub hidden: Option<Hidden>,
}

pub(crate) struct Args<'a> {
    pub row_axis: Axis,
    pub col_axis: Axis,
    pub measure: &'a Measure,
    pub label: String,
    pub filter: &'a Filter,
    pub path_depth: u32,
}

fn headers(totals: BTreeMap<String, f64>) -> Vec<XtabHeader> {
    let mut headers: Vec<XtabHeader> = totals
        .into_iter()
        .map(|(name, total)| XtabHeader { name, total })
        .collect();
    headers.sort_by(|a, b| {
        b.total
            .total_cmp(&a.total)
            .then_with(|| a.name.cmp(&b.name))
    });
    headers
}

pub(crate) fn xtab(index: &Index, args: &Args) -> Result<Xtab, QueryError> {
    let Measure::Sum { base, .. } = args.measure else {
        return Err(QueryError::SizeIsRatio);
    };
    let depth = args.path_depth.max(1) as usize;
    let keys = [args.row_axis.column(), args.col_axis.column()];
    let mut cells: BTreeMap<(String, String), f64> = BTreeMap::new();
    let mut row_totals: BTreeMap<String, f64> = BTreeMap::new();
    let mut col_totals: BTreeMap<String, f64> = BTreeMap::new();
    for (key, sums) in index.grouped(&keys, args.filter, &args.measure.terms())? {
        let mut key = key.into_iter();
        let row = args.row_axis.value(key.next().flatten(), depth);
        let col = args.col_axis.value(key.next().flatten(), depth);
        let value = sums[0];
        if value <= 0.0 {
            continue;
        }
        *cells.entry((row.clone(), col.clone())).or_default() += value;
        *row_totals.entry(row).or_default() += value;
        *col_totals.entry(col).or_default() += value;
    }
    let total = row_totals.values().sum();
    let (rows, cols) = (headers(row_totals), headers(col_totals));
    let positions = |list: &[XtabHeader]| -> BTreeMap<String, u32> {
        (0..)
            .zip(list)
            .map(|(i, header)| (header.name.clone(), i))
            .collect()
    };
    let (row_at, col_at) = (positions(&rows), positions(&cols));
    let scale = scale::build(
        &args.label,
        true,
        &cells.values().copied().collect::<Vec<_>>(),
    );
    let mut placed: Vec<XtabCell> = cells
        .into_iter()
        .map(|((row, col), value)| XtabCell {
            row: row_at[&row],
            col: col_at[&col],
            value,
            color_step: scale.as_ref().map_or(0, |scale| scale.step(value)),
        })
        .collect();
    placed.sort_by_key(|cell| (cell.row, cell.col));
    Ok(Xtab {
        row_axis: args.row_axis,
        col_axis: args.col_axis,
        path_depth: args.path_depth.max(1),
        measure: SizeInfo {
            measure: args.label.clone(),
            unit: base.unit().to_owned(),
        },
        total,
        rows,
        cols,
        cells: placed,
        scale,
        hidden: None,
    })
}

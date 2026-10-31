//! Colour scales. The server decides every colour step; the UI only looks them up.
//!
//! Sequential scales cut the value range into equal-interval bins, log-spaced when the range
//! spans more than [`LOG_SPAN`] times. Diverging scales (ratios) are symmetric in log space
//! around 1. Ranges come from the tiles that are drawn without anything inside them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How many bins a scale has. Odd, so a diverging scale has a middle bin around 1:1.
pub const SCALE_STEPS: u32 = 7;
/// A range wider than this ratio of largest to smallest positive value is binned in log space.
const LOG_SPAN: f64 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "Answer.ts")]
pub enum ScaleKind {
    Sequential,
    Diverging,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "Answer.ts")]
pub enum Spacing {
    Linear,
    Log,
}

/// What the legend needs: the measure, the range, and where each bin starts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct Scale {
    pub kind: ScaleKind,
    pub spacing: Spacing,
    pub steps: u32,
    pub measure: String,
    pub min: f64,
    pub max: f64,
    /// The value in the middle of a diverging scale.
    pub midpoint: Option<f64>,
    /// The lower bound of each step, then the upper bound of the last: `steps + 1` values.
    pub edges: Vec<f64>,
}

impl Scale {
    /// The step a value falls in.
    #[must_use]
    pub fn step(&self, value: f64) -> u32 {
        let found = self
            .edges
            .iter()
            .skip(1)
            .take(self.steps as usize - 1)
            .take_while(|&&edge| value >= edge)
            .count();
        u32::try_from(found).unwrap_or(0)
    }
}

/// Builds the scale for the values of the drawn tiles. `None` when no tile has a value.
#[must_use]
pub fn build(measure: &str, diverging: bool, values: &[f64]) -> Option<Scale> {
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return None;
    }
    let steps = SCALE_STEPS;
    let positive_min = finite
        .iter()
        .copied()
        .filter(|v| *v > 0.0)
        .fold(f64::INFINITY, f64::min);
    let max = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if diverging {
        let reach = finite
            .iter()
            .filter(|v| **v > 0.0)
            .map(|v| v.ln().abs())
            .fold(0.0, f64::max)
            .max(2_f64.ln());
        let edges = (0..=steps)
            .map(|i| (-reach + 2.0 * reach * f64::from(i) / f64::from(steps)).exp())
            .collect();
        return Some(Scale {
            kind: ScaleKind::Diverging,
            spacing: Spacing::Log,
            steps,
            measure: measure.to_owned(),
            min: (-reach).exp(),
            max: reach.exp(),
            midpoint: Some(1.0),
            edges,
        });
    }
    let min = finite.iter().copied().fold(f64::INFINITY, f64::min);
    let log = positive_min.is_finite() && max / positive_min > LOG_SPAN;
    let (spacing, low, edges) = if log {
        let edges = (0..=steps)
            .map(|i| positive_min * (max / positive_min).powf(f64::from(i) / f64::from(steps)))
            .collect();
        (Spacing::Log, positive_min, edges)
    } else {
        let edges = (0..=steps)
            .map(|i| min + (max - min) * f64::from(i) / f64::from(steps))
            .collect();
        (Spacing::Linear, min, edges)
    };
    Some(Scale {
        kind: ScaleKind::Sequential,
        spacing,
        steps,
        measure: measure.to_owned(),
        min: low,
        max,
        midpoint: None,
        edges,
    })
}

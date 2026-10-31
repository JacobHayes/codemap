//! Robust statistics shared by the detectors that judge a subject against its peers.

/// Scales the median absolute deviation to a standard deviation for normal data.
const MAD_SCALE: f64 = 0.6745;

/// The middle of an ascending sample (the mean of the two middle values when even).
pub(crate) fn median(sorted: &[f64]) -> f64 {
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        f64::midpoint(sorted[mid - 1], sorted[mid])
    } else {
        sorted[mid]
    }
}

/// The modified z-score of every value, `0.6745 * (x - median) / MAD`, and the median. The
/// deviation is floored at `floor` so a uniform sample does not turn small differences into
/// outliers. Empty values give an empty answer.
pub(crate) fn modified_z(values: &[f64], floor: f64) -> (Vec<f64>, f64) {
    if values.is_empty() {
        return (Vec::new(), 0.0);
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let centre = median(&sorted);
    let mut deviations: Vec<f64> = values.iter().map(|v| (v - centre).abs()).collect();
    deviations.sort_by(f64::total_cmp);
    let mad = median(&deviations).max(floor);
    let scores = values
        .iter()
        .map(|v| MAD_SCALE * (v - centre) / mad)
        .collect();
    (scores, centre)
}

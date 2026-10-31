//! Detectors, ranking and the query layer, on synthetic trees written for each test.

#![expect(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::format_collect,
    clippy::assert_is_empty,
    reason = "a failed test is the intended outcome of a bad unwrap, and helpers are not tests"
)]

mod common;
mod detectors;
mod queries;
mod similarity;

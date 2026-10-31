//! `codemap index` through the library: languages, hash-and-diff, and incremental equivalence.
#![expect(
    clippy::unwrap_used,
    reason = "a failed test is the intended outcome of a bad unwrap, and helpers are not tests"
)]

mod common;
mod ignored;
mod incremental;
mod languages;
mod worktree;

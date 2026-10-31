//! The plain-text extractor: tokens per line for every file no language claims.

use codemap_lang::{Concern, Role};

use crate::facts::{LineFact, split_lines, token_count};

pub fn extract(text: &str, role: Role) -> Vec<LineFact> {
    split_lines(text)
        .into_iter()
        .map(|line| LineFact {
            token_count: token_count(line),
            node_count: 0,
            depth: 0,
            role,
            concern: Concern::Core,
            symbol: None,
        })
        .collect()
}

//! The query layer: one `Query` in, one `Answer` out. Every surface (CLI, MCP, web UI) is a thin
//! shell over it.

mod facts;
mod index;
mod role;
mod store;
mod syntax;
mod text;

pub use facts::{FileFacts, LineFact, RefFact, SymbolFact, SymbolKind, extract};
pub use index::{IndexError, IndexReport, index};
pub use role::role_for_path;
pub use store::STATE_DIR;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A typed question. Adding a variant is a compile error in every surface until it is handled.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "Query.ts")]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Query {
    /// Totals for the whole index.
    Overview,
}

/// The answer to a [`Query`]. A placeholder until the index exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "Answer.ts")]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Answer {
    /// There is no index yet, so there is nothing to say.
    Placeholder { note: String },
}

/// Answers a query.
#[must_use]
pub fn answer(query: &Query) -> Answer {
    match query {
        Query::Overview => Answer::Placeholder {
            note: "no index yet".to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_is_answered_with_a_placeholder() {
        assert!(matches!(
            answer(&Query::Overview),
            Answer::Placeholder { .. }
        ));
    }
}

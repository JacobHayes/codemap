//! TypeScript and TSX: the JavaScript table plus the grammars' own tags query.

use crate::Language;
use crate::javascript::{ASSERTION_PREFIXES, KINDS, LOGGING_CALLEES, mark};

pub static TYPESCRIPT: Language = Language {
    name: "typescript",
    extensions: &["ts", "mts", "cts"],
    grammar: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    kinds: KINDS,
    // The TypeScript query adds interfaces and signatures to the JavaScript one.
    tags_query: tree_sitter_javascript::TAGS_QUERY,
    extra_tags_query: tree_sitter_typescript::TAGS_QUERY,
    callee_fields: &["function"],
    logging_callees: LOGGING_CALLEES,
    assertion_prefixes: ASSERTION_PREFIXES,
    test_stem_prefixes: &[],
    test_stem_suffixes: &[],
    mark,
};

pub static TSX: Language = Language {
    name: "tsx",
    extensions: &["tsx"],
    grammar: || tree_sitter_typescript::LANGUAGE_TSX.into(),
    ..TYPESCRIPT
};

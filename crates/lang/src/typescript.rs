//! TypeScript and TSX: the JavaScript table plus the grammars' own tags query.

use crate::Language;
use crate::javascript::{ASSERTION_PREFIXES, CASE_LABEL_FIELDS, KINDS, LOGGING_CALLEES, mark};

pub static TYPESCRIPT: Language = Language {
    name: "typescript",
    extensions: &["ts", "mts", "cts"],
    grammar: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    kinds: KINDS,
    // The TypeScript query adds interfaces and signatures to the JavaScript one.
    tags_query: tree_sitter_javascript::TAGS_QUERY,
    // Heritage clauses are not in the grammar's query: `implements` and `extends` both make a
    // class an implementor of what they name.
    extra_tags_queries: &[
        tree_sitter_typescript::TAGS_QUERY,
        "(class_declaration (class_heritage (implements_clause [(type_identifier) @name (generic_type name: (type_identifier) @name)]))) @reference.implementation
        (class_declaration (class_heritage (extends_clause value: (identifier) @name))) @reference.implementation
        (abstract_class_declaration (class_heritage (implements_clause [(type_identifier) @name (generic_type name: (type_identifier) @name)]))) @reference.implementation
        (abstract_class_declaration (class_heritage (extends_clause value: (identifier) @name))) @reference.implementation",
    ],
    case_label_fields: CASE_LABEL_FIELDS,
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

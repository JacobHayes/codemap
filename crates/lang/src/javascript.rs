//! JavaScript, and the table TypeScript and TSX share: a node kind a grammar lacks is skipped,
//! so one table serves all three.

use crate::{Concern, Kind, Language, Mark, Node};

pub(crate) const KINDS: &[(&str, Kind)] = &[
    ("function_declaration", Kind::Function),
    ("generator_function_declaration", Kind::Function),
    ("function_expression", Kind::Function),
    ("generator_function", Kind::Function),
    ("arrow_function", Kind::Function),
    ("method_definition", Kind::Function),
    ("class_declaration", Kind::Type),
    ("class", Kind::Type),
    ("abstract_class_declaration", Kind::Abstraction),
    ("interface_declaration", Kind::Abstraction),
    ("enum_declaration", Kind::Type),
    ("type_alias_declaration", Kind::Type),
    ("internal_module", Kind::Module),
    ("program", Kind::Module),
    ("if_statement", Kind::Conditional),
    ("switch_statement", Kind::Switch),
    ("else_clause", Kind::Branch),
    ("ternary_expression", Kind::Branch),
    ("switch_case", Kind::Branch),
    ("switch_default", Kind::Branch),
    ("for_statement", Kind::Loop),
    ("for_in_statement", Kind::Loop),
    ("while_statement", Kind::Loop),
    ("do_statement", Kind::Loop),
    ("catch_clause", Kind::Handler),
    ("finally_clause", Kind::Handler),
    ("call_expression", Kind::Call),
    ("comment", Kind::Comment),
    ("string", Kind::String),
    ("template_string", Kind::String),
    ("import_statement", Kind::Import),
];

pub(crate) const CASE_LABEL_FIELDS: &[&str] = &["value"];

pub(crate) const LOGGING_CALLEES: &[&str] =
    &["trace", "debug", "info", "warn", "error", "fatal", "log"];

/// `expect(..)`, `assert*(..)` and the matcher that ends an `expect(..).toBe(..)` chain.
pub(crate) const ASSERTION_PREFIXES: &[&str] = &[
    "assert",
    "expect",
    "toBe",
    "toEqual",
    "toStrictEqual",
    "toHave",
    "toMatch",
    "toContain",
    "toThrow",
    "strictEqual",
    "deepEqual",
    "deepStrictEqual",
];

/// Test runners (vitest, jest, mocha) declare tests as calls: `it("name", () => ..)`,
/// `test.only(..)` and `it.each(table)("name", ..)`. A suite is a group, a case a test, named
/// by its string (Playwright's `test.describe` is a suite too).
pub(crate) const TEST_TAGS_QUERIES: &[&str] = &[
    r#"(call_expression
        function: [
          (identifier) @_runner
          (member_expression object: (identifier) @_runner property: (property_identifier) @_modifier)
          (call_expression function: (member_expression object: (identifier) @_runner property: (property_identifier) @_modifier))
        ]
        arguments: (arguments . (string) @name . [(arrow_function) (function_expression)])
        (#match? @_runner "^(it|test)$")
        (#not-match? @_modifier "^(describe|suite)$")) @definition.test"#,
    r#"(call_expression
        function: [
          (identifier) @_runner
          (member_expression object: (identifier) @_runner property: (property_identifier) @_modifier)
          (call_expression function: (member_expression object: (identifier) @_runner property: (property_identifier) @_modifier))
        ]
        arguments: (arguments . (string) @name . [(arrow_function) (function_expression)])
        (#match? @_runner "^(describe|suite)$")) @definition.group"#,
    r#"(call_expression
        function: (member_expression object: (identifier) @_runner property: (property_identifier) @_modifier)
        arguments: (arguments . (string) @name . [(arrow_function) (function_expression)])
        (#eq? @_runner "test")
        (#match? @_modifier "^(describe|suite)$")) @definition.group"#,
];

pub static JAVASCRIPT: Language = Language {
    name: "javascript",
    extensions: &["js", "jsx", "mjs", "cjs"],
    grammar: || tree_sitter_javascript::LANGUAGE.into(),
    kinds: KINDS,
    tags_query: tree_sitter_javascript::TAGS_QUERY,
    extra_tags_queries: &[
        "(class_declaration (class_heritage (identifier) @name)) @reference.implementation",
        TEST_TAGS_QUERIES[0],
        TEST_TAGS_QUERIES[1],
        TEST_TAGS_QUERIES[2],
    ],
    case_label_fields: CASE_LABEL_FIELDS,
    callee_fields: &["function"],
    logging_callees: LOGGING_CALLEES,
    assertion_prefixes: ASSERTION_PREFIXES,
    test_stem_prefixes: &[],
    test_stem_suffixes: &[],
    mark,
};

/// The callback of `promise.catch(..)` or `.finally(..)` is handling.
pub(crate) fn mark(node: &Node, source: &[u8]) -> Option<Mark> {
    if node.kind() != "arguments" {
        return None;
    }
    let callee = node.parent()?.child_by_field_name("function")?;
    let property = callee.child_by_field_name("property")?;
    matches!(property.utf8_text(source).ok()?, "catch" | "finally")
        .then_some(Mark::Concern(Concern::Handler))
}

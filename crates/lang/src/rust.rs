//! Rust: the table and its exceptions (test attributes and test modules).

use crate::{Concern, Kind, Language, Mark, Node, Role};

pub static RUST: Language = Language {
    name: "rust",
    extensions: &["rs"],
    grammar: || tree_sitter_rust::LANGUAGE.into(),
    kinds: &[
        ("function_item", Kind::Function),
        ("struct_item", Kind::Type),
        ("enum_item", Kind::Type),
        ("union_item", Kind::Type),
        ("trait_item", Kind::Type),
        ("type_item", Kind::Type),
        ("mod_item", Kind::Module),
        ("if_expression", Kind::Branch),
        ("match_arm", Kind::Branch),
        ("for_expression", Kind::Loop),
        ("while_expression", Kind::Loop),
        ("loop_expression", Kind::Loop),
        ("call_expression", Kind::Call),
        ("macro_invocation", Kind::Call),
        ("line_comment", Kind::Comment),
        ("block_comment", Kind::Comment),
        ("string_literal", Kind::String),
        ("raw_string_literal", Kind::String),
        ("use_declaration", Kind::Import),
    ],
    tags_query: tree_sitter_rust::TAGS_QUERY,
    callee_fields: &["function", "macro"],
    logging_callees: &["trace", "debug", "info", "warn", "error", "log", "eprintln"],
    assertion_prefixes: &["assert", "debug_assert"],
    test_stem_prefixes: &[],
    test_stem_suffixes: &["_test", "_tests"],
    mark,
};

fn mark(node: &Node, source: &[u8]) -> Option<Mark> {
    match node.kind() {
        "attribute_item" if is_test_attribute(node, source) => Some(Mark::RoleThrough(Role::Test)),
        "function_item" if has_test_attribute(node, source) => Some(Mark::TestFunction),
        "mod_item" if module_name(node, source) == Some("tests") => Some(Mark::Role(Role::Test)),
        // An arm that matches the error case of a `Result`.
        "match_arm" if arm_pattern_is_err(node, source) => Some(Mark::Concern(Concern::Handler)),
        _ => None,
    }
}

fn text<'a>(node: &Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn module_name<'a>(node: &Node, source: &'a [u8]) -> Option<&'a str> {
    Some(text(&node.child_by_field_name("name")?, source))
}

fn arm_pattern_is_err(node: &Node, source: &[u8]) -> bool {
    node.child_by_field_name("pattern")
        .is_some_and(|pattern| text(&pattern, source).starts_with("Err("))
}

/// `#[test]`, `#[cfg(test)]` and path variants such as `#[tokio::test]`.
fn is_test_attribute(node: &Node, source: &[u8]) -> bool {
    let attribute = text(node, source);
    let body = attribute.trim_start_matches("#[").trim_end_matches(']');
    body == "test" || body.ends_with("::test") || body.replace(' ', "") == "cfg(test)"
}

fn has_test_attribute(node: &Node, source: &[u8]) -> bool {
    let mut previous = node.prev_named_sibling();
    while let Some(sibling) = previous {
        match sibling.kind() {
            "attribute_item" if is_test_attribute(&sibling, source) => return true,
            "attribute_item" | "line_comment" => previous = sibling.prev_named_sibling(),
            _ => return false,
        }
    }
    false
}

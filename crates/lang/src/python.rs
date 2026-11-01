//! Python: the table and its exceptions (docstrings, test names).

use crate::{Concern, Kind, Language, Mark, Node, Role};

pub static PYTHON: Language = Language {
    name: "python",
    type_scope: None,
    extensions: &["py", "pyi"],
    grammar: || tree_sitter_python::LANGUAGE.into(),
    kinds: &[
        ("function_definition", Kind::Function),
        ("class_definition", Kind::Type),
        ("module", Kind::Module),
        ("if_statement", Kind::Conditional),
        ("elif_clause", Kind::Conditional),
        ("else_clause", Kind::Branch),
        ("match_statement", Kind::Switch),
        ("for_statement", Kind::Loop),
        ("while_statement", Kind::Loop),
        ("except_clause", Kind::Handler),
        ("except_group_clause", Kind::Handler),
        ("finally_clause", Kind::Handler),
        ("assert_statement", Kind::Assertion),
        ("call", Kind::Call),
        ("comment", Kind::Comment),
        ("string", Kind::String),
        ("import_statement", Kind::Import),
        ("import_from_statement", Kind::Import),
        ("future_import_statement", Kind::Import),
    ],
    tags_query: tree_sitter_python::TAGS_QUERY,
    extra_tags_queries: &[
        "(class_definition superclasses: (argument_list [(identifier) @name (attribute attribute: (identifier) @name)])) @reference.implementation",
    ],
    case_label_fields: &["case_pattern"],
    case_names_bind: true,
    abort_callees: &[],
    case_fallthrough: false,
    capitalised_constructors: true,
    callee_fields: &["function"],
    logging_callees: &[
        "debug",
        "info",
        "warning",
        "warn",
        "error",
        "exception",
        "critical",
        "log",
        "print",
    ],
    assertion_prefixes: &["assert"],
    test_stem_prefixes: &["test_"],
    test_stem_suffixes: &["_test"],
    generated_header: None,
    mark,
};

fn mark(node: &Node, source: &[u8]) -> Option<Mark> {
    match node.kind() {
        "function_definition" if name_of(node, source).starts_with("test") => {
            Some(Mark::TestFunction)
        }
        "class_definition" if name_of(node, source).starts_with("Test") => {
            Some(Mark::Role(Role::Test))
        }
        "class_definition" if is_abstract(node, source) => Some(Mark::Abstraction),
        "expression_statement" if is_docstring(node) => Some(Mark::Concern(Concern::Comment)),
        _ => None,
    }
}

fn name_of<'a>(node: &Node, source: &'a [u8]) -> &'a str {
    node.child_by_field_name("name")
        .and_then(|name| name.utf8_text(source).ok())
        .unwrap_or("")
}

/// A lone string that opens a module, class or function body.
fn is_docstring(node: &Node) -> bool {
    let lone_string = node.named_child_count() == 1
        && node
            .named_child(0)
            .is_some_and(|child| child.kind() == "string");
    let first = node
        .prev_named_sibling()
        .is_none_or(|previous| previous.kind() == "comment");
    let parent = node.parent();
    let in_body = parent.is_some_and(|parent| match parent.kind() {
        "module" => true,
        "block" => parent.parent().is_some_and(|owner| {
            matches!(owner.kind(), "function_definition" | "class_definition")
        }),
        _ => false,
    });
    lone_string && first && in_body
}

/// A class deriving from `ABC` or `Protocol`, or built by `ABCMeta`.
fn is_abstract(node: &Node, source: &[u8]) -> bool {
    let bases = node
        .child_by_field_name("superclasses")
        .and_then(|bases| bases.utf8_text(source).ok())
        .unwrap_or("");
    bases
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|word| matches!(word, "ABC" | "ABCMeta" | "Protocol"))
}

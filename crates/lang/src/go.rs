//! Go: the table and its exceptions (test functions, interfaces, `if err != nil`).

use crate::{Concern, GeneratedHeader, Kind, Language, Mark, Node};

/// A method continues the type it is declared on, whichever file or order: the receiver's type
/// name is the extension's name, with pointers and type arguments stripped.
const METHOD_EXTENDS_TYPE: &str = "(method_declaration receiver: (parameter_list (parameter_declaration type: [(type_identifier) @name (pointer_type (type_identifier) @name) (generic_type type: (type_identifier) @name) (pointer_type (generic_type type: (type_identifier) @name))]))) @extension.class";

/// An embedded field (a field with a type and no name) promotes the embedded type's methods, so
/// the struct extends (or, for an interface, implements) it. Go has no `implements` clause.
const EMBEDDED_FIELD: &str = "(type_spec type: (struct_type (field_declaration_list (field_declaration !name type: [(type_identifier) @name (pointer_type (type_identifier) @name) (qualified_type name: (type_identifier) @name) (generic_type type: (type_identifier) @name)])))) @reference.implementation";

pub static GO: Language = Language {
    name: "go",
    extensions: &["go"],
    grammar: || tree_sitter_go::LANGUAGE.into(),
    kinds: &[
        ("function_declaration", Kind::Function),
        ("method_declaration", Kind::Function),
        ("type_spec", Kind::Type),
        ("type_alias", Kind::Type),
        ("if_statement", Kind::Conditional),
        ("expression_switch_statement", Kind::Switch),
        ("type_switch_statement", Kind::Switch),
        ("select_statement", Kind::Switch),
        ("expression_case", Kind::Branch),
        ("type_case", Kind::Branch),
        ("communication_case", Kind::Branch),
        ("default_case", Kind::Branch),
        ("for_statement", Kind::Loop),
        ("call_expression", Kind::Call),
        ("comment", Kind::Comment),
        ("interpreted_string_literal", Kind::String),
        ("raw_string_literal", Kind::String),
        ("import_declaration", Kind::Import),
    ],
    tags_query: tree_sitter_go::TAGS_QUERY,
    // The grammar's query tags types as `definition.type`, which is not a symbol kind, and
    // never reads methods as continuing their receiver.
    extra_tags_queries: &[
        "(type_spec name: (type_identifier) @name) @definition.class",
        "(type_alias name: (type_identifier) @name) @definition.class",
        METHOD_EXTENDS_TYPE,
        EMBEDDED_FIELD,
    ],
    case_label_fields: &["value", "type"],
    case_names_bind: false,
    callee_fields: &["function"],
    // `Error` is left out: `err.Error()` renders a message, it does not log one.
    logging_callees: &[
        "Print", "Printf", "Println", "Fatal", "Fatalf", "Fatalln", "Debug", "Debugf", "Info",
        "Infof", "Warn", "Warnf", "Warning", "Warningf", "Log", "Logf",
    ],
    assertion_prefixes: &["Assert", "Require", "assert", "require"],
    test_stem_prefixes: &[],
    test_stem_suffixes: &["_test"],
    // Generators put this line above the package clause, below any licence header.
    generated_header: Some(GeneratedHeader {
        line_prefix: "// Code generated ",
        line_suffix: "DO NOT EDIT.",
        ends_before: "package ",
    }),
    mark,
};

fn mark(node: &Node, source: &[u8]) -> Option<Mark> {
    match node.kind() {
        "function_declaration" if is_test_function(node, source) => Some(Mark::TestFunction),
        "type_spec" if is_interface(node) => Some(Mark::Abstraction),
        "block" if is_error_handling(node, source) => Some(Mark::Concern(Concern::Handler)),
        _ => None,
    }
}

fn text<'a>(node: &Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

/// `func TestX(t *testing.T)`, and the benchmark and fuzz forms: the name convention plus the
/// `testing` parameter, so a production function called `TestConnection` is not a test.
fn is_test_function(node: &Node, source: &[u8]) -> bool {
    let name = node
        .child_by_field_name("name")
        .map_or("", |name| text(&name, source));
    let named = ["Test", "Benchmark", "Fuzz"].iter().any(|prefix| {
        name.strip_prefix(prefix)
            .is_some_and(|rest| !rest.starts_with(char::is_lowercase))
    });
    named
        && node
            .child_by_field_name("parameters")
            .is_some_and(|parameters| text(&parameters, source).contains("testing."))
}

/// A type spec whose type is an interface.
fn is_interface(spec: &Node) -> bool {
    spec.child_by_field_name("type")
        .is_some_and(|kind| kind.kind() == "interface_type")
}

/// The block of `if err != nil { .. }`, or the `else` of `if err == nil { .. } else { .. }`.
fn is_error_handling(block: &Node, source: &[u8]) -> bool {
    let Some(parent) = block.parent().filter(|p| p.kind() == "if_statement") else {
        return false;
    };
    let Some(condition) = parent.child_by_field_name("condition") else {
        return false;
    };
    let checks = |operator: &str| compares_error_with_nil(&condition, source, operator);
    if parent.child_by_field_name("consequence") == Some(*block) {
        checks("!=")
    } else {
        parent.child_by_field_name("alternative") == Some(*block) && checks("==")
    }
}

/// `err <operator> nil` where the left side is named like an error.
fn compares_error_with_nil(condition: &Node, source: &[u8], operator: &str) -> bool {
    let operands = (
        condition.child_by_field_name("left"),
        condition.child_by_field_name("operator"),
        condition.child_by_field_name("right"),
    );
    let (Some(left), Some(op), Some(right)) = operands else {
        return false;
    };
    let name = text(&left, source).rsplit('.').next().unwrap_or("");
    text(&op, source) == operator
        && text(&right, source) == "nil"
        && (name.starts_with("err") || name.ends_with("Err") || name.ends_with("err"))
}

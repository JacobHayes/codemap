//! Syntax shapes the detectors need and a language-neutral model cannot express as lines:
//! functions that only forward to another function, and the case sets of `match`, `switch` and
//! `if` chains. Both read the generic kinds of a language's table, never node names of their own.

use std::collections::BTreeSet;

use codemap_lang::{Kind, Language, Node};

use crate::arms;
use crate::facts::SwitchFact;

/// A function whose body has more than this many nodes beyond its one call (the block and a
/// `return`, `await` or `?` around it) is doing more than forwarding.
const FORWARD_EXTRA_NODES_MAX: u32 = 3;
/// Node kinds containing these compute something: a body with one is not forwarding.
/// So do statements that schedule, defer or hand off the call rather than make it.
const COMPUTING: &[&str] = &[
    "binary",
    "unary",
    "ternary",
    "operator",
    "assignment",
    "send_statement",
    "go_statement",
    "defer_statement",
];
/// Comparisons against the same expression that make an `if` chain a switch in disguise.
const CHAIN_COMPARISONS_MIN: usize = 3;
/// How deep below an `if` condition a comparison is looked for (parentheses, `not`, `and`).
const COMPARISON_DEPTH_MAX: usize = 4;

pub(crate) fn kind_of(kinds: &[Option<Kind>], node: &Node) -> Option<Kind> {
    if node.is_named() {
        kinds.get(usize::from(node.kind_id())).copied().flatten()
    } else {
        None
    }
}

fn line(row: usize) -> u32 {
    u32::try_from(row + 1).unwrap_or(u32::MAX)
}

pub(crate) fn text<'a>(node: &Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

/// What a body is made of, counted outside the one call it may hold.
#[derive(Default)]
struct Body {
    calls: u32,
    extra_nodes: u32,
    /// Something a forwarder does not contain: a branch, loop, handler, switch or nested function.
    logic: bool,
}

impl Body {
    fn scan(&mut self, kinds: &[Option<Kind>], node: &Node, in_call: bool) {
        let kind = kind_of(kinds, node);
        if COMPUTING.iter().any(|word| node.kind().contains(word)) {
            self.logic = true;
        }
        match kind {
            Some(Kind::Comment | Kind::String) => return,
            Some(
                Kind::Branch
                | Kind::Conditional
                | Kind::Switch
                | Kind::Loop
                | Kind::Handler
                | Kind::Function
                | Kind::Type
                | Kind::Abstraction,
            ) => self.logic = true,
            Some(Kind::Call) => self.calls += 1,
            _ => {}
        }
        if node.is_named() && !in_call && kind != Some(Kind::Call) && !is_list_wrapper(node) {
            self.extra_nodes += 1;
        }
        let inside = in_call || kind == Some(Kind::Call);
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.scan(kinds, &child, inside);
        }
    }
}

/// A node that only holds one other node and spans exactly it (Go's `statement_list` and
/// `expression_list`): it adds nothing of its own.
fn is_list_wrapper(node: &Node) -> bool {
    node.named_child_count() == 1
        && node
            .named_child(0)
            .is_some_and(|only| only.byte_range() == node.byte_range())
}

/// The identifiers a subtree names: `self` and `this` count, so do plain and type identifiers.
/// In `binding` position (a parameter list) a destructured name counts too.
fn identifiers<'a>(node: &Node, source: &'a [u8], binding: bool, out: &mut BTreeSet<&'a str>) {
    let kind = node.kind();
    let named = if binding {
        kind.contains("identifier")
    } else {
        kind.ends_with("identifier")
    };
    if node.child_count() == 0 && (named || matches!(kind, "self" | "this")) {
        out.insert(text(node, source));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        identifiers(&child, source, binding, out);
    }
}

/// The last name of a callee when the name says what it is: `a::b::<T>::c` and `self.c` are
/// `c`, and so is `module.c` for a plain identifier that is not a parameter or a constant. A call through
/// anything else (`self.items.c`, `argument.c`) goes to a type the names cannot tell.
fn callee_name(
    language: &Language,
    call: &Node,
    source: &[u8],
    parameters: &BTreeSet<&str>,
) -> Option<String> {
    let callee = language
        .callee_fields
        .iter()
        .find_map(|field| call.child_by_field_name(field))?;
    let callee = text(&callee, source);
    let callee = strip_generics(callee);
    let callee = callee.as_str();
    if let Some((receiver, _)) = callee.rsplit_once('.') {
        let receiver = receiver.trim_end_matches('?');
        let plain = receiver.chars().all(|c| c.is_alphanumeric() || c == '_');
        // `KINDS.includes` calls a method of a constant, not a function of a module.
        let constant = receiver
            .chars()
            .all(|c| c.is_uppercase() || c.is_ascii_digit() || c == '_');
        let own = matches!(receiver, "self" | "this" | "cls");
        let module = plain && !constant && !parameters.contains(receiver);
        if !(own || module) {
            return None;
        }
    }
    callee
        .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
        .find(|part| !part.is_empty())
        .map(str::to_owned)
}

/// The text without generic argument lists: `Vec::<u8>::from` is `Vec::::from`.
fn strip_generics(text: &str) -> String {
    let mut depth = 0_u32;
    text.chars()
        .filter(|&c| match c {
            '<' => {
                depth += 1;
                false
            }
            '>' => {
                depth = depth.saturating_sub(1);
                false
            }
            _ => depth == 0,
        })
        .collect()
}

/// The name of the function a function only forwards to: its body is one call whose arguments
/// are its own parameters, with at most [`FORWARD_EXTRA_NODES_MAX`] other nodes. A string among
/// the arguments adds something of its own (`(e) => pinned(e, "pinned")` picks a case), so that
/// call is not forwarding.
pub(crate) fn forwarded_callee(
    language: &Language,
    kinds: &[Option<Kind>],
    function: &Node,
    source: &[u8],
) -> Option<String> {
    // `const wrap = (x) => sink(x)` is a declarator whose value is the callable.
    let function = if function.child_by_field_name("body").is_none() {
        function.child_by_field_name("value")?
    } else {
        *function
    };
    let function = &function;
    let body = function.child_by_field_name("body")?;
    let mut shape = Body::default();
    shape.scan(kinds, &body, false);
    if shape.calls != 1 || shape.logic || shape.extra_nodes > FORWARD_EXTRA_NODES_MAX {
        return None;
    }
    if kind_of(kinds, &body) != Some(Kind::Call) && statement_count(kinds, &body) > 1 {
        return None;
    }
    let call = first_call(kinds, &body)?;
    let arguments = call.child_by_field_name("arguments")?;
    if passes_string(kinds, &arguments) {
        return None;
    }
    let mut passed = BTreeSet::new();
    identifiers(&arguments, source, false, &mut passed);
    // A destructured parameter is a name picked out of an argument, not an argument: passing it
    // on is a step of work. Only the names of plain parameters count as passed on.
    let mut plain = BTreeSet::new();
    let mut own = BTreeSet::new();
    for field in ["parameters", "parameter"] {
        if let Some(parameters) = function.child_by_field_name(field) {
            identifiers(&parameters, source, false, &mut plain);
            identifiers(&parameters, source, true, &mut own);
        }
    }
    // Passing nothing on is not forwarding unless there was nothing to pass. A call whose
    // arguments are all literals passes nothing of the function's own on.
    let receivers = ["self", "this", "cls"];
    let has_arguments = own.iter().any(|name| !receivers.contains(name));
    let passes_arguments = passed.iter().any(|name| !receivers.contains(name));
    let only_literals = passed.is_empty() && arguments.named_child_count() > 0;
    (passed.is_subset(&plain) && (passes_arguments || !has_arguments) && !only_literals)
        .then(|| callee_name(language, &call, source, &own))
        .flatten()
}

/// Whether a string is passed as an argument itself; one inside a type assertion is erased.
fn passes_string(kinds: &[Option<Kind>], arguments: &Node) -> bool {
    let mut cursor = arguments.walk();
    arguments
        .named_children(&mut cursor)
        .any(|argument| kind_of(kinds, &argument) == Some(Kind::String))
}

/// Statements of a body, not counting comments and a lone string (a docstring).
fn statement_count(kinds: &[Option<Kind>], body: &Node) -> usize {
    let mut cursor = body.walk();
    body.named_children(&mut cursor)
        .filter(|statement| {
            let mut inner = statement.walk();
            let mut parts = statement.named_children(&mut inner);
            let lone_string = parts
                .next()
                .is_some_and(|part| kind_of(kinds, &part) == Some(Kind::String))
                && parts.next().is_none();
            !matches!(kind_of(kinds, statement), Some(Kind::Comment)) && !lone_string
        })
        .count()
}

fn first_call<'t>(kinds: &[Option<Kind>], node: &Node<'t>) -> Option<Node<'t>> {
    if kind_of(kinds, node) == Some(Kind::Call) {
        return Some(*node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(|child| first_call(kinds, &child))
}

/// Case sets of every `match`/`switch` and of every `if` chain comparing one expression with
/// at least [`CHAIN_COMPARISONS_MIN`] constants.
pub(crate) fn switches(
    language: &Language,
    kinds: &[Option<Kind>],
    root: &Node,
    source: &[u8],
) -> Vec<SwitchFact> {
    let mut found = Vec::new();
    let mut in_chain = BTreeSet::new();
    let mut stack = vec![*root];
    while let Some(node) = stack.pop() {
        let facts = match kind_of(kinds, &node) {
            Some(Kind::Switch) => {
                let (labels, last) = switch_labels(language, kinds, &node, source);
                Some((labels, last, arms::classify(language, kinds, &node, source)))
            }
            Some(Kind::Conditional) if !in_chain.contains(&node.id()) => {
                chain_labels(kinds, &node, source, &mut in_chain).map(|chain| {
                    let shape =
                        arms::classify_chain(language, kinds, &chain.links, chain.closing, source);
                    (chain.labels, chain.last, shape)
                })
            }
            _ => None,
        };
        if let Some((labels, last, shape)) = facts {
            let labels = normalise(&labels, language.case_names_bind);
            if !labels.is_empty() {
                found.push(SwitchFact {
                    symbol: None,
                    start_line: line(node.start_position().row),
                    end_line: line(last.end_position().row),
                    labels,
                    wildcard: shape.wildcard,
                    table: shape.table,
                    degenerate: shape.degenerate,
                    duplicated: shape.duplicated,
                });
            }
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    found.sort_by_key(|fact| (fact.start_line, fact.end_line));
    found
}

/// The cases of a switch: kept in a `body`, or directly among its children (Go).
pub(crate) fn case_nodes<'t>(kinds: &[Option<Kind>], switch: &Node<'t>) -> Vec<Node<'t>> {
    let body = switch.child_by_field_name("body");
    let cases = body.as_ref().unwrap_or(switch);
    let mut cursor = cases.walk();
    cases
        .named_children(&mut cursor)
        .filter(|case| match kind_of(kinds, case) {
            Some(Kind::Comment) => false,
            kind => body.is_some() || kind == Some(Kind::Branch),
        })
        .collect()
}

fn switch_labels<'t>(
    language: &Language,
    kinds: &[Option<Kind>],
    switch: &Node<'t>,
    source: &[u8],
) -> (Vec<String>, Node<'t>) {
    let labels = case_nodes(kinds, switch)
        .iter()
        .flat_map(|case| case_labels(language, case, source))
        .collect();
    (labels, *switch)
}

/// The nodes a case matches: the first of the language's label fields that exists.
pub(crate) fn label_nodes<'t>(language: &Language, case: &Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = case.walk();
    language
        .case_label_fields
        .iter()
        .find_map(|name| {
            let fields: Vec<Node> = case.children_by_field_name(name, &mut cursor).collect();
            if fields.is_empty() {
                let mut inner = case.walk();
                case.named_children(&mut inner)
                    .find(|child| child.kind() == *name)
                    .map(|child| vec![child])
            } else {
                Some(fields)
            }
        })
        .unwrap_or_default()
}

/// What one case matches, split into its parts when it is a list (`case a, b:`, or a type case
/// naming several types).
fn case_labels(language: &Language, case: &Node, source: &[u8]) -> Vec<String> {
    let mut labels = Vec::new();
    for label in label_nodes(language, case) {
        if label.kind().ends_with("_list") {
            let mut inner = label.walk();
            labels.extend(
                label
                    .named_children(&mut inner)
                    .map(|part| text(&part, source).to_owned()),
            );
        } else if label.is_named() {
            labels.push(text(&label, source).to_owned());
        }
    }
    labels
}

/// An `if`/`else if` chain that compares one expression with constants.
struct Chain<'t> {
    labels: Vec<String>,
    /// The last link, which ends the chain's lines.
    last: Node<'t>,
    links: Vec<Node<'t>>,
    /// The closing `else`, if there is one.
    closing: Option<Node<'t>>,
}

/// The constants an `if`/`else if` chain starting at `head` compares one expression with. Every
/// link is added to `in_chain` so it is not a chain of its own.
fn chain_labels<'t>(
    kinds: &[Option<Kind>],
    head: &Node<'t>,
    source: &[u8],
    in_chain: &mut BTreeSet<usize>,
) -> Option<Chain<'t>> {
    let mut links = Vec::new();
    let mut closing = None;
    collect_chain(kinds, head, &mut links, &mut closing);
    in_chain.extend(links.iter().map(Node::id));
    let compared: Vec<(&str, &str)> = links
        .iter()
        .filter_map(|link| comparison(kinds, &link.child_by_field_name("condition")?, source, 0))
        .collect();
    let (subject, _) = *compared.first()?;
    let labels: Vec<String> = compared
        .iter()
        .filter(|(left, _)| *left == subject)
        .map(|(_, right)| (*right).to_owned())
        .collect();
    (labels.len() >= CHAIN_COMPARISONS_MIN).then(|| Chain {
        labels,
        last: *links.last().unwrap_or(head),
        links,
        closing,
    })
}

/// The links of an `if` chain, in order: each `elif`, and each `else` that holds another `if`.
/// An `else` that holds none closes the chain.
fn collect_chain<'t>(
    kinds: &[Option<Kind>],
    link: &Node<'t>,
    out: &mut Vec<Node<'t>>,
    closing: &mut Option<Node<'t>>,
) {
    out.push(*link);
    let mut cursor = link.walk();
    for alternative in link.children_by_field_name("alternative", &mut cursor) {
        if kind_of(kinds, &alternative) == Some(Kind::Conditional) {
            collect_chain(kinds, &alternative, out, closing);
            continue;
        }
        let mut inner = alternative.walk();
        let nested: Vec<Node> = alternative
            .named_children(&mut inner)
            .filter(|child| kind_of(kinds, child) == Some(Kind::Conditional))
            .collect();
        if nested.is_empty() {
            *closing = Some(alternative);
        }
        for child in nested {
            collect_chain(kinds, &child, out, closing);
        }
    }
}

/// `subject == constant` somewhere in a condition: the two sides' text. A constant is a string,
/// a number or a qualified or capitalised name (an enum variant), never a plain variable.
fn comparison<'a>(
    kinds: &[Option<Kind>],
    node: &Node,
    source: &'a [u8],
    depth: usize,
) -> Option<(&'a str, &'a str)> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    if children
        .iter()
        .any(|child| !child.is_named() && matches!(child.kind(), "==" | "==="))
    {
        let mut named = node.walk();
        let sides: Vec<Node> = node.named_children(&mut named).collect();
        let (left, right) = (sides.first()?, sides.last()?);
        let constant = text(right, source);
        let is_constant = kind_of(kinds, right) == Some(Kind::String)
            || constant.contains("::")
            || constant.contains('.')
            || constant.starts_with(|c: char| c.is_ascii_digit() || c.is_uppercase());
        return is_constant.then(|| (text(left, source), constant));
    }
    if depth == COMPARISON_DEPTH_MAX {
        return None;
    }
    children
        .iter()
        .filter(|child| child.is_named())
        .find_map(|child| comparison(kinds, child, source, depth + 1))
}

/// The alternatives of an or-pattern, split at the bars outside any payload.
pub(crate) fn alternatives(case: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0_u32;
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    for (at, c) in case.char_indices() {
        // Brackets and bars inside a quoted literal are text.
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == open {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' | '`' => quote = Some(c),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => {
                parts.push(&case[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&case[start..]);
    parts
}

/// One alternative of a case pattern with its payload cut. Payloads are cut from patterns only;
/// in an expression `[]int` is the label.
fn pattern_part(part: &str, names_bind: bool) -> &str {
    let part = part
        .trim()
        .trim_start_matches('&')
        .trim_start_matches("ref ");
    if part.starts_with(['"', '\'', '`']) || !names_bind {
        part
    } else {
        part.split(['(', '{', '[']).next().unwrap_or(part).trim()
    }
}

/// Whether a pattern part is a name that matches anything rather than a constant.
fn is_binding(part: &str, names_bind: bool) -> bool {
    let literal = matches!(part, "true" | "false" | "null" | "undefined" | "nil");
    names_bind
        && part.starts_with(|c: char| c.is_lowercase() || c == '_')
        && !part.contains(['.', ':'])
        && !literal
}

/// Whether a case text matches anything: one of its alternatives is a bare binding, and no guard
/// narrows it.
pub(crate) fn matches_anything(case: &str, names_bind: bool) -> bool {
    !case.contains(" if ")
        && alternatives(case)
            .into_iter()
            .any(|part| is_binding(pattern_part(part, names_bind), names_bind))
}

/// Case texts as comparable labels: or-patterns split, payloads and guards cut, catch-alls and
/// bindings dropped, duplicates removed, sorted.
fn normalise(cases: &[String], names_bind: bool) -> Vec<String> {
    let mut labels = BTreeSet::new();
    for case in cases {
        // A quoted literal is one label whatever it contains.
        if case.trim().starts_with(['"', '\'', '`']) {
            labels.insert(case.trim().to_owned());
            continue;
        }
        let case = case.split(" if ").next().unwrap_or(case);
        for part in alternatives(case) {
            let part = pattern_part(part, names_bind);
            if !part.is_empty() && !is_binding(part, names_bind) {
                labels.insert(part.to_owned());
            }
        }
    }
    labels.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::normalise;

    #[test]
    fn labels_drop_payloads_bindings_and_guards() {
        let cases = [
            "Kind::A(x) | Kind::B { .. }",
            "n if n > 3",
            "_",
            "\"go\"",
            "Kind::A",
        ]
        .map(str::to_owned);
        assert_eq!(normalise(&cases, true), ["\"go\"", "Kind::A", "Kind::B"]);
    }

    #[test]
    fn expression_labels_keep_their_brackets_and_lowercase_names() {
        let cases = ["[]int, map[string]int", "modeOne", "Foo{}"].map(str::to_owned);
        assert_eq!(
            normalise(&cases, false),
            ["Foo{}", "[]int, map[string]int", "modeOne"]
        );
    }

    #[test]
    fn quoted_labels_and_booleans_survive() {
        let cases = ["\"sign|out\"", "\"sign if yes\"", "true", "false"].map(str::to_owned);
        assert_eq!(
            normalise(&cases, true),
            ["\"sign if yes\"", "\"sign|out\"", "false", "true"]
        );
    }

    #[test]
    fn generic_arguments_do_not_hide_the_callee() {
        assert_eq!(super::strip_generics("Vec::<u8>::from"), "Vec::::from");
    }
}

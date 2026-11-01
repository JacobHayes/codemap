//! What the arms of a `match` or `switch` look like, read through the generic kinds of a
//! language's table: whether one arm catches every other case, whether the arms only map cases
//! to constants, and whether one or two arms carry all the logic. A new case is easy to miss in
//! the first kind of site and the other two are a table or an accessor in disguise.

use std::collections::BTreeSet;

use codemap_lang::{Kind, Language, Node};

use crate::shape::{alternatives, case_nodes, kind_of, label_nodes, matches_anything, text};

/// Arms with a body of their own, at most this many, make a site degenerate when all the others
/// share one trivial body.
const REAL_ARMS_MAX: usize = 2;
/// Degenerate sites need at least this many cases behind the shared trivial body.
const TRIVIAL_CASES_MIN: usize = 2;
/// Node kinds containing these compute something, so a body with one is not a constant.
const LOGIC_WORDS: &[&str] = &[
    "binary",
    "conditional",
    "unary",
    "ternary",
    "operator",
    "assignment",
    "send_statement",
    "go_statement",
    "defer_statement",
    "await",
    "inc_statement",
    "dec_statement",
    "comprehension",
    "generator",
    "update",
    "interpolation",
    "substitution",
    "closure",
    "lambda",
];
/// Node kinds containing these end the arm by failing.
const ABORT_WORDS: &[&str] = &["throw", "raise"];
/// Statements that leave a case without being part of its body.
const EXIT_KINDS: &[&str] = &["break_statement"];
/// A statement that hands the case to the next one.
const FALLTHROUGH_KINDS: &[&str] = &["fallthrough_statement"];
/// Constructors and conversions that build a constant from constants.
const CONSTRUCTORS: &[&str] = &["new", "default"];
const CONVERSIONS: &[&str] = &["to_owned", "to_string", "into", "clone"];

/// What the arms of one site add up to.
#[expect(
    clippy::struct_excessive_bools,
    reason = "four independent features of one site"
)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ArmShape {
    pub wildcard: bool,
    pub table: bool,
    pub degenerate: bool,
    /// Two arms for different cases have the same body.
    pub duplicated: bool,
}

enum Body {
    /// Only fails.
    Abort,
    /// A constant, `None`, a path: its text with whitespace collapsed, to compare bodies.
    Trivial(String),
    /// Logic of its own: the words of the body, to find arms that repeat each other, or nothing
    /// when a guard makes the arm its own.
    Real(Option<String>),
}

struct Arm {
    /// Cases the arm names; a catch-all counts as one.
    cases: usize,
    catch_all: bool,
    body: Body,
}

struct Reader<'a> {
    language: &'a Language,
    kinds: &'a [Option<Kind>],
    source: &'a [u8],
}

pub(crate) fn classify(
    language: &Language,
    kinds: &[Option<Kind>],
    switch: &Node,
    source: &[u8],
) -> ArmShape {
    let reader = Reader {
        language,
        kinds,
        source,
    };
    summarize(&reader.arms(switch))
}

/// The arms of an `if` chain: each link, and its closing `else` as the catch-all.
pub(crate) fn classify_chain(
    language: &Language,
    kinds: &[Option<Kind>],
    links: &[Node],
    closing: Option<Node>,
    source: &[u8],
) -> ArmShape {
    let reader = Reader {
        language,
        kinds,
        source,
    };
    let none = BTreeSet::new();
    let mut arms: Vec<Arm> = links
        .iter()
        .filter_map(|link| link.child_by_field_name("consequence"))
        .map(|body| Arm {
            cases: 1,
            catch_all: false,
            body: reader.classify_body(&[body], &none),
        })
        .collect();
    if let Some(closing) = closing {
        let mut cursor = closing.walk();
        let body: Vec<Node> = closing
            .named_children(&mut cursor)
            .filter(|child| kind_of(kinds, child) != Some(Kind::Comment))
            .collect();
        arms.push(Arm {
            cases: 1,
            catch_all: true,
            body: reader.classify_body(&body, &none),
        });
    }
    summarize(&arms)
}

fn summarize(arms: &[Arm]) -> ArmShape {
    let wildcard = arms
        .iter()
        .any(|arm| arm.catch_all || matches!(arm.body, Body::Abort));
    let others: Vec<&Arm> = arms
        .iter()
        .filter(|arm| !matches!(arm.body, Body::Abort))
        .collect();
    let real = others
        .iter()
        .filter(|arm| matches!(arm.body, Body::Real(_)))
        .count();
    let trivial: Vec<(&String, usize)> = others
        .iter()
        .filter_map(|arm| match &arm.body {
            Body::Trivial(text) => Some((text, arm.cases)),
            _ => None,
        })
        .collect();
    let distinct: BTreeSet<&String> = trivial.iter().map(|(text, _)| *text).collect();
    let table = real == 0 && others.len() >= 2;
    let degenerate = (1..=REAL_ARMS_MAX).contains(&real)
        && distinct.len() == 1
        && trivial.iter().map(|(_, cases)| cases).sum::<usize>() >= TRIVIAL_CASES_MIN;
    let mut seen = BTreeSet::new();
    let duplicated = others.iter().any(|arm| match &arm.body {
        Body::Real(Some(text)) => !seen.insert(text),
        _ => false,
    });
    ArmShape {
        wildcard,
        table,
        degenerate,
        duplicated,
    }
}

impl Reader<'_> {
    /// The arms of a switch. A case with no statements falls into the next one, so the two are
    /// one arm.
    fn arms(&self, switch: &Node) -> Vec<Arm> {
        let mut arms = Vec::new();
        let mut pending_cases = 0;
        let mut pending_catch_all = false;
        for case in case_nodes(self.kinds, switch) {
            let labels = label_nodes(self.language, &case);
            let names: Vec<&str> = labels
                .iter()
                .map(|label| text(label, self.source))
                .collect();
            let guarded = case.child_by_field_name("guard").is_some()
                || labels
                    .iter()
                    .any(|label| label.child_by_field_name("condition").is_some());
            // A case with no label is `default`.
            let catch_all = !guarded
                && (labels.is_empty()
                    || names
                        .iter()
                        .any(|name| matches_anything(name, self.language.case_names_bind)));
            pending_cases += labels
                .iter()
                .map(|label| case_count(label, self.source))
                .sum::<usize>()
                .max(1);
            pending_catch_all |= catch_all;
            let body = self.body_nodes(&case, &labels);
            let falls = match &body[..] {
                [only] => {
                    // Go wraps a case's statements in a list.
                    let inner = (only.named_child_count() == 1)
                        .then(|| only.named_child(0))
                        .flatten();
                    FALLTHROUGH_KINDS.contains(&only.kind())
                        || inner.is_some_and(|node| FALLTHROUGH_KINDS.contains(&node.kind()))
                }
                _ => false,
            };
            if falls || (body.is_empty() && self.language.case_fallthrough) {
                continue;
            }
            let bound = self.bound_names(&labels);
            arms.push(Arm {
                cases: std::mem::take(&mut pending_cases),
                catch_all: std::mem::take(&mut pending_catch_all),
                // A guard is a check at run time, which a table has none of.
                body: if guarded {
                    Body::Real(None)
                } else {
                    self.classify_body(&body, &bound)
                },
            });
        }
        if pending_cases > 0 {
            arms.push(Arm {
                cases: pending_cases,
                catch_all: pending_catch_all,
                body: Body::Trivial(String::new()),
            });
        }
        arms
    }

    /// What follows the labels of a case: its statements or expression, without comments, the
    /// guard, or a trailing `break` when something else is there.
    fn body_nodes<'t>(&self, case: &Node<'t>, labels: &[Node<'t>]) -> Vec<Node<'t>> {
        let guard = case.child_by_field_name("guard").map(|node| node.id());
        let mut cursor = case.walk();
        let all: Vec<Node> = case
            .named_children(&mut cursor)
            .filter(|child| {
                kind_of(self.kinds, child) != Some(Kind::Comment)
                    && Some(child.id()) != guard
                    && !labels.iter().any(|label| label.id() == child.id())
                    && !child.kind().ends_with("attribute_item")
            })
            .collect();
        let without_exit: Vec<Node> = all
            .iter()
            .copied()
            .filter(|node| !EXIT_KINDS.contains(&node.kind()))
            .collect();
        if without_exit.is_empty() {
            all
        } else {
            without_exit
        }
    }

    /// Lower-case names a pattern binds, which a body using them is not a constant.
    fn bound_names(&self, labels: &[Node]) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        if self.language.case_names_bind {
            for label in labels {
                collect_names(label, self.source, &mut names);
            }
        }
        names
    }

    fn classify_body(&self, body: &[Node], bound: &BTreeSet<String>) -> Body {
        if let [only] = body
            && self.aborts_in(only)
        {
            return Body::Abort;
        }
        if body.iter().all(|node| self.trivial(node, bound)) {
            Body::Trivial(self.key(body))
        } else {
            Body::Real(Some(self.key(body)))
        }
    }

    /// The words of a body without comments, to compare bodies: literals keep their contents,
    /// and only the block braces, trailing `;` and leading `return` around the whole body
    /// are dropped, so `{ x; }`, `return x` and `x` are one body.
    fn key(&self, body: &[Node]) -> String {
        let mut tokens: Vec<&str> = Vec::new();
        for node in body {
            self.tokens(node, &mut tokens);
        }
        loop {
            match tokens.as_slice() {
                [.., ";" | ","] => {
                    tokens.pop();
                }
                ["{", .., "}"] => {
                    tokens.remove(0);
                    tokens.pop();
                }
                ["return", ..] => {
                    tokens.remove(0);
                }
                _ => break,
            }
        }
        tokens.join(" ")
    }

    fn tokens<'s>(&'s self, node: &Node, out: &mut Vec<&'s str>) {
        if kind_of(self.kinds, node) == Some(Kind::Comment) {
            return;
        }
        if node.child_count() == 0 || kind_of(self.kinds, node) == Some(Kind::String) {
            let token = text(node, self.source);
            if !token.trim().is_empty() {
                out.push(token);
            }
            return;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.tokens(&child, out);
        }
    }

    /// Whether a body is one statement that only fails, however many block or statement
    /// wrappers surround it.
    fn aborts_in(&self, node: &Node) -> bool {
        if self.aborts(node) {
            return true;
        }
        if kind_of(self.kinds, node) == Some(Kind::Call) {
            return false;
        }
        let mut cursor = node.walk();
        let mut statements = node
            .named_children(&mut cursor)
            .filter(|child| kind_of(self.kinds, child) != Some(Kind::Comment));
        match (statements.next(), statements.next()) {
            (Some(only), None) => self.aborts_in(&only),
            _ => false,
        }
    }

    fn aborts(&self, node: &Node) -> bool {
        if ABORT_WORDS.iter().any(|word| node.kind().contains(word)) {
            return true;
        }
        kind_of(self.kinds, node) == Some(Kind::Call)
            && self
                .callee(node)
                .is_some_and(|name| self.language.abort_callees.contains(&last_segment(name)))
    }

    fn callee<'s>(&'s self, call: &Node) -> Option<&'s str> {
        self.language
            .callee_fields
            .iter()
            .find_map(|field| call.child_by_field_name(field))
            .map(|callee| text(&callee, self.source))
    }

    /// Whether a node is a value written out: a literal, a path, or a constructor applied to
    /// such values.
    fn trivial(&self, node: &Node, bound: &BTreeSet<String>) -> bool {
        // A sign in front of a constant is still a constant; any other prefix operator acts.
        let negation = node.kind().contains("unary") && is_negation(node);
        if !negation && LOGIC_WORDS.iter().any(|word| node.kind().contains(word)) {
            return false;
        }
        match kind_of(self.kinds, node) {
            Some(Kind::Comment) => return true,
            Some(Kind::Call) => return self.trivial_call(node, bound),
            Some(
                Kind::Branch
                | Kind::Conditional
                | Kind::Switch
                | Kind::Loop
                | Kind::Handler
                | Kind::Function
                | Kind::Abstraction
                | Kind::Assertion
                | Kind::Type,
            ) => return false,
            _ => {}
        }
        let mut cursor = node.walk();
        let mut children = node.named_children(&mut cursor).peekable();
        if children.peek().is_none() {
            return !bound.contains(text(node, self.source));
        }
        children.all(|child| self.trivial(&child, bound))
    }

    fn trivial_call(&self, call: &Node, bound: &BTreeSet<String>) -> bool {
        let Some(callee) = self.callee(call) else {
            return false;
        };
        let name = last_segment(callee);
        let qualified = callee.contains('.');
        let capitalised =
            self.language.capitalised_constructors && name.starts_with(char::is_uppercase);
        let builds = !qualified && (capitalised || CONSTRUCTORS.contains(&name));
        let converts = qualified && CONVERSIONS.contains(&name);
        if !(builds || converts) {
            return false;
        }
        let mut cursor = call.walk();
        call.named_children(&mut cursor)
            .all(|child| self.trivial(&child, bound))
    }
}

/// The cases one label names: the parts of a list, or the alternatives of an or-pattern that
/// are not inside a payload.
fn case_count(label: &Node, source: &[u8]) -> usize {
    if label.kind().ends_with("_list") {
        return label.named_child_count();
    }
    alternatives(text(label, source)).len()
}

/// `-1`, `-libc::EBADF`: a sign in front of a constant is still a constant.
fn is_negation(unary: &Node) -> bool {
    unary
        .child(0)
        .is_some_and(|operator| operator.kind() == "-")
}

fn last_segment(callee: &str) -> &str {
    callee
        .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
        .find(|part| !part.is_empty())
        .unwrap_or("")
}

fn collect_names(node: &Node, source: &[u8], names: &mut BTreeSet<String>) {
    // A name inside a path (`schema::Kind`, `Kind.A`) is not bound by the pattern.
    let in_path = node.parent().is_some_and(|parent| {
        ["scoped", "dotted", "attribute", "path"]
            .iter()
            // A lone name in a dotted name is a capture (`case Circle(radius)`), not a path.
            .any(|word| parent.kind().contains(word) && parent.named_child_count() > 1)
    });
    let binds = node.kind().ends_with("identifier")
        && !matches!(node.kind(), "type_identifier" | "field_identifier");
    if node.child_count() == 0 && binds && !in_path {
        let name = text(node, source);
        if name.starts_with(|c: char| c.is_lowercase() || c == '_') {
            names.insert(name.to_owned());
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_names(&child, source, names);
    }
}

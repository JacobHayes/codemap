//! The syntax extractor: one generic walk driven by a [`Language`]'s table and tags query.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use codemap_lang::{Concern, Kind, LANGUAGES, Language, Mark, Node, Role};
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::facts::{
    LineFact, RefFact, SwitchFact, SymbolFact, SymbolKind, split_lines, token_count,
};
use crate::fingerprint::{Fingerprint, LINES_MIN, fingerprint};
use crate::shape;

/// What one parse yields.
pub struct Parsed {
    pub lines: Vec<LineFact>,
    pub symbols: Vec<SymbolFact>,
    pub refs: Vec<RefFact>,
    pub switches: Vec<SwitchFact>,
}

/// Each shipped language's tags query, compiled once.
static QUERIES: LazyLock<BTreeMap<&'static str, Query>> = LazyLock::new(|| {
    LANGUAGES
        .iter()
        .map(|language| {
            let query = Query::new(
                &(language.grammar)(),
                &format!(
                    "{}\n{}",
                    language.tags_query,
                    language.extra_tags_queries.join("\n")
                ),
            )
            .expect("a shipped tags query compiles against its grammar");
            (language.name, query)
        })
        .collect()
});

/// Extracts lines, symbols and references. `None` when the grammar cannot parse the text at all,
/// so the caller falls back to plain text.
pub fn extract(language: &Language, text: &str, file_role: Role) -> Option<Parsed> {
    let mut parser = Parser::new();
    parser.set_language(&(language.grammar)()).ok()?;
    let tree = parser.parse(text, None)?;
    let source = text.as_bytes();
    let lines = split_lines(text);

    let kinds = kind_table(language);
    let mut grid = Grid::new(&lines, file_role);
    grid.paint(language, &kinds, &tree, source);
    let (symbols, refs, line_symbols) =
        symbols_and_refs(language, &kinds, &tree, source, lines.len());
    let refs = refs
        .into_iter()
        .map(|(name, kind, row)| RefFact {
            name,
            kind,
            line: to_line(row),
            symbol: line_symbols[row],
        })
        .collect();
    let depths = grid.depths();
    let facts = lines
        .iter()
        .enumerate()
        .map(|(row, line)| grid.line(row, line, line_symbols[row], depths[row]))
        .collect();
    let mut switches = shape::switches(language, &kinds, &tree.root_node(), source);
    for switch in &mut switches {
        switch.symbol = line_symbols[switch.start_line as usize - 1];
    }
    Some(Parsed {
        lines: facts,
        symbols,
        refs,
        switches,
    })
}

fn to_line(row: usize) -> u32 {
    u32::try_from(row + 1).unwrap_or(u32::MAX)
}

/// First and last row a node occupies; a node that ends at column 0 does not occupy that row.
fn rows(node: &Node) -> (usize, usize) {
    let start = node.start_position().row;
    let end = node.end_position();
    let last = if end.column == 0 && end.row > start {
        end.row - 1
    } else {
        end.row
    };
    (start, last)
}

/// Per-line attribution, painted node by node from the outermost in: the innermost wins.
struct Grid<'a> {
    lines: &'a [&'a str],
    role: Vec<Role>,
    concern: Vec<Concern>,
    node_count: Vec<u32>,
    /// Difference array of nesting depth.
    depth_delta: Vec<i64>,
}

impl<'a> Grid<'a> {
    fn new(lines: &'a [&'a str], file_role: Role) -> Self {
        Self {
            lines,
            role: vec![file_role; lines.len()],
            concern: vec![Concern::Core; lines.len()],
            node_count: vec![0; lines.len()],
            depth_delta: vec![0; lines.len() + 1],
        }
    }

    /// Nesting depth per row: the running sum of the difference array.
    fn depths(&self) -> Vec<u32> {
        let mut running = 0_i64;
        self.depth_delta[..self.lines.len()]
            .iter()
            .map(|delta| {
                running += delta;
                u32::try_from(running).unwrap_or(0)
            })
            .collect()
    }

    fn line(&self, row: usize, text: &str, symbol: Option<u32>, depth: u32) -> LineFact {
        LineFact {
            token_count: token_count(text),
            node_count: self.node_count[row],
            depth,
            role: self.role[row],
            concern: self.concern[row],
            symbol,
        }
    }

    fn paint(&mut self, language: &Language, kinds: &[Option<Kind>], tree: &Tree, source: &[u8]) {
        let mut cursor = tree.walk();
        loop {
            self.visit(language, kinds, &cursor.node(), source);
            if cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return;
                }
            }
        }
    }

    fn visit(&mut self, language: &Language, kinds: &[Option<Kind>], node: &Node, source: &[u8]) {
        let (first, last) = rows(node);
        if node.is_named() {
            self.node_count[first] += 1;
        }
        let kind = if node.is_named() {
            kinds.get(usize::from(node.kind_id())).copied().flatten()
        } else {
            None
        };
        match kind {
            Some(
                Kind::Function
                | Kind::Type
                | Kind::Abstraction
                | Kind::Module
                | Kind::Branch
                | Kind::Conditional
                | Kind::Loop
                | Kind::Handler,
            ) if node.parent().is_some() => {
                self.nest(first, last);
            }
            _ => {}
        }
        let concern = match kind {
            Some(Kind::Handler) => Some(Concern::Handler),
            Some(Kind::Assertion) => Some(Concern::Assertion),
            Some(Kind::Import) => Some(Concern::Import),
            Some(Kind::Comment) => self.starts_line(node).then_some(Concern::Comment),
            Some(Kind::Call) => call_concern(language, node, source),
            _ => None,
        };
        if let Some(concern) = concern {
            self.paint_concern(first, last, concern);
        }
        match (language.mark)(node, source) {
            Some(Mark::Concern(concern)) => self.paint_concern(first, last, concern),
            Some(Mark::Role(role)) => self.paint_role(first, last, role),
            Some(Mark::TestFunction) => self.paint_role(first, last, Role::Test),
            Some(Mark::RoleThrough(role)) => {
                let through = node.next_named_sibling().map_or(last, |next| rows(&next).1);
                self.paint_role(first, through.max(last), role);
            }
            Some(Mark::Abstraction) | None => {}
        }
    }

    /// Rows strictly inside a scope are one level deeper; a closing bracket line is not inside.
    fn nest(&mut self, first: usize, last: usize) {
        let closes = self.lines[last].trim_start().starts_with(['}', ')', ']']);
        let inside_last = if closes { last.saturating_sub(1) } else { last };
        if inside_last > first {
            self.depth_delta[first + 1] += 1;
            self.depth_delta[inside_last + 1] -= 1;
        }
    }

    /// A comment owns its line only when nothing precedes it there.
    fn starts_line(&self, node: &Node) -> bool {
        let start = node.start_position();
        let line = self.lines[start.row];
        line.len() - line.trim_start().len() == start.column
    }

    fn paint_concern(&mut self, first: usize, last: usize, concern: Concern) {
        self.concern[first..=last].fill(concern);
    }

    fn paint_role(&mut self, first: usize, last: usize, role: Role) {
        let last = last.min(self.role.len() - 1);
        self.role[first..=last].fill(role);
    }
}

/// Node-kind id to generic kind, for the table's named kinds.
fn kind_table(language: &Language) -> Vec<Option<Kind>> {
    let grammar = (language.grammar)();
    let mut table = vec![None; grammar.node_kind_count()];
    for &(name, kind) in language.kinds {
        let id = usize::from(grammar.id_for_node_kind(name, true));
        if id != 0 {
            table[id] = Some(kind);
        }
    }
    table
}

/// Logging and assertion calls are told apart by the callee's last name.
fn call_concern(language: &Language, node: &Node, source: &[u8]) -> Option<Concern> {
    let callee = language
        .callee_fields
        .iter()
        .find_map(|field| node.child_by_field_name(field))?;
    let text = callee.utf8_text(source).ok()?;
    let name = text
        .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
        .find(|part| !part.is_empty())?;
    if language
        .assertion_prefixes
        .iter()
        .any(|prefix| name.starts_with(prefix))
    {
        Some(Concern::Assertion)
    } else if language.logging_callees.contains(&name) {
        Some(Concern::Logging)
    } else {
        None
    }
}

/// A raw reference: name, kind, 0-based row.
type RawRef = (String, String, usize);

/// Runs the tags query: definitions become symbols (outermost first), references stay by name.
fn symbols_and_refs(
    language: &Language,
    kinds: &[Option<Kind>],
    tree: &Tree,
    source: &[u8],
    line_count: usize,
) -> (Vec<SymbolFact>, Vec<RawRef>, Vec<Option<u32>>) {
    let query = &QUERIES[language.name];
    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), source);
    // Keyed by byte range so a definition several patterns match (methods) counts once.
    let mut definitions: BTreeMap<(usize, usize), (SymbolKind, String, Node)> = BTreeMap::new();
    let mut refs = Vec::new();
    while let Some(found) = matches.next() {
        let mut name = None;
        let mut tag = None;
        for capture in found.captures() {
            let capture_name = names[capture.index as usize];
            if capture_name == "name" {
                name = capture
                    .node
                    .utf8_text(source)
                    .ok()
                    .map(|text| (text, capture.node));
            } else {
                tag = Some((capture_name, capture.node));
            }
        }
        let (Some((name, name_node)), Some((tag, node))) = (name, tag) else {
            continue;
        };
        if let Some(kind) = tag.strip_prefix("reference.") {
            refs.push((
                name.to_owned(),
                kind.to_owned(),
                name_node.start_position().row,
            ));
        } else if let Some(kind) = definition_kind(tag, language, &node, source) {
            definitions
                .entry((node.start_byte(), node.end_byte()))
                .or_insert((kind, name.to_owned(), node));
        }
    }
    let mut ordered: Vec<_> = definitions.into_iter().collect();
    ordered.sort_by_key(|((start, end), _)| (*start, std::cmp::Reverse(*end)));
    let mut symbols: Vec<SymbolFact> = Vec::new();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut line_symbols = vec![None; line_count];
    for ((start, end), (kind, name, node)) in ordered {
        while open
            .last()
            .is_some_and(|&top| !(ranges[top].0 <= start && end <= ranges[top].1))
        {
            open.pop();
        }
        let (first, last) = rows(&node);
        let index = u32::try_from(symbols.len()).unwrap_or(u32::MAX);
        line_symbols[first..=last].fill(Some(index));
        let (forwards, abstraction) = shape_of(kind, language, kinds, &node, source);
        symbols.push(SymbolFact {
            kind,
            name,
            parent: open
                .last()
                .map(|&top| u32::try_from(top).unwrap_or(u32::MAX)),
            start_line: to_line(first),
            end_line: to_line(last),
            content_hash: blake3::hash(&source[start..end]).to_hex().to_string(),
            fingerprint: fingerprint_of(kind, language, &node, source, kinds),
            forwards,
            abstraction,
        });
        open.push(symbols.len() - 1);
        ranges.push((start, end));
    }
    (symbols, refs, line_symbols)
}

/// Functions and tests of at least [`LINES_MIN`] lines get fingerprints.
fn fingerprint_of(
    kind: SymbolKind,
    language: &Language,
    node: &Node,
    source: &[u8],
    kinds: &[Option<Kind>],
) -> Option<Fingerprint> {
    let (first, last) = rows(node);
    let callable = matches!(kind, SymbolKind::Function | SymbolKind::Test);
    (callable && to_line(last) - to_line(first) + 1 >= LINES_MIN)
        .then(|| fingerprint(language, node, source, kinds))
        .flatten()
}

fn definition_kind(
    tag: &str,
    language: &Language,
    node: &Node,
    source: &[u8],
) -> Option<SymbolKind> {
    match tag.strip_prefix("definition.")? {
        "class" | "interface" => Some(SymbolKind::Type),
        "module" => Some(SymbolKind::Module),
        "function" | "method" => {
            let is_test = (language.mark)(node, source) == Some(Mark::TestFunction);
            Some(if is_test {
                SymbolKind::Test
            } else {
                SymbolKind::Function
            })
        }
        _ => None,
    }
}

/// A trait, interface or abstract class: the table says so, or the language's `mark` does.
fn is_abstraction(language: &Language, kinds: &[Option<Kind>], node: &Node, source: &[u8]) -> bool {
    kinds.get(usize::from(node.kind_id())).copied().flatten() == Some(Kind::Abstraction)
        || (language.mark)(node, source) == Some(Mark::Abstraction)
}

/// What a definition's shape says: the callee a function only forwards to, and whether the
/// type exists to be implemented.
fn shape_of(
    kind: SymbolKind,
    language: &Language,
    kinds: &[Option<Kind>],
    node: &Node,
    source: &[u8],
) -> (Option<String>, bool) {
    let forwards = (kind == SymbolKind::Function)
        .then(|| shape::forwarded_callee(language, kinds, node, source))
        .flatten();
    (forwards, is_abstraction(language, kinds, node, source))
}

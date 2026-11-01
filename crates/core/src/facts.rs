//! The facts an extractor produces for one file: a pure function of its path and bytes.

use codemap_lang::{Concern, Role, language_for_path};

use crate::fingerprint::Fingerprint;
use crate::{role, syntax, text};

/// What a symbol is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Module,
    Type,
    Function,
    Test,
}

impl SymbolKind {
    /// The name stored in the index.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Type => "type",
            Self::Function => "function",
            Self::Test => "test",
        }
    }
}

/// The attribution of one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineFact {
    pub token_count: u32,
    pub node_count: u32,
    pub depth: u32,
    pub role: Role,
    pub concern: Concern,
    /// Index into [`FileFacts::symbols`] of the innermost symbol covering the line.
    pub symbol: Option<u32>,
}

/// A module, type, function or test with its 1-based inclusive line range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolFact {
    pub kind: SymbolKind,
    pub name: String,
    /// Index into [`FileFacts::symbols`].
    pub parent: Option<u32>,
    pub start_line: u32,
    pub end_line: u32,
    pub content_hash: String,
    /// Functions and tests big enough to compare for duplication.
    pub fingerprint: Option<Fingerprint>,
    /// For a function whose body only forwards its arguments to one call: that callee's name.
    pub forwards: Option<String>,
    /// A trait, interface or abstract class: a type that exists to be implemented.
    pub abstraction: bool,
}

/// A name as written, before resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefFact {
    pub name: String,
    pub kind: String,
    pub line: u32,
    /// Index into [`FileFacts::symbols`] of the symbol the reference sits in.
    pub symbol: Option<u32>,
}

/// The cases a `match`, `switch` or `if` chain tells apart, as comparable labels.
#[expect(
    clippy::struct_excessive_bools,
    reason = "four independent features of one site"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchFact {
    /// Index into [`FileFacts::symbols`] of the symbol the switch sits in.
    pub symbol: Option<u32>,
    pub start_line: u32,
    pub end_line: u32,
    /// Distinct, sorted, with payloads and catch-alls removed.
    pub labels: Vec<String>,
    /// A catch-all arm (`_`, a bare binding, `default`, an `else`) or an arm that only aborts:
    /// a case added later is not seen here.
    pub wildcard: bool,
    /// Every arm maps its cases to a constant, with no logic.
    pub table: bool,
    /// One or two arms have a body of their own; every other arm has the same trivial body.
    pub degenerate: bool,
    /// Two arms for different cases have the same body, with logic of its own.
    pub duplicated: bool,
    /// One position of a tuple match, which may tell fewer cases apart than a set needs and
    /// still be a site of one.
    pub partial: bool,
}

/// Everything extracted from one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFacts {
    pub language: Option<&'static str>,
    pub role: Role,
    pub size_bytes: u64,
    /// Binary files keep a size and no lines.
    pub binary: bool,
    pub lines: Vec<LineFact>,
    pub symbols: Vec<SymbolFact>,
    pub refs: Vec<RefFact>,
    pub switches: Vec<SwitchFact>,
}

/// How many leading bytes are searched for a NUL to call a file binary.
const BINARY_SNIFF_BYTES: usize = 8192;

/// Extracts a file's facts. `path` is relative to the root with `/` separators.
#[must_use]
pub fn extract(path: &str, bytes: &[u8]) -> FileFacts {
    let size_bytes = bytes.len() as u64;
    let sniffed = &bytes[..bytes.len().min(BINARY_SNIFF_BYTES)];
    let text = match std::str::from_utf8(bytes) {
        Ok(text) if !sniffed.contains(&0) => text,
        _ => {
            return FileFacts {
                language: None,
                role: role::role_for_path(path, None, ""),
                size_bytes,
                binary: true,
                lines: Vec::new(),
                symbols: Vec::new(),
                refs: Vec::new(),
                switches: Vec::new(),
            };
        }
    };
    let language = language_for_path(path);
    let file_role = role::role_for_path(path, language, text);
    let mut facts = FileFacts {
        language: language.map(|language| language.name),
        role: file_role,
        size_bytes,
        binary: false,
        lines: Vec::new(),
        symbols: Vec::new(),
        refs: Vec::new(),
        switches: Vec::new(),
    };
    let parsed = language
        .filter(|_| !text.is_empty())
        .and_then(|language| syntax::extract(language, text, file_role));
    match parsed {
        Some(parsed) => {
            facts.lines = parsed.lines;
            facts.symbols = parsed.symbols;
            facts.refs = parsed.refs;
            facts.switches = parsed.switches;
        }
        None => facts.lines = text::extract(text, file_role),
    }
    facts
}

/// Splits text into lines the way an editor counts them: a final newline ends the last line
/// rather than starting another, and `\r` is not part of a line.
pub(crate) fn split_lines(text: &str) -> Vec<&str> {
    let body = text.strip_suffix('\n').unwrap_or(text);
    if text.is_empty() {
        return Vec::new();
    }
    body.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// Tokens in one line under the encoding recorded in the index metadata.
pub(crate) fn token_count(line: &str) -> u32 {
    u32::try_from(bpe_openai::o200k_base().count(line)).unwrap_or(u32::MAX)
}

/// Name of the encoding `token_count` uses, recorded in the index metadata.
pub(crate) const TOKEN_ENCODING: &str = "o200k_base";

//! Grammars and their node-kind tables. A [`Language`] is data: the generic extractor in
//! `codemap-core` reads it and no language has code of its own beyond a small `mark` function.

mod javascript;
mod python;
mod rust;
mod typescript;

pub use tree_sitter::Node;

/// What a file is for. Decided by path conventions, then overridden line by line by in-file
/// evidence such as Rust test modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Prod,
    Test,
    Bench,
    Example,
    Script,
    Docs,
    Config,
    Generated,
    Vendored,
}

impl Role {
    /// Every role, in display order.
    pub const ALL: [Self; 9] = [
        Self::Prod,
        Self::Test,
        Self::Bench,
        Self::Example,
        Self::Script,
        Self::Docs,
        Self::Config,
        Self::Generated,
        Self::Vendored,
    ];

    /// The role stored under this name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.as_str() == name)
    }

    /// The name stored in the index.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prod => "prod",
            Self::Test => "test",
            Self::Bench => "bench",
            Self::Example => "example",
            Self::Script => "script",
            Self::Docs => "docs",
            Self::Config => "config",
            Self::Generated => "generated",
            Self::Vendored => "vendored",
        }
    }
}

/// What a line does. `Core` is the default; the rest come from the innermost classified node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Concern {
    Core,
    Handler,
    Assertion,
    Logging,
    Comment,
    Import,
}

impl Concern {
    /// Every concern, in display order.
    pub const ALL: [Self; 6] = [
        Self::Core,
        Self::Handler,
        Self::Assertion,
        Self::Logging,
        Self::Comment,
        Self::Import,
    ];

    /// The name stored in the index.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Handler => "handler",
            Self::Assertion => "assertion",
            Self::Logging => "logging",
            Self::Comment => "comment",
            Self::Import => "import",
        }
    }
}

/// The generic kinds a node kind can map to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Function,
    Type,
    Module,
    Branch,
    Loop,
    Handler,
    Assertion,
    Call,
    Comment,
    String,
    Import,
}

/// In-file evidence a language reports about one node, beyond the kind table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// The node's lines have this role.
    Role(Role),
    /// The node and the next named sibling have this role (an attribute and its item).
    RoleThrough(Role),
    /// The node is a function that is a test; its lines are test lines.
    TestFunction,
    /// The node's lines have this concern.
    Concern(Concern),
}

/// A language as data.
pub struct Language {
    pub name: &'static str,
    /// File extensions without the dot, lowercase.
    pub extensions: &'static [&'static str],
    pub grammar: fn() -> tree_sitter::Language,
    /// Node kind to generic kind.
    pub kinds: &'static [(&'static str, Kind)],
    /// The grammar crate's tags query: definitions become symbols, references become refs.
    pub tags_query: &'static str,
    /// Further definition patterns for scopes the tags query leaves out, such as Rust `impl`
    /// blocks, so a method's parent is its type.
    pub extra_tags_query: &'static str,
    /// Fields of a call node that hold the callee.
    pub callee_fields: &'static [&'static str],
    /// Callee names (last path segment) that are logging.
    pub logging_callees: &'static [&'static str],
    /// Callee-name prefixes that are assertions.
    pub assertion_prefixes: &'static [&'static str],
    /// File-stem prefixes and suffixes that mark a test file.
    pub test_stem_prefixes: &'static [&'static str],
    pub test_stem_suffixes: &'static [&'static str],
    /// The language's few exceptions to the table.
    pub mark: fn(&Node, &[u8]) -> Option<Mark>,
}

/// Every shipped language: one registry line each.
pub static LANGUAGES: &[&Language] = &[
    &rust::RUST,
    &python::PYTHON,
    &javascript::JAVASCRIPT,
    &typescript::TYPESCRIPT,
    &typescript::TSX,
];

/// The language for a path, by extension.
#[must_use]
pub fn language_for_path(path: &str) -> Option<&'static Language> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let (_, extension) = name.rsplit_once('.')?;
    let extension = extension.to_ascii_lowercase();
    LANGUAGES
        .iter()
        .copied()
        .find(|language| language.extensions.contains(&extension.as_str()))
}

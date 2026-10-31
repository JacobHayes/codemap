//! Similarity fingerprints of a function, taken from the syntax tree's leaf tokens.
//!
//! Whitespace and comments are not tokens, so formatting never matters. The *exact* fingerprint
//! hashes every token as written. The *near* signature is a `MinHash` over shingles (runs of
//! consecutive tokens) of an abstracted stream in which identifiers and literals are placeholders,
//! so a renamed copy, or one with other constants, still matches. Operators, keywords and
//! punctuation stay, so different logic does not. All of it works on node kinds and text, so no
//! language contributes code.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use codemap_lang::{Kind, LANGUAGES, Language, Node};

/// Hashes in a signature. 64 hashes in 16 bands of 4 rows put the chance that a pair of true
/// Jaccard 0.8 shares a band above 99.9% and that a pair at 0.3 does at about 12%, which the
/// confirmation step then discards.
pub const MINHASH_LEN: usize = 64;

/// Leaf tokens in one shingle. Shorter runs match unrelated code that only shares punctuation;
/// longer ones make one edit break many shingles.
const SHINGLE_WIDTH: usize = 5;

/// A function with fewer lines is not fingerprinted: accessors, delegating one-liners and
/// constructors repeat everywhere and are not an idea anyone would extract.
pub const LINES_MIN: u32 = 6;

/// A function with fewer leaf tokens is not fingerprinted, however it is laid out: a tall
/// function of short statements would otherwise match on structure alone.
const TOKENS_MIN: usize = 30;

/// What one leaf token stands for in the near signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Identifier,
    Literal,
    /// Keywords, punctuation and everything else: kept as written.
    Other,
}

/// Kind-name parts that make a leaf a literal's value.
const LITERAL_PARTS: [&str; 12] = [
    "literal", "string", "number", "integer", "float", "char", "escape", "content", "fragment",
    "regex", "boolean", "null",
];

fn class_of(kind: &str) -> Class {
    if kind.contains("identifier") {
        Class::Identifier
    } else if LITERAL_PARTS.iter().any(|part| kind.contains(part)) {
        Class::Literal
    } else {
        Class::Other
    }
}

/// Each shipped language's leaf class by node kind id, built once.
static CLASSES: LazyLock<BTreeMap<&'static str, Vec<Class>>> = LazyLock::new(|| {
    LANGUAGES
        .iter()
        .map(|language| {
            let grammar = (language.grammar)();
            let table = (0..grammar.node_kind_count())
                .map(|id| {
                    let id = u16::try_from(id).unwrap_or(u16::MAX);
                    class_of(grammar.node_kind_for_id(id).unwrap_or(""))
                })
                .collect();
            (language.name, table)
        })
        .collect()
});

/// The fingerprints of one function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// Over every token as written.
    pub exact: u64,
    /// One minimum per hash function over the shingles of the abstracted stream.
    pub minhash: [u32; MINHASH_LEN],
}

/// A fixed, well-mixed 64-bit permutation (the `SplitMix64` finalizer).
const fn mix(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const SEEDS: [u64; MINHASH_LEN] = {
    let mut seeds = [0; MINHASH_LEN];
    let mut i = 0;
    while i < MINHASH_LEN {
        seeds[i] = mix(i as u64 + 1);
        i += 1;
    }
    seeds
};

/// FNV-1a over bytes: cheap, and stable across runs and platforms.
fn hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xCBF2_9CE4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01B3)
    })
}

/// The leaf tokens of a function as (written, abstracted) hashes.
fn tokens(
    language: &Language,
    root: &Node,
    source: &[u8],
    kinds: &[Option<Kind>],
) -> Vec<(u64, u64)> {
    let classes = &CLASSES[language.name];
    let mut out = Vec::new();
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        let id = usize::from(node.kind_id());
        let comment = node.is_named() && kinds.get(id).copied().flatten() == Some(Kind::Comment);
        let leaf = node.child_count() == 0;
        if leaf && !comment {
            let text = hash_bytes(&source[node.start_byte()..node.end_byte()]);
            let written = mix(text ^ ((id as u64) << 48));
            let abstracted = match classes.get(id).copied().unwrap_or(Class::Other) {
                Class::Identifier => 1,
                Class::Literal => 2,
                Class::Other => mix(text) | 4,
            };
            out.push((written, abstracted));
        }
        if !leaf && !comment && cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.node() == *root {
                return out;
            }
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return out;
            }
        }
    }
}

/// Fingerprints the function `root`; `None` when it has too few tokens to say anything.
pub(crate) fn fingerprint(
    language: &Language,
    root: &Node,
    source: &[u8],
    kinds: &[Option<Kind>],
) -> Option<Fingerprint> {
    let tokens = tokens(language, root, source, kinds);
    if tokens.len() < TOKENS_MIN {
        return None;
    }
    let exact = tokens.iter().fold(0_u64, |hash, &(written, _)| {
        mix(hash.rotate_left(7) ^ written)
    });
    let mut minhash = [u32::MAX; MINHASH_LEN];
    for window in tokens.windows(SHINGLE_WIDTH) {
        let shingle = window.iter().fold(0_u64, |hash, &(_, abstracted)| {
            mix(hash.rotate_left(11) ^ abstracted)
        });
        for (slot, seed) in minhash.iter_mut().zip(SEEDS) {
            let value = u32::try_from(mix(shingle ^ seed) >> 32).unwrap_or(u32::MAX);
            *slot = (*slot).min(value);
        }
    }
    Some(Fingerprint { exact, minhash })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::extract;

    const ORIGINAL: &str = "fn total(items: &[Item], rate: f64) -> f64 {
    let mut sum = 0.0;
    for item in items {
        if item.active {
            sum += item.price * item.count as f64;
        }
    }
    sum * (1.0 + rate)
}
";

    /// The fingerprint of the file's only function.
    fn only(path: &str, text: &str) -> Option<Fingerprint> {
        let facts = extract(path, text.as_bytes());
        assert_eq!(facts.symbols.len(), 1);
        facts.symbols[0].fingerprint.clone()
    }

    fn similarity(a: &Fingerprint, b: &Fingerprint) -> f64 {
        let equal = a.minhash.iter().zip(&b.minhash).filter(|(x, y)| x == y);
        f64::from(u32::try_from(equal.count()).unwrap()) / 64.0
    }

    #[test]
    fn formatting_and_comments_do_not_change_the_exact_fingerprint() {
        let reformatted = "// Adds up the active items.
fn total(items: &[Item], rate: f64) -> f64 { let mut sum = 0.0;
    for item in items { if item.active {
            sum += item.price * item.count as f64; // price times count
    } }
    /* tax */
    sum
        * (1.0 + rate) }
";
        assert_eq!(
            only("a.rs", ORIGINAL).unwrap().exact,
            only("b.rs", reformatted).unwrap().exact
        );
    }

    #[test]
    fn a_renamed_copy_differs_exactly_but_matches_nearly() {
        let renamed = ORIGINAL
            .replace("total", "cost")
            .replace("items", "lines")
            .replace("rate", "tax")
            .replace("sum", "acc")
            .replace("1.0", "2.5");
        let (original, copy) = (
            only("a.rs", ORIGINAL).unwrap(),
            only("b.rs", &renamed).unwrap(),
        );
        assert_ne!(original.exact, copy.exact);
        assert!(similarity(&original, &copy) > 0.95);
    }

    #[test]
    fn different_logic_of_the_same_size_does_not_match() {
        let other = "fn drain(queue: &mut Vec<Job>, limit: usize) -> Option<Job> {
    while queue.len() > limit {
        match queue.pop() {
            Some(job) if job.urgent => return Some(job),
            Some(_) => continue,
            None => break,
        }
    }
    queue.first().cloned()
}
";
        let (original, other) = (
            only("a.rs", ORIGINAL).unwrap(),
            only("b.rs", other).unwrap(),
        );
        assert!(similarity(&original, &other) < 0.3);
    }

    #[test]
    fn the_same_idea_in_another_language_is_not_a_match() {
        let python = "def total(items, rate):
    total = 0.0
    for item in items:
        if item.active:
            total += item.price * item.count
    return total * (1.0 + rate)
";
        let (original, port) = (
            only("a.rs", ORIGINAL).unwrap(),
            only("b.py", python).unwrap(),
        );
        assert_ne!(original.exact, port.exact);
        assert!(similarity(&original, &port) < 0.3);
    }

    #[test]
    fn small_functions_have_no_fingerprint() {
        let few_lines =
            "fn area(w: f64, h: f64) -> f64 { let a = w * h; if a < 0.0 { 0.0 } else { a } }\n";
        let few_tokens = "fn quiet() {\n    a();\n    b();\n    c();\n    d();\n    e();\n}\n";
        assert!(only("a.rs", few_lines).is_none());
        assert!(only("a.rs", few_tokens).is_none());
    }
}

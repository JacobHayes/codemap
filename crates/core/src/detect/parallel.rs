//! Parallel structures: the same thing built more than once under a slightly different name or
//! switched on in more than one place.
//!
//! **Naming families.** Types and functions of production code whose names share a stem and
//! differ by a marker that says "another version of the same thing". Names are split into words
//! (`FooV2`, `foo_v2` and `Foo2` all become `foo` and a version); the markers are the words and
//! numbers in [`LEADING`] and [`TRAILING`]. A family is every symbol of one kind class (types,
//! or functions) with one stem, where at least one member carries a marker and the group is
//! more than one name. Choices:
//!
//! - `Impl` and `Base` are not markers: `Foo` and `FooImpl` is the ordinary pairing of an
//!   interface with its implementation, which the indirection detector already reports as a
//!   single implementation when it is one. `Builder`, `Error`, `Kind` and the like are likewise
//!   ordinary.
//! - A bare number (`Foo2`) marks a type only, only 1 to 3 (not the sizes of `Int8` or `Int64`), and only beside the
//!   unnumbered `Foo`; without those `Vec2`, `Vec3`, `Sha256`, `open64` and `openat2` would be
//!   families. `V2` counts alone and anywhere, because a `v` says it is a version.
//! - A name defined more than [`NAME_REPEAT_MAX`] times is a convention (`new`, `open`,
//!   `fmt`), not a thing with versions, and its group is left out. Function names that start
//!   with an underscore are left out too: they are ABI or macro plumbing.
//! - `new` leads the name of a constructor in many languages, so it is a marker only on types
//!   and as a suffix.
//!
//! **Repeated discriminators.** The extractor records the set of cases every `match`, `switch`
//! and `if` chain over constants tells apart. The same set of at least [`CASES_MIN`] cases in at
//! least [`SITES_MIN`] places in production code is one finding: adding a case means finding
//! all the sites.
//!
//! Both are findings in every file they touch, each listing all members as evidence.

use std::collections::{BTreeMap, BTreeSet};

use duckdb::params;

use super::{Context, Detector, Finding, SubjectKind, is_ranked};
use crate::reader::{Index, QueryError, SymbolRow};
use crate::store::LABEL_SEPARATOR;
use codemap_lang::Role;

/// Words that mark a variant when they start a name. `next` and `copy` are left out: `next_line`
/// and `kernel_copy` name steps and operations more often than versions.
const LEADING: &[&str] = &["legacy", "old", "new", "deprecated", "compat"];
/// Words that mark a variant when they end a name.
const TRAILING: &[&str] = &["legacy", "old", "new", "deprecated", "compat", "backup"];
/// A stem shorter than this says too little about what the variants share.
const STEM_LEN_MIN: usize = 3;
/// More definitions of one name than this make it a convention.
const NAME_REPEAT_MAX: usize = 3;
/// Members listed as evidence.
const EVIDENCE_MAX: usize = 12;
const CASES_MIN: usize = 3;
const SITES_MIN: usize = 3;

pub struct Parallel;

impl Detector for Parallel {
    fn name(&self) -> &'static str {
        "parallel"
    }

    fn describe(&self) -> &'static str {
        "Names that are versions of each other, and the same set of cases switched on in several places"
    }

    fn detect(&self, context: &Context) -> Result<Vec<Finding>, QueryError> {
        let symbols = context.index.symbols()?;
        let mut findings = Vec::new();
        for family in families(&symbols) {
            findings.extend(family_findings(&family));
        }
        for set in discriminators(context.index)? {
            findings.extend(discriminator_findings(&set));
        }
        Ok(findings)
    }
}

/// Lines covered by line ranges, counting a line in nested ranges once.
fn distinct_lines(ranges: impl Iterator<Item = (u32, u32)>) -> f64 {
    let mut ranges: Vec<_> = ranges.collect();
    ranges.sort_unstable();
    let mut total = 0;
    let mut reached = 0;
    for (start, end) in ranges {
        let start = start.max(reached + 1);
        if end >= start {
            total += end - start + 1;
            reached = end;
        }
    }
    f64::from(total)
}

fn number(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

fn lines(symbol: &SymbolRow) -> u32 {
    symbol.end_line - symbol.start_line + 1
}

// --- naming families ---

/// Splits a name into lower-case words and digit runs: `HTTPServerV2` is `http server v 2`.
fn words(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    for (at, &c) in chars.iter().enumerate() {
        let previous = at.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(at + 1).copied();
        let boundary = !c.is_alphanumeric()
            || previous.is_some_and(|p| {
                (p.is_lowercase() && c.is_uppercase())
                    || (p.is_alphabetic() != c.is_alphabetic() && p.is_alphanumeric())
                    || (p.is_uppercase()
                        && c.is_uppercase()
                        && next.is_some_and(char::is_lowercase))
            });
        if boundary && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        if c.is_alphanumeric() {
            current.extend(c.to_lowercase());
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// How a name relates to its stem.
#[derive(Debug, PartialEq, Eq)]
struct Split {
    stem: String,
    marked: bool,
    /// The marker is a bare number, which needs the unnumbered name beside it.
    bare_number: bool,
}

fn split(name: &str, is_type: bool) -> Split {
    let mut words = words(name);
    let mut marked = false;
    let mut bare_number = false;
    if let Some(last) = words.last()
        && last.chars().all(|c| c.is_ascii_digit())
        && words.len() > 1
    {
        let digits = words.pop().unwrap_or_default();
        marked = true;
        if words.last().is_some_and(|word| word == "v") {
            words.pop();
        } else if is_type && matches!(digits.as_str(), "1" | "2" | "3") {
            bare_number = true;
        } else {
            // Not a version: `open64`, `sha256`, `utf8`.
            words.push(digits);
            marked = false;
        }
    }
    if words.len() > 1 && words.last().is_some_and(|w| TRAILING.contains(&w.as_str())) {
        words.pop();
        marked = true;
        bare_number = false;
    }
    if words.len() > 1
        && words
            .first()
            .is_some_and(|w| LEADING.contains(&w.as_str()) && (is_type || w != "new"))
    {
        words.remove(0);
        marked = true;
        bare_number = false;
    }
    Split {
        stem: words.concat(),
        marked,
        bare_number,
    }
}

/// A group of symbols that are versions of one stem.
struct Family<'a> {
    stem: String,
    members: Vec<&'a SymbolRow>,
}

fn families(symbols: &[SymbolRow]) -> Vec<Family<'_>> {
    let mut groups: BTreeMap<(bool, String), Vec<(&SymbolRow, Split)>> = BTreeMap::new();
    for symbol in symbols {
        let is_type = symbol.kind == "type";
        if !(is_type || symbol.kind == "function")
            || !is_ranked(&symbol.role)
            || symbol.role == Role::Test.as_str()
        {
            continue;
        }
        if !is_type && symbol.name.starts_with('_') {
            continue;
        }
        let parts = split(&symbol.name, is_type);
        if parts.stem.len() >= STEM_LEN_MIN {
            groups
                .entry((is_type, parts.stem.clone()))
                .or_default()
                .push((symbol, parts));
        }
    }
    groups
        .into_iter()
        .filter_map(|((_, stem), members)| {
            let names: BTreeSet<&str> = members.iter().map(|(s, _)| s.name.as_str()).collect();
            // `Int` and `_int` differ only in style.
            let spellings: BTreeSet<String> = members
                .iter()
                .map(|(s, _)| words(&s.name).concat())
                .collect();
            let plain = members.iter().any(|(_, parts)| !parts.marked);
            let marked: Vec<_> = members.iter().filter(|(_, parts)| parts.marked).collect();
            let counts = marked.iter().any(|(_, parts)| !parts.bare_number) || plain;
            let repeated = names.iter().any(|name| {
                // A Rust `impl` block is a symbol of its type's name: count files, not blocks.
                let files: BTreeSet<&str> = members
                    .iter()
                    .filter(|(s, _)| s.name == *name)
                    .map(|(s, _)| s.path.as_str())
                    .collect();
                files.len() > NAME_REPEAT_MAX
            });
            (spellings.len() > 1 && !marked.is_empty() && counts && !repeated).then(|| Family {
                stem,
                members: members.into_iter().map(|(symbol, _)| symbol).collect(),
            })
        })
        .collect()
}

fn family_findings(family: &Family) -> Vec<Finding> {
    let names: BTreeSet<&str> = family.members.iter().map(|s| s.name.as_str()).collect();
    let shown: Vec<String> = names.iter().take(6).map(|n| format!("`{n}`")).collect();
    let reason = format!(
        "{} names are versions of one thing: {}{}.",
        names.len(),
        shown.join(", "),
        if names.len() > shown.len() {
            ", and more"
        } else {
            ""
        }
    );
    let files: BTreeSet<&str> = family.members.iter().map(|s| s.path.as_str()).collect();
    let first = family.members[0];
    files
        .iter()
        .map(|&file| {
            let here = distinct_lines(
                family
                    .members
                    .iter()
                    .filter(|s| s.path == file)
                    .map(|s| (s.start_line, s.end_line)),
            );
            let mut finding = Finding::new(
                "parallel",
                SubjectKind::Group,
                &format!("{}::{}#{}", first.path, first.name, first.start_line),
                reason.clone(),
                here,
                0.7,
            )
            .measure("members", number(family.members.len()))
            .measure("names", number(names.len()))
            .measure("files", number(files.len()))
            .measure(
                "totalLines",
                family.members.iter().map(|s| f64::from(lines(s))).sum(),
            );
            file.clone_into(&mut finding.area);
            finding
                .labels
                .insert("signal".to_owned(), "naming_family".to_owned());
            finding
                .labels
                .insert("stem".to_owned(), family.stem.clone());
            let mut parts: Vec<_> = family.members.iter().collect();
            parts.sort_by_key(|member| member.path != file);
            for member in parts.into_iter().take(EVIDENCE_MAX) {
                finding = finding.evidence(&member.path, &[(member.start_line, member.end_line)]);
            }
            finding
        })
        .collect()
}

// --- repeated discriminators ---

/// One place that switches on a case set.
struct Site {
    path: String,
    start_line: u32,
    end_line: u32,
}

struct CaseSet {
    labels: Vec<String>,
    sites: Vec<Site>,
}

fn discriminators(index: &Index) -> Result<Vec<CaseSet>, QueryError> {
    let mut statement = index.store().connection().prepare(
        "SELECT w.path, w.start_line, w.end_line, w.labels, l.role
         FROM switches w JOIN lines l ON l.path = w.path AND l.line = w.start_line
         ORDER BY w.labels, w.path, w.start_line",
    )?;
    let rows = statement.query_map(params![], |row| {
        Ok((
            Site {
                path: row.get(0)?,
                start_line: row.get(1)?,
                end_line: row.get(2)?,
            },
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut sets: BTreeMap<String, Vec<Site>> = BTreeMap::new();
    for row in rows {
        let (site, labels, role) = row?;
        if is_ranked(&role) && role != Role::Test.as_str() {
            sets.entry(labels).or_default().push(site);
        }
    }
    Ok(sets
        .into_iter()
        .filter(|(labels, sites)| {
            labels.split(LABEL_SEPARATOR).count() >= CASES_MIN && sites.len() >= SITES_MIN
        })
        .map(|(labels, sites)| CaseSet {
            labels: labels.split(LABEL_SEPARATOR).map(str::to_owned).collect(),
            sites,
        })
        .collect())
}

fn discriminator_findings(set: &CaseSet) -> Vec<Finding> {
    let shown: Vec<&str> = set.labels.iter().take(5).map(String::as_str).collect();
    let reason = format!(
        "The same {} cases ({}{}) are switched on in {} places.",
        set.labels.len(),
        shown.join(", "),
        if set.labels.len() > shown.len() {
            ", ..."
        } else {
            ""
        },
        set.sites.len()
    );
    let files: BTreeSet<&str> = set.sites.iter().map(|s| s.path.as_str()).collect();
    let first = &set.sites[0];
    files
        .iter()
        .map(|&file| {
            let here = distinct_lines(
                set.sites
                    .iter()
                    .filter(|s| s.path == file)
                    .map(|s| (s.start_line, s.end_line)),
            );
            let mut finding = Finding::new(
                "parallel",
                SubjectKind::Group,
                &format!("{}#{}", first.path, first.start_line),
                reason.clone(),
                here,
                0.6,
            )
            .measure("cases", number(set.labels.len()))
            .measure("sites", number(set.sites.len()))
            .measure("files", number(files.len()));
            file.clone_into(&mut finding.area);
            finding
                .labels
                .insert("signal".to_owned(), "repeated_discriminator".to_owned());
            // This file's sites first, so the cap never leaves the area without its own range.
            let mut parts: Vec<_> = set.sites.iter().collect();
            parts.sort_by_key(|site| site.path != file);
            for site in parts.into_iter().take(EVIDENCE_MAX) {
                finding = finding.evidence(&site.path, &[(site.start_line, site.end_line)]);
            }
            finding
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Split, split, words};

    #[test]
    fn names_split_into_words_and_versions() {
        assert_eq!(words("HTTPServerV2"), ["http", "server", "v", "2"]);
        assert_eq!(words("foo_v2"), ["foo", "v", "2"]);
        assert_eq!(words("Foo2"), ["foo", "2"]);
        assert_eq!(split("open64", false).stem, "open64");
    }

    #[test]
    fn markers_come_off_the_stem() {
        let stem = |name: &str, is_type| split(name, is_type).stem;
        assert_eq!(stem("FooV2", true), "foo");
        assert_eq!(stem("LegacyFoo", true), "foo");
        assert_eq!(stem("foo_new", false), "foo");
        assert_eq!(stem("new_foo", false), "newfoo");
        assert_eq!(stem("FooImpl", true), "fooimpl");
        assert_eq!(
            split("Foo2", true),
            Split {
                stem: "foo".to_owned(),
                marked: true,
                bare_number: true
            }
        );
    }
}

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
//! and `if` chain over constants tells apart, and what its arms look like. Sites of at least
//! [`CASES_MIN`] cases that are mostly the same set (a site that names some cases and catches
//! the rest still belongs to it) are one set when there are [`SITES_MIN`] of them in production
//! code. Matching a closed set in many places is ordinary, so the finding counts only the sites
//! that look like something else: a catch-all arm (a new case is silently missed), one or two
//! real arms among identical trivial ones (a missing accessor), arms that only map cases to
//! constants (a missing table), and arms for different cases with the same body (the logic
//! written once per variant). A match over a tuple is a site of each enum in a position that
//! varies, and a position with fewer cases than a set needs may join one by name. Sites whose
//! arms are distinct behaviour do not count, and a set needs [`FLAGGED_SITES_MIN`] flagged
//! sites: one on its own is a linter's finding. Sets of string or number literals are untyped
//! tags and count every site. A set whose cases are all qualified by a type the indexed code
//! does not define belongs to a dependency: its catch-alls are safe and stop counting, and its
//! tables and repeated bodies still do.
//!
//! Each is one finding, its own area: the evidence lists every member or site and the mass is
//! their distinct lines. A family's id is a digest of its stem and kind class and a
//! discriminator's of its case set, so both keep their id as members come and go.

use std::collections::{BTreeMap, BTreeSet};

use duckdb::params;

use super::{
    Context, Detector, Finding, distinct_lines_by_path, group_id, is_ranked, places_title,
};
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
const CASES_MIN: usize = 3;
/// A site of fewer cases (one position of a tuple match) may join a set but never starts one.
const PARTIAL_CASES_MIN: usize = 2;
const SITES_MIN: usize = 3;

pub struct Parallel;

impl Detector for Parallel {
    fn name(&self) -> &'static str {
        "parallel"
    }

    fn describe(&self) -> &'static str {
        "Names that are versions of each other, and sets of cases matched in several places with catch-all arms, constant tables or one real arm"
    }

    fn detect(&self, context: &Context) -> Result<Vec<Finding>, QueryError> {
        let symbols = context.index.symbols()?;
        let mut findings = Vec::new();
        let keys = context.index.member_keys()?;
        findings.extend(families(&symbols).iter().map(|f| family_finding(f, &keys)));
        findings.extend(
            discriminators(context.index)?
                .iter()
                .filter_map(discriminator_finding),
        );
        Ok(findings)
    }
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
    is_type: bool,
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
        .filter_map(|((is_type, stem), members)| {
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
                is_type,
                members: members.into_iter().map(|(symbol, _)| symbol).collect(),
            })
        })
        .collect()
}

fn family_finding(family: &Family, keys: &BTreeMap<String, String>) -> Finding {
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
    let mass = distinct_lines_by_path(
        family
            .members
            .iter()
            .map(|s| (s.path.as_str(), s.start_line, s.end_line)),
    )
    .values()
    .sum::<u32>();
    let key = [format!(
        "{}:{}",
        if family.is_type { "type" } else { "function" },
        family.stem
    )];
    let mut finding = Finding::group(
        "parallel",
        &group_id("parallel", &key),
        places_title(
            &format!("Versions of {}", family.stem),
            family.members.len(),
            files.len(),
        ),
        reason,
        f64::from(mass),
        0.7,
    )
    .measure("members", number(family.members.len()))
    .measure("names", number(names.len()))
    .measure("files", number(files.len()))
    .measure(
        "totalLines",
        family.members.iter().map(|s| f64::from(lines(s))).sum(),
    );
    finding
        .labels
        .insert("signal".to_owned(), "naming_family".to_owned());
    finding
        .labels
        .insert("stem".to_owned(), family.stem.clone());
    finding = finding.members(
        family
            .members
            .iter()
            .map(|m| keys.get(&m.id).cloned().unwrap_or_default()),
    );
    for member in &family.members {
        finding = finding.evidence(&member.path, &[(member.start_line, member.end_line)]);
    }
    finding
}

// --- repeated discriminators ---

/// A catch-all arm: a case added later falls into it without anyone seeing it. The heaviest
/// signal, because the miss is silent.
const WILDCARD_WEIGHT: f64 = 4.0;
/// One or two arms with real bodies and the rest the same constant: a missing accessor or a
/// `matches!`.
const DEGENERATE_WEIGHT: f64 = 2.0;
/// Arms that only map cases to constants: several of them over one set are a missing data table.
const TABLE_WEIGHT: f64 = 2.0;
/// Two arms for different cases with the same body: the logic written once per variant.
const DUPLICATED_WEIGHT: f64 = 2.0;
/// A set is a finding only when this many of its sites are flagged: a single catch-all is a
/// linter's finding, a pattern across sites is codemap's.
const FLAGGED_SITES_MIN: usize = 2;
/// Confidence is `score / (score + knee)`; it only breaks ties.
const SCORE_KNEE: f64 = 12.0;
/// Confidence floor of literal tags, which count every site.
const LITERAL_CONFIDENCE: f64 = 0.6;
/// A site belongs to the case set whose cases hold this share of its own, so a site that handles
/// some cases and catches the rest is still a site of the whole set.
const SUBSET_SHARE: f64 = 0.75;

/// One place that switches on a case set.
#[expect(
    clippy::struct_excessive_bools,
    reason = "four independent features of one site"
)]
struct Site {
    /// The enclosing symbol's scope-qualified key, or the path.
    key: String,
    path: String,
    start_line: u32,
    end_line: u32,
    wildcard: bool,
    table: bool,
    degenerate: bool,
    duplicated: bool,
    /// One position of a tuple match.
    partial: bool,
    /// Where a type with no definition in the index is a dependency's, if anywhere.
    type_scope: Option<String>,
}

impl Site {
    /// What the site adds to its set's score: the heaviest thing it is, or nothing for a site
    /// whose arms are distinct behaviour.
    fn weight(&self) -> f64 {
        if self.wildcard {
            WILDCARD_WEIGHT
        } else if self.degenerate {
            DEGENERATE_WEIGHT
        } else if self.table {
            TABLE_WEIGHT
        } else if self.duplicated {
            DUPLICATED_WEIGHT
        } else {
            0.0
        }
    }
}

/// A case set and every place that switches on it (or on most of it).
struct CaseSet {
    /// The cases of the set, which give its finding a stable id.
    labels: Vec<String>,
    sites: Vec<Site>,
    /// Literal tags (strings, numbers) are always candidates: every site counts.
    literal: bool,
}

/// Strings and numbers written out, as against names of an enum or constants.
fn is_literal(label: &str) -> bool {
    label.starts_with(['"', '\'', '`', '-']) || label.starts_with(|c: char| c.is_ascii_digit())
}

fn discriminators(index: &Index) -> Result<Vec<CaseSet>, QueryError> {
    let mut statement = index.store().connection().prepare(
        "SELECT w.path, w.start_line, w.end_line, w.labels, l.role,
                w.wildcard, w.table_like, w.degenerate, w.symbol, w.duplicated, w.partial, f.type_scope
         FROM switches w JOIN lines l ON l.path = w.path AND l.line = w.start_line
         JOIN files f ON f.path = w.path
         ORDER BY w.labels, w.path, w.start_line",
    )?;
    let keys = index.member_keys()?;
    let rows = statement.query_map(params![], |row| {
        let path: String = row.get(0)?;
        let symbol: Option<String> = row.get(8)?;
        Ok((
            Site {
                key: symbol
                    .and_then(|id| keys.get(&id).cloned())
                    .unwrap_or_else(|| path.clone()),
                path,
                start_line: row.get(1)?,
                end_line: row.get(2)?,
                wildcard: row.get(5)?,
                table: row.get(6)?,
                degenerate: row.get(7)?,
                duplicated: row.get(9)?,
                partial: row.get(10)?,
                type_scope: row.get(11)?,
            },
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut sets: BTreeMap<Vec<String>, Vec<Site>> = BTreeMap::new();
    for row in rows {
        let (site, labels, role) = row?;
        if is_ranked(&role) && role != Role::Test.as_str() {
            let labels: Vec<String> = labels.split(LABEL_SEPARATOR).map(str::to_owned).collect();
            // Only a tuple position may be a site of fewer cases than a set needs.
            if labels.len() >= CASES_MIN || site.partial {
                sets.entry(labels).or_default().push(site);
            }
        }
    }
    let (literal, named): (Vec<_>, Vec<_>) = sets
        .into_iter()
        .filter(|(labels, _)| labels.len() >= PARTIAL_CASES_MIN)
        .partition(|(labels, _)| labels.iter().all(|label| is_literal(label)));
    let mut found: Vec<CaseSet> = literal
        .into_iter()
        .filter(|(labels, _)| labels.len() >= CASES_MIN)
        .map(|(labels, sites)| CaseSet {
            labels,
            sites,
            literal: true,
        })
        .collect();
    let defined = defined_types(index)?;
    found.extend(cluster(named).into_iter().map(|mut set| {
        // A dependency's enum cannot grow a case, so its catch-alls are safe; its tables and
        // repeated bodies are still a smell.
        if !defined_here(&set, &defined) {
            for site in &mut set.sites {
                site.wildcard = false;
            }
        }
        set
    }));
    found.retain(|set| set.sites.len() >= SITES_MIN);
    Ok(found)
}

/// Top-level (or module-level) type names by scope: an associated type, or a type of a language
/// outside the scope that shares a name, does not make a dependency's enum local.
fn defined_types(index: &Index) -> Result<BTreeSet<(String, String)>, QueryError> {
    let mut statement = index.store().connection().prepare(
        "SELECT f.type_scope, s.name
         FROM symbols s JOIN files f ON f.path = s.path
         LEFT JOIN symbols p ON p.id = s.parent
         WHERE f.type_scope IS NOT NULL AND (p.id IS NULL OR p.kind = 'module')",
    )?;
    let rows = statement.query_map(params![], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The type a path-qualified case belongs to: `Value` in `serde_json::Value::Null`. A binding
/// (`x @ Kind::A`) is not part of the path, and a field access (`TAGS.a`) is not a type.
fn qualifier(label: &str) -> Option<&str> {
    let (path, _) = label.rsplit_once("::")?;
    let path = path.rsplit('@').next().unwrap_or(path).trim();
    // `Kind::<u8>::A` has the generic arguments between the type and the case.
    let path = path.split('<').next().unwrap_or(path);
    path.rsplit(':').find(|part| !part.is_empty())
}

/// Whether a set could grow a case in this codebase, so its catch-alls matter. A set whose cases are all qualified by a
/// type that nothing in the indexed code defines (`Value::Null` with no `Value` here) belongs to
/// a dependency, which cannot gain cases under the code that matches on it. A name counts when
/// a top-level (or module-level) symbol of that name is in the type scope of the matching code.
/// A site whose language has no type scope proves nothing and keeps the set. It cannot see an
/// enum imported under another name (`use a::Real as Alias` looks external), an enum declared
/// by a macro, a dependency's type that shares a name with a local one (looks local), or a type
/// nested in another.
fn defined_here(set: &CaseSet, defined: &BTreeSet<(String, String)>) -> bool {
    let scopes: Option<BTreeSet<&str>> = set
        .sites
        .iter()
        .map(|site| site.type_scope.as_deref())
        .collect();
    let Some(scopes) = scopes else {
        return true;
    };
    set.labels.iter().any(|label| match qualifier(label) {
        // `Self::A` is the enum of the impl, and `crate::A` names a re-export, which is
        // resolved by name only.
        Some(name) => {
            matches!(name, "Self" | "crate" | "self" | "super")
                || scopes
                    .iter()
                    .any(|&scope| defined.contains(&(scope.to_owned(), name.to_owned())))
        }
        None => true,
    })
}

/// Groups case sets that are mostly the same cases, the largest first: the biggest set stands
/// for the type, and a smaller one that is mostly inside it is a partial match on that type.
fn cluster(mut sets: Vec<(Vec<String>, Vec<Site>)>) -> Vec<CaseSet> {
    sets.sort_by_key(|(labels, _)| std::cmp::Reverse(labels.len()));
    let mut clusters: Vec<(BTreeSet<String>, Vec<Site>)> = Vec::new();
    // Clusters each label belongs to, so a set only meets the clusters it shares a case with.
    let mut by_label: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (labels, sites) in sets {
        let mut shared: BTreeMap<usize, usize> = BTreeMap::new();
        for label in &labels {
            for &at in by_label.get(label).into_iter().flatten() {
                *shared.entry(at).or_default() += 1;
            }
        }
        // Fewer cases than a set needs may only join one, and only by name (`Kind::A`).
        let partial = labels.len() < CASES_MIN;
        let qualified = labels.iter().all(|label| label.contains(['.', ':']));
        let home = shared.into_iter().find(|&(_, shared)| {
            shared >= CASES_MIN.min(labels.len())
                && number(shared) >= SUBSET_SHARE * number(labels.len())
                && (!partial || qualified)
        });
        if home.is_none() && partial {
            continue;
        }
        if let Some((at, _)) = home {
            clusters[at].1.extend(sites);
        } else {
            for label in &labels {
                by_label
                    .entry(label.clone())
                    .or_default()
                    .push(clusters.len());
            }
            clusters.push((labels.into_iter().collect(), sites));
        }
    }
    clusters
        .into_iter()
        .map(|(universe, mut sites)| {
            // Two positions of one tuple match are one place.
            let mut seen = BTreeSet::new();
            sites.retain(|site| seen.insert((site.path.clone(), site.start_line, site.end_line)));
            (universe, sites)
        })
        .map(|(universe, sites)| CaseSet {
            labels: universe.into_iter().collect(),
            sites,
            literal: false,
        })
        .collect()
}

/// The sites that make the set worth a look, or none when it is not: behaviour dispatch is
/// left out, and a set needs [`FLAGGED_SITES_MIN`] sites that stand out.
fn flagged(set: &CaseSet) -> Vec<&Site> {
    if set.literal {
        return set.sites.iter().collect();
    }
    let flagged: Vec<&Site> = set
        .sites
        .iter()
        .filter(|site| site.weight() > 0.0)
        .collect();
    if flagged.len() < FLAGGED_SITES_MIN {
        return Vec::new();
    }
    flagged
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// How many flagged sites have each feature; a site with several counts in each.
struct Counts {
    wildcards: usize,
    degenerate: usize,
    tables: usize,
    duplicated: usize,
}

/// The reason, as the counts alone.
fn discriminator_reason(set: &CaseSet, counts: &Counts) -> String {
    let mut details = Vec::new();
    if counts.tables > 0 {
        details.push(format!("case-to-constant tables: {}", counts.tables));
    }
    if counts.degenerate > 0 {
        details.push(format!(
            "one or two own arms, the rest one constant: {}",
            counts.degenerate
        ));
    }
    if counts.duplicated > 0 {
        details.push(format!("arms with identical bodies: {}", counts.duplicated));
    }
    if counts.wildcards > 0 {
        details.push(format!("catch-all arms: {}", counts.wildcards));
    }
    let kind = if set.literal {
        "literal cases"
    } else {
        "cases"
    };
    let places = plural(set.sites.len(), "place", "places");
    let detail = if details.is_empty() {
        String::new()
    } else {
        format!(" ({})", details.join(", "))
    };
    format!(
        "A set of {} {kind} is matched in {places}{detail}.",
        set.labels.len()
    )
}

/// The finding for a set, counting only its flagged sites.
fn discriminator_finding(set: &CaseSet) -> Option<Finding> {
    let flagged = flagged(set);
    if flagged.is_empty() {
        return None;
    }
    let count = |pick: fn(&Site) -> bool| flagged.iter().filter(|site| pick(site)).count();
    let counts = Counts {
        wildcards: count(|site| site.wildcard),
        degenerate: count(|site| site.degenerate),
        tables: count(|site| site.table),
        duplicated: count(|site| site.duplicated),
    };
    let score: f64 = flagged.iter().map(|site| site.weight()).sum();
    let confidence = if set.literal {
        LITERAL_CONFIDENCE.max(score / (score + SCORE_KNEE))
    } else {
        score / (score + SCORE_KNEE)
    };
    let files: BTreeSet<&str> = flagged.iter().map(|s| s.path.as_str()).collect();
    let mass = distinct_lines_by_path(
        flagged
            .iter()
            .map(|s| (s.path.as_str(), s.start_line, s.end_line)),
    )
    .values()
    .sum::<u32>();
    let mut finding = Finding::group(
        "parallel",
        &group_id("parallel", &set.labels),
        places_title(
            &format!("Set of {} cases", set.labels.len()),
            flagged.len(),
            files.len(),
        ),
        discriminator_reason(set, &counts),
        f64::from(mass),
        confidence,
    )
    .measure("cases", number(set.labels.len()))
    .measure("sites", number(set.sites.len()))
    .measure("flaggedSites", number(flagged.len()))
    .measure("wildcardSites", number(counts.wildcards))
    .measure("degenerateSites", number(counts.degenerate))
    .measure("tableSites", number(counts.tables))
    .measure("duplicatedSites", number(counts.duplicated))
    .measure("files", number(files.len()))
    .measure("score", score)
    .fact(format!("{} cases", set.labels.len()))
    .fact(format!(
        "matched in {}",
        plural(set.sites.len(), "place", "places")
    ))
    .fact(format!("{} case-to-constant tables", counts.tables))
    .fact(format!(
        "{} with one or two own arms and one constant for the rest",
        counts.degenerate
    ))
    .fact(format!("arms with identical bodies: {}", counts.duplicated))
    .fact(format!("{} with a catch-all arm", counts.wildcards));
    finding
        .labels
        .insert("signal".to_owned(), "repeated_discriminator".to_owned());
    finding = finding.members(flagged.iter().map(|s| s.key.clone()));
    for site in &flagged {
        finding = finding.evidence(&site.path, &[(site.start_line, site.end_line)]);
    }
    Some(finding)
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

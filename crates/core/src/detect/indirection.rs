//! Indirection: code that stands between a caller and the work.
//!
//! Two signals, each over production code outside generated and vendored files (tests are not
//! flagged):
//!
//! - **Pass-through wrappers.** A function of at most [`WRAPPER_LINES_MAX`] lines whose body is
//!   one call that passes its own parameters on (the extractor decides the shape, see
//!   `shape.rs`) and whose callee resolves to a function or type of the codebase with
//!   confidence of at least [`TARGET_CONFIDENCE_MIN`]. Wrappers that forward to wrappers are one
//!   finding, the chain, listed from the outermost call inward and ending at the function that
//!   does the work. A function that forwards to a function of its own name is left out: it
//!   delegates to another type's method, which names cannot tell from recursion. The extractor
//!   gives no forwarded callee to functions whose signature another API dictates: methods a
//!   contract demands (of a class that implements or extends something, or of a trait `impl`
//!   block) and functions that are values in an object literal (a callback option, an interface
//!   member).
//! - **Single-implementation abstractions.** A trait, interface or abstract class that exactly
//!   one production type implements or extends (a blanket implementation, whose implementor is a
//!   generic parameter, counts as many). An abstraction whose name also resolves to
//!   others with low confidence is skipped, because its implementors cannot be counted.
//!
//! A chain of several wrappers and an abstraction with its implementation are groups: one
//! finding, its own area, listing every part as evidence. A lone wrapper is a finding in its
//! file. A chain's id is a digest of its outermost wrapper's path and name and an abstraction's
//! of its own, so they hold while the rest of the group changes.

use std::collections::{BTreeMap, BTreeSet};

use codemap_lang::Role;

use super::{Context, Detector, Finding, SubjectKind, count, group_id, is_ranked, places_title};
use crate::link::{Edge, IMPLEMENTATION, edges};
use crate::reader::{Index, QueryError, SymbolRow};

/// A wrapper is small: a signature and a call, with room for a long parameter list that rustfmt
/// puts one to a line.
const WRAPPER_LINES_MAX: u32 = 10;
/// Edges below this confidence (a name that matches more than two definitions) do not say what
/// a function forwards to.
const TARGET_CONFIDENCE_MIN: f64 = 0.4;
/// An implementor edge below this confidence may belong to another abstraction of the same name.
const IMPLEMENTOR_CONFIDENCE_MIN: f64 = 0.5;
/// Names quoted in a reason.
const NAMES_IN_REASON_MAX: usize = 4;

pub struct Indirection;

impl Detector for Indirection {
    fn name(&self) -> &'static str {
        "indirection"
    }

    fn describe(&self) -> &'static str {
        "Functions that only pass calls on, and abstractions with one implementation"
    }

    fn detect(&self, context: &Context) -> Result<Vec<Finding>, QueryError> {
        let population = Population::load(context.index)?;
        let wrappers = population.wrappers();
        let mut findings = chain_findings(&population, &wrappers);
        findings.extend(abstraction_findings(&population));
        Ok(findings)
    }
}

fn lines(symbol: &SymbolRow) -> u32 {
    symbol.end_line - symbol.start_line + 1
}

fn number(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

fn subject(symbol: &SymbolRow) -> String {
    format!("{}::{}#{}", symbol.path, symbol.name, symbol.start_line)
}

fn names(symbols: &[&SymbolRow]) -> String {
    let shown: Vec<String> = symbols
        .iter()
        .take(NAMES_IN_REASON_MAX)
        .map(|symbol| format!("`{}`", symbol.name))
        .collect();
    let more = symbols.len().saturating_sub(NAMES_IN_REASON_MAX);
    if more > 0 {
        format!("{}, and {more} more", shown.join(", "))
    } else {
        shown.join(", ")
    }
}

/// The symbols and edges the signals read.
struct Population {
    symbols: Vec<SymbolRow>,
    by_id: BTreeMap<String, usize>,
    edges: Vec<Edge>,
    /// Scope-qualified keys by symbol id, for group identities.
    keys: BTreeMap<String, String>,
}

impl Population {
    fn load(index: &Index) -> Result<Self, QueryError> {
        let symbols = index.symbols()?;
        let by_id = symbols
            .iter()
            .enumerate()
            .map(|(at, symbol)| (symbol.id.clone(), at))
            .collect();
        Ok(Self {
            symbols,
            by_id,
            edges: edges(index)?,
            keys: index.member_keys()?,
        })
    }

    fn key(&self, symbol: &SymbolRow) -> String {
        self.keys.get(&symbol.id).cloned().unwrap_or_default()
    }

    fn get(&self, id: &str) -> Option<&SymbolRow> {
        self.by_id.get(id).map(|&at| &self.symbols[at])
    }

    /// Production code a person can edit.
    fn is_production(symbol: &SymbolRow) -> bool {
        is_ranked(&symbol.role) && symbol.role != Role::Test.as_str()
    }

    fn functions(&self) -> impl Iterator<Item = &SymbolRow> {
        self.symbols
            .iter()
            .filter(|symbol| symbol.kind == "function" && Self::is_production(symbol))
    }

    /// Each wrapper (by id) with the symbol it forwards to and how sure the link is.
    fn wrappers(&self) -> BTreeMap<&str, (&SymbolRow, f64)> {
        let mut from: BTreeMap<&str, Vec<&Edge>> = BTreeMap::new();
        for edge in self.edges.iter().filter(|edge| edge.kind != IMPLEMENTATION) {
            from.entry(&edge.from).or_default().push(edge);
        }
        let mut found = BTreeMap::new();
        for function in self.functions() {
            let Some(callee) = function.forwards.as_deref() else {
                continue;
            };
            if lines(function) > WRAPPER_LINES_MAX {
                continue;
            }
            let target = from
                .get(function.id.as_str())
                .into_iter()
                .flatten()
                .filter(|edge| edge.confidence >= TARGET_CONFIDENCE_MIN)
                .filter_map(|edge| Some((self.get(&edge.to)?, edge.confidence)))
                .filter(|(target, _)| target.name == callee && target.name != function.name)
                .max_by(|a, b| a.1.total_cmp(&b.1));
            if let Some(target) = target {
                found.insert(function.id.as_str(), target);
            }
        }
        found
    }
}

type Wrappers<'a> = BTreeMap<&'a str, (&'a SymbolRow, f64)>;

/// One chain: the wrappers from the outermost inward and the function that does the work.
struct Chain<'a> {
    wrappers: Vec<&'a SymbolRow>,
    worker: &'a SymbolRow,
    confidence: f64,
}

/// Wrappers joined by forwarding to each other, as chains.
fn chains<'a>(population: &'a Population, wrappers: &Wrappers<'a>) -> Vec<Chain<'a>> {
    let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
    for (&id, (target, _)) in wrappers {
        if wrappers.contains_key(target.id.as_str()) {
            parent.insert(id, &target.id);
        }
    }
    // The last wrapper of a component. Wrappers that forward round a cycle have no function
    // that does the work, so they make no chain.
    let end_of = |start: &'a str| -> Option<&'a str> {
        let mut at = start;
        let mut seen = BTreeSet::new();
        while let Some(&next) = parent.get(at) {
            if !seen.insert(at) {
                return None;
            }
            at = next;
        }
        Some(at)
    };
    let mut components: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for &id in wrappers.keys() {
        if let Some(end) = end_of(id) {
            components.entry(end).or_default().push(id);
        }
    }
    components
        .into_values()
        .filter_map(|ids| {
            let mut members: Vec<&SymbolRow> =
                ids.iter().filter_map(|id| population.get(id)).collect();
            // Outermost first: the most forwarding steps to the end.
            let depth = |symbol: &SymbolRow| {
                let mut steps = 0;
                let mut at = symbol.id.as_str();
                while let Some(&next) = parent.get(at) {
                    steps += 1;
                    at = next;
                    if steps > ids.len() {
                        break;
                    }
                }
                steps
            };
            members.sort_by_key(|symbol| {
                (
                    std::cmp::Reverse(depth(symbol)),
                    symbol.path.clone(),
                    symbol.start_line,
                )
            });
            let last = members.last()?;
            let (worker, _) = wrappers[last.id.as_str()];
            let confidence = ids.iter().map(|id| wrappers[id].1).fold(1.0, f64::min);
            Some(Chain {
                wrappers: members,
                worker,
                confidence,
            })
        })
        .collect()
}

fn chain_findings(population: &Population, wrappers: &Wrappers) -> Vec<Finding> {
    chains(population, wrappers)
        .iter()
        .map(|chain| chain_finding(chain, population))
        .collect()
}

fn chain_reason(chain: &Chain) -> String {
    let [only] = chain.wrappers.as_slice() else {
        return format!(
            "{} functions each only pass their arguments on, from {} down to `{}`, which does the work.",
            chain.wrappers.len(),
            names(&chain.wrappers),
            chain.worker.name
        );
    };
    format!(
        "`{}` is {} lines that only pass its arguments on to `{}`.",
        only.name,
        lines(only),
        chain.worker.name
    )
}

/// One finding for the chain; a lone wrapper is a function in its file, a longer chain a group.
fn chain_finding(chain: &Chain, population: &Population) -> Finding {
    let head = chain.wrappers[0];
    let total: u32 = chain.wrappers.iter().map(|symbol| lines(symbol)).sum();
    let files: BTreeSet<&str> = chain.wrappers.iter().map(|s| s.path.as_str()).collect();
    let (reason, confidence) = (chain_reason(chain), chain.confidence);
    let mut finding = if chain.wrappers.len() == 1 {
        Finding::new(
            "indirection",
            SubjectKind::Function,
            &subject(head),
            reason,
            f64::from(total),
            confidence,
        )
    } else {
        // The outermost wrapper names the chain: inner links come and go.
        let outermost = [population.key(head)];
        Finding::group(
            "indirection",
            &group_id("indirection", &outermost),
            places_title(
                &format!("Pass-through chain to {}", chain.worker.name),
                chain.wrappers.len(),
                files.len(),
            ),
            reason,
            f64::from(total),
            confidence,
        )
    }
    .measure("chainLength", number(chain.wrappers.len()))
    .measure("files", number(files.len()))
    .measure("wrapperLines", f64::from(total))
    .fact(format!(
        "{} line{} of wrapper in all",
        count(f64::from(total)),
        if total == 1 { "" } else { "s" }
    ));
    finding
        .labels
        .insert("signal".to_owned(), "pass_through".to_owned());
    finding = finding.members(
        chain
            .wrappers
            .iter()
            .chain([&chain.worker])
            .map(|s| population.key(s)),
    );
    for symbol in chain.wrappers.iter().chain([&chain.worker]) {
        finding = finding.evidence(&symbol.path, &[(symbol.start_line, symbol.end_line)]);
    }
    finding
}

fn abstraction_findings(population: &Population) -> Vec<Finding> {
    let mut implementors: BTreeMap<&str, Vec<&Edge>> = BTreeMap::new();
    for edge in population.edges.iter().filter(|e| e.kind == IMPLEMENTATION) {
        implementors.entry(&edge.to).or_default().push(edge);
    }
    population
        .symbols
        .iter()
        .filter(|symbol| symbol.abstraction && Population::is_production(symbol))
        .filter_map(|abstraction| {
            let incoming = implementors.get(abstraction.id.as_str())?;
            if incoming
                .iter()
                .any(|edge| edge.confidence < IMPLEMENTOR_CONFIDENCE_MIN)
            {
                return None;
            }
            let production: Vec<&SymbolRow> = incoming
                .iter()
                .filter_map(|edge| population.get(&edge.from))
                .filter(|symbol| Population::is_production(symbol))
                .collect();
            // A single capital letter names a generic parameter: a blanket implementation,
            // which stands for any number of types.
            let distinct: BTreeSet<(&str, &str)> = production
                .iter()
                .map(|s| (s.path.as_str(), s.name.as_str()))
                .collect();
            if distinct.iter().any(|(_, name)| name.len() == 1) {
                return None;
            }
            let only = production.first()?;
            (distinct.len() == 1).then(|| abstraction_finding(abstraction, only, population))
        })
        .collect()
}

/// One finding for the pair, pointing at both.
fn abstraction_finding(
    abstraction: &SymbolRow,
    only: &SymbolRow,
    population: &Population,
) -> Finding {
    let files: BTreeSet<&str> = [abstraction.path.as_str(), only.path.as_str()].into();
    let mut finding = Finding::group(
        "indirection",
        &group_id("indirection", &[population.key(abstraction)]),
        places_title(
            &format!("Single implementation of {}", abstraction.name),
            2,
            files.len(),
        ),
        format!(
            "`{}` has a single implementation, `{}`.",
            abstraction.name, only.name
        ),
        f64::from(lines(abstraction)),
        1.0,
    )
    .measure("implementors", 1.0)
    .measure("abstractionLines", f64::from(lines(abstraction)))
    .evidence(
        &abstraction.path,
        &[(abstraction.start_line, abstraction.end_line)],
    )
    .evidence(&only.path, &[(only.start_line, only.end_line)])
    .members([population.key(abstraction), population.key(only)]);
    finding
        .labels
        .insert("signal".to_owned(), "single_implementation".to_owned());
    finding
}

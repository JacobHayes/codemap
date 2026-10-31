//! The link analysis: references as written become edges between symbols, with a confidence,
//! and every symbol gets `fan_in` and `fan_out` measures. It reads the whole index and is
//! recomputed with it.
//!
//! Resolution is by name, so it is approximate. A reference is looked up among the functions and
//! types with that name (a call may mean either, because constructors are calls; a type or
//! implementation reference means a type), in three tiers, and the first tier with a candidate
//! decides:
//!
//! 1. **The same file**, confidence [`SAME_FILE`].
//! 2. **The same directory**, confidence [`SAME_DIRECTORY`].
//! 3. **The whole codebase**, confidence [`CODEBASE`].
//!
//! When the tier holds several candidates the confidence is split between them, each edge
//! getting `tier / candidates`. A tier with more than [`CANDIDATES_MAX`] candidates is
//! ambiguous beyond use (`new`, `get`, `len`) and the reference is dropped rather than spread
//! thin. Further rules:
//!
//! - Generated and vendored symbols are neither ends of an edge: nobody can cut them.
//! - A reference outside any symbol, and one that resolves to the symbol it sits in
//!   (recursion, or a method delegating to a same-named method of another type), makes no edge.
//! - An implementation reference that names the symbol it sits in (the grammar reads
//!   `impl Foo { .. }` as an implementation of `Foo`) is a scope, not a relation.
//! - Several references from one symbol to another of one kind are one edge, with the best
//!   confidence.
//! - `fan_in` and `fan_out` sum the confidence of a symbol's incoming and outgoing edges, so an
//!   ambiguous name counts for a fraction. Tests are sources like any other symbol.

use std::collections::BTreeMap;

use duckdb::params;

use crate::detect::is_ranked;
use crate::reader::{Index, QueryError, SymbolRow};

pub(crate) const SAME_FILE: f64 = 1.0;
pub(crate) const SAME_DIRECTORY: f64 = 0.9;
pub(crate) const CODEBASE: f64 = 0.8;
/// Beyond this many same-named candidates in a tier, the name says too little to link.
const CANDIDATES_MAX: usize = 4;

pub(crate) const IMPLEMENTATION: &str = "implementation";

/// A resolved reference.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Edge {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub confidence: f64,
}

struct Reference {
    path: String,
    name: String,
    kind: String,
    from: String,
}

fn references(index: &Index) -> Result<Vec<Reference>, QueryError> {
    let mut statement = index.store().connection().prepare(
        "SELECT path, name, kind, symbol FROM refs WHERE symbol IS NOT NULL ORDER BY path, line",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(Reference {
            path: row.get(0)?,
            name: row.get(1)?,
            kind: row.get(2)?,
            from: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// The candidates by name and then by file, in path order.
struct Definitions<'a>(BTreeMap<&'a str, BTreeMap<&'a str, Vec<&'a SymbolRow>>>);

impl<'a> Definitions<'a> {
    fn new(symbols: &[&'a SymbolRow]) -> Self {
        let mut by_name: BTreeMap<&str, BTreeMap<&str, Vec<&SymbolRow>>> = BTreeMap::new();
        for &symbol in symbols {
            by_name
                .entry(&symbol.name)
                .or_default()
                .entry(&symbol.path)
                .or_default()
                .push(symbol);
        }
        Self(by_name)
    }

    /// The candidates of the first tier that has any, and that tier's confidence.
    fn resolve(&self, reference: &Reference) -> Option<(Vec<&'a SymbolRow>, f64)> {
        let files = self.0.get(reference.name.as_str())?;
        let accepts = |symbol: &&SymbolRow| match reference.kind.as_str() {
            "call" | "class" => matches!(symbol.kind.as_str(), "function" | "type"),
            _ => symbol.kind == "type",
        };
        let pick = |tier: Vec<&'a SymbolRow>| (!tier.is_empty()).then_some(tier);
        let here = pick(
            files
                .get(reference.path.as_str())
                .into_iter()
                .flatten()
                .copied()
                .filter(accepts)
                .collect(),
        );
        if let Some(found) = here {
            return Some((found, SAME_FILE));
        }
        let dir = directory(&reference.path);
        let near = pick(
            files
                .iter()
                .filter(|(path, _)| directory(path) == dir)
                .flat_map(|(_, symbols)| symbols.iter().copied())
                .filter(accepts)
                .collect(),
        );
        if let Some(found) = near {
            return Some((found, SAME_DIRECTORY));
        }
        pick(files.values().flatten().copied().filter(accepts).collect())
            .map(|found| (found, CODEBASE))
    }
}

/// Resolves every reference. Edges are keyed by (from, to, kind) with the best confidence.
fn resolve_all(symbols: &[SymbolRow], refs: &[Reference]) -> Vec<Edge> {
    let ranked: Vec<&SymbolRow> = symbols
        .iter()
        .filter(|symbol| is_ranked(&symbol.role))
        .collect();
    let by_id: BTreeMap<&str, &SymbolRow> = ranked
        .iter()
        .map(|symbol| (symbol.id.as_str(), *symbol))
        .collect();
    let definitions = Definitions::new(
        &ranked
            .iter()
            .copied()
            .filter(|symbol| matches!(symbol.kind.as_str(), "function" | "type"))
            .collect::<Vec<_>>(),
    );
    let mut edges: BTreeMap<(&str, &str, &str), f64> = BTreeMap::new();
    for reference in refs {
        let Some(source) = by_id.get(reference.from.as_str()) else {
            continue;
        };
        if reference.kind == IMPLEMENTATION && reference.name == source.name {
            continue;
        }
        let Some((candidates, tier)) = definitions.resolve(reference) else {
            continue;
        };
        if candidates.len() > CANDIDATES_MAX {
            continue;
        }
        #[expect(clippy::cast_precision_loss, reason = "at most CANDIDATES_MAX")]
        let confidence = tier / candidates.len() as f64;
        for target in candidates.into_iter().filter(|t| t.id != source.id) {
            let best = edges
                .entry((&source.id, &target.id, &reference.kind))
                .or_insert(0.0);
            *best = best.max(confidence);
        }
    }
    edges
        .into_iter()
        .map(|((from, to, kind), confidence)| Edge {
            from: from.to_owned(),
            to: to.to_owned(),
            kind: kind.to_owned(),
            confidence,
        })
        .collect()
}

/// Rewrites `edges` and the `fan_in`/`fan_out` rows of `measures`.
pub(crate) fn run(index: &Index) -> Result<(), QueryError> {
    let symbols = index.symbols()?;
    let edges = resolve_all(&symbols, &references(index)?);
    let connection = index.store().connection();
    connection.execute_batch("DELETE FROM edges; DELETE FROM measures;")?;
    let mut rows = connection.appender("edges")?;
    let mut fan: BTreeMap<(&str, &str), f64> = BTreeMap::new();
    for edge in &edges {
        rows.append_row(params![edge.from, edge.to, edge.kind, edge.confidence])?;
        *fan.entry((&edge.from, "fan_out")).or_default() += edge.confidence;
        *fan.entry((&edge.to, "fan_in")).or_default() += edge.confidence;
    }
    rows.flush()?;
    drop(rows);
    let mut measures = connection.appender("measures")?;
    for ((subject, key), value) in fan {
        measures.append_row(params![subject, key, value])?;
    }
    measures.flush()?;
    Ok(())
}

/// Every edge, in a stable order.
pub(crate) fn edges(index: &Index) -> Result<Vec<Edge>, QueryError> {
    let mut statement = index.store().connection().prepare(
        "SELECT from_symbol, to_symbol, kind, confidence FROM edges
         ORDER BY from_symbol, to_symbol, kind",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(Edge {
            from: row.get(0)?,
            to: row.get(1)?,
            kind: row.get(2)?,
            confidence: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(id: &str, path: &str, name: &str, kind: &str) -> SymbolRow {
        SymbolRow {
            id: id.to_owned(),
            path: path.to_owned(),
            name: name.to_owned(),
            kind: kind.to_owned(),
            parent: None,
            start_line: 1,
            end_line: 3,
            role: "prod".to_owned(),
            forwards: None,
            abstraction: false,
        }
    }

    fn reference(path: &str, name: &str, kind: &str, from: &str) -> Reference {
        Reference {
            path: path.to_owned(),
            name: name.to_owned(),
            kind: kind.to_owned(),
            from: from.to_owned(),
        }
    }

    /// Target and confidence of every edge, by target.
    fn linked(symbols: &[SymbolRow], refs: &[Reference]) -> BTreeMap<String, f64> {
        resolve_all(symbols, refs)
            .into_iter()
            .map(|edge| (edge.to, edge.confidence))
            .collect()
    }

    #[test]
    fn the_nearest_tier_with_a_candidate_decides() {
        let symbols = [
            symbol("a#0", "src/a.rs", "caller", "function"),
            symbol("a#1", "src/a.rs", "work", "function"),
            symbol("b#0", "src/b.rs", "work", "function"),
            symbol("b#1", "src/b.rs", "near", "function"),
            symbol("c#0", "lib/c.rs", "far", "function"),
        ];
        let refs = ["work", "near", "far"].map(|name| reference("src/a.rs", name, "call", "a#0"));
        let edges = linked(&symbols, &refs);
        // `work` is in the file itself, so the copy next door is not a candidate.
        assert_eq!(edges["a#1"], SAME_FILE);
        assert!(!edges.contains_key("b#0"));
        assert_eq!(edges["b#1"], SAME_DIRECTORY);
        assert_eq!(edges["c#0"], CODEBASE);
    }

    #[test]
    fn ambiguity_splits_confidence_and_beyond_the_limit_drops_the_reference() {
        let mut symbols = vec![symbol("a#0", "src/a.rs", "caller", "function")];
        for at in 0..2 {
            symbols.push(symbol(
                &format!("x{at}#0"),
                &format!("lib/x{at}.rs"),
                "pair",
                "function",
            ));
        }
        for at in 0..=CANDIDATES_MAX {
            symbols.push(symbol(
                &format!("y{at}#0"),
                &format!("lib/y{at}.rs"),
                "many",
                "function",
            ));
        }
        let refs = ["pair", "many"].map(|name| reference("src/a.rs", name, "call", "a#0"));
        let edges = linked(&symbols, &refs);
        assert_eq!(edges["x0#0"], CODEBASE / 2.0);
        assert_eq!(edges["x1#0"], CODEBASE / 2.0);
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn recursion_scopes_and_symbols_nobody_can_cut_make_no_edge() {
        let mut vendored = symbol("v#0", "vendor/v.rs", "lib_call", "function");
        "vendored".clone_into(&mut vendored.role);
        let symbols = [
            symbol("a#0", "src/a.rs", "again", "function"),
            symbol("a#1", "src/a.rs", "Shape", "type"),
            symbol("a#2", "src/a.rs", "Shape", "type"),
            vendored,
        ];
        let refs = [
            reference("src/a.rs", "again", "call", "a#0"),
            // `impl Shape { .. }` names the scope it sits in.
            reference("src/a.rs", "Shape", IMPLEMENTATION, "a#2"),
            reference("src/a.rs", "lib_call", "call", "a#0"),
            reference("src/a.rs", "again", "call", "missing"),
        ];
        assert!(linked(&symbols, &refs).is_empty());
    }

    #[test]
    fn a_type_reference_does_not_mean_a_function_and_repeats_are_one_edge() {
        let symbols = [
            symbol("a#0", "src/a.rs", "caller", "function"),
            symbol("a#1", "src/a.rs", "make", "function"),
            symbol("a#2", "src/a.rs", "Made", "type"),
        ];
        let refs = [
            reference("src/a.rs", "make", "type", "a#0"),
            reference("src/a.rs", "Made", "type", "a#0"),
            reference("src/a.rs", "Made", "type", "a#0"),
        ];
        let edges = resolve_all(&symbols, &refs);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].to, "a#2");
    }
}

//! `Tree`: a hierarchy with a size and a colour measure, shaped for the treemap.
//!
//! Every row (a file, or a symbol with its file) is a chain of nodes from the root down. Chains
//! are cut at the depth limit, so deeper mass lands on the deepest drawn ancestor. A node's
//! `size` is its own mass; a parent's total is the sum over its subtree. `colorStep` and `value`
//! are those of the whole subtree, so an aggregated tile is coloured by what it aggregates.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::Hidden;
use super::scale::{self, Scale};
use crate::measure::{self, Filter, Measure, MeasureRef};
use crate::reader::{Index, QueryError, SymbolRow};

/// Levels drawn below the root unless the query says otherwise.
pub const DEPTH_DEFAULT: u32 = 3;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "Query.ts")]
pub enum Hierarchy {
    /// Directories, then files.
    #[default]
    Path,
    /// Files, then the symbols nested in them.
    Symbol,
}

/// What a tile's size means.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct SizeInfo {
    pub measure: String,
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct TreeNode {
    /// Position in `nodes`; a parent has a smaller id than its children.
    pub id: u32,
    pub parent: Option<u32>,
    pub name: String,
    /// The node's own mass; the layout adds the descendants'.
    pub size: f64,
    /// The mass of the node and everything under it.
    pub total: f64,
    /// 0 to `scale.steps - 1`; null when the node has no colour value.
    pub color_step: Option<u32>,
    /// The colour measure over the whole subtree.
    pub value: Option<f64>,
    /// `directory`, `file`, or a symbol kind.
    pub kind: String,
    /// The file or directory this node is, or contains the symbol it is.
    pub path: String,
    /// Rows cut off below this node by the depth limit.
    pub hidden: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct Tree {
    pub hierarchy: Hierarchy,
    pub depth: u32,
    pub size: SizeInfo,
    pub total: f64,
    pub nodes: Vec<TreeNode>,
    pub scale: Option<Scale>,
    /// What the default population left out.
    pub hidden: Option<Hidden>,
}

#[derive(Clone)]
struct Link {
    key: String,
    name: String,
    kind: String,
    path: String,
}

struct Row {
    chain: Vec<Link>,
    sums: Vec<f64>,
}

struct Acc {
    parent: Option<usize>,
    link: Link,
    inclusive: Vec<f64>,
    own: f64,
    hidden: u32,
}

pub(crate) struct Args<'a> {
    pub hierarchy: Hierarchy,
    pub size: &'a Measure,
    pub size_label: String,
    pub color: Option<(&'a Measure, String)>,
    pub filter: &'a Filter,
    pub depth: u32,
}

pub(crate) fn tree(index: &Index, args: &Args) -> Result<Tree, QueryError> {
    let Measure::Sum { base, .. } = args.size else {
        return Err(QueryError::SizeIsRatio);
    };
    let mut terms = args.size.terms();
    if let Some((color, _)) = &args.color {
        terms.extend(color.terms());
    }
    let rows = match args.hierarchy {
        Hierarchy::Path => path_rows(index, args.filter, &terms)?,
        Hierarchy::Symbol => symbol_rows(index, args.filter, &terms)?,
    };
    let mut accs: Vec<Acc> = Vec::new();
    let mut at: BTreeMap<String, usize> = BTreeMap::new();
    for row in rows {
        let drawn = row.chain.len().min(args.depth as usize + 1);
        let cut = row.chain.len() > drawn;
        let mut parent = None;
        for (level, link) in row.chain.into_iter().take(drawn).enumerate() {
            let slot = *at.entry(link.key.clone()).or_insert_with(|| {
                accs.push(Acc {
                    parent,
                    link,
                    inclusive: vec![0.0; row.sums.len()],
                    own: 0.0,
                    hidden: 0,
                });
                accs.len() - 1
            });
            let acc = &mut accs[slot];
            acc.inclusive
                .iter_mut()
                .zip(&row.sums)
                .for_each(|(a, b)| *a += b);
            if level + 1 == drawn {
                acc.own += row.sums[0];
                acc.hidden += u32::from(cut);
            }
            parent = Some(slot);
        }
    }
    Ok(finish(&accs, args, base.unit()))
}

fn finish(accs: &[Acc], args: &Args, unit: &str) -> Tree {
    // Nodes with no mass cannot be drawn; their descendants have none either.
    let mut renumbered: Vec<Option<u32>> = Vec::with_capacity(accs.len());
    let mut kept = 0_u32;
    for acc in accs {
        if acc.inclusive[0] > 0.0 {
            renumbered.push(Some(kept));
            kept += 1;
        } else {
            renumbered.push(None);
        }
    }
    let color_of = |acc: &Acc| {
        args.color
            .as_ref()
            .and_then(|(measure, _)| measure.value(&acc.inclusive[1..]))
    };
    let mut has_child = vec![false; accs.len()];
    for acc in accs {
        if let (Some(parent), true) = (acc.parent, acc.inclusive[0] > 0.0) {
            has_child[parent] = true;
        }
    }
    // A ratio of zero (no tests, no handling) is not a finding: it takes the plain fill.
    let quiet_zero = args.color.as_ref().is_some_and(|c| c.0.is_ratio());
    let coloured = |value: f64| !(quiet_zero && value <= 0.0);
    let tile_values: Vec<f64> = accs
        .iter()
        .enumerate()
        .filter(|(i, acc)| renumbered[*i].is_some() && !has_child[*i] && acc.inclusive[0] > 0.0)
        .filter_map(|(_, acc)| color_of(acc))
        .collect();
    let scale = args
        .color
        .as_ref()
        .and_then(|(_, label)| scale::build(label, quiet_zero, &tile_values));
    let total = accs.first().map_or(0.0, |acc| acc.inclusive[0]);
    let nodes = accs
        .iter()
        .enumerate()
        .filter_map(|(i, acc)| {
            let id = renumbered[i]?;
            let value = color_of(acc);
            Some(TreeNode {
                id,
                parent: acc.parent.and_then(|p| renumbered[p]),
                name: acc.link.name.clone(),
                size: acc.own,
                total: acc.inclusive[0],
                color_step: scale
                    .as_ref()
                    .zip(value.filter(|v| coloured(*v)))
                    .map(|(scale, v)| scale.step(v)),
                value,
                kind: acc.link.kind.clone(),
                path: acc.link.path.clone(),
                hidden: acc.hidden,
            })
        })
        .collect();
    Tree {
        hierarchy: args.hierarchy,
        depth: args.depth,
        size: SizeInfo {
            measure: args.size_label.clone(),
            unit: unit.to_owned(),
        },
        total,
        nodes,
        scale,
        hidden: None,
    }
}

fn path_rows(
    index: &Index,
    filter: &Filter,
    terms: &[measure::Term],
) -> Result<Vec<Row>, QueryError> {
    let root_path = filter
        .path
        .as_deref()
        .unwrap_or("")
        .trim_matches('/')
        .to_owned();
    let root_name = root_path
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .map_or_else(|| index.root_name(), str::to_owned);
    let mut rows = Vec::new();
    for (path, sums) in index.file_sums(filter, terms)? {
        let mut chain = vec![Link {
            key: root_path.clone(),
            name: root_name.clone(),
            kind: if path == root_path {
                "file"
            } else {
                "directory"
            }
            .to_owned(),
            path: root_path.clone(),
        }];
        if path != root_path {
            let below = if root_path.is_empty() {
                path.as_str()
            } else {
                &path[root_path.len() + 1..]
            };
            let parts: Vec<&str> = below.split('/').collect();
            let mut key = root_path.clone();
            for (i, part) in parts.iter().enumerate() {
                key = if key.is_empty() {
                    (*part).to_owned()
                } else {
                    format!("{key}/{part}")
                };
                chain.push(Link {
                    key: key.clone(),
                    name: (*part).to_owned(),
                    kind: if i + 1 == parts.len() {
                        "file"
                    } else {
                        "directory"
                    }
                    .to_owned(),
                    path: key.clone(),
                });
            }
        }
        rows.push(Row { chain, sums });
    }
    Ok(rows)
}

fn symbol_rows(
    index: &Index,
    filter: &Filter,
    terms: &[measure::Term],
) -> Result<Vec<Row>, QueryError> {
    let symbols: BTreeMap<String, SymbolRow> = index
        .symbols()?
        .into_iter()
        .map(|symbol| (symbol.id.clone(), symbol))
        .collect();
    let root = Link {
        key: String::new(),
        name: index.root_name(),
        kind: "directory".to_owned(),
        path: String::new(),
    };
    let mut rows = Vec::new();
    for (path, symbol, sums) in index.symbol_sums(filter, terms)? {
        let mut chain = vec![
            root.clone(),
            Link {
                key: path.clone(),
                name: path.clone(),
                kind: "file".to_owned(),
                path: path.clone(),
            },
        ];
        let mut nested = Vec::new();
        let mut next = symbol.as_deref();
        while let Some(found) = next.and_then(|id| symbols.get(id)) {
            nested.push(Link {
                key: found.id.clone(),
                name: found.name.clone(),
                kind: found.kind.clone(),
                path: path.clone(),
            });
            next = found.parent.as_deref();
        }
        chain.extend(nested.into_iter().rev());
        rows.push(Row { chain, sums });
    }
    Ok(rows)
}

/// Resolves a measure reference against the catalog.
pub(crate) fn resolve(reference: &MeasureRef) -> Result<Measure, QueryError> {
    match reference {
        MeasureRef::Named(name) => {
            measure::named(name).ok_or_else(|| QueryError::UnknownMeasure(name.clone()))
        }
        MeasureRef::Inline(measure) => Ok(measure.clone()),
    }
}

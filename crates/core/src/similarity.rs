//! The similarity analysis: exact and near copies among the fingerprinted functions, written to
//! `similar` as pairs (a, b, kind, score). It reads the whole index and is recomputed with it.
//!
//! Functions with the same exact fingerprint form a group, joined to its first member by exact
//! pairs of score 1. One member of each group then stands for it in the near search: `MinHash`
//! signatures are cut into bands, functions that agree on a whole band are candidates (LSH), and a
//! candidate pair is a near pair when its estimated Jaccard similarity reaches
//! [`NEAR_JACCARD_MIN`] and its identifiers as written overlap by at least [`IDENT_JACCARD_MIN`].
//! A pair whose functions are already connected is skipped, so `similar`
//! holds enough pairs to connect every cluster rather than every pair inside it, and a bucket of
//! thousands of look-alikes costs a pass, not a square. A bucket still has a comparison budget
//! ([`BUCKET_COMPARISONS_MAX`]), because rejected candidates stay in it as anchors.
//!
//! The semantic analysis ([`crate::semantic`]) later adds pairs of kind `semantic` for functions
//! that do the same thing in different words.
//!
//! The population is functions and tests outside generated and vendored code: nobody can cut
//! those by editing them.

use std::collections::{BTreeMap, BTreeSet};

use codemap_lang::Role;
use duckdb::params;

use crate::fingerprint::{IDENT_LEN, MINHASH_LEN, jaccard};
use crate::reader::{Index, QueryError};
use crate::semantic::SEMANTIC;

/// Estimated Jaccard similarity of the abstracted shingles at which two functions are near
/// copies. Renaming and changing constants leaves it at 1; a handful of edited lines in a
/// function of a few dozen lines keeps it above this, while functions that merely share a shape
/// stay well below.
pub(crate) const NEAR_JACCARD_MIN: f64 = 0.8;

/// Estimated Jaccard similarity of the identifiers as written that a near pair must also reach.
/// Abstraction makes tables over different names look alike (a dozen `match` functions turning
/// different enums into strings); the names they use are what tells them apart. A renamed copy
/// keeps its callees, fields and types, so it stays well above this, and unrelated functions sit
/// near zero. Low, because a copy may rename most of its locals.
pub(crate) const IDENT_JACCARD_MIN: f64 = 0.25;

/// Hashes per band. With [`MINHASH_LEN`] hashes this makes 16 bands (see `MINHASH_LEN`).
const BAND_ROWS: usize = 4;

/// Comparisons one bucket may spend. A rejected candidate stays in its bucket as an anchor, so a
/// bucket of many functions that share a band but not a shape would otherwise cost a square. The
/// bucket is read in a fixed order, so the same comparisons are made on every run; those past
/// the budget are skipped and counted in the index summary. Far above what buckets of true
/// copies need (one comparison per cluster seen), so only degenerate buckets reach it.
const BUCKET_COMPARISONS_MAX: usize = 10_000;

/// Meta key holding the comparisons the last run skipped, for the index summary.
pub(crate) const SKIPPED_META: &str = "near_comparisons_skipped";

pub(crate) const EXACT: &str = "exact";
pub(crate) const NEAR: &str = "near";

/// The `WHERE` clause over `symbols s` and `lines l` (the line the symbol starts on) that selects
/// the population: fingerprinted functions outside generated and vendored code.
pub(crate) fn eligible_filter() -> String {
    format!(
        "s.exact_hash IS NOT NULL AND l.role NOT IN ('{}', '{}')",
        Role::Generated.as_str(),
        Role::Vendored.as_str()
    )
}

/// A fingerprinted function.
struct Entry {
    id: String,
    path: String,
    start_line: u32,
    end_line: u32,
    exact: u64,
    minhash: [u32; MINHASH_LEN],
    idents: [u32; IDENT_LEN],
}

impl Entry {
    /// Whether two functions share lines, as a function and one nested in it do.
    fn overlaps(&self, other: &Self) -> bool {
        self.path == other.path
            && self.start_line <= other.end_line
            && other.start_line <= self.end_line
    }
}

fn load(index: &Index) -> Result<Vec<Entry>, QueryError> {
    let mut statement = index.store().connection().prepare(&format!(
        "SELECT s.id, s.path, s.start_line, s.end_line, s.exact_hash, s.minhash, s.idents
         FROM symbols s JOIN lines l ON l.path = s.path AND l.line = s.start_line
         WHERE {}
         ORDER BY s.path, s.start_line, s.end_line DESC",
        eligible_filter()
    ))?;
    let rows = statement.query_map([], |row| {
        let bytes: Vec<u8> = row.get(5)?;
        let mut minhash = [0; MINHASH_LEN];
        for (slot, chunk) in minhash.iter_mut().zip(bytes.as_chunks::<4>().0) {
            *slot = u32::from_le_bytes(*chunk);
        }
        let bytes: Vec<u8> = row.get(6)?;
        let mut idents = [0; IDENT_LEN];
        for (slot, chunk) in idents.iter_mut().zip(bytes.as_chunks::<4>().0) {
            *slot = u32::from_le_bytes(*chunk);
        }
        Ok(Entry {
            id: row.get(0)?,
            path: row.get(1)?,
            start_line: row.get(2)?,
            end_line: row.get(3)?,
            exact: row.get(4)?,
            minhash,
            idents,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Disjoint sets with path halving.
pub(crate) struct Components {
    parent: Vec<usize>,
    /// Members of each set, at its root.
    size: Vec<usize>,
}

impl Components {
    pub(crate) fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            size: vec![1; size],
        }
    }

    pub(crate) fn find(&mut self, mut item: usize) -> usize {
        while self.parent[item] != item {
            self.parent[item] = self.parent[self.parent[item]];
            item = self.parent[item];
        }
        item
    }

    /// Members in the set holding `item`.
    pub(crate) fn size_of(&mut self, item: usize) -> usize {
        let root = self.find(item);
        self.size[root]
    }

    /// Joins two sets; false when they were one already.
    pub(crate) fn join(&mut self, a: usize, b: usize) -> bool {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            let (root, child) = (a.min(b), a.max(b));
            self.parent[child] = root;
            self.size[root] += self.size[child];
        }
        a != b
    }
}

/// A pair of entry positions (lower first) with its kind and score.
type Pair = (usize, usize, &'static str, f64);

/// Exact groups as pairs to each group's first member, and the first members.
fn exact_pairs(entries: &[Entry], components: &mut Components) -> (Vec<Pair>, Vec<usize>) {
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|&i| (entries[i].exact, i));
    let mut pairs = Vec::new();
    let mut firsts = Vec::new();
    for run in order.chunk_by(|&a, &b| entries[a].exact == entries[b].exact) {
        firsts.push(run[0]);
        for &other in &run[1..] {
            components.join(run[0], other);
            pairs.push((run[0], other, EXACT, 1.0));
        }
    }
    firsts.sort_unstable();
    (pairs, firsts)
}

/// Confirms the candidates of one bucket. Each function is scored against one anchor per cluster
/// already seen in the bucket, not against every earlier function, so a bucket of look-alikes
/// costs a pass. Returns the comparisons skipped for want of budget: an estimate, the anchors
/// at the point the budget ran out times the functions left.
fn join_bucket(
    entries: &[Entry],
    bucket: &[(&[u32], usize)],
    components: &mut Components,
    pairs: &mut Vec<Pair>,
    mut budget: usize,
) -> usize {
    let mut anchors: Vec<usize> = Vec::new();
    let mut skipped = 0;
    for (at, &(_, later)) in bucket.iter().enumerate() {
        if budget == 0 {
            // Stop here: counting each remaining pair would itself be the square.
            skipped += anchors.len() * (bucket.len() - at);
            break;
        }
        for (position, &anchor) in anchors.iter().enumerate() {
            if components.find(anchor) == components.find(later)
                || entries[anchor].overlaps(&entries[later])
            {
                continue;
            }
            if budget == 0 {
                skipped += anchors.len() - position;
                break;
            }
            budget -= 1;
            let score = jaccard(&entries[anchor].minhash, &entries[later].minhash);
            if score >= NEAR_JACCARD_MIN
                && jaccard(&entries[anchor].idents, &entries[later].idents) >= IDENT_JACCARD_MIN
            {
                components.join(anchor, later);
                pairs.push((anchor, later, NEAR, score));
            }
        }
        anchors.push(later);
        let mut seen = BTreeSet::new();
        anchors.retain(|&anchor| seen.insert(components.find(anchor)));
    }
    skipped
}

/// Near pairs among the group representatives, found band by band, and the comparisons skipped.
fn near_pairs(
    entries: &[Entry],
    firsts: &[usize],
    components: &mut Components,
) -> (Vec<Pair>, usize) {
    let mut pairs = Vec::new();
    let mut skipped = 0;
    for band in 0..MINHASH_LEN / BAND_ROWS {
        let rows = band * BAND_ROWS..(band + 1) * BAND_ROWS;
        let mut keyed: Vec<(&[u32], usize)> = firsts
            .iter()
            .map(|&i| (&entries[i].minhash[rows.clone()], i))
            .collect();
        keyed.sort_unstable();
        for bucket in keyed.chunk_by(|a, b| a.0 == b.0) {
            skipped += join_bucket(
                entries,
                bucket,
                components,
                &mut pairs,
                BUCKET_COMPARISONS_MAX,
            );
        }
    }
    (pairs, skipped)
}

/// Rewrites `similar` from the fingerprints in `symbols`.
pub(crate) fn run(index: &Index) -> Result<(), QueryError> {
    let entries = load(index)?;
    let mut components = Components::new(entries.len());
    let (mut pairs, firsts) = exact_pairs(&entries, &mut components);
    let (near, skipped) = near_pairs(&entries, &firsts, &mut components);
    pairs.extend(near);
    index.store().set_meta(SKIPPED_META, &skipped.to_string())?;
    let connection = index.store().connection();
    connection.execute_batch("DELETE FROM \"similar\"")?;
    let mut rows = connection.appender("similar")?;
    for (a, b, kind, score) in pairs {
        rows.append_row(params![entries[a].id, entries[b].id, kind, score])?;
    }
    rows.flush()?;
    drop(rows);
    write_duplicated(index)
}

/// Rewrites `duplicated`: the lines of every copy but the longest in each group, so that
/// summing it counts the lines a group could do without once.
pub(crate) fn write_duplicated(index: &Index) -> Result<(), QueryError> {
    let connection = index.store().connection();
    connection.execute_batch("DELETE FROM duplicated")?;
    let mut rows = connection.appender("duplicated")?;
    let mut written = BTreeSet::new();
    for group in groups(index)? {
        let largest = group.largest();
        for (position, member) in group.members.iter().enumerate() {
            if position == largest {
                continue;
            }
            for line in member.start_line..=member.end_line {
                // Nested functions can both be members.
                if written.insert((member.path.clone(), line)) {
                    rows.append_row(params![member.path, line])?;
                }
            }
        }
    }
    rows.flush()?;
    Ok(())
}

/// A function or test in a group of copies.
#[derive(Debug, Clone)]
pub(crate) struct Member {
    pub path: String,
    pub name: String,
    pub start_line: u32,
    pub end_line: u32,
    pub is_test: bool,
    /// The function's scope-qualified key, see `Index::member_keys`.
    pub key: String,
    /// The content hash of the function.
    pub content_hash: String,
}

impl Member {
    pub fn lines(&self) -> u32 {
        self.end_line - self.start_line + 1
    }
}

/// Functions connected by similar pairs: copies of one idea.
#[derive(Debug, Clone)]
pub(crate) struct Group {
    /// In file and line order.
    pub members: Vec<Member>,
    /// Every pair that holds the group together is exact.
    pub exact: bool,
    /// Some pair that holds the group together was confirmed by a model, not by the text.
    pub semantic: bool,
    /// The weakest pair's score.
    pub score_min: f64,
    /// The weakest probability a model gave among the semantic pairs, if there are any: unlike
    /// `score_min` it is never a text similarity.
    pub model_score_min: Option<f64>,
}

impl Group {
    pub fn lines_total(&self) -> u32 {
        self.members.iter().map(Member::lines).sum()
    }

    /// Position of the longest member, the first of equals.
    pub fn largest(&self) -> usize {
        let lines: Vec<u32> = self.members.iter().map(Member::lines).collect();
        lines
            .iter()
            .enumerate()
            .max_by_key(|&(position, &lines)| (lines, std::cmp::Reverse(position)))
            .map_or(0, |(position, _)| position)
    }

    /// Lines beyond the longest member.
    pub fn lines_duplicated(&self) -> u32 {
        self.lines_total() - self.members[self.largest()].lines()
    }

    pub fn file_count(&self) -> usize {
        self.members
            .iter()
            .map(|member| member.path.as_str())
            .collect::<BTreeSet<_>>()
            .len()
    }

    pub fn is_test(&self) -> bool {
        self.members.iter().all(|member| member.is_test)
    }
}

/// Every function in a similar pair, in file order, with its place by symbol id.
fn paired_members(index: &Index) -> Result<(BTreeMap<String, usize>, Vec<Member>), QueryError> {
    let mut statement = index.store().connection().prepare(
        "SELECT s.id, s.path, s.name, s.kind = 'test' OR l.role = 'test', s.start_line, s.end_line,
                s.content_hash
         FROM symbols s JOIN lines l ON l.path = s.path AND l.line = s.start_line
         WHERE s.id IN (SELECT a FROM \"similar\" UNION SELECT b FROM \"similar\")
         ORDER BY s.path, s.start_line, s.end_line DESC",
    )?;
    let mut places = BTreeMap::new();
    let mut members = Vec::new();
    let keys = index.member_keys()?;
    let found = statement.query_map([], |row| {
        let id: String = row.get(0)?;
        let member = Member {
            key: keys.get(&id).cloned().unwrap_or_default(),
            path: row.get(1)?,
            name: row.get(2)?,
            is_test: row.get(3)?,
            start_line: row.get(4)?,
            end_line: row.get(5)?,
            content_hash: row.get(6)?,
        };
        Ok((id, member))
    })?;
    for row in found {
        let (id, member) = row?;
        places.insert(id, members.len());
        members.push(member);
    }
    Ok((places, members))
}

/// The connected components of `similar`, in file order of their first members.
pub(crate) fn groups(index: &Index) -> Result<Vec<Group>, QueryError> {
    let connection = index.store().connection();
    let (places, members) = paired_members(index)?;
    let mut components = Components::new(members.len());
    let mut statement = connection.prepare("SELECT a, b, kind, score FROM \"similar\"")?;
    let pairs = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let place = |id: &str| places[id];
    for (a, b, _, _) in &pairs {
        components.join(place(a), place(b));
    }
    let mut by_root: BTreeMap<usize, Group> = BTreeMap::new();
    for (position, member) in members.into_iter().enumerate() {
        let group = by_root.entry(components.find(position)).or_insert(Group {
            members: Vec::new(),
            exact: true,
            semantic: false,
            score_min: 1.0,
            model_score_min: None,
        });
        group.members.push(member);
    }
    for (a, _, kind, score) in &pairs {
        let group = by_root
            .get_mut(&components.find(place(a)))
            .expect("every paired function is in a group");
        group.exact &= kind == EXACT;
        group.semantic |= kind == SEMANTIC;
        group.score_min = group.score_min.min(*score);
        if kind == SEMANTIC {
            group.model_score_min = Some(group.model_score_min.map_or(*score, |m| m.min(*score)));
        }
    }
    let mut groups: Vec<Group> = by_root.into_values().collect();
    groups.sort_by(|a, b| {
        (&a.members[0].path, a.members[0].start_line)
            .cmp(&(&b.members[0].path, b.members[0].start_line))
    });
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Functions that share a band and nothing else: every one is rejected and stays an anchor.
    fn look_alikes(count: usize) -> Vec<Entry> {
        (0..count)
            .map(|at| {
                let mut minhash = [0_u32; MINHASH_LEN];
                minhash[BAND_ROWS..].fill(u32::try_from(at + 1).unwrap());
                let tag = u32::try_from(at).unwrap();
                Entry {
                    id: at.to_string(),
                    path: format!("{at}.rs"),
                    start_line: 1,
                    end_line: 9,
                    exact: u64::from(tag),
                    minhash,
                    idents: [tag; IDENT_LEN],
                }
            })
            .collect()
    }

    fn run_bucket(entries: &[Entry], budget: usize) -> (usize, usize) {
        let bucket: Vec<(&[u32], usize)> = (0..entries.len())
            .map(|at| (&entries[at].minhash[..BAND_ROWS], at))
            .collect();
        let mut pairs = Vec::new();
        let skipped = join_bucket(
            entries,
            &bucket,
            &mut Components::new(entries.len()),
            &mut pairs,
            budget,
        );
        (pairs.len(), skipped)
    }

    #[test]
    fn a_bucket_of_look_alikes_stops_at_its_budget_and_counts_what_it_skipped() {
        let entries = look_alikes(10);
        // Every function meets every earlier one: 45 comparisons.
        assert_eq!(run_bucket(&entries, usize::MAX), (0, 0));
        assert_eq!(run_bucket(&entries, 45), (0, 0));
        // Out of budget during the seventh function, which has six anchors and made five
        // comparisons; the last three functions would each have met seven anchors.
        assert_eq!(run_bucket(&entries, 20), (0, 1 + 3 * 7));
    }

    #[test]
    fn copies_are_still_found_within_the_budget() {
        let mut entries = look_alikes(4);
        entries[3].minhash = entries[0].minhash;
        entries[3].idents = entries[0].idents;
        assert_eq!(run_bucket(&entries, 10).0, 1);
    }
}

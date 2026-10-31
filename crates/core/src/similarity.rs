//! The similarity analysis: exact and near copies among the fingerprinted functions, written to
//! `similar` as pairs (a, b, kind, score). It reads the whole index and is recomputed with it.
//!
//! Functions with the same exact fingerprint form a group, joined to its first member by exact
//! pairs of score 1. One member of each group then stands for it in the near search: `MinHash`
//! signatures are cut into bands, functions that agree on a whole band are candidates (LSH), and a
//! candidate pair is a near pair when its estimated Jaccard similarity reaches
//! [`NEAR_JACCARD_MIN`]. A pair whose functions are already connected is skipped, so `similar`
//! holds enough pairs to connect every cluster rather than every pair inside it, and a bucket of
//! thousands of look-alikes costs a pass, not a square.
//!
//! The population is functions and tests outside generated and vendored code: nobody can cut
//! those by editing them.

use std::collections::BTreeSet;

use codemap_lang::Role;
use duckdb::params;

use crate::fingerprint::{MINHASH_LEN, jaccard};
use crate::reader::{Index, QueryError};

/// Estimated Jaccard similarity of the abstracted shingles at which two functions are near
/// copies. Renaming and changing constants leaves it at 1; a handful of edited lines in a
/// function of a few dozen lines keeps it above this, while functions that merely share a shape
/// stay well below.
pub(crate) const NEAR_JACCARD_MIN: f64 = 0.8;

/// Hashes per band. With [`MINHASH_LEN`] hashes this makes 16 bands (see `MINHASH_LEN`).
const BAND_ROWS: usize = 4;

pub(crate) const EXACT: &str = "exact";
pub(crate) const NEAR: &str = "near";

/// A fingerprinted function.
struct Entry {
    id: String,
    path: String,
    start_line: u32,
    end_line: u32,
    exact: u64,
    minhash: [u32; MINHASH_LEN],
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
        "SELECT s.id, s.path, s.start_line, s.end_line, s.exact_hash, s.minhash
         FROM symbols s JOIN lines l ON l.path = s.path AND l.line = s.start_line
         WHERE s.exact_hash IS NOT NULL AND l.role NOT IN ('{}', '{}')
         ORDER BY s.path, s.start_line, s.end_line DESC",
        Role::Generated.as_str(),
        Role::Vendored.as_str()
    ))?;
    let rows = statement.query_map([], |row| {
        let bytes: Vec<u8> = row.get(5)?;
        let mut minhash = [0; MINHASH_LEN];
        for (slot, chunk) in minhash.iter_mut().zip(bytes.as_chunks::<4>().0) {
            *slot = u32::from_le_bytes(*chunk);
        }
        Ok(Entry {
            id: row.get(0)?,
            path: row.get(1)?,
            start_line: row.get(2)?,
            end_line: row.get(3)?,
            exact: row.get(4)?,
            minhash,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Disjoint sets with path halving.
struct Components(Vec<usize>);

impl Components {
    fn new(size: usize) -> Self {
        Self((0..size).collect())
    }

    fn find(&mut self, mut item: usize) -> usize {
        while self.0[item] != item {
            self.0[item] = self.0[self.0[item]];
            item = self.0[item];
        }
        item
    }

    /// Joins two sets; false when they were one already.
    fn join(&mut self, a: usize, b: usize) -> bool {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
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
/// costs a pass.
fn join_bucket(
    entries: &[Entry],
    bucket: &[(&[u32], usize)],
    components: &mut Components,
    pairs: &mut Vec<Pair>,
) {
    let mut anchors: Vec<usize> = Vec::new();
    for &(_, later) in bucket {
        for &anchor in &anchors {
            if components.find(anchor) == components.find(later)
                || entries[anchor].overlaps(&entries[later])
            {
                continue;
            }
            let score = jaccard(&entries[anchor].minhash, &entries[later].minhash);
            if score >= NEAR_JACCARD_MIN {
                components.join(anchor, later);
                pairs.push((anchor, later, NEAR, score));
            }
        }
        anchors.push(later);
        let mut seen = BTreeSet::new();
        anchors.retain(|&anchor| seen.insert(components.find(anchor)));
    }
}

/// Near pairs among the group representatives, found band by band.
fn near_pairs(entries: &[Entry], firsts: &[usize], components: &mut Components) -> Vec<Pair> {
    let mut pairs = Vec::new();
    for band in 0..MINHASH_LEN / BAND_ROWS {
        let rows = band * BAND_ROWS..(band + 1) * BAND_ROWS;
        let mut keyed: Vec<(&[u32], usize)> = firsts
            .iter()
            .map(|&i| (&entries[i].minhash[rows.clone()], i))
            .collect();
        keyed.sort_unstable();
        for bucket in keyed.chunk_by(|a, b| a.0 == b.0) {
            join_bucket(entries, bucket, components, &mut pairs);
        }
    }
    pairs
}

/// Rewrites `similar` from the fingerprints in `symbols`.
pub(crate) fn run(index: &Index) -> Result<(), QueryError> {
    let entries = load(index)?;
    let mut components = Components::new(entries.len());
    let (mut pairs, firsts) = exact_pairs(&entries, &mut components);
    pairs.extend(near_pairs(&entries, &firsts, &mut components));
    let connection = index.store().connection();
    connection.execute_batch("DELETE FROM \"similar\"")?;
    let mut rows = connection.appender("similar")?;
    for (a, b, kind, score) in pairs {
        rows.append_row(params![entries[a].id, entries[b].id, kind, score])?;
    }
    rows.flush()?;
    Ok(())
}

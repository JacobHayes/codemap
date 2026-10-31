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
//! thousands of look-alikes costs a pass, not a square.
//!
//! The population is functions and tests outside generated and vendored code: nobody can cut
//! those by editing them.

use std::collections::{BTreeMap, BTreeSet};

use codemap_lang::Role;
use duckdb::params;

use crate::fingerprint::{IDENT_LEN, MINHASH_LEN, jaccard};
use crate::reader::{Index, QueryError};

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
    drop(rows);
    write_duplicated(index)
}

/// Rewrites `duplicated`: the lines of every copy but the longest in each group, so that
/// summing it counts the lines a group could do without once.
fn write_duplicated(index: &Index) -> Result<(), QueryError> {
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
    /// The weakest pair's score.
    pub score_min: f64,
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

/// The connected components of `similar`, in file order of their first members.
pub(crate) fn groups(index: &Index) -> Result<Vec<Group>, QueryError> {
    let connection = index.store().connection();
    let mut statement = connection.prepare(
        "SELECT s.id, s.path, s.name, s.kind = 'test' OR l.role = 'test', s.start_line, s.end_line
         FROM symbols s JOIN lines l ON l.path = s.path AND l.line = s.start_line
         WHERE s.id IN (SELECT a FROM \"similar\" UNION SELECT b FROM \"similar\")
         ORDER BY s.path, s.start_line, s.end_line DESC",
    )?;
    let mut places = BTreeMap::new();
    let mut members = Vec::new();
    let found = statement.query_map([], |row| {
        let member = Member {
            path: row.get(1)?,
            name: row.get(2)?,
            is_test: row.get(3)?,
            start_line: row.get(4)?,
            end_line: row.get(5)?,
        };
        Ok((row.get::<_, String>(0)?, member))
    })?;
    for row in found {
        let (id, member) = row?;
        places.insert(id, members.len());
        members.push(member);
    }
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
            score_min: 1.0,
        });
        group.members.push(member);
    }
    for (a, _, kind, score) in &pairs {
        let group = by_root
            .get_mut(&components.find(place(a)))
            .expect("every paired function is in a group");
        group.exact &= kind == EXACT;
        group.score_min = group.score_min.min(*score);
    }
    let mut groups: Vec<Group> = by_root.into_values().collect();
    groups.sort_by(|a, b| {
        (&a.members[0].path, a.members[0].start_line)
            .cmp(&(&b.members[0].path, b.members[0].start_line))
    });
    Ok(groups)
}

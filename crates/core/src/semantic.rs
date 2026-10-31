//! Semantic duplication: functions that do the same thing in different words, found across
//! files and languages. Only runs when `[inference]` is configured.
//!
//! Every eligible function (the population of [`crate::similarity`]) is embedded from its
//! normalized source; the nearest neighbours of each by cosine are candidates; a decision model
//! confirms each candidate not already connected to its partner. Vectors are cached by content
//! hash and model, answers by the pair of hashes, model and prompt version, so an unchanged
//! codebase makes no call.
//!
//! The run happens after the deterministic analyses have been committed, outside any transaction:
//! the network is slow and the findings that need no model must not wait for it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use duckdb::params;
use rayon::prelude::*;

use crate::config::{Config, Inference};
use crate::inference::{
    ChatDecisionModel, Client, DecisionModel, InferenceError, SAME_BEHAVIOUR_PROMPT_VERSION,
    SameBehaviour, unit,
};
use crate::reader::{Index, QueryError};
use crate::similarity::{Components, eligible_filter};

pub(crate) const SEMANTIC: &str = "semantic";

/// Cosine similarity at which a pair is worth asking a model about. Code in one codebase shares
/// vocabulary, so the vectors are crowded: in a 466-function repository every pair the model
/// confirmed scored above 0.7, and the 719 it was asked about below that were all rejected.
pub(crate) const SIMILARITY_MIN: f32 = 0.7;

/// Candidates kept per function. A function has a handful of real counterparts at most, and the
/// strongest come first; more only spends calls on look-alikes.
pub(crate) const NEIGHBOURS_MAX: usize = 3;

/// Probability of "same thing" at which a candidate becomes a semantic pair. Models are
/// overconfident either way, so this is deliberately high: a wrong pair merges two clusters.
pub(crate) const PROBABILITY_MIN: f64 = 0.8;

/// Functions in a group beyond which a semantic pair no longer joins two clusters. Pairs chain:
/// every link can be a fair "same thing" while the ends are different things, and on real
/// repositories the unbounded groups were dozens of look-alike probes and wrappers, not copies of
/// one idea. Past this size the group is already a family to look at as a whole.
pub(crate) const GROUP_SIZE_MAX: usize = 12;

/// Model calls per run. A first run on a large codebase is bounded in cost and time; the rest are
/// asked on later runs, strongest candidates first, because answers are cached.
pub(crate) const CALLS_MAX: usize = 2000;

/// Characters of a function's normalized source that are embedded. Embedding models take far
/// more, but the opening of a function says what it does and a long tail only dilutes it.
const EMBED_CHARS_MAX: usize = 6000;

/// Functions per embeddings request: large enough to amortise the round trip, small enough that
/// a request stays well under the provider's size limits.
const EMBED_BATCH: usize = 32;

/// Rows of the neighbour search handled together: the block of vectors (128 KiB at 1024
/// dimensions) stays in cache while every other vector streams past it.
const ROW_BLOCK: usize = 32;

/// Concurrent requests. Embedding batches are large and few; decisions are small and many, and
/// 16 at once kept a thousand-call run under a minute without a rate limit. Retries absorb the
/// occasional 429.
const EMBED_THREADS: usize = 8;
const DECIDE_THREADS: usize = 16;

/// Why semantic duplication did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticOff {
    /// The config has no `[inference]` table.
    NotConfigured,
    /// The environment variable that should hold the API key is unset or empty.
    MissingKey { env: String },
}

/// What a run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SemanticRun {
    /// Eligible functions.
    pub function_count: u64,
    /// Functions embedded by a call this run; the rest came from the cache.
    pub embedded_count: u64,
    /// Candidate pairs worth a decision: near neighbours not already connected.
    pub candidate_count: u64,
    /// Candidates a model was asked about this run; the rest were answered from the cache.
    pub asked_count: u64,
    /// Candidates left for a later run because the call budget was spent.
    pub skipped_count: u64,
    /// Functions and candidates left for a later run: no usable answer, or source that changed
    /// since it was indexed.
    pub failed_count: u64,
    /// Pairs added to `similar`.
    pub confirmed_count: u64,
    /// Spend reported by the provider, in millionths of a dollar; `None` when it did not say.
    pub cost_micro_usd: Option<u64>,
}

/// What the semantic analysis did in one refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticStatus {
    Off(SemanticOff),
    /// Nothing it reads has changed since its last complete run.
    Unchanged,
    Ran(SemanticRun),
    /// The model API could not be used; the deterministic results stand.
    Failed(String),
    /// Configured, and not run yet.
    Pending,
    /// A newer change arrived first; the run will be made again.
    Superseded,
}

impl SemanticStatus {
    /// The status in plain words, as `codemap index` prints it and the overview carries it.
    #[must_use]
    pub fn summary(&self) -> String {
        let head = "semantic duplication:";
        match self {
            Self::Off(SemanticOff::NotConfigured) => {
                format!("{head} off, no [inference] in .codemap/config.toml")
            }
            Self::Off(SemanticOff::MissingKey { env }) => format!("{head} off, ${env} is not set"),
            Self::Unchanged => format!("{head} unchanged"),
            Self::Pending => format!("{head} waiting to run"),
            Self::Superseded => format!("{head} interrupted by a newer change"),
            Self::Failed(why) => format!("{head} failed, {why}"),
            Self::Ran(run) => {
                let cost = run
                    .cost_micro_usd
                    .map_or("cost not reported".to_owned(), |micro| {
                        format!("${}.{:06}", micro / 1_000_000, micro % 1_000_000)
                    });
                let mut parts = vec![format!(
                    "{} functions ({} embedded now), {} candidates ({} asked), {} confirmed, {cost}",
                    run.function_count,
                    run.embedded_count,
                    run.candidate_count,
                    run.asked_count,
                    run.confirmed_count,
                )];
                if run.skipped_count > 0 {
                    parts.push(format!(
                        "{} candidates left for the next run (call budget spent)",
                        run.skipped_count
                    ));
                }
                if run.failed_count > 0 {
                    parts.push(format!("{} could not be processed", run.failed_count));
                }
                format!("{head} {}", parts.join("; "))
            }
        }
    }
}

/// A configured analysis: where to call and with what key.
pub(crate) struct Setup<'a> {
    inference: &'a Inference,
    key: String,
}

/// The configured analysis, or why there is none.
pub(crate) fn setup(config: &Config) -> Result<Setup<'_>, SemanticOff> {
    let inference = config
        .inference
        .as_ref()
        .ok_or(SemanticOff::NotConfigured)?;
    match std::env::var(&inference.api_key_env) {
        Ok(key) if !key.trim().is_empty() => Ok(Setup {
            inference,
            key: key.trim().to_owned(),
        }),
        _ => Err(SemanticOff::MissingKey {
            env: inference.api_key_env.clone(),
        }),
    }
}

/// The result of a run: pairs to add to `similar` as (a, b, probability).
pub(crate) struct Outcome {
    pub status: SemanticStatus,
    pub pairs: Vec<(String, String, f64)>,
    /// Every candidate was answered; only then is the run not repeated until something changes.
    pub complete: bool,
}

enum Stop {
    /// A newer refresh made this run's input stale.
    Superseded,
    Query(QueryError),
    Model(InferenceError),
}

impl From<QueryError> for Stop {
    fn from(error: QueryError) -> Self {
        Self::Query(error)
    }
}

impl From<duckdb::Error> for Stop {
    fn from(error: duckdb::Error) -> Self {
        Self::Query(error.into())
    }
}

impl From<InferenceError> for Stop {
    fn from(error: InferenceError) -> Self {
        Self::Model(error)
    }
}

/// An eligible function.
struct Function {
    id: String,
    path: String,
    start_line: u32,
    end_line: u32,
    hash: String,
}

impl Function {
    fn overlaps(&self, other: &Self) -> bool {
        self.path == other.path
            && self.start_line <= other.end_line
            && other.start_line <= self.end_line
    }
}

/// Runs the analysis against the index as committed.
///
/// # Errors
/// Fails only when the index cannot be read or written; a model API that cannot be used is a
/// [`SemanticStatus::Failed`] outcome, so that the deterministic results stand.
pub(crate) fn run(
    index: &Index,
    setup: &Setup<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Outcome, QueryError> {
    let client = Client::new(&setup.inference.base_url, setup.key.clone());
    let decider = ChatDecisionModel {
        client: &client,
        model: &setup.inference.decision_model,
    };
    let mut report = SemanticRun::default();
    let result = analyse(index, setup, &client, &decider, cancelled, &mut report);
    report.cost_micro_usd = client.cost_micro_usd();
    match result {
        Ok(pairs) => Ok(Outcome {
            complete: report.skipped_count == 0 && report.failed_count == 0,
            status: SemanticStatus::Ran(report),
            pairs,
        }),
        Err(Stop::Query(error)) => Err(error),
        Err(Stop::Superseded) => Ok(Outcome {
            status: SemanticStatus::Superseded,
            pairs: Vec::new(),
            complete: false,
        }),
        Err(Stop::Model(error)) => Ok(Outcome {
            status: SemanticStatus::Failed(error.to_string()),
            pairs: Vec::new(),
            complete: false,
        }),
    }
}

fn analyse(
    index: &Index,
    setup: &Setup<'_>,
    client: &Client,
    decider: &dyn DecisionModel,
    cancelled: &dyn Fn() -> bool,
    report: &mut SemanticRun,
) -> Result<Vec<(String, String, f64)>, Stop> {
    let functions = load(index)?;
    report.function_count = functions.len() as u64;
    let mut sources = Sources::new(
        index.root(),
        index.store().file_hashes().map_err(QueryError::from)?,
    );
    let vectors = embeddings(
        index,
        client,
        &setup.inference.embedding_model,
        &functions,
        &mut sources,
        cancelled,
        report,
    )?;
    let mut components = connected(index, &functions)?;
    let held: Vec<(usize, &[f32])> = functions
        .iter()
        .enumerate()
        .filter_map(|(position, function)| {
            vectors
                .get(&function.hash)
                .map(|vector| (position, vector.as_slice()))
        })
        .collect();
    let candidates = candidates(&functions, &held, &mut components);
    report.candidate_count = candidates.len() as u64;
    let answers = confirm(
        index,
        decider,
        &functions,
        &candidates,
        &mut sources,
        cancelled,
        report,
    )?;
    // Candidates first, strongest by similarity; then every confirmation the cache still holds
    // for functions that are here, so that a pair pushed out of its members' nearest neighbours
    // by newer code stays a finding.
    let mut ordered: Vec<(usize, usize)> = candidates.clone();
    ordered.extend(known_pairs(&functions, &answers));
    let mut pairs = Vec::new();
    for (a, b) in ordered {
        let key = answer_key(&functions[a].hash, &functions[b].hash);
        if let Some(&probability) = answers.get(&key)
            && probability >= PROBABILITY_MIN
            && join_within(&mut components, a, b, GROUP_SIZE_MAX)
        {
            pairs.push((
                functions[a].id.clone(),
                functions[b].id.clone(),
                probability,
            ));
        }
    }
    report.confirmed_count = pairs.len() as u64;
    Ok(pairs)
}

fn load(index: &Index) -> Result<Vec<Function>, QueryError> {
    let mut statement = index.store().connection().prepare(&format!(
        "SELECT s.id, s.path, s.start_line, s.end_line, s.content_hash
         FROM symbols s JOIN lines l ON l.path = s.path AND l.line = s.start_line
         WHERE {}
         ORDER BY s.path, s.start_line, s.end_line DESC",
        eligible_filter()
    ))?;
    let rows = statement.query_map([], |row| {
        Ok(Function {
            id: row.get(0)?,
            path: row.get(1)?,
            start_line: row.get(2)?,
            end_line: row.get(3)?,
            hash: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The exact and near groups already in `similar`, over positions in `functions`.
fn connected(index: &Index, functions: &[Function]) -> Result<Components, QueryError> {
    let places: BTreeMap<&str, usize> = functions
        .iter()
        .enumerate()
        .map(|(position, function)| (function.id.as_str(), position))
        .collect();
    let mut components = Components::new(functions.len());
    let mut statement = index
        .store()
        .connection()
        .prepare("SELECT a, b FROM \"similar\"")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (a, b) = row?;
        if let (Some(&a), Some(&b)) = (places.get(a.as_str()), places.get(b.as_str())) {
            components.join(a, b);
        }
    }
    Ok(components)
}

/// Function sources, read once per file and cut to the lines of a function. A file whose bytes
/// are no longer the ones that were indexed (edited since, or gone) has no source: its symbols'
/// lines would cut the wrong text, and what a model said about that text must not be cached under
/// the old content hash.
struct Sources<'a> {
    root: &'a std::path::Path,
    indexed: BTreeMap<String, String>,
    files: BTreeMap<String, Option<Vec<String>>>,
}

impl<'a> Sources<'a> {
    fn new(root: &'a std::path::Path, indexed: BTreeMap<String, String>) -> Self {
        Self {
            root,
            indexed,
            files: BTreeMap::new(),
        }
    }

    fn read(&self, path: &str) -> Option<Vec<String>> {
        let bytes = fs::read(self.root.join(path)).ok()?;
        let hash = blake3::hash(&bytes).to_hex().to_string();
        (self.indexed.get(path) == Some(&hash)).then(|| {
            String::from_utf8_lossy(&bytes)
                .lines()
                .map(|line| line.trim_end().to_owned())
                .collect()
        })
    }

    /// The function's lines without blank lines, trailing whitespace and the indentation they
    /// all share, cut to [`EMBED_CHARS_MAX`]: how a function is laid out says nothing about what
    /// it does, but how deep a statement sits does. `None` when the file is not as indexed.
    fn text(&mut self, function: &Function) -> Option<String> {
        if !self.files.contains_key(&function.path) {
            let lines = self.read(&function.path);
            self.files.insert(function.path.clone(), lines);
        }
        let lines = self.files[&function.path].as_ref()?;
        let first = function.start_line.saturating_sub(1) as usize;
        let last = (function.end_line as usize).min(lines.len());
        let body: Vec<&String> = lines
            .get(first..last)
            .unwrap_or_default()
            .iter()
            .filter(|line| !line.trim().is_empty())
            .collect();
        let margin = body
            .iter()
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);
        let mut text = String::new();
        for line in body {
            text.push_str(line.get(margin..).unwrap_or(line));
            text.push('\n');
        }
        Some(text.chars().take(EMBED_CHARS_MAX).collect())
    }
}

/// Runs `work` on `items` over `threads` threads and hands each result to `sink` on the calling
/// thread, so only that thread touches the database. Stops handing out items once `sink` breaks.
fn fan_out<T: Sync, R: Send>(
    items: &[T],
    threads: usize,
    work: impl Fn(&T) -> R + Sync,
    mut sink: impl FnMut(usize, R) -> ControlFlow<()>,
) {
    let next = AtomicUsize::new(0);
    let stopped = AtomicBool::new(false);
    thread::scope(|scope| {
        let (sender, receiver) = mpsc::channel();
        for _ in 0..threads.min(items.len()) {
            let sender = sender.clone();
            let (next, stopped, work) = (&next, &stopped, &work);
            scope.spawn(move || {
                while !stopped.load(Ordering::Relaxed) {
                    let position = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(position) else {
                        break;
                    };
                    if sender.send((position, work(item))).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        for (position, result) in receiver {
            if sink(position, result).is_break() {
                stopped.store(true, Ordering::Relaxed);
            }
        }
    });
}

fn vector_bytes(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// The cached vector of every content hash embedded by `model`.
fn cached_vectors(
    connection: &duckdb::Connection,
    model: &str,
) -> Result<BTreeMap<String, Vec<f32>>, Stop> {
    let mut vectors = BTreeMap::new();
    let mut statement =
        connection.prepare("SELECT content_hash, vector FROM embeddings WHERE model = ?")?;
    for row in statement.query_map(params![model], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
    })? {
        let (hash, bytes) = row?;
        let vector = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| f32::from_le_bytes(*chunk))
            .collect();
        vectors.insert(hash, unit(vector));
    }
    Ok(vectors)
}

/// A vector for every function's content hash: from the cache, or embedded now and cached.
/// Hashes that could not be embedded are absent.
fn embeddings(
    index: &Index,
    client: &Client,
    model: &str,
    functions: &[Function],
    sources: &mut Sources<'_>,
    cancelled: &dyn Fn() -> bool,
    report: &mut SemanticRun,
) -> Result<BTreeMap<String, Vec<f32>>, Stop> {
    let connection = index.store().connection();
    // Vectors of code that is gone are not worth keeping.
    connection.execute_batch(
        "DELETE FROM embeddings WHERE content_hash NOT IN (SELECT content_hash FROM symbols)",
    )?;
    let mut vectors = cached_vectors(connection, model)?;
    let mut missing: Vec<(String, String)> = Vec::new();
    let mut seen = BTreeSet::new();
    for function in functions {
        if vectors.contains_key(&function.hash) || !seen.insert(function.hash.as_str()) {
            continue;
        }
        match sources.text(function) {
            Some(text) => missing.push((function.hash.clone(), text)),
            // Changed since indexed: the next refresh will see the new text.
            None => report.failed_count += 1,
        }
    }
    let batches: Vec<&[(String, String)]> = missing.chunks(EMBED_BATCH).collect();
    let mut failure = None;
    fan_out(
        &batches,
        EMBED_THREADS,
        |batch| {
            let texts: Vec<String> = batch.iter().map(|(_, text)| text.clone()).collect();
            client.embed(model, &texts)
        },
        |position, result| {
            if cancelled() {
                failure = Some(Stop::Superseded);
                return ControlFlow::Break(());
            }
            match result {
                Ok(embedded) => {
                    for ((hash, _), vector) in batches[position].iter().zip(embedded) {
                        let stored = connection.execute(
                            "INSERT INTO embeddings VALUES (?, ?, ?)",
                            params![hash, model, vector_bytes(&vector)],
                        );
                        if let Err(error) = stored {
                            failure = Some(Stop::from(error));
                            return ControlFlow::Break(());
                        }
                        report.embedded_count += 1;
                        vectors.insert(hash.clone(), vector);
                    }
                    ControlFlow::Continue(())
                }
                Err(error) => {
                    failure = Some(error.into());
                    ControlFlow::Break(())
                }
            }
        },
    );
    match failure {
        Some(stop) => Err(stop),
        None => Ok(vectors),
    }
}

/// The dot product, in eight independent sums so that the compiler can use vector instructions
/// (a single running sum must be added in order, which it may not reorder).
fn dot(a: &[f32], b: &[f32]) -> f32 {
    const LANES: usize = 8;
    let mut sums = [0.0_f32; LANES];
    let ((a_chunks, a_tail), (b_chunks, b_tail)) = (a.as_chunks::<LANES>(), b.as_chunks::<LANES>());
    let tail: f32 = a_tail.iter().zip(b_tail).map(|(x, y)| x * y).sum();
    for (x, y) in a_chunks.iter().zip(b_chunks) {
        for lane in 0..LANES {
            sums[lane] += x[lane] * y[lane];
        }
    }
    sums.iter().sum::<f32>() + tail
}

/// For each of `held` (position, vector), its closest others by cosine similarity: at most
/// `per_item`, each at least `similarity_min`. Pairs are (lower position, higher position,
/// similarity), strongest first and, among equals, in position order, so the result does not
/// depend on how the search is scheduled. A neighbour must be `wanted` (by position) to count:
/// pairs that cannot be candidates are left out before the cap, or they would crowd out the ones
/// that can. Of the neighbours in one `cluster` only the closest counts, for the same reason.
/// Vectors must be unit length of one dimension; the
/// search is exact, a dot product against every vector in cache-sized blocks: 20,000 functions
/// take under two seconds, and the cost grows with the square, so 100,000 take under a minute.
pub(crate) fn neighbours(
    held: &[(usize, &[f32])],
    per_item: usize,
    similarity_min: f32,
    wanted: impl Fn(usize, usize) -> bool + Sync,
    cluster: impl Fn(usize) -> usize + Sync,
) -> Vec<(usize, usize, f32)> {
    let rows: Vec<usize> = (0..held.len()).collect();
    let per_row: Vec<Vec<(f32, usize)>> = rows
        .par_chunks(ROW_BLOCK)
        .flat_map_iter(|block| {
            let mut bests: Vec<Vec<(f32, usize)>> =
                vec![Vec::with_capacity(per_item + 1); block.len()];
            // Each vector in turn is compared with the whole block, so it is read from memory
            // once per block and not once per row.
            for (other, (other_place, candidate)) in held.iter().enumerate() {
                for (&row, best) in block.iter().zip(&mut bests) {
                    if other == row || !wanted(held[row].0, *other_place) {
                        continue;
                    }
                    let similarity = dot(held[row].1, candidate);
                    if similarity < similarity_min {
                        continue;
                    }
                    let better = |kept: f32, kept_at: usize| {
                        (kept, std::cmp::Reverse(kept_at)) > (similarity, std::cmp::Reverse(other))
                    };
                    // One neighbour per cluster: copies of one function would otherwise fill
                    // every slot and hide the other functions near it.
                    let same = cluster(*other_place);
                    if let Some(kept) = best.iter().position(|&(_, at)| cluster(held[at].0) == same)
                    {
                        if better(best[kept].0, best[kept].1) {
                            continue;
                        }
                        best.remove(kept);
                    }
                    let at = best.partition_point(|&(kept, kept_at)| better(kept, kept_at));
                    if at < per_item {
                        best.insert(at, (similarity, other));
                        best.truncate(per_item);
                    }
                }
            }
            bests
        })
        .collect();
    let mut seen = BTreeSet::new();
    let mut pairs = Vec::new();
    for (row, best) in per_row.iter().enumerate() {
        for &(similarity, other) in best {
            let (a, b) = (held[row.min(other)].0, held[row.max(other)].0);
            if seen.insert((a, b)) {
                pairs.push((a, b, similarity));
            }
        }
    }
    pairs.sort_by(|x, y| y.2.total_cmp(&x.2).then((x.0, x.1).cmp(&(y.0, y.1))));
    pairs
}

/// Candidate pairs of function positions, strongest first: neighbours that are not already
/// connected, not nested in each other, and the strongest one between any two clusters (asking
/// again for another pair of the same two clusters is a call spent on the same question).
fn candidates(
    functions: &[Function],
    held: &[(usize, &[f32])],
    components: &mut Components,
) -> Vec<(usize, usize)> {
    let roots: Vec<usize> = (0..functions.len()).map(|i| components.find(i)).collect();
    let wanted = |a: usize, b: usize| {
        roots[a] != roots[b]
            && functions[a].hash != functions[b].hash
            && !functions[a].overlaps(&functions[b])
    };
    let mut clusters = BTreeSet::new();
    let mut kept = Vec::new();
    for (a, b, _) in neighbours(held, NEIGHBOURS_MAX, SIMILARITY_MIN, wanted, |a| roots[a]) {
        if clusters.insert((roots[a].min(roots[b]), roots[a].max(roots[b]))) {
            kept.push((a, b));
        }
    }
    kept
}

/// Pairs of functions (by position, the first function of each content hash) that the cache
/// holds a confirmation for, most probable first.
fn known_pairs(
    functions: &[Function],
    answers: &BTreeMap<(String, String), f64>,
) -> Vec<(usize, usize)> {
    let mut first_of: BTreeMap<&str, usize> = BTreeMap::new();
    for (position, function) in functions.iter().enumerate() {
        first_of.entry(function.hash.as_str()).or_insert(position);
    }
    let mut known: Vec<(f64, usize, usize)> = answers
        .iter()
        .filter(|&(_, &probability)| probability >= PROBABILITY_MIN)
        .filter_map(|((hash_a, hash_b), &probability)| {
            let (a, b) = (
                *first_of.get(hash_a.as_str())?,
                *first_of.get(hash_b.as_str())?,
            );
            (!functions[a].overlaps(&functions[b])).then_some((probability, a.min(b), a.max(b)))
        })
        .collect();
    known.sort_by(|x, y| y.0.total_cmp(&x.0).then((x.1, x.2).cmp(&(y.1, y.2))));
    known.into_iter().map(|(_, a, b)| (a, b)).collect()
}

/// Joins the clusters of `a` and `b` unless they are one already or would pass `size_max`.
fn join_within(components: &mut Components, a: usize, b: usize, size_max: usize) -> bool {
    components.size_of(a) + components.size_of(b) <= size_max && components.join(a, b)
}

/// The cache key of a pair's answer: the hashes in order.
fn answer_key(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_owned(), b.to_owned())
    } else {
        (b.to_owned(), a.to_owned())
    }
}

/// The first `calls_max` of `pending`, and how many were left out.
pub(crate) fn within_budget<T>(mut pending: Vec<T>, calls_max: usize) -> (Vec<T>, usize) {
    let skipped = pending.len().saturating_sub(calls_max);
    pending.truncate(calls_max);
    (pending, skipped)
}

/// Probability for every candidate that has an answer: cached, or asked now within the budget.
fn confirm(
    index: &Index,
    decider: &dyn DecisionModel,
    functions: &[Function],
    candidates: &[(usize, usize)],
    sources: &mut Sources<'_>,
    cancelled: &dyn Fn() -> bool,
    report: &mut SemanticRun,
) -> Result<BTreeMap<(String, String), f64>, Stop> {
    let connection = index.store().connection();
    let (model, prompt) = (decider.name(), SAME_BEHAVIOUR_PROMPT_VERSION);
    let mut answers: BTreeMap<(String, String), f64> = BTreeMap::new();
    let mut statement = connection.prepare(
        "SELECT hash_a, hash_b, probability FROM answers WHERE model = ? AND prompt = ?",
    )?;
    for row in statement.query_map(params![model, prompt], |row| {
        Ok(((row.get(0)?, row.get(1)?), row.get(2)?))
    })? {
        let (key, probability) = row?;
        answers.insert(key, probability);
    }
    let pending: Vec<(usize, usize)> = candidates
        .iter()
        .copied()
        .filter(|&(a, b)| {
            !answers.contains_key(&answer_key(&functions[a].hash, &functions[b].hash))
        })
        .collect();
    let (pending, skipped) = within_budget(pending, CALLS_MAX);
    report.skipped_count = skipped as u64;
    let mut questions: Vec<(String, String, String, String)> = Vec::new();
    for &(a, b) in &pending {
        let key = answer_key(&functions[a].hash, &functions[b].hash);
        match (sources.text(&functions[a]), sources.text(&functions[b])) {
            (Some(first), Some(second)) => questions.push((key.0, key.1, first, second)),
            // Changed since indexed: the next refresh will see the new text.
            _ => report.failed_count += 1,
        }
    }
    let mut failure = None;
    fan_out(
        &questions,
        DECIDE_THREADS,
        |(_, _, first, second)| decider.decide(&SameBehaviour { first, second }),
        |position, result| {
            if cancelled() {
                failure = Some(Stop::Superseded);
                return ControlFlow::Break(());
            }
            let (hash_a, hash_b, ..) = &questions[position];
            match result {
                Ok(probability) => {
                    let stored = connection.execute(
                        "INSERT INTO answers VALUES (?, ?, ?, ?, ?)",
                        params![hash_a, hash_b, model, prompt, probability],
                    );
                    if let Err(error) = stored {
                        failure = Some(Stop::from(error));
                        return ControlFlow::Break(());
                    }
                    report.asked_count += 1;
                    answers.insert((hash_a.clone(), hash_b.clone()), probability);
                    ControlFlow::Continue(())
                }
                Err(InferenceError::Undecided(_)) => {
                    report.asked_count += 1;
                    report.failed_count += 1;
                    ControlFlow::Continue(())
                }
                Err(error) => {
                    failure = Some(error.into());
                    ControlFlow::Break(())
                }
            }
        },
    );
    match failure {
        Some(stop) => Err(stop),
        None => Ok(answers),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(vectors: &[Vec<f32>]) -> Vec<(usize, &[f32])> {
        vectors
            .iter()
            .enumerate()
            .map(|(i, v)| (i, v.as_slice()))
            .collect()
    }

    fn at_angle(degrees: f32) -> Vec<f32> {
        let radians = degrees.to_radians();
        vec![radians.cos(), radians.sin()]
    }

    #[test]
    fn neighbours_are_the_closest_above_the_floor_strongest_first() {
        let vectors = [
            at_angle(0.0),
            at_angle(10.0),
            at_angle(20.0),
            at_angle(90.0),
        ];
        let pairs = neighbours(&held(&vectors), 1, 0.9, |_, _| true, |place| place);
        let positions: Vec<(usize, usize)> = pairs.iter().map(|&(a, b, _)| (a, b)).collect();
        // 0-1 is the closest pair; 2's closest is 1; 3 has nothing above the floor.
        assert_eq!(positions, vec![(0, 1), (1, 2)]);
        assert!(pairs[0].2 >= pairs[1].2);
    }

    #[test]
    fn neighbours_are_symmetric_and_listed_once() {
        let vectors = [at_angle(0.0), at_angle(5.0)];
        assert_eq!(
            neighbours(&held(&vectors), 5, 0.5, |_, _| true, |place| place).len(),
            1
        );
    }

    #[test]
    fn per_item_cap_applies_to_each_function() {
        let vectors = [at_angle(0.0), at_angle(1.0), at_angle(2.0), at_angle(3.0)];
        let pairs = neighbours(&held(&vectors), 1, 0.5, |_, _| true, |place| place);
        // Each function keeps one neighbour, so at most four pairs and fewer than all six.
        assert!(pairs.len() <= 4);
    }

    #[test]
    fn unwanted_neighbours_do_not_use_up_the_cap() {
        let vectors = [at_angle(0.0), at_angle(1.0), at_angle(2.0), at_angle(30.0)];
        // 0's two closest are 1 and 2, which are not wanted; 3 must still be found.
        let pairs = neighbours(
            &held(&vectors),
            1,
            0.5,
            |a, b| a == 3 || b == 3 || (a, b) == (1, 2),
            |place| place,
        );
        assert!(pairs.iter().any(|&(a, b, _)| (a, b) == (0, 3)), "{pairs:?}");
    }

    #[test]
    fn source_keeps_nesting_but_not_margin_blank_lines_or_edited_files() {
        let dir = tempfile::tempdir().unwrap();
        let text = "class A:\n    def run(self):\n\n        for x in xs:\n            use(x)\n        return 1\n";
        fs::write(dir.path().join("a.py"), text).unwrap();
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        let function = Function {
            id: "a.py#1".to_owned(),
            path: "a.py".to_owned(),
            start_line: 2,
            end_line: 6,
            hash: String::new(),
        };
        let mut sources = Sources::new(
            dir.path(),
            BTreeMap::from([("a.py".to_owned(), hash.clone())]),
        );
        assert_eq!(
            sources.text(&function).as_deref(),
            Some("def run(self):\n    for x in xs:\n        use(x)\n    return 1\n")
        );
        // The file no longer matches what was indexed: no text, so nothing is cached for it.
        let mut stale = Sources::new(
            dir.path(),
            BTreeMap::from([("a.py".to_owned(), hash + "x")]),
        );
        assert_eq!(stale.text(&function), None);
    }

    #[test]
    fn dot_is_the_sum_of_products_whatever_the_length() {
        for length in [0_u16, 1, 7, 8, 9, 1024] {
            let a: Vec<f32> = (0..length).map(|i| f32::from(i % 5)).collect();
            let b: Vec<f32> = (0..length).map(|i| f32::from(i % 3)).collect();
            let expected: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
            assert!((dot(&a, &b) - expected).abs() <= 1e-3 * expected.max(1.0));
        }
    }

    #[test]
    fn copies_of_one_function_take_one_slot_between_them() {
        // 0, 1 and 2 are one cluster, all close to 3; 4 is a different function near 3 too.
        let vectors = [
            at_angle(0.0),
            at_angle(1.0),
            at_angle(2.0),
            at_angle(4.0),
            at_angle(12.0),
        ];
        let cluster = |place: usize| place.min(2) * usize::from(place < 3);
        let pairs = neighbours(&held(&vectors), 2, 0.5, |_, _| true, cluster);
        let from_three: Vec<usize> = pairs
            .iter()
            .filter_map(|&(a, b, _)| (b == 3).then_some(a).or((a == 3).then_some(b)))
            .collect();
        assert!(from_three.contains(&4), "{pairs:?}");
    }

    #[test]
    fn nothing_below_the_floor_is_a_neighbour() {
        let vectors = [at_angle(0.0), at_angle(80.0)];
        assert_eq!(
            neighbours(&held(&vectors), 5, 0.5, |_, _| true, |place| place).len(),
            0
        );
    }

    #[test]
    fn budget_keeps_the_strongest_and_counts_the_rest() {
        let (kept, skipped) = within_budget(vec![1, 2, 3, 4, 5], 3);
        assert_eq!((kept, skipped), (vec![1, 2, 3], 2));
        let (kept, skipped) = within_budget(vec![1, 2], 3);
        assert_eq!((kept, skipped), (vec![1, 2], 0));
    }

    #[test]
    fn clusters_stop_growing_at_the_size_limit() {
        let mut components = Components::new(6);
        assert!(join_within(&mut components, 0, 1, 3));
        assert!(join_within(&mut components, 1, 2, 3));
        // {0, 1, 2} and {3} would make four.
        assert!(!join_within(&mut components, 2, 3, 3));
        assert!(join_within(&mut components, 4, 5, 3));
        assert!(!join_within(&mut components, 4, 5, 3));
    }

    #[test]
    fn answers_are_keyed_by_the_ordered_pair() {
        assert_eq!(answer_key("b", "a"), answer_key("a", "b"));
    }

    #[test]
    fn without_inference_in_the_config_the_analysis_is_off() {
        let config = Config::default();
        assert_eq!(setup(&config).err(), Some(SemanticOff::NotConfigured));
    }

    #[test]
    fn without_the_key_in_the_environment_the_analysis_is_off() {
        let config = Config {
            inference: Some(Inference {
                embedding_model: "e".to_owned(),
                decision_model: "d".to_owned(),
                base_url: "http://localhost".to_owned(),
                api_key_env: "CODEMAP_TEST_KEY_THAT_IS_NEVER_SET".to_owned(),
            }),
            ..Config::default()
        };
        assert_eq!(
            setup(&config).err(),
            Some(SemanticOff::MissingKey {
                env: "CODEMAP_TEST_KEY_THAT_IS_NEVER_SET".to_owned()
            })
        );
    }

    mod seeded {
        //! The whole analysis through the caches: vectors and answers are seeded, so no call is
        //! made (the configured address would refuse one).

        use std::fs;
        use std::path::Path;

        use duckdb::Connection;

        use super::super::*;
        use crate::index::index;

        /// A full run: the refresh, then the semantic step.
        fn index_all(root: &Path) -> Result<SemanticStatus, crate::IndexError> {
            index(root)?;
            Index::create(root)?.refresh_semantic()
        }

        const RUST: &str = "fn total(items: &[Item]) -> f64 {
    let mut sum = 0.0;
    for item in items {
        if item.active {
            sum += item.price * item.count as f64;
        } else {
            sum -= item.refund;
        }
    }
    let tax = sum * 0.2;
    let shipping = if sum > 100.0 { 0.0 } else { 5.0 };
    let discount = sum * 0.1;
    sum + tax + shipping - discount
}
";

        const PYTHON: &str = "def order_value(lines):
    value = 0
    for line in lines:
        if line.enabled:
            value += line.cost * line.qty
        else:
            value -= line.credit
    duty = value * 0.2
    carriage = 0 if value > 100 else 5
    rebate = value * 0.1
    total = value + duty
    total = total + carriage
    return total - rebate
";

        const UNRELATED: &str = "def retry(action, attempts=3):
    for attempt in range(attempts):
        try:
            return action()
        except OSError as error:
            last = error
            time.sleep(2 ** attempt)
            log.warning('retrying after %s', error)
    raise RuntimeError('gave up') from last
";

        fn write(root: &Path, path: &str, text: &str) {
            fs::write(root.join(path), text).unwrap();
        }

        /// Indexes `files`, then (with models configured) seeds a vector per function name and
        /// an answer for each named pair, and indexes again.
        fn semantic_run(
            files: &[(&str, &str)],
            vectors: &[(&str, [f32; 3])],
            answers: &[(&str, &str, f64)],
            configured: bool,
        ) -> (tempfile::TempDir, SemanticStatus) {
            let dir = tempfile::tempdir().unwrap();
            for (path, text) in files {
                write(dir.path(), path, text);
            }
            index(dir.path()).unwrap();
            if configured {
                write(
                    dir.path(),
                    ".codemap/config.toml",
                    "[inference]\nembedding_model = \"embed-test\"\ndecision_model = \"decide-test\"\n\
                     base_url = \"http://127.0.0.1:9\"\napi_key_env = \"PATH\"\n",
                );
                let connection =
                    Connection::open(dir.path().join(".codemap/index.duckdb")).unwrap();
                let hash = |name: &str| -> String {
                    connection
                        .query_row(
                            "SELECT content_hash FROM symbols WHERE name = ? LIMIT 1",
                            params![name],
                            |row| row.get(0),
                        )
                        .unwrap()
                };
                for (name, vector) in vectors {
                    if !files.iter().any(|(_, text)| text.contains(name)) {
                        continue;
                    }
                    connection
                        .execute(
                            "INSERT INTO embeddings VALUES (?, 'embed-test', ?)",
                            params![hash(name), vector_bytes(vector)],
                        )
                        .unwrap();
                }
                for (first, second, probability) in answers {
                    let key = answer_key(&hash(first), &hash(second));
                    connection
                        .execute(
                            "INSERT INTO answers VALUES (?, ?, 'decide-test', ?, ?)",
                            params![key.0, key.1, SAME_BEHAVIOUR_PROMPT_VERSION, probability],
                        )
                        .unwrap();
                }
            }
            let status = index_all(dir.path()).unwrap();
            (dir, status)
        }

        fn kinds(root: &Path) -> Vec<String> {
            let connection = Connection::open(root.join(".codemap/index.duckdb")).unwrap();
            let mut statement = connection
                .prepare("SELECT kind FROM \"similar\" ORDER BY kind")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        }

        fn matches(root: &Path) -> Vec<String> {
            let connection = Connection::open(root.join(".codemap/index.duckdb")).unwrap();
            let mut statement = connection
                .prepare("SELECT labels FROM findings WHERE detector = 'duplication'")
                .unwrap();
            let labels: Vec<String> = statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            labels
                .iter()
                .map(|text| {
                    let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
                    parsed["match"].as_str().unwrap().to_owned()
                })
                .collect()
        }

        const FILES: [(&str, &str); 3] = [("a.rs", RUST), ("b.py", PYTHON), ("c.py", UNRELATED)];
        const VECTORS: [(&str, [f32; 3]); 3] = [
            ("total", [1.0, 0.0, 0.0]),
            ("order_value", [0.95, 0.31, 0.0]),
            ("retry", [0.0, 0.0, 1.0]),
        ];

        #[test]
        fn a_confirmed_pair_across_languages_becomes_a_semantic_group() {
            let (dir, status) =
                semantic_run(&FILES, &VECTORS, &[("total", "order_value", 0.95)], true);
            assert_eq!(kinds(dir.path()), vec!["semantic"]);
            assert_eq!(matches(dir.path()), vec!["semantic", "semantic"]);
            let SemanticStatus::Ran(run) = status else {
                panic!("did not run: {status:?}");
            };
            // Only the two similar functions are candidates, and the cache answered.
            assert_eq!(
                (run.function_count, run.embedded_count, run.candidate_count),
                (3, 0, 1)
            );
            assert_eq!((run.asked_count, run.confirmed_count), (0, 1));
            assert_eq!(run.cost_micro_usd, Some(0));
            // Nothing changed since, so the next refresh does no semantic work.
            assert_eq!(index_all(dir.path()).unwrap(), SemanticStatus::Unchanged);
        }

        #[test]
        fn semantic_pairs_survive_a_refresh_that_rebuilds_the_deterministic_ones() {
            let (dir, _) = semantic_run(&FILES, &VECTORS, &[("total", "order_value", 0.95)], true);
            write(dir.path(), "c.py", &format!("{UNRELATED}\n# edited\n"));
            let status = index_all(dir.path()).unwrap();
            assert_eq!(kinds(dir.path()), vec!["semantic"]);
            assert!(matches!(status, SemanticStatus::Ran(_)));
        }

        #[test]
        fn a_confirmed_pair_stays_when_newer_code_crowds_it_out_of_the_neighbours() {
            let lookalike = |name: &str, body: &str| {
                format!("def {name}(rows):\n    out = []\n    seen = set()\n{body}    return out\n")
            };
            let bodies = [
                "    for row in rows:\n        if row.key not in seen:\n            seen.add(row.key)\n            out.append(row)\n",
                "    index = 0\n    while index < len(rows):\n        out.append(rows[index])\n        index += 2\n    seen.add(index)\n",
                "    for left, right in zip(rows, rows[1:]):\n        if left.key == right.key:\n            out.append(left)\n    seen.update(out)\n",
                "    out = sorted(rows, key=lambda row: row.key, reverse=True)\n    seen.update(row.key for row in out)\n    out = out[:10]\n",
            ];
            let names = ["pick_a", "pick_b", "pick_c", "pick_d"];
            let mut files: Vec<(String, String)> = FILES
                .iter()
                .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
                .collect();
            let mut vectors = vec![
                ("total", [1.0, 0.0, 0.0]),
                ("order_value", [0.906, 0.423, 0.0]),
                ("retry", [0.0, 0.0, 1.0]),
            ];
            let mut answers = vec![("total", "order_value", 0.95)];
            for (angle_deg, (name, body)) in [8.0_f32, 12.0, 16.0, 20.0]
                .into_iter()
                .zip(names.iter().zip(bodies))
            {
                files.push((format!("{name}.py"), lookalike(name, body)));
                let angle = angle_deg.to_radians();
                vectors.push((name, [angle.cos(), angle.sin(), 0.0]));
            }
            // Every other pair is answered (no) so that nothing is asked.
            let all: Vec<&str> = vectors.iter().map(|(name, _)| *name).collect();
            for (i, a) in all.iter().enumerate() {
                for b in &all[i + 1..] {
                    if (*a, *b) != ("total", "order_value") {
                        answers.push((a, b, 0.1));
                    }
                }
            }
            let borrowed: Vec<(&str, &str)> = files
                .iter()
                .map(|(p, t)| (p.as_str(), t.as_str()))
                .collect();
            let (dir, _) = semantic_run(&borrowed, &vectors, &answers, true);
            assert_eq!(kinds(dir.path()), vec!["semantic"]);
        }

        #[test]
        fn the_overview_says_what_the_semantic_step_did() {
            let said = |root: &Path| match crate::answer(
                &Index::open(root).unwrap(),
                &crate::Query::Overview,
            )
            .unwrap()
            {
                crate::Answer::Overview(overview) => overview.semantic,
                other => panic!("not an overview: {other:?}"),
            };
            let (dir, status) =
                semantic_run(&FILES, &VECTORS, &[("total", "order_value", 0.95)], true);
            assert_eq!(said(dir.path()), status.summary());
            // A refresh that rebuilds the analyses leaves the step due until it has run again.
            write(dir.path(), "c.py", &format!("{UNRELATED}\n# edited\n"));
            index(dir.path()).unwrap();
            assert_eq!(said(dir.path()), SemanticStatus::Pending.summary());
        }

        #[test]
        fn a_pair_the_model_rejects_is_not_a_duplicate() {
            let (dir, status) =
                semantic_run(&FILES, &VECTORS, &[("total", "order_value", 0.1)], true);
            assert_eq!(kinds(dir.path()).len(), 0);
            assert_eq!(matches(dir.path()).len(), 0);
            let SemanticStatus::Ran(run) = status else {
                panic!("did not run: {status:?}");
            };
            assert_eq!((run.candidate_count, run.confirmed_count), (1, 0));
        }

        #[test]
        fn copies_already_found_are_not_asked_about_again() {
            let files = [("a.rs", RUST), ("d.rs", RUST), ("b.py", PYTHON)];
            let (dir, status) =
                semantic_run(&files, &VECTORS, &[("total", "order_value", 0.95)], true);
            // The two identical functions are one cluster; the Python one joins it once.
            assert_eq!(kinds(dir.path()), vec!["exact", "semantic"]);
            let SemanticStatus::Ran(run) = status else {
                panic!("did not run: {status:?}");
            };
            assert_eq!((run.candidate_count, run.confirmed_count), (1, 1));
        }

        #[test]
        fn without_models_configured_the_analysis_is_skipped() {
            let (dir, status) = semantic_run(&FILES, &VECTORS, &[], false);
            assert_eq!(status, SemanticStatus::Off(SemanticOff::NotConfigured));
            assert_eq!(kinds(dir.path()).len(), 0);
        }
    }
}

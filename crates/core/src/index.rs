//! Discover, hash, diff, extract in parallel, and write what changed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::Ordering;

use duckdb::params;
use ignore::WalkBuilder;
use rayon::prelude::*;

use crate::config::{Config, ConfigError, RoleOverrides};
use crate::detect::{detect, rank};
use crate::facts::{FileFacts, TOKEN_ENCODING, extract};
use crate::reader::{Index, QueryError};
use crate::semantic::{self, SemanticStatus};
use crate::store::{STATE_DIR, Store, StoreError};
use crate::worktree::seed;
use crate::{link, similarity};

/// Version-control directories are never part of the codebase, whatever the ignore files say.
const SKIPPED_DIRS: [&str; 4] = [".git", ".jj", ".hg", ".svn"];

/// Version-control and state directories are never indexed.
pub(crate) fn is_skipped(name: &str) -> bool {
    name == STATE_DIR || SKIPPED_DIRS.contains(&name)
}

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("cannot walk the tree: {0}")]
    Walk(#[from] ignore::Error),
    #[error("cannot read {path}: {source}")]
    Read { path: String, source: io::Error },
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("cannot analyse the index: {0}")]
    Analysis(#[from] QueryError),
}

/// What one run of [`index`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexReport {
    pub file_count: u64,
    pub line_count: u64,
    /// Files extracted this run: changed, new, or all of them after a build change.
    pub extracted_count: u64,
    /// Near-duplicate comparisons left out because a bucket of look-alikes exceeded its budget.
    pub comparisons_skipped: u64,
}

/// A file read this run: unchanged files carry no facts.
struct Scanned {
    path: String,
    hash: String,
    facts: Option<FileFacts>,
}

/// Brings the index under `root` up to date with the tree, creating it when missing. The analysis
/// that needs models is a separate step ([`Index::refresh_semantic`]).
///
/// # Errors
/// Fails when the tree cannot be walked or read, or the index cannot be written.
pub fn index(root: &Path) -> Result<IndexReport, IndexError> {
    Index::create(root)?.refresh()
}

impl Index {
    /// Opens the index under `root` for refreshing, creating it when missing. A missing index in
    /// a worktree starts as a copy of the main checkout's.
    ///
    /// # Errors
    /// Fails when the state directory or database cannot be created or opened.
    pub fn create(root: &Path) -> Result<Self, IndexError> {
        seed(root);
        Ok(Self::new(Store::open(root)?, root))
    }

    /// Brings the index up to date with the tree: hash-and-diff, extract what changed, rerun the
    /// analyses.
    ///
    /// Everything it writes lands in one transaction. `DuckDB` gives every other connection to the
    /// same database (see [`Index::try_clone`]) snapshot isolation, so a reader sees the whole
    /// previous state until the commit and the whole new state after it, never a half-written
    /// index, and is never blocked. Only one refresh may run at a time.
    ///
    /// # Errors
    /// Fails when the tree cannot be walked or read, or the index cannot be written; the index
    /// then keeps its previous state.
    pub fn refresh(&self) -> Result<IndexReport, IndexError> {
        let store = self.store();
        let config = Config::load(self.root())?;
        let roles = RoleOverrides::new(&config.roles)?;
        // Role overrides change what extraction stores, so a change to them re-extracts everything.
        let roles_key = format!("{:?}", config.roles);
        let stale = store.meta("build_id")?.as_deref() != Some(BUILD_ID)
            || store.meta("tokenizer")?.as_deref() != Some(TOKEN_ENCODING)
            || store.meta("roles")?.as_deref() != Some(roles_key.as_str());
        let stored = if stale {
            BTreeMap::new()
        } else {
            store.file_hashes()?
        };

        let root = self.root();
        let paths = discover(root)?;
        let scanned = paths
            .par_iter()
            .map(|path| scan(root, path, stored.get(path), &roles))
            .collect::<Result<Vec<_>, _>>()?;

        let present: BTreeSet<&str> = paths.iter().map(String::as_str).collect();
        let mut stale_paths: Vec<&str> = stored
            .keys()
            .map(String::as_str)
            .filter(|path| !present.contains(path))
            .collect();
        stale_paths.extend(
            scanned
                .iter()
                .filter(|file| file.facts.is_some())
                .map(|file| file.path.as_str()),
        );

        let writing = self.lock_writing();
        store.begin()?;
        let written = self.write(&scanned, stale, &stale_paths, &config, BUILD_ID, &roles_key);
        match written {
            Ok(()) => store.commit()?,
            Err(error) => {
                store.rollback()?;
                return Err(error);
            }
        }
        // Before the lock is released, so a semantic run waiting for it sees it was superseded.
        self.generation.fetch_add(1, Ordering::SeqCst);
        drop(writing);
        self.note_semantic_pending(&config, BUILD_ID)?;
        let (file_count, line_count) = store.totals()?;
        Ok(IndexReport {
            file_count,
            line_count,
            comparisons_skipped: store
                .meta(similarity::SKIPPED_META)?
                .and_then(|count| count.parse().ok())
                .unwrap_or(0),
            extracted_count: scanned.iter().filter(|file| file.facts.is_some()).count() as u64,
        })
    }

    /// The writes of one refresh, inside the caller's transaction.
    fn write(
        &self,
        scanned: &[Scanned],
        stale: bool,
        stale_paths: &[&str],
        config: &Config,
        build_id: &str,
        roles_key: &str,
    ) -> Result<(), IndexError> {
        let store = self.store();
        if stale {
            store.clear()?;
        } else {
            store.delete_files(stale_paths)?;
        }
        store.insert(scanned.iter().filter_map(|file| {
            file.facts
                .as_ref()
                .map(|facts| (file.path.as_str(), file.hash.as_str(), facts))
        }))?;
        store.set_meta("build_id", build_id)?;
        store.set_meta("tokenizer", TOKEN_ENCODING)?;
        store.set_meta("roles", roles_key)?;
        self.analyse(config, build_id)
    }

    /// Runs the analyses, the detectors and ranking over the whole index, unless nothing they read has changed.
    fn analyse(&self, config: &Config, build_id: &str) -> Result<(), IndexError> {
        let key = self.analysis_key(config, build_id)?;
        if self.store().meta("analysis")?.as_deref() == Some(key.as_str()) {
            return Ok(());
        }
        link::run(self)?;
        similarity::run(self)?;
        // Semantic pairs went with the old ones: they are due again.
        self.store().set_meta("semantic", "")?;
        let mut findings = detect(self, config)?;
        let areas = rank(&mut findings, config);
        self.save_analysis(&findings, &areas)?;
        self.store().set_meta("analysis", &key)?;
        Ok(())
    }

    /// Identifies everything the analyses read.
    fn analysis_key(&self, config: &Config, build_id: &str) -> Result<String, IndexError> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(build_id.as_bytes());
        hasher.update(config.text.as_bytes());
        for (path, hash) in self.store().file_hashes()? {
            hasher.update(path.as_bytes());
            hasher.update(hash.as_bytes());
        }
        Ok(hasher.finalize().to_hex().to_string())
    }

    /// Records in plain words what the semantic analysis will do about this refresh, for the
    /// overview: off and why, still due, or (left as it was) done for this very content.
    fn note_semantic_pending(&self, config: &Config, build_id: &str) -> Result<(), IndexError> {
        let status = if let Err(off) = semantic::setup(config) {
            SemanticStatus::Off(off)
        } else {
            let key = self.analysis_key(config, build_id)?;
            if self.store().meta("semantic")?.as_deref() == Some(key.as_str()) {
                SemanticStatus::Unchanged
            } else {
                SemanticStatus::Pending
            }
        };
        Ok(self
            .store()
            .set_meta("semantic_status", &status.summary())?)
    }

    /// Adds semantic pairs, when models are configured. It runs after a refresh has committed,
    /// so the deterministic results are readable while models are slow, and a model API that
    /// fails leaves them standing. A second, short transaction then folds the pairs into
    /// `duplicated`, findings and areas. A refresh that commits meanwhile supersedes the run.
    ///
    /// # Errors
    /// Fails when the index cannot be read or written; a model API that cannot be used is a
    /// [`SemanticStatus::Failed`] result.
    pub fn refresh_semantic(&self) -> Result<SemanticStatus, IndexError> {
        let generation = self.generation.load(Ordering::SeqCst);
        let cancelled = || self.generation.load(Ordering::SeqCst) != generation;
        let config = Config::load(self.root())?;
        let setup = match semantic::setup(&config) {
            Ok(setup) => setup,
            Err(off) => {
                let status = SemanticStatus::Off(off);
                self.store()
                    .set_meta("semantic_status", &status.summary())?;
                return Ok(status);
            }
        };
        let key = self.analysis_key(&config, BUILD_ID)?;
        if self.store().meta("semantic")?.as_deref() == Some(key.as_str()) {
            let status = SemanticStatus::Unchanged;
            self.store()
                .set_meta("semantic_status", &status.summary())?;
            return Ok(status);
        }
        let outcome = semantic::run(self, &setup, &cancelled)?;
        if outcome.status == SemanticStatus::Superseded {
            return Ok(outcome.status);
        }
        let store = self.store();
        let writing = self.lock_writing();
        if cancelled() {
            return Ok(SemanticStatus::Superseded);
        }
        if !outcome.pairs.is_empty() {
            store.begin()?;
            let added = self.add_semantic(&config, &outcome.pairs);
            if let Err(error) = added {
                store.rollback()?;
                return Err(error);
            }
            store.commit()?;
        }
        if outcome.complete {
            store.set_meta("semantic", &key)?;
        }
        store.set_meta("semantic_status", &outcome.status.summary())?;
        drop(writing);
        Ok(outcome.status)
    }

    fn add_semantic(
        &self,
        config: &Config,
        pairs: &[(String, String, f64)],
    ) -> Result<(), IndexError> {
        let mut rows = self
            .store()
            .connection()
            .appender("similar")
            .map_err(QueryError::from)?;
        for (a, b, probability) in pairs {
            rows.append_row(params![a, b, semantic::SEMANTIC, probability])
                .map_err(QueryError::from)?;
        }
        rows.flush().map_err(QueryError::from)?;
        drop(rows);
        similarity::write_duplicated(self)?;
        let mut findings = detect(self, config)?;
        let areas = rank(&mut findings, config);
        self.save_analysis(&findings, &areas)?;
        Ok(())
    }
}

/// Digest of the core and language crates' sources, computed by `build.rs`: a change to either
/// may change what extraction produces.
const BUILD_ID: &str = env!("CODEMAP_INDEX_BUILD");

/// Every file under `root` that ignore files let through, as sorted `/`-separated relative paths.
fn discover(root: &Path) -> Result<Vec<String>, IndexError> {
    let mut paths = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .follow_links(false)
        .sort_by_file_path(Ord::cmp)
        .filter_entry(|entry| !is_skipped(&entry.file_name().to_string_lossy()))
        .build();
    for entry in walker {
        let entry = entry?;
        if entry.file_type().is_some_and(|kind| kind.is_file())
            && let Ok(relative) = entry.path().strip_prefix(root)
        {
            // Stored paths are UTF-8; a name that is not cannot be one.
            let parts: Option<Vec<&str>> = relative
                .components()
                .map(|part| part.as_os_str().to_str())
                .collect();
            paths.extend(parts.map(|parts| parts.join("/")));
        }
    }
    paths.sort();
    Ok(paths)
}

/// Reads and hashes one file, and extracts it unless its hash matches the stored one.
fn scan(
    root: &Path,
    path: &str,
    stored_hash: Option<&String>,
    roles: &RoleOverrides,
) -> Result<Scanned, IndexError> {
    let bytes = fs::read(root.join(path)).map_err(|source| IndexError::Read {
        path: path.to_owned(),
        source,
    })?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let facts = (stored_hash != Some(&hash)).then(|| {
        let mut facts = extract(path, &bytes);
        if let Some(role) = roles.role_for(path) {
            facts.role = role;
            for line in &mut facts.lines {
                line.role = role;
            }
        }
        facts
    });
    Ok(Scanned {
        path: path.to_owned(),
        hash,
        facts,
    })
}

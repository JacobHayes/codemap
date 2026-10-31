//! Discover, hash, diff, extract in parallel, and write what changed.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use ignore::WalkBuilder;
use rayon::prelude::*;

use crate::config::{Config, ConfigError, RoleOverrides};
use crate::detect::{detect, rank};
use crate::facts::{FileFacts, TOKEN_ENCODING, extract};
use crate::reader::{Index, QueryError};
use crate::store::{STATE_DIR, Store, StoreError};

/// Version-control directories are never part of the codebase, whatever the ignore files say.
const SKIPPED_DIRS: [&str; 4] = [".git", ".jj", ".hg", ".svn"];

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("cannot walk the tree: {0}")]
    Walk(#[from] ignore::Error),
    #[error("cannot read {path}: {source}")]
    Read { path: String, source: io::Error },
    #[error("cannot hash the running executable: {0}")]
    BuildId(io::Error),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("cannot analyse the index: {0}")]
    Analysis(#[from] QueryError),
}

/// What one run of [`index`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexReport {
    pub file_count: u64,
    pub line_count: u64,
    /// Files extracted this run: changed, new, or all of them after a build change.
    pub extracted_count: u64,
}

/// A file read this run: unchanged files carry no facts.
struct Scanned {
    path: String,
    hash: String,
    facts: Option<FileFacts>,
}

/// Brings the index under `root` up to date with the tree.
///
/// # Errors
/// Fails when the tree cannot be walked or read, or the index cannot be written.
pub fn index(root: &Path) -> Result<IndexReport, IndexError> {
    let store = Store::open(root)?;
    let config = Config::load(root)?;
    let roles = RoleOverrides::new(&config.roles)?;
    // Role overrides change what extraction stores, so a change to them re-extracts everything.
    let roles_key = format!("{:?}", config.roles);
    let build_id = build_id()?;
    let stale = store.meta("build_id")?.as_deref() != Some(build_id.as_str())
        || store.meta("tokenizer")?.as_deref() != Some(TOKEN_ENCODING)
        || store.meta("roles")?.as_deref() != Some(roles_key.as_str());
    let stored = if stale {
        BTreeMap::new()
    } else {
        store.file_hashes()?
    };

    let paths = discover(root)?;
    let scanned = paths
        .par_iter()
        .map(|path| scan(root, path, stored.get(path), &roles))
        .collect::<Result<Vec<_>, _>>()?;

    let present: std::collections::BTreeSet<&str> = paths.iter().map(String::as_str).collect();
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
    let extracted = scanned.iter().filter(|file| file.facts.is_some());

    store.begin()?;
    if stale {
        store.clear()?;
    } else {
        store.delete_files(&stale_paths)?;
    }
    store.insert(extracted.clone().filter_map(|file| {
        file.facts
            .as_ref()
            .map(|facts| (file.path.as_str(), file.hash.as_str(), facts))
    }))?;
    store.set_meta("build_id", &build_id)?;
    store.set_meta("tokenizer", TOKEN_ENCODING)?;
    store.set_meta("roles", &roles_key)?;
    store.commit()?;

    let (file_count, line_count) = store.totals()?;
    analyse(store, root, &config, &build_id)?;
    Ok(IndexReport {
        file_count,
        line_count,
        extracted_count: extracted.count() as u64,
    })
}

/// Runs the detectors and ranking over the whole index, unless nothing they read has changed.
fn analyse(store: Store, root: &Path, config: &Config, build_id: &str) -> Result<(), IndexError> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(build_id.as_bytes());
    hasher.update(config.text.as_bytes());
    for (path, hash) in store.file_hashes()? {
        hasher.update(path.as_bytes());
        hasher.update(hash.as_bytes());
    }
    let key = hasher.finalize().to_hex().to_string();
    if store.meta("analysis")?.as_deref() == Some(key.as_str()) {
        return Ok(());
    }
    let index = Index::new(store, root);
    let mut findings = detect(&index, config)?;
    let areas = rank(&mut findings, config);
    index.store().begin()?;
    index.save_analysis(&findings, &areas)?;
    index.store().set_meta("analysis", &key)?;
    index.store().commit()?;
    Ok(())
}

/// Hashes the running executable: any rebuild of codemap changes what extraction may produce.
fn build_id() -> Result<String, IndexError> {
    let executable = std::env::current_exe().map_err(IndexError::BuildId)?;
    let file = fs::File::open(executable).map_err(IndexError::BuildId)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(file).map_err(IndexError::BuildId)?;
    Ok(hasher.finalize().to_hex().to_string())
}

/// Every file under `root` that ignore files let through, as sorted `/`-separated relative paths.
fn discover(root: &Path) -> Result<Vec<String>, IndexError> {
    let mut paths = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .follow_links(false)
        .sort_by_file_path(Ord::cmp)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            name != STATE_DIR && !SKIPPED_DIRS.contains(&name.as_ref())
        })
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

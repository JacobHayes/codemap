//! Keeps the index fresh: watches the root and re-indexes after the tree has been quiet.

use std::fs;
use std::io;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use codemap_core::{Index, IndexError, IndexReport, SemanticStatus, is_config, is_indexed};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

/// How long the tree must be quiet before it is re-indexed. An editor save, a formatter run or a
/// `git checkout` touches many files over a few hundred milliseconds, and indexing between them
/// would do the work twice.
const QUIET_PERIOD_MS: u64 = 300;

#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("cannot watch {path}: {source}")]
    Watch { path: String, source: notify::Error },
    #[error("cannot resolve {path}: {source}")]
    Resolve { path: String, source: io::Error },
    #[error(transparent)]
    Index(#[from] IndexError),
}

/// A running watcher; dropping it stops watching.
pub struct Watcher {
    _watcher: RecommendedWatcher,
}

/// Refreshes `index`, then watches its root and, after each quiet period that follows a change
/// to an indexed file or the config, refreshes it again and calls `refreshed` with the outcome.
/// After each refresh the analysis that needs models runs on its own thread, one run at a time:
/// it never delays a refresh, a refresh that commits meanwhile supersedes it, and `semantic_done`
/// hears of each run that finished.
/// Watching starts before the first refresh, and changes that arrive during a refresh queue up
/// and trigger the next one, so no change is missed.
///
/// # Errors
/// Fails when the root cannot be resolved or watched, or the first refresh fails.
pub fn watch(
    index: Index,
    refreshed: impl Fn(Result<IndexReport, IndexError>) + Send + 'static,
    semantic_done: impl Fn(Result<SemanticStatus, IndexError>) + Send + 'static,
) -> Result<Watcher, WatchError> {
    // Events carry the path the watch was registered under, so match against the same one.
    let root = fs::canonicalize(index.root()).map_err(|source| WatchError::Resolve {
        path: index.root().display().to_string(),
        source,
    })?;
    let (sender, receiver) = mpsc::channel();
    let watcher = notify::recommended_watcher(move |event| {
        // The receiver is gone only when the watcher is being dropped.
        let _ = sender.send(event);
    })
    .and_then(|mut watcher| {
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map(|()| watcher)
    })
    .map_err(|source| WatchError::Watch {
        path: root.display().to_string(),
        source,
    })?;
    let (due, queued) = mpsc::channel::<()>();
    let semantic_index = index.try_clone().map_err(IndexError::from)?;
    thread::spawn(move || {
        while queued.recv().is_ok() {
            // Runs queued while one was going are covered by the next.
            while queued.try_recv().is_ok() {}
            semantic_done(semantic_index.refresh_semantic());
        }
    });
    index.refresh()?;
    // The worker is gone only when the watcher is being dropped.
    let _ = due.send(());
    let quiet_period = Duration::from_millis(QUIET_PERIOD_MS);
    let is_relevant = {
        let root = root.clone();
        move |event: &notify::Result<notify::Event>| match event {
            // A lost or failed event may have hidden a change.
            Err(_) => true,
            Ok(event) => {
                !matches!(event.kind, EventKind::Access(_))
                    && event
                        .paths
                        .iter()
                        .any(|path| is_indexed(&root, path) || is_config(&root, path))
            }
        }
    };
    thread::spawn(move || {
        while let Ok(first) = receiver.recv() {
            if !is_relevant(&first) {
                continue;
            }
            // Each relevant event pushes the deadline out; irrelevant ones never delay it.
            let mut deadline = Instant::now() + quiet_period;
            loop {
                match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(event) if is_relevant(&event) => deadline = Instant::now() + quiet_period,
                    Ok(_) => {}
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            let result = index.refresh();
            let succeeded = result.is_ok();
            refreshed(result);
            if succeeded {
                let _ = due.send(());
            }
        }
    });
    Ok(Watcher { _watcher: watcher })
}

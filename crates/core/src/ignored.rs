//! Whether a path belongs to the index, for the watcher: the same answer [`crate::index`] gives by
//! walking, without the walk.

use std::path::Path;

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::index::is_skipped;

/// The `.gitignore` and `.ignore` files directly inside `dir`, as one matcher.
fn layer(dir: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(dir);
    // A missing or unreadable ignore file has no rules, which is what the walk would see too.
    let _ = builder.add(dir.join(".gitignore"));
    let _ = builder.add(dir.join(".ignore"));
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

/// Whether `path` (absolute, under `root`) is a file the index would hold: not in a
/// version-control or state directory, and not ignored by the ignore files from `root` down to it.
///
/// Global git excludes and ignore files above `root` are not consulted, so a path they hide is
/// reported as indexed: the watcher then re-indexes for nothing, and the index stays right.
#[must_use]
pub fn is_indexed(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let parts: Vec<_> = relative.components().collect();
    let mut layers = vec![layer(root)];
    let mut current = root.to_owned();
    for (position, part) in parts.iter().enumerate() {
        if is_skipped(&part.as_os_str().to_string_lossy()) {
            return false;
        }
        current.push(part);
        let is_dir = position + 1 < parts.len() || current.is_dir();
        // The nearest ignore file with an opinion decides, as in the walk.
        let ignored = layers
            .iter()
            .rev()
            .map(|layer| layer.matched(&current, is_dir))
            .find(|found| !matches!(found, Match::None))
            .is_some_and(|found| found.is_ignore());
        if ignored {
            return false;
        }
        if is_dir {
            layers.push(layer(&current));
        }
    }
    !parts.is_empty()
}

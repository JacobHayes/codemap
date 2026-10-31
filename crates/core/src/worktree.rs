//! Worktrees start warm: a git worktree or jj workspace without an index of its own begins with a
//! copy of the main checkout's, and the next hash-and-diff brings it up to date. Finding the main
//! checkout is the only thing codemap asks version control.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::store::{DATABASE_FILE, STATE_DIR};

/// Copies the main checkout's index into `root` when `root` is a worktree with no index yet.
///
/// Seeding only saves work, so every failure (no version control, no index to copy, an
/// unreadable source) leaves `root` to be indexed from scratch.
pub(crate) fn seed(root: &Path) {
    if root.join(STATE_DIR).join(DATABASE_FILE).exists() {
        return;
    }
    let Some(main) = main_checkout(root) else {
        return;
    };
    if fs::canonicalize(root).is_ok_and(|root| root == main) {
        return;
    }
    // A failed copy leaves nothing behind but a temp file the next attempt overwrites.
    let _ = copy_index(&main, root);
}

/// The main checkout of the workspace or worktree at `root`; none when `root` is itself a main
/// checkout or is not under version control.
fn main_checkout(root: &Path) -> Option<PathBuf> {
    jj_main(root).or_else(|| git_main(root))
}

/// A non-default jj workspace's `.jj/repo` is a file holding the path (relative to `.jj`) of the
/// main checkout's `.jj/repo` directory.
fn jj_main(root: &Path) -> Option<PathBuf> {
    let jj = root.join(".jj");
    let pointer = fs::read_to_string(jj.join("repo")).ok()?;
    let repo = fs::canonicalize(jj.join(pointer.trim())).ok()?;
    Some(repo.parent()?.parent()?.to_owned())
}

/// A git worktree's `.git` is a file, and the common git directory is the main checkout's `.git`.
fn git_main(root: &Path) -> Option<PathBuf> {
    if !root.join(".git").is_file() {
        return None;
    }
    let output = Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let common = fs::canonicalize(root.join(String::from_utf8(output.stdout).ok()?.trim())).ok()?;
    (common.file_name()? == ".git").then(|| common.parent().map(Path::to_owned))?
}

/// Copies `<main>/.codemap/index.duckdb` to `<root>/.codemap/index.duckdb` through a temp file and
/// a rename, so a reader never finds a partial index. Skipped when the source may be mid-write: a
/// write-ahead log beside it, or its size or modification time changing while it was copied.
fn copy_index(main: &Path, root: &Path) -> io::Result<()> {
    let source = main.join(STATE_DIR).join(DATABASE_FILE);
    if !source.is_file()
        || main
            .join(STATE_DIR)
            .join(format!("{DATABASE_FILE}.wal"))
            .exists()
    {
        return Ok(());
    }
    let before = fs::metadata(&source)?;
    let dir = root.join(STATE_DIR);
    fs::create_dir_all(&dir)?;
    let temp = dir.join(format!("{DATABASE_FILE}.seed"));
    fs::copy(&source, &temp)?;
    let after = fs::metadata(&source)?;
    if after.len() != before.len() || after.modified()? != before.modified()? {
        return fs::remove_file(&temp);
    }
    fs::rename(&temp, dir.join(DATABASE_FILE))
}

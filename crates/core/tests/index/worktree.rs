//! Worktrees start warm: a copy of the main checkout's index, brought up to date by the next run.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::common::{copy_tree, dump, tree_of};
use codemap_core::{STATE_DIR, index};

const EDITED: &str = "def added():\n    return 1\n";

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// A fresh index of the python fixture with the same edit as the worktrees get.
fn fresh_edited_dump() -> Vec<String> {
    let fresh = tree_of("python");
    fs::write(fresh.path().join("greeter.py"), EDITED).unwrap();
    let report = index(fresh.path()).unwrap();
    assert_eq!(report.extracted_count, report.file_count);
    dump(fresh.path())
}

#[test]
fn a_git_worktree_starts_from_the_main_index_and_re_extracts_only_changes() {
    let parent = tempfile::tempdir().unwrap();
    let main = parent.path().join("main");
    copy_tree(&crate::common::fixture("python"), &main);
    git(&main, &["init", "-q"]);
    let first = index(&main).unwrap();
    git(&main, &["add", "-A"]);
    git(&main, &["commit", "-q", "-m", "files"]);
    let worktree = parent.path().join("wt");
    git(
        &main,
        &["worktree", "add", "-q", worktree.to_str().unwrap()],
    );

    fs::write(worktree.join("greeter.py"), EDITED).unwrap();
    let report = index(&worktree).unwrap();

    assert_eq!(report.extracted_count, 1);
    assert_eq!(report.file_count, first.file_count);
    assert_eq!(dump(&worktree), fresh_edited_dump());
}

#[test]
fn a_jj_workspace_starts_from_the_main_index() {
    let parent = tempfile::tempdir().unwrap();
    let main = parent.path().join("main");
    copy_tree(&crate::common::fixture("python"), &main);
    fs::create_dir_all(main.join(".jj/repo")).unwrap();
    index(&main).unwrap();
    let workspace = parent.path().join("ws");
    copy_tree(&crate::common::fixture("python"), &workspace);
    fs::create_dir_all(workspace.join(".jj")).unwrap();
    fs::write(workspace.join(".jj/repo"), "../../main/.jj/repo").unwrap();

    fs::write(workspace.join("greeter.py"), EDITED).unwrap();
    let report = index(&workspace).unwrap();

    assert_eq!(report.extracted_count, 1);
    assert_eq!(dump(&workspace), fresh_edited_dump());
}

#[test]
fn a_directory_that_is_not_a_worktree_is_indexed_from_scratch() {
    let parent = tempfile::tempdir().unwrap();
    let main = parent.path().join("main");
    copy_tree(&crate::common::fixture("python"), &main);
    index(&main).unwrap();
    let sibling = parent.path().join("sibling");
    copy_tree(&main, &sibling);

    let report = index(&sibling).unwrap();

    assert_eq!(report.extracted_count, report.file_count);
}

#[test]
fn a_worktree_whose_main_has_no_index_is_indexed_from_scratch() {
    let parent = tempfile::tempdir().unwrap();
    let main = parent.path().join("main");
    copy_tree(&crate::common::fixture("python"), &main);
    git(&main, &["init", "-q"]);
    git(&main, &["add", "-A"]);
    git(&main, &["commit", "-q", "-m", "files"]);
    let worktree = parent.path().join("wt");
    git(
        &main,
        &["worktree", "add", "-q", worktree.to_str().unwrap()],
    );

    let report = index(&worktree).unwrap();

    assert_eq!(report.extracted_count, report.file_count);
    assert!(!main.join(STATE_DIR).exists());
}

#[test]
fn a_main_index_that_may_be_mid_write_is_not_copied() {
    let parent = tempfile::tempdir().unwrap();
    let main = parent.path().join("main");
    copy_tree(&crate::common::fixture("python"), &main);
    fs::create_dir_all(main.join(".jj/repo")).unwrap();
    index(&main).unwrap();
    // A write-ahead log beside the index means writes are not yet folded into the file.
    fs::write(main.join(STATE_DIR).join("index.duckdb.wal"), b"").unwrap();
    let workspace = parent.path().join("ws");
    copy_tree(&crate::common::fixture("python"), &workspace);
    fs::create_dir_all(workspace.join(".jj")).unwrap();
    fs::write(workspace.join(".jj/repo"), "../../main/.jj/repo").unwrap();

    let report = index(&workspace).unwrap();

    assert_eq!(report.extracted_count, report.file_count);
}

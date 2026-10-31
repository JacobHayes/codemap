//! The watcher's question "is this path indexed?" agrees with what indexing holds.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::common::open;
use codemap_core::{index, is_indexed};

fn write(root: &Path, path: &str) {
    let full = root.join(path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, "x = 1\n").unwrap();
}

fn files_on_disk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_on_disk(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn is_indexed_agrees_with_the_files_indexing_holds() {
    let tree = tempfile::tempdir().unwrap();
    let root = tree.path();
    fs::write(root.join(".gitignore"), "target/\n*.log\n!keep.log\n").unwrap();
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("sub/.gitignore"), "local.py\n/rooted.py\n").unwrap();
    fs::write(root.join("sub/.ignore"), "!notes.log\n").unwrap();
    for path in [
        "a.py",
        "target/debug/out.py",
        "x.log",
        "keep.log",
        "sub/b.py",
        "sub/local.py",
        "sub/rooted.py",
        "sub/deep/rooted.py",
        "sub/deep/local.py",
        "sub/notes.log",
        ".git/config",
        ".jj/repo.py",
    ] {
        write(root, path);
    }
    index(root).unwrap();
    let indexed: BTreeSet<String> = {
        let connection = open(root);
        let mut statement = connection.prepare("SELECT path FROM files").unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };

    let mut on_disk = Vec::new();
    files_on_disk(root, &mut on_disk);
    // The index's own files are never indexed either.
    assert!(
        on_disk
            .iter()
            .any(|path| path.starts_with(root.join(".codemap")))
    );
    for path in on_disk {
        let relative = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            is_indexed(root, &path),
            indexed.contains(&relative),
            "{relative}"
        );
    }
    assert!(!indexed.is_empty());
}

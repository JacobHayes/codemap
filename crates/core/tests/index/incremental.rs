//! Hash-and-diff: unchanged files are left alone, and an incremental index equals a fresh one.

use std::fs;
use std::path::Path;

use crate::common::{copy_tree, dump, fixture, tree_of};
use codemap_core::{STATE_DIR, index};

#[test]
fn unchanged_files_are_not_extracted_again() {
    let tree = tree_of("python");
    let first = index(tree.path()).unwrap();
    assert_eq!(first.extracted_count, first.file_count);

    assert_eq!(index(tree.path()).unwrap().extracted_count, 0);

    // Same bytes again is not a change; different bytes in one file is exactly one.
    let path = tree.path().join("greeter.py");
    let original = fs::read(&path).unwrap();
    fs::write(&path, &original).unwrap();
    assert_eq!(index(tree.path()).unwrap().extracted_count, 0);
    fs::write(&path, [original.as_slice(), b"\nx = 1\n"].concat()).unwrap();
    assert_eq!(index(tree.path()).unwrap().extracted_count, 1);
}

#[test]
fn the_state_directory_is_ignored_by_git_except_its_config() {
    let tree = tree_of("text");
    index(tree.path()).unwrap();
    let ignore = fs::read_to_string(tree.path().join(STATE_DIR).join(".gitignore")).unwrap();
    assert!(ignore.lines().any(|line| line == "*"));
    assert!(ignore.lines().any(|line| line == "!config.toml"));
    // The index never indexes itself.
    let report = index(tree.path()).unwrap();
    assert_eq!(report.file_count, 2);
}

#[test]
fn ignore_files_are_honoured_without_version_control() {
    let tree = tree_of("text");
    fs::write(tree.path().join(".gitignore"), "skipped/\n").unwrap();
    fs::create_dir(tree.path().join("skipped")).unwrap();
    fs::write(tree.path().join("skipped/x.txt"), "x\n").unwrap();
    let connection = {
        index(tree.path()).unwrap();
        crate::common::open(tree.path())
    };
    let skipped: u64 = connection
        .query_row(
            "SELECT count(*) FROM files WHERE path LIKE 'skipped/%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(skipped, 0);
}

/// A tiny deterministic generator, so a failure replays.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(self.0 >> 33).unwrap() % bound
    }
}

fn files_under(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    collect_files(root, "", &mut found);
    found.sort();
    found
}

fn collect_files(dir: &Path, prefix: &str, found: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == STATE_DIR {
            continue;
        }
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if entry.file_type().unwrap().is_dir() {
            collect_files(&entry.path(), &path, found);
        } else {
            found.push(path);
        }
    }
}

#[test]
fn an_incremental_index_equals_a_fresh_one() {
    let mixed = tempfile::tempdir().unwrap();
    for name in ["rust", "python", "text"] {
        copy_tree(&fixture(name), &mixed.path().join(name));
    }
    let live = tempfile::tempdir().unwrap();
    copy_tree(mixed.path(), live.path());
    index(live.path()).unwrap();

    let donors = files_under(mixed.path());
    let mut random = Lcg(7);
    // Each index costs ~0.3 s in a debug build, so few rounds carry many mutations.
    for round in 0..5 {
        let present = files_under(live.path());
        for _ in 0..=random.next(5) {
            let donor = &donors[random.next(donors.len())];
            let bytes = fs::read(mixed.path().join(donor)).unwrap();
            match random.next(3) {
                0 if !present.is_empty() => {
                    fs::remove_file(live.path().join(&present[random.next(present.len())])).ok();
                }
                1 if !present.is_empty() => {
                    // Edit: replace a file's content with another file's, extension kept.
                    let target = live.path().join(&present[random.next(present.len())]);
                    if target.exists() {
                        fs::write(target, bytes).unwrap();
                    }
                }
                _ => {
                    let target = live
                        .path()
                        .join(format!("added/{round}/{}", donor.replace('/', "_")));
                    fs::create_dir_all(target.parent().unwrap()).unwrap();
                    fs::write(target, bytes).unwrap();
                }
            }
        }
        index(live.path()).unwrap();

        let fresh = tempfile::tempdir().unwrap();
        copy_tree(live.path(), fresh.path());
        index(fresh.path()).unwrap();
        assert_eq!(dump(live.path()), dump(fresh.path()), "round {round}");
    }
}

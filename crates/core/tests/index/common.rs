//! Helpers shared by the indexing tests: fixture trees and a read-only view of the index.

use std::fs;
use std::path::{Path, PathBuf};

use duckdb::Connection;

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Copies a directory tree, skipping the index.
pub fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_name() == codemap_core::STATE_DIR {
            continue;
        }
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A temporary tree holding a copy of the named fixture.
pub fn tree_of(name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture(name), dir.path());
    dir
}

pub fn open(root: &Path) -> Connection {
    Connection::open(root.join(codemap_core::STATE_DIR).join("index.duckdb")).unwrap()
}

/// Role and concern of one line.
pub fn line(root: &Path, path: &str, line: u32) -> (String, String) {
    open(root)
        .query_row(
            "SELECT role, concern FROM lines WHERE path = ? AND line = ?",
            duckdb::params![path, line],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

/// Symbols of a file as (name, kind, parent name, first line, last line), in file order.
pub fn symbols(root: &Path, path: &str) -> Vec<(String, String, Option<String>, u32, u32)> {
    let connection = open(root);
    let mut statement = connection
        .prepare(
            "SELECT s.name, s.kind, p.name, s.start_line, s.end_line
             FROM symbols s LEFT JOIN symbols p ON p.id = s.parent
             WHERE s.path = ? ORDER BY s.start_line, s.end_line DESC",
        )
        .unwrap();
    statement
        .query_map([path], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// Every row of every table except metadata, as sorted text.
pub fn dump(root: &Path) -> Vec<String> {
    let connection = open(root);
    let mut rows = Vec::new();
    for table in ["files", "lines", "symbols", "refs"] {
        let mut statement = connection
            .prepare(&format!(
                "SELECT CAST(t AS VARCHAR) AS r FROM {table} t ORDER BY r"
            ))
            .unwrap();
        rows.extend(
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .map(Result::unwrap)
                .map(|row| format!("{table}: {row}")),
        );
    }
    rows
}

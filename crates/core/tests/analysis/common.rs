//! Builders for synthetic trees and a helper to index and ask.

use std::fs;
use std::path::Path;

use codemap_core::{Answer, Index, Query, answer, index};

pub fn write(root: &Path, path: &str, text: &str) {
    let full = root.join(path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, text).unwrap();
}

/// `count` small functions of plain logic, five lines each.
pub fn plain_functions(prefix: &str, count: usize) -> String {
    (0..count)
        .map(|i| {
            format!(
                "fn {prefix}{i}(a: u32) -> u32 {{\n    let b = a + 1;\n    let c = b * 2;\n    c - a\n}}\n\n"
            )
        })
        .collect()
}

/// `count` functions that spend most of their lines on the error arm.
pub fn handler_functions(prefix: &str, count: usize) -> String {
    (0..count)
        .map(|i| {
            format!(
                "fn {prefix}{i}(x: Result<u32, String>) -> u32 {{\n    match x {{\n        Ok(v) => v,\n        Err(e) => {{\n            let a = e.len();\n            let b = a + 1;\n            b as u32\n        }}\n    }}\n}}\n\n"
            )
        })
        .collect()
}

/// A tree of ordinary files, to be the codebase the outlier stands out from.
pub fn baseline(root: &Path, files: usize) {
    for i in 0..files {
        write(
            root,
            &format!("src/mod{i}.rs"),
            &plain_functions(&format!("m{i}_"), 6),
        );
    }
}

/// Ordinary files spread over directories of three, to be the peers a directory stands out from.
pub fn baseline_dirs(root: &Path, dirs: usize) {
    for i in 0..dirs * 3 {
        write(
            root,
            &format!("src/pkg{}/mod{i}.rs", i / 3),
            &plain_functions(&format!("m{i}_"), 6),
        );
    }
}

pub fn indexed(root: &Path) -> Index {
    index(root).unwrap();
    Index::open(root).unwrap()
}

pub fn ask(index: &Index, query: &Query) -> Answer {
    answer(index, query).unwrap()
}

pub fn areas(index: &Index) -> codemap_core::AreaList {
    match ask(
        index,
        &Query::Areas {
            top: None,
            path: None,
            detector: None,
        },
    ) {
        Answer::Areas(list) => list,
        other => panic!("not areas: {other:?}"),
    }
}

/// The findings of one detector across every area.
pub fn findings_of(index: &Index, detector: &str) -> Vec<codemap_core::Finding> {
    areas(index)
        .areas
        .iter()
        .flat_map(|area| {
            match ask(
                index,
                &Query::Area {
                    subject: area.subject.clone(),
                },
            ) {
                Answer::Area(detail) => detail.findings,
                other => panic!("not an area: {other:?}"),
            }
        })
        .filter(|finding| finding.detector == detector)
        .collect()
}

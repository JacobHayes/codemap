//! The similarity analysis writes pairs for exact and near copies, and none for code that only
//! looks alike, is too small, is generated or vendored, or is written in another language.

use duckdb::Connection;

use crate::common::{indexed, write};

/// A function with the given names and constant: the same code under every spelling.
pub fn total(name: &str, items: &str, sum: &str, scale: &str) -> String {
    format!(
        "fn {name}({items}: &[Item]) -> f64 {{
    let mut {sum} = 0.0;
    for item in {items} {{
        if item.active {{
            {sum} += item.price * item.count as f64;
        }}
    }}
    {sum} * {scale}
}}
"
    )
}

/// Unrelated logic of about the same size.
const DRAIN: &str = "fn drain(queue: &mut Vec<Job>, limit: usize) -> Option<Job> {
    while queue.len() > limit {
        match queue.pop() {
            Some(job) if job.urgent => return Some(job),
            Some(_) => continue,
            None => break,
        }
    }
    queue.first().cloned()
}
";

/// Every pair as (path of a, path of b, kind), in a stable order.
fn pairs(root: &std::path::Path) -> Vec<(String, String, String, f64)> {
    let connection = Connection::open(root.join(".codemap/index.duckdb")).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT x.path, y.path, p.kind, p.score FROM \"similar\" p
             JOIN symbols x ON x.id = p.a JOIN symbols y ON y.id = p.b
             ORDER BY x.path, y.path, p.kind",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn pairs_of(files: &[(&str, &str)]) -> Vec<(String, String, String, f64)> {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        write(dir.path(), path, text);
    }
    drop(indexed(dir.path()));
    pairs(dir.path())
}

#[test]
fn an_exact_copy_is_an_exact_pair_whatever_its_formatting() {
    let copy = total("total", "items", "sum", "1.5").replace("\n    ", "\n\n        ");
    let found = pairs_of(&[
        ("src/a.rs", &total("total", "items", "sum", "1.5")),
        ("src/b.rs", &copy),
    ]);
    assert_eq!(found.len(), 1);
    assert_eq!(
        (&found[0].0[..], &found[0].1[..], &found[0].2[..]),
        ("src/a.rs", "src/b.rs", "exact")
    );
    assert!((found[0].3 - 1.0).abs() < f64::EPSILON);
}

#[test]
fn a_renamed_copy_is_a_near_pair() {
    let found = pairs_of(&[
        ("src/a.rs", &total("total", "items", "sum", "1.5")),
        ("src/b.rs", &total("cost", "lines", "acc", "2.0")),
    ]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].2, "near");
    assert!(found[0].3 >= 0.8);
}

#[test]
fn copies_chain_into_one_cluster_without_a_pair_for_every_two() {
    let files: Vec<(String, String)> = (0..6)
        .map(|i| {
            (
                format!("src/f{i}.rs"),
                total(&format!("t{i}"), "items", "sum", "1.5"),
            )
        })
        .collect();
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(p, t)| (&p[..], &t[..])).collect();
    // Six identical functions: five pairs to the first, not fifteen.
    assert_eq!(pairs_of(&borrowed).len(), 5);
}

#[test]
fn different_logic_of_a_similar_size_is_not_paired() {
    let found = pairs_of(&[
        ("src/a.rs", &total("total", "items", "sum", "1.5")),
        ("src/b.rs", DRAIN),
    ]);
    assert!(found.is_empty());
}

#[test]
fn tiny_functions_are_not_paired() {
    let tiny = "fn id(x: u32) -> u32 {\n    x\n}\n";
    assert!(pairs_of(&[("src/a.rs", tiny), ("src/b.rs", tiny)]).is_empty());
}

#[test]
fn a_port_to_another_language_is_not_paired() {
    let python = "def total(items):
    total = 0.0
    for item in items:
        if item.active:
            total += item.price * item.count
    return total * 1.5
";
    let found = pairs_of(&[
        ("src/a.rs", &total("total", "items", "sum", "1.5")),
        ("src/a.py", python),
    ]);
    assert!(found.is_empty());
}

#[test]
fn vendored_and_generated_copies_are_outside_the_population() {
    let code = total("total", "items", "sum", "1.5");
    let found = pairs_of(&[
        ("src/a.rs", &code),
        ("vendor/dep/b.rs", &code),
        ("src/gen.rs", &format!("// @generated\n{code}")),
    ]);
    assert!(found.is_empty());
}

//! Every language's table, through the index: symbols, roles and concerns on specific lines.

use crate::common::{line, symbols, tree_of};
use codemap_core::index;

fn kinds(root: &std::path::Path, path: &str, number: u32) -> (String, String) {
    line(root, path, number)
}

fn pair(role: &str, concern: &str) -> (String, String) {
    (role.to_owned(), concern.to_owned())
}

#[test]
fn rust_lines_carry_role_and_concern() {
    let tree = tree_of("rust");
    index(tree.path()).unwrap();
    let at = |number| kinds(tree.path(), "src/lib.rs", number);
    assert_eq!(at(1), pair("prod", "comment"));
    assert_eq!(at(2), pair("prod", "import"));
    assert_eq!(at(4), pair("prod", "comment"));
    assert_eq!(at(5), pair("prod", "core"));
    assert_eq!(at(10), pair("prod", "assertion"));
    assert_eq!(at(11), pair("prod", "logging"));
    assert_eq!(at(13), pair("prod", "core"));
    // The error arm is handling, and the log call inside it is logging.
    assert_eq!(at(14), pair("prod", "handler"));
    assert_eq!(at(15), pair("prod", "logging"));
    assert_eq!(at(16), pair("prod", "handler"));
    assert_eq!(at(18), pair("prod", "core"));
    // Test modules override the path role, attribute lines included.
    assert_eq!(at(21), pair("test", "core"));
    assert_eq!(at(23), pair("test", "import"));
    assert_eq!(at(27), pair("test", "assertion"));
    assert_eq!(at(29), pair("test", "core"));
    // `if let Err` and the `else` of `let .. else` are handling; the success branch is not.
    assert_eq!(at(37), pair("prod", "handler"));
    assert_eq!(at(38), pair("prod", "handler"));
    assert_eq!(at(40), pair("prod", "core"));
    assert_eq!(at(42), pair("prod", "handler"));
    assert_eq!(at(43), pair("prod", "handler"));
    assert_eq!(at(45), pair("prod", "core"));
}

#[test]
fn rust_symbols_nest_and_tests_are_tests() {
    let tree = tree_of("rust");
    index(tree.path()).unwrap();
    let found = symbols(tree.path(), "src/lib.rs");
    let expect = |name: &str, kind: &str, parent: Option<&str>, first, last| {
        (
            name.to_owned(),
            kind.to_owned(),
            parent.map(str::to_owned),
            first,
            last,
        )
    };
    assert_eq!(
        found,
        vec![
            expect("Shape", "type", None, 5, 7),
            expect("area", "function", None, 9, 19),
            expect("tests", "module", None, 22, 29),
            expect("area_is_positive", "test", Some("tests"), 26, 28),
            // Methods sit under the type their `impl` block names.
            expect("Shape", "type", None, 31, 47),
            expect("sides", "function", Some("Shape"), 32, 34),
            expect("checked", "function", Some("Shape"), 36, 46),
        ]
    );
}

#[test]
fn python_lines_carry_role_and_concern() {
    let tree = tree_of("python");
    index(tree.path()).unwrap();
    let at = |number| kinds(tree.path(), "greeter.py", number);
    assert_eq!(at(1), pair("prod", "comment"));
    assert_eq!(at(2), pair("prod", "import"));
    assert_eq!(at(5), pair("prod", "core"));
    assert_eq!(at(6), pair("prod", "comment"));
    assert_eq!(at(9), pair("prod", "comment"));
    assert_eq!(at(11), pair("prod", "logging"));
    assert_eq!(at(12), pair("prod", "core"));
    assert_eq!(at(14), pair("prod", "handler"));
    assert_eq!(at(15), pair("prod", "logging"));
    assert_eq!(at(16), pair("prod", "handler"));
    assert_eq!(at(17), pair("prod", "core"));
    assert_eq!(at(20), pair("test", "core"));
    assert_eq!(at(21), pair("test", "assertion"));
}

#[test]
fn python_symbols_nest_and_test_files_are_tests() {
    let tree = tree_of("python");
    index(tree.path()).unwrap();
    let found = symbols(tree.path(), "greeter.py");
    let names: Vec<_> = found
        .iter()
        .map(|(name, kind, parent, first, last)| {
            (
                name.as_str(),
                kind.as_str(),
                parent.as_deref(),
                *first,
                *last,
            )
        })
        .collect();
    assert_eq!(
        names,
        vec![
            ("Greeter", "type", None, 5, 17),
            ("greet", "function", Some("Greeter"), 8, 17),
            ("test_greet", "test", None, 20, 21),
        ]
    );
    // A test file by name: every line is a test line.
    assert_eq!(line(tree.path(), "test_other.py", 1), pair("test", "core"));
}

#[test]
fn references_keep_the_name_as_written_and_the_symbol_they_sit_in() {
    let tree = tree_of("python");
    index(tree.path()).unwrap();
    let connection = crate::common::open(tree.path());
    let (kind, owner): (String, String) = connection
        .query_row(
            "SELECT r.kind, s.name FROM refs r JOIN symbols s ON s.id = r.symbol
             WHERE r.name = 'write'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((kind.as_str(), owner.as_str()), ("call", "greet"));
}

#[test]
fn paths_decide_roles_and_content_can_mark_generated() {
    let tree = tree_of("rust");
    index(tree.path()).unwrap();
    let connection = crate::common::open(tree.path());
    let mut statement = connection
        .prepare("SELECT path, role FROM files ORDER BY path")
        .unwrap();
    let roles: Vec<(String, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let roles: Vec<_> = roles
        .iter()
        .map(|(p, r)| (p.as_str(), r.as_str()))
        .collect();
    assert_eq!(
        roles,
        vec![
            ("Cargo.toml", "config"),
            ("benches/area.rs", "bench"),
            ("examples/demo.rs", "example"),
            ("scripts/run.sh", "script"),
            ("src/generated.rs", "generated"),
            ("src/lib.rs", "prod"),
            ("tests/it.rs", "test"),
            ("vendor/dep.rs", "vendored"),
        ]
    );
}

#[test]
fn other_text_files_get_lines_and_tokens_and_binary_files_only_a_size() {
    let tree = tree_of("text");
    std::fs::write(tree.path().join("blob.bin"), [0_u8, 1, 2, 255]).unwrap();
    index(tree.path()).unwrap();
    let connection = crate::common::open(tree.path());
    let (lines, tokens): (u64, u64) = connection
        .query_row(
            "SELECT count(*), sum(tokens) FROM lines WHERE path = 'data.csv'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(lines, 2);
    assert!(tokens > 0);
    assert_eq!(
        line(tree.path(), "notes/readme.md", 1),
        pair("docs", "core")
    );
    let (size, binary, binary_lines): (u64, bool, u64) = connection
        .query_row(
            "SELECT size_bytes, is_binary, (SELECT count(*) FROM lines WHERE path = 'blob.bin')
             FROM files WHERE path = 'blob.bin'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!((size, binary, binary_lines), (4, true, 0));
}

#[test]
fn empty_source_files_are_indexed_with_no_lines() {
    let tree = tree_of("text");
    std::fs::write(tree.path().join("empty.rs"), "").unwrap();
    std::fs::write(tree.path().join("empty.py"), "").unwrap();
    let report = index(tree.path()).unwrap();
    assert_eq!(report.file_count, 4);
}

#[test]
fn typescript_lines_carry_role_and_concern() {
    let tree = tree_of("typescript");
    index(tree.path()).unwrap();
    let at = |number| kinds(tree.path(), "src/greeter.ts", number);
    assert_eq!(at(1), pair("prod", "comment"));
    assert_eq!(at(2), pair("prod", "import"));
    assert_eq!(at(10), pair("prod", "core"));
    assert_eq!(at(11), pair("prod", "logging"));
    assert_eq!(at(13), pair("prod", "core"));
    // `catch` and `finally` clauses are handling; the log call inside stays logging.
    assert_eq!(at(14), pair("prod", "handler"));
    assert_eq!(at(15), pair("prod", "logging"));
    assert_eq!(at(16), pair("prod", "handler"));
    assert_eq!(at(18), pair("prod", "assertion"));
    assert_eq!(at(20), pair("prod", "core"));
    // The callback of a promise `.catch(..)` is handling; the `.then(..)` before it is not.
    assert_eq!(at(28), pair("prod", "core"));
    assert_eq!(at(29), pair("prod", "handler"));
    assert_eq!(at(30), pair("prod", "logging"));
    assert_eq!(at(31), pair("prod", "handler"));
}

#[test]
fn typescript_symbols_nest_and_test_paths_are_tests() {
    let tree = tree_of("typescript");
    index(tree.path()).unwrap();
    let found = symbols(tree.path(), "src/greeter.ts");
    let found: Vec<_> = found
        .iter()
        .map(|(name, kind, parent, first, last)| {
            (
                name.as_str(),
                kind.as_str(),
                parent.as_deref(),
                *first,
                *last,
            )
        })
        .collect();
    assert_eq!(
        found,
        vec![
            ("Greeting", "type", None, 4, 6),
            ("Greeter", "type", None, 8, 22),
            ("greet", "function", Some("Greeter"), 9, 21),
            ("shout", "function", None, 24, 24),
            ("load", "function", None, 26, 33),
        ]
    );
    // `*.test.ts` and `e2e/` are test files; TSX is parsed with JSX.
    assert_eq!(
        line(tree.path(), "src/greeter.test.ts", 4),
        pair("test", "assertion")
    );
    assert_eq!(line(tree.path(), "e2e/flow.ts", 1), pair("test", "core"));
    assert_eq!(symbols(tree.path(), "src/view.tsx")[0].0, "View");
    assert_eq!(line(tree.path(), "src/view.tsx", 1), pair("prod", "import"));
}

#[test]
fn javascript_lines_carry_role_and_concern() {
    let tree = tree_of("javascript");
    index(tree.path()).unwrap();
    let at = |number| kinds(tree.path(), "util.js", number);
    assert_eq!(at(5), pair("prod", "core"));
    assert_eq!(at(6), pair("prod", "handler"));
    assert_eq!(at(7), pair("prod", "logging"));
    assert_eq!(at(9), pair("prod", "assertion"));
    let found: Vec<_> = symbols(tree.path(), "util.js")
        .into_iter()
        .map(|(name, kind, parent, ..)| (name, kind, parent))
        .collect();
    assert_eq!(
        found,
        vec![
            ("read".to_owned(), "function".to_owned(), None),
            ("Box".to_owned(), "type".to_owned(), None),
            (
                "open".to_owned(),
                "function".to_owned(),
                Some("Box".to_owned())
            ),
        ]
    );
}

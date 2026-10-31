//! The indirection detector fires on wrappers, chains, layers and abstractions with one
//! implementation, and stays quiet on functions that do something and on tests.

use crate::common::{baseline, findings_of, indexed, write};

fn signal(index: &codemap_core::Index, signal: &str) -> Vec<codemap_core::Finding> {
    findings_of(index, "indirection")
        .into_iter()
        .filter(|finding| finding.labels["signal"] == signal)
        .collect()
}

const WORKER: &str = "pub fn load_config(path: &str) -> String {
    let text = path.trim();
    let upper = text.to_uppercase();
    format!(\"{upper}{text}\")
}
";

#[test]
fn a_function_that_only_passes_its_arguments_on_is_a_wrapper() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "src/worker.rs", WORKER);
    write(
        dir.path(),
        "src/api.rs",
        "pub fn read_config(path: &str) -> String {\n    load_config(path)\n}\n",
    );
    let index = indexed(dir.path());
    let found = signal(&index, "pass_through");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].area, "src/api.rs");
    assert_eq!(found[0].measures["chainLength"], 1.0);
    // The wrapper and the function that does the work can both be opened.
    let paths: Vec<_> = found[0].evidence.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["src/api.rs", "src/worker.rs"]);
}

#[test]
fn a_chain_of_wrappers_is_one_finding_in_every_file_it_crosses() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "src/worker.rs", WORKER);
    write(
        dir.path(),
        "src/a.rs",
        "pub fn outer(path: &str) -> String {\n    middle(path)\n}\n",
    );
    write(
        dir.path(),
        "src/b.rs",
        "pub fn middle(path: &str) -> String {\n    inner(path)\n}\n",
    );
    write(
        dir.path(),
        "src/c.rs",
        "pub fn inner(path: &str) -> String {\n    load_config(path)\n}\n",
    );
    let index = indexed(dir.path());
    let found = signal(&index, "pass_through");
    let mut areas: Vec<_> = found.iter().map(|f| f.area.as_str()).collect();
    areas.sort_unstable();
    assert_eq!(areas, ["src/a.rs", "src/b.rs", "src/c.rs"]);
    for finding in &found {
        assert_eq!(finding.measures["chainLength"], 3.0);
        assert_eq!(finding.subject, found[0].subject);
        // Outermost first, the function that does the work last.
        assert_eq!(finding.evidence.len(), 4);
        assert_eq!(finding.evidence[0].path, "src/a.rs");
        assert_eq!(finding.evidence[3].path, "src/worker.rs");
    }
}

#[test]
fn functions_that_do_something_are_not_wrappers() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "src/worker.rs", WORKER);
    write(
        dir.path(),
        "src/api.rs",
        // Transforms its argument, makes two calls, branches, forwards a constant, and wraps a
        // function the codebase does not define.
        "pub fn a(path: &str) -> String {
    load_config(path.trim())
}

pub fn b(path: &str) -> String {
    let first = load_config(path);
    load_config(&first)
}

pub fn c(path: &str, quick: bool) -> String {
    if quick { String::new() } else { load_config(path) }
}

pub fn d(path: &str) -> String {
    load_config(DEFAULT)
}

pub fn e(path: &str) -> String {
    std::fs::read_to_string(path).unwrap()
}

pub fn f(path: &str) -> String {
    external_thing(path)
}

pub struct Holder {
    inner: Inner,
}

impl Holder {
    // Through a field, whose type the names do not tell; and to a method of its own name.
    pub fn config(&self, path: &str) -> String {
        self.inner.load_config(path)
    }

    pub fn load_config(&self, path: &str) -> String {
        self.inner.load_config(path)
    }
}
",
    );
    let index = indexed(dir.path());
    assert!(signal(&index, "pass_through").is_empty());
}

#[test]
fn wrappers_in_tests_and_methods_a_trait_demands_are_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "src/worker.rs", WORKER);
    write(
        dir.path(),
        "tests/helpers.rs",
        "pub fn read_config(path: &str) -> String {\n    load_config(path)\n}\n",
    );
    write(
        dir.path(),
        "src/shown.rs",
        "pub struct Shown;\n\nimpl std::fmt::Display for Shown {\n    fn fmt(&self, out: &mut std::fmt::Formatter) -> std::fmt::Result {\n        render(self, out)\n    }\n}\n\npub fn render(shown: &Shown, out: &mut std::fmt::Formatter) -> std::fmt::Result {\n    let a = 1;\n    let b = a + 1;\n    write!(out, \"{b}\")\n}\n",
    );
    let index = indexed(dir.path());
    assert!(signal(&index, "pass_through").is_empty());
}

#[test]
fn a_directory_of_mostly_wrappers_is_a_thin_layer() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "src/worker.rs", &(0..5).map(|i| format!("pub fn work{i}(x: u32) -> u32 {{\n    let a = x + {i};\n    let b = a * 2;\n    b - x\n}}\n\n")).collect::<String>());
    let facade: String = (0..5)
        .map(|i| format!("pub fn do{i}(x: u32) -> u32 {{\n    work{i}(x)\n}}\n\n"))
        .collect();
    write(dir.path(), "src/facade/api.rs", &facade);
    let index = indexed(dir.path());
    let layers = signal(&index, "thin_layer");
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].subject, "src/facade");
    assert_eq!(layers[0].measures["wrappers"], 5.0);
    assert_eq!(layers[0].measures["share"], 1.0);
}

#[test]
fn a_directory_where_wrappers_are_a_minority_is_not_a_layer() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "src/worker.rs", WORKER);
    let mixed: String = (0..6)
        .map(|i| {
            if i < 2 {
                format!("pub fn w{i}(path: &str) -> String {{\n    load_config(path)\n}}\n\n")
            } else {
                format!("pub fn real{i}(x: u32) -> u32 {{\n    let a = x + {i};\n    let b = a * 2;\n    b - x\n}}\n\n")
            }
        })
        .collect();
    write(dir.path(), "src/mixed/api.rs", &mixed);
    let index = indexed(dir.path());
    assert!(signal(&index, "thin_layer").is_empty());
}

fn implementation(kind: &str) -> Vec<(&'static str, String)> {
    match kind {
        "rust" => vec![
            ("src/store.rs", "pub trait Store {\n    fn put(&self, key: u32);\n}\n".to_owned()),
            ("src/sql.rs", "pub struct SqlStore;\n\nimpl Store for SqlStore {\n    fn put(&self, key: u32) {\n        let _ = key + 1;\n    }\n}\n".to_owned()),
        ],
        "typescript" => vec![
            ("src/store.ts", "export interface Store {\n  put(key: number): void;\n}\n".to_owned()),
            ("src/sql.ts", "export class SqlStore implements Store {\n  put(key: number): void {\n    const next = key + 1;\n    console.log(next);\n  }\n}\n".to_owned()),
        ],
        _ => vec![
            ("src/store.py", "from abc import ABC, abstractmethod\n\nclass Store(ABC):\n    @abstractmethod\n    def put(self, key): ...\n".to_owned()),
            ("src/sql.py", "from store import Store\n\nclass SqlStore(Store):\n    def put(self, key):\n        return key + 1\n".to_owned()),
        ],
    }
}

#[test]
fn an_abstraction_with_one_implementor_is_flagged_in_each_language() {
    for kind in ["rust", "typescript", "python"] {
        let dir = tempfile::tempdir().unwrap();
        baseline(dir.path(), 5);
        for (path, text) in implementation(kind) {
            write(dir.path(), path, &text);
        }
        let index = indexed(dir.path());
        let found = signal(&index, "single_implementation");
        assert_eq!(found.len(), 2, "{kind}");
        assert!(found[0].reason.contains("Store"), "{kind}");
        assert_eq!(found[0].evidence.len(), 2, "{kind}");
        assert_eq!(found[0].measures["implementors"], 1.0);
    }
}

#[test]
fn abstractions_with_none_or_several_implementors_or_only_test_ones_are_quiet() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    for (path, text) in implementation("rust") {
        write(dir.path(), path, &text);
    }
    // A second production implementor.
    write(
        dir.path(),
        "src/mem.rs",
        "pub struct MemStore;\n\nimpl Store for MemStore {\n    fn put(&self, key: u32) {\n        let _ = key;\n    }\n}\n",
    );
    // A trait nobody implements, and one only a test implements.
    write(
        dir.path(),
        "src/unused.rs",
        "pub trait Unused {\n    fn go(&self);\n}\n",
    );
    write(
        dir.path(),
        "src/mock.rs",
        "pub trait Probe {\n    fn go(&self);\n}\n",
    );
    write(
        dir.path(),
        "tests/probe.rs",
        "struct Fake;\n\nimpl Probe for Fake {\n    fn go(&self) {}\n}\n",
    );
    let index = indexed(dir.path());
    assert!(signal(&index, "single_implementation").is_empty());
}

#[test]
fn wrappers_that_forward_round_a_cycle_and_same_named_implementors_are_handled() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(
        dir.path(),
        "src/cycle.rs",
        "pub fn ping(n: u32) -> u32 {\n    pong(n)\n}\n\npub fn pong(n: u32) -> u32 {\n    ping(n)\n}\n",
    );
    for (path, text) in implementation("rust") {
        write(dir.path(), path, &text);
    }
    // A different type of the same name in another module also implements the trait.
    write(
        dir.path(),
        "src/other/sql.rs",
        "pub struct SqlStore;\n\nimpl Store for SqlStore {\n    fn put(&self, key: u32) {\n        let _ = key;\n    }\n}\n",
    );
    let index = indexed(dir.path());
    assert!(signal(&index, "pass_through").is_empty());
    assert!(signal(&index, "single_implementation").is_empty());
}

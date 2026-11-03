//! The indirection detector fires on wrappers, chains and abstractions with one
//! implementation, and stays quiet on functions that do something and on tests.

use codemap_core::SubjectKind;

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
fn a_chain_of_wrappers_is_one_group_whatever_the_files_it_crosses() {
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
    assert_eq!(found.len(), 1);
    let finding = &found[0];
    assert_eq!(finding.subject_kind, SubjectKind::Group);
    assert_eq!(finding.measures["chainLength"], 3.0);
    assert_eq!(finding.measures["files"], 3.0);
    // Outermost first, the function that does the work last.
    assert_eq!(finding.evidence.len(), 4);
    assert_eq!(finding.evidence[0].path, "src/a.rs");
    assert_eq!(finding.evidence[3].path, "src/worker.rs");
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
fn an_inherent_method_of_a_type_that_also_implements_a_trait_is_still_a_wrapper() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(
        dir.path(),
        "src/limit.rs",
        "use std::str::FromStr;\n\npub struct L;\n\nimpl L {\n    fn name(&self) -> &str {\n        let a = \"x\";\n        a\n    }\n    fn third(&self) {\n        let a = 1;\n        drop(a);\n    }\n    fn as_str(&self) -> &str {\n        self.name()\n    }\n    fn other(&self) {\n        self.third()\n    }\n}\n\nimpl FromStr for L {\n    type Err = ();\n    fn from_str(text: &str) -> Result<L, ()> {\n        Ok(L)\n    }\n}\n",
    );
    let index = indexed(dir.path());
    // Both inherent forwarders count; the trait method is dictated by the trait.
    assert_eq!(signal(&index, "pass_through").len(), 2);
}

#[test]
fn a_go_function_that_only_passes_its_arguments_on_is_a_wrapper() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(
        dir.path(),
        "src/worker.go",
        "package src\n\nfunc LoadConfig(path string) string {\n\ttext := strings.TrimSpace(path)\n\tupper := strings.ToUpper(text)\n\treturn upper + text\n}\n",
    );
    write(
        dir.path(),
        "src/api.go",
        "package src\n\nfunc ReadConfig(path string) string {\n\treturn LoadConfig(path)\n}\n\nfunc (c *Client) Fetch(path string) string {\n\tLoadConfig(path)\n}\n\nfunc Shown(path string) string {\n\treturn LoadConfig(path + \"x\")\n}\n\nfunc Send(ch chan string, path string) {\n\tch <- LoadConfig(path)\n}\n\nfunc Spawn(path string) {\n\tgo LoadConfig(path)\n}\n\nfunc Later(path string) {\n\tdefer LoadConfig(path)\n}\n",
    );
    let index = indexed(dir.path());
    let found = signal(&index, "pass_through");
    // `ReadConfig` and `Fetch` forward; `Shown` computes its argument, and sending, spawning
    // or deferring the call is more than passing it on.
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|finding| finding.area == "src/api.go"));
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
        "go" => vec![
            ("src/store.go", "package src\n\ntype Store interface {\n\tPut(key int)\n}\n".to_owned()),
            ("src/sql.go", "package src\n\ntype SqlStore struct {\n\tStore\n}\n\nfunc (s *SqlStore) Put(key int) {\n\tnext := key + 1\n\t_ = next\n}\n".to_owned()),
        ],
        _ => vec![
            ("src/store.py", "from abc import ABC, abstractmethod\n\nclass Store(ABC):\n    @abstractmethod\n    def put(self, key): ...\n".to_owned()),
            ("src/sql.py", "from store import Store\n\nclass SqlStore(Store):\n    def put(self, key):\n        return key + 1\n".to_owned()),
        ],
    }
}

#[test]
fn an_abstraction_with_one_implementor_is_flagged_in_each_language() {
    for kind in ["rust", "typescript", "python", "go"] {
        let dir = tempfile::tempdir().unwrap();
        baseline(dir.path(), 5);
        for (path, text) in implementation(kind) {
            write(dir.path(), path, &text);
        }
        let index = indexed(dir.path());
        let found = signal(&index, "single_implementation");
        assert_eq!(found.len(), 1, "{kind}");
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

#[test]
fn a_call_that_adds_a_string_or_crosses_languages_is_not_a_wrapper() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "src/worker.rs", WORKER);
    write(
        dir.path(),
        "src/api.rs",
        "pub fn read_config(path: &str) -> String {\n    load_config_as(path, \"upper\")\n}\n\npub fn load_config_as(path: &str, mode: &str) -> String {\n    let text = path.trim();\n    format!(\"{mode}{text}\")\n}\n",
    );
    // A script's `load_config(path)` has no script definition, and the Rust one is out of reach.
    write(
        dir.path(),
        "web/api.ts",
        "export function readConfig(path: string) {\n  return load_config(path);\n}\n",
    );
    let index = indexed(dir.path());
    assert!(signal(&index, "pass_through").is_empty());
}

#[test]
fn a_method_of_a_constant_or_of_a_destructured_parameter_is_not_a_forwarded_call() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(
        dir.path(),
        "web/lists.ts",
        "export function includes(list: string[], item: string) {\n  const found = list.indexOf(item);\n  return found >= 0 ? list[found] : undefined;\n}\n",
    );
    write(
        dir.path(),
        "web/model.ts",
        "const KINDS = ['a', 'b'];\n\nexport function isKind(kind: string) {\n  return KINDS.includes(kind);\n}\n\nexport const read = ({ tokens }: Host) => tokens.includes();\n",
    );
    let index = indexed(dir.path());
    assert!(signal(&index, "pass_through").is_empty());
}

#[test]
fn a_call_that_passes_only_literals_or_destructured_names_is_not_a_wrapper() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(
        dir.path(),
        "src/sys.rs",
        "fn create_raw(flags: i32) -> i32 {\n    let a = flags + 1;\n    a * 2\n}\n\nfn open_raw() -> i32 {\n    let a = 1;\n    a * 2\n}\n\npub fn create() -> i32 {\n    create_raw(0)\n}\n\npub fn open() -> i32 {\n    open_raw()\n}\n\npub fn create_with(flags: i32) -> i32 {\n    create_raw(flags)\n}\n",
    );
    write(
        dir.path(),
        "web/ask.ts",
        "export function ask(deriver: string) {\n  const upper = deriver.toUpperCase();\n  return upper + deriver;\n}\n\nexport const read = ({ deriver }: Host) => ask(deriver);\n",
    );
    let index = indexed(dir.path());
    // `open` and `create_with` forward; `create` passes only a literal and `read` only a name
    // picked out of its parameter.
    assert_eq!(signal(&index, "pass_through").len(), 2);
}

#[test]
fn a_type_assertion_with_a_string_type_is_still_a_wrapper() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(
        dir.path(),
        "web/sink.ts",
        "export function sink(kind: string) {\n  const upper = kind.toUpperCase();\n  return upper + kind;\n}\n",
    );
    write(
        dir.path(),
        "web/wrap.ts",
        "export function wrap(kind: string) {\n  return sink(kind as \"a\");\n}\n",
    );
    let index = indexed(dir.path());
    assert_eq!(signal(&index, "pass_through").len(), 1);
}

#[test]
fn an_abstraction_group_keeps_its_id_when_its_implementation_is_replaced() {
    let id_with = |implementor: &str| {
        let dir = tempfile::tempdir().unwrap();
        baseline(dir.path(), 5);
        write(
            dir.path(),
            "src/store.rs",
            "pub trait Store {\n    fn put(&self, key: u32);\n}\n",
        );
        write(
            dir.path(),
            "src/imp.rs",
            &format!(
                "pub struct {implementor};\n\nimpl Store for {implementor} {{\n    fn put(&self, key: u32) {{\n        let _ = key + 1;\n    }}\n}}\n"
            ),
        );
        let index = indexed(dir.path());
        signal(&index, "single_implementation").remove(0).area
    };
    assert_eq!(id_with("SqlStore"), id_with("MemStore"));
}

const SINK: &str = "export function sink(kind: string) {\n  const upper = kind.toUpperCase();\n  return upper + kind;\n}\n";

#[test]
fn functions_that_are_values_in_an_object_literal_are_adapters_not_wrappers() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "web/sink.ts", SINK);
    write(
        dir.path(),
        "web/wire.ts",
        "export function wrap(kind: string) {\n  return sink(kind);\n}\n\nexport const handlers = {\n  onKind: (kind: string) => sink(kind),\n  apply(kind: string) {\n    return sink(kind);\n  },\n};\n",
    );
    let index = indexed(dir.path());
    let found = signal(&index, "pass_through");
    assert_eq!(found.len(), 1);
    assert!(found[0].reason.contains("wrap"), "{}", found[0].reason);
}

#[test]
fn an_interface_in_a_declaration_file_is_not_a_single_implementation() {
    let found_with = |interface_path: &str| {
        let dir = tempfile::tempdir().unwrap();
        baseline(dir.path(), 5);
        write(
            dir.path(),
            interface_path,
            "export interface Pty {\n  write(data: string): void;\n}\n",
        );
        write(
            dir.path(),
            "web/pty.ts",
            "export class LocalPty implements Pty {\n  write(data: string) {\n    console.log(data.trim());\n  }\n}\n",
        );
        signal(&indexed(dir.path()), "single_implementation").len()
    };
    assert_eq!(found_with("web/types.ts"), 1);
    assert_eq!(found_with("web/vendor.d.ts"), 0);
}

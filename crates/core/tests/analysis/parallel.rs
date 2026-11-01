//! The parallel-structures detector fires on naming families and on case sets switched on in
//! several places, and stays quiet on ordinary names and one-off switches.

use codemap_core::SubjectKind;

use crate::common::{baseline, findings_of, indexed, write};

fn signal(index: &codemap_core::Index, signal: &str) -> Vec<codemap_core::Finding> {
    findings_of(index, "parallel")
        .into_iter()
        .filter(|finding| finding.labels["signal"] == signal)
        .collect()
}

fn strukt(name: &str) -> String {
    format!("pub struct {name} {{\n    pub id: u32,\n}}\n")
}

#[test]
fn versions_of_one_name_are_one_family_that_lists_every_file() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "src/order.rs", &strukt("Order"));
    write(dir.path(), "src/order_v2.rs", &strukt("OrderV2"));
    write(dir.path(), "src/legacy.rs", &strukt("LegacyOrder"));
    write(dir.path(), "src/other.rs", &strukt("Invoice"));
    let index = indexed(dir.path());
    let found = signal(&index, "naming_family");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].subject_kind, SubjectKind::Group);
    assert_eq!(found[0].measures["names"], 3.0);
    assert_eq!(found[0].measures["files"], 3.0);
    let mut paths: Vec<_> = found[0].evidence.iter().map(|e| e.path.as_str()).collect();
    paths.sort_unstable();
    assert_eq!(paths, ["src/legacy.rs", "src/order.rs", "src/order_v2.rs"]);
    // Three two-line structs of three lines each.
    assert_eq!(found[0].mass_lines, 9.0);
    assert_eq!(found[0].labels["stem"], "order");
}

#[test]
fn function_versions_in_snake_case_and_a_marker_word_are_families() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(
        dir.path(),
        "src/fns.rs",
        "pub fn parse_input(a: u32) -> u32 {\n    a + 1\n}\n\npub fn parse_input_v1(a: u32) -> u32 {\n    a + 2\n}\n\npub fn render_page(a: u32) -> u32 {\n    a + 3\n}\n\npub fn render_page_new(a: u32) -> u32 {\n    a + 4\n}\n",
    );
    let index = indexed(dir.path());
    let mut stems: Vec<_> = signal(&index, "naming_family")
        .iter()
        .map(|f| f.labels["stem"].clone())
        .collect();
    stems.sort();
    assert_eq!(stems, ["parseinput", "renderpage"]);
}

#[test]
fn ordinary_pairings_and_numbered_dimensions_are_not_families() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    let names = [
        "Store",
        "StoreImpl",
        "StoreBase",
        "StoreBuilder",
        "StoreError",
        "Vec2",
        "Vec3",
        "Sha256",
        "Sha512",
    ];
    for name in names {
        write(dir.path(), &format!("src/{name}.rs"), &strukt(name));
    }
    // Constructors named for the type, and `new` leading a function name.
    write(
        dir.path(),
        "src/ctor.rs",
        "pub fn new_session(a: u32) -> u32 {\n    a + 1\n}\n\npub fn session(a: u32) -> u32 {\n    a + 2\n}\n",
    );
    let index = indexed(dir.path());
    assert!(signal(&index, "naming_family").is_empty());
}

fn dispatch(name: &str, cases: &[&str]) -> String {
    let arms: String = cases
        .iter()
        .enumerate()
        .map(|(i, case)| format!("        {case} => {i},\n"))
        .collect();
    format!("pub fn {name}(kind: Kind) -> u32 {{\n    match kind {{\n{arms}    }}\n}}\n")
}

const KINDS: [&str; 4] = ["Kind::Circle", "Kind::Square", "Kind::Line", "Kind::Dot"];

#[test]
fn the_same_cases_switched_on_in_several_places_are_one_finding() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    for (file, name) in [("area", "area"), ("draw", "draw"), ("name", "label")] {
        write(
            dir.path(),
            &format!("src/{file}.rs"),
            &dispatch(name, &KINDS),
        );
    }
    // Different cases, and a set matched only twice.
    write(
        dir.path(),
        "src/other.rs",
        &dispatch("other", &["Mode::A", "Mode::B", "Mode::C"]),
    );
    write(
        dir.path(),
        "src/other2.rs",
        &dispatch("other2", &["Mode::A", "Mode::B", "Mode::C"]),
    );
    let index = indexed(dir.path());
    let found = signal(&index, "repeated_discriminator");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].measures["files"], 3.0);
    assert_eq!(found[0].measures["sites"], 3.0);
    assert_eq!(found[0].measures["cases"], 4.0);
    assert_eq!(found[0].evidence.len(), 3);
}

#[test]
fn payloads_guards_and_catch_alls_do_not_change_the_case_set() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    let shapes = [
        "pub fn a(k: Kind) -> u32 {\n    match k {\n        Kind::Circle(r) => r,\n        Kind::Square { side } => side,\n        Kind::Line | Kind::Dot => 0,\n        _ => 1,\n    }\n}\n",
        "pub fn b(k: Kind) -> u32 {\n    match k {\n        Kind::Circle(_) => 1,\n        Kind::Square { .. } => 2,\n        Kind::Line => 3,\n        Kind::Dot => 4,\n    }\n}\n",
        "pub fn c(k: Kind, n: u32) -> u32 {\n    match k {\n        Kind::Circle(_) if n > 3 => 1,\n        Kind::Square { .. } => 2,\n        Kind::Line => 3,\n        Kind::Dot => 4,\n        other => 5,\n    }\n}\n",
    ];
    for (i, shape) in shapes.iter().enumerate() {
        write(dir.path(), &format!("src/s{i}.rs"), shape);
    }
    let index = indexed(dir.path());
    let found = signal(&index, "repeated_discriminator");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].measures["sites"], 3.0);
}

#[test]
fn switches_in_other_forms_and_languages_count_and_tests_do_not() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    let ts = |name: &str| {
        format!(
            "export function {name}(kind: string): number {{\n  switch (kind) {{\n    case \"circle\": return 1;\n    case \"square\": return 2;\n    case \"line\": return 3;\n    default: return 0;\n  }}\n}}\n"
        )
    };
    let py = |name: &str| {
        format!(
            "def {name}(kind):\n    if kind == \"circle\":\n        return 1\n    elif kind == \"square\":\n        return 2\n    elif kind == \"line\":\n        return 3\n    return 0\n"
        )
    };
    write(dir.path(), "web/a.ts", &ts("a"));
    write(dir.path(), "web/b.ts", &ts("b"));
    write(dir.path(), "svc/c.py", &py("c"));
    let go = "package svc\n\nfunc d(kind string) int {\n\tswitch kind {\n\tcase \"circle\":\n\t\treturn 1\n\tcase \"square\":\n\t\treturn 2\n\tcase \"line\":\n\t\treturn 3\n\tdefault:\n\t\treturn 0\n\t}\n}\n";
    write(dir.path(), "svc/d.go", go);
    let index = indexed(dir.path());
    // Four sites of one set across a switch and an if chain, in three languages.
    let found = signal(&index, "repeated_discriminator");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].measures["sites"], 4.0);

    let quiet = tempfile::tempdir().unwrap();
    baseline(quiet.path(), 5);
    for name in ["a", "b", "c"] {
        write(quiet.path(), &format!("tests/{name}.ts"), &ts(name));
    }
    let index = indexed(quiet.path());
    assert!(signal(&index, "repeated_discriminator").is_empty());
}

#[test]
fn go_cases_listed_together_or_apart_and_type_switches_are_one_case_set() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    let apart = "package svc\n\nfunc a(v any) int {\n\tswitch v.(type) {\n\tcase int:\n\t\treturn 1\n\tcase string:\n\t\treturn 2\n\tcase bool:\n\t\treturn 3\n\t}\n\treturn 0\n}\n";
    let together = "package svc\n\nfunc b(v any) int {\n\tswitch v.(type) {\n\tcase int, string:\n\t\treturn 1\n\tcase bool:\n\t\treturn 3\n\t}\n\treturn 0\n}\n";
    let named = "package svc\n\nfunc c(v int) int {\n\tswitch v {\n\tcase modeOne, modeTwo:\n\t\treturn 1\n\tcase modeThree:\n\t\treturn 3\n\t}\n\treturn 0\n}\n";
    write(dir.path(), "svc/a.go", apart);
    write(dir.path(), "svc/b.go", together);
    write(dir.path(), "svc/c.go", &apart.replace("func a", "func c"));
    // Constants named in lowercase are cases too, not bindings.
    for name in ["d", "e", "f"] {
        write(
            dir.path(),
            &format!("svc/{name}.go"),
            &named.replace("func c", &format!("func {name}")),
        );
    }
    let index = indexed(dir.path());
    let found = signal(&index, "repeated_discriminator");
    // The type cases and the named constants are two sets of three sites each.
    let sites: Vec<_> = found.iter().map(|f| f.measures["sites"]).collect();
    assert_eq!(sites, [3.0; 2]);
}

#[test]
fn a_bare_number_marks_a_type_beside_its_unnumbered_name_only() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "src/a.rs", &(strukt("Job") + &strukt("Job2")));
    write(
        dir.path(),
        "src/b.rs",
        &(strukt("Sizes8") + &strukt("Sizes16")),
    );
    // Same spelling in another style is not a version.
    write(
        dir.path(),
        "src/c.rs",
        "pub fn Fetch(a: u32) -> u32 {\n    a + 1\n}\n\npub fn _fetch(a: u32) -> u32 {\n    a + 2\n}\n",
    );
    let index = indexed(dir.path());
    let stems: Vec<_> = signal(&index, "naming_family")
        .iter()
        .map(|f| f.labels["stem"].clone())
        .collect();
    assert_eq!(stems, ["job"]);
}

#[test]
fn nested_switches_count_their_lines_once_and_every_site_is_evidence() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    let nested = "pub fn outer(k: Kind) -> u32 {\n    match k {\n        Kind::Circle => match k {\n            Kind::Circle => 1,\n            Kind::Square => 2,\n            Kind::Line => 3,\n        },\n        Kind::Square => 2,\n        Kind::Line => 3,\n    }\n}\n";
    write(dir.path(), "src/n.rs", nested);
    for i in 0..14 {
        write(
            dir.path(),
            &format!("src/f{i:02}.rs"),
            &dispatch("f", &KINDS[..3]),
        );
    }
    let index = indexed(dir.path());
    let found = signal(&index, "repeated_discriminator");
    assert_eq!(found.len(), 1);
    let finding = &found[0];
    // The outer match spans 9 lines and the inner 5 inside it; each other site is 5 lines.
    assert_eq!(finding.mass_lines, 9.0 + 14.0 * 5.0);
    // The outer and inner match of the nested file, and one site in each of the others.
    assert_eq!(finding.evidence.len(), 16);
    assert!(finding.evidence.iter().any(|e| e.path == "src/n.rs"));
    assert!(finding.evidence.iter().any(|e| e.path == "src/f13.rs"));
}

#[test]
fn a_family_of_types_and_one_of_functions_never_share_an_id() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(
        dir.path(),
        "src/a.py",
        "class Function:\n    pass\n\nclass LegacyFunction:\n    pass\n\ndef type(a):\n    return a + 1\n\ndef type_old(a):\n    return a + 2\n",
    );
    let index = indexed(dir.path());
    let found = signal(&index, "naming_family");
    let ids: std::collections::BTreeSet<_> = found.iter().map(|f| f.area.clone()).collect();
    assert_eq!(found.len(), 2);
    assert_eq!(ids.len(), 2);
}

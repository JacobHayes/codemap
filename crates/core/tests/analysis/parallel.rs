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
    let nested = "pub fn outer(k: Kind) -> u32 {\n    match k {\n        Kind::Circle => match k {\n            Kind::Circle => 1,\n            Kind::Square => 2,\n            Kind::Line => 3,\n        },\n        Kind::Square => 2,\n        Kind::Line => 3,\n        _ => 0,\n    }\n}\n";
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
    // The outer match spans 10 lines and the inner 5 inside it; each other site is 5 lines.
    assert_eq!(finding.mass_lines, 10.0 + 14.0 * 5.0);
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

/// A function matching `kind` with one arm per case, each calling its own function.
fn behaviour(name: &str, cases: &[&str], catch_all: Option<&str>) -> String {
    let arms: String = cases
        .iter()
        .enumerate()
        .map(|(i, case)| format!("        {case} => step_{i}(kind, {i} + 1),\n"))
        .chain(catch_all.map(|arm| format!("        {arm},\n")))
        .collect();
    format!("pub fn {name}(kind: Kind) -> u32 {{\n    match kind {{\n{arms}    }}\n}}\n")
}

fn sites(index: &codemap_core::Index) -> Vec<codemap_core::Finding> {
    signal(index, "repeated_discriminator")
}

#[test]
fn exhaustive_behaviour_dispatch_in_many_files_is_not_a_finding() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    for file in ["a", "b", "c", "d"] {
        write(
            dir.path(),
            &format!("src/{file}.rs"),
            &behaviour(file, &KINDS, None),
        );
    }
    assert!(sites(&indexed(dir.path())).is_empty());
}

#[test]
fn a_catch_all_among_behaviour_sites_is_flagged_even_when_it_names_fewer_cases() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "src/a.rs", &behaviour("a", &KINDS, None));
    write(dir.path(), "src/b.rs", &behaviour("b", &KINDS, None));
    for (file, arm) in [
        ("c", "_ => step_9(kind, 1)"),
        ("d", "other => step_9(other, 2)"),
        ("e", "_ => unreachable!()"),
    ] {
        write(
            dir.path(),
            &format!("src/{file}.rs"),
            &behaviour(file, &KINDS[..3], Some(arm)),
        );
    }
    let found = sites(&indexed(dir.path()));
    let wildcards = found[0].measures["wildcardSites"];
    assert_eq!(wildcards, 3.0);
    assert_eq!(found[0].measures["sites"], 5.0);
    assert_eq!(found[0].measures["cases"], 4.0);
    // Only the flagged sites are evidence and mass.
    assert_eq!(found[0].evidence.len(), 3);
    assert!(found.iter().all(|f| f.area != "src/a.rs"));
}

#[test]
fn constant_tables_count_and_a_single_table_does_not() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "src/a.rs", &dispatch("a", &KINDS));
    write(dir.path(), "src/b.rs", &behaviour("b", &KINDS, None));
    write(dir.path(), "src/c.rs", &behaviour("c", &KINDS, None));
    assert!(sites(&indexed(dir.path())).is_empty());
    write(dir.path(), "src/d.rs", &dispatch("d", &KINDS));
    let found = sites(&indexed(dir.path()));
    assert_eq!(found[0].measures["tableSites"], 2.0);
    assert_eq!(found[0].measures["wildcardSites"], 0.0);
}

#[test]
fn one_real_arm_among_identical_trivial_ones_is_degenerate() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    let degenerate = |name: &str| {
        format!(
            "pub fn {name}(kind: Kind) -> Option<u32> {{\n    match kind {{\n        Kind::Circle => Some(compute(kind)),\n        Kind::Square => None,\n        Kind::Line => None,\n        Kind::Dot => None,\n    }}\n}}\n"
        )
    };
    write(dir.path(), "src/a.rs", &degenerate("a"));
    write(dir.path(), "src/b.rs", &degenerate("b"));
    write(dir.path(), "src/c.rs", &behaviour("c", &KINDS, None));
    let found = sites(&indexed(dir.path()));
    assert_eq!(found[0].measures["degenerateSites"], 2.0);
}

#[test]
fn a_set_needs_two_flagged_sites_and_a_set_in_one_file_counts() {
    let catching = |name: &str| behaviour(name, &KINDS, Some("_ => step_9(kind, 1)"));
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    let all: Vec<String> = ["a", "b", "c"]
        .iter()
        .map(|name| behaviour(name, &KINDS, None))
        .collect();
    write(
        dir.path(),
        "src/own.rs",
        &[all[0].clone(), all[1].clone(), catching("c")].join("\n"),
    );
    // One catch-all among behaviour sites is a linter's finding.
    assert!(sites(&indexed(dir.path())).is_empty());
    write(
        dir.path(),
        "src/own.rs",
        &[all[0].clone(), catching("b"), catching("c")].join("\n"),
    );
    let found = sites(&indexed(dir.path()));
    assert_eq!(found[0].measures["files"], 1.0);
    assert_eq!(found[0].measures["wildcardSites"], 2.0);
}

#[test]
fn arms_with_identical_bodies_are_counted_per_site() {
    let repeated = |name: &str| {
        format!(
            "pub fn {name}(k: Kind) -> u32 {{\n    match k {{\n        Kind::Circle => step_a(k, 1),\n        Kind::Square => step_a(k, 1),\n        Kind::Line => step_b(k, 2),\n        Kind::Dot => step_c(k, 3),\n    }}\n}}\n"
        )
    };
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "src/a.rs", &repeated("a"));
    write(dir.path(), "src/b.rs", &repeated("b"));
    write(dir.path(), "src/c.rs", &behaviour("c", &KINDS, None));
    let found = sites(&indexed(dir.path()));
    assert_eq!(found[0].measures["duplicatedSites"], 2.0);
    assert_eq!(found[0].measures["flaggedSites"], 2.0);
}

#[test]
fn catch_alls_are_found_in_every_language() {
    let python = |name: &str, last: &str| {
        format!(
            "def {name}(kind):\n    match kind:\n        case Kind.CIRCLE:\n            return step_a(kind, 1)\n        case Kind.SQUARE:\n            return step_b(kind, 2)\n        case Kind.LINE:\n            return step_c(kind, 3)\n        {last}:\n            return step_d(kind, 4)\n"
        )
    };
    let typescript = |name: &str, last: &str| {
        format!(
            "export function {name}(kind: Kind): number {{\n  switch (kind) {{\n    case Kind.Circle:\n      return stepA(kind, 1);\n    case Kind.Square:\n      return stepB(kind, 2);\n    case Kind.Line:\n      return stepC(kind, 3);\n    {last}:\n      return stepD(kind, 4);\n  }}\n}}\n"
        )
    };
    let go = |name: &str, last: &str| {
        format!(
            "package svc\n\nfunc {name}(kind Kind) int {{\n\tswitch kind {{\n\tcase KindCircle:\n\t\treturn stepA(kind, 1)\n\tcase KindSquare:\n\t\treturn stepB(kind, 2)\n\tcase KindLine:\n\t\treturn stepC(kind, 3)\n\t{last}:\n\t\treturn stepD(kind, 4)\n\t}}\n}}\n"
        )
    };
    // The last arm of each language: a named case, then a catch-all.
    let cases = [
        ("py", "case Kind.DOT", "case _"),
        ("ts", "case Kind.Dot", "default"),
        ("go", "case KindDot", "default"),
    ];
    for (language, named, catch_all) in cases {
        let dir = tempfile::tempdir().unwrap();
        baseline(dir.path(), 5);
        let render = |name: &str, last: &str| match language {
            "py" => python(name, last),
            "ts" => typescript(name, last),
            _ => go(name, last),
        };
        for (file, last) in [
            ("a", named),
            ("b", named),
            ("c", catch_all),
            ("d", catch_all),
        ] {
            write(
                dir.path(),
                &format!("src/{file}.{language}"),
                &render(file, last),
            );
        }
        let found = sites(&indexed(dir.path()));
        assert_eq!(found.len(), 1, "{language}");
        assert_eq!(found[0].measures["wildcardSites"], 2.0, "{language}");
        assert_eq!(found[0].measures["sites"], 4.0, "{language}");
    }
}

#[test]
fn if_chains_over_named_cases_are_tables_or_catch_alls_and_raising_arms_are_traps() {
    let table = |name: &str| {
        format!(
            "export function {name}(kind: Kind): number {{\n  if (kind === Kind.Circle) {{\n    return 1;\n  }} else if (kind === Kind.Square) {{\n    return 2;\n  }} else if (kind === Kind.Line) {{\n    return 3;\n  }}\n  return 0;\n}}\n"
        )
    };
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    for name in ["a", "b", "c"] {
        write(dir.path(), &format!("web/{name}.ts"), &table(name));
    }
    let found = sites(&indexed(dir.path()));
    assert_eq!(found[0].measures["tableSites"], 3.0);

    let python = |name: &str, tail: &str| {
        format!(
            "def {name}(kind):\n    if kind == Kind.CIRCLE:\n        return step_a(kind, 1)\n    elif kind == Kind.SQUARE:\n        return step_b(kind, 2)\n    elif kind == Kind.LINE:\n        return step_c(kind, 3)\n{tail}"
        )
    };
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "svc/a.py", &python("a", ""));
    write(dir.path(), "svc/b.py", &python("b", ""));
    write(
        dir.path(),
        "svc/c.py",
        &python("c", "    else:\n        return step_d(kind, 4)\n"),
    );
    write(
        dir.path(),
        "svc/d.py",
        &python("d", "    else:\n        raise ValueError(kind)\n"),
    );
    let found = sites(&indexed(dir.path()));
    assert_eq!(found[0].measures["wildcardSites"], 2.0);
    assert_eq!(found[0].measures["sites"], 4.0);

    let ts = |name: &str, last: &str| {
        format!(
            "export function {name}(kind: Kind): number {{\n  switch (kind) {{\n    case Kind.Circle:\n      return stepA(kind, 1);\n    case Kind.Square:\n      return stepB(kind, 2);\n    case Kind.Line:\n      return stepC(kind, 3);\n    {last}\n  }}\n}}\n"
        )
    };
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(
        dir.path(),
        "web/a.ts",
        &ts("a", "case Kind.Dot: return stepD(kind, 4);"),
    );
    write(
        dir.path(),
        "web/b.ts",
        &ts("b", "case Kind.Dot: return stepD(kind, 4);"),
    );
    write(
        dir.path(),
        "web/c.ts",
        &ts("c", "case Kind.Dot: throw new Error(\"no\");"),
    );
    write(
        dir.path(),
        "web/d.ts",
        &ts("d", "case Kind.Dot: throw new Error(\"no\");"),
    );
    let found = sites(&indexed(dir.path()));
    assert_eq!(found[0].measures["wildcardSites"], 2.0);
}

#[test]
fn bodies_that_compute_are_not_constants() {
    // An `@` in the body becomes the arm's number, so no two arms repeat each other.
    let ts = |name: &str, body: &str| {
        let [b1, b2, b3] = ["1", "2", "3"].map(|n| body.replace('@', n));
        format!(
            "export function {name}(kind: Kind) {{\n  switch (kind) {{\n    case Kind.Circle:\n      return {b1};\n    case Kind.Square:\n      return {b2};\n    case Kind.Line:\n      return {b3};\n  }}\n}}\n"
        )
    };
    let go = |name: &str| {
        format!(
            "package svc\n\nfunc {name}(kind Kind) int {{\n\tswitch kind {{\n\tcase KindCircle:\n\t\treturn ComputeA(kind)\n\tcase KindSquare:\n\t\treturn ComputeB(kind)\n\tcase KindLine:\n\t\treturn ComputeC(kind)\n\t}}\n\treturn 0\n}}\n"
        )
    };
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    for (file, body) in [
        ("a", "`x${compute@(kind)}`"),
        ("b", "++counter@"),
        ("c", "delete table@.id"),
    ] {
        write(dir.path(), &format!("web/{file}.ts"), &ts(file, body));
    }
    for name in ["d", "e", "f"] {
        write(dir.path(), &format!("svc/{name}.go"), &go(name));
    }
    assert!(sites(&indexed(dir.path())).is_empty());
    // Plain literals, a sign and a string with spaces are constants.
    let quiet = tempfile::tempdir().unwrap();
    baseline(quiet.path(), 5);
    for (file, body) in [("a", "-1"), ("b", "\"a b\""), ("c", "`plain`")] {
        write(quiet.path(), &format!("web/{file}.ts"), &ts(file, body));
    }
    let found = sites(&indexed(quiet.path()));
    assert_eq!(found[0].measures["tableSites"], 3.0);
}

#[test]
fn a_comment_beside_an_aborting_arm_does_not_hide_it() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "src/a.rs", &behaviour("a", &KINDS, None));
    write(dir.path(), "src/b.rs", &behaviour("b", &KINDS, None));
    let trap = behaviour("c", &KINDS[..3], None).replace(
        "    }\n}\n",
        "        Kind::Dot => {\n            // never built\n            unreachable!()\n        }\n    }\n}\n",
    );
    write(dir.path(), "src/c.rs", &trap);
    write(dir.path(), "src/d.rs", &trap.replace("fn c(", "fn d("));
    let found = sites(&indexed(dir.path()));
    assert_eq!(found[0].measures["wildcardSites"], 2.0);
}

#[test]
fn qualified_paths_nested_alternatives_and_empty_go_cases_keep_their_shape() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    // A qualified constant is not a bound name; a bar inside a payload is not a catch-all.
    for name in ["a", "b", "c"] {
        write(
            dir.path(),
            &format!("src/{name}.rs"),
            &format!(
                "pub fn {name}(k: schema::Kind) -> Option<bool> {{\n    match k {{\n        schema::Kind::A => schema::FIRST,\n        schema::Kind::B => schema::SECOND,\n        schema::Kind::C(Some(true | false)) => schema::THIRD,\n    }}\n}}\n"
            ),
        );
    }
    let found = sites(&indexed(dir.path()));
    assert_eq!(found[0].measures["tableSites"], 3.0);
    assert_eq!(found[0].measures["wildcardSites"], 0.0);

    // Empty Go cases end the switch: two of them and one real case are a degenerate site.
    let quiet = tempfile::tempdir().unwrap();
    baseline(quiet.path(), 5);
    for name in ["d", "e", "f"] {
        write(
            quiet.path(),
            &format!("svc/{name}.go"),
            &format!(
                "package svc\n\nfunc {name}(kind Kind) {{\n\tswitch kind {{\n\tcase KindCircle:\n\tcase KindSquare:\n\tcase KindLine:\n\t\tcompute(kind)\n\t}}\n}}\n"
            ),
        );
    }
    let found = sites(&indexed(quiet.path()));
    assert_eq!(found[0].measures["degenerateSites"], 3.0);
}

#[test]
fn negative_constants_count_and_increments_and_comments_do_not_confuse_the_bodies() {
    let go = |name: &str, counter: &str| {
        format!(
            "package svc\n\nfunc {name}(kind Kind) {{\n\tswitch kind {{\n\tcase KindCircle:\n\t\t{counter}1++\n\tcase KindSquare:\n\t\t{counter}2++\n\tcase KindLine:\n\t\t{counter}3++\n\t}}\n}}\n"
        )
    };
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    for (name, counter) in [("a", "x"), ("b", "y"), ("c", "z")] {
        write(dir.path(), &format!("svc/{name}.go"), &go(name, counter));
    }
    assert!(sites(&indexed(dir.path())).is_empty());

    let python = |name: &str| {
        format!(
            "def {name}(kind):\n    match kind:\n        case Kind.CIRCLE:\n            return -1\n        case Kind.SQUARE:\n            return -2\n        case Kind.LINE:\n            return -3\n"
        )
    };
    let quiet = tempfile::tempdir().unwrap();
    baseline(quiet.path(), 5);
    for name in ["a", "b", "c"] {
        write(quiet.path(), &format!("svc/{name}.py"), &python(name));
    }
    assert_eq!(sites(&indexed(quiet.path()))[0].measures["tableSites"], 3.0);

    // A comment inside one of the identical arms does not make it a different body.
    let rust = |name: &str, none: &str| {
        format!(
            "pub fn {name}(k: Kind) -> Option<u32> {{\n    match k {{\n        Kind::Circle => Some(compute(k)),\n        Kind::Square => {none},\n        Kind::Line => None,\n        Kind::Dot => None,\n    }}\n}}\n"
        )
    };
    let commented = tempfile::tempdir().unwrap();
    baseline(commented.path(), 5);
    for name in ["a", "b", "c"] {
        write(
            commented.path(),
            &format!("src/{name}.rs"),
            &rust(name, "{ /* unused */ None }"),
        );
    }
    let found = sites(&indexed(commented.path()));
    assert_eq!(found[0].measures["degenerateSites"], 3.0);
}

#[test]
fn guards_conditional_values_and_distinct_strings_are_not_constant_tables() {
    let python = |name: &str, arm: &str| {
        format!(
            "def {name}(kind):\n    match kind:\n        case Kind.CIRCLE{arm}\n        case Kind.SQUARE:\n            return 2\n        case Kind.LINE:\n            return 3\n"
        )
    };
    for arm in [
        " if check(kind):\n            return 1",
        ":\n            return 1 if enabled else 0",
    ] {
        let dir = tempfile::tempdir().unwrap();
        baseline(dir.path(), 5);
        for name in ["a", "b", "c"] {
            write(dir.path(), &format!("svc/{name}.py"), &python(name, arm));
        }
        let found = sites(&indexed(dir.path()));
        assert!(
            found.is_empty() || found[0].measures["tableSites"] == 0.0,
            "{arm}"
        );
    }

    let rust = |name: &str| {
        format!(
            "pub fn {name}(k: Kind) -> &'static str {{\n    match k {{\n        Kind::Circle => compute(k),\n        Kind::Square => \"x\",\n        Kind::Line => \" x \",\n    }}\n}}\n"
        )
    };
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    for name in ["a", "b", "c"] {
        write(dir.path(), &format!("src/{name}.rs"), &rust(name));
    }
    assert!(sites(&indexed(dir.path())).is_empty());
}

#[test]
fn tuple_matches_are_sites_of_the_enum_in_each_position_that_varies() {
    // `(field, payload)`: the field position tells four cases apart, the payload two. Arms for
    // the two payloads repeat each other's bodies.
    let tuple = |name: &str| {
        format!(
            "pub fn {name}(f: Field, p: Payload) -> Option<u32> {{\n    match (f, p) {{\n        (Field::A, Payload::X(w)) => Some(w.a),\n        (Field::A, Payload::Y(w)) => Some(w.a),\n        (Field::B, Payload::X(w)) => Some(w.b),\n        (Field::B, Payload::Y(w)) => Some(w.b),\n        (Field::C, Payload::X(w)) => Some(w.c),\n        (Field::C, Payload::Y(w)) => Some(w.c),\n        _ => None,\n    }}\n}}\n"
        )
    };
    let payload = |name: &str| {
        format!(
            "pub fn {name}(p: Payload) -> u32 {{\n    match p {{\n        Payload::X(w) => step_a(w, 1),\n        Payload::Y(w) => step_b(w, 2),\n        Payload::Z => step_c(3),\n    }}\n}}\n"
        )
    };
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 5);
    write(dir.path(), "src/a.rs", &tuple("a"));
    write(dir.path(), "src/b.rs", &tuple("b"));
    write(dir.path(), "src/d.rs", &tuple("d"));
    write(dir.path(), "src/c.rs", &payload("c"));
    let found = sites(&indexed(dir.path()));
    // Field: the three tuple sites. Payload: those three, which join by their two payloads,
    // and the plain match.
    let sizes: Vec<_> = found
        .iter()
        .map(|f| (f.measures["cases"], f.measures["sites"]))
        .collect();
    assert_eq!(found.len(), 2, "{sizes:?}");
    for finding in &found {
        assert_eq!(finding.measures["duplicatedSites"], 3.0);
        assert_eq!(finding.measures["wildcardSites"], 3.0);
    }
}

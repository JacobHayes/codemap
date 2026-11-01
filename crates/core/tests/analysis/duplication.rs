//! The duplication detector fires on copies, in production code and in tests, and stays quiet on
//! code that only has the same shape.

use codemap_core::{Answer, Filter, Finding, Hierarchy, MeasureRef, Query, SubjectKind};

use crate::common::{areas, ask, baseline, indexed, write};
use crate::similarity::total;

fn duplication_in(index: &codemap_core::Index, area: &str) -> Vec<Finding> {
    match ask(
        index,
        &Query::Area {
            subject: area.to_owned(),
        },
    ) {
        Answer::Area(detail) => detail
            .findings
            .into_iter()
            .filter(|finding| finding.detector == "duplication")
            .collect(),
        other => panic!("not an area: {other:?}"),
    }
}

fn all_duplication(index: &codemap_core::Index) -> Vec<Finding> {
    areas(index)
        .areas
        .iter()
        .flat_map(|area| duplication_in(index, &area.subject))
        .collect()
}

#[test]
fn copies_are_one_group_that_every_file_with_a_member_points_at() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 20);
    write(dir.path(), "a/one.rs", &total("one", "items", "sum", "1.5"));
    write(dir.path(), "b/two.rs", &total("two", "items", "sum", "1.5"));
    // Two copies in one file, with another name each: still one group of four.
    let both = format!(
        "{}\n{}",
        total("three", "items", "sum", "1.5"),
        total("four", "items", "sum", "1.5")
    );
    write(dir.path(), "c/three.rs", &both);
    let index = indexed(dir.path());

    for area in ["a/one.rs", "b/two.rs", "c/three.rs"] {
        let found = duplication_in(&index, area);
        assert_eq!(found.len(), 1, "{area}");
        let finding = &found[0];
        assert_eq!(finding.subject_kind, SubjectKind::Group);
        assert_eq!(finding.area, area);
        assert_eq!(finding.measures["members"], 4.0);
        assert_eq!(finding.measures["files"], 3.0);
        assert_eq!(finding.measures["totalLines"], 4.0 * 9.0);
        assert_eq!(finding.measures["duplicatedLines"], 3.0 * 9.0);
        assert_eq!(finding.labels["kind"], "function");
        // Every copy can be opened from any of the areas.
        assert_eq!(finding.evidence.len(), 4);
        assert!(finding.evidence.iter().any(|e| e.path == "a/one.rs"));
    }
    assert_eq!(all_duplication(&index).len(), 3);
    // The file with two copies carries both in its mass.
    let area = areas(&index)
        .areas
        .into_iter()
        .find(|area| area.subject == "c/three.rs")
        .unwrap();
    assert_eq!(area.mass_lines, 18.0);
}

#[test]
fn go_copies_under_other_names_are_a_group() {
    let go = |name: &str, items: &str, sum: &str| {
        format!(
            "package pkg\n\nfunc {name}({items} []Item) float64 {{\n\t{sum} := 0.0\n\tfor _, item := range {items} {{\n\t\tif item.Active {{\n\t\t\t{sum} += item.Price * float64(item.Count)\n\t\t}}\n\t}}\n\treturn {sum} * 1.5\n}}\n"
        )
    };
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 20);
    write(dir.path(), "a/one.go", &go("one", "items", "sum"));
    write(dir.path(), "b/two.go", &go("two", "rows", "acc"));
    write(dir.path(), "c/three.go", &go("three", "list", "total"));
    let index = indexed(dir.path());
    let found = duplication_in(&index, "a/one.go");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].measures["members"], 3.0);
    assert_eq!(found[0].labels["kind"], "function");
}

fn indexed_copies() -> (tempfile::TempDir, codemap_core::Index) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.rs", &total("total", "items", "sum", "1.5"));
    write(dir.path(), "b.rs", &total("total", "items", "sum", "1.5"));
    write(dir.path(), "c.rs", &total("total", "items", "sum", "1.5"));
    let index = indexed(dir.path());
    (dir, index)
}

#[test]
fn renamed_copies_group_as_near_and_say_so() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.rs", &total("total", "items", "sum", "1.5"));
    write(dir.path(), "b.rs", &total("cost", "lines", "acc", "2.0"));
    write(dir.path(), "c.rs", &total("sum_up", "rows", "tot", "3.0"));
    let index = indexed(dir.path());
    let found = duplication_in(&index, "a.rs");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].labels["match"], "near");
    assert!(found[0].measures["similarity"] >= 0.8);
    let (_dir, copies) = indexed_copies();
    let exact = duplication_in(&copies, "a.rs");
    assert_eq!(exact[0].labels["match"], "exact");
}

#[test]
fn copies_in_test_code_use_the_same_engine_and_are_labelled_tests() {
    let test = |name: &str| {
        format!(
            "#[test]\nfn {name}() {{\n    let mut cart = Cart::new();\n    cart.add(Item::new(\"pen\", 2));\n    cart.add(Item::new(\"ink\", 3));\n    cart.remove(\"pen\");\n    assert_eq!(cart.len(), 1);\n    assert_eq!(cart.total(), 3);\n}}\n"
        )
    };
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "tests/a.rs", &test("removes_one"));
    write(dir.path(), "tests/b.rs", &test("drops_one"));
    write(dir.path(), "tests/c.rs", &test("loses_one"));
    let index = indexed(dir.path());
    let found = duplication_in(&index, "tests/a.rs");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].labels["kind"], "test");
}

#[test]
fn every_file_of_a_group_gets_an_area() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 20);
    write(dir.path(), "a/one.rs", &total("one", "items", "sum", "1.5"));
    write(dir.path(), "b/two.rs", &total("two", "items", "sum", "1.5"));
    write(
        dir.path(),
        "c/three.rs",
        &total("three", "items", "sum", "1.5"),
    );
    let index = indexed(dir.path());
    let listed = areas(&index);
    let area = listed
        .areas
        .iter()
        .find(|a| a.subject == "a/one.rs")
        .unwrap();
    assert_eq!(area.detectors, ["duplication"]);
    assert_eq!(
        listed
            .areas
            .iter()
            .filter(|a| a.detectors.contains(&"duplication".to_owned()))
            .count(),
        3
    );
}

#[test]
fn a_small_group_is_paired_but_does_not_become_an_area() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 20);
    // Two copies of a nine-line function duplicate fewer lines than an area is worth.
    write(dir.path(), "a.rs", &total("one", "items", "sum", "1.5"));
    write(dir.path(), "b.rs", &total("two", "items", "sum", "1.5"));
    let index = indexed(dir.path());
    assert!(all_duplication(&index).is_empty());
    assert_eq!(crate::similarity::pair_count(dir.path()), 1);
}

#[test]
fn similar_shapes_with_different_logic_and_tiny_functions_are_quiet() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 20);
    // Same signature, loop and size, different operators and control flow.
    write(
        dir.path(),
        "a.rs",
        "fn scan(items: &[Item]) -> u32 {\n    let mut n = 0;\n    for item in items {\n        if item.price > 3 {\n            n += 1;\n        }\n    }\n    n\n}\n",
    );
    write(
        dir.path(),
        "b.rs",
        "fn scan(items: &[Item]) -> u32 {\n    let mut n = 0;\n    let mut i = items.len();\n    while i > 0 {\n        n = n * 2 - item_at(items, i);\n        i -= 1;\n    }\n    n\n}\n",
    );
    let index = indexed(dir.path());
    assert!(all_duplication(&index).is_empty());
}

#[test]
fn duplicated_lines_count_every_copy_but_the_longest_and_can_colour_the_map() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 20);
    for name in ["a", "b", "c"] {
        write(
            dir.path(),
            &format!("src/{name}.rs"),
            &total(name, "items", "sum", "1.5"),
        );
    }
    let index = indexed(dir.path());
    let measure = |name: &str| {
        let query = Query::Tree {
            hierarchy: Hierarchy::Path,
            size: Some(MeasureRef::Named(name.to_owned())),
            color: None,
            filter: Filter::default(),
            depth: None,
        };
        match ask(&index, &query) {
            Answer::Tree(tree) => tree.total,
            other => panic!("not a tree: {other:?}"),
        }
    };
    assert_eq!(measure("duplicated_lines"), 2.0 * 9.0);
    assert!(measure("lines") > measure("duplicated_lines"));
}

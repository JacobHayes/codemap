//! The duplication detector fires on copies, in production code and in tests, and stays quiet on
//! code that only has the same shape.

use codemap_core::{Answer, Filter, Finding, Hierarchy, MeasureRef, Query, SubjectKind};

use crate::common::{areas, ask, baseline, indexed, write};
use crate::similarity::total;

fn detail(index: &codemap_core::Index, subject: &str) -> codemap_core::AreaDetail {
    match ask(
        index,
        &Query::Area {
            subject: subject.to_owned(),
        },
    ) {
        Answer::Area(detail) => detail,
        other => panic!("not an area: {other:?}"),
    }
}

/// One finding per duplicate group, whatever number of files it spans.
fn all_duplication(index: &codemap_core::Index) -> Vec<Finding> {
    areas(index)
        .areas
        .iter()
        .flat_map(|area| detail(index, &area.subject).findings)
        .filter(|finding| finding.detector == "duplication")
        .collect()
}

#[test]
fn copies_are_one_group_area_that_lists_every_copy() {
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

    let found = all_duplication(&index);
    assert_eq!(found.len(), 1);
    let finding = &found[0];
    assert_eq!(finding.subject_kind, SubjectKind::Group);
    assert_eq!(finding.measures["members"], 4.0);
    assert_eq!(finding.measures["files"], 3.0);
    assert_eq!(finding.measures["totalLines"], 4.0 * 9.0);
    assert_eq!(finding.measures["duplicatedLines"], 3.0 * 9.0);
    assert_eq!(finding.labels["kind"], "function");
    // Every copy can be opened from the one area.
    assert_eq!(finding.evidence.len(), 4);
    for file in ["a/one.rs", "b/two.rs", "c/three.rs"] {
        assert!(finding.evidence.iter().any(|e| e.path == file));
    }
    // The area's mass is the copies' lines counted once, not the files' lines.
    let group = areas(&index)
        .areas
        .into_iter()
        .find(|area| area.subject == finding.area)
        .unwrap();
    assert_eq!(group.subject_kind, SubjectKind::Group);
    assert_eq!(group.mass_lines, 4.0 * 9.0);
    assert!(group.title.contains("3 files"));
}

#[test]
fn a_file_lists_the_groups_it_belongs_to_with_its_share() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 20);
    write(dir.path(), "a/one.rs", &total("one", "items", "sum", "1.5"));
    write(dir.path(), "b/two.rs", &total("two", "items", "sum", "1.5"));
    let both = format!(
        "{}\n{}",
        total("three", "items", "sum", "1.5"),
        total("four", "items", "sum", "1.5")
    );
    write(dir.path(), "c/three.rs", &both);
    let index = indexed(dir.path());
    let group = all_duplication(&index).remove(0).area;

    let one = detail(&index, "a/one.rs");
    assert_eq!(one.groups.len(), 1);
    assert_eq!(one.groups[0].group.subject, group);
    assert_eq!(one.groups[0].lines, 9);
    assert!((one.groups[0].share - 0.25).abs() < 1e-9);
    // A directory sums its files; the file with two copies holds half the group.
    assert!((detail(&index, "c").groups[0].share - 0.5).abs() < 1e-9);
    assert_eq!(detail(&index, "").groups.len(), 1);
    // The group points back at the areas of its files.
    assert!(detail(&index, &group).groups.is_empty());
}

#[test]
fn a_group_survives_a_filter_when_any_member_matches() {
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
    let listed = |path: Option<&str>, role: Option<&str>| {
        let filter = Filter {
            path: path.map(str::to_owned),
            role: role.map(str::to_owned),
            ..Filter::default()
        };
        match ask(
            &index,
            &Query::Areas {
                top: None,
                detector: Some("duplication".to_owned()),
                filter,
            },
        ) {
            Answer::Areas(list) => list.areas.len(),
            other => panic!("not areas: {other:?}"),
        }
    };
    assert_eq!(listed(None, None), 1);
    assert_eq!(listed(Some("b"), None), 1);
    assert_eq!(listed(Some("elsewhere"), None), 0);
    assert_eq!(listed(None, Some("prod")), 1);
    assert_eq!(listed(None, Some("test")), 0);
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
    let found = all_duplication(&index);
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
    let found = all_duplication(&index);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].labels["match"], "near");
    assert!(found[0].measures["similarity"] >= 0.8);
    let (_dir, copies) = indexed_copies();
    let exact = all_duplication(&copies);
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
    let found = all_duplication(&index);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].labels["kind"], "test");
}

#[test]
fn a_small_near_group_is_paired_but_does_not_become_an_area() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 20);
    // Two near copies of a nine-line function duplicate fewer lines than an area is worth.
    write(dir.path(), "a.rs", &total("one", "items", "sum", "1.5"));
    write(dir.path(), "b.rs", &total("two", "rows", "acc", "2.0"));
    let index = indexed(dir.path());
    assert!(all_duplication(&index).is_empty());
    assert_eq!(crate::similarity::pair_count(dir.path()), 1);
}

#[test]
fn a_small_exact_pair_is_an_area() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 20);
    write(dir.path(), "a.rs", &total("total", "items", "sum", "1.5"));
    write(dir.path(), "b.rs", &total("total", "items", "sum", "1.5"));
    let index = indexed(dir.path());
    let found = all_duplication(&index);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].labels["match"], "exact");
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

fn copies_in(dir: &std::path::Path, files: &[&str]) -> codemap_core::Index {
    baseline(dir, 20);
    for file in files {
        write(dir, file, &total("one", "items", "sum", "1.5"));
    }
    indexed(dir)
}

#[test]
fn a_group_is_acknowledged_by_its_id_or_by_a_path_that_holds_all_its_members() {
    let dir = tempfile::tempdir().unwrap();
    let files = ["a/one.rs", "a/two.rs", "a/x/three.rs"];
    let id = all_duplication(&copies_in(dir.path(), &files))
        .remove(0)
        .area;
    let acknowledged = |config: &str| {
        write(dir.path(), ".codemap/config.toml", config);
        areas(&indexed(dir.path()))
            .areas
            .into_iter()
            .find(|area| area.subject == id)
            .unwrap()
            .acknowledged
    };
    assert_eq!(
        acknowledged(&format!(
            "[[acknowledge]]\ngroup = \"{id}\"\nreason = \"by id\"\n"
        )),
        Some("by id".to_owned())
    );
    // A path covers a group only when every member is under it.
    assert_eq!(
        acknowledged("[[acknowledge]]\npath = \"a/x\"\nreason = \"x\"\n"),
        None
    );
    assert_eq!(
        acknowledged("[[acknowledge]]\npath = \"a\"\nreason = \"all\"\n"),
        Some("all".to_owned())
    );
}

#[test]
fn an_acknowledgement_follows_a_group_while_it_is_worked_on() {
    let dir = tempfile::tempdir().unwrap();
    let files = ["a/one.rs", "b/two.rs", "c/three.rs", "d/four.rs"];
    let before = all_duplication(&copies_in(dir.path(), &files)).remove(0);
    let id = before.area.clone();
    let members = before.labels["members"].replace('\n', "\", \"");
    let config = format!(
        "[[acknowledge]]\ngroup = \"{id}\"\nmembers = [\"{members}\"]\nreason = \"kept on purpose\"\n"
    );
    write(dir.path(), ".codemap/config.toml", &config);
    let state = |index: &codemap_core::Index| {
        areas(index)
            .areas
            .into_iter()
            .filter(|a| a.subject_kind == SubjectKind::Group)
            .map(|a| (a.subject, a.acknowledged))
            .collect::<Vec<_>>()
    };
    let found = state(&indexed(dir.path()));
    assert_eq!(found, [(id.clone(), Some("kept on purpose".to_owned()))]);

    // The longest copy is edited: the group is named by another hash now, and still acknowledged.
    write(
        dir.path(),
        "a/one.rs",
        &total("one", "items", "sum", "1.5").replace("sum * 1.5", "sum * 1.5 + 1.0"),
    );
    let found = state(&indexed(dir.path()));
    assert_eq!(found.len(), 1);
    assert_ne!(found[0].0, id);
    assert_eq!(found[0].1.as_deref(), Some("kept on purpose"));

    // A near copy drops out: three of the four members remain.
    std::fs::remove_file(dir.path().join("d/four.rs")).unwrap();
    let found = state(&indexed(dir.path()));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].1.as_deref(), Some("kept on purpose"));

    // Nothing shares half its members with the acknowledged set any more: the group is listed
    // again, unacknowledged.
    write(dir.path(), "c/three.rs", "fn other() {}\n");
    write(dir.path(), "b/two.rs", "fn another() {}\n");
    assert!(
        state(&indexed(dir.path()))
            .iter()
            .all(|(_, acknowledged)| acknowledged.is_none())
    );
}
#[test]
fn same_named_methods_of_different_clusters_are_different_groups() {
    let first = |class: &str| {
        format!(
            "class {class}:\n    def run(self, items):\n        total = 0\n        seen = 0\n        for item in items:\n            if item.active:\n                total += item.price * item.count\n                seen += 1\n            else:\n                seen -= 1\n        if seen > 3:\n            total = total * 2\n        return total\n"
        )
    };
    let second = |class: &str| {
        format!(
            "class {class}:\n    def run(self, path):\n        lines = []\n        with open(path) as handle:\n            while True:\n                row = handle.readline()\n                if not row:\n                    break\n                lines.append(row.strip().upper())\n        names = sorted(set(lines))\n        joined = \",\".join(names)\n        print(joined, len(names))\n        return names\n"
        )
    };
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 20);
    // Two clusters of `run` in one file: A and B copy each other, and so do C and D.
    write(
        dir.path(),
        "src/ops.py",
        &[first("A"), first("B"), second("C"), second("D")].concat(),
    );
    let both = all_duplication(&indexed(dir.path()));
    let ids: std::collections::BTreeSet<_> = both.iter().map(|f| f.area.clone()).collect();
    assert_eq!(both.len(), 2);
    assert_eq!(ids.len(), 2);
}

#[test]
fn a_closure_copied_with_its_enclosing_function_is_not_a_group_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 20);
    let source = "export function debounce(fn, wait) {\n  let timer = null;\n  let last = 0;\n  const flush = function () {\n    if (timer === null) {\n      return;\n    }\n    clearTimeout(timer);\n    timer = null;\n    last = Date.now();\n    fn(last);\n  };\n  const ret = function (value) {\n    flush();\n    timer = setTimeout(() => fn(value), wait);\n  };\n  return ret;\n}\n";
    write(dir.path(), "a/utils.ts", source);
    write(dir.path(), "b/utils.ts", source);
    let index = indexed(dir.path());

    let found = all_duplication(&index);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].measures["members"], 2.0);
    assert_eq!(found[0].measures["largestLines"], 18.0);
}

#[test]
fn a_function_sharing_a_line_with_the_one_before_it_is_not_nested_in_it() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 20);
    let long = "export function longer(a) {\n  let sum = 0;\n  for (const x of a) {\n    if (x > 3) {\n      sum += x * 2;\n    } else {\n      sum -= 1;\n    }\n  }\n  return sum;\n}";
    let short = "function shorter(items) {\n  const out = [];\n  for (const item of items) {\n    out.push(item.name);\n    out.push(item.id);\n  }\n  return out;\n}\n";
    for file in ["a/one.ts", "b/two.ts"] {
        write(dir.path(), file, &format!("{long} {short}"));
    }
    let index = indexed(dir.path());

    assert_eq!(all_duplication(&index).len(), 2);
}

//! The query layer through its public function: filters, measures, tree shaping and the envelope.

use codemap_core::{
    Answer, Base, Envelope, Filter, Hierarchy, Measure, MeasureRef, Query, QueryError, Tree, answer,
};

use crate::common::{ask, indexed, plain_functions, write};

fn tree_of(index: &codemap_core::Index, query: &Query) -> Tree {
    match ask(index, query) {
        Answer::Tree(tree) => tree,
        other => panic!("not a tree: {other:?}"),
    }
}

fn tree_with(
    index: &codemap_core::Index,
    filter: Filter,
    color: Option<&str>,
    depth: Option<u32>,
) -> Tree {
    tree_of(index, &tree_query(filter, color, depth))
}

fn tree_query(filter: Filter, color: Option<&str>, depth: Option<u32>) -> Query {
    Query::Tree {
        hierarchy: Hierarchy::Path,
        size: None,
        color: color.map(|name| MeasureRef::Named(name.to_owned())),
        filter,
        depth,
    }
}

/// Sizes and test lines vary by file so colour steps have something to separate.
fn mixed_tree(root: &std::path::Path) {
    for i in 1..=8 {
        write(
            root,
            &format!("src/a/b/c/f{i}.rs"),
            &plain_functions(&format!("f{i}_"), i * i),
        );
    }
    write(root, "src/top.rs", &plain_functions("top", 5));
    write(root, "tests/t.rs", &plain_functions("t", 4));
    write(root, "notes.md", "one\ntwo\n");
}

#[test]
fn the_root_total_is_every_line_and_parents_precede_children() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    let index = indexed(dir.path());
    let Answer::Overview(overview) = ask(&index, &Query::Overview) else {
        panic!()
    };
    let tree = tree_with(&index, Filter::default(), None, Some(10));
    assert_eq!(tree.total as u64, overview.totals.lines);
    assert_eq!(
        tree.nodes.iter().map(|n| n.size).sum::<f64>() as u64,
        overview.totals.lines
    );
    for node in &tree.nodes {
        assert!(node.parent.is_none_or(|p| p < node.id));
        assert_eq!(tree.nodes[node.id as usize].id, node.id);
    }
    assert_eq!(tree.size.unit, "lines");
}

#[test]
fn depth_aggregates_deeper_mass_into_the_deepest_drawn_ancestor() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    let index = indexed(dir.path());
    let deep = tree_with(
        &index,
        Filter::default(),
        Some("test_to_prod_ratio"),
        Some(10),
    );
    let shallow = tree_with(
        &index,
        Filter::default(),
        Some("test_to_prod_ratio"),
        Some(2),
    );
    assert_eq!(deep.total, shallow.total);
    let depth_of = |tree: &Tree, mut id: u32| {
        let mut depth = 0;
        while let Some(parent) = tree.nodes[id as usize].parent {
            id = parent;
            depth += 1;
        }
        depth
    };
    assert!(shallow.nodes.iter().all(|n| depth_of(&shallow, n.id) <= 2));
    assert!(deep.nodes.iter().any(|n| depth_of(&deep, n.id) > 2));
    // src/a is the cut: its tile carries everything beneath it, and says how much was folded.
    let cut = shallow.nodes.iter().find(|n| n.path == "src/a").unwrap();
    assert_eq!(
        cut.size,
        (1..=8).map(|i: u32| f64::from(i * i * 6)).sum::<f64>()
    );
    assert_eq!(cut.hidden, 8);
    // An aggregated tile is coloured by its aggregate, not left blank.
    assert!(
        shallow
            .nodes
            .iter()
            .filter(|n| n.value.is_some())
            .all(|n| n.color_step.is_some())
    );
}

#[test]
fn filters_narrow_every_dimension() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    let index = indexed(dir.path());
    let total = |filter: Filter| tree_with(&index, filter, None, None).total;
    assert_eq!(
        total(Filter {
            role: Some("test".into()),
            ..Filter::default()
        }),
        24.0
    );
    assert_eq!(
        total(Filter {
            path: Some("src/a".into()),
            ..Filter::default()
        }),
        (1..=8).map(|i| f64::from(i * i * 6)).sum::<f64>()
    );
    assert_eq!(
        total(Filter {
            language: Some("other".into()),
            ..Filter::default()
        }),
        2.0
    );
    let functions = total(Filter {
        symbol_kind: Some("function".into()),
        ..Filter::default()
    });
    // The blank lines between functions belong to no symbol.
    assert!(functions > 0.0);
    assert!(
        functions
            < total(Filter {
                language: Some("rust".into()),
                ..Filter::default()
            })
    );
    // The path filter is a boundary, not a string prefix.
    assert_eq!(
        total(Filter {
            path: Some("src/to".into()),
            ..Filter::default()
        }),
        0.0
    );
    assert_eq!(
        total(Filter {
            path: Some("src/top.rs".into()),
            concern: Some("core".into()),
            ..Filter::default()
        }),
        30.0
    );
}

#[test]
fn ratio_measures_divide_sums_and_colour_symmetrically_around_one() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    write(dir.path(), "src/in_test.rs", "fn a() {}\n");
    let index = indexed(dir.path());
    let inline = MeasureRef::Inline(Measure::Ratio {
        numerator: codemap_core::Term::new(
            Base::Lines,
            Filter {
                role: Some("test".into()),
                ..Filter::default()
            },
        ),
        denominator: codemap_core::Term::new(
            Base::Lines,
            Filter {
                role: Some("prod".into()),
                ..Filter::default()
            },
        ),
    });
    let query = Query::Tree {
        hierarchy: Hierarchy::Path,
        size: None,
        color: Some(inline),
        filter: Filter::default(),
        depth: Some(3),
    };
    let tree = tree_of(&index, &query);
    let root = &tree.nodes[0];
    let total = |role: &str| {
        tree_with(
            &index,
            Filter {
                role: Some(role.into()),
                ..Filter::default()
            },
            None,
            None,
        )
        .total
    };
    assert!((root.value.unwrap() - total("test") / total("prod")).abs() < 1e-9);
    let scale = tree.scale.unwrap();
    assert_eq!(scale.midpoint, Some(1.0));
    assert!(
        (scale.min * scale.max - 1.0).abs() < 1e-9,
        "symmetric in log space"
    );
    assert_eq!(scale.edges.len() as u32, scale.steps + 1);
    // A tile with no prod lines has no ratio, so no colour.
    let tests = tree.nodes.iter().find(|n| n.path == "tests/t.rs").unwrap();
    assert_eq!(tests.value, None);
    assert_eq!(tests.color_step, None);
}

#[test]
fn colour_steps_never_decrease_with_value_and_log_spacing_is_announced() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    // Line counts per file range over more than 100x only with a huge file added.
    write(dir.path(), "src/huge.rs", &plain_functions("h", 3000));
    let index = indexed(dir.path());
    let tree = tree_with(&index, Filter::default(), Some("lines"), Some(5));
    let scale = tree.scale.clone().unwrap();
    assert_eq!(scale.kind, codemap_core::ScaleKind::Sequential);
    assert_eq!(scale.spacing, codemap_core::Spacing::Log);
    let mut pairs: Vec<(f64, u32)> = tree
        .nodes
        .iter()
        .filter_map(|n| Some((n.value?, n.color_step?)))
        .collect();
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(pairs.windows(2).all(|w| w[0].1 <= w[1].1), "{pairs:?}");
    assert!(pairs.iter().all(|p| p.1 < scale.steps));
    let linear = tree_with(
        &index,
        Filter {
            path: Some("src/a".into()),
            ..Filter::default()
        },
        Some("lines"),
        Some(5),
    );
    assert_eq!(linear.scale.unwrap().spacing, codemap_core::Spacing::Linear);
}

#[test]
fn symbol_hierarchy_nests_methods_under_their_type() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "a.rs",
        "struct S;\nimpl S {\n    fn one(&self) -> u32 {\n        1\n    }\n    fn two(&self) -> u32 {\n        2\n    }\n}\n",
    );
    let index = indexed(dir.path());
    let query = Query::Tree {
        hierarchy: Hierarchy::Symbol,
        size: None,
        color: None,
        filter: Filter::default(),
        depth: Some(5),
    };
    let tree = tree_of(&index, &query);
    let find = |name: &str| tree.nodes.iter().find(|n| n.name == name).unwrap();
    let method = find("one");
    let owner = &tree.nodes[method.parent.unwrap() as usize];
    assert_eq!((owner.name.as_str(), owner.kind.as_str()), ("S", "type"));
    assert_eq!(method.kind, "function");
    assert_eq!(tree.nodes[owner.parent.unwrap() as usize].name, "a.rs");
}

#[test]
fn a_ratio_cannot_be_a_size_and_unknown_names_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    let index = indexed(dir.path());
    let ratio = |name: &str| Query::Tree {
        hierarchy: Hierarchy::Path,
        size: Some(MeasureRef::Named(name.to_owned())),
        color: None,
        filter: Filter::default(),
        depth: None,
    };
    assert!(matches!(
        answer(&index, &ratio("test_to_prod_ratio")),
        Err(QueryError::SizeIsRatio)
    ));
    assert!(matches!(
        answer(&index, &ratio("nope")),
        Err(QueryError::UnknownMeasure(_))
    ));
    assert!(matches!(
        answer(
            &index,
            &Query::Area {
                subject: "missing".into()
            }
        ),
        Err(QueryError::UnknownArea(_))
    ));
}

#[test]
fn the_overview_is_aggregates_that_add_up() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    let index = indexed(dir.path());
    let Answer::Overview(overview) = ask(&index, &Query::Overview) else {
        panic!()
    };
    for shares in [
        &overview.by_role,
        &overview.by_concern,
        &overview.by_language,
    ] {
        assert_eq!(
            shares.iter().map(|s| s.lines).sum::<u64>(),
            overview.totals.lines
        );
        assert!((shares.iter().map(|s| s.share).sum::<f64>() - 1.0).abs() < 1e-9);
    }
    let detectors: Vec<_> = overview
        .areas
        .by_detector
        .iter()
        .map(|d| d.detector.as_str())
        .collect();
    assert_eq!(
        detectors,
        codemap_core::DETECTORS
            .iter()
            .map(|d| d.name())
            .collect::<Vec<_>>()
    );
}

#[test]
fn the_envelope_names_its_schema_and_flattens_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    let index = indexed(dir.path());
    let envelope = Envelope::from(ask(&index, &Query::Catalog));
    let json = serde_json::to_value(&envelope).unwrap();
    assert_eq!(json["schema"], codemap_core::SCHEMA);
    assert_eq!(json["kind"], "catalog");
    assert!(
        json["measures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["name"] == "handler_lines")
    );
    let back: Envelope = serde_json::from_value(json).unwrap();
    assert_eq!(back, envelope);
    let query: Query = serde_json::from_str(
        r#"{"kind":"tree","color":"handler_to_core_ratio","filter":{"symbolKind":"function"}}"#,
    )
    .unwrap();
    assert!(matches!(query, Query::Tree { depth: None, .. }));
}

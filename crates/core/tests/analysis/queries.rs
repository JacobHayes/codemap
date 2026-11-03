//! The query layer through its public function: filters, measures, tree shaping and the envelope.

use codemap_core::{
    Answer, Axis, Base, Envelope, Filter, Hierarchy, Measure, MeasureRef, Query, QueryError,
    SOURCE_LINES_MAX, Source, Tree, Xtab, answer,
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
    let deep = tree_with(&index, Filter::default(), Some("lines"), Some(10));
    let shallow = tree_with(&index, Filter::default(), Some("lines"), Some(2));
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
fn generated_and_vendored_lines_are_hidden_unless_included() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    write(
        dir.path(),
        "gen.rs",
        &format!("// @generated\n{}", plain_functions("g", 20)),
    );
    let index = indexed(dir.path());
    let tree = |filter: Filter| tree_with(&index, filter, None, None);
    let named = |role: &str| {
        tree(Filter {
            role: Some(role.into()),
            ..Filter::default()
        })
    };
    let generated = named("generated");
    assert!(generated.total > 0.0);
    assert!(
        generated.hidden.is_none(),
        "asking for the role is not hiding it"
    );

    let default = tree(Filter::default());
    let everything = tree(Filter::everything());
    assert_eq!(everything.total - default.total, generated.total);
    assert!(everything.hidden.is_none());
    let hidden = default.hidden.expect("the answer says what it left out");
    assert_eq!(f64::from(hidden.lines), generated.total);
    assert!(hidden.roles.iter().any(|role| role == "generated"));

    // A cross-tab keeps to the same population and says the same thing.
    let by_role = |filter| xtab_of(&index, Axis::Role, Axis::Concern, filter);
    assert!(
        by_role(Filter::default())
            .rows
            .iter()
            .all(|r| r.name != "generated")
    );
    assert!(
        by_role(Filter::everything())
            .rows
            .iter()
            .any(|r| r.name == "generated")
    );
    assert_eq!(by_role(Filter::default()).hidden, Some(hidden));

    // A measure's own filter narrows inside the query's population and never widens it.
    let ratio = tree_with(&index, Filter::default(), Some("test_to_prod_ratio"), None);
    assert!(ratio.hidden.is_some());
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
fn ratio_measures_divide_sums_and_colour_sequentially_with_zero_plain() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    write(dir.path(), "src/tests/t.rs", &plain_functions("it", 2));
    write(dir.path(), "lib/x.rs", "fn a() {}\n");
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
        depth: Some(1),
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
    // A ratio of zero takes the plain fill; every other ratio is a step of the scale.
    let zero = tree.nodes.iter().find(|n| n.path == "lib").unwrap();
    assert_eq!(zero.value, Some(0.0));
    assert_eq!(zero.color_step, None);
    assert!(scale.min > 0.0);
    // With nothing but zeros the scale still names the measure, and no tile takes a step.
    let flat = tree_with(
        &index,
        Filter {
            path: Some("lib".into()),
            ..Filter::default()
        },
        Some("test_to_prod_ratio"),
        Some(1),
    );
    assert!(flat.scale.is_some());
    assert!(flat.nodes.iter().all(|n| n.color_step.is_none()));
    assert!(
        tree.nodes
            .iter()
            .filter(|n| n.value.is_some_and(|v| v > 0.0))
            .all(|n| n.color_step.is_some())
    );
    assert_eq!(scale.edges.len() as u32, scale.steps + 1);
    // A tile with no prod lines has no ratio, so no colour.
    let tests = tree.nodes.iter().find(|n| n.path == "tests").unwrap();
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
                subject: "missing".into(),
                filter: None,
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

fn xtab_of(index: &codemap_core::Index, rows: Axis, cols: Axis, filter: Filter) -> Xtab {
    let query = Query::Xtab {
        rows,
        cols,
        measure: None,
        filter,
        path_depth: None,
    };
    match ask(index, &query) {
        Answer::Xtab(xtab) => xtab,
        other => panic!("not a cross-tab: {other:?}"),
    }
}

#[test]
fn a_cross_tab_adds_up_in_its_cells_and_margins() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    let index = indexed(dir.path());
    let Answer::Overview(overview) = ask(&index, &Query::Overview) else {
        panic!()
    };
    let xtab = xtab_of(&index, Axis::Role, Axis::Concern, Filter::default());
    assert_eq!(xtab.total as u64, overview.totals.lines);
    let cell_sum: f64 = xtab.cells.iter().map(|c| c.value).sum();
    assert_eq!(cell_sum, xtab.total);
    for (i, row) in xtab.rows.iter().enumerate() {
        let across: f64 = xtab
            .cells
            .iter()
            .filter(|c| c.row as usize == i)
            .map(|c| c.value)
            .sum();
        assert_eq!(across, row.total);
    }
    // Margins are ordered largest first, and cells point at real headers.
    assert!(xtab.rows.windows(2).all(|w| w[0].total >= w[1].total));
    assert!(xtab.cols.windows(2).all(|w| w[0].total >= w[1].total));
    assert!(
        xtab.cells
            .iter()
            .all(|c| { (c.row as usize) < xtab.rows.len() && (c.col as usize) < xtab.cols.len() })
    );
    // The biggest cell takes the top colour step, the smallest the lowest.
    let scale = xtab.scale.as_ref().expect("cells have a scale");
    let top = xtab
        .cells
        .iter()
        .max_by(|a, b| a.value.total_cmp(&b.value))
        .unwrap();
    let low = xtab
        .cells
        .iter()
        .min_by(|a, b| a.value.total_cmp(&b.value))
        .unwrap();
    assert_eq!(top.color_step, scale.steps - 1);
    assert_eq!(low.color_step, 0);
}

#[test]
fn a_cross_tab_honours_filters_and_cuts_paths_at_a_depth() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    let index = indexed(dir.path());
    let filter = Filter {
        role: Some("test".to_owned()),
        ..Filter::default()
    };
    let xtab = xtab_of(&index, Axis::Path, Axis::Language, filter);
    assert_eq!(
        xtab.rows
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>(),
        ["tests"]
    );
    let deep = Query::Xtab {
        rows: Axis::Path,
        cols: Axis::Role,
        measure: None,
        filter: Filter::default(),
        path_depth: Some(3),
    };
    let Answer::Xtab(deep) = ask(&index, &deep) else {
        panic!()
    };
    assert!(deep.rows.iter().any(|r| r.name == "src/a/b"));
    assert!(deep.rows.iter().any(|r| r.name == "notes.md"));
    // Under a path, the depth counts from it: the default keeps the folder's children.
    let under = Query::Xtab {
        rows: Axis::Path,
        cols: Axis::Role,
        measure: None,
        filter: Filter {
            path: Some("src/a".to_owned()),
            ..Filter::default()
        },
        path_depth: None,
    };
    let Answer::Xtab(under) = ask(&index, &under) else {
        panic!()
    };
    assert_eq!(
        under
            .rows
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>(),
        ["src/a/b"]
    );
}

#[test]
fn a_cross_tab_rejects_a_ratio_and_a_bad_filter() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    let index = indexed(dir.path());
    let query = |measure: Option<&str>, role: Option<&str>| Query::Xtab {
        rows: Axis::Role,
        cols: Axis::Concern,
        measure: measure.map(|m| MeasureRef::Named(m.to_owned())),
        filter: Filter {
            role: role.map(str::to_owned),
            ..Filter::default()
        },
        path_depth: None,
    };
    assert!(matches!(
        answer(&index, &query(Some("test_to_prod_ratio"), None)),
        Err(QueryError::SizeIsRatio)
    ));
    assert!(matches!(
        answer(&index, &query(None, Some("nonsense"))),
        Err(QueryError::UnknownValue { .. })
    ));
}

fn source_of(
    index: &codemap_core::Index,
    path: &str,
    range: (Option<u32>, Option<u32>),
) -> Result<Source, QueryError> {
    let query = Query::Source {
        path: path.to_owned(),
        start_line: range.0,
        end_line: range.1,
    };
    answer(index, &query).map(|a| match a {
        Answer::Source(source) => source,
        other => panic!("not source: {other:?}"),
    })
}

#[test]
fn source_returns_the_lines_attributed_and_the_findings_that_touch_them() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    let index = indexed(dir.path());
    let whole = source_of(&index, "src/top.rs", (None, None)).unwrap();
    let text = std::fs::read_to_string(dir.path().join("src/top.rs")).unwrap();
    assert_eq!(whole.line_count as usize, text.lines().count());
    assert_eq!(whole.lines.len(), whole.line_count as usize);
    assert_eq!(whole.lines[0].number, 1);
    assert_eq!(whole.lines[0].text, text.lines().next().unwrap());
    assert!(whole.lines.iter().all(|l| l.role == "prod"));

    let part = source_of(&index, "src/top.rs", (Some(2), Some(3))).unwrap();
    assert_eq!(
        part.lines.iter().map(|l| l.number).collect::<Vec<_>>(),
        [2, 3]
    );
    // Every reported range overlaps the lines asked for.
    assert!(
        part.findings
            .iter()
            .all(|f| f.start_line <= 3 && f.end_line >= 2)
    );
    // An end past the file clips to it; a start past it is an error.
    let beyond = source_of(&index, "src/top.rs", (Some(28), Some(900))).unwrap();
    assert_eq!(beyond.end_line, whole.line_count);
    assert!(matches!(
        source_of(&index, "src/top.rs", (Some(900), Some(901))),
        Err(QueryError::BadRange)
    ));
}

#[test]
fn source_refuses_paths_outside_the_root_and_files_it_does_not_hold() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    let index = indexed(dir.path());
    for path in ["../secret", "/etc/passwd", "src/../../x", ""] {
        assert!(
            matches!(
                source_of(&index, path, (None, None)),
                Err(QueryError::OutsideRoot(_))
            ),
            "{path}"
        );
    }
    assert!(matches!(
        source_of(&index, "src/missing.rs", (None, None)),
        Err(QueryError::UnknownFile(_))
    ));
    assert!(matches!(
        source_of(&index, "src/top.rs", (Some(5), Some(2))),
        Err(QueryError::BadRange)
    ));
}

#[test]
fn source_reports_findings_with_their_detector_area_and_reason() {
    let dir = tempfile::tempdir().unwrap();
    crate::common::baseline(dir.path(), 20);
    for i in 0..3 {
        write(
            dir.path(),
            &format!("handlers/h{i}.rs"),
            &crate::common::handler_functions(&format!("h{i}_"), 20),
        );
    }
    let index = indexed(dir.path());
    let source = source_of(&index, "handlers/h0.rs", (Some(10), Some(20))).unwrap();
    assert!(!source.findings.is_empty());
    assert!(source.findings.iter().all(|f| {
        !f.detector.is_empty() && !f.reason.is_empty() && f.start_line <= 20 && f.end_line >= 10
    }));
    let quiet = source_of(&index, "src/mod0.rs", (None, None)).unwrap();
    assert!(quiet.findings.is_empty());
}

#[test]
fn source_caps_a_long_file_and_counts_what_follows() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "big.rs", &plain_functions("f", 120));
    let index = indexed(dir.path());
    let first = source_of(&index, "big.rs", (None, None)).unwrap();
    assert_eq!(first.lines.len(), SOURCE_LINES_MAX as usize);
    assert_eq!(first.more, first.line_count - SOURCE_LINES_MAX);
    let rest = source_of(&index, "big.rs", (Some(first.end_line + 1), None)).unwrap();
    assert_eq!(rest.start_line, first.end_line + 1);
    assert_eq!(
        first.lines.len() as u32 + rest.lines.len() as u32 + rest.more,
        first.line_count
    );
}

#[test]
fn source_refuses_a_file_changed_since_it_was_indexed() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.rs", &plain_functions("f", 2));
    let index = indexed(dir.path());
    write(dir.path(), "a.rs", &plain_functions("g", 3));
    assert!(matches!(
        source_of(&index, "a.rs", (None, None)),
        Err(QueryError::Stale(_))
    ));
}

#[test]
fn lines_outside_any_symbol_can_be_filtered_by_the_kind_a_cross_tab_names() {
    let dir = tempfile::tempdir().unwrap();
    mixed_tree(dir.path());
    let index = indexed(dir.path());
    let by_kind = xtab_of(&index, Axis::Kind, Axis::Role, Filter::default());
    let none = by_kind
        .rows
        .iter()
        .find(|r| r.name == "none")
        .expect("bare lines");
    let filter = Filter {
        symbol_kind: Some("none".to_owned()),
        ..Filter::default()
    };
    let drilled = xtab_of(&index, Axis::Role, Axis::Concern, filter);
    assert_eq!(drilled.total, none.total);
}

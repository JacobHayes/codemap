//! Each detector fires on a tree built to trip it and stays quiet on one that is uniform.

use codemap_core::{Answer, Filter, Query, QueryError, SubjectKind, answer};

use crate::common::{
    areas, baseline, baseline_dirs, findings_of, handler_functions, indexed, plain_functions, write,
};

#[test]
fn composition_fires_on_a_directory_heavy_in_handling() {
    let dir = tempfile::tempdir().unwrap();
    baseline_dirs(dir.path(), 20);
    for i in 0..3 {
        write(
            dir.path(),
            &format!("handlers/h{i}.rs"),
            &handler_functions(&format!("h{i}_"), 20),
        );
    }
    let index = indexed(dir.path());
    let found = findings_of(&index, "composition");
    let handlers: Vec<_> = found
        .iter()
        .filter(|f| f.labels["category"] == "error handling")
        .collect();
    assert!(
        handlers
            .iter()
            .any(|f| f.subject == "handlers" && f.subject_kind == SubjectKind::Directory)
    );
    let finding = handlers.iter().find(|f| f.subject == "handlers").unwrap();
    assert!(finding.measures["share"] > finding.measures["peerMedian"] + 0.1);
    assert!(finding.measures["excessLines"] >= 50.0);
    assert!(!finding.evidence.is_empty());
}

#[test]
fn composition_judges_against_peers_not_the_codebase_total() {
    let dir = tempfile::tempdir().unwrap();
    baseline_dirs(dir.path(), 20);
    // One comment-only schema holds most of the codebase's lines, so the codebase-wide comment
    // share is high; it must not hide a file that is unusual among ordinary files.
    let schema: String = (0..6000)
        .map(|i| format!("// field {i} of the schema\n"))
        .collect();
    write(
        dir.path(),
        "schema/schema.rs",
        &format!("{schema}fn schema() {{}}\n"),
    );
    let chatty: String = (0..80)
        .map(|i| format!("// note {i} on the next lines\n"))
        .collect();
    write(
        dir.path(),
        "src/pkg0/chatty.rs",
        &format!("{chatty}{}", plain_functions("c", 8)),
    );
    let index = indexed(dir.path());
    let found = findings_of(&index, "composition");
    assert!(
        found
            .iter()
            .any(|f| f.subject == "src/pkg0/chatty.rs" && f.labels["category"] == "comments")
    );
}

#[test]
fn composition_stays_quiet_on_a_uniform_codebase_and_on_tiny_files() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 20);
    // Entirely handling, but far too small to matter.
    write(dir.path(), "tiny.rs", &handler_functions("t", 1));
    let index = indexed(dir.path());
    assert!(findings_of(&index, "composition").is_empty());
}

#[test]
fn composition_measures_test_code_against_production_lines() {
    let dir = tempfile::tempdir().unwrap();
    baseline_dirs(dir.path(), 20);
    // All test code, no production lines: outside the population, so no share.
    for i in 0..3 {
        write(
            dir.path(),
            &format!("tests/t{i}.rs"),
            &plain_functions(&format!("t{i}_"), 20),
        );
    }
    let index = indexed(dir.path());
    assert!(findings_of(&index, "composition").is_empty());

    // A little production code with a lot of tests beside it.
    write(dir.path(), "pkg/src/a.rs", &plain_functions("a", 8));
    for i in 0..3 {
        write(
            dir.path(),
            &format!("pkg/tests/t{i}.rs"),
            &plain_functions(&format!("p{i}_"), 60),
        );
    }
    let index = indexed(dir.path());
    let found = findings_of(&index, "composition");
    assert!(found.iter().any(|f| f.subject == "pkg"));
    assert!(found.iter().all(|f| f.subject != "tests"));
}

#[test]
fn size_fires_on_an_outlier_file_function_and_concentrated_directory() {
    let dir = tempfile::tempdir().unwrap();
    baseline_dirs(dir.path(), 25);
    for i in 0..4 {
        write(
            dir.path(),
            &format!("big/huge{i}.rs"),
            &plain_functions(&format!("huge{i}_"), 150),
        );
    }
    write(dir.path(), "big/huge.rs", &plain_functions("huge", 400));
    let long_body: String = (0..120).map(|i| format!("    let v{i} = {i};\n")).collect();
    write(
        dir.path(),
        "src/long.rs",
        &format!("fn long() {{\n{long_body}}}\n"),
    );
    let index = indexed(dir.path());
    let found = findings_of(&index, "size");
    assert!(
        found
            .iter()
            .any(|f| f.subject == "big/huge.rs" && f.subject_kind == SubjectKind::File)
    );
    assert!(
        found
            .iter()
            .any(|f| f.subject == "src/long.rs::long" && f.subject_kind == SubjectKind::Function)
    );
    assert!(
        found
            .iter()
            .any(|f| f.subject == "big" && f.subject_kind == SubjectKind::Directory)
    );
    // Function findings roll up to the file they sit in.
    assert!(
        areas(&index)
            .areas
            .iter()
            .any(|a| a.subject == "src/long.rs")
    );
}

#[test]
fn every_finding_carries_facts_to_show() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 30);
    write(dir.path(), "big/huge.rs", &plain_functions("huge", 400));
    write(dir.path(), "handlers/h.rs", &handler_functions("h", 40));
    let index = indexed(dir.path());
    for detector in ["size", "composition"] {
        let found = findings_of(&index, detector);
        assert!(!found.is_empty(), "{detector} found nothing");
        assert!(found.iter().all(|f| !f.facts.is_empty()), "{detector}");
    }
}

#[test]
fn size_measures_only_files_in_a_programming_language() {
    let dir = tempfile::tempdir().unwrap();
    baseline_dirs(dir.path(), 25);
    let rows: String = (0..5000).map(|i| format!("row{i},{i}\n")).collect();
    write(dir.path(), "src/pkg0/vectors.csv", &rows);
    write(dir.path(), "src/pkg0/vectors.json", &rows);
    let index = indexed(dir.path());
    assert!(findings_of(&index, "size").is_empty());
    write(
        dir.path(),
        "src/pkg0/huge.rs",
        &plain_functions("huge", 400),
    );
    let index = indexed(dir.path());
    let found = findings_of(&index, "size");
    assert!(found.iter().any(|f| f.subject == "src/pkg0/huge.rs"));
    assert!(found.iter().all(|f| !f.subject.contains("vectors")));
}

#[test]
fn comments_in_type_declaration_files_are_not_judged() {
    let dir = tempfile::tempdir().unwrap();
    baseline_dirs(dir.path(), 20);
    let comments: String = (0..80)
        .map(|i| format!("// note {i} on the next lines\n"))
        .collect();
    let chatty = format!("{comments}{}", plain_functions("c", 8));
    write(dir.path(), "src/pkg0/api.rs", &chatty);
    let found = findings_of(&indexed(dir.path()), "composition");
    assert!(found.iter().any(|f| f.subject == "src/pkg0/api.rs"));

    let dir = tempfile::tempdir().unwrap();
    baseline_dirs(dir.path(), 20);
    let declared = format!("{comments}export declare function f(): void;\n");
    write(dir.path(), "src/pkg0/api.d.ts", &declared);
    let found = findings_of(&indexed(dir.path()), "composition");
    assert!(found.iter().all(|f| !f.subject.contains("api.d.ts")));
}

#[test]
fn size_stays_quiet_when_files_are_alike() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 40);
    let index = indexed(dir.path());
    assert!(findings_of(&index, "size").is_empty());
}

#[test]
fn rules_fire_per_file_and_function_and_directory_from_the_config() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 3);
    write(dir.path(), "handlers/h.rs", &handler_functions("h", 10));
    write(
        dir.path(),
        ".codemap/config.toml",
        r#"
[[rule]]
name = "file handling"
measure = "handler_lines"
per = "file"
at_most = 20

[[rule]]
name = "function handling"
measure = "handler_lines"
per = "function"
at_most = 3

[[rule]]
name = "dir handling"
measure = "handler_lines"
per = "directory"
at_most = 20
"#,
    );
    let index = indexed(dir.path());
    let found = findings_of(&index, "rules");
    let rule = |name: &str| {
        found
            .iter()
            .filter(|f| f.labels["rule"] == name)
            .collect::<Vec<_>>()
    };
    assert_eq!(rule("file handling").len(), 1);
    assert_eq!(rule("file handling")[0].subject, "handlers/h.rs");
    assert_eq!(rule("function handling").len(), 10);
    assert_eq!(rule("dir handling")[0].subject, "handlers");
    assert!(rule("file handling")[0].measures["value"] > 20.0);
}

#[test]
fn without_a_config_there_are_no_rule_findings() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 3);
    write(dir.path(), "handlers/h.rs", &handler_functions("h", 10));
    let index = indexed(dir.path());
    assert!(findings_of(&index, "rules").is_empty());
}

#[test]
fn generated_and_vendored_mass_is_not_ranked() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 30);
    write(dir.path(), "vendor/lib.rs", &plain_functions("v", 600));
    write(dir.path(), "api.lock", &"x = 1\n".repeat(5000));
    write(
        dir.path(),
        "gen/out.rs",
        &format!("// @generated\n{}", plain_functions("g", 600)),
    );
    let index = indexed(dir.path());
    assert!(areas(&index).areas.is_empty());
    let Answer::Overview(overview) = answer(&index, &Query::Overview).unwrap() else {
        panic!()
    };
    // Still reported: the overview counts them.
    assert!(
        overview
            .by_role
            .iter()
            .any(|s| s.name == "generated" && s.lines > 5000)
    );
    assert!(overview.by_role.iter().any(|s| s.name == "vendored"));
}

#[test]
fn acknowledged_areas_rank_after_the_rest_and_are_counted() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 30);
    write(dir.path(), "big/huge.rs", &plain_functions("huge", 600));
    write(dir.path(), "other/large.rs", &plain_functions("large", 200));
    let before = areas(&indexed(dir.path()));
    assert_eq!(before.acknowledged, 0);
    assert_eq!(before.areas[0].subject, "big/huge.rs");

    write(
        dir.path(),
        ".codemap/config.toml",
        "[[acknowledge]]\npath = \"big\"\nreason = \"the parser tables are big on purpose\"\n",
    );
    let index = indexed(dir.path());
    let after = areas(&index);
    assert_eq!(after.matching, before.matching, "nothing is hidden");
    assert!(after.acknowledged >= 1);
    let first_acknowledged = after
        .areas
        .iter()
        .position(|a| a.acknowledged.is_some())
        .unwrap();
    assert!(
        after.areas[first_acknowledged..]
            .iter()
            .all(|a| a.acknowledged.is_some())
    );
    let huge = after
        .areas
        .iter()
        .find(|a| a.subject == "big/huge.rs")
        .unwrap();
    assert_eq!(
        huge.acknowledged.as_deref(),
        Some("the parser tables are big on purpose")
    );
    let Answer::Overview(overview) = answer(&index, &Query::Overview).unwrap() else {
        panic!()
    };
    assert_eq!(overview.areas.acknowledged, after.acknowledged);
}

#[test]
fn an_acknowledgement_for_one_detector_leaves_the_others_open() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 30);
    write(dir.path(), "big/huge.rs", &plain_functions("huge", 600));
    write(
        dir.path(),
        ".codemap/config.toml",
        "[[acknowledge]]\npath = \"big/huge.rs\"\ndetector = \"rules\"\nreason = \"fine\"\n",
    );
    let index = indexed(dir.path());
    let huge = areas(&index)
        .areas
        .into_iter()
        .find(|a| a.subject == "big/huge.rs")
        .unwrap();
    assert_eq!(huge.acknowledged, None);
}

#[test]
fn role_overrides_apply_at_index_time() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 3);
    write(dir.path(), "fixtures/data.rs", &plain_functions("d", 50));
    write(
        dir.path(),
        ".codemap/config.toml",
        "[roles]\n\"fixtures/\" = \"generated\"\n",
    );
    let index = indexed(dir.path());
    let Answer::Overview(overview) = answer(&index, &Query::Overview).unwrap() else {
        panic!()
    };
    assert!(
        overview
            .by_role
            .iter()
            .any(|s| s.name == "generated" && s.lines == 300)
    );
}

#[test]
fn areas_filter_by_the_lines_their_findings_touch() {
    let dir = tempfile::tempdir().unwrap();
    baseline_dirs(dir.path(), 20);
    for i in 0..3 {
        write(
            dir.path(),
            &format!("handlers/h{i}.rs"),
            &handler_functions(&format!("h{i}_"), 20),
        );
    }
    let index = indexed(dir.path());
    let subjects = |filter: Filter| -> Vec<String> {
        match answer(
            &index,
            &Query::Areas {
                top: None,
                detector: None,
                filter,
            },
        ) {
            Ok(Answer::Areas(list)) => list.areas.into_iter().map(|a| a.subject).collect(),
            other => panic!("not areas: {other:?}"),
        }
    };
    let by = |role: Option<&str>, concern: Option<&str>, language: Option<&str>| {
        subjects(Filter {
            role: role.map(str::to_owned),
            concern: concern.map(str::to_owned),
            language: language.map(str::to_owned),
            ..Filter::default()
        })
    };
    let all = subjects(Filter::default());
    assert!(all.iter().any(|s| s == "handlers"));

    let handling = by(None, Some("handler"), None);
    assert!(handling.iter().any(|s| s == "handlers"));
    assert!(by(None, Some("assertion"), None).is_empty());

    assert_eq!(by(None, None, Some("rust")), all);
    assert!(by(None, None, Some("python")).is_empty());
    assert!(by(Some("test"), None, None).is_empty());
    assert!(
        by(Some("prod"), Some("handler"), None)
            .iter()
            .any(|s| s == "handlers")
    );
    assert!(matches!(
        answer(
            &index,
            &Query::Areas {
                top: None,
                detector: None,
                filter: Filter {
                    role: Some("nonsense".to_owned()),
                    ..Filter::default()
                },
            }
        ),
        Err(QueryError::UnknownValue { .. })
    ));
}

//! Each detector fires on a tree built to trip it and stays quiet on one that is uniform.

use codemap_core::{Answer, Query, SubjectKind, answer};

use crate::common::{areas, ask, baseline, handler_functions, indexed, plain_functions, write};

fn findings_of(index: &codemap_core::Index, detector: &str) -> Vec<codemap_core::Finding> {
    areas(index)
        .areas
        .iter()
        .flat_map(|area| {
            match ask(
                index,
                &Query::Area {
                    subject: area.subject.clone(),
                },
            ) {
                Answer::Area(detail) => detail.findings,
                other => panic!("not an area: {other:?}"),
            }
        })
        .filter(|finding| finding.detector == detector)
        .collect()
}

#[test]
fn composition_fires_on_a_directory_heavy_in_handling() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 20);
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
    assert!(finding.measures["share"] > 2.0 * finding.measures["codebaseShare"]);
    assert!(finding.measures["excessLines"] >= 50.0);
    assert!(!finding.evidence.is_empty());
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
fn size_fires_on_an_outlier_file_function_and_concentrated_directory() {
    let dir = tempfile::tempdir().unwrap();
    baseline(dir.path(), 30);
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

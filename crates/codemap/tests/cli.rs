//! The verbs end to end: the real binary on a tree it indexes itself.
#![expect(
    clippy::format_collect,
    clippy::assert_is_empty,
    reason = "test fixtures are built from repeated snippets"
)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codemap"))
        .args(args)
        .arg("-C")
        .arg(root)
        .output()
        .expect("the binary runs")
}

fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    for i in 0..25 {
        let body: String = (0..6)
            .map(|j| format!("def f{i}_{j}(a):\n    b = a + 1\n    return b\n\n"))
            .collect();
        fs::create_dir_all(dir.path().join("pkg")).expect("mkdir");
        fs::write(dir.path().join(format!("pkg/m{i}.py")), body).expect("write");
    }
    let huge: String = (0..500)
        .map(|j| format!("def g{j}(a):\n    b = a + 1\n    return b\n\n"))
        .collect();
    fs::write(dir.path().join("huge.py"), huge).expect("write");
    dir
}

fn json(output: &Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout is one JSON envelope")
}

#[test]
fn every_verb_indexes_first_and_answers_in_text_and_json() {
    let dir = tree();
    let root = dir.path();

    // No index yet: the first verb builds it.
    let overview = json(&run(root, &["overview", "--json"]));
    assert_eq!(overview["schema"], "codemap.answer/v1");
    assert_eq!(overview["kind"], "overview");
    assert!(root.join(".codemap/index.duckdb").exists());

    let areas = json(&run(root, &["areas", "--top", "3", "--json"]));
    assert_eq!(areas["areas"][0]["subject"], "huge.py");
    let subject = areas["areas"][0]["subject"]
        .as_str()
        .expect("subject")
        .to_owned();

    let area = json(&run(root, &["area", &subject, "--json"]));
    assert_eq!(area["kind"], "area");
    assert!(!area["findings"].as_array().expect("findings").is_empty());

    let tree = json(&run(
        root,
        &[
            "tree",
            "--color",
            "handler_to_core_ratio",
            "--depth",
            "1",
            "--json",
        ],
    ));
    assert_eq!(tree["kind"], "tree");
    assert_eq!(tree["nodes"][0]["parent"], serde_json::Value::Null);

    let catalog = json(&run(root, &["catalog", "--json"]));
    assert!(catalog["detectors"].as_array().expect("detectors").len() >= 3);

    // Text: a totals line first, then a header row, and repo-relative paths.
    let text = String::from_utf8(run(root, &["areas"]).stdout).expect("utf-8");
    let mut lines = text.lines();
    assert!(lines.next().expect("totals line").contains("areas"));
    assert!(text.contains("huge.py"));
    assert!(!text.contains(root.to_str().expect("utf-8 path")));
    for verb in ["overview", "tree", "catalog"] {
        assert!(run(root, &[verb]).status.success());
    }
}

#[test]
fn an_unknown_subject_fails_with_a_message_on_stderr() {
    let dir = tree();
    let output = run(dir.path(), &["area", "nowhere"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn xtab_and_source_answer_in_text_and_json() {
    let dir = tree();
    let root = dir.path();

    let xtab = json(&run(
        root,
        &["xtab", "--rows", "path", "--cols", "concern", "--json"],
    ));
    assert_eq!(xtab["kind"], "xtab");
    assert_eq!(xtab["rows"][0]["name"], "huge.py");
    let text =
        String::from_utf8(run(root, &["xtab", "--rows", "role", "--cols", "concern"]).stdout)
            .expect("utf-8");
    assert!(
        text.lines()
            .last()
            .expect("a margin row")
            .starts_with("total")
    );

    let source = json(&run(
        root,
        &["source", "huge.py", "--lines", "2-3", "--json"],
    ));
    assert_eq!(source["kind"], "source");
    assert_eq!(source["lines"].as_array().expect("lines").len(), 2);
    assert_eq!(source["lines"][0]["number"], 2);
    let text =
        String::from_utf8(run(root, &["source", "huge.py", "--lines", "1"]).stdout).expect("utf-8");
    assert!(text.contains("def g0(a):"));

    let outside = run(root, &["source", "../etc/passwd"]);
    assert!(!outside.status.success());
    assert!(outside.stdout.is_empty());
}

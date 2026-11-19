//! End-to-end checks of the CLI closed loop.
//!
//! They run the real binary: create a native project, reopen it, inspect it and
//! walk the static show. Corpus, visual baselines and PPTX round trips belong to
//! the product test suite, not to this smoke layer.

use std::{
    path::PathBuf,
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_open-ppt")).args(args).output().expect("the CLI binary must run")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock after epoch").as_nanos();
    let dir = std::env::temp_dir().join(format!("open-ppt-cli-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temporary directory");
    dir
}

#[test]
fn capabilities_report_project_support_and_no_presentation_format() {
    let output = run(&["capabilities", "--json"]);
    assert!(output.status.success(), "capabilities must succeed");
    let capabilities: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("capabilities JSON");

    assert_eq!(capabilities["project"]["import"], "supported");
    assert_eq!(capabilities["playback"]["staticSlides"], "supported");
    assert_eq!(capabilities["checks"]["contrast"], "supported");
    assert_eq!(capabilities["formats"]["pptx"]["import"], "unsupported");
    assert_eq!(capabilities["exportTargets"]["pptx"], "unsupported");
}

#[test]
fn create_open_inspect_and_play_keep_one_deck() {
    let dir = temp_dir("loop");
    let project = dir.join("deck.project");
    let project = project.to_str().expect("UTF-8 path");

    let created = run(&["create", "--out", project, "--title", "Research review", "--slide", "Where growth comes from"]);
    assert!(created.status.success(), "create must succeed: {}", String::from_utf8_lossy(&created.stderr));

    let opened = run(&["open", project, "--json"]);
    assert!(opened.status.success(), "open must succeed");
    let summary: serde_json::Value = serde_json::from_str(&stdout(&opened)).expect("summary JSON");
    assert_eq!(summary["revision"], 1);
    assert_eq!(summary["slides"].as_array().map(Vec::len), Some(1));
    assert_eq!(summary["slides"][0]["title"], "Where growth comes from");

    let inspected = run(&["inspect", project, "--checks", "out-of-bounds,contrast,assets", "--json"]);
    assert!(inspected.status.success(), "inspect must pass: {}", String::from_utf8_lossy(&inspected.stderr));
    let report: serde_json::Value = serde_json::from_str(&stdout(&inspected)).expect("report JSON");
    assert_eq!(report["passed"], true);
    assert_eq!(report["checkerVersion"], "authored-p1");

    let played = run(&["play", project, "--json"]);
    assert!(played.status.success(), "play must succeed");
    let frames: serde_json::Value = serde_json::from_str(&stdout(&played)).expect("frames JSON");
    assert_eq!(frames["frames"].as_array().map(Vec::len), Some(1));
    assert_eq!(frames["frames"][0]["revision"], 1);

    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn unsupported_check_is_reported_instead_of_passing_silently() {
    let dir = temp_dir("check");
    let project = dir.join("deck.project");
    let project = project.to_str().expect("UTF-8 path");
    assert!(run(&["create", "--out", project, "--slide", "Only slide"]).status.success());

    let inspected = run(&["inspect", project, "--checks", "overflow", "--json"]);
    assert!(inspected.status.success(), "warnings do not fail the run");
    let report: serde_json::Value = serde_json::from_str(&stdout(&inspected)).expect("report JSON");
    let codes: Vec<&str> =
        report["issues"].as_array().expect("issues").iter().filter_map(|issue| issue["code"].as_str()).collect();
    assert!(codes.contains(&"deck.check-unsupported"), "missing checks must be visible: {codes:?}");

    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn usage_and_missing_input_fail_with_distinct_exit_codes() {
    let missing_out = run(&["create", "--title", "No target"]);
    assert_eq!(missing_out.status.code(), Some(2));

    let unknown = run(&["create", "--out", "unused", "--bogus", "value"]);
    assert_eq!(unknown.status.code(), Some(2));

    let missing_file = run(&["open", "does-not-exist.project"]);
    assert_eq!(missing_file.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&missing_file.stderr).contains("io/read"));
}

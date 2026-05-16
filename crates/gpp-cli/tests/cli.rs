//! End-to-end tests for the `gpp` binary.

use assert_cmd::Command;
use predicates::str::contains;

fn gpp() -> Command {
    Command::cargo_bin("gpp").unwrap()
}

#[test]
fn init_creates_repository_layout() {
    let dir = tempfile::tempdir().unwrap();
    gpp()
        .args(["init"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(contains("Initialized empty gpp repository"));

    let gpp_dir = dir.path().join(".gpp");
    assert!(gpp_dir.join("objects").is_dir());
    assert!(gpp_dir.join("refs").is_dir());
    assert!(gpp_dir.join("config.toml").is_file());
    assert_eq!(
        std::fs::read_to_string(gpp_dir.join("HEAD")).unwrap(),
        "ref: refs/main\n"
    );
}

#[test]
fn init_refuses_existing_repo() {
    let dir = tempfile::tempdir().unwrap();
    gpp()
        .args(["init"])
        .current_dir(dir.path())
        .assert()
        .success();
    gpp()
        .args(["init"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(contains("already a gpp repository"));
}

#[test]
fn init_graphex_flag_sets_config() {
    let dir = tempfile::tempdir().unwrap();
    gpp()
        .args(["init", "--graphex"])
        .current_dir(dir.path())
        .assert()
        .success();
    let cfg = std::fs::read_to_string(dir.path().join(".gpp/config.toml")).unwrap();
    let doc: toml::Value = toml::from_str(&cfg).unwrap();
    assert_eq!(doc["graphex"]["enabled"].as_bool(), Some(true));
}

#[test]
fn from_git_is_reported_unimplemented() {
    let dir = tempfile::tempdir().unwrap();
    gpp()
        .args(["init", "--from-git", "/tmp/whatever"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(contains("not implemented yet"));
}

#[test]
fn status_reports_branch() {
    let dir = tempfile::tempdir().unwrap();
    gpp()
        .args(["init"])
        .current_dir(dir.path())
        .assert()
        .success();
    gpp()
        .args(["status"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(contains("On branch: main"));
}

#[test]
fn status_outside_repo_fails() {
    let dir = tempfile::tempdir().unwrap();
    gpp()
        .args(["status"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(contains("not a gpp repository"));
}

#[test]
fn config_get_set_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    gpp()
        .args(["init"])
        .current_dir(dir.path())
        .assert()
        .success();

    gpp()
        .args(["config", "set", "timeline.retention_days", "60"])
        .current_dir(dir.path())
        .assert()
        .success();
    gpp()
        .args(["config", "get", "timeline.retention_days"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(contains("60"));
}

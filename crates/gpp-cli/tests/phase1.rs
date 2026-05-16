//! End-to-end tests for the Phase 1 commands.

use assert_cmd::Command;
use predicates::str::contains;

/// A `gpp` command rooted in `dir` with an isolated HOME (no global config).
fn gpp(dir: &std::path::Path, home: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("gpp").unwrap();
    c.current_dir(dir)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME");
    c
}

struct Ctx {
    _home: tempfile::TempDir,
    repo: tempfile::TempDir,
    home_path: std::path::PathBuf,
}

fn init_repo() -> Ctx {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let home_path = home.path().to_path_buf();
    gpp(repo.path(), &home_path).arg("init").assert().success();
    Ctx {
        _home: home,
        repo,
        home_path,
    }
}

#[test]
fn timeline_captures_changes() {
    let ctx = init_repo();
    std::fs::write(ctx.repo.path().join("a.txt"), "hello\n").unwrap();
    gpp(ctx.repo.path(), &ctx.home_path)
        .arg("timeline")
        .assert()
        .success()
        .stdout(contains("file changed"));
}

#[test]
fn ignored_paths_not_captured() {
    let ctx = init_repo();
    std::fs::create_dir_all(ctx.repo.path().join("target")).unwrap();
    std::fs::write(ctx.repo.path().join("target/x.o"), "junk").unwrap();
    gpp(ctx.repo.path(), &ctx.home_path)
        .arg("timeline")
        .assert()
        .success()
        .stdout(contains("(no timeline entries)"));
}

#[test]
fn promote_then_log_and_status() {
    let ctx = init_repo();
    std::fs::write(ctx.repo.path().join("main.rs"), "fn main() {}\n").unwrap();

    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["promote", "-m", "initial commit", "--intent", "feature"])
        .assert()
        .success()
        .stdout(contains("Promoted"))
        .stdout(contains("on main"));

    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["log", "--oneline"])
        .assert()
        .success()
        .stdout(contains("initial commit"));

    gpp(ctx.repo.path(), &ctx.home_path)
        .arg("status")
        .assert()
        .success()
        .stdout(contains("On branch: main (cs:"));
}

#[test]
fn promote_with_nothing_fails() {
    let ctx = init_repo();
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["promote", "-m", "empty"])
        .assert()
        .failure()
        .stderr(contains("nothing to promote"));
}

#[test]
fn promote_requires_message() {
    let ctx = init_repo();
    std::fs::write(ctx.repo.path().join("f"), "x").unwrap();
    gpp(ctx.repo.path(), &ctx.home_path)
        .arg("promote")
        .assert()
        .failure()
        .stderr(contains("message is required"));
}

#[test]
fn diff_working_vs_head() {
    let ctx = init_repo();
    std::fs::write(ctx.repo.path().join("a.txt"), "one\n").unwrap();
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["promote", "-m", "base"])
        .assert()
        .success();
    std::fs::write(ctx.repo.path().join("a.txt"), "one\ntwo\n").unwrap();
    gpp(ctx.repo.path(), &ctx.home_path)
        .arg("diff")
        .assert()
        .success()
        .stdout(contains("+two"));
}

#[test]
fn branch_create_switch_list() {
    let ctx = init_repo();
    std::fs::write(ctx.repo.path().join("a"), "x").unwrap();
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["promote", "-m", "base"])
        .assert()
        .success();

    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["branch", "create", "feature/x"])
        .assert()
        .success();
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["branch", "switch", "feature/x"])
        .assert()
        .success();
    gpp(ctx.repo.path(), &ctx.home_path)
        .arg("branch")
        .assert()
        .success()
        .stdout(contains("* feature/x"));
}

#[test]
fn unknown_branch_switch_fails() {
    let ctx = init_repo();
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["branch", "switch", "nope"])
        .assert()
        .failure()
        .stderr(contains("does not exist"));
}

#[test]
fn timeline_prune_reports_count() {
    let ctx = init_repo();
    std::fs::write(ctx.repo.path().join("a"), "x").unwrap();
    gpp(ctx.repo.path(), &ctx.home_path)
        .arg("timeline")
        .assert()
        .success();
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["timeline", "prune", "--older-than", "0s"])
        .assert()
        .success()
        .stdout(contains("pruned"));
}

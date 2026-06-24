//! End-to-end tests for policy enforcement points beyond promotion:
//! the `warn` point (timeline capture) and the `block` point (sync).
//!
//! Background: a `block`-severity rule already aborts promotion. These tests
//! cover the two enforcement points that were previously wired to nothing —
//! so a policy that *looked* active was silently a no-op there.

use assert_cmd::Command;
use predicates::prelude::*;
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

fn install_secrets_policy(ctx: &Ctx) {
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["policy", "template", "secrets-scan"])
        .assert()
        .success();
}

// An obviously-fake key that still matches the AWS-access-key rule (block) and
// the generic-secret rule (warn) from the secrets-scan template.
const AWS_KEY_LINE: &str = "let k = \"AKIAIOSFODNN7EXAMPLE\";\n";

// ---- warn point: timeline capture -----------------------------------------

#[test]
fn timeline_warns_on_policy_violation() {
    let ctx = init_repo();
    install_secrets_policy(&ctx);
    std::fs::write(ctx.repo.path().join("config.rs"), AWS_KEY_LINE).unwrap();

    // `gpp timeline` captures, and must surface the violation as a non-fatal
    // warning (with the "will block promote/sync" hint for block-severity).
    gpp(ctx.repo.path(), &ctx.home_path)
        .arg("timeline")
        .assert()
        .success() // warn never aborts
        .stderr(contains("policy warning"))
        .stderr(contains("aws-access-key"))
        .stderr(contains("will block promote/sync"));
}

#[test]
fn timeline_silent_when_clean() {
    let ctx = init_repo();
    install_secrets_policy(&ctx);
    std::fs::write(ctx.repo.path().join("main.rs"), "fn main() {}\n").unwrap();

    gpp(ctx.repo.path(), &ctx.home_path)
        .arg("timeline")
        .assert()
        .success()
        .stderr(contains("policy warning").not());
}

// ---- block point: sync -----------------------------------------------------

#[test]
fn sync_blocks_when_tip_violates_policy() {
    let ctx = init_repo();

    // Promote the offending content *before* the policy exists, so it reaches
    // a branch tip (the real-world scenario: policy installed after the fact).
    std::fs::write(ctx.repo.path().join("config.rs"), AWS_KEY_LINE).unwrap();
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["promote", "-m", "add config", "--intent", "feature"])
        .assert()
        .success();

    install_secrets_policy(&ctx);

    // A peer must exist so we reach the transmit path; the address is never
    // contacted because the policy gate fails first.
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["sync", "add", "dead", "127.0.0.1:1"])
        .assert()
        .success();

    gpp(ctx.repo.path(), &ctx.home_path)
        .arg("sync")
        .assert()
        .failure()
        .stderr(contains("sync blocked by policy"))
        .stderr(contains("aws-access-key"));
}

#[test]
fn sync_serve_blocks_when_tip_violates_policy() {
    let ctx = init_repo();
    std::fs::write(ctx.repo.path().join("config.rs"), AWS_KEY_LINE).unwrap();
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["promote", "-m", "add config", "--intent", "feature"])
        .assert()
        .success();
    install_secrets_policy(&ctx);

    // `serve` would otherwise bind a socket and block forever; the policy gate
    // must reject before that, so the command returns promptly with an error.
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["sync", "serve", "127.0.0.1:0"])
        .timeout(std::time::Duration::from_secs(20))
        .assert()
        .failure()
        .stderr(contains("sync blocked by policy"));
}

#[test]
fn sync_allowed_when_clean() {
    let ctx = init_repo();
    std::fs::write(ctx.repo.path().join("main.rs"), "fn main() {}\n").unwrap();
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["promote", "-m", "clean", "--intent", "feature"])
        .assert()
        .success();
    install_secrets_policy(&ctx);

    // Unreachable peer: per-peer connect errors are reported but the command
    // still succeeds. The point is the policy gate does NOT block clean content.
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["sync", "add", "dead", "127.0.0.1:1"])
        .assert()
        .success();
    gpp(ctx.repo.path(), &ctx.home_path)
        .arg("sync")
        .assert()
        .success()
        .stderr(contains("sync blocked by policy").not());
}

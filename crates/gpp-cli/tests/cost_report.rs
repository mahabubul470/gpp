//! End-to-end tests for agent cost self-reporting (`gpp cost --report`).
//!
//! Background: a changeset's cost record is created at promote time with
//! tokens/cost = 0. `--report` is the path an agent (or wrapper, or the
//! `report_cost` MCP tool) uses to attribute real usage. These cover that the
//! report lands on the *same* record the promote created (id resolution) and
//! that repeat reports accumulate rather than replace.

use assert_cmd::Command;
use predicates::str::contains;

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

/// Write a file and promote it, returning a clean `Ctx`.
fn promote_one(ctx: &Ctx) {
    std::fs::write(ctx.repo.path().join("main.rs"), "fn main() {}\n").unwrap();
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["promote", "-m", "initial"])
        .assert()
        .success();
}

/// Parse the cost roll-up JSON for one numeric field.
fn cost_field(ctx: &Ctx, field: &str) -> f64 {
    let out = gpp(ctx.repo.path(), &ctx.home_path)
        .args(["cost", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    v[field].as_f64().unwrap()
}

#[test]
fn report_lands_on_promote_record_and_accumulates() {
    let ctx = init_repo();
    promote_one(&ctx);

    // One changeset, zero cost so far.
    assert_eq!(cost_field(&ctx, "changesets"), 1.0);
    assert_eq!(cost_field(&ctx, "input_tokens"), 0.0);

    // Report against HEAD — resolves to the promote record, doesn't add a row.
    gpp(ctx.repo.path(), &ctx.home_path)
        .args([
            "cost",
            "--report",
            "HEAD",
            "--model",
            "claude-opus-4-8",
            "--input",
            "1500",
            "--output",
            "300",
            "--cost-micro",
            "22000",
        ])
        .assert()
        .success()
        .stdout(contains("recorded usage"));

    assert_eq!(cost_field(&ctx, "changesets"), 1.0, "no stray record");
    assert_eq!(cost_field(&ctx, "input_tokens"), 1500.0);
    assert_eq!(cost_field(&ctx, "cost_usd"), 0.022);

    // Second report accumulates onto the same record.
    gpp(ctx.repo.path(), &ctx.home_path)
        .args([
            "cost",
            "--report",
            "HEAD",
            "--input",
            "500",
            "--output",
            "100",
            "--cost-micro",
            "8000",
        ])
        .assert()
        .success();

    assert_eq!(cost_field(&ctx, "changesets"), 1.0);
    assert_eq!(cost_field(&ctx, "input_tokens"), 2000.0);
    assert_eq!(cost_field(&ctx, "output_tokens"), 400.0);
    assert_eq!(cost_field(&ctx, "cost_usd"), 0.030);
}

#[test]
fn report_json_output_reports_running_total() {
    let ctx = init_repo();
    promote_one(&ctx);

    let out = gpp(ctx.repo.path(), &ctx.home_path)
        .args([
            "cost",
            "--report",
            "HEAD",
            "--input",
            "1000",
            "--cost-micro",
            "12000",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["added"]["input_tokens"].as_i64().unwrap(), 1000);
    assert_eq!(v["total_cost_usd"].as_f64().unwrap(), 0.012);
}

#[test]
fn report_unknown_changeset_fails() {
    let ctx = init_repo();
    promote_one(&ctx);
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["cost", "--report", "zzzznotahash", "--input", "10"])
        .assert()
        .failure();
}

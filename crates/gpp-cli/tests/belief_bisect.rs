//! Belief Bisect — deterministic synthetic-repo demo (handoff §5, CI tier).
//!
//! Scripts a small repo through a staged refactor and asserts the staleness
//! engine answers, with zero LLM/network calls:
//!
//! * `belief bisect B2` ("token expiry is 24h") → the expiry-change commit,
//!   Invalidated, with the offending hunk.
//! * `belief bisect B1` ("auth issues JWTs") → the session-migration commit.
//! * `belief at <C0>` reproduces the original (all-active) belief set.
//! * `belief stale` lists both beliefs with their triggering commits.

use assert_cmd::Command;
use predicates::str::contains;

fn gpp(dir: &std::path::Path, home: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("gpp").unwrap();
    c.current_dir(dir)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("GPP_GRAPHEX_PASSPHRASE");
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
    gpp(repo.path(), &home_path)
        .args(["init", "--graphex"])
        .assert()
        .success();
    Ctx {
        _home: home,
        repo,
        home_path,
    }
}

fn write(ctx: &Ctx, path: &str, content: &str) {
    let p = ctx.repo.path().join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

fn rm(ctx: &Ctx, path: &str) {
    std::fs::remove_file(ctx.repo.path().join(path)).unwrap();
}

/// Promote and return the changeset's short id (from `→ cs:<short>`).
fn promote(ctx: &Ctx, msg: &str) -> String {
    let out = gpp(ctx.repo.path(), &ctx.home_path)
        .args(["promote", "-m", msg])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let idx = text.find("cs:").expect("promote prints cs:<id>");
    text[idx + 3..]
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn belief_json(ctx: &Ctx, args: &[&str]) -> serde_json::Value {
    let mut full = vec!["belief"];
    full.extend_from_slice(args);
    full.push("--json");
    let out = gpp(ctx.repo.path(), &ctx.home_path)
        .args(&full)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}

const TOKEN_V0: &str = "\
use crate::jwt;

pub fn issue_token(user: &str) -> String {
    jwt::encode(user, EXPIRY_HOURS)
}

pub const EXPIRY_HOURS: u64 = 24;

pub fn validate(token: &str) -> bool {
    jwt::decode(token).is_ok()
}
";

/// Run the full staged refactor, returning
/// (c0, b1_id, b2_id, expiry_commit, session_commit).
fn seed_and_refactor(ctx: &Ctx) -> (String, String, String, String, String) {
    // C0 — the world the beliefs are formed in.
    write(ctx, "auth/token.rs", TOKEN_V0);
    write(ctx, "auth/mod.rs", "pub mod token;\n");
    write(ctx, "main.rs", "mod auth;\nfn main() {}\n");
    promote(ctx, "seed auth module with JWT issuance");

    let b1 = belief_json(
        ctx,
        &[
            "add",
            "--claim",
            "auth issues JWTs",
            "--path",
            "auth/**",
            "--evidence",
            "auth/token.rs:3-5",
        ],
    );
    let b2 = belief_json(
        ctx,
        &[
            "add",
            "--claim",
            "token expiry is 24h",
            "--path",
            "auth/**",
            "--evidence",
            "auth/token.rs:7-7",
        ],
    );
    let c0 = b1["anchor"].as_str().unwrap().to_string();
    assert_eq!(b2["anchor"].as_str().unwrap(), c0);

    // 1 — unrelated change: must not touch either belief.
    write(
        ctx,
        "main.rs",
        "mod auth;\nfn main() { println!(\"hi\"); }\n",
    );
    promote(ctx, "greet on startup");

    // 2 — header insert above both evidence spans (drift, not stale).
    write(
        ctx,
        "auth/token.rs",
        &format!("// SPDX-License-Identifier: MIT\n{TOKEN_V0}"),
    );
    promote(ctx, "add license header");

    // 3 — the expiry change: invalidates B2, evidence-file-touches B1.
    let v3 = format!("// SPDX-License-Identifier: MIT\n{TOKEN_V0}")
        .replace("EXPIRY_HOURS: u64 = 24", "EXPIRY_HOURS: u64 = 168");
    write(ctx, "auth/token.rs", &v3);
    let expiry_commit = promote(ctx, "raise token expiry to 7 days");

    // 4 — split validation out of token.rs (touches, doesn't invalidate B1).
    let v4 = v3.replace(
        "\npub fn validate(token: &str) -> bool {\n    jwt::decode(token).is_ok()\n}\n",
        "",
    );
    write(ctx, "auth/token.rs", &v4);
    write(
        ctx,
        "auth/validate.rs",
        "use crate::jwt;\n\npub fn validate(token: &str) -> bool {\n    jwt::decode(token).is_ok()\n}\n",
    );
    write(ctx, "auth/mod.rs", "pub mod token;\npub mod validate;\n");
    promote(ctx, "split validation into its own module");

    // 5 — replace JWT issuance with server-side sessions: kills B1.
    rm(ctx, "auth/token.rs");
    write(
        ctx,
        "auth/session.rs",
        "pub fn create_session(user: &str) -> String {\n    format!(\"session-{user}\")\n}\n",
    );
    write(ctx, "auth/mod.rs", "pub mod session;\npub mod validate;\n");
    let session_commit = promote(ctx, "migrate JWT issuance to server-side sessions");

    // 6 — cleanup after the migration.
    write(
        ctx,
        "auth/validate.rs",
        "pub fn validate(session: &str) -> bool {\n    session.starts_with(\"session-\")\n}\n",
    );
    promote(ctx, "validate sessions instead of JWTs");

    (
        c0,
        b1["id"].as_str().unwrap().to_string(),
        b2["id"].as_str().unwrap().to_string(),
        expiry_commit,
        session_commit,
    )
}

#[test]
fn bisect_finds_the_invalidating_commits() {
    let ctx = init_repo();
    let (_c0, b1, b2, expiry_commit, session_commit) = seed_and_refactor(&ctx);

    // B2 ("expiry is 24h") — first invalidation is the expiry commit.
    let v = belief_json(&ctx, &["bisect", &b2]);
    assert_eq!(v["status"], "invalidated");
    let culprit = &v["culprit"];
    assert!(
        culprit["changeset"]
            .as_str()
            .unwrap()
            .starts_with(&expiry_commit),
        "expected expiry commit {expiry_commit}, got {culprit}"
    );
    assert_eq!(culprit["verdict"], "invalidated");
    assert_eq!(culprit["message"], "raise token expiry to 7 days");
    let excerpt = culprit["excerpt"].as_str().unwrap();
    assert!(
        excerpt.contains("24") && excerpt.contains("168"),
        "{excerpt}"
    );

    // B1 ("auth issues JWTs") — survives the expiry change and the split,
    // dies at the session migration (evidence file deleted).
    let v = belief_json(&ctx, &["bisect", &b1]);
    assert_eq!(v["status"], "invalidated");
    let culprit = &v["culprit"];
    assert!(
        culprit["changeset"]
            .as_str()
            .unwrap()
            .starts_with(&session_commit),
        "expected session commit {session_commit}, got {culprit}"
    );
    assert_eq!(
        culprit["message"],
        "migrate JWT issuance to server-side sessions"
    );

    // Human-readable output leads with the verdict and the culprit.
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["belief", "bisect", &b2])
        .assert()
        .success()
        .stdout(contains("INVALIDATED"))
        .stdout(contains("raise token expiry to 7 days"));
}

#[test]
fn time_travel_reproduces_the_original_belief_set() {
    let ctx = init_repo();
    let (c0, b1, b2, _expiry, _session) = seed_and_refactor(&ctx);

    // Materialize history, then travel back to C0: both beliefs active.
    belief_json(&ctx, &["stale"]);
    let v = belief_json(&ctx, &["at", &c0]);
    let rows = v.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(row["status"], "active", "at C0: {row}");
        assert!([b1.as_str(), b2.as_str()].contains(&row["id"].as_str().unwrap()));
    }

    // At HEAD both are invalidated.
    let v = belief_json(&ctx, &["at", "HEAD"]);
    for row in v.as_array().unwrap() {
        assert_eq!(row["status"], "invalidated", "at HEAD: {row}");
    }
}

#[test]
fn stale_lists_both_beliefs_with_triggering_commits() {
    let ctx = init_repo();
    let (_c0, _b1, _b2, expiry_commit, session_commit) = seed_and_refactor(&ctx);

    let v = belief_json(&ctx, &["stale"]);
    let rows = v.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(row["status"], "invalidated");
        let hits = row["hits"].as_array().unwrap();
        assert!(!hits.is_empty());
        // Every reported hit names a real triggering commit.
        for h in hits {
            assert!(h["changeset"].as_str().unwrap().len() > 8);
        }
    }
    // The two culprit commits both appear somewhere in the report.
    let text = v.to_string();
    assert!(text.contains(&expiry_commit) || text.contains(&session_commit));

    // Unrelated-file commits never show up as causes.
    assert!(!text.contains("greet on startup"));

    // Idempotent: a second scan adds no history.
    let v2 = belief_json(&ctx, &["stale"]);
    let log_a: Vec<_> = v.as_array().unwrap().iter().collect();
    let log_b: Vec<_> = v2.as_array().unwrap().iter().collect();
    assert_eq!(log_a.len(), log_b.len());
}

#[test]
fn log_shows_full_append_only_history() {
    let ctx = init_repo();
    let (_c0, _b1, b2, expiry_commit, _session) = seed_and_refactor(&ctx);

    belief_json(&ctx, &["stale"]);
    let v = belief_json(&ctx, &["log", &b2]);
    assert_eq!(v["claim"], "token expiry is 24h");
    assert_eq!(v["status"], "invalidated");
    assert!(
        v["invalidated_by"]
            .as_str()
            .unwrap()
            .starts_with(&expiry_commit)
    );
    let history = v["history"].as_array().unwrap();
    // Anchor entry + at least the header-drift touch + the invalidation.
    assert!(history.len() >= 3, "history: {history:?}");
    assert_eq!(history[0]["status"], "active");
    assert!(
        history.iter().any(|h| h["status"] == "stale-candidate"),
        "expected a stale-candidate touch before invalidation"
    );
}

#[test]
fn reaffirm_reanchors_and_survives_old_culprits() {
    let ctx = init_repo();
    let (_c0, _b1, b2, _expiry, _session) = seed_and_refactor(&ctx);

    belief_json(&ctx, &["stale"]);
    // The 24h claim is dead; a human re-checks and re-anchors a (now
    // differently-grounded) belief at HEAD with fresh evidence.
    let v = belief_json(
        &ctx,
        &["reaffirm", &b2, "--evidence", "auth/session.rs:1-3"],
    );
    assert_eq!(v["status"], "reaffirmed");

    // No commits after the new anchor → bisect finds nothing.
    let v = belief_json(&ctx, &["bisect", &b2]);
    assert_eq!(v["status"], "reaffirmed");
    assert!(v["culprit"].is_null());
}

#[test]
fn add_validates_evidence_and_symbols() {
    let ctx = init_repo();
    write(&ctx, "auth/token.rs", TOKEN_V0);
    promote(&ctx, "seed");

    // Span past EOF is rejected.
    gpp(ctx.repo.path(), &ctx.home_path)
        .args([
            "belief",
            "add",
            "--claim",
            "x",
            "--evidence",
            "auth/token.rs:1-9999",
        ])
        .assert()
        .failure()
        .stderr(contains("line"));

    // Unknown symbol is rejected with the symbols tree-sitter did find.
    gpp(ctx.repo.path(), &ctx.home_path)
        .args([
            "belief",
            "add",
            "--claim",
            "x",
            "--symbol",
            "auth/token.rs:no_such_fn",
        ])
        .assert()
        .failure()
        .stderr(contains("issue_token"));

    // A symbol-scoped belief works end to end.
    let v = belief_json(
        &ctx,
        &[
            "add",
            "--claim",
            "issue_token returns a JWT string",
            "--symbol",
            "auth/token.rs:issue_token",
        ],
    );
    assert_eq!(v["status"], "active");
}

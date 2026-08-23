//! End-to-end: agent-written beliefs over the real MCP stdio server, the
//! freshness envelope in `graphex_query`, and event-driven invalidation
//! landing in the inbox.
//!
//! Flow: promote C0 → agent calls `propose_belief` (lands Proposed, invisible
//! to `graphex_query`) → human `graphex accept` → evidence line changes in
//! C1 → `belief stale` flips it to invalidated exactly once and emits
//! `belief.invalidated` → `graphex_query` shows the envelope with the culprit.

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

/// Drive the MCP server with one `tools/call` and return the text result.
fn mcp_call(ctx: &Ctx, tool: &str, args: serde_json::Value) -> serde_json::Value {
    let script = format!(
        "{}\n{}\n{}\n",
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
                           "params":{"name":tool,"arguments":args}}),
    );
    let out = gpp(ctx.repo.path(), &ctx.home_path)
        .args(["mcp-server", "--stdio"])
        .write_stdin(script)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let resp = text
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["id"] == 2)
        .expect("tools/call response");
    resp["result"].clone()
}

fn mcp_text(v: &serde_json::Value) -> String {
    v["content"][0]["text"].as_str().unwrap_or("").to_string()
}

#[test]
fn agent_proposed_belief_is_gated_policed_and_surfaced() {
    let ctx = init_repo();
    std::fs::create_dir_all(ctx.repo.path().join("auth")).unwrap();
    std::fs::write(
        ctx.repo.path().join("auth/token.rs"),
        "pub const EXPIRY_HOURS: u64 = 24;\npub fn issue() {}\n",
    )
    .unwrap();
    let c0 = promote(&ctx, "seed auth");

    // The tool is advertised.
    let listed = gpp(ctx.repo.path(), &ctx.home_path)
        .args(["mcp-server", "--stdio"])
        .write_stdin("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n")
        .assert()
        .success();
    assert!(String::from_utf8_lossy(&listed.get_output().stdout).contains("\"propose_belief\""));

    // Bad evidence is rejected with the same message a human would get.
    let r = mcp_call(
        &ctx,
        "propose_belief",
        serde_json::json!({"claim":"x","evidence":["auth/token.rs:40-41"]}),
    );
    assert_eq!(r["isError"], true);
    assert!(mcp_text(&r).contains("only 3 line(s)"), "{r}");

    // A well-formed belief lands Proposed, anchored at C0.
    let r = mcp_call(
        &ctx,
        "propose_belief",
        serde_json::json!({"claim":"token expiry is 24h",
                           "evidence":["auth/token.rs:1-1"],
                           "symbols":["auth/token.rs:issue"]}),
    );
    assert_ne!(r["isError"], true, "{r}");
    let text = mcp_text(&r);
    assert!(text.contains("proposed belief"), "{text}");
    assert!(text.contains(&format!("cs:{}", &c0[..8])), "{text}");

    // Invisible to the agent's own context until a human accepts it …
    let ctx_text = mcp_text(&mcp_call(&ctx, "graphex_query", serde_json::json!({})));
    assert!(!ctx_text.contains("token expiry"), "{ctx_text}");
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["graphex", "pending"])
        .assert()
        .success()
        .stdout(contains("token expiry is 24h"));
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["graphex", "accept", "token expiry is 24h"])
        .assert()
        .success();

    // … after which it carries a freshness envelope.
    let ctx_text = mcp_text(&mcp_call(&ctx, "graphex_query", serde_json::json!({})));
    assert!(
        ctx_text.contains(&format!("token expiry is 24h [anchored cs:{}", &c0[..8])),
        "{ctx_text}"
    );
    assert!(ctx_text.contains("0 commits since"), "{ctx_text}");

    // An unrelated file changes: the evidence *file* is untouched, so no
    // transition, no event, and the promote output says nothing about it.
    std::fs::write(ctx.repo.path().join("main.rs"), "fn main() {}\n").unwrap();
    let out = gpp(ctx.repo.path(), &ctx.home_path)
        .args(["promote", "-m", "add main"])
        .assert()
        .success();
    assert!(!String::from_utf8_lossy(&out.get_output().stdout).contains("belief "));

    // History moves against the evidence. `promote` itself now runs the
    // scan: the transition is reported inline and the event is emitted
    // without any hook or manual `belief stale`.
    std::fs::write(
        ctx.repo.path().join("auth/token.rs"),
        "pub const EXPIRY_HOURS: u64 = 168;\npub fn issue() {}\n",
    )
    .unwrap();
    let out = gpp(ctx.repo.path(), &ctx.home_path)
        .args(["promote", "-m", "raise expiry to 7 days"])
        .assert()
        .success()
        .stdout(contains("belief invalidated"))
        .stdout(contains("token expiry is 24h"));
    let text = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let idx = text.find("cs:").unwrap();
    let c1 = text[idx + 3..]
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();

    // Manual rescans afterwards change nothing and emit nothing.
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["belief", "stale"])
        .assert()
        .success()
        .stdout(contains("invalidated"));
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["belief", "stale"])
        .assert()
        .success();

    // An agent cannot bless an invalidated belief back — grounds are gone.
    let r = mcp_call(
        &ctx,
        "reaffirm_belief",
        serde_json::json!({"belief":"token expiry is 24h"}),
    );
    assert_eq!(r["isError"], true);
    assert!(mcp_text(&r).contains("cannot"), "{r}");
    let inbox = gpp(ctx.repo.path(), &ctx.home_path)
        .arg("inbox")
        .assert()
        .success();
    let inbox = String::from_utf8_lossy(&inbox.get_output().stdout).to_string();
    assert_eq!(
        inbox.matches("belief.invalidated").count(),
        1,
        "one event per transition (promote), not per rescan:\n{inbox}"
    );
    assert!(
        inbox.contains("raise expiry") || inbox.contains(&c1[..8]),
        "{inbox}"
    );

    // The agent now sees the verdict and its culprit in-line.
    let ctx_text = mcp_text(&mcp_call(&ctx, "graphex_query", serde_json::json!({})));
    assert!(
        ctx_text.contains(&format!(
            "token expiry is 24h ✗ [INVALIDATED at cs:{}",
            &c1[..8]
        )),
        "{ctx_text}"
    );
}

#[test]
fn agent_can_reaffirm_a_stale_candidate_but_not_an_invalidated_belief() {
    let ctx = init_repo();
    std::fs::create_dir_all(ctx.repo.path().join("auth")).unwrap();
    std::fs::write(
        ctx.repo.path().join("auth/token.rs"),
        "pub const EXPIRY_HOURS: u64 = 24;\npub fn issue() {}\n",
    )
    .unwrap();
    promote(&ctx, "seed");
    let r = mcp_call(
        &ctx,
        "propose_belief",
        serde_json::json!({"claim":"expiry is 24h","evidence":["auth/token.rs:1-1"]}),
    );
    assert_ne!(r["isError"], true, "{r}");
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["graphex", "accept", "expiry is 24h"])
        .assert()
        .success();

    // Touch the evidence *file* below the span: stale-candidate, not invalidated.
    std::fs::write(
        ctx.repo.path().join("auth/token.rs"),
        "pub const EXPIRY_HOURS: u64 = 24;\npub fn issue() {}\npub fn revoke() {}\n",
    )
    .unwrap();
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["promote", "-m", "add revoke"])
        .assert()
        .success()
        .stdout(contains("belief stale-candidate"));

    // The agent re-checks, and may reaffirm: re-anchored at HEAD, back to clean.
    let r = mcp_call(
        &ctx,
        "reaffirm_belief",
        serde_json::json!({"belief":"expiry is 24h"}),
    );
    assert_ne!(r["isError"], true, "{r}");
    assert!(mcp_text(&r).contains("re-anchored"), "{r}");
    let ctx_text = mcp_text(&mcp_call(&ctx, "graphex_query", serde_json::json!({})));
    assert!(
        ctx_text.contains("expiry is 24h [anchored cs:"),
        "{ctx_text}"
    );
    assert!(!ctx_text.contains("stale candidate"), "{ctx_text}");
    gpp(ctx.repo.path(), &ctx.home_path)
        .args(["belief", "log", "expiry is 24h"])
        .assert()
        .success()
        .stdout(contains("reaffirmed by agent:mcp-client"));
}

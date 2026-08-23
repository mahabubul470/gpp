//! P0.5 — cross-layer integration tests. Every crate has its own isolated
//! suite; these drive the real `gpp` binary across layer boundaries:
//!
//! 1. two-peer sync round-trip over a live `sync serve` socket
//! 2. git-import → promote → git-export fidelity (history survives the bridge
//!    both ways, byte-for-byte content)
//! 3. promote → review → RBAC merge gate (unapproved blocked, approved merges)
//! 4. MCP query → propose_graph_update → human accept → query sees it
//!
//! Each test is self-contained (own HOME + repo dirs), so they run in
//! parallel and leave nothing behind.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command as StdCommand, Stdio};

use assert_cmd::Command;
use predicates::str::contains;

struct Home(tempfile::TempDir);

impl Home {
    /// A fresh HOME with a global gpp identity written directly.
    fn new(name: &str, email: &str) -> Self {
        let d = tempfile::tempdir().unwrap();
        let cfg = d.path().join(".config/gpp");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(
            cfg.join("config.toml"),
            format!("[user]\nname = \"{name}\"\nemail = \"{email}\"\n"),
        )
        .unwrap();
        Home(d)
    }
    fn path(&self) -> &std::path::Path {
        self.0.path()
    }
}

fn gpp(dir: &std::path::Path, home: &Home) -> Command {
    let mut c = Command::cargo_bin("gpp").unwrap();
    c.current_dir(dir)
        .env("HOME", home.path())
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("GPP_GRAPHEX_PASSPHRASE");
    c
}

fn init(dir: &std::path::Path, home: &Home, graphex: bool) {
    let mut c = gpp(dir, home);
    c.arg("init");
    if graphex {
        c.arg("--graphex");
    }
    c.assert().success();
}

fn promote(dir: &std::path::Path, home: &Home, msg: &str) -> String {
    let out = gpp(dir, home)
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

fn stdout_of(a: assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stdout).to_string()
}

// ---- 1. two-peer sync ------------------------------------------------------

/// `gpp sync serve 127.0.0.1:0` as a child; returns it plus the bound port.
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn serve(dir: &std::path::Path, home: &Home) -> (Server, String) {
    let mut child = StdCommand::new(assert_cmd::cargo::cargo_bin("gpp"))
        .current_dir(dir)
        .env("HOME", home.path())
        .env_remove("XDG_CONFIG_HOME")
        .args(["sync", "serve", "127.0.0.1:0"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut lines = BufReader::new(stderr).lines();
    let addr = loop {
        let line = lines.next().expect("serve prints its address").unwrap();
        if let Some(rest) = line.strip_prefix("serving syncs on ") {
            break rest.split_whitespace().next().unwrap().to_string();
        }
    };
    // Keep draining stderr in the background so the child never blocks.
    std::thread::spawn(move || for _ in lines {});
    (Server(child), addr)
}

#[test]
fn two_peer_sync_round_trip_over_a_live_socket() {
    let home = Home::new("Ann", "ann@example.com");
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    init(a.path(), &home, false);
    init(b.path(), &home, false);

    std::fs::write(a.path().join("lib.rs"), "pub fn from_a() {}\n").unwrap();
    let cs_a = promote(a.path(), &home, "work on A");

    let (_srv, addr) = serve(a.path(), &home);

    // A second peer of the *same* repository shares its repo id (there is no
    // `gpp clone` yet — bootstrapping is copying the id; see TODO).
    let id = std::fs::read(a.path().join(".gpp/sync/repo_id")).unwrap();
    std::fs::create_dir_all(b.path().join(".gpp/sync")).unwrap();
    std::fs::write(b.path().join(".gpp/sync/repo_id"), id).unwrap();

    gpp(b.path(), &home)
        .args(["sync", "add", "a", &addr])
        .assert()
        .success();
    gpp(b.path(), &home).arg("sync").assert().success();

    // B now has A's changeset and can read the file out of it.
    let log = stdout_of(gpp(b.path(), &home).arg("log").assert().success());
    assert!(log.contains("work on A"), "{log}");
    assert!(log.contains(&cs_a[..8]), "{log}");

    // And the reverse direction: B promotes, syncs again, A's log has it.
    std::fs::write(
        b.path().join("lib.rs"),
        "pub fn from_a() {}\npub fn from_b() {}\n",
    )
    .unwrap();
    promote(b.path(), &home, "work on B");
    gpp(b.path(), &home).arg("sync").assert().success();
    let log_a = stdout_of(gpp(a.path(), &home).arg("log").assert().success());
    assert!(log_a.contains("work on B"), "{log_a}");
}

// ---- 2. git bridge fidelity -------------------------------------------------

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = StdCommand::new("git")
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Git User")
        .env("GIT_AUTHOR_EMAIL", "git@example.com")
        .env("GIT_COMMITTER_NAME", "Git User")
        .env("GIT_COMMITTER_EMAIL", "git@example.com")
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn git_import_promote_export_round_trips_history_and_content() {
    let home = Home::new("Ann", "ann@example.com");
    let src = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();

    // A small Git history: two commits, one of them a nested path.
    git(src.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(src.path().join("README.md"), "# legacy\n").unwrap();
    git(src.path(), &["add", "."]);
    git(src.path(), &["commit", "-q", "-m", "legacy: readme"]);
    std::fs::create_dir_all(src.path().join("src")).unwrap();
    std::fs::write(
        src.path().join("src/main.rs"),
        "fn main() { println!(\"hi\"); }\n",
    )
    .unwrap();
    git(src.path(), &["add", "."]);
    git(src.path(), &["commit", "-q", "-m", "legacy: main"]);
    let git_head = git(src.path(), &["rev-parse", "HEAD"]).trim().to_string();

    // Import, then do native gpp work on top.
    init(work.path(), &home, false);
    gpp(work.path(), &home)
        .args(["git-import", src.path().to_str().unwrap()])
        .assert()
        .success();
    let log = stdout_of(gpp(work.path(), &home).arg("log").assert().success());
    assert!(
        log.contains("legacy: main") && log.contains("legacy: readme"),
        "{log}"
    );

    // The imported changeset carries the git SHA — the bridge's provenance link.
    assert!(
        log.contains(&format!("Git:     {git_head}")),
        "imported changeset should show git_commit:\n{log}"
    );

    // Native work in the gpp checkout (imported files are materialised there).
    assert!(
        work.path().join("src/main.rs").exists(),
        "import should materialise the tree"
    );
    std::fs::write(work.path().join("src/lib.rs"), "pub fn added_in_gpp() {}\n").unwrap();
    promote(work.path(), &home, "native: add lib");

    // Export into a fresh Git repo and check both history and bytes.
    gpp(work.path(), &home)
        .args(["git-export", out.path().to_str().unwrap()])
        .assert()
        .success();
    let glog = git(out.path(), &["log", "--format=%s", "main"]);
    assert!(glog.contains("native: add lib"), "{glog}");
    assert!(
        glog.contains("legacy: main") && glog.contains("legacy: readme"),
        "{glog}"
    );
    let lib = git(out.path(), &["show", "main:src/lib.rs"]);
    assert_eq!(lib, "pub fn added_in_gpp() {}\n");
    let main = git(out.path(), &["show", "main:src/main.rs"]);
    assert_eq!(main, "fn main() { println!(\"hi\"); }\n");
}

// ---- 3. promote → review → merge gate -----------------------------------------

#[test]
fn review_and_rbac_gate_a_merge_end_to_end() {
    let author = Home::new("Ann", "ann@example.com");
    let maintainer = Home::new("Max", "max@example.com");
    let repo = tempfile::tempdir().unwrap();
    init(repo.path(), &author, false);

    // Protect main: one human maintainer approval required.
    gpp(repo.path(), &maintainer)
        .args(["rbac", "assign", "max@example.com", "maintainer"])
        .assert()
        .success();
    gpp(repo.path(), &maintainer)
        .args(["rbac", "protect", "main", "--min-reviewers", "1"])
        .assert()
        .success();

    std::fs::write(repo.path().join("feature.rs"), "pub fn f() {}\n").unwrap();
    let cs = promote(repo.path(), &author, "add feature");

    // Promote auto-opened a review; pending → the review layer refuses first.
    gpp(repo.path(), &author)
        .args(["review", "show", &cs])
        .assert()
        .success()
        .stdout(contains("pending"));
    gpp(repo.path(), &maintainer)
        .args(["review", "merge", &cs])
        .assert()
        .failure()
        .stderr(contains("needs approval"));

    // A contributor approves (review layer satisfied: 1 human approval) but
    // may not merge a protected branch — that is the RBAC gate.
    let contributor = Home::new("Cat", "cat@example.com");
    gpp(repo.path(), &maintainer)
        .args(["rbac", "assign", "cat@example.com", "contributor"])
        .assert()
        .success();
    gpp(repo.path(), &contributor)
        .args(["review", "approve", &cs, "--reason", "LGTM"])
        .assert()
        .success();
    gpp(repo.path(), &contributor)
        .args(["review", "merge", &cs])
        .assert()
        .failure()
        .stderr(contains("merge blocked"));

    // The maintainer merges the approved review.
    gpp(repo.path(), &maintainer)
        .args(["review", "merge", &cs])
        .assert()
        .success()
        .stdout(contains("merged"));
    gpp(repo.path(), &author)
        .args(["review", "show", &cs])
        .assert()
        .success()
        .stdout(contains("merged"));
}

// ---- 4. MCP query → propose → accept ------------------------------------------

fn mcp_call(dir: &std::path::Path, home: &Home, tool: &str, args: serde_json::Value) -> String {
    let script = format!(
        "{}\n{}\n",
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
                           "params":{"name":tool,"arguments":args}}),
    );
    let out = gpp(dir, home)
        .args(["mcp-server", "--stdio"])
        .write_stdin(script)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let resp = String::from_utf8(out)
        .unwrap()
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["id"] == 2)
        .expect("tools/call response");
    assert_ne!(resp["result"]["isError"], true, "{resp}");
    resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn mcp_query_propose_accept_loop_across_sdk_graphex_and_cli() {
    let home = Home::new("Ann", "ann@example.com");
    let repo = tempfile::tempdir().unwrap();
    init(repo.path(), &home, true);
    std::fs::write(repo.path().join("queue.rs"), "pub struct RetryQueue;\n").unwrap();
    promote(repo.path(), &home, "seed");

    // Agent: context is empty of the module, proposes it.
    let before = mcp_call(repo.path(), &home, "graphex_query", serde_json::json!({}));
    assert!(!before.contains("retry-queue"), "{before}");
    let r = mcp_call(
        repo.path(),
        &home,
        "propose_graph_update",
        serde_json::json!({"node_type":"module","name":"retry-queue",
                           "description":"backoff retry queue for outbound jobs"}),
    );
    assert!(r.contains("awaiting human approval"), "{r}");

    // Still invisible to the agent; visible to the human as pending.
    let mid = mcp_call(repo.path(), &home, "graphex_query", serde_json::json!({}));
    assert!(!mid.contains("retry-queue"), "{mid}");
    gpp(repo.path(), &home)
        .args(["graphex", "pending"])
        .assert()
        .success()
        .stdout(contains("retry-queue"));

    // Human accepts; the next agent query carries it, and the audit log
    // shows both the proposal and the projection read.
    gpp(repo.path(), &home)
        .args(["graphex", "accept", "retry-queue"])
        .assert()
        .success();
    let after = mcp_call(repo.path(), &home, "graphex_query", serde_json::json!({}));
    assert!(
        after.contains("retry-queue") && after.contains("backoff retry queue"),
        "{after}"
    );
    let audit = stdout_of(
        gpp(repo.path(), &home)
            .args(["graphex", "audit"])
            .assert()
            .success(),
    );
    assert!(audit.contains("propose_update"), "{audit}");
    assert!(audit.contains("agent:mcp-client"), "{audit}");
}

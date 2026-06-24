//! End-to-end test for Graphex-driven reviewer assignment.
//!
//! A changeset's suggested reviewers should include the people the knowledge
//! graph names as owners (`owned-by` edges) of the touched modules — not just
//! RBAC maintainers. Here `dana@x.io` owns the `payments` module and holds no
//! RBAC role at all, yet a change under `src/payments/` notifies her.

use assert_cmd::Command;
use predicates::str::contains;

fn gpp(dir: &std::path::Path, home: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("gpp").unwrap();
    c.current_dir(dir)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME");
    c
}

#[test]
fn graphex_owner_is_suggested_as_reviewer() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let h = home.path();
    let r = repo.path();

    gpp(r, h).args(["init", "--graphex"]).assert().success();
    // The inbox we inspect is whoami()'s — make that the owner.
    gpp(r, h)
        .args(["config", "--global", "set", "user.email", "dana@x.io"])
        .assert()
        .success();

    // Graph: module `payments` owned-by dana@x.io (dana has no RBAC role).
    gpp(r, h)
        .args([
            "graphex", "add", "--type", "module", "--name", "payments", "-d", "payments",
        ])
        .assert()
        .success();
    gpp(r, h)
        .args([
            "graphex",
            "add",
            "--type",
            "person",
            "--name",
            "dana@x.io",
            "-d",
            "owner",
        ])
        .assert()
        .success();
    gpp(r, h)
        .args([
            "graphex",
            "link",
            "payments",
            "--relation",
            "owned-by",
            "--to",
            "dana@x.io",
        ])
        .assert()
        .success();

    // Touch the payments module and promote.
    std::fs::create_dir_all(r.join("src/payments")).unwrap();
    std::fs::write(r.join("src/payments/mod.rs"), "fn pay() {}\n").unwrap();
    gpp(r, h)
        .args(["promote", "-m", "add pay()"])
        .assert()
        .success();

    // dana, the graph owner, was notified as a suggested reviewer.
    gpp(r, h)
        .arg("inbox")
        .assert()
        .success()
        .stdout(contains("changeset.promoted"));
}

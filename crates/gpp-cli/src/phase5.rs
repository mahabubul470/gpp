//! Phase 5 commands: `gpp sync`, `gpp replay`, `gpp merge`.

use std::collections::BTreeMap;
use std::net::TcpListener;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use gpp_core::{Hash, ObjectStore};
use gpp_history::{Author, Changeset, Intent, IntentType, RefStore};
use gpp_sync::{SyncOptions, SyncReport};

use crate::cli::{MergeArgs, ReplayArgs, SyncArgs, SyncSub};
use crate::config;
use crate::repo::Repo;

fn discover(repo_override: Option<&Path>) -> Result<Repo> {
    let start = match repo_override {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    Repo::discover(&start)
}

fn config_author() -> Author {
    if let Ok(path) = config::global_config_path() {
        let doc = config::load_doc(&path).unwrap_or(toml::Value::Table(Default::default()));
        let name = config::get_key(&doc, "user.name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();
        let email = config::get_key(&doc, "user.email")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown@localhost")
            .to_string();
        Author::human(name, email)
    } else {
        Author::human("Unknown", "unknown@localhost")
    }
}

fn now_micros() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

// ---- peers.toml ------------------------------------------------------------

fn peers_path(repo: &Repo) -> std::path::PathBuf {
    repo.gpp_dir().join("sync").join("peers.toml")
}

/// `(name, address)` pairs from `.gpp/sync/peers.toml`.
pub(crate) fn load_peers(repo: &Repo) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(peers_path(repo)) else {
        return Vec::new();
    };
    let Ok(val) = text.parse::<toml::Value>() else {
        return Vec::new();
    };
    val.get("peer")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    Some((
                        e.get("name")?.as_str()?.to_string(),
                        e.get("address")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn save_peers(repo: &Repo, peers: &[(String, String)]) -> Result<()> {
    let mut s = String::new();
    for (n, a) in peers {
        s.push_str(&format!("[[peer]]\nname = {n:?}\naddress = {a:?}\n\n"));
    }
    let path = peers_path(repo);
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, s)?;
    Ok(())
}

pub(crate) fn print_report(name: &str, r: &SyncReport) {
    println!(
        "synced {name}: ↓{} objects, {} refs adopted, {} forks, {} policies, {} graph rows",
        r.objects_received, r.refs_adopted, r.forks_created, r.policies_added, r.graph_rows_merged
    );
}

pub fn sync(args: &SyncArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let gpp = repo.gpp_dir();
    let opts = SyncOptions {
        graph_only: args.graph_only,
        include_graphex: args.include_graphex,
    };

    match &args.action {
        Some(SyncSub::Add { name, address }) => {
            let mut peers = load_peers(&repo);
            peers.retain(|(n, _)| n != name);
            peers.push((name.clone(), address.clone()));
            save_peers(&repo, &peers)?;
            println!("added peer {name} → {address}");
            Ok(())
        }
        Some(SyncSub::Remove { name }) => {
            let mut peers = load_peers(&repo);
            let before = peers.len();
            peers.retain(|(n, _)| n != name);
            if peers.len() == before {
                bail!("no peer named {name:?}");
            }
            save_peers(&repo, &peers)?;
            println!("removed peer {name}");
            Ok(())
        }
        Some(SyncSub::Status) => {
            let peers = load_peers(&repo);
            if peers.is_empty() {
                println!("(no peers — add one with `gpp sync add <name> <addr>`)");
            }
            for (n, a) in peers {
                println!("{n:<16} {a}");
            }
            let id = gpp_sync::ensure_repo_id(&gpp).map_err(|e| anyhow!("{e}"))?;
            println!("repo id: {id}");
            Ok(())
        }
        Some(SyncSub::Serve { address }) => {
            gpp_sync::ensure_repo_id(&gpp).map_err(|e| anyhow!("{e}"))?;
            let listener =
                TcpListener::bind(address).with_context(|| format!("binding {address}"))?;
            eprintln!(
                "serving syncs on {} … (Ctrl-C to stop)",
                listener.local_addr()?
            );
            for stream in listener.incoming() {
                let stream = stream?;
                let who = stream
                    .peer_addr()
                    .map(|a| a.to_string())
                    .unwrap_or_else(|_| "peer".into());
                match gpp_sync::serve(stream, &gpp, &who, opts) {
                    Ok(r) => print_report(&who, &r),
                    Err(e) => eprintln!("sync from {who} failed: {e}"),
                }
            }
            Ok(())
        }
        Some(SyncSub::Peer { name }) => {
            let addr = load_peers(&repo)
                .into_iter()
                .find(|(n, _)| n == name)
                .map(|(_, a)| a)
                .ok_or_else(|| anyhow!("no peer named {name:?}"))?;
            let r = gpp_sync::connect(&addr, &gpp, name, opts).map_err(|e| anyhow!("{e}"))?;
            print_report(name, &r);
            Ok(())
        }
        None => {
            let peers = load_peers(&repo);
            if peers.is_empty() {
                bail!("no peers configured (use `gpp sync add` or `gpp sync serve`)");
            }
            for (name, addr) in peers {
                match gpp_sync::connect(&addr, &gpp, &name, opts) {
                    Ok(r) => print_report(&name, &r),
                    Err(e) => eprintln!("sync with {name} failed: {e}"),
                }
            }
            Ok(())
        }
    }
}

// ---- gpp replay ------------------------------------------------------------

fn resolve_cs(repo: &Repo, spec: &str) -> Result<Hash> {
    let refs = RefStore::open(&repo.gpp_dir());
    let s = spec.strip_prefix("cs:").unwrap_or(spec);
    if s.eq_ignore_ascii_case("HEAD") {
        return refs
            .head_tip()?
            .ok_or_else(|| anyhow!("HEAD has no changesets"));
    }
    if let Ok(h) = Hash::from_base32(s) {
        return Ok(h);
    }
    refs.read_ref(s)?
        .ok_or_else(|| anyhow!("cannot resolve {spec:?}"))
}

fn detect_toolchain() -> BTreeMap<String, String> {
    let mut tc = BTreeMap::new();
    for (tool, args) in [("rustc", ["--version"]), ("cargo", ["--version"])] {
        if let Ok(out) = std::process::Command::new(tool).args(args).output()
            && out.status.success()
        {
            tc.insert(
                tool.to_string(),
                String::from_utf8_lossy(&out.stdout).trim().to_string(),
            );
        }
    }
    tc
}

pub fn replay(args: &ReplayArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let store = ObjectStore::open(&repo.gpp_dir());
    let cs = resolve_cs(&repo, &args.changeset)?;

    let mut env = BTreeMap::new();
    for kv in &args.env {
        let (k, v) = kv
            .split_once('=')
            .ok_or_else(|| anyhow!("--env {kv:?} must be key=value"))?;
        env.insert(k.to_string(), v.to_string());
    }

    let snap =
        gpp_replay::snapshot(&store, &cs, detect_toolchain(), env).map_err(|e| anyhow!("{e}"))?;
    println!(
        "snapshot cs:{} for changeset cs:{}",
        snap.short(),
        cs.short()
    );

    if args.dry_run {
        let files =
            gpp_replay::replay(&store, &snap, &args.output, true).map_err(|e| anyhow!("{e}"))?;
        println!("would reproduce {} file(s):", files.len());
        for f in files {
            println!("  {f}");
        }
        return Ok(());
    }

    if args.diff {
        let drift =
            gpp_replay::diff_against(&store, &snap, &args.output).map_err(|e| anyhow!("{e}"))?;
        if drift.is_empty() {
            println!("✓ {} matches the snapshot exactly", args.output.display());
        } else {
            println!("{} path(s) drifted from the snapshot:", drift.len());
            for d in drift {
                println!("  ~ {d}");
            }
        }
        return Ok(());
    }

    let files =
        gpp_replay::replay(&store, &snap, &args.output, false).map_err(|e| anyhow!("{e}"))?;
    let drift =
        gpp_replay::diff_against(&store, &snap, &args.output).map_err(|e| anyhow!("{e}"))?;
    println!(
        "reproduced {} file(s) into {} ({})",
        files.len(),
        args.output.display(),
        if drift.is_empty() {
            "verified".into()
        } else {
            format!("{} drifted", drift.len())
        }
    );
    Ok(())
}

// ---- gpp merge -------------------------------------------------------------

pub fn merge(args: &MergeArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let store = ObjectStore::open(&repo.gpp_dir());
    let refs = RefStore::open(&repo.gpp_dir());

    let branch = repo.current_branch()?;
    let ours = refs
        .read_ref(&branch)?
        .ok_or_else(|| anyhow!("current branch {branch:?} has no tip"))?;
    let theirs = refs
        .read_ref(&args.fork_ref)?
        .ok_or_else(|| anyhow!("no fork ref {:?}", args.fork_ref))?;
    if ours == theirs {
        println!("already up to date");
        return Ok(());
    }

    // Merge changeset: two parents, taking the fork's tree (explicit, no
    // silent content merge — the developer reviews afterward).
    let their_cs: Changeset = store.read(&theirs)?;
    let ts = now_micros();
    let intent = Intent {
        intent_type: IntentType::HumanDirected,
        description: format!("merge {} into {}", args.fork_ref, branch),
        prompt: None,
        task_reference: None,
        goal: None,
        constraints: Vec::new(),
        timestamp: ts,
    };
    let intent_id = store.write(&intent)?;
    let merged = Changeset {
        parents: vec![ours, theirs],
        tree: their_cs.tree,
        timestamp: ts,
        author: config_author(),
        committer: None,
        intent: Some(intent_id),
        timeline_range: None,
        metadata: Default::default(),
    };
    let id = store.write(&merged)?;
    refs.write_ref(&branch, id)?;
    refs.delete_ref(&args.fork_ref).ok();
    println!(
        "merged {} into {branch} → cs:{} (fork ref removed)",
        args.fork_ref,
        id.short()
    );
    println!("note: tree taken from the fork; review and adjust as needed");
    Ok(())
}

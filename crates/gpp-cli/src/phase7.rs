//! Phase 7 commands: `gpp remote` (platform integration) and `gpp relay`
//! (client-side relay/peer management).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use gpp_core::{Blob, EntryKind, Hash, ObjectStore, Tree};
use gpp_history::{AuthorType, Changeset, RefStore};
use gpp_remote::{
    Enrichment, GenericGitRemote, Platform, PrRequest, RemoteConfig, ReqwestClient, create_pr,
    fetch_ci_status, fetch_pr_reviews,
};

use crate::cli::{RelayAction, RelayArgs, RemoteAction, RemoteArgs};
use crate::repo::Repo;

fn discover(repo_override: Option<&Path>) -> Result<Repo> {
    let start = match repo_override {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    Repo::discover(&start)
}

fn flatten(store: &ObjectStore, root: &Hash) -> Result<BTreeMap<String, Hash>> {
    fn walk(
        s: &ObjectStore,
        h: &Hash,
        prefix: &str,
        out: &mut BTreeMap<String, Hash>,
    ) -> Result<()> {
        let t: Tree = s.read(h)?;
        for e in t.entries {
            let path = if prefix.is_empty() {
                e.name.clone()
            } else {
                format!("{prefix}/{}", e.name)
            };
            match e.kind {
                EntryKind::Directory => walk(s, &e.hash, &path, out)?,
                EntryKind::File | EntryKind::Symlink => {
                    out.insert(path, e.hash);
                }
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(store, root, "", &mut out)?;
    Ok(out)
}

/// Semantic-change one-liners for HEAD vs. its first parent.
fn head_semantic_summary(repo: &Repo) -> Vec<String> {
    let store = ObjectStore::open(&repo.gpp_dir());
    let refs = RefStore::open(&repo.gpp_dir());
    let Ok(Some(tip)) = refs.head_tip() else {
        return Vec::new();
    };
    let Ok(cs) = store.read::<Changeset>(&tip) else {
        return Vec::new();
    };
    let new = flatten(&store, &cs.tree).unwrap_or_default();
    let old = cs
        .parents
        .first()
        .and_then(|p| store.read::<Changeset>(p).ok())
        .map(|pc| flatten(&store, &pc.tree).unwrap_or_default())
        .unwrap_or_default();

    let mut out = Vec::new();
    let mut paths: Vec<&String> = new.keys().chain(old.keys()).collect();
    paths.sort();
    paths.dedup();
    for p in paths {
        if new.get(p) == old.get(p) {
            continue;
        }
        let nb = new
            .get(p)
            .and_then(|h| store.read::<Blob>(h).ok())
            .map(|b| b.content)
            .unwrap_or_default();
        let ob = old
            .get(p)
            .and_then(|h| store.read::<Blob>(h).ok())
            .map(|b| b.content)
            .unwrap_or_default();
        if let Ok(d) = gpp_diff::semantic(p, &ob, &nb) {
            for op in d.ops.iter().take(5) {
                out.push(match op {
                    gpp_diff::ChangeOp::Added(x) => format!("+ {} {} in {p}", x.kind, x.name),
                    gpp_diff::ChangeOp::Removed(x) => format!("- {} {} in {p}", x.kind, x.name),
                    gpp_diff::ChangeOp::Modified { new, .. } => {
                        format!("~ {} {} in {p}", new.kind, new.name)
                    }
                    gpp_diff::ChangeOp::Renamed { old, new } => {
                        format!("» {} → {} in {p}", old.name, new.name)
                    }
                });
            }
        }
        if out.len() >= 20 {
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// gpp remote
// ---------------------------------------------------------------------------

pub fn remote(args: &RemoteArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let gpp = repo.gpp_dir();

    match &args.action {
        RemoteAction::Setup {
            platform,
            repository,
            token_env,
            remote_url,
        } => {
            let cfg = RemoteConfig {
                platform: Platform::parse(platform).map_err(|e| anyhow!("{e}"))?,
                api_token_env: token_env.clone(),
                repository: repository.clone(),
                remote_url: remote_url.clone(),
            };
            cfg.save(&gpp).map_err(|e| anyhow!("{e}"))?;
            println!(
                "remote configured: {} {} (token env ${})",
                platform, repository, token_env
            );
            Ok(())
        }
        RemoteAction::Status => {
            let cfg = RemoteConfig::load(&gpp).map_err(|e| anyhow!("{e}"))?;
            println!("platform:   {}", cfg.platform.as_str());
            println!("repository: {}", cfg.repository);
            println!("token env:  ${}", cfg.api_token_env);
            println!("remote url: {}", cfg.remote_url);
            println!(
                "token:      {}",
                if std::env::var(&cfg.api_token_env).is_ok() {
                    "present"
                } else {
                    "MISSING (set the env var to create PRs)"
                }
            );
            Ok(())
        }
        RemoteAction::PrCreate { base, head, title } => {
            let cfg = RemoteConfig::load(&gpp).map_err(|e| anyhow!("{e}"))?;
            if cfg.platform == Platform::Generic {
                bail!("platform is 'generic' — use `gpp remote push` instead");
            }
            let token = std::env::var(&cfg.api_token_env).map_err(|_| {
                anyhow!("${} is not set (needed for PR creation)", cfg.api_token_env)
            })?;
            let branch = match head {
                Some(h) => h.clone(),
                None => repo.current_branch().unwrap_or_else(|_| "main".into()),
            };
            let refs = RefStore::open(&gpp);
            let store = ObjectStore::open(&gpp);
            let tip = refs
                .head_tip()?
                .ok_or_else(|| anyhow!("no changesets to open a PR for"))?;
            let cs: Changeset = store.read(&tip)?;
            let intent = cs
                .intent
                .and_then(|i| store.read::<gpp_history::Intent>(&i).ok());
            let message = intent
                .as_ref()
                .map(|i| i.description.clone())
                .unwrap_or_else(|| "gpp changeset".into());
            let pr_title = title.clone().unwrap_or_else(|| {
                message
                    .lines()
                    .next()
                    .unwrap_or("gpp changeset")
                    .to_string()
            });
            let enrich = Enrichment {
                intent: intent.as_ref().map(|i| format!("{:?}", i.intent_type)),
                semantic_summary: head_semantic_summary(&repo),
                agent: (cs.author.author_type == AuthorType::Agent)
                    .then(|| cs.author.identity.clone()),
                policy_results: vec![],
                cost_usd: None,
                trust: None,
            };
            let body = gpp_remote::pr_body(&pr_title, &message, &enrich);
            let result = create_pr(
                cfg.platform,
                &cfg.repository,
                &token,
                &PrRequest {
                    title: pr_title,
                    body,
                    head: branch,
                    base: base.clone(),
                },
                &ReqwestClient {
                    auth_header: if cfg.platform == Platform::GitLab {
                        "PRIVATE-TOKEN"
                    } else {
                        "Authorization"
                    },
                    bearer: cfg.platform != Platform::GitLab,
                },
            )
            .map_err(|e| anyhow!("{e}"))?;
            println!("opened PR #{} → {}", result.number, result.url);
            Ok(())
        }
        RemoteAction::Push { branch } => {
            let cfg = RemoteConfig::load(&gpp).map_err(|e| anyhow!("{e}"))?;
            if cfg.remote_url.is_empty() {
                bail!("no remote_url configured (run `gpp remote setup --remote-url …`)");
            }
            let msg = GenericGitRemote::push(&gpp, &cfg.remote_url, branch)
                .map_err(|e| anyhow!("{e}"))
                .context("git push")?;
            println!("{msg}");
            Ok(())
        }
        RemoteAction::Ci { git_ref } => {
            let (cfg, token) = remote_auth(&gpp)?;
            let git_ref = match git_ref {
                Some(r) => r.clone(),
                None => repo.current_branch().unwrap_or_else(|_| "main".into()),
            };
            let st = fetch_ci_status(cfg.platform, &cfg.repository, &git_ref, &token, &github())
                .map_err(|e| anyhow!("{e}"))?;
            let mark = match st.state.as_str() {
                "success" => "✓",
                "pending" => "…",
                _ => "✗",
            };
            println!("{mark} CI {} for {} @ {git_ref}", st.state, cfg.repository);
            for (ctx, state) in &st.checks {
                println!("    {state:<10} {ctx}");
            }
            Ok(())
        }
        RemoteAction::Reviews { pr } => {
            let (cfg, token) = remote_auth(&gpp)?;
            let s = fetch_pr_reviews(cfg.platform, &cfg.repository, *pr, &token, &github())
                .map_err(|e| anyhow!("{e}"))?;
            println!(
                "PR #{pr}: {} approval(s), {} change request(s), {} comment(s) — {}",
                s.approvals,
                s.changes_requested,
                s.comments,
                if s.is_approved() {
                    "APPROVED ✓"
                } else {
                    "not yet mergeable"
                }
            );
            for r in &s.reviews {
                println!("    {:<18} {}", r.user, r.state);
            }
            Ok(())
        }
    }
}

/// Load remote config and the API token, requiring a GitHub remote (inbound
/// sync is GitHub-only for now).
fn remote_auth(gpp: &Path) -> Result<(RemoteConfig, String)> {
    let cfg = RemoteConfig::load(gpp).map_err(|e| anyhow!("{e}"))?;
    if cfg.platform != Platform::GitHub {
        bail!("inbound sync (ci/reviews) is implemented for GitHub only");
    }
    let token = std::env::var(&cfg.api_token_env).map_err(|_| {
        anyhow!(
            "${} is not set (needed for inbound sync)",
            cfg.api_token_env
        )
    })?;
    Ok((cfg, token))
}

/// A GitHub-authenticated HTTP client (Bearer token).
fn github() -> ReqwestClient {
    ReqwestClient {
        auth_header: "Authorization",
        bearer: true,
    }
}

// ---------------------------------------------------------------------------
// gpp relay (a relay is just a well-known, always-on peer)
// ---------------------------------------------------------------------------

pub fn relay(args: &RelayArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let gpp = repo.gpp_dir();
    let opts = gpp_sync::SyncOptions::default();

    match &args.action {
        RelayAction::Status => {
            let peers = crate::phase5::load_peers(&repo);
            if peers.is_empty() {
                println!("(no relays configured — `gpp relay add <name> <addr>`)");
            }
            for (n, a) in peers {
                println!("{n:<16} {a}");
            }
            Ok(())
        }
        RelayAction::Add { name, address } => {
            let mut peers = crate::phase5::load_peers(&repo);
            peers.retain(|(n, _)| n != name);
            peers.push((name.clone(), address.clone()));
            crate::phase5::save_peers(&repo, &peers)?;
            println!("relay {name} → {address} added");
            Ok(())
        }
        RelayAction::Remove { name } => {
            let mut peers = crate::phase5::load_peers(&repo);
            let before = peers.len();
            peers.retain(|(n, _)| n != name);
            if peers.len() == before {
                bail!("no relay named {name:?}");
            }
            crate::phase5::save_peers(&repo, &peers)?;
            println!("relay {name} removed");
            Ok(())
        }
        RelayAction::Push { name } | RelayAction::Pull { name } => {
            let addr = crate::phase5::load_peers(&repo)
                .into_iter()
                .find(|(n, _)| n == name)
                .map(|(_, a)| a)
                .ok_or_else(|| anyhow!("no relay named {name:?}"))?;
            let r = gpp_sync::connect(&addr, &gpp, name, opts).map_err(|e| anyhow!("{e}"))?;
            crate::phase5::print_report(name, &r);
            Ok(())
        }
    }
}

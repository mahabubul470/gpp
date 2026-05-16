//! Phase 2 commands: Git bridge (import / export / continuous bridge).

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::cli::{GitBridgeArgs, GitExportArgs, GitImportArgs};
use crate::repo::Repo;

fn discover(repo_override: Option<&Path>) -> Result<Repo> {
    let start = match repo_override {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    Repo::discover(&start)
}

pub fn git_import(args: &GitImportArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let stats = gpp_git_bridge::import(&args.path, &repo.gpp_dir())
        .with_context(|| format!("importing Git repo at {}", args.path.display()))?;
    println!(
        "Imported {} commit(s) ({} already present), set {} branch ref(s)",
        stats.commits_imported, stats.commits_skipped, stats.branches_set
    );
    Ok(())
}

pub fn git_export(args: &GitExportArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let stats = gpp_git_bridge::export(&repo.gpp_dir(), &args.path)
        .with_context(|| format!("exporting to Git repo at {}", args.path.display()))?;
    println!(
        "Exported {} commit(s) ({} already present), set {} branch ref(s)",
        stats.commits_exported, stats.commits_skipped, stats.branches_set
    );
    Ok(())
}

pub fn git_bridge(args: &GitBridgeArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let gpp_dir = repo.gpp_dir();

    let sync_once = || -> Result<()> {
        let i = gpp_git_bridge::import(&args.path, &gpp_dir)?;
        if i.commits_imported > 0 {
            println!("← imported {} new commit(s)", i.commits_imported);
        }
        if args.export {
            let e = gpp_git_bridge::export(&gpp_dir, &args.path)?;
            if e.commits_exported > 0 {
                println!("→ exported {} new commit(s)", e.commits_exported);
            }
        }
        Ok(())
    };

    sync_once()?;
    if !args.watch {
        return Ok(());
    }

    eprintln!(
        "bridging {} ↔ gpp every {}s … (Ctrl-C to stop)",
        args.path.display(),
        args.interval
    );
    let mut last = gpp_git_bridge::head_oid(&args.path)?;
    loop {
        std::thread::sleep(Duration::from_secs(args.interval.max(1)));
        let now = gpp_git_bridge::head_oid(&args.path)?;
        if now != last {
            sync_once()?;
            last = now;
        }
    }
}

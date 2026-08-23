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

    // Importing from a *sibling* checkout into an empty gpp working dir
    // would leave HEAD's tree with no files on disk — and the next
    // `promote` would snapshot that emptiness as a deletion of every
    // imported file. Materialise HEAD once when the working dir is empty;
    // never touch a checkout that already has content.
    if working_dir_is_empty(&repo.root)? {
        let written = materialize_head(&repo)?;
        if written > 0 {
            println!("Materialised {written} file(s) from HEAD into the working directory");
        }
    }
    Ok(())
}

fn working_dir_is_empty(root: &Path) -> Result<bool> {
    for entry in std::fs::read_dir(root)? {
        let name = entry?.file_name();
        if name != ".gpp" && name != ".git" {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Write HEAD's tree to the working directory. Returns files written.
fn materialize_head(repo: &Repo) -> Result<usize> {
    use gpp_core::{Blob, ObjectStore, flatten_tree};
    use gpp_history::{Changeset, RefStore};
    let Some(tip) = RefStore::open(&repo.gpp_dir()).head_tip()? else {
        return Ok(0);
    };
    let store = ObjectStore::open(&repo.gpp_dir());
    let cs: Changeset = store.read(&tip)?;
    let files = flatten_tree(&store, &cs.tree)?;
    for (path, blob) in &files {
        let target = repo.root.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, store.read::<Blob>(blob)?.content)?;
    }
    Ok(files.len())
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

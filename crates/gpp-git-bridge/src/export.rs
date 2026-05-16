//! Export gpp history into a Git repository.
//!
//! Each gpp branch's first-parent changeset chain is replayed oldest-first
//! into Git commits; gpp trees/blobs become Git trees/blobs. The mapping DB
//! makes this incremental and idempotent, so re-running only appends new
//! commits. Git only ever sees clean commits — gpp metadata stays local.

use std::path::Path;

use git2::{Repository, Signature, Time};
use gpp_core::{Blob, EntryKind, Hash, ObjectStore, Tree};
use gpp_history::{Author, RefStore, walk};

use crate::error::{Error, Result};
use crate::map::HashMap;

/// What an export run produced.
#[derive(Debug, Default, Clone)]
pub struct ExportStats {
    pub commits_exported: usize,
    pub commits_skipped: usize,
    pub branches_set: usize,
}

/// Export all gpp branches into the Git repo at `git_path` (initialized if
/// it does not exist). `gpp_dir` is the `.gpp/` directory.
pub fn export(gpp_dir: &Path, git_path: &Path) -> Result<ExportStats> {
    let repo = match Repository::open(git_path) {
        Ok(r) => r,
        Err(_) => Repository::init(git_path)?,
    };
    let store = ObjectStore::open(gpp_dir);
    let refs = RefStore::open(gpp_dir);
    let map = HashMap::open(gpp_dir)?;
    let mut stats = ExportStats::default();

    for branch in refs.list()? {
        let Some(tip) = branch.tip else { continue };

        // Oldest-first first-parent chain.
        let mut chain = walk(&store, Some(tip), 1_000_000)?;
        chain.reverse();

        let mut last_oid = None;
        for rec in &chain {
            if let Some(existing) = map.commit_for_gpp(&rec.id)? {
                last_oid = Some(git2::Oid::from_str(&existing)?);
                stats.commits_skipped += 1;
                continue;
            }

            let tree_oid = build_git_tree(&repo, &store, &rec.changeset.tree)?;
            let tree = repo.find_tree(tree_oid)?;

            // Parents: map gpp parents to git commits.
            let mut parent_commits = Vec::new();
            for p in &rec.changeset.parents {
                if let Some(po) = map.commit_for_gpp(p)? {
                    parent_commits.push(repo.find_commit(git2::Oid::from_str(&po)?)?);
                }
            }
            let parent_refs: Vec<&git2::Commit> = parent_commits.iter().collect();

            let secs = rec.changeset.timestamp / 1_000_000;
            let author_sig = signature(&rec.changeset.author, secs)?;
            let committer_sig = match &rec.changeset.committer {
                Some(c) => signature(c, secs)?,
                None => author_sig.clone(),
            };

            let oid = repo.commit(
                None,
                &author_sig,
                &committer_sig,
                rec.message(),
                &tree,
                &parent_refs,
            )?;
            map.link(&oid.to_string(), &rec.id)?;
            last_oid = Some(oid);
            stats.commits_exported += 1;
        }

        if let Some(oid) = last_oid {
            repo.reference(
                &format!("refs/heads/{}", branch.name),
                oid,
                true,
                "gpp git-export",
            )?;
            stats.branches_set += 1;
        }
    }

    if let Ok(head) = refs.head_branch()
        && repo.find_reference(&format!("refs/heads/{head}")).is_ok()
    {
        repo.set_head(&format!("refs/heads/{head}"))?;
    }

    Ok(stats)
}

fn signature<'a>(a: &Author, secs: i64) -> Result<Signature<'a>> {
    let name = if a.name.is_empty() {
        "Unknown"
    } else {
        &a.name
    };
    let email = if a.identity.is_empty() {
        "unknown@localhost"
    } else {
        &a.identity
    };
    Ok(Signature::new(name, email, &Time::new(secs, 0))?)
}

/// Recursively materialize a gpp tree as a Git tree, returning its oid.
fn build_git_tree(repo: &Repository, store: &ObjectStore, tree: &Hash) -> Result<git2::Oid> {
    let t: Tree = store.read(tree)?;
    let mut builder = repo.treebuilder(None)?;
    for e in t.entries {
        let mode: i32 = match e.kind {
            EntryKind::Directory => 0o040000,
            EntryKind::Symlink => 0o120000,
            EntryKind::File => {
                if e.mode == 0o100755 {
                    0o100755
                } else {
                    0o100644
                }
            }
        };
        let oid = match e.kind {
            EntryKind::Directory => build_git_tree(repo, store, &e.hash)?,
            EntryKind::File | EntryKind::Symlink => {
                let blob: Blob = store.read(&e.hash)?;
                repo.blob(&blob.content)?
            }
        };
        builder
            .insert(&e.name, oid, mode)
            .map_err(|err| Error::Other(format!("tree insert {:?}: {err}", e.name)))?;
    }
    Ok(builder.write()?)
}

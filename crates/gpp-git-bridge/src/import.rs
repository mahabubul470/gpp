//! Import Git history into gpp.
//!
//! Commits are walked oldest-first (reverse topological) so every parent is
//! mapped before its children. Each Git commit becomes a gpp [`Changeset`]
//! with a synthesized [`Intent`] from the commit message; Git trees/blobs
//! become gpp [`Tree`]/[`Blob`] objects. Re-running is incremental: already
//! mapped commits are skipped, so this doubles as a pull.

use std::path::Path;

use git2::{ObjectType as GitObjectType, Repository, Sort};
use gpp_core::{Blob, EntryKind, Hash, ObjectStore, Tree, TreeEntry};
use gpp_history::{Author, AuthorType, Changeset, Intent, IntentType, RefStore};

use crate::error::{Error, Result};
use crate::map::HashMap;

/// What an import run produced.
#[derive(Debug, Default, Clone)]
pub struct ImportStats {
    pub commits_imported: usize,
    pub commits_skipped: usize,
    pub branches_set: usize,
}

/// Import every commit reachable from local branches of the Git repo at
/// `git_path` into the gpp repo rooted at `gpp_dir` (the `.gpp/` directory).
pub fn import(git_path: &Path, gpp_dir: &Path) -> Result<ImportStats> {
    let repo = Repository::open(git_path)?;
    let store = ObjectStore::open(gpp_dir);
    let refs = RefStore::open(gpp_dir);
    let map = HashMap::open(gpp_dir)?;
    let mut stats = ImportStats::default();

    let mut walk = repo.revwalk()?;
    walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)?;
    walk.push_glob("refs/heads/*")?;
    if repo.head().is_ok() {
        let _ = walk.push_head();
    }

    for oid in walk {
        let oid = oid?;
        let oid_hex = oid.to_string();
        if map.gpp_for_commit(&oid_hex)?.is_some() {
            stats.commits_skipped += 1;
            continue;
        }
        let commit = repo.find_commit(oid)?;

        // Convert the commit's root tree.
        let git_tree = commit.tree()?;
        let tree_hash = convert_tree(&repo, &store, &git_tree)?;

        // Map parents (present already thanks to reverse-topo order).
        let mut parents = Vec::new();
        for p in commit.parent_ids() {
            if let Some(h) = map.gpp_for_commit(&p.to_string())? {
                parents.push(h);
            }
        }

        let a = commit.author();
        let c = commit.committer();
        let author = Author {
            author_type: AuthorType::Human,
            name: a.name().unwrap_or("Unknown").to_string(),
            identity: a.email().unwrap_or("unknown@localhost").to_string(),
            agent_meta: None,
        };
        let committer = Author {
            author_type: AuthorType::Human,
            name: c.name().unwrap_or("Unknown").to_string(),
            identity: c.email().unwrap_or("unknown@localhost").to_string(),
            agent_meta: None,
        };

        let msg = commit.message().unwrap_or("").trim().to_string();
        let timestamp = commit.time().seconds() * 1_000_000;
        let intent = Intent {
            intent_type: IntentType::HumanDirected,
            description: if msg.is_empty() {
                format!("git commit {}", &oid_hex[..8.min(oid_hex.len())])
            } else {
                msg
            },
            prompt: None,
            task_reference: None,
            goal: None,
            constraints: Vec::new(),
            timestamp,
        };
        let intent_id = store.write(&intent)?;

        let mut metadata = std::collections::BTreeMap::new();
        metadata.insert("git_commit".to_string(), oid_hex.clone());
        let changeset = Changeset {
            parents,
            tree: tree_hash,
            timestamp,
            author,
            committer: Some(committer),
            intent: Some(intent_id),
            timeline_range: None,
            metadata,
        };
        let cs_id = store.write(&changeset)?;
        map.link(&oid_hex, &cs_id)?;
        stats.commits_imported += 1;
    }

    // Mirror local branches and HEAD.
    for b in repo.branches(Some(git2::BranchType::Local))? {
        let (branch, _) = b?;
        let Some(name) = branch.name()?.map(str::to_string) else {
            continue;
        };
        if let Some(target) = branch.get().target()
            && let Some(tip) = map.gpp_for_commit(&target.to_string())?
        {
            refs.write_ref(&name, tip)?;
            stats.branches_set += 1;
        }
    }
    if let Ok(head) = repo.head()
        && head.is_branch()
        && let Some(name) = head.shorthand()
    {
        refs.set_head_branch(name)?;
    }

    Ok(stats)
}

/// Recursively convert a Git tree into stored gpp objects, returning its hash.
fn convert_tree(repo: &Repository, store: &ObjectStore, git_tree: &git2::Tree) -> Result<Hash> {
    let mut entries = Vec::new();
    for ent in git_tree.iter() {
        let name = ent.name().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let mode = ent.filemode() as u32;
        match ent.kind() {
            Some(GitObjectType::Tree) => {
                let sub = ent
                    .to_object(repo)?
                    .into_tree()
                    .map_err(|_| Error::Other(format!("tree entry {name:?} is not a tree")))?;
                let hash = convert_tree(repo, store, &sub)?;
                entries.push(TreeEntry {
                    name,
                    kind: EntryKind::Directory,
                    hash,
                    mode,
                    size: 0,
                });
            }
            Some(GitObjectType::Blob) => {
                let blob = ent
                    .to_object(repo)?
                    .into_blob()
                    .map_err(|_| Error::Other(format!("blob entry {name:?} is not a blob")))?;
                let content = blob.content().to_vec();
                let size = content.len() as u64;
                let hash = store.write(&Blob::new(content))?;
                let kind = if mode == 0o120000 {
                    EntryKind::Symlink
                } else {
                    EntryKind::File
                };
                entries.push(TreeEntry {
                    name,
                    kind,
                    hash,
                    mode,
                    size,
                });
            }
            // Submodule (gitlink) or unknown — skip, leaving a note in logs.
            other => {
                tracing::warn!(?other, %name, "skipping unsupported git tree entry");
            }
        }
    }
    Ok(store.write(&Tree::new(entries))?)
}

/// Oid of the Git repo's current HEAD commit (for change detection in watch).
pub fn head_oid(git_path: &Path) -> Result<Option<String>> {
    let repo = Repository::open(git_path)?;
    Ok(repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .map(|o| o.to_string()))
}

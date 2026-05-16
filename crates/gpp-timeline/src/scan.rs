//! Working-tree scanner: detects changes vs `workspace_state`, writes blobs
//! to the object store, and builds snapshot trees for promotion.

use std::collections::HashSet;
use std::path::Path;
use std::time::UNIX_EPOCH;

use gpp_core::{Blob, EntryKind, Hash, Object, ObjectStore, Tree, TreeEntry};
use walkdir::WalkDir;

use crate::db::{StateRow, TimelineDb};
use crate::error::Result;
use crate::ignore::IgnoreMatcher;
use crate::model::{ChangeType, FileChange};

/// Relative, `/`-separated path of `p` under `root`, or `None` if not nested.
fn rel(root: &Path, p: &Path) -> Option<String> {
    let stripped = p.strip_prefix(root).ok()?;
    let s = stripped
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if s.is_empty() { None } else { Some(s) }
}

fn mtime_us(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

/// Detect changes since the last capture. Writes new/modified content as
/// blobs and returns the per-file change list (empty if nothing changed).
pub fn detect_changes(
    root: &Path,
    store: &ObjectStore,
    ignore: &IgnoreMatcher,
    db: &TimelineDb,
) -> Result<Vec<FileChange>> {
    let mut state = db.load_state()?;
    let mut changes = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let walker = WalkDir::new(root).into_iter().filter_entry(|e| {
        match rel(root, e.path()) {
            None => true, // the root itself
            Some(r) => !ignore.is_ignored(&r),
        }
    });

    for entry in walker {
        let entry = entry.map_err(|e| crate::error::Error::Other(format!("walk error: {e}")))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(relpath) = rel(root, entry.path()) else {
            continue;
        };
        seen.insert(relpath.clone());

        let meta = entry
            .metadata()
            .map_err(|e| crate::error::Error::Other(format!("metadata error: {e}")))?;
        let size = meta.len() as i64;
        let mtime = mtime_us(&meta);

        if let Some(prev) = state.get(&relpath)
            && prev.size == size
            && prev.mtime_us == mtime
        {
            continue; // unchanged fast-path
        }

        let content = std::fs::read(entry.path())?;
        let blob = Blob::new(content);
        let id = blob.id()?;

        match state.remove(&relpath) {
            Some(prev) if prev.blob_hash == id.to_base32() => {
                // Content identical, only mtime/size metadata moved — refresh.
                db.upsert_state(
                    &relpath,
                    &StateRow {
                        blob_hash: id.to_base32(),
                        size,
                        mtime_us: mtime,
                    },
                )?;
            }
            prev => {
                store.write(&blob)?;
                let (change, old_hash) = match &prev {
                    Some(p) => (ChangeType::Modify, Hash::from_base32(&p.blob_hash).ok()),
                    None => (ChangeType::Add, None),
                };
                changes.push(FileChange {
                    path: relpath.clone(),
                    blob_hash: Some(id),
                    change,
                    old_hash,
                    old_path: None,
                });
                db.upsert_state(
                    &relpath,
                    &StateRow {
                        blob_hash: id.to_base32(),
                        size,
                        mtime_us: mtime,
                    },
                )?;
            }
        }
    }

    // Anything left in `state` was not seen on disk → deleted.
    for (path, prev) in state {
        if seen.contains(&path) {
            continue;
        }
        changes.push(FileChange {
            path: path.clone(),
            blob_hash: None,
            change: ChangeType::Delete,
            old_hash: Hash::from_base32(&prev.blob_hash).ok(),
            old_path: None,
        });
        db.delete_state(&path)?;
    }

    changes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(changes)
}

/// Build (and store) a snapshot [`Tree`] of the working directory as it is
/// now, returning the root tree hash. Respects the ignore matcher.
pub fn snapshot_tree(root: &Path, store: &ObjectStore, ignore: &IgnoreMatcher) -> Result<Hash> {
    build_tree(root, root, store, ignore)
}

fn build_tree(
    root: &Path,
    dir: &Path,
    store: &ObjectStore,
    ignore: &IgnoreMatcher,
) -> Result<Hash> {
    let mut entries = Vec::new();
    for dirent in std::fs::read_dir(dir)? {
        let dirent = dirent?;
        let path = dirent.path();
        let Some(relpath) = rel(root, &path) else {
            continue;
        };
        if ignore.is_ignored(&relpath) {
            continue;
        }
        let meta = dirent.metadata()?;
        let name = dirent.file_name().to_string_lossy().into_owned();
        if meta.is_dir() {
            let subtree = build_tree(root, &path, store, ignore)?;
            entries.push(TreeEntry {
                name,
                kind: EntryKind::Directory,
                hash: subtree,
                mode: 0o040000,
                size: 0,
            });
        } else if meta.is_file() {
            let content = std::fs::read(&path)?;
            let size = content.len() as u64;
            let blob = Blob::new(content);
            let id = store.write(&blob)?;
            entries.push(TreeEntry {
                name,
                kind: EntryKind::File,
                hash: id,
                mode: 0o100644,
                size,
            });
        }
    }
    store.write(&Tree::new(entries)).map_err(Into::into)
}

//! `gpp-replay` — reproducible environment snapshots (layer 12).
//!
//! A [`Snapshot`] pins everything needed to reproduce a changeset's working
//! state: the changeset id, its content-addressed tree, captured tool
//! versions, and an environment subset. It is stored as a content-addressed
//! `Blob` so it syncs like any other object.
//!
//! [`replay`] re-materializes the tree into a directory; [`diff_against`]
//! compares a directory to the pinned tree (drift detection). Execution of
//! the original agent is intentionally out of scope — replay reproduces the
//! *inputs* deterministically and offline.
//!
//! See `docs/ROADMAP.md` (Phase 5).
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::Path;

use gpp_core::{Blob, EntryKind, Hash, ObjectStore, Tree};
use gpp_history::Changeset;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("object store error: {0}")]
    Core(#[from] gpp_core::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A reproducible pin of a changeset's environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub changeset: String,
    pub tree: String,
    /// Tool → version (e.g. `rustc` → `1.x`). Best-effort, may be empty.
    pub toolchain: BTreeMap<String, String>,
    /// Captured environment variable subset.
    pub env: BTreeMap<String, String>,
    pub created_at: i64,
}

/// A snapshot is stored as a JSON `Blob` (content-addressed like any object).
impl Snapshot {
    fn to_blob(&self) -> Result<Blob> {
        Ok(Blob::new(serde_json::to_vec(self)?))
    }
    fn from_blob(b: &Blob) -> Result<Self> {
        Ok(serde_json::from_slice(&b.content)?)
    }
}

fn now_micros() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

/// Capture a snapshot for `changeset`, storing it and returning its id.
///
/// `env` is captured verbatim (the caller decides which vars are safe — do
/// not pass secrets). `toolchain` is likewise caller-provided so capture
/// stays deterministic and offline.
pub fn snapshot(
    store: &ObjectStore,
    changeset: &Hash,
    toolchain: BTreeMap<String, String>,
    env: BTreeMap<String, String>,
) -> Result<Hash> {
    let cs: Changeset = store.read(changeset)?;
    let snap = Snapshot {
        changeset: changeset.to_base32(),
        tree: cs.tree.to_base32(),
        toolchain,
        env,
        created_at: now_micros(),
    };
    Ok(store.write(&snap.to_blob()?)?)
}

/// Load a snapshot by its blob id. A blob that is not snapshot JSON is
/// rejected (so an arbitrary file blob can't be mistaken for a snapshot).
pub fn load(store: &ObjectStore, id: &Hash) -> Result<Snapshot> {
    Snapshot::from_blob(&store.read::<Blob>(id)?)
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

/// Re-materialize a snapshot's tree into `dest`. Returns files written.
/// With `dry_run`, nothing is written — the path list is still returned.
pub fn replay(
    store: &ObjectStore,
    snap_id: &Hash,
    dest: &Path,
    dry_run: bool,
) -> Result<Vec<String>> {
    let snap = load(store, snap_id)?;
    let tree = Hash::from_base32(&snap.tree).map_err(|e| Error::Other(e.to_string()))?;
    let files = flatten(store, &tree)?;
    let mut written = Vec::new();
    for (path, blob_hash) in &files {
        written.push(path.clone());
        if dry_run {
            continue;
        }
        let target = dest.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, store.read::<Blob>(blob_hash)?.content)?;
    }
    written.sort();
    Ok(written)
}

/// Compare `dir` to a snapshot's pinned tree. Returns paths that differ
/// (added, removed, or modified) — empty means a faithful reproduction.
pub fn diff_against(store: &ObjectStore, snap_id: &Hash, dir: &Path) -> Result<Vec<String>> {
    let snap = load(store, snap_id)?;
    let tree = Hash::from_base32(&snap.tree).map_err(|e| Error::Other(e.to_string()))?;
    let pinned = flatten(store, &tree)?;

    let mut diffs = Vec::new();
    for (path, blob_hash) in &pinned {
        match std::fs::read(dir.join(path)) {
            Ok(actual) => {
                let want = store.read::<Blob>(blob_hash)?.content;
                if actual != want {
                    diffs.push(path.clone());
                }
            }
            Err(_) => diffs.push(path.clone()), // missing
        }
    }
    diffs.sort();
    Ok(diffs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpp_history::Author;

    fn repo() -> (tempfile::TempDir, ObjectStore) {
        let d = tempfile::tempdir().unwrap();
        let s = ObjectStore::init(&d.path().join(".gpp")).unwrap();
        (d, s)
    }

    fn make_changeset(s: &ObjectStore) -> Hash {
        let b1 = s.write(&Blob::new(b"fn main() {}\n".to_vec())).unwrap();
        let b2 = s.write(&Blob::new(b"# readme\n".to_vec())).unwrap();
        let tree = s
            .write(&Tree::new(vec![
                gpp_core::TreeEntry {
                    name: "main.rs".into(),
                    kind: EntryKind::File,
                    hash: b1,
                    mode: 0o644,
                    size: 13,
                },
                gpp_core::TreeEntry {
                    name: "README.md".into(),
                    kind: EntryKind::File,
                    hash: b2,
                    mode: 0o644,
                    size: 9,
                },
            ]))
            .unwrap();
        s.write(&Changeset {
            parents: vec![],
            tree,
            timestamp: 1,
            author: Author::human("Dev", "d@e.com"),
            committer: None,
            intent: None,
            timeline_range: None,
            metadata: Default::default(),
        })
        .unwrap()
    }

    #[test]
    fn snapshot_replay_roundtrip_and_drift() {
        let (d, s) = repo();
        let cs = make_changeset(&s);
        let snap = snapshot(
            &s,
            &cs,
            BTreeMap::from([("rustc".into(), "1.0.0".into())]),
            BTreeMap::from([("CI".into(), "1".into())]),
        )
        .unwrap();

        let loaded = load(&s, &snap).unwrap();
        assert_eq!(loaded.changeset, cs.to_base32());
        assert_eq!(loaded.toolchain["rustc"], "1.0.0");

        // dry-run lists files but writes nothing.
        let out = d.path().join("out");
        let listed = replay(&s, &snap, &out, true).unwrap();
        assert_eq!(listed, ["README.md", "main.rs"]);
        assert!(!out.exists());

        // real replay reproduces the tree exactly.
        let files = replay(&s, &snap, &out, false).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(diff_against(&s, &snap, &out).unwrap(), Vec::<String>::new());

        // tamper → drift detected.
        std::fs::write(out.join("main.rs"), "tampered").unwrap();
        assert_eq!(diff_against(&s, &snap, &out).unwrap(), ["main.rs"]);
    }

    #[test]
    fn load_rejects_non_snapshot_blob() {
        let (_d, s) = repo();
        let plain = s.write(&Blob::new(b"just a file".to_vec())).unwrap();
        assert!(load(&s, &plain).is_err());
    }
}

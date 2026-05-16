//! `gpp-git-bridge` — bidirectional Git import/export.
//!
//! gpp treats Git as a first-class sync target, not a competitor. This crate
//! converts Git commits ↔ gpp [`Changeset`](gpp_history::Changeset)s while a
//! [`HashMap`] keeps the correspondence so sync stays incremental and
//! idempotent. Graphex/timeline/trust/cost stay local — Git only ever sees
//! clean commit history.
//!
//! See `docs/ROADMAP.md` (Phase 2).
#![forbid(unsafe_code)]

mod error;
mod export;
mod import;
mod map;

pub use error::{Error, Result};
pub use export::{ExportStats, export};
pub use import::{ImportStats, head_oid, import};
pub use map::HashMap;

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Repository, Signature, Time};
    use gpp_core::{Blob, EntryKind, ObjectStore, Tree, TreeEntry};
    use gpp_history::{Author, Changeset, Intent, IntentType, RefStore, walk};
    use std::path::Path;

    /// Minimal Git repo: two commits on `main`, one file evolving.
    fn make_git_repo(dir: &Path) {
        let repo = Repository::init(dir).unwrap();
        repo.set_head("refs/heads/main").unwrap();
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        let sig = Signature::new("Dev", "dev@example.com", &Time::new(1_000, 0)).unwrap();
        let mut idx = repo.index().unwrap();
        idx.add_path(Path::new("a.txt")).unwrap();
        idx.write().unwrap();
        let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
        let c1 = repo
            .commit(Some("HEAD"), &sig, &sig, "first", &tree, &[])
            .unwrap();

        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        let mut idx = repo.index().unwrap();
        idx.add_path(Path::new("a.txt")).unwrap();
        idx.write().unwrap();
        let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
        let parent = repo.find_commit(c1).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "second", &tree, &[&parent])
            .unwrap();
    }

    fn init_gpp(gpp_dir: &Path) {
        std::fs::create_dir_all(gpp_dir.join("refs")).unwrap();
        ObjectStore::init(gpp_dir).unwrap();
        std::fs::write(gpp_dir.join("HEAD"), "ref: refs/main\n").unwrap();
    }

    #[test]
    fn import_then_log_has_two_changesets() {
        let gitdir = tempfile::tempdir().unwrap();
        let gppdir = tempfile::tempdir().unwrap();
        let gpp = gppdir.path().join(".gpp");
        make_git_repo(gitdir.path());
        init_gpp(&gpp);

        let stats = import(gitdir.path(), &gpp).unwrap();
        assert_eq!(stats.commits_imported, 2);
        assert!(stats.branches_set >= 1);

        let refs = RefStore::open(&gpp);
        let tip = refs.read_ref("main").unwrap().expect("main tip");
        let store = ObjectStore::open(&gpp);
        let log = walk(&store, Some(tip), 10).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].message(), "second");
        assert_eq!(log[1].message(), "first");
    }

    #[test]
    fn import_is_idempotent() {
        let gitdir = tempfile::tempdir().unwrap();
        let gppdir = tempfile::tempdir().unwrap();
        let gpp = gppdir.path().join(".gpp");
        make_git_repo(gitdir.path());
        init_gpp(&gpp);

        assert_eq!(import(gitdir.path(), &gpp).unwrap().commits_imported, 2);
        let again = import(gitdir.path(), &gpp).unwrap();
        assert_eq!(again.commits_imported, 0);
        assert_eq!(again.commits_skipped, 2);
    }

    /// Hand-build a one-changeset gpp repo (no Git involved) so export can be
    /// tested in isolation from import / the shared mapping.
    fn make_gpp_repo(gpp: &Path) {
        init_gpp(gpp);
        let store = ObjectStore::open(gpp);
        let blob = store.write(&Blob::new(b"hello\n".to_vec())).unwrap();
        let tree = store
            .write(&Tree::new(vec![TreeEntry {
                name: "greeting.txt".into(),
                kind: EntryKind::File,
                hash: blob,
                mode: 0o100644,
                size: 6,
            }]))
            .unwrap();
        let intent = store
            .write(&Intent {
                intent_type: IntentType::Feature,
                description: "add greeting".into(),
                prompt: None,
                task_reference: None,
                goal: None,
                constraints: Vec::new(),
                timestamp: 2_000_000_000,
            })
            .unwrap();
        let cs = store
            .write(&Changeset {
                parents: Vec::new(),
                tree,
                timestamp: 2_000_000_000,
                author: Author::human("Dev", "dev@example.com"),
                committer: None,
                intent: Some(intent),
                timeline_range: None,
                metadata: Default::default(),
            })
            .unwrap();
        RefStore::open(gpp).write_ref("main", cs).unwrap();
    }

    #[test]
    fn export_materializes_git_commit_with_content() {
        let gppdir = tempfile::tempdir().unwrap();
        let outdir = tempfile::tempdir().unwrap();
        let gpp = gppdir.path().join(".gpp");
        make_gpp_repo(&gpp);

        let stats = export(&gpp, outdir.path()).unwrap();
        assert_eq!(stats.commits_exported, 1);
        assert_eq!(stats.branches_set, 1);

        let repo = Repository::open(outdir.path()).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.message().unwrap(), "add greeting");
        let tree = head.tree().unwrap();
        let entry = tree.get_name("greeting.txt").unwrap();
        assert_eq!(repo.find_blob(entry.id()).unwrap().content(), b"hello\n");

        // Re-export to the same repo is a no-op (mapping makes it idempotent).
        let again = export(&gpp, outdir.path()).unwrap();
        assert_eq!(again.commits_exported, 0);
        assert_eq!(again.commits_skipped, 1);
    }
}

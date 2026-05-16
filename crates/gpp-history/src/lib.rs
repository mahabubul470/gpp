//! `gpp-history` — curated changeset history promoted from the timeline.
//!
//! Layer 3 of gpp. Defines the [`Changeset`]/[`Intent`] objects, the branch
//! [`RefStore`], promotion of timeline entries into changesets, and DAG
//! traversal for `gpp log`.
#![forbid(unsafe_code)]

mod error;
mod log;
mod object;
mod promote;
mod refs;

pub use error::{Error, Result};
pub use log::{ChangesetRecord, walk};
pub use object::{Author, AuthorType, Changeset, Intent, IntentType};
pub use promote::{PromoteOptions, PromoteOutcome, promote};
pub use refs::{BranchInfo, RefStore};

#[cfg(test)]
mod tests {
    use super::*;
    use gpp_core::ObjectStore;
    use gpp_timeline::Timeline;

    fn repo() -> (tempfile::TempDir, Timeline, RefStore) {
        let dir = tempfile::tempdir().unwrap();
        let gpp = dir.path().join(".gpp");
        std::fs::create_dir_all(gpp.join("refs")).unwrap();
        ObjectStore::init(&gpp).unwrap();
        std::fs::write(gpp.join("HEAD"), "ref: refs/main\n").unwrap();
        let tl = Timeline::open(dir.path(), ["target/**"]).unwrap();
        let refs = RefStore::open(&gpp);
        (dir, tl, refs)
    }

    fn opts(msg: &str) -> PromoteOptions {
        PromoteOptions {
            from: None,
            to: None,
            message: msg.to_string(),
            intent_type: IntentType::Feature,
            task: None,
            author: Author::human("Dev One", "dev1@example.com"),
        }
    }

    #[test]
    fn promote_creates_changeset_and_advances_branch() {
        let (dir, mut tl, refs) = repo();
        std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();

        let out = promote(&mut tl, &refs, opts("first change")).unwrap();
        assert_eq!(out.branch, "main");
        assert_eq!(out.entries_promoted, 1);
        assert_eq!(refs.read_ref("main").unwrap(), Some(out.changeset));

        // Second changeset chains onto the first.
        std::fs::write(dir.path().join("a.txt"), "hello\nworld\n").unwrap();
        let out2 = promote(&mut tl, &refs, opts("second change")).unwrap();

        let store = ObjectStore::open(&dir.path().join(".gpp"));
        let log = walk(&store, Some(out2.changeset), 10).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].message(), "second change");
        assert_eq!(log[1].message(), "first change");
        assert_eq!(log[0].changeset.parents, vec![out.changeset]);
    }

    #[test]
    fn nothing_to_promote_errors() {
        let (_dir, mut tl, refs) = repo();
        let err = promote(&mut tl, &refs, opts("noop")).unwrap_err();
        assert!(matches!(err, Error::NothingToPromote));
    }

    #[test]
    fn branch_create_switch_list() {
        let (dir, mut tl, refs) = repo();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let out = promote(&mut tl, &refs, opts("base")).unwrap();

        // create feature branch at current tip, switch to it
        refs.write_ref("feature/x", out.changeset).unwrap();
        refs.set_head_branch("feature/x").unwrap();
        assert_eq!(refs.head_branch().unwrap(), "feature/x");

        let names: Vec<_> = refs.list().unwrap().into_iter().map(|b| b.name).collect();
        assert!(names.contains(&"main".to_string()));
        assert!(names.contains(&"feature/x".to_string()));
    }
}

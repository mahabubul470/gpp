//! `gpp-timeline` — continuous file-change capture.
//!
//! Layer 2 of gpp. Watches the working tree, records every change as an
//! append-only timeline entry in SQLite, and writes content to the
//! `gpp-core` object store. Curated history (`gpp-history`) is promoted
//! *from* the timeline.
#![forbid(unsafe_code)]

mod db;
mod error;
mod ignore;
mod model;
mod scan;
mod watch;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpp_core::{Hash, ObjectStore};

pub use error::{Error, Result};
pub use ignore::IgnoreMatcher;
pub use model::{AuthorKind, ChangeType, EntryFilter, EntryView, FileChange, Source};
pub use watch::DEFAULT_DEBOUNCE_MS;

use db::TimelineDb;

/// Current time in Unix microseconds (UTC), as stored everywhere in gpp.
pub fn now_micros() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

/// The timeline engine for one repository.
pub struct Timeline {
    root: PathBuf,
    store: ObjectStore,
    db: TimelineDb,
    ignore: IgnoreMatcher,
}

impl Timeline {
    /// Open the timeline for the repo rooted at `repo_root`.
    ///
    /// `ignore_patterns` is the merged gitignore-style list (the configured
    /// `[timeline].ignore` plus any `.gppignore`); `.gpp/` is always ignored.
    pub fn open<I, S>(repo_root: &Path, ignore_patterns: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let gpp_dir = repo_root.join(".gpp");
        let store = ObjectStore::open(&gpp_dir);
        let db = TimelineDb::open(&gpp_dir.join("timeline/timeline.db"))?;
        let ignore = IgnoreMatcher::new(ignore_patterns)?;
        Ok(Self {
            root: repo_root.to_path_buf(),
            store,
            db,
            ignore,
        })
    }

    pub fn store(&self) -> &ObjectStore {
        &self.store
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Scan the working tree and, if anything changed, append one entry.
    /// Returns the new entry id, or `None` if there were no changes.
    pub fn capture(
        &mut self,
        author_kind: AuthorKind,
        author_id: &str,
        source: Source,
    ) -> Result<Option<i64>> {
        let changes = scan::detect_changes(&self.root, &self.store, &self.ignore, &self.db)?;
        if changes.is_empty() {
            return Ok(None);
        }
        let summary = format!(
            "{} file{} changed via {}",
            changes.len(),
            if changes.len() == 1 { "" } else { "s" },
            source.as_str()
        );
        let id = self.db.insert_entry(
            now_micros(),
            author_kind,
            author_id,
            source.as_str(),
            Some(&summary),
            &changes,
        )?;
        Ok(Some(id))
    }

    /// Query timeline entries (newest first).
    pub fn entries(&self, filter: &EntryFilter) -> Result<Vec<EntryView>> {
        self.db.query_entries(filter)
    }

    /// Ids of unpromoted entries within an inclusive id range.
    pub fn unpromoted_in_range(&self, from: Option<i64>, to: Option<i64>) -> Result<Vec<i64>> {
        self.db.unpromoted_in_range(from, to)
    }

    /// Mark entries as promoted into `changeset`.
    pub fn mark_promoted(&self, entry_ids: &[i64], changeset: &str) -> Result<()> {
        self.db.mark_promoted(entry_ids, changeset)
    }

    /// Build and store a snapshot tree of the working dir; return its hash.
    pub fn snapshot_tree(&self) -> Result<Hash> {
        scan::snapshot_tree(&self.root, &self.store, &self.ignore)
    }

    /// Remove entries older than `cutoff_us`; returns entries removed.
    pub fn prune(&self, cutoff_us: i64) -> Result<u64> {
        self.db.prune(cutoff_us, now_micros())
    }

    /// Watch the working tree and capture after each settled burst,
    /// invoking `on_entry` with any new entry id. Blocks until interrupted.
    pub fn watch<F>(
        &mut self,
        author_kind: AuthorKind,
        author_id: &str,
        debounce: Duration,
        mut on_entry: F,
    ) -> Result<()>
    where
        F: FnMut(i64),
    {
        let root = self.root.clone();
        watch::watch_loop(&root, debounce, || {
            if let Some(id) = self.capture(author_kind, author_id, Source::FsWatch)? {
                on_entry(id);
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Timeline) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".gpp")).unwrap();
        ObjectStore::init(&dir.path().join(".gpp")).unwrap();
        let tl = Timeline::open(dir.path(), ["target/**", "*.log"]).unwrap();
        (dir, tl)
    }

    #[test]
    fn capture_records_adds_then_modifies_then_deletes() {
        let (dir, mut tl) = setup();
        let f = dir.path().join("a.txt");

        std::fs::write(&f, "one\n").unwrap();
        let e1 = tl
            .capture(AuthorKind::Human, "dev1@example.com", Source::Cli)
            .unwrap();
        assert!(e1.is_some());

        // No change → no entry.
        assert!(
            tl.capture(AuthorKind::Human, "dev1@example.com", Source::Cli)
                .unwrap()
                .is_none()
        );

        std::fs::write(&f, "one\ntwo\n").unwrap();
        let e2 = tl
            .capture(AuthorKind::Human, "dev1@example.com", Source::Cli)
            .unwrap();
        assert!(e2.is_some());

        std::fs::remove_file(&f).unwrap();
        let e3 = tl
            .capture(AuthorKind::Human, "dev1@example.com", Source::Cli)
            .unwrap();
        assert!(e3.is_some());

        let entries = tl
            .entries(&EntryFilter {
                limit: Some(50),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].files[0].change, ChangeType::Delete);
    }

    #[test]
    fn ignored_files_are_not_captured() {
        let (dir, mut tl) = setup();
        std::fs::write(dir.path().join("debug.log"), "noise").unwrap();
        std::fs::create_dir_all(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target/x"), "build").unwrap();
        assert!(
            tl.capture(AuthorKind::Human, "d", Source::Cli)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn snapshot_tree_is_deterministic() {
        let (dir, mut tl) = setup();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "fn main() {}").unwrap();
        tl.capture(AuthorKind::Human, "d", Source::Cli).unwrap();
        assert_eq!(tl.snapshot_tree().unwrap(), tl.snapshot_tree().unwrap());
    }

    #[test]
    fn prune_removes_old_entries() {
        let (dir, mut tl) = setup();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        tl.capture(AuthorKind::Human, "d", Source::Cli).unwrap();
        // Cutoff far in the future removes everything.
        let removed = tl.prune(now_micros() + 1_000_000).unwrap();
        assert_eq!(removed, 1);
        assert!(tl.entries(&EntryFilter::default()).unwrap().is_empty());
    }
}

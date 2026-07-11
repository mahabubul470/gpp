//! The staleness engine: deterministic belief invalidation from history.
//!
//! Given a belief and a range of changesets (its anchor → a tip), the engine
//! walks the repo's own first-parent history and classifies every commit
//! whose diff intersects the belief's scope:
//!
//! * evidence span content changed / evidence file deleted → **Invalidated**
//! * scope or symbol overlap only → **StaleCandidate**
//!
//! Everything is diff intersection + blob hashes — zero LLM or network
//! calls. Evidence spans are drift-adjusted commit by commit (a change
//! *above* a span moves it; only a change *inside* it invalidates), so line
//! numbers recorded at the anchor stay meaningful across unrelated edits.

use std::collections::{BTreeMap, HashSet};

use globset::{Glob, GlobSet, GlobSetBuilder};
use gpp_core::{Blob, Hash, ObjectStore, flatten_tree};
use gpp_diff::{LineOp, LineOpKind};
use gpp_history::Changeset;

use crate::belief::{BeliefData, BeliefStatus, Cause, StatusChange};
use crate::error::{Error, Result};
use crate::object::GraphNode;
use crate::store::GraphStore;

/// One commit whose diff intersected a belief's scope.
#[derive(Debug, Clone)]
pub struct ScanHit {
    pub changeset: Hash,
    /// Original git SHA when the changeset came through the git bridge.
    pub git_commit: Option<String>,
    pub timestamp: i64,
    pub message: String,
    pub verdict: BeliefStatus,
    pub causes: Vec<Cause>,
    /// Offending hunk (for evidence/symbol overlaps).
    pub excerpt: Option<String>,
}

/// A changeset in walk order with what the engine needs from it.
struct ChainEntry {
    id: Hash,
    tree: Hash,
    timestamp: i64,
    message: String,
    git_commit: Option<String>,
}

/// First-parent chain from `tip` back to (and including) `anchor`,
/// returned oldest-first. Errors if `anchor` is not on the chain.
fn chain_to_anchor(objects: &ObjectStore, tip: Hash, anchor: Hash) -> Result<Vec<ChainEntry>> {
    let mut chain = Vec::new();
    let mut cursor = Some(tip);
    let mut found = false;
    while let Some(id) = cursor {
        let cs: Changeset = objects.read(&id)?;
        let message = match cs.intent {
            Some(h) => objects
                .read::<gpp_history::Intent>(&h)
                .map(|i| i.description)
                .unwrap_or_else(|_| "(no message)".into()),
            None => "(no message)".into(),
        };
        chain.push(ChainEntry {
            id,
            tree: cs.tree,
            timestamp: cs.timestamp,
            message,
            git_commit: cs.metadata.get("git_commit").cloned(),
        });
        if id == anchor {
            found = true;
            break;
        }
        cursor = cs.parents.first().copied();
    }
    if !found {
        return Err(Error::Other(format!(
            "anchor changeset {} is not an ancestor of {} (first-parent)",
            anchor.short(),
            tip.short()
        )));
    }
    chain.reverse();
    Ok(chain)
}

fn scope_globs(paths: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in paths {
        b.add(Glob::new(p).map_err(|e| Error::Other(format!("bad scope glob {p:?}: {e}")))?);
    }
    b.build()
        .map_err(|e| Error::Other(format!("bad scope globs: {e}")))
}

/// Does any changed op intersect old-side lines `[from, to]`?
/// Inserts count only when they land strictly *inside* the range —
/// an insert at the top boundary merely shifts the range down.
fn ops_intersect(ops: &[LineOp], from: usize, to: usize) -> bool {
    ops.iter().any(|op| match op.kind {
        LineOpKind::Equal => false,
        LineOpKind::Insert => op.old_start > from && op.old_start <= to,
        LineOpKind::Delete | LineOpKind::Replace => {
            op.old_start <= to && op.old_start + op.old_len > from
        }
    })
}

/// Net line delta of all changes strictly above `from` (for span drift).
fn drift_before(ops: &[LineOp], from: usize) -> isize {
    ops.iter()
        .filter(|op| op.kind != LineOpKind::Equal)
        .filter(|op| {
            if op.old_len == 0 {
                op.old_start <= from
            } else {
                op.old_start + op.old_len - 1 < from
            }
        })
        .map(|op| op.new_len as isize - op.old_len as isize)
        .sum()
}

/// Resolve a symbol's current 1-based line span in `content` via tree-sitter.
/// `None` when the language is unsupported or the symbol isn't found.
fn symbol_span(path: &str, content: &[u8], symbol: &str) -> Option<(usize, usize)> {
    let parser = gpp_diff::parser_for_path(path).ok()?;
    let decls = gpp_diff::parse_declarations(parser.as_ref(), content).ok()?;
    decls
        .iter()
        .find(|d| d.name == symbol)
        .map(|d| (d.start_line, d.end_line))
}

/// Scan `anchor..tip` for commits intersecting the belief's scope.
/// Hits are returned oldest-first; drift-adjusted evidence spans decide
/// Invalidated vs StaleCandidate.
pub fn scan(objects: &ObjectStore, belief: &BeliefData, tip: Hash) -> Result<Vec<ScanHit>> {
    let chain = chain_to_anchor(objects, tip, belief.anchor)?;
    let globs = scope_globs(&belief.scope.paths)?;

    // Drift-adjusted evidence spans; None once the evidence is gone.
    let mut spans: Vec<Option<(usize, usize)>> =
        belief.evidence.iter().map(|e| Some(e.span)).collect();

    let mut hits = Vec::new();
    let mut old_files: Option<BTreeMap<String, Hash>> = None; // lazily seeded from anchor

    for window in chain.windows(2) {
        let (parent, commit) = (&window[0], &window[1]);
        let old = match old_files.take() {
            Some(f) => f,
            None => flatten_tree(objects, &parent.tree)?,
        };
        let new = flatten_tree(objects, &commit.tree)?;

        let mut causes: Vec<Cause> = Vec::new();
        let mut excerpt: Option<String> = None;

        let mut paths: Vec<&String> = old.keys().chain(new.keys()).collect();
        paths.sort();
        paths.dedup();

        for path in paths {
            let (oh, nh) = (old.get(path.as_str()), new.get(path.as_str()));
            if oh == nh {
                continue;
            }

            let ev_idx: Vec<usize> = belief
                .evidence
                .iter()
                .enumerate()
                .filter(|(_, e)| &e.path == path)
                .map(|(i, _)| i)
                .collect();
            let symbols: Vec<&crate::belief::SymbolRef> = belief
                .scope
                .symbols
                .iter()
                .filter(|s| &s.path == path)
                .collect();
            let glob_matched = globs.is_match(path.as_str());
            if ev_idx.is_empty() && symbols.is_empty() && !glob_matched {
                continue;
            }

            match (oh, nh) {
                // Deleted file.
                (Some(_), None) => {
                    let mut deleted_evidence = false;
                    for i in &ev_idx {
                        if spans[*i].is_some() {
                            spans[*i] = None;
                            deleted_evidence = true;
                        }
                    }
                    if deleted_evidence {
                        causes.push(Cause::EvidenceDeleted { path: path.clone() });
                    } else {
                        causes.push(Cause::ScopeTouched { path: path.clone() });
                    }
                }
                // Added file: can't carry anchor evidence; scope touch only.
                (None, Some(_)) => {
                    causes.push(Cause::ScopeTouched { path: path.clone() });
                }
                // Modified file.
                (Some(oh), Some(nh)) => {
                    let old_blob = objects.read::<Blob>(oh)?.content;
                    let new_blob = objects.read::<Blob>(nh)?.content;
                    let ops = gpp_diff::line_ops(&old_blob, &new_blob);

                    let mut classified = false;
                    for i in &ev_idx {
                        let Some((from, to)) = spans[*i] else {
                            continue;
                        };
                        match &ops {
                            Some(ops) if !ops_intersect(ops, from, to) => {
                                // Span untouched — drift it and flag the file.
                                let d = drift_before(ops, from);
                                spans[*i] = Some((
                                    (from as isize + d).max(1) as usize,
                                    (to as isize + d).max(1) as usize,
                                ));
                                causes.push(Cause::EvidenceFileTouched { path: path.clone() });
                            }
                            _ => {
                                // Span content changed (or binary — treat a
                                // whole-file change as touching the span).
                                spans[*i] = None;
                                causes.push(Cause::EvidenceChanged {
                                    path: path.clone(),
                                    span: (from, to),
                                });
                                if excerpt.is_none() {
                                    excerpt = gpp_diff::excerpt(&old_blob, &new_blob, from, to, 2);
                                }
                            }
                        }
                        classified = true;
                    }

                    for s in &symbols {
                        let Some((from, to)) = symbol_span(path, &old_blob, &s.name) else {
                            continue;
                        };
                        if let Some(ops) = &ops
                            && ops_intersect(ops, from, to)
                        {
                            causes.push(Cause::SymbolTouched {
                                path: path.clone(),
                                symbol: s.name.clone(),
                            });
                            if excerpt.is_none() {
                                excerpt = gpp_diff::excerpt(&old_blob, &new_blob, from, to, 2);
                            }
                            classified = true;
                        } else if ops.is_some() {
                            // Symbol resolvable and untouched: refined away.
                            classified = true;
                        }
                    }

                    // Plain scope match with no finer signal available.
                    if !classified && glob_matched {
                        causes.push(Cause::ScopeTouched { path: path.clone() });
                    }
                }
                (None, None) => unreachable!("path came from one of the maps"),
            }
        }

        if !causes.is_empty() {
            let verdict = if causes.iter().any(|c| {
                matches!(
                    c,
                    Cause::EvidenceChanged { .. } | Cause::EvidenceDeleted { .. }
                )
            }) {
                BeliefStatus::Invalidated
            } else {
                BeliefStatus::StaleCandidate
            };
            hits.push(ScanHit {
                changeset: commit.id,
                git_commit: commit.git_commit.clone(),
                timestamp: commit.timestamp,
                message: commit.message.clone(),
                verdict,
                causes,
                excerpt,
            });
        }

        old_files = Some(new);
    }

    Ok(hits)
}

/// Scan a belief and record the hits into its append-only history
/// (idempotent). Persists only when something new was recorded.
/// Returns the up-to-date node and the hits from this scan.
pub fn scan_and_record(
    store: &GraphStore,
    id: &Hash,
    tip: Hash,
) -> Result<(GraphNode, Vec<ScanHit>)> {
    let mut node = store.get_node(id)?;
    let Some(mut data) = node.belief.clone() else {
        return Err(Error::Other(format!("node {} is not a belief", id.short())));
    };
    let hits = scan(store.objects(), &data, tip)?;
    let mut changed = false;
    for h in &hits {
        changed |= data.record(StatusChange {
            changeset: h.changeset,
            at: h.timestamp,
            to: h.verdict,
            causes: h.causes.clone(),
        });
    }
    if changed {
        node.belief = Some(data);
        crate::belief::save_belief(store, &node)?;
    }
    Ok((node, hits))
}

/// Every changeset reachable from `from` (all parents, not just first) —
/// the ancestor set used by `belief at` time-travel.
pub fn ancestors(objects: &ObjectStore, from: Hash) -> Result<HashSet<Hash>> {
    let mut seen = HashSet::new();
    let mut stack = vec![from];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let cs: Changeset = objects.read(&id)?;
        stack.extend(cs.parents.iter().copied());
    }
    Ok(seen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::belief::{BeliefStatus, Evidence, Scope, SymbolRef};
    use gpp_core::{EntryKind, Tree, TreeEntry};
    use gpp_history::{Author, Intent, IntentType};

    /// Build a (possibly nested) tree from `path -> content`.
    fn build_tree(store: &ObjectStore, files: &[(&str, &str)]) -> Hash {
        use std::collections::BTreeMap;
        #[derive(Default)]
        struct Dir<'a> {
            files: BTreeMap<String, &'a str>,
            dirs: BTreeMap<String, Dir<'a>>,
        }
        let mut root = Dir::default();
        for (path, content) in files {
            let mut cur = &mut root;
            let parts: Vec<&str> = path.split('/').collect();
            for part in &parts[..parts.len() - 1] {
                cur = cur.dirs.entry(part.to_string()).or_default();
            }
            cur.files
                .insert(parts.last().expect("non-empty path").to_string(), content);
        }
        fn write_dir(store: &ObjectStore, dir: &Dir) -> Hash {
            let mut entries = Vec::new();
            for (name, sub) in &dir.dirs {
                entries.push(TreeEntry {
                    name: name.clone(),
                    kind: EntryKind::Directory,
                    hash: write_dir(store, sub),
                    mode: 0o755,
                    size: 0,
                });
            }
            for (name, content) in &dir.files {
                let blob = store
                    .write(&Blob::new(content.as_bytes().to_vec()))
                    .expect("write blob");
                entries.push(TreeEntry {
                    name: name.clone(),
                    kind: EntryKind::File,
                    hash: blob,
                    mode: 0o644,
                    size: content.len() as u64,
                });
            }
            store.write(&Tree::new(entries)).expect("write tree")
        }
        write_dir(store, &root)
    }

    fn commit(
        store: &ObjectStore,
        parent: Option<Hash>,
        files: &[(&str, &str)],
        msg: &str,
        ts: i64,
    ) -> Hash {
        let tree = build_tree(store, files);
        let intent = store
            .write(&Intent {
                intent_type: IntentType::HumanDirected,
                description: msg.to_string(),
                prompt: None,
                task_reference: None,
                goal: None,
                constraints: vec![],
                timestamp: ts,
            })
            .expect("write intent");
        store
            .write(&Changeset {
                parents: parent.into_iter().collect(),
                tree,
                timestamp: ts,
                author: Author::human("Dev", "dev@example.com"),
                committer: None,
                intent: Some(intent),
                timeline_range: None,
                metadata: Default::default(),
            })
            .expect("write changeset")
    }

    fn objects() -> (tempfile::TempDir, ObjectStore) {
        let d = tempfile::tempdir().expect("tempdir");
        let s = ObjectStore::init(d.path()).expect("init");
        (d, s)
    }

    const TOKEN_V0: &str = "use jwt;\n\npub fn issue_token() -> Token {\n    jwt::encode(claims())\n}\n\npub const EXPIRY_HOURS: u64 = 24;\n";

    fn belief_on_expiry(anchor: Hash, blob: Hash) -> BeliefData {
        BeliefData {
            scope: Scope {
                paths: vec!["auth/**".into()],
                symbols: vec![],
            },
            anchor,
            evidence: vec![Evidence {
                path: "auth/token.rs".into(),
                span: (7, 7), // the EXPIRY_HOURS line
                blob_hash: blob,
            }],
            status: BeliefStatus::Active,
            invalidated_by: None,
            history: vec![],
        }
    }

    fn blob_of(store: &ObjectStore, tree_root: Hash, path: &str) -> Hash {
        *flatten_tree(store, &tree_root)
            .expect("flatten")
            .get(path)
            .expect("path in tree")
    }

    fn tree_of(store: &ObjectStore, cs: Hash) -> Hash {
        store.read::<Changeset>(&cs).expect("changeset").tree
    }

    #[test]
    fn drift_then_invalidation() {
        let (_d, s) = objects();
        let c0 = commit(&s, None, &[("auth/token.rs", TOKEN_V0)], "seed", 1_000);
        let ev_blob = blob_of(&s, tree_of(&s, c0), "auth/token.rs");
        let belief = belief_on_expiry(c0, ev_blob);

        // C1 inserts two lines at the top (evidence drifts 7 → 9) — the
        // span content is untouched, so this is only a stale candidate.
        let v1 = format!("// SPDX\n// new header\n{TOKEN_V0}");
        let c1 = commit(&s, Some(c0), &[("auth/token.rs", &v1)], "add header", 2_000);

        // C2 edits the drifted expiry line — invalidation.
        let v2 = v1.replace("EXPIRY_HOURS: u64 = 24", "EXPIRY_HOURS: u64 = 168");
        let c2 = commit(
            &s,
            Some(c1),
            &[("auth/token.rs", &v2)],
            "7 day expiry",
            3_000,
        );

        let hits = scan(&s, &belief, c2).expect("scan");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].verdict, BeliefStatus::StaleCandidate);
        assert!(matches!(
            hits[0].causes[0],
            Cause::EvidenceFileTouched { .. }
        ));
        assert_eq!(hits[1].verdict, BeliefStatus::Invalidated);
        assert!(matches!(hits[1].causes[0], Cause::EvidenceChanged { .. }));
        let x = hits[1].excerpt.as_deref().expect("offending hunk");
        assert!(x.contains("24") && x.contains("168"), "excerpt: {x}");
    }

    #[test]
    fn deleted_evidence_invalidates() {
        let (_d, s) = objects();
        let c0 = commit(
            &s,
            None,
            &[("auth/token.rs", TOKEN_V0), ("lib.rs", "mod auth;\n")],
            "seed",
            1_000,
        );
        let ev_blob = blob_of(&s, tree_of(&s, c0), "auth/token.rs");
        let belief = belief_on_expiry(c0, ev_blob);

        let c1 = commit(
            &s,
            Some(c0),
            &[
                ("auth/session.rs", "pub fn session() {}\n"),
                ("lib.rs", "mod auth;\n"),
            ],
            "sessions replace tokens",
            2_000,
        );
        let hits = scan(&s, &belief, c1).expect("scan");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].verdict, BeliefStatus::Invalidated);
        assert!(
            hits[0]
                .causes
                .iter()
                .any(|c| matches!(c, Cause::EvidenceDeleted { .. })),
            "causes: {:?}",
            hits[0].causes
        );
    }

    #[test]
    fn symbol_refinement_skips_unrelated_edits() {
        let (_d, s) = objects();
        let src0 =
            "pub fn issue_token() -> u32 {\n    1\n}\n\npub fn refresh() -> u32 {\n    2\n}\n";
        let c0 = commit(&s, None, &[("auth/token.rs", src0)], "seed", 1_000);
        let belief = BeliefData {
            scope: Scope {
                paths: vec![],
                symbols: vec![SymbolRef {
                    path: "auth/token.rs".into(),
                    name: "issue_token".into(),
                }],
            },
            anchor: c0,
            evidence: vec![],
            status: BeliefStatus::Active,
            invalidated_by: None,
            history: vec![],
        };

        // Edit only `refresh` — refined away, no hit.
        let src1 = src0.replace("    2\n", "    42\n");
        let c1 = commit(
            &s,
            Some(c0),
            &[("auth/token.rs", &src1)],
            "tweak refresh",
            2_000,
        );
        assert!(scan(&s, &belief, c1).expect("scan").is_empty());

        // Edit `issue_token` — symbol hit, stale candidate.
        let src2 = src1.replace("    1\n", "    9\n");
        let c2 = commit(
            &s,
            Some(c1),
            &[("auth/token.rs", &src2)],
            "change issue",
            3_000,
        );
        let hits = scan(&s, &belief, c2).expect("scan");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].verdict, BeliefStatus::StaleCandidate);
        assert!(matches!(hits[0].causes[0], Cause::SymbolTouched { .. }));
    }

    #[test]
    fn record_is_append_only_and_idempotent() {
        let (_d, s) = objects();
        let c0 = commit(&s, None, &[("auth/token.rs", TOKEN_V0)], "seed", 1_000);
        let ev_blob = blob_of(&s, tree_of(&s, c0), "auth/token.rs");
        let mut data = belief_on_expiry(c0, ev_blob);
        let v2 = TOKEN_V0.replace("24", "168");
        let c1 = commit(
            &s,
            Some(c0),
            &[("auth/token.rs", &v2)],
            "week expiry",
            2_000,
        );

        let hits = scan(&s, &data, c1).expect("scan");
        for h in &hits {
            assert!(data.record(StatusChange {
                changeset: h.changeset,
                at: h.timestamp,
                to: h.verdict,
                causes: h.causes.clone(),
            }));
        }
        assert_eq!(data.status, BeliefStatus::Invalidated);
        assert_eq!(data.invalidated_by, Some(c1));
        let len = data.history.len();

        // Re-recording the same scan changes nothing.
        for h in &hits {
            assert!(!data.record(StatusChange {
                changeset: h.changeset,
                at: h.timestamp,
                to: h.verdict,
                causes: h.causes.clone(),
            }));
        }
        assert_eq!(data.history.len(), len);
    }

    #[test]
    fn time_travel_status_at() {
        let (_d, s) = objects();
        let c0 = commit(&s, None, &[("auth/token.rs", TOKEN_V0)], "seed", 1_000);
        let ev_blob = blob_of(&s, tree_of(&s, c0), "auth/token.rs");
        let mut data = belief_on_expiry(c0, ev_blob);
        data.history.push(StatusChange {
            changeset: c0,
            at: 1_000,
            to: BeliefStatus::Active,
            causes: vec![],
        });

        let c1 = commit(
            &s,
            Some(c0),
            &[("auth/token.rs", TOKEN_V0)],
            "noop-ish",
            2_000,
        );
        let v2 = TOKEN_V0.replace("24", "168");
        let c2 = commit(
            &s,
            Some(c1),
            &[("auth/token.rs", &v2)],
            "week expiry",
            3_000,
        );

        for h in scan(&s, &data, c2).expect("scan") {
            data.record(StatusChange {
                changeset: h.changeset,
                at: h.timestamp,
                to: h.verdict,
                causes: h.causes.clone(),
            });
        }
        assert_eq!(data.status, BeliefStatus::Invalidated);

        // As of C1 the belief still held; as of C2 it is invalidated.
        let at_c1 = ancestors(&s, c1).expect("ancestors");
        assert_eq!(data.status_at(&at_c1), Some(BeliefStatus::Active));
        let at_c2 = ancestors(&s, c2).expect("ancestors");
        assert_eq!(data.status_at(&at_c2), Some(BeliefStatus::Invalidated));

        // Where the anchor isn't reachable, the belief did not exist.
        let pre = HashSet::new();
        assert_eq!(data.status_at(&pre), None);
    }
}

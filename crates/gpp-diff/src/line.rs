//! Line-based diff: the traditional fallback (unified diffs + per-file stats).
//!
//! Used for binary content and any file whose language has no registered
//! semantic parser.

use similar::{ChangeTag, TextDiff};

/// Added / removed line counts for a single file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileStat {
    pub added: usize,
    pub removed: usize,
}

impl FileStat {
    pub fn is_empty(&self) -> bool {
        self.added == 0 && self.removed == 0
    }
}

/// Compute added/removed line counts between two byte blobs.
///
/// Binary content (invalid UTF-8) is reported as a whole-file replacement
/// rather than a line diff.
pub fn stat(old: &[u8], new: &[u8]) -> FileStat {
    match (std::str::from_utf8(old), std::str::from_utf8(new)) {
        (Ok(o), Ok(n)) => {
            let diff = TextDiff::from_lines(o, n);
            let mut s = FileStat::default();
            for change in diff.iter_all_changes() {
                match change.tag() {
                    ChangeTag::Insert => s.added += 1,
                    ChangeTag::Delete => s.removed += 1,
                    ChangeTag::Equal => {}
                }
            }
            s
        }
        _ => {
            if old == new {
                FileStat::default()
            } else {
                FileStat {
                    added: 1,
                    removed: 1,
                }
            }
        }
    }
}

/// What a [`LineOp`] does to the old side of a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOpKind {
    Equal,
    Delete,
    Insert,
    Replace,
}

/// One diff operation in line coordinates.
///
/// `old_start`/`new_start` are 1-based line numbers of the first affected
/// line on each side; an `Insert` has `old_len == 0` and `old_start` is the
/// line *before which* the insertion happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineOp {
    pub kind: LineOpKind,
    pub old_start: usize,
    pub old_len: usize,
    pub new_start: usize,
    pub new_len: usize,
}

/// Structured per-line diff operations between two text blobs.
///
/// Returns `None` for binary (non-UTF-8) content — callers should treat that
/// as a whole-file change.
pub fn line_ops(old: &[u8], new: &[u8]) -> Option<Vec<LineOp>> {
    let (Ok(o), Ok(n)) = (std::str::from_utf8(old), std::str::from_utf8(new)) else {
        return None;
    };
    let diff = TextDiff::from_lines(o, n);
    let ops = diff
        .ops()
        .iter()
        .map(|op| {
            let (kind, old_range, new_range) = match op {
                similar::DiffOp::Equal {
                    old_index,
                    new_index,
                    len,
                } => (
                    LineOpKind::Equal,
                    *old_index..*old_index + len,
                    *new_index..*new_index + len,
                ),
                similar::DiffOp::Delete {
                    old_index,
                    old_len,
                    new_index,
                } => (
                    LineOpKind::Delete,
                    *old_index..*old_index + old_len,
                    *new_index..*new_index,
                ),
                similar::DiffOp::Insert {
                    old_index,
                    new_index,
                    new_len,
                } => (
                    LineOpKind::Insert,
                    *old_index..*old_index,
                    *new_index..*new_index + new_len,
                ),
                similar::DiffOp::Replace {
                    old_index,
                    old_len,
                    new_index,
                    new_len,
                } => (
                    LineOpKind::Replace,
                    *old_index..*old_index + old_len,
                    *new_index..*new_index + new_len,
                ),
            };
            LineOp {
                kind,
                old_start: old_range.start + 1,
                old_len: old_range.len(),
                new_start: new_range.start + 1,
                new_len: new_range.len(),
            }
        })
        .collect();
    Some(ops)
}

/// Render a short `-`/`+` excerpt of the ops touching old-side lines
/// `[from, to]` (1-based inclusive), each with up to `context` preceding
/// unchanged lines. Used for "offending hunk" displays; `None` if the
/// content is binary or nothing in that range changed.
pub fn excerpt(old: &[u8], new: &[u8], from: usize, to: usize, context: usize) -> Option<String> {
    let (Ok(o), Ok(n)) = (std::str::from_utf8(old), std::str::from_utf8(new)) else {
        return None;
    };
    let old_lines: Vec<&str> = o.lines().collect();
    let new_lines: Vec<&str> = n.lines().collect();
    let ops = line_ops(old, new)?;

    let mut out = String::new();
    let mut hit = false;
    for op in &ops {
        if op.kind == LineOpKind::Equal {
            continue;
        }
        // An insert (old_len == 0) intersects when it lands strictly inside
        // the range; deletes/replaces when their old span overlaps it.
        let overlaps = if op.old_len == 0 {
            op.old_start > from && op.old_start <= to
        } else {
            op.old_start <= to && op.old_start + op.old_len > from
        };
        if !overlaps {
            continue;
        }
        hit = true;
        let ctx_from = op.old_start.saturating_sub(context).max(1);
        for i in ctx_from..op.old_start {
            if let Some(l) = old_lines.get(i - 1) {
                out.push_str(&format!("   {i:>5} | {l}\n"));
            }
        }
        for i in op.old_start..op.old_start + op.old_len {
            if let Some(l) = old_lines.get(i - 1) {
                out.push_str(&format!(" - {i:>5} | {l}\n"));
            }
        }
        for j in op.new_start..op.new_start + op.new_len {
            if let Some(l) = new_lines.get(j - 1) {
                out.push_str(&format!(" + {j:>5} | {l}\n"));
            }
        }
    }
    hit.then_some(out)
}

/// Render a unified diff for one file. `path` labels the `---`/`+++` headers.
///
/// Returns `None` when the contents are identical. Binary differences yield a
/// one-line `Binary files differ` marker.
pub fn unified(path: &str, old: &[u8], new: &[u8]) -> Option<String> {
    if old == new {
        return None;
    }
    let (Ok(o), Ok(n)) = (std::str::from_utf8(old), std::str::from_utf8(new)) else {
        return Some(format!("--- a/{path}\n+++ b/{path}\nBinary files differ\n"));
    };

    let diff = TextDiff::from_lines(o, n);
    let mut out = format!("--- a/{path}\n+++ b/{path}\n");
    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            out.push_str("...\n");
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    ChangeTag::Insert => '+',
                    ChangeTag::Delete => '-',
                    ChangeTag::Equal => ' ',
                };
                out.push(sign);
                out.push_str(change.value());
                if !change.value().ends_with('\n') {
                    out.push_str("\n\\ No newline at end of file\n");
                }
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_is_none() {
        assert!(unified("a.txt", b"same\n", b"same\n").is_none());
        assert!(stat(b"x", b"x").is_empty());
    }

    #[test]
    fn counts_added_and_removed() {
        let s = stat(b"a\nb\nc\n", b"a\nB\nc\nd\n");
        assert_eq!(s.added, 2); // "B" and "d"
        assert_eq!(s.removed, 1); // "b"
    }

    #[test]
    fn unified_has_headers_and_signs() {
        let d = unified("f.rs", b"one\ntwo\n", b"one\ntwo changed\n").unwrap();
        assert!(d.contains("--- a/f.rs"));
        assert!(d.contains("+++ b/f.rs"));
        assert!(d.contains("-two\n"));
        assert!(d.contains("+two changed\n"));
    }

    #[test]
    fn line_ops_report_ranges() {
        // old: a b c   new: a X c d  → replace line 2, insert after line 3.
        let ops = line_ops(b"a\nb\nc\n", b"a\nX\nc\nd\n").unwrap();
        let changed: Vec<_> = ops.iter().filter(|o| o.kind != LineOpKind::Equal).collect();
        assert_eq!(changed.len(), 2);
        assert_eq!(changed[0].kind, LineOpKind::Replace);
        assert_eq!((changed[0].old_start, changed[0].old_len), (2, 1));
        assert_eq!(changed[1].kind, LineOpKind::Insert);
        assert_eq!(changed[1].old_len, 0);
        assert_eq!(changed[1].new_len, 1);
    }

    #[test]
    fn line_ops_binary_is_none() {
        assert!(line_ops(&[0, 159], b"text\n").is_none());
    }

    #[test]
    fn excerpt_targets_range() {
        let old = b"l1\nl2\nl3\nl4\nl5\n";
        let new = b"l1\nl2\nCHANGED\nl4\nl5\n";
        // Change is on line 3: an excerpt for [3,3] sees it…
        let x = excerpt(old, new, 3, 3, 1).unwrap();
        assert!(x.contains("- ") && x.contains("l3"));
        assert!(x.contains("+ ") && x.contains("CHANGED"));
        // …an excerpt for [5,5] does not.
        assert!(excerpt(old, new, 5, 5, 1).is_none());
    }

    #[test]
    fn binary_is_marked() {
        let d = unified("img.png", &[0, 159, 146], &[0, 1, 2]).unwrap();
        assert!(d.contains("Binary files differ"));
    }
}

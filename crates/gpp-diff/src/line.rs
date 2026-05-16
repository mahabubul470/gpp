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
    fn binary_is_marked() {
        let d = unified("img.png", &[0, 159, 146], &[0, 1, 2]).unwrap();
        assert!(d.contains("Binary files differ"));
    }
}

//! Structural diff over extracted [`Declaration`]s.
//!
//! Per file we classify each declaration as added, removed, modified
//! (same name, different body) or renamed (different name, identical body).
//! Across files, [`detect_moves`] promotes a remove-here / add-there pair
//! with the same body fingerprint into a single move.

use crate::error::Result;
use crate::lang::detect_language;
use crate::parser::{Declaration, parse_declarations};

/// One structural change to a declaration within a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeOp {
    Added(Declaration),
    Removed(Declaration),
    /// Same name, changed signature/body.
    Modified {
        old: Declaration,
        new: Declaration,
    },
    /// Different name, identical body.
    Renamed {
        old: Declaration,
        new: Declaration,
    },
}

/// The semantic diff of a single file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSemanticDiff {
    pub path: String,
    pub ops: Vec<ChangeOp>,
}

impl FileSemanticDiff {
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

/// A declaration that left one file and reappeared in another unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    pub kind: String,
    pub name: String,
    pub from_path: String,
    pub to_path: String,
}

/// Semantic-diff one file. `path` selects the parser by extension.
///
/// Files whose language has no parser, or that are not UTF-8, return an
/// error so callers can fall back to the line-based diff.
pub fn semantic(path: &str, old: &[u8], new: &[u8]) -> Result<FileSemanticDiff> {
    let lang = detect_language(path).ok_or(crate::Error::UnsupportedLanguage)?;
    let parser = crate::parser_for(lang)?;
    let old_decls = parse_declarations(parser.as_ref(), old)?;
    let new_decls = parse_declarations(parser.as_ref(), new)?;
    Ok(FileSemanticDiff {
        path: path.to_string(),
        ops: diff_decls(old_decls, new_decls),
    })
}

fn diff_decls(old: Vec<Declaration>, new: Vec<Declaration>) -> Vec<ChangeOp> {
    let mut ops = Vec::new();
    let mut old_left: Vec<Declaration> = Vec::new();
    let mut new_left: Vec<Declaration> = new.clone();

    // Pass 1: match by name.
    for o in old {
        if let Some(pos) = new_left
            .iter()
            .position(|n| n.name == o.name && n.kind == o.kind)
        {
            let n = new_left.remove(pos);
            if n.full_fp != o.full_fp {
                ops.push(ChangeOp::Modified { old: o, new: n });
            }
            // identical → no op
        } else {
            old_left.push(o);
        }
    }

    // Pass 2: rename — unmatched old + unmatched new with equal body_fp.
    let mut still_removed = Vec::new();
    for o in old_left {
        if let Some(pos) = new_left
            .iter()
            .position(|n| n.body_fp == o.body_fp && n.kind == o.kind)
        {
            let n = new_left.remove(pos);
            ops.push(ChangeOp::Renamed { old: o, new: n });
        } else {
            still_removed.push(o);
        }
    }

    for o in still_removed {
        ops.push(ChangeOp::Removed(o));
    }
    for n in new_left {
        ops.push(ChangeOp::Added(n));
    }

    ops.sort_by_key(op_sort_line);
    ops
}

fn op_sort_line(op: &ChangeOp) -> usize {
    match op {
        ChangeOp::Added(d) | ChangeOp::Removed(d) => d.start_line,
        ChangeOp::Modified { new, .. } | ChangeOp::Renamed { new, .. } => new.start_line,
    }
}

/// Promote cross-file "removed here, added there" pairs into moves.
///
/// Mutates the per-file diffs in place (dropping the matched add/remove ops)
/// and returns the moves found. Matching is by `body_fp` + kind; a same-name
/// match is preferred when several candidates share a fingerprint.
pub fn detect_moves(files: &mut [FileSemanticDiff]) -> Vec<Move> {
    // Collect (file_index, op_index, decl) for every Removed / Added.
    let mut removed: Vec<(usize, usize, Declaration)> = Vec::new();
    let mut added: Vec<(usize, usize, Declaration)> = Vec::new();
    for (fi, f) in files.iter().enumerate() {
        for (oi, op) in f.ops.iter().enumerate() {
            match op {
                ChangeOp::Removed(d) => removed.push((fi, oi, d.clone())),
                ChangeOp::Added(d) => added.push((fi, oi, d.clone())),
                _ => {}
            }
        }
    }

    let mut moves = Vec::new();
    let mut drop_ops: Vec<(usize, usize)> = Vec::new();
    let mut used_added = vec![false; added.len()];

    for (rfi, roi, rd) in &removed {
        // Prefer same-name match, then any same-body match in another file.
        let pick = added
            .iter()
            .enumerate()
            .filter(|(ai, (afi, _, ad))| {
                !used_added[*ai] && afi != rfi && ad.body_fp == rd.body_fp && ad.kind == rd.kind
            })
            .min_by_key(|(_, (_, _, ad))| if ad.name == rd.name { 0 } else { 1 });

        if let Some((ai, (afi, aoi, ad))) = pick {
            used_added[ai] = true;
            moves.push(Move {
                kind: rd.kind.clone(),
                name: if ad.name == rd.name {
                    rd.name.clone()
                } else {
                    format!("{} → {}", rd.name, ad.name)
                },
                from_path: files[*rfi].path.clone(),
                to_path: files[*afi].path.clone(),
            });
            drop_ops.push((*rfi, *roi));
            drop_ops.push((*afi, *aoi));
        }
    }

    // Remove matched ops high-index-first so earlier indices stay valid.
    drop_ops.sort_by(|a, b| b.cmp(a));
    for (fi, oi) in drop_ops {
        files[fi].ops.remove(oi);
    }
    moves
}

/// Render a human-readable semantic diff for one file plus any moves.
pub fn render(diff: &FileSemanticDiff, moves: &[Move]) -> String {
    let mut out = String::new();
    let file_moves: Vec<&Move> = moves
        .iter()
        .filter(|m| m.from_path == diff.path || m.to_path == diff.path)
        .collect();
    if diff.ops.is_empty() && file_moves.is_empty() {
        return out;
    }
    out.push_str(&diff.path);
    out.push('\n');
    for op in &diff.ops {
        match op {
            ChangeOp::Added(d) => {
                out.push_str(&format!("  + {} {}\n", d.kind, d.name));
            }
            ChangeOp::Removed(d) => {
                out.push_str(&format!("  - {} {}\n", d.kind, d.name));
            }
            ChangeOp::Modified { new, .. } => {
                out.push_str(&format!("  ~ {} {}  (body changed)\n", new.kind, new.name));
            }
            ChangeOp::Renamed { old, new } => {
                out.push_str(&format!(
                    "  » {} {} → {}  (renamed)\n",
                    new.kind, old.name, new.name
                ));
            }
        }
    }
    for m in file_moves {
        if m.from_path == diff.path {
            out.push_str(&format!(
                "  ↦ {} {}  moved to {}\n",
                m.kind, m.name, m.to_path
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_add_remove_modify() {
        let old = b"fn keep() {}\nfn gone() { panic!(\"old\") }\nfn tweak(n: u8) -> u8 { n }\n";
        let new = b"fn keep() {}\nfn tweak(n: u8) -> u8 { n + 1 }\nfn fresh() { todo!(\"new\") }\n";
        let d = semantic("a.rs", old, new).unwrap();
        let kinds: Vec<_> = d
            .ops
            .iter()
            .map(|o| match o {
                ChangeOp::Added(x) => format!("+{}", x.name),
                ChangeOp::Removed(x) => format!("-{}", x.name),
                ChangeOp::Modified { new, .. } => format!("~{}", new.name),
                ChangeOp::Renamed { old, new } => format!("{}>{}", old.name, new.name),
            })
            .collect();
        assert!(kinds.contains(&"-gone".to_string()));
        assert!(kinds.contains(&"+fresh".to_string()));
        assert!(kinds.contains(&"~tweak".to_string()));
        assert!(!kinds.iter().any(|k| k.contains("keep")));
    }

    #[test]
    fn detects_rename() {
        let old = b"fn original(a: u8, b: u8) -> u8 { a + b }\n";
        let new = b"fn combined(a: u8, b: u8) -> u8 { a + b }\n";
        let d = semantic("x.rs", old, new).unwrap();
        assert_eq!(d.ops.len(), 1);
        assert!(matches!(&d.ops[0], ChangeOp::Renamed { .. }));
    }

    #[test]
    fn detects_cross_file_move() {
        let mut files = vec![
            semantic("from.rs", b"fn shared(n: u8) -> u8 { n * 2 }\n", b"").unwrap(),
            semantic("to.rs", b"", b"fn shared(n: u8) -> u8 { n * 2 }\n").unwrap(),
        ];
        let moves = detect_moves(&mut files);
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].name, "shared");
        assert_eq!(moves[0].from_path, "from.rs");
        assert_eq!(moves[0].to_path, "to.rs");
        assert!(files.iter().all(|f| f.ops.is_empty()));
    }

    #[test]
    fn unsupported_language_errors() {
        assert!(semantic("notes.md", b"a", b"b").is_err());
    }
}

//! Auto-inference: propose graph nodes from structural code changes.
//!
//! After a changeset is promoted, the set of changed paths is inspected for
//! new modules/crates. Each distinct module root that is not already a node
//! becomes a *proposed* `Module` node (subject to human approval) — never an
//! `Active` one. This is deliberately conservative: it suggests, the human
//! decides (`docs/GRAPHEX_PROTOCOL.md`).

use std::collections::BTreeSet;

/// A suggested module: `(name, why)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub name: String,
    pub reason: String,
}

/// Derive a module root from a path:
/// * `crates/<x>/...`  → `x`   (Cargo workspace member)
/// * `src/<x>/...`     → `x`   (module under a src tree)
/// * `<x>/...`         → `x`   (top-level directory)
/// * bare file         → file stem
fn module_root(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match parts.as_slice() {
        ["crates", x, ..] | ["src", x, ..] => Some(x.to_string()),
        [x, _rest @ ..] if parts.len() > 1 => Some(x.to_string()),
        [only] => std::path::Path::new(only)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned()),
        _ => None,
    }
}

/// Suggest module nodes for changed paths, skipping ones already known.
pub fn suggest_modules(changed: &[String], existing_names: &BTreeSet<String>) -> Vec<Suggestion> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for p in changed {
        let Some(root) = module_root(p) else { continue };
        if existing_names.contains(&root) || !seen.insert(root.clone()) {
            continue;
        }
        out.push(Suggestion {
            reason: format!("new/changed code under {root:?} (e.g. {p})"),
            name: root,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_roots_and_dedupes() {
        let changed = vec![
            "crates/gpp-retry/src/lib.rs".to_string(),
            "crates/gpp-retry/src/queue.rs".to_string(),
            "src/payments/mod.rs".to_string(),
            "README.md".to_string(),
        ];
        let mut existing = BTreeSet::new();
        existing.insert("README".to_string());
        let s = suggest_modules(&changed, &existing);
        let names: Vec<_> = s.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["gpp-retry", "payments"]);
    }

    #[test]
    fn skips_existing() {
        let changed = vec!["crates/known/src/a.rs".to_string()];
        let existing = BTreeSet::from(["known".to_string()]);
        assert!(suggest_modules(&changed, &existing).is_empty());
    }
}

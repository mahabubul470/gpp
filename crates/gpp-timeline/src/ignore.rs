//! `.gitignore`-style path matching for `.gppignore` + the configured
//! `[timeline].ignore` list.
//!
//! This implements the common subset of gitignore semantics:
//! - blank lines and `#` comments are skipped
//! - a leading `!` negates (un-ignores) a previously matched path
//! - a trailing `/` restricts the pattern to directories (and their contents)
//! - a pattern containing a non-trailing `/` is anchored to the repo root;
//!   a pattern with no `/` matches that basename at any depth
//! - `*`, `**` and `?` glob wildcards are supported
//!
//! `.gpp/` is always ignored regardless of configuration.

use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::error::{Error, Result};

/// Compiled ignore rules. Match paths relative to the repo root, `/`-separated.
pub struct IgnoreMatcher {
    ignore: GlobSet,
    unignore: GlobSet,
}

impl IgnoreMatcher {
    /// Build from a list of gitignore-style patterns (config + `.gppignore`).
    pub fn new<I, S>(patterns: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut ignore = GlobSetBuilder::new();
        let mut unignore = GlobSetBuilder::new();

        // `.gpp/` is never captured.
        for g in expand(".gpp/") {
            ignore.add(compile(&g)?);
        }

        for raw in patterns {
            let line = raw.as_ref().trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (neg, body) = match line.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, line),
            };
            let target = if neg { &mut unignore } else { &mut ignore };
            for g in expand(body) {
                target.add(compile(&g)?);
            }
        }

        Ok(Self {
            ignore: ignore.build().map_err(|e| Error::Ignore {
                pattern: "<set>".into(),
                source: e,
            })?,
            unignore: unignore.build().map_err(|e| Error::Ignore {
                pattern: "<set>".into(),
                source: e,
            })?,
        })
    }

    /// True if `rel_path` (relative, `/`-separated) should be ignored.
    pub fn is_ignored(&self, rel_path: &str) -> bool {
        let p = rel_path.trim_start_matches("./");
        self.ignore.is_match(p) && !self.unignore.is_match(p)
    }
}

/// Translate one gitignore pattern into glob strings: the path itself plus a
/// `…/**` form so a matched directory also ignores its contents.
fn expand(pattern: &str) -> Vec<String> {
    let no_trailing = pattern.trim_end_matches('/');
    let leading_anchor = no_trailing.starts_with('/');
    let inner = no_trailing.trim_start_matches('/');
    if inner.is_empty() {
        return Vec::new();
    }
    // Anchored to root if there is any slash other than a trailing one.
    let anchored = leading_anchor || inner.contains('/');
    let base = if anchored {
        inner.to_string()
    } else {
        format!("**/{inner}")
    };
    vec![base.clone(), format!("{base}/**")]
}

fn compile(glob: &str) -> Result<Glob> {
    Glob::new(glob).map_err(|e| Error::Ignore {
        pattern: glob.to_string(),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pats: &[&str]) -> IgnoreMatcher {
        IgnoreMatcher::new(pats.iter().copied()).unwrap()
    }

    #[test]
    fn gpp_dir_always_ignored() {
        let im = m(&[]);
        assert!(im.is_ignored(".gpp/objects/aa/bb"));
        assert!(im.is_ignored(".gpp"));
    }

    #[test]
    fn default_patterns() {
        let im = m(&["target/**", "*.pyc", "node_modules/**", "__pycache__/**"]);
        assert!(im.is_ignored("target/debug/foo"));
        assert!(im.is_ignored("src/__pycache__/x.pyc"));
        assert!(im.is_ignored("a/b/c.pyc"));
        assert!(im.is_ignored("node_modules/left-pad/index.js"));
        assert!(!im.is_ignored("src/main.rs"));
    }

    #[test]
    fn negation_unignores() {
        let im = m(&["build/**", "!build/keep.txt"]);
        assert!(im.is_ignored("build/out.o"));
        assert!(!im.is_ignored("build/keep.txt"));
    }

    #[test]
    fn basename_matches_any_depth() {
        let im = m(&["secret.key"]);
        assert!(im.is_ignored("secret.key"));
        assert!(im.is_ignored("deep/nested/secret.key"));
    }
}

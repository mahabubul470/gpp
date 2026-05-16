//! Repository discovery and the `.gpp/` directory layout.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// The set of directories created inside `.gpp/` on `gpp init`.
///
/// Mirrors the layout documented in `docs/ARCHITECTURE.md`. Subsystems that
/// own these directories are implemented in later roadmap phases; the skeleton
/// is created up front so the on-disk contract is stable.
pub const GPP_SUBDIRS: &[&str] = &[
    "objects",
    "timeline",
    "graphex",
    "graphex/keys",
    "trust",
    "policies",
    "reviews",
    "rbac",
    "notify",
    "remote",
    "remote/cache",
    "refs",
    "refs/explorations",
    "refs/agents",
    "git-bridge",
];

/// An opened gpp repository.
pub struct Repo {
    /// Working tree root (the directory that contains `.gpp/`).
    pub root: PathBuf,
}

impl Repo {
    /// The `.gpp/` directory.
    pub fn gpp_dir(&self) -> PathBuf {
        self.root.join(".gpp")
    }

    /// Path to the repository config file.
    pub fn config_path(&self) -> PathBuf {
        self.gpp_dir().join("config.toml")
    }

    /// Path to `HEAD`.
    pub fn head_path(&self) -> PathBuf {
        self.gpp_dir().join("HEAD")
    }

    /// Discover the repository containing `start` by walking upward.
    ///
    /// `start` may be an explicit `--repo` path or the current directory.
    pub fn discover(start: &Path) -> Result<Repo> {
        let start = start
            .canonicalize()
            .with_context(|| format!("cannot access path {}", start.display()))?;
        for dir in start.ancestors() {
            if dir.join(".gpp").is_dir() {
                return Ok(Repo {
                    root: dir.to_path_buf(),
                });
            }
        }
        bail!(
            "not a gpp repository (no .gpp/ found in {} or any parent)",
            start.display()
        );
    }

    /// Current branch name, parsed from `HEAD` (`ref: refs/<name>`).
    pub fn current_branch(&self) -> Result<String> {
        let head =
            std::fs::read_to_string(self.head_path()).with_context(|| "failed to read HEAD")?;
        let head = head.trim();
        let name = head
            .strip_prefix("ref: refs/")
            .with_context(|| format!("HEAD is not a symbolic ref: {head:?}"))?;
        Ok(name.to_string())
    }

    /// Count stored objects (files under `objects/`, ignoring temp files).
    pub fn object_count(&self) -> usize {
        let objects = self.gpp_dir().join("objects");
        let mut count = 0;
        let mut stack = vec![objects];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if !entry.file_name().to_string_lossy().starts_with(".tmp-") {
                    count += 1;
                }
            }
        }
        count
    }
}

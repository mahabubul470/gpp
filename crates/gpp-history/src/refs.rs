//! Branch refs and `HEAD`, stored as plain files under `.gpp/`.
//!
//! `.gpp/HEAD` holds `ref: refs/<name>\n`. Each branch is a file
//! `.gpp/refs/<name>` whose contents are the tip changeset's base32 hash.
//! Exploration branches live under `refs/explorations/`.

use std::path::{Path, PathBuf};

use gpp_core::Hash;

use crate::error::{Error, Result};

pub struct RefStore {
    gpp_dir: PathBuf,
}

/// A branch and its tip (if any changesets have been promoted to it).
#[derive(Debug, Clone)]
pub struct BranchInfo {
    pub name: String,
    pub tip: Option<Hash>,
    pub is_head: bool,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}

impl RefStore {
    pub fn open(gpp_dir: &Path) -> Self {
        Self {
            gpp_dir: gpp_dir.to_path_buf(),
        }
    }

    fn head_path(&self) -> PathBuf {
        self.gpp_dir.join("HEAD")
    }

    fn ref_path(&self, name: &str) -> PathBuf {
        self.gpp_dir.join("refs").join(name)
    }

    /// Branch name `HEAD` points at.
    pub fn head_branch(&self) -> Result<String> {
        let head = std::fs::read_to_string(self.head_path())?;
        head.trim()
            .strip_prefix("ref: refs/")
            .map(|s| s.to_string())
            .ok_or_else(|| Error::Other(format!("HEAD is not a symbolic ref: {head:?}")))
    }

    /// Point `HEAD` at `name` (the branch need not have a tip yet).
    pub fn set_head_branch(&self, name: &str) -> Result<()> {
        if !valid_name(name) {
            return Err(Error::InvalidRefName(name.to_string()));
        }
        std::fs::write(self.head_path(), format!("ref: refs/{name}\n"))?;
        Ok(())
    }

    /// Tip changeset of a branch, or `None` if it has no changesets / no file.
    pub fn read_ref(&self, name: &str) -> Result<Option<Hash>> {
        match std::fs::read_to_string(self.ref_path(name)) {
            Ok(s) => {
                let t = s.trim();
                if t.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(Hash::from_base32(t).map_err(|e| {
                        Error::Other(format!("corrupt ref {name:?}: {e}"))
                    })?))
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Io(e)),
        }
    }

    /// Set a branch tip (creating the ref file and any parent dirs).
    pub fn write_ref(&self, name: &str, tip: Hash) -> Result<()> {
        if !valid_name(name) {
            return Err(Error::InvalidRefName(name.to_string()));
        }
        let path = self.ref_path(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, format!("{}\n", tip.to_base32()))?;
        Ok(())
    }

    pub fn ref_exists(&self, name: &str) -> bool {
        self.ref_path(name).is_file()
    }

    pub fn delete_ref(&self, name: &str) -> Result<()> {
        let path = self.ref_path(name);
        if !path.is_file() {
            return Err(Error::NoSuchBranch(name.to_string()));
        }
        std::fs::remove_file(path)?;
        Ok(())
    }

    /// Tip of the currently checked-out branch.
    pub fn head_tip(&self) -> Result<Option<Hash>> {
        self.read_ref(&self.head_branch()?)
    }

    /// All branches (recursively under `refs/`), sorted by name.
    pub fn list(&self) -> Result<Vec<BranchInfo>> {
        let head = self.head_branch().unwrap_or_default();
        let refs_root = self.gpp_dir.join("refs");
        let mut out = Vec::new();
        let mut stack = vec![refs_root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for ent in rd.flatten() {
                let p = ent.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.is_file() {
                    let name = p
                        .strip_prefix(&refs_root)
                        .unwrap_or(&p)
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/");
                    let tip = self.read_ref(&name)?;
                    out.push(BranchInfo {
                        is_head: name == head,
                        name,
                        tip,
                    });
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }
}

//! Plain data types shared across the timeline crate.

use gpp_core::Hash;

/// Whether a timeline action was taken by a human or an AI agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorKind {
    Human,
    Agent,
}

impl AuthorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthorKind::Human => "human",
            AuthorKind::Agent => "agent",
        }
    }
    pub fn from_db(s: &str) -> Self {
        match s {
            "agent" => AuthorKind::Agent,
            _ => AuthorKind::Human,
        }
    }
}

/// Where a timeline entry originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Editor,
    Cli,
    AgentSdk,
    FsWatch,
    Import,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Editor => "editor",
            Source::Cli => "cli",
            Source::AgentSdk => "agent-sdk",
            Source::FsWatch => "fs-watch",
            Source::Import => "import",
        }
    }
}

/// Kind of change to a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Add,
    Modify,
    Delete,
    Rename,
}

impl ChangeType {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeType::Add => "add",
            ChangeType::Modify => "modify",
            ChangeType::Delete => "delete",
            ChangeType::Rename => "rename",
        }
    }
    pub fn from_db(s: &str) -> Self {
        match s {
            "add" => ChangeType::Add,
            "delete" => ChangeType::Delete,
            "rename" => ChangeType::Rename,
            _ => ChangeType::Modify,
        }
    }
}

/// One file's change within a timeline entry.
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    /// Post-change blob hash (`None` for deletes).
    pub blob_hash: Option<Hash>,
    pub change: ChangeType,
    /// Pre-change blob hash (`None` for adds).
    pub old_hash: Option<Hash>,
    pub old_path: Option<String>,
}

/// A timeline entry as returned by queries.
#[derive(Debug, Clone)]
pub struct EntryView {
    pub id: i64,
    pub timestamp: i64,
    pub author_kind: AuthorKind,
    pub author_id: String,
    pub source: String,
    pub summary: Option<String>,
    pub parent_id: Option<i64>,
    pub promoted_to: Option<String>,
    pub files: Vec<FileChange>,
}

/// Filters for [`crate::Timeline::entries`].
#[derive(Default)]
pub struct EntryFilter {
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub author: Option<String>,
    pub file_glob: Option<globset::GlobMatcher>,
    pub limit: Option<u32>,
}

//! Walk the changeset DAG for `gpp log`.

use gpp_core::{Hash, ObjectStore};

use crate::error::Result;
use crate::object::{Changeset, Intent};

/// One changeset plus its resolved intent (for display).
#[derive(Debug)]
pub struct ChangesetRecord {
    pub id: Hash,
    pub changeset: Changeset,
    pub intent: Option<Intent>,
}

impl ChangesetRecord {
    /// Human-readable message (intent description, or a placeholder).
    pub fn message(&self) -> &str {
        self.intent
            .as_ref()
            .map(|i| i.description.as_str())
            .unwrap_or("(no message)")
    }
}

/// Walk history from `start` following the first parent, newest first,
/// returning up to `limit` records.
pub fn walk(
    store: &ObjectStore,
    start: Option<Hash>,
    limit: usize,
) -> Result<Vec<ChangesetRecord>> {
    let mut out = Vec::new();
    let mut cursor = start;
    while let Some(id) = cursor {
        if out.len() >= limit {
            break;
        }
        let changeset: Changeset = store.read(&id)?;
        let intent = match changeset.intent {
            Some(h) => Some(store.read::<Intent>(&h)?),
            None => None,
        };
        let next = changeset.parents.first().copied();
        out.push(ChangesetRecord {
            id,
            changeset,
            intent,
        });
        cursor = next;
    }
    Ok(out)
}

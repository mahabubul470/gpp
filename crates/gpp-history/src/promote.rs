//! Promote unpromoted timeline entries into a curated [`Changeset`].

use gpp_timeline::{AuthorKind, Source, Timeline};

use crate::error::{Error, Result};
use crate::object::{Author, AuthorType, Changeset, Intent, IntentType};
use crate::refs::RefStore;

/// Inputs to [`promote`].
pub struct PromoteOptions {
    /// Inclusive timeline entry id range (`None` = open-ended).
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub message: String,
    pub intent_type: IntentType,
    pub task: Option<String>,
    pub author: Author,
}

/// What [`promote`] produced.
#[derive(Debug)]
pub struct PromoteOutcome {
    pub changeset: gpp_core::Hash,
    pub intent: gpp_core::Hash,
    pub entries_promoted: usize,
    pub branch: String,
}

/// Capture any pending edits, then fold the unpromoted entries in range into
/// a new changeset on the current branch and advance the branch ref.
pub fn promote(
    timeline: &mut Timeline,
    refs: &RefStore,
    opts: PromoteOptions,
) -> Result<PromoteOutcome> {
    // Make sure freshly-edited files are captured before we snapshot.
    let kind = match opts.author.author_type {
        AuthorType::Human => AuthorKind::Human,
        AuthorType::Agent => AuthorKind::Agent,
    };
    timeline.capture(kind, &opts.author.identity, Source::Cli)?;

    let entry_ids = timeline.unpromoted_in_range(opts.from, opts.to)?;
    if entry_ids.is_empty() {
        return Err(Error::NothingToPromote);
    }

    let timestamp = gpp_timeline::now_micros();
    let tree = timeline.snapshot_tree()?;

    let intent = Intent {
        intent_type: opts.intent_type,
        description: opts.message.clone(),
        prompt: None,
        task_reference: opts.task.clone(),
        goal: None,
        constraints: Vec::new(),
        timestamp,
    };
    let intent_id = timeline.store().write(&intent)?;

    let branch = refs.head_branch()?;
    let parents = refs.read_ref(&branch)?.map(|h| vec![h]).unwrap_or_default();

    let range = (
        *entry_ids.first().expect("non-empty"),
        *entry_ids.last().expect("non-empty"),
    );
    let changeset = Changeset {
        parents,
        tree,
        timestamp,
        author: opts.author.clone(),
        committer: None,
        intent: Some(intent_id),
        timeline_range: Some(range),
        metadata: Default::default(),
    };
    let cs_id = timeline.store().write(&changeset)?;

    timeline.mark_promoted(&entry_ids, &cs_id.to_base32())?;
    refs.write_ref(&branch, cs_id)?;

    Ok(PromoteOutcome {
        changeset: cs_id,
        intent: intent_id,
        entries_promoted: entry_ids.len(),
        branch,
    })
}

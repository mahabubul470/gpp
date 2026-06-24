//! Phase 6 commands: `gpp review`, `gpp rbac`, `gpp inbox`, `gpp notify`,
//! plus the promotion hook that opens a review and emits an event.

use std::path::Path;

use anyhow::{Result, anyhow, bail};
use gpp_core::{Hash, ObjectStore};
use gpp_notify::{EventType, HttpSender, Notifier};
use gpp_rbac::{BranchProtection, Role, RoleStore};
use gpp_review::{Decision, ReviewStatus, ReviewStore};

use crate::cli::{
    InboxAction, InboxArgs, NotifyAction, NotifyArgs, RbacAction, RbacArgs, ReviewAction,
    ReviewArgs,
};
use crate::config;
use crate::repo::Repo;

fn discover(repo_override: Option<&Path>) -> Result<Repo> {
    let start = match repo_override {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    Repo::discover(&start)
}

/// Current user's identity (global config `[user].email`).
fn whoami() -> String {
    config::global_config_path()
        .ok()
        .and_then(|p| config::load_doc(&p).ok())
        .and_then(|d| {
            config::get_key(&d, "user.email").and_then(|v| v.as_str().map(str::to_string))
        })
        .unwrap_or_else(|| "unknown@localhost".into())
}

/// Resolve a changeset spec (`HEAD`, a branch, a short or full hash) to its
/// canonical full base32 id — the key reviews are stored under.
pub(crate) fn resolve_changeset(repo: &Repo, spec: &str) -> Result<String> {
    let s = spec.strip_prefix("cs:").unwrap_or(spec);
    let refs = gpp_history::RefStore::open(&repo.gpp_dir());
    if s.eq_ignore_ascii_case("HEAD") {
        return Ok(refs
            .head_tip()?
            .ok_or_else(|| anyhow!("HEAD has no changesets"))?
            .to_base32());
    }
    if let Ok(h) = Hash::from_base32(s) {
        return Ok(h.to_base32());
    }
    if let Some(tip) = refs.read_ref(s)? {
        return Ok(tip.to_base32());
    }
    // Unique short-hash prefix over stored objects.
    let matches: Vec<String> = ObjectStore::open(&repo.gpp_dir())
        .iter_ids()
        .into_iter()
        .map(|h| h.to_base32())
        .filter(|b| b.starts_with(s))
        .collect();
    match matches.as_slice() {
        [one] => Ok(one.clone()),
        [] => bail!("cannot resolve changeset {spec:?}"),
        _ => bail!("ambiguous changeset prefix {spec:?}"),
    }
}

fn parse_status(s: &str) -> Option<ReviewStatus> {
    Some(match s {
        "pending" => ReviewStatus::Pending,
        "approved" => ReviewStatus::Approved,
        "changes_requested" => ReviewStatus::ChangesRequested,
        "rejected" => ReviewStatus::Rejected,
        "merged" => ReviewStatus::Merged,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// promotion hook (called from phase1::promote, best-effort)
// ---------------------------------------------------------------------------

/// Open a review (if `[review].auto_create_on_promote`, default true) and
/// emit a `changeset.promoted` event to suggested reviewers.
pub(crate) fn on_promote(repo: &Repo, changeset: &str, author_id: &str, author_is_agent: bool) {
    if let Err(e) = on_promote_inner(repo, changeset, author_id, author_is_agent) {
        tracing::warn!("review/notify hook failed: {e:#}");
    }
}

fn on_promote_inner(
    repo: &Repo,
    changeset: &str,
    author_id: &str,
    author_is_agent: bool,
) -> Result<()> {
    let gpp = repo.gpp_dir();
    let auto = config::load_doc(&repo.config_path())
        .ok()
        .and_then(|d| {
            config::get_key(&d, "review.auto_create_on_promote").and_then(|v| v.as_bool())
        })
        .unwrap_or(true);

    // Graphex code-owners of the touched modules lead the suggestion; RBAC
    // maintainers fill in behind them. Best-effort: a graph/role read failure
    // just yields fewer suggestions, never blocks the promote.
    let owners = graphex_owners_for(repo, changeset).unwrap_or_default();
    let reviewers = RoleStore::open(&gpp)
        .ok()
        .and_then(|rs| ReviewStore::suggest_reviewers_with_owners(&owners, &rs).ok())
        .unwrap_or(owners);

    if auto {
        ReviewStore::open(&gpp)?
            .request(changeset, author_id)
            .map_err(|e| anyhow!("{e}"))?;
    }
    Notifier::open(&gpp)?
        .emit(
            EventType::ChangesetPromoted,
            author_id,
            author_is_agent,
            "changeset",
            changeset,
            &format!("changeset {changeset} promoted"),
            &reviewers,
        )
        .map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

/// People the knowledge graph names as owners (`owned-by` edges) of the
/// modules a changeset touched. Maps changed paths → module roots → graph
/// nodes → their `owned-by` owners. Returns owner node names (which double as
/// reviewer identities), de-duplicated, order-stable.
fn graphex_owners_for(repo: &Repo, changeset: &str) -> Result<Vec<String>> {
    use gpp_graphex::GraphStore;

    let id = Hash::from_base32(changeset.strip_prefix("cs:").unwrap_or(changeset))?;
    let changed = crate::phase3::changed_paths(repo, &id)?;
    let roots = gpp_graphex::module_roots(&changed);
    let gs = GraphStore::open(&repo.gpp_dir())?;

    let mut seen = std::collections::HashSet::new();
    let mut owners = Vec::new();
    for root in roots {
        // A module that isn't in the graph simply has no graph-owner.
        let Ok(node) = gs.node_id_by_name(&root) else {
            continue;
        };
        for (_rel, owner_id) in gs.neighbours(&node, Some("owned-by"))? {
            if let Some(meta) = gs.node_meta(&owner_id)?
                && seen.insert(meta.name.clone())
            {
                owners.push(meta.name);
            }
        }
    }
    Ok(owners)
}

// ---------------------------------------------------------------------------
// gpp review
// ---------------------------------------------------------------------------

pub fn review(args: &ReviewArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let gpp = repo.gpp_dir();
    let store = ReviewStore::open(&gpp)?;
    let me = whoami();

    match &args.action {
        ReviewAction::List { status } => {
            let st = status.as_deref().and_then(parse_status);
            let rows = store.list(st)?;
            if rows.is_empty() {
                println!("(no reviews)");
            }
            for r in rows {
                println!(
                    "{:<18} {:<10} {}  by {}",
                    &r.changeset,
                    r.status.as_str(),
                    &r.id[..12.min(r.id.len())],
                    r.requested_by
                );
            }
            Ok(())
        }
        ReviewAction::Show { changeset } => {
            let changeset = &resolve_changeset(&repo, changeset)?;
            let r = store
                .by_changeset(changeset)?
                .ok_or_else(|| anyhow!("no review for {changeset}"))?;
            println!("review {} for {}", &r.id[..12], r.changeset);
            println!("  status:       {}", r.status.as_str());
            println!("  requested by: {}", r.requested_by);
            let (n, human) = store.approval_summary(changeset)?;
            println!("  approvals:    {n} (human: {human})");
            for c in store.comments(changeset)? {
                let loc = c
                    .file_path
                    .map(|f| format!("{f}:{}", c.line_number.unwrap_or(0)))
                    .unwrap_or_else(|| "(general)".into());
                println!("  • {} [{}] {}", c.author_id, loc, c.body);
            }
            Ok(())
        }
        ReviewAction::Request { changeset } => {
            let changeset = &resolve_changeset(&repo, changeset)?;
            let r = store.request(changeset, &me)?;
            println!("review opened for {} ({})", r.changeset, &r.id[..12]);
            Ok(())
        }
        ReviewAction::Approve { changeset, reason } => {
            let changeset = &resolve_changeset(&repo, changeset)?;
            let st = store.decide(changeset, &me, false, Decision::Approve, reason.as_deref())?;
            emit_review_event(&gpp, EventType::ReviewApproved, &me, changeset);
            println!("approved → review is {}", st.as_str());
            Ok(())
        }
        ReviewAction::RequestChanges { changeset, reason } => {
            let changeset = &resolve_changeset(&repo, changeset)?;
            let st = store.decide(
                changeset,
                &me,
                false,
                Decision::RequestChanges,
                Some(reason),
            )?;
            emit_review_event(&gpp, EventType::ReviewChangesRequested, &me, changeset);
            println!("changes requested → review is {}", st.as_str());
            Ok(())
        }
        ReviewAction::Reject { changeset, reason } => {
            let changeset = &resolve_changeset(&repo, changeset)?;
            let st = store.decide(changeset, &me, false, Decision::Reject, Some(reason))?;
            emit_review_event(&gpp, EventType::ReviewRejected, &me, changeset);
            println!("rejected → review is {}", st.as_str());
            Ok(())
        }
        ReviewAction::Comment {
            changeset,
            body,
            file,
            line,
        } => {
            let changeset = &resolve_changeset(&repo, changeset)?;
            store.comment(changeset, &me, false, file.as_deref(), *line, body)?;
            println!("comment added to {changeset}");
            Ok(())
        }
        ReviewAction::Comments { changeset } => {
            let changeset = &resolve_changeset(&repo, changeset)?;
            for c in store.comments(changeset)? {
                let loc = c
                    .file_path
                    .map(|f| format!("{f}:{}", c.line_number.unwrap_or(0)))
                    .unwrap_or_else(|| "(general)".into());
                println!("{} [{}] {}", c.author_id, loc, c.body);
            }
            Ok(())
        }
        ReviewAction::Merge { changeset } => {
            let changeset = &resolve_changeset(&repo, changeset)?;
            let r = store
                .by_changeset(changeset)?
                .ok_or_else(|| anyhow!("no review for {changeset}"))?;
            if r.status != ReviewStatus::Approved {
                bail!("review is {} — needs approval first", r.status.as_str());
            }
            let (approvals, human) = store.approval_summary(changeset)?;
            let branch = repo.current_branch()?;
            let roles = RoleStore::open(&gpp)?;
            roles
                .can_merge(&gpp_rbac::MergeRequest {
                    branch: branch.clone(),
                    merger_identity: me.clone(),
                    merger_is_agent: false,
                    approvals,
                    has_human_approval: human,
                    policies_passed: true,
                })
                .map_err(|e| anyhow!("merge blocked: {e}"))?;
            store.merge(changeset, &me)?;
            println!("merged review for {changeset} into {branch}");
            Ok(())
        }
    }
}

fn emit_review_event(gpp: &Path, et: EventType, actor: &str, changeset: &str) {
    if let Ok(n) = Notifier::open(gpp) {
        let _ = n.emit(
            et,
            actor,
            false,
            "review",
            changeset,
            &format!("{} on {changeset}", et.as_str()),
            &[],
        );
    }
}

// ---------------------------------------------------------------------------
// gpp rbac
// ---------------------------------------------------------------------------

pub fn rbac(args: &RbacArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let store = RoleStore::open(&repo.gpp_dir())?;
    let me = whoami();

    match &args.action {
        RbacAction::Show => {
            let list = store.list()?;
            if list.is_empty() {
                println!("(no role assignments — everyone defaults to reader)");
            }
            for a in list {
                let exp = a
                    .expires_at
                    .map(|e| format!(" (expires {e}us)"))
                    .unwrap_or_default();
                println!(
                    "{:<28} {:<12} by {}{exp}",
                    a.identity,
                    a.role.as_str(),
                    a.assigned_by
                );
            }
            Ok(())
        }
        RbacAction::Assign {
            identity,
            role,
            reason,
            expires,
        } => {
            let role = Role::parse(role).map_err(|e| anyhow!("{e}"))?;
            let expires_at = expires
                .as_deref()
                .map(crate::phase1::parse_time)
                .transpose()?;
            store
                .assign(identity, role, &me, reason.as_deref(), expires_at)
                .map_err(|e| anyhow!("{e}"))?;
            println!("assigned {} → {}", identity, role.as_str());
            Ok(())
        }
        RbacAction::Revoke { identity } => {
            store.revoke(identity, &me).map_err(|e| anyhow!("{e}"))?;
            println!("revoked role for {identity}");
            Ok(())
        }
        RbacAction::Whoami => {
            let role = store.role_of(&me)?;
            println!("{me}: {}", role.as_str());
            Ok(())
        }
        RbacAction::Protect {
            branch,
            min_reviewers,
            require_human,
            require_role,
            allow_agent_merge,
        } => {
            store
                .protect(&BranchProtection {
                    branch_pattern: branch.clone(),
                    min_reviewers: *min_reviewers,
                    require_human: *require_human,
                    require_role: Role::parse(require_role).map_err(|e| anyhow!("{e}"))?,
                    require_policy_pass: true,
                    allow_agent_merge: *allow_agent_merge,
                })
                .map_err(|e| anyhow!("{e}"))?;
            println!("protection set for {branch:?}");
            Ok(())
        }
        RbacAction::Protections => {
            for p in store.protections()? {
                println!(
                    "{:<16} min_reviewers={} require_human={} require_role={} agent_merge={}",
                    p.branch_pattern,
                    p.min_reviewers,
                    p.require_human,
                    p.require_role.as_str(),
                    p.allow_agent_merge
                );
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// gpp inbox
// ---------------------------------------------------------------------------

pub fn inbox(args: &InboxArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let n = Notifier::open(&repo.gpp_dir())?;
    let me = whoami();

    if let Some(InboxAction::Ack { id, all }) = &args.action {
        if *all {
            let c = n.ack_all(&me)?;
            println!("acknowledged {c} notification(s)");
        } else if let Some(id) = id {
            n.ack(*id)?;
            println!("acknowledged #{id}");
        } else {
            bail!("specify an id or --all");
        }
        return Ok(());
    }

    if args.unread {
        println!("{} unread", n.unread_count(&me)?);
        return Ok(());
    }
    let items = n.inbox(&me, args.limit)?;
    if items.is_empty() {
        println!("(inbox empty)");
    }
    for i in items {
        println!(
            "{} #{:<4} {:<24} {}",
            if i.read { " " } else { "•" },
            i.notif_id,
            i.event_type,
            i.summary
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// gpp notify
// ---------------------------------------------------------------------------

pub fn notify(args: &NotifyArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let n = Notifier::open(&repo.gpp_dir())?;

    match &args.action {
        NotifyAction::Integrations => {
            let list = n.integrations()?;
            if list.is_empty() {
                println!("(no integrations configured)");
            }
            for (b, url, ev) in list {
                println!("{b:<10} {url}  events={ev}");
            }
            Ok(())
        }
        NotifyAction::Add {
            backend,
            url,
            secret,
            events,
        } => {
            let ev: Vec<&str> = events.split(',').map(|s| s.trim()).collect();
            n.add_integration(backend, url, secret.as_deref(), &ev)?;
            println!("added {backend} integration");
            Ok(())
        }
        NotifyAction::Remove { backend } => {
            n.remove_integration(backend)?;
            println!("removed {backend}");
            Ok(())
        }
        NotifyAction::Dispatch => {
            let delivered = n.dispatch(&HttpSender)?;
            println!("dispatched {delivered} delivery(ies)");
            Ok(())
        }
        NotifyAction::Events => {
            for e in gpp_notify::event_catalog() {
                println!("{e}");
            }
            Ok(())
        }
    }
}

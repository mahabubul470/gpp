//! Phase 4 commands: `gpp trust|policy|cost|anomaly|audit`, plus the
//! promotion-time governance hooks (policy enforcement, trust/cost/anomaly
//! recording) called from `phase1::promote`.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use gpp_anomaly::{AnomalyStore, ChangesetFacts};
use gpp_core::{Blob, EntryKind, Hash, ObjectStore, Tree};
use gpp_cost::{CostFilter, CostRecord, CostStore, Usage};
use gpp_history::{AuthorType, Changeset};
use gpp_policy::{ChangesetFacts as PolFacts, PolicySet, Severity};
use gpp_trust::{TrustStatus, TrustStore};

use crate::cli::{
    AnomalyAction, AnomalyArgs, AuditArgs, CostArgs, PolicyAction, PolicyArgs, TrustAction,
    TrustArgs,
};
use crate::phase1::parse_time;
use crate::repo::Repo;

fn now_micros() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

fn discover(repo_override: Option<&Path>) -> Result<Repo> {
    let start = match repo_override {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    Repo::discover(&start)
}

fn policies_dir(repo: &Repo) -> std::path::PathBuf {
    repo.gpp_dir().join("policies")
}

fn flatten(store: &ObjectStore, root: &Hash) -> Result<BTreeMap<String, Hash>> {
    fn walk(
        store: &ObjectStore,
        h: &Hash,
        prefix: &str,
        out: &mut BTreeMap<String, Hash>,
    ) -> Result<()> {
        let tree: Tree = store.read(h)?;
        for e in tree.entries {
            let path = if prefix.is_empty() {
                e.name.clone()
            } else {
                format!("{prefix}/{}", e.name)
            };
            match e.kind {
                EntryKind::Directory => walk(store, &e.hash, &path, out)?,
                EntryKind::File | EntryKind::Symlink => {
                    out.insert(path, e.hash);
                }
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(store, root, "", &mut out)?;
    Ok(out)
}

/// `(files_changed, lines_changed)` for a changeset vs. its first parent.
fn changeset_delta(store: &ObjectStore, cs_id: &Hash) -> Result<(i64, i64)> {
    let cs: Changeset = store.read(cs_id)?;
    let new = flatten(store, &cs.tree)?;
    let old = match cs.parents.first() {
        Some(p) => {
            let pc: Changeset = store.read(p)?;
            flatten(store, &pc.tree)?
        }
        None => BTreeMap::new(),
    };
    let mut files = 0i64;
    let mut lines = 0i64;
    let mut paths: Vec<&String> = old.keys().chain(new.keys()).collect();
    paths.sort();
    paths.dedup();
    for p in paths {
        let (o, n) = (old.get(p), new.get(p));
        if o == n {
            continue;
        }
        files += 1;
        let ob = o
            .map(|h| store.read::<Blob>(h))
            .transpose()?
            .map(|b| b.content);
        let nb = n
            .map(|h| store.read::<Blob>(h))
            .transpose()?
            .map(|b| b.content);
        let st = gpp_diff::stat(ob.as_deref().unwrap_or(&[]), nb.as_deref().unwrap_or(&[]));
        lines += (st.added + st.removed) as i64;
    }
    Ok((files, lines))
}

// ---------------------------------------------------------------------------
// Promotion-time governance hooks (called from phase1::promote)
// ---------------------------------------------------------------------------

/// Enforce content policies over a flattened working snapshot *before* a
/// changeset is created. `Block` violations abort promotion; `warn`/`audit`
/// are reported but allowed. No-op when no policies are installed.
pub(crate) fn enforce_content_policies(
    repo: &Repo,
    snapshot: &BTreeMap<String, Hash>,
    store: &ObjectStore,
) -> Result<()> {
    let set =
        PolicySet::load_dir(&policies_dir(repo)).map_err(|e| anyhow!("loading policies: {e}"))?;
    if set.is_empty() {
        return Ok(());
    }
    let mut blocked = Vec::new();
    for (path, hash) in snapshot {
        let Ok(blob) = store.read::<Blob>(hash) else {
            continue;
        };
        let Ok(text) = String::from_utf8(blob.content) else {
            continue; // binary: skip content rules
        };
        for v in set.check_content(path, &text) {
            match v.severity {
                Severity::Block => {
                    blocked.push(format!(
                        "  [{}/{}] {} @ {}",
                        v.policy, v.rule, v.message, v.location
                    ));
                }
                Severity::Warn => {
                    eprintln!(
                        "policy warning [{}/{}]: {} @ {}",
                        v.policy, v.rule, v.message, v.location
                    );
                }
                Severity::Audit => {
                    tracing::info!(policy=%v.policy, rule=%v.rule, "policy audit: {}", v.message);
                }
            }
        }
    }
    if !blocked.is_empty() {
        bail!(
            "promotion blocked by policy:\n{}\n(fix the violations or adjust the policy)",
            blocked.join("\n")
        );
    }
    Ok(())
}

/// Surface content-policy issues for the working snapshot at `tree` — the
/// `warn` enforcement point (timeline capture). Nothing here aborts: `warn`
/// hits are printed, `block` hits are printed too but flagged as "will block
/// promote/sync", and `audit` hits are logged. Best-effort: any policy-load
/// or read error is logged and swallowed, never surfaced to the user mid-edit.
/// No-op when no policies are installed.
pub(crate) fn warn_content_policies(repo: &Repo, tree: &Hash, store: &ObjectStore) {
    let set = match PolicySet::load_dir(&policies_dir(repo)) {
        Ok(s) if !s.is_empty() => s,
        Ok(_) => return,
        Err(e) => {
            tracing::warn!("loading policies for timeline warn: {e}");
            return;
        }
    };
    let snapshot = match flatten(store, tree) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("policy scan: {e}");
            return;
        }
    };
    for (path, hash) in &snapshot {
        let Ok(blob) = store.read::<Blob>(hash) else {
            continue;
        };
        let Ok(text) = String::from_utf8(blob.content) else {
            continue; // binary: skip content rules
        };
        for v in set.check_content(path, &text) {
            match v.severity {
                Severity::Block => eprintln!(
                    "policy warning [{}/{}]: {} @ {} (will block promote/sync)",
                    v.policy, v.rule, v.message, v.location
                ),
                Severity::Warn => eprintln!(
                    "policy warning [{}/{}]: {} @ {}",
                    v.policy, v.rule, v.message, v.location
                ),
                Severity::Audit => {
                    tracing::info!(policy=%v.policy, rule=%v.rule, "policy audit: {}", v.message)
                }
            }
        }
    }
}

/// Enforce content policies over everything a sync could transmit — the
/// `block` enforcement point — *before* any bytes leave the repo. Scans the
/// flattened snapshot of every local branch tip (the same set `build_push`
/// sends); a single `block` violation aborts the sync. `warn`/`audit` are
/// surfaced but allowed. No-op when no policies are installed.
///
/// This matters even though promotion already blocks: content can reach a tip
/// without passing the *current* policy — installed after the fact, or pulled
/// in from a peer — and would otherwise be re-pushed onward unchecked.
pub(crate) fn enforce_sync_policies(repo: &Repo) -> Result<()> {
    let set =
        PolicySet::load_dir(&policies_dir(repo)).map_err(|e| anyhow!("loading policies: {e}"))?;
    if set.is_empty() {
        return Ok(());
    }
    let store = ObjectStore::open(&repo.gpp_dir());
    let refs = gpp_history::RefStore::open(&repo.gpp_dir());

    // Union of (path -> distinct blob hashes) across every branch tip: the
    // exact content `build_push` would offer a peer. Dedup blobs so identical
    // content reachable from several tips is only scanned (and reported) once.
    let mut snapshot: BTreeMap<String, std::collections::BTreeSet<Hash>> = BTreeMap::new();
    for b in refs.list().map_err(|e| anyhow!("listing branches: {e}"))? {
        let Some(tip) = b.tip else {
            continue;
        };
        let cs: Changeset = store
            .read(&tip)
            .with_context(|| format!("reading changeset {}", tip.to_base32()))?;
        for (path, hash) in flatten(&store, &cs.tree)? {
            snapshot.entry(path).or_default().insert(hash);
        }
    }

    let mut blocked = Vec::new();
    for (path, hashes) in &snapshot {
        for hash in hashes {
            let Ok(blob) = store.read::<Blob>(hash) else {
                continue;
            };
            let Ok(text) = String::from_utf8(blob.content) else {
                continue; // binary: skip content rules
            };
            for v in set.check_content(path, &text) {
                match v.severity {
                    Severity::Block => blocked.push(format!(
                        "  [{}/{}] {} @ {}",
                        v.policy, v.rule, v.message, v.location
                    )),
                    Severity::Warn => eprintln!(
                        "policy warning [{}/{}]: {} @ {}",
                        v.policy, v.rule, v.message, v.location
                    ),
                    Severity::Audit => {
                        tracing::info!(policy=%v.policy, rule=%v.rule, "policy audit: {}", v.message)
                    }
                }
            }
        }
    }
    if !blocked.is_empty() {
        bail!(
            "sync blocked by policy:\n{}\n(fix the violations or adjust the policy)",
            blocked.join("\n")
        );
    }
    Ok(())
}

/// Record trust / cost / anomaly signals *after* a successful promotion.
/// Best-effort: a recording failure is logged, never fatal to the promote.
pub(crate) fn record_promotion(
    repo: &Repo,
    cs_id: &Hash,
    author_type: AuthorType,
    author_id: &str,
    author_name: &str,
) {
    if let Err(e) = record_promotion_inner(repo, cs_id, author_type, author_id, author_name) {
        tracing::warn!("governance recording failed: {e:#}");
    }
}

fn record_promotion_inner(
    repo: &Repo,
    cs_id: &Hash,
    author_type: AuthorType,
    author_id: &str,
    author_name: &str,
) -> Result<()> {
    let gpp = repo.gpp_dir();
    let store = ObjectStore::open(&gpp);
    let (files, lines) = changeset_delta(&store, cs_id)?;

    // Trust: only agents accrue reputation.
    if author_type == AuthorType::Agent {
        let ts = TrustStore::open(&gpp)?;
        ts.record_event(
            author_id,
            author_name,
            None,
            "changeset_promoted",
            Some(&cs_id.to_base32()),
            None,
        )?;
    }

    // Cost: a record with token/cost unknown (0) until an SDK reports it.
    let cost = CostStore::open(&gpp)?;
    cost.record(&CostRecord {
        changeset_id: cs_id.to_base32(),
        agent_id: author_id.to_string(),
        model_id: if author_type == AuthorType::Agent {
            "unknown".into()
        } else {
            "human".into()
        },
        files_touched: files,
        lines_changed: lines,
        timestamp: now_micros(),
        ..Default::default()
    })?;

    // Anomaly: count this author's changesets in the last 24h for burst.
    let anomaly = AnomalyStore::open(&gpp)?;
    let raised = anomaly.detect(&ChangesetFacts {
        agent_id: Some(author_id.to_string()),
        changeset: cs_id.to_base32(),
        files_touched: files,
        lines_changed: lines,
        recent_changesets_in_window: recent_changeset_count(&store, repo, author_id)?,
    })?;
    for r in raised {
        eprintln!("anomaly: {r}");
    }
    Ok(())
}

/// Count changesets by `author_id` reachable from HEAD within the last 24h.
fn recent_changeset_count(store: &ObjectStore, repo: &Repo, author_id: &str) -> Result<i64> {
    let refs = gpp_history::RefStore::open(&repo.gpp_dir());
    let Some(tip) = refs.head_tip()? else {
        return Ok(0);
    };
    let cutoff = now_micros() - 86_400_000_000;
    let mut n = 0;
    for rec in gpp_history::walk(store, Some(tip), 1000)? {
        if rec.changeset.timestamp < cutoff {
            break;
        }
        if rec.changeset.author.identity == author_id {
            n += 1;
        }
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// gpp trust
// ---------------------------------------------------------------------------

fn parse_future(spec: &str) -> Option<i64> {
    if spec == "permanent" {
        return None;
    }
    let (num, unit) = spec.split_at(spec.find(|c: char| !c.is_ascii_digit())?);
    let n: i64 = num.parse().ok()?;
    let us = match unit {
        "s" => 1_000_000,
        "m" => 60_000_000,
        "h" => 3_600_000_000,
        "d" => 86_400_000_000,
        _ => return None,
    };
    Some(now_micros() + n * us)
}

pub fn trust(args: &TrustArgs, repo_override: Option<&Path>, json: bool) -> Result<()> {
    let repo = discover(repo_override)?;
    let ts = TrustStore::open(&repo.gpp_dir())?;
    match &args.action {
        TrustAction::Show { agent } => {
            let agents = match agent {
                Some(a) => ts.score(a)?.into_iter().collect(),
                None => ts.list()?,
            };
            if agents.is_empty() {
                println!("(no agents tracked yet)");
            }
            if json {
                let arr: Vec<_> = agents
                    .iter()
                    .map(|a| {
                        serde_json::json!({
                            "agent_id": a.agent_id, "score": a.trust_score,
                            "status": a.effective_status(now_micros()).as_str(),
                            "changesets": a.total_changesets,
                            "survived": a.survived_review, "regressions": a.regressions,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&serde_json::json!(arr))?);
            } else {
                for a in &agents {
                    println!(
                        "{:<24} {:>6.1}  {:<16} cs={} survived={} regr={}",
                        a.agent_id,
                        a.trust_score,
                        a.effective_status(now_micros()).as_str(),
                        a.total_changesets,
                        a.survived_review,
                        a.regressions
                    );
                }
            }
            Ok(())
        }
        TrustAction::History { agent, since } => {
            let since = since.as_deref().map(parse_time).transpose()?;
            let rows = ts.history(agent, since, 100)?;
            if rows.is_empty() {
                println!("(no events)");
            }
            for (t, ev, cs, det) in rows {
                println!(
                    "{t}us  {ev:<22} {} {}",
                    cs.unwrap_or_default(),
                    det.unwrap_or_default()
                );
            }
            Ok(())
        }
        TrustAction::Policy => {
            let p = gpp_trust::TrustPolicy::default();
            println!("auto_merge_min      = {}", p.auto_merge_min);
            println!("review_required_min = {}", p.review_required_min);
            println!("sandbox_min         = {}", p.sandbox_min);
            println!("(thresholds mirror [trust] in .gpp/config.toml)");
            Ok(())
        }
        TrustAction::Override {
            agent,
            status,
            reason,
            duration,
        } => {
            let st = TrustStatus::parse(status).map_err(|e| anyhow!("{e}"))?;
            let until = duration.as_deref().and_then(parse_future);
            ts.override_status(agent, st, reason, until)
                .map_err(|e| anyhow!("{e}"))?;
            println!(
                "Override set: {agent} → {} ({})",
                st.as_str(),
                duration.as_deref().unwrap_or("permanent")
            );
            Ok(())
        }
        TrustAction::Reset { agent } => {
            ts.reset(agent).map_err(|e| anyhow!("{e}"))?;
            println!("Reset trust for {agent}");
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// gpp policy
// ---------------------------------------------------------------------------

pub fn policy(args: &PolicyArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let dir = policies_dir(&repo);
    std::fs::create_dir_all(&dir).ok();

    match &args.action {
        PolicyAction::List => {
            let set = PolicySet::load_dir(&dir).map_err(|e| anyhow!("{e}"))?;
            if set.is_empty() {
                println!("(no policies installed — try `gpp policy templates`)");
            }
            for p in &set.policies {
                println!("{:<16} {}", p.name, p.description);
            }
            Ok(())
        }
        PolicyAction::Show { name } => {
            let set = PolicySet::load_dir(&dir).map_err(|e| anyhow!("{e}"))?;
            let p = set
                .policies
                .iter()
                .find(|p| p.name == *name)
                .ok_or_else(|| anyhow!("no policy named {name:?}"))?;
            println!("{} — {}", p.name, p.description);
            // Re-read raw file for rule listing.
            let raw =
                std::fs::read_to_string(dir.join(format!("{name}.policy"))).unwrap_or_default();
            println!("{raw}");
            Ok(())
        }
        PolicyAction::Add { file } => {
            let text = std::fs::read_to_string(file)
                .with_context(|| format!("reading {}", file.display()))?;
            let p = gpp_policy::Policy::parse(&file.display().to_string(), &text)
                .map_err(|e| anyhow!("{e}"))?;
            let dest = dir.join(format!("{}.policy", p.name));
            std::fs::write(&dest, &text)?;
            println!("Installed policy {:?} → {}", p.name, dest.display());
            Ok(())
        }
        PolicyAction::Template { name } => {
            let t = gpp_policy::template(name)
                .ok_or_else(|| anyhow!("no built-in template {name:?}"))?;
            let dest = dir.join(format!("{name}.policy"));
            std::fs::write(&dest, t)?;
            println!("Installed template {name:?} → {}", dest.display());
            Ok(())
        }
        PolicyAction::Templates => {
            for (n, _) in gpp_policy::TEMPLATES {
                println!("{n}");
            }
            Ok(())
        }
        PolicyAction::Remove { name } => {
            let p = dir.join(format!("{name}.policy"));
            if !p.exists() {
                bail!("no policy named {name:?}");
            }
            std::fs::remove_file(p)?;
            println!("Removed policy {name:?}");
            Ok(())
        }
        PolicyAction::Validate { file } => {
            let text = std::fs::read_to_string(file)?;
            match gpp_policy::Policy::parse(&file.display().to_string(), &text) {
                Ok(p) => {
                    println!("OK: policy {:?} parsed", p.name);
                    Ok(())
                }
                Err(e) => bail!("invalid: {e}"),
            }
        }
        PolicyAction::Check { changeset } => {
            let set = PolicySet::load_dir(&dir).map_err(|e| anyhow!("{e}"))?;
            if set.is_empty() {
                println!("(no policies installed)");
                return Ok(());
            }
            let store = ObjectStore::open(&repo.gpp_dir());
            let files = collect_files(&repo, &store, changeset.as_deref())?;
            let mut total = 0;
            for (path, content) in &files {
                if let Ok(text) = std::str::from_utf8(content) {
                    for v in set.check_content(path, text) {
                        total += 1;
                        println!(
                            "{:<6} [{}/{}] {} @ {}",
                            v.severity.as_str(),
                            v.policy,
                            v.rule,
                            v.message,
                            v.location
                        );
                    }
                }
            }
            // Changeset-level rules.
            if let Some(spec) = changeset {
                let id = resolve_cs(&repo, spec)?;
                let cs: Changeset = store.read(&id)?;
                let names = flatten(&store, &cs.tree)?
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>();
                let facts = PolFacts {
                    author_is_agent: cs.author.author_type == AuthorType::Agent,
                    files: names,
                    has_human_review: false,
                };
                for v in set.check_changeset(&facts) {
                    total += 1;
                    println!(
                        "{:<6} [{}/{}] {}",
                        v.severity.as_str(),
                        v.policy,
                        v.rule,
                        v.message
                    );
                }
            }
            if total == 0 {
                println!("✓ no policy violations");
            } else {
                println!("\n{total} violation(s)");
            }
            Ok(())
        }
    }
}

fn resolve_cs(repo: &Repo, spec: &str) -> Result<Hash> {
    let refs = gpp_history::RefStore::open(&repo.gpp_dir());
    let s = spec.strip_prefix("cs:").unwrap_or(spec);
    if s.eq_ignore_ascii_case("HEAD") {
        return refs
            .head_tip()?
            .ok_or_else(|| anyhow!("HEAD has no changesets"));
    }
    if let Ok(h) = Hash::from_base32(s) {
        return Ok(h);
    }
    refs.read_ref(s)?
        .ok_or_else(|| anyhow!("cannot resolve {spec:?}"))
}

fn collect_files(
    repo: &Repo,
    store: &ObjectStore,
    changeset: Option<&str>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let tree = match changeset {
        Some(spec) => {
            let cs: Changeset = store.read(&resolve_cs(repo, spec)?)?;
            cs.tree
        }
        None => {
            let tl = gpp_timeline::Timeline::open(&repo.root, Vec::<String>::new())
                .map_err(|e| anyhow!("timeline: {e}"))?;
            tl.snapshot_tree().map_err(|e| anyhow!("snapshot: {e}"))?
        }
    };
    let mut out = BTreeMap::new();
    for (p, h) in flatten(store, &tree)? {
        if let Ok(b) = store.read::<Blob>(&h) {
            out.insert(p, b.content);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// gpp cost
// ---------------------------------------------------------------------------

pub fn cost(args: &CostArgs, repo_override: Option<&Path>, json: bool) -> Result<()> {
    let repo = discover(repo_override)?;
    let cs = CostStore::open(&repo.gpp_dir())?;

    if let Some(spec) = &args.report {
        // Map HEAD / branch / short prefix to the canonical id the
        // promote-time record is keyed under, so the report updates that
        // record instead of creating a stray one.
        let changeset = crate::phase6::resolve_changeset(&repo, spec)?;
        let usage = Usage {
            input_tokens: args.input,
            output_tokens: args.output,
            cached_tokens: args.cached,
            cost_microdollars: args.cost_micro,
            duration_ms: args.duration_ms,
        };
        cs.add_usage(&changeset, "", &args.model, &usage)?;
        let rec = cs.get(&changeset)?;
        if json {
            let total = rec.as_ref().map(|r| r.cost_microdollars).unwrap_or(0);
            println!(
                "{}",
                serde_json::json!({
                    "changeset": changeset,
                    "model": args.model,
                    "added": {
                        "input_tokens": usage.input_tokens,
                        "output_tokens": usage.output_tokens,
                        "cached_tokens": usage.cached_tokens,
                        "cost_usd": usage.cost_microdollars as f64 / 1e6,
                    },
                    "total_cost_usd": total as f64 / 1e6,
                })
            );
        } else {
            let total = rec.as_ref().map(|r| r.cost_microdollars).unwrap_or(0);
            println!(
                "recorded usage for {changeset}: +{} in / +{} out tokens, +${:.4} (total ${:.4})",
                usage.input_tokens,
                usage.output_tokens,
                usage.cost_microdollars as f64 / 1e6,
                total as f64 / 1e6,
            );
        }
        return Ok(());
    }

    if let Some(dollars) = args.budget_alert {
        cs.set_budget(&args.module, (dollars * 1_000_000.0) as i64, 0.8)?;
        println!(
            "Weekly budget for {:?} set to ${:.2} (alert at 80%)",
            args.module, dollars
        );
        return Ok(());
    }
    if args.budget {
        let st = cs.budget_status()?;
        if st.is_empty() {
            println!("(no budgets configured — set one with --budget-alert)");
        }
        for b in st {
            println!(
                "{:<16} spent ${:.2} / ${:.2} this week{}",
                b.module_pattern,
                b.spent_this_week as f64 / 1e6,
                b.weekly_limit as f64 / 1e6,
                if b.alerting { "  ⚠ ALERT" } else { "" }
            );
        }
        return Ok(());
    }

    let filter = CostFilter {
        since: args.since.as_deref().map(parse_time).transpose()?,
        until: args.until.as_deref().map(parse_time).transpose()?,
        agent: args.agent.clone(),
    };

    if args.breakdown {
        for (agent, model, micro, n) in cs.breakdown(&filter)? {
            println!(
                "{agent:<24} {model:<18} ${:.4}  ({n} cs)",
                micro as f64 / 1e6
            );
        }
        return Ok(());
    }

    let s = cs.summarize(&filter)?;
    if json {
        println!(
            "{}",
            serde_json::json!({
                "changesets": s.changesets,
                "input_tokens": s.input_tokens,
                "output_tokens": s.output_tokens,
                "cost_usd": s.cost_microdollars as f64 / 1e6,
                "lines_changed": s.lines_changed,
                "lines_survived": s.lines_survived,
                "cost_per_survived_line_usd":
                    s.cost_per_survived_line().map(|v| v / 1e6),
            })
        );
        return Ok(());
    }
    println!("changesets:     {}", s.changesets);
    println!(
        "tokens:         in {} / out {} / cached {}",
        s.input_tokens, s.output_tokens, s.cached_tokens
    );
    println!("cost:           ${:.4}", s.cost_microdollars as f64 / 1e6);
    println!(
        "lines:          {} changed / {} survived",
        s.lines_changed, s.lines_survived
    );
    if args.efficiency {
        match s.cost_per_survived_line() {
            Some(v) => println!("efficiency:     ${:.6} per survived line", v / 1e6),
            None => println!("efficiency:     n/a (no reviewed/survived lines yet)"),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// gpp anomaly
// ---------------------------------------------------------------------------

pub fn anomaly(args: &AnomalyArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let an = AnomalyStore::open(&repo.gpp_dir())?;
    let since = args.since.as_deref().map(parse_time).transpose()?;

    match &args.action {
        None => {
            let rows = an.list(true, args.agent.as_deref(), since, 100)?;
            if rows.is_empty() {
                println!("✓ no unresolved anomalies");
            }
            for e in rows {
                println!(
                    "#{:<4} {:<8} {:<18} {}",
                    e.id, e.severity, e.rule_id, e.description
                );
            }
            Ok(())
        }
        Some(AnomalyAction::History) => {
            for e in an.list(false, args.agent.as_deref(), since, 200)? {
                println!(
                    "#{:<4} {:<8} {:<18} {} {}",
                    e.id,
                    e.severity,
                    e.rule_id,
                    if e.resolved { "[resolved]" } else { "[open]" },
                    e.description
                );
            }
            Ok(())
        }
        Some(AnomalyAction::Resolve { id, reason }) => {
            an.resolve(*id, "cli-user", reason)
                .map_err(|e| anyhow!("{e}"))?;
            println!("Resolved anomaly #{id}");
            Ok(())
        }
        Some(AnomalyAction::Rules) => {
            for r in an.rules()? {
                println!(
                    "{:<18} threshold={:<6} severity={:<8} {}",
                    r.rule_id,
                    r.threshold,
                    r.severity.as_str(),
                    if r.enabled { "enabled" } else { "disabled" }
                );
            }
            Ok(())
        }
        Some(AnomalyAction::Configure {
            rule,
            threshold,
            enabled,
        }) => {
            an.configure(rule, *threshold, *enabled)
                .map_err(|e| anyhow!("{e}"))?;
            println!("Updated rule {rule:?}");
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// gpp audit
// ---------------------------------------------------------------------------

pub fn audit(args: &AuditArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let gpp = repo.gpp_dir();
    let since = args.since.as_deref().map(parse_time).transpose()?;

    println!("# gpp audit report");
    println!("repository: {}", repo.root.display());

    if let Ok(ts) = TrustStore::open(&gpp) {
        let agents = ts.list().unwrap_or_default();
        println!("\n## Agent trust ({} agents)", agents.len());
        for a in agents {
            println!(
                "- {} score {:.1} status {}",
                a.agent_id,
                a.trust_score,
                a.effective_status(now_micros()).as_str()
            );
        }
    }

    if let Ok(an) = AnomalyStore::open(&gpp) {
        let open = an.list(true, None, since, 100).unwrap_or_default();
        println!("\n## Unresolved anomalies ({})", open.len());
        for e in open {
            println!("- #{} [{}] {}", e.id, e.severity, e.description);
        }
    }

    if args.include_cost
        && let Ok(cs) = CostStore::open(&gpp)
    {
        let s = cs.summarize(&CostFilter {
            since,
            ..Default::default()
        })?;
        println!("\n## Cost");
        println!(
            "- {} changesets, ${:.4}, {} lines changed",
            s.changesets,
            s.cost_microdollars as f64 / 1e6,
            s.lines_changed
        );
    }

    if args.include_graphex {
        match gpp_graphex::GraphStore::open(&gpp) {
            Ok(g) => {
                let rows = g.read_audit(since, None, 50).unwrap_or_default();
                println!("\n## Graphex access ({} entries)", rows.len());
                for (t, at, aid, action, _) in rows {
                    println!("- {t}us {at} {aid} {action}");
                }
            }
            Err(_) => println!("\n## Graphex access\n- (graphex not initialized)"),
        }
    }
    Ok(())
}

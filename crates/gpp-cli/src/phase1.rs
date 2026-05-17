//! Phase 1 commands: timeline, promote, log, diff, branch.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use gpp_core::{Blob, EntryKind, Hash, ObjectStore, Tree};
use gpp_history::{Author, AuthorType, IntentType, PromoteOptions, RefStore, walk};
use gpp_timeline::{AuthorKind, EntryFilter, Source, Timeline, now_micros};

use crate::cli::{
    BranchAction, BranchArgs, DiffArgs, LogArgs, PromoteArgs, TimelineAction, TimelineArgs,
};
use crate::config;
use crate::repo::Repo;

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

fn discover(repo_override: Option<&Path>) -> Result<Repo> {
    let start = match repo_override {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    Repo::discover(&start)
}

/// Merged gitignore-style patterns: `[timeline].ignore` + `.gppignore`.
fn ignore_patterns(repo: &Repo) -> Vec<String> {
    let mut pats = Vec::new();
    if let Ok(doc) = config::load_doc(&repo.config_path())
        && let Some(arr) = config::get_key(&doc, "timeline.ignore").and_then(|v| v.as_array())
    {
        pats.extend(arr.iter().filter_map(|v| v.as_str().map(str::to_string)));
    }
    if let Ok(txt) = std::fs::read_to_string(repo.root.join(".gppignore")) {
        pats.extend(txt.lines().map(str::to_string));
    }
    pats
}

/// Author from the global config `[user]` table.
fn config_author() -> Author {
    let name;
    let email;
    if let Ok(path) = config::global_config_path() {
        let doc = config::load_doc(&path).unwrap_or(toml::Value::Table(Default::default()));
        name = config::get_key(&doc, "user.name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();
        email = config::get_key(&doc, "user.email")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown@localhost")
            .to_string();
    } else {
        name = "Unknown".into();
        email = "unknown@localhost".into();
    }
    Author::human(name, email)
}

fn open_timeline(repo: &Repo) -> Result<Timeline> {
    Timeline::open(&repo.root, ignore_patterns(repo)).context("failed to open timeline")
}

fn refstore(repo: &Repo) -> RefStore {
    RefStore::open(&repo.gpp_dir())
}

/// Days since the Unix epoch for a proleptic-Gregorian Y-M-D (Howard Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Parse a time spec to Unix microseconds. Supports `Nd/Nh/Nm/Ns` (relative),
/// `today`, `YYYY-MM-DD`, and a bare integer (Unix seconds).
pub(crate) fn parse_time(spec: &str) -> Result<i64> {
    let s = spec.trim();
    let now = now_micros();
    if s == "today" {
        let day = now / 86_400_000_000;
        return Ok(day * 86_400_000_000);
    }
    if let Some(num) = s.strip_suffix(|c| matches!(c, 's' | 'm' | 'h' | 'd'))
        && let Ok(n) = num.parse::<i64>()
    {
        let unit = s.chars().last().unwrap();
        let us = match unit {
            's' => 1_000_000,
            'm' => 60_000_000,
            'h' => 3_600_000_000,
            'd' => 86_400_000_000,
            _ => unreachable!(),
        };
        return Ok(now - n * us);
    }
    if let Some((y, rest)) = s.split_once('-')
        && let Some((m, d)) = rest.split_once('-')
        && let (Ok(y), Ok(m), Ok(d)) = (y.parse::<i64>(), m.parse::<i64>(), d.parse::<i64>())
    {
        return Ok(days_from_civil(y, m, d) * 86_400_000_000);
    }
    if let Ok(secs) = s.parse::<i64>() {
        return Ok(secs * 1_000_000);
    }
    bail!("could not parse time {spec:?} (try \"1h\", \"2d\", \"today\", or \"YYYY-MM-DD\")")
}

fn parse_duration_us(spec: &str) -> Result<i64> {
    let s = spec.trim();
    let (num, unit) = s.split_at(
        s.find(|c: char| !c.is_ascii_digit())
            .ok_or_else(|| anyhow!("duration {spec:?} needs a unit (s/m/h/d)"))?,
    );
    let n: i64 = num
        .parse()
        .with_context(|| format!("bad duration {spec:?}"))?;
    let us = match unit {
        "s" => 1_000_000,
        "m" => 60_000_000,
        "h" => 3_600_000_000,
        "d" => 86_400_000_000,
        other => bail!("unknown duration unit {other:?} (use s/m/h/d)"),
    };
    Ok(n * us)
}

fn short(h: &Hash) -> String {
    h.short()
}

fn fmt_age(ts_us: i64) -> String {
    let secs = (now_micros() - ts_us).max(0) / 1_000_000;
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

/// Recursively flatten a stored [`Tree`] into `path -> blob hash`.
fn flatten_tree(store: &ObjectStore, root: &Hash) -> Result<BTreeMap<String, Hash>> {
    fn walk(
        store: &ObjectStore,
        tree_hash: &Hash,
        prefix: &str,
        out: &mut BTreeMap<String, Hash>,
    ) -> Result<()> {
        let tree: Tree = store.read(tree_hash)?;
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

/// Resolve `HEAD` / a branch name / `cs:<hash>` / a bare base32 hash.
fn resolve_commitish(repo: &Repo, refs: &RefStore, s: &str) -> Result<Hash> {
    let s = s.strip_prefix("cs:").unwrap_or(s);
    if s.eq_ignore_ascii_case("HEAD") {
        return refs
            .head_tip()?
            .ok_or_else(|| anyhow!("HEAD has no changesets yet"));
    }
    if s.len() == gpp_core::HASH_STR_LEN
        && let Ok(h) = Hash::from_base32(s)
    {
        return Ok(h);
    }
    if let Some(tip) = refs.read_ref(s)? {
        return Ok(tip);
    }
    let _ = repo;
    bail!("could not resolve {s:?} to a changeset (use HEAD, a branch, or a full hash)")
}

/// Repo state for `gpp status`: (timeline entries, unpromoted, HEAD tip short).
pub fn summarize(repo: &Repo) -> Result<(usize, usize, Option<String>)> {
    let tl = open_timeline(repo)?;
    let entries = tl
        .entries(&EntryFilter {
            limit: Some(u32::MAX),
            ..Default::default()
        })?
        .len();
    let unpromoted = tl.unpromoted_in_range(None, None)?.len();
    let tip = refstore(repo).head_tip()?.map(|h| h.short());
    Ok((entries, unpromoted, tip))
}

// ---------------------------------------------------------------------------
// gpp timeline
// ---------------------------------------------------------------------------

pub fn timeline(args: &TimelineArgs, repo_override: Option<&Path>, json: bool) -> Result<()> {
    let repo = discover(repo_override)?;
    let mut tl = open_timeline(&repo)?;

    match &args.action {
        Some(TimelineAction::Watch) => {
            let author = config_author();
            eprintln!("watching {} … (Ctrl-C to stop)", repo.root.display());
            tl.watch(
                AuthorKind::Human,
                &author.identity,
                Duration::from_millis(gpp_timeline::DEFAULT_DEBOUNCE_MS),
                |id| println!("captured timeline entry #{id}"),
            )?;
            Ok(())
        }
        Some(TimelineAction::Prune { older_than }) => {
            let retention_us = match older_than {
                Some(spec) => parse_duration_us(spec)?,
                None => {
                    let doc = config::load_doc(&repo.config_path())?;
                    let days = config::get_key(&doc, "timeline.retention_days")
                        .and_then(|v| v.as_integer())
                        .unwrap_or(30);
                    days * 86_400_000_000
                }
            };
            let removed = tl.prune(now_micros() - retention_us)?;
            println!(
                "pruned {removed} timeline entr{}",
                if removed == 1 { "y" } else { "ies" }
            );
            Ok(())
        }
        Some(TimelineAction::Export { path }) => {
            // Capture first so an export reflects the latest state.
            tl.capture(AuthorKind::Human, &config_author().identity, Source::Cli)?;
            let entries = tl.entries(&build_filter(args)?)?;
            let arr: Vec<serde_json::Value> = entries.iter().map(entry_json).collect();
            let body = serde_json::to_string_pretty(&serde_json::json!(arr))?;
            match path {
                Some(p) => {
                    std::fs::write(p, body)?;
                    println!("exported {} entries to {}", entries.len(), p.display());
                }
                None => println!("{body}"),
            }
            Ok(())
        }
        // default listing and `search` share the same filtered view
        None | Some(TimelineAction::Search) => {
            tl.capture(AuthorKind::Human, &config_author().identity, Source::Cli)?;
            let entries = tl.entries(&build_filter(args)?)?;
            if json {
                let arr: Vec<serde_json::Value> = entries.iter().map(entry_json).collect();
                println!("{}", serde_json::to_string_pretty(&serde_json::json!(arr))?);
                return Ok(());
            }
            if entries.is_empty() {
                println!("(no timeline entries)");
                return Ok(());
            }
            for e in &entries {
                let promoted = if e.promoted_to.is_some() {
                    " [promoted]"
                } else {
                    ""
                };
                println!(
                    "#{:<5} {:<9} {}:{:<22} {}{}",
                    e.id,
                    fmt_age(e.timestamp),
                    e.author_kind.as_str(),
                    e.author_id,
                    e.summary.clone().unwrap_or_default(),
                    promoted
                );
                if args.stat {
                    for f in &e.files {
                        println!("        {:<7} {}", f.change.as_str(), f.path);
                    }
                }
            }
            Ok(())
        }
    }
}

fn build_filter(args: &TimelineArgs) -> Result<EntryFilter> {
    Ok(EntryFilter {
        since: args.since.as_deref().map(parse_time).transpose()?,
        until: args.until.as_deref().map(parse_time).transpose()?,
        author: args.author.clone(),
        file_glob: match &args.file {
            Some(p) => Some(
                globset::Glob::new(p)
                    .with_context(|| format!("bad --file glob {p:?}"))?
                    .compile_matcher(),
            ),
            None => None,
        },
        limit: Some(args.limit),
    })
}

fn entry_json(e: &gpp_timeline::EntryView) -> serde_json::Value {
    serde_json::json!({
        "id": e.id,
        "timestamp": e.timestamp,
        "author_type": e.author_kind.as_str(),
        "author_id": e.author_id,
        "source": e.source,
        "summary": e.summary,
        "promoted_to": e.promoted_to,
        "files": e.files.iter().map(|f| serde_json::json!({
            "path": f.path,
            "change": f.change.as_str(),
        })).collect::<Vec<_>>(),
    })
}

// ---------------------------------------------------------------------------
// gpp promote
// ---------------------------------------------------------------------------

pub fn promote(args: &PromoteArgs, repo_override: Option<&Path>) -> Result<()> {
    if args.interactive {
        bail!("--interactive is not implemented in Phase 1");
    }
    if args.auto_summarize {
        bail!("--auto-summarize needs the AI layer (later phase); pass -m instead");
    }
    if args.sign {
        bail!("--sign needs the signing/key layer (later phase)");
    }
    let message = args
        .message
        .clone()
        .ok_or_else(|| anyhow!("a changeset message is required: pass -m <msg>"))?;

    let repo = discover(repo_override)?;
    let mut tl = open_timeline(&repo)?;
    let refs = refstore(&repo);

    let resolve_bound = |spec: &str, upper: bool| -> Result<i64> {
        if let Ok(id) = spec.parse::<i64>() {
            return Ok(id);
        }
        // time spec → nearest entry id bound
        let t = parse_time(spec)?;
        let all = tl.entries(&EntryFilter {
            limit: Some(u32::MAX),
            ..Default::default()
        })?;
        let mut chosen = if upper { i64::MIN } else { i64::MAX };
        for e in &all {
            if upper && e.timestamp <= t {
                chosen = chosen.max(e.id);
            } else if !upper && e.timestamp >= t {
                chosen = chosen.min(e.id);
            }
        }
        Ok(chosen)
    };
    let from = args
        .from
        .as_deref()
        .map(|s| resolve_bound(s, false))
        .transpose()?;
    let to = args
        .to
        .as_deref()
        .map(|s| resolve_bound(s, true))
        .transpose()?;

    let intent_type = args
        .intent
        .as_deref()
        .map(IntentType::parse)
        .unwrap_or(IntentType::HumanDirected);

    // Phase 4: enforce content policies on the working snapshot. `Block`
    // severity aborts here — before any changeset object is created.
    {
        let store = ObjectStore::open(&repo.gpp_dir());
        let snap = tl.snapshot_tree()?;
        let snapshot = flatten_tree(&store, &snap)?;
        crate::phase4::enforce_content_policies(&repo, &snapshot, &store)?;
    }

    let author = config_author();
    let outcome = gpp_history::promote(
        &mut tl,
        &refs,
        PromoteOptions {
            from,
            to,
            message,
            intent_type,
            task: args.task.clone(),
            author: author.clone(),
        },
    )?;

    // Phase 4: record trust / cost / anomaly signals (best-effort).
    crate::phase4::record_promotion(
        &repo,
        &outcome.changeset,
        author.author_type,
        &author.identity,
        &author.name,
    );

    // Phase 6: open a review + emit a promotion event (best-effort).
    crate::phase6::on_promote(
        &repo,
        &outcome.changeset.to_base32(),
        &author.identity,
        author.author_type == gpp_history::AuthorType::Agent,
    );

    println!(
        "Promoted {} timeline entr{} → cs:{} on {}",
        outcome.entries_promoted,
        if outcome.entries_promoted == 1 {
            "y"
        } else {
            "ies"
        },
        short(&outcome.changeset),
        outcome.branch
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// gpp log
// ---------------------------------------------------------------------------

pub fn log(args: &LogArgs, repo_override: Option<&Path>) -> Result<()> {
    if args.semantic {
        eprintln!("note: --semantic has no data in Phase 1 (semantic diff is Phase 2)");
    }
    let repo = discover(repo_override)?;
    let refs = refstore(&repo);
    let store = ObjectStore::open(&repo.gpp_dir());

    let Some(tip) = refs.head_tip()? else {
        println!("(no changesets yet — use `gpp promote -m \"…\"`)");
        return Ok(());
    };

    let want_intent = args.intent.as_deref().map(IntentType::parse);
    let since = args.since.as_deref().map(parse_time).transpose()?;
    let until = args.until.as_deref().map(parse_time).transpose()?;

    // Over-fetch then filter so -n counts post-filter results.
    let records = walk(&store, Some(tip), 100_000)?;
    let mut shown = 0usize;
    for r in &records {
        let a = &r.changeset.author;
        if args.agent && a.author_type != AuthorType::Agent {
            continue;
        }
        if args.human && a.author_type != AuthorType::Human {
            continue;
        }
        if let Some(want) = &args.author
            && &a.identity != want
        {
            continue;
        }
        if let (Some(w), Some(i)) = (want_intent, r.intent.as_ref())
            && i.intent_type != w
        {
            continue;
        }
        if let Some(s) = since
            && r.changeset.timestamp < s
        {
            continue;
        }
        if let Some(u) = until
            && r.changeset.timestamp > u
        {
            continue;
        }
        if shown >= args.limit {
            break;
        }
        shown += 1;

        let g = if args.graph { "* " } else { "" };
        if args.oneline {
            println!("{g}cs:{}  {}", short(&r.id), r.message());
        } else {
            println!("{g}changeset cs:{}", r.id);
            println!("  Author:  {} <{}>", a.name, a.identity);
            println!("  Date:    {}", fmt_age(r.changeset.timestamp));
            if let Some(i) = &r.intent {
                println!("  Intent:  {:?}", i.intent_type);
                if let Some(t) = &i.task_reference {
                    println!("  Task:    {t}");
                }
            }
            println!("\n    {}\n", r.message());
        }
    }
    if shown == 0 {
        println!("(no changesets matched the given filters)");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// gpp diff
// ---------------------------------------------------------------------------

pub fn diff(args: &DiffArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let refs = refstore(&repo);
    let store = ObjectStore::open(&repo.gpp_dir());

    let (old, new): (BTreeMap<String, Hash>, BTreeMap<String, Hash>) = match args.target.as_deref()
    {
        None => {
            // working tree vs HEAD
            let tl = open_timeline(&repo)?;
            let new_tree = tl.snapshot_tree()?;
            let old = match refs.head_tip()? {
                Some(tip) => {
                    let cs: gpp_history::Changeset = store.read(&tip)?;
                    flatten_tree(&store, &cs.tree)?
                }
                None => BTreeMap::new(),
            };
            (old, flatten_tree(&store, &new_tree)?)
        }
        Some(spec) if spec.contains("..") => {
            let (a, b) = spec.split_once("..").unwrap();
            let ca: gpp_history::Changeset = store.read(&resolve_commitish(&repo, &refs, a)?)?;
            let cb: gpp_history::Changeset = store.read(&resolve_commitish(&repo, &refs, b)?)?;
            (
                flatten_tree(&store, &ca.tree)?,
                flatten_tree(&store, &cb.tree)?,
            )
        }
        Some(spec) => {
            let id = resolve_commitish(&repo, &refs, spec)?;
            let cs: gpp_history::Changeset = store.read(&id)?;
            let old = match cs.parents.first() {
                Some(p) => {
                    let pc: gpp_history::Changeset = store.read(p)?;
                    flatten_tree(&store, &pc.tree)?
                }
                None => BTreeMap::new(),
            };
            (old, flatten_tree(&store, &cs.tree)?)
        }
    };

    let mut paths: Vec<&String> = old.keys().chain(new.keys()).collect();
    paths.sort();
    paths.dedup();

    // Semantic is the default; --line forces line-based; --stat/--files stay
    // line-based (they are inherently line counts / name lists).
    let semantic_mode = !args.line && !args.stat && !args.files;

    let mut total = gpp_diff::FileStat::default();
    let mut changed_files = 0usize;
    let mut line_blocks: Vec<String> = Vec::new();
    let mut sem_diffs: Vec<gpp_diff::FileSemanticDiff> = Vec::new();
    let mut any = false;

    for path in paths {
        let o = old.get(path);
        let n = new.get(path);
        if o == n {
            continue;
        }
        any = true;
        changed_files += 1;
        let ob = match o {
            Some(h) => store.read::<Blob>(h)?.content,
            None => Vec::new(),
        };
        let nb = match n {
            Some(h) => store.read::<Blob>(h)?.content,
            None => Vec::new(),
        };
        let st = gpp_diff::stat(&ob, &nb);
        total.added += st.added;
        total.removed += st.removed;

        if args.files {
            println!("{path}");
            continue;
        }
        if args.stat {
            println!("  {path:<40} +{:<5} -{}", st.added, st.removed);
            continue;
        }

        let semantic_capable = semantic_mode && gpp_diff::detect_language(path).is_some();
        if semantic_capable {
            match gpp_diff::semantic(path, &ob, &nb) {
                Ok(d) => {
                    sem_diffs.push(d);
                    continue;
                }
                // Parse failure (e.g. syntax error mid-edit): fall back.
                Err(_) => {
                    if args.semantic {
                        eprintln!("note: {path}: semantic parse failed, using line diff");
                    }
                }
            }
        }
        if let Some(d) = gpp_diff::unified(path, &ob, &nb) {
            line_blocks.push(d);
        }
    }

    if args.files {
        if !any {
            println!("(no changes)");
        }
        return Ok(());
    }
    if args.stat {
        if !any {
            println!("(no changes)");
        } else {
            println!(
                "  {changed_files} file(s) changed, +{} -{}",
                total.added, total.removed
            );
        }
        return Ok(());
    }

    for b in &line_blocks {
        print!("{b}");
    }
    let moves = gpp_diff::detect_moves(&mut sem_diffs);
    let mut sem_printed = false;
    for d in &sem_diffs {
        let r = gpp_diff::render(d, &moves);
        if !r.is_empty() {
            print!("{r}");
            sem_printed = true;
        }
    }

    if !any {
        println!("(no changes)");
    } else if line_blocks.is_empty() && !sem_printed {
        println!("(no semantic changes — formatting/comments only; use --line for raw diff)");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// gpp branch
// ---------------------------------------------------------------------------

pub fn branch(args: &BranchArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let refs = refstore(&repo);

    match &args.action {
        None => {
            let head = refs.head_branch().unwrap_or_default();
            for b in refs.list()? {
                if b.name.starts_with("explorations/") && !args.all {
                    continue;
                }
                let marker = if b.name == head { "*" } else { " " };
                let tip = b
                    .tip
                    .map(|h| format!("cs:{}", h.short()))
                    .unwrap_or_else(|| "(empty)".into());
                println!("{marker} {:<28} {tip}", b.name);
            }
            Ok(())
        }
        Some(BranchAction::Create { name }) => {
            if refs.ref_exists(name) {
                bail!("branch {name:?} already exists");
            }
            let tip = refs
                .head_tip()?
                .ok_or_else(|| anyhow!("current branch has no changesets to branch from"))?;
            refs.write_ref(name, tip)?;
            println!("created branch {name} at cs:{}", tip.short());
            Ok(())
        }
        Some(BranchAction::Delete { name }) => {
            refs.delete_ref(name)?;
            println!("deleted branch {name}");
            Ok(())
        }
        Some(BranchAction::Switch { name }) => {
            if !refs.ref_exists(name) {
                bail!("branch {name:?} does not exist (create it first)");
            }
            refs.set_head_branch(name)?;
            println!("switched to branch {name}");
            Ok(())
        }
        Some(BranchAction::Explore { name }) => {
            let full = format!("explorations/{name}");
            let tip = refs
                .head_tip()?
                .ok_or_else(|| anyhow!("current branch has no changesets to explore from"))?;
            refs.write_ref(&full, tip)?;
            println!("created exploration branch {full} at cs:{}", tip.short());
            Ok(())
        }
    }
}

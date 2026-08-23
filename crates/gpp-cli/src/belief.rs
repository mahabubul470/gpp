//! `gpp belief` — VCS-native knowledge staleness.
//!
//! Beliefs are Graphex nodes whose claims are anchored to a changeset with
//! evidence spans. The staleness engine (`gpp_graphex::scan`) walks the
//! repo's own history, so "what did we believe, when did it go stale, and
//! which commit did it" is answered deterministically — no LLM, no network.

use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use gpp_core::{Hash, flatten_tree};
use gpp_graphex::{AccessTier, BeliefStatus, ScanHit, Scope};
use gpp_history::{Changeset, RefStore};

use crate::cli::{BeliefAction, BeliefArgs};
use crate::phase3::{config_author, default_tier, open_graph};
use crate::repo::Repo;

fn discover(repo_override: Option<&Path>) -> Result<Repo> {
    let start = match repo_override {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    Repo::discover(&start)
}

fn resolve_cs(repo: &Repo, spec: &str) -> Result<Hash> {
    let refs = RefStore::open(&repo.gpp_dir());
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
        .ok_or_else(|| anyhow!("cannot resolve {spec:?} to a changeset"))
}

fn head_tip(repo: &Repo) -> Result<Hash> {
    RefStore::open(&repo.gpp_dir())
        .head_tip()?
        .ok_or_else(|| anyhow!("no changesets yet — `gpp promote` first"))
}

fn ymd(us: i64) -> String {
    let days = us / 86_400_000_000;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// First line of a (possibly multi-line) commit message.
fn subject(msg: &str) -> &str {
    msg.lines().next().unwrap_or(msg)
}

fn commit_label(cs: &Hash, git: Option<&str>) -> String {
    match git {
        Some(sha) => format!("cs:{} (git {})", cs.short(), &sha[..sha.len().min(8)]),
        None => format!("cs:{}", cs.short()),
    }
}

fn hit_json(h: &ScanHit) -> serde_json::Value {
    serde_json::json!({
        "changeset": h.changeset.to_base32(),
        "git_commit": h.git_commit,
        "timestamp_us": h.timestamp,
        "message": h.message,
        "verdict": h.verdict.as_str(),
        "causes": h.causes.iter().map(|c| c.describe()).collect::<Vec<_>>(),
        "excerpt": h.excerpt,
    })
}

pub fn belief(args: &BeliefArgs, repo_override: Option<&Path>, json: bool) -> Result<()> {
    let repo = discover(repo_override)?;
    let gs = open_graph(&repo)?;

    match &args.action {
        BeliefAction::Add {
            claim,
            paths,
            symbols,
            evidence,
            tier,
        } => {
            if paths.is_empty() && symbols.is_empty() && evidence.is_empty() {
                bail!("a belief needs a scope: at least one --path, --symbol or --evidence");
            }
            let anchor = head_tip(&repo)?;
            let cs: Changeset = gs.objects().read(&anchor)?;
            let files = flatten_tree(gs.objects(), &cs.tree)?;

            let symbols = symbols
                .iter()
                .map(|s| gpp_graphex::parse_symbol_spec(s))
                .collect::<gpp_graphex::Result<Vec<_>>>()?;
            for w in gpp_graphex::verify_symbols(gs.objects(), &files, &symbols)? {
                eprintln!("warning: {w}");
            }
            let evidence = gpp_graphex::verify_evidence(gs.objects(), &files, evidence)?;
            let tier = match tier {
                Some(t) => AccessTier::parse(t)?,
                None => default_tier(&repo),
            };
            let scope = Scope {
                paths: paths.clone(),
                symbols,
            };
            let id = gpp_graphex::add_belief(
                &gs,
                claim,
                scope,
                anchor,
                cs.timestamp,
                evidence,
                tier,
                config_author(&repo),
            )
            .context("recording belief")?;

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "id": id.to_base32(),
                        "claim": claim,
                        "anchor": anchor.to_base32(),
                        "status": "active",
                    }))?
                );
            } else {
                println!(
                    "Recorded belief {} anchored at cs:{}",
                    id.short(),
                    anchor.short()
                );
                println!("  \"{claim}\"");
            }
            Ok(())
        }

        BeliefAction::Log { id } => {
            let (bid, node) = gpp_graphex::resolve_belief(&gs, id)?;
            let data = node
                .belief
                .as_ref()
                .ok_or_else(|| anyhow!("node {} has no belief payload", bid.short()))?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "id": bid.to_base32(),
                        "claim": node.description,
                        "status": data.status.as_str(),
                        "anchor": data.anchor.to_base32(),
                        "invalidated_by": data.invalidated_by.map(|h| h.to_base32()),
                        "history": data.history.iter().map(|h| serde_json::json!({
                            "changeset": h.changeset.to_base32(),
                            "at_us": h.at,
                            "status": h.to.as_str(),
                            "causes": h.causes.iter().map(|c| c.describe()).collect::<Vec<_>>(),
                        })).collect::<Vec<_>>(),
                    }))?
                );
                return Ok(());
            }
            println!("belief {}  \"{}\"", bid.short(), node.description);
            println!(
                "  status: {} · anchored at cs:{}",
                data.status.as_str(),
                data.anchor.short()
            );
            println!("  history:");
            for h in &data.history {
                let causes = if h.causes.is_empty() {
                    "(anchored)".to_string()
                } else {
                    h.causes
                        .iter()
                        .map(|c| c.describe())
                        .collect::<Vec<_>>()
                        .join("; ")
                };
                println!(
                    "    {}  {:<16} cs:{}  {causes}",
                    ymd(h.at),
                    h.to.as_str(),
                    h.changeset.short()
                );
            }
            Ok(())
        }

        BeliefAction::At { changeset } => {
            let target = resolve_cs(&repo, changeset)?;
            let anc = gpp_graphex::ancestors(gs.objects(), target)?;
            let mut rows = Vec::new();
            for (bid, node) in gpp_graphex::list_beliefs(&gs)? {
                let Some(data) = node.belief.as_ref() else {
                    continue;
                };
                if let Some(status) = data.status_at(&anc) {
                    rows.push((bid, node.description.clone(), status));
                }
            }
            if json {
                let arr: Vec<_> = rows
                    .iter()
                    .map(|(id, claim, status)| {
                        serde_json::json!({
                            "id": id.to_base32(),
                            "claim": claim,
                            "status": status.as_str(),
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&serde_json::json!(arr))?);
                return Ok(());
            }
            if rows.is_empty() {
                println!("(no beliefs existed at cs:{})", target.short());
                return Ok(());
            }
            println!("beliefs as of cs:{}:", target.short());
            for (bid, claim, status) in rows {
                println!("  {:<16} {}  \"{claim}\"", status.as_str(), bid.short());
            }
            Ok(())
        }

        BeliefAction::Stale { since } => {
            let tip = head_tip(&repo)?;
            let since_ts = match since {
                Some(s) => {
                    let h = resolve_cs(&repo, s)?;
                    Some(gs.objects().read::<Changeset>(&h)?.timestamp)
                }
                None => None,
            };
            let mut report = Vec::new();
            // Record + notify transitions (shared with `promote` and the
            // agent SDK), then report everything currently stale.
            gpp_sdk::scan_beliefs(&repo.gpp_dir(), tip)?;
            for (bid, node) in gpp_graphex::list_beliefs(&gs)? {
                match gpp_graphex::scan_and_record(&gs, &bid, tip) {
                    Ok((node, hits)) => {
                        let data = node.belief.expect("belief node scanned");
                        if matches!(
                            data.status,
                            BeliefStatus::StaleCandidate | BeliefStatus::Invalidated
                        ) {
                            let hits: Vec<ScanHit> = hits
                                .into_iter()
                                .filter(|h| since_ts.is_none_or(|t| h.timestamp >= t))
                                .collect();
                            report.push((bid, node.description, data.status, hits));
                        }
                    }
                    Err(e) => eprintln!(
                        "warning: skipping belief {} ({}): {e}",
                        bid.short(),
                        node.description
                    ),
                }
            }
            if json {
                let arr: Vec<_> = report
                    .iter()
                    .map(|(id, claim, status, hits)| {
                        serde_json::json!({
                            "id": id.to_base32(),
                            "claim": claim,
                            "status": status.as_str(),
                            "hits": hits.iter().map(hit_json).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&serde_json::json!(arr))?);
                return Ok(());
            }
            if report.is_empty() {
                println!("all beliefs hold (no scope intersections since anchor)");
                return Ok(());
            }
            for (bid, claim, status, hits) in report {
                println!("{:<16} {}  \"{claim}\"", status.as_str(), bid.short());
                for h in hits {
                    println!(
                        "    {}  {}  {}  — {}",
                        ymd(h.timestamp),
                        commit_label(&h.changeset, h.git_commit.as_deref()),
                        h.verdict.as_str(),
                        h.causes
                            .iter()
                            .map(|c| c.describe())
                            .collect::<Vec<_>>()
                            .join("; ")
                    );
                }
            }
            Ok(())
        }

        BeliefAction::Bisect { id } => {
            let tip = head_tip(&repo)?;
            let (bid, _) = gpp_graphex::resolve_belief(&gs, id)?;
            let (node, hits) = gpp_graphex::scan_and_record(&gs, &bid, tip)?;
            let data = node.belief.as_ref().expect("belief node scanned");

            let culprit = hits
                .iter()
                .find(|h| h.verdict == BeliefStatus::Invalidated)
                .or_else(|| hits.first());

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "id": bid.to_base32(),
                        "claim": node.description,
                        "status": data.status.as_str(),
                        "anchor": data.anchor.to_base32(),
                        "culprit": culprit.map(hit_json),
                        "hits": hits.iter().map(hit_json).collect::<Vec<_>>(),
                    }))?
                );
                return Ok(());
            }

            println!("belief {}  \"{}\"", bid.short(), node.description);
            println!(
                "  anchored at cs:{} · status: {}",
                data.anchor.short(),
                data.status.as_str()
            );
            let Some(hit) = culprit else {
                println!("\nno commit since the anchor intersects this belief — it holds.");
                return Ok(());
            };
            let earlier = hits
                .iter()
                .take_while(|h| h.changeset != hit.changeset)
                .count();
            if earlier > 0 {
                println!("  ({earlier} earlier stale signal(s) before the verdict below)");
            }
            println!();
            println!(
                "{}  {}  {}",
                hit.verdict.as_str().to_uppercase(),
                commit_label(&hit.changeset, hit.git_commit.as_deref()),
                ymd(hit.timestamp)
            );
            println!("  \"{}\"", subject(&hit.message));
            for c in &hit.causes {
                println!("  cause: {}", c.describe());
            }
            if let Some(x) = &hit.excerpt {
                println!();
                print!("{x}");
            }
            Ok(())
        }

        BeliefAction::Reaffirm { id, evidence } => {
            let tip = head_tip(&repo)?;
            let (bid, _) = gpp_graphex::resolve_belief(&gs, id)?;
            let cs: Changeset = gs.objects().read(&tip)?;
            let files = flatten_tree(gs.objects(), &cs.tree)?;
            let author = config_author(&repo);
            gpp_graphex::reaffirm_belief(
                &gs,
                &bid,
                tip,
                cs.timestamp,
                &files,
                evidence,
                &author.identity,
            )
            .context("reaffirming belief")?;

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "id": bid.to_base32(),
                        "status": "reaffirmed",
                        "anchor": tip.to_base32(),
                    }))?
                );
            } else {
                println!(
                    "Reaffirmed belief {} — re-anchored at cs:{}",
                    bid.short(),
                    tip.short()
                );
            }
            Ok(())
        }
    }
}

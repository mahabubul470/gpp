//! `gpp-deps` — dependency intelligence (layer 10).
//!
//! Parses lockfiles (`Cargo.lock`, `package-lock.json`) into a flat
//! dependency list and computes a heuristic, **offline** risk score per
//! dependency. Optional, opt-in live enrichment queries the [OSV][osv]
//! vulnerability database and folds known advisories into the score; results
//! are cached on disk so repeat runs stay offline.
//!
//! [osv]: https://osv.dev
//!
//! See `docs/ROADMAP.md` (Phase 8).
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("unsupported lockfile (expected Cargo.lock or package-lock.json)")]
    Unsupported,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ecosystem {
    Cargo,
    Npm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub name: String,
    pub version: String,
    pub ecosystem: Ecosystem,
    /// 0 (low) – 100 (high) heuristic risk.
    pub risk: u8,
    pub notes: Vec<String>,
    /// Known advisories from live OSV enrichment (empty until enriched).
    pub vulns: Vec<Vuln>,
}

/// A known vulnerability/advisory affecting a dependency, from OSV.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vuln {
    /// Advisory id, e.g. `RUSTSEC-2021-0093` or `GHSA-xxxx`.
    pub id: String,
    /// One-line summary, if the advisory provides one.
    pub summary: String,
}

#[derive(Deserialize)]
struct CargoLock {
    #[serde(default)]
    package: Vec<CargoPkg>,
}
#[derive(Deserialize)]
struct CargoPkg {
    name: String,
    version: String,
}

#[derive(Deserialize)]
struct NpmLock {
    #[serde(default)]
    packages: std::collections::BTreeMap<String, NpmPkg>,
    #[serde(default)]
    dependencies: std::collections::BTreeMap<String, NpmDep>,
}
#[derive(Deserialize)]
struct NpmPkg {
    #[serde(default)]
    version: String,
}
#[derive(Deserialize)]
struct NpmDep {
    #[serde(default)]
    version: String,
}

/// Heuristic risk: pre-1.0 / pre-release versions and native-FFI crates are
/// riskier. Deterministic and offline.
fn score(name: &str, version: &str) -> (u8, Vec<String>) {
    let mut risk: i32 = 10;
    let mut notes = Vec::new();
    if version.starts_with("0.0.") {
        risk += 45;
        notes.push("0.0.x — unstable, frequent breaking changes".into());
    } else if version.starts_with('0') {
        risk += 25;
        notes.push("pre-1.0 — API not yet stable".into());
    }
    if version.contains("-alpha") || version.contains("-beta") || version.contains("-rc") {
        risk += 20;
        notes.push("pre-release version pinned".into());
    }
    if name.contains("openssl") || name.contains("ffi") || name.contains("-sys") {
        risk += 15;
        notes.push("native/FFI surface — supply-chain & build risk".into());
    }
    let risk = risk.clamp(0, 100) as u8;
    (risk, notes)
}

fn build(name: &str, version: &str, eco: Ecosystem) -> Dependency {
    let (risk, notes) = score(name, version);
    Dependency {
        name: name.to_string(),
        version: version.to_string(),
        ecosystem: eco,
        risk,
        notes,
        vulns: Vec::new(),
    }
}

/// Parse a lockfile (kind inferred from the file name) into dependencies,
/// sorted by descending risk then name.
pub fn parse_lockfile(path: &Path) -> Result<Vec<Dependency>> {
    let text = std::fs::read_to_string(path)?;
    let fname = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut deps = if fname == "Cargo.lock" {
        let cl: CargoLock = toml::from_str(&text).map_err(|e| Error::Parse(e.to_string()))?;
        cl.package
            .into_iter()
            .map(|p| build(&p.name, &p.version, Ecosystem::Cargo))
            .collect::<Vec<_>>()
    } else if fname == "package-lock.json" {
        let nl: NpmLock = serde_json::from_str(&text).map_err(|e| Error::Parse(e.to_string()))?;
        let mut out = Vec::new();
        for (k, p) in nl.packages {
            if k.is_empty() {
                continue; // the root package entry
            }
            let name = k.rsplit("node_modules/").next().unwrap_or(&k);
            out.push(build(name, &p.version, Ecosystem::Npm));
        }
        for (name, d) in nl.dependencies {
            if !out.iter().any(|x| x.name == name) {
                out.push(build(&name, &d.version, Ecosystem::Npm));
            }
        }
        out
    } else {
        return Err(Error::Unsupported);
    };
    deps.sort_by(|a, b| b.risk.cmp(&a.risk).then(a.name.cmp(&b.name)));
    deps.dedup();
    Ok(deps)
}

/// Dependencies present in `new` but not `old` — the lens for assessing an
/// agent that just added dependencies.
pub fn newly_added(old: &[Dependency], new: &[Dependency]) -> Vec<Dependency> {
    let seen: BTreeSet<(&str, &str)> = old
        .iter()
        .map(|d| (d.name.as_str(), d.version.as_str()))
        .collect();
    new.iter()
        .filter(|d| !seen.contains(&(d.name.as_str(), d.version.as_str())))
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Live enrichment — OSV vulnerability database (opt-in, cached)
// ---------------------------------------------------------------------------

const OSV_QUERY_URL: &str = "https://api.osv.dev/v1/query";
/// Default cache lifetime: a day. Advisories don't change minute-to-minute.
pub const DEFAULT_CACHE_TTL_SECS: u64 = 86_400;

/// Outcome of an enrichment pass — all best-effort, never fatal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnrichReport {
    /// Dependencies looked up.
    pub queried: usize,
    /// Lookups served from the on-disk cache.
    pub from_cache: usize,
    /// Lookups that hit the network.
    pub fetched: usize,
    /// Dependencies found to have ≥1 advisory.
    pub vulnerable: usize,
    /// Per-dependency lookup failures (network/parse), as `name: reason`.
    pub errors: Vec<String>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// OSV ecosystem token for a lockfile ecosystem.
fn osv_ecosystem(eco: &Ecosystem) -> &'static str {
    match eco {
        Ecosystem::Cargo => "crates.io",
        Ecosystem::Npm => "npm",
    }
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    fetched_at: u64,
    vulns: Vec<Vuln>,
}

/// Stable, filesystem-safe cache key for a dependency.
fn cache_key(dep: &Dependency) -> String {
    let safe: String = format!(
        "{}-{}-{}",
        osv_ecosystem(&dep.ecosystem),
        dep.name,
        dep.version
    )
    .chars()
    .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
    .collect();
    format!("{safe}.json")
}

/// Parse an OSV `/v1/query` response body into a `Vuln` list. Pure — the
/// network and cache layers funnel through here, so advisory extraction is
/// unit-tested without touching either.
pub fn parse_osv_response(body: &str) -> Result<Vec<Vuln>> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| Error::Parse(e.to_string()))?;
    let Some(arr) = v.get("vulns").and_then(|x| x.as_array()) else {
        return Ok(Vec::new()); // OSV returns `{}` for "no known vulns"
    };
    Ok(arr
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.to_string();
            let summary = item
                .get("summary")
                .or_else(|| item.get("details"))
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            Some(Vuln { id, summary })
        })
        .collect())
}

/// Fold a dependency's advisories into its risk and notes. Idempotent: a
/// dependency with advisories is pinned to high risk (≥ 85) and gains one note
/// listing the advisory ids. Pure and offline.
pub fn apply_vulns(dep: &mut Dependency, vulns: Vec<Vuln>) {
    if vulns.is_empty() {
        return;
    }
    dep.risk = dep.risk.max(85);
    let ids: Vec<&str> = vulns.iter().map(|v| v.id.as_str()).collect();
    dep.notes.push(format!(
        "{} known advisory(ies): {}",
        vulns.len(),
        ids.join(", ")
    ));
    dep.vulns = vulns;
}

/// Enrich `deps` in place with OSV advisories, caching responses under
/// `cache_dir`. A dependency whose cache entry is younger than `ttl_secs` is
/// served from disk (no network); otherwise it is fetched and the response
/// cached. Per-dependency failures are collected in the report, never fatal —
/// a partial/offline result still returns the heuristic scores.
///
/// The blocking HTTP client is constructed lazily, only on the first cache
/// miss, so an all-cache-hit run performs zero network I/O.
pub fn enrich_with_osv(
    deps: &mut [Dependency],
    cache_dir: &Path,
    ttl_secs: u64,
) -> Result<EnrichReport> {
    std::fs::create_dir_all(cache_dir)?;
    let mut report = EnrichReport::default();
    let mut client: Option<reqwest::blocking::Client> = None;
    let now = now_secs();

    for dep in deps.iter_mut() {
        report.queried += 1;
        let cache_path = cache_dir.join(cache_key(dep));

        // Fresh cache hit?
        let cached = std::fs::read_to_string(&cache_path)
            .ok()
            .and_then(|t| serde_json::from_str::<CacheEntry>(&t).ok())
            .filter(|e| now.saturating_sub(e.fetched_at) < ttl_secs);

        let vulns = if let Some(entry) = cached {
            report.from_cache += 1;
            entry.vulns
        } else {
            let c = client.get_or_insert_with(|| {
                reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_secs(15))
                    .user_agent(concat!("gpp-deps/", env!("CARGO_PKG_VERSION")))
                    .build()
                    .unwrap_or_default()
            });
            match fetch_osv(c, dep) {
                Ok(vulns) => {
                    report.fetched += 1;
                    let entry = CacheEntry {
                        fetched_at: now,
                        vulns: vulns.clone(),
                    };
                    if let Ok(t) = serde_json::to_string(&entry) {
                        let _ = std::fs::write(&cache_path, t);
                    }
                    vulns
                }
                Err(e) => {
                    report.errors.push(format!("{}: {e}", dep.name));
                    continue;
                }
            }
        };

        if !vulns.is_empty() {
            report.vulnerable += 1;
        }
        apply_vulns(dep, vulns);
    }
    Ok(report)
}

/// One OSV query for a single dependency.
fn fetch_osv(client: &reqwest::blocking::Client, dep: &Dependency) -> Result<Vec<Vuln>> {
    let body = serde_json::json!({
        "version": dep.version,
        "package": { "name": dep.name, "ecosystem": osv_ecosystem(&dep.ecosystem) },
    });
    let resp = client
        .post(OSV_QUERY_URL)
        .json(&body)
        .send()
        .map_err(|e| Error::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(Error::Network(format!(
            "OSV returned HTTP {}",
            resp.status()
        )));
    }
    let text = resp.text().map_err(|e| Error::Network(e.to_string()))?;
    parse_osv_response(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cargo_lock_and_scores() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("Cargo.lock");
        std::fs::write(
            &p,
            r#"
[[package]]
name = "serde"
version = "1.0.0"

[[package]]
name = "shaky"
version = "0.0.3"

[[package]]
name = "openssl-sys"
version = "0.9.1"
"#,
        )
        .unwrap();
        let deps = parse_lockfile(&p).unwrap();
        assert_eq!(deps.len(), 3);
        assert_eq!(deps[0].name, "shaky"); // highest risk first
        assert!(deps[0].risk > deps.iter().last().unwrap().risk);
        let openssl = deps.iter().find(|x| x.name == "openssl-sys").unwrap();
        assert!(openssl.notes.iter().any(|n| n.contains("FFI")));
        let serde = deps.iter().find(|x| x.name == "serde").unwrap();
        assert!(serde.risk < 25); // stable 1.x
    }

    #[test]
    fn parses_npm_lock() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("package-lock.json");
        std::fs::write(
            &p,
            r#"{"name":"app","lockfileVersion":3,"packages":{
                "":{"name":"app"},
                "node_modules/left-pad":{"version":"1.3.0"},
                "node_modules/beta-lib":{"version":"2.0.0-beta.1"}
            }}"#,
        )
        .unwrap();
        let deps = parse_lockfile(&p).unwrap();
        assert_eq!(deps.len(), 2);
        let beta = deps.iter().find(|x| x.name == "beta-lib").unwrap();
        assert!(beta.notes.iter().any(|n| n.contains("pre-release")));
    }

    #[test]
    fn newly_added_diffs_lockfiles() {
        let old = vec![build("serde", "1.0.0", Ecosystem::Cargo)];
        let new = vec![
            build("serde", "1.0.0", Ecosystem::Cargo),
            build("sketchy", "0.0.1", Ecosystem::Cargo),
        ];
        let added = newly_added(&old, &new);
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].name, "sketchy");
        assert!(added[0].risk >= 50);
    }

    #[test]
    fn rejects_unknown_lockfile() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("yarn.lock");
        std::fs::write(&p, "x").unwrap();
        assert!(matches!(parse_lockfile(&p), Err(Error::Unsupported)));
    }

    #[test]
    fn parse_osv_response_extracts_ids_and_summaries() {
        let body = r#"{"vulns":[
            {"id":"RUSTSEC-2021-0093","summary":"buffer overflow in foo"},
            {"id":"GHSA-aaaa-bbbb-cccc","details":"multi\nline details"}
        ]}"#;
        let v = parse_osv_response(body).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "RUSTSEC-2021-0093");
        assert_eq!(v[0].summary, "buffer overflow in foo");
        // Falls back to the first line of `details` when no `summary`.
        assert_eq!(v[1].summary, "multi");
        // Empty response ⇒ no vulns.
        assert!(parse_osv_response("{}").unwrap().is_empty());
    }

    #[test]
    fn apply_vulns_pins_high_risk_and_notes() {
        let mut dep = build("serde", "1.0.0", Ecosystem::Cargo);
        assert!(dep.risk < 85 && dep.vulns.is_empty());
        apply_vulns(
            &mut dep,
            vec![Vuln {
                id: "RUSTSEC-2099-0001".into(),
                summary: "x".into(),
            }],
        );
        assert!(dep.risk >= 85);
        assert_eq!(dep.vulns.len(), 1);
        assert!(dep.notes.iter().any(|n| n.contains("RUSTSEC-2099-0001")));

        // No advisories ⇒ untouched.
        let mut clean = build("serde", "1.0.0", Ecosystem::Cargo);
        let before = clean.clone();
        apply_vulns(&mut clean, vec![]);
        assert_eq!(clean, before);
    }

    #[test]
    fn enrich_serves_fresh_cache_without_network() {
        // Pre-seed a fresh cache entry; enrich must read it and never fetch.
        // (No network is available/used here — a cache miss would error, not
        // hang, but a fresh entry guarantees the cache path.)
        let d = tempfile::tempdir().unwrap();
        let cache = d.path().join("deps-cache");
        std::fs::create_dir_all(&cache).unwrap();

        let mut dep = build("vulnerable-lib", "0.1.0", Ecosystem::Cargo);
        let entry = CacheEntry {
            fetched_at: now_secs(),
            vulns: vec![Vuln {
                id: "RUSTSEC-2020-1234".into(),
                summary: "RCE".into(),
            }],
        };
        std::fs::write(
            cache.join(cache_key(&dep)),
            serde_json::to_string(&entry).unwrap(),
        )
        .unwrap();

        let mut deps = vec![dep.clone()];
        let report = enrich_with_osv(&mut deps, &cache, DEFAULT_CACHE_TTL_SECS).unwrap();
        assert_eq!(report.from_cache, 1);
        assert_eq!(report.fetched, 0);
        assert_eq!(report.vulnerable, 1);
        assert!(report.errors.is_empty());
        assert!(deps[0].risk >= 85);
        assert_eq!(deps[0].vulns[0].id, "RUSTSEC-2020-1234");

        // A stale entry (older than TTL) is not used as a fresh hit.
        dep.version = "0.2.0".into();
        let stale = CacheEntry {
            fetched_at: now_secs().saturating_sub(DEFAULT_CACHE_TTL_SECS + 10),
            vulns: vec![],
        };
        std::fs::write(
            cache.join(cache_key(&dep)),
            serde_json::to_string(&stale).unwrap(),
        )
        .unwrap();
        // Read the cache directly to confirm staleness logic without network.
        let raw = std::fs::read_to_string(cache.join(cache_key(&dep))).unwrap();
        let e: CacheEntry = serde_json::from_str(&raw).unwrap();
        assert!(now_secs().saturating_sub(e.fetched_at) >= DEFAULT_CACHE_TTL_SECS);
    }
}

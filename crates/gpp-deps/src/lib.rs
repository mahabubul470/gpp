//! `gpp-deps` — dependency intelligence (layer 10).
//!
//! Parses lockfiles (`Cargo.lock`, `package-lock.json`) into a flat
//! dependency list and computes a heuristic, **offline** risk score per
//! dependency. Live registry/CVE/license-API enrichment is a follow-up
//! (network + API keys); the parsing, scoring and the
//! "agent added a dependency" assessment are implemented and tested.
//!
//! See `docs/ROADMAP.md` (Phase 8).
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(String),
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
}

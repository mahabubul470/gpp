//! Repository config generation and generic dotted-key TOML access.
//!
//! The typed model below mirrors `docs/DATA_MODEL.md` § Repository Config and
//! is used only to *generate* `.gpp/config.toml` on `gpp init`. `gpp config
//! get/set/list` operates generically on the parsed TOML so it keeps working
//! as new sections are added in later phases.

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;

/// Options that influence the generated config (from `gpp init` flags).
#[derive(Debug, Default)]
pub struct InitOptions {
    pub graphex: bool,
    pub timeline: bool,
    pub encryption: bool,
    pub git_bridge_url: Option<String>,
}

#[derive(Serialize)]
struct RepoConfig {
    core: Core,
    timeline: Timeline,
    graphex: Graphex,
    trust: Trust,
    cost: Cost,
    review: Review,
    sync: Sync,
    relay: Relay,
    remote: Remote,
    #[serde(rename = "git-bridge")]
    git_bridge: GitBridge,
}

#[derive(Serialize)]
struct Core {
    version: String,
    encryption: bool,
}

#[derive(Serialize)]
struct Timeline {
    enabled: bool,
    debounce_ms: u32,
    retention_days: u32,
    ignore: Vec<String>,
}

#[derive(Serialize)]
struct Graphex {
    enabled: bool,
    default_access_tier: String,
    federation: Vec<String>,
    distribute_via: String,
}

#[derive(Serialize)]
struct Trust {
    auto_merge_min: u32,
    review_required_min: u32,
    sandbox_min: u32,
}

#[derive(Serialize)]
struct Cost {
    enabled: bool,
    default_budget_weekly: u64,
}

#[derive(Serialize)]
struct Review {
    auto_assign_owners: bool,
    min_reviewers: u32,
    require_human: bool,
    auto_create_on_promote: bool,
}

#[derive(Serialize)]
struct Sync {
    peers: Vec<String>,
    transport: String,
    port: u16,
}

#[derive(Serialize)]
struct Relay {
    enabled: bool,
    address: String,
    auto_push: bool,
}

#[derive(Serialize)]
struct Remote {
    platform: String,
    api_token_env: String,
    repository: String,
    pr: RemotePr,
    sync: RemoteSync,
}

#[derive(Serialize)]
struct RemotePr {
    auto_create: bool,
    include_intent: bool,
    include_semantic_diff: bool,
    include_agent_meta: bool,
    include_policy_results: bool,
    include_cost: bool,
    draft: bool,
}

#[derive(Serialize)]
struct RemoteSync {
    mirror_reviews: bool,
    mirror_comments: bool,
    import_ci_status: bool,
}

#[derive(Serialize)]
struct GitBridge {
    enabled: bool,
    remote: String,
    auto_sync: bool,
}

/// Render the initial `.gpp/config.toml` for the given options.
pub fn render_repo_config(opts: &InitOptions) -> Result<String> {
    let git_bridge_url = opts.git_bridge_url.clone();
    let cfg = RepoConfig {
        core: Core {
            version: env!("CARGO_PKG_VERSION").to_string(),
            encryption: opts.encryption,
        },
        timeline: Timeline {
            enabled: opts.timeline,
            debounce_ms: 100,
            retention_days: 30,
            ignore: [
                ".gpp/**",
                "node_modules/**",
                "target/**",
                ".git/**",
                "*.pyc",
                "__pycache__/**",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        },
        graphex: Graphex {
            enabled: opts.graphex,
            default_access_tier: "agent-readable".into(),
            federation: vec![],
            distribute_via: "relay".into(),
        },
        trust: Trust {
            auto_merge_min: 90,
            review_required_min: 70,
            sandbox_min: 50,
        },
        cost: Cost {
            enabled: true,
            default_budget_weekly: 500_000_000,
        },
        review: Review {
            auto_assign_owners: true,
            min_reviewers: 1,
            require_human: true,
            auto_create_on_promote: true,
        },
        sync: Sync {
            peers: vec![],
            transport: "tcp+noise".into(),
            port: 9473,
        },
        relay: Relay {
            enabled: false,
            address: String::new(),
            auto_push: true,
        },
        remote: Remote {
            platform: "github".into(),
            api_token_env: "GITHUB_TOKEN".into(),
            repository: String::new(),
            pr: RemotePr {
                auto_create: true,
                include_intent: true,
                include_semantic_diff: true,
                include_agent_meta: true,
                include_policy_results: true,
                include_cost: true,
                draft: false,
            },
            sync: RemoteSync {
                mirror_reviews: true,
                mirror_comments: true,
                import_ci_status: true,
            },
        },
        git_bridge: GitBridge {
            enabled: git_bridge_url.is_some(),
            remote: git_bridge_url.unwrap_or_default(),
            auto_sync: false,
        },
    };
    let body = toml::to_string_pretty(&cfg).context("failed to render config.toml")?;
    Ok(format!(
        "# gpp repository configuration — see docs/DATA_MODEL.md\n\n{body}"
    ))
}

/// Path to the global config (`$XDG_CONFIG_HOME/gpp/config.toml`).
pub fn global_config_path() -> Result<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        return Ok(PathBuf::from(xdg).join("gpp/config.toml"));
    }
    let home = std::env::var("HOME").context("HOME is not set; cannot locate global config")?;
    Ok(PathBuf::from(home).join(".config/gpp/config.toml"))
}

/// Read a dotted key (`trust.auto_merge_min`) from a TOML document.
pub fn get_key<'a>(doc: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    let mut cur = doc;
    for part in key.split('.') {
        cur = cur.as_table()?.get(part)?;
    }
    Some(cur)
}

/// Set a dotted key, creating intermediate tables. Value is parsed as
/// bool / integer / float, falling back to a string.
pub fn set_key(doc: &mut toml::Value, key: &str, raw: &str) -> Result<()> {
    let parsed = parse_scalar(raw);
    let parts: Vec<&str> = key.split('.').collect();
    let (last, parents) = parts
        .split_last()
        .ok_or_else(|| anyhow!("empty config key"))?;
    let mut cur = doc;
    for part in parents {
        let table = cur
            .as_table_mut()
            .ok_or_else(|| anyhow!("config path {key:?} traverses a non-table value"))?;
        cur = table
            .entry((*part).to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    }
    cur.as_table_mut()
        .ok_or_else(|| anyhow!("config path {key:?} traverses a non-table value"))?
        .insert((*last).to_string(), parsed);
    Ok(())
}

fn parse_scalar(raw: &str) -> toml::Value {
    if let Ok(b) = raw.parse::<bool>() {
        toml::Value::Boolean(b)
    } else if let Ok(i) = raw.parse::<i64>() {
        toml::Value::Integer(i)
    } else if let Ok(fl) = raw.parse::<f64>() {
        toml::Value::Float(fl)
    } else {
        toml::Value::String(raw.to_string())
    }
}

/// Flatten a TOML document into sorted `dotted.key = value` lines.
pub fn flatten(doc: &toml::Value) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(prefix: &str, v: &toml::Value, out: &mut Vec<String>) {
        match v {
            toml::Value::Table(t) => {
                for (k, val) in t {
                    let p = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    walk(&p, val, out);
                }
            }
            other => out.push(format!("{prefix} = {other}")),
        }
    }
    walk("", doc, &mut out);
    out.sort();
    out
}

/// Load a TOML file into a `Value`, or an empty table if it does not exist.
pub fn load_doc(path: &std::path::Path) -> Result<toml::Value> {
    match std::fs::read_to_string(path) {
        Ok(s) => toml::from_str(&s).with_context(|| format!("invalid TOML in {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(toml::Value::Table(toml::map::Map::new()))
        }
        Err(e) => bail!("failed to read {}: {e}", path.display()),
    }
}

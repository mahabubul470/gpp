//! Implementations of the Phase 0 commands: `init`, `status`, `config`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use gpp_core::ObjectStore;

use crate::cli::{ConfigAction, ConfigArgs, InitArgs, StatusArgs};
use crate::config::{self, InitOptions};
use crate::repo::{GPP_SUBDIRS, Repo};

/// `gpp init` — create a new repository.
pub fn init(args: &InitArgs, json: bool, quiet: bool) -> Result<()> {
    if args.from_git.is_some() {
        bail!(
            "--from-git is not implemented yet (Git import lands in Phase 2; see docs/ROADMAP.md)"
        );
    }
    if args.template.is_some() {
        bail!("--template is not implemented yet (project templates land in a later phase)");
    }

    let target = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&target)
        .with_context(|| format!("cannot create {}", target.display()))?;
    let root = target
        .canonicalize()
        .with_context(|| format!("cannot access {}", target.display()))?;
    let gpp_dir = root.join(".gpp");

    if gpp_dir.exists() {
        bail!("{} is already a gpp repository", root.display());
    }

    for sub in GPP_SUBDIRS {
        std::fs::create_dir_all(gpp_dir.join(sub))
            .with_context(|| format!("failed to create .gpp/{sub}"))?;
    }
    ObjectStore::init(&gpp_dir).context("failed to initialize object store")?;

    let opts = InitOptions {
        graphex: args.graphex,
        timeline: !args.no_timeline,
        encryption: args.encryption,
        git_bridge_url: args.git_bridge.clone(),
    };
    std::fs::write(
        gpp_dir.join("config.toml"),
        config::render_repo_config(&opts)?,
    )
    .context("failed to write config.toml")?;

    // HEAD points at the default branch; the ref file itself is created when
    // the first changeset is promoted (Phase 1).
    std::fs::write(gpp_dir.join("HEAD"), "ref: refs/main\n").context("failed to write HEAD")?;

    if json {
        let out = serde_json::json!({
            "initialized": true,
            "root": root,
            "graphex": args.graphex,
            "timeline": !args.no_timeline,
            "encryption": args.encryption,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else if !quiet {
        println!("Initialized empty gpp repository in {}", gpp_dir.display());
        if args.graphex {
            println!("  Graphex knowledge graph: enabled");
        }
        if args.encryption {
            println!("  Full-repo encryption: enabled");
        }
        if args.no_timeline {
            println!("  Timeline capture: disabled");
        }
        if let Some(url) = &args.git_bridge {
            println!("  Git bridge remote: {url} (sync lands in Phase 2)");
        }
    }
    Ok(())
}

/// `gpp status` — show repository state.
pub fn status(args: &StatusArgs, repo_override: Option<&Path>, json: bool) -> Result<()> {
    let start = match repo_override {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    let repo = Repo::discover(&start)?;
    let branch = repo.current_branch()?;
    let objects = repo.object_count();

    let doc = config::load_doc(&repo.config_path())?;
    let timeline_enabled = config::get_key(&doc, "timeline.enabled")
        .and_then(toml::Value::as_bool)
        .unwrap_or(true);

    if json {
        let out = serde_json::json!({
            "branch": branch,
            "objects": objects,
            "timeline": {
                "enabled": timeline_enabled,
                "entries": 0,
                "status": "not-implemented (Phase 1)",
            },
            "unpromoted_changes": 0,
            "active_agents": [],
            "policy_violations": 0,
            "session_cost_microdollars": 0,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    if args.short {
        println!("{branch} · {objects} objects · 0 unpromoted");
        return Ok(());
    }

    println!("On branch: {branch}");
    println!("Objects: {objects} stored");
    println!(
        "Timeline: {} (0 entries — capture engine lands in Phase 1)",
        if timeline_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!("Unpromoted changes: 0");
    println!("Active agents: none");
    println!("Policy violations: 0");
    println!("Cost this session: $0.00");

    if args.timeline || args.agents || args.cost {
        println!();
        println!("(--timeline/--agents/--cost detail is not available until Phase 1)");
    }
    Ok(())
}

/// `gpp config` — get / set / list / edit configuration.
pub fn config(args: &ConfigArgs, repo_override: Option<&Path>, quiet: bool) -> Result<()> {
    let path = if args.global {
        config::global_config_path()?
    } else {
        let start = match repo_override {
            Some(p) => p.to_path_buf(),
            None => std::env::current_dir()?,
        };
        Repo::discover(&start)?.config_path()
    };

    match &args.action {
        ConfigAction::Get { key } => {
            let doc = config::load_doc(&path)?;
            match config::get_key(&doc, key) {
                Some(v) => {
                    println!("{}", scalar_display(v));
                    Ok(())
                }
                None => bail!("config key {key:?} is not set"),
            }
        }
        ConfigAction::Set { key, value } => {
            let mut doc = config::load_doc(&path)?;
            config::set_key(&mut doc, key, value)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, toml::to_string_pretty(&doc)?)
                .with_context(|| format!("failed to write {}", path.display()))?;
            if !quiet {
                println!("{key} = {value}");
            }
            Ok(())
        }
        ConfigAction::List => {
            let doc = config::load_doc(&path)?;
            for line in config::flatten(&doc) {
                println!("{line}");
            }
            Ok(())
        }
        ConfigAction::Edit => {
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let status = std::process::Command::new(&editor)
                .arg(&path)
                .status()
                .with_context(|| format!("failed to launch editor {editor:?}"))?;
            if !status.success() {
                bail!("editor exited with status {status}");
            }
            Ok(())
        }
    }
}

fn scalar_display(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

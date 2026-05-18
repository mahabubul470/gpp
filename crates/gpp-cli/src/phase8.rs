//! Phase 8 commands: `gpp ui` (terminal UI) and `gpp deps` (dependency
//! intelligence).

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use gpp_tui::{LayoutPreset, Panel};

use crate::cli::{DepsArgs, UiArgs};
use crate::repo::Repo;

fn discover(repo_override: Option<&Path>) -> Result<Repo> {
    let start = match repo_override {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    Repo::discover(&start)
}

pub fn ui(args: &UiArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let layout = LayoutPreset::parse(&args.layout)
        .ok_or_else(|| anyhow!("unknown layout {:?}", args.layout))?;
    let focus = match &args.panel {
        Some(p) => Some(Panel::parse(p).ok_or_else(|| anyhow!("unknown panel {p:?}"))?),
        None => None,
    };
    gpp_tui::run(&repo.gpp_dir(), layout, focus, !args.no_live)?;
    Ok(())
}

fn autodetect_lockfile(root: &Path) -> Option<PathBuf> {
    for name in ["Cargo.lock", "package-lock.json"] {
        let p = root.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

pub fn deps(args: &DepsArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override).ok();
    let lockfile = match &args.lockfile {
        Some(p) => p.clone(),
        None => {
            let root = repo
                .as_ref()
                .map(|r| r.root.clone())
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            autodetect_lockfile(&root)
                .ok_or_else(|| anyhow!("no Cargo.lock / package-lock.json found"))?
        }
    };

    let mut deps = gpp_deps::parse_lockfile(&lockfile).map_err(|e| anyhow!("{e}"))?;

    if let Some(old) = &args.since {
        let old_deps = gpp_deps::parse_lockfile(old).map_err(|e| anyhow!("{e}"))?;
        deps = gpp_deps::newly_added(&old_deps, &deps);
        if deps.is_empty() {
            println!("no newly-added dependencies vs {}", old.display());
            return Ok(());
        }
        println!("newly-added dependencies:");
    }

    let mut shown = 0;
    for d in &deps {
        if d.risk < args.min_risk {
            continue;
        }
        shown += 1;
        let flag = if d.risk >= 60 {
            "⚠"
        } else if d.risk >= 30 {
            "•"
        } else {
            " "
        };
        println!("{flag} {:<28} {:<14} risk {:>3}", d.name, d.version, d.risk);
        for n in &d.notes {
            println!("    - {n}");
        }
    }
    if shown == 0 {
        bail!("no dependencies at or above risk {}", args.min_risk);
    }
    println!(
        "\n{shown} dependency(ies) shown from {}",
        lockfile.display()
    );
    Ok(())
}

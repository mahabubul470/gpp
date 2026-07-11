//! `gpp` — command-line interface.
//!
//! Phase 0 surface: `init`, `status`, `config`. Other commands from
//! `docs/CLI_SPEC.md` arrive in later roadmap phases.
#![forbid(unsafe_code)]

mod belief;
mod cli;
mod commands;
mod config;
mod mcp;
mod phase1;
mod phase2;
mod phase3;
mod phase4;
mod phase5;
mod phase6;
mod phase7;
mod phase8;
mod repo;

use std::process::ExitCode;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use cli::{Cli, Command};

fn main() -> ExitCode {
    let args = Cli::parse();
    init_tracing(args.verbose);

    let repo_override = args.repo.as_deref();
    let result = match &args.command {
        Command::Init(a) => commands::init(a, args.json, args.quiet),
        Command::Status(a) => commands::status(a, repo_override, args.json),
        Command::Config(a) => commands::config(a, repo_override, args.quiet),
        Command::Timeline(a) => phase1::timeline(a, repo_override, args.json),
        Command::Promote(a) => phase1::promote(a, repo_override),
        Command::Log(a) => phase1::log(a, repo_override),
        Command::Diff(a) => phase1::diff(a, repo_override),
        Command::Branch(a) => phase1::branch(a, repo_override),
        Command::GitImport(a) => phase2::git_import(a, repo_override),
        Command::GitExport(a) => phase2::git_export(a, repo_override),
        Command::GitBridge(a) => phase2::git_bridge(a, repo_override),
        Command::Keys(a) => phase3::keys(a, repo_override),
        Command::Graphex(a) => phase3::graphex(a, repo_override, args.json),
        Command::Belief(a) => belief::belief(a, repo_override, args.json),
        Command::McpServer(a) => phase3::mcp_server(a, repo_override),
        Command::Trust(a) => phase4::trust(a, repo_override, args.json),
        Command::Policy(a) => phase4::policy(a, repo_override),
        Command::Cost(a) => phase4::cost(a, repo_override, args.json),
        Command::Anomaly(a) => phase4::anomaly(a, repo_override),
        Command::Audit(a) => phase4::audit(a, repo_override),
        Command::Sync(a) => phase5::sync(a, repo_override),
        Command::Replay(a) => phase5::replay(a, repo_override),
        Command::Merge(a) => phase5::merge(a, repo_override),
        Command::Review(a) => phase6::review(a, repo_override),
        Command::Rbac(a) => phase6::rbac(a, repo_override),
        Command::Inbox(a) => phase6::inbox(a, repo_override),
        Command::Notify(a) => phase6::notify(a, repo_override),
        Command::Remote(a) => phase7::remote(a, repo_override),
        Command::Relay(a) => phase7::relay(a, repo_override),
        Command::Ui(a) => phase8::ui(a, repo_override),
        Command::Deps(a) => phase8::deps(a, repo_override),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn init_tracing(verbose: u8) {
    let default = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_env("GPP_LOG").unwrap_or_else(|_| EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .without_time()
        .init();
}

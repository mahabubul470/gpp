//! `gpp` — command-line interface.
//!
//! Phase 0 surface: `init`, `status`, `config`. Other commands from
//! `docs/CLI_SPEC.md` arrive in later roadmap phases.
#![forbid(unsafe_code)]

mod cli;
mod commands;
mod config;
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

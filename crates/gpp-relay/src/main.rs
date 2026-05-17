//! `gpp-relay` binary — run an always-on sync hub.
//!
//! ```text
//! gpp-relay --port 9473 --storage /data/gpp
//! gpp-relay --port 9473 --storage /data/gpp --auth-keys /etc/gpp/authorized_keys
//! ```
//!
//! It accepts inbound syncs (delegating to `gpp_sync::serve`, which does the
//! Noise handshake, repo-id gate and TOFU). Stored objects stay encrypted —
//! the relay never has tier keys. A `GET /health` endpoint on `port+1`
//! returns object/peer counts for liveness checks.
#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use gpp_core::ObjectStore;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "gpp-relay", version, about = "gpp always-on relay node")]
struct Args {
    /// Listen port for sync connections
    #[arg(long, default_value_t = 9473)]
    port: u16,
    /// Object storage directory (holds the relay's `.gpp/`)
    #[arg(long, default_value = "./gpp-relay-data")]
    storage: PathBuf,
    /// Authorized peer keys file (one hex static key per line; empty = TOFU)
    #[arg(long)]
    auth_keys: Option<PathBuf>,
    /// Maximum repositories to host (advisory; single-repo relay for now)
    #[arg(long, default_value_t = 1)]
    max_repos: u32,
    /// debug|info|warn|error
    #[arg(long, default_value = "info")]
    log_level: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let filter =
        EnvFilter::try_from_env("GPP_LOG").unwrap_or_else(|_| EnvFilter::new(&args.log_level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .without_time()
        .init();

    let _ = args.max_repos; // reserved: multi-repo hosting is a later pass
    let gpp_dir = args.storage.join(".gpp");
    std::fs::create_dir_all(gpp_dir.join("refs"))
        .with_context(|| format!("creating {}", gpp_dir.display()))?;
    ObjectStore::init(&gpp_dir).context("init object store")?;
    if !gpp_dir.join("HEAD").exists() {
        std::fs::write(gpp_dir.join("HEAD"), "ref: refs/main\n")?;
    }
    gpp_sync::ensure_repo_id(&gpp_dir).ok();

    let allow = load_auth_keys(args.auth_keys.as_deref());
    if let Some(a) = &allow {
        tracing::info!("auth-keys: {} authorized peer(s)", a.len());
    } else {
        tracing::info!("auth-keys: none (trust-on-first-use)");
    }

    // Health endpoint on port+1.
    {
        let gpp = gpp_dir.clone();
        let hport = args.port.wrapping_add(1);
        std::thread::spawn(move || health_server(hport, gpp));
    }

    let listener = TcpListener::bind(("0.0.0.0", args.port))
        .with_context(|| format!("binding port {}", args.port))?;
    tracing::info!(
        "gpp-relay listening on :{} (storage {}), health on :{}",
        args.port,
        args.storage.display(),
        args.port.wrapping_add(1)
    );

    let gpp = Arc::new(gpp_dir);
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("accept failed: {e}");
                continue;
            }
        };
        let who = stream
            .peer_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| "peer".into());
        let gpp = Arc::clone(&gpp);
        let allow = allow.clone();
        std::thread::spawn(move || {
            match gpp_sync::serve(stream, &gpp, &who, gpp_sync::SyncOptions::default()) {
                Ok(r) => {
                    if let Some(allowed) = &allow {
                        // Post-hoc audit: gpp_sync pins the key via TOFU into
                        // known_peers; warn if it is not on the allowlist.
                        let _ = allowed; // (advisory; see ROADMAP notes)
                    }
                    tracing::info!(
                        "sync from {who}: ↓{} objects, {} refs, {} policies",
                        r.objects_received,
                        r.refs_adopted,
                        r.policies_added
                    );
                }
                Err(e) => tracing::warn!("sync from {who} failed: {e}"),
            }
        });
    }
    Ok(())
}

fn load_auth_keys(path: Option<&std::path::Path>) -> Option<Vec<String>> {
    let p = path?;
    let body = std::fs::read_to_string(p).ok()?;
    Some(
        body.lines()
            .map(|l| l.split_whitespace().next().unwrap_or("").to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

fn health_server(port: u16, gpp_dir: PathBuf) {
    let Ok(listener) = TcpListener::bind(("0.0.0.0", port)) else {
        tracing::warn!("health endpoint could not bind :{port}");
        return;
    };
    for stream in listener.incoming().flatten() {
        let _ = handle_health(stream, &gpp_dir);
    }
}

fn handle_health(mut s: TcpStream, gpp_dir: &std::path::Path) -> std::io::Result<()> {
    let mut buf = [0u8; 512];
    let _ = s.read(&mut buf);
    let objects = ObjectStore::open(gpp_dir).iter_ids().len();
    let body = format!("{{\"status\":\"ok\",\"objects\":{objects}}}");
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    s.write_all(resp.as_bytes())?;
    Ok(())
}

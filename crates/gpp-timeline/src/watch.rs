//! Debounced filesystem watcher built on `notify`.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::{EventKind, RecursiveMode, Watcher};

use crate::error::{Error, Result};

/// Default debounce window (see `[timeline].debounce_ms`).
pub const DEFAULT_DEBOUNCE_MS: u64 = 100;

/// Watch `root` and invoke `on_quiet` after each burst of changes settles.
///
/// Blocks forever (until the process is interrupted). Events confined to
/// `.gpp/` are ignored so capture writes don't retrigger the watcher.
pub fn watch_loop<F>(root: &Path, debounce: Duration, mut on_quiet: F) -> Result<()>
where
    F: FnMut() -> Result<()>,
{
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .map_err(|e| Error::Watch(e.to_string()))?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| Error::Watch(e.to_string()))?;

    let gpp_dir = root.join(".gpp");
    let relevant = |paths: &[PathBuf]| paths.iter().any(|p| !p.starts_with(&gpp_dir));

    loop {
        // Block until the first relevant event.
        let first = match rx.recv() {
            Ok(Ok(ev)) => ev,
            Ok(Err(e)) => return Err(Error::Watch(e.to_string())),
            Err(_) => return Ok(()), // watcher dropped
        };
        if matches!(first.kind, EventKind::Access(_)) || !relevant(&first.paths) {
            continue;
        }
        // Coalesce: keep draining until the channel is quiet for `debounce`.
        loop {
            match rx.recv_timeout(debounce) {
                Ok(_) => continue,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
        on_quiet()?;
    }
}

//! `gpp-sync` — CRDT-based, offline-first P2P synchronization (layer 8).
//!
//! Transport is TCP + Noise_XX (mutual static-key auth, TOFU). Peers exchange
//! a *state vector* (object id set, branch tips, policy set) then transfer
//! only what the other lacks:
//!
//! * **Objects** — content-addressed, add-only set; verified on receipt.
//! * **Refs** — adopt missing branches; divergent same-name branches are
//!   preserved as `name@peer` forks (no silent merge — see `gpp merge`).
//! * **Policies** — add-only union by name.
//! * **Graphex** — optional zero-knowledge index sync (encrypted blobs ride
//!   the object set; metadata merges OR-Set / LWW).
//!
//! Trust and timeline are **never** synced (per `docs/SYNC_PROTOCOL.md`).
#![forbid(unsafe_code)]

mod error;
mod transport;

use std::collections::BTreeMap;
use std::net::TcpStream;
use std::path::{Path, PathBuf};

use gpp_core::{Hash, ObjectStore};
use gpp_graphex::{EdgeIndexRow, GraphStore, NodeIndexRow};
use gpp_history::RefStore;
use serde::{Deserialize, Serialize};

pub use error::{Error, Result};
pub use transport::{Transport, generate_keypair};

const PROTOCOL_VERSION: u32 = 1;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn unhex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return Err(Error::Protocol("odd hex".into()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| Error::Protocol(e.to_string())))
        .collect()
}

// ---- sync identity / repo id / TOFU ---------------------------------------

fn sync_dir(gpp: &Path) -> PathBuf {
    gpp.join("sync")
}

/// Persistent Noise static keypair for this repo: `(private, public)`.
pub fn ensure_identity(gpp: &Path) -> Result<(Vec<u8>, Vec<u8>)> {
    let dir = sync_dir(gpp);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("identity");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let mut it = s.split_whitespace();
        if let (Some(p), Some(q)) = (it.next(), it.next()) {
            return Ok((unhex(p)?, unhex(q)?));
        }
    }
    let (priv_, pub_) = generate_keypair()?;
    std::fs::write(&path, format!("{} {}", hex(&priv_), hex(&pub_)))?;
    Ok((priv_, pub_))
}

/// Stable repository id (shared by all replicas). Created on first use;
/// [`set_repo_id`] lets a fresh clone adopt a peer's id.
pub fn ensure_repo_id(gpp: &Path) -> Result<String> {
    let dir = sync_dir(gpp);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("repo_id");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let t = s.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    let (_, pubkey) = generate_keypair()?;
    let id = Hash::of(&pubkey).to_base32();
    std::fs::write(&path, &id)?;
    Ok(id)
}

pub fn set_repo_id(gpp: &Path, id: &str) -> Result<()> {
    let dir = sync_dir(gpp);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("repo_id"), id)?;
    Ok(())
}

/// Trust-on-first-use check for a peer's static key.
fn tofu(gpp: &Path, peer_name: &str, remote_static: &[u8]) -> Result<()> {
    let path = sync_dir(gpp).join("known_peers");
    let key = hex(remote_static);
    let body = std::fs::read_to_string(&path).unwrap_or_default();
    for line in body.lines() {
        let mut it = line.split_whitespace();
        if let (Some(n), Some(k)) = (it.next(), it.next())
            && n == peer_name
        {
            if k == key {
                return Ok(());
            }
            return Err(Error::PeerKeyChanged(peer_name.to_string()));
        }
    }
    let mut f = body;
    f.push_str(&format!("{peer_name} {key}\n"));
    std::fs::write(&path, f)?;
    Ok(())
}

// ---- wire messages ---------------------------------------------------------

#[derive(Serialize, Deserialize)]
enum Msg {
    Hello {
        repo_id: String,
        version: u32,
    },
    State {
        object_ids: Vec<String>,
        branch_tips: BTreeMap<String, String>,
        policies: Vec<String>,
    },
    Push {
        objects: Vec<(String, String)>, // (id, hex frame)
        refs: BTreeMap<String, String>,
        policies: Vec<(String, String)>, // (name, contents)
        nodes: Vec<NodeIndexRow>,
        edges: Vec<EdgeIndexRow>,
    },
    Done,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub objects_received: usize,
    pub refs_adopted: usize,
    pub forks_created: usize,
    pub policies_added: usize,
    pub graph_rows_merged: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SyncOptions {
    pub graph_only: bool,
    pub include_graphex: bool,
}

fn send(t: &mut Transport, m: &Msg) -> Result<()> {
    t.send(&serde_json::to_vec(m)?)
}
fn recv(t: &mut Transport) -> Result<Msg> {
    Ok(serde_json::from_slice(&t.recv()?)?)
}

// ---- local state helpers ---------------------------------------------------

fn policies_dir(gpp: &Path) -> PathBuf {
    gpp.join("policies")
}

fn local_policies(gpp: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Ok(rd) = std::fs::read_dir(policies_dir(gpp)) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "policy")
                && let (Some(stem), Ok(body)) = (
                    p.file_stem().map(|s| s.to_string_lossy().into_owned()),
                    std::fs::read_to_string(&p),
                )
            {
                out.insert(stem, body);
            }
        }
    }
    out
}

fn build_state(gpp: &Path, opts: SyncOptions) -> Result<Msg> {
    let store = ObjectStore::open(gpp);
    let object_ids = if opts.graph_only {
        Vec::new()
    } else {
        store.iter_ids().iter().map(|h| h.to_base32()).collect()
    };
    let refs = RefStore::open(gpp);
    let mut branch_tips = BTreeMap::new();
    for b in refs.list().map_err(|e| Error::Other(e.to_string()))? {
        if let Some(tip) = b.tip {
            branch_tips.insert(b.name, tip.to_base32());
        }
    }
    Ok(Msg::State {
        object_ids,
        branch_tips,
        policies: local_policies(gpp).into_keys().collect(),
    })
}

/// Compute the Push payload this side owes a peer with the given state.
fn build_push(gpp: &Path, remote: &Msg, opts: SyncOptions) -> Result<Msg> {
    let Msg::State {
        object_ids: their_objs,
        policies: their_pol,
        ..
    } = remote
    else {
        return Err(Error::Protocol("expected State".into()));
    };
    let store = ObjectStore::open(gpp);
    let theirs: std::collections::BTreeSet<&String> = their_objs.iter().collect();

    let mut objects = Vec::new();
    if !opts.graph_only {
        for h in store.iter_ids() {
            let b32 = h.to_base32();
            if !theirs.contains(&b32) {
                objects.push((b32, hex(&store.read_raw(&h)?)));
            }
        }
    }

    let refs = RefStore::open(gpp);
    let mut ref_map = BTreeMap::new();
    for b in refs.list().map_err(|e| Error::Other(e.to_string()))? {
        if let Some(tip) = b.tip {
            ref_map.insert(b.name, tip.to_base32());
        }
    }

    let local_pol = local_policies(gpp);
    let their_set: std::collections::BTreeSet<&String> = their_pol.iter().collect();
    let policies: Vec<(String, String)> = local_pol
        .into_iter()
        .filter(|(n, _)| !their_set.contains(n))
        .collect();

    let (nodes, edges) = if opts.graph_only || opts.include_graphex {
        match GraphStore::open(gpp) {
            Ok(g) => g.export_index().map_err(|e| Error::Other(e.to_string()))?,
            Err(_) => (Vec::new(), Vec::new()),
        }
    } else {
        (Vec::new(), Vec::new())
    };

    Ok(Msg::Push {
        objects,
        refs: ref_map,
        policies,
        nodes,
        edges,
    })
}

/// Apply a received Push to the local repo.
fn apply_push(gpp: &Path, peer_name: &str, m: Msg, rep: &mut SyncReport) -> Result<()> {
    let Msg::Push {
        objects,
        refs,
        policies,
        nodes,
        edges,
    } = m
    else {
        return Err(Error::Protocol("expected Push".into()));
    };

    let store = ObjectStore::open(gpp);
    for (id, frame) in objects {
        let h = Hash::from_base32(&id).map_err(|e| Error::Protocol(e.to_string()))?;
        store.write_raw(&h, &unhex(&frame)?)?;
        rep.objects_received += 1;
    }

    let refstore = RefStore::open(gpp);
    for (name, tip_s) in refs {
        let tip = Hash::from_base32(&tip_s).map_err(|e| Error::Protocol(e.to_string()))?;
        match refstore
            .read_ref(&name)
            .map_err(|e| Error::Other(e.to_string()))?
        {
            None => {
                refstore
                    .write_ref(&name, tip)
                    .map_err(|e| Error::Other(e.to_string()))?;
                rep.refs_adopted += 1;
            }
            Some(local) if local == tip => {}
            Some(_) => {
                // Divergent: preserve the peer's as a fork; never auto-merge.
                // (`@` is not a valid ref char, so use a dotted form.)
                let fork = format!("{name}.fork.{peer_name}");
                refstore
                    .write_ref(&fork, tip)
                    .map_err(|e| Error::Other(e.to_string()))?;
                rep.forks_created += 1;
            }
        }
    }

    let pdir = policies_dir(gpp);
    for (name, body) in policies {
        let dest = pdir.join(format!("{name}.policy"));
        if !dest.exists() {
            std::fs::create_dir_all(&pdir)?;
            std::fs::write(dest, body)?;
            rep.policies_added += 1;
        }
    }

    if (!nodes.is_empty() || !edges.is_empty())
        && let Ok(g) = GraphStore::open(gpp)
    {
        rep.graph_rows_merged += g
            .import_index(&nodes, &edges)
            .map_err(|e| Error::Other(e.to_string()))?;
    }
    Ok(())
}

fn handshake_hello(t: &mut Transport, gpp: &Path, initiator: bool) -> Result<()> {
    let repo_id = ensure_repo_id(gpp)?;
    let hello = Msg::Hello {
        repo_id: repo_id.clone(),
        version: PROTOCOL_VERSION,
    };
    let theirs = if initiator {
        send(t, &hello)?;
        recv(t)?
    } else {
        let r = recv(t)?;
        send(t, &hello)?;
        r
    };
    match theirs {
        Msg::Hello { repo_id: tid, .. } if tid == repo_id => Ok(()),
        Msg::Hello { repo_id: tid, .. } => Err(Error::RepoMismatch {
            local: repo_id,
            remote: tid,
        }),
        _ => Err(Error::Protocol("expected Hello".into())),
    }
}

/// Drive a sync as the connecting peer.
pub fn connect(addr: &str, gpp: &Path, peer_name: &str, opts: SyncOptions) -> Result<SyncReport> {
    let (priv_, _) = ensure_identity(gpp)?;
    let stream = TcpStream::connect(addr)?;
    let mut t = Transport::initiate(stream, &priv_)?;
    tofu(gpp, peer_name, &t.remote_static)?;
    handshake_hello(&mut t, gpp, true)?;

    let mut rep = SyncReport::default();
    send(&mut t, &build_state(gpp, opts)?)?;
    let remote_state = recv(&mut t)?;

    // Push phase, then pull phase.
    send(&mut t, &build_push(gpp, &remote_state, opts)?)?;
    send(&mut t, &Msg::Done)?;
    loop {
        match recv(&mut t)? {
            Msg::Done => break,
            m => apply_push(gpp, peer_name, m, &mut rep)?,
        }
    }
    Ok(rep)
}

/// Handle one inbound sync on an accepted stream (responder role).
/// Trust-on-first-use only; see [`serve_with_auth`] to additionally gate on an
/// authorized-keys allowlist.
pub fn serve(
    stream: TcpStream,
    gpp: &Path,
    peer_name: &str,
    opts: SyncOptions,
) -> Result<SyncReport> {
    serve_with_auth(stream, gpp, peer_name, opts, None)
}

/// Like [`serve`], but reject any peer whose Noise static key is not in
/// `allow` *immediately after the handshake* — before the repo-id exchange,
/// TOFU pinning, or any object data. `None` disables the check (TOFU only).
///
/// Noise XX only reveals the initiator's static key once the handshake
/// completes, so this is the earliest point the key is known; rejecting here
/// means an unauthorized peer transfers no application data. Keys are compared
/// in the lowercase-hex form written to `known_peers`, case-insensitively.
pub fn serve_with_auth(
    stream: TcpStream,
    gpp: &Path,
    peer_name: &str,
    opts: SyncOptions,
    allow: Option<&[String]>,
) -> Result<SyncReport> {
    let (priv_, _) = ensure_identity(gpp)?;
    let mut t = Transport::respond(stream, &priv_)?;
    if let Some(allow) = allow {
        let key = hex(&t.remote_static);
        if !allow.iter().any(|k| k.trim().eq_ignore_ascii_case(&key)) {
            return Err(Error::Unauthorized(key));
        }
    }
    tofu(gpp, peer_name, &t.remote_static)?;
    handshake_hello(&mut t, gpp, false)?;

    let mut rep = SyncReport::default();
    let remote_state = recv(&mut t)?;
    send(&mut t, &build_state(gpp, opts)?)?;

    // Pull phase first (mirror of the initiator), then push.
    loop {
        match recv(&mut t)? {
            Msg::Done => break,
            m => apply_push(gpp, peer_name, m, &mut rep)?,
        }
    }
    send(&mut t, &build_push(gpp, &remote_state, opts)?)?;
    send(&mut t, &Msg::Done)?;
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn init_repo(root: &Path) -> PathBuf {
        let gpp = root.join(".gpp");
        std::fs::create_dir_all(gpp.join("refs")).unwrap();
        ObjectStore::init(&gpp).unwrap();
        std::fs::write(gpp.join("HEAD"), "ref: refs/main\n").unwrap();
        gpp
    }

    #[test]
    fn converges_objects_refs_and_policies() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let ga = init_repo(a.path());
        let gb = init_repo(b.path());

        let id = ensure_repo_id(&ga).unwrap();
        set_repo_id(&gb, &id).unwrap();

        let sa = ObjectStore::open(&ga);
        let oid = sa
            .write(&gpp_core::Blob::new(b"hello sync".to_vec()))
            .unwrap();
        RefStore::open(&ga).write_ref("main", oid).unwrap();
        std::fs::create_dir_all(ga.join("policies")).unwrap();
        std::fs::write(ga.join("policies/secrets.policy"), "name=\"secrets\"\n").unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let gb2 = gb.clone();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            serve(s, &gb2, "peerA", SyncOptions::default()).unwrap()
        });

        let rep = connect(&addr, &ga, "peerB", SyncOptions::default()).unwrap();
        let srv = server.join().unwrap();

        assert_eq!(srv.objects_received, 1);
        assert_eq!(srv.refs_adopted, 1);
        assert_eq!(srv.policies_added, 1);
        let sb = ObjectStore::open(&gb);
        assert_eq!(
            sb.read::<gpp_core::Blob>(&oid).unwrap().content,
            b"hello sync"
        );
        assert_eq!(RefStore::open(&gb).read_ref("main").unwrap(), Some(oid));
        assert!(gb.join("policies/secrets.policy").exists());
        assert_eq!(rep.objects_received, 0);
    }

    #[test]
    fn divergent_branch_becomes_a_fork() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let ga = init_repo(a.path());
        let gb = init_repo(b.path());
        let id = ensure_repo_id(&ga).unwrap();
        set_repo_id(&gb, &id).unwrap();

        let oa = ObjectStore::open(&ga)
            .write(&gpp_core::Blob::new(b"A tip".to_vec()))
            .unwrap();
        let ob = ObjectStore::open(&gb)
            .write(&gpp_core::Blob::new(b"B tip".to_vec()))
            .unwrap();
        RefStore::open(&ga).write_ref("main", oa).unwrap();
        RefStore::open(&gb).write_ref("main", ob).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let gb2 = gb.clone();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            serve(s, &gb2, "A", SyncOptions::default()).unwrap()
        });
        connect(&addr, &ga, "B", SyncOptions::default()).unwrap();
        server.join().unwrap();

        let rb = RefStore::open(&gb);
        assert_eq!(rb.read_ref("main").unwrap(), Some(ob));
        assert_eq!(rb.read_ref("main.fork.A").unwrap(), Some(oa));
    }

    #[test]
    fn unauthorized_static_key_is_rejected_before_data() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let ga = init_repo(a.path());
        let gb = init_repo(b.path());
        let id = ensure_repo_id(&ga).unwrap();
        set_repo_id(&gb, &id).unwrap();

        // Give A a tip so that, if the gate failed open, objects would flow.
        let oa = ObjectStore::open(&ga)
            .write(&gpp_core::Blob::new(b"secret".to_vec()))
            .unwrap();
        RefStore::open(&ga).write_ref("main", oa).unwrap();

        // Allowlist contains some other key, never A's.
        let allow = vec!["00".repeat(32)];
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let gb2 = gb.clone();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            serve_with_auth(s, &gb2, "A", SyncOptions::default(), Some(&allow))
        });
        let r = connect(&addr, &ga, "B", SyncOptions::default());
        let srv = server.join().unwrap();

        assert!(matches!(srv, Err(Error::Unauthorized(_))));
        assert!(
            r.is_err(),
            "client should not complete against a rejecting server"
        );
        // Nothing was received: the rejection precedes the repo-id hello.
        assert!(ObjectStore::open(&gb).read::<gpp_core::Blob>(&oa).is_err());
        assert!(RefStore::open(&gb).read_ref("main").unwrap().is_none());
    }

    #[test]
    fn authorized_static_key_is_accepted() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let ga = init_repo(a.path());
        let gb = init_repo(b.path());
        let id = ensure_repo_id(&ga).unwrap();
        set_repo_id(&gb, &id).unwrap();

        let oa = ObjectStore::open(&ga)
            .write(&gpp_core::Blob::new(b"hello".to_vec()))
            .unwrap();
        RefStore::open(&ga).write_ref("main", oa).unwrap();

        // A's public static key, in the same hex form `known_peers` uses.
        let (_, pub_a) = ensure_identity(&ga).unwrap();
        let allow = vec![hex(&pub_a)];

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let gb2 = gb.clone();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            serve_with_auth(s, &gb2, "A", SyncOptions::default(), Some(&allow)).unwrap()
        });
        connect(&addr, &ga, "B", SyncOptions::default()).unwrap();
        let srv = server.join().unwrap();

        assert_eq!(srv.objects_received, 1);
        assert_eq!(RefStore::open(&gb).read_ref("main").unwrap(), Some(oa));
    }

    #[test]
    fn repo_id_mismatch_is_rejected() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let ga = init_repo(a.path());
        let gb = init_repo(b.path());
        ensure_repo_id(&ga).unwrap();
        ensure_repo_id(&gb).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let gb2 = gb.clone();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            serve(s, &gb2, "A", SyncOptions::default())
        });
        let r = connect(&addr, &ga, "B", SyncOptions::default());
        let _ = server.join().unwrap();
        assert!(matches!(r, Err(Error::RepoMismatch { .. })));
    }
}

//! `gpp-relay` — always-on sync hub (layer 13).
//!
//! A relay is **not** a server with authority: it is a persistent peer that
//! stores encrypted objects and forwards them. It never decrypts anything
//! (Graphex is always encrypted; code blobs may be too). The binary
//! (`src/main.rs`) wraps [`gpp_sync::serve`] in an accept loop plus a tiny
//! health endpoint.
//!
//! See `docs/SECURITY_MODEL.md` (§ Relay), `docs/ROADMAP.md` (Phase 7).
#![forbid(unsafe_code)]

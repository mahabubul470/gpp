//! `gpp-core` — content-addressed, encrypted-capable object store.
//!
//! This is layer 1 (Storage) of gpp. Phase 0 implements the unencrypted,
//! zstd-compressed object store with [`Blob`] and [`Tree`] objects, atomic
//! idempotent writes, and hash-verified reads.
//!
//! See `docs/ARCHITECTURE.md` and `docs/DATA_MODEL.md`.
#![forbid(unsafe_code)]

mod error;
mod hash;
mod object;
mod store;
mod wire;

pub use error::{Error, Result};
pub use hash::{HASH_LEN, HASH_STR_LEN, Hash, SHORT_LEN};
pub use object::{Blob, EntryKind, Object, ObjectType, Tree, TreeEntry};
pub use store::{ObjectStore, flatten_tree};
pub use wire::{VERSION as WIRE_VERSION, flags as wire_flags};

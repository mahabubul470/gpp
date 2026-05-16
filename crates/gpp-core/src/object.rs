//! Core object types: [`Blob`] and [`Tree`].
//!
//! Every object has a canonical byte encoding (`encode_body`). Its content
//! address is `BLAKE3(encode_body())`. For a [`Blob`] the body is the raw
//! file content, so the id is `blake3(content)` exactly as `docs/DATA_MODEL.md`
//! specifies. For a [`Tree`] the body is MessagePack of the entry list with
//! entries sorted by name, making the id deterministic regardless of insertion
//! order.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::hash::Hash;

/// On-the-wire object type code (stored in the frame header).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    Blob,
    Tree,
}

impl ObjectType {
    /// Numeric code written into the frame header.
    pub fn code(self) -> u8 {
        match self {
            ObjectType::Blob => 1,
            ObjectType::Tree => 2,
        }
    }

    /// Parse a code byte from a frame header.
    pub fn from_code(code: u8) -> Result<Self> {
        match code {
            1 => Ok(ObjectType::Blob),
            2 => Ok(ObjectType::Tree),
            other => Err(Error::UnknownObjectType(other)),
        }
    }
}

/// A storable, content-addressed object.
pub trait Object: Sized {
    /// This object's wire type.
    const TYPE: ObjectType;

    /// Canonical byte encoding that is hashed and (after compression) stored.
    fn encode_body(&self) -> Result<Vec<u8>>;

    /// Reconstruct an object from its canonical body bytes.
    fn decode_body(bytes: &[u8]) -> Result<Self>;

    /// Content address of this object: `BLAKE3(encode_body())`.
    fn id(&self) -> Result<Hash> {
        Ok(Hash::of(&self.encode_body()?))
    }
}

/// Raw file content. Equivalent to Git's blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob {
    pub content: Vec<u8>,
}

impl Blob {
    pub fn new(content: impl Into<Vec<u8>>) -> Self {
        Self {
            content: content.into(),
        }
    }
}

impl Object for Blob {
    const TYPE: ObjectType = ObjectType::Blob;

    fn encode_body(&self) -> Result<Vec<u8>> {
        Ok(self.content.clone())
    }

    fn decode_body(bytes: &[u8]) -> Result<Self> {
        Ok(Blob {
            content: bytes.to_vec(),
        })
    }
}

/// Kind of filesystem entry a [`TreeEntry`] points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
}

/// One entry in a [`Tree`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntry {
    pub name: String,
    pub kind: EntryKind,
    pub hash: Hash,
    pub mode: u32,
    pub size: u64,
}

/// A directory listing. Each entry points to a [`Blob`] or another [`Tree`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tree {
    pub entries: Vec<TreeEntry>,
}

impl Tree {
    pub fn new(entries: Vec<TreeEntry>) -> Self {
        Self { entries }
    }

    /// Entries sorted by name — the canonical ordering used for hashing.
    fn canonical_entries(&self) -> Vec<TreeEntry> {
        let mut e = self.entries.clone();
        e.sort_by(|a, b| a.name.cmp(&b.name));
        e
    }
}

impl Object for Tree {
    const TYPE: ObjectType = ObjectType::Tree;

    fn encode_body(&self) -> Result<Vec<u8>> {
        rmp_serde::to_vec(&self.canonical_entries()).map_err(|e| Error::Serialize(e.to_string()))
    }

    fn decode_body(bytes: &[u8]) -> Result<Self> {
        let entries: Vec<TreeEntry> =
            rmp_serde::from_slice(bytes).map_err(|e| Error::Deserialize(e.to_string()))?;
        Ok(Tree { entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_id_is_blake3_of_content() {
        let b = Blob::new(b"hello".to_vec());
        assert_eq!(b.id().unwrap(), Hash::of(b"hello"));
    }

    #[test]
    fn blob_body_roundtrips() {
        let b = Blob::new(b"some bytes".to_vec());
        let body = b.encode_body().unwrap();
        assert_eq!(Blob::decode_body(&body).unwrap(), b);
    }

    #[test]
    fn tree_id_is_order_independent() {
        let e1 = TreeEntry {
            name: "a.txt".into(),
            kind: EntryKind::File,
            hash: Hash::of(b"a"),
            mode: 0o644,
            size: 1,
        };
        let e2 = TreeEntry {
            name: "b.txt".into(),
            kind: EntryKind::File,
            hash: Hash::of(b"b"),
            mode: 0o644,
            size: 1,
        };
        let t1 = Tree::new(vec![e1.clone(), e2.clone()]);
        let t2 = Tree::new(vec![e2, e1]);
        assert_eq!(t1.id().unwrap(), t2.id().unwrap());
    }

    #[test]
    fn tree_body_roundtrips() {
        let t = Tree::new(vec![TreeEntry {
            name: "src".into(),
            kind: EntryKind::Directory,
            hash: Hash::of(b"tree"),
            mode: 0o755,
            size: 0,
        }]);
        let body = t.encode_body().unwrap();
        assert_eq!(Tree::decode_body(&body).unwrap().entries.len(), 1);
    }

    #[test]
    fn object_type_codes_roundtrip() {
        for ty in [ObjectType::Blob, ObjectType::Tree] {
            assert_eq!(ObjectType::from_code(ty.code()).unwrap(), ty);
        }
        assert!(ObjectType::from_code(99).is_err());
    }
}

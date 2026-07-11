//! The content-addressed object store: `.gpp/objects/`.
//!
//! Objects are stored at `objects/<aa>/<rest>` where `<aa>` is the first two
//! characters of the base32 id and `<rest>` is the remaining 50. Writes are
//! atomic (temp file + rename) and idempotent (an id that already exists is a
//! no-op). Reads verify the BLAKE3 content address before returning.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::hash::Hash;
use crate::object::Object;
use crate::wire;

/// Handle to a repository's object store.
#[derive(Debug, Clone)]
pub struct ObjectStore {
    objects_dir: PathBuf,
}

impl ObjectStore {
    /// Open the store rooted at `<gpp_dir>/objects` (does not create it).
    pub fn open(gpp_dir: &Path) -> Self {
        Self {
            objects_dir: gpp_dir.join("objects"),
        }
    }

    /// Create the `objects/` directory and return a handle.
    pub fn init(gpp_dir: &Path) -> Result<Self> {
        let store = Self::open(gpp_dir);
        fs::create_dir_all(&store.objects_dir)?;
        Ok(store)
    }

    /// Filesystem path an object id maps to.
    fn path_for(&self, id: &Hash) -> PathBuf {
        let s = id.to_base32();
        let (shard, rest) = s.split_at(2);
        self.objects_dir.join(shard).join(rest)
    }

    /// True if an object with this id is present.
    pub fn contains(&self, id: &Hash) -> bool {
        self.path_for(id).exists()
    }

    /// Store an object, returning its content address. Idempotent.
    pub fn write<T: Object>(&self, object: &T) -> Result<Hash> {
        let body = object.encode_body()?;
        let id = Hash::of(&body);
        let path = self.path_for(&id);
        if path.exists() {
            tracing::trace!(%id, "object already present, skipping write");
            return Ok(id);
        }

        let frame = wire::encode(T::TYPE, &body)?;
        let dir = path
            .parent()
            .expect("object path always has a shard parent");
        fs::create_dir_all(dir)?;

        // Atomic publish: write to a uniquely-named temp file, then rename.
        let tmp = dir.join(format!(".tmp-{}", id.short()));
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&frame)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        tracing::debug!(%id, bytes = frame.len(), "wrote object");
        Ok(id)
    }

    /// Raw stored frame bytes for an object (for sync transfer). The object
    /// is *not* decoded — callers move opaque, content-addressed frames.
    pub fn read_raw(&self, id: &Hash) -> Result<Vec<u8>> {
        match fs::read(self.path_for(id)) {
            Ok(b) => Ok(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Error::NotFound(*id)),
            Err(e) => Err(Error::Io(e)),
        }
    }

    /// Write a raw frame received from a peer. The frame is decoded and its
    /// BLAKE3 body hash must equal `id`, so a malicious peer cannot inject
    /// content under the wrong address. Idempotent.
    pub fn write_raw(&self, id: &Hash, frame: &[u8]) -> Result<()> {
        let decoded = wire::decode(frame)?;
        let computed = Hash::of(&decoded.body);
        if computed != *id {
            return Err(Error::HashMismatch {
                expected: *id,
                computed,
            });
        }
        let path = self.path_for(id);
        if path.exists() {
            return Ok(());
        }
        let dir = path
            .parent()
            .expect("object path always has a shard parent");
        fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(".tmp-{}", id.short()));
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(frame)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Enumerate every stored object id (skips `.tmp-` files).
    pub fn iter_ids(&self) -> Vec<Hash> {
        let mut out = Vec::new();
        let Ok(shards) = fs::read_dir(&self.objects_dir) else {
            return out;
        };
        for shard in shards.flatten() {
            if !shard.path().is_dir() {
                continue;
            }
            let shard_name = shard.file_name().to_string_lossy().into_owned();
            let Ok(rest) = fs::read_dir(shard.path()) else {
                continue;
            };
            for ent in rest.flatten() {
                let fname = ent.file_name().to_string_lossy().into_owned();
                if fname.starts_with(".tmp-") {
                    continue;
                }
                if let Ok(h) = Hash::from_base32(&format!("{shard_name}{fname}")) {
                    out.push(h);
                }
            }
        }
        out
    }

    /// Read an object by id, verifying its type and content address.
    pub fn read<T: Object>(&self, id: &Hash) -> Result<T> {
        let path = self.path_for(id);
        let frame = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::NotFound(*id));
            }
            Err(e) => return Err(Error::Io(e)),
        };

        let decoded = wire::decode(&frame)?;
        if decoded.object_type != T::TYPE {
            return Err(Error::TypeMismatch {
                expected: T::TYPE.code(),
                found: decoded.object_type.code(),
            });
        }

        let computed = Hash::of(&decoded.body);
        if computed != *id {
            return Err(Error::HashMismatch {
                expected: *id,
                computed,
            });
        }
        T::decode_body(&decoded.body)
    }
}

/// Recursively flatten a stored [`Tree`](crate::Tree) into `path -> blob hash`
/// (files and symlinks; directories are descended into).
pub fn flatten_tree(
    store: &ObjectStore,
    root: &Hash,
) -> Result<std::collections::BTreeMap<String, Hash>> {
    fn walk(
        store: &ObjectStore,
        tree_hash: &Hash,
        prefix: &str,
        out: &mut std::collections::BTreeMap<String, Hash>,
    ) -> Result<()> {
        let tree: crate::Tree = store.read(tree_hash)?;
        for e in tree.entries {
            let path = if prefix.is_empty() {
                e.name.clone()
            } else {
                format!("{prefix}/{}", e.name)
            };
            match e.kind {
                crate::EntryKind::Directory => walk(store, &e.hash, &path, out)?,
                crate::EntryKind::File | crate::EntryKind::Symlink => {
                    out.insert(path, e.hash);
                }
            }
        }
        Ok(())
    }
    let mut out = std::collections::BTreeMap::new();
    walk(store, root, "", &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Blob, EntryKind, Tree, TreeEntry};

    fn store() -> (tempfile::TempDir, ObjectStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::init(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn write_then_read_blob() {
        let (_d, s) = store();
        let blob = Blob::new(b"content addressed!".to_vec());
        let id = s.write(&blob).unwrap();
        assert!(s.contains(&id));
        assert_eq!(s.read::<Blob>(&id).unwrap(), blob);
    }

    #[test]
    fn write_is_idempotent() {
        let (_d, s) = store();
        let blob = Blob::new(b"dup".to_vec());
        assert_eq!(s.write(&blob).unwrap(), s.write(&blob).unwrap());
    }

    #[test]
    fn write_then_read_tree() {
        let (_d, s) = store();
        let blob_id = s.write(&Blob::new(b"file".to_vec())).unwrap();
        let tree = Tree::new(vec![TreeEntry {
            name: "file.txt".into(),
            kind: EntryKind::File,
            hash: blob_id,
            mode: 0o644,
            size: 4,
        }]);
        let id = s.write(&tree).unwrap();
        assert_eq!(s.read::<Tree>(&id).unwrap(), tree);
    }

    #[test]
    fn missing_object_is_not_found() {
        let (_d, s) = store();
        let err = s.read::<Blob>(&Hash::of(b"nope")).unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
    }

    #[test]
    fn type_mismatch_is_rejected() {
        let (_d, s) = store();
        let id = s.write(&Blob::new(b"i am a blob".to_vec())).unwrap();
        assert!(matches!(
            s.read::<Tree>(&id).unwrap_err(),
            Error::TypeMismatch { .. }
        ));
    }

    #[test]
    fn raw_transfer_roundtrips_and_enumerates() {
        let (_d, src) = store();
        let id = src.write(&Blob::new(b"sync me".to_vec())).unwrap();
        let frame = src.read_raw(&id).unwrap();

        let dst_dir = tempfile::tempdir().unwrap();
        let dst = ObjectStore::init(dst_dir.path()).unwrap();
        dst.write_raw(&id, &frame).unwrap();
        assert_eq!(dst.read::<Blob>(&id).unwrap().content, b"sync me");

        let ids = dst.iter_ids();
        assert_eq!(ids, vec![id]);
        // Idempotent re-write is fine.
        dst.write_raw(&id, &frame).unwrap();
    }

    #[test]
    fn write_raw_rejects_wrong_address() {
        let (_d, s) = store();
        let frame = wire::encode(crate::object::ObjectType::Blob, b"real").unwrap();
        let wrong = Hash::of(b"not-the-body");
        assert!(matches!(
            s.write_raw(&wrong, &frame).unwrap_err(),
            Error::HashMismatch { .. }
        ));
    }

    #[test]
    fn corrupted_object_is_detected() {
        let (_d, s) = store();
        let id = s.write(&Blob::new(b"tamper me".to_vec())).unwrap();
        let path = s.path_for(&id);
        let mut bytes = fs::read(&path).unwrap();
        let n = bytes.len();
        bytes[n - 5] ^= 0xff;
        fs::write(&path, &bytes).unwrap();
        assert!(s.read::<Blob>(&id).is_err());
    }
}

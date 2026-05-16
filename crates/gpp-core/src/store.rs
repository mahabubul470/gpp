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

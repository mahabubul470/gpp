//! The Graphex key store (`.gpp/graphex/keys/`).
//!
//! Key hierarchy (`docs/GRAPHEX_PROTOCOL.md`):
//!
//! * `master.age` — the repo's `age` X25519 identity (root of trust).
//! * `public.key` — plaintext 32-byte key (public-tier content is not
//!   encrypted, only compressed; the key exists for a uniform API).
//! * `<tier>.age` — each non-public tier's 32-byte AES key, sealed to the
//!   master recipient.
//!
//! Simplification vs. the doc: `master.age` holds the identity directly
//! rather than being passphrase-wrapped, and `human-only` is master-sealed
//! like the other tiers (passphrase gating is a later hardening pass —
//! tracked in the ROADMAP Phase 3 notes).

use std::path::{Path, PathBuf};

use age::secrecy::ExposeSecret;

use crate::crypto::{age_decrypt, age_encrypt, new_symmetric_key};
use crate::error::{Error, Result};
use crate::object::AccessTier;

const TIERS: &[AccessTier] = &[
    AccessTier::Public,
    AccessTier::AgentReadable,
    AccessTier::AgentRestricted,
    AccessTier::HumanOnly,
];

pub struct KeyStore {
    dir: PathBuf,
    identity: String,
    recipient: String,
}

fn keys_dir(gpp_dir: &Path) -> PathBuf {
    gpp_dir.join("graphex").join("keys")
}

fn hex_encode(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hex_decode(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return Err(Error::Crypto("odd hex length".into()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| Error::Crypto(e.to_string())))
        .collect()
}

impl KeyStore {
    /// True if a key store has already been generated here.
    pub fn exists(gpp_dir: &Path) -> bool {
        keys_dir(gpp_dir).join("master.age").is_file()
    }

    /// Generate a fresh key hierarchy. Errors if one already exists.
    pub fn generate(gpp_dir: &Path) -> Result<KeyStore> {
        if Self::exists(gpp_dir) {
            return Err(Error::Other(
                "key store already exists (use `gpp keys rotate`)".into(),
            ));
        }
        let dir = keys_dir(gpp_dir);
        std::fs::create_dir_all(&dir)?;

        let id = age::x25519::Identity::generate();
        let identity = id.to_string().expose_secret().to_string();
        let recipient = id.to_public().to_string();
        std::fs::write(dir.join("master.age"), &identity)?;

        let ks = KeyStore {
            dir,
            identity,
            recipient,
        };
        for &tier in TIERS {
            ks.write_tier_key(tier, &new_symmetric_key()?)?;
        }
        Ok(ks)
    }

    /// Open an existing key store.
    pub fn open(gpp_dir: &Path) -> Result<KeyStore> {
        let dir = keys_dir(gpp_dir);
        let identity = std::fs::read_to_string(dir.join("master.age"))
            .map_err(|_| Error::NoKeys)?
            .trim()
            .to_string();
        let id: age::x25519::Identity = identity
            .parse()
            .map_err(|e| Error::Crypto(format!("bad master identity: {e}")))?;
        let recipient = id.to_public().to_string();
        Ok(KeyStore {
            dir,
            identity,
            recipient,
        })
    }

    fn tier_path(&self, tier: AccessTier) -> PathBuf {
        match tier {
            AccessTier::Public => self.dir.join("public.key"),
            t => self.dir.join(format!("{}.age", t.as_str())),
        }
    }

    fn write_tier_key(&self, tier: AccessTier, key: &[u8; 32]) -> Result<()> {
        let path = self.tier_path(tier);
        if tier == AccessTier::Public {
            std::fs::write(path, hex_encode(key))?;
        } else {
            std::fs::write(path, age_encrypt(&self.recipient, key)?)?;
        }
        Ok(())
    }

    /// The 32-byte symmetric key for a tier (decrypting via master if needed).
    pub fn tier_key(&self, tier: AccessTier) -> Result<[u8; 32]> {
        let path = self.tier_path(tier);
        let bytes = if tier == AccessTier::Public {
            hex_decode(std::fs::read_to_string(&path)?.trim())?
        } else {
            let ct = std::fs::read(&path)?;
            age_decrypt(&self.identity, &ct)?
        };
        bytes
            .try_into()
            .map_err(|_| Error::Crypto("tier key is not 32 bytes".into()))
    }

    /// Whether a tier's content is encrypted (everything except `public`).
    pub fn is_encrypted(tier: AccessTier) -> bool {
        tier != AccessTier::Public
    }

    pub fn master_recipient(&self) -> &str {
        &self.recipient
    }

    /// Regenerate every non-master tier key. Callers must re-encrypt existing
    /// content (see `GraphStore::rotate_keys`).
    pub fn regenerate_tier_keys(&self) -> Result<()> {
        for &tier in TIERS {
            self.write_tier_key(tier, &new_symmetric_key()?)?;
        }
        Ok(())
    }

    /// Which tiers currently have a key file.
    pub fn present_tiers(&self) -> Vec<AccessTier> {
        TIERS
            .iter()
            .copied()
            .filter(|&t| self.tier_path(t).is_file())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_open_and_use_keys() {
        let d = tempfile::tempdir().unwrap();
        let gpp = d.path().join(".gpp");
        std::fs::create_dir_all(&gpp).unwrap();

        let ks = KeyStore::generate(&gpp).unwrap();
        assert!(KeyStore::exists(&gpp));
        assert!(KeyStore::generate(&gpp).is_err()); // no clobber

        let reopened = KeyStore::open(&gpp).unwrap();
        for &t in TIERS {
            // Same key seen by the generating and the reopened store.
            assert_eq!(ks.tier_key(t).unwrap(), reopened.tier_key(t).unwrap());
        }
        assert_eq!(reopened.present_tiers().len(), 4);
    }

    #[test]
    fn rotation_changes_tier_keys() {
        let d = tempfile::tempdir().unwrap();
        let gpp = d.path().join(".gpp");
        std::fs::create_dir_all(&gpp).unwrap();
        let ks = KeyStore::generate(&gpp).unwrap();
        let before = ks.tier_key(AccessTier::AgentReadable).unwrap();
        ks.regenerate_tier_keys().unwrap();
        let after = ks.tier_key(AccessTier::AgentReadable).unwrap();
        assert_ne!(before, after);
    }
}

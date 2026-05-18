//! The Graphex key store (`.gpp/graphex/keys/`).
//!
//! Key hierarchy (`docs/GRAPHEX_PROTOCOL.md`):
//!
//! * `master.age` — the repo's `age` X25519 identity (root of trust). When
//!   a passphrase is configured it is **scrypt-passphrase-wrapped** at rest;
//!   otherwise the identity is stored directly (default/unattended mode).
//! * `public.key` — plaintext 32-byte key (public-tier content is not
//!   encrypted, only compressed; the key exists for a uniform API).
//! * `agent-readable.age`, `agent-restricted.age` — 32-byte AES keys sealed
//!   to the master recipient (the local runtime can decrypt with the master
//!   identity, so trusted agents read these without a human present).
//! * `human-only.age` — when a passphrase is configured, the human-only key
//!   is sealed **directly to the passphrase**, so it cannot be decrypted by
//!   the master identity alone (the documented human-only gate).
//!
//! The passphrase is taken from `$GPP_GRAPHEX_PASSPHRASE` (or passed
//! explicitly via [`KeyStore::generate_with`] / [`KeyStore::open_with`]).
//! With no passphrase the behaviour is exactly the legacy one, so existing
//! repositories keep working unchanged.

use std::path::{Path, PathBuf};

use age::secrecy::ExposeSecret;

use crate::crypto::{age_encrypt, age_open, new_symmetric_key, passphrase_encrypt};
use crate::error::{Error, Result};
use crate::object::AccessTier;

const TIERS: &[AccessTier] = &[
    AccessTier::Public,
    AccessTier::AgentReadable,
    AccessTier::AgentRestricted,
    AccessTier::HumanOnly,
];

/// Environment variable holding the optional key-store passphrase.
pub const PASSPHRASE_ENV: &str = "GPP_GRAPHEX_PASSPHRASE";

fn env_passphrase() -> Option<String> {
    std::env::var(PASSPHRASE_ENV).ok().filter(|s| !s.is_empty())
}

pub struct KeyStore {
    dir: PathBuf,
    identity: String,
    recipient: String,
    passphrase: Option<String>,
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

fn parses_as_identity(s: &str) -> bool {
    s.trim().parse::<age::x25519::Identity>().is_ok()
}

impl KeyStore {
    /// True if a key store has already been generated here.
    pub fn exists(gpp_dir: &Path) -> bool {
        keys_dir(gpp_dir).join("master.age").is_file()
    }

    /// True if the on-disk master key is passphrase-wrapped (binary `age`
    /// data rather than a plaintext identity string).
    pub fn is_passphrase_protected(gpp_dir: &Path) -> bool {
        match std::fs::read(keys_dir(gpp_dir).join("master.age")) {
            Ok(b) => !std::str::from_utf8(&b).is_ok_and(parses_as_identity),
            Err(_) => false,
        }
    }

    /// Generate a fresh hierarchy, taking the passphrase from the
    /// environment. Errors if one already exists.
    pub fn generate(gpp_dir: &Path) -> Result<KeyStore> {
        Self::generate_with(gpp_dir, env_passphrase().as_deref())
    }

    /// Generate, explicitly choosing whether to passphrase-protect.
    pub fn generate_with(gpp_dir: &Path, passphrase: Option<&str>) -> Result<KeyStore> {
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

        match passphrase {
            Some(p) => std::fs::write(
                dir.join("master.age"),
                passphrase_encrypt(p, identity.as_bytes())?,
            )?,
            None => std::fs::write(dir.join("master.age"), &identity)?,
        }

        let ks = KeyStore {
            dir,
            identity,
            recipient,
            passphrase: passphrase.map(str::to_owned),
        };
        for &tier in TIERS {
            ks.write_tier_key(tier, &new_symmetric_key()?)?;
        }
        Ok(ks)
    }

    /// Open an existing key store (passphrase from the environment).
    pub fn open(gpp_dir: &Path) -> Result<KeyStore> {
        Self::open_with(gpp_dir, env_passphrase().as_deref())
    }

    /// Open, explicitly supplying the passphrase (if any).
    pub fn open_with(gpp_dir: &Path, passphrase: Option<&str>) -> Result<KeyStore> {
        let dir = keys_dir(gpp_dir);
        let raw = std::fs::read(dir.join("master.age")).map_err(|_| Error::NoKeys)?;
        let identity = if let Ok(text) = std::str::from_utf8(&raw)
            && parses_as_identity(text)
        {
            text.trim().to_string()
        } else {
            // Passphrase-wrapped master.
            let pass = passphrase.ok_or(Error::PassphraseRequired)?;
            let id = age_open(None, Some(pass), &raw)?;
            String::from_utf8(id)
                .map_err(|_| Error::Crypto("master key is not valid UTF-8".into()))?
        };
        let parsed: age::x25519::Identity = identity
            .parse()
            .map_err(|e| Error::Crypto(format!("bad master identity: {e}")))?;
        Ok(KeyStore {
            dir,
            recipient: parsed.to_public().to_string(),
            identity,
            passphrase: passphrase.map(str::to_owned),
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
        } else if tier == AccessTier::HumanOnly
            && let Some(p) = &self.passphrase
        {
            // The documented human-only gate: sealed to the passphrase, so
            // the master identity alone cannot decrypt it.
            std::fs::write(path, passphrase_encrypt(p, key)?)?;
        } else {
            std::fs::write(path, age_encrypt(&self.recipient, key)?)?;
        }
        Ok(())
    }

    /// The 32-byte symmetric key for a tier. `human-only` additionally
    /// requires the passphrase when the store is passphrase-protected.
    pub fn tier_key(&self, tier: AccessTier) -> Result<[u8; 32]> {
        let path = self.tier_path(tier);
        let bytes = if tier == AccessTier::Public {
            hex_decode(std::fs::read_to_string(&path)?.trim())?
        } else {
            let ct = std::fs::read(&path)?;
            age_open(Some(&self.identity), self.passphrase.as_deref(), &ct)?
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

    /// Whether this open store is operating with a passphrase.
    pub fn passphrase_protected(&self) -> bool {
        self.passphrase.is_some()
    }

    /// Regenerate every non-master tier key (master identity unchanged).
    /// Callers must re-encrypt existing content (`GraphStore::rotate_keys`).
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
    fn generate_open_and_use_keys_legacy() {
        let d = tempfile::tempdir().unwrap();
        let gpp = d.path().join(".gpp");
        std::fs::create_dir_all(&gpp).unwrap();

        let ks = KeyStore::generate(&gpp).unwrap(); // no env → legacy
        assert!(KeyStore::exists(&gpp));
        assert!(!KeyStore::is_passphrase_protected(&gpp));
        assert!(KeyStore::generate(&gpp).is_err()); // no clobber

        let reopened = KeyStore::open(&gpp).unwrap();
        for &t in TIERS {
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

    #[test]
    fn passphrase_protected_store_roundtrips() {
        let d = tempfile::tempdir().unwrap();
        let gpp = d.path().join(".gpp");
        std::fs::create_dir_all(&gpp).unwrap();

        let ks = KeyStore::generate_with(&gpp, Some("s3cret")).unwrap();
        assert!(KeyStore::is_passphrase_protected(&gpp));

        // Wrong / missing passphrase cannot even open the master key.
        assert!(matches!(
            KeyStore::open_with(&gpp, None),
            Err(Error::PassphraseRequired)
        ));
        assert!(KeyStore::open_with(&gpp, Some("wrong")).is_err());

        let reopened = KeyStore::open_with(&gpp, Some("s3cret")).unwrap();
        assert!(reopened.passphrase_protected());
        // Agent-tier keys decrypt (master-sealed); human-only needs the pass.
        assert_eq!(
            ks.tier_key(AccessTier::AgentReadable).unwrap(),
            reopened.tier_key(AccessTier::AgentReadable).unwrap()
        );
        assert_eq!(
            ks.tier_key(AccessTier::HumanOnly).unwrap(),
            reopened.tier_key(AccessTier::HumanOnly).unwrap()
        );
    }

    #[test]
    fn human_only_is_passphrase_gated_not_master_readable() {
        let d = tempfile::tempdir().unwrap();
        let gpp = d.path().join(".gpp");
        std::fs::create_dir_all(&gpp).unwrap();
        KeyStore::generate_with(&gpp, Some("pw")).unwrap();

        // An attacker with the master identity but no passphrase: simulate
        // by opening with the right passphrase (so master loads) but then
        // checking the human-only blob is a passphrase envelope, unreadable
        // via the master identity alone.
        let ks = KeyStore::open_with(&gpp, Some("pw")).unwrap();
        let human_blob = std::fs::read(gpp.join("graphex/keys/human-only.age")).unwrap();
        // Master identity alone (no passphrase) must fail on human-only.
        assert!(age_open(Some(&ks.identity), None, &human_blob).is_err());
        // Agent-restricted, by contrast, IS master-readable.
        let ar = std::fs::read(gpp.join("graphex/keys/agent-restricted.age")).unwrap();
        assert!(age_open(Some(&ks.identity), None, &ar).is_ok());
    }
}

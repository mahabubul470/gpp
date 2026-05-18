//! Low-level primitives: `age` envelopes for keys, AES-256-GCM for nodes.
//!
//! Node content uses *envelope encryption*: each node's bytes are sealed with
//! its tier's symmetric key (AES-256-GCM); the tier keys are themselves sealed
//! to the repo master `age` identity. `public`-tier content is not encrypted
//! (only zstd-compressed), matching the storage table in the protocol doc.

use std::io::{Read, Write};

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};

use crate::error::{Error, Result};

const ENV_VERSION: u8 = 1;
const SCHEME_PLAIN_ZSTD: u8 = 0;
const SCHEME_AESGCM_ZSTD: u8 = 1;

fn rand_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).map_err(|e| Error::Crypto(format!("rng: {e}")))?;
    Ok(b)
}

/// A fresh 32-byte symmetric key.
pub fn new_symmetric_key() -> Result<[u8; 32]> {
    rand_bytes::<32>()
}

/// Seal node `plaintext` for `tier_key`. If `encrypt` is false (public tier)
/// the bytes are only zstd-compressed. Output is a self-describing envelope.
pub fn seal(plaintext: &[u8], tier_key: &[u8; 32], encrypt: bool) -> Result<Vec<u8>> {
    let compressed =
        zstd::encode_all(plaintext, 3).map_err(|e| Error::Crypto(format!("zstd: {e}")))?;
    let mut out = vec![ENV_VERSION];
    if !encrypt {
        out.push(SCHEME_PLAIN_ZSTD);
        out.extend_from_slice(&[0u8; 12]);
        out.extend_from_slice(&compressed);
        return Ok(out);
    }
    let nonce_bytes = rand_bytes::<12>()?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(tier_key));
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), compressed.as_ref())
        .map_err(|e| Error::Crypto(format!("aes-gcm seal: {e}")))?;
    out.push(SCHEME_AESGCM_ZSTD);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Reverse of [`seal`].
pub fn open(envelope: &[u8], tier_key: &[u8; 32]) -> Result<Vec<u8>> {
    if envelope.len() < 14 || envelope[0] != ENV_VERSION {
        return Err(Error::Crypto("bad envelope header".into()));
    }
    let scheme = envelope[1];
    let nonce = &envelope[2..14];
    let body = &envelope[14..];
    let compressed = match scheme {
        SCHEME_PLAIN_ZSTD => body.to_vec(),
        SCHEME_AESGCM_ZSTD => {
            let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(tier_key));
            cipher
                .decrypt(Nonce::from_slice(nonce), body)
                .map_err(|e| Error::Crypto(format!("aes-gcm open: {e}")))?
        }
        other => return Err(Error::Crypto(format!("unknown scheme {other}"))),
    };
    zstd::decode_all(&compressed[..]).map_err(|e| Error::Crypto(format!("unzstd: {e}")))
}

/// Encrypt `data` to an `age` X25519 recipient string (`age1…`).
pub fn age_encrypt(recipient: &str, data: &[u8]) -> Result<Vec<u8>> {
    let r: age::x25519::Recipient = recipient
        .parse()
        .map_err(|e| Error::Crypto(format!("bad recipient: {e}")))?;
    let enc = age::Encryptor::with_recipients(vec![Box::new(r)])
        .ok_or_else(|| Error::Crypto("no recipients".into()))?;
    let mut out = Vec::new();
    let mut w = enc
        .wrap_output(&mut out)
        .map_err(|e| Error::Crypto(format!("age wrap: {e}")))?;
    w.write_all(data)
        .map_err(|e| Error::Crypto(format!("age write: {e}")))?;
    w.finish()
        .map_err(|e| Error::Crypto(format!("age finish: {e}")))?;
    Ok(out)
}

/// Encrypt `data` with a passphrase (age scrypt recipient).
pub fn passphrase_encrypt(pass: &str, data: &[u8]) -> Result<Vec<u8>> {
    let enc = age::Encryptor::with_user_passphrase(age::secrecy::Secret::new(pass.to_owned()));
    let mut out = Vec::new();
    let mut w = enc
        .wrap_output(&mut out)
        .map_err(|e| Error::Crypto(format!("age wrap: {e}")))?;
    w.write_all(data)
        .map_err(|e| Error::Crypto(format!("age write: {e}")))?;
    w.finish()
        .map_err(|e| Error::Crypto(format!("age finish: {e}")))?;
    Ok(out)
}

/// Open an `age` blob that may be sealed *either* to an X25519 identity
/// (legacy / master-sealed) *or* to a passphrase (hardened). The envelope
/// header selects the path, so callers don't need to know which was used.
pub fn age_open(identity: Option<&str>, passphrase: Option<&str>, data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    match age::Decryptor::new(data).map_err(|e| Error::Crypto(format!("age: {e}")))? {
        age::Decryptor::Recipients(d) => {
            let id_str = identity.ok_or_else(|| {
                Error::Crypto("recipients envelope but no identity available".into())
            })?;
            let id: age::x25519::Identity = id_str
                .parse()
                .map_err(|e| Error::Crypto(format!("bad identity: {e}")))?;
            let mut r = d
                .decrypt(std::iter::once(&id as &dyn age::Identity))
                .map_err(|e| Error::Crypto(format!("age decrypt: {e}")))?;
            r.read_to_end(&mut out)
                .map_err(|e| Error::Crypto(format!("age read: {e}")))?;
        }
        age::Decryptor::Passphrase(d) => {
            let pass = passphrase.ok_or(Error::Crypto("passphrase required".into()))?;
            let mut r = d
                .decrypt(&age::secrecy::Secret::new(pass.to_owned()), None)
                .map_err(|e| Error::Crypto(format!("age passphrase decrypt: {e}")))?;
            r.read_to_end(&mut out)
                .map_err(|e| Error::Crypto(format!("age read: {e}")))?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passphrase_roundtrip_and_wrong_pass_fails() {
        let ct = passphrase_encrypt("correct horse", b"master identity").unwrap();
        assert_eq!(
            age_open(None, Some("correct horse"), &ct).unwrap(),
            b"master identity"
        );
        assert!(age_open(None, Some("wrong"), &ct).is_err());
        assert!(age_open(None, None, &ct).is_err());
    }

    #[test]
    fn age_open_handles_recipient_envelopes_too() {
        use age::secrecy::ExposeSecret;
        let id = age::x25519::Identity::generate();
        let ct = age_encrypt(&id.to_public().to_string(), b"tier key").unwrap();
        let pt = age_open(Some(id.to_string().expose_secret()), None, &ct).unwrap();
        assert_eq!(pt, b"tier key");
    }

    #[test]
    fn seal_open_roundtrip_encrypted() {
        let key = new_symmetric_key().unwrap();
        let env = seal(b"secret graph node", &key, true).unwrap();
        assert_ne!(&env[14..], b"secret graph node");
        assert_eq!(open(&env, &key).unwrap(), b"secret graph node");
    }

    #[test]
    fn seal_open_roundtrip_public() {
        let key = [0u8; 32];
        let env = seal(b"public info", &key, false).unwrap();
        assert_eq!(open(&env, &key).unwrap(), b"public info");
    }

    #[test]
    fn wrong_key_fails() {
        let k1 = new_symmetric_key().unwrap();
        let k2 = new_symmetric_key().unwrap();
        let env = seal(b"x", &k1, true).unwrap();
        assert!(open(&env, &k2).is_err());
    }
}

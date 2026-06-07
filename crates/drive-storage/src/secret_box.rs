//! AES-256-GCM envelope for bring-your-own storage secrets.
//! Spec: docs/research/08-byo-storage.md §"Crypto envelope".
//!
//! On-disk form is `BASE64(nonce || ciphertext || tag)` — one self-contained
//! string per secret. The AAD is `workspace_storage.id || ":" || key_version`,
//! so a ciphertext can't be swapped between rows or revived after rotation.
//!
//! We use AES-GCM from RustCrypto (audited; widely deployed). No homebrew.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};
use rand::RngCore;

/// 12 bytes — AES-GCM standard. 16-byte tag is appended by `encrypt`.
const NONCE_LEN: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum SecretBoxError {
    #[error("malformed envelope")]
    Malformed,
    #[error("seal failed")]
    Seal,
    #[error("decrypt failed — wrong key, tampered ciphertext, or AAD mismatch")]
    Open,
}

/// Seal `plaintext` with the master key + a per-row AAD.
///
/// AAD MUST be the concatenation of the workspace_storage row id and the
/// current `key_version`, formatted as `<ulid>:<n>`. The same string must
/// be provided at `open` time or decryption fails. Rotating credentials =
/// bump key_version + re-seal — old ciphertexts then fail AAD check, which
/// is exactly the invariant the cache invalidation depends on.
pub fn seal(master_key: &[u8; 32], plaintext: &[u8], aad: &str) -> Result<String, SecretBoxError> {
    let cipher = Aes256Gcm::new(master_key.into());
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| SecretBoxError::Seal)?;

    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(STANDARD_NO_PAD.encode(&out))
}

/// Verify + decrypt an envelope produced by [`seal`]. Returns the original
/// plaintext bytes. Fails on any of: malformed base64, short envelope,
/// tampered ciphertext, wrong master key, wrong AAD.
pub fn open(
    master_key: &[u8; 32],
    envelope_b64: &str,
    aad: &str,
) -> Result<Vec<u8>, SecretBoxError> {
    let raw = STANDARD_NO_PAD
        .decode(envelope_b64.as_bytes())
        .map_err(|_| SecretBoxError::Malformed)?;
    if raw.len() <= NONCE_LEN + 16 {
        return Err(SecretBoxError::Malformed);
    }
    let (nonce_bytes, ct) = raw.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = Aes256Gcm::new(master_key.into());
    cipher
        .decrypt(
            nonce,
            Payload {
                msg: ct,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| SecretBoxError::Open)
}

/// Parses a hex master key from a 64-char string. Used by the binary to
/// load `DRIVE_STORAGE_SECRET_KEY`. Anything other than exactly 32 bytes
/// of valid hex is rejected at boot — half-configured crypto is worse
/// than no crypto.
pub fn parse_master_key_hex(hex: &str) -> Result<[u8; 32], &'static str> {
    let trimmed = hex.trim();
    if trimmed.len() != 64 {
        return Err("DRIVE_STORAGE_SECRET_KEY must be 64 hex chars (32 bytes)");
    }
    let mut out = [0u8; 32];
    for (i, chunk) in trimmed.as_bytes().chunks(2).enumerate() {
        let hi = hex_digit(chunk[0]).ok_or("DRIVE_STORAGE_SECRET_KEY contains non-hex")?;
        let lo = hex_digit(chunk[1]).ok_or("DRIVE_STORAGE_SECRET_KEY contains non-hex")?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + b - b'a'),
        b'A'..=b'F' => Some(10 + b - b'A'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, slot) in k.iter_mut().enumerate() {
            *slot = i as u8;
        }
        k
    }

    #[test]
    fn roundtrip() {
        let k = key();
        let pt = b"AKIAEXAMPLESECRETKEY";
        let env = seal(&k, pt, "01ARZ:1").unwrap();
        assert_eq!(open(&k, &env, "01ARZ:1").unwrap(), pt);
    }

    #[test]
    fn aad_mismatch_fails() {
        let k = key();
        let env = seal(&k, b"secret", "row1:1").unwrap();
        assert!(matches!(
            open(&k, &env, "row1:2").err(),
            Some(SecretBoxError::Open)
        ));
        assert!(matches!(
            open(&k, &env, "row2:1").err(),
            Some(SecretBoxError::Open)
        ));
    }

    #[test]
    fn tamper_fails() {
        let k = key();
        let env = seal(&k, b"secret", "row1:1").unwrap();
        let mut bytes = STANDARD_NO_PAD.decode(env.as_bytes()).unwrap();
        // Flip the last byte (part of the auth tag) — must fail.
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        let tampered = STANDARD_NO_PAD.encode(&bytes);
        assert!(matches!(
            open(&k, &tampered, "row1:1").err(),
            Some(SecretBoxError::Open)
        ));
    }

    #[test]
    fn wrong_key_fails() {
        let env = seal(&key(), b"secret", "row:1").unwrap();
        let mut wrong = key();
        wrong[0] ^= 1;
        assert!(matches!(
            open(&wrong, &env, "row:1").err(),
            Some(SecretBoxError::Open)
        ));
    }

    #[test]
    fn malformed_b64_fails() {
        assert!(matches!(
            open(&key(), "!!!!", "row:1").err(),
            Some(SecretBoxError::Malformed)
        ));
    }

    #[test]
    fn short_envelope_fails() {
        let too_short = STANDARD_NO_PAD.encode([0u8; 10]);
        assert!(matches!(
            open(&key(), &too_short, "row:1").err(),
            Some(SecretBoxError::Malformed)
        ));
    }

    #[test]
    fn parses_hex_key() {
        let hex = "0001020304050607080910111213141516171819202122232425262728293031";
        let k = parse_master_key_hex(hex).unwrap();
        assert_eq!(k[0], 0x00);
        assert_eq!(k[3], 0x03);
    }

    #[test]
    fn rejects_short_hex() {
        assert!(parse_master_key_hex("deadbeef").is_err());
    }

    #[test]
    fn rejects_non_hex() {
        assert!(parse_master_key_hex(&"z".repeat(64)).is_err());
    }
}

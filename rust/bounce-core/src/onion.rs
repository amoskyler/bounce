//! Tor v3 onion service addressing.
//!
//! A device's identity in Bounce *is* its onion address. The address is a
//! reversible encoding of an Ed25519 public key, which is what lets any device
//! verify a signature from any other device knowing nothing but its address —
//! there is no key distribution step and no directory to consult.
//!
//! The encoding is specified in Tor rend-spec-v3 §6:
//!
//! ```text
//! onion_address = base32(PUBKEY || CHECKSUM || VERSION)
//! CHECKSUM      = SHA3-256(".onion checksum" || PUBKEY || VERSION)[..2]
//! VERSION       = 0x03
//! ```

use data_encoding::BASE32_NOPAD;
use sha3::{Digest, Sha3_256};

use crate::error::{Error, Result};

/// Length of the base32 onion service ID, without the `.onion` suffix.
pub const ONION_ADDRESS_LENGTH: usize = 56;

/// Version byte for v3 onion services.
pub const ONION_VERSION: u8 = 0x03;

const CHECKSUM_PREFIX: &[u8] = b".onion checksum";

/// Derive the onion service ID for an Ed25519 public key.
pub fn address_from_public_key(public_key: &[u8; 32]) -> String {
    let mut buf = Vec::with_capacity(35);
    buf.extend_from_slice(public_key);
    buf.extend_from_slice(&checksum(public_key));
    buf.push(ONION_VERSION);
    BASE32_NOPAD.encode(&buf).to_lowercase()
}

/// Recover the Ed25519 public key encoded in an onion service ID.
///
/// The address may be given with or without a `.onion` suffix and in any case.
pub fn public_key_from_address(address: &str) -> Result<[u8; 32]> {
    let trimmed = address.trim().trim_end_matches(".onion");
    if trimmed.len() != ONION_ADDRESS_LENGTH {
        return Err(Error::InvalidOnionAddress(format!(
            "expected {ONION_ADDRESS_LENGTH} characters, got {}",
            trimmed.len()
        )));
    }

    let decoded = BASE32_NOPAD
        .decode(trimmed.to_uppercase().as_bytes())
        .map_err(|e| Error::InvalidOnionAddress(e.to_string()))?;
    if decoded.len() != 35 {
        return Err(Error::InvalidOnionAddress(format!(
            "decoded to {} bytes, expected 35",
            decoded.len()
        )));
    }

    if decoded[34] != ONION_VERSION {
        return Err(Error::InvalidOnionAddress(format!(
            "unsupported onion version {}",
            decoded[34]
        )));
    }

    let mut public_key = [0u8; 32];
    public_key.copy_from_slice(&decoded[..32]);

    if decoded[32..34] != checksum(&public_key) {
        return Err(Error::InvalidOnionAddress("checksum mismatch".into()));
    }

    Ok(public_key)
}

/// Whether a string is a syntactically valid v3 onion service ID.
pub fn is_valid_address(address: &str) -> bool {
    public_key_from_address(address).is_ok()
}

fn checksum(public_key: &[u8; 32]) -> [u8; 2] {
    let mut hasher = Sha3_256::new();
    hasher.update(CHECKSUM_PREFIX);
    hasher.update(public_key);
    hasher.update([ONION_VERSION]);
    let digest = hasher.finalize();
    [digest[0], digest[1]]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tor's own documentation example: the DuckDuckGo onion service.
    const DDG: &str = "duckduckgogg42xjoc72x3sjasowoarfbgcmvfimaftt6twagswzczad";

    #[test]
    fn known_address_round_trips() {
        let key = public_key_from_address(DDG).expect("well-formed address");
        assert_eq!(address_from_public_key(&key), DDG);
    }

    #[test]
    fn accepts_suffix_and_mixed_case() {
        let bare = public_key_from_address(DDG).unwrap();
        let suffixed = public_key_from_address(&format!("{DDG}.onion")).unwrap();
        let upper = public_key_from_address(&DDG.to_uppercase()).unwrap();
        assert_eq!(bare, suffixed);
        assert_eq!(bare, upper);
    }

    #[test]
    fn rejects_corrupted_checksum() {
        // Flip a character in the middle of the key material.
        let mut chars: Vec<char> = DDG.chars().collect();
        chars[5] = if chars[5] == 'a' { 'b' } else { 'a' };
        let corrupted: String = chars.into_iter().collect();
        assert!(matches!(
            public_key_from_address(&corrupted),
            Err(Error::InvalidOnionAddress(_))
        ));
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(public_key_from_address("tooshort").is_err());
        assert!(public_key_from_address(&"a".repeat(57)).is_err());
    }

    #[test]
    fn generated_keys_round_trip() {
        for _ in 0..32 {
            let signing = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
            let public = signing.verifying_key().to_bytes();
            let address = address_from_public_key(&public);
            assert_eq!(address.len(), ONION_ADDRESS_LENGTH);
            assert_eq!(public_key_from_address(&address).unwrap(), public);
        }
    }
}

//! The signed container that authenticates frames.
//!
//! Most frames travel wrapped in a [`SignedContainer`]: the encoded frame, the
//! address of the device that signed it, and an Ed25519 signature over
//! `BLAKE3(payload)`.
//!
//! Two properties of this design matter downstream:
//!
//! 1. **The signature covers the transmitted bytes.** On receipt the original
//!    payload buffer is retained, so a frame can be relayed to another device
//!    and still verify there, even if that device would have encoded the same
//!    logical frame differently.
//! 2. **The signer is a device, not a user.** Establishing that a frame really
//!    came from the user it claims requires a second step — resolving the
//!    signing device to its owner — which is what [`signed_by_user`] does.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto;
use crate::error::{Error, Result};
use crate::msgpack;

/// A frame payload plus the device signature that authenticates it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedContainer {
    /// Onion address of the signing device.
    #[serde(rename = "Signer")]
    pub signer: String,
    /// The encoded frame.
    #[serde(rename = "Payload", default, with = "crate::msgpack::nullable_bytes")]
    pub payload: Vec<u8>,
    /// Ed25519 signature over `BLAKE3(payload)`.
    #[serde(rename = "Signature", default, with = "crate::msgpack::nullable_bytes")]
    pub signature: Vec<u8>,
}

impl SignedContainer {
    /// Sign `payload` with this device's key.
    pub fn create(key: &crypto::DeviceKey, payload: Vec<u8>) -> Self {
        let digest = crypto::hash(&payload);
        SignedContainer {
            signer: key.address(),
            signature: key.sign(&digest).to_vec(),
            payload,
        }
    }

    /// Whether the signature is valid for the claimed signer.
    pub fn is_valid(&self) -> bool {
        let digest = crypto::hash(&self.payload);
        crypto::verify_signature(&self.signer, &digest, &self.signature)
    }

    /// Encode this container for transmission.
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }

    /// Decode a container **and verify its signature**.
    ///
    /// This is the only decoding path callers should use for untrusted input;
    /// it refuses to hand back a container whose signature does not check out.
    pub fn unpack(bytes: &[u8]) -> Result<Self> {
        let container: SignedContainer = msgpack::from_slice(bytes)?;
        if !container.is_valid() {
            return Err(Error::InvalidSignature);
        }
        Ok(container)
    }

    /// Decode the inner frame.
    pub fn decode_payload<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        msgpack::from_slice(&self.payload)
    }
}

/// Resolve a signing device to its owner and check it against the user a frame
/// claims to be from.
///
/// `device_owner` looks a device address up in the caller's store, returning
/// `None` if the device is unknown — an unknown signer never satisfies the
/// check, since a device we have never been introduced to cannot speak for
/// anyone.
pub fn signed_by_user<F>(container: &SignedContainer, claimed_author: Uuid, device_owner: F) -> bool
where
    F: FnOnce(&str) -> Option<Uuid>,
{
    match device_owner(&container.signer) {
        Some(owner) => owner == claimed_author,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_and_verifies() {
        let key = crypto::DeviceKey::generate();
        let container = SignedContainer::create(&key, b"a frame".to_vec());

        assert_eq!(container.signer, key.address());
        assert!(container.is_valid());
    }

    #[test]
    fn survives_encode_decode() {
        let key = crypto::DeviceKey::generate();
        let container = SignedContainer::create(&key, b"a frame".to_vec());

        let encoded = container.encode().unwrap();
        let decoded = SignedContainer::unpack(&encoded).unwrap();
        assert_eq!(decoded, container);
    }

    #[test]
    fn unpack_rejects_a_tampered_payload() {
        let key = crypto::DeviceKey::generate();
        let mut container = SignedContainer::create(&key, b"original".to_vec());
        container.payload = b"tampered".to_vec();

        let encoded = container.encode().unwrap();
        assert!(matches!(
            SignedContainer::unpack(&encoded),
            Err(Error::InvalidSignature)
        ));
    }

    #[test]
    fn unpack_rejects_a_forged_signer() {
        let key = crypto::DeviceKey::generate();
        let impostor = crypto::DeviceKey::generate();

        // Claim to be a different device while keeping the original signature.
        let mut container = SignedContainer::create(&key, b"payload".to_vec());
        container.signer = impostor.address();

        let encoded = container.encode().unwrap();
        assert!(matches!(
            SignedContainer::unpack(&encoded),
            Err(Error::InvalidSignature)
        ));
    }

    #[test]
    fn signed_by_user_requires_a_known_device_owned_by_the_author() {
        let key = crypto::DeviceKey::generate();
        let container = SignedContainer::create(&key, b"payload".to_vec());

        let author = Uuid::new_v4();
        let someone_else = Uuid::new_v4();

        assert!(signed_by_user(&container, author, |_| Some(author)));
        assert!(!signed_by_user(&container, author, |_| Some(someone_else)));
        // A device we have never been introduced to speaks for nobody.
        assert!(!signed_by_user(&container, author, |_| None));
    }

    #[test]
    fn payload_round_trips_through_the_container() {
        #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        struct Inner {
            #[serde(rename = "Text")]
            text: String,
        }

        let key = crypto::DeviceKey::generate();
        let inner = Inner {
            text: "hello".into(),
        };
        let container = SignedContainer::create(&key, msgpack::to_vec(&inner).unwrap());

        assert_eq!(container.decode_payload::<Inner>().unwrap(), inner);
    }
}

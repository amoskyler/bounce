//! Encrypted devices.
//!
//! An encrypted device is a Bounce instance that relays and stores frames it
//! cannot read. That makes it safe to run one somewhere always-online but less
//! trusted — a VPS — so that messages still arrive when every device you
//! actually own is offline.
//!
//! ## How a frame is encrypted to a device
//!
//! A normal device generates a fresh data encryption key (DEK) per frame and
//! seals the frame with it. Then, for each user in the frame's scope, it does
//! an X25519 exchange between its own user key and that user's public key to
//! get a key encryption key (KEK), and seals the DEK under that. Those sealed
//! DEKs are the [`Recipient`] list.
//!
//! ```text
//!   frame ──seal(DEK)──> ciphertext
//!   DEK   ──seal(KEK_alice)──> recipient{alice}
//!   DEK   ──seal(KEK_bob)────> recipient{bob}
//! ```
//!
//! The encrypted device checks that its own owner is among the recipients,
//! stores the blob, and learns nothing beyond the frame's UUID and type — which
//! it needs in order to participate in the reference flow.
//!
//! Recipient lists are capped at [`crate::MAXIMUM_RECIPIENTS`] users so they do
//! not grow linearly with group size.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;
use crate::msgpack;

/// A sealed copy of a frame's DEK for one user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipient {
    #[serde(skip)]
    pub id: Uuid,
    #[serde(skip)]
    pub encrypted_frame_id: Uuid,

    /// The recipient's X25519 public key.
    #[serde(rename = "PublicKey", default, with = "crate::msgpack::nullable_bytes")]
    pub public_key: Vec<u8>,

    /// The sender's X25519 public key, so the recipient can derive the same KEK.
    #[serde(rename = "EncrypterKey", default, with = "crate::msgpack::nullable_bytes")]
    pub encrypter_key: Vec<u8>,

    #[serde(rename = "EncryptedDEK", default, with = "crate::msgpack::nullable_bytes")]
    pub encrypted_dek: Vec<u8>,
}

/// A sealed copy of a DEK for one specific *device* rather than a user.
///
/// Used only when re-keying: the frame that carries a user's new keys cannot
/// itself be encrypted to those keys, so it is encrypted to each surviving
/// device's own X25519 key instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRecipient {
    #[serde(skip)]
    pub id: Uuid,
    #[serde(skip)]
    pub encrypted_frame_id: Uuid,

    #[serde(rename = "RecipientAddress")]
    pub recipient_address: String,

    /// Address of the device that did the encrypting.
    #[serde(rename = "Counterparty")]
    pub counterparty: String,

    #[serde(rename = "EncryptedDEK", default, with = "crate::msgpack::nullable_bytes")]
    pub encrypted_dek: Vec<u8>,
}

/// A frame as it is stored on an encrypted device.
///
/// Only `id` and `frame_type` are shared with the plaintext original — enough
/// to offer and request the frame by reference, and nothing more.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedFrame {
    #[serde(rename = "ID")]
    pub id: Uuid,

    #[serde(rename = "Type")]
    pub frame_type: u16,

    #[serde(rename = "Timestamp")]
    pub timestamp: i64,

    #[serde(skip)]
    pub saved_at: i64,

    /// The sealed frame.
    #[serde(rename = "Payload", default, with = "crate::msgpack::nullable_bytes")]
    pub payload: Vec<u8>,

    #[serde(rename = "DeleteAt")]
    pub delete_at: i64,

    /// Groups frames that should be deleted together when a conversation's
    /// history is cleared. The encrypted device cannot tell what the key means.
    #[serde(rename = "BatchDeleteKey")]
    pub batch_delete_key: Uuid,

    #[serde(rename = "CanBatchDelete")]
    pub can_batch_delete: bool,

    #[serde(rename = "Recipients")]
    #[serde(default, deserialize_with = "crate::msgpack::nullable_seq")]
    pub recipients: Vec<Recipient>,

    #[serde(rename = "DeviceRecipients")]
    #[serde(default, deserialize_with = "crate::msgpack::nullable_seq")]
    pub device_recipients: Vec<DeviceRecipient>,
}

impl EncryptedFrame {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }

    /// Find this user's sealed DEK, if they are a recipient.
    pub fn recipient_for(&self, public_key: &[u8]) -> Option<&Recipient> {
        self.recipients.iter().find(|r| r.public_key == public_key)
    }

    /// Find this device's sealed DEK, if it is a device recipient.
    pub fn device_recipient_for(&self, address: &str) -> Option<&DeviceRecipient> {
        self.device_recipients
            .iter()
            .find(|r| r.recipient_address == address)
    }

    /// Whether the given user is authorized to hold this frame.
    ///
    /// An encrypted device refuses to store anything its owner is not a
    /// recipient of, which is what stops it from being used as general-purpose
    /// storage by whoever can reach it.
    pub fn is_authorized_for(&self, public_key: &[u8]) -> bool {
        self.recipient_for(public_key).is_some()
    }

    /// Recover the plaintext frame using a user's X25519 private key.
    pub fn decrypt_for_user(&self, private_key: &[u8], public_key: &[u8]) -> Result<Vec<u8>> {
        use crate::crypto;

        let recipient = self
            .recipient_for(public_key)
            .ok_or(crate::error::Error::DecryptionFailed)?;

        let kek = crypto::generate_kek(private_key, &recipient.encrypter_key)?;
        let dek = crypto::open(&kek, &recipient.encrypted_dek)?;
        crypto::open(&dek, &self.payload)
    }

    /// Recover the plaintext frame using a device's own X25519 private key.
    ///
    /// A [`DeviceRecipient`] names the encrypting device by address rather than
    /// by key, so the caller resolves that address to its X25519 public key and
    /// passes it in as `counterparty_public_key`.
    pub fn decrypt_for_device(
        &self,
        private_key: &[u8],
        address: &str,
        counterparty_public_key: &[u8],
    ) -> Result<Vec<u8>> {
        use crate::crypto;

        let recipient = self
            .device_recipient_for(address)
            .ok_or(crate::error::Error::DecryptionFailed)?;

        let kek = crypto::generate_kek(private_key, counterparty_public_key)?;
        let dek = crypto::open(&kek, &recipient.encrypted_dek)?;
        crypto::open(&dek, &self.payload)
    }
}

/// Seal a frame for a set of user public keys.
///
/// Returns the sealed frame and one [`Recipient`] per key. `recipients` is
/// expected to already be capped at [`crate::MAXIMUM_RECIPIENTS`] and to
/// include the encrypted device's owner.
pub fn seal_for_recipients(
    sender_private_key: &[u8],
    sender_public_key: &[u8],
    recipient_public_keys: &[Vec<u8>],
    plaintext: &[u8],
) -> Result<(Vec<u8>, Vec<Recipient>)> {
    use crate::crypto;

    let dek = crypto::generate_dek();
    let ciphertext = crypto::seal(&dek, plaintext)?;

    let mut recipients = Vec::with_capacity(recipient_public_keys.len());
    for public_key in recipient_public_keys {
        let kek = crypto::generate_kek(sender_private_key, public_key)?;
        recipients.push(Recipient {
            id: Uuid::new_v4(),
            encrypted_frame_id: Uuid::nil(),
            public_key: public_key.clone(),
            encrypter_key: sender_public_key.to_vec(),
            encrypted_dek: crypto::seal(&kek, &dek)?,
        });
    }

    Ok((ciphertext, recipients))
}

/// A challenge an encrypted device issues to learn which user a peer speaks for.
///
/// The encrypted device does not know which devices belong to which users, so
/// it cannot decide what to offer. It sends an ephemeral public key and some
/// random data; the peer proves control of a user key by returning that data
/// sealed under the resulting shared secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedReferenceOfferChallenge {
    #[serde(rename = "Key", default, with = "crate::msgpack::nullable_bytes")]
    pub key: Vec<u8>,
    #[serde(rename = "Challenge", default, with = "crate::msgpack::nullable_bytes")]
    pub challenge: Vec<u8>,
}

impl EncryptedReferenceOfferChallenge {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
}

/// The answer to an [`EncryptedReferenceOfferChallenge`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedReferenceOfferResponse {
    #[serde(rename = "PublicKey", default, with = "crate::msgpack::nullable_bytes")]
    pub public_key: Vec<u8>,
    #[serde(rename = "Response", default, with = "crate::msgpack::nullable_bytes")]
    pub response: Vec<u8>,
}

impl EncryptedReferenceOfferResponse {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }

    /// Answer a challenge with a user's X25519 private key.
    pub fn answer(
        challenge: &EncryptedReferenceOfferChallenge,
        private_key: &[u8],
        public_key: &[u8],
    ) -> Result<Self> {
        use crate::crypto;
        let shared = crypto::generate_kek(private_key, &challenge.key)?;
        Ok(EncryptedReferenceOfferResponse {
            public_key: public_key.to_vec(),
            response: crypto::seal(&shared, &challenge.challenge)?,
        })
    }

    /// Check an answer against the ephemeral private key the challenge was
    /// issued with.
    pub fn verify(
        &self,
        challenge: &EncryptedReferenceOfferChallenge,
        ephemeral_private_key: &[u8],
    ) -> bool {
        use crate::crypto;
        let Ok(shared) = crypto::generate_kek(ephemeral_private_key, &self.public_key) else {
            return false;
        };
        crypto::open(&shared, &self.response)
            .map(|plaintext| plaintext == challenge.challenge)
            .unwrap_or(false)
    }
}

/// A request to be admitted as the manager of an encrypted device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedDeviceManagementRequest {
    #[serde(rename = "Secret")]
    pub secret: String,
    #[serde(rename = "SigningKey", default, with = "crate::msgpack::nullable_bytes")]
    pub signing_key: Vec<u8>,
    #[serde(rename = "Pubkey", default, with = "crate::msgpack::nullable_bytes")]
    pub pubkey: Vec<u8>,
}

impl EncryptedDeviceManagementRequest {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
}

/// The encrypted device's answer to a management request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedDeviceManagementResponse {
    #[serde(rename = "Accepted")]
    pub accepted: bool,
}

impl EncryptedDeviceManagementResponse {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
}

/// An instruction to drop everything in a batch older than a cutoff, used when
/// a conversation's history is cleared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedClearBefore {
    #[serde(rename = "ID")]
    pub id: Uuid,
    #[serde(rename = "BatchKey")]
    pub batch_key: Uuid,
    #[serde(rename = "Timestamp")]
    pub timestamp: i64,
}

impl EncryptedClearBefore {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
}

/// A user authorized to store frames on an encrypted device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedUser {
    pub id: Uuid,
    pub public_key: Vec<u8>,
    pub signing_key: Vec<u8>,
}

/// Trim a recipient list to the protocol's cap while guaranteeing that the
/// encrypted device's owner is retained.
///
/// Without the guarantee, a large group could randomly exclude the very user
/// whose device is being written to, and the device would refuse the frame.
pub fn prune_recipients(must_include: Uuid, users: &[(Uuid, Vec<u8>)]) -> Vec<(Uuid, Vec<u8>)> {
    if users.len() <= crate::MAXIMUM_RECIPIENTS {
        return users.to_vec();
    }

    let mut required = None;
    let mut others = Vec::new();
    for user in users {
        if user.0 == must_include {
            required = Some(user.clone());
        } else {
            others.push(user.clone());
        }
    }

    // Sample the rest without replacement.
    let keep = match required {
        Some(_) => crate::MAXIMUM_RECIPIENTS - 1,
        None => crate::MAXIMUM_RECIPIENTS,
    };

    use rand::seq::SliceRandom;
    others.shuffle(&mut rand::thread_rng());
    others.truncate(keep);

    if let Some(required) = required {
        others.push(required);
    }
    others
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto;

    #[test]
    fn a_recipient_can_recover_the_frame() {
        let (alice_private, alice_public) = crypto::generate_x25519_keypair();
        let (bob_private, bob_public) = crypto::generate_x25519_keypair();

        let plaintext = b"the contents of a message";
        let (ciphertext, recipients) = seal_for_recipients(
            &alice_private,
            &alice_public,
            &[alice_public.to_vec(), bob_public.to_vec()],
            plaintext,
        )
        .unwrap();

        let frame = EncryptedFrame {
            id: Uuid::new_v4(),
            frame_type: crate::types::FrameType::DirectMessage.as_u16(),
            timestamp: 0,
            saved_at: 0,
            payload: ciphertext,
            delete_at: 0,
            batch_delete_key: Uuid::nil(),
            can_batch_delete: false,
            recipients,
            device_recipients: vec![],
        };

        // Both named recipients get the plaintext back.
        assert_eq!(
            frame.decrypt_for_user(&bob_private, &bob_public).unwrap(),
            plaintext
        );
        assert_eq!(
            frame.decrypt_for_user(&alice_private, &alice_public).unwrap(),
            plaintext
        );
    }

    #[test]
    fn a_non_recipient_cannot_read_the_frame() {
        let (alice_private, alice_public) = crypto::generate_x25519_keypair();
        let (eve_private, eve_public) = crypto::generate_x25519_keypair();

        let (ciphertext, recipients) =
            seal_for_recipients(&alice_private, &alice_public, &[alice_public.to_vec()], b"secret")
                .unwrap();

        let frame = EncryptedFrame {
            id: Uuid::new_v4(),
            frame_type: 0,
            timestamp: 0,
            saved_at: 0,
            payload: ciphertext,
            delete_at: 0,
            batch_delete_key: Uuid::nil(),
            can_batch_delete: false,
            recipients,
            device_recipients: vec![],
        };

        assert!(!frame.is_authorized_for(&eve_public));
        assert!(frame.decrypt_for_user(&eve_private, &eve_public).is_err());
    }

    #[test]
    fn the_stored_frame_reveals_only_its_reference_metadata() {
        let (private, public) = crypto::generate_x25519_keypair();
        let plaintext = b"a distinctive plaintext marker";

        let (ciphertext, recipients) =
            seal_for_recipients(&private, &public, &[public.to_vec()], plaintext).unwrap();

        let frame = EncryptedFrame {
            id: Uuid::new_v4(),
            frame_type: crate::types::FrameType::GroupMessage.as_u16(),
            timestamp: 12345,
            saved_at: 0,
            payload: ciphertext,
            delete_at: 0,
            batch_delete_key: Uuid::nil(),
            can_batch_delete: false,
            recipients,
            device_recipients: vec![],
        };

        // Whatever the encrypted device stores must not contain the plaintext.
        let stored = frame.encode().unwrap();
        assert!(!stored
            .windows(plaintext.len())
            .any(|window| window == plaintext));
    }

    #[test]
    fn the_challenge_proves_control_of_a_user_key() {
        let (ephemeral_private, ephemeral_public) = crypto::generate_x25519_keypair();
        let (user_private, user_public) = crypto::generate_x25519_keypair();

        let challenge = EncryptedReferenceOfferChallenge {
            key: ephemeral_public.to_vec(),
            challenge: crypto::random_bytes(32),
        };

        let response =
            EncryptedReferenceOfferResponse::answer(&challenge, &user_private, &user_public)
                .unwrap();
        assert!(response.verify(&challenge, &ephemeral_private));
    }

    #[test]
    fn a_challenge_cannot_be_answered_without_the_matching_private_key() {
        let (ephemeral_private, ephemeral_public) = crypto::generate_x25519_keypair();
        let (_, victim_public) = crypto::generate_x25519_keypair();
        let (attacker_private, _) = crypto::generate_x25519_keypair();

        let challenge = EncryptedReferenceOfferChallenge {
            key: ephemeral_public.to_vec(),
            challenge: crypto::random_bytes(32),
        };

        // Claim to be the victim while holding a different private key.
        let mut forged =
            EncryptedReferenceOfferResponse::answer(&challenge, &attacker_private, &victim_public)
                .unwrap();
        forged.public_key = victim_public.to_vec();

        assert!(!forged.verify(&challenge, &ephemeral_private));
    }

    #[test]
    fn pruning_keeps_the_owner_and_respects_the_cap() {
        let owner = Uuid::new_v4();
        let mut users: Vec<(Uuid, Vec<u8>)> = (0..40)
            .map(|_| (Uuid::new_v4(), crypto::random_bytes(32)))
            .collect();
        users.push((owner, crypto::random_bytes(32)));

        let pruned = prune_recipients(owner, &users);

        assert_eq!(pruned.len(), crate::MAXIMUM_RECIPIENTS);
        assert!(
            pruned.iter().any(|(id, _)| *id == owner),
            "the encrypted device's owner must always be a recipient"
        );
    }

    #[test]
    fn pruning_leaves_small_groups_alone() {
        let owner = Uuid::new_v4();
        let users: Vec<(Uuid, Vec<u8>)> = std::iter::once((owner, vec![0u8; 32]))
            .chain((0..3).map(|_| (Uuid::new_v4(), vec![1u8; 32])))
            .collect();

        assert_eq!(prune_recipients(owner, &users).len(), users.len());
    }
}

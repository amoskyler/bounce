//! Adding contacts and pairing new devices.
//!
//! ## Adding a contact
//!
//! There is no directory to look anyone up in, so two users add each other by
//! meeting in person. One displays a short-lived secret and its device address;
//! the other scans it and connects.
//!
//! ```text
//!   requester                          offerer
//!       |                                  |  displays secret + address
//!       |------- AddUserRequest ---------->|  secret + my whole user record
//!       |<------ AddUserRequestAccepted ---|  their record + their signature of mine
//!       |                                  |
//!       |  builds AddUser: both records, both signatures, both addresses
//!       |------- AddUser ----------------->|  broadcast to both device groups
//! ```
//!
//! The resulting [`AddUser`] is self-contained proof that a device from each
//! side consented. That is what lets a device which was offline for the whole
//! exchange — and has never heard of the new contact — accept them later,
//! without having to trust the peer that presents it.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::identity::{ProfileSettings, User};
use super::Broadcastable;
use crate::error::Result;
use crate::types::{FrameType, Scope};
use crate::{msgpack, xor};

/// First step of adding a contact: the scanning side presents the secret it
/// read along with its own user record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddUserRequest {
    #[serde(rename = "Secret")]
    pub secret: String,
    /// The requester's encoded [`User`], device group included.
    #[serde(rename = "RequesterUser", default, with = "crate::msgpack::nullable_bytes")]
    pub requester_user: Vec<u8>,
}

impl AddUserRequest {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }

    pub fn user(&self) -> Result<User> {
        msgpack::from_slice(&self.requester_user)
    }
}

/// Second step: the offering side returns its own record plus a signature over
/// the requester's record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddUserRequestAccepted {
    /// The offerer's encoded [`User`].
    #[serde(rename = "OfferUser", default, with = "crate::msgpack::nullable_bytes")]
    pub offer_user: Vec<u8>,
    /// The offering device's signature over `BLAKE3(requester_user)`.
    #[serde(rename = "OfferSignature", default, with = "crate::msgpack::nullable_bytes")]
    pub offer_signature: Vec<u8>,
    /// Address of the device that produced that signature.
    ///
    /// Absent in the Go implementation, and absent here too by default: the
    /// signer is whichever device the connection was established with, an
    /// address the handshake already proved. Kept as an optional field so a
    /// frame carrying it still decodes.
    #[serde(rename = "OfferDevice", default, skip_serializing_if = "Option::is_none")]
    pub offer_device: Option<String>,
}

impl AddUserRequestAccepted {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }

    pub fn user(&self) -> Result<User> {
        msgpack::from_slice(&self.offer_user)
    }
}

/// Sent when a secret is wrong, expired, or the request is otherwise refused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddUserRequestRejected {}

impl AddUserRequestRejected {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
}

/// The completed, self-verifying record of two users adding each other.
///
/// Anyone in either device group can check this without having witnessed the
/// exchange: each side's signature covers the other side's user record, and the
/// signing devices are named, so both signatures can be verified against the
/// addresses they were made with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddUser {
    #[serde(rename = "ID")]
    pub id: Uuid,

    /// XOR of the two user IDs, identifying the pairing without naming either.
    #[serde(rename = "Xor")]
    pub xor: Uuid,

    #[serde(rename = "Timestamp")]
    pub timestamp: i64,

    #[serde(skip)]
    pub saved_at: i64,

    #[serde(rename = "OfferUser", default, with = "crate::msgpack::nullable_bytes")]
    pub offer_user: Vec<u8>,

    #[serde(rename = "RequesterUser", default, with = "crate::msgpack::nullable_bytes")]
    pub requester_user: Vec<u8>,

    #[serde(rename = "OfferDevice")]
    pub offer_device: String,

    #[serde(rename = "RequesterDevice")]
    pub requester_device: String,

    /// Offering device's signature over `BLAKE3(requester_user)`.
    #[serde(rename = "OfferSignature", default, with = "crate::msgpack::nullable_bytes")]
    pub offer_signature: Vec<u8>,

    /// Requesting device's signature over `BLAKE3(offer_user)`.
    #[serde(rename = "RequesterSignature", default, with = "crate::msgpack::nullable_bytes")]
    pub requester_signature: Vec<u8>,
}

impl AddUser {
    pub fn offerer(&self) -> Result<User> {
        msgpack::from_slice(&self.offer_user)
    }

    pub fn requester(&self) -> Result<User> {
        msgpack::from_slice(&self.requester_user)
    }

    /// Verify that both sides really consented.
    ///
    /// Each signature is checked against the address of the device that claims
    /// to have made it, over the hash of the *other* side's record — so neither
    /// party can be added without a device of theirs having signed for it.
    pub fn signatures_are_valid(&self) -> bool {
        use crate::crypto;

        let offer_ok = crypto::verify_signature(
            &self.offer_device,
            &crypto::hash(&self.requester_user),
            &self.offer_signature,
        );
        let requester_ok = crypto::verify_signature(
            &self.requester_device,
            &crypto::hash(&self.offer_user),
            &self.requester_signature,
        );
        offer_ok && requester_ok
    }

    /// Whether the signing devices really belong to the users being added.
    ///
    /// A valid signature from a device that is not in the corresponding device
    /// group proves nothing about that user's consent.
    pub fn signers_are_members(&self) -> Result<bool> {
        let offerer = self.offerer()?;
        let requester = self.requester()?;
        Ok(device_group_contains(&offerer, &self.offer_device)
            && device_group_contains(&requester, &self.requester_device))
    }

    /// Whether the XOR field really names these two users.
    pub fn xor_matches_users(&self) -> Result<bool> {
        Ok(xor(self.offerer()?.id, self.requester()?.id) == self.xor)
    }
}

fn device_group_contains(user: &User, address: &str) -> bool {
    user.devices.iter().any(|d| d.address == address)
}

impl Broadcastable for AddUser {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::AddUser
    }
    fn payload(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
    fn scope(&self, _my_id: Uuid) -> Scope {
        Scope::User
    }
    fn destination(&self, my_id: Uuid) -> Uuid {
        xor(my_id, self.xor)
    }
    fn author(&self) -> Uuid {
        Uuid::nil()
    }
    fn timestamp(&self) -> i64 {
        self.timestamp
    }
    fn saved_at(&self) -> i64 {
        self.saved_at
    }
}

/// A new device asking to join an existing profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncDeviceRequest {
    /// The joining device's signature over the existing device's address —
    /// its consent to be added.
    #[serde(rename = "Signature", default, with = "crate::msgpack::nullable_bytes")]
    pub signature: Vec<u8>,
    /// The short-lived secret shown by the existing device.
    #[serde(rename = "Secret")]
    pub secret: String,
}

impl SyncDeviceRequest {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
}

/// The profile handed to a device that has been admitted.
///
/// This is the one frame that carries private key material, and it only ever
/// travels between two devices owned by the same person over an authenticated
/// connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncDeviceRequestAccepted {
    #[serde(rename = "Profile")]
    pub profile: User,

    #[serde(rename = "PrivateECDHKey", default, with = "crate::msgpack::nullable_bytes")]
    pub private_ecdh_key: Vec<u8>,
    #[serde(rename = "PublicECDHKey", default, with = "crate::msgpack::nullable_bytes")]
    pub public_ecdh_key: Vec<u8>,
    #[serde(rename = "PrivateECDSAKey", default, with = "crate::msgpack::nullable_bytes")]
    pub private_ecdsa_key: Vec<u8>,
    #[serde(rename = "PublicECDSAKey", default, with = "crate::msgpack::nullable_bytes")]
    pub public_ecdsa_key: Vec<u8>,

    #[serde(rename = "Settings")]
    pub settings: Option<ProfileSettings>,

    /// Whether the admitting device has history to offer once pairing is done.
    #[serde(rename = "References")]
    pub references: bool,
}

impl SyncDeviceRequestAccepted {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
}

/// Sent when a pairing secret is wrong or expired.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncDeviceRequestRejected {}

impl SyncDeviceRequestRejected {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
}

/// A pending offer to add a contact or a device, valid for a short window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncDeviceOffer {
    pub id: Uuid,
    pub timestamp: i64,
    pub secret: String,
}

/// How long a displayed pairing secret stays valid.
pub const OFFER_VALIDITY_SECONDS: i64 = 5 * 60;

impl SyncDeviceOffer {
    pub fn is_expired(&self, now: i64) -> bool {
        now - self.timestamp > OFFER_VALIDITY_SECONDS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::DeviceKey;
    use crate::frames::identity::Device;

    /// Build a user with one device, returning the user and its device key.
    fn user_with_device(name: &str) -> (User, DeviceKey) {
        let key = DeviceKey::generate();
        let user_id = Uuid::new_v4();
        let mut user = User::new(user_id, name.into());
        user.devices.push(Device::new(
            Uuid::new_v4(),
            user_id,
            key.address(),
            1_700_000_000,
        ));
        (user, key)
    }

    /// Run the full add-user exchange and return the resulting record.
    fn complete_add_user() -> (AddUser, DeviceKey, DeviceKey) {
        let (offerer, offer_key) = user_with_device("Offerer");
        let (requester, requester_key) = user_with_device("Requester");

        let offer_bytes = msgpack::to_vec(&offerer).unwrap();
        let requester_bytes = msgpack::to_vec(&requester).unwrap();

        // Each side signs the hash of the other side's record.
        let offer_signature = offer_key.sign(&crate::crypto::hash(&requester_bytes)).to_vec();
        let requester_signature = requester_key.sign(&crate::crypto::hash(&offer_bytes)).to_vec();

        let add_user = AddUser {
            id: Uuid::new_v4(),
            xor: xor(offerer.id, requester.id),
            timestamp: 1_700_000_000,
            saved_at: 0,
            offer_user: offer_bytes,
            requester_user: requester_bytes,
            offer_device: offer_key.address(),
            requester_device: requester_key.address(),
            offer_signature,
            requester_signature,
        };

        (add_user, offer_key, requester_key)
    }

    #[test]
    fn a_completed_exchange_verifies() {
        let (add_user, _, _) = complete_add_user();

        assert!(add_user.signatures_are_valid());
        assert!(add_user.signers_are_members().unwrap());
        assert!(add_user.xor_matches_users().unwrap());
    }

    #[test]
    fn either_side_recovers_the_other_from_the_xor() {
        let (add_user, _, _) = complete_add_user();
        let offerer = add_user.offerer().unwrap();
        let requester = add_user.requester().unwrap();

        assert_eq!(add_user.destination(offerer.id), requester.id);
        assert_eq!(add_user.destination(requester.id), offerer.id);
        assert_eq!(add_user.scope(offerer.id), Scope::User);
    }

    #[test]
    fn tampering_with_either_record_invalidates_the_signatures() {
        let (add_user, _, _) = complete_add_user();

        // Substitute a different user for the requester.
        let (impostor, _) = user_with_device("Impostor");
        let mut forged = add_user.clone();
        forged.requester_user = msgpack::to_vec(&impostor).unwrap();
        assert!(!forged.signatures_are_valid());

        // Or for the offerer.
        let mut forged = add_user.clone();
        forged.offer_user = msgpack::to_vec(&impostor).unwrap();
        assert!(!forged.signatures_are_valid());
    }

    #[test]
    fn a_signature_from_an_unrelated_device_is_rejected() {
        let (add_user, _, requester_key) = complete_add_user();

        // A valid signature, but made by a device outside the offerer's group.
        let outsider = DeviceKey::generate();
        let mut forged = add_user.clone();
        forged.offer_device = outsider.address();
        forged.offer_signature = outsider
            .sign(&crate::crypto::hash(&add_user.requester_user))
            .to_vec();

        // The signature itself checks out...
        assert!(forged.signatures_are_valid());
        // ...but the signer is not in the device group it claims to speak for.
        assert!(!forged.signers_are_members().unwrap());

        let _ = requester_key;
    }

    #[test]
    fn a_mismatched_xor_is_detected() {
        let (add_user, _, _) = complete_add_user();

        let mut forged = add_user.clone();
        forged.xor = Uuid::new_v4();
        assert!(!forged.xor_matches_users().unwrap());
    }

    #[test]
    fn add_user_round_trips() {
        let (add_user, _, _) = complete_add_user();
        let encoded = msgpack::to_vec(&add_user).unwrap();
        let decoded: AddUser = msgpack::from_slice(&encoded).unwrap();

        assert_eq!(decoded.id, add_user.id);
        assert_eq!(decoded.offer_device, add_user.offer_device);
        assert!(decoded.signatures_are_valid());
    }

    #[test]
    fn pairing_offers_expire() {
        let offer = SyncDeviceOffer {
            id: Uuid::new_v4(),
            timestamp: 1_000,
            secret: "hunter2".into(),
        };

        assert!(!offer.is_expired(1_000));
        assert!(!offer.is_expired(1_000 + OFFER_VALIDITY_SECONDS));
        assert!(offer.is_expired(1_001 + OFFER_VALIDITY_SECONDS));
    }

    #[test]
    fn request_carries_a_decodable_user() {
        let (user, _) = user_with_device("Requester");
        let request = AddUserRequest {
            secret: "abc123".into(),
            requester_user: msgpack::to_vec(&user).unwrap(),
        };

        let decoded: AddUserRequest = msgpack::from_slice(&request.encode().unwrap()).unwrap();
        assert_eq!(decoded.secret, "abc123");
        assert_eq!(decoded.user().unwrap().id, user.id);
    }
}

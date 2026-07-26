//! The frames that make up the Bounce protocol.
//!
//! Every struct here encodes to the same MessagePack bytes as its counterpart
//! in the Go implementation. Field names are the Go field names — MessagePack
//! keys are part of the wire format, so they are pinned with `#[serde(rename)]`
//! rather than left to Rust's naming conventions.
//!
//! ## Fields that never go on the wire
//!
//! Go marks local-only columns with `msgpack:"-"`. Those fields exist here too,
//! because they are still part of the stored row, but they carry
//! `#[serde(skip)]`. Three of them recur on every authenticated frame and are
//! grouped into [`SignedFrame`]:
//!
//! - `signer` — address of the device that signed the frame
//! - `original_payload` — the exact bytes the signature covers
//! - `signature` — the signature itself
//!
//! Retaining `original_payload` is what allows a frame to be relayed onward and
//! still verify on the next device, so it is populated on receipt and never
//! recomputed.

pub mod encrypted;
pub mod file;
pub mod group;
pub mod identity;
pub mod message;
pub mod pairing;
pub mod transport;
pub mod update;

pub use encrypted::*;
pub use file::*;
pub use group::*;
pub use identity::*;
pub use message::*;
pub use pairing::*;
pub use transport::*;
pub use update::*;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::DeviceKey;
use crate::error::Result;
use crate::msgpack;
use crate::signed::SignedContainer;
use crate::types::{FrameType, Scope};

/// The signature material carried alongside an authenticated frame.
///
/// None of these fields are encoded into the frame body: the signature covers
/// the body, so it necessarily lives outside it, in the enclosing
/// [`SignedContainer`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedFrame {
    /// Address of the device that produced the signature.
    pub signer: String,
    /// The exact encoded frame the signature covers.
    pub original_payload: Vec<u8>,
    /// Ed25519 signature over `BLAKE3(original_payload)`.
    pub signature: Vec<u8>,
}

impl SignedFrame {
    /// Populate from a verified container.
    pub fn from_container(container: &SignedContainer) -> Self {
        SignedFrame {
            signer: container.signer.clone(),
            original_payload: container.payload.clone(),
            signature: container.signature.clone(),
        }
    }

    /// Rebuild the container this frame arrived in, so it can be relayed
    /// byte-for-byte.
    pub fn to_container(&self) -> SignedContainer {
        SignedContainer {
            signer: self.signer.clone(),
            payload: self.original_payload.clone(),
            signature: self.signature.clone(),
        }
    }

    /// Whether this frame has been signed yet.
    pub fn is_signed(&self) -> bool {
        !self.signature.is_empty() && !self.signer.is_empty()
    }
}

/// A frame that can be addressed to a set of devices and gossiped.
///
/// This mirrors the `broadcastable` interface in `chat/protocol.go`. The
/// engine uses it to decide who a frame goes to and to track delivery, without
/// needing to know which concrete frame it holds.
pub trait Broadcastable {
    /// Stable identifier, used for acks, delivery records, and references.
    fn id(&self) -> Uuid;

    /// The type tag written into the frame header.
    fn frame_type(&self) -> FrameType;

    /// The encoded frame body as it goes on the wire. For authenticated frames
    /// this is the enclosing signed container.
    fn payload(&self) -> Result<Vec<u8>>;

    /// Which set of devices this frame is delivered to.
    fn scope(&self, my_id: Uuid) -> Scope;

    /// The user or group the scope is resolved against.
    fn destination(&self, my_id: Uuid) -> Uuid;

    /// The user who authored the frame.
    fn author(&self) -> Uuid;

    /// When the author says they created it.
    fn timestamp(&self) -> i64;

    /// When this device first stored it, which is the ordering key for catch
    /// ups. Frames that are never replayed can leave this at zero.
    fn saved_at(&self) -> i64 {
        0
    }
}

/// Encode a frame body, wrap it in a signed container, and return both the
/// container and the body the signature covers.
///
/// Used when creating a frame locally; received frames keep the bytes they
/// arrived with instead.
pub fn sign_frame<T: Serialize>(key: &DeviceKey, body: &T) -> Result<(SignedContainer, Vec<u8>)> {
    let encoded = msgpack::to_vec(body)?;
    let container = SignedContainer::create(key, encoded.clone());
    Ok((container, encoded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_frame_round_trips_through_a_container() {
        let key = DeviceKey::generate();
        let container = SignedContainer::create(&key, b"body".to_vec());

        let frame = SignedFrame::from_container(&container);
        assert!(frame.is_signed());
        assert_eq!(frame.to_container(), container);
        assert!(frame.to_container().is_valid());
    }

    #[test]
    fn an_unsigned_frame_reports_itself_as_such() {
        assert!(!SignedFrame::default().is_signed());
    }
}

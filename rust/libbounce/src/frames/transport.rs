//! Frames that move other frames around: acknowledgements, the reference flow,
//! and liveness signalling.
//!
//! ## The reference flow
//!
//! Bounce never re-sends a frame a peer already has. When two devices connect,
//! each offers *references* — just a UUID and a type — for every frame the peer
//! is entitled to but has no delivery record for. The peer answers with an
//! [`Ack`] for the ones it already holds and a [`ReferenceRequest`] for the
//! ones it does not, and the originator replies with a [`CatchUp`] carrying
//! only the frames that were actually asked for.
//!
//! ```text
//!   device A                         device B
//!      |-------- ReferenceOffer -------->|   here is what I have
//!      |<------- ReferenceRequest -------|   send me these
//!      |<------- Ack --------------------|   I already had these
//!      |-------- CatchUp --------------->|   the frames themselves
//! ```

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;
use crate::msgpack;
use crate::types::FrameType;

/// A pointer to a frame, small enough to enumerate in bulk.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FrameReference {
    #[serde(skip)]
    pub id: Uuid,

    #[serde(rename = "FrameID")]
    pub frame_id: Uuid,

    #[serde(rename = "Type")]
    pub frame_type: u16,

    /// Which peer this reference concerns; local bookkeeping only.
    #[serde(skip)]
    pub peer: String,

    #[serde(skip)]
    pub state: i64,
    #[serde(skip)]
    pub last_action: i64,
    #[serde(skip)]
    pub created_at: i64,
}

impl FrameReference {
    pub fn new(frame_id: Uuid, frame_type: FrameType) -> Self {
        FrameReference {
            id: Uuid::new_v4(),
            frame_id,
            frame_type: frame_type.as_u16(),
            peer: String::new(),
            state: 0,
            last_action: 0,
            created_at: 0,
        }
    }

    pub fn kind(&self) -> Result<FrameType> {
        FrameType::from_u16(self.frame_type)
    }
}

/// Confirmation that a frame was received and handled.
///
/// Acks are always device-to-device and are the only way delivery is ever
/// established — a device never infers that a peer holds a frame from anything
/// else.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {
    #[serde(rename = "References")]
    #[serde(default, deserialize_with = "crate::msgpack::nullable_seq")]
    pub references: Vec<FrameReference>,
}

impl Ack {
    pub fn single(frame_id: Uuid, frame_type: FrameType) -> Self {
        Ack {
            references: vec![FrameReference::new(frame_id, frame_type)],
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
}

/// Everything the sender holds that the recipient may not.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceOffer {
    #[serde(rename = "ID")]
    pub id: Uuid,
    #[serde(rename = "References")]
    #[serde(default, deserialize_with = "crate::msgpack::nullable_seq")]
    pub references: Vec<FrameReference>,
}

impl ReferenceOffer {
    pub fn new(references: Vec<FrameReference>) -> Self {
        ReferenceOffer {
            id: Uuid::new_v4(),
            references,
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
}

/// The subset of an offer the recipient actually wants.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceRequest {
    #[serde(rename = "References")]
    #[serde(default, deserialize_with = "crate::msgpack::nullable_seq")]
    pub references: Vec<FrameReference>,
}

impl ReferenceRequest {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
}

/// A frame carried inside a catch up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatchUpFrame {
    #[serde(rename = "ID")]
    pub id: Uuid,
    #[serde(rename = "Type")]
    pub frame_type: u16,
    #[serde(rename = "Payload", default, with = "crate::msgpack::nullable_bytes")]
    pub payload: Vec<u8>,
}

/// A batch of frames sent in response to a [`ReferenceRequest`].
///
/// Frames are ordered by when the sender first stored them, with ties broken by
/// [`FrameType::catch_up_order`], so that a replaying device always learns about
/// a user before the messages that user wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatchUp {
    #[serde(rename = "Frames")]
    #[serde(default, deserialize_with = "crate::msgpack::nullable_seq")]
    pub frames: Vec<CatchUpFrame>,
}

impl CatchUp {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }

    /// Order frames for replay.
    ///
    /// `saved_at` is the sender's local storage time for each frame. Frames
    /// whose type has no defined replay position sort last, since nothing
    /// depends on them.
    pub fn sort_for_replay(frames: &mut [(CatchUpFrame, i64)]) {
        frames.sort_by(|(a_frame, a_saved), (b_frame, b_saved)| {
            a_saved.cmp(b_saved).then_with(|| {
                let order = |frame: &CatchUpFrame| {
                    FrameType::from_u16(frame.frame_type)
                        .ok()
                        .and_then(FrameType::catch_up_order)
                        .unwrap_or(u8::MAX)
                };
                order(a_frame).cmp(&order(b_frame))
            })
        });
    }
}

/// A no-op frame that keeps an idle socket from being reaped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeepAlive {}

impl KeepAlive {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
}

/// Sent to a user's own devices to say this device has the interface in the
/// foreground, so the others can suppress duplicate notifications.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveDevice {}

impl ActiveDevice {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
}

/// A record that a specific frame reached a specific device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryRecord {
    pub id: Uuid,
    pub created_at: i64,
    /// Address of the device that acknowledged the frame.
    pub destination: String,
    pub frame_id: Uuid,
    pub frame_type: u16,
}

impl DeliveryRecord {
    pub fn new(destination: String, frame_id: Uuid, frame_type: FrameType, created_at: i64) -> Self {
        DeliveryRecord {
            id: Uuid::new_v4(),
            created_at,
            destination,
            frame_id,
            frame_type: frame_type.as_u16(),
        }
    }
}

/// An explicit list of device addresses that a frame should reach.
///
/// Used when the natural scope of a frame is about to vanish. Deleting a group,
/// for example, removes the group that would have defined the scope, so the
/// membership is snapshotted into a custom scope first and the deletion is
/// addressed to that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomScope {
    pub id: Uuid,
    pub created_at: i64,
    /// Comma-separated device addresses.
    pub addresses: String,
}

impl CustomScope {
    pub fn address_list(&self) -> Vec<String> {
        if self.addresses.is_empty() {
            Vec::new()
        } else {
            self.addresses.split(',').map(|s| s.to_string()).collect()
        }
    }

    pub fn from_addresses(id: Uuid, addresses: &[String], created_at: i64) -> Self {
        CustomScope {
            id,
            created_at,
            addresses: addresses.join(","),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_encode_only_the_pointer() {
        let mut reference = FrameReference::new(Uuid::new_v4(), FrameType::DirectMessage);
        reference.peer = "somepeer".into();
        reference.state = 3;

        let encoded = msgpack::to_vec(&reference).unwrap();
        let text = String::from_utf8_lossy(&encoded);

        assert!(text.contains("FrameID"));
        assert!(text.contains("Type"));
        // Local bookkeeping stays local.
        assert!(!text.contains("Peer"));
        assert!(!text.contains("somepeer"));
        assert!(!text.contains("State"));
    }

    #[test]
    fn ack_round_trips() {
        let frame_id = Uuid::new_v4();
        let ack = Ack::single(frame_id, FrameType::GroupMessage);

        let decoded: Ack = msgpack::from_slice(&ack.encode().unwrap()).unwrap();
        assert_eq!(decoded.references.len(), 1);
        assert_eq!(decoded.references[0].frame_id, frame_id);
        assert_eq!(decoded.references[0].kind().unwrap(), FrameType::GroupMessage);
    }

    #[test]
    fn catch_up_replays_in_storage_order() {
        let frame = |ft: FrameType| CatchUpFrame {
            id: Uuid::new_v4(),
            frame_type: ft.as_u16(),
            payload: vec![],
        };

        let mut frames = vec![
            (frame(FrameType::DirectMessage), 200),
            (frame(FrameType::AddUser), 100),
            (frame(FrameType::GroupMessage), 150),
        ];
        CatchUp::sort_for_replay(&mut frames);

        let order: Vec<u16> = frames.iter().map(|(f, _)| f.frame_type).collect();
        assert_eq!(
            order,
            vec![
                FrameType::AddUser.as_u16(),
                FrameType::GroupMessage.as_u16(),
                FrameType::DirectMessage.as_u16(),
            ]
        );
    }

    #[test]
    fn frames_saved_in_the_same_second_use_the_dependency_order() {
        let frame = |ft: FrameType| CatchUpFrame {
            id: Uuid::new_v4(),
            frame_type: ft.as_u16(),
            payload: vec![],
        };

        // All stored at the same instant: a message must not be replayed before
        // the device that authored it is known.
        let mut frames = vec![
            (frame(FrameType::DirectMessage), 100),
            (frame(FrameType::GroupMessage), 100),
            (frame(FrameType::Device), 100),
            (frame(FrameType::AddUser), 100),
            (frame(FrameType::GroupCreation), 100),
        ];
        CatchUp::sort_for_replay(&mut frames);

        let order: Vec<u16> = frames.iter().map(|(f, _)| f.frame_type).collect();
        assert_eq!(
            order,
            vec![
                FrameType::AddUser.as_u16(),
                FrameType::Device.as_u16(),
                FrameType::DirectMessage.as_u16(),
                FrameType::GroupCreation.as_u16(),
                FrameType::GroupMessage.as_u16(),
            ]
        );
    }

    #[test]
    fn unorderable_types_sort_last_within_their_second() {
        let frame = |ft: u16| CatchUpFrame {
            id: Uuid::new_v4(),
            frame_type: ft,
            payload: vec![],
        };

        let mut frames = vec![
            (frame(FrameType::Ack.as_u16()), 100),
            (frame(FrameType::AddUser.as_u16()), 100),
        ];
        CatchUp::sort_for_replay(&mut frames);
        assert_eq!(frames[0].0.frame_type, FrameType::AddUser.as_u16());
    }

    #[test]
    fn custom_scope_addresses_round_trip() {
        let addresses = vec!["deviceone".to_string(), "devicetwo".to_string()];
        let scope = CustomScope::from_addresses(Uuid::new_v4(), &addresses, 0);
        assert_eq!(scope.address_list(), addresses);

        let empty = CustomScope::from_addresses(Uuid::new_v4(), &[], 0);
        assert!(empty.address_list().is_empty());
    }

    #[test]
    fn empty_frames_encode_as_empty_maps() {
        // Go marshals an empty struct as a zero-length map; both sides must
        // accept it.
        let encoded = KeepAlive {}.encode().unwrap();
        assert_eq!(encoded, vec![0x80]);
        assert_eq!(
            msgpack::from_slice::<KeepAlive>(&encoded).unwrap(),
            KeepAlive {}
        );

        assert_eq!(ActiveDevice {}.encode().unwrap(), vec![0x80]);
    }
}

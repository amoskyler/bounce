//! Chat messages, their attachments, and the ephemeral frames that accompany
//! them.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Broadcastable, SignedFrame};
use crate::error::Result;
use crate::types::{FrameType, Scope};
use crate::{msgpack, xor};

/// A non-image file attached to a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileAttachment {
    #[serde(rename = "ID")]
    pub id: Uuid,
    /// The distributed file this attachment refers to.
    #[serde(rename = "FileID")]
    pub file_id: Uuid,
    #[serde(rename = "MessageID")]
    pub message_id: Uuid,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Size")]
    pub size: i64,
}

/// An image attached to a message.
///
/// Dimensions and a BlurHash travel with the attachment so the interface can
/// reserve the right space and show a placeholder before the image itself has
/// been fetched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageAttachment {
    #[serde(rename = "ID")]
    pub id: Uuid,
    #[serde(rename = "FileID")]
    pub file_id: Uuid,
    #[serde(rename = "MessageID")]
    pub message_id: Uuid,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Size")]
    pub size: i64,
    #[serde(rename = "Width")]
    pub width: i64,
    #[serde(rename = "Height")]
    pub height: i64,
    #[serde(rename = "BlurHash")]
    pub blur_hash: String,
}

/// A message from one user to another.
///
/// The thread is identified by `xor`, the XOR of the two participants' user
/// IDs, rather than by naming a recipient. Either side recovers the
/// counterparty by XORing their own ID back out, and a message to oneself has
/// `xor == nil`, which is what distinguishes a note-to-self (sync scoped) from
/// a conversation (user scoped).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectMessage {
    #[serde(skip)]
    pub signed: SignedFrame,

    #[serde(rename = "ID")]
    pub id: Uuid,

    #[serde(skip)]
    pub saved_at: i64,

    #[serde(rename = "WrittenAt")]
    pub written_at: i64,

    /// When the message expires under the thread's retention policy; zero means
    /// it is kept indefinitely.
    #[serde(rename = "DeleteAt")]
    pub delete_at: i64,

    #[serde(skip)]
    pub seen: bool,

    /// Set once the message has gone [`crate::UNDELIVERABLE_AFTER_SECONDS`]
    /// without a single acknowledgement.
    #[serde(skip)]
    pub undeliverable: bool,

    #[serde(rename = "Author")]
    pub author: Uuid,

    /// XOR of the two users in the conversation.
    #[serde(rename = "Xor")]
    pub xor: Uuid,

    #[serde(rename = "Text")]
    pub text: String,

    #[serde(rename = "FileAttachments")]
    #[serde(default, deserialize_with = "crate::msgpack::nullable_seq")]
    pub file_attachments: Vec<FileAttachment>,

    #[serde(rename = "ImageAttachments")]
    #[serde(default, deserialize_with = "crate::msgpack::nullable_seq")]
    pub image_attachments: Vec<ImageAttachment>,
}

impl DirectMessage {
    /// Create a message from `author` to `recipient`.
    pub fn new(author: Uuid, recipient: Uuid, text: String, written_at: i64) -> Self {
        DirectMessage {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            saved_at: 0,
            written_at,
            delete_at: 0,
            seen: true,
            undeliverable: false,
            author,
            xor: xor(author, recipient),
            text,
            file_attachments: Vec::new(),
            image_attachments: Vec::new(),
        }
    }

    /// A message with no text and no attachments carries nothing and is
    /// rejected on both send and receive.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
            && self.image_attachments.is_empty()
            && self.file_attachments.is_empty()
    }

    /// Whether the body is within the protocol's length limit.
    pub fn text_within_limit(&self) -> bool {
        self.text.chars().count() <= crate::MAXIMUM_MESSAGE_CHARACTERS
    }
}

impl Broadcastable for DirectMessage {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::DirectMessage
    }
    fn payload(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(&self.signed.to_container())
    }
    fn scope(&self, _my_id: Uuid) -> Scope {
        // A note to oneself has no counterparty to XOR out, so it only needs to
        // reach this user's own devices.
        if self.xor.is_nil() {
            Scope::Sync
        } else {
            Scope::User
        }
    }
    fn destination(&self, my_id: Uuid) -> Uuid {
        xor(self.xor, my_id)
    }
    fn author(&self) -> Uuid {
        self.author
    }
    fn timestamp(&self) -> i64 {
        self.written_at
    }
    fn saved_at(&self) -> i64 {
        self.saved_at
    }
}

/// A message to every member of a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMessage {
    #[serde(skip)]
    pub signed: SignedFrame,

    #[serde(rename = "ID")]
    pub id: Uuid,

    #[serde(skip)]
    pub saved_at: i64,

    #[serde(rename = "WrittenAt")]
    pub written_at: i64,

    #[serde(rename = "DeleteAt")]
    pub delete_at: i64,

    #[serde(skip)]
    pub seen: bool,

    #[serde(skip)]
    pub undeliverable: bool,

    #[serde(rename = "Author")]
    pub author: Uuid,

    /// The group this message belongs to.
    #[serde(rename = "Destination")]
    pub destination: Uuid,

    #[serde(rename = "Text")]
    pub text: String,

    #[serde(rename = "FileAttachments")]
    #[serde(default, deserialize_with = "crate::msgpack::nullable_seq")]
    pub file_attachments: Vec<FileAttachment>,

    #[serde(rename = "ImageAttachments")]
    #[serde(default, deserialize_with = "crate::msgpack::nullable_seq")]
    pub image_attachments: Vec<ImageAttachment>,
}

impl GroupMessage {
    pub fn new(author: Uuid, group: Uuid, text: String, written_at: i64) -> Self {
        GroupMessage {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            saved_at: 0,
            written_at,
            delete_at: 0,
            seen: true,
            undeliverable: false,
            author,
            destination: group,
            text,
            file_attachments: Vec::new(),
            image_attachments: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
            && self.image_attachments.is_empty()
            && self.file_attachments.is_empty()
    }

    pub fn text_within_limit(&self) -> bool {
        self.text.chars().count() <= crate::MAXIMUM_MESSAGE_CHARACTERS
    }
}

impl Broadcastable for GroupMessage {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::GroupMessage
    }
    fn payload(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(&self.signed.to_container())
    }
    fn scope(&self, _my_id: Uuid) -> Scope {
        Scope::Group
    }
    fn destination(&self, _my_id: Uuid) -> Uuid {
        self.destination
    }
    fn author(&self) -> Uuid {
        self.author
    }
    fn timestamp(&self) -> i64 {
        self.written_at
    }
    fn saved_at(&self) -> i64 {
        self.saved_at
    }
}

/// Confirmation that a user has read a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadReceipt {
    #[serde(skip)]
    pub signed: SignedFrame,

    #[serde(rename = "ID")]
    pub id: Uuid,

    /// The user who did the reading.
    #[serde(rename = "Actor")]
    pub actor: Uuid,

    /// Resolved locally from the target message rather than sent, so that a
    /// receipt cannot claim to belong to a conversation it does not.
    #[serde(skip)]
    pub destination: Uuid,

    #[serde(skip)]
    pub scope: i64,

    /// The message that was read.
    #[serde(rename = "Target")]
    pub target: Uuid,

    /// Whether the target is a direct or group message.
    #[serde(rename = "TargetType")]
    pub target_type: u16,

    #[serde(rename = "Timestamp")]
    pub timestamp: i64,

    #[serde(skip)]
    pub saved_at: i64,
}

impl Broadcastable for ReadReceipt {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::ReadReceipt
    }
    fn payload(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(&self.signed.to_container())
    }
    fn scope(&self, _my_id: Uuid) -> Scope {
        Scope::from_i64(self.scope).unwrap_or(Scope::Sync)
    }
    fn destination(&self, _my_id: Uuid) -> Uuid {
        self.destination
    }
    fn author(&self) -> Uuid {
        self.actor
    }
    fn timestamp(&self) -> i64 {
        self.timestamp
    }
    fn saved_at(&self) -> i64 {
        self.saved_at
    }
}

/// Notification that a user is composing a message.
///
/// Typing indicators are the one broadcast frame that is never relayed to
/// encrypted devices: they are worthless a second after they are sent, and
/// storing them would leak a fine-grained activity trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypingIndicator {
    #[serde(skip)]
    pub signed: SignedFrame,

    #[serde(rename = "ID")]
    pub id: Uuid,

    /// The conversation being typed in.
    ///
    /// For a group this is the group ID. For a direct message it is the **XOR
    /// of the two users**, the same convention [`DirectMessage::xor`] uses —
    /// not the recipient's ID. Either side recovers the counterparty by XORing
    /// their own ID back out.
    #[serde(rename = "Thread")]
    pub thread: Uuid,

    /// Whether `thread` names a conversation pair or a group.
    #[serde(rename = "MessageType")]
    pub message_type: u16,

    #[serde(rename = "Author")]
    pub author: Uuid,

    /// Stamped on receipt, so a peer cannot backdate an indicator to keep it
    /// alive.
    #[serde(skip)]
    pub received_at: i64,
}

impl Broadcastable for TypingIndicator {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::TypingIndicator
    }
    fn payload(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(&self.signed.to_container())
    }
    fn scope(&self, my_id: Uuid) -> Scope {
        if self.message_type == FrameType::GroupMessage.as_u16() {
            Scope::Group
        } else if self.destination(my_id) == my_id {
            Scope::Sync
        } else {
            Scope::User
        }
    }
    fn destination(&self, my_id: Uuid) -> Uuid {
        if self.message_type == FrameType::GroupMessage.as_u16() {
            self.thread
        } else {
            // `thread` is the XOR pair, so XORing ourselves out leaves the
            // counterparty.
            crate::xor(my_id, self.thread)
        }
    }
    fn author(&self) -> Uuid {
        self.author
    }
    fn timestamp(&self) -> i64 {
        self.received_at
    }
}

/// An unsent message body, synced across a user's own devices so a conversation
/// can be continued wherever it was left.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    #[serde(skip)]
    pub signed: SignedFrame,

    #[serde(rename = "ID")]
    pub id: Uuid,

    #[serde(rename = "Thread")]
    pub thread: Uuid,

    #[serde(rename = "Text")]
    pub text: String,

    #[serde(rename = "Timestamp")]
    pub timestamp: i64,

    /// Whether this draft has been flushed to the database.
    #[serde(rename = "Saved")]
    pub saved: bool,

    #[serde(skip)]
    pub saved_at: i64,
}

impl Broadcastable for Draft {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::Draft
    }
    fn payload(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(&self.signed.to_container())
    }
    fn scope(&self, _my_id: Uuid) -> Scope {
        Scope::Sync
    }
    fn destination(&self, my_id: Uuid) -> Uuid {
        my_id
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_message_thread_is_recoverable_from_either_side() {
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();

        let message = DirectMessage::new(alice, bob, "hi".into(), 100);

        // Alice XORs herself out and sees Bob; Bob does the reverse.
        assert_eq!(message.destination(alice), bob);
        assert_eq!(message.destination(bob), alice);
        assert_eq!(message.scope(alice), Scope::User);
    }

    #[test]
    fn a_note_to_self_stays_within_the_device_group() {
        let me = Uuid::new_v4();
        let message = DirectMessage::new(me, me, "remember this".into(), 100);

        assert!(message.xor.is_nil());
        assert_eq!(message.scope(me), Scope::Sync);
        assert_eq!(message.destination(me), me);
    }

    #[test]
    fn empty_messages_are_recognised() {
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();

        assert!(DirectMessage::new(alice, bob, String::new(), 0).is_empty());
        assert!(DirectMessage::new(alice, bob, "   \t\n ".into(), 0).is_empty());
        assert!(!DirectMessage::new(alice, bob, "text".into(), 0).is_empty());

        // Attachments alone are enough to carry a message.
        let mut attachment_only = DirectMessage::new(alice, bob, String::new(), 0);
        attachment_only.image_attachments.push(ImageAttachment {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            message_id: attachment_only.id,
            name: "photo.jpg".into(),
            size: 1024,
            width: 100,
            height: 100,
            blur_hash: String::new(),
        });
        assert!(!attachment_only.is_empty());
    }

    #[test]
    fn message_length_limit_counts_scalar_values_not_bytes() {
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();

        // Multi-byte characters count once each, so this is within the limit
        // despite being several times the limit in bytes.
        let emoji = "🌍".repeat(crate::MAXIMUM_MESSAGE_CHARACTERS);
        assert!(DirectMessage::new(alice, bob, emoji, 0).text_within_limit());

        let too_long = "a".repeat(crate::MAXIMUM_MESSAGE_CHARACTERS + 1);
        assert!(!DirectMessage::new(alice, bob, too_long, 0).text_within_limit());
    }

    #[test]
    fn direct_message_encodes_only_wire_fields() {
        let mut message = DirectMessage::new(Uuid::new_v4(), Uuid::new_v4(), "hello".into(), 5);
        message.seen = true;
        message.undeliverable = true;
        message.saved_at = 999;

        let encoded = msgpack::to_vec(&message).unwrap();
        let text = String::from_utf8_lossy(&encoded);

        for wire_field in ["ID", "WrittenAt", "DeleteAt", "Author", "Xor", "Text"] {
            assert!(text.contains(wire_field), "missing {wire_field}");
        }
        for local_field in ["SavedAt", "Seen", "Undeliverable"] {
            assert!(!text.contains(local_field), "leaked {local_field}");
        }

        let decoded: DirectMessage = msgpack::from_slice(&encoded).unwrap();
        assert_eq!(decoded.id, message.id);
        assert_eq!(decoded.text, "hello");
        assert!(!decoded.undeliverable);
    }

    #[test]
    fn group_messages_are_group_scoped() {
        let group = Uuid::new_v4();
        let message = GroupMessage::new(Uuid::new_v4(), group, "hi all".into(), 0);

        assert_eq!(message.scope(Uuid::new_v4()), Scope::Group);
        assert_eq!(message.destination(Uuid::new_v4()), group);
    }

    #[test]
    fn typing_indicator_threads_by_xor_like_a_direct_message() {
        // The Go implementation sets Thread to xor(sender, recipient) and
        // recovers the counterparty with xor(myID, Thread). Using the raw
        // recipient ID instead leaves every indicator unroutable between the
        // two implementations, in both directions.
        let me = Uuid::new_v4();
        let other = Uuid::new_v4();

        let mut indicator = TypingIndicator {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            thread: crate::xor(me, other),
            message_type: FrameType::DirectMessage.as_u16(),
            author: me,
            received_at: 0,
        };

        assert_eq!(indicator.scope(me), Scope::User);
        // Either side recovers the other.
        assert_eq!(indicator.destination(me), other);
        assert_eq!(indicator.destination(other), me);

        // Typing in a note-to-self thread only concerns our own devices.
        indicator.thread = Uuid::nil();
        assert_eq!(indicator.destination(me), me);
        assert_eq!(indicator.scope(me), Scope::Sync);

        // Groups name themselves directly.
        let group = Uuid::new_v4();
        indicator.message_type = FrameType::GroupMessage.as_u16();
        indicator.thread = group;
        assert_eq!(indicator.scope(me), Scope::Group);
        assert_eq!(indicator.destination(me), group);
    }

    #[test]
    fn drafts_never_leave_the_device_group() {
        let me = Uuid::new_v4();
        let draft = Draft {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            thread: Uuid::new_v4(),
            text: "half-written".into(),
            timestamp: 1,
            saved: false,
            saved_at: 0,
        };

        assert_eq!(draft.scope(me), Scope::Sync);
        assert_eq!(draft.destination(me), me);
        // Drafts are attributed to no user; they are device state, not content.
        assert_eq!(draft.author(), Uuid::nil());
    }

    #[test]
    fn attachments_survive_encoding() {
        let mut message = GroupMessage::new(Uuid::new_v4(), Uuid::new_v4(), String::new(), 0);
        message.file_attachments.push(FileAttachment {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            message_id: message.id,
            name: "report.pdf".into(),
            size: 4096,
        });
        message.image_attachments.push(ImageAttachment {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            message_id: message.id,
            name: "photo.png".into(),
            size: 2048,
            width: 640,
            height: 480,
            blur_hash: "LKO2".into(),
        });

        let decoded: GroupMessage =
            msgpack::from_slice(&msgpack::to_vec(&message).unwrap()).unwrap();
        assert_eq!(decoded.file_attachments, message.file_attachments);
        assert_eq!(decoded.image_attachments, message.image_attachments);
    }
}

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

/// What kind of message a quote refers to, so a reply to a photo can say so
/// rather than quoting an empty string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum QuoteKind {
    Text = 0,
    Image = 1,
    File = 2,
}

impl QuoteKind {
    pub fn as_u16(self) -> u16 {
        self as u16
    }

    pub fn from_u16(value: u16) -> Self {
        match value {
            1 => QuoteKind::Image,
            2 => QuoteKind::File,
            // Anything unrecognised reads as text. A quote is decoration on a
            // message that is itself perfectly valid, so an unknown kind must
            // not cost the reply.
            _ => QuoteKind::Text,
        }
    }
}

/// The excerpt a reply carries of the message it answers.
///
/// A snapshot rather than a bare id, for two reasons that both come up in
/// ordinary use: a reply can arrive before its target on a catch up, and the
/// target may have expired under a retention policy. Signal carries the snapshot
/// for the first reason; the second is ours.
///
/// This is an added map key. Go decodes the frames it sits on without knowing it
/// exists and relays the original bytes untouched, so it costs no coordination —
/// see `docs/protocol-extensions.md`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quote {
    /// The message being replied to.
    #[serde(rename = "Target")]
    pub target: Uuid,

    #[serde(rename = "Author")]
    pub author: Uuid,

    /// A bounded excerpt, truncated at the source so the renderer never has to
    /// re-derive the limit.
    #[serde(rename = "Text")]
    pub text: String,

    #[serde(rename = "Kind")]
    pub kind: u16,

    /// The **original's** expiry, not the reply's.
    ///
    /// Without this a reply to a thirty-second message re-publishes its text
    /// under the reply's retention, which may be unlimited. Carrying it lets the
    /// recipient blank the quote on schedule and keep the reply.
    #[serde(rename = "ExpiresAt")]
    pub expires_at: i64,
}

impl Quote {
    /// Longest excerpt carried, in characters.
    ///
    /// Two lines of Signal's quote block at its metrics. Counted in characters
    /// rather than bytes for the same reason [`crate::MAXIMUM_MESSAGE_CHARACTERS`]
    /// is: a limit measured in bytes truncates a Japanese quote to a third of an
    /// English one.
    pub const MAXIMUM_TEXT_CHARACTERS: usize = 160;

    /// Build a quote of a message, truncating the excerpt.
    pub fn of(target: Uuid, author: Uuid, text: &str, kind: QuoteKind, expires_at: i64) -> Self {
        Quote {
            target,
            author,
            text: truncate_chars(text, Self::MAXIMUM_TEXT_CHARACTERS),
            kind: kind.as_u16(),
            expires_at,
        }
    }

    pub fn kind(&self) -> QuoteKind {
        QuoteKind::from_u16(self.kind)
    }

    /// Whether the quoted message has expired, so the excerpt must not be shown.
    ///
    /// A zero expiry means the original is kept indefinitely.
    pub fn has_expired(&self, now: i64) -> bool {
        self.expires_at != 0 && self.expires_at <= now
    }

    /// A quote with the excerpt removed, keeping enough to say what is missing.
    ///
    /// Applied in place when the original expires; the reply itself is
    /// untouched.
    pub fn blanked(&self) -> Self {
        Quote {
            target: self.target,
            author: self.author,
            text: String::new(),
            kind: self.kind,
            expires_at: self.expires_at,
        }
    }
}

/// Cut a string to `limit` characters, marking that it was cut.
fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    // One character short, so the ellipsis lands inside the limit rather than
    // pushing the result one over it.
    let mut out: String = text.chars().take(limit.saturating_sub(1)).collect();
    out.push('…');
    out
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

    /// Set when this message is a reply. See [`Quote`].
    #[serde(rename = "Quote", default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<Quote>,

    /// When this message was deleted for everyone, or zero.
    ///
    /// Local, and a tombstone rather than a removal: delete the row and
    /// `has_frame` starts answering false, a peer re-offers the original
    /// through the reference flow, and the message comes back.
    #[serde(skip)]
    pub deleted_at: i64,

    /// Who deleted it — the author, or a group admin. Kept because the three
    /// sentences the interface shows cannot be told apart from the row alone.
    #[serde(skip)]
    pub deleted_by: Uuid,
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
            quote: None,
            deleted_at: 0,
            deleted_by: Uuid::nil(),
        }
    }

    /// A message with no text and no attachments carries nothing and is
    /// rejected on both send and receive.
    ///
    /// A quote does not rescue an otherwise empty message: replying with
    /// nothing is not a message, and Go would refuse it anyway.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
            && self.image_attachments.is_empty()
            && self.file_attachments.is_empty()
    }

    /// Whether this message has been deleted for everyone.
    pub fn is_deleted(&self) -> bool {
        self.deleted_at != 0
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

    /// Set when this message is a reply. See [`Quote`].
    #[serde(rename = "Quote", default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<Quote>,

    #[serde(skip)]
    pub deleted_at: i64,

    #[serde(skip)]
    pub deleted_by: Uuid,
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
            quote: None,
            deleted_at: 0,
            deleted_by: Uuid::nil(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
            && self.image_attachments.is_empty()
            && self.file_attachments.is_empty()
    }

    /// Whether this message has been deleted for everyone.
    pub fn is_deleted(&self) -> bool {
        self.deleted_at != 0
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

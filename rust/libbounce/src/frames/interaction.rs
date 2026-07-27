//! Reacting to a message, and deleting one after it has been sent.
//!
//! Both refer to a message that already exists, so neither can ride it — they
//! need frame types of their own, and they are the only two frames in this crate
//! the Go implementation has no counterpart for. Go closes the connection on a
//! frame type it does not know (`chat/remote_device.go:224`) rather than
//! ignoring it, so both are gated on a capability the receiving device
//! advertises. See [`crate::types::capability`] and
//! `docs/protocol-extensions.md`.
//!
//! Both are shaped on [`super::ReadReceipt`], which is the closest thing that
//! already exists: it names a message by id and type, carries an actor, and
//! **derives its destination and scope from the target rather than carrying
//! them**. That last part is a security property, not a saving — a frame that
//! declared its own conversation could claim one it does not belong to.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Broadcastable, SignedFrame};
use crate::error::Result;
use crate::msgpack;
use crate::types::{FrameType, Scope};

/// How long after writing a message its author may delete it for everyone.
///
/// Signal's `getNormalDeleteMaxAgeMs`, which defaults to a day
/// (`ts/util/getDeleteMaxAgeMs.dom.ts` in Signal Desktop 8.20).
pub const DELETE_WINDOW_SECONDS: i64 = 24 * 60 * 60;

/// How long after writing a message a group admin may delete it.
///
/// Signal's `getAdminDeleteMaxAgeMs`, separate from the author's window because
/// they are configured separately there even though both default to a day.
pub const ADMIN_DELETE_WINDOW_SECONDS: i64 = 24 * 60 * 60;

/// Extra time a *receiver* allows on top of the sending window.
///
/// Signal's `MESSAGE_SEND_GRACE_PERIOD`. A delete frame can sit in a reference
/// queue for as long as a peer is offline, and refusing it on arrival would make
/// deletion unreliable in exactly the case where it matters most — the peer who
/// was not there to see the message withdrawn.
pub const DELETE_GRACE_SECONDS: i64 = 24 * 60 * 60;

/// An emoji reaction to a message.
///
/// One per person per message: a second reaction from the same actor replaces
/// the first, and `remove` withdraws it. That is Signal's rule, and it is what
/// keeps the resolved state a map rather than a log that has to be replayed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reaction {
    #[serde(skip)]
    pub signed: SignedFrame,

    #[serde(rename = "ID")]
    pub id: Uuid,

    /// The person reacting.
    #[serde(rename = "Actor")]
    pub actor: Uuid,

    /// The message reacted to.
    #[serde(rename = "Target")]
    pub target: Uuid,

    /// Whether the target is a direct or a group message.
    #[serde(rename = "TargetType")]
    pub target_type: u16,

    /// One emoji. Empty when `remove` is set.
    #[serde(rename = "Emoji")]
    pub emoji: String,

    /// Withdraw this actor's reaction rather than setting one.
    #[serde(rename = "Remove")]
    pub remove: bool,

    #[serde(rename = "Timestamp")]
    pub timestamp: i64,

    /// Resolved locally from the target message rather than carried.
    #[serde(skip)]
    pub destination: Uuid,

    #[serde(skip)]
    pub scope: i64,

    /// Copied from the target, so a reaction cannot outlive what it is attached
    /// to. Zero when the target is kept indefinitely.
    #[serde(skip)]
    pub delete_at: i64,

    #[serde(skip)]
    pub saved_at: i64,
}

impl Reaction {
    /// The longest an emoji may be in bytes.
    ///
    /// A single emoji can legitimately be long — a family with four skin tones
    /// and joiners runs past thirty bytes — so this is a sanity bound rather
    /// than a shape check. [`is_valid_emoji`] does the real work.
    pub const MAXIMUM_EMOJI_BYTES: usize = 64;

    pub fn set(actor: Uuid, target: Uuid, target_type: FrameType, emoji: String, timestamp: i64) -> Self {
        Reaction {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            actor,
            target,
            target_type: target_type.as_u16(),
            emoji,
            remove: false,
            timestamp,
            destination: Uuid::nil(),
            scope: Scope::Sync.as_i64(),
            delete_at: 0,
            saved_at: 0,
        }
    }

    pub fn clear(actor: Uuid, target: Uuid, target_type: FrameType, timestamp: i64) -> Self {
        Reaction {
            emoji: String::new(),
            remove: true,
            ..Reaction::set(actor, target, target_type, String::new(), timestamp)
        }
    }

    /// Whether the payload is the right shape.
    ///
    /// Checked before anything is stored. This is the only place a remote peer
    /// chooses characters that end up rendered in the message list at a size we
    /// picked, so an unvalidated string here is arbitrary content injection, not
    /// a cosmetic problem.
    pub fn has_valid_payload(&self) -> bool {
        if self.remove {
            return self.emoji.is_empty();
        }
        self.emoji.len() <= Self::MAXIMUM_EMOJI_BYTES && is_valid_emoji(&self.emoji)
    }
}

/// Whether a string is exactly one emoji.
///
/// Deliberately structural rather than a table lookup: it accepts a run of
/// characters that are all drawn from the sets emoji are built out of — pictographs,
/// modifiers, joiners, variation selectors, regional indicators, keycaps — and
/// rejects anything containing a character that is not. That admits a
/// nonsensical sequence a real keyboard would not produce, and refuses every
/// letter, digit, space and control character, which is what the check is for.
/// A table would have to be regenerated with each Unicode release to stay
/// correct, and would fail closed on a new emoji rather than an old attack.
///
/// **The joiner is named individually, not taken from its block.** An earlier
/// draft admitted the whole of General Punctuation (U+2000–206F) "for the
/// joiner", which also admitted U+202E RIGHT-TO-LEFT OVERRIDE — a character
/// whose entire function is to reverse the display of the text around it, in a
/// pill rendered next to a message. Blocks are the wrong granularity here;
/// every non-pictographic character this accepts is listed by hand.
pub fn is_valid_emoji(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }

    let mut saw_pictograph = false;
    for character in text.chars() {
        let code = character as u32;
        let pictographic = matches!(code,
            // The emoji planes, which already contain the regional indicators
            // at 1F1E6..1F1FF that flags are built from.
            0x1F000..=0x1FAFF
            | 0x2600..=0x27BF   // miscellaneous symbols and dingbats
            | 0x2B00..=0x2BFF   // arrows and geometric shapes
            | 0x2190..=0x21FF   // arrows
            | 0x2300..=0x23FF   // technical, which is where ⌚ and ⏰ live
            | 0x25A0..=0x25FF   // geometric shapes
            | 0x00A9 | 0x00AE   // © and ®
        );
        let modifier = matches!(code,
            0x200D              // zero-width joiner
            | 0xFE0E | 0xFE0F   // variation selectors
            | 0x20E3            // combining keycap
            | 0x1F3FB..=0x1F3FF // skin tone modifiers
            | 0xE0020..=0xE007F // tag characters, for subdivision flags
            | 0x0030..=0x0039   // digits, but only as a keycap base
            | 0x0023 | 0x002A   // # and *, likewise
        );

        if !pictographic && !modifier {
            return false;
        }
        if pictographic && code != 0x200D {
            saw_pictograph = true;
        }
    }

    // A string of nothing but joiners and digits is not an emoji. Requiring at
    // least one pictograph is what stops "123" and "#" passing as keycap bases.
    saw_pictograph || text.chars().any(|c| c as u32 == 0x20E3)
}

impl Broadcastable for Reaction {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::Reaction
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

/// Withdrawal of a message that has already been sent.
///
/// The effect on the target is a tombstone, never a removal — see
/// `Store::delete_message_for_everyone`. Deleting the row would make the
/// deletion undo itself: `has_frame` starts answering false, the peer
/// classifies the original as wanted, offers it back, and the message returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteMessage {
    #[serde(skip)]
    pub signed: SignedFrame,

    #[serde(rename = "ID")]
    pub id: Uuid,

    /// The message's author, or a group admin acting as one.
    #[serde(rename = "Actor")]
    pub actor: Uuid,

    #[serde(rename = "Target")]
    pub target: Uuid,

    #[serde(rename = "TargetType")]
    pub target_type: u16,

    /// Set when the actor is deleting somebody else's message by virtue of
    /// being an admin. Carried rather than inferred so the receiving side knows
    /// which window and which permission check to apply, and so the interface
    /// can say "an admin removed this" rather than naming the author.
    #[serde(rename = "AdminDelete")]
    pub admin_delete: bool,

    #[serde(rename = "Timestamp")]
    pub timestamp: i64,

    #[serde(skip)]
    pub destination: Uuid,

    #[serde(skip)]
    pub scope: i64,

    #[serde(skip)]
    pub saved_at: i64,
}

impl DeleteMessage {
    pub fn new(actor: Uuid, target: Uuid, target_type: FrameType, admin: bool, timestamp: i64) -> Self {
        DeleteMessage {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            actor,
            target,
            target_type: target_type.as_u16(),
            admin_delete: admin,
            timestamp,
            destination: Uuid::nil(),
            scope: Scope::Sync.as_i64(),
            saved_at: 0,
        }
    }
}

/// Whether a message is still young enough to be deleted for everyone.
///
/// `written_at` is the target's, `now` the moment being tested. Both in seconds.
///
/// Signal derives message age from a server timestamp; there is none here, so
/// this uses the target's own `written_at`. That is author-controlled, so an
/// author can backdate a message to widen their own window — a limitation worth
/// knowing about and not worth defending against, since the alternative is a
/// timestamp nobody can establish. It does not extend to the admin case, where
/// the actor is not the author and the timestamp is not theirs.
pub fn within_delete_window(written_at: i64, now: i64, admin: bool, receiving: bool) -> bool {
    let window = if admin {
        ADMIN_DELETE_WINDOW_SECONDS
    } else {
        DELETE_WINDOW_SECONDS
    };
    // A receiver is deliberately more permissive than a sender, because the
    // frame may have spent the difference sitting in a queue.
    let grace = if receiving { DELETE_GRACE_SECONDS } else { 0 };
    now.saturating_sub(written_at) <= window + grace
}

impl Broadcastable for DeleteMessage {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::DeleteMessage
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reaction_derives_its_conversation_rather_than_carrying_it() {
        let me = Uuid::new_v4();
        let target = Uuid::new_v4();
        let mut reaction = Reaction::set(me, target, FrameType::DirectMessage, "👍".into(), 10);

        // Nothing about the conversation is on the wire.
        let encoded = msgpack::to_vec(&reaction).unwrap();
        let text = String::from_utf8_lossy(&encoded);
        for wire_field in ["ID", "Actor", "Target", "TargetType", "Emoji", "Remove"] {
            assert!(text.contains(wire_field), "missing {wire_field}");
        }
        for local_field in ["Destination", "Scope", "DeleteAt", "SavedAt"] {
            assert!(!text.contains(local_field), "leaked {local_field}");
        }

        // And what is resolved locally is what routing uses.
        let counterparty = Uuid::new_v4();
        reaction.destination = counterparty;
        reaction.scope = Scope::User.as_i64();
        assert_eq!(reaction.destination(me), counterparty);
        assert_eq!(reaction.scope(me), Scope::User);
    }

    #[test]
    fn a_reaction_round_trips() {
        let reaction = Reaction::set(Uuid::new_v4(), Uuid::new_v4(), FrameType::GroupMessage, "🎉".into(), 7);
        let decoded: Reaction = msgpack::from_slice(&msgpack::to_vec(&reaction).unwrap()).unwrap();

        assert_eq!(decoded.emoji, "🎉");
        assert_eq!(decoded.target, reaction.target);
        assert_eq!(decoded.target_type, FrameType::GroupMessage.as_u16());
        assert!(!decoded.remove);
    }

    #[test]
    fn emoji_validation_accepts_real_emoji_and_refuses_text() {
        for good in [
            "👍",          // one code point
            "🎉",
            "❤️",          // with a variation selector
            "👍🏽",          // with a skin tone
            "👩‍👩‍👧‍👦",  // joined sequence
            "🇬🇧",          // regional indicator pair
            "⌚",          // outside the emoji planes
            "©️",
        ] {
            assert!(is_valid_emoji(good), "rejected {good:?}");
        }

        for bad in [
            "",
            "a",
            "no",
            "👍 ",              // trailing space
            "👍a",              // an emoji and a letter
            "<script>",
            "\u{202E}",          // a right-to-left override on its own
            "👍\u{202E}",         // and smuggled in behind a real emoji
            "\u{2066}",          // left-to-right isolate
            "\u{200B}",          // zero-width space
            "\u{2028}",          // line separator
            "123",               // keycap bases with no keycap
            "\u{200D}",          // a lone joiner
        ] {
            assert!(!is_valid_emoji(bad), "accepted {bad:?}");
        }
    }

    #[test]
    fn a_removal_carries_no_emoji() {
        let clear = Reaction::clear(Uuid::new_v4(), Uuid::new_v4(), FrameType::DirectMessage, 1);
        assert!(clear.remove);
        assert!(clear.has_valid_payload());

        // A removal that also names an emoji is malformed: it is ambiguous
        // whether it sets or clears, and the two have opposite effects.
        let mut confused = clear.clone();
        confused.emoji = "👍".into();
        assert!(!confused.has_valid_payload());

        // As is a set with nothing to set.
        let mut empty = Reaction::set(Uuid::new_v4(), Uuid::new_v4(), FrameType::DirectMessage, String::new(), 1);
        empty.remove = false;
        assert!(!empty.has_valid_payload());
    }

    #[test]
    fn the_delete_window_is_a_day_and_the_receiver_allows_two() {
        let written = 1_000_000;
        let hour = 60 * 60;

        // Sending.
        assert!(within_delete_window(written, written + 23 * hour, false, false));
        assert!(!within_delete_window(written, written + 25 * hour, false, false));

        // Receiving the same frame: a day of grace on top, because it may have
        // been queued for a peer that was offline.
        assert!(within_delete_window(written, written + 25 * hour, false, true));
        assert!(within_delete_window(written, written + 47 * hour, false, true));
        assert!(!within_delete_window(written, written + 49 * hour, false, true));

        // Admins get their own window, configured separately even though the
        // two currently agree.
        assert!(within_delete_window(written, written + 23 * hour, true, false));
        assert!(!within_delete_window(written, written + 25 * hour, true, false));
    }

    #[test]
    fn a_clock_that_runs_backwards_does_not_open_the_window_forever() {
        // `now` before `written_at` means one of the two clocks is wrong.
        // Saturating rather than wrapping keeps that from becoming a huge
        // positive age, which would slam the window shut instead of leaving it
        // open — the safe direction is the one that still allows the delete.
        let written = 1_000_000;
        assert!(within_delete_window(written, written - 10_000, false, false));
    }

    #[test]
    fn a_delete_encodes_only_wire_fields() {
        let mut delete = DeleteMessage::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            FrameType::GroupMessage,
            true,
            42,
        );
        delete.destination = Uuid::new_v4();
        delete.scope = Scope::Group.as_i64();

        let encoded = msgpack::to_vec(&delete).unwrap();
        let text = String::from_utf8_lossy(&encoded);
        for wire_field in ["ID", "Actor", "Target", "TargetType", "AdminDelete", "Timestamp"] {
            assert!(text.contains(wire_field), "missing {wire_field}");
        }
        assert!(!text.contains("Destination"));
        assert!(!text.contains("Scope"));

        let decoded: DeleteMessage = msgpack::from_slice(&encoded).unwrap();
        assert!(decoded.admin_delete);
        assert_eq!(decoded.target, delete.target);
        // Routing state does not survive the wire; it is re-derived on receipt.
        assert!(decoded.destination.is_nil());
    }
}

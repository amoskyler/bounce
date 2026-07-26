//! Events the engine emits to whatever is driving the interface.
//!
//! The Go implementation calls into a `UI` interface with about seventy
//! methods. That works when the interface lives in the same process, but the
//! Electron client is on the other side of an N-API boundary, so the same
//! information is modelled here as a serialisable enum delivered over a
//! channel. One variant per thing the interface needs to react to.
//!
//! Events are *facts about state that already changed*, never requests. By the
//! time an event is emitted the database has been written, so a client can
//! render it without asking anything back.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::settings::SettingsView;

/// A snapshot of a user, as the interface needs it.
///
/// The per-conversation settings below are carried for the same reason
/// [`GroupView`] carries them: every one of these controls is *seeded* from the
/// current value, so a view that omits one leaves its widget stuck on the
/// default. A retention selector that always reads "Off" is not a cosmetic
/// defect — it tells the user their messages are kept when they are expiring,
/// or the reverse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserView {
    pub id: Uuid,
    pub name: String,
    /// The locally chosen alias, which takes precedence when set.
    pub alias: String,
    pub images: Vec<Uuid>,
    pub blocked: bool,
    pub accepted: bool,
    pub introduction_time: i64,
    pub last_activity: i64,
    pub muted_until: i64,
    /// How long messages in this thread are kept, in seconds; zero keeps them
    /// indefinitely.
    pub retention: i64,
    /// Messages written before this are gone, on every device in the thread.
    pub clear_before: i64,
    /// Whether the conversation is on the thread list. A contact exists before
    /// there is anything to show for them.
    pub open_dm: bool,
    /// Private notes about the contact, never sent to them.
    pub notes: String,
    /// Whether this conversation overrides the profile-wide read receipt
    /// setting, and what it overrides it to. Two fields rather than an
    /// `Option<bool>` because that is the shape the wire and the store already
    /// use — see `UpdateDmType::SetReadReceipts`, which encodes exactly these
    /// two bytes.
    pub read_receipts_overridden: bool,
    pub read_receipts_enabled: bool,
    pub typing_indicators_overridden: bool,
    pub typing_indicators_enabled: bool,
    /// When this conversation was last opened, which is what keeps a thread
    /// with an unsent draft from sinking down the list.
    pub last_opened: i64,
    pub online: bool,
}

/// A snapshot of a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupView {
    pub id: Uuid,
    pub name: String,
    pub images: Vec<Uuid>,
    pub members: Vec<Uuid>,
    pub admins: Vec<Uuid>,
    pub invites: Vec<Uuid>,
    pub created_by: Uuid,
    pub created_at: i64,
    pub last_activity: i64,
    pub muted_until: i64,
    pub retention: i64,
    pub last_opened: i64,
    pub restrict_posting: bool,
    pub restrict_group_edits: bool,
    pub restrict_user_management: bool,
}

/// A snapshot of a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub id: Uuid,
    pub name: String,
    pub address: String,
    pub created_at: i64,
    pub last_seen: i64,
    /// True for the device this engine is running on.
    pub local: bool,
    pub online: bool,
    pub revoked: bool,
}

/// An attachment, as rendered in a message bubble.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentView {
    pub id: Uuid,
    pub file_id: Uuid,
    pub name: String,
    pub size: i64,
    /// Present for images.
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub blur_hash: Option<String>,
    /// Fraction downloaded, from zero to one.
    pub progress: f64,
}

/// A message, either direct or in a group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    pub id: Uuid,
    /// The conversation: a user ID for a direct message, a group ID otherwise.
    pub thread: Uuid,
    pub author: Uuid,
    pub text: String,
    pub written_at: i64,
    /// When the message expires under the thread's retention policy; zero means
    /// it is kept indefinitely.
    pub expires_at: i64,
    pub seen: bool,
    /// Set once we have given up trying to deliver it.
    pub undeliverable: bool,
    /// Users known to have received it.
    pub delivered_to: Vec<Uuid>,
    /// Users known to have read it.
    pub read_by: Vec<Uuid>,
    pub attachments: Vec<AttachmentView>,
    /// Whether this device's owner wrote it.
    pub outgoing: bool,
}

/// A status change in a conversation: a rename, an invitation, a departure.
///
/// It carries a kind and the ids involved, never a finished sentence. The
/// wording belongs to the client — see [`crate::engine::system`] for why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemMessageView {
    pub id: Uuid,
    /// The conversation it belongs to, threaded exactly like a message.
    pub thread: Uuid,
    /// Who made the change.
    pub actor: Uuid,
    /// One of a fixed set of kinds; see `SYSTEM_MESSAGE_KINDS` on the client.
    pub kind: String,
    /// The user the change is about, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Free text the sentence needs: a new group name, a retention label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub timestamp: i64,
}

/// Everything the interface needs on startup, so it can render without a
/// round-trip per conversation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialState {
    pub profile: Option<UserView>,
    pub network_online: bool,
    pub device_revoked: bool,
    pub sync_devices: Vec<DeviceView>,
    pub users: Vec<UserView>,
    pub groups: Vec<GroupView>,
    pub messages: Vec<MessageView>,
    pub system_messages: Vec<SystemMessageView>,
    pub drafts: Vec<DraftView>,
}

/// An unsent message body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftView {
    pub thread: Uuid,
    pub text: String,
}

/// Something that happened, which the interface should reflect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Event {
    /// The full state, emitted once when a client attaches.
    Ready { state: Box<InitialState> },

    /// The overlay network became reachable, or stopped being reachable.
    NetworkOnline,
    NetworkOffline,

    /// A profile was created on this device.
    ProfileCreated { user: UserView, device: DeviceView },

    /// A profile-wide setting changed. The whole set is carried, so a client
    /// replaces its copy rather than patching one field.
    SettingsUpdated { settings: SettingsView },

    /// A message arrived or was sent.
    MessageReceived { message: Box<MessageView> },
    MessageSent { message: Box<MessageView> },

    /// Something happened in a conversation that the timeline records: a
    /// rename, an invitation, a departure.
    SystemMessage { message: SystemMessageView },

    /// A message reached one of a user's devices.
    MessageDelivered { message_id: Uuid, user_id: Uuid },
    /// Someone read one of our messages.
    MessageRead { message_id: Uuid, user_id: Uuid },
    /// We read a message on another of our own devices.
    MessageSeen { message_id: Uuid },
    /// Delivery was abandoned.
    MessageUndeliverable { message_id: Uuid },
    /// A message expired or was deleted.
    MessageDeleted { message_id: Uuid },

    /// Someone is composing a message.
    TypingStarted { user_id: Uuid, thread: Uuid },
    TypingStopped { user_id: Uuid, thread: Uuid },

    /// A contact was added, or their profile changed.
    UserAdded { user: UserView },
    UserUpdated { user: UserView },
    UserOnline { user_id: Uuid },
    UserOffline { user_id: Uuid },

    /// A group was created, or its state was recomputed.
    GroupUpdated { group: Box<GroupView> },
    /// We were removed from a group, or it was deleted.
    GroupRemoved { group_id: Uuid, actor: Uuid },

    /// A device joined, left, or changed within our own device group.
    DeviceAdded { device: DeviceView },
    DeviceUpdated { device: DeviceView },
    DeviceOnline { device_id: Uuid },
    DeviceOffline { device_id: Uuid },

    /// A draft changed on another of our devices.
    DraftUpdated { draft: DraftView },

    /// Catching up with a peer began, progressed, or finished. The interface
    /// uses this to avoid replaying a month of history one frame at a time.
    SyncStarted,
    SyncProgress { fraction: f64 },
    SyncComplete,

    /// A file finished downloading, or made progress.
    FileProgress { file_id: Uuid, fraction: f64 },
    FileComplete { file_id: Uuid },

    /// Something went wrong that the interface should surface.
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_fields_are_camel_case() {
        // The TypeScript side reads camelCase. Rust's snake_case would arrive
        // as `undefined` on every field — and because the variant tag is
        // renamed separately, checking only `type` would not catch it.
        let event = Event::MessageDelivered {
            message_id: Uuid::nil(),
            user_id: Uuid::nil(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert!(json.get("messageId").is_some(), "got {json}");
        assert!(json.get("userId").is_some(), "got {json}");
        assert!(json.get("message_id").is_none());
    }

    #[test]
    fn view_struct_fields_are_camel_case() {
        let view = MessageView {
            id: Uuid::nil(),
            thread: Uuid::nil(),
            author: Uuid::nil(),
            text: "hello".into(),
            written_at: 7,
            expires_at: 0,
            seen: false,
            undeliverable: false,
            delivered_to: vec![],
            read_by: vec![],
            attachments: vec![],
            outgoing: true,
        };
        let json = serde_json::to_value(&view).unwrap();

        for field in ["writtenAt", "expiresAt", "deliveredTo", "readBy"] {
            assert!(json.get(field).is_some(), "missing {field} in {json}");
        }
        assert!(json.get("written_at").is_none());

        // And the nested case: a whole snapshot.
        let state = InitialState {
            profile: None,
            network_online: true,
            device_revoked: false,
            sync_devices: vec![],
            users: vec![],
            groups: vec![],
            messages: vec![view],
            system_messages: vec![],
            drafts: vec![],
        };
        let json = serde_json::to_value(&state).unwrap();
        assert!(json.get("networkOnline").is_some(), "got {json}");
        assert!(json.get("syncDevices").is_some());
        assert!(json.get("systemMessages").is_some());
        assert!(json["messages"][0].get("writtenAt").is_some());
    }

    #[test]
    fn a_user_carries_the_settings_its_controls_are_seeded_from() {
        // Each of these drives a widget the client renders as a *controlled*
        // input: absent the value, the widget reads its default, and choosing
        // anything else snaps straight back. The retention selector spent this
        // whole gap reading "Off" for conversations that were expiring.
        let view = UserView {
            id: Uuid::nil(),
            name: "Alice".into(),
            alias: String::new(),
            images: vec![],
            blocked: false,
            accepted: true,
            introduction_time: 0,
            last_activity: 0,
            muted_until: 0,
            retention: 3600,
            clear_before: 12,
            open_dm: true,
            notes: "met at the conference".into(),
            read_receipts_overridden: true,
            read_receipts_enabled: false,
            typing_indicators_overridden: false,
            typing_indicators_enabled: true,
            last_opened: 90,
            online: false,
        };
        let json = serde_json::to_value(&view).unwrap();

        assert_eq!(json["retention"], 3600);
        assert_eq!(json["lastOpened"], 90);
        assert_eq!(json["clearBefore"], 12);
        assert_eq!(json["openDm"], true);
        assert_eq!(json["notes"], "met at the conference");
        assert_eq!(json["readReceiptsOverridden"], true);
        assert_eq!(json["readReceiptsEnabled"], false);
        assert_eq!(json["typingIndicatorsOverridden"], false);
        assert_eq!(json["typingIndicatorsEnabled"], true);
        assert!(json.get("clear_before").is_none(), "got {json}");
    }

    #[test]
    fn a_system_message_omits_the_fields_it_has_no_value_for() {
        // The client's type marks `subject` and `value` optional, and reads a
        // missing one as "this kind does not use it". Serialising them as null
        // would still be falsy in JavaScript, but an empty string would not,
        // so they are left out entirely.
        let event = Event::SystemMessage {
            message: SystemMessageView {
                id: Uuid::nil(),
                thread: Uuid::nil(),
                actor: Uuid::nil(),
                kind: "userLeft".into(),
                subject: None,
                value: None,
                timestamp: 0,
            },
        };
        let json = serde_json::to_value(&event).unwrap();

        assert_eq!(json["type"], "systemMessage");
        assert!(json["message"].get("subject").is_none(), "got {json}");
        assert!(json["message"].get("value").is_none());
    }

    #[test]
    fn events_serialize_with_a_discriminating_tag() {
        // The client dispatches on `type`, so the tag has to be present and
        // stable.
        let event = Event::MessageDelivered {
            message_id: Uuid::nil(),
            user_id: Uuid::nil(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "messageDelivered");
    }

    #[test]
    fn unit_events_still_carry_their_tag() {
        let json = serde_json::to_value(Event::NetworkOnline).unwrap();
        assert_eq!(json["type"], "networkOnline");
    }

    #[test]
    fn events_round_trip() {
        let event = Event::TypingStarted {
            user_id: Uuid::new_v4(),
            thread: Uuid::new_v4(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), event);
    }
}

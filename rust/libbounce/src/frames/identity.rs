//! Devices, users, and the signatures that bind devices into a device group.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::Broadcastable;
use crate::error::Result;
use crate::msgpack;
use crate::types::{FrameType, Scope};

/// The pair of signatures that admits a device into a device group.
///
/// A device joins by exchanging signatures with one device that is already a
/// member: the existing device signs the newcomer's address (consenting to add
/// it) and the newcomer signs the existing device's address (consenting to be
/// added). Both signatures travel with the device forever, because they are
/// what any third party checks to decide the group is genuine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntroductionSignature {
    #[serde(rename = "ID")]
    pub id: Uuid,
    #[serde(rename = "DeviceID")]
    pub device_id: Uuid,
    /// Address of the device already in the group that admitted this one.
    #[serde(rename = "PreexistingDevice")]
    pub preexisting_device: String,
    /// The existing device's signature over the new device's address.
    #[serde(rename = "SignatureOfNewDevice", default, with = "crate::msgpack::nullable_bytes")]
    pub signature_of_new_device: Vec<u8>,
    /// The new device's signature over the existing device's address.
    #[serde(rename = "SignatureOfPreexistingDevice", default, with = "crate::msgpack::nullable_bytes")]
    pub signature_of_preexisting_device: Vec<u8>,
}

/// One instance of Bounce.
///
/// A device's `address` is its onion service ID, which is also the encoding of
/// its Ed25519 public key — so the address alone is enough to verify anything
/// the device signs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    #[serde(rename = "ID")]
    pub id: Uuid,

    /// Local label for the device. Names are chosen by the owner and are not
    /// shared with other users, so this never goes on the wire.
    #[serde(skip)]
    pub name: String,

    #[serde(rename = "UserID")]
    pub user_id: Uuid,

    /// Onion service ID; the device's identity.
    #[serde(rename = "Address")]
    pub address: String,

    /// When the device was created, per its own clock.
    #[serde(rename = "Timestamp")]
    pub timestamp: i64,

    #[serde(skip)]
    pub saved_at: i64,

    #[serde(skip)]
    pub last_seen: i64,

    /// Zero if active; otherwise the time the device was revoked. Revoked
    /// devices are retained forever, because their keys may still be needed to
    /// validate historic signatures in the device group.
    #[serde(rename = "RevokedAt")]
    pub revoked_at: i64,

    /// Per-device X25519 keys, used only to deliver re-keying frames through an
    /// encrypted device. Private material never leaves this device.
    #[serde(skip)]
    pub ecdh_public_key: Vec<u8>,
    #[serde(skip)]
    pub ecdh_private_key: Vec<u8>,

    /// Absent only on the founding device of a device group.
    #[serde(rename = "Signature")]
    pub signature: Option<IntroductionSignature>,

    /// Protocol extensions this device understands.
    ///
    /// An added key, which every build that predates it decodes as absent — and
    /// absent is exactly the right default, because those are the builds that
    /// cannot handle the frames it gates. The Go implementation ignores the key
    /// and relays the record byte for byte inside `User`, so a Rust device's
    /// capabilities reach another Rust peer even through a Go intermediary.
    ///
    /// See [`crate::types::capability`] and `docs/protocol-extensions.md`.
    #[serde(
        rename = "Capabilities",
        default,
        deserialize_with = "crate::msgpack::nullable_seq"
    )]
    pub capabilities: Vec<String>,
}

impl Device {
    pub fn new(id: Uuid, user_id: Uuid, address: String, timestamp: i64) -> Self {
        Device {
            id,
            name: String::new(),
            user_id,
            address,
            timestamp,
            saved_at: 0,
            last_seen: 0,
            revoked_at: 0,
            ecdh_public_key: Vec::new(),
            ecdh_private_key: Vec::new(),
            signature: None,
            // Every device this build creates speaks everything this build
            // speaks. Devices already in the database predate the field and
            // read as legacy until they re-announce, which fails towards
            // sending less rather than more.
            capabilities: crate::types::capability::SUPPORTED
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        }
    }

    /// Whether this device advertises understanding a protocol extension.
    pub fn supports(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|held| held == capability)
    }

    /// Whether it is safe to send this frame type to this device.
    ///
    /// Types that predate the extensions are always safe. An extension is only
    /// safe once the device has said so, because the cost of guessing wrong is
    /// not a dropped frame — Go closes the connection, and the reference flow
    /// then re-offers the frame on every reconnection.
    pub fn accepts(&self, frame_type: FrameType) -> bool {
        match frame_type.capability() {
            None => true,
            Some(capability) => self.supports(capability),
        }
    }

    pub fn is_revoked(&self) -> bool {
        self.revoked_at != 0
    }

    /// Whether the device was already revoked at the given time. Frames signed
    /// after a device's revocation are rejected; frames signed before it remain
    /// valid.
    pub fn was_revoked_at(&self, timestamp: i64) -> bool {
        self.revoked_at != 0 && self.revoked_at <= timestamp
    }
}

impl Broadcastable for Device {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::Device
    }
    fn payload(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
    fn scope(&self, _my_id: Uuid) -> Scope {
        Scope::Global
    }
    fn destination(&self, _my_id: Uuid) -> Uuid {
        Uuid::nil()
    }
    fn author(&self) -> Uuid {
        self.user_id
    }
    fn timestamp(&self) -> i64 {
        self.timestamp
    }
    fn saved_at(&self) -> i64 {
        self.saved_at
    }
}

/// Settings that apply to the whole profile rather than one conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileSettings {
    #[serde(skip)]
    pub id: Uuid,
    #[serde(rename = "UserID")]
    pub user_id: Uuid,
    /// Comma-separated group IDs the user has blocked.
    #[serde(rename = "BlockedGroups")]
    pub blocked_groups: String,
    #[serde(rename = "DefaultGroupRetention")]
    pub default_group_retention: i64,
    #[serde(rename = "DefaultSendReadReceipts")]
    pub default_send_read_receipts: bool,
    #[serde(rename = "DefaultSendTypingIndicators")]
    pub default_send_typing_indicators: bool,
    #[serde(rename = "NewGroupRestrictUserManagement")]
    pub new_group_restrict_user_management: bool,
    #[serde(rename = "NewGroupRestrictGroupEdits")]
    pub new_group_restrict_group_edits: bool,
    #[serde(rename = "NewGroupRestrictPosting")]
    pub new_group_restrict_posting: bool,
    #[serde(rename = "AutoJoinGroups")]
    pub auto_join_groups: i64,
    #[serde(rename = "DefaultDMRetention")]
    pub default_dm_retention: i64,
}

impl ProfileSettings {
    /// The defaults a freshly created profile starts with: four weeks of
    /// retention, receipts and typing indicators on, and new groups restricting
    /// user management to admins.
    pub fn defaults(user_id: Uuid) -> Self {
        const FOUR_WEEKS: i64 = 24 * 60 * 60 * 7 * 4;
        ProfileSettings {
            id: Uuid::new_v4(),
            user_id,
            blocked_groups: String::new(),
            default_group_retention: FOUR_WEEKS,
            default_send_read_receipts: true,
            default_send_typing_indicators: true,
            new_group_restrict_user_management: true,
            new_group_restrict_group_edits: false,
            new_group_restrict_posting: false,
            auto_join_groups: 0,
            default_dm_retention: FOUR_WEEKS,
        }
    }
}

/// A person, represented as the set of devices they own.
///
/// There is no global namespace: a `User` is only ever learned through a direct
/// introduction or a shared group, and the fields marked `#[serde(skip)]` are
/// this device's private view of them (aliases, notes, mute state) which is
/// never shared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    #[serde(rename = "ID")]
    pub id: Uuid,

    #[serde(rename = "Name")]
    pub name: String,

    /// Comma-separated history of profile image file IDs, most recent last.
    #[serde(rename = "Images")]
    pub images: String,

    /// True for the one user that is this device's owner.
    #[serde(skip)]
    pub profile: bool,

    /// Comma-separated addresses of encrypted devices this user owns.
    #[serde(rename = "EncryptedDevices")]
    pub encrypted_devices: String,

    /// Ed25519 keys at the user level, distinct from per-device Tor keys.
    #[serde(skip)]
    pub public_ecdsa_key: Vec<u8>,
    #[serde(skip)]
    pub private_ecdsa_key: Vec<u8>,

    /// X25519 public key, shared so others can encrypt to this user when
    /// relaying through an encrypted device. Rolled whenever a device is
    /// revoked.
    #[serde(rename = "PublicECDHKey", default, with = "crate::msgpack::nullable_bytes")]
    pub public_ecdh_key: Vec<u8>,

    #[serde(skip)]
    pub private_ecdh_key: Vec<u8>,

    #[serde(skip)]
    pub open_dm: bool,
    #[serde(skip)]
    pub last_opened: i64,
    #[serde(skip)]
    pub retention: i64,
    #[serde(skip)]
    pub clear_before: i64,
    #[serde(skip)]
    pub muted_until: i64,
    #[serde(skip)]
    pub last_activity: i64,
    #[serde(skip)]
    pub read_receipts_overridden: bool,
    #[serde(skip)]
    pub read_receipts_enabled: bool,
    #[serde(skip)]
    pub typing_indicators_overridden: bool,
    #[serde(skip)]
    pub typing_indicators_enabled: bool,
    #[serde(skip)]
    pub introduction_method: String,
    #[serde(skip)]
    pub introduction_time: i64,
    #[serde(skip)]
    pub introduction_metadata: Uuid,
    #[serde(skip)]
    pub alias: String,
    #[serde(skip)]
    pub notes: String,
    #[serde(skip)]
    pub blocked: bool,
    #[serde(skip)]
    pub accepted: bool,

    /// The user's device group.
    #[serde(rename = "Devices")]
    #[serde(default, deserialize_with = "crate::msgpack::nullable_seq")]
    pub devices: Vec<Device>,
}

impl User {
    pub fn new(id: Uuid, name: String) -> Self {
        User {
            id,
            name,
            images: String::new(),
            profile: false,
            encrypted_devices: String::new(),
            public_ecdsa_key: Vec::new(),
            private_ecdsa_key: Vec::new(),
            public_ecdh_key: Vec::new(),
            private_ecdh_key: Vec::new(),
            open_dm: false,
            last_opened: 0,
            retention: 0,
            clear_before: 0,
            muted_until: 0,
            last_activity: 0,
            read_receipts_overridden: false,
            read_receipts_enabled: true,
            typing_indicators_overridden: false,
            typing_indicators_enabled: true,
            introduction_method: String::new(),
            introduction_time: 0,
            introduction_metadata: Uuid::nil(),
            alias: String::new(),
            notes: String::new(),
            blocked: false,
            accepted: false,
            devices: Vec::new(),
        }
    }

    /// The profile image history, oldest first.
    pub fn image_ids(&self) -> Vec<Uuid> {
        parse_uuid_list(&self.images)
    }

    /// Addresses of the encrypted devices this user owns.
    pub fn encrypted_device_addresses(&self) -> Vec<String> {
        if self.encrypted_devices.is_empty() {
            Vec::new()
        } else {
            self.encrypted_devices
                .split(',')
                .map(|s| s.to_string())
                .collect()
        }
    }

    /// Addresses of every device in the group that has not been revoked.
    pub fn active_device_addresses(&self) -> Vec<String> {
        self.devices
            .iter()
            .filter(|d| !d.is_revoked())
            .map(|d| d.address.clone())
            .collect()
    }

    /// The name shown in the interface: a locally set alias wins over the name
    /// the user broadcasts for themselves.
    pub fn display_name(&self) -> &str {
        if self.alias.is_empty() {
            &self.name
        } else {
            &self.alias
        }
    }
}

/// The user-level keys that are re-issued when a device is revoked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeySet {
    #[serde(rename = "PrivateECDSAKey", with = "serde_bytes")]
    pub private_ecdsa_key: Vec<u8>,
    #[serde(rename = "PublicECDSAKey", with = "serde_bytes")]
    pub public_ecdsa_key: Vec<u8>,
    #[serde(rename = "PrivateECDHKey", with = "serde_bytes")]
    pub private_ecdh_key: Vec<u8>,
    #[serde(rename = "PublicECDHKey", default, with = "crate::msgpack::nullable_bytes")]
    pub public_ecdh_key: Vec<u8>,
    #[serde(rename = "Kek", with = "serde_bytes")]
    pub kek: Vec<u8>,
}

/// Parse one of the protocol's comma-separated UUID lists.
///
/// Empty strings yield an empty list; unparseable entries are skipped rather
/// than failing the whole list, since a single malformed entry from a peer
/// should not discard the rest.
pub fn parse_uuid_list(joined: &str) -> Vec<Uuid> {
    if joined.is_empty() {
        return Vec::new();
    }
    joined.split(',').filter_map(|s| Uuid::parse_str(s).ok()).collect()
}

/// Render a UUID list back into the protocol's comma-separated form.
pub fn join_uuid_list(ids: &[Uuid]) -> String {
    ids.iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Whether a user-supplied name is acceptable.
///
/// Names may not be empty, may not have leading or trailing whitespace, may not
/// contain newlines, and are capped at [`crate::MAXIMUM_NAME_LENGTH`] scalar
/// values.
pub fn valid_user_name(name: &str) -> bool {
    !name.is_empty() && valid_device_name(name)
}

/// Like [`valid_user_name`], but the empty string is allowed — an unnamed
/// device is legal.
pub fn valid_device_name(name: &str) -> bool {
    name == name.trim()
        && !name.contains('\n')
        && name.chars().count() <= crate::MAXIMUM_NAME_LENGTH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_encodes_only_wire_fields() {
        let device = Device::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "someaddress".into(),
            1_700_000_000,
        );
        let encoded = msgpack::to_vec(&device).unwrap();

        // Seven keys: ID, UserID, Address, Timestamp, RevokedAt, Signature,
        // Capabilities. The last is an added key that the Go implementation
        // decodes and discards, which is what makes it safe to add at all —
        // verified against `Basekick-Labs/msgpack/v6` in both directions.
        assert_eq!(encoded[0], 0x87);

        let text = String::from_utf8_lossy(&encoded);
        for wire_field in [
            "ID",
            "UserID",
            "Address",
            "Timestamp",
            "RevokedAt",
            "Signature",
            "Capabilities",
        ] {
            assert!(text.contains(wire_field), "missing {wire_field}");
        }

        // A fresh device advertises what this build speaks, so a peer knows
        // without asking. Silence would read as legacy and cost us the feature.
        assert_eq!(device.capabilities, crate::types::capability::SUPPORTED);
        for local_field in ["Name", "SavedAt", "LastSeen", "ECDHPublicKey"] {
            assert!(!text.contains(local_field), "leaked {local_field}");
        }
    }

    #[test]
    fn device_round_trips() {
        let mut device = Device::new(Uuid::new_v4(), Uuid::new_v4(), "addr".into(), 42);
        device.name = "not on the wire".into();

        let decoded: Device = msgpack::from_slice(&msgpack::to_vec(&device).unwrap()).unwrap();
        assert_eq!(decoded.id, device.id);
        assert_eq!(decoded.address, device.address);
        assert_eq!(decoded.timestamp, 42);
        // Local-only fields come back at their defaults.
        assert_eq!(decoded.name, "");
    }

    #[test]
    fn device_revocation_is_time_relative() {
        let mut device = Device::new(Uuid::new_v4(), Uuid::new_v4(), "addr".into(), 0);
        assert!(!device.is_revoked());
        assert!(!device.was_revoked_at(1_000));

        device.revoked_at = 500;
        assert!(device.is_revoked());
        // Frames signed before the revocation are still honoured.
        assert!(!device.was_revoked_at(499));
        assert!(device.was_revoked_at(500));
        assert!(device.was_revoked_at(501));
    }

    #[test]
    fn devices_are_globally_scoped() {
        let device = Device::new(Uuid::new_v4(), Uuid::new_v4(), "addr".into(), 0);
        assert_eq!(device.scope(Uuid::new_v4()), Scope::Global);
        assert_eq!(device.destination(Uuid::new_v4()), Uuid::nil());
        assert_eq!(device.author(), device.user_id);
    }

    #[test]
    fn user_hides_private_state_from_the_wire() {
        let mut user = User::new(Uuid::new_v4(), "Alice".into());
        user.private_ecdh_key = vec![1, 2, 3];
        user.private_ecdsa_key = vec![4, 5, 6];
        user.alias = "local nickname".into();
        user.notes = "private notes".into();
        user.public_ecdh_key = vec![7; 32];

        let encoded = msgpack::to_vec(&user).unwrap();
        let text = String::from_utf8_lossy(&encoded);

        assert!(text.contains("PublicECDHKey"));
        assert!(!text.contains("PrivateECDHKey"));
        assert!(!text.contains("PrivateECDSAKey"));
        assert!(!text.contains("Alias"));
        assert!(!text.contains("Notes"));

        // And the secret bytes themselves are absent.
        assert!(!encoded.windows(3).any(|w| w == [1, 2, 3]));
        assert!(!encoded.windows(3).any(|w| w == [4, 5, 6]));
    }

    #[test]
    fn uuid_lists_round_trip() {
        let ids = vec![Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        assert_eq!(parse_uuid_list(&join_uuid_list(&ids)), ids);

        assert!(parse_uuid_list("").is_empty());
        assert_eq!(join_uuid_list(&[]), "");

        // A malformed entry is dropped without discarding its neighbours.
        let id = Uuid::new_v4();
        assert_eq!(parse_uuid_list(&format!("{id},not-a-uuid")), vec![id]);
    }

    #[test]
    fn active_device_addresses_exclude_revoked() {
        let user_id = Uuid::new_v4();
        let mut user = User::new(user_id, "Alice".into());
        user.devices.push(Device::new(Uuid::new_v4(), user_id, "alive".into(), 0));

        let mut revoked = Device::new(Uuid::new_v4(), user_id, "revoked".into(), 0);
        revoked.revoked_at = 1;
        user.devices.push(revoked);

        assert_eq!(user.active_device_addresses(), vec!["alive".to_string()]);
    }

    #[test]
    fn display_name_prefers_a_local_alias() {
        let mut user = User::new(Uuid::new_v4(), "Broadcast Name".into());
        assert_eq!(user.display_name(), "Broadcast Name");

        user.alias = "What I Call Them".into();
        assert_eq!(user.display_name(), "What I Call Them");
    }

    #[test]
    fn name_validation_matches_go_rules() {
        assert!(valid_user_name("Alice"));
        assert!(valid_user_name("Ünïcödé 名前"));

        assert!(!valid_user_name(""));
        assert!(!valid_user_name(" leading"));
        assert!(!valid_user_name("trailing "));
        assert!(!valid_user_name("two\nlines"));
        assert!(!valid_user_name(&"a".repeat(crate::MAXIMUM_NAME_LENGTH + 1)));
        assert!(valid_user_name(&"a".repeat(crate::MAXIMUM_NAME_LENGTH)));

        // Device names may be blank.
        assert!(valid_device_name(""));
    }
}

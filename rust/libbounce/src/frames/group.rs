//! Groups, their immutable creation record, and the updates that change them.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::identity::{parse_uuid_list, User};
use super::{Broadcastable, SignedFrame};
use crate::crypto;
use crate::error::Result;
use crate::msgpack;
use crate::types::{FrameType, Scope, UpdateGroupType};

/// A group chat.
///
/// This struct plays two roles. Inside a [`GroupCreation`] it is the immutable
/// original state that the group's ID is derived from; in the database it is
/// the current state, recomputed from that origin plus every accepted update.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    /// Derived from the hash of the creation blob, so a peer cannot alter the
    /// original state and still be talking about the same group.
    #[serde(rename = "ID")]
    pub id: Uuid,

    #[serde(rename = "Name")]
    pub name: String,

    /// Comma-separated history of group image file IDs.
    #[serde(rename = "Images")]
    pub images: String,

    #[serde(rename = "CreatedBy")]
    pub created_by: Uuid,

    #[serde(rename = "CreatedAt")]
    pub created_at: i64,

    #[serde(rename = "Retention")]
    pub retention: i64,

    #[serde(rename = "ClearBefore")]
    pub clear_before: i64,

    #[serde(rename = "MutedUntil")]
    pub muted_until: i64,

    #[serde(rename = "Users")]
    #[serde(default, deserialize_with = "crate::msgpack::nullable_seq")]
    pub users: Vec<User>,

    /// Comma-separated user IDs with administrative rights.
    #[serde(rename = "Admins")]
    pub admins: String,

    /// Comma-separated user IDs with an outstanding invitation.
    #[serde(rename = "Invites")]
    pub invites: String,

    #[serde(skip)]
    pub invited_by: Uuid,
    #[serde(skip)]
    pub invited_at: i64,
    #[serde(skip)]
    pub accepted_at: i64,

    #[serde(rename = "BlockedUsers")]
    pub blocked_users: String,

    #[serde(rename = "RestrictUserManagement")]
    pub restrict_user_management: bool,

    #[serde(rename = "RestrictGroupEdits")]
    pub restrict_group_edits: bool,

    #[serde(rename = "RestrictPosting")]
    pub restrict_posting: bool,

    #[serde(rename = "LastActivity")]
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
    pub delivery_records_cleared_for: Uuid,
    #[serde(skip)]
    pub last_opened: i64,
}

impl Group {
    pub fn admin_ids(&self) -> Vec<Uuid> {
        parse_uuid_list(&self.admins)
    }
    pub fn invite_ids(&self) -> Vec<Uuid> {
        parse_uuid_list(&self.invites)
    }
    pub fn blocked_user_ids(&self) -> Vec<Uuid> {
        parse_uuid_list(&self.blocked_users)
    }
    pub fn image_ids(&self) -> Vec<Uuid> {
        parse_uuid_list(&self.images)
    }
    pub fn member_ids(&self) -> Vec<Uuid> {
        self.users.iter().map(|u| u.id).collect()
    }
}

/// The immutable record that brings a group into existence.
///
/// `data` holds the encoded original [`Group`], and the group's ID is the first
/// 16 bytes of its BLAKE3 hash. Because every later frame addresses the group
/// by that ID, there is no way to rewrite the founding state without producing
/// a different group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupCreation {
    #[serde(skip)]
    pub signed: SignedFrame,

    #[serde(rename = "ID")]
    pub id: Uuid,

    #[serde(rename = "Timestamp")]
    pub timestamp: i64,

    #[serde(skip)]
    pub saved_at: i64,

    /// The encoded original group state.
    #[serde(rename = "Data", default, with = "crate::msgpack::nullable_bytes")]
    pub data: Vec<u8>,
}

impl GroupCreation {
    /// Derive the group ID a creation blob must carry.
    pub fn derive_id(data: &[u8]) -> Uuid {
        Uuid::from_bytes(crypto::hash_16(data))
    }

    /// Build a creation record for an initial group state.
    pub fn create(group: &Group, timestamp: i64) -> Result<Self> {
        let data = msgpack::to_vec(group)?;
        Ok(GroupCreation {
            signed: SignedFrame::default(),
            id: Self::derive_id(&data),
            timestamp,
            saved_at: 0,
            data,
        })
    }

    /// Whether the declared ID really is the hash of the payload.
    ///
    /// A creation whose ID does not match its data is discarded: accepting it
    /// would let a peer substitute different founding state under an ID other
    /// devices already trust.
    pub fn id_matches_data(&self) -> bool {
        Self::derive_id(&self.data) == self.id
    }

    /// Decode the original group state, stamping in the derived ID.
    pub fn group(&self) -> Result<Group> {
        let mut group: Group = msgpack::from_slice(&self.data)?;
        group.id = self.id;
        Ok(group)
    }
}

impl Broadcastable for GroupCreation {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::GroupCreation
    }
    fn payload(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(&self.signed.to_container())
    }
    fn scope(&self, _my_id: Uuid) -> Scope {
        Scope::GroupWithInvites
    }
    fn destination(&self, _my_id: Uuid) -> Uuid {
        self.id
    }
    fn author(&self) -> Uuid {
        // Group creations are never broadcast globally, so the author is not
        // consulted and is left unset.
        Uuid::nil()
    }
    fn timestamp(&self) -> i64 {
        self.timestamp
    }
    fn saved_at(&self) -> i64 {
        self.saved_at
    }
}

/// One device's signature over an [`UpdateGroup`], attesting that it saw the
/// update and considered it valid.
///
/// Confirmations are what make timestamp forgery ineffective: an update backed
/// by a majority of the group's users beats an earlier-timestamped update that
/// nobody confirmed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Confirmation {
    #[serde(rename = "ID")]
    pub id: Uuid,

    /// The update being confirmed.
    #[serde(rename = "UpdateGroupID")]
    pub update_group_id: Uuid,

    #[serde(skip)]
    pub destination: Uuid,
    #[serde(skip)]
    pub author: Uuid,
    #[serde(skip)]
    pub custom_scope: Uuid,

    #[serde(rename = "SigningDevice")]
    pub signing_device: String,

    #[serde(rename = "Signature", default, with = "crate::msgpack::nullable_bytes")]
    pub signature: Vec<u8>,

    #[serde(rename = "Timestamp")]
    pub timestamp: i64,

    #[serde(skip)]
    pub saved_at: i64,
}

impl Broadcastable for Confirmation {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::Confirmation
    }
    fn payload(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
    fn scope(&self, _my_id: Uuid) -> Scope {
        if self.custom_scope.is_nil() {
            Scope::Group
        } else {
            Scope::Custom
        }
    }
    fn destination(&self, _my_id: Uuid) -> Uuid {
        self.destination
    }
    fn author(&self) -> Uuid {
        self.author
    }
    fn timestamp(&self) -> i64 {
        self.timestamp
    }
    fn saved_at(&self) -> i64 {
        self.saved_at
    }
}

/// An atomic change to a group's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateGroup {
    #[serde(skip)]
    pub signed: SignedFrame,

    #[serde(rename = "ID")]
    pub id: Uuid,

    /// The user making the change.
    #[serde(rename = "Actor")]
    pub actor: Uuid,

    /// The group being changed.
    #[serde(rename = "Target")]
    pub target: Uuid,

    #[serde(rename = "Timestamp")]
    pub timestamp: i64,

    #[serde(skip)]
    pub saved_at: i64,

    #[serde(rename = "Type")]
    pub update_type: u16,

    /// Type-dependent payload; see [`UpdateGroup::has_valid_payload`].
    #[serde(rename = "Data", default, with = "crate::msgpack::nullable_bytes")]
    pub data: Vec<u8>,

    /// Set when the group this update belongs to has been deleted locally and
    /// the update must still reach a fixed set of devices.
    #[serde(skip)]
    pub custom_scope: Uuid,

    #[serde(rename = "Confirmations")]
    #[serde(default, deserialize_with = "crate::msgpack::nullable_seq")]
    pub confirmations: Vec<Confirmation>,

    #[serde(skip)]
    pub applied: bool,
    #[serde(skip)]
    pub notified: bool,
    #[serde(skip)]
    pub seen: bool,
}

impl UpdateGroup {
    pub fn new(actor: Uuid, group: Uuid, update_type: UpdateGroupType, data: Vec<u8>, timestamp: i64) -> Self {
        UpdateGroup {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            actor,
            target: group,
            timestamp,
            saved_at: 0,
            update_type: update_type.as_u16(),
            data,
            custom_scope: Uuid::nil(),
            confirmations: Vec::new(),
            applied: false,
            notified: false,
            seen: false,
        }
    }

    pub fn kind(&self) -> Result<UpdateGroupType> {
        UpdateGroupType::from_u16(self.update_type)
    }

    /// How many of `users` have confirmed this update.
    ///
    /// Only confirmations from users currently in the group count, so a removed
    /// member cannot keep propping up an update.
    pub fn confirming_users(&self, users: &[Uuid]) -> usize {
        let mut counted: Vec<Uuid> = Vec::new();
        for confirmation in &self.confirmations {
            if users.contains(&confirmation.author) && !counted.contains(&confirmation.author) {
                counted.push(confirmation.author);
            }
        }
        counted.len()
    }

    /// Whether a strict majority of `users` have confirmed this update.
    pub fn is_confirmed_by_majority(&self, users: &[Uuid]) -> bool {
        if users.is_empty() {
            return false;
        }
        (self.confirming_users(users) as f64 / users.len() as f64) > 0.5
    }

    /// Whether `data` is well-formed for this update's type.
    ///
    /// Each type expects a specific payload; anything else is treated as
    /// malformed rather than being applied with a garbage value.
    pub fn has_valid_payload(&self) -> bool {
        use UpdateGroupType::*;
        let Ok(kind) = self.kind() else {
            return false;
        };
        match kind {
            // A UTF-8 name within the length limit.
            ChangeName => std::str::from_utf8(&self.data)
                .map(super::identity::valid_user_name)
                .unwrap_or(false),
            // A single image file ID, or empty to clear the image.
            SetImage => self.data.is_empty() || Uuid::from_slice(&self.data).is_ok(),
            // A whole encoded user, so the invitee's device group travels with
            // the invitation.
            InviteUser => msgpack::from_slice::<User>(&self.data).is_ok(),
            // A user ID.
            RemoveUser | PromoteAdmin | DemoteAdmin | RevokeInvite => {
                Uuid::from_slice(&self.data).is_ok()
            }
            // An 8 byte big-endian integer.
            ChangeRetention | ChangeMutedUntil | SetClearBefore => self.data.len() == 8,
            // A single boolean byte.
            ChangeUserManagementPermission
            | ChangeGroupEditsPermission
            | ChangePostingPermission
            | RespondToInvite => self.data.len() == 1,
            // An override flag plus the value it overrides to.
            SetReadReceiptSettings | SetTypingIndicatorSettings => self.data.len() == 2,
            // Carry no payload.
            Delete | Block => self.data.is_empty(),
        }
    }

    /// Read an 8 byte little-endian integer payload.
    ///
    /// Little-endian because that is what the Go implementation writes; the
    /// choice is arbitrary but it is part of the wire format.
    pub fn data_as_i64(&self) -> Option<i64> {
        <[u8; 8]>::try_from(self.data.as_slice())
            .ok()
            .map(i64::from_le_bytes)
    }

    /// Read a boolean payload.
    pub fn data_as_bool(&self) -> Option<bool> {
        (self.data.len() == 1).then(|| self.data[0] != 0)
    }

    /// Read a UUID payload.
    pub fn data_as_uuid(&self) -> Option<Uuid> {
        Uuid::from_slice(&self.data).ok()
    }

    /// Encode an 8 byte little-endian integer payload.
    pub fn encode_i64(value: i64) -> Vec<u8> {
        value.to_le_bytes().to_vec()
    }

    /// Encode a boolean payload.
    pub fn encode_bool(value: bool) -> Vec<u8> {
        vec![u8::from(value)]
    }
}

impl Broadcastable for UpdateGroup {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::UpdateGroup
    }
    fn payload(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(&self.signed.to_container())
    }
    fn scope(&self, _my_id: Uuid) -> Scope {
        // Personal preferences about a group concern nobody else.
        if self.kind().map(UpdateGroupType::is_personal).unwrap_or(false) {
            return Scope::Sync;
        }
        if !self.custom_scope.is_nil() {
            return Scope::Custom;
        }
        // Everything else reaches invitees too, so they can see what they are
        // being invited to before accepting.
        Scope::GroupWithInvites
    }
    fn destination(&self, _my_id: Uuid) -> Uuid {
        self.target
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

    fn group_with_name(name: &str) -> Group {
        Group {
            name: name.into(),
            created_by: Uuid::new_v4(),
            created_at: 1_700_000_000,
            ..Default::default()
        }
    }

    #[test]
    fn group_id_is_the_hash_of_its_creation_data() {
        let group = group_with_name("Book Club");
        let creation = GroupCreation::create(&group, group.created_at).unwrap();

        assert!(creation.id_matches_data());
        assert_eq!(creation.id, GroupCreation::derive_id(&creation.data));
    }

    #[test]
    fn a_rewritten_creation_no_longer_matches_its_id() {
        let creation = GroupCreation::create(&group_with_name("Book Club"), 0).unwrap();

        // Swap in different founding state while keeping the advertised ID.
        let mut forged = creation.clone();
        forged.data = msgpack::to_vec(&group_with_name("Not Book Club")).unwrap();

        assert!(!forged.id_matches_data());
    }

    #[test]
    fn different_groups_get_different_ids() {
        let a = GroupCreation::create(&group_with_name("A"), 0).unwrap();
        let b = GroupCreation::create(&group_with_name("B"), 0).unwrap();
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn creation_round_trips_to_the_original_group() {
        let group = group_with_name("Book Club");
        let creation = GroupCreation::create(&group, group.created_at).unwrap();

        let recovered = creation.group().unwrap();
        assert_eq!(recovered.name, "Book Club");
        assert_eq!(recovered.created_at, group.created_at);
        // The ID is stamped in from the hash rather than trusted from the blob.
        assert_eq!(recovered.id, creation.id);
    }

    #[test]
    fn personal_updates_stay_on_our_own_devices() {
        let me = Uuid::new_v4();
        let group = Uuid::new_v4();

        for kind in [
            UpdateGroupType::ChangeMutedUntil,
            UpdateGroupType::SetReadReceiptSettings,
            UpdateGroupType::SetTypingIndicatorSettings,
        ] {
            let update = UpdateGroup::new(me, group, kind, vec![], 0);
            assert_eq!(update.scope(me), Scope::Sync, "{kind:?} should be private");
        }

        let shared = UpdateGroup::new(me, group, UpdateGroupType::ChangeName, vec![], 0);
        assert_eq!(shared.scope(me), Scope::GroupWithInvites);
    }

    #[test]
    fn a_custom_scope_overrides_the_default() {
        let me = Uuid::new_v4();
        let mut update =
            UpdateGroup::new(me, Uuid::new_v4(), UpdateGroupType::Delete, vec![], 0);
        assert_eq!(update.scope(me), Scope::GroupWithInvites);

        update.custom_scope = Uuid::new_v4();
        assert_eq!(update.scope(me), Scope::Custom);
    }

    #[test]
    fn payload_validation_is_type_specific() {
        let actor = Uuid::new_v4();
        let group = Uuid::new_v4();
        let make = |kind, data| UpdateGroup::new(actor, group, kind, data, 0);

        assert!(make(UpdateGroupType::ChangeName, b"Good Name".to_vec()).has_valid_payload());
        assert!(!make(UpdateGroupType::ChangeName, b" bad name ".to_vec()).has_valid_payload());
        assert!(!make(UpdateGroupType::ChangeName, vec![0xff, 0xfe]).has_valid_payload());

        assert!(make(UpdateGroupType::ChangeRetention, UpdateGroup::encode_i64(3600))
            .has_valid_payload());
        assert!(!make(UpdateGroupType::ChangeRetention, vec![1, 2, 3]).has_valid_payload());

        assert!(make(UpdateGroupType::RemoveUser, Uuid::new_v4().as_bytes().to_vec())
            .has_valid_payload());
        assert!(!make(UpdateGroupType::RemoveUser, vec![1, 2, 3]).has_valid_payload());

        assert!(make(UpdateGroupType::Delete, vec![]).has_valid_payload());
        assert!(!make(UpdateGroupType::Delete, vec![1]).has_valid_payload());

        assert!(make(UpdateGroupType::ChangePostingPermission, vec![1]).has_valid_payload());
        assert!(!make(UpdateGroupType::ChangePostingPermission, vec![1, 0]).has_valid_payload());

        assert!(make(UpdateGroupType::SetReadReceiptSettings, vec![1, 0]).has_valid_payload());
        assert!(!make(UpdateGroupType::SetReadReceiptSettings, vec![1]).has_valid_payload());

        // An invitation carries a whole user structure.
        let user = User::new(Uuid::new_v4(), "Invitee".into());
        assert!(make(UpdateGroupType::InviteUser, msgpack::to_vec(&user).unwrap())
            .has_valid_payload());
        assert!(!make(UpdateGroupType::InviteUser, b"not a user".to_vec()).has_valid_payload());
    }

    #[test]
    fn payload_accessors_round_trip() {
        let update = UpdateGroup::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            UpdateGroupType::ChangeRetention,
            UpdateGroup::encode_i64(-42),
            0,
        );
        assert_eq!(update.data_as_i64(), Some(-42));

        let flag = UpdateGroup::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            UpdateGroupType::ChangePostingPermission,
            UpdateGroup::encode_bool(true),
            0,
        );
        assert_eq!(flag.data_as_bool(), Some(true));

        let target = Uuid::new_v4();
        let removal = UpdateGroup::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            UpdateGroupType::RemoveUser,
            target.as_bytes().to_vec(),
            0,
        );
        assert_eq!(removal.data_as_uuid(), Some(target));
    }

    fn confirmation_from(author: Uuid, update_id: Uuid) -> Confirmation {
        Confirmation {
            id: Uuid::new_v4(),
            update_group_id: update_id,
            destination: Uuid::nil(),
            author,
            custom_scope: Uuid::nil(),
            signing_device: String::new(),
            signature: Vec::new(),
            timestamp: 0,
            saved_at: 0,
        }
    }

    #[test]
    fn majority_confirmation_requires_more_than_half() {
        let members: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
        let mut update = UpdateGroup::new(
            members[0],
            Uuid::new_v4(),
            UpdateGroupType::ChangeName,
            b"New".to_vec(),
            0,
        );

        // Two of four is not a majority.
        update.confirmations = vec![
            confirmation_from(members[0], update.id),
            confirmation_from(members[1], update.id),
        ];
        assert_eq!(update.confirming_users(&members), 2);
        assert!(!update.is_confirmed_by_majority(&members));

        update.confirmations.push(confirmation_from(members[2], update.id));
        assert!(update.is_confirmed_by_majority(&members));
    }

    #[test]
    fn confirmations_from_outsiders_and_duplicates_do_not_count() {
        let members: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        let outsider = Uuid::new_v4();

        let mut update = UpdateGroup::new(
            members[0],
            Uuid::new_v4(),
            UpdateGroupType::ChangeName,
            b"New".to_vec(),
            0,
        );
        update.confirmations = vec![
            confirmation_from(members[0], update.id),
            // The same member twice is still one confirming user.
            confirmation_from(members[0], update.id),
            // Someone who is not in the group carries no weight.
            confirmation_from(outsider, update.id),
        ];

        assert_eq!(update.confirming_users(&members), 1);
        assert!(!update.is_confirmed_by_majority(&members));
    }

    #[test]
    fn an_empty_group_has_no_majority() {
        let update = UpdateGroup::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            UpdateGroupType::ChangeName,
            b"New".to_vec(),
            0,
        );
        assert!(!update.is_confirmed_by_majority(&[]));
    }
}

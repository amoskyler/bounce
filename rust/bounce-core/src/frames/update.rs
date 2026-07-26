//! Updates to users, devices, direct message threads, and profile settings.
//!
//! These follow the same shape as [`super::UpdateGroup`]: a target, a type tag,
//! and a type-dependent payload. Unlike group updates they are not subject to
//! consensus — each concerns either a single user's own profile or a single
//! conversation, so there is no shared state for two actors to race over.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Broadcastable, SignedFrame};
use crate::error::{Error, Result};
use crate::types::{FrameType, Scope};
use crate::{msgpack, xor};

/// The kind of change an [`UpdateUser`] applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum UpdateUserType {
    UpdateName = 0,
    UpdateImage = 1,
    AddEncryptedDevice = 2,
    RemoveEncryptedDevice = 3,
    SetEncryptedDeviceName = 4,
    /// Re-issues the user's keys, which happens whenever a device is revoked.
    ReplaceKeys = 5,
    ReplaceEcdhPublicKey = 6,
}

impl UpdateUserType {
    pub fn as_u16(self) -> u16 {
        self as u16
    }

    pub fn from_u16(value: u16) -> Result<Self> {
        use UpdateUserType::*;
        Ok(match value {
            0 => UpdateName,
            1 => UpdateImage,
            2 => AddEncryptedDevice,
            3 => RemoveEncryptedDevice,
            4 => SetEncryptedDeviceName,
            5 => ReplaceKeys,
            6 => ReplaceEcdhPublicKey,
            other => return Err(Error::UnknownFrameType(other)),
        })
    }

    /// Whether this change concerns only the user's own devices.
    ///
    /// Key replacement and encrypted device naming carry private material or
    /// private labels, so they must not reach contacts.
    fn is_private(self) -> bool {
        matches!(
            self,
            UpdateUserType::SetEncryptedDeviceName | UpdateUserType::ReplaceKeys
        )
    }
}

/// A change to a user's profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateUser {
    #[serde(skip)]
    pub signed: SignedFrame,

    #[serde(rename = "ID")]
    pub id: Uuid,

    /// The user being changed.
    #[serde(rename = "Target")]
    pub target: Uuid,

    #[serde(rename = "Type")]
    pub update_type: u16,

    #[serde(rename = "Data", with = "serde_bytes")]
    pub data: Vec<u8>,

    /// The previous value, kept locally so the interface can say what a name
    /// changed *from*.
    #[serde(skip)]
    pub previous_data: Vec<u8>,

    #[serde(rename = "Timestamp")]
    pub timestamp: i64,

    #[serde(skip)]
    pub saved_at: i64,
    #[serde(skip)]
    pub seen: bool,
}

impl UpdateUser {
    pub fn new(target: Uuid, update_type: UpdateUserType, data: Vec<u8>, timestamp: i64) -> Self {
        UpdateUser {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            target,
            update_type: update_type.as_u16(),
            data,
            previous_data: Vec::new(),
            timestamp,
            saved_at: 0,
            seen: false,
        }
    }

    pub fn kind(&self) -> Result<UpdateUserType> {
        UpdateUserType::from_u16(self.update_type)
    }
}

impl Broadcastable for UpdateUser {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::UpdateUser
    }
    fn payload(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(&self.signed.to_container())
    }
    fn scope(&self, _my_id: Uuid) -> Scope {
        if self.kind().map(UpdateUserType::is_private).unwrap_or(false) {
            Scope::Sync
        } else {
            // Name and image changes go to every contact.
            Scope::Global
        }
    }
    fn destination(&self, _my_id: Uuid) -> Uuid {
        self.target
    }
    fn author(&self) -> Uuid {
        self.target
    }
    fn timestamp(&self) -> i64 {
        self.timestamp
    }
    fn saved_at(&self) -> i64 {
        self.saved_at
    }
}

/// The kind of change an [`UpdateDevice`] applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum UpdateDeviceType {
    UpdateName = 0,
    Revoke = 1,
    SetPublicKey = 2,
}

impl UpdateDeviceType {
    pub fn as_u16(self) -> u16 {
        self as u16
    }

    pub fn from_u16(value: u16) -> Result<Self> {
        Ok(match value {
            0 => UpdateDeviceType::UpdateName,
            1 => UpdateDeviceType::Revoke,
            2 => UpdateDeviceType::SetPublicKey,
            other => return Err(Error::UnknownFrameType(other)),
        })
    }
}

/// A change to one device in a device group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateDevice {
    #[serde(skip)]
    pub signed: SignedFrame,

    #[serde(rename = "ID")]
    pub id: Uuid,

    /// The device being changed.
    #[serde(rename = "Target")]
    pub target: Uuid,

    #[serde(rename = "Type")]
    pub update_type: u16,

    #[serde(rename = "Data", with = "serde_bytes")]
    pub data: Vec<u8>,

    #[serde(rename = "Timestamp")]
    pub timestamp: i64,

    #[serde(skip)]
    pub saved_at: i64,
    #[serde(skip)]
    pub author: Uuid,
}

impl UpdateDevice {
    pub fn new(target: Uuid, update_type: UpdateDeviceType, data: Vec<u8>, timestamp: i64) -> Self {
        UpdateDevice {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            target,
            update_type: update_type.as_u16(),
            data,
            timestamp,
            saved_at: 0,
            author: Uuid::nil(),
        }
    }

    pub fn kind(&self) -> Result<UpdateDeviceType> {
        UpdateDeviceType::from_u16(self.update_type)
    }
}

impl Broadcastable for UpdateDevice {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::UpdateDevice
    }
    fn payload(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(&self.signed.to_container())
    }
    fn scope(&self, _my_id: Uuid) -> Scope {
        // Everyone must learn about a revocation, so they stop accepting frames
        // signed by that device. A rename is nobody else's business.
        if matches!(self.kind(), Ok(UpdateDeviceType::Revoke)) {
            Scope::Global
        } else {
            Scope::Sync
        }
    }
    fn destination(&self, _my_id: Uuid) -> Uuid {
        self.target
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

/// The kind of change an [`UpdateDm`] applies to a direct message thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum UpdateDmType {
    ChangeMutedUntil = 0,
    ChangeRetention = 1,
    SetClearBefore = 2,
    SetReadReceipts = 3,
    SetTypingIndicators = 4,
    SetOpen = 5,
    SetAlias = 6,
    SetNotes = 7,
    SetBlocked = 8,
    OfferRetention = 9,
}

impl UpdateDmType {
    pub fn as_u16(self) -> u16 {
        self as u16
    }

    pub fn from_u16(value: u16) -> Result<Self> {
        use UpdateDmType::*;
        Ok(match value {
            0 => ChangeMutedUntil,
            1 => ChangeRetention,
            2 => SetClearBefore,
            3 => SetReadReceipts,
            4 => SetTypingIndicators,
            5 => SetOpen,
            6 => SetAlias,
            7 => SetNotes,
            8 => SetBlocked,
            9 => OfferRetention,
            other => return Err(Error::UnknownFrameType(other)),
        })
    }

    /// Whether this change affects the conversation for both participants.
    ///
    /// Retention and history clearing are properties of the thread itself, so
    /// the counterparty needs them. Mute state, aliases, and notes are one
    /// side's private view.
    pub fn is_shared(self) -> bool {
        matches!(
            self,
            UpdateDmType::ChangeRetention
                | UpdateDmType::SetClearBefore
                | UpdateDmType::OfferRetention
        )
    }

    /// Whether the change gets a row in the timeline, and so is kept as a
    /// frame rather than folded straight into the user record.
    ///
    /// An offer is not a change — it asks the other side to agree to a
    /// retention period — so it leaves nothing behind until it is accepted.
    pub fn leaves_a_record(self) -> bool {
        matches!(
            self,
            UpdateDmType::ChangeRetention | UpdateDmType::SetClearBefore
        )
    }
}

/// A change to a direct message thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateDm {
    #[serde(skip)]
    pub signed: SignedFrame,

    #[serde(rename = "ID")]
    pub id: Uuid,

    #[serde(rename = "Actor")]
    pub actor: Uuid,

    /// XOR of the two users in the thread.
    #[serde(rename = "Target")]
    pub target: Uuid,

    #[serde(rename = "Timestamp")]
    pub timestamp: i64,

    #[serde(skip)]
    pub saved_at: i64,
    #[serde(skip)]
    pub seen: bool,

    #[serde(rename = "Type")]
    pub update_type: u16,

    #[serde(rename = "Data", with = "serde_bytes")]
    pub data: Vec<u8>,
}

impl UpdateDm {
    pub fn new(actor: Uuid, target: Uuid, update_type: UpdateDmType, data: Vec<u8>, timestamp: i64) -> Self {
        UpdateDm {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            actor,
            target,
            timestamp,
            saved_at: 0,
            seen: false,
            update_type: update_type.as_u16(),
            data,
        }
    }

    pub fn kind(&self) -> Result<UpdateDmType> {
        UpdateDmType::from_u16(self.update_type)
    }
}

impl Broadcastable for UpdateDm {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::UpdateDm
    }
    fn payload(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(&self.signed.to_container())
    }
    fn scope(&self, _my_id: Uuid) -> Scope {
        let shared = self.kind().map(UpdateDmType::is_shared).unwrap_or(false);
        if !self.target.is_nil() && shared {
            Scope::User
        } else {
            Scope::Sync
        }
    }
    fn destination(&self, my_id: Uuid) -> Uuid {
        xor(my_id, self.target)
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
    fn profile_changes_reach_contacts_but_keys_do_not() {
        let me = Uuid::new_v4();

        let rename = UpdateUser::new(me, UpdateUserType::UpdateName, b"New Name".to_vec(), 0);
        assert_eq!(rename.scope(me), Scope::Global);

        let image = UpdateUser::new(me, UpdateUserType::UpdateImage, vec![], 0);
        assert_eq!(image.scope(me), Scope::Global);

        // Private key material must never leave the device group.
        let rekey = UpdateUser::new(me, UpdateUserType::ReplaceKeys, vec![1, 2, 3], 0);
        assert_eq!(rekey.scope(me), Scope::Sync);

        let encrypted_name =
            UpdateUser::new(me, UpdateUserType::SetEncryptedDeviceName, vec![], 0);
        assert_eq!(encrypted_name.scope(me), Scope::Sync);
    }

    #[test]
    fn revocations_are_announced_globally_and_renames_are_not() {
        let me = Uuid::new_v4();
        let device = Uuid::new_v4();

        let revoke = UpdateDevice::new(device, UpdateDeviceType::Revoke, vec![], 0);
        assert_eq!(revoke.scope(me), Scope::Global);

        let rename = UpdateDevice::new(device, UpdateDeviceType::UpdateName, b"Laptop".to_vec(), 0);
        assert_eq!(rename.scope(me), Scope::Sync);

        let key = UpdateDevice::new(device, UpdateDeviceType::SetPublicKey, vec![0; 32], 0);
        assert_eq!(key.scope(me), Scope::Sync);
    }

    #[test]
    fn dm_settings_scope_by_whether_they_are_shared() {
        let me = Uuid::new_v4();
        let them = Uuid::new_v4();
        let thread = xor(me, them);

        for shared in [
            UpdateDmType::ChangeRetention,
            UpdateDmType::SetClearBefore,
            UpdateDmType::OfferRetention,
        ] {
            let update = UpdateDm::new(me, thread, shared, vec![], 0);
            assert_eq!(update.scope(me), Scope::User, "{shared:?} is shared");
            assert_eq!(update.destination(me), them);
        }

        for private in [
            UpdateDmType::ChangeMutedUntil,
            UpdateDmType::SetAlias,
            UpdateDmType::SetNotes,
            UpdateDmType::SetBlocked,
            UpdateDmType::SetOpen,
        ] {
            let update = UpdateDm::new(me, thread, private, vec![], 0);
            assert_eq!(update.scope(me), Scope::Sync, "{private:?} is private");
        }
    }

    #[test]
    fn a_shared_dm_update_with_no_counterparty_stays_local() {
        let me = Uuid::new_v4();
        // A nil target means the note-to-self thread; there is nobody to tell.
        let update = UpdateDm::new(me, Uuid::nil(), UpdateDmType::ChangeRetention, vec![], 0);
        assert_eq!(update.scope(me), Scope::Sync);
    }

    #[test]
    fn update_types_round_trip() {
        for raw in 0u16..=6 {
            assert_eq!(UpdateUserType::from_u16(raw).unwrap().as_u16(), raw);
        }
        assert!(UpdateUserType::from_u16(7).is_err());

        for raw in 0u16..=2 {
            assert_eq!(UpdateDeviceType::from_u16(raw).unwrap().as_u16(), raw);
        }
        assert!(UpdateDeviceType::from_u16(3).is_err());

        for raw in 0u16..=9 {
            assert_eq!(UpdateDmType::from_u16(raw).unwrap().as_u16(), raw);
        }
        assert!(UpdateDmType::from_u16(10).is_err());
    }

    #[test]
    fn update_user_encodes_only_wire_fields() {
        let mut update =
            UpdateUser::new(Uuid::new_v4(), UpdateUserType::UpdateName, b"New".to_vec(), 7);
        update.previous_data = b"Old".to_vec();
        update.seen = true;

        let encoded = msgpack::to_vec(&update).unwrap();
        let text = String::from_utf8_lossy(&encoded);

        assert!(text.contains("Target"));
        assert!(text.contains("Data"));
        assert!(!text.contains("PreviousData"));
        assert!(!text.contains("Seen"));

        let decoded: UpdateUser = msgpack::from_slice(&encoded).unwrap();
        assert_eq!(decoded.data, b"New".to_vec());
        assert!(decoded.previous_data.is_empty());
    }
}

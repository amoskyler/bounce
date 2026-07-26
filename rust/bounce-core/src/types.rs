//! Frame type identifiers and broadcast scopes.
//!
//! These values are part of the wire format and must stay numerically identical
//! to `chat/protocol.go` in the Go implementation.

use crate::error::{Error, Result};

/// The numeric identifier carried in the first two bytes of every frame header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum FrameType {
    DirectMessage = 0,
    GroupMessage = 1,
    ReferenceOffer = 2,
    ReferenceRequest = 3,
    CatchUp = 4,
    Ack = 5,
    KeepAlive = 6,
    SyncDeviceRequest = 7,
    SyncDeviceRequestRejected = 8,
    SyncDeviceRequestAccepted = 9,
    Device = 10,
    UpdateDm = 11,
    GroupCreation = 12,
    UpdateGroup = 13,
    TypingIndicator = 14,
    AddUserRequest = 15,
    AddUserRequestAccepted = 16,
    AddUserRequestRejected = 17,
    AddUser = 18,
    Confirmation = 19,
    UpdateUser = 20,
    UpdateDevice = 21,
    ReadReceipt = 22,
    UpdateSettings = 23,
    File = 24,
    Chunk = 25,
    ChunkOffer = 26,
    ChunkRequest = 27,
    ActiveDevice = 28,
    EncryptedFrame = 29,
    EncryptedCatchUp = 30,
    EncryptedDeviceManagementRequest = 31,
    EncryptedDeviceManagementResponse = 32,
    EncryptedReferenceOfferChallenge = 33,
    EncryptedReferenceOfferResponse = 34,
    AppendRecipient = 35,
    ManageEncryptedDevice = 36,
    EncryptedDeviceManagementActionResponse = 37,
    GetManagementKeyHash = 38,
    ManagementKeyHashResponse = 39,
    AppendRecipientRequest = 40,
    AppendRecipientResponse = 41,
    AppendRecipientPayloads = 42,
    Draft = 43,
    EncryptedReceive = 44,
    EncryptedChunkOffer = 45,
    EncryptedStorageReferenceOffer = 46,
    EncryptedStorageReferenceRequest = 47,
    EncryptedChunkStorageRequest = 48,
    RequestEcro = 49,
    EncryptedClearBefore = 50,
    ChunkUnavailable = 51,
}

impl FrameType {
    pub fn as_u16(self) -> u16 {
        self as u16
    }

    pub fn from_u16(value: u16) -> Result<Self> {
        use FrameType::*;
        Ok(match value {
            0 => DirectMessage,
            1 => GroupMessage,
            2 => ReferenceOffer,
            3 => ReferenceRequest,
            4 => CatchUp,
            5 => Ack,
            6 => KeepAlive,
            7 => SyncDeviceRequest,
            8 => SyncDeviceRequestRejected,
            9 => SyncDeviceRequestAccepted,
            10 => Device,
            11 => UpdateDm,
            12 => GroupCreation,
            13 => UpdateGroup,
            14 => TypingIndicator,
            15 => AddUserRequest,
            16 => AddUserRequestAccepted,
            17 => AddUserRequestRejected,
            18 => AddUser,
            19 => Confirmation,
            20 => UpdateUser,
            21 => UpdateDevice,
            22 => ReadReceipt,
            23 => UpdateSettings,
            24 => File,
            25 => Chunk,
            26 => ChunkOffer,
            27 => ChunkRequest,
            28 => ActiveDevice,
            29 => EncryptedFrame,
            30 => EncryptedCatchUp,
            31 => EncryptedDeviceManagementRequest,
            32 => EncryptedDeviceManagementResponse,
            33 => EncryptedReferenceOfferChallenge,
            34 => EncryptedReferenceOfferResponse,
            35 => AppendRecipient,
            36 => ManageEncryptedDevice,
            37 => EncryptedDeviceManagementActionResponse,
            38 => GetManagementKeyHash,
            39 => ManagementKeyHashResponse,
            40 => AppendRecipientRequest,
            41 => AppendRecipientResponse,
            42 => AppendRecipientPayloads,
            43 => Draft,
            44 => EncryptedReceive,
            45 => EncryptedChunkOffer,
            46 => EncryptedStorageReferenceOffer,
            47 => EncryptedStorageReferenceRequest,
            48 => EncryptedChunkStorageRequest,
            49 => RequestEcro,
            50 => EncryptedClearBefore,
            51 => ChunkUnavailable,
            other => return Err(Error::UnknownFrameType(other)),
        })
    }

    /// The relative order in which frame types are replayed during a catch up.
    ///
    /// Frames saved in the same second are replayed in this order so that, for
    /// example, the device that authored a message is always known before the
    /// message itself is processed. Types absent from this table are never
    /// included in a catch up.
    pub fn catch_up_order(self) -> Option<u8> {
        use FrameType::*;
        Some(match self {
            AddUser => 0,
            Device => 1,
            UpdateDevice => 2,
            UpdateUser => 3,
            UpdateDm => 4,
            DirectMessage => 5,
            GroupCreation => 6,
            UpdateGroup => 7,
            GroupMessage => 8,
            Confirmation => 9,
            ReadReceipt => 10,
            UpdateSettings => 11,
            File => 12,
            ChunkOffer => 13,
            EncryptedChunkOffer => 14,
            Draft => 15,
            _ => return None,
        })
    }
}

/// The set of devices a broadcast frame is delivered to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum Scope {
    /// This user's own devices.
    Sync = 0,
    /// This user's devices plus every device of one other user.
    User = 1,
    /// Every device of every member of a group.
    Group = 2,
    /// Every device belonging to a known contact; used for profile updates.
    Global = 3,
    /// An explicit device list stored in the database, used when the natural
    /// scope of a frame is about to disappear (such as a group being deleted).
    Custom = 4,
    /// Like [`Scope::Group`], plus the devices of users with a pending invite.
    GroupWithInvites = 5,
}

impl Scope {
    pub fn as_i64(self) -> i64 {
        self as i64
    }

    pub fn from_i64(value: i64) -> Result<Self> {
        Ok(match value {
            0 => Scope::Sync,
            1 => Scope::User,
            2 => Scope::Group,
            3 => Scope::Global,
            4 => Scope::Custom,
            5 => Scope::GroupWithInvites,
            other => return Err(Error::UnknownScope(other)),
        })
    }
}

/// The kind of change an `updateGroup` frame applies to a group.
///
/// Mirrors the `updateGroupType*` constants in `chat/update_group.go`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum UpdateGroupType {
    ChangeName = 0,
    InviteUser = 1,
    RemoveUser = 2,
    ChangeMutedUntil = 3,
    ChangeRetention = 4,
    SetClearBefore = 5,
    PromoteAdmin = 6,
    DemoteAdmin = 7,
    ChangeUserManagementPermission = 8,
    ChangeGroupEditsPermission = 9,
    ChangePostingPermission = 10,
    Delete = 11,
    Block = 12,
    SetReadReceiptSettings = 13,
    SetTypingIndicatorSettings = 14,
    SetImage = 15,
    RevokeInvite = 16,
    RespondToInvite = 17,
}

impl UpdateGroupType {
    pub fn as_u16(self) -> u16 {
        self as u16
    }

    pub fn from_u16(value: u16) -> Result<Self> {
        use UpdateGroupType::*;
        Ok(match value {
            0 => ChangeName,
            1 => InviteUser,
            2 => RemoveUser,
            3 => ChangeMutedUntil,
            4 => ChangeRetention,
            5 => SetClearBefore,
            6 => PromoteAdmin,
            7 => DemoteAdmin,
            8 => ChangeUserManagementPermission,
            9 => ChangeGroupEditsPermission,
            10 => ChangePostingPermission,
            11 => Delete,
            12 => Block,
            13 => SetReadReceiptSettings,
            14 => SetTypingIndicatorSettings,
            15 => SetImage,
            16 => RevokeInvite,
            17 => RespondToInvite,
            other => return Err(Error::UnknownFrameType(other)),
        })
    }

    /// Whether this update only affects the acting user's own view of the
    /// group, and so is never subject to permission checks or consensus
    /// conflicts with other users.
    pub fn is_personal(self) -> bool {
        matches!(
            self,
            UpdateGroupType::ChangeMutedUntil
                | UpdateGroupType::SetReadReceiptSettings
                | UpdateGroupType::SetTypingIndicatorSettings
        )
    }
}

/// How a user came to be known to this device.
pub mod introduction {
    /// This user is the profile that owns this device.
    pub const PROFILE: &str = "profile";
    /// Added directly by scanning a code.
    pub const ADD_USER: &str = "add_user";
    /// Met through a shared group.
    pub const GROUP: &str = "group";
}

/// The role a distributed file plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum FileType {
    // These are the values the Go implementation stores and transmits, so the
    // order is fixed: a group image is 0 there, not a user image.
    GroupImage = 0,
    UserImage = 1,
    MessageAttachment = 2,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_types_round_trip() {
        for raw in 0u16..=51 {
            let ft = FrameType::from_u16(raw).expect("every id in range is known");
            assert_eq!(ft.as_u16(), raw);
        }
        assert!(FrameType::from_u16(52).is_err());
    }

    #[test]
    fn scopes_round_trip() {
        for raw in 0i64..=5 {
            assert_eq!(Scope::from_i64(raw).unwrap().as_i64(), raw);
        }
        assert!(Scope::from_i64(6).is_err());
    }

    #[test]
    fn catch_up_order_matches_go_table() {
        assert_eq!(FrameType::AddUser.catch_up_order(), Some(0));
        assert_eq!(FrameType::Draft.catch_up_order(), Some(15));
        // Transport-level frames are never replayed.
        assert_eq!(FrameType::Ack.catch_up_order(), None);
        assert_eq!(FrameType::KeepAlive.catch_up_order(), None);
    }
}

//! SQLite persistence.
//!
//! Every frame this device accepts is stored, because storage is what makes the
//! reference flow possible: a peer that comes online after a month asks what we
//! have, and we can only answer from what we kept.
//!
//! The [`Store`] wraps a single connection behind a mutex. Bounce's write
//! volume is a chat application's — a handful of frames a second at worst — so
//! a connection pool would add contention-management complexity for no gain,
//! and serialising writes removes a whole class of interleaving bugs from the
//! frame handlers.

pub mod schema;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension, Row};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::frames::file::File;
use crate::frames::group::{Confirmation, Group, GroupCreation, UpdateGroup};
use crate::frames::identity::{Device, IntroductionSignature, ProfileSettings, User};
use crate::frames::message::{
    DirectMessage, Draft, FileAttachment, GroupMessage, ImageAttachment, ReadReceipt,
};
use crate::frames::pairing::{AddUser, SyncDeviceOffer};
use crate::frames::transport::{CustomScope, DeliveryRecord, FrameReference};
use crate::frames::update::{UpdateDevice, UpdateDm, UpdateSettings, UpdateUser};
use crate::frames::SignedFrame;
use crate::types::FrameType;

/// A handle to the device's database.
pub struct Store {
    connection: Mutex<Connection>,
    /// Where files too large to keep in the database live.
    ///
    /// A chunk is capped at a megabyte and an embedded file at twenty, so
    /// those sit in `chunks.data` quite happily. Anything larger is seeded
    /// from, and downloaded to, a file on disk — holding a gigabyte of
    /// ciphertext in SQLite rows would mean loading it all to read any of it.
    ///
    /// `None` for an in-memory database, which has nowhere to put them.
    blobs: Option<PathBuf>,
}

impl Store {
    /// Open, creating or migrating the schema as needed.
    ///
    /// Large-file storage goes in a `blobs` directory beside the database, as
    /// it does in the Go implementation.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let connection = Connection::open(path)?;
        schema::ensure(&connection)?;

        let blobs = path.parent().map(|parent| parent.join("blobs"));
        if let Some(directory) = &blobs {
            std::fs::create_dir_all(directory)?;
        }

        Ok(Store {
            connection: Mutex::new(connection),
            blobs,
        })
    }

    /// Open an in-memory database, for tests.
    pub fn in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        schema::ensure(&connection)?;
        Ok(Store {
            connection: Mutex::new(connection),
            blobs: None,
        })
    }

    /// Give an in-memory store somewhere to put large files.
    pub fn with_blobs_directory(mut self, directory: impl AsRef<Path>) -> Result<Self> {
        let directory = directory.as_ref().to_path_buf();
        std::fs::create_dir_all(&directory)?;
        self.blobs = Some(directory);
        Ok(self)
    }

    /// Where a file's bytes live on this device, for a file too large to
    /// embed. `None` when there is nowhere to put one.
    pub fn blob_path(&self, file_id: Uuid) -> Option<PathBuf> {
        self.blobs
            .as_ref()
            .map(|directory| directory.join(file_id.to_string()))
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = self
            .connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&guard)
    }

    // ---------------------------------------------------------------------
    // Users
    // ---------------------------------------------------------------------

    /// Insert or replace a user, along with their devices.
    ///
    /// The user row and its devices are written in one transaction: a device
    /// group that fails a constraint part-way through must not leave a contact
    /// half-created.
    pub fn save_user(&self, user: &User) -> Result<()> {
        self.with(|connection| {
            connection.execute_batch("BEGIN IMMEDIATE")?;
            connection.execute(
                r#"INSERT INTO users (
                    id, name, images, profile, encrypted_devices,
                    public_ecdsa_key, private_ecdsa_key, public_ecdh_key, private_ecdh_key,
                    open_dm, last_opened, retention, clear_before, muted_until, last_activity,
                    read_receipts_overridden, read_receipts_enabled,
                    typing_indicators_overridden, typing_indicators_enabled,
                    introduction_method, introduction_time, introduction_metadata,
                    alias, notes, blocked, accepted
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                    ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26
                )
                ON CONFLICT (id) DO UPDATE SET
                    name = excluded.name,
                    images = excluded.images,
                    encrypted_devices = excluded.encrypted_devices,
                    public_ecdh_key = excluded.public_ecdh_key,
                    -- Never backwards: a record arriving from another device
                    -- must not make a conversation look staler than it is.
                    last_activity = MAX(users.last_activity, excluded.last_activity)"#,
                params![
                    uuid_bytes(user.id),
                    user.name,
                    user.images,
                    user.profile,
                    user.encrypted_devices,
                    user.public_ecdsa_key,
                    user.private_ecdsa_key,
                    user.public_ecdh_key,
                    user.private_ecdh_key,
                    user.open_dm,
                    user.last_opened,
                    user.retention,
                    user.clear_before,
                    user.muted_until,
                    user.last_activity,
                    user.read_receipts_overridden,
                    user.read_receipts_enabled,
                    user.typing_indicators_overridden,
                    user.typing_indicators_enabled,
                    user.introduction_method,
                    user.introduction_time,
                    uuid_bytes(user.introduction_metadata),
                    user.alias,
                    user.notes,
                    user.blocked,
                    user.accepted,
                ],
            )
            .inspect_err(|_| {
                let _ = connection.execute_batch("ROLLBACK");
            })?;
            Ok(())
        })?;

        let devices = (|| -> Result<()> {
            for device in &user.devices {
                self.save_device(device)?;
            }
            Ok(())
        })();

        self.with(|connection| {
            connection.execute_batch(if devices.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
            Ok(())
        })?;

        devices
    }

    /// Look a user up by ID, with their device group loaded.
    pub fn user(&self, id: Uuid) -> Result<Option<User>> {
        let user = self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM users WHERE id = ?1",
                    params![uuid_bytes(id)],
                    row_to_user,
                )
                .optional()?)
        })?;

        match user {
            Some(mut user) => {
                user.devices = self.devices_for_user(user.id)?;
                Ok(Some(user))
            }
            None => Ok(None),
        }
    }

    /// The profile that owns this device.
    pub fn profile(&self) -> Result<Option<User>> {
        let user = self.with(|connection| {
            Ok(connection
                .query_row("SELECT * FROM users WHERE profile = 1", [], row_to_user)
                .optional()?)
        })?;

        match user {
            Some(mut user) => {
                user.devices = self.devices_for_user(user.id)?;
                Ok(Some(user))
            }
            None => Ok(None),
        }
    }

    /// The profile's user ID, or an error if no profile has been created.
    pub fn my_user_id(&self) -> Result<Uuid> {
        self.with(|connection| {
            connection
                .query_row("SELECT id FROM users WHERE profile = 1", [], |row| {
                    row.get::<_, Vec<u8>>(0)
                })
                .optional()?
                .and_then(|bytes| Uuid::from_slice(&bytes).ok())
                .ok_or(Error::NoProfile)
        })
    }

    /// Every known user, with device groups loaded.
    /// Record that a conversation saw traffic, if this is the newest we know.
    ///
    /// Peering reads this to decide who is worth dialling, so a conversation
    /// that never records activity is one this device will never reach out to
    /// again. It only ever moves forward: a message arriving late must not
    /// drag a conversation's recency backwards.
    pub fn note_user_activity(&self, user_id: Uuid, at: i64) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "UPDATE users SET last_activity = ?2 WHERE id = ?1 AND last_activity < ?2",
                params![uuid_bytes(user_id), at],
            )?;
            Ok(())
        })
    }

    /// Record that a conversation was opened, whichever kind it is.
    ///
    /// Written on its own rather than through `save_group`, whose upsert
    /// deliberately leaves the locally-owned columns alone: everything it
    /// writes comes from consensus, and a recomputation must not overwrite a
    /// choice this device made about its own view.
    pub fn note_conversation_opened(&self, conversation: Uuid, at: i64) -> Result<()> {
        self.with(|connection| {
            let changed = connection.execute(
                "UPDATE groups SET last_opened = ?2 WHERE id = ?1",
                params![uuid_bytes(conversation), at],
            )?;
            if changed == 0 {
                connection.execute(
                    "UPDATE users SET last_opened = ?2 WHERE id = ?1",
                    params![uuid_bytes(conversation), at],
                )?;
            }
            Ok(())
        })
    }

    /// The same, for a group.
    pub fn note_group_activity(&self, group_id: Uuid, at: i64) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "UPDATE groups SET last_activity = ?2 WHERE id = ?1 AND last_activity < ?2",
                params![uuid_bytes(group_id), at],
            )?;
            Ok(())
        })
    }

    pub fn all_users(&self) -> Result<Vec<User>> {
        let mut users = self.with(|connection| {
            let mut statement = connection.prepare("SELECT * FROM users ORDER BY name")?;
            let rows = statement.query_map([], row_to_user)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })?;

        for user in &mut users {
            user.devices = self.devices_for_user(user.id)?;
        }
        Ok(users)
    }

    /// Mark a set of users as accepted.
    ///
    /// "Accepted" means this device's owner has knowingly agreed to be in a
    /// group with them — which is what accepting an invitation asserts about
    /// everyone already in it. Nothing else reads it, and it never leaves the
    /// device: the auto-join policy is its only consumer.
    pub fn mark_users_accepted(&self, ids: &[Uuid]) -> Result<()> {
        self.with(|connection| {
            for id in ids {
                connection.execute(
                    "UPDATE users SET accepted = 1 WHERE id = ?1",
                    params![uuid_bytes(*id)],
                )?;
            }
            Ok(())
        })
    }

    /// Overwrite the mutable, locally-owned fields of a user row.
    /// Replace this profile's user-level keys.
    ///
    /// Separate from [`Store::save_user`] because that deliberately refuses to
    /// touch key columns on conflict: a user record relayed from a peer must
    /// never be able to overwrite our own keys, which is exactly what an upsert
    /// would let it do. Rolling keys is the one legitimate reason to write
    /// them, so it gets its own statement — and one restricted to the profile
    /// row, so it cannot be aimed at somebody else.
    pub fn replace_profile_keys(
        &self,
        user_id: Uuid,
        public_ecdsa: &[u8],
        private_ecdsa: &[u8],
        public_ecdh: &[u8],
        private_ecdh: &[u8],
    ) -> Result<()> {
        self.with(|connection| {
            let changed = connection.execute(
                "UPDATE users
                 SET public_ecdsa_key = ?2, private_ecdsa_key = ?3,
                     public_ecdh_key = ?4, private_ecdh_key = ?5
                 WHERE id = ?1 AND profile = 1",
                params![
                    uuid_bytes(user_id),
                    public_ecdsa,
                    private_ecdsa,
                    public_ecdh,
                    private_ecdh,
                ],
            )?;
            if changed == 0 {
                return Err(Error::NoProfile);
            }
            Ok(())
        })
    }

    pub fn update_user_local_state(&self, user: &User) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"UPDATE users SET
                    open_dm = ?2, last_opened = ?3, retention = ?4, clear_before = ?5,
                    muted_until = ?6, last_activity = ?7, alias = ?8, notes = ?9,
                    blocked = ?10, accepted = ?11
                   WHERE id = ?1"#,
                params![
                    uuid_bytes(user.id),
                    user.open_dm,
                    user.last_opened,
                    user.retention,
                    user.clear_before,
                    user.muted_until,
                    user.last_activity,
                    user.alias,
                    user.notes,
                    user.blocked,
                    user.accepted,
                ],
            )?;
            Ok(())
        })
    }

    // ---------------------------------------------------------------------
    // Devices
    // ---------------------------------------------------------------------

    /// Insert or update a device.
    ///
    /// Conflicts resolve on the **address**, not the row id. An address is a
    /// public key and so is the device's real identity, whereas the id is a
    /// UUID the sending peer chose. Keying on the id would let a record about
    /// one user reuse an id belonging to another user's device and overwrite
    /// its revocation state.
    pub fn save_device(&self, device: &Device) -> Result<()> {
        self.with(|connection| {
            // Refuse an id already held by a different address, which would
            // otherwise violate the primary key or silently rebind it.
            let clashing: Option<String> = connection
                .query_row(
                    "SELECT address FROM devices WHERE id = ?1",
                    params![uuid_bytes(device.id)],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(existing_address) = clashing {
                if existing_address != device.address {
                    return Err(Error::InvalidFrame(format!(
                        "device id {} is already held by {existing_address}",
                        device.id
                    )));
                }
            }

            connection.execute(
                r#"INSERT INTO devices (
                    id, user_id, name, address, timestamp, saved_at, last_seen,
                    revoked_at, ecdh_public_key, ecdh_private_key
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT (address) DO UPDATE SET
                    name = excluded.name,
                    last_seen = excluded.last_seen,
                    -- A revocation is permanent: never un-revoke, and keep the
                    -- earliest time, since it decides which historic frames
                    -- remain valid.
                    revoked_at = CASE
                        WHEN devices.revoked_at = 0 THEN excluded.revoked_at
                        ELSE devices.revoked_at
                    END,
                    ecdh_public_key = excluded.ecdh_public_key
                WHERE devices.user_id = excluded.user_id"#,
                params![
                    uuid_bytes(device.id),
                    uuid_bytes(device.user_id),
                    device.name,
                    device.address,
                    device.timestamp,
                    device.saved_at,
                    device.last_seen,
                    device.revoked_at,
                    device.ecdh_public_key,
                    device.ecdh_private_key,
                ],
            )?;

            if let Some(signature) = &device.signature {
                connection.execute(
                    r#"INSERT INTO introduction_signatures (
                        id, device_id, preexisting_device,
                        signature_of_new_device, signature_of_preexisting_device
                    ) VALUES (?1, ?2, ?3, ?4, ?5)
                    ON CONFLICT (device_id) DO NOTHING"#,
                    params![
                        uuid_bytes(signature.id),
                        uuid_bytes(device.id),
                        signature.preexisting_device,
                        signature.signature_of_new_device,
                        signature.signature_of_preexisting_device,
                    ],
                )?;
            }
            Ok(())
        })
    }

    pub fn device_by_address(&self, address: &str) -> Result<Option<Device>> {
        self.with(|connection| {
            let device = connection
                .query_row(
                    "SELECT * FROM devices WHERE address = ?1",
                    params![address],
                    row_to_device,
                )
                .optional()?;
            match device {
                Some(mut device) => {
                    device.signature = load_signature(connection, device.id)?;
                    Ok(Some(device))
                }
                None => Ok(None),
            }
        })
    }

    /// Every device this profile knows of, its own included.
    ///
    /// Peering walks this to find devices it still owes frames to, which is
    /// the one case where recency is not allowed to decide.
    pub fn all_devices(&self) -> Result<Vec<Device>> {
        self.with(|connection| {
            let mut statement = connection.prepare("SELECT * FROM devices")?;
            let rows = statement.query_map([], row_to_device)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    pub fn devices_for_user(&self, user_id: Uuid) -> Result<Vec<Device>> {
        self.with(|connection| {
            let mut statement =
                connection.prepare("SELECT * FROM devices WHERE user_id = ?1 ORDER BY timestamp")?;
            let mut devices: Vec<Device> = statement
                .query_map(params![uuid_bytes(user_id)], row_to_device)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for device in &mut devices {
                device.signature = load_signature(connection, device.id)?;
            }
            Ok(devices)
        })
    }

    pub fn all_device_addresses(&self) -> Result<Vec<String>> {
        self.with(|connection| {
            let mut statement =
                connection.prepare("SELECT address FROM devices WHERE revoked_at = 0")?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    /// The user who owns a device address, if we know it.
    pub fn device_owner(&self, address: &str) -> Result<Option<Uuid>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT user_id FROM devices WHERE address = ?1",
                    params![address],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()?
                .and_then(|bytes| Uuid::from_slice(&bytes).ok()))
        })
    }

    pub fn mark_device_seen(&self, address: &str, at: i64) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "UPDATE devices SET last_seen = ?2 WHERE address = ?1",
                params![address, at],
            )?;
            Ok(())
        })
    }

    /// Store a change to the profile-wide settings.
    pub fn save_update_settings(&self, update: &UpdateSettings) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"INSERT INTO update_settings (
                    id, type, data, timestamp, saved_at, author,
                    signer, original_payload, signature
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                ON CONFLICT (id) DO NOTHING"#,
                params![
                    uuid_bytes(update.id),
                    update.update_type,
                    update.data,
                    update.timestamp,
                    update.saved_at,
                    uuid_bytes(update.author),
                    update.signed.signer,
                    update.signed.original_payload,
                    update.signed.signature,
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_settings_frame(&self, id: Uuid) -> Result<Option<UpdateSettings>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM update_settings WHERE id = ?1",
                    params![uuid_bytes(id)],
                    row_to_update_settings,
                )
                .optional()?)
        })
    }

    /// One device, by ID.
    pub fn device_by_id(&self, id: Uuid) -> Result<Option<Device>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM devices WHERE id = ?1",
                    params![uuid_bytes(id)],
                    row_to_device,
                )
                .optional()?)
        })
    }

    /// Store a change to a device group.
    ///
    /// Kept as a frame, not merely applied, because a revocation has to reach
    /// every contact and the reference flow can only offer what it can find.
    pub fn save_update_device(&self, update: &UpdateDevice) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"INSERT INTO update_devices (
                    id, target, type, data, timestamp, saved_at, author,
                    signer, original_payload, signature
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT (id) DO NOTHING"#,
                params![
                    uuid_bytes(update.id),
                    uuid_bytes(update.target),
                    update.update_type,
                    update.data,
                    update.timestamp,
                    update.saved_at,
                    uuid_bytes(update.author),
                    update.signed.signer,
                    update.signed.original_payload,
                    update.signed.signature,
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_device(&self, id: Uuid) -> Result<Option<UpdateDevice>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM update_devices WHERE id = ?1",
                    params![uuid_bytes(id)],
                    row_to_update_device,
                )
                .optional()?)
        })
    }

    pub fn revoke_device(&self, address: &str, at: i64) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "UPDATE devices SET revoked_at = ?2 WHERE address = ?1 AND revoked_at = 0",
                params![address, at],
            )?;
            Ok(())
        })
    }

    pub fn rename_device(&self, id: Uuid, name: &str) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "UPDATE devices SET name = ?2 WHERE id = ?1",
                params![uuid_bytes(id), name],
            )?;
            Ok(())
        })
    }

    // ---------------------------------------------------------------------
    // Profile settings
    // ---------------------------------------------------------------------

    pub fn save_profile_settings(&self, settings: &ProfileSettings) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"INSERT INTO profile_settings (
                    id, user_id, blocked_groups, default_group_retention,
                    default_send_read_receipts, default_send_typing_indicators,
                    new_group_restrict_user_management, new_group_restrict_group_edits,
                    new_group_restrict_posting, auto_join_groups, default_dm_retention
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                ON CONFLICT (user_id) DO UPDATE SET
                    blocked_groups = excluded.blocked_groups,
                    default_group_retention = excluded.default_group_retention,
                    default_send_read_receipts = excluded.default_send_read_receipts,
                    default_send_typing_indicators = excluded.default_send_typing_indicators,
                    new_group_restrict_user_management = excluded.new_group_restrict_user_management,
                    new_group_restrict_group_edits = excluded.new_group_restrict_group_edits,
                    new_group_restrict_posting = excluded.new_group_restrict_posting,
                    auto_join_groups = excluded.auto_join_groups,
                    default_dm_retention = excluded.default_dm_retention"#,
                params![
                    uuid_bytes(settings.id),
                    uuid_bytes(settings.user_id),
                    settings.blocked_groups,
                    settings.default_group_retention,
                    settings.default_send_read_receipts,
                    settings.default_send_typing_indicators,
                    settings.new_group_restrict_user_management,
                    settings.new_group_restrict_group_edits,
                    settings.new_group_restrict_posting,
                    settings.auto_join_groups,
                    settings.default_dm_retention,
                ],
            )?;
            Ok(())
        })
    }

    pub fn profile_settings(&self, user_id: Uuid) -> Result<Option<ProfileSettings>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM profile_settings WHERE user_id = ?1",
                    params![uuid_bytes(user_id)],
                    |row| {
                        Ok(ProfileSettings {
                            id: row_uuid(row, "id")?,
                            user_id: row_uuid(row, "user_id")?,
                            blocked_groups: row.get("blocked_groups")?,
                            default_group_retention: row.get("default_group_retention")?,
                            default_send_read_receipts: row.get("default_send_read_receipts")?,
                            default_send_typing_indicators: row
                                .get("default_send_typing_indicators")?,
                            new_group_restrict_user_management: row
                                .get("new_group_restrict_user_management")?,
                            new_group_restrict_group_edits: row
                                .get("new_group_restrict_group_edits")?,
                            new_group_restrict_posting: row.get("new_group_restrict_posting")?,
                            auto_join_groups: row.get("auto_join_groups")?,
                            default_dm_retention: row.get("default_dm_retention")?,
                        })
                    },
                )
                .optional()?)
        })
    }

    // ---------------------------------------------------------------------
    // Direct messages
    // ---------------------------------------------------------------------

    pub fn save_direct_message(&self, message: &DirectMessage) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"INSERT INTO direct_messages (
                    id, saved_at, written_at, delete_at, seen, undeliverable,
                    author, xor, text, signer, original_payload, signature
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                ON CONFLICT (id) DO NOTHING"#,
                params![
                    uuid_bytes(message.id),
                    message.saved_at,
                    message.written_at,
                    message.delete_at,
                    message.seen,
                    message.undeliverable,
                    uuid_bytes(message.author),
                    uuid_bytes(message.xor),
                    message.text,
                    message.signed.signer,
                    message.signed.original_payload,
                    message.signed.signature,
                ],
            )?;
            save_attachments(
                connection,
                message.id,
                &message.file_attachments,
                &message.image_attachments,
            )
        })
    }

    pub fn direct_message(&self, id: Uuid) -> Result<Option<DirectMessage>> {
        self.with(|connection| {
            let message = connection
                .query_row(
                    "SELECT * FROM direct_messages WHERE id = ?1",
                    params![uuid_bytes(id)],
                    row_to_direct_message,
                )
                .optional()?;
            match message {
                Some(mut message) => {
                    load_attachments(connection, &mut message)?;
                    Ok(Some(message))
                }
                None => Ok(None),
            }
        })
    }

    /// Messages in a conversation, oldest first.
    pub fn direct_messages_for_thread(&self, xor: Uuid, limit: i64) -> Result<Vec<DirectMessage>> {
        self.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT * FROM direct_messages WHERE xor = ?1
                 ORDER BY written_at DESC, id DESC LIMIT ?2",
            )?;
            let mut messages: Vec<DirectMessage> = statement
                .query_map(params![uuid_bytes(xor), limit], row_to_direct_message)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            // Queried newest-first so the limit takes the most recent, then
            // reversed so callers get chronological order.
            messages.reverse();
            for message in &mut messages {
                load_attachments(connection, message)?;
            }
            Ok(messages)
        })
    }

    pub fn mark_direct_message_seen(&self, id: Uuid) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "UPDATE direct_messages SET seen = 1 WHERE id = ?1",
                params![uuid_bytes(id)],
            )?;
            Ok(())
        })
    }

    pub fn mark_direct_message_undeliverable(&self, id: Uuid) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "UPDATE direct_messages SET undeliverable = 1 WHERE id = ?1",
                params![uuid_bytes(id)],
            )?;
            Ok(())
        })
    }

    pub fn mark_group_message_undeliverable(&self, id: Uuid) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "UPDATE group_messages SET undeliverable = 1 WHERE id = ?1",
                params![uuid_bytes(id)],
            )?;
            Ok(())
        })
    }

    /// Mark every message written before `cutoff` that has never reached a
    /// single device, returning what changed.
    ///
    /// "Never reached anybody" is the absence of a delivery record for the
    /// frame — not the absence of one for a particular peer — because a message
    /// that got to one of the recipient's devices is delivered. The flag is
    /// advisory: the message is kept, and it never crosses the wire.
    pub fn mark_stale_messages_undeliverable(&self, cutoff: i64) -> Result<Vec<Uuid>> {
        self.with(|connection| {
            let mut marked = Vec::new();

            for (table, frame_type) in [
                ("direct_messages", FrameType::DirectMessage),
                ("group_messages", FrameType::GroupMessage),
            ] {
                let mut statement = connection.prepare(&format!(
                    "SELECT m.id FROM {table} AS m
                     LEFT JOIN delivery_records AS d
                       ON d.frame_id = m.id AND d.frame_type = ?1
                     WHERE d.id IS NULL AND m.undeliverable = 0 AND m.written_at <= ?2"
                ))?;
                let rows = statement.query_map(params![frame_type.as_u16(), cutoff], |row| {
                    row.get::<_, Vec<u8>>(0)
                })?;
                let ids: Vec<Uuid> = rows
                    .collect::<std::result::Result<Vec<_>, _>>()?
                    .into_iter()
                    .filter_map(|bytes| Uuid::from_slice(&bytes).ok())
                    .collect();

                for id in &ids {
                    connection.execute(
                        &format!("UPDATE {table} SET undeliverable = 1 WHERE id = ?1"),
                        params![uuid_bytes(*id)],
                    )?;
                }
                marked.extend(ids);
            }

            Ok(marked)
        })
    }

    // ---------------------------------------------------------------------
    // Group messages
    // ---------------------------------------------------------------------

    pub fn save_group_message(&self, message: &GroupMessage) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"INSERT INTO group_messages (
                    id, saved_at, written_at, delete_at, seen, undeliverable,
                    author, destination, text, signer, original_payload, signature
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                ON CONFLICT (id) DO NOTHING"#,
                params![
                    uuid_bytes(message.id),
                    message.saved_at,
                    message.written_at,
                    message.delete_at,
                    message.seen,
                    message.undeliverable,
                    uuid_bytes(message.author),
                    uuid_bytes(message.destination),
                    message.text,
                    message.signed.signer,
                    message.signed.original_payload,
                    message.signed.signature,
                ],
            )?;
            save_attachments(
                connection,
                message.id,
                &message.file_attachments,
                &message.image_attachments,
            )
        })
    }

    pub fn group_message(&self, id: Uuid) -> Result<Option<GroupMessage>> {
        self.with(|connection| {
            let message = connection
                .query_row(
                    "SELECT * FROM group_messages WHERE id = ?1",
                    params![uuid_bytes(id)],
                    row_to_group_message,
                )
                .optional()?;
            match message {
                Some(mut message) => {
                    load_attachments(connection, &mut message)?;
                    Ok(Some(message))
                }
                None => Ok(None),
            }
        })
    }

    pub fn group_messages_for_thread(&self, group: Uuid, limit: i64) -> Result<Vec<GroupMessage>> {
        self.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT * FROM group_messages WHERE destination = ?1
                 ORDER BY written_at DESC, id DESC LIMIT ?2",
            )?;
            let mut messages: Vec<GroupMessage> = statement
                .query_map(params![uuid_bytes(group), limit], row_to_group_message)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            messages.reverse();
            for message in &mut messages {
                load_attachments(connection, message)?;
            }
            Ok(messages)
        })
    }

    // ---------------------------------------------------------------------
    // Contact introduction
    // ---------------------------------------------------------------------

    /// Replace the current pairing offer.
    ///
    /// At most one offer is live per device: displaying a new code invalidates
    /// whatever was on screen before, so a code photographed off someone's
    /// shoulder stops working the moment they open the screen again.
    pub fn replace_pairing_offer(&self, offer: &SyncDeviceOffer) -> Result<()> {
        self.with(|connection| {
            connection.execute("DELETE FROM pairing_offers", [])?;
            connection.execute(
                "INSERT INTO pairing_offers (id, timestamp, secret) VALUES (?1, ?2, ?3)",
                params![uuid_bytes(offer.id), offer.timestamp, offer.secret],
            )?;
            Ok(())
        })
    }

    pub fn pairing_offer_by_secret(&self, secret: &str) -> Result<Option<SyncDeviceOffer>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM pairing_offers WHERE secret = ?1",
                    params![secret],
                    |row| {
                        Ok(SyncDeviceOffer {
                            id: row_uuid(row, "id")?,
                            timestamp: row.get("timestamp")?,
                            secret: row.get("secret")?,
                        })
                    },
                )
                .optional()?)
        })
    }

    /// Consume a secret: delete the offer and record the secret as spent.
    ///
    /// Deliberately done *before* the expiry check, so a secret presented too
    /// late is still burned rather than left usable by whoever else saw it.
    /// Replace whatever device-pairing offer was outstanding.
    ///
    /// A separate table from [`Store::replace_pairing_offer`] on purpose; see
    /// the schema. Only one is ever live, so a secret shown earlier stops
    /// working the moment a new one is displayed.
    pub fn replace_sync_offer(&self, offer: &SyncDeviceOffer) -> Result<()> {
        self.with(|connection| {
            connection.execute("DELETE FROM sync_device_offers", [])?;
            connection.execute(
                "INSERT INTO sync_device_offers (id, timestamp, secret) VALUES (?1, ?2, ?3)",
                params![uuid_bytes(offer.id), offer.timestamp, offer.secret],
            )?;
            Ok(())
        })
    }

    /// Drop the outstanding device-pairing offer, spent or abandoned.
    pub fn clear_sync_offers(&self) -> Result<()> {
        self.with(|connection| {
            connection.execute("DELETE FROM sync_device_offers", [])?;
            Ok(())
        })
    }

    /// Look up an outstanding device-pairing offer.
    pub fn sync_offer_by_secret(&self, secret: &str) -> Result<Option<SyncDeviceOffer>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM sync_device_offers WHERE secret = ?1",
                    params![secret],
                    |row| {
                        Ok(SyncDeviceOffer {
                            id: row_uuid(row, "id")?,
                            timestamp: row.get("timestamp")?,
                            secret: row.get("secret")?,
                        })
                    },
                )
                .optional()?)
        })
    }

    /// Forget what we have already sent a device.
    ///
    /// Used when a device re-pairs: its first attempt evidently did not
    /// finish, and we cannot know how much of what we sent it survived, so the
    /// reference flow is made to offer everything again.
    pub fn forget_deliveries_to(&self, address: &str) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "DELETE FROM delivery_records WHERE destination = ?1",
                params![address],
            )?;
            Ok(())
        })
    }

    pub fn burn_secret(&self, secret: &str) -> Result<()> {
        self.with(|connection| {
            connection.execute("DELETE FROM pairing_offers WHERE secret = ?1", params![secret])?;
            connection.execute(
                "INSERT INTO burned_secrets (secret, burned_at) VALUES (?1, ?2)
                 ON CONFLICT (secret) DO NOTHING",
                params![secret, crate::now()],
            )?;
            Ok(())
        })
    }

    /// Whether a secret has already been used.
    ///
    /// Unlike the Go implementation's in-memory set, this survives a restart,
    /// so a secret cannot be replayed by waiting for the app to be reopened.
    pub fn secret_is_burned(&self, secret: &str) -> Result<bool> {
        self.with(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM burned_secrets WHERE secret = ?1",
                params![secret],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        })
    }

    pub fn save_add_user(&self, record: &AddUser) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"INSERT INTO add_users (
                    id, xor, timestamp, saved_at, offer_user, requester_user,
                    offer_device, requester_device, offer_signature, requester_signature
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT (id) DO NOTHING"#,
                params![
                    uuid_bytes(record.id),
                    uuid_bytes(record.xor),
                    record.timestamp,
                    record.saved_at,
                    record.offer_user,
                    record.requester_user,
                    record.offer_device,
                    record.requester_device,
                    record.offer_signature,
                    record.requester_signature,
                ],
            )?;
            Ok(())
        })
    }

    pub fn add_user_record(&self, id: Uuid) -> Result<Option<AddUser>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM add_users WHERE id = ?1",
                    params![uuid_bytes(id)],
                    |row| {
                        Ok(AddUser {
                            id: row_uuid(row, "id")?,
                            xor: row_uuid(row, "xor")?,
                            timestamp: row.get("timestamp")?,
                            saved_at: row.get("saved_at")?,
                            offer_user: row.get("offer_user")?,
                            requester_user: row.get("requester_user")?,
                            offer_device: row.get("offer_device")?,
                            requester_device: row.get("requester_device")?,
                            offer_signature: row.get("offer_signature")?,
                            requester_signature: row.get("requester_signature")?,
                        })
                    },
                )
                .optional()?)
        })
    }

    // ---------------------------------------------------------------------
    // Read receipts
    // ---------------------------------------------------------------------

    /// Store a read receipt.
    ///
    /// `destination` and `scope` are this device's own derivation, not
    /// anything the sender claimed — see [`crate::frames::message::ReadReceipt`].
    pub fn save_read_receipt(&self, receipt: &ReadReceipt) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"INSERT INTO read_receipts (
                    id, actor, destination, scope, target, target_type,
                    timestamp, saved_at, signer, original_payload, signature
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                ON CONFLICT (id) DO NOTHING"#,
                params![
                    uuid_bytes(receipt.id),
                    uuid_bytes(receipt.actor),
                    uuid_bytes(receipt.destination),
                    receipt.scope,
                    uuid_bytes(receipt.target),
                    receipt.target_type,
                    receipt.timestamp,
                    receipt.saved_at,
                    receipt.signed.signer,
                    receipt.signed.original_payload,
                    receipt.signed.signature,
                ],
            )?;
            Ok(())
        })
    }

    pub fn read_receipt(&self, id: Uuid) -> Result<Option<ReadReceipt>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM read_receipts WHERE id = ?1",
                    params![uuid_bytes(id)],
                    row_to_read_receipt,
                )
                .optional()?)
        })
    }

    /// The users who have read a given message.
    pub fn readers_of(&self, target: Uuid) -> Result<Vec<Uuid>> {
        self.with(|connection| {
            let mut statement =
                connection.prepare("SELECT DISTINCT actor FROM read_receipts WHERE target = ?1")?;
            let rows = statement.query_map(params![uuid_bytes(target)], |row| {
                row.get::<_, Vec<u8>>(0)
            })?;
            Ok(rows
                .collect::<std::result::Result<Vec<_>, _>>()?
                .into_iter()
                .filter_map(|bytes| Uuid::from_slice(&bytes).ok())
                .collect())
        })
    }

    /// Receipts that arrived before the message they refer to.
    ///
    /// They are stored with a nil destination and sync scope because there was
    /// nothing to derive from; once the message lands they can be resolved
    /// properly.
    pub fn unresolved_read_receipts_for(&self, target: Uuid) -> Result<Vec<ReadReceipt>> {
        self.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT * FROM read_receipts WHERE target = ?1 AND destination = ?2",
            )?;
            let rows = statement.query_map(
                params![uuid_bytes(target), uuid_bytes(Uuid::nil())],
                row_to_read_receipt,
            )?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    /// Fill in the destination and scope of a receipt that arrived early.
    pub fn resolve_read_receipt(&self, id: Uuid, destination: Uuid, scope: i64) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "UPDATE read_receipts SET destination = ?2, scope = ?3 WHERE id = ?1",
                params![uuid_bytes(id), uuid_bytes(destination), scope],
            )?;
            Ok(())
        })
    }

    /// Delete every message whose retention period has run out.
    ///
    /// Zero means "kept indefinitely" rather than "expired at the epoch", which
    /// is why the predicate is on `delete_at != 0` as well as the cutoff.
    pub fn delete_expired_messages(&self, now: i64) -> Result<Vec<Uuid>> {
        self.with(|connection| {
            let mut removed = Vec::new();

            for table in ["direct_messages", "group_messages"] {
                let mut statement = connection.prepare(&format!(
                    "SELECT id FROM {table} WHERE delete_at != 0 AND delete_at <= ?1"
                ))?;
                let rows = statement.query_map(params![now], |row| row.get::<_, Vec<u8>>(0))?;
                for bytes in rows {
                    if let Ok(id) = Uuid::from_slice(&bytes?) {
                        removed.push(id);
                    }
                }

                connection.execute(
                    &format!("DELETE FROM {table} WHERE delete_at != 0 AND delete_at <= ?1"),
                    params![now],
                )?;
            }

            delete_attachments_of(connection, &removed)?;
            // Hand the freed pages back, so the bytes are gone from the file
            // and not merely unreferenced inside it.
            if !removed.is_empty() {
                connection.execute_batch("PRAGMA incremental_vacuum;")?;
            }
            Ok(removed)
        })
    }

    /// Delete a conversation's messages written before a cutoff.
    ///
    /// Returns the IDs removed, so the interface can drop them without
    /// re-reading the thread. `conversation` is a group ID or, for a direct
    /// message thread, the counterparty — the XOR is derived here.
    pub fn delete_messages_before(&self, conversation: Uuid, cutoff: i64) -> Result<Vec<Uuid>> {
        let my_id = self.my_user_id()?;
        let thread_xor = crate::xor(my_id, conversation);

        self.with(|connection| {
            let mut removed = Vec::new();

            for (table, column, key) in [
                ("group_messages", "destination", conversation),
                ("direct_messages", "xor", thread_xor),
            ] {
                let mut statement = connection.prepare(&format!(
                    "SELECT id FROM {table} WHERE {column} = ?1 AND written_at < ?2"
                ))?;
                let rows = statement.query_map(params![uuid_bytes(key), cutoff], |row| {
                    row.get::<_, Vec<u8>>(0)
                })?;
                for bytes in rows {
                    if let Ok(id) = Uuid::from_slice(&bytes?) {
                        removed.push(id);
                    }
                }

                connection.execute(
                    &format!("DELETE FROM {table} WHERE {column} = ?1 AND written_at < ?2"),
                    params![uuid_bytes(key), cutoff],
                )?;
            }

            delete_attachments_of(connection, &removed)?;
            // Hand the freed pages back, so the bytes are gone from the file
            // and not merely unreferenced inside it.
            if !removed.is_empty() {
                connection.execute_batch("PRAGMA incremental_vacuum;")?;
            }
            Ok(removed)
        })
    }

    /// Mark a message as seen, whichever kind it is.
    ///
    /// Returns whether this actually changed anything, so callers can avoid
    /// re-broadcasting a receipt for a message that was already read.
    pub fn mark_seen(&self, id: Uuid, frame_type: FrameType) -> Result<bool> {
        let table = match frame_type {
            FrameType::DirectMessage => "direct_messages",
            FrameType::GroupMessage => "group_messages",
            FrameType::UpdateGroup => "update_groups",
            // Group creations carry no seen state, and other types are not
            // markable at all.
            _ => return Ok(false),
        };

        self.with(|connection| {
            let changed = connection.execute(
                &format!("UPDATE {table} SET seen = 1 WHERE id = ?1 AND seen = 0"),
                params![uuid_bytes(id)],
            )?;
            Ok(changed > 0)
        })
    }

    // ---------------------------------------------------------------------
    // Groups
    // ---------------------------------------------------------------------

    pub fn save_group_creation(&self, creation: &GroupCreation) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"INSERT INTO group_creations (
                    id, timestamp, saved_at, data, signer, original_payload, signature
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT (id) DO NOTHING"#,
                params![
                    uuid_bytes(creation.id),
                    creation.timestamp,
                    creation.saved_at,
                    creation.data,
                    creation.signed.signer,
                    creation.signed.original_payload,
                    creation.signed.signature,
                ],
            )?;
            Ok(())
        })
    }

    pub fn group_creation(&self, id: Uuid) -> Result<Option<GroupCreation>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM group_creations WHERE id = ?1",
                    params![uuid_bytes(id)],
                    |row| {
                        Ok(GroupCreation {
                            signed: row_to_signed_frame(row)?,
                            id: row_uuid(row, "id")?,
                            timestamp: row.get("timestamp")?,
                            saved_at: row.get("saved_at")?,
                            data: row.get("data")?,
                        })
                    },
                )
                .optional()?)
        })
    }

    pub fn save_update_group(&self, update: &UpdateGroup) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"INSERT INTO update_groups (
                    id, actor, target, timestamp, saved_at, type, data, custom_scope,
                    applied, notified, seen, signer, original_payload, signature
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                ON CONFLICT (id) DO NOTHING"#,
                params![
                    uuid_bytes(update.id),
                    uuid_bytes(update.actor),
                    uuid_bytes(update.target),
                    update.timestamp,
                    update.saved_at,
                    update.update_type,
                    update.data,
                    uuid_bytes(update.custom_scope),
                    update.applied,
                    update.notified,
                    update.seen,
                    update.signed.signer,
                    update.signed.original_payload,
                    update.signed.signature,
                ],
            )?;
            for confirmation in &update.confirmations {
                save_confirmation_inner(connection, confirmation)?;
            }
            Ok(())
        })
    }

    /// Every update for a group, in timestamp order, with confirmations
    /// attached. This is what [`crate::consensus::recompute`] consumes.
    pub fn updates_for_group(&self, group: Uuid) -> Result<Vec<UpdateGroup>> {
        self.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT * FROM update_groups WHERE target = ?1 ORDER BY timestamp, id",
            )?;
            let mut updates: Vec<UpdateGroup> = statement
                .query_map(params![uuid_bytes(group)], row_to_update_group)?
                .collect::<std::result::Result<Vec<_>, _>>()?;

            for update in &mut updates {
                update.confirmations = load_confirmations(connection, update.id)?;
            }
            Ok(updates)
        })
    }

    /// Look up a single update by ID, with its confirmations attached.
    pub fn update_group(&self, id: Uuid) -> Result<Option<UpdateGroup>> {
        self.with(|connection| {
            let update = connection
                .query_row(
                    "SELECT * FROM update_groups WHERE id = ?1",
                    params![uuid_bytes(id)],
                    row_to_update_group,
                )
                .optional()?;
            match update {
                Some(mut update) => {
                    update.confirmations = load_confirmations(connection, update.id)?;
                    Ok(Some(update))
                }
                None => Ok(None),
            }
        })
    }

    /// One confirmation, by ID.
    ///
    /// The reference flow needs this to decide who a confirmation may be
    /// offered to, which it works out from the update it refers to.
    pub fn confirmation(&self, id: Uuid) -> Result<Option<Confirmation>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM confirmations WHERE id = ?1",
                    params![uuid_bytes(id)],
                    row_to_confirmation,
                )
                .optional()?)
        })
    }

    pub fn save_confirmation(&self, confirmation: &Confirmation) -> Result<()> {
        self.with(|connection| save_confirmation_inner(connection, confirmation))
    }

    /// Write a group's recomputed state.
    pub fn save_group(&self, group: &Group) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"INSERT INTO groups (
                    id, name, images, created_by, created_at, retention, clear_before,
                    muted_until, admins, invites, invited_by, invited_at, accepted_at,
                    blocked_users, restrict_user_management, restrict_group_edits,
                    restrict_posting, last_activity, read_receipts_overridden,
                    read_receipts_enabled, typing_indicators_overridden,
                    typing_indicators_enabled, last_opened
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                    ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23
                )
                ON CONFLICT (id) DO UPDATE SET
                    name = excluded.name,
                    images = excluded.images,
                    retention = excluded.retention,
                    clear_before = excluded.clear_before,
                    muted_until = excluded.muted_until,
                    admins = excluded.admins,
                    invites = excluded.invites,
                    invited_by = excluded.invited_by,
                    invited_at = excluded.invited_at,
                    accepted_at = excluded.accepted_at,
                    blocked_users = excluded.blocked_users,
                    restrict_user_management = excluded.restrict_user_management,
                    restrict_group_edits = excluded.restrict_group_edits,
                    restrict_posting = excluded.restrict_posting,
                    last_activity = excluded.last_activity"#,
                params![
                    uuid_bytes(group.id),
                    group.name,
                    group.images,
                    uuid_bytes(group.created_by),
                    group.created_at,
                    group.retention,
                    group.clear_before,
                    group.muted_until,
                    group.admins,
                    group.invites,
                    uuid_bytes(group.invited_by),
                    group.invited_at,
                    group.accepted_at,
                    group.blocked_users,
                    group.restrict_user_management,
                    group.restrict_group_edits,
                    group.restrict_posting,
                    group.last_activity,
                    group.read_receipts_overridden,
                    group.read_receipts_enabled,
                    group.typing_indicators_overridden,
                    group.typing_indicators_enabled,
                    group.last_opened,
                ],
            )?;

            // Membership is fully replaced, since it is derived state.
            connection.execute(
                "DELETE FROM group_users WHERE group_id = ?1",
                params![uuid_bytes(group.id)],
            )?;
            for user in &group.users {
                connection.execute(
                    "INSERT OR IGNORE INTO group_users (group_id, user_id) VALUES (?1, ?2)",
                    params![uuid_bytes(group.id), uuid_bytes(user.id)],
                )?;
            }
            Ok(())
        })
    }

    pub fn group(&self, id: Uuid) -> Result<Option<Group>> {
        let group = self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM groups WHERE id = ?1",
                    params![uuid_bytes(id)],
                    row_to_group,
                )
                .optional()?)
        })?;

        match group {
            Some(mut group) => {
                for member in self.group_member_ids(group.id)? {
                    if let Some(user) = self.user(member)? {
                        group.users.push(user);
                    }
                }
                Ok(Some(group))
            }
            None => Ok(None),
        }
    }

    pub fn all_groups(&self) -> Result<Vec<Group>> {
        let ids = self.with(|connection| {
            let mut statement = connection.prepare("SELECT id FROM groups")?;
            let rows = statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
            Ok(rows
                .collect::<std::result::Result<Vec<_>, _>>()?
                .into_iter()
                .filter_map(|bytes| Uuid::from_slice(&bytes).ok())
                .collect::<Vec<_>>())
        })?;

        let mut groups = Vec::new();
        for id in ids {
            if let Some(group) = self.group(id)? {
                groups.push(group);
            }
        }
        Ok(groups)
    }

    pub fn group_member_ids(&self, group: Uuid) -> Result<Vec<Uuid>> {
        self.with(|connection| {
            let mut statement =
                connection.prepare("SELECT user_id FROM group_users WHERE group_id = ?1")?;
            let rows = statement.query_map(params![uuid_bytes(group)], |row| {
                row.get::<_, Vec<u8>>(0)
            })?;
            Ok(rows
                .collect::<std::result::Result<Vec<_>, _>>()?
                .into_iter()
                .filter_map(|bytes| Uuid::from_slice(&bytes).ok())
                .collect())
        })
    }

    /// Users who share at least one group with `user_id`, which defines the
    /// overlap scope.
    pub fn users_sharing_a_group_with(&self, user_id: Uuid) -> Result<Vec<Uuid>> {
        self.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT DISTINCT other.user_id
                 FROM group_users AS mine
                 JOIN group_users AS other ON other.group_id = mine.group_id
                 WHERE mine.user_id = ?1 AND other.user_id != ?1",
            )?;
            let rows = statement.query_map(params![uuid_bytes(user_id)], |row| {
                row.get::<_, Vec<u8>>(0)
            })?;
            Ok(rows
                .collect::<std::result::Result<Vec<_>, _>>()?
                .into_iter()
                .filter_map(|bytes| Uuid::from_slice(&bytes).ok())
                .collect())
        })
    }

    // ---------------------------------------------------------------------
    // Delivery tracking and the reference flow
    // ---------------------------------------------------------------------

    /// Record that a frame reached a device.
    pub fn record_delivery(&self, record: &DeliveryRecord) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"INSERT INTO delivery_records (id, created_at, destination, frame_id, frame_type)
                   VALUES (?1, ?2, ?3, ?4, ?5)
                   ON CONFLICT (destination, frame_id, frame_type) DO NOTHING"#,
                params![
                    uuid_bytes(record.id),
                    record.created_at,
                    record.destination,
                    uuid_bytes(record.frame_id),
                    record.frame_type,
                ],
            )?;
            Ok(())
        })
    }

    pub fn is_delivered_to(
        &self,
        destination: &str,
        frame_id: Uuid,
        frame_type: FrameType,
    ) -> Result<bool> {
        self.with(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM delivery_records
                 WHERE destination = ?1 AND frame_id = ?2 AND frame_type = ?3",
                params![destination, uuid_bytes(frame_id), frame_type.as_u16()],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        })
    }

    /// How many devices have acknowledged a frame.
    pub fn delivery_count(&self, frame_id: Uuid, frame_type: FrameType) -> Result<i64> {
        self.with(|connection| {
            Ok(connection.query_row(
                "SELECT COUNT(*) FROM delivery_records WHERE frame_id = ?1 AND frame_type = ?2",
                params![uuid_bytes(frame_id), frame_type.as_u16()],
                |row| row.get(0),
            )?)
        })
    }

    /// Build a reference offer for a peer: every frame we hold that we have no
    /// record of delivering to them.
    ///
    /// `authorized` decides which frames the peer is entitled to; the caller
    /// supplies it because entitlement depends on scope, which depends on state
    /// this module does not interpret.
    ///
    /// A message older than [`crate::UNDELIVERABLE_AFTER_SECONDS`] is left out
    /// for anybody but our own devices. Without that floor a message that can
    /// never be delivered — the recipient's device is gone — is re-offered on
    /// every reconnection for the rest of the database's life, and the offer
    /// grows without bound. Our own devices have no floor: a second device
    /// joining a year-old profile is entitled to the whole history.
    pub fn references_not_delivered_to(
        &self,
        peer: &str,
        authorized: impl Fn(Uuid, FrameType) -> bool,
    ) -> Result<Vec<FrameReference>> {
        let own_device = match (self.device_owner(peer)?, self.my_user_id()) {
            (Some(owner), Ok(my_id)) => owner == my_id,
            _ => false,
        };
        let cutoff = if own_device {
            0
        } else {
            crate::now() - crate::UNDELIVERABLE_AFTER_SECONDS
        };

        let candidates = self.with(|connection| {
            // The frame types that participate in the reference flow, each
            // paired with the table it lives in and with whether that table
            // records when the frame was written — only messages age out.
            let sources = [
                ("direct_messages", FrameType::DirectMessage, true),
                ("group_messages", FrameType::GroupMessage, true),
                ("group_creations", FrameType::GroupCreation, false),
                ("update_groups", FrameType::UpdateGroup, false),
                ("read_receipts", FrameType::ReadReceipt, false),
                ("files", FrameType::File, false),
                ("update_dms", FrameType::UpdateDm, false),
                ("update_users", FrameType::UpdateUser, false),
                ("drafts", FrameType::Draft, false),
            ];

            let mut references = Vec::new();
            for (table, frame_type, ages_out) in sources {
                let floor = if ages_out { "AND t.written_at >= ?3" } else { "" };
                let sql = format!(
                    "SELECT t.id FROM {table} AS t
                     LEFT JOIN delivery_records AS d
                       ON d.frame_id = t.id AND d.frame_type = ?2 AND d.destination = ?1
                     WHERE d.id IS NULL {floor}"
                );
                let mut statement = connection.prepare(&sql)?;
                let id_of = |row: &Row| row.get::<_, Vec<u8>>(0);
                let rows: Vec<Vec<u8>> = if ages_out {
                    statement
                        .query_map(params![peer, frame_type.as_u16(), cutoff], id_of)?
                        .collect::<std::result::Result<Vec<_>, _>>()?
                } else {
                    statement
                        .query_map(params![peer, frame_type.as_u16()], id_of)?
                        .collect::<std::result::Result<Vec<_>, _>>()?
                };
                for bytes in rows {
                    if let Ok(id) = Uuid::from_slice(&bytes) {
                        references.push(FrameReference::new(id, frame_type));
                    }
                }
            }
            Ok(references)
        })?;

        Ok(candidates
            .into_iter()
            .filter(|reference| {
                FrameType::from_u16(reference.frame_type)
                    .map(|kind| authorized(reference.frame_id, kind))
                    .unwrap_or(false)
            })
            .collect())
    }

    /// Whether a frame is present locally, used to answer a reference offer.
    pub fn has_frame(&self, frame_id: Uuid, frame_type: FrameType) -> Result<bool> {
        let table = match frame_type {
            FrameType::DirectMessage => "direct_messages",
            FrameType::GroupMessage => "group_messages",
            FrameType::GroupCreation => "group_creations",
            FrameType::UpdateGroup => "update_groups",
            FrameType::ReadReceipt => "read_receipts",
            FrameType::Device => "devices",
            FrameType::File => "files",
            FrameType::UpdateDm => "update_dms",
            FrameType::UpdateUser => "update_users",
            FrameType::Draft => "drafts",
            _ => return Ok(false),
        };

        self.with(|connection| {
            let count: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE id = ?1"),
                params![uuid_bytes(frame_id)],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        })
    }

    /// Fetch a frame's wire payload, for inclusion in a catch up.
    pub fn frame_payload(&self, frame_id: Uuid, frame_type: FrameType) -> Result<Option<Vec<u8>>> {
        use crate::frames::Broadcastable;

        Ok(match frame_type {
            FrameType::DirectMessage => self
                .direct_message(frame_id)?
                .map(|message| message.payload())
                .transpose()?,
            FrameType::GroupMessage => self
                .group_message(frame_id)?
                .map(|message| message.payload())
                .transpose()?,
            FrameType::GroupCreation => self
                .group_creation(frame_id)?
                .map(|creation| creation.payload())
                .transpose()?,
            FrameType::UpdateGroup => self
                .update_group(frame_id)?
                .map(|update| update.payload())
                .transpose()?,
            FrameType::ReadReceipt => self
                .read_receipt(frame_id)?
                .map(|receipt| receipt.payload())
                .transpose()?,
            FrameType::File => self.file(frame_id)?.map(|file| file.payload()).transpose()?,
            FrameType::UpdateDm => self
                .update_dm(frame_id)?
                .map(|update| update.payload())
                .transpose()?,
            FrameType::Confirmation => self
                .confirmation(frame_id)?
                .map(|confirmation| confirmation.payload())
                .transpose()?,
            FrameType::UpdateDevice => self
                .update_device(frame_id)?
                .map(|update| update.payload())
                .transpose()?,
            FrameType::UpdateSettings => self
                .update_settings_frame(frame_id)?
                .map(|update| update.payload())
                .transpose()?,
            FrameType::UpdateUser => self
                .update_user(frame_id)?
                .map(|update| update.payload())
                .transpose()?,
            FrameType::Draft => self.draft(frame_id)?.map(|draft| draft.payload()).transpose()?,
            _ => None,
        })
    }

    /// When this device first stored a frame, which is the catch up sort key.
    pub fn frame_saved_at(&self, frame_id: Uuid, frame_type: FrameType) -> Result<Option<i64>> {
        let table = match frame_type {
            FrameType::DirectMessage => "direct_messages",
            FrameType::GroupMessage => "group_messages",
            FrameType::GroupCreation => "group_creations",
            FrameType::UpdateGroup => "update_groups",
            FrameType::ReadReceipt => "read_receipts",
            FrameType::File => "files",
            FrameType::UpdateDm => "update_dms",
            FrameType::UpdateUser => "update_users",
            FrameType::Draft => "drafts",
            _ => return Ok(None),
        };

        self.with(|connection| {
            Ok(connection
                .query_row(
                    &format!("SELECT saved_at FROM {table} WHERE id = ?1"),
                    params![uuid_bytes(frame_id)],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?)
        })
    }

    // ---------------------------------------------------------------------
    // Conversation updates
    // ---------------------------------------------------------------------

    /// Store a change to a direct message thread.
    ///
    /// Only the kinds both participants share are kept; see the table comment
    /// in [`schema`](super::schema).
    pub fn save_update_dm(&self, update: &UpdateDm) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"INSERT INTO update_dms (
                    id, actor, target, type, data, timestamp, saved_at, seen,
                    signer, original_payload, signature
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                ON CONFLICT (id) DO NOTHING"#,
                params![
                    uuid_bytes(update.id),
                    uuid_bytes(update.actor),
                    uuid_bytes(update.target),
                    update.update_type,
                    update.data,
                    update.timestamp,
                    update.saved_at,
                    update.seen,
                    update.signed.signer,
                    update.signed.original_payload,
                    update.signed.signature,
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_dm(&self, id: Uuid) -> Result<Option<UpdateDm>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM update_dms WHERE id = ?1",
                    params![uuid_bytes(id)],
                    row_to_update_dm,
                )
                .optional()?)
        })
    }

    /// Every stored conversation update, oldest first.
    pub fn all_update_dms(&self) -> Result<Vec<UpdateDm>> {
        self.with(|connection| {
            let mut statement =
                connection.prepare("SELECT * FROM update_dms ORDER BY timestamp, id")?;
            let rows = statement.query_map([], row_to_update_dm)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    // ---------------------------------------------------------------------
    // Profile updates
    // ---------------------------------------------------------------------

    /// Store a change to a user's profile.
    pub fn save_update_user(&self, update: &UpdateUser) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"INSERT INTO update_users (
                    id, target, type, data, previous_data, timestamp, saved_at, seen,
                    signer, original_payload, signature
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                ON CONFLICT (id) DO NOTHING"#,
                params![
                    uuid_bytes(update.id),
                    uuid_bytes(update.target),
                    update.update_type,
                    update.data,
                    update.previous_data,
                    update.timestamp,
                    update.saved_at,
                    update.seen,
                    update.signed.signer,
                    update.signed.original_payload,
                    update.signed.signature,
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_user(&self, id: Uuid) -> Result<Option<UpdateUser>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM update_users WHERE id = ?1",
                    params![uuid_bytes(id)],
                    row_to_update_user,
                )
                .optional()?)
        })
    }

    /// Every stored change to one user's profile, oldest first.
    ///
    /// This is what the profile is rebuilt from, so the order is the order the
    /// changes are applied in — never the order they arrived in.
    pub fn updates_for_user(&self, target: Uuid) -> Result<Vec<UpdateUser>> {
        self.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT * FROM update_users WHERE target = ?1 ORDER BY timestamp, id",
            )?;
            let rows = statement.query_map(params![uuid_bytes(target)], row_to_update_user)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    /// Every stored profile update, oldest first.
    pub fn all_update_users(&self) -> Result<Vec<UpdateUser>> {
        self.with(|connection| {
            let mut statement =
                connection.prepare("SELECT * FROM update_users ORDER BY timestamp, id")?;
            let rows = statement.query_map([], row_to_update_user)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    // ---------------------------------------------------------------------
    // Files and chunks
    // ---------------------------------------------------------------------

    /// Store a file's metadata.
    pub fn save_file(&self, file: &File) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"INSERT INTO files (
                    id, name, type, attached_to, hash, size, chunk_size, hash_list,
                    encrypted_hash_list, key, nonce, path, wanted, downloaded,
                    scope, destination, author, timestamp, saved_at,
                    signer, original_payload, signature
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                    ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22
                )
                ON CONFLICT (id) DO UPDATE SET
                    wanted = excluded.wanted OR files.wanted,
                    downloaded = excluded.downloaded OR files.downloaded"#,
                params![
                    uuid_bytes(file.id),
                    file.name,
                    file.file_type,
                    uuid_bytes(file.attached_to),
                    file.hash,
                    file.size,
                    file.chunk_size,
                    file.hash_list,
                    file.encrypted_hash_list,
                    file.key,
                    file.nonce,
                    file.path,
                    file.wanted,
                    file.downloaded,
                    file.scope,
                    uuid_bytes(file.destination),
                    uuid_bytes(file.author),
                    file.timestamp,
                    file.saved_at,
                    file.signed.signer,
                    file.signed.original_payload,
                    file.signed.signature,
                ],
            )?;
            Ok(())
        })
    }

    pub fn file(&self, id: Uuid) -> Result<Option<File>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM files WHERE id = ?1",
                    params![uuid_bytes(id)],
                    row_to_file,
                )
                .optional()?)
        })
    }

    /// Record a chunk's place in a file, with or without its bytes.
    ///
    /// Called with `data = None` when a file record arrives and we learn which
    /// chunks exist, then again with the bytes once each one is fetched.
    pub fn save_chunk(
        &self,
        file_id: Uuid,
        index: i64,
        hash: &str,
        data: Option<&[u8]>,
    ) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                r#"INSERT INTO chunks (id, file_id, hash, idx, downloaded, data)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                   ON CONFLICT (file_id, idx) DO UPDATE SET
                       -- Never overwrite bytes we already hold with nothing.
                       data = COALESCE(excluded.data, chunks.data),
                       downloaded = chunks.downloaded OR excluded.downloaded"#,
                params![
                    uuid_bytes(Uuid::new_v4()),
                    uuid_bytes(file_id),
                    hash,
                    index,
                    data.is_some(),
                    data,
                ],
            )?;
            Ok(())
        })
    }

    /// Record that we now hold a chunk whose bytes live on disk.
    ///
    /// A large file's chunks carry no `data`, so "do we have it" cannot be
    /// answered by looking for bytes in the row.
    pub fn mark_chunk_downloaded(&self, file_id: Uuid, index: i64) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "UPDATE chunks SET downloaded = 1 WHERE file_id = ?1 AND idx = ?2",
                params![uuid_bytes(file_id), index],
            )?;
            Ok(())
        })
    }

    /// The bytes of a chunk, by content hash.
    ///
    /// Content-addressed, so a chunk fetched for one file also serves any other
    /// file that happens to contain the same bytes.
    pub fn chunk_data(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT data FROM chunks WHERE hash = ?1 AND data IS NOT NULL LIMIT 1",
                    params![hash],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()?)
        })
    }

    /// Whether this device holds a chunk's bytes, wherever they live.
    ///
    /// Not `chunk_data(..).is_some()`: a large file's chunks are on disk and
    /// carry no row data, so that test would say no forever and the same chunk
    /// would be requested on every offer.
    pub fn has_chunk(&self, hash: &str) -> Result<bool> {
        self.with(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM chunks
                 WHERE hash = ?1 AND (data IS NOT NULL OR downloaded = 1)",
                params![hash],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        })
    }

    /// Reassemble a file, or `None` if any chunk is still missing.
    pub fn file_data(&self, id: Uuid) -> Result<Option<Vec<u8>>> {
        let Some(file) = self.file(id)? else {
            return Ok(None);
        };

        let mut assembled = Vec::with_capacity(file.size.max(0) as usize);
        for hash in file.chunk_hashes() {
            match self.chunk_data(&hash)? {
                Some(bytes) => assembled.extend_from_slice(&bytes),
                None => return Ok(None),
            }
        }
        Ok(Some(assembled))
    }

    /// How much of a file is present, from zero to one.
    ///
    /// Counted in one query rather than one per chunk: the interface asks for
    /// this once per attachment every time it builds a message view, and a
    /// timeline redraw would otherwise be thousands of round trips.
    pub fn file_progress(&self, id: Uuid) -> Result<f64> {
        self.with(|connection| {
            // A chunk counts as held if its *content* is stored anywhere, not
            // only under this file: chunks are content-addressed, so a file
            // that shares one with another file already has it. Reporting
            // otherwise would leave an assemblable file stuck below 100%, and
            // the interface only asks for the bytes once it reaches that.
            //
            // `downloaded` is checked as well as `data`, because a file too
            // large to embed keeps its bytes on disk and its rows carry no
            // data at all — counting only `data` would leave every large
            // transfer stuck at zero for ever.
            let (total, held) = connection.query_row(
                r#"SELECT COUNT(*),
                          SUM(CASE WHEN EXISTS (
                              SELECT 1 FROM chunks AS held
                              WHERE held.hash = wanted.hash
                                AND (held.data IS NOT NULL OR held.downloaded = 1)
                          ) THEN 1 ELSE 0 END)
                   FROM chunks AS wanted WHERE wanted.file_id = ?1"#,
                params![uuid_bytes(id)],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?.unwrap_or(0))),
            )?;

            // No chunk rows means we have never seen the file's metadata, not
            // that it is complete.
            if total == 0 {
                return Ok(0.0);
            }
            Ok(held as f64 / total as f64)
        })
    }

    /// Mark a file as one this device wants after all.
    pub fn mark_file_wanted(&self, id: Uuid) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "UPDATE files SET wanted = 1 WHERE id = ?1",
                params![uuid_bytes(id)],
            )?;
            Ok(())
        })
    }

    /// Record where a file's bytes will live on this device.
    pub fn set_file_path(&self, id: Uuid, path: &str) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "UPDATE files SET path = ?2 WHERE id = ?1",
                params![uuid_bytes(id), path],
            )?;
            Ok(())
        })
    }

    pub fn mark_file_downloaded(&self, id: Uuid) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "UPDATE files SET downloaded = 1 WHERE id = ?1",
                params![uuid_bytes(id)],
            )?;
            Ok(())
        })
    }

    /// Note that a device says it holds a chunk.
    ///
    /// Returns whether this was news. Chunk offers are gossiped and not stored
    /// as frames, so novelty is what stops them circulating forever.
    pub fn record_chunk_location(&self, hash: &str, address: &str, at: i64) -> Result<bool> {
        self.with(|connection| {
            let inserted = connection.execute(
                "INSERT INTO chunk_locations (hash, address, offered_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (hash, address) DO NOTHING",
                params![hash, address, at],
            )?;
            if inserted == 0 {
                // Already known, but the offer is fresher than what we had.
                connection.execute(
                    "UPDATE chunk_locations SET offered_at = ?3
                     WHERE hash = ?1 AND address = ?2 AND offered_at < ?3",
                    params![hash, address, at],
                )?;
            }
            Ok(inserted > 0)
        })
    }

    /// Forget that a device holds a chunk, after it said it does not.
    pub fn forget_chunk_location(&self, hash: &str, address: &str) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "DELETE FROM chunk_locations WHERE hash = ?1 AND address = ?2",
                params![hash, address],
            )?;
            Ok(())
        })
    }

    /// Devices that have offered a chunk, most recently seen first.
    pub fn chunk_locations(&self, hash: &str) -> Result<Vec<String>> {
        self.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT address FROM chunk_locations WHERE hash = ?1 ORDER BY offered_at DESC",
            )?;
            let rows = statement.query_map(params![hash], |row| row.get::<_, String>(0))?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    /// Which file a chunk hash belongs to, so an arriving chunk can be placed.
    pub fn file_for_chunk(&self, hash: &str) -> Result<Option<Uuid>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT file_id FROM chunks WHERE hash = ?1 LIMIT 1",
                    params![hash],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()?
                .and_then(|bytes| Uuid::from_slice(&bytes).ok()))
        })
    }

    /// Files still missing chunks, so a reconnect can resume them.
    pub fn incomplete_wanted_files(&self) -> Result<Vec<File>> {
        self.with(|connection| {
            let mut statement =
                connection.prepare("SELECT * FROM files WHERE wanted = 1 AND downloaded = 0")?;
            let rows = statement.query_map([], row_to_file)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    // ---------------------------------------------------------------------
    // Custom scopes and drafts
    // ---------------------------------------------------------------------

    pub fn save_custom_scope(&self, scope: &CustomScope) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "INSERT INTO custom_scopes (id, created_at, addresses) VALUES (?1, ?2, ?3)
                 ON CONFLICT (id) DO UPDATE SET addresses = excluded.addresses",
                params![uuid_bytes(scope.id), scope.created_at, scope.addresses],
            )?;
            Ok(())
        })
    }

    pub fn custom_scope(&self, id: Uuid) -> Result<Option<CustomScope>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM custom_scopes WHERE id = ?1",
                    params![uuid_bytes(id)],
                    |row| {
                        Ok(CustomScope {
                            id: row_uuid(row, "id")?,
                            created_at: row.get("created_at")?,
                            addresses: row.get("addresses")?,
                        })
                    },
                )
                .optional()?)
        })
    }

    /// Store a draft, replacing whatever the thread held before.
    ///
    /// The old row is deleted rather than updated in place, because the id is
    /// the frame's identity: a peer that has acknowledged the previous draft
    /// would never ask for an update filed under the same id, so editing in
    /// place would sync the first keystroke and nothing after it. Go does the
    /// same, at `chat/drafts.go:205-217`.
    pub fn save_draft(&self, draft: &Draft) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "DELETE FROM drafts WHERE thread = ?1",
                params![uuid_bytes(draft.thread)],
            )?;
            if draft.text.trim().is_empty() {
                // An emptied draft is a deleted draft.
                return Ok(());
            }
            connection.execute(
                "INSERT INTO drafts (
                    id, thread, text, timestamp, saved_at, signer, original_payload, signature
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    uuid_bytes(draft.id),
                    uuid_bytes(draft.thread),
                    draft.text,
                    draft.timestamp,
                    draft.saved_at,
                    draft.signed.signer,
                    draft.signed.original_payload,
                    draft.signed.signature,
                ],
            )?;
            Ok(())
        })
    }

    pub fn all_drafts(&self) -> Result<Vec<Draft>> {
        self.with(|connection| {
            let mut statement = connection.prepare("SELECT * FROM drafts")?;
            let rows = statement.query_map([], row_to_draft)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    pub fn draft(&self, id: Uuid) -> Result<Option<Draft>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM drafts WHERE id = ?1",
                    params![uuid_bytes(id)],
                    row_to_draft,
                )
                .optional()?)
        })
    }

    /// The draft a thread currently holds, if any.
    pub fn draft_for_thread(&self, thread: Uuid) -> Result<Option<Draft>> {
        self.with(|connection| {
            Ok(connection
                .query_row(
                    "SELECT * FROM drafts WHERE thread = ?1",
                    params![uuid_bytes(thread)],
                    row_to_draft,
                )
                .optional()?)
        })
    }
}

// -------------------------------------------------------------------------
// Row mapping
// -------------------------------------------------------------------------

fn uuid_bytes(id: Uuid) -> Vec<u8> {
    id.as_bytes().to_vec()
}

fn row_uuid(row: &Row, column: &str) -> rusqlite::Result<Uuid> {
    let bytes: Option<Vec<u8>> = row.get(column)?;
    Ok(bytes
        .and_then(|bytes| Uuid::from_slice(&bytes).ok())
        .unwrap_or_else(Uuid::nil))
}

fn row_to_signed_frame(row: &Row) -> rusqlite::Result<SignedFrame> {
    Ok(SignedFrame {
        signer: row.get("signer")?,
        original_payload: row.get("original_payload")?,
        signature: row.get("signature")?,
    })
}

fn row_to_user(row: &Row) -> rusqlite::Result<User> {
    Ok(User {
        id: row_uuid(row, "id")?,
        name: row.get("name")?,
        images: row.get("images")?,
        profile: row.get("profile")?,
        encrypted_devices: row.get("encrypted_devices")?,
        public_ecdsa_key: row.get::<_, Option<Vec<u8>>>("public_ecdsa_key")?.unwrap_or_default(),
        private_ecdsa_key: row
            .get::<_, Option<Vec<u8>>>("private_ecdsa_key")?
            .unwrap_or_default(),
        public_ecdh_key: row.get::<_, Option<Vec<u8>>>("public_ecdh_key")?.unwrap_or_default(),
        private_ecdh_key: row.get::<_, Option<Vec<u8>>>("private_ecdh_key")?.unwrap_or_default(),
        open_dm: row.get("open_dm")?,
        last_opened: row.get("last_opened")?,
        retention: row.get("retention")?,
        clear_before: row.get("clear_before")?,
        muted_until: row.get("muted_until")?,
        last_activity: row.get("last_activity")?,
        read_receipts_overridden: row.get("read_receipts_overridden")?,
        read_receipts_enabled: row.get("read_receipts_enabled")?,
        typing_indicators_overridden: row.get("typing_indicators_overridden")?,
        typing_indicators_enabled: row.get("typing_indicators_enabled")?,
        introduction_method: row.get("introduction_method")?,
        introduction_time: row.get("introduction_time")?,
        introduction_metadata: row_uuid(row, "introduction_metadata")?,
        alias: row.get("alias")?,
        notes: row.get("notes")?,
        blocked: row.get("blocked")?,
        accepted: row.get("accepted")?,
        devices: Vec::new(),
    })
}

/// Delete the attachments of a set of deleted messages, and the bytes behind
/// them.
///
/// Deleting the rows and leaving the blobs is the worst of both worlds: the
/// conversation is gone from the interface and every photo in it is still
/// recoverable from the database file, which is exactly what somebody clearing
/// their history is trying to prevent. `chunks.file_id` cascades, so removing
/// the `files` row is what frees the bytes.
fn delete_attachments_of(connection: &Connection, messages: &[Uuid]) -> Result<()> {
    for message in messages {
        let mut files = Vec::new();
        for table in ["file_attachments", "image_attachments"] {
            let mut statement =
                connection.prepare(&format!("SELECT file_id FROM {table} WHERE message_id = ?1"))?;
            let rows = statement.query_map(params![uuid_bytes(*message)], |row| {
                row.get::<_, Vec<u8>>(0)
            })?;
            for bytes in rows {
                if let Ok(id) = Uuid::from_slice(&bytes?) {
                    files.push(id);
                }
            }
            connection.execute(
                &format!("DELETE FROM {table} WHERE message_id = ?1"),
                params![uuid_bytes(*message)],
            )?;
        }

        for file in files {
            // A file still hanging off a message that survived is not ours to
            // remove.
            let referenced: i64 = connection.query_row(
                "SELECT (SELECT COUNT(*) FROM file_attachments WHERE file_id = ?1)
                      + (SELECT COUNT(*) FROM image_attachments WHERE file_id = ?1)",
                params![uuid_bytes(file)],
                |row| row.get(0),
            )?;
            if referenced > 0 {
                continue;
            }

            hand_chunks_to_surviving_files(connection, file)?;
            connection.execute("DELETE FROM files WHERE id = ?1", params![uuid_bytes(file)])?;
        }
    }
    Ok(())
}

/// Before a file's chunk rows are cascaded away, give their bytes to any file
/// that still needs them.
///
/// Chunks are content-addressed and shared: the same photo sent twice is two
/// `files` rows over one set of bytes, and only the row that actually fetched
/// them holds them — every other row for that content is an empty placeholder.
/// Deleting the holder without this would leave the surviving copy permanently
/// unopenable, with no way to fetch the content again.
fn hand_chunks_to_surviving_files(connection: &Connection, file: Uuid) -> Result<()> {
    let mut statement =
        connection.prepare("SELECT hash, data FROM chunks WHERE file_id = ?1 AND data IS NOT NULL")?;
    let held: Vec<(String, Vec<u8>)> = statement
        .query_map(params![uuid_bytes(file)], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    for (hash, data) in held {
        connection.execute(
            "UPDATE chunks SET data = ?3, downloaded = 1
             WHERE hash = ?2 AND file_id != ?1 AND data IS NULL",
            params![uuid_bytes(file), hash, data],
        )?;
    }
    Ok(())
}

fn row_to_draft(row: &Row) -> rusqlite::Result<Draft> {
    Ok(Draft {
        signed: row_to_signed_frame(row)?,
        id: row_uuid(row, "id")?,
        thread: row_uuid(row, "thread")?,
        text: row.get("text")?,
        timestamp: row.get("timestamp")?,
        saved: true,
        saved_at: row.get("saved_at")?,
    })
}

fn row_to_update_user(row: &Row) -> rusqlite::Result<UpdateUser> {
    Ok(UpdateUser {
        signed: row_to_signed_frame(row)?,
        id: row_uuid(row, "id")?,
        target: row_uuid(row, "target")?,
        update_type: row.get("type")?,
        data: row.get::<_, Option<Vec<u8>>>("data")?.unwrap_or_default(),
        previous_data: row
            .get::<_, Option<Vec<u8>>>("previous_data")?
            .unwrap_or_default(),
        timestamp: row.get("timestamp")?,
        saved_at: row.get("saved_at")?,
        seen: row.get("seen")?,
    })
}

fn row_to_update_settings(row: &Row) -> rusqlite::Result<UpdateSettings> {
    Ok(UpdateSettings {
        signed: row_to_signed_frame(row)?,
        id: row_uuid(row, "id")?,
        update_type: row.get("type")?,
        data: row.get::<_, Option<Vec<u8>>>("data")?.unwrap_or_default(),
        timestamp: row.get("timestamp")?,
        saved_at: row.get("saved_at")?,
        author: row_uuid(row, "author")?,
    })
}

fn row_to_update_device(row: &Row) -> rusqlite::Result<UpdateDevice> {
    Ok(UpdateDevice {
        signed: row_to_signed_frame(row)?,
        id: row_uuid(row, "id")?,
        target: row_uuid(row, "target")?,
        update_type: row.get("type")?,
        data: row.get::<_, Option<Vec<u8>>>("data")?.unwrap_or_default(),
        timestamp: row.get("timestamp")?,
        saved_at: row.get("saved_at")?,
        author: row_uuid(row, "author")?,
    })
}

fn row_to_update_dm(row: &Row) -> rusqlite::Result<UpdateDm> {
    Ok(UpdateDm {
        signed: row_to_signed_frame(row)?,
        id: row_uuid(row, "id")?,
        actor: row_uuid(row, "actor")?,
        target: row_uuid(row, "target")?,
        timestamp: row.get("timestamp")?,
        saved_at: row.get("saved_at")?,
        seen: row.get("seen")?,
        update_type: row.get("type")?,
        data: row.get::<_, Option<Vec<u8>>>("data")?.unwrap_or_default(),
    })
}

fn row_to_file(row: &Row) -> rusqlite::Result<File> {
    Ok(File {
        signed: row_to_signed_frame(row)?,
        id: row_uuid(row, "id")?,
        name: row.get("name")?,
        file_type: row.get("type")?,
        attached_to: row_uuid(row, "attached_to")?,
        hash: row.get("hash")?,
        size: row.get("size")?,
        chunk_size: row.get("chunk_size")?,
        hash_list: row.get("hash_list")?,
        encrypted_hash_list: row.get("encrypted_hash_list")?,
        key: row.get::<_, Option<Vec<u8>>>("key")?.unwrap_or_default(),
        nonce: row.get::<_, Option<Vec<u8>>>("nonce")?.unwrap_or_default(),
        path: row.get("path")?,
        wanted: row.get("wanted")?,
        downloaded: row.get("downloaded")?,
        scope: row.get("scope")?,
        destination: row_uuid(row, "destination")?,
        author: row_uuid(row, "author")?,
        timestamp: row.get("timestamp")?,
        saved_at: row.get("saved_at")?,
    })
}

fn row_to_device(row: &Row) -> rusqlite::Result<Device> {
    Ok(Device {
        id: row_uuid(row, "id")?,
        name: row.get("name")?,
        user_id: row_uuid(row, "user_id")?,
        address: row.get("address")?,
        timestamp: row.get("timestamp")?,
        saved_at: row.get("saved_at")?,
        last_seen: row.get("last_seen")?,
        revoked_at: row.get("revoked_at")?,
        ecdh_public_key: row.get::<_, Option<Vec<u8>>>("ecdh_public_key")?.unwrap_or_default(),
        ecdh_private_key: row
            .get::<_, Option<Vec<u8>>>("ecdh_private_key")?
            .unwrap_or_default(),
        signature: None,
    })
}

fn load_signature(connection: &Connection, device_id: Uuid) -> Result<Option<IntroductionSignature>> {
    Ok(connection
        .query_row(
            "SELECT * FROM introduction_signatures WHERE device_id = ?1",
            params![uuid_bytes(device_id)],
            |row| {
                Ok(IntroductionSignature {
                    id: row_uuid(row, "id")?,
                    device_id: row_uuid(row, "device_id")?,
                    preexisting_device: row.get("preexisting_device")?,
                    signature_of_new_device: row.get("signature_of_new_device")?,
                    signature_of_preexisting_device: row.get("signature_of_preexisting_device")?,
                })
            },
        )
        .optional()?)
}

fn row_to_direct_message(row: &Row) -> rusqlite::Result<DirectMessage> {
    Ok(DirectMessage {
        signed: row_to_signed_frame(row)?,
        id: row_uuid(row, "id")?,
        saved_at: row.get("saved_at")?,
        written_at: row.get("written_at")?,
        delete_at: row.get("delete_at")?,
        seen: row.get("seen")?,
        undeliverable: row.get("undeliverable")?,
        author: row_uuid(row, "author")?,
        xor: row_uuid(row, "xor")?,
        text: row.get("text")?,
        file_attachments: Vec::new(),
        image_attachments: Vec::new(),
    })
}

fn row_to_group_message(row: &Row) -> rusqlite::Result<GroupMessage> {
    Ok(GroupMessage {
        signed: row_to_signed_frame(row)?,
        id: row_uuid(row, "id")?,
        saved_at: row.get("saved_at")?,
        written_at: row.get("written_at")?,
        delete_at: row.get("delete_at")?,
        seen: row.get("seen")?,
        undeliverable: row.get("undeliverable")?,
        author: row_uuid(row, "author")?,
        destination: row_uuid(row, "destination")?,
        text: row.get("text")?,
        file_attachments: Vec::new(),
        image_attachments: Vec::new(),
    })
}

fn row_to_read_receipt(row: &Row) -> rusqlite::Result<ReadReceipt> {
    Ok(ReadReceipt {
        signed: row_to_signed_frame(row)?,
        id: row_uuid(row, "id")?,
        actor: row_uuid(row, "actor")?,
        destination: row_uuid(row, "destination")?,
        scope: row.get("scope")?,
        target: row_uuid(row, "target")?,
        target_type: row.get("target_type")?,
        timestamp: row.get("timestamp")?,
        saved_at: row.get("saved_at")?,
    })
}

fn row_to_group(row: &Row) -> rusqlite::Result<Group> {
    Ok(Group {
        id: row_uuid(row, "id")?,
        name: row.get("name")?,
        images: row.get("images")?,
        created_by: row_uuid(row, "created_by")?,
        created_at: row.get("created_at")?,
        retention: row.get("retention")?,
        clear_before: row.get("clear_before")?,
        muted_until: row.get("muted_until")?,
        users: Vec::new(),
        admins: row.get("admins")?,
        invites: row.get("invites")?,
        invited_by: row_uuid(row, "invited_by")?,
        invited_at: row.get("invited_at")?,
        accepted_at: row.get("accepted_at")?,
        blocked_users: row.get("blocked_users")?,
        restrict_user_management: row.get("restrict_user_management")?,
        restrict_group_edits: row.get("restrict_group_edits")?,
        restrict_posting: row.get("restrict_posting")?,
        last_activity: row.get("last_activity")?,
        read_receipts_overridden: row.get("read_receipts_overridden")?,
        read_receipts_enabled: row.get("read_receipts_enabled")?,
        typing_indicators_overridden: row.get("typing_indicators_overridden")?,
        typing_indicators_enabled: row.get("typing_indicators_enabled")?,
        delivery_records_cleared_for: Uuid::nil(),
        last_opened: row.get("last_opened")?,
    })
}

fn row_to_update_group(row: &Row) -> rusqlite::Result<UpdateGroup> {
    Ok(UpdateGroup {
        signed: row_to_signed_frame(row)?,
        id: row_uuid(row, "id")?,
        actor: row_uuid(row, "actor")?,
        target: row_uuid(row, "target")?,
        timestamp: row.get("timestamp")?,
        saved_at: row.get("saved_at")?,
        update_type: row.get("type")?,
        data: row.get("data")?,
        custom_scope: row_uuid(row, "custom_scope")?,
        confirmations: Vec::new(),
        applied: row.get("applied")?,
        notified: row.get("notified")?,
        seen: row.get("seen")?,
    })
}

fn save_confirmation_inner(connection: &Connection, confirmation: &Confirmation) -> Result<()> {
    connection.execute(
        r#"INSERT INTO confirmations (
            id, update_group_id, destination, author, custom_scope,
            signing_device, signature, timestamp, saved_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT (id) DO NOTHING"#,
        params![
            uuid_bytes(confirmation.id),
            uuid_bytes(confirmation.update_group_id),
            uuid_bytes(confirmation.destination),
            uuid_bytes(confirmation.author),
            uuid_bytes(confirmation.custom_scope),
            confirmation.signing_device,
            confirmation.signature,
            confirmation.timestamp,
            confirmation.saved_at,
        ],
    )?;
    Ok(())
}

fn row_to_confirmation(row: &Row) -> rusqlite::Result<Confirmation> {
    Ok(Confirmation {
        id: row_uuid(row, "id")?,
        update_group_id: row_uuid(row, "update_group_id")?,
        destination: row_uuid(row, "destination")?,
        author: row_uuid(row, "author")?,
        custom_scope: row_uuid(row, "custom_scope")?,
        signing_device: row.get("signing_device")?,
        signature: row.get("signature")?,
        timestamp: row.get("timestamp")?,
        saved_at: row.get("saved_at")?,
    })
}

fn load_confirmations(connection: &Connection, update_id: Uuid) -> Result<Vec<Confirmation>> {
    let mut statement =
        connection.prepare("SELECT * FROM confirmations WHERE update_group_id = ?1")?;
    let rows = statement.query_map(params![uuid_bytes(update_id)], row_to_confirmation)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn save_attachments(
    connection: &Connection,
    message_id: Uuid,
    files: &[FileAttachment],
    images: &[ImageAttachment],
) -> Result<()> {
    for attachment in files {
        connection.execute(
            "INSERT INTO file_attachments (id, file_id, message_id, name, size)
             VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT (id) DO NOTHING",
            params![
                uuid_bytes(attachment.id),
                uuid_bytes(attachment.file_id),
                uuid_bytes(message_id),
                attachment.name,
                attachment.size,
            ],
        )?;
    }
    for attachment in images {
        connection.execute(
            "INSERT INTO image_attachments
                (id, file_id, message_id, name, size, width, height, blur_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT (id) DO NOTHING",
            params![
                uuid_bytes(attachment.id),
                uuid_bytes(attachment.file_id),
                uuid_bytes(message_id),
                attachment.name,
                attachment.size,
                attachment.width,
                attachment.height,
                attachment.blur_hash,
            ],
        )?;
    }
    Ok(())
}

/// A message that can carry attachments, so one loader serves both kinds.
///
/// The attachments were always saved for both, but only direct messages were
/// ever read back — so a group's pictures survived a restart in the database
/// and vanished from the screen. A trait rather than two near-identical
/// functions, because two of them is how they drifted apart in the first
/// place.
trait Attachable {
    fn id(&self) -> Uuid;
    fn attachment_lists(&mut self) -> (&mut Vec<FileAttachment>, &mut Vec<ImageAttachment>);
}

impl Attachable for DirectMessage {
    fn id(&self) -> Uuid {
        self.id
    }
    fn attachment_lists(&mut self) -> (&mut Vec<FileAttachment>, &mut Vec<ImageAttachment>) {
        (&mut self.file_attachments, &mut self.image_attachments)
    }
}

impl Attachable for GroupMessage {
    fn id(&self) -> Uuid {
        self.id
    }
    fn attachment_lists(&mut self) -> (&mut Vec<FileAttachment>, &mut Vec<ImageAttachment>) {
        (&mut self.file_attachments, &mut self.image_attachments)
    }
}

fn load_attachments<M: Attachable>(connection: &Connection, message: &mut M) -> Result<()> {
    let message_id = message.id();
    let (file_list, image_list) = message.attachment_lists();

    let mut files = connection.prepare("SELECT * FROM file_attachments WHERE message_id = ?1")?;
    *file_list = files
        .query_map(params![uuid_bytes(message_id)], |row| {
            Ok(FileAttachment {
                id: row_uuid(row, "id")?,
                file_id: row_uuid(row, "file_id")?,
                message_id: row_uuid(row, "message_id")?,
                name: row.get("name")?,
                size: row.get("size")?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut images = connection.prepare("SELECT * FROM image_attachments WHERE message_id = ?1")?;
    *image_list = images
        .query_map(params![uuid_bytes(message_id)], |row| {
            Ok(ImageAttachment {
                id: row_uuid(row, "id")?,
                file_id: row_uuid(row, "file_id")?,
                message_id: row_uuid(row, "message_id")?,
                name: row.get("name")?,
                size: row.get("size")?,
                width: row.get("width")?,
                height: row.get("height")?,
                blur_hash: row.get("blur_hash")?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::DeviceKey;

    fn store() -> Store {
        Store::in_memory().expect("in-memory database opens")
    }

    fn profile_user(name: &str) -> (User, DeviceKey) {
        let key = DeviceKey::generate();
        let user_id = Uuid::new_v4();
        let mut user = User::new(user_id, name.into());
        user.profile = true;
        user.accepted = true;
        user.public_ecdh_key = vec![7; 32];
        user.private_ecdh_key = vec![8; 32];
        user.devices.push(Device::new(
            Uuid::new_v4(),
            user_id,
            key.address(),
            1_700_000_000,
        ));
        (user, key)
    }

    #[test]
    fn schema_is_created_at_the_current_version() {
        let store = store();
        let version = store.with(schema::version).unwrap();
        assert_eq!(version, schema::SCHEMA_VERSION);
    }

    #[test]
    fn a_profile_round_trips_with_its_devices() {
        let store = store();
        let (user, key) = profile_user("Alice");
        store.save_user(&user).unwrap();

        let loaded = store.profile().unwrap().expect("profile exists");
        assert_eq!(loaded.id, user.id);
        assert_eq!(loaded.name, "Alice");
        assert!(loaded.profile);
        assert_eq!(loaded.devices.len(), 1);
        assert_eq!(loaded.devices[0].address, key.address());

        // Private key material survives, since it lives only on this device.
        assert_eq!(loaded.private_ecdh_key, vec![8; 32]);
        assert_eq!(store.my_user_id().unwrap(), user.id);
    }

    #[test]
    fn my_user_id_errors_before_a_profile_exists() {
        assert!(matches!(store().my_user_id(), Err(Error::NoProfile)));
    }

    #[test]
    fn only_one_profile_may_exist() {
        let store = store();
        let (first, _) = profile_user("Alice");
        store.save_user(&first).unwrap();

        let (second, _) = profile_user("Impostor");
        assert!(
            store.save_user(&second).is_err(),
            "a second profile row must be rejected by the unique index"
        );
    }

    #[test]
    fn devices_carry_their_introduction_signatures() {
        let store = store();
        let (user, founder_key) = profile_user("Alice");
        store.save_user(&user).unwrap();

        let new_key = DeviceKey::generate();
        let (preexisting_signs_new, new_signs_preexisting) =
            crate::device_group::create_introduction_signatures(&founder_key, &new_key);

        let mut second = Device::new(Uuid::new_v4(), user.id, new_key.address(), 1_700_000_100);
        second.signature = Some(IntroductionSignature {
            id: Uuid::new_v4(),
            device_id: second.id,
            preexisting_device: founder_key.address(),
            signature_of_new_device: preexisting_signs_new,
            signature_of_preexisting_device: new_signs_preexisting,
        });
        store.save_device(&second).unwrap();

        let devices = store.devices_for_user(user.id).unwrap();
        assert_eq!(devices.len(), 2);

        // And the reloaded group still validates, which is the point of
        // persisting the signatures at all.
        assert_eq!(crate::device_group::validate(&devices), Ok(()));
    }

    #[test]
    fn revoking_a_device_is_recorded_and_idempotent() {
        let store = store();
        let (user, key) = profile_user("Alice");
        store.save_user(&user).unwrap();

        store.revoke_device(&key.address(), 500).unwrap();
        let device = store.device_by_address(&key.address()).unwrap().unwrap();
        assert_eq!(device.revoked_at, 500);

        // A second revocation must not move the timestamp, since the original
        // time is what decides which historic frames remain valid.
        store.revoke_device(&key.address(), 900).unwrap();
        let device = store.device_by_address(&key.address()).unwrap().unwrap();
        assert_eq!(device.revoked_at, 500);
    }

    #[test]
    fn revoked_devices_are_excluded_from_the_address_list() {
        let store = store();
        let (user, key) = profile_user("Alice");
        store.save_user(&user).unwrap();

        assert_eq!(store.all_device_addresses().unwrap().len(), 1);
        store.revoke_device(&key.address(), 1).unwrap();
        assert!(store.all_device_addresses().unwrap().is_empty());
    }

    #[test]
    fn a_direct_message_round_trips_with_attachments() {
        let store = store();
        let (me, key) = profile_user("Alice");
        store.save_user(&me).unwrap();

        let them = Uuid::new_v4();
        let mut message = DirectMessage::new(me.id, them, "hello".into(), 1_700_000_000);
        message.saved_at = 1_700_000_001;
        message.file_attachments.push(FileAttachment {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            message_id: message.id,
            name: "report.pdf".into(),
            size: 4096,
        });

        let body = crate::msgpack::to_vec(&message).unwrap();
        let container = crate::signed::SignedContainer::create(&key, body);
        message.signed = SignedFrame::from_container(&container);

        store.save_direct_message(&message).unwrap();

        let loaded = store.direct_message(message.id).unwrap().expect("message exists");
        assert_eq!(loaded.text, "hello");
        assert_eq!(loaded.xor, message.xor);
        assert_eq!(loaded.file_attachments.len(), 1);
        assert_eq!(loaded.file_attachments[0].name, "report.pdf");

        // The signature material is preserved exactly, so the message can still
        // be relayed to another device and verify there.
        assert_eq!(loaded.signed, message.signed);
        assert!(loaded.signed.to_container().is_valid());
    }

    #[test]
    fn a_group_message_keeps_its_attachments_across_a_reload() {
        // They were always written; only direct messages were ever read back,
        // so a group's pictures survived in the database and disappeared from
        // the screen the moment the app was restarted.
        let store = store();
        let group = Uuid::new_v4();

        let mut message = GroupMessage::new(Uuid::new_v4(), group, "look".into(), 100);
        message.image_attachments.push(ImageAttachment {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            message_id: message.id,
            name: "photo.png".into(),
            size: 2048,
            width: 800,
            height: 600,
            blur_hash: "LEHV6nWB2yk8pyo0adR*".into(),
        });
        message.file_attachments.push(FileAttachment {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            message_id: message.id,
            name: "notes.pdf".into(),
            size: 4096,
        });

        store.save_group_message(&message).expect("saves");

        // By id...
        let one = store.group_message(message.id).expect("reads").expect("exists");
        assert_eq!(one.image_attachments.len(), 1, "the picture was lost");
        assert_eq!(one.image_attachments[0].name, "photo.png");
        assert_eq!(one.image_attachments[0].width, 800);
        assert_eq!(one.image_attachments[0].blur_hash, "LEHV6nWB2yk8pyo0adR*");
        assert_eq!(one.file_attachments.len(), 1);
        assert_eq!(one.file_attachments[0].name, "notes.pdf");

        // ...and the way the timeline actually loads a thread, which is the
        // path the reader sees.
        let thread = store.group_messages_for_thread(group, 50).expect("reads the thread");
        assert_eq!(thread.len(), 1);
        assert_eq!(thread[0].image_attachments.len(), 1, "the picture was lost on reload");
        assert_eq!(thread[0].file_attachments.len(), 1);
    }

    #[test]
    fn saving_a_message_twice_does_not_duplicate_it() {
        let store = store();
        let (me, _) = profile_user("Alice");
        store.save_user(&me).unwrap();

        let message = DirectMessage::new(me.id, Uuid::new_v4(), "hello".into(), 0);
        store.save_direct_message(&message).unwrap();
        store.save_direct_message(&message).unwrap();

        let thread = store.direct_messages_for_thread(message.xor, 100).unwrap();
        assert_eq!(thread.len(), 1);
    }

    #[test]
    fn thread_history_is_returned_oldest_first_within_the_limit() {
        let store = store();
        let (me, _) = profile_user("Alice");
        store.save_user(&me).unwrap();
        let them = Uuid::new_v4();

        for i in 0..10 {
            let mut message = DirectMessage::new(me.id, them, format!("message {i}"), 1_000 + i);
            message.saved_at = 1_000 + i;
            store.save_direct_message(&message).unwrap();
        }

        let xor = crate::xor(me.id, them);
        let recent = store.direct_messages_for_thread(xor, 3).unwrap();

        assert_eq!(recent.len(), 3);
        // The limit takes the newest three, but they read oldest to newest.
        assert_eq!(recent[0].text, "message 7");
        assert_eq!(recent[2].text, "message 9");
    }

    #[test]
    fn delivery_records_are_unique_per_device_and_frame() {
        let store = store();
        let frame_id = Uuid::new_v4();

        let record = DeliveryRecord::new("peer-one".into(), frame_id, FrameType::DirectMessage, 100);
        store.record_delivery(&record).unwrap();
        // A duplicate ack must not inflate the count.
        store
            .record_delivery(&DeliveryRecord::new(
                "peer-one".into(),
                frame_id,
                FrameType::DirectMessage,
                200,
            ))
            .unwrap();

        assert_eq!(
            store.delivery_count(frame_id, FrameType::DirectMessage).unwrap(),
            1
        );
        assert!(store
            .is_delivered_to("peer-one", frame_id, FrameType::DirectMessage)
            .unwrap());
        assert!(!store
            .is_delivered_to("peer-two", frame_id, FrameType::DirectMessage)
            .unwrap());

        // A different device is tracked separately.
        store
            .record_delivery(&DeliveryRecord::new(
                "peer-two".into(),
                frame_id,
                FrameType::DirectMessage,
                300,
            ))
            .unwrap();
        assert_eq!(
            store.delivery_count(frame_id, FrameType::DirectMessage).unwrap(),
            2
        );
    }

    #[test]
    fn reference_offers_exclude_frames_already_delivered() {
        let store = store();
        let (me, _) = profile_user("Alice");
        store.save_user(&me).unwrap();
        let them = Uuid::new_v4();

        // Written now, because an offer has an age floor: see
        // `an_undeliverable_message_is_only_offered_to_our_own_devices`.
        let delivered = DirectMessage::new(me.id, them, "already sent".into(), crate::now());
        let pending = DirectMessage::new(me.id, them, "still pending".into(), crate::now());
        store.save_direct_message(&delivered).unwrap();
        store.save_direct_message(&pending).unwrap();

        store
            .record_delivery(&DeliveryRecord::new(
                "peer".into(),
                delivered.id,
                FrameType::DirectMessage,
                0,
            ))
            .unwrap();

        let references = store.references_not_delivered_to("peer", |_, _| true).unwrap();
        let ids: Vec<Uuid> = references.iter().map(|r| r.frame_id).collect();

        assert!(ids.contains(&pending.id));
        assert!(
            !ids.contains(&delivered.id),
            "a frame the peer has acknowledged must never be offered again"
        );
    }

    #[test]
    fn reference_offers_respect_the_authorization_filter() {
        let store = store();
        let (me, _) = profile_user("Alice");
        store.save_user(&me).unwrap();

        let allowed = DirectMessage::new(me.id, Uuid::new_v4(), "allowed".into(), crate::now());
        let forbidden = DirectMessage::new(me.id, Uuid::new_v4(), "forbidden".into(), crate::now());
        store.save_direct_message(&allowed).unwrap();
        store.save_direct_message(&forbidden).unwrap();

        let references = store
            .references_not_delivered_to("peer", |id, _| id == allowed.id)
            .unwrap();

        assert_eq!(references.len(), 1);
        assert_eq!(references[0].frame_id, allowed.id);
    }

    #[test]
    fn an_undeliverable_message_is_only_offered_to_our_own_devices() {
        // A message nobody has taken in four weeks is one nobody is going to
        // take. Offering it forever grows the offer without bound; our own
        // devices are the exception, because a second device joining an old
        // profile is entitled to the whole history.
        let store = store();
        let (me, my_key) = profile_user("Alice");
        store.save_user(&me).unwrap();

        let ancient = DirectMessage::new(
            me.id,
            Uuid::new_v4(),
            "sent into the void".into(),
            crate::now() - crate::UNDELIVERABLE_AFTER_SECONDS - 1,
        );
        let recent = DirectMessage::new(me.id, Uuid::new_v4(), "fresh".into(), crate::now());
        store.save_direct_message(&ancient).unwrap();
        store.save_direct_message(&recent).unwrap();

        let ids = |peer: &str| -> Vec<Uuid> {
            store
                .references_not_delivered_to(peer, |_, _| true)
                .unwrap()
                .into_iter()
                .map(|reference| reference.frame_id)
                .collect()
        };

        let stranger = ids("some-other-device");
        assert!(stranger.contains(&recent.id));
        assert!(!stranger.contains(&ancient.id));

        // Our own device: no floor at all.
        let mine = ids(&my_key.address());
        assert!(mine.contains(&recent.id));
        assert!(mine.contains(&ancient.id));
    }

    #[test]
    fn a_message_nobody_ever_received_is_marked_undeliverable() {
        let store = store();
        let (me, _) = profile_user("Alice");
        store.save_user(&me).unwrap();

        let cutoff = crate::now() - crate::UNDELIVERABLE_AFTER_SECONDS;
        let stale = DirectMessage::new(me.id, Uuid::new_v4(), "never landed".into(), cutoff - 1);
        let delivered = DirectMessage::new(me.id, Uuid::new_v4(), "landed".into(), cutoff - 1);
        let recent = DirectMessage::new(me.id, Uuid::new_v4(), "too soon to say".into(), crate::now());
        for message in [&stale, &delivered, &recent] {
            store.save_direct_message(message).unwrap();
        }
        store
            .record_delivery(&DeliveryRecord::new(
                "peer".into(),
                delivered.id,
                FrameType::DirectMessage,
                0,
            ))
            .unwrap();

        let marked = store.mark_stale_messages_undeliverable(cutoff).unwrap();
        assert_eq!(marked, vec![stale.id]);
        assert!(store.direct_message(stale.id).unwrap().unwrap().undeliverable);
        assert!(!store.direct_message(delivered.id).unwrap().unwrap().undeliverable);
        assert!(!store.direct_message(recent.id).unwrap().unwrap().undeliverable);

        // Idempotent: a second pass has nothing left to say.
        assert!(store
            .mark_stale_messages_undeliverable(cutoff)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn has_frame_answers_a_reference_offer() {
        let store = store();
        let (me, _) = profile_user("Alice");
        store.save_user(&me).unwrap();

        let message = DirectMessage::new(me.id, Uuid::new_v4(), "held".into(), 0);
        store.save_direct_message(&message).unwrap();

        assert!(store.has_frame(message.id, FrameType::DirectMessage).unwrap());
        assert!(!store.has_frame(Uuid::new_v4(), FrameType::DirectMessage).unwrap());
        // A type we do not store frames for is never claimed.
        assert!(!store.has_frame(message.id, FrameType::KeepAlive).unwrap());
    }

    #[test]
    fn a_group_and_its_consensus_inputs_round_trip() {
        let store = store();
        let (me, key) = profile_user("Alice");
        store.save_user(&me).unwrap();

        let group = Group {
            name: "Book Club".into(),
            created_by: me.id,
            created_at: 1_700_000_000,
            users: vec![me.clone()],
            admins: me.id.to_string(),
            ..Default::default()
        };
        let creation = GroupCreation::create(&group, group.created_at).unwrap();

        let mut stored_group = group.clone();
        stored_group.id = creation.id;
        store.save_group_creation(&creation).unwrap();
        store.save_group(&stored_group).unwrap();

        let loaded_creation = store.group_creation(creation.id).unwrap().unwrap();
        assert!(loaded_creation.id_matches_data());

        let loaded_group = store.group(creation.id).unwrap().expect("group exists");
        assert_eq!(loaded_group.name, "Book Club");
        assert_eq!(loaded_group.users.len(), 1);
        assert_eq!(loaded_group.users[0].id, me.id);

        // An update with a confirmation attached comes back intact, which is
        // what consensus needs to resolve conflicts.
        let mut update = UpdateGroup::new(
            me.id,
            creation.id,
            crate::types::UpdateGroupType::ChangeName,
            b"Renamed".to_vec(),
            1_700_000_100,
        );
        update.signed.signer = key.address();
        update.signed.signature = vec![1; 64];
        update.confirmations.push(Confirmation {
            id: Uuid::new_v4(),
            update_group_id: update.id,
            destination: creation.id,
            author: me.id,
            custom_scope: Uuid::nil(),
            signing_device: key.address(),
            signature: vec![2; 64],
            timestamp: 1_700_000_100,
            saved_at: 0,
        });
        store.save_update_group(&update).unwrap();

        let updates = store.updates_for_group(creation.id).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].confirmations.len(), 1);
        assert_eq!(updates[0].confirmations[0].author, me.id);

        // And the whole thing recomputes.
        let stack =
            crate::consensus::recompute(&loaded_creation, &updates, me.id).unwrap();
        assert_eq!(stack.top().unwrap().name, "Renamed");
    }

    #[test]
    fn group_overlap_is_computed_from_shared_membership() {
        let store = store();
        let (me, _) = profile_user("Alice");
        store.save_user(&me).unwrap();

        let bob = User::new(Uuid::new_v4(), "Bob".into());
        let carol = User::new(Uuid::new_v4(), "Carol".into());
        store.save_user(&bob).unwrap();
        store.save_user(&carol).unwrap();

        let shared = Group {
            id: Uuid::new_v4(),
            name: "Shared".into(),
            users: vec![me.clone(), bob.clone()],
            ..Default::default()
        };
        store.save_group(&shared).unwrap();

        let overlap = store.users_sharing_a_group_with(me.id).unwrap();
        assert_eq!(overlap, vec![bob.id]);
        assert!(
            !overlap.contains(&carol.id),
            "Carol shares no group and must not appear in the overlap"
        );
    }

    #[test]
    fn drafts_are_one_per_thread_and_clearing_deletes_them() {
        let store = store();
        let thread = Uuid::new_v4();

        let mut draft = Draft {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            thread,
            text: "half written".into(),
            timestamp: 100,
            saved: false,
            saved_at: 0,
        };
        store.save_draft(&draft).unwrap();

        // A later draft for the same thread replaces the earlier one.
        draft.text = "rewritten".into();
        draft.timestamp = 200;
        store.save_draft(&draft).unwrap();

        let drafts = store.all_drafts().unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].text, "rewritten");

        // Emptying the box removes the draft rather than storing whitespace.
        draft.text = "   ".into();
        store.save_draft(&draft).unwrap();
        assert!(store.all_drafts().unwrap().is_empty());
    }

    #[test]
    fn a_draft_keeps_the_bytes_it_was_signed_as() {
        // Without them a stored draft cannot be relayed: `all_drafts` would be
        // synthesising an empty signature, and a device that was offline for
        // the keystroke has nothing to verify.
        let store = store();
        let (me, key) = profile_user("Alice");
        store.save_user(&me).unwrap();

        let mut draft = Draft {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            thread: Uuid::new_v4(),
            text: "half written".into(),
            timestamp: 100,
            saved: false,
            saved_at: 7,
        };
        let body = crate::msgpack::to_vec(&draft).unwrap();
        draft.signed =
            SignedFrame::from_container(&crate::signed::SignedContainer::create(&key, body));
        store.save_draft(&draft).unwrap();

        let stored = store.draft(draft.id).unwrap().expect("the draft is stored");
        assert_eq!(stored.signed.signer, key.address());
        assert!(stored.signed.to_container().is_valid());
        assert_eq!(store.frame_saved_at(draft.id, FrameType::Draft).unwrap(), Some(7));
        assert!(store.frame_payload(draft.id, FrameType::Draft).unwrap().is_some());

        // Replacing it retires the old id, because that id is what a peer
        // acknowledged: editing in place would sync the first keystroke and
        // nothing after it.
        let mut replacement = draft.clone();
        replacement.id = Uuid::new_v4();
        replacement.text = "rewritten".into();
        store.save_draft(&replacement).unwrap();

        assert!(store.draft(draft.id).unwrap().is_none());
        assert_eq!(store.all_drafts().unwrap().len(), 1);
        assert_eq!(
            store.draft_for_thread(draft.thread).unwrap().unwrap().text,
            "rewritten"
        );
    }

    #[test]
    fn profile_updates_are_stored_and_replayable_in_order() {
        let store = store();
        let (me, key) = profile_user("Alice");
        store.save_user(&me).unwrap();

        let other = Uuid::new_v4();
        let mut updates = Vec::new();
        for (target, name, timestamp) in [
            (me.id, "Alice Cooper", 200),
            (me.id, "Alice C", 100),
            (other, "Somebody Else", 150),
        ] {
            let mut update = crate::frames::update::UpdateUser::new(
                target,
                crate::frames::update::UpdateUserType::UpdateName,
                name.as_bytes().to_vec(),
                timestamp,
            );
            update.saved_at = timestamp;
            update.previous_data = b"Alice".to_vec();
            let body = crate::msgpack::to_vec(&update).unwrap();
            update.signed =
                SignedFrame::from_container(&crate::signed::SignedContainer::create(&key, body));
            store.save_update_user(&update).unwrap();
            updates.push(update);
        }

        // Oldest first, and only the one user's: the replay applies them in
        // this order, so it is the order that decides the final name.
        let mine = store.updates_for_user(me.id).unwrap();
        assert_eq!(mine.len(), 2);
        assert_eq!(mine[0].data, b"Alice C".to_vec());
        assert_eq!(mine[1].data, b"Alice Cooper".to_vec());
        assert_eq!(mine[0].previous_data, b"Alice".to_vec());
        assert!(mine[0].signed.to_container().is_valid());

        assert_eq!(store.all_update_users().unwrap().len(), 3);
        assert!(store.has_frame(updates[0].id, FrameType::UpdateUser).unwrap());
        assert!(store
            .frame_payload(updates[0].id, FrameType::UpdateUser)
            .unwrap()
            .is_some());
        assert_eq!(
            store.frame_saved_at(updates[0].id, FrameType::UpdateUser).unwrap(),
            Some(200)
        );
    }

    #[test]
    fn custom_scopes_preserve_their_address_lists() {
        let store = store();
        let scope = CustomScope::from_addresses(
            Uuid::new_v4(),
            &["device-one".to_string(), "device-two".to_string()],
            100,
        );
        store.save_custom_scope(&scope).unwrap();

        let loaded = store.custom_scope(scope.id).unwrap().expect("scope exists");
        assert_eq!(loaded.address_list(), vec!["device-one", "device-two"]);
    }

    #[test]
    fn device_owner_resolves_addresses_to_users() {
        let store = store();
        let (me, key) = profile_user("Alice");
        store.save_user(&me).unwrap();

        assert_eq!(store.device_owner(&key.address()).unwrap(), Some(me.id));
        assert_eq!(store.device_owner("unknown-address").unwrap(), None);
    }

    #[test]
    fn a_frame_payload_can_be_retrieved_for_a_catch_up() {
        let store = store();
        let (me, key) = profile_user("Alice");
        store.save_user(&me).unwrap();

        let mut message = DirectMessage::new(me.id, Uuid::new_v4(), "catch me up".into(), 5);
        let body = crate::msgpack::to_vec(&message).unwrap();
        message.signed =
            SignedFrame::from_container(&crate::signed::SignedContainer::create(&key, body));
        message.saved_at = 42;
        store.save_direct_message(&message).unwrap();

        let payload = store
            .frame_payload(message.id, FrameType::DirectMessage)
            .unwrap()
            .expect("payload is available");

        // What comes back is a valid signed container, ready to put on the wire.
        let container = crate::signed::SignedContainer::unpack(&payload).unwrap();
        assert!(container.is_valid());

        assert_eq!(
            store.frame_saved_at(message.id, FrameType::DirectMessage).unwrap(),
            Some(42)
        );
    }

    #[test]
    fn profile_settings_round_trip() {
        let store = store();
        let (me, _) = profile_user("Alice");
        store.save_user(&me).unwrap();

        let mut settings = ProfileSettings::defaults(me.id);
        settings.default_dm_retention = 12_345;
        store.save_profile_settings(&settings).unwrap();

        let loaded = store.profile_settings(me.id).unwrap().expect("settings exist");
        assert_eq!(loaded.default_dm_retention, 12_345);
        assert!(loaded.new_group_restrict_user_management);
    }

    // -----------------------------------------------------------------
    // Files and chunks
    // -----------------------------------------------------------------

    fn file_record(id: Uuid, hashes: &[&str]) -> File {
        File {
            signed: crate::frames::SignedFrame::default(),
            id,
            name: "photo.png".into(),
            file_type: 2,
            attached_to: Uuid::new_v4(),
            hash: String::new(),
            size: 3,
            chunk_size: crate::CHUNK_SIZE as i64,
            hash_list: hashes.join(","),
            encrypted_hash_list: String::new(),
            key: Vec::new(),
            nonce: Vec::new(),
            path: String::new(),
            wanted: true,
            downloaded: false,
            scope: 0,
            destination: Uuid::new_v4(),
            author: Uuid::new_v4(),
            timestamp: 0,
            saved_at: 0,
        }
    }

    #[test]
    fn a_file_is_only_assembled_once_every_chunk_is_present() {
        let store = store();
        let id = Uuid::new_v4();
        store.save_file(&file_record(id, &["aa", "bb"])).unwrap();

        store.save_chunk(id, 0, "aa", Some(b"first")).unwrap();
        store.save_chunk(id, 1, "bb", None).unwrap();

        assert_eq!(store.file_progress(id).unwrap(), 0.5);
        assert!(store.file_data(id).unwrap().is_none(), "half a file is not a file");

        store.save_chunk(id, 1, "bb", Some(b"second")).unwrap();
        assert_eq!(store.file_progress(id).unwrap(), 1.0);
        assert_eq!(store.file_data(id).unwrap().unwrap(), b"firstsecond".to_vec());
    }

    #[test]
    fn re_recording_a_chunk_never_erases_its_bytes() {
        // A file record arriving after the chunk it describes would otherwise
        // overwrite the data with nothing, and the download would restart.
        let store = store();
        let id = Uuid::new_v4();
        store.save_file(&file_record(id, &["aa"])).unwrap();

        store.save_chunk(id, 0, "aa", Some(b"payload")).unwrap();
        store.save_chunk(id, 0, "aa", None).unwrap();

        assert_eq!(store.chunk_data("aa").unwrap().unwrap(), b"payload".to_vec());
    }

    #[test]
    fn a_chunk_held_for_one_file_counts_towards_another() {
        // Chunks are content-addressed, so two files that share bytes share
        // the chunk. Counting only this file's own rows would leave a file
        // that is fully assemblable reporting as incomplete, and the interface
        // waits for 100% before asking for the bytes.
        let store = store();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();

        store.save_file(&file_record(first, &["shared"])).unwrap();
        store.save_chunk(first, 0, "shared", Some(b"same bytes")).unwrap();

        store.save_file(&file_record(second, &["shared"])).unwrap();
        store.save_chunk(second, 0, "shared", None).unwrap();

        assert_eq!(store.file_progress(second).unwrap(), 1.0);
        assert_eq!(store.file_data(second).unwrap().unwrap(), b"same bytes".to_vec());
    }

    #[test]
    fn a_file_we_have_never_heard_of_is_at_zero() {
        // Not one: an unknown file is missing everything, and reporting it
        // complete would have the interface ask for bytes that do not exist.
        assert_eq!(store().file_progress(Uuid::new_v4()).unwrap(), 0.0);
    }

    #[test]
    fn a_chunk_location_is_news_only_once() {
        let store = store();
        assert!(store.record_chunk_location("aa", "peer.onion", 10).unwrap());
        assert!(!store.record_chunk_location("aa", "peer.onion", 20).unwrap());
        assert_eq!(store.chunk_locations("aa").unwrap(), vec!["peer.onion"]);

        store.forget_chunk_location("aa", "peer.onion").unwrap();
        assert!(store.chunk_locations("aa").unwrap().is_empty());
    }
}

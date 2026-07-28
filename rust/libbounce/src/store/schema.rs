//! The SQLite schema.
//!
//! The tables mirror the GORM models in the Go implementation closely enough
//! that the two could read each other's databases, but this is a fresh schema
//! rather than a migration target: it is created from scratch and versioned by
//! [`SCHEMA_VERSION`].
//!
//! Two conventions run through it:
//!
//! - **UUIDs are stored as 16 byte blobs**, matching how they travel on the
//!   wire, so no conversion is needed on either boundary.
//! - **Timestamps are Unix seconds**, and zero means "unset" rather than the
//!   epoch. That is the protocol's convention, and using `NULL` instead would
//!   force every comparison to handle three states.

use rusqlite::{Connection, OptionalExtension};

use crate::error::{Error, Result};

/// Bumped whenever the schema changes in a way that needs a migration.
pub const SCHEMA_VERSION: i64 = 10;

/// Create every table and index, if they do not already exist.
pub fn create(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        PRAGMA synchronous = NORMAL;
        -- Freed pages keep their contents until something overwrites them, so
        -- deleting an attachment is not the same as the bytes leaving the file.
        -- This has to be set before the first table exists — on a database that
        -- already has one it is a silent no-op, and only a full VACUUM would
        -- change it — which is why it is here rather than in a migration.
        PRAGMA auto_vacuum = INCREMENTAL;

        -- People. Exactly one row has profile = 1: the owner of this device.
        CREATE TABLE IF NOT EXISTS users (
            id                            BLOB PRIMARY KEY NOT NULL,
            name                          TEXT NOT NULL DEFAULT '',
            images                        TEXT NOT NULL DEFAULT '',
            profile                       INTEGER NOT NULL DEFAULT 0,
            encrypted_devices             TEXT NOT NULL DEFAULT '',
            public_ecdsa_key              BLOB,
            private_ecdsa_key             BLOB,
            public_ecdh_key               BLOB,
            private_ecdh_key              BLOB,
            open_dm                       INTEGER NOT NULL DEFAULT 0,
            last_opened                   INTEGER NOT NULL DEFAULT 0,
            retention                     INTEGER NOT NULL DEFAULT 0,
            clear_before                  INTEGER NOT NULL DEFAULT 0,
            muted_until                   INTEGER NOT NULL DEFAULT 0,
            last_activity                 INTEGER NOT NULL DEFAULT 0,
            read_receipts_overridden      INTEGER NOT NULL DEFAULT 0,
            read_receipts_enabled         INTEGER NOT NULL DEFAULT 1,
            typing_indicators_overridden  INTEGER NOT NULL DEFAULT 0,
            typing_indicators_enabled     INTEGER NOT NULL DEFAULT 1,
            introduction_method           TEXT NOT NULL DEFAULT '',
            introduction_time             INTEGER NOT NULL DEFAULT 0,
            introduction_metadata         BLOB,
            alias                         TEXT NOT NULL DEFAULT '',
            notes                         TEXT NOT NULL DEFAULT '',
            blocked                       INTEGER NOT NULL DEFAULT 0,
            accepted                      INTEGER NOT NULL DEFAULT 0
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_users_profile
            ON users (profile) WHERE profile = 1;

        -- Devices. The address is the onion service ID and the identity.
        CREATE TABLE IF NOT EXISTS devices (
            id                 BLOB PRIMARY KEY NOT NULL,
            user_id            BLOB NOT NULL,
            name               TEXT NOT NULL DEFAULT '',
            address            TEXT NOT NULL UNIQUE,
            timestamp          INTEGER NOT NULL DEFAULT 0,
            saved_at           INTEGER NOT NULL DEFAULT 0,
            last_seen          INTEGER NOT NULL DEFAULT 0,
            revoked_at         INTEGER NOT NULL DEFAULT 0,
            ecdh_public_key    BLOB,
            ecdh_private_key   BLOB,
            -- Protocol extensions this device advertises, comma separated.
            -- Empty means legacy, which is the correct reading of a device that
            -- predates the column: those are exactly the builds that cannot
            -- handle the frames it gates.
            capabilities       TEXT NOT NULL DEFAULT '',
            FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_devices_user ON devices (user_id);

        -- The mutual signatures that admit a device to a device group.
        CREATE TABLE IF NOT EXISTS introduction_signatures (
            id                                BLOB PRIMARY KEY NOT NULL,
            device_id                         BLOB NOT NULL UNIQUE,
            preexisting_device                TEXT NOT NULL,
            signature_of_new_device           BLOB NOT NULL,
            signature_of_preexisting_device   BLOB NOT NULL,
            FOREIGN KEY (device_id) REFERENCES devices (id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS profile_settings (
            id                                  BLOB PRIMARY KEY NOT NULL,
            user_id                             BLOB NOT NULL UNIQUE,
            blocked_groups                      TEXT NOT NULL DEFAULT '',
            default_group_retention             INTEGER NOT NULL DEFAULT 0,
            default_send_read_receipts          INTEGER NOT NULL DEFAULT 1,
            default_send_typing_indicators      INTEGER NOT NULL DEFAULT 1,
            new_group_restrict_user_management  INTEGER NOT NULL DEFAULT 1,
            new_group_restrict_group_edits      INTEGER NOT NULL DEFAULT 0,
            new_group_restrict_posting          INTEGER NOT NULL DEFAULT 0,
            auto_join_groups                    INTEGER NOT NULL DEFAULT 0,
            default_dm_retention                INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
        );

        -- Current group state, recomputed from the creation record plus updates.
        CREATE TABLE IF NOT EXISTS groups (
            id                            BLOB PRIMARY KEY NOT NULL,
            name                          TEXT NOT NULL DEFAULT '',
            images                        TEXT NOT NULL DEFAULT '',
            created_by                    BLOB,
            created_at                    INTEGER NOT NULL DEFAULT 0,
            retention                     INTEGER NOT NULL DEFAULT 0,
            clear_before                  INTEGER NOT NULL DEFAULT 0,
            muted_until                   INTEGER NOT NULL DEFAULT 0,
            admins                        TEXT NOT NULL DEFAULT '',
            invites                       TEXT NOT NULL DEFAULT '',
            invited_by                    BLOB,
            invited_at                    INTEGER NOT NULL DEFAULT 0,
            accepted_at                   INTEGER NOT NULL DEFAULT 0,
            blocked_users                 TEXT NOT NULL DEFAULT '',
            restrict_user_management      INTEGER NOT NULL DEFAULT 0,
            restrict_group_edits          INTEGER NOT NULL DEFAULT 0,
            restrict_posting              INTEGER NOT NULL DEFAULT 0,
            last_activity                 INTEGER NOT NULL DEFAULT 0,
            read_receipts_overridden      INTEGER NOT NULL DEFAULT 0,
            read_receipts_enabled         INTEGER NOT NULL DEFAULT 1,
            typing_indicators_overridden  INTEGER NOT NULL DEFAULT 0,
            typing_indicators_enabled     INTEGER NOT NULL DEFAULT 1,
            last_opened                   INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS group_users (
            group_id  BLOB NOT NULL,
            user_id   BLOB NOT NULL,
            PRIMARY KEY (group_id, user_id),
            FOREIGN KEY (group_id) REFERENCES groups (id) ON DELETE CASCADE,
            FOREIGN KEY (user_id)  REFERENCES users  (id) ON DELETE CASCADE
        );

        -- The immutable founding record. `data` is what the group ID hashes.
        CREATE TABLE IF NOT EXISTS group_creations (
            id                BLOB PRIMARY KEY NOT NULL,
            timestamp         INTEGER NOT NULL DEFAULT 0,
            saved_at          INTEGER NOT NULL DEFAULT 0,
            data              BLOB NOT NULL,
            signer            TEXT NOT NULL DEFAULT '',
            original_payload  BLOB NOT NULL,
            signature         BLOB NOT NULL
        );

        CREATE TABLE IF NOT EXISTS update_groups (
            id                BLOB PRIMARY KEY NOT NULL,
            actor             BLOB NOT NULL,
            target            BLOB NOT NULL,
            timestamp         INTEGER NOT NULL DEFAULT 0,
            saved_at          INTEGER NOT NULL DEFAULT 0,
            type              INTEGER NOT NULL,
            data              BLOB NOT NULL,
            custom_scope      BLOB,
            applied           INTEGER NOT NULL DEFAULT 0,
            notified          INTEGER NOT NULL DEFAULT 0,
            seen              INTEGER NOT NULL DEFAULT 0,
            signer            TEXT NOT NULL DEFAULT '',
            original_payload  BLOB NOT NULL,
            signature         BLOB NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_update_groups_target ON update_groups (target);

        CREATE TABLE IF NOT EXISTS confirmations (
            id               BLOB PRIMARY KEY NOT NULL,
            update_group_id  BLOB NOT NULL,
            destination      BLOB,
            author           BLOB,
            custom_scope     BLOB,
            signing_device   TEXT NOT NULL DEFAULT '',
            signature        BLOB NOT NULL,
            timestamp        INTEGER NOT NULL DEFAULT 0,
            saved_at         INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_confirmations_update
            ON confirmations (update_group_id);

        CREATE TABLE IF NOT EXISTS direct_messages (
            id                BLOB PRIMARY KEY NOT NULL,
            saved_at          INTEGER NOT NULL DEFAULT 0,
            written_at        INTEGER NOT NULL DEFAULT 0,
            delete_at         INTEGER NOT NULL DEFAULT 0,
            seen              INTEGER NOT NULL DEFAULT 0,
            undeliverable     INTEGER NOT NULL DEFAULT 0,
            author            BLOB NOT NULL,
            xor               BLOB NOT NULL,
            text              TEXT NOT NULL DEFAULT '',
            signer            TEXT NOT NULL DEFAULT '',
            original_payload  BLOB NOT NULL,
            signature         BLOB NOT NULL,

            -- Deleted for everyone. A tombstone, never a removal: delete the
            -- row and `has_frame` answers false, the peer classifies the
            -- original as wanted, offers it back, and the message returns.
            deleted_at        INTEGER NOT NULL DEFAULT 0,
            -- The author, or the admin who removed it. Kept because the three
            -- sentences the interface shows cannot be told apart without it.
            deleted_by        BLOB,

            -- The reply's quote of the message it answers. `quote_expires_at`
            -- is the *original's* expiry, so the excerpt can be blanked on
            -- schedule without touching the reply.
            quote_target      BLOB,
            quote_author      BLOB,
            quote_text        TEXT NOT NULL DEFAULT '',
            quote_kind        INTEGER NOT NULL DEFAULT 0,
            quote_expires_at  INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_direct_messages_xor
            ON direct_messages (xor, written_at);

        CREATE TABLE IF NOT EXISTS group_messages (
            id                BLOB PRIMARY KEY NOT NULL,
            saved_at          INTEGER NOT NULL DEFAULT 0,
            written_at        INTEGER NOT NULL DEFAULT 0,
            delete_at         INTEGER NOT NULL DEFAULT 0,
            seen              INTEGER NOT NULL DEFAULT 0,
            undeliverable     INTEGER NOT NULL DEFAULT 0,
            author            BLOB NOT NULL,
            destination       BLOB NOT NULL,
            text              TEXT NOT NULL DEFAULT '',
            signer            TEXT NOT NULL DEFAULT '',
            original_payload  BLOB NOT NULL,
            signature         BLOB NOT NULL,

            -- Deleted for everyone. A tombstone, never a removal: delete the
            -- row and `has_frame` answers false, the peer classifies the
            -- original as wanted, offers it back, and the message returns.
            deleted_at        INTEGER NOT NULL DEFAULT 0,
            -- The author, or the admin who removed it. Kept because the three
            -- sentences the interface shows cannot be told apart without it.
            deleted_by        BLOB,

            -- The reply's quote of the message it answers. `quote_expires_at`
            -- is the *original's* expiry, so the excerpt can be blanked on
            -- schedule without touching the reply.
            quote_target      BLOB,
            quote_author      BLOB,
            quote_text        TEXT NOT NULL DEFAULT '',
            quote_kind        INTEGER NOT NULL DEFAULT 0,
            quote_expires_at  INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_group_messages_destination
            ON group_messages (destination, written_at);

        CREATE TABLE IF NOT EXISTS file_attachments (
            id          BLOB PRIMARY KEY NOT NULL,
            file_id     BLOB NOT NULL,
            message_id  BLOB NOT NULL,
            name        TEXT NOT NULL DEFAULT '',
            size        INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_file_attachments_message
            ON file_attachments (message_id);

        CREATE TABLE IF NOT EXISTS image_attachments (
            id          BLOB PRIMARY KEY NOT NULL,
            file_id     BLOB NOT NULL,
            message_id  BLOB NOT NULL,
            name        TEXT NOT NULL DEFAULT '',
            size        INTEGER NOT NULL DEFAULT 0,
            width       INTEGER NOT NULL DEFAULT 0,
            height      INTEGER NOT NULL DEFAULT 0,
            blur_hash   TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS idx_image_attachments_message
            ON image_attachments (message_id);

        CREATE TABLE IF NOT EXISTS read_receipts (
            id                BLOB PRIMARY KEY NOT NULL,
            actor             BLOB NOT NULL,
            destination       BLOB,
            scope             INTEGER NOT NULL DEFAULT 0,
            target            BLOB NOT NULL,
            target_type       INTEGER NOT NULL DEFAULT 0,
            timestamp         INTEGER NOT NULL DEFAULT 0,
            saved_at          INTEGER NOT NULL DEFAULT 0,
            signer            TEXT NOT NULL DEFAULT '',
            original_payload  BLOB NOT NULL,
            signature         BLOB NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_read_receipts_target ON read_receipts (target);

        -- Emoji reactions. One per person per message, which is the unique
        -- index below rather than a rule the engine has to remember: an upsert
        -- on (target, actor) gated on a newer timestamp gives last-write-wins
        -- for free, and makes two reactions from one person unrepresentable.
        --
        -- `delete_at` is copied from the target so a reaction cannot outlive
        -- what it is attached to, and so the existing retention sweep collects
        -- it with no new machinery.
        CREATE TABLE IF NOT EXISTS reactions (
            id                BLOB PRIMARY KEY NOT NULL,
            actor             BLOB NOT NULL,
            target            BLOB NOT NULL,
            target_type       INTEGER NOT NULL DEFAULT 0,
            emoji             TEXT NOT NULL DEFAULT '',
            timestamp         INTEGER NOT NULL DEFAULT 0,
            delete_at         INTEGER NOT NULL DEFAULT 0,
            destination       BLOB,
            scope             INTEGER NOT NULL DEFAULT 0,
            saved_at          INTEGER NOT NULL DEFAULT 0,
            signer            TEXT NOT NULL DEFAULT '',
            original_payload  BLOB NOT NULL,
            signature         BLOB NOT NULL
        );
        -- Every reaction frame is kept, including the withdrawals.
        --
        -- A unique index on (target, actor) was the obvious shape — one
        -- reaction per person is the rule, so make it unrepresentable — and it
        -- was wrong for the same reason deleting a message row is wrong. A
        -- withdrawal replacing the row destroys the id of the frame it
        -- withdraws, so `has_frame` starts answering false for it, the peer
        -- re-offers the original, and the reaction we took back comes back.
        --
        -- The rule still holds; it is applied on read instead. `resolve` folds
        -- the log to one standing reaction per person, which is what the rest
        -- of this engine does with every other frame.
        DROP INDEX IF EXISTS idx_reactions_one_per_actor;
        CREATE INDEX IF NOT EXISTS idx_reactions_target ON reactions (target);
        CREATE INDEX IF NOT EXISTS idx_reactions_actor ON reactions (target, actor);

        -- Withdrawals of sent messages.
        --
        -- Stored as frames in their own right, and not merely applied, because
        -- everything that gets a deletion to a peer who was offline runs
        -- through storage: `has_frame`, `frame_payload`,
        -- `references_not_delivered_to` and `peer_may_have` all consult a
        -- table. A delete that is not stored is a delete that is never
        -- re-offered, which is a deletion that silently did not happen.
        CREATE TABLE IF NOT EXISTS delete_messages (
            id                BLOB PRIMARY KEY NOT NULL,
            actor             BLOB NOT NULL,
            target            BLOB NOT NULL,
            target_type       INTEGER NOT NULL DEFAULT 0,
            admin_delete      INTEGER NOT NULL DEFAULT 0,
            timestamp         INTEGER NOT NULL DEFAULT 0,
            destination       BLOB,
            scope             INTEGER NOT NULL DEFAULT 0,
            saved_at          INTEGER NOT NULL DEFAULT 0,
            signer            TEXT NOT NULL DEFAULT '',
            original_payload  BLOB NOT NULL,
            signature         BLOB NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_delete_messages_target
            ON delete_messages (target);

        -- Proof that a specific frame reached a specific device. This is the
        -- only source of delivery knowledge; nothing is ever inferred.
        CREATE TABLE IF NOT EXISTS delivery_records (
            id           BLOB PRIMARY KEY NOT NULL,
            created_at   INTEGER NOT NULL DEFAULT 0,
            destination  TEXT NOT NULL,
            frame_id     BLOB NOT NULL,
            frame_type   INTEGER NOT NULL,
            UNIQUE (destination, frame_id, frame_type)
        );
        CREATE INDEX IF NOT EXISTS idx_delivery_records_frame
            ON delivery_records (frame_id, frame_type);

        CREATE TABLE IF NOT EXISTS custom_scopes (
            id          BLOB PRIMARY KEY NOT NULL,
            created_at  INTEGER NOT NULL DEFAULT 0,
            addresses   TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS files (
            id                   BLOB PRIMARY KEY NOT NULL,
            name                 TEXT NOT NULL DEFAULT '',
            type                 INTEGER NOT NULL DEFAULT 0,
            attached_to          BLOB,
            hash                 TEXT NOT NULL DEFAULT '',
            size                 INTEGER NOT NULL DEFAULT 0,
            chunk_size           INTEGER NOT NULL DEFAULT 0,
            hash_list            TEXT NOT NULL DEFAULT '',
            encrypted_hash_list  TEXT NOT NULL DEFAULT '',
            key                  BLOB,
            nonce                BLOB,
            path                 TEXT NOT NULL DEFAULT '',
            wanted               INTEGER NOT NULL DEFAULT 0,
            downloaded           INTEGER NOT NULL DEFAULT 0,
            scope                INTEGER NOT NULL DEFAULT 0,
            destination          BLOB,
            author               BLOB,
            timestamp            INTEGER NOT NULL DEFAULT 0,
            saved_at             INTEGER NOT NULL DEFAULT 0,
            signer               TEXT NOT NULL DEFAULT '',
            original_payload     BLOB NOT NULL DEFAULT x'',
            signature            BLOB NOT NULL DEFAULT x''
        );

        -- One piece of a file. `data` is null until the chunk arrives.
        --
        -- Chunk bytes live in the database rather than in a blobs directory:
        -- a chunk is capped at 1 MiB and an embedded file at 20 MiB, so the
        -- rows stay small, and keeping them here means a chunk cannot be
        -- orphaned from its metadata by a partial write.
        CREATE TABLE IF NOT EXISTS chunks (
            id              BLOB PRIMARY KEY NOT NULL,
            file_id         BLOB NOT NULL,
            hash            TEXT NOT NULL,
            encrypted_hash  TEXT NOT NULL DEFAULT '',
            idx             INTEGER NOT NULL DEFAULT 0,
            downloaded      INTEGER NOT NULL DEFAULT 0,
            data            BLOB,
            FOREIGN KEY (file_id) REFERENCES files (id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_chunks_hash ON chunks (hash);

        -- Chunks are content-addressed, so lookups by hash cross files: a
        -- chunk fetched for one file serves every other file containing the
        -- same bytes, and progress is counted that way.

        -- Which devices have said they hold a chunk, so a download has
        -- somewhere to ask. Content-addressed, so one row serves every file
        -- that happens to contain the same bytes.
        CREATE TABLE IF NOT EXISTS chunk_locations (
            hash        TEXT NOT NULL,
            address     TEXT NOT NULL,
            offered_at  INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (hash, address)
        );

        -- The offers themselves, kept as stored frames.
        --
        -- This port originally treated them as ephemeral and bounded their
        -- gossip by novelty instead. That is the reason a file record arriving
        -- through catch-up named no holder at all: offers are the only thing
        -- that says who has the bytes, and a device that joined later had
        -- never heard one. Go stores them (`chat/file.go:344`) and replays
        -- them through the reference flow like anything else, which is what
        -- makes a chunk findable long after it was first announced.
        --
        -- `last_request_time` is carried because Go carries it; both sides
        -- keep it local (`msgpack:"-"`).
        CREATE TABLE IF NOT EXISTS chunk_offers (
            id                 BLOB PRIMARY KEY NOT NULL,
            scope              INTEGER NOT NULL DEFAULT 0,
            destination        BLOB,
            author             BLOB,
            file_id            BLOB NOT NULL,
            hash               TEXT NOT NULL,
            location           TEXT NOT NULL,
            timestamp          INTEGER NOT NULL DEFAULT 0,
            saved_at           INTEGER NOT NULL DEFAULT 0,
            last_request_time  INTEGER NOT NULL DEFAULT 0,
            signer             TEXT NOT NULL DEFAULT '',
            original_payload   BLOB NOT NULL DEFAULT x'',
            signature          BLOB NOT NULL DEFAULT x''
        );
        CREATE INDEX IF NOT EXISTS idx_chunk_offers_hash ON chunk_offers (hash);
        CREATE INDEX IF NOT EXISTS idx_chunk_offers_file ON chunk_offers (file_id);

        -- A change to a direct message thread.
        --
        -- Only the shared kinds are kept: retention and history clearing are
        -- properties of the conversation and both sides need them, and both
        -- get a row in the timeline saying who changed what. Mute state,
        -- aliases and notes are one side's private view, are applied straight
        -- to the `users` row, and are never stored as frames.
        CREATE TABLE IF NOT EXISTS update_dms (
            id                BLOB PRIMARY KEY NOT NULL,
            actor             BLOB,
            target            BLOB,
            type              INTEGER NOT NULL DEFAULT 0,
            data              BLOB,
            timestamp         INTEGER NOT NULL DEFAULT 0,
            saved_at          INTEGER NOT NULL DEFAULT 0,
            seen              INTEGER NOT NULL DEFAULT 0,
            signer            TEXT NOT NULL DEFAULT '',
            original_payload  BLOB NOT NULL DEFAULT x'',
            signature         BLOB NOT NULL DEFAULT x''
        );

        CREATE INDEX IF NOT EXISTS idx_update_dms_target ON update_dms (target);

        -- A change to a user's profile: their name, or one of their images.
        --
        -- Kept rather than folded straight into the `users` row for the same
        -- reason group updates are: the row is *derived* by replaying every
        -- update for that user, which is what makes the result independent of
        -- the order they arrived in, and what lets a device that was offline be
        -- caught up with the frames rather than with a summary.
        CREATE TABLE IF NOT EXISTS update_users (
            id                BLOB PRIMARY KEY NOT NULL,
            target            BLOB NOT NULL,
            type              INTEGER NOT NULL DEFAULT 0,
            data              BLOB,
            -- The value being replaced, kept locally so the timeline can say
            -- what a name changed *from*. Never travels on the wire.
            previous_data     BLOB,
            timestamp         INTEGER NOT NULL DEFAULT 0,
            saved_at          INTEGER NOT NULL DEFAULT 0,
            seen              INTEGER NOT NULL DEFAULT 0,
            signer            TEXT NOT NULL DEFAULT '',
            original_payload  BLOB NOT NULL DEFAULT x'',
            signature         BLOB NOT NULL DEFAULT x''
        );

        CREATE INDEX IF NOT EXISTS idx_update_users_target ON update_users (target);

        -- An unsent message body, one per thread.
        --
        -- The signature columns are what make a draft relayable: a device that
        -- was offline is caught up with the frame itself, so the bytes it was
        -- signed as have to survive the round trip through the database.
        CREATE TABLE IF NOT EXISTS drafts (
            id                BLOB PRIMARY KEY NOT NULL,
            thread            BLOB NOT NULL UNIQUE,
            text              TEXT NOT NULL DEFAULT '',
            timestamp         INTEGER NOT NULL DEFAULT 0,
            saved_at          INTEGER NOT NULL DEFAULT 0,
            signer            TEXT NOT NULL DEFAULT '',
            original_payload  BLOB NOT NULL DEFAULT x'',
            signature         BLOB NOT NULL DEFAULT x''
        );

        -- Short-lived secrets for adding a contact or pairing a device.
        -- A change to one device in a device group: a rename, a public key,
        -- or a revocation.
        --
        -- Kept as frames rather than merely applied, because a revocation is
        -- broadcast globally and every contact that was offline has to be able
        -- to catch up on it. A contact who never learns of a revocation keeps
        -- trusting the revoked device, which is the whole thing revoking is
        -- meant to prevent.
        CREATE TABLE IF NOT EXISTS update_devices (
            id                BLOB PRIMARY KEY NOT NULL,
            target            BLOB,
            type              INTEGER NOT NULL DEFAULT 0,
            data              BLOB,
            timestamp         INTEGER NOT NULL DEFAULT 0,
            saved_at          INTEGER NOT NULL DEFAULT 0,
            author            BLOB,
            signer            TEXT NOT NULL DEFAULT '',
            original_payload  BLOB NOT NULL DEFAULT x'',
            signature         BLOB NOT NULL DEFAULT x''
        );

        CREATE INDEX IF NOT EXISTS idx_update_devices_target ON update_devices (target);

        -- A change to the profile-wide settings.
        --
        -- Sync-scoped, so these never reach a contact; kept as frames so a
        -- device that was offline for a change replays it rather than staying
        -- on a stale preference.
        CREATE TABLE IF NOT EXISTS update_settings (
            id                BLOB PRIMARY KEY NOT NULL,
            type              INTEGER NOT NULL DEFAULT 0,
            data              BLOB,
            timestamp         INTEGER NOT NULL DEFAULT 0,
            saved_at          INTEGER NOT NULL DEFAULT 0,
            author            BLOB,
            signer            TEXT NOT NULL DEFAULT '',
            original_payload  BLOB NOT NULL DEFAULT x'',
            signature         BLOB NOT NULL DEFAULT x''
        );

        -- Data repairs that have already been applied.
        --
        -- The schema converges automatically, but a repair to *rows* has no
        -- structure to compare against — running one twice would undo a
        -- choice the user made in between. This records which have run.
        CREATE TABLE IF NOT EXISTS backfills (
            name        TEXT PRIMARY KEY NOT NULL,
            applied_at  INTEGER NOT NULL DEFAULT 0
        );

        -- The secret behind an "add me as a contact" code.
        CREATE TABLE IF NOT EXISTS pairing_offers (
            id         BLOB PRIMARY KEY NOT NULL,
            timestamp  INTEGER NOT NULL DEFAULT 0,
            secret     TEXT NOT NULL UNIQUE
        );

        -- The secret behind a "link this device" code.
        --
        -- Deliberately a separate table from `pairing_offers`, as it is in the
        -- Go implementation. The two codes look identical and grant wildly
        -- different things: one makes somebody a contact, the other hands over
        -- the profile's private keys. Sharing a table would mean a code shown
        -- to a stranger so they could message you could instead be redeemed to
        -- join your device group.
        CREATE TABLE IF NOT EXISTS sync_device_offers (
            id         BLOB PRIMARY KEY NOT NULL,
            timestamp  INTEGER NOT NULL DEFAULT 0,
            secret     TEXT NOT NULL UNIQUE
        );

        -- Secrets that have been used. A secret is burned on first use whether
        -- or not the request succeeded, so a captured one cannot be replayed.
        CREATE TABLE IF NOT EXISTS burned_secrets (
            secret     TEXT PRIMARY KEY NOT NULL,
            burned_at  INTEGER NOT NULL DEFAULT 0
        );

        -- The self-verifying record of two users adding each other. Retained so
        -- it can be replayed to a device that was offline for the exchange.
        CREATE TABLE IF NOT EXISTS add_users (
            id                   BLOB PRIMARY KEY NOT NULL,
            xor                  BLOB NOT NULL,
            timestamp            INTEGER NOT NULL DEFAULT 0,
            saved_at             INTEGER NOT NULL DEFAULT 0,
            offer_user           BLOB NOT NULL,
            requester_user       BLOB NOT NULL,
            offer_device         TEXT NOT NULL,
            requester_device     TEXT NOT NULL,
            offer_signature      BLOB NOT NULL,
            requester_signature  BLOB NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_add_users_xor ON add_users (xor);
        "#,
    )?;

    Ok(())
}

/// The schema version recorded in the database file.
pub fn version(connection: &Connection) -> Result<i64> {
    Ok(connection.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

// ---------------------------------------------------------------------------
// Migration
// ---------------------------------------------------------------------------

/// Bring a database up to the current schema, whatever state it is in.
///
/// ## Why this does not trust the version number
///
/// [`create`] is written entirely in `CREATE ... IF NOT EXISTS`, which is a
/// no-op against a table that already exists. That is right for tables and
/// indexes and silently wrong for *columns*: a database from an older build
/// keeps its old columns, and stamping the version afterwards told it the lie
/// that it was current. One on this machine ended up recorded as version 4
/// while structurally being version 2, missing `chunks.data` — so every
/// attachment failed with "no column data in chunks table", and because the
/// version said 4 nothing would ever have repaired it.
///
/// So the version is not the source of truth here; the database itself is.
/// Every open compares the live schema against a reference built from
/// [`create`] in memory and applies whatever is missing. That costs a handful
/// of `PRAGMA table_info` reads, it is idempotent, and it repairs a database
/// that lies about its version.
///
/// It also means an added table, index or column needs no migration code at
/// all — changing [`create`] is enough, because the reference is derived from
/// it. Only changes that SQLite's `ALTER TABLE` cannot express, such as adding
/// a `UNIQUE` constraint, need an entry in [`rebuild_tables`].
pub fn ensure(connection: &Connection) -> Result<()> {
    // Constraint changes rebuild a table, which requires foreign keys to be
    // off — and that pragma is a no-op inside a transaction, so it has to be
    // set before any of this starts.
    connection.pragma_update(None, "foreign_keys", "OFF")?;

    let result = converge(connection);

    connection.pragma_update(None, "foreign_keys", "ON")?;
    result?;

    // Only now, and only on success. A database that failed to migrate must
    // come back as stale next time rather than as current-but-broken.
    connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

fn converge(connection: &Connection) -> Result<()> {
    // Whatever is wholly absent — new tables, new indexes — comes from the
    // ordinary DDL. Only after that can columns be compared, because a table
    // added on this pass has all of its columns already.
    create(connection)?;

    let reference = Connection::open_in_memory()?;
    create(&reference)?;

    for table in table_names(&reference)? {
        rebuild_if_constraints_changed(connection, &reference, &table)?;
        add_missing_columns(connection, &reference, &table)?;
    }

    // Indexes come last: a UNIQUE one cannot be created while rows still
    // violate it, and the deduplication that makes room for it needs the
    // columns to exist first.
    create_deferred_indexes(connection)?;

    backfill(connection)?;
    Ok(())
}

/// Repairs to *data* that a schema change makes necessary.
///
/// Distinct from the convergence above, which only ever adds structure. Each
/// one here has to be safe to run repeatedly, because `ensure` runs on every
/// open and there is no per-step bookkeeping.
fn backfill(connection: &Connection) -> Result<()> {
    // Runs once. A repair to rows has no structure to compare against, so a
    // second pass would undo whatever the user did in between — here, closing
    // a conversation the first pass had opened.
    let done: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM backfills WHERE name = 'open_dm_from_history'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if done > 0 {
        return Ok(());
    }

    // `open_dm` decides whether a conversation appears on the thread list, and
    // for the whole of this port's life nothing ever wrote it — so every
    // contact adopted before now carries the `false` the column defaults to.
    // The renderer papered over that by showing anyone with message history
    // regardless, which is what made closing a conversation a no-op.
    //
    // Now that the flag is honoured, that default would empty an existing
    // user's sidebar on the first launch after upgrading. So anyone actually
    // corresponded with is opened, once.
    //
    // A thread is keyed by the XOR of the two participants rather than by
    // naming them, and SQLite has no XOR over blobs, so the pairing is
    // computed here instead of in the statement.
    let profile: Option<Vec<u8>> = connection
        .query_row("SELECT id FROM users WHERE profile = 1", [], |row| row.get(0))
        .optional()?;
    let Some(profile) = profile.and_then(|bytes| uuid::Uuid::from_slice(&bytes).ok()) else {
        // No profile yet: a fresh database has nothing to repair, and marking
        // it done costs nothing because there will never be anything either.
        connection.execute(
            "INSERT INTO backfills (name, applied_at) VALUES ('open_dm_from_history', ?1)",
            rusqlite::params![crate::now()],
        )?;
        return Ok(());
    };

    let candidates: Vec<Vec<u8>> = {
        let mut statement = connection
            .prepare("SELECT id FROM users WHERE profile = 0 AND blocked = 0 AND open_dm = 0")?;
        let rows = statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };

    for bytes in candidates {
        let Ok(user_id) = uuid::Uuid::from_slice(&bytes) else {
            continue;
        };
        let thread = crate::xor(profile, user_id);

        let messages: i64 = connection.query_row(
            "SELECT COUNT(*) FROM direct_messages WHERE xor = ?1 LIMIT 1",
            rusqlite::params![thread.as_bytes().to_vec()],
            |row| row.get(0),
        )?;

        if messages > 0 {
            connection.execute(
                "UPDATE users SET open_dm = 1 WHERE id = ?1",
                rusqlite::params![bytes],
            )?;
        }
    }

    connection.execute(
        "INSERT INTO backfills (name, applied_at) VALUES ('open_dm_from_history', ?1)",
        rusqlite::params![crate::now()],
    )?;
    Ok(())
}

/// Every table in a database, excluding SQLite's own.
fn table_names(connection: &Connection) -> Result<Vec<String>> {
    let mut statement = connection.prepare(
        "SELECT name FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
         ORDER BY name",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// One column, as `PRAGMA table_info` describes it.
struct Column {
    name: String,
    kind: String,
    not_null: bool,
    default: Option<String>,
}

/// A table's columns, in declaration order.
fn columns(connection: &Connection, table: &str) -> Result<Vec<Column>> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info(\"{table}\")"))?;
    let rows = statement.query_map([], |row| {
        Ok(Column {
            name: row.get(1)?,
            kind: row.get(2)?,
            not_null: row.get::<_, i64>(3)? != 0,
            default: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Add any column the reference has and the live database does not.
///
/// `ALTER TABLE ADD COLUMN` cannot add a `NOT NULL` column without a default,
/// because there is no value to give the existing rows. Every column in this
/// schema either is nullable or carries a default, so the constraint is
/// preserved rather than quietly dropped — but if that ever stops being true,
/// this refuses instead of producing a table that differs from a fresh one.
fn add_missing_columns(connection: &Connection, reference: &Connection, table: &str) -> Result<()> {
    let live: std::collections::HashSet<String> = columns(connection, table)?
        .into_iter()
        .map(|column| column.name)
        .collect();

    for Column {
        name,
        kind,
        not_null,
        default,
    } in columns(reference, table)?
    {
        if live.contains(&name) {
            continue;
        }

        let mut declaration = format!("\"{name}\" {kind}");
        match (&default, not_null) {
            (Some(value), _) => {
                declaration.push_str(&format!(" DEFAULT {value}"));
                if not_null {
                    declaration.push_str(" NOT NULL");
                }
            }
            (None, true) => {
                return Err(Error::Migration(format!(
                    "cannot add {table}.{name}: it is NOT NULL with no default, so existing \
                     rows have no value to take. Give it a default in the schema."
                )))
            }
            (None, false) => {}
        }

        tracing::info!(table, column = %name, "adding a column an older database lacks");
        connection.execute_batch(&format!("ALTER TABLE \"{table}\" ADD COLUMN {declaration};"))?;
    }
    Ok(())
}

/// Tables whose *constraints* changed, which `ALTER TABLE` cannot express.
///
/// Each entry names the table and the SQL that resolves rows the new
/// constraint would reject. Adding one is the only manual step a schema change
/// ever needs here.
fn rebuild_tables() -> &'static [(&'static str, &'static str)] {
    &[(
        // `secret` gained UNIQUE. A pairing secret is single-use and expires in
        // five minutes, so duplicates are stale by definition and dropping all
        // but the newest costs nothing a user would notice.
        "pairing_offers",
        "DELETE FROM pairing_offers WHERE rowid NOT IN (
             SELECT MAX(rowid) FROM pairing_offers GROUP BY secret
         );",
    )]
}

/// Rebuild a table whose declared constraints no longer match the reference.
///
/// This is SQLite's documented twelve-step procedure: build the new shape
/// alongside, copy what both have in common, swap, and check foreign keys. It
/// runs in one transaction, so an interrupted migration leaves the old table
/// untouched rather than a half-copied one.
fn rebuild_if_constraints_changed(
    connection: &Connection,
    reference: &Connection,
    table: &str,
) -> Result<()> {
    let Some((_, dedupe)) = rebuild_tables().iter().find(|(name, _)| *name == table) else {
        return Ok(());
    };

    let live_ddl = table_ddl(connection, table)?;
    let target_ddl = table_ddl(reference, table)?;
    if normalise(&live_ddl) == normalise(&target_ddl) {
        return Ok(());
    }

    tracing::info!(table, "rebuilding a table whose constraints changed");

    // Copy only the columns both shapes share; anything new is filled in by
    // `add_missing_columns` on the pass that follows.
    let target_columns: Vec<String> = columns(reference, table)?
        .into_iter()
        .map(|column| column.name)
        .collect();
    let live_columns: std::collections::HashSet<String> = columns(connection, table)?
        .into_iter()
        .map(|column| column.name)
        .collect();
    let shared: Vec<String> = target_columns
        .into_iter()
        .filter(|name| live_columns.contains(name))
        .map(|name| format!("\"{name}\""))
        .collect();
    let shared = shared.join(", ");

    let scratch = format!("{table}__migrating");
    let creation = target_ddl.replacen(table, &scratch, 1);

    connection.execute_batch(&format!(
        "BEGIN;
         {dedupe}
         DROP TABLE IF EXISTS \"{scratch}\";
         {creation};
         INSERT INTO \"{scratch}\" ({shared}) SELECT {shared} FROM \"{table}\";
         DROP TABLE \"{table}\";
         ALTER TABLE \"{scratch}\" RENAME TO \"{table}\";
         COMMIT;"
    ))?;

    Ok(())
}

/// The `CREATE TABLE` statement a table was declared with.
fn table_ddl(connection: &Connection, table: &str) -> Result<String> {
    Ok(connection.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
        rusqlite::params![table],
        |row| row.get::<_, String>(0),
    )?)
}

/// Collapse whitespace so two equivalent declarations compare equal.
fn normalise(ddl: &str) -> String {
    ddl.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Create the indexes that could not be created until the data allowed it.
///
/// `create` already issues these, but a `CREATE UNIQUE INDEX` against rows
/// that violate it fails — and `execute_batch` abandons the rest of the batch
/// when it does. Deduplicating first and retrying here is what lets an old
/// database gain the constraint instead of being stuck behind it.
fn create_deferred_indexes(connection: &Connection) -> Result<()> {
    // Two chunk rows for the same position in the same file are the same
    // chunk; keep whichever one actually holds bytes.
    connection.execute_batch(
        "DELETE FROM chunks WHERE rowid NOT IN (
             SELECT rowid FROM (
                 SELECT rowid, ROW_NUMBER() OVER (
                     PARTITION BY file_id, idx ORDER BY (data IS NOT NULL) DESC, rowid
                 ) AS rank FROM chunks
             ) WHERE rank = 1
         );
         CREATE UNIQUE INDEX IF NOT EXISTS idx_chunks_file_idx ON chunks (file_id, idx);",
    )?;
    Ok(())
}

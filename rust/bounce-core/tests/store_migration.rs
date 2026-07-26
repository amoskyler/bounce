//! Migrating a database written by an older build.
//!
//! The failure these pin down was not a missing migration step but a missing
//! migration *mechanism*: `CREATE TABLE IF NOT EXISTS` does nothing to a table
//! that already exists, so new columns never appeared, and the version was
//! stamped as current regardless. A database ended up recorded as version 4
//! while structurally being version 2, which meant nothing would ever repair
//! it.

use std::collections::BTreeSet;

use bounce_core::store::{schema, Store};
use rusqlite::Connection;

/// The schema as it stood at version 2, for the tables these tests touch.
///
/// Copied verbatim from a version 2 database found on disk rather than
/// paraphrased, because the whole point is to compare against what an older
/// build actually wrote - a hand-typed approximation would pass while the real
/// thing failed. `chunks` has no `data` column and `pairing_offers.secret` has
/// no UNIQUE, which are the two changes that need real migration work.
const V2_SUBSET: &str = r#"
    CREATE TABLE chunks (
                id              BLOB PRIMARY KEY NOT NULL,
                file_id         BLOB NOT NULL,
                hash            TEXT NOT NULL,
                encrypted_hash  TEXT NOT NULL DEFAULT '',
                idx             INTEGER NOT NULL DEFAULT 0,
                downloaded      INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (file_id) REFERENCES files (id) ON DELETE CASCADE
            );

    CREATE TABLE direct_messages (
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
                signature         BLOB NOT NULL
            );

    CREATE TABLE files (
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

    CREATE TABLE pairing_offers (
                id         BLOB PRIMARY KEY NOT NULL,
                timestamp  INTEGER NOT NULL DEFAULT 0,
                secret     TEXT NOT NULL
            );

    CREATE INDEX IF NOT EXISTS idx_chunks_hash ON chunks (hash);
"#;

/// Build a database shaped like an older build's, and stamp it with `version`.
fn legacy_database(path: &std::path::Path, version: i64) -> Connection {
    let connection = Connection::open(path).expect("opens");
    connection.execute_batch(V2_SUBSET).expect("creates the old shape");
    connection
        .pragma_update(None, "user_version", version)
        .expect("stamps");
    connection
}

/// Every table, column, and index, so two databases can be compared exactly.
fn fingerprint(connection: &Connection) -> BTreeSet<String> {
    let mut out = BTreeSet::new();

    let mut tables = connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
        .unwrap();
    let names: Vec<String> = tables
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    for table in names {
        out.insert(format!("table:{table}"));
        let mut info = connection
            .prepare(&format!("PRAGMA table_info(\"{table}\")"))
            .unwrap();
        let cols = info
            .query_map([], |row| {
                Ok(format!(
                    "column:{table}.{}:{}:notnull={}:default={:?}",
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })
            .unwrap();
        for col in cols {
            out.insert(col.unwrap());
        }
    }

    let mut indexes = connection
        .prepare("SELECT name, sql FROM sqlite_master WHERE type='index' AND name NOT LIKE 'sqlite_%'")
        .unwrap();
    let rows = indexes
        .query_map([], |row| {
            Ok(format!(
                "index:{}:{}",
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?
                    .unwrap_or_default()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            ))
        })
        .unwrap();
    for index in rows {
        out.insert(index.unwrap());
    }

    out
}

fn temp_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bounce-migration-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("bounce.db")
}

#[test]
fn a_database_lying_about_its_version_is_still_repaired() {
    // The one that actually bit: stamped current, structurally two versions
    // old. A migration keyed on the version number alone would skip it and it
    // would stay broken forever.
    let path = temp_path("poisoned");
    let _ = std::fs::remove_file(&path);

    let legacy = legacy_database(&path, schema::SCHEMA_VERSION);
    legacy
        .execute(
            "INSERT INTO direct_messages
                 (id, author, xor, text, original_payload, signature)
             VALUES (
                 x'0102030405060708090a0b0c0d0e0f10',
                 x'11111111111111111111111111111111',
                 x'22222222222222222222222222222222',
                 'keep me', x'', x''
             )",
            [],
        )
        .unwrap();
    drop(legacy);

    let store = Store::open(&path).expect("opens and migrates");
    drop(store);

    let migrated = Connection::open(&path).unwrap();
    let has_data: i64 = migrated
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('chunks') WHERE name='data'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(has_data, 1, "chunks.data must have been added");

    // And the message survived.
    let text: String = migrated
        .query_row("SELECT text FROM direct_messages", [], |row| row.get(0))
        .unwrap();
    assert_eq!(text, "keep me");
}

#[test]
fn a_migrated_database_matches_a_freshly_created_one() {
    // The property that makes the whole approach trustworthy: after migrating,
    // there is no way to tell an upgraded database from a new one.
    let legacy_path = temp_path("compare-legacy");
    let fresh_path = temp_path("compare-fresh");
    let _ = std::fs::remove_file(&legacy_path);
    let _ = std::fs::remove_file(&fresh_path);

    drop(legacy_database(&legacy_path, 2));

    drop(Store::open(&legacy_path).expect("migrates"));
    drop(Store::open(&fresh_path).expect("creates"));

    let migrated = fingerprint(&Connection::open(&legacy_path).unwrap());
    let fresh = fingerprint(&Connection::open(&fresh_path).unwrap());

    let only_migrated: Vec<_> = migrated.difference(&fresh).collect();
    let only_fresh: Vec<_> = fresh.difference(&migrated).collect();

    assert!(
        only_migrated.is_empty() && only_fresh.is_empty(),
        "migrated and fresh differ.\nonly in migrated: {only_migrated:#?}\nonly in fresh: {only_fresh:#?}"
    );
}

#[test]
fn migrating_twice_changes_nothing() {
    let path = temp_path("idempotent");
    let _ = std::fs::remove_file(&path);
    drop(legacy_database(&path, 2));

    drop(Store::open(&path).expect("first"));
    let once = fingerprint(&Connection::open(&path).unwrap());

    drop(Store::open(&path).expect("second"));
    let twice = fingerprint(&Connection::open(&path).unwrap());

    assert_eq!(once, twice);
}

#[test]
fn legacy_rows_that_violate_a_new_unique_constraint_are_resolved() {
    // Both new UNIQUE constraints can be violated by rows an older build was
    // happy to store. Creating the constraint would fail, and with it the rest
    // of the batch, leaving the database half-migrated.
    let path = temp_path("duplicates");
    let _ = std::fs::remove_file(&path);

    let legacy = legacy_database(&path, 2);
    legacy
        .execute_batch(
            "INSERT INTO pairing_offers (id, timestamp, secret) VALUES
                 (x'01000000000000000000000000000000', 1, 'same-secret'),
                 (x'02000000000000000000000000000000', 2, 'same-secret');
             INSERT INTO files (id) VALUES (x'0f000000000000000000000000000000');
             INSERT INTO chunks (id, file_id, hash, idx) VALUES
                 (x'03000000000000000000000000000000', x'0f000000000000000000000000000000', 'aa', 0),
                 (x'04000000000000000000000000000000', x'0f000000000000000000000000000000', 'aa', 0);",
        )
        .unwrap();
    drop(legacy);

    drop(Store::open(&path).expect("migrates past the duplicates"));

    let migrated = Connection::open(&path).unwrap();
    let offers: i64 = migrated
        .query_row("SELECT COUNT(*) FROM pairing_offers", [], |row| row.get(0))
        .unwrap();
    let chunks: i64 = migrated
        .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
        .unwrap();
    assert_eq!(offers, 1, "the newer pairing offer is kept");
    assert_eq!(chunks, 1, "one chunk per (file, index)");

    // And the constraints really are in force now.
    assert!(
        migrated
            .execute(
                "INSERT INTO pairing_offers (id, timestamp, secret)
                 VALUES (x'05000000000000000000000000000000', 3, 'same-secret')",
                [],
            )
            .is_err(),
        "secret must be UNIQUE after migration"
    );
}

#[test]
fn a_fresh_database_records_the_current_version() {
    let path = temp_path("version");
    let _ = std::fs::remove_file(&path);
    drop(Store::open(&path).expect("creates"));

    let connection = Connection::open(&path).unwrap();
    assert_eq!(schema::version(&connection).unwrap(), schema::SCHEMA_VERSION);
}

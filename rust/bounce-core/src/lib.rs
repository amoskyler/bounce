//! # bounce-core
//!
//! The Bounce protocol and cryptography core, ported from the reference Go
//! implementation in `chat/`.
//!
//! Bounce is a distributed group chat protocol in which every instance is a Tor
//! hidden service and every connection between devices runs over the mixnet.
//! There are no servers: frames are gossiped between devices that are in scope
//! for them, and devices that were offline catch up via the reference flow.
//!
//! This crate is deliberately transport- and UI-agnostic. It speaks the same
//! wire format as the Go implementation (see [`wire`] and [`msgpack`]) so a Rust
//! device and a Go device can participate in the same device group.
//!
//! ## Layout
//!
//! - [`wire`] — the type-length-value framing used on every socket
//! - [`msgpack`] — Go-compatible MessagePack encoding for frame payloads
//! - [`crypto`] — Ed25519 signing, X25519 ECDH, AES-256-GCM, BLAKE3
//! - [`onion`] — v3 onion address encoding, which doubles as device identity
//! - [`signed`] — the signed container that wraps authenticated frames
//! - [`frames`] — the frame structs themselves
//! - [`device_group`] — mutual-signature device group validation
//! - [`consensus`] — the group consensus canonical stack
//! - [`store`] — SQLite persistence
//! - [`net`] — the `Network` trait plus TCP and Tor transports
//! - [`engine`] — peer management, broadcast, frame dispatch, reference flow

pub mod consensus;
pub mod crypto;
pub mod device_group;
pub mod engine;
pub mod error;
pub mod frames;
pub mod msgpack;
pub mod net;
pub mod onion;
pub mod scope;
pub mod signed;
pub mod store;
pub mod types;
pub mod wire;

pub use error::{Error, Result};
pub use types::{FrameType, Scope};

use uuid::Uuid;

/// Maximum length of a user, group, or device name, in Unicode scalar values.
pub const MAXIMUM_NAME_LENGTH: usize = 128;

/// Maximum length of a chat message, in Unicode scalar values.
pub const MAXIMUM_MESSAGE_CHARACTERS: usize = 15_000;

/// Sentinel retention value meaning "muted until explicitly unmuted".
pub const MUTED_FOREVER: i64 = -1;

/// Files at or below this size are embedded in the blobs directory and fetched
/// automatically by clients. Larger files are seeded in place from disk.
pub const EMBEDDED_FILE_LIMIT: i64 = 20 * 1024 * 1024;

/// Maximum size of a single file chunk.
pub const CHUNK_SIZE: usize = 1024 * 1024;

/// A frame that has gone this long without a single successful delivery is
/// marked undeliverable and dropped from reference offers.
pub const UNDELIVERABLE_AFTER_SECONDS: i64 = 4 * 7 * 24 * 60 * 60;

/// Encrypted frames carry at most this many per-user recipients, so that
/// recipient lists do not grow without bound in large groups.
pub const MAXIMUM_RECIPIENTS: usize = 15;

/// XOR two UUIDs.
///
/// Direct messages identify their thread by the XOR of the two participating
/// user IDs, which lets either side derive the counterparty from their own ID
/// without the frame naming a recipient.
pub fn xor(a: Uuid, b: Uuid) -> Uuid {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut out = [0u8; 16];
    for i in 0..16 {
        out[i] = a[i] ^ b[i];
    }
    Uuid::from_bytes(out)
}

/// Current wall-clock time as a Unix timestamp in seconds.
pub fn now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xor_is_reversible() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let x = xor(a, b);
        assert_eq!(xor(x, a), b);
        assert_eq!(xor(x, b), a);
    }

    #[test]
    fn xor_with_self_is_nil() {
        let a = Uuid::new_v4();
        assert_eq!(xor(a, a), Uuid::nil());
    }
}

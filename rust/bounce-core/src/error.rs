//! Error types for the Bounce core.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("msgpack encode error: {0}")]
    Encode(#[from] rmp_serde::encode::Error),

    #[error("msgpack decode error: {0}")]
    Decode(#[from] rmp_serde::decode::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// The schema on disk cannot be brought up to date.
    #[error("schema migration failed: {0}")]
    Migration(String),

    #[error("signed container has invalid signature")]
    InvalidSignature,

    #[error("frame from unknown device is too large ({0} bytes)")]
    UnknownDeviceFrameTooLarge(u32),

    #[error("frame from known device is too large ({0} bytes)")]
    KnownDeviceFrameTooLarge(u32),

    #[error("payload of {0} bytes cannot fit in a frame")]
    PayloadTooLarge(usize),

    #[error("unknown frame type {0}")]
    UnknownFrameType(u16),

    #[error("unknown broadcast scope {0}")]
    UnknownScope(i64),

    #[error("invalid onion address: {0}")]
    InvalidOnionAddress(String),

    #[error("invalid key material: {0}")]
    InvalidKey(String),

    #[error("decryption failed")]
    DecryptionFailed,

    #[error("user not found")]
    UserNotFound,

    #[error("device not found")]
    DeviceNotFound,

    #[error("group not found")]
    GroupNotFound,

    #[error("no profile has been created on this device")]
    NoProfile,

    #[error("a profile already exists on this device")]
    ProfileExists,

    #[error("group state history stack is empty")]
    StackEmpty,

    #[error("change is not permitted: {0}")]
    NotPermitted(&'static str),

    #[error("invalid frame: {0}")]
    InvalidFrame(String),

    #[error("network error: {0}")]
    Network(String),
}

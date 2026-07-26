//! File distribution.
//!
//! Files are split into chunks of at most [`crate::CHUNK_SIZE`] and identified
//! by a hash list. A device that holds a chunk advertises it with a
//! [`ChunkOffer`]; a device that wants it answers with a [`ChunkRequest`]. Once
//! a device has the data it offers the chunk in turn, so a popular file spreads
//! across the group and later arrivals can pull different pieces from different
//! peers, the way a swarm does.
//!
//! Small files (up to [`crate::EMBEDDED_FILE_LIMIT`]) are copied into the blobs
//! directory and fetched automatically. Larger ones are seeded in place from
//! wherever they already are on disk and are only fetched on request.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Broadcastable, SignedFrame};
use crate::error::Result;
use crate::types::{FrameType, Scope};
use crate::{msgpack, xor};

/// Metadata describing a distributed file.
///
/// The `key` and `encrypted_hash_list` let any device that has this record ask
/// an encrypted device for ciphertext chunks and decrypt them, without the
/// encrypted device learning what it is storing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    #[serde(skip)]
    pub signed: SignedFrame,

    #[serde(rename = "ID")]
    pub id: Uuid,

    #[serde(rename = "Name")]
    pub name: String,

    /// One of [`crate::types::FileType`].
    #[serde(rename = "Type")]
    pub file_type: i64,

    /// The message or profile this file belongs to.
    #[serde(rename = "AttachedTo")]
    pub attached_to: Uuid,

    #[serde(rename = "Hash")]
    pub hash: String,

    #[serde(rename = "Size")]
    pub size: i64,

    #[serde(rename = "ChunkSize")]
    pub chunk_size: i64,

    /// Comma-separated chunk hashes, in order.
    #[serde(rename = "HashList")]
    pub hash_list: String,

    /// The same list, encrypted, for use with encrypted devices.
    #[serde(rename = "EncryptedHashList")]
    pub encrypted_hash_list: String,

    #[serde(rename = "Key", default, with = "crate::msgpack::nullable_bytes")]
    pub key: Vec<u8>,

    #[serde(rename = "Nonce", default, with = "crate::msgpack::nullable_bytes")]
    pub nonce: Vec<u8>,

    /// Where the bytes live on this device.
    #[serde(skip)]
    pub path: String,

    /// Whether this device wants the file.
    #[serde(skip)]
    pub wanted: bool,

    #[serde(skip)]
    pub downloaded: bool,

    /// The scope the file is distributed in, mirroring the message it is
    /// attached to.
    #[serde(rename = "Scope")]
    pub scope: i64,

    #[serde(rename = "Destination")]
    pub destination: Uuid,

    #[serde(rename = "Author")]
    pub author: Uuid,

    #[serde(rename = "Timestamp")]
    pub timestamp: i64,

    #[serde(skip)]
    pub saved_at: i64,
}

impl File {
    /// The ordered chunk hashes.
    pub fn chunk_hashes(&self) -> Vec<String> {
        if self.hash_list.is_empty() {
            Vec::new()
        } else {
            self.hash_list.split(',').map(|s| s.to_string()).collect()
        }
    }

    /// How many chunks the file is split into.
    pub fn chunk_count(&self) -> usize {
        self.chunk_hashes().len()
    }

    /// Whether the file is small enough to be embedded and fetched
    /// automatically.
    pub fn is_embedded(&self) -> bool {
        self.size <= crate::EMBEDDED_FILE_LIMIT
    }
}

impl Broadcastable for File {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::File
    }
    fn payload(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(&self.signed.to_container())
    }
    fn scope(&self, _my_id: Uuid) -> Scope {
        Scope::from_i64(self.scope).unwrap_or(Scope::Sync)
    }
    fn destination(&self, my_id: Uuid) -> Uuid {
        match Scope::from_i64(self.scope) {
            // A file shared only with our own devices is addressed to us.
            Ok(Scope::Sync) => my_id,
            // A file in a conversation is addressed by the XOR pair, like the
            // message it is attached to.
            Ok(Scope::User) => xor(self.destination, my_id),
            _ => self.destination,
        }
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

/// An advertisement that a device holds a particular chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkOffer {
    #[serde(skip)]
    pub signed: SignedFrame,

    #[serde(rename = "ID")]
    pub id: Uuid,

    #[serde(rename = "Scope")]
    pub scope: i64,

    #[serde(rename = "Destination")]
    pub destination: Uuid,

    #[serde(rename = "Author")]
    pub author: Uuid,

    #[serde(rename = "FileID")]
    pub file_id: Uuid,

    /// Hash of the chunk being offered.
    #[serde(rename = "Hash")]
    pub hash: String,

    /// Address of the device that holds it.
    #[serde(rename = "Location")]
    pub location: String,

    #[serde(rename = "Timestamp")]
    pub timestamp: i64,

    #[serde(skip)]
    pub saved_at: i64,

    #[serde(skip)]
    pub last_request_time: i64,
}

impl Broadcastable for ChunkOffer {
    fn id(&self) -> Uuid {
        self.id
    }
    fn frame_type(&self) -> FrameType {
        FrameType::ChunkOffer
    }
    fn payload(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(&self.signed.to_container())
    }
    fn scope(&self, _my_id: Uuid) -> Scope {
        Scope::from_i64(self.scope).unwrap_or(Scope::Sync)
    }
    fn destination(&self, my_id: Uuid) -> Uuid {
        if matches!(Scope::from_i64(self.scope), Ok(Scope::User)) {
            xor(self.destination, my_id)
        } else {
            self.destination
        }
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

/// A request for the bytes of one chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkRequest {
    #[serde(rename = "Hash")]
    pub hash: String,
}

impl ChunkRequest {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
}

/// A reply saying a requested chunk is not available after all, so the
/// requester can try another peer rather than waiting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkUnavailable {
    #[serde(rename = "Hash")]
    pub hash: String,
}

impl ChunkUnavailable {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }
}

/// One piece of a file.
///
/// Only `data` goes on the wire; the chunk is identified by hashing what
/// arrives, so a peer cannot mislabel a chunk to poison a download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    #[serde(skip)]
    pub id: Uuid,
    #[serde(skip)]
    pub file_id: Uuid,
    #[serde(skip)]
    pub hash: String,
    #[serde(skip)]
    pub encrypted_hash: String,
    #[serde(skip)]
    pub index: i64,
    #[serde(skip)]
    pub downloaded: bool,

    #[serde(rename = "Data", default, with = "crate::msgpack::nullable_bytes")]
    pub data: Vec<u8>,
}

impl Chunk {
    pub fn encode(&self) -> Result<Vec<u8>> {
        msgpack::to_vec(self)
    }

    /// The hash the data actually has.
    pub fn computed_hash(&self) -> String {
        hex::encode(crate::crypto::hash(&self.data))
    }

    /// Whether the data matches the hash it was requested under.
    pub fn matches_expected_hash(&self, expected: &str) -> bool {
        self.computed_hash() == expected
    }
}

/// Split a buffer into chunks of at most [`crate::CHUNK_SIZE`] bytes.
pub fn split_into_chunks(file_id: Uuid, data: &[u8]) -> Vec<Chunk> {
    data.chunks(crate::CHUNK_SIZE)
        .enumerate()
        .map(|(index, piece)| {
            let mut chunk = Chunk {
                id: Uuid::new_v4(),
                file_id,
                hash: String::new(),
                encrypted_hash: String::new(),
                index: index as i64,
                downloaded: true,
                data: piece.to_vec(),
            };
            chunk.hash = chunk.computed_hash();
            chunk
        })
        .collect()
}

/// Build the comma-separated hash list for a set of chunks, in order.
pub fn hash_list(chunks: &[Chunk]) -> String {
    chunks
        .iter()
        .map(|c| c.hash.clone())
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitting_preserves_the_data() {
        let file_id = Uuid::new_v4();
        // Two and a half chunks.
        let data: Vec<u8> = (0..crate::CHUNK_SIZE * 2 + 512)
            .map(|i| (i % 251) as u8)
            .collect();

        let chunks = split_into_chunks(file_id, &data);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].data.len(), crate::CHUNK_SIZE);
        assert_eq!(chunks[1].data.len(), crate::CHUNK_SIZE);
        assert_eq!(chunks[2].data.len(), 512);

        let rejoined: Vec<u8> = chunks.iter().flat_map(|c| c.data.clone()).collect();
        assert_eq!(rejoined, data);

        // Indices are assigned in order so the file can be reassembled.
        assert_eq!(chunks.iter().map(|c| c.index).collect::<Vec<_>>(), vec![0, 1, 2]);
    }

    #[test]
    fn an_exactly_sized_file_is_one_chunk() {
        let data = vec![7u8; crate::CHUNK_SIZE];
        assert_eq!(split_into_chunks(Uuid::new_v4(), &data).len(), 1);
    }

    #[test]
    fn an_empty_file_has_no_chunks() {
        assert!(split_into_chunks(Uuid::new_v4(), &[]).is_empty());
    }

    #[test]
    fn chunk_integrity_is_checked_against_the_hash() {
        let chunks = split_into_chunks(Uuid::new_v4(), b"some file contents");
        let chunk = &chunks[0];

        assert!(chunk.matches_expected_hash(&chunk.hash));

        // A peer that returns different bytes under the same hash is caught.
        let mut poisoned = chunk.clone();
        poisoned.data = b"different contents".to_vec();
        assert!(!poisoned.matches_expected_hash(&chunk.hash));
    }

    #[test]
    fn identical_chunks_hash_identically() {
        let a = split_into_chunks(Uuid::new_v4(), b"same bytes");
        let b = split_into_chunks(Uuid::new_v4(), b"same bytes");
        // Content addressing does not depend on which file the chunk came from.
        assert_eq!(a[0].hash, b[0].hash);
    }

    #[test]
    fn hash_list_round_trips_through_a_file() {
        let chunks = split_into_chunks(Uuid::new_v4(), &vec![0u8; crate::CHUNK_SIZE * 2 + 1]);
        let file = File {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            name: "big.bin".into(),
            file_type: 2,
            attached_to: Uuid::new_v4(),
            hash: String::new(),
            size: (crate::CHUNK_SIZE * 2 + 1) as i64,
            chunk_size: crate::CHUNK_SIZE as i64,
            hash_list: hash_list(&chunks),
            encrypted_hash_list: String::new(),
            key: vec![],
            nonce: vec![],
            path: String::new(),
            wanted: true,
            downloaded: false,
            scope: Scope::Group.as_i64(),
            destination: Uuid::new_v4(),
            author: Uuid::new_v4(),
            timestamp: 0,
            saved_at: 0,
        };

        assert_eq!(file.chunk_count(), 3);
        assert_eq!(
            file.chunk_hashes(),
            chunks.iter().map(|c| c.hash.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn embedded_threshold_matches_the_limit() {
        let mut file = File {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            name: String::new(),
            file_type: 2,
            attached_to: Uuid::nil(),
            hash: String::new(),
            size: crate::EMBEDDED_FILE_LIMIT,
            chunk_size: 0,
            hash_list: String::new(),
            encrypted_hash_list: String::new(),
            key: vec![],
            nonce: vec![],
            path: String::new(),
            wanted: false,
            downloaded: false,
            scope: Scope::Sync.as_i64(),
            destination: Uuid::nil(),
            author: Uuid::nil(),
            timestamp: 0,
            saved_at: 0,
        };

        assert!(file.is_embedded());
        file.size += 1;
        assert!(!file.is_embedded());
    }

    #[test]
    fn file_destination_follows_its_scope() {
        let me = Uuid::new_v4();
        let them = Uuid::new_v4();

        let mut file = File {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            name: String::new(),
            file_type: 2,
            attached_to: Uuid::nil(),
            hash: String::new(),
            size: 0,
            chunk_size: 0,
            hash_list: String::new(),
            encrypted_hash_list: String::new(),
            key: vec![],
            nonce: vec![],
            path: String::new(),
            wanted: false,
            downloaded: false,
            scope: Scope::User.as_i64(),
            destination: xor(me, them),
            author: me,
            timestamp: 0,
            saved_at: 0,
        };
        assert_eq!(file.destination(me), them);

        file.scope = Scope::Sync.as_i64();
        assert_eq!(file.destination(me), me);

        let group = Uuid::new_v4();
        file.scope = Scope::Group.as_i64();
        file.destination = group;
        assert_eq!(file.destination(me), group);
    }

    #[test]
    fn only_chunk_data_goes_on_the_wire() {
        let chunk = &split_into_chunks(Uuid::new_v4(), b"payload")[0];
        let encoded = chunk.encode().unwrap();
        let text = String::from_utf8_lossy(&encoded);

        assert!(text.contains("Data"));
        assert!(!text.contains("Hash"));
        assert!(!text.contains("Index"));
        assert!(!text.contains("FileID"));

        let decoded: Chunk = msgpack::from_slice(&encoded).unwrap();
        assert_eq!(decoded.data, chunk.data);
        // The receiver recomputes the identity rather than trusting it.
        assert_eq!(decoded.computed_hash(), chunk.hash);
    }
}

//! Emit Rust-encoded frames as JSON, so the Go implementation can decode them.
//!
//! This is the outbound half of the interop check in `tests/go_interop.rs`:
//!
//! ```text
//! cargo run -p libbounce --example emit_fixtures | go run . verify
//! ```

use libbounce::crypto::DeviceKey;
use libbounce::frames::file::{self, ChunkOffer, File};
use libbounce::frames::group::{Group, GroupCreation};
use libbounce::frames::identity::{Device, User};
use libbounce::frames::message::{DirectMessage, FileAttachment};
use libbounce::frames::transport::{Ack, FrameReference};
use libbounce::frames::SignedFrame;
use libbounce::signed::SignedContainer;
use libbounce::types::Scope;
use libbounce::{msgpack, types::FrameType};
use uuid::Uuid;

fn main() {
    // A deterministic key, so repeated runs produce comparable output.
    let key = DeviceKey::from_seed(&[42u8; 32]);

    let author = Uuid::parse_str("bbbbbbbb-0000-4000-8000-000000000002").unwrap();
    let recipient = Uuid::parse_str("cccccccc-0000-4000-8000-000000000003").unwrap();

    let mut message = DirectMessage::new(author, recipient, "hello from Rust 🦀".into(), 1_700_000_000);
    message.delete_at = 1_700_086_400;
    message.file_attachments.push(FileAttachment {
        id: Uuid::parse_str("dddddddd-0000-4000-8000-000000000004").unwrap(),
        file_id: Uuid::parse_str("eeeeeeee-0000-4000-8000-000000000005").unwrap(),
        message_id: message.id,
        name: "notes.txt".into(),
        size: 2048,
    });

    let body = msgpack::to_vec(&message).expect("message encodes");
    let container = SignedContainer::create(&key, body);
    let container_bytes = container.encode().expect("container encodes");

    // A group creation, so Go can check that ID derivation agrees.
    let creator_id = Uuid::parse_str("11111111-0000-4000-8000-000000000006").unwrap();
    let mut creator = User::new(creator_id, "Rust Creator".into());
    creator.devices.push(Device::new(
        Uuid::parse_str("22222222-0000-4000-8000-000000000007").unwrap(),
        creator_id,
        key.address(),
        1_700_000_000,
    ));

    let group = Group {
        name: "Rust Interop Group".into(),
        created_by: creator_id,
        created_at: 1_700_000_000,
        users: vec![creator],
        admins: creator_id.to_string(),
        ..Default::default()
    };
    let creation = GroupCreation::create(&group, 1_700_000_000).expect("creation builds");
    let creation_bytes = msgpack::to_vec(&creation).expect("creation encodes");

    // A file and one of its chunk offers. Both carry byte fields that are
    // empty for an unencrypted file, which is exactly the shape that used to
    // come out as an empty array and be refused by Go.
    let file_id = Uuid::parse_str("33333333-0000-4000-8000-000000000008").unwrap();
    let chunks = file::split_into_chunks(file_id, b"a short attachment");
    let file = File {
        signed: SignedFrame::default(),
        id: file_id,
        name: "photo.png".into(),
        file_type: 2,
        attached_to: message.id,
        hash: hex::encode(libbounce::crypto::hash(b"a short attachment")),
        size: 18,
        chunk_size: libbounce::CHUNK_SIZE as i64,
        hash_list: file::hash_list(&chunks),
        encrypted_hash_list: String::new(),
        key: Vec::new(),
        nonce: Vec::new(),
        path: String::new(),
        wanted: true,
        downloaded: true,
        scope: Scope::User.as_i64(),
        destination: message.xor,
        author,
        timestamp: 1_700_000_000,
        saved_at: 0,
    };
    let file_bytes = msgpack::to_vec(&file).expect("file encodes");

    let offer = ChunkOffer {
        signed: SignedFrame::default(),
        id: Uuid::parse_str("44444444-0000-4000-8000-000000000009").unwrap(),
        scope: Scope::User.as_i64(),
        destination: message.xor,
        author,
        file_id,
        hash: chunks[0].hash.clone(),
        location: key.address(),
        timestamp: 1_700_000_000,
        saved_at: 0,
        last_request_time: 0,
    };
    let offer_bytes = msgpack::to_vec(&offer).expect("offer encodes");

    let ack = Ack {
        references: vec![
            FrameReference::new(message.id, FrameType::DirectMessage),
            FrameReference::new(creation.id, FrameType::UpdateGroup),
        ],
    };
    let ack_bytes = ack.encode().expect("ack encodes");

    let output = serde_json::json!({
        "ed25519_public_key": hex::encode(key.public_key()),
        "onion_address": key.address(),
        "signed_container": hex::encode(&container_bytes),
        "group_creation": hex::encode(&creation_bytes),
        "group_id": creation.id.to_string(),
        "ack": hex::encode(&ack_bytes),
        "file": hex::encode(&file_bytes),
        "chunk_offer": hex::encode(&offer_bytes),
        "chunk_hash": chunks[0].hash.clone(),
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

//! Cross-implementation compatibility with the reference Go engine.
//!
//! `tests/fixtures/go_fixtures.json` holds frames encoded by the Go
//! implementation, using the same libraries `chat/` depends on:
//! `Basekick-Labs/msgpack/v6`, `google/uuid`, `zeebo/blake3`, and the standard
//! library's Ed25519. These tests decode those bytes and check that the Rust
//! port agrees about every field, every signature, and every derived ID.
//!
//! To regenerate the fixtures, run the harness in `tests/fixtures/`:
//!
//! ```text
//! go run . emit > go_fixtures.json
//! ```
//!
//! The reverse direction — Go decoding Rust output — is driven by
//! `examples/emit_fixtures.rs`.

use bounce_core::crypto;
use bounce_core::frames::group::GroupCreation;
use bounce_core::frames::message::{DirectMessage, TypingIndicator};
use bounce_core::frames::transport::Ack;
use bounce_core::onion;
use bounce_core::signed::SignedContainer;
use bounce_core::{msgpack, types::FrameType};
use uuid::Uuid;

/// Load the Go-produced fixture set.
fn fixtures() -> serde_json::Map<String, serde_json::Value> {
    let raw = include_str!("fixtures/go_fixtures.json");
    serde_json::from_str::<serde_json::Value>(raw)
        .expect("fixtures are valid JSON")
        .as_object()
        .expect("fixtures are a JSON object")
        .clone()
}

fn bytes(name: &str) -> Vec<u8> {
    let fixtures = fixtures();
    let hex_string = fixtures[name].as_str().expect("fixture is a hex string");
    hex::decode(hex_string).expect("fixture is valid hex")
}

#[test]
fn decodes_a_direct_message_encoded_by_go() {
    let body = bytes("direct_message_body");
    let message: DirectMessage = msgpack::from_slice(&body).expect("Go's encoding decodes");

    assert_eq!(
        message.id,
        Uuid::parse_str("aaaaaaaa-0000-4000-8000-000000000001").unwrap()
    );
    assert_eq!(
        message.author,
        Uuid::parse_str("bbbbbbbb-0000-4000-8000-000000000002").unwrap()
    );
    assert_eq!(
        message.xor,
        Uuid::parse_str("cccccccc-0000-4000-8000-000000000003").unwrap()
    );
    assert_eq!(message.written_at, 1_700_000_000);
    assert_eq!(message.delete_at, 1_700_086_400);

    // Multi-byte characters survive the round trip intact.
    assert_eq!(message.text, "hello from Go 👋");

    assert_eq!(message.file_attachments.len(), 1);
    let attachment = &message.file_attachments[0];
    assert_eq!(attachment.name, "report.pdf");
    assert_eq!(attachment.size, 4096);
    assert_eq!(attachment.message_id, message.id);

    assert!(message.image_attachments.is_empty());
}

/// A plain text message exactly as `chat/direct_message.go` builds one.
///
/// `SendDirectMessage` never assigns the attachment slices, so Go marshals them
/// as `nil` (`0xc0`) rather than as empty arrays. Every ordinary message from a
/// Go client looks like this, and a `Vec<T>` that cannot decode `nil` would
/// reject all of them.
const GO_MESSAGE_WITH_NIL_ATTACHMENTS: &str = "88a24944c410aaaaaaaa0000400080000000000000\
01a95772697474656e4174d3000000006553f100a844656c6574654174d30000000000000000a6417574686f72\
c410bbbbbbbb000040008000000000000002a3586f72c410cccccccc000040008000000000000003a454657874\
ba706c61696e20746578742c206e6f206174746163686d656e7473af46696c654174746163686d656e7473c0b0\
496d6167654174746163686d656e7473c0";

#[test]
fn decodes_a_go_message_whose_attachment_lists_are_nil() {
    let bytes = hex::decode(GO_MESSAGE_WITH_NIL_ATTACHMENTS.replace(['\n', ' '], ""))
        .expect("fixture is valid hex");

    let message: DirectMessage =
        msgpack::from_slice(&bytes).expect("a plain Go message must decode");

    assert_eq!(message.text, "plain text, no attachments");
    assert_eq!(message.written_at, 1_700_000_000);
    assert!(message.file_attachments.is_empty());
    assert!(message.image_attachments.is_empty());
    assert!(!message.is_empty(), "it has text, so it is not an empty message");
}

#[test]
fn agrees_with_go_on_the_blake3_digest() {
    let body = bytes("direct_message_body");
    let expected = bytes("body_blake3");
    assert_eq!(crypto::hash(&body).to_vec(), expected);
}

#[test]
fn verifies_a_signature_produced_by_go() {
    let container_bytes = bytes("signed_container");
    let container: SignedContainer =
        msgpack::from_slice(&container_bytes).expect("Go's container decodes");

    // The fixture's signer field is a placeholder, since the harness signs with
    // a bare Ed25519 key rather than a live hidden service. Recover the address
    // the key would have and verify against that.
    let public_key: [u8; 32] = bytes("ed25519_public_key")
        .try_into()
        .expect("32 byte public key");
    let address = onion::address_from_public_key(&public_key);

    let digest = crypto::hash(&container.payload);
    assert!(
        crypto::verify_signature(&address, &digest, &container.signature),
        "a signature made by Go must verify in Rust"
    );

    // And the same container with a modified payload must not.
    let mut tampered = container.clone();
    tampered.payload.push(0);
    let tampered_digest = crypto::hash(&tampered.payload);
    assert!(!crypto::verify_signature(
        &address,
        &tampered_digest,
        &tampered.signature
    ));
}

#[test]
fn derives_the_same_group_id_as_go() {
    let group_data = bytes("group_data");
    let expected = fixtures()["group_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .expect("fixture group id parses");

    assert_eq!(
        GroupCreation::derive_id(&group_data),
        expected,
        "group IDs must be derived identically or the two implementations \
         would disagree about which group a frame belongs to"
    );
}

#[test]
fn decodes_a_group_creation_encoded_by_go() {
    let creation_bytes = bytes("group_creation");
    let creation: GroupCreation =
        msgpack::from_slice(&creation_bytes).expect("Go's group creation decodes");

    assert!(creation.id_matches_data());
    assert_eq!(creation.timestamp, 1_700_000_000);

    let group = creation.group().expect("embedded group decodes");
    assert_eq!(group.name, "Interop Group");
    assert_eq!(group.users.len(), 1);
    assert_eq!(group.users[0].name, "Creator");
    assert_eq!(group.users[0].devices.len(), 1);
    assert_eq!(group.users[0].devices[0].address, "someonionaddress");
    assert_eq!(group.admin_ids(), vec![group.created_by]);

    // The founding state that consensus starts from is recovered intact.
    let state = bounce_core::consensus::GroupState::from_group(&group);
    assert!(state.is_member(group.created_by));
    assert!(state.is_admin(group.created_by));
}

#[test]
fn a_go_typing_indicator_threads_by_xor() {
    // Go's TypingInDirectMessage sets Thread to xor(sender, recipient), the
    // same convention a direct message uses. Reading it as the raw recipient ID
    // makes the indicator unroutable in both directions — it is silently
    // dropped rather than failing loudly, which is how it went unnoticed.
    use bounce_core::frames::Broadcastable;

    let indicator: TypingIndicator =
        msgpack::from_slice(&bytes("typing_indicator")).expect("Go's indicator decodes");

    let fixtures = fixtures();
    let sender = Uuid::parse_str(fixtures["typing_sender"].as_str().unwrap()).unwrap();
    let recipient = Uuid::parse_str(fixtures["typing_recipient"].as_str().unwrap()).unwrap();

    assert_eq!(indicator.author, sender);
    assert_eq!(
        indicator.thread,
        bounce_core::xor(sender, recipient),
        "Thread must be the XOR pair, not either participant's ID"
    );

    // And each side recovers the other from it.
    assert_eq!(indicator.destination(recipient), sender);
    assert_eq!(indicator.destination(sender), recipient);
}

#[test]
fn decodes_an_ack_encoded_by_go() {
    let ack: Ack = msgpack::from_slice(&bytes("ack")).expect("Go's ack decodes");

    assert_eq!(ack.references.len(), 2);
    assert_eq!(
        ack.references[0].frame_id,
        Uuid::parse_str("aaaaaaaa-0000-4000-8000-000000000001").unwrap()
    );
    assert_eq!(ack.references[0].kind().unwrap(), FrameType::DirectMessage);
    assert_eq!(ack.references[1].kind().unwrap(), FrameType::UpdateGroup);
}

#[test]
fn re_encoding_a_go_frame_produces_an_equivalent_frame() {
    // Rust and Go differ on integer width — Go always writes fixed-width i64,
    // rmp-serde writes compact — so the bytes are not identical. What must hold
    // is that a Go frame decoded and re-encoded by Rust decodes back to the
    // same values.
    let original = bytes("direct_message_body");
    let message: DirectMessage = msgpack::from_slice(&original).unwrap();

    let reencoded = msgpack::to_vec(&message).unwrap();
    let round_tripped: DirectMessage = msgpack::from_slice(&reencoded).unwrap();

    assert_eq!(round_tripped, message);
}

#[test]
fn a_file_with_no_key_encodes_its_empty_fields_as_bin() {
    // rmp-serde picks `bin` over an array by inspecting a sequence's elements,
    // and an empty sequence has none — so an unencrypted file's `Key` and
    // `Nonce` came out as `0x90` and Go refused the entire frame with
    // "invalid code=90 decoding string/bytes length". Every attachment sent in
    // the clear has this shape, so it is the common case rather than a corner.
    use bounce_core::frames::file::File;
    use bounce_core::frames::SignedFrame;

    let file = File {
        signed: SignedFrame::default(),
        id: Uuid::nil(),
        name: "photo.png".into(),
        file_type: 2,
        attached_to: Uuid::nil(),
        hash: String::new(),
        size: 1,
        chunk_size: 1,
        hash_list: String::new(),
        encrypted_hash_list: String::new(),
        key: Vec::new(),
        nonce: Vec::new(),
        path: String::new(),
        wanted: false,
        downloaded: false,
        scope: 0,
        destination: Uuid::nil(),
        author: Uuid::nil(),
        timestamp: 0,
        saved_at: 0,
    };

    let encoded = msgpack::to_vec(&file).expect("encodes");

    // "Key" (0xa3 'K' 'e' 'y') followed by bin8 of length zero.
    assert!(
        encoded
            .windows(6)
            .any(|window| window == [0xa3, b'K', b'e', b'y', 0xc4, 0x00]),
        "Key must be an empty bin, got {}",
        hex::encode(&encoded),
    );

    // And it still decodes back to an empty vector rather than to nothing.
    let decoded: File = msgpack::from_slice(&encoded).expect("decodes");
    assert!(decoded.key.is_empty());
    assert!(decoded.nonce.is_empty());
}

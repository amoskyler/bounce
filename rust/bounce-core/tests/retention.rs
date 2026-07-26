//! Do messages that were promised to disappear actually disappear?
//!
//! Every one of these tests fails against the shipped port, and most of them
//! fail *silently* from the user's point of view: the timer counts down on the
//! bubble, the bubble stays, and the plaintext is still in `bounce.db`. The
//! attachment tests are the sharper half — clear-history is an explicitly
//! privacy-motivated action, and before this every photo in the conversation
//! remained recoverable from the database afterwards.

use std::sync::Arc;
use std::time::Duration;

use bounce_core::crypto::DeviceKey;
use bounce_core::engine::files::OutgoingAttachment;
use bounce_core::engine::{Engine, Event};
use bounce_core::frames::identity::User;
use bounce_core::frames::message::{DirectMessage, ImageAttachment};
use bounce_core::net::{StaticDirectory, TcpNetwork};
use bounce_core::store::Store;
use tokio::sync::mpsc::UnboundedReceiver;
use uuid::Uuid;

struct Instance {
    engine: Arc<Engine<TcpNetwork>>,
    store: Arc<Store>,
    events: UnboundedReceiver<Event>,
    user: User,
    address: String,
}

async fn start(name: &str, directory: Arc<StaticDirectory>) -> Instance {
    let key = DeviceKey::generate();
    let address = key.address();
    let network = Arc::new(TcpNetwork::bind(key.clone(), directory).await.expect("binds"));
    let store = Arc::new(Store::in_memory().expect("opens"));
    let (engine, events) = Engine::new(key, Arc::clone(&store), network);
    let user = engine.create_profile(name, "laptop").expect("profile");
    Instance { engine, store, events, user, address }
}

/// A contact with no devices: enough to hang a conversation off, without a
/// second engine.
fn contact(instance: &Instance, name: &str) -> Uuid {
    let mut user = User::new(Uuid::new_v4(), name.into());
    user.accepted = true;
    user.open_dm = true;
    instance.store.save_user(&user).expect("saves the contact");
    user.id
}

/// Store a message directly, so its timestamps can be whatever the test needs.
///
/// The engine only ever stamps `delete_at` in the future, so a message that has
/// *already* expired — the one case the sweep exists for — cannot be produced
/// through the sending path without waiting for it in real time.
fn store_message(instance: &Instance, with: Uuid, text: &str, written_at: i64, delete_at: i64) -> DirectMessage {
    let mut message = DirectMessage::new(instance.user.id, with, text.into(), written_at);
    message.delete_at = delete_at;
    message.saved_at = written_at;
    instance.store.save_direct_message(&message).expect("saves");
    message
}

fn deleted_ids(events: &mut UnboundedReceiver<Event>) -> Vec<Uuid> {
    let mut ids = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let Event::MessageDeleted { message_id } = event {
            ids.push(message_id);
        }
    }
    ids
}

/// A one-chunk image, small enough to be embedded and fetched automatically.
fn photo() -> (Vec<u8>, OutgoingAttachment) {
    let data: Vec<u8> = (0..4096).map(|index| (index % 251) as u8).collect();
    (
        data.clone(),
        OutgoingAttachment {
            name: "photo.png".into(),
            data,
            is_image: true,
            width: 32,
            height: 32,
            blur_hash: String::new(),
        },
    )
}

#[tokio::test]
async fn a_message_past_its_expiry_is_deleted_and_the_client_is_told() {
    // The gap itself. `delete_at` was written by both send paths and read back
    // by nothing but the timer on the bubble.
    let directory = Arc::new(StaticDirectory::new());
    let mut alice = start("Alice", directory).await;
    let bob = contact(&alice, "Bob");

    let now = bounce_core::now();
    let expired = store_message(&alice, bob, "this was meant to vanish", now - 120, now - 60);
    let kept = store_message(&alice, bob, "this one is not on a timer", now - 120, 0);
    let later = store_message(&alice, bob, "this one expires tomorrow", now - 120, now + 86_400);

    let removed = alice.engine.sweep_expired().expect("sweeps");

    assert_eq!(removed, vec![expired.id], "exactly the expired message goes");
    assert!(
        alice.store.direct_message(expired.id).unwrap().is_none(),
        "the plaintext must be out of the database, not merely off the screen",
    );
    assert!(alice.store.direct_message(kept.id).unwrap().is_some());
    assert!(alice.store.direct_message(later.id).unwrap().is_some());

    assert_eq!(
        deleted_ids(&mut alice.events),
        vec![expired.id],
        "the interface has no other way to learn a message went away",
    );
}

#[tokio::test]
async fn the_sweep_runs_on_its_own_for_as_long_as_the_engine_does() {
    // A client left open for a week is the case that matters: nothing calls
    // the sweep by hand, so the timer task is the whole feature.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", directory).await;
    let bob = contact(&alice, "Bob");

    let now = bounce_core::now();
    let expired = store_message(&alice, bob, "gone by now", now - 120, now - 60);

    tokio::spawn(Arc::clone(&alice.engine).run_retention());

    for _ in 0..50 {
        if alice.store.direct_message(expired.id).unwrap().is_none() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the retention task never swept an expired message");
}

#[tokio::test]
async fn the_sweep_enforces_a_cutoff_that_arrived_from_somewhere_else() {
    // `clear_before` is shared conversation state. It reaches this device
    // through consensus or catch-up, and the group path folds it into the
    // group record without deleting a thing — so the sweep is what makes the
    // cutoff mean the same on every device rather than only on the one the
    // button was pressed on.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", directory).await;
    let bob = contact(&alice, "Bob");

    let now = bounce_core::now();
    let before = store_message(&alice, bob, "from before the clear", now - 600, 0);
    let after = store_message(&alice, bob, "from after it", now - 60, 0);

    let mut user = alice.store.user(bob).unwrap().expect("the contact exists");
    user.clear_before = now - 300;
    alice.store.update_user_local_state(&user).unwrap();

    let removed = alice.engine.sweep_expired().expect("sweeps");

    assert_eq!(removed, vec![before.id]);
    assert!(alice.store.direct_message(before.id).unwrap().is_none());
    assert!(
        alice.store.direct_message(after.id).unwrap().is_some(),
        "the cutoff is a cutoff, not a wipe",
    );
}

#[tokio::test]
async fn deleting_a_message_takes_its_attachment_bytes_with_it() {
    // The privacy defect behind clear-history: the rows went, the bytes did
    // not, and every photo in the conversation stayed recoverable from
    // `chunks.data`.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", directory).await;
    let bob = contact(&alice, "Bob");

    let (bytes, attachment) = photo();
    let sent = alice
        .engine
        .send_direct_message_with_attachments(bob, "look at this", vec![attachment])
        .await
        .expect("sends");

    let file_id = sent.attachments[0].file_id;
    let hashes = alice
        .store
        .file(file_id)
        .unwrap()
        .expect("the file was stored")
        .chunk_hashes();
    assert_eq!(
        alice.store.file_data(file_id).unwrap().as_deref(),
        Some(bytes.as_slice()),
        "the bytes are there to begin with, or this test proves nothing",
    );

    // Clear the history, exactly as `clear_history` does: everything written
    // before the cutoff goes.
    alice
        .store
        .delete_messages_before(bob, sent.written_at + 1)
        .expect("clears");

    assert!(alice.store.file(file_id).unwrap().is_none(), "the file record survived");
    assert!(alice.store.file_data(file_id).unwrap().is_none());
    for hash in &hashes {
        assert!(
            alice.store.chunk_data(hash).unwrap().is_none(),
            "the bytes of {hash} are still recoverable from the database",
        );
    }
}

#[tokio::test]
async fn an_expiring_message_takes_its_attachment_bytes_with_it() {
    // The same cascade, reached through the sweep rather than through
    // clear-history: a disappearing photo has to disappear.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", directory).await;
    let bob = contact(&alice, "Bob");

    // A one second policy, so the send path stamps `delete_at` itself: the
    // expiry has to be the real one for this to be a test of the product
    // rather than of the fixture.
    let mut policy = alice.store.user(bob).unwrap().unwrap();
    policy.retention = 1;
    alice.store.update_user_local_state(&policy).unwrap();

    let (_, attachment) = photo();
    let sent = alice
        .engine
        .send_direct_message_with_attachments(bob, "burn after reading", vec![attachment])
        .await
        .expect("sends");
    let file_id = sent.attachments[0].file_id;
    let hashes = alice.store.file(file_id).unwrap().unwrap().chunk_hashes();
    assert!(sent.expires_at > 0, "the send path must stamp an expiry");

    tokio::time::sleep(Duration::from_millis(2100)).await;
    alice.engine.sweep_expired().expect("sweeps");

    assert!(alice.store.direct_message(sent.id).unwrap().is_none());
    for hash in &hashes {
        assert!(
            alice.store.chunk_data(hash).unwrap().is_none(),
            "an expired photo's bytes are still in the database",
        );
    }
}

#[tokio::test]
async fn a_chunk_another_file_still_needs_is_not_taken_with_it() {
    // Chunks are content-addressed and shared: the same photo sent twice is
    // two `files` rows over one set of bytes, and only the row that actually
    // fetched them holds them — `file_progress` counts a chunk as held if the
    // content exists anywhere at all. Cascading the delete without checking
    // would leave the second photo permanently unopenable.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", directory).await;
    let bob = contact(&alice, "Bob");

    let (bytes, attachment) = photo();
    let sent = alice
        .engine
        .send_direct_message_with_attachments(bob, "the first copy", vec![attachment])
        .await
        .expect("sends");
    let first_file = sent.attachments[0].file_id;

    // A second message carrying the same content, as it looks on the receiving
    // side: the chunk rows exist and are empty, because the bytes were already
    // in the database under the first file and were never fetched again.
    let mut second_file = alice.store.file(first_file).unwrap().expect("the first file");
    let hashes = second_file.chunk_hashes();
    second_file.id = Uuid::new_v4();
    second_file.downloaded = false;

    let mut second = DirectMessage::new(alice.user.id, bob, "the second copy".into(), sent.written_at + 60);
    second_file.attached_to = second.id;
    second.image_attachments.push(ImageAttachment {
        id: Uuid::new_v4(),
        file_id: second_file.id,
        message_id: second.id,
        name: second_file.name.clone(),
        size: second_file.size,
        width: 32,
        height: 32,
        blur_hash: String::new(),
    });
    alice.store.save_file(&second_file).expect("saves the second file");
    for (index, hash) in hashes.iter().enumerate() {
        alice
            .store
            .save_chunk(second_file.id, index as i64, hash, None)
            .expect("saves an empty chunk row");
    }
    alice.store.save_direct_message(&second).expect("saves");

    assert_eq!(
        alice.store.file_data(second_file.id).unwrap().as_deref(),
        Some(bytes.as_slice()),
        "the second copy reads through to the shared bytes to begin with",
    );

    // The first message goes. The second one is still on screen.
    alice
        .store
        .delete_messages_before(bob, sent.written_at + 1)
        .expect("clears");

    assert!(alice.store.file(first_file).unwrap().is_none());
    assert!(alice.store.direct_message(second.id).unwrap().is_some());
    assert_eq!(
        alice.store.file_data(second_file.id).unwrap().as_deref(),
        Some(bytes.as_slice()),
        "a chunk a surviving file still needs must survive the delete",
    );

    // And once nothing wants them, the bytes go.
    alice
        .store
        .delete_messages_before(bob, second.written_at + 1)
        .expect("clears the rest");
    for hash in &hashes {
        assert!(
            alice.store.chunk_data(hash).unwrap().is_none(),
            "the last reference is gone, so the bytes must be too",
        );
    }
}

#[tokio::test]
async fn a_message_that_expired_in_transit_is_refused() {
    // Otherwise retention is undone by gossip: every peer that has not caught
    // up re-offers what it still holds, so a message the sweep deleted comes
    // straight back on the next connection. Go refuses it on the way in
    // (`chat/direct_message.go:236`) for exactly this reason.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;
    tokio::spawn(Arc::clone(&alice.engine).run_listener());
    tokio::spawn(Arc::clone(&bob.engine).run_listener());

    // Introduce them by hand, without opening a connection.
    for (left, right) in [(&alice, &bob), (&bob, &alice)] {
        let mut record = right.user.clone();
        record.profile = false;
        record.private_ecdh_key = Vec::new();
        record.private_ecdsa_key = Vec::new();
        record.accepted = true;
        left.store.save_user(&record).expect("saves the contact");
    }

    // A one second policy, so the message is stale by the time Bob is dialled.
    let mut policy = alice.store.user(bob.user.id).unwrap().unwrap();
    policy.retention = 1;
    alice.store.update_user_local_state(&policy).unwrap();

    alice
        .engine
        .send_direct_message(bob.user.id, "should never be readable")
        .await
        .expect("sends into the void");
    tokio::time::sleep(Duration::from_millis(2100)).await;

    // Now they meet, and Alice replays what she is still holding.
    Arc::clone(&alice.engine).connect(&bob.address).await.expect("dials");

    // The control: a message with no policy on it, sent afterwards, proves the
    // channel works and the first one was refused rather than merely lost.
    policy.retention = 0;
    alice.store.update_user_local_state(&policy).unwrap();
    alice
        .engine
        .send_direct_message(bob.user.id, "this one keeps")
        .await
        .expect("sends");

    let mut received = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(250), bob.events.recv()).await {
            Ok(Some(Event::MessageReceived { message })) => {
                received.push(message.text.clone());
                if message.text == "this one keeps" {
                    break;
                }
            }
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    assert!(
        received.contains(&"this one keeps".to_string()),
        "the control message never arrived, so this test proves nothing: {received:?}",
    );
    assert!(
        !received.contains(&"should never be readable".to_string()),
        "a message that expired in transit was accepted and stored",
    );
}

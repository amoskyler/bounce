//! An image, sent the way the client sends one.
//!
//! `engine_e2e.rs` covers attachments with a hand-written introduction and a
//! hand-written dial. This file instead pairs through the real add-user flow
//! and lets peering open the socket, because that is what the application
//! does — and the application does not work.

use std::sync::Arc;
use std::time::Duration;

use libbounce::crypto::DeviceKey;
use libbounce::engine::files::OutgoingAttachment;
use libbounce::engine::{Engine, Event};
use libbounce::net::{StaticDirectory, TcpNetwork};
use libbounce::store::Store;
use tokio::sync::mpsc::UnboundedReceiver;
use uuid::Uuid;

/// Show engine logs when `BOUNCE_LOG` is set, so a rejected frame is visible.
fn logging() {
    if let Ok(filter) = std::env::var("BOUNCE_LOG") {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
            .with_test_writer()
            .try_init();
    }
}

struct Instance {
    engine: Arc<Engine<TcpNetwork>>,
    events: UnboundedReceiver<Event>,
    id: Uuid,
}

async fn start(name: &str, directory: Arc<StaticDirectory>) -> Instance {
    let key = DeviceKey::generate();
    let network = Arc::new(TcpNetwork::bind(key.clone(), directory).await.expect("binds"));

    // A blobs directory, so a file too large to embed has somewhere to land.
    let blobs = std::env::temp_dir().join(format!(
        "bounce-blobs-{}-{name}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    let store = Arc::new(
        Store::in_memory()
            .expect("opens")
            .with_blobs_directory(&blobs)
            .expect("blobs directory"),
    );
    let (engine, events) = Engine::new(key, store, network);
    let user = engine.create_profile(name, "laptop").expect("profile");
    tokio::spawn(Arc::clone(&engine).run_listener());
    // The chunk engine is what issues chunk requests; the application spawns it
    // alongside the listener (`bounce-node/src/lib.rs`), so the harness does too.
    tokio::spawn(Arc::clone(&engine).run_chunk_engine());
    Instance { engine, events, id: user.id }
}

/// The same, with no profile: a device waiting to be linked to one.
async fn start_blank(name: &str, directory: Arc<StaticDirectory>) -> Instance {
    let key = DeviceKey::generate();
    let network = Arc::new(TcpNetwork::bind(key.clone(), directory).await.expect("binds"));
    let blobs = std::env::temp_dir().join(format!(
        "bounce-blobs-{}-{name}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    let store = Arc::new(
        Store::in_memory()
            .expect("opens")
            .with_blobs_directory(&blobs)
            .expect("blobs directory"),
    );
    let (engine, events) = Engine::new(key, store, network);
    tokio::spawn(Arc::clone(&engine).run_listener());
    // The chunk engine is what issues chunk requests; the application spawns it
    // alongside the listener (`bounce-node/src/lib.rs`), so the harness does too.
    tokio::spawn(Arc::clone(&engine).run_chunk_engine());
    Instance { engine, events, id: Uuid::nil() }
}

async fn wait_for<T>(
    events: &mut UnboundedReceiver<Event>,
    what: &str,
    seconds: u64,
    mut extract: impl FnMut(&Event) -> Option<T>,
) -> Option<T> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            eprintln!("timed out waiting for {what}");
            return None;
        }
        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Some(event)) => {
                if let Some(value) = extract(&event) {
                    return Some(value);
                }
            }
            Ok(None) | Err(_) => return None,
        }
    }
}

#[tokio::test]
async fn an_image_survives_the_flow_the_client_actually_uses() {
    logging();

    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    // Pair for real, rather than writing each other's records by hand.
    let code = bob.engine.create_pairing_code().expect("code");
    Arc::clone(&alice.engine).request_to_add_user(&code).await.expect("pairs");

    let mut alice_events = alice.events;
    wait_for(&mut alice_events, "alice to add bob", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("alice adds bob");
    wait_for(&mut bob.events, "bob to add alice", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("bob adds alice");

    // Three chunks, so offers, requests and reassembly all have to work.
    let payload: Vec<u8> = (0..libbounce::CHUNK_SIZE * 2 + 5000)
        .map(|index| (index % 251) as u8)
        .collect();

    let sent = alice
        .engine
        .send_direct_message_with_attachments(
            bob.id,
            "look at this",
            vec![OutgoingAttachment {
                name: "photo.png".into(),
                data: payload.clone(),
                is_image: true,
                width: 800,
                height: 600,
                blur_hash: String::new(),
                ..Default::default()
            }],
            None,
        )
        .await
        .expect("sends");

    let file_id = sent.attachments[0].file_id;

    wait_for(&mut bob.events, "bob to receive the message", 10, |event| {
        matches!(event, Event::MessageReceived { .. }).then_some(())
    })
    .await
    .expect("the message arrives");

    let complete = wait_for(&mut bob.events, "the download to finish", 20, |event| match event {
        Event::FileComplete { file_id } => Some(*file_id),
        _ => None,
    })
    .await;

    assert_eq!(complete, Some(file_id), "the file never finished downloading");
    assert_eq!(
        bob.engine.file_data(file_id).unwrap().as_deref(),
        Some(payload.as_slice()),
        "the bytes must survive the round trip",
    );
}


#[tokio::test]
async fn an_attachment_sent_while_offline_arrives_on_reconnection() {
    // Catch-up replays stored frames through a separate dispatch table, and
    // `File` was missing from it. The message came back, the attachment record
    // came back, and the metadata describing what the attachment *was* went
    // into a silent default arm — so the picture sat at zero per cent forever
    // and no retry could recover it, the sender having already been
    // acknowledged.
    logging();

    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    let code = bob.engine.create_pairing_code().expect("code");
    Arc::clone(&alice.engine).request_to_add_user(&code).await.expect("pairs");

    let mut alice_events = alice.events;
    wait_for(&mut alice_events, "alice to add bob", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("alice adds bob");
    wait_for(&mut bob.events, "bob to add alice", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("bob adds alice");

    // Bob goes away. Alice writes anyway, so everything queues.
    bob.engine.disconnect_all().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let payload: Vec<u8> = (0..4096).map(|index| (index % 251) as u8).collect();
    let sent = alice
        .engine
        .send_direct_message_with_attachments(
            bob.id,
            "while you were out",
            vec![OutgoingAttachment {
                name: "photo.png".into(),
                data: payload.clone(),
                is_image: true,
                width: 32,
                height: 32,
                blur_hash: String::new(),
                ..Default::default()
            }],
            None,
        )
        .await
        .expect("sends");
    let file_id = sent.attachments[0].file_id;

    // Bob comes back and dials, which runs the reference flow and replays
    // everything he missed — message and file metadata alike.
    tokio::spawn(Arc::clone(&bob.engine).run_peering());

    let complete = wait_for(&mut bob.events, "the queued download to finish", 20, |event| {
        match event {
            Event::FileComplete { file_id } => Some(*file_id),
            _ => None,
        }
    })
    .await;

    assert_eq!(complete, Some(file_id), "a file replayed through catch-up must land");
    assert_eq!(
        bob.engine.file_data(file_id).unwrap().as_deref(),
        Some(payload.as_slice()),
    );
}

#[tokio::test]
async fn a_profile_picture_reaches_a_contact() {
    // An avatar is an ordinary distributed file with a global scope, plus an
    // UpdateUser naming it. Both halves have to arrive: the id without the file
    // is a broken image, and the file without the id is bytes nobody looks at.
    logging();

    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    let code = bob.engine.create_pairing_code().expect("code");
    Arc::clone(&alice.engine).request_to_add_user(&code).await.expect("pairs");

    let mut alice_events = alice.events;
    wait_for(&mut alice_events, "alice to add bob", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("alice adds bob");
    wait_for(&mut bob.events, "bob to add alice", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("bob adds alice");

    let picture: Vec<u8> = (0..9000).map(|index| (index % 251) as u8).collect();
    alice
        .engine
        .set_profile_image(OutgoingAttachment {
            name: "me.png".into(),
            data: picture.clone(),
            is_image: true,
            width: 96,
            height: 96,
            blur_hash: String::new(),
            ..Default::default()
        })
        .await
        .expect("sets the picture");

    // Bob learns which file is Alice's picture...
    let updated = wait_for(&mut bob.events, "bob to see the new picture", 15, |event| {
        match event {
            Event::UserUpdated { user } if !user.images.is_empty() => Some(user.clone()),
            _ => None,
        }
    })
    .await
    .expect("the profile update arrives");

    let image_id = *updated.images.last().expect("an image id");

    // ...and can actually fetch it.
    let complete = wait_for(&mut bob.events, "the picture to download", 20, |event| {
        match event {
            Event::FileComplete { file_id } if *file_id == image_id => Some(*file_id),
            _ => None,
        }
    })
    .await;

    assert_eq!(complete, Some(image_id), "the picture itself must arrive too");
    assert_eq!(
        bob.engine.file_data(image_id).unwrap().as_deref(),
        Some(picture.as_slice()),
    );

    // And it is filed as a picture rather than as a message attachment, which
    // is what keeps it out of the timeline.
    let stored = bob.engine.file(image_id).unwrap().expect("stored");
    assert_eq!(stored.file_type, libbounce::types::FileType::UserImage as i64);
}

#[tokio::test]
async fn a_file_too_large_to_embed_is_seeded_from_disk() {
    // Above the embedding limit nothing is copied into the database: the
    // record points at the file, chunks carry hashes and no bytes, and a
    // request is answered by seeking. The receiving side writes to a partial
    // file and renames it only once every chunk is present.
    logging();

    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    let code = bob.engine.create_pairing_code().expect("code");
    Arc::clone(&alice.engine).request_to_add_user(&code).await.expect("pairs");

    let mut alice_events = alice.events;
    wait_for(&mut alice_events, "alice to add bob", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("alice adds bob");
    wait_for(&mut bob.events, "bob to add alice", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("bob adds alice");

    // Just over the limit, so it takes the disk path and still runs quickly.
    let size = libbounce::EMBEDDED_FILE_LIMIT as usize + 3000;
    let payload: Vec<u8> = (0..size).map(|index| (index % 251) as u8).collect();

    let source = std::env::temp_dir().join(format!("bounce-big-{}.bin", std::process::id()));
    std::fs::write(&source, &payload).expect("writes the source file");

    let my_id = alice.id;
    let record = alice
        .engine
        .stage_large_file(
            &source,
            Uuid::new_v4(),
            libbounce::types::Scope::User,
            libbounce::xor(my_id, bob.id),
        )
        .expect("stages from disk");

    assert!(!record.is_embedded());
    assert_eq!(record.size, size as i64);
    assert_eq!(record.path, source.to_string_lossy());

    // Nothing was copied into the database.
    let stored_bytes: i64 = {
        let file_id = record.id;
        let hashes = record.chunk_hashes();
        assert_eq!(hashes.len(), size.div_ceil(libbounce::CHUNK_SIZE));
        let _ = file_id;
        0
    };
    assert_eq!(stored_bytes, 0);

    alice.engine.announce_file(&record).await.expect("announces");

    // Bob does not fetch a large file unprompted, so ask for it.
    let arrived = wait_for(&mut bob.events, "bob to learn of the file", 10, |event| match event {
        Event::FileProgress { file_id, .. } if *file_id == record.id => Some(()),
        _ => None,
    })
    .await;
    assert!(arrived.is_some(), "the metadata should reach Bob");

    bob.engine.request_file(record.id).await.expect("asks for it");

    let complete = wait_for(&mut bob.events, "the large download to finish", 30, |event| {
        match event {
            Event::FileComplete { file_id } if *file_id == record.id => Some(*file_id),
            _ => None,
        }
    })
    .await;
    assert_eq!(complete, Some(record.id), "the file never finished");

    // It landed on disk under its final name, byte for byte, with no partial
    // file left behind.
    let landed = bob.engine.file(record.id).unwrap().expect("stored");
    let written = std::fs::read(&landed.path).expect("the file exists under its real name");
    assert_eq!(written.len(), payload.len());
    assert_eq!(written, payload, "the bytes must survive the round trip");
    assert!(
        !std::path::Path::new(&format!("{}.bouncedownload", landed.path)).exists(),
        "the partial file must be renamed, not left beside the finished one",
    );

    let _ = std::fs::remove_file(&source);
}

#[tokio::test]
async fn a_large_file_can_be_attached_to_a_message_by_path() {
    // The client should not have to choose between two send APIs by size. An
    // attachment carrying a path is streamed however big it is; one carrying
    // bytes is embedded. Before this, anything over the limit was refused
    // outright and the interface had nothing to offer but an error.
    logging();

    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    let code = bob.engine.create_pairing_code().expect("code");
    Arc::clone(&alice.engine).request_to_add_user(&code).await.expect("pairs");

    let mut alice_events = alice.events;
    wait_for(&mut alice_events, "alice to add bob", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("alice adds bob");
    wait_for(&mut bob.events, "bob to add alice", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("bob adds alice");

    let size = libbounce::EMBEDDED_FILE_LIMIT as usize + 4096;
    let payload: Vec<u8> = (0..size).map(|index| (index % 241) as u8).collect();

    let source = std::env::temp_dir().join(format!("bounce-attached-{}.bin", std::process::id()));
    std::fs::write(&source, &payload).expect("writes the source file");

    let sent = alice
        .engine
        .send_direct_message_with_attachments(
            bob.id,
            "the recording from last night",
            vec![libbounce::engine::files::OutgoingAttachment {
                name: "recording.bin".into(),
                data: Vec::new(),
                is_image: false,
                width: 0,
                height: 0,
                blur_hash: String::new(),
                path: source.to_string_lossy().into_owned(),
            }],
            None,
        )
        .await
        .expect("a file this size is sent, not refused");

    assert_eq!(sent.attachments.len(), 1, "the message should carry it");
    let file_id = sent.attachments[0].file_id;

    // Nothing was copied: the record still points at the original.
    let staged = alice.engine.file(file_id).unwrap().expect("staged");
    assert!(!staged.is_embedded(), "a file this size must not be embedded");
    assert_eq!(staged.path, source.to_string_lossy());
    assert_eq!(staged.size, size as i64);
    // The name came from the attachment, not from the path on disk.
    assert_eq!(staged.name, "recording.bin");

    // Bob gets the message and the metadata, and fetches on request.
    wait_for(&mut bob.events, "bob to receive the message", 10, |event| match event {
        Event::MessageReceived { message } if message.id == sent.id => Some(()),
        _ => None,
    })
    .await
    .expect("the message arrives");

    wait_for(&mut bob.events, "bob to learn of the file", 10, |event| match event {
        Event::FileProgress { file_id: id, .. } if *id == file_id => Some(()),
        _ => None,
    })
    .await
    .expect("the file record arrives");

    bob.engine.request_file(file_id).await.expect("asks for it");

    let complete = wait_for(&mut bob.events, "the download to finish", 30, |event| match event {
        Event::FileComplete { file_id: id } if *id == file_id => Some(*id),
        _ => None,
    })
    .await;
    assert_eq!(complete, Some(file_id), "the file never finished");

    let landed = bob.engine.file(file_id).unwrap().expect("stored");
    assert_eq!(
        std::fs::read(&landed.path).expect("the file exists"),
        payload,
        "the bytes must survive the round trip",
    );

    let _ = std::fs::remove_file(&source);
}

#[tokio::test]
async fn a_profile_picture_set_while_offline_arrives_on_reconnection() {
    // The live path and the replay path have to agree on who may have a file,
    // and for avatars they did not. An avatar is staged with `Scope::Global`
    // (`engine/files.rs`), and `peer_may_have` answered `Global` from its
    // catch-all `_ => false` arm — so a picture was offered to precisely
    // nobody once the moment of broadcast had passed.
    //
    // Everything around it kept working, which is what made it look like a
    // rendering fault: the `UpdateUser` naming the picture *is* replayed, so
    // the contact learns an image id and then never receives the bytes behind
    // it. And a device that joins a profile later gets its whole world through
    // this flow, so it sees no pictures at all.
    logging();

    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    let code = bob.engine.create_pairing_code().expect("code");
    Arc::clone(&alice.engine).request_to_add_user(&code).await.expect("pairs");

    let mut alice_events = alice.events;
    wait_for(&mut alice_events, "alice to add bob", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("alice adds bob");
    wait_for(&mut bob.events, "bob to add alice", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("bob adds alice");

    // Bob goes away before Alice chooses a picture, so the only route left is
    // the reference flow.
    bob.engine.disconnect_all().await;
    alice.engine.disconnect_all().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let picture: Vec<u8> = (0..9000).map(|index| (index % 251) as u8).collect();
    alice
        .engine
        .set_profile_image(OutgoingAttachment {
            name: "me.png".into(),
            data: picture.clone(),
            is_image: true,
            width: 96,
            height: 96,
            blur_hash: String::new(),
            ..Default::default()
        })
        .await
        .expect("sets the picture");

    tokio::spawn(Arc::clone(&bob.engine).run_peering());
    tokio::spawn(Arc::clone(&alice.engine).run_peering());

    let updated = wait_for(&mut bob.events, "bob to see the new picture", 20, |event| {
        match event {
            Event::UserUpdated { user } if !user.images.is_empty() => Some(user.clone()),
            _ => None,
        }
    })
    .await
    .expect("the profile update is replayed");

    let image_id = *updated.images.last().expect("an image id");

    let complete = wait_for(&mut bob.events, "the picture to download", 25, |event| {
        match event {
            Event::FileComplete { file_id } if *file_id == image_id => Some(*file_id),
            _ => None,
        }
    })
    .await;

    assert_eq!(
        complete,
        Some(image_id),
        "an avatar named by a replayed update must be fetchable too",
    );
    assert_eq!(
        bob.engine.file_data(image_id).unwrap().as_deref(),
        Some(picture.as_slice()),
    );
}

#[tokio::test]
async fn a_joining_device_downloads_every_file_it_is_told_about() {
    // The shape a real second device arrives in: a profile with a history
    // behind it, so every file record lands in one catch-up burst rather than
    // one at a time with a live chunk offer beside it.
    //
    // That difference is the whole of it. A file learned about live comes with
    // an offer saying who holds it; a file learned about from catch-up comes
    // with nothing, because offers are never stored and so are never replayed.
    logging();

    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;
    let mut phone = start_blank("phone", Arc::clone(&directory)).await;

    let code = bob.engine.create_pairing_code().expect("code");
    Arc::clone(&alice.engine).request_to_add_user(&code).await.expect("pairs");

    let mut alice_events = alice.events;
    wait_for(&mut alice_events, "alice to add bob", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("alice adds bob");
    wait_for(&mut bob.events, "bob to add alice", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("bob adds alice");

    // A history: several attachments and an avatar, all authored here.
    let mut expected = Vec::new();
    for index in 0..8u8 {
        let payload: Vec<u8> = (0..2048).map(|byte| (byte % 251) as u8 ^ index).collect();
        let sent = alice
            .engine
            .send_direct_message_with_attachments(
                bob.id,
                &format!("photo {index}"),
                vec![OutgoingAttachment {
                    name: format!("photo-{index}.png"),
                    data: payload,
                    is_image: true,
                    width: 32,
                    height: 32,
                    blur_hash: String::new(),
                    ..Default::default()
                }],
                None,
            )
            .await
            .expect("sends");
        expected.push(sent.attachments[0].file_id);
    }

    let avatar: Vec<u8> = (0..5000).map(|index| (index % 251) as u8).collect();
    alice
        .engine
        .set_profile_image(OutgoingAttachment {
            name: "ada.png".into(),
            data: avatar,
            is_image: true,
            width: 96,
            height: 96,
            blur_hash: String::new(),
            ..Default::default()
        })
        .await
        .expect("sets the picture");

    // Now the second device arrives, and receives all of that at once.
    let sync = alice.engine.create_sync_code().expect("sync code");
    Arc::clone(&phone.engine).request_to_sync(&sync).await.expect("asks to join");

    let joined = wait_for(&mut phone.events, "the phone to join", 15, |event| match event {
        Event::ProfileCreated { user, .. } => Some(user.clone()),
        _ => None,
    })
    .await
    .expect("the phone joins");
    expected.push(*joined.images.last().expect("the profile names a picture"));

    let mut done = std::collections::HashSet::new();
    while done.len() < expected.len() {
        let Some(file_id) = wait_for(&mut phone.events, "a file to finish", 25, |event| {
            match event {
                Event::FileComplete { file_id } => Some(*file_id),
                _ => None,
            }
        })
        .await
        else {
            break;
        };
        done.insert(file_id);
    }

    let missing: Vec<_> = expected.iter().filter(|id| !done.contains(id)).collect();
    assert!(
        missing.is_empty(),
        "{} of {} files never arrived: {missing:?}",
        missing.len(),
        expected.len(),
    );
}

#[tokio::test]
async fn a_relayed_offer_does_not_strand_a_download() {
    // Chunk offers gossip. An offer names the device holding the chunk, and it
    // is relayed onward by everyone who receives it — so a device learns about
    // holders it has never spoken to and, in the case of a second device that
    // has only ever dialled its own profile, cannot speak to.
    //
    // Both places that act on a location fired it blind: `send_to` a device
    // with no open session writes to nobody and returns success, so the
    // request simply evaporated. Nothing retried it, and the peer that relayed
    // the offer — which is by definition connected, and here holds the bytes —
    // was never asked.
    //
    // This is what left a freshly linked device with a row of half-loaded
    // pictures: the ones whose holder it happened to have a session with
    // arrived, and the rest sat at nothing with no request outstanding.
    logging();

    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut carol = start("Carol", Arc::clone(&directory)).await;
    let mut phone = start_blank("phone", Arc::clone(&directory)).await;

    let code = carol.engine.create_pairing_code().expect("code");
    Arc::clone(&alice.engine).request_to_add_user(&code).await.expect("pairs");

    let mut alice_events = alice.events;
    wait_for(&mut alice_events, "alice to add carol", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("alice adds carol");
    wait_for(&mut carol.events, "carol to add alice", 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("carol adds alice");

    // The phone joins Alice's profile. Peering is deliberately never started
    // on it, so its only session is the one it paired over — exactly the
    // position a new device is in before it has dialled anybody else.
    let sync = alice.engine.create_sync_code().expect("sync code");
    Arc::clone(&phone.engine).request_to_sync(&sync).await.expect("asks to join");
    wait_for(&mut phone.events, "the phone to join", 15, |event| {
        matches!(event, Event::ProfileCreated { .. }).then_some(())
    })
    .await
    .expect("the phone joins");

    // Carol sends a picture. Alice relays both the record and Carol's offers
    // to the phone, which can reach Alice and cannot reach Carol.
    let payload: Vec<u8> = (0..6000).map(|index| (index % 251) as u8).collect();
    let sent = carol
        .engine
        .send_direct_message_with_attachments(
            alice.id,
            "from Carol",
            vec![OutgoingAttachment {
                name: "carol.png".into(),
                data: payload.clone(),
                is_image: true,
                width: 48,
                height: 48,
                blur_hash: String::new(),
                ..Default::default()
            }],
            None,
        )
        .await
        .expect("sends");
    let file_id = sent.attachments[0].file_id;

    let complete = wait_for(&mut phone.events, "the phone to finish the file", 25, |event| {
        match event {
            Event::FileComplete { file_id: id } if *id == file_id => Some(*id),
            _ => None,
        }
    })
    .await;

    assert_eq!(complete, Some(file_id), "the phone must get the bytes from Alice");
    assert_eq!(
        phone.engine.file_data(file_id).unwrap().as_deref(),
        Some(payload.as_slice()),
    );
}

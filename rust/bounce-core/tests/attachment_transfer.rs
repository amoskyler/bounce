//! An image, sent the way the client sends one.
//!
//! `engine_e2e.rs` covers attachments with a hand-written introduction and a
//! hand-written dial. This file instead pairs through the real add-user flow
//! and lets peering open the socket, because that is what the application
//! does — and the application does not work.

use std::sync::Arc;
use std::time::Duration;

use bounce_core::crypto::DeviceKey;
use bounce_core::engine::files::OutgoingAttachment;
use bounce_core::engine::{Engine, Event};
use bounce_core::net::{StaticDirectory, TcpNetwork};
use bounce_core::store::Store;
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
    let store = Arc::new(Store::in_memory().expect("opens"));
    let (engine, events) = Engine::new(key, store, network);
    let user = engine.create_profile(name, "laptop").expect("profile");
    tokio::spawn(Arc::clone(&engine).run_listener());
    Instance { engine, events, id: user.id }
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
    let payload: Vec<u8> = (0..bounce_core::CHUNK_SIZE * 2 + 5000)
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
            }],
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
            }],
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

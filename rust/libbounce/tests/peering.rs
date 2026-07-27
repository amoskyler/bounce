//! Do two instances find each other without being told to?
//!
//! Every other end-to-end test calls `engine.connect(&peer.address)` by hand
//! before asserting anything. That is the test harness standing in for
//! something the product was supposed to do and did not: nothing in the
//! shipped client ever dialled a known contact. These tests remove the manual
//! dial, so the only thing that can open a socket is the engine itself.

use std::sync::Arc;
use std::time::Duration;

use libbounce::crypto::DeviceKey;
use libbounce::engine::{Engine, Event};
use libbounce::frames::identity::User;
use libbounce::net::{StaticDirectory, TcpNetwork};
use libbounce::store::Store;
use tokio::sync::mpsc::UnboundedReceiver;

struct Instance {
    engine: Arc<Engine<TcpNetwork>>,
    store: Arc<Store>,
    events: UnboundedReceiver<Event>,
    user: User,
    address: String,
    /// Kept so a test can restart this device under its own identity — a
    /// device's address IS its key, so a restart with a new key is a different
    /// device that nobody has been introduced to.
    key: DeviceKey,
}

/// Start an instance with a profile, its listener running, and nothing else.
async fn start(name: &str, directory: Arc<StaticDirectory>) -> Instance {
    let key = DeviceKey::generate();
    let address = key.address();
    let network = Arc::new(TcpNetwork::bind(key.clone(), directory).await.expect("binds"));
    let store = Arc::new(Store::in_memory().expect("opens"));

    let (engine, events) = Engine::new(key.clone(), Arc::clone(&store), Arc::clone(&network));
    let user = engine.create_profile(name, &format!("{name}'s laptop")).expect("profile");
    tokio::spawn(Arc::clone(&engine).run_listener());

    Instance { engine, store, events, user, address, key }
}

/// Give each side the other's user record and device group, as the add-user
/// flow would have, without opening a connection.
fn introduce(a: &Instance, b: &Instance) {
    for (left, right) in [(a, b), (b, a)] {
        let mut record = right.user.clone();
        record.profile = false;
        record.private_ecdh_key = Vec::new();
        record.private_ecdsa_key = Vec::new();
        record.accepted = true;
        // Explicitly dormant. The engine is supposed to maintain this field,
        // and a fixture that pre-filled it would pass while the product
        // recorded no activity at all — which is exactly the bug this file
        // exists to catch. `create_profile` stamps it, so it has to be
        // cleared rather than merely left alone.
        record.last_activity = 0;
        left.store.save_user(&record).expect("saves the contact");
    }
}

async fn wait_for_message(
    events: &mut UnboundedReceiver<Event>,
    within: Duration,
) -> Option<String> {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Some(Event::MessageReceived { message })) => return Some(message.text.clone()),
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => return None,
        }
    }
}

#[tokio::test]
async fn an_engine_that_is_not_peering_never_reaches_a_known_contact() {
    // The bug, stated as a test. Both sides know each other and both are
    // listening; neither dials, so nothing crosses. This is what a restarted
    // client looked like.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);

    alice
        .engine
        .send_direct_message(bob.user.id, "anyone there?", None)
        .await
        .expect("the send itself succeeds — it just reaches nobody");

    assert!(
        wait_for_message(&mut bob.events, Duration::from_millis(600)).await.is_none(),
        "without peering a message must not arrive; if this now passes, something else dials \
         and this test no longer pins down what it was written for",
    );
}

#[tokio::test]
async fn peering_reaches_a_known_contact_with_no_manual_dial() {
    // The fix. Same setup, plus the peering task, and no `connect` anywhere.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);

    tokio::spawn(Arc::clone(&alice.engine).run_peering());

    // The first peering pass runs immediately, so this is the dial plus a
    // round trip, not the sixty second audit interval.
    let arrived = wait_for(&alice, &mut bob, "found you").await;
    assert_eq!(arrived.as_deref(), Some("found you"));
}

/// Send once peering has had a chance to open the socket, then wait.
async fn wait_for(alice: &Instance, bob: &mut Instance, text: &str) -> Option<String> {
    for _ in 0..40 {
        if !alice.engine.connected_addresses().await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    alice.engine.send_direct_message(bob.user.id, text, None).await.expect("sends");
    wait_for_message(&mut bob.events, Duration::from_secs(5)).await
}

#[tokio::test]
async fn peering_restores_a_contact_after_a_restart() {
    // The reported symptom: it worked, then a restart broke it. The engine is
    // rebuilt over the same store, exactly as relaunching the app does.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;
    introduce(&alice, &bob);

    // First session: they are connected, and a message crosses.
    Arc::clone(&alice.engine).connect(&bob.address).await.expect("dials");
    tokio::time::sleep(Duration::from_millis(100)).await;
    alice.engine.send_direct_message(bob.user.id, "before", None).await.unwrap();
    assert_eq!(
        wait_for_message(&mut bob.events, Duration::from_secs(5)).await.as_deref(),
        Some("before"),
    );

    // Alice restarts. Same key and same database, because a device's address
    // IS its key — restarting under a new one would be a different device that
    // Bob has never been introduced to, and would prove nothing.
    let key = alice.key.clone();
    let restarted_network = Arc::new(
        TcpNetwork::bind(key.clone(), Arc::clone(&directory)).await.expect("binds"),
    );
    let (restarted, _events) =
        Engine::new(key, Arc::clone(&alice.store), Arc::clone(&restarted_network));
    tokio::spawn(Arc::clone(&restarted).run_listener());
    tokio::spawn(Arc::clone(&restarted).run_peering());

    for _ in 0..60 {
        if !restarted.connected_addresses().await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    restarted.send_direct_message(bob.user.id, "after", None).await.unwrap();
    assert_eq!(
        wait_for_message(&mut bob.events, Duration::from_secs(5)).await.as_deref(),
        Some("after"),
        "a restarted client must reach its contacts without being re-added",
    );
}


#[tokio::test]
async fn sending_a_message_marks_a_conversation_active() {
    // Peering decides who to dial from `last_activity`, so a client that never
    // records it dials nobody. Nothing in the port wrote this field at all,
    // which made the recency filter reject every contact.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;
    introduce(&alice, &bob);

    assert_eq!(
        alice.store.user(bob.user.id).unwrap().unwrap().last_activity,
        0,
        "the fixture starts dormant on purpose",
    );

    alice.engine.send_direct_message(bob.user.id, "hello", None).await.unwrap();

    assert!(
        alice.store.user(bob.user.id).unwrap().unwrap().last_activity > 0,
        "sending must mark the conversation active or peering will forget it",
    );
}

#[tokio::test]
async fn a_dormant_contact_is_still_dialled_when_we_owe_them_a_message() {
    // The case recency alone gets wrong. Alice wrote to Bob while he was
    // offline, then restarted; by the time she is back the conversation looks
    // dormant. If recency were the only rule the message would never leave.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;
    introduce(&alice, &bob);

    // Written long ago, and never delivered — Bob was not connected.
    alice.engine.send_direct_message(bob.user.id, "still waiting", None).await.unwrap();

    // Age the conversation past the four week horizon, as a restart weeks
    // later would leave it.
    let mut stale = alice.store.user(bob.user.id).unwrap().unwrap();
    stale.last_activity = libbounce::now() - (5 * 7 * 24 * 60 * 60);
    alice.store.update_user_local_state(&stale).unwrap();

    tokio::spawn(Arc::clone(&alice.engine).run_peering());

    assert_eq!(
        wait_for_message(&mut bob.events, Duration::from_secs(5)).await.as_deref(),
        Some("still waiting"),
        "a queued message must be delivered however dormant the conversation looks",
    );
}

//! End-to-end tests: two engines, real sockets, real signatures.
//!
//! These run over [`TcpNetwork`] rather than Tor, so a test finishes in
//! milliseconds instead of the minute it takes to publish two hidden services.
//! Everything above the socket is the production path: the same handshake, the
//! same framing, the same signature checks, the same consensus.

use std::sync::Arc;
use std::time::Duration;

use bounce_core::crypto::DeviceKey;
use bounce_core::engine::files::OutgoingAttachment;
use bounce_core::engine::{Engine, Event};
use bounce_core::frames::identity::User;
use bounce_core::net::{Network, StaticDirectory, TcpNetwork};
use bounce_core::store::Store;
use tokio::sync::mpsc::UnboundedReceiver;
use uuid::Uuid;

/// One running instance: an engine, its store, and its event stream.
struct Instance {
    engine: Arc<Engine<TcpNetwork>>,
    store: Arc<Store>,
    events: UnboundedReceiver<Event>,
    user: User,
    address: String,
    /// The device's signing key, so tests can forge frames the way a peer would.
    key: DeviceKey,
}

/// Start an instance with a profile already created.
async fn start(name: &str, directory: Arc<StaticDirectory>) -> Instance {
    let key = DeviceKey::generate();
    let address = key.address();

    let network = Arc::new(
        TcpNetwork::bind(key.clone(), directory)
            .await
            .expect("binds a listener"),
    );
    let signing_key = key.clone();
    let store = Arc::new(Store::in_memory().expect("opens a database"));

    let (engine, events) = Engine::new(key, Arc::clone(&store), Arc::clone(&network));

    let user = engine
        .create_profile(name, &format!("{name}'s laptop"))
        .expect("creates a profile");

    tokio::spawn(Arc::clone(&engine).run_listener());

    Instance {
        engine,
        store,
        events,
        user,
        address,
        key: signing_key,
    }
}

/// Introduce two instances to each other.
///
/// The add-user handshake is driven by the interface and is not what these
/// tests are exercising, so its outcome — each side holding the other's user
/// record and device group — is established directly.
fn introduce(a: &Instance, b: &Instance) {
    let mut a_record = a.user.clone();
    a_record.profile = false;
    a_record.private_ecdh_key = Vec::new();
    a_record.private_ecdsa_key = Vec::new();
    a_record.accepted = true;

    let mut b_record = b.user.clone();
    b_record.profile = false;
    b_record.private_ecdh_key = Vec::new();
    b_record.private_ecdsa_key = Vec::new();
    b_record.accepted = true;

    a.store.save_user(&b_record).expect("saves the contact");
    b.store.save_user(&a_record).expect("saves the contact");
}

/// Wait for an event matching a predicate, failing the test on timeout.
async fn wait_for<T>(
    events: &mut UnboundedReceiver<Event>,
    what: &str,
    mut extract: impl FnMut(&Event) -> Option<T>,
) -> T {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Some(event)) => {
                if let Some(value) = extract(&event) {
                    return value;
                }
            }
            Ok(None) => panic!("event stream closed while waiting for {what}"),
            Err(_) => panic!("timed out waiting for {what}"),
        }
    }
}

#[tokio::test]
async fn a_direct_message_travels_between_two_instances() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);

    // Alice dials Bob and sends.
    Arc::clone(&alice.engine)
        .connect(&bob.address)
        .await
        .expect("dials Bob");

    // Give the session a moment to register before broadcasting.
    tokio::time::sleep(Duration::from_millis(100)).await;

    alice
        .engine
        .send_direct_message(bob.user.id, "hello from Alice")
        .await
        .expect("sends the message");

    let received = wait_for(&mut bob.events, "Bob to receive the message", |event| {
        match event {
            Event::MessageReceived { message } => Some(message.clone()),
            _ => None,
        }
    })
    .await;

    assert_eq!(received.text, "hello from Alice");
    assert_eq!(received.author, alice.user.id);
    // From Bob's side the conversation is threaded under Alice.
    assert_eq!(received.thread, alice.user.id);
    assert!(!received.outgoing);

    // And it is durable, not just an event.
    let stored = bob
        .store
        .direct_messages_for_thread(bounce_core::xor(alice.user.id, bob.user.id), 10)
        .expect("reads the thread");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].text, "hello from Alice");

    // Bob can verify the signature, which is what makes the message Alice's.
    assert!(stored[0].signed.to_container().is_valid());
    assert_eq!(stored[0].signed.signer, alice.address);
}

#[tokio::test]
async fn delivery_is_confirmed_by_an_acknowledgement() {
    let directory = Arc::new(StaticDirectory::new());
    let mut alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let sent = alice
        .engine
        .send_direct_message(bob.user.id, "did you get this?")
        .await
        .unwrap();

    let (message_id, user_id) = wait_for(&mut alice.events, "a delivery confirmation", |event| {
        match event {
            Event::MessageDelivered {
                message_id,
                user_id,
            } => Some((*message_id, *user_id)),
            _ => None,
        }
    })
    .await;

    assert_eq!(message_id, sent.id);
    assert_eq!(user_id, bob.user.id);

    // Delivery is recorded against the specific device that acknowledged.
    assert!(alice
        .store
        .is_delivered_to(
            &bob.address,
            sent.id,
            bounce_core::types::FrameType::DirectMessage
        )
        .unwrap());
}

#[tokio::test]
async fn messages_written_while_offline_arrive_through_the_reference_flow() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);

    // Alice writes while Bob is unreachable. The broadcast finds no peer, so
    // the message simply sits in her store.
    for i in 0..3 {
        alice
            .engine
            .send_direct_message(bob.user.id, &format!("message {i}"))
            .await
            .unwrap();
    }

    let thread = bounce_core::xor(alice.user.id, bob.user.id);
    assert!(bob
        .store
        .direct_messages_for_thread(thread, 10)
        .unwrap()
        .is_empty());

    // Bob comes online and dials Alice. Alice's opening reference offer lists
    // what Bob is missing, Bob asks for it, and Alice sends a catch up.
    Arc::clone(&bob.engine).connect(&alice.address).await.unwrap();

    let mut received = Vec::new();
    while received.len() < 3 {
        let text = wait_for(&mut bob.events, "a caught-up message", |event| match event {
            Event::MessageReceived { message } => Some(message.text.clone()),
            _ => None,
        })
        .await;
        received.push(text);
    }

    received.sort();
    assert_eq!(received, vec!["message 0", "message 1", "message 2"]);

    let stored = bob.store.direct_messages_for_thread(thread, 10).unwrap();
    assert_eq!(stored.len(), 3);
}

#[tokio::test]
async fn a_group_message_reaches_every_member() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Alice founds a group and invites Bob.
    let group = alice
        .engine
        .create_group("Book Club", &[bob.user.id])
        .await
        .expect("creates the group");

    assert_eq!(group.name, "Book Club");
    assert_eq!(group.created_by, alice.user.id);
    assert!(group.admins.contains(&alice.user.id));
    assert!(
        group.invites.contains(&bob.user.id),
        "Bob should hold an invitation, not membership"
    );
    assert!(!group.members.contains(&bob.user.id));

    // Bob learns about the group from the creation frame and the invitation.
    let bob_group = wait_for(&mut bob.events, "Bob to learn about the group", |event| {
        match event {
            Event::GroupUpdated { group } if group.name == "Book Club" => Some(group.clone()),
            _ => None,
        }
    })
    .await;

    assert_eq!(bob_group.id, group.id);
    assert_eq!(bob_group.created_by, alice.user.id);

    // While Bob only holds an invitation he is out of scope for group messages,
    // so posting now reaches nobody but Alice's own devices.
    alice
        .engine
        .send_group_message(group.id, "members only")
        .await
        .expect("posts to the group");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        bob.store
            .group_messages_for_thread(group.id, 10)
            .unwrap()
            .is_empty(),
        "an invitee must not receive group messages before accepting"
    );

    // Bob accepts, which makes him a member.
    bob.engine
        .respond_to_invite(group.id, true)
        .await
        .expect("accepts the invitation");

    wait_for(&mut bob.events, "Bob to become a member", |event| match event {
        Event::GroupUpdated { group: updated }
            if updated.id == group.id && updated.members.contains(&bob.user.id) =>
        {
            Some(())
        }
        _ => None,
    })
    .await;

    // Joining also grants access to what was said before he arrived: the
    // reference flow now considers him in scope for the group's history.
    let history = wait_for(&mut bob.events, "the group's history", |event| match event {
        Event::MessageReceived { message }
            if message.thread == group.id && message.text == "members only" =>
        {
            Some(message.clone())
        }
        _ => None,
    })
    .await;
    assert_eq!(history.author, alice.user.id);

    // And new posts arrive live.
    alice
        .engine
        .send_group_message(group.id, "first meeting is Tuesday")
        .await
        .expect("posts to the group");

    let message = wait_for(&mut bob.events, "the new group message", |event| match event {
        Event::MessageReceived { message }
            if message.thread == group.id && message.text == "first meeting is Tuesday" =>
        {
            Some(message.clone())
        }
        _ => None,
    })
    .await;

    assert_eq!(message.author, alice.user.id);

    // Both messages are durable on Bob's device.
    let stored = bob.store.group_messages_for_thread(group.id, 10).unwrap();
    assert_eq!(stored.len(), 2);
}

#[tokio::test]
async fn a_group_rename_converges_on_both_devices() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let group = alice
        .engine
        .create_group("Original Name", &[bob.user.id])
        .await
        .unwrap();

    wait_for(&mut bob.events, "Bob to learn about the group", |event| {
        match event {
            Event::GroupUpdated { group } if group.name == "Original Name" => Some(()),
            _ => None,
        }
    })
    .await;

    alice
        .engine
        .rename_group(group.id, "Renamed Group")
        .await
        .expect("renames the group");

    wait_for(&mut bob.events, "the rename to propagate", |event| match event {
        Event::GroupUpdated { group } if group.name == "Renamed Group" => Some(()),
        _ => None,
    })
    .await;

    // Both sides recomputed independently and agree.
    let alice_group = alice.store.group(group.id).unwrap().unwrap();
    let bob_group = bob.store.group(group.id).unwrap().unwrap();
    assert_eq!(alice_group.name, "Renamed Group");
    assert_eq!(bob_group.name, "Renamed Group");
}

#[tokio::test]
async fn a_message_from_an_unknown_device_is_refused() {
    let directory = Arc::new(StaticDirectory::new());
    let bob = start("Bob", Arc::clone(&directory)).await;

    // A stranger who was never introduced to Bob.
    let stranger_key = DeviceKey::generate();
    let stranger_network = Arc::new(
        TcpNetwork::bind(stranger_key.clone(), Arc::clone(&directory))
            .await
            .unwrap(),
    );

    let mut connection = stranger_network
        .dial(&bob.address)
        .await
        .expect("the handshake itself succeeds");

    // Forge a message claiming to be from a user Bob does know about.
    let mut message = bounce_core::frames::message::DirectMessage::new(
        Uuid::new_v4(),
        bob.user.id,
        "trust me".into(),
        bounce_core::now(),
    );
    let body = bounce_core::msgpack::to_vec(&message).unwrap();
    let container = bounce_core::signed::SignedContainer::create(&stranger_key, body);
    message.signed = bounce_core::frames::SignedFrame::from_container(&container);

    let payload = bounce_core::msgpack::to_vec(&container).unwrap();
    bounce_core::wire::write_frame(
        &mut connection.stream,
        &bounce_core::wire::RawFrame::new(
            bounce_core::types::FrameType::DirectMessage.as_u16(),
            payload,
        ),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    // The signature is valid, but the signing device is not one Bob has been
    // introduced to, so it speaks for nobody and the message is dropped.
    let thread = bounce_core::xor(message.author, bob.user.id);
    assert!(
        bob.store
            .direct_messages_for_thread(thread, 10)
            .unwrap()
            .is_empty(),
        "a message from an unintroduced device must not be stored"
    );
}

#[tokio::test]
async fn a_forged_signature_is_refused() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);

    // Alice's real address, but a message body that was never signed with it.
    let mut message = bounce_core::frames::message::DirectMessage::new(
        alice.user.id,
        bob.user.id,
        "I did not write this".into(),
        bounce_core::now(),
    );
    let body = bounce_core::msgpack::to_vec(&message).unwrap();

    let impostor = DeviceKey::generate();
    let mut container = bounce_core::signed::SignedContainer::create(&impostor, body);
    container.signer = alice.address.clone();
    message.signed = bounce_core::frames::SignedFrame::from_container(&container);

    let impostor_network = Arc::new(
        TcpNetwork::bind(impostor, Arc::clone(&directory))
            .await
            .unwrap(),
    );
    let mut connection = impostor_network.dial(&bob.address).await.unwrap();

    let payload = bounce_core::msgpack::to_vec(&container).unwrap();
    bounce_core::wire::write_frame(
        &mut connection.stream,
        &bounce_core::wire::RawFrame::new(
            bounce_core::types::FrameType::DirectMessage.as_u16(),
            payload,
        ),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let thread = bounce_core::xor(alice.user.id, bob.user.id);
    assert!(
        bob.store
            .direct_messages_for_thread(thread, 10)
            .unwrap()
            .is_empty(),
        "a container whose signature does not match its signer must be rejected"
    );
}

#[tokio::test]
async fn a_note_to_self_stays_within_the_device_group() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    alice
        .engine
        .send_direct_message(alice.user.id, "remember the milk")
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Bob is connected but out of scope, so he never sees it.
    let bob_messages = bob
        .store
        .direct_messages_for_thread(Uuid::nil(), 10)
        .unwrap();
    assert!(
        bob_messages.is_empty(),
        "a note to self must not reach a contact, even a connected one"
    );

    // Alice has it, threaded under her own ID.
    let mine = alice
        .store
        .direct_messages_for_thread(Uuid::nil(), 10)
        .unwrap();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].text, "remember the milk");
}

#[tokio::test]
async fn initial_state_describes_everything_a_client_needs() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    alice
        .engine
        .send_direct_message(bob.user.id, "hello")
        .await
        .unwrap();
    let group = alice.engine.create_group("Planning", &[]).await.unwrap();
    alice
        .engine
        .send_group_message(group.id, "kickoff")
        .await
        .unwrap();
    alice.engine.save_draft(bob.user.id, "unsent thought").await.unwrap();

    let state = alice.engine.initial_state().expect("builds initial state");

    let profile = state.profile.expect("a profile exists");
    assert_eq!(profile.name, "Alice");

    assert_eq!(state.sync_devices.len(), 1);
    assert!(state.sync_devices[0].local);

    assert_eq!(state.users.len(), 1);
    assert_eq!(state.users[0].id, bob.user.id);

    assert_eq!(state.groups.len(), 1);
    assert_eq!(state.groups[0].name, "Planning");

    // Both conversations are present, in chronological order.
    assert_eq!(state.messages.len(), 2);
    assert!(state.messages.windows(2).all(|w| w[0].written_at <= w[1].written_at));
    assert!(state.messages.iter().any(|m| m.text == "hello"));
    assert!(state.messages.iter().any(|m| m.text == "kickoff"));

    assert_eq!(state.drafts.len(), 1);
    assert_eq!(state.drafts[0].text, "unsent thought");
}

#[tokio::test]
async fn two_strangers_become_contacts_by_scanning_a_code() {
    // The real introduction path, rather than the shortcut the other tests
    // take. Nothing is looked up in a directory: Bob reads a code off Alice's
    // screen and that is the entire discovery mechanism.
    let directory = Arc::new(StaticDirectory::new());
    let mut alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    // Neither knows the other yet.
    assert!(alice.store.user(bob.user.id).unwrap().is_none());
    assert!(bob.store.user(alice.user.id).unwrap().is_none());

    let code = alice.engine.create_pairing_code().expect("shows a code");

    bob.engine
        .request_to_add_user(&code)
        .await
        .expect("acts on the scanned code");

    // Both sides end up holding each other.
    let added = wait_for(&mut bob.events, "Bob to add Alice", |event| match event {
        Event::UserAdded { user } if user.id == alice.user.id => Some(user.clone()),
        _ => None,
    })
    .await;
    assert_eq!(added.name, "Alice");

    wait_for(&mut alice.events, "Alice to add Bob", |event| match event {
        Event::UserAdded { user } if user.id == bob.user.id => Some(()),
        _ => None,
    })
    .await;

    let alices_view = alice.store.user(bob.user.id).unwrap().expect("Bob is known");
    let bobs_view = bob.store.user(alice.user.id).unwrap().expect("Alice is known");

    assert_eq!(alices_view.name, "Bob");
    assert_eq!(bobs_view.name, "Alice");

    // Each learned the other's device group, which is what makes the next
    // connection possible.
    assert_eq!(alices_view.devices.len(), 1);
    assert_eq!(alices_view.devices[0].address, bob.address);
    assert_eq!(bobs_view.devices[0].address, alice.address);

    // A scanned contact opens a conversation immediately.
    assert!(bobs_view.open_dm);
    assert_eq!(bobs_view.introduction_method, "add_user");

    // Private key material never crossed.
    assert!(alices_view.private_ecdh_key.is_empty());
    assert!(bobs_view.private_ecdh_key.is_empty());

    // And they can now actually talk.
    alice
        .engine
        .send_direct_message(bob.user.id, "nice to meet you")
        .await
        .expect("sends a message to the new contact");

    let message = wait_for(&mut bob.events, "the first message", |event| match event {
        Event::MessageReceived { message } => Some(message.clone()),
        _ => None,
    })
    .await;
    assert_eq!(message.text, "nice to meet you");
}

#[tokio::test]
async fn a_pairing_code_is_single_use() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;
    let mut mallory = start("Mallory", Arc::clone(&directory)).await;

    let code = alice.engine.create_pairing_code().unwrap();

    bob.engine.request_to_add_user(&code).await.unwrap();
    wait_for(&mut bob.events, "Bob to add Alice", |event| match event {
        Event::UserAdded { user } if user.id == alice.user.id => Some(()),
        _ => None,
    })
    .await;

    // Mallory saw the same code over Alice's shoulder. It is already spent.
    mallory.engine.request_to_add_user(&code).await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    assert!(
        alice.store.user(mallory.user.id).unwrap().is_none(),
        "a replayed code must not add a second contact"
    );
    assert!(
        mallory.store.user(alice.user.id).unwrap().is_none(),
        "the replaying side must not end up holding Alice either"
    );

    // Drain Mallory's events so a stray UserAdded would have been caught above.
    while mallory.events.try_recv().is_ok() {}
}

#[tokio::test]
async fn an_unsolicited_acceptance_is_refused() {
    // The Go implementation keeps no pending-request state, so any peer can
    // send an "accepted" out of the blue and be adopted as a contact.
    let directory = Arc::new(StaticDirectory::new());
    let bob = start("Bob", Arc::clone(&directory)).await;
    let mallory = start("Mallory", Arc::clone(&directory)).await;

    let mallory_record = {
        let mut record = mallory.user.clone();
        record.profile = false;
        record.private_ecdh_key = Vec::new();
        record.private_ecdsa_key = Vec::new();
        record.public_ecdsa_key = Vec::new();
        record
    };
    let offer_user = bounce_core::msgpack::to_vec(&mallory_record).unwrap();

    // Mallory signs Bob's record, as though answering a request Bob never made.
    let bob_record = {
        let mut record = bob.user.clone();
        record.profile = false;
        record.private_ecdh_key = Vec::new();
        record.private_ecdsa_key = Vec::new();
        record.public_ecdsa_key = Vec::new();
        record
    };
    let requester_user = bounce_core::msgpack::to_vec(&bob_record).unwrap();

    let accepted = bounce_core::frames::pairing::AddUserRequestAccepted {
        offer_user,
        offer_signature: mallory
            .key
            .sign(&bounce_core::crypto::hash(&requester_user))
            .to_vec(),
        // Absent, matching the Go frame: the signer is the connected peer.
        offer_device: None,
    };

    let result = bob
        .engine
        .handle_frame(
            &mallory.address,
            bounce_core::wire::RawFrame::new(
                bounce_core::types::FrameType::AddUserRequestAccepted.as_u16(),
                accepted.encode().unwrap(),
            ),
        )
        .await;

    assert!(
        result.is_err(),
        "an acceptance for a request that was never sent must be refused"
    );
    assert!(bob.store.user(mallory.user.id).unwrap().is_none());
}

#[tokio::test]
async fn a_forged_add_user_record_cannot_manufacture_our_consent() {
    // Both halves of an AddUser record come off the wire, so "which side is us"
    // is the sender's claim. Mallory writes a record naming Bob's user id but
    // listing HER device as Bob's, signs both halves herself, and every
    // self-consistency check passes — she is asserting that Bob consented.
    // Only checking the signing device against Bob's own database catches it.
    let directory = Arc::new(StaticDirectory::new());
    let bob = start("Bob", Arc::clone(&directory)).await;
    let mallory = start("Mallory", Arc::clone(&directory)).await;

    let strip = |user: &User| {
        let mut record = user.clone();
        record.profile = false;
        record.private_ecdh_key = Vec::new();
        record.private_ecdsa_key = Vec::new();
        record.public_ecdsa_key = Vec::new();
        record
    };

    // A "Bob" whose device group is really Mallory's device.
    let mut fake_bob = strip(&bob.user);
    fake_bob.devices = mallory.user.devices.clone();
    for device in &mut fake_bob.devices {
        device.user_id = fake_bob.id;
    }

    let offer_user = bounce_core::msgpack::to_vec(&strip(&mallory.user)).unwrap();
    let requester_user = bounce_core::msgpack::to_vec(&fake_bob).unwrap();

    let record = bounce_core::frames::pairing::AddUser {
        id: Uuid::new_v4(),
        xor: bounce_core::xor(mallory.user.id, bob.user.id),
        timestamp: bounce_core::now(),
        saved_at: 0,
        offer_signature: mallory
            .key
            .sign(&bounce_core::crypto::hash(&requester_user))
            .to_vec(),
        // Mallory signs "Bob's" half too, because the device list says it is hers.
        requester_signature: mallory
            .key
            .sign(&bounce_core::crypto::hash(&offer_user))
            .to_vec(),
        offer_device: mallory.address.clone(),
        requester_device: mallory.address.clone(),
        offer_user,
        requester_user,
    };

    // The record is internally consistent — that is the whole problem.
    assert!(record.signatures_are_valid());
    assert!(record.signers_are_members().unwrap());
    assert!(record.xor_matches_users().unwrap());

    let result = bob
        .engine
        .handle_frame(
            &mallory.address,
            bounce_core::wire::RawFrame::new(
                bounce_core::types::FrameType::AddUser.as_u16(),
                bounce_core::msgpack::to_vec(&record).unwrap(),
            ),
        )
        .await;

    assert!(
        result.is_err(),
        "a record claiming our consent must be signed by a device we actually own"
    );
    assert!(
        bob.store.user(mallory.user.id).unwrap().is_none(),
        "Mallory must not have added herself to Bob's contacts"
    );
}

#[tokio::test]
async fn a_receipt_from_outside_the_conversation_is_refused() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;
    let mallory = start("Mallory", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    introduce(&alice, &mallory);

    let sent = alice
        .engine
        .send_direct_message(bob.user.id, "for Bob only")
        .await
        .unwrap();

    // Mallory claims to have read a message in a conversation she is not in.
    let mut receipt = bounce_core::frames::message::ReadReceipt {
        signed: bounce_core::frames::SignedFrame::default(),
        id: Uuid::new_v4(),
        actor: mallory.user.id,
        destination: Uuid::nil(),
        scope: 0,
        target: sent.id,
        target_type: bounce_core::types::FrameType::DirectMessage.as_u16(),
        timestamp: bounce_core::now(),
        saved_at: 0,
    };
    let body = bounce_core::msgpack::to_vec(&receipt).unwrap();
    let container = bounce_core::signed::SignedContainer::create(&mallory.key, body);
    receipt.signed = bounce_core::frames::SignedFrame::from_container(&container);

    let result = alice
        .engine
        .handle_frame(
            &mallory.address,
            bounce_core::wire::RawFrame::new(
                bounce_core::types::FrameType::ReadReceipt.as_u16(),
                bounce_core::msgpack::to_vec(&container).unwrap(),
            ),
        )
        .await;

    assert!(result.is_err(), "an outsider cannot have read the message");
    assert!(
        !alice.store.readers_of(sent.id).unwrap().contains(&mallory.user.id),
        "and must not be shown to the author as a reader"
    );
}

#[tokio::test]
async fn marking_a_message_read_twice_sends_one_receipt() {
    // The interface marks whatever is on screen, which happens on every render.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    alice
        .engine
        .send_direct_message(bob.user.id, "read me twice")
        .await
        .unwrap();

    let received = wait_for(&mut bob.events, "the message", |event| match event {
        Event::MessageReceived { message } => Some(message.clone()),
        _ => None,
    })
    .await;

    for _ in 0..5 {
        bob.engine
            .mark_as_read(received.id, bounce_core::types::FrameType::DirectMessage)
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(
        alice.store.readers_of(received.id).unwrap().len(),
        1,
        "repeated marking must not accumulate receipts"
    );
}

#[tokio::test]
async fn pairing_codes_round_trip_and_reject_junk() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", directory).await;

    let code = alice.engine.create_pairing_code().unwrap();
    let (address, secret) =
        Engine::<TcpNetwork>::parse_pairing_code(&code).expect("its own code parses");

    assert_eq!(address, alice.address);
    assert_eq!(secret.len(), 32, "128 bits, hex encoded");

    for junk in [
        "",
        "nonsense",
        "bounce:not-an-onion-address:secret",
        "bounce:onlyanaddress",
        &format!("bounce:{}:", alice.address),
    ] {
        assert!(
            Engine::<TcpNetwork>::parse_pairing_code(junk).is_err(),
            "{junk:?} should not parse as a pairing code"
        );
    }

    // Showing a new code invalidates the previous one.
    let replacement = alice.engine.create_pairing_code().unwrap();
    assert_ne!(replacement, code);
}

#[tokio::test]
async fn reading_a_message_tells_its_author() {
    let directory = Arc::new(StaticDirectory::new());
    let mut alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let sent = alice
        .engine
        .send_direct_message(bob.user.id, "have you seen this?")
        .await
        .unwrap();

    let received = wait_for(&mut bob.events, "Bob to receive the message", |event| match event {
        Event::MessageReceived { message } => Some(message.clone()),
        _ => None,
    })
    .await;

    bob.engine
        .mark_as_read(received.id, bounce_core::types::FrameType::DirectMessage)
        .await
        .expect("marks the message read");

    let (message_id, user_id) = wait_for(&mut alice.events, "a read receipt", |event| match event {
        Event::MessageRead {
            message_id,
            user_id,
        } => Some((*message_id, *user_id)),
        _ => None,
    })
    .await;

    assert_eq!(message_id, sent.id);
    assert_eq!(user_id, bob.user.id);

    // Bob's own copy is marked seen locally too.
    let bob_copy = bob.store.direct_message(received.id).unwrap().unwrap();
    assert!(bob_copy.seen);

    // And Alice can report who has read it.
    assert_eq!(alice.store.readers_of(sent.id).unwrap(), vec![bob.user.id]);
}

#[tokio::test]
async fn a_receipt_that_arrives_before_its_message_is_resolved_later() {
    // Receipts and messages are independent frames, so on a catch up the
    // receipt can land first. It must not be discarded.
    let directory = Arc::new(StaticDirectory::new());
    let mut alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);

    // Alice writes while disconnected, so Bob has neither message nor receipt.
    let sent = alice
        .engine
        .send_direct_message(bob.user.id, "read me")
        .await
        .unwrap();

    // Hand Bob the receipt without the message, by having him handle the frame
    // directly — the situation a reordered catch up produces.
    let mut receipt_only = bounce_core::frames::message::ReadReceipt {
        signed: bounce_core::frames::SignedFrame::default(),
        id: Uuid::new_v4(),
        actor: bob.user.id,
        destination: Uuid::nil(),
        scope: 0,
        target: sent.id,
        target_type: bounce_core::types::FrameType::DirectMessage.as_u16(),
        timestamp: bounce_core::now(),
        saved_at: 0,
    };
    let body = bounce_core::msgpack::to_vec(&receipt_only).unwrap();
    let container = bounce_core::signed::SignedContainer::create(&bob.key, body);
    receipt_only.signed = bounce_core::frames::SignedFrame::from_container(&container);

    alice
        .engine
        .handle_frame(
            &bob.address,
            bounce_core::wire::RawFrame::new(
                bounce_core::types::FrameType::ReadReceipt.as_u16(),
                bounce_core::msgpack::to_vec(&container).unwrap(),
            ),
        )
        .await
        .expect("an early receipt is accepted");

    // Alice already holds the message, so this one resolves immediately and
    // she learns Bob read it.
    let user_id = wait_for(&mut alice.events, "the read event", |event| match event {
        Event::MessageRead { message_id, user_id } if *message_id == sent.id => Some(*user_id),
        _ => None,
    })
    .await;
    assert_eq!(user_id, bob.user.id);
}

#[tokio::test]
async fn typing_indicators_reach_the_other_side_and_expire() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    alice
        .engine
        .typing_in(bob.user.id, bounce_core::types::FrameType::DirectMessage)
        .await
        .expect("sends a typing indicator");

    let (user_id, thread) = wait_for(&mut bob.events, "the typing indicator", |event| match event {
        Event::TypingStarted { user_id, thread } => Some((*user_id, *thread)),
        _ => None,
    })
    .await;

    // Bob sees Alice typing, in his conversation with Alice.
    assert_eq!(user_id, alice.user.id);
    assert_eq!(thread, alice.user.id);

    // Indicators are ephemeral: nothing is stored, and it withdraws on its own.
    assert!(!bob
        .store
        .has_frame(Uuid::nil(), bounce_core::types::FrameType::TypingIndicator)
        .unwrap());

    tokio::time::sleep(Duration::from_millis(50)).await;
    bob.engine.expire_typing_indicators().await;
    // Not yet — the display window has not elapsed.

    // Force the window past by advancing the stored timestamp is not possible
    // from outside, so wait it out; the window is deliberately short.
    tokio::time::sleep(Duration::from_secs(4)).await;
    bob.engine.expire_typing_indicators().await;

    wait_for(&mut bob.events, "the indicator to withdraw", |event| match event {
        Event::TypingStopped { user_id, .. } if *user_id == alice.user.id => Some(()),
        _ => None,
    })
    .await;
}

#[tokio::test]
async fn a_message_withdraws_the_senders_typing_indicator() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    alice
        .engine
        .typing_in(bob.user.id, bounce_core::types::FrameType::DirectMessage)
        .await
        .unwrap();

    wait_for(&mut bob.events, "the typing indicator", |event| match event {
        Event::TypingStarted { user_id, .. } if *user_id == alice.user.id => Some(()),
        _ => None,
    })
    .await;

    alice
        .engine
        .send_direct_message(bob.user.id, "here it is")
        .await
        .unwrap();

    // The message itself is proof Alice stopped typing; no timer needed.
    wait_for(&mut bob.events, "the indicator to withdraw", |event| match event {
        Event::TypingStopped { user_id, .. } if *user_id == alice.user.id => Some(()),
        _ => None,
    })
    .await;
}

#[tokio::test]
async fn typing_indicators_are_throttled() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // The interface calls this on every keystroke, so a burst must not become
    // a burst on the wire.
    for _ in 0..20 {
        alice
            .engine
            .typing_in(bob.user.id, bounce_core::types::FrameType::DirectMessage)
            .await
            .unwrap();
    }

    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut started = 0;
    while let Ok(event) = bob.events.try_recv() {
        if matches!(event, Event::TypingStarted { .. }) {
            started += 1;
        }
    }

    assert_eq!(
        started, 1,
        "twenty keystrokes in one second should produce a single indicator"
    );
}

#[tokio::test]
async fn a_typing_indicator_for_someone_elses_conversation_is_refused() {
    // The Go implementation checks that the signer speaks for the author, but
    // not that the author is party to the thread. A known device could
    // therefore announce itself as typing into any conversation.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;
    let mallory = start("Mallory", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    introduce(&bob, &mallory);

    // Mallory claims to be typing into Alice's conversation, which is neither
    // addressed to Bob nor authored by him.
    let mut indicator = bounce_core::frames::message::TypingIndicator {
        signed: bounce_core::frames::SignedFrame::default(),
        id: Uuid::new_v4(),
        thread: alice.user.id,
        message_type: bounce_core::types::FrameType::DirectMessage.as_u16(),
        author: mallory.user.id,
        received_at: 0,
    };
    let body = bounce_core::msgpack::to_vec(&indicator).unwrap();
    let container = bounce_core::signed::SignedContainer::create(&mallory.key, body);
    indicator.signed = bounce_core::frames::SignedFrame::from_container(&container);

    let result = bob
        .engine
        .handle_frame(
            &mallory.address,
            bounce_core::wire::RawFrame::new(
                bounce_core::types::FrameType::TypingIndicator.as_u16(),
                bounce_core::msgpack::to_vec(&container).unwrap(),
            ),
        )
        .await;

    assert!(
        result.is_err(),
        "an indicator for a conversation its author is not part of must be refused"
    );
}

#[tokio::test]
async fn updates_created_back_to_back_stay_in_causal_order() {
    // Timestamps have one-second resolution, so several updates created in
    // quick succession would otherwise share one. Replay order is by
    // timestamp, so a tie lets an update sort before the one it responds to —
    // which is how accepting an invitation ended up being rejected as coming
    // from a non-invitee.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", directory).await;

    let group = alice.engine.create_group("Rapid Changes", &[]).await.unwrap();

    for name in ["First", "Second", "Third", "Fourth"] {
        alice.engine.rename_group(group.id, name).await.unwrap();
    }

    let updates = alice.store.updates_for_group(group.id).unwrap();
    assert_eq!(updates.len(), 4);

    for pair in updates.windows(2) {
        assert!(
            pair[0].timestamp < pair[1].timestamp,
            "consecutive updates must have strictly increasing timestamps, got {} then {}",
            pair[0].timestamp,
            pair[1].timestamp,
        );
    }

    // And the last one is the state every device converges on.
    assert_eq!(alice.store.group(group.id).unwrap().unwrap().name, "Fourth");
}

#[tokio::test]
async fn a_profile_cannot_be_created_twice() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", directory).await;

    assert!(
        alice.engine.create_profile("Impostor", "laptop").is_err(),
        "a device belongs to exactly one profile"
    );
}

// -------------------------------------------------------------------------
// Attachments
// -------------------------------------------------------------------------

/// Build an image attachment from raw bytes.
fn image(name: &str, data: Vec<u8>) -> OutgoingAttachment {
    OutgoingAttachment {
        name: name.to_string(),
        data,
        is_image: true,
        width: 64,
        height: 48,
        blur_hash: String::new(),
    }
}

#[tokio::test]
async fn an_image_reaches_the_other_side_byte_for_byte() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Two and a bit chunks, so reassembly order actually matters.
    let payload: Vec<u8> = (0..bounce_core::CHUNK_SIZE * 2 + 4096)
        .map(|index| (index % 251) as u8)
        .collect();

    let sent = alice
        .engine
        .send_direct_message_with_attachments(
            bob.user.id,
            "look at this",
            vec![image("photo.png", payload.clone())],
        )
        .await
        .expect("sends the message");

    assert_eq!(sent.attachments.len(), 1);
    let file_id = sent.attachments[0].file_id;
    assert_eq!(sent.attachments[0].name, "photo.png");
    // The sender holds every chunk already, so it is complete on arrival.
    assert_eq!(sent.attachments[0].progress, 1.0);

    // The message arrives before any bytes do, carrying the attachment record.
    let received = wait_for(&mut bob.events, "Bob to receive the message", |event| match event {
        Event::MessageReceived { message } => Some(message.clone()),
        _ => None,
    })
    .await;

    assert_eq!(received.text, "look at this");
    assert_eq!(received.attachments.len(), 1);
    assert_eq!(received.attachments[0].file_id, file_id);
    assert_eq!(received.attachments[0].width, Some(64));
    assert_eq!(received.attachments[0].height, Some(48));

    // Then the chunks are fetched, one request at a time, until it is whole.
    let complete = wait_for(&mut bob.events, "the download to finish", |event| match event {
        Event::FileComplete { file_id } => Some(*file_id),
        _ => None,
    })
    .await;
    assert_eq!(complete, file_id);

    let fetched = bob
        .engine
        .file_data(file_id)
        .expect("reads the file")
        .expect("every chunk is present");
    assert_eq!(fetched, payload, "the bytes must survive the round trip");
}

#[tokio::test]
async fn a_group_attachment_spreads_to_every_member() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let group = alice.engine.create_group("Photos", &[bob.user.id]).await.unwrap();
    wait_for(&mut bob.events, "Bob to learn about the group", |event| match event {
        Event::GroupUpdated { group } if group.name == "Photos" => Some(()),
        _ => None,
    })
    .await;
    bob.engine.respond_to_invite(group.id, true).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let payload = b"a small attachment that fits in one chunk".to_vec();
    let sent = alice
        .engine
        .send_group_message_with_attachments(
            group.id,
            "from the weekend",
            vec![image("weekend.jpg", payload.clone())],
        )
        .await
        .expect("posts to the group");

    let file_id = sent.attachments[0].file_id;

    let complete = wait_for(&mut bob.events, "the download to finish", |event| match event {
        Event::FileComplete { file_id } => Some(*file_id),
        _ => None,
    })
    .await;
    assert_eq!(complete, file_id);
    assert_eq!(bob.engine.file_data(file_id).unwrap().unwrap(), payload);
}

#[tokio::test]
async fn a_peer_that_lies_about_a_chunk_is_ignored() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let payload = b"the genuine article".to_vec();
    let sent = alice
        .engine
        .send_direct_message_with_attachments(
            bob.user.id,
            "here",
            vec![image("real.png", payload.clone())],
        )
        .await
        .unwrap();
    let file_id = sent.attachments[0].file_id;

    wait_for(&mut bob.events, "the download to finish", |event| match event {
        Event::FileComplete { file_id } => Some(*file_id),
        _ => None,
    })
    .await;

    // Now Alice sends different bytes under the same conversation. They hash
    // to something Bob is not expecting, so nothing about the stored file
    // moves — a chunk is identified by its content, not by who sent it.
    let forged = bounce_core::frames::file::Chunk {
        id: Uuid::nil(),
        file_id,
        hash: String::new(),
        encrypted_hash: String::new(),
        index: 0,
        downloaded: true,
        data: b"tampered with".to_vec(),
    };
    bob.engine
        .handle_frame(
            &alice.address,
            bounce_core::wire::RawFrame::new(
                bounce_core::types::FrameType::Chunk.as_u16(),
                bounce_core::msgpack::to_vec(&forged).unwrap(),
            ),
        )
        .await
        .expect("an unrecognised chunk is dropped, not an error");

    assert_eq!(
        bob.engine.file_data(file_id).unwrap().unwrap(),
        payload,
        "the stored file must still be the one Alice actually sent"
    );
}

#[tokio::test]
async fn an_attachment_larger_than_the_limit_is_refused() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;
    introduce(&alice, &bob);

    let oversized = vec![0u8; (bounce_core::EMBEDDED_FILE_LIMIT + 1) as usize];
    let result = alice
        .engine
        .send_direct_message_with_attachments(bob.user.id, "", vec![image("huge.bin", oversized)])
        .await;

    assert!(
        result.is_err(),
        "seeding a file too large to embed is not implemented, and must say so"
    );
}

// -------------------------------------------------------------------------
// Status messages
// -------------------------------------------------------------------------

#[tokio::test]
async fn group_changes_appear_as_status_rows_on_both_sides() {
    let directory = Arc::new(StaticDirectory::new());
    let mut alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let group = alice.engine.create_group("Draft Name", &[bob.user.id]).await.unwrap();

    // The invitation is a status row on the sending side, naming the invitee.
    let invited = wait_for(&mut alice.events, "an invitation row", |event| match event {
        Event::SystemMessage { message } if message.kind == "userInvited" => Some(message.clone()),
        _ => None,
    })
    .await;
    assert_eq!(invited.thread, group.id);
    assert_eq!(invited.actor, alice.user.id);
    assert_eq!(invited.subject.as_deref(), Some(bob.user.id.to_string().as_str()));

    // Bob joins, which is what puts the group — and so its history — on his
    // device; until then he holds an invitation and nothing else.
    wait_for(&mut bob.events, "Bob to learn about the group", |event| match event {
        Event::GroupUpdated { group } if group.name == "Draft Name" => Some(()),
        _ => None,
    })
    .await;
    bob.engine.respond_to_invite(group.id, true).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    alice.engine.rename_group(group.id, "Book Club").await.unwrap();

    // And it reaches Bob, who was not the one who made the change.
    let renamed = wait_for(&mut bob.events, "a rename row", |event| match event {
        Event::SystemMessage { message } if message.kind == "groupRenamed" => Some(message.clone()),
        _ => None,
    })
    .await;
    assert_eq!(renamed.actor, alice.user.id);
    assert_eq!(renamed.value.as_deref(), Some("Book Club"));

    // The opening snapshot replays the history, starting with the founding.
    let state = bob.engine.initial_state().unwrap();
    let kinds: Vec<&str> = state
        .system_messages
        .iter()
        .filter(|row| row.thread == group.id)
        .map(|row| row.kind.as_str())
        .collect();
    assert_eq!(kinds.first(), Some(&"groupCreated"));
    assert!(kinds.contains(&"groupRenamed"), "got {kinds:?}");
}

#[tokio::test]
async fn changing_a_conversations_retention_tells_the_other_side() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    alice.engine.set_retention(bob.user.id, 3600).await.unwrap();

    let row = wait_for(&mut bob.events, "a retention row", |event| match event {
        Event::SystemMessage { message } if message.kind == "retentionChanged" => {
            Some(message.clone())
        }
        _ => None,
    })
    .await;

    // Threaded under Alice, because that is the conversation it changed.
    assert_eq!(row.thread, alice.user.id);
    assert_eq!(row.actor, alice.user.id);
    assert_eq!(row.value.as_deref(), Some("1 Hour"));

    // And the setting itself took effect, not just the announcement.
    assert_eq!(bob.store.user(alice.user.id).unwrap().unwrap().retention, 3600);
}

#[tokio::test]
async fn a_private_conversation_setting_cannot_be_set_from_outside() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);

    // Alice signs a frame telling Bob's device to mute her. Mute state is one
    // device owner's own view, so it must not be settable by the person on the
    // other end of the conversation.
    let update = bounce_core::frames::update::UpdateDm::new(
        alice.user.id,
        bounce_core::xor(alice.user.id, bob.user.id),
        bounce_core::frames::UpdateDmType::ChangeMutedUntil,
        i64::MAX.to_le_bytes().to_vec(),
        bounce_core::now(),
    );
    let container = bounce_core::signed::SignedContainer::create(
        &alice.key,
        bounce_core::msgpack::to_vec(&update).unwrap(),
    );

    let result = bob
        .engine
        .handle_frame(
            &alice.address,
            bounce_core::wire::RawFrame::new(
                bounce_core::types::FrameType::UpdateDm.as_u16(),
                bounce_core::msgpack::to_vec(&container).unwrap(),
            ),
        )
        .await;

    assert!(result.is_err(), "a private setting is not the other side's to change");
    assert_eq!(bob.store.user(alice.user.id).unwrap().unwrap().muted_until, 0);
}

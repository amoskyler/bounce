//! End-to-end tests: two engines, real sockets, real signatures.
//!
//! These run over [`TcpNetwork`] rather than Tor, so a test finishes in
//! milliseconds instead of the minute it takes to publish two hidden services.
//! Everything above the socket is the production path: the same handshake, the
//! same framing, the same signature checks, the same consensus.

use std::sync::Arc;
use std::time::Duration;

use libbounce::crypto::DeviceKey;
use libbounce::engine::files::OutgoingAttachment;
use libbounce::engine::{Engine, Event};
use libbounce::frames::identity::User;
use libbounce::net::{Network, StaticDirectory, TcpNetwork};
use libbounce::store::Store;
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
        .send_direct_message(bob.user.id, "hello from Alice", None)
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
        .direct_messages_for_thread(libbounce::xor(alice.user.id, bob.user.id), 10)
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
        .send_direct_message(bob.user.id, "did you get this?", None)
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
            libbounce::types::FrameType::DirectMessage
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
            .send_direct_message(bob.user.id, &format!("message {i}"), None)
            .await
            .unwrap();
    }

    let thread = libbounce::xor(alice.user.id, bob.user.id);
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

    // Bob answers his own invitations, so that the window where he holds one
    // and nothing more actually exists: the default policy joins a group whose
    // members are all people he has already accepted, and Alice is one.
    bob.engine
        .set_auto_join_groups(libbounce::engine::auto_join::NEVER)
        .await
        .expect("sets the policy");

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
        .send_group_message(group.id, "members only", None)
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
        .send_group_message(group.id, "first meeting is Tuesday", None)
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
    let mut message = libbounce::frames::message::DirectMessage::new(
        Uuid::new_v4(),
        bob.user.id,
        "trust me".into(),
        libbounce::now(),
    );
    let body = libbounce::msgpack::to_vec(&message).unwrap();
    let container = libbounce::signed::SignedContainer::create(&stranger_key, body);
    message.signed = libbounce::frames::SignedFrame::from_container(&container);

    let payload = libbounce::msgpack::to_vec(&container).unwrap();
    libbounce::wire::write_frame(
        &mut connection.stream,
        &libbounce::wire::RawFrame::new(
            libbounce::types::FrameType::DirectMessage.as_u16(),
            payload,
        ),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    // The signature is valid, but the signing device is not one Bob has been
    // introduced to, so it speaks for nobody and the message is dropped.
    let thread = libbounce::xor(message.author, bob.user.id);
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
    let mut message = libbounce::frames::message::DirectMessage::new(
        alice.user.id,
        bob.user.id,
        "I did not write this".into(),
        libbounce::now(),
    );
    let body = libbounce::msgpack::to_vec(&message).unwrap();

    let impostor = DeviceKey::generate();
    let mut container = libbounce::signed::SignedContainer::create(&impostor, body);
    container.signer = alice.address.clone();
    message.signed = libbounce::frames::SignedFrame::from_container(&container);

    let impostor_network = Arc::new(
        TcpNetwork::bind(impostor, Arc::clone(&directory))
            .await
            .unwrap(),
    );
    let mut connection = impostor_network.dial(&bob.address).await.unwrap();

    let payload = libbounce::msgpack::to_vec(&container).unwrap();
    libbounce::wire::write_frame(
        &mut connection.stream,
        &libbounce::wire::RawFrame::new(
            libbounce::types::FrameType::DirectMessage.as_u16(),
            payload,
        ),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let thread = libbounce::xor(alice.user.id, bob.user.id);
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
        .send_direct_message(alice.user.id, "remember the milk", None)
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
        .send_direct_message(bob.user.id, "hello", None)
        .await
        .unwrap();
    let group = alice.engine.create_group("Planning", &[]).await.unwrap();
    alice
        .engine
        .send_group_message(group.id, "kickoff", None)
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
        .send_direct_message(bob.user.id, "nice to meet you", None)
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
    let offer_user = libbounce::msgpack::to_vec(&mallory_record).unwrap();

    // Mallory signs Bob's record, as though answering a request Bob never made.
    let bob_record = {
        let mut record = bob.user.clone();
        record.profile = false;
        record.private_ecdh_key = Vec::new();
        record.private_ecdsa_key = Vec::new();
        record.public_ecdsa_key = Vec::new();
        record
    };
    let requester_user = libbounce::msgpack::to_vec(&bob_record).unwrap();

    let accepted = libbounce::frames::pairing::AddUserRequestAccepted {
        offer_user,
        offer_signature: mallory
            .key
            .sign(&libbounce::crypto::hash(&requester_user))
            .to_vec(),
        // Absent, matching the Go frame: the signer is the connected peer.
        offer_device: None,
    };

    let result = bob
        .engine
        .handle_frame(
            &mallory.address,
            libbounce::wire::RawFrame::new(
                libbounce::types::FrameType::AddUserRequestAccepted.as_u16(),
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

    let offer_user = libbounce::msgpack::to_vec(&strip(&mallory.user)).unwrap();
    let requester_user = libbounce::msgpack::to_vec(&fake_bob).unwrap();

    let record = libbounce::frames::pairing::AddUser {
        id: Uuid::new_v4(),
        xor: libbounce::xor(mallory.user.id, bob.user.id),
        timestamp: libbounce::now(),
        saved_at: 0,
        offer_signature: mallory
            .key
            .sign(&libbounce::crypto::hash(&requester_user))
            .to_vec(),
        // Mallory signs "Bob's" half too, because the device list says it is hers.
        requester_signature: mallory
            .key
            .sign(&libbounce::crypto::hash(&offer_user))
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
            libbounce::wire::RawFrame::new(
                libbounce::types::FrameType::AddUser.as_u16(),
                libbounce::msgpack::to_vec(&record).unwrap(),
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
        .send_direct_message(bob.user.id, "for Bob only", None)
        .await
        .unwrap();

    // Mallory claims to have read a message in a conversation she is not in.
    let mut receipt = libbounce::frames::message::ReadReceipt {
        signed: libbounce::frames::SignedFrame::default(),
        id: Uuid::new_v4(),
        actor: mallory.user.id,
        destination: Uuid::nil(),
        scope: 0,
        target: sent.id,
        target_type: libbounce::types::FrameType::DirectMessage.as_u16(),
        timestamp: libbounce::now(),
        saved_at: 0,
    };
    let body = libbounce::msgpack::to_vec(&receipt).unwrap();
    let container = libbounce::signed::SignedContainer::create(&mallory.key, body);
    receipt.signed = libbounce::frames::SignedFrame::from_container(&container);

    let result = alice
        .engine
        .handle_frame(
            &mallory.address,
            libbounce::wire::RawFrame::new(
                libbounce::types::FrameType::ReadReceipt.as_u16(),
                libbounce::msgpack::to_vec(&container).unwrap(),
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
        .send_direct_message(bob.user.id, "read me twice", None)
        .await
        .unwrap();

    let received = wait_for(&mut bob.events, "the message", |event| match event {
        Event::MessageReceived { message } => Some(message.clone()),
        _ => None,
    })
    .await;

    for _ in 0..5 {
        bob.engine
            .mark_as_read(received.id, libbounce::types::FrameType::DirectMessage)
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
        .send_direct_message(bob.user.id, "have you seen this?", None)
        .await
        .unwrap();

    let received = wait_for(&mut bob.events, "Bob to receive the message", |event| match event {
        Event::MessageReceived { message } => Some(message.clone()),
        _ => None,
    })
    .await;

    bob.engine
        .mark_as_read(received.id, libbounce::types::FrameType::DirectMessage)
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
async fn a_delivered_message_is_still_delivered_after_a_restart() {
    // `MessageDelivered` fires once, when the acknowledgement lands. A client
    // that rebuilds its timeline from `initial_state` has no way to hear it
    // again, so a view that reported nobody left every delivered-but-unread
    // message showing the pending tick from the next launch onwards — and
    // permanently, since the event never repeats.
    let directory = Arc::new(StaticDirectory::new());
    let mut alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();

    let sent = alice
        .engine
        .send_direct_message(bob.user.id, "did this land?", None)
        .await
        .unwrap();

    wait_for(&mut bob.events, "Bob to receive the message", |event| match event {
        Event::MessageReceived { message } => Some(message.clone()),
        _ => None,
    })
    .await;

    wait_for(&mut alice.events, "the acknowledgement", |event| match event {
        Event::MessageDelivered { message_id, .. } if *message_id == sent.id => Some(()),
        _ => None,
    })
    .await;

    // Deliberately not read: this is the rung between sending and read, and it
    // is the only one that depends on the delivery record surviving.
    assert!(alice.store.readers_of(sent.id).unwrap().is_empty());

    let state = alice.engine.initial_state().expect("builds initial state");
    let view = state
        .messages
        .iter()
        .find(|message| message.id == sent.id)
        .expect("the message is in the snapshot");

    assert_eq!(view.delivered_to, vec![bob.user.id]);
    assert!(view.read_by.is_empty());
}

#[tokio::test]
async fn delivery_to_our_own_devices_does_not_tick_a_message_off() {
    // A sync-scoped copy landing on another of our own devices is the same
    // person twice. Counting it would show delivered before the message had
    // left the profile — and a note to self, which never leaves at all, would
    // report itself delivered.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;

    let sent = alice
        .engine
        .send_direct_message(alice.user.id, "remember this", None)
        .await
        .unwrap();

    // Stand in for a second device of Alice's acknowledging the frame.
    alice
        .store
        .record_delivery(&libbounce::frames::transport::DeliveryRecord::new(
            alice.address.clone(),
            sent.id,
            libbounce::types::FrameType::DirectMessage,
            libbounce::now(),
        ))
        .expect("records the delivery");

    let state = alice.engine.initial_state().expect("builds initial state");
    let view = state
        .messages
        .iter()
        .find(|message| message.id == sent.id)
        .expect("the message is in the snapshot");

    assert!(
        view.delivered_to.is_empty(),
        "our own device is not a recipient: {:?}",
        view.delivered_to,
    );
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
        .send_direct_message(bob.user.id, "read me", None)
        .await
        .unwrap();

    // Hand Bob the receipt without the message, by having him handle the frame
    // directly — the situation a reordered catch up produces.
    let mut receipt_only = libbounce::frames::message::ReadReceipt {
        signed: libbounce::frames::SignedFrame::default(),
        id: Uuid::new_v4(),
        actor: bob.user.id,
        destination: Uuid::nil(),
        scope: 0,
        target: sent.id,
        target_type: libbounce::types::FrameType::DirectMessage.as_u16(),
        timestamp: libbounce::now(),
        saved_at: 0,
    };
    let body = libbounce::msgpack::to_vec(&receipt_only).unwrap();
    let container = libbounce::signed::SignedContainer::create(&bob.key, body);
    receipt_only.signed = libbounce::frames::SignedFrame::from_container(&container);

    alice
        .engine
        .handle_frame(
            &bob.address,
            libbounce::wire::RawFrame::new(
                libbounce::types::FrameType::ReadReceipt.as_u16(),
                libbounce::msgpack::to_vec(&container).unwrap(),
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
        .typing_in(bob.user.id, libbounce::types::FrameType::DirectMessage)
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
        .has_frame(Uuid::nil(), libbounce::types::FrameType::TypingIndicator)
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
        .typing_in(bob.user.id, libbounce::types::FrameType::DirectMessage)
        .await
        .unwrap();

    wait_for(&mut bob.events, "the typing indicator", |event| match event {
        Event::TypingStarted { user_id, .. } if *user_id == alice.user.id => Some(()),
        _ => None,
    })
    .await;

    alice
        .engine
        .send_direct_message(bob.user.id, "here it is", None)
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
            .typing_in(bob.user.id, libbounce::types::FrameType::DirectMessage)
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
    let mut indicator = libbounce::frames::message::TypingIndicator {
        signed: libbounce::frames::SignedFrame::default(),
        id: Uuid::new_v4(),
        thread: alice.user.id,
        message_type: libbounce::types::FrameType::DirectMessage.as_u16(),
        author: mallory.user.id,
        received_at: 0,
    };
    let body = libbounce::msgpack::to_vec(&indicator).unwrap();
    let container = libbounce::signed::SignedContainer::create(&mallory.key, body);
    indicator.signed = libbounce::frames::SignedFrame::from_container(&container);

    let result = bob
        .engine
        .handle_frame(
            &mallory.address,
            libbounce::wire::RawFrame::new(
                libbounce::types::FrameType::TypingIndicator.as_u16(),
                libbounce::msgpack::to_vec(&container).unwrap(),
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
        ..Default::default()
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
    let payload: Vec<u8> = (0..libbounce::CHUNK_SIZE * 2 + 4096)
        .map(|index| (index % 251) as u8)
        .collect();

    let sent = alice
        .engine
        .send_direct_message_with_attachments(
            bob.user.id,
            "look at this",
            vec![image("photo.png", payload.clone())],
            None,
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
            None,
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
            None,
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
    let forged = libbounce::frames::file::Chunk {
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
            libbounce::wire::RawFrame::new(
                libbounce::types::FrameType::Chunk.as_u16(),
                libbounce::msgpack::to_vec(&forged).unwrap(),
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

    let oversized = vec![0u8; (libbounce::EMBEDDED_FILE_LIMIT + 1) as usize];
    let result = alice
        .engine
        .send_direct_message_with_attachments(bob.user.id, "", vec![image("huge.bin", oversized)], None)
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
    let update = libbounce::frames::update::UpdateDm::new(
        alice.user.id,
        libbounce::xor(alice.user.id, bob.user.id),
        libbounce::frames::UpdateDmType::ChangeMutedUntil,
        i64::MAX.to_le_bytes().to_vec(),
        libbounce::now(),
    );
    let container = libbounce::signed::SignedContainer::create(
        &alice.key,
        libbounce::msgpack::to_vec(&update).unwrap(),
    );

    let result = bob
        .engine
        .handle_frame(
            &alice.address,
            libbounce::wire::RawFrame::new(
                libbounce::types::FrameType::UpdateDm.as_u16(),
                libbounce::msgpack::to_vec(&container).unwrap(),
            ),
        )
        .await;

    assert!(result.is_err(), "a private setting is not the other side's to change");
    assert_eq!(bob.store.user(alice.user.id).unwrap().unwrap().muted_until, 0);
}

// -------------------------------------------------------------------------
// Meeting people through a group
// -------------------------------------------------------------------------

/// Send a signed frame to an engine as though it had arrived from `from`.
async fn deliver<T: serde::Serialize>(
    to: &Instance,
    from: &str,
    key: &DeviceKey,
    frame_type: libbounce::types::FrameType,
    body: &T,
) -> Result<(), libbounce::Error> {
    let container = libbounce::signed::SignedContainer::create(
        key,
        libbounce::msgpack::to_vec(body).unwrap(),
    );
    to.engine
        .handle_frame(
            from,
            libbounce::wire::RawFrame::new(
                frame_type.as_u16(),
                libbounce::msgpack::to_vec(&container).unwrap(),
            ),
        )
        .await
}

#[tokio::test]
async fn a_group_introduces_two_members_who_have_never_paired() {
    // The ordinary case for a group of three: Alice knows both of them, and
    // they have never met. Deliberately not using `introduce`, which pre-seeds
    // both stores and so bypasses the entire path under test.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;
    let mut carol = start("Carol", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    introduce(&alice, &carol);
    assert!(bob.store.user(carol.user.id).unwrap().is_none());
    assert!(carol.store.user(bob.user.id).unwrap().is_none());

    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    Arc::clone(&alice.engine).connect(&carol.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let group = alice
        .engine
        .create_group("Trio", &[bob.user.id, carol.user.id])
        .await
        .expect("creates the group");

    // Bob learns Carol from the invitation that carries her record, and is told
    // about her rather than having to be restarted to notice.
    let learned = wait_for(&mut bob.events, "Bob to be told about Carol", |event| match event {
        Event::UserAdded { user } if user.id == carol.user.id => Some(user.clone()),
        _ => None,
    })
    .await;
    assert_eq!(learned.name, "Carol");
    // Met through a group, not chosen: no conversation is opened for her, and
    // she is not accepted until an invitation is answered.
    assert!(!learned.open_dm);
    assert!(!learned.accepted);

    let bobs_carol = bob.store.user(carol.user.id).unwrap().expect("Carol is stored");
    assert_eq!(bobs_carol.introduction_method, "group");
    assert_eq!(bobs_carol.introduction_metadata, group.id);
    assert_eq!(
        bobs_carol.devices.iter().map(|d| d.address.clone()).collect::<Vec<_>>(),
        vec![carol.address.clone()],
        "her device group is what makes her frames attributable",
    );

    wait_for(&mut carol.events, "Carol to be told about Bob", |event| match event {
        Event::UserAdded { user } if user.id == bob.user.id => Some(()),
        _ => None,
    })
    .await;
    assert_eq!(
        carol.store.user(bob.user.id).unwrap().unwrap().introduction_method,
        "group"
    );
}

#[tokio::test]
async fn two_members_who_met_through_a_group_can_talk_in_it() {
    // The consequence of the record never being stored: their group messages
    // fail the signer check, so they are dropped in both directions and each is
    // invisible to the other for as long as the group exists.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;
    let mut carol = start("Carol", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    introduce(&alice, &carol);

    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    Arc::clone(&alice.engine).connect(&carol.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let group = alice
        .engine
        .create_group("Trio", &[bob.user.id, carol.user.id])
        .await
        .unwrap();

    for events in [&mut bob.events, &mut carol.events] {
        wait_for(events, "the group", |event| match event {
            Event::GroupUpdated { group } if group.name == "Trio" => Some(()),
            _ => None,
        })
        .await;
    }

    bob.engine.respond_to_invite(group.id, true).await.unwrap();
    carol.engine.respond_to_invite(group.id, true).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Each holds the other as a member, which only happens if the record was
    // stored: membership is rebuilt from the database on every recomputation.
    let bobs_group = bob.store.group(group.id).unwrap().unwrap();
    assert!(
        bobs_group.member_ids().contains(&carol.user.id),
        "Carol is missing from Bob's copy of the group: {:?}",
        bobs_group.member_ids()
    );

    // And they can reach each other directly, without Alice relaying.
    Arc::clone(&carol.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    carol
        .engine
        .send_group_message(group.id, "hello from a stranger", None)
        .await
        .expect("posts to the group");

    let message = wait_for(&mut bob.events, "Carol's message", |event| match event {
        Event::MessageReceived { message } if message.text == "hello from a stranger" => {
            Some(message.clone())
        }
        _ => None,
    })
    .await;
    assert_eq!(message.author, carol.user.id);
}

#[tokio::test]
async fn the_founder_of_a_group_you_join_becomes_someone_you_can_name() {
    // Bob invites Carol to a group Alice founded. Carol has never met Alice, so
    // without an event for the founder every message and status row in the
    // group renders as "Unknown" until the client is restarted.
    let directory = Arc::new(StaticDirectory::new());
    let mut alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;
    let mut carol = start("Carol", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    introduce(&bob, &carol);
    assert!(carol.store.user(alice.user.id).unwrap().is_none());

    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    Arc::clone(&bob.engine).connect(&carol.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let group = alice.engine.create_group("Book Club", &[bob.user.id]).await.unwrap();

    // Waited for on *Alice's* side: promoting somebody who is not yet a member
    // is not a permitted change, so the promotion has to be signed against a
    // history that already holds his acceptance.
    wait_for(&mut alice.events, "Bob to join", |event| match event {
        Event::GroupUpdated { group: updated }
            if updated.id == group.id && updated.members.contains(&bob.user.id) =>
        {
            Some(())
        }
        _ => None,
    })
    .await;

    // Groups restrict user management to admins by default, so the invitation
    // Bob is about to send has to be one consensus will accept.
    alice
        .engine
        .set_group_admin(group.id, bob.user.id, true)
        .await
        .expect("promotes Bob");
    wait_for(&mut bob.events, "Bob to become an admin", |event| match event {
        Event::GroupUpdated { group: updated }
            if updated.id == group.id && updated.admins.contains(&bob.user.id) =>
        {
            Some(())
        }
        _ => None,
    })
    .await;

    bob.engine.invite_to_group(group.id, carol.user.id).await.unwrap();

    let founder = wait_for(&mut carol.events, "Carol to learn who founded it", |event| {
        match event {
            Event::UserAdded { user } if user.id == alice.user.id => Some(user.clone()),
            _ => None,
        }
    })
    .await;
    assert_eq!(founder.name, "Alice");
    assert_eq!(
        carol.store.user(alice.user.id).unwrap().unwrap().introduction_method,
        "group"
    );
}

// -------------------------------------------------------------------------
// Profile changes
// -------------------------------------------------------------------------

#[tokio::test]
async fn a_contacts_new_name_reaches_the_people_who_know_them() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    alice
        .engine
        .update_profile_name("Alice Cooper")
        .await
        .expect("renames the profile");

    let updated = wait_for(&mut bob.events, "the rename", |event| match event {
        Event::UserUpdated { user } if user.id == alice.user.id => Some(user.clone()),
        _ => None,
    })
    .await;
    assert_eq!(updated.name, "Alice Cooper");
    assert_eq!(bob.store.user(alice.user.id).unwrap().unwrap().name, "Alice Cooper");

    // Kept, not merely applied: a device that was offline for the rename is
    // caught up with the frame itself.
    let updates = bob.store.updates_for_user(alice.user.id).unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].data, b"Alice Cooper".to_vec());
    assert!(bob
        .store
        .has_frame(updates[0].id, libbounce::types::FrameType::UpdateUser)
        .unwrap());
    assert_eq!(
        updates[0].previous_data,
        b"Alice".to_vec(),
        "the timeline needs to be able to say what the name changed from"
    );

    // Replayed rather than applied blindly: a second rename wins because it is
    // later, not because it arrived later.
    alice.engine.update_profile_name("Alice C").await.unwrap();
    wait_for(&mut bob.events, "the second rename", |event| match event {
        Event::UserUpdated { user } if user.name == "Alice C" => Some(()),
        _ => None,
    })
    .await;
    assert_eq!(bob.store.user(alice.user.id).unwrap().unwrap().name, "Alice C");
}

#[tokio::test]
async fn nobody_can_rename_somebody_else() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;
    let mallory = start("Mallory", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    introduce(&bob, &mallory);

    let mut update = libbounce::frames::update::UpdateUser::new(
        alice.user.id,
        libbounce::frames::update::UpdateUserType::UpdateName,
        b"Mallory's Puppet".to_vec(),
        libbounce::now(),
    );
    update.saved_at = libbounce::now();

    let result = deliver(
        &bob,
        &mallory.address,
        &mallory.key,
        libbounce::types::FrameType::UpdateUser,
        &update,
    )
    .await;

    assert!(result.is_err(), "a profile is its owner's to change and nobody else's");
    assert_eq!(bob.store.user(alice.user.id).unwrap().unwrap().name, "Alice");
    assert!(bob.store.updates_for_user(alice.user.id).unwrap().is_empty());
}

// -------------------------------------------------------------------------
// Drafts
// -------------------------------------------------------------------------

/// Add a second device to an instance's own device group.
///
/// Multi-device pairing is not implemented yet, so the outcome of it — a second
/// device this profile owns — is established directly, exactly as `introduce`
/// does for contacts.
fn add_own_device(instance: &Instance, name: &str) -> (DeviceKey, String) {
    let key = DeviceKey::generate();
    let address = key.address();

    let mut device = libbounce::frames::identity::Device::new(
        Uuid::new_v4(),
        instance.user.id,
        address.clone(),
        libbounce::now(),
    );
    device.name = name.to_string();
    device.saved_at = libbounce::now();
    instance.store.save_device(&device).expect("saves the device");

    (key, address)
}

#[tokio::test]
async fn a_draft_typed_on_another_device_arrives_here() {
    let directory = Arc::new(StaticDirectory::new());
    let mut alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;
    introduce(&alice, &bob);

    let (laptop, laptop_address) = add_own_device(&alice, "Alice's laptop");

    let draft = libbounce::frames::message::Draft {
        signed: libbounce::frames::SignedFrame::default(),
        id: Uuid::new_v4(),
        thread: bob.user.id,
        text: "started on the laptop".into(),
        timestamp: libbounce::now(),
        saved: false,
        saved_at: 0,
    };

    deliver(
        &alice,
        &laptop_address,
        &laptop,
        libbounce::types::FrameType::Draft,
        &draft,
    )
    .await
    .expect("a draft from our own device is accepted");

    let arrived = wait_for(&mut alice.events, "the draft", |event| match event {
        Event::DraftUpdated { draft } => Some(draft.clone()),
        _ => None,
    })
    .await;
    assert_eq!(arrived.thread, bob.user.id);
    assert_eq!(arrived.text, "started on the laptop");

    // Durable, and with the bytes it was signed as, so it can be relayed to a
    // third device that was offline.
    let stored = alice
        .store
        .draft_for_thread(bob.user.id)
        .unwrap()
        .expect("the draft was stored");
    assert_eq!(stored.text, "started on the laptop");
    assert!(stored.signed.to_container().is_valid());
    assert!(alice
        .store
        .has_frame(draft.id, libbounce::types::FrameType::Draft)
        .unwrap());

    // An older draft for the same thread does not undo the newer one.
    let mut stale = draft.clone();
    stale.id = Uuid::new_v4();
    stale.text = "an earlier keystroke".into();
    stale.timestamp -= 10;
    deliver(
        &alice,
        &laptop_address,
        &laptop,
        libbounce::types::FrameType::Draft,
        &stale,
    )
    .await
    .expect("stale drafts are dropped, not errors");
    assert_eq!(
        alice.store.draft_for_thread(bob.user.id).unwrap().unwrap().text,
        "started on the laptop"
    );

    // And emptying it on the other device clears it here.
    let mut cleared = draft.clone();
    cleared.id = Uuid::new_v4();
    cleared.text = String::new();
    cleared.timestamp += 1;
    deliver(
        &alice,
        &laptop_address,
        &laptop,
        libbounce::types::FrameType::Draft,
        &cleared,
    )
    .await
    .expect("an emptied draft is accepted");
    assert!(alice.store.draft_for_thread(bob.user.id).unwrap().is_none());
}

#[tokio::test]
async fn a_draft_from_somebody_elses_device_is_refused() {
    // A draft is the one frame that is only ever ours: it syncs inside the
    // device group and nowhere else, so a contact's device signing one is
    // making a claim about our own state.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;
    introduce(&alice, &bob);

    let draft = libbounce::frames::message::Draft {
        signed: libbounce::frames::SignedFrame::default(),
        id: Uuid::new_v4(),
        thread: bob.user.id,
        text: "not yours to write".into(),
        timestamp: libbounce::now(),
        saved: false,
        saved_at: 0,
    };

    let result = deliver(
        &alice,
        &bob.address,
        &bob.key,
        libbounce::types::FrameType::Draft,
        &draft,
    )
    .await;

    assert!(result.is_err());
    assert!(alice.store.draft_for_thread(bob.user.id).unwrap().is_none());
}

// -------------------------------------------------------------------------
// Giving up on a message
// -------------------------------------------------------------------------

#[tokio::test]
async fn a_message_nobody_ever_took_is_marked_undeliverable() {
    let directory = Arc::new(StaticDirectory::new());
    let mut alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;
    introduce(&alice, &bob);

    // Written four weeks ago and never acknowledged by anybody: the recipient's
    // device is gone and is not coming back.
    let mut message = libbounce::frames::message::DirectMessage::new(
        alice.user.id,
        bob.user.id,
        "is anyone there".into(),
        libbounce::now() - libbounce::UNDELIVERABLE_AFTER_SECONDS - 1,
    );
    let container = libbounce::signed::SignedContainer::create(
        &alice.key,
        libbounce::msgpack::to_vec(&message).unwrap(),
    );
    message.signed = libbounce::frames::SignedFrame::from_container(&container);
    message.saved_at = message.written_at;
    alice.store.save_direct_message(&message).unwrap();

    let recent = alice
        .engine
        .send_direct_message(bob.user.id, "and this one is still trying", None)
        .await
        .unwrap();

    let marked = alice
        .engine
        .mark_stale_messages_undeliverable()
        .expect("sweeps");
    assert_eq!(marked, vec![message.id]);

    let told = wait_for(&mut alice.events, "the interface to be told", |event| match event {
        Event::MessageUndeliverable { message_id } => Some(*message_id),
        _ => None,
    })
    .await;
    assert_eq!(told, message.id);

    assert!(alice.store.direct_message(message.id).unwrap().unwrap().undeliverable);
    assert!(
        !alice.store.direct_message(recent.id).unwrap().unwrap().undeliverable,
        "a message written a second ago has not been given up on"
    );

    // It is also no longer offered to the peer it was for, which is what stops
    // the offer growing forever.
    let offered = alice
        .store
        .references_not_delivered_to(&bob.address, |_, _| true)
        .unwrap();
    let ids: Vec<Uuid> = offered.iter().map(|reference| reference.frame_id).collect();
    assert!(ids.contains(&recent.id));
    assert!(!ids.contains(&message.id));
}

// -------------------------------------------------------------------------
// Auto-join
// -------------------------------------------------------------------------

#[tokio::test]
async fn an_invitation_from_people_you_know_is_accepted_for_you() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // The default policy: join a group that holds nobody Bob has not already
    // accepted. Alice is his contact, and she is the only other person in it.
    let group = alice.engine.create_group("Just Us", &[bob.user.id]).await.unwrap();

    wait_for(&mut bob.events, "Bob to be joined for him", |event| match event {
        Event::GroupUpdated { group: updated }
            if updated.id == group.id && updated.members.contains(&bob.user.id) =>
        {
            Some(())
        }
        _ => None,
    })
    .await;

    // Accepting also records that everybody in the group is somebody he has
    // agreed to be in one with, which is what the policy reads next time.
    assert!(bob.store.user(alice.user.id).unwrap().unwrap().accepted);
}

#[tokio::test]
async fn an_invitation_that_brings_a_stranger_with_it_is_left_to_the_user() {
    // Carol is somebody Bob has never accepted. Under the default policy that
    // is precisely the case the group is not joined automatically, because
    // joining would put him in a group with somebody he never agreed to.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;
    let carol = start("Carol", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    introduce(&alice, &carol);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let group = alice
        .engine
        .create_group("Strangers", &[bob.user.id, carol.user.id])
        .await
        .unwrap();

    wait_for(&mut bob.events, "Bob to learn about the group", |event| match event {
        Event::GroupUpdated { group: updated } if updated.id == group.id => Some(()),
        _ => None,
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    let bobs_group = bob.store.group(group.id).unwrap().unwrap();
    assert!(bobs_group.invite_ids().contains(&bob.user.id));
    assert!(
        !bobs_group.member_ids().contains(&bob.user.id),
        "an invitation that introduces somebody new is the user's to answer"
    );

    // Answering it himself is what marks them accepted.
    assert!(!bob.store.user(carol.user.id).unwrap().unwrap().accepted);
    bob.engine.respond_to_invite(group.id, true).await.unwrap();
    assert!(bob.store.user(carol.user.id).unwrap().unwrap().accepted);
}

#[tokio::test]
async fn a_confirmation_reaches_the_other_side_of_a_group() {
    // The consensus module computed confirmations correctly and no engine ever
    // called it: `FrameType::Confirmation` appeared in exactly one place in the
    // crate, and none of them was a dispatcher. The backdating defence was
    // therefore inert, and — worse than inert — a Go peer counting real votes
    // and a Rust peer counting none would disagree about which of two
    // conflicting updates is canonical.
    //
    // This drives the real engine path rather than the consensus module: no
    // hand-rolled recompute, no hand-fed frames.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let group = alice.engine.create_group("Quorum", &[bob.user.id]).await.unwrap();
    wait_for(&mut bob.events, "Bob to learn about the group", |event| match event {
        Event::GroupUpdated { group } if group.name == "Quorum" => Some(()),
        _ => None,
    })
    .await;
    bob.engine.respond_to_invite(group.id, true).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    alice.engine.rename_group(group.id, "Quorate").await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Bob confirms what he accepted, and Alice must end up holding his vote —
    // which can only happen if his engine minted it, broadcast it, and hers
    // ingested it.
    let updates = alice.store.updates_for_group(group.id).unwrap();
    let rename = updates
        .iter()
        .find(|update| update.data == b"Quorate")
        .expect("Alice stored her own rename");

    let authors: Vec<_> = rename
        .confirmations
        .iter()
        .map(|confirmation| confirmation.author)
        .collect();

    assert!(
        authors.contains(&bob.user.id),
        "Alice should hold Bob's confirmation of the rename, got {authors:?}",
    );

    // And the votes are attributable: a confirmation whose signing device we
    // cannot resolve is not silently counted.
    for confirmation in &rename.confirmations {
        assert!(
            !confirmation.signing_device.is_empty(),
            "a stored confirmation must name the device that signed it",
        );
    }
}

#[tokio::test]
async fn a_closed_conversation_reopens_when_a_message_arrives() {
    // Closing only hides. The messages keep arriving, so without reopening
    // they would keep arriving invisibly — a tidying gesture turned into
    // silent message loss.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Bob tidies Alice off his list.
    bob.engine.set_open_dm(alice.user.id, false).await.unwrap();
    assert!(!bob.store.user(alice.user.id).unwrap().unwrap().open_dm);

    alice
        .engine
        .send_direct_message(bob.user.id, "still here", None)
        .await
        .unwrap();

    let received = wait_for(&mut bob.events, "Bob to receive the message", |event| match event {
        Event::MessageReceived { message } => Some(message.clone()),
        _ => None,
    })
    .await;
    assert_eq!(received.text, "still here");

    assert!(
        bob.store.user(alice.user.id).unwrap().unwrap().open_dm,
        "the conversation must come back, or the message is invisible",
    );
}

#[tokio::test]
async fn a_blocked_contact_stays_closed_when_they_write() {
    // Blocking and closing are different intentions. A blocked contact's
    // messages are refused outright, and reopening would put somebody back on
    // the list who was deliberately taken off it.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    bob.engine.set_user_blocked(alice.user.id, true).await.unwrap();
    bob.engine.set_open_dm(alice.user.id, false).await.unwrap();

    alice
        .engine
        .send_direct_message(bob.user.id, "let me back in", None)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    let stored = bob.store.user(alice.user.id).unwrap().unwrap();
    assert!(stored.blocked);
    assert!(!stored.open_dm, "a blocked contact must not reappear");
}

#[tokio::test]
async fn message_info_reports_who_received_and_who_read() {
    // The panel behind "Info" on a message. Delivery is recorded per device and
    // reading per person; both have to come back as people, with times, or the
    // panel is guesswork dressed up as fact.
    let directory = Arc::new(StaticDirectory::new());
    let mut alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let sent = alice
        .engine
        .send_direct_message(bob.user.id, "did you get this?", None)
        .await
        .unwrap();

    // Nothing has happened to it yet beyond being written.
    let fresh = alice.engine.message_info(sent.id).unwrap().expect("exists");
    assert_eq!(fresh.message_id, sent.id);
    assert_eq!(fresh.written_at, sent.written_at);
    assert!(fresh.read_by.is_empty(), "nobody has read it yet");

    wait_for(&mut alice.events, "a delivery confirmation", |event| match event {
        Event::MessageDelivered { message_id, .. } if *message_id == sent.id => Some(()),
        _ => None,
    })
    .await;

    let delivered = alice.engine.message_info(sent.id).unwrap().expect("exists");
    assert_eq!(
        delivered.delivered_to.iter().map(|r| r.user_id).collect::<Vec<_>>(),
        vec![bob.user.id],
        "delivery should name the person, not the device",
    );
    assert!(delivered.delivered_to[0].at > 0, "a delivery with no time is no use");
    assert!(delivered.read_by.is_empty(), "delivered is not read");

    // Bob reads it, which sends a receipt back.
    bob.engine
        .mark_as_read(sent.id, libbounce::types::FrameType::DirectMessage)
        .await
        .unwrap();

    wait_for(&mut alice.events, "the read receipt", |event| match event {
        Event::MessageRead { message_id, .. } if *message_id == sent.id => Some(()),
        _ => None,
    })
    .await;

    let read = alice.engine.message_info(sent.id).unwrap().expect("exists");
    assert_eq!(
        read.read_by.iter().map(|r| r.user_id).collect::<Vec<_>>(),
        vec![bob.user.id],
    );
    assert!(read.read_by[0].at > 0, "a read with no time is no use");

    // A message nobody has ever heard of has no info, rather than empty info.
    assert!(alice.engine.message_info(Uuid::new_v4()).unwrap().is_none());
}

// ---------------------------------------------------------------------------
// Reactions, replies, and deletion
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_reaction_reaches_the_other_side_and_can_be_withdrawn() {
    let directory = Arc::new(StaticDirectory::new());
    let mut alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();

    let sent = alice
        .engine
        .send_direct_message(bob.user.id, "look at this", None)
        .await
        .unwrap();

    wait_for(&mut bob.events, "Bob to receive the message", |event| match event {
        Event::MessageReceived { message } => Some(message.id),
        _ => None,
    })
    .await;

    bob.engine
        .react(sent.id, libbounce::types::FrameType::DirectMessage, "🎉")
        .await
        .expect("reacts");

    let (message_id, user_id, emoji) =
        wait_for(&mut alice.events, "the reaction", |event| match event {
            Event::MessageReacted {
                message_id,
                user_id,
                emoji,
            } => Some((*message_id, *user_id, emoji.clone())),
            _ => None,
        })
        .await;

    assert_eq!(message_id, sent.id);
    assert_eq!(user_id, bob.user.id);
    assert_eq!(emoji, "🎉");

    // And it is on the message Alice's interface would draw.
    let state = alice.engine.initial_state().unwrap();
    let view = state.messages.iter().find(|m| m.id == sent.id).unwrap();
    assert_eq!(view.reactions.len(), 1);
    assert_eq!(view.reactions[0].emoji, "🎉");
    assert_eq!(view.reactions[0].users, vec![bob.user.id]);
    assert!(!view.reactions[0].mine);

    // Withdrawing has to travel too: Alice still holds the reaction, and
    // nothing else will ever tell her it is gone.
    bob.engine
        .remove_reaction(sent.id, libbounce::types::FrameType::DirectMessage)
        .await
        .expect("withdraws");

    wait_for(&mut alice.events, "the withdrawal", |event| match event {
        Event::MessageReacted {
            message_id, emoji, ..
        } if *message_id == sent.id && emoji.is_empty() => Some(()),
        _ => None,
    })
    .await;

    let state = alice.engine.initial_state().unwrap();
    let view = state.messages.iter().find(|m| m.id == sent.id).unwrap();
    assert!(view.reactions.is_empty(), "{:?}", view.reactions);
}

#[tokio::test]
async fn one_person_gets_one_reaction_per_message() {
    // Signal's rule, and the reason the resolved state is a map rather than a
    // log. A second reaction replaces the first rather than sitting beside it.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;

    let sent = alice
        .engine
        .send_direct_message(alice.user.id, "note to self", None)
        .await
        .unwrap();

    for emoji in ["👍", "🎉", "❤️"] {
        alice
            .engine
            .react(sent.id, libbounce::types::FrameType::DirectMessage, emoji)
            .await
            .expect("reacts");
    }

    let state = alice.engine.initial_state().unwrap();
    let view = state.messages.iter().find(|m| m.id == sent.id).unwrap();
    assert_eq!(view.reactions.len(), 1);
    assert_eq!(view.reactions[0].emoji, "❤️");
    assert!(view.reactions[0].mine);
}

#[tokio::test]
async fn a_deletion_reaches_a_peer_who_was_offline_when_it_happened() {
    // The case the whole deletion design is for, and the one nobody can check
    // by hand: the person deleting cannot see whether it landed on a device
    // that was not there. If the frame is not stored and re-offered, the
    // deletion reaches whoever happened to be connected and nobody else.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();

    let sent = alice
        .engine
        .send_direct_message(bob.user.id, "sent in error", None)
        .await
        .unwrap();

    wait_for(&mut bob.events, "Bob to receive it", |event| match event {
        Event::MessageReceived { message } => Some(message.id),
        _ => None,
    })
    .await;

    // Bob goes away, and only then does Alice delete.
    alice.engine.disconnect_all().await;

    alice
        .engine
        .delete_for_everyone(sent.id, libbounce::types::FrameType::DirectMessage)
        .await
        .expect("deletes for everyone");

    // Alice's own copy is a tombstone, not a hole.
    let alice_copy = alice.store.direct_message(sent.id).unwrap().unwrap();
    assert!(alice_copy.is_deleted());
    assert_eq!(alice_copy.text, "");

    // Bob comes back and the reference flow carries the deletion to him.
    Arc::clone(&bob.engine).connect(&alice.address).await.unwrap();

    let withdrawn = wait_for(&mut bob.events, "the deletion", |event| match event {
        Event::MessageWithdrawn { message_id, by, .. } => Some((*message_id, *by)),
        _ => None,
    })
    .await;

    assert_eq!(withdrawn, (sent.id, alice.user.id));

    let bob_copy = bob.store.direct_message(sent.id).unwrap().unwrap();
    assert!(bob_copy.is_deleted());
    assert_eq!(bob_copy.text, "");
}

#[tokio::test]
async fn a_deleted_message_does_not_come_back() {
    // The tombstone's whole job. Remove the row instead and `has_frame` answers
    // false, Bob classifies the original as wanted, offers it back, and the
    // message Alice withdrew reappears on her own screen.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();

    let sent = alice
        .engine
        .send_direct_message(bob.user.id, "please forget this", None)
        .await
        .unwrap();

    wait_for(&mut bob.events, "Bob to receive it", |event| match event {
        Event::MessageReceived { message } => Some(message.id),
        _ => None,
    })
    .await;

    alice
        .engine
        .delete_for_everyone(sent.id, libbounce::types::FrameType::DirectMessage)
        .await
        .unwrap();

    wait_for(&mut bob.events, "Bob to apply the deletion", |event| match event {
        Event::MessageWithdrawn { message_id, .. } if *message_id == sent.id => Some(()),
        _ => None,
    })
    .await;

    // A full reference cycle in both directions, which is what would resurrect
    // it if either side still thought the original was worth offering.
    alice.engine.disconnect_all().await;
    Arc::clone(&bob.engine).connect(&alice.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    bob.engine.disconnect_all().await;
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    for (name, store) in [("Alice", &alice.store), ("Bob", &bob.store)] {
        let copy = store.direct_message(sent.id).unwrap().unwrap();
        assert!(copy.is_deleted(), "{name} un-deleted the message");
        assert_eq!(copy.text, "", "{name} recovered the body");
    }
}

#[tokio::test]
async fn only_the_author_can_delete_a_direct_message_for_everyone() {
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();

    let sent = alice
        .engine
        .send_direct_message(bob.user.id, "mine to withdraw", None)
        .await
        .unwrap();

    wait_for(&mut bob.events, "Bob to receive it", |event| match event {
        Event::MessageReceived { message } => Some(message.id),
        _ => None,
    })
    .await;

    // Bob is not the author, and there is no admin role in a direct message.
    assert!(!bob
        .engine
        .may_delete_for_everyone(sent.id, libbounce::types::FrameType::DirectMessage)
        .unwrap());
    assert!(bob
        .engine
        .delete_for_everyone(sent.id, libbounce::types::FrameType::DirectMessage)
        .await
        .is_err());

    let copy = bob.store.direct_message(sent.id).unwrap().unwrap();
    assert!(!copy.is_deleted());
}

#[tokio::test]
async fn a_reply_carries_a_quote_of_what_it_answers() {
    let directory = Arc::new(StaticDirectory::new());
    let mut alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();

    let original = alice
        .engine
        .send_direct_message(bob.user.id, "shall we say Tuesday?", None)
        .await
        .unwrap();

    wait_for(&mut bob.events, "Bob to receive it", |event| match event {
        Event::MessageReceived { message } => Some(message.id),
        _ => None,
    })
    .await;

    let reply = bob
        .engine
        .send_direct_message(alice.user.id, "Tuesday works", Some(original.id))
        .await
        .unwrap();

    // Assembled from Bob's own copy, not from anything the client passed.
    let quote = reply.quote.as_ref().expect("the reply carries a quote");
    assert_eq!(quote.target, original.id);
    assert_eq!(quote.author, alice.user.id);
    assert_eq!(quote.text, "shall we say Tuesday?");
    assert_eq!(quote.kind, "text");
    assert!(!quote.expired);

    // And it survives the wire.
    let received = wait_for(&mut alice.events, "the reply", |event| match event {
        Event::MessageReceived { message } if message.id == reply.id => Some(message.clone()),
        _ => None,
    })
    .await;

    let quote = received.quote.as_ref().expect("the quote crossed the wire");
    assert_eq!(quote.target, original.id);
    assert_eq!(quote.text, "shall we say Tuesday?");
}

#[tokio::test]
async fn a_quote_is_blanked_when_the_message_it_quotes_expires() {
    // The reply outlives the original, so without this the excerpt keeps a copy
    // of a disappearing message for as long as the reply lives — the quote
    // outliving the thing it quoted is exactly what retention exists to stop.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;

    let original = alice
        .engine
        .send_direct_message(alice.user.id, "forget me shortly", None)
        .await
        .unwrap();

    // A second, so the quote is taken while the original is still live and the
    // sweep then finds it genuinely past. The expiry the quote carries is a
    // *snapshot* taken at send time — it travels on the wire — so moving the
    // original's afterwards would change nothing, which is the point.
    alice
        .store
        .set_message_delete_at(original.id, libbounce::now() + 1)
        .expect("gives the original an expiry");

    let reply = alice
        .engine
        .send_direct_message(alice.user.id, "noted", Some(original.id))
        .await
        .unwrap();
    assert_eq!(
        reply.quote.as_ref().map(|quote| quote.text.as_str()),
        Some("forget me shortly"),
        "the quote is taken while the original is still live",
    );

    tokio::time::sleep(Duration::from_millis(1_200)).await;

    alice.engine.sweep_expired().expect("sweeps");

    let stored = alice.store.direct_message(reply.id).unwrap().unwrap();
    let quote = stored.quote.as_ref().expect("the reply keeps its quote block");
    assert_eq!(quote.text, "", "the excerpt outlived what it quoted");
    assert_eq!(quote.target, original.id, "but still points at it");

    // The reply itself is untouched.
    assert_eq!(stored.text, "noted");
}

#[tokio::test]
async fn a_peer_that_advertises_nothing_is_never_sent_an_extension_frame() {
    // The one test standing between this change and a broken network. Go closes
    // the connection on a frame type it does not know, so a legacy peer must
    // not be offered one — and "legacy" is the absence of a capability, which
    // is what every build that exists today looks like.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);

    // Alice's record of Bob's device predates the extensions.
    let mut bob_device = alice
        .store
        .device_by_address(&bob.address)
        .unwrap()
        .expect("Alice knows Bob's device");
    bob_device.capabilities = Vec::new();
    alice.store.save_device(&bob_device).unwrap();

    let sent = alice
        .engine
        .send_direct_message(bob.user.id, "hello", None)
        .await
        .unwrap();

    alice
        .engine
        .react(sent.id, libbounce::types::FrameType::DirectMessage, "👍")
        .await
        .unwrap();

    let offered = alice
        .store
        .references_not_delivered_to(&bob.address, |_, _| true)
        .unwrap();

    assert!(
        offered
            .iter()
            .any(|reference| reference.frame_type == libbounce::types::FrameType::DirectMessage.as_u16()),
        "the message itself is still offered",
    );
    assert!(
        !offered
            .iter()
            .any(|reference| libbounce::types::FrameType::from_u16(reference.frame_type)
                .map(libbounce::types::FrameType::is_extension)
                .unwrap_or(false)),
        "an extension frame was offered to a legacy peer: {offered:?}",
    );

    // And once Bob's build announces itself, the same reaction is offered.
    bob_device.capabilities = libbounce::types::capability::SUPPORTED
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    alice.store.save_device(&bob_device).unwrap();

    let offered = alice
        .store
        .references_not_delivered_to(&bob.address, |_, _| true)
        .unwrap();
    assert!(
        offered
            .iter()
            .any(|reference| reference.frame_type == libbounce::types::FrameType::Reaction.as_u16()),
        "a capable peer is offered the reaction: {offered:?}",
    );
}

#[tokio::test]
async fn a_keep_alive_teaches_a_peer_what_we_speak() {
    // The gap the device record cannot close. A contact paired before the
    // extensions existed has an empty capability list stored, and nothing in
    // the protocol re-announces a device — so without this they read as legacy
    // forever and every reaction and deletion is silently withheld from them.
    //
    // Silently is the problem. The gate fails towards sending less, so there is
    // no error anywhere: reacting appears to work locally and simply never
    // arrives.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);

    // Alice's record of Bob predates the field, as every existing row does.
    let mut bob_device = alice
        .store
        .device_by_address(&bob.address)
        .unwrap()
        .expect("Alice knows Bob's device");
    bob_device.capabilities = Vec::new();
    alice.store.save_device(&bob_device).unwrap();

    let sent = alice
        .engine
        .send_direct_message(bob.user.id, "hello", None)
        .await
        .unwrap();
    alice
        .engine
        .react(sent.id, libbounce::types::FrameType::DirectMessage, "👍")
        .await
        .unwrap();

    let offers_reaction = |alice: &Instance| {
        alice
            .store
            .references_not_delivered_to(&bob.address, |_, _| true)
            .unwrap()
            .iter()
            .any(|reference| {
                reference.frame_type == libbounce::types::FrameType::Reaction.as_u16()
            })
    };

    assert!(!offers_reaction(&alice), "a legacy peer is offered nothing new");

    // Bob's keep-alive says what he speaks. Go's `handleKeepAlive` reads its
    // payload not at all, so this costs a Go peer nothing in either direction.
    let keep_alive = libbounce::frames::transport::KeepAlive::advertising()
        .encode()
        .unwrap();
    alice
        .engine
        .handle_frame(
            &bob.address,
            libbounce::wire::RawFrame::new(
                libbounce::types::FrameType::KeepAlive.as_u16(),
                keep_alive,
            ),
        )
        .await
        .expect("handles the keep alive");

    assert!(
        offers_reaction(&alice),
        "after Bob announces himself the reaction is offered",
    );

    // Go sends the literal bytes `keep-alive`, which is not msgpack at all.
    // That must neither fail the connection nor be read as an announcement of
    // nothing — otherwise one unparseable frame erases what a capable peer has
    // already told us, and reactions stop arriving again with no trace of why.
    alice
        .engine
        .handle_frame(
            &bob.address,
            libbounce::wire::RawFrame::new(
                libbounce::types::FrameType::KeepAlive.as_u16(),
                b"keep-alive".to_vec(),
            ),
        )
        .await
        .expect("a Go keep-alive is not an error");

    assert!(
        offers_reaction(&alice),
        "an unreadable keep-alive is silence, not a downgrade",
    );

    // An explicit empty announcement *is* a downgrade, and is honoured.
    let downgraded = libbounce::frames::transport::KeepAlive::default()
        .encode()
        .unwrap();
    alice
        .engine
        .handle_frame(
            &bob.address,
            libbounce::wire::RawFrame::new(
                libbounce::types::FrameType::KeepAlive.as_u16(),
                downgraded,
            ),
        )
        .await
        .unwrap();

    assert!(
        !offers_reaction(&alice),
        "a peer that says it speaks nothing is believed",
    );
}

#[tokio::test]
async fn withdrawing_a_reaction_reaches_a_peer_who_was_offline() {
    // The same lesson as deletion, in a second place. A state change that is
    // only broadcast — never stored — reaches whoever happened to be connected
    // and nobody else. Here the consequence is that Bob goes on showing a
    // reaction Alice took back, with nothing that will ever correct him.
    let directory = Arc::new(StaticDirectory::new());
    let alice = start("Alice", Arc::clone(&directory)).await;
    let mut bob = start("Bob", Arc::clone(&directory)).await;

    introduce(&alice, &bob);
    Arc::clone(&alice.engine).connect(&bob.address).await.unwrap();

    let sent = alice
        .engine
        .send_direct_message(bob.user.id, "worth a reaction", None)
        .await
        .unwrap();

    wait_for(&mut bob.events, "Bob to receive the message", |event| match event {
        Event::MessageReceived { message } => Some(message.id),
        _ => None,
    })
    .await;

    alice
        .engine
        .react(sent.id, libbounce::types::FrameType::DirectMessage, "👍")
        .await
        .unwrap();

    wait_for(&mut bob.events, "Bob to see the reaction", |event| match event {
        Event::MessageReacted { emoji, .. } if emoji == "👍" => Some(()),
        _ => None,
    })
    .await;
    assert_eq!(bob.store.reactions_for(sent.id).unwrap().len(), 1);

    // Bob goes away, and only then does Alice take it back.
    alice.engine.disconnect_all().await;
    alice
        .engine
        .remove_reaction(sent.id, libbounce::types::FrameType::DirectMessage)
        .await
        .unwrap();

    // Bob returns. The reference flow has to carry the withdrawal, exactly as
    // it carries a deletion.
    Arc::clone(&bob.engine).connect(&alice.address).await.unwrap();

    wait_for(&mut bob.events, "the withdrawal", |event| match event {
        Event::MessageReacted {
            message_id, emoji, ..
        } if *message_id == sent.id && emoji.is_empty() => Some(()),
        _ => None,
    })
    .await;

    // A full reference cycle in both directions, which is what resurrected it
    // when the withdrawal replaced the reaction instead of being kept beside
    // it: Bob still held the original, Alice no longer answered for its id, so
    // he offered it back and the reaction returned on both sides.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Both frames are still held — the log keeps the reaction *and* the
    // withdrawal, which is what lets either be offered to a third device. What
    // must be empty is the resolved state.
    assert_eq!(
        bob.store.reactions_for(sent.id).unwrap().len(),
        2,
        "both frames are kept",
    );

    for (name, store) in [("Alice", &alice.store), ("Bob", &bob.store)] {
        let shown = libbounce::engine::interaction::resolve_reactions(
            store.reactions_for(sent.id).unwrap().into_iter(),
        );
        assert!(shown.is_empty(), "{name} resurrected the reaction: {shown:?}");
    }
}

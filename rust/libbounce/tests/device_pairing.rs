//! Adding a second device to a profile.
//!
//! The whole of the authentication is a secret read off one screen and two
//! signatures, so these tests care less about the happy path than about what
//! the flow refuses.

use std::sync::Arc;
use std::time::Duration;

use libbounce::crypto::DeviceKey;
use libbounce::engine::{Engine, Event};
use libbounce::frames::Broadcastable;
use libbounce::net::{StaticDirectory, TcpNetwork};
use libbounce::store::Store;
use tokio::sync::mpsc::UnboundedReceiver;

struct Instance {
    engine: Arc<Engine<TcpNetwork>>,
    store: Arc<Store>,
    events: UnboundedReceiver<Event>,
    address: String,
    /// Kept so a test can forge a frame the way a peer would.
    key: DeviceKey,
}

/// Start an instance with no profile; the caller decides whether it gets one.
async fn start(directory: Arc<StaticDirectory>) -> Instance {
    let key = DeviceKey::generate();
    let address = key.address();
    let network = Arc::new(TcpNetwork::bind(key.clone(), directory).await.expect("binds"));
    let store = Arc::new(Store::in_memory().expect("opens"));
    let (engine, events) = Engine::new(key.clone(), Arc::clone(&store), network);
    tokio::spawn(Arc::clone(&engine).run_listener());
    // The chunk engine is what issues chunk requests; the application spawns it
    // alongside the listener (`bounce-node/src/lib.rs`), so the harness does too.
    tokio::spawn(Arc::clone(&engine).run_chunk_engine());
    Instance { engine, store, events, address, key }
}

async fn wait_for<T>(
    events: &mut UnboundedReceiver<Event>,
    seconds: u64,
    mut extract: impl FnMut(&Event) -> Option<T>,
) -> Option<T> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
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
async fn a_second_device_joins_a_profile_and_receives_its_keys() {
    let directory = Arc::new(StaticDirectory::new());
    let laptop = start(Arc::clone(&directory)).await;
    let mut phone = start(Arc::clone(&directory)).await;

    let profile = laptop.engine.create_profile("Ada", "laptop").expect("profile");

    let code = laptop.engine.create_sync_code().expect("code");
    Arc::clone(&phone.engine).request_to_sync(&code).await.expect("asks to join");

    let created = wait_for(&mut phone.events, 10, |event| match event {
        Event::ProfileCreated { user, .. } => Some(user.clone()),
        _ => None,
    })
    .await
    .expect("the phone adopts the profile");

    // The same person, not a new one.
    assert_eq!(created.id, profile.id);
    assert_eq!(created.name, "Ada");

    let adopted = phone.store.profile().unwrap().expect("stored");
    assert!(!adopted.private_ecdsa_key.is_empty(), "the private keys must come across");
    assert!(!adopted.private_ecdh_key.is_empty());
    assert_eq!(adopted.private_ecdsa_key, profile.private_ecdsa_key);

    // Both devices are in the group, on both sides, and the group validates on
    // its own — which is what lets a contact trust it without having been here.
    let on_phone = phone.store.profile().unwrap().unwrap();
    let on_laptop = laptop.store.profile().unwrap().unwrap();
    assert_eq!(on_phone.devices.len(), 2, "the phone should see both devices");
    assert_eq!(on_laptop.devices.len(), 2, "the laptop should see both devices");
    assert!(libbounce::device_group::user_has_valid_device_group(&on_phone));
    assert!(libbounce::device_group::user_has_valid_device_group(&on_laptop));
}

#[tokio::test]
async fn a_pairing_secret_works_exactly_once() {
    let directory = Arc::new(StaticDirectory::new());
    let laptop = start(Arc::clone(&directory)).await;
    let mut phone = start(Arc::clone(&directory)).await;
    let mut tablet = start(Arc::clone(&directory)).await;

    laptop.engine.create_profile("Ada", "laptop").unwrap();
    let code = laptop.engine.create_sync_code().unwrap();

    Arc::clone(&phone.engine).request_to_sync(&code).await.unwrap();
    wait_for(&mut phone.events, 10, |event| {
        matches!(event, Event::ProfileCreated { .. }).then_some(())
    })
    .await
    .expect("the first device joins");

    // The same secret, replayed by somebody who saw it.
    Arc::clone(&tablet.engine).request_to_sync(&code).await.unwrap();
    let joined = wait_for(&mut tablet.events, 2, |event| {
        matches!(event, Event::ProfileCreated { .. }).then_some(())
    })
    .await;

    assert!(joined.is_none(), "a spent secret must not admit a second device");
    assert!(tablet.store.profile().unwrap().is_none());
    assert_eq!(
        laptop.store.profile().unwrap().unwrap().devices.len(),
        2,
        "the device group must not have grown",
    );
}

#[tokio::test]
async fn a_contact_code_cannot_be_redeemed_for_the_private_keys() {
    // The two codes look identical and grant wildly different things: one makes
    // somebody a contact, the other hands over the profile's private keys. Go
    // keeps them in separate tables; the port had a single `pairing_offers`,
    // and reusing it here would have meant a code shown to a stranger so they
    // could message you could instead be used to join your device group.
    let directory = Arc::new(StaticDirectory::new());
    let laptop = start(Arc::clone(&directory)).await;
    let mut attacker = start(Arc::clone(&directory)).await;

    laptop.engine.create_profile("Ada", "laptop").unwrap();

    // An "add me as a contact" code, offered in good faith.
    let contact_code = laptop.engine.create_pairing_code().expect("contact code");

    // Presented to the device-pairing flow instead.
    Arc::clone(&attacker.engine)
        .request_to_sync(&contact_code)
        .await
        .expect("the request itself is sent; it is the answer that must refuse");

    let joined = wait_for(&mut attacker.events, 2, |event| {
        matches!(event, Event::ProfileCreated { .. }).then_some(())
    })
    .await;

    assert!(joined.is_none(), "a contact code must never yield the profile");
    assert!(attacker.store.profile().unwrap().is_none());
    assert_eq!(laptop.store.profile().unwrap().unwrap().devices.len(), 1);
}

#[tokio::test]
async fn a_device_that_already_has_a_profile_refuses_to_join_another() {
    let directory = Arc::new(StaticDirectory::new());
    let laptop = start(Arc::clone(&directory)).await;
    let phone = start(Arc::clone(&directory)).await;

    laptop.engine.create_profile("Ada", "laptop").unwrap();
    let mine = phone.engine.create_profile("Bo", "phone").unwrap();

    let code = laptop.engine.create_sync_code().unwrap();
    let result = Arc::clone(&phone.engine).request_to_sync(&code).await;

    assert!(result.is_err(), "a device belongs to exactly one person");
    assert_eq!(phone.store.profile().unwrap().unwrap().id, mine.id);
}

#[tokio::test]
async fn an_unsolicited_acceptance_cannot_replace_a_profile() {
    // Nothing about the acceptance frame is signed as a whole — it is trusted
    // because it answers a request we made. A device that already has a profile
    // made no such request.
    let directory = Arc::new(StaticDirectory::new());
    let laptop = start(Arc::clone(&directory)).await;
    let phone = start(Arc::clone(&directory)).await;

    let theirs = laptop.engine.create_profile("Ada", "laptop").unwrap();
    let mine = phone.engine.create_profile("Bo", "phone").unwrap();

    let accepted = libbounce::frames::pairing::SyncDeviceRequestAccepted {
        profile: laptop.store.profile().unwrap().unwrap(),
        private_ecdh_key: vec![1; 32],
        public_ecdh_key: vec![2; 32],
        private_ecdsa_key: vec![3; 32],
        public_ecdsa_key: vec![4; 32],
        settings: None,
        references: false,
    };

    let result = phone
        .engine
        .handle_frame(
            &laptop.address,
            libbounce::wire::RawFrame::new(
                libbounce::types::FrameType::SyncDeviceRequestAccepted.as_u16(),
                accepted.encode().unwrap(),
            ),
        )
        .await;

    assert!(result.is_err(), "an unrequested profile must be refused");
    assert_eq!(phone.store.profile().unwrap().unwrap().id, mine.id);
    assert_ne!(mine.id, theirs.id);
}

#[tokio::test]
async fn revoking_a_device_reaches_a_contact_and_stops_it_being_trusted() {
    // A revocation is the one device change that goes to everybody. Until a
    // contact learns of it they keep accepting frames signed by the revoked
    // device as genuinely ours, which is the whole thing revoking is for.
    let directory = Arc::new(StaticDirectory::new());
    let laptop = start(Arc::clone(&directory)).await;
    let mut phone = start(Arc::clone(&directory)).await;
    let mut contact = start(Arc::clone(&directory)).await;

    let ada = laptop.engine.create_profile("Ada", "laptop").unwrap();
    contact.engine.create_profile("Bo", "phone").unwrap();

    // Ada pairs a second device.
    let code = laptop.engine.create_sync_code().unwrap();
    Arc::clone(&phone.engine).request_to_sync(&code).await.unwrap();
    wait_for(&mut phone.events, 10, |event| {
        matches!(event, Event::ProfileCreated { .. }).then_some(())
    })
    .await
    .expect("the phone joins");

    // Bo learns Ada's device group by being introduced to her.
    let bo_code = contact.engine.create_pairing_code().unwrap();
    Arc::clone(&laptop.engine).request_to_add_user(&bo_code).await.unwrap();
    wait_for(&mut contact.events, 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("Bo adds Ada");
    tokio::time::sleep(Duration::from_millis(300)).await;

    let phone_address = phone.address.clone();
    let phone_device = contact
        .store
        .devices_for_user(ada.id)
        .unwrap()
        .into_iter()
        .find(|device| device.address == phone_address)
        .expect("Bo should know about Ada's phone");
    assert!(!phone_device.is_revoked(), "not revoked yet");

    // Ada loses the phone and revokes it from the laptop.
    Arc::clone(&laptop.engine).connect(&contact.address).await.ok();
    tokio::time::sleep(Duration::from_millis(150)).await;
    laptop.engine.revoke_device(phone_device.id).await.expect("revokes");
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Locally it is revoked...
    let locally = laptop
        .store
        .device_by_address(&phone_address)
        .unwrap()
        .expect("kept, not deleted");
    assert!(locally.is_revoked(), "the laptop must mark it revoked");

    // ...and so is Bo's copy, which is the half that was missing.
    let at_contact = contact
        .store
        .device_by_address(&phone_address)
        .unwrap()
        .expect("Bo keeps the row");
    assert!(
        at_contact.is_revoked(),
        "a contact must learn of the revocation, or they keep trusting the device",
    );
}

#[tokio::test]
async fn the_last_device_cannot_be_revoked() {
    // Revoking it would leave the profile unable to sign anything at all,
    // including the frame saying it had been revoked.
    let directory = Arc::new(StaticDirectory::new());
    let laptop = start(directory).await;
    let ada = laptop.engine.create_profile("Ada", "laptop").unwrap();

    let only = laptop.store.devices_for_user(ada.id).unwrap();
    assert_eq!(only.len(), 1);

    assert!(laptop.engine.revoke_device(only[0].id).await.is_err());
    assert!(!laptop
        .store
        .device_by_address(&laptop.address)
        .unwrap()
        .unwrap()
        .is_revoked());
}

#[tokio::test]
async fn one_user_cannot_revoke_anothers_device() {
    let directory = Arc::new(StaticDirectory::new());
    let ada = start(Arc::clone(&directory)).await;
    let mut bo = start(Arc::clone(&directory)).await;

    ada.engine.create_profile("Ada", "laptop").unwrap();
    let bo_user = bo.engine.create_profile("Bo", "laptop").unwrap();

    let code = bo.engine.create_pairing_code().unwrap();
    Arc::clone(&ada.engine).request_to_add_user(&code).await.unwrap();
    wait_for(&mut bo.events, 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("introduced");

    let bo_device = ada
        .store
        .devices_for_user(bo_user.id)
        .unwrap()
        .into_iter()
        .next()
        .expect("Ada knows Bo's device");

    // Ada tries to revoke a device that is not hers.
    assert!(ada.engine.revoke_device(bo_device.id).await.is_err());
    assert!(!bo
        .store
        .device_by_address(&bo.address)
        .unwrap()
        .unwrap()
        .is_revoked());
}

#[tokio::test]
async fn revoking_a_device_replaces_the_profile_keys() {
    // Revoking stops a device signing as us from now on. It does not make it
    // forget the profile keys it already holds, so a stolen device would keep
    // every ability that depended on them. Go ends `RevokeDevice` with
    // `rollKeys()` for exactly this reason.
    let directory = Arc::new(StaticDirectory::new());
    let laptop = start(Arc::clone(&directory)).await;
    let mut phone = start(Arc::clone(&directory)).await;

    let before = laptop.engine.create_profile("Ada", "laptop").unwrap();
    assert!(!before.private_ecdsa_key.is_empty());

    let code = laptop.engine.create_sync_code().unwrap();
    Arc::clone(&phone.engine).request_to_sync(&code).await.unwrap();
    wait_for(&mut phone.events, 10, |event| {
        matches!(event, Event::ProfileCreated { .. }).then_some(())
    })
    .await
    .expect("the phone joins");

    // The phone now holds the same private keys the laptop does — that is what
    // pairing hands over, and what makes revocation insufficient on its own.
    let shared = phone.store.profile().unwrap().unwrap();
    assert_eq!(shared.private_ecdsa_key, before.private_ecdsa_key);

    let phone_device = laptop
        .store
        .devices_for_user(before.id)
        .unwrap()
        .into_iter()
        .find(|device| device.address == phone.address)
        .expect("the laptop knows the phone");

    laptop.engine.revoke_device(phone_device.id).await.expect("revokes");

    let after = laptop.store.profile().unwrap().unwrap();
    assert_ne!(
        after.private_ecdsa_key, before.private_ecdsa_key,
        "the signing key must be replaced",
    );
    assert_ne!(
        after.private_ecdh_key, before.private_ecdh_key,
        "the agreement key must be replaced too",
    );
    assert_ne!(after.public_ecdh_key, before.public_ecdh_key);
    assert!(!after.private_ecdsa_key.is_empty(), "and must still be usable");

    // The device group is untouched: an address *is* a device key, so rolling
    // those would rename every device and orphan the group.
    assert_eq!(
        laptop.store.profile().unwrap().unwrap().devices.len(),
        2,
        "revoked devices are kept, and nothing else changed",
    );
    assert_eq!(laptop.engine.address(), laptop.address);
}

#[tokio::test]
async fn a_setting_changed_on_one_device_reaches_the_other() {
    // Profile-wide preferences are one person's, so every device they own
    // needs them. Until a second device could exist this was invisible; now a
    // change made on the laptop that never reaches the phone leaves one person
    // with two answers to the same question.
    let directory = Arc::new(StaticDirectory::new());
    let laptop = start(Arc::clone(&directory)).await;
    let mut phone = start(Arc::clone(&directory)).await;

    laptop.engine.create_profile("Ada", "laptop").unwrap();
    let code = laptop.engine.create_sync_code().unwrap();
    Arc::clone(&phone.engine).request_to_sync(&code).await.unwrap();
    wait_for(&mut phone.events, 10, |event| {
        matches!(event, Event::ProfileCreated { .. }).then_some(())
    })
    .await
    .expect("the phone joins");

    let before = phone.engine.settings().expect("reads settings");
    assert!(before.default_read_receipts, "the default we are about to change");

    laptop
        .engine
        .set_default_read_receipts(false)
        .await
        .expect("changes it on the laptop");

    let announced = wait_for(&mut phone.events, 10, |event| match event {
        Event::SettingsUpdated { settings } => Some(settings.clone()),
        _ => None,
    })
    .await
    .expect("the phone is told");

    assert!(!announced.default_read_receipts);
    assert!(
        !phone.engine.settings().unwrap().default_read_receipts,
        "and it is stored, not merely announced",
    );
}

#[tokio::test]
async fn a_contact_cannot_change_our_settings() {
    // Settings are sync-scoped, so one should never arrive from a contact at
    // all — but the handler is what makes that a rule rather than an
    // assumption about who can reach us.
    let directory = Arc::new(StaticDirectory::new());
    let ada = start(Arc::clone(&directory)).await;
    let mut bo = start(Arc::clone(&directory)).await;

    ada.engine.create_profile("Ada", "laptop").unwrap();
    bo.engine.create_profile("Bo", "laptop").unwrap();

    let code = bo.engine.create_pairing_code().unwrap();
    Arc::clone(&ada.engine).request_to_add_user(&code).await.unwrap();
    wait_for(&mut bo.events, 10, |event| {
        matches!(event, Event::UserAdded { .. }).then_some(())
    })
    .await
    .expect("introduced");

    let before = bo.engine.settings().unwrap().default_read_receipts;

    // Ada signs a settings change and aims it at Bo.
    let mut update = libbounce::frames::update::UpdateSettings::new(
        libbounce::frames::update::UpdateSettingsType::DefaultReadReceipts,
        vec![0],
        libbounce::now(),
    );
    let body = libbounce::msgpack::to_vec(&update).unwrap();
    let container = libbounce::signed::SignedContainer::create(&ada.key, body);
    update.signed = libbounce::frames::SignedFrame::from_container(&container);

    let result = bo
        .engine
        .handle_frame(
            &ada.address,
            libbounce::wire::RawFrame::new(
                libbounce::types::FrameType::UpdateSettings.as_u16(),
                update.payload().unwrap(),
            ),
        )
        .await;

    assert!(result.is_err(), "a contact's device may not reconfigure us");
    assert_eq!(bo.engine.settings().unwrap().default_read_receipts, before);
}

#[tokio::test]
async fn the_joining_device_sees_the_one_that_admitted_it_as_connected() {
    // Both devices are on the same socket, so both must say the same thing
    // about it. What they said instead was the opposite: the laptop showed the
    // phone as connected while the phone showed the laptop as "Never
    // connected", forever, because nothing on the phone ever credited the
    // session it had joined over.
    //
    // `serve_peer` decides whether a peer is a known device once, when the
    // socket opens, and only then records `last_seen` and announces the device
    // online. Pairing is precisely the case where the peer becomes known
    // *during* the session — over that very socket — so the phone's window
    // for crediting the laptop had already closed by the time it knew who the
    // laptop was. Go re-checks on every frame and re-stamps `last_seen`
    // (`chat/remote_device.go:197-222`).
    //
    // It cannot heal on its own either: peering skips any address already
    // connected, so as long as the pairing socket stays up neither side dials
    // again, and no second session ever runs the code that would fix it.
    let directory = Arc::new(StaticDirectory::new());
    let laptop = start(Arc::clone(&directory)).await;
    let mut phone = start(Arc::clone(&directory)).await;

    laptop.engine.create_profile("Ada", "laptop").unwrap();
    let code = laptop.engine.create_sync_code().unwrap();
    Arc::clone(&phone.engine).request_to_sync(&code).await.unwrap();
    wait_for(&mut phone.events, 10, |event| {
        matches!(event, Event::ProfileCreated { .. }).then_some(())
    })
    .await
    .expect("the phone joins");

    // The pairing socket is still open at this point, on both sides.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let stored = phone
        .store
        .device_by_address(&laptop.address)
        .unwrap()
        .expect("the phone knows the laptop");
    assert_ne!(
        stored.last_seen, 0,
        "the phone must record having been connected to the laptop",
    );

    // And the snapshot a client rebuilds from has to agree with the events it
    // was sent. The phone reloads it the moment pairing completes — the
    // profile it did not have now exists — so a snapshot that reports every
    // device offline overwrites the truth rather than confirming it.
    for (side, state, peer) in [
        ("phone", phone.engine.initial_state().unwrap(), &laptop.address),
        ("laptop", laptop.engine.initial_state().unwrap(), &phone.address),
    ] {
        let view = state
            .sync_devices
            .iter()
            .find(|device| &device.address == peer)
            .unwrap_or_else(|| panic!("{side} lists the other device"));
        assert!(
            view.online,
            "{side} should report the device it is connected to as online",
        );
    }
}

#[tokio::test]
async fn renaming_a_device_does_not_report_it_offline() {
    // `DeviceUpdated` carries a whole device record and the client replaces
    // its own with it, so anything the view gets wrong is not a momentary
    // glitch — it sticks until the next event says otherwise. The rename path
    // knew only whether the device was this one, so it answered the presence
    // question with that, and renaming the phone from the laptop greyed the
    // phone out until the session ended.
    let directory = Arc::new(StaticDirectory::new());
    let mut laptop = start(Arc::clone(&directory)).await;
    let mut phone = start(Arc::clone(&directory)).await;

    let profile = laptop.engine.create_profile("Ada", "laptop").unwrap();
    let code = laptop.engine.create_sync_code().unwrap();
    Arc::clone(&phone.engine).request_to_sync(&code).await.unwrap();
    wait_for(&mut phone.events, 10, |event| {
        matches!(event, Event::ProfileCreated { .. }).then_some(())
    })
    .await
    .expect("the phone joins");

    let phone_device = laptop
        .store
        .devices_for_user(profile.id)
        .unwrap()
        .into_iter()
        .find(|device| device.address == phone.address)
        .expect("the laptop knows the phone");

    laptop.engine.rename_device(phone_device.id, "phone").expect("renames");

    let updated = wait_for(&mut laptop.events, 10, |event| match event {
        Event::DeviceUpdated { device } if device.id == phone_device.id => Some(device.clone()),
        _ => None,
    })
    .await
    .expect("the laptop hears about its own rename");

    assert_eq!(updated.name, "phone");
    assert!(!updated.local);
    assert!(
        updated.online,
        "a rename must not knock a connected device offline",
    );
}

#[tokio::test]
async fn a_joining_device_receives_the_profile_picture() {
    // A device that joins later has no history at all: everything it knows
    // arrives through the reference flow. So anything that flow declines to
    // offer is not merely delayed on a new device, it is absent — which is how
    // a freshly linked device came to show every avatar as a blank circle
    // while the device that admitted it showed them all.
    let directory = Arc::new(StaticDirectory::new());
    let laptop = start(Arc::clone(&directory)).await;
    let mut phone = start(Arc::clone(&directory)).await;

    laptop.engine.create_profile("Ada", "laptop").unwrap();

    let picture: Vec<u8> = (0..7000).map(|index| (index % 251) as u8).collect();
    laptop
        .engine
        .set_profile_image(libbounce::engine::files::OutgoingAttachment {
            name: "ada.png".into(),
            data: picture.clone(),
            is_image: true,
            width: 96,
            height: 96,
            blur_hash: String::new(),
            ..Default::default()
        })
        .await
        .expect("sets the picture");

    let code = laptop.engine.create_sync_code().unwrap();
    Arc::clone(&phone.engine).request_to_sync(&code).await.unwrap();

    let joined = wait_for(&mut phone.events, 15, |event| match event {
        Event::ProfileCreated { user, .. } => Some(user.clone()),
        _ => None,
    })
    .await
    .expect("the phone joins");

    // The profile record names the picture, as it did before — that half was
    // never the problem.
    let image_id = *joined.images.last().expect("the profile names a picture");

    let complete = wait_for(&mut phone.events, 20, |event| match event {
        Event::FileComplete { file_id } if *file_id == image_id => Some(*file_id),
        _ => None,
    })
    .await;

    assert_eq!(complete, Some(image_id), "and the bytes have to follow it");
    assert_eq!(
        phone.engine.file_data(image_id).unwrap().as_deref(),
        Some(picture.as_slice()),
    );
}

#[tokio::test]
async fn a_joining_device_learns_who_the_profile_knows() {
    // `apply_add_user` is documented as "the one path that both sides and every
    // later-arriving device take", and `AddUser` sorts first in the catch-up
    // order table so a contact is established before anything that refers to
    // them. Neither was reachable: the reference flow never listed `add_users`
    // among the tables it draws from, `peer_may_have` had no arm for the type,
    // the store could not produce its payload, and the catch-up dispatcher had
    // nowhere to send it.
    //
    // So a device that joined later did not learn the contacts — and since a
    // frame is only accepted from a device that speaks for a known user,
    // everything those contacts had ever said was then rejected on arrival.
    let directory = Arc::new(StaticDirectory::new());
    let laptop = start(Arc::clone(&directory)).await;
    let carol = start(Arc::clone(&directory)).await;
    let mut phone = start(Arc::clone(&directory)).await;

    laptop.engine.create_profile("Ada", "laptop").unwrap();
    let carol_user = carol.engine.create_profile("Carol", "carol").unwrap();

    let code = carol.engine.create_pairing_code().expect("code");
    Arc::clone(&laptop.engine).request_to_add_user(&code).await.expect("adds carol");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        laptop.store.user(carol_user.id).unwrap().is_some(),
        "the laptop knows Carol before the phone joins",
    );

    let sync = laptop.engine.create_sync_code().unwrap();
    Arc::clone(&phone.engine).request_to_sync(&sync).await.unwrap();
    wait_for(&mut phone.events, 15, |event| {
        matches!(event, Event::ProfileCreated { .. }).then_some(())
    })
    .await
    .expect("the phone joins");
    tokio::time::sleep(Duration::from_millis(500)).await;

    let known = phone.store.user(carol_user.id).unwrap();
    assert!(known.is_some(), "the phone must be told who the profile knows");
    assert_eq!(known.unwrap().name, "Carol");

    // And her devices with her, or nothing she signs can be attributed.
    assert!(
        !phone.store.devices_for_user(carol_user.id).unwrap().is_empty(),
        "a contact without devices is a contact nothing can be accepted from",
    );
}

#[tokio::test]
async fn a_contact_learns_about_a_newly_linked_device() {
    // `handle_sync_device_request` broadcasts the new `Device` frame to
    // everyone, because a contact who does not know about a device will refuse
    // everything that device signs. Nothing received it: this port had no
    // handler for `FrameType::Device`, so a frame it broadcast itself landed in
    // the dispatcher's "no handler for frame type yet" arm on every peer.
    let directory = Arc::new(StaticDirectory::new());
    let laptop = start(Arc::clone(&directory)).await;
    let bob = start(Arc::clone(&directory)).await;
    let mut phone = start(Arc::clone(&directory)).await;

    let ada = laptop.engine.create_profile("Ada", "laptop").unwrap();
    bob.engine.create_profile("Bob", "bob").unwrap();

    let code = bob.engine.create_pairing_code().expect("code");
    Arc::clone(&laptop.engine).request_to_add_user(&code).await.expect("adds bob");
    tokio::time::sleep(Duration::from_millis(500)).await;

    assert_eq!(
        bob.store.devices_for_user(ada.id).unwrap().len(),
        1,
        "Bob starts knowing only the laptop",
    );

    let sync = laptop.engine.create_sync_code().unwrap();
    Arc::clone(&phone.engine).request_to_sync(&sync).await.unwrap();
    wait_for(&mut phone.events, 15, |event| {
        matches!(event, Event::ProfileCreated { .. }).then_some(())
    })
    .await
    .expect("the phone joins");
    tokio::time::sleep(Duration::from_millis(800)).await;

    let known = bob.store.devices_for_user(ada.id).unwrap();
    assert_eq!(known.len(), 2, "Bob must learn about Ada's new device");
    assert!(known.iter().any(|device| device.address == phone.address));

    // And the group Bob now holds still validates, which is what makes the
    // introduction chain worth carrying at all.
    let ada_as_bob_sees_her = bob.store.user(ada.id).unwrap().expect("known");
    assert!(libbounce::device_group::user_has_valid_device_group(
        &ada_as_bob_sees_her
    ));
}

#[tokio::test]
async fn a_setting_changed_while_a_sibling_was_away_reaches_it_on_reconnection() {
    // Live delivery of an `UpdateSettings` already worked; replay did not.
    // `update_settings` was absent from the reference sources, so a change made
    // while another of your devices was off left the two disagreeing
    // permanently — the frame was stored, could be served on request, and was
    // never offered to anybody.
    let directory = Arc::new(StaticDirectory::new());
    let laptop = start(Arc::clone(&directory)).await;
    let mut phone = start(Arc::clone(&directory)).await;

    laptop.engine.create_profile("Ada", "laptop").unwrap();
    let code = laptop.engine.create_sync_code().unwrap();
    Arc::clone(&phone.engine).request_to_sync(&code).await.unwrap();
    wait_for(&mut phone.events, 15, |event| {
        matches!(event, Event::ProfileCreated { .. }).then_some(())
    })
    .await
    .expect("the phone joins");

    assert!(phone.engine.settings().unwrap().default_read_receipts);

    // The phone goes away, and the laptop changes its mind.
    phone.engine.disconnect_all().await;
    laptop.engine.disconnect_all().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    laptop
        .engine
        .set_default_read_receipts(false)
        .await
        .expect("changes the setting");

    // Reconnecting is the only route left.
    Arc::clone(&phone.engine).connect(&laptop.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;

    assert!(
        !phone.engine.settings().unwrap().default_read_receipts,
        "the phone must catch up on a setting it missed",
    );
}

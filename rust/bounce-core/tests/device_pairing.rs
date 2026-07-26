//! Adding a second device to a profile.
//!
//! The whole of the authentication is a secret read off one screen and two
//! signatures, so these tests care less about the happy path than about what
//! the flow refuses.

use std::sync::Arc;
use std::time::Duration;

use bounce_core::crypto::DeviceKey;
use bounce_core::engine::{Engine, Event};
use bounce_core::frames::Broadcastable;
use bounce_core::net::{StaticDirectory, TcpNetwork};
use bounce_core::store::Store;
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
    assert!(bounce_core::device_group::user_has_valid_device_group(&on_phone));
    assert!(bounce_core::device_group::user_has_valid_device_group(&on_laptop));
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

    let accepted = bounce_core::frames::pairing::SyncDeviceRequestAccepted {
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
            bounce_core::wire::RawFrame::new(
                bounce_core::types::FrameType::SyncDeviceRequestAccepted.as_u16(),
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
    let mut update = bounce_core::frames::update::UpdateSettings::new(
        bounce_core::frames::update::UpdateSettingsType::DefaultReadReceipts,
        vec![0],
        bounce_core::now(),
    );
    let body = bounce_core::msgpack::to_vec(&update).unwrap();
    let container = bounce_core::signed::SignedContainer::create(&ada.key, body);
    update.signed = bounce_core::frames::SignedFrame::from_container(&container);

    let result = bo
        .engine
        .handle_frame(
            &ada.address,
            bounce_core::wire::RawFrame::new(
                bounce_core::types::FrameType::UpdateSettings.as_u16(),
                update.payload().unwrap(),
            ),
        )
        .await;

    assert!(result.is_err(), "a contact's device may not reconfigure us");
    assert_eq!(bo.engine.settings().unwrap().default_read_receipts, before);
}

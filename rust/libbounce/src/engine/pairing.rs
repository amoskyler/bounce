//! Adding a second device to a profile.
//!
//! A profile is a keypair and a device group. Adding a device means proving to
//! an existing one that you are standing next to it, and receiving the profile
//! — private keys included — in return. There is no server to mediate it and no
//! password to fall back on, so the whole of the authentication is a secret
//! shown on one screen and typed into the other, plus two signatures.
//!
//! ```text
//!   existing device                        joining device
//!     │  shows "address:secret"  ─ ─ ─ ─ ─▶ │  (read off the screen)
//!     │ ◀── SyncDeviceRequest{sig, secret}  │
//!     │  burn the secret, check the sig     │
//!     │  write the device row               │
//!     │  SyncDeviceRequestAccepted ───────▶ │  profile + private keys
//!     │  broadcast the new Device           │  save, then peer and catch up
//!     │  send a reference offer ──────────▶ │
//! ```
//!
//! ## What the two signatures are for
//!
//! The request carries the joining device's signature over the *existing*
//! device's address, and the acceptance records the existing device's signature
//! over the *joining* one's. Together they are an
//! [`IntroductionSignature`](crate::frames::identity::IntroductionSignature):
//! a self-verifying record that both devices consented, which every other
//! device in the group — and every contact — can check without having been
//! present. That is what lets a device group be validated by a third party
//! rather than merely asserted.
//!
//! Signing the *counterparty's* address in each direction is what stops a
//! replay: a signature harvested from one pairing names the device it was made
//! for and is worthless anywhere else.
//!
//! ## The secret is single use
//!
//! It is burned before the signature is checked, not after, so a wrong guess
//! costs the attacker the secret rather than giving them another try. Offers
//! expire after five minutes, and displaying a new one replaces
//! any outstanding offer — there is never more than one live at a time.

use std::sync::Arc;

use uuid::Uuid;

use crate::device_group;
use crate::error::{Error, Result};
use crate::frames::identity::{Device, IntroductionSignature, KeySet, User};
use crate::frames::update::{UpdateDevice, UpdateDeviceType, UpdateUser, UpdateUserType};
use crate::frames::SignedFrame;
use crate::frames::pairing::{
    SyncDeviceOffer, SyncDeviceRequest, SyncDeviceRequestAccepted, SyncDeviceRequestRejected,
};
use crate::net::Network;
use crate::types::FrameType;
use crate::signed::SignedContainer;
use crate::wire::RawFrame;

use super::{Engine, Event};

impl<N: Network + 'static> Engine<N> {
    // ---------------------------------------------------------------------
    // On the device that already has the profile
    // ---------------------------------------------------------------------

    /// Produce the string to show on screen so another device can join.
    ///
    /// Displaying one invalidates whatever was displayed before: only a single
    /// offer is ever outstanding, so a secret glimpsed over somebody's shoulder
    /// an hour ago is not still live.
    pub fn create_sync_code(&self) -> Result<String> {
        if self.store.profile()?.is_none() {
            return Err(Error::NoProfile);
        }

        let offer = SyncDeviceOffer {
            id: Uuid::new_v4(),
            timestamp: crate::now(),
            secret: hex::encode(crate::crypto::random_bytes(16)),
        };
        self.store.replace_sync_offer(&offer)?;

        Ok(format!("{}:{}", self.network.address(), offer.secret))
    }

    /// A device is asking to join this profile.
    pub(super) async fn handle_sync_device_request(
        &self,
        peer: &str,
        payload: &[u8],
    ) -> Result<()> {
        let request: SyncDeviceRequest = crate::msgpack::from_slice(payload)?;

        // A secret already spent is ignored in silence rather than rejected:
        // answering would confirm to whoever replayed it that it was once real.
        // The offer row alone is not enough to decide this — it is deleted on
        // use, and a deleted row is indistinguishable from one that never
        // existed, which is precisely what let a spent secret admit a second
        // device.
        if self.store.secret_is_burned(&request.secret)? {
            return Ok(());
        }

        let Some(offer) = self.store.sync_offer_by_secret(&request.secret)? else {
            self.reject_sync(peer).await;
            return Err(Error::NotPermitted("no such pairing secret"));
        };

        // Burned and deleted before anything below can fail, so a request that
        // gets the signature wrong has still spent the secret rather than
        // being handed another attempt.
        self.store.burn_secret(&request.secret)?;
        self.store.clear_sync_offers()?;

        if offer.is_expired(crate::now()) {
            self.reject_sync(peer).await;
            return Err(Error::NotPermitted("the pairing secret has expired"));
        }

        // The signature must be this peer signing *our* address. A signature
        // over anything else — including one lifted from a different pairing —
        // does not verify here.
        if !crate::crypto::verify_signature(peer, self.network.address().as_bytes(), &request.signature)
        {
            self.reject_sync(peer).await;
            return Err(Error::NotPermitted("pairing signature does not check out"));
        }

        let profile = self.store.profile()?.ok_or(Error::NoProfile)?;

        // A device we already know to belong to somebody else cannot become one
        // of ours, whatever secret it presents.
        if let Some(known) = self.store.device_by_address(peer)? {
            if known.user_id != profile.id {
                return Err(Error::NotPermitted(
                    "that device already belongs to another user",
                ));
            }

            // A device we have already admitted, asking again — its first
            // attempt evidently did not finish. Clearing the delivery records
            // makes the reference flow re-offer everything, since we cannot
            // know how much of it landed.
            self.store.forget_deliveries_to(peer)?;
            self.accept_sync(peer, &profile, false).await?;
            self.offer_references_to(peer).await;
            return Ok(());
        }

        // Both halves of the introduction, so any third device can verify that
        // this pairing happened without having been there.
        let device = Device {
            id: Uuid::new_v4(),
            name: String::new(),
            user_id: profile.id,
            address: peer.to_string(),
            timestamp: crate::now(),
            saved_at: crate::now(),
            last_seen: crate::now(),
            revoked_at: 0,
            // Filled in by the device itself once it is up; there is nothing
            // to encrypt to it yet, and we hold none of its private material.
            ecdh_public_key: Vec::new(),
            ecdh_private_key: Vec::new(),
            // We are recording a device we have only just met, and it has not
            // told us what it speaks. Empty means legacy, so we send it nothing
            // it might not survive until it announces itself.
            capabilities: Vec::new(),
            signature: Some(IntroductionSignature {
                id: Uuid::new_v4(),
                device_id: Uuid::nil(),
                preexisting_device: self.network.address(),
                signature_of_new_device: self.key.sign(peer.as_bytes()).to_vec(),
                signature_of_preexisting_device: request.signature.clone(),
            }),
        };
        self.store.save_device(&device)?;

        // Re-read, so what goes over the wire includes the device we just
        // admitted — the joining side refuses a group it is not in.
        let profile = self.store.profile()?.ok_or(Error::NoProfile)?;
        self.accept_sync(peer, &profile, true).await?;

        self.emit(Event::DeviceAdded {
            device: self.device_view(&device, false, true),
        });

        // Everyone else in the profile, and every contact, needs to know the
        // group has grown, or frames signed by the new device are unattributable.
        self.store.record_delivery(&crate::frames::transport::DeliveryRecord::new(
            peer.to_string(),
            device.id,
            FrameType::Device,
            crate::now(),
        ))?;
        self.broadcast(&device).await?;

        self.offer_references_to(peer).await;
        Ok(())
    }

    /// Hand the profile over, private keys included.
    async fn accept_sync(&self, peer: &str, profile: &User, include_keys: bool) -> Result<()> {
        let settings = self.store.profile_settings(profile.id)?;

        let accepted = SyncDeviceRequestAccepted {
            profile: shareable_profile(profile),
            // Only on first admission. A device asking again already has them,
            // and re-sending key material buys nothing for the risk.
            private_ecdh_key: if include_keys { profile.private_ecdh_key.clone() } else { Vec::new() },
            public_ecdh_key: profile.public_ecdh_key.clone(),
            private_ecdsa_key: if include_keys { profile.private_ecdsa_key.clone() } else { Vec::new() },
            public_ecdsa_key: profile.public_ecdsa_key.clone(),
            settings,
            references: self.has_anything_for(peer)?,
        };

        self.send_to(
            peer,
            RawFrame::new(
                FrameType::SyncDeviceRequestAccepted.as_u16(),
                accepted.encode()?,
            ),
        )
        .await;
        Ok(())
    }

    async fn reject_sync(&self, peer: &str) {
        let Ok(payload) = SyncDeviceRequestRejected {}.encode() else {
            return;
        };
        self.send_to(
            peer,
            RawFrame::new(FrameType::SyncDeviceRequestRejected.as_u16(), payload),
        )
        .await;
    }

    /// Whether we hold anything this device has not seen.
    fn has_anything_for(&self, peer: &str) -> Result<bool> {
        Ok(!self.build_reference_offer(peer)?.references.is_empty())
    }

    async fn offer_references_to(&self, peer: &str) {
        let Ok(offer) = self.build_reference_offer(peer) else {
            return;
        };
        if offer.references.is_empty() {
            return;
        }
        let Ok(payload) = offer.encode() else {
            return;
        };
        self.send_to(peer, RawFrame::new(FrameType::ReferenceOffer.as_u16(), payload))
            .await;
    }

    // ---------------------------------------------------------------------
    // On the device that is joining
    // ---------------------------------------------------------------------

    /// Join an existing profile using a code read off its screen.
    ///
    /// Only possible on a device with no profile of its own: a device belongs
    /// to exactly one person, and there is no merge.
    pub async fn request_to_sync(self: &Arc<Self>, code: &str) -> Result<()> {
        if self.store.profile()?.is_some() {
            return Err(Error::NotPermitted(
                "this device already belongs to a profile",
            ));
        }

        let (address, secret) = Self::parse_pairing_code(code)?;
        if address == self.network.address() {
            return Err(Error::NotPermitted("that is this device's own code"));
        }

        let request = SyncDeviceRequest {
            // Signing *their* address is what makes this consent to join that
            // specific device, and worthless if replayed at another.
            signature: self.key.sign(address.as_bytes()).to_vec(),
            secret,
        };

        Arc::clone(self).connect(&address).await?;
        // Give the session a moment to register before writing to it.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        self.send_to(
            &address,
            RawFrame::new(FrameType::SyncDeviceRequest.as_u16(), request.encode()?),
        )
        .await;
        Ok(())
    }

    /// We have been admitted: adopt the profile we were sent.
    pub(super) async fn handle_sync_device_request_accepted(
        &self,
        peer: &str,
        payload: &[u8],
    ) -> Result<()> {
        let accepted: SyncDeviceRequestAccepted = crate::msgpack::from_slice(payload)?;

        // Unsolicited, or a second copy after we already joined. Either way a
        // profile is not something to overwrite.
        if self.store.profile()?.is_some() {
            return Err(Error::NotPermitted("a profile already exists here"));
        }

        let mut profile = accepted.profile;
        profile.profile = true;
        profile.open_dm = true;
        profile.private_ecdh_key = accepted.private_ecdh_key;
        profile.public_ecdh_key = accepted.public_ecdh_key;
        profile.private_ecdsa_key = accepted.private_ecdsa_key;
        profile.public_ecdsa_key = accepted.public_ecdsa_key;

        // The group has to stand on its own: every device introduced by another
        // device already in it, with both signatures checking out. Without this
        // a peer could hand us a profile whose "device group" is whatever it
        // liked, and we would then vouch for it to everybody else.
        if !device_group::user_has_valid_device_group(&profile) {
            return Err(Error::NotPermitted(
                "the profile offered has an invalid device group",
            ));
        }

        // And we have to be in it. A group that does not contain this device is
        // one we would immediately be unable to sign for.
        let me = self.network.address();
        if !profile.devices.iter().any(|device| device.address == me) {
            return Err(Error::NotPermitted(
                "the profile offered does not include this device",
            ));
        }

        // The device that admitted us must be in it too, or nothing about the
        // exchange is attributable.
        if !profile.devices.iter().any(|device| device.address == peer) {
            return Err(Error::NotPermitted(
                "the profile offered does not include the device that sent it",
            ));
        }

        self.store.save_user(&profile)?;
        if let Some(settings) = accepted.settings {
            self.store.save_profile_settings(&settings)?;
        }

        for device in &profile.devices {
            self.emit(Event::DeviceAdded {
                device: self.device_view(device, device.address == me, device.address == peer),
            });
        }
        self.emit(Event::ProfileCreated {
            user: self.user_view(&profile, true),
            device: profile
                .devices
                .iter()
                .find(|device| device.address == me)
                .map(|device| self.device_view(device, true, true))
                .expect("checked above"),
        });

        // History arrives through the ordinary reference flow; saying it is
        // coming lets the interface show a sync rather than an empty profile.
        if accepted.references {
            self.emit(Event::SyncStarted);
        }
        Ok(())
    }

    /// The other device refused the secret.
    pub(super) async fn handle_sync_device_request_rejected(&self, _peer: &str) -> Result<()> {
        self.emit(Event::Error {
            message: "That pairing code was not accepted. Show a new one and try again.".into(),
        });
        Ok(())
    }
}

/// The profile as it goes to a joining device.
///
/// Private keys travel in their own fields on the frame rather than inside the
/// user record, matching the Go implementation, so they are not written twice
/// and cannot be left in by accident when the record is relayed onward.
fn shareable_profile(profile: &User) -> User {
    let mut shared = profile.clone();
    shared.private_ecdh_key = Vec::new();
    shared.private_ecdsa_key = Vec::new();
    shared
}

// -------------------------------------------------------------------------
// Revocation
// -------------------------------------------------------------------------

impl<N: Network + 'static> Engine<N> {
    /// Take a device out of this profile's device group.
    ///
    /// A revocation is the one device change that goes to *everybody*
    /// (`Scope::Global`), not just to the profile's own devices. Every contact
    /// has to learn about it, because until they do they will keep accepting
    /// frames signed by the revoked device as genuinely ours — which is the
    /// entire point of revoking it.
    ///
    /// Revoked rows are kept forever rather than deleted. The device's
    /// signatures are still part of the group's history, and a validator
    /// walking the introduction chain needs them; what changes is that
    /// `revoked_at` is now set, so `signer_speaks_for` refuses anything the
    /// device signs *after* that moment while still accepting what it signed
    /// before.
    pub async fn revoke_device(&self, device_id: Uuid) -> Result<()> {
        let profile = self.store.profile()?.ok_or(Error::NoProfile)?;

        let Some(device) = profile
            .devices
            .iter()
            .find(|candidate| candidate.id == device_id)
            .cloned()
        else {
            return Err(Error::NotPermitted("that device is not part of this profile"));
        };

        if device.user_id != profile.id {
            return Err(Error::NotPermitted("cannot revoke another user's device"));
        }
        if device.is_revoked() {
            return Err(Error::NotPermitted("that device is already revoked"));
        }

        // Revoking the only device would leave the profile unable to sign
        // anything, including the frame that says it was revoked.
        let active = profile.devices.iter().filter(|d| !d.is_revoked()).count();
        if active <= 1 {
            return Err(Error::NotPermitted("cannot revoke the last device"));
        }

        let mut update = UpdateDevice::new(
            device_id,
            UpdateDeviceType::Revoke,
            Vec::new(),
            crate::now(),
        );
        update.author = profile.id;
        update.saved_at = crate::now();

        let body = crate::msgpack::to_vec(&update)?;
        let container = SignedContainer::create(&self.key, body);
        update.signed = SignedFrame::from_container(&container);

        self.apply_device_revocation(&device.address, update.timestamp)?;
        self.store.save_update_device(&update)?;
        self.broadcast(&update).await?;

        // Revoking stops the device signing as us from now on. It does not
        // make it forget the profile keys it already held, so those have to be
        // replaced or a stolen device keeps every ability that depended on
        // them. Go ends `RevokeDevice` the same way.
        self.roll_keys().await?;
        Ok(())
    }

    /// Re-issue this profile's user-level keys.
    ///
    /// Two frames, because the audiences differ and one of them must never
    /// leave the device group:
    ///
    /// - [`UpdateUserType::ReplaceKeys`] carries the whole set, private halves
    ///   included, and is sync-scoped.
    /// - [`UpdateUserType::ReplaceEcdhPublicKey`] carries only the new public
    ///   key and is global, because contacts encrypt to it.
    ///
    /// The device keys are untouched: a device's address *is* its key, so
    /// rolling those would change every address and orphan the group. What is
    /// replaced is the user-level pair that a revoked device could otherwise
    /// go on using.
    pub async fn roll_keys(&self) -> Result<()> {
        let mut profile = self.store.profile()?.ok_or(Error::NoProfile)?;

        let signing = crate::crypto::DeviceKey::generate();
        let (private_ecdh, public_ecdh) = crate::crypto::generate_x25519_keypair();

        let key_set = KeySet {
            private_ecdsa_key: signing.to_private_bytes(),
            public_ecdsa_key: signing.public_key().to_vec(),
            private_ecdh_key: private_ecdh.to_vec(),
            public_ecdh_key: public_ecdh.to_vec(),
            // Carried even though encrypted devices are not implemented, so a
            // Go peer in this device group is not handed a set it cannot use.
            kek: crate::crypto::random_bytes(32),
        };

        // Not `save_user`: its upsert refuses to touch key columns, so that a
        // relayed record cannot overwrite our own. This is the one write that
        // legitimately replaces them.
        self.store.replace_profile_keys(
            profile.id,
            &key_set.public_ecdsa_key,
            &key_set.private_ecdsa_key,
            &key_set.public_ecdh_key,
            &key_set.private_ecdh_key,
        )?;

        profile.private_ecdsa_key = key_set.private_ecdsa_key.clone();
        profile.public_ecdsa_key = key_set.public_ecdsa_key.clone();
        profile.private_ecdh_key = key_set.private_ecdh_key.clone();
        profile.public_ecdh_key = key_set.public_ecdh_key.clone();

        self.publish_profile_update(
            profile.id,
            UpdateUserType::ReplaceKeys,
            crate::msgpack::to_vec(&key_set)?,
        )
            .await?;
        self.publish_profile_update(
            profile.id,
            UpdateUserType::ReplaceEcdhPublicKey,
            key_set.public_ecdh_key.clone(),
        )
        .await?;

        Ok(())
    }

    /// Sign, store and broadcast one change to this profile.
    async fn publish_profile_update(
        &self,
        my_id: Uuid,
        kind: UpdateUserType,
        data: Vec<u8>,
    ) -> Result<()> {
        let mut update = UpdateUser::new(my_id, kind, data, self.next_profile_update_timestamp(my_id)?);
        update.saved_at = crate::now();

        let body = crate::msgpack::to_vec(&update)?;
        let container = SignedContainer::create(&self.key, body);
        update.signed = SignedFrame::from_container(&container);

        // Stored as well as sent, so a device that was offline for the roll
        // replays it rather than being left on keys nobody else holds.
        self.store.save_update_user(&update)?;
        self.broadcast(&update).await
    }

    /// Somebody's device group changed.
    pub(super) async fn handle_update_device(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let (mut update, signed) = self.unpack_signed::<UpdateDevice>(payload)?;
        update.signed = signed;

        // The signer has to be a device of the user whose group this changes.
        // Without that, anybody could revoke anybody.
        let Some(signing_device) = self.store.device_by_address(&update.signed.signer)? else {
            return Err(Error::InvalidFrame(
                "update device signed by a device we do not know".into(),
            ));
        };
        update.author = signing_device.user_id;

        let Some(target) = self.store.device_by_id(update.target)? else {
            // A device we have never heard of. Storing the update anyway would
            // let it apply if the device turns up later, which is what Go does.
            update.saved_at = crate::now();
            self.store.save_update_device(&update)?;
            self.send_ack(peer, update.id, FrameType::UpdateDevice).await;
            return Ok(());
        };

        // A user may only change their own devices.
        if target.user_id != signing_device.user_id {
            return Err(Error::NotPermitted(
                "a device may only be changed by its own user",
            ));
        }
        if let Some(author) = self.store.user(update.author)? {
            if author.blocked {
                self.send_ack(peer, update.id, FrameType::UpdateDevice).await;
                return Ok(());
            }
        }

        if self.store.has_frame(update.id, FrameType::UpdateDevice)? {
            self.send_ack(peer, update.id, FrameType::UpdateDevice).await;
            return Ok(());
        }

        update.saved_at = crate::now();
        self.store.save_update_device(&update)?;
        self.send_ack(peer, update.id, FrameType::UpdateDevice).await;

        // A name is the owner's private label and never leaves their own device
        // group; a public key is only useful to encrypted devices, which are
        // not implemented. Both are stored and otherwise ignored.
        if matches!(update.kind(), Ok(UpdateDeviceType::Revoke)) {
            self.apply_device_revocation(&target.address, update.timestamp)?;
        }

        self.broadcast(&update).await?;
        Ok(())
    }

    /// Mark a device revoked and stop talking to it.
    fn apply_device_revocation(&self, address: &str, at: i64) -> Result<()> {
        self.store.revoke_device(address, at)?;

        if let Ok(Some(device)) = self.store.device_by_address(address) {
            self.emit(Event::DeviceUpdated {
                device: self.device_view(&device, false, false),
            });
        }
        Ok(())
    }
}

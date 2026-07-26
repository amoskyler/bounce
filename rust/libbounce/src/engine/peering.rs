//! Keeping connections open to the devices we need to reach.
//!
//! There is no server. A message reaches somebody only if this device has a
//! socket open to one of their devices at the moment it is sent, or opens one
//! later and replays through the reference flow. Nothing establishes those
//! sockets on its own — so something has to, continuously, and that is this.
//!
//! ## What went wrong without it
//!
//! [`Engine::connect`] had exactly one caller in the shipped product: the
//! add-user flow. A restart therefore left the client accept-only. Its own
//! onion service was still published under the same key, so a *peer running
//! the Go client* would dial in on its next audit and everything looked fine;
//! two Electron clients, neither of which dialled, simply never spoke again.
//! Re-adding a contact appeared to fix it because the pairing flow dials —
//! and the fix lasted exactly until the next restart.
//!
//! ## The policy, from the Go implementation
//!
//! `chat/device_pool.go` and the Peering section of `docs/design.md`:
//!
//! - dial this profile's own devices always — sync traffic is the one thing
//!   that must never be partitioned;
//! - dial the devices of users and groups active in the last four weeks, up to
//!   [`CONNECTIONS_PER_THREAD`] per conversation, chosen at random;
//! - dial *anyone at all* we are still holding undelivered frames for, however
//!   long it has been, because the alternative is that the frames never leave;
//! - re-audit every [`AUDIT_INTERVAL`], filling in whatever has dropped;
//! - keep each socket alive with a frame every [`KEEP_ALIVE_INTERVAL`], since
//!   an idle Tor circuit is closed under us;
//! - back off a device that was dialled recently, and back off much harder
//!   from one that refused.
//!
//! The random choice is not decoration. Every device in a group is a valid
//! route to it, and if every member deterministically dialled the same few,
//! the group would bifurcate into cliques that gossip within themselves and
//! not across.
//!
//! ## Why not simply dial everyone, every time
//!
//! Because it is not needed, and it is not free.
//!
//! Not needed, because frames gossip. Every device in a conversation relays to
//! every other device in scope, and each new connection opens with a reference
//! offer that backfills whatever the peer missed. One live socket into a
//! conversation is therefore enough to learn everything in it; four is
//! redundancy against the one you picked being offline. Reaching is symmetric
//! too — the listener is always up, so a contact who wants us dials in whether
//! or not we dialled them.
//!
//! Not free, because every dial is a Tor circuit: six hops, seconds to build,
//! and real capacity on the network. A device with two hundred contacts would
//! spend minutes of every start-up on circuits to people it is not talking to.
//!
//! So recency bounds the *routine* dialling, and undelivered content overrides
//! it — if we are holding something for somebody, we go and find them no
//! matter how long it has been.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use rand::seq::SliceRandom;
use uuid::Uuid;

use crate::error::Result;
use crate::frames::transport::KeepAlive;
use crate::net::Network;
use crate::types::FrameType;
use crate::wire::RawFrame;

use super::Engine;

/// How often the set of open connections is re-examined.
const AUDIT_INTERVAL: Duration = Duration::from_secs(60);

/// How often a frame is written to each peer to hold the circuit open.
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);

/// How long to leave a device alone after dialling it.
const DIAL_COOLDOWN_SECONDS: i64 = 30;

/// How long to leave a device alone after a dial *failed*.
///
/// Much longer, because the usual reason is that the device is simply not
/// running, and an onion dial that will not succeed is expensive — it holds a
/// circuit open through the timeout.
const FAILED_DIAL_COOLDOWN_SECONDS: i64 = 30 * 60;

/// How recently a conversation must have seen traffic to be worth dialling.
const ACTIVE_WITHIN_SECONDS: i64 = 4 * 7 * 24 * 60 * 60;

/// Whether a conversation is recent enough to dial routinely.
///
/// Zero means *unknown*, not *ancient*, and is treated as worth dialling. A
/// conversation that has genuinely never seen traffic is one we were only just
/// introduced to; and a database written by a build that did not record
/// activity at all has zero on every row, so reading zero as dormant would
/// leave an existing user unable to reach a single one of their contacts.
fn recently_active(last_activity: i64, cutoff: i64) -> bool {
    last_activity == 0 || last_activity >= cutoff
}

/// How many devices to keep open per conversation during steady state.
const CONNECTIONS_PER_THREAD: usize = 4;

/// How many to try on the first pass, when nothing is connected yet.
const STARTUP_DIALS_PER_THREAD: usize = 50;

/// Add up to `limit` of `addresses` to `chosen`, skipping any already picked.
///
/// The choice is random rather than the first few: every device in a group is
/// a valid route to it, and if every member deterministically picked the same
/// ones the group would split into cliques that gossip internally and not
/// across.
fn take(addresses: Vec<String>, limit: usize, chosen: &mut Vec<String>, seen: &mut HashSet<String>) {
    let mut candidates = addresses;
    candidates.shuffle(&mut rand::thread_rng());
    for address in candidates.into_iter().take(limit) {
        if seen.insert(address.clone()) {
            chosen.push(address);
        }
    }
}

/// When each device was last dialled, and last refused.
#[derive(Default)]
pub(super) struct PeeringState {
    last_dial: HashMap<String, i64>,
    last_failure: HashMap<String, i64>,
}

impl PeeringState {
    /// Whether a device should be left alone for now.
    fn on_cooldown(&self, address: &str, now: i64) -> bool {
        if let Some(at) = self.last_failure.get(address) {
            if now - at < FAILED_DIAL_COOLDOWN_SECONDS {
                return true;
            }
        }
        if let Some(at) = self.last_dial.get(address) {
            if now - at < DIAL_COOLDOWN_SECONDS {
                return true;
            }
        }
        false
    }
}

impl<N: Network + 'static> Engine<N> {
    /// Dial what we can reach, then keep doing so.
    ///
    /// Spawned once at start-up and never returns.
    pub async fn run_peering(self: Arc<Self>) {
        // The first pass is wider than the steady-state one: nothing is
        // connected yet, so there is no reason to hold back.
        self.audit(STARTUP_DIALS_PER_THREAD).await;

        tokio::spawn(Arc::clone(&self).run_keep_alive());

        loop {
            tokio::time::sleep(AUDIT_INTERVAL).await;
            self.audit(CONNECTIONS_PER_THREAD).await;
        }
    }

    /// Hold every open circuit open.
    ///
    /// Tor closes an idle circuit, and a closed circuit is indistinguishable
    /// from a peer that went away — the next audit would dial again, sixty
    /// seconds later, having lost everything in between.
    async fn run_keep_alive(self: Arc<Self>) {
        let Ok(payload) = KeepAlive {}.encode() else {
            return;
        };

        loop {
            tokio::time::sleep(KEEP_ALIVE_INTERVAL).await;

            let frame = RawFrame::new(FrameType::KeepAlive.as_u16(), payload.clone());
            let peers = self.peers.read().await;
            for peer in peers.values() {
                // A full queue means the peer is not keeping up, in which case
                // it does not need a keep-alive to know we are here.
                let _ = peer.sender.try_send(frame.clone());
            }
        }
    }

    /// One pass: work out who we should be able to reach, and dial the gaps.
    async fn audit(self: &Arc<Self>, per_thread: usize) {
        match self.addresses_to_dial(per_thread).await {
            Ok(addresses) => {
                for address in addresses {
                    self.dial(address).await;
                }
            }
            Err(error) => tracing::warn!(%error, "could not work out who to peer with"),
        }
    }

    /// The devices worth dialling right now.
    ///
    /// Read-only and synchronous over the database, so the peer map is locked
    /// briefly rather than held across every dial.
    async fn addresses_to_dial(&self, per_thread: usize) -> Result<Vec<String>> {
        let now = crate::now();
        let cutoff = now - ACTIVE_WITHIN_SECONDS;
        let me = self.network.address();

        let connected: HashSet<String> = self.peers.read().await.keys().cloned().collect();
        let peering = self.peering.lock().await;

        // A device is worth dialling if it is not us, not revoked, not already
        // connected, and not on cooldown.
        let usable = |address: &str| {
            address != me && !connected.contains(address) && !peering.on_cooldown(address, now)
        };

        let mut chosen: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        // Our own devices, always and all of them. Everything else can wait
        // for the next audit; a device group that is not talking to itself is
        // a profile silently diverging.
        if let Some(profile) = self.store.profile()? {
            let mine: Vec<String> = profile
                .devices
                .iter()
                .filter(|device| !device.is_revoked() && usable(&device.address))
                .map(|device| device.address.clone())
                .collect();
            take(mine, usize::MAX, &mut chosen, &mut seen);
        }

        let my_id = self.store.my_user_id().ok();

        for user in self.store.all_users()? {
            if user.blocked || Some(user.id) == my_id {
                continue;
            }
            if !recently_active(user.last_activity, cutoff) {
                continue;
            }
            let addresses: Vec<String> = self
                .store
                .devices_for_user(user.id)?
                .into_iter()
                .filter(|device| !device.is_revoked() && usable(&device.address))
                .map(|device| device.address)
                .collect();
            take(addresses, per_thread, &mut chosen, &mut seen);
        }

        for group in self.store.all_groups()? {
            if !recently_active(group.last_activity, cutoff) {
                continue;
            }
            let mut addresses = Vec::new();
            for member in group.member_ids() {
                if Some(member) == my_id {
                    continue;
                }
                // A blocked member is not dialled even for a group we share,
                // which is what makes blocking mean something.
                if self.store.user(member)?.is_some_and(|user| user.blocked) {
                    continue;
                }
                for device in self.store.devices_for_user(member)? {
                    if !device.is_revoked() && usable(&device.address) {
                        addresses.push(device.address);
                    }
                }
            }
            take(addresses, per_thread, &mut chosen, &mut seen);
        }

        // Anyone we are still holding frames for, however dormant. Recency is
        // a budget on routine dialling, not a reason to strand a message: a
        // contact who has been quiet for a year still gets dialled if there is
        // something in the queue for them.
        drop(peering);
        for address in self.addresses_owed_frames(&connected, now).await? {
            if seen.insert(address.clone()) {
                chosen.push(address);
            }
        }

        Ok(chosen)
    }

    /// Devices we hold undelivered frames for.
    ///
    /// This reuses the reference flow's own view of what a peer is owed and
    /// entitled to, so it can never dial somebody to deliver something they
    /// were not allowed to have in the first place.
    async fn addresses_owed_frames(
        &self,
        connected: &HashSet<String>,
        now: i64,
    ) -> Result<Vec<String>> {
        let me = self.network.address();
        let my_id = self.store.my_user_id()?;
        let peering = self.peering.lock().await;

        let mut owed = Vec::new();
        for device in self.store.all_devices()? {
            if device.is_revoked() || device.address == me {
                continue;
            }
            if connected.contains(&device.address) || peering.on_cooldown(&device.address, now) {
                continue;
            }
            if self
                .store
                .user(device.user_id)?
                .is_some_and(|user| user.blocked)
            {
                continue;
            }

            let references = self
                .store
                .references_not_delivered_to(&device.address, |frame_id, frame_type| {
                    self.peer_may_have(device.user_id, my_id, frame_id, frame_type)
                        .unwrap_or(false)
                })?;

            if !references.is_empty() {
                owed.push(device.address);
            }
        }
        Ok(owed)
    }

    /// Dial one device, recording the attempt either way.
    async fn dial(self: &Arc<Self>, address: String) {
        self.peering
            .lock()
            .await
            .last_dial
            .insert(address.clone(), crate::now());

        match Arc::clone(self).connect(&address).await {
            Ok(()) => {
                self.peering.lock().await.last_failure.remove(&address);
            }
            Err(error) => {
                tracing::debug!(%address, %error, "could not reach a device");
                self.peering
                    .lock()
                    .await
                    .last_failure
                    .insert(address, crate::now());
            }
        }
    }

    /// Dial a conversation's devices now, without waiting for the next audit.
    ///
    /// Opening a conversation, or sending into one, is a statement that the
    /// other side is wanted — the sixty second audit is the floor, not the
    /// response time.
    pub async fn reach_for(self: &Arc<Self>, conversation: Uuid) {
        let addresses = match self.conversation_addresses(conversation).await {
            Ok(addresses) => addresses,
            Err(error) => {
                tracing::debug!(%error, "could not resolve a conversation's devices");
                return;
            }
        };

        for address in addresses {
            self.dial(address).await;
        }
    }

    /// The dialable devices of one conversation, group or direct.
    async fn conversation_addresses(&self, conversation: Uuid) -> Result<Vec<String>> {
        let now = crate::now();
        let me = self.network.address();
        let connected: HashSet<String> = self.peers.read().await.keys().cloned().collect();
        let peering = self.peering.lock().await;

        let members = match self.store.group(conversation)? {
            Some(group) => group.member_ids(),
            None => vec![conversation],
        };

        let mut addresses = Vec::new();
        for member in members {
            if self.store.user(member)?.is_some_and(|user| user.blocked) {
                continue;
            }
            for device in self.store.devices_for_user(member)? {
                if device.is_revoked() || device.address == me {
                    continue;
                }
                if connected.contains(&device.address) || peering.on_cooldown(&device.address, now)
                {
                    continue;
                }
                addresses.push(device.address);
            }
        }
        Ok(addresses)
    }
}

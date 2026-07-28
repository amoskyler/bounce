//! The chat engine.
//!
//! Ties the pieces together: it owns the [`Store`], holds the device key, talks
//! to peers over a [`Network`], and turns inbound frames into database writes
//! and [`Event`]s.
//!
//! ## The shape of a peer session
//!
//! ```text
//!   connect ──▶ send reference offer ──▶ read frames in a loop
//!                                            │
//!                        ┌───────────────────┼───────────────────┐
//!                        ▼                   ▼                   ▼
//!                  content frame       reference flow         keep-alive
//!                   validate,           offer/request/
//!                   store, emit,        catch-up
//!                   ack, relay
//! ```
//!
//! ## Validation happens once, at the edge
//!
//! Every inbound frame is checked before it touches the database:
//!
//! 1. the signature verifies against the signing device's address;
//! 2. the signing device belongs to the user the frame claims as author;
//! 3. that device was not revoked before the frame was written; and
//! 4. the author is not blocked.
//!
//! Only then is it stored. Handlers downstream can therefore treat a stored
//! frame as authentic, and nothing re-checks it.

mod chunks;
pub mod event;
pub mod files;
pub mod interaction;
pub mod pairing;
pub mod peering;
pub mod retention;
pub mod settings;
pub mod system;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

pub use event::{
    AttachmentView, DeviceView, DraftView, Event, GroupView, InitialState, MessageInfo,
    MessageView, QuoteView, ReactionView, Receipt, SystemMessageView, UserView,
};
pub use files::OutgoingAttachment;
use interaction::quote_view;
pub use settings::{auto_join, SettingsView};

use crate::consensus;
use crate::crypto::DeviceKey;
use crate::device_group;
use crate::error::{Error, Result};
use crate::frames::group::{Confirmation, Group, GroupCreation, UpdateGroup};
use crate::frames::identity::{Device, ProfileSettings, User};
use crate::frames::pairing::{
    AddUser, AddUserRequest, AddUserRequestAccepted, AddUserRequestRejected, SyncDeviceOffer,
};
use crate::frames::message::{DirectMessage, Draft, GroupMessage, ReadReceipt, TypingIndicator};
use crate::frames::update::{UpdateDeviceType, UpdateDm, UpdateDmType, UpdateUser};
use crate::frames::transport::{
    Ack, CatchUp, CatchUpFrame, DeliveryRecord, ReferenceOffer, ReferenceRequest,
};
use crate::frames::{Broadcastable, SignedFrame};
use crate::net::{Network, PeerConnection};
use crate::scope::{self, ScopeContext};
use crate::signed::SignedContainer;
use crate::store::Store;
use crate::types::{FrameType, Scope, UpdateGroupType};
use crate::wire::{self, RawFrame};

/// How long a device waits before re-broadcasting a typing indicator for a
/// thread it is still being typed in.
///
/// The comparison is strictly-less on whole seconds, so the effective interval
/// is a little over three seconds.
const TYPING_SEND_COOLDOWN_SECONDS: i64 = 3;

/// How long an inbound typing indicator stays on screen before it is withdrawn.
const TYPING_DISPLAY_SECONDS: i64 = 3;

/// How long an indicator's ID is remembered for de-duplication.
///
/// Typing indicators are gossiped like everything else, so the same one comes
/// back from several peers. Without this, each copy would re-broadcast and the
/// indicator would circulate indefinitely.
const TYPING_SEEN_RETENTION_SECONDS: i64 = 60;

/// How long an outgoing add-user request stays answerable.
///
/// Beyond this an "accepted" is treated as unsolicited, so a stale reply cannot
/// be used to slip a contact in later.
const PENDING_REQUEST_VALIDITY_SECONDS: i64 = 300;

/// A frame waiting to be written to a peer.
type Outbound = RawFrame;

/// Ephemeral typing state.
///
/// Typing indicators are never stored: they are worthless a second after they
/// are sent, and keeping them would leave a fine-grained record of when
/// somebody was at their keyboard.
#[derive(Default)]
struct TypingState {
    /// Indicator IDs already handled, with the time each was seen.
    seen: HashMap<Uuid, i64>,
    /// When this device last sent an indicator for a thread.
    last_sent: HashMap<Uuid, i64>,
    /// When each `(user, thread)` was last reported as typing.
    displaying: HashMap<(Uuid, Uuid), i64>,
}

impl TypingState {
    /// Whether this indicator has already been handled, recording it if not.
    fn already_seen(&mut self, id: Uuid, now: i64) -> bool {
        self.seen
            .retain(|_, seen_at| *seen_at >= now - TYPING_SEEN_RETENTION_SECONDS);

        if self.seen.contains_key(&id) {
            return true;
        }
        self.seen.insert(id, now);
        false
    }

    /// Whether sending for this thread is still within its cooldown.
    ///
    /// The timestamp is only advanced when a send actually happens, so the
    /// interval runs from the last indicator that went out rather than from the
    /// last keystroke.
    fn should_wait_before_sending(&mut self, thread: Uuid, now: i64) -> bool {
        match self.last_sent.get(&thread) {
            Some(&last) if last >= now - TYPING_SEND_COOLDOWN_SECONDS => true,
            _ => {
                self.last_sent.insert(thread, now);
                false
            }
        }
    }

    /// Record that a user is typing; returns true if this is a fresh start
    /// rather than a refresh of one already on screen.
    fn begin_displaying(&mut self, user: Uuid, thread: Uuid, now: i64) -> bool {
        self.displaying.insert((user, thread), now).is_none()
    }

    /// Withdraw every indicator that has aged out.
    fn expired(&mut self, now: i64) -> Vec<(Uuid, Uuid)> {
        let expired: Vec<(Uuid, Uuid)> = self
            .displaying
            .iter()
            .filter(|(_, &shown_at)| shown_at < now - TYPING_DISPLAY_SECONDS)
            .map(|(&key, _)| key)
            .collect();

        for key in &expired {
            self.displaying.remove(key);
        }
        expired
    }

    /// Withdraw one user's indicator, which happens as soon as the message they
    /// were composing arrives.
    fn stop_displaying(&mut self, user: Uuid, thread: Uuid) -> bool {
        self.displaying.remove(&(user, thread)).is_some()
    }
}

/// A connected peer.
struct Peer {
    /// Frames queued for this peer.
    sender: mpsc::Sender<Outbound>,
}

/// The engine.
pub struct Engine<N: Network> {
    key: DeviceKey,
    store: Arc<Store>,
    network: Arc<N>,
    events: mpsc::UnboundedSender<Event>,
    /// The open sessions, under a *synchronous* lock.
    ///
    /// It has to be readable without awaiting, because who is connected is
    /// part of the boot snapshot and `initial_state` is called straight from
    /// the client rather than from inside the runtime. Nothing here is ever
    /// held across an await — every use takes the guard, writes to some
    /// channels, and drops it — so an async lock bought nothing but the
    /// inability to answer that question.
    peers: std::sync::RwLock<HashMap<String, Peer>>,
    /// Serialises frame handling. Handlers read-modify-write state that spans
    /// several tables, and the volume is low enough that a single lock is
    /// simpler and safer than per-table locking.
    handler_lock: Mutex<()>,
    typing: Mutex<TypingState>,
    /// Peers we have sent an add-user request to, and when.
    pending_add_requests: Mutex<HashMap<String, i64>>,
    /// Dial history, so a device is not hammered and a dead one is backed off.
    peering: Mutex<peering::PeeringState>,
    /// Who holds which chunk, and what we have asked for. Synchronous, because
    /// the scheduler decides under the lock and sends after dropping it.
    chunk_engine: std::sync::Mutex<chunks::ChunkEngine>,
    /// Asks the peering loop to look again without waiting out its interval.
    /// Go spawns `auditPeers` outright after a catch-up; this is the same
    /// thing from a context that has no `Arc` to spawn with.
    audit_now: tokio::sync::Notify,
    /// Peers owed a reference offer. Sending one can take a minute of retries,
    /// so it never happens on the caller's task.
    reference_offers: mpsc::UnboundedSender<String>,
    /// The other end, taken by `run_listener`. Held here rather than spawned
    /// from `new` because an engine can be built outside a runtime — several
    /// unit tests do exactly that to reach a synchronous method — and spawning
    /// there would panic.
    offer_requests: std::sync::Mutex<Option<mpsc::UnboundedReceiver<String>>>,
}

impl<N: Network + 'static> Engine<N> {
    /// Create an engine, returning it alongside the stream of events.
    pub fn new(
        key: DeviceKey,
        store: Arc<Store>,
        network: Arc<N>,
    ) -> (Arc<Self>, mpsc::UnboundedReceiver<Event>) {
        let (events, receiver) = mpsc::unbounded_channel();
        let (reference_offers, offer_requests) = mpsc::unbounded_channel();
        let engine = Arc::new(Engine {
            key,
            store,
            network,
            events,
            peers: std::sync::RwLock::new(HashMap::new()),
            handler_lock: Mutex::new(()),
            typing: Mutex::new(TypingState::default()),
            pending_add_requests: Mutex::new(HashMap::new()),
            peering: Mutex::new(peering::PeeringState::default()),
            chunk_engine: std::sync::Mutex::new(chunks::ChunkEngine::default()),
            audit_now: tokio::sync::Notify::new(),
            reference_offers,
            offer_requests: std::sync::Mutex::new(Some(offer_requests)),
        });
        (engine, receiver)
    }

    /// This device's address.
    pub fn address(&self) -> String {
        self.network.address()
    }

    /// Note that a conversation saw traffic.
    ///
    /// Peering decides who to dial from this, so failing to record it is what
    /// makes a client stop reaching out to somebody it talks to every day. It
    /// is deliberately infallible: a message that arrived is not worth
    /// rejecting because a bookkeeping write failed.
    fn note_activity_with(&self, user_id: Uuid, at: i64) {
        if let Err(error) = self.store.note_user_activity(user_id, at) {
            tracing::debug!(%error, "could not record conversation activity");
        }
    }

    /// Put a closed conversation back on the list because something arrived
    /// in it.
    ///
    /// Closing a conversation only hides it — nothing is deleted, and the
    /// messages keep arriving. Without this they would keep arriving
    /// *invisibly*, which turns a tidying gesture into silent message loss.
    /// Go reopens in its interface (`ui/direct_message.go:1207`); doing it in
    /// the engine means a second client gets the same behaviour for free, and
    /// a message that lands while no window is open is not missed.
    fn reopen_conversation(&self, user_id: Uuid) -> Result<()> {
        let Some(user) = self.store.user(user_id)? else {
            return Ok(());
        };
        // A blocked contact stays closed: their messages are refused, and
        // reopening would put somebody back on the list who was deliberately
        // taken off it.
        if user.open_dm || user.blocked {
            return Ok(());
        }

        let mut reopened = user;
        reopened.open_dm = true;
        self.store.update_user_local_state(&reopened)?;
        self.emit(Event::UserUpdated {
            user: self.user_view(&reopened, false),
        });
        Ok(())
    }

    fn note_activity_in(&self, group_id: Uuid, at: i64) {
        if let Err(error) = self.store.note_group_activity(group_id, at) {
            tracing::debug!(%error, "could not record group activity");
        }
    }

    /// Drop every open session.
    ///
    /// Exists for tests that need a device to look offline without tearing
    /// down its store and key. Peering re-establishes whatever it needs.
    pub async fn disconnect_all(&self) {
        self.peers.write().expect("peer map").clear();
    }

    /// The devices this engine currently holds a session with.
    ///
    /// A message only reaches somebody whose address is in here at the moment
    /// it is broadcast; everyone else has to wait for the reference flow on a
    /// later connection.
    pub async fn connected_addresses(&self) -> Vec<String> {
        self.peers.read().expect("peer map").keys().cloned().collect()
    }

    /// The same set, without awaiting, for peering.
    fn connected_addresses_now(&self) -> HashSet<String> {
        self.peers.read().expect("peer map").keys().cloned().collect()
    }

    /// Whether a session is open to this exact device.
    fn is_connected(&self, address: &str) -> bool {
        self.peers.read().expect("peer map").contains_key(address)
    }

    /// Whether any device belonging to `user` is connected right now.
    ///
    /// Somebody is reachable if *any* of their devices is; presence is a fact
    /// about the person, which is the granularity the sidebar shows it at.
    fn user_is_online(&self, user: Uuid) -> bool {
        self.store
            .devices_for_user(user)
            .map(|devices| {
                devices
                    .iter()
                    .any(|device| !device.is_revoked() && self.is_connected(&device.address))
            })
            .unwrap_or(false)
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    fn emit(&self, event: Event) {
        // A closed receiver means the client detached; the engine keeps running.
        let _ = self.events.send(event);
    }

    // ---------------------------------------------------------------------
    // Profile
    // ---------------------------------------------------------------------

    /// Create the profile that owns this device.
    ///
    /// This is the founding act: it generates the user's keys and records this
    /// device as the one member of the device group with no introduction
    /// signature.
    pub fn create_profile(&self, name: &str, device_name: &str) -> Result<User> {
        if self.store.profile()?.is_some() {
            return Err(Error::ProfileExists);
        }
        if !crate::frames::identity::valid_user_name(name) {
            return Err(Error::InvalidFrame("invalid profile name".into()));
        }

        let user_id = Uuid::new_v4();
        let (private_ecdh, public_ecdh) = crate::crypto::generate_x25519_keypair();
        let signing_key = DeviceKey::generate();

        let mut device = Device::new(
            Uuid::new_v4(),
            user_id,
            self.network.address(),
            crate::now(),
        );
        device.name = device_name.to_string();
        device.saved_at = crate::now();
        let (device_ecdh_private, device_ecdh_public) = crate::crypto::generate_x25519_keypair();
        device.ecdh_private_key = device_ecdh_private.to_vec();
        device.ecdh_public_key = device_ecdh_public.to_vec();

        let mut user = User::new(user_id, name.to_string());
        user.profile = true;
        user.accepted = true;
        user.open_dm = true;
        user.introduction_method = crate::types::introduction::PROFILE.to_string();
        user.introduction_time = crate::now();
        user.last_activity = crate::now();
        user.public_ecdh_key = public_ecdh.to_vec();
        user.private_ecdh_key = private_ecdh.to_vec();
        user.public_ecdsa_key = signing_key.public_key().to_vec();
        user.private_ecdsa_key = signing_key.to_private_bytes();
        user.devices.push(device.clone());

        self.store.save_user(&user)?;
        self.store
            .save_profile_settings(&ProfileSettings::defaults(user_id))?;

        self.emit(Event::ProfileCreated {
            user: self.user_view(&user, false),
            device: self.device_view(&device, true),
        });

        Ok(user)
    }

    /// Everything a freshly attached client needs to render.
    ///
    /// Presence included, by way of `user_view` and `device_view`, which read
    /// the live peer map. `UserOnline` and `DeviceOnline` fire once, when a
    /// session opens, and a client rebuilding from this snapshot cannot hear
    /// them again — so a snapshot that reported everybody offline did not
    /// merely start pessimistic, it overwrote what the client already knew and
    /// stayed wrong for the life of the session.
    ///
    /// A joining device reloads this the instant pairing completes, because
    /// the profile it did not have now exists. That is how a freshly paired
    /// device came to describe the very socket it had just arrived over as
    /// "Never connected".
    pub fn initial_state(&self) -> Result<InitialState> {
        let profile = self.store.profile()?;
        let my_id = profile.as_ref().map(|p| p.id);

        let mut users = Vec::new();
        let mut messages = Vec::new();

        for user in self.store.all_users()? {
            if Some(user.id) == my_id {
                continue;
            }
            users.push(self.user_view(&user, false));

            if let Some(my_id) = my_id {
                let thread = crate::xor(my_id, user.id);
                for message in self.store.direct_messages_for_thread(thread, 200)? {
                    messages.push(self.direct_message_view(&message, my_id, user.id)?);
                }
            }
        }

        let mut groups = Vec::new();
        for group in self.store.all_groups()? {
            groups.push(self.group_view(&group));
            for message in self.store.group_messages_for_thread(group.id, 200)? {
                messages.push(self.group_message_view(&message, my_id)?);
            }
        }

        // Notes to self live in a thread with a nil XOR.
        if let Some(my_id) = my_id {
            for message in self.store.direct_messages_for_thread(Uuid::nil(), 200)? {
                messages.push(self.direct_message_view(&message, my_id, my_id)?);
            }
        }

        messages.sort_by_key(|message| message.written_at);

        let sync_devices = match &profile {
            Some(profile) => profile
                .devices
                .iter()
                .map(|device| {
                    let local = device.address == self.network.address();
                    self.device_view(device, local)
                })
                .collect(),
            None => Vec::new(),
        };

        let drafts = self
            .store
            .all_drafts()?
            .into_iter()
            .map(|draft| DraftView {
                thread: draft.thread,
                text: draft.text,
            })
            .collect();

        Ok(InitialState {
            profile: profile.as_ref().map(|user| self.user_view(user, true)),
            network_online: true,
            device_revoked: false,
            sync_devices,
            users,
            groups,
            messages,
            system_messages: self.system_message_views()?,
            drafts,
        })
    }

    // ---------------------------------------------------------------------
    // Sending
    // ---------------------------------------------------------------------

    /// Write and broadcast a direct message.
    pub async fn send_direct_message(
        &self,
        recipient: Uuid,
        text: &str,
        reply_to: Option<Uuid>,
    ) -> Result<MessageView> {
        let my_id = self.store.my_user_id()?;

        if text.chars().count() > crate::MAXIMUM_MESSAGE_CHARACTERS {
            return Err(Error::InvalidFrame("message is too long".into()));
        }

        let mut message = DirectMessage::new(my_id, recipient, text.to_string(), crate::now());
        message.quote = self.quote_of(reply_to)?;
        if message.is_empty() {
            return Err(Error::InvalidFrame("refusing to send an empty message".into()));
        }
        message.saved_at = crate::now();

        // Retention on the thread decides when it expires.
        if let Some(user) = self.store.user(recipient)? {
            if user.retention > 0 {
                message.delete_at = crate::now() + user.retention;
            }
        }

        let body = crate::msgpack::to_vec(&message)?;
        let container = SignedContainer::create(&self.key, body);
        message.signed = SignedFrame::from_container(&container);

        self.store.save_direct_message(&message)?;
        self.note_activity_with(recipient, message.written_at);

        let view = self.direct_message_view(&message, my_id, recipient)?;
        self.emit(Event::MessageSent {
            message: Box::new(view.clone()),
        });

        self.broadcast(&message).await?;
        Ok(view)
    }

    /// Write and broadcast a group message.
    pub async fn send_group_message(
        &self,
        group_id: Uuid,
        text: &str,
        reply_to: Option<Uuid>,
    ) -> Result<MessageView> {
        let my_id = self.store.my_user_id()?;

        let group = self
            .store
            .group(group_id)?
            .ok_or(Error::GroupNotFound)?;

        // Posting can be restricted to admins.
        if group.restrict_posting && !group.admin_ids().contains(&my_id) {
            return Err(Error::NotPermitted("posting is restricted to admins"));
        }

        let mut message = GroupMessage::new(my_id, group_id, text.to_string(), crate::now());
        message.quote = self.quote_of(reply_to)?;
        if message.is_empty() {
            return Err(Error::InvalidFrame("refusing to send an empty message".into()));
        }
        message.saved_at = crate::now();
        if group.retention > 0 {
            message.delete_at = crate::now() + group.retention;
        }

        let body = crate::msgpack::to_vec(&message)?;
        let container = SignedContainer::create(&self.key, body);
        message.signed = SignedFrame::from_container(&container);

        self.store.save_group_message(&message)?;
        self.note_activity_in(group_id, message.written_at);

        let view = self.group_message_view(&message, Some(my_id))?;
        self.emit(Event::MessageSent {
            message: Box::new(view.clone()),
        });

        self.broadcast(&message).await?;
        Ok(view)
    }

    /// Create a group with this device's owner as the sole member and admin.
    pub async fn create_group(&self, name: &str, initial_invites: &[Uuid]) -> Result<GroupView> {
        let my_id = self.store.my_user_id()?;
        let me = self.store.profile()?.ok_or(Error::NoProfile)?;

        if !crate::frames::identity::valid_user_name(name) {
            return Err(Error::InvalidFrame("invalid group name".into()));
        }

        let settings = self
            .store
            .profile_settings(my_id)?
            .unwrap_or_else(|| ProfileSettings::defaults(my_id));

        let group = Group {
            name: name.to_string(),
            created_by: my_id,
            created_at: crate::now(),
            retention: settings.default_group_retention,
            users: vec![me],
            admins: my_id.to_string(),
            restrict_user_management: settings.new_group_restrict_user_management,
            restrict_group_edits: settings.new_group_restrict_group_edits,
            restrict_posting: settings.new_group_restrict_posting,
            last_activity: crate::now(),
            ..Default::default()
        };

        let mut creation = GroupCreation::create(&group, group.created_at)?;

        // The signature covers the creation record, not the bare group blob —
        // the blob is already pinned by the ID, and the record adds the
        // timestamp that must be pinned too.
        let body = crate::msgpack::to_vec(&creation)?;
        let container = SignedContainer::create(&self.key, body);
        creation.signed = SignedFrame::from_container(&container);
        creation.saved_at = crate::now();

        self.store.save_group_creation(&creation)?;

        let mut stored = group.clone();
        stored.id = creation.id;
        self.store.save_group(&stored)?;

        self.broadcast(&creation).await?;

        for invitee in initial_invites {
            self.invite_to_group(creation.id, *invitee).await?;
        }

        let refreshed = self.store.group(creation.id)?.ok_or(Error::GroupNotFound)?;
        let view = self.group_view(&refreshed);
        self.emit(Event::GroupUpdated {
            group: Box::new(view.clone()),
        });
        Ok(view)
    }

    /// Invite a user to a group.
    pub async fn invite_to_group(&self, group_id: Uuid, invitee: Uuid) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        let user = self.store.user(invitee)?.ok_or(Error::UserNotFound)?;

        // The invitation carries the invitee's whole record, so the rest of the
        // group can reach them before they have accepted.
        let update = self
            .sign_update_group(my_id, group_id, UpdateGroupType::InviteUser, crate::msgpack::to_vec(&user)?)?;

        self.apply_update_group(update).await
    }

    /// Rename a group.
    pub async fn rename_group(&self, group_id: Uuid, name: &str) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        if !crate::frames::identity::valid_user_name(name) {
            return Err(Error::InvalidFrame("invalid group name".into()));
        }
        let update = self.sign_update_group(
            my_id,
            group_id,
            UpdateGroupType::ChangeName,
            name.as_bytes().to_vec(),
        )?;
        self.apply_update_group(update).await
    }

    /// Accept or decline an invitation.
    ///
    /// Accepting is what moves a user from the invited set into membership,
    /// and with it from seeing only the group's metadata to seeing its
    /// messages.
    pub async fn respond_to_invite(&self, group_id: Uuid, accept: bool) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        if accept {
            // Joining is consent to be in a group with the people already in
            // it, so everyone in it becomes accepted — which is what the
            // auto-join policy later reads to decide whether a group contains
            // anybody new. Go does this first too, in `AcceptInvite`
            // (chat/update_group.go:821).
            self.accept_all_users(group_id)?;
        }
        let response = if accept {
            consensus::state::sentinels::ACCEPT_INVITE
        } else {
            consensus::state::sentinels::REJECT_INVITE
        };
        let update = self.sign_update_group(
            my_id,
            group_id,
            UpdateGroupType::RespondToInvite,
            vec![response],
        )?;
        self.apply_update_group(update).await
    }

    /// Leave a group.
    pub async fn leave_group(&self, group_id: Uuid) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        let update = self.sign_update_group(
            my_id,
            group_id,
            UpdateGroupType::RemoveUser,
            my_id.as_bytes().to_vec(),
        )?;
        self.apply_update_group(update).await
    }

    /// Mark everyone in a group as somebody this device's owner has agreed to
    /// be in a group with.
    fn accept_all_users(&self, group_id: Uuid) -> Result<()> {
        let Some(group) = self.store.group(group_id)? else {
            return Ok(());
        };
        let everyone: Vec<Uuid> = group
            .member_ids()
            .into_iter()
            .chain(group.invite_ids())
            .collect();
        self.store.mark_users_accepted(&everyone)
    }

    /// Accept an invitation without asking, if the policy says to.
    ///
    /// The default is [`auto_join::ONLY_WITHOUT_NEW_USERS`], which is why the
    /// flag it reads has to mean something: a group that contains anybody this
    /// device's owner has not knowingly agreed to is one they get asked about.
    /// Users met through a group are exactly the ones that are not accepted
    /// (see [`Engine::adopt_group_user`]), so the policy holds.
    ///
    /// One thing Go has and this does not: it refuses to apply a setting that
    /// was changed *after* the invitation arrived
    /// (`chat/consensus_store.go:592`), so turning auto-join on cannot
    /// retroactively accept invitations already sitting there. Nothing here
    /// records when a setting changed, so that guard is unported.
    async fn auto_join_if_policy_allows(&self, group_id: Uuid) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        let Some(group) = self.store.group(group_id)? else {
            return Ok(());
        };
        if !group.invite_ids().contains(&my_id) || group.member_ids().contains(&my_id) {
            return Ok(());
        }

        let settings = self
            .store
            .profile_settings(my_id)?
            .unwrap_or_else(|| ProfileSettings::defaults(my_id));

        let join = match settings.auto_join_groups {
            auto_join::ALWAYS => true,
            auto_join::ONLY_WITHOUT_NEW_USERS => {
                // A user we have no row for at all counts as new, exactly as an
                // unaccepted one does.
                let mut all_known = true;
                for id in group.member_ids().into_iter().chain(group.invite_ids()) {
                    if id == my_id {
                        continue;
                    }
                    all_known &= self
                        .store
                        .user(id)?
                        .is_some_and(|user| user.accepted && !user.blocked);
                }
                all_known
            }
            _ => false,
        };

        if join {
            tracing::info!(%group_id, "auto-joining a group of people already accepted");
            self.respond_to_invite(group_id, true).await?;
        }
        Ok(())
    }

    fn sign_update_group(
        &self,
        actor: Uuid,
        group_id: Uuid,
        kind: UpdateGroupType,
        data: Vec<u8>,
    ) -> Result<UpdateGroup> {
        // Timestamps have one-second resolution, and updates are replayed in
        // timestamp order. An update we create in response to one we have just
        // seen — accepting an invitation, say — must therefore sort after it,
        // or every device would replay the response before the invitation and
        // reject it. Advancing past the newest update we already hold for the
        // group guarantees that, without depending on how fast the clock ticks.
        let latest_known = self
            .store
            .updates_for_group(group_id)?
            .iter()
            .map(|update| update.timestamp)
            .max()
            .unwrap_or(0);
        let timestamp = crate::now().max(latest_known + 1);

        let mut update = UpdateGroup::new(actor, group_id, kind, data, timestamp);
        update.saved_at = crate::now();

        let body = crate::msgpack::to_vec(&update)?;
        let container = SignedContainer::create(&self.key, body);
        update.signed = SignedFrame::from_container(&container);
        Ok(update)
    }

    /// Store an update, recompute the group, broadcast, and notify.
    async fn apply_update_group(&self, update: UpdateGroup) -> Result<()> {
        self.store.save_update_group(&update)?;
        // Recompute before broadcasting: the update may itself change who is in
        // scope for it, and an invitation only reaches the invitee once the
        // recomputed state lists them.
        self.recompute_and_confirm(update.target).await?;
        self.emit_group_system_message(&update);
        self.broadcast(&update).await?;
        self.reoffer_to_group_scope(update.target).await?;
        Ok(())
    }

    /// Send a fresh reference offer to every connected device in a group's
    /// scope.
    ///
    /// A scope change makes frames deliverable that were not before: inviting
    /// someone brings every frame about the group within reach of a device
    /// that previously had no claim to any of them. Rather than working out
    /// which frames just became eligible, the reference flow is simply re-run,
    /// and it computes the difference itself.
    async fn reoffer_to_group_scope(&self, group_id: Uuid) -> Result<()> {
        let Some(group) = self.store.group(group_id)? else {
            return Ok(());
        };

        let mut in_scope = Vec::new();
        for user_id in group.member_ids().into_iter().chain(group.invite_ids()) {
            for device in self.store.devices_for_user(user_id)? {
                if !device.is_revoked() && device.address != self.network.address() {
                    in_scope.push(device.address);
                }
            }
        }

        for address in in_scope {
            if !self.peers.read().expect("peer map").contains_key(&address) {
                continue;
            }
            let offer = self.build_reference_offer(&address)?;
            if offer.references.is_empty() {
                continue;
            }
            self.send_to(
                &address,
                RawFrame::new(FrameType::ReferenceOffer.as_u16(), offer.encode()?),
            )
            .await;
        }

        Ok(())
    }

    /// Rebuild a group's state from its creation record plus every known
    /// update, and persist the result.
    ///
    /// State is always rebuilt rather than mutated, which is what makes the
    /// outcome independent of the order updates arrived in.
    /// Rebuild a group from its creation record and every update it has seen.
    ///
    /// Returns the confirmations this device now owes — a signature for each
    /// valid update it has not yet vouched for. Minting is separated from
    /// broadcasting because this is synchronous and the broadcast is not; see
    /// [`Engine::recompute_and_confirm`], which is what callers want.
    fn recompute_group(&self, group_id: Uuid) -> Result<Vec<Confirmation>> {
        let my_id = self.store.my_user_id()?;

        let Some(creation) = self.store.group_creation(group_id)? else {
            // An update for a group we have not been told about yet. It will be
            // recomputed once the creation record arrives.
            return Ok(Vec::new());
        };

        let updates = self.store.updates_for_group(group_id)?;
        let stack = consensus::recompute(&creation, &updates, my_id)?;
        let state = stack.top()?;

        let mut group = creation.group()?;
        group.id = group_id;
        group.name = state.name.clone();
        group.admins = crate::frames::identity::join_uuid_list(&state.admins);
        group.invites = crate::frames::identity::join_uuid_list(&state.invites);
        group.blocked_users = crate::frames::identity::join_uuid_list(&state.blocked_users);
        group.images = crate::frames::identity::join_uuid_list(&state.images);
        group.retention = state.retention;
        group.clear_before = state.clear_before;
        group.muted_until = state.muted_until;
        group.restrict_posting = state.posting_restricted;
        group.restrict_group_edits = state.editing_restricted;
        group.restrict_user_management = state.user_management_restricted;
        group.invited_by = state.invited_by;
        group.invited_at = state.invited_at;
        group.accepted_at = state.accepted_at;

        // Anyone the group brought us into contact with is stored first, or the
        // loop below would drop them: it reads membership out of the database,
        // and a member who is not there is not merely missing a contact card —
        // `save_group` rewrites `group_users` from this list, `signer_speaks_for`
        // has no device to attribute their messages to, and peering never dials
        // them. Go creates the row in the same place, as a side effect of
        // consensus (`createNewUserIfNeeded`, chat/consensus_store.go:1021).
        self.adopt_users_met_through_group(group_id, &group.users, &stack, state)?;

        // Membership comes from the consensus result, not the founding record.
        group.users.clear();
        for member in &state.users {
            if let Some(user) = self.store.user(*member)? {
                group.users.push(user);
            }
        }

        // A group we have been removed from, or that has been deleted, is
        // reported rather than stored as if we were still in it.
        if let Some(removed) = &state.removed_by {
            self.store.save_group(&group)?;
            self.emit(Event::GroupRemoved {
                group_id,
                actor: removed.actor,
            });
            return Ok(Vec::new());
        }
        if let Some(deleted) = &state.deleted_by {
            self.emit(Event::GroupRemoved {
                group_id,
                actor: deleted.actor,
            });
            return Ok(Vec::new());
        }

        self.store.save_group(&group)?;
        self.emit(Event::GroupUpdated {
            group: Box::new(self.group_view(&group)),
        });

        // Confirmations are minted only after the group has been written,
        // because they go out at group scope and that scope is what has just
        // been recomputed (chat/consensus_store.go:255).
        let my_address = self.network.address();
        let now = crate::now();
        let mut minted = Vec::new();
        for update in consensus::confirmation::owed(&stack, my_id, &my_address)? {
            let confirmation = consensus::confirmation::mint(update, my_id, &self.key, now);
            self.store.save_confirmation(&confirmation)?;
            minted.push(confirmation);
        }
        Ok(minted)
    }

    /// Recompute a group and broadcast whatever confirmations that produced.
    ///
    /// Timestamps are forgeable, so confirmations are the protocol's answer to
    /// an admin backdating an update: the earlier of two conflicting updates
    /// wins unless the later one carries more of them. A device that computes
    /// them and never sends them leaves the defence inert and, worse, disagrees
    /// with a Go peer about which update is canonical.
    async fn recompute_and_confirm(&self, group_id: Uuid) -> Result<()> {
        for confirmation in self.recompute_group(group_id)? {
            self.broadcast(&confirmation).await?;
        }
        Ok(())
    }

    /// Store every user this group has introduced us to.
    ///
    /// A group of three where two people have never paired is the ordinary
    /// case, not a corner one: each learns of the other from the invitation
    /// that brought them in, and that record is the only copy of their device
    /// group anyone will ever send.
    ///
    /// Only *accepted* updates are read. An invitation consensus rejected — one
    /// from a stranger, or from a member without the right to invite — is a
    /// stranger's claim about who somebody is, and adopting from it would let
    /// any peer that can reach us write rows into our contact list.
    fn adopt_users_met_through_group(
        &self,
        group_id: Uuid,
        founding: &[User],
        stack: &consensus::CanonicalStack,
        state: &consensus::GroupState,
    ) -> Result<()> {
        let mut carried: Vec<User> = founding.to_vec();

        for update in stack.accepted_updates() {
            if !matches!(update.kind(), Ok(UpdateGroupType::InviteUser)) {
                continue;
            }
            match crate::msgpack::from_slice::<User>(&update.data) {
                Ok(user) => carried.push(user),
                Err(error) => {
                    tracing::warn!(%error, update = %update.id, "invitation carries no readable user")
                }
            }
        }

        for user in carried {
            // The same filter Go applies (`chat/consensus_store.go:246`): a
            // record only becomes a contact if the resolved state actually puts
            // them in the group.
            if state.is_member(user.id) || state.is_invited(user.id) {
                self.adopt_group_user(&user, group_id)?;
            }
        }
        Ok(())
    }

    /// Adopt one user we have met through a group, if they are new and their
    /// record holds up.
    ///
    /// Returns whether a contact was created.
    fn adopt_group_user(&self, user: &User, group_id: Uuid) -> Result<bool> {
        if self.store.my_user_id().is_ok_and(|my_id| my_id == user.id) {
            return Ok(false);
        }
        // Already known, from a pairing or from an earlier group. Their record
        // was established by a stronger introduction than this one, and a group
        // update is not authority to rewrite it.
        if self.store.user(user.id)?.is_some() {
            return Ok(false);
        }

        let refuse = |reason: &str| {
            tracing::warn!(user = %user.id, %group_id, reason, "refusing a user carried by a group");
            Ok(false)
        };

        if !crate::frames::identity::valid_user_name(&user.name) {
            return refuse("the name is not acceptable");
        }
        if !device_group::user_has_valid_device_group(user) {
            return refuse("the device group does not validate");
        }
        // Every device must claim the user it is filed under, or a device row
        // lands under an id the record chose and `save_device`'s upsert would
        // let it overwrite an unrelated device.
        if user.devices.iter().any(|device| device.user_id != user.id) {
            return refuse("a device in the group claims a different owner");
        }
        // A device address is a public key, so two users claiming one is a
        // contradiction rather than a merge.
        for device in &user.devices {
            if self
                .store
                .device_owner(&device.address)?
                .is_some_and(|owner| owner != user.id)
            {
                return refuse("a device is already held by another user");
            }
        }

        let mut adopted = shareable(user);
        adopted.introduction_method = crate::types::introduction::GROUP.to_string();
        adopted.introduction_time = crate::now();
        adopted.introduction_metadata = group_id;
        // Somebody met in a group is not somebody this device's owner chose:
        // no conversation is opened for them, and they are not accepted until
        // an invitation is answered — which is what the auto-join policy reads.
        adopted.open_dm = false;
        adopted.accepted = false;
        // Go dials them immediately (`UserConnectionDesired`). Here peering
        // decides from activity, so a contact met a second ago has to look
        // active or nothing would ever reach out to them.
        adopted.last_activity = crate::now();

        self.store.save_user(&adopted)?;
        self.emit(Event::UserAdded {
            user: self.user_view(&adopted, false),
        });
        Ok(true)
    }

    // ---------------------------------------------------------------------
    // Contact introduction
    // ---------------------------------------------------------------------

    /// Generate a pairing code for someone to scan.
    ///
    /// The returned string carries this device's address and a fresh secret.
    /// There is no directory to look anyone up in, so this — shown on a screen,
    /// read by a person standing next to you — is the whole of Bounce's contact
    /// discovery.
    ///
    /// The format is `<address>:<secret>`, matching what the Go implementation
    /// produces, so a code from either can be pasted into the other.
    ///
    /// Calling this invalidates any previously displayed code.
    pub fn create_pairing_code(&self) -> Result<String> {
        // 16 random bytes, hex encoded — the same shape Go uses, and far beyond
        // guessable inside the five minute window.
        let secret = hex::encode(crate::crypto::random_bytes(16));

        self.store.replace_pairing_offer(&SyncDeviceOffer {
            id: Uuid::new_v4(),
            timestamp: crate::now(),
            secret: secret.clone(),
        })?;

        Ok(format!("{}:{}", self.network.address(), secret))
    }

    /// Split a pairing code back into an address and a secret.
    ///
    /// Accepts an optional `bounce:` prefix as well as the bare Go form, since
    /// earlier builds of this client emitted the prefixed variant.
    pub fn parse_pairing_code(code: &str) -> Result<(String, String)> {
        let trimmed = code.trim().strip_prefix("bounce:").unwrap_or(code.trim());

        let (address, secret) = trimmed
            .split_once(':')
            .ok_or_else(|| Error::InvalidFrame("not a valid pairing code".into()))?;

        if !crate::onion::is_valid_address(address) || secret.is_empty() {
            return Err(Error::InvalidFrame("not a valid pairing code".into()));
        }

        Ok((address.to_string(), secret.to_string()))
    }

    /// Act on a scanned pairing code: connect and ask to be added.
    pub async fn request_to_add_user(self: &Arc<Self>, code: &str) -> Result<()> {
        let (address, secret) = Self::parse_pairing_code(code)?;
        let me = self.store.profile()?.ok_or(Error::NoProfile)?;

        // Remember that we asked. Without this, an unsolicited "accepted" from
        // any peer would be processed as though we had initiated it — which is
        // exactly how the Go implementation can be made to add a stranger.
        self.pending_add_requests
            .lock()
            .await
            .insert(address.clone(), crate::now());

        let request = AddUserRequest {
            secret,
            requester_user: crate::msgpack::to_vec(&shareable(&me))?,
        };

        Arc::clone(self).connect(&address).await?;
        // Give the session a moment to register before writing to it.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        self.send_to(
            &address,
            RawFrame::new(FrameType::AddUserRequest.as_u16(), request.encode()?),
        )
        .await;

        Ok(())
    }

    /// Someone scanned our code and is asking to be added.
    async fn handle_add_user_request(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let request: AddUserRequest = crate::msgpack::from_slice(payload)?;

        // A secret already spent is silently ignored rather than rejected:
        // answering would confirm to whoever replayed it that it was once real.
        if self.store.secret_is_burned(&request.secret)? {
            return Ok(());
        }

        let Some(offer) = self.store.pairing_offer_by_secret(&request.secret)? else {
            self.reject_add_user(peer).await;
            return Ok(());
        };

        // Burn before checking expiry, so a late arrival cannot leave the
        // secret usable.
        self.store.burn_secret(&request.secret)?;

        if offer.is_expired(crate::now()) {
            self.reject_add_user(peer).await;
            return Ok(());
        }

        let requester: User = crate::msgpack::from_slice(&request.requester_user)?;

        if let Err(problem) = self.vet_incoming_user(&requester, peer) {
            tracing::warn!(%problem, "rejecting an add user request");
            self.reject_add_user(peer).await;
            return Ok(());
        }

        let me = self.store.profile()?.ok_or(Error::NoProfile)?;
        let offer_user = crate::msgpack::to_vec(&shareable(&me))?;

        // Sign the requester's record exactly as it arrived: this signature is
        // what proves our consent to anyone who later sees the assembled
        // record, so it has to cover the same bytes they will hash.
        let accepted = AddUserRequestAccepted {
            offer_signature: self
                .key
                .sign(&crate::crypto::hash(&request.requester_user))
                .to_vec(),
            // Left unset so the encoding matches Go's two-field frame exactly;
            // the recipient uses the address the connection proved.
            offer_device: None,
            offer_user,
        };

        self.send_to(
            peer,
            RawFrame::new(
                FrameType::AddUserRequestAccepted.as_u16(),
                accepted.encode()?,
            ),
        )
        .await;

        Ok(())
    }

    /// Our request was accepted; assemble and publish the record.
    async fn handle_add_user_request_accepted(&self, peer: &str, payload: &[u8]) -> Result<()> {
        // Only from a peer we actually asked. This is the check the Go
        // implementation lacks, and without it any contact can push us into
        // adding a user of their choosing.
        let asked = self
            .pending_add_requests
            .lock()
            .await
            .remove(peer)
            .is_some_and(|asked_at| crate::now() - asked_at <= PENDING_REQUEST_VALIDITY_SECONDS);

        if !asked {
            return Err(Error::NotPermitted(
                "unsolicited add user acceptance",
            ));
        }

        let accepted: AddUserRequestAccepted = crate::msgpack::from_slice(payload)?;
        let offer_user: User = crate::msgpack::from_slice(&accepted.offer_user)?;

        // The Go implementation does not name the signing device: it is
        // whichever device we are connected to, an address the handshake has
        // already proven. Trust the connection over anything in the frame.
        let offer_device = peer.to_string();

        if let Err(problem) = self.vet_incoming_user(&offer_user, peer) {
            return Err(Error::InvalidFrame(format!(
                "add user acceptance rejected: {problem}"
            )));
        }

        // The signing device must be one of theirs, and the signature must
        // cover our record.
        if !offer_user
            .devices
            .iter()
            .any(|device| device.address == offer_device)
        {
            return Err(Error::InvalidFrame(
                "acceptance signed by a device outside the offering user's group".into(),
            ));
        }

        let me = self.store.profile()?.ok_or(Error::NoProfile)?;
        let requester_user = crate::msgpack::to_vec(&shareable(&me))?;

        if !crate::crypto::verify_signature(
            &offer_device,
            &crate::crypto::hash(&requester_user),
            &accepted.offer_signature,
        ) {
            return Err(Error::InvalidSignature);
        }

        let record = AddUser {
            id: Uuid::new_v4(),
            xor: crate::xor(offer_user.id, me.id),
            timestamp: crate::now(),
            saved_at: crate::now(),
            requester_signature: self
                .key
                .sign(&crate::crypto::hash(&accepted.offer_user))
                .to_vec(),
            requester_device: self.network.address().to_string(),
            offer_user: accepted.offer_user,
            requester_user,
            offer_device,
            offer_signature: accepted.offer_signature,
        };

        // Apply it locally exactly as a peer would, then publish it so both
        // device groups converge.
        self.apply_add_user(&record).await?;
        self.broadcast(&record).await?;
        Ok(())
    }

    async fn handle_add_user(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let record: AddUser = crate::msgpack::from_slice(payload)?;

        if self.store.add_user_record(record.id)?.is_some() {
            self.send_ack(peer, record.id, FrameType::AddUser).await;
            return Ok(());
        }

        self.apply_add_user(&record).await?;
        self.send_ack(peer, record.id, FrameType::AddUser).await;
        self.broadcast(&record).await?;
        Ok(())
    }

    /// Validate an add-user record and adopt the counterparty.
    ///
    /// Every check happens here rather than in the handlers, because this is
    /// the one path that both sides and every later-arriving device take.
    async fn apply_add_user(&self, record: &AddUser) -> Result<()> {
        let my_id = self.store.my_user_id()?;

        if !record.signatures_are_valid() {
            return Err(Error::InvalidSignature);
        }
        if !record.signers_are_members()? {
            return Err(Error::InvalidFrame(
                "an add user signature came from a device outside the group it speaks for".into(),
            ));
        }
        if !record.xor_matches_users()? {
            return Err(Error::InvalidFrame(
                "add user record names a pair it was not made for".into(),
            ));
        }

        let offerer = record.offerer()?;
        let requester = record.requester()?;

        // Whichever of the two is not us is the new contact.
        //
        // Both blobs come off the wire, so "which side is us" is an attacker's
        // claim until checked. `signers_are_members` only proves each signature
        // came from a device listed in its *own* blob — and the attacker wrote
        // both blobs. So a record naming our user ID with the attacker's device
        // in our device list would otherwise verify perfectly, and we would
        // adopt them as a contact having consented to nothing.
        //
        // The fix is to require that our side was signed by a device we
        // actually own, according to our own database.
        let (counterparty, our_signing_device) = if offerer.id == my_id {
            (requester, &record.offer_device)
        } else if requester.id == my_id {
            (offerer, &record.requester_device)
        } else {
            // A record between two other people, relayed to us. It is valid but
            // says nothing about our contacts, so it is stored for gossip only.
            self.store.save_add_user(record)?;
            return Ok(());
        };

        // Our consent has to be verifiable against devices we hold, not against
        // a device list the record supplies.
        match self.store.device_owner(our_signing_device)? {
            Some(owner) if owner == my_id => {}
            _ => {
                return Err(Error::InvalidFrame(
                    "add user record claims our consent but was not signed by a device we own"
                        .into(),
                ))
            }
        }

        if !device_group::user_has_valid_device_group(&counterparty) {
            return Err(Error::InvalidFrame(
                "the new contact has an invalid device group".into(),
            ));
        }

        // Every device in the record must claim the user it is filed under.
        // Otherwise a device row lands under an id the record chose, and the
        // upsert in `save_device` would let it overwrite an unrelated device.
        if counterparty
            .devices
            .iter()
            .any(|device| device.user_id != counterparty.id)
        {
            return Err(Error::InvalidFrame(
                "a device in the new contact's group claims a different owner".into(),
            ));
        }

        let existing = self.store.user(counterparty.id)?;
        let mut adopted = shareable(&counterparty);

        match &existing {
            Some(known) => {
                // Someone we already know. Their record here is not authority to
                // replace their device group wholesale — the Go implementation
                // merges it unconditionally, which lets a stale or hostile copy
                // introduce devices. Only devices that slot into the group we
                // already hold are taken.
                adopted.devices = known.devices.clone();
                for device in &counterparty.devices {
                    let already_known = adopted
                        .devices
                        .iter()
                        .any(|existing| existing.address == device.address);

                    if already_known {
                        continue;
                    }
                    if device_group::is_valid_addition(&adopted, device) {
                        adopted.devices.push(device.clone());
                    } else {
                        tracing::warn!(
                            user = %counterparty.id,
                            address = %device.address,
                            "ignoring a device that does not fit the known device group"
                        );
                    }
                }

                // Local state — alias, mute, retention — is ours, not theirs.
                adopted.alias = known.alias.clone();
                adopted.notes = known.notes.clone();
                adopted.blocked = known.blocked;
                adopted.accepted = known.accepted;
                adopted.open_dm = known.open_dm;
                adopted.introduction_method = known.introduction_method.clone();
                adopted.introduction_time = known.introduction_time;
                adopted.introduction_metadata = known.introduction_metadata;
                // `save_user` persists last_activity, so leaving it at the
                // wire record's value would reset a known contact's ordering
                // in the conversation list.
                adopted.last_activity = known.last_activity;
            }
            None => {
                adopted.accepted = true;
                // A scanned contact opens a conversation straight away; one met
                // through a group does not.
                adopted.open_dm = true;
                adopted.introduction_method = crate::types::introduction::ADD_USER.to_string();
                adopted.introduction_time = crate::now();
                adopted.introduction_metadata = record.id;
                // A new contact counts as active from the moment they are
                // added, which is what Go's create hook does. Leaving it at
                // zero would make peering treat somebody you met a second ago
                // as dormant and never dial them.
                adopted.last_activity = crate::now();
                adopted.last_activity = crate::now();
            }
        }

        // A device address is a public key, so two users claiming the same one
        // is a contradiction rather than a merge.
        for device in &adopted.devices {
            if let Some(owner) = self.store.device_owner(&device.address)? {
                if owner != adopted.id {
                    return Err(Error::InvalidFrame(format!(
                        "device {} is already held by another user",
                        device.address
                    )));
                }
            }
        }

        self.store.save_user(&adopted)?;
        self.store.save_add_user(record)?;

        let event = if existing.is_some() {
            Event::UserUpdated {
                user: self.user_view(&adopted, false),
            }
        } else {
            Event::UserAdded {
                user: self.user_view(&adopted, false),
            }
        };
        self.emit(event);

        Ok(())
    }

    async fn handle_add_user_rejected(&self, peer: &str) -> Result<()> {
        self.pending_add_requests.lock().await.remove(peer);
        self.emit(Event::Error {
            message: "The other device declined the request. Codes expire after five \
                      minutes — try generating a fresh one."
                .into(),
        });
        Ok(())
    }

    async fn reject_add_user(&self, peer: &str) {
        if let Ok(payload) = (AddUserRequestRejected {}).encode() {
            self.send_to(
                peer,
                RawFrame::new(FrameType::AddUserRequestRejected.as_u16(), payload),
            )
            .await;
        }
    }

    /// Checks common to both directions of the exchange.
    fn vet_incoming_user(&self, user: &User, peer: &str) -> std::result::Result<(), String> {
        if !crate::frames::identity::valid_user_name(&user.name) {
            // The Go implementation skips this, so a peer can set any name at
            // all — including one with newlines, to spoof interface layout.
            return Err("the user's name is not acceptable".into());
        }
        if !device_group::user_has_valid_device_group(user) {
            return Err("the user's device group does not validate".into());
        }
        if !user.devices.iter().any(|device| device.address == peer) {
            return Err("the request did not come from a device in the user's group".into());
        }
        if user.devices.iter().any(|device| device.user_id != user.id) {
            return Err("a device in the group claims a different owner".into());
        }
        if let Ok(my_id) = self.store.my_user_id() {
            if user.id == my_id {
                return Err("the user claims to be us".into());
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Conversation settings
    // ---------------------------------------------------------------------

    /// Build, store, apply and broadcast a change to a direct message thread.
    ///
    /// Which devices see it depends on the type: retention and history clearing
    /// are properties of the conversation and reach the counterparty, while
    /// mute state, aliases and notes are this user's private view and stay
    /// inside their own device group. [`UpdateDm::scope`] encodes that.
    async fn apply_update_dm(
        &self,
        counterparty: Uuid,
        kind: UpdateDmType,
        data: Vec<u8>,
    ) -> Result<()> {
        let my_id = self.store.my_user_id()?;

        let mut update = UpdateDm::new(
            my_id,
            crate::xor(my_id, counterparty),
            kind,
            data,
            crate::now(),
        );
        update.saved_at = crate::now();

        let body = crate::msgpack::to_vec(&update)?;
        let container = SignedContainer::create(&self.key, body);
        update.signed = SignedFrame::from_container(&container);

        self.apply_dm_setting_locally(counterparty, kind, &update.data)?;

        // Retention and history clearing belong to the conversation rather
        // than to one side's view of it, so they are kept as frames: the
        // counterparty needs them, and both timelines record who made the
        // change. Everything else is private and leaves no trace.
        if kind.leaves_a_record() {
            self.store.save_update_dm(&update)?;
            self.emit_dm_system_message(&update);
        }

        self.broadcast(&update).await?;
        Ok(())
    }

    /// A change to a conversation arrived from the other side.
    async fn handle_update_dm(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let (mut update, signed) = self.unpack_signed::<UpdateDm>(payload)?;
        update.signed = signed;

        if !self.signer_speaks_for(&update.signed.signer, update.actor, update.timestamp)? {
            return Err(Error::InvalidFrame(
                "conversation update signer does not speak for its actor".into(),
            ));
        }

        let kind = UpdateDmType::from_u16(update.update_type)?;
        // Only the shared kinds cross the wire at all. A frame claiming to set
        // our mute state, or our private alias for somebody, is not a setting
        // the other side gets to have an opinion about.
        if !kind.is_shared() {
            self.send_ack(peer, update.id, FrameType::UpdateDm).await;
            return Err(Error::NotPermitted(
                "that conversation setting is not the other side's to change",
            ));
        }

        let my_id = self.store.my_user_id()?;
        let counterparty = crate::xor(update.target, my_id);
        // The actor must be a participant: either the person we are talking to
        // or one of our own devices catching us up.
        if update.actor != counterparty && update.actor != my_id {
            return Err(Error::NotPermitted("actor is not in the conversation"));
        }
        if let Some(user) = self.store.user(update.actor)? {
            if user.blocked {
                self.send_ack(peer, update.id, FrameType::UpdateDm).await;
                return Ok(());
            }
        }

        if self.store.has_frame(update.id, FrameType::UpdateDm)? {
            self.send_ack(peer, update.id, FrameType::UpdateDm).await;
            return Ok(());
        }

        update.saved_at = crate::now();
        if kind.leaves_a_record() {
            self.store.save_update_dm(&update)?;
        }
        self.send_ack(peer, update.id, FrameType::UpdateDm).await;

        self.apply_dm_setting_locally(counterparty, kind, &update.data)?;

        // Clearing the history is the conversation's decision, not one
        // device's, so the messages go here too.
        if kind == UpdateDmType::SetClearBefore {
            if let Some(cutoff) = <[u8; 8]>::try_from(update.data.as_slice())
                .ok()
                .map(i64::from_le_bytes)
            {
                for message_id in self.store.delete_messages_before(counterparty, cutoff)? {
                    self.emit(Event::MessageDeleted { message_id });
                }
            }
        }

        self.emit_dm_system_message(&update);
        self.broadcast(&update).await?;
        Ok(())
    }

    /// Fold a direct message setting into the local user record.
    fn apply_dm_setting_locally(
        &self,
        counterparty: Uuid,
        kind: UpdateDmType,
        data: &[u8],
    ) -> Result<()> {
        let Some(mut user) = self.store.user(counterparty)? else {
            return Err(Error::UserNotFound);
        };

        let as_i64 = || {
            <[u8; 8]>::try_from(data)
                .map(i64::from_le_bytes)
                .map_err(|_| Error::InvalidFrame("expected an 8 byte integer".into()))
        };
        let as_text = || {
            std::str::from_utf8(data)
                .map(str::to_string)
                .map_err(|_| Error::InvalidFrame("expected UTF-8".into()))
        };

        match kind {
            UpdateDmType::ChangeMutedUntil => user.muted_until = as_i64()?,
            UpdateDmType::ChangeRetention => user.retention = as_i64()?,
            UpdateDmType::SetClearBefore => user.clear_before = as_i64()?,
            UpdateDmType::SetAlias => user.alias = as_text()?,
            UpdateDmType::SetNotes => user.notes = as_text()?,
            UpdateDmType::SetBlocked => user.blocked = data.first().is_some_and(|byte| *byte != 0),
            UpdateDmType::SetOpen => user.open_dm = data.first().is_some_and(|byte| *byte != 0),
            UpdateDmType::SetReadReceipts => {
                if data.len() != 2 {
                    return Err(Error::InvalidFrame("expected two bytes".into()));
                }
                user.read_receipts_overridden = data[0] != 0;
                user.read_receipts_enabled = data[1] != 0;
            }
            UpdateDmType::SetTypingIndicators => {
                if data.len() != 2 {
                    return Err(Error::InvalidFrame("expected two bytes".into()));
                }
                user.typing_indicators_overridden = data[0] != 0;
                user.typing_indicators_enabled = data[1] != 0;
            }
            UpdateDmType::OfferRetention => {}
        }

        self.store.update_user_local_state(&user)?;
        self.emit(Event::UserUpdated {
            user: self.user_view(&user, false),
        });
        Ok(())
    }

    /// Mute a conversation until a timestamp. [`crate::MUTED_FOREVER`] mutes
    /// indefinitely; zero unmutes.
    pub async fn set_muted_until(&self, conversation: Uuid, until: i64) -> Result<()> {
        if self.store.group(conversation)?.is_some() {
            let my_id = self.store.my_user_id()?;
            let update = self.sign_update_group(
                my_id,
                conversation,
                UpdateGroupType::ChangeMutedUntil,
                UpdateGroup::encode_i64(until),
            )?;
            return self.apply_update_group(update).await;
        }

        self.apply_update_dm(
            conversation,
            UpdateDmType::ChangeMutedUntil,
            until.to_le_bytes().to_vec(),
        )
        .await
    }

    /// Block or unblock a contact.
    ///
    /// A blocked contact's frames are dropped on arrival rather than merely
    /// hidden, so blocking is enforced at the protocol layer and not just in
    /// the interface.
    pub async fn set_user_blocked(&self, user_id: Uuid, blocked: bool) -> Result<()> {
        self.apply_update_dm(user_id, UpdateDmType::SetBlocked, vec![u8::from(blocked)])
            .await
    }

    /// Show or hide a direct conversation.
    ///
    /// Not a local view toggle: `SetOpen` is sync-scoped, so the choice reaches
    /// this profile's other devices. Nothing is discarded — the contact, their
    /// messages and their device group all stay — which is what makes it the
    /// reversible counterpart to blocking, and the way back to somebody whose
    /// conversation was closed.
    pub async fn set_open_dm(&self, user_id: Uuid, open: bool) -> Result<()> {
        self.apply_update_dm(user_id, UpdateDmType::SetOpen, vec![u8::from(open)])
            .await
    }

    /// Stamp a conversation as opened now.
    ///
    /// Local on both implementations — there is nothing to send — so this is
    /// the one conversation setting that is not a frame. A thread holding an
    /// unsent draft should stay where the user left it rather than sinking to
    /// the age of its last message, and this is what the ordering reads.
    pub fn set_last_opened(&self, conversation: Uuid) -> Result<()> {
        let now = crate::now();
        self.store.note_conversation_opened(conversation, now)?;

        if let Some(group) = self.store.group(conversation)? {
            self.emit(Event::GroupUpdated {
                group: Box::new(self.group_view(&group)),
            });
        } else if let Some(user) = self.store.user(conversation)? {
            self.emit(Event::UserUpdated {
                user: self.user_view(&user, false),
            });
        }
        Ok(())
    }

    /// Give a contact a local nickname, or clear it with an empty string.
    pub async fn set_user_alias(&self, user_id: Uuid, alias: &str) -> Result<()> {
        if !alias.is_empty() && !crate::frames::identity::valid_user_name(alias) {
            return Err(Error::InvalidFrame("invalid alias".into()));
        }
        self.apply_update_dm(user_id, UpdateDmType::SetAlias, alias.as_bytes().to_vec())
            .await
    }

    /// Attach private notes to a contact.
    pub async fn set_user_notes(&self, user_id: Uuid, notes: &str) -> Result<()> {
        self.apply_update_dm(user_id, UpdateDmType::SetNotes, notes.as_bytes().to_vec())
            .await
    }

    /// How long messages in a conversation are kept; zero keeps them forever.
    pub async fn set_retention(&self, conversation: Uuid, seconds: i64) -> Result<()> {
        if self.store.group(conversation)?.is_some() {
            let my_id = self.store.my_user_id()?;
            let update = self.sign_update_group(
                my_id,
                conversation,
                UpdateGroupType::ChangeRetention,
                UpdateGroup::encode_i64(seconds),
            )?;
            return self.apply_update_group(update).await;
        }

        self.apply_update_dm(
            conversation,
            UpdateDmType::ChangeRetention,
            seconds.to_le_bytes().to_vec(),
        )
        .await
    }

    /// Clear a conversation's history, for everyone in it.
    ///
    /// This does not merely hide messages locally: the cutoff is shared, so
    /// every device in scope drops what came before it and refuses to accept it
    /// again from a peer that has not caught up.
    pub async fn clear_history(&self, conversation: Uuid) -> Result<()> {
        let cutoff = crate::now();

        if self.store.group(conversation)?.is_some() {
            let my_id = self.store.my_user_id()?;
            let update = self.sign_update_group(
                my_id,
                conversation,
                UpdateGroupType::SetClearBefore,
                UpdateGroup::encode_i64(cutoff),
            )?;
            self.apply_update_group(update).await?;
        } else {
            self.apply_update_dm(
                conversation,
                UpdateDmType::SetClearBefore,
                cutoff.to_le_bytes().to_vec(),
            )
            .await?;
        }

        for message_id in self.store.delete_messages_before(conversation, cutoff)? {
            self.emit(Event::MessageDeleted { message_id });
        }
        Ok(())
    }

    /// Set per-conversation read receipt behaviour, or clear the override.
    pub async fn set_read_receipts(&self, conversation: Uuid, setting: Option<bool>) -> Result<()> {
        let data = vec![u8::from(setting.is_some()), u8::from(setting.unwrap_or(true))];

        if self.store.group(conversation)?.is_some() {
            let my_id = self.store.my_user_id()?;
            let update = self.sign_update_group(
                my_id,
                conversation,
                UpdateGroupType::SetReadReceiptSettings,
                data,
            )?;
            return self.apply_update_group(update).await;
        }

        self.apply_update_dm(conversation, UpdateDmType::SetReadReceipts, data)
            .await
    }

    /// Set per-conversation typing indicator behaviour, or clear the override.
    pub async fn set_typing_indicators(
        &self,
        conversation: Uuid,
        setting: Option<bool>,
    ) -> Result<()> {
        let data = vec![u8::from(setting.is_some()), u8::from(setting.unwrap_or(true))];

        if self.store.group(conversation)?.is_some() {
            let my_id = self.store.my_user_id()?;
            let update = self.sign_update_group(
                my_id,
                conversation,
                UpdateGroupType::SetTypingIndicatorSettings,
                data,
            )?;
            return self.apply_update_group(update).await;
        }

        self.apply_update_dm(conversation, UpdateDmType::SetTypingIndicators, data)
            .await
    }

    // ---------------------------------------------------------------------
    // Group management
    // ---------------------------------------------------------------------

    /// Remove a member. Removing yourself is leaving.
    pub async fn remove_from_group(&self, group_id: Uuid, user_id: Uuid) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        let update = self.sign_update_group(
            my_id,
            group_id,
            UpdateGroupType::RemoveUser,
            user_id.as_bytes().to_vec(),
        )?;
        self.apply_update_group(update).await
    }

    /// Withdraw an invitation that has not been answered.
    pub async fn revoke_invite(&self, group_id: Uuid, user_id: Uuid) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        let update = self.sign_update_group(
            my_id,
            group_id,
            UpdateGroupType::RevokeInvite,
            user_id.as_bytes().to_vec(),
        )?;
        self.apply_update_group(update).await
    }

    /// Promote or demote a group administrator.
    pub async fn set_group_admin(&self, group_id: Uuid, user_id: Uuid, admin: bool) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        let kind = if admin {
            UpdateGroupType::PromoteAdmin
        } else {
            UpdateGroupType::DemoteAdmin
        };
        let update =
            self.sign_update_group(my_id, group_id, kind, user_id.as_bytes().to_vec())?;
        self.apply_update_group(update).await
    }

    /// Delete a group for everyone. Admins only.
    pub async fn delete_group(&self, group_id: Uuid) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        let update =
            self.sign_update_group(my_id, group_id, UpdateGroupType::Delete, Vec::new())?;
        self.apply_update_group(update).await
    }

    /// Block a group, which also leaves it and stops tracking further changes.
    pub async fn block_group(&self, group_id: Uuid) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        let update = self.sign_update_group(my_id, group_id, UpdateGroupType::Block, Vec::new())?;
        self.apply_update_group(update).await
    }

    /// Set one of a group's three permission restrictions. Admins only.
    pub async fn set_group_permission(
        &self,
        group_id: Uuid,
        permission: GroupPermission,
        restricted: bool,
    ) -> Result<()> {
        use consensus::state::sentinels;

        let my_id = self.store.my_user_id()?;
        let kind = match permission {
            GroupPermission::Posting => UpdateGroupType::ChangePostingPermission,
            GroupPermission::Edits => UpdateGroupType::ChangeGroupEditsPermission,
            GroupPermission::UserManagement => UpdateGroupType::ChangeUserManagementPermission,
        };
        let byte = if restricted {
            sentinels::PERMISSION_RESTRICTED
        } else {
            sentinels::PERMISSION_UNRESTRICTED
        };

        let update = self.sign_update_group(my_id, group_id, kind, vec![byte])?;
        self.apply_update_group(update).await
    }

    // ---------------------------------------------------------------------
    // Profile
    // ---------------------------------------------------------------------

    /// Change the name contacts see, and tell them.
    pub async fn update_profile_name(&self, name: &str) -> Result<()> {
        if !crate::frames::identity::valid_user_name(name) {
            return Err(Error::InvalidFrame("invalid profile name".into()));
        }

        let my_id = self.store.my_user_id()?;
        let mut profile = self.store.profile()?.ok_or(Error::NoProfile)?;
        let previous = profile.name.clone();
        profile.name = name.to_string();
        self.store.save_user(&profile)?;

        // Profile updates are replayed in timestamp order, and timestamps have
        // one-second resolution — so two renames in the same second would be
        // ordered by their random ids, and the older one could win. Advancing
        // past the newest we hold is the same rule `sign_update_group` follows,
        // for the same reason.
        let latest_known = self
            .store
            .updates_for_user(my_id)?
            .iter()
            .map(|update| update.timestamp)
            .max()
            .unwrap_or(0);

        let mut update = UpdateUser::new(
            my_id,
            crate::frames::update::UpdateUserType::UpdateName,
            name.as_bytes().to_vec(),
            crate::now().max(latest_known + 1),
        );
        update.saved_at = crate::now();
        update.previous_data = previous.into_bytes();

        let body = crate::msgpack::to_vec(&update)?;
        let container = SignedContainer::create(&self.key, body);
        update.signed = SignedFrame::from_container(&container);

        // Stored, not merely broadcast: a device that was offline for the
        // rename is caught up with the frame, and our other devices replay it
        // to reach the same name rather than being told what to write.
        self.store.save_update_user(&update)?;

        self.emit(Event::UserUpdated {
            user: self.user_view(&profile, true),
        });
        self.broadcast(&update).await?;
        Ok(())
    }

    /// Apply a profile change somebody made to themselves.
    async fn handle_update_user(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let (mut update, signed) = self.unpack_signed::<UpdateUser>(payload)?;
        update.signed = signed;

        // A profile is the user's own to change and nobody else's, so the
        // author and the target are the same person by definition.
        if !self.signer_speaks_for(&update.signed.signer, update.target, update.timestamp)? {
            return Err(Error::InvalidFrame(
                "update user signer does not speak for the user it changes".into(),
            ));
        }
        if let Some(target) = self.store.user(update.target)? {
            if target.blocked {
                // Acknowledged, so a blocked contact stops offering it.
                self.send_ack(peer, update.id, FrameType::UpdateUser).await;
                return Ok(());
            }
        }
        if self.store.has_frame(update.id, FrameType::UpdateUser)? {
            self.send_ack(peer, update.id, FrameType::UpdateUser).await;
            return Ok(());
        }
        if !update_user_payload_is_valid(&update) {
            self.send_ack(peer, update.id, FrameType::UpdateUser).await;
            return Err(Error::InvalidFrame("unusable update user payload".into()));
        }

        update.saved_at = crate::now();
        update.previous_data = self.previous_profile_value(&update)?;
        self.store.save_update_user(&update)?;
        self.send_ack(peer, update.id, FrameType::UpdateUser).await;

        self.replay_profile_updates(update.target)?;
        self.broadcast(&update).await?;
        Ok(())
    }

    /// Rebuild a user's profile from every update we hold for them.
    ///
    /// Replaying rather than applying the update that just arrived is what
    /// makes the outcome independent of arrival order — the same reason group
    /// state is rebuilt rather than mutated. It is also what makes a rename
    /// signed by a device that was later revoked drop out of the result.
    fn replay_profile_updates(&self, target: Uuid) -> Result<()> {
        use crate::frames::update::UpdateUserType;

        let Some(mut user) = self.store.user(target)? else {
            // A profile change for somebody we have never met. The frame is
            // kept and relayed; there is no row to apply it to.
            return Ok(());
        };

        let mut name = user.name.clone();
        let mut images = user.image_ids();

        for update in self.store.updates_for_user(target)? {
            if !self.signer_speaks_for(&update.signed.signer, target, update.timestamp)? {
                continue;
            }
            match update.kind() {
                Ok(UpdateUserType::UpdateName) => {
                    if let Ok(new_name) = std::str::from_utf8(&update.data) {
                        if crate::frames::identity::valid_user_name(new_name) {
                            name = new_name.to_string();
                        }
                    }
                }
                Ok(UpdateUserType::UpdateImage) => {
                    if let Ok(image) = Uuid::from_slice(&update.data) {
                        if !images.contains(&image) {
                            images.push(image);
                        }
                    }
                }
                // Key rolling and encrypted device management are their own
                // gaps; replaying them here would apply half a feature.
                _ => {}
            }
        }

        let images = crate::frames::identity::join_uuid_list(&images);
        if name == user.name && images == user.images {
            return Ok(());
        }

        user.name = name;
        user.images = images;
        self.store.save_user(&user)?;
        self.emit(Event::UserUpdated {
            user: self.user_view(&user, false),
        });
        Ok(())
    }

    /// The value an update replaces, kept so the interface can say what a name
    /// changed *from*.
    fn previous_profile_value(&self, update: &UpdateUser) -> Result<Vec<u8>> {
        use crate::frames::update::UpdateUserType;

        if !matches!(update.kind(), Ok(UpdateUserType::UpdateName)) {
            return Ok(Vec::new());
        }
        // The newest earlier rename, or failing that whatever the row says now.
        let earlier = self
            .store
            .updates_for_user(update.target)?
            .into_iter()
            .rfind(|stored| {
                matches!(stored.kind(), Ok(UpdateUserType::UpdateName))
                    && stored.timestamp < update.timestamp
            })
            .map(|stored| stored.data);

        Ok(match earlier {
            Some(data) => data,
            None => self
                .store
                .user(update.target)?
                .map(|user| user.name.into_bytes())
                .unwrap_or_default(),
        })
    }

    // ---------------------------------------------------------------------
    // Read receipts
    // ---------------------------------------------------------------------

    /// Mark a message as read, and tell the people entitled to know.
    ///
    /// Whether the author actually learns of it depends on this device's
    /// settings — see [`Engine::send_read_receipt`].
    pub async fn mark_as_read(&self, target: Uuid, target_type: FrameType) -> Result<()> {
        // Idempotent: the interface marks whatever is on screen, which happens
        // on every render, and each call would otherwise mint and broadcast a
        // fresh receipt for a message already read.
        if !self.store.mark_seen(target, target_type)? {
            return Ok(());
        }

        // Tell our own interface immediately, so it stops re-reporting the
        // message as unread.
        self.emit(Event::MessageSeen { message_id: target });

        self.send_read_receipt(target, target_type).await
    }

    /// Build, store, and broadcast a read receipt.
    ///
    /// Disabling read receipts does not suppress the receipt — it narrows its
    /// scope to this user's own devices, so their other devices still learn the
    /// message was read while the author does not. Doing it that way rather
    /// than by not sending keeps the two devices' view of "seen" consistent.
    async fn send_read_receipt(&self, target: Uuid, target_type: FrameType) -> Result<()> {
        let my_id = self.store.my_user_id()?;

        let Some(context) = self.read_receipt_context(target, target_type)? else {
            // Nothing to derive a scope from; the message is not one receipts
            // are defined for.
            return Ok(());
        };

        // Never send a receipt for a message we wrote ourselves.
        if context.author == my_id {
            return Ok(());
        }

        let mut receipt = ReadReceipt {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            actor: my_id,
            destination: context.destination,
            scope: context.scope.as_i64(),
            target,
            target_type: target_type.as_u16(),
            timestamp: crate::now(),
            saved_at: crate::now(),
        };

        let body = crate::msgpack::to_vec(&receipt)?;
        let container = SignedContainer::create(&self.key, body);
        receipt.signed = SignedFrame::from_container(&container);

        self.store.save_read_receipt(&receipt)?;
        self.broadcast(&receipt).await?;
        Ok(())
    }

    /// Where a read receipt should go, and who wrote the message it refers to.
    ///
    /// `destination` and `scope` are deliberately not carried on the wire: each
    /// device derives them from its own copy of the target message and its own
    /// privacy settings. A peer therefore cannot widen a receipt's audience by
    /// claiming a scope, and turning receipts off narrows both what this device
    /// sends and what it is willing to surface.
    fn read_receipt_context(
        &self,
        target: Uuid,
        target_type: FrameType,
    ) -> Result<Option<ReadReceiptContext>> {
        let my_id = self.store.my_user_id()?;
        let settings = self
            .store
            .profile_settings(my_id)?
            .unwrap_or_else(|| ProfileSettings::defaults(my_id));

        match target_type {
            FrameType::GroupMessage => {
                let Some(message) = self.store.group_message(target)? else {
                    return Ok(None);
                };
                let enabled = match self.store.group(message.destination)? {
                    Some(group) => resolve_override(
                        group.read_receipts_overridden,
                        group.read_receipts_enabled,
                        settings.default_send_read_receipts,
                    ),
                    None => settings.default_send_read_receipts,
                };

                Ok(Some(ReadReceiptContext {
                    destination: message.destination,
                    author: message.author,
                    scope: if enabled { Scope::Group } else { Scope::Sync },
                }))
            }

            FrameType::DirectMessage => {
                let Some(message) = self.store.direct_message(target)? else {
                    return Ok(None);
                };
                let counterparty = crate::xor(message.xor, my_id);

                // A note to self has nobody to inform.
                if counterparty == my_id || message.xor.is_nil() {
                    return Ok(Some(ReadReceiptContext {
                        destination: counterparty,
                        author: message.author,
                        scope: Scope::Sync,
                    }));
                }

                let enabled = match self.store.user(counterparty)? {
                    Some(user) => resolve_override(
                        user.read_receipts_overridden,
                        user.read_receipts_enabled,
                        settings.default_send_read_receipts,
                    ),
                    None => settings.default_send_read_receipts,
                };

                Ok(Some(ReadReceiptContext {
                    destination: counterparty,
                    author: message.author,
                    scope: if enabled { Scope::User } else { Scope::Sync },
                }))
            }

            // Receipts are only defined for messages.
            _ => Ok(None),
        }
    }

    /// Whether a user could legitimately have read a message.
    ///
    /// Only the participants in a conversation can have read anything in it.
    /// Without this a stranger's receipt would be stored, relayed to everyone
    /// in the conversation, and shown to the author as though they had read it.
    fn may_have_read(&self, actor: Uuid, target: Uuid, target_type: FrameType) -> Result<bool> {
        Ok(match target_type {
            FrameType::GroupMessage => match self.store.group_message(target)? {
                Some(message) => match self.store.group(message.destination)? {
                    Some(group) => group.member_ids().contains(&actor),
                    // Unknown group: nothing to judge against, so no.
                    None => false,
                },
                None => false,
            },
            FrameType::DirectMessage => match self.store.direct_message(target)? {
                Some(message) => {
                    // Exactly the two people in the thread, and no one else.
                    let my_id = self.store.my_user_id()?;
                    let counterparty = crate::xor(message.xor, my_id);
                    actor == message.author || actor == counterparty || actor == my_id
                }
                None => false,
            },
            _ => false,
        })
    }

    async fn handle_read_receipt(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let (mut receipt, signed) = self.unpack_signed::<ReadReceipt>(payload)?;
        receipt.signed = signed;

        if !self.signer_speaks_for(&receipt.signed.signer, receipt.actor, receipt.timestamp)? {
            return Err(Error::InvalidFrame(
                "read receipt signer does not speak for its actor".into(),
            ));
        }

        if let Some(actor) = self.store.user(receipt.actor)? {
            if actor.blocked {
                self.send_ack(peer, receipt.id, FrameType::ReadReceipt).await;
                return Ok(());
            }
        }

        if self.store.has_frame(receipt.id, FrameType::ReadReceipt)? {
            self.send_ack(peer, receipt.id, FrameType::ReadReceipt).await;
            return Ok(());
        }

        let my_id = self.store.my_user_id()?;
        let target_type = FrameType::from_u16(receipt.target_type)?;
        receipt.saved_at = crate::now();

        let context = self.read_receipt_context(receipt.target, target_type)?;

        // Only judge entitlement once the target is known; an early receipt is
        // re-checked when the message lands.
        if context.is_some() && !self.may_have_read(receipt.actor, receipt.target, target_type)? {
            return Err(Error::NotPermitted(
                "read receipt author is not part of the conversation",
            ));
        }

        let Some(context) = context else {
            // The receipt arrived before the message it refers to. Store it
            // unresolved so it is not lost, acknowledge it so the peer stops
            // offering it, but do not relay something we cannot scope.
            receipt.destination = Uuid::nil();
            receipt.scope = Scope::Sync.as_i64();
            self.store.save_read_receipt(&receipt)?;
            self.send_ack(peer, receipt.id, FrameType::ReadReceipt).await;
            return Ok(());
        };

        receipt.destination = context.destination;
        receipt.scope = context.scope.as_i64();
        self.store.save_read_receipt(&receipt)?;
        self.send_ack(peer, receipt.id, FrameType::ReadReceipt).await;

        if receipt.actor == my_id {
            // We read this on another of our own devices.
            self.store.mark_seen(receipt.target, target_type)?;
            self.emit(Event::MessageSeen {
                message_id: receipt.target,
            });
        } else if context.author == my_id && context.scope != Scope::Sync {
            // Somebody read something we wrote. The scope check is what makes
            // disabling receipts symmetric: with them off we do not surface
            // other people's either.
            self.emit(Event::MessageRead {
                message_id: receipt.target,
                user_id: receipt.actor,
            });
        }

        self.broadcast(&receipt).await?;
        Ok(())
    }

    /// Resolve any receipts that arrived before a message did.
    ///
    /// Called once the message lands, so a receipt that could not be scoped on
    /// arrival still takes effect.
    async fn resolve_early_read_receipts(&self, target: Uuid, target_type: FrameType) -> Result<()> {
        let pending = self.store.unresolved_read_receipts_for(target)?;
        if pending.is_empty() {
            return Ok(());
        }

        let Some(context) = self.read_receipt_context(target, target_type)? else {
            return Ok(());
        };
        let my_id = self.store.my_user_id()?;

        for receipt in pending {
            if !self.may_have_read(receipt.actor, target, target_type)? {
                // Now that the message is known, this receipt turns out to be
                // from somebody outside the conversation.
                continue;
            }

            self.store
                .resolve_read_receipt(receipt.id, context.destination, context.scope.as_i64())?;

            if receipt.actor == my_id {
                self.store.mark_seen(target, target_type)?;
                self.emit(Event::MessageSeen { message_id: target });
            } else if context.author == my_id && context.scope != Scope::Sync {
                self.emit(Event::MessageRead {
                    message_id: target,
                    user_id: receipt.actor,
                });
            }
        }

        Ok(())
    }

    // ---------------------------------------------------------------------
    // Typing indicators
    // ---------------------------------------------------------------------

    /// Announce that this user is composing a message.
    ///
    /// Safe to call on every keystroke: sends are throttled to one per
    /// [`TYPING_SEND_COOLDOWN_SECONDS`] per thread.
    pub async fn typing_in(&self, thread: Uuid, message_type: FrameType) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        let now = crate::now();

        // Respect the recipient's-side setting on our side too: if we have
        // typing indicators off for this conversation, we do not send.
        if !self.typing_indicators_enabled_for(thread, message_type)? {
            return Ok(());
        }

        if self.typing.lock().await.should_wait_before_sending(thread, now) {
            return Ok(());
        }

        // Direct message threads are identified by the XOR of the two users,
        // matching how the frame is routed on both sides. `thread` arrives from
        // the interface as the conversation ID, which for a direct message is
        // the counterparty.
        let wire_thread = if message_type == FrameType::GroupMessage {
            thread
        } else {
            crate::xor(my_id, thread)
        };

        let mut indicator = TypingIndicator {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            thread: wire_thread,
            message_type: message_type.as_u16(),
            author: my_id,
            received_at: now,
        };

        let body = crate::msgpack::to_vec(&indicator)?;
        let container = SignedContainer::create(&self.key, body);
        indicator.signed = SignedFrame::from_container(&container);

        // Record our own indicator as seen, so the copy that comes back from a
        // peer is not treated as news.
        self.typing.lock().await.already_seen(indicator.id, now);

        self.broadcast(&indicator).await?;
        Ok(())
    }

    async fn handle_typing_indicator(&self, _peer: &str, payload: &[u8]) -> Result<()> {
        let (mut indicator, signed) = self.unpack_signed::<TypingIndicator>(payload)?;
        indicator.signed = signed;
        let now = crate::now();

        // Stamped locally: a peer cannot backdate an indicator to make it
        // linger, and cannot post-date one to make it arrive from the future.
        indicator.received_at = now;

        if !self.signer_speaks_for(&indicator.signed.signer, indicator.author, now)? {
            return Err(Error::InvalidFrame(
                "typing indicator signer does not speak for its author".into(),
            ));
        }

        if let Some(author) = self.store.user(indicator.author)? {
            if author.blocked {
                return Ok(());
            }
        }

        let my_id = self.store.my_user_id()?;
        let message_type = FrameType::from_u16(indicator.message_type)?;

        // The author must actually be party to the thread they claim to be
        // typing in. Without this, any device we know could announce itself as
        // typing into any conversation.
        let ui_thread = match message_type {
            FrameType::GroupMessage => {
                let Some(group) = self.store.group(indicator.thread)? else {
                    return Ok(());
                };
                if !group.member_ids().contains(&indicator.author) {
                    return Err(Error::NotPermitted(
                        "typing indicator author is not a member of the group",
                    ));
                }
                indicator.thread
            }
            FrameType::DirectMessage => {
                // XOR ourselves out of the pair to get the counterparty, which
                // is the conversation this belongs to on our side.
                let counterparty = crate::xor(my_id, indicator.thread);

                // The author has to be one of the two people in that pair.
                // Without this any device we know could announce itself as
                // typing into any conversation.
                if indicator.author != my_id && indicator.author != counterparty {
                    return Err(Error::NotPermitted(
                        "typing indicator is for a conversation its author is not part of",
                    ));
                }

                counterparty
            }
            _ => return Ok(()),
        };

        // De-duplicate before relaying, or gossiped copies would circulate.
        if self.typing.lock().await.already_seen(indicator.id, now) {
            return Ok(());
        }

        // Our own echo needs no on-screen indicator.
        if indicator.author != my_id
            && self.typing_indicators_enabled_for(ui_thread, message_type)?
        {
            let fresh = self
                .typing
                .lock()
                .await
                .begin_displaying(indicator.author, ui_thread, now);

            if fresh {
                self.emit(Event::TypingStarted {
                    user_id: indicator.author,
                    thread: ui_thread,
                });
            }
        }

        // Relay onward regardless of our own display setting: suppressing it
        // locally should not cut other people out of the conversation.
        self.broadcast(&indicator).await?;
        Ok(())
    }

    /// Withdraw indicators that have aged out.
    ///
    /// Driven by [`Engine::run_typing_expiry`]; separated so tests can step it
    /// deterministically.
    pub async fn expire_typing_indicators(&self) {
        let expired = self.typing.lock().await.expired(crate::now());
        for (user_id, thread) in expired {
            self.emit(Event::TypingStopped { user_id, thread });
        }
    }

    /// Withdraw a user's indicator immediately, which the arrival of their
    /// message should do.
    async fn clear_typing_indicator(&self, user_id: Uuid, thread: Uuid) {
        if self.typing.lock().await.stop_displaying(user_id, thread) {
            self.emit(Event::TypingStopped { user_id, thread });
        }
    }

    /// Withdraw stale typing indicators on a timer, for as long as the engine
    /// runs.
    pub async fn run_typing_expiry(self: Arc<Self>) {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            ticker.tick().await;
            self.expire_typing_indicators().await;
        }
    }

    /// Give up on messages that have never reached anybody.
    ///
    /// Go schedules a timer per message at send time and re-runs the same query
    /// at start-up (`chat/database.go:180`); the timers do not survive a
    /// restart, so the sweep is the part that actually decides. Four weeks with
    /// no delivery record from any device is the threshold, and the flag is
    /// advisory — the message stays, and the reference flow stops offering it
    /// to anyone but our own devices.
    pub fn mark_stale_messages_undeliverable(&self) -> Result<Vec<Uuid>> {
        let cutoff = crate::now() - crate::UNDELIVERABLE_AFTER_SECONDS;
        let marked = self.store.mark_stale_messages_undeliverable(cutoff)?;

        for message_id in &marked {
            self.emit(Event::MessageUndeliverable {
                message_id: *message_id,
            });
        }
        Ok(marked)
    }

    /// Whether typing indicators are on for a conversation.
    fn typing_indicators_enabled_for(&self, thread: Uuid, message_type: FrameType) -> Result<bool> {
        let my_id = self.store.my_user_id()?;
        let settings = self
            .store
            .profile_settings(my_id)?
            .unwrap_or_else(|| ProfileSettings::defaults(my_id));
        let default = settings.default_send_typing_indicators;

        Ok(match message_type {
            FrameType::GroupMessage => match self.store.group(thread)? {
                Some(group) => resolve_override(
                    group.typing_indicators_overridden,
                    group.typing_indicators_enabled,
                    default,
                ),
                None => default,
            },
            _ => match self.store.user(thread)? {
                Some(user) => resolve_override(
                    user.typing_indicators_overridden,
                    user.typing_indicators_enabled,
                    default,
                ),
                None => default,
            },
        })
    }

    /// Save a draft and sync it to our other devices.
    pub async fn save_draft(&self, thread: Uuid, text: &str) -> Result<()> {
        let mut draft = Draft {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            thread,
            text: text.to_string(),
            timestamp: crate::now(),
            saved: false,
            saved_at: crate::now(),
        };

        let body = crate::msgpack::to_vec(&draft)?;
        let container = SignedContainer::create(&self.key, body);
        draft.signed = SignedFrame::from_container(&container);

        self.store.save_draft(&draft)?;
        self.broadcast(&draft).await?;
        Ok(())
    }

    /// Adopt a draft typed on another of this profile's devices.
    async fn handle_draft(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let (mut draft, signed) = self.unpack_signed::<Draft>(payload)?;
        draft.signed = signed;

        // A draft never leaves the device group, so anything claiming to be one
        // from outside it is not a draft at all.
        let my_id = self.store.my_user_id()?;
        if !self.signer_speaks_for(&draft.signed.signer, my_id, draft.timestamp)? {
            return Err(Error::InvalidFrame(
                "a draft must be signed by one of our own devices".into(),
            ));
        }

        if self.store.has_frame(draft.id, FrameType::Draft)? {
            self.send_ack(peer, draft.id, FrameType::Draft).await;
            return Ok(());
        }

        // Newest wins, and an emptied draft wins a tie: two devices typing into
        // the same thread in the same second must converge, and converging on
        // "cleared" is the harmless direction. This is the rule at
        // `chat/drafts.go:157`.
        if let Some(existing) = self.store.draft_for_thread(draft.thread)? {
            let stale = draft.timestamp < existing.timestamp
                || (draft.timestamp == existing.timestamp && !draft.text.trim().is_empty());
            if stale {
                self.send_ack(peer, draft.id, FrameType::Draft).await;
                return Ok(());
            }
        }

        draft.saved = true;
        draft.saved_at = crate::now();
        self.store.save_draft(&draft)?;
        self.send_ack(peer, draft.id, FrameType::Draft).await;

        self.emit(Event::DraftUpdated {
            draft: DraftView {
                thread: draft.thread,
                text: draft.text.clone(),
            },
        });

        // Relayed onward, so a third device that was offline for the keystroke
        // still ends up with it.
        self.broadcast(&draft).await?;
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Broadcast
    // ---------------------------------------------------------------------

    /// Write a frame to every connected device in its scope that has not
    /// already acknowledged it.
    pub async fn broadcast<B: Broadcastable>(&self, frame: &B) -> Result<()> {
        let my_id = self.store.my_user_id().unwrap_or_else(|_| Uuid::nil());

        let context = StoreScopeContext {
            store: &self.store,
            my_user_id: my_id,
            my_address: self.network.address(),
        };

        let targets = scope::resolve(
            &context,
            frame.scope(my_id),
            frame.destination(my_id),
            frame.author(),
        );

        let payload = frame.payload()?;
        let raw = RawFrame::new(frame.frame_type().as_u16(), payload);

        let peers = self.peers.read().expect("peer map");
        let in_scope = targets.len();
        let mut written = 0usize;
        let mut offline = 0usize;
        let mut unsupported = 0usize;

        for address in targets {
            // A device that has not said it understands this frame type does
            // not get it. The cost of guessing wrong is not a dropped frame:
            // the Go implementation closes the connection on an unknown type
            // (`chat/remote_device.go:224`), and the reference flow would then
            // re-offer this frame on every reconnection, so the peer
            // relationship would fail rather than the feature.
            if frame.frame_type().is_extension()
                && !crate::store::accepts(
                    self.store.device_by_address(&address).ok().flatten().as_ref(),
                    frame.frame_type(),
                )
            {
                unsupported += 1;
                continue;
            }

            // Skip anything the peer already told us it has.
            if self
                .store
                .is_delivered_to(&address, frame.id(), frame.frame_type())
                .unwrap_or(false)
            {
                continue;
            }
            if !peers.contains_key(&address) {
                offline += 1;
            }
            if let Some(peer) = peers.get(&address) {
                written += 1;
                // A full queue means the peer is not keeping up; dropping is
                // correct, because the reference flow will offer the frame
                // again on the next connection.
                let _ = peer.sender.try_send(raw.clone());
            }
        }

        // The commonest reason a frame appears to vanish is that nobody in its
        // scope was connected, which is invisible from the outside.
        tracing::debug!(
            frame = ?frame.frame_type(), id = %frame.id(),
            in_scope, written, offline, unsupported,
            "broadcast",
        );

        Ok(())
    }

    /// Write a frame to one specific device.
    async fn send_to(&self, address: &str, frame: RawFrame) {
        let peers = self.peers.read().expect("peer map");
        if let Some(peer) = peers.get(address) {
            let _ = peer.sender.try_send(frame);
        }
    }

    // ---------------------------------------------------------------------
    // Peer sessions
    // ---------------------------------------------------------------------

    /// Accept connections until the network stops producing them.
    pub async fn run_listener(self: Arc<Self>) {
        // The reference-offer sender rides along here: this is the point at
        // which the engine is live and inside a runtime, and everything that
        // asks for an offer does so as a consequence of a connection.
        if let Some(requests) = self.offer_requests.lock().expect("offer requests").take() {
            tokio::spawn(Arc::clone(&self).run_reference_offers(requests));
        }

        loop {
            match self.network.accept().await {
                Ok(connection) => {
                    let engine = Arc::clone(&self);
                    tokio::spawn(async move {
                        if let Err(error) = engine.serve_peer(connection).await {
                            tracing::debug!(%error, "peer session ended");
                        }
                    });
                }
                Err(error) => {
                    tracing::warn!(%error, "failed to accept a connection");
                    // A tight failure loop would spin the CPU; back off.
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
            }
        }
    }

    /// Dial a peer and serve the resulting session.
    pub async fn connect(self: Arc<Self>, address: &str) -> Result<()> {
        let connection = self.network.dial(address).await?;
        let engine = Arc::clone(&self);
        tokio::spawn(async move {
            if let Err(error) = engine.serve_peer(connection).await {
                tracing::debug!(%error, "peer session ended");
            }
        });
        Ok(())
    }

    /// Drive one peer session to completion.
    async fn serve_peer(self: Arc<Self>, connection: PeerConnection<N::Stream>) -> Result<()> {
        let peer_address = connection.peer_address.clone();
        let (mut reader, mut writer) = tokio::io::split(connection.stream);

        let (sender, mut queue) = mpsc::channel::<Outbound>(256);
        self.peers.write().expect("peer map").insert(
            peer_address.clone(),
            Peer {
                sender: sender.clone(),
            },
        );

        let known = self.store.device_by_address(&peer_address)?;
        if known.is_some() {
            self.store.mark_device_seen(&peer_address, crate::now())?;
            self.emit_device_online(&peer_address).await;
        }
        // Deliberately not `let`: a device can become known *during* this
        // session, and the frame size limit has to follow. See the read loop.
        let mut device_is_known = known.is_some();

        // The writer task owns the socket's write half, so handlers never block
        // on I/O and frames from different tasks cannot interleave mid-frame.
        let writer_task = tokio::spawn(async move {
            while let Some(frame) = queue.recv().await {
                if wire::write_frame(&mut writer, &frame).await.is_err() {
                    break;
                }
            }
        });

        // Open with a reference offer, so a peer that has been away catches up
        // without either side re-sending what the other already holds.
        //
        // Through the retrying path, like Go's `remote_device.go:97`. A socket
        // that has only just been established is the likeliest one to be dead
        // without saying so, and the opening offer is the frame that decides
        // whether anything else happens at all.
        self.offer_references_to_soon(&peer_address);

        // Nothing is asked for here. A new session may make holders reachable
        // that were not before, and the chunk engine notices that on its next
        // pass — asking from the session start instead was this port's own
        // invention, and it fired exactly once per connection, which on a
        // socket that stays up for days means exactly once.

        let result = loop {
            // A stranger is held to a small frame limit so they cannot exhaust
            // memory. But pairing *is* the conversation in which a stranger
            // becomes a contact, and it happens over this very socket — so a
            // limit decided once at the handshake stays wrong for the rest of
            // the session. That is what made attachments fail: a chunk is a
            // megabyte, the stranger limit is a megabyte, and the eleven bytes
            // of framing on top killed the connection every time.
            //
            // The lookup only runs while the answer is still "no", so it costs
            // one indexed query per frame during the brief window before
            // pairing completes, and nothing at all afterwards.
            if !device_is_known {
                device_is_known = self
                    .store
                    .device_by_address(&peer_address)
                    .map(|device| device.is_some())
                    .unwrap_or(false);

                // A stranger who has just become one of our own devices, which
                // is exactly what pairing is: the joining device dials before
                // it knows anybody, and learns who it is talking to from a
                // frame on this very socket. The credit at the top of the
                // session was skipped because there was nobody to credit yet.
                //
                // Doing it on the transition rather than on every frame — Go
                // re-stamps continuously (`chat/remote_device.go:197-222`) —
                // matches what the rest of this function already does, which
                // is to record when a session began and let the online flag
                // carry the rest.
                //
                // Without this the omission is permanent, not transient:
                // peering never dials an address it is already connected to,
                // so the pairing socket is the only session these two devices
                // will ever have, and no later one comes along to fix it.
                if device_is_known {
                    if let Err(error) = self.store.mark_device_seen(&peer_address, crate::now()) {
                        tracing::debug!(%error, peer = %peer_address, "could not record a new device as seen");
                    }
                    self.emit_device_online(&peer_address).await;
                }
            }

            match wire::read_frame(&mut reader, device_is_known).await {
                Ok(frame) => {
                    if let Err(error) = self.handle_frame(&peer_address, frame).await {
                        tracing::debug!(%error, peer = %peer_address, "frame rejected");
                    }
                }
                Err(error) => break Err(error),
            }
        };

        if let Err(error) = &result {
            if matches!(
                error,
                Error::UnknownDeviceFrameTooLarge(_) | Error::KnownDeviceFrameTooLarge(_)
            ) {
                // This kills the connection, so it is never just noise.
                tracing::warn!(%error, peer = %peer_address, "dropping a peer over frame size");
            }
        }

        self.peers.write().expect("peer map").remove(&peer_address);
        writer_task.abort();
        self.emit_device_offline(&peer_address).await;

        result
    }

    async fn emit_device_online(&self, address: &str) {
        if let Ok(Some(device)) = self.store.device_by_address(address) {
            if let Ok(my_id) = self.store.my_user_id() {
                if device.user_id == my_id {
                    self.emit(Event::DeviceOnline { device_id: device.id });
                } else {
                    self.emit(Event::UserOnline {
                        user_id: device.user_id,
                    });
                }
            }
        }
    }

    async fn emit_device_offline(&self, address: &str) {
        if let Ok(Some(device)) = self.store.device_by_address(address) {
            if let Ok(my_id) = self.store.my_user_id() {
                if device.user_id == my_id {
                    self.emit(Event::DeviceOffline { device_id: device.id });
                } else {
                    self.emit(Event::UserOffline {
                        user_id: device.user_id,
                    });
                }
            }
        }
    }

    // ---------------------------------------------------------------------
    // Inbound frames
    // ---------------------------------------------------------------------

    /// Dispatch one inbound frame.
    pub async fn handle_frame(&self, peer: &str, frame: RawFrame) -> Result<()> {
        let _guard = self.handler_lock.lock().await;
        let frame_type = FrameType::from_u16(frame.frame_type)?;

        match frame_type {
            FrameType::KeepAlive => self.handle_keep_alive(peer, &frame.payload),
            FrameType::Ack => self.handle_ack(peer, &frame.payload),
            FrameType::DirectMessage => self.handle_direct_message(peer, &frame.payload).await,
            FrameType::GroupMessage => self.handle_group_message(peer, &frame.payload).await,
            FrameType::GroupCreation => self.handle_group_creation(peer, &frame.payload).await,
            FrameType::UpdateGroup => self.handle_update_group(peer, &frame.payload).await,
            FrameType::ReferenceOffer => self.handle_reference_offer(peer, &frame.payload).await,
            FrameType::ReferenceRequest => self.handle_reference_request(peer, &frame.payload).await,
            FrameType::CatchUp => self.handle_catch_up(peer, &frame.payload).await,
            FrameType::ReadReceipt => self.handle_read_receipt(peer, &frame.payload).await,
            FrameType::TypingIndicator => self.handle_typing_indicator(peer, &frame.payload).await,
            FrameType::AddUserRequest => self.handle_add_user_request(peer, &frame.payload).await,
            FrameType::AddUserRequestAccepted => {
                self.handle_add_user_request_accepted(peer, &frame.payload).await
            }
            FrameType::AddUserRequestRejected => self.handle_add_user_rejected(peer).await,
            FrameType::AddUser => self.handle_add_user(peer, &frame.payload).await,
            FrameType::Device => self.handle_device(peer, &frame.payload).await,
            FrameType::UpdateDevice => self.handle_update_device(peer, &frame.payload).await,
            FrameType::UpdateSettings => self.handle_update_settings(peer, &frame.payload).await,
            FrameType::SyncDeviceRequest => {
                self.handle_sync_device_request(peer, &frame.payload).await
            }
            FrameType::SyncDeviceRequestAccepted => {
                self.handle_sync_device_request_accepted(peer, &frame.payload).await
            }
            FrameType::SyncDeviceRequestRejected => {
                self.handle_sync_device_request_rejected(peer).await
            }
            FrameType::UpdateDm => self.handle_update_dm(peer, &frame.payload).await,
            FrameType::Confirmation => self.handle_confirmation(peer, &frame.payload).await,
            FrameType::UpdateUser => self.handle_update_user(peer, &frame.payload).await,
            FrameType::Draft => self.handle_draft(peer, &frame.payload).await,
            FrameType::File => self.handle_file(peer, &frame.payload).await,
            FrameType::ChunkOffer => self.handle_chunk_offer(peer, &frame.payload).await,
            FrameType::ChunkRequest => self.handle_chunk_request(peer, &frame.payload).await,
            FrameType::Reaction => self.handle_reaction(peer, &frame.payload).await,
            FrameType::DeleteMessage => self.handle_delete_message(peer, &frame.payload).await,
            FrameType::Chunk => self.handle_chunk(peer, &frame.payload).await,
            FrameType::ChunkUnavailable => {
                self.handle_chunk_unavailable(peer, &frame.payload).await
            }
            other => {
                tracing::debug!(frame_type = ?other, "no handler for frame type yet");
                Ok(())
            }
        }
    }

    /// Verify a signed frame and decode its body.
    ///
    /// Returns the decoded frame along with the signature material, which is
    /// retained so the frame can be relayed byte-for-byte.
    fn unpack_signed<T: serde::de::DeserializeOwned>(
        &self,
        payload: &[u8],
    ) -> Result<(T, SignedFrame)> {
        let container = SignedContainer::unpack(payload)?;
        let body: T = container.decode_payload()?;
        Ok((body, SignedFrame::from_container(&container)))
    }

    /// Check that a frame's signer really speaks for its claimed author, and
    /// was entitled to at the time it was written.
    fn signer_speaks_for(&self, signer: &str, author: Uuid, written_at: i64) -> Result<bool> {
        let Some(device) = self.store.device_by_address(signer)? else {
            // A device we have never been introduced to speaks for nobody.
            return Ok(false);
        };
        if device.user_id != author {
            return Ok(false);
        }
        // A device that had already been revoked cannot author new frames,
        // though frames it signed beforehand remain valid.
        if device.revoked_at != 0 && device.revoked_at < written_at {
            return Ok(false);
        }
        Ok(true)
    }

    async fn handle_direct_message(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let (mut message, signed) = self.unpack_signed::<DirectMessage>(payload)?;
        message.signed = signed;

        if !self.signer_speaks_for(&message.signed.signer, message.author, message.written_at)? {
            return Err(Error::InvalidFrame(
                "direct message signer does not speak for its author".into(),
            ));
        }
        if message.is_empty() || !message.text_within_limit() {
            // Acknowledge it anyway, so the peer stops offering it.
            self.send_ack(peer, message.id, FrameType::DirectMessage).await;
            return Err(Error::InvalidFrame("unacceptable direct message".into()));
        }
        if let Some(author) = self.store.user(message.author)? {
            if author.blocked {
                self.send_ack(peer, message.id, FrameType::DirectMessage).await;
                return Ok(());
            }
        }

        // Already held: acknowledge and relay nothing.
        if self.store.has_frame(message.id, FrameType::DirectMessage)? {
            self.send_ack(peer, message.id, FrameType::DirectMessage).await;
            return Ok(());
        }

        let my_id = self.store.my_user_id()?;

        // A message that expired, or that predates the thread's clear cutoff,
        // is refused rather than stored and swept: storing it would let gossip
        // undo retention on every reconnection. Acknowledged so the peer stops
        // offering it.
        if self.already_gone(message.destination(my_id), message.written_at, message.delete_at)? {
            self.send_ack(peer, message.id, FrameType::DirectMessage).await;
            return Ok(());
        }
        message.saved_at = crate::now();
        message.seen = message.author == my_id;

        self.store.save_direct_message(&message)?;
        self.send_ack(peer, message.id, FrameType::DirectMessage).await;

        let counterparty = message.destination(my_id);
        self.note_activity_with(counterparty, message.written_at);
        self.reopen_conversation(counterparty)?;

        // The author has evidently stopped composing.
        self.clear_typing_indicator(message.author, counterparty).await;

        let view = self.direct_message_view(&message, my_id, counterparty)?;
        self.emit(Event::MessageReceived {
            message: Box::new(view),
        });

        // A receipt may have arrived before the message did; now it can be
        // scoped. The same is true of a deletion and of reactions — all three
        // are independent frames, and a catch up replays them in whatever order
        // they were saved.
        self.resolve_early_read_receipts(message.id, FrameType::DirectMessage)
            .await?;
        self.apply_pending_deletion(message.id, FrameType::DirectMessage)?;
        self.resolve_parked_reactions(message.id, FrameType::DirectMessage)?;

        // Gossip onward to anyone else in scope who has not acknowledged it.
        self.broadcast(&message).await?;
        Ok(())
    }

    async fn handle_group_message(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let (mut message, signed) = self.unpack_signed::<GroupMessage>(payload)?;
        message.signed = signed;

        if !self.signer_speaks_for(&message.signed.signer, message.author, message.written_at)? {
            return Err(Error::InvalidFrame(
                "group message signer does not speak for its author".into(),
            ));
        }
        if message.is_empty() || !message.text_within_limit() {
            self.send_ack(peer, message.id, FrameType::GroupMessage).await;
            return Err(Error::InvalidFrame("unacceptable group message".into()));
        }

        // The author must actually be in the group, and allowed to post.
        let Some(group) = self.store.group(message.destination)? else {
            return Err(Error::GroupNotFound);
        };
        if !group.member_ids().contains(&message.author) {
            return Err(Error::NotPermitted("author is not a member of the group"));
        }
        if group.restrict_posting && !group.admin_ids().contains(&message.author) {
            return Err(Error::NotPermitted("posting is restricted to admins"));
        }

        if self.store.has_frame(message.id, FrameType::GroupMessage)? {
            self.send_ack(peer, message.id, FrameType::GroupMessage).await;
            return Ok(());
        }

        // As for a direct message: what retention removed must not come back
        // through the reference flow.
        if self.already_gone(message.destination, message.written_at, message.delete_at)? {
            self.send_ack(peer, message.id, FrameType::GroupMessage).await;
            return Ok(());
        }

        let my_id = self.store.my_user_id()?;
        message.saved_at = crate::now();
        message.seen = message.author == my_id;

        self.store.save_group_message(&message)?;
        self.send_ack(peer, message.id, FrameType::GroupMessage).await;
        self.note_activity_in(message.destination, message.written_at);

        self.clear_typing_indicator(message.author, message.destination).await;

        let view = self.group_message_view(&message, Some(my_id))?;
        self.emit(Event::MessageReceived {
            message: Box::new(view),
        });

        self.resolve_early_read_receipts(message.id, FrameType::GroupMessage)
            .await?;
        self.apply_pending_deletion(message.id, FrameType::GroupMessage)?;
        self.resolve_parked_reactions(message.id, FrameType::GroupMessage)?;

        self.broadcast(&message).await?;
        Ok(())
    }

    async fn handle_group_creation(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let (mut creation, signed) = self.unpack_signed::<GroupCreation>(payload)?;
        creation.signed = signed;

        // The declared ID must be the hash of the founding state, or a peer
        // could substitute different state under an ID others already trust.
        if !creation.id_matches_data() {
            return Err(Error::InvalidFrame(
                "group creation ID does not match its data".into(),
            ));
        }

        let group = creation.group()?;

        // A group is founded by exactly one user, who is its only admin.
        if group.users.len() != 1
            || group.users[0].id != group.created_by
            || group.admins != group.created_by.to_string()
        {
            return Err(Error::InvalidFrame(
                "group creation does not name its creator as the sole member and admin".into(),
            ));
        }
        if creation.timestamp != group.created_at {
            return Err(Error::InvalidFrame(
                "group creation timestamp does not match the group".into(),
            ));
        }
        // The signer must be one of the founding devices.
        if !group.users[0]
            .devices
            .iter()
            .any(|device| device.address == creation.signed.signer)
        {
            return Err(Error::InvalidFrame(
                "group creation was not signed by a founding device".into(),
            ));
        }
        // And that founder's device group must itself be valid.
        if !device_group::user_has_valid_device_group(&group.users[0]) {
            return Err(Error::InvalidFrame(
                "group creator has an invalid device group".into(),
            ));
        }

        if self.store.has_frame(creation.id, FrameType::GroupCreation)? {
            self.send_ack(peer, creation.id, FrameType::GroupCreation).await;
            return Ok(());
        }

        creation.saved_at = crate::now();
        self.store.save_group_creation(&creation)?;

        // Learn the founder, so their later frames can be attributed — and so
        // the interface has a name for them. Until this emitted an event the
        // client had no way to resolve the id mid-session: its user map is fed
        // by the boot snapshot and by user events, and nothing else, so an
        // admin who was not already a contact rendered as "Unknown" until the
        // next restart.
        for user in &group.users {
            self.adopt_group_user(user, creation.id)?;
        }
        let mut stored = group.clone();
        stored.id = creation.id;
        self.store.save_group(&stored)?;

        self.send_ack(peer, creation.id, FrameType::GroupCreation).await;
        self.recompute_and_confirm(creation.id).await?;
        self.broadcast(&creation).await?;
        self.reoffer_to_group_scope(creation.id).await?;
        // The invitation can arrive before the record of the group it is for,
        // in which case this is the first moment the policy can be applied.
        self.auto_join_if_policy_allows(creation.id).await?;
        Ok(())
    }

    async fn handle_update_group(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let (mut update, signed) = self.unpack_signed::<UpdateGroup>(payload)?;
        update.signed = signed;

        if !self.signer_speaks_for(&update.signed.signer, update.actor, update.timestamp)? {
            // An invitation may be the first thing that introduces the actor's
            // device group, so it is allowed through to the consensus stack,
            // which registers those devices before checking the signature.
            if !matches!(update.kind(), Ok(UpdateGroupType::InviteUser)) {
                return Err(Error::InvalidFrame(
                    "update group signer does not speak for its actor".into(),
                ));
            }
        }

        if self.store.has_frame(update.id, FrameType::UpdateGroup)? {
            self.send_ack(peer, update.id, FrameType::UpdateGroup).await;
            return Ok(());
        }

        update.saved_at = crate::now();
        self.store.save_update_group(&update)?;
        self.send_ack(peer, update.id, FrameType::UpdateGroup).await;

        // Whether the update is actually accepted is consensus's decision, made
        // when the group is rebuilt.
        self.recompute_and_confirm(update.target).await?;
        self.emit_group_system_message(&update);
        self.broadcast(&update).await?;
        self.reoffer_to_group_scope(update.target).await?;
        // Last, so the acceptance this may create is signed against the state
        // everything above has already settled.
        self.auto_join_if_policy_allows(update.target).await?;
        Ok(())
    }

    /// Ingest a peer's confirmation of an update group.
    ///
    /// A confirmation is not wrapped in a signed container — it *is* a
    /// signature — so attributing it to a device is the whole of the
    /// authentication, and the three fields Go keeps off the wire are derived
    /// from the update it refers to rather than believed.
    async fn handle_confirmation(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let mut confirmation: Confirmation = crate::msgpack::from_slice(payload)?;

        if self.store.has_frame(confirmation.id, FrameType::Confirmation)? {
            self.send_ack(peer, confirmation.id, FrameType::Confirmation).await;
            return Ok(());
        }

        confirmation.author = consensus::confirmation::attribute(&confirmation, |address| {
            self.store.device_owner(address).ok().flatten()
        })?;

        // A blocked user's vote is discarded, but the peer that relayed it is
        // acked anyway so it stops re-offering it.
        if matches!(self.store.user(confirmation.author), Ok(Some(ref user)) if user.blocked) {
            self.send_ack(peer, confirmation.id, FrameType::Confirmation).await;
            return Ok(());
        }

        let Some(update) = self.store.update_group(confirmation.update_group_id)? else {
            // The confirmation outran the update it refers to. Keep it so it
            // counts the moment the update lands; with no destination yet there
            // is nobody to relay it to, so the delivery record is written here
            // rather than by `broadcast`.
            confirmation.saved_at = crate::now();
            self.store.save_confirmation(&confirmation)?;
            self.store.record_delivery(&DeliveryRecord::new(
                peer.to_string(),
                confirmation.id,
                FrameType::Confirmation,
                crate::now(),
            ))?;
            self.send_ack(peer, confirmation.id, FrameType::Confirmation).await;
            return Ok(());
        };

        confirmation.destination = update.target;
        confirmation.custom_scope = update.custom_scope;

        if let Some(group) = self.store.group(update.target)? {
            if !consensus::confirmation::author_may_confirm(&group, confirmation.author) {
                // Nothing to store, but ack regardless: the delivery record is
                // only written on ack, so a silent drop is re-offered on every
                // reference cycle forever.
                self.send_ack(peer, confirmation.id, FrameType::Confirmation).await;
                return Ok(());
            }
        }

        confirmation.saved_at = crate::now();
        self.store.save_confirmation(&confirmation)?;
        self.send_ack(peer, confirmation.id, FrameType::Confirmation).await;

        // A confirmation can flip which of two conflicting updates is
        // canonical, so the group is rebuilt before the frame is relayed.
        self.recompute_and_confirm(update.target).await?;
        self.broadcast(&confirmation).await?;
        Ok(())
    }

    fn handle_ack(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let ack: Ack = crate::msgpack::from_slice(payload)?;

        for reference in &ack.references {
            let Ok(frame_type) = reference.kind() else {
                continue;
            };
            self.store.record_delivery(&DeliveryRecord::new(
                peer.to_string(),
                reference.frame_id,
                frame_type,
                crate::now(),
            ))?;

            // Tell the interface which user now has the message, so the tick
            // marks can update.
            //
            // Only for the two frame types that *are* messages. Go switches on
            // the type and calls `MessageDelivered` for exactly these
            // (`chat/ack.go:83-131`); firing it for everything meant an
            // acknowledged file, receipt or device record was announced to the
            // client as a delivered message under its own frame id, and the
            // client dutifully looked for a message with that id.
            if matches!(
                frame_type,
                FrameType::DirectMessage | FrameType::GroupMessage
            ) {
                if let Ok(Some(user_id)) = self.store.device_owner(peer) {
                    self.emit(Event::MessageDelivered {
                        message_id: reference.frame_id,
                        user_id,
                    });
                }
            }
        }
        Ok(())
    }

    async fn send_ack(&self, peer: &str, frame_id: Uuid, frame_type: FrameType) {
        if let Ok(payload) = Ack::single(frame_id, frame_type).encode() {
            self.send_to(peer, RawFrame::new(FrameType::Ack.as_u16(), payload))
                .await;
        }
    }

    // ---------------------------------------------------------------------
    // Reference flow
    // ---------------------------------------------------------------------

    /// Everything we hold that this peer is entitled to and has not
    /// acknowledged.
    fn build_reference_offer(&self, peer: &str) -> Result<ReferenceOffer> {
        let my_id = self.store.my_user_id()?;
        let Some(peer_device) = self.store.device_by_address(peer)? else {
            // We offer nothing to a device we do not know.
            return Ok(ReferenceOffer::new(Vec::new()));
        };
        let peer_user = peer_device.user_id;

        let references = self
            .store
            .references_not_delivered_to(peer, |frame_id, frame_type| {
                self.peer_may_have(peer_user, my_id, frame_id, frame_type)
                    .unwrap_or(false)
            })?;

        Ok(ReferenceOffer::new(references))
    }

    /// Whether a peer is in scope for a frame we hold.
    ///
    /// Offering a reference discloses that the frame exists, so this is
    /// evaluated before the offer is built rather than when the frame is
    /// requested.
    fn peer_may_have(
        &self,
        peer_user: Uuid,
        my_id: Uuid,
        frame_id: Uuid,
        frame_type: FrameType,
    ) -> Result<bool> {
        Ok(match frame_type {
            FrameType::DirectMessage => match self.store.direct_message(frame_id)? {
                // Either side of the conversation, or one of our own devices.
                Some(message) => {
                    peer_user == my_id || message.destination(my_id) == peer_user
                }
                None => false,
            },
            FrameType::GroupMessage => match self.store.group_message(frame_id)? {
                Some(message) => match self.store.group(message.destination)? {
                    Some(group) => group.member_ids().contains(&peer_user),
                    None => false,
                },
                None => false,
            },
            FrameType::ReadReceipt => match self.store.read_receipt(frame_id)? {
                Some(receipt) => {
                    // A receipt reaches the same audience its target message
                    // does, and never the ones we kept to ourselves.
                    if receipt.scope == Scope::Sync.as_i64() {
                        return Ok(peer_user == my_id);
                    }
                    let target_type = FrameType::from_u16(receipt.target_type)?;
                    self.peer_may_have(peer_user, my_id, receipt.target, target_type)?
                }
                None => false,
            },

            FrameType::GroupCreation | FrameType::UpdateGroup => {
                // Group metadata also reaches invitees, so they can see what
                // they have been invited to before deciding.
                let group_id = match frame_type {
                    FrameType::GroupCreation => frame_id,
                    _ => match self.store.update_group(frame_id)? {
                        Some(update) => update.target,
                        None => return Ok(false),
                    },
                };
                match self.store.group(group_id)? {
                    Some(group) => {
                        group.member_ids().contains(&peer_user)
                            || group.invite_ids().contains(&peer_user)
                    }
                    None => false,
                }
            }

            FrameType::UpdateUser => match self.store.update_user(frame_id)? {
                // A profile change reaches the same people the profile does:
                // our own devices, the user it is about, and anyone who shares
                // a group with them. Key rolling scopes to Sync and stays home.
                Some(update) => {
                    if update.scope(my_id) == Scope::Sync {
                        peer_user == my_id
                    } else {
                        peer_user == my_id
                            || peer_user == update.target
                            || update.target == my_id
                            || self
                                .store
                                .users_sharing_a_group_with(update.target)?
                                .contains(&peer_user)
                    }
                }
                None => false,
            },

            // An introduction reaches the same two parties it is between: our
            // own devices, so a device that joins later inherits the contact,
            // and the counterparty, who is the other signatory.
            // (`chat/reference_offer.go:541`.)
            FrameType::AddUser => match self.store.add_user_record(frame_id)? {
                Some(record) => peer_user == my_id || crate::xor(record.xor, my_id) == peer_user,
                None => false,
            },

            // Device group membership (`chat/reference_offer.go:481`).
            //
            // Our own devices may learn about every device we know of — a
            // sibling has to be able to attribute anything any contact signs.
            // Anyone else gets ours, their own, and those of people they share
            // a group with, and never the record of the device being offered
            // to, which already has it.
            FrameType::Device => match self.store.device_by_id(frame_id)? {
                Some(device) => {
                    if peer_user == my_id {
                        true
                    } else {
                        device.user_id == my_id
                            || device.user_id == peer_user
                            || self
                                .store
                                .users_sharing_a_group_with(peer_user)?
                                .contains(&device.user_id)
                    }
                }
                None => false,
            },

            // Changes to a device group (`chat/reference_offer.go:862`).
            //
            // Our own devices get every change; everybody else gets only
            // revocations, and only from the same overlap a device record
            // itself travels through. A rename is nobody else's business, but a
            // revocation is everybody's — until a contact learns of it they go
            // on accepting frames signed by the revoked device as genuinely
            // ours, which is the whole point of revoking it.
            FrameType::UpdateDevice => match self.store.update_device(frame_id)? {
                Some(update) => {
                    if peer_user == my_id {
                        true
                    } else if update.update_type != UpdateDeviceType::Revoke.as_u16() {
                        false
                    } else {
                        update.author == my_id
                            || update.author == peer_user
                            || self
                                .store
                                .users_sharing_a_group_with(peer_user)?
                                .contains(&update.author)
                    }
                }
                None => false,
            },

            // Profile-wide preferences are one person's, and reach only their
            // own devices (`chat/reference_offer.go:991`).
            FrameType::UpdateSettings => {
                peer_user == my_id && self.store.update_settings_frame(frame_id)?.is_some()
            }

            // An offer says where the bytes of a chunk are, so it travels with
            // the same audience as the file it belongs to
            // (`chat/reference_offer.go:1125`, which sorts offers into the
            // same scope buckets). Judged on the offer's own scope rather than
            // the file's: the file record may not have reached us yet, and
            // offers routinely arrive first.
            FrameType::ChunkOffer => match self.store.chunk_offer(frame_id)? {
                Some(offer) => match Scope::from_i64(offer.scope) {
                    Ok(Scope::Sync) => peer_user == my_id,
                    Ok(Scope::User) => {
                        peer_user == my_id || crate::xor(offer.destination, my_id) == peer_user
                    }
                    Ok(Scope::Group) | Ok(Scope::GroupWithInvites) => {
                        match self.store.group(offer.destination)? {
                            Some(group) => group.member_ids().contains(&peer_user),
                            None => false,
                        }
                    }
                    Ok(Scope::Global) => {
                        offer.author == my_id
                            || peer_user == my_id
                            || peer_user == offer.author
                            || self
                                .store
                                .users_sharing_a_group_with(offer.author)?
                                .contains(&peer_user)
                    }
                    _ => false,
                },
                None => false,
            }

            // A draft is nobody's business but this profile's own devices.
            FrameType::Draft => peer_user == my_id && self.store.draft(frame_id)?.is_some(),

            FrameType::Confirmation => match self.store.confirmation(frame_id)? {
                // A confirmation reaches exactly the audience the update it
                // refers to reaches.
                Some(record) => self.peer_may_have(
                    peer_user,
                    my_id,
                    record.update_group_id,
                    FrameType::UpdateGroup,
                )?,
                None => false,
            },

            // Both follow their target's audience, the way a read receipt does:
            // a reaction to a group message reaches the group, a deletion of a
            // direct message reaches the counterparty, and a frame we parked
            // because its target never arrived reaches nobody, because there is
            // nothing to judge it against.
            FrameType::Reaction => match self.store.reaction(frame_id)? {
                Some(reaction) => {
                    if reaction.scope == Scope::Sync.as_i64() {
                        return Ok(peer_user == my_id);
                    }
                    let target_type = FrameType::from_u16(reaction.target_type)?;
                    self.peer_may_have(peer_user, my_id, reaction.target, target_type)?
                }
                None => false,
            },

            FrameType::DeleteMessage => match self.store.delete_message_frame(frame_id)? {
                Some(delete) => {
                    if delete.scope == Scope::Sync.as_i64() {
                        return Ok(peer_user == my_id);
                    }
                    let target_type = FrameType::from_u16(delete.target_type)?;
                    // Deliberately judged against the *tombstone*, which is
                    // still there. Judging against a removed row would answer
                    // "nobody", and the deletion would never be offered on.
                    self.peer_may_have(peer_user, my_id, delete.target, target_type)?
                }
                None => false,
            },

            FrameType::File => match self.store.file(frame_id)? {
                // A file reaches whoever the message it hangs off reaches, so
                // the scope it declares is what decides — not the attachment,
                // which may not have arrived on this device yet.
                Some(record) => match Scope::from_i64(record.scope) {
                    Ok(Scope::Sync) => peer_user == my_id,
                    Ok(Scope::User) => {
                        peer_user == my_id || crate::xor(record.destination, my_id) == peer_user
                    }
                    Ok(Scope::Group) | Ok(Scope::GroupWithInvites) => {
                        match self.store.group(record.destination)? {
                            Some(group) => group.member_ids().contains(&peer_user),
                            None => false,
                        }
                    }
                    // Avatars, profile and group alike (`engine/files.rs`
                    // stages both as global). This has to mirror
                    // `scope::global_addresses`, or the two routes to the same
                    // file disagree: ours may go to anyone we know, and
                    // somebody else's is relayed only inside the overlap,
                    // because a shared group is the proof that its author
                    // already exposes their profile to that person.
                    //
                    // Falling through to `_ => false` meant an avatar was
                    // offered to nobody at all once the moment of broadcast had
                    // passed, while the `UpdateUser` naming it replayed
                    // perfectly — so a contact who was away, or a device that
                    // joined afterwards, held an image id with no image.
                    Ok(Scope::Global) => {
                        record.author == my_id
                            || peer_user == my_id
                            || peer_user == record.author
                            || self
                                .store
                                .users_sharing_a_group_with(record.author)?
                                .contains(&peer_user)
                    }
                    _ => false,
                },
                None => false,
            },
            _ => false,
        })
    }

    async fn handle_reference_offer(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let offer: ReferenceOffer = crate::msgpack::from_slice(payload)?;

        // The offer itself is acknowledged, which is how the sender learns it
        // arrived at all (`chat/reference_offer.go:1403`). It is not a stored
        // frame; the acknowledgement exists solely so an offer written to a
        // socket that had already died can be sent again.
        self.send_ack(peer, offer.id, FrameType::ReferenceOffer).await;

        let mut wanted = Vec::new();
        let mut already_held = Vec::new();

        for reference in offer.references {
            let Ok(frame_type) = reference.kind() else {
                continue;
            };
            if self.store.has_frame(reference.frame_id, frame_type)? {
                already_held.push(reference);
            } else {
                wanted.push(reference);
            }
        }

        // Acknowledging what we already have stops the peer offering it again.
        if !already_held.is_empty() {
            let ack = Ack {
                references: already_held,
            };
            self.send_to(peer, RawFrame::new(FrameType::Ack.as_u16(), ack.encode()?))
                .await;
        }

        if !wanted.is_empty() {
            let request = ReferenceRequest { references: wanted };
            self.send_to(
                peer,
                RawFrame::new(FrameType::ReferenceRequest.as_u16(), request.encode()?),
            )
            .await;
        }

        Ok(())
    }

    async fn handle_reference_request(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let request: ReferenceRequest = crate::msgpack::from_slice(payload)?;

        let my_id = self.store.my_user_id()?;
        let peer_user = match self.store.device_by_address(peer)? {
            Some(device) => device.user_id,
            None => return Ok(()),
        };

        let mut frames: Vec<(CatchUpFrame, i64)> = Vec::new();
        let peer_device = self.store.device_by_address(peer)?;

        for reference in request.references {
            let Ok(frame_type) = reference.kind() else {
                continue;
            };
            // Re-check entitlement: a request is not authorization.
            if !self.peer_may_have(peer_user, my_id, reference.frame_id, frame_type)? {
                continue;
            }
            // And re-check capability, which the offer side already applies.
            // A peer should never be asking for a frame type it never had
            // offered — but the cost of humouring one that does is not a
            // dropped frame. Go refuses the entire catch-up bundle if it
            // contains a type it does not allow in one, and as of upstream
            // `dafec89` that path takes `catchUpMutex` a second time while
            // already holding it (`chat/catch_up.go:124` and `:137`), which
            // deadlocks the client and takes its whole reference flow with it.
            // One frame we should not have sent would be enough.
            if !crate::store::accepts(peer_device.as_ref(), frame_type) {
                continue;
            }
            let Some(payload) = self.store.frame_payload(reference.frame_id, frame_type)? else {
                continue;
            };
            let saved_at = self
                .store
                .frame_saved_at(reference.frame_id, frame_type)?
                .unwrap_or(0);

            frames.push((
                CatchUpFrame {
                    id: reference.frame_id,
                    frame_type: frame_type.as_u16(),
                    payload,
                },
                saved_at,
            ));
        }

        if frames.is_empty() {
            return Ok(());
        }

        // Ordered so the peer never sees a message before the context it needs.
        CatchUp::sort_for_replay(&mut frames);

        let catch_up = CatchUp {
            frames: frames.into_iter().map(|(frame, _)| frame).collect(),
        };
        self.send_to(
            peer,
            RawFrame::new(FrameType::CatchUp.as_u16(), catch_up.encode()?),
        )
        .await;

        Ok(())
    }

    async fn handle_catch_up(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let catch_up: CatchUp = crate::msgpack::from_slice(payload)?;
        let total = catch_up.frames.len();

        if total == 0 {
            return Ok(());
        }

        self.emit(Event::SyncStarted);

        for (index, frame) in catch_up.frames.into_iter().enumerate() {
            // Each frame goes through the same handler it would have on the
            // wire, so nothing skips validation just because it arrived late.
            let raw = RawFrame::new(frame.frame_type, frame.payload);
            if let Err(error) = Box::pin(self.handle_frame_unlocked(peer, raw)).await {
                tracing::debug!(%error, "frame in catch up rejected");
            }
            self.emit(Event::SyncProgress {
                fraction: (index + 1) as f64 / total as f64,
            });
        }

        self.emit(Event::SyncComplete);

        // Offer again. Go does this from three places after a catch-up
        // (`chat/catch_up.go:337`, `:350`, `:568`), and the reason it matters
        // most is the case its own comment describes: a device we did not
        // recognise when the socket opened was offered nothing, and handling
        // its catch-up is often exactly what teaches us who it is. Without a
        // second offer the two sides sit connected with everything still to
        // say. It is also what makes a large initial sync converge in waves
        // rather than in one shot that either works or does not.
        if self.store.device_by_address(peer)?.is_some() {
            self.offer_references_to_soon(peer);
        }

        // A catch-up is the likeliest moment to have learned about devices
        // worth dialling — contacts arrive in it, and each brings its own.
        self.audit_now.notify_one();

        Ok(())
    }

    /// Dispatch without taking the handler lock, for use from inside a handler
    /// that already holds it.
    ///
    /// This must cover every frame type the reference flow can replay, and it
    /// silently dropped two of them. A `File` arriving through catch-up — which
    /// is what an attachment sent while this device was offline looks like —
    /// went nowhere, so the message appeared with an attachment whose metadata
    /// never existed and whose progress could never leave zero. The loss was
    /// permanent: the sender's acknowledgement had already retired the frame.
    ///
    /// Chunk requests and the chunks themselves are deliberately absent: they
    /// are not stored and never enter the reference flow, so there is nothing
    /// for catch-up to replay. Chunk *offers* are a different matter — they
    /// are stored precisely so they can be replayed, since they are the only
    /// record of who holds a chunk.
    async fn handle_frame_unlocked(&self, peer: &str, frame: RawFrame) -> Result<()> {
        let frame_type = FrameType::from_u16(frame.frame_type)?;
        match frame_type {
            // Sorted first by `catch_up_order`, so the contact exists before
            // the frames that only mean anything once it does. A device that
            // joins later has no other way to learn who the profile knows, and
            // a frame signed by a device belonging to nobody is refused.
            FrameType::AddUser => self.handle_add_user(peer, &frame.payload).await,
            FrameType::Device => self.handle_device(peer, &frame.payload).await,
            FrameType::DirectMessage => self.handle_direct_message(peer, &frame.payload).await,
            FrameType::GroupMessage => self.handle_group_message(peer, &frame.payload).await,
            FrameType::GroupCreation => self.handle_group_creation(peer, &frame.payload).await,
            FrameType::UpdateGroup => self.handle_update_group(peer, &frame.payload).await,
            FrameType::ReadReceipt => self.handle_read_receipt(peer, &frame.payload).await,
            FrameType::File => self.handle_file(peer, &frame.payload).await,
            FrameType::ChunkOffer => self.handle_chunk_offer(peer, &frame.payload).await,
            FrameType::UpdateDm => self.handle_update_dm(peer, &frame.payload).await,
            FrameType::UpdateUser => self.handle_update_user(peer, &frame.payload).await,
            FrameType::Draft => self.handle_draft(peer, &frame.payload).await,
            FrameType::Confirmation => self.handle_confirmation(peer, &frame.payload).await,
            FrameType::UpdateDevice => self.handle_update_device(peer, &frame.payload).await,
            FrameType::UpdateSettings => self.handle_update_settings(peer, &frame.payload).await,
            // Both arms matter more here than in the live dispatcher: a
            // deletion replayed on catch-up is the only way it reaches a peer
            // who was offline when the message was withdrawn, which is exactly
            // the peer the person deleting it cannot check on.
            FrameType::Reaction => self.handle_reaction(peer, &frame.payload).await,
            FrameType::DeleteMessage => self.handle_delete_message(peer, &frame.payload).await,
            other => {
                // Loud, because this is what the omission above looked like:
                // a frame accepted into catch-up, counted towards progress,
                // and thrown away.
                tracing::warn!(frame_type = ?other, "no catch-up handler for a replayed frame");
                Ok(())
            }
        }
    }

    // ---------------------------------------------------------------------
    // Views
    // ---------------------------------------------------------------------

    /// One contact, as the client sees them.
    ///
    /// `online` is read from the live peer map rather than defaulted, because
    /// this view is both the boot snapshot and the payload of every
    /// `UserUpdated` — and the client replaces its whole record from either.
    /// Reporting a constant `false` therefore did not merely start pessimistic:
    /// a contact who was connected went grey the moment they changed their
    /// name or their picture, and stayed grey until the session ended.
    fn user_view(&self, user: &User, is_profile: bool) -> UserView {
        // Not for ourselves. Our own devices being reachable is not the same
        // question, and the note-to-self row does not ask it.
        let online = !is_profile && self.user_is_online(user.id);

        UserView {
            id: user.id,
            name: user.name.clone(),
            alias: user.alias.clone(),
            images: user.image_ids(),
            blocked: user.blocked,
            accepted: user.accepted,
            introduction_time: user.introduction_time,
            last_activity: user.last_activity,
            muted_until: user.muted_until,
            // Every one of these is already in the `users` row; the view simply
            // stopped carrying them, which left each control on the client
            // seeded from its default rather than from the stored value.
            retention: user.retention,
            clear_before: user.clear_before,
            open_dm: user.open_dm,
            notes: user.notes.clone(),
            read_receipts_overridden: user.read_receipts_overridden,
            read_receipts_enabled: user.read_receipts_enabled,
            typing_indicators_overridden: user.typing_indicators_overridden,
            typing_indicators_enabled: user.typing_indicators_enabled,
            last_opened: user.last_opened,
            online,
        }
    }

    /// One of this profile's own devices.
    ///
    /// Like `user_view`, `online` is derived rather than passed in. Every
    /// caller used to supply it, and most of them guessed: renaming a device
    /// from another one reported it offline (`engine/settings.rs`), because
    /// the only thing the rename path knew was whether the device was this
    /// one. This device is always reachable from itself.
    fn device_view(&self, device: &Device, local: bool) -> DeviceView {
        let online =
            !device.is_revoked() && (local || self.is_connected(&device.address));

        DeviceView {
            id: device.id,
            name: device.name.clone(),
            address: device.address.clone(),
            created_at: device.timestamp,
            last_seen: device.last_seen,
            local,
            online,
            revoked: device.is_revoked(),
        }
    }

    fn group_view(&self, group: &Group) -> GroupView {
        GroupView {
            id: group.id,
            name: group.name.clone(),
            images: group.image_ids(),
            members: group.member_ids(),
            admins: group.admin_ids(),
            invites: group.invite_ids(),
            created_by: group.created_by,
            created_at: group.created_at,
            last_activity: group.last_activity,
            muted_until: group.muted_until,
            retention: group.retention,
            last_opened: group.last_opened,
            restrict_posting: group.restrict_posting,
            restrict_group_edits: group.restrict_group_edits,
            restrict_user_management: group.restrict_user_management,
        }
    }

    /// Everything known about what happened to one message.
    ///
    /// Looked up on demand rather than carried on every `MessageView`: this is
    /// four extra queries, and a thread of ten thousand messages would run them
    /// ten thousand times to populate a panel showing one.
    pub fn message_info(&self, message_id: Uuid) -> Result<Option<MessageInfo>> {
        let (written_at, expires_at, audience) =
            if let Some(message) = self.store.direct_message(message_id)? {
                (message.written_at, message.delete_at, Vec::new())
            } else if let Some(message) = self.store.group_message(message_id)? {
                let audience = self
                    .store
                    .group(message.destination)?
                    .map(|group| group.member_ids())
                    .unwrap_or_default();
                (message.written_at, message.delete_at, audience)
            } else {
                return Ok(None);
            };

        let read_by = self
            .store
            .read_times_for(message_id)?
            .into_iter()
            .map(|(user_id, at)| Receipt { user_id, at })
            .collect();

        // Delivery is per device; the panel is about people. Earliest wins, so
        // a second device receiving it later does not push the time out.
        let mut delivered: Vec<Receipt> = Vec::new();
        for (address, at) in self.store.delivery_times_for(message_id)? {
            let Some(user_id) = self.store.device_owner(&address)? else {
                continue;
            };
            match delivered.iter_mut().find(|entry| entry.user_id == user_id) {
                Some(entry) => entry.at = entry.at.min(at),
                None => delivered.push(Receipt { user_id, at }),
            }
        }
        delivered.sort_by_key(|entry| entry.at);

        Ok(Some(MessageInfo {
            message_id,
            written_at,
            expires_at,
            read_by,
            delivered_to: delivered,
            audience,
        }))
    }

    fn direct_message_view(
        &self,
        message: &DirectMessage,
        my_id: Uuid,
        counterparty: Uuid,
    ) -> Result<MessageView> {
        Ok(MessageView {
            id: message.id,
            // Notes to self are threaded under our own ID.
            thread: if message.xor.is_nil() {
                my_id
            } else {
                counterparty
            },
            author: message.author,
            text: message.text.clone(),
            written_at: message.written_at,
            expires_at: message.delete_at,
            seen: message.seen,
            undeliverable: message.undeliverable,
            delivered_to: self.recipients_reached(message.id, message.author),
            read_by: self.store.readers_of(message.id).unwrap_or_default(),
            attachments: self
                .attachment_views(&message.image_attachments, &message.file_attachments),
            outgoing: message.author == my_id,
            reactions: self.reaction_views(message.id, my_id),
            quote: quote_view(message.quote.as_ref()),
            deleted_at: message.deleted_at,
            deleted_by: (!message.deleted_by.is_nil()).then_some(message.deleted_by),
            deleted_by_admin: message.deleted_at != 0
                && message.deleted_by != message.author
                && !message.deleted_by.is_nil(),
        })
    }

    /// Who, other than the author, has a device holding this frame.
    ///
    /// The author is dropped because a sync-scoped copy landing on the sender's
    /// own second device is not delivery — it is the same person twice, and
    /// counting it would tick a message off before it had left the profile.
    ///
    /// A view built before this existed reported nobody, which meant an
    /// outgoing message that had been delivered but not read fell back to the
    /// pending tick on every restart, and stayed there: `messageDelivered`
    /// fires once, when the acknowledgement arrives, and never again.
    fn recipients_reached(&self, frame_id: Uuid, author: Uuid) -> Vec<Uuid> {
        let mut reached = self.store.deliverers_of(frame_id).unwrap_or_default();
        reached.retain(|user| *user != author);
        reached
    }

    fn group_message_view(&self, message: &GroupMessage, my_id: Option<Uuid>) -> Result<MessageView> {
        Ok(MessageView {
            id: message.id,
            thread: message.destination,
            author: message.author,
            text: message.text.clone(),
            written_at: message.written_at,
            expires_at: message.delete_at,
            seen: message.seen,
            undeliverable: message.undeliverable,
            delivered_to: self.recipients_reached(message.id, message.author),
            read_by: self.store.readers_of(message.id).unwrap_or_default(),
            attachments: self
                .attachment_views(&message.image_attachments, &message.file_attachments),
            outgoing: Some(message.author) == my_id,
            reactions: self.reaction_views(message.id, my_id.unwrap_or_else(Uuid::nil)),
            quote: quote_view(message.quote.as_ref()),
            deleted_at: message.deleted_at,
            deleted_by: (!message.deleted_by.is_nil()).then_some(message.deleted_by),
            deleted_by_admin: message.deleted_at != 0
                && message.deleted_by != message.author
                && !message.deleted_by.is_nil(),
        })
    }
}

/// A user record as it goes to somebody else.
///
/// Private key material and this device's own view of the person — alias,
/// notes, mute state — are cleared. `serde(skip)` already keeps them off the
/// wire; clearing them here means they cannot leak into a record we hand to
/// another device inside an add-user blob either.
fn shareable(user: &User) -> User {
    let mut shared = user.clone();
    shared.profile = false;
    shared.private_ecdh_key = Vec::new();
    shared.private_ecdsa_key = Vec::new();
    shared.public_ecdsa_key = Vec::new();
    shared.alias = String::new();
    shared.notes = String::new();
    shared
}

/// Whether a profile update carries a payload its type can use.
///
/// Checked before the frame is stored, because a stored update is replayed on
/// every recomputation and an unusable one would be re-examined forever.
fn update_user_payload_is_valid(update: &UpdateUser) -> bool {
    use crate::frames::update::UpdateUserType::*;

    match update.kind() {
        Ok(UpdateName) => std::str::from_utf8(&update.data)
            .map(crate::frames::identity::valid_user_name)
            .unwrap_or(false),
        Ok(UpdateImage) => Uuid::from_slice(&update.data).is_ok(),
        Ok(AddEncryptedDevice) | Ok(RemoveEncryptedDevice) => !update.data.is_empty(),
        // Key material and encrypted device names have no length this layer can
        // check; Go says the same (`chat/update_user.go:130`).
        Ok(_) => true,
        Err(_) => false,
    }
}

/// Which of a group's three restrictions is being changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupPermission {
    /// Only admins may post.
    Posting,
    /// Only admins may change the name, image, or retention.
    Edits,
    /// Only admins may invite, remove, promote or demote.
    UserManagement,
}

/// Where a read receipt goes and who wrote the message it refers to.
struct ReadReceiptContext {
    destination: Uuid,
    author: Uuid,
    scope: Scope,
}

/// Apply a per-conversation override to a profile-wide default.
///
/// An override only counts when it has actually been set; otherwise the
/// profile default wins.
fn resolve_override(overridden: bool, value: bool, default: bool) -> bool {
    if overridden {
        value
    } else {
        default
    }
}

/// Adapts the [`Store`] to the read-only view scope resolution needs.
struct StoreScopeContext<'a> {
    store: &'a Store,
    my_user_id: Uuid,
    my_address: String,
}

impl ScopeContext for StoreScopeContext<'_> {
    fn my_user_id(&self) -> Uuid {
        self.my_user_id
    }

    fn my_address(&self) -> &str {
        &self.my_address
    }

    fn devices_for_user(&self, user_id: Uuid) -> Vec<String> {
        self.store
            .devices_for_user(user_id)
            .map(|devices| {
                devices
                    .into_iter()
                    .filter(|device| !device.is_revoked())
                    .map(|device| device.address)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn members_of_group(&self, group_id: Uuid) -> Vec<Uuid> {
        self.store.group_member_ids(group_id).unwrap_or_default()
    }

    fn invitees_of_group(&self, group_id: Uuid) -> Vec<Uuid> {
        self.store
            .group(group_id)
            .ok()
            .flatten()
            .map(|group| group.invite_ids())
            .unwrap_or_default()
    }

    fn all_device_addresses(&self) -> Vec<String> {
        self.store.all_device_addresses().unwrap_or_default()
    }

    fn users_sharing_a_group_with(&self, user_id: Uuid) -> Vec<Uuid> {
        self.store
            .users_sharing_a_group_with(user_id)
            .unwrap_or_default()
    }

    fn custom_scope_addresses(&self, scope_id: Uuid) -> Vec<String> {
        self.store
            .custom_scope(scope_id)
            .ok()
            .flatten()
            .map(|scope| scope.address_list())
            .unwrap_or_default()
    }

    fn is_encrypted_device(&self, _address: &str) -> bool {
        // Encrypted devices are tracked on the owning user record; until that
        // path is wired up, no address is treated as encrypted.
        false
    }

    fn is_revoked(&self, address: &str) -> bool {
        self.store
            .device_by_address(address)
            .ok()
            .flatten()
            .map(|device| device.is_revoked())
            .unwrap_or(false)
    }
}

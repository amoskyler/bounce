//! N-API bindings exposing the Bounce engine to the Electron main process.
//!
//! Signal Desktop reaches its Rust core (`libsignal`) through a Neon native
//! module; this is the same arrangement with napi-rs. The engine runs on a
//! Tokio runtime inside this module, entirely in the main process — the
//! renderer never touches it, and never holds a key.
//!
//! ```text
//!   renderer (React)          main process               this module
//!        │  contextBridge          │                          │
//!        ├── invoke ──────────────▶│── method call ──────────▶│  Tokio + engine
//!        │                         │                          │
//!        │◀───── webContents ──────│◀──── threadsafe fn ──────│  events
//! ```
//!
//! Commands cross as method calls returning promises. Events cross the other
//! way through a threadsafe function, so the engine can push without the
//! renderer polling.
//!
//! Everything crossing the boundary is JSON, matching the `Event` enum's serde
//! representation. That keeps this layer thin: it owns lifetimes and threading,
//! not protocol logic.

#![deny(clippy::all)]

use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ErrorStrategy, ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;
use tokio::sync::Mutex;

use libbounce::crypto::DeviceKey;
use libbounce::engine::files::OutgoingAttachment;
use libbounce::engine::{Engine, Event, GroupPermission};
use libbounce::net::{HandshakeMode, StaticDirectory, TcpNetwork, TorNetwork, Transport};
use libbounce::store::Store;
use libbounce::types::FrameType;

type BounceEngine = Engine<Transport>;

/// Where events go once a subscriber attaches.
type EventSink = ThreadsafeFunction<String, ErrorStrategy::Fatal>;

/// A file the renderer is asking to send.
///
/// Dimensions come from the client, which has already decoded the image to
/// show a preview; the engine does not decode images. They are ignored unless
/// `isImage` is set.
#[napi(object)]
pub struct Attachment {
    pub name: String,
    pub data: Buffer,
    pub is_image: bool,
    pub width: i64,
    pub height: i64,
    /// A BlurHash placeholder, or empty if the client computed none.
    ///
    /// Not `Option<String>`: napi-rs maps that to `string | undefined`, and a
    /// client passing an explicit `null` — which is what "no blur hash" looks
    /// like in JavaScript — fails conversion rather than defaulting.
    pub blur_hash: String,
    /// Where the file is on disk, for one too large to read into memory.
    ///
    /// Set this and leave `data` empty and the engine streams it: hashed a
    /// chunk at a time and served by seeking, never held whole. The renderer
    /// gets a path from the file picker without reading anything, which is why
    /// a multi-gigabyte file can be attached at all.
    ///
    /// Optional, unlike the fields around it. A required field is one every
    /// caller must set from the day it is added, and a caller that does not
    /// fails object conversion — which is to say every attachment stops
    /// sending, not just the large ones.
    pub path: Option<String>,
}

fn convert(attachments: Vec<Attachment>) -> Vec<OutgoingAttachment> {
    attachments
        .into_iter()
        .map(|attachment| OutgoingAttachment {
            name: attachment.name,
            data: attachment.data.to_vec(),
            is_image: attachment.is_image,
            width: attachment.width,
            height: attachment.height,
            blur_hash: attachment.blur_hash,
            path: attachment.path.unwrap_or_default(),
        })
        .collect()
}

/// A running Bounce instance.
#[napi]
pub struct BounceNode {
    engine: Arc<BounceEngine>,
    runtime: Arc<tokio::runtime::Runtime>,
    address: String,
    transport: String,
    anonymous: bool,
    /// Events emitted before a subscriber attached, replayed on subscribe so
    /// nothing from start-up is lost.
    backlog: Arc<Mutex<Vec<String>>>,
    sink: Arc<Mutex<Option<EventSink>>>,
}

#[napi]
impl BounceNode {
    /// Open an instance, creating the data directory if needed.
    ///
    /// `use_tor` selects the transport. With it off there is **no metadata
    /// protection at all** — that mode exists for local development, and
    /// `anonymous` reports which one is in force so the interface can say so.
    ///
    /// The device key is persisted as `device_key` and is the device's
    /// identity: deleting it and starting again produces a different device
    /// that other members of the device group will not recognise.
    /// `go_compatible` makes outbound handshakes match the Go implementation.
    ///
    /// Required to dial a Go peer, and **it lets every address you dial obtain
    /// a signature from this device** — see `libbounce::net`. Inbound
    /// connections from Go peers work either way.
    #[napi(factory)]
    pub fn open(data_directory: String, use_tor: bool, go_compatible: bool) -> Result<Self> {
        let directory = std::path::PathBuf::from(&data_directory);
        std::fs::create_dir_all(&directory).map_err(to_napi_error)?;
        install_logging(&directory);

        let handshake = if go_compatible {
            HandshakeMode::Compatible
        } else {
            HandshakeMode::Strict
        };

        let key = load_or_create_key(&directory)?;
        let address = key.address();

        let store = Arc::new(Store::open(directory.join("bounce.db")).map_err(to_napi_error)?);

        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(to_napi_error)?,
        );

        let network = if use_tor {
            // Bootstrapping Tor and publishing the service takes tens of
            // seconds on a cold start.
            let tor = runtime
                .block_on(TorNetwork::start(
                    key.clone(),
                    &directory.join("tor-state"),
                    &directory.join("tor-cache"),
                ))
                .map_err(to_napi_error)?;
            Transport::Tor(tor.with_handshake_mode(handshake))
        } else {
            // Two clients on one machine are separate processes with separate
            // data directories, so the rendezvous file has to live somewhere
            // neither of them owns. The temp directory is common to both.
            let shared = std::env::var_os("BOUNCE_DEV_PEERS")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("bounce-dev-peers.json"));

            let peers = Arc::new(StaticDirectory::shared_at(shared));
            let tcp = runtime
                .block_on(TcpNetwork::bind(key.clone(), peers))
                .map_err(to_napi_error)?;
            Transport::Tcp(tcp.with_handshake_mode(handshake))
        };

        let transport = network.name().to_string();
        let anonymous = network.is_anonymous();
        let network = Arc::new(network);

        let (engine, mut events) = Engine::new(key, store, network);

        let backlog = Arc::new(Mutex::new(Vec::new()));
        let sink: Arc<Mutex<Option<EventSink>>> = Arc::new(Mutex::new(None));

        // One pump for the engine's whole life. Until a subscriber attaches,
        // events accumulate; after that they go straight across.
        {
            let backlog = Arc::clone(&backlog);
            let sink = Arc::clone(&sink);
            runtime.spawn(async move {
                while let Some(event) = events.recv().await {
                    let Ok(payload) = serde_json::to_string(&event) else {
                        continue;
                    };
                    let guard = sink.lock().await;
                    match guard.as_ref() {
                        Some(callback) => {
                            callback.call(payload, ThreadsafeFunctionCallMode::NonBlocking);
                        }
                        None => backlog.lock().await.push(payload),
                    }
                }
            });
        }

        runtime.spawn(Arc::clone(&engine).run_listener());
        runtime.spawn(Arc::clone(&engine).run_typing_expiry());
        // Without this the client is accept-only: it publishes an onion
        // service and waits, and two clients that both wait never speak
        // again after a restart.
        runtime.spawn(Arc::clone(&engine).run_peering());
        // Retention is a promise about plaintext, not a label. The engine
        // prunes once on start-up, but a client left open for a week would
        // otherwise keep everything that expired during it, so the sweep has
        // to run for the life of the process.
        runtime.spawn(Arc::clone(&engine).run_retention());

        Ok(BounceNode {
            engine,
            runtime,
            address,
            transport,
            anonymous,
            backlog,
            sink,
        })
    }

    /// This device's onion address, which is its identity on the network.
    #[napi(getter)]
    pub fn address(&self) -> String {
        self.address.clone()
    }

    /// Which transport is in force: `"tor"` or `"tcp"`.
    #[napi(getter)]
    pub fn transport(&self) -> String {
        self.transport.clone()
    }

    /// Whether the active transport protects metadata.
    #[napi(getter)]
    pub fn anonymous(&self) -> bool {
        self.anonymous
    }

    /// Attach a callback that receives every engine event as a JSON string.
    ///
    /// Anything emitted before this point is replayed immediately.
    #[napi(ts_args_type = "callback: (event: string) => void")]
    pub fn subscribe(&self, callback: JsFunction) -> Result<()> {
        let threadsafe: EventSink =
            callback.create_threadsafe_function(0, |context| Ok(vec![context.value]))?;

        let sink = Arc::clone(&self.sink);
        let backlog = Arc::clone(&self.backlog);

        self.runtime.block_on(async move {
            let buffered = std::mem::take(&mut *backlog.lock().await);
            for payload in buffered {
                threadsafe.call(payload, ThreadsafeFunctionCallMode::NonBlocking);
            }
            *sink.lock().await = Some(threadsafe);
        });

        Ok(())
    }

    /// Whether a profile has been created on this device.
    #[napi]
    pub fn has_profile(&self) -> Result<bool> {
        Ok(self
            .engine
            .store()
            .profile()
            .map_err(to_napi_error)?
            .is_some())
    }

    /// Create the profile that owns this device.
    #[napi]
    pub fn create_profile(&self, name: String, device_name: String) -> Result<String> {
        let user = self
            .engine
            .create_profile(&name, &device_name)
            .map_err(to_napi_error)?;
        Ok(user.id.to_string())
    }

    /// The full state a freshly attached renderer needs, as JSON.
    #[napi]
    pub fn initial_state(&self) -> Result<String> {
        let state = self.engine.initial_state().map_err(to_napi_error)?;
        serde_json::to_string(&state).map_err(to_napi_error)
    }

    // -- contact introduction ------------------------------------------------

    /// Produce a pairing code for another person to scan.
    ///
    /// Bounce has no directory, so this is the whole of contact discovery.
    /// Showing a new code invalidates the previous one, and a code is spent the
    /// first time it is used.
    #[napi]
    pub fn create_pairing_code(&self) -> Result<String> {
        self.engine.create_pairing_code().map_err(to_napi_error)
    }

    /// Act on a scanned pairing code.
    #[napi]
    pub async fn request_to_add_user(&self, code: String) -> Result<()> {
        Arc::clone(&self.engine)
            .request_to_add_user(&code)
            .await
            .map_err(to_napi_error)
    }

    // -- messaging -----------------------------------------------------------

    /// Send a direct message, returning the stored message as JSON.
    #[napi]
    pub async fn send_direct_message(&self, recipient: String, text: String) -> Result<String> {
        let recipient = parse_uuid(&recipient)?;
        let message = Arc::clone(&self.engine)
            .send_direct_message(recipient, &text)
            .await
            .map_err(to_napi_error)?;
        serde_json::to_string(&message).map_err(to_napi_error)
    }

    /// Send a group message, returning the stored message as JSON.
    #[napi]
    pub async fn send_group_message(&self, group_id: String, text: String) -> Result<String> {
        let group_id = parse_uuid(&group_id)?;
        let message = Arc::clone(&self.engine)
            .send_group_message(group_id, &text)
            .await
            .map_err(to_napi_error)?;
        serde_json::to_string(&message).map_err(to_napi_error)
    }

    /// Send a direct message with files attached.
    ///
    /// The bytes are copied across the boundary once, here, and never leave the
    /// main process again: the renderer asks for a file by id when it wants to
    /// display it.
    #[napi]
    pub async fn send_direct_message_with_attachments(
        &self,
        recipient: String,
        text: String,
        attachments: Vec<Attachment>,
    ) -> Result<String> {
        let recipient = parse_uuid(&recipient)?;
        let message = Arc::clone(&self.engine)
            .send_direct_message_with_attachments(recipient, &text, convert(attachments))
            .await
            .map_err(to_napi_error)?;
        serde_json::to_string(&message).map_err(to_napi_error)
    }

    /// Send a group message with files attached.
    #[napi]
    pub async fn send_group_message_with_attachments(
        &self,
        group_id: String,
        text: String,
        attachments: Vec<Attachment>,
    ) -> Result<String> {
        let group_id = parse_uuid(&group_id)?;
        let message = Arc::clone(&self.engine)
            .send_group_message_with_attachments(group_id, &text, convert(attachments))
            .await
            .map_err(to_napi_error)?;
        serde_json::to_string(&message).map_err(to_napi_error)
    }

    /// Everything known about what happened to one message.
    ///
    /// Returned as JSON rather than a napi object because it is a tree of
    /// records and the renderer only ever reads it — declaring the shape a
    /// fourth time would be a fourth place to forget a field.
    #[napi]
    pub fn message_info(&self, message_id: String) -> Result<Option<String>> {
        let id = parse_uuid(&message_id)?;
        let info = self.engine.message_info(id).map_err(to_napi_error)?;
        match info {
            Some(info) => Ok(Some(serde_json::to_string(&info).map_err(to_napi_error)?)),
            None => Ok(None),
        }
    }

    /// The bytes of an attachment, or null while it is still downloading.
    #[napi]
    pub fn file_data(&self, file_id: String) -> Result<Option<Buffer>> {
        let file_id = parse_uuid(&file_id)?;
        Ok(self
            .engine
            .file_data(file_id)
            .map_err(to_napi_error)?
            .map(Buffer::from))
    }

    /// Mark a message read and, settings permitting, tell its author.
    #[napi]
    pub async fn mark_as_read(&self, message_id: String, is_group: bool) -> Result<()> {
        let message_id = parse_uuid(&message_id)?;
        let frame_type = if is_group {
            FrameType::GroupMessage
        } else {
            FrameType::DirectMessage
        };

        Arc::clone(&self.engine)
            .mark_as_read(message_id, frame_type)
            .await
            .map_err(to_napi_error)
    }

    /// Report that the user is composing a message.
    ///
    /// Safe to call on every keystroke; sends are throttled internally.
    #[napi]
    pub async fn typing_in(&self, thread: String, is_group: bool) -> Result<()> {
        let thread = parse_uuid(&thread)?;
        let frame_type = if is_group {
            FrameType::GroupMessage
        } else {
            FrameType::DirectMessage
        };

        Arc::clone(&self.engine)
            .typing_in(thread, frame_type)
            .await
            .map_err(to_napi_error)
    }

    // -- groups --------------------------------------------------------------

    /// Create a group, returning it as JSON.
    #[napi]
    pub async fn create_group(&self, name: String, invites: Vec<String>) -> Result<String> {
        let invites: Result<Vec<_>> = invites.iter().map(|id| parse_uuid(id)).collect();
        let group = Arc::clone(&self.engine)
            .create_group(&name, &invites?)
            .await
            .map_err(to_napi_error)?;
        serde_json::to_string(&group).map_err(to_napi_error)
    }

    #[napi]
    pub async fn invite_to_group(&self, group_id: String, user_id: String) -> Result<()> {
        let (group_id, user_id) = (parse_uuid(&group_id)?, parse_uuid(&user_id)?);
        Arc::clone(&self.engine)
            .invite_to_group(group_id, user_id)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn respond_to_invite(&self, group_id: String, accept: bool) -> Result<()> {
        let group_id = parse_uuid(&group_id)?;
        Arc::clone(&self.engine)
            .respond_to_invite(group_id, accept)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn rename_group(&self, group_id: String, name: String) -> Result<()> {
        let group_id = parse_uuid(&group_id)?;
        Arc::clone(&self.engine)
            .rename_group(group_id, &name)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn leave_group(&self, group_id: String) -> Result<()> {
        let group_id = parse_uuid(&group_id)?;
        Arc::clone(&self.engine)
            .leave_group(group_id)
            .await
            .map_err(to_napi_error)
    }

    // -- conversation settings ----------------------------------------------

    /// Mute until a Unix timestamp. `-1` mutes indefinitely, `0` unmutes.
    /// Works for both direct conversations and groups.
    #[napi]
    pub async fn set_muted_until(&self, conversation: String, until: i64) -> Result<()> {
        let conversation = parse_uuid(&conversation)?;
        Arc::clone(&self.engine)
            .set_muted_until(conversation, until)
            .await
            .map_err(to_napi_error)
    }

    /// Block or unblock a contact. A blocked contact's frames are dropped on
    /// arrival, not merely hidden.
    #[napi]
    pub async fn set_user_blocked(&self, user_id: String, blocked: bool) -> Result<()> {
        let user_id = parse_uuid(&user_id)?;
        Arc::clone(&self.engine)
            .set_user_blocked(user_id, blocked)
            .await
            .map_err(to_napi_error)
    }

    /// Show or hide a direct conversation.
    ///
    /// Not a local view toggle: it is sync-scoped, so hiding a conversation
    /// hides it on this profile's other devices too. Hiding does not discard
    /// anything — the contact, their messages and their device group stay — so
    /// this is the reversible counterpart to `set_user_blocked`.
    #[napi]
    pub async fn set_open_dm(&self, user_id: String, open: bool) -> Result<()> {
        let user_id = parse_uuid(&user_id)?;
        Arc::clone(&self.engine)
            .set_open_dm(user_id, open)
            .await
            .map_err(to_napi_error)
    }

    /// Give a contact a local nickname; an empty string clears it.
    #[napi]
    pub async fn set_user_alias(&self, user_id: String, alias: String) -> Result<()> {
        let user_id = parse_uuid(&user_id)?;
        Arc::clone(&self.engine)
            .set_user_alias(user_id, &alias)
            .await
            .map_err(to_napi_error)
    }

    /// Attach private notes to a contact.
    #[napi]
    pub async fn set_user_notes(&self, user_id: String, notes: String) -> Result<()> {
        let user_id = parse_uuid(&user_id)?;
        Arc::clone(&self.engine)
            .set_user_notes(user_id, &notes)
            .await
            .map_err(to_napi_error)
    }

    /// How long messages are kept, in seconds; `0` keeps them forever.
    #[napi]
    pub async fn set_retention(&self, conversation: String, seconds: i64) -> Result<()> {
        let conversation = parse_uuid(&conversation)?;
        Arc::clone(&self.engine)
            .set_retention(conversation, seconds)
            .await
            .map_err(to_napi_error)
    }

    /// Clear a conversation's history for everyone in it.
    #[napi]
    pub async fn clear_history(&self, conversation: String) -> Result<()> {
        let conversation = parse_uuid(&conversation)?;
        Arc::clone(&self.engine)
            .clear_history(conversation)
            .await
            .map_err(to_napi_error)
    }

    /// Override read receipts for one conversation. `null` follows the profile
    /// default.
    #[napi]
    pub async fn set_read_receipts(
        &self,
        conversation: String,
        setting: Option<bool>,
    ) -> Result<()> {
        let conversation = parse_uuid(&conversation)?;
        Arc::clone(&self.engine)
            .set_read_receipts(conversation, setting)
            .await
            .map_err(to_napi_error)
    }

    /// Override typing indicators for one conversation. `null` follows the
    /// profile default.
    #[napi]
    pub async fn set_typing_indicators(
        &self,
        conversation: String,
        setting: Option<bool>,
    ) -> Result<()> {
        let conversation = parse_uuid(&conversation)?;
        Arc::clone(&self.engine)
            .set_typing_indicators(conversation, setting)
            .await
            .map_err(to_napi_error)
    }

    /// Stamp a conversation as opened now.
    ///
    /// Synchronous, unlike its neighbours, because there is nothing to send:
    /// `last_opened` is local on both implementations, so this writes a column
    /// and emits an event. It exists because a thread holding an unsent draft
    /// should stay near the top of the list rather than sinking to the age of
    /// its last message.
    #[napi]
    pub fn set_last_opened(&self, conversation: String) -> Result<()> {
        let conversation = parse_uuid(&conversation)?;
        self.engine
            .set_last_opened(conversation)
            .map_err(to_napi_error)
    }

    // -- group management ----------------------------------------------------

    /// Remove a member; passing your own ID leaves the group.
    #[napi]
    pub async fn remove_from_group(&self, group_id: String, user_id: String) -> Result<()> {
        let (group_id, user_id) = (parse_uuid(&group_id)?, parse_uuid(&user_id)?);
        Arc::clone(&self.engine)
            .remove_from_group(group_id, user_id)
            .await
            .map_err(to_napi_error)
    }

    /// Withdraw an invitation that has not been answered.
    #[napi]
    pub async fn revoke_invite(&self, group_id: String, user_id: String) -> Result<()> {
        let (group_id, user_id) = (parse_uuid(&group_id)?, parse_uuid(&user_id)?);
        Arc::clone(&self.engine)
            .revoke_invite(group_id, user_id)
            .await
            .map_err(to_napi_error)
    }

    /// Promote or demote a group administrator.
    #[napi]
    pub async fn set_group_admin(
        &self,
        group_id: String,
        user_id: String,
        admin: bool,
    ) -> Result<()> {
        let (group_id, user_id) = (parse_uuid(&group_id)?, parse_uuid(&user_id)?);
        Arc::clone(&self.engine)
            .set_group_admin(group_id, user_id, admin)
            .await
            .map_err(to_napi_error)
    }

    /// Delete a group for everyone. Admins only.
    #[napi]
    pub async fn delete_group(&self, group_id: String) -> Result<()> {
        let group_id = parse_uuid(&group_id)?;
        Arc::clone(&self.engine)
            .delete_group(group_id)
            .await
            .map_err(to_napi_error)
    }

    /// Block a group, which also leaves it.
    #[napi]
    pub async fn block_group(&self, group_id: String) -> Result<()> {
        let group_id = parse_uuid(&group_id)?;
        Arc::clone(&self.engine)
            .block_group(group_id)
            .await
            .map_err(to_napi_error)
    }

    /// Restrict or unrestrict one of a group's permissions. Admins only.
    ///
    /// `permission` is one of `"posting"`, `"edits"`, `"userManagement"`.
    #[napi]
    pub async fn set_group_permission(
        &self,
        group_id: String,
        permission: String,
        restricted: bool,
    ) -> Result<()> {
        let group_id = parse_uuid(&group_id)?;
        let permission = match permission.as_str() {
            "posting" => GroupPermission::Posting,
            "edits" => GroupPermission::Edits,
            "userManagement" => GroupPermission::UserManagement,
            other => {
                return Err(Error::new(
                    Status::InvalidArg,
                    format!("unknown group permission: {other}"),
                ))
            }
        };

        Arc::clone(&self.engine)
            .set_group_permission(group_id, permission, restricted)
            .await
            .map_err(to_napi_error)
    }

    // -- profile -------------------------------------------------------------

    /// Change the name contacts see.
    #[napi]
    pub async fn update_profile_name(&self, name: String) -> Result<()> {
        Arc::clone(&self.engine)
            .update_profile_name(&name)
            .await
            .map_err(to_napi_error)
    }

    // -- misc ----------------------------------------------------------------

    /// Save a draft and sync it to this user's other devices.
    #[napi]
    pub async fn save_draft(&self, thread: String, text: String) -> Result<()> {
        let thread = parse_uuid(&thread)?;
        Arc::clone(&self.engine)
            .save_draft(thread, &text)
            .await
            .map_err(to_napi_error)
    }

    // -- profile settings ----------------------------------------------------
    //
    // These only touch the database and emit an event; nothing is broadcast,
    // so they are synchronous like `initial_state` rather than promise-returning.

    /// The profile-wide settings, as JSON.
    #[napi]
    pub fn settings(&self) -> Result<String> {
        let settings = self.engine.settings().map_err(to_napi_error)?;
        serde_json::to_string(&settings).map_err(to_napi_error)
    }

    /// Retention, in seconds, for conversations started from now on.
    #[napi]
    pub async fn set_default_retention(&self, seconds: i64) -> Result<()> {
        self.engine.set_default_retention(seconds)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn set_default_read_receipts(&self, enabled: bool) -> Result<()> {
        self.engine.set_default_read_receipts(enabled)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn set_default_typing_indicators(&self, enabled: bool) -> Result<()> {
        self.engine
            .set_default_typing_indicators(enabled)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn set_new_group_restrict_posting(&self, restricted: bool) -> Result<()> {
        self.engine
            .set_new_group_restrict_posting(restricted)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn set_new_group_restrict_edits(&self, restricted: bool) -> Result<()> {
        self.engine
            .set_new_group_restrict_edits(restricted)
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn set_new_group_restrict_user_management(&self, restricted: bool) -> Result<()> {
        self.engine
            .set_new_group_restrict_user_management(restricted)
            .await
            .map_err(to_napi_error)
    }

    /// 0 joins only groups with no unknown users, 1 never joins, 2 always does.
    #[napi]
    pub async fn set_auto_join_groups(&self, setting: i64) -> Result<()> {
        self.engine.set_auto_join_groups(setting)
            .await
            .map_err(to_napi_error)
    }

    // -- this profile's devices ----------------------------------------------

    /// Every device in this profile's device group, as JSON.
    #[napi]
    pub fn devices(&self) -> Result<String> {
        let devices = self.engine.devices().map_err(to_napi_error)?;
        serde_json::to_string(&devices).map_err(to_napi_error)
    }

    /// Give one of this profile's devices a name. Local only; nothing is sent.
    #[napi]
    pub fn rename_device(&self, device_id: String, name: String) -> Result<()> {
        let device_id = parse_uuid(&device_id)?;
        self.engine.rename_device(device_id, &name).map_err(to_napi_error)
    }

    /// Dial a conversation's devices now rather than waiting for the next
    /// peering audit.
    ///
    /// Opening a conversation is a statement that the other side is wanted;
    /// the audit interval is the floor on how stale a connection can get, not
    /// the response time when somebody is looking at the screen.
    #[napi]
    pub async fn reach_for(&self, conversation: String) -> Result<()> {
        let conversation = parse_uuid(&conversation)?;
        Arc::clone(&self.engine).reach_for(conversation).await;
        Ok(())
    }

    // -- device pairing and pictures ----------------------------------------

    /// A code another device can use to join this profile.
    ///
    /// Deliberately distinct from `create_pairing_code`, which invites a
    /// contact. The two look the same and grant very different things.
    #[napi]
    pub fn create_sync_code(&self) -> Result<String> {
        self.engine.create_sync_code().map_err(to_napi_error)
    }

    /// Join an existing profile using a code from one of its devices.
    #[napi]
    pub async fn request_to_sync(&self, code: String) -> Result<()> {
        Arc::clone(&self.engine)
            .request_to_sync(&code)
            .await
            .map_err(to_napi_error)
    }

    /// Take a device out of this profile's device group.
    #[napi]
    pub async fn revoke_device(&self, device_id: String) -> Result<()> {
        let device_id = parse_uuid(&device_id)?;
        Arc::clone(&self.engine)
            .revoke_device(device_id)
            .await
            .map_err(to_napi_error)
    }

    /// Set this profile's picture.
    #[napi]
    pub async fn set_profile_image(&self, image: Attachment) -> Result<()> {
        let image = convert(vec![image]).remove(0);
        Arc::clone(&self.engine)
            .set_profile_image(image)
            .await
            .map_err(to_napi_error)
    }

    /// Set a group's picture.
    #[napi]
    pub async fn set_group_image(&self, group_id: String, image: Attachment) -> Result<()> {
        let group_id = parse_uuid(&group_id)?;
        let image = convert(vec![image]).remove(0);
        Arc::clone(&self.engine)
            .set_group_image(group_id, image)
            .await
            .map_err(to_napi_error)
    }

    /// Dial a peer by address.
    #[napi]
    pub async fn connect_to_peer(&self, address: String) -> Result<()> {
        Arc::clone(&self.engine)
            .connect(&address)
            .await
            .map_err(to_napi_error)
    }

    /// Detach the event sink, so the engine stops calling into JavaScript.
    ///
    /// The runtime owns a listener task and a per-peer writer task, none of
    /// which end on their own; without this the threadsafe function keeps the
    /// Node process alive after the window closes. The engine itself is not
    /// torn down — the process is about to exit — but nothing further crosses
    /// the boundary.
    #[napi]
    pub fn shutdown(&self) -> Result<()> {
        let sink = Arc::clone(&self.sink);
        self.runtime.block_on(async move {
            if let Some(callback) = sink.lock().await.take() {
                // Release the reference the threadsafe function holds on the
                // JavaScript side.
                let _ = callback.abort();
            }
        });
        Ok(())
    }
}

/// Load the device key from disk, generating one on first run.
fn load_or_create_key(directory: &std::path::Path) -> Result<DeviceKey> {
    let path = directory.join("device_key");

    if path.exists() {
        let bytes = std::fs::read(&path).map_err(to_napi_error)?;
        return DeviceKey::from_bytes(&bytes).map_err(to_napi_error);
    }

    let key = DeviceKey::generate();
    std::fs::write(&path, key.to_private_bytes()).map_err(to_napi_error)?;

    // The key is the device's identity; nothing else on the system needs it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(to_napi_error)?;
    }

    Ok(key)
}

/// Turn on engine logging when `BOUNCE_LOG` is set.
///
/// Everything below the event stream — which frame was refused and why, which
/// peer a chunk went to, why a transfer stopped — is invisible to the client
/// otherwise. An attachment that silently failed turned out to be one line
/// about a frame size.
///
/// Output goes to stderr *and* to `bounce.log` in the data directory, because
/// an app launched from Finder has no stderr anybody can read. Off unless
/// asked for: these lines name onion addresses and conversation ids.
fn install_logging(directory: &std::path::Path) {
    use std::sync::Once;

    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let Ok(filter) = std::env::var("BOUNCE_LOG") else {
            return;
        };

        let path = directory.join("bounce.log");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path);

        match file {
            Ok(file) => {
                let _ = tracing_subscriber::fmt()
                    .with_env_filter(tracing_subscriber::EnvFilter::new(&filter))
                    .with_ansi(false)
                    .with_writer(move || {
                        // Cloning the handle per event costs a syscall, which
                        // is nothing next to what is being logged.
                        file.try_clone().map(LogSink::File).unwrap_or(LogSink::Stderr)
                    })
                    .try_init();
                eprintln!("bounce: logging to {}", path.display());
            }
            Err(error) => {
                eprintln!("bounce: could not open {}: {error}", path.display());
                let _ = tracing_subscriber::fmt()
                    .with_env_filter(tracing_subscriber::EnvFilter::new(&filter))
                    .with_writer(std::io::stderr)
                    .try_init();
            }
        }
    });
}

/// Where a log line goes, with a fallback if the file handle is lost.
enum LogSink {
    File(std::fs::File),
    Stderr,
}

impl std::io::Write for LogSink {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        match self {
            LogSink::File(file) => file.write(buffer),
            LogSink::Stderr => std::io::stderr().write(buffer),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            LogSink::File(file) => file.flush(),
            LogSink::Stderr => std::io::stderr().flush(),
        }
    }
}

fn parse_uuid(value: &str) -> Result<uuid::Uuid> {
    uuid::Uuid::parse_str(value)
        .map_err(|error| Error::new(Status::InvalidArg, format!("invalid UUID: {error}")))
}

fn to_napi_error<E: std::fmt::Display>(error: E) -> Error {
    Error::new(Status::GenericFailure, error.to_string())
}

/// Keeps the unused-import lint honest about the type alias above.
const _: fn() = || {
    let _: Option<Event> = None;
};

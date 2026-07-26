//! Sending attachments, and fetching the ones that arrive.
//!
//! A file never travels with the message it belongs to. The message carries an
//! attachment record naming a file ID; the file's *metadata* — name, size, and
//! the ordered list of chunk hashes — goes out separately as a signed [`File`]
//! frame; and the bytes themselves move one chunk at a time, on request.
//!
//! ```text
//!   sender                                   receiver
//!     │  DirectMessage (names file IDs)  ───▶ │  bubble appears, greyed out
//!     │  File (hash list, size, name)    ───▶ │  chunk rows created, empty
//!     │  ChunkOffer × n ("I have this")  ───▶ │
//!     │ ◀─── ChunkRequest (by hash)           │
//!     │  Chunk (raw bytes)               ───▶ │  hash checked, stored
//!     │                                       │  ChunkOffer × n re-broadcast
//! ```
//!
//! Three things fall out of that shape:
//!
//! **Chunks are content-addressed.** A chunk is identified by the hash of its
//! bytes, never by a name a peer chose, so a peer that answers a request with
//! different data is caught by the hash check and the data is dropped. It also
//! means the same chunk serves every file that contains it.
//!
//! **Anyone who has a chunk can serve it.** Once a device stores a chunk it
//! offers it in turn, so a file spreads through a group rather than streaming
//! out of the author's device fifteen times. Requests go to one holder at a
//! time; a [`ChunkUnavailable`] reply moves on to the next.
//!
//! **The bytes are optional.** A message renders as soon as it arrives, with
//! its attachments showing progress, and the interface asks for the data when
//! it has it. Losing a peer mid-download costs the chunks in flight, not the
//! message.
//!
//! ## What is not here
//!
//! Only files small enough to embed ([`crate::EMBEDDED_FILE_LIMIT`]) can be
//! sent. Larger ones are seeded in place from disk in the Go implementation,
//! which needs a path that survives restarts and a reader that streams rather
//! than buffering; sending one here fails with a message saying so rather than
//! silently truncating. Encrypted-device storage — where a device holds
//! ciphertext chunks it cannot read — is likewise unimplemented; those frame
//! types are reserved but never sent.

use uuid::Uuid;

use crate::error::{Error, Result};
use crate::frames::file::{self, ChunkOffer, ChunkRequest, ChunkUnavailable, File};
use crate::frames::message::{DirectMessage, FileAttachment, GroupMessage, ImageAttachment};
use crate::frames::update::{UpdateUser, UpdateUserType};
use crate::frames::SignedFrame;
use crate::net::Network;
use crate::signed::SignedContainer;
use crate::types::{FileType, FrameType, Scope, UpdateGroupType};
use crate::wire::RawFrame;

use super::event::{AttachmentView, MessageView};
use super::{Engine, Event};

/// A file the interface is asking to send.
///
/// The dimensions come from the client, which has already decoded the image to
/// show a preview; the engine does not decode images itself.
#[derive(Debug, Clone)]
pub struct OutgoingAttachment {
    pub name: String,
    pub data: Vec<u8>,
    /// Whether to attach this as an image, so it renders inline.
    pub is_image: bool,
    pub width: i64,
    pub height: i64,
    /// A BlurHash placeholder, if the client computed one.
    pub blur_hash: String,
}

impl<N: Network + 'static> Engine<N> {
    // ---------------------------------------------------------------------
    // Sending
    // ---------------------------------------------------------------------

    /// Write and broadcast a direct message carrying attachments.
    ///
    /// The message goes out first and the files follow, so the recipient has
    /// somewhere to hang the progress before any bytes move.
    pub async fn send_direct_message_with_attachments(
        &self,
        recipient: Uuid,
        text: &str,
        attachments: Vec<OutgoingAttachment>,
    ) -> Result<MessageView> {
        let my_id = self.store.my_user_id()?;

        if text.chars().count() > crate::MAXIMUM_MESSAGE_CHARACTERS {
            return Err(Error::InvalidFrame("message is too long".into()));
        }

        let mut message = DirectMessage::new(my_id, recipient, text.to_string(), crate::now());
        message.saved_at = crate::now();

        if let Some(user) = self.store.user(recipient)? {
            if user.retention > 0 {
                message.delete_at = crate::now() + user.retention;
            }
        }

        // A note to self only needs to reach our own devices, and has no
        // counterparty to XOR out.
        let (scope, destination) = if message.xor.is_nil() {
            (Scope::Sync, my_id)
        } else {
            (Scope::User, message.xor)
        };

        let files = self.stage_attachments(
            &attachments,
            message.id,
            my_id,
            scope,
            destination,
            message.written_at,
        )?;
        attach(
            &files,
            &attachments,
            message.id,
            &mut message.file_attachments,
            &mut message.image_attachments,
        );

        if message.is_empty() {
            return Err(Error::InvalidFrame("refusing to send an empty message".into()));
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
        self.announce_files(&files).await?;
        Ok(view)
    }

    /// Write and broadcast a group message carrying attachments.
    pub async fn send_group_message_with_attachments(
        &self,
        group_id: Uuid,
        text: &str,
        attachments: Vec<OutgoingAttachment>,
    ) -> Result<MessageView> {
        let my_id = self.store.my_user_id()?;

        if text.chars().count() > crate::MAXIMUM_MESSAGE_CHARACTERS {
            return Err(Error::InvalidFrame("message is too long".into()));
        }

        let group = self.store.group(group_id)?.ok_or(Error::GroupNotFound)?;
        if group.restrict_posting && !group.admin_ids().contains(&my_id) {
            return Err(Error::NotPermitted("posting is restricted to admins"));
        }

        let mut message = GroupMessage::new(my_id, group_id, text.to_string(), crate::now());
        message.saved_at = crate::now();
        if group.retention > 0 {
            message.delete_at = crate::now() + group.retention;
        }

        let files = self.stage_attachments(
            &attachments,
            message.id,
            my_id,
            Scope::Group,
            group_id,
            message.written_at,
        )?;
        attach(
            &files,
            &attachments,
            message.id,
            &mut message.file_attachments,
            &mut message.image_attachments,
        );

        if message.is_empty() {
            return Err(Error::InvalidFrame("refusing to send an empty message".into()));
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
        self.announce_files(&files).await?;
        Ok(view)
    }

    /// Set this profile's picture.
    ///
    /// An avatar is an ordinary distributed file with two differences: its type
    /// says what it is for, and its scope is [`Scope::Global`] rather than one
    /// conversation — a picture is shown to everyone who knows you, so it has
    /// to reach all of them. The [`UpdateUser`] that follows is what tells
    /// contacts to look for it; the file itself is fetched the same way any
    /// attachment is.
    pub async fn set_profile_image(&self, image: OutgoingAttachment) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        let mut profile = self.store.profile()?.ok_or(Error::NoProfile)?;

        let record = self.stage_image(&image, my_id, my_id, FileType::UserImage)?;

        // Go keeps the history as a comma-separated list and reads the last
        // entry as current, so an older device shown an earlier id still has
        // something to render rather than nothing.
        profile.images = push_image(&profile.images, record.id);
        self.store.save_user(&profile)?;

        let mut update = UpdateUser::new(
            my_id,
            UpdateUserType::UpdateImage,
            record.id.as_bytes().to_vec(),
            self.next_profile_update_timestamp(my_id)?,
        );
        update.saved_at = crate::now();

        let body = crate::msgpack::to_vec(&update)?;
        let container = SignedContainer::create(&self.key, body);
        update.signed = SignedFrame::from_container(&container);
        self.store.save_update_user(&update)?;

        self.emit(Event::UserUpdated {
            user: self.user_view(&profile, true),
        });

        self.broadcast(&update).await?;
        self.announce_files(std::slice::from_ref(&record)).await?;
        Ok(())
    }

    /// Set a group's picture.
    ///
    /// Unlike a profile picture this goes through group consensus, because a
    /// group's appearance is shared state that any admin may change and two
    /// admins may change at once.
    pub async fn set_group_image(&self, group_id: Uuid, image: OutgoingAttachment) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        let group = self.store.group(group_id)?.ok_or(Error::GroupNotFound)?;

        if group.restrict_group_edits && !group.admin_ids().contains(&my_id) {
            return Err(Error::NotPermitted("editing is restricted to admins"));
        }

        let record = self.stage_image(&image, my_id, group_id, FileType::GroupImage)?;

        // The file is announced before the update that names it, or a member
        // acts on an id they cannot yet fetch.
        self.announce_files(std::slice::from_ref(&record)).await?;

        let update = self.sign_update_group(
            my_id,
            group_id,
            UpdateGroupType::SetImage,
            record.id.as_bytes().to_vec(),
        )?;
        self.apply_update_group(update).await
    }

    /// A timestamp strictly newer than any profile update we hold.
    ///
    /// Profile updates replay in timestamp order and timestamps have
    /// one-second resolution, so two changes in the same second would be
    /// ordered by their random ids and the older one could win.
    fn next_profile_update_timestamp(&self, user_id: Uuid) -> Result<i64> {
        let latest = self
            .store
            .updates_for_user(user_id)?
            .iter()
            .map(|update| update.timestamp)
            .max()
            .unwrap_or(0);
        Ok(crate::now().max(latest + 1))
    }

    /// Store an image as a globally scoped file, ready to be announced.
    fn stage_image(
        &self,
        image: &OutgoingAttachment,
        author: Uuid,
        destination: Uuid,
        kind: FileType,
    ) -> Result<File> {
        if !image.is_image {
            return Err(Error::InvalidFrame("a picture is expected here".into()));
        }

        self.stage_file(
            image,
            // An avatar hangs off the thing it depicts rather than off a
            // message, which is what `attached_to` means for these types.
            destination,
            author,
            Scope::Global,
            destination,
            crate::now(),
            kind,
        )
    }

    /// The assembled bytes of a file, once every chunk is present.
    pub fn file_data(&self, file_id: Uuid) -> Result<Option<Vec<u8>>> {
        self.store.file_data(file_id)
    }

    /// A file's metadata, for callers that need to know what it is for.
    pub fn file(&self, file_id: Uuid) -> Result<Option<File>> {
        self.store.file(file_id)
    }

    /// Split each attachment into chunks and store it, signed and ready to
    /// serve, without announcing anything yet.
    fn stage_attachments(
        &self,
        attachments: &[OutgoingAttachment],
        message_id: Uuid,
        author: Uuid,
        scope: Scope,
        destination: Uuid,
        written_at: i64,
    ) -> Result<Vec<File>> {
        attachments
            .iter()
            .map(|attachment| {
                self.stage_attachment(attachment, message_id, author, scope, destination, written_at)
            })
            .collect()
    }

    fn stage_attachment(
        &self,
        attachment: &OutgoingAttachment,
        message_id: Uuid,
        author: Uuid,
        scope: Scope,
        destination: Uuid,
        written_at: i64,
    ) -> Result<File> {
        self.stage_file(
            attachment,
            message_id,
            author,
            scope,
            destination,
            written_at,
            FileType::MessageAttachment,
        )
    }

    /// Store a file, signed and ready to serve, without announcing anything.
    ///
    /// `kind` is on the wire and therefore inside the signature, so it has to
    /// be decided here rather than patched onto the record afterwards.
    #[allow(clippy::too_many_arguments)]
    fn stage_file(
        &self,
        attachment: &OutgoingAttachment,
        attached_to: Uuid,
        author: Uuid,
        scope: Scope,
        destination: Uuid,
        written_at: i64,
        kind: FileType,
    ) -> Result<File> {
        if attachment.data.is_empty() {
            return Err(Error::InvalidFrame("refusing to send an empty file".into()));
        }
        if attachment.data.len() as i64 > crate::EMBEDDED_FILE_LIMIT {
            return Err(Error::InvalidFrame(format!(
                "{} is larger than the {} MiB attachment limit",
                attachment.name,
                crate::EMBEDDED_FILE_LIMIT / (1024 * 1024)
            )));
        }

        let file_id = Uuid::new_v4();
        let chunks = file::split_into_chunks(file_id, &attachment.data);

        let mut record = File {
            signed: SignedFrame::default(),
            id: file_id,
            name: attachment.name.clone(),
            file_type: kind as i64,
            attached_to,
            hash: hex::encode(crate::crypto::hash(&attachment.data)),
            size: attachment.data.len() as i64,
            chunk_size: crate::CHUNK_SIZE as i64,
            hash_list: file::hash_list(&chunks),
            encrypted_hash_list: String::new(),
            key: Vec::new(),
            nonce: Vec::new(),
            path: String::new(),
            wanted: true,
            downloaded: true,
            scope: scope.as_i64(),
            destination,
            author,
            timestamp: written_at,
            saved_at: crate::now(),
        };

        // Signing covers only the fields that go on the wire; `wanted`,
        // `downloaded` and `path` are this device's own business and are
        // skipped, so setting them above does not invalidate anything.
        let body = crate::msgpack::to_vec(&record)?;
        let container = SignedContainer::create(&self.key, body);
        record.signed = SignedFrame::from_container(&container);

        self.store.save_file(&record)?;
        for chunk in &chunks {
            self.store
                .save_chunk(file_id, chunk.index, &chunk.hash, Some(&chunk.data))?;
        }

        Ok(record)
    }

    /// Broadcast each file's metadata, then advertise every chunk of it.
    async fn announce_files(&self, files: &[File]) -> Result<()> {
        for record in files {
            tracing::info!(
                file = %record.id, name = %record.name, size = record.size,
                chunks = record.chunk_hashes().len(), scope = record.scope,
                "announcing a file",
            );
            self.broadcast(record).await?;
            for hash in record.chunk_hashes() {
                self.offer_chunk(record, &hash).await?;
            }
        }
        Ok(())
    }

    /// Tell everyone in a file's scope that this device holds one of its
    /// chunks.
    async fn offer_chunk(&self, record: &File, hash: &str) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        let now = crate::now();

        let mut offer = ChunkOffer {
            signed: SignedFrame::default(),
            id: Uuid::new_v4(),
            scope: record.scope,
            destination: record.destination,
            // The offer is ours, whoever wrote the file: it says where a copy
            // is, and the device signing it is the one holding that copy.
            author: my_id,
            file_id: record.id,
            hash: hash.to_string(),
            location: self.network.address(),
            timestamp: now,
            saved_at: now,
            last_request_time: 0,
        };

        let body = crate::msgpack::to_vec(&offer)?;
        let container = SignedContainer::create(&self.key, body);
        offer.signed = SignedFrame::from_container(&container);

        tracing::debug!(file = %record.id, hash = %hash, "offering a chunk");
        self.broadcast(&offer).await
    }

    // ---------------------------------------------------------------------
    // Receiving
    // ---------------------------------------------------------------------

    /// A file's metadata arrived: record what it is made of, so the chunks
    /// have somewhere to land.
    pub(super) async fn handle_file(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let (mut record, signed) = self.unpack_signed::<File>(payload)?;
        record.signed = signed;

        if !self.signer_speaks_for(&record.signed.signer, record.author, record.timestamp)? {
            return Err(Error::InvalidFrame(
                "file signer does not speak for its author".into(),
            ));
        }
        if let Some(author) = self.store.user(record.author)? {
            if author.blocked {
                self.send_ack(peer, record.id, FrameType::File).await;
                return Ok(());
            }
        }

        let hashes = record.chunk_hashes();
        if hashes.is_empty() || record.size <= 0 {
            self.send_ack(peer, record.id, FrameType::File).await;
            return Err(Error::InvalidFrame("file describes no content".into()));
        }
        // A hash list that does not add up to the declared size is either a
        // mistake or an attempt to make us hold more than we agreed to.
        let claimed = hashes.len() as i64 * record.chunk_size.max(1);
        if record.size > claimed || record.size <= claimed - record.chunk_size.max(1) {
            self.send_ack(peer, record.id, FrameType::File).await;
            return Err(Error::InvalidFrame(
                "file size does not match its chunk list".into(),
            ));
        }

        if self.store.has_frame(record.id, FrameType::File)? {
            self.send_ack(peer, record.id, FrameType::File).await;
            return Ok(());
        }

        record.saved_at = crate::now();
        // Small files are fetched without being asked for; anything larger is
        // left until somebody wants it.
        record.wanted = record.is_embedded();
        record.downloaded = false;

        tracing::info!(
            file = %record.id, name = %record.name, size = record.size,
            chunks = hashes.len(), wanted = record.wanted,
            "accepted a file record",
        );
        self.store.save_file(&record)?;
        for (index, hash) in hashes.iter().enumerate() {
            self.store.save_chunk(record.id, index as i64, hash, None)?;
        }
        self.send_ack(peer, record.id, FrameType::File).await;

        // We may already hold some of these chunks, from another file that
        // happens to share them.
        self.report_progress(&record).await?;

        // Relay onward, then ask whoever has already offered chunks for them.
        self.broadcast(&record).await?;
        self.request_missing_chunks(&record).await?;
        Ok(())
    }

    /// Somebody says they hold a chunk. Remember where, and ask for it if we
    /// want it.
    pub(super) async fn handle_chunk_offer(&self, _peer: &str, payload: &[u8]) -> Result<()> {
        let (mut offer, signed) = self.unpack_signed::<ChunkOffer>(payload)?;
        offer.signed = signed;

        if !self.signer_speaks_for(&offer.signed.signer, offer.author, offer.timestamp)? {
            return Err(Error::InvalidFrame(
                "chunk offer signer does not speak for its author".into(),
            ));
        }
        if let Some(author) = self.store.user(offer.author)? {
            if author.blocked {
                return Ok(());
            }
        }
        // An offer names the device holding the chunk. Accepting a location
        // the signer does not control would let anyone redirect downloads at
        // a device of their choosing.
        if offer.location != offer.signed.signer {
            return Err(Error::InvalidFrame(
                "chunk offer points somewhere its signer does not control".into(),
            ));
        }

        // Offers are not stored as frames, so gossip is bounded by novelty
        // instead: an offer that tells us nothing new goes no further.
        if !self
            .store
            .record_chunk_location(&offer.hash, &offer.location, offer.timestamp)?
        {
            tracing::trace!(hash = %offer.hash, "a chunk offer we already had");
            return Ok(());
        }
        tracing::debug!(
            file = %offer.file_id, hash = %offer.hash, from = %offer.location,
            "a chunk was offered",
        );
        self.broadcast(&offer).await?;

        let Some(record) = self.store.file(offer.file_id)? else {
            // Metadata we have not seen yet. The location is remembered, and
            // the request goes out when the file record arrives.
            tracing::debug!(file = %offer.file_id, "offered a chunk of a file we do not know yet");
            return Ok(());
        };
        if !record.wanted {
            tracing::debug!(file = %offer.file_id, "not fetching a file we do not want");
            return Ok(());
        }
        if self.store.has_chunk(&offer.hash)? {
            return Ok(());
        }

        self.request_chunk(&offer.hash, &offer.location).await;
        Ok(())
    }

    /// A peer wants a chunk. Send it, or say we do not have it so they can ask
    /// somebody else rather than waiting.
    pub(super) async fn handle_chunk_request(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let request: ChunkRequest = crate::msgpack::from_slice(payload)?;

        match self.store.chunk_data(&request.hash)? {
            Some(data) => {
                tracing::debug!(hash = %request.hash, to = %peer, bytes = data.len(), "serving a chunk");
                // The payload is the chunk itself. See `frames::file::Chunk`:
                // the receiver hashes the whole frame to identify it, so any
                // wrapper at all makes the chunk unrecognisable.
                self.send_to(peer, RawFrame::new(FrameType::Chunk.as_u16(), data))
                    .await;
            }
            None => {
                tracing::warn!(hash = %request.hash, to = %peer, "asked for a chunk we do not have");
                let reply = ChunkUnavailable { hash: request.hash };
                self.send_to(
                    peer,
                    RawFrame::new(FrameType::ChunkUnavailable.as_u16(), reply.encode()?),
                )
                .await;
            }
        }
        Ok(())
    }

    /// Chunk bytes arrived.
    ///
    /// Nothing on the wire says which chunk this is — the hash of what arrived
    /// does. A peer that sends bytes we never asked for simply matches no
    /// outstanding chunk and is dropped.
    pub(super) async fn handle_chunk(&self, _peer: &str, payload: &[u8]) -> Result<()> {
        // The payload is the chunk, unwrapped. Hashing it is the only way to
        // learn which chunk it is.
        if payload.is_empty() || payload.len() > crate::CHUNK_SIZE {
            return Err(Error::InvalidFrame("chunk is not a plausible size".into()));
        }

        let hash = hex::encode(crate::crypto::hash(payload));
        let Some(file_id) = self.store.file_for_chunk(&hash)? else {
            // Either a chunk we never asked for, or — the interop failure —
            // one whose framing does not match what we hash.
            tracing::warn!(
                bytes = payload.len(), hash = %hash,
                "a chunk arrived that matches no file we know",
            );
            return Ok(());
        };
        tracing::debug!(file = %file_id, hash = %hash, bytes = payload.len(), "a chunk arrived");
        let Some(record) = self.store.file(file_id)? else {
            return Ok(());
        };
        if !record.wanted || self.store.has_chunk(&hash)? {
            return Ok(());
        }

        let index = record
            .chunk_hashes()
            .iter()
            .position(|candidate| candidate == &hash)
            .unwrap_or(0) as i64;
        self.store.save_chunk(file_id, index, &hash, Some(payload))?;

        // Having it means we can serve it.
        self.offer_chunk(&record, &hash).await?;
        self.report_progress(&record).await?;
        Ok(())
    }

    /// A holder turned out not to have the chunk after all.
    pub(super) async fn handle_chunk_unavailable(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let reply: ChunkUnavailable = crate::msgpack::from_slice(payload)?;
        self.store.forget_chunk_location(&reply.hash, peer)?;

        // Try the next holder we know of, if there is one.
        for location in self.store.chunk_locations(&reply.hash)? {
            if location != peer {
                self.request_chunk(&reply.hash, &location).await;
                break;
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Fetching
    // ---------------------------------------------------------------------

    /// Ask one device for one chunk.
    async fn request_chunk(&self, hash: &str, location: &str) {
        tracing::debug!(hash = %hash, from = %location, "requesting a chunk");
        let request = ChunkRequest {
            hash: hash.to_string(),
        };
        match crate::msgpack::to_vec(&request) {
            Ok(payload) => {
                self.send_to(
                    location,
                    RawFrame::new(FrameType::ChunkRequest.as_u16(), payload),
                )
                .await;
            }
            Err(error) => tracing::warn!(%error, "could not encode a chunk request"),
        }
    }

    /// Ask for every chunk of a file we do not have, from whoever has offered
    /// it.
    async fn request_missing_chunks(&self, record: &File) -> Result<()> {
        if !record.wanted {
            return Ok(());
        }
        for hash in record.chunk_hashes() {
            if self.store.has_chunk(&hash)? {
                continue;
            }
            // One holder at a time: a `ChunkUnavailable` moves on to the next,
            // so asking everyone at once would only duplicate the transfer.
            if let Some(location) = self.store.chunk_locations(&hash)?.into_iter().next() {
                self.request_chunk(&hash, &location).await;
            }
        }
        Ok(())
    }

    /// Ask a device that just connected for anything still outstanding.
    ///
    /// Chunk offers are ephemeral — they are not stored, so the reference flow
    /// never replays them — which means a download interrupted by a
    /// disconnection would otherwise never resume. Asking the peer directly
    /// costs one frame per missing chunk and is answered with
    /// [`ChunkUnavailable`] if they do not have it.
    pub(super) async fn resume_downloads(&self, peer: &str) -> Result<()> {
        for record in self.store.incomplete_wanted_files()? {
            for hash in record.chunk_hashes() {
                if !self.store.has_chunk(&hash)? {
                    self.request_chunk(&hash, peer).await;
                }
            }
        }
        Ok(())
    }

    /// Emit how far along a file is, and mark it done once it is whole.
    async fn report_progress(&self, record: &File) -> Result<()> {
        let fraction = self.store.file_progress(record.id)?;

        if fraction >= 1.0 {
            if !record.downloaded {
                self.store.mark_file_downloaded(record.id)?;
            }
            self.emit(Event::FileComplete { file_id: record.id });
        } else {
            self.emit(Event::FileProgress {
                file_id: record.id,
                fraction,
            });
        }
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Views
    // ---------------------------------------------------------------------

    /// Render a message's attachments, with how much of each is present.
    pub(super) fn attachment_views(
        &self,
        images: &[ImageAttachment],
        files: &[FileAttachment],
    ) -> Vec<AttachmentView> {
        let mut views = Vec::with_capacity(images.len() + files.len());

        for attachment in images {
            views.push(AttachmentView {
                id: attachment.id,
                file_id: attachment.file_id,
                name: attachment.name.clone(),
                size: attachment.size,
                width: Some(attachment.width),
                height: Some(attachment.height),
                blur_hash: Some(attachment.blur_hash.clone()),
                progress: self.store.file_progress(attachment.file_id).unwrap_or(0.0),
            });
        }
        for attachment in files {
            views.push(AttachmentView {
                id: attachment.id,
                file_id: attachment.file_id,
                name: attachment.name.clone(),
                size: attachment.size,
                width: None,
                height: None,
                blur_hash: None,
                progress: self.store.file_progress(attachment.file_id).unwrap_or(0.0),
            });
        }
        views
    }
}

/// Hang staged files off a message as attachment records.
fn attach(
    files: &[File],
    attachments: &[OutgoingAttachment],
    message_id: Uuid,
    file_attachments: &mut Vec<FileAttachment>,
    image_attachments: &mut Vec<ImageAttachment>,
) {
    for (record, attachment) in files.iter().zip(attachments) {
        if attachment.is_image {
            image_attachments.push(ImageAttachment {
                id: Uuid::new_v4(),
                file_id: record.id,
                message_id,
                name: record.name.clone(),
                size: record.size,
                width: attachment.width,
                height: attachment.height,
                blur_hash: attachment.blur_hash.clone(),
            });
        } else {
            file_attachments.push(FileAttachment {
                id: Uuid::new_v4(),
                file_id: record.id,
                message_id,
                name: record.name.clone(),
                size: record.size,
            });
        }
    }
}

/// Append an image id to a user's or group's image history.
///
/// Kept as a comma-separated list with the current picture last, matching Go.
/// The history is what lets a device that only ever heard about an earlier id
/// still render something rather than nothing.
fn push_image(existing: &str, image: Uuid) -> String {
    if existing.is_empty() {
        return image.to_string();
    }
    if existing.split(',').any(|id| id == image.to_string()) {
        return existing.to_string();
    }
    format!("{existing},{image}")
}

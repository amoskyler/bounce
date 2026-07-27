//! Reacting to a message, and deleting one that has already been sent.
//!
//! Both are shaped on the read-receipt path next door in [`super`], for the
//! same reason they are shaped on `ReadReceipt` on the wire: they name a
//! message by id and type, and they derive where they are going from that
//! message rather than trusting the sender to say. A frame that declared its own
//! conversation could claim one it does not belong to.
//!
//! ## What is different about deletion
//!
//! Everything else here follows the existing grain. Deletion has one property
//! nothing else in the engine has: **it must be able to reach a peer who was not
//! there when it happened, and it must stay applied once it does.**
//!
//! That is why the frame is stored rather than merely applied, and why the
//! target is emptied rather than removed. Storing it puts the deletion in the
//! reference flow, so a peer that reconnects a week later is offered it. Keeping
//! the row is what makes it stick: delete the row and `has_frame` answers false,
//! the peer classifies the original as wanted, offers it back, and the message
//! returns. The tombstone is not a nicety — it is the thing that makes a
//! deletion permanent.
//!
//! The tombstone keeps the message's own `delete_at`, so a deleted message in a
//! disappearing thread eventually leaves nothing behind at all.

use uuid::Uuid;

use crate::error::{Error, Result};
use crate::frames::interaction::{within_delete_window, DeleteMessage, Reaction};
use crate::frames::message::{Quote, QuoteKind};
use crate::frames::transport::KeepAlive;
use crate::frames::SignedFrame;
use crate::net::Network;
use crate::signed::SignedContainer;
use crate::types::{FrameType, Scope};

use super::event::{Event, QuoteView, ReactionView};
use super::Engine;

/// Where an interaction with a message should go, and who wrote it.
struct TargetContext {
    /// The user or group the scope resolves against.
    destination: Uuid,
    scope: Scope,
    /// The message's author.
    author: Uuid,
    /// When it was written, for the delete window.
    written_at: i64,
    /// When it expires, so a reaction can inherit it.
    delete_at: i64,
    /// The group it belongs to, if it is a group message.
    group: Option<Uuid>,
    /// Whether it has already been deleted for everyone.
    deleted: bool,
}

impl<N: Network + 'static> Engine<N> {
    // ---------------------------------------------------------------------
    // Reacting
    // ---------------------------------------------------------------------

    /// React to a message, replacing any reaction we had already left on it.
    pub async fn react(&self, target: Uuid, target_type: FrameType, emoji: &str) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        let Some(context) = self.target_context(target, target_type)? else {
            return Err(Error::InvalidFrame(
                "cannot react to a message this device does not have".into(),
            ));
        };

        // A deleted message has nothing left to react to, and a reaction would
        // be stored against a tombstone that is about to be swept.
        if context.deleted {
            return Ok(());
        }

        let mut reaction = Reaction::set(
            my_id,
            target,
            target_type,
            emoji.to_string(),
            self.next_reaction_timestamp(target, my_id),
        );
        reaction.destination = context.destination;
        reaction.scope = context.scope.as_i64();
        reaction.delete_at = context.delete_at;
        reaction.saved_at = crate::now();

        if !reaction.has_valid_payload() {
            return Err(Error::InvalidFrame(format!(
                "{emoji:?} is not a single emoji"
            )));
        }

        self.sign_and_apply_reaction(reaction).await
    }

    /// Withdraw our reaction to a message.
    pub async fn remove_reaction(&self, target: Uuid, target_type: FrameType) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        let Some(context) = self.target_context(target, target_type)? else {
            return Ok(());
        };

        let mut reaction =
            Reaction::clear(my_id, target, target_type, self.next_reaction_timestamp(target, my_id));
        reaction.destination = context.destination;
        reaction.scope = context.scope.as_i64();
        reaction.delete_at = context.delete_at;
        reaction.saved_at = crate::now();

        self.sign_and_apply_reaction(reaction).await
    }

    async fn sign_and_apply_reaction(&self, mut reaction: Reaction) -> Result<()> {
        let body = crate::msgpack::to_vec(&reaction)?;
        let container = SignedContainer::create(&self.key, body);
        reaction.signed = SignedFrame::from_container(&container);

        self.store.save_reaction(&reaction)?;
        self.emit(Event::MessageReacted {
            message_id: reaction.target,
            user_id: reaction.actor,
            emoji: reaction.emoji.clone(),
        });

        // A removal is broadcast even though it leaves nothing behind locally:
        // the peers still hold the reaction it withdraws, and nothing else will
        // ever tell them it is gone.
        self.broadcast(&reaction).await
    }

    /// A timestamp that is strictly later than anything we have already said
    /// about this message.
    ///
    /// Timestamps are whole seconds — the protocol's convention everywhere —
    /// and reacting, looking again, and changing your mind all happen inside
    /// one. Two frames from the same person in the same second have no
    /// well-defined order, and whichever way that is resolved something real
    /// breaks: let the later arrival win and an out-of-order catch-up
    /// resurrects a reaction that was withdrawn; let the withdrawal win and
    /// re-reacting immediately after withdrawing is silently dropped, for good.
    ///
    /// So ties are not resolved, they are avoided. A frame we create is stamped
    /// past whatever we last said, which makes the order total for the only
    /// case that produces ties in practice: one person, one message, one
    /// second. Two *different* people colliding still ties, and
    /// [`resolve_reactions`] settles that in favour of the withdrawal.
    fn next_reaction_timestamp(&self, target: Uuid, my_id: Uuid) -> i64 {
        let latest = self
            .store
            .reactions_for(target)
            .unwrap_or_default()
            .into_iter()
            .filter(|reaction| reaction.actor == my_id)
            .map(|reaction| reaction.timestamp)
            .max()
            .unwrap_or(0);

        crate::now().max(latest + 1)
    }

    /// Ingest a peer's reaction.
    pub(super) async fn handle_reaction(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let (mut reaction, signed) = self.unpack_signed::<Reaction>(payload)?;
        reaction.signed = signed;

        if self.store.has_frame(reaction.id, FrameType::Reaction)? {
            self.send_ack(peer, reaction.id, FrameType::Reaction).await;
            return Ok(());
        }

        // The signing device has to speak for the actor, exactly as it does for
        // a message. Without this anybody could react as anybody.
        if !self.signer_speaks_for(&reaction.signed.signer, reaction.actor, reaction.timestamp)? {
            tracing::warn!(id = %reaction.id, "reaction signed by a different user than its actor");
            return Ok(());
        }

        if !reaction.has_valid_payload() {
            // The only place a remote peer picks characters that end up
            // rendered in the timeline. Refuse rather than store.
            tracing::warn!(id = %reaction.id, "reaction with a payload that is not one emoji");
            self.send_ack(peer, reaction.id, FrameType::Reaction).await;
            return Ok(());
        }

        let target_type = FrameType::from_u16(reaction.target_type)?;
        let Some(context) = self.target_context(reaction.target, target_type)? else {
            // The message has not arrived. Park the reaction with a nil
            // destination and sync scope, the way an early read receipt is
            // parked, and resolve it when the message lands.
            reaction.destination = Uuid::nil();
            reaction.scope = Scope::Sync.as_i64();
            reaction.saved_at = crate::now();
            self.store.save_reaction(&reaction)?;
            self.send_ack(peer, reaction.id, FrameType::Reaction).await;
            return Ok(());
        };

        if !self.may_interact_with(reaction.actor, &context)? {
            tracing::warn!(
                id = %reaction.id, actor = %reaction.actor,
                "reaction from somebody outside the conversation",
            );
            return Ok(());
        }

        reaction.destination = context.destination;
        reaction.scope = context.scope.as_i64();
        reaction.delete_at = context.delete_at;
        reaction.saved_at = crate::now();

        let changed = self.store.save_reaction(&reaction)?;
        self.send_ack(peer, reaction.id, FrameType::Reaction).await;

        if changed {
            self.emit(Event::MessageReacted {
                message_id: reaction.target,
                user_id: reaction.actor,
                emoji: reaction.emoji.clone(),
            });
        }

        self.broadcast(&reaction).await
    }

    // ---------------------------------------------------------------------
    // Deleting
    // ---------------------------------------------------------------------

    /// Remove a message from this device only.
    ///
    /// No frame, nothing broadcast, nobody else affected. Signal keeps this and
    /// "delete for everyone" as two menu items because they are two different
    /// promises, and conflating them is how somebody believes they withdrew
    /// something they only hid.
    pub fn delete_for_me(&self, target: Uuid, target_type: FrameType) -> Result<()> {
        self.store.delete_message_locally(target, target_type)?;
        self.emit(Event::MessageDeleted { message_id: target });
        Ok(())
    }

    /// Whether this device could delete a message for everyone right now.
    ///
    /// Exposed so the interface can show or hide the action rather than
    /// offering it and failing.
    pub fn may_delete_for_everyone(&self, target: Uuid, target_type: FrameType) -> Result<bool> {
        let my_id = self.store.my_user_id()?;
        let Some(context) = self.target_context(target, target_type)? else {
            return Ok(false);
        };
        Ok(self.delete_permission(my_id, &context).is_some())
    }

    /// Withdraw a message from everybody who received it.
    pub async fn delete_for_everyone(&self, target: Uuid, target_type: FrameType) -> Result<()> {
        let my_id = self.store.my_user_id()?;
        let Some(context) = self.target_context(target, target_type)? else {
            return Err(Error::InvalidFrame(
                "cannot delete a message this device does not have".into(),
            ));
        };

        if context.deleted {
            return Ok(());
        }

        let Some(as_admin) = self.delete_permission(my_id, &context) else {
            return Err(Error::InvalidFrame(
                "this message can no longer be deleted for everyone".into(),
            ));
        };

        let mut delete = DeleteMessage::new(my_id, target, target_type, as_admin, crate::now());
        delete.destination = context.destination;
        delete.scope = context.scope.as_i64();
        delete.saved_at = crate::now();

        let body = crate::msgpack::to_vec(&delete)?;
        let container = SignedContainer::create(&self.key, body);
        delete.signed = SignedFrame::from_container(&container);

        // Stored before it is applied, and before it is sent. A deletion we
        // applied but did not store is one the reference flow will never
        // re-offer, which means it reached whoever happened to be connected and
        // nobody else.
        self.store.save_delete_message(&delete)?;
        self.apply_deletion(&delete, target_type)?;
        self.broadcast(&delete).await
    }

    /// Ingest a peer's deletion.
    pub(super) async fn handle_delete_message(&self, peer: &str, payload: &[u8]) -> Result<()> {
        let (mut delete, signed) = self.unpack_signed::<DeleteMessage>(payload)?;
        delete.signed = signed;

        if self.store.has_frame(delete.id, FrameType::DeleteMessage)? {
            self.send_ack(peer, delete.id, FrameType::DeleteMessage).await;
            return Ok(());
        }

        if !self.signer_speaks_for(&delete.signed.signer, delete.actor, delete.timestamp)? {
            tracing::warn!(id = %delete.id, "deletion signed by a different user than its actor");
            return Ok(());
        }

        let target_type = FrameType::from_u16(delete.target_type)?;

        let Some(context) = self.target_context(delete.target, target_type)? else {
            // The deletion outran the message it withdraws, which happens
            // routinely on a catch up. Store it anyway: `handle_direct_message`
            // and `handle_group_message` consult `deletion_of` when a message
            // arrives, so the message lands already emptied rather than being
            // visible until something else comes along.
            delete.destination = Uuid::nil();
            delete.scope = Scope::Sync.as_i64();
            delete.saved_at = crate::now();
            self.store.save_delete_message(&delete)?;
            self.send_ack(peer, delete.id, FrameType::DeleteMessage).await;
            return Ok(());
        };

        if self.delete_permission_for_peer(delete.actor, delete.admin_delete, &context) {
            delete.destination = context.destination;
            delete.scope = context.scope.as_i64();
            delete.saved_at = crate::now();

            self.store.save_delete_message(&delete)?;
            self.apply_deletion(&delete, target_type)?;
            self.send_ack(peer, delete.id, FrameType::DeleteMessage).await;
            return self.broadcast(&delete).await;
        }

        tracing::warn!(
            id = %delete.id, actor = %delete.actor, admin = delete.admin_delete,
            "deletion refused: not the author, not an admin, or outside the window",
        );
        // Acked deliberately. The frame is well formed and we have made our
        // decision about it; without the ack the sender re-offers it on every
        // reference cycle for as long as both devices exist.
        self.send_ack(peer, delete.id, FrameType::DeleteMessage).await;
        Ok(())
    }

    /// Apply a deletion to the message it names, and tell the interface.
    fn apply_deletion(&self, delete: &DeleteMessage, target_type: FrameType) -> Result<()> {
        let applied = self.store.delete_message_for_everyone(
            delete.target,
            target_type,
            delete.actor,
            delete.timestamp,
        )?;

        if applied {
            self.emit(Event::MessageWithdrawn {
                message_id: delete.target,
                by: delete.actor,
                admin: delete.admin_delete,
            });
        }
        Ok(())
    }

    /// Scope and announce any reaction that arrived before its message.
    ///
    /// A parked reaction was stored with a nil destination, because there was
    /// nothing to derive one from. Now that the message is here it can be
    /// scoped — which is what puts it back into the reference flow — and the
    /// actor can be checked against the conversation, which was not possible
    /// before either.
    pub(super) fn resolve_parked_reactions(
        &self,
        target: Uuid,
        target_type: FrameType,
    ) -> Result<()> {
        let Some(context) = self.target_context(target, target_type)? else {
            return Ok(());
        };

        for mut reaction in self.store.reactions_for(target)? {
            if reaction.destination != Uuid::nil() {
                continue;
            }

            // Checked only now. A reaction from somebody outside the
            // conversation was unjudgeable while the message was missing, so it
            // was parked rather than refused; this is where it is refused.
            if !self.may_interact_with(reaction.actor, &context)? {
                self.store.discard_reaction(reaction.id)?;
                continue;
            }

            reaction.destination = context.destination;
            reaction.scope = context.scope.as_i64();
            reaction.delete_at = context.delete_at;
            self.store.rescope_reaction(
                reaction.id,
                reaction.destination,
                reaction.scope,
                reaction.delete_at,
            )?;

            self.emit(Event::MessageReacted {
                message_id: reaction.target,
                user_id: reaction.actor,
                emoji: reaction.emoji.clone(),
            });
        }
        Ok(())
    }

    /// Apply any deletion that arrived before the message it withdraws.
    ///
    /// Called from the message handlers. The two frames are independent and a
    /// catch up can replay them in either order, so this is the other half of
    /// the parking in [`Self::handle_delete_message`] — without it a message
    /// deleted while we were offline arrives visible and stays that way.
    pub(super) fn apply_pending_deletion(
        &self,
        target: Uuid,
        target_type: FrameType,
    ) -> Result<bool> {
        let Some(delete) = self.store.deletion_of(target)? else {
            return Ok(false);
        };

        let Some(context) = self.target_context(target, target_type)? else {
            return Ok(false);
        };
        if !self.delete_permission_for_peer(delete.actor, delete.admin_delete, &context) {
            return Ok(false);
        }

        self.apply_deletion(&delete, target_type)?;
        Ok(true)
    }

    // ---------------------------------------------------------------------
    // Replying
    // ---------------------------------------------------------------------

    /// Build the quote for a reply, from this device's own copy.
    ///
    /// The caller passes the id of the message being replied to and nothing
    /// else. That is deliberate: the snapshot is assembled here, from what we
    /// actually hold, so a client cannot put words in somebody else's quoted
    /// message — a quote is attributed, and an attributed excerpt somebody else
    /// chose the text of is a forgery with a name on it.
    ///
    /// Returns `None` for an unknown or deleted target, so replying to
    /// something that has just gone sends an ordinary message rather than
    /// failing.
    pub(super) fn quote_of(&self, reply_to: Option<Uuid>) -> Result<Option<Quote>> {
        let Some(target) = reply_to else {
            return Ok(None);
        };

        // Either kind; the caller does not have to know which, because a reply
        // is always within one conversation and the id is unambiguous.
        // Whatever the message carried, reduced to what a quote block needs.
        struct Quoted {
            author: Uuid,
            text: String,
            delete_at: i64,
            deleted: bool,
            kind: QuoteKind,
        }

        // The quote block should read as a reply to a photo when it is one,
        // rather than as a reply to nothing.
        macro_rules! reduce {
            ($message:expr) => {{
                let message = $message;
                let kind = if !message.image_attachments.is_empty() {
                    QuoteKind::Image
                } else if !message.file_attachments.is_empty() {
                    QuoteKind::File
                } else {
                    QuoteKind::Text
                };
                Quoted {
                    author: message.author,
                    text: message.text,
                    delete_at: message.delete_at,
                    deleted: message.deleted_at != 0,
                    kind,
                }
            }};
        }

        // Either kind; the caller does not have to know which, because a reply
        // is always within one conversation and the id is unambiguous.
        let quoted = match self.store.direct_message(target)? {
            Some(message) => Some(reduce!(message)),
            None => self.store.group_message(target)?.map(|message| reduce!(message)),
        };

        let Some(quoted) = quoted else {
            return Ok(None);
        };
        if quoted.deleted {
            return Ok(None);
        }

        Ok(Some(Quote::of(
            target,
            quoted.author,
            &quoted.text,
            quoted.kind,
            quoted.delete_at,
        )))
    }

    /// Reactions to one message, grouped by emoji for rendering.
    ///
    /// Grouped here rather than in the client because the shape is the same
    /// everywhere and the client would otherwise redo it on every render.
    /// Failures come back as no reactions: a bubble that draws without its
    /// pills is worth more than a timeline that does not draw.
    pub(super) fn reaction_views(&self, target: Uuid, my_id: Uuid) -> Vec<ReactionView> {
        group_reactions(
            self.store.reactions_for(target).unwrap_or_default().into_iter(),
            my_id,
        )
    }

    // ---------------------------------------------------------------------
    // Capability discovery
    // ---------------------------------------------------------------------

    /// Learn what a peer speaks from its keep-alive.
    ///
    /// This is what closes the gap the device record cannot: a contact paired
    /// before the extensions existed has an empty capability list stored, and
    /// nothing in the protocol ever re-announces a device. Without this they
    /// read as legacy forever, and — because the gate fails towards sending
    /// less — every reaction and every deletion is silently withheld from them,
    /// with no error anywhere to say so.
    ///
    /// Written only when the answer has changed. A keep-alive arrives on every
    /// connection on a short cycle, and a database write per peer per cycle for
    /// a value that almost never moves is a cost with nothing on the other side
    /// of it.
    pub(super) fn handle_keep_alive(&self, peer: &str, payload: &[u8]) -> Result<()> {
        // Silence is not an announcement. A Go peer sends `keep-alive` as raw
        // bytes, and treating that as "speaks nothing" would erase what a
        // capable peer had already told us on every frame it could not parse.
        let Some(announced) = KeepAlive::announced(payload) else {
            return Ok(());
        };

        let Some(mut device) = self.store.device_by_address(peer)? else {
            // A device we have no record of. Nothing to attach this to, and
            // nothing is sent to an unknown peer anyway.
            return Ok(());
        };

        if device.capabilities == announced {
            return Ok(());
        }

        tracing::debug!(peer, ?announced, "peer announced its capabilities");
        device.capabilities = announced;
        self.store.save_device(&device)?;
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Shared
    // ---------------------------------------------------------------------

    /// Everything an interaction needs to know about the message it names.
    ///
    /// Returns `None` when the message is not held on this device, which is the
    /// signal to park the frame rather than to reject it.
    fn target_context(&self, target: Uuid, target_type: FrameType) -> Result<Option<TargetContext>> {
        let my_id = self.store.my_user_id()?;

        Ok(match target_type {
            FrameType::GroupMessage => self.store.group_message(target)?.map(|message| TargetContext {
                destination: message.destination,
                scope: Scope::Group,
                author: message.author,
                written_at: message.written_at,
                delete_at: message.delete_at,
                group: Some(message.destination),
                deleted: message.is_deleted(),
            }),

            FrameType::DirectMessage => self.store.direct_message(target)?.map(|message| {
                let counterparty = crate::xor(message.xor, my_id);
                // A note to self has no counterparty; it concerns our own
                // devices and nobody else's.
                let sync_only = message.xor.is_nil() || counterparty == my_id;

                TargetContext {
                    destination: counterparty,
                    scope: if sync_only { Scope::Sync } else { Scope::User },
                    author: message.author,
                    written_at: message.written_at,
                    delete_at: message.delete_at,
                    group: None,
                    deleted: message.is_deleted(),
                }
            }),

            // Neither is defined for anything but a message.
            _ => None,
        })
    }

    /// Whether a user is a party to the conversation a message is in.
    ///
    /// Without this a stranger's reaction would be stored, relayed onward to
    /// everybody in the conversation, and rendered under the bubble.
    fn may_interact_with(&self, actor: Uuid, context: &TargetContext) -> Result<bool> {
        let my_id = self.store.my_user_id()?;

        Ok(match context.group {
            Some(group) => match self.store.group(group)? {
                Some(group) => group.member_ids().contains(&actor),
                None => false,
            },
            // Either side of the conversation, and nobody else.
            None => actor == my_id || actor == context.destination,
        })
    }

    /// Whether *we* may delete this message, and if so whether as an admin.
    ///
    /// `Some(false)` means as the author, `Some(true)` as an admin, `None` not
    /// at all. Authorship is preferred when both apply, because it is the
    /// narrower claim and needs no group membership to verify.
    fn delete_permission(&self, actor: Uuid, context: &TargetContext) -> Option<bool> {
        let now = crate::now();

        // A note to self has no window. Deleting one is deleting your own data
        // on your own devices, which is not a withdrawal from anybody. Signal
        // excludes note-to-self from delete-for-everyone entirely; here the
        // frame is sync-scoped and genuinely useful, so it is allowed instead.
        let sync_only = context.scope == Scope::Sync;

        if actor == context.author
            && (sync_only || within_delete_window(context.written_at, now, false, false))
        {
            return Some(false);
        }

        if let Some(group) = context.group {
            let is_admin = self
                .store
                .group(group)
                .ok()
                .flatten()
                .map(|group| group.admin_ids().contains(&actor))
                .unwrap_or(false);

            if is_admin && within_delete_window(context.written_at, now, true, false) {
                return Some(true);
            }
        }

        None
    }

    /// The same judgement for a deletion that arrived from somebody else.
    ///
    /// Deliberately more permissive on time than [`Self::delete_permission`]:
    /// the frame may have spent a day in a reference queue waiting for this
    /// device to come back, and rejecting it on arrival would make deletion
    /// unreliable in precisely the case the person cares about most.
    fn delete_permission_for_peer(
        &self,
        actor: Uuid,
        claimed_admin: bool,
        context: &TargetContext,
    ) -> bool {
        let now = crate::now();
        let sync_only = context.scope == Scope::Sync;

        if !claimed_admin {
            return actor == context.author
                && (sync_only || within_delete_window(context.written_at, now, false, true));
        }

        // An admin claim is checked against our own view of the group, not
        // against the claim. Otherwise "AdminDelete: true" would be a licence
        // for anybody to delete anything.
        let Some(group) = context.group else {
            return false;
        };
        let is_admin = self
            .store
            .group(group)
            .ok()
            .flatten()
            .map(|group| group.admin_ids().contains(&actor))
            .unwrap_or(false);

        is_admin && within_delete_window(context.written_at, now, true, true)
    }
}

/// The reaction each person is currently standing behind.
///
/// The table is a log of frames, so this is where "one reaction per person"
/// actually happens. Later beats earlier, and **a withdrawal beats a reaction
/// of the same age** — timestamps are whole seconds, so a reaction and the
/// withdrawal that follows it routinely share one, and letting arrival order
/// decide is how a reaction somebody took back comes back.
///
/// Returns nothing for a person whose latest frame is a withdrawal.
pub fn resolve_reactions(frames: impl Iterator<Item = Reaction>) -> Vec<Reaction> {
    let mut standing: Vec<Reaction> = Vec::new();

    for frame in frames {
        match standing.iter_mut().find(|held| held.actor == frame.actor) {
            Some(held) => {
                let newer = frame.timestamp > held.timestamp;
                let withdraws_a_tie =
                    frame.timestamp == held.timestamp && frame.emoji.is_empty();
                if newer || withdraws_a_tie {
                    *held = frame;
                }
            }
            None => standing.push(frame),
        }
    }

    standing.retain(|frame| !frame.emoji.is_empty());
    standing
}

/// Fold a message's reactions into one entry per emoji, in first-use order.
///
/// A free function so both the per-message path and the batched snapshot path
/// share it — those two disagreeing is the kind of bug that only shows up after
/// a restart, when the timeline is built the other way.
pub(super) fn group_reactions(
    reactions: impl Iterator<Item = Reaction>,
    my_id: Uuid,
) -> Vec<ReactionView> {
    let mut grouped: Vec<ReactionView> = Vec::new();

    for reaction in resolve_reactions(reactions) {
        // A parked reaction has no scope yet, which means we have not been able
        // to check that its author is in the conversation. Showing it would put
        // a stranger's emoji under a bubble.
        if reaction.destination.is_nil() && reaction.scope == Scope::Sync.as_i64() {
            continue;
        }

        match grouped.iter_mut().find(|entry| entry.emoji == reaction.emoji) {
            Some(entry) => {
                entry.users.push(reaction.actor);
                entry.mine |= reaction.actor == my_id;
            }
            None => grouped.push(ReactionView {
                emoji: reaction.emoji.clone(),
                users: vec![reaction.actor],
                mine: reaction.actor == my_id,
            }),
        }
    }

    grouped
}

/// Render a stored quote for the client, resolving whether it has expired.
///
/// The expiry is decided here rather than in the client so that "expired" means
/// the same thing everywhere, and so a client rendering a stale snapshot cannot
/// show an excerpt the sweep has already decided is past.
pub(super) fn quote_view(quote: Option<&Quote>) -> Option<QuoteView> {
    let quote = quote?;
    let expired = quote.has_expired(crate::now());

    Some(QuoteView {
        target: quote.target,
        author: quote.author,
        text: if expired { String::new() } else { quote.text.clone() },
        kind: match quote.kind() {
            QuoteKind::Image => "image".into(),
            QuoteKind::File => "file".into(),
            QuoteKind::Text => "text".into(),
        },
        expired,
    })
}

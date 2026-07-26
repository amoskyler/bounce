//! Making disappearing messages disappear.
//!
//! Retention is the one control in the product where implementing half of it
//! is worse than implementing none. Both ends agree a policy, both ends stamp
//! `delete_at` on every message they store, and the interface renders a timer
//! on each bubble counting down to it — while the plaintext sits in
//! `bounce.db` forever. The user is not merely unserved, they are told a
//! specific untruth about what was destroyed. This is what destroys it.
//!
//! ## Why a sweep, rather than a timer per message
//!
//! Go does both. It batch-deletes whatever has already expired when the
//! database is opened (`chat/database.go:161`, `:209`) and then spawns a
//! goroutine per future expiry that sleeps until the moment and deletes the
//! row (`chat/database.go:170-177`, `chat/direct_message.go:619-637`).
//!
//! The goroutines are the half not worth porting. They do not survive a
//! restart, which is why the batch delete has to exist at all; they scale with
//! the size of the timeline rather than with what is actually expiring; and
//! nothing in the interface can tell the difference, because the shortest
//! retention the product offers is an hour (`ui/retention.go:14`) and the
//! bubble timer is rendered in minutes. So this is the batch delete alone, run
//! often enough that the difference stays invisible.
//!
//! ## Deleting is not enough by itself
//!
//! Every frame is gossiped, and a peer that has not caught up re-offers what it
//! still holds on its next connection. A sweep with no matching refusal would
//! produce a message that vanishes and comes back, indefinitely — so the pair
//! is what makes an expiry final. [`Engine::already_gone`] is that refusal, and
//! it is the same check Go makes on the way in (`chat/direct_message.go:236`
//! for expiry, `:226` for cleared history).
//!
//! ## Clearing history is swept here too
//!
//! `clear_before` is shared conversation state that arrives over the wire, so
//! the cutoff routinely lands on a device long after the messages it covers
//! were stored — and for groups it arrives through consensus, which folds it
//! into the group record and deletes nothing at all. Sweeping it on the same
//! pass is what makes "clear history" mean the same thing on every device
//! rather than only on the one the button was pressed on.

use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use crate::error::Result;
use crate::net::Network;

use super::{Engine, Event};

/// The longest the sweep will sleep when nothing is due sooner.
///
/// Also the cadence for the undeliverable pass, which is measured in weeks and
/// does not care. The cost is two scans of the message tables a minute at the
/// very most, against a database whose write volume is a chat application's.
const SWEEP_INTERVAL: Duration = Duration::from_secs(30);

/// The shortest, so a burst of expiries cannot turn the sweep into a spin.
const MINIMUM_SWEEP_INTERVAL: Duration = Duration::from_secs(1);

impl<N: Network + 'static> Engine<N> {
    /// Sweep, then keep sweeping, for as long as the engine runs.
    ///
    /// Spawned once at start-up beside [`Engine::run_typing_expiry`]. The first
    /// pass is immediate: it is the start-up prune, catching everything that
    /// expired while the client was closed.
    pub async fn run_retention(self: Arc<Self>) {
        loop {
            if let Err(error) = self.sweep_expired() {
                // Never fatal. A sweep that fails leaves messages that should
                // be gone, which is bad, but a client that stops running
                // because of it leaves all of them.
                tracing::warn!(%error, "could not sweep expired messages");
            }
            if let Err(error) = self.sweep_undeliverable() {
                tracing::warn!(%error, "could not mark undeliverable messages");
            }
            tokio::time::sleep(self.until_next_sweep()).await;
        }
    }

    /// How long to wait before sweeping again.
    ///
    /// Long by default, but never past the next message due to disappear. A
    /// thirty-second timer that is only noticed on a thirty-second cadence
    /// spends up to half its visible life reading zero, which makes the clock
    /// on the message a lie at exactly the lengths where somebody is watching
    /// it. Bounded below so a thread full of simultaneous expiries settles at
    /// one pass a second rather than spinning.
    fn until_next_sweep(&self) -> Duration {
        let Ok(Some(next)) = self.store.next_expiry() else {
            return SWEEP_INTERVAL;
        };

        let remaining = next - crate::now();
        if remaining <= 0 {
            return MINIMUM_SWEEP_INTERVAL;
        }

        Duration::from_secs(remaining as u64)
            .clamp(MINIMUM_SWEEP_INTERVAL, SWEEP_INTERVAL)
    }

    /// Give up on messages nobody has acknowledged.
    ///
    /// This shares the retention loop because it is the same shape of job —
    /// something that becomes true with the passage of time, and so needs a
    /// timer to notice. Without a caller,
    /// [`Engine::mark_stale_messages_undeliverable`] was a method the tests
    /// exercised and the product never ran, so the failed tick mark it exists
    /// to raise was never shown to anybody. It emits the events itself.
    pub fn sweep_undeliverable(&self) -> Result<Vec<Uuid>> {
        let marked = self.mark_stale_messages_undeliverable()?;
        if !marked.is_empty() {
            tracing::info!(count = marked.len(), "gave up delivering messages");
        }
        Ok(marked)
    }

    /// One pass: delete everything past its expiry or its thread's cutoff.
    ///
    /// Returns what was removed, and emits [`Event::MessageDeleted`] for each
    /// so an attached client drops the bubble without re-reading the thread.
    /// Separated from [`Engine::run_retention`] so `initial_state` can prune
    /// before it builds a snapshot, and so tests can step it deterministically.
    pub fn sweep_expired(&self) -> Result<Vec<Uuid>> {
        let mut deleted = self.store.delete_expired_messages(crate::now())?;

        // Only for a profile that exists: `delete_messages_before` derives the
        // thread from our own ID, and a device that has not been set up yet
        // has no messages to sweep anyway.
        if self.store.my_user_id().is_ok() {
            for user in self.store.all_users()? {
                if user.clear_before > 0 {
                    deleted.extend(
                        self.store
                            .delete_messages_before(user.id, user.clear_before)?,
                    );
                }
            }
            for group in self.store.all_groups()? {
                if group.clear_before > 0 {
                    deleted.extend(
                        self.store
                            .delete_messages_before(group.id, group.clear_before)?,
                    );
                }
            }
        }

        if !deleted.is_empty() {
            tracing::info!(count = deleted.len(), "swept messages that are past");
            for message_id in &deleted {
                self.emit(Event::MessageDeleted {
                    message_id: *message_id,
                });
            }
        }
        Ok(deleted)
    }

    /// Whether an arriving message is one we should never store: already past
    /// its own expiry, or written before its thread's history was cleared.
    ///
    /// Storing it would undo both features on the next gossip round, since the
    /// sweep would delete it and the peer that offered it would offer it again.
    /// `thread` is the group ID for a group message, or the counterparty for a
    /// direct one — the same key `delete_messages_before` takes.
    pub(super) fn already_gone(&self, thread: Uuid, written_at: i64, delete_at: i64) -> Result<bool> {
        // Zero means "kept indefinitely", not "expired at the epoch".
        if delete_at != 0 && delete_at <= crate::now() {
            return Ok(true);
        }

        let cleared = match self.store.group(thread)? {
            Some(group) => group.clear_before,
            // A thread we have no record of has cleared nothing. Notes to self
            // resolve to our own user row, which carries its own cutoff.
            None => self
                .store
                .user(thread)?
                .map(|user| user.clear_before)
                .unwrap_or(0),
        };

        // Strictly before, matching the cutoff the deletes use: a message
        // written in the same second as the clear is on the near side of it.
        Ok(cleared > 0 && written_at < cleared)
    }
}

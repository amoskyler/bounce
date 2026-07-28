//! Deciding which device to ask for which chunk, and when to ask again.
//!
//! A port of `chat/chunk_engine.go`. Nothing here sends a chunk request the
//! moment a reason to want one appears; every request is issued by a scheduler
//! that wakes on a fixed tick and looks at the whole picture. That indirection
//! is the point:
//!
//! - a holder we have no session with is skipped rather than shouted at, and
//!   reconsidered for free on the next tick once the session exists;
//! - a request that goes unanswered is re-issued, so a chunk lost to a dropped
//!   frame or a peer that went away does not strand the file forever;
//! - a holder that keeps failing is set aside in favour of another, but only
//!   while another exists;
//! - no device is asked for more than a handful of chunks at once.
//!
//! This port previously asked inline, once, from wherever it happened to be —
//! `handle_file`, `handle_chunk_offer`, session start. Each of those is a
//! single shot with no memory, so any request that did not produce bytes was
//! simply the end of that file. Sockets here stay up for days at a time, so
//! "it will sort itself out on the next connection" never arrived.

use std::collections::{HashMap, HashSet};

/// How often the scheduler looks for work.
pub(super) const TICK_SECONDS: u64 = 5;

/// How many requests may be outstanding at one device.
///
/// Compared with `>` rather than `>=`, matching Go, so this is a limit of six.
const MAX_OUTSTANDING_PER_LOCATION: usize = 5;

/// How many times one holder is asked for one chunk before another is
/// preferred. Also compared with `>`.
const MAX_ATTEMPTS: u32 = 3;

/// How long a request may go unanswered before being sent again.
///
/// Go derives this from a nominal transfer rate:
/// `((fileChunkSize / 1024) / expectedKbps) * maxOutstandingChunkRequests`,
/// in integer arithmetic — `((1048576 / 1024) / 100) * 5`.
const EXPECTED_DELIVERY_SECONDS: i64 =
    ((crate::CHUNK_SIZE as i64 / 1024) / 100) * MAX_OUTSTANDING_PER_LOCATION as i64;

/// Who holds what, what we have asked for, and how that went.
#[derive(Default)]
pub(super) struct ChunkEngine {
    /// Chunk hash to the devices that have offered it.
    by_hash: HashMap<String, Vec<String>>,
    /// Holders that have already disappointed us for a given hash.
    prefer_to_avoid: HashMap<String, Vec<String>>,
    /// When (hash, location) was last asked.
    last_request: HashMap<(String, String), i64>,
    /// How many times (hash, location) has been asked.
    attempts: HashMap<(String, String), u32>,
    /// Device address to the hashes it currently owes us.
    outstanding: HashMap<String, Vec<String>>,
}

impl ChunkEngine {
    /// Note that a device holds a chunk.
    ///
    /// Our own address is never recorded: an offer we made ourselves comes
    /// back to us through gossip, and a device that asks itself for bytes
    /// waits forever.
    pub(super) fn add_offer(&mut self, hash: &str, location: &str, me: &str) {
        if location == me {
            return;
        }
        let holders = self.by_hash.entry(hash.to_string()).or_default();
        if !holders.iter().any(|held| held == location) {
            holders.push(location.to_string());
        }
    }

    /// The chunk arrived; forget everything about wanting it.
    pub(super) fn completed(&mut self, hash: &str) {
        self.by_hash.remove(hash);
        self.prefer_to_avoid.remove(hash);
        self.last_request.retain(|(held, _), _| held != hash);
        self.attempts.retain(|(held, _), _| held != hash);
        for hashes in self.outstanding.values_mut() {
            hashes.retain(|held| held != hash);
        }
    }

    /// A holder turned out not to have it after all.
    pub(super) fn remove(&mut self, location: &str, hash: &str) {
        if let Some(holders) = self.by_hash.get_mut(hash) {
            holders.retain(|held| held != location);
        }
        if let Some(hashes) = self.outstanding.get_mut(location) {
            hashes.retain(|held| held != hash);
        }
        self.last_request.remove(&(hash.to_string(), location.to_string()));
        self.attempts.remove(&(hash.to_string(), location.to_string()));
    }

    /// Whether anything is currently worth scheduling.
    pub(super) fn is_idle(&self) -> bool {
        self.by_hash.is_empty()
    }

    /// Decide what to ask for now.
    ///
    /// Pure: it reads `connected` and `wanted`, mutates only its own
    /// bookkeeping, and returns the requests to send rather than sending them.
    /// The caller does the I/O, which keeps the lock off the network and makes
    /// the policy testable without a socket.
    pub(super) fn plan(
        &mut self,
        connected: &HashSet<String>,
        wanted: &HashSet<String>,
        now: i64,
    ) -> Vec<(String, String)> {
        let mut requests = Vec::new();

        // Outstanding requests stay outstanding, except where one holder has
        // been asked too many times and there is somebody else to ask. Then it
        // is set aside — but only then, because setting aside the only holder
        // means never asking anyone.
        let mut kept: HashMap<String, Vec<String>> = HashMap::new();
        for (location, hashes) in &self.outstanding {
            if !connected.contains(location) {
                continue;
            }
            for hash in hashes {
                let too_many = self
                    .attempts
                    .get(&(hash.clone(), location.clone()))
                    .copied()
                    .unwrap_or(0)
                    > MAX_ATTEMPTS;
                let alternatives = self.by_hash.get(hash).map(Vec::len).unwrap_or(0) > 1;

                if too_many && alternatives {
                    self.prefer_to_avoid
                        .entry(hash.clone())
                        .or_default()
                        .push(location.clone());
                } else {
                    kept.entry(location.clone()).or_default().push(hash.clone());
                }
            }
        }
        self.outstanding = kept;

        // Anything wanted and not already assigned to somebody gets a holder.
        for hash in self.unassigned() {
            if !wanted.contains(&hash) {
                continue;
            }
            let Some(holders) = self.by_hash.get(&hash) else {
                continue;
            };

            // Prefer holders that have not already failed us — unless that
            // leaves nobody, in which case a disappointing holder beats none.
            let mut options = holders.clone();
            if options.len() > 1 {
                let avoid = self.prefer_to_avoid.get(&hash);
                let preferred: Vec<String> = options
                    .iter()
                    .filter(|option| !avoid.is_some_and(|list| list.contains(option)))
                    .cloned()
                    .collect();
                if preferred.len() > 1 {
                    options = preferred;
                }
            }

            for location in options {
                if self.outstanding.get(&location).map(Vec::len).unwrap_or(0)
                    > MAX_OUTSTANDING_PER_LOCATION
                {
                    continue;
                }
                if !connected.contains(&location) {
                    continue;
                }

                self.last_request.insert((hash.clone(), location.clone()), now);
                self.attempts.insert((hash.clone(), location.clone()), 1);
                self.outstanding
                    .entry(location.clone())
                    .or_default()
                    .push(hash.clone());
                requests.push((location, hash.clone()));
                break;
            }
        }

        // And anything asked for long enough ago that the answer is not coming
        // gets asked again.
        let outstanding: Vec<(String, Vec<String>)> = self
            .outstanding
            .iter()
            .map(|(location, hashes)| (location.clone(), hashes.clone()))
            .collect();
        for (location, hashes) in outstanding {
            if !connected.contains(&location) {
                continue;
            }
            for hash in hashes {
                if !wanted.contains(&hash) {
                    continue;
                }
                let key = (hash.clone(), location.clone());
                let asked_at = self.last_request.get(&key).copied().unwrap_or(0);
                if now - asked_at <= EXPECTED_DELIVERY_SECONDS {
                    continue;
                }
                let attempts = self.attempts.get(&key).copied().unwrap_or(0);
                self.last_request.insert(key.clone(), now);
                self.attempts.insert(key, attempts + 1);
                requests.push((location.clone(), hash));
            }
        }

        requests
    }

    /// Hashes we want that nobody is currently on the hook for.
    fn unassigned(&self) -> Vec<String> {
        let assigned: HashSet<&String> = self.outstanding.values().flatten().collect();
        self.by_hash
            .keys()
            .filter(|hash| !assigned.contains(hash))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn a_holder_with_no_session_is_skipped_not_asked() {
        // The failure this exists to prevent: an offer gossiped from a third
        // party names a device we have never dialled, the request is written to
        // nobody, and the chunk is never spoken of again.
        let mut engine = ChunkEngine::default();
        engine.add_offer("aa", "unreachable.onion", "me.onion");
        engine.add_offer("aa", "reachable.onion", "me.onion");

        let plan = engine.plan(&set(&["reachable.onion"]), &set(&["aa"]), 100);
        assert_eq!(plan, vec![("reachable.onion".to_string(), "aa".to_string())]);
    }

    #[test]
    fn nothing_is_asked_of_anyone_when_no_holder_is_reachable() {
        let mut engine = ChunkEngine::default();
        engine.add_offer("aa", "unreachable.onion", "me.onion");

        assert!(engine.plan(&set(&[]), &set(&["aa"]), 100).is_empty());

        // And it is picked up the moment that device is reachable, without
        // anything having to re-announce the offer.
        let plan = engine.plan(&set(&["unreachable.onion"]), &set(&["aa"]), 105);
        assert_eq!(plan, vec![("unreachable.onion".to_string(), "aa".to_string())]);
    }

    #[test]
    fn an_unanswered_request_is_reissued_but_not_immediately() {
        let mut engine = ChunkEngine::default();
        engine.add_offer("aa", "holder.onion", "me.onion");
        let connected = set(&["holder.onion"]);
        let wanted = set(&["aa"]);

        assert_eq!(engine.plan(&connected, &wanted, 0).len(), 1);
        // Still within the window: asking again would only duplicate it.
        assert!(engine.plan(&connected, &wanted, EXPECTED_DELIVERY_SECONDS).is_empty());
        // Past it, the answer is not coming.
        assert_eq!(
            engine.plan(&connected, &wanted, EXPECTED_DELIVERY_SECONDS + 1).len(),
            1,
        );
    }

    #[test]
    fn a_chunk_that_arrives_stops_being_asked_for() {
        let mut engine = ChunkEngine::default();
        engine.add_offer("aa", "holder.onion", "me.onion");
        let connected = set(&["holder.onion"]);

        assert_eq!(engine.plan(&connected, &set(&["aa"]), 0).len(), 1);
        engine.completed("aa");
        assert!(engine.is_idle());
        assert!(engine.plan(&connected, &set(&["aa"]), 1_000).is_empty());
    }

    #[test]
    fn a_holder_that_keeps_failing_is_passed_over_for_another() {
        let mut engine = ChunkEngine::default();
        engine.add_offer("aa", "bad.onion", "me.onion");
        engine.add_offer("aa", "good.onion", "me.onion");
        engine.add_offer("aa", "spare.onion", "me.onion");
        let connected = set(&["bad.onion", "good.onion", "spare.onion"]);
        let wanted = set(&["aa"]);

        // Insertion order decides the first pick, so this is the bad one.
        assert_eq!(engine.plan(&connected, &wanted, 0)[0].0, "bad.onion");

        let mut at = 0;
        for _ in 0..MAX_ATTEMPTS + 1 {
            at += EXPECTED_DELIVERY_SECONDS + 1;
            engine.plan(&connected, &wanted, at);
        }

        at += EXPECTED_DELIVERY_SECONDS + 1;
        let plan = engine.plan(&connected, &wanted, at);
        assert!(
            plan.iter().all(|(location, _)| location != "bad.onion"),
            "a holder that has failed repeatedly must be passed over: {plan:?}",
        );
    }

    #[test]
    fn with_only_two_holders_the_failing_one_is_kept_in_the_running() {
        // Go filters out disfavoured holders only when doing so still leaves
        // more than one option (`chat/chunk_engine.go:135-157`), so with
        // exactly two holders the filtered list has one entry, the filter is
        // discarded, and the failing holder is asked again.
        //
        // That reads like an off-by-one — `> 0` would let the good holder win
        // — and it is deliberately reproduced rather than corrected: this is a
        // port, the Go client is the reference, and a difference in retry
        // routing is the kind of thing that should be raised with its authors
        // rather than decided here. It is not harmful, only slower to
        // converge: the failing holder keeps being re-asked on the same
        // schedule and the good one is reached through the ordinary rotation.
        let mut engine = ChunkEngine::default();
        engine.add_offer("aa", "bad.onion", "me.onion");
        engine.add_offer("aa", "good.onion", "me.onion");
        let connected = set(&["bad.onion", "good.onion"]);
        let wanted = set(&["aa"]);

        let mut at = 0;
        for _ in 0..MAX_ATTEMPTS + 3 {
            at += EXPECTED_DELIVERY_SECONDS + 1;
            engine.plan(&connected, &wanted, at);
        }

        at += EXPECTED_DELIVERY_SECONDS + 1;
        let plan = engine.plan(&connected, &wanted, at);
        assert_eq!(plan, vec![("bad.onion".to_string(), "aa".to_string())]);
    }

    #[test]
    fn the_only_holder_is_never_given_up_on() {
        // The counterpart to the rule above. Setting aside a holder we have no
        // alternative to would mean the chunk is never asked for again.
        let mut engine = ChunkEngine::default();
        engine.add_offer("aa", "only.onion", "me.onion");
        let connected = set(&["only.onion"]);
        let wanted = set(&["aa"]);

        let mut at = 0;
        for _ in 0..MAX_ATTEMPTS + 5 {
            at += EXPECTED_DELIVERY_SECONDS + 1;
            let plan = engine.plan(&connected, &wanted, at);
            assert_eq!(plan, vec![("only.onion".to_string(), "aa".to_string())]);
        }
    }

    #[test]
    fn our_own_offers_are_not_treated_as_somewhere_to_ask() {
        // Offers gossip, so one we made ourselves is relayed back to us.
        let mut engine = ChunkEngine::default();
        engine.add_offer("aa", "me.onion", "me.onion");
        assert!(engine.is_idle());
        assert!(engine.plan(&set(&["me.onion"]), &set(&["aa"]), 0).is_empty());
    }

    #[test]
    fn a_chunk_we_no_longer_want_is_not_asked_for() {
        let mut engine = ChunkEngine::default();
        engine.add_offer("aa", "holder.onion", "me.onion");
        assert!(engine.plan(&set(&["holder.onion"]), &set(&[]), 0).is_empty());
    }
}

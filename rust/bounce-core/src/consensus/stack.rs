//! The canonical stack: deterministic group state from unordered updates.
//!
//! Every device must reach the same group state from the same set of updates,
//! no matter what order they arrived in and no matter that timestamps come from
//! clocks nobody controls. The stack is how that is achieved.
//!
//! Updates are applied in timestamp order on top of the group's creation state.
//! When an update is not permitted against the current top of the stack, it is
//! either dropped or — if a majority of the group's users have confirmed it —
//! treated as evidence that something already on the stack is the real problem.
//!
//! ## The attack this defends against
//!
//! Timestamps alone are trivially forgeable:
//!
//! 1. Group G has admins A and B.
//! 2. A revokes B's admin rights and broadcasts the update.
//! 3. B sees it and broadcasts a *backdated* update revoking A instead.
//!
//! Ordering purely by timestamp hands the win to B every time, and no device
//! can tell that B lied. So devices broadcast a [`Confirmation`] — a signature
//! — for each valid update they see. When two updates conflict, the earlier one
//! still wins **unless** the later one has majority confirmation and the earlier
//! one does not.
//!
//! [`Confirmation`]: crate::frames::group::Confirmation
//!
//! That does not make the race disappear, but it changes who wins it: if A's
//! revocation reaches a majority of the group before B's forgery circulates, B
//! can never undo it. Either way every device converges on the same answer.

use uuid::Uuid;

use super::state::{apply_update, change_is_noop, state_change_allowed, GroupState};
use crate::error::{Error, Result};
use crate::frames::group::UpdateGroup;

/// A group's accepted history, as a stack of states.
///
/// The bottom of the stack is the founding state; each entry above it is the
/// state produced by one accepted update.
#[derive(Debug, Clone)]
pub struct CanonicalStack {
    my_id: Uuid,
    /// Device address to owning user, needed to check that an update really was
    /// signed by the user it claims to act for.
    address_map: std::collections::HashMap<String, Uuid>,
    /// Device address to revocation time, so updates signed after a device was
    /// revoked can be rejected.
    revoked_map: std::collections::HashMap<String, i64>,
    history: Vec<GroupState>,
    /// A snapshot taken before speculative unwinding, so a failed attempt can
    /// be rolled back rather than leaving the history damaged.
    stash: Option<Vec<GroupState>>,
}

impl CanonicalStack {
    pub fn new(
        initial_state: GroupState,
        address_map: std::collections::HashMap<String, Uuid>,
        revoked_map: std::collections::HashMap<String, i64>,
        my_id: Uuid,
    ) -> Self {
        CanonicalStack {
            my_id,
            address_map,
            revoked_map,
            history: vec![initial_state],
            stash: None,
        }
    }

    /// The current group state.
    pub fn top(&self) -> Result<&GroupState> {
        self.history.last().ok_or(Error::StackEmpty)
    }

    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }

    /// How many updates have been accepted.
    pub fn accepted_update_count(&self) -> usize {
        self.history.len().saturating_sub(1)
    }

    /// Every accepted update, in order.
    pub fn accepted_updates(&self) -> Vec<UpdateGroup> {
        self.history
            .iter()
            .filter_map(|state| state.update.as_ref().map(|u| (**u).clone()))
            .collect()
    }

    /// Register a device so its signatures can be attributed and checked.
    pub fn register_device(&mut self, address: String, user_id: Uuid, revoked_at: i64) {
        self.address_map.insert(address.clone(), user_id);
        self.revoked_map.insert(address, revoked_at);
    }

    /// Push an update, assuming it has already been permitted.
    fn push(&mut self, update: &UpdateGroup) -> Result<()> {
        let top = self.top()?.clone();
        let mut next = apply_update(&top, update, self.my_id)?;

        // Record the update that blocked or removed us, so the interface can
        // say who did it. These markers are carried forward, not recomputed.
        if !top.is_blocked(self.my_id) && next.is_blocked(self.my_id) {
            next.blocked_by = Some(Box::new(update.clone()));
        }

        if next.is_member(self.my_id) || next.is_invited(self.my_id) {
            next.removed_by = None;
        } else if top.is_member(self.my_id) || top.is_invited(self.my_id) {
            next.removed_by = Some(Box::new(update.clone()));
        }

        if !top.is_member(self.my_id) && next.is_member(self.my_id) {
            next.accepted_at = update.timestamp;
        }

        self.history.push(next);
        Ok(())
    }

    /// Remove and return the most recent accepted update.
    fn pop(&mut self) -> Result<UpdateGroup> {
        let state = self.history.pop().ok_or(Error::StackEmpty)?;
        state
            .update
            .map(|u| *u)
            .ok_or(Error::InvalidFrame("popped the initial state".into()))
    }

    fn stash(&mut self) {
        self.stash = Some(self.history.clone());
    }

    fn restore(&mut self) {
        if let Some(stashed) = self.stash.take() {
            self.history = stashed;
        }
    }

    /// Insert an update into the history, resolving any conflict it exposes.
    ///
    /// Updates must be offered in timestamp order; an out-of-order update is
    /// rejected rather than silently reordering accepted history.
    pub fn insert(&mut self, update: &UpdateGroup) {
        self.insert_with_depth(update, 0);
    }

    /// Recursion depth is bounded so that a pathological set of mutually
    /// conflicting updates cannot drive the reconciliation into a stack
    /// overflow.
    fn insert_with_depth(&mut self, update: &UpdateGroup, depth: usize) {
        const MAX_DEPTH: usize = 64;
        if depth > MAX_DEPTH {
            tracing::warn!(
                update_id = %update.id,
                "abandoning update insertion: conflict resolution recursed too deeply"
            );
            return;
        }

        // An invitation carries the invitee's device group, so register it
        // before checking the signature — the signer may be one of those very
        // devices.
        if matches!(update.kind(), Ok(crate::types::UpdateGroupType::InviteUser)) {
            if let Ok(user) = crate::msgpack::from_slice::<crate::frames::identity::User>(&update.data)
            {
                for device in &user.devices {
                    self.register_device(device.address.clone(), user.id, device.revoked_at);
                }
            }
        }

        // The update must have been signed by a device belonging to the user it
        // claims to act for.
        match self.address_map.get(&update.signed.signer) {
            Some(owner) if *owner == update.actor => {}
            _ => {
                tracing::warn!(
                    update_id = %update.id,
                    actor = %update.actor,
                    signer = %update.signed.signer,
                    "rejecting update group not signed by its actor"
                );
                return;
            }
        }

        // A device cannot act after it has been revoked.
        if let Some(&revoked_at) = self.revoked_map.get(&update.signed.signer) {
            if revoked_at != 0 && revoked_at < update.timestamp {
                tracing::warn!(
                    update_id = %update.id,
                    signer = %update.signed.signer,
                    "rejecting update group signed by a revoked device"
                );
                return;
            }
        }

        if !update.has_valid_payload() {
            tracing::warn!(update_id = %update.id, "rejecting update group with invalid payload");
            return;
        }

        let Ok(last_state) = self.top().cloned() else {
            return;
        };

        // Timestamp ordering is the caller's responsibility; violating it would
        // corrupt the history.
        if self.history.len() > 1 {
            if let Some(previous) = &last_state.update {
                if update.timestamp < previous.timestamp {
                    tracing::error!(
                        update_id = %update.id,
                        "refusing out of order update group"
                    );
                    return;
                }
            }
        }

        // Once we have blocked a group we stop tracking changes to it.
        if last_state.blocked_by.is_some() {
            return;
        }

        if change_is_noop(&last_state, update, self.my_id) {
            return;
        }

        if state_change_allowed(&last_state, update, self.my_id).is_ok() {
            if let Err(error) = self.push(update) {
                tracing::error!(update_id = %update.id, %error, "failed to push permitted update");
            }
            return;
        }

        // The update is not permitted against the current state. If a majority
        // of the group has confirmed it anyway, the fault may lie with
        // something already accepted, so look for the conflict.
        if !update.is_confirmed_by_majority(&last_state.users) {
            return;
        }

        self.resolve_conflict(update, depth);
    }

    /// Unwind the history looking for the update that makes `update`
    /// impermissible, and decide which of the two survives.
    fn resolve_conflict(&mut self, update: &UpdateGroup, depth: usize) {
        let mut unwound: Vec<UpdateGroup> = Vec::new();
        self.stash();

        loop {
            let removed = match self.pop() {
                Ok(removed) => removed,
                Err(_) => {
                    // Ran out of history without finding a conflict: this
                    // update was never permissible.
                    self.restore();
                    return;
                }
            };
            unwound.insert(0, removed);

            if self.is_empty() {
                self.restore();
                return;
            }

            let Ok(new_top) = self.top().cloned() else {
                self.restore();
                return;
            };

            if state_change_allowed(&new_top, update, self.my_id).is_err() {
                // Still blocked; keep unwinding.
                continue;
            }

            // The last thing removed is the conflicting update.
            let conflict = unwound[0].clone();

            if conflict.is_confirmed_by_majority(&new_top.users) {
                // Both are confirmed, so the earlier one wins and this update
                // is discarded.
                self.restore();
                return;
            }

            // The conflict is unconfirmed, so it loses. Drop it, replay
            // everything that came after, then retry this update.
            self.stash = None;
            for replay in unwound.iter().skip(1) {
                self.insert_with_depth(replay, depth + 1);
            }
            self.insert_with_depth(update, depth + 1);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::DeviceKey;
    use crate::frames::group::{Confirmation, UpdateGroup};
    use crate::types::UpdateGroupType;
    use std::collections::HashMap;

    /// A group of test users, each with one device, so tests can talk about
    /// people rather than key material.
    struct Fixture {
        users: Vec<Uuid>,
        keys: Vec<DeviceKey>,
        address_map: HashMap<String, Uuid>,
        revoked_map: HashMap<String, i64>,
    }

    impl Fixture {
        fn new(count: usize) -> Self {
            let mut fixture = Fixture {
                users: Vec::new(),
                keys: Vec::new(),
                address_map: HashMap::new(),
                revoked_map: HashMap::new(),
            };
            for _ in 0..count {
                let user_id = Uuid::new_v4();
                let key = DeviceKey::generate();
                fixture.address_map.insert(key.address(), user_id);
                fixture.revoked_map.insert(key.address(), 0);
                fixture.users.push(user_id);
                fixture.keys.push(key);
            }
            fixture
        }

        /// Build an update signed by user `index`.
        fn update(
            &self,
            index: usize,
            kind: UpdateGroupType,
            data: Vec<u8>,
            timestamp: i64,
        ) -> UpdateGroup {
            let mut update = UpdateGroup::new(self.users[index], Uuid::new_v4(), kind, data, timestamp);
            update.signed.signer = self.keys[index].address();
            update.signed.signature = vec![1; 64];
            update
        }

        /// Add confirmations from the given users.
        fn confirm(&self, update: &mut UpdateGroup, confirmers: &[usize]) {
            for &index in confirmers {
                update.confirmations.push(Confirmation {
                    id: Uuid::new_v4(),
                    update_group_id: update.id,
                    destination: Uuid::nil(),
                    author: self.users[index],
                    custom_scope: Uuid::nil(),
                    signing_device: self.keys[index].address(),
                    signature: vec![2; 64],
                    timestamp: update.timestamp,
                    saved_at: 0,
                });
            }
        }

        fn stack(&self, members: &[usize], admins: &[usize]) -> CanonicalStack {
            let state = GroupState {
                name: "Test Group".into(),
                users: members.iter().map(|&i| self.users[i]).collect(),
                admins: admins.iter().map(|&i| self.users[i]).collect(),
                ..Default::default()
            };
            CanonicalStack::new(
                state,
                self.address_map.clone(),
                self.revoked_map.clone(),
                self.users[0],
            )
        }
    }

    #[test]
    fn a_permitted_update_is_accepted() {
        let fixture = Fixture::new(2);
        let mut stack = fixture.stack(&[0, 1], &[0]);

        let rename = fixture.update(0, UpdateGroupType::ChangeName, b"Renamed".to_vec(), 100);
        stack.insert(&rename);

        assert_eq!(stack.top().unwrap().name, "Renamed");
        assert_eq!(stack.accepted_update_count(), 1);
    }

    #[test]
    fn an_update_signed_by_the_wrong_user_is_rejected() {
        let fixture = Fixture::new(2);
        let mut stack = fixture.stack(&[0, 1], &[0, 1]);

        // Signed by user 1's device but claiming to act as user 0.
        let mut forged = fixture.update(1, UpdateGroupType::ChangeName, b"Forged".to_vec(), 100);
        forged.actor = fixture.users[0];

        stack.insert(&forged);
        assert_eq!(stack.accepted_update_count(), 0);
    }

    #[test]
    fn an_update_from_a_revoked_device_is_rejected() {
        let mut fixture = Fixture::new(2);
        // User 1's device was revoked at t=50.
        let revoked_address = fixture.keys[1].address();
        fixture.revoked_map.insert(revoked_address, 50);

        let mut stack = fixture.stack(&[0, 1], &[0, 1]);

        let after = fixture.update(1, UpdateGroupType::ChangeName, b"Too Late".to_vec(), 100);
        stack.insert(&after);
        assert_eq!(stack.accepted_update_count(), 0);

        // An update signed before the revocation is still honoured.
        let before = fixture.update(1, UpdateGroupType::ChangeName, b"In Time".to_vec(), 10);
        stack.insert(&before);
        assert_eq!(stack.top().unwrap().name, "In Time");
    }

    #[test]
    fn out_of_order_updates_are_refused() {
        let fixture = Fixture::new(2);
        let mut stack = fixture.stack(&[0, 1], &[0]);

        stack.insert(&fixture.update(0, UpdateGroupType::ChangeName, b"Second".to_vec(), 200));
        // An update with an earlier timestamp arriving after cannot be spliced
        // into settled history.
        stack.insert(&fixture.update(0, UpdateGroupType::ChangeName, b"First".to_vec(), 100));

        assert_eq!(stack.top().unwrap().name, "Second");
        assert_eq!(stack.accepted_update_count(), 1);
    }

    #[test]
    fn noop_updates_are_not_recorded() {
        let fixture = Fixture::new(2);
        let mut stack = fixture.stack(&[0, 1], &[0]);

        stack.insert(&fixture.update(0, UpdateGroupType::ChangeName, b"Test Group".to_vec(), 100));
        assert_eq!(stack.accepted_update_count(), 0);
    }

    #[test]
    fn an_unconfirmed_impermissible_update_is_dropped() {
        let fixture = Fixture::new(3);
        // User 1 is a plain member and edits are restricted.
        let mut stack = fixture.stack(&[0, 1, 2], &[0]);
        stack.history[0].editing_restricted = true;

        let overreach = fixture.update(1, UpdateGroupType::ChangeName, b"Hijacked".to_vec(), 100);
        stack.insert(&overreach);

        assert_eq!(stack.top().unwrap().name, "Test Group");
        assert_eq!(stack.accepted_update_count(), 0);
    }

    #[test]
    fn the_backdating_attack_fails_when_the_honest_update_has_majority() {
        // The scenario from the module documentation: admin A revokes admin B,
        // B answers with a backdated revocation of A.
        //
        // Five users, so a majority is three. A's revocation is confirmed by
        // three of them before B's forgery circulates.
        let fixture = Fixture::new(5);
        let (a, b) = (0usize, 1usize);

        let mut stack = fixture.stack(&[0, 1, 2, 3, 4], &[a, b]);

        // A demotes B at t=200, confirmed by a majority.
        let mut a_demotes_b = fixture.update(
            a,
            UpdateGroupType::DemoteAdmin,
            fixture.users[b].as_bytes().to_vec(),
            200,
        );
        fixture.confirm(&mut a_demotes_b, &[0, 2, 3]);
        stack.insert(&a_demotes_b);

        assert!(!stack.top().unwrap().is_admin(fixture.users[b]));

        // B now claims to have demoted A first, at t=100. Because A's update is
        // already settled and B is no longer an admin, B's forgery is not
        // permitted, and B has no confirmations to override it.
        let b_demotes_a = fixture.update(
            b,
            UpdateGroupType::DemoteAdmin,
            fixture.users[a].as_bytes().to_vec(),
            100,
        );
        stack.insert(&b_demotes_a);

        let final_state = stack.top().unwrap();
        assert!(
            final_state.is_admin(fixture.users[a]),
            "the honest admin must survive the backdated attack"
        );
        assert!(!final_state.is_admin(fixture.users[b]));
    }

    #[test]
    fn a_confirmed_update_displaces_an_unconfirmed_conflict() {
        // The mirror image: the update already on the stack is unconfirmed, and
        // a later-arriving update carries majority confirmation. The confirmed
        // one should win.
        let fixture = Fixture::new(5);
        let (a, b) = (0usize, 1usize);

        let mut stack = fixture.stack(&[0, 1, 2, 3, 4], &[a, b]);

        // B demotes A at t=100, with nobody confirming.
        let b_demotes_a = fixture.update(
            b,
            UpdateGroupType::DemoteAdmin,
            fixture.users[a].as_bytes().to_vec(),
            100,
        );
        stack.insert(&b_demotes_a);
        assert!(!stack.top().unwrap().is_admin(fixture.users[a]));

        // A's demotion of B carries majority confirmation, so B's unconfirmed
        // update is unwound and A's is applied in its place.
        let mut a_demotes_b = fixture.update(
            a,
            UpdateGroupType::DemoteAdmin,
            fixture.users[b].as_bytes().to_vec(),
            200,
        );
        fixture.confirm(&mut a_demotes_b, &[0, 2, 3]);
        stack.insert(&a_demotes_b);

        let final_state = stack.top().unwrap();
        assert!(
            final_state.is_admin(fixture.users[a]),
            "the confirmed update should have displaced the unconfirmed one"
        );
        assert!(!final_state.is_admin(fixture.users[b]));
    }

    #[test]
    fn two_confirmed_conflicts_resolve_in_favour_of_the_earlier() {
        let fixture = Fixture::new(5);
        let (a, b) = (0usize, 1usize);

        let mut stack = fixture.stack(&[0, 1, 2, 3, 4], &[a, b]);

        // B demotes A first, and it is confirmed.
        let mut b_demotes_a = fixture.update(
            b,
            UpdateGroupType::DemoteAdmin,
            fixture.users[a].as_bytes().to_vec(),
            100,
        );
        fixture.confirm(&mut b_demotes_a, &[1, 2, 3]);
        stack.insert(&b_demotes_a);

        // A's later update is also confirmed, but the earlier one holds.
        let mut a_demotes_b = fixture.update(
            a,
            UpdateGroupType::DemoteAdmin,
            fixture.users[b].as_bytes().to_vec(),
            200,
        );
        fixture.confirm(&mut a_demotes_b, &[0, 2, 3]);
        stack.insert(&a_demotes_b);

        let final_state = stack.top().unwrap();
        assert!(!final_state.is_admin(fixture.users[a]));
        assert!(
            final_state.is_admin(fixture.users[b]),
            "when both are confirmed the earlier update wins"
        );
    }

    #[test]
    fn devices_are_registered_from_invitations() {
        let fixture = Fixture::new(2);
        let mut stack = fixture.stack(&[0], &[0]);

        // Build an invitee with a device group of their own.
        let invitee_id = Uuid::new_v4();
        let invitee_key = DeviceKey::generate();
        let mut invitee = crate::frames::identity::User::new(invitee_id, "Invitee".into());
        invitee.devices.push(crate::frames::identity::Device::new(
            Uuid::new_v4(),
            invitee_id,
            invitee_key.address(),
            0,
        ));

        let invite = fixture.update(
            0,
            UpdateGroupType::InviteUser,
            crate::msgpack::to_vec(&invitee).unwrap(),
            100,
        );
        stack.insert(&invite);

        let state = stack.top().unwrap();
        assert!(state.is_invited(invitee_id));
        // The invitee's devices are now reachable, so the group can talk to
        // them before they accept.
        assert!(state
            .scope_addresses_with_invites()
            .contains(&invitee_key.address()));
    }

    #[test]
    fn history_converges_regardless_of_arrival_order() {
        // The core guarantee: two devices that see the same updates end up in
        // the same state even if they receive them differently. Both apply in
        // timestamp order, which is what the engine sorts by before inserting.
        let fixture = Fixture::new(3);

        let rename = fixture.update(0, UpdateGroupType::ChangeName, b"Renamed".to_vec(), 100);
        let promote = fixture.update(
            0,
            UpdateGroupType::PromoteAdmin,
            fixture.users[1].as_bytes().to_vec(),
            200,
        );
        let restrict = fixture.update(
            0,
            UpdateGroupType::ChangePostingPermission,
            vec![super::super::state::sentinels::PERMISSION_RESTRICTED],
            300,
        );

        let mut first = fixture.stack(&[0, 1, 2], &[0]);
        for update in [&rename, &promote, &restrict] {
            first.insert(update);
        }

        // A device that received them out of order sorts by timestamp first.
        let mut received = vec![restrict.clone(), rename.clone(), promote.clone()];
        received.sort_by_key(|u| u.timestamp);

        let mut second = fixture.stack(&[0, 1, 2], &[0]);
        for update in &received {
            second.insert(update);
        }

        let (a, b) = (first.top().unwrap(), second.top().unwrap());
        assert!(a.is_equivalent_to(b));
        assert_eq!(a.name, "Renamed");
        assert!(a.is_admin(fixture.users[1]));
        assert!(a.posting_restricted);
    }

    #[test]
    fn blocking_freezes_further_changes() {
        let fixture = Fixture::new(2);
        let mut stack = fixture.stack(&[0, 1], &[0, 1]);

        // We block the group.
        stack.insert(&fixture.update(0, UpdateGroupType::Block, vec![], 100));
        assert!(stack.top().unwrap().blocked_by.is_some());

        // Nothing after that is tracked.
        stack.insert(&fixture.update(1, UpdateGroupType::ChangeName, b"Ignored".to_vec(), 200));
        assert_eq!(stack.top().unwrap().name, "Test Group");
    }

    #[test]
    fn removal_is_attributed_to_the_update_that_caused_it() {
        let fixture = Fixture::new(2);
        // We are user 0; user 1 is the admin who removes us.
        let mut stack = fixture.stack(&[0, 1], &[1]);

        let removal = fixture.update(
            1,
            UpdateGroupType::RemoveUser,
            fixture.users[0].as_bytes().to_vec(),
            100,
        );
        stack.insert(&removal);

        let state = stack.top().unwrap();
        assert!(!state.is_member(fixture.users[0]));
        assert_eq!(
            state.removed_by.as_ref().map(|u| u.actor),
            Some(fixture.users[1])
        );
    }

    #[test]
    fn accepted_updates_are_reported_in_order() {
        let fixture = Fixture::new(2);
        let mut stack = fixture.stack(&[0, 1], &[0]);

        let first = fixture.update(0, UpdateGroupType::ChangeName, b"One".to_vec(), 100);
        let second = fixture.update(0, UpdateGroupType::ChangeName, b"Two".to_vec(), 200);
        stack.insert(&first);
        stack.insert(&second);

        let accepted = stack.accepted_updates();
        assert_eq!(accepted.len(), 2);
        assert_eq!(accepted[0].id, first.id);
        assert_eq!(accepted[1].id, second.id);
    }
}

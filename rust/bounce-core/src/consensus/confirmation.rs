//! Minting and vetting the signatures that decide a conflict.
//!
//! [`stack`](super::stack) describes what confirmations are *for*: they are the
//! evidence that lets a later update displace an earlier, backdated one. That
//! only works if devices actually produce them, which is what this module is —
//! the rule for when this device owes a signature, what that signature covers,
//! and the checks a peer's signature has to pass before it is allowed to count
//! towards a majority.
//!
//! The rules follow the Go implementation: `sendConfirmation`
//! (`chat/confirmation.go:201`) for the frame, the block at
//! `chat/consensus_store.go:249-259` for when one is minted, and
//! `handleConfirmation` (`chat/confirmation.go:90`) for the inbound checks.
//! Nothing here touches storage or the network — a confirmation is decided on
//! and checked here, then saved and broadcast by the engine.

use uuid::Uuid;

use super::stack::CanonicalStack;
use crate::crypto::{self, DeviceKey};
use crate::error::{Error, Result};
use crate::frames::group::{Confirmation, Group, UpdateGroup};
use crate::types::UpdateGroupType;

/// Sign an update, attesting that this device saw it and considered it valid.
///
/// The signature covers the update's raw ID bytes rather than a digest of an
/// encoded frame, because a confirmation is an assertion *about* another frame:
/// the ID is already the thing every device agrees on. Go signs `ug.ID[:]`
/// (`chat/confirmation.go:211`), and the bytes are identical on both sides.
pub fn mint(update: &UpdateGroup, author: Uuid, key: &DeviceKey, now: i64) -> Confirmation {
    Confirmation {
        id: Uuid::new_v4(),
        update_group_id: update.id,
        // The group the update belongs to, which is also the broadcast scope.
        // The custom scope is left unset, as Go leaves it
        // (`chat/confirmation.go:204-209`): it is stamped in on receipt, from
        // the update the confirmation refers to.
        destination: update.target,
        author,
        custom_scope: Uuid::nil(),
        signing_device: key.address(),
        signature: key.sign(update.id.as_bytes()).to_vec(),
        timestamp: now,
        saved_at: now,
    }
}

/// The accepted updates this device owes a confirmation for.
///
/// Everything canonical gets one, with four exceptions, all of them Go's
/// (`chat/consensus_store.go:249-259`):
///
/// - our own updates, because signing what we just said proves nothing;
/// - blocks and invite responses, which are one person's own business and
///   which no majority is entitled to overturn;
/// - updates that landed while we were not a member, since we cannot attest to
///   history we did not witness;
/// - updates we have already signed.
///
/// The final state gates the lot: a confirmation is broadcast at group scope,
/// so once the group is deleted, or we have been removed from it or blocked in
/// it, there is nobody left for us to say it to.
///
/// De-duplication reads the confirmations the store attached to each update
/// when it loaded them, which is the same check Go makes as a lookup in
/// `sendConfirmation` (`chat/confirmation.go:203`) without the query.
pub fn owed<'a>(
    stack: &'a CanonicalStack,
    my_id: Uuid,
    my_address: &str,
) -> Result<Vec<&'a UpdateGroup>> {
    let final_state = stack.top()?;
    if final_state.is_deleted() || final_state.is_blocked(my_id) || !final_state.is_member(my_id) {
        return Ok(Vec::new());
    }

    let mut owed = Vec::new();
    for (update, state) in stack.accepted_history() {
        if !state.is_member(my_id) || update.actor == my_id {
            continue;
        }
        if matches!(
            update.kind(),
            Ok(UpdateGroupType::Block) | Ok(UpdateGroupType::RespondToInvite)
        ) {
            continue;
        }
        if update
            .confirmations
            .iter()
            .any(|existing| existing.signing_device == my_address)
        {
            continue;
        }
        owed.push(update);
    }
    Ok(owed)
}

/// Check an inbound confirmation's signature and resolve the user behind it.
///
/// A confirmation carries no signed container — it *is* a signature — so this
/// is the only thing standing between the wire and the confirmation table. The
/// author travels nowhere on the wire (see [`Confirmation`]'s skipped fields):
/// it is derived here from the signing device, so a peer cannot claim someone
/// else's vote.
///
/// `device_owner` resolves a device address to its owner, returning `None` for
/// a device we have never been introduced to, whose signature therefore speaks
/// for nobody.
pub fn attribute<F>(confirmation: &Confirmation, device_owner: F) -> Result<Uuid>
where
    F: FnOnce(&str) -> Option<Uuid>,
{
    if !crypto::verify_signature(
        &confirmation.signing_device,
        confirmation.update_group_id.as_bytes(),
        &confirmation.signature,
    ) {
        return Err(Error::InvalidSignature);
    }
    device_owner(&confirmation.signing_device).ok_or(Error::DeviceNotFound)
}

/// Whether a confirmation's author has any standing in the group it refers to.
///
/// Only members are counted towards a majority
/// ([`UpdateGroup::confirming_users`]), so this is about what is worth keeping:
/// an invitee's confirmation is stored because they may accept before it is
/// next counted, while a stranger's is dropped rather than left to accumulate.
pub fn author_may_confirm(group: &Group, author: Uuid) -> bool {
    group.member_ids().contains(&author) || group.invite_ids().contains(&author)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::state::GroupState;
    use crate::frames::identity::User;
    use std::collections::HashMap;

    /// Three users with one device each, and a stack they all belong to.
    struct Fixture {
        users: Vec<Uuid>,
        keys: Vec<DeviceKey>,
    }

    impl Fixture {
        fn new(count: usize) -> Self {
            let mut fixture = Fixture {
                users: Vec::new(),
                keys: Vec::new(),
            };
            for _ in 0..count {
                fixture.users.push(Uuid::new_v4());
                fixture.keys.push(DeviceKey::generate());
            }
            fixture
        }

        fn stack(&self, my_index: usize) -> CanonicalStack {
            let mut address_map = HashMap::new();
            let mut revoked_map = HashMap::new();
            for (index, key) in self.keys.iter().enumerate() {
                address_map.insert(key.address(), self.users[index]);
                revoked_map.insert(key.address(), 0);
            }
            let state = GroupState {
                name: "Test Group".into(),
                users: self.users.clone(),
                admins: self.users.clone(),
                ..Default::default()
            };
            CanonicalStack::new(state, address_map, revoked_map, self.users[my_index])
        }

        fn update(
            &self,
            index: usize,
            kind: UpdateGroupType,
            data: Vec<u8>,
            timestamp: i64,
        ) -> UpdateGroup {
            let mut update =
                UpdateGroup::new(self.users[index], Uuid::new_v4(), kind, data, timestamp);
            update.signed.signer = self.keys[index].address();
            update.signed.signature = vec![1; 64];
            update
        }
    }

    #[test]
    fn every_update_from_someone_else_is_owed_a_confirmation() {
        let fixture = Fixture::new(3);
        let mut stack = fixture.stack(0);
        stack.insert(&fixture.update(1, UpdateGroupType::ChangeName, b"Renamed".to_vec(), 100));

        let owed = owed(&stack, fixture.users[0], &fixture.keys[0].address()).unwrap();
        assert_eq!(owed.len(), 1);
        assert_eq!(owed[0].actor, fixture.users[1]);
    }

    #[test]
    fn our_own_updates_are_not_confirmed() {
        let fixture = Fixture::new(3);
        let mut stack = fixture.stack(0);
        stack.insert(&fixture.update(0, UpdateGroupType::ChangeName, b"Renamed".to_vec(), 100));

        assert!(owed(&stack, fixture.users[0], &fixture.keys[0].address())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn blocks_and_invite_responses_are_not_confirmed() {
        let fixture = Fixture::new(3);
        let mut stack = fixture.stack(0);

        // Someone else blocking the group is their decision alone, and a group
        // nobody has blocked out from under us stays otherwise unchanged.
        stack.insert(&fixture.update(1, UpdateGroupType::Block, vec![], 100));

        assert!(owed(&stack, fixture.users[0], &fixture.keys[0].address())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn an_update_we_have_already_signed_is_not_signed_again() {
        let fixture = Fixture::new(3);
        let mut stack = fixture.stack(0);

        let mut update = fixture.update(1, UpdateGroupType::ChangeName, b"Renamed".to_vec(), 100);
        // The confirmation the store would have handed back with the update.
        update
            .confirmations
            .push(mint(&update, fixture.users[0], &fixture.keys[0], 100));
        stack.insert(&update);

        assert!(owed(&stack, fixture.users[0], &fixture.keys[0].address())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn nothing_is_owed_once_we_are_out_of_the_group() {
        let fixture = Fixture::new(3);
        let mut stack = fixture.stack(0);

        stack.insert(&fixture.update(1, UpdateGroupType::ChangeName, b"Renamed".to_vec(), 100));
        stack.insert(&fixture.update(
            1,
            UpdateGroupType::RemoveUser,
            fixture.users[0].as_bytes().to_vec(),
            200,
        ));

        // The rename would otherwise be owed a confirmation, but a group-scoped
        // broadcast from outside the group reaches nobody.
        assert!(owed(&stack, fixture.users[0], &fixture.keys[0].address())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_minted_confirmation_attributes_to_the_user_who_signed_it() {
        let fixture = Fixture::new(2);
        let update = fixture.update(1, UpdateGroupType::ChangeName, b"Renamed".to_vec(), 100);

        let confirmation = mint(&update, fixture.users[0], &fixture.keys[0], 100);
        assert_eq!(confirmation.update_group_id, update.id);
        assert_eq!(confirmation.destination, update.target);

        let author = attribute(&confirmation, |address| {
            (address == fixture.keys[0].address()).then_some(fixture.users[0])
        })
        .expect("a freshly minted confirmation verifies");
        assert_eq!(author, fixture.users[0]);
    }

    #[test]
    fn a_confirmation_for_a_different_update_does_not_verify() {
        let fixture = Fixture::new(2);
        let update = fixture.update(1, UpdateGroupType::ChangeName, b"Renamed".to_vec(), 100);
        let other = fixture.update(1, UpdateGroupType::ChangeName, b"Other".to_vec(), 200);

        // Re-pointing a genuine signature at another update is the cheapest
        // forgery available, and the signature covers exactly the ID it must.
        let mut stolen = mint(&update, fixture.users[0], &fixture.keys[0], 100);
        stolen.update_group_id = other.id;

        assert!(matches!(
            attribute(&stolen, |_| Some(fixture.users[0])),
            Err(Error::InvalidSignature)
        ));
    }

    #[test]
    fn a_confirmation_from_an_unknown_device_is_refused() {
        let fixture = Fixture::new(2);
        let update = fixture.update(1, UpdateGroupType::ChangeName, b"Renamed".to_vec(), 100);
        let confirmation = mint(&update, fixture.users[0], &fixture.keys[0], 100);

        assert!(matches!(
            attribute(&confirmation, |_| None),
            Err(Error::DeviceNotFound)
        ));
    }

    #[test]
    fn only_participants_may_confirm() {
        let member = User::new(Uuid::new_v4(), "Member".into());
        let invitee = Uuid::new_v4();
        let group = Group {
            users: vec![member.clone()],
            invites: invitee.to_string(),
            ..Default::default()
        };

        assert!(author_may_confirm(&group, member.id));
        assert!(author_may_confirm(&group, invitee));
        assert!(!author_may_confirm(&group, Uuid::new_v4()));
    }
}

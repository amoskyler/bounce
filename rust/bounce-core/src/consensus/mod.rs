//! Group consensus.
//!
//! Groups have no owner and no server, yet every member must agree on the
//! group's name, membership, admin list, and permissions — even when updates
//! arrive out of order and even when a participant lies about when they acted.
//!
//! Three mechanisms produce that agreement:
//!
//! - **[`state`]** defines what a group's state is, which changes are permitted
//!   against it, and how each change is applied.
//! - **[`stack`]** replays updates in timestamp order and resolves conflicts,
//!   using confirmations from other members to decide which of two mutually
//!   exclusive updates survives.
//! - **[`confirmation`]** produces those confirmations and vets the ones peers
//!   send, which is what keeps the conflict resolution supplied with evidence.
//!
//! See the [`stack`] module documentation for the attack this design defends
//! against and how confirmations defeat it.

pub mod confirmation;
pub mod stack;
pub mod state;

pub use stack::CanonicalStack;
pub use state::{apply_update, change_is_noop, state_change_allowed, GroupState};

use std::collections::HashMap;
use uuid::Uuid;

use crate::error::Result;
use crate::frames::group::{Group, GroupCreation, UpdateGroup};

/// Recompute a group's current state from its creation record and the full set
/// of updates known for it.
///
/// This is the entry point the engine uses whenever a group's history changes:
/// rather than mutating state incrementally, it rebuilds from the origin, which
/// is what makes the result independent of arrival order.
pub fn recompute(
    creation: &GroupCreation,
    updates: &[UpdateGroup],
    my_id: Uuid,
) -> Result<CanonicalStack> {
    let group = creation.group()?;
    let initial = GroupState::from_group(&group);

    let (address_map, revoked_map) = device_maps(&group);

    let mut stack = CanonicalStack::new(initial, address_map, revoked_map, my_id);

    // Updates are applied oldest first; the stack refuses anything out of order.
    let mut ordered: Vec<UpdateGroup> = updates.to_vec();
    ordered.sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then_with(|| a.id.cmp(&b.id)));

    for update in &ordered {
        stack.insert(update);
    }

    Ok(stack)
}

/// Build the address-to-user and address-to-revocation maps for a group's
/// founding members.
fn device_maps(group: &Group) -> (HashMap<String, Uuid>, HashMap<String, i64>) {
    let mut address_map = HashMap::new();
    let mut revoked_map = HashMap::new();
    for user in &group.users {
        for device in &user.devices {
            address_map.insert(device.address.clone(), user.id);
            revoked_map.insert(device.address.clone(), device.revoked_at);
        }
    }
    (address_map, revoked_map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::DeviceKey;
    use crate::frames::identity::{Device, User};
    use crate::types::UpdateGroupType;

    /// Create a group founded by one user with one device.
    fn founded_group() -> (GroupCreation, Uuid, DeviceKey) {
        let creator_id = Uuid::new_v4();
        let key = DeviceKey::generate();

        let mut creator = User::new(creator_id, "Founder".into());
        creator.devices.push(Device::new(
            Uuid::new_v4(),
            creator_id,
            key.address(),
            1_000,
        ));

        let group = Group {
            name: "Book Club".into(),
            created_by: creator_id,
            created_at: 1_000,
            users: vec![creator],
            admins: creator_id.to_string(),
            ..Default::default()
        };

        let creation = GroupCreation::create(&group, 1_000).unwrap();
        (creation, creator_id, key)
    }

    fn signed_update(
        key: &DeviceKey,
        actor: Uuid,
        group: Uuid,
        kind: UpdateGroupType,
        data: Vec<u8>,
        timestamp: i64,
    ) -> UpdateGroup {
        let mut update = UpdateGroup::new(actor, group, kind, data, timestamp);
        update.signed.signer = key.address();
        update.signed.signature = vec![0; 64];
        update
    }

    #[test]
    fn a_group_with_no_updates_is_its_founding_state() {
        let (creation, creator_id, _) = founded_group();
        let stack = recompute(&creation, &[], creator_id).unwrap();

        let state = stack.top().unwrap();
        assert_eq!(state.name, "Book Club");
        assert!(state.is_member(creator_id));
        assert!(state.is_admin(creator_id));
        assert_eq!(stack.accepted_update_count(), 0);
    }

    #[test]
    fn recompute_is_independent_of_the_order_updates_are_supplied_in() {
        let (creation, creator_id, key) = founded_group();
        let group_id = creation.id;

        let rename = signed_update(
            &key,
            creator_id,
            group_id,
            UpdateGroupType::ChangeName,
            b"Renamed".to_vec(),
            2_000,
        );
        let retention = signed_update(
            &key,
            creator_id,
            group_id,
            UpdateGroupType::ChangeRetention,
            UpdateGroup::encode_i64(86_400),
            3_000,
        );
        let restrict = signed_update(
            &key,
            creator_id,
            group_id,
            UpdateGroupType::ChangePostingPermission,
            vec![state::sentinels::PERMISSION_RESTRICTED],
            4_000,
        );

        let forwards = recompute(
            &creation,
            &[rename.clone(), retention.clone(), restrict.clone()],
            creator_id,
        )
        .unwrap();
        let backwards = recompute(
            &creation,
            &[restrict, retention, rename],
            creator_id,
        )
        .unwrap();

        let (a, b) = (forwards.top().unwrap(), backwards.top().unwrap());
        assert!(a.is_equivalent_to(b));
        assert_eq!(a.name, "Renamed");
        assert_eq!(a.retention, 86_400);
        assert!(a.posting_restricted);
    }

    #[test]
    fn recompute_rebuilds_the_same_state_from_scratch_each_time() {
        let (creation, creator_id, key) = founded_group();
        let updates = vec![signed_update(
            &key,
            creator_id,
            creation.id,
            UpdateGroupType::ChangeName,
            b"Renamed".to_vec(),
            2_000,
        )];

        let first = recompute(&creation, &updates, creator_id).unwrap();
        let second = recompute(&creation, &updates, creator_id).unwrap();
        assert!(first.top().unwrap().is_equivalent_to(second.top().unwrap()));
    }

    #[test]
    fn updates_from_outside_the_group_are_ignored() {
        let (creation, creator_id, _) = founded_group();

        // An outsider's device is not in the founding device map, so nothing it
        // signs can be attributed.
        let outsider_key = DeviceKey::generate();
        let outsider_update = signed_update(
            &outsider_key,
            Uuid::new_v4(),
            creation.id,
            UpdateGroupType::ChangeName,
            b"Hijacked".to_vec(),
            2_000,
        );

        let stack = recompute(&creation, &[outsider_update], creator_id).unwrap();
        assert_eq!(stack.top().unwrap().name, "Book Club");
        assert_eq!(stack.accepted_update_count(), 0);
    }
}

//! Group state, and the rules for applying an update to it.

use std::collections::HashMap;

use uuid::Uuid;

use crate::error::{Error, Result};
use crate::frames::group::{Group, UpdateGroup};
use crate::frames::identity::User;
use crate::msgpack;
use crate::types::UpdateGroupType;

/// Payload sentinel bytes. These are part of the wire format.
pub mod sentinels {
    pub const PERMISSION_UNRESTRICTED: u8 = 0x00;
    pub const PERMISSION_RESTRICTED: u8 = 0x01;

    pub const REJECT_INVITE: u8 = 0x00;
    pub const ACCEPT_INVITE: u8 = 0x01;

    pub const OVERRIDDEN: u8 = 0x01;
    /// Note the inversion: the *enabled* sentinel is zero.
    pub const ENABLED: u8 = 0x00;
}

/// A group's state at one point in its history.
///
/// Two devices that have seen the same set of updates must arrive at an
/// identical `GroupState`, whatever order the updates arrived in. That is the
/// property the canonical stack exists to guarantee.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupState {
    pub name: String,
    pub images: Vec<Uuid>,
    pub users: Vec<Uuid>,
    pub admins: Vec<Uuid>,
    pub invites: Vec<Uuid>,
    pub blocked_users: Vec<Uuid>,
    /// Device addresses per user, needed to resolve the group's scope.
    pub devices: HashMap<Uuid, Vec<String>>,

    pub muted_until: i64,
    pub retention: i64,
    pub clear_before: i64,

    pub posting_restricted: bool,
    pub editing_restricted: bool,
    pub user_management_restricted: bool,

    pub read_receipts_overridden: bool,
    pub read_receipts_enabled: bool,
    pub typing_indicators_overridden: bool,
    pub typing_indicators_enabled: bool,

    pub invited_by: Uuid,
    pub invited_at: i64,
    pub accepted_at: i64,

    /// The update that deleted the group, if any.
    pub deleted_by: Option<Box<UpdateGroup>>,
    /// The update that removed us from the group, if any.
    pub removed_by: Option<Box<UpdateGroup>>,
    /// The update by which we blocked the group, if any.
    pub blocked_by: Option<Box<UpdateGroup>>,

    /// The update that produced this state. The initial state has none.
    pub update: Option<Box<UpdateGroup>>,
}

impl GroupState {
    /// Build the founding state from a group's creation record.
    pub fn from_group(group: &Group) -> Self {
        let mut state = GroupState {
            name: group.name.clone(),
            images: group.image_ids(),
            users: group.member_ids(),
            admins: group.admin_ids(),
            invites: Vec::new(),
            blocked_users: Vec::new(),
            devices: HashMap::new(),
            muted_until: group.muted_until,
            retention: group.retention,
            clear_before: group.clear_before,
            posting_restricted: group.restrict_posting,
            editing_restricted: group.restrict_group_edits,
            user_management_restricted: group.restrict_user_management,
            read_receipts_overridden: false,
            read_receipts_enabled: true,
            typing_indicators_overridden: false,
            typing_indicators_enabled: true,
            invited_by: Uuid::nil(),
            invited_at: 0,
            accepted_at: 0,
            deleted_by: None,
            removed_by: None,
            blocked_by: None,
            update: None,
        };

        for user in &group.users {
            state.devices.insert(
                user.id,
                user.devices.iter().map(|d| d.address.clone()).collect(),
            );
        }

        state
    }

    pub fn is_member(&self, user_id: Uuid) -> bool {
        self.users.contains(&user_id)
    }
    pub fn is_admin(&self, user_id: Uuid) -> bool {
        self.admins.contains(&user_id)
    }
    pub fn is_invited(&self, user_id: Uuid) -> bool {
        self.invites.contains(&user_id)
    }
    pub fn is_blocked(&self, user_id: Uuid) -> bool {
        self.blocked_users.contains(&user_id)
    }
    pub fn is_deleted(&self) -> bool {
        self.deleted_by.is_some()
    }

    /// Every device address in the group, which is what a group-scoped
    /// broadcast resolves to.
    pub fn scope_addresses(&self) -> Vec<String> {
        let mut addresses = Vec::new();
        for user_id in &self.users {
            if let Some(user_devices) = self.devices.get(user_id) {
                addresses.extend(user_devices.iter().cloned());
            }
        }
        addresses
    }

    /// Like [`GroupState::scope_addresses`], plus the devices of invited users.
    pub fn scope_addresses_with_invites(&self) -> Vec<String> {
        let mut addresses = self.scope_addresses();
        for user_id in &self.invites {
            if let Some(user_devices) = self.devices.get(user_id) {
                addresses.extend(user_devices.iter().cloned());
            }
        }
        addresses
    }

    /// Compare two states for the purpose of detecting no-op updates.
    ///
    /// The bookkeeping fields — which update produced the state, and the
    /// removal and deletion markers — are excluded, because they always differ
    /// between a state and its successor and would make every update look like
    /// a change.
    pub fn is_equivalent_to(&self, other: &GroupState) -> bool {
        self.name == other.name
            && self.images == other.images
            && self.users == other.users
            && self.admins == other.admins
            && self.invites == other.invites
            && self.blocked_users == other.blocked_users
            && self.muted_until == other.muted_until
            && self.retention == other.retention
            && self.clear_before == other.clear_before
            && self.posting_restricted == other.posting_restricted
            && self.editing_restricted == other.editing_restricted
            && self.user_management_restricted == other.user_management_restricted
            && self.read_receipts_overridden == other.read_receipts_overridden
            && self.read_receipts_enabled == other.read_receipts_enabled
            && self.typing_indicators_overridden == other.typing_indicators_overridden
            && self.typing_indicators_enabled == other.typing_indicators_enabled
            && self.deleted_by.is_some() == other.deleted_by.is_some()
    }
}

/// Whether `update` may be applied to `state` by the user who signed it.
///
/// `my_id` is the local user, needed because a few update types are only ever
/// legitimate when a user is changing their own view of a group.
pub fn state_change_allowed(state: &GroupState, update: &UpdateGroup, my_id: Uuid) -> Result<()> {
    use UpdateGroupType::*;

    let kind = update.kind()?;

    // Non-members can do exactly two things: respond to an invitation they
    // hold, and block the group.
    if !state.is_member(update.actor) && !matches!(kind, RespondToInvite | Block) {
        return Err(Error::NotPermitted("actor is not a member of the group"));
    }

    let requires_admin_if = |restricted: bool, message: &'static str| -> Result<()> {
        if restricted && !state.is_admin(update.actor) {
            Err(Error::NotPermitted(message))
        } else {
            Ok(())
        }
    };

    match kind {
        ChangeName | SetImage | ChangeRetention | SetClearBefore => requires_admin_if(
            state.editing_restricted,
            "group edits are restricted to admins",
        ),

        InviteUser | RevokeInvite => requires_admin_if(
            state.user_management_restricted,
            "user management is restricted to admins",
        ),

        RemoveUser => {
            let target = update
                .data_as_uuid()
                .ok_or(Error::NotPermitted("remove user payload is not a UUID"))?;

            // Anyone may remove themselves.
            if update.actor == target {
                return Ok(());
            }
            // The admin who deleted the group cannot be removed, which would
            // otherwise undo the deletion.
            if let Some(deleted_by) = &state.deleted_by {
                if deleted_by.actor == target {
                    return Err(Error::NotPermitted(
                        "cannot remove the admin who deleted the group",
                    ));
                }
            }
            requires_admin_if(
                state.user_management_restricted,
                "user management is restricted to admins",
            )
        }

        PromoteAdmin => {
            if state.is_admin(update.actor) {
                let target = update
                    .data_as_uuid()
                    .ok_or(Error::NotPermitted("promote payload is not a UUID"))?;
                if state.is_member(target) {
                    Ok(())
                } else {
                    Err(Error::NotPermitted("cannot promote a non-member"))
                }
            } else if state.admins.is_empty() {
                // A group that has lost all its admins would otherwise be
                // permanently frozen, so anyone may appoint one.
                Ok(())
            } else {
                Err(Error::NotPermitted("only admins may promote"))
            }
        }

        DemoteAdmin => {
            let target = update
                .data_as_uuid()
                .ok_or(Error::NotPermitted("demote payload is not a UUID"))?;
            if let Some(deleted_by) = &state.deleted_by {
                if deleted_by.actor == target {
                    return Err(Error::NotPermitted(
                        "cannot demote the admin who deleted the group",
                    ));
                }
            }
            if state.is_admin(update.actor) {
                Ok(())
            } else {
                Err(Error::NotPermitted("only admins may demote"))
            }
        }

        ChangeUserManagementPermission | ChangeGroupEditsPermission | ChangePostingPermission => {
            if state.is_admin(update.actor) {
                Ok(())
            } else {
                Err(Error::NotPermitted("only admins may change permissions"))
            }
        }

        Delete => {
            if state.is_deleted() {
                Err(Error::NotPermitted("group is already deleted"))
            } else if state.is_admin(update.actor) {
                Ok(())
            } else {
                Err(Error::NotPermitted("only admins may delete the group"))
            }
        }

        // Blocking is always the blocker's own decision.
        Block => Ok(()),

        // Personal settings are only ever changed by their owner.
        ChangeMutedUntil | SetReadReceiptSettings | SetTypingIndicatorSettings => {
            if my_id == update.actor {
                Ok(())
            } else {
                Err(Error::NotPermitted("personal settings are self-only"))
            }
        }

        RespondToInvite => {
            if state.is_invited(update.actor) {
                Ok(())
            } else {
                Err(Error::NotPermitted("only an invitee may respond"))
            }
        }
    }
}

/// Apply an update to a state, returning the new state.
///
/// This performs no permission checking; callers go through
/// [`state_change_allowed`] first.
pub fn apply_update(state: &GroupState, update: &UpdateGroup, my_id: Uuid) -> Result<GroupState> {
    use UpdateGroupType::*;

    let mut next = state.clone();
    next.update = Some(Box::new(update.clone()));

    match update.kind()? {
        ChangeName => {
            let name = std::str::from_utf8(&update.data)
                .map_err(|_| Error::InvalidFrame("group name is not UTF-8".into()))?;
            if !crate::frames::identity::valid_user_name(name) {
                return Err(Error::InvalidFrame("invalid group name".into()));
            }
            next.name = name.to_string();
        }

        SetImage => {
            let image = update
                .data_as_uuid()
                .ok_or_else(|| Error::InvalidFrame("image payload is not a UUID".into()))?;
            next.images.push(image);
        }

        InviteUser => {
            let user: User = msgpack::from_slice(&update.data)?;
            // Inviting an existing member changes nothing.
            if next.is_member(user.id) {
                return Ok(next);
            }
            if !next.is_invited(user.id) {
                next.invites.push(user.id);
            }
            if user.id == my_id {
                next.invited_by = update.actor;
                next.invited_at = update.timestamp;
            }
            // The invitation carries the invitee's device group, so the rest of
            // the group can reach them before they have accepted.
            next.devices.insert(
                user.id,
                user.devices.iter().map(|d| d.address.clone()).collect(),
            );
        }

        RemoveUser => {
            let target = update
                .data_as_uuid()
                .ok_or_else(|| Error::InvalidFrame("remove payload is not a UUID".into()))?;
            next.users.retain(|id| *id != target);
            next.admins.retain(|id| *id != target);
            // Removing the deleter undoes the deletion.
            if next
                .deleted_by
                .as_ref()
                .is_some_and(|deleted| deleted.actor == target)
            {
                next.deleted_by = None;
            }
        }

        ChangeMutedUntil => {
            next.muted_until = update
                .data_as_i64()
                .ok_or_else(|| Error::InvalidFrame("muted until payload is not an i64".into()))?;
        }

        ChangeRetention => {
            next.retention = update
                .data_as_i64()
                .ok_or_else(|| Error::InvalidFrame("retention payload is not an i64".into()))?;
        }

        SetClearBefore => {
            next.clear_before = update
                .data_as_i64()
                .ok_or_else(|| Error::InvalidFrame("clear before payload is not an i64".into()))?;
        }

        PromoteAdmin => {
            let target = update
                .data_as_uuid()
                .ok_or_else(|| Error::InvalidFrame("promote payload is not a UUID".into()))?;
            if !next.is_member(target) {
                return Err(Error::NotPermitted("cannot promote a non-member"));
            }
            if !next.is_admin(target) {
                next.admins.push(target);
            }
        }

        DemoteAdmin => {
            let target = update
                .data_as_uuid()
                .ok_or_else(|| Error::InvalidFrame("demote payload is not a UUID".into()))?;
            next.admins.retain(|id| *id != target);
            if next
                .deleted_by
                .as_ref()
                .is_some_and(|deleted| deleted.actor == target)
            {
                next.deleted_by = None;
            }
        }

        ChangeUserManagementPermission => {
            next.user_management_restricted = permission_is_restricted(&update.data)?;
        }
        ChangeGroupEditsPermission => {
            next.editing_restricted = permission_is_restricted(&update.data)?;
        }
        ChangePostingPermission => {
            next.posting_restricted = permission_is_restricted(&update.data)?;
        }

        Delete => {
            next.deleted_by = Some(Box::new(update.clone()));
        }

        Block => {
            if !next.is_blocked(update.actor) {
                next.blocked_users.push(update.actor);
            }
            next.users.retain(|id| *id != update.actor);
            next.admins.retain(|id| *id != update.actor);
            next.invites.retain(|id| *id != update.actor);
        }

        SetReadReceiptSettings => {
            let (overridden, enabled) = settings_payload(&update.data)?;
            next.read_receipts_overridden = overridden;
            next.read_receipts_enabled = enabled;
        }

        SetTypingIndicatorSettings => {
            let (overridden, enabled) = settings_payload(&update.data)?;
            next.typing_indicators_overridden = overridden;
            next.typing_indicators_enabled = enabled;
        }

        RevokeInvite => {
            let target = update
                .data_as_uuid()
                .ok_or_else(|| Error::InvalidFrame("revoke payload is not a UUID".into()))?;
            next.invites.retain(|id| *id != target);
        }

        RespondToInvite => {
            if update.data.len() != 1 {
                return Err(Error::InvalidFrame("invite response must be one byte".into()));
            }
            let accepted = update.data[0] == sentinels::ACCEPT_INVITE;
            next.invites.retain(|id| *id != update.actor);
            if accepted {
                next.users.push(update.actor);
            }
        }
    }

    Ok(next)
}

/// Whether applying `update` would leave the state unchanged.
///
/// No-ops are dropped rather than pushed, so that a redundant update cannot be
/// used to displace a real one during conflict resolution.
pub fn change_is_noop(state: &GroupState, update: &UpdateGroup, my_id: Uuid) -> bool {
    match apply_update(state, update, my_id) {
        // An update that cannot even be applied changes nothing.
        Err(_) => true,
        Ok(next) => next.is_equivalent_to(state),
    }
}

fn permission_is_restricted(data: &[u8]) -> Result<bool> {
    if data.len() != 1 {
        return Err(Error::InvalidFrame(
            "permission payload must be one byte".into(),
        ));
    }
    match data[0] {
        sentinels::PERMISSION_RESTRICTED => Ok(true),
        sentinels::PERMISSION_UNRESTRICTED => Ok(false),
        _ => Err(Error::InvalidFrame("invalid permission byte".into())),
    }
}

/// Decode an override/value settings payload into `(overridden, enabled)`.
fn settings_payload(data: &[u8]) -> Result<(bool, bool)> {
    if data.len() != 2 {
        return Err(Error::InvalidFrame(
            "settings payload must be two bytes".into(),
        ));
    }
    Ok((
        data[0] == sentinels::OVERRIDDEN,
        data[1] == sentinels::ENABLED,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::group::UpdateGroup;

    fn state_with(users: Vec<Uuid>, admins: Vec<Uuid>) -> GroupState {
        GroupState {
            name: "Test Group".into(),
            users,
            admins,
            ..Default::default()
        }
    }

    fn update(actor: Uuid, kind: UpdateGroupType, data: Vec<u8>) -> UpdateGroup {
        UpdateGroup::new(actor, Uuid::new_v4(), kind, data, 1_000)
    }

    #[test]
    fn non_members_can_only_respond_to_invites_and_block() {
        let member = Uuid::new_v4();
        let outsider = Uuid::new_v4();
        let state = state_with(vec![member], vec![member]);

        assert!(state_change_allowed(
            &state,
            &update(outsider, UpdateGroupType::ChangeName, b"Hijacked".to_vec()),
            member
        )
        .is_err());

        // Blocking is always allowed.
        assert!(state_change_allowed(
            &state,
            &update(outsider, UpdateGroupType::Block, vec![]),
            member
        )
        .is_ok());
    }

    #[test]
    fn editing_restrictions_are_enforced() {
        let admin = Uuid::new_v4();
        let member = Uuid::new_v4();
        let mut state = state_with(vec![admin, member], vec![admin]);

        // Unrestricted: anyone may rename.
        assert!(state_change_allowed(
            &state,
            &update(member, UpdateGroupType::ChangeName, b"New".to_vec()),
            admin
        )
        .is_ok());

        state.editing_restricted = true;
        assert!(state_change_allowed(
            &state,
            &update(member, UpdateGroupType::ChangeName, b"New".to_vec()),
            admin
        )
        .is_err());
        assert!(state_change_allowed(
            &state,
            &update(admin, UpdateGroupType::ChangeName, b"New".to_vec()),
            admin
        )
        .is_ok());
    }

    #[test]
    fn anyone_may_remove_themselves() {
        let admin = Uuid::new_v4();
        let member = Uuid::new_v4();
        let mut state = state_with(vec![admin, member], vec![admin]);
        state.user_management_restricted = true;

        // A plain member cannot remove someone else...
        assert!(state_change_allowed(
            &state,
            &update(member, UpdateGroupType::RemoveUser, admin.as_bytes().to_vec()),
            admin
        )
        .is_err());

        // ...but leaving is always their own decision.
        assert!(state_change_allowed(
            &state,
            &update(member, UpdateGroupType::RemoveUser, member.as_bytes().to_vec()),
            admin
        )
        .is_ok());
    }

    #[test]
    fn a_group_without_admins_can_appoint_one() {
        let member = Uuid::new_v4();
        let state = state_with(vec![member], vec![]);

        // Otherwise the group would be permanently frozen.
        assert!(state_change_allowed(
            &state,
            &update(member, UpdateGroupType::PromoteAdmin, member.as_bytes().to_vec()),
            member
        )
        .is_ok());
    }

    #[test]
    fn non_admins_cannot_promote_while_an_admin_exists() {
        let admin = Uuid::new_v4();
        let member = Uuid::new_v4();
        let state = state_with(vec![admin, member], vec![admin]);

        assert!(state_change_allowed(
            &state,
            &update(member, UpdateGroupType::PromoteAdmin, member.as_bytes().to_vec()),
            admin
        )
        .is_err());
    }

    #[test]
    fn a_non_member_cannot_be_promoted() {
        let admin = Uuid::new_v4();
        let outsider = Uuid::new_v4();
        let state = state_with(vec![admin], vec![admin]);

        assert!(state_change_allowed(
            &state,
            &update(admin, UpdateGroupType::PromoteAdmin, outsider.as_bytes().to_vec()),
            admin
        )
        .is_err());
    }

    #[test]
    fn the_deleting_admin_is_protected_from_removal_and_demotion() {
        let admin = Uuid::new_v4();
        let other_admin = Uuid::new_v4();
        let mut state = state_with(vec![admin, other_admin], vec![admin, other_admin]);

        let deletion = update(admin, UpdateGroupType::Delete, vec![]);
        state = apply_update(&state, &deletion, admin).unwrap();
        assert!(state.is_deleted());

        // Removing or demoting the deleter would resurrect the group.
        assert!(state_change_allowed(
            &state,
            &update(other_admin, UpdateGroupType::RemoveUser, admin.as_bytes().to_vec()),
            other_admin
        )
        .is_err());
        assert!(state_change_allowed(
            &state,
            &update(other_admin, UpdateGroupType::DemoteAdmin, admin.as_bytes().to_vec()),
            other_admin
        )
        .is_err());
    }

    #[test]
    fn a_group_cannot_be_deleted_twice() {
        let admin = Uuid::new_v4();
        let mut state = state_with(vec![admin], vec![admin]);
        state = apply_update(&state, &update(admin, UpdateGroupType::Delete, vec![]), admin).unwrap();

        assert!(state_change_allowed(
            &state,
            &update(admin, UpdateGroupType::Delete, vec![]),
            admin
        )
        .is_err());
    }

    #[test]
    fn personal_settings_are_self_only() {
        let me = Uuid::new_v4();
        let them = Uuid::new_v4();
        let state = state_with(vec![me, them], vec![]);

        let mute = UpdateGroup::encode_i64(9_999);
        assert!(state_change_allowed(
            &state,
            &update(me, UpdateGroupType::ChangeMutedUntil, mute.clone()),
            me
        )
        .is_ok());

        // Somebody else muting the group on my behalf is meaningless.
        assert!(state_change_allowed(
            &state,
            &update(them, UpdateGroupType::ChangeMutedUntil, mute),
            me
        )
        .is_err());
    }

    #[test]
    fn only_an_invitee_may_respond_to_an_invitation() {
        let admin = Uuid::new_v4();
        let invitee = Uuid::new_v4();
        let stranger = Uuid::new_v4();

        let mut state = state_with(vec![admin], vec![admin]);
        state.invites.push(invitee);

        assert!(state_change_allowed(
            &state,
            &update(invitee, UpdateGroupType::RespondToInvite, vec![sentinels::ACCEPT_INVITE]),
            admin
        )
        .is_ok());
        assert!(state_change_allowed(
            &state,
            &update(stranger, UpdateGroupType::RespondToInvite, vec![sentinels::ACCEPT_INVITE]),
            admin
        )
        .is_err());
    }

    #[test]
    fn accepting_an_invitation_makes_the_user_a_member() {
        let admin = Uuid::new_v4();
        let invitee = Uuid::new_v4();
        let mut state = state_with(vec![admin], vec![admin]);
        state.invites.push(invitee);

        let accepted = apply_update(
            &state,
            &update(invitee, UpdateGroupType::RespondToInvite, vec![sentinels::ACCEPT_INVITE]),
            admin,
        )
        .unwrap();

        assert!(accepted.is_member(invitee));
        assert!(!accepted.is_invited(invitee));
    }

    #[test]
    fn rejecting_an_invitation_only_clears_it() {
        let admin = Uuid::new_v4();
        let invitee = Uuid::new_v4();
        let mut state = state_with(vec![admin], vec![admin]);
        state.invites.push(invitee);

        let rejected = apply_update(
            &state,
            &update(invitee, UpdateGroupType::RespondToInvite, vec![sentinels::REJECT_INVITE]),
            admin,
        )
        .unwrap();

        assert!(!rejected.is_member(invitee));
        assert!(!rejected.is_invited(invitee));
    }

    #[test]
    fn blocking_removes_the_blocker_from_every_list() {
        let admin = Uuid::new_v4();
        let leaver = Uuid::new_v4();
        let mut state = state_with(vec![admin, leaver], vec![admin, leaver]);
        state.invites.push(leaver);

        let blocked = apply_update(&state, &update(leaver, UpdateGroupType::Block, vec![]), admin)
            .unwrap();

        assert!(blocked.is_blocked(leaver));
        assert!(!blocked.is_member(leaver));
        assert!(!blocked.is_admin(leaver));
        assert!(!blocked.is_invited(leaver));
        // Everyone else is untouched.
        assert!(blocked.is_member(admin));
    }

    #[test]
    fn removing_the_deleter_resurrects_the_group() {
        let admin = Uuid::new_v4();
        let mut state = state_with(vec![admin], vec![admin]);
        state = apply_update(&state, &update(admin, UpdateGroupType::Delete, vec![]), admin).unwrap();
        assert!(state.is_deleted());

        // Applied directly, bypassing the permission check that normally
        // forbids this, to confirm the state transition itself is consistent.
        let revived = apply_update(
            &state,
            &update(admin, UpdateGroupType::RemoveUser, admin.as_bytes().to_vec()),
            admin,
        )
        .unwrap();
        assert!(!revived.is_deleted());
    }

    #[test]
    fn settings_payloads_decode_with_the_inverted_enabled_sentinel() {
        let me = Uuid::new_v4();
        let state = state_with(vec![me], vec![me]);

        // Overridden and enabled.
        let enabled = apply_update(
            &state,
            &update(
                me,
                UpdateGroupType::SetReadReceiptSettings,
                vec![sentinels::OVERRIDDEN, sentinels::ENABLED],
            ),
            me,
        )
        .unwrap();
        assert!(enabled.read_receipts_overridden);
        assert!(enabled.read_receipts_enabled);

        // Overridden and disabled.
        let disabled = apply_update(
            &state,
            &update(
                me,
                UpdateGroupType::SetReadReceiptSettings,
                vec![sentinels::OVERRIDDEN, 0x01],
            ),
            me,
        )
        .unwrap();
        assert!(disabled.read_receipts_overridden);
        assert!(!disabled.read_receipts_enabled);
    }

    #[test]
    fn permission_payloads_are_validated() {
        let admin = Uuid::new_v4();
        let state = state_with(vec![admin], vec![admin]);

        let restricted = apply_update(
            &state,
            &update(
                admin,
                UpdateGroupType::ChangePostingPermission,
                vec![sentinels::PERMISSION_RESTRICTED],
            ),
            admin,
        )
        .unwrap();
        assert!(restricted.posting_restricted);

        // Anything other than the two sentinels is malformed.
        assert!(apply_update(
            &state,
            &update(admin, UpdateGroupType::ChangePostingPermission, vec![0x7f]),
            admin
        )
        .is_err());
    }

    #[test]
    fn redundant_updates_are_recognised_as_noops() {
        let admin = Uuid::new_v4();
        let mut state = state_with(vec![admin], vec![admin]);
        state.name = "Existing".into();

        // Renaming to the current name changes nothing.
        assert!(change_is_noop(
            &state,
            &update(admin, UpdateGroupType::ChangeName, b"Existing".to_vec()),
            admin
        ));
        assert!(!change_is_noop(
            &state,
            &update(admin, UpdateGroupType::ChangeName, b"Different".to_vec()),
            admin
        ));

        // Promoting someone who is already an admin changes nothing.
        assert!(change_is_noop(
            &state,
            &update(admin, UpdateGroupType::PromoteAdmin, admin.as_bytes().to_vec()),
            admin
        ));
    }

    #[test]
    fn an_inapplicable_update_counts_as_a_noop() {
        let admin = Uuid::new_v4();
        let state = state_with(vec![admin], vec![admin]);

        // A malformed payload cannot be applied, so it is treated as changing
        // nothing rather than as a conflict.
        assert!(change_is_noop(
            &state,
            &update(admin, UpdateGroupType::ChangeRetention, vec![1, 2, 3]),
            admin
        ));
    }

    #[test]
    fn scope_includes_invited_users_only_when_asked() {
        let member = Uuid::new_v4();
        let invitee = Uuid::new_v4();

        let mut state = state_with(vec![member], vec![member]);
        state.invites.push(invitee);
        state.devices.insert(member, vec!["memberdevice".into()]);
        state.devices.insert(invitee, vec!["inviteedevice".into()]);

        assert_eq!(state.scope_addresses(), vec!["memberdevice".to_string()]);

        let with_invites = state.scope_addresses_with_invites();
        assert!(with_invites.contains(&"memberdevice".to_string()));
        assert!(with_invites.contains(&"inviteedevice".to_string()));
    }
}

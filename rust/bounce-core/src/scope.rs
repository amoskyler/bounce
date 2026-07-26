//! Resolving a frame's scope to a concrete set of device addresses.
//!
//! Broadcasting in Bounce is not "send to everyone" — each frame names a scope,
//! and the scope is resolved against local state to produce the list of devices
//! that are entitled to it. A frame is never written to a device outside its
//! scope, which is what keeps a message from leaking to a contact who happens
//! to be connected at the time.
//!
//! The resolution rules per scope:
//!
//! | Scope | Devices |
//! |---|---|
//! | [`Sync`] | this user's own devices |
//! | [`User`] | own devices, plus every device of one other user |
//! | [`Group`] | every device of every group member |
//! | [`GroupWithInvites`] | as `Group`, plus devices of invited users |
//! | [`Global`] | every known contact's devices, or the overlap set |
//! | [`Custom`] | an explicit list stored in the database |
//!
//! [`Sync`]: Scope::Sync
//! [`User`]: Scope::User
//! [`Group`]: Scope::Group
//! [`GroupWithInvites`]: Scope::GroupWithInvites
//! [`Global`]: Scope::Global
//! [`Custom`]: Scope::Custom
//!
//! ## The overlap rule
//!
//! Global scope is asymmetric. A profile update we *authored* may go to every
//! device we know. A profile update authored by someone *else* may only be
//! relayed to users who share a group with that author — because a shared group
//! is proof that the author already exposes their profile to that user.
//! Forwarding beyond that would tell a contact something about the author's
//! social graph that the author never disclosed.

use std::collections::HashSet;
use uuid::Uuid;

use crate::types::Scope;

/// The local state a scope is resolved against.
///
/// This is a read-only view, supplied by the engine from the database. Keeping
/// resolution separate from storage makes the rules testable in isolation,
/// which matters because a mistake here is a privacy leak rather than a crash.
pub trait ScopeContext {
    /// The local user's ID.
    fn my_user_id(&self) -> Uuid;

    /// This device's own address, which is always excluded from broadcasts.
    fn my_address(&self) -> &str;

    /// Active device addresses belonging to a user.
    fn devices_for_user(&self, user_id: Uuid) -> Vec<String>;

    /// Member user IDs of a group.
    fn members_of_group(&self, group_id: Uuid) -> Vec<Uuid>;

    /// User IDs with a pending invitation to a group.
    fn invitees_of_group(&self, group_id: Uuid) -> Vec<Uuid>;

    /// Every known device address.
    fn all_device_addresses(&self) -> Vec<String>;

    /// Users who share at least one group with `user_id`.
    fn users_sharing_a_group_with(&self, user_id: Uuid) -> Vec<Uuid>;

    /// The addresses stored in a custom scope.
    fn custom_scope_addresses(&self, scope_id: Uuid) -> Vec<String>;

    /// Whether an address belongs to an encrypted device. Encrypted devices
    /// receive frames through a separate path and are excluded from ordinary
    /// broadcast targets.
    fn is_encrypted_device(&self, address: &str) -> bool;

    /// Whether a device has been revoked.
    fn is_revoked(&self, address: &str) -> bool;
}

/// Resolve a scope to the device addresses a frame should be written to.
///
/// The result never contains this device, revoked devices, encrypted devices,
/// or duplicates.
pub fn resolve<C: ScopeContext>(
    context: &C,
    scope: Scope,
    destination: Uuid,
    author: Uuid,
) -> Vec<String> {
    let raw = match scope {
        Scope::Sync => context.devices_for_user(context.my_user_id()),

        Scope::User => {
            // A user-scoped frame addressed to ourselves has no counterparty,
            // so it degrades to sync scope.
            if destination == context.my_user_id() || destination.is_nil() {
                context.devices_for_user(context.my_user_id())
            } else {
                let mut addresses = context.devices_for_user(destination);
                addresses.extend(context.devices_for_user(context.my_user_id()));
                addresses
            }
        }

        Scope::Group => group_addresses(context, destination, false),

        Scope::GroupWithInvites => group_addresses(context, destination, true),

        Scope::Global => global_addresses(context, author),

        Scope::Custom => context.custom_scope_addresses(destination),
    };

    dedupe_and_filter(context, raw)
}

fn group_addresses<C: ScopeContext>(
    context: &C,
    group_id: Uuid,
    include_invitees: bool,
) -> Vec<String> {
    let mut addresses = Vec::new();

    for member in context.members_of_group(group_id) {
        addresses.extend(context.devices_for_user(member));
    }

    if include_invitees {
        for invitee in context.invitees_of_group(group_id) {
            addresses.extend(context.devices_for_user(invitee));
        }
    }

    addresses
}

fn global_addresses<C: ScopeContext>(context: &C, author: Uuid) -> Vec<String> {
    if author == context.my_user_id() {
        // Our own profile updates may go to anyone we know.
        return context.all_device_addresses();
    }

    // Someone else's update may only be relayed within the overlap — see the
    // rule in the module documentation.
    let mut addresses = context.devices_for_user(context.my_user_id());
    addresses.extend(context.devices_for_user(author));

    for user_id in context.users_sharing_a_group_with(author) {
        addresses.extend(context.devices_for_user(user_id));
    }

    addresses
}

fn dedupe_and_filter<C: ScopeContext>(context: &C, addresses: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for address in addresses {
        if address == context.my_address() {
            continue;
        }
        if context.is_revoked(&address) {
            continue;
        }
        if context.is_encrypted_device(&address) {
            continue;
        }
        if seen.insert(address.clone()) {
            out.push(address);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// An in-memory context, so scope rules can be tested without a database.
    #[derive(Default)]
    struct TestContext {
        my_user: Uuid,
        my_address: String,
        user_devices: HashMap<Uuid, Vec<String>>,
        group_members: HashMap<Uuid, Vec<Uuid>>,
        group_invitees: HashMap<Uuid, Vec<Uuid>>,
        overlaps: HashMap<Uuid, Vec<Uuid>>,
        custom_scopes: HashMap<Uuid, Vec<String>>,
        encrypted: HashSet<String>,
        revoked: HashSet<String>,
    }

    impl ScopeContext for TestContext {
        fn my_user_id(&self) -> Uuid {
            self.my_user
        }
        fn my_address(&self) -> &str {
            &self.my_address
        }
        fn devices_for_user(&self, user_id: Uuid) -> Vec<String> {
            self.user_devices.get(&user_id).cloned().unwrap_or_default()
        }
        fn members_of_group(&self, group_id: Uuid) -> Vec<Uuid> {
            self.group_members.get(&group_id).cloned().unwrap_or_default()
        }
        fn invitees_of_group(&self, group_id: Uuid) -> Vec<Uuid> {
            self.group_invitees
                .get(&group_id)
                .cloned()
                .unwrap_or_default()
        }
        fn all_device_addresses(&self) -> Vec<String> {
            self.user_devices.values().flatten().cloned().collect()
        }
        fn users_sharing_a_group_with(&self, user_id: Uuid) -> Vec<Uuid> {
            self.overlaps.get(&user_id).cloned().unwrap_or_default()
        }
        fn custom_scope_addresses(&self, scope_id: Uuid) -> Vec<String> {
            self.custom_scopes.get(&scope_id).cloned().unwrap_or_default()
        }
        fn is_encrypted_device(&self, address: &str) -> bool {
            self.encrypted.contains(address)
        }
        fn is_revoked(&self, address: &str) -> bool {
            self.revoked.contains(address)
        }
    }

    /// Three users: me (two devices), alice (two), and bob (one).
    struct World {
        context: TestContext,
        me: Uuid,
        alice: Uuid,
        bob: Uuid,
    }

    fn world() -> World {
        let me = Uuid::new_v4();
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();

        let mut context = TestContext {
            my_user: me,
            my_address: "my-laptop".into(),
            ..Default::default()
        };
        context
            .user_devices
            .insert(me, vec!["my-laptop".into(), "my-phone".into()]);
        context
            .user_devices
            .insert(alice, vec!["alice-phone".into(), "alice-desktop".into()]);
        context.user_devices.insert(bob, vec!["bob-phone".into()]);

        World {
            context,
            me,
            alice,
            bob,
        }
    }

    #[test]
    fn sync_scope_excludes_this_device() {
        let w = world();
        let targets = resolve(&w.context, Scope::Sync, Uuid::nil(), w.me);

        // Our other device, but never ourselves.
        assert_eq!(targets, vec!["my-phone".to_string()]);
    }

    #[test]
    fn user_scope_reaches_both_device_groups() {
        let w = world();
        let targets = resolve(&w.context, Scope::User, w.alice, w.me);

        assert!(targets.contains(&"alice-phone".to_string()));
        assert!(targets.contains(&"alice-desktop".to_string()));
        assert!(targets.contains(&"my-phone".to_string()));
        assert!(!targets.contains(&"my-laptop".to_string()));
        // Bob is not part of this conversation.
        assert!(!targets.contains(&"bob-phone".to_string()));
    }

    #[test]
    fn a_user_scoped_frame_addressed_to_ourselves_degrades_to_sync() {
        let w = world();
        let targets = resolve(&w.context, Scope::User, w.me, w.me);
        assert_eq!(targets, vec!["my-phone".to_string()]);
    }

    #[test]
    fn group_scope_covers_members_but_not_invitees() {
        let mut w = world();
        let group = Uuid::new_v4();
        w.context.group_members.insert(group, vec![w.me, w.alice]);
        w.context.group_invitees.insert(group, vec![w.bob]);

        let members_only = resolve(&w.context, Scope::Group, group, w.me);
        assert!(members_only.contains(&"alice-phone".to_string()));
        assert!(members_only.contains(&"my-phone".to_string()));
        assert!(
            !members_only.contains(&"bob-phone".to_string()),
            "an invitee must not receive group messages before joining"
        );

        let with_invites = resolve(&w.context, Scope::GroupWithInvites, group, w.me);
        assert!(
            with_invites.contains(&"bob-phone".to_string()),
            "an invitee must see the group's metadata so they can decide"
        );
    }

    #[test]
    fn our_own_profile_updates_go_to_every_contact() {
        let w = world();
        let targets = resolve(&w.context, Scope::Global, Uuid::nil(), w.me);

        assert!(targets.contains(&"alice-phone".to_string()));
        assert!(targets.contains(&"bob-phone".to_string()));
        assert!(targets.contains(&"my-phone".to_string()));
    }

    #[test]
    fn someone_elses_profile_update_only_reaches_the_overlap() {
        let mut w = world();
        // Alice shares a group with Bob. Carol is a contact of ours with no
        // known tie to Alice.
        w.context.overlaps.insert(w.alice, vec![w.bob]);

        let carol = Uuid::new_v4();
        w.context
            .user_devices
            .insert(carol, vec!["carol-phone".into()]);

        let targets = resolve(&w.context, Scope::Global, Uuid::nil(), w.alice);

        assert!(targets.contains(&"alice-phone".to_string()));
        assert!(targets.contains(&"bob-phone".to_string()));
        assert!(targets.contains(&"my-phone".to_string()));
        assert!(
            !targets.contains(&"carol-phone".to_string()),
            "relaying Alice's update to Carol would disclose a tie Alice never made"
        );
    }

    #[test]
    fn custom_scope_uses_its_stored_address_list() {
        let mut w = world();
        let scope_id = Uuid::new_v4();
        w.context.custom_scopes.insert(
            scope_id,
            vec!["alice-phone".into(), "bob-phone".into(), "my-laptop".into()],
        );

        let targets = resolve(&w.context, Scope::Custom, scope_id, w.me);

        assert_eq!(targets.len(), 2);
        assert!(targets.contains(&"alice-phone".to_string()));
        assert!(targets.contains(&"bob-phone".to_string()));
        // Still never ourselves.
        assert!(!targets.contains(&"my-laptop".to_string()));
    }

    #[test]
    fn revoked_and_encrypted_devices_are_excluded() {
        let mut w = world();
        w.context.revoked.insert("alice-desktop".into());
        w.context.encrypted.insert("bob-phone".into());

        let targets = resolve(&w.context, Scope::User, w.alice, w.me);
        assert!(targets.contains(&"alice-phone".to_string()));
        assert!(
            !targets.contains(&"alice-desktop".to_string()),
            "a revoked device must never receive anything"
        );

        let global = resolve(&w.context, Scope::Global, Uuid::nil(), w.me);
        assert!(
            !global.contains(&"bob-phone".to_string()),
            "encrypted devices receive frames through the encrypted path only"
        );
    }

    #[test]
    fn targets_are_deduplicated() {
        let mut w = world();
        let group = Uuid::new_v4();
        // The same user listed as both member and invitee.
        w.context.group_members.insert(group, vec![w.alice]);
        w.context.group_invitees.insert(group, vec![w.alice]);

        let targets = resolve(&w.context, Scope::GroupWithInvites, group, w.me);
        assert_eq!(targets.len(), 2);
        assert_eq!(
            targets.iter().filter(|a| *a == "alice-phone").count(),
            1,
            "a device must be written to once per frame"
        );
    }

    #[test]
    fn an_unknown_group_resolves_to_nothing() {
        let w = world();
        assert!(resolve(&w.context, Scope::Group, Uuid::new_v4(), w.me).is_empty());
    }
}

//! The backdating defence, driven over frames instead of struct fields.
//!
//! The unit tests in `consensus::stack` prove the *resolution rule* — that a
//! confirmed update displaces an unconfirmed one — by pushing confirmations
//! straight into `UpdateGroup::confirmations`. They say nothing about whether a
//! device ever produces one, which is exactly where the port had a hole: with
//! nothing minting confirmations, the rule could never fire and a backdated
//! update won every time.
//!
//! So these tests run the whole loop. A device recomputes its group, works out
//! what it owes a signature for, mints it, and hands back the bytes that go on
//! the wire. Another device decodes those bytes, checks the signature, resolves
//! the signer to a user, stores it, and — only because of that — comes to a
//! different conclusion about who runs the group.
//!
//! The cast throughout: Ada and Boris are the two admins, Cleo, Dev and Eve are
//! ordinary members. Ada demotes Boris; Boris answers with a demotion of Ada
//! backdated to sort ahead of hers. Five users means three confirmations are a
//! majority.

use libbounce::consensus::{self, confirmation, CanonicalStack};
use libbounce::crypto::DeviceKey;
use libbounce::error::Result;
use libbounce::frames::group::{Confirmation, Group, GroupCreation, UpdateGroup};
use libbounce::frames::identity::{join_uuid_list, Device, User};
use libbounce::frames::{Broadcastable, SignedFrame};
use libbounce::msgpack;
use libbounce::signed::SignedContainer;
use libbounce::store::Store;
use libbounce::types::UpdateGroupType;
use libbounce::Error;
use uuid::Uuid;

/// A person and the single device they act through.
struct Participant {
    user: User,
    key: DeviceKey,
}

impl Participant {
    fn new(name: &str) -> Self {
        let id = Uuid::new_v4();
        let key = DeviceKey::generate();
        let mut user = User::new(id, name.to_string());
        user.devices
            .push(Device::new(Uuid::new_v4(), id, key.address(), 1_000));
        Participant { user, key }
    }
}

/// Found a group with everyone already in it.
///
/// A real group starts with one member and grows by invitation, which the
/// engine tests cover; here the membership is fixed and what is under test is
/// what happens to the admin list afterwards.
fn found_group(members: &[Participant], admins: &[usize]) -> GroupCreation {
    let group = Group {
        name: "Consensus".into(),
        created_by: members[0].user.id,
        created_at: 1_000,
        users: members.iter().map(|m| m.user.clone()).collect(),
        admins: join_uuid_list(
            &admins
                .iter()
                .map(|&index| members[index].user.id)
                .collect::<Vec<_>>(),
        ),
        ..Default::default()
    };

    let mut creation = GroupCreation::create(&group, 1_000).expect("encodes the founding state");
    let container = SignedContainer::create(
        &members[0].key,
        msgpack::to_vec(&creation).expect("encodes the creation record"),
    );
    creation.signed = SignedFrame::from_container(&container);
    creation
}

/// Build an update and return it alongside the bytes that travel.
fn signed_update(
    actor: &Participant,
    group: Uuid,
    kind: UpdateGroupType,
    data: Vec<u8>,
    timestamp: i64,
) -> (UpdateGroup, Vec<u8>) {
    let mut update = UpdateGroup::new(actor.user.id, group, kind, data, timestamp);
    let container = SignedContainer::create(
        &actor.key,
        msgpack::to_vec(&update).expect("encodes the update"),
    );
    update.signed = SignedFrame::from_container(&container);
    let wire = container.encode().expect("encodes the container");
    (update, wire)
}

/// One device: its database, its key, and the user it speaks for.
struct Node {
    store: Store,
    user: Uuid,
    key: DeviceKey,
    group: Uuid,
}

impl Node {
    /// Everyone in the group already knows everyone else's device group, which
    /// the introduction handshake would otherwise have established.
    fn new(me: &Participant, creation: &GroupCreation, everyone: &[Participant]) -> Node {
        let store = Store::in_memory().expect("opens a database");
        for person in everyone {
            store.save_user(&person.user).expect("saves a contact");
        }
        store
            .save_group_creation(creation)
            .expect("saves the founding record");

        let node = Node {
            store,
            user: me.user.id,
            key: me.key.clone(),
            group: creation.id,
        };
        node.settle(0);
        node
    }

    /// Take an update off the wire the way the engine's `handle_update_group`
    /// does: verify the container, keep the bytes the signature covers, store
    /// it, and let consensus decide later whether it is canonical.
    fn receive_update(&self, wire: &[u8], now: i64) {
        let container = SignedContainer::unpack(wire).expect("the update verifies");
        let mut update: UpdateGroup = container.decode_payload().expect("the update decodes");
        update.signed = SignedFrame::from_container(&container);
        update.saved_at = now;
        self.store
            .save_update_group(&update)
            .expect("saves the update");
    }

    fn stack(&self) -> CanonicalStack {
        let creation = self
            .store
            .group_creation(self.group)
            .expect("reads the founding record")
            .expect("the founding record is present");
        let updates = self
            .store
            .updates_for_group(self.group)
            .expect("reads the updates");
        consensus::recompute(&creation, &updates, self.user).expect("recomputes the group")
    }

    /// The engine's `recompute_group`: rebuild the state, write it, then mint,
    /// save and hand back the confirmations this device now owes.
    ///
    /// The order matters — a confirmation is broadcast to the group as the
    /// recomputed state defines it, so the state has to be written first.
    fn settle(&self, now: i64) -> Vec<Vec<u8>> {
        let stack = self.stack();
        let state = stack.top().expect("the stack is never empty");

        let mut group = self
            .store
            .group_creation(self.group)
            .unwrap()
            .unwrap()
            .group()
            .expect("decodes the founding state");
        group.id = self.group;
        group.admins = join_uuid_list(&state.admins);
        group.invites = join_uuid_list(&state.invites);
        group.users = state
            .users
            .iter()
            .filter_map(|id| self.store.user(*id).expect("reads a member"))
            .collect();
        self.store.save_group(&group).expect("saves the group");

        let mut wire = Vec::new();
        for update in confirmation::owed(&stack, self.user, &self.key.address())
            .expect("works out what is owed")
        {
            let minted = confirmation::mint(update, self.user, &self.key, now);
            self.store
                .save_confirmation(&minted)
                .expect("saves the confirmation");
            wire.push(minted.payload().expect("encodes the confirmation"));
        }
        wire
    }

    /// The engine's `handle_confirmation`: everything the frame does not carry
    /// is derived here rather than believed.
    fn receive_confirmation(&self, wire: &[u8], now: i64) -> Result<()> {
        let mut received: Confirmation = msgpack::from_slice(wire)?;

        received.author = confirmation::attribute(&received, |address| {
            self.store.device_owner(address).ok().flatten()
        })?;

        // A confirmation for an update we do not hold yet would be stored
        // pending, which is the engine's problem; here the update always
        // precedes it.
        let update = self
            .store
            .update_group(received.update_group_id)?
            .ok_or(Error::InvalidFrame("no such update group".into()))?;
        received.destination = update.target;
        received.custom_scope = update.custom_scope;

        let group = self.store.group(update.target)?.ok_or(Error::GroupNotFound)?;
        if !confirmation::author_may_confirm(&group, received.author) {
            return Err(Error::InvalidFrame(
                "confirmation from outside the group".into(),
            ));
        }

        received.saved_at = now;
        self.store.save_confirmation(&received)?;
        Ok(())
    }

    fn is_admin(&self, user: Uuid) -> bool {
        self.stack().top().unwrap().is_admin(user)
    }

    fn accepted(&self) -> Vec<Uuid> {
        self.stack()
            .accepted_updates()
            .iter()
            .map(|update| update.id)
            .collect()
    }
}

/// The shared setup: five people, a group, and the two conflicting updates.
struct Scenario {
    people: Vec<Participant>,
    creation: GroupCreation,
    honest: (UpdateGroup, Vec<u8>),
    forgery: (UpdateGroup, Vec<u8>),
}

fn scenario() -> Scenario {
    let people: Vec<Participant> = ["Ada", "Boris", "Cleo", "Dev", "Eve"]
        .iter()
        .map(|name| Participant::new(name))
        .collect();
    let creation = found_group(&people, &[0, 1]);

    // Ada demotes Boris, honestly, at t=2000.
    let honest = signed_update(
        &people[0],
        creation.id,
        UpdateGroupType::DemoteAdmin,
        people[1].user.id.as_bytes().to_vec(),
        2_000,
    );

    // Boris answers by demoting Ada, backdated so that replaying in timestamp
    // order puts his update first — the whole attack in one field.
    let forgery = signed_update(
        &people[1],
        creation.id,
        UpdateGroupType::DemoteAdmin,
        people[0].user.id.as_bytes().to_vec(),
        1_500,
    );

    Scenario {
        people,
        creation,
        honest,
        forgery,
    }
}

#[test]
fn a_backdated_update_loses_to_confirmations_that_arrived_over_the_wire() {
    let scenario = scenario();
    let nodes: Vec<Node> = scenario
        .people
        .iter()
        .map(|person| Node::new(person, &scenario.creation, &scenario.people))
        .collect();
    let (cleo, dev, eve) = (&nodes[2], &nodes[3], &nodes[4]);

    // Ada's demotion reaches the three ordinary members first, and each of them
    // signs it as a matter of course.
    let mut circulating = Vec::new();
    for node in [cleo, dev, eve] {
        node.receive_update(&scenario.honest.1, 2_010);
        circulating.push(node.settle(2_020));
    }
    assert_eq!(
        circulating[0].len(),
        1,
        "seeing someone else's update should produce exactly one confirmation"
    );

    // Cleo's and Dev's confirmations reach Eve. With her own that is three of
    // five users: a majority.
    for wire in circulating[0].iter().chain(circulating[1].iter()) {
        eve.receive_confirmation(wire, 2_030)
            .expect("a genuine confirmation is accepted");
    }

    // Only now does Boris's forgery arrive. It sorts first, so replaying puts
    // Ada's demotion up against a state in which she is no longer an admin —
    // the point at which the confirmations decide the outcome.
    eve.receive_update(&scenario.forgery.1, 2_040);

    assert!(
        eve.is_admin(scenario.people[0].user.id),
        "the honest admin must survive a backdated demotion"
    );
    assert!(!eve.is_admin(scenario.people[1].user.id));

    let accepted = eve.accepted();
    assert!(accepted.contains(&scenario.honest.0.id));
    assert!(
        !accepted.contains(&scenario.forgery.0.id),
        "the backdated update should have been unwound, not merely outvoted"
    );

    // And the forgery earns no confirmation of its own: it never became
    // canonical, so there is nothing to attest to.
    assert!(eve.settle(2_050).is_empty());
}

#[test]
fn one_confirmation_is_not_a_majority() {
    // The same attack against a device that saw Ada's update but never received
    // anyone else's signature for it. This is what every Rust node did before
    // confirmations were broadcast, and it is why a Go peer in the same group
    // computed a different admin list.
    let scenario = scenario();
    let eve = Node::new(&scenario.people[4], &scenario.creation, &scenario.people);

    eve.receive_update(&scenario.honest.1, 2_010);
    assert_eq!(
        eve.settle(2_020).len(),
        1,
        "Eve signs what she sees, but one of five is not a majority"
    );

    eve.receive_update(&scenario.forgery.1, 2_040);

    assert!(!eve.is_admin(scenario.people[0].user.id));
    assert!(
        eve.is_admin(scenario.people[1].user.id),
        "unopposed, the earlier timestamp wins — which is the attack succeeding"
    );
}

#[test]
fn a_tampered_confirmation_is_refused_and_does_not_count() {
    let scenario = scenario();
    let nodes: Vec<Node> = scenario
        .people
        .iter()
        .map(|person| Node::new(person, &scenario.creation, &scenario.people))
        .collect();
    let (cleo, dev, eve) = (&nodes[2], &nodes[3], &nodes[4]);

    for node in [cleo, dev, eve] {
        node.receive_update(&scenario.honest.1, 2_010);
    }
    let from_cleo = cleo.settle(2_020).remove(0);
    let from_dev = dev.settle(2_020).remove(0);
    eve.settle(2_020);

    // Boris re-signs Cleo's confirmation with his own key, keeping her address
    // on it. The signature no longer matches the address it claims.
    let mut tampered: Confirmation = msgpack::from_slice(&from_cleo).unwrap();
    tampered.signature = scenario.people[1]
        .key
        .sign(tampered.update_group_id.as_bytes())
        .to_vec();
    let tampered = tampered.payload().unwrap();

    assert!(matches!(
        eve.receive_confirmation(&tampered, 2_030),
        Err(Error::InvalidSignature)
    ));
    eve.receive_confirmation(&from_dev, 2_030)
        .expect("Dev's is genuine");

    eve.receive_update(&scenario.forgery.1, 2_040);

    // Two of five is not a majority, so the forgery stands. Dropping the
    // forged confirmation is what makes the difference between this and the
    // first test.
    assert!(!eve.is_admin(scenario.people[0].user.id));
    assert!(eve.is_admin(scenario.people[1].user.id));
}

#[test]
fn the_wire_frame_carries_no_author_or_destination() {
    // Go marshals a confirmation with `msgpack:"-"` on Destination, Author and
    // CustomScope (chat/confirmation.go:20-24): a vote is attributed to
    // whoever's key signed it, not to whoever the frame says. Both sides have
    // to agree on that, or the counting differs.
    let scenario = scenario();
    let cleo = Node::new(&scenario.people[2], &scenario.creation, &scenario.people);
    cleo.receive_update(&scenario.honest.1, 2_010);
    let wire = cleo.settle(2_020).remove(0);

    let decoded: Confirmation = msgpack::from_slice(&wire).expect("decodes as a confirmation");
    assert_eq!(decoded.update_group_id, scenario.honest.0.id);
    assert_eq!(decoded.signing_device, scenario.people[2].key.address());
    assert!(
        decoded.author.is_nil() && decoded.destination.is_nil(),
        "neither the author nor the destination travels"
    );

    let author = confirmation::attribute(&decoded, |address| {
        cleo.store.device_owner(address).ok().flatten()
    })
    .expect("the signature verifies against the address that signed it");
    assert_eq!(author, scenario.people[2].user.id);

    // A stranger's confirmation is not kept, however well-formed.
    let outsider = Participant::new("Mallory");
    let mut theirs: Confirmation = msgpack::from_slice(&wire).unwrap();
    theirs.id = Uuid::new_v4();
    theirs.signing_device = outsider.key.address();
    theirs.signature = outsider.key.sign(theirs.update_group_id.as_bytes()).to_vec();
    assert!(matches!(
        cleo.receive_confirmation(&theirs.payload().unwrap(), 2_030),
        Err(Error::DeviceNotFound)
    ));
}

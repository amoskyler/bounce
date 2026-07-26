# Parity backlog

The Rust core and Electron client (`rust/`, `electron/`) speak the same protocol
as the Go implementation (`chat/`, `ui/`) and are correct on the paths they
cover: frames encode and verify against real Go output in both directions, group
consensus converges, and the [status section](rust-electron.md#status) of the
port's design document lists what is verified end to end. What follows is the
audit of everything else — twenty-six places where the port either does less
than `chat/` does, or does it somewhere the interface cannot reach. Three
themes account for most of it. **`handle_frame`'s catch-all arm**
(`rust/bounce-core/src/engine/mod.rs:2328`) silently drops six frame types that
Go handles, so profile changes, device revocations, settings and drafts arrive
and evaporate. **`UserView`** (`rust/bounce-core/src/engine/event.rs:21-33`) is
missing four fields the `users` row already stores, so settings the engine
persists correctly are invisible to the renderer and read back as their
defaults. And **the port collapsed Go's two lists** — the contact store and the
open-conversation list — **into one sidebar**, which removes the only route back
to a contact once they leave it. None of this breaks the wire: a Go peer and an
Electron peer interoperate, and every gap below fails closed rather than leaking.

Severity is about user harm, not effort. `critical` means the product's headline
feature is broken; `high` means data or a privacy control the user explicitly set
is silently lost; `medium` means an expectation is violated visibly; `low` means
cosmetic or unreachable today. "User-visible" asks whether someone using the
Electron client can observe it without a debugger.

## The gaps

| ID | Area | Title | Severity | Visible | One line |
|---|---|---|---|---|---|
| [P1](#p1) | engine | Users met through a group invitation are never persisted | critical | yes | Groups of three or more silently drop members you have not paired with |
| [P2](#p2) | consensus | Confirmations are never broadcast | high | no | The backdating defence is dead code, and Go and Rust peers diverge |
| [P3](#p3) | engine | Retention is written but never enforced | high | yes | Disappearing messages never disappear, and the UI shows a timer |
| [P4](#p4) | engine | Device revocation never propagates | high | no | A contact's stolen device stays trusted and keeps receiving traffic |
| [P5](#p5) | files | An attachment sent to an offline peer is lost | high | yes | `File` is missing from the catch-up dispatcher; the bubble arrives empty |
| [P6](#p6) | files | Attachment bytes are never deleted | high | no | Clear-history leaves every photo recoverable from `bounce.db` |
| [P7](#p7) | ui | Blocking a contact is irreversible | high | yes | The Unblock button lives inside a panel that blocking unmounts |
| [P8](#p8) | ui | A DM's retention control always reads "Off" | high | yes | `UserView` carries no retention, so the select is misseeded and inert |
| [P9](#p9) | ui | Contact notes are write-only, and a blur erases them | high | yes | The textarea seeds from `''` and writes that back on blur |
| [P10](#p10) | contacts | Conversation visibility is neither read nor settable | medium | yes | Every known user is an unconditional sidebar row; nothing can hide one |
| [P11](#p11) | engine | Group-creation users emit no `UserAdded` | medium | yes | A group founder you have not met renders as "Unknown" until restart |
| [P12](#p12) | engine | No `update_users` table | medium | yes | A contact's name is written once at introduction and never again |
| [P13](#p13) | engine | `update_dms` stores 2 of 10 update types | medium | no | State is no longer reconstructible by replay; retention offers vanish |
| [P14](#p14) | engine | Multi-device pairing is not implemented | medium | yes | A second device cannot join a profile; settings sync depends on it |
| [P15](#p15) | engine | Undeliverable marking never runs | medium | yes | A message that never lands is never marked, and is re-offered forever |
| [P16](#p16) | ui | No per-conversation receipt or typing override | medium | yes | Go's Advanced Options accordion has no counterpart |
| [P17](#p17) | ui | Network and device-revoked banners are never shown | medium | yes | `networkOnline` is stored and read by nothing; `deviceRevoked` is dropped |
| [P18](#p18) | ui | Avatars are never displayed and cannot be set | medium | yes | `images` reaches the renderer; `Avatar` has no image code path |
| [P19](#p19) | ui | The timeline always jumps to the newest message | medium | yes | No scroll-to-first-unread, no jump-to-bottom, and bulk read on select |
| [P20](#p20) | engine | `accepted` is dead, and auto-join is unported | low | yes | A setting the UI cannot reach gates a policy nothing consults |
| [P21](#p21) | ui | `last_opened` is persisted but never written | low | yes | A thread with a draft sinks in the sidebar instead of being pinned |
| [P22](#p22) | ui | You can invite yourself to a new group | low | yes | The picker is derived from conversations, which include note-to-self |
| [P23](#p23) | engine | Drafts never sync | low | no | Outbound is signed and broadcast; there is no inbound handler |
| [P24](#p24) | engine | Encrypted devices: frames exist, flows do not | low | no | ~3,100 lines of Go with no counterpart; opt-in, availability only |
| [P25](#p25) | ui | Device rename is plumbed but has no control | low | yes | Every layer down to the engine works; nothing calls it |
| [P26](#p26) | ui | No QR code or copy button for the pairing code | low | yes | The code is selectable text in a dialog that says to show it to someone |

---

## Critical

### P1

**Users met through a group invitation are never persisted**

The port derives all group state from the store, so a member it never stored is
not merely missing a contact card — they are erased from the group.

Go creates the user row as a side effect of consensus. `setGroupStateInDatabase`
runs every user in the resolved state through `createNewUserIfNeeded`
(`chat/consensus_store.go:680`), which validates the incoming device group,
inserts each device, stamps `IntroductionMethod = userIntroductionGroup` and
`IntroductionMetadata = groupID` (`chat/consensus_store.go:1044-1046`), saves the
user, and calls `UserConnectionDesired` to start dialling them
(`chat/consensus_store.go:1055`).

The port decodes the invitee's `User` in three places and stores it in none:
`consensus/stack.rs:167-175` registers their devices in the in-memory
`address_map`, `consensus/state.rs:338-356` records the addresses in
`GroupState::devices`, and `engine/system.rs:137-143` takes the subject id for a
status row. `recompute_group` then rebuilds membership with `if let Some(user) =
self.store.user(*member)?` (`rust/bounce-core/src/engine/mod.rs:707-712`),
silently skipping anyone absent. `types.rs:273`'s `introduction::GROUP` has no
assignment anywhere in the crate.

Four consequences follow from that one filter, and all of them persist across
restarts because `save_group` rewrites `group_users` from the filtered list
(`store/mod.rs:1123-1133`): the member is missing from the member list; their
group messages fail `signer_speaks_for` (`engine/mod.rs:2350-2354`) because their
devices were never saved, so the message is discarded before the membership check
is reached; `peering.rs:259-273` never dials them; and `peer_may_have`
(`engine/mod.rs:2651-2657`) denies them history. In a group of three where two
people have not paired in person, each is invisible to the other and their
messages are dropped in both directions.

The existing suite does not catch this because
`a_group_message_reaches_every_member` (`tests/engine_e2e.rs:243`) pre-seeds both
stores through the `introduce` helper (`tests/engine_e2e.rs:67-83`), which
bypasses the missing path entirely.

**Done** — `handle_update_group` decodes the `InviteUser` payload and persists the
carried `User` and devices before `recompute_group`, gated on
`device_group::user_has_valid_device_group` and the same "device claims a
different owner" check `apply_add_user` uses (`engine/mod.rs:1005-1013`), stamping
`introduction::GROUP` and the group id, and emitting `Event::UserAdded`. A test
covering three engines where two are strangers.

**Size** — M

---

## High

### P2

**Confirmations are never broadcast**

Timestamps are forgeable, so the design's answer to an admin backdating an update
is confirmations: devices broadcast a signature for each valid update they see,
and the earlier of two conflicting updates wins *unless* the later one has
majority confirmation. `consensus/stack.rs:12-30` and
[the design document](rust-electron.md#group-consensus) both present this as the
centrepiece. The port implements the consuming half and none of the producing
half, so the control is inert.

Go mints one for every newly-canonical update the user was a member for, skipping
its own updates, blocks and invite responses (`chat/consensus_store.go:253-258`).
`sendConfirmation` (`chat/confirmation.go:201`) de-duplicates against the local
device, signs the update's id, stores the row and broadcasts at group scope;
`handleConfirmation` (`chat/confirmation.go:90`) ingests peers' copies, including
ones that arrive before the update they refer to; `getConfirmationsToOffer`
(`chat/reference_offer.go:767`) puts them in the reference flow.

In the port, `Confirmation` exists with a full `Broadcastable` impl
(`frames/group.rs:209,237`) and `store/mod.rs:1061`'s `save_confirmation` has no
caller outside its own definition. `recompute_group`
(`rust/bounce-core/src/engine/mod.rs:673`) writes state and emits `GroupUpdated`
without minting anything. `handle_frame` has no `FrameType::Confirmation` arm, so
type 19 lands in the catch-all at `engine/mod.rs:2328`.
`references_not_delivered_to` (`store/mod.rs:1275-1291`) lists seven source tables
and not confirmations, and `has_frame` returns `false` for the type.

With the confirmation list permanently empty, `is_confirmed_by_majority` at
`consensus/stack.rs:247` always fails and `resolve_conflict` is unreachable. The
backdated forgery has the earlier timestamp, so it is applied first, and the
honest later update — the one that needs majority confirmation to unwind it —
never gets it. On a Rust node the backdating attacker wins every time. A Go peer
in the same group accumulates confirmations and unwinds the forgery, so the two
compute different admin sets for the same group with no server to reconcile them.

There is a second, quieter cost: because `has_frame` says `false`, a Rust node
classifies every Go-offered confirmation as wanted, requests it, receives it, and
drops it without acking. Go only writes the delivery record on ack
(`chat/ack.go:47`), so the same confirmation is re-offered on every reference
cycle, permanently.

`the_backdating_attack_fails_when_the_honest_update_has_majority`
(`consensus/stack.rs:359-368`) passes because it injects confirmations directly
into the struct — the unit is tested, the system is unwired.

**Done** — `recompute_group` walks the accepted stack, skips our own updates and
`Block`/`RespondToInvite`, and mints, saves and broadcasts a `Confirmation` for
each update not already confirmed by our address. `handle_confirmation` verifies
the signature against the signing device, checks the author is a member, stores,
acks and relays. `confirmations` added to `references_not_delivered_to`,
`has_frame`, `frame_payload` and a `peer_may_have` arm. A test that reaches
`resolve_conflict` through the wire rather than through struct injection.

**Size** — M

---

### P3

**Retention is written but never enforced**

Both ends agree on a retention policy that neither local store honours, and the
interface affirms it with a timer icon on every message.

Go prunes on start-up and schedules the rest. `pruneDirectMessages`
(`chat/database.go:161`) batch-deletes `delete_at != 0 AND delete_at < now`, then
spawns `deleteDirectMessageAt` per future expiry
(`chat/database.go:170-177`, `chat/direct_message.go:619-637`), which also calls
`ui.DeleteItem` so the message leaves the screen. `pruneGroupMessages`
(`chat/database.go:209`) mirrors it, both run from `GetInitialState`
(`chat/database.go:267-272`), and an inbound message already past its `DeleteAt`
is refused before the insert (`chat/direct_message.go:236-244`,
`chat/group_message.go:253-261`).

The port writes the field and never reads it back for anything but display.
`message.delete_at = crate::now() + retention` at
`rust/bounce-core/src/engine/mod.rs:410` and `:450`, and the attachment
equivalents at `engine/files.rs:101,171`. There is no `WHERE delete_at` query in
the crate, no trigger in `store/schema.rs`, and the single `DELETE FROM` against a
message table (`store/mod.rs:914`, inside `delete_messages_before`) is called only
from the two clear-history paths (`engine/mod.rs:1286`, `:1445`). `initial_state`
does no pruning, the only periodic task is `expire_typing_indicators`
(`engine/mod.rs:2047`), and neither `handle_direct_message` nor
`handle_group_message` checks `delete_at`, so a message that expired in transit is
stored permanently.

What makes this worse than an unimplemented feature is that everything around it
works. The retention control is reachable (`SettingsPanel.tsx:335` through
`bounce-node/src/lib.rs:495`), it propagates over the wire and through consensus
(`consensus/state.rs:381`, e2e-tested at `tests/engine_e2e.rs:1498`),
`Conversation.tsx:532` renders a timer on every message with `expiresAt > 0`, and
`Event::MessageDeleted` is declared "A message expired or was deleted"
(`engine/event.rs:193`) and handled in the renderer (`state.ts:300`). The expiry
half of its own docstring is dead. Plaintext the user believes was destroyed sits
in SQLite forever.

**Done** — `store.delete_expired_messages(now) -> Vec<Uuid>` covering both message
tables and the orphaned files and chunks, called from `initial_state` and from a
sweep spawned beside `run_typing_expiry`, emitting `Event::MessageDeleted` per
row; inbound messages whose `delete_at` has already passed are refused, matching
`chat/direct_message.go:236`.

**Size** — M

---

### P4

**Device revocation never propagates, and keys never roll**

The consuming side of revocation is fully and correctly ported. The write path is
missing, so no device already in the database can ever move from trusted to
revoked.

Go's `RevokeDevice` (`chat/update_device.go:490`) refuses another user's device, an
already-revoked one, and the last one, broadcasts an `updateDevice` of type
`Revoke` at **global** scope (`chat/update_device.go:56-58`), then calls
`rollKeys` (`:548`). The frame is a stored row in `update_devices`
(`chat/database.go:33`), replayed in catch-up (`chat/catch_up.go:24`).
`updateDeviceState` (`chat/update_device.go:241`) replays every update in
timestamp order, applies the earliest revocation, writes `devices.revoked_at`,
marks the address in the pool, pushes the revoke frame at the revoked device, and
calls `revokeUnauthorizedDeviceActions` (`:363`) which deletes the group
creations, group messages, update-groups and update-users the device signed after
its revocation and re-runs consensus. `rollKeys` (`chat/update_user.go:715`) mints
a fresh X25519 and Ed25519 pair, pushes them to encrypted devices signed with the
*old* key, and broadcasts `ReplaceKeys` and `ReplaceEcdhPublicKey`.

The port has no `update_devices` table in `store/schema.rs` and no
`FrameType::UpdateDevice` arm, so an inbound revocation reaches
`engine/mod.rs:2328` and is logged away. `Store::revoke_device`
(`rust/bounce-core/src/store/mod.rs:428`) exists with callers only at
`store/mod.rs:2226,2232,2244`, all tests. There is no `Engine::revoke_device`, no
`roll_keys`, no `revoke_unauthorized_device_actions`, and `frames/identity.rs:345`
`KeySet` is defined and never produced or consumed. `rename_device`
(`engine/settings.rs:225`) is explicitly local and broadcasts nothing.

Everything downstream of the flag already works, which is what makes the fix
narrow: `save_device` (`store/mod.rs:299-328`) persists `revoked_at` straight off
the wire with an upsert that adopts an incoming revocation and refuses to undo
one, `device_group.rs:144-155` rejects devices introduced by a revoked introducer,
`consensus/stack.rs:194-199` rejects updates signed after revocation,
`scope.rs:163` excludes revoked devices from every broadcast, `peering.rs:230`
refuses to dial them, and `signer_speaks_for` (`engine/mod.rs:2350-2364`) gates
authorship. So a contact adopted while a device of theirs is *already* revoked is
handled correctly; a contact who revokes one *later* is not, and there is no
second route in — `apply_add_user` replaces the wire device list with the locally
held one for a known contact (`engine/mod.rs:1058`).

The residual risk is the reachable one: a Rust node peered with a Go contact who
revokes a stolen laptop keeps trusting it, keeps accepting its group updates, and
keeps dialling it with the group's traffic, indefinitely. Self-revocation is
inapplicable until [P14](#p14) lands — Go itself refuses to revoke the last
device — and `rollKeys` has exactly one caller in the Go tree, `RevokeDevice`, so
key rotation follows this work rather than preceding it.

**Done** — an `update_devices` table (id, target, type, data, timestamp, saved_at,
author, signer, original_payload, signature) with a store save/load pair; a
`handle_update_device` that replays a target's updates in timestamp order, calls
the existing `Store::revoke_device`, emits an event, and pushes the frame at the
revoked address before dropping it; arms in both `handle_frame` and
`handle_frame_unlocked`; `revoke_unauthorized_actions` plus a `recompute_group`
re-run. `Engine::revoke_device` and `roll_keys` come with [P14](#p14).

**Size** — L

---

### P5

**An attachment sent to an offline peer is lost**

This is the ordinary case for a serverless system, and the loss is silent and
permanent.

`handle_frame_unlocked` (`rust/bounce-core/src/engine/mod.rs:2835-2845`) is the
only path catch-up uses, and it dispatches five frame types —
`DirectMessage`, `GroupMessage`, `GroupCreation`, `UpdateGroup`, `ReadReceipt` —
with `_ => Ok(())` for everything else. The full `handle_frame` *does* handle
`File`, so this is an omission in the catch-up dispatcher specifically rather than
an unimplemented feature. The sender genuinely offers and transmits the frame; the
receiver throws it away. The message bubble arrives and the attachment renders at
0% forever, since progress falls back to `unwrap_or(0.0)`
(`engine/files.rs:628,640`).

Go replays both halves: `typeFile` and `typeChunkOffer` are catch-up-eligible
(`chat/catch_up.go:27-28`), `chunkOffer` is a first-class signed frame
(`chat/file.go:344-357`) stored in `chunk_offers` (`chat/database.go:38`) and
served through the reference flow (`chat/reference_offer.go:1130-1246`,
`chat/reference_request.go:479-489`), and the chunk engine rebuilds its download
plan from the persisted offers on restart (`chat/chunk_engine.go:52-62`).

The port replaces `chunk_offers` with `chunk_locations (hash, address,
offered_at)` (`store/schema.rs:323-328`) — a local hint with no id, signer,
signature, file id, scope or retry timestamp — and says so:
`engine/files.rs:426-427` ("Offers are not stored as frames, so gossip is bounded
by novelty instead") and `files.rs:571-577` ("Chunk offers are ephemeral — they
are not stored, so the reference flow never replays them").

These are two defects, and they are separable. Adding
`FrameType::File => self.handle_file(...)` to the catch-up dispatcher turns
permanent loss into a one-reconnect delay: the metadata lands on the first
reconnect, but `resume_downloads` runs at connect time
(`engine/mod.rs:2241-2245`) *before* the reference offer arrives and only iterates
`incomplete_wanted_files`, so the bytes need a second connect. That second part is
a latency divergence, not data loss, and `chunk_locations` is itself persisted, so
restart-persistence is preserved — only cross-device replay of locations is lost.

**Done** — `File` (and `UpdateDm`, dropped by the same arm) handled in
`handle_frame_unlocked`, with an e2e test that sends an attachment to an offline
peer and asserts the bytes on reconnect. Then, separately, either a real
`chunk_offers` table carried by catch-up and the reference flow, or a
`resume_downloads` re-run after the catch-up bundle applies.

**Size** — S for the loss, M for full parity

---

### P6

**Attachment bytes are never deleted, including after clear-history**

The harm here is a privacy one rather than a disk one. `clear_history` is an
explicitly privacy-motivated, consensus-shared action, and after it every photo
and file ever sent in the thread is still recoverable from `bounce.db`.

Go never puts bytes in the database — `chunk.Data` is tagged `gorm:"-"`
(`chat/file.go:874`) and the bytes go to disk — and deletion cascades all the way
out to the filesystem: `directMessage.AfterDelete`
(`chat/direct_message.go:48-59`) deletes the attachment rows,
`fileAttachment`/`imageAttachment.AfterDelete` (`chat/message_attachment.go:17,35`)
delete the `file` row, and `file.AfterDelete` (`chat/file.go:84-109`) deletes the
chunks, the chunk offers, and calls `os.Remove(f.Path)`. Clear-history deletes per
row so the hooks fire with a populated primary key
(`chat/consensus_store.go:1294-1312`).

The port stores every byte in `chunks.data BLOB` (`store/schema.rs:304-313`),
written by `save_chunk` (`store/mod.rs:1486-1512`). `delete_messages_before`
(`store/mod.rs:890-919`) removes rows from `group_messages` and `direct_messages`
and nothing else. A case-insensitive scan of the store finds eight `DELETE`
statements and none touches `files`, `chunks`, `file_attachments` or
`image_attachments` in normal operation. `delete_at` is written on attachments
(`engine/files.rs:101,171`) and read back only to populate `expires_at` for the UI.

Two things are worth stating precisely, because they are easy to get wrong. The
`ON DELETE CASCADE` on `chunks.file_id` (`schema.rs:312`) is live —
`PRAGMA foreign_keys = ON` is set at `schema.rs:28` — and fires whenever a `files`
row is deleted. It is dead only because nothing deletes from `files`. And VACUUM
is not a parity gap: Go never vacuums either, and SQLite reuses freed pages, so
the file plateaus rather than growing without bound. The defect is that the bytes
are never freed at all.

**Done** — `delete_messages_before` collects the removed ids and deletes the
`files` rows they reference through `file_attachments` and `image_attachments`
(the chunk cascade then frees the blobs), followed by the attachment rows. The
same sweep runs on `delete_at` for [P3](#p3). `PRAGMA auto_vacuum = INCREMENTAL`
set at creation, since it cannot be enabled later without a full VACUUM, with
`PRAGMA incremental_vacuum` after a sweep.

**Size** — M

---

### P7

**Blocking a contact is irreversible in the client**

Contacts can only be established by an in-person single-use pairing code, and a
Bounce user has no directory to re-find someone through. Blocking is a one-way
destructive action with no in-app recovery, and the confirm text ("Their messages
will be refused rather than hidden", `DetailsPanel.tsx:150`) gives no hint of it.

Go blocks and closes the DM too — `chat/update_dm.go:382-383` sets
`open = !blocked` — which is precisely why it pairs blocking with a Contacts
screen. `ui/menu.go:71` wires a first-class menu button to `showNewDM`, the
contacts browser lists every known user with a "Show Blocked" checkbox
(`ui/new_dm_container.go:64-68`, filtered at `:94-96`), tapping a blocked row
re-opens the DM through `openAndPopulateDM` → `SetOpenDM(true)`
(`ui/direct_message.go:1290`), whose later timestamp wins the replay, and the
thread opens with the entry disabled (`ui/user.go:138-142`) and an Unblock button
(`ui/direct_message.go:727-745`).

The port inherited the disappearing conversation without the mechanism Go pairs
it with. `conversations()` drops blocked users at
`electron/src/renderer/state.ts:447` (`if (user.blocked) continue;`), and
`App.tsx:160-162` resolves the *selected* conversation out of that same filtered
list — so `selected` goes `undefined` the instant the `userUpdated` event lands,
`DetailsPanel` unmounts under the user's cursor (`App.tsx:336-342` is its only
render site), and the Unblock branch at `DetailsPanel.tsx:140-146` becomes dead
code. Search runs over the already-filtered list, group member rows are
non-interactive `div`s (`DetailsPanel.tsx:245-260`), and "Add contact" is a
pairing-code exchange, not a browser.

There is no out-of-band workaround either. `apply_add_user` preserves local state
when re-adopting a known contact, including `adopted.blocked = known.blocked`
(`rust/bounce-core/src/engine/mod.rs:1079-1086`), and `peering.rs:239` skips
dialling blocked users — so physically re-pairing with the person in front of you
does not restore them. Recovery means editing SQLite.

Nothing is lost or corrupted: the engine state is intact and one already-working
`setUserBlocked(id, false)` call away (`engine/mod.rs:1378-1380`,
`bounce-node/src/lib.rs:465-470`). This is purely UI reachability.

**Done** — either blocked users stay in the list in a muted style, or a contacts
surface lists `state.users` independently of `conversations()` with a "Show
blocked" toggle, as `new_dm_container.go` does. As an immediate stopgap,
`selectedConversation` is not cleared when the selected user becomes blocked.
Best done together with [P10](#p10), which needs the same contacts surface.

**Size** — S

---

### P8

**A direct conversation's retention control always reads "Off"**

Retention is a privacy control. Set it on a Fyne device and the Electron client
shows "Off" for the same conversation, so a user may believe DMs persist while
they are silently expiring — or the reverse.

Go seeds the selector from stored per-user state: `dm.retention` comes from
`bounceUser.State.Retention` (`ui/direct_message.go:188`), the selector is set
with `getRetentionName(dm.retention)` (`ui/direct_message.go:611-612`), and
`SetDMState` re-seeds and refreshes it on every change
(`ui/direct_message.go:1023-1027`).

The port renders `<ConversationSettings conversationId={user.id}
mutedUntil={user.mutedUntil} />` with no retention prop
(`electron/src/renderer/DetailsPanel.tsx:131`), and `ConversationSettings` falls
back to `value={retention ?? 0}` (`DetailsPanel.tsx:373`). It cannot be a
forgotten prop: the group path five lines up passes `retention={group.retention}`
(`DetailsPanel.tsx:310-314`), and the field is absent one layer lower too —
`UserView` (`rust/bounce-core/src/engine/event.rs:21-33`) carries `muted_until`
and no retention, while `GroupView` carries both, and `user_view`
(`engine/mod.rs:2851-2864`) never populates it despite `User` owning the field
(`store/mod.rs:82,111,1820`).

The control is not just misseeded, it is visually inert. As a controlled select
with no local state, choosing "1 week" fires `setRetention` and React snaps the
display straight back to "Off". The write does land — `engine/mod.rs:1399-1416`
routes non-group ids to `UpdateDmType::ChangeRetention`, applied at `:1326` and
persisted — but the `Event::UserUpdated` re-emitted afterwards carries a
`UserView` with no retention, so nothing can ever correct the display. The one
mitigation is that `engine/system.rs:213-224` emits a `retentionChanged` status
row into the timeline, so the change is not wholly invisible.

The comment at `DetailsPanel.tsx:346` ("Mute and retention, which work the same
for contacts and groups") shows this is an oversight rather than a scope-out.

**Done** — `retention` added to `UserView`, populated in `user_view`, carried
through the preload `User` type, and passed from `ContactDetails` the way
`GroupDetails` already passes `group.retention`.

**Size** — S

---

### P9

**Contact notes are write-only, and a blur silently erases them**

The engine persists notes correctly (`store/schema.rs:56`, `store/mod.rs:1832`).
The read path was never built, and the write is unguarded, so the two combine into
silent unrecoverable loss of user-authored text.

Go loads the stored note into the entry (`dm.notesEntry.SetText(dm.user.notes)`,
`ui/direct_message.go:765`), keeps it disabled until an explicit edit tap
(`:758-766`), and gives it separate save and cancel buttons — the save button is
the only caller of `SetUserNotes` (`:780-785`) and cancel restores the old value
(`:786-791`). `SetUserState` re-seeds the live widget when the note changes
elsewhere (`ui/user.go:131`).

The port initialises the textarea with a constant: `const [notes, setNotes] =
React.useState('')` (`electron/src/renderer/DetailsPanel.tsx:84`) — the line above
it reads `React.useState(user.alias)`, which is the pattern this one is missing.
It then writes on every blur, `onBlur={() => void window.bounce.setUserNotes(user.id,
notes)}` (`DetailsPanel.tsx:119-127`). Focusing the box and leaving it overwrites
the stored note with `""`.

There is no read path to seed from: `UserView` has `alias` and no `notes`
(`engine/event.rs:21-33`), `user_view` never populates one
(`engine/mod.rs:2851-2864`), and a repo-wide grep finds only the write chain
(`main/index.ts:190` → `main/engine.ts:310` → `bounce-node/src/lib.rs:485` →
`engine/mod.rs:1393`).

The erasure is not local. `set_user_notes` routes through `apply_update_dm` with
`UpdateDmType::SetNotes`, which is `Scope::Sync` and so syncs within the user's
own device group, and `leaves_a_record()` is false for it
(`frames/update.rs:307-312`), so the empty overwrite propagates with no record to
replay back from.

**Done** — `notes` added to `UserView` and the preload `User`, the textarea seeded
from it, and `setUserNotes` called only when the value actually changed.

**Size** — S

---

## Medium

### P10

**Conversation visibility is neither read nor settable**

Go keeps two lists — the contact store (`ui.users`) and the open-conversation
list (`ui.threads`) — and `open_dm` is what separates them. `ui/ui.go:459-465`
adds every user to the contact store but calls `NewDirectMessage(u)`, which does
`ui.threads.add` (`ui/direct_message.go:383`), only `if u.State.Open`. It is live
membership, not just a start-up filter: `SetDMState`
(`ui/direct_message.go:993-1017`) adds the thread when the flag turns on and
removes it when it turns off. `SetOpenDM` (`chat/update_dm.go:782-796`) builds and
broadcasts the sync-scoped update behind the "Hide" button
(`ui/direct_message.go:658-671`), so hiding on one device hides on all of them.

The port collapsed the two lists into one. `conversations()`
(`electron/src/renderer/state.ts:446-458`) iterates every user and pushes a
`direct` conversation for each, skipping only blocked ones, and nothing upstream
filters (`App.tsx:154-157` applies only the search box). It could not honour the
flag even if it wanted to: `UserView` has no `open_dm`
(`rust/bounce-core/src/engine/event.rs:21-33`) and `user_view` drops it
(`engine/mod.rs:2851-2864`), though the field is stored (`store/schema.rs:42`),
written (`store/mod.rs:107,223`), read back (`store/mod.rs:1777`), preserved
across re-adoption (`engine/mod.rs:1064`) and correctly mutated by inbound frames
(`engine/mod.rs:1326`).

There is also no producer. Every other DM setting has an engine method —
`set_muted_until` (`engine/mod.rs:1353`), `set_blocked`, `set_alias`,
`set_user_notes` (`:1393`), `set_retention` (`:1399`) — but `SetOpen` appears only
as a consumer. The napi surface has no equivalent, and there is no Hide
affordance and no contacts screen in the renderer.

This is reachable in ordinary use, not a corner case. Go creates group-met users
with `IntroductionMethod = userIntroductionGroup` and `OpenDM` false
(`chat/consensus_store.go:1044-1047`, `chat/user.go:395-402`), and the port
reproduces the storage side faithfully — `open_dm` is `#[serde(skip)]`
(`frames/identity.rs:233`), `User::new` defaults it false (`:285`), and the port's
own comment at `engine/mod.rs:1075-1077` reads "A scanned contact opens a
conversation straight away; one met through a group does not." So joining a
twenty-person group produces twenty phantom sidebar rows in Electron and none in
Go. The cross-device case is worse in a mixed group: a Go device hides a DM,
`SetOpen(false)` syncs, the Rust store applies it, and the Electron sidebar does
not move.

Nothing is lost and no frame is malformed — this is view composition at a single
chokepoint.

**Done** — `open_dm` added to `UserView` and populated in `user_view`;
`conversations()` filters on it while always keeping note-to-self and any user
with messages; `Engine::set_open_dm` mirroring `set_muted_until`, exposed through
`bounce-node` and preload and wired to a "Hide conversation" action; and the
contacts surface from [P7](#p7) for everyone the sidebar no longer shows.

**Size** — M

---

### P11

**Users learned from a group creation frame emit no `UserAdded`**

`handle_group_creation` saves each founding user with `self.store.save_user(user)?`
(`rust/bounce-core/src/engine/mod.rs:2528`) and emits no event. `Event::UserAdded`
comes from exactly one place, the add-user adoption path
(`engine/mod.rs:1101-1110`). The renderer's `state.users` is populated only by the
boot snapshot (`App.tsx:111-114`) and by `userAdded`/`userUpdated`
(`state.ts:359-363`), and there is no user-lookup IPC in `main/index.ts`, so an
unknown id can never be resolved mid-session. `initial_state` does list every
stored user, so it heals on restart.

The frame carries exactly one user, the creator (`engine/mod.rs:2452-2459`), so
this fires whenever an admin who is not the creator invites you, and again on
catch-up replay for a freshly linked device. The blast radius is wider than the
member list: `DetailsPanel.tsx:231-238` and `:264` fall back to the literal
`'Unknown'`, `Conversation.tsx:492` labels message bubbles with it, and
`App.tsx:73` puts it in desktop notification bodies.

Go has no equivalent problem, though not for the reason it first appears.
`handleGroupCreation` saves no users at all — they are created downstream by
`createNewUserIfNeeded` (`chat/consensus_store.go:1022`), which also emits no user
event. What saves Go is that `SetGroupState` (`chat/consensus_store.go:975-999`)
ships `Users` and `Invites` as full `User` structs with names and images, and the
Fyne UI folds them into its global cache (`ui/group.go:847-873`). Go learns the
name live, from the group-state event itself.

**Done** — `handle_group_creation` emits `Event::UserAdded` (or `UserUpdated` for
a user already known) after each `save_user`. The alternative — putting `UserView`s
rather than bare UUIDs in `GroupView.members` — is a larger change to
`group_view` (`engine/mod.rs:2857-2859`) and the preload `Group` type, and is
worth considering if [P1](#p1) is being done at the same time.

**Size** — S

---

### P12

**No `update_users` table: a contact's profile change never propagates**

The port ships the sending half of profile renames end to end and the receiving
half not at all, so the sender's UI updates, the sender gets no error, and every
Rust recipient silently discards the frame. A contact's name is written once, at
introduction (`engine/mod.rs:1162`), and never again.

Go's `updateUser` (`chat/update_user.go:42-53`) is a stored, signed, broadcast
frame with `PreviousData` carried specifically so the timeline can say what a name
changed *from*. The table is `update_users` (`chat/database.go:32`), it is
catch-up-eligible (`chat/catch_up.go:23`), `handleUpdateUser`
(`chat/update_user.go:137-243`) checks blocked author, revoked signer and
`signedByUser`, and `updateUserState` (`:349-601`) replays every stored row for
that target to recompute name, images, encrypted devices and keys.

In the port the frame is modelled fully, including `previous_data`
(`frames/update.rs:63-92`) with a correct `Broadcastable` impl (`:114-141`), and
`FrameType::UpdateUser = 20` exists — but there is no `update_users` table in
`store/schema.rs`, no `save_update_user` in the store, and no arm in `handle_frame`
(`engine/mod.rs:2328`) or `handle_frame_unlocked` (`:2835-2845`). It is also
absent from `references_not_delivered_to`, `has_frame`, `frame_payload` and
`frame_saved_at`, so the frame is never re-offered to a peer that was offline.
`FrameType::catch_up_order` already reserves `UpdateUser => 3` (`types.rs:142`) —
the scaffolding declares an intent the storage layer never fulfills.

Meanwhile `update_profile_name` (`engine/mod.rs:1579-1606`) updates the local row,
signs an `UpdateUser`, emits `Event::UserUpdated` locally and broadcasts it,
without ever persisting it.

Scope is narrower than the Go surface suggests: four of Go's seven update types
belong to encrypted-device management ([P24](#p24)) and key rolling
([P4](#p4)), and `UpdateImage` waits on [P18](#p18). The reachable consequence is
that a contact's display name goes stale forever. Note the Rust→Go direction
already works — the serde renames match Go's wire fields; only receipt and
persistence are missing.

**Done** — an `update_users` table mirroring `update_groups` with an index on
target; a `save_update_user`/`all_update_users` pair; a `handle_update_user` that
verifies `signer_speaks_for`, stores, replays the target's rows to recompute the
`users` row, and rebroadcasts; arms in both dispatch tables; and the frame added
to the four reference-flow lookups.

**Size** — M

---

### P13

**`update_dms` stores 2 of 10 update types**

Go stores all ten unconditionally (`chat/update_dm.go:270` inbound, `:570` local)
and *derives* per-conversation state — muted_until, clear_before, read receipts,
typing, open, alias, notes, blocked, retention — by replaying every row for the
thread in timestamp order (`chat/update_dm.go:337-401`) before writing the result
to the `users` row (`:486-498`). That replay is the only channel these fields
have; they are `msgpack:"-"` and never travel inside a user record
(`chat/user.go:41,50-52`).

The port persists two. `leaves_a_record()` is `ChangeRetention | SetClearBefore`
(`rust/bounce-core/src/frames/update.rs:307-312`) and gates `save_update_dm` on
both the local path (`engine/mod.rs:1221-1224`) and the inbound one (`:1272-1274`);
everything else is folded straight into the `users` row by
`apply_dm_setting_locally` (`engine/mod.rs:1298-1349`). So state is no longer
reconstructible by replay.

The wire half is self-inconsistent, and this is the part to fix. Non-shared kinds
are still broadcast with `Scope::Sync` (`frames/update.rs:376-383`), which
resolves to the user's own devices — and `handle_update_dm` rejects on kind alone,
before any sender check (`engine/mod.rs:1245-1250`), with the actor-identity check
that would recognise one of our own devices sitting unreachable at `:1256`. The
port emits frames it is guaranteed to refuse. `update_dms` are also missing from
`peer_may_have` (`engine/mod.rs:2636-2657` falls through to `_ => false`) even
though the store lists them as a reference source (`store/mod.rs:1290`), and from
`handle_frame_unlocked`, so one inside a catch-up is dropped.

Two things temper this. The blanket rejection is deliberate hardening and fixes a
real weakness in the reference — see [Deliberate divergences](#deliberate-divergences)
— and the headline symptom (mute one device, stay unmuted on another) is
unobservable until [P14](#p14) lands. What *is* reachable today is `OfferRetention`:
it is `is_shared()` so it is accepted on the wire, but it is never stored and
`apply_dm_setting_locally` ignores it (`engine/mod.rs:1341`), so Go's retention
negotiation (`chat/update_dm.go:404-483`), which depends on those rows, has no
counterpart.

**Done** — every kind stored, as Go does; the blanket `!kind.is_shared()`
rejection replaced by a check on whether the signer is one of our own devices, so
a non-shared kind is refused only when it comes from the counterparty;
`OfferRetention` stored and the negotiation implemented; `UpdateDm` added to
`peer_may_have` and `handle_frame_unlocked`.

**Size** — M

---

### P14

**Multi-device pairing is not implemented**

Listed in [rust-electron.md](rust-electron.md#not-started) as not started, and
disclosed in the client — `SettingsPanel.tsx:40-42` defines `PAIRING_PENDING`
("Multi-device pairing is not implemented yet. The frames exist in the core, but
nothing drives the flow that adopts or retires a second device"), the Revoke and
Add device buttons are hard-disabled (`:259,265`), and `:268-271` says in prose
that the profile lives on this device alone. It is here because it is the blocker
for five other rows, not because it is undocumented.

Go mints a five-minute secret and returns `address:secret` (`GetNewSyncString`,
`chat/sync_device_offer.go:31`). `RequestToSync` (`chat/sync_device_request.go:204`)
refuses when a profile exists, dials, and sends a `syncDeviceRequest` carrying the
joining device's signature of the offerer's address. `handleSyncDeviceRequest`
(`:43`) burns the secret, enforces the 300-second window, verifies the signature,
creates the device row with a mutual introduction signature (`:149-153`), replies
with the full profile *and the user's private ECDH and ECDSA keys and settings*
(`:170-178`), and broadcasts the new device globally (`:195`).
`handleSyncDeviceRequestAccepted` (`chat/sync_device_request_accepted.go:38`)
adopts the profile, validates the device group, generates a device ECDH key and
triggers a catch-up.

All four frames exist in the port (`frames/pairing.rs:216,238,267,277`) and encode
correctly. Nothing sends or handles them: `handle_frame`
(`rust/bounce-core/src/engine/mod.rs:2302-2332`) has no arm for types 7, 8, 9 or
10, so they reach the catch-all at `:2328`. `create_pairing_code`
(`engine/mod.rs:755`) reuses the `SyncDeviceOffer` row for the *contact* add-user
flow, so there is no device-pairing offer at all, and `bounce-node/src/lib.rs`
exposes no `request_to_sync`. `Onboarding.tsx:26-75` renders one form whose only
action is `createProfile`, with no second branch and — unlike the settings panel —
no hint that adoption is unavailable, so a user who already has a Go profile
elsewhere will create a second unrelated identity and appear to their contacts as
a stranger.

The absence is fail-safe on the security axis: `SyncDeviceRequestAccepted` is the
one frame carrying private keys, and with no handler no peer can push a profile
and key material into an Electron client.

Folded in here rather than filed separately, because each is a strict sub-item:
**profile settings never reaching other devices** (no `update_settings` table in
`store/schema.rs`; `Engine::update_settings`, `engine/settings.rs:166-178`, is a
private local mutator that signs, stores and broadcasts nothing; documented at
`engine/settings.rs:10-15` and in rust-electron.md), the **second-device
onboarding branch**, and the **inert Revoke and Add device buttons**.

**Done** — `create_sync_code()` writing a distinct offer row, kept separate from
the add-user offer so one code cannot be used for the other; `request_to_sync(code)`
refusing when a profile exists; handlers for types 7, 8, 9 mirroring
`sync_device_request.go:43` and `sync_device_request_accepted.go:38`, including
clearing delivery records on re-sync and broadcasting the new `Device` frame; a
`Device` handler gated on `device_group::is_valid_addition`; an `update_settings`
table with a sync-scoped frame emitted from each setter in `engine/settings.rs`;
and a second onboarding branch rendering the `syncStarted`/`syncProgress`/
`syncComplete` events already reduced at `state.ts:419-426`.

**Size** — L

---

### P15

**Undeliverable marking never runs, and reference offers have no age cutoff**

Go schedules `checkIfDirectMessageUndeliverableAt(now + 4 weeks, id)` on send
(`chat/direct_message.go:600` → `:639-701`) and the group equivalent
(`chat/group_message.go:621` → `:660`); each counts delivery records at the
deadline and, on zero, writes `undeliverable = true` and calls
`ui.MarkMessageUndeliverable`. Acks cancel the pending check
(`chat/ack.go:101,125`), and `chat/database.go:180-205,228-253` re-run the same
LEFT JOIN on start-up. Correspondingly, reference offers exclude anything older
than `undeliverableAfter` for non-sync peers (`chat/reference_offer.go:376,401-403`),
with no floor on the own-device branch.

The port has `mark_direct_message_undeliverable` (`store/mod.rs:603`) with zero
callers and no group equivalent. `crate::UNDELIVERABLE_AFTER_SECONDS`
(`lib.rs:67`) is referenced only by a doc comment on `DirectMessage::undeliverable`
(`frames/message.rs:82`). `send_direct_message` (`engine/mod.rs:401-427`) and
`send_group_message` (`:444-467`) save, emit, broadcast and return with no timer;
the only periodic job is `expire_typing_indicators`. There is no start-up sweep.
The `undeliverable` flag is plumbed all the way to the renderer and is always
`false`, and `Event::MessageUndeliverable` (`engine/event.rs:191`) is fully
handled downstream (`preload/index.ts:163`, `state.ts:289-296`,
`Conversation.tsx:551-553`) and never constructed anywhere in `rust/` — dead end
to end, not merely unplumbed.

`references_not_delivered_to` (`store/mod.rs:1275-1322`) builds its LEFT JOIN with
no `written_at` floor and no `undeliverable` predicate, and `peer_may_have`
filters by scope only, so a message that can never be delivered is re-offered on
every reconnection forever. That makes the port more eager rather than incorrect —
it keeps trying to deliver frames Go abandons — at the cost of per-peer offer
growth.

Nothing is lost: `undeliverable` is an advisory tick-mark, the message stays in
the store, and the field is local-only (`frames/message.rs:537`), so it never
crosses the wire. It also only manifests after four weeks of total non-delivery,
which is why no test window catches it.

**Done** — `mark_group_message_undeliverable` plus a
`store.mark_stale_messages_undeliverable(cutoff) -> Vec<Uuid>` doing Go's LEFT
JOIN, called from the same sweep as [P3](#p3), emitting the existing
`Event::MessageUndeliverable`; `AND written_at >= ?cutoff` added to the non-sync
branches of `references_not_delivered_to`.

**Size** — M

---

### P16

**No per-conversation read-receipt or typing-indicator override**

Both Go edit screens carry an "Advanced Options" accordion with two tri-state
selects — "Default (On)"/"Default (Off)", "On", "Off" — whose first label tracks
the global default (`ui/settings_container.go:288-308`). Groups:
`ui/edit_group_container.go:100-103,474-485`, written through
`SetGroupReadReceiptSettings`/`SetGroupTypingIndicatorSettings` (`:290-317`). DMs:
`ui/direct_message.go:615-618,961-972`, written at `:842-869`.

`ConversationSettings` (`electron/src/renderer/DetailsPanel.tsx:345-388`) renders
exactly two controls, mute and disappearing messages; the file contains no
occurrence of "receipts", "indicators" or "advanced". `setReadReceipts` and
`setTypingIndicators` are exposed with an explicit `boolean | null` signature
meaning "use the default" (`preload/index.ts:307-311`) and no renderer control
calls either.

The fix needs a view field, not just a widget: the preload `User` (`:38-49`) and
`Group` (`:52-67`) carry `mutedUntil` and `retention` and no override state, so
the renderer cannot read back the current setting. The backend below is complete
and faithful — `bounce-node/src/lib.rs:516-538` bridges both and
`engine/mod.rs:1452-1480` encodes `vec![override_flag, value]`, the same two-byte
shape Go writes.

Absent an override the port behaves exactly as Go does for any conversation the
user never touched, and the global toggles are present
(`SettingsPanel.tsx:351-368`), so a privacy-conscious user can still turn both off
everywhere. What is missing is per-conversation granularity.

**Done** — the override state added to `UserView`/`GroupView` and the preload
types, and two tri-state selects in `ConversationSettings` whose first option is
labelled from the global default, mapping to `setReadReceipts(id, null|true|false)`.

**Size** — M

---

### P17

**Network status, reconnection and device-revoked warnings are never shown**

Go keeps a banner above the thread list reading "network is starting..." at launch
(`ui/ui.go:358`, placed at `ui/main_container.go:158,180`), switching to "network
connection lost, reconnecting..." on `NetworkOffline` and hiding on
`NetworkOnline` (`ui/ui.go:860-886`), and replaced by a danger-styled "This device
has been revoked" when the initial state says so (`ui/ui.go:821-825`).

The port's only banners are the no-Tor warning and the dismissible error banner
(`electron/src/renderer/App.tsx:274-291`). `networkOnline` is folded into state
(`state.ts:194,232-236`) and read by no component; `deviceRevoked` arrives in the
snapshot (`preload/index.ts:135`) and is dropped by the `loaded` reducer, which
copies eight fields and not that one (`state.ts:189-201`). `.banner--offline` and
`.banner--syncing` are defined in `styles.css:1318,1322` with zero consumers,
which is where the wiring stopped.

It is dead one layer lower too: `initial_state` hardcodes `network_online: true,
device_revoked: false` (`rust/bounce-core/src/engine/mod.rs:378-379`), and
`Event::NetworkOnline`/`NetworkOffline` (`engine/event.rs:166-167`) are never
emitted anywhere in the Rust tree. So the flags are pinned even if a component
read them.

In Go this is a purely informational label — `NetworkOffline()` blocks nothing —
and the port still gives indirect feedback through delivery ticks
(`Conversation.tsx:550-578`) and per-contact presence dots (`Avatar.tsx:97`), and
does show the security-relevant no-Tor banner.

**Done** — the engine emits `NetworkOnline`/`NetworkOffline` and reports both
fields honestly from `initial_state`; the reducer keeps `deviceRevoked`; and a
banner renders from `state.networkOnline` with a distinct starting state before
the first event, plus a danger banner for revocation.

**Size** — S

---

### P18

**Avatars and group images are never displayed, and cannot be set**

The *setting* half is on the port's [not-started list](rust-electron.md#not-started)
and the initials fallback is a deliberate design choice
([rust-electron.md](rust-electron.md#the-interface)). The *display* half is
neither, and is the cheap and surprising part: the data already reaches the
renderer and is thrown away.

Go puts a 128px avatar with a file picker at the top of the profile editor,
size-checked against `chat.EmbeddedFileLimit`, calling `UpdateProfileImage`
(`ui/edit_profile_container.go:251-293`); the first-run creator does the same
(`ui/main_container.go:321-351`) and the group edit screen has the group twin
calling `SetGroupImage` (`ui/edit_group_container.go:39-95`). `defaultImage`
renders the stored image circle-cropped whenever one exists, falling back to
colour-plus-initials only when it does not (`ui/default_image.go:82-198`).

`Avatar` takes `{id, name, size, online, className}` and renders
`<span className="avatar__initials">` on a palette tint
(`electron/src/renderer/Avatar.tsx:81-100`) — no image branch, no `src`, at any of
its nine call sites. Yet `User.images` and `Group.images` are already there
(`preload/index.ts:43,55`) from `UserView.images` (`engine/event.rs:26`), and
`fileData(fileId)` is already exposed end to end (`main/engine.ts:39,260`,
`main/index.ts:162`, `preload/index.ts:256`) and used by
`renderer/attachment-data.ts:56`. A grep for `.images` across the renderer returns
nothing.

The receive path works: `handle_file` (`engine/files.rs:343-398`) never inspects
`file_type`, so a Go peer's embedded image is stored, chunk-fetched and readable,
and remote `SetImage` updates are applied to group state
(`consensus/state.rs:331-335`, persisted at `engine/mod.rs:696`). The port even
renders "changed the group image" status rows (`engine/system.rs:129`,
`SystemMessage.tsx:90-91`) — correctly, since they report what a Go peer did, and
the sentences are deliberately word-for-word with `ui/thread_item.go` so both
clients read the same history.

The send path is absent at every layer: `create_profile` takes only name and
device name (`engine/mod.rs:266`), `create_group` takes no image
(`engine/mod.rs:470`, `bounce-node/src/lib.rs:404`, `preload/index.ts:259`),
`stage_attachment` is the only file-creation path and hardcodes
`FileType::MessageAttachment` (`engine/files.rs:262`), and
`UpdateUserType::UpdateImage` is defined (`frames/update.rs:21`) and never
emitted. Note that setting a *user* image also depends on [P12](#p12), since
`FrameType::UpdateUser` has no inbound handler.

**Done** — split it. Display first: `Avatar` renders from `user.images` /
`group.images` through the existing `fileData`, with the initials circle as the
fallback. Then setting: `updateProfileImage` and `setGroupImage` through the
engine, napi and bridge, with pickers on the profile and group avatars and in the
new-group dialog, size-checked against `EMBEDDED_FILE_LIMIT`.

**Size** — M (S for display alone)

---

### P19

**The timeline always jumps to the newest message**

Go opens a thread at the last read message. `scrollToLastRead`
(`ui/chat_history.go:534-551`) walks forward to the first unseen item and scrolls
to the one above it, falling through to the bottom only when everything is read;
`displayThread` calls it bracketed by `disableSeenTracking = true/false`
(`ui/thread.go:440,458-459`) so the walk emits no receipts. A floating
jump-to-bottom icon appears when the reader is more than 2.5 screens above the end
(`ui/chat_history.go:576-589`) and, when tapped, scrolls down, zeroes the unread
counter and marks everything read (`:89-110`).

The port pins to the bottom on every `threadId` change:
`atBottomRef.current = true; setTimelineAtBottom(threadId, true)`
(`electron/src/renderer/Conversation.tsx:322-328`) then
`element.scrollTop = element.scrollHeight` (`:331-337`), with
`useVisibleRange.ts:167-179` seeding the first window at the tail. There is no
jump-to-bottom control anywhere — no matching CSS, no component — though
`notifications.ts:201-218` keeps an `atBottomByThread` map (the port of
`chat/notification_context.go`'s `scrolledDown`) that reads pin state without
exposing a way to change it. Unread state exists in the sidebar
(`LeftPane.tsx:119,171-175`); nothing consumes it for positioning.

`App.tsx:233-252` also marks every `!outgoing && !seen` message read the moment a
conversation is selected, with no visibility or focus gate, where Go marks per row
(`ui/chat_history.go:402-423`) gated on `threadActive() && windowFocused()`
(`:807-812`). Go converges on the same end state as soon as the reader reaches the
bottom (`:793-807`), so this is a timing divergence plus a missing focus gate
rather than a new disclosure class — but the focus gate is worth having.

Go's `scrollToLastRead` fires on first open only (`ui/thread.go:435-436`); the
port re-pins on every switch, so it diverges on both first open and return visits.

**Done** — on mount, scroll to the index of the first unseen incoming message
rather than the end; a floating button when the scroller is far from the bottom
that scrolls down and reports the thread read; and the read-on-select sweep gated
on window focus.

**Size** — M

---

## Low

### P20

**`accepted` is dead end to end, and the auto-join policy is unported**

Go's `AcceptInvite` calls `acceptAllUsers(groupID)` first, marking every member
and invitee `accepted = true` (`chat/update_group.go:821-859`). The only reader in
the entire Go tree is the auto-join decision: under
`OnlyAutoJoinGroupsWithNoNewUsers`, a group containing any non-accepted user is
not auto-joined (`chat/consensus_store.go:591-624`), and the Fyne UI exposes the
setting (`ui/settings_container.go:24-32,125,190-198`).

`respond_to_invite` (`rust/bounce-core/src/engine/mod.rs:526-541`) signs and
applies the update and does nothing else. `accepted` is written in two of Go's
three places — own profile (`engine/mod.rs:261`) and add-user adoption (`:1063`,
copied at `:1050`) — and read in none. `auto_join_groups` is stored, validated,
settable, returned in `SettingsView` (`engine/settings.rs:134-148,188`), exposed
through napi (`bounce-node/src/lib.rs:701`) and IPC
(`main/index.ts:243`, `preload/index.ts:345`), and consumed by nothing:
`recompute_group` has no auto-join hook and nothing under `consensus/` reads it.
The whole auto-injection block from `setRollbacksApplicationsAndGroupState` is
unported, including the blocked-user auto-leave and auto-reject at
`chat/consensus_store.go:520-589`.

The failure is in the conservative direction — the port never auto-joins, so it
can never add a user to a group they did not explicitly accept; the regression is
one extra click. `Accepted` is `msgpack:"-"` (`chat/user.go:53`), so Go and Rust
peers compute identical group state. And the setting is currently invisible:
`SettingsPanel.tsx:100-109` flattens `Settings` field by field and omits
`autoJoinGroups`, so no user can set it and observe the no-op. Both sides default
to `ONLY_WITHOUT_NEW_USERS` (`frames/identity.rs:185`), so this would bite at the
default rather than only for users who changed a setting.

The reason to file it at all is the trap: a future implementer inherits a flag
written in two of three places and will silently under-auto-join, because users
met through group invites never become accepted.

**Done** — `accept_all_users` on the accept branch of `respond_to_invite`, and a
group-state recomputation that consults `settings.auto_join_groups` together with
the members' `accepted` flags before auto-accepting. Depends on [P1](#p1): there
are no group-met users to mark accepted until they are persisted.

**Size** — M

---

### P21

**`last_opened` is persisted but never written**

Go's `SetDMLastOpened` writes the column (`chat/user.go:462-469`), is on the engine
interface (`chat/engine.go:54`, group twin at `:61`), and is called whenever a
thread is displayed (`ui/direct_message.go:309-310`, `ui/group.go:602-603`,
`ui/invite.go:162-163`). It surfaces as `DMState.LastOpened`
(`chat/database.go:395`) and feeds the sidebar sort at `ui/thread.go:48-62`, which
promotes a thread to `max(lastMessageTime, lastOpenTime)` when `hasDraft()` is
true (`ui/direct_message.go:121-123`).

The column exists (`store/schema.rs:43`, groups at `:128`) and round-trips
(`store/mod.rs:82,108,217,224,1778`), and the only assignment in the crate is the
`last_opened: 0` initialiser at `frames/identity.rs:286`. It is absent from
`UserView` and `GroupView` (`engine/event.rs:21-53`), has no napi method, and
`lastOpened` appears nowhere in `electron/`. The sidebar sorts on
`b.lastActivity - a.lastActivity || name` (`state.ts:488`) with `lastActivityFor`
considering only the newest message (`:491-497`).

The behaviour is observable because drafts *are* ported (`LeftPane.tsx:116-122`,
the "Draft: " prefix at `styles.css:448-449`, `DraftView` at
`engine/event.rs:153-156`): in Go a draft pins the thread to the top and persists
across restarts; in Electron it stays buried at its last-message position while
displaying "Draft:". The field is local-only on both sides
(`chat/user.go:38`, `frames/identity.rs:234-235`), so nothing syncs and nothing is
at risk.

`update_user_local_state` already writes the column (`store/mod.rs:217,224`), so
the persistence half is done.

**Done** — a `set_last_opened(thread)` engine and napi method called on
conversation select, the field on `UserView`/`GroupView`, and the draft-aware
ordering rule from `ui/thread.go:47-62` reproduced in the sidebar sort. Groups and
invites need it too.

**Size** — S

---

### P22

**You can invite yourself to a new group**

Go builds the picker from the contact store and skips blocked users and the
profile itself (`ui/new_group_container.go:340-347`), feeding `InitialInvites` at
`:132-140`.

The port derives `contactNames` from `allConversations.filter(c => c.kind ===
'direct')` (`electron/src/renderer/App.tsx:348`), and `conversations()` appends a
note-to-self row keyed on the profile's own id with `kind: 'direct'`
(`state.ts:474-486`). So the current user appears in the invite list as
"<name> (You)" and can be checked; `NewGroupDialog` (`App.tsx:360-434`) applies no
id filter, and every layer below is a pass-through down to
`engine/mod.rs:482-484`.

The port already has the right convention next door, which is what makes this an
oversight rather than a choice: the add-member-to-existing-group picker sources
from `state.users` and filters `user.id !== myId && !user.blocked`
(`DetailsPanel.tsx:169-175`). `NewGroupDialog` is the sole departure.

Downstream it is harmless. `create_group` seeds `users: vec![me]`
(`engine/mod.rs:455`), `GroupState::from_group` copies it so `is_member(my_id)` is
true, `apply_update` short-circuits ("Inviting an existing member changes
nothing", `consensus/state.rs:340-343`), and `change_is_noop` makes the stack drop
the update before pushing (`consensus/stack.rs:233-235`). No error propagates,
membership is not duplicated, and no invite banner appears. The residue is one
wasted signed frame and a spurious "invited <you> to the group" status row, since
`emit_group_system_message` fires unconditionally (`engine/mod.rs:606`).

**Done** — the picker built from `state.users` excluding `state.profile.id` and
blocked users, the same source a contacts view would use.

**Size** — S

---

### P23

**Drafts never sync**

The outbound half is complete and correct — this is a missing receiver, not a
missing feature.

Go signs a draft (`chat/drafts.go:185-193`), and `syncDrafts` deletes prior drafts
for the thread, persists and broadcasts (`:205-230`), driven every five seconds by
`keepDraftsSynced` (`:20-25`). Scope is `scopeSync` with destination self
(`:76-86`). `handleDraft` (`:88-171`) verifies the signer is one of our own
devices (`:135`), rejects revoked signers (`:126-133`) and reconciles by timestamp
(`:157`). Drafts are catch-up-eligible (`chat/catch_up.go:29`) and served in the
reference flow (`chat/reference_offer.go:1265-1276`).

`Engine::save_draft` (`rust/bounce-core/src/engine/mod.rs:2084-2103`) builds a
`SignedContainer`, saves and calls `self.broadcast(&draft)`, and `Draft`'s
`Broadcastable` returns `Scope::Sync` with destination self
(`frames/message.rs:432-452`, scope unit-tested at `:593-608`). A Go sibling would
accept and display it.

What is missing is receipt and durability. There is no `FrameType::Draft` arm —
it falls to `engine/mod.rs:2328` — and no `handle_draft` anywhere in `rust/`.
`Event::DraftUpdated` is declared (`engine/event.rs:216-217`) and already handled
by the renderer (`preload/index.ts:177`, `state.ts:414`) and never emitted. The
`drafts` table has only id, thread, text, timestamp, saved_at
(`store/schema.rs:353-359`), so `all_drafts` synthesises `SignedFrame::default()`
(`store/mod.rs:1768`) — which is why the missing columns matter: a stored draft
has no bytes to relay. Drafts are absent from `has_frame`, `frame_payload` and
`frame_saved_at`, so none is ever replayed to a device that was offline.

Unobservable until [P14](#p14), and the draft survives on the device it was typed
on regardless.

**Done** — signer, original_payload and signature columns on `drafts`; a
`handle_draft` applying newest-timestamp-wins as `chat/drafts.go:157` does, after
verifying the signer is one of our own devices; and drafts added to the three
reference-flow lookups.

**Size** — M

---

### P24

**Encrypted devices: the frames exist, none of the flows do**

Listed under
["Implemented but not yet wired into the engine"](rust-electron.md#implemented-but-not-yet-wired-into-the-engine).
It is the largest single item in this document and is here for sizing, not
because it is a surprise.

Go implements a whole second engine mode. `chat/encrypted_device.go` is 1,966
lines: `StartEncryptedDevice` (`:49`), the management handshake (`:235`, `:273`,
`:331`), ciphertext storage (`:461`, `:543`), the ECDH reference-offer challenge in
both directions (`:1203`, `:1301`, `:1358`), management actions and key-hash
checks (`:1529`, `:1706`, `:1740`), recipient appending (`:793`, `:866`, `:954`,
`:1124`), clear-before propagation (`:601`, `:702`) and draft pruning (`:1915`).
`sendToEncryptedDevices` (`chat/encryption.go:64`) is called on every broadcast
(`chat/protocol.go:252`). `chat/encrypted_catch_up.go` (436 lines) and
`chat/encrypted_file_storage.go` (737 lines) complete it, and
`chat/protocol.go:167-189` registers nineteen handlers for the encrypted role.

`rust/bounce-core/src/frames/encrypted.rs` is 529 lines that stop at data
structures plus unit tests: `Recipient` (`:37`), `DeviceRecipient` (`:61`),
`EncryptedFrame` (`:83`), `seal_for_recipients` (`:187`), the reference-offer
challenge and response (`:220`, `:235`), the management request and response
(`:280`, `:297`), `EncryptedClearBefore` (`:311`) and `prune_recipients` (`:339`).
Missing entirely: `EncryptedCatchUp`, `EncryptedReceive`, `ManageEncryptedDevice`,
`EncryptedDeviceManagementActionResponse`, `GetManagementKeyHash`,
`ManagementKeyHashResponse`, the four `AppendRecipient` frames,
`EncryptedStorageReferenceOffer`/`Request`, `EncryptedChunkStorageRequest`,
`EncryptedChunkOffer` and `RequestEcro`. The type discriminants all exist
(`types.rs:41-62`); the payload structs and flows do not. `handle_frame` has no
arm for any of them, `Engine::broadcast` never fans out sealed copies, and
`StoreScopeContext::is_encrypted_device` (`engine/mod.rs:3053`) returns `false`
with a comment saying why.

That stub is consistent rather than hazardous: encrypted device addresses live in
the `users.encrypted_devices` CSV column (`frames/identity.rs:213-215`), never as
rows in `devices`, and every input to `scope::resolve` reads the devices table, so
no encrypted address can reach the filter at `scope.rs:166`.

An encrypted device is an opt-in deployment mode with its own entrypoint, run on a
VPS, and `encryptedReceive`/`encryptedCatchUp` are only ever emitted *by* one to
its owner's devices — so a port user who never provisions one sees no encrypted
frame at all. In a mixed network where a Go peer owns one, the Rust peer simply
does not stash ciphertext there and the messages still arrive through the ordinary
reference flow when both are online. The loss is an availability optimisation,
which is exactly what [goals.md](goals.md) says encrypted devices are for.

**Done** — stage it. (1) The ~13 missing frame structs. (2) Store tables for
`encrypted_sync_devices`, `recipients`, `device_recipients`, `encrypted_frames`.
(3) `Engine::broadcast` fanning out sealed copies via `seal_for_recipients` to
addresses from `User::encrypted_device_addresses` (`frames/identity.rs:312`), with
`is_encrypted_device` reading that. (4) The management pairing flow and the ECRO
challenge/response handlers. (5) A separate encrypted-device binary role.
Encrypted file storage depends on large-file seeding and follows it.

**Size** — L

---

### P25

**Device rename is plumbed end to end and has no control**

Go opens an edit dialog per device row with an editable name applied through
`RenameDevice` (`ui/edit_profile_container.go:203-211`), the address, last-seen and
created dates (`:88-129`), and a Revoke button behind a confirmation (`:142-160`).

`renameDevice` works at every layer of the port — `engine/settings.rs:225-249` →
`bounce-node/src/lib.rs:719-723` → `main/engine.ts:392-393` →
`main/index.ts:248-250` → `preload/index.ts:361-362` — and no renderer code calls
it. `DevicesSection` renders name, This device and Revoked badges, address and an
online dot, kept live by `state.ts:396-411`
(`SettingsPanel.tsx:235-262,276-281`), with the Revoke and Add device buttons
correctly disabled pending [P14](#p14).

Impact is small and worth stating so nobody over-scopes it: the list has exactly
one row until pairing lands, the user names it at onboarding
(`Onboarding.tsx:14,33,63-65`), and the name never leaves the device
(`engine/settings.rs:222-224`).

**Done** — the device name made an editable field wired to the existing
`renameDevice`; Revoke and Add device stay disabled.

**Size** — S

---

### P26

**No QR code and no copy button for the pairing code**

Contact discovery is meant to happen with the two people in the same room
(`engine/mod.rs:747-749`), and a Go mobile peer's Scan tab opens a camera
expecting a QR (`ui/add_user.go:258-267`).

Go encodes the add-user string as a 256px QR shown at 300px
(`ui/add_user.go:54,66-67,284`) and attaches a clipboard-copy action item to the
text entry (`:287`), inside a Share/Scan tab pair with a live camera preview
(`:114-142,220-267`).

`AddContactDialog` prints the code in a plain `div`
(`electron/src/renderer/App.tsx:479-481`) with a single paste input (`:483-489`)
and no copy control, under text telling the user to "Show this code to someone
next to you". `electron/package.json` declares zero runtime dependencies and there
is no QR code anywhere in the tree.

Two things keep this at low. The missing scan path does not apply to a desktop
client — Go gates its camera on `fyne.CurrentDevice().(sensor.CameraDevice)`
(`ui/add_user.go:160`) and without one the Scan tab collapses to a paste entry
(`:269-275`), the same as Electron's. And the manual path works:
`styles.css:1238` sets `user-select: text` with `word-break: break-all`, so the
full code is visible and selectable, and
[rust-electron.md](rust-electron.md#running-it) documents the intended desktop flow
as copying one client's code into the other's. The codes remain wire-compatible
with Go (`engine/mod.rs:751-752`).

**Done** — the pairing code rendered as an inline SVG QR, self-contained with no
CDN, plus the copy button already used for the address at
`SettingsPanel.tsx:196-219`.

**Size** — S

---

## Deliberate divergences

The port follows `chat/` closely, except where reading it closely turned up a
weakness or where a different structure does the same job. These are on purpose.
[rust-electron.md](rust-electron.md#other-divergences-from-the-go-implementation)
carries the protocol-level list — `addUser` consent, unsolicited acceptances,
device-group merging, read-receipt and typing-indicator actor checks, device row
identity, burned secrets, and update timestamp resolution — plus the
[handshake signing oracle](rust-electron.md#the-handshake-signing-oracle), which
is a fix the Go implementation still needs. What follows is what turned up in this
audit and is *not* in that list.

| Area | Go behaviour | Here | Why |
|---|---|---|---|
| Inbound `updateDM` | No scope or actor gate on receipt (`chat/update_dm.go:182-289`), so a counterparty can push a `ChangeMutedUntil`, `SetAlias`, `SetNotes` or `SetBlocked` that Go stores and replays into your `users` row — including flipping your `blocked` flag for them | Non-shared kinds are refused outright (`engine/mod.rs:1245-1250`), with a named test at `tests/engine_e2e.rs:1527-1562` | Your private opinion of someone is not theirs to set. See [P13](#p13) for the part of this that still needs fixing — the port broadcasts the same kinds with `Scope::Sync` and so refuses its own devices |
| `open_dm` | Derived from `introduction_method` on every DM recompute (`chat/user.go:395-403`, `chat/update_dm.go:325`) | Materialised once at introduction: `#[serde(skip)]` with a `false` default (`frames/identity.rs:233,285`), set true only on the scanned-contact path (`engine/mod.rs:1074-1081`), and `save_user`'s upsert refreshes only name, images, encrypted_devices, public_ecdh_key and last_activity (`store/mod.rs:91-96`) so a later group frame cannot clobber it | Same value, fewer moving parts. `introduction_method` is stored but read by nothing, which is intentional — it does not need to be a decision input |
| Restricted posting | Enforced only by greying out the entry; there is no send-side check in `chat/group_message.go:447` onward | `engine/mod.rs:438-441` and `engine/files.rs:164` return `NotPermitted` before the frame is created; the renderer surfaces it in the error banner (`App.tsx:184-189,280-283`) | The reference relies on the UI being the enforcement point. Here the engine is. The composer not greying out is a cosmetic difference over a stricter guarantee |
| Pairing offers | Two tables, one per offer kind (`chat/add_user_offer.go:17`, `chat/sync_device_offer.go:17`), and a sync secret outlives navigating away from the screen | One `pairing_offers` table, at most one live offer per device (`store/mod.rs:679-693`), single-use via a persisted `burned_secrets` table, with `UNIQUE` on `secret` (`store/schema.rs:365`) | "Displaying a new code invalidates whatever was on screen before, so a code photographed off someone's shoulder stops working the moment they open the screen again." Whoever implements [P14](#p14) should keep the two kinds distinguishable — add a `kind` column rather than sharing the row |
| Chunk row identity | The primary key is derived from the file id and content hash — and the send and receive paths disagree about how (`chat/file.go:1330-1332` uses the raw digest, `chat/file.go:263-267` the hex string), so the same chunk already has different ids on either side | Random UUID with `UNIQUE (file_id, idx)` and an upsert on that pair (`store/schema.rs:651`, `store/mod.rs:1536-1541`) | The id never goes on the wire in either implementation (`chat/file.go:866-875`, `frames/file.rs:255-272`) and neither ever queries by it. Go's scheme also collides when a file contains two identical chunks, which is routine at a 1 MiB chunk size |
| Schema migration | GORM `AutoMigrate` (`chat/database.go:87`) | A reference database built from `create()` and diffed, issuing `ALTER TABLE ADD COLUMN` for anything missing (`store/schema.rs:424-426,506-544`), with a table-rebuild path for changed constraints | "An added table, index or column needs no migration code at all." Covered by `tests/store_migration.rs`, including that a migrated database is indistinguishable from a fresh one |
| Attachment bytes | On disk, with the row holding a path (`chat/file.go:874`) | In `chunks.data` (`store/schema.rs:304-313`), so "a chunk cannot be orphaned from its metadata by a partial write" | The storage choice is sound; the missing deletion sweep ([P6](#p6)) is not part of it |
| Files above 20 MiB | Seeded in place from disk (`chat/file.go:1287`) | Refused at pick time by name and size (`Attachments.tsx:156-157`), before bytes are read | On the [not-wired list](rust-electron.md#implemented-but-not-yet-wired-into-the-engine). The refusal is deliberate: it needs a path that survives restarts and a streaming reader. The receive-side default matches Go exactly — `record.wanted = record.is_embedded()` (`engine/files.rs:382`) against `chat/file.go:286-288` — so a large inbound file renders "Not downloaded" rather than a spinner. What is genuinely missing is Go's later opt-in (`DownloadFileToDisk`), and the comment at `engine/files.rs:380-381` implies a want-trigger that does not exist |
| Avatars | Stored image, circle-cropped, initials only as fallback (`ui/default_image.go:82-198`) | Deterministic-tint initials for everyone ([rust-electron.md](rust-electron.md#the-interface)) | A design choice for the fallback, not for the absence of the image path — see [P18](#p18) |
| Status row wording | — | Taken from `ui/thread_item.go` word for word (`SystemMessage.tsx:1-9`), including rows for changes the port cannot itself make, such as "changed the group image" | "Two people watching the same conversation from the two clients should read the same history." A row reporting what a Go peer did is correct even when the local client has no way to do it |
| Peering | `chat/device_pool.go:58-131` | `engine/peering.rs` — a 60s audit, 15s keep-alives, dial and failed-dial cooldowns, per-scope connection targets, randomised selection, plus `reach_for` for immediate dialling on conversation open. Tested in `tests/peering.rs` | At parity, with the addition. Noted here because the file is new enough that a reader working from stale line numbers may conclude it does not exist |

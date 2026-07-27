# Replies, reactions, and deleting a message

**Status** — design, for review.
**Scope** — three message interactions the Bounce protocol does not have: replying
to a specific message, reacting to one with an emoji, and deleting one after it
has been sent. Engine, storage, and interface.
**Companion** — [`docs/protocol-extensions.md`](../../protocol-extensions.md),
the coordination document for the Go maintainer. Anything in this file that
describes the wire is restated there in a form that can be implemented against
without reading Rust.

---

## Why this is a protocol change and not a feature

All three interactions refer to a message that has already been sent. None of
them can ride the message itself, so each needs something new on the wire — and
the Go implementation in `chat/` is unusually hostile to new things.

Four behaviours, all read out of `chat/` rather than assumed:

| Situation | What Go does | Source |
|---|---|---|
| Unknown **frame type**, live connection | logs `"peer sent an unsupported frame type, disconnecting"`, then `conn.Close()` | `chat/remote_device.go:224-231` |
| Unknown frame type inside a **catch up** | refuses the **entire bundle** — `return nil, false` | `chat/catch_up.go:131-136` |
| Unknown **map key** in a known frame | skipped silently | verified, see below |
| Relaying any frame | re-emits `OriginalPayload` byte for byte | `chat/direct_message.go:87-99` |

The third was checked by running it against the exact library the project pins,
`github.com/Basekick-Labs/msgpack/v6 v6.1.0`: a struct carrying two fields the
receiving build has never heard of decodes cleanly into the older struct, and
older bytes decode into the newer struct with the new fields zeroed. Combined
with the fourth, that means **an added field is free in both directions and
survives a Go relay hop with its signature intact**.

The first two are what make this a coordination problem. An unknown frame type
is not ignored — it drops the connection. And because the reference flow re-offers
anything a peer has not acknowledged, the drop repeats on every reconnection.
The failure mode is not "Go peers cannot see reactions", it is "the peer
relationship stops working". There is no version or capability signal anywhere in
the protocol to gate on; `device` (`chat/device.go:20-33`) carries none.

### The decision

Ship the new frame types anyway, and coordinate:

1. **New frame types** for reactions and deletion, modelled properly rather than
   contorted into an existing frame.
2. **A document for the Go maintainer** — the companion file — asking first for a
   *whitelist* (accept and discard types 52 and 53 instead of disconnecting),
   which is a small patch, and separately supplying the full specification so the
   features can be implemented when they choose.
3. **A local switch** to stop sending the new frame types, for the window before
   a whitelisted Go build is out.

Replies stay an added field, because they genuinely are one — a reply is a
message, and its quote is part of it.

### Proposed addition: gate on an advertised capability

**This is an amendment to the decision above, offered because the window it
closes is not small. Take it or drop it; the rest of the spec does not depend on
it.**

The switch in (3) is the only thing standing between a Bounce user and a broken
peer relationship with a Go contact, and it is a global on/off that the user has
to reason about. For it to be safe it has to default to **off**, which means the
feature ships disabled for everyone — including all-Rust conversations, where it
was never at risk.

One added field removes that trade. `Device` gains:

```rust
/// Protocol extensions this device understands. Absent on any build that
/// predates them, which is exactly the right default.
#[serde(rename = "Capabilities", default, deserialize_with = "crate::msgpack::nullable_seq")]
pub capabilities: Vec<String>,
```

`Device` records travel inside `User` (`frames/identity.rs:270`), which travels
inside `AddUser` blobs and group state — and Go relays those byte for byte, so a
Rust device's capabilities reach another Rust peer even through a Go
intermediary. Go itself ignores the key.

Broadcast scope then filters: a frame type in the extension set only goes to
devices that advertised it. Go devices never receive one, so they never
disconnect, and the whitelist patch becomes belt and braces rather than a
prerequisite. The switch survives as a kill switch rather than as the only
defence.

The gap it does not close: devices already in the database have no capabilities
recorded, including our own. They read as legacy until they re-announce. For a
product at this stage that is acceptable and should be stated in the release
notes rather than migrated around; if it turns out to matter, a `SetCapabilities`
variant of `UpdateDevice` at global scope is the follow-up.

**Recommended defaults.** With the capability gate: the switch defaults to *send*.
Without it: the switch defaults to *hold* until a whitelisted Go build has
shipped, and the release notes say so.

---

## Frames

Two new types, taking the next free numbers. Both are shaped on `ReadReceipt`
(`frames/message.rs:271-332`), which is the closest thing that already exists: it
names a message by id and type, carries an actor, and resolves its own
destination locally rather than trusting the sender to declare one.

```rust
FrameType::Reaction      = 52
FrameType::DeleteMessage = 53
```

`catch_up_order` gets `Reaction => 16` and `DeleteMessage => 17`, after `Draft`
at 15. Appending rather than inserting matters: the table decides replay order
within a second, and renumbering it would make our ordering disagree with Go's
for types they already have. Deletion sorts last on purpose, so it applies after
anything it might target.

### `Reaction` — type 52

```rust
pub struct Reaction {
    #[serde(skip)] pub signed: SignedFrame,

    #[serde(rename = "ID")] pub id: Uuid,
    /// The person reacting.
    #[serde(rename = "Actor")] pub actor: Uuid,
    /// The message reacted to.
    #[serde(rename = "Target")] pub target: Uuid,
    /// `DirectMessage` or `GroupMessage`.
    #[serde(rename = "TargetType")] pub target_type: u16,
    /// One emoji. Empty when `remove` is set.
    #[serde(rename = "Emoji")] pub emoji: String,
    /// Withdraw this actor's reaction rather than set one.
    #[serde(rename = "Remove")] pub remove: bool,
    #[serde(rename = "Timestamp")] pub timestamp: i64,

    /// Derived from the target message, never carried — a reaction must not be
    /// able to claim membership of a conversation it does not belong to.
    #[serde(skip)] pub destination: Uuid,
    #[serde(skip)] pub scope: i64,
    #[serde(skip)] pub saved_at: i64,
}
```

**Resolution.** One reaction per person per message, last write wins by
`timestamp`, `remove` clears. That is Signal's rule and it is what keeps the
state a map rather than a log.

**Validation on receipt**, in this order, each failing closed:

- `signer_speaks_for(actor)` — the same check every authored frame gets.
- The target exists locally, or the frame is parked (below).
- The actor is a participant: the counterparty or ourselves for a direct
  message, a current member for a group.
- `emoji` is a **single grapheme cluster** that is entirely emoji, and at most
  a small byte cap. Without this the pill under a bubble is an arbitrary string
  a peer chose, rendered at the size we chose. This is the one new place a peer
  gets to put characters on our screen, so it is the one that needs the check.

**Retention.** A reaction's `delete_at` is copied from its target. It cannot
outlive the thing it is attached to, and the existing sweep (`next_expiry`) picks
it up with no new machinery.

**Arriving early.** A reaction whose target we have not received is stored
unresolved — nil destination, sync scope — and resolved when the message lands.
This is exactly what read receipts already do
(`store::unresolved_read_receipts_for`, `engine::resolve_early_read_receipts`),
and the reaction path should reuse the shape rather than invent a second one.

### `DeleteMessage` — type 53

```rust
pub struct DeleteMessage {
    #[serde(skip)] pub signed: SignedFrame,

    #[serde(rename = "ID")] pub id: Uuid,
    /// Who is deleting. The message author, or a group admin.
    #[serde(rename = "Actor")] pub actor: Uuid,
    #[serde(rename = "Target")] pub target: Uuid,
    #[serde(rename = "TargetType")] pub target_type: u16,
    /// Set when the actor is acting as an admin rather than as the author.
    #[serde(rename = "AdminDelete")] pub admin_delete: bool,
    #[serde(rename = "Timestamp")] pub timestamp: i64,

    #[serde(skip)] pub destination: Uuid,
    #[serde(skip)] pub scope: i64,
    #[serde(skip)] pub saved_at: i64,
}
```

The rules are Signal 8.20's, from `ts/util/canDeleteForEveryone.preload.ts`.
They changed materially in 8.x and the older 7.x rules are wrong in two places,
so they are worth stating rather than assuming:

| Rule | Value | Note |
|---|---|---|
| Author may delete their own | within **24h** of `written_at` | Signal's `getNormalDeleteMaxAgeMs`, which defaults to `DAY` |
| **Group admin** may delete anyone's | within **24h** | `getAdminDeleteMaxAgeMs`; new in 8.x |
| Receiver accepts up to | **window + 24h** | Signal's `MESSAGE_SEND_GRACE_PERIOD`; a frame can sit in a reference queue for a long time |
| Already deleted | refused | not an error, just a no-op |

Two deliberate divergences from Signal, both because Bounce is not built the
same way:

- **Signal computes age from a server timestamp; there is no server here.** We
  use the target's `written_at`, which the deleting party authored. That is
  forgeable — but only by the author, and only to delete their own older
  message. The admin case is the one that matters, and there the actor is not
  the author, so the timestamp they would be lying about is not theirs.
- **Signal excludes note-to-self from delete-for-everyone.** We allow it, with
  no time limit, at `Scope::Sync`. In Bounce a note to self is a real frame
  synced across your own devices, so deleting it on all of them is not "for
  everyone" at all — it is deleting your own data on your own machines.

**Effect, and the part that is easy to get wrong.** The row is *not* removed. It
is emptied — text cleared, attachment rows dropped, and the underlying `files`
rows deleted so the chunk cascade frees the bytes (the same path parity
[P6](../../parity.md#p6) needs) — and a `deleted_at` tombstone is set.

The tombstone is load-bearing. Delete the row outright and `has_frame` starts
answering `false`, the reference flow classifies the original as wanted, a peer
re-offers it, and **the message comes back**. The tombstone is what makes the
deletion stick.

Deleting also takes its reactions with it, and any reply quoting it degrades to
"Original message not available" — which falls out of the quote mechanism below
rather than needing its own rule.

**Delete for me** is separate and carries no frame at all: the row goes, nothing
is broadcast, and the message is untouched everywhere else. Signal keeps these as
two distinct menu items and so should we, because they are two genuinely
different promises.

### Replies — an added field

A reply is a message, so this is a field on `DirectMessage` and `GroupMessage`
rather than a frame. Go ignores the key and relays it intact, so this half of the
work needs no coordination at all.

```rust
#[serde(rename = "Quote", default)]
pub quote: Option<Quote>,

pub struct Quote {
    /// The message being replied to.
    #[serde(rename = "Target")] pub target: Uuid,
    #[serde(rename = "Author")] pub author: Uuid,
    /// A bounded excerpt, not the whole body.
    #[serde(rename = "Text")] pub text: String,
    /// 0 text, 1 image, 2 file — so a reply to a photo can say so.
    #[serde(rename = "Kind")] pub kind: u16,
    /// The **original's** `delete_at`, not the reply's.
    #[serde(rename = "ExpiresAt")] pub expires_at: i64,
}
```

**Why a snapshot at all.** Without one, a reply that arrives before its target —
ordinary on a catch up, not a corner case — renders as a dangling stub, and so
does a reply to a message that has since expired. Signal carries the snapshot for
the same reason.

**Why `ExpiresAt`.** Bounce has disappearing messages and Signal's quote does not
account for them: a plain snapshot re-publishes the text of a thirty-second
message under the reply's retention, which may be forever. Carrying the
original's expiry lets the recipient blank the quote on schedule, leaving the
reply intact above an "Original message not available" stub. The sweep is local
and rides the existing retention job.

This is not a defence against the sender — they had the plaintext already and can
put it in the reply body if they want to. It is a correctness fix for the honest
case, which is every case that actually happens.

`Text` is capped at **160 characters**, which is what fits the two-line quote
block at Signal's metrics; longer bodies are truncated with an ellipsis at the
source so the cap is not something the renderer has to re-derive.

---

## Storage

Three schema changes.

```sql
-- Reactions. Their own table rather than a column, because a message has many
-- and they arrive and disappear independently of it.
CREATE TABLE reactions (
    id           BLOB PRIMARY KEY,
    actor        BLOB NOT NULL,
    target       BLOB NOT NULL,
    target_type  INTEGER NOT NULL,
    emoji        TEXT NOT NULL,
    timestamp    INTEGER NOT NULL,
    delete_at    INTEGER NOT NULL DEFAULT 0,
    saved_at     INTEGER NOT NULL,
    destination  BLOB NOT NULL,
    scope        INTEGER NOT NULL,
    signer       TEXT NOT NULL,
    original_payload BLOB NOT NULL,
    signature    BLOB NOT NULL
);
CREATE UNIQUE INDEX reactions_one_per_actor ON reactions (target, actor);
CREATE INDEX reactions_by_target ON reactions (target);
```

The unique index is the resolution rule expressed where it cannot be forgotten:
an upsert on `(target, actor)` gated on a newer `timestamp`, and a delete for
`remove`.

```sql
-- Deletion, and replies, on both message tables.
ALTER TABLE direct_messages ADD COLUMN deleted_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE direct_messages ADD COLUMN deleted_by BLOB;
ALTER TABLE direct_messages ADD COLUMN quote_target BLOB;
ALTER TABLE direct_messages ADD COLUMN quote_author BLOB;
ALTER TABLE direct_messages ADD COLUMN quote_text TEXT NOT NULL DEFAULT '';
ALTER TABLE direct_messages ADD COLUMN quote_kind INTEGER NOT NULL DEFAULT 0;
ALTER TABLE direct_messages ADD COLUMN quote_expires_at INTEGER NOT NULL DEFAULT 0;
-- and the same seven on group_messages.
```

`deleted_by` is kept because the interface says different things for "You
deleted this message", "This message was deleted", and an admin removing
somebody else's — and reconstructing which from the row alone is not possible.

Delete frames themselves need a table too, for the same reason every other frame
has one: `has_frame`, `frame_payload`, `references_not_delivered_to` and
`peer_may_have` all consult storage, and a delete that is not stored is a delete
that is never re-offered to a peer who was offline when it happened. That is the
single most important thing to get right in this whole document — a deletion
that does not reach an offline peer is a deletion that silently did not happen.

---

## Engine

New methods, each mirroring an existing one rather than inventing a shape:

| Method | Mirrors |
|---|---|
| `react(target, target_type, emoji)` | `mark_as_read` |
| `remove_reaction(target, target_type)` | — |
| `delete_for_everyone(target, target_type)` | `mark_as_read` |
| `delete_for_me(target, target_type)` | local, no frame |
| `send_direct_message(..., quote)` / `send_group_message(..., quote)` | extends the existing signatures |

New handlers: `handle_reaction`, `handle_delete_message`, both in `handle_frame`
**and** `handle_frame_unlocked` — the catch-up dispatcher is where parity
[P5](../../parity.md#p5) was lost by handling a frame in one and not the other,
and it is a silent failure both times.

New events, following `MessageRead`'s shape:

```rust
Event::MessageReacted   { message_id, user_id, emoji }      // emoji "" == removed
Event::MessageDeleted   { message_id, by, admin }           // extends the existing variant
```

`MessageView` grows `reactions: Vec<ReactionView>`, `quote: Option<QuoteView>`,
`deleted_at`, and `deleted_by`. All four have to be populated in
`initial_state` as well as in the live event path — the delivery-tick regression
fixed alongside this work was exactly that mistake, a field the live path filled
and the snapshot did not, so it looked correct until the app was restarted.

---

## Interface

Signal's full treatment, as directed.

**Hover actions.** A row that appears beside a bubble on hover: react, reply,
more. This does not exist yet — right-click → Info is the only message action
today — so it is a prerequisite for both features and should land first.

**Reacting.** Six recent emoji plus a full picker, which `EmojiPicker` and
`loadRecentEmoji` already provide. Pills sit under the bubble, grouped by emoji
with a count; your own is outlined; clicking it removes it. A sheet lists who
reacted with what.

**Replying.** A quote preview above the composer with a dismiss button; a quote
block inside the sent bubble in the quoted author's palette colour; clicking it
scrolls to the original and flashes it. When `quote_expires_at` has passed, the
block renders "Original message not available" in italic and is not clickable.

**Deleted.** Signal's italic stub with a slashed-circle icon, in place of the
body, keeping the timestamp. Three strings — yours, theirs, an admin's.

**The switch.** In settings, under a heading that says what it is for. Local
only, deliberately *not* an `UpdateSettings` frame: it is a temporary migration
control rather than a preference, and syncing it would put a new value into a
wire enum shared with Go for no benefit.

---

## Testing

Beyond the obvious per-unit coverage, five that are worth naming because they are
the ones that fail quietly:

1. **A delete reaches a peer who was offline when it happened.** Send, take the
   peer down, delete, bring it back, assert the tombstone. This is the one that
   catches a missing `handle_frame_unlocked` arm or an unregistered reference
   source.
2. **A deleted message does not come back.** After a delete, run a full reference
   cycle in both directions and assert the body is still gone. This is the
   tombstone's whole job.
3. **A reaction that arrives before its message.** Park and resolve, mirroring
   `a_receipt_that_arrives_before_its_message_is_resolved_later`.
4. **A reply's quote blanks when the original expires**, while the reply itself
   survives.
5. **A Go peer stays connected.** With the capability gate, assert that a device
   advertising nothing is not sent a type 52. Without it, assert the switch
   actually suppresses the send. Either way this is the test that stands between
   the change and a broken network, and it should not be left to manual checking.

---

## Sequencing

1. Hover actions and the message action menu — a prerequisite for both features,
   and independently useful.
2. Replies. No coordination needed, no new frame type, so it can ship while the
   Go conversation is still happening.
3. The capability field and the switch.
4. Reactions.
5. Deletion, which is last because the tombstone interacts with retention,
   clear-history, and the reference flow, and is easier to reason about once
   reactions have established the frame shape.

---

## Open questions

- **The capability amendment** — accept or drop. Everything else stands either
  way; only the switch's default changes.
- **Admin delete** needs a decision on whether it uses Bounce's existing
  `restrictUserManagement` permission or a new one. Signal ties it to the plain
  admin role, which maps onto `Group::admins`, and that is the recommendation —
  but it means an admin can remove anybody's message in a group, which is worth
  being deliberate about rather than inheriting.
- **Reaction visibility to a Go member of a group.** They will not see reactions,
  and their device will still acknowledge the frame if the whitelist lands, so
  our delivery ticks will say delivered. That is honest — it was delivered — but
  the companion document should say plainly that a mixed group has members who
  cannot see part of the conversation.

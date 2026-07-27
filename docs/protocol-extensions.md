# Protocol extensions: replies, reactions, and message deletion

**For the maintainers of the Go implementation (`chat/`, `ui/`, `fyne/`, `android/`).**

This document has two halves and they can be acted on independently.

- **[Part one](#part-one-the-compatibility-request)** asks for a small change so
  that Go builds tolerate two new frame types instead of disconnecting. It is
  roughly fifteen lines and requires implementing nothing.
- **[Part two](#part-two-the-frame-specifications)** is the full specification of
  what those frames mean, so the features can be implemented whenever you choose.

Nothing here requires feature parity. Part one alone is enough to keep the two
implementations on the same network.

---

## Why you are getting this

The Rust/Electron client is adding three message interactions the protocol does
not currently express: replying to a specific message, reacting to one with an
emoji, and deleting one after it has been sent.

Replies need nothing from you. They ride an added map key on `directMessage` and
`groupMessage`, and your decoder already skips keys it does not recognise —
verified against `github.com/Basekick-Labs/msgpack/v6 v6.1.0`, in both
directions. Because `getPayload()` re-emits `OriginalPayload` byte for byte
(`chat/direct_message.go:87-99`), a Go node relaying a reply passes the quote
through intact and the signature still verifies. You can ignore that half of this
document entirely.

Reactions and deletion are different, because both refer to a message that has
already been sent and neither can ride it. They need frame types of their own.

---

## Part one: the compatibility request

### The problem, precisely

`readFrames` treats an unknown frame type as fatal:

```go
// chat/remote_device.go:224-231
handler, ok := handlers[frameType]
if !ok {
    log.WithFields(log.Fields{
        "peer": peer,
        "type": frameType,
    }).Error("peer sent an unsupported frame type, disconnecting")
    conn.Close()
    return
}
```

And `handleCatchUp` refuses the whole bundle rather than the one frame:

```go
// chat/catch_up.go:131-136
if _, present := allowedCatchUpFrames[fr.Type]; !present {
    log.WithFields(log.Fields{ ... }).Warn(
        "refusing to process catch up that contains frame not allowed in catch ups")
    return nil, false
}
```

Together these turn "a peer sent me something I do not understand" into a
persistent outage rather than a shrug. The reference flow re-offers any frame a
peer has not acknowledged, so the disconnect recurs on every reconnection, and a
catch up containing a single reaction drops every other frame in the bundle with
it. The observable result is not "reactions do not appear" — it is that the peer
relationship stops working, including for plain messages.

We are not asking you to change that policy in general. Refusing the unknown is a
defensible default for a protocol with no negotiation, and we would probably have
written it the same way.

### What we are asking for

Accept and discard two specific types, and allow them in catch ups:

```go
const (
    typeReaction      = uint16(52)
    typeDeleteMessage = uint16(53)
)

// A frame we know about and have chosen not to implement. Acknowledging it is
// the point: without the ack the sender re-offers it on every reference cycle
// forever.
func (b *Bounce) handleUnimplemented(peer string, payload []byte, _ bool) (broadcastable, bool) {
    sc, err := b.unpackSignedContainer(payload)
    if err != nil {
        return nil, false
    }
    var frame struct {
        ID uuid.UUID
    }
    if err := msgpack.Unmarshal(sc.Payload, &frame); err != nil {
        return nil, false
    }
    go b.sendAck(peer, frameType, frame.ID)
    return nil, false
}
```

registered in `getHandlers` for both types, plus both added to
`allowedCatchUpFrames`.

The acknowledgement is the part that matters and it is easy to leave out. Every
one of these frames carries an `ID` at the top level of its payload, in the same
position `readReceipt` does, specifically so that a handler which understands
nothing else can still ack. Without the ack, delivery records are never written
and the sender re-offers the same frame on every reference cycle, permanently —
the behaviour `chat/ack.go:47` already exists to prevent.

Returning `nil, false` is deliberate: the frame is not rebroadcast, so a Go node
does not relay reactions onward. That costs redundancy, not delivery — frames are
addressed to every device in scope directly.

### What we are doing on our side in the meantime

Two things, so that this is not urgent for you.

1. **A capability gate.** `device` gains an optional `Capabilities` key listing
   the extensions a device understands. Absent means legacy, which is the correct
   default for every build that exists today. We only send types 52 and 53 to
   devices that advertise them, so a Go peer should not receive one at all. The
   key rides inside `user.Devices`, which you already relay byte for byte, so it
   reaches other Rust peers through a Go intermediary without your involvement.

   ```
   device {
     ...existing fields...
     Capabilities []string   // e.g. ["react", "delete", "quote"]
   }
   ```

   You can ignore this key. If you later want the same protection in the other
   direction, reading it costs one field.

2. **A client-side switch** that stops us emitting the new frame types at all,
   for anyone who hits trouble before a whitelisted build is out.

Part one is therefore belt-and-braces rather than a prerequisite. We would still
like it, because a capability gate is only as good as the records it reads and a
device that never re-announced will read as legacy — the failure mode being that
we *under*-send, which is safe, but the reverse would not be.

### What a mixed network looks like after part one

Honestly stated, because it is the thing to weigh:

- A Go user in a group where others are reacting sees the messages and not the
  reactions. There is no marker; the reactions are simply not there.
- A message deleted by its author stays visible on Go clients. This is the one
  worth thinking hardest about, because a user who deletes something reasonably
  believes it is gone, and on a mixed network it is not gone everywhere. Our
  interface will say so once we know Go peers are on the network; until part two
  lands, "delete for everyone" means "everyone running a client that implements
  it".
- Everything else — messages, attachments, groups, receipts, typing, retention —
  is unaffected.

---

## Part two: the frame specifications

Field names are the msgpack keys. Every frame is carried inside the usual
`signedContainer`, and `Signer`, `OriginalPayload` and `Signature` are
`msgpack:"-"` as everywhere else.

### `typeReaction` = 52

```go
type reaction struct {
    SignedFrame
    ID         uuid.UUID
    Actor      uuid.UUID  // who is reacting
    Target     uuid.UUID  // the message reacted to
    TargetType uint16     // typeDirectMessage or typeGroupMessage
    Emoji      string     // one emoji; empty when Remove is set
    Remove     bool       // withdraw this actor's reaction
    Timestamp  int64

    // Derived from the target message on receipt, never carried — a reaction
    // must not be able to claim a conversation it does not belong to. Same
    // treatment readReceipt gives Destination and Scope.
    Destination uuid.UUID `msgpack:"-"`
    Scope       int64     `msgpack:"-"`
    SavedAt     int64     `msgpack:"-"`
}
```

**Scope and destination** resolve exactly as `readReceipt` does: from the target
message, not from the frame.

**State.** One reaction per actor per message; last write wins on `Timestamp`;
`Remove` clears. Storing them keyed on `(Target, Actor)` gives you that for free.

**Validation.** Signer speaks for `Actor`; the actor is a participant in the
target's conversation; and `Emoji` is a single emoji grapheme cluster. The last
one is not fussiness — it is the only new place a remote peer chooses characters
that get rendered, so an unchecked string is an arbitrary-content injection into
the message list.

**Retention.** A reaction inherits its target's `DeleteAt` and must not outlive
it.

**Ordering.** `catchUpOrder` 16, after `typeDraft` at 15. Appended rather than
inserted, so no existing value moves.

**Early arrival.** A reaction can precede its target on a catch up. Park it and
resolve when the message lands, the way `readReceipt` already does.

### `typeDeleteMessage` = 53

```go
type deleteMessage struct {
    SignedFrame
    ID          uuid.UUID
    Actor       uuid.UUID  // the author, or a group admin
    Target      uuid.UUID
    TargetType  uint16
    AdminDelete bool       // acting as an admin rather than as the author
    Timestamp   int64

    Destination uuid.UUID `msgpack:"-"`
    Scope       int64     `msgpack:"-"`
    SavedAt     int64     `msgpack:"-"`
}
```

**Permission and window**, following Signal Desktop 8.20
(`ts/util/canDeleteForEveryone.preload.ts`):

| Actor | May delete | Window from the target's `WrittenAt` |
|---|---|---|
| the message's author | their own message | 24 hours |
| a group admin | anybody's message in that group | 24 hours |
| receiver's acceptance | either of the above | 48 hours — the window plus a day of grace |

The grace period exists because a frame can sit in a reference queue while a peer
is offline, and rejecting it on arrival would make deletion unreliable in exactly
the case where it matters most.

Signal derives message age from a server timestamp. There is none here, so
`WrittenAt` is used. It is author-controlled, which means an author can backdate
to widen their own window — a limitation worth knowing about and not worth
defending against, since the alternative is a timestamp nobody can establish.

**Note to self** has no time limit and is `scopeSync`. Signal excludes its
note-to-self from delete-for-everyone entirely; here a note to self is a real
frame synced across your own devices, so deleting it on all of them is deleting
your own data on your own machines rather than reaching anybody else.

**Effect — the part to get right.** Do not delete the row. Empty it: clear the
body, drop the attachment rows and the `file` rows they reference (your
`AfterDelete` hooks in `chat/file.go:84-109` then free the chunks and the file on
disk), and set a tombstone.

Deleting the row outright makes the deletion undo itself. `hasFrame` starts
answering false, the peer classifies the original as wanted, offers it back, and
the message returns. The tombstone is what makes a deletion permanent, and it is
also what lets you keep acknowledging the original so the offer stops.

Reactions to a deleted message go with it.

**Ordering.** `catchUpOrder` 17 — last, so a deletion applies after anything it
might target has been replayed.

### The `Quote` key on `directMessage` and `groupMessage`

No new type; an optional key on two existing frames.

```go
type quote struct {
    Target    uuid.UUID  // the message replied to
    Author    uuid.UUID
    Text      string     // bounded excerpt, at most 160 characters
    Kind      uint16     // 0 text, 1 image, 2 file
    ExpiresAt int64      // the ORIGINAL's DeleteAt, not the reply's
}

// on directMessage and groupMessage:
Quote *quote
```

A snapshot rather than a bare id, so a reply renders when it arrives before its
target — ordinary on a catch up — or when the original has since expired.

`ExpiresAt` is the original's expiry, not the reply's, and it is there because
Bounce has disappearing messages and Signal's quote design does not. A plain
snapshot re-publishes the text of a thirty-second message under the reply's
retention, which may be unlimited. Carrying the original's expiry lets a client
blank the quote on schedule and show "Original message not available" while
keeping the reply. If you implement quotes, please honour it; a client that
ignores it turns every reply into a retention leak.

---

## Reference material

Rust definitions, wire fixtures, and a byte-level round-trip suite live in
`rust/libbounce/src/frames/` and `rust/libbounce/tests/go_interop.rs`. The
existing fixture exchange is the fastest way to check an implementation against
ours in both directions, and we are happy to add fixtures for these three on
request.

Questions, corrections, and disagreements about any of the above are welcome —
in particular the frame numbers, which are the one thing that is expensive to
change later and cheap to change now.

# The Rust core and Electron client

This document describes the Rust implementation of the Bounce protocol
(`rust/`) and the Electron desktop client built on it (`electron/`). It covers
the architecture, how the pieces fit together, what is finished, and what is
not.

## Why this shape

Signal Desktop separates a Rust cryptography and protocol core (`libsignal`)
from a TypeScript interface, bridged by a native Node module. The core owns
keys, protocol state, and the database; the interface owns pixels. The bridge is
narrow and explicitly enumerated.

Bounce's port follows the same division, for the same reasons: protocol
mistakes are security bugs, and a memory-safe implementation with a strong type
system is a better place for them than a renderer. Nothing in the Electron
process ever holds a private key or parses a frame off the wire.

```text
┌──────────────────────────────────────────────────────────────┐
│  renderer  (React, sandboxed, no node integration)           │
│    state.ts reducer ── components ── styles.css              │
└───────────────────────────┬──────────────────────────────────┘
                            │ contextBridge: enumerated methods
┌───────────────────────────┴──────────────────────────────────┐
│  Electron main process                                       │
│    src/main/index.ts   window + IPC handlers                 │
│    src/main/engine.ts  loads the native module               │
└───────────────────────────┬──────────────────────────────────┘
                            │ N-API (napi-rs)
┌───────────────────────────┴──────────────────────────────────┐
│  bounce-node   thin binding: lifetimes, threading, JSON      │
├──────────────────────────────────────────────────────────────┤
│  libbounce   the protocol                                  │
│    engine ── consensus ── scope ── device_group              │
│    frames ── signed ── msgpack ── wire                       │
│    crypto ── onion ── store ── net                           │
└──────────────────────────────────────────────────────────────┘
```

## Wire compatibility with the Go implementation

The Rust core speaks the same protocol as `chat/`, not a lookalike. Three
details make that true:

**MessagePack.** Go encodes with `Basekick-Labs/msgpack/v6`, which writes
structs as maps keyed by the exact Go field name and `[]byte` as `bin`.
`rmp-serde`'s defaults differ on both counts, so [`msgpack`] turns on
struct-map mode and every byte field carries `#[serde(with = "serde_bytes")]`.
`uuid::Uuid` already encodes as a 16-byte `bin` under a non-human-readable
serializer, matching Go.

**Signatures cover transmitted bytes.** Go writes integers at fixed width;
`rmp-serde` writes them compactly. Both are valid MessagePack and each decodes
the other, so the encodings are not byte-identical — and they do not need to be.
A signature always covers the exact payload that went on the wire, and received
frames keep that buffer in `SignedFrame::original_payload` rather than being
re-encoded. The same applies to group IDs, which hash the transmitted creation
blob.

**Tor keys are pre-expanded.** Tor stores hidden service keys as 64 bytes of
clamped scalar plus nonce prefix, not as a 32-byte seed. `DeviceKey::Expanded`
signs with those directly via `ed25519_dalek::hazmat`, producing ordinary
Ed25519 signatures that verify against the public key recovered from the onion
address.

### Interop is tested against real Go output

`rust/libbounce/tests/fixtures/` contains a Go harness that links the same
libraries `chat/` depends on. It runs in both directions:

```bash
# Go → Rust: decode and verify Go-encoded frames
cd rust && cargo test -p libbounce --test go_interop

# Rust → Go: have Go decode and verify Rust-encoded frames
cargo run -q -p libbounce --example emit_fixtures \
  | (cd libbounce/tests/fixtures && go run . verify)
```

The reverse direction confirms Go accepts Rust's signed containers, derives the
same group ID from Rust's creation blob, and recovers the same onion address
from Rust's device key.

## The core, module by module

| Module | What it does |
|---|---|
| [`wire`] | The six-byte type-length-value header, with separate payload caps for known and unknown devices |
| [`msgpack`] | Go-compatible encoding, described above |
| [`crypto`] | Ed25519 (seed and Tor-expanded), X25519, AES-256-GCM in Go's random-nonce layout, BLAKE3 |
| [`onion`] | v3 onion address encoding — a device's address *is* its public key |
| [`signed`] | The signed container that authenticates frames |
| [`frames`] | Every frame struct, with local-only fields marked `#[serde(skip)]` |
| [`device_group`] | Mutual-signature validation: connectivity, founder rooting, revoked introducers |
| [`scope`] | Resolving a frame's scope to concrete device addresses |
| [`consensus`] | Group state, permissions, and the canonical stack |
| [`store`] | SQLite persistence |
| [`net`] | The `Network` trait, the handshake, the Tor transport, and a TCP one for tests |
| [`engine`] | Peer sessions, broadcast, frame dispatch, the reference flow |

### Group consensus

The subtlest part, and the one most worth reading: `consensus/stack.rs`.

Timestamps are forgeable, so ordering by them alone lets an attacker win any
race by backdating. Bounce's answer is confirmations — devices broadcast a
signature for each valid update they see, and when two updates conflict the
earlier one wins *unless* the later one has majority confirmation and the
earlier does not. The stack implements that by unwinding accepted history to
find the conflicting update, comparing confirmation counts, and replaying.

The scenario is tested directly, in both directions:
`the_backdating_attack_fails_when_the_honest_update_has_majority` and
`a_confirmed_update_displaces_an_unconfirmed_conflict`.

### Scope resolution

`scope.rs` is where a privacy mistake would be a leak rather than a crash, so
it is a pure function over a trait rather than something tangled into the
database. The asymmetry worth noting is global scope: our own profile updates
may go to any contact, but someone else's may only be relayed to users who
share a group with them, because a shared group is proof the author already
exposes their profile to that user.

## Running it

```bash
# Build the core and the native module
cd rust
cargo test                       # 239 tests, TCP transport
cargo test --features tor        # plus the Tor transport's own tests
cargo build --release -p bounce-node

# Build and run the client
cd ../electron
npm install
npm run build                    # stages the native module, bundles main + renderer
npm start
```

`npm run package` produces a distributable for the current platform;
`npm run package:all` targets macOS, Windows, and Linux. The native module is
staged as `native/bounce.node` — Node only loads a file as an addon if it
carries that extension, which is why the cargo artifact is copied rather than
loaded in place.

The first launch bootstraps Tor and publishes the onion service, which takes
tens of seconds. `BOUNCE_NO_TOR=1 npm start` skips that during development, at
the cost of all metadata protection.

To exercise a conversation, run two clients with separate profile directories:

```bash
BOUNCE_NO_TOR=1 BOUNCE_DATA_DIR=/tmp/bounce-a npm start
BOUNCE_NO_TOR=1 BOUNCE_DATA_DIR=/tmp/bounce-b npm start
```

Create a profile in each, then pair them by copying one's code into the other's
add-contact box. Without a hidden service directory the two find each other
through a rendezvous file in the system temp directory (`BOUNCE_DEV_PEERS`
overrides it) — a development shim that publishes local ports in the clear, and
which the Tor transport does not use.

### Previewing the interface

`electron/scripts/preview.cjs` renders the interface against fixture data and
writes a PNG, without a live engine or a second device:

```bash
cd electron
npx electron scripts/preview.cjs light.png
BOUNCE_PREVIEW_THEME=dark BOUNCE_PREVIEW_ROW=1 npx electron scripts/preview.cjs dark.png
```

## The interface

The renderer reproduces Signal Desktop's visual system: the ultramarine accent
(`#2c6bed`), Signal's neutral gray ramp, Inter at 14px, 18px bubble radii with
a 4px tail on the last message of a run, the 380px left pane, 48px conversation
avatars with deterministic tints, and the same light and dark palettes.

State is a single reducer over engine events — Signal keeps its renderer state
in Redux, and this is the same idea without the dependency. Every action is
either a snapshot the engine handed over or an event it emitted, so the reducer
only folds in facts and never decides anything the engine has not.

The bridge in `src/preload/index.ts` is the renderer's entire attack surface:
every method is enumerated by hand and there is no generic `invoke(channel, …)`
escape hatch. `EngineEvent` is a discriminated union mirroring the Rust `Event`
enum, so a variant renamed on the Rust side fails to compile here rather than
silently doing nothing.

## The Tor transport

`net/tor.rs` runs an in-process v3 onion service via
[arti](https://gitlab.torproject.org/tpo/core/arti), the Tor Project's own Rust
implementation. That replaces the Go client's dependency on go-libtor, which is
C tor behind cgo — the reason its first build is slow and its Windows build
needs a mingw cross-compiler.

### The device key is the onion identity

Bounce's identity model requires the service to come up under the key already
persisted, not one arti generates. `HsIdKeypair` stores an *expanded* secret key
"for compatibility with the C tor implementation, and in order to support
custom-generated addresses" — the same 64 byte form
`DeviceKey::to_expanded_bytes` produces. So a device keeps its address across
the port from Go and across restarts, with no migration.

`TorNetwork::start` verifies this rather than assuming it: if the address arti
publishes differs from the one derived from our key, it fails loudly instead of
quietly becoming a different device. The check is meaningful because the two
addresses are derived independently — one through arti's `HsIdKey`, one by
[`onion`] encoding the public key by hand.
`arti_derives_the_same_onion_address_as_bounce` asserts they agree over randomly
generated keys.

### Two arti behaviours that had to be worked around

Both were found by compiling and running a probe against the real crate rather
than from documentation — docs.rs's build of 0.44.0 failed.

**`launch_onion_service_with_hsid` refuses to overwrite.** It inserts the
supplied key into the primary keystore with `overwrite = false`, so the *second*
launch under a nickname fails with `KeyAlreadyExists`. Every restart would have
hit this. The fix clears the service's keystore directory first, which also
guarantees the running service uses our key. Falling back to plain
`launch_onion_service` on error would be worse: a diverged stored key would
bring the service up under the wrong address without complaint.

**The keystore is on disk.** That same insert is not ephemeral, so arti's state
directory holds a copy of the identity key. It is created `0700` and must be
treated as secret-bearing.

Two smaller ones: arti leaves the rustls provider unselected, so the first TLS
handshake panics unless `ring` is installed explicitly at start-up; and
`launch_onion_service_with_hsid` sits behind arti's `experimental-api` feature,
outside its semver guarantees, which is why `arti-client` is pinned to
`=0.44.0`.

`DataStream` implements tokio's `AsyncRead`/`AsyncWrite` natively, so the wire
codec needed no adapter.

### Running without it

`BOUNCE_NO_TOR=1` selects the TCP transport, for development where waiting on a
bootstrap per restart is not workable. It provides **no metadata protection**,
and the interface says so in a banner rather than letting it pass unnoticed.

## The handshake signing oracle

**This affects the Go implementation and should be fixed there.**

A device's key signs two things: handshake responses, and the BLAKE3 digest
inside every signed frame. In Go, `TorNetwork.Dial` signs the challenge exactly
as the listener sent it:

```go
challenge, err := read(conn, handshakeChallengeSize)  // 32 bytes, peer-chosen
response := bounceTor.Sign(challenge)
```

while `createSignedContainer` signs `blake3.Sum256(payload)` — also 32 bytes,
with the same key and no domain separation. The two message spaces are
identical.

So **any address you dial is a signing oracle**. A malicious listener sends
`BLAKE3(frame)` where the random challenge belongs and receives a signature that
verifies as a frame authored by your device. It can then send your contacts
messages, group removals, or consensus confirmations in your name, harvesting a
fresh signature on every reconnect. Nothing downstream can tell the difference,
because the signature is genuine.

This port signs a domain-separated transcript instead:

```text
BLAKE3("bounce-handshake-v1" || listener_address || challenge)
```

The tag makes the two spaces disjoint — producing a frame signature this way
would need a BLAKE3 preimage. Binding the listener's address also stops the
response being relayed, so a peer you dial cannot turn around and authenticate
as you somewhere else.

`a_dialed_peer_cannot_use_the_handshake_as_a_signing_oracle` runs the attack and
asserts the harvested signature does not validate the forged frame.

### Interoperating with a Go client

Verifying and signing are treated differently, because only one of them is
dangerous:

- **A Go client can dial us with no configuration.** As listener we try the
  transcript and fall back to the bare challenge. Verifying costs nothing — we
  produce no signature — so accepting the old form loses no safety.
- **Dialing a Go client needs `BOUNCE_GO_COMPAT=1`.** There we choose what to
  sign, and signing a peer-supplied blob is the vulnerability itself. It is
  opt-in, logs a warning, and should be considered temporary.

The real fix is to patch `network/tor.go` to sign a transcript too, after which
the flag is unnecessary.

Frame encoding and signatures are unchanged, so the interop tests still pass in
both directions; it is the transport handshake alone that diverges.

## Other divergences from the Go implementation

The port follows the Go reference closely, except where reading it closely
turned up a weakness. These are deliberate:

| Area | Go behaviour | Here |
|---|---|---|
| `addUser` | Which side is "us" is taken from the record, and our consent is only checked against a device list the sender wrote — so a forged record adds a stranger to your contacts | The device that signed our half must be one our own database says we own |
| `addUserRequestAccepted` | No pending-request state, so any peer can send one unprompted and be adopted as a contact | Refused unless we actually sent a matching request, within five minutes |
| Re-adding a known user | The wire-supplied device group is merged unconditionally | Only devices that are valid additions to the group already held are taken |
| Read receipts | The actor is never checked against the conversation, so an outsider's receipt is stored, relayed, and shown to the author | The actor must be a participant |
| Typing indicators | The author is never checked against the thread, so a known device can appear to be typing in any conversation | The author must be a group member, or the direct thread must be ours |
| Device rows | Upserted on a peer-chosen UUID, so a record about one user can overwrite another user's device | Upserted on the address, which is the real identity; revocations are never undone |
| Burned secrets | An in-memory set, so a restart makes a used secret usable again | Persisted |
| Requester name | Never validated, so a name may contain newlines | Validated like any other |
| Update timestamps | Second resolution, so an update can sort before the one it answers | Local updates advance past the newest already held for that group |

## Status

### Working end to end

Verified by `rust/libbounce/tests/engine_e2e.rs`, which runs two engines over
real sockets with real signatures:

- **contact introduction** — two strangers become contacts by one scanning the
  other's code, with no directory involved; codes are single-use and expire
- profile creation, device keys, onion addressing
- direct messages, including notes to self staying inside the device group
- group creation, invitations, accepting, renaming, leaving
- group messages, with invitees correctly excluded until they join — and
  correctly receiving the group's history once they do
- **read receipts**, including ones that arrive before the message they refer to
- **typing indicators**, throttled on send, withdrawn on timeout or on the
  message arriving
- delivery tracking from acknowledgements
- the reference flow: messages written while a peer was offline arrive when it
  reconnects
- rejection of frames from unintroduced devices, forged signatures, unsolicited
  acceptances, and typing indicators for conversations their author is not in
- group consensus converging on both devices
- **attachments** — a file sent from one instance arrives byte-for-byte at the
  other, fetched a chunk at a time, and a peer that answers with different
  bytes under the same hash is ignored
- **status rows** — a rename or an invitation reaches the other side and is
  replayed in the opening snapshot
- interop in both directions: the Go harness in
  `libbounce/tests/fixtures/` decodes Rust-encoded messages, groups, acks,
  files and chunk offers, and `tests/go_interop.rs` decodes Go-encoded ones

### Implemented but not yet wired into the engine

The frames, crypto, and helpers exist and are tested; the engine does not drive
them yet:

- **Encrypted devices** — sealing, recipient lists, the reference-offer
  challenge, and recipient pruning are all in `frames/encrypted.rs`. The device
  management flow is not.
- **Large files** — anything above `EMBEDDED_FILE_LIMIT` (20 MiB) is seeded in
  place from disk in the Go implementation, which needs a path that survives
  restarts and a reader that streams. Sending one fails with a message saying
  so rather than truncating it; smaller files work end to end.

### Not started

- Multi-device pairing — a second device joining an existing profile. The frames
  exist in `frames/pairing.rs`; the flow does not.
- Avatars and group images. `FileType::UserImage` and `GroupImage` exist and a
  user record carries an image list, but nothing sets one.
- Profile-wide settings reaching a user's *other* devices. The frame type is
  reserved as `FrameType::UpdateSettings`; a change applies to the device it
  was made on and no other.

### Not carried over

The Go implementation's Fyne UI and Android service have no counterpart here;
the Electron client replaces the former, and Android is out of scope for a
desktop port.

[`wire`]: ../rust/libbounce/src/wire.rs
[`msgpack`]: ../rust/libbounce/src/msgpack.rs
[`crypto`]: ../rust/libbounce/src/crypto.rs
[`onion`]: ../rust/libbounce/src/onion.rs
[`signed`]: ../rust/libbounce/src/signed.rs
[`frames`]: ../rust/libbounce/src/frames/
[`device_group`]: ../rust/libbounce/src/device_group.rs
[`scope`]: ../rust/libbounce/src/scope.rs
[`consensus`]: ../rust/libbounce/src/consensus/
[`store`]: ../rust/libbounce/src/store/
[`net`]: ../rust/libbounce/src/net/
[`engine`]: ../rust/libbounce/src/engine/

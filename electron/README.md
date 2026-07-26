# Bounce desktop

The Electron client, built on the Rust core in [`../rust`](../rust).

Runs on macOS, Windows, and Linux. The interface reproduces Signal Desktop's
visual system — the ultramarine accent, the neutral gray ramp, Inter at 14px,
18px bubble radii, the 380px left pane, and matching light and dark palettes.

## Running it

The Rust core must be built first; the client loads it as a native module.

```bash
cd ../rust && cargo build --release -p bounce-node
cd ../electron

npm install
npm run build     # stages native/bounce.node, bundles main + preload + renderer
npm start
```

`npm run dev` does the build and launch in one step.

The first launch bootstraps Tor and publishes this device's onion service, which
takes tens of seconds. During development:

```bash
BOUNCE_NO_TOR=1 npm start
```

skips it — at the cost of **all metadata protection**. The interface shows a
banner whenever that mode is in force.

## Talking to a Go client

A Go client can connect to this one with no configuration. Going the other way —
this client dialing a Go one — needs:

```bash
BOUNCE_GO_COMPAT=1 npm start
```

because the Go handshake requires signing a challenge the peer chose, which is
[a signing oracle](../docs/rust-electron.md#the-handshake-signing-oracle). The
flag is opt-in for that reason and logs a warning. Pairing codes are the same
`<address>:<secret>` format in both.

## Trying it with two clients

Bounce needs someone to talk to, and there is no server to register with. A
device's identity *is* its key, so testing first-run behaviour or re-pairing the
same two clients means starting from an empty profile:

```bash
# first terminal
npm run fresh

# second terminal
npm run fresh -- b
```

Each wipes its profile, rebuilds, and launches. Profiles live under the system
temp directory, so nothing touches your real application data.

```bash
npm run fresh -- b --tor         # over Tor instead of plain TCP
npm run fresh -- a --keep        # relaunch without wiping
npm run fresh -- a --go-compat   # dial a Go client (see above)
npm run fresh -- a --reset-peers # also clear the local rendezvous file
```

`npm run fresh:a` and `fresh:b` are shorthands for the first two.

Create a profile in each. Then in one, click the compose icon, copy the code it
shows, paste it into the other's **Add a contact** box, and press Add. They
appear in each other's conversation list and can message.

`fresh` rebuilds the TypeScript but not the Rust. After changing the core:

```bash
cd ../rust && cargo build --release -p bounce-node && cd ../electron
```

Without Tor there is no hidden service directory, so the two instances find each
other through a rendezvous file in the system temp directory
(`BOUNCE_DEV_PEERS` overrides the path). That is a development shim: it
publishes which local port each device is on, in the clear. Over Tor the address
is all a peer needs and nothing is published locally.

## Packaging

One command builds the Rust core, stages it, bundles the client, and produces
an installer:

```bash
npm run package             # installer for this machine
npm run package:dir         # unpacked app only, much faster to iterate on
npm run package:universal   # macOS arm64 + x86_64 in one bundle
npm run package -- --arch x64
```

Output lands in `release/`.

### One machine, one platform

The engine is a compiled native module, so a package is only valid for the
platform and architecture it was built for. `electron-builder --mac --win
--linux` would wrap this machine's binary in three installers and two of them
would fail to load the engine at launch, with nothing to indicate why until a
user ran one.

So `package` builds the Rust for the target it is packaging and refuses targets
it cannot compile for. Producing all three means running it on all three, or
wiring cross-linkers into CI. There is deliberately no `package:all`.

Cross-*architecture* on the same OS works if the Rust target is installed:

```bash
rustup target add x86_64-apple-darwin
npm run package:universal
```

### Size

About 250 MB unpacked on macOS: roughly 180 MB of Electron and 13 MB of engine,
most of the latter being arti. Nothing from `node_modules` is shipped — React is
bundled into the renderer at build time, so it is a dev dependency.

macOS builds are unsigned; distributing them needs a Developer ID and
notarisation.

## Previewing the interface

Renders against fixture data and writes a PNG — no engine, no second device,
and no screen recording permission needed:

```bash
npx electron scripts/preview.cjs light.png
BOUNCE_PREVIEW_THEME=dark BOUNCE_PREVIEW_ROW=1 npx electron scripts/preview.cjs dark.png
```

## Layout

```text
src/main/       window, IPC handlers, and loading the native module
src/preload/    the contextBridge — the renderer's entire attack surface
src/renderer/   React interface
  state.ts      one reducer folding engine events into view state
  styles.css    the visual system, as custom properties
```

The renderer is sandboxed: context isolation on, node integration off, and a
content security policy that forbids remote origins outright, since everything
is bundled. No private key or wire frame ever reaches it.

## Adding a contact

There is no directory and no search. Open the compose button, show the generated
code to someone next to you, and paste theirs back. Codes are single-use and
expire after five minutes.

Architecture and status: [`docs/rust-electron.md`](../docs/rust-electron.md).

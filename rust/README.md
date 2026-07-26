# libbounce

The Bounce protocol and cryptography, in Rust.

- **`libbounce`** — the protocol: wire format, cryptography, frames, device
  groups, scopes, group consensus, storage, transport, and the chat engine.
- **`bounce-node`** — N-API bindings that expose the engine to the Electron
  client in `../electron`.

It speaks the same wire protocol as the reference Go implementation in
`../chat`, and that is verified against real Go output in both directions —
see [Wire compatibility](../docs/rust-electron.md#wire-compatibility-with-the-go-implementation).

## Build and test

```bash
cargo test                           # 239 tests
cargo test --features tor            # plus the Tor transport's own tests
cargo build --release -p bounce-node # the native module for Electron
```

The `tor` feature is off by default so the suite does not pay arti's build cost;
`bounce-node` turns it on.

Cross-language interop, which needs a Go toolchain:

```bash
cargo test -p libbounce --test go_interop          # Go → Rust
cargo run -q -p libbounce --example emit_fixtures \
  | (cd libbounce/tests/fixtures && go run . verify) # Rust → Go
```

## Where to start reading

`src/lib.rs` has a map of the modules. The most interesting are:

- **`src/consensus/stack.rs`** — how devices agree on group state without a
  server, and why forging a timestamp does not win the race.
- **`src/scope.rs`** — how a frame's scope becomes a concrete device list, and
  the asymmetry in global scope that keeps a contact's social graph private.
- **`src/net/tor.rs`** — why the persisted device key can be handed to arti as
  the onion identity, and the two arti behaviours that had to be worked around.

Full architecture and status: [`docs/rust-electron.md`](../docs/rust-electron.md).

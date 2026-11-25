# openmesh-ffi

UniFFI-ready foreign-function interface for OpenMesh mobile clients (Android first, iOS later).

**Status: v0.1.0 — implemented Rust surface, UniFFI bindings deferred to M2.** The Rust types
and methods in `src/lib.rs` are the intended final surface and delegate to the real
`openmesh-core` engine (announce, send, ingest, tick, nodes, contacts, channels, stats), with
unit tests. What is deliberately not in v0.1.0 is the `uniffi` dependency and the generated
Kotlin bindings; the crate builds with `cargo build -p openmesh-ffi` and adds no heavy
dependencies. The Android app keeps using `DemoEngineBridge` until M2.

The Android scaffold (`apps/android`) already defines the matching Kotlin seam in
`data/engine/EngineBridge.kt`; replacing its `DemoEngineBridge` with a wrapper over the generated
bindings is the M2 task.

---

## 1. Why UniFFI

* One Rust implementation, generated idiomatic Kotlin (`ByteArray`, `List`, sealed exceptions)
  and Swift bindings, no hand-written JNI.
* Keeps crypto and protocol logic in Rust; the app never re-implements OMP framing.
* Reference: <https://mozilla.github.io/uniffi-rs/>

## 2. Planned surface

| Function / type | Purpose |
| --- | --- |
| `FfiEngine::create(seed, config) -> Arc<FfiEngine>` | Construct the engine from a 32-byte seed and configuration |
| `FfiEngine::generate_seed() -> Vec<u8>` | CSPRNG seed for first-run identity creation |
| `FfiEngine::node_id() -> String` | Self-certifying 8-byte node id as hex |
| `FfiEngine::public_identity() -> Vec<u8>` | 72-byte identity for QR/contact sharing |
| `FfiEngine::send_text(peer_id, text, now_ms) -> FfiProcessed` | Seal and emit a direct message; `FfiProcessed.msg_id` is the message id |
| `FfiEngine::ingest(frame, rssi, snr, now_ms) -> FfiProcessed` | Process one OMP frame from the radio/bridge |
| `FfiEngine::tick(now_ms) -> FfiProcessed` | Timers: re-announces, retransmissions, resource ARQ, store offers, peer aging |
| `FfiEngine::announce(now_ms) -> FfiProcessed` | Build a signed announce frame |
| `FfiProcessed { events, outbound, msg_id }` | Events for the UI + the exact OMP frames to transmit (no Rust→Kotlin callbacks; the Kotlin adapter exposes a Flow) |
| `FfiEngine::nodes() -> Vec<FfiNode>` | Known nodes for the Nodes screen |
| `FfiEngine::stats() -> FfiStats` | Counters for the status/airtime UI |
| `FfiEngine::add_contact(identity, name)` / `remove_contact(node_id)` | Contact management (identity is 72 bytes from a QR code) |
| `FfiEngine::add_channel(name, psk)` / `remove_channel(channel_id)` | Channel management |
| `FfiEngine::set_announce_interval(ms)` | Runtime config |
| `FfiEngineConfig`, `FfiRole`, `FfiEvent`, `FfiNode`, `FfiMessage`, `FfiStats`, `FfiError` | Data/error types |

Everything takes `now_ms` from the caller: the engine never reads the wall clock, which keeps
tests and the simulator deterministic (see `docs/architecture/PROTOCOL.md` §12).

## 3. Design rules

1. **No callbacks across the boundary.** Events are queued and polled. A JNI callback that
   re-enters Kotlin from a Rust thread is the usual source of mobile FFI crashes; polling fits
   the app's coroutine/StateFlow model.
2. **No panics across the boundary.** Every fallible method returns `Result<_, FfiError>`. The
   stub returns `NotImplemented` rather than calling `todo!()` in the core.
3. **Secrets stay in.** The seed is copied on creation and never returned; `public_identity()`
   is public data. The identity should be stored in Android Keystore (M2) and handed to Rust
   only when the engine starts.
4. **Bounded inputs.** `ingest` enforces `MAX_FRAME_LEN` (= OMP MTU, 240 bytes); attachment bytes
   do not travel through this API — they go through the bridge/attachment store or the resource
   layer, chunk by chunk.
5. **Threading.** State is behind a `Mutex`; callers may invoke from any thread but must not
   assume re-entrancy. UniFFI objects are `Send + Sync` when their contents are.
6. **Version pinning.** `protocol_version()` returns `omp/1`; the app must refuse a radio/core
   mismatch (same rule as the companion `Hello` handshake).

## 4. Enabling UniFFI (M2)

Add to `Cargo.toml`:

```toml
[dependencies]
uniffi = { version = "0.29", features = ["cli"] }

[build-dependencies]
uniffi = { version = "0.29", features = ["build"] }
```

Add `uniffi::setup_scaffolding!();` to `src/lib.rs`, annotate the exported impl with
`#[uniffi::export]` (proc-macro style fits this small API better than UDL), then generate
bindings in CI:

```bash
cargo build -p openmesh-ffi
cargo install uniffi-bindgen --version 0.29.0
uniffi-bindgen generate --library target/debug/libopenmesh_ffi.so \
    --language kotlin --out-dir apps/android/app/src/main/java
```

Generated Kotlin lands under `apps/android/app/src/main/java/uniffi/openmesh/` and is not
hand-edited; `apps/android/.../data/engine/EngineBridge.kt` adapts it to the app's coroutines and
StateFlow. Add `cargo test -p openmesh-ffi` (Rust-side unit tests for seed validation, identity
round-trip, error mapping) to CI before generating bindings.

## 5. Implementation status

The v0.1.0 surface **delegates to the real `openmesh-core` engine** for construction, identity,
contacts, channels, nodes, stats, announce, send_text, ingest and tick, and maps every
`openmesh_core::Event` variant to a typed `FfiEvent`. What is still missing for a production
mobile client is not the FFI plumbing but the app-side integration and the Android milestone:

| Item | Status |
| --- | --- |
| Rust delegation + event mapping | done, with unit tests (`cargo test -p openmesh-ffi`) |
| UniFFI attributes + generated Kotlin bindings | M2 (add the dependency, annotate, generate in CI) |
| Android `EngineBridge` wrapper replacing `DemoEngineBridge` | M2 |
| Kotlin interop/instrumentation tests on a device | M2 |
| Long-running engine tests (resource transfers, store-and-forward) on-device | M2/M3 |

Until M2 the Android app keeps using `DemoEngineBridge`; the FFI crate stays buildable and
tested in CI.

## 6. Verification

```bash
cargo build -p openmesh-ffi        # must pass at all times
cargo test -p openmesh-ffi         # unit tests (added with the real implementation)
cargo clippy -p openmesh-ffi -- -D warnings
```

At v0.1.0 the build is the gate; bindings and Kotlin interop tests arrive in M2 together with
the Android GA milestone (`docs/ROADMAP.md`).

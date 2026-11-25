# ADR 0004 — Rust protocol core + C++ firmware + golden vectors

* **Status:** accepted (v0.1.0)
* **Date:** 2026-11-25
* **Decision record:** summary row 4 in
  [ARCHITECTURE.md §4](../architecture/ARCHITECTURE.md#4-key-decisions)
* **Related:** [ADR 0001](0001-own-protocol-vs-reticulum.md), [ADR 0005](0005-companion-radio-architecture.md),
  [PROTOCOL.md §15](../architecture/PROTOCOL.md), [research notes](../research/reticulum-lxmf-notes.md) §5.5

## 1. Context

OpenMesh has two implementations of the same wire protocol:

1. a **host** implementation used by the bridge, CLI, simulator and mobile clients
   (via FFI), and
2. a **firmware** implementation running on nRF52840 and ESP32-S3 radios.

They must stay byte-compatible. Two implementations of a crypto-heavy binary protocol are a
classic source of drift, and on LoRa a one-byte difference means dropped frames, not a warning.
We had to choose:

* **One language for both** (Rust everywhere, or C++ everywhere). Rust on nRF52840/ESP32-S3 is
  possible (`no_std`, `esp-hal`/`embassy`/`nrf-hal`), but the mature LoRa HALs (RadioLib,
  board support for the Seeed/RAK/Heltec boards) are Arduino C++, and the reference firmware we
  studied (`ratspeak-handheld`) uses arduino/PlatformIO for exactly this reason.
* **Rust host + C++ firmware, with a shared contract.** Two languages, but each on its strongest
  platform, with a conformance mechanism.
* **C++ everywhere** including hosts. Would mean re-implementing crypto in C++ (dangerous) and
  losing the Rust ecosystem for the bridge/simulator/mobile FFI.

We also studied the closest precedent: Ratspeak's handheld firmware. It uses Arduino C++ for
drivers/UI and a **Rust `no_std` protocol core compiled to an ESP32-S3 staticlib** (built with
espup/`-Zbuild-std`, pinned toolchain, prebuilt `.a` per memory profile with provenance files).
That validates the "two languages, one contract" approach but is heavier than our target
hardware: their core includes Reticulum Lite/LXMF Lite and LXST embedded, while our firmware only
needs relays, announces, store-and-forward and the companion link.

## 2. Decision

**The protocol logic lives twice by design:**

* `crates/openmesh-core` — Rust, IO-free, deterministic, `#![forbid(unsafe_code)]`, the reference
  implementation for wire format, crypto, flooding, resources, store-and-forward, companion
  codec and LoRa math. It powers the bridge, CLI, simulator and the FFI layer for mobile.
* `firmware/lib/openmesh` — C++17 (PlatformIO/Arduino), a thin but complete implementation of
  the same wire format and relay behaviour, with a HAL per board. No Rust on the MCU in v0.1.

**`protocol/vectors/` is the normative cross-language contract.** JSON vectors are generated
from the Rust core and replayed by both Rust and C++ tests. Protocol changes require a spec edit,
new vectors, passing core tests and a firmware conformance note.

### 2.1 What the vectors cover

| File | Covers |
| --- | --- |
| `frame.json` | header encode/decode, flags, TTL, malformed inputs |
| `identity.json` | seed → node id, public identity bytes, signatures |
| `seal.json` | DM/channel sealing with fixed nonce prefixes, AAD construction |
| `announce.json` | fixed binary announce, signature, self-certification |
| `app_message.json` | CBOR `AppMessage` round-trip and edge cases |
| `resource.json` | chunking, manifest, reassembly, ACK bitmaps, window math |
| `companion.json` | companion SLIP framing, CRC, CBOR payloads |

### 2.2 Rules that keep the two implementations honest

1. **The spec is normative, not the code.** `docs/architecture/PROTOCOL.md` defines the wire; both
   implementations follow it. If they disagree, that is a bug in one of them, found by vectors.
2. **Vectors are generated, not hand-written.** `cargo run -p openmesh-core --example gen_vectors`
   produces deterministic JSON with fixed seeds/nonces. A change to a vector requires a protocol
   version bump or an explicit "non-wire" justification.
3. **Firmware tests replay the same JSON** in a PlatformIO native environment, so a C++ change
   that breaks compatibility fails without a radio.
4. **The engine is IO-free and clock-injected.** `Engine::ingest(bytes, link, now_ms)` and
   `Engine::tick(now_ms)` mean the same inputs produce the same outputs in tests, vectors and
   the simulator. This is what makes cross-language determinism possible for crypto and CBOR.
5. **No C++ re-implementation of crypto primitives by hand.** The firmware uses a vetted library
   for Ed25519/X25519/ChaCha20-Poly1305 (or, where unavailable, the board's hardware-backed
   primitives) and is validated against vectors; constant-time properties are reviewed, not
   invented.

## 3. Consequences

### 3.1 Positive

* **Each platform uses its strongest ecosystem.** Rust: memory safety, cargo test, proptest,
  fuzzing, the simulator, UniFFI for mobile. C++: Arduino board support, RadioLib, PlatformIO
  toolchains for all four boards.
* **The firmware stays small and reviewable.** A relay does not need CBOR, resource ARQ or
  store-and-forward offers? — it does need a compact subset, and that subset is testable with
  the same vectors. On nRF52840 the binary and RAM fit with room for the radio HAL.
* **The simulator can be shared.** It models the Rust engine and a virtual LoRa medium; firmware
  behaviour is validated against vectors, not re-simulated in C++.
* **Testability without hardware.** The Rust core and vectors run on any CI machine; firmware
  native tests run without radios; hardware tests are the last gate, not the first.
* **A clean mobile path.** `crates/openmesh-ffi` wraps the same core for Android/iOS; the Kotlin
  scaffold is written against it from day one (see [ADR 0005](0005-companion-radio-architecture.md)
  and [ADR 0006](0006-web-first-clients-tauri-native-android.md)).

### 3.2 Negative

* **Two codebases, two bug surfaces.** Every protocol change must be implemented twice (or
  explicitly marked host-only). Golden vectors reduce drift but do not eliminate it: they cover
  the byte format, not scheduling, storage or state machines.
* **C++ firmware lacks Rust's safety guarantees.** Buffer handling in the frame codec, store
  queue and companion parser must be reviewed and fuzzed; the native test suite helps but is
  not a proof.
* **No `no_std` Rust shared core for firmware.** The Ratspeak handheld shows it is possible;
  we chose not to because our firmware surface is smaller and the Arduino HAL ecosystem is
  stronger for these boards. Revisit if the firmware grows links/ratchets/calls.
* **Vector maintenance cost.** Generating and committing JSON for seven categories, plus
  replaying them on device, is real CI time; it is the price of two implementations.
* **On-device crypto may differ subtly** (RNG quality, hardware vs software Ed25519). Vectors
  with fixed nonces test determinism, not RNG behaviour; add a self-test for the platform RNG.

### 3.3 Mitigations

* CI: Rust tests + vectors + C++ native vector tests + clippy/fmt on both.
* A "spec size model" test that asserts frame variants fit the MTU (born from a real bug; see
  [ai-development-workflow.md](../research/ai-development-workflow.md) §6).
* Firmware conformance note in every protocol PR (per ARCHITECTURE §12).
* A fuzz target for the frame/companion decoders; run it in CI for a fixed time, longer nightly.
* Keep the C++ surface minimal: if a feature needs complex state (resources, store-offer
  bookkeeping), implement it in the host and let the radio be a relay unless the feature is
  explicitly required standalone.

## 4. Alternatives considered

| Alternative | Why rejected |
| --- | --- |
| Rust everywhere (Rust firmware) | Strong safety story, but Arduino C++ HAL/board support is materially better for our four boards; embedded Rust toolchain/`no_std` ecosystem would be an additional project risk in v0.1; the Ratspeak precedent works but carries a large custom `no_std` Reticulum core we do not need. |
| C++ everywhere (C++ host too) | Re-implementing crypto/CBOR/redb/async in C++ is slower and riskier; loses Rust test/sim/FFI ecosystem; bridge maintenance would suffer. |
| Single Rust core compiled to a C ABI staticlib for firmware (Ratspeak-style) | Proven, but requires a firmware-grade `no_std` core, per-profile artifacts, toolchain pinning and C ABI plumbing; our firmware needs a smaller subset, and the team chose C++ for HAL maturity. Could be revisited for OMP/2 if shared logic grows. |
| Code generation from one schema (e.g. protobuf) for both languages | Helps data types but not crypto/state machines/flooding; adds a toolchain and runtime to firmware; CBOR is already simple and testable. |
| Shared C library (Rust core as a `no_std` staticlib) for all targets | Effectively the Ratspeak approach; rejected for v0.1 as above, but `openmesh-ffi` keeps the door open for mobile. |
| **Rust host core + C++ firmware + golden vectors (chosen)** | Best platform fit, smallest firmware, strong host tooling, and a concrete drift-control mechanism. |

## 5. Follow-ups

1. **M1:** stand up `pio test -e native` vector replay in CI for at least one nRF52 and one
   ESP32-S3 env; measure flash/RAM headroom per board.
2. **M1:** add fuzz targets for the C++ frame/companion decoders (AFL++ or libFuzzer with a
   harness) and a Rust `cargo fuzz` target; document corpora provenance.
3. **M1:** add the spec size-model test and generated Appendix A table (see ADR 0001 follow-ups).
4. **OMP/2:** re-evaluate a shared Rust `no_std` core if links/ratchets/calls land; the
   ratspeak-handheld C ABI and provenance practice is the template to copy (not its code).
5. **Release:** verify that the firmware binary and the host core both report the same
   `PROTO_VERSION`/`VERSION_STR`, and fail the build if they diverge.

# OpenMesh Architecture

**Status:** living document · v0.1.0 · 2026-11-25

OpenMesh is a LoRa-first mesh network for text, images and voice notes, with a client
ecosystem that does not require a phone number, account or internet. This document explains
how the system is put together, why it is built this way, and what is deliberately not built yet.

---

## 1. Goals and non-goals

### Goals

1. **Works with zero infrastructure.** Two handhelds + a solar repeater on a ridge is a
   complete network.
2. **Rich media over LoRa.** Compressed images and short voice notes are first-class, not
   afterthoughts, with honest airtime estimates before you hit send.
3. **Store-and-forward.** Repeaters hold encrypted envelopes for offline peers for days.
4. **One protocol, many transports.** LoRa, WiFi/Ethernet, USB, BLE — same frames, same
   security model.
5. **Clients that don't suck.** Web/PWA + packaged desktop app sharing one codebase, and a
   native Android app.
6. **Hackable and testable.** Deterministic core, golden vectors, a mesh simulator that runs
   thousands of nodes in one process.
7. **100 % AI-built, human-directed.** Every line is reviewable, documented and reproducible.

### Non-goals (v0.1)

* Replacing internet messaging at scale (this is a mesh, not a CDN).
* Real-time voice calls (voice *messages* only until OMP/2 ratchets land).
* Anonymity/anti-traffic-analysis guarantees.
* Compatibility with Meshtastic or Reticulum on the wire (bridges are planned, see §10).

---

## 2. Component map

```
┌──────────────────────────── clients ─────────────────────────────┐
│  apps/web (React PWA) ──┐             apps/android (Compose)      │
│  desktop via Tauri ─────┤                    │                     │
└─────────────────────────┼────────────────────┼─────────────────────┘
                          │ WebSocket API      │ companion protocol
                          ▼                    │ (USB / BLE)
                 ┌──────────────────┐          │
                 │ openmesh-bridge  │          │
                 │  (Rust daemon)   │          │
                 │ ┌──────────────┐ │          │
                 │ │ openmesh-core│ │          │
                 │ │  protocol    │ │          │
                 │ └──────────────┘ │          │
                 │ store (redb)     │          │
                 │ attachments      │          │
                 └────┬────────┬────┘          │
                      │        │               │
        ┌─────────────┘        └─────────┐     │
        ▼                                ▼     ▼
  ┌──────────┐   ┌──────────┐   ┌──────────────────┐
  │ serial   │   │ UDP/TCP  │   │ mesh radios      │
  │ radio    │   │ LAN/WAN  │   │ (firmware, C++)  │
  └────┬─────┘   └────┬─────┘   └────────┬─────────┘
       │              │                  │ LoRa 868/915 MHz
       ▼              ▼                  ▼
  ┌──────────────────────────────────────────────────────────────┐
  │                    OpenMesh mesh (OMP/1)                      │
  │  clients ◄──► trackers ◄──► repeaters ◄──► gateways ◄──► ...  │
  └──────────────────────────────────────────────────────────────┘
```

### Crates and packages

| Path | Contents |
| --- | --- |
| `crates/openmesh-core` | Wire format, crypto, announces, flooding, resources, store-and-forward, companion codec, LoRa math. IO-free state machine. |
| `crates/openmesh-bridge` | Daemon: transports (serial/UDP/TCP/**backbone**/simulated), redb store, attachment store, WS/REST API, static file serving, backbone control surface. |
| `crates/openmesh-cli` | Terminal client / diagnostics (send, listen, announce, node list, airtime calculator). |
| `crates/openmesh-sim` | Discrete-event simulator: virtual LoRa medium, loss/latency model, N nodes, scenario assertions. |
| `crates/openmesh-ffi` | UniFFI surface for native mobile clients. |
| `firmware/` | PlatformIO C++ firmware for the four reference boards, per-board HAL, companion link, store-and-forward. |
| `apps/web` | React + TypeScript + MapLibre client, also packaged with Tauri 2 for Linux/macOS/Windows. |
| `apps/flasher` | Static web flasher (esp-web-tools for ESP32, UF2 for nRF52) + links to release images. |
| `tools/flash` | Native CLI flasher for Fedora/Linux (esptool for ESP32, UF2 copy for nRF52). |
| `apps/android` | Kotlin/Compose app scaffold using the companion protocol. |
| `protocol/vectors` | Golden vectors shared by Rust and C++ implementations. |
| `docs/` | This doc, the protocol spec, ADRs, research notes, hardware notes. |

---

## 3. Request/data flow

### Sending a text

1. UI posts `send_message` over WS.
2. Bridge asks `openmesh-core` to build an `AppMessage`, seals it with the peer's static ECDH
   key (or the channel key), assigns the next `seq`, and returns an outbound frame.
3. Bridge hands the frame to each transport (radio via serial; LAN via UDP broadcast;
   simulator via virtual link).
4. Every node that hears it deduplicates, decrements TTL and re-broadcasts; the addressee
   decrypts, stores, acks.
5. The ack travels back the same way; the bridge marks the message delivered and pushes an
   `ack` event.

### Sending an image

1. The web client downscales/re-encodes the image (canvas + `createImageBitmap`) to a target
   size chosen from the current radio profile and region.
2. `POST /api/attachments` stores the bytes; `send_message` references the attachment id.
3. Core builds a manifest message (fits one frame) and streams chunks with selective-repeat
   ARQ; the UI shows progress in airtime terms ("~18 s of airtime").

### Receiving while the phone is away

1. A repeater hears a `Data` envelope for an absent node and stores the ciphertext.
2. Later the addressee announces; the repeater offers the stored ids, the addressee replies
   with what it is missing, and the repeater replays those envelopes flagged `FROM_STORE`.
3. The addressee's bridge deduplicates, decrypts, stores and acks.

---

## 4. Key decisions

The full records live in [`docs/adr/`](../adr/) (MADR format). Summary:

| # | Decision | Why |
| --- | --- | --- |
| 1 | [Build our own OMP protocol instead of speaking Reticulum](../adr/0001-own-protocol-vs-reticulum.md) | Reticulum is excellent but its link/resource machinery is heavy on MCUs, its client ecosystem is the pain point, and we need store-and-forward with tight LoRa airtime budgets. A bridge to Reticulum/LXMF is planned instead of forced compatibility. |
| 2 | [Managed flooding, not routing tables](../adr/0002-managed-flooding.md) | Proven at Meshtastic scale on single-channel LoRa; zero convergence time; repeaters are trivial. Directed routing is reserved for OMP/2. |
| 3 | [Static ECDH for v1, ratchet later](../adr/0003-static-ecdh-then-ratchet.md) | One-frame DMs, no handshake, tiny overhead. Forward secrecy is scheduled for OMP/2 (needed for calls). |
| 4 | [Rust protocol core + C++ firmware + golden vectors](../adr/0004-rust-core-cpp-firmware-golden-vectors.md) | Rust gives memory safety, testability and the simulator; C++/PlatformIO gives reliable Arduino support for ESP32 and nRF52 and a single codebase for all boards. Vectors keep them honest. |
| 5 | [Companion-radio architecture](../adr/0005-companion-radio-architecture.md) | Apps own UI, storage, crypto for their own traffic; radios only relay/announce/store. Repeaters are cheap and battery-friendly; apps can be updated without reflashing. |
| 6 | [Web-first clients (React + Tauri)](../adr/0006-web-first-clients-tauri-native-android.md) | One codebase covers web, Linux, macOS and Windows; MapLibre and MediaRecorder give maps and voice without native work. Android is native Compose for BLE/USB quality. |
| 7 | [Opus for voice, Codec2 optional](../adr/0007-media-pipeline.md) | Opus is universally decodable in browsers/phones and ~7 kbps for voice is acceptable on a 1–5 kbps mesh with chunking; Codec2 700C is available for the smallest presets. |
| 8 | [AGPL-3.0](../adr/0008-agpl-3-0-licensing.md) | Network copyleft keeps the mesh and every client fork open; compatible with the GPL-3.0 references we studied. |
| 9 | [Backbone mode](../adr/0009-backbone-mode.md) | Always-on authenticated TCP relay plus a loopback control panel: the deployment story beyond one radio's range. Token auth with end-to-end payload encryption; TLS/VPN termination documented for hostile networks. |

---

## 5. Hardware support

| Board | MCU | Radio | GNSS | Notes | Role |
| --- | --- | --- | --- | --- | --- |
| Seeed SenseCAP T1000-E | nRF52840 | LR1110 (SX126x-compatible) | Airoha | Credit-card tracker, 700 mAh, IP65 | tracker/client |
| RAK WisBlock solar node | nRF52840 (RAK4631) | SX1262 | optional | Solar enclosure, big battery | repeater |
| Seeed SenseCAP Solar Node P1 | nRF52840 | SX1262 | yes | Solar node, IPX antenna | repeater/gateway |
| Heltec Wireless Tracker | ESP32-S3 | SX1262 | L76K | TFT screen, WiFi/BLE | tracker/gateway/client |

See `docs/hardware/boards.md` for pin maps and `firmware/README.md` for build/flash steps.

---

## 6. Security model

* **Identity**: Ed25519 + X25519, seed stored encrypted at rest (device keystore on phones,
  NVS/flash with key derived from a device secret on MCUs).
* **Direct messages**: XChaCha20-Poly1305 with static-ECDH-derived keys, header as AAD.
* **Groups**: shared 256-bit PSK, per-message Ed25519 signature over the inner message.
* **Relays**: see ciphertext and routing metadata only; cannot forge, cannot read.
* **Threats explicitly out of scope v1**: traffic analysis resistance, forward secrecy,
  malicious-relay denial of service, key revocation (documented as OMP/2 work).
* **What a relay learns**: which pseudonymous address sent to which, when, how large, and
  how many hops. No names, content, or group membership.

---

## 7. Build, test, run

```bash
# one-time: this machine has no system gcc; zig is wired in as the C compiler/linker
./scripts/dev-env.sh          # prints/export env hints

cargo test -p openmesh-core   # protocol unit + property tests
cargo test                    # whole workspace
cargo run -p openmesh-sim     # mesh scenarios (chat, relay chain, store-forward, image)
cargo run -p openmesh-bridge -- --demo  # bridge on a simulated radio + demo traffic
cargo run -p openmesh-cli -- --help
pnpm --dir apps/web install && pnpm --dir apps/web dev   # UI at http://localhost:5173
```

---

## 8. Testing strategy

| Layer | How |
| --- | --- |
| Wire codec | Unit tests + golden vectors (`protocol/vectors`) |
| Crypto | Known-answer tests, tamper/negative tests |
| Protocol engine | Two-engine in-process tests: send → flood → ack → store-forward |
| Resources | Loss/duplicate/reorder simulation; reassembly fuzz |
| Whole mesh | `openmesh-sim` scenarios with assertions on delivery rate, airtime, convergence |
| Firmware | PlatformIO unit tests replaying the same golden vectors on-device |
| UI | Vitest unit tests + Playwright smoke test against the bridge demo mode |
| Release | CI matrix builds Rust, web, firmware (nRF52 + ESP32-S3); Android best-effort |

---

## 9. Roadmap

| Milestone | Contents |
| --- | --- |
| **M0 (this repo)** | Spec, core crate, bridge + WS API, web client, simulator, firmware for 4 boards, flashers, docs |
| M1 | Field testing on real hardware, store-forward tuning, OTA updates, channel invites |
| M2 | Android app GA, BLE transport, background sync, notification actions |
| M3 | OMP/2: session ratchets, voice calls, directed routing option |
| M4 | NomadNet-compatible layer (see §10), file transfers, LXMF bridge |
| M5 | Room servers (channel history), multi-channel radios, network map API |

---

## 10. Reticulum / NomadNet plan

Reticulum's strengths are its mature crypto (links, ratchets) and its interface abstraction;
its weakness for us is client ergonomics and MCU cost. OpenMesh will not pretend to be
Reticulum on the wire. Instead:

1. **`openmesh-gateway` (M4)**: a bridge node that speaks OMP on one side and Reticulum
   (TCP/interface) on the other, translating addresses and message envelopes where semantics
   allow (LXMF DM ↔ OMP direct message). This gives OpenMesh users access to existing
   NomadNet/Reticulum services without dragging the stack into firmware.
2. **NomadNet-compatible client layer (M4/M5)**: OpenMesh-native "pages" served by room
   servers, with a subset of NomadNet's micron markup, so we can host boards/messages that
   also render in NomadNet clients. Our client will not render the full NomadNet stack at
   first; the goal is a *replacement made for OpenMesh*, not a reimplementation.
3. Address mapping and identity bridging are documented in `docs/research/nomadnet-bridge.md`.

Rationale: the user-facing complaint about Reticulum clients is real; the right answer is a
first-class OpenMesh client with an opt-in bridge, not a half-compatible fork.

---

## 11. Risks and open questions

| Risk | Mitigation |
| --- | --- |
| LoRa airtime for images is user-hostile | Pre-send estimate, aggressive compression, size caps per profile/region, thumbnails-first |
| Managed flooding scales poorly past ~100 nodes in one region | TTL tuning, dedup, priority queueing, directed routing in OMP/2 |
| Two protocol implementations drift | Golden vectors in CI for both; C++ tests replay the JSON |
| ESP32-S3 + nRF52 support burden | Single C++ codebase with a thin HAL; board configs, not forks |
| No forward secrecy in v1 | Documented; OMP/2 ratchet; voice calls gated on it |
| Android build not verifiable on this machine | CI job with SDK; scaffold kept minimal and documented |
| Regulatory duty cycles | Region profiles + token-bucket budget in firmware and bridge |

---

## 12. Repository conventions

* `main` is protected: PRs, green CI, linear history.
* `crates/*` are libraries; only `*-bridge`, `*-cli`, `*-sim` produce binaries.
* Protocol changes require: spec edit + vectors + core tests + firmware conformance note.
* Feature flags: `openmesh-core` has no optional features in v0.1; `openmesh-bridge` uses
  `--features real-serial` for hardware transports (default is simulator + UDP).
* Commit style: `area: summary` (e.g. `core: fix resource window ack`).

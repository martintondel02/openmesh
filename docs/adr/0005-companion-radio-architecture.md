# ADR 0005 — Companion-radio architecture

* **Status:** accepted (v0.1.0)
* **Date:** 2026-11-25
* **Decision record:** summary row 5 in
  [ARCHITECTURE.md §4](../architecture/ARCHITECTURE.md#4-key-decisions)
* **Related:** [COMPANION-API.md](../architecture/COMPANION-API.md),
  [WEBSOCKET-API.md](../architecture/WEBSOCKET-API.md),
  [ADR 0004](0004-rust-core-cpp-firmware-golden-vectors.md),
  [ADR 0006](0006-web-first-clients-tauri-native-android.md),
  [ADR 0007](0007-media-pipeline.md)

## 1. Context

Every LoRa mesh needs to answer: where does the application live, and what does the radio do?

| Model | Examples | Implications |
| --- | --- | --- |
| **Fat firmware**: UI, storage and app logic on the device | consumer FRS radios, some Meshtastic standalone builds | radio must have a screen; protocol and app update together; limited processing/memory; hard to build a rich client ecosystem |
| **Dumb modem**: radio exposes raw LoRa TX/RX, all protocol in the app | LoRaWAN-style modems, RNode in raw mode | app must be always connected (or the radio does nothing useful); phone battery and OS background limits hurt |
| **Companion radio**: radio owns protocol essentials (relay, announce, store) and exposes a typed protocol to the app; app owns identity/UI/keys for its own traffic | Meshtastic, MeshCore, RNode + Reticulum host, **OpenMesh** | radios are cheap, battery-friendly and useful standalone; apps update independently; a small typed API is the contract |
| **Bridge daemon**: host owns everything, radio is a dumb serial modem | OpenMesh desktop, Reticulum `rnsd` | one implementation of storage/sync, multiple clients share a session; needs a host device |

OpenMesh targets both cheap repeaters (solar, no screen, no phone) and rich clients (web,
desktop, Android) that may not be near a computer. A pure dumb modem fails the repeater case; a
fat firmware fails the client case. The companion model with a **bridge** for desktop/Web and a
**direct companion link** for Android covers both.

## 2. Decision

**Radios run OMP/1 as full mesh nodes (client/repeater/gateway/tracker class) and expose a small
typed companion API to applications.** Applications own their identity, storage and UI; the
radio can operate standalone (relay, announce, store) when no app is attached.

### 2.1 Division of responsibility

| Responsibility | Radio (firmware) | App/bridge |
| --- | --- | --- |
| LoRa PHY, radio config, region/duty-cycle enforcement | yes | configures only |
| Frame codec, dedup, flooding/TTL, announce relay | yes | no |
| Own identity (infrastructure frames: announces, telemetry) | yes | no |
| Store-and-forward queue | yes (repeater/gateway roles) | no |
| User identity and keys for user traffic | **no** (app supplies frames / holds identity) | yes |
| Message storage, threads, contacts, attachments | no | yes (bridge/redb or app DB) |
| Media compression/playback, map, QR, notifications | no | yes |
| Companion link (USB CDC / BLE NUS / TCP) | yes (server) | yes (client) |

### 2.2 Companion API shape (normative in COMPANION-API.md)

* Framing: SLIP (`0xC0` delimiter, escaping) around a compact header
  `b"OM" | type(u8) | len(u16 LE) | payload | crc16`.
* Message types: `Hello`, `HelloAck`, `ConfigGet`, `ConfigSet`, `Config`, `Tx`, `Rx`, `StatsGet`,
  `Stats`, `Ping`, `Pong`, `Event`, `Reset`.
* `Hello`/`HelloAck` exchanges protocol version, firmware version, node id, `max_frame` (240),
  caps, region and profile. The app MUST match `proto`.
* `Tx` carries one complete OMP frame (≤ 240 B); `Rx` delivers any valid received frame plus
  RSSI/SNR/link/arrival time. Radios MAY filter frames, but must report the filter mode in caps.
* Config keys are namespaced strings (`radio.profile`, `radio.region`, `node.name`,
  `mesh.ttl`, ...), one pair per `ConfigSet`.
* A radio with no app for `STANDALONE_GRACE_MS` (60 s) enables its own relay/store/announce loop
  with its own identity.
* Companion links are **not mutually authenticated in v1**: physical access is the trust
  boundary (same as Meshtastic/RNode), with a pairing key planned for a later version.

### 2.3 Bridge API for desktop/web

On hosts, apps do not talk to the radio directly: `openmesh-bridge` owns the radio(s), the
engine, storage (redb) and attachments, and exposes a WebSocket + REST API
([WEBSOCKET-API.md](../architecture/WEBSOCKET-API.md)) with a loopback-only bearer token. This
keeps one correct implementation of storage/sync and lets multiple clients (web UI, CLI) share
one session. The Android app talks to a radio directly over USB/BLE; if it talks to a bridge, it
uses the same WS API.

### 2.4 Why the app owns identity for user traffic

Two options were possible: the radio holds the user's identity and signs everything (like a
phone's secure element), or the app holds it and gives the radio finished frames. The second was
chosen because:

* the identity must survive radio replacement/reflash; the app's keystore is a better home;
* the bridge can then decrypt/encrypt and store messages, which it needs for threads and
  attachments anyway;
* a radio can be a shared repeater without holding anyone's keys;
* it keeps the radio's attack surface small (no key storage, no message content).

The cost: while the app is away, the radio cannot *originate* user messages, only relay/store
them. That is acceptable: messages composed offline are queued by the app/bridge and sent when
the link returns; the radio's job in the interim is the mesh.

## 3. Consequences

### 3.1 Positive

* **Cheap repeaters.** A solar nRF52840 node can relay/announce/store with no screen, no phone,
  no keys of its own. This is the deployment that makes the mesh useful on a ridge.
* **Apps update independently of firmware.** UI, media codecs, maps, notifications and
  encryption changes can ship without reflashing radios (a huge practical advantage on
  nRF52/ESP32 field devices).
* **One protocol core, multiple front-ends.** Desktop bridge, CLI, simulator and (later) mobile
  FFI all use `openmesh-core`; radios implement the same wire format in C++ (ADR 0004).
* **Battery and OS friendliness.** A phone app can sleep; the radio keeps the mesh alive; a
  bridge can run on a Raspberry Pi/home server 24/7.
* **Typed API instead of raw modem.** `Tx`/`Rx` with a framed, CRC-checked, CBOR-typed envelope is
  much easier to build clients against than a raw serial stream (contrast MeshCore's byte
  commands, which are workable but fragile).
* **Testability.** The bridge + simulator can run the whole stack with no hardware; the
  companion codec has golden vectors.

### 3.2 Negative

* **Companion link is a single point of failure for mobile.** USB/BLE flakiness, Android
  permission and foreground-service constraints, BLE MTU fragmentation, and no authentication in
  v1. Apps must handle reconnects and frame loss.
* **Two storage implementations.** The bridge stores messages; the app stores its own copy (or
  relies on the bridge). Sync semantics between phone and bridge are non-trivial and not fully
  solved in v0.1.
* **No user messages while detached.** A standalone radio cannot receive a DM *for the user*
  and store it for the app unless it has the user's keys — so a repeater without the phone
  cannot act as the user's inbox. (It can store the *ciphertext* as a relay for later retrieval;
  that is precisely the store-and-forward design.)
* **More moving parts.** Radio firmware + companion protocol + bridge + WS API + app transports;
  each interface is a test surface.
* **Companion throughput.** A 12 KiB image is ~69 chunks of 213 bytes; over BLE at 20–100 KB/s
  this is fine, but over a slow serial link (115200 baud ≈ 11 KB/s) it is seconds; not a
  bottleneck for our caps, but the framing/ack design must not stall on it.
* **Security boundary is physical access.** A malicious local process can open the serial/BLE
  device and send frames as the app; documented, pairing key planned.

### 3.3 Mitigations

* Framing with CRC, length and version; reject oversized `Tx` frames with an explicit `Event`.
* Reconnect logic with backoff in clients (documented in the Android README and web client).
* Bridge token auth + loopback-only binding; explicit `--bind` escape hatch documented as
  unsafe.
* Standalone grace period (60 s) so a radio does not flap between app-controlled and standalone
  modes.
* Golden vectors for the companion codec; a fake-radio test harness in the bridge/simulator for
  CI without hardware.
* Pairing key (M2) in `Hello`/`HelloAck`, with a physical button press to authorise.

## 4. Alternatives considered

| Alternative | Why rejected |
| --- | --- |
| Fat firmware (screen + full client on radio) | Only works on T-Deck-class boards; excludes cheap solar repeaters; firmware update burden for UI/media; our four-board matrix includes headless nRF52 nodes. |
| Dumb modem (all protocol in the app) | Radio useless standalone, phone must be connected for any mesh function; background/battery constraints; store-and-forward impossible without a host. |
| Radio holds user identity and full message store | Key management on cheap MCUs is risky; messages exposed to radio firmware bugs; identity lost on reflash; repeater would hold private keys. |
| Single bridge-only architecture (no companion) | Desktop/web fine, but Android needs a host or a cable to a PC; no phone-only field use. Meshtastic/MeshCore demonstrate the demand for phone+radio directly. |
| Raw RNode-style modem protocol | Maximum flexibility, but every client re-implements framing, dedup, store; our typed companion API is a product feature. |
| **Companion radio + bridge for hosts (chosen)** | Covers repeaters, phone-direct and desktop/web; keeps radios cheap and apps updatable; testable with a simulator. |

## 5. Follow-ups

1. **M1:** companion link throughput/latency benchmark over USB CDC and BLE NUS with 240-byte
   frames, and a frame-loss/reconnect test on Android.
2. **M2:** pairing key and authenticated companion handshake; document the threat model change.
3. **M2:** background sync on Android (foreground service + notification actions) and a
   "radio connected/disconnected" UX.
4. **M2/M4:** bridge ↔ app sync model for users who use both (message ids, delivery state,
   attachment cache); decide whether the phone mirrors the bridge store or keeps its own.
5. **M5:** multi-radio support in the bridge (one LoRa + one Wi-Fi gateway) and a
   `hello.radios[]` capability extension, as WEBSOCKET-API already anticipates.

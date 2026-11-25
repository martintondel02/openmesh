//! Companion API has moved into the core crate for the Rust side. This document describes the
//! normative semantics shared with firmware and mobile clients.

# OpenMesh Companion API (radio ⇄ app)

**Status:** normative for v0.1.0 · **Transport:** USB CDC, BLE NUS, TCP (optional)

The companion API is how an app talks to a local radio when the app itself does not implement
LoRa. On desktop, apps normally talk to `openmesh-bridge` instead; Android talks directly to a
radio over USB or BLE. A radio behaves identically with no app attached (standalone repeater
mode): it keeps announcing, relaying and storing envelopes.

## 1. Framing

Stream transports use SLIP-style framing (the same framing is used for a raw OMP frame stream so
one serial port can carry both):

```
0xC0            frame delimiter (start and end)
0xDB 0xDC       escaped 0xC0 inside the payload
0xDB 0xDD       escaped 0xDB inside the payload
```

Inside the delimiter:

```
magic(2) = b"OM" | type(u8) | len(u16 LE) | payload(len) | crc16/payload-crc
```

* `len` is the payload length, max 1024.
* CRC-16/CCITT-FALSE (polynomial 0x1021, initial value 0xFFFF, no reflection, no final XOR)
  computed over `type || len || payload` and transmitted as a **little-endian u16** immediately
  after the payload. Frames with a bad CRC are dropped and counted. Reference check value:
  CRC-16/CCITT-FALSE of ASCII `"123456789"` is `0x29B1`.
* BLE uses the same framing; the BLE characteristic is a Nordic-UART-like service with RX/TX
  characteristics and notifications. MTU fragments are reassembled by the framing layer.

## 2. Message types

| Value | Direction | Type | Payload (CBOR) |
| --- | --- | --- | --- |
| 1 | app → radio | `Hello` | `{proto: u8, app_version: str, wants_events: bool}` |
| 2 | radio → app | `HelloAck` | `{proto: u8, fw_version: str, node_id: bytes(8), max_frame: u16, caps: u16, region: str, profile: str}` |
| 3 | app → radio | `ConfigGet` | `{}` |
| 4 | app → radio | `ConfigSet` | `{key, value}` (one pair; keys below) |
| 5 | radio → app | `Config` | `{entries: [[key, value], ...]}` |
| 6 | app → radio | `Tx` | `{frame: bytes}` — one whole OMP frame (≤ 240 B) |
| 7 | radio → app | `Rx` | `{frame: bytes, rssi: i16, snr: i16, link: str, at_ms: u64}` |
| 8 | app → radio | `StatsGet` | `{}` |
| 9 | radio → app | `Stats` | `{entries: [[key, u64], ...]}` |
| 10 | app → radio | `Ping` | `{nonce: u32}` |
| 11 | radio → app | `Pong` | `{nonce: u32}` |
| 12 | radio → app | `Event` | `{kind: str, detail: str}` (`boot`, `radio_error`, `announce_tx`, `store_push`, `store_deliver`, `low_battery`, `gps_fix`) |
| 13 | app → radio | `Reset` | `{}` — reboot the radio after an OTA write |

## 3. Handshake

```
app  → Hello{proto:1, app_version:"openmesh-android/0.1.0", wants_events:true}
radio→ HelloAck{proto:1, fw_version:"0.1.0", node_id:..., max_frame:240, caps:..., region:"EU868", profile:"MEDIUM_FAST"}
```

The app MUST match `proto`. After `HelloAck` the radio pushes `Rx` frames as they arrive and
accepts `Tx` frames. The radio MUST reject `Tx` frames larger than `max_frame` with an `Event`
`{kind:"error", detail:"frame_too_large"}`.

## 4. Configuration keys

| Key | Values | Notes |
| --- | --- | --- |
| `node.name` | string ≤ 32 | announce display name |
| `node.class` | `client`, `repeater`, `gateway`, `tracker`, `room_server` | reboot recommended |
| `radio.region` | `EU868`, `US915`, `AU915`, `IN865`, `KR920`, `AS923` | reboot required |
| `radio.profile` | `MEDIUM_FAST`, `LONG_FAST`, `LONG_SLOW`, `SHORT_FAST`, `SHORT_TURBO` | |
| `radio.tx_power` | `-9..22` dBm | clamped to region max |
| `radio.frequency` | kHz, override for test networks | advanced |
| `mesh.ttl` | 1..15 | default 5 |
| `mesh.store` | `0`/`1` | store-and-forward on relays |
| `mesh.store_capacity` | envelopes | default 64 |
| `mesh.announce_interval` | seconds | default 1800 / 3600 |
| `mesh.position_sharing` | `0`/`1` | GPS trackers only |
| `gps.enabled` | `0`/`1` | |

## 5. Radio behaviour requirements

* The radio keeps its own monotonic `seq` counter which MUST be used for all frames it
  originates (announces, telemetry). Frames received from the app carry the app's `seq` and are
  forwarded unchanged — the app is responsible for sequencing its own identity's frames
  (the bridge owns the identity when a phone is attached; the radio uses its own identity only
  for infrastructure frames).
* `Tx` frames are transmitted on the radio with TTL unchanged; dedup applies as for any frame.
* `Rx` frames are emitted for every valid frame received (including ones addressed to others),
  so the app can do its own protocol processing. Radios MAY filter frames the app cannot need
  (e.g. announces already relayed) to save BLE bandwidth; when they do, they MUST signal the
  filtering mode in `HelloAck.caps`.
* Standalone mode: if no app is connected for `STANDALONE_GRACE_MS` (60 s), the radio enables its
  own relay/store/announce loop with its own identity.

## 6. Security notes

* USB/BLE companion links are **not** mutually authenticated in v1: any local process can open
  the serial port. Physical access is the trust boundary (same as Meshtastic/RNode). A future
  version will add a pairing key in `Hello`/`HelloAck`.
* Apps MUST NOT expose the serial/BLE device to untrusted users.

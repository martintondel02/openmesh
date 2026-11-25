# OpenMesh firmware

One C++17 codebase for all four reference boards: Seeed SenseCAP T1000-E (nRF52840 + LR1110),
RAK WisBlock 4631 in the solar enclosure (nRF52840 + SX1262), Seeed SenseCAP Solar Node P1
(nRF52840 + SX1262 + GNSS) and Heltec Wireless Tracker (ESP32-S3 + SX1262 + GNSS + TFT).

The protocol core (`lib/openmesh/`) is **Arduino-free** and mirrors the normative OMP/1 spec in
`docs/architecture/PROTOCOL.md`. All hardware access (radio, flash, USB/BLE, ADC, GNSS UART, RNG,
sleep) is injected through small HAL interfaces, so the same code runs on a host for tests.

```
firmware/
├── platformio.ini              envs: t1000e, rak4631_solar, seeed_solar, heltec_tracker, native
├── boards/                     pin maps + role defaults (own code; sources cited in NOTICE)
│   ├── board.h / board_select.h
│   └── t1000e.cpp  rak4631_solar.cpp  seeed_solar.cpp  heltec_tracker.cpp
├── lib/openmesh/               portable protocol library (no Arduino includes)
│   ├── include/openmesh/       om_types, om_cbor, om_sha256, om_identity, om_messages,
│   │                           om_dedup, om_region, om_store, om_companion, om_gps,
│   │                           om_power, om_engine
│   ├── src/                    implementations
│   └── third_party/monocypher/ Monocypher 4.0.2 (BSD-2/CC0) + RFC 8032 ed25519 module
├── src/
│   ├── main.cpp                radio loop, companion link, GPS/battery, announce scheduler
│   └── hal/                    RadioLib, LittleFS/InternalFileSystem, USB CDC + BLE NUS, ADC
├── test/test_protocol/         host unit + integration tests and golden-vector replay
├── ci/host_tests.sh            compiler-light host test runner (zig or gcc)
├── ci/build.yml                GitHub Actions matrix (copy to .github/workflows/firmware.yml)
├── NOTICE                      third-party licences and pin-map sources
└── README.md                   this file
```

---

## 1. Build and flash matrix

| Environment | Board | Flash | Upload |
| --- | --- | --- | --- |
| `t1000e` | Seeed SenseCAP T1000-E | 1 MiB internal, UF2 bootloader | UF2 copy / `nrfutil` |
| `rak4631_solar` | RAK WisBlock 4631 (solar enclosure) | 1 MiB internal, UF2 bootloader | UF2 copy / `nrfutil` |
| `seeed_solar` | Seeed SenseCAP Solar Node P1 | 2 MiB QSPI, UF2 bootloader | UF2 copy / `nrfutil` |
| `heltec_tracker` | Heltec Wireless Tracker V1.1 | 8 MiB, Espressif bootloader | esptool / WebSerial |
| `native` | host (no hardware) | — | `pio test -e native` |

```bash
python3 -m pip install -U platformio
cd firmware
pio run -e t1000e                    # build (also rak4631_solar, seeed_solar, heltec_tracker)
pio run -e heltec_tracker -t upload  # esptool over USB
pio test -e native                   # host protocol tests

# hardware-free fallback if PlatformIO's native compiler is unavailable:
./ci/host_tests.sh
```

**Flashing end users:** use the web flasher (`apps/flasher/`) or the Linux CLI
(`tools/flash/openmesh-flash`). Board-by-board instructions, pins and power numbers are in
`docs/hardware/boards.md`.

The three nRF52 environments use the Meshtastic `platform-nordicnrf52` fork only for board
definitions (`tracker-t1000-e`, `wiscore_rak4631`, `seeed_solar_node`) and the Adafruit nRF52
Arduino core; no Meshtastic application source is used. Pin maps are our own files, derived from
public hardware documentation and cited in `NOTICE`.

> **Build verification status (honest):** the portable protocol library, the host test suite and
> the golden-vector replay are compiled and executed locally (see §2). The four hardware
> environments were **not compiled on this machine**: `pio run -e t1000e` failed while cloning the
> pinned nRF52 platform fork (`Recv failure: Connection reset by peer` from github.com) before any
> toolchain download. The CI workflow in `ci/build.yml` builds the full matrix; exact local
> commands are above. Hardware bring-up (radio init, GNSS baud/ADC calibration, sleep current) is
> unverified until the first board is flashed — see §9.

---

## 2. Verification performed

Host tests exercise the frozen wire format (address-class bit, binary announce, TTL-independent
AAD, `msg_ref`, 34-byte resource AAD, companion CRC/SLIP) plus crypto known-answer vectors,
dedup/store, engine relay/store-forward and companion framing:

```
$ ./ci/host_tests.sh
==> compiler: zig c++
==> running firmware protocol tests
...
44 test case(s), 3421 assertion(s), 0 failure(s)
RESULT: PASS
```

The same suite runs under PlatformIO's native environment (verified locally by putting zig
`gcc`/`g++`/`ar` shims on `PATH`; on a machine with gcc it works directly):

```
$ pio test -e native
44 test cases: 44 succeeded
```

Golden vectors in `protocol/vectors/*.json` are replayed as part of the same run. All seven files
(`frame`, `identity`, `announce`, `seal`, `app_message`, `resource`, `companion`) pass
byte-for-byte: frame encode/decode/AAD (including the clear-text `StoreOffer`/`StoreWant` case),
seed→identity/channel/DM-key derivation, fixed-message Ed25519 signatures, opening the
fixed-nonce seal vectors in both DM directions and on a channel, complete sealed-body layouts
(identity hint included), binary announce body/signature/TTL-derived hops, canonical
AppMessage/Ack CBOR, resource blob/manifest/chunk/AAD-34 layout and ACK bitmaps, and companion
payload/chunk/CRC/SLIP encoding. The vector replay adds ~950 assertions on top of the unit tests.
If a file is missing (partial checkout) the loader prints a visible `SKIP` and the suite still
passes.

---

## 3. Roles and behaviour

| Role | Selected by | Behaviour |
| --- | --- | --- |
| `tracker` | board default on T1000-E / Heltec, or `node.class=tracker` | Announces every 30 min ±10 %, shares GNSS position every 5 min (channel, falling back to a direct message to a known gateway/repeater because a signed `GroupMessage` usually exceeds one frame), telemetry every 15 min, low-battery policy, USB/BLE companion. |
| `repeater` | board default on RAK solar / Seeed solar | Announces every 60 min, floods every floodable frame with TTL−1, retains encrypted `Data`/`Resource` envelopes for unicast destinations (72 h / 48 envelopes), offer-want replay, no position sharing. |
| `gateway` | `node.class=gateway` | Repeater plus telemetry beacons. WiFi/Ethernet bridging is v0.2+; the ESP32-S3 hardware is present. |
| `client` | companion `ConfigSet` | Radio as a modem: app owns messaging; radio still relays/announces in standalone mode. |

A radio with no app attached keeps announcing, relaying and storing (standalone repeater mode).
Role, region, profile, TTL, store toggle and position sharing are settable at runtime through the
companion API; region changes need a reboot for the radio to retune.

---

## 4. Protocol library notes

* **Frame codec exactly per PROTOCOL.md §4**: 240-byte MTU, 22-byte header,
  `version(3) | type(5)` and `ttl(4) | flags(4)`, big-endian `seq`. Bit 3 of the flag nibble is
  reserved and rejected on decode. Unknown frame types (11–31) are ignored, not errors.
* **AAD excludes the TTL nibble** (byte 1 high nibble zeroed) on sender and receiver, so relays
  can decrement TTL without breaking AEAD. Relays copy body and flags unchanged.
* **Sealed body** = `payload_flags(1) || msg_ref(4 BE) || [identity(72)] || nonce_prefix(12) ||
  ct || tag(16)`. `payload_flags` bit 0 is set whenever the 72-byte identity hint is included.
* **Announce is a fixed binary body** (`152 + name_len` bytes, not CBOR), signed over
  `"openmesh/v1/announce" || src(8) || seq(4 BE) || body_without_signature`. Offset 4 is reserved
  zero; hop count is derived as `8 − frame.ttl`. Relays never modify signed bytes.
* **Address class**: unicast node ids clear bit 7 of byte 0, channel ids set it. Relays store only
  unicast envelopes, never channel traffic.
* **Envelopes**: X25519 static DH + HKDF-SHA256 direct keys and per-channel HKDF keys;
  XChaCha20-Poly1305 with a 12-byte random nonce prefix and 12 zero bytes.
* **Resources**: sealed blob = `0x00 || nonce(12) || ciphertext || tag(16)`; manifest `sha256`
  covers the whole sealed blob; AAD is
  `"openmesh/v1/resource" || res_id || msg_id || total_len || chunk_bytes` (34 bytes).
  Chunk body = `flags(0) || msg_ref || res_id || chunk_index || slice`, `chunk_bytes = 180`.
* **Store-and-forward**: relays retain original frame bytes and replay with `ttl = default_ttl`
  (legal because TTL is outside the AAD); offer/want bodies are clear-text CBOR because a relay
  has no key material for the addressee. See "spec gaps" in §9.
* **Dedup**: fixed 2048-entry ring `(src, seq)` with 30 min TTL, no heap; app-layer
  `(src, msg_ref)` replay cache for store replays.
* **Duty cycle**: token bucket over transmitted airtime, 1 % for EU868/AS923, unlimited where the
  region only has fair-use rules; LoRa airtime computed with the Semtech formula.

### Why a hand-rolled CBOR codec?

The payload schemas are a dozen small structs. A full CBOR library would add build surface and
heap use to every target; the codec in `om_cbor.cpp` is ~350 lines, allocation-free, emits the
same canonical form `serde`/`ciborium` produces (definite lengths, declaration-order keys,
minimal integers, omitted `None`s) and tolerates unknown keys on read as the spec requires.
`test_protocol.cpp` pins the byte layout and the vector replay re-encodes Rust-produced CBOR to
prove byte compatibility.

### Why Monocypher + a separate SHA-256?

Monocypher is small, auditable and dual-licensed BSD-2/CC0. **Important:** Monocypher's default
EdDSA uses BLAKE2b and is *not* wire-compatible with `ed25519-dalek`. The firmware therefore uses
Monocypher's optional `monocypher-ed25519` module (RFC 8032, SHA-512). SHA-256/HMAC/HKDF are
implemented in `om_sha256.c` because Monocypher does not ship SHA-256 and OMP/1 uses it for node
ids, channel ids, resource hashes and HKDF. Known-answer tests cover FIPS 180-4, RFC 4231,
RFC 5869 and RFC 8032.

### Key storage

The 32-byte master seed is generated from the hardware RNG on first boot and stored as
`XChaCha20-Poly1305(seed)` under a key derived from the MCU unique id (nRF52 `FICR.DEVICEID`,
ESP32 eFuse MAC). This protects against casual flash extraction; it is **not** protection against
an attacker with the physical device. A future version should use the nRF52 CryptoCell / ESP32
digital-signature peripheral or an external secure element.

---

## 5. Companion link (USB / BLE)

Both transports carry the same framing (`docs/architecture/COMPANION-API.md`): SLIP delimiters
(`0xC0`, `0xDB 0xDC`, `0xDB 0xDD`) around
`"OM" | type(u8) | len(u16 LE) | payload | crc16` (CRC-16/CCITT-FALSE over type+len+payload).
BLE uses Nordic-UART-like service characteristics and reassembles MTU fragments; the radio
accepts the same chunks over USB CDC and BLE.

Quick start with a serial terminal (or the bridge/web client):

```
app → radio  Hello{proto:1, app_version:"...", wants_events:true}
radio → app  HelloAck{proto:1, fw_version:"0.1.0", node_id:(8), max_frame:240,
                      caps:(u16), region:"EU868", profile:"MEDIUM_FAST"}
radio → app  Event{kind:"boot", detail:"0.1.0"}
radio → app  Rx{frame:(bytes), rssi:i16, snr:i16, link:"lora", at_ms:u64}
app → radio  Tx{frame:(one whole OMP frame ≤240 B)}
```

Configuration keys accepted through `ConfigSet` (one key/value per message):

| Key | Values | Notes |
| --- | --- | --- |
| `node.name` | string ≤32 | announce display name |
| `node.class` | `client` / `repeater` / `gateway` / `tracker` / `room_server` | reboot recommended |
| `radio.region` | `EU868`, `US915`, `AU915`, `IN865`, `KR920`, `AS923` | reboot required |
| `radio.profile` | `MEDIUM_FAST`, `LONG_FAST`, `LONG_SLOW`, `SHORT_FAST`, `SHORT_TURBO` | live |
| `radio.tx_power` | `-9..22` dBm | clamped to the region maximum |
| `radio.frequency` | kHz (advanced) | test networks only |
| `mesh.ttl` | `1..15` | default 5 |
| `mesh.store` | `0`/`1` | store-and-forward on relays |
| `mesh.store_capacity` | envelopes | compile-time capacity in v0.1 |
| `mesh.announce_interval` | seconds ≥300 | default 1800/3600 |
| `mesh.position_sharing` | `0`/`1` | GNSS boards only |
| `gps.enabled` | `0`/`1` | GNSS boards only |

The default channel (`OM_DEFAULT_CHANNEL_NAME`, default `openmesh-public`) is derived from a
well-known string and is **public by design**; it makes out-of-the-box mesh testing work. Add a
private channel PSK through the app for real traffic.

---

## 6. Power

Power budgets, solar sizing, antenna and enclosure guidance live in
`docs/hardware/boards.md` §4–5. Summary for v0.1 (receiver left on, estimates):

| Role | Average current | Notes |
| --- | --- | --- |
| Repeater (RAK / Seeed solar) | ~13–15 mA ⇒ 310–360 mAh/day | dominated by RX; TX duty ≤0.03 % |
| Tracker (T1000-E, GNSS duty) | ~13–16 mA | 700 mAh ≈ 2 days; deep sleep is M1 work |
| Tracker (Heltec, screen + ESP32) | ~75–95 mA | USB or large pack |

`hal::sleepMs()` uses RTC/WFE light sleep on nRF52 and ESP32 light sleep; **deep sleep between
tracker fixes is not implemented yet** (see §9). Repeaters are meant to be solar-powered.

---

## 7. Pin maps

Pin definitions are in `firmware/boards/<board>.cpp`, with a human-readable table and per-board
sources in `docs/hardware/boards.md` §2 and `NOTICE`. Board defaults (role, region, profile,
capabilities) live in the same files. If you port a new board: copy one board file, adjust pins,
select it in `platformio.ini`, add the board to `apps/flasher/boards.json` and `tools/flash`.

---

## 8. CI

`ci/build.yml` (copy to `.github/workflows/firmware.yml`) runs:

* `host-tests`: `ci/host_tests.sh` plus `pio test -e native` on Ubuntu;
* `build`: a PlatformIO matrix for the four board environments, uploading `.uf2`/`.bin`/`.hex`
  artifacts;
* release jobs can attach the artifacts as `openmesh-<board>.uf2` and
  `openmesh-heltec_tracker-full.bin`, which is exactly what `apps/flasher/boards.json` and
  `tools/flash/openmesh-flash` consume.

---


> **Verification status:** the portable protocol core is validated on host against all golden
> vectors (44 cases / 3 421 assertions) and the native PlatformIO env. The four hardware-target
> PlatformIO builds need first-article board bring-up (`firmware/boards/pio/`,
> `firmware/variants/`, SoftDevice linker script) and are marked best-effort in CI; see §9.
## 9. Known issues, limitations and spec gaps

These are tracked for v0.1.x/M1 and are worth reading before trusting the firmware in the field.

1. **Hardware builds unverified locally.** The host library and tests are compiled; the four
   Arduino/PlatformIO environments were not compiled on the development machine (toolchain
   download). CI builds them. Radio bring-up, GNSS bauds, ADC calibration and sleep current need
   first-article measurement.
2. **Deep sleep missing for trackers.** v0.1 keeps the receiver on continuously and uses light
   sleep only; battery life is therefore days, not months. The power table reflects that. The
   scheduler and `PowerPolicy` already emit low-power hints.
3. **Battery/ADC calibration.** OCV tables come from vendor variants; divider multipliers and the
   3.0 V/3.3 V ADC references differ per board and need bench calibration (±5–10 % likely).
4. **Spec gap — clear-text StoreOffer/StoreWant.** PROTOCOL.md §4.1 says clear bodies are only
   valid for announce/probe/probe-response, but §10 requires a relay with no key material for the
   addressee to send `StoreOffer`/`StoreWant`. The firmware sends them as clear-text CBOR and
   relays them like any other floodable frame. This needs a spec decision (e.g. an explicit
   exception) before v1.0.
5. **Spec arithmetic — resource AAD length.** PROTOCOL.md §9.1 says "(25 bytes)"; the formula
   `"openmesh/v1/resource" || res_id || msg_id || total_len || chunk_bytes` yields **34 bytes**
   (20-byte domain). The implementation and tests use 34; core-protocol confirmed this is the
   intended value.
6. **Spec ambiguity — telemetry in AppMessage.** `services.rs` has `MessageKind::Telemetry` but no
   `AppMessage.telemetry` field. The firmware encodes telemetry as an optional `telemetry` CGOR
   key (tolerated by serde) and documents it as provisional until the Rust schema settles.
7. **Positions to a channel rarely fit one frame.** A signed `GroupMessage` adds 100 bytes of
   overhead, so a position usually exceeds 218 bytes. v0.1 falls back to a direct AEAD message to
   a known gateway/repeater (which fits) instead of implementing chunked `long_text` resources for
   positions. Proper resource fallback is M1.
8. **Firmware does not reassemble resources.** `Resource` frames are forwarded and surfaced to
   the companion app (which owns reassembly/ARQ); the radio never decrypts chunks, matching the
   spec's relay model.
9. **Wall-clock timestamps** are boot-relative until the platform provides a real time source;
   GNSS date/time parsing is not wired into `setWallTime` yet. Companion/bridge-supplied
   timestamps are unaffected.
10. **BLE NUS API version.** nRF52 uses the Adafruit Bluefruit `BLEUart`; ESP32-S3 pins
    NimBLE-Arduino **1.4.3** (the 1.x callback API). Bumping NimBLE to 2.x requires adapting the
    server/characteristic callbacks in `src/hal/hal_companion.cpp`.
11. **Store queue flash wear.** The queue is persisted every 5 minutes when dirty (13.4 KiB
    rewrite). Repeaters with high envelope turnover may rewrite frequently; long-term this wants
    an append-only log with wear levelling. v0.1 documents the trade-off rather than solving it.
12. **Vectors.** All published golden vectors pass in the host suite (7/7 files, including
    `seal.json` opened in both DM directions, channel sealing and a tampered-ciphertext reject;
    production sealing is covered by unit round-trips because fixed nonce prefixes are
    test-only). `vectors_are_current` in the Rust crate guards drift.

---

## 10. Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| Build fails resolving boards | Meshtastic platform fork not reachable | Check network/proxy; `pio pkg update`; see `platformio.ini` comments |
| `pio test -e native` says no compiler | No system gcc/g++ | Use `./ci/host_tests.sh` (zig fallback) or install a host compiler |
| No serial output | USB CDC not enumerated; monitor speed | `pio device monitor -b 115200`; try the bootloader/USB-JTAG port |
| Radio init error (`radio_error init`) | SPI pins/TCXO/RF-switch mismatch, missing power enable | Verify board build flag and `boards/<board>.cpp`; RAK4631 needs P1.05 high |
| Announce never heard | No antenna, region mismatch, channel PSK mismatch | Attach antenna; `radio.region` via companion; check `Stats` |
| Repeater drops frames | EU868/AS923 1 % duty budget exhausted | Expected under load; inspect `Stats.airtime_ms`/`budget_drops` |
| GPS no fix | GNSS rail off, wrong baud, indoor | Check `gps.enabled`, board GNSS pins, move outdoors |
| Companion Hello gets no reply | Wrong baud (USB CDC ignores baud but monitors), CRC/SLIP mismatch | Use the bridge/web client; verify CRC over type+len+payload |
| Store replay not delivered | Addressee unknown to relay or offer cadence not elapsed | Wait ≥5 min or trigger a fresh announce |

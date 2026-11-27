# OpenMesh firmware

**Firmware version:** 0.7.1 · legal-maximum region table, board PA caps and the warning-gated
regulatory override (see §3.1).

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

| Environment | Board | Radio / PA cap | Flash | Upload |
| --- | --- | --- | --- | --- |
| `t1000e` | Seeed SenseCAP T1000-E | LR1110 / **15 dBm** | 1 MiB internal, UF2 bootloader | UF2 copy / `nrfutil` |
| `rak4631_solar` | RAK WisBlock 4631 (solar enclosure) | SX1262 / **22 dBm** | 1 MiB internal, UF2 bootloader | UF2 copy / `nrfutil` |
| `seeed_solar` | Seeed SenseCAP Solar Node P1 | SX1262 / **22 dBm** | 2 MiB QSPI, UF2 bootloader | UF2 copy / `nrfutil` |
| `heltec_tracker` | Heltec Wireless Tracker V1.1 | SX1262 / **22 dBm** | 8 MiB, Espressif bootloader | esptool / WebSerial |
| `native` | host (no hardware) | — | — | `pio test -e native` |

The PA cap is absolute: effective TX power is `min(region legal max, board PA cap)`, and the
regulatory override can never exceed it. The single source of truth is `boards/board.h`
(`kPaCaps[]`).

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
51 test case(s), 3639 assertion(s), 0 failure(s)
RESULT: PASS
```

The same suite runs under PlatformIO's native environment (verified locally by putting zig
`gcc`/`g++`/`ar` shims on `PATH`; on a machine with gcc it works directly):

```
$ pio test -e native
51 test cases: 51 succeeded
```

Golden vectors in `protocol/vectors/*.json` are replayed as part of the same run. All seven files
(`frame`, `identity`, `announce`, `seal`, `app_message`, `resource`, `companion`) pass
byte-for-byte: frame encode/decode/AAD (including the clear-text `StoreOffer`/`StoreWant` case),
seed→identity/channel/DM-key derivation, fixed-message Ed25519 signatures, opening the
fixed-nonce seal vectors in both DM directions and on a channel, complete sealed-body layouts
(identity hint included), binary announce body/signature/TTL-derived hops, canonical
AppMessage/Ack CBOR, resource blob/manifest/chunk/AAD-34 layout and ACK bitmaps, and companion
payload/chunk/CRC/SLIP encoding. The vector replay adds ~950 assertions on top of the unit tests; seven additional v0.5.0 cases
cover the region table, profile bandwidth validation, board PA caps, effective power policy, duty
override capacity/blocking, frequency validation and override persistence.
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
Role, region, profile, TX power, custom frequency, duty override, TTL, store toggle and position
sharing are settable at runtime through the companion API. Region/profile/power/frequency changes
are applied to the radio without a reboot (`hal::Radio::reconfigure`); a profile whose bandwidth
exceeds the new region's legal maximum is rejected (or falls back to `LONG_FAST` on EU433 /
`MEDIUM_FAST` elsewhere when a region change invalidates the current profile).


## 3.1 Legal maxima, board PA caps and the regulatory override (v0.5.0)

> **Not legal advice.** The values below are our best reading of CEPT ERC/REC 70-03 Annex 1, the
> Norwegian Nkom/EEA framework and the Meshtastic region table, cross-checked 2026-11. National
> law changes and antenna gain change effective radiated power. **Verify your local rules** — the
> power values are EIRP ceilings and a high-gain antenna can push you over the limit.

Region table v2 (mirrored byte-for-byte in `lib/openmesh/src/om_region.cpp` from
`crates/openmesh-core::util::REGIONS`; host test `test_region_table_v2` pins the numbers):

| Region | Centre (kHz) | Max power (dBm EIRP) | Duty cycle | Max BW (kHz) |
| --- | --- | --- | --- | --- |
| `EU868` (incl. Norway) | 869 525 | 27 | 10 % | 250 |
| `EU433` (new in v0.5.0) | 433 920 | 10 | 10 % | 250 |
| `US915` | 915 000 | 30 | none (fair use) | 500 |
| `AU915` | 915 000 | 30 | none (fair use) | 500 |
| `IN865` | 865 000 | 30 | none | 250 |
| `KR920` | 920 000 | 23 | none | 250 |
| `AS923` | 923 200 | 20 | 10 % | 250 |

**Defaults are the legal maxima.** On boot the firmware sets
`tx_power_dbm = min(region.max_power_dbm, board.max_tx_power_dbm)` and configures the duty-cycle
token bucket from the region. Board PA caps are hard limits (never exceeded, even with the
override):

| Board | Radio | PA cap |
| --- | --- | --- |
| Seeed T1000-E | LR1110 | 15 dBm |
| RAK4631 solar | SX1262 | 22 dBm |
| Seeed Solar P1 | SX1262 | 22 dBm |
| Heltec Wireless Tracker | SX1262 | 22 dBm |

**Profile validation:** a preset whose `bw_khz` exceeds `region.bw_khz` is rejected by
`ConfigSet radio.profile` (clients get an `Event{kind:"error", detail:"profile_bw_exceeds_region_bw"}`);
`SHORT_TURBO` (500 kHz) is therefore only selectable in US915/AU915. A region change that
invalidates the current profile falls back to `LONG_FAST` on EU433 and `MEDIUM_FAST` elsewhere.

**Override (advanced / emergency use only).** Three companion `ConfigSet` keys expose the
regulatory override fields from `docs/design/regulatory-and-regions.md` §2.3:

| Key | Meaning |
| --- | --- |
| `radio.duty_override` | `-1` legal region duty (default) or `0..100` %; `0` means "no transmissions". Re-configures the token bucket live. |
| `radio.frequency` | custom centre in kHz (`0` clears). Validated against the radio range 150 000–960 000 kHz; out-of-range is rejected. |
| `radio.tx_power` | requested power; clamped to `min(legal region max, board PA cap)` unless another override field is active, in which case it may rise to the **board PA cap only**. |

When an override exceeds the legal defaults (`overrideActive()`), the firmware:

* logs the full warning over the serial console once per activation,
* emits a companion `Event{kind:"error", detail:"regulatory_override_active"}`,
* reports `override_active=1`, `duty_pct`, `tx_power_dbm` and `frequency_khz` in the companion
  `Stats` reply (no wire/telemetry schema change — golden vectors are unchanged).

The warning string surfaced on every path is the contract text verbatim:

> Regulatory override active — transmissions may violate local radio regulations and void your
> radio's certification. Use only in a life-threatening emergency where local law permits. You
> are responsible for compliance and for any interference or damage.

Override settings (duty, power request, custom frequency, together with region/profile/mesh
settings) are persisted to `om_cfg.bin` on an accepted `ConfigSet` and restored on the next boot;
a corrupt or missing file falls back to board defaults. Enabling an override from the firmware
companion API does **not** require the typed acknowledgement — the bridge/CLI/UI own that gated UX
per the contract; the firmware companion link is a local, physical-access trust boundary (see
`docs/architecture/COMPANION-API.md` §6). Treat the CLI/web surfaces as the acknowledgement gate
and do not expose the raw serial/BLE link to untrusted users.

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
* **Duty cycle**: token bucket over transmitted airtime using the v0.5.0 legal maxima (10 % for
  EU868/EU433/AS923, unlimited where the region only has fair-use rules), with an optional
  override (`radio.duty_override`); LoRa airtime computed with the Semtech formula.

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
radio → app  HelloAck{proto:1, fw_version:"0.5.0", node_id:(8), max_frame:240,
                      caps:(u16), region:"EU868", profile:"MEDIUM_FAST"}
radio → app  Event{kind:"boot", detail:"0.5.0"}
radio → app  Rx{frame:(bytes), rssi:i16, snr:i16, link:"lora", at_ms:u64}
app → radio  Tx{frame:(one whole OMP frame ≤240 B)}
```

Configuration keys accepted through `ConfigSet` (one key/value per message):

| Key | Values | Notes |
| --- | --- | --- |
| `node.name` | string ≤32 | announce display name |
| `node.class` | `client` / `repeater` / `gateway` / `tracker` / `room_server` | reboot recommended |
| `radio.region` | `EU868`, `EU433`, `US915`, `AU915`, `IN865`, `KR920`, `AS923` | live retune |
| `radio.profile` | `MEDIUM_FAST`, `LONG_FAST`, `LONG_SLOW`, `SHORT_FAST`, `SHORT_TURBO` | rejected if `bw_khz > region.bw_khz` |
| `radio.tx_power` | `-9..30` dBm request | clamped to `min(region max, board PA cap)`; only exceeds the legal max when an override is active |
| `radio.duty_override` | `-1` = legal region duty, `0..100` % | token bucket re-configured live; `0` blocks all TX |
| `radio.frequency` | kHz, `0` = region centre | advanced/custom centre; out-of-radio-range rejected (150 000–960 000 kHz) |
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
## 8.1 First-article bring-up — T1000-E and RAK4631 (2026-11-26/27)

The T1000-E was the first physical board; the RAK4631 WisBlock 4631 followed. Both now compile,
flash, boot, answer the USB companion link and pass a two-node RF test in both directions on
their BLE-compiled-out images (`env:t1000e_usb`, `env:rak4631_usb`). Bring-up required these fixes
(all in-tree, so the board envs work from a clean PlatformIO install):

| Area | Fix |
| --- | --- |
| Toolchain | pin `toolchain-gccarmnoneeabi@1.100301.220327` (GCC 10.3): GCC 7.2's `arm_acle.h` is not C++17-safe; `build_unflags = -std=gnu++11` so the firmware's `-std=gnu++17` wins |
| CMSIS/TinyUSB | add `platformio/framework-cmsis` and `extra_scripts/openmesh_nrf52.py`, which adds the CMSIS include and the TinyUSB library include (`cores/nRF5/TinyUSB` is missing from the plain git framework checkout) |
| Framing | include `boards/*.cpp` in `build_src_filter`; `LED_BUILTIN` added to the variant headers |
| Link | add `-Wl,--wrap=malloc/free/realloc/calloc` (required by Adafruit FreeRTOS `heap_3`), and pin `tool-adafruit-nrfutil` / `tool-bossac-nordicnrf52` for the upload target |
| RadioLib 7 | nRF52 SPI pins via `SPI.setPins(miso, sck, mosi)` + `SPI.begin()`, and pass `SPIClass` by reference to `Module` |
| RadioLib 7 RX | poll the radio IRQ flags (`getIrqFlags`/`getPacketLength`/`readData`/`clearIrqFlags`): `PhysicalLayer::available()` is the direct-receive buffer and is always 0 in LoRa packet mode, so the original receive path never read a frame |
| LR1110 RF switch | the T1000-E sub-GHz antenna switch is driven by the LR1110's DIO5-DIO8; RadioLib `begin()` does not install the switch table, so the board-specific Meshtastic T1000-E table must be set via `setRfSwitchTable()` (see `src/hal/hal_radio.cpp`) |
| RAK startup | **do not** add `-Wl,--defsym=__StackSize=...`: it changes the symbol the Adafruit startup stack-fill loop uses while the linker keeps `__StackLimit`, so the fill runs past the end of RAM and the chip faults before `setup()` (the former 16 KiB "fix" was the RAK blocker; the 2 KiB linker stack is ISR/SoftDevice-only) |
| Runtime / sleep | `sleepMs()` calls `delay()` (TinyUSB CDC flush + `vTaskDelay`) instead of a busy loop, so the FreeRTOS tickless-idle low-power state is used; direct RTC1/WFE pokes remain forbidden (FreeRTOS/SoftDevice own the clock domain) |
| Companion stack | the Adafruit loop task has a 4 KiB stack (1024 words); the companion chunk/SLIP/message buffers (≈7.7 KiB worst case) live in file-scope statics in `src/main.cpp`, not on that stack |
| Companion writes | USB CDC and Adafruit `BLEUart` writes can be short (`BLEUart` unbuffered TX sends one notification but reports full length); both transports chunk/retry with a bounded timeout so large replies (`HelloAck`, `Stats`) are never truncated |
| BLE name | advertised name starts with `OpenMesh` (`OpenMesh T1000-E`, `OpenMesh RAK4631 solar`): the Android app's BLE scan only lists radios with that prefix |
| SoftDevice for flash | with BLE compiled out the SoftDevice is still enabled (`OM_ENABLE_SOFTDEVICE=1` in the `*_usb` envs) because Adafruit `InternalFS` writes use `sd_flash_*` SVCs; `randomBytes()` then uses the SoftDevice RNG application API instead of the SD-owned `NRF_RNG` peripheral |
| USB/BLE | BLE + USB coexistence is fixed by ordering: enable the SoftDevice in `setup()` and start BLE advertising from `loop()` ~5 s after boot. Starting advertising during setup overlapped the TinyUSB/SoftDevice hand-off and radio bring-up and destabilised USB on both first articles. Verified: stable USB companion + host-visible advertising + RF two-node PASS on `t1000e` and `rak4631_solar` |
| RAM | 16 peers / 16 stored envelopes (the full 2048-entry dedup table is kept) to leave stack headroom next to S140 |
| Diagnostics | `[env:t1000e_probe]` (`firmware/probe/main.cpp`) is a USB-CDC + LED first-article probe; `[om]` serial markers and short settle delays in `setup()` keep the boot path observable |

Verified on hardware 2026-11-27: build, UF2 (`0xADA52840`) and nrfutil flashing on both boards,
boot, USB CDC enumeration, USB companion **Ping/Pong + HelloAck + Stats**, `radio=ok`, light sleep
(uptime and companion stay live), BLE + USB coexistence on the product images (0 USB disconnects
after boot, host scans see `OpenMesh T1000-E` / `OpenMesh RAK4631 solar`), a full BLE NUS companion
session from a host BLE client (Ping/Pong + HelloAck with node id/region/profile), the InternalFS
write path used by first-boot identity/store writes (`env:t1000e_fswrite` wrote and read back a
file), a full blank-board first boot with a regenerated persistent identity (`env:t1000e_fsformat`
erase → `env:t1000e` boot), and the two-node RF test
([`tools/two_node_test.py`](tools/two_node_test.py)) passing **A->B and B->A** with advancing
`tx_frames`/`airtime_ms`/`rx_frames` (EU868 / MEDIUM_FAST / 869525 kHz). BLE advertising works on
the RAK4631 (host scan sees `RAK WisBlock 4631 solar`), but BLE-on images are not yet viable wired
companion nodes.

Current board-by-board state, root causes, diagnostics and next steps are tracked in
[hardware bring-up status, 2026-11-27](../docs/hardware/bringup-status-2026-11-27.md). Not yet
verified: a BLE companion **session** (NUS Ping/Pong from a host; only advertising is proven),
sleep current and deep sleep, battery calibration, and the other two boards.

## 9. Known issues, limitations and spec gaps

These are tracked for v0.1.x/M1 and are worth reading before trusting the firmware in the field.

1. **Hardware builds and first article.** The host library/tests and the T1000-E and RAK4631
   targets build locally; both boards boot, flash and pass the USB companion + BLE + two-node RF
   checks on their product images (§8.1). The remaining two board environments (Seeed Solar Node
   P1, Heltec Wireless Tracker) still need first-article build/boot checks; GNSS bauds, ADC
   calibration and sleep current need hardware measurement.
2. **Deep sleep missing for trackers.** v0.1 keeps the receiver on continuously and uses light
   sleep only (nRF52: `delay()` + FreeRTOS tickless idle); battery life is therefore days, not
   months. The power table reflects that. The scheduler and `PowerPolicy` already emit low-power
   hints.
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
12. **Regulatory override surfaces.** The firmware companion accepts override fields without the
    typed acknowledgement required by the bridge/CLI/UI (local physical-access trust boundary).
    The warning is surfaced as a serial log line, a companion `Event` and `Stats.override_active`;
    it is not carried in a `Telemetry` wire field because v0.5.0 makes no wire-schema change.
    `docs/hardware/boards.md` still carries the v0.1 region/power figures (owned by the docs
    workstream, outside this task's write scope).
13. **Vectors.** All published golden vectors pass in the host suite (7/7 files, including
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
| Repeater drops frames | EU868/AS923 10 % duty budget exhausted | Expected under load; inspect `Stats.airtime_ms`/`budget_drops` |
| GPS no fix | GNSS rail off, wrong baud, indoor | Check `gps.enabled`, board GNSS pins, move outdoors |
| Companion Hello gets no reply | Wrong baud (USB CDC ignores baud but monitors), CRC/SLIP mismatch | Use the bridge/web client; verify CRC over type+len+payload |
| Store replay not delivered | Addressee unknown to relay or offer cadence not elapsed | Wait ≥5 min or trigger a fresh announce |

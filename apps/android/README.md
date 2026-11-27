# OpenMesh for Android

Standalone Android client for OpenMesh v0.7.1 (regulatory maxima + emergency override). The Rust OMP/1 protocol engine runs
**on the phone** through UniFFI; the LoRa radio is a dumb authenticated companion pipe over BLE or
USB. No bridge, no server, no account.

```
┌────────────────────── Android phone ──────────────────────┐
│ Compose UI (chats, thread, channels, map, nodes, contacts, │
│ radio, settings, onboarding, help)                         │
│        │ StateFlow / Flow                ▲ notifications    │
│  MeshService (foreground, connectedDevice)                  │
│   ├─ UniFfiEngineBridge ◄─ UniFFI ─ openmesh-ffi/rust core │
│   ├─ FramedCompanionClient (SLIP + OM + CRC16) over BLE/USB│
│   └─ Room (messages/contacts/channels/nodes) + DataStore   │
└──────────────┬─────────────────────────────────────────────┘
               │ BLE NUS / USB CDC serial (companion frames)
        ┌──────▼──────┐
        │ LoRa radio  │  OpenMesh firmware: relay + store + companion
        └─────────────┘
```

---

## What is implemented (v0.4.0)

| Area | Status |
| --- | --- |
| Rust engine via UniFFI (`uniffi.openmesh_ffi.OpenMeshNode`) | ✅ `UniFfiEngineBridge` — identity, DM/channel text, position, attachments, announce, retry, mark-read, channels, contacts, stats, state export/import; no direct JNI from UI |
| Foreground service (`MeshService`, `connectedDevice`) | ✅ survives backgrounding; 250 ms `tick` loop, `ingestFrame`, outbound frames → companion write, reconnect/backoff via transports |
| Companion transports | ✅ BLE (Nordic-UART-like, device name prefix `OpenMesh`) and USB CDC serial (115200 8N1); SLIP framing + CRC16 + CBOR handshake; radio settings get/set and stats |
| Persistence | ✅ Room (messages, contacts, channels, node cache), DataStore (prefs, UI mode, setup, engine state, identity ciphertext); identity seed wrapped by AndroidKeyStore AES-GCM |
| Notifications | ✅ message channel with sender/channel + text preview (never media), tap → thread, mark-read on open; persistent service notification |
| Onboarding | ✅ 3 steps (role → country/name/privacy → secure) applying the shared `content/presets/setup.json` preset; creates a private channel + invite QR |
| Chats + thread | ✅ direct/channel threads, announced display names, explicit delivery/read labels (tap a bubble for the plain-language status), outgoing-media progress + stall detection/retry, retry, reply model, day separators, text/position/attachment bubbles |
| Media | ✅ send: gallery/camera image → downscale+WebP loop to the profile cap with pre-send airtime estimate; voice note with `MediaRecorder` AAC-LC mono 16 kHz ~12 kbps MP4 (duration cap per profile). Receive: images decode inline (tap → full-screen viewer), voice notes play in the thread (Codec2 via FFI → `AudioTrack`, Opus/AAC via `MediaPlayer`) with play/stop state and honest format/duration/size labels |
| QR | ✅ zxing scan for contact/channel invites; own contact QR; channel invite QR; every `openmesh://` URL validated |
| Channels | ✅ create with invite QR, join via scan/paste, list/unread |
| Nodes / map | ✅ node list (announced name + short id, role, hops, RSSI/SNR, battery, last seen); peer link-stability panel (quality, route, delivery summary); osmdroid map with node markers |
| Radio screen | ✅ BLE/USB connect, firmware/region/profile, signal, frames/airtime counters, tips |
| Settings | ✅ beginner/advanced modes, name/role/region/profile/store-forward/announce, notifications, radio, airtime, identity/backup warning, help/about links |
| Help | ✅ bundled `content/help` (17 topics, offline), search, beginner/advanced filter, lightweight markdown reader; every control maps to a help topic (`HelpKeys`) |
| About/agents/backbone | ✅ MCP guidance, backbone tip with public-exposure/Openflare note, licences, version |
| Tests | ✅ 39 JVM unit tests (preset parity with `content/presets/setup.json`, QR parse/build, media caps/math, notifications, help registry, framing, airtime) + `cargo test -p openmesh-ffi` (8) |

A **regulatory override** is deliberately hard to enable and is never offered in beginner mode or the setup wizard. It is not legal advice: values are EIRP ceilings, antenna gain changes effective power, and national law varies. See “Regulatory override” below.

Known limits (documented, not secrets): no Play Store/F-Droid listing yet (APK sideload only); no
emulator instrumentation tests in CI (JVM tests only); voice uses AAC/MP4 on Android while the web
client uses Opus/OGG (both playable by the other client); the map shows node positions and
offline-cached tiles but does not yet draw a "my location" marker/history (needs a runtime location
permission flow); radio firmware must be flashed with OpenMesh firmware that supports the companion
protocol.

---

## Regulatory maxima and the emergency override (v0.5.0)

Defaults are the **legal maxima** for the selected region. The Settings screen (Advanced) and the
radio/status card show e.g. `Legal limit: 27 dBm · 10 % duty · 250 kHz bandwidth` (EU868) plus a
“verify your local rules” note. The setup wizard shows the legal line but never offers an override.

| Region | Centre | Max power (EIRP) | Duty | Max BW |
| --- | --- | --- | --- | --- |
| EU868 | 869 525 kHz | 27 dBm | 10 % | 250 kHz |
| EU433 | 433 920 kHz | 10 dBm | 10 % | 250 kHz |
| US915 | 915 000 kHz | 30 dBm | fair use | 500 kHz |
| AU915 | 915 000 kHz | 30 dBm | fair use | 500 kHz |
| IN865 | 865 000 kHz | 30 dBm | fair use | 250 kHz |
| KR920 | 920 000 kHz | 23 dBm | fair use | 250 kHz |
| AS923 | 923 200 kHz | 20 dBm | 10 % | 250 kHz |

Effective TX power is always clamped to the radio hardware cap (SX1262 boards 22 dBm, LR1110
15 dBm). Values are EIRP ceilings; **a high-gain antenna can push you over the legal limit**.

### Enabling an override (Advanced → Settings → Regulatory)

1. Tap **Enable override…**. The modal shows the full warning: transmissions may violate local law,
   void certification and cause interference; use only in a life-threatening emergency where local
   law permits; you are responsible.
2. Enter duty % (default = legal max), TX power dBm (default = legal max, hard-capped by hardware),
   optional frequency kHz (150 000–960 000 kHz, antenna warning) and an optional reason (≤120 chars,
   recorded in the activity log).
3. **Safe reductions** (duty/power at or below legal, region frequency) can be saved without a typed
   acknowledgement.
4. Going **above** a legal limit requires ticking “I understand” **and** typing `OVERRIDE` exactly.
   The engine validates the same rule (`regulatoryOverride` patch, `FfiException.InvalidArgument`
   on failure); the modal shows the error instead of silently failing.
5. While active, every screen shows a red banner:
   “⚠ Regulatory override active — may violate local law; you are responsible. Disable in Advanced →
   Regulatory.” The radio/config sync sends the effective `radio.tx_power`,
   `radio.duty_override` (`-1` = legal region duty) and `radio.frequency` (kHz).
6. **Disable is one tap** in the same card; it clears the effect, the acknowledgement and the warning
   immediately, and pushes the legal defaults back to the radio.

QA tip (debug builds): long-press the “About & agents” title → **Reset & restart** clears the
onboarding flag for re-testing; the override itself can be disabled in Settings at any time.

---

## Media tiers and tiny voice (v0.6.0)

Tier metadata is data, not code: `content/presets/media.json` is copied to `assets/presets/media.json`
by Gradle and parsed by `MediaTiers.kt` (JVM parity test against the real repository file).

| Tier | Image target | Voice codec / MIME | Bitrate | Max duration |
| --- | --- | --- | --- | --- |
| Tiny | 320 px WebP q0.35 | Codec2 1300 · `audio/c2-1300` | 1.3 kbps (175 B/s) | 30 s |
| Medium | 640 px WebP q0.55 | Codec2 3200 · `audio/c2-3200` | 3.2 kbps (400 B/s) | 20 s |
| High | 1280 px WebP q0.70 | Opus/OGG (API 29+) or AAC/MP4 | ~12 kbps | 15 s |
| Original | no resize (explicit confirm) | — | — | — |

* **Images**: the composer shows a tier menu, compresses with `Bitmap.compress(WEBP_LOSSY)`, then a
  preview with the **actual bytes**, estimated airtime and % of the hourly duty budget. The default
  is the largest tier whose target fits the active profile cap; **Original** requires a confirmation
  and is disabled on `LONG_*` profiles.
* **Voice**: recorder tier selector. Tiny/medium record raw 8 kHz mono s16le PCM with `AudioRecord`
  and encode through the Rust Codec2 FFI (`openmeshVoiceEncode`); the effective duration cap is
  `min(tier max, profile cap / tier bytes-per-second)` and the same bytes+airtime preview is shown
  before sending. High uses the platform recorder (OGG/Opus on API 29+, AAC/MP4 fallback).
* **Received images**: stored attachments are decoded off the UI thread with `BitmapFactory`
  (downsampled to a safe edge for the bubble), shown as an inline thumbnail and opened full-screen
  on tap; missing/corrupt files show plain-language fallback copy instead of a dead end.
* **Playback**: Codec2 attachments are decoded through `openmeshVoiceDecode` and played with
  `AudioTrack` (8 kHz mono); Opus/AAC use `MediaPlayer`. One player per thread, so notes never
  overlap; bubbles show Loading/Playing/error state, format, duration (from metadata or the Codec2
  payload size) and size.
* **Settings**: default image and voice tier are persisted in DataStore (defaults from
  `media.json`: medium image, tiny voice) and used by the composer/recorder.
* Wire format is unchanged: attachments carry MIME + bytes as before; Codec2 payloads are raw
  frame streams (7 B/40 ms tiny, 8 B/20 ms medium).

---

## Display names, delivery status and peer details (v0.7.2)

**Display name (announced, not identity).** Settings → *Your identity* shows `You are <name> · <short id>`.
The name is carried in announces (max 32 UTF‑8 bytes, trimmed, empty rejected) so other nodes can
show a friendly name; the 8‑byte node id and the Ed25519/X25519 keys never change when you rename.
Saving a name applies it to the engine config, pushes it to the radio companion and **sends an
announce immediately** (the regular announce interval is 30 min), then shows a confirmation. Your
contact QR on the Contacts tab uses the same name. Peers that have not announced a name appear as
`node <first8>`.

The app resolves a label in this order: channel name → the name the peer last announced → the
locally pinned invite/contact name → `node <first8>`. A peer rename therefore replaces a stale
cached name; a pinned invite name is only a fallback (there is no alias-editing UI yet). Names are
shown in the chats list, thread headers, channel sender labels, nodes list/details, map markers,
notifications and the peer panel. Peer details always show the name **and** the short id.

**Delivery and read receipts.** Outgoing bubbles show an explicit word next to the tick:
*Queued* (waiting for a radio link; queued messages are kept), *Sent*, *Delivered*, *Read*,
*At relay* (store-and-forward) or *Failed — tap to retry*. Tap or long-press an outgoing bubble for a
plain-language explanation. Direct messages can receive a `read` ack from the peer (the app sends
`mark_read` when you open a thread); channel/group messages have **no per-person receipts**, so the
copy says so instead of pretending. Status updates are monotonic — a late packet cannot downgrade
*Delivered*/*Read* back to *Sent*.

**Peer link stability panel.** Tap a chat header (or a node in the Nodes tab) for name + short id,
role/class, hops, RSSI/SNR, last heard, a *Strong / OK / Weak / Offline* quality label, whether they
are a pinned contact, the phone's own radio-link summary, and a delivery/read summary for that
conversation (or channel semantics for group chats). Route wording is beginner-friendly: *Direct
link (0 hops)* vs *Via N relays*. A node not heard for 5 minutes is labelled *Offline* even if an old
RSSI value is cached.

## Outgoing media reliability (v0.7.2)

Attachments are the one transfer type that can stay in flight for minutes, so the thread does not
leave the user guessing:

- **Progress** — when the core reports resource progress for an outgoing attachment, the bubble
  shows a progress bar and "Uploading… N%". Before the first chunk event it shows an indeterminate
  bar with "Uploading…" (the FFI cannot always map a pending resource to its message yet).
- **Slow-media hint** — while an upload has been running for more than 8 s, the thread shows
  "Large media can take minutes on radio — it continues in the background. If it stalls you can
  retry from the message." The hint disappears once the upload finishes or stalls (the bubble then
  takes over).
- **Stall detector** — a per-message tracker watches ResourceProgress and delivery events. If an
  outgoing attachment still shows `SENT` with no activity for **2× the expected transfer window,
  clamped to 45–180 s**, the bubble flips to "Upload stalled — no progress for N s." with a
  **Retry upload** button (also available from the message-status dialog). `SENT` remains "handed to
  the radio", `DELIVERED` remains "peer/bridge ACK"; a channel still has no per-recipient receipt.
- **Retry without duplicates** — retry re-sends the stored bytes through the normal attachment path
  as a fresh attempt and replaces the local row in place (delete old id + insert the new engine id),
  so there is always exactly one bubble. The core's `retry_message` only retransmits a still-pending
  announcing message, not a stalled resource, which is why the fallback exists. Queued attachments
  show "Queued — it will upload when the radio link is ready."
- **Duty-aware pacing hint** — the core helper `openmeshResourcePaceIntervalMs(profile, region,
  batch)` (2 chunks per window) is applied to the node at engine start and whenever the profile or
  region changes: `node.setResourcePaceIntervalMs(pace)`. The app logs
  `resource pace: profile=… region=… batch=2 -> Nms` once per configuration (useful for the stability
  test); the core default is kept if the helper is unavailable.

## Build

### Prerequisites

* JDK 17+ (CI uses Temurin 21; this host has `$HOME/.local/java/jdk21`).
* Android SDK Platform 35, Build-Tools 35.0.x, NDK 27.0.12077973.
* Rust stable with Android targets: `rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android`.
* `cargo-ndk` (`cargo install cargo-ndk --locked`).
* Node/pnpm are **not** needed: the web client is not part of the Android build.

### 1. Rust engine + generated Kotlin bindings

```sh
export JAVA_HOME="$HOME/.local/java/jdk21"
export ANDROID_HOME="$HOME/android-sdk"
export ANDROID_SDK_ROOT="$ANDROID_HOME"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.0.12077973"
export PATH="$HOME/.cargo/bin:$PATH"

cd apps/android
./scripts/build-ffi.sh
```

The script cross-compiles `crates/openmesh-ffi` for `arm64-v8a`, `armeabi-v7a` and `x86_64`
(override with `OPENMESH_ANDROID_ABIS`), writes `app/src/main/jniLibs/<abi>/libopenmesh_ffi.so`,
then generates Kotlin into `app/src/main/java/uniffi/openmesh_ffi/`.
Both are **generated build outputs and gitignored** — never commit them.

If the generated bindings are absent, Gradle falls back to a clearly-marked local stub
(`app/src/ffiStub/`) so the UI still compiles; that build reports "engine not linked" and must not
be shipped. CI always runs `build-ffi.sh` first, which excludes the stub automatically.

### 2. Gradle build

```sh
echo "sdk.dir=$ANDROID_HOME" > local.properties    # gitignored
./gradlew lintDebug testDebugUnitTest assembleDebug
```

Outputs:

* debug APK: `app/build/outputs/apk/debug/app-debug.apk` (≈23 MB, three ABIs + assets)
* release APK: `app/build/outputs/apk/release/app-release-unsigned.apk` (≈5 MB when unsigned)
* unit test report: `app/build/reports/tests/testDebugUnitTest/index.html`

### 3. Release signing (never commit a keystore)

`app/build.gradle.kts` reads signing material from environment variables:

| Variable | Meaning |
| --- | --- |
| `ANDROID_KEYSTORE_FILE` | path to a JKS/PKCS12 keystore |
| `ANDROID_KEYSTORE_PASSWORD` | store password |
| `ANDROID_KEY_ALIAS` | key alias |
| `ANDROID_KEY_PASSWORD` | key password |

```sh
export ANDROID_KEYSTORE_FILE="$HOME/keys/openmesh-release.jks"
export ANDROID_KEYSTORE_PASSWORD='…'
export ANDROID_KEY_ALIAS='openmesh'
export ANDROID_KEY_PASSWORD='…'
./gradlew assembleRelease
```

If `ANDROID_KEYSTORE_FILE` is unset/nonexistent, `assembleRelease` produces an **unsigned** APK;
sign it yourself with `apksigner`:

```sh
$ANDROID_HOME/build-tools/35.0.0/apksigner sign --ks openmesh-release.jks \
  --out openmesh-0.4.0-release.apk app/build/outputs/apk/release/app-release-unsigned.apk
```

CI (`.github/workflows/android.yml`) builds the debug APK on every PR and, when repository secrets
`ANDROID_KEYSTORE_BASE64`/`ANDROID_KEYSTORE_PASSWORD`/`ANDROID_KEY_ALIAS`/`ANDROID_KEY_PASSWORD`
exist, a signed release APK. Keystores are decoded to `$RUNNER_TEMP` only.

---

## Install (sideload)

1. Build/sign the APK (above), or download `openmesh-0.4.0-android-debug.apk` from the release page.
2. Enable **Install unknown apps** for your file manager/browser, or use `adb`:
   ```sh
   adb install -r app/build/outputs/apk/debug/app-debug.apk
   ```
3. Open **OpenMesh**. The first launch is the 3-step onboarding:
   * **What is this device?** client / tracker / repeater / gateway;
   * **Where are you?** country (drives the legal region), node name, keep-location-private;
   * **Secure my node** — applies the shared preset and creates a private channel, showing the
     invite QR.
4. Grant **notifications** (optional) and, when connecting a radio, Bluetooth/USB permissions.
5. Pair a radio and send your first message. Without a radio the app still runs the engine and
   queues frames for when a companion link appears.

---

## Pairing a radio

| Board | Link | Notes |
| --- | --- | --- |
| Seeed SenseCAP T1000-E | BLE | pocket tracker/client; enable the companion firmware build |
| RAK WisBlock (RAK4631) | USB serial | solar repeater; use a data USB cable |
| Seeed SenseCAP Solar Node P1 | BLE or USB | solar repeater/gateway |
| Heltec Wireless Tracker | USB serial / BLE | tracker / screen node |

* **BLE** — the app scans for devices whose name starts with `OpenMesh` and connects to the
  Nordic-UART-like service. Press the board's pairing/wake button if it is not advertising.
* **USB** — connect a data cable and accept the Android USB-host permission prompt. Serial
  settings are 115200 8N1; no driver installation is needed.
* The radio must run OpenMesh companion firmware with the matching region/profile. The app pushes
  profile/region/name/store-forward settings on connect.
* Changing profile/region disconnects other nodes on the old setting — update the whole group.

---

## Radio connection UX

The radio screen is state-driven rather than a single "connected" flag:

* Phases: **scanning → connecting → handshaking → ready**, each with plain-language text, a
  step rail, an elapsed counter and a Cancel action; the spinner is never the only indicator
  (screen readers get a state description).
* BLE scans for ~5 s and connects to the strongest OpenMesh radio, showing how many it found and
  which device it is connecting to (name, address, signal).
* Failures are typed and persistent: Bluetooth off, permissions, no radio found, scan/GATT failure,
  missing companion service, handshake timeout, link lost, USB device/permission/port. Each shows
  one or two beginner hints (and Help → Troubleshooting) plus Try again / USB / BLE.
* A ready link keeps a health check: if the radio stops answering its 15 s keep-alive for 30 s it is
  marked "not answering" with a Reconnect action; losing the link shows a "Lost connection to the
  radio" card instead of silently looking connected.
* Messages sent while the link is not ready are stored as **queued** (bubble label + inline card +
  banner chip) and sent automatically when the radio becomes ready; queued messages survive an app
  restart. The queue is capped at 50 with a clear error.
* Demo mode is shown as "Demo mode · virtual mesh" — never as a radio error or a connect prompt.

The copy rules live in `docs/design/ux-help-system.md` §6.1; the pure presentation/error logic is in
`ui/radio/RadioPresentation.kt` + `data/transport/LinkProgress.kt` and unit-tested on the JVM.

---

## Data, identity and security

* The 32-byte identity seed is generated on the device, wrapped with an AES-GCM key in
  **AndroidKeyStore**, and stored only as ciphertext in DataStore. Losing the Keystore/factory reset
  loses the identity; there is no account recovery. Export/backup is available in Settings (and the
  About screen explains the warning).
* Room stores messages, contacts, channels and node cache. **Channel PSKs are never stored in the
  database** — channel keys live in the engine state, exported opaquely and persisted locally.
* Message bodies are end-to-end encrypted by the engine; the radio and relays see ciphertext,
  metadata and timing only.
* No analytics, no cloud calls, no accounts. Map tiles are the only optional network use (osmdroid);
  cache the area you need if fully offline.
* API/MCP: the Android app has no local MCP server in v0.4.0. To give an AI agent control, run
  `openmesh-bridge mcp` on the machine that hosts a bridge/backbone; see `docs/agents/mcp.md`.

---

## Tests and verification (this host)

```sh
cd apps/android
./gradlew lintDebug testDebugUnitTest assembleDebug
cargo test -p openmesh-ffi          # 8/8
```

Current results: **77/77 JVM unit tests pass**, `lintDebug` passes, `assembleDebug` produces
`app/build/outputs/apk/debug/app-debug.apk` (~23 MB), `assembleRelease` produces an unsigned
~5.1 MB APK. The APK contains `assets/help/*` (17 topics + index), `assets/presets/setup.json`, and
`lib/{arm64-v8a,armeabi-v7a,x86_64}/libopenmesh_ffi.so`.

The unit tests cover: preset parity against the real `content/presets/setup.json` (all roles,
countries, fallback, security defaults), invite parse/build/validation, image caps + downscale +
sample-size math, voice caps, notification preview rules, help-topic registry parity with the help
index, SLIP/CRC/CBOR framing, airtime model.

---

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| Gradle: "SDK location not found" | `echo "sdk.dir=$ANDROID_HOME" > local.properties` or set `ANDROID_HOME`. |
| `build-ffi.sh: ANDROID_NDK_HOME is not set` | Point it at `…/ndk/27.0.12077973` (or install NDK 27). |
| `cargo ndk` unknown command | `cargo install cargo-ndk --locked`. |
| Bindgen generated nothing | The host library must be a **debug** build (release strips `UNIFFI_META` symbols); the script handles this. |
| "OpenMesh core not linked in this build" | You built with the stub; run `scripts/build-ffi.sh` then rebuild. |
| App asks for token/panel | That is the desktop bridge workflow, not Android: the Android app is standalone and never asks for a bridge token. |
| No radio found (BLE) | Board not advertising (press pairing/wake); BLE permissions denied; device name must start with `OpenMesh`. |
| USB connect fails | Use a data cable, accept the permission prompt, check the board's USB CDC firmware. |
| Messages stay on one tick | No peer/radio in range, profile mismatch, or the recipient is offline with no store-capable relay; the engine retries and store-and-forward covers up to 72 h. |
| Voice note won't record | Microphone permission / another app holds the mic; profile caps are 4–15 s. |
| Map blank | osmdroid tiles need network once; previously viewed areas are cached. |
| Lint fails in CI | Run `./gradlew lintDebug` locally; generated UniFFI sources are excluded from NewApi checks by design. |

---

## Repository layout

```
apps/android/
├── app/src/main/java/org/openmesh/android/
│   ├── data/engine/        # EngineBridge seam + UniFfiEngineBridge (real) + demo engine
│   ├── data/repository/    # MeshRepository: state, engine loop wiring, invites, presets
│   ├── data/transport/     # BLE/USB transports, SLIP/CRC/CBOR companion client
│   ├── data/db/            # Room entities/DAO/database
│   ├── data/prefs/         # DataStore preferences (mode, setup, identity ciphertext)
│   ├── data/presets/       # shared beginner presets parser + apply logic
│   ├── data/security/      # AndroidKeyStore-wrapped identity seed
│   ├── data/help/          # bundled help corpus + control→topic registry
│   ├── data/notify/        # notification channels + preview builder
│   ├── media/              # image compression, AAC voice recorder/player, QR renderer
│   ├── service/            # MeshService (foreground, connectedDevice)
│   └── ui/                 # Compose screens, navigation, theme, components
├── app/src/ffiStub/        # local-only stub bindings (excluded when real bindings exist)
├── scripts/build-ffi.sh    # cargo-ndk + uniffi-bindgen
└── app/src/test/           # 39 JVM unit tests
```

## Licence

AGPL-3.0-only. Third-party components: osmdroid, zxing-android-embedded, zxing core,
usb-serial-for-android, Nordic Android BLE, JNA (see repository `NOTICE`). No Reticulum-licensed
code is used.

## Foreground service type and permissions (Android 14+)

`MeshService` keeps the mesh alive in the background. Android 14+ requires a granted Bluetooth or
USB permission before a `connectedDevice` foreground service may start; a fresh install has
neither, so the service tries `connectedDevice` first and falls back to the declared
`specialUse` type until a radio is paired. That keeps the engine and the persistent notification
running instead of crashing the app on first launch. Once the user grants Bluetooth (Radio → BLE)
or accepts the USB permission, the service runs as a normal `connectedDevice` service.

## Device verification
**v0.6.0 media-tier spot-check (Pixel-class A069P, Android 17, build 0.6.0/versionCode 7):**

- **Tiny voice:** recorded 17 s with `AudioRecord` → FFI `openmeshVoiceEncode("tiny")` → 2.9 KiB
  `audio/c2-1300`; bubble label "Codec2 1.3k · 17 s · 2.9 KiB"; airtime 11.3 s / 3.2 % of the
  hourly duty; playback via FFI decode + `AudioTrack` confirmed (AudioTrack session in logcat),
  no crash. (Transfer shows failed with no radio attached, as expected.)
- **Image tiers:** all four chips visible (Tiny 320 / Medium 640 / High 1280 / Original); Tiny on a
  1260×2800 screenshot → measured 1.7 KiB WebP, 144×320, ≈7.1 s airtime / 2.0 % duty, sent and
  shown as "WebP · 1.7 KiB"; Original → 92.2 KiB PNG with over-cap warning + confirmation
  checkbox, Send correctly blocked.
- Compression trace visible in `adb logcat -s OpenMesh` (`image compress: begin … webp … success`);
  no FATAL/ANR. (Web side: 270 tests incl. a real Codec2 WASM round-trip; cross-client playback
  over the mesh still needs radios.)


**v0.5.1 regulatory-override spot-check (Pixel-class A069P, Android 17, build 0.5.1/versionCode 6):**

- Advanced → Settings → Regulatory shown the EU868 legal limit (27 dBm · 10 % · 250 kHz) and the
  warning card; enabling with the default safe values (22 dBm / 10 % / region centre) saved
  **without typed confirmation** and produced the app-wide banner, the engine log
  `regulatory: override enabled … acknowledged=true`, and Radio → "Effective duty: 10 %".
- With duty raised to 100 %, the acknowledgement UI appeared and **Enable override was inert until
  the "I understand" checkbox was ticked and `OVERRIDE` typed** (no engine log / no state change
  before that).
- After confirmation: `override active: 22 dBm · 100 % · 869.525 MHz`, banner everywhere,
  `regulatory: override enabled … duty=100 … acknowledged=true`, and Radio →
  "Effective duty: 100 % (region/override policy)". No crashes/ANRs (`adb logcat -b crash` empty).
- One-tap **Disable override** removed the banner, logged
  `regulatory: override disabled … acknowledged=false`, and Radio → "Effective duty: 10 %".
  Hardware cap stayed at 22 dBm (legal region max 27 dBm was never exceeded).


The debug APK was installed and exercised on a real Android phone (Pixel-class, Android 17):
fresh install → notification permission → onboarding → engine start → private channel/invite QR
→ channel message send → offline Help → all main screens, with no crashes and no ANRs
(`adb logcat -s OpenMesh`, `adb logcat -b crash`). Real radio pairing, mesh delivery and
image/voice capture still need radio hardware and a second node.

## QA reset (debug builds)

Long-press the **About & agents** title → **Reset & restart** clears `setup_completed` (keeps
identity, messages and settings) so onboarding can be re-tested without uninstalling.

# OpenMesh for Android

Standalone Android client for OpenMesh v0.5.1 (regulatory maxima + emergency override). The Rust OMP/1 protocol engine runs
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
| Chats + thread | ✅ direct/channel threads, delivery ticks, retry, reply model, day separators, text/position/attachment bubbles |
| Media | ✅ gallery/camera image → downscale+JPEG loop to the profile cap with pre-send airtime estimate; voice note with `MediaRecorder` AAC-LC mono 16 kHz ~12 kbps MP4 (duration cap per profile), playback |
| QR | ✅ zxing scan for contact/channel invites; own contact QR; channel invite QR; every `openmesh://` URL validated |
| Channels | ✅ create with invite QR, join via scan/paste, list/unread |
| Nodes / map | ✅ node list (role, hops, RSSI/SNR, battery, last seen); osmdroid map with node markers |
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

Current results: **62/62 JVM unit tests pass**, `lintDebug` passes, `assembleDebug` produces
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

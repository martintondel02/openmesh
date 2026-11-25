# OpenMesh for Android

Kotlin + Jetpack Compose client for OpenMesh. It talks to an OpenMesh radio over USB serial or
BLE (the companion API) or, on a LAN, to an `openmesh-bridge` over its WebSocket API. This is
the **v0.1.0 scaffold**: structurally complete, CI-ready, and deliberately honest about what is
(and is not) wired up yet.

> **Not compiled on the development host.** The machine that built v0.1.0 has no Android SDK
> and no emulator, so `./gradlew assembleDebug` has **not** been run locally. The project is
> written to build with the standard SDK/AGP toolchain and the CI workflow below builds it; treat
> this as reviewed source, not a verified APK. See "What is verified" for the static checks that
> were done instead.

---

## 1. What is in this scaffold

| Area | Status |
| --- | --- |
| Gradle KTS project (`settings.gradle.kts`, root `build.gradle.kts`, `app/build.gradle.kts`, `gradle/libs.versions.toml`, wrapper jar+properties) | done |
| Manifest with the right permissions (Bluetooth scan/connect, USB host, foreground service, notifications, camera, legacy location) | done |
| Material 3 dark theme, Compose navigation (Chats, Map, Nodes, Contacts, Settings), top-level bottom bar | done |
| Screens: `ChatListScreen`, `ChatThreadScreen`, `NodesScreen`, `MapScreen` (osmdroid), `ContactsScreen`, `SettingsScreen`, `QrScanScreen` | done, with seeded demo content so every screen is reviewable |
| Companion transport abstraction (`CompanionTransport`) | done |
| USB serial transport (`usb-serial-for-android`, 115200 8N1, USB host permission flow) | done |
| BLE transport (Nordic-UART-like service, platform scanner + Nordic BLE library for GATT) | done (targets `no.nordicsemi.android:ble:2.7.5`) |
| Companion framing: SLIP de-framing, CRC-16/CCITT-FALSE, `OM` header, CBOR payload codec | done |
| `FramedCompanionClient`: HELLO/HELLO_ACK handshake, TX/RX, config get/set, stats, ping, events | done |
| Repository with `StateFlow`s (messages, chats, nodes, contacts, channels, radio, settings) | done |
| Foreground service for the radio link (`connectedDevice` type) | done |
| QR scanning for `openmesh://contact` / `openmesh://channel` invites (zxing-android-embedded) + manual paste | done |
| Airtime estimator (spec formula + resource transfer model) and pre-send estimate dialog | done |
| Unit tests: airtime parity with PROTOCOL.md Appendix A, CRC check value, SLIP escaping/CRC rejection, CBOR round-trips | done (JVM unit tests) |
| OMP frame construction/parsing (Rust core via UniFFI) | **not yet** — `DemoEngineBridge` is used; `FfiEngineBridgeUnavailable` documents the gap (M2) |
| Image compression/upload, real microphone recording, local message database, background sync, notifications with reply actions | **not yet** (M2) |
| MapLibre native renderer | **not yet** — osmdroid is used (no API key); MapLibre is a documented alternative |

Because the core is not linked, sending from a **connected** radio reports an explicit error
("OpenMesh core not linked in this build") rather than pretending to send; in **demo mode**
(the default) messages are simulated locally so the UI state machine (queued → sent → delivered)
is exercised. That behaviour is intentional and visible in the UI.

---

## 2. Build

### Prerequisites

* JDK 17 or newer (CI uses Temurin 21; AGP 8.7 supports it).
* Android SDK Platform 35 + Build-Tools 35.0.x.
* Android Studio Ladybug or newer (optional; the CLI build is enough).
* A device or emulator running Android 8.0 (API 26) or newer.

### Command line

```bash
cd apps/android
# Optional: point Gradle at your SDK if it is not in ANDROID_HOME / local.properties
echo "sdk.dir=$HOME/Android/Sdk" > local.properties

./gradlew assembleDebug          # APK: app/build/outputs/apk/debug/app-debug.apk
./gradlew testDebugUnitTest      # JVM unit tests (airtime, CRC, SLIP, CBOR)
./gradlew lintDebug              # optional; first run downloads SDK components
```

Install on a device with USB debugging:

```bash
adb install -r app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n org.openmesh.android.debug/org.openmesh.android.MainActivity
```

The Gradle wrapper jar is committed (`gradle/wrapper/gradle-wrapper.jar`, Gradle 8.10.2) so CI
does not need a system Gradle. If you delete the jar, `./gradlew` falls back to a `gradle`
binary on `PATH`.

### Android Studio

Open `apps/android` as a project, let Gradle sync (it downloads the Gradle distribution and
dependencies), pick the `app` configuration, and run.

### CI

`.github/workflows/android.yml` already builds `assembleDebug` on `ubuntu-latest` with JDK 21 and
`android-actions/setup-android`. A minimal equivalent:

```yaml
name: Android
on:
  push: { branches: [main], paths: ["apps/android/**", ".github/workflows/android.yml"] }
  pull_request: { paths: ["apps/android/**"] }
jobs:
  build:
    runs-on: ubuntu-latest
    defaults: { run: { working-directory: apps/android } }
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-java@v4
        with: { distribution: temurin, java-version: "21" }
      - uses: android-actions/setup-android@v3
      - uses: gradle/actions/setup-gradle@v4
      - run: ./gradlew assembleDebug testDebugUnitTest --no-daemon
      - uses: actions/upload-artifact@v4
        with:
          name: openmesh-android-debug
          path: apps/android/app/build/outputs/apk/debug/*.apk
```

If the checked-in wrapper jar is ever rejected by policy, add `gradle-version: "8.10.2"` to
`setup-gradle` and run `gradle assembleDebug` instead.

---

## 3. What is verified (and what is not)

Verified locally (no SDK required):

* Kotlin sources, Gradle KTS files and manifest are self-consistent (packages, resource names,
  dependency coordinates are declared in `gradle/libs.versions.toml`).
* Unit-test sources match the production API (airtime estimator, CRC, SLIP, CBOR).
* The airtime model reproduces `PROTOCOL.md` Appendix A values (240 B `MEDIUM_FAST` = 611 ms,
  201 B = 519 ms) — the test asserts this and is the cross-language check for the Rust
  `util::lora_airtime_ms` implementation.
* The JSON/URI invite format matches `WEBSOCKET-API.md` §4.3/4.4 (`openmesh://contact…`,
  `openmesh://channel…`).
* The companion framing follows `COMPANION-API.md` §1 (SLIP delimiters, `OM` magic, type,
  little-endian length, CRC over `type || len || payload`).

**Not verified:**

* `assembleDebug` / `lint` / instrumented tests / emulator run — no SDK on the build host.
* Real USB and BLE radios (the code paths are written against `usb-serial-for-android` 3.8.1 and
  Nordic BLE 2.7.5, but no device was attached).
* zxing scanner Activity and osmdroid tile rendering on a device.
* API-level edge cases (BLE permissions on API 26–30 vs 31+, USB permission PendingIntent flags,
  foreground-service type on API 34+).

### Known dependencies on external APIs

The USB and BLE implementations are thin adapters over third-party libraries. If a library
changes its API, the adapters are the only files to fix:

* `usb-serial-for-android` (`com.github.mik3y:usb-serial-for-android:3.8.1`, JitPack):
  `UsbSerialProber`, `UsbSerialPort`, `UsbManager`.
* Nordic Android-BLE-Library (`no.nordicsemi.android:ble:2.7.5`):
  `BleManager`, `BleManagerGattCallback`, notification/MTU requests.
* zxing-android-embedded (`com.journeyapps:zxing-android-embedded:4.3.0`) + zxing core 3.5.3.
* osmdroid (`org.osmdroid:osmdroid-android:6.1.20`) with OSM tiles (requires internet; the mesh
  itself does not).

---

## 4. Package map

```
org.openmesh.android
├── OpenMeshApplication.kt        process-wide MeshGraph init
├── MainActivity.kt               Compose host, notification permission, openmesh:// deep links
├── data
│   ├── model/                    domain models + DemoData (seeded preview content)
│   ├── transport/
│   │   ├── CompanionTransport.kt     byte-stream interface (USB/BLE/demo)
│   │   ├── UsbSerialTransport.kt     CDC-ACM/CP210x/CH340/FTDI via usb-serial-for-android
│   │   ├── BleTransport.kt           NUS GATT via Nordic BLE library
│   │   ├── SlipFraming.kt            SLIP + OM header + CRC-16/CCITT-FALSE
│   │   ├── CborCodec.kt              minimal CBOR subset for companion payloads
│   │   └── FramedCompanionClient.kt  handshake, TX/RX, config, stats, events
│   ├── engine/                   EngineBridge (FFI boundary) + DemoEngineBridge
│   └── repository/               MeshRepository (StateFlows), QrInvite parser, MeshGraph
├── service/CompanionService.kt   foreground service (connectedDevice)
├── ui/                           theme, navigation, components, screens
└── util/                         Hex, Crc16, AirtimeEstimator (mirrors Rust util.rs)
```

The intended M2 path: implement `create_engine`, `send_text`, `ingest`, `nodes` and `next_event`
in `crates/openmesh-ffi` with UniFFI, generate Kotlin bindings, and replace
`DemoEngineBridge`/`FfiEngineBridgeUnavailable` with the generated implementation. The
repository and screens do not need to change; `EngineBridge` is the seam.

---

## 5. Companion API conformance notes

* **Framing**: `0xC0` frame delimiter; `0xDB 0xDC` = 0xC0, `0xDB 0xDD` = 0xDB; payload is
  `OM | type(u8) | len(u16 LE) | payload | crc16`. The app's decoder tolerates arbitrary
  split/coalesced reads and drops frames with a bad CRC or a length > 1024.
* **CRC byte order**: `COMPANION-API.md` does not state whether the CRC-16 is stored
  little-endian or big-endian; this client uses **little-endian** (consistent with the length
  field). Firmware must match this; it should be pinned in the spec (flagged to the architecture
  owner).
* **HELLO**: sent immediately after the byte link is connected (USB port open / BLE notifications
  enabled). The app requires `HelloAck.proto == 1`; mismatches surface in Settings and disable
  sending.
* **TX/RX**: `Tx` carries exactly one OMP frame (≤ `HelloAck.max_frame`, default 240); `Rx`
  frames are passed to the core. Radios may filter frames but must report the filter mode in
  caps.
* **Configs sent on connect**: `radio.profile`, `radio.region`, `node.name`, `mesh.store`
  (values as strings, per COMPANION-API §4).
* **Security**: the companion link is not mutually authenticated in v1 (physical access is the
  trust boundary). Do not expose the USB/BLE device to untrusted apps; a pairing key is planned
  for M2.

---

## 6. Permissions and privacy

| Permission | Why |
| --- | --- |
| `BLUETOOTH_SCAN` (`neverForLocation`) / `BLUETOOTH_CONNECT` | BLE companion link |
| `BLUETOOTH` / `BLUETOOTH_ADMIN` (≤ API 30), `ACCESS_FINE_LOCATION` (≤ API 30) | legacy BLE scanning |
| `USB` host (no permission constant; `usb_device_filter.xml`) | USB serial companion link |
| `FOREGROUND_SERVICE` + `FOREGROUND_SERVICE_CONNECTED_DEVICE` | keep the radio link alive |
| `POST_NOTIFICATIONS` | link/message notifications |
| `CAMERA` | QR invite scanning |
| `INTERNET` + `ACCESS_NETWORK_STATE` | map tiles only; the mesh works offline |

No location or message data leaves the device except over the mesh/companion link. The map is
the only feature that uses the internet; demo mode and the mesh are fully offline.

---

## 7. Roadmap (M2, from `docs/ROADMAP.md`)

1. UniFFI bindings for the Rust core (identity, engine, events) replacing the demo engine.
2. Image compression and attachment upload; real Opus voice recording/playback; media progress
   from core resource events.
3. Local persistence (messages, contacts, channels) and bridge ↔ app sync semantics.
4. BLE/USB hardening: pairing key, reconnect/backoff, permission and MTU edge cases, OEM matrix.
5. Background sync with message notifications and reply/mark-read actions.
6. F-Droid/APK release, signing, CI artifacts, reproducible builds where possible.
7. Accessibility and UX pass (dark theme, large text, RTL, no placeholder copy).

Until then, treat the app as a reviewable scaffold for the Android client structure and the
companion protocol, not as a usable messenger.

---

## 8. Sources and related docs

* `docs/architecture/COMPANION-API.md` — normative companion framing/messages/handshake.
* `docs/architecture/PROTOCOL.md` §9 (resources), §13 (companion summary), Appendix A (airtime).
* `docs/architecture/WEBSOCKET-API.md` — bridge API used when the app talks to a bridge.
* `docs/research/lora-capacity-and-media.md` — why the pre-send estimate dialog exists and what
  the numbers mean.
* `docs/ROADMAP.md` — M2 Android GA scope.

# Android GA — v0.4.0 specification

**Status:** normative for v0.4.0 · **Applies to:** `crates/openmesh-ffi` (UniFFI surface),
`apps/android` (Kotlin/Compose app), `.github/workflows/android.yml`, `scripts/`, `content/presets/`
**Related:** [COMPANION-API.md](../architecture/COMPANION-API.md), [PROTOCOL.md](../architecture/PROTOCOL.md),
[AI-NATIVE.md](../architecture/AI-NATIVE.md), [ux-help-system.md](ux-help-system.md)

The Android app is a **full standalone client**: the Rust protocol engine runs on the phone
(`openmesh-ffi` via UniFFI), the radio is a dumb authenticated pipe over BLE or USB, and the app
keeps the mesh alive in a foreground service. No bridge, no server, no account.

```
┌────────────────────── Android phone ──────────────────────┐
│ Compose UI (chats, map, nodes, contacts, help, onboarding) │
│        │ StateFlow/Flow                    ▲ notifications │
│  MeshService (foreground, connectedDevice) │                │
│   ├─ OpenMeshNode  ◄── UniFFI ── Rust engine (openmesh-core)│
│   ├─ CompanionClient (SLIP + OM + CRC16 framing)           │
│   └─ Room (messages, contacts, channels) + DataStore config│
└──────────────┬─────────────────────────────────────────────┘
               │ BLE NUS / USB CDC serial (companion frames)
        ┌──────▼──────┐
        │ LoRa radio  │  firmware: relay + store + companion
        └─────────────┘
```

---

## 1. Rust FFI surface (UniFFI)

`crates/openmesh-ffi` switches from the hand-written placeholder to **UniFFI (proc-macro API,
`uniffi` ≥ 0.28)**, exposing exactly what the app needs and nothing more. Everything is a thin,
typed wrapper over `openmesh-core`; no protocol logic lives in the FFI layer.

```kotlin
// Generated Kotlin package: uniffi.openmesh_ffi (uniffi 0.28.x, proc-macro)
// class OpenMeshNode(seed: ByteArray, config: FfiConfig) : Disposable, AutoCloseable
class OpenMeshNode(seed: ByteArray, config: FfiConfig) {
    fun nodeId(): String
    fun publicIdentityB64(): String
    fun configJson(): String
    fun setConfig(jsonPatch: String)

    fun ingestFrame(frame: ByteArray, nowMs: Long): FfiProcessed
    fun tick(nowMs: Long): FfiProcessed

    fun sendText(target: FfiTarget, text: String, nowMs: Long): FfiProcessed
    fun sendPosition(target: FfiTarget, lat: Double, lon: Double,
                     altM: Int?, accuracyM: Int?, nowMs: Long): FfiProcessed
    fun sendAttachment(target: FfiTarget, mime: String, filename: String?,
                       data: ByteArray, durationMs: Long?, nowMs: Long): FfiProcessed
    fun announce(nowMs: Long): FfiProcessed
    fun retryMessage(msgId: Long, nowMs: Long): FfiProcessed
    fun markRead(msgId: Long, nowMs: Long): FfiProcessed

    fun addContact(identityB64: String, name: String)
    fun removeContact(nodeIdHex: String)
    fun createChannel(name: String): FfiChannel
    fun joinChannel(name: String, pskB64: String): FfiChannel
    fun channels(): List<FfiChannelInfo>
    fun nodes(): List<FfiNode>
    fun stats(): FfiStats
    fun exportState(): ByteArray
    fun importState(state: ByteArray)
}
```

Types (all `uniffi::Record` / `Enum`, simple fields only):

| Type | Fields |
| --- | --- |
| `FfiConfig` | `name`, `nodeClass` (`client`/`repeater`/`gateway`/`tracker`), `profile`, `region`, `storeForward`, `acceptChannels`, `announceIntervalS` |
| `FfiTarget` | `Enum { Direct(nodeIdHex), Channel(channelIdHex) }` |
| `FfiProcessed` | `events: List<FfiEvent>`, `outbound: List<FfiOutbound>`, `msgId: Long?` — `msgId` is additive and non-null only from the `send*` methods, so the app can key its Room row immediately (the engine does not echo its own sends; acks arrive later) |
| `FfiOutbound` | `frame: ByteArray`, `mode: FfiSendMode` (sealed: `Flood` / `Direct(peerHex)`) |
| `FfiEvent` | `Message{from,msgId,text?,receivedAtMs,channel?}`, `GroupMessage{channel,from,msgId,text?}`, `Ack{msgId,kind}`, `NodeSeen{node}`, `Position{from,lat,lon,...}`, `Telemetry{node}`, `ResourceProgress{peer,msgId,resId,received,total}`, `ResourceReceived{peer,channel?,msgId,mime,filename?,data,durationMs?}`, `AttachmentSent{peer,msgId}`, `StoreOffer{relay,count}`, `Error{message}` |
| `FfiNode` | `nodeIdHex`, `name`, `class`, `caps: List<String>`, `hops`, `lastSeenMs`, `rssi?`, `snr?`, `batteryPct?`, `lat?`, `lon?` |
| `FfiChannel` / `FfiChannelInfo` | `channelIdHex`, `name`, `inviteUrl` (`FfiChannel` also `pskB64` **once**) |
| `FfiStats` | `txFrames`, `rxFrames`, `forwarded`, `dropped`, `nodes`, `channels`, `airtimeMs` |
| Errors | UniFFI generates base class `FfiException` with nested `InvalidArgument`, `UnknownPeer`, `PayloadTooLarge`, `Internal`, each carrying `msg`. Kotlin catches `FfiException` and switches on the subtype. |

Rules:

* **Threading:** object is `Arc<Mutex<Engine>>`, `Send + Sync`; every method takes `&self`.
  Kotlin calls it from the service coroutine and the UI reads only via `StateFlow` snapshots.
* **Time:** callers pass `nowMs`; no wall-clock reads inside the engine.
* **Attachments:** `sendAttachment` encapsulates message + manifest + resource in one call and
  emits `AttachmentSent` plus progress events; incoming data arrives via `ResourceReceived`.
* **State:** `exportState`/`importState` (opaque JSON) persist contacts/channels/counters; message
  history lives in Room, not in the engine.
* **Secrets:** PSK returned once by `createChannel`; no seed ever leaves the app except through
  the encrypted store. Errors never include secrets. The app generates its 32-byte seed with
  `SecureRandom` on first run — the FFI deliberately exposes no `generateSeed()` helper.
* **Contacts:** `FfiNode` intentionally has no `isContact` flag; contact state lives in the app
  database (Room) and is joined at the repository layer.
* **Numeric types:** ids/timestamps use signed Rust types so Kotlin sees `Long`/`Int` (not ULong).
* A `uniffi-bindgen` binary is part of the crate
  (`cargo run -p openmesh-ffi --bin uniffi-bindgen -- generate --library … --language kotlin`).
* `cargo test -p openmesh-ffi` covers: two-node DM+ack through the FFI, channel create/join,
  attachment round-trip, state export/import, target parsing, error mapping.
* Generated Kotlin + `.so` files are **build outputs**, gitignored, produced by
  `apps/android/scripts/build-ffi.sh` and CI.

---

## 2. App architecture

### 2.1 Service and loop
`MeshService` (foreground; `connectedDevice` type, falling back to `specialUse` when no
Bluetooth/USB permission has been granted yet — Android 14+ refuses `connectedDevice` otherwise,
and the fallback keeps the service alive instead of crashing a fresh install):
1. Loads/creates the identity seed (Keystore-wrapped AES-GCM, stored in DataStore).
2. Builds `OpenMeshNode`, restores exported engine state.
3. Opens the selected companion transport (`BleTransport` or `UsbSerialTransport`), performs the
   companion `Hello`, announces.
4. Runs one loop: read companion frames → `ingestFrame` → deliver `FfiOutbound` frames to the
   radio; every 250 ms `tick(now)`; persist config/state on change; reconnect with backoff; update
   `StateFlow` and notifications.
5. Stops cleanly on user request, persisting state first, and calls `close()` on the engine
   object (UniFFI `AutoCloseable`).

The **radio never runs the app's protocol identity**: it is a framed pipe (`COMPANION-API.md`).
Radios may still relay/store on their own; that is harmless and documented.

### 2.2 Data
* **Room**: messages (direct + channel), contacts, channels (no PSKs in DB), nodes cache,
  attachment metadata; attachment bytes under `filesDir/attachments/<id>`.
* **DataStore**: UI mode (beginner/advanced), radio selection, notification prefs, identity
  ciphertext, setup completion.
* Single source of truth: repository exposes `StateFlow<MeshState>`; UI never calls FFI directly.

### 2.3 Screens (GA)
Onboarding (3-step beginner wizard → applies local secure preset), Chats + thread (text, image,
voice, delivery ticks, retry, day separators, reply-to), Channels (create with invite QR, join via
camera/paste), Nodes (roles, signal, battery, last seen, map link), Map (osmdroid, markers,
position history, own position), Contacts (QR scan + display + paste identity, verify), Radio
(scan/connect BLE/USB, signal, stats), Settings (beginner/advanced modes, name/role/region/profile,
store-forward, announce interval, notifications, identity export/backup warning), Help (bundled
corpus from `content/help`, search + audience filter), About/Agents (MCP tips, links, version,
licences), Backbone tip (run it on a server; exposure via Openflare).

Beginner/advanced and help-tip rules follow `docs/design/ux-help-system.md`: beginner hides raw
identity, TTL/profile math and advanced radio settings behind the mode switch; every control has
an inline explanation with a deep link into Help.

### 2.4 Media and QR
* **Images:** picker/camera → downscale + compress (JPEG/WebP) in a loop until under the profile's
  attachment cap (same table as web/`util::max_attachment_bytes`), show estimated airtime before
  send; never block the service loop.
* **Voice:** `MediaRecorder` AAC-LC mono 16 kHz ~12 kbps in MP4 (`audio/mp4`), duration capped by
  profile; playback with `MediaPlayer` + waveform from amplitude samples. Documented as
  Android-native alternative to the web's Opus/OGG.
* **QR:** zxing embedded for scanning, zxing core for rendering own contact/channel invites; every
  scanned `openmesh://contact|channel|backbone` URL is validated before use (unit-tested parser).

### 2.5 Notifications
Notification channels: `messages` (per conversation, silent grouping), `service` (persistent).
New direct/channel message → notification with sender/channel name, text preview (never media
content), tap → thread deep link, mark-read on open. POST_NOTIFICATIONS requested in onboarding.
Battery: no polling wake-locks beyond the foreground service; radio link keeps the CPU awake only
while connected.

---

## 3. Presets shared with the bridge

The beginner secure preset is **data, not code duplication**: `content/presets/setup.json`
(country→region, role→profile/store-forward/privacy/announce) is the single source. The bridge
loads it for `/api/setup/*`; the Android app ships it as an asset and applies it locally; the web
demo uses the same file. Kotlin unit tests assert the generated table matches the asset exactly.

---

## 4. Build and CI

Local (developer with Rust + Android SDK/NDK):

```bash
apps/android/scripts/build-ffi.sh          # cargo ndk → jniLibs + UniFFI Kotlin
cd apps/android && ./gradlew assembleDebug
```

CI (`.github/workflows/android.yml`, required gate):

1. JDK 21, Android SDK platform 35 + build-tools 35 + NDK 27, Rust toolchain + android targets,
   `cargo-ndk` (cached).
2. `apps/android/scripts/build-ffi.sh` (arm64-v8a, armeabi-v7a, x86_64).
3. `./gradlew lintDebug assembleDebug` → upload `app-debug.apk` artifact.
4. Release path: if `ANDROID_KEYSTORE_BASE64` + `ANDROID_KEYSTORE_PASSWORD` +
   `ANDROID_KEY_ALIAS` + `ANDROID_KEY_PASSWORD` secrets exist, `assembleRelease` signed and upload;
   otherwise build `assembleRelease` unsigned and document how to sign. Never commit a keystore.
5. Release workflow attaches `openmesh-<version>-android-debug.apk` (and signed release APK when
   configured) to the GitHub release.

---

## 5. Acceptance criteria (v0.4.0)

1. `apps/android/scripts/build-ffi.sh` produces Kotlin bindings + `.so` for three ABIs from a clean
   tree.
2. `./gradlew assembleDebug` passes locally (Lead verifies with installed SDK) and in CI; debug
   APK artifact uploaded and attached to the v0.4.0 release.
3. App boots to onboarding; completing it applies the secure preset, creates a channel + invite
   QR, and lands in Chats with the radio connect card.
4. With a connected companion radio (simulated transport in debug builds for CI/manual): send text
   DM + channel message, receive both, delivery ticks update; send/receive an image and a voice
   note; QR scan adds a contact; notification appears and opens the thread; service survives app
   backgrounding.
5. Beginner/advanced modes and Help work offline from bundled content; every control has an
   explanation.
6. JVM unit tests (≥25) cover QR parsing, preset parity with `content/presets/setup.json`, media
   size targets, companion framing, repository reducers, notification content; `cargo test -p
   openmesh-ffi` green; `cargo test --workspace` stays green.
7. No secrets/keystores committed; `scripts/check_docs.py`/`check_help.py`/`check_openapi.py`
   green; README/INSTALL updated (Android install via APK, sideload steps, F-Droid note as future).

### Device verification (v0.4.0)

Verified on a real Google phone (Pixel-class A069P, Android 17, 1260x2800 @480dpi) via USB adb:

1. Fresh install (`adb uninstall` + install): notification-permission prompt, three onboarding
   steps with correct insets, engine start (~1 s), preset persisted, private channel created,
   invite QR shown, "Start messaging" opens Chats with one `#family` channel (no duplicates on
   re-setup).
2. `OpenMesh` logcat shows `onboarding: applying preset → engine start: ready → config applied →
   preset persisted setup_completed=true → channel created → MeshService: onCreate`. The service
   starts as `connectedDevice` when a Bluetooth/USB permission is held and as `specialUse`
   otherwise, with **no crash** in either case.
3. Channel message send works (bubble + `now`), thread header shows the channel name, Help renders
   the bundled corpus offline with audience filtering, Nodes/Map/Contacts/Settings/Radio/About all
   render; no FATAL/ANR in logcat.

Not verifiable without hardware/second node (documented limits): real BLE/USB radio pairing, mesh
delivery/acks to another node, incoming-message notifications, image/voice capture end-to-end, and
online map tiles.

### Known limits for v0.4.0 (documented, not blockers)
* No Play Store/F-Droid listing yet; APK sideload only (debug + optionally signed release).
* No emulator instrumentation tests in CI (no hardware acceleration budget); JVM unit tests only.
* Voice uses AAC/MP4 on Android (web uses Opus/OGG); both are playable by the other client.
* Radio firmware must be flashed with OpenMesh firmware supporting the companion link.

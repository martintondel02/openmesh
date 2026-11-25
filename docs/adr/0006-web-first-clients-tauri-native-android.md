# ADR 0006 — Web-first clients, Tauri for desktop, native Android

* **Status:** accepted (v0.1.0)
* **Date:** 2026-11-25
* **Decision record:** summary row 6 in
  [ARCHITECTURE.md §4](../architecture/ARCHITECTURE.md#4-key-decisions)
* **Related:** [WEBSOCKET-API.md](../architecture/WEBSOCKET-API.md),
  [ADR 0005](0005-companion-radio-architecture.md),
  [ADR 0007](0007-media-pipeline.md),
  [Ratspeak research](../research/reticulum-lxmf-notes.md) §5.4

## 1. Context

OpenMesh needs clients on the web, on Linux/macOS/Windows desktops, and on Android. The
constraints:

* **One team, no native-platform specialists.** The project is built by a small human-directed
  AI team; maintaining three UI codebases (web, desktop, Android) in three frameworks is not
  feasible for v0.1.
* **Media needs modern web APIs.** Image downscaling (`createImageBitmap`, canvas), audio
  recording (`MediaRecorder` with Opus in Ogg), and WebGL maps (MapLibre) are strongest in the
  browser. Native platforms re-implement these per OS.
* **Desktop wants to be more than a browser tab.** Access to the local bridge, filesystem,
  tray/notifications, auto-update and signing matter for a real desktop app.
* **Android has genuinely different requirements.** USB host serial and BLE companion links,
  foreground services, notification actions, QR scanning, and a native map/voice experience.
  A wrapped web app would technically run but would be a worse citizen on battery, background
  limits, and hardware access — exactly where Meshtastic's native app wins.
* Prior art: Ratspeak (Tauri 2 + Rust core) ships desktop and mobile from one Rust core and a
  webview UI; it also needed vendored `tao`/`wry` patches for Android. Meshtastic's Android app
  is native Kotlin, its desktop is a separate app; the ecosystems diverge.

## 2. Decision

**Web-first UI in React/TypeScript, packaged for desktop with Tauri 2, and a separate native
Kotlin/Jetpack Compose app for Android.**

### 2.1 Client matrix

| Platform | Stack | Transport to mesh | State |
| --- | --- | --- | --- |
| Web (PWA, `apps/web`) | React 19 + TypeScript + Vite + Tailwind, MapLibre GL, MediaRecorder | `openmesh-bridge` WebSocket/REST on loopback | primary client; demo mode with seeded data when no bridge |
| Desktop Linux/macOS/Windows | the same web build inside Tauri 2 (`apps/web/src-tauri`), Rust shell for bridge lifecycle, tray, notifications | bridge on loopback (bundled or system) | primary desktop client; not buildable on this host (webkit deps), shipped from CI |
| Android (`apps/android`) | Kotlin + Jetpack Compose (Material 3), USB-serial (`usb-serial-for-android`) + BLE NUS (`Nordic`), ML Kit/zxing for QR, foreground service | companion API over USB/BLE, or bridge WS when on the same LAN | native scaffold in v0.1; planned GA in M2 |
| iOS | not planned for v0.1 | — | Android-first because USB/BLE and sideloading are practical |

### 2.2 The shared contract is the bridge API, not a UI codebase

All clients speak the same **WebSocket + REST API** ([WEBSOCKET-API.md](../architecture/WEBSOCKET-API.md)),
which is JSON, versioned, and transport-agnostic. That is what makes two UI stacks acceptable:

* the web build and the Android app can implement the same event handlers independently;
* the CLI and any future integrations can consume the same API;
* the Android app uses the companion API when talking to a radio, and the same message/thread
  models map onto its repository/StateFlow layer;
* the bridge demo mode (seeded data, simulated radio) makes both UIs testable without hardware.

### 2.3 Android specifics (why native)

* USB host serial: `UsbManager` + a serial driver; the companion API frames over CDC-ACM.
* BLE NUS: scan/connect/GATT, MTU negotiation, notification reassembly; needs a foreground
  service for background links and notification actions.
* QR contact/channel invites: camera + ML Kit/zxing; the invite encodes `openmesh://contact?...`
  / `openmesh://channel?...` with keys/profile/region.
* Maps: osmdroid or MapLibre native; offline tile considerations differ from the web.
* Voice: `MediaRecorder`/`MediaCodec` (Opus in Ogg/WebM) and playback; no browser constraints.
* Notifications and runtime permissions (BLUETOOTH_SCAN/CONNECT, POST_NOTIFICATIONS, location for
  BLE scanning) are native concerns a webview cannot handle cleanly.
* The app is designed to swap in the Rust core via UniFFI for key storage/session logic later
  (`crates/openmesh-ffi`), keeping the wire logic in one place.

## 3. Consequences

### 3.1 Positive

* **One UI codebase for web + desktop.** Tauri uses the same Vite build, so feature work on the
  web client benefits desktop automatically.
* **Best-in-class media on web/desktop.** The browser's image/canvas/MediaRecorder APIs cover the
  image-compression and Opus-recording pipeline without native plugins.
* **Native Android quality where it matters.** USB/BLE reliability, background service,
  notifications, QR and maps are implemented natively instead of via web shims.
* **One API contract, two renderers.** The WS API and its data models are the stable boundary;
  clients can evolve independently.
* **Simulator/demo mode everywhere.** The bridge can run against a simulated radio, so both UIs
  are reviewable in CI and screenshots without hardware.

### 3.2 Negative

* **Two UI implementations to keep consistent.** Chats/map/nodes/contacts/settings exist twice
  (React and Compose), with duplicated UX decisions and potential drift. The mitigation is a
  shared API and an explicit screen-by-screen parity checklist, not shared code.
* **Tauri desktop is not "just web".** Native packaging, signing, webkit dependencies,
  auto-update and platform-specific bugs are real work; the Ratspeak project needed vendored
  webview patches for Android, which we avoid by not shipping Tauri on mobile.
* **Android scaffold is unverified here.** No Android SDK on the build host; the app is CI-ready
  but compiled only in CI/human environments (documented in `apps/android/README.md`).
* **Web stack lock-in.** React + Vite + Tailwind + MapLibre is a large dependency tree; a future
  rewrite is expensive.
* **Feature lag.** Native Android will trail the web client unless parity is enforced; the
  v0.1 Android scaffold is deliberately structural (navigation, transport, repository) rather
  than feature-complete.
* **Tauri bundle is a release engineering task.** Not part of v0.1 acceptance (system webkit
  missing on the host); it is a CI/human pipeline item.

### 3.3 Mitigations

* Parity checklist in the roadmap (M2: Android GA) and screen-level specs in
  `apps/android/README.md`.
* The WS API is versioned (`v: 1`); clients must ignore unknown events/fields, so the web and
  Android clients can ship at different speeds.
* Demo mode in the bridge with seeded data lets both UIs be developed/reviewed without hardware.
* Shared design tokens (dark theme, spacing, type scale) documented once and copied into Tailwind
  and Compose themes.
* Keep the Android app thin: repository/StateFlow + Compose screens + transport; push complex
  logic into the bridge/FFI.

## 4. Alternatives considered

| Alternative | Why rejected |
| --- | --- |
| React Native for all mobile + web | Web + mobile from one codebase, but USB serial/BLE/foreground services on Android require native modules anyway; RN's bridge adds complexity for a small team; WebView UI on Android has the same battery/background issues as Tauri mobile. |
| Flutter for mobile + desktop + web | Strong UI toolkit, but Dart is a separate ecosystem from the Rust/TS teams, and USB/BLE Android still needs platform channels; web output is heavyweight. |
| Tauri for Android too | Tauri 2 supports Android, and Ratspeak ships it, but it required vendored `tao`/`wry`/`tauri-runtime-wry` patches for Activity lifetime bugs; the companion USB/BLE path is better served by native Kotlin. |
| Native desktop apps per OS (Swift/C#/GTK) | Three codebases; not feasible for a small team. |
| PWA only (no desktop packaging) | Loses filesystem/tray/auto-update and offline app-shell guarantees; Tauri is cheap on top of the same build. |
| Web only, Android as PWA | USB serial/BLE/notifications/background are not viable in a browser; the core use case is a phone talking to a radio. |
| **Web + Tauri desktop + native Android (chosen)** | Maximises shared code where it matters (web/desktop), gives Android the native quality the companion model needs, and keeps one API boundary. |

## 5. Follow-ups

1. **M2:** Android parity checklist (chats/threads, groups, contacts+QR, nodes, map, settings,
   voice record/playback, image attach with airtime estimate, connection service, notifications).
2. **M2:** wire the Kotlin repository to `openmesh-ffi` (UniFFI) once the FFI surface is real, so
   the app can use the Rust core for identity/session logic; keep the companion transport
   separate.
3. **M2:** CI builds `assembleDebug` on an Android runner; release signing and F-Droid metadata.
4. **M2/M3:** Tauri bundle CI for Linux/macOS/Windows (webkit dependencies), signing and
   auto-update channel.
5. **M3:** design parity tests/review checklist; consider generating client models from a shared
   JSON schema to reduce drift between TS and Kotlin types.

# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Protocol changes are called out
explicitly.

## [0.7.1] - 2026-11-26

### Changed — dependency maintenance (no functional or wire changes)

- **Tailwind CSS 4.3.5** migration in the web client: `@tailwindcss/vite` 4.3.5, `@import
  "tailwindcss"` + `@config "../tailwind.config.ts"`, removed `postcss`/`autoprefixer` and the
  PostCSS config, and updated v4 utility renames (`shadow-sm`→`shadow-xs`, `rounded-sm`→`rounded-xs`,
  `backdrop-blur-sm`→`backdrop-blur-xs`). Custom theme (colors, fonts, shadows, animations) kept;
  build/tests/demo verified with no visual drift.
- `@vitejs/plugin-react` 5.2.0 and `@types/node` 26.6.7 adopted (PR #11 merged; #8/#10 superseded).
- Rust lockfile kept on the latest semver-compatible versions (zerocopy 0.8.66); the combined
  major-version Dependabot PR (#14) was closed: ed25519-dalek/x25519-dalek 3, chacha20poly1305/
  sha2/hkdf 0.11/0.13, rand 0.10, base64 0.23, tokio-tungstenite 0.30 and uniffi 0.29 touch the
  crypto/wire/FFI layers and need a dedicated migration.
- Dependabot policy now groups **minor/patch** updates and ignores Rust major versions (majors are
  tracked as a migration task; security updates are unaffected). npm major updates stay individual.

### Fixed
- CI dependency noise from incompatible grouped major upgrades.

## [0.7.0] - 2026-11-26

### Added — remote operations and gateways

**Remote MCP over HTTP (opt-in)**
- `<data-dir>/mcp-http.json` (mode 0600, default disabled) exposes the MCP server on the bridge's
  existing HTTP listener at `/mcp`: JSON-RPC over POST (JSON or single SSE frame), SSE keep-alive
  on GET, `Mcp-Session-Id` sessions, bearer-token auth with the bridge token, **read-only scope by
  default** (`allow_write` opt-in), existing `confirm:true` rules, rate limits, 256 KiB body cap,
  and activity-log audit for every write (source `agent:<name>`/`agent:mcp-http`, no secrets).
- `openmesh-cli agent config --transport http` prints a client config; docs/agents/mcp.md gained a
  Remote MCP section and AI-NATIVE §1.1 documents the transport/scope rules. stdio remains the
  default; MCP stays (no removal).

**Webhook gateways (both directions)**
- `<data-dir>/gateways.json` (mode 0600) connectors with outbound signed webhooks (HMAC-SHA256
  `X-OpenMesh-Signature`, message metadata, channel/direct filters, attachment metadata only,
  bounded retries/backoff, activity audit) and optional inbound `POST
  /api/gateways/{id}/inbound` (constant-time HMAC verification, length/rate limits, injects a
  display-attributed text message into a configured channel).
- REST/CLI: GET/PUT/DELETE `/api/gateways*`, `POST /api/gateways/{id}/test`,
  `openmesh-cli gateways list|show|set|remove|test`; secrets are env-var references only, never
  stored, logged or returned. Adapter recipes for Matrix, SMS, email and satellite in
  `docs/guides/remote-operations.md`; new help topic 18.

**Multiple backbone links**
- `<data-dir>/backbone-links.json`; the bridge can join several backbones concurrently with
  per-link reconnect/backoff and status, deduped by the engine; legacy `join` imports as
  `default`. APIs `GET /api/backbone/links` + PUT/DELETE, `GET /api/backbone` links summary,
  WebSocket status; CLI `openmesh-cli backbone links list|add|remove`; BACKBONE.md multiple-links
  spec.

**Docs/help**
- New help topic "Gateways and remote agents" (18 topics), `docs/guides/remote-operations.md`,
  ADR-0012; README remote-operations section.

### Security
- Loopback defaults, token auth, read-only MCP default, env-only secrets, constant-time HMAC,
  rate limits, audit trail, and explicit docs that gateways break E2E for the bridged leg.
- No wire-format changes; golden vectors byte-identical.

### Changed
- Version 0.7.0 (workspace/web/Tauri/Android `versionCode` 8/firmware `OM_FW_VERSION`).

## [0.6.0] - 2026-11-26

### Added — media tiers and tiny voice (Codec2)

**Air-time-first media tiers**
- One shared tier table (`content/presets/media.json`) used by web, Android and tests:
  images Tiny (320 px) / Medium (640 px) / High (1280 px) WebP / Original;
  voice Tiny (Codec2 1.3 kbps, ≈1.4 KiB per 8 s), Medium (Codec2 3.2 kbps, ≈3.2 KiB per 8 s),
  High (Opus 12 kbps, ≈12 KiB per 8 s; Android may use AAC/MP4 where OGG/Opus is unavailable).
- Pre-send preview shows actual bytes, estimated airtime and % of the hourly duty budget, and
  suggests the largest tier that fits the active profile; Original is confirmation-gated and
  disabled by default on long-range profiles. Effective voice caps follow
  `min(tier max, profile cap / bytes per second)`.

**Codec2 tiny/medium voice, everywhere**
- New `openmesh-voice` Rust crate (pure-Rust `codec2` crate, LGPL-2.1-only AND MIT) implementing
  the 1300/3200 modes with fixed raw frame streams (`audio/c2-1300`: 7 B/40 ms;
  `audio/c2-3200`: 8 B/20 ms; 8 kHz mono s16).
- New `openmesh-voice-wasm` crate + `scripts/build-voice-wasm.sh`: the same code compiled to
  `wasm32-unknown-unknown` for the web client (fetched at runtime, graceful fallback if absent).
- UniFFI functions `openmeshVoiceEncode`/`openmeshVoiceDecode`/`openmeshVoiceTiersJson` for the
  Android engine; Android records PCM with `AudioRecord`, encodes tiny/medium voice via FFI and
  plays it with `AudioTrack`; High stays on the platform recorder.
- Cross-client playback: web/Android/desktop all play Codec2 raw, Opus/OGG and AAC/MP4.

**Docs**
- PROTOCOL §9.4 rewritten: voice tier table, framing, MIME, effective caps and airtime examples;
  media tier helpers and `content/presets/media.json`; new `docs/design/media-tiers.md`; help
  topic "Images and voice notes" rewritten around tiers and costs; NOTICE updated for `codec2`.
- No wire-format change; golden vectors byte-identical.

### Changed
- Version 0.6.0 (workspace/web/Tauri/Android `versionCode` 7/firmware `OM_FW_VERSION`).
  CI/release builds the Codec2 WASM before the web bundle.

## [0.5.1] - 2026-11-26

### Security
- **Web map:** upgrade `maplibre-gl` 5.24.1 → **6.19.0** (GHSA-jrc7-96c5-q579, patched in 6.4.1
  — XSS sanitizer bypass in `DOM.sanitize()`). Migrated to the v6 named exports and to an
  explicit bundled worker URL (`setWorkerUrl`) so the ES-module worker loads in Vite dev and in
  production builds; verified map rendering in both with zero console errors.
- **Web test tooling:** upgrade `vitest` 3.2.7 → **5.1.1** (GHSA-82fw-gwwq-j7x9 path traversal
  through `@vitest/mocker`; brings patched `@vitest/mocker` 5.1.1 and `tinypool` 2.2.0, covering
  GHSA-5gmw-xhrv-c9v3 and GHSA-85c8-ppgw-ccpr prototype-pollution→RCE). Node 22/24 is supported
  (CI uses Node 22); `tsconfig.app.json` now includes Node types for the test files that read
  `content/help`.

### Changed
- Version 0.5.1 (workspace, web/Tauri, Android `versionName`/`versionCode` 6, firmware
  `OM_FW_VERSION`). No protocol, wire-format, API or behaviour changes; golden vectors unchanged.

## [0.5.0] - 2026-11-26

### Added — legal-maximum defaults and a warning-gated regulatory override

**Region profiles at the legal maximum**
- `EU868` (including Norway) now defaults to **27 dBm EIRP / 10 % duty cycle / 250 kHz max
  bandwidth** in the 869.4–869.65 MHz sub-band; previously 14 dBm / 1 %, a conservative hybrid of
  the 868.0–868.6 MHz rules (~10× less airtime, ~13 dB less link budget).
- New `EU433` region (433.05–434.79 MHz, 10 dBm, 10 %, 250 kHz).
- `US915`/`AU915` raised to 30 dBm and 500 kHz max BW; `IN865` 30 dBm; `KR920` 23 dBm; `AS923`
  20 dBm / 10 % (conservative floor across the group).
- Profile/bandwidth validation: profiles wider than the region maximum are rejected (no 500 kHz
  in EU bands); firmware, bridge, web and Android share the same table.
- Defaults now come from the legal maxima; effective TX power is clamped to the board PA cap
  (SX1262 22 dBm, LR1110 15 dBm).

**Regulatory override (emergency use)**
- Advanced-only, off-by-default override for **duty cycle**, **TX power** and **frequency** in
  core, firmware, bridge API/CLI/MCP, web and Android.
- Full warning + "I understand" checkbox + typed `OVERRIDE` in web/Android; CLI requires
  `--i-understand-risk` when exceeding legal limits; MCP requires `confirm: true`; read-only
  agents are refused.
- Persistent red banner while active, activity-log audit entries, one-tap disable; safe
  reductions (lower duty/power, same frequency) need no acknowledgement.
- Hardware PA caps remain absolute; out-of-range frequencies rejected; no protocol/wire change.

**Docs and help**
- New help topic **"Legal limits, power and emergency override"** (17 topics), updated
  `radio-and-region`, new guide `docs/guides/regulatory-and-emergency.md`, ADR 0011 and the
  implementation contract `docs/design/regulatory-and-regions.md`.
- Region source notes (CEPT ERC/REC 70-03 Annex 1, Nkom/EEA, Meshtastic cross-check) and explicit
  "not legal advice / antenna EIRP" caveats on every surface.

### Changed
- Workspace version 0.5.0; Android `versionName` 0.5.0 (`versionCode` 5); firmware `OM_FW_VERSION`.
  No protocol or API breaking changes (all additive); golden vectors unchanged.

## [0.4.0] - 2026-11-25

### Added — Android GA and public-exposure guidance

**Android app (GA)**
- The Compose app now runs the real Rust protocol engine on the phone via **UniFFI**
  (`crates/openmesh-ffi`): `OpenMeshNode` object, typed events/outbound frames, attachment
  send/receive, export/import state, generated Kotlin bindings for `arm64-v8a`, `armeabi-v7a`
  and `x86_64`.
- Foreground `MeshService` keeps the mesh alive over **BLE (Nordic UART) or USB CDC** companion
  links, reconnects with backoff, persists messages in Room, stores the identity seed
  Keystore-wrapped, and posts message notifications that open the right thread.
- Screens: beginner onboarding applying the shared secure preset, chats (text/image/voice,
  delivery ticks, retry, replies), channels with invite QR, nodes, osmdroid map, contacts with
  QR scan/display, radio connect/status, settings with beginner/advanced modes, bundled Help
  corpus, agent/backbone tips.
- Media: in-app camera/gallery images compressed to the profile's airtime budget; voice notes
  recorded as small AAC/MP4 with playback; QR rendering for invites.
- Verified end-to-end on a real Android phone (Pixel-class, Android 17): fresh install,
  notification-permission prompt, onboarding, engine start, channel + invite QR, message send,
  offline Help, all main screens, zero crashes. Missing radio permissions no longer crash the
  foreground service: it starts as `connectedDevice` when a Bluetooth/USB permission is held and
  falls back to the declared `specialUse` type otherwise.
- Device-QA fixes: channel threads show the channel name (not the id), re-running setup reuses the
  existing private channel instead of duplicating it, and Bluetooth permissions are requested
  before scanning.
- Build: `apps/android/scripts/build-ffi.sh` (cargo-ndk + uniffi-bindgen) and CI that installs
  JDK 21 + Android SDK 35 + NDK 27, builds the debug APK (and a signed release APK when keystore
  secrets are configured), tests, and attaches the APK to releases.

**Shared presets and machine-readable setup**
- Beginner secure presets moved to `content/presets/setup.json`, embedded by the bridge and
  shipped as an Android asset; `/api/setup/*` behaviour is unchanged and unit-tested against the
  file (60 country codes, 4 roles).

**Public exposure guidance (Openflare)**
- Backbone setup payload (`GET /api/backbone/setup`) now includes an `exposure` object:
  recommended **Openflare** provider (outbound-only tunnel, no open inbound port), when to use it,
  steps, alternatives (WireGuard, Tailscale, Cloudflare Tunnel, Caddy/nginx), copyable loopback
  bind command and a guide link.
- Web backbone panel and setup wizard render the new "Expose it publicly (optional)" step and an
  Exposure card; new `docs/guides/backbone-exposure.md` covers Openflare, generic tunnels,
  VPN alternatives, a security checklist and troubleshooting.
- BACKBONE.md §8-§9b specify the payload and exposure rules.

### Changed
- Workspace version 0.4.0; Android `versionName` 0.4.0 (`versionCode` 4). No protocol or API
  breaking changes (FFI is new, bridge API additive).

## [0.3.0] - 2026-11-25

### Added — beginner-friendly UX and an AI-native control plane

**Beginner / advanced UX** (`docs/design/ux-help-system.md`)
- First-run 3-screen onboarding that applies a **secure preset** (region from country, radio
  profile, private channel + invite QR, position sharing off, token auth on) via
  `GET /api/setup/state`, `GET /api/setup/recommend`, `POST /api/setup/quick`; resumable
  "Get started" checklist.
- Beginner/Advanced mode switch (persisted, synced with the bridge): beginner shows only the
  choices that matter in plain language; advanced exposes every documented field, airtime math and
  secrets management.
- **Help Center** (`/help/:slug`, search, beginner/advanced filter, deep links, ToC) built from a
  single help corpus (`content/help/`, 16 topics + glossary) that is also embedded in the bridge
  (`GET /api/help*`) and served to AI agents; `HelpTip` on every control, ~35-term glossary
  popovers, guided empty states, actionable error toasts, insecurity/no-radio banners.
- **Agents page**: MCP configuration snippets (Claude/Cursor/DSH/generic), copyable beginner and
  advanced system prompts, read-only explainer, attributed activity log, token-revocation guidance.

**AI-native control plane** (`docs/architecture/AI-NATIVE.md`)
- `openmesh-bridge mcp`: Model Context Protocol server over stdio with **27 typed tools**
  (13 read, 14 write), resources (help topics, status, nodes, config), 5 prompts, read-only mode,
  `confirm: true` for destructive actions, 30 writes/min cap and `X-OpenMesh-Agent` attribution.
- Activity log: bounded 256-entry write log with source attribution (`user`, `web`, `cli`,
  `agent:<name>`), no bodies or secrets; `GET /api/activity`, MCP `openmesh_activity`, web table.
- `openmesh-cli agent prompt --tier beginner|advanced`, `agent config --client …`,
  `agent activity`, `help [topic]`.
- Machine-readable contracts: complete OpenAPI 3.1 (`docs/api/openapi.json`, served at
  `GET /api/openapi.json`, enforced by `scripts/check_openapi.py`), root `AGENTS.md`, `llms.txt`.
- Docs: `docs/QUICKSTART.md`, advanced cookbook, troubleshooting playbook, FAQ, long-form glossary,
  MCP reference + 7-prompt pack, ADR 0010.

### Fixed
- `openmesh-cli` no longer panics on startup from a clap `help` subcommand collision
  (`disable_help_subcommand`); `--help` and `help [topic]` both work.

## [0.2.0] - 2026-11-25

### Added — Backbone mode

OpenMesh bridges can now run as an always-on **backbone**: an authenticated TCP relay with a
live control panel, so a VPS, Raspberry Pi or home server can join distant meshes into one
network without any central service.

- **OBB/1 backbone protocol** (`docs/architecture/BACKBONE.md`): length-prefixed TCP framing,
  `hello → challenge → HMAC-SHA256(token) → welcome` handshake, open/allowlist/token auth modes,
  max peers, handshake rate limits, ping/RTT, per-peer frame/byte accounting, graceful kick.
  OMP payloads stay end-to-end encrypted; the backbone relays ciphertext and routing metadata only.
- **Backbone control API + panel**: `/api/backbone`, peers, live config, token rotation, setup
  payload and QR; web UI "Backbone" screen with stats cards, peer table/kick, live enable/disable,
  port/bind/max-peer/allowlist/store-forward controls and a copy-button setup wizard.
- **Client mode**: `openmesh-bridge join 'openmesh://backbone?…'` connects a gateway/repeater over
  the internet with reconnect/backoff and shows up in the backbone peer table.
- **Installer & deployment**: `scripts/install.sh` release installer (sha256-verified, no root by
  default, optional systemd user service), hardened systemd units, Docker/compose example,
  `docs/INSTALL.md` and a backbone operations guide (sizing, monitoring, TLS termination, WireGuard).
- `openmesh-bridge install-service --backbone` and `openmesh-cli backbone …` commands.

### Changed
- Bumped workspace version to 0.2.0; bridge API gained the backbone surface (backwards compatible:
  v1 API, additive events/commands).
- Release assets now include `install.sh` alongside binaries, web bundle and `SHA256SUMS.txt`.

## [0.1.0] - 2026-11-25

First public scaffold of OpenMesh.

### Added
- **OMP/1 protocol specification** (`docs/architecture/PROTOCOL.md`) with frame format, identity,
  AEAD envelopes, announces, managed flooding, resources/ARQ, store-and-forward, companion API,
  airtime model and security notes.
- `openmesh-core` Rust crate: frame codec, Ed25519/X25519 identities, XChaCha20-Poly1305 sealing,
  channel keys, CBOR message schemas, fragmentation/reassembly, dedup cache, airtime budget,
  store queue, LoRa airtime/region math, golden-vector generation.
- `openmesh-bridge` local daemon with WebSocket + REST API, redb persistence, attachment store,
  simulated/UDP/TCP transports and token auth.
- `openmesh-cli` diagnostics and `openmesh-sim` discrete-event mesh simulator with scenarios
  (relay chain, lossy retransmit, offline store-and-forward, image transfer).
- Web/PWA client (`apps/web`): chats, group channels, MapLibre map, node list, contacts, settings,
  image compression with airtime estimates, Opus voice notes, demo mode; Tauri desktop shell.
- Firmware (`firmware/`) for Seeed T1000-E, RAK4631 solar, Seeed Solar Node P1 and Heltec
  Wireless Tracker: OMP/1 codec, relay, store-and-forward, companion link, GPS, duty-cycle budget.
- Browser flasher (`apps/flasher`) and native Linux/Fedora flasher CLI (`tools/flash`).
- Android Compose scaffold (`apps/android`), UniFFI surface plan (`crates/openmesh-ffi`).
- Architecture docs, ADRs, research notes (Reticulum/LXMF, Meshtastic/MeshCore comparison,
  NomadNet bridge design, LoRa capacity analysis, AI development workflow) and roadmap.

### Security
- Alpha, unaudited. No forward secrecy in v1 (planned for OMP/2). See `SECURITY.md`.

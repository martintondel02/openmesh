# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Protocol changes are called out
explicitly.

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

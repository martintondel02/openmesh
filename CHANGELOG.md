# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Protocol changes are called out
explicitly.

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

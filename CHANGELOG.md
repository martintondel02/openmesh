# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Protocol changes are called out
explicitly.

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

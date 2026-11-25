# OpenMesh roadmap

**Status:** living document · v0.1.0 · **Last updated:** 2026-11-25
**Calendar model:** one maintainer working with an AI agent team (DeepSeek Harness), roughly
30–40 focused hours per week, with hardware available from M1 onward. Dates are *relative
weeks from v0.1.0*, not promises; a 20 % buffer is included per milestone.

Related: [Architecture](architecture/ARCHITECTURE.md) ·
[Protocol](architecture/PROTOCOL.md) · [ADRs](adr/) ·
[Research](research/) · [AI build process](research/ai-development-workflow.md).

---

## 1. Guiding principles

1. **Airtime is the product constraint.** Every feature is judged in seconds of channel time
   and has a UX for "too expensive" (see [capacity analysis](research/lora-capacity-and-media.md)).
2. **Text first, then media, then interop.** Do not build the bridge before the mesh works in
   the field. Do not build calls before forward secrecy.
3. **One protocol, proven by two implementations.** Rust host core and C++ firmware share
   golden vectors; changes require vectors and a firmware conformance note.
4. **Ship small, honest increments.** Each milestone has measurable exit criteria and a
   published "known limitations" list.
5. **No fabricated benchmarks.** Numbers in docs are either measured in the simulator/field and
   dated, or explicitly labelled as computed estimates.

---

## 2. Milestone overview

| Milestone | Theme | Target (weeks after v0.1.0) | Nature |
| --- | --- | --- | --- |
| **M0** | v0.1.0: spec, core, bridge, web, firmware, simulator, flashers, docs, Android scaffold | 0 (delivered as this milestone) | repository/CI |
| **v0.2** | Backbone mode: OBB/1 authenticated TCP relay, live control panel, installer + ops docs (ships alongside M1) | weeks 1–2 | infrastructure |
| **M1** | Field-ready text mesh: hardware bring-up, store-forward tuning, OTA, invites, airtime caps | weeks 1–8 | hardware + tuning |
| **M2** | Android GA + companion polish + background sync + media UX | weeks 9–20 | mobile product |
| **M3** | OMP/2: ratchets, optional directed routing, voice calls (gated) | weeks 21–34 | protocol |
| **M4** | NomadNet/Reticulum bridge + files + room pages | weeks 32–46 | interop |
| **M5** | Room servers, multi-channel radios, network map API, Meshtastic text gateway | weeks 47–60 | scale/ecosystem |

Dependencies: M1 feeds M2 (real hardware behaviour). M2 feeds M4 (clients need a stable message
model). M3 is mostly independent but gates calls. M4 must not start before M1 is stable, because
the gateway's OMP adapter depends on a behaving bridge/engine. M5 depends on M4's identity
registry and on M3's routing decision.

---

## 3. M0 — v0.1.0: the repository milestone (done)

**Goal:** a coherent, reviewable, buildable-as-far-as-the-environment-allows open-source
repository that another person can pick up.

### Deliverables (this milestone)

| Area | Contents | Status at v0.1.0 |
| --- | --- | --- |
| Specs | `ARCHITECTURE.md`, `PROTOCOL.md` (OMP/1), `COMPANION-API.md`, `WEBSOCKET-API.md` | **done** (normative, cross-linked) |
| Core | `crates/openmesh-core`: identity/crypto, wire codec, flooding/dedup, routing budget, resources ARQ, store-and-forward queue, services schemas, LoRa math, companion codec, IO-free engine | **delivered in M0** — engine/resources/companion implemented by the core workstream; golden vectors generated |
| Bridge | `crates/openmesh-bridge`: WS + REST API per spec, token auth, transports (sim/UDP/TCP/serial feature), redb store, attachments, demo mode | **delivered in M0** (demo + sim; real-radio paths behind feature flags) |
| CLI | `crates/openmesh-cli`: send/listen/announce/nodes/airtime | **delivered in M0** |
| Simulator | `crates/openmesh-sim`: virtual LoRa medium, loss/latency, N nodes, scenario assertions incl. airtime | **delivered in M0** (scenarios: chat, relay chain, lossy/retry, store-forward, image) |
| Web client | `apps/web`: React + Vite + Tailwind + MapLibre, chats/groups/map/nodes/contacts/settings, image compression, voice notes, demo mode, Vitest | **delivered in M0** (Tauri shell config present; desktop bundle is CI/human) |
| Desktop | Tauri 2 shell config in `apps/web/src-tauri` | **scaffolded** (not built on the build host: webkit deps absent) |
| Firmware | `firmware/` PlatformIO C++17: 4 boards (T1000-E, RAK4631 solar, SenseCAP Solar P1, Heltec Wireless Tracker), shared lib (frame codec, announce, dedup, relay, store, companion link, GPS/battery, region budgets) | **delivered in M0** as source + board configs; hardware flashing/field test is M1 |
| Flashers | `apps/flasher` static web flasher (esp-web-tools + UF2), `tools/flash` Python CLI | **delivered in M0** |
| Android | `apps/android` Compose scaffold: navigation, screens, companion transport (USB/BLE), framed client, repository/StateFlow, QR, foreground service, README + CI workflow | **scaffold delivered in M0**; not compiled locally (no SDK); GA is M2 |
| FFI | `crates/openmesh-ffi`: UniFFI-ready surface plan + buildable stub; `cargo build -p openmesh-ffi` passes | **delivered in M0** |
| Docs | ADRs 0001–0008, research notes (Reticulum/LXMF, Meshtastic vs MeshCore, NomadNet bridge, LoRa capacity, AI workflow), this roadmap, hardware notes | **delivered in M0** |
| CI | Rust build/test/clippy/fmt, web build/test, firmware vector tests (best effort), Android `assembleDebug`, markdown link check | **delivered in M0**; hardware jobs run in M1 |

### Exit criteria (v0.1.0)

* `cargo build --workspace` and `cargo test` pass where the environment allows; clippy clean.
* `cargo run -p openmesh-sim` passes all scenarios and exits non-zero on assertion failure.
* `cargo run -p openmesh-bridge -- --demo` serves `/api/health` and `/api/ws`.
* `pnpm --dir apps/web build` and `pnpm test` pass; UI runs in demo mode with no bridge.
* `cargo build -p openmesh-ffi` passes.
* `protocol/vectors/*.json` validate in Rust and in the firmware native test suite.
* Docs: every research note has sources; ADRs 0001–0008 link back to ARCHITECTURE §4; no broken
  relative links; Android README states honestly that it was not compiled locally.
* Known limitations published (no field test, no hardware validation, no security audit, Android
  unverified locally, Tauri unbuilt).

### Explicitly *not* in v0.1.0

* Forward secrecy, calls, directed routing (OMP/2).
* Reticulum/LXMF/NomadNet bridge (M4).
* Room servers / channel history (M5).
* Android feature parity / store release (M2).
* OTA firmware updates (M1).
* Multi-channel radio support (M5).

---

## 3b. v0.2 — Backbone mode (shipped with M1)

**Delivered:** OBB/1 authenticated TCP relay (`hello → challenge → HMAC(token) → welcome`), peer
table with live frame/byte/RTT accounting, kick, token rotation, allowlist, max peers and
store-and-forward toggle; a control panel at `/backbone` with a copy-button setup wizard; client
mode (`openmesh-bridge join`), `install-service --backbone`, a sha256-verified one-line installer,
hardened systemd units, Docker/compose, and deployment/TLS/monitoring guides.
See [INSTALL.md](INSTALL.md), [operations/backbone-deployment.md](operations/backbone-deployment.md),
[ADR 0009](adr/0009-backbone-mode.md). Wire protocol unchanged (OMP/1).

**Still open from this theme:** native TLS in OBB/2, per-peer identity instead of a shared token,
panel session hardening, multi-backbone auto-peering/federation (currently manual `join` links).

## 4. M1 — Field-ready text mesh (weeks 1–8)

**Goal:** two handhelds and a solar repeater survive a real week outdoors; store-and-forward and
duty-cycle behaviour are measured, not assumed.

### Deliverables

1. **Hardware bring-up** for all four boards: flash, radio TX/RX verified, GPS, battery/telemetry,
   companion link over USB and BLE (one board at a time, in the order: Heltec tracker → RAK4631
   → T1000-E → SenseCAP Solar P1).
2. **Field test kit:** two handhelds, one solar repeater, a duty-cycle/airtime logger (firmware
   counters + bridge stats export), a simple RSSI/range test mode, and a test plan
   (range, hops, announce loss, store-forward success, duty-cycle adherence).
3. **Store-and-forward tuning:** offer/want cadence, 16-id offer frames, quota per addressee,
   TTL vs flash budget, media envelope policy (text by default), and a UI "stored at relay /
   delivered" distinction end-to-end.
4. **Airtime caps per region/profile:** implement the budget model from
   [ADR 0007](adr/0007-media-pipeline.md) §2.3; add `region.duty_cycle_pct` handling and a
   pre-send estimate test matrix. Update PROTOCOL §9.4 if the model proves it.
5. **SNR-weighted relay delay** (Meshtastic lesson) in firmware/simulator; measure delivery rate
   and airtime at 10/25/50 nodes in the simulator and on 3 physical nodes.
6. **OTA updates** for nRF52 (UF2/DFU) and ESP32 (esp_https_ota / esp-web-tools), with rollback
   and signature/provenance; document the update ritual (backup identity first).
7. **Channel invites:** `openmesh://channel?...` carrying PSK + name + profile/region, QR in
   web/Android, paste import; tests for malformed invites.
8. **Companion hardening:** reconnect/backoff, frame CRC/oversize tests, BLE MTU edge cases,
   USB serial hotplug; a fake-radio harness for CI.
9. **Firmware conformance:** `pio test -e native` replaying all vectors in CI; flash/RAM
   headroom report per board; fuzz the C++ decoders.
10. **Security review v0:** external review of crypto composition, store-and-forward replay,
    companion trust boundary; publish findings and fixes (or "not audited" if no reviewer).

### Exit criteria

* 7-day field test with ≥ 95 % text delivery within TTL across 2 hops and ≥ 80 % store-forward
  success for a peer offline 24 h.
* Measured duty-cycle adherence ≤ region cap over 24 h (firmware counters), including announces.
* Simulator scenarios assert delivery/airtime at 10/25/50 nodes and 20 %/50 % loss; results are
  published with the version.
* OTA update succeeds from N-1 to N on at least one nRF52 and one ESP32 board without losing
  identity or messages.
* No P0 security finding from review v0; P1 findings fixed or documented.

### Risks

| Risk | Likelihood | Impact | Mitigation |
| --- | --- | --- | --- |
| Radio/board bring-up issues (LR1110 vs SX1262 quirks, antenna) | high | schedule | order boards early; keep a UART/console debug build; one board at a time |
| Store-and-forward flash exhaustion with media | medium | field reliability | text-only default; quotas; monitor eviction counters |
| Duty-cycle rules differ from our model | medium | legal/field failure | conservative 1 % default; local regulator review; config override |
| Field test weather/range worse than expected | medium | UX | log RSSI/SNR; adjust profiles; publish real numbers |
| Solo maintainer bandwidth | high | schedule | AI agents for code; human for hardware/field; cut scope (OTA can slip to M2) |

---

## 5. M2 — Android GA and companion polish (weeks 9–20)

**Goal:** the Android app is a first-class client, not a scaffold: connect a radio over USB or
BLE, chat/groups/contacts/QR/nodes/map/settings, send images and voice notes with airtime
estimates, run in the background.

### Deliverables

1. **Wire the Kotlin app to the real stack:** `openmesh-ffi` UniFFI bindings for identity,
   engine, events; `FramedCompanionClient` implementing COMPANION-API (SLIP + CRC + CBOR);
   repository/StateFlow with reconnection states.
2. **Feature parity checklist:** Chats/threads (delivery ticks), groups/channels, contacts + QR
   scan/import, nodes list, MapLibre/osmdroid map with positions, settings (profile/region/
   announce/store), image attach with client-side compression + estimate, voice note
   record/playback (Opus; Codec2 decision), notifications.
3. **Foreground service + background sync:** connection service with notification, message
   notifications with reply/mark-read actions, battery-friendly BLE/serial reconnect.
4. **Media UX:** pre-send airtime/allowance dialog, thumbnail-first flow, progress in chunks +
   airtime, retry states, "stored at relay" vs "delivered" honesty.
5. **Distribution:** F-Droid/APK release, signing, CI `assembleDebug`/`assembleRelease` artifacts,
   versioning, crash reporting opt-in (offline-friendly), reproducible builds where possible.
6. **Companion pairing key (M2):** authenticated `Hello`/`HelloAck` with a physical button
   confirmation; update COMPANION-API and the app.
7. **Bridge ↔ app coexistence:** a user may run both; define sync (message ids, read state,
   attachment cache) or explicitly support "phone talks to radio; bridge runs separately".
8. **Accessibility/UX pass:** dark theme, large text, RTL-safe layouts, no placeholder copy.

### Exit criteria

* Android app connects to a physical radio over USB and BLE, sends/receives text in both
  directions, survives airplane-mode/reconnect cycles, and passes a 30-minute background test
  with notifications.
* Image send (≤ profile cap) and voice note send/playback work end-to-end, with an accurate
  pre-send estimate displayed (within ±20 % of measured airtime on a bench test).
* Parity checklist signed off against the web client (documented exclusions allowed: e.g. map
  providers).
* F-Droid release or a signed APK with install instructions; no ANRs in a 30-minute monkey test.
* Pairing-key handshake works on both USB and BLE; a wrong key is rejected with a clear error.

### Risks

| Risk | Likelihood | Impact | Mitigation |
| --- | --- | --- | --- |
| BLE/serial flakiness across OEMs | high | product | per-OEM test matrix (Pixel + one budget device); robust reconnect; log buffer |
| Android background limits (Doze) | high | notifications | foreground service with `connectedDevice` type; exact wording in UI; fallback polling |
| UniFFI/JNI integration complexity | medium | schedule | keep FFI narrow; test on host first; use `uniffi` generated bindings |
| Codec2 audio not natively decodable | medium | feature | bundle decoder or restrict to desktop; decide early in M2 |
| Play store policy / AGPL | low | distribution | F-Droid first; document store policy |

---

## 6. M3 — OMP/2: ratchets, directed routing, voice calls (weeks 21–34)

**Goal:** forward secrecy, better scaling on dense/slow networks, and (gated) real-time calls
over suitable transports.

### Deliverables

1. **OMP/2 ratchets** per [ADR 0003](adr/0003-static-ecdh-then-ratchet.md): announced ratchet
   keys (preferred) or symmetric chain ratchet; key ring persistence, rotation, expiry,
   replay-safe epochs; announce extension field; golden vectors; migration path from OMP/1
   peers.
2. **Directed routing (optional)** using reserved `RouteRequest`/`RouteResponse`: returned-path
   or AODV-style, with flooding fallback; simulator model; a documented gate for enabling it
   ("flooding fails above X nodes / Y % loss in field data").
3. **Voice calls (conditional):** only if ratchets + a non-duty-limited transport path exist.
   Realistic scope: calls over Wi-Fi/TCP/UDP and `SHORT_FAST`/`SHORT_TURBO` at short range,
   Opus 8–16 kbps, PTT (half duplex) first, full duplex later. Do not promise calls on 1 %-duty
   LoRa. Reuse the LXST *lessons* (profiles, jitter buffers, no-receipt media), not its code
   (license restrictions — see [reticulum notes](research/reticulum-lxmf-notes.md) §4.4).
4. **Multi-device identity:** one identity on phone + desktop with message sync; requires a
   device-linking protocol and a decision on trust (each device holds the seed? delegated keys?).
   Could slip to M5 if it threatens ratchets/calls.
5. **Security review v1:** external review of OMP/2 changes, ratchet implementation, call
   handshake; publish findings.
6. **Spec version bump:** PROTO/version field changes, compatibility rules, migration notes.

### Exit criteria

* Ratchets: forward secrecy demonstrated by a test (captured messages cannot be decrypted after
  key deletion) and by a documented threat-model change; OMP/1 peers still interoperate or fail
  gracefully.
* Directed routing (if shipped): measurable improvement over flooding in the simulator and on a
  3-node/2-hop physical chain; flooding fallback verified.
* Calls (if shipped): 60-second PTT call at `SHORT_FAST` or Wi-Fi with < 5 % frame loss and
  acceptable latency, forward secrecy active, no battery/thermal regressions; otherwise a
  written decision to defer.
* External review v1 complete; no unfixed critical findings.

### Risks

| Risk | Likelihood | Impact | Mitigation |
| --- | --- | --- | --- |
| Ratchet complexity/bugs | high | security | implement the simplest viable ratchet; vectors, interop tests, external review; don't ship calls without it |
| Directed routing regressions | medium | reliability | keep flooding default; feature-gate; simulator-first |
| Calls are a rabbit hole (audio, echo, latency) | high | schedule | PTT only, fast transports only, explicit non-goal on duty-limited bands; can defer whole item to M5 |
| Spec/version migration pain | medium | ecosystem | version negotiation, dual-stack window, clear migration notes |
| Single maintainer + security review cost | medium | schedule | budget 2 weeks; schedule review early |

---

## 7. M4 — Reticulum/NomadNet bridge (weeks 32–46)

**Goal:** OpenMesh users can exchange DMs with LXMF users and browse NomadNet pages, through a
gateway, without putting Reticulum on the mesh.

Design: [nomadnet-bridge.md](research/nomadnet-bridge.md). Phases M4.0–M4.5, 3–4 months of
part-time work after M1 is stable.

### Deliverables

1. **M4.0 spike:** local Reticulum testnet (rnsd + NomadNet page node + lxmd propagation node),
   prove page fetch and LXMF send/receive from a helper process; record the license boundary.
2. **M4.1 identity registry:** shadow identities, bridge contacts, encrypted storage, onboarding
   UX (QR/paste), revocation.
3. **M4.2 DM bridge:** text both directions, delivery state mapping (never "stored = delivered"),
   ACK synthesis, dedup, rate limits, bridge badges in clients.
4. **M4.3 attachments:** capped re-encoding both ways, progress, explicit failure messages,
   propagation-size warnings.
5. **M4.4 page proxy + OpenMesh pages:** sanitised Micron subset parser, structured page model,
   web + Android renderers, read-only OpenMesh page namespace.
6. **M4.5 hardening:** threat-model tests, DoS limits, observability, docs, license review
   record.

### Exit criteria

* Bidirectional DM and 4 KiB attachment work between an OpenMesh node and an LXMF client over a
  local testnet; gateway restart mid-conversation loses no messages and creates no duplicates.
* Page browsing works from web and Android against a real NomadNet node; malformed/hostile page
  fuzzing finds no panics; response size/time limits enforced.
* Generated OpenMesh pages render in NomadNet and escape user content (no markup/escape
  injection).
* License review recorded; no Reticulum-licensed code in the release tree; gateway runs with the
  Python stack absent (bridge-only mode).
* Threat model and privacy notice published (gateway sees bridged plaintext; not E2E across
  protocols).

### Risks

| Risk | Likelihood | Impact | Mitigation |
| --- | --- | --- | --- |
| License interpretation (separate process/no linking) | medium | legal | lawyer review in M4.0; document; if unclear, fall back to a user-installed helper and no bundled integration |
| Reticulum API drift/experimental Rust ports | high | maintenance | pin versions; adapter isolation; integration tests; support Python reference first |
| Identity/trust UX (shadow identities, bridge contact confusion) | high | product | explicit badges, onboarding text, per-contact bridging toggles, revocation |
| Attachment size mismatch (LXMF 1 KB default) | high | feature disappointment | caps + explicit errors + page/file fallback; document |
| Scope creep into group/calls bridging | high | schedule | hard non-goal in M4; revisit in M5 research |

---

## 8. M5 — Room servers, multi-channel radios, ecosystem (weeks 47–60)

**Goal:** make the mesh useful for communities, not just pairs.

### Deliverables

1. **Room servers (M5):** channel history stored on infrastructure nodes; late-join sync with a
   timestamp/message-id cursor (MeshCore's room-server model is the simplest reference);
   `channel_history` resource kind; retention/quota policy; privacy (channel PSK holders only);
   UI for joining a room and reading history.
2. **Multi-channel radios:** one radio serving multiple channels simultaneously (frequency
   hopping or multiple radios in the bridge); channel priority and airtime arbitration; spec
   extension (frame format already supports channel ids; the radio config/companion API grows).
3. **Network map API:** aggregate anonymised node positions/links from opted-in gateways;
   a public map (like meshmap-style projects) with privacy controls; no user content.
4. **Meshtastic text/position gateway (community-driven, optional):** protobuf boundary, no
   media, no store-and-forward; see
   [comparison](research/meshtastic-vs-meshcore-vs-openmesh.md) §4.3. Requires a champion;
   not a core-team commitment.
5. **Federation/scale:** multiple regions interconnected via gateways; announce rate limiting
   and path/route decisions at the boundary; maybe OMP/2 directed routing becomes default in
   dense networks.
6. **Ecosystem:** F-Droid/desktop releases, packaging, docs site, contributor guides, hardware
   kits; a public testnet.

### Exit criteria

* A room server holds ≥ 1 000 messages for 30 days within flash/RAM budget; a late joiner syncs
  in < 60 s at `MEDIUM_FAST` (simulated/bench).
* Multi-channel radio works on one board with two channels and documented airtime arbitration.
* Network map has privacy review; opt-out works; no location leak beyond intent.
* If the Meshtastic gateway ships: bidirectional text with no false delivery claims; documented
  feature asymmetry.

### Risks

| Risk | Likelihood | Impact | Mitigation |
| --- | --- | --- | --- |
| Room-server storage/privacy complexity | high | schedule | start with text-only, bounded history, explicit retention UI |
| Multi-channel hardware limits | high | feasibility | prototype on ESP32-S3 with two radios before promising; fall back to bridge-side multi-radio |
| Map privacy/abuse | medium | trust | opt-in, aggregation, rate limits, no message metadata |
| Maintainer/bus factor | high | sustainability | document everything, keep CI green, recruit co-maintainers in M2–M3 |
| Scope explosion (calls, AI features, etc.) | high | focus | revisit roadmap quarterly; non-goals list is binding |

---

## 9. Calendar summary

| Weeks | Milestone | Key output |
| --- | --- | --- |
| 0 | M0 | v0.1.0 repository (this milestone) |
| 1–8 | M1 | field-tested text mesh, store-forward, OTA, invites, airtime caps |
| 9–20 | M2 | Android GA, companion polish, background sync, media UX |
| 21–34 | M3 | OMP/2 ratchets, directed routing, conditional calls |
| 32–46 | M4 | Reticulum/NomadNet bridge (overlaps M3 tail: different workstreams, same maintainer) |
| 47–60 | M5 | room servers, multi-channel, map API, optional Meshtastic gateway |

Realistic total: **~14 months** for a solo AI-assisted maintainer to reach M5, with M1–M2 being
the value-dense part (a working field mesh + a good Android app). If the maintainer can only do
10 h/week, multiply the calendar by ~3; if a second human joins for field testing, M1 shortens
by 2–3 weeks.

---

## 10. Cross-milestone risks and decisions

| Risk | Owner | Mitigation | Decision point |
| --- | --- | --- | --- |
| Media airtime is user-hostile | product | caps + pre-send estimate + thumbnails + Codec2 (ADR 0007) | end of M1 (field data) |
| Flooding saturates dense networks | protocol | TTL/priority/SNR delay, OMP/2 directed routing | end of M3 (sim + field gate) |
| No forward secrecy | security | OMP/2 ratchets; UI honesty; external review | M3 kickoff |
| Regulatory duty cycles | product/firmware | conservative 1 % default, per-region config, legal review | M1 (before EU field test) |
| Store-forward flash limits | firmware | text default, quotas, per-board budgets | M1 |
| Android unverified locally | mobile | CI `assembleDebug`, F-Droid, physical devices | M2 kickoff |
| Bridge license/trust | interop | separate process, lawyer review, badges | M4.0 |
| Single-maintainer bus factor | project | docs, CI, task board, recruit co-maintainers | continuous |
| AI-generated code review burden | project | acceptance commands, golden vectors, external review for security | every milestone |
| Spec drift | docs | generated tables/vectors, link checks, size-model tests | M1 |

---

## 11. Versioning

* **v0.1.x** — M0 fixes only (docs, tests, bug fixes; no wire changes).
* **v0.2** — end of M1: field-tested firmware 1.0, bridge 0.2, web 0.2; wire still OMP/1.
* **v0.3** — M2: Android GA, companion pairing, media UX; wire still OMP/1.
* **v1.0** — after M3: OMP/2 with ratchets; migration completed; external security review; this
  is the first "recommended for real use" release.
* **v1.1** — M4: bridge. **v1.2** — M5 features.

Wire changes require a `PROTO_VERSION` bump (3-bit field) and a dual-stack migration window of at
least 6 months for LoRa firmware, because reflashing field repeaters is slow and sometimes
impossible.

---

## 12. What would make us change this plan

1. **Field data shows flooding fails earlier than expected** (< 20 nodes): pull directed routing
   from M3 into M2, and make it default-on rather than optional.
2. **Media is barely used in field testing:** re-scope M2 media work toward text/position polish
   and store-forward reliability; keep media as an advanced feature.
3. **Android ecosystem demand is stronger than desktop:** move Android GA before Tauri packaging,
   and consider iOS via a shared Kotlin Multiplatform layer later.
4. **Reticulum ecosystem becomes the dominant client story:** prioritise M4 and consider a
   first-party NomadNet-compatible client (but never by compromising OMP/1's frame budget).
5. **A security review finds a structural issue in the static-ECDH design:** pull ratchets
   forward (M2), accept a wire version bump, and delay media/calls.
6. **Hardware supply (LR1110/SX1262 boards) becomes unreliable:** narrow the board matrix to the
   two most available and invest in the bridge/simulator instead.

---

## 13. Done in v0.1.0 vs planned (summary)

| Capability | v0.1.0 (M0) | Planned |
| --- | --- | --- |
| OMP/1 wire format, crypto, announces, flooding, resources, store-forward | delivered (spec + Rust core + C++ firmware sources + vectors) | tuning in M1 |
| Bridge WS/REST, attachments, demo, simulator | delivered | production hardening M1 |
| Web client (PWA) + demo mode | delivered | Tauri bundles in M1/M2; parity in M2 |
| Firmware for 4 boards | source + configs delivered | hardware bring-up, OTA, field test in M1 |
| Flashers (web + native) | delivered | release manifests/CI signing in M1 |
| Android app | Compose scaffold (not compiled locally) | GA in M2 |
| FFI/UniFFI | stub + plan builds | real bindings in M2 |
| Voice notes | Opus client path + Codec2 design decision | Codec2 client support M2 |
| Images | client compression + resources + caps | airtime-budget caps M1 |
| Forward secrecy / calls / directed routing | not present (documented) | M3 |
| Reticulum/NomadNet bridge | design + plan | M4 |
| Room servers / multi-channel / map API | not present | M5 |
| Security audit | not performed | M1 (v0), M3 (v1) |
| Field test | not performed | M1 |

If you are reading this after v0.1.0: check the release notes and `CHANGELOG.md` for what
actually shipped; this roadmap is the intent, and the repository's acceptance gates are the
truth.

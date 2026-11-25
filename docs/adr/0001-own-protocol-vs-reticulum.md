# ADR 0001 — Build our own OMP protocol instead of speaking Reticulum

* **Status:** accepted (v0.1.0)
* **Date:** 2026-11-25
* **Decision record:** MADR-style; summary row 1 in
  [ARCHITECTURE.md §4](../architecture/ARCHITECTURE.md#4-key-decisions)
* **Related:** [ADR 0002](0002-managed-flooding.md), [ADR 0003](0003-static-ecdh-then-ratchet.md),
  [ADR 0007](0007-media-pipeline.md), [Reticulum research notes](../research/reticulum-lxmf-notes.md),
  [NomadNet bridge design](../research/nomadnet-bridge.md)

## 1. Context

Reticulum is the most mature open, cryptography-first mesh stack in this space. It has a decade
of design work, strong primitives (X25519, Ed25519, HKDF, AES-256-CBC+HMAC), a real announce and
path-discovery system, links with forward secrecy, resources for arbitrary-size transfers, and
a client ecosystem (NomadNet, Sideband, Ratspeak, MeshChat). We studied it deeply before deciding
what OpenMesh should be.

OpenMesh's goals are narrower and different in three ways that turned out to be decisive:

1. **LoRa first, with a 240-byte frame budget.** Reticulum's packet MTU is 500 bytes, its link
   MDU 431 bytes, and its header is 19–35 bytes. Its interface model assumes a byte stream where
   a radio modem interface can fragment. Our firmware model is "one LoRa packet = one OMP
   frame", because an nRF52840 cannot afford an IP-style fragmentation layer and because
   packet-level ACKs are simpler for flooding relays.
2. **Media as a first-class, capped feature.** Reticulum gets media through LXST over links
   (real-time calls). OpenMesh wants images and voice *messages* that work through store-and-
   forward and flooding, with pre-send airtime estimates. That needs a resource layer designed
   for a 1-frame MTU and for duty-cycle regions, not a link-first design.
3. **Client quality as the product bet.** The recurring complaint about Reticulum clients is
   ergonomics. We want one React/Tauri client and a native Android client against one clean
   bridge API. Building that on Reticulum would mean either wrapping a Python daemon and
   inheriting its client model, or re-implementing Reticulum.

We also needed store-and-forward that works on a solar repeater with 1 MiB of flash: encrypted
envelopes, offer/want handshakes, TTLs. LXMF propagation nodes solve the same problem but assume
links and a much larger host.

### 1.1 What Reticulum costs on our medium

Measured with the Semtech formula (see [capacity analysis](../research/lora-capacity-and-media.md)):

| Operation | Reticulum | OpenMesh OMP/1 |
| --- | --- | --- |
| Announce | 167–200 B packet (447–519 ms at MF) | 186 B frame (488 ms at MF) |
| Cold link to a new peer | path request 51 B + announce + link request 83 B + link proof 115 B + RTT 99 B ≈ 0.6–2.3 s of airtime before data | none; static ECDH key derived from the peer's announced identity, one sealed frame |
| 100-char message | link packet (≈ 55 B envelope + payload) or encrypted packet | one 201 B frame (519 ms at MF) |
| 5 KiB image | link + resource; efficient but link setup amortised | manifest + 29 chunks + ACKs ≈ 18 s at MF |

The link/handshake machinery is excellent for interactive sessions and large transfers, and
ruinous for short messages on a shared LoRa channel. We could not find a way to get both
forward secrecy and one-frame DMs without either a prekey scheme (out of scope for v0.1) or
links (too expensive at chat scale).

### 1.2 The alternative we did not take

Fork Reticulum, add a LoRa-optimised profile, and contribute it upstream. This fails on three
counts: the Reticulum License is not OSI-open and is incompatible with our AGPL-3.0 project
(field-of-use and AI-training conditions; see [ADR 0008](0008-agpl-3-0-licensing.md)), upstream
controls the wire format and release cadence, and its client model is the thing we want to
change. A fork would be a permanent merge burden and a license violation.

## 2. Decision

**OpenMesh defines its own wire protocol, OMP/1, optimised for one-frame LoRa operation.** It
borrows design ideas (self-certifying identities, signed announces, managed flooding, resource
manifests and selective repeat, propagation-node store-and-forward) but shares no code and no
wire format with Reticulum.

**A bridge to Reticulum/LXMF/NomadNet is planned for M4** as a separate gateway process that
terminates both stacks, rather than making the mesh half-compatible. See
[nomadnet-bridge.md](../research/nomadnet-bridge.md).

### 2.1 What OMP/1 fixes for v0.1

* 22-byte header, 218-byte body, 240-byte frame.
* 8-byte self-certifying node ids; high-bit address-class rule distinguishing unicast from
  channels.
* Clear-text `msg_ref` (4 bytes) in the envelope so relays can index store-and-forward
  envelopes without decrypting (Reticulum uses destination hashes; we cannot afford 16-byte
  destination fields per chunk).
* Signed fixed binary announces (`152 + name_len` bytes) with Ed25519 and self-certifying ids.
* Static X25519 ECDH direct messages sealed with XChaCha20-Poly1305; channel PSKs with inner
  Ed25519 signatures.
* Managed flooding with `(src, seq)` dedup and TTL.
* Store-and-forward envelope queue with offer/want.
* Resources: manifest + 180-byte ciphertext chunks + selective-repeat ARQ.
* A companion API (framed, typed, CBOR) for apps and a WebSocket/REST bridge API for desktop.

### 2.2 What OMP deliberately does not have in v1

Links, path discovery, ratchets, forward secrecy, real-time calls, destination-based routing,
128-bit addresses, interface access codes, stamps. Each is either scheduled for OMP/2 (ratchets,
directed routing), provided by the M4 bridge (Reticulum interop), or rejected as not worth the
airtime (stamps, IFAC).

## 3. Consequences

### 3.1 Positive

* **One frame, one message.** A text is 201 bytes / 519 ms at `MEDIUM_FAST`; no handshake, no
  path lookup, no per-frame destination tables.
* **Firmware is small enough for nRF52840 and ESP32-S3.** No link/resource/ratchet/transport
  state machines on the MCU; the core engine is IO-free and testable.
* **Deterministic and simulatable.** The engine takes `now_ms` and bytes; the simulator runs
  thousands of virtual nodes in one process (a Reticulum-like stack is much harder to simulate
  because of interface/daemon coupling).
* **License-clean.** No Reticulum-licensed code anywhere in the tree.
* **Client API is ours.** The bridge WS/REST and companion API are designed for our UI, with
  typed events and attachment handling.

### 3.2 Negative

* **No forward secrecy in v1.** A compromised long-term key decrypts past direct messages. This
  is documented in PROTOCOL.md §17 and scheduled for OMP/2
  ([ADR 0003](0003-static-ecdh-then-ratchet.md)).
* **No Reticulum ecosystem access until M4.** OpenMesh nodes cannot talk to NomadNet/LXMF
  services until the gateway exists; the bridge is a separate milestone with its own risks.
* **Flooding does not scale indefinitely.** Managed flooding is fine at the scale we target
  (tens–hundreds of nodes per region) but will need directed routing (OMP/2) beyond that
  ([ADR 0002](0002-managed-flooding.md)).
* **We re-implemented solved problems.** Announce validation, resource ARQ, store-and-forward
  and crypto composition all exist in Reticulum; ours are simpler and less battle-tested.
* **Two ecosystems to explain.** Users and contributors must understand that OpenMesh is not
  Reticulum-compatible on the wire; the bridge is the interop story.

### 3.3 Mitigations

* Golden vectors and a C++/Rust golden-vector gate to keep the two OMP implementations honest.
* A size-model test to keep every frame variant within the MTU (a class of bug found during this
  work, see [ai-development-workflow.md](../research/ai-development-workflow.md) §6).
* The M4 gateway as the Reticulum on-ramp, with an explicit trust model (no E2E across
  protocols).
* Ratchets and directed routing are specified as OMP/2, not hand-waved.

## 4. Alternatives considered

| Alternative | Why rejected |
| --- | --- |
| Speak Reticulum on the wire | 500-byte MTU, 19–35-byte headers, link/handshake cost, no store-and-forward model suited to our repeaters, client model we want to replace; license prevents code reuse. |
| Fork Reticulum and optimise it for LoRa | License incompatibility, upstream control, permanent merge burden. |
| Adopt LXMF as the application layer and Reticulum as transport, only write clients | Would satisfy interop but inherits links/paths on the MCU; still no media caps or airtime honesty; store-and-forward requires a propagation node on a host. |
| Use Meshtastic as the transport and add media above it | Protobuf envelope and 239-byte payload leave no room for our media framing; GPL-3.0 firmware; no store-and-forward; no self-certifying IDs. |
| Wait for Reticulum to get a LoRa-friendly profile | Not under our control; the client/UX problem would remain. |
| **Own protocol + separate M4 bridge (chosen)** | Best fit for LoRa airtime, media, store-and-forward and client UX; interop via a gateway process; license-clean. |

## 5. Follow-ups

1. OMP/2: define ratchets and optional directed routing, and revisit a link-like session layer
   only if calls become a goal (see [ADR 0003](0003-static-ecdh-then-ratchet.md)).
2. M4: build and test the Reticulum gateway per
   [nomadnet-bridge.md](../research/nomadnet-bridge.md).
3. Add the size-model CI test so no future frame variant exceeds the MTU.
4. Re-read the Reticulum manual and the ports before OMP/2 design; the ecosystem moves.

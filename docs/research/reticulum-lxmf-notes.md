# Reticulum, LXMF and LXST — research notes for OpenMesh

**Status:** research note, not normative · **Last updated:** 2026-11-25
**Audience:** OpenMesh implementers deciding what to borrow for OMP/1, OMP/2 and the
M4 Reticulum bridge.

This note records what Reticulum, LXMF and LXST actually do, how the Ratspeak-derived Rust
ports are structured, and what OpenMesh should copy, adapt or deliberately reject. It is
written from primary sources: the Reticulum manual, the source of the reference-adjacent Rust
ports, and their READMEs. Everything that looks like a protocol constant was read from the
source or from the normative manual and is marked with its source.

Related documents:

* [Architecture](../architecture/ARCHITECTURE.md) §4, §10 — decisions this note informs.
* [Protocol spec](../architecture/PROTOCOL.md) — the OMP/1 alternative.
* [ADR 0001](../adr/0001-own-protocol-vs-reticulum.md) — own protocol vs Reticulum.
* [ADR 0004](../adr/0004-rust-core-cpp-firmware-golden-vectors.md) — Rust core + C++ firmware.
* [NomadNet bridge design](nomadnet-bridge.md) — M4 gateway built on the findings here.
* [LoRa capacity analysis](lora-capacity-and-media.md) — the airtime numbers cited here.

---

## 1. Sources and method

### 1.1 Public documentation fetched

| Source | URL | Used for |
| --- | --- | --- |
| Reticulum manual (index) | <https://reticulum.network/manual/> | manual structure, version 1.6.1 |
| Reticulum manual, "Understanding Reticulum" | <https://reticulum.network/manual/understanding.html> | destinations, announces, links, resources, wire format, crypto primitives, packet size examples |
| Reticulum manual, "Reticulum License" | <https://reticulum.network/manual/license.html> | exact license conditions |
| NomadNet repository README | <https://raw.githubusercontent.com/markqvist/NomadNet/master/README.md> | what NomadNet is, features, GPLv3 statement |
| NomadNet page server source | <https://raw.githubusercontent.com/markqvist/NomadNet/master/nomadnet/Node.py> | page/file request paths, access rules |
| Meshtastic overview | <https://meshtastic.org/docs/overview/> | comparison context (§4) |
| MeshCore packet format | <https://docs.meshcore.io/packet_format/> | comparison context (§4) |

### 1.2 Repository checkouts studied

All five repositories were cloned into `/tmp/research` for this report; a MeshCore checkout was
made separately for the protocol comparison. Commit identifiers as observed:

| Checkout | Commit | License |
| --- | --- | --- |
| `rsReticulum` | `f6d0d69` | Reticulum License |
| `rsLXMF` | `5c18a5f` | Reticulum License |
| `rsLXST` | `19ac89e` | LXST license (non-commercial, no derivatives) |
| `Ratspeak` | `4bc3012` | Reticulum License |
| `ratspeak-handheld` | `2e91b8e` | Reticulum License (bundled RNode is GPLv3) |
| `meshcore` (comparison only) | `75b04ef` | MIT |

The READMEs of `rsReticulum` and `rsLXMF` state that they track Python Reticulum 1.6.1 and LXMF
1.3.1 respectively, and that they were written *from the reference source, not from a
specification*. That matters for anyone tempted to port them: the code is the compatibility
specification, and the license forbids reuse (see §2.4).

### 1.3 What this note does not claim

* It does not claim to specify Reticulum. The authoritative description is the Python reference
  implementation plus the manual; the Rust ports are experimental and version-pinned.
* Airtime numbers for Reticulum sizes are computed with the same Semtech formula OpenMesh uses
  (`crates/openmesh-core/src/util.rs`) and are estimates, not measurements on hardware.
* Legal conclusions are engineering risk assessments. A license lawyer should confirm before any
  code is copied; the safe policy adopted by OpenMesh is simply "no code copied".

---

## 2. Reticulum in one page (and then in detail)

Reticulum is a cryptography-first, medium-agnostic networking stack for high-latency,
low-bandwidth links. Its unit of addressing is the **destination**, a 128-bit truncated hash.
Nodes learn destinations and their public keys through signed **announces**, discover multi-hop
**paths** through transport nodes, then either send single encrypted packets or establish
**links** that provide forward secrecy, reliability, MTU discovery, and **resources** for
arbitrary-size transfers.

Two properties are foundational and worth stating before any comparison:

1. **Nothing is an IP address.** A destination hash is derived from an application name and,
   for single destinations, from the owner's identity hash, so there is no port, route or
   registration authority. A destination can move anywhere in the network and announce again.
2. **Every packet to a single destination is encrypted** with a per-packet ephemeral ECDH key
   (X25519) and AES; links additionally derive per-link keys and support key ratchets. Transport
   nodes route ciphertext and can prove delivery without reading it.

### 2.1 Identity and destination hashing

From `rsReticulum/crates/rns-identity/src/identity.rs`, `destination.rs`, `name_hash.rs`, and
the manual's "Identities" and "Destination Naming" sections:

```
identity private key : 64 bytes (X25519 private 32 || Ed25519 seed 32)
identity public key  : 64 bytes (X25519 public 32 || Ed25519 public 32)
identity hash        : SHA-256(public key)[:16]
name hash            : SHA-256(full dotted app name)[:10]      e.g. "lxmf.delivery"
destination hash     : SINGLE/GROUP: SHA-256(name_hash || identity_hash)[:16]
                       PLAIN:        SHA-256(name_hash)[:16]
link id              : SHA-256(hashable part of LINKREQUEST)[:16]
ratchet id           : 10-byte ratchet public key derivation
```

Destination types:

| Type | Encryption | Multi-hop | Intended use |
| --- | --- | --- | --- |
| `SINGLE` | per-packet ECDH with the destination identity | yes | most application traffic, DMs |
| `PLAIN` | none | no (direct only) | public broadcast metadata, discovery |
| `GROUP` | symmetric AES-256 key | only after a link is established | group messaging |
| `LINK` | per-link ECDH-derived keys | yes | reliability, requests, resources, forward secrecy |

A link is not addressed by a long-term destination hash but by the 16-byte *link id*, a hash of
the link request packet. Transport nodes keep a link table so replies can find their way back to
the initiator through the same path — that is how bidirectional communication is achieved
without the initiator revealing a destination of its own.

### 2.2 Announces

An announce is a signed, unencrypted packet that teaches the network "this destination exists,
here is its public key". The wire layout, from `rns-identity/src/announce.rs`, is a fixed binary
layout, not CBOR:

```
public_key(64) || name_hash(10) || random_hash(10) || [ratchet(32)] || signature(64) || app_data
random_hash = random(5) || unix_seconds_be(5)          (40-bit ordering value)
signed data = dest_hash(16) || public_key || name_hash || random_hash || [ratchet] || app_data
signature   = Ed25519(identity.sign_sk, signed data)
```

The receive-side rules in the manual's "The Announce Mechanism in Detail" are worth
internalising even if we never implement Reticulum:

* if the exact announce was seen before, ignore it;
* record the ingress transport node and the hop count;
* stop forwarding after `m + 1` retransmissions (default `m = 128`);
* retransmit after a randomised delay, on interfaces that have announce bandwidth
  (default allocation: 2 % of interface capacity);
* prioritise announces whose hop count is lower (closer destinations) when queues are full;
* retry `r` times (default 1) if no neighbour rebroadcasts with a higher hop count;
* expire old path entries (default destination timeout 604 800 s = 7 days).

That is a sophisticated, bandwidth-budgeted gossip protocol. It exists because announces in a
large Reticulum network are background traffic that must not crowd out data. OpenMesh's managed
flood, by contrast, has no dedicated announce bandwidth budget; it relies on 30/60-minute
cadence and small frames. Our research note records the gap; ADR 0002 explains why v1 accepts
it.

### 2.3 Path discovery and transport nodes

Reticulum distinguishes **instances** (endpoints, `enable_transport = No`) from **transport
nodes** (infrastructure that forwards announces and data). Path discovery works with path
request/response packets; the manual lists example on-wire sizes of 51 bytes for a path request
and 167 bytes for an announce. Requests are rate-limited and tagged to suppress floods; the
Rust implementation's constants (`rns-transport/src/constants.rs`) give a sense of the tuning:

| Constant | Value | Meaning |
| --- | --- | --- |
| `PATHFINDER_M` | 128 | exclusive upper bound on on-wire hop count |
| `PATHFINDER_R` | 1 | announce retransmit retries |
| `PATHFINDER_G` | 5 s | grace before retry |
| `PATHFINDER_E` / `DESTINATION_TIMEOUT` | 604 800 s | path expiry (7 days) |
| `PATH_REQUEST_TIMEOUT` | 15 s | base recursive path discovery timeout |
| `LINK_TIMEOUT` | 900 s | transport link-table entry timeout |
| `JOB_INTERVAL_MS` | 250 | job loop period |
| `HASHLIST_MAXSIZE` | 1 000 000 | duplicate-packet hash list capacity |

For a LoRa network, path discovery is *chatty*: a path request is 51 bytes (181 ms at
`MEDIUM_FAST`, see the capacity note), often followed by an announce, a link request and a link
proof before application data flows. OpenMesh's managed flood has zero convergence time and no
path state: every node is a relay by default. That is the core trade-off captured in
[ADR 0001](../adr/0001-own-protocol-vs-reticulum.md) and why directed routing is deferred to
OMP/2.

### 2.4 Link establishment, cryptography and cost

Reticulum links are the most interesting machinery for OpenMesh because they solve reliability,
forward secrecy and large transfers in one abstraction. From the manual's "Link Establishment in
Detail" and `rsReticulum/crates/rns-link`:

```
LINKREQUEST  : destination hash, ephemeral X25519 public key LKi(32) + Ed25519 key EKi(32)
               link id = SHA-256(hashable part of the request)[:16]
LINKPROOF    : responder ephemeral public key LKr + Ed25519 signature of (link_id || LKr)
               by the destination's long-term signing key
key schedule : ECDH(my ephemeral, their ephemeral) -> HKDF-SHA256(..., salt = link_id)
               -> 32-byte HMAC key || 32-byte AES-256-CBC cipher key
encryption   : Fernet-style token: AES-256-CBC + PKCS7 + HMAC-SHA256 (IV 16 || HMAC 32 overhead)
forward secrecy : ephemeral ECDH per link; initiator anonymity is preserved (no identity sent)
```

Constants that matter for cost:

| Constant | Value (source) |
| --- | --- |
| packet MTU | 500 bytes (`rns-wire/constants.rs`) |
| header sizes | 19 bytes (header type 1) / 35 bytes (header type 2) |
| max data unit (MDU) | 464 bytes |
| link MDU | 431 bytes (`floor((500-1-19-48)/16)*16-1`) |
| single-packet encrypted MDU | 383 bytes |
| AEAD/token overhead | 48 bytes (16-byte IV + 32-byte HMAC) |
| link MTU signalling field | 21 usable bits, base 500 bytes |
| keepalive | default 360 s, min 5 s, max 360 s; stale at 2× keepalive |
| establishment timeout | 6 s per hop |
| accepted encryption modes | AES-256-CBC only; AES-128-CBC and AES-256-GCM are representable but rejected |

The manual lists measured on-wire packet sizes that make handshake cost concrete: link request
83 bytes, link proof 115 bytes, link RTT 99 bytes, link keepalive 20 bytes. Computed at
`MEDIUM_FAST` (SF9/250 kHz/CR 4/5, our formula in
[lora-capacity-and-media.md](lora-capacity-and-media.md)):

| Packet | Bytes | Airtime @ MEDIUM_FAST |
| --- | --- | --- |
| Path request | 51 | 181 ms |
| Announce (small) | 167 | 447 ms |
| Link request | 83 | 253 ms |
| Link proof | 115 | 325 ms |
| Link RTT | 99 | 294 ms |
| Keepalive | 20 | 110 ms |

So a cold Reticulum link costs at least ~0.6 s of *transmit* airtime (request + proof + RTT), on
top of any path discovery, before the first byte of application data. Every hop retransmits
each of those packets; on a 5-hop LoRa mesh the aggregate channel cost is several seconds. For a
40-byte chat message, that is two orders of magnitude more overhead than OpenMesh's single sealed
`Data` frame (201 bytes for a 100-character message, 519 ms at `MEDIUM_FAST`, one frame total).

This is why OpenMesh v1 uses static ECDH and one-frame DMs, accepts the loss of forward secrecy,
and schedules links/ratchets for OMP/2 — but it is also why a Reticulum *bridge* (not a
reimplementation) is the honest way to reach that ecosystem.

### 2.5 Resources

A resource is Reticulum's reliable, arbitrarily large transfer over an established link. The
implementation in `rsReticulum/crates/rns-protocol/src/resource.rs` shows the shape:

* The transfer is segmented; each segment is a set of parts of up to `MDU` (464) bytes.
* The receiver tracks parts with a **hashmap** (4-byte map hashes, `HASHMAP_STRIDE = 74` in the
  control packet) and requests missing parts.
* The window adapts to the measured rate: initial 4, minimum 2, maximum 75 for fast links,
  10 for slow, 4 for very slow (`WINDOW_INITIAL`, `WINDOW_MAX_FAST`, ...).
* Data may be compressed automatically with bzip2 (`AUTO_COMPRESS_MAX_SIZE` = 64 MiB);
  compression is signalled in the advertisement.
* Advertisements/part requests/proofs are distinct packet contexts; proofs are 96-byte explicit
  (packet hash + signature) or 64-byte implicit signatures.
* Retries: `MAX_RETRIES = 16`, `MAX_ADV_RETRIES = 4`, proof timeout 3×RTT, part timeout 4×RTT
  (2× after RTT measured); maximum efficient resource size ~1 MiB, hard maximum 128 MiB
  (`MAX_RESOURCE_SIZE`), `MAX_SEGMENTS = 128`.

OpenMesh's resource design is deliberately simpler: a manifest plus fixed-size ciphertext
chunks and a 32-bit selective-repeat bitmap, no per-part hashmap, no compression (the client
compresses images before handing them over), no segmentation negotiation. The reasons are
firmware memory (a hashmap of 74-byte entries per segment is expensive on an nRF52840) and the
fact that our payloads are already small (≤ 24 KiB) by policy. The *concept* — manifest first,
then chunks, receiver-driven selective repeat — is borrowed directly.

### 2.6 Ratchets, channels, IFAC, stamps

Three smaller Reticulum features inform OMP/2 and bridge design:

* **Ratchets.** A destination can include a 32-byte ratchet public key in its announce. Senders
  mix a `ratchet_id` into packet encryption so that compromise of the long-term key does not
  decrypt past traffic; receivers keep a ring of up to 512 keys, rotate at most every 1800 s,
  and expire after 30 days (`RATCHET_COUNT`, `RATCHET_INTERVAL`, `RATCHET_EXPIRY` in
  `rns-wire/constants.rs`). This is the closest analogue to the OMP/2 plan in
  [ADR 0003](../adr/0003-static-ecdh-then-ratchet.md).
* **Channels.** Reticulum has a link-oriented `Channel` abstraction for ordered, typed
  messages with backpressure — useful for interactive protocols (NomadNet page requests,
  telephony signalling). OpenMesh v1 has no link abstraction, so channels map poorly; the M4
  gateway must either use Reticulum's Channel API (reference implementation / rsReticulum) or
  plain request/response over links.
* **IFAC and stamps.** Interface Access Codes authenticate an entire interface by adding an
  Ed25519 signature (or truncated signature) to every packet, optionally combined with a
  passphrase-derived shared identity. Cryptographic stamps are proof-of-work attached to
  announces/messages for spam resistance. Both are optional, interface-local features;
  OpenMesh's equivalent is the channel PSK plus local rate limits. Do not build stamps into
  OMP/1 — they cost airtime and battery for a mesh where the dominant threat is accidental
  congestion, not spam.

### 2.7 Interfaces (why Reticulum is "medium agnostic" in practice)

The manual and the Rust README list a broad interface zoo: TCP client/server, UDP (unicast and
broadcast), IPv6 multicast AutoInterface, I2P, stdio pipe, Backbone (HDLC over TCP), serial,
KISS, AX.25 KISS, RNode over serial/TCP/BLE, Bluetooth Peer, Android USB-OTG, and Local
shared-instance sockets. Reticulum's transport layer sees all of them as packet links with
bitrate/mode metadata.

OpenMesh's [PROTOCOL.md](../architecture/PROTOCOL.md) §14 covers the five transports we actually
need (LoRa, USB, BLE, UDP, TCP) plus WebSocket for the bridge. The lesson from Reticulum is the
*shape*: a `Transport` trait with "emit frame / deliver frame / link metadata" and a bridge that
routes between them, rather than special-casing radios in the protocol engine. That shape is
already reflected in `openmesh-bridge`'s transport split.

---

## 3. LXMF — messaging on top of Reticulum

LXMF is the "Lightweight Extensible Message Format": signed messages between Reticulum
destinations, with optional store-and-forward through **propagation nodes**. It is the layer
OpenMesh's M4 bridge would interoperate with; it is not a mesh protocol of its own.

### 3.1 Message format

From `rsLXMF/crates/lxmf-core/src/constants.rs`, `message.rs` and the README's delivery model
section:

```
destination_hash(16) || source_hash(16) || signature(64) || payload
payload = MessagePack([timestamp, title, content, fields, [stamp]])
```

`title` and `content` are byte strings on the wire. The `fields` map is keyed by integer IDs and
carries structured extensions. Standard field IDs observed in the port:

| ID | Field | Notes |
| --- | --- | --- |
| `0x01` | embedded LXMs | nested messages |
| `0x02`/`0x03` | telemetry / telemetry stream | sensor apps |
| `0x05` | file attachments | list of name/data pairs |
| `0x06` | image | attachment specialisation |
| `0x07` | audio | codec mode byte + audio bytes |
| `0x08` | thread | full thread id hash |
| `0x0B` | group | group id |
| `0x0C` | ticket | propagation ticket |
| `0x30` | reply-to | full message hash |
| `0x31` | reply quote | quoted UTF-8 |
| `0x40` | reaction | message hash + content |
| `0x41` | comment | message hash |
| `0x42` | continuation | message hash |
| `0xFB`–`0xFF` | custom / non-specific / debug | app-defined |

`LXMF_OVERHEAD = 112` bytes (two 16-byte hashes + 64-byte signature + 8-byte timestamp +
8-byte MessagePack struct overhead) before any payload. A minimal packed message therefore
costs ~112 bytes plus title/content; at 500-byte Reticulum packets it fits opportunistically
only for short messages.

Audio field mode values include Codec2 450PWB/450/700C/1200/1300/1400/1600/2400/3200 and Opus
OGG/LBW/MBW/PTT/RT HDX/FDX/standard/HQ/broadcast/lossless — LXMF explicitly supports low-bitrate
voice *messages*, which validates OpenMesh's decision to ship voice notes before calls.

### 3.2 Delivery methods

LXMF has four delivery shapes, all described in the rsLXMF README:

| Method | Transport | Notes |
| --- | --- | --- |
| **Opportunistic** | single encrypted Reticulum packet | no link; delivery proof required; falls back to Direct if too large |
| **Direct** | Reticulum link + resource | one link per message batch; larger content |
| **Propagated** | deposit on a propagation node, retrieve later | store-and-forward, stamps/tickets, peer sync |
| **Paper** | local `lxm://` URI (QR) | no network at all; library support only in the port |

The propagation node is the piece that maps to OpenMesh repeaters. Observed endpoints in
`rsLXMF` (all Link requests to the `lxmf.propagation` SINGLE destination):

| Path | Purpose |
| --- | --- |
| `/offer` | peer offers transient IDs it can serve (message sync between propagation nodes) |
| `/get` | client lists and downloads transient IDs (`[None, None]` to list) |
| `/pn/get/stats` | node statistics |
| `/pn/peer/sync` | propagation-peer sync |

Stored messages are keyed by a 32-byte **transient id** (hash of the packed LXMF data), so a node
can deduplicate and a client can ask "which of these do I not have?". Peering is recursive up to
`autopeer_maxdepth` (default 6) and limited by `max_peers` (default 20). Storage has explicit
quotas: `delivery_transfer_max_accepted_size` defaults to 1 KB in the daemon (library default
1000 KB), propagation node storage limit and per-message accepted size are configurable, and
stamps (`stamp_cost`, `propagation_stamp_cost_target`) gate deposits under load.

This is a more elaborate store-and-forward than OMP/1's `StoreOffer`/`StoreWant` handshake, and
it shows the limits of the problem: quotas, stamp costs, peering depth, per-peer sync
throttles. OpenMesh v1 deliberately starts with the simplest correct version (72 h TTL, 64
envelopes, offer/want on announce) and treats LXMF's design as the roadmap for tuning.

### 3.3 What LXMF does *not* provide

* It is not a flooded mesh protocol. Without a path to the destination or a propagation node,
  nothing moves.
* It has no channel/group semantics of its own that map cleanly to OpenMesh channels: LXMF group
  fields ride inside signed messages; group key distribution is application-level.
* Message size is bounded by resource limits and recipient policy (default 1 KB accepted size in
  the daemon!), not by a mesh MTU. A 5 KiB image is already "large" in LXMF land.
* Real-time media is LXST's job, not LXMF's.

---

## 4. LXST — real-time media over Reticulum links

LXST ("Lightweight Extensible Signal Transport") is Mark Qvist's telephony/media protocol on top
of Reticulum Links. The Rust port `rsLXST` documents its shape precisely; it is the best public
description of what a Reticulum call actually costs.

### 4.1 Signal model

From `rsLXST/crates/lxst-core/src/shared/profile.rs` and the README:

* Signalling status values: `Busy`, `Rejected`, `Calling`, `Available`, `Ringing`, `Connecting`,
  `Established`.
* Call modes: `FullDuplex`, `HalfDuplex` (PTT).
* Packets are sent over an established Reticulum link with **no receipts** — media does not
  wait for per-packet proofs. Retransmission is not used for audio; late packets are dropped
  by the jitter buffer.
* Signalling is MessagePack over link packets; raw audio frames follow the selected profile.

### 4.2 Profiles and codecs

| Profile | Frame time | Codec | Channels | Frame rate ceiling |
| --- | --- | --- | --- | --- |
| `BandwidthUltraLow` (ULBW) | 400 ms | Codec2 700C | 1 | 0.7 kbps class |
| `BandwidthVeryLow` (VLBW) | 320 ms | Codec2 1600 | 1 | 1.6 kbps class |
| `BandwidthLow` (LBW) | 200 ms | Codec2 3200 | 1 | 3.2 kbps class |
| `QualityMedium` (MQ, default) | 60 ms | Opus VoiceMedium | 1 | 8 kbps |
| `QualityHigh` (HQ) | 60 ms | Opus VoiceHigh | 1 | 16 kbps |
| `QualityMax` (SHQ) | 60 ms | Opus VoiceMax | 2 | 32 kbps |
| `LatencyLow` (LL) | 20 ms | Opus VoiceMedium | 1 | 8 kbps |
| `LatencyUltraLow` (ULL) | 10 ms | Opus VoiceMedium | 1 | 8 kbps |

Jitter buffers: 2 frames for ULBW/VLBW/LBW/ULL, 3 for LL, 5 for MQ/HQ/SHQ. The default profile is
`QualityMedium`, i.e. an 8 kbps Opus stream.

**The airtime consequence is decisive for LoRa.** An 8 kbps stream must transmit ~1 000 bytes
per second *continuously*. Reticulum's link MDU is 431 bytes; with AES token overhead a
sustained call needs a medium that can carry roughly 10 kbps of payload in real time. Our
capacity calculations ([lora-capacity-and-media.md](lora-capacity-and-media.md)) give the
effective payload throughput of an OMP chunked transfer on each OpenMesh profile:

| Profile | Effective payload throughput (continuous TX) |
| --- | --- |
| `LONG_SLOW` | 0.12 kbps |
| `LONG_FAST` | 0.79 kbps |
| `MEDIUM_FAST` | 2.62 kbps |
| `SHORT_FAST` | 8.28 kbps |
| `SHORT_TURBO` | 16.55 kbps |

Even before duty-cycle limits, only `SHORT_TURBO` has headroom for an 8 kbps call, and
`SHORT_FAST` is marginal once retransmits and duplex gaps are counted. In EU868/AS923 at 1 %
duty cycle, the average budget is 10–20 B/s — calls are physically impossible. This is the
technical reason Reticulum/LXST calls are a Wi-Fi/TCP/short-range-fast-medium feature, and the
reason OpenMesh v1 ships voice *messages* with airtime estimates instead of calls. It also
confirms the architecture's plan to gate calls on OMP/2 ratchets and on non-duty-limited
transports.

### 4.3 Embedded boundary

`rsLXST` contains `lxst-embedded` (a bounded `no_std` framing/session/PTT layer) and
`lxst-codec2` (allocator-free Codec2 700C/1600/3200). The handheld firmware uses both through a
Rust staticlib. This is important evidence that a no_std Rust media stack for MCUs is feasible —
but we cannot use these crates (next section).

### 4.4 License: we cannot reuse rsLXST (or the Reticulum-license repos)

Two different licenses are in play, and both prevent code reuse in an AGPL-3.0 project:

1. **Reticulum License** (`rsReticulum`, `rsLXMF`, `Ratspeak`, `ratspeak-handheld`, and the
   reference Reticulum/LXMF). The text is MIT-like but adds two conditions: the software may not
   be used in systems whose functions include purposefully harming humans, and it may not be
   used in the creation of AI/ML/language-model training datasets. These are field-of-use
   restrictions; they conflict with the Open Source Definition and cannot be represented as
   additional terms in an AGPL-3.0 distribution. We may read these repositories to understand
   the protocol; we may not copy code from them into OpenMesh.
2. **LXST license** (`rsLXST`). The `LICENSE` file states, verbatim: *"This implementation of
   LXST is derived from the LXST reference implementation, and used under license for Ratspeak,
   for non-commercial use only. No derivatives or use outside Ratspeak is permitted."* This is
   not open source at all. **OpenMesh cannot reuse rsLXST, not even its Codec2 crate**, and
   should not port LXST code. If voice calls are ever shipped, they need either an independent
   implementation of the LXST *protocol* from public documentation (wire compatibility) or a
   different design, plus a license review.

This is why [ADR 0008](../adr/0008-agpl-3-0-licensing.md) makes third-party policy explicit and
why every research doc in this directory is a *design reference*, never a source of code.

---

## 5. How the Ratspeak Rust ports are structured

The ports exist because a Tauri app wanted a Rust-native (not Python) Reticulum stack with
Android/iOS embedding and a Rust protocol core on MCUs. Their crate decomposition is the most
useful thing to copy — as a pattern, not as code.

### 5.1 rsReticulum (11 crates + tools)

| Crate | Responsibility (from `Cargo.toml` members and README) |
| --- | --- |
| `rns-crypto` | X25519, Ed25519, HKDF, HMAC, SHA, AES-CBC, PKCS7, Fernet-style token |
| `rns-wire` | packets, headers, flags, contexts, link signalling, packet hashes, receipts |
| `rns-identity` | identities, destinations, announces, ratchets, known destinations, IFAC, stamps |
| `rns-transport` | announce dispatch/validation, path discovery, dedup, blackholes, rate tables, shared instance |
| `rns-link` | link establishment, encryption, keepalive, MTU discovery, key derivation, requests, stamps |
| `rns-protocol` | resources, channels, channel messages, compression, stream data, rnsh/buffer |
| `rns-interface` | TCP/UDP/Auto/I2P/pipe/backbone/serial/KISS/RNode/BLE/Android USB |
| `rns-runtime` | daemon runtime: link manager, destination runtime, resource spool, RPC, probe |
| `rns-tools` | `rnsd-rs`, `rnstatus-rs`, `rnpath-rs`, `rnid-rs`, `rnprobe-rs`, `rncp-rs`, ... |
| `rns-ratkey` | hardware-backed identity/attestation backends (experimental) |
| `rns-git` | `rngit` repository node, NomadNet page generation, Micron conversion |

The layering is strict and *isomorphic to the Python package*: wire → identity → transport →
link → protocol → runtime → interfaces/tools. The port also persists Python-compatible SQLite
state (`known_destinations.db`, `cache/announces.db`) so the two implementations can share
state. That is a level of compatibility work that only makes sense if your goal is "be
Reticulum"; OpenMesh's goal is different, so we only borrow the layering discipline.

### 5.2 rsLXMF (2 crates)

`lxmf-core` (message codec, router, propagation node/client, stamps, tickets, state store) and
`lxmf-tools` (`lxmd-rs` daemon). It depends on the sibling `rsReticulum` crates by path, not on
crates.io: the ports are a single source tree in practice, pinned to the same Reticulum release.
That is a warning for anyone planning to embed a *published* Rust Reticulum library: none is
stable or independently packaged, which is why the M4 bridge plan (§6, and
[nomadnet-bridge.md](nomadnet-bridge.md)) assumes a sidecar process, not a linked library.

### 5.3 rsLXST (5 crates)

| Crate | Responsibility |
| --- | --- |
| `lxst-codec2` | allocation-free native Codec2 700C/1600/3200 |
| `lxst-embedded` | bounded `no_std` framing, session/PTT policy, Codec2 integration |
| `lxst-core` | constants, profiles, signalling, MessagePack packets, raw audio, Opus, jitter buffers |
| `lxst-rns` | link-packet boundary for signalling and media |
| `lxst-telephony` | call state machine, service layer, discovery, timeouts, teardown |

The host app still owns microphone/speaker, permissions, audio sessions and UI. This split
(signal logic in Rust, I/O in the host) is exactly the split OpenMesh wants between
`openmesh-core` and app/firmware; study it as architecture, not code.

### 5.4 Ratspeak (Tauri 2 desktop + mobile client)

Workspace members: `ratspeak-core` (config, delivery types, emitter, names, notification, radio),
`ratspeak-db` (SQLite/r2d2, admission, static nodes), `ratspeak-runtime` (announce handling,
channels, hosted propagation, inbound admission, link accounting, image attachments),
`ratspeak-tauri` (commands, diagnostics, invoke gate, notifier, process, window prefs),
`rsnode-update` (delta update). The Tauri shell lives in `src-tauri/` with per-platform configs
(`tauri.android.conf.json`, `tauri.ios.conf.json`, ...), a vendored `tao`/`wry`/`tauri-runtime-wry`
patch for Android Activity lifetime bugs, and a `capabilities/` directory.

Relevant lessons for OpenMesh's clients:

* A Rust core + thin platform shells works for Linux/macOS/Windows *and* Android/iOS, but Tauri
  still needed vendored patches for mobile — "one codebase everywhere" has a cost.
* Images are compressed client-side (`image` crate with jpeg/png/webp) and attachments are
  stored as files, with message metadata referencing them: the same model as OpenMesh's
  `POST /api/attachments` + attachment id.
* Voice *messages* (recorded Opus in OGG) are separate from calls (LXST); the app ships both as
  distinct features. OpenMesh should do the same and not conflate them.
* The app stores its own SQLite database and runs a hosted propagation node option; OpenMesh's
  bridge + redb mirrors this but with a smaller feature surface.

### 5.5 ratspeak-handheld (PlatformIO firmware with a Rust protocol core)

This is the closest precedent for [ADR 0004](../adr/0004-rust-core-cpp-firmware-golden-vectors.md),
and it validates the "Rust core + C++ firmware" split — with an important difference: the
handheld core is a *Rust staticlib compiled to xtensa*, not a reimplementation in C++.

Observed structure:

* `platformio.ini` + `boards/` + `partitions/` + `launcher/` + `src/` (Arduino C++17): display,
  input, power, storage, audio, board drivers. Libraries pinned (ArduinoJson, LovyanGFX/LVGL,
  M5GFX/M5Unified, RadioLib is *not* used here — the radio is driven by RNode-compatible code).
* `protocol/` is a separate Cargo project (`ratspeak-handheld-protocol`) with
  `crate-type = ["staticlib", "rlib"]`, `no_std` firmware profile, `panic = "abort"`,
  and features `profile-small` / `profile-micro` selecting exactly one monomorphised node type.
* Rust dependencies are the *embedded* ports `rns-lite-core` (rsReticulumLite) and
  `lxmf-lite-core` (rsLXMFLite), plus `lxst-embedded` / `lxst-codec2`. The host-facing
  `rsReticulum` crates are **not** used on the MCU: the Lite crates are `no_std` re-packagings
  of the same protocol logic for constrained targets.
* `protocol/include/ratspeak_protocol.h` is the C ABI: opaque context, caller-owned buffers
  (transport tables sized by `transport_size/align`, ratchet persistence blobs up to ~2.7 KB),
  versioned status codes, explicit "no allocation freed by C / no C allocation freed by Rust"
  contract, panic isolation on host builds (`catch_unwind`), abort+watchdog on firmware.
* `protocol/build-xtensa.sh` rebuilds prebuilt `libratspeak_protocol.a` for the ESP32-S3 via the
  espup `esp` toolchain, `-Zbuild-std=core,alloc`, `--locked --offline`, a fixed stage directory
  with `--remap-path-prefix` to make builds reproducible, a source fingerprint,
  `release_identity.py` toolchain pinning, and a `PROVENANCE.txt` recording source commits and
  artifact SHA-256s.
* Targets are ESP32-S3 only (T-Deck Plus, T-Pager, Cardputer Adv, ThinkNode M9). nRF52 is not
  covered by this firmware; that is consistent with its display-centric hardware.

Lessons for OpenMesh firmware:

1. The split works, but the boundary must be *bounded and boring*: caller-owned buffers, fixed
   capacities, explicit error codes, no unwinding across FFI, no hidden allocation.
2. Prebuilt staticlibs solve the "Arduino users don't have Rust" problem, but reproducibility
   (pinned toolchain, remapped paths, provenance file) is what makes them reviewable.
3. A single monomorphised profile per artifact (`small`/`micro`) is how they fit the core on
   different boards. OpenMesh's firmware is C++ with a HAL instead, so this specific problem
   does not arise; but the "one code path, board configs not forks" principle is the same.
4. Their C ABI is a good template for OpenMesh's future `openmesh-ffi` (see §7): opaque handles,
   event callbacks/polling, caller-owned buffers for anything large.

---

## 6. What OpenMesh borrows, adapts and rejects

The table below is the core of this note. "Borrow" means the idea is adopted; "adapt" means the
idea is adopted with a different mechanism; "reject" means we have a concrete technical reason
not to do it, not that it is bad.

| Reticulum/LXMF/LXST idea | Verdict | Reasoning |
| --- | --- | --- |
| 128-bit destination hashes | **Reject** | 8-byte node ids are enough for a mesh of thousands and save 8 bytes per frame header (22 vs 19/35-byte headers). With the high-bit address-class rule, 57 bits remain; collision probability at 100k nodes is ~10⁻⁷. Reticulum's extra bytes buy an internet-scale address space we do not need on one channel. |
| 500-byte MTU / 431-byte link MDU | **Reject for LoRa OMP/1, revisit for OMP/2** | SX126x maximum LoRa payload is 255 bytes; 500-byte Reticulum packets are fragmented by the RNode-style interface into multiple LoRa transmissions, and our firmware targets need frames that map 1:1 to radio packets. 240 bytes keeps a 1-frame MTU with headroom. If OMP/2 adds links, a 255-byte frame plus a link layer could approach Reticulum's MDU without IP-style fragmentation. |
| Announce as signed fixed binary | **Borrow (already adopted)** | We independently discovered a 72-byte identity + 64-byte signature + CBOR does not fit 218 bytes; the spec now uses a fixed binary announce (152 + name_len ≤ 184 bytes). Reticulum's exactly this design, and for the same reasons: fixed offsets are firmware-friendly and the identity is self-certifying. |
| Announce/pathfinder tuning (bandwidth budget, hop prioritisation, hop limit 128) | **Adapt** | Our announces piggyback on the flooding layer with TTL 8 and a 5-minute floor. The *idea* of capping announce bandwidth share is right; our implementation does it with cadence + jitter + TTL rather than a per-interface announce queue. OMP/2 should adopt a priority queue if announce loss becomes measurable. |
| Path discovery / transport nodes | **Reject for OMP/1, adapt for OMP/2** | Managed flooding has zero convergence time and no path state, which is the right call for single-channel LoRa at Meshtastic scale. If OMP/2 adds directed routing, Reticulum's path request/response + link table is the reference design, including the 7-day expiry and 15 s discovery budget. |
| Links with ephemeral ECDH + HKDF + AES-CBC/HMAC | **Reject for v1; adapt for OMP/2 calls** | The handshake costs ≥ 0.6 s of airtime (request 253 ms + proof 325 ms + RTT 294 ms at `MEDIUM_FAST`) plus path discovery, and relay forwarding multiplies it per hop. One-frame static-ECDH DMs are 2+ orders of magnitude cheaper for chat. Forward secrecy and a link abstraction *are* the right foundation for calls, so OMP/2 ratchets + a lightweight session layer should borrow the key schedule ideas, not the Reticulum packet format. |
| Resource: manifest + parts + adaptive window + hashmap + bz2 | **Adapt** | We keep manifest + selective-repeat ARQ + receiver-driven ack, but use fixed 180-byte chunks and a 32-bit bitmap instead of a per-part hashmap, and no automatic compression (clients compress first). This is a memory/complexity decision for nRF52840-class firmware; Reticulum's window sizing (4→75) is worth revisiting if we support links. |
| Ratchets (32-byte, 512-ring, 30-min rotation) | **Adapt, OMP/2** | Correct shape for forward secrecy. Our static ECDH keys are derived from one seed; adding a ratchet means an extra announce field and a key ring. The Reticulum constants are a sensible starting point; OMP/2 should also consider PQ ratchets, which Reticulum has reserved but not shipped. |
| LXMF propagation nodes (offer/get, transient ids, stamps, quotas) | **Borrow selectively, M4** | Our `StoreOffer`/`StoreWant` is the minimal version. LXMF's transient-id model (hash of packed message), per-peer sync and quotas are the natural next step for store-and-forward tuning (M1). Stamps are not worth it in v1. |
| LXMF signed message envelope (`dest || source || sig || MessagePack`) | **Adapt, M4 bridge only** | Inside OMP we have our own envelope; at the bridge we must construct/parse LXMF exactly. The important design detail is that LXMF signs the whole packed message, so the bridge cannot "re-sign" without access to the user's identity — identity mapping is the hard part (see [nomadnet-bridge.md](nomadnet-bridge.md)). |
| LXST real-time calls | **Reject for v1, revisit for OMP/2** | Calls need continuous throughput: only the fast presets are physically able to carry 8 kbps, and duty-limited regions cannot at all. Voice *messages* are the right v1 product. Also, rsLXST is non-commercial/no-derivatives; any future implementation must be independent. |
| Medium-agnostic transport trait + bridge between interfaces | **Borrow** | Reticulum's interface abstraction is good engineering. Our bridge already has a transport abstraction; keep it generic and put policy in the daemon, not the protocol engine. |
| Tauri + Rust shared core for desktop/mobile | **Adapt** | Same shape (Rust core, thin UI), different UI stack (web-first React + Tauri, native Compose for Android). Reticulum's ports show the approach is viable on mobile but requires platform-specific patches; budget for that. |
| Rust protocol core compiled as a C ABI staticlib for firmware | **Adapt** | Strong precedent (ratspeak-handheld). OpenMesh chose C++ firmware + Rust host because Arduino C++ radio HALs are mature and the team wants firmware testable in the same language as the HAL. The C ABI discipline from the handheld is the part to copy, for the future `openmesh-ffi` and for firmware test hooks. |
| Reticulum License / LXST license code | **Reject (no code reuse)** | Field-of-use and AI-training restrictions (Reticulum) and non-commercial/no-derivatives (LXST) are incompatible with an AGPL-3.0 project. Read-only research only. |

### 6.1 MTU: the single most consequential difference

Reticulum's 500-byte packets are not a mistake; they are tuned for *its* interface model, where
an interface is a byte stream and the stack can fragment/aggregate as needed (RNode interface
does exactly this). OpenMesh's interface model is "one LoRa packet = one frame", because the
firmware must not implement an IP-style fragmentation layer on an MCU, and because packet-level
acknowledgement is simpler to reason about for flooding.

The consequence: every byte of header and crypto overhead matters ~2× more for us. Our 22-byte
header + 33-byte envelope overhead + 64-byte signature where present is already tight: a
100-character DM frame is 201 bytes (519 ms at `MEDIUM_FAST`), and the channel-message schema
needed a redesign to fit ~60 characters per frame (verified during this research; PROTOCOL.md
§7.3). Reticulum's 19-byte header + 48-byte token overhead on a 500-byte packet hides this
pressure; we cannot.

### 6.2 Handshake cost: why "just use links" does not work on LoRa

It is tempting to say "Reticulum's links are better, let's have links". Do the arithmetic first:

* Chat message via static ECDH: 1 frame (201 bytes) + 1 ACK (120 bytes) = 855 ms of airtime at
  `MEDIUM_FAST`, all of it useful payload.
* Chat message via a cold link: path request 181 ms + announce 447 ms (if path unknown) + link
  request 253 ms + proof 325 ms + RTT 294 ms + encrypted data frame ~550 ms + proof/ack
  ~300 ms ≈ 2.3 s of airtime, with several frames that every relay must forward too.
* A 10-node mesh forwarding a flooded announce costs 10 × 447 ms = 4.5 s of aggregate airtime;
  every node's 1 % EU868 budget is only 36 s/hour.

Links pay off when you send many bytes or need forward secrecy; they are ruinous for short
messages. OpenMesh made the right v1 call; the M4 bridge is how we participate in the link-based
world without imposing it on the mesh.

### 6.3 MCU viability

The Reticulum ecosystem's own answer is instructive: the handheld firmware does not run
Reticulum proper. It runs **rsReticulumLite/rsLXMFLite** — `no_std`, bounded, single-profile
builds of the same logic with caller-owned buffers, and even then it is an ESP32-S3 with PSRAM
for the "small" profile and internal RAM for the "micro" profile. A full Reticulum instance with
links, resources, path discovery, ratchets and SQLite state is a Linux-class program; a
firmware-grade subset is a significant engineering project by itself.

OpenMesh's firmware, by contrast, is C++ relay/announce/store with no path state, no links, no
ratchets, no resource hashmap — a much smaller surface. That is the main reason the four-board
support is feasible. It also means "run Reticulum on the repeater" is not on the roadmap: a
gateway (M4) runs on a Linux/Android-class host next to a radio.

### 6.4 Store-and-forward

Both ecosystems converge on the same shape: ciphertext envelopes, addressed to a destination,
offered to the addressee when it reappears, with quotas and TTLs. LXMF adds transient ids,
peering and stamps. OpenMesh's offer/want handshake is correct but underspecified for scale:
the `StoreOffer` body carries up to 16 clear-text msg_refs, so a relay with 64 envelopes offers
over 4 frames; there is no per-peer "still missing" bitmap beyond the want list, and no quota
negotiation. The M1 tuning work should borrow LXMF's transient-id-based exchange and per-peer
sync throttles — see [ROADMAP.md](../ROADMAP.md).

Media in store-and-forward is a trap: a 12 KiB image is ~69 resource frames (14.8 KiB of
ciphertext including headers/overhead) per addressee. At the default 64-envelope capacity, a
relay full of media would need ~1 MB of flash just for the queue — half the nRF52840's 1 MB.
Recommendation recorded in [lora-capacity-and-media.md](lora-capacity-and-media.md): store text
and small resources only, cap media envelopes per addressee, and say "text store-and-forward"
in the UI.

---

## 7. Implications for OpenMesh crates and APIs

This section is deliberately concrete; it feeds the ADRs and the M4 bridge design.

### 7.1 `openmesh-core`

* Keep the engine IO-free and deterministic (already the case). Reticulum's runtime
  demonstrates how much complexity lives above the protocol layer (link manager, resource
  spool, RPC); keeping that out of the core is what makes the simulator and fuzzing feasible.
* Add the concept of a **bounded work budget** to resource send/receive (chunks per tick), the
  way Reticulum limits processing per job-loop iteration. This is what prevents a large transfer
  from starving announces/ACKs on an MCU.
* When OMP/2 links land, reuse the *key schedule shape* (ephemeral ECDH → HKDF with link/session
  id as info/salt → separate MAC and cipher keys) but not AES-CBC: XChaCha20-Poly1305 is simpler,
  misuse-resistant, and already implemented in the core.
* Keep announce as fixed binary (now in the spec); document the layout in the golden vectors so
  C++/Rust cannot drift.

### 7.2 `openmesh-bridge` (and M4 gateway)

* Model the Reticulum side as an **external process** (`rnsd-rs` or Python `rnsd`) and speak to it
  over its shared-instance/RPC socket or over TCP interface, rather than linking the
  experimental Rust crates. Reasons: license (no code reuse), API stability, and the
  single-source-tree dependency of rsReticulum/rsLXMF.
* Translate at the envelope boundary only: OMP direct message ↔ LXMF direct/opportunistic
  message, OMP channel message ↔ application-defined LXMF group field, OMP attachment ↔ LXMF
  field 0x05/0x06 with a resource. See [nomadnet-bridge.md](nomadnet-bridge.md) for the mapping
  table and what cannot survive.
* Store the bridge's own identity separately from the user's node identity, and make the
  identity-mapping trust model explicit in the UI ("messages sent through this bridge are
  signed by the bridge").

### 7.3 `openmesh-ffi` and mobile

* Follow the handheld C ABI discipline: opaque engine handle, explicit capacity errors,
  caller-owned buffers for frame/attachment data, no panic across the boundary, versioned status
  codes, and a single-threaded assumption per handle (callers serialise).
* Expose an event stream rather than callbacks that re-enter Rust: the Kotlin side can poll
  `next_event()` and translate to Flow. This matches how the companion transport already works
  (read loop → events).
* Keep the UniFFI surface deliberately narrow: create engine from seed, set config, send text,
  ingest frame, list nodes/messages, resource progress events. Anything that needs a callback
  into platform code (notifications, audio) stays in Kotlin/Swift.

### 7.4 Testing

* Reticulum's Python↔Rust interop tests (live interop behind `#[ignore]`) are a good model for
  our M4 bridge: run a local Reticulum testnet (two `rnsd` instances + `nomadnet` + a Python LXMF
  client) and assert byte-level interop for announces, opportunistic messages and propagation.
* Golden vectors are still the right cross-language gate for OMP; the M4 bridge should add
  captured Reticulum packet fixtures (pcap-style hex dumps) with explicit provenance.

---

## 8. Open questions

1. **Ratchet vs prekey bundles.** Reticulum's ratchet design assumes announces carry ratchets.
   For OMP/2, is a per-peer ratchet ring (Reticulum-style) or a signed prekey bundle
   (Signal-style) a better fit for intermittently connected LoRa peers? The Reticulum approach
   avoids a round trip; Signal-style is better understood and easier to audit.
2. **Link-less forward secrecy.** Can we get some form of forward secrecy without links, e.g.
   per-message ephemeral keys for the first message to a peer plus ratcheting after? That would
   preserve the one-frame DM advantage while removing the "long-term key decrypts history"
   problem earlier than OMP/2.
3. **NomadNet page subset.** Which Micron features are worth implementing server-side versus
   rendering to a plain-text subset? See [nomadnet-bridge.md](nomadnet-bridge.md) §6.
4. **Bridge identity model.** Should an OpenMesh node that uses a gateway expose the gateway as
   its Reticulum destination (gateway-signed), or should the gateway only forward already-signed
   LXMF messages it cannot create? The latter is simpler and more honest, but requires the user
   to have a Reticulum identity.
5. **Duty-cycle-aware media caps.** Reticulum does not solve this; the LXST profiles are about
   codecs, not regulation. OpenMesh's per-profile caps need a per-region airtime budget model
   (M1) — see [lora-capacity-and-media.md](lora-capacity-and-media.md) §7.

---

## 9. Reading list

Primary sources, in the order a new contributor should read them:

1. Reticulum manual, "What is Reticulum?" and "Understanding Reticulum":
   <https://reticulum.network/manual/understanding.html>
2. Reticulum manual wire format and cryptographic primitives (same page, "Protocol Specifics").
3. Reticulum License: <https://reticulum.network/manual/license.html>
4. NomadNet README: <https://raw.githubusercontent.com/markqvist/NomadNet/master/README.md>
5. rsReticulum README + `crates/rns-wire/src/constants.rs` (local checkout `f6d0d69`).
6. rsLXMF README + `crates/lxmf-core/src/constants.rs` / `message.rs` (local checkout `5c18a5f`).
7. rsLXST README + `crates/lxst-core/src/shared/profile.rs` (local checkout `19ac89e`).
8. ratspeak-handheld README + `protocol/include/ratspeak_protocol.h` +
   `protocol/build-xtensa.sh` (local checkout `2e91b8e`).
9. Ratspeak README + root `Cargo.toml` (local checkout `4bc3012`).
10. MeshCore packet format docs (for contrast): <https://docs.meshcore.io/packet_format/>.

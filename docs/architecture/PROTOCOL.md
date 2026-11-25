# OpenMesh Protocol (OMP/1) — Specification

**Status:** draft, normative for OpenMesh v0.1.0
**Wire version:** 1 (`PROTO_VERSION = 1`)
**Last updated:** 2026-11-25

This document is the single source of truth for everything that touches the air. The Rust
implementation lives in `crates/openmesh-core` (`wire`, `crypto`, `services`, `engine`); the
C++ firmware in `firmware/lib/openmesh` mirrors it. Any behaviour not described here is
undefined. Golden vectors in `protocol/vectors/` are normative test data.

Keywords: **MUST**, **MUST NOT**, **SHOULD**, **MAY** per RFC 2119.

---

## 1. Design goals

1. **LoRa-first.** Every design decision is judged by bytes and airtime. MTU is 240 bytes on
   LoRa and the same frames are reused on every other transport.
2. **End-to-end encryption.** Relays and store-and-forward nodes only ever see ciphertext and
   routing metadata (source, destination, sequence).
3. **Managed flooding.** No global routing table is required; every node is a potential
   repeater. This is the only scheme that has been proven to work at Meshtastic scale on
   single-channel LoRa.
4. **Store-and-forward for offline peers.** Infrastructure nodes may retain encrypted
   envelopes for days and deliver them the next time the addressee is heard.
5. **Rich media.** Images and voice notes travel as chunked resources with selective-repeat
   ARQ, size-capped per radio profile and region duty cycle.
6. **Deterministic, testable core.** The host engine is IO-free; the simulator and firmware
   conformance tests share the same golden vectors.

---

## 2. Terminology

| Term | Meaning |
| --- | --- |
| Node | Any device with an OpenMesh identity. |
| Relay / repeater | Node that forwards frames (and optionally stores them). |
| Envelope | An encrypted frame body stored by a relay for a destination. |
| Frame | One OMP packet: 22-byte header + body, ≤ 240 bytes. |
| Resource | A payload split into chunks with ARQ (image, voice note, long text). |
| Channel | A symmetric group key + 8-byte channel address. |
| Companion | Protocol between an app and its local radio over USB/BLE/TCP. |
| Airtime | Time the radio is transmitting; constrained by duty cycle. |

---

## 3. Identity and addresses

Each node has two long-term key pairs and one derived address.

```
seed        : 32 random bytes (master secret, stored encrypted on the node)
sign_sk/sign_pk : Ed25519,  sign_sk = SigningKey::from_bytes(seed)
dh_sk/dh_pk     : X25519,   dh_sk = HKDF-SHA256(salt="openmesh/v1/dh", ikm=seed, info="static-x25519")
node_id     : SHA-256("openmesh/v1/node" || sign_pk || dh_pk)[0..8]
```

* `node_id` is a **self-certifying 8-byte address**. Receivers MUST verify that the announced
  keys hash to the announced `node_id` before trusting them.
* A full public identity is 72 bytes: `node_id(8) || sign_pk(32) || dh_pk(32)`.
* Channel addresses: `channel_id = SHA-256("openmesh/v1/chan" || channel_key)[0..8]`.
* `BROADCAST = FF FF FF FF FF FF FF FF`.

Node ids are not secrets; they are pseudonymous and unlinkable without out-of-band context.

---

## 4. Frame format

All integers are big-endian. A frame is at most **240 bytes** (`MTU`).

```
offset  size  field
0       1     version (3 bits, high) | frame_type (5 bits, low)
1       1     ttl (4 bits, high) | flags (4 bits, low)
2       8     src node id
10      8     dst node id | channel id | BROADCAST
18      4     seq (per-sender monotonically increasing)
22      ..    body (0..218 bytes)
```

* `version` MUST be 1 for this spec. Receivers MUST drop other versions.
* `ttl` 0..15. A relay that forwards a frame MUST decrement it; when the value is 0 the frame
  MUST NOT be forwarded.
* `seq` is a per-source counter. Senders MUST increment it for every transmitted frame. It is
  used for duplicate suppression and as a nonce component; it MUST NOT be reused within a
  node's lifetime for a given key epoch.

### 4.1 Header flags (low nibble of byte 1)

| Bit | Name | Meaning |
| --- | --- | --- |
| 0 | `ENC` | Body is AEAD-sealed. Clear bodies are only valid for `Announce`, `Probe`, `ProbeResponse`. |
| 1 | `ACK_REQ` | Sender wants a delivery acknowledgement. |
| 2 | `PRIORITY` | Time-sensitive: ACKs, path replies, voice. Relays SHOULD forward first. |
| 3 | `FROM_STORE` | Frame was replayed by a relay's store-and-forward queue. |

### 4.2 Frame types

| Value | Name | Direction | Storable |
| --- | --- | --- | --- |
| 0 | `Announce` | broadcast | no |
| 1 | `Data` | direct or channel | yes |
| 2 | `Ack` | direct | no |
| 3 | `Resource` | direct or channel | yes |
| 4 | `RouteRequest` | reserved | no |
| 5 | `RouteResponse` | reserved | no |
| 6 | `Probe` | broadcast/direct | no |
| 7 | `ProbeResponse` | direct | no |
| 8 | `StoreOffer` | direct | no |
| 9 | `StoreWant` | direct | no |
| 10 | `Admin` | direct | no |

Unknown frame types MUST be ignored (not treated as errors) so future versions can add types.

---

## 5. Encrypted envelope

All sealed bodies (except announces) use **XChaCha20-Poly1305**.

```
body := payload_flags(1) || [identity(72) if IDENTITY_INCLUDED] || nonce_prefix(12) || ct || tag(16)
```

* `nonce = nonce_prefix(12) || 0^12` (24 bytes total). The prefix MUST be generated by a CSPRNG.
  With random 96-bit prefixes, key reuse is safe up to 2^48 messages per key.
* `AAD` = the 22-byte frame header exactly as transmitted (including the possibly decremented
  TTL). Relays MUST NOT modify anything else. Receivers MUST recompute the AAD from the frame
  they received.
* `payload_flags`:
  | Bit | Name | Meaning |
  | --- | --- | --- |
  | 0 | `IDENTITY_INCLUDED` | Body contains the sender's 72-byte public identity before the nonce. Required on the first message to an unknown peer. |
  | 1 | `IS_ACK` | Body is a CBOR `Ack` (see §8). |
  | 2 | `IS_MANIFEST` | Body is a CBOR `ResourceManifest` (see §9). |
  | 3 | `RESERVED` | MUST be 0. |

### 5.1 Direct message key

```
shared = X25519(my_dh_sk, peer_dh_pk)
key    = HKDF-SHA256(ikm=shared, salt="openmesh/v1/dm",
                     info=min(node_id_a,node_id_b) || max(node_id_a,node_id_b))
```

Both directions derive the same key. Keys MUST be cached per peer and discarded when the node
is forgotten.

### 5.2 Channel (group) key

```
channel_key = HKDF-SHA256(ikm=psk32, salt="openmesh/v1/channel", info="channel/" || name)[0..32]
```

A channel frame has `dst = channel_id`. Senders MUST address channel frames to the channel id,
not `BROADCAST`, so that clients can tell channel traffic from announces. Relays treat
non-addressed frames as floodable.

### 5.3 Plaintext payloads

After decryption the plaintext is CBOR (`ciborium`, definite lengths, no floats where integers
suffice):

* direct `Data` with `payload_flags` = 0 or `IDENTITY_INCLUDED`: a CBOR `AppMessage` (§7)
* channel `Data`: a CBOR `GroupMessage` (§7.3)
* `payload_flags.IS_ACK`: CBOR `Ack`
* `payload_flags.IS_MANIFEST`: CBOR `ResourceManifest`

The Rust schemas in `crates/openmesh-core/src/services.rs` are normative. Field names are the
CBOR map keys.

---

## 6. Announce

`Announce` frames are **clear-text** (readable by anyone) and **signed**.

Body layout (CBOR `Announce`):

| Field | Type | Notes |
| --- | --- | --- |
| `version` | u8 | 1 |
| `class` | enum | `client`, `repeater`, `gateway`, `room_server`, `tracker` |
| `caps` | u16 | capability bits |
| `name` | string | 0..32 bytes UTF-8, MAY be empty |
| `hops` | u8 | incremented by each relay that rebroadcasts |
| `timestamp_ms` | u64 | sender's Unix ms; receivers only use it for display |
| `interval_s` | u16 | intended re-announce period |
| `ident` | bytes(72) | public identity |

The frame body is `announce_cbor || signature(64)`, where

```
signature = Ed25519(sign_sk, "openmesh/v1/announce" || src(8) || seq(4 BE) || announce_cbor)
```

Receivers MUST verify the signature and the self-certifying node id before using the announce.
Announces are relayed with TTL 8 (default) and **MUST NOT** be retained by store-and-forward
queues.

Announce cadence: clients every 30 min (±10% jitter) plus immediately on start, role change or
network change. Repeaters/gateways every 60 min. A node MUST NOT announce more than once per
5 minutes except in response to an explicit probe.

---

## 7. Messages

### 7.1 Direct message

`Data` frame, `dst = recipient node_id`, `ENC | ACK_REQ`, body per §5.

Recipient processing:
1. Verify `src` is known or read `identity` from the body when `IDENTITY_INCLUDED` is set.
   If the sender is unknown and no identity is present, drop the frame (relays still forward).
2. Decrypt; on failure drop and count a decryption error (never send an error frame back).
3. Emit the message to the application and send an `Ack{kind: delivered}` if `ACK_REQ`.
4. Deduplicate by `(src, msg_id)` at the application layer before surfacing.

### 7.2 Message ids

`msg_id` is a random u32 generated by the sender. Receivers MUST treat `(src, msg_id)` as the
message identity. `AppMessage.id` MUST equal the id used in the ACK.

### 7.3 Group message

`Data` frame, `dst = channel_id`, `ENC`, body per §5 sealed with the channel key.

Plaintext is CBOR `GroupMessage { sender: PublicIdentity, message: AppMessage,
signature: Vec<u8> }` where

```
signature = Ed25519(sender.sign_sk,
            "openmesh/v1/group-msg" || sender.node_id || channel_id || cbor(AppMessage without signature))
```

Receivers MUST verify the signature before surfacing a group message. Because the channel key
is shared, the signature is what proves authorship.

### 7.4 Position

`AppMessage{kind: position, position: Position}`. May be sent direct or to a channel. Clients
usually broadcast position to a channel (`dst = channel`) or to chosen contacts.

### 7.5 Telemetry

`AppMessage{kind: telemetry}` from infrastructure nodes, normally direct to a gateway/monitor
or broadcast to a status channel. `Telemetry` fields are all optional.

---

## 8. Acknowledgements

`Ack` frames are sealed direct messages to the original sender. Plaintext is CBOR `Ack`:

| Field | Type | Notes |
| --- | --- | --- |
| `kind` | enum | `delivered`, `read`, `resource_window`, `resource_complete` |
| `msg_id` | u32 | message being acked |
| `res_id` | u32? | resource being acked (resource kinds) |
| `next_expected` | u16? | next missing chunk |
| `bitmap` | u32? | 32 chunks after `next_expected`; bit 0 = `next_expected+1` |
| `timestamp_ms` | u64 | |

Rules:
* `delivered` MUST be sent for every received `Data` frame with `ACK_REQ`, after the message
  is accepted by the application layer (durably stored or actively displayed).
* `read` is optional and user-triggered.
* ACK frames MUST NOT request ACKs themselves.
* Senders retry unacked messages with `Data` retransmission after `ACK_TIMEOUT_MS`
  (see §12), up to `MAX_MSG_RETRIES`. ACK frames are never retried.

---

## 9. Resources (images, voice notes, long text)

A resource is a single AEAD-sealed blob split into chunks. The manifest tells the receiver how
to reassemble it; the chunks carry ciphertext only, so no per-chunk crypto overhead is paid.

### 9.1 Manifest

Sent as a `Data` message with `payload_flags.IS_MANIFEST`:

```
ResourceManifest {
  res_id: u32, msg_id: u32, kind: attachment|long_text|avatar|channel_history,
  mime: string, total_len: u32, chunk_bytes: u16, chunk_count: u16, sha256: [u8;32]
}
```

`sha256` is over the **sealed blob** (`total_len` bytes) so receivers verify integrity before
decrypting — this also covers transport corruption. `res_id` is random u32 per sender.

### 9.2 Chunks

`Resource` frame body:

```
payload_flags(1) || nonce-less(0) || res_id(4) || chunk_index(2) || ciphertext_slice
```

Resource frames carry `ENC` set, but the slice is already ciphertext: the nonce/tag live only
inside the sealed blob at the manifest's fixed offsets. Receivers MUST NOT attempt AEAD on
individual chunk frames; they reassemble `total_len` bytes, verify `sha256`, then decrypt with
the direct/channel key.

Chunk size: `chunk_bytes = 180` by default (fits `218 - 1 - 4 - 2 = 211`; headroom kept for
future fields). Senders MUST ensure `chunk_count = ceil(total_len / chunk_bytes)`.

### 9.3 ARQ

* Window: 8 chunks in flight by default.
* Receiver sends `Ack{kind: resource_window}` every `RESOURCE_ACK_EVERY` (default 8) chunks or
  immediately after detecting a hole / end of burst, with `next_expected` + 32-bit bitmap.
* Sender retransmits missing chunks after `RESOURCE_RTO_MS` (default 3× expected ack time,
  min 1500 ms) with exponential backoff, max `MAX_RESOURCE_RETRIES` (6).
* Transfer completes when all chunks are acked; sender MAY keep the blob for
  `RESOURCE_KEEP_MS` to serve retransmits, then frees it.
* Receiver emits `ResourceReceived` only after `sha256` verification and successful decryption.
  Then it MUST send `Ack{kind: resource_complete}`.
* Transfers for unknown peers are dropped; relays forward resource frames normally.

### 9.4 Size policy

| Profile | Attachment cap (default) | Voice cap |
| --- | --- | --- |
| `SHORT_TURBO`, `SHORT_FAST` | 24 KiB | 12 s |
| `MEDIUM_FAST` | 12 KiB | 10 s |
| `LONG_FAST` | 6 KiB | 8 s |
| `LONG_SLOW` | 2 KiB | 5 s |

In 1 % duty-cycle regions the sender MUST estimate airtime and either clamp or ask the user to
confirm before sending. Estimator: `util::lora_airtime_ms`.

Voice notes: Opus mono 6–10 kbps, 16 kHz, 20 ms frames, packaged as `audio/ogg; codecs=opus`;
optional Codec2 700C profile (`audio/c2-700c`) for the most constrained presets.

---

## 10. Store-and-forward

Relays with `STORE` capability and queue capacity > 0 MAY retain `Data`/`Resource` envelopes
whose destination is not currently reachable directly.

```
retain  : if frame is storable, dst != me, TTL would expire the frame, and queue has room
          → store ciphertext envelope keyed by (dst, msg_id)
offer   : when the addressee is heard (any frame from it or a signed announce), send
          StoreOffer{ids} (up to 16 ids per frame, direct, PRIORITY)
want    : addressee replies StoreWant{ids} for ids it does not already have
deliver : relay replays the envelopes with FROM_STORE set, original seq/body, TTL = DEFAULT_TTL
expire  : envelopes are evicted after store_ttl_ms (default 72 h) or when full (oldest first)
```

Addressees MUST deduplicate replayed envelopes by `(src, msg_id)` and MUST ack them normally.
Relays MUST NOT store announces, ACKs, offers or wants. A relay MUST NOT decrypt stored
envelopes; it has no key material for them.

Offer cadence per peer: at most once per 5 minutes, and once on every fresh announce.

---

## 11. Flooding and duplicate suppression

OMP/1 is a **managed flood**: every frame is broadcast once by every node that hears it, subject
to TTL and dedup.

```
on receive frame F on link L:
  if F.ttl == 0 and F.dst not in {me, BROADCAST, my_channels}: drop        # cannot forward
  if dedup.check_and_insert(F.src, F.seq) == false: drop                    # already seen
  learn(F.src, L)                                                          # peer bookkeeping
  if F.dst in {me}            : deliver/decrypt
  if F.dst in my_channels     : try channel decrypt, deliver
  if F.typ == Announce        : parse, verify, learn, relay if ttl>0
  if F.typ == StoreOffer/Want : handle, relay if ttl>0
  if relay_role and F.typ in {Data, Resource} and F.dst == me: maybe store
  if F.ttl > 0: emit relay_copy(F) with ttl-1, same body, AAD recomputed on receipt
```

* Default TTL: 5 for client frames, 8 for announces.
* Dedup cache: `(src, seq)` with 30 min TTL and ≥ 2048 entries.
* Relays SHOULD prioritise `PRIORITY` frames and SHOULD enforce an airtime budget
  (token bucket) of at most 1 % duty cycle in EU868/AS923.
* Nodes MUST NOT originate more than `MAX_ORIGINATE_PER_MIN` frames in a minute unless the
  frames are ACKs or probe replies.

---

## 12. Timing parameters

| Constant | Value | Notes |
| --- | --- | --- |
| `ANNOUNCE_INTERVAL_CLIENT_MS` | 1 800 000 | 30 min ±10 % |
| `ANNOUNCE_INTERVAL_RELAY_MS` | 3 600 000 | 60 min |
| `ANNOUNCE_MIN_INTERVAL_MS` | 300 000 | 5 min floor |
| `DEDUP_TTL_MS` | 1 800 000 | |
| `ACK_TIMEOUT_MS` | 8 000 | retry data messages |
| `MAX_MSG_RETRIES` | 3 | |
| `RESOURCE_WINDOW` | 8 | chunks |
| `RESOURCE_RTO_MS` | 3 000 | ×2 backoff |
| `MAX_RESOURCE_RETRIES` | 6 | |
| `RESOURCE_KEEP_MS` | 600 000 | after completion |
| `STORE_OFFER_INTERVAL_MS` | 300 000 | per peer |
| `STORE_TTL_MS` | 259 200 000 | 72 h |
| `PEER_EXPIRY_MS` | 86 400 000 | 24 h without traffic |
| `DEFAULT_TTL` | 5 | clients |
| `ANNOUNCE_TTL` | 8 | |

Implementations MUST tolerate clock skew of ±5 minutes and SHOULD use monotonic time for
timeouts.

---

## 13. Companion API (radio ⇄ app)

Framing: `b"OM" | type(u8) | len(u16 LE) | payload`. Max payload 1024 bytes (one frame plus
metadata). Types and payload CBOR schemas are defined in `crates/openmesh-core/src/companion.rs`.
Full semantics in `docs/architecture/COMPANION-API.md`.

App → radio: `Hello`, `ConfigGet`, `ConfigSet`, `Tx`, `StatsGet`, `Ping`, `Reset`.
Radio → app: `HelloAck`, `Config`, `Rx`, `Stats`, `Pong`, `Event`.

A radio with no connected app still relays, announces and stores envelopes
(standalone repeater mode).

---

## 14. Transport bindings

| Transport | Framing | Notes |
| --- | --- | --- |
| LoRa (SX1262/LR1110) | one frame per packet, ≤ 240 B | No length prefix; radio packet boundaries delimit frames. |
| USB CDC serial | SLIP-style: `0xC0` delimiter, `0xDB 0xDC` = 0xC0, `0xDB 0xDD` = 0xDB | Shared with companion API; frames are distinguished by the companion header. |
| BLE (NUS-like) | 2-byte length prefix, MTU-negotiated chunks | Same as serial at the stream level. |
| UDP (WiFi/Ethernet LAN) | datagram per frame | Broadcast discovery to port 4403. |
| TCP (WAN relay) | 2-byte length prefix | Optional, used for internet bridges. |
| WebSocket (bridge ↔ UI) | JSON API, not OMP | See `WEBSOCKET-API.md`. |

Bridges (gateways) forward frames between any of these transports with the same TTL/dedup
rules; they are not special-cased in the protocol.

---

## 15. Golden vectors

`protocol/vectors/` contains JSON files with inputs and expected outputs for:

| File | Covers |
| --- | --- |
| `frame.json` | header encode/decode, flags, TTL, malformed inputs |
| `identity.json` | seed → node id, public identity bytes, signatures |
| `seal.json` | DM/channel sealing with fixed nonces (use `nonce_prefix` override for tests) |
| `announce.json` | CBOR announce + signature verification |
| `app_message.json` | AppMessage CBOR round-trip |
| `resource.json` | chunking, manifest, reassembly, ACK bitmaps |
| `companion.json` | companion frames |

Vectors are generated from Rust with `cargo run -p openmesh-core --example gen_vectors`
(workstream: core) and MUST be validated by the firmware test suite
(`firmware/test/test_protocol/`). A change that alters a vector requires a spec version bump.

---

## 16. Versioning and extension rules

* The 3-bit version field gates breaking changes. Version 1 implementations MUST drop frames
  with other versions.
* New frame types with the reserved values 11–31 MAY be added without a version bump; unknown
  types MUST be ignored.
* Reserved header flag values: only the four defined bits exist; the high nibble of byte 1
  MUST be 0.
* CBOR payloads MUST tolerate unknown keys (forward compatibility) and MUST reject missing
  required keys.
* `payload_flags.RESERVED` bits MUST be zero; receivers MUST reject bodies with unknown flags.

---

## 17. Security considerations

* **Metadata is visible.** Relays see source and destination ids, hop counts and timing.
  Traffic analysis is possible; future versions MAY add cover traffic and destination
  blinding.
* **No forward secrecy in v1.** Static ECDH means a compromised long-term key decrypts past
  direct messages. A ratchet (double-ratchet style, Reticulum-like) is planned for OMP/2;
  voice calls will require it.
* **Replay protection.** `(src, seq)` dedup covers accidental replays; for hostile replays the
  recipient MUST also deduplicate at the application layer by `(src, msg_id)` and SHOULD cache
  seen `msg_id`s for 24 h.
* **Group membership is a shared secret.** Anyone with the channel key can read and write.
  Signature verification prevents impersonation within the channel but cannot revoke.
* **Store-and-forward is untrusted.** Relays can drop, delay or replay envelopes but cannot
  read or forge them (ACKs are sealed too). Clients MUST NOT treat "stored by relay" as proof
  of delivery.
* **Downgrade and DoS.** Relays MUST NOT modify frames; AAD covers the whole header.
  Rate limiting is local per sender; there is no global reputation in v1.
* **Key storage.** Devices MUST store the 32-byte seed encrypted at rest (see
  `ARCHITECTURE.md` §Security). Apps SHOULD use OS keystores.

---

## Appendix A — Worked sizes

| Payload | Frames | Airtime @ `MEDIUM_FAST` (SF9/250k) |
| --- | --- | --- |
| 100-char text | 1 | ~610 ms |
| 5 KiB image | 29 chunks + manifest + 3 ACKs ≈ 33 | ~20 s |
| 8 s Opus voice (~7 KiB) | 41 + ack ≈ 45 | ~27 s |
| Announce (200 B) | 1 | ~520 ms |

Airtime is computed by `util::lora_airtime_ms`; it includes preamble, header and CRC.

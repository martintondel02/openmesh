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

* `node_id` is a **self-certifying 8-byte address**:
  `node_id = (SHA-256("openmesh/v1/node" || sign_pk || dh_pk)[0..8]) with the high bit of byte 0
  cleared`. Receivers MUST verify that the announced keys hash to the announced `node_id` before
  trusting them.
* A full public identity is 72 bytes: `node_id(8) || sign_pk(32) || dh_pk(32)`.
* Channel addresses: `channel_id = (SHA-256("openmesh/v1/chan" || channel_key)[0..8]) with the
  high bit of byte 0 set`.
* `BROADCAST = FF FF FF FF FF FF FF FF` (all ones; top bit set but distinct from every channel id,
  which would require a 2^-64 preimage to collide).
* **Address class rule:** if the high bit of byte 0 is 0 the address is a *unicast node id*; if it
  is 1 the address is a *channel*. This lets relays decide, from the header alone, whether an
  envelope can be stored for a single addressee (§10) or must be flooded without retention.
  It costs one bit of address entropy (57 bits remain — still far beyond collision needs).

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
| 0 | `ENC` | Body is AEAD-sealed. Clear bodies are only valid for `Announce`, `Probe`, `ProbeResponse`, `StoreOffer`, `StoreWant`. |
| 1 | `ACK_REQ` | Sender wants a delivery acknowledgement. |
| 2 | `PRIORITY` | Time-sensitive: ACKs, path replies, voice. Relays SHOULD forward first. |
| 3 | reserved | MUST be 0. Relays cannot set it (flags are authenticated); store replay is signalled out-of-band by the offer/want handshake. |

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

All sealed bodies (except announces, which are signed but not encrypted) use
**XChaCha20-Poly1305**. ACKs follow the same layout with `msg_ref` set to the acknowledged
message id.

```
body := payload_flags(1) || msg_ref(4 BE) || [identity(72) if IDENTITY_INCLUDED] || nonce_prefix(12) || ct || tag(16)
```

`msg_ref` is a clear-text random u32 chosen by the sender (the same value as `AppMessage.id`). It
is the only application-visible metadata a relay gets, and it exists so relays can index and
offer stored envelopes without decrypting them. It reveals nothing beyond "these frames belong
to the same logical message".

* `nonce = nonce_prefix(12) || 0^12` (24 bytes total). The prefix MUST be generated by a CSPRNG.
  With random 96-bit prefixes, key reuse is safe up to 2^48 messages per key.
* `AAD` = the 22-byte frame header with the **TTL nibble (high nibble of byte 1) cleared to
  zero**. TTL is the only header field relays may change, so it cannot be authenticated; every
  other bit (version, type, flags, src, dst, seq) is covered. Senders and receivers MUST both
  zero the TTL nibble before computing the AAD. Relays MUST NOT modify anything else.
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

A channel frame has `dst = channel_id` (top bit set, §3). Senders MUST address channel frames to
the channel id, not `BROADCAST`, so that clients can tell channel traffic from announces and so
relays know the frame is not storable. Relays treat non-addressed frames as floodable.

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

`Announce` frames are **clear-text** (readable by anyone) and **signed**. The body is a compact
fixed binary layout (not CBOR) because a 72-byte identity plus a 64-byte signature does not fit
in a CBOR body within the 218-byte limit, and firmware decoders benefit from fixed offsets:

```
offset  size  field
0       1     version (1)
1       1     class (0=client, 1=repeater, 2=gateway, 3=room_server, 4=tracker)
2       2     caps (u16 BE, bitflags §6.1)
4       1     reserved (MUST be 0)
5       1     name_len (0..32)
6       2     interval_s (u16 BE, intended re-announce period)
8       8     timestamp_ms (u64 BE)
16      8     node_id
24      32    sign_pk (Ed25519)
56      32    dh_pk (X25519)
88      N     name (UTF-8, name_len bytes)
88+N    64    signature (Ed25519)
```

Total: `152 + name_len` bytes, maximum 184 bytes — one frame, comfortably within the MTU.

`signature = Ed25519(sign_sk, "openmesh/v1/announce" || src(8) || seq(4 BE) || body_without_signature)`

**Relays must not modify signed bytes.** The hop count is therefore *not* a field: announce
frames are always originated with `TTL = ANNOUNCE_TTL (8)` and a receiver derives
`hops = ANNOUNCE_TTL - frame.ttl` (clamped to 0). Any announce whose TTL is not 8 at origin is
invalid; relays decrement TTL as usual.

Receivers MUST verify the signature AND that `SHA-256("openmesh/v1/node" || sign_pk || dh_pk)[0..8]
== node_id`. Announces with a bad signature or inconsistent identity are dropped (relays must
not forward them). Announce frames are relayed with TTL 8 and MUST NOT be stored for
store-and-forward.

### 6.1 Capability bits

| Bit | Cap | Meaning |
| --- | --- | --- |
| 0 | `RELAY` | forwards frames |
| 1 | `STORE` | store-and-forward capable |
| 2 | `GPS` | has a position source |
| 3 | `WIFI` | has WiFi |
| 4 | `ETHERNET` | has Ethernet |
| 5 | `BLE` | provides a BLE companion link |
| 6 | `USB` | provides a USB companion link |
| 7 | `SCREEN` | has a display |
| 8 | `AUDIO_OUT` | can play audio |
| 9 | `SOLAR` | solar powered |

### 6.2 Cadence

Clients every 30 min (±10 % jitter) plus immediately on start, role change or network change.
Repeaters/gateways every 60 min. A node MUST NOT announce more than once per 5 minutes except
in reply to an explicit probe. Announce jitter is important: synchronised announces from many
nodes cause collisions on a shared channel.

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

`msg_id` is an opaque u32 generated by the sender (the reference engine derives it from a
per-identity monotonic counter and a derivation epoch). Receivers MUST treat `(src, msg_id)` as
the message identity. `AppMessage.id` MUST equal the id used in the ACK.

Senders MUST NOT reuse a `msg_id` for a different message. The allocation counter MUST be
persisted with the sender's other state (`Engine::export_state`/`import_state` in the reference
engine, additive state JSON version 2 fields `msg_counter`/`msg_id_epoch` in the FFI): a restart
that resets the counter can reissue an id, overwrite the app's message row for a stalled
attachment and break retry. Wire format is unchanged — `msg_id` stays a u32 in the clear. Importing
a pre-version-2 state (no counter) starts at a high legacy floor with the current epoch so ids
issued by older app versions are never re-derived.

### 7.3 Group message

`Data` frame, `dst = channel_id`, `ENC`, body sealed with the channel key (§5.2).

Plaintext is CBOR `GroupMessage`:

| Field | Type | Notes |
| --- | --- | --- |
| `sender` | bytes(8) | sender's node id |
| `signature` | bytes(64) | Ed25519 over the signed payload below |
| `message` | `AppMessage` | application message (its own `signature`/`sender` fields are unused here) |

```
signature = Ed25519(sender.sign_sk,
            "openmesh/v1/group-msg" || sender.node_id(8) || channel_id(8) || cbor(message))
```

Receivers resolve the sender's Ed25519 public key from the local peer book (learned from
announces or from a preceding `IDENTITY_INCLUDED` frame). If the signer is unknown, receivers
MUST look for `payload_flags.IDENTITY_INCLUDED` in the frame; if the identity is still unknown
the message MUST be dropped (it cannot be authenticated). This keeps the per-message overhead at
8 + 64 bytes instead of repeating a 72-byte identity.

If the sealed body would exceed `MAX_BODY` (about 60 characters of text in practice — and almost
always for position/telemetry payloads because of the 64-byte signature), the sender MUST send
the message as a resource (§9) addressed to the channel: the plaintext is `CBOR(GroupMessage)`,
sealed and chunked exactly like any other resource, with `kind = long_text` (or `position` when a
future kind is defined; v1 reuses `long_text`). Receivers reassemble, verify the sender signature
on the decoded message, then surface it normally. `GroupMessage` never contains a full public
identity.

This applies to every channel message kind: a `send_position` or `send_telemetry` to a channel
MUST automatically fall back to a resource when the single-frame layout does not fit.

### 7.4 Position

`AppMessage{kind: position, position: Position}`. May be sent direct or to a channel. Clients
usually broadcast position to a channel (`dst = channel`) or to chosen contacts.

### 7.5 Telemetry

`AppMessage{kind: telemetry, telemetry: Some(Telemetry)}` from infrastructure nodes, normally
direct to a gateway/monitor or broadcast to a status channel. `Telemetry` fields are all optional.

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

The sealed blob is exactly `payload_flags(1)=0 || nonce_prefix(12) || ciphertext || tag(16)`,
where the plaintext is the resource payload (raw attachment bytes for `attachment`, CBOR for
`long_text`, channel history frames for `channel_history`). It is sealed with the direct-message
key for direct resources or the channel key for channel resources, and the AAD is

```
"openmesh/v1/resource" || res_id(4 BE) || msg_id(4 BE) || total_len(4 BE) || chunk_bytes(2 BE)
```

(34 bytes: 20-byte domain string + 4 + 4 + 4 + 2). `sha256` MUST NOT be part of this AAD: it is computed over the sealed blob, so
including it would be circular. Integrity is still fully covered: the manifest message is itself
a sealed `Data` frame (AEAD-protected), its `sha256` covers the whole sealed blob, and the AAD
above binds the *other* manifest parameters so a relay cannot swap blobs or re-frame them under a
different resource id, message id, length or chunk size. Blob size is `total_len`; the receiver
reassembles exactly that many bytes, verifies `sha256`, then decrypts.

### 9.2 Chunks

`Resource` frame body:

```
payload_flags(1) || msg_ref(4 BE) || res_id(4 BE) || chunk_index(2 BE) || ciphertext_slice
```

`msg_ref` is the message id from the manifest's `msg_id`, in clear text so relays can associate
chunks with a stored envelope. `payload_flags` MUST be 0 in v1.

Resource frames carry `ENC` set, but the slice is already ciphertext: the nonce/tag live only
inside the sealed blob at the manifest's fixed offsets. Receivers MUST NOT attempt AEAD on
individual chunk frames; they reassemble `total_len` bytes, verify `sha256`, then decrypt with
the direct/channel key.

Chunk size: `chunk_bytes = 180` by default (fits `218 - 1 - 4 - 4 - 2 = 207`; headroom kept for
future fields). Senders MUST ensure `chunk_count = ceil(total_len / chunk_bytes)`.

### 9.3 ARQ

* Window: 24 chunks in flight by default (must stay ≤ 32: the ack carries one `u32` bitmap).
* Receiver sends `Ack{kind: resource_window}` every `RESOURCE_ACK_EVERY` (default 8) new chunks,
  immediately after detecting a hole, and after `RESOURCE_ACK_DELAY_MS` (default 5000 ms) of quiet
  time (end of burst), with `next_expected` + 32-bit bitmap.
* Sender retransmits missing chunks after `RESOURCE_RTO_MS` (default 3 s) with exponential
  backoff capped at `RESOURCE_MAX_RETRY_BACKOFF_MS` (60 s). Give-up is **time-based**: an outgoing
  resource is abandoned only after `RESOURCE_RETRY_BUDGET_MS` (15 min) without acknowledged chunk
  progress (a hard ceiling of `MAX_RESOURCE_RETRIES` = 32 consecutive retries also applies). Fixed
  retry counts are not used: under duty-cycle pacing a single window can take tens of seconds of
  wall time before its ack arrives.
* Senders MUST pace chunk emission to the radio's legal airtime, not to the host tick rate. The
  core exposes `util::resource_pace_interval_ms(profile, duty_pct, batch)` =
  `ceil(batch × lora_airtime_ms(profile, 240 B) / (duty_pct / 100))`, clamped to 250 ms..=120 s
  (fair-use regions without a duty cap use 100 %), and `Engine::set_resource_pace_interval_ms`
  applies the hint to **both** tick- and ack-driven releases: at most `RESOURCE_TX_BATCH` (2) new
  chunks leave the engine per interval. The bridge computes the hint from `RuntimeConfig`'s
  profile/region at startup and after every config change and logs it at info level. The FFI
  exposes `openmeshResourcePaceIntervalMs(profile, region, batch)` plus
  `OpenMeshNode::setResourcePaceIntervalMs(ms)` so mobile embedders apply the same hint after node
  creation and config changes. Unset embedders keep the backwards-compatible 1000 ms default.
  Rationale: a companion radio under 10 % duty-cycle pacing drops a whole-window burst before its
  chunks reach air (phone→bridge attachment loss), and a fast-ticking app cannot outpace the
  radio's FIFO this way. Pacing is a transport-timing concern only — frames are unchanged.
* These timings are implementation defaults, not wire-format fields: `protocol/vectors/` does not
  cover them and no frame layout changes.
* Transfer completes when all chunks are acked; sender MAY keep the blob for
  `RESOURCE_KEEP_MS` to serve retransmits, then frees it.
* Receiver emits `ResourceReceived` only after `sha256` verification and successful decryption.
  Then it MUST send `Ack{kind: resource_complete}`.
* Transfers for unknown peers are dropped; relays forward resource frames normally.

### 9.4 Size policy

Airtime below is measured with CR 4/5, a 16-symbol preamble and CRC on, for the full transfer
including manifest and ACKs (MEDIUM_FAST: a 240-byte frame is 611 ms; a 5 KiB image is ~18 s).
TX power does not affect airtime. In duty-cycle regions the sender MUST estimate airtime and
either clamp the attachment or require explicit user confirmation. Estimator:
`util::lora_airtime_ms`.

| Profile | 240 B frame | 5 KiB image | Default attachment cap | Voice duration cap (tiny / medium / high) |
| --- | --- | --- | --- | --- |
| `SHORT_TURBO` | 97 ms | ~2.5 s | 24 KiB (~12 s) | 30 s / 20 s / 15 s |
| `SHORT_FAST` | 194 ms | ~5 s | 24 KiB (~24 s) | 30 s / 20 s / 15 s |
| `MEDIUM_FAST` | 611 ms | ~18 s | 12 KiB (~42 s) | 30 s / 20 s / 8 s |
| `LONG_FAST` | 2 034 ms | ~53 s | 6 KiB (~64 s) | 30 s / 15 s / 4 s |
| `LONG_SLOW` | 13 509 ms | ~354 s | 1.5 KiB (~105 s) | 8 s / 3 s / 1 s |

Voice caps are `min(tier_max_duration, profile_attachment_cap / bytes_per_second)`; clients MUST
show the effective cap and estimated airtime before sending.

On `LONG_SLOW`, images are effectively a last resort: clients MUST warn that a 5 KiB image costs
about six minutes of channel airtime and SHOULD offer a text-only or thumbnail-only alternative.
On `SHORT_*` profiles, voice notes are cheap enough to be the default way to send long thoughts.

### 9.4.1 Voice tiers (v0.6)

Voice notes use three tiers; tiny/medium are **Codec2** (raw frame streams, no container), high is
**Opus** (clients may substitute AAC/MP4 where OGG/Opus is unavailable, and must declare the MIME):

| Tier | MIME | Input / framing | 8 s payload | Airtime at `MEDIUM_FAST` |
| --- | --- | --- | --- | --- |
| tiny | `audio/c2-1300` | 8 kHz mono s16; 320 samples = 40 ms → 52 bits = 7 B frames | ≈ 1.4 KiB | ~7 s |
| medium | `audio/c2-3200` | 8 kHz mono s16; 160 samples = 20 ms → 64 bits = 8 B frames | ≈ 3.2 KiB | ~13 s |
| high | `audio/ogg; codecs=opus` | Opus mono ~12 kbps, 20 ms frames | ≈ 12 KiB | ~43 s |

Codec2 payloads are concatenated frames with fixed byte lengths (7 and 8 bytes); decoders MUST
ignore a trailing partial frame and MUST reject payloads with an unsupported mode. Encoders drop
a trailing partial PCM frame. The tiny tier is the intended default on constrained profiles: an
8-second intelligible voice note costs less than one 640 px image. Tier metadata (sizes, MIME,
sample rates, duration caps) lives in `content/presets/media.json` and is shared by all clients.
No wire-format change: resources already carry MIME + bytes.

**Region legal maxima.** Defaults are the legal maxima: selecting a region sets the TX power to
`region.max_power_dbm`, the duty-cycle budget to `region.duty_cycle_pct` and the centre frequency
to `region.freq_khz`, unless the user explicitly lowers them (or an active regulatory override,
§9.5, says otherwise). `Region.bw_khz` is the **maximum legal bandwidth**: a profile wider than
that is not selectable, and clients fall back to `MEDIUM_FAST` (`LONG_FAST` on `EU433`) with a
visible note. The power values are EIRP ceilings — antenna gain can push effective radiated
power over the limit. Every surface that shows these numbers MUST also say "verify your local
rules"; this table is not legal advice. Per-country sources and caveats live in
[regulatory-and-regions.md](../design/regulatory-and-regions.md) §1.

| Region | Centre (kHz) | Max power (dBm EIRP) | Duty cycle | Max BW (kHz) | Notes |
| --- | --- | --- | --- | --- | --- |
| `EU868` | 869 525 | 27 | 10 % | 250 | CEPT 869.4–869.65 MHz, 500 mW ERP, ≤10 %; covers Norway (Nkom/EEA). No 500 kHz presets in this band. |
| `EU433` | 433 920 | 10 | 10 % | 250 | CEPT 433.05–434.79 MHz, 10 mW; short range, good propagation. |
| `US915` | 915 000 | 30 | none (fair use) | 500 | FCC 902–928 MHz, 1 W conducted with FHSS/DTS rules; 500 kHz presets allowed. |
| `AU915` | 915 000 | 30 | none (fair use) | 500 | ACMA 915–928 MHz, 1 W EIRP. |
| `IN865` | 865 000 | 30 | none | 250 | India 865–867 MHz; conservative 250 kHz until the max-BW audit completes. |
| `KR920` | 920 000 | 23 | none | 250 | Korea 920–923 MHz. |
| `AS923` | 923 200 | 20 | 10 % | 250 | Conservative floor across the AS923 group (SG/MY/TH/ID/PH/VN/JP…); per-country split is future work. |

The effective TX power is always `min(region.max_power_dbm, hardware_max)`; board caps are
22 dBm for SX1262 boards and 15 dBm for the LR1110 (T1000-E). `SHORT_TURBO` (500 kHz) is therefore
only selectable in `US915` and `AU915`, and is rejected in `EU868`, `EU433`, `IN865`, `KR920` and
`AS923`.

### 9.5 Regulatory policy and override

The wire protocol does not change: duty cycle, TX power and centre frequency are local radio
configuration, never fields in a frame, and the golden vectors are independent of region
settings. The normative contract for the numbers, UI rules and JSON surfaces is
[regulatory-and-regions.md](../design/regulatory-and-regions.md).

```
policy = region defaults
if override.enabled:
    duty      = override.duty_cycle_pct ?? region.duty_cycle_pct
    power     = min(override.max_power_dbm ?? region.max_power_dbm, hardware_max)
    frequency = override.frequency_khz ?? region.freq_khz
```

* A regulatory override is **disabled by default** and only takes effect after the user has
  explicitly acknowledged the warning defined in
  [regulatory-and-regions.md](../design/regulatory-and-regions.md) §2.1
  (`openmesh_core::regulatory::OVERRIDE_WARNING`). While active it MUST be shown on every
  relevant surface with that warning text verbatim, and disabling it MUST take effect
  immediately without acknowledgement.
* Acknowledgement is required only when the override **exceeds a legal limit** (higher duty,
  higher TX power, or a frequency different from the region centre). Lowering power or duty, or
  staying on the region frequency, needs no acknowledgement. `duty_cycle_pct = 0` means
  "transmit nothing", which is always legal and needs no acknowledgement. The optional reason is
  limited to 120 characters.
* The hardware cap always wins, even under an override; an override cannot raise effective
  radiated power above the region's EIRP limit once antenna gain is accounted for. Antenna gain
  is the operator's responsibility.
* Enforcement points: firmware airtime token bucket (duty) and radio configuration (power,
  frequency, PA clamp); bridge airtime estimates, config/status/setup and activity log; client
  estimates and the override banner. No other protocol behaviour changes.
* **Not legal advice.** National law changes; implementations MUST tell users to verify their
  local rules.

---

## 10. Store-and-forward

Relays with `STORE` capability and queue capacity > 0 MAY retain `Data`/`Resource` envelopes
whose destination is not currently reachable directly.

```
retain  : if frame is storable, dst != me, dst is a unicast node id (high bit 0, §3), and the
          queue has room → store ciphertext envelope keyed by (dst, msg_ref), keeping the
          original frame bytes. Frames addressed to a channel (high bit 1) MUST NOT be stored:
          channel history is a room-server feature (M5), not store-and-forward.
offer   : when the addressee is heard (any frame from it or a signed announce), send
          StoreOffer{ids} (up to 16 clear-text msg_refs per frame, direct, PRIORITY)
want    : addressee replies StoreWant{ids} for ids it does not already have

`StoreOffer` and `StoreWant` bodies are **clear-text CBOR** (`{ids: [u32]}`) even though they are
unicast frames. Rationale: a relay may hold envelopes for a node whose announce it has not heard
(e.g. mail left by a gateway), so it cannot always seal to the addressee; and the `msg_ref` values
are already clear-text in `Data`/`Resource` bodies, so this leaks nothing new. Spoofed offers or
wants are a denial-of-service nuisance only (the addressee deduplicates and the envelopes stay
encrypted to it). Relays MUST rate-limit offers to once per peer per 5 minutes.
deliver : relay replays the original frames verbatim except TTL = DEFAULT_TTL (the AAD is
          TTL-independent by design, so the addressee can still decrypt and verify them)
expire  : envelopes are evicted after store_ttl_ms (default 72 h) or when full (oldest first)
```

Addressees MUST deduplicate replayed envelopes by `(src, msg_ref)` and MUST ack them normally.
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
  if F.ttl > 0: emit relay_copy(F) with ttl-1, same body, same flags
                (AEAD AAD ignores TTL, so the ciphertext survives any number of hops)
```

* Default TTL: 5 for client frames, 8 for announces.
* Dedup cache: `(src, seq)` with 30 min TTL and ≥ 2048 entries.
* Relays SHOULD prioritise `PRIORITY` frames and SHOULD enforce the region's duty-cycle budget
  (token bucket): 10 % in `EU868`/`EU433`/`AS923`, fair use in `US915`/`AU915`/`IN865`/`KR920`,
  honouring an active regulatory override (§9.5).
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
| `RESOURCE_WINDOW` | 24 | chunks (<=32: one `u32` ack bitmap) |
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

Framing (normative): SLIP delimiters (`0xC0`, escaped with `0xDB`), then
`b"OM" | type(u8) | len(u16 LE) | payload | crc16`. Max payload 1024 bytes. Types, payload CBOR
schemas and semantics are defined in [`COMPANION-API.md`](COMPANION-API.md); the Rust codec lives
in `crates/openmesh-core/src/companion.rs`.

App → radio: `Hello`, `ConfigGet`, `ConfigSet`, `Tx`, `StatsGet`, `Ping`, `Reset`.
Radio → app: `HelloAck`, `Config`, `Rx`, `Stats`, `Pong`, `Event`.

A radio with no connected app still relays, announces and stores envelopes
(standalone repeater mode).

---

## 14. Transport bindings

| Transport | Framing | Notes |
| --- | --- | --- |
| LoRa (SX1262/LR1110) | one frame per packet, ≤ 240 B | No length prefix; radio packet boundaries delimit frames. |
| USB CDC serial | Companion framing (SLIP + `OM` header + CRC), see `COMPANION-API.md` | OMP frames travel inside companion `Tx`/`Rx` messages; there is no raw frame stream. |
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

Measured with the Semtech formula and the actual body layouts in this spec, on `MEDIUM_FAST`
(SF9, BW 250 kHz, CR 4/5, 16-symbol preamble, CRC on):

| Payload | Frames | Airtime |
| --- | --- | --- |
| 100-character text (AppMessage, single Data frame) | 1 (201 B frame) | 519 ms |
| Announce (12-char name, binary layout) | 1 (186 B frame) | 488 ms |
| 5 KiB image (29 chunks of 180 B) | 34 (1 manifest + 29 chunks + 4 ACKs) | ~17.8 s |
| 8 s Opus voice note (~7 KiB) | 44 | ~23.3 s |
| Small ACK | 1 (≈120 B frame) | 335 ms |

Frame overhead: 22-byte header + 1 payload_flags + 4 msg_ref + 12 nonce prefix + 16 AEAD tag =
55 bytes for a short direct message, leaving 163 bytes of plaintext per frame; resources pay
only 22 + 1 + 4 + 4 + 2 = 33 bytes per chunk frame.

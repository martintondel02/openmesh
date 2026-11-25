# Meshtastic vs MeshCore vs OpenMesh — feature and protocol comparison

**Status:** research note · **Last updated:** 2026-11-25
**Question:** we are building a fourth LoRa mesh. What do the two mature ones do, what do they
teach us, where can OpenMesh be meaningfully different, and what would it take to interoperate?

Related: [Architecture](../architecture/ARCHITECTURE.md) ·
[Protocol](../architecture/PROTOCOL.md) ·
[Reticulum/LXMF notes](reticulum-lxmf-notes.md) ·
[LoRa capacity analysis](lora-capacity-and-media.md) ·
[ADR 0001](../adr/0001-own-protocol-vs-reticulum.md) ·
[ADR 0002](../adr/0002-managed-flooding.md).

---

## 1. Sources and method

Public documentation and source used, all fetched or checked out on 2026-11-25:

| Source | What it gave us |
| --- | --- |
| <https://meshtastic.org/docs/overview/> | packet shape, cleartext 16-byte header, payload limit, flooding/retransmit behaviour, client packet buffering, channel model |
| <https://meshtastic.org/docs/overview/encryption/> | AES-CTR channel keys, AES-CCM option, X25519+AES-256-CCM DMs, XEdDSA signed broadcasts (+66 bytes), node number = CRC-32(pubkey), no forward secrecy |
| <https://meshtastic.org/docs/overview/radio-settings/> | nine modem presets with SF/BW/CR/data rates, regional duty-cycle notes (e.g. EU 869.4–869.65 MHz at 10 %) |
| <https://raw.githubusercontent.com/meshtastic/firmware/master/LICENSE> | GPL-3.0 |
| <https://raw.githubusercontent.com/meshtastic/firmware/master/README.md> | firmware targets: ESP32, nRF52, RP2040/RP2350, Linux |
| <https://meshcore.io/> | positioning, MIT license, companion-node model, apps/platforms |
| <https://docs.meshcore.io/> (index) | documentation map |
| <https://docs.meshcore.io/packet_format/> | 1-byte header, route types, path-length encoding, 184-byte payload, 64-byte max path |
| <https://docs.meshcore.io/payloads/> | advert, ACK, request/response, room-server login/sync, group text/datagram, control, custom |
| <https://docs.meshcore.io/companion_protocol/> | BLE NUS UUIDs, 176-byte frame cap, byte-command protocol, 133-character messages, channel slots |
| MeshCore repository checkout `75b04ef` | `license.txt` (MIT), `platformio.ini` (ESP32/nRF52/RP2040/STM32 envs, RadioLib pin), `src/MeshCore.h` (`MAX_PACKET_PAYLOAD 184`, `MAX_PATH_SIZE 64`), room-server example |
| OpenMesh repository | `docs/architecture/PROTOCOL.md`, `ARCHITECTURE.md`, `crates/openmesh-core` |

Airtime numbers are computed with the Semtech formula independently reimplemented for
[lora-capacity-and-media.md](lora-capacity-and-media.md) and cross-checked against the values in
PROTOCOL.md Appendix A on `MEDIUM_FAST`. They are estimates for comparison, not lab measurements.

---

## 2. The three systems in one paragraph each

**Meshtastic** is a mature, mass-market LoRa mesh for text, position and telemetry. Devices run
GPL-3.0 firmware (ESP32, nRF52, RP2040/RP2350, Linux) and are driven by phone/desktop clients
over BLE, serial, Wi-Fi or MQTT. Its packet format is a 16-byte cleartext routing header
(destination, sender, packet id, flags, channel hash, next-hop/relay for directed delivery)
followed by an encrypted protobuf payload of at most 239 bytes. Channels are identified by a
one-byte hash and encrypted with a PSK (AES-CTR by default, AES-CCM opt-in); direct messages use
X25519 + AES-256-CCM; broadcasts can be XEdDSA-signed. Routing is managed flooding with
SNR-weighted random rebroadcast delays, plus an optional directed path once a neighbour route is
known. There is no forward secrecy and no store-and-forward beyond a small buffer for a
currently-disconnected client.

**MeshCore** is a lighter, firmware-first C++ mesh (MIT, Arduino/PlatformIO on ESP32, nRF52,
RP2040, STM32) aimed at embedded developers. Its packet format is remarkably compact: 1-byte
header (route type, payload type, payload version), optional 4-byte transport codes, a path field
with 1–3-byte node hashes (max 64 bytes), and at most 184 bytes of payload. Node identity is a
32-byte Ed25519 public key, and the **node hash is the first byte of that key** — one-byte
addresses, which is why frames are so small. It supports flood and **direct routing** (returned
paths teach senders a route), adverts, requests/responses, anonymous room-server login with
timestamp-based sync, group text with a 1-byte channel hash, and a byte-command companion
protocol over a Nordic-UART-like BLE service (176-byte frames, serial console). Repeaters and
room servers are first-class roles; there is no general store-and-forward of encrypted DMs and no
media.

**OpenMesh** (this repository) is a LoRa-first mesh for text, images and voice notes with
zero-infrastructure operation. Its OMP/1 frame is a 22-byte header plus up to 218 bytes of body;
node ids are 8 bytes with a high-bit address-class rule separating unicast from channels.
Direct messages are XChaCha20-Poly1305 under a static ECDH key (one frame, no handshake); channels
are PSK-sealed with Ed25519-signed inner messages; routing is managed flooding; relays can store
encrypted envelopes for offline peers for 72 h; images and voice notes travel as chunked
resources with selective-repeat ARQ and mandatory pre-send airtime estimates. Clients are a
React/Tauri web-and-desktop app and a native Android app, both talking to a Rust bridge (WS/REST)
or a radio (companion API). Firmware is C++/PlatformIO for four boards (two nRF52840, two
ESP32-S3), and the protocol core is Rust with golden vectors and a discrete-event simulator.
License is AGPL-3.0.

---

## 3. Comparison table

Legend: ✔ native, ◐ partial/optional, ✘ absent.

### 3.1 Product and protocol

| Dimension | Meshtastic | MeshCore | OpenMesh |
| --- | --- | --- | --- |
| Primary goal | consumer LoRa messaging/position | embeddable mesh library + radios | LoRa mesh with rich media and offline delivery |
| Framing | 16-byte cleartext routing header + encrypted protobuf payload | 1-byte header + optional transport codes + path field + payload | 22-byte header + AEAD/CBOR body |
| Max on-air payload | 239 bytes (clients ~200) | 184 bytes | 218-byte body (240-byte frame) |
| Addressing | 32-bit node number = CRC-32(pubkey); broadcast `0xFFFFFFFF` | 1-byte hash = first byte of 32-byte Ed25519 key | 8-byte node id = SHA-256("openmesh/v1/node"‖keys) with class bit |
| Channel addressing | 1-byte channel hash | 1-byte channel hash = SHA-256(secret)[0] | 8-byte channel id = SHA-256("openmesh/v1/chan"‖key) with high bit set |
| Routing | managed flooding, optional directed next-hop | flood or **direct** with returned paths; transport codes for region scoping | managed flooding only; directed routing reserved for OMP/2 |
| Retransmission | ACK + retry (3 broadcast, 5 DM), adaptive interval | ACK (CRC-32 based), attempt counter in text frames | ACK + retry (3 data, resource ARQ with 6 retries) |
| Max hop budget | `hop_limit`/`hop_start` nibbles | route/path up to 63 hops, 1–3-byte hashes | TTL 4-bit (default 5, announce 8) |
| Duplicate suppression | packet id | per-route duplicates tracked | `(src, seq)` 30-min cache |
| Forward secrecy | ✘ | ✘ (static keys) | ✘ in v1, ratchet planned for OMP/2 |
| Identity model | key pair, node number derived | key pair, node hash = key prefix | dual key pair (Ed25519+X25519), self-certifying id |
| Store-and-forward | ◐ packet buffer 8/16/32 for disconnected client only | ◐ room-server posts with "sync since timestamp" | ✔ relay envelopes, offer/want handshake, 72 h TTL |
| Media (images/voice) | ✘ (text, position, telemetry, waypoints) | ✘ (text, telemetry, room posts) | ✔ images + Opus/Codec2 voice notes as resources |
| Airtime estimate before send | ✘ | ✘ | ✔ required by spec; shown in UI |
| Client model | phone/desktop app over BLE/serial/Wi-Fi/MQTT | companion radio over BLE (NUS) or serial; byte-command protocol | Rust bridge WS/REST + web/Tauri client; Android via companion API (USB/BLE) |
| Firmware targets | ESP32, nRF52, RP2040/RP2350, Linux | ESP32-S3/C6, nRF52, RP2040, STM32 | nRF52840 ×2, ESP32-S3 ×2 (see ARCHITECTURE §5) |
| Firmware language | C++ (Arduino/PlatformIO) | C++ (Arduino/PlatformIO) | C++17 (PlatformIO) + Rust core for host |
| License | GPL-3.0 | MIT | AGPL-3.0-or-later |
| Third-party stack | own protobufs, RadioLib etc. | own library + RadioLib | own protocol; no runtime dependency on Reticulum/Meshtastic |

### 3.2 Security and privacy

| Property | Meshtastic | MeshCore | OpenMesh |
| --- | --- | --- | --- |
| Channel encryption | AES-CTR with PSK (128/256-bit); AES-CCM opt-in (2.8.1+) | AES-256 (CBC/CTR per firmware) with 16-byte channel secret + 2-byte MAC | XChaCha20-Poly1305 with HKDF-derived channel key |
| Channel authenticity | unsigned by default; XEdDSA-signed broadcasts (2.8+) | no sender signature on group text; name is unverified text | Ed25519 signature inside the sealed message |
| DM encryption | X25519 + AES-256-CCM | X25519-derived key + AES + MAC | static X25519 → HKDF → XChaCha20-Poly1305 |
| DM authenticity | key agreement only (implicit) | signature in contact-message variant | AEAD tag (implicit) + identity in first message |
| Identity trust | TOFU; node number = CRC-32 of key | TOFU; 6-byte pubkey prefix in messages | self-certifying 8-byte id; full identity 72 B |
| Metadata exposed | dest, src, packet id, channel hash, hop count | dest/src 1-byte hashes, path bytes | src/dst 8-byte ids, TTL, seq, clear `msg_ref` |
| Public/known channel key | default channel key `AQ==` (public) | public channel key published | no default channel; channels are opt-in with random PSK |
| Forward secrecy | ✘ | ✘ | ✘ v1 (documented) |

### 3.3 Operational trade-offs

| Dimension | Meshtastic | MeshCore | OpenMesh |
| --- | --- | --- | --- |
| Header overhead per frame | 16 B header + protobuf wrapper | 1 B + path bytes + 1–3 B hashes | 22 B + 33 B AEAD envelope (55 B) |
| A 100-char text frame | roughly 200 B → ~1.7 s at LongFast (SF11/250) | roughly 120 B → ~0.75 s at default SF8/62.5 kHz | 201 B → 519 ms at `MEDIUM_FAST`; 1.75 s at `LONG_FAST` |
| Address collision risk | 32-bit: birthday bound at ~77k nodes | 8-bit: collisions likely above ~20 nodes per channel; mitigated by direct routing + per-hop path | 57-bit unicast: collision negligible at mesh scale |
| Network size assumption | hundreds–thousands of nodes, multiple channels | tens of nodes per channel, repeaters | tens–hundreds per region, flooding keeps it flat |
| Duty-cycle awareness | region presets and firmware limits | region presets, no per-frame airtime UX | profile+region model, token-bucket budget, pre-send estimate |
| Client polish | very mature ecosystem | newer, rapidly improving | goal of this project; scaffold stage today |
| Extensibility | protobuf ports/modules | payload types, companion commands | frame types 11–31 reserved; CBOR unknown-key tolerance |

### 3.4 What the numbers mean

A 100-character text message costs about 0.5 s of airtime on OpenMesh at `MEDIUM_FAST`, about
1.75 s on both OpenMesh `LONG_FAST` and Meshtastic `LongFast` (same SF11/250 preset, similar frame
size), and about 0.75 s on MeshCore's default narrowband SF8/62.5 kHz preset. Those are in the
same order of magnitude — the design space is real, not a marketing race. The differences show
up in *what else* the frame carries (a protobuf envelope, a path field, a signature) and in
*what the network does with it* (flood at one hop or all hops, store it, ack it).

OpenMesh's media feature is only honest because the airtime model is explicit: a 5 KiB image is
~18 s at `MEDIUM_FAST` but ~354 s at `LONG_SLOW` (see
[lora-capacity-and-media.md](lora-capacity-and-media.md)); the same image at Meshtastic's
LongFast would be ~9 minutes of airtime for 5 KiB, which is presumably why Meshtastic does not
offer it. The comparison is not "we are faster"; it is "we make the cost visible and give the
user small enough units to make a decision".

---

## 4. Meshtastic: lessons learned

### 4.1 What it gets right

1. **Managed flooding at scale.** The SNRo-weighted random rebroadcast delay is the single most
   important idea in the ecosystem: distant nodes relay first, nearby nodes hear them and drop
   their own copies, so a message propagates outward without a routing table and without a
   thundering herd. OpenMesh's flooding should adopt the SNR-weighted delay for relays (not just
   fixed jitter) — this is a concrete M1 improvement, not a protocol change.
2. **Channel model users understand.** One-byte channel hash + PSK, up to eight channels, a
   QR/URL that carries all radio settings. The "channel URL carries the LoRa preset" trick is
   excellent onboarding; OpenMesh should copy the *idea* (our `openmesh://channel?...` can carry
   profile/region in addition to the PSK).
3. **Directed delivery as an optimisation, not a requirement.** Meshtastic falls back to
   flooding when a next-hop relay does not respond. That is the right failure mode for a mesh
   and matches our decision to keep flooding as the baseline.
4. **Firmware breadth.** One C++ codebase across ESP32, nRF52, RP2040/RP2350 and Linux is a
   serious engineering achievement; it validates our four-board single-codebase plan.
5. **Honest documentation of limitations.** The encryption docs say plainly that there is no
   forward secrecy and that signed broadcasts add 66 bytes. OpenMesh should keep that tone.

### 4.2 What it gets wrong, or chooses not to do

1. **No media.** The 239-byte payload ceiling plus a protobuf wrapper makes images and voice
   notes impractical; even a compressed 2 KiB thumbnail needs 9–16 frames before overhead, with
   no resource ARQ layer. Meshtastic avoids the UX problem by not offering the feature. We need
   the feature, so we need the transport work (resources, ARQ, caps) — that is the main
   differentiator, and the main risk.
2. **No true store-and-forward.** The packet buffer only holds packets for a client that is
   currently disconnected; it is not an offline inbox with a TTL. OpenMesh's relay envelopes are
   a genuine feature difference, and the user-visible promise must be precise ("a repeater may
   hold it for up to 72 h; this is not proof of delivery").
3. **Opaque airtime.** Nothing in the client tells you what a message will cost or how busy the
   channel is. Users on 1 % duty-cycle bands have no idea why messages fail. Our pre-send
   estimate is a product decision backed by the spec.
4. **TOFU with weak address binding.** Node number = CRC-32(public key) and nodes trust the first
   key they hear. Our self-certifying 8-byte id costs 4 more bytes but removes the "wrong key for
   a node number" class of bug.
5. **GPL-3.0 firmware and a large upstream.** Embedding Meshtastic code in an AGPL project is
   possible (GPLv3 §13 permits combining with AGPLv3), but we do not want the dependency; also
   the mobile app ecosystem is separate, not shared with our web-first client.

### 4.3 What interop would require

A Meshtastic gateway would translate at the *protobuf* boundary:

| Step | Difficulty | Notes |
| --- | --- | --- |
| Map node numbers ↔ 8-byte ids | low | maintain a `crc32(pubkey) → node_id` table; CRC collisions are possible but rare |
| Map channels | low | 1-byte hash ↔ channel id; PSK must be shared out of band; AES-CTR vs XChaCha re-encryption at the gateway |
| Map DMs | medium | gateway needs one side's private key to decrypt/re-encrypt; or a "bridge identity" that is a Meshtastic peer and an OpenMesh peer |
| Map text/position/telemetry | medium | protobuf port numbers vs CBOR `AppMessage`; straightforward, lossy for unknown fields |
| Map ACKs/delivery | medium | different semantics (Meshtastic ack is a routing-level packet); gateway must synthesize |
| Media | impossible | Meshtastic cannot represent a 12 KiB image as a message; would require a separate app-level port and firmware change |
| Store-and-forward | N/A | Meshtastic has no equivalent to translate |
| Legal | workable | GPL-3.0 bridge process is fine next to AGPL code; do not link GPL firmware into the AGPL core |

**Verdict:** a Meshtastic text/position gateway is feasible as an M5 community project but is not
on the critical path, and the media asymmetry means it will never be a full interop story.

---

## 5. MeshCore: lessons learned

### 5.1 What it gets right

1. **Direct routing with returned paths.** A receiver learns the path a flood took (path bytes
   are recorded by relays), returns it to the sender, and subsequent messages are source-routed
   hop by hop. This is a large airtime win in line-of-sight repeater chains, and it needs no
   global routing protocol. OpenMesh reserves `RouteRequest`/`RouteResponse` for exactly this;
   MeshCore is the proof it can be simple. See [ADR 0002](../adr/0002-managed-flooding.md) for why
   we still ship flooding first.
2. **Tiny frames.** 1-byte header, 1-byte hashes, 2-byte MAC. A 100-char message is ~120 bytes on
   the wire. The trade-off is address collision: with one-byte node hashes, a room of >20 nodes
   has a meaningful chance of two sharing a byte. MeshCore mitigates with direct routes and
   per-hop path matching rather than by growing the address. We should *not* copy 1-byte
   addresses (our store-and-forward depends on globally unique 8-byte ids), but we should keep
   asking whether every header byte earns its place.
3. **Roles are explicit and cheap.** Companion, repeater, room server, sensor — each is an
   example application on the same library. Our `NodeClass` enum mirrors this; the lesson is to
   keep the radio code role-agnostic and put role behaviour in a small policy layer.
4. **Room servers are a simple, useful primitive.** Posts are stored in a cyclic queue; clients
   log in anonymously with "sync since timestamp" and receive new posts. It is not encrypted
   end-to-end store-and-forward, but it is enough for a community board and easy to reason
   about. OpenMesh's `channel_history` resource kind in OMP/1 anticipates the same use case;
   MeshCore's timestamp-cursor sync is a good protocol for M5 room servers.
5. **Companion node model matches ours.** One BLE/serial link, device owns the radio, app owns
   the UI. Our companion API is more structured (typed CBOR messages + CRC framing vs raw
   byte commands), which is worth it for an app ecosystem.

### 5.2 What it gets wrong, or chooses not to do

1. **Unverified group messages.** Group text carries a sender name as unverified text and no
   signature; anyone with the channel key can impersonate anyone. OpenMesh's Ed25519 inner
   signature costs 64 bytes per group message but is the right call for a network that wants to
   carry community/room traffic.
2. **No media.** Same story as Meshtastic, for the same reason: small frames, no ARQ layer.
3. **Address entropy.** 8-bit hashes are fine for a single channel with a few dozen nodes and
   direct routes, but they do not compose with store-and-forward or multi-region networks.
4. **Companion protocol fragility.** One frame per BLE write, one command at a time, 5 s
   timeouts, manual command queues, no general length-prefixed framing layer, and inconsistent
   constants (payload limit 165 vs 167 vs 176 across layers, documented as known discrepancies).
   Our length+CRC framing with a single max payload is more robust; MeshCore's docs are honest
   about their own inconsistencies, which is a useful warning.
5. **No explicit airtime budgeting.** Region presets exist, but per-message cost is not surfaced.

### 5.3 What interop would require

There are two plausible shapes, both hard:

* **Packet-level bridge.** Translate OMP frames ↔ MeshCore packets. Blocked by the 1-byte node
  hash (cannot represent an 8-byte id without a table and collisions), by routing semantics
  (path bytes vs TTL), and by encryption (different schemes and MAC sizes). A gateway could
  maintain a per-neighbour hash table, but it would be a stateful translation proxy for a
  256-node address space — fragile.
* **Companion-protocol impersonation.** Make OpenMesh firmware answer MeshCore's companion
  commands so MeshCore apps can drive our radios. This is more feasible technically (the app
  side is documented and MIT) but it means implementing someone else's UI contract forever, and
  it exposes none of OpenMesh's differentiators (media, store-and-forward) through that app.

**Verdict:** no MeshCore interop on the roadmap. Borrow the direct-routing idea for OMP/2 and
keep the companion API clean; watch MeshCore's growth as a signal for how much of the market
wants "small and simple" versus "rich media".

---

## 6. Where OpenMesh differentiates

### 6.1 Images and voice with airtime honesty (primary)

No other LoRa mesh in this comparison ships first-class images and voice notes. The enabling
pieces are all in OMP/1: a resource layer with selective-repeat ARQ, attachment storage on the
bridge, client-side compression, and a per-profile/per-region airtime estimator that is shown
before sending and enforced by caps. The differentiation is not "we can send a 5 KiB image" —
it is "we tell you it costs 18 s of airtime at `MEDIUM_FAST`, 6 minutes at `LONG_SLOW`, and here
are the thumbnails-first options". Meshtastic's and MeshCore's docs avoid media because the
airtime story is bad; we lean into it.

### 6.2 Store-and-forward repeaters (primary)

An encrypted envelope held for 72 h and offered when the addressee next appears is a genuinely
different capability. It must be described precisely: relays cannot read or forge; storing is
not delivery; media storage is quota-limited (text store-and-forward by default). This is the
feature that makes a solar repeater on a ridge useful for hikers who check in once a day.

### 6.3 Client quality across web, desktop and Android (primary)

Meshtastic's app ecosystem is mature but platform-specific; MeshCore's is newer. One React
codebase for web + Tauri desktop, plus a native Compose Android app sharing the same protocol
semantics (via bridge or companion), means conversation lists, maps, contact QR, media
playback and airtime UI behave the same everywhere. That is a product bet, not a protocol
feature, and it is where most of the remaining work in this repository is.

### 6.4 Multi-transport bridge (secondary)

The bridge speaks LoRa serial, UDP broadcast, TCP and a simulator with the same frames. That
makes a Wi-Fi link between two homes or an internet tunnel a first-class transport without
changing the protocol, and it makes demos/CI possible with no hardware. Meshtastic has MQTT and
Wi-Fi but treats them as separate gateways; MeshCore has KISS modems and WiFi-capable boards but
a simpler transport matrix.

### 6.5 Engineering honesty as a feature (secondary)

Golden vectors across Rust and C++, an IO-free deterministic core, a discrete-event simulator
with assertions, and published ADRs including the ones where we say "no". This does not sell
radios, but it is what makes a fourth mesh credible to reviewers and contributors — and it is
why an independent size recomputation caught two protocol bugs in one afternoon (see
[ai-development-workflow.md](ai-development-workflow.md) §6).

### 6.6 What OpenMesh is *not*

* Not a Meshtastic replacement at scale (no 100k-node ambitions, no protobuf ecosystem).
* Not a MeshCore replacement for tiny embedded projects (our frames and stack are bigger).
* Not a Reticulum clone (no links/paths/ratchets on the MCU; a bridge is planned for M4).
* Not an anonymity network (metadata is visible; documented in PROTOCOL §17).
* Not a real-time voice system in v1 (voice messages only; calls gated on OMP/2 + ratchets).

---

## 7. Cross-cutting lessons (what a fourth mesh must get right)

1. **Airtime is the product constraint.** Every feature must be expressible in seconds of
   airtime and have a UX for "too expensive". Meshtastic's and MeshCore's feature sets are
   shaped by this; ours must be too, explicitly.
2. **Flooding is the only routing that works everywhere.** Direct routing is an optimisation
   with a fallback, never a requirement. Design store-and-forward and media so they degrade
   gracefully when only flooding is available.
3. **Address length is a system-wide choice.** Meshtastic's 32-bit ids buy store-and-forward and
   multi-channel clarity; MeshCore's 1-byte hashes buy tiny frames; our 8-byte ids buy
   self-certification and store-and-forward. Do not mix assumptions later.
4. **Security defaults matter more than algorithms.** MeshCore's group messages are unverified
   and Meshtastic's default channel key is public; users still treat them as private. OpenMesh
   should default to signed channel messages and no public channel, and say so in the UI.
5. **Channel onboarding is the hard UX problem.** QR/URL invites (Meshtastic) are the bar; ours
   must carry channel PSK + name + profile/region and work offline.
6. **Companion protocol quality decides the app ecosystem.** MeshCore's byte commands are easy
   to implement badly; our typed CBOR + CRC framing is a better foundation, and the FFI/Android
   work should keep the same discipline.
7. **Licenses decide what can be shared.** GPL-3.0 (Meshtastic), MIT (MeshCore), AGPL-3.0
   (OpenMesh), Reticulum/LXST licenses (incompatible with reuse). Any bridge story must start
   with the license boundary, not end with it.

---

## 8. Interop roadmap (recommendation)

| Target | Milestone | Shape | Realistic? |
| --- | --- | --- | --- |
| Reticulum / LXMF / NomadNet | M4 | gateway process speaking OMP ↔ Reticulum; text + small attachments; identity mapping documented | yes — see [nomadnet-bridge.md](nomadnet-bridge.md) |
| Meshtastic | M5 (community) | text/position-only gateway; protobuf boundary; no media | maybe; needs a champion |
| MeshCore | not planned | packet bridge blocked by 1-byte hashes; companion impersonation possible but strategically weak | no |
| OpenMesh ↔ OpenMesh over UDP/TCP | M0/M1 | same frames over Wi-Fi/Ethernet; bridge already supports it | yes (already scaffolded) |

The principle: interoperate where a *bridge process* can own the other stack's complexity, not
by compromising OMP/1's frame budget. That is the same reason OpenMesh rejected Reticulum
compatibility on the wire in [ADR 0001](../adr/0001-own-protocol-vs-reticulum.md).

---

## 9. Open questions

1. Should OMP/2 direct routing copy MeshCore's returned-path model exactly, or use a
   Reticulum-style path request with a transport-node table? The former is simpler on MCUs; the
   latter scales better across regions.
2. Should OpenMesh adopt SNR-weighted relay delay in v1.1? It is a firmware-only change (no wire
   change) and likely improves dense-network behaviour; needs simulator support first.
3. Is a Meshtastic gateway worth a permanent maintenance burden for text-only interop? Decide
   after M1 field data on how many users are dual-network.
4. How should the UI represent a channel that is "public by design" if we ever add a discovery
   channel? Meshtastic's default public key is a cautionary tale; MeshCore's hashtag channels
   make the trade-off explicit. Prefer explicit.
5. Can resource transfers reuse Meshtastic-style protobuf port numbers for interop, or is CBOR
   strictly better for our firmware? CBOR is already chosen; do not revisit without a measured
   win.

---

## 10. Sources (all read for this note)

* Meshtastic overview — <https://meshtastic.org/docs/overview/>
* Meshtastic encryption — <https://meshtastic.org/docs/overview/encryption/>
* Meshtastic radio settings — <https://meshtastic.org/docs/overview/radio-settings/>
* Meshtastic firmware license (GPL-3.0) —
  <https://raw.githubusercontent.com/meshtastic/firmware/master/LICENSE>
* Meshtastic firmware README —
  <https://raw.githubusercontent.com/meshtastic/firmware/master/README.md>
* MeshCore home page (MIT claim, companion-node model) — <https://meshcore.io/>
* MeshCore docs index — <https://docs.meshcore.io/>
* MeshCore packet format — <https://docs.meshcore.io/packet_format/>
* MeshCore payload format — <https://docs.meshcore.io/payloads/>
* MeshCore companion protocol — <https://docs.meshcore.io/companion_protocol/>
* MeshCore source checkout `75b04ef`: `license.txt`, `platformio.ini`, `src/MeshCore.h`,
  `examples/simple_room_server/MyMesh.cpp`
* OpenMesh: `docs/architecture/ARCHITECTURE.md`, `docs/architecture/PROTOCOL.md`,
  `crates/openmesh-core/src/{util,companion,services}.rs`
* Airtime: independent Semtech-formula implementation, cross-checked against PROTOCOL.md
  Appendix A; details in [lora-capacity-and-media.md](lora-capacity-and-media.md)

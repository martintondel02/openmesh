# NomadNet ⇄ OpenMesh bridge — M4 design

**Status:** design proposal for M4 · **Last updated:** 2026-11-25
**Owner:** platform/docs workstream · **Depends on:** OMP/1 stable, bridge WS/REST API,
repeater store-and-forward tuning (M1).

This document specifies the `openmesh-gateway` node: a process that lets OpenMesh users exchange
direct messages with Reticulum/LXMF users and browse NomadNet pages, without putting Reticulum
on the mesh and without compromising OMP/1's frame budget. It is intentionally concrete
(process boundaries, tables, phases, tests) because M4 is where most bridge projects fail: the
protocol translation is easy, the identity and trust model is not.

Related:

* [ARCHITECTURE.md](../architecture/ARCHITECTURE.md) §10 — the decision this implements.
* [Reticulum/LXMF research notes](reticulum-lxmf-notes.md) — protocol facts and license rules.
* [PROTOCOL.md](../architecture/PROTOCOL.md) — OMP/1 envelopes, resources, store-and-forward.
* [WEBSOCKET-API.md](../architecture/WEBSOCKET-API.md) — bridge API the gateway rides on.
* [ADR 0001](../adr/0001-own-protocol-vs-reticulum.md),
  [ADR 0005](../adr/0005-companion-radio-architecture.md),
  [ADR 0008](../adr/0008-agpl-3-0-licensing.md).

---

## 1. Scope

### 1.1 In scope (M4)

1. **Bidirectional direct messages** between an OpenMesh node and an LXMF destination:
   OpenMesh → LXMF (new message, reply) and LXMF → OpenMesh (new message, reply, delivery
   proof surfaced as an OpenMesh ACK-like event).
2. **Attachments in both directions** with hard size caps, re-encoded through both stacks
   (never ciphertext-passthrough), and clear UX about when the transfer can work.
3. **NomadNet page and file browsing** for OpenMesh clients: the gateway fetches pages over
   Reticulum and exposes them through the bridge API as a structured page model; OpenMesh web
   and Android clients render it.
4. **OpenMesh-origin pages**: the gateway serves a documented Micron subset generated from
   OpenMesh channel history / attachments so that NomadNet users can read OpenMesh room
   content (M4.4; room-server history itself is M5).
5. **Propagation-node integration** (optional, opt-in): the gateway can use an LXMF propagation
   node as its outbound store-and-forward for messages when the peer is offline.

### 1.2 Explicit non-goals (M4)

* No OMP frames on Reticulum and no Reticulum packets on LoRa. The two protocols never share a
  wire format; the gateway terminates both.
* No end-to-end encryption across protocols. Re-encryption implies the gateway sees plaintext;
  the UI and docs must say so on every bridged conversation.
* No group/channel bridging in M4. OMP channels are PSK groups; LXMF has no equivalent channel
  primitive, and a bridge that pretends otherwise would be a security hole. Channels map to
  pages/room content (one-way publishing) at most. Group bridging is an M5 research item.
* No real-time LXST calls. Calls need continuous throughput (see
  [reticulum-lxmf-notes.md](reticulum-lxmf-notes.md) §4.2); not a gateway feature.
* No identity delegation with legal weight: a bridged message is signed by the gateway's
  identity, annotated with the OpenMesh id, not cryptographically signed by the OpenMesh user.
* No use of Reticulum-licensed or LXST-licensed code. The gateway depends on the Python
  reference implementation (`rns`, `lxmf`, optionally `nomadnet`) as a *separate process* or
  on publicly documented wire behaviour implemented independently.

### 1.3 Definitions

| Term | Meaning here |
| --- | --- |
| OMP side | The mesh: OpenMesh nodes, bridge, repeaters. |
| RNS side | A Reticulum instance/transport with an LXMF client and optionally a NomadNet page node. |
| Gateway | This process; owns a Reticulum identity and an OMP identity; performs translation. |
| Shadow identity | A Reticulum key pair the gateway generates for an OpenMesh node so that LXMF peers have a stable destination to address. |
| Bridge contact | An LXMF destination represented to OpenMesh clients as a contact (with a gateway-owned identity) so they can send to it. |
| Page node | A NomadNet-style Reticulum service registering `/page/...`, `/file/...` request handlers. |
| Micron | NomadNet's text markup language. |

---

## 2. Background: what NomadNet actually is

NomadNet is a Reticulum application, not a protocol. From its README and page-server source
(`nomadnet/Node.py` in the public mirror):

* It is built on **LXMF** for messaging and **Reticulum** for transport; it runs over packet
  radio, LoRa, serial, Wi-Fi or the internet without a central service.
* A **page node** is an LXMF/Reticulum destination that registers request handlers for paths.
  Observed paths: `/page/<file>` (`.mu` page files), `/file/<file>` (downloads), `/media` (image
  conversion/serving endpoint), plus a default `/page/index.mu`.
* Pages are **Micron** markup files. Pages may be static files or executable programs (the
  node runs them and returns stdout, passing `link_id`/`remote_identity` and request fields as
  environment). Access can be restricted with per-file `.allowed` lists.
* It includes a built-in browser, page caching, a message-store for offline users (LXMF
  propagation), a file-sharing browser and an RRC live chat client.
* NomadNet is licensed **GPLv3** (per its README). It is a sibling application, not a library we
  can assume.

Two consequences for the design:

1. **Pages are just request/response over a Reticulum link.** To fetch a page, the gateway
   establishes a link to the node destination and sends a request for the path; the response is
   bytes (Micron text or a file). The gateway does not need NomadNet internals — but it does
   need to speak Reticulum link/request semantics, which is exactly what the reference stack is
   for.
2. **Page content is untrusted.** Remote nodes can serve arbitrary bytes, including terminal
   escapes and enormous responses. The gateway MUST treat page content as hostile input: size
   caps, timeouts, sanitisation, no execution, no automatic link following.

---

## 3. Roles and process architecture

```
   OpenMesh web/Android clients
             │  WS/REST (existing bridge API + gateway endpoints)
             ▼
   ┌──────────────────────┐        ┌────────────────────────────┐
   │ openmesh-bridge      │◄──────►│ openmesh-gateway (M4)      │
   │ core engine + radio  │  local │  - OMP adapter             │
   │ store, attachments   │  IPC   │  - identity registry       │
   └──────────┬───────────┘        │  - LXMF translator         │
              │ LoRa/UDP/TCP       │  - page proxy + cache      │
              ▼                    └──────────┬─────────────────┘
        OpenMesh mesh                          │ Reticulum shared instance / TCP
                                               ▼
                                    ┌──────────────────────────┐
                                    │ rnsd (Reticulum daemon)  │
                                    │  ├─ TCP/Wi-Fi/LoRa ifaces│
                                    │  └─ NomadNet page nodes  │
                                    └──────────────────────────┘
                                               │
                                    LXMF peers, propagation nodes
```

### 3.1 Why a sidecar process, not a linked library

| Option | Verdict | Reason |
| --- | --- | --- |
| Link rsReticulum/rsLXMF crates into the gateway | reject | Reticulum License restricts use (field-of-use, AI-training) and is incompatible with AGPL-3.0 redistribution; the Rust ports also use sibling path dependencies and are experimental. |
| Link Python `rns`/`lxmf` via embedding | reject | Same license conflict plus Python embedding complexity; the reference stack is a GPL-incompatible-license dependency for us. |
| Run `rnsd` (Python or rsReticulum) as an external daemon and speak its **shared-instance/RPC** interface | **choose** | The daemon is a separate program with its own license; we interact over a documented local socket. The gateway stays AGPL-3.0, the Reticulum stack keeps its own license and lifecycle. |
| Implement Reticulum wire from scratch inside the gateway | reject for M4 | Link/resource/announce/ratchet state machines are a large project; a bridge is not the place to re-litigate them. |

The gateway therefore has three adapters:

1. **OMP adapter**: connects to `openmesh-bridge` over its existing WebSocket/REST API, or to a
   radio over the companion API when no bridge is present (Android gateway variant). It uses
   bridge events (`message`, `ack`, `resource_progress`, `node`) and commands
   (`send_message`, `add_contact`, `get_messages`, `POST /api/attachments`).
2. **RNS adapter**: connects to a local Reticulum shared instance (default ports 37428/37429)
   or a TCP interface configured point-to-point, and uses an LXMF router identity owned by the
   gateway. In M4 we may either implement this adapter against the Python `lxmf`/`rns` APIs in a
   small helper process, or against a *documented wire profile* we implement independently; the
   former is faster, the latter is license-clean. **Decision needed at M4 kickoff** (see §11).
3. **Page proxy**: uses Reticulum request/response over a link (the same RNS adapter) to fetch
   `/page/...` and `/file/...`, with a cache and size/time limits.

### 3.2 Roles

| Role | Runs where | Responsibilities |
| --- | --- | --- |
| OMP node | phone / handheld | end-user app; OMP identity; sends/receives DMs, resources |
| Bridge | desktop / gateway host | OMP store, radio, WS/REST API, attachments |
| Gateway | same host as bridge (recommended), or small VPS with a radio | translation, identity registry, RNS adapter, page proxy, rate limits |
| RNS daemon | gateway host or LAN | interfaces, announce handling, link/resource state |
| Page node | any Reticulum node | serves Micron pages and files |
| LXMF propagation node | any Reticulum node | store-and-forward for LXMF when peers are offline |

The gateway SHOULD be co-located with the bridge: it needs low-latency access to messages and
attachments, and the bridge API binds loopback by default. A remote gateway is possible but
requires the bridge's `--bind` escape hatch and a documented threat model (WEBSOCKET-API §1).

---

## 4. Identity and address mapping

This is the hard part. OMP has 8-byte node ids derived from an Ed25519+X25519 key pair; Reticulum
has 16-byte destination hashes derived from an app name and a 64-byte identity public key. LXMF
message signatures are made by the sender's Reticulum identity, so a bridge cannot create a
message "from" an OpenMesh user without holding a Reticulum private key that represents them.

### 4.1 Identity models

| Model | How it works | Pros | Cons |
| --- | --- | --- | --- |
| **A. Shadow identities (recommended default)** | Gateway generates a Reticulum identity per OpenMesh node and stores it encrypted; the LXMF destination is derived from that identity (`lxmf.delivery` name + identity hash). Gateway signs bridged messages as that shadow identity. | Zero setup for users; stable destination per node; gateway can receive replies and forward them; one gateway can serve a whole mesh. | Gateway holds keys for the user's bridged identity → gateway is trusted; destination is not the user's "real" Reticulum identity. |
| **B. User-supplied identity (advanced)** | User imports/derives a Reticulum identity into the gateway (or the phone holds it and the gateway only relays). | User owns the identity; messages verifiable by third parties as theirs. | Key management UX; importing a private key into a shared gateway is a real risk; not feasible on MCUs. |
| **C. Gateway identity only** | All bridged traffic is signed by one gateway identity, annotated with the OpenMesh node id in a custom field. | Simplest; clear trust statement. | LXMF peers see one identity; replies to different users collide; no per-user continuity. |

**Recommendation:** Model A by default, with Model B as an import path in M5, and a visible
"bridged" badge in every client conversation. Model C is only for a one-way "OpenMesh public
feed" page.

### 4.2 Address mapping table

The gateway keeps a persistent table (redb or SQLite; not the bridge's message store):

| Mapping | Rule | Notes |
| --- | --- | --- |
| OMP node id → shadow Reticulum identity | Generate on first bridge use; store `identity` (64-byte private) encrypted with a gateway key; cache destination hash | identity file format must match the Reticulum stack we use (`rns` identity file). |
| OMP node id → LXMF delivery destination | `Destination(lxmf.delivery, shadow_identity)` → 16-byte hash | The hash is derived by the RNS stack; never compute it differently. |
| OMP node id → display name | From contact/announce name; included in LXMF title/app_data as a hint | Name is user-controlled text; sanitise. |
| LXMF source destination → OMP node id | Only for destinations the gateway created (shadow identities) or explicitly "bridge contacts" imported by an admin | Unknown LXMF senders have no OMP id; they appear as bridge contacts (next row). |
| Unknown LXMF peer → OMP bridge contact | Gateway creates a **bridge contact proxy**: an OMP identity held by the gateway (Ed25519+X25519) plus a display name (LXMF peer name or hash prefix) | OMP senders encrypt to this proxy; the gateway decrypts and forwards. This makes the gateway the endpoint for that conversation. |
| OMP channel id → page namespace | `openmesh://channel/<channel_id_hex>` served as `/page/openmesh/<channel>/...` (M4.4, read-only) | No channel-key bridging. |
| OMP attachment id → LXMF attachment | Re-encode into LXMF field 0x05 (file) or 0x06 (image), base64-free binary payload | Size caps in §5.4. |
| OMP msg_id (u32) → LXMF message hash (32 B) | Table `(peer, msg_id) ↔ lxmf_hash` + `(peer, transient_id)` for propagation | Needed for ACKs/read receipts and dedup across retries. |
| OMP node id → local bridge `node_id` string | Same value; clients already use hex | No translation needed on the OMP side. |

### 4.3 Deterministic vs random destinations

A shadow destination is derived from a randomly generated identity. Do **not** try to derive a
Reticulum private key deterministically from the OMP node id: Reticulum identities are X25519
static keys, and a deterministic derivation would let anyone who knows the OMP id compute the
Reticulum key (the OMP id is public, derived from public keys). The gateway must generate and
store a secret.

### 4.4 What the mapping cannot preserve

| OMP property | LXMF/Reticulum reality | Consequence |
| --- | --- | --- |
| Self-certifying identity continuity | Shadow identity is gateway-generated; user's OMP id is only an annotation | A bridged conversation is with the gateway, not cryptographically with the user |
| End-to-end encryption between OMP peers | Gateway decrypts to re-encrypt | The gateway is in the trust boundary; document it everywhere |
| Forward secrecy (OMP/2 plan) | LXMF opportunistic/direct has none in current versions; ratchets are per-destination and optional | Bridged DMs cannot promise FS; UI must not show the "sealed" badge |
| Channel semantics (PSK groups) | LXMF has no channel primitive | No channel bridging in M4 |
| OMP ACKs / read receipts | LXMF delivery proofs exist, read receipts are application-level | Map delivery proof → OpenMesh "delivered" event; never claim read unless a field says so |
| OMP TTL/flood metadata | Not represented | Gateway must not pretend to know hop counts; expose RNS RTT instead |
| Attachments up to 24 KiB policy | LXMF recipient size limits (often 1 KB by default in the daemon), propagation node quotas | Cap bridged attachments to what the LXMF side accepts (§5.4); direct-link-only for larger |
| Position/telemetry | LXMF has telemetry fields/apps, but formats differ | Map position only as a text/custom field hint, or defer to M5 |
| Voice notes | LXMF field 0x07 audio exists, but codec containers differ | Defer; do not transcode audio in the gateway in M4 |

---

## 5. Message translation

### 5.1 OMP → LXMF

Input: bridge event `message` with `peer`, `from`, `text`, optional `attachment`, plus the
stored contact. Steps:

1. Resolve `peer` → shadow identity + LXMF destination. If absent, generate it if the user has
   enabled bridging for that contact; otherwise emit an event `bridge_unavailable` and do not
   send.
2. Build an LXMF message:
   * `destination = lxmf_delivery_dest(peer)`
   * `title = ""` (or a short app title; titles cost bytes)
   * `content = text` (UTF-8 bytes)
   * fields:
     * `0xFB` custom type: `"openmesh"` (bytes)
     * `0xFC` custom data: CBOR `{node_id, name, msg_id, sent_at_ms}` — lets an
       OpenMesh-aware receiver display provenance; other LXMF clients ignore it
     * `0x30` reply-to: mapped LXMF hash when `reply_to` is set and known
     * `0x05`/`0x06`: attachment metadata + bytes (see §5.4)
3. Choose delivery method by policy: opportunistic if the packed message fits one Reticulum
   packet and no attachment; direct if larger and a path exists; propagated if enabled and
   configured (attachment over propagation is usually rejected by default quotas — surface the
   error rather than silently switching).
4. Sign with the shadow identity; retain the message hash ↔ OMP msg_id mapping.
5. On the OMP side, do **not** mark the OMP message delivered until the LXMF send reports a
   delivery proof; until then report `sent` with a "bridge" qualifier (the bridge API's
   `delivered` flag must remain false). The gateway synthesises an OMP `Ack{delivered}` message
   back to the sender's node when the LXMF proof arrives (as a direct Data frame from the
   shadow identity? No: the sender's node must receive a frame addressed to it from a known
   identity). Design choice: the gateway, which owns the OMP adapter, sends an OMP `Ack` frame
   on behalf of the shadow OMP identity of the peer. That identity is a real OMP key pair the
   gateway holds for the bridge contact proxy. The sender sees "delivered by <bridge>" — a
   normal ACK with an annotation in the UI from the bridge.

### 5.2 LXMF → OMP

Input: an incoming LXMF message (delivered opportunistically or directly, or retrieved from a
propagation node). Steps:

1. Verify the LXMF signature. If invalid, drop and count; never translate.
2. Look up the source destination in the mapping table:
   * known shadow identity → map to its OMP node id (a reply to a bridged conversation);
   * known bridge contact → map to the contact proxy's OMP id;
   * unknown → apply policy: drop with a log, or create a bridge contact proxy on the fly if
     `allow_inbound_bridge_contacts` is enabled (default: on for a dedicated gateway, off for a
     personal one).
3. Convert:
   * `content` → `AppMessage{kind: Text, text: ...}`
   * `title` → prefix the text only if non-empty and not duplicated (LXMF has no title field in
     OpenMesh)
   * `reply_to` (field 0x30) → resolve LXMF hash to OMP msg_id if known, else drop the thread
     hint
   * custom field 0xFC → if it contains an OpenMesh node id, use it for attribution display;
     never trust it for identity
   * attachment fields → download the referenced data from the LXMF resource if present in the
     message (LXMF file fields usually carry the bytes inline for small files; large files use
     resources managed by the LXMF stack) → store via `POST /api/attachments`, reference in the
     OMP message
4. Send to the mapped OMP contact through the bridge API. The bridge seals with the recipient's
   key, so the gateway must use an OMP identity that is a contact of the recipient (the bridge
   contact proxy) — or, for replies to an OpenMesh-origin conversation, the shadow identity
   that already represents the OpenMesh user to LXMF. Both are identities the gateway holds.
5. Surface the LXMF delivery proof to the OpenMesh sender as an ACK-like event only if the reply
   came from an identity that the OpenMesh node knows; otherwise attach a `bridge` flag in the
   message metadata.

### 5.3 Delivery state mapping

| LXMF state | OMP presentation |
| --- | --- |
| packed/queued locally | `send_result` `queued: true`; UI "waiting for bridge" |
| link established, resource in progress | progress event; UI progress bar in airtime terms |
| delivery proof received | `Ack{kind: delivered}` synthesised by the gateway; UI tick + "via bridge" |
| failed/timeout | OMP `Event("bridge_send_failed", detail)`; UI retry |
| propagation node accepted | **not** delivery; UI "stored at relay (not delivered)" |
| retrieved from propagation | normal incoming message, no ACK semantics invented |

The rule: **never map "stored at a propagation node" to OMP "delivered"**. This is the same
honesty rule PROTOCOL §10 applies to OMP store-and-forward.

### 5.4 Attachments and resources

OMP resources are AEAD-sealed blobs chunked at 180 bytes with selective-repeat ARQ. Reticulum
resources are a different framing. The gateway therefore cannot pass OMP chunks through; it
must reassemble the OMP resource (the bridge already has the plaintext after decryption), then
hand bytes to LXMF, which will pack them into its own message/resource. On the way back, the
LXMF library gives bytes; the gateway POSTs them as an attachment.

Recommended M4 caps:

| Path | Cap | Rationale |
| --- | --- | --- |
| OMP → LXMF attachment, direct link | 8 KiB | Fits a direct resource without multi-minute LoRa airtime; matches our 6–12 KiB caps and keeps bridge memory bounded |
| OMP → LXMF attachment, propagated | 1 KiB (warn below 2 KiB) | The LXMF daemon default accepted size is 1 KB; larger messages are commonly rejected |
| LXMF → OMP attachment | 8 KiB | Same; larger payloads can be fetched as files via NomadNet page/file paths instead |
| File browsing/download | 64 KiB per file, 1 MiB cache budget | Page/file proxy limits; user-initiated downloads only |
| Image formats | pass-through `image/jpeg`, `image/png`, `image/webp` | No transcoding in M4; M5 may downscale for bandwidth |
| Audio | not bridged in M4 | LXMF audio containers and OMP voice notes use different packaging; a lossy transcode is a product decision, not a protocol one |

If either side cannot accept the attachment, the gateway MUST fail the send with a clear error
("the LXMF peer cannot accept a 12 KiB attachment; send a page link or reduce to under 1 KiB")
rather than silently sending a text-only message.

---

## 6. NomadNet page and file proxy

### 6.1 Fetch flow

OpenMesh client → bridge REST `GET /api/bridge/page?dest=<rns_dest>&path=/page/index.mu` →
gateway → Reticulum link → page node → response bytes → gateway sanitises/parses → JSON page
model → client renders.

```
1. Resolve destination: known page node from announces, or user-provided 32-hex destination.
2. Reject if destination is blackholed or not a known LXMF/page node (allowlist policy).
3. Establish/reuse a Reticulum link (keep a small LRU of links; close idle links after 10 min).
4. Request the path with a timeout (default 20 s) and a response size limit (256 KiB).
5. Classify response: Micron text (content-type or `.mu` extension) vs file bytes.
6. For Micron: parse the allowed subset, produce a structured document, sanitise all text
   (strip ANSI escapes, control characters, overly long lines).
7. Cache with ETag = SHA-256(response), TTL 10 min; serve from cache with `stale` marker.
8. Emit `bridge_page_fetched` event with duration, bytes, cache hit, and link cost estimate.
```

Page files can be executable on the remote node. The gateway never executes anything; it only
fetches. It MUST NOT follow links automatically (no crawling), and it SHOULD show the requesting
user the destination hash, response time and byte count.

### 6.2 Structured page model

Micron is a line-oriented markup. The gateway parses a conservative subset into a model both
clients can render:

```json
{
  "dest": "0123456789abcdef0123456789abcdef",
  "path": "/page/index.mu",
  "title": "Node page",
  "blocks": [
    {"kind": "heading", "level": 1, "text": "Welcome"},
    {"kind": "text", "spans": [{"text": "open "}, {"text": "docs", "bold": true}]},
    {"kind": "link", "label": "files", "url": ":/page/files.mu", "external": false},
    {"kind": "bullets", "items": ["first", "second"]},
    {"kind": "rule"},
    {"kind": "literal", "text": "verbatim block"}
  ],
  "truncated": false,
  "fetched_at_ms": 1760000000000,
  "source_bytes": 812
}
```

Parsing rules for the subset (derived from the Micron conventions used by NomadNet and its
Markdown converter, which rsReticulum ports as `md2micron.rs`):

| Micron construct | Syntax observed | Model block/span |
| --- | --- | --- |
| heading level 1–6 | one to six `>` characters then text | `heading` |
| bold | `` `!text`! `` | span `bold: true` |
| italic | `` `*text`* `` | span `italic: true` |
| underline | `` `_text`_ `` | span `underline: true` |
| link | `` `[label`url] `` | `link` (resolve local `:/page/...` and `:/file/...` to this node; external to explicit external URL, never auto-fetch) |
| bullet list | `  • item` / `- ` lines | `bullets` |
| horizontal rule | `-` alone | `rule` |
| literal block | lines wrapped in `` `= `` markers | `literal` |
| colour/background codes | `` `F<rgb> ``, `` `B<rgb> ``, `` `f`b `` reset | dropped in M4 (client theme owns colours) |
| tables, images, forms | Micron features | dropped/plain-text in M4; see §6.4 |

Unknown constructs MUST NOT abort the page: render them as literal text and set a
`unsupported` count so the UI can warn. This is the same forward-compatibility rule OMP applies
to unknown CBOR keys.

### 6.3 Serving OpenMesh content as pages (M4.4, read-only)

The gateway can expose a small read-only page namespace so NomadNet users can read OpenMesh
room servers without an OpenMesh client:

| Path | Source | Notes |
| --- | --- | --- |
| `/page/openmesh/index.mu` | gateway status: node name, mesh stats, channels | Always available when the page server is enabled |
| `/page/openmesh/<channel>/index.mu` | channel history (room-server feature; M5) or last N messages from the local bridge | Read-only, rate-limited, `.allowed` access list optional |
| `/file/openmesh/<attachment_id>/<name>` | bridge attachment store | Only attachments explicitly shared to a channel/page |
| `/page/openmesh/<channel>/about.mu` | channel description | Admin-editable |

Pages are generated server-side in the Micron subset (headings, text, bullets, links, rule,
literal). **Do not** expose raw user text without sanitisation: escape backticks, newlines and
control characters so a message cannot inject markup or terminal escapes. This is a security
requirement, not a nicety.

### 6.4 Deferred features (M5+)

* Images in pages: NomadNet supports image rendering in some terminals; our clients can render
  images natively. Defer until the page model has an image block and an explicit size budget.
* Forms/POST-like interactions: they require request payloads and server-side state. M4 is
  read-only; adding forms means a capability/permission model.
* RRC live chat: a separate real-time protocol over links; out of scope.
* Page caching in OpenMesh clients: the bridge should cache server-side; client-side caching can
  come with conversation storage.

---

## 7. Security and trust model

### 7.1 The gateway is a trust boundary, and must say so

Because the gateway must decrypt OMP DMs (to re-encrypt them as LXMF) and decrypt LXMF messages
(to re-encrypt as OMP), it sees all bridged plaintext. Users must be told:

* Every bridged conversation shows "bridged via <gateway name>; not end-to-end encrypted across
  protocols" in the conversation header and in message metadata.
* The gateway operator can read bridged messages. Self-host the gateway if that is not
  acceptable.
* Attachments are stored in plaintext on the gateway when bridged (the bridge store may already
  hold them after decryption; document retention).
* Keys: the bridge seed, the gateway Reticulum identity, shadow identities and the registry are
  secrets. Store them in OS keystore where possible, encrypted at rest elsewhere. The gateway
  MUST NOT log private keys or message plaintext by default; log metadata at INFO, content only
  behind an explicit debug flag.

### 7.2 Threat model (M4)

| Threat | Mitigation |
| --- | --- |
| Malicious LXMF peer sends huge resources / page responses (DoS) | Hard size caps (256 KiB response, 8 KiB attachment, 64 KiB file), timeouts, memory-bounded streaming, link teardown on violation |
| Spam from the RNS side into OpenMesh | Allowlist/denylist of LXMF sources, per-peer rate limit (e.g. 20 messages/min), drop-with-count, no auto-reply storms |
| Malicious Micron content (terminal escapes, injection into generated pages) | Sanitise text, never execute, escape user content when generating pages, no auto link-follow |
| Impersonation of OpenMesh users on the RNS side | Only shadow identities created by this gateway may claim a node id in the custom field; the gateway verifies the OMP envelope signature before translating; third-party LXMF senders are bridge contacts, not users |
| Gateway key compromise | Per-node shadow identities limit the blast radius; support rotating/revoking shadow identities; allow users to disable bridging per contact; never reuse the gateway identity as a user identity |
| Gateway used as an anonymising relay for abuse | Operator policy + AGPL source availability; rate limits; optional allowlist; log bridged volume |
| Replay across protocols | OMP dedup is per OMP identity; LXMF dedup is by message hash; gateway keeps a `(source, lxmf_hash)` seen-set with TTL |
| Malformed protocol from either side | Both adapters are separate processes: a crash in the RNS daemon must not take down the gateway; a bad OMP frame is dropped by core |

### 7.3 Privacy of page browsing

Page fetches reveal the destination hash and request path to the remote node and to any transport
node on the path. The gateway SHOULD:

* only fetch on explicit user action (no background crawling);
* optionally route through a user-configured transport/proxy if one exists;
* report the destination hash and response size to the user;
* keep a local cache to avoid repeated fetches (also reduces load on remote nodes).

### 7.4 Licensing obligations

* The gateway is part of OpenMesh and is AGPL-3.0-or-later. Serving it over a network triggers
  AGPL §13: users interacting with it must be offered the source. Document where to get it.
* The RNS side runs as a separate program under its own license (Reticulum License or GPLv3 for
  NomadNet). Do not link; communicate over sockets. This keeps the licenses separate.
* Do not bundle Reticulum-licensed code, assets or documentation into OpenMesh releases. The
  bridge may *document* the interop and point users to the upstream installer.
* If a helper process uses Python `rns`/`lxmf`, ship it as an optional, separately-installed
  extra (`pipx install rns lxmf nomadnet`), not as an OpenMesh dependency. The OpenMesh binary
  must run without it (bridge-only mode).

---

## 8. Interfaces (API sketch)

### 8.1 Bridge/gateway REST endpoints (local, token-authenticated like the bridge)

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/api/bridge/status` | gateway identity destination, RNS connection state, peer count, queue depths |
| GET | `/api/bridge/contacts` | bridge contacts (LXMF peers seen or imported) with `bridged: true` |
| POST | `/api/bridge/contacts` | import an LXMF destination/name for bridging |
| GET | `/api/bridge/page?dest=&path=` | fetch/serve a page through the page proxy (JSON model) |
| GET | `/api/bridge/file?dest=&path=` | fetch a file (bytes; capped) |
| POST | `/api/bridge/send` | bridge-specific send when bridging a new peer: `{lxmf_dest, text, attachment_id}` |
| GET | `/api/bridge/events` | SSE stream of bridge events (or WS messages with `type: bridge_*`) |
| POST | `/api/bridge/pages/openmesh/...` | M4.4 page server management (enable/disable, access lists) |

Events pushed to clients: `bridge_status`, `bridge_message`, `bridge_ack`, `bridge_error`,
`bridge_page_fetched`, `bridge_peer_seen`.

### 8.2 OMP-side control frames

Do **not** add new OMP wire frames for the bridge in M4. The gateway is an application on top of
the bridge API; OMP nodes do not need to know a bridge exists. The only OMP-visible artefact is
the bridge contact proxy identity (a normal node with `class: gateway`, caps `WIFI|ETHERNET`).

If later a radio needs to be a gateway without a host bridge (e.g. a Heltec tracker with Wi-Fi),
the OMP `Admin` frame or a new `Bridge` frame type could carry page fetches. That is an OMP/2
topic and must not be added ad hoc.

### 8.3 Configuration (gateway TOML)

```toml
[gateway]
name = "ridge-gateway"
bridge_url = "http://127.0.0.1:8787"
bridge_token_file = "~/.config/openmesh/bridge-token"
data_dir = "~/.local/share/openmesh/gateway"
allow_inbound_bridge_contacts = true

[rns]
config_dir = "~/.rsReticulum"        # or ~/.reticulum for the Python stack
instance_mode = "shared"             # shared | tcp
tcp_host = "127.0.0.1"
tcp_port = 4242

[lxmf]
delivery_dest_name = "lxmf.delivery"
propagation_enabled = false
propagation_node = "0123456789abcdef0123456789abcdef"
stamp_cost = 0

[limits]
max_attachment_kib = 8
max_page_kib = 256
max_file_kib = 64
page_timeout_s = 20
link_idle_close_s = 600
peer_rate_limit_per_min = 20

[page_server]
enabled = false
bind_destination = "openmesh.pages"
allowed_file_extensions = [".mu"]
```

---

## 9. Phased plan

| Phase | Deliverable | Exit criteria |
| --- | --- | --- |
| **M4.0 — RNS spike** | throwaway script: connect to a local `rnsd`, fetch a NomadNet page path, send/receive an LXMF opportunistic message with the Python stack | page bytes and LXMF message round-trip observed; document exact API calls and license boundary |
| **M4.1 — identity registry** | shadow identities, bridge contacts, persistent table, key encryption, `openmesh://bridge` onboarding in clients | create bridge contact from a QR/paste; restart gateway and destinations are stable; unit tests for mapping collisions and revocation |
| **M4.2 — DM bridge** | OMP→LXMF and LXMF→OMP text messages, delivery state mapping, ACK synthesis, dedup, rate limits, events in bridge API + UI badges | two-node testnet: OpenMesh node ↔ LXMF client both directions; no false "delivered"; kill/restart gateway mid-conversation resumes without duplicates |
| **M4.3 — attachments** | capped re-encoded attachments both directions, progress events, explicit failures, propagation-size warnings | 4 KiB image each way; 12 KiB rejected with actionable message; propagation node with 1 KB limit rejects gracefully |
| **M4.4 — page proxy + OpenMesh pages** | page/file fetch with cache and sanitisation, structured page model, web + Android renderers, read-only OpenMesh page namespace | browse a real NomadNet page from OpenMesh clients; generated page renders in NomadNet; fuzz the Micron parser with 10k random inputs without panics |
| **M4.5 — hardening** | threat-model tests, DoS limits, observability (`bridge_frames_total`, `bridge_bytes_total`), docs, ADR update | soak test 24 h with synthetic peers; memory bounded; docs merged; license review recorded |

Calendar estimate for a solo maintainer with AI assistance: M4.0 1 week, M4.1 2 weeks, M4.2 3
weeks, M4.3 2–3 weeks, M4.4 3–4 weeks, M4.5 2 weeks. Total **3–4 months** after OMP/1 and M1 are
stable; see [ROADMAP.md](../ROADMAP.md).

---

## 10. Test plan against a local Reticulum testnet

### 10.1 Topology

```
rnsd A (TCP server 4242) ── TCP ── rnsd B (TCP client) ── NomadNet page node + lxmd propagation node
        │                                   │
   gateway (RNS adapter)                LXMF client (Python)
        │
   openmesh-bridge (sim or UDP) ── OMP nodes (openmesh-sim / two engine instances / handset)
```

Run with the Python reference stack (`pipx install rns lxmf nomadnet`) in the test harness; the
gateway under test uses whichever adapter was chosen in M4.0. This testnet is *not* part of the
OpenMesh build: tests are `#[ignore]` network tests plus a `scripts/bridge-testnet.sh` that
provisions a temporary config directory.

### 10.2 Test matrix

| ID | Scenario | Expected |
| --- | --- | --- |
| B-01 | Fetch `/page/index.mu` from a page node | 200, page model, `< 2 s` on TCP, no unsafe blocks |
| B-02 | Fetch missing page (`/page/nope.mu`) | structured 404 error, no crash, no retry storm |
| B-03 | Page > `max_page_kib` | truncated flag + error event; link torn down |
| B-04 | Malformed Micron (escape sequences, unterminated literal, 1 MiB line) | sanitised output, `unsupported` count, no panic |
| B-05 | OMP → LXMF text, peer online | LXMF client receives exact text + custom-provenance field; OMP shows delivered only after proof |
| B-06 | LXMF → OMP text, unknown source | bridge contact created, message arrives with bridge badge, provenance not trusted for identity |
| B-07 | Reply chains both ways | `reply_to` ↔ `0x30` mapping preserved for known hashes; thread hint dropped cleanly for unknown |
| B-08 | Gateway restarts mid-conversation | mapping stable, no duplicate messages, no duplicate LXM hashes, ACK state reconciled |
| B-09 | Propagation-only delivery (peer offline) | gateway deposits to propagation node; OpenMesh shows "stored at relay (not delivered)"; retrieval later delivers |
| B-10 | Attachment 4 KiB OMP → LXMF direct | bytes identical (SHA-256), progress events monotonic, completion |
| B-11 | Attachment 12 KiB OMP → LXMF | rejected with actionable error; no partial message visible to peer |
| B-12 | LXMF attachment 8 KiB → OMP | bridge attachment id created, OMP resource transfers, receiver stores |
| B-13 | LXMF peer floods 100 messages | rate limit kicks in, drops counted, no OMP ACK synthesized, no gateway OOM |
| B-14 | Malicious page with 1 MiB response / slow drip | timeout/size cap, link closed, other fetches unaffected |
| B-15 | Identity revocation | shadow identity removed → bridged messages to that mapping fail explicitly; old messages still readable |
| B-16 | License/provenance check | no Reticulum-licensed files in the release tarball; gateway runs with Python stack absent (bridge-only) |
| B-17 | OpenMesh page serving → NomadNet client | page renders in `nomadnet`; user text escaped so markup injection is impossible |
| B-18 | Simultaneous transfers | two attachments + page fetch: queues bounded, priorities respected |

### 10.3 Fixtures and CI

* Capture real LXMF packets and page responses into `protocol/vectors/bridge/` with provenance
  notes (destination hashes, versions, license of the fixture source). These are *integration*
  fixtures; the OMP golden vectors remain normative for OMP.
* Run `scripts/bridge-testnet.sh --ci` in a nightly job (network-dependent tests are never
  required for pull requests). The PR job runs pure unit tests for the page parser, mapping
  tables, state machine and rate limiter.
* Fuzz targets: Micron parser, LXMF field parser, attachment metadata. Run at least one
  long-running fuzz session per milestone; store corpora under `protocol/fuzz/`.

---

## 11. Open questions

1. **License-clean adapter.** Implement the RNS adapter against Python `rns` APIs (fast, but
   requires users to install a Reticulum-licensed dependency; the gateway process itself does
   not link it, so the combined-work question needs a lawyer's read) or implement the subset of
   the Reticulum wire we need independently (clean, but link/resource/announce state machines
   are weeks of work). M4.0 must produce a recommendation with the measured effort. Prefer the
   separate-process route: it is both faster and more defensible.
2. **Does LXM delivery proof reach the gateway for opportunistic sends?** The reference stack
   reports proofs to the sender; the gateway must retain them and map them to OMP ACKs. Verify
   in M4.0 that proofs survive daemon restarts and are exposed through the API we use.
3. **Propagation quota discovery.** LXMF announces include propagation metadata; can the gateway
   learn a node's accepted size before depositing, so the attachment cap adapts? If not, keep
   the conservative 1 KiB default and document why.
4. **One gateway per mesh or per user?** A per-mesh gateway centralises trust (one box reads all
   bridged plaintext). Per-user gateways are safer but mean N Reticulum identities and N
   daemons. Decide with M4.1 telemetry (how many users bridge).
5. **Page rendering parity.** How much of Micron should the OpenMesh page model support before
   it is worth maintaining? Start with headings/text/links/bullets/literal and add constructs
   only when pages in the wild need them; track `unsupported` counts.
6. **Room server ↔ LXMF propagation relationship.** Should OpenMesh room servers eventually be
   reachable as LXMF propagation nodes (so LXMF users can leave messages for OpenMesh users)?
   This is the most interesting M5 idea and the most dangerous (untrusted store); needs a
   separate design.
7. **Identity import UX.** If a user imports a real Reticulum identity, where do the private
   keys live and who signs? Importing into a gateway is a security regression compared with
   keeping the identity on the phone and bridging only by message relay. Probably defer.

---

## 12. Sources

* NomadNet README (features, GPLv3, LXMF/Reticulum foundation) —
  <https://raw.githubusercontent.com/markqvist/NomadNet/master/README.md>
* NomadNet page server (`/page`, `/file`, `/media` request handlers, `.allowed` access control,
  executable pages) — <https://raw.githubusercontent.com/markqvist/NomadNet/master/nomadnet/Node.py>
* Reticulum manual, "Understanding Reticulum" (links, requests, resources, destinations) —
  <https://reticulum.network/manual/understanding.html>
* Reticulum manual, "Reticulum License" — <https://reticulum.network/manual/license.html>
* LXMF message format, fields, delivery methods, propagation endpoints — rsLXMF checkout
  `5c18a5f`: `README.md`, `crates/lxmf-core/src/constants.rs`, `crates/lxmf-core/src/message.rs`
* Micron syntax subset — rsReticulum checkout `f6d0d69`:
  `crates/rns-git/src/md2micron.rs` (a documented port of NomadNet's Markdown→Micron converter)
* OpenMesh: `docs/architecture/PROTOCOL.md` §9–§10, `docs/architecture/WEBSOCKET-API.md`,
  `docs/architecture/COMPANION-API.md`, `crates/openmesh-bridge` transport/store modules
* Capacity numbers: [lora-capacity-and-media.md](lora-capacity-and-media.md)

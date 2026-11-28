# Recipient-accepted media transfers — consent design and rollout plan

**Status:** proposed (2026-11-28) · **Applies to:** `crates/openmesh-core`, `crates/openmesh-ffi`,
`crates/openmesh-bridge`, `apps/web`, `apps/android`, `content/help/media.md`,
`docs/architecture/PROTOCOL.md` (phase 2 only).
**Decision:** implement a **wire-compatible phase 1** (a compact offer/accept handshake carried as
ordinary text messages) for **direct chats** first; design the **phase 2** structured message kind
now but ship it only with a protocol bump, capability negotiation and regenerated vectors.
**No wire changes in phase 1, no radio experiments required for the prototype.**

> User requirement: *the recipient must accept a media transfer before the radio spends the
> bandwidth.* One 5.9 KiB image is ~22 s of airtime and — at `MEDIUM_FAST`/EU868 10 % duty with
> ACKs and retries — **about 7.5 minutes of wall time**; today the payload starts automatically.
> Related: [media tiers](media-tiers.md), [capacity analysis](../research/lora-capacity-and-media.md),
> [ADR 0007](../adr/0007-media-pipeline.md), [PROTOCOL.md](../architecture/PROTOCOL.md) §9,
> [WEBSOCKET-API.md](../architecture/WEBSOCKET-API.md), [media help text](../../content/help/media.md).

---

## 1. Problem and current behaviour

Today the sender's client compresses/encodes the media, the core engine emits a resource manifest
and chunks immediately (`Engine::send_resource`; web `POST /api/attachments` →
`send_message`/`send_group_message` with `attachment_id`; Android `sendAttachment`), and relays
repeat every chunk. The recipient sees "Receiving…" and cannot refuse; the sender cannot know
whether the recipient wanted the bytes. With duty-cycle pacing a single 6018-byte image
(6047-byte sealed blob, 34 × 180-byte chunks) is ≈20.8 s of chunk airtime + manifest/ACK overhead,
i.e. ≈3.7 min of *wall-clock* floor at 10 % duty, and the user measured ≈7.5 min with retries and
ACK round-trips. In a 1 %-duty region that is most of the hourly transmit budget
(see [capacity analysis](../research/lora-capacity-and-media.md) §4).

Other constraints that shape the design:

* **Every frame is repeated by relays.** A 34-chunk resource is not paid once per link; offer and
  decision messages are small but still flood like any `Data` frame (PROTOCOL §11).
* **Resources auto-start.** Consent must therefore be enforced **before** the client calls the
  resource path, not by an engine flag that later drops chunks (dropping mid-transfer wastes the
  airtime already spent).
* **Channels are broadcast.** Any member in range receives a channel resource; a per-recipient
  transfer cannot be addressed individually in OMP/1.
* **Store-and-forward exists for envelopes, not for media consent.** Offers are ordinary
  application messages and may arrive late (or be replayed); the payload itself is a resource and
  is not a durable store-and-forward object.
* **Wire compatibility is a hard requirement for phase 1.** New `MessageKind` variants or CBOR
  fields require PROTOCOL.md, regenerated `protocol/vectors/`, a `PROTO_VERSION` note and firmware
  conformance (AGENTS.md §2). We cannot block the user-facing fix on that.

---

## 2. Goals and non-goals

### Goals (phase 1, prototype)

1. A recipient sees an **offer** (size, type, estimated radio time) and explicitly **accepts** or
   **declines** before any chunk is transmitted.
2. The offer itself costs **one small message**, not a resource payload; declining spends no
   resource chunks.
3. Works in **direct chats** on web and Android; channel behaviour is documented and not silently
   changed.
4. No wire-format change, no new `MessageKind`, no golden-vector churn; old clients still render
   the offer as readable text.
5. Pending offers, accepts, declines and timeouts have clear, tested UI states; restart does not
   leave a dangling transfer.
6. Abuse is bounded: offer rate limits, size caps, expiry, no auto-accept, authenticated
   accept/decline.

### Non-goals for phase 1

* Thumbnail/preview pixels inside the offer (a preview would itself be a resource transfer; phase 2
  may allow an optional tiny preview).
* "Accept a smaller version" re-compression server/client flow (phase 2; phase 1 users can decline
  and chat/choose a tier before re-offering).
* **Channel media consent.** Channel sends keep the current explicit sender-side cost
  confirmation; per-recipient channel consent is designed below but not shipped in the prototype
  (see §6.3 for why).
* MCP/CLI agent media consent (operator tools keep `attachment_id` semantics; document that they
  are trusted tools).
* Store-and-forward guarantees for the payload; only the small decision messages inherit normal
  message delivery.

---

## 3. Threat and UX analysis

| Scenario | Risk | Mitigation |
| --- | --- | --- |
| Unsolicited media spam | Recipient pays receive airtime/battery; relays repeat it | Offer is one small message; payload never starts without an accept; per-peer offer rate limits and mute/block |
| Accept griefing | An attacker forces a sender to spend minutes of airtime by accepting expensive offers | Accept is authenticated per sender/target; sender shows the estimate and can cancel; offers expire; sender-side per-recipient cooldown after a declined/expired offer |
| Offer metadata leak | Size/MIME/tier/timing are visible to relays and recipients | Offer carries no filename/thumbnail/captions by default; keep metadata minimal; document that size/type are visible on air |
| Channel fan-out amplification | One offer + one upload is heard by every member and repeated by relays | One offer per sender/channel per window; upload once after first accept; UI states "everyone in range receives the file once one person accepts"; phase 2 adds quorum |
| Duplicate/replayed accepts | Sender uploads twice, wasting airtime | Decision marker carries the offer message id and SHA prefix; sender deduplicates by `(sender, offer_id)` and accepts only the first valid decision; expiry enforced |
| Forged accept | Wrong node triggers a transfer | Direct chats: only the addressed peer's decision counts; channels: signed group messages attribute the member; marker must reference a live local offer |
| Recipient offline / sender offline | Accept arrives late or after expiry; sender not ready | Offer/decision carry an absolute expiry; expired accepts are ignored; clients queue the payload send until the radio is ready (existing honest-delivery queue) and never auto-retry expired offers |
| Sender waits forever | Pending transfer clutters chat | Explicit pending state + countdown + Cancel; timeout marks the offer expired on both sides without additional frames |
| Accidental accept | Cost surprise | Accept button shows the estimate and a confirmation is optional; default focus is Decline/Dismiss; accessibility labels |
| "Channel consent" illusion | A member believes their decline stops the transfer | Documented: channel media is a broadcast; consent is advisory in phase 1; no channel offers in the prototype |

UX principle (beginner copy, [ux-help-system.md](ux-help-system.md)): describe cost in **radio
time** ("about 7 minutes of radio time") and name the action ("Ask before sending"), not only in
bytes or KiB.

---

## 4. Phase 1 — wire-compatible offer/accept handshake

### 4.1 Transport

All handshake messages are **ordinary `MessageKind::Text` messages** whose body is a compact
URI marker. Rationale:

* Existing encoders/decoders already carry text in `AppMessage`; no CBOR schema, `MessageKind`
  enum or golden-vector change.
* Clients that do not understand the marker display it as text (plain-text fallback), satisfying
  the requirement; new clients detect the marker and render an offer/decision card.
* The message id is the offer id; `reply_to`/marker `offer` fields link decisions to the offer.
* Text messages are still delivered/ACKed with normal semantics, so a lost offer is retried by
  the ordinary message retry path.

**Cost:** a direct offer with a known peer identity is one `Data` frame (~120–200 bytes depending
on marker length; the marker below is ≤100 ASCII characters). A channel offer with >~10 characters
goes through the existing signed-group long-text resource path (a signed `Data` frame plus a
manifest and one chunk ≈ 3 small frames, ≈1–2 s airtime at `MEDIUM_FAST`); this is the same path a
long channel text uses today and is still negligible versus the media transfer. Markers must stay
ASCII, percent-encoded, and ≤100 characters; longer text would become a long-text resource and
should be rejected as an invalid offer.

### 4.2 Marker grammar (version 1)

Media type codes keep the marker inside one frame. Only the bandwidth-heavy user-facing types are
allowed in phase 1; generic files use the existing immediate-send path with a sender-side cost
confirmation until phase 2.

| `mt` | MIME | Notes |
| --- | --- | --- |
| `1` | `image/webp` | default image tier output |
| `2` | `image/jpeg` | clients that cannot produce WebP |
| `3` | `image/png` | screenshots; cost warning |
| `4` | `audio/c2-1300` | Codec2 tiny voice |
| `5` | `audio/c2-3200` | Codec2 medium voice |
| `6` | `audio/ogg; codecs=opus` | Opus voice (clients may substitute AAC/MP4 and declare it in phase 2) |

Offer (sender → recipient(s)):

```
openmesh://media-offer/1?mt=1&b=6047&n=34&h=195ce33c&e=1795821000
```

| Field | Meaning |
| --- | --- |
| `mt` | media type code from the table above |
| `b` | plaintext payload size in bytes (the media bytes, before resource sealing) |
| `n` | number of resource chunks the sender will use (`ceil((b+29)/180)`) |
| `h` | first 8 hex characters of SHA-256 over the **sealed** resource blob; binds the eventual payload to this offer |
| `e` | offer expiry, Unix seconds |
| optional `d` | voice duration in ms (`d=8000`) |
| optional `t` | sender tier label (`t=tiny`/`medium`/`high`/`original`) for display only |

Decision (recipient → sender), sent with `reply_to = offer message id`:

```
openmesh://media-decision/1?o=1720520168&a=1      # accept
openmesh://media-decision/1?o=1720520168&a=0      # decline
```

Optional sender cancel (broadcast to the same audience):

```
openmesh://media-decision/1?o=1720520168&c=1
```

Rules:

* Offer id = the offer message's `msg_id`; `o` is decimal and must match a live local offer.
* `a=1` accepts, `a=0` declines; silence past `e` means declined (sender shows "no answer").
* The decision must be authenticated by the channel/direct encryption; direct decisions from any
  node other than the addressed peer are ignored, channel decisions from any verified member are
  valid (first valid accept wins in phase 1; see §6).
* A decision marker referencing an unknown, expired, already-accepted or already-declined offer is
  ignored **without an error message** (no extra airtime).
* If a client cannot parse the marker it MUST show the raw text (fallback) and MUST NOT start a
  resource automatically.

### 4.3 State machines

Sender (client-side; the engine is only asked to send the payload after accept):

```
draft --(sender confirms estimate)--> offered --(accept)--> transferring --> sent/failed
                                        |--(decline)--------> declined
                                        |--(expiry)----------> expired (offer stays in history)
                                        |--(sender cancel)---> cancelled
```

Receiver:

```
offer received --> validating --> pending --(accept)--> accepted (wait for resource; progress UI)
                                   |--(decline)--------> declined (decision message)
                                   |--(expiry)----------> expired
accepted --(payload never arrives before resource timeout)--> "not received — ask sender"
```

Implementation notes:

* The sender persists an offer row (`offer_id`, target, marker fields, estimate, state, expiry)
  locally; only then sends the offer text. On accept it calls the existing attachment path **once**
  (guarded by state transition + a local lock). The attachment upload on the web is deferred until
  accept; Android defers `sendAttachment` until accept.
* The receiver validates before showing the card: known sender/contact or channel member, known
  `mt`, `b > 0` and within `MAX_ATTACHMENT_BYTES`, `n == ceil((b+29)/180)`, `h` is 8 hex chars,
  `e > now`; otherwise treat as plain text.
* On restart, clients restore durable offer rows; offers past `e` are marked expired and never
  trigger a payload. Full engine-state export cadence is unchanged (media transfer state is app
  state, not engine state).

### 4.4 Estimates shown to both sides

The sender computes the estimate **before** sending the offer and includes it implicitly through
`b`/`n`; both sides must show radio time, not just bytes:

```
airtime_ms  ≈ n × lora_airtime_ms(profile, 240) + manifest/ack overhead   (util::lora_airtime_ms)
wall_ms     ≈ airtime_ms / duty_cycle_pct × 100                            (plus retry margin)
```

Rules:

* Sender shows "≈X s of airtime, ≈Y min of radio time at your current profile; ≈Z % of the hourly
  budget" using the existing media pre-send estimate UI, and asks for confirmation to send the
  **offer**.
* Recipient's card shows the sender's estimate range and states that accepting will receive one
  copy; it never claims a precise completion time (retries and ACKs vary).
* In 1 % regions, the UI warns that one 5.9 KiB image can consume most of the hourly airtime before
  the sender offers it; the accept card repeats the warning.

---

## 5. Phase 2 — structured protocol message kind (next protocol bump)

Phase 1 is deliberately a bridge; phase 2 replaces the markers with first-class fields so clients
do not parse strings, offers can carry previews/variants and channels can negotiate quorum.

### 5.1 Wire changes (requires PROTOCOL bump)

* New `MessageKind` variants: `media_offer` and `media_decision` (CBOR enum values added at the
  end of the existing list; old decoders treat an unknown variant as undecodable, so **never send
  these to peers that did not advertise support**).
* New optional `AppMessage` fields:

```
media_offer: Option<MediaOffer> {
  offer_id: u32, mime: String, tier: Option<String>,
  plaintext_bytes: u32, chunk_bytes: u16, chunk_count: u16,
  sealed_sha256: [u8; 32], duration_ms: Option<u32>,
  expires_at_ms: u64, estimated_airtime_ms: u32, estimated_wall_ms: u32,
}
media_required: Option<MediaDecision> {
  offer_id: u32, decision: accept|decline|cancel, variant: Option<String>,
}
```

* Announce capability bit `MEDIA_CONSENT` (proposal: the next free caps bit after `SOLAR`,
  `1 << 10`, PROTOCOL §6.1). Clients advertise it; senders only use the structured kind for peers
  whose latest announce carries it. Channels keep using phase-1 markers until the capability is
  common (the sender cannot know every listener's version from a single announce; phase 2 may add
  a channel-level “all members support…” check or keep markers as the channel format).
* PROTOCOL.md updates: §9.5 media consent, §6.1 caps bit, §7 application message schema, §12 timing
  defaults; regenerate `protocol/vectors/` for the new app-message/kind shapes; note in
  `PROTO_VERSION`/migration docs; firmware conformance replay (firmware relays opaque `Data`
  frames, but the vector policy still applies).
* Bridge: message JSON gains `media_offer`/`media_decision` objects and message state
  (`pending|accepted|declined|expired`), WS events `media_offer`/`media_decision`, OpenAPI and
  `WEBSOCKET-API.md` updates, MCP tools accepting an explicit `media_consent` mode.
* FFI: if clients need the engine to emit the structured kinds, add `send_media_offer` /
  `send_media_decision` wrappers (or keep composing `AppMessage` in Kotlin); document in
  `COMPANION-API.md`.

### 5.2 Why not phase 2 now

* New enum variants break old app decoders on channels (they cannot render an offer; the message
  would be dropped as undecodable rather than shown as text).
* Vectors, firmware conformance and `PROTO_VERSION` gates would delay the user-facing fix.
* The user is actively field-testing; phase 1 requires no radio/protocol change and no flash.

---

## 6. Decisions and semantics

### 6.1 Direct chats (prototype scope)

One sender, one recipient, explicit accept. The sender shows the cost, sends one offer frame, and
waits. This is the recommended prototype: it covers the user's stated case (send a picture to one
person without spending their bandwidth unasked) and needs only client code.

### 6.2 Accept/decline policy

* Accept is a strong, authenticated signal; decline is preferred explicit (one small frame) so the
  sender frees the pending state quickly. Silence = declined at expiry.
* The sender may cancel before an accept; after accept the transfer proceeds (the sender may still
  abort if the radio fails, shown as failed).
* No automatic retry of an expired offer; a retry is a new offer with a new message id and a new
  expiry (sender UI: "Offer again").
* Replays: accept with the same offer id is processed once; a second accept is ignored.

### 6.3 Channels (documented for phase 1, implementation phase 2)

Channel media is broadcast: any accept triggers **one** upload that every member in range receives.
Therefore:

* Phase 1 (prototype) ships **DM offers only**. Channel media keeps the current sender-side cost
  confirmation and should be used sparingly; the composer warning stays.
* Phase 1.5 (after DM validation, before phase 2): channel offer allowed behind an explicit
  "ask the channel" action; sender waits for the **first** accept, then uploads once; the offer
  card and composer both state "Everyone in range will receive this file once someone accepts;
  declines do not exclude anyone." One offer per sender/channel per window; no duplicate uploads
  (sender state machine guards).
* Phase 2: structured decisions can carry a **quorum policy** (`first accept`, `N accepts`,
  `any`); recipients who decline are not addressed (the resource is still broadcast in OMP/1, so
  this is a UI/policy improvement, not a wire exclusion). Channel history/resource storage remains
  best-effort per PROTOCOL §9/§11.

### 6.4 Offline recipients and store-forward

* Offers/decisions are ordinary messages; they may be delivered late via normal retries and
  store-and-forward. Expiry (`e`) is authoritative and checked on receipt.
* The payload is only generated and sent after an accept while the sender's radio is ready. If the
  sender is offline when the accept arrives, the client queues the payload send until the engine is
  ready (existing honest-delivery queue) and never sends it to an expired offer.
* Relays may store `Data`/`Resource` envelopes for absent peers (PROTOCOL §11); this design does
  **not** rely on store-and-forward for the payload and does not add a new stored type.

### 6.5 Timeouts, limits and abuse controls (draft defaults)

| Control | Value (proposal) |
| --- | --- |
| Offer expiry (`e`) | 10 minutes after send |
| Sender pending timeout UI | countdown from `e`; no frame sent at expiry |
| Sender retry | one manual "Offer again" (new offer id); no automatic re-offer |
| Max pending outgoing offers per peer | 2 |
| Max pending outgoing offers per channel | 1 (phase 1.5) |
| Offer rate limit | 1 per peer per 5 min; ≤6/hour per peer; channel: 1 per 10 min (phase 1.5) |
| Declined cooldown | 10 min before offering to the same peer again (client-side) |
| Max offer marker length | 100 ASCII characters (DM single frame); reject longer |
| Max offered payload | active profile `max_attachment_bytes` (PROTOCOL §9.4); reject others |
| Accept authenticity | direct: addressee only; channel: verified member; one accept per offer |
| Spam/mute | existing contact block/mute applies; offers never auto-accept, never auto-download to storage |

These are client-side defaults; phase 2 can promote them to PROTOCOL §12 timing values if they
prove stable.

### 6.6 Privacy

The offer reveals to relays and recipients that media of a given size/type is about to be sent.
That is inherent to consent and acceptable; do not add filenames, captions, EXIF or thumbnails to
the offer (phase 2 optional preview must be separately consented). The SHA prefix is a binding
commitment, not a privacy leak (it is already visible in the later manifest).

---

## 7. Core, API and client impact

### 7.1 Phase 1 (prototype)

| Layer | Change |
| --- | --- |
| Core engine | **None.** Text messages already carry markers; resource transfer stays exactly as today (`send_resource`) and is only called after accept. A tiny pure helper/estimate is optional but not required (clients already compute estimates for the pre-send UI). |
| FFI | **None** for the handshake (text/attachment APIs already exist). No binding regeneration. |
| Bridge API / OpenAPI / WS | **None required.** Web/Android use existing text messages (`POST /api/messages`, `/api/channels/:id/messages`, WS `send_message`/`send_group_message`) and existing attachment upload (`POST /api/attachments`) only after accept. MCP/CLI stay immediate-send (documented). |
| Bridge message store | No schema break: markers stay text; optional UI-only parsing client-side. |
| Web | Offer/decision marker parser + pending-offer store (zustand + localStorage persistence); composer becomes "Prepare → Offer" for DM media; offer card with Accept/Decline; accept sends decision text then uploads+sends attachment; pending state visible in the thread and survives reload. |
| Android | Same state machine in `MeshRepository`; `Room` message/media fields (`offerId`, `offerState`, `offerExpiresAtMs`, `estimatedWallMs`); notification Accept/Decline actions; defer `sendAttachment`/upload until accept; persist pending offers across process restarts. |
| Help/docs | Update [`content/help/media.md`](../../content/help/media.md) with "Media asks first" and channel caveat; web/Android strings and accessibility labels; screenshots. |

### 7.2 Data model (client-side, phase 1)

```
PendingOffer {
  offerId: u32          // message id of the offer text
  direction: out|in
  peerId | channelId
  mediaType: u8 | mime
  plaintextBytes: u32
  chunkCount: u16
  sealedShaPrefix: [u8;4]
  expiresAtMs: u64
  estimatedAirtimeMs: u32
  estimatedWallMs: u32
  state: pending | accepted | declined | expired | cancelled | failed
  createdAtMs: u64
}
```

The chat message carrying the marker stores `offerId` so the card can be re-rendered from history
(new clients parse the marker; old clients show text). Keep the marker in the sent/received text
in storage (it is the fallback and the idempotency key).

### 7.3 Phase 2 impact

Core: enum/fields + caps bit + docs/vectors. Bridge: JSON/WS/OpenAPI + optional stored columns.
FFI: only if structured kinds are exposed (otherwise clients construct `AppMessage` through a new
send API). Android/web: render structured cards directly; keep marker parser for compatibility with
phase-1 peers. Migration: peers without the caps bit get phase-1 markers from new clients;
markers remain readable forever.

---

## 8. Prototype scope and acceptance tests

**Recommended first slice:** direct-chat offers on **web** (fastest to iterate) then Android, both
against the same marker spec; no channel offers; no previews; no protocol bump.

Acceptance criteria (implemented as tests where the layer allows, manual where it is UI-only):

1. **Offer costs one frame, no payload:** posting the offer text emits exactly one `Data` frame
   (`send_text` returns 1 outbound frame for a ≤100-char marker to a known peer) and **zero**
   `Resource` frames/manifests.
2. **Decline:** a decline marker produces no attachment upload and no resource frames; sender row
   becomes `declined`.
3. **Timeout:** after `e`, the offer is `expired`; an accept arriving later is ignored and starts no
   transfer; no frames are emitted by the timeout itself.
4. **Accept:** exactly one transfer starts; duplicate accepts do not start a second; the existing
   resource path completes (simulator/integration test) and the receiver stores the payload.
5. **Validation/replay:** wrong-target direct accept, accept for an unknown/expired offer, malformed
   marker, oversized `b` or inconsistent `n` are ignored (treated as plain text).
6. **Restart:** pending offers persist and re-render; expired offers are cleaned up; no dangling
   `transferring` state and no automatic resend after restart.
7. **Channel (phase 1.5 test):** one offer frame to the channel, first accept causes exactly one
   upload; second accept starts no second upload; composer/card carry the "everyone receives" copy.
8. **Estimate:** displayed airtime/wall-time uses the same formula as `util::lora_airtime_ms` /
   `max_attachment_bytes`; offer `n`/`b` equal the payload the attachment path will actually send.
9. **Fallback:** a client without the feature shows the marker as text and downloads nothing.
10. **No regression:** existing immediate-send flows (non-offer attachments, MCP/CLI, channel
    media with warning) behave exactly as before when the feature is disabled/unused; phase 1
    leaves protocol vectors byte-identical.

---

## 9. Implementation plan and task breakdown

Estimates are engineer-days for the prototype; phase 2 is gated on a protocol bump.

| ID | Area | Deliverable | Depends on | Est. |
| --- | --- | --- | --- | --- |
| M1 | design | Sign-off of this doc + marker fixtures (offer/accept/decline/expired examples) | Lead/human review | 0.25 |
| M2 | spec/tests | Shared marker validation fixtures (JSON examples consumed by web + Android unit tests); estimate examples from `lora_airtime_ms` | M1 | 0.5 |
| W1 | web | Marker parser + pending-offer store (persisted), DM composer "Offer" flow, offer/decision cards, accept triggers upload+send, decline/timeout/cancel, tests | M2 | 2 |
| A1 | Android | Same state machine in `MeshRepository`, Room fields + migration, notification actions, offer cards, defer `sendAttachment`, persistence across restart, tests | M2 | 2.5 |
| Q1 | QA | Two-client DM acceptance pass (offer/decline/accept/timeout/restart), fallback with old client, airtime numbers sanity; screenshots | W1+A1 | 1 |
| H1 | help/docs | `content/help/media.md` section, composer/card copy (beginner language), channel warning; update media tier doc references | M1 | 0.5 |
| M3 | phase 1.5 channel | Channel offer marker + first-accept single-upload state machine + tests (behind a setting) | W1+A1 | 1.5 |
| P1 | phase 2 protocol | PROTOCOL §9.5/§6.1 caps, `MediaOffer`/`MediaDecision` + `MessageKind` variants, `MEDIA_CONSENT` caps bit, vectors + firmware conformance + migration note | M3 validated | 2–3 |
| B1 | phase 2 bridge | JSON/WS events, message state fields, OpenAPI/WEBSOCKET-API, MCP `media_consent` mode | P1 | 1 |
| C1 | phase 2 clients | Structured offer UI replaces marker parsing for capable peers, keep fallback, capability negotiation | P1+B1+W1+A1 | 2 |
| F1 | phase 2 FFI | Structured offer/decision send wrappers if required; COMPANION-API + bindings | P1 | 0.5–1 |

**Blocking flag:** P1 (and anything depending on it) needs Lead approval for a protocol version
bump, regenerated golden vectors and a firmware conformance run. Phase 1 has **no protocol impact**
and can proceed once the Lead approves this design.

---

## 10. Open questions

1. **Marker channel:** keep `MessageKind::Text` (plain-text fallback, phase 1) — accepted tradeoff;
   revisit using `Control` in phase 2 if the structured kind lands.
2. **Media type codes:** freeze the phase-1 code table in a shared fixture; generic files must wait
   for phase 2 or keep immediate send.
3. **Channel consent wording:** "everyone in range receives it once one person accepts" — needs
   human copy review/translation.
4. **Estimate display:** show both airtime and empirical wall-clock range ("≈4–8 min depending on
   retries") rather than a single number; confirm with user testing.
5. **Quorum policy (phase 2):** first-accept vs N-accepts vs sender chooses; likely a per-channel
   preference stored client-side.
6. **Previews (phase 2):** optional tiny preview resource separately consented; explicitly out of
   phase 1.
7. **MCP/CLI:** keep immediate `attachment_id` sends for automation (trusted tools) or gate behind
   an explicit operator flag; decide before phase 2.

---

## 11. References

* [PROTOCOL.md](../architecture/PROTOCOL.md) §6.1 (announce caps), §7 (application messages),
  §9 (resources and size policy), §11 (flooding/dedup), §12 (timing).
* [ADR 0007 — media pipeline](../adr/0007-media-pipeline.md).
* [Media tiers and tiny voice](media-tiers.md).
* [LoRa capacity and media — quantitative analysis](../research/lora-capacity-and-media.md).
* [WEBSOCKET-API.md](../architecture/WEBSOCKET-API.md) §6 (attachments) and §7 (send commands).
* [Media help text](../../content/help/media.md), [UX/help system](ux-help-system.md).
* [ROADMAP.md](../ROADMAP.md) — current milestone and media principles.

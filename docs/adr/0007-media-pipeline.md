# ADR 0007 — Media pipeline: Opus/Codec2, image compression, airtime caps

* **Status:** accepted (v0.1.0); caps are being refined per region in M1
* **Date:** 2026-11-25
* **Decision record:** summary row 7 in
  [ARCHITECTURE.md §4](../architecture/ARCHITECTURE.md#4-key-decisions)
* **Related:** [PROTOCOL.md §9](../architecture/PROTOCOL.md),
  [capacity analysis](../research/lora-capacity-and-media.md),
  [ADR 0001](0001-own-protocol-vs-reticulum.md), [ADR 0006](0006-web-first-clients-tauri-native-android.md)

## 1. Context

Images and voice notes are OpenMesh's primary differentiator. They are also the only features
whose cost is measured in *minutes of channel time*, so the design must be explicit about:

1. **Where encoding happens.** Browser `createImageBitmap` + canvas can downscale/re-encode
   images; `MediaRecorder` can record Opus in an Ogg/WebM container. The bridge should not be an
   image/audio service — it stores and transports bytes.
2. **Which codecs.** Opus is universally decodable (browsers, Android, desktop) and efficient at
   6–10 kbps for voice; Codec2 is dramatically cheaper (700–3200 bps) but needs a decoder on the
   client (not native on Android/iOS). LXST/LXMF both use Opus *and* Codec2 modes, which is
   evidence both are relevant.
3. **How big is too big.** The airtime analysis shows the answer is profile- and
   region-dependent by two orders of magnitude:

| Profile | 240 B frame | 5 KiB image | 8 s Opus voice | 8 s Codec2 700C |
| --- | --- | --- | --- | --- |
| `SHORT_TURBO` | 97 ms | 2.9 s | 3.8 s | 0.5 s |
| `SHORT_FAST` | 194 ms | 5.8 s | 7.7 s | 1.1 s |
| `MEDIUM_FAST` | 611 ms | 18.3 s | 24.2 s | 3.4 s |
| `LONG_FAST` | 2 034 ms | 61.0 s | 80.6 s | 11.4 s |
| `LONG_SLOW` | 13 509 ms | 406 s | 537 s | 75.7 s |

A single byte cap cannot be right for all of these. Worse, in a 1 % duty-cycle region the
sender has only 36 s of airtime per hour: a 12 KiB image at `MEDIUM_FAST` is 42 s of airtime —
already over the budget — while at `LONG_SLOW` even a 1 KiB thumbnail is 100 s.

4. **What the user sees.** A progress bar in bytes hides the real constraint. The UI must show
   airtime seconds, duty-cycle impact, and alternatives ("send a 2 KiB thumbnail"), or users
   will conclude the app is broken.

## 2. Decision

**Media is encoded client-side, transported as OMP resources, capped by a per-profile byte
policy today and an airtime-budget policy in M1, with Opus as the default voice codec on fast
profiles and Codec2 700C on constrained profiles.**

### 2.1 Images

* **Client-side only.** Web: `createImageBitmap` + canvas → JPEG/WebP; Android: `Bitmap`
  downscale + `ImageDecoder`/`BitmapFactory` → JPEG/WebP. The bridge stores the resulting bytes
  as an attachment and streams it as a resource; it does not transcode.
* **Defaults:** longest edge 1024 px (768 px on `LONG_*`, 512 px on `LONG_SLOW`); JPEG quality 60
  (45 on slow profiles); WebP for screenshots/graphics; PNG only with a warning.
* **Target sizes:** thumbnail ≤ 1.5 KiB, standard ≤ 6 KiB, HQ ≤ 12 KiB (desktop/web may choose
  from the estimate).
* **Metadata:** strip EXIF/GPS by default; location is a separate position message ("attach
  location" is explicit).
* **Transport:** resource manifest + 180-byte chunks + selective-repeat ARQ (PROTOCOL §9);
  `kind = attachment`, MIME from the encoder, SHA-256 over the sealed blob.
* **Caps today (PROTOCOL §9.4):** 24 KiB `SHORT_*`, 12 KiB `MEDIUM_FAST`, 6 KiB `LONG_FAST`,
  1.5 KiB `LONG_SLOW`. The sender MUST estimate airtime and require confirmation in 1 % regions.

### 2.2 Voice notes

* **Opus default:** mono, 16 kHz, 20 ms frames, 6–10 kbps, Ogg container
  (`audio/ogg; codecs=opus`) — exactly what browsers and Android can record/play without extra
  codecs.
* **Codec2 option:** 700C mono (`audio/c2-700c`, ~700 bps) for constrained presets, 4–8 s caps.
  This is an order of magnitude cheaper (see table above) but requires a decoder on the client;
  shipping Codec2 voice notes therefore requires either a bundled decoder (FFI/native lib) or
  restricting the feature to desktop/embedded targets until M2.
* **No real-time calls in v1.** Calls need continuous throughput (8 kbps Opus at minimum) and
  forward secrecy; on 1 %-duty LoRa they are physically impossible, and on fast presets they are
  range-limited. Calls are an OMP/2 feature gated on ratchets and on non-duty-limited
  transports (see [reticulum-lxmf-notes.md](../research/reticulum-lxmf-notes.md) §4.2).
* **Voice messages are messages:** they are resources with a duration metadata field, delivered
  through the same store-and-forward/ACK path as images, with the same pre-send estimate.

### 2.3 Airtime caps (M1 refinement, proposed)

Replace fixed byte caps with a function of profile **and region duty cycle**:

```
budget_ms   = min(30_000, duty_cycle_pct/100 * 3_600_000 / 4)     # max 30 s, ≤25 % of hourly budget
cap_bytes   = floor(budget_ms / airtime_per_chunk_ms(profile)) * 180 * 0.8
```

Current values from the capacity analysis:

| Profile / region | Suggested cap | Current spec cap | Action |
| --- | --- | --- | --- |
| `SHORT_TURBO` / US915 | 24 KiB (cap binds) | 24 KiB | keep |
| `SHORT_FAST` / US915 | 24 KiB | 24 KiB | keep |
| `MEDIUM_FAST` / US915 | ~8 KiB | 12 KiB | tighten at M1 |
| `MEDIUM_FAST` / EU868 1 % | ~2.5 KiB | 12 KiB | tighten significantly |
| `LONG_FAST` / EU868 1 % | ~0.8 KiB | 6 KiB | tighten significantly |
| `LONG_SLOW` / EU868 1 % | text only | 1.5 KiB | drop media or warn hard |

The cap is a *sender policy*, not a wire rule; receivers must accept anything within the wire
limits. The firmware's duty-cycle token bucket and the bridge's estimator enforce the budget at
send time, and the clients show the estimate before the user commits.

### 2.4 UX contract

Before sending any media, the client MUST show:

* estimated airtime in seconds (one transmission, plus "per relay in range"),
* estimated wall-clock time at the current region's duty cycle,
* the fraction of the sender's hourly airtime allowance,
* a smaller alternative when the estimate exceeds the budget,
* for store-and-forward contexts, the exact promise: "a repeater may hold this for up to 72 h;
  that is not proof of delivery" (and "media store-and-forward is disabled/limited on this
  repeater" when applicable).

During transfer show chunks and remaining airtime, and "retrying (n/6)" on ARQ stalls. After
transfer, show delivered/stored/failed with the same honesty as text.

## 3. Consequences

### 3.1 Positive

* **One codec path per platform.** Opus is natively supported by web/Android/desktop; no native
  audio plugins for the default path.
* **No bridge-side media processing.** The bridge is a byte store; CPU and attack surface stay
  small; clients control quality/size with immediate preview.
* **Costs are visible.** Airtime estimates turn the fundamental LoRa constraint into a product
  feature instead of a support burden.
* **Caps are explicit and tunable.** Changing a region or profile changes what is possible, and
  the policy is in one place (spec + bridge config + clients).
* **Codec2 escape hatch.** For the slowest presets, voice is still possible at 700 bps — 8–10×
  cheaper than Opus — at the cost of a bundled decoder.
* **Store-and-forward stays honest.** Media envelopes are large; the design prefers text
  store-and-forward and documents media storage as a per-board budget (see capacity §6.4).

### 3.2 Negative

* **Images on slow profiles are effectively unusable.** That is physics, not a bug; the UI must
  say so and offer text/thumbnails.
* **Codec2 playback on phones.** Android/iOS do not ship Codec2 decoders; the app must bundle one
  (native lib or Rust via FFI) or the feature is desktop-only on constrained profiles. This is
  an M2 product decision with real app-size/complexity cost.
* **Client-side compression quality varies.** Different browsers/codecs produce different bytes
  for the same image; the estimate is based on actual output bytes (POST returns size), so the
  UI is accurate, but cross-platform size parity is not guaranteed.
* **Large media in store-and-forward is a flash problem.** 64 text envelopes ≈ 12–20 KiB;
  64 image envelopes ≈ 0.5–2 MiB, which does not fit nRF52840 boards. Media store-and-forward is
  quota-limited or disabled by default.
* **No transcoding means no rescue.** If a user sends a too-large image from a client that
  ignores the cap, it is rejected, not shrunk by the bridge.
* **Forward secrecy gap extends to media.** Images/voice are encrypted with static ECDH/channel
  keys in v1; a long-term key compromise exposes old media too.

### 3.3 Mitigations

* Enforce caps in both the client (pre-send) and the bridge (server-side reject with a clear
  error); never silently truncate.
* Thumbnail-first flow: send a small preview, then the full image on explicit request.
* Airtime meter in the client (used % of allowance this hour) and per-transfer estimates.
* Document Codec2 playback requirements in the Android README before advertising the feature.
* Per-board store quota configuration in firmware; surface "this repeater stores text only".
* M1 field data to replace the 30 s/25 % budget heuristic with measured values.

## 4. Alternatives considered

| Alternative | Why rejected |
| --- | --- |
| Server-side transcoding in the bridge | Adds CPU, codecs, dependencies and attack surface to a Rust daemon; the client already has the image and the user can preview; violates "bridge is transport, not media service". |
| Opus only (no Codec2) | Unusable on `LONG_*`/`LONG_SLOW`: an 8 s note costs 1.3–9 minutes of airtime; voice would be a fast-profile-only feature, undermining the "rich media on LoRa" goal. |
| Codec2 only | Poor quality for music/general audio, needs a bundled decoder on every client, worse than Opus at the same bitrate on fast profiles. |
| Fixed byte caps for all profiles/regions | Wrong by 2 orders of magnitude; a 12 KiB cap in a 1 % region is 117 % of the hourly budget, and in `LONG_SLOW` it is absurd. Airtime-derived caps are the correct model. |
| Real-time voice calls (LXST-style) | Requires continuous 8+ kbps; impossible in 1 % regions and only viable on `SHORT_*` at short range; needs forward secrecy; not a v1 feature. |
| No media (Meshtastic/MeshCore approach) | Rejected by the product goal; media is the differentiator. But their choice is a constant reminder of the cost. |
| **Client-side encode + resource transport + airtime caps (chosen)** | Keeps the bridge simple, uses native platform codecs, and makes the physical cost visible and enforceable. |

## 5. Follow-ups

1. **M1:** implement the airtime-budget cap function in bridge + clients; add config
   (`region.duty_cycle_override`) and a pre-send estimate test matrix.
2. **M1:** simulator scenarios for image/voice transfers at 0/5/20/50 % chunk loss with airtime
   and wall-clock assertions; publish measured (simulated) results in the README, not invented
   numbers.
3. **M2:** decide Codec2 client support (bundle a decoder via FFI vs desktop-only); measure app
   size and battery impact.
4. **M2:** thumbnail-first UI flow (preview → full) and a "data saver" mode that enforces the
   smallest profile cap.
5. **M3:** OMP/2 ratchets and calls; revisit media encryption then.
6. **M5:** room-server channel history as a resource (`channel_history` kind) with its own cap
   and retention policy; do not reuse store-and-forward media quotas for history.

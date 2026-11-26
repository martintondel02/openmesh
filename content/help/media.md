# Images and voice notes

Photos and voice notes are the most expensive things you can send over a mesh radio. This topic
explains the size tiers, compression and airtime, so you can pick the smallest thing that does the
job.

## What it is

OpenMesh sends media as **resources**: an encrypted blob split into chunks, reassembled and
verified by SHA-256 on the other side. The app compresses images, encodes voice, and shows the
**actual size and estimated airtime** before sending.

* **Images** — four tiers: **Tiny** (320 px), **Medium** (640 px), **High** (1280 px) and
  **Original**. Tiny/Medium/High are compressed WebP; Original is only for backbone/USB transfers
  or when you explicitly accept the cost.
* **Voice notes** — three tiers:
  * **Tiny** — Codec2 at ~1.3 kbps: an 8-second note is about **1.4 KiB** (robotic but
    intelligible).
  * **Medium** — Codec2 at ~3.2 kbps: 8 seconds ≈ **3.2 KiB**.
  * **High** — Opus at ~12 kbps: 8 seconds ≈ **12 KiB**, best quality.
* **Files/attachments** — other file types use the same chunked transfer; they count against the
  same budget and are not optimised for radio.

## Why it matters

A radio channel is shared. One 5 KiB photo at the default profile occupies the air for ~18
seconds, and every relay repeats it. On a 10 % duty-cycle band (EU 869.4–869.65 MHz) that is about
5 % of your hourly transmit budget; on a 1 % band it is half of it. Tiny voice exists so a short
message can get through even when an image cannot: 8 seconds of Codec2 costs less than a
thumbnail.

## Quick start

1. In a chat, tap **attach** and choose a tier — **Tiny** is a good default for photos; for voice,
   **Tiny** is the default on slow profiles.
2. The app compresses/encodes and shows **actual size, estimated airtime and % of the hourly duty
   budget** before sending. If the tier is too big for the current radio profile, the app
   suggests the next smaller one.
3. Send. You will see transfer progress; the recipient can open the media once complete.
4. If the message is marked failed, retry; the chunks are already encrypted and stored locally.

## Advanced notes

### Choosing a tier

| Media tier | What you get | 8 s voice / image size | Airtime at `MEDIUM_FAST` (EU868) |
| --- | --- | --- | --- |
| Image Tiny | 320 px WebP | a few KiB (content-dependent) | a few seconds |
| Image Medium | 640 px WebP | ~10–20 KiB | ~20–40 s |
| Image High | 1280 px WebP | tens of KiB | minutes |
| Voice Tiny | Codec2 1.3 kbps | ≈ 1.4 KiB / 8 s | ~7 s |
| Voice Medium | Codec2 3.2 kbps | ≈ 3.2 KiB / 8 s | ~13 s |
| Voice High | Opus 12 kbps | ≈ 12 KiB / 8 s | ~43 s |

The profile's default cap still applies (e.g. 6 KiB on `LONG_FAST`), and voice duration is capped
at `min(tier max, profile cap / bytes-per-second)` — usually 30 s for Tiny, 20 s for Medium and
15 s for High, shrinking on the slowest profiles (Tiny is only ~8 s on `LONG_SLOW`). The app shows
the effective cap before you record.

### Size caps and airtime by profile (OMP/1 defaults)

| Profile | Default image cap | Image airtime (approx.) | 240 B frame |
| --- | --- | --- | --- |
| `SHORT_TURBO` | 24 KiB | ~12 s | 97 ms |
| `SHORT_FAST` | 24 KiB | ~24 s | 194 ms |
| `MEDIUM_FAST` | 12 KiB | ~42 s | 611 ms |
| `LONG_FAST` | 6 KiB | ~64 s | 2 034 ms |
| `LONG_SLOW` | 1.5 KiB | ~105 s | 13 509 ms |

These numbers are model estimates for a clean channel; retries and relays add more. The cap is
chosen so the transfer fits the profile's practical airtime.

### Transfer mechanics

* `chunk_bytes` = 180 by default; a 5 KiB image is 29 chunks plus a manifest and ACKs.
* Selective repeat with a window of 8 chunks in flight; the receiver sends a window ACK every 8
  chunks or after the RTO, and the sender retransmits only missing chunks.
* Long text uses the same resource machinery, so a very long message can behave like a small file.
* The receiver verifies SHA-256 before decrypting — a corrupted transfer is retried, not shown.
* Voice payloads: tiny/medium are raw Codec2 frames (7 B/40 ms and 8 B/20 ms, 8 kHz mono); high is
  Opus in OGG (or AAC/MP4 where the platform lacks OGG/Opus). Clients play all three formats.

### Cross-client notes

* Web, Android and desktop can all send tiny/medium voice (the codec is pure Rust, compiled to
  WASM for the browser and linked into the Android engine).
* High-quality voice from Android may be AAC/MP4 (API < 29) or OGG/Opus; the other clients play
  both.
* If a peer's app is too old to know a MIME type, the media is stored locally and can be saved as
  a file once the app updates — the mesh transfer itself is unaffected.

### When not to send media

* On `LONG_SLOW`/`LONG_FAST` profiles, prefer text or Tiny voice; a single photo can monopolise
  the channel.
* On a 1 % duty-cycle band, check the airtime warning before sending more than one image per hour.
* During an emergency, short text is more reliable than media; media competes with ACKs.
* If the recipient is on a low-power tracker, media may drain their battery or never fit their
  storage.

## Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| "Not enough airtime" warning | The tier exceeds the profile cap or duty budget | Choose a smaller tier (Tiny/Medium) or wait for the duty budget to refill. |
| Voice note is unintelligible | Tiny Codec2 is very low bitrate | Use Medium or High for important messages. |
| Voice playback is silent | Missing codec support (old client) or muted media | Update the app; check media volume; sender can resend as High. |
| Image looks blocky | Tiny/Medium downscale aggressively | Use High for details, but check the airtime estimate first. |
| Transfer stalls at 100% | Recipient storage/permission issue or ACK lost | Retry; check both devices' free space. |

## Related topics

[Radios, regions and airtime](radio-and-region.md) · [Messaging](messaging.md) ·
[Legal limits, power and emergency override](regulatory-and-emergency.md) ·
[Troubleshooting](troubleshooting.md)

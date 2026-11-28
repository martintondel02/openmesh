# Images and voice notes

Photos and voice notes are the most expensive things you can send over a mesh radio. This topic
explains the size tiers, compression and airtime, so you can pick the smallest thing that does the
job.

## What it is

OpenMesh sends media as **resources**: an encrypted blob split into chunks, reassembled and
verified by SHA-256 on the other side. The app compresses images, encodes voice, and shows the
**actual size and estimated airtime** before sending.

* **Images** — four tiers: **Tiny** (≤256 px), **Medium** (≤512 px), **High** (≤1024 px) and
  **Original**. Tiny/Medium/High are compressed: the app encodes both **AVIF** (a newer compressed
  image format) and **WebP** at matching visual quality and sends whichever file is smaller (WebP
  wins a tie). If the AVIF encoder cannot run on your device, it sends WebP (or JPEG) with the
  correct file type. Original is only for backbone/USB transfers or when you explicitly accept the
  cost.
* **Voice notes** — three tiers:
  * **Tiny** — Codec2 at ~1.3 kbps: an 8-second note is about **1.4 KiB** (robotic but
    intelligible).
  * **Medium** — Codec2 at ~3.2 kbps: 8 seconds ≈ **3.2 KiB**.
  * **High** — Opus at ~12 kbps: 8 seconds ≈ **12 KiB**, best quality.
* **Files/attachments** — other file types use the same chunked transfer; they count against the
  same budget and are not optimised for radio.

## Why it matters

A radio channel is shared. One Medium photo from our test (512 px, about 7 KiB) keeps the air
busy for around 25 seconds at the default radio profile, and every relay repeats it. On a 10 %
duty-cycle band (EU 869.4–869.65 MHz) that is roughly 7 % of your hourly transmit budget; a Tiny
photo (about 2.6 KiB) uses about 2.5 %. On a 1 % band the Medium photo alone is most of the hourly
budget. Tiny voice exists so a short message can get through even when an image cannot: 8 seconds
of Codec2 costs less than a thumbnail.

## Quick start

1. In a chat, tap **attach** and choose a tier — **Tiny** is a good default for photos; for voice,
   **Tiny** is the default on slow profiles.
2. The app compresses/encodes and shows **actual size, format, estimated airtime and % of the
   hourly duty budget** before sending. The tier tiles show the format Auto picked for that image —
   the smaller of AVIF and WebP. If the tier is too big for the current radio profile, the app
   suggests the next smaller one.
3. Send. You will see transfer progress. On the receiving side the bubble shows **Receiving…**
   until the transfer completes, then the image renders inline (tap it for full size) or the voice
   note plays in the thread (tap to play, tap again to stop).
4. If the message is marked failed, retry; the chunks are already encrypted and stored locally.

## Advanced notes

### Choosing a tier

| Media tier | What you get | 8 s voice / test-photo size | Airtime at `MEDIUM_FAST` (EU868) |
| --- | --- | --- | --- |
| Image Tiny | ≤256 px, Auto (smaller of AVIF/WebP) | ≈2.6 KiB in our test photo | ~9 s |
| Image Medium | ≤512 px, Auto (smaller of AVIF/WebP) | ≈7.3 KiB | ~24 s |
| Image High | ≤1024 px, Auto (smaller of AVIF/WebP) | ≈16 KiB (larger photos more) | ~51 s |
| Voice Tiny | Codec2 1.3 kbps | ≈ 1.4 KiB / 8 s | ~7 s |
| Voice Medium | Codec2 3.2 kbps | ≈ 3.2 KiB / 8 s | ~13 s |
| Voice High | Opus 12 kbps | ≈ 12 KiB / 8 s | ~43 s |

The image figures are **measured examples**, not guarantees: one 768×512 test photo in a desktop
browser (November 2026), with no relays or retries, encoded to roughly 2.6 KiB / 7.3 KiB / 16 KiB
for Tiny / Medium / High. Your photo and encoder will differ — the app always shows the real bytes
before sending. The app downloads its AVIF encoder the first time you send an image (about 400 KiB
compressed, only once per session, in the background); if that download or your device cannot run
it, the app sends WebP with the correct `image/webp` type instead. Content-dependent comparison
means some photos come out smaller as WebP and are sent that way. Received AVIF images render
inline on clients that can decode them.

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

* Web, Android and desktop can all send and play tiny/medium voice (Codec2 is pure Rust, compiled
  to WASM for the browser and linked into the Android engine). Received images render inline in
  the conversation on both clients; no external gallery or player app is needed.
* A voice note or image that arrives without a duration or with a transfer still in progress is
  labelled honestly ("Receiving…", duration computed from the Codec2 payload size) instead of
  showing a fake placeholder.
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
| Image shows only a filename, or "not on this phone yet" | The transfer never completed, or an older client without media previews | Wait for "Receiving…" to finish and tap the image; update the app; ask the sender to reseat the message. |
| Transfer stalls at 100% | Recipient storage/permission issue or ACK lost | Retry; check both devices' free space. |

## Related topics

[Radios, regions and airtime](radio-and-region.md) · [Messaging](messaging.md) ·
[Legal limits, power and emergency override](regulatory-and-emergency.md) ·
[Troubleshooting](troubleshooting.md)

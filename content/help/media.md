# Images and voice notes

Photos and voice notes are the most expensive things you can send over a mesh radio. This topic
explains compression, size caps and airtime, so you can decide when a picture is worth the
channel time.

## What it is

OpenMesh sends media as **resources**: an encrypted blob split into chunks, reassembled and
verified by SHA-256 on the other side. The app compresses images (and encodes voice) before
uploading, and shows an estimated airtime before sending.

* **Images** — compressed client-side to fit the profile's cap; sent as a manifest plus chunks.
* **Voice notes** — Opus (default, ~6–10 kbps) or the optional Codec2 700C mode (~700 bytes for
  8 seconds) for extremely low-bandwidth links.
* **Files/attachments** — other file types use the same chunked transfer; they count against the
  same budget and are not optimised for radio.

## Why it matters

A radio channel is shared. One 5 KiB photo at the default profile occupies the air for ~18
seconds, and every relay repeats it. On a 1 % duty-cycle band that can be half your hourly
transmit budget. Sending a full-resolution photo directly is impossible; the app's compression
and caps are what make images work at all.

## Quick start

1. In a chat, tap the **attach** button (camera/image or voice).
2. For images, the app compresses to the target size and shows **estimated airtime** — confirm
   only if it is acceptable.
3. For voice, hold to record; the app shows duration and size. On slow profiles, note the cap
   (e.g. 4 s on `LONG_SLOW`).
4. Send. You will see transfer progress; the recipient can open the media once complete.
5. If the message is marked failed, retry; the chunks are already encrypted and stored locally.

## Advanced notes

### Size caps and airtime by profile (OMP/1 defaults)

| Profile | Default image cap | Image airtime (approx.) | Default voice cap | 240 B frame |
| --- | --- | --- | --- | --- |
| `SHORT_TURBO` | 24 KiB | ~12 s | 15 s | 97 ms |
| `SHORT_FAST` | 24 KiB | ~24 s | 15 s | 194 ms |
| `MEDIUM_FAST` | 12 KiB | ~42 s | 10 s | 611 ms |
| `LONG_FAST` | 6 KiB | ~64 s | 8 s | 2 034 ms |
| `LONG_SLOW` | 1.5 KiB | ~105 s | 4 s | 13 509 ms |

The cap is chosen so the transfer fits the profile's practical airtime; a 24 KiB image is only
allowed where the radio can move it. Below the cap, times scale roughly with size. These numbers
are model estimates and assume a clean channel — retries add more.

### Transfer mechanics

* `chunk_bytes` = 180 by default; a 5 KiB image is 29 chunks plus a manifest and ACKs.
* Selective repeat with a window of 8 chunks in flight; the receiver sends a window ACK every 8
  chunks or after the RTO (3× expected ACK time), and the sender retransmits only missing chunks.
* Long text uses the same resource machinery, so a very long message can behave like a small file.
* The receiver verifies SHA-256 over the reassembled ciphertext before decrypting — a corrupted
  transfer is retried, not displayed.
* Voice metadata includes duration; images include dimensions/mime so the UI can render
  placeholders before completion.

### When not to send media

* On `LONG_SLOW`/`LONG_FAST` profiles, prefer text; a single photo can monopolise the channel.
* On a 1 % duty-cycle region, check the app's airtime warning before sending more than one image
  per hour.
* During an emergency, short text is more reliable than a picture; media competes with ACKs.
* If the recipient is on a low-power tracker, media may drain their battery or never fit their
  storage.

### Codec2 (optional)

For voice on very slow links, the app may offer Codec2 700C (`audio/c2-700c`): ~700 bytes for
8 seconds, intelligible but robotic. It is the difference between "a sentence gets through" and
"no voice at all" on `LONG_SLOW`. Support depends on both clients; the app falls back to Opus
when the recipient cannot decode it.

## Troubleshooting

| Symptom | Check | Fix |
| --- | --- | --- |
| Attach button disabled | Profile cap too small / airtime budget exhausted | Switch profile, send text, or wait for the duty-cycle window. |
| Transfer stalls at a percentage | Lost chunks or peer out of range | Retry; move closer; check the peer is still connected. |
| Recipient sees "unsupported media" | Old client or missing codec (e.g. Codec2) | Update both clients, or resend as Opus/image. |
| Audio is unreadable | Too much compression or very low profile | Record shorter, speak closer to the mic, or switch profile. |
| Image looks blocky | Compression to fit the cap | Expected; lower resolution is the price of radio. |
| Repeated media sends at same time | Automatic retry plus manual retry | Wait for automatic retries to finish before tapping retry. |

## Related topics

[Radios, regions and airtime](radio-and-region.md) · [Messages, delivery and offline mail](messaging.md) ·
[Troubleshooting](troubleshooting.md)

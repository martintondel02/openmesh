# LoRa capacity and media — quantitative analysis

**Status:** research note · **Last updated:** 2026-11-25
**Purpose:** answer, with numbers rather than vibes: what do OMP/1 frames cost, how long do
images and voice notes take on each `LoraProfile`, what do duty cycles do to those answers, how
many messages can a mesh carry, and what should the UI say to users.

Related: [Protocol](../architecture/PROTOCOL.md) §9/§12/Appendix A ·
[Architecture](../architecture/ARCHITECTURE.md) §11 ·
[ADR 0007](../adr/0007-media-pipeline.md) ·
[Meshtastic/MeshCore comparison](meshtastic-vs-meshcore-vs-openmesh.md) ·
[Reticulum notes](reticulum-lxmf-notes.md) §4.2.

---

## 1. Method and reproducibility

### 1.1 The airtime formula

All airtime values in this note come from an independent reimplementation of the Semtech LoRa
airtime formula (AN1200.13/AN1200.22), written from the formula rather than copied from
`crates/openmesh-core/src/util.rs`:

```
Tsym          = 2^SF / BW
Tpreamble     = (n_preamble + 4.25) * Tsym
PL            = payload_bytes + (2 if CRC else 0)
IH            = 0 with explicit header, 1 with implicit header
DE            = 1 when SF >= 11 and BW == 125 kHz (low-data-rate optimisation), else 0
n_payload     = max(ceil((8*PL - 4*SF + 28 - 20*IH) / (4*(SF - 2*DE))), 0)
n_symbols     = 8 + n_payload * (CR + 4)          # CR = 1..4 for 4/5..4/8
airtime_ms    = ceil((Tpreamble + n_symbols * Tsym) * 1000)
```

Profiles, matching `crates/openmesh-core/src/util.rs` and (for the overlapping presets) the
Meshtastic 2.8 preset table:

| OpenMesh profile | SF | BW | CR | Preamble | Nominal data rate |
| --- | --- | --- | --- | --- | --- |
| `LONG_SLOW` | 12 | 125 kHz | 4/8 | 16 | 183 bps |
| `LONG_FAST` | 11 | 250 kHz | 4/5 | 16 | 1.07 kbps |
| `MEDIUM_FAST` (default) | 9 | 250 kHz | 4/5 | 16 | 3.52 kbps |
| `SHORT_FAST` | 7 | 250 kHz | 4/5 | 16 | 10.94 kbps |
| `SHORT_TURBO` | 7 | 500 kHz | 4/5 | 16 | 21.88 kbps |

The nominal-rate formula `SF * (BW / 2^SF) * 4/(4+CR)` reproduces the Meshtastic table for
`SHORT_TURBO` (21.88), `SHORT_FAST` (10.94), `MEDIUM_FAST` (3.52), `LONG_FAST` (1.07) and the
custom presets; it is a good sanity check that the parameters are understood. Note that
`LONG_SLOW` in OpenMesh (SF12/125/CR4/8, 183 bps) is not one of Meshtastic's nine presets — it
is the slowest custom configuration we support.

The formula is deterministic and easy to re-check: `cargo test -p openmesh-core` contains a
plausibility test around the same numbers, and Appendix A of PROTOCOL.md is generated from it.

### 1.2 Frame sizes used

Sizes are measured, not assumed: the `services.rs` schemas were serialised with `ciborium`
(a throwaway `/tmp` crate depending on `openmesh-core`) and combined with the envelope layout in
PROTOCOL.md §5/§9. After the protocol fixes of 2026-11-25:

| Frame content | Body bytes | Frame bytes (22-byte header) |
| --- | --- | --- |
| Header only (empty body) | 0 | 22 |
| ACK, delivered (CBOR ≈ 65 B + 33 B envelope) | 98 | 120 |
| ACK, resource window (CBOR ≈ 88 B + 33 B) | 121 | 143 |
| Announce, 12-char name (binary, 152 + 12) | 164 | 186 |
| Resource manifest (CBOR ≈ 136 B + 33 B) | 169 | 191 |
| 100-char direct text, first frame without identity (CBOR 146 + 33) | 179 | 201 |
| Resource chunk, 180-byte chunk (1+4+4+2 + 180) | 191 | 213 |
| 100-char direct text **with** identity (146 + 33 + 72) | 251 | 273 |
| Maximum OMP body | 218 | 240 |
| Maximum announce (32-char name) | 184 | 206 |

Implications already worth recording:

* A 100-character DM fits one frame **only if the receiver already knows the sender's identity**
  (201 B). The first message to an unknown peer needs `IDENTITY_INCLUDED` and becomes 273 B → it
  cannot be one `Data` frame; the client must either send a short intro, split into two messages,
  or use the long-text resource path. This is a UX/protocol nuance the clients must handle.
* A group message (channel) has 8 B sender id + 64 B signature inside the sealed body; the spec
  correctly limits single-frame channel text to about 60 characters and mandates resources
  beyond that.
* Announce now fits comfortably (186 B for a 12-char name; 206 B worst case) after the binary
  announce fix.

### 1.3 Verification against PROTOCOL.md Appendix A

The independent implementation reproduces the spec's numbers:

| Quantity | Spec (Appendix A) | This note | Delta |
| --- | --- | --- | --- |
| 240-byte frame, `MEDIUM_FAST` | 611 ms | 611 ms | exact |
| 100-char text, 201 B frame | 519 ms | 519 ms | exact |
| Announce, 12-char name | 478 ms for 185 B | 488 ms for 186 B | +10 ms (1 byte + preamble rounding) |
| 5 KiB image (manifest + 29 chunks + ACKs) | ~17.8 s | 18.3 s | +0.5 s (ACK counting) |
| 8 s Opus voice | ~23.3 s | 24.2 s | +0.9 s (ACK counting) |

Deltas of ≤3 % are acceptable for planning; they come from how many window ACKs are counted
(one per 8 chunks plus a completion ACK) and from the exact announce frame length. The one
number that was *not* reproducible before the fix was the 100-char text row: it quoted 610 ms,
which is the airtime of a full 240-byte frame, not of the 197–201-byte text frame. That was one
of the two wire-budget bugs found by this research (see
[ai-development-workflow.md](ai-development-workflow.md) §6); the spec has been corrected.

Also note: the test comment in `crates/openmesh-core/src/util.rs` says a 240-byte frame at
SF9/250 kHz takes "roughly 0.3–0.4 s". The actual value is 611 ms; the assertion range
(450–900 ms) passes, so the test is fine but the comment is stale and should be corrected by the
core workstream.

---

## 2. Airtime of individual frames

All values in milliseconds, computed with the formula above. "—" means the profile is not
supported for that payload in one frame (over MTU).

| Frame | Bytes | `LONG_SLOW` | `LONG_FAST` | `MEDIUM_FAST` | `SHORT_FAST` | `SHORT_TURBO` |
| --- | --- | --- | --- | --- | --- | --- |
| Header only | 22 | 2 237 | 396 | 120 | 33 | 17 |
| ACK delivered | 120 | 7 218 | 1 133 | 335 | 105 | 53 |
| ACK resource window | 143 | 8 528 | 1 297 | 386 | 122 | 61 |
| Announce (12-char name) | 186 | 10 888 | 1 625 | 488 | 153 | 77 |
| Resource manifest | 191 | 11 150 | 1 666 | 499 | 158 | 79 |
| 100-char DM (known peer) | 201 | 11 674 | 1 747 | 519 | 163 | 82 |
| Resource chunk (180 B) | 213 | 12 198 | 1 829 | 550 | 174 | 87 |
| Max OMP frame | 240 | 13 509 | 2 034 | 611 | 194 | 97 |
| 100-char DM + identity | 273 | 15 344 | 2 280 | 683 | 217 | 109 |

Read that table as the **cost of one transmission**. In a flooding mesh, every node that hears
a frame and has TTL left retransmits it, so a message heard by N relays costs roughly N times
its row (ignoring dedup and collisions). A 10-node cluster therefore spends ~5.2 s of aggregate
airtime per 100-char text at `MEDIUM_FAST` (10 × 519 ms).

### 2.1 Per-byte efficiency

The useful-payload efficiency of a resource chunk frame is 180 / 213 = **84.5 %**. Per chunk the
overhead is 33 bytes (22 header + 1 flags + 4 msg_ref + 4 res_id + 2 index); the manifest adds
191 bytes and each window ACK 143 bytes. For a 5 KiB image (29 chunks, 4 window ACKs + 1
completion ACK + 1 manifest):

```
overhead  = 191 + 5*143 + 29*33 = 191 + 715 + 957 = 1 863 B
payload   = 5 120 B
wire      = 6 983 B      → 27 % overhead, 73 % payload efficiency
```

Compare Reticulum's link MDU of 431 bytes with 48-byte token overhead (~90 % payload
efficiency) — Reticulum amortises better because it has larger packets and no per-frame
addressing. Ours is the price of a 1-frame MTU on MCUs; it is acceptable for our size caps.

### 2.2 Time per character

At `MEDIUM_FAST`, a 100-character message costs 519 ms → **5.2 ms per character** at one hop.
At `LONG_SLOW` it is 117 ms/char; at `SHORT_TURBO` 0.8 ms/char. This is a useful UX number:
"typing 200 characters costs about a second of channel time per relay in range".

---

## 3. Image transfer times

Assumptions: default 180-byte chunks; one manifest frame (191 B); one window ACK (143 B) per 8
chunks; one completion ACK (120 B); no loss and no retransmissions (a lossy channel adds
retransmits, see §5). "Wall @1 %" is the wall-clock time if a node is limited to 1 % duty cycle
and uses its whole budget for this transfer (i.e. `airtime × 100 / 3600` hours). "Wall @10 %"
is the same with a 10 % duty cycle (the EU 869.4–869.65 MHz sub-band, which Meshtastic documents
as 10 %; our conservative default profile assumes 1 % everywhere in EU868).

### 3.1 Default attachment caps (PROTOCOL.md §9.4)

| Profile | Cap | Chunks | Frames (incl. ACKs) | Airtime | % of 1 % hourly budget | Wall @1 % | Wall @10 % |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `SHORT_TURBO` | 24 KiB | 137 | 157 | **13.1 s** | 36 % | 22 min | 2.2 min |
| `SHORT_FAST` | 24 KiB | 137 | 157 | **26.3 s** | 73 % | 44 min | 4.4 min |
| `MEDIUM_FAST` | 12 KiB | 69 | 80 | **42.3 s** | 117 % | 1 h 10 min | 7.0 min |
| `LONG_FAST` | 6 KiB | 35 | 42 | **73.3 s** | 204 % | 2 h 02 min | 12.2 min |
| `LONG_SLOW` | 1.5 KiB | 9 | 13 | **145.2 s** | 403 % | 4 h 02 min | 24.2 min |

Two observations:

1. **"% of 1 % hourly budget" > 100 %** means the transfer cannot complete in one hour on a
   single 1 %-duty node — it must spread over multiple hours. For `LONG_SLOW` the cap is
   physically absurd under a 1 % regime: a 1.5 KiB attachment takes ~4 hours of wall clock.
2. `SHORT_TURBO`/`SHORT_FAST` caps are the only ones that fit inside a 1 % hourly budget
   (36 s). `MEDIUM_FAST` at 42 s is slightly over, `LONG_FAST` is 2× over.

### 3.2 Common sizes on every profile

| Image size | `LONG_SLOW` | `LONG_FAST` | `MEDIUM_FAST` | `SHORT_FAST` | `SHORT_TURBO` |
| --- | --- | --- | --- | --- | --- |
| 1 KiB | 100.1 s | 15.1 s | 4.5 s | 1.4 s | 0.7 s |
| 2 KiB | 181.8 s | 27.3 s | 8.2 s | 2.6 s | 1.3 s |
| 5 KiB | 406.2 s | 61.0 s | 18.3 s | 5.8 s | 2.9 s |
| 12 KiB | 936.8 s | 140.7 s | 42.3 s | 13.4 s | 6.7 s |
| 24 KiB | 1 843.0 s | 276.7 s | 83.1 s | 26.3 s | 13.1 s |

At `LONG_SLOW`, even a 1 KiB thumbnail is over a minute of airtime. At `MEDIUM_FAST`, a 5 KiB
image is ~18 s, which is the spec's worked example. At `SHORT_FAST`, images are genuinely
usable (5 KiB in ~6 s).

### 3.3 Voice note transfer times

Opus at 7 kbps (the spec's default target) and Codec2 700C at 0.7 kbps, with the same manifest +
ACK assumptions:

| Duration | Codec | Bytes | Chunks | `LONG_SLOW` | `LONG_FAST` | `MEDIUM_FAST` | `SHORT_FAST` | `SHORT_TURBO` |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 4 s | Opus 7 kbps | 3 500 | 20 | 287.9 s | 43.3 s | 13.0 s | 4.1 s | 2.1 s |
| 4 s | Codec2 700C | 350 | 2 | 51.3 s | 7.8 s | 2.3 s | 0.7 s | 0.4 s |
| 8 s | Opus 7 kbps | 7 000 | 39 | 536.7 s | 80.6 s | 24.2 s | 7.7 s | 3.8 s |
| 8 s | Codec2 700C | 700 | 4 | 75.7 s | 11.4 s | 3.4 s | 1.1 s | 0.5 s |
| 10 s | Opus 7 kbps | 8 750 | 49 | 675.8 s | 101.5 s | 30.5 s | 9.6 s | 4.8 s |
| 10 s | Codec2 700C | 875 | 5 | 87.9 s | 13.2 s | 4.0 s | 1.3 s | 0.6 s |
| 15 s | Opus 7 kbps | 13 125 | 73 | 994.1 s | 149.3 s | 44.8 s | 14.2 s | 7.1 s |
| 15 s | Codec2 700C | 1 313 | 8 | 124.5 s | 18.7 s | 5.6 s | 1.8 s | 0.9 s |

The conclusion is blunt: **Opus voice notes are only practical on `SHORT_*` profiles.** On
`LONG_*` and `LONG_SLOW`, the 5 % duty cycle and long airtimes make an 8 s Opus note cost minutes
to hours of channel time, and the user experience ("recorded 8 s, sends for 9 minutes") is
unacceptable. Codec2 700C is 8–10× cheaper and should be the default (or the only option) for
`LONG_*`/`LONG_SLOW`. This refines ARCHITECTURE.md §4 decision 7 ("Opus for voice, Codec2
optional"): Opus is right for capable presets and clients, but Codec2 is *required* for the
constrained presets, not merely optional.

---

## 4. Duty-cycle budgets and regions

`crates/openmesh-core/src/util.rs` defines the region profiles:

| Region | Centre | Max power | Duty cycle in the model | Notes |
| --- | --- | --- | --- | --- |
| `EU868` | 869.525 MHz | 14 dBm | 1 % | conservative default; the 869.4–869.65 MHz sub-band is documented by Meshtastic as 10 % |
| `US915` | 915.000 MHz | 20 dBm | none (fair use) | FCC part 15.247; dwell-time interpretations should be checked for long frames |
| `AU915` | 915.000 MHz | 20 dBm | none (fair use) | ACMA LIPD |
| `IN865` | 865.000 MHz | 20 dBm | none (fair use) | WPC rules |
| `KR920` | 920.000 MHz | 14 dBm | none (fair use) | KC rules |
| `AS923` | 923.200 MHz | 14 dBm | 1 % | varies by country |

The duty-cycle model in the code is intentionally simple (one percentage for the whole band).
Real regulations are per-sub-band, per-power and sometimes per-channel; before shipping,
verify against the local regulator and update `REGIONS` with the real sub-band data. The
capacity consequences are enormous:

| Duty | Seconds of TX per hour | Seconds of TX per day |
| --- | --- | --- |
| 1 % | 36 s | 14.4 min |
| 10 % | 360 s | 2.4 h |
| none | limited by medium/fair use | — |

### 4.1 What 36 s/hour buys

At `MEDIUM_FAST`, 36 s of hourly airtime is:

* 69 max-size frames,
* ~42 complete 100-char text exchanges (data + ACK = 854 ms each),
* ~2 × 5 KiB images (18.3 s each) if imaging is the only traffic — i.e. one image every
  ~30 minutes while spending the whole budget,
* ~1.5 Opus 8 s voice notes (24.2 s each),
* ~10.6 Codec2 8 s voice notes (3.4 s each).

At `LONG_FAST`, the same 36 s is:

* 12.5 text exchanges,
* ~0.6 of a 5 KiB image (61 s),
* ~0.45 of an Opus 8 s note (80.6 s),
* ~3 Codec2 8 s notes (11.4 s).

At `LONG_SLOW`, 36 s is:

* 1.9 text exchanges,
* no 1 KiB image (100.1 s) in one hour,
* no Opus voice note,
* ~0.5 of a Codec2 8 s note (75.7 s).

### 4.2 Announce overhead

Announces are background cost. With the fixed 186-byte announce and 30-minute cadence, each
client spends `2 × 488 ms = 0.98 s/hour` at `MEDIUM_FAST` = **0.027 % duty**. Repeaters at 60
minute cadence spend half that. Even at `LONG_SLOW` (2 × 10.9 s = 21.8 s/hour = 0.61 %) announces
are a real fraction of a 1 % budget — another argument for not using the slowest profile as a
default anywhere. Across a 10-node mesh, the *aggregate* announce airtime at `MEDIUM_FAST` is
10 × 0.98 s = 9.8 s/hour, i.e. 0.27 % occupancy of the shared channel — negligible, but it
doubles if the 15-minute cadence is ever proposed, and at `LONG_SLOW` ten nodes announcing
twice an hour would occupy 6.1 % of the channel with nothing but announces.

---

## 5. Loss, ARQ and retransmission overhead

The numbers above assume a perfect channel. Real LoRa at range loses frames. With the
selective-repeat ARQ in PROTOCOL.md §9.3 (window 8, ACK every 8 chunks, 6 retries):

| Loss rate | Extra airtime for a 5 KiB image at `MEDIUM_FAST` | Notes |
| --- | --- | --- |
| 0 % | 18.3 s | baseline |
| 5 % of chunks | ~19.2 s | 1–2 chunks retransmitted |
| 20 % of chunks | ~22.9 s | ~6 chunks retransmitted, one window ACK per retry |
| 50 % of chunks | ~36 s | transfer takes ~2× airtime; still completes if ACKs get through |
| 80 % of chunks | likely fails | window ACKs also lossy; sender gives up after 6 retries |

Loss also costs *wall-clock* time because retransmission RTO is at least 1.5 s and doubles
(3 s, 6 s, …) — a 20 % loss transfer can take 2–3× longer than airtime alone suggests, since the
sender waits for ACKs that may never come. The `RESOURCE_RTO_MS = 3000` default plus exponential
backoff means a single lost window can add ~6–12 s of silence. UX should show "retrying (x/6)"
rather than a stalled progress bar.

The simulator (`crates/openmesh-sim`) already has a lossy-channel scenario; the media roadmap
should add asserted scenarios for 20 % and 50 % chunk loss with airtime accounting (see
[ROADMAP.md](../ROADMAP.md) M1).

---

## 6. Network capacity estimates

These are order-of-magnitude models, not benchmark results. They assume a single channel,
perfect carrier sense (LoRa has none), no hidden terminals, and flooding where every node relays
every frame once. Real capacity is lower; use the models to compare profiles and to size
defaults, not to promise throughput.

### 6.1 Model

For a mesh of `N` nodes, each node's transmitter is limited to duty `d` (1 %, 10 %, or fair
use). A short text exchange costs `A = A(data) + A(ack)` seconds of airtime per transmission
per node that relays it:

```
per-node messages/hour     ≤ (d * 3600) / A
aggregate transmissions/h  = M * N * A / 3600        (each message relayed by all N nodes)
channel occupancy          = M * N * A / 3600
constraint                 = occupancy < U * N      (U = per-node duty, since each node can
                                                     transmit at most U of the time)
⇒ M ≤ (U * 3600) / A        independent of N
```

So the per-node duty cycle, not the node count, sets the *order of magnitude* of message
throughput; node count sets collision probability and latency. A practical design target is
channel occupancy ≤ 10–20 %, which for N nodes each capped at 1 % means N ≤ 10–20 at full load.
Beyond that, increase the cap, split channels/profiles, or add directed routing.

### 6.2 Short text capacity

`A = A(201 B) + A(120 B)` (100-char DM + ACK):

| Profile | A (one hop) | Messages/hour at 1 % duty | Messages/hour at 10 % duty | Messages/hour fair-use (assume 10 % target) |
| --- | --- | --- | --- | --- |
| `LONG_SLOW` | 18.9 s | **1.9** | 19 | 19 |
| `LONG_FAST` | 2.88 s | **12.5** | 125 | 125 |
| `MEDIUM_FAST` | 0.85 s | **42.2** | 421 | 421 |
| `SHORT_FAST` | 0.27 s | **134** | 1 343 | 1 343 |
| `SHORT_TURBO` | 0.14 s | **267** | 2 667 | 2 667 |

These are *per-channel* ceilings with every node relaying once. Divide by N for the effective
per-node rate when the channel is saturated: at `MEDIUM_FAST`/1 % and N=10, each node gets
~4.2 messages/hour if all nodes are equally active, while total channel throughput is ~42
messages/hour. Collisions and MAC overhead will cut this by 30–70 %; a safe planning number is
**20–30 messages/hour for a 10-node `MEDIUM_FAST` mesh in a 1 % region**, and 3–10× that in 10 %
or fair-use regions.

### 6.3 Media capacity

Using the default caps (airtime per transfer from §3.1) and the same `M ≤ U*3600/A` model for a
10-node mesh all of whose nodes have 1 % (or 10 %) budgets:

| Profile | Airtime per capped image | Capped images/hour, 10-node 1 % | Capped images/hour, 10-node 10 % |
| --- | --- | --- | --- |
| `SHORT_TURBO` (24 KiB) | 13.1 s | 27.5 | 275 |
| `SHORT_FAST` (24 KiB) | 26.3 s | 13.7 | 137 |
| `MEDIUM_FAST` (12 KiB) | 42.3 s | 8.5 | 85 |
| `LONG_FAST` (6 KiB) | 73.3 s | 4.9 | 49 |
| `LONG_SLOW` (1.5 KiB) | 145.2 s | 2.5 | 25 |

Again, "10-node 1 %" is optimistic (no loss, perfect relaying). A useful way to say it in the
UI: **at `MEDIUM_FAST` in EU868 (1 %), a 12 KiB image is roughly a 1-hour-channel-budget event
for the sender and about 1/8 of the whole 10-node mesh's hourly capacity.** That is the honest
description of rich media over LoRa: possible, scarce, and worth showing the price of.

### 6.4 Store-and-forward capacity

Default `RelayConfig { store_capacity: 64, store_ttl_ms: 72 h }` in
`crates/openmesh-core/src/engine.rs`. Envelope storage cost = ciphertext bytes + metadata
(~40 B per envelope). For media:

| Envelope | Wire bytes per frame | Frames | Stored ciphertext | 64 envelopes |
| --- | --- | --- | --- | --- |
| Text (200 B) | 201 | 1 | ~180 B | ~12 KiB |
| 1.5 KiB image | 213 | 13 | ~2.2 KiB | ~141 KiB |
| 6 KiB image | 213 | 42 | ~8.6 KiB | ~550 KiB |
| 12 KiB image | 213 | 80 | ~16.5 KiB | ~1.06 MiB |
| 24 KiB image | 213 | 157 | ~32.7 KiB | ~2.1 MiB |

An nRF52840 has 1 MiB flash total (minus bootloader/application); an ESP32-S3 module typically
has 4–16 MiB. Filling a 64-envelope queue with 12 KiB images needs ~1 MiB of storage, which will
not fit on the nRF52 repeaters that are the most likely store-and-forward nodes. Conclusion:

* Store **text envelopes** by default (64 × text ≈ 12–20 KiB — trivial).
* Optionally store **small resources** with a separate resource quota (e.g. 16 envelopes or
  64 KiB total, whichever is smaller), never a flat 64-envelope media queue.
* Say "text store-and-forward" in the UI. Do not promise photo delivery through a solar
  repeater unless the firmware config explicitly enables media storage and the UI reflects it.
* Do not store channel-addressed frames at all (PROTOCOL §10 now says this explicitly; channel
  history is a room-server feature, M5).

---

## 7. Recommendations: caps, defaults, and what the UI must say

### 7.1 Replace byte caps with airtime caps (spec change proposal)

The current caps in PROTOCOL.md §9.4 are per-profile byte counts. The model above shows the real
constraint is **airtime per transfer within a duty-cycle budget**, and the bytes-per-second of
each profile differ by 100×. Recommend expressing caps as:

```
max_attachment_bytes(profile, region) =
    (airtime_budget_ms(profile, region) / 1000) / airtime_per_chunk_s(profile)
    * chunk_payload_bytes (180) * efficiency_factor (0.8)
```

with `airtime_budget_ms` defaulting to `min(30_000, duty_cycle_pct/100 * 3600_000 / 4)` —
i.e. at most 30 s of airtime and at most a quarter of the hourly budget. This yields:

| Profile / region | Budget | Suggested cap | Spec cap today | Comment |
| --- | --- | --- | --- | --- |
| `SHORT_TURBO` / US915 | 30 s | 24 KiB (cap binds; 30 s would allow ~49 KiB) | 24 KiB | keep 24 KiB |
| `SHORT_FAST` / US915 | 30 s | 24 KiB (cap binds; 30 s would allow ~25 KiB) | 24 KiB | close; keep 24 KiB |
| `MEDIUM_FAST` / US915 | 30 s | ~8 KiB | 12 KiB | tighten to 8 KiB |
| `MEDIUM_FAST` / EU868 1 % | 9 s (quarter of 36 s) | ~2.5 KiB | 12 KiB | **much tighter**; 12 KiB exceeds the hourly budget |
| `LONG_FAST` / EU868 1 % | 9 s | ~0.8 KiB | 6 KiB | **7× tighter** |
| `LONG_SLOW` / EU868 1 % | 9 s | ~0.13 KiB (text only) | 1.5 KiB | images effectively impossible |

The exact numbers are less important than the principle: **caps must be computed from the
region's duty cycle**, and the UI should present them as time, not bytes. If the spec keeps
byte caps for simplicity, at minimum add a rule: "in 1 % duty regions the sender MUST clamp to
the airtime budget (30 s and ≤25 % of the hourly budget), even if that is below the per-profile
byte cap."

### 7.2 What the pre-send UI should show

For every image or voice note, before sending:

1. **Airtime estimate** in seconds and as "how long the channel is busy for one transmission"
   per relay in range. Example: "≈18 s of airtime · ~6 min to send at 1 % duty".
2. **Budget impact**: a small bar "this month? no — this hour: uses 50 % of your hourly airtime
   allowance". Reject when > budget, require explicit confirmation when > 25 %.
3. **Alternatives**: "send 2 KiB thumbnail" (≈7 s at MF), "send as text", "wait for
   `SHORT_FAST` coverage".
4. **Honest relay wording**: "A repeater may store this for up to 72 h; that is not proof of
   delivery." (This applies to the store-forward badge, not to media specifically.)
5. **Current channel activity**: airtime used this hour, queue depth, current profile and
   region, and a note that switching profile changes what is possible ("`LONG_SLOW` selected:
   images will take minutes; voice is Codec2-only").

Never show a progress bar in bytes only; show chunks *and* remaining airtime. A stalled ARQ
retry should say "retrying (2/6)" so users do not think the app hung.

### 7.3 Recommended image pipeline

| Step | Setting | Rationale |
| --- | --- | --- |
| Longest edge | 1024 px default, 768 px on `LONG_*`, 512 px on `LONG_SLOW` | 5 KiB at q≈60 is roughly a 768–1024 px JPEG; bigger is wasted on LoRa |
| Format | JPEG for photos, WebP for screenshots/graphics, PNG only with explicit warning | WebP/JPEG are 3–10× smaller than PNG for photos |
| Quality | JPEG 60 (default), 45 (slow profiles) | Below ~40 artefacts dominate at thumbnail sizes |
| Target sizes | thumbnail ≤ 1.5 KiB, standard ≤ 6 KiB, HQ ≤ 12 KiB | 1.5 KiB = one 9-chunk transfer at MF (~4 s); 6 KiB = 35 chunks (~11 s); 12 KiB = 69 chunks (~21 s) |
| EXIF/GPS | strip by default; offer "attach location" as separate position message | Privacy + bytes |
| Re-encode server-side? | No, never; the client chooses size from the airtime budget | The bridge is transport, not an image service |

### 7.4 Recommended voice pipeline

| Profile class | Default codec | Cap | Rationale |
| --- | --- | --- | --- |
| `SHORT_TURBO`, `SHORT_FAST` | Opus mono 8 kbps, 16 kHz, OGG | 15 s (~15 KiB, ≤26 s airtime) | Opus is universally decodable and cheap here |
| `MEDIUM_FAST` | Opus mono 7 kbps | 10 s (~8.75 KiB, ~30 s airtime) | still acceptable for a "long thought" |
| `LONG_FAST` | Codec2 700C mono | 8 s (~700 B, ~11 s airtime) | Opus would be 80 s+, unusable |
| `LONG_SLOW` | Codec2 700C mono | 4 s (~350 B, ~51 s airtime — warn heavily) or text | Opus is minutes; voice is a last resort |
| Any 1 % region | halve the caps if the transfer would exceed 25 % of the hourly budget | | |

Playback compatibility note: not all Android devices decode Codec2 natively. If Codec2 voice
notes are used, the app must bundle a decoder (e.g. via the FFI/UniFFI layer or a native
library) or restrict Codec2 to firmware-to-firmware/desktop cases. This is a real product
constraint for the Android client (M2).

### 7.5 Airtime meter and rate limiting

The bridge and firmware should both expose `airtime_used_pct` and enforce a token bucket
(`crates/openmesh-core/src/routing.rs` has `AirtimeBudget`). The UI should show a simple meter:
green (<25 % of allowance), amber (25–75 %), red (>75 %, send requires confirmation). In 1 %
regions, a red meter is normal during image sends; the messaging must explain that instead of
silently failing.

---

## 8. Open questions and risks

1. **Regulatory accuracy.** The region table is a model, not legal advice. Sub-band duty cycles
   (EU 868.0–868.6 vs 869.4–869.65 at 10 % vs 1 %), channel dwell time (US 915), and LBT/AFA
   requirements differ. Before shipping caps per region, have the region table reviewed; until
   then, keep the conservative 1 % default and document it.
2. **No carrier sense.** LoRa has no CSMA; two nodes transmitting simultaneously collide. The
   capacity model assumes no hidden terminals. Real dense meshes will be worse; the simulator
   should model collisions before we publish capacity claims (M1).
3. **Priority inversion.** A large image transfer occupies the channel in 8-chunk windows; ACKs
   and announces interleave, but a voice note or text can wait seconds. The `PRIORITY` flag and
   a preemptive window scheduler are needed for good UX; the current spec only says relays
   SHOULD prioritise. Prototype in the simulator (M1).
4. **Codec2 playback on phones.** See §7.4. Either bundle a decoder or accept that Codec2 voice
   notes are desktop/embedded-only.
5. **Media store-and-forward.** As computed, media envelopes are large. Decide per-board flash
   budgets in firmware (M1), and make the feature explicit in the UI.
6. **Airtime estimates are estimates.** They assume a clean channel. Add a measured factor
   (e.g. "×1.3 for typical loss") from field data before promising wall-clock times.
7. **MTU headroom.** `MTU = 240` leaves 15 bytes below the 255-byte LoRa payload limit. If
   OMP/2 needs larger frames (links), revisit the limit; today, 240 is right for compatibility
   across SX1262/LR1110 and leaves room for future radio framing.

---

## 9. Reproduction

The calculations in this note were produced with two throwaway scripts (kept out of the
repository because they depend on paths outside it):

* a Python implementation of the Semtech formula with the profile table, generating §2–§5;
* a Rust program depending on `openmesh-core` that serialises `AppMessage`,
  `ResourceManifest`, `Ack`, `Announce` and `GroupMessage` with `ciborium` to get exact frame
  sizes, then feeds those sizes into the Python model.

To reproduce without leaving the repository:

```bash
cargo test -p openmesh-core            # includes the airtime plausibility test
cargo run -p openmesh-sim             # scenarios report airtime statistics
cargo run -p openmesh-cli -- airtime LONG_FAST 240   # per-frame airtime (see cli --help)
```

If the numbers in this note ever disagree with `PROTOCOL.md` Appendix A, the spec is
normative. Report the discrepancy with the measured frame size and the formula inputs rather
than editing the spec, as described in the repository conventions
([ARCHITECTURE.md](../architecture/ARCHITECTURE.md) §12).

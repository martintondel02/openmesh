# ADR 0002 — Managed flooding, not routing tables

* **Status:** accepted (v0.1.0); directed routing deferred to OMP/2
* **Date:** 2026-11-25
* **Decision record:** summary row 2 in
  [ARCHITECTURE.md §4](../architecture/ARCHITECTURE.md#4-key-decisions)
* **Related:** [ADR 0001](0001-own-protocol-vs-reticulum.md),
  [PROTOCOL.md §11](../architecture/PROTOCOL.md), [ROADMAP](../ROADMAP.md) M3,
  [Meshtastic/MeshCore comparison](../research/meshtastic-vs-meshcore-vs-openmesh.md)

## 1. Context

A mesh needs a way to get a frame from a sender to a receiver that may be several hops away.
The two families are:

* **Flooding:** every node rebroadcasts every frame once, subject to a hop limit and duplicate
  suppression. No topology knowledge, no convergence time, robust to mobility and link loss.
  Cost: O(nodes × frames) transmissions; worse in dense networks.
* **Routing:** nodes learn paths (proactively or on demand) and forward along them. Cost:
  control traffic, convergence time, state, and route breakage under mobility; better scaling.
* **Hybrid:** flood small control/discovery traffic and route data (Reticulum, MeshCore direct
  mode, Meshtastic directed delivery as an optimisation).

The LoRa medium makes this decision unusually consequential:

* a single 240-byte frame costs 97 ms (`SHORT_TURBO`) to 13.5 s (`LONG_SLOW`) of airtime;
* a 1 % duty cycle gives a node 36 s per hour total;
* there is no carrier sense or MAC layer, so every extra transmission also risks collisions;
* our target deployments are small (a valley, a festival, a hiking group, a few dozen repeaters),
  not city-scale.

Prior art:

* **Meshtastic** has used managed flooding at scale for years, with SNR-weighted random
  rebroadcast delays and an *optional* directed next-hop once a route is known, falling back to
  flooding. It is the strongest evidence that flooding works on single-channel LoRa.
* **MeshCore** supports direct routing with returned paths and uses flooding for discovery. Its
  1-byte node hashes make direct paths cheap; its frames are small.
* **Reticulum** uses announce-driven path discovery with transport nodes and 7-day path expiry;
  the machinery is impressive but chatty for a 240-byte, 1 %-duty medium.

We needed a v1 answer that is correct, testable, and cheap on MCUs, and a story for scaling later.

## 2. Decision

**OMP/1 uses managed flooding for all frames.** Every node that hears a frame and has TTL left
rebroadcasts it once, subject to `(src, seq)` deduplication (`DEDUP_TTL_MS = 1 800 000`,
≥ 2 048 entries). Default TTL is 5 for client frames, 8 for announces. Relays SHOULD prioritise
`PRIORITY` frames and enforce an airtime budget (token bucket, ≤ 1 % in EU868/AS923).

**No routing tables, no path discovery, no link layer in OMP/1.** Directed routing is reserved for
OMP/2 using the `RouteRequest`/`RouteResponse` frame types (already reserved in the spec) and is
gated on field evidence that flooding is the bottleneck.

### 2.1 Concrete rules fixed by this decision

* Relaying is unconditional: every node is a potential repeater; there is no "router" role in
  the frame format. Infrastructure nodes differ by behaviour (store, announce cadence), not by a
  forwarding protocol.
* Deduplication is by `(src, seq)` and is local; there is no global coordination.
* TTL is 4 bits, decremented by relays; the header is authenticated except for the TTL nibble
  (PROTOCOL §5), so ciphertext survives any number of hops.
* Announces set `hops` and are relayed with TTL 8; duplicate suppression is the same mechanism.
* Store-and-forward (PROTOCOL §10) is a *relay behaviour*, not a route: a relay stores envelopes
  for addressees it cannot currently hear and offers them when the addressee is seen again.
* The airtime budget is the safety valve: when the channel is busy, relays must drop, not queue
  forever.

### 2.2 Deliberately out of scope in v1

* Path discovery/transport-node tables.
* Next-hop pinning or source routes.
* Per-link sequence spaces, channels or reliability (that is Reticulum's link layer).
* Any assumption that topology is stable.

## 3. Consequences

### 3.1 Positive

* **Zero convergence time.** A message goes out immediately; there is no "wait for routes".
  This matters for store-and-forward repeaters that may only hear a node briefly.
* **Trivial firmware.** A relay is: decode header, check dedup, decrement TTL, re-emit,
  optionally store. No path state, no timers, no route expiry; fits on nRF52840 with room for
  the radio HAL and store queue.
* **Robust to mobility and topology change.** Nodes can move, repeaters can come and go; the
  network keeps working as long as connectivity exists.
* **Deterministic simulator.** Flooding is easy to model; the simulator can run thousands of
  nodes and assert delivery rates and airtime (see `crates/openmesh-sim`).
* **Store-and-forward composes cleanly.** Relays store ciphertext; they never need to know a
  path. When the addressee appears, the offer/want handshake works over the same flood.

### 3.2 Negative

* **O(N) transmissions per frame.** In a dense cluster, every node retransmits every message, so
  aggregate airtime grows linearly with node count. Capacity estimates (see
  [lora-capacity-and-media.md](../research/lora-capacity-and-media.md) §6) show this is fine for
  tens of nodes but saturates beyond ~50–100 in one region.
* **Collision risk.** LoRa has no carrier sense; simultaneous relays collide. We mitigate with
  TTL limits, dedup, random jitter in firmware, and priority for ACKs/announces, but the
  simulator and field data are needed to tune. (Meshtastic's SNR-weighted delay is a proven
  improvement we should adopt — see follow-ups.)
* **No delivery guarantees beyond ACK retries.** A message can be lost in a collision storm;
  senders retry up to `MAX_MSG_RETRIES = 3`.
* **Broadcast cost is paid by uninterested nodes.** Nodes cannot opt out of relaying per
  destination; they can only apply airtime/rate limits.
* **Large resources occupy the channel.** Flooding a 29-chunk image means every relay transmits
  29 frames; the ARQ windows are paced but the load is multiplicative.

### 3.3 Mitigations and tuning knobs

| Symptom | Knob | Where |
| --- | --- | --- |
| Dense cluster congestion | TTL tuning, dedup, priority queueing, airtime token bucket | PROTOCOL §11, firmware |
| Late relays causing collisions | SNR-weighted random rebroadcast delay (planned) | firmware/sim, M1 |
| Images hogging the channel | size caps, pre-send estimate, thumbnails, `PRIORITY` for voice/ACKs | ADR 0007, clients |
| Scaling beyond ~100 nodes | OMP/2 directed routing option | OMP/2 (M3) |
| Store growth | TTL, capacity, per-board flash budgets | PROTOCOL §10, firmware |

## 4. Alternatives considered

| Alternative | Why rejected for v1 |
| --- | --- |
| AODV/OLSR-style routing tables | Control overhead and convergence time dwarf the data on a 1 %-duty LoRa channel; state does not fit comfortably on nRF52840; breaks under mobility. |
| Reticulum-style announce/path discovery + transport nodes | Works, but chatty (path request 51 B, announces, path expiry 7 days) and assumes link/link-table machinery we do not have; better as an M4 gateway feature. |
| MeshCore-style direct routing as the default | Requires returned paths and 1-byte hash discipline; direct mode needs a working path before the first message; a flood fallback is still needed. Good for OMP/2 as an optimisation, not as v1 default. |
| Meshtastic-style flooding + next-hop optimisation | This is essentially "flooding with a cache"; the next-hop cache is a routing feature that needs path validation. Defer to OMP/2 after measuring. |
| **Managed flooding only (chosen)** | Simplest correct scheme at our target scale; proven on LoRa; smallest firmware; best fit for store-and-forward. |

## 5. Follow-ups

1. **M1:** add SNR-weighted relay delay to the firmware/simulator and measure dense-cluster
   delivery and airtime. This is the single highest-value improvement and needs no wire change.
2. **M1:** add asserted simulator scenarios at 10/25/50/100 nodes; publish delivery rate, airtime
   and collision counts (no invented benchmarks — measure in the simulator).
3. **M2/M3:** specify OMP/2 directed routing using the reserved `RouteRequest`/`RouteResponse`
   frame types; model it in the simulator before implementing on hardware.
4. **M3:** decide the gate for switching to directed routing ("flooding fails above X nodes /
   Y% loss in field data").

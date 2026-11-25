# Nodes, roles and telemetry

The Nodes screen is the mesh's dashboard: who is reachable, how they are connected, how strong
the signal is, how much battery they have left and what job each device does.

## What it is

Each device is a **node** with:

* a **node id** (short public identifier) and a display **name**;
* a **class** — what kind of device it is: `client`, `repeater`, `gateway`, `room_server` or
  `tracker`;
* **capabilities** — what it can do, advertised as flags: relay, store, GPS, Wi-Fi, Ethernet, BLE,
  USB, screen, audio out, solar;
* **telemetry** — optional battery voltage/percentage, uptime, airtime used, frame counters,
  temperature and solar input;
* **link quality** — last-seen time, hop count, RSSI (signal strength), SNR (signal-to-noise ratio).

## Why it matters

A mesh is only as good as its repeaters. The Nodes screen tells you whether a message failed
because of distance (high hops, weak RSSI), because a repeater is down (nodes disappear), or
because a device is low on battery (telemetry). It also tells you what a node can do for you: a
`store` node can hold mail, a `gateway` can bridge to IP, a `tracker` mostly beacons.

## Quick start

1. Open **Nodes**. Each row shows name, node id, class, hops and last seen.
2. Tap a node for details: public key/fingerprint, capabilities, position, battery, signal history
   and a "message" action if it is a contact.
3. Sort by last seen to find silent nodes; sort by hops to see the network's edge.
4. Use the filter chips (clients/repeaters/gateways/trackers/all) to focus.
5. Watch the **repeater** rows: they should be online all the time and have a stable hop count.
   A repeater flapping on/off usually means power or antenna problems.

## Advanced notes

### Classes vs roles

* **Class** is the advertised device type (`client`, `repeater`, `gateway`, `room_server`,
  `tracker`). It is descriptive, used by the UI and by other nodes for routing decisions.
* **Role** is what the local engine actually does: a client forwards traffic but never stores for
  others; a relay floods frames and may store envelopes (default capacity 64, TTL 72 h, flood
  budget 120 frames/min). A gateway additionally bridges transports.

A box can be a `client` class with the relay role disabled if you want a leaf node. For a public
repeater, use class `repeater` and the relay role.

### Capability flags

| Flag | Meaning |
| --- | --- |
| `relay` | forwards frames |
| `store` | can hold store-and-forward envelopes |
| `gps` | has a position source |
| `wifi` / `ethernet` | has an IP link (gateway candidates) |
| `ble` / `usb` | companion-radio or cable link |
| `screen` | local display |
| `audio_out` | can play voice notes |
| `solar` | solar/battery powered |

### Announce cadence and hops

* Clients announce every 30 minutes; repeaters every 60 minutes. Announces have TTL 8 and are
  never stored for store-and-forward.
* `hops` = announce TTL (8) minus the received TTL, so 0 means a direct neighbour. Hop count is
  a distance estimate, not a route: it changes as the mesh re-forms.
* Nodes further than the TTL never appear, even if they are physically reachable over a different
  path. Add repeaters or raise TTL (advanced) if the mesh is deeper than five hops.

### Reading signal and telemetry

* **RSSI** is in dBm; closer to 0 is stronger (`-60` is good, `-120` is marginal). **SNR** in dB;
  LoRa can decode below the noise floor, so negative SNR is normal at long range.
* Telemetry is optional and rate-limited. A tracker may report battery/position every 60 minutes;
  a USB-powered client may report nothing.
* `battery_pct` is an estimate; `battery_mv` is the raw voltage. Treat sudden percentage drops as
  suspect until confirmed by voltage.
* Airtime counters reset when the node reboots; compare over longer windows.

## Troubleshooting

| Symptom | Check | Fix |
| --- | --- | --- |
| A node vanished | Last seen, battery, power, whether it moved | Power-cycle; check antenna/cable; move closer. |
| Hop count changes constantly | Mesh is re-forming or links are marginal | Add a repeater with clear line of sight; fix antennas. |
| RSSI good but messages fail | Interference, duty cycle, or profile mismatch upstream | Check SNR and airtime; match profiles across nodes. |
| Repeater shows no clients | Profile/region mismatch, or nobody in range | Verify settings; check antenna placement. |
| Battery reads 100% but dies | Percentage estimate is wrong; solar input intermittent | Use voltage/telemetry history; check solar/charging. |
| Telemetry missing for a node | Node does not have that sensor or has telemetry disabled | Expected; not all nodes report the same fields. |

## Related topics

[Radios, regions and airtime](radio-and-region.md) · [Run a backbone](backbone.md) ·
[Map and location sharing](map-and-position.md) · [Troubleshooting](troubleshooting.md)

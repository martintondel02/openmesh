# Map and location sharing

OpenMesh can show nodes and contacts on a map and let you share a position deliberately. Location
is the most sensitive data a mesh handles, so the defaults are private and sharing is always an
explicit action.

## What it is

* **Position** — latitude/longitude (stored with 7 decimal places), optional altitude, accuracy in
  metres, speed, heading and the time of the fix.
* **Node positions** — if a node has GPS and shares positions, its last known position appears on
  the Map screen with a timestamp and accuracy radius.
* **Share once** — send your current position to a contact or channel as a message. It is a
  message like any other, encrypted end-to-end.
* **Live sharing** — optional periodic updates (for a group ride or a field team), implemented as
  repeated position messages with an interval you control.
* **Offline maps** — map tiles are cached for areas you have viewed; without internet the map may
  be blank until you load the region in advance.

## Why it matters

A radio network can find you when satellites and cell towers cannot — useful in the backcountry,
but dangerous if location leaks by default. OpenMesh therefore does not broadcast your position
unless you (or a node you configured) explicitly shares it. Remember that a position message,
once sent, cannot be un-sent: everyone who received it keeps it.

## Quick start

1. Open **Map**. Your own node appears once a position source is available (GPS on a phone, a
   tracker node, or a manually dropped pin).
2. To share, open a chat → **+ → Share position**, or use the location button in the composer.
   Choose a contact (private) or a channel (everyone in it).
3. The recipient sees a pin with accuracy and age; tapping it opens the map.
4. To stop live sharing, return to the sharing indicator in the chat and press **Stop**.
5. For privacy, open **Settings → Privacy** and confirm "Keep my location private" is on (the
   beginner preset enables it).

## Advanced notes

* **Precision** — positions are stored as `lat_e7`/`lon_e7` (1e-7 degrees, ~1 cm resolution). The
  app may round or fuzz display; the underlying message keeps full precision. If you need coarser
  privacy, share a dropped pin instead of a GPS fix.
* **Sharing targets** — a position sent to a contact is sealed to that identity; a position sent
  to a channel is readable by everyone with the channel key. There is no per-message recipient
  list inside a channel.
* **Tracker nodes** — a `tracker` class node can beacon positions on a schedule (default 60 min
  announce, battery-safe). Its position may be shared with a channel configured on the node; treat
  tracker output as public to that channel.
* **Metadata** — the message includes a timestamp; recipients can see when the fix was taken, not
  just when it arrived. Do not assume "share once" means "only visible while online".
* **Map sources** — the web client uses a standard raster map with local caching. If you are fully
  offline, pre-load your area or use the coordinate list on the Nodes screen instead.
* **API/agents** — `openmesh_share_position` can send a position once (`to` or `channel`). Agents
  must be explicitly allowed to call write tools; every call is attributed in the activity log.

## Troubleshooting

| Symptom | Check | Fix |
| --- | --- | --- |
| No position on the map | GPS permission denied, no fix yet, or location sharing off | Enable location permission; wait for a GPS fix outdoors; check Privacy settings. |
| Position is hours old | The node has not sent an update or is offline | Check the node's last-seen time; live sharing may have stopped. |
| Map is blank offline | Tiles not cached | Pan around the area while online once, or use the node's coordinates. |
| Your position shared by mistake | Check the chat for a sent position; recall is impossible | Tell the recipients; stop live sharing; consider a new channel key if it was a channel. |
| Contact sees an imprecise pin | GPS accuracy/urban canyon or coarse pin | Wait for a better fix; check the accuracy radius shown. |

## Related topics

[Nodes, roles and telemetry](nodes-telemetry.md) · [Security and privacy](security.md) ·
[Messages, delivery and offline mail](messaging.md)

### Choosing what to share

Ask three questions before sending a position:

1. **Who needs it?** A contact gets an encrypted point-to-point message; a channel gets it to
   everyone holding that channel key. If the answer is "only the rescue team", use a contact.
2. **How precise?** A GPS fix is precise to metres; a dropped pin is a deliberate approximation.
   Sharing a pin near your home is not the same as sharing your exact fix.
3. **For how long?** "Share once" is a single message. Live sharing repeats until you stop it, and
   each repeat is another message that recipients keep. Prefer a short window (e.g. the duration
   of a ride) over "always on".

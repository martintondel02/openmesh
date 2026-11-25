# Build a tiny mesh

A mesh is just radios that agree to relay for each other. This topic builds the smallest useful
one — two people, one private channel, and optionally one repeater on a hill — and shows how to
invite someone without exposing your identity.

## What it is

The word "mesh" sounds complicated; in OpenMesh it means:

* **Nodes** — each copy of the bridge (or radio) is a node with its own long-lived identity.
* **Repeaters** — nodes that forward other people's messages and can hold encrypted mail while
  someone is offline. They cannot read the mail.
* **Channels** — shared-key group chats. A channel is the easiest way to bring people together;
  anyone with the invite can read and write.
* **Contacts** — direct, verified conversations with one person, using their public key.

You can run all of this with zero infrastructure: two handhelds are already a network.

## Why it matters

Getting the mental model right explains most later behaviour. Messages are not "sent to a
server": they are broadcast, deduplicated, and relayed until they reach their destination or
expire. Distance therefore comes from **repeaters**, not from the app, and invites are the only
thing you need to exchange to create a private group.

## Quick start

### 1. Two nodes, one channel

1. On the first device, open the app → **Get started** → **Secure my node**. The wizard creates a
   private channel and shows an invite like
   `openmesh://channel?name=family&id=…&psk=…`.
2. Copy the invite (or show the QR) and give it to the second person over a channel you already
   trust — in person, Signal, email, a note.
3. On the second device, open **Contacts → Import invite** (or scan the QR) and paste it. Both
   devices now show the same channel.
4. Send a message in the channel and watch it arrive.

If you are testing alone, start two bridges with different data directories and different API
ports:

```sh
openmesh-bridge --demo --data-dir /tmp/mesh-a --listen 127.0.0.1:8788
openmesh-bridge --demo --data-dir /tmp/mesh-b --listen 127.0.0.1:8789
```

Demo mode gives each bridge a virtual network, so this is a UI test rather than a real radio test
— useful for learning the flow, not for measuring range.

### 2. Add a repeater (optional, real radios only)

A repeater is a node with no screen, usually solar-powered, placed somewhere high. Flash a
supported board with repeater firmware (class `repeater`), give it the same region/profile as the
other nodes, and it will relay automatically. In the app it appears on the Nodes screen with the
**repeater** role and a store capability if it has storage configured.

Place it so it can hear at least two of your nodes; a repeater that hears only one adds nothing.

### 3. Test the link

* Watch **Nodes** — a healthy mesh shows your partner and the repeater with hop counts.
* Send a message and check the delivery ticks.
* Ask the other person to power off, send another message, and watch it sit at one tick; when
  their node returns (or a repeater offers it), it should arrive. That is store-and-forward.

## Advanced notes

* **Hop budget** — client frames default to a TTL (hop limit) of 5; announces use 8. Every relay
  decrements it. Long chains need enough TTL and enough well-placed repeaters.
* **Channel IDs and keys** — a channel's address is derived from its shared key, and messages are
  sealed with that key and signed by the sender. Anyone with the invite can impersonate another
  member's *name*, but cannot forge the signature of someone else's node identity. Treat channel
  membership as a shared secret: re-create the channel (and re-invite) if a member leaves and you
  need forward secrecy.
* **Repeater roles and caps** — repeaters advertise `relay`; store-capable nodes advertise `store`.
  A room server stores channel history for late joiners (separate role) — do not expect a plain
  repeater to backfill old channel messages.
* **Profile and region must match** — nodes that disagree on LoRa profile/region simply cannot
  hear each other. Pick one locally and stick to it; the Radio and region topic has the trade-offs.
* **Invite hygiene** — channel invites contain the channel key. Do not post them publicly unless
  you intend the channel to be public. Contact invites contain only public keys.

## Troubleshooting

| Symptom | Check | Fix |
| --- | --- | --- |
| You see nodes but messages do not arrive | Are both on the same channel? Do delivery ticks advance? | Re-share the invite; confirm one channel is selected on both. |
| Nobody appears on Nodes | Region/profile mismatch, no shared transport, radios out of range | Match region/profile; move closer; check the radio is detected. |
| One direction works only | Asymmetric links are rare but possible with marginal signal | Reposition antennas/nodes; add a repeater between you. |
| Messages expire before the person returns | Store-and-forward disabled or no store-capable relay in range | Enable store on a repeater (it must have storage), or send later. |
| Repeater shows zero hops for everyone | Firmware region/profile mismatch | Flash/config the repeater for the same region and profile. |

## Related topics

[Group chats (channels)](channels.md) · [Contacts and identity](contacts-identity.md) ·
[Radios, regions and airtime](radio-and-region.md) · [Nodes, roles and telemetry](nodes-telemetry.md) ·
[Messages, delivery and offline mail](messaging.md)

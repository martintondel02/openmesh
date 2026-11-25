# Group chats (channels)

Channels are OpenMesh's group conversations: one shared key, many people, messages signed so you
can tell who wrote them. They are the easiest way to bring a family, team or event online.

## What it is

* **Channel** — an address derived from a shared 32-byte key (PSK), plus a display name.
* **Invite** — an `openmesh://channel?name=…&id=…&psk=…` link (or QR). Anyone with it can join,
  read and write. Treat it like a password, not a public URL.
* **Membership** — there is no central member list on radio: your node accepts messages for every
  channel it has a key for. The app shows known senders from their signed messages and announces.
* **Signatures** — each channel message carries the sender's public key and an Ed25519 signature,
  so members can verify authorship even though the group key is shared. A member can copy another
  member's display name; the signature (and node id) is what actually identifies them.

## Why it matters

Direct messages need a verified contact and a public key. A channel needs only one invite and
works for a group immediately — ideal for a first mesh, an event, or a family. The trade-off is
that the shared key is the security boundary: whoever holds it can read the channel, and leaving
the group cannot un-share what they already have.

## Quick start

### Create a channel

1. Open **Chats → New channel** (or the setup wizard's "create private channel" step).
2. Name it (`family`, `camp-2026`, `team-alpha`). Optionally supply your own 32-byte PSK; otherwise
   the app generates one.
3. Copy the invite or show the QR. **The PSK is shown once in full** — share it only with people
   you want in the channel.
4. Send a message; it should appear to everyone who joined.

### Join a channel

1. **Contacts → Import invite**, paste `openmesh://channel?…`, or scan the QR.
2. Confirm the channel name and join. The app stores the key and starts accepting messages.
3. Say hello; existing members will see your signed message and can save you as a contact.

### Invite flow that respects privacy

* Share the invite over an existing trusted channel (in person, Signal, email) — not on a public
  forum.
* Prefer the QR for in-person invites: no copy/paste mistakes and no chat logs holding the key.
* If someone leaves and you need forward secrecy, create a **new** channel with a new key and
  re-invite the remaining people. Do not keep using the old channel.

## Advanced notes

* **Identity of a channel** — the 8-byte channel id is derived from the PSK, so the same key
  produces the same channel id. The display name is metadata and can differ between members; the
  id is what matches.
* **PSK handling** — the bridge never returns the PSK from `GET /api/channels`; it is shown only
  when created or when you reveal the invite locally. Back up your channel invites if you need to
  re-join after losing a device.
* **Key rotation** — no in-place rotation: create a new channel, re-invite members, announce the
  switch on the old channel, then stop using it. This is intentional; silent rekeying without
  out-of-band verification is worse.
* **Room servers** — a `room_server` role can store and serve channel history for late joiners.
  A plain repeater does not backfill channel history; it only relays and (optionally) stores
  envelopes for offline direct messages.
* **Duty cycle** — group traffic multiplies airtime because every member's node may relay. Keep
  channel use text-first on slow profiles and avoid simultaneous media sends.
* **Moderation** — there are no admins on radio. "Remove member" means re-create the channel with
  a new key. The app can hide a channel locally, but that is cosmetic.
* **API** — `GET /api/channels` lists joined channels without keys; `POST /api/channels` creates
  one and returns the invite once; `POST /api/channels/join` accepts an invite URL. Writing through
  an agent requires an MCP write tool and is recorded in the activity log.

## Troubleshooting

| Symptom | Check | Fix |
| --- | --- | --- |
| Messages in a channel do not appear for one person | Same channel id? Same profile/region? | Re-share the invite; verify the channel id matches. |
| "Invite invalid" | Truncated paste or wrong URL scheme | Copy the full `openmesh://channel?…` link; use the QR. |
| Everyone appears with the same name | Display names are self-chosen and unsigned metadata | Rely on node ids/signatures; save verified contacts. |
| New member sees no history | Plain repeater/room server does not backfill | Expected; a room server role or recent messages only. |
| Channel messages cost a lot of airtime | Group fan-out and media | Send text, smaller media, or use a faster profile locally. |

## Related topics

[Contacts and identity](contacts-identity.md) · [Build a tiny mesh](first-mesh.md) ·
[Security and privacy](security.md) · [Messages, delivery and offline mail](messaging.md)

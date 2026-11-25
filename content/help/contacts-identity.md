# Contacts and identity

Your identity is the most important thing in OpenMesh: it is how people verify you, and no
company can recover it for you. This topic explains node ids, public keys, invites and what a
contact actually proves.

## What it is

* **Identity key** — a long-lived secret seed stored in your data directory (`identity.key`). All
  your node ids, encryption keys and signatures derive from it.
* **Node id** — a short 8-byte public label (hex, e.g. `6b1f4a02c9de8810`) derived from your public
  key. It identifies you on the wire and is what peers see.
* **Public identity** — the full public key bundle (72 bytes, base64 in JSON/QR) used by others to
  encrypt to you and verify your signatures.
* **Contact invite** — `openmesh://contact?node=…&name=…&identity=…`; it carries only public data,
  safe to share.
* **Contact** — someone you saved with their public identity. Direct messages are sealed to that
  key, so only they can read them.

## Why it matters

Because there is no account server, **possession of the identity key is the account**. Lose it
and you are a new person; leak it and someone can impersonate you and read your direct messages.
Contacts are how the network replaces "verified by a company" with "verified by you": comparing a
short fingerprint or scanning a QR proves you have the right public key.

## Quick start

### Share your identity

1. Open **Contacts → Your card** (or Settings → Identity). You will see your name, node id and a
   QR code.
2. For a private conversation, show the contact QR in person, or copy the contact invite and send
   it over a channel you trust. Contact invites contain no secrets.
3. Never send `identity.key` or a backup seed to anyone, ever.

### Add someone

1. Scan their contact QR or paste their `openmesh://contact?…` invite.
2. Compare the **fingerprint** (a short hash of their public key) out of band if you need strong
   verification — read it aloud, compare on screen, or check a signed message.
3. Save. Direct messages to them are now end-to-end encrypted with their key; the app shows the
   verification state in their contact card.

### Back up your identity

1. Settings → Identity → **Back up identity**. Export the encrypted backup (choose a strong
   passphrase) or copy `identity.key` from your data directory to offline storage.
2. Store it separately from the device, ideally encrypted, and test the restore once.
3. If you migrate devices, restore the key **before** starting the bridge so peers see the same
   node id.

## Advanced notes

* **Derivation** — the node id is the hash of the public key (first 8 bytes); the public identity
  is the 72-byte bundle (Ed25519 signing key + X25519 encryption key, with format bytes). The seed
  never leaves the bridge; JSON APIs expose only the public identity.
* **Verification states** — the UI distinguishes "saved", "verified" (fingerprint compared) and
  "TOFU" (first-seen, unverified). A verified badge means "the key has not changed since you
  verified it"; it does not prove the person's legal name.
* **Identity reset** — a reset is intentionally not an API call: restarting with a fresh
  `--data-dir` (or removing `identity.key` while the bridge is stopped) creates a new identity.
  All contacts must re-add you, and your old direct messages become unreadable.
* **Multiple devices** — each bridge has its own identity. There is no multi-device sync for
  direct messages in this release; channels are the practical way to share across devices until
  multi-device lands. Do not copy `identity.key` onto two simultaneously running bridges — frame
  dedup and sequence numbers assume one active node per identity.
* **Revocation** — there is no broadcast revocation list. If a key leaks, create a new identity,
  tell contacts out of band, and rotate any channel invites you control. A leaked identity can
  still read messages sent to the old public key, so treat those as compromised.
* **Contacts and channels** — channel messages are signed by the sender's identity, so even
  without a saved contact you can see a consistent node id. Saving the contact makes replies
  private and enables direct delivery.

## Troubleshooting

| Symptom | Check | Fix |
| --- | --- | --- |
| Your contacts see a different node id | Restored an old/new identity, or a second bridge is running with the same key | Stop duplicate bridges; restore the intended `identity.key` and restart. |
| "Contact invite invalid" | Truncated URL or wrong scheme | Copy the full `openmesh://contact?…`; scan the QR in person. |
| Cannot send to a contact | They re-installed and their key changed | Verify the new fingerprint and update the contact. |
| Lost your identity | No account recovery exists | Use your backup; otherwise create a new identity and re-invite. |
| Verification badge disappeared | The contact's public key changed (possible impersonation or re-install) | Verify out of band before re-saving. |

## Related topics

[Security and privacy](security.md) · [Group chats (channels)](channels.md) ·
[Map and location sharing](map-and-position.md) · [Troubleshooting](troubleshooting.md)

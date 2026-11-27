# Messages, delivery and offline mail

This topic explains what actually happens between pressing send and seeing the double tick — and
how a message can reach someone who was offline when you sent it.

## What it is

OpenMesh has two kinds of conversations:

* **Direct messages** — sealed to one person's public key. Only their device can decrypt them.
* **Channels** — group conversations sealed with a shared channel key and signed by the sender.

Messages are **frames** broadcast over radio. Every frame carries a tiny header (source, sequence,
TTL, flags) and an encrypted body. Any node in range forwards the frame until it reaches its
destination, the hop limit runs out, or it is dropped.

**Delivery states** shown in the app:

| Tick | Meaning |
| --- | --- |
| clock/queued | Accepted by your bridge; waiting for the radio or a route. |
| single tick | Transmitted; no delivery acknowledgement yet. |
| double tick | The recipient's device acknowledged delivery (decrypted and accepted). |
| warning | No acknowledgement after several attempts; the message may still be stored somewhere. |

## Why it matters

Radio is not a wire. Frames get lost, two people transmit at once, or the recipient is out of
range. OpenMesh handles that with acknowledgements, retries, deduplication and — if a relay has
storage — **store-and-forward**: the encrypted envelope is held for up to 72 hours and offered
when the person reappears. Knowing which state you are looking at tells you whether to wait, retry
or move to a better position.

## Quick start

1. Open **Chats**, choose a conversation, type a message and send.
2. Watch the status: the bubble shows an icon plus a label — **Sending…**, **Sent** (handed to
   your radio), **Delivered to <name>** (their device acknowledged it), **Read by <name>** (they
   opened the conversation; only if read receipts are on), **Queued** (radio not ready yet) or
   **Not delivered**. Tap the message (or its info action) for a details dialog with the same
   explanation and a Retry button when failed.
3. If the person is out of range, send anyway. If a store-capable repeater is in range, the app
   will keep the message queued and deliver it when they return. You can see queued mail under
   the node's details or in the message's status.
4. Long-press (or use the message menu) for **Retry** if a delivery looks stuck, **Copy** to
   copy text, and **Reply** to quote.
5. If you open the app while messages arrived, read receipts are sent automatically as the
   conversation is marked read (you can disable read receipts in Settings → Privacy).

## Advanced notes

* **Timers (OMP/1 defaults)** — an unacknowledged direct message is retried after
  `ACK_TIMEOUT_MS` = 8 s, up to `MAX_MSG_RETRIES` = 3 attempts. After that the message is marked
  failed but may still be sitting in a relay's store. ACKs themselves are never retried.
* **Hop limit** — client frames are originated with TTL 5 (announces use 8). Each relay
  decrements TTL and drops at zero. A message crossing more than five links needs a higher TTL
  (advanced setting) or fewer hops.
* **Deduplication** — receivers keep a `(src, seq)` dedup cache for 30 minutes (≥ 2048 entries),
  so the same frame arriving over three paths is shown once. Relays never rewrite frames.
* **Ordering** — OMP is best-effort, not ordered. The UI sorts by receive time; a retried message
  can appear "late" among newer ones. Message ids are stable for replies and reactions.
* **Read receipts** — optional and privacy-sensitive: they tell the sender when *you* opened the
  conversation, not when you were online. Keep them off in high-threat settings.
* **Long text** — messages over one frame become a **resource** (chunked transfer with selective
  repeat), so a 2,000-character note may take several seconds and show progress.
* **Store-and-forward** — requires a relay with the `store` capability and storage budget. It
  stores ciphertext only; the relay cannot read your message and cannot prove it delivered. Treat
  "stored" as "likely to arrive later", not a guarantee: envelopes expire after 72 hours by
  default, and a full store evicts the oldest mail first.
* **Group delivery** — channel messages are broadcast to all members, so there is no
  per-recipient receipt: the app labels them **Sent to channel** and the details dialog says so
  explicitly. Missing some peers at send time is normal on radio; there is no "delivered to
  everyone" state.
* **Direct delivery** — a direct message is acknowledged by the recipient's device (delivered) and
  optionally when they open the conversation (read). "Delivered" never means a human read it.
* **Why a tick can stall** — under duty-cycle limits a message may wait behind a media transfer;
  the status stays **Sent** until the ACK arrives, and the details dialog explains it is waiting
  rather than lost.

## Troubleshooting

| Symptom | Check | Fix |
| --- | --- | --- |
| Single tick forever | Is the recipient online? Are you heard by any relay? | Move to a better spot, add a repeater, retry. |
| Failed after retries | TTL too small, profile mismatch, or recipient offline with no store relay | Raise TTL (advanced), match profiles, or wait for the person to come back. |
| Message appears twice | Sender retried and both copies arrived | Harmless; dedup covers most cases, and retries are numbered. |
| Messages vanish after days | Retention/expiry in the store or app | Check Settings → Storage; back up if you need history. |
| Read receipt never arrives | Recipient has read receipts off, or ACK was lost | Not an error; the message can still be delivered. |
| Queue grows and nothing sends | Radio busy with a large transfer or duty-cycle limit | Wait for media to finish; check Settings → Radio airtime. |

## Related topics

[Group chats (channels)](channels.md) · [Nodes, roles and telemetry](nodes-telemetry.md) ·
[Images and voice notes](media.md) · [Troubleshooting](troubleshooting.md)

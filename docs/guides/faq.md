# FAQ

Long-form answers to the questions that come up most often. For one-line definitions see the
[glossary](../glossary.md); for the in-app version see [help FAQ](../../content/help/faq.md).

## Product and scope

### Is OpenMesh a replacement for Signal or WhatsApp?

No. OpenMesh is a radio mesh and local-first network for places where infrastructure is missing,
expensive or untrusted. It carries text, positions, small images and voice notes over links that
move a few hundred bytes per second. It has no phone-number identity, no group video, no cloud
backup and no account recovery. Use it *alongside* a normal messenger, not instead of one for
everything.

### Does it need the internet?

For a local radio mesh: no. Radios talk directly (or through repeaters) with no internet, no cell
tower and no server. A **backbone** (optional) needs a reachable TCP port or VPN so distant meshes
can connect; a **gateway** needs some IP path to that backbone.

### What hardware do I need?

For a software-only evaluation, nothing: `openmesh-bridge --demo` runs a virtual mesh. For real
coverage you need at least two LoRa-capable devices (handhelds/trackers/repeaters); see
[docs/hardware/boards.md](../hardware/boards.md). A Raspberry Pi or small VPS is enough for a
backbone.

### Is there a mobile app?

The v0.3.0 web client is a PWA and works in a mobile browser against a bridge on the same device
or network. The Android Compose app is a scaffold (M2 is the GA milestone); Tauri packaging exists
for desktop and needs system WebView deps. Check the [roadmap](../ROADMAP.md) before planning on a
store app.

### Is it free? Can I self-host?

Yes: AGPL-3.0, self-hosted by design, no accounts and no OpenMesh-hosted service. You can run the
bridge, a backbone and your own clients. See
[ADR 0008](../adr/0008-agpl-3-0-licensing.md).

### Why another mesh protocol instead of Meshtastic/MeshCore/Reticulum?

Existing systems do text well; OpenMesh targets images, voice and a first-class client, with a
240-byte-MTU-friendly design (managed flooding, no per-message handshake, store-and-forward
envelopes). The trade-offs and prior art are documented in
[ADR 0001](../adr/0001-own-protocol-vs-reticulum.md) and
[research notes](../research/). If plain text is all you need, Meshtastic may be the better choice.

## Identity, security and privacy

### What happens if I lose my phone or laptop?

You lose that node's identity and local history unless you backed up `identity.key` and the data
directory. There is no account server and no recovery. If you have the backup, restore it before
starting the bridge so your node id stays the same; if not, you become a new node and contacts
must re-add you. See [security help](../../content/help/security.md).

### Can the backbone/relay read my messages?

No. Message bodies, attachments and positions are end-to-end encrypted; relays forward ciphertext
and see metadata (node ids, sizes, timing, topology). A malicious relay can drop, delay or replay
traffic, and can read messages **from nodes that trust it as their own bridge**, which is why you
run your own bridge or trust the operator. Transport between mesh and backbone is plaintext OBB/1
in v0.3.0 unless wrapped in WireGuard/TLS.

### Is the radio traffic encrypted?

Payloads are encrypted (XChaCha20-Poly1305), and announces/channel messages are signed (Ed25519).
Radio headers and timing are public to anyone with a radio, as with any radio system. The
identity seed, API token and channel PSKs are the secrets that matter.

### What does "verified" mean for a contact?

You compared the contact's fingerprint/public key out of band (QR, read aloud, signed message).
It means the key has not changed silently since you verified; it does not prove a legal name.
Unverified contacts are trust-on-first-use (TOFU). See
[contacts and identity](../../content/help/contacts-identity.md).

### Can a channel member impersonate someone else?

They can copy a display name. Channel messages carry the sender's node id and an Ed25519
signature, so the actual sender is verifiable; the display name is metadata. Compare node ids for
decisions that matter. Removing a member requires creating a new channel with a new key — there
is no in-place rekey.

### Is there forward secrecy?

Not for static-DH direct messages in v0.3.0: a stolen identity seed can decrypt previously
captured direct messages. Channel security relies on the shared key. OMP/2 ratchets are on the
roadmap ([M3](../ROADMAP.md)); re-create channels after membership changes if you need a clean
break.

### What does the AI agent see? Can it leak my data?

The MCP server is a local process with the same powers as the API token: it can read messages and,
if not read-only, send messages and change settings. It never gets your identity seed or PSKs
(except one-time outputs). Use `OPENMESH_MCP_READ_ONLY=1` unless writes are needed, name the agent,
review the activity log, keep the API loopback-only, and never paste secrets into a prompt. See
[docs/agents/mcp.md](../agents/mcp.md).

## Mesh behaviour

### Why does a message show only one tick?

It was transmitted but no acknowledgement arrived. The recipient may be offline/out of range, the
TTL may have expired, the profile may differ, or there is no store-capable relay. OpenMesh retries
after 8 s up to 3 times, then marks it failed (it may still be stored somewhere). See
[messages and offline mail](../../content/help/messaging.md).

### How far can it reach?

Line of sight, typically a few kilometres at `MEDIUM_FAST` with a decent antenna, more with slower
profiles and elevated repeaters, much less in cities/valleys. Antenna height and placement beat
power. A repeater that hears at least two nodes is worth more than a bigger antenna on one node.

### Why is my image taking minutes?

Slow profiles and/or duty-cycle limits. A 5 KiB image takes ~18 s of airtime at `MEDIUM_FAST`,
~6 minutes at `LONG_SLOW`, plus relays and retries. The app shows an estimate before sending and
caps size per profile. Text is almost always cheaper and more reliable.

### Do all nodes need the same settings?

Same **region** and **profile**, yes — otherwise they cannot decode each other. Names, roles,
position sharing and store settings can differ. Repeaters must match the clients they serve.

### Why do nodes disappear from the list?

Announces are periodic (clients 30 min, repeaters 60 min) and TTL-limited (8 for announces). A
node that is off, out of range, past the hop limit or misconfigured will age out of the list.
This is normal; "last seen" tells you how stale it is.

### Can I use two bridges with the same identity (multi-device sync)?

Not safely in v0.3.0. Each bridge should have its own identity; copying `identity.key` to two
running bridges causes sequence/dedup conflicts. Use channels to share conversations across
devices until multi-device sync lands, and do not run two bridges on the same data dir.

## Agents and API

### What can an AI agent do?

Read tools: health, status, nodes, messages, channels, contacts, backbone status/peers/setup,
setup state, activity, help. Write tools: send messages, share position, announce, mark read,
retry, set config, quick setup, create/join channel, add contact, backbone config, rotate token,
kick peer (destructive tools need `confirm: true`). See the full table in
[docs/agents/mcp.md](../agents/mcp.md).

### Is it safe to let an agent write?

Use read-only mode unless needed, give the agent a scoped name, review Activity, keep the API
token local and rotate it to revoke. Destructive tools require explicit confirmation. An agent
cannot read your identity seed or PSKs; it can read messages and act as you on the bridge.

### Can I use the API without an agent?

Yes. `GET /api/openapi.json` describes every REST route; `openmesh-cli` wraps common operations;
`/api/ws` and `/api/events` give live updates. Use a bearer token from
`~/.config/openmesh/bridge-token` and keep the API on loopback. See
[the advanced cookbook](advanced-cookbook.md).

### Why did my agent get 401?

The bridge token was rotated, the token file moved, or the agent was configured without the token.
Update `OPENMESH_TOKEN`/`OPENMESH_TOKEN_FILE` in the client config and restart it.

## Troubleshooting and support

### Where are the logs?

stderr of the bridge (journald for services), `--log debug` for detail; see
[troubleshooting §6](troubleshooting.md#6-where-are-the-logs). Never post tokens or seeds.

### How do I report a bug?

Open a GitHub issue with version, OS, how you run it (demo/sim/radio/backbone), steps, expected vs
actual, health/status output and redacted logs. The
[troubleshooting guide](troubleshooting.md#9-bug-report-template) has a template.

### Does OpenMesh work without any internet at all?

Yes for radio meshes and local LAN. Backbone/gateway features are optional. Agents need a local
process and loopback API; they do not need internet unless the model itself does.

### Can I use it legally in my country?

Region presets follow common regulatory bands and conservative duty cycles, but you are
responsible for local law and hardware certification. Check your regulator and keep power/duty
cycle within limits. See [radio and region](../../content/help/radio-and-region.md).

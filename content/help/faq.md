# FAQ

Short, honest answers to the questions people ask after their first mesh. If a question needs more
than a paragraph, it links to the topic that explains it.

## What it is

OpenMesh is a local-first mesh networking stack: a bridge daemon, a web app, optional radios, and —
in v0.3.0 — a local MCP server so AI agents can help operate it. It is not a hosted messaging
service and it has no accounts.

## Why it matters

Most "mesh apps" are either demos or require a company's servers. OpenMesh is designed to keep
working when the internet does not, while still letting you connect distant meshes through a
self-hosted backbone and automate routine work with an agent.

## Quick start

New here? Read [Your first mesh in 10 minutes](getting-started.md) first. The shortest path is:

```sh
# install
curl -fsSL https://github.com/martintondel02/openmesh/releases/latest/download/install.sh | sh
# try it with no hardware
openmesh-bridge --demo
# open http://127.0.0.1:8787
```

Then press **Secure my node** and invite someone to your channel.

## Advanced notes

### Product and scope

**Is OpenMesh a replacement for WhatsApp/Signal?**
No. It is a radio mesh and local-first network for places and situations where infrastructure is
missing or untrusted. It can carry text, positions, small images and voice notes over very slow
links; it is not a high-bandwidth chat app.

**Does it need the internet?**
No for local radio meshes. A backbone (optional) needs a public TCP port or a VPN so distant
meshes can connect.

**Is there a mobile app?**
The v0.3.0 web client is a PWA and works in a phone browser; Android has a Compose scaffold, and
Tauri packaging exists for desktop. Native mobile polish is on the roadmap.

**Is it free/open source?**
Yes, AGPL-3.0. You can self-host, modify and audit it; the repository's ADR 0008 records the licensing
rationale.

**Can I run my own backbone?**
Yes — that is the point. See [Run a backbone](backbone.md). It needs a small VPS/Raspberry Pi and
one open TCP port (or a WireGuard/Tailscale network).

### Identity and security

**What happens if I lose my phone/laptop?**
You lose that node's identity and local history unless you backed up `identity.key` and the data
directory. There is no account recovery. See [Security and privacy](security.md).

**Can the backbone read my messages?**
No. Payloads are end-to-end encrypted; the backbone relays ciphertext and sees metadata. It can,
however, drop or delay traffic, so trust your operator (or run your own).

**Is the radio traffic encrypted?**
Message bodies and resources are encrypted. Radio headers and timing are public, as with any
radio. Backbone TCP links in v0.3.0 are plaintext; use a VPN/TLS wrapper on hostile networks.

**What does "verified contact" mean?**
You compared the contact's fingerprint/key out of band. It means the key has not silently changed;
it does not prove a legal identity. See [Contacts and identity](contacts-identity.md).

**Can a channel member impersonate another?**
They can copy a display name, but channel messages are signed by node identities, so the
signature/node id reveals the real sender. Compare node ids for important decisions.

**Is there forward secrecy?**
Not for static-DH direct messages in v0.3.0; OMP/2 ratchets are planned. Channels rely on the
shared key, so re-create the channel if a member leaves and you need a clean break.

**Can I use my own radio hardware?**
Supported LoRa boards and companion/Bluetooth radios are documented in the repository's hardware
docs. `--serial <port>` requires a build with the `real-serial` feature.

### Mesh behaviour

**Why does a message show one tick?**
It was transmitted but not acknowledged. The recipient may be offline, out of range, or no
store-capable relay is around. See [Messages, delivery and offline mail](messaging.md).

**How far can it reach?**
Line of sight, typically a few kilometres with a good antenna at `MEDIUM_FAST`, more with slower
profiles and elevated repeaters; much less in cities or valleys. Repeaters matter more than
power.

**Why is my image taking minutes?**
Slow radio profiles and duty-cycle limits. A 5 KiB image is ~18 s at `MEDIUM_FAST`, ~6 minutes at
`LONG_SLOW`, plus relays. See [Images and voice notes](media.md).

**Can I send files?**
Small attachments work through the same chunked resource transfer, but radio is not a file
transport. Prefer text and small images.

**Do all nodes need the same settings?**
Same region and profile, yes; otherwise they cannot hear each other. Other settings (name, role,
position) can differ. See [Radios, regions and airtime](radio-and-region.md).

### Agents and API

**What can an AI agent do with my node?**
Read status/messages/nodes and, if not read-only, send messages, share position, change config and
manage backbone settings. It never gets your identity seed or PSKs (except one-time outputs).
See [Use your AI agent](agents.md).

**Is it safe to let an agent write?**
Use read-only mode unless needed, name the agent, review Activity, and keep the API token local.
Destructive tools require confirmation.

**Can I use the API without an agent?**
Yes: `GET /api/openapi.json` documents every REST route; the CLI (`openmesh-cli`) wraps common
operations; the WebSocket API is documented in the repository.

**Why did my agent suddenly get 401?**
The bridge token was rotated or the token file moved. Update the agent's env/config and restart
it.

### Troubleshooting and support

**Where are the logs?**
The bridge logs to stderr (journald when run as a service). Run with `--log debug` for detail.
See [When something is broken](troubleshooting.md).

**Where do I report a bug?**
The repository's GitHub issues, with version, OS, bridge mode (demo/sim/radio), health/status output
and redacted logs.

## Troubleshooting

| Question | Answer |
| --- | --- |
| "Install failed: checksum mismatch" | Corrupt download or wrong mirror — re-download; never bypass verification. |
| "Port 8787 in use" | Another bridge is running; stop it or use `--listen 127.0.0.1:8788`. |
| "Browser blank" | Web bundle missing or `--serve-web` wrong; reinstall the web asset. |
| "Panel 401" | Paste `~/.config/openmesh/bridge-token` into Settings → Bridge. |
| "No radio detected" | Check cable/permissions/serial feature; demo mode has no physical radio. |

## Related topics

[Glossary](glossary.md) · [Security and privacy](security.md) · [Troubleshooting](troubleshooting.md) ·
[Your first mesh in 10 minutes](getting-started.md)

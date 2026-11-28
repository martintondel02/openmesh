# Security and privacy

OpenMesh's promise is simple: your messages are sealed on your device, and relays — including
OpenMesh backbones — only see ciphertext and metadata. This topic explains exactly what that means,
what can still go wrong, and the checklist for a setup you can trust.

## What it is

* **End-to-end encryption** — direct and channel message bodies are encrypted with
  XChaCha20-Poly1305. Direct messages derive a key from a static X25519 Diffie-Hellman secret;
  channel messages use the channel's 32-byte key. Relays never hold the keys.
* **Signatures** — announces and channel messages are signed with Ed25519. Others can verify who
  authored a message and that it was not modified.
* **Identity** — a 32-byte seed on your disk (`identity.key`, mode 0600) derives your Ed25519 and
  X25519 keys; your node id is `SHA-256("openmesh/v1/node" || ed25519_pub || x25519_pub)[0..8]`.
  There is no account recovery.
* **Local API** — the bridge listens on loopback only and requires a bearer token. It is the
  control plane: whoever has the token can read messages and change settings.
* **Transport** — radio frames are public to anyone listening, but payloads are encrypted; OBB/1
  backbone links are plaintext TCP in v0.3.0 and should be wrapped in a VPN/TLS on hostile networks.

## Why it matters

"Encrypted" is often used to mean "we promise not to look". OpenMesh is built so the relay
operator *cannot* read message bodies: they see frames, node ids, sizes and timing. That is a
different threat model, and it changes what you must protect: your identity seed, your API token,
your channel keys, and your location. Get those four right and the mesh is strong; leak them and
the encryption cannot help you.

## Quick start

The secure beginner path is one click: **Get started → Secure my node**. It applies token auth,
loopback API, a private channel, position sharing off, sane announce timing and no open backbone
mode. Then do this five-minute checklist:

1. **Back up your identity** (Settings → Identity) to encrypted offline storage. Test the restore.
2. **Protect the API token** — keep `~/.config/openmesh/bridge-token` mode 0600; never paste it into
   a website or commit it. Agents should read it from the file, not from shell history.
3. **Keep the API on loopback** — do not pass a non-loopback `--listen`; use an SSH tunnel or a TLS
   reverse proxy with authentication if you need remote panel access.
4. **Keep channel invites private** — the invite contains the channel key. Share it over an
   existing trusted channel or in person.
5. **Leave position sharing off** unless you deliberately need it; then share a pin or a short
   live window, not continuous tracking.

## Advanced notes

### What each party can see

| Party | Can see | Cannot see |
| --- | --- | --- |
| Random radio listener | frames, node ids, timing, sizes | message bodies, channel keys, identities |
| Repeater | same as a listener, plus forwarding metadata | message bodies, who is who beyond node ids |
| Backbone operator | peer node ids, IPs, RTT, frame counters, metadata | message bodies (OMP E2E), identity seeds, PSKs |
| Local API client (web/CLI/agent) | everything on your bridge, including history | other nodes' seeds; channel PSKs are not returned via GET |
| Someone with your device | your identity and history | — treat device compromise as total compromise |

### Channel caveats

* **Public Mesh has no privacy.** Its PSK is published with the app, so anyone can join, read and
  post. Signatures still verify which node wrote a message, but there is no confidentiality: do not
  post personal data, private locations or anything sensitive there. It is auto-created, pinned and
  cannot be left/deleted.
* The channel key is shared: any member can decrypt all channel traffic, and can create messages
  that appear to come from any **display name**. Signatures bind messages to node identities, so
  compare node ids for important decisions.
* Removing a member requires a new channel and key; old messages remain readable to them.
* Channel messages are sealed with a key derived from the PSK; the PSK itself is never transmitted.

### Backbone and transport

* OBB/1 uses a shared token proved by HMAC-SHA256 (never sent on the wire), with nonces and
  constant-time comparison. It prevents unauthorised peers and replayed handshakes.
* OBB/1 does not encrypt the TCP stream in v0.3.0. Metadata and sizes are visible to a network
  observer. Use WireGuard/Tailscale (recommended) or nginx `stream` TLS + `stunnel` on peers.
* Open mode (`auth: "open"`) disables peer authentication; it is only for trusted
  LANs/VPNs and shows a red warning.

### Agent and automation safety

* The MCP server is a local process with the same power as the API token: read tools expose your
  mesh; write tools can send messages and change config. Use `OPENMESH_MCP_READ_ONLY=1` unless the
  agent needs writes, and name agents with `OPENMESH_AGENT_NAME` for the activity log.
* Every write is logged (method, path, summary, source, ok/error) in a 256-entry ring; message
  bodies and secrets are never logged. Review Activity after agent sessions.
* Destructive tools require `"confirm": true`; rotate the API token to revoke all clients at once.

### Threat-model gaps (honest list)

* No forward secrecy for static-DH direct messages in v1: a stolen identity seed can decrypt
  captured direct messages. OMP/2 ratchets are planned.
* Radio metadata is inherently public: presence, timing and approximate traffic volume leak even
  with encrypted payloads.
* A compromised host (malware with your user account) can read the bridge store and token. Use OS
  disk encryption and a locked screen.
* No account recovery, no key transparency service, no remote attestation. Verification is manual.

## Troubleshooting

| Symptom | Check | Fix |
| --- | --- | --- |
| Panel/API reachable from another machine | `ss -ltnp` on 8787; `--listen` value | Bind loopback; add SSH tunnel/TLS proxy. |
| Token in shell history | `grep` your history file | Rotate the API token; use `--token-file` or env vars. |
| Unknown agent activity | Agents → Activity, `GET /api/activity` | Revoke/rotate token; remove MCP config; review changes. |
| Lost identity | Backup restore test failed or no backup | Create a new identity; re-add contacts and channels. |
| Suspected message interception | Were endpoints' keys verified? Was the channel invite shared publicly? | Verify fingerprints; create a new channel with a new key. |
| Backbone peers show unknown nodes | Token shared too widely or `--open` | Rotate OBB token; use allowlist/max-peers; disable open mode. |

## Related topics

[Contacts and identity](contacts-identity.md) · [Run a backbone](backbone.md) ·
[Use your AI agent](agents.md) · [Glossary](glossary.md)

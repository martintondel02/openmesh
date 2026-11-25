# Glossary

Long-form definitions of OpenMesh terms, including protocol-level details that the in-app
[help glossary](../content/help/glossary.md) keeps to one line. Read this when you want the exact
meaning, not just the gist.

---

## Identity and cryptography

**Identity seed** — a 32-byte secret generated on first run and stored in `<data-dir>/identity.key`
(mode 0600). Everything public about you — Ed25519 signing key, X25519 key-agreement key, node id —
is derived from it. There is no account server and no recovery: losing the seed loses the identity;
leaking it allows impersonation and decryption of direct messages sent to your old public key.
See [ADR 0003](adr/0003-static-ecdh-then-ratchet.md).

**Public identity** — the 72-byte public bundle shared in invites and announces:
8-byte node id + 32-byte Ed25519 verification key + 32-byte X25519 public key
(`PUBLIC_IDENTITY_LEN = 8 + 32 + 32`). It is safe to share; it is what others encrypt to and verify
signatures against.

**Node id** — an 8-byte public identifier derived as
`SHA-256("openmesh/v1/node" || ed25519_pub || x25519_pub)[0..8]`. Rendered as 16 hex characters
(e.g. `6b1f4a02c9de8810`). It is a hash, not a random label: anyone can verify that a public
identity matches a node id, which is what makes contact verification meaningful.

**Ed25519** — signature scheme used for announces and channel messages. It proves authorship and
integrity; it does not encrypt.

**X25519** — Diffie-Hellman key agreement. Direct messages derive a shared secret from the sender's
and recipient's static X25519 keys, then expand it with HKDF-SHA256. Static DH means no per-message
handshake (cheap on LoRa), at the cost of forward secrecy in v1 (OMP/2 adds ratchets).

**XChaCha20-Poly1305** — the AEAD used for message bodies and resource chunks. Provides
confidentiality and integrity; the 24-byte nonce is derived from the frame sequence/message id so
nonces never repeat for a given key.

**HKDF-SHA256** — key-derivation function turning a shared secret into purpose-specific keys
(direct messages, channel keys, attachments) with domain separation.

**PSK (pre-shared key)** — for channels, the 32-byte secret from which the channel id and message
keys are derived. Distributed inside `openmesh://channel?...` invites, never returned by
`GET /api/channels`. Anyone with the PSK can read and write the channel, so treat the invite as the
secret.

**AEAD / AAD** — authenticated encryption with associated data. OMP/1 authenticates the 22-byte
frame header as AAD **with the TTL nibble cleared**, because relays are allowed to decrement TTL;
any change to other header fields breaks authentication.

**End-to-end encryption (E2E)** — only the endpoints hold the keys. Relays (repeaters, backbones)
see ciphertext, node ids, sizes and timing; they cannot read message bodies. The local bridge of
the recipient is an endpoint and can read its own messages.

**Metadata** — what encryption does not hide: who is talking to whom (node ids), when, sizes,
approximate topology and presence. Treat it as visible to relays and radio observers.

---

## Protocol and mesh

**OMP/1** — OpenMesh Protocol version 1, the wire format. Managed flooding, announce-based
discovery, no global routing state, 240-byte MTU, AEAD payloads, dedup and TTL per frame.

**Frame** — one OMP/1 packet: fixed 22-byte header (version/type, TTL nibble, source, destination,
sequence, flags, payload length etc.) plus an encrypted body. Data/ACK/announce/position/control
types share the header. Maximum payload is 240 bytes.

**TTL** — hop limit. Each relay decrements it; frames die at zero. Client-originated frames default
to 5, announces to 8. Changing TTL is the only header mutation relays may perform.

**Managed flooding** — every node re-broadcasts frames it has not seen before, subject to TTL,
dedup and priority. It is robust without routing tables but costs aggregate airtime; see
[ADR 0002](adr/0002-managed-flooding.md).

**Dedup** — receivers remember `(source, sequence)` pairs for 30 minutes (≥ 2048 entries) so the
same frame arriving over multiple paths is processed once.

**Announce** — a signed broadcast with name, class, capabilities, public keys and position (if
permitted). Announces use TTL 8 and are never stored for store-and-forward. Clients announce every
30 minutes, repeaters every 60.

**ACK** — an acknowledgement frame. `delivered` for a data message (double tick), plus
`resource_window` ACKs for chunked media and read receipts. Unacked data is retried after 8 s, up to
3 attempts; ACKs are never retried.

**Resource** — a chunked, AEAD-sealed blob (image, voice note, long text, file). The manifest
describes chunk count and SHA-256; chunks are reassembled and verified before decryption.
Selective repeat with a window of 8 chunks; 180-byte chunks by default.

**Airtime** — the time a radio spends transmitting a frame. It is the shared scarce resource:
every relay, ACK and retry consumes it. Regulatory duty cycles (e.g. 1 %/hour in much of the EU)
cap it per transmitter.

**Duty cycle** — the allowed fraction of time a transmitter may be on air in a window. The bridge
models region budgets and warns before expensive sends. Fair-use regions use different rules.

**LoRa** — the long-range low-power modulation used by the radios. LoRa parameters are SF
(spreading factor), BW (bandwidth) and CR (coding rate); slower/higher-SF settings reach further at
the cost of airtime.

**Profile** — a named LoRa preset (`LONG_SLOW`, `LONG_FAST`, `MEDIUM_FAST`, `SHORT_FAST`,
`SHORT_TURBO`). All nodes and repeaters in a group must use the same profile to decode each other.

**Region** — the legal band/power regime for a country (`EU868`, `US915`, `AU915`, `IN865`,
`KR920`, `AS923`). Region and profile must match across the mesh.

**RSSI / SNR** — received signal strength (dBm) and signal-to-noise ratio (dB). RSSI closer to 0
is stronger; LoRa can decode below the noise floor, so negative SNR is normal at range.

**Node class** — advertised device type: `client`, `repeater`, `gateway`, `room_server`, `tracker`.
Descriptive for the UI and peers; distinct from the engine's local role.

**Role** — what the local engine does: a client forwards but does not store for others; a relay
floods frames and may store envelopes (default capacity 64, TTL 72 h, 120 frames/min budget).

**Capabilities (caps)** — a u16 bitmask advertised in announces (relay, store, GPS, Wi-Fi,
Ethernet, BLE, USB, screen, audio out, solar), rendered as strings in JSON APIs.

**Repeater** — headless node that forwards traffic and can store envelopes for offline nodes.

**Gateway** — node bridging LoRa to IP (Wi-Fi/Ethernet), e.g. to join a backbone.

**Room server** — node that stores and serves channel history for late joiners (M5 feature; not the
same as store-and-forward for direct messages).

**Tracker** — low-power node that mostly beacons position/telemetry.

**Store-and-forward** — relay behaviour: keep an encrypted envelope for an absent unicast
destination and offer it when the addressee reappears. Default retention 72 h, capacity 64
envelopes; oldest evicted first when full. Stored ≠ delivered.

**Envelope** — the stored form of a message for an absent node: ciphertext plus routing metadata.

**Mesh** — a set of nodes relaying for each other, extending range without infrastructure.

**Hop** — one relay traversal. `hops` in the UI is derived from announce TTL; it is a distance
estimate, not a route.

---

## Bridge and clients

**Bridge** — the local daemon (`openmesh-bridge`). Owns identity, radios, message store, API,
backbone mode and the MCP server. Clients (web, CLI, agents, mobile) talk to it, not directly to
radios.

**API token** — a random bearer token generated on first run at
`~/.config/openmesh/bridge-token` (mode 0600), required by all REST routes except `/api/health`
and by the WebSocket (`?token=`). Different from the OBB backbone token.

**WebSocket API** — the primary client protocol at `/api/ws`: JSON envelope
`{v:1,type,id?,data}`, server events (`hello`, `message`, `group_message`, `ack`, `node`,
`position`, `telemetry`, `backbone_status`, `backbone_peer`, …), client commands (`send_message`,
`send_group_message`, `send_position`, `get_messages`, `sync`, `set_config`, …).

**REST API** — loopback HTTP surface for status/config/nodes/contacts/channels/messages/
attachments/backbone/help/setup/activity/openapi. Documented by `docs/api/openapi.json` and served
at `/api/openapi.json`.

**SSE** — server-sent events stream at `/api/events` for simple live updates (same events as the
WebSocket, no commands).

**Demo mode** — `--demo`: in-process virtual transport plus seeded nodes/messages for UI review and
onboarding. No hardware, no real network, not persistent in the same way as a real mesh.

**Sim mode** — `--sim`: deterministic virtual transport with an empty state; knobs `--sim-loss`,
`--sim-latency`, `--sim-seed`; used by tests, docs and the simulator.

**CLI** — `openmesh-cli`: send/listen/announce/nodes/stats/channels/backbone/agent/help/airtime,
with `--sim` standalone mode and `--json` output.

**Airtime calculator** — `openmesh-cli airtime --profile … --bytes … --region …` and the UI's
pre-send estimate; computes LoRa airtime from the profile parameters and the payload size.

---

## Backbone

**Backbone** — an always-on bridge with a public OBB/1 listener that links distant meshes. Optional
and self-hosted; no central service.

**OBB/1** — the backbone transport: length-prefixed TCP frames carrying OMP frames, with an
authenticated handshake (`hello → challenge → auth(HMAC-SHA256) → welcome`), per-IP rate limits,
idle timeout and no token on the wire. Plaintext transport in v0.3.0; wrap it in WireGuard/TLS on
hostile networks.

**Backbone token** — the shared OBB/1 secret in `<data-dir>/backbone.json` (0600), generated on
first run, printed once (connection string / `--print-setup`), proved by HMAC. Rotate via panel,
`openmesh-cli backbone rotate-token` or `POST /api/backbone/token/rotate`.

**Connection string** — `openmesh://backbone?host=…&port=…&token=…&node=…&name=…&v=1`. Used by
`openmesh-bridge join`; contains the OBB token, so share it privately.

**Peer** — another bridge connected over OBB/1; peers have direction (`inbound`/`outbound`), remote
address, RTT (null until the first ping; default 30 s), frame/byte counters and auth mode.

**Allowlist** — optional node-id allowlist for a backbone, in addition to the token.

**Open mode** — backbone auth mode without a token (`auth: "open"`); only for trusted LAN/VPN,
shown as a red warning.

**Setup payload** — `GET /api/backbone/setup` / `--print-setup`: connection string, firewall
commands, systemd unit, gateway join command and notes. Treat it as secret.

---

## AI-native and UX

**MCP (Model Context Protocol)** — the standard by which AI clients discover and call tools,
read resources and use prompts. OpenMesh implements a local stdio server: `openmesh-bridge mcp`.

**MCP server** — the local JSON-RPC 2.0 process. It is a thin client of the bridge REST API, holds
no state/keys, and is configured by environment variables (`OPENMESH_API`, `OPENMESH_TOKEN`,
`OPENMESH_TOKEN_FILE`, `OPENMESH_MCP_READ_ONLY`, `OPENMESH_AGENT_NAME`, `OPENMESH_HELP_DIR`).

**Tool** — an MCP action such as `openmesh_send_message`. Read tools are always available; write
tools are hidden in read-only mode; destructive tools require `confirm: true`.

**Resource (MCP)** — a readable object exposed to agents: `openmesh://help/index`,
`openmesh://help/{slug}`, `openmesh://status`, `openmesh://nodes`, `openmesh://config`.

**Prompt (MCP)** — a user-selectable instruction template: `manage_my_mesh`, `set_up_my_node`,
`troubleshoot_mesh`, `set_up_backbone`, `invite_someone`.

**Activity log** — a bounded (256-entry) metadata-only log of write API calls with attribution
(`user`, `cli`, `web`, `agent:<name>`), shown at `/api/activity` and in the web Agents page. Read
calls are not logged; no bodies/tokens/PSKs appear.

**Read-only mode** — `OPENMESH_MCP_READ_ONLY=1`: the MCP server hides and refuses all write tools.

**Help corpus** — `content/help/*.md` + `index.json`, embedded in the bridge and web client and
served by `/api/help`; single source for the Help Center, docs, MCP and prompts. Checked by
`scripts/check_help.py`.

**Setup preset** — server-owned secure defaults applied by "Secure my node" / `POST
/api/setup/quick`: role, country→region, profile, privacy, store-forward, announce interval and an
optional private channel.

**Beginner / Advanced mode** — progressive-disclosure levels in the web client. Beginner is the
default for new installs; advanced reveals backbone/agents/activity and raw settings. The mode never
changes a configuration by itself.

**HelpTip / Term** — inline explanation popovers and glossary links in the UI, tied to help-topic
anchors; required by the UX spec for controls that cost airtime, battery or privacy.

---

## See also

[Help glossary (one line each)](../content/help/glossary.md) · [FAQ](guides/faq.md) ·
[PROTOCOL.md](architecture/PROTOCOL.md) · [WEBSOCKET-API.md](architecture/WEBSOCKET-API.md) ·
[docs/agents/mcp.md](agents/mcp.md) · [ROADMAP.md](ROADMAP.md)

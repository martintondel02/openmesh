# Glossary

One-line definitions for every OpenMesh term you are likely to meet. Terms are grouped roughly by
topic; the first mention in other help topics links back here.

## Radio and airtime

**LoRa** — a long-range, low-power radio modulation. Slow (hundreds of bits per second), but
capable of reaching kilometres with tiny transmitters.

**Airtime** — the time a radio spends transmitting a frame. The shared resource of a mesh; every
relay consumes it too.

**Duty cycle** — the share of time a transmitter may be on air (e.g. 1 % per hour in much of the EU).
Exceeding it is illegal and can jam your own mesh.

**Profile** — the LoRa modem preset: spreading factor (SF), bandwidth (BW) and coding rate (CR).
Slower profiles reach further; nodes must use the same profile to talk.

**Region** — the legal frequency band and power limits for your country (`EU868`, `US915`, `AU915`,
`IN865`, `KR920`, `AS923`).

**SF / BW / CR** — spreading factor (chirp time), bandwidth (kHz) and coding rate (error-correction
overhead). Higher SF = longer range and slower.

**RSSI** — received signal strength in dBm. Closer to zero is stronger; `-120 dBm` is marginal.

**SNR** — signal-to-noise ratio in dB. LoRa can decode below the noise floor, so negative values
are normal at range.

**Frame** — one OMP/1 radio packet: a 22-byte header plus an encrypted body of up to 240 bytes.

**MTU** — maximum frame payload, 240 bytes for OMP/1 data.

## Mesh concepts

**Node** — one instance of the bridge/radio with its own identity.

**Node id** — an 8-byte public identifier derived from your public keys; shown as 16 hex characters.

**Announce** — a signed broadcast saying "I exist, here is my name, class, capabilities and public
keys". Clients announce every 30 min, repeaters every 60 min.

**Hop / TTL** — each relay decreases the hop limit; frames are dropped at zero. Default TTL is 5
for client traffic, 8 for announces.

**Dedup** — receivers remember `(source, sequence)` pairs for 30 minutes so the same frame arriving
over several paths is shown once.

**ACK** — an acknowledgement frame confirming a message was received; the double tick in the app.

**Repeater** — a headless node that forwards frames and may store envelopes for offline peers.

**Gateway** — a node that bridges radio to IP (Wi-Fi/Ethernet), e.g. to join a backbone.

**Room server** — a node role that stores and serves channel history for late joiners.

**Tracker** — a low-power node that mostly beacons position/telemetry.

**Store-and-forward** — a relay keeping an encrypted envelope for an offline recipient and
offering it when they reappear (default TTL 72 h, capacity 64 envelopes).

**Envelope** — the stored form of a message destined for an absent node; ciphertext only.

**Mesh** — a group of nodes that relay for each other, extending range without infrastructure.

## Identity and security

**Identity seed** — the 32-byte secret in `identity.key` from which all your keys derive. Losing it
means losing your identity; leaking it means impersonation.

**Public identity** — the 72-byte public bundle (node id + Ed25519 + X25519 public keys) shared in
invites, announces and QR codes.

**Ed25519** — signature algorithm used for announces and channel messages.

**X25519** — Diffie-Hellman key agreement used to derive direct-message keys.

**XChaCha20-Poly1305** — authenticated encryption (AEAD) used for message bodies and resources.

**HKDF-SHA256** — key-derivation function that turns a shared secret into per-purpose keys.

**Node id derivation** — `SHA-256("openmesh/v1/node" || ed25519_pub || x25519_pub)[0..8]`.

**PSK** — pre-shared key. For channels it is the 32-byte secret from which the channel id and
message keys are derived. Never returned by `GET /api/channels`.

**Channel** — a group conversation identified by an 8-byte id derived from its PSK. Messages are
encrypted with the channel key and signed by the sender.

**Contact** — a saved identity you can message directly; direct messages are sealed to their
X25519 key.

**TOFU** — trust-on-first-use: a contact saved without comparing fingerprints. Convenient, weaker
than out-of-band verification.

**Fingerprint** — a short hash of a public key used to compare identities out of band.

**Open mode** — backbone auth mode without a token, for trusted LANs/VPNs only.

**Allowlist** — optional list of node ids allowed to connect to a backbone in addition to the
token.

**E2E / end-to-end encryption** — only endpoints hold keys; relays see ciphertext.

**Metadata** — what encryption cannot hide: node ids, sizes, timing, approximate topology.

## Backbone

**Backbone** — an always-on bridge with a public TCP listener that links distant meshes.

**OBB/1** — the backbone transport protocol: token-authenticated, length-prefixed TCP carrying OMP
frames. Plaintext in v0.3.0; wrap in VPN/TLS for hostile networks.

**Backbone token** — the shared OBB/1 secret stored in `<data-dir>/backbone.json` (0600), printed
once on first run; peers prove it with HMAC-SHA256.

**API token** — the bridge's local REST/WebSocket bearer token (`bridge-token`, 0600); what the
panel, CLI and agents use. Different from the backbone token.

**Peer** — another bridge connected over OBB/1; shown with direction, RTT, counters and auth mode.

**RTT** — round-trip time measured by backbone pings; `null` until the first ping (30 s default).

**Setup payload** — the onboarding JSON from `/api/backbone/setup`: connection string, firewall
commands, systemd unit, notes.

**Connection string** — `openmesh://backbone?host=…&port=…&token=…&node=…&name=…&v=1` used by
`openmesh-bridge join`.

**Join** — client mode of a bridge that dials a backbone and reconnects with backoff.

## App, API and agents

**Bridge** — the local daemon (`openmesh-bridge`) that owns identity, radios, store and API.

**Web client** — the browser app served by the bridge at `/` (panel at `/backbone`, help at
`/help`).

**Demo mode** — `--demo`: a virtual mesh with seeded data, no hardware needed.

**Sim mode** — `--sim`: virtual transport with an empty state, used for tests and experiments.

**API** — the local REST endpoints under `/api/` plus WebSocket `/api/ws` and SSE `/api/events`.

**WebSocket** — full-duplex JSON API used by the app for live events and commands.

**SSE** — server-sent events stream at `/api/events` for simple live updates.

**MCP** — Model Context Protocol: the standard by which AI agents discover and call tools.

**MCP server** — `openmesh-bridge mcp`: a stdio JSON-RPC process that exposes tools/resources/
prompts backed by the bridge API.

**Tool** — an MCP action such as `openmesh_send_message`; read tools are always available, write
tools can be disabled with read-only mode.

**Activity log** — a 256-entry ring of write actions with source (`user`, `agent:<name>`,
`web`/`cli`), path and summary; no bodies or secrets.

**Resource (MCP)** — a readable object exposed to agents, e.g. `openmesh://help/index`.

**Prompt (MCP)** — a ready-made agent instruction, e.g. `troubleshoot_mesh`.

**Read-only mode** — `OPENMESH_MCP_READ_ONLY=1`: the MCP server hides and refuses all write tools.

**Setup preset** — the server-owned secure defaults applied by "Secure my node"/`setup/quick`
(role, country, profile, privacy, channel).

**Beginner / Advanced mode** — UI disclosure levels; the same data and settings, fewer dials by
default in beginner mode.

**Help Center** — the in-app reader for these topics at `/help`, searchable and deep-linkable.

## Related topics

[Your first mesh in 10 minutes](getting-started.md) · [Security and privacy](security.md) ·
[Radios, regions and airtime](radio-and-region.md) · [FAQ](faq.md)

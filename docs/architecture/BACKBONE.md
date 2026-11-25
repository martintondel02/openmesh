# OpenMesh Backbone Mode (OBB/1)

**Status:** normative for v0.2.0
**Applies to:** `crates/openmesh-bridge` (server + client), `apps/web` (control panel),
`tools/flash`/installers (deployment), `docs/INSTALL.md`
**Related:** [PROTOCOL.md](PROTOCOL.md) (OMP/1 frames), [WEBSOCKET-API.md](WEBSOCKET-API.md) §8
(bridge API), [../INSTALL.md](../INSTALL.md)

A **backbone** is an always-on OpenMesh bridge that accepts authenticated TCP links from other
bridges, gateways and repeaters. It is the "tower-free tower": one VPS/Raspberry Pi with a public
port joins distant meshes into one network, and its web panel shows live statistics and controls.

OMP frames stay end-to-end encrypted; the backbone only relays ciphertext and routing metadata.
The TCP link is therefore a *transport*, not a trust boundary for message contents. Transport
authentication (a shared token) prevents unauthorised peers from injecting traffic.

```
                       ┌───────────────────────────────┐
  radio mesh A ────────│  openmesh-bridge backbone     │
  (LoRa/UDP)           │   OBB/1 TCP :9443  + panel    │──────── radio mesh B
                       │   store-and-forward relay     │
                       └───────────────────────────────┘
                                   ▲  ▲
                    OBB/1 clients  │  │  (bridges, gateways,
                    with token     │  │   repeaters over IP)
```

---

## 1. Operating modes

| Mode | Command | Behaviour |
| --- | --- | --- |
| Backbone server | `openmesh-bridge backbone` | Listens on TCP, authenticates peers, relays frames between peers and its own radio/network interfaces, exposes `/api/backbone*`, serves the panel. |
| Backbone client | `openmesh-bridge join <openmesh://backbone?...>` | Dials a backbone, reconnects with backoff, advertises itself, relays frames between its local interfaces and the backbone. Shows up in the server's peer table. |
| Legacy raw TCP | `--tcp-listen` / `--tcp-peer` | Pre-v0.2 length-prefixed frame stream, unauthenticated. Kept for compatibility, **not** exposed in the panel; use backbone mode for anything reachable from a network. |

A backbone server is a normal bridge in every other respect: it can own a LoRa radio, speak UDP on
the LAN, store messages for offline nodes (`store_forward`), and run the same WebSocket API for
local clients.

---

## 2. TCP framing

```
message := u16 BE length L (1..=4200) || payload[L]
payload[0] := kind
kind 0x00 : control message, payload[1..] = canonical CBOR (§3)
kind 0x01 : one complete OMP/1 frame, payload[1..] = frame bytes (≤ 240)
```

Unknown `kind` values are ignored (the length prefix keeps the stream in sync). A message with
`L == 0` or `L > 4200` is a protocol error and closes the connection. Each side MUST enforce
read/write timeouts and a maximum in-flight queue per peer.

---

## 3. Handshake

```
client                                    server
  │ ── HELLO ────────────────────────────▶ │   proto ovb/1, v 1, node_id, nonce, app, caps
  │ ◀────────────────────────── CHALLENGE ─ │   server node_id, server nonce, auth mode
  │ ── AUTH ─────────────────────────────▶ │   HMAC-SHA256 proof, or null for open mode
  │ ◀──────────────── WELCOME | AUTH_FAILED │   session, features, ping interval, max frame
  │ ── PING / PONG / STATS / frames ─────▶ │
```

Timeouts: 10 s per handshake step, 90 s without traffic closes the connection. The server MUST
rate-limit handshakes per remote IP (e.g. 10/minute, 30 concurrent pending) and MUST NOT log
tokens.

### 3.1 Control messages (CBOR maps, `kind 0x00`)

| `type` | Direction | Fields |
| --- | --- | --- |
| `hello` | C→S | `proto:"obb/1"`, `v:1`, `node_id:bytes(8)`, `app:str`, `nonce:bytes(16)`, `caps:u16` |
| `challenge` | S→C | `node_id:bytes(8)`, `nonce:bytes(16)`, `auth:"hmac-sha256"\|"open"`, `server:str` |
| `auth` | C→S | `mac: bytes(32)` or `mac: null` (open mode) |
| `welcome` | S→C | `session:u32`, `node_id:bytes(8)`, `name:str`, `features:[str]`, `ping_interval_s:u16`, `max_frame:u16`, `peers_connected:u16` |
| `auth_failed` | S→C | `reason:str` (`bad_token`, `not_allowlisted`, `max_peers`, `handshake_timeout`, `server_full`) |
| `ping` / `pong` | both | `nonce:u32` (`pong` MAY add `rtt_ms:u16` for the responder's view) |
| `stats` | both | `peers:u16`, `frames_in:u64`, `frames_out:u64`, `bytes_in:u64`, `bytes_out:u64`, `uptime_s:u64` |
| `close` | both | `reason:str` |
| `error` | both | `code:str`, `message:str` (then close) |

`features` in `welcome` currently: `["frames", "stats", "store_forward", "panel"]`.

### 3.2 Authentication

Default mode `token`:

```
token      : 32 random bytes, base64url (no padding), file mode 0600
mac        : HMAC-SHA256(key = token,
               "openmesh/backbone/v1/auth" || client_nonce(16) || server_nonce(16)
               || client_node_id(8) || server_node_id(8))
```

The server MUST compare the MAC in constant time. Tokens are never sent on the wire after the
initial `token`/rotation API response; a client only proves knowledge of them.

Mode `open` (explicit opt-in, `auth: "open"`) accepts `auth {mac: null}`. It exists for trusted
LANs and must be shown as a red warning in the panel and setup output.

Optional `allowlist`: when non-empty, only listed transport node ids may complete the handshake
(`auth_failed.not_allowlisted`). The transport node id is the peer's bridge identity, not
necessarily the source of every frame it relays.

---

## 4. Frame relay rules

After `welcome`, `kind 0x01` messages carry OMP frames:

1. A frame received from peer P is injected into the local engine (`Engine::ingest` with
   `LinkInfo { link: backbone(P), peer: P.node_id }`).
2. The engine emits `Outbound` frames. The bridge delivers each frame to:
   * every connected peer except the one it arrived from, and
   * every other local transport (LoRa / UDP / sim), honouring `SendMode::Direct(node)` by
     preferring the peer whose node id matches, otherwise flooding.
3. Relays respect OMP TTL/dedup exactly as on any other interface. The backbone MUST NOT rewrite
   frames or decrypt payloads.
4. Store-and-forward: with `store_forward: true` the backbone's engine keeps encrypted envelopes
   for absent unicast destinations and offers them when the addressee appears (see
   PROTOCOL.md §10). This is what makes it useful for a mesh whose nodes are only sometimes
   online.

---

## 5. Server configuration

Persisted at `<data-dir>/backbone.json` (mode 0600, created on first run):

```json
{
  "enabled": true,
  "bind": "0.0.0.0",
  "port": 9443,
  "auth": "token",
  "token": "<base64url 32 bytes>",
  "max_peers": 32,
  "allowlist": [],
  "store_forward": true,
  "name": "oslo-backbone",
  "public_host": "mesh.example.org",
  "ping_interval_s": 30,
  "stats_push_s": 30
}
```

* `public_host` is used to build the shareable connection string in the setup panel
  (falls back to the detected public/host address).
* Bind defaults to `0.0.0.0` **in backbone mode only** and prints a warning. The API (`:8787`)
  stays loopback-only unless explicitly changed.
* Listener settings (`bind`, `port`, `enabled`) apply live: the control API restarts the listener
  without dropping the process. Changing `auth`, `token`, `allowlist` disconnects peers that no
  longer authenticate on their next handshake; the panel offers a "kick all" after rotation.
* `max_peers` rejects new handshakes with `max_peers` once reached.
* Values are clamped/validated server-side (`port` 1..65535, `max_peers` 1..256, `name` ≤ 32
  chars, `public_host` ≤ 255 chars, allowlist entries 8-byte node ids).

---

## 6. Connection string

```
openmesh://backbone?host=<host>&port=<port>&token=<base64url>&node=<server_node_id>&name=<label>&v=1
```

* `token` is omitted in open mode.
* Used by `openmesh-bridge join '<url>'` and shown (with copy + QR) in the panel.
* The token is a bearer secret: the panel hides it behind a reveal button and the setup
  instructions include rotation + firewall guidance.

---

## 7. Control surface

### 7.1 REST (all require the bridge token unless noted)

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/api/backbone` | `{enabled, running, bind, port, auth, token_set, max_peers, allowlist, store_forward, name, public_host, ping_interval_s, totals:{...}}` — token never returned |
| GET | `/api/backbone/peers` | `[{node_id, name, direction, remote_addr, connected_at_ms, last_seen_ms, frames_in, frames_out, bytes_in, bytes_out, rtt_ms, app_version, auth}]` |
| POST | `/api/backbone/config` | Partial update of §5 fields (except token); returns the new config + `restarted` flag |
| POST | `/api/backbone/token/rotate` | Returns `{token: "<new base64url>"}` **once**; old token is invalid immediately |
| DELETE | `/api/backbone/peers/:node_id` | Disconnect a peer (`{disconnected:true}`) |
| GET | `/api/backbone/setup` | Ready-to-copy onboarding payload, see §8 |
| GET | `/api/backbone/qr.svg` | QR code (SVG) of the current connection string, for terminal/print use |

### 7.2 WebSocket events / commands

Events (server → client, pushed while connected and on change):

```json
{ "v":1, "type":"backbone_status", "data": {
  "running": true, "peers": 2, "uptime_s": 3600,
  "frames_in": 1042, "frames_out": 998, "bytes_in": 250080, "bytes_out": 239520,
  "throughput_in_bps": 812, "throughput_out_bps": 764,
  "store_forward_envelopes": 3, "auth_failures": 0, "last_error": null } }

{ "v":1, "type":"backbone_peer", "data": {
  "action": "joined" | "left" | "updated", "peer": { ...same shape as GET /api/backbone/peers... } } }
```

Commands (client → server): `get_backbone`, `set_backbone_config`, `backbone_kick`
(`{node_id}`), `backbone_rotate_token`. Unknown fields are ignored, changes are broadcast back
via `backbone_status` / `config`.

---

## 8. Setup payload (`GET /api/backbone/setup`)

Everything the panel needs to onboard a user without reading docs:

```json
{
  "listen": "0.0.0.0:9443",
  "public_host": "mesh.example.org",
  "connection_string": "openmesh://backbone?host=...&port=9443&token=...&node=...",
  "token_set": true,
  "auth": "token",
  "steps": [
    { "title": "Install OpenMesh", "commands": ["curl -fsSL .../install.sh | sh"], "note": "..." },
    { "title": "Start the backbone", "commands": ["openmesh-bridge backbone --data-dir ~/.openmesh"], "note": "..." },
    { "title": "Open the firewall", "commands": ["sudo firewall-cmd --add-port=9443/tcp --permanent", "sudo firewall-cmd --reload"], "note": "..." },
    { "title": "Keep it running", "commands": ["openmesh-bridge install-service --backbone"], "note": "systemd user service, survives reboot" },
    { "title": "Connect a gateway", "commands": ["openmesh-bridge join '<connection-string>'"], "note": "..." }
  ],
  "systemd_unit": "[Unit]\n...",
  "firewall": { "firewalld": ["..."], "ufw": ["..."] },
  "exposure": {
    "recommended": "openflare",
    "provider": { "name": "Openflare", "url": "https://github.com/martintondel02/openflare",
                  "description": "Self-hosted tunnel, reverse proxy and control plane with outbound-only connectors and one public edge." },
    "when_to_use": "Your backbone is behind NAT/CGNAT, or you do not want to open an inbound port.",
    "steps": ["bind the backbone to 127.0.0.1", "create a TCP tunnel to 127.0.0.1:9443", "set public_host to the tunnel hostname", "share the connection string"],
    "alternatives": ["WireGuard", "Tailscale", "Cloudflare Tunnel", "Caddy/nginx stream TLS"],
    "bind_command": "openmesh-bridge backbone --bind 127.0.0.1 --port 9443 --public-host <host>",
    "guide_url": "https://github.com/martintondel02/openmesh/blob/main/docs/guides/backbone-exposure.md"
  },
  "notes": ["OpenMesh payloads are end-to-end encrypted; the backbone relays ciphertext only."],
  "panel_url": "http://127.0.0.1:8787/backbone"
}
```

Command strings are generated from the live configuration (port, public host, binary name).
The web panel renders these with copy buttons and a QR code; the CLI prints the same thing with
`--print-setup`.

---

## 9. CLI surface

```bash
# Server (first run generates a token, persists config, prints onboarding)
openmesh-bridge backbone [--bind 0.0.0.0] [--port 9443] [--data-dir DIR]
                         [--token-file FILE] [--token TOKEN] [--open]
                         [--max-peers N] [--allowlist ID,ID] [--name NAME]
                         [--public-host HOST] [--no-store-forward]
                         [--listen 127.0.0.1:8787] [--serve-web DIR] [--print-setup]

# Client (persistent outbound link to a backbone, reconnects with backoff)
openmesh-bridge join 'openmesh://backbone?host=...&port=9443&token=...'

# Service installation (writes a systemd user unit and prints the commands)
openmesh-bridge install-service --backbone [--system]

# Diagnostics through the local API
openmesh-cli backbone info
openmesh-cli backbone peers
openmesh-cli backbone kick <node_id>
openmesh-cli backbone rotate-token
openmesh-cli backbone connect '<url>'
openmesh-cli backbone disconnect <node_id>
```

`openmesh-cli` reads the bridge token from `~/.config/openmesh/bridge-token` (or `--token` /
`$OPENMESH_TOKEN`) and talks to `--api http://127.0.0.1:8787`.

---

## 9b. Public exposure (NAT, firewalls and Openflare)

A backbone is only useful to remote gateways if they can reach its OBB/1 port. Options, in order of
preference:

| Situation | Recommended |
| --- | --- |
| Gateways are on a VPN/WireGuard/Tailscale mesh | Keep the backbone bound to the VPN interface; no public exposure |
| Behind NAT/CGNAT, or you refuse to open an inbound port | **Openflare** (outbound-only connector + one public TCP edge) or another tunnel |
| You control the firewall and DNS | Port-forward 9443/tcp and terminate TLS in front (Caddy/nginx stream) |
| Temporary testing only | `ssh -L`/WireGuard from the gateway host |

`/api/backbone/setup` exposes an `exposure` object with the recommended provider, ready-to-copy
steps, alternatives, an optional copyable `bind_command` and a documentation `guide_url`, so the
panel and CLI can walk a user through it without external docs. `provider` is
`{name, url, description}`; `steps` and `alternatives` are arrays of strings. Clients MUST tolerate
the whole object being absent (older bridges) and fall back to local guidance.

**Openflare** (<https://github.com/martintondel02/openflare>) is a self-hosted tunnel, reverse proxy
and control plane with outbound-only connectors and one public edge: run a connector next to the
backbone, point a TCP service at `127.0.0.1:9443`, and give the backbone the resulting public
hostname via `--public-host` (or in the panel). It handles certificates and the public edge, so no
inbound port needs to be opened. The repository is being prepared for public launch; until it is
available, the WireGuard/Tailscale and Caddy/nginx recipes in
[`docs/guides/backbone-exposure.md`](../guides/backbone-exposure.md) work today.

Hard rules when exposing a backbone:

1. Expose **only the OBB/1 TCP port**. The bridge API/panel (`:8787`) stays loopback-only.
2. Keep `auth: token` (never `open`) and rotate the token if it ever leaked; allowlist known peer
   node ids when the set is small.
3. End-to-end encryption is unchanged by exposure: the tunnel operator sees ciphertext and
   metadata only, never message contents. The token is the transport access control.
4. If a tunnel terminates TLS at the edge, keep the token regardless — TLS is transport hygiene,
   not peer authentication.
5. Watch the panel's peer table and `auth_failures`; enable activity review and alerts for unknown
   peers.

## 10. Security model

| Concern | Mitigation |
| --- | --- |
| Unauthorised frame injection | HMAC-SHA256 token proof (constant-time compare), optional allowlist, max peers, handshake rate limits |
| Payload confidentiality | OMP frames are already E2E encrypted; the backbone stores/relays ciphertext only. It can see source/destination ids, sizes and timing. |
| Transport eavesdropping | For hostile networks terminate TLS in front of the port (Caddy/nginx/stunnel) or use WireGuard/Tailscale; documented in `docs/operations/backbone-deployment.md`. Native TLS may come in a later release. |
| Token leakage | Never logged, never returned by GET endpoints, hidden behind reveal in the panel, one-click rotation disconnects stale peers, example config files use placeholders |
| DoS | per-IP handshake rate limit, 10 s handshake timeout, `max_peers`, per-peer queue cap, 90 s idle close, backpressure on slow peers |
| Privilege | backbone runs unprivileged, binds a high port, stores config 0600; systemd unit applies `NoNewPrivileges`, `ProtectSystem`, `PrivateTmp` |
| Replay | handshake nonces are fresh per connection; HMAC binds both nonces and both node ids |

**What the operator can and cannot see:** metadata (which pseudonymous node ids exchange how many
bytes when), and the contents of traffic from nodes that trust the backbone operator's own
identity. Never message plaintext from other nodes.

---

## 11. Acceptance criteria (v0.2.0)

1. `openmesh-bridge backbone` starts, prints the onboarding guide, persists `backbone.json`
   (0600), serves the panel and `/api/health`.
2. Wrong token / allowlist miss / over-capacity are rejected with `auth_failed` + reason, counted
   in `auth_failures`, and never crash or leak the token.
3. `openmesh-bridge join <url>` connects, appears in `/api/backbone/peers`, relays an OMP frame
   end-to-end between two bridges over real TCP (integration test with two processes or two
   in-process bridges), reconnects after the server restarts.
4. Panel: stats cards, peer table with kick, live enable/disable + port change, token rotation
   (shown once), allowlist editor, store-forward toggle, setup wizard with copy buttons + QR;
   demo mode renders the panel with seeded peers and no backend.
5. `install.sh` installs a release binary + web bundle, verifies sha256, and `install-service`
   produces a working user systemd unit; docs cover Fedora/Debian/macOS/Windows and firewall.
6. All existing CI gates stay green plus new tests for handshake auth, peer accounting, control
   API, and panel components.

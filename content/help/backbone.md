# Run a backbone

A **backbone** is an always-on OpenMesh bridge with a public TCP listener. It stitches distant
meshes together — hikers in the mountains, family in a city, a second community across the
country — without a central service. This topic is for advanced users; it assumes you are
comfortable with a terminal, firewalls and systemd.

## What it is

* **OBB/1** — the backbone transport: length-prefixed TCP frames carrying OMP frames between
  bridges. It can be enabled with `openmesh-bridge backbone`; clients connect with
  `openmesh-bridge join '<openmesh://backbone?...>'`.
* **Two tokens** — the **OBB/1 backbone token** (shared secret in `<data-dir>/backbone.json`,
  mode 0600, printed once on first run) authenticates peers via HMAC-SHA256 and never crosses the
  wire. The **bridge API token** (`~/.config/openmesh/bridge-token`) protects the local panel/REST
  API and is a different secret.
* **Control panel** — the bridge serves the web UI at `/backbone` on loopback
  (`http://127.0.0.1:8787/backbone`), with live stats, peers, config, token rotation and setup/QR.
* **Security posture** — OMP payloads are end-to-end encrypted; the backbone relays ciphertext and
  metadata. OBB/1 itself is plaintext in v0.3.0: put WireGuard/Tailscale or a TLS wrapper in front
  for hostile networks.

## Why it matters

Radio range is limited by physics and terrain. A backbone turns one machine with a public IP into
a relay that connects several local meshes, while remaining self-hosted and account-free. It is the
difference between a valley network and a regional one — and it concentrates trust: the backbone
operator sees peer metadata (who is talking to whom, sizes, timings), though not message contents.

## Quick start

### 1. Start a backbone

On a VPS, Raspberry Pi or a box with a public address:

```sh
openmesh-bridge backbone \
  --data-dir ~/.openmesh \
  --serve-web ~/.local/share/openmesh/web \
  --listen 127.0.0.1:8787
```

First run prints an onboarding guide and creates `~/.openmesh/backbone.json` (mode 0600) with a
fresh OBB token. The OBB listener defaults to `0.0.0.0:9443`; the API stays on loopback. Use
`--print-setup` to print the guide again without starting the daemon.

### 2. Open the firewall

```sh
sudo firewall-cmd --permanent --add-port=9443/tcp && sudo firewall-cmd --reload   # Fedora
sudo ufw allow 9443/tcp                                                          # Debian/Ubuntu
```

Open **only** 9443/tcp publicly. Never expose 8787; reach the panel via SSH tunnel
(`ssh -L 8787:127.0.0.1:8787 user@host`) or a TLS reverse proxy with authentication.

### 3. Connect a gateway

Copy the connection string from the panel (or `openmesh-cli backbone info`) and run on the other
bridge:

```sh
openmesh-bridge join 'openmesh://backbone?host=mesh.example.org&port=9443&token=…&v=1'
```

It reconnects automatically with backoff and appears in the peer table.

### 4. Keep it running

```sh
openmesh-bridge install-service --backbone     # hardened systemd user unit + enable commands
```

For a system-wide service use `--system` (or the unit in `packaging/systemd/`). The unit passes
`--data-dir`, `--listen`, `--serve-web`, `--port` and (when supplied) `--token-file`; review it
with `systemctl --user cat openmesh-backbone.service`.

## Advanced notes

### Flags you will actually use

| Flag | Meaning |
| --- | --- |
| `--bind ADDR` | OBB listener bind (default `0.0.0.0`; prints a warning) |
| `--port PORT` | OBB listener port (default 9443) |
| `--token-file PATH` | **bridge API token** file (not the OBB secret) |
| `--token VALUE` | bridge API token value (provisioning/tests) |
| `--backbone-token TOKEN` / `--backbone-token-file PATH` | pre-provision the OBB/1 secret (additive, optional) |
| `--open` | disable OBB peer auth (trusted LAN/VPN only; red warning) |
| `--max-peers N`, `--allowlist ID,ID` | peer cap and node-id allowlist |
| `--name NAME`, `--public-host HOST` | display name and connection-string host |
| `--no-store-forward` | disable relay store-and-forward |
| `--print-setup` | print the onboarding payload and exit |

### Configuration

`<data-dir>/backbone.json` (0600) holds `enabled`, `bind`, `port`, `auth`, `token`, `max_peers`,
`allowlist`, `store_forward`, `name`, `public_host`, `ping_interval_s`, `stats_push_s`. Listener
settings apply live; changing auth/token/allowlist disconnects peers that no longer authenticate.
The panel edits the same file through `POST /api/backbone/config`.

### REST/CLI

`GET /api/backbone`, `/api/backbone/peers`, `/api/backbone/setup`, `/api/backbone/qr.svg`; mutating
`POST /api/backbone/config`, `POST /api/backbone/token/rotate`, `DELETE /api/backbone/peers/:id`.
CLI equivalents: `openmesh-cli backbone info|peers|kick <node_id>|rotate-token|connect <url>|
disconnect <node_id>`.

### Security and operations

* **Transport** — OBB/1 has no native TLS in v0.3.0. Use WireGuard or Tailscale (recommended), or
  terminate TLS with nginx `stream` + `stunnel` on each peer. SSH tunnels are fine for a single
  gateway but not for a mesh.
* **Token rotation** — panel → Rotate token, or `openmesh-cli backbone rotate-token`. The new token
  is shown once; stale peers are disconnected. Rotate on member departure or suspected leak.
* **Threat model** — token auth prevents unauthorised frame injection; payloads are E2E encrypted;
  the operator can observe metadata and drop/delay traffic. Use a VPN for metadata protection.
* **Monitoring** — `GET /api/health` (public), `/api/backbone` (config + `totals` nested), peer
  list with RTT/counters, systemd logs. `rtt_ms` is `null` until the first ping (30 s default).
* **Sizing** — frame fan-out grows with peers; text is cheap, media expensive. A 1 vCPU/512 MB
  VPS handles tens of peers for text; see the operations guide for formulas and tuning.
* **Backups** — back up `identity.key`, `backbone.json` and the API token; losing the OBB token
  means every peer must be reconnected with a new connection string.

## Troubleshooting

| Symptom | Check | Fix |
| --- | --- | --- |
| Peers cannot connect | Firewall/cloud security group, wrong host/port, listener bound to loopback | `nc -vz host 9443`; open 9443/tcp; check `--bind`. |
| `auth_failed` | Wrong/rotated OBB token, allowlist, max peers | Re-copy the connection string; check allowlist/max-peers; rotate only intentionally. |
| Panel unreachable | API on loopback only, or token missing | SSH tunnel to 8787; paste `~/.config/openmesh/bridge-token`. |
| Backbone dies after logout | No linger for the user service | `sudo loginctl enable-linger "$USER"`. |
| Peers connect but no traffic | Profile/region mismatch on the joined mesh, or TTL too small | Match profiles/regions; raise TTL at the edge if needed. |
| Token lost | `backbone.json` deleted/corrupt | Restore backup or rotate and re-share; identity stays intact. |

## Related topics

[Security and privacy](security.md) · [Install OpenMesh](install.md) ·
[Use your AI agent](agents.md) · [Troubleshooting](troubleshooting.md)

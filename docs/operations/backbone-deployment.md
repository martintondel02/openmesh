# Backbone deployment and operations

**Status:** v0.2.0 · audience: operators running a public/always-on OpenMesh backbone ·
**Last updated:** 2026-11-25

This guide covers running `openmesh-bridge backbone` in production: choosing a host, sizing,
monitoring, backups, TLS/VPN, multi-region topologies, rate limits, upgrades and incident
response. Installation itself is in [INSTALL.md](../INSTALL.md); the protocol contract is
[BACKBONE.md](../architecture/BACKBONE.md).

**Scope reminder:** a backbone relays **end-to-end encrypted OMP frames** and metadata. It can
see which pseudonymous node ids exchange how many bytes when, and it can read traffic from nodes
that trust its own identity — but not message plaintext from other nodes. Treat it as
infrastructure, not as a messaging provider.

---

## 1. What you are deploying

```
                       ┌──────────────────────────────────────────────┐
   radio mesh A ──────▶│  openmesh-bridge backbone                    │
   (LoRa / UDP / TCP)  │   • OBB/1 TCP listener   :9443 (public)      │
                       │   • local API + panel    :8787 (loopback)    │
                       │   • OMP/1 engine, store-and-forward, redb    │──▶ radio mesh B
                       └───────────────▲──────────────▲───────────────┘
                                       │              │
                        OBB/1 clients  │              │  OBB/1 clients
                        (bridges,      │              │  (bridges, gateways,
                         gateways)     │              │   repeaters over IP)
```

A backbone is a normal bridge plus a public authenticated TCP listener. It can also own a LoRa
radio and speak UDP/TCP on a LAN, so one box can join a local radio mesh to the internet. Peers
authenticate with a shared token (HMAC-SHA256 handshake); frames stay encrypted end-to-end, so
the backbone does not need to be trusted with message contents.

Deployment shapes covered here:

1. **Single backbone** on a VPS/Raspberry Pi with a public port; gateways `join` it.
2. **Backbone behind a VPN** (WireGuard/Tailscale) with only the VPN port public — recommended
   when the network is hostile.
3. **Multi-region backbones** chained (`backbone A` joins `backbone B`) and/or connected over a
   VPN, with store-and-forward bridging islands.
4. **Container** deployment (Docker/Compose) for operators who prefer immutable hosts.

---

## 2. Host selection

| Host class | Suitable for | Notes |
| --- | --- | --- |
| 1 vCPU / 512 MB VPS (x86_64) | a regional backbone with up to ~32 peers | smallest sensible public host; add swap or 256 MB RAM for comfort |
| 2 vCPU / 1–2 GB VPS | up to ~64+ peers, busy store-and-forward, media relay | headroom for redb cache and bursty attachment traffic |
| Raspberry Pi 4/5 (4 GB) | home/solar-adjacent backbone, ≤ 10–20 peers | always-on power, **USB SSD instead of SD card** (redb writes and logs wear cards), wired Ethernet or good Wi-Fi, UPS |
| Home server / NAS | trusted private backbone | fine for a club/community; expose only 9443 or use a VPN |
| Container host | any of the above | see [§8](#8-containers); host networking recommended on Linux |

Requirements:

* a stable public IPv4 and/or IPv6 address (or a VPN address) and DNS if clients use a hostname;
* inbound **9443/tcp** reachable from peers (or only the VPN port);
* outbound HTTPS for nothing at runtime (the daemon does not phone home), but you want it for
  upgrades;
* accurate clock (NTP) — logs and handshake/rotation timing assume sane UTC;
* OS: any modern Linux; macOS/Windows work for small non-service deployments; Raspberry Pi OS
  (aarch64) works from a source build.

Do **not** run a backbone on metered mobile data or a laptop that sleeps: OBB/1 keeps a
long-lived connection and reconnects with backoff; reliability is the point.

---

## 3. Sizing and capacity

These are **planning estimates**, not measured benchmarks. They derive from the OBB/1 framing
(2-byte length + 1-byte kind + OMP frame ≤ 240 B, plus TCP/IP overhead) and the OMP message
sizes in [PROTOCOL.md](../architecture/PROTOCOL.md) Appendix A. Measure your own deployment with
the panel counters and `/api/backbone` (§4) — the formulas below are there so you can redo the
estimate for your traffic mix.

### 3.1 Bandwidth model

A backbone floods received OMP frames to its other peers, so a frame is transmitted once per
peer (fan-out `F ≈ peers − 1`). Approximate on-wire bytes per OMP frame:

```
frame_wire ≈ 240 B max + 3 B OBB header + ~40 B TCP/IP ≈ 283 B  (worst case)
```

| Event | OMP frames | Per-event traffic on a backbone with 16 peers (F=15) |
| --- | --- | --- |
| Announce (one node, every 30–60 min) | 1 (≤ 240 B) | ~4.2 KB per announce relayed |
| 100-char text message + ACK | 2 (≈ 200 B each) | ~8.5 KB |
| Position report | 1 | ~4.2 KB |
| 5 KiB image | ~35 (manifest + 29 chunks + ACKs) | ~149 KB per transfer |
| 24 KiB image (max cap on fast profiles) | ~157 | ~667 KB per transfer |
| 8 s Opus voice note (~7 KiB) | ~44 | ~187 KB per transfer |

Sustained bandwidth depends on human activity, not peer count:

```
outbound_Bps ≈ (messages_per_minute / 60) × frames_per_message × 283 × F
```

Examples: 6 text messages/min with 16 peers ≈ **0.85 KB/s (≈ 7 kbit/s)** outbound; 1 image per
minute (5 KiB) with the same fan-out ≈ **2.5 KB/s (≈ 20 kbit/s)** average, with bursts of a few
hundred kbit/s while a transfer is relaying. A 100 Mbit/s VPS port is far beyond what a text
mesh needs; the practical limits are per-peer queues and the peers' own links.

### 3.2 CPU and memory

OBB/1 authenticates only at handshake time (HMAC-SHA256); steady-state work is framing, dedup
and copying ciphertext. The OMP payload crypto happens at the endpoints, not on the backbone.

| Load | Suggested CPU/RAM | Reasoning |
| --- | --- | --- |
| ≤ 8 peers, text/position | 1 shared vCPU, 256–512 MB | handshake bursts and redb; mostly idle |
| ≤ 32 peers, text + occasional media | 1 vCPU, 512 MB–1 GB | per-peer queues + redb cache + web static serving |
| 32–64 peers, busy media/store-forward | 2 vCPU, 1–2 GB | bursty fan-out, attachment reads, larger cache |
| > 64 peers | measure first; shard by region | OBB/1 floods to peers; fan-out is the cost driver |

Add `MemoryMax=512M` (already in the units) and watch `systemctl status`/cgroup memory; the
daemon fails cleanly and restarts on OOM rather than taking the host down.

### 3.3 Disk

Backbone state lives in the data dir (identity, redb database, attachments, `backbone.json`):

| Item | Approximate size |
| --- | --- |
| `identity.key` + `backbone.json` + API token | < 1 KB (back these up!) |
| text message row (includes indexes/metadata) | ~0.3–1 KB per message |
| image/voice attachment | the file size (compressed client-side: typically 1–24 KiB) |
| store-and-forward envelopes (relays) | ciphertext frames, default queue 64 envelopes / 72 h |

Rules of thumb: **1 million text messages ≈ 0.5–1 GB**; a community with 100 KiB/day of media
for a year ≈ 35 GB (unlikely; most media is not stored forever — check your retention settings).
Monitor free space and the store counters in the panel; set `store_ttl_ms`/capacity per your
retention policy.

### 3.4 Measuring the real numbers

After a day of traffic:

```bash
TOKEN=$(cat ~/.config/openmesh/bridge-token)
curl -fsS -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8787/api/backbone | jq .
```

The REST response has the live counters nested under `totals`
(`frames_in`, `frames_out`, `bytes_in`, `bytes_out`, `auth_failures`, `uptime_s`, `peers`), while
the `backbone_status` WebSocket event reports the same values flat. Use two samples 60 s apart
to compute rates; compare against §3.1. The peer list (`/api/backbone/peers`, a JSON array of
peer objects with direction, remote address, RTT and per-peer counters) is the place to find a
peer that is retrying or flooding.

---

## 4. Monitoring

### 4.1 Health and status endpoints

| Endpoint | Auth | Use |
| --- | --- | --- |
| `GET /api/health` | public | liveness (`{"ok":true,"version":"0.2.0"}`); use for uptime checks/healthchecks |
| `GET /api/backbone` | bearer | config + live totals (token redacted) |
| `GET /api/backbone/peers` | bearer | per-peer table (direction, remote, RTT, counters) |
| `GET /api/backbone/setup` | bearer | onboarding payload incl. connection string; treat as secret |
| `GET /api/backbone/qr.svg` | bearer | QR of the connection string |
| WS events | bearer | `backbone_status`, `backbone_peer` (`joined`/`left`/`updated`) |

When the bridge is **not** in backbone mode, `GET /api/backbone` returns
`{"enabled": false, ...}` (HTTP 200) and `/api/backbone/peers` returns an empty list (or
`{"enabled":false,"peers":[]}` on a plain bridge) rather than 404, so tooling can treat "not
enabled" as a normal state. `mode` is `server` on a backbone, `client` after `join`, and the
`enabled` flag reflects whether the local OBB/1 listener is active.

Peer entries from `/api/backbone/peers` have the shape:

```json
{"node_id":"…","name":"…","direction":"inbound|outbound","remote_addr":"ip:port",
 "connected_at_ms":0,"last_seen_ms":0,"frames_in":0,"frames_out":0,"bytes_in":0,"bytes_out":0,
 "rtt_ms":41,"app_version":"…","auth":"token|open"}
```

`rtt_ms` is `null` until the first PING/PONG (default interval 30 s) — display it as "—" rather
than 0 in dashboards, and do not alert on it during the first ping window.

Minimal monitoring loop (cron/systemd timer, or your monitoring agent):

```bash
#!/bin/sh
set -eu
TOKEN=$(cat "$HOME/.config/openmesh/bridge-token")
health=$(curl -fsS --max-time 5 http://127.0.0.1:8787/api/health) || { echo "UNHEALTHY"; exit 1; }
backbone=$(curl -fsS --max-time 5 -H "Authorization: Bearer $TOKEN" \
  http://127.0.0.1:8787/api/backbone) || { echo "API AUTH FAILED"; exit 1; }
printf '%s\n' "$backbone" | jq -r \
  '"peers=\(.peers) in=\(.totals.frames_in) out=\(.totals.frames_out) auth_fail=\(.totals.auth_failures)"'
```

### 4.2 Alert candidates

| Signal | Warning | Critical | Action |
| --- | --- | --- | --- |
| `/api/health` failing for 2+ checks | 1 min | 5 min | check service, host, disk; restart if wedged |
| `auth_failures` delta | > 5/min | > 20/min | wrong/rotated token, scanner, or abuse; check logs/remote IPs, keep 9443 firewalled as intended |
| `peers` | ≥ 80 % of `max_peers` | = max_peers | raise limit after sizing check, or investigate a misbehaving peer |
| `store_forward_envelopes` | ≥ 70 % capacity | ≥ 90 % | review retention/quota, disk space, offline peers |
| disk free (data dir) | < 20 % | < 10 % | clean attachments/DB per retention policy; expand disk |
| `systemctl status` restarts | > 3/hour | crash loop | logs (`journalctl`), disk, config; roll back upgrade |
| TLS certificate expiry (panel/OBB proxy) | 14 days | 7 days | renew (certbot/Caddy automatic) |
| outbound throughput | near WAN uplink for > 5 min | link saturated | check for a peer flood/abuse; kick peer; rate-limit at firewall |

### 4.3 Logs

The daemon logs to stderr (systemd/journald); `RUST_LOG=info` is the default, `debug` is
verbose. It **never logs tokens**.

```bash
journalctl --user -u openmesh-backbone.service -f            # user service
sudo journalctl -u openmesh-backbone.service -f              # system service
journalctl --user -u openmesh-backbone.service --since '1 hour ago' | grep -i 'auth\|error\|warn'
```

Rotate/limit journald (e.g. `SystemMaxUse=500M` in `journald.conf`) and keep logs for incident
analysis. Structured log aggregation is optional; a line-oriented journal is enough for a mesh
backbone.

### 4.4 Panel as a dashboard

The panel (loopback/SSH tunnel/TLS proxy) shows status cards, throughput, peers with kick,
configuration toggles, token rotation and the setup wizard/QR. It is a convenience, not a
replacement for health checks: do not point external monitoring at it if it is behind auth.

---

## 5. Backup and restore

### 5.1 What to back up

| Path | Why | Secret? |
| --- | --- | --- |
| `<data-dir>/identity.key` | node identity; losing it changes the node id | **yes**, mode 0600 |
| `<data-dir>/backbone.json` | OBB/1 settings + **backbone token** | **yes**, 0600 |
| bridge token (`~/.config/openmesh/bridge-token` or `/etc/openmesh/bridge-token`) | panel/API access | **yes**, 0600 |
| `<data-dir>/openmesh.redb` | messages, contacts, channels, nodes | contains metadata/message text |
| `<data-dir>/attachments/` | stored media | potentially sensitive |
| unit files / compose config | reproducible deployment | no |

Encrypt backups (`age`, `gpg`, restic/borg) and store them offline; losing `identity.key` is
recoverable (new identity, contacts need re-announcing) but losing the backbone token means
reconnecting every peer.

### 5.2 Consistent backup

Stop the service for a copy-consistent backup of the database, or snapshot the whole data dir at
the filesystem/LVM level:

```bash
systemctl --user stop openmesh-backbone.service      # or sudo systemctl stop ...
tar -C "$HOME" -czf "openmesh-backup-$(date +%F).tar.gz" .openmesh .config/openmesh
systemctl --user start openmesh-backbone.service
```

Docker (named volume):

```bash
docker run --rm -v openmesh-data:/data -v "$PWD":/backup alpine \
  tar -C /data -czf /backup/openmesh-data-$(date +%F).tar.gz .
```

### 5.3 Restore

```bash
systemctl --user stop openmesh-backbone.service
tar -C "$HOME" -xzf openmesh-backup-YYYY-MM-DD.tar.gz
chmod 600 ~/.openmesh/identity.key ~/.openmesh/backbone.json ~/.config/openmesh/bridge-token
chmod 700 ~/.openmesh ~/.config/openmesh
systemctl --user start openmesh-backbone.service
curl -fsS http://127.0.0.1:8787/api/health
```

If the restore is on a new hostname/IP, update `public_host` in the panel/config and re-share the
connection string. If you restored only the identity but not `backbone.json`, a new token is
generated — every peer must use the new connection string.

Test a restore at least once before you need it.

---

## 6. TLS and VPN options

v0.2.0 has **no native TLS inside OBB/1**: the token is proved by HMAC, and payloads are
end-to-end encrypted, but the TCP stream itself is plaintext unless wrapped. Options, best first:

| Option | Protects | Complexity | Recommendation |
| --- | --- | --- | --- |
| **WireGuard** (or **Tailscale**) | transport + network access control | low–medium | **Recommended for public internet**: bind the backbone to the VPN address or firewall 9443 to the VPN, open only the VPN port |
| TLS wrapper: nginx `stream` + `stunnel` on clients | transport only | medium | use when peers cannot install a VPN; requires client-side wrapper on every joining host |
| Caddy with `caddy-l4` plugin | transport only | medium | possible, but not stock Caddy; nginx stream is simpler to audit |
| SSH tunnel for each peer | transport | low for one peer | fine for a single temporary gateway, not for an always-on mesh |
| Plain OBB/1 on a trusted LAN/VPN | — | lowest | acceptable inside a private network; never expose 9443 to the open internet without transport protection |

### 6.1 WireGuard (recommended)

See [`packaging/examples/wireguard-backbone.conf`](../../packaging/examples/wireguard-backbone.conf)
for a server + two-gateway example. Summary:

1. Put the backbone and gateways in a WireGuard network (e.g. `10.44.0.0/24`).
2. Firewall: allow `51820/udp` publicly; do **not** open `9443` on the public interface.
3. Set `backbone.json` `bind` to the WireGuard address, or leave `0.0.0.0` and restrict 9443 with
   firewalld zone/source rules (`packaging/firewall.md`).
4. Gateways join `openmesh://backbone?host=10.44.0.1&port=9443&token=…`.
5. Keep the panel on loopback; use an SSH tunnel for admin access.

Tailscale is an easier variant: install on backbone and gateways, bind to the tailnet address or
restrict 9443 to the tailnet interface, and use Tailscale ACLs to control who may connect.

### 6.2 Panel HTTPS (Caddy/nginx)

The panel/API is HTTP on `127.0.0.1:8787`. To reach it remotely, put a TLS reverse proxy with
authentication in front of it:

* Caddy: [`packaging/examples/Caddyfile.panel`](../../packaging/examples/Caddyfile.panel)
  (`caddy hash-password`, automatic certificates, WebSocket upgrades handled).
* nginx: [`packaging/examples/nginx-panel.conf`](../../packaging/examples/nginx-panel.conf)
  (`auth_basic`, `proxy_set_header Upgrade`, certbot paths).

Both keep 8787 off the public interface; only 443 is exposed. Prefer SSH port forwarding for
personal use (`ssh -L 8787:127.0.0.1:8787 user@backbone`).

### 6.3 OBB/1 over TLS (nginx stream + stunnel)

Because `openmesh-bridge join` speaks plaintext OBB/1, wrapping the port requires **both** ends:

* server: nginx `stream` listening on `9443 ssl`, proxying to the backbone's plaintext port
  (e.g. `127.0.0.1:9444`) — [`packaging/examples/nginx-stream-tls.conf`](../../packaging/examples/nginx-stream-tls.conf);
  set `backbone.json` `port` to the internal port and firewall only 9443.
* each joining host: `stunnel` client listening on `127.0.0.1:9444` and connecting to
  `mesh.example.org:9443` — [`packaging/examples/stunnel-client.conf`](../../packaging/examples/stunnel-client.conf);
  join `openmesh://backbone?host=127.0.0.1&port=9444&token=…`.

The OBB token still authenticates the protocol end-to-end; TLS protects only the transport. If a
peer cannot run stunnel, use WireGuard/Tailscale instead. Native TLS is a possible future
release; do not assume it until documented in BACKBONE.md.

---

## 7. Multi-region topologies

### 7.1 Hub and spokes

```
        region A                 region B
   mesh A ── gateway A ──▶  backbone (public)  ◀── gateway B ── mesh B
                                   ▲
                          gateway C ── mesh C (region C)
```

One public backbone (or one per region), gateways/repeaters join over IP. Best when regions have
decent internet but no direct radio path. Keep `max_peers` above the number of gateways plus
expected transient peers.

### 7.2 Chained backbones

A backbone can itself be a client of another backbone (`openmesh-bridge join …`), so a private
regional backbone can federate with a public hub:

```
 mesh A ──▶ regional-backbone-A ──join──▶ hub-backbone ──join──▶ regional-backbone-B ◀── mesh B
```

Use this sparingly: every frame crosses each backbone with per-peer fan-out (§3.1), and loops
are absorbed by OMP dedup but still cost bandwidth. For three or more regions prefer a star
(hub) over a ring. Give each backbone a distinct `name` so the peer tables are readable.

### 7.3 Store-and-forward islands

OMP store-and-forward (OMP/1 §10) lets repeaters hold encrypted envelopes for offline nodes for
72 h. A backbone with `store_forward: true` extends that across regions: a message for a node in
another region can be deposited and delivered when its gateway reconnects. Size the data dir for
envelopes (§3.3) and document the promise: **stored ≠ delivered**.

### 7.4 Multi-region checklist

* [ ] DNS/`public_host` set per region; connection strings are region-specific.
* [ ] Tokens per backbone (do not reuse across unrelated communities).
* [ ] Distinct `name` per backbone and per gateway where possible.
* [ ] Store-forward quotas and TTL set per policy.
* [ ] Cross-region latency measured (panel RTT) and acceptable for OMP retry timers.
* [ ] Backup each backbone's identity/config independently; a restored region keeps its node id.

---

## 8. Containers

The image is built from [`packaging/Dockerfile`](../../packaging/Dockerfile); a Compose example
is in [`packaging/docker-compose.yml`](../../packaging/docker-compose.yml). It runs as uid 10001,
keeps state in `/var/lib/openmesh`, builds the web panel into `/usr/share/openmesh/web`, and has
a `curl /api/health` healthcheck.

```bash
cd packaging
docker compose up -d --build
docker compose logs -f backbone
curl -fsS http://127.0.0.1:8787/api/health
```

* **Host networking (Linux, recommended)**: peers show their real IPs; the panel binds
  `127.0.0.1:8787` inside the host namespace. On Docker Desktop use published ports and accept
  that per-IP handshake limits become global.
* **State**: named volume by default. To use a bind mount, `chown -R 10001:10001 data config`
  first (matching the image user) and mount `./data:/var/lib/openmesh`.
* **Config edits**: use the panel (live) or `docker exec` into the container; the named volume is
  the source of truth. Back it up before upgrades.
* **Firewall**: published container ports can bypass ufw/firewalld — host networking avoids that;
  see [`packaging/firewall.md`](../../packaging/firewall.md#docker-caveats).
* **TLS/VPN**: same options as §6; terminate TLS/VPN on the host or a proxy container.

Not built on the OpenMesh dev host (no Docker); the Dockerfile/compose are reviewed recipes.
Report build issues with the Docker version and `docker compose config` output.

---

## 9. Rate limits, quotas and tuning

Defaults are deliberately conservative; tune with measurements, not guesses. Most settings are
live-editable in the panel (and via `POST /api/backbone/config`), and persisted to
`<data-dir>/backbone.json` (0600).

| Knob | Default | What it does | When to change |
| --- | --- | --- | --- |
| `max_peers` | spec default (see panel) | hard cap on concurrent OBB/1 peers | raise only after CPU/bandwidth check; lower to reduce attack surface |
| handshake rate limit | 10/min per remote IP, 30 pending | limits connection churn/brute force | usually leave; VPN deployments make it per-VPN-IP |
| handshake timeout | 10 s | closes half-open connections | leave |
| idle timeout | 90 s without traffic | drops dead peers | leave (clients reconnect with backoff) |
| per-peer queue cap | engine/bridge default | backpressure on slow peers | keep; raising risks memory blow-up |
| `auth` | `token` | token HMAC vs `open` | never `open` on a public port |
| `allowlist` | empty | node-id allowlist in addition to the token | clubs/known peers; combine with token |
| `store_forward` | on for relay-class | retain envelopes for offline peers | disable on a pure relay hub or when disk-bound |
| `ping_interval_s`, `stats_push_s` | 30 s | keepalive/RTT and panel pressure | raise to 60 s on very large peer counts |
| store TTL / capacity | 72 h / 64 envelopes (relay defaults) | offline inbox retention | shrink for privacy/disk; grow for field teams |
| attachment policy | client-side caps (profile) | media size | tune per community; see ADR 0007 |

Signs you should tune: `auth_failures` rising (abuse/token mismatch), peers at cap
(capacity), envelopes near cap (offline population/retention), high disk (retention/attachments),
and throughput saturated (uplink or fan-out). Change one knob at a time and re-measure with §3.4.

---

## 10. Upgrading

1. **Back up** (§5) — at minimum `identity.key`, `backbone.json`, bridge token.
2. Note the current version and peer list (`openmesh-bridge --version`, panel).
3. Upgrade with the installer (idempotent; config/data untouched) or by replacing binaries:
   ```bash
   curl -fsSL https://github.com/martintondel02/openmesh/releases/latest/download/install.sh | sh
   ```
4. Restart the service (`systemctl --user restart openmesh-backbone.service` or the system
   equivalent). With Docker, rebuild/pull and `docker compose up -d`.
5. Verify:
   ```bash
   openmesh-bridge --version
   curl -fsS http://127.0.0.1:8787/api/health
   TOKEN=$(cat ~/.config/openmesh/bridge-token)
   curl -fsS -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8787/api/backbone | jq '{peers, uptime_s: .totals.uptime_s}'
   ```
6. Watch peers reconnect (backoff is automatic) and `auth_failures` stay flat. If something is
   wrong, restore the previous binary (`openmesh-bridge.prev`) and restart; state/config are
   forward-compatible within v0.2.x.

Backbone protocol compatibility: OBB/1 is versioned in the handshake (`obb/1`, `v:1`). v0.2.x
keeps it; a future change will be negotiated, and the release notes will say whether mixed-version
peers work. Keep at least one previous binary on disk for rollback.

---

## 11. Incident checklist

Use this as a runbook; replace paths for system installs (`/var/lib/openmesh`, `/etc/openmesh`,
`sudo journalctl -u ...`).

### 11.1 Suspected token leak

1. Panel → Backbone → **Rotate token**, or `openmesh-cli backbone rotate-token`. The new token is
   shown once; stale peers are disconnected.
2. Distribute the new connection string/QR to legitimate peers out of band.
3. Check `auth_failures` and logs for the leak window; kick unknown peers
   (`openmesh-cli backbone kick <node_id>` or panel).
4. If the **bridge API token** leaked, stop the service, delete
   `~/.config/openmesh/bridge-token`, restart (a new one is generated), and re-paste it into the
   panel. If the **identity key** leaked, treat the node id as compromised: replace the identity
   (`identity.key`) and re-announce — contacts will see a new node id.
5. Review backups for copies of the secret; rotate any copies.

### 11.2 Abuse / flooding peer

1. Identify in the panel peer table (high frames/bytes, low RTT, repeated reconnects) or in
   `/api/backbone/peers`.
2. Kick it (`openmesh-cli backbone kick <node_id>`).
3. If it returns with the same token, rotate the token; add its node id to `allowlist` mode only
   if you know who it is.
4. Firewall-level block its IP as a last resort; note that mobile peers change IPs.
5. Preserve logs for the incident report (no tokens are logged).

### 11.3 auth_failures rising

1. Check whether you rotated the token and not everyone updated (legitimate).
2. Check remote IPs/timing in logs — scanners hammering 9443 vs misconfigured peers.
3. Keep 9443 firewalled to the expected networks/VPN; the handshake rate limit is a backstop, not
   a substitute for firewalling.
4. If a peer is stuck, kick it; clients reconnect with backoff and re-read their token.

### 11.4 Disk full / database errors

1. `df -h` the data dir; `du -sh <data-dir>/*`.
2. Stop the service; remove only what your retention policy allows (old attachments per
   policy, stale backups). Never delete `identity.key`/`backbone.json`.
3. If redb is corrupt, restore from backup (§5); otherwise let it recreate (messages lost,
   identity preserved if `identity.key` kept).
4. Add disk/monitoring limits before restarting.

### 11.5 Crash loop / high CPU

1. `systemctl --user status ...` / `journalctl ... -n 200`.
2. Check memory/disk first; `MemoryMax` may be too low for a load spike.
3. Roll back the last upgrade; restart.
4. If a specific peer/transfer triggers it, capture the peer id and logs, kick the peer, and open
   an issue with version + logs (redact node ids if needed).

### 11.6 Certificate / VPN outage

1. Certificate expiry: renew (certbot/Caddy) and reload the proxy; verify with
   `openssl s_client -connect mesh.example.org:9443 -servername mesh.example.org` (TLS wrapper) or
   `curl -I https://mesh.example.org` (panel).
2. WireGuard/Tailscale down: bring the tunnel up, check `wg show`, then confirm
   `openmesh-cli backbone info` shows peers reconnecting.
3. While VPN is down, peers fall back to their other transports (radio/LAN) if configured; the
   backbone simply reconnects later. Store-and-forward absorbs the gap within retention.

### 11.7 Backbone token lost entirely

1. Restore `backbone.json` from backup, or let the daemon create a new one on next start (new
   token, old clients fail).
2. Use `--backbone-token` / `--backbone-token-file` for automation that cannot read the file.
3. Re-share the connection string from the panel/setup payload.

---

## 12. Command reference

```bash
# Process/service
systemctl --user status openmesh-backbone.service
journalctl --user -u openmesh-backbone.service -f
systemctl --user restart openmesh-backbone.service
sudo systemctl status openmesh-backbone.service         # system install

# API (loopback; token required except /api/health)
TOKEN=$(cat ~/.config/openmesh/bridge-token)
curl -fsS http://127.0.0.1:8787/api/health
curl -fsS -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8787/api/backbone | jq .
curl -fsS -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8787/api/backbone/peers | jq '.[]'
curl -fsS -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8787/api/backbone/setup | jq -r .connection_string

# CLI (reads the same token; --api to point elsewhere)
openmesh-cli backbone info
openmesh-cli backbone peers
openmesh-cli backbone kick <node_id>
openmesh-cli backbone rotate-token
openmesh-cli backbone connect '<openmesh://backbone?...>'
openmesh-cli backbone disconnect <node_id>

# Onboarding output without starting the daemon
openmesh-bridge backbone --data-dir ~/.openmesh --print-setup
```

Secret reminder: on the `backbone` subcommand, `--token-file`/`--token` provision the **local
bridge API token** (panel/REST), not the OBB/1 peer secret. The OBB/1 token lives only in
`<data-dir>/backbone.json` (0600), is printed once on first run / `--print-setup`, and is rotated
with `openmesh-cli backbone rotate-token` or `POST /api/backbone/token/rotate`. Automation that
must pre-provision the OBB secret can use the additive `--backbone-token TOKEN` /
`--backbone-token-file PATH` flags; do not confuse either token with the other.

Note on the setup payload: `GET /api/backbone/setup` uses `listen` for the **OBB/1 listener**
(`bind:port`, e.g. `0.0.0.0:9443`) and `panel_url` for the local API/panel; the connection string
contains the OBB backbone token. Treat the whole payload as secret and share only the connection
string, over a trusted channel.


---

## 13. Security checklist (operational)

* [ ] Backbone runs unprivileged (systemd user/service unit or container uid 10001).
* [ ] `backbone.json`, `identity.key`, token files are `0600`; data dir `0700`.
* [ ] Only 9443/tcp (or VPN port) is public; 8787 is loopback-only.
* [ ] Panel behind SSH tunnel or TLS proxy with authentication; never directly exposed.
* [ ] VPN/TLS transport protection on untrusted networks; native OBB/1 TLS not assumed.
* [ ] Token rotated on staff/peer departure and after any suspected exposure.
* [ ] Open mode disabled on public ports (panel shows a red warning if enabled).
* [ ] Backups encrypted, tested, and stored separately from the host.
* [ ] Upgrades backed up first; previous binary kept for rollback.
* [ ] Host patched, NTP enabled, disk/health monitoring wired to an alert you actually read.
* [ ] Log retention and privacy policy: backbone logs contain metadata (node ids, IPs, timings),
      not message contents or tokens.

---

## 14. See also

* [INSTALL.md](../INSTALL.md) — installation, first run, panel access, firewall basics.
* [BACKBONE.md](../architecture/BACKBONE.md) — OBB/1 framing/handshake, config, API, security model.
* [WEBSOCKET-API.md](../architecture/WEBSOCKET-API.md) §8 — backbone REST/WS surface.
* [ADR 0009](../adr/0009-backbone-mode.md) — why authenticated TCP + panel, token-vs-TLS trade-offs.
* Packaging: [`packaging/README.md`](../../packaging/README.md), `packaging/systemd/`,
  `packaging/docker-compose.yml`, `packaging/examples/`, `packaging/firewall.md`.

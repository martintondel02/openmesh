# Advanced cookbook

Practical recipes for people who already have a working mesh and want to tune, automate or scale
it. Every recipe lists **goal**, **when to use it**, the **commands/config**, how to **verify**, and
the **rollback**. Assumes OpenMesh v0.3.0, a Unix-like host and a bridge running on the default
`http://127.0.0.1:8787` unless stated.

> Conventions: `TOKEN=$(cat ~/.config/openmesh/bridge-token)`; API calls use
> `curl -fsS -H "Authorization: Bearer $TOKEN"`. All settings can also be changed in the panel or
> via `POST/PATCH /api/config`; the CLI/MCP paths are shown where they are more convenient.

---

## 1. Tune the radio profile and power

**Goal:** trade range against speed and airtime deliberately.
**When:** nodes hear each other but links are marginal, or media is too slow for the profile.

Profile/region must match across the whole group, including repeaters. Region is a legal choice
(see [radios, regions and airtime](../../content/help/radio-and-region.md)); do not raise power
beyond local limits.

```sh
# Look at what is configured now
curl -fsS -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8787/api/config | jq '.radio'

# Change profile (live) — example: LONG_FAST for rural range
curl -fsS -X PATCH -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"radio":{"profile":"LONG_FAST"}}' http://127.0.0.1:8787/api/config | jq '.radio'

# CLI alternative (any bridge API)
openmesh-cli --json stats
openmesh-cli airtime --profile LONG_FAST --bytes 240 --region EU868
```

Verify: Nodes screen shows nodes again (they must be updated to the same profile); `stats` shows
frame counts; `airtime` shows the new cost. Roll back with the previous profile value.

Notes:
- Slower profiles (higher SF) increase airtime ~2–4× per step; see the profile table in the radio
  help topic.
- Power is not a substitute for antenna height; a 14 dBm node on a roof usually beats 20 dBm in a
  valley.
- Changing profile disconnects nodes using the old one. Announce the change; update repeaters
  first if they are the only path.

---

## 2. Run a backbone behind WireGuard with systemd

**Goal:** link two meshes over the internet without exposing OBB/1 to the public internet.
**When:** hostile networks, or you want network-level access control in addition to the token.

**Server (public VPS):**

```bash
# 1. WireGuard: server 10.44.0.1/24, peers 10.44.0.2, .3 (see packaging/examples/wireguard-backbone.conf)
sudo apt install wireguard && sudo systemctl enable --now wg-quick@wg0

# 2. Install OpenMesh (release installer or source build)
curl -fsSL https://github.com/martintondel02/openmesh/releases/latest/download/install.sh | sh

# 3. Bind the OBB listener to the VPN address, keep the API on loopback
openmesh-bridge backbone --bind 10.44.0.1 --port 9443 --data-dir ~/.openmesh \
  --listen 127.0.0.1:8787 --serve-web ~/.local/share/openmesh/web

# 4. Persist it (writes a hardened systemd user unit; review ExecStart)
openmesh-bridge install-service --backbone --port 9443 --data-dir ~/.openmesh
systemctl --user daemon-reload && systemctl --user enable --now openmesh-backbone.service
sudo loginctl enable-linger "$USER"

# 5. Firewall: only WireGuard
sudo ufw allow 51820/udp
# do NOT open 9443 publicly
```

**Gateway (joining side):**

```bash
openmesh-bridge join 'openmesh://backbone?host=10.44.0.1&port=9443&token=…&v=1' \
  --data-dir ~/.openmesh
openmesh-cli backbone info        # mode: client, peers: 1
```

Verify on the server: `openmesh-cli backbone peers` shows the gateway with direction, RTT and
counters; `/api/health` is green; `ss -ltnp | grep 9443` shows the VPN bind address. Rollback:
`systemctl --user disable --now openmesh-backbone.service` and restore `backbone.json` if you
changed it.

Notes:
- `--token-file` on `backbone` is the **bridge API token**, not the OBB secret. The OBB token
  lives in `~/.openmesh/backbone.json` and is printed once on first run; rotate with
  `openmesh-cli backbone rotate-token`.
- If the gateway cannot install WireGuard, TLS-wrap the port with nginx `stream` +
  `stunnel` (see `packaging/examples/`). OBB/1 itself is plaintext in v0.3.0.
- `max_peers`, `allowlist` and `--open` are in the backbone help topic; never leave `--open` on a
  public port.

---

## 3. Create channels with your own keys

**Goal:** deterministic/controlled channel identity, or re-creating a channel after a member
leaves.
**When:** team channels with a key you manage, or rotating a channel for forward secrecy.

A channel id is derived from the 32-byte PSK; the same key reproduces the same channel.

```sh
# 32 random bytes, base64url (no padding) — generate on a trusted machine
PSK=$(python3 -c 'import os,base64; print(base64.urlsafe_b64encode(os.urandom(32)).decode().rstrip("="))')

# Create a channel with that key; returns psk_once + invite_url exactly once
curl -fsS -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d "{\"name\":\"team-alpha\",\"psk\":\"$PSK\"}" http://127.0.0.1:8787/api/channels | jq .

# Join from another bridge with the invite URL or name+psk
curl -fsS -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"url":"openmesh://channel?name=team-alpha&id=…&psk=…"}' \
  http://127.0.0.1:8787/api/channels/join | jq .
```

Security: the invite contains the key; share it out of band. Rotating means creating a **new**
channel with a new key and re-inviting; there is no in-place rekey (silent rekey without
verification would be worse). `GET /api/channels` never returns PSKs.

Verify: both bridges list the same `channel_id`; a message appears on both. Rollback: leave the
channel on each node (UI) or restore the previous store from backup.

---

## 4. Script the REST + WebSocket API

**Goal:** dashboards, bots, exporters, test harnesses.
**When:** you need automation beyond the CLI/MCP, or you are building a client.

The full contract is `docs/api/openapi.json` (also served at `/api/openapi.json`). Auth is a bearer
token; the API is loopback-only.

```sh
TOKEN=$(cat ~/.config/openmesh/bridge-token)
H=(-H "Authorization: Bearer $TOKEN")

# Snapshot: status, nodes, channels, recent direct messages
curl -fsS "${H[@]}" http://127.0.0.1:8787/api/status | jq .
curl -fsS "${H[@]}" http://127.0.0.1:8787/api/nodes | jq '.nodes[] | {node_id,name,class,hops,rssi}'
curl -fsS "${H[@]}" http://127.0.0.1:8787/api/channels | jq '.channels[]'
curl -fsS "${H[@]}" 'http://127.0.0.1:8787/api/messages?limit=20' | jq '.messages[]'

# Send a direct message (recipient must be a contact/known node)
curl -fsS -X POST "${H[@]}" -H 'Content-Type: application/json' \
  -d '{"to":"aabbccdd00112233","text":"hello from a script"}' \
  http://127.0.0.1:8787/api/messages | jq .
```

Live events via WebSocket (`ws://127.0.0.1:8787/api/ws?token=$TOKEN`) or SSE
(`curl -N "${H[@]}" http://127.0.0.1:8787/api/events`). Envelope:
`{"v":1,"type":"…","id?":"…","data":{…}}`; commands echo `id`; errors use
`{"type":"error","data":{"code","message"}}`.

Attribution for automated writes:

```sh
curl -fsS -X POST -H "Authorization: Bearer $TOKEN" -H "X-OpenMesh-Agent: my-exporter" \
  -H 'Content-Type: application/json' -d '{"text":"…"}' http://127.0.0.1:8787/api/announce
curl -fsS -H "Authorization: Bearer $TOKEN" 'http://127.0.0.1:8787/api/activity?limit=20' | jq
```

Operational notes: handle 401 (rotate/re-read token), 409/503 with backoff, 400 with a fix (do not
retry blindly); paginate with `before_ms`; keep secrets out of logs; reconnect with jitter and
deduplicate events by message id after reconnect.

---

## 5. MCP agent automations

**Goal:** have Claude/Cursor/DSH/any MCP client operate the node safely.
**When:** recurring summaries, guided setup, backup checks, backbone operations.

```sh
# 1. Generate the client config (absolute paths; token read from file)
openmesh-cli agent config --client generic

# 2. Start read-only, and give the agent a scoped name
OPENMESH_MCP_READ_ONLY=1 OPENMESH_AGENT_NAME=nightly openmesh-bridge mcp
```

In the client, select the `manage_my_mesh` prompt for daily operations, or copy a task prompt from
[the prompt pack](../agents/prompts.md). Useful MCP tools: `openmesh_status`, `openmesh_nodes`,
`openmesh_activity`, `openmesh_help`; writes (`openmesh_send_message`, `openmesh_set_config`,
`openmesh_setup_quick`, backbone tools) are only exposed when read-only is off, and destructive
ones require `confirm: true`.

Verify attribution: after a write, `GET /api/activity` shows `source: "agent:<name>"` with a summary
(never bodies/tokens). Revoke by rotating the bridge token (stop bridge → delete `bridge-token` →
restart → paste new token into clients).

Safety: loopback only; separate agent identity; review activity; never paste tokens/PSKs into a
prompt; do not let an agent change radio/backbone settings without a human decision.

---

## 6. Backups and migration

**Goal:** survive disk loss, move to a new host, or clone a node (carefully).
**When:** before upgrades, before identity changes, and as a routine.

What to back up:

* `identity.key` — your identity; losing it changes your node id (no recovery).
* `backbone.json` — OBB config + token (if backbone mode).
* bridge API token (`~/.config/openmesh/bridge-token`).
* `openmesh.redb` + `attachments/` — history and media.

Consistent copy:

```bash
systemctl --user stop openmesh-backbone.service   # or the plain bridge service
tar -C "$HOME" -czf "openmesh-$(date +%F).tar.gz" .openmesh .config/openmesh
systemctl --user start openmesh-backbone.service
gpg -c openmesh-*.tar.gz && shred -u openmesh-*.tar.gz   # encrypt at rest
```

Restore:

```bash
systemctl --user stop openmesh-backbone.service
tar -C "$HOME" -xzf openmesh-YYYY-MM-DD.tar.gz
chmod 600 ~/.openmesh/identity.key ~/.openmesh/backbone.json ~/.config/openmesh/bridge-token
chmod 700 ~/.openmesh ~/.config/openmesh
systemctl --user start openmesh-backbone.service
curl -fsS http://127.0.0.1:8787/api/health
```

Verify the node id is unchanged, contacts/channels are present, backbone peers reconnect (peers
need the same connection string/QR; if the hostname/IP changed, update `public_host`).

Do **not** run two bridges with the same `identity.key` at the same time (sequence numbers and
dedup assume one active node per identity), and do not copy the API token to an untrusted machine.
Test a restore before you need it.

---

## 7. Multi-node deployment (repeaters, gateways, firmware)

**Goal:** cover an area with a small fleet and predictable roles.
**When:** community/campus/valley deployment, or extending range beyond one link.

Plan:

1. **One region and one profile for the whole site.** Changing profile later means re-flashing
   repeaters, so decide early (rural: `LONG_FAST`; mixed: `MEDIUM_FAST`; dense/LOS: `SHORT_FAST`).
2. **Repeaters on high ground with power.** A repeater should hear at least two other nodes.
   Solar nodes need a battery buffer; check telemetry after a week.
3. **Gateways where there is internet.** A gateway bridges radio↔IP (and can join a backbone).
   Keep the API loopback-only on every host.
4. **Trackers last.** They are battery-sensitive and mostly beacon; do not rely on them for
   bidirectional traffic.
5. **Flashing:** `tools/flash/openmesh-flash --list`, then `--board <name> --port <serial>` (or the
   browser flasher). Firmware builds per board are in CI; see [boards](../../docs/hardware/boards.md).

Verify after a rollout: every node sees the repeater with stable hops; `openmesh-cli nodes` from a
gateway shows the expected topology; duty-cycle and battery telemetry are within budget; a
message from the far edge reaches a gateway.

Rollback: keep the previous firmware/`.uf2` and config values; revert one node at a time and watch
Nodes after each change.

---

## 8. Reproducible demo/CI harness

**Goal:** test clients, docs and demos without radios.
**When:** CI, screenshots, new contributors, agent evaluations.

```sh
# Deterministic virtual mesh with loss/latency knobs
openmesh-bridge --sim --sim-loss 20 --sim-latency 150 --sim-seed 42 --listen 127.0.0.1:8787

# Seeded demo data (web screenshots, onboarding)
openmesh-bridge --demo

# Full simulator scenarios (two-node chat, relay chain, lossy ARQ, store-forward, image)
cargo run -p openmesh-sim -- --profile ci

# Web client standalone demo (no backend)
pnpm --dir apps/web dev    # http://localhost:5173/?demo=1
```

Use a throwaway `--data-dir /tmp/mesh-…` for tests; never point tests at a real data dir. Verify
expected frames/messages in the simulator report, and API responses against the OpenAPI schemas.

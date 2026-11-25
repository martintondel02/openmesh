# Troubleshooting guide

A symptom → check → fix playbook for OpenMesh v0.3.0. Start at the top and work down; most mesh
problems are one of: bridge down, wrong token, radio/region/profile mismatch, out of range, or a
message too expensive for the link.

Related: [advanced cookbook](advanced-cookbook.md) · in-app help topics
[troubleshooting](../../content/help/troubleshooting.md), [radio & region](../../content/help/radio-and-region.md),
[messaging](../../content/help/messaging.md), [backbone](../../content/help/backbone.md),
[agents](../../content/help/agents.md).

---

## 1. First five commands (always run these)

```sh
openmesh-bridge --version
curl -fsS http://127.0.0.1:8787/api/health
TOKEN=$(cat ~/.config/openmesh/bridge-token)
curl -fsS -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8787/api/status | jq .
openmesh-cli stats
```

What they tell you:

* bridge version and whether the daemon is alive;
* whether the API is reachable and the token is correct;
* radio connected, region/profile, airtime, queues, mesh counters;
* the same radio/mesh view from the CLI (works when the browser does not).

If `/api/health` fails, the problem is the bridge process or its port, not the mesh. Check the
terminal output / `journalctl` (see §6) before changing any radio setting.

---

## 2. Bridge and API problems

| Symptom | Check | Fix |
| --- | --- | --- |
| `openmesh-bridge: command not found` | Is `~/.local/bin` on `PATH`? `command -v openmesh-bridge` | `export PATH="$HOME/.local/bin:$PATH"`; re-open the shell. |
| `connection refused` from the browser/curl | Is the bridge running? Which port? `ss -ltnp \| grep 8787` | Start it; check `--listen`; another instance may hold the port. |
| Port already in use | `ss -ltnp \| grep 8787` | Stop the other bridge or use `--listen 127.0.0.1:8788` and update the panel URL. |
| Browser loads but panel says 401 | Token missing/wrong, or rotated | Paste `~/.config/openmesh/bridge-token` into Settings → Bridge; regenerate only if compromised. |
| Browser page is blank | Web bundle missing or `--serve-web` wrong | Point `--serve-web` at the directory containing `index.html`; reinstall the web release asset. |
| API works locally but not from another machine | By design: API is loopback-only | Use an SSH tunnel (`ssh -L 8787:127.0.0.1:8787 user@host`) or a TLS reverse proxy with auth. Do not bind publicly. |
| High CPU/RAM | Large store, media, many peers, debug logging | Check `/api/status`, disk, log level; trim retention/attachments; restart with `--log info`. |
| Disk full / store errors | `df -h`, `du -sh <data-dir>` | Free space, prune attachments/old logs per policy; never delete `identity.key`. |
| Bridge crashes on start | Config/data permissions or corrupt DB | Check logs; verify data dir 0700 and files 0600; restore from backup if the DB is corrupt. |

---

## 3. Radio and link problems

| Symptom | Check | Fix |
| --- | --- | --- |
| No radio listed | Cable, power, permissions (`dialout`), serial feature, port path | Re-seat cable; add user to `dialout`; use a `--features real-serial` build; `--serial /dev/ttyACM0`. |
| Nodes list is empty | Same region/profile on all nodes? Any shared transport (radio/LAN/backbone)? | Match region/profile everywhere; check antennas; move closer/outdoors. |
| Nodes appear but messages fail | Hops/TTL, marginal RSSI/SNR, profile mismatch on a relay | Check hop counts and SNR; raise TTL for deep chains; fix the relay's config. |
| Only one direction works | Asymmetric marginal link or a one-way relay | Reposition antennas/nodes; add a repeater between the two. |
| Range suddenly worse | Antenna/cable, interference, new obstructions, profile change | Compare RSSI/SNR history; revert the last change; improve antenna height. |
| Repeater keeps disappearing | Power/battery, solar budget, thermal, firmware crash | Check telemetry/voltage; move to a better power source; reflash known-good firmware. |
| Duty-cycle warnings | Region limit reached by media/announces | Reduce media size/frequency, lower announce rate (advanced), wait for the window. |
| `radio_busy` errors | A large transfer is in flight | Wait for the transfer; avoid simultaneous media; check queue. |

Radio facts to sanity-check against (model estimates, see PROTOCOL.md): one 240-byte frame is
~0.61 s at `MEDIUM_FAST`, ~2.0 s at `LONG_FAST`, ~13.5 s at `LONG_SLOW`; a 5 KiB image is ~18 s at
`MEDIUM_FAST`; client TTL is 5 hops, announce TTL 8.

---

## 4. Message delivery problems

| Symptom | Check | Fix |
| --- | --- | --- |
| Single tick forever | Recipient online? Any store-capable relay in range? | Wait, retry, move closer; add a `store` repeater. |
| Fails after several retries | TTL exhausted, recipient offline with no store, profile mismatch | Raise TTL (advanced), fix config, wait for the recipient. |
| Duplicates | Sender retried; dedup window expired or bridge restarted | Harmless; avoid manual + automatic retry at the same time. |
| Old messages missing | Retention/expiry, new data dir, restore mismatch | Check Settings → Storage and the data dir in use; restore from backup if needed. |
| Read receipts missing | Recipient disabled them, or ACK lost | Not an error; delivery can still have succeeded. |
| Channel messages missing for one member | Different channel id/key, profile mismatch, not actually joined | Compare the invite/channel id; re-share the invite; check region/profile. |
| Media stalls | Out of range mid-transfer, lost chunks, airtime cap | Retry when the peer is reachable; use smaller media; check the profile cap. |

Delivery model: `ACK_TIMEOUT_MS` = 8 s, up to 3 retries; store-and-forward retains ciphertext for
72 h (default), 64 envelopes; relays may evict oldest when full.

---

## 5. Backbone problems

| Symptom | Check | Fix |
| --- | --- | --- |
| Peers cannot connect | Public port/firewall/security group, DNS, `--bind`, VPN up | `nc -vz host 9443`; open 9443/tcp; check `wg show`/tailscale status; verify bind address. |
| `auth_failed` | Wrong/rotated OBB token, allowlist, max peers | Re-copy the connection string/QR; check allowlist/max_peers; rotate only intentionally. |
| Panel unreachable | API loopback-only, token missing | SSH tunnel to 8787; paste the bridge token. |
| Service dies on logout | systemd user manager stops without linger | `sudo loginctl enable-linger "$USER"`. |
| Peers connect, no traffic | Joined mesh on a different profile/region, or TTL too small | Match profiles/regions; raise TTL at the edge if needed. |
| `rtt_ms` is null | First ping not yet exchanged (30 s default) | Wait one ping interval; not an error. |
| Backbone token lost | `backbone.json` deleted/corrupt | Restore backup or let a new one be generated and re-share the connection string; identity is preserved. |
| High bandwidth | Media fan-out, many peers, retry storms | Tune `max_peers`, store-forward, media caps; inspect per-peer counters. |

`--token-file` on `backbone` sets the **bridge API token**; the OBB secret is only in
`<data-dir>/backbone.json`. See [backbone.md](../../content/help/backbone.md) and
[BACKBONE.md](../architecture/BACKBONE.md).

---

## 6. Where are the logs?

```sh
# Running in a terminal
openmesh-bridge --log debug          # verbose; tokens are never logged

# systemd user service (backbone or bridge)
journalctl --user -u openmesh-backbone.service -f
journalctl --user -u openmesh-backbone.service --since '1 hour ago' | grep -iE 'error|warn|auth'

# system service
sudo journalctl -u openmesh-backbone.service -f
```

Log levels: `info` (default) is enough for operations; `debug` for reproducing a bug. Keep log
retention bounded (journald `SystemMaxUse`), and redact node ids/IPs before pasting logs publicly.

---

## 7. Agent and MCP problems

| Symptom | Check | Fix |
| --- | --- | --- |
| Client cannot start the server | Absolute `command` path, args `["mcp"]`, binary executable | Use `openmesh-cli agent config --client <name>` output; test the path manually. |
| 401 from every tool | Token rotated/moved, `OPENMESH_TOKEN*` wrong | Update the env/config, restart the client, paste the new panel token. |
| Write tools missing | `OPENMESH_MCP_READ_ONLY=1` | Remove/set `0`, restart the MCP client. |
| `confirm_required` | Destructive tool without confirmation | Re-issue with `"confirm": true` after a human agrees. |
| Timeouts | Bridge/radio busy; 10 s HTTP timeout | Check `/api/status`; long media returns a queued result instead. |
| Actions missing from Activity | Reads are not logged; missing `X-OpenMesh-Agent` | Set `OPENMESH_AGENT_NAME`; check `GET /api/activity`. |
| `initialize` never returns | stdout polluted by a wrapper, or bridge unreachable | Run `openmesh-bridge mcp` directly in a terminal to see stderr. |

Full reference: [docs/agents/mcp.md](../agents/mcp.md).

---

## 8. Installer and upgrade problems

| Symptom | Fix |
| --- | --- |
| `checksum mismatch` | Corrupt download/mirror — re-download; never bypass verification. |
| `no official build for …` | Build from source (e.g. Linux ARM, Intel macOS) or use a supported host. |
| `--system` asks for sudo in CI | Re-run with `--yes` as a sudo-capable user, or install per-user. |
| Service unit path not found after moving dirs | Re-run the installer or reinstate the unit with the new paths (`openmesh-bridge install-service --backbone …`). |
| Upgrade changed nothing | Wrong version pinned/old binary on `PATH`; check `openmesh-bridge --version` and `command -v`. |
| Need to roll back | Keep the previous binary; restore it, restart, verify `/api/health` and peers; or restore the data backup. |

The installer is idempotent and never overwrites config/tokens/identity; it verifies SHA-256 before
installing. See [INSTALL.md](../INSTALL.md).

---

## 9. Bug-report template

Copy this into the issue (redact tokens, keys, PSKs, IPs if sensitive):

```text
OpenMesh version: <openmesh-bridge --version>
OS/arch: <uname -a>
How run: demo | sim | radio | backbone | container
Radio/board: <model, USB/serial>
Steps to reproduce: 1. … 2. … 3. …
Expected: …
Actual: …
/api/health: <output>
/api/status: <jq . | redact>
Relevant logs (last 50 lines, redacted): <journalctl …>
What changed before it broke: <upgrade/config/antenna/…>
```

Please include `--log debug` output for reproducible bugs, and state which checks you already ran
from this guide so the maintainers do not repeat them.

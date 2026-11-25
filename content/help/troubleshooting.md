# When something is broken

Most mesh problems are one of five things: no radio, wrong region/profile, out of range, a token
or permission problem, or a message that is too big for the link. This playbook walks from the
cheapest check to the most invasive, with the exact commands.

## What it is

A diagnostic order that works for beginners and advanced users:

1. Is the bridge alive?
2. Is the radio detected and configured?
3. Are nodes visible (and are they the right nodes)?
4. Are messages reaching the next hop, and are ACKs coming back?
5. Is a specific feature (media, backbone, agent) failing for its own reason?
6. Where are the logs, and what should I paste into a bug report?

## Why it matters

Randomly changing settings makes things worse and hides the original cause. Following the same
order every time means you can tell whether a change helped, and lets someone else reproduce your
problem from the diagnostics you collect.

## Quick start

### 1. Bridge alive?

```sh
curl -fsS http://127.0.0.1:8787/api/health
```

Expected: `{"ok":true,"version":"0.3.0"}`. If this fails, the bridge is not running or is on a
different port. Start it, check the terminal, and look at the logs (§6).

### 2. Radio detected?

* Open **Settings → Radio**: is a radio listed and marked connected? Are region and profile set?
* CLI: `openmesh-cli stats` shows radio state, airtime and queue; `openmesh-cli airtime` estimates
  a frame on your profile.
* Demo/sim users: `--demo` and `--sim` have no physical radio; "no radio" is expected there.

### 3. Nodes visible?

* Open **Nodes** and sort by last seen. If nobody is there, the problem is link/profile/region, not
  the app.
* Check every node and repeater uses the **same region and profile** (Radio and region topic).
* Move one node closer or outdoors; RSSI around `-120 dBm` is marginal, SNR below `-15 dB` is
  often undecodable.

### 4. Messages arriving?

* Single tick = transmitted, no ACK. Double tick = delivered.
* Recipient offline? Store-and-forward needs a repeater with storage in range (Messages topic).
* TTL: client frames default to 5 hops. A chain longer than that silently drops.
* Retry from the message menu; then check the recipient's app is running and on the same channel
  or with your contact saved.

### 5. Feature-specific

| Feature | First check |
| --- | --- |
| Images/voice | Profile cap and airtime estimate; is the transfer progressing? |
| Backbone | `openmesh-cli backbone info`; firewall 9443/tcp; peer auth errors in logs. |
| Agents | `curl` health with the token; `OPENMESH_MCP_READ_ONLY`; MCP path absolute. |
| Map | Location permission/fix, privacy setting, cached tiles. |

### 6. Logs and diagnostics

```sh
openmesh-bridge --log debug            # verbose terminal logs (or RUST_LOG=debug)
journalctl --user -u openmesh-backbone.service -f   # if installed as a service
openmesh-cli stats --json              # machine-readable radio/mesh state
openmesh-cli nodes                     # what the bridge knows
openmesh-cli channels                  # joined channels
```

For a bug report include: `openmesh-bridge --version`, `uname -a`, how you run the bridge (demo,
sim, real radio), the health/status output, relevant log lines with tokens/node ids redacted, and
the exact steps that reproduce the problem.

## Advanced notes

### Common root causes, ranked

1. **Profile/region mismatch** — nodes hear each other but reject frames at the modem level; the
   Nodes list stays empty. Fix: one profile/region for the whole group.
2. **No store-capable relay** — offline messages expire. Fix: add a repeater with `store` caps.
3. **Duty-cycle saturation** — media eats the hourly budget; new sends queue. Fix: text-first,
   smaller media, or wait for the window.
4. **Wrong token** — 401 from panel/CLI/agent. Fix: paste `~/.config/openmesh/bridge-token`.
5. **API exposed or blocked** — remote panel fails because it is loopback (by design) or a proxy
   misconfigured. Fix: SSH tunnel/TLS proxy, do not bind publicly.
6. **Radio USB/driver problems** — device not detected. Fix: cable, permissions (`dialout`), serial
   feature enabled, port path.
7. **Old binary/web bundle mismatch** — UI calls endpoints the bridge does not have. Fix: reinstall
   both from the same release.

### Useful one-liners

```sh
# Is anything listening?
ss -ltnp | grep -E '8787|9443'

# Does the API answer with the token?
curl -fsS -H "Authorization: Bearer $(cat ~/.config/openmesh/bridge-token)" \
  http://127.0.0.1:8787/api/status

# Backbone health and peers
openmesh-cli backbone info
openmesh-cli backbone peers

# Airtime estimate for 100 bytes on the configured profile
openmesh-cli airtime --bytes 100
```

### Reset paths (increasing severity)

1. Restart the bridge (`Ctrl-C`, start again); does the problem persist?
2. Restart the radio (unplug/replug, or power-cycle the board).
3. Re-run the installer to repair binaries/web bundle (data is untouched).
4. Restore `identity.key`/database from backup (last resort; back up first).
5. Start a fresh data dir to prove whether the problem is data-related
   (`--data-dir /tmp/test-mesh`), then migrate deliberately.

## Troubleshooting

| Symptom | Most likely cause | First fix |
| --- | --- | --- |
| `/api/health` fails | bridge not running / wrong port | start it; check `--listen` and logs |
| Panel asks for token | missing/wrong API token | paste `~/.config/openmesh/bridge-token` |
| No radio listed | USB/driver/permissions/feature | check cable, `dialout`, `real-serial` build |
| Nodes list empty | region/profile mismatch or no power | match settings; check antennas/power |
| One-way messages | marginal link, asymmetric path | reposition, add repeater, retry |
| Messages stuck at one tick | recipient offline, no store relay, TTL | wait/add store relay/raise TTL/retry |
| Media fails | cap/duty-cycle/airtime | smaller media, faster profile, wait |
| Backbone peers 0 | firewall, wrong token, listener | open 9443; check `auth_failed`; re-copy invite |
| Agent 401 | token rotated/moved | update agent env/config, restart agent |
| High CPU/disk | large store, media, logs | retention settings, rotate logs, back up first |

## Related topics

[Radios, regions and airtime](radio-and-region.md) · [Messages, delivery and offline mail](messaging.md) ·
[Run a backbone](backbone.md) · [Use your AI agent](agents.md) · [Glossary](glossary.md)

# Prompt pack for OpenMesh agents

Copy-paste prompts for the jobs people actually ask an AI agent to do with OpenMesh. Each entry
says what the prompt does, when to use it, and what to watch out for. All prompts assume the MCP
server is connected (see [mcp.md](mcp.md)); the agent can also work from pasted diagnostics if MCP
is not available.

**How to use this pack**

1. Start with the agent in **read-only mode** (`OPENMESH_MCP_READ_ONLY=1`) and let it read
   `openmesh_status`, `openmesh_setup_state` and the relevant help topic.
2. Grant writes only for the specific task, and tell the agent which writes it may perform.
3. Review `openmesh_activity` afterwards and revoke access (rotate the API token) when done.

Prompts are written for a capable general-purpose agent. They assume the agent will call MCP
tools; if it cannot, it should ask you for the equivalent command output and say what it needs.

---

## 1. Troubleshoot a broken mesh

**What it does:** walks the diagnostic order (health → radio → nodes → messages → airtime) and
ends with a ranked set of causes and one next action. It should not change settings without
asking.

**When to use:** "nobody can hear me", "messages don't arrive", "the map is empty", "the radio
disappeared".

**Safety:** read-only. The agent may propose config changes; approve them one at a time and
re-check. Do not let it rotate the token, reinstall, or delete data without a backup.

```text
You are diagnosing an OpenMesh v0.3.0 mesh. Work read-only unless I explicitly approve a change.
Use the MCP tools: openmesh_health, openmesh_status, openmesh_nodes, openmesh_messages,
openmesh_channels, openmesh_help (topics: troubleshooting, radio-and-region, messaging,
nodes-telemetry). Also call openmesh_activity to see whether an agent or config change preceded
the problem.

Follow this order and report what you find at each step:
1. Health: does the bridge answer, and what version is it?
2. Radio: is a radio connected, what region/profile, airtime/queue/duty cycle?
3. Nodes: which nodes are visible, last seen, hops, RSSI/SNR? Are repeaters present?
4. Messages: check queue/delivery state for a recent failed message; is the recipient a contact?
5. Airtime: is the profile too slow/fast, is the duty-cycle budget exhausted, are media transfers
   stuck?
6. Compare with the relevant help topic and the expected values.

Then give me: (a) the most likely cause ranked 1–3 with evidence, (b) the single cheapest next
action, (c) any setting change you recommend with its exact current and proposed value, and
(d) what to check after the change. Do not apply changes yet.
```

---

## 2. Set up the mesh for my family

**What it does:** guides a non-technical setup — role, country/region, secure defaults, a private
channel and a shareable invite — using the server-owned preset so the web UI, CLI and agent agree.

**When to use:** a new household/camp/family deployment, or when someone asks "how do I get my
family started?".

**Safety:** write tools needed (`openmesh_setup_quick`). Confirm the country/region before applying
(it drives legal radio settings), and share the invite over a trusted channel, not publicly.

```text
Help me set up OpenMesh for my family. Use the MCP tools openmesh_setup_state,
openmesh_setup_recommend (via openmesh_setup_state/recommended or openmesh_help), openmesh_setup_quick,
openmesh_channels and openmesh_help topics getting-started, first-mesh, channels, security.

1. Ask me exactly three things: (a) role (client for a phone/computer, repeater for a roof box,
   gateway for a box connected to the internet, tracker), (b) my country, (c) a friendly node name.
2. Call openmesh_setup_state and show me the recommended region/profile and the current checklist.
3. Explain in one plain sentence per choice what the preset will change (region, profile, privacy,
   announce interval, private channel). Flag if my country is unknown to the preset.
4. After I confirm, call openmesh_setup_quick with those values, create_channel=true and
   privacy="private". Show the returned checklist and the channel invite.
5. Give me a short, non-technical invitation message I can send to family members, plus the step
   they need to do to join.
6. End with the three most likely beginner problems (no radio, wrong region, out of range) and the
   help topics to read. Do not change anything else, and do not enable backbone or open mode.
```

---

## 3. Back up and migrate a node

**What it does:** produces a safe, verifiable backup plan for identity/config/history, checks what
is essential, and walks a migration without duplicating an identity.

**When to use:** new device, VPS migration, before an upgrade, or "I don't want to lose my mesh".

**Safety:** read-only for inspection; file copies happen on the host, not through MCP. Never send
`identity.key`, `backbone.json` or the API token to an agent/chat. Stop the bridge (or snapshot the
data dir) before copying the database.

```text
I need to back up and possibly migrate my OpenMesh node. Work read-only; never ask me to paste
secrets into this chat. Use openmesh_status, openmesh_config (openmesh_help topic security) and
openmesh_backbone_status if backbone mode is on.

1. Tell me exactly which files matter on this host and where they live by default:
   identity.key, backbone.json, bridge-token, the redb database and attachments (and what each is
   used for). Use the help topics security, install and backbone for the authoritative paths.
2. Give me a consistent backup procedure (stop the service or snapshot the data dir first; mode
   0600 for secrets; encrypt the backup; test a restore).
3. Give me a restore/migration checklist: stop bridge, restore files, permissions, start, verify
   node id is unchanged, verify peers/contacts/channels, then clean up.
4. List the failure modes: restoring only part of the state, running two bridges with the same
   identity simultaneously, losing the backbone token (peers must reconnect), and what to do in
   each case.
5. Tell me what to verify after the migration (node id, contact list, channel count, backbone peer
   count).
Do not read or echo the contents of secret files; check existence/permissions only if I ask.
```

---

## 4. Run a backbone on a VPS

**What it does:** produces a deployment plan for OBB/1 backbone mode: config, firewall, systemd,
token handling, join instructions, monitoring and hardening.

**When to use:** connecting two distant meshes, a community hub, or replacing an unreliable
gateway.

**Safety:** write tools may configure the local bridge, but firewall/systemd/VPS changes are
host operations the agent cannot and should not perform for you. Confirm the public port and the
token distribution path; use WireGuard/Tailscale or TLS wrapping on hostile networks.

```text
I want to run an OpenMesh v0.3.0 backbone on a small VPS. Use openmesh_help topics backbone,
security, troubleshooting; openmesh_backbone_status / openmesh_backbone_setup if a backbone is
already configured; openmesh_setup_state for the local node.

1. Produce a step-by-step plan: install, `openmesh-bridge backbone` invocation (data dir, API
   listen loopback, optional serve-web), first-run token output, `--print-setup`, and
   `install-service --backbone`.
2. Give firewall rules for firewalld and ufw, and the cloud security-group equivalent. Only the
   OBB port (default 9443/tcp) is public; the API (8787) stays loopback or behind SSH/TLS.
3. Explain the two tokens clearly: the OBB backbone token (backbone.json, printed once, used by
   join/QR) versus the bridge API token (panel/REST). Show how to rotate the OBB token and what
   happens to peers.
4. Show the exact `openmesh-bridge join '<connection string>'` command for a gateway, and what
   "connected/peers" looks like afterwards.
5. Add monitoring: /api/health, /api/backbone totals, peer RTT, logs; alert thresholds worth using.
6. Hardening: WireGuard/Tailscale recommendation, TLS wrapper alternative, max_peers/allowlist,
   no open mode, backups of identity/backbone config, upgrade/rollback.
7. Give me the final checklist I can tick off, and list the risks (metadata visibility, plaintext
   transport without VPN/TLS, token leakage, single point of failure).
Do not invent flags; if unsure, say what to check in `openmesh-bridge backbone --help`.
```

---

## 5. Developer integration with the API

**What it does:** helps build a client/script against the bridge REST + WebSocket + MCP surface,
with auth, schemas, pagination, activity attribution and error handling.

**When to use:** building a dashboard, bot, exporter, mobile client or test harness.

**Safety:** read-only by default; any write endpoint should be gated behind explicit confirmation.
Keep tokens out of source and logs; use loopback or a tunnel.

```text
I'm integrating with the OpenMesh bridge API (v0.3.0). Use the embedded spec at
GET /api/openapi.json (or docs/api/openapi.json) as the source of truth, plus openmesh_help topics
agents and security.

1. Summarise the auth model (bearer token, loopback-only), the main resources and how they relate
   (nodes, contacts, channels, messages, attachments, backbone, setup, activity).
2. Show me idiomatic request examples for: health, status, nodes, direct/channel message history,
   send a message, upload an attachment, subscribe to /api/ws and /api/events, and list activity.
3. Explain pagination (`limit`, `before_ms`), message ids, attachment ids, and the delivered/read
   model.
4. Show how to attribute writes with X-OpenMesh-Agent / X-OpenMesh-Client, and how to handle the
   JSON error shape (error/code/message/help_topic) with retries for 409/503 and no retries for
   4xx.
5. Recommend a client architecture: reconnect/backoff, event dedup, optimistic UI, token from a
   file/env, never in logs.
6. Generate a minimal, runnable script (my language: <LANGUAGE>) that reads a message history and
   a health check, with tests/mocks for the HTTP layer. Use the OpenAPI schemas for validation.
Ask me before suggesting any write endpoint beyond a test payload.
```

---

## 6. Tweak a sensor or radio profile

**What it does:** reasons about a custom sensor/telemetry node or a radio-profile change: which
fields exist, what the change costs in airtime/range, and how to roll it out safely.

**When to use:** adding a sensor, changing `MEDIUM_FAST`→`LONG_FAST`, adjusting power/region,
battery/solar tuning, or deciding that a change is not worth it.

**Safety:** radio/regulatory settings are legally constrained and affect every node in range.
Confirm region and power limits, change one variable at a time, and keep a rollback value.

```text
I want to change my OpenMesh radio/sensor setup. Work from openmesh_status, openmesh_config,
openmesh_nodes and the help topics radio-and-region, nodes-telemetry, media, troubleshooting.

1. Show me the relevant current settings (region, profile, tx power, frequency, announce interval)
   and what they mean in one line each.
2. For my proposed change (<describe it>), quantify the trade-off: approximate frame airtime,
   impact on text/media, range implications, duty-cycle budget, and effect on battery-powered nodes.
   Use the numbers in the radio help topic and state they are model estimates.
3. Check legality: does the region allow the power/frequency? If my country/region is unknown, stop
   and ask.
4. Give a rollout plan: change one setting, how to verify with nodes/status, what "bad" looks like,
   how long to observe, and the exact rollback value.
5. If the change requires a firmware/config update on a repeater or tracker, give the safe order so
   the mesh does not split (update repeaters first/last as appropriate, keep one gateway on the old
   profile during the transition).
6. Confirm the final values before applying anything; use openmesh_set_config only if I explicitly
   approve, and record the before/after in the activity log.
```

---

## 7. Operate daily (advanced)

**What it does:** a standing prompt for routine operations — summaries, peer/queue checks, activity
review, and upgrade readiness — without touching anything destructive.

**When to use:** once a mesh is live and you want an agent to watch it.

**Safety:** read-only; escalate only by asking a human. Good default for a read-only agent.

```text
Act as the read-only operator assistant for my OpenMesh node. Each session:
1. Report health, status, node count, peers (if backbone), airtime/duty cycle and any anomalies.
2. Review the last 20 activity entries for unexpected writes; summarise by source.
3. Flag: auth failures, peers at max_peers, queue growth, store-forward pressure, low disk, stale
   repeaters, certificate/VPN expiry if I tell you about one.
4. Check openmesh_help for the relevant topic and include the link.
5. End with at most three concrete actions for me (with exact commands or settings), and state
   clearly that you did not change anything.
Never rotate tokens, kick peers, change radio settings or send messages in this mode.
```

---

## Choosing the right tier

`openmesh-cli agent prompt --tier beginner|advanced` prints a ready system prompt with the live
status, MCP config and help index filled in. Use **beginner** for family/onboarding conversations
and **advanced** for operations, protocol questions and developer work; it only changes the
explanations, not permissions. Pair it with this pack when you want a task-specific script.

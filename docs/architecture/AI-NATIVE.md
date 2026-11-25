# AI-Native OpenMesh — v0.3.0 specification

**Status:** normative for v0.3.0 · **Applies to:** `crates/openmesh-bridge` (MCP + activity +
OpenAPI), `crates/openmesh-cli` (agent commands), `apps/web` (Agents page), `content/help/`,
`docs/agents/`, `AGENTS.md`, `llms.txt`, `docs/api/openapi.json`
**Related:** [ux-help-system.md](../design/ux-help-system.md), [WEBSOCKET-API.md](../architecture/WEBSOCKET-API.md)

"AI-native" means three concrete things, not a marketing badge:

1. **An agent can operate a node** through a real, typed, permissioned interface (MCP).
2. **A human can hand their agent a prompt and it knows what to do** — with live state, the right
   help topic and explicit safety rules (prompt pack + `agent prompt` command).
3. **An agent can work on the project itself** — `AGENTS.md`, machine-readable API spec and
   `llms.txt` are shipped so coding agents know the invariants (protocol spec is normative,
   golden vectors, write scopes, test commands).

---

## 1. MCP server

`openmesh-bridge mcp` runs a **Model Context Protocol** server on **stdio** (JSON-RPC 2.0, one
JSON object per line). It is a thin, honest client of the local bridge REST API — it holds no
state and never touches identity keys directly.

```
Claude Desktop / Cursor / DSH / any MCP client
        │  stdio JSON-RPC (MCP)
        ▼
openmesh-bridge mcp ── HTTP ──► http://127.0.0.1:8787 (bridge REST)
                                     └─ radio / mesh / store
```

Environment:

| Variable | Default | Meaning |
| --- | --- | --- |
| `OPENMESH_API` | `http://127.0.0.1:8787` | bridge base URL |
| `OPENMESH_TOKEN` | — | bridge API token (preferred) |
| `OPENMESH_TOKEN_FILE` | `~/.config/openmesh/bridge-token` | token file fallback |
| `OPENMESH_MCP_READ_ONLY` | `0` | `1` = expose only read tools, reject writes |
| `OPENMESH_AGENT_NAME` | hostname | sent as `X-OpenMesh-Agent`, shown in the activity log |
| `OPENMESH_HELP_DIR` | `content/help` | help corpus for `openmesh_help` (falls back to embedded copy) |

Protocol: MCP `initialize` echoes the client's `protocolVersion`, advertises
`capabilities.tools/resources/prompts`, `serverInfo {name:"openmesh", version:<crate version>}`.
Supports `tools/list`, `tools/call`, `resources/list`, `resources/read`, `prompts/list`,
`prompts/get`, `ping`. Unknown methods return JSON-RPC `-32601`. Never writes anything except
JSON-RPC to stdout (logs go to stderr).

### 1.1 Tools

Read-only tools (always available):

| Tool | Arguments | Returns |
| --- | --- | --- |
| `openmesh_health` | — | bridge/API version, uptime, radio connected |
| `openmesh_status` | — | node identity summary, radio profile/region, airtime %, queues |
| `openmesh_nodes` | `role?`, `has_position?`, `limit?` | known nodes with class, caps, hops, RSSI/SNR, battery, position, last seen |
| `openmesh_node` | `node_id` | one node + contact status |
| `openmesh_messages` | `peer?`, `channel?`, `limit?`, `before_ms?` | message history (direct or channel) |
| `openmesh_channels` | — | channels (never PSKs) |
| `openmesh_contacts` | — | contacts with identity verification state |
| `openmesh_backbone_status` | — | backbone config (token redacted), totals, mode |
| `openmesh_backbone_peers` | — | connected peers with counters |
| `openmesh_backbone_setup` | — | onboarding payload: connection string, steps, firewall, systemd |
| `openmesh_setup_state` | — | first-run/checklist state + recommendations |
| `openmesh_activity` | `limit?` | recent write actions with source (`user` / `agent:<name>`) |
| `openmesh_help` | `topic?`, `audience?` | help index, or one topic's markdown from `content/help` |

Write tools (hidden when `OPENMESH_MCP_READ_ONLY=1`; ⚠ = requires `"confirm": true`):

| Tool | Arguments | Effect |
| --- | --- | --- |
| `openmesh_send_message` | `to`, `text`, `reply_to?`, `attachment_id?` | E2E direct message |
| `openmesh_send_channel_message` | `channel`, `text`, `attachment_id?` | group message |
| `openmesh_share_position` | `to?` or `channel?` | share current position once |
| `openmesh_announce` | — | force an announce |
| `openmesh_mark_read` | `msg_id` | read receipt |
| `openmesh_retry_message` | `msg_id` | retry delivery |
| `openmesh_set_config` | `patch` (node/radio/features partial) | apply config live |
| `openmesh_setup_quick` | `role`, `country`, `node_name?`, `privacy?`, `create_channel?` | secure beginner preset (see UX spec §5) |
| `openmesh_create_channel` | `name`, `psk?` | create channel, returns invite URL + PSK **once** |
| `openmesh_join_channel` | `url` or `name`+`psk` | join a channel |
| `openmesh_add_contact` | `invite_url` or `identity`+`name` | add a verified contact |
| `openmesh_backbone_config` | `patch` | live backbone config |
| `openmesh_rotate_backbone_token` ⚠ | `confirm`, `kick_peers?` | rotate token, returns it **once** |
| `openmesh_kick_backbone_peer` ⚠ | `node_id`, `confirm` | disconnect a peer |

Tool contract rules:

* Every tool has a JSON Schema `inputSchema`; results return MCP `content` (human-readable text)
  **and** `structuredContent` (the same data as JSON) so agents can reason and render.
* Errors return `isError: true` with a plain-language `message`, a `code`, and `help_topic`.
* No tool returns API tokens, PSKs, identity seeds or full identities except the one-time
  `rotate`/`create_channel` outputs, which are labelled "shown once".
* Tools are idempotent where possible (`openmesh_announce`); destructive actions are marked ⚠ and
  require `confirm: true`; the MCP server refuses destructive calls in read-only mode.
* Timeouts: 10 s per HTTP call; long media transfers return a queued result with a message id
  instead of blocking.

### 1.2 Resources

* `openmesh://help/index` — help topic index
* `openmesh://help/{slug}` — one help topic (markdown)
* `openmesh://status` — current status JSON
* `openmesh://nodes` — node list JSON
* `openmesh://config` — config JSON with secrets redacted

### 1.3 Prompts (user-selectable in MCP clients)

| Prompt | Purpose |
| --- | --- |
| `manage_my_mesh` | "Here is my mesh state; help me operate it safely" — embeds status, node list, help index and the security rules. |
| `set_up_my_node` | Beginner onboarding assistant: asks role/country/name, then calls `openmesh_setup_quick`. |
| `troubleshoot_mesh` | Diagnostic playbook: health → radio → peers → queue → airtime → help topic. |
| `set_up_backbone` | Walks through backbone config, firewall, systemd and gateway `join`. |
| `invite_someone` | Creates/reads a channel invite and drafts a shareable message. |

---

## 2. Activity log (transparency)

The bridge records every **write** API call in a bounded ring buffer (256 entries):

```json
{ "id": 12, "at_ms": 1760000000000, "source": "agent:claude", "method": "POST",
  "path": "/api/messages", "summary": "send message to 6b1f… (14 bytes)",
  "ok": true, "error": null }
```

* Source comes from `X-OpenMesh-Agent: <name>` (MCP sets it), `X-OpenMesh-Client: web|cli`, or
  `user` when absent.
* Read calls are not logged (no surveillance of browsing), only mutations.
* `GET /api/activity?limit=100`; MCP `openmesh_activity`; web "Agents → Activity" table.
* UI shows a "connected agent" badge with last action time; one click revokes (rotate token).
* Log entries never contain message bodies, tokens or PSKs.

---

## 3. Prompt pack, `AGENTS.md`, `llms.txt`, OpenAPI

### 3.1 User-facing prompt pack — `docs/agents/prompts.md`
Copy-paste prompts for popular agents (Claude Code/Desktop, Cursor, DSH, generic MCP):

* "Connect to my OpenMesh node and tell me what's wrong" (troubleshooting)
* "Set up a secure mesh for my family" (beginner onboarding)
* "Run a backbone on this VPS" (advanced)
* "Write a script that forwards new messages to my phone" (developer)
* "Add a new sensor type / change the radio profile" (advanced tweaking)

Each prompt includes the MCP config snippet and safe-operation rules, and says **what the agent
must confirm with the human first** (token rotation, open auth, deleting contacts).

### 3.2 `openmesh-cli agent …`
```
openmesh-cli agent prompt --tier beginner|advanced [--json]
    Prints a ready system prompt: current node state, MCP config, available tools,
    safety rules and relevant help topics. Beginner tier explains jargon; advanced
    tier includes protocol details and exact numbers.
openmesh-cli agent config --client claude|cursor|dsh|generic
    Prints the MCP client config JSON with the right absolute binary path.
openmesh-cli agent activity [--limit N]
    Shows the recent write actions (source, action, result).
```

### 3.3 Repository-level `AGENTS.md` (root)
Instructions for coding agents working **on** the repo: normative docs, invariants (protocol spec
+ golden vectors), build/test commands (including the zig-cc shim and `scripts/check.sh`), write
scopes, commit/PR rules, and where to add help content. `llms.txt` gives a compact map of the
docs for LLM crawlers.

### 3.4 `docs/api/openapi.json`
Machine-readable REST description (OpenAPI 3.1) covering every `/api/*` endpoint: parameters,
request/response schemas, auth, and the `X-OpenMesh-Agent` header. Served by the bridge at
`GET /api/openapi.json`; CI checks it documents every route in the bridge router and that
schemas parse.

---

## 4. Safety model for agents

* **Least privilege**: read-only mode exists and is the recommended default for exploratory
  agents; write tools are explicit.
* **Confirmations**: destructive or secret-revealing tools require `confirm: true`; the prompt
  pack instructs agents to ask the human first and to never paste tokens into chat logs.
* **No secret expansion**: the agent never needs the identity seed; it only talks to the bridge.
  Backbone token rotation returns once, and the prompt pack says to write it to a file, not to the
  conversation.
* **Auditability**: every mutation is attributed and visible in the UI/API.
* **Revocation**: rotating the bridge API token (or `--read-only`) immediately cuts an agent off.
* **Rate limiting**: MCP server serialises calls and caps writes to 30/min by default.

---

## 5. Acceptance criteria (v0.3.0)

1. `openmesh-bridge mcp` passes an MCP client handshake and exposes the full tool set; read-only
   mode hides/refuses writes; a scripted JSON-RPC test covers `initialize`, `tools/list`,
   `tools/call` (read + write + error + confirm-required + read-only rejection), `resources/read`,
   `prompts/get`.
2. `openmesh-cli agent prompt` prints a useful, accurate prompt for beginner and advanced tiers;
   `agent config` prints a working client config; `agent activity` renders the log.
3. Activity log records mutations with correct `source` and is visible in `/api/activity` and the
   web Agents page; no message bodies or secrets appear.
4. `GET /api/openapi.json` validates against OpenAPI 3.1 and covers all bridge routes (CI check).
5. `AGENTS.md`, `llms.txt`, `docs/agents/prompts.md`, `docs/agents/mcp.md` exist, are linked from
   README, and contain no fabricated commands (verified against `--help` output).
6. Beginner can use "copy this prompt + this config" from the Agents page without touching a
   terminal: panel shows MCP config, one-click copy prompt, read-only toggle and activity.
7. All existing gates stay green (Rust tests, web tests, docs links, clippy/fmt).

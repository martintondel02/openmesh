# MCP server reference

`openmesh-bridge mcp` is OpenMesh's local **Model Context Protocol** server. It lets an AI agent
(Claude Desktop/Code, Cursor, DSH, or any MCP client) inspect and operate your mesh through the
same local REST API the web panel uses — without giving the agent your identity seed, channel keys
or the ability to reach the bridge except over loopback.

**Status:** v0.3.0 · normative sources: [AI-NATIVE.md](../architecture/AI-NATIVE.md) §1,
[ux-help-system.md](../design/ux-help-system.md) · related:
[prompt pack](prompts.md), [AGENTS.md](../../AGENTS.md), [help topic "Use your AI agent"](../../content/help/agents.md).

---

## 1. Architecture and trust model

```
Claude Desktop / Claude Code / Cursor / DSH / any MCP client
        │  stdio: JSON-RPC 2.0, one JSON object per line (MCP)
        ▼
openmesh-bridge mcp            ← local process started by the client
        │  HTTP: Authorization: Bearer <bridge-token>
        ▼
bridge REST API  http://127.0.0.1:8787      ← identity, radio, store, backbone
```

* The MCP server is a **thin client**. It holds no state and never touches `identity.key`, the
  database, PSKs or the radio directly. Every action goes through the bridge REST API.
* It only works while the bridge is running. The bridge must be reachable at `OPENMESH_API`
  (loopback by default) and the token must be valid.
* The bridge API stays **loopback-only** by design. If the agent runs on another machine, tunnel
  the API (SSH port-forward/VPN) or run the MCP server where the bridge runs; never expose 8787
  to a network.
* MCP uses **stdio** for protocol messages; diagnostic logging goes to **stderr** only. Nothing
  but JSON-RPC is written to stdout.

### Transport and protocol

* MCP over stdio: one JSON object per line, JSON-RPC 2.0.
* `initialize` echoes the client's `protocolVersion` (tested with `2025-06-18`), advertises
  `capabilities.tools/resources/prompts`, and returns
  `serverInfo {name: "openmesh", version: "0.3.0"}` plus short instructions.
* Supported methods: `initialize`, `ping`, `tools/list`, `tools/call`, `resources/list`,
  `resources/read`, `prompts/list`, `prompts/get`. Unknown methods return JSON-RPC `-32601`.
* Tool results return MCP `content` (human-readable text) **and** `structuredContent` (the same
  data as JSON). Tool failures set `isError: true` and include a plain-language message, a code
  and a `help_topic` slug.
* Per-call timeout: 10 seconds. Long media transfers return a queued result with a message id
  instead of blocking.
* Write rate limit: 30 writes/minute per server process.

---

## 2. Install and configure

### 2.1 Prerequisites

1. OpenMesh v0.3.0 installed and the bridge running:
   ```sh
   curl -fsS http://127.0.0.1:8787/api/health    # {"ok":true,"version":"0.3.0"}
   ```
2. The bridge API token at `~/.config/openmesh/bridge-token` (mode 0600), or `OPENMESH_TOKEN`.
3. An MCP-capable client. The binary is `openmesh-bridge` (installed at `~/.local/bin/` by the
   installer, or `target/release/openmesh-bridge` from a source build). Always use an **absolute
   path** in client configs — MCP clients do not inherit your shell `PATH`.

Generate a ready-to-paste config for the bridge on this machine:

```sh
openmesh-cli agent config --client claude     # or cursor, dsh, generic
```

This prints JSON to stdout (safe to redirect) and a one-line hint to stderr. Copy the
`mcpServers` object into your client configuration, then restart the client.

### 2.2 Claude Desktop

Edit the desktop app's config (create it if missing):

| OS | Path |
| --- | --- |
| macOS | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| Windows | `%APPDATA%\Claude\claude_desktop_config.json` |
| Linux | `~/.config/Claude/claude_desktop_config.json` |

```json
{
  "mcpServers": {
    "openmesh": {
      "command": "/home/you/.local/bin/openmesh-bridge",
      "args": ["mcp"],
      "env": {
        "OPENMESH_API": "http://127.0.0.1:8787",
        "OPENMESH_TOKEN_FILE": "/home/you/.config/openmesh/bridge-token",
        "OPENMESH_AGENT_NAME": "claude-desktop"
      }
    }
  }
}
```

Quit and reopen Claude Desktop; the tools appear under the MCP/tools menu.

### 2.3 Claude Code

```sh
# macOS/Linux — add a user-scoped MCP server
claude mcp add openmesh -- /home/you/.local/bin/openmesh-bridge mcp

# verify
claude mcp list
```

`claude mcp add` supports `--scope user|project|local`; project-scoped configs live in
`.mcp.json`. For explicit environment variables, edit the generated entry or use the JSON shape
from §2.2 (Claude Code accepts the same `command`/`args`/`env` fields).

### 2.4 Cursor

Edit `~/.cursor/mcp.json` (global) or `.cursor/mcp.json` (project):

```json
{
  "mcpServers": {
    "openmesh": {
      "command": "/home/you/.local/bin/openmesh-bridge",
      "args": ["mcp"],
      "env": {
        "OPENMESH_API": "http://127.0.0.1:8787",
        "OPENMESH_TOKEN_FILE": "/home/you/.config/openmesh/bridge-token",
        "OPENMESH_AGENT_NAME": "cursor"
      }
    }
  }
}
```

Restart Cursor, then enable the server in **Settings → MCP** if it is listed but disabled.

### 2.5 DSH

In the harness MCP settings, add a **stdio** server:

| Field | Value |
| --- | --- |
| name | `openmesh` |
| command | `/home/you/.local/bin/openmesh-bridge` (absolute) |
| args | `["mcp"]` |
| env | `OPENMESH_API=http://127.0.0.1:8787`, `OPENMESH_TOKEN_FILE=/home/you/.config/openmesh/bridge-token`, `OPENMESH_AGENT_NAME=dsh` |

DSH passes the `env` map to the child process; do not rely on your interactive shell environment
being inherited. Restart the session after saving.

### 2.6 Generic stdio client

Any MCP client that supports stdio servers can use this shape:

```json
{
  "mcpServers": {
    "openmesh": {
      "command": "/absolute/path/to/openmesh-bridge",
      "args": ["mcp"],
      "env": {
        "OPENMESH_API": "http://127.0.0.1:8787",
        "OPENMESH_TOKEN_FILE": "/absolute/path/to/bridge-token",
        "OPENMESH_AGENT_NAME": "my-agent"
      }
    }
  }
}
```

Manual smoke test without a client:

```sh
printf '%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  | openmesh-bridge mcp
```

---

## 3. Environment variables

| Variable | Default | Meaning |
| --- | --- | --- |
| `OPENMESH_API` | `http://127.0.0.1:8787` | Bridge REST base URL. Must reach the local bridge. |
| `OPENMESH_TOKEN` | — | Bridge API token value (preferred over the token file). |
| `OPENMESH_TOKEN_FILE` | `~/.config/openmesh/bridge-token` | Token file fallback, read per request/start. |
| `OPENMESH_MCP_READ_ONLY` | `0` | `1` hides every write tool and refuses write calls (`MCP_READ_ONLY` is accepted as an alias). |
| `OPENMESH_AGENT_NAME` | hostname | Sent as `X-OpenMesh-Agent`, shown as `agent:<name>` in the activity log. |
| `OPENMESH_HELP_DIR` | `content/help` | Help corpus dir for `openmesh_help` (falls back to the embedded copy). |
| `OPENMESH_MCP_WRITE_LIMIT` | `30` | Writes/minute (additive testing hook; production default is 30). |

The server is configured by environment only; `openmesh-bridge mcp --help` exists, but the bridge
global flags are not used by the MCP server (set `OPENMESH_*` in the client config). Never put a
token in `args` (visible in process lists); use `OPENMESH_TOKEN_FILE` or the client's secret store.

---

## 4. Tool catalog

The table is the contract (AI-NATIVE.md §1.1) and matches `tools/list` in v0.3.0. All read tools are
always available; write tools are hidden and rejected when `OPENMESH_MCP_READ_ONLY=1`. ⚠ marks
destructive tools that require `"confirm": true` in their arguments.

### 4.1 Read-only tools

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

### 4.2 Write tools

| Tool | Arguments | Effect |
| --- | --- | --- |
| `openmesh_send_message` | `to`, `text`, `reply_to?`, `attachment_id?` | E2E direct message |
| `openmesh_send_channel_message` | `channel`, `text`, `attachment_id?` | group message |
| `openmesh_share_position` | `to?` or `channel?` | share current position once |
| `openmesh_announce` | — | force an announce |
| `openmesh_mark_read` | `msg_id` | read receipt |
| `openmesh_retry_message` | `msg_id` | retry delivery |
| `openmesh_set_config` | `patch` (node/radio/features partial), `regulatory_override?` (emergency; enabling/exceeding requires `confirm: true`), `confirm?` | apply config live; returns the override warning when active |
| `openmesh_setup_quick` | `role`, `country`, `node_name?`, `privacy?`, `create_channel?` | secure beginner preset (UX spec §5) |
| `openmesh_create_channel` | `name`, `psk?` | create channel, returns invite URL + PSK **once** |
| `openmesh_join_channel` | `url` or `name`+`psk` | join a channel |
| `openmesh_add_contact` | `invite_url` or `identity`+`name` | add a verified contact |
| `openmesh_backbone_config` | `patch` | live backbone config |
| `openmesh_rotate_backbone_token` ⚠ | `confirm`, `kick_peers?` | rotate token, returns it **once** |
| `openmesh_kick_backbone_peer` ⚠ | `node_id`, `confirm` | disconnect a peer |

### 4.3 Tool contract rules

* Every tool has a JSON Schema `inputSchema`; results include `content` and `structuredContent`.
* Errors return `isError: true` with `message`, `code` and `help_topic`; codes include
  `bad_request`, `not_found`, `unknown_peer`, `radio_busy`, `confirm_required`, `timeout`.
* No tool returns API tokens, PSKs, identity seeds or full identities — except one-time outputs
  from `openmesh_rotate_backbone_token` and `openmesh_create_channel`, labelled "shown once".
* Read-only mode hides writes from `tools/list` (not just rejects calls), so clients never see
  tools they cannot use.
* Write calls are recorded in the bridge activity log with `source=agent:<OPENMESH_AGENT_NAME>`.

---

## 5. Resources and prompts

### 5.1 Resources (`resources/list`, `resources/read`)

| URI | Content | MIME |
| --- | --- | --- |
| `openmesh://help/index` | Help topic index (JSON) | `text/markdown` |
| `openmesh://help/{slug}` | One help topic (all 16 topics are listed, e.g. `openmesh://help/getting-started`) | `text/markdown` |
| `openmesh://status` | Current bridge status JSON | `application/json` |
| `openmesh://nodes` | Known nodes JSON | `application/json` |
| `openmesh://config` | Configuration JSON with secrets redacted | `application/json` |

### 5.2 Prompts (`prompts/list`, `prompts/get`)

| Prompt | Purpose |
| --- | --- |
| `manage_my_mesh` | "Here is my mesh state; help me operate it safely" — embeds status, node list, help index and security rules. |
| `set_up_my_node` | Beginner onboarding assistant: asks role/country/name, then calls `openmesh_setup_quick`. |
| `troubleshoot_mesh` | Diagnostic playbook: health → radio → peers → queue → airtime → help topic. |
| `set_up_backbone` | Walks through backbone config, firewall, systemd and gateway `join`. |
| `invite_someone` | Creates/reads a channel invite and drafts a shareable message. |

---

## 6. Read-only mode, activity and safety

### 6.1 Read-only mode

For "look, don't touch" assistants (dashboards, triage, remote hands), launch with:

```json
"env": { "OPENMESH_MCP_READ_ONLY": "1", "OPENMESH_AGENT_NAME": "triage" }
```

* `tools/list` contains only the 13 read tools.
* Any write `tools/call` is refused before it reaches the bridge.
* The web panel's Agents page shows the read-only state and the activity log; a "connected agent"
  badge appears after the first agent write (read-only agents do not appear as writers).

### 6.2 Activity and attribution

* The bridge records every **write** API call in a bounded 256-entry ring:
  `{id, at_ms, source, method, path, summary, ok, error}`.
* `source` is `agent:<name>` from `X-OpenMesh-Agent` (set automatically by MCP from
  `OPENMESH_AGENT_NAME`), `web`/`cli` from `X-OpenMesh-Client`, otherwise `user`.
* Read calls are **not** logged. Entries never contain message bodies, tokens or PSKs.
* View it with `openmesh-cli agent activity --limit 50`, the `openmesh_activity` tool, or
  `GET /api/activity?limit=50`.

### 6.3 Safety rules for humans

1. **Use a separate agent identity/token** where your client supports it; revoke by rotating the
   bridge API token (stop bridge → delete `bridge-token` → restart → paste new token into panel
   and clients).
2. **Start read-only.** Grant writes only when the agent needs them.
3. **Confirm destructive calls yourself.** `openmesh_rotate_backbone_token` and
   `openmesh_kick_backbone_peer` require `confirm: true`; the MCP server does not invent it.
4. **Never paste tokens/PSKs into a chat.** Tool errors are redacted, but a human can still leak
   secrets in a prompt; instruct the agent to read from the token file instead.
5. **Review Activity** after sessions; look for unexpected config changes, sends or rotations.
6. **Keep the API loopback-only.** The security of the MCP path is the security of the bridge API
   token plus your local machine.

---

## 7. Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| Client shows no tools / server fails to start | `command` is not absolute, binary missing, or args wrong | Use `openmesh-cli agent config --client <x>` output; test the binary path manually. |
| Server starts then exits immediately | Bridge not running, or stdout polluted by a wrapper/log | Check `curl http://127.0.0.1:8787/api/health`; ensure only the MCP server writes stdout. |
| Every tool returns 401/unauthorized | Wrong/rotated token, token file moved | Update `OPENMESH_TOKEN`/`OPENMESH_TOKEN_FILE`; restart the client; paste the new panel token. |
| `initialize` works, `tools/call` times out | Bridge hung/radio busy, or HTTP timeout | Check `GET /api/status`, logs with `--log debug`; long media returns queued instead of blocking. |
| Write tools missing from `tools/list` | `OPENMESH_MCP_READ_ONLY=1` | Remove the variable (or set `0`) and restart the MCP client. |
| `confirm_required` error | Destructive tool called without confirmation | Re-issue with `"confirm": true` only after a human agrees. |
| `Unknown method` / `-32601` | Client sends an unsupported MCP method | Upgrade the client/bridge; methods supported are listed in §1. |
| `-32602` / invalid params | Tool arguments do not match `inputSchema` | Check the schema via `tools/list`; common issues: node id format, `msg_id` type. |
| Agent actions not in Activity | Only writes are logged, or `X-OpenMesh-Agent` missing | Set `OPENMESH_AGENT_NAME`; perform a write; check `GET /api/activity`. |
| `connection refused` | Bridge not listening at `OPENMESH_API` | Start the bridge; verify port 8787 and `--listen`. |
| Tools work but results look stale | Read cache/UI delay, or bridge snapshot interval | Read `openmesh_status` again; check `/api/status` directly. |

Diagnostic commands:

```sh
openmesh-bridge --version
curl -fsS http://127.0.0.1:8787/api/health
openmesh-cli agent config --client generic
openmesh-cli agent activity --limit 10
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' | openmesh-bridge mcp
```

If the MCP server fails before `initialize`, run it in a terminal to see stderr (it logs the bridge
URL it is using and the API error, never the token).

---

## 8. See also

* [Prompt pack](prompts.md) — copy-paste prompts for common jobs.
* [AI-NATIVE.md](../architecture/AI-NATIVE.md) — tool catalog, resources, prompts and rationale.
* [WEBSOCKET-API.md](../architecture/WEBSOCKET-API.md) — the REST/WS API the server calls.
* [AGENTS.md](../../AGENTS.md) — instructions for coding agents working on this repository.
* Help topic [Use your AI agent](../../content/help/agents.md) — the beginner-friendly version.

# Use your AI agent

OpenMesh v0.3.0 is **AI-native**: a local Model Context Protocol (MCP) server lets Claude, Cursor,
DSH or any MCP client read your mesh, send messages and apply safe presets — with your node
staying in control. This topic covers setup, permissions and guardrails.

## What it is

* **`openmesh-bridge mcp`** — a local MCP server that speaks JSON-RPC 2.0 over stdio and talks to
  your bridge's REST API at `http://127.0.0.1:8787`. Your agent launches it; it holds no keys and
  no state.
* **Tools** — read tools (`openmesh_health`, `openmesh_status`, `openmesh_nodes`,
  `openmesh_messages`, `openmesh_channels`, `openmesh_contacts`, `openmesh_backbone_status`,
  `openmesh_activity`, `openmesh_help`, …) and write tools (`openmesh_send_message`,
  `openmesh_share_position`, `openmesh_set_config`, `openmesh_setup_quick`,
  `openmesh_create_channel`, `openmesh_add_contact`, `openmesh_backbone_config`, …). Destructive
  tools require `"confirm": true`.
* **Resources and prompts** — help topics, status, nodes and config are exposed as MCP resources;
  ready-made prompts (`manage_my_mesh`, `set_up_my_node`, `troubleshoot_mesh`, `set_up_backbone`,
  `invite_someone`) guide an agent through common jobs.
* **Activity log** — every write from an agent is attributed (`X-OpenMesh-Agent: <name>`) and
  visible in the panel's Agents → Activity table. Read calls are not logged.

## Why it matters

A well-intentioned agent can save you an enormous amount of fiddling: "why is my mesh slow?",
"set up my parents' node", "run a backbone on my VPS", "write a script against the API". The risk is
equally real: an agent with a token can send messages as you and change radio settings. OpenMesh
makes that risk visible and bounded — a separate token file, a read-only switch, confirmation for
dangerous calls, and an audit trail anyone can inspect.

## Quick start

1. Install OpenMesh v0.3.0 and make sure the bridge is running (`curl http://127.0.0.1:8787/api/health`).
2. Print a ready-made client config:

   ```sh
   openmesh-cli agent config --client claude    # or cursor, dsh, generic
   ```

   It contains the absolute path to `openmesh-bridge`, `mcp` as the argument, and the token/API
   environment. Paste it into your client's MCP configuration.

3. For a manual config, the shape is:

   ```json
   {
     "mcpServers": {
       "openmesh": {
         "command": "/home/you/.local/bin/openmesh-bridge",
         "args": ["mcp"],
         "env": {
           "OPENMESH_API": "http://127.0.0.1:8787",
           "OPENMESH_TOKEN_FILE": "/home/you/.config/openmesh/bridge-token",
           "OPENMESH_AGENT_NAME": "claude"
         }
       }
     }
   }
   ```

4. Restart your agent, then ask it to call `openmesh_health` or use the `manage_my_mesh` prompt.
5. Copy a starter prompt with `openmesh-cli agent prompt --tier beginner` (or `--tier advanced`).

## Advanced notes

### Environment

| Variable | Default | Meaning |
| --- | --- | --- |
| `OPENMESH_API` | `http://127.0.0.1:8787` | bridge base URL |
| `OPENMESH_TOKEN` | — | API token (preferred over the file) |
| `OPENMESH_TOKEN_FILE` | `~/.config/openmesh/bridge-token` | token fallback |
| `OPENMESH_MCP_READ_ONLY` | `0` | `1` = only read tools; writes are hidden and refused |
| `OPENMESH_AGENT_NAME` | hostname | attribution in the activity log |
| `OPENMESH_HELP_DIR` | `content/help` | help corpus for `openmesh_help` (falls back to embedded copy) |

### Safety rules (enforced by the server)

* 30 writes per minute per agent, 10-second HTTP timeout; long media returns a queued result.
* Dangerous tools (`openmesh_rotate_backbone_token`, `openmesh_kick_backbone_peer`) need
  `"confirm": true`; read-only mode rejects them outright.
* The server never returns API tokens, PSKs, identity seeds or full identities — except one-time
  outputs (`rotate_token`, `create_channel`), labelled "shown once".
* Errors include a plain-language message, a code and a help topic so the agent can recover.
* The bridge API stays loopback-only; the agent runs on the same machine (or through an SSH tunnel
  you control). Do not expose 8787 to a network so an agent elsewhere can reach it.

### Which agent for which job

* **Beginner** — `set_up_my_node` prompt + `openmesh_setup_quick`: role, country, name, privacy.
* **Troubleshooting** — `troubleshoot_mesh`: health → radio → peers → queues → airtime → help topic.
* **Operations** — `set_up_backbone` and `openmesh_backbone_status/peers/setup`; a human approves
  firewall and token changes.
* **Development** — `GET /api/openapi.json` + the generated MCP tools; keep writes idempotent and
  attribute them with `X-OpenMesh-Agent`.
* **Family setup** — `invite_someone` drafts a shareable message from a channel invite.

### Revoking access

* Delete or rotate the bridge API token (stop the bridge, remove `bridge-token`, restart, paste the
  new one into the panel and the agent config). Rotation invalidates every client, including the
  web UI, until updated.
* Remove the MCP server entry from the agent config to stop it launching.
* Review Activity for what was done before the revoke; entries contain no message bodies.

## Troubleshooting

| Symptom | Check | Fix |
| --- | --- | --- |
| Agent cannot start the MCP server | Path in `command` is absolute and executable? | Use `openmesh-cli agent config` output; test `/path/openmesh-bridge mcp` manually. |
| 401 from every tool | Token file moved, or bridge token rotated | Update `OPENMESH_TOKEN*` or paste the new token; restart the agent. |
| Tools list is missing write tools | `OPENMESH_MCP_READ_ONLY=1` | Remove the variable or set `0` and restart the agent. |
| Agent says "connection refused" | Bridge not running or not on 8787 | Start the bridge; check `OPENMESH_API`. |
| Dangerous call returns `confirm_required` | Safety confirmation missing | Re-issue with `"confirm": true` after the human agrees. |
| No activity entries | Only writes are logged; reads are not | Perform a write tool; check the Agents → Activity screen. |

## Related topics

[Security and privacy](security.md) · [Run a backbone](backbone.md) ·
[Troubleshooting](troubleshooting.md) · [Glossary](glossary.md)

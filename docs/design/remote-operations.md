# Remote operations and gateways — v0.7.0 contract

**Status:** normative for v0.7.0 · **Applies to:** `crates/openmesh-bridge` (MCP HTTP, gateways,
multi-backbone links), `crates/openmesh-cli`, `docs/api/openapi.json`, `docs/agents/mcp.md`,
`docs/architecture/{AI-NATIVE,BACKBONE}.md`, `apps/web` (operator UI), `content/help/` and
`docs/guides/` (Lead), ADR-0012.
**Decisions:** keep MCP (stdio stays the default, HTTP is opt-in); gateways are **generic
authenticated webhooks** in v0.7 — Matrix/email/SMS/satellite are bridged through adapters, not
direct vendor SDKs yet; multi-backbone links (client side) ship, backbone-to-backbone peering and
OBB/2 TLS are deferred.

> Security first: everything here is **loopback by default**, bearer-token authenticated, rate
> limited, secrets come from env vars only (never written to config or logs), and every write is
> audited in the activity log.

---

## 1. Remote MCP over HTTP (AI-native operations)

The MCP server currently runs over stdio (`openmesh-bridge mcp`) and proxies to the local REST API.
v0.7 adds an opt-in **HTTP transport on the bridge's existing HTTP listener** so an agent on
another machine (or a tunnel/VPN) can operate the node.

### Configuration — `<data-dir>/mcp-http.json` (mode 0600, default disabled)

```json
{
  "enabled": false,
  "path": "/mcp",
  "allow_write": false,
  "require_confirm": true,
  "rate_limit_per_min": 60,
  "max_sessions": 8
}
```

* `enabled=false` → `/mcp` returns 404 (default).
* Auth: `Authorization: Bearer <bridge token>` (same token as REST). Wrong/missing → 401 JSON-RPC
  error, no tool execution.
* Scope: read-only by default. Write/confirm-gated tools are refused with
  `{code:"http_scope_read_only"}` unless `allow_write=true`; `require_confirm=true` keeps the
  existing `confirm:true` requirement for dangerous tools (config/regulatory/backbone/key ops).
* Transport: POST `/mcp` with a JSON-RPC message → JSON-RPC response (`application/json`); if the
  request `Accept` includes `text/event-stream`, reply with one SSE `data:` frame then close.
  `GET /mcp` returns an SSE keep-alive stream (`event: ping` every 20 s) while enabled; `DELETE
  /mcp` ends the session. `Mcp-Session-Id` is accepted and echoed; a value is generated if absent.
* Rate limit per session/token; `max_sessions` bounded; requests larger than 256 KiB rejected.
* Every write tool call is appended to the activity log (`source: agent:<name>` when a client name
  is supplied via `X-OpenMesh-Agent`, else `source: agent:mcp-http`); responses never include
  tokens/PSKs.
* CLI: `openmesh-cli agent config --transport http --url http://127.0.0.1:8787 --token-file …`
  prints a ready-to-paste client config; `agent prompt` unchanged.
* Docs: `docs/agents/mcp.md` gains a "Remote MCP over HTTP" section (bind/loopback warning,
  tunnel/VPN guidance, scopes, read-only default, token handling, example curl sessions), and
  `AI-NATIVE.md` §1.1 documents the transport and scope rules.

### Tests

JSON-RPC `initialize` → `tools/list` over POST; read tool works; write tool refused in read-only;
write tool requires `confirm` when enabled; bad token → 401 without execution; `Accept:
text/event-stream` yields one `data:` frame; GET returns SSE ping headers; session header echoed;
rate limit returns JSON-RPC error; `/mcp` 404 when disabled.

---

## 2. Gateways — authenticated webhooks in both directions

Generic connectors that bridge a channel to any external service through an adapter (SMS gateway,
satellite messenger, Matrix/email bot, ntfy, custom script). Configuration lives in
`<data-dir>/gateways.json` (mode 0600). **Secret values are never stored** — only env-var names.

```json
{
  "version": 1,
  "connectors": [
    {
      "id": "sms-ops",
      "kind": "webhook",
      "enabled": true,
      "outbound": {
        "url": "https://gateway.example/openmesh",
        "secret_env": "OPENMESH_SMS_OUT_SECRET",
        "channel_ids": [],
        "include_attachments": false,
        "timeout_ms": 10000,
        "max_retries": 3
      },
      "inbound": {
        "enabled": true,
        "secret_env": "OPENMESH_SMS_IN_SECRET",
        "channel_id": "dba1337a96acc686",
        "max_chars": 400
      }
    }
  ]
}
```

* **Outbound:** on every new **channel** message (and, when `channel_ids` is empty, all channels;
  direct messages only if `"include_direct": true`), POST JSON:

  ```http
  POST <outbound.url>
  Content-Type: application/json
  X-OpenMesh-Signature: sha256=<hex HMAC-SHA256(secret, body)>
  {"event":"message","connector":"sms-ops","channel_id":"…","message_id":…,
   "from":"<node id>","from_name":"…","text":"…","timestamp_ms":…,
   "attachment":{"mime":"image/webp","size":1234}}
  ```

  `attachment` is metadata only (v0.7 does not push attachment bytes through webhooks). Delivery
  is best-effort with bounded retries/backoff; failures and retry exhaustion are activity-log
  entries (metadata only). Text is truncated to 4 000 chars.
* **Inbound:** `POST /api/gateways/{id}/inbound` with the same signature header over the raw body
  and JSON `{"text":"…","from":"+4712345678","timestamp_ms":…}`; injects a channel message
  attributed to the gateway (`from` is display-only, never an identity claim), max `max_chars`,
  rate limited (30/min). Bad signature/missing secret/disabled inbound → 401/404/400, no
  injection. Injection is audited and appears on the mesh like any channel message.
* **API:** `GET /api/gateways` (redacted config + runtime status: last success/failure/retries,
  secret presence booleans), `PUT /api/gateways/{id}` (upsert; rejects inline secrets and unknown
  env names when set), `DELETE /api/gateways/{id}`, `POST /api/gateways/{id}/test` (sends a signed
  test payload; returns status). All write routes require the API token; activity-log entries.
* **CLI:** `openmesh-cli gateways list|show <id>|set <id> --from-file|remove <id>|test <id>` using
  the REST API; secrets are passed as `--secret-env OUT=NAME --secret-env IN=NAME` or config file
  references, never argv values.
* **Web UI:** Backbone/Agents section → "Gateways" card: connector list with status pills, add/edit
  form (URL, env-var names, channels, inbound toggle + generated inbound URL with copy button),
  test button, enable/disable, delete (type-to-confirm); beginner sees a one-line explanation and
  no forms. Demo shows seeded connectors.
* **Docs/help:** new help topic `gateways-and-remote-agents` (17→18 topics; Lead-owned index) and
  `docs/guides/remote-operations.md` with adapter examples for a Matrix bot, an SMS provider
  webhook and an email forwarder; BACKBONE.md §"Gateways" pointing at it.

### Tests

Signature generation/verification (constant-time compare) with golden HMAC; outbound POST to a
local test server (correct headers/body, retry on 500, give-up after max_retries); attachments
metadata only; channel filter; inbound success → channel message event; inbound bad signature /
disabled / over-length → correct status and no injection; secret redaction in API/CLI output;
activity entries; rate limit; OpenAPI coverage.

---

## 3. Multiple backbone links (client side)

A bridge may join several backbones at once (redundancy, separate communities, failover).
Configuration `<data-dir>/backbone-links.json` (mode 0600):

```json
{"version":1,"links":[
  {"name":"oslo","url":"openmesh://backbone?host=…&port=9443&token=…&node=…","enabled":true},
  {"name":"cabin","url":"openmesh://backbone?host=…&port=9443&token=…&node=…","enabled":true}
]}
```

* Manager keeps one reconnect loop per enabled link with per-link backoff; the engine's `(src,seq)`
  dedup suppresses duplicates across links.
* Compatibility: the existing single `join`/`backbone.json` client path keeps working; a saved
  `backbone-links.json` takes precedence, and the legacy link is imported as `default` on first
  run.
* API: `GET /api/backbone/links` → `[{name,mode:"client",enabled,connected,last_error,stats...}]`;
  `PUT /api/backbone/links/{name}` / `DELETE` manage entries; status/peer tables include the link
  name. `GET /api/backbone` gains `links` summary.
* CLI: `openmesh-cli backbone links list|add --url … --name …|remove <name>`; existing `join`
  writes a link named `default`.
* UI: Backbone panel lists links with status, add/remove, copy invite-style `openmesh://` link.
  WebSocket `backbone_status` includes links.
* BACKBONE.md: new §"Multiple outbound links" (topology, loop safety via dedup, operator advice);
  docs/help mention redundancy.
* Tests: two mock backbone servers (local TcpListener) accepted; one link down while the other
  stays connected; restart restores enabled links; legacy join import; API/CLI CRUD; no secret
  leakage (tokens redacted except in the locally generated link when explicitly requested).

---

## 4. MCP decision and non-goals for v0.7

* **MCP stays.** stdio remains the default, fully local path; HTTP is opt-in and read-only by
  default. No removal, no feature-gate of stdio.
* **Not in v0.7:** direct Matrix/email/SMS vendor connectors (use webhook adapters), inbound
  attachment upload through gateways, OBB/2 TLS and per-peer identity, backbone-to-backbone
  peering. OBB/2 gets a dedicated design/ADR in the next slice.
* No wire-format changes; golden vectors unchanged.

## 5. Acceptance

* `cargo test --workspace --all-targets`, clippy `-D warnings`, fmt; OpenAPI/help/docs checkers.
* New tests: MCP HTTP (≥8), gateways (≥10), multi-link (≥6); existing 245 stay green.
* Web build/tests + new gateway/MCP/link UI tests; screenshots.
* Docs/help: new help topic 18, remote-operations guide, ADR-0012, MCP/BACKBONE/AI-NATIVE updates.
* Version 0.7.0 (workspace/web/Tauri/Android versionCode 8/firmware); release assets.

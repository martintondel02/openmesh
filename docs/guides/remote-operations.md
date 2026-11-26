# Remote operations: MCP over HTTP, gateways and multi-backbone links

Operator guide for OpenMesh v0.7.0. It covers the three ways a bridge can reach beyond its local
radios: the opt-in **MCP HTTP transport** for remote AI agents, **gateways** (signed webhooks in
both directions) with adapter recipes for Matrix, SMS and email, and **multiple backbone links**
for redundancy. It assumes a bridge running on `http://127.0.0.1:8787` with the token at
`~/.config/openmesh/bridge-token` and the data directory at `~/.openmesh` (defaults; adjust if you
used `--data-dir`, `--token-file` or a container layout).

> **Read this first.** Everything here is **loopback by default**, bearer-token authenticated and
> rate limited; gateway secrets live only in environment variables. Remote access does not weaken
> mesh encryption itself, but a gateway **deliberately breaks end-to-end protection for the
> bridged leg** and a remote MCP token is **control of your node**. Treat both as privileged
> operator features, not convenience toggles. The normative contract is
> [remote-operations.md](../design/remote-operations.md); decisions are recorded in
> [ADR-0012](../adr/0012-remote-mcp-and-gateways.md).

Conventions used below:

```sh
TOKEN=$(cat ~/.config/openmesh/bridge-token)
API=http://127.0.0.1:8787
curl -fsS -H "Authorization: Bearer $TOKEN" "$API/api/health"
```

`curl -fsS` fails on HTTP errors, which makes it useful in scripts; drop `-f` when you want to read
the JSON error body. Never put the token on a command line that is logged or stored in shell
history; prefer `--token-file`, an environment variable, or a `curl` config file with mode `0600`.

---

## 1. Remote MCP over HTTP

### 1.1 When to use it

Use the HTTP transport when the MCP client (Claude Code, Cursor, DSH, a custom agent) does **not**
run on the bridge host, but the two machines are connected by a private network: WireGuard,
Tailscale, an SSH tunnel, or a LAN/VPN you control. Keep `stdio` (`openmesh-bridge mcp`) for a
local agent — it has no listening socket and is the safest default.

Do **not** publish `/mcp` on the public internet. If you must cross an untrusted network, terminate
TLS in front of the bridge and still keep the bearer token and read-only scope. A tunnel is easier
to secure than a public reverse proxy; see §1.4.

### 1.2 Configuration

`<data-dir>/mcp-http.json`, mode `0600`, disabled unless you create it:

```json
{
  "enabled": true,
  "path": "/mcp",
  "allow_write": false,
  "require_confirm": true,
  "rate_limit_per_min": 60,
  "max_sessions": 8
}
```

| Field | Meaning |
| --- | --- |
| `enabled` | `false` (default) makes `/mcp` return **404**. Restart the bridge after changing it. |
| `path` | Listener path; `/mcp` unless you have a reason to change it. |
| `allow_write` | `false` = read-only: write tools are refused with `http_scope_read_only`. |
| `require_confirm` | `true` keeps the per-call `confirm: true` requirement for dangerous tools (config, regulatory, backbone, key operations). Leave it on. |
| `rate_limit_per_min` | Per session/token request budget. |
| `max_sessions` | Simultaneous MCP sessions; keep it small. |

```sh
install -m 0600 /dev/null ~/.openmesh/mcp-http.json
cat > ~/.openmesh/mcp-http.json <<'JSON'
{"enabled":true,"path":"/mcp","allow_write":false,"require_confirm":true,
 "rate_limit_per_min":60,"max_sessions":8}
JSON
# restart the bridge (systemd: sudo systemctl restart openmesh-bridge)
```

Auth is the **same bridge token as REST**: `Authorization: Bearer <token>`. A missing or wrong
token returns 401 without executing anything. Requests above 256 KiB are rejected.

### 1.3 A session with curl

MCP over HTTP is JSON-RPC 2.0. Send `POST /mcp`; when the request's `Accept` includes
`text/event-stream` the bridge answers with one SSE `data:` frame and closes. `GET /mcp` is an SSE
keep-alive stream (`event: ping` every 20 s) while enabled, and `DELETE /mcp` ends the session.
`Mcp-Session-Id` is echoed if supplied, otherwise generated.

```sh
# 1. initialize (use the protocol version your client negotiates)
curl -sS -X POST "$API/mcp" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-06-18","capabilities":{},
        "clientInfo":{"name":"ops-shell","version":"1.0"}}}'

# 2. list tools (read tools only unless allow_write=true)
curl -sS -X POST "$API/mcp" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'

# 3. call a read tool
curl -sS -X POST "$API/mcp" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
        "name":"openmesh_status","arguments":{}}}'
```

Write tools need `allow_write=true` **and** `confirm: true`; only then:

```sh
curl -sS -X POST "$API/mcp" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{
        "name":"openmesh_set_config","arguments":{"profile":"LONG_FAST","confirm":true}}}'
```

With read-only scope the same call returns a JSON-RPC error with code
`http_scope_read_only` and the node is not changed. To tell the activity log who acted, send
`X-OpenMesh-Agent: ops-shell`; otherwise writes are attributed to `agent:mcp-http`.

SSE response shape (one `data:` frame):

```text
data: {"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"...status..."}]}}
```

Keep-alive stream and session end:

```sh
curl -sS -N -H "Authorization: Bearer $TOKEN" -H 'Accept: text/event-stream' "$API/mcp"
curl -sS -X DELETE -H "Authorization: Bearer $TOKEN" "$API/mcp"
```

### 1.4 Private networking patterns

* **Loopback (recommended):** leave the bridge bound to `127.0.0.1`; run the agent on the same host.
* **SSH tunnel:** on the agent machine run
  `ssh -N -L 8787:127.0.0.1:8787 user@bridge-host`, then point the agent at
  `http://127.0.0.1:8787`. Nothing new is exposed on the bridge host.
* **WireGuard/Tailscale:** bind the API to the VPN address (or keep loopback and use the VPN host
  as a jump host), firewall the port to VPN peers only, and use the bridge token as the second
  factor. Do not enable port forwarding from the internet.
* **Reverse proxy (last resort):** TLS termination with a strong cipher suite, an allow-list, rate
  limiting, and a private network between proxy and bridge. Never expose plain HTTP publicly.

Token handling: create a separate token file per agent if your deployment supports it, `chmod 600`
it, configure the MCP client to read the file (not to embed the token in a repository), and rotate
via the bridge token flow if a machine is lost. The CLI can print a ready client config:

```sh
openmesh-cli agent config --transport http \
  --url http://127.0.0.1:8787 --token-file ~/.config/openmesh/bridge-token
```

### 1.5 Audit and troubleshooting

Every **write** tool call is appended to the activity log with the agent source; reads are not
logged. Check the panel's Agents → Activity view or `GET /api/events?…` (see
[WEBSOCKET-API.md](../architecture/WEBSOCKET-API.md)) after a session. Responses never include
tokens or PSKs.

| Symptom | Cause | Fix |
| --- | --- | --- |
| `/mcp` returns 404 | HTTP MCP disabled | Set `enabled=true`, restart the bridge. |
| 401 JSON-RPC error | Wrong/missing bearer token | Use the bridge token; check the client sends `Authorization`. |
| `http_scope_read_only` | Write tool in read-only mode | Intentional default; set `allow_write=true` only after review. |
| `confirm_required` | Dangerous tool without confirmation | Re-issue with `"confirm": true` after a human agrees. |
| 429 / rate-limit error | Too many requests | Raise `rate_limit_per_min` modestly or back off the agent. |
| Session rejected | Too many sessions / oversized request | Lower concurrency; keep requests below 256 KiB. |

---

## 2. Gateways: signed webhooks in both directions

A gateway bridges **one channel** to any external service through an adapter you run. The bridge
speaks a generic, signed webhook protocol; your adapter talks to Matrix, an SMS API, SMTP/IMAP,
satellite or a custom script. This keeps vendor SDKs out of the bridge and puts provider
credentials in your adapter, not in OpenMesh.

> **Security boundary.** The bridged leg is **not end-to-end encrypted**: the bridge, adapter,
> provider and anyone with access to those systems can read the plaintext. Bridge only channels
> whose members accept that, and never bridge a channel carrying secrets or personal data you
> would not send over SMS/email.

### 2.1 Configuration file

`<data-dir>/gateways.json`, mode `0600`:

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
        "channel_ids": ["dba1337a96acc686"],
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

Rules:

* Only **env-var names** belong here. The API rejects inline secrets and unknown environment
  variable names when you set a connector; secrets are never written to config or logs.
* `channel_ids` empty means “all channels”; direct messages are bridged only with
  `"include_direct": true` (off by default — think before enabling it).
* `include_attachments` is metadata only in v0.7.0; bytes are never pushed through a webhook.
* `inbound.channel_id` is the channel external messages are injected into; `max_chars` is the
  inbound text cap (400 for SMS, 4 000-ish for chat services).
* The bridge exposes `POST /api/gateways/{id}/inbound` for adapters. The panel generates the exact
  URL for you; do not guess it.

Secrets on the bridge host (systemd example):

```ini
# /etc/systemd/system/openmesh-bridge.service.d/secrets.conf
[Service]
EnvironmentFile=/etc/openmesh/gateways.env
```

```sh
# /etc/openmesh/gateways.env  (root-owned, mode 0600)
OPENMESH_SMS_OUT_SECRET=<32+ random bytes, hex/base64>
OPENMESH_SMS_IN_SECRET=<different 32+ random bytes>
```

Generate secrets with `openssl rand -hex 32`. Use a **different** secret per direction and per
connector. After changing env vars, restart the bridge so the process sees them.

### 2.2 Outbound payload and HMAC verification

On each new channel message the bridge POSTs JSON to the connector's `outbound.url`:

```http
POST /openmesh HTTP/1.1
Content-Type: application/json
X-OpenMesh-Signature: sha256=<hex HMAC-SHA256(secret, raw_body)>

{"event":"message","connector":"sms-ops","channel_id":"dba1337a96acc686",
 "message_id":42,"from":"8f3a…","from_name":"Ada","text":"Cabin is fine",
 "timestamp_ms":1770000000000,"attachment":{"mime":"image/webp","size":1234}}
```

The signature is computed over the **raw request body bytes**, not a parsed/re-serialized version.
Compare it in constant time. Minimal adapter receiver:

```python
#!/usr/bin/env python3
import hashlib, hmac, json, os
from http.server import BaseHTTPRequestHandler, HTTPServer

SECRET = os.environ["OPENMESH_SMS_OUT_SECRET"].encode()

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        raw = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        got = self.headers.get("X-OpenMesh-Signature", "")
        want = "sha256=" + hmac.new(SECRET, raw, hashlib.sha256).hexdigest()
        if not hmac.compare_digest(got, want):
            self.send_response(401); self.end_headers(); return
        event = json.loads(raw)
        if event.get("connector") != "sms-ops":
            self.send_response(404); self.end_headers(); return
        print("send SMS to the on-call number:", event.get("text", "")[:400])
        self.send_response(204); self.end_headers()
        # Idempotency: key on event["message_id"]; retries reuse the same id.

HTTPServer(("127.0.0.1", 9101), Handler).serve_forever()
```

Verify by hand:

```sh
BODY='{"text":"test","from":"+4712345678","timestamp_ms":1770000000000}'
SIG=$(printf '%s' "$BODY" | openssl dgst -sha256 -hmac "$OPENMESH_SMS_IN_SECRET" -hex | sed 's/^.* //')
curl -sS -X POST "$API/api/gateways/sms-ops/inbound" \
  -H "X-OpenMesh-Signature: sha256=$SIG" -H 'Content-Type: application/json' \
  --data-binary "$BODY"
```

Outbound delivery is best-effort with bounded retries/backoff; exhausted retries are recorded as
activity-log entries (metadata only). Text is truncated to 4 000 characters.

### 2.3 Inbound injection and testing

Adapters POST the **same style of signature** over the raw body to the generated inbound URL:

```http
POST /api/gateways/sms-ops/inbound
X-OpenMesh-Signature: sha256=<hex HMAC-SHA256(inbound secret, raw_body)>

{"text":"On my way","from":"+4712345678","timestamp_ms":1770000000000}
```

The bridge verifies the signature, connector enablement and `max_chars`, applies a 30/min rate
limit, then injects the text into the configured channel attributed to the gateway. `from` is
**display text only** and is never trusted as an identity claim. Bad signature, missing secret,
disabled inbound and oversized text are rejected without injection; successful injections are
audited and appear on the mesh like any other channel message.

Manage connectors with the API (token required) or the panel:

```sh
curl -sS -H "Authorization: Bearer $TOKEN" "$API/api/gateways" | jq
curl -sS -X POST -H "Authorization: Bearer $TOKEN" "$API/api/gateways/sms-ops/test" | jq
curl -sS -X DELETE -H "Authorization: Bearer $TOKEN" "$API/api/gateways/sms-ops"
```

The CLI wrapper (secrets by env-var name, never argv values):

```sh
openmesh-cli gateways list
openmesh-cli gateways show sms-ops
openmesh-cli gateways set sms-ops --from-file ~/sms-gateway.json \
  --secret-env OUT=OPENMESH_SMS_OUT_SECRET --secret-env IN=OPENMESH_SMS_IN_SECRET
openmesh-cli gateways test sms-ops
openmesh-cli gateways remove sms-ops
```

`GET /api/gateways` returns redacted config plus runtime status (last success/failure/retries and
secret-presence booleans). `PUT /api/gateways/{id}` upserts; `DELETE` removes; every write route is
token-protected and audited.

### 2.4 Adapter recipe A — Matrix bot

Goal: mirror a channel into a Matrix room and let room members reply into the channel.

1. Create a dedicated Matrix bot account and invite it to the room. Keep its access token in the
   adapter's environment (`MATRIX_HOMESERVER`, `MATRIX_ACCESS_TOKEN`, `MATRIX_ROOM_ID`,
   `OPENMESH_SMS_OUT_SECRET`, `OPENMESH_SMS_IN_SECRET`). If the room uses Matrix E2EE, the bot must
   support encryption and the room members must accept the bridge/operator seeing plaintext.
2. Bridge **inbound** (Matrix → OpenMesh): poll `/sync` (or run a bot library) and POST each
   allowed sender's text to the generated inbound URL with a valid signature:

```python
import hashlib, hmac, os, requests

API = os.environ.get("OPENMESH_API", "http://127.0.0.1:8787")
IN_SECRET = os.environ["OPENMESH_SMS_IN_SECRET"].encode()
MATRIX = os.environ["MATRIX_HOMESERVER"].rstrip("/")
ROOM = os.environ["MATRIX_ROOM_ID"]
BOT_TOKEN = os.environ["MATRIX_ACCESS_TOKEN"]

def to_mesh(text, sender):
    body = json.dumps({"text": text[:400], "from": sender, "timestamp_ms": int(time.time()*1000)},
                      separators=(",", ":")).encode()
    sig = "sha256=" + hmac.new(IN_SECRET, body, hashlib.sha256).hexdigest()
    requests.post(f"{API}/api/gateways/matrix/inbound", data=body,
                  headers={"X-OpenMesh-Signature": sig, "Content-Type": "application/json"},
                  timeout=5).raise_for_status()

# Outbound: the bridge POSTs {"event":"message","text":...} to your adapter (see §2.2).
def to_matrix(text):
    txn = str(uuid.uuid4())
    requests.put(f"{MATRIX}/_matrix/client/v3/rooms/{ROOM}/send/m.room.message/{txn}",
                 headers={"Authorization": f"Bearer {BOT_TOKEN}"},
                 json={"msgtype": "m.text", "body": text}, timeout=10).raise_for_status()
```

3. Run the adapter beside the bridge (loopback), register its URL as the connector's `outbound.url`,
   keep only the intended room allow-listed, and test both directions. Apply a sender allow-list:
   `from` is display text, not an OpenMesh identity.
4. Matrix homeservers store plaintext for the bridged leg; run the bot on infrastructure you trust
   and do not bridge rooms with sensitive content.

### 2.5 Adapter recipe B — SMS provider webhook

Goal: forward channel text as SMS and let replies arrive on the mesh.

1. Choose a provider that offers an HTTP send API and an inbound webhook/callback. Put its
   credentials in the adapter (`SMS_API_URL`, `SMS_API_KEY`, `SMS_FROM`), not in
   `gateways.json`.
2. **Outbound** (OpenMesh → SMS): the adapter verifies `X-OpenMesh-Signature` exactly as in §2.2,
   then calls the provider API. Example generic call:

```python
def send_sms(text: str):
    requests.post(os.environ["SMS_API_URL"],
                  headers={"Authorization": f"Bearer {os.environ['SMS_API_KEY']}"},
                  json={"from": os.environ["SMS_FROM"], "to": os.environ["SMS_ONCALL"],
                        "text": text[:400]}, timeout=10).raise_for_status()
```

3. **Inbound** (SMS → OpenMesh): point the provider's callback at your adapter (public HTTPS, with
   the provider's signature/allow-list check where available), then normalize and forward the
   message to the OpenMesh inbound URL with an HMAC signature (§2.3). Set the connector's
   `inbound.max_chars` to **400**; strip MMS/media and forward text only. Use the sender's number
   as `from` display text and add an allow-list/replay window for `timestamp_ms`.
4. SMS is stored and readable by the carrier/provider: inform channel members. Never enable
   `include_direct` for SMS unless everyone involved expects SMS copies of direct messages.

### 2.6 Adapter recipe C — Email forwarder

Goal: forward bridged channel messages to an address and accept replies by email.

* **Outbound:** a Python adapter verifies the HMAC and sends with SMTP; recipient addresses come
  from the adapter's environment.

```python
import smtplib
from email.message import EmailMessage

def send_mail(subject, text):
    msg = EmailMessage()
    msg["From"] = os.environ["SMTP_FROM"]
    msg["To"] = os.environ["MAIL_TO"]
    msg["Subject"] = subject[:120]
    msg.set_content(text)
    with smtplib.SMTP_SSL(os.environ["SMTP_HOST"], int(os.environ.get("SMTP_PORT", "465"))) as s:
        s.login(os.environ["SMTP_USER"], os.environ["SMTP_PASSWORD"])
        s.send_message(msg)
```

* **Inbound:** configure an MTA alias/pipe or an IMAP poller to call a small script with the email
  body; the script posts it to the inbound URL with the inbound HMAC (§2.3), using the sender as
  display text and a 4 000-character cap. Reject HTML/attachments and quote-only replies; keep an
  allow-list of senders.
* Email is store-and-forward plaintext at several hops. Do not bridge channels with personal data
  or secrets; treat the mailbox as part of the security boundary.

---

## 3. Multiple backbone links

A bridge can join several backbones at once — separate communities, redundancy, or a failover link
for a remote site. Configuration is `<data-dir>/backbone-links.json` (mode `0600`):

```json
{"version":1,"links":[
 {"name":"oslo","url":"openmesh://backbone?host=oslo.example&port=9443&token=…&node=…","enabled":true},
 {"name":"cabin","url":"openmesh://backbone?host=cabin.example&port=9443&token=…&node=…","enabled":true}
]}
```

* Each enabled link has its own reconnect loop and backoff. The engine's `(src, seq)` dedup
  suppresses frames that arrive over more than one link, so a message is delivered once.
* The legacy single `join`/`backbone.json` path keeps working; a saved `backbone-links.json` takes
  precedence, and the legacy link is imported as `default` on first run.
* Manage links in the Backbone panel or with the API/CLI:

```sh
curl -sS -H "Authorization: Bearer $TOKEN" "$API/api/backbone/links" | jq
curl -sS -H "Authorization: Bearer $TOKEN" "$API/api/backbone" | jq '.links'

openmesh-cli backbone links list
openmesh-cli backbone links add --name oslo --url 'openmesh://backbone?…'
openmesh-cli backbone links remove cabin
```

* `GET /api/backbone/links` returns `name`, `mode:"client"`, `enabled`, `connected`, `last_error`
  and per-link stats; `PUT /api/backbone/links/{name}` and `DELETE` manage entries, with the link
  name included in status/peer tables. Tokens are redacted except in a locally generated invite
  link when you explicitly request it.
* Topology advice and loop safety live in [BACKBONE.md](../architecture/BACKBONE.md) and
  [backbone-exposure.md](backbone-exposure.md). Backbone-to-backbone peering and OBB/2 TLS are
  **not** in v0.7.0; each link is a client connection.

---

## 4. Security checklist

Before enabling any remote feature:

- [ ] Bridge HTTP listener bound to `127.0.0.1` (or a VPN interface), never `0.0.0.0` on the open
      internet.
- [ ] Bridge token file mode `0600`, strong/random, not in shell history, scripts or git; rotate
      after a machine loss.
- [ ] MCP HTTP: `enabled=true` only when needed, `allow_write=false` unless reviewed,
      `require_confirm=true`, small `rate_limit_per_min`/`max_sessions`.
- [ ] Remote MCP reached through SSH tunnel/WireGuard/Tailscale, not a public port.
- [ ] Gateway secrets only in environment variables; `gateways.json` mode `0600`; different
      secrets per connector and direction; unknown env-var names rejected.
- [ ] Adapter verifies the HMAC over raw bytes in constant time, checks `connector`, applies an
      idempotency key (`message_id`), and returns quickly.
- [ ] Inbound adapters validate their own provider (Matrix room allow-list, SMS provider
      signature/allow-list, email sender allow-list) and use `timestamp_ms` replay windows.
- [ ] Channel allow-list (`channel_ids`) narrow; `include_direct` remains false.
- [ ] Attachments stay metadata-only; do not build side channels that re-upload bytes.
- [ ] Activity log reviewed after agent sessions and gateway changes; connector status pills
      green; retries/exhaustion checked.
- [ ] Bridged channels are explicitly announced as **outside the E2E boundary**; members agree.
- [ ] Disable/delete unused connectors and disable `/mcp` when not in use.

---

## 5. Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| `/mcp` 404 | HTTP transport disabled | `enabled=true` in `mcp-http.json`, restart bridge. |
| MCP 401 | Missing/wrong bearer token | Use the bridge token; check proxy doesn't strip `Authorization`. |
| MCP read tools work, writes refuse | Read-only default | Set `allow_write=true` only deliberately; restart. |
| MCP 429 | Rate limit | Back off, or raise `rate_limit_per_min` modestly. |
| Gateway test 401/404 | Wrong inbound URL or disabled connector | Re-copy generated URL; enable connector. |
| Outbound adapter gets no calls | Connector disabled / channel filter excludes it / URL unreachable | Check status and channels; curl adapter from bridge host. |
| Adapter rejects signature | Body was parsed/re-encoded before HMAC, or wrong secret | HMAC the raw bytes; confirm the env var in the bridge process. |
| Inbound 401 | Wrong inbound secret, signature over modified body | Recompute over raw body with the inbound secret. |
| Inbound 400 | `max_chars` exceeded or malformed JSON | Shorten text; validate JSON. |
| Inbound 429 | 30/min rate limit | Batch/back off; do not disable the limit globally. |
| Duplicate messages across backbones | Two links delivering same frame | Dedup should hide it; verify both links share the same mesh/identity; report if persistent. |
| No activity entries | Only writes logged; reads not | Trigger a write; check Agents → Activity. |
| Secrets missing after restart | `EnvironmentFile` not loaded or wrong variable name | Fix env file; restart; check the connector's secret-presence booleans. |

When in doubt: disable the connector and set `enabled=false` for `/mcp`. Local mesh traffic is
unaffected.

---

## 6. Scope and disclaimers

v0.7.0 gateways are **text-only**, attachments are **metadata-only**, and connectors are generic
webhooks rather than vendor SDKs. Inbound attachment upload, OBB/2 TLS, backbone peering and
per-peer gateway identity are deferred. This guide is operational documentation, not legal or
security advice: bridging messages to SMS/email/satellite may create obligations under privacy,
wiretap, data-protection or provider terms, and the operator is responsible for the adapter, the
secrets, the channel membership and the content that leaves the mesh.

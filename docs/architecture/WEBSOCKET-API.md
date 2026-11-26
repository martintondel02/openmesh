# OpenMesh Bridge API — WebSocket + REST

**Status:** normative for v0.1.0
**Applies to:** `crates/openmesh-bridge` (server), `apps/web` (client), future mobile clients
**Base URL:** `http://127.0.0.1:8787` (default; configurable)

The bridge is a local daemon that owns the radio(s), the protocol engine and the message store.
Apps never talk to a radio directly on desktop; they talk to the bridge over loopback. This keeps
one implementation of storage/sync and lets multiple clients (web UI, CLI, future mobile) share
the same session.

---

## 1. Connection and auth

* WebSocket: `ws://127.0.0.1:8787/api/ws`
* REST: `http://127.0.0.1:8787/api/...`
* Auth: a random 32-byte token generated on first run and written to
  `~/.config/openmesh/bridge-token`. Clients send `Authorization: Bearer <token>` (REST) or
  `?token=<token>` (WebSocket, since browsers cannot set headers). Requests without a valid
  token MUST be rejected with HTTP 401 / WS close code 4401.
* `GET /api/health` is public (no token) and returns `{"ok":true,"version":"0.1.0"}`.
* The bridge MUST only bind loopback by default. Binding to `0.0.0.0` requires an explicit
  `--bind` flag and is documented as unsafe.

All JSON is UTF-8. Binary blobs (images, audio) are base64 inside JSON only for small payloads;
larger uploads use `POST /api/attachments` (raw bytes) and are referenced by id.

---

## 2. Envelope

Every WebSocket message (both directions) is a JSON object with:

```json
{ "v": 1, "type": "<type>", "id": "<optional client request id>", "data": { ... } }
```

* `v`: protocol version, currently `1`.
* Capability representation: `hello.node.caps` is a **u16 bitmask** (wire form);
  `node` events use a **string array** (`["gps","relay"]`) for readability. Clients must accept
  both forms wherever caps appear.
* `hello.node.public_key` is the bridge node's own 72-byte public identity (base64) or `null` if
  the bridge does not expose it. When null, clients cannot render their own contact QR and must
  degrade gracefully (v1 bridge behaviour).
* Own-identity reset is intentionally not an API operation: it requires restarting the bridge
  with `--reset-identity` so all derived state is regenerated atomically.
* `type`: discriminator (see tables).
* `id`: optional client-generated string; the server echoes it in the response for
  request/response commands. Server-pushed events omit it.
* `data`: type-specific object. May be absent for commands with no arguments.

Errors are always:

```json
{ "v": 1, "type": "error", "id": "<echoed request id>", "data": { "code": "bad_request", "message": "..." } }
```

Error codes: `bad_request`, `unauthorized`, `unknown_peer`, `not_found`, `too_large`,
`radio_busy`, `internal`.

---

## 3. Server → client events

Events are pushed after the client sends `hello`-equivalent initial sync (see §4.1).

### 3.1 `hello`

Sent once when the connection is accepted.

```json
{ "v":1, "type":"hello", "data": {
  "bridge_version": "0.1.0",
  "protocol": "omp/1",
  "node": { "node_id": "ab12…", "name": "martin-phone", "class": "client", "caps": 81,
            "public_key": null },
  "radios": [ { "id": "sim0", "kind": "sim", "label": "Simulator", "connected": true,
                "profile": "MEDIUM_FAST", "region": "EU868" } ],
  "features": ["images","voice","map","groups","store_forward"],
  "store": { "messages": 0, "contacts": 0, "channels": 0 }
} }
```

### 3.2 `status`

Periodic (1 Hz) bridge/radio status.

```json
{ "v":1, "type":"status", "data": {
  "uptime_s": 1234,
  "radio": { "connected": true, "tx_queue": 0, "airtime_used_pct": 0.4,
             "rssi": -87, "snr": 6.5, "frequency_mhz": 869.525, "tx_power_dbm": 14 },
  "mesh": { "tx_frames": 120, "rx_frames": 240, "forwarded": 31, "dropped": 4,
            "known_nodes": 12, "stored_envelopes": 3, "duty_cycle_pct": 0.4 },
  "storage": { "used_bytes": 1048576, "db_path": "~/.local/share/openmesh/openmesh.redb" }
} }
```

### 3.3 `node`

Emitted when a node is discovered or its announce changes.

```json
{ "v":1, "type":"node", "data": {
  "node_id": "ab12…", "name": "solar-ridge", "class": "repeater", "caps": ["relay","store","gps"],
  "hops": 2, "last_seen_ms": 1760000000000, "rssi": -101, "snr": 4.25,
  "position": { "lat": 59.9137, "lon": 10.7522, "alt_m": 45, "accuracy_m": 8,
                "timestamp_ms": 1760000000000 },
  "battery_pct": 87, "is_contact": false, "public_key": "…base64 72-byte identity…"
} }
```

`caps` is an array of strings (`gps`, `wifi`, `ethernet`, `store`, `relay`, `ble`, `usb`,
`screen`, `audio_out`, `solar`) for readability.

### 3.4 `message`

A direct message, incoming or an echo of a locally-sent one (echoes carry `outgoing: true`).

```json
{ "v":1, "type":"message", "data": {
  "msg_id": 123456789,
  "peer": "ab12…",                  // conversation partner (the other node)
  "from": "ab12…", "to": "cd34…",
  "kind": "text",                   // text | position | attachment | telemetry
  "text": "hello mesh",
  "sent_at_ms": 1760000000000,
  "received_at_ms": 1760000000050,
  "outgoing": false,
  "delivered": true,
  "read": false,
  "reply_to": null,
  "thread": null,
  "attachment": {
    "id": "att_7f3a",               // bridge attachment id (see §6)
    "mime": "image/jpeg", "size": 5120, "sha256": "…hex…",
    "duration_ms": null, "name": "trail.jpg", "progress": 1.0
  },
  "position": null
} }
```

* `attachment.progress` is 0..1 while a resource is transferring and 1.0 when complete.
  The bridge emits a `message_update` event with the same `msg_id` when progress changes.
* `message_update` carries **only changed fields plus `msg_id`**; clients merge it into the
  existing message. A failed send stays failed until a `delivered`/`read` ack arrives or the
  client retries.
* Channel read state (`mark_group_read`) is client-side only in v1: there is no per-member read
  receipt for groups, because group frames are broadcast and ack-free.

### 3.5 `message_update`

Same shape as `message`; only changed fields are present plus `msg_id`.

### 3.6 `group_message`

```json
{ "v":1, "type":"group_message", "data": {
  "msg_id": 123456790, "channel_id": "ff00…", "channel_name": "family",
  "from": "ab12…", "from_name": "martin", "sender_key": "…base64…",
  "kind": "text", "text": "on my way", "sent_at_ms": 1760000000000,
  "outgoing": false, "signature_valid": true,
  "attachment": null, "position": null
} }
```

### 3.7 `ack`

```json
{ "v":1, "type":"ack", "data": { "msg_id": 123456789, "peer": "ab12…",
  "kind": "delivered", "at_ms": 1760000001234 } }
```

### 3.8 `position`

```json
{ "v":1, "type":"position", "data": { "node_id": "ab12…", "channel_id": null,
  "position": { "lat": 59.9137, "lon": 10.7522, "alt_m": 45, "accuracy_m": 8,
                "speed_cm_s": 120, "heading_deg": 180, "timestamp_ms": 1760000000000 } } }
```

### 3.9 `telemetry`

```json
{ "v":1, "type":"telemetry", "data": { "node_id": "ab12…", "battery_mv": 3980,
  "battery_pct": 82, "uptime_s": 86400, "airtime_used_ms": 42000, "temperatur_c": 21,
  "solar_mv": 5100, "timestamp_ms": 1760000000000 } }
```

### 3.10 `store_offer`

```json
{ "v":1, "type":"store_offer", "data": { "relay": "ab12…", "count": 3 } }
```

### 3.11 `resource_progress`

```json
{ "v":1, "type":"resource_progress", "data": { "peer": "ab12…", "msg_id": 123456789,
  "res_id": 42, "received": 12, "total": 29, "direction": "in" } }
```

### 3.12 `backbone_status` and `backbone_peer`

Backbone-mode events are specified in [BACKBONE.md](BACKBONE.md) §7.2 and are only emitted when
the bridge runs with a backbone interface (or has an outbound backbone link). Clients MUST ignore
them when the `hello` payload does not list `"backbone"` in `features`.

### 3.13 `config`

Full configuration snapshot (reply to `get_config` and pushed after `set_config`).

```json
{ "v":1, "type":"config", "data": {
  "node": { "name": "martin-phone", "class": "client", "announce_interval_s": 1800 },
  "radio": { "profile": "MEDIUM_FAST", "region": "EU868", "tx_power_dbm": 14,
             "frequency_mhz": 869.525, "interface": "sim" },
  "features": { "auto_announce": true, "store_forward": true, "position_sharing": true },
  "channels": [ { "channel_id": "ff00…", "name": "family", "psk_set": true } ]
} }
```

PSKs are never returned; only whether one is set.

---

## 4. Client → server commands

### 4.1 Initial sync

```json
{ "v":1, "type":"sync", "id":"s1", "data": { "since_ms": 0 } }
```

The server replies with a sequence of `node`, `message`, `group_message`, `config` events, then

```json
{ "v":1, "type":"sync_done", "id":"s1", "data": { "messages": 120, "nodes": 14 } }
```

### 4.2 Messages

| type | data | effect |
| --- | --- | --- |
| `send_message` | `{ "to": "<node_id>", "text": "...", "reply_to": null, "attachment_id": null }` | Encrypt and send. Returns `{"msg_id": n}` via `send_result`. |
| `send_group_message` | `{ "channel_id": "...", "text": "...", "attachment_id": null }` | Channel message. |
| `send_position` | `{ "to": "<node_id>" }` or `{ "channel_id": "..." }` | Uses the bridge's configured position (or `{"position": {...}}` to override once). |
| `mark_read` | `{ "msg_id": n }` | Sends a `read` ack when possible. |
| `retry` | `{ "msg_id": n }` | Re-send an unacked message. |
| `delete_message` | `{ "msg_id": n }` | Local-only delete. |

`send_*` commands get a response:

```json
{ "v":1, "type":"send_result", "id":"m1", "data": { "msg_id": 123456789,
  "est_airtime_ms": 610, "queued": true } }
```

### 4.3 Contacts

| type | data |
| --- | --- |
| `add_contact` | `{ "node_id": "...", "name": "Alice", "identity": "<base64 72B>", "verify_key": true }` |
| `remove_contact` | `{ "node_id": "..." }` |
| `get_contacts` | `{}` → replies `contacts` event `{ "contacts": [ { "node_id","name","identity","last_seen_ms" } ] }` |
| `contact_qr` | `{ "node_id": "..." }` → `contact_url` event with `openmesh://contact?...` |

### 4.4 Channels

| type | data |
| --- | --- |
| `create_channel` | `{ "name": "...", "psk": "<base64 32B>" }` (generates PSK if absent) → `channel_created` `{ "channel_id": "...", "name": "...", "psk": "<base64>" }` (PSK returned **once**, never again) |
| `join_channel` | `{ "name": "...", "psk": "<base64 32B>" }` → `channel_created` (same shape, `psk` omitted) and/or a `channels` event; clients should accept both |
| `leave_channel` | `{ "channel_id": "..." }` |
| `get_channels` | `{}` → `channels` event |
| `channel_invite` | `{ "channel_id": "..." }` → `channel_url` (`openmesh://channel?...`) |

### 4.5 Configuration and diagnostics

| type | data | effect |
| --- | --- | --- |
| `get_config` | `{}` | → `config` |
| `set_config` | partial config object | Applies at runtime; radio-only changes go to the radio. |
| `announce` | `{}` | Force an announce now. |
| `get_stats` | `{}` | → `stats` event (same shape as `status.mesh` + per-link counters). |
| `get_nodes` | `{}` | → `nodes` event `{ "nodes": [...] }`. |
| `get_messages` | `{ "peer": "...", "before_ms": null, "limit": 100 }` | → `messages` event `{ "messages": [...] }`. |
| `get_nodes_map` | `{}` | → `nodes` (clients can render from `node` events; this is the bulk fetch). |
| `radio_scan` | `{}` | Reserved; → `error{code:"radio_busy"}` when unsupported. |
| `get_backbone` | `{}` | → `backbone_status` + `config` (backbone section). See [BACKBONE.md](BACKBONE.md). |
| `set_backbone_config` | partial config object | Applies live where possible; returns `config`. |
| `backbone_kick` | `{ "node_id": "..." }` | Disconnects one backbone peer. |
| `backbone_rotate_token` | `{}` | Rotates the backbone token; returns it **once** in `backbone_token`. |

### 4.6 Example exchange

```json
// client
{"v":1,"type":"send_message","id":"m1","data":{"to":"cd34…","text":"hi"}}
// server
{"v":1,"type":"send_result","id":"m1","data":{"msg_id":7,"est_airtime_ms":610,"queued":true}}
{"v":1,"type":"message","data":{"msg_id":7,"peer":"cd34…","from":"ab12…","to":"cd34…",
 "kind":"text","text":"hi","sent_at_ms":1760000000000,"outgoing":true,"delivered":false}}
// later
{"v":1,"type":"ack","data":{"msg_id":7,"peer":"cd34…","kind":"delivered","at_ms":1760000001234}}
```

---

## 5. REST endpoints

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/api/health` | Public liveness probe. |
| GET | `/api/status` | Same payload as the `status` event. |
| GET | `/api/config` | Same as the `config` event. |
| GET | `/api/nodes` | Array of node objects. |
| GET | `/api/nodes/:id` | One node. |
| GET | `/api/messages?peer=<id>&limit=100&before_ms=` | Direct message history. |
| GET | `/api/channels` | Channels (no PSKs). |
| GET | `/api/messages/channel/:id?limit=100` | Group history. |
| GET | `/api/contacts` | Contacts. |
| GET | `/api/attachments/:id` | Raw bytes with `Content-Type` from the stored metadata. |
| POST | `/api/attachments` | Raw body upload (`Content-Type` required, `X-OpenMesh-Filename` optional). Returns `{ "id": "att_…", "size": n, "sha256": "…" }`. Max 32 MiB. |
| GET | `/api/events` | Server-Sent Events stream (optional convenience; WS preferred). |
| GET | `/` | Serves the built web app (`apps/web/dist`) when present. |
| GET | `/api/gateways` | Webhook gateway connectors with redacted config + delivery/inbound status. |
| PUT | `/api/gateways/:id` | Upsert one connector (env-var secret names only). |
| DELETE | `/api/gateways/:id` | Remove one connector. |
| POST | `/api/gateways/:id/test` | Send one signed test payload to the outbound webhook. |
| POST | `/api/gateways/:id/inbound` | HMAC-only inbound adapter callback (no bearer token); injects a channel message. |
| GET | `/api/backbone/links` | Outbound backbone links with per-link status (tokens redacted). |
| PUT | `/api/backbone/links/:name` | Add/update an outbound link (`openmesh://backbone?...` URL). |
| DELETE | `/api/backbone/links/:name` | Remove an outbound link. |

---

## 6. Attachments

1. The client compresses images / encodes voice locally (target size from `est_airtime_ms`
   budgets; the UI shows estimated airtime before sending).
2. `POST /api/attachments` with raw bytes → `{ id, size, sha256 }`.
3. `send_message` with `attachment_id` (image/voice) or `send_group_message` likewise.
4. The bridge creates an `AttachmentRef`, sends a manifest message, streams chunks, and emits
   `resource_progress` events. Outgoing message objects show `attachment.progress`.
5. Incoming attachments are stored by the bridge and served from `/api/attachments/:id`; the
   `message` event includes the id and metadata.

Attachment ids are `att_<12 hex>`; storage lives under `~/.local/share/openmesh/attachments/`.

---

## 7. Versioning

* The `v` field gates breaking changes to this API.
* New event/command types MAY be added at any time; clients MUST ignore unknown `type`s.
* Fields MAY be added to `data` objects; clients MUST ignore unknown fields.
* Removing or renaming a field requires a `v` bump and a bridge release note.


---

## 8. Backbone control surface

Backbone mode (always-on TCP relay with a public port and a control panel) adds a small set of
REST endpoints and WS events on top of this API. The full contract — OBB/1 handshake, peer
accounting, configuration fields, setup payload and security model — lives in
[BACKBONE.md](BACKBONE.md).

| Method | Path | Auth | Purpose |
| --- | --- | --- | --- |
| GET | `/api/backbone` | yes | Backbone config + live totals (token redacted) |
| GET | `/api/backbone/peers` | yes | Connected peer list with per-peer counters |
| POST | `/api/backbone/config` | yes | Partial live update (enable/bind/port/max_peers/allowlist/store_forward/public_host) |
| POST | `/api/backbone/token/rotate` | yes | New token returned once; old token invalidated |
| DELETE | `/api/backbone/peers/:node_id` | yes | Kick a peer |
| GET | `/api/backbone/setup` | yes | Onboarding steps, firewall commands, systemd unit, connection string |
| GET | `/api/backbone/qr.svg` | yes | QR code of the connection string |
| GET | `/api/backbone/links` | yes | Outbound links with per-link status (tokens redacted) |
| PUT | `/api/backbone/links/:name` | yes | Add/update an outbound link |
| DELETE | `/api/backbone/links/:name` | yes | Remove an outbound link |

When the bridge is not in backbone mode these endpoints return
`{"enabled": false}` (HTTP 200) rather than 404, so the panel can offer to enable backbone mode
with one click (`POST /api/backbone/config {"enabled": true}`). Outbound links work in any mode
and are described in [BACKBONE.md §7b](BACKBONE.md).

## 9. Gateways and remote MCP

Webhook gateways bridge channels to external services through signed HTTP adapters; remote MCP
over HTTP is opt-in on the same listener at `/mcp`. Both are disabled by default and documented in
[remote-operations.md](../guides/remote-operations.md) (operator guide), ADR-0012 and the machine
contract `docs/api/openapi.json`. Security rules: env-only secrets, HMAC-signed inbound callbacks,
loopback default, read-only MCP scope unless explicitly enabled, and activity-log audit for every
write. A gateway leg is no longer end-to-end encrypted — its operator sees the bridged plaintext.

# Gateways and remote agents

OpenMesh normally keeps messages inside your own radios. v0.7.0 adds two deliberate ways to reach
outside that circle: **gateways** that bridge a channel to another service (SMS, Matrix, email,
satellite), and an optional **remote MCP server** that lets an AI agent on another machine operate
your node. Both are off by default, both are for advanced operators, and both have clear security
trade-offs you should understand before enabling them.

## What a gateway is

A **gateway** (also called a connector) is a small bridge between one OpenMesh **channel** and an
outside service. It does not talk to that service directly: you run an **adapter** — a short script
or bot you control — and the gateway exchanges signed webhook messages with it.

* **Outbound:** when someone posts in the bridged channel, the bridge sends an HTTP `POST` to your
  adapter with the message text, sender name, channel id, timestamp and attachment **metadata**.
  Your adapter then does the service-specific work: send an SMS, post to a Matrix room, email it.
* **Inbound:** your adapter sends a signed HTTP `POST` to the bridge's inbound route. The bridge
  injects it into the channel as a normal message attributed to the gateway (for example
  “SMS gateway”), so everyone on the mesh sees it.

Why use one? A family channel can reach someone with only a basic phone (SMS), a team can follow
along in Matrix, or a remote cabin can be monitored by email. The gateway is the door; the adapter
is what walks through it.

The word **webhook** simply means “an HTTP POST that one program makes to another when something
happens”. The **signature** is an HMAC-SHA256 checksum made with a shared secret, so the receiver
can prove the request really came from your bridge and was not changed in transit.

## What leaves the mesh (and what does not)

This is the part that matters most.

* Messages **inside** OpenMesh are end-to-end encrypted between members.
* A gateway deliberately **breaks that end-to-end protection for the bridged leg**. The adapter,
  the SMS provider, the Matrix server and anyone who can read those systems see the plaintext of
  messages sent to that channel. The gateway operator can read them too.
* Only the bridged channel is affected. Direct messages and other channels stay encrypted.
* **Remote MCP** is different: it does not forward messages to a third party, but an AI agent with
  a valid token can read and (if you allow writes) change your node. Treat the token like a
  password.

Never bridge a channel that carries secrets, and never assume an outside service is private just
because it is “only email” or “only SMS”. Providers store messages.

## Remote MCP over HTTP

The MCP server that AI agents use can run in two ways:

* **stdio (default, local):** your agent launches `openmesh-bridge mcp` on the same machine. Nothing
  listens on the network.
* **HTTP (opt-in, v0.7.0):** the bridge exposes `/mcp` on its existing HTTP listener so an agent on
  another machine, or reached through a private tunnel, can connect. This is **read-only by
  default**; write tools are refused unless you explicitly allow them, and dangerous tools still
  require a `confirm: true` acknowledgement.

The HTTP transport is controlled by `<data-dir>/mcp-http.json` and is **disabled** by default
(`/mcp` returns 404). Authentication uses the same bridge API bearer token as the panel and CLI.
Every write an agent makes is recorded in the Activity log with an `agent:` source.

Safety rules:

1. Keep it on **loopback** (`127.0.0.1`) or reach it through a VPN or SSH tunnel. Do not port-forward
   `/mcp` to the public internet.
2. Use a long random token, keep the token file at mode `0600`, and rotate it if it may have leaked.
3. Leave `allow_write` off until you understand exactly which tools your agent needs.
4. Read the Activity log after an agent session; reads are not logged, writes are.

The operator guide has copy-paste JSON-RPC `curl` sessions, tunnel patterns and the full scope
rules: [Remote operations](../../docs/guides/remote-operations.md).

## How to enable, test and disable a gateway

1. Pick the channel and the service, and write or install an adapter first.
2. Store the shared secrets in **environment variables** on the bridge host (for example
   `OPENMESH_SMS_OUT_SECRET`). OpenMesh only stores the *name* of the variable, never the value.
3. In the panel, open **Backbone/Agents → Gateways** (or use `openmesh-cli gateways set`). Add the
   connector: adapter URL, secret variable names, channels to bridge, and whether inbound is
   enabled. Inbound gets a generated URL you copy into your adapter.
4. Use **Test** (`POST /api/gateways/{id}/test`) to send a signed test payload to your adapter.
5. Post in the bridged channel and confirm your adapter receives it; send an inbound test and confirm
   it appears on the mesh.
6. To pause, toggle the connector **off**. To stop for good, delete it. Rotating the secret means
   changing the environment variable on both sides and updating the connector if the variable name
   changes.

Only an operator with the API token can change connectors. The API redacts secret values and
reports only whether each referenced environment variable is present.

## Limits in v0.7.0

* **Text only.** Webhooks carry text. Attachments are represented by **metadata only** (MIME type
  and size); v0.7.0 does not push image or voice bytes through a gateway, and inbound attachment
  upload is not supported.
* **Outbound text is truncated to 4 000 characters.** Inbound text is capped by the connector's
  `max_chars` (for SMS, 400 is a sensible value).
* Delivery is **best-effort with bounded retries and backoff**. If your adapter is down, messages
  are retried, then given up on; failures appear in the Activity log (metadata only).
* Inbound is rate limited (30 requests per minute) and requires a valid signature. Bad signatures,
  missing secrets and disabled connectors are rejected without injecting anything.
* **No vendor SDKs yet.** Matrix, email, SMS and satellite services go through adapters you run;
  OBB/2 TLS peering and attachment push are deferred.
* A bridge can join **several backbones** at once for redundancy; duplicate frames are suppressed
  by the normal `(src, seq)` dedup.

## Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| Test button says 401/404 | Wrong/missing inbound URL, or connector disabled | Re-copy the generated inbound URL; enable the connector. |
| Adapter receives nothing | Gateway disabled, wrong channel filter, or adapter URL unreachable | Check the connector's channels, status pill and last error; curl the adapter URL from the bridge host. |
| Inbound message not visible | Bad HMAC, missing secret env var, or over `max_chars` | Verify the signature over the **raw body**, check `env` of the bridge process, shorten the text. |
| `401` when calling `/mcp` | Missing/incorrect bearer token, or HTTP MCP disabled | Use the bridge token; confirm `enabled=true` and restart the bridge. |
| Agent can read but not write | Read-only default | Only if you intend it: set `allow_write=true` and restart. |
| Duplicate messages with two backbones | Both links deliver the same frame | Expected dedup should hide this; report if duplicates persist in the UI. |
| Activity log empty | Reads are not logged | Perform a write; only writes are recorded. |

If you are unsure, disable the connector and stop remote MCP access first. Turning a gateway off
never affects normal mesh traffic; the bridged channel simply stops crossing to the outside
service.

## Related topics

[Run a backbone](backbone.md) · [Use your AI agent](agents.md) ·
[Security and privacy](security.md) · [When something is broken](troubleshooting.md)

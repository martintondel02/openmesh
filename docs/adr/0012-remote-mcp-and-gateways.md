# ADR 0012 — Remote MCP and gateway architecture

* **Status:** accepted (v0.7.0)
* **Date:** 2026-11-26
* **Decision record:** MADR-style; referenced from
  [remote-operations.md](../design/remote-operations.md) (implementation contract) and the
  operator guide [Remote operations](../guides/remote-operations.md)
* **Related:** [ADR 0005](0005-companion-radio-architecture.md) (bridge/radio boundary),
  [ADR 0009](0009-backbone-mode.md) (backbone), [ADR 0010](0010-ai-native-and-beginner-ux.md)
  (AI-native UX, activity log), [ADR 0011](0011-regulatory-override.md) (write-tool confirmation
  and audit precedent)

## 1. Context

OpenMesh is local-first: an IO-free engine, a bridge that owns the store and radio, and clients
(web, CLI, Android) that talk to the bridge. Its MCP server was deliberately stdio-only: an agent
launches `openmesh-bridge mcp` locally, so there is no network listener and no remote attack
surface. The activity log records writes, dangerous tools need `confirm: true`, and a read-only
environment switch exists.

v0.7.0 adds two operator needs that pull in the opposite direction:

1. **Agents and operators are increasingly remote.** A VPS-hosted bridge, a cabin node or a
   community relay is administered over SSH, a VPN or a tunnel. Requiring the MCP client to run on
   the bridge host forces awkward workarounds (SSH into the host, running an agent inside a
   container, exposing the REST API to an ad-hoc proxy).
2. **Communities already live in other tools.** Users want a family channel to reach a phone by
   SMS, a team room in Matrix, or a mailbox for a remote site. Building and maintaining a Matrix
   SDK, an SMS SDK and an SMTP implementation in the bridge would add large dependencies, provider
   credentials and a permanent compatibility burden to a component whose security surface should
   stay small.

At the same time, both features create obvious risks: a remote MCP token is remote control of a
node (including message sending and config changes), and a gateway deliberately converts
end-to-end-encrypted channel content into plaintext for an external service. The architecture has
to make the **secure path the default**, make the trade-off explicit, and keep adapters outside the
trusted core.

## 2. Decision

1. **MCP stdio remains the default and recommended transport.** The local server is unchanged; no
   feature is gated behind HTTP and no tool is removed from stdio.
2. **MCP over HTTP is opt-in, loopback-first, authenticated and read-only by default.** It reuses
   the bridge's existing HTTP listener and bearer token, is disabled unless
   `<data-dir>/mcp-http.json` enables it (`404` otherwise), rejects missing/wrong tokens with 401
   before any tool execution, rate-limits per token/session and bounds concurrent sessions.
   `allow_write=false` refuses write tools with `http_scope_read_only`; enabling writes still
   requires `require_confirm=true` for dangerous tools. Every write is attributed in the activity
   log (`agent:<name>` or `agent:mcp-http`). Clients are expected to reach it over loopback, SSH
   tunnels or a VPN — the bridge does not ship a public-exposure mode.
3. **Gateways are generic, signed webhooks rather than vendor SDKs.** A gateway is a connector
   between one channel and an adapter the operator runs. Outbound: signed JSON POSTs on channel
   messages (text + attachment metadata only, bounded retries/backoff, activity-log outcomes).
   Inbound: a signed POST to `/api/gateways/{id}/inbound` that injects a channel message
   attributed to the gateway, with a display-only `from`, `max_chars`, HMAC verification and a
   30/min rate limit. Matrix, SMS, email, satellite and custom scripts are all adapters; v0.7.0
   ships no vendor-specific connector in the bridge.
4. **Secrets never enter OpenMesh configuration.** `gateways.json` stores environment-variable
   *names* only; the API rejects inline secrets and unknown env names; logs and API output redact
   secrets and expose presence booleans at most.
5. **The operator-facing UI and API make the boundary visible.** A Gateways card (advanced only)
   shows status, last success/failure/retries, generated inbound URLs and a test button; the help
   topic and guide state plainly that a bridged leg is outside E2EE and that the gateway operator
   can read bridged plaintext.
6. **Multiple outbound backbone links are supported client-side.** A bridge can join several
   OBB/1 backbones with per-link reconnect/backoff; the engine's `(src, seq)` dedup suppresses
   duplicates across links. The legacy single-link path keeps working and is imported as
   `default`. Peer/token redaction follows the existing backbone rules.
7. **Write scopes and confirmation stay centralized.** Remote MCP and gateway injection do not
   invent a second permission model: they reuse the REST token, the existing dangerous-tool
   `confirm` rule and the activity log. Gateways are channel-scoped, never identity claims.

## 3. Alternatives considered

| Option | Why not |
| --- | --- |
| **Keep MCP stdio-only** | Safest, but forces agents onto the bridge host and blocks the common "administer a VPS/cabin node from my laptop or CI" workflow; users would expose the REST API through ad-hoc proxies with no scope rules or audit. |
| **MCP HTTP enabled by default and/or bound publicly** | Turns a local-first mesh node into a remotely controllable service by default. Token scope is too broad for an accidental public bind; the support and liability burden would be high. |
| **Remote MCP with its own token system and scopes** | Duplicates the existing token/scope/audit model and creates drift; reusing the bridge token plus read-only default and confirmation is simpler and already understood by operators. |
| **Direct Matrix/email/SMS vendor SDKs in the bridge** | Large dependencies, provider-specific credentials and permanent maintenance; provider API churn would touch the trusted core; most services already offer webhooks/HTTP APIs, so adapters are the better boundary. |
| **No gateways at all** | Leaves the most requested resilience feature (reach someone outside the mesh in an emergency) unimplemented and pushes users to unsafe unofficial forks. |
| **Bridge channels through a hosted relay** | Creates a new central trust point and a data honeypot; the project cannot operate or secure such a service, and it contradicts the local-first model. |
| **Backbone-to-backbone peering / OBB/2 now** | Requires per-peer identity, TLS, loop prevention and routing semantics that need their own design; client multi-link covers the redundancy need in v0.7.0 with existing OBB/1. |

## 4. Consequences

**Positive**

* Operators can use remote agents and external services without weakening the default local path;
  the secure configuration (loopback, read-only, no connector) is what a fresh install gets.
* The bridge keeps a small dependency and secret surface; adapters live in operator-controlled
  processes and can be audited, replaced or run on separate hosts.
* One permission/audit model (token, `confirm`, activity log) applies to every remote write path.
* Clients can join several backbones for redundancy and community separation without protocol
  changes.

**Negative / risks**

* **Bridging breaks E2EE for that leg.** Channel text reaches the adapter, provider and any reader
  of those systems; the operator and channel members must accept that. Mitigation: explicit
  documentation, advanced-only UI, channel allow-lists and the option to keep attachments
  metadata-only.
* **Exfiltration and prompt-injection risk.** A remote agent token, or an adapter with broad
  credentials, could leak messages or be manipulated. Mitigation: read-only default, token-file
  handling, rate limits, scoped adapters, audit and the option to disable either feature.
* **Token scope is coarse.** The bridge token is the same one that controls the REST API; a remote
  MCP client with `allow_write=true` can do anything a human operator can. Mitigation: keep writes
  off, rotate tokens, use separate deployments, review activity.
* **Public-exposure mistakes.** A user may still port-forward the bridge despite guidance.
  Mitigation: loopback defaults, 404 when MCP is disabled, no public-bind mode, warnings in docs and
  help.
* **Operational drag.** Gateways require an adapter, an HMAC secret pair, a reachable endpoint and
  provider credentials; retries can duplicate sends, so adapters must be idempotent on
  `message_id`.
* **Liability.** Bridged content may create privacy/wiretap/provider-terms obligations; OpenMesh
  provides the transport, not compliance.

## 5. Non-goals for v0.7.0

* OBB/2 TLS, per-peer backbone identity and backbone-to-backbone peering (deferred to a dedicated
  design/ADR).
* Attachment **bytes** through gateways, and inbound attachment upload (metadata only for now).
* Vendor-specific Matrix/email/SMS/satellite connectors inside the bridge, and any hosted relay.
* Multi-user accounts, fine-grained per-token scopes, or a second permission model for gateways.
* Public-internet binding for `/mcp`; operators must provide a tunnel/VPN/reverse proxy.

## 6. Compliance and documentation

* No wire-format change and no golden-vector change; OMP/1 resources and attachment MIME/bytes stay
  as specified in [PROTOCOL.md](../architecture/PROTOCOL.md).
* The normative behaviour is in [remote-operations.md](../design/remote-operations.md); the help
  topic `gateways-and-remote-agents` and the
  [operator guide](../guides/remote-operations.md) carry the user-facing warnings.
* This ADR does not claim that any adapter, provider or gateway deployment is compliant with local
  privacy/telecom law; operators are responsible for their choice of service and content.

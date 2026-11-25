# ADR 0009 — Backbone mode: authenticated TCP relay + local control panel

* **Status:** accepted (v0.2.0)
* **Date:** 2026-11-25
* **Decision record:** MADR-style; new decision not summarised in
  [ARCHITECTURE.md §4](../architecture/ARCHITECTURE.md#4-key-decisions) yet (suggested row:
  "Backbone mode | authenticated TCP relay + loopback control panel | ADR 0009")
* **Related:** [BACKBONE.md](../architecture/BACKBONE.md),
  [INSTALL.md](../INSTALL.md),
  [backbone deployment & operations](../operations/backbone-deployment.md),
  [ADR 0001](0001-own-protocol-vs-reticulum.md), [ADR 0005](0005-companion-radio-architecture.md),
  [ADR 0006](0006-web-first-clients-tauri-native-android.md),
  [ADR 0008](0008-agpl-3-0-licensing.md)

## 1. Context

OpenMesh v0.1.0 is a local-first mesh: radios talk LoRa, a local bridge relays on UDP/TCP within
a LAN, and clients talk to their own bridge. That works for a valley, a site or a small
community, but it does not connect **disjoint meshes**. Real deployments kept hitting the same
wall:

* a hiking group's radios are in the mountains; their families are in the city with internet but
  no radio path;
* two communities 200 km apart both have LoRa meshes but no line-of-sight between them;
* a mesh's only internet-capable node is a phone with a hotspot, and there is no way for a
  gateway on a VPS to join the mesh;
* NAT/CGNAT, changing IPs and firewalls make direct peer-to-peer mesh links fragile, while every
  useful network (and every hostile network) has a TCP route through a server somewhere.

The obvious answers were unsatisfying:

1. **Raw unauthenticated TCP bridge links** (`--tcp-listen`/`--tcp-peer`) already existed for WAN
   bridging, but they are a pre-v0.2 length-prefixed frame stream with no authentication. Putting
   one on a public port invites arbitrary frame injection, and there is no operator UI.
2. **MQTT broker** in front of the mesh. MQTT is a mature transport but adds a broker dependency,
   a topic namespace, its own auth model, and a lot of protocol surface for a mesh that already
   has a frame format. It does not solve "how do I join two OMP meshes" without a convention layer
   nobody can audit. It also makes the bridge a second-class citizen of a third-party protocol.
3. **Full VPN (WireGuard/Tailscale) only.** Excellent for security, but requiring every joining
   gateway to install and manage a VPN is too much friction for a hobbyist mesh — and the VPN
   still needs a way to know who is a legitimate mesh peer vs a random node on the VPN.
4. **Reticulum-style transport nodes / links.** We studied Reticulum deeply (ADR 0001). Its
   announce/path/link machinery is powerful, but porting it to run on our bridge would add the
   exact handshake/link cost we rejected for the LoRa side, and its license prevents code reuse.
5. **Federated "mesh" as a hosted cloud service.** Rejected by the project's no-account,
   no-central-service stance; users must be able to run the joining point themselves.

What operators needed was concrete: **one binary that can be put on a VPS/Raspberry Pi, opened on
one TCP port, and joined by gateways/repeaters over IP with a shared secret, plus a UI that
explains how to do it.** That is "backbone mode": the tower-free tower.

Security constraints that shaped the design:

* message payloads are already end-to-end encrypted (OMP/1); the backbone only relays ciphertext
  and routing metadata, so transport confidentiality is about *metadata* and *frame injection*,
  not about exposing messages;
* a shared token must never travel on the wire in plaintext if we can avoid it, and must never be
  logged;
* the operator must not need root, a database server, or a TLS certificate just to relay frames
  on a trusted network;
* the same binary, with no extra daemon, must serve the control panel — operators should not need
  a separate web server just to see peer counts;
* the bridge API already has a loopback-only, token-authenticated WebSocket/REST surface. Reusing
  it for backbone controls keeps one client stack and one auth model.

## 2. Decision

**Add OBB/1 backbone mode to `openmesh-bridge`, with a token-authenticated TCP transport and a
control panel served by the bridge's existing local API.**

### 2.1 Protocol (normative: [BACKBONE.md](../architecture/BACKBONE.md))

* Length-prefixed TCP framing: `u16 BE length || kind || payload`; `kind 0x00` = CBOR control,
  `kind 0x01` = one complete OMP frame (≤ 240 B). Unknown kinds ignored for forward compatibility.
* Three-message handshake: `hello` (proto `obb/1`, node id, client nonce, caps) →
  `challenge` (server node id + nonce, auth mode) → `auth` (HMAC-SHA256 proof over both nonces and
  both node ids) → `welcome` (session id, features, ping interval, max frame) or
  `auth_failed`.
* Token is a shared secret stored in `<data-dir>/backbone.json` (0600), generated on first run,
  **never sent on the wire** — only proved through HMAC, compared in constant time.
* Optional allowlist of node ids, `max_peers`, per-IP handshake rate limiting (10/min, 30 pending),
  10 s handshake timeout, 90 s idle close, per-peer queue caps and backpressure.
* Server mode relays frames between OBB/1 peers and its own radio/LAN transports; client mode
  (`openmesh-bridge join`) dials, reconnects with backoff and shows up in the peer table.
* OMP/1 frames are unchanged: encrypted bodies, TTL/dedup; the backbone is a transport, not a
  trust boundary for message contents.
* The old unauthenticated `--tcp-listen`/`--tcp-peer` flags stay for compatibility but are not
  exposed in the panel and are documented as LAN-only/legacy.

### 2.2 Control plane

* `/api/backbone`, `/api/backbone/peers`, `/api/backbone/config`,
  `/api/backbone/token/rotate`, `/api/backbone/peers/:node_id`, `/api/backbone/setup`,
  `/api/backbone/qr.svg` (BACKBONE.md §7–§8; WEBSOCKET-API.md §8).
* Live WS events `backbone_status`/`backbone_peer`; commands `get_backbone`,
  `set_backbone_config`, `backbone_kick`, `backbone_rotate_token`.
* Served by the **existing local bridge API** (loopback `127.0.0.1:8787`, bearer token, token file
  `0600`). No separate control-plane daemon, no separate auth model, no public admin port.
* The web bundle serves the panel at `/backbone`; the panel shows status/throughput/peers, live
  config, token rotation (new token shown once), the setup wizard and a QR code.
* First run prints an onboarding guide and persists `backbone.json`; `--print-setup` prints the
  same without starting the daemon; `openmesh-cli backbone …` wraps the API for terminals.

### 2.3 Deployment story

* `scripts/install.sh`: POSIX sh, no root by default, release download with SHA-256 verification,
  installs binaries + web bundle, `--service` for a systemd user unit, `--dry-run`, `--uninstall`,
  `--base-url`/`OPENMESH_BASE_URL` for mirrors/tests.
* Hardened systemd units (user and system, dedicated `openmesh` user via sysusers,
  `StateDirectory`/`ConfigurationDirectory`, `NoNewPrivileges`, `ProtectSystem=strict`,
  `ProtectHome`, `PrivateTmp`, `MemoryMax`), Dockerfile + Compose (non-root, host networking
  guidance), firewall notes, Caddy/nginx panel proxies, nginx-stream+stunnel and
  WireGuard/Tailscale OBB transport options.
* Documentation is part of the deliverable: `docs/INSTALL.md` and
  `docs/operations/backbone-deployment.md` (sizing, monitoring, backup, TLS/VPN, multi-region,
  rate limits, upgrades, incident runbooks).

### 2.4 Why tokens, not TLS inside the protocol (v1)

* **Scope and risk:** implementing TLS (or a Noise/TLS-like handshake) inside OBB/1 means
  certificate/key management, cipher-suite choices, verification UX and a second crypto protocol
  to review — in a release whose main risk is already a new network daemon. A shared token is a
  few hundred lines of HMAC verification; TLS is a project.
* **Payloads are already E2E encrypted.** OBB/1 transport encryption protects *metadata* and
  prevents *frame injection*; token proof already prevents unauthorised injection. TLS adds
  metadata confidentiality on the wire segment, which is a real but narrower requirement and can
  be met by WireGuard/Tailscale or a TLS wrapper without changing the protocol.
* **Trust models differ.** Some operators run a backbone on a private VPN (TLS redundant), others
  on a public VPS (VPN/TLS wrapper advised). Keeping OBB/1 plaintext with a clear recommendation
  lets each choose; building TLS in would force certificate management on hobbyist deployments.
* **Forward compatibility.** Tokens can be rotated live and are already persisted; a future
  OBB/2 can negotiate TLS or use pre-shared-key TLS without breaking the framing handshake
  (version fields exist). Doing it later, with field feedback, is cheaper than guessing now.
* **No silent downgrade.** The handshake advertises `auth: "hmac-sha256" | "open"`; clients and
  the panel show open mode as a red warning, and open mode is explicit opt-in.

### 2.5 Why the panel lives in the local bridge API

* One daemon, one auth token, one API surface: the panel is just another client of the bridge
  (the same React app), so no extra service to install/harden/backup.
* Loopback-only by default: the admin surface is not another public port. Remote operators use
  an SSH tunnel or a TLS reverse proxy with authentication; both are documented.
* The setup payload, live config and peer controls need the same data the bridge already owns;
  splitting them into a separate daemon would duplicate state and create auth/version skew.
* It keeps the "no accounts, no cloud" promise: the panel runs on the operator's box.

## 3. Consequences

### 3.1 Positive

* **Disjoint meshes can be joined in minutes.** One VPS/Raspberry Pi with 9443/tcp open; gateways
  run `openmesh-bridge join '<connection-string>'`. Token and QR onboarding are in the panel and
  `--print-setup`.
* **No new centralised service.** The backbone is self-hosted infrastructure; there is no
  OpenMesh account server or directory. Operators control membership (token/allowlist) and can
  run it on a LAN/VPN.
* **Payload confidentiality is unchanged.** Backbone operators cannot read OMP-encrypted
  messages from other nodes; the worst case is metadata exposure plus traffic from nodes that
  trust the operator's own identity.
* **Small operational footprint.** One Rust binary + static web bundle; no database server (redb
  is embedded), no broker, no TLS requirement on private networks; systemd user unit means no root
  and no new system user for hobbyists.
* **Operable.** Panel counters, `/api/backbone`, health endpoint, CLI diagnostics, setup payload,
  token rotation/kick; deployment guide covers sizing, monitoring, backups, TLS/VPN, upgrades and
  incidents.
* **Reuses proven pieces.** The bridge API/auth, OMP engine, store-and-forward, web client and
  installer patterns from v0.1.0; OBB/1 is a thin, reviewable transport around them.

### 3.2 Negative

* **Transport is plaintext by default.** Without WireGuard/Tailscale or a TLS wrapper, a network
  observer sees OBB/1 metadata and can block/tamper (though not decrypt OMP payloads or inject
  frames without the token). This is documented prominently; it is the main security trade-off
  of v1.
* **New attack surface.** A public daemon port with a handshake, rate limits, allowlists and
  panel controls. Mitigations are substantial (unprivileged, hardening, HMAC, rate limits,
  loopback panel) but this needs field exposure and review; treat v0.2.0 backbone as beta.
* **Operator trust is explicit.** A backbone sees metadata and can drop/delay traffic. The docs
  say so; users must choose an operator they trust, or run their own.
* **Fan-out cost.** OBB/1 floods frames to peers; bandwidth grows with peers and media usage
  (§3.1 of the ops guide). It is not a mesh router with path optimisation; that is OMP/2's
  territory.
* **Two deployment paths to keep consistent.** Installer, CLI `install-service`, systemd
  templates, Docker and docs must agree on flags/paths. We pinned the CLI contract with the
  runtime workstream and test the installer against a fake release; docs describe both service
  variants.
* **Panel/token UX.** The web UI needs the bridge API token pasted into Settings; there is no
  magic local-auth handshake yet. It is one copy/paste, documented; a future release could inject
  a short-lived panel token from the serving bridge.
* **No native TLS yet.** Some operators will expect `https://` on the OBB port; they must use a
  VPN/TLS wrapper. A future OBB version may negotiate TLS; until then docs must be explicit.

### 3.3 Mitigations

* Ship hardened units/container by default: non-root, `NoNewPrivileges`, `ProtectSystem`,
  `ProtectHome`, `PrivateTmp`, `MemoryMax`, read-only container rootfs, dropped capabilities.
* Loopback-only API/panel, bearer token `0600`, token rotation, kick, allowlist, max peers, rate
  limits, idle timeouts, constant-time HMAC compare, nonces in the handshake, no token logging.
* Clear docs and panel warnings for open mode and plaintext transport; WireGuard/Tailscale first
  recommendation, TLS wrapper recipe for those who cannot use a VPN.
* Installer verifies SHA-256 before installing, never overwrites config/data, supports `--dry-run`
  and `--uninstall`; test hooks for mirrors/CI; installs only high-port, unprivileged services.
* Backups/restore, monitoring, incident runbooks and a command reference in the ops guide.
* A future ADR will cover native TLS/OBB/2, multi-backbone routing improvements and panel token
  injection; the version/handshake fields leave room.

## 4. Alternatives considered

| Alternative | Why rejected |
| --- | --- |
| Unauthenticated raw TCP links on a public port | No auth (frame injection), no operator visibility, no peer management; kept only for LAN/legacy compatibility and hidden from the panel. |
| MQTT broker as the backbone transport | Adds a broker dependency, topic/auth conventions, and a second protocol to secure; does not match OMP framing; ops burden on hobbyists. |
| WireGuard/Tailscale only (no OBB/1) | Best security, but requires every joining gateway to run a VPN and still lacks mesh-level auth/membership; too much friction for the target users. Recommended *on top* of OBB/1 when possible. |
| Reticulum-style transport nodes/links | Wrong cost model for LoRa/our bridge, license prevents reuse (ADR 0001/0008); would delay v0.2 by months. |
| Build TLS inside OBB/1 first | Large scope (cert management, cipher review) for a v1 daemon; E2E encryption already protects payloads; VPN/TLS wrapper covers metadata today; native TLS can be negotiated in OBB/2. |
| Separate control-plane daemon/web server | Duplicates auth/state/versioning, more to install/harden/backup; panel as a bridge client is simpler and keeps loopback-only admin. |
| Hosted federation service | Violates no-account/no-central-service stance; operators must own the joining point. |
| Peer-to-peer NAT traversal (STUN/TURN/hole punching) | Complex, unreliable on CGNAT, still needs a rendezvous; a public TCP relay is simpler and predictable for always-on gateways. |
| **OBB/1 authenticated TCP + loopback panel + installer/devops story (chosen)** | Minimum protocol surface that solves disjoint meshes; reuses bridge auth/API/web; operable and deployable by a hobbyist or a sysadmin; clear path to add TLS/routing later. |

## 5. Security trade-off summary

| Concern | v0.2.0 posture |
| --- | --- |
| Frame injection | HMAC-SHA256 token proof, constant-time compare, nonces, allowlist, max peers, handshake rate limits |
| Metadata confidentiality on the wire | **Not provided by OBB/1**; use WireGuard/Tailscale (recommended) or nginx-stream+stunnel TLS wrapper; documented |
| Message confidentiality | OMP/1 E2E encryption unchanged; backbone relays ciphertext only |
| Token leakage | Never logged, never returned by GETs, `0600` files, one-click rotation disconnects stale peers, example configs use placeholders |
| DoS | per-IP handshake limit (10/min, 30 pending), 10 s handshake timeout, 90 s idle close, per-peer queues/backpressure, `max_peers`, systemd `MemoryMax` |
| Privilege | unprivileged user/container uid, high ports, hardened units, no capabilities |
| Replay | fresh nonces per connection; HMAC binds both nonces and both node ids; OMP dedup remains |
| Operator trust | explicit in docs/panel: operator sees metadata and can drop/delay; not a messaging provider |

## 6. Follow-ups

1. **OBB/2: native TLS or PSK-TLS** (or Noise) negotiated in the handshake, with certificate
   pinning/rotation UX in the panel; revisit after v0.2 field feedback and a security review.
2. **Panel token injection:** when the panel is served by the same bridge, inject a short-lived
   panel session token so operators do not paste the API token; keep the bearer token for external
   API clients.
3. **Membership improvements:** per-peer tokens/identities (instead of one shared secret),
   allowlist management UI, audit log of joins/kicks/rotations.
4. **Routing/scale:** if fan-out becomes the bottleneck, evaluate OMP/2 directed routing or a
   backbone gossip protocol; measure first with the counters in the ops guide.
5. **Security review:** external review of the OBB/1 handshake, rate limiting and panel auth
   before recommending public backbones beyond beta; add a reproducibility check for the
   installer's checksum handling.
6. **Add an ADR row/link in ARCHITECTURE.md §4** and README/ROADMAP links (Lead owns those files);
   the decision is otherwise fully documented in [BACKBONE.md](../architecture/BACKBONE.md).

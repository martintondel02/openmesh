# ADR 0010 — AI-native surface and beginner-friendly UX

* **Status:** accepted (v0.3.0)
* **Date:** 2026-11-25
* **Decision record:** MADR-style; referenced from [ARCHITECTURE.md §4](../architecture/ARCHITECTURE.md)
  (suggested row: "AI-native + beginner UX | guided setup, one help corpus, MCP server, activity log | ADR 0010")
* **Normative companions:** [ux-help-system.md](../design/ux-help-system.md),
  [AI-NATIVE.md](../architecture/AI-NATIVE.md), [WEBSOCKET-API.md](../architecture/WEBSOCKET-API.md),
  [BACKBONE.md](../architecture/BACKBONE.md)
* **Related:** [ADR 0006](0006-web-first-clients-tauri-native-android.md) (clients),
  [ADR 0007](0007-media-pipeline.md) (media), [ADR 0009](0009-backbone-mode.md) (backbone)

## 1. Context

v0.1.0 delivered the protocol, firmware, bridge, web client, simulator and docs. v0.2.0 added
backbone mode: one command to put a relay on a VPS, a token/QR join flow and an operations guide.
The project could now connect distant meshes — but two problems remained:

1. **The first ten minutes were still expert-only.** A newcomer faced a wall of radio profiles,
   regions, tokens, ports, TTLs and CLI flags. The web client was good once configured, but
   "install → secure → invite someone" was scattered across README, ops guides and guesswork.
   The most common support question was not protocol-level; it was "which setting do I pick?".
2. **AI agents were becoming the interface of choice for technical work.** Users were already
   pasting logs, configs and docs into Claude/Cursor/DSH to troubleshoot or script the API. Without
   a supported surface, every integration invented its own scraping, its own permissions and its
   own risk of leaking tokens. The project's own development is AI-assisted; dogfooding an agent
   interface was both a product need and an engineering one.

A third, quieter problem: the same explanation existed in four places (README, in-app tooltips,
docs guides, support answers) and inevitably drifted. Fixing one copy left three stale.

Constraints that shaped the response:

* **No cloud, no account.** Any AI feature must run locally; the model may be remote, the data
  and authority stay on the user's machine.
* **The bridge is the trust boundary.** Identity keys, PSKs, radio state and the message store live
  behind the local API. New surfaces must not bypass it or add a second authority.
* **Beginner-friendly must not mean reduced.** Advanced users must keep every dial and number;
  hiding is a default, never a restriction.
* **Help must be one corpus.** The web Help Center, the docs, the agent prompt and the MCP tools
  must agree; the corpus should also be useful offline and embeddable.
* **Safety is visible.** An agent that can send messages and change radio settings is a real
  authority; the user must be able to see what it did and revoke it.
* **Small team, small release.** v0.3.0 had to ship as documentation + a thin local server + a
  guided UI, not a new hosted product or a plugin ecosystem.

## 2. Decision

**Make OpenMesh v0.3.0 "beginner-friendly + AI-native" by adding (a) beginner/advanced modes with
server-owned setup presets, (b) a single author-once help corpus shared by app/docs/agents, and
(c) a local MCP server as a thin, attributable, permissioned client of the existing REST API.**

### 2.1 Beginner and advanced modes

* **Beginner mode is the default for new installs.** Navigation exposes Chats, Map, Nodes,
  Contacts and Help; Settings shows guided sections (identity, region/profile-only radio, privacy,
  "Secure my node" checklist). Advanced mode adds Backbone, Agents and Activity, plus full radio
  parameters, TTL/announce/power, airtime maths and raw API/MCP details.
* **The mode never changes a setting.** Switching to advanced only reveals controls; switching
  back hides them. Persisted locally (`openmesh.mode`) and synced as `features.ui_mode`; local wins
  for layout.
* **Secure defaults are the fast path.** "Secure my node" calls `POST /api/setup/quick`, which
  applies a **server-owned preset** for the chosen role (client/tracker/repeater/gateway): region
  by country, profile, store-forward, position privacy, announce interval, and an optional private
  channel with a one-time invite. The presets live in the bridge so the UI, CLI and agents behave
  identically; the web client does not invent its own defaults.
* **Every cost is explained where it is chosen.** Inline `HelpTip`s state airtime/battery/privacy
  cost in one sentence and link to a help topic; dangerous actions use typed confirmations.
* **No dead ends.** Empty states, errors and onboarding steps offer a next action and a help link;
  error payloads carry a `help_topic`.

### 2.2 One help corpus, three surfaces

* `content/help/*.md` + `index.json` are the **single source of truth** (17 topics: getting
  started, install, first mesh, radio/region, messaging, media, channels, contacts/identity,
  map/position, nodes/telemetry, backbone, agents, security, troubleshooting, glossary, FAQ).
  Each topic is self-contained, starts with an H1 title, and carries audience (`beginner`,
  `advanced`, `both`), reading time and related topics.
* The bridge **embeds** the corpus at build time (`build.rs`) and serves `GET /api/help` and
  `GET /api/help/:slug`; the web client renders the same markdown in the Help Center and copies it
  for demo/offline mode; MCP exposes it as `openmesh_help` and as `openmesh://help/{slug}`
  resources; CLI reads it via the API (`openmesh-cli help [topic]`).
* Long-form docs (`docs/`, guides, ADRs) explain *why* and link back to help topics. Help topics
  explain *what/now* and link to each other. The README/QUICKSTART are entry points, not a fourth
  copy.

### 2.3 MCP server (AI-native)

* **`openmesh-bridge mcp`** is a local **stdio JSON-RPC 2.0** Model Context Protocol server. It is
  a *thin client of the bridge REST API*: it holds no state, never touches identity keys, the
  database or the radio directly, and is configured entirely by environment
  (`OPENMESH_API`, `OPENMESH_TOKEN`, `OPENMESH_TOKEN_FILE`, `OPENMESH_MCP_READ_ONLY`,
  `OPENMESH_AGENT_NAME`, `OPENMESH_HELP_DIR`).
* **Tool catalog** (normative in AI-NATIVE.md §1.1): 13 read tools (health, status, nodes, node,
  messages, channels, contacts, backbone status/peers/setup, setup state, activity, help) and 14
  write tools (send/message/channel, position, announce, mark-read, retry, set-config,
  setup-quick, create/join channel, add contact, backbone config, rotate token ⚠, kick peer ⚠).
  Every tool has a JSON Schema, returns human-readable `content` + structured JSON, and reports
  errors with a code and `help_topic`.
* **Permissions are explicit:** read-only mode hides and refuses write tools; destructive tools
  require `confirm: true`; writes are capped at 30/min and HTTP calls time out at 10 s; secrets are
  never returned except one-time token/PSK outputs labelled "shown once".
* **Attribution is visible:** mutating calls may carry `X-OpenMesh-Agent` (MCP sets it from
  `OPENMESH_AGENT_NAME`) or `X-OpenMesh-Client`; the bridge records them in a bounded 256-entry
  activity log (`GET /api/activity`), shown in the web Agents page. Read calls are not logged;
  bodies/tokens/PSKs never appear.
* **Machine-readable contracts:** `docs/api/openapi.json` is a complete OpenAPI 3.1 spec for every
  REST route, embedded and served at `GET /api/openapi.json`; `scripts/check_openapi.py` enforces
  coverage of every `/api/...` route in the HTTP module. `AGENTS.md` + `llms.txt` give humans and
  crawlers a stable map of the repository; `docs/agents/prompts.md` is a copy-paste prompt pack.

### 2.4 Why this is one release, not three

The three pieces reinforce each other:

* The **help corpus** is what makes beginner mode safe (the `HelpTip` links have somewhere to go)
  and what makes agents useful (`openmesh_help`, prompts, resources).
* The **setup presets** give both the wizard and the agent the same correct actions, so "set up my
  node" cannot produce a different config depending on the client.
* The **activity log + attribution** make the agent surface debuggable and revocable, which is a
  prerequisite for letting non-experts use it at all.
* The **OpenAPI spec** keeps the UI, CLI, MCP and docs from drifting apart; the checker makes drift
  a CI failure instead of a support ticket.

## 3. Consequences

### 3.1 Positive

* **A beginner can finish in ten minutes.** Demo mode → secure preset → private channel → invite →
  message, with the same flow in the app, QUICKSTART and help. No radio required for the first
  half; no manual token handling for the panel.
* **One explanation, three surfaces.** Updating a help topic updates the Help Center, the bridge
  API, MCP resources and the demo copy. `scripts/check_help.py` prevents index/file drift.
* **Agents are first-class but bounded.** Users get natural-language operations, scripted audits
  and guided setup without giving up the local trust model; read-only mode, confirmation, rate
  limits, loopback-only API and the activity log bound the blast radius.
* **Advanced users keep everything.** Beginner mode hides, never disables; every setting, number
  and API remains reachable in advanced mode, the CLI and the docs.
* **Contracts are machine-checkable.** OpenAPI coverage, help corpus consistency, docs links and
  the existing golden vectors/CI make regressions visible.
* **The project dogfoods its own tooling.** `AGENTS.md`, prompts and the MCP surface were used to
  build v0.3.0; the pain points found became product requirements.

### 3.2 Negative / trade-offs

* **New attack surface:** a local MCP process + write API. Mitigated by loopback-only binding,
  bearer token, read-only mode, confirmation, rate limits, attribution/activity log, and by never
  giving the server the identity seed. A user who installs a malicious agent config is still at
  risk; docs say so.
* **More code and docs to keep coherent:** presets are server-owned (good), but UI copy, help
  topics, CLI prompts and OpenAPI examples must stay aligned. Checkers cover the mechanical part;
  wording drift remains a review concern.
* **"AI-native" can read as hype.** We keep the claim concrete: a local MCP server plus a
  transparency log, no cloud dependency, no autonomous background agent. If the tooling regresses,
  the feature is invisible; it is additive, not load-bearing.
* **Beginner mode is a product opinion.** Some users want all dials immediately; the mode toggle
  and persisted preference address that, but the default will still feel restrictive to experts.
* **Help corpus is prose, not per-field metadata:** the `HelpTip` registry must map fields to
  topic anchors by hand (tested in the web client), so adding a setting still requires a help
  decision — deliberate friction against unexplained controls.
* **MCP is one protocol, not a plugin API.** Third parties cannot add tools without forking the
  server or calling REST directly; that is acceptable for v0.3 but may change if an ecosystem
  appears.
* **OpenAPI maintenance:** every new route must be added to the spec and checker; this is cheap
  but easy to forget, hence the CI failure it produces.

### 3.3 Mitigations

* Server-owned presets and a single help corpus eliminate the worst drift class (different defaults
  in different clients).
* `scripts/check_openapi.py` scans the HTTP module for `/api/...` literals and fails on missing
  coverage; `check_help.py` verifies slugs/titles/metadata; `check_docs.py` verifies links.
* MCP is read-only by default in documentation/examples; writes require opting out; destructive
  tools require confirmation; tokens come from a file, not args.
* Activity log is local, bounded and metadata-only; no message bodies, tokens or PSKs, no read
  surveillance.
* `AGENTS.md` makes the invariants (golden vectors, spec-first, no secrets, checkers) explicit for
  both human and AI contributors.
* Roadmap keeps the security review (OMP/2, external review) separate from UX/agent features; the
  agent surface does not gate wire-protocol work.

## 4. Alternatives considered

| Alternative | Why rejected |
| --- | --- |
| **Docs-only beginner help** (no modes, no presets) | Keeps code small but leaves the "which setting?" wall in place; support burden stays; the UI still exposes expert knobs. |
| **Separate beginner app / separate advanced client** | Two clients to build, test, secure and document; beginners hit a cliff when they outgrow it; settings would diverge. |
| **Client-side presets** (web/CLI each define defaults) | Three sources of truth for the same "secure setup"; agents and CLI would drift; support becomes "which client did you use?". Server-owned presets make behaviour identical. |
| **Hide advanced features entirely** | Violates the principle that nothing is removed; power users and operators need raw config/airtime/backbone controls; hiding also makes docs incomplete. |
| **Cloud agent service / hosted assistant** | Violates no-account/no-cloud; concentrates identity and metadata; would need a new trust model and privacy policy. |
| **Plugin/extensions API for agents** | Large API surface, untrusted code in the bridge process, new security review; MCP + REST covers the use cases without executing third-party code. |
| **Give MCP direct engine/DB access** | Faster but bypasses the API's auth, validation, activity log and rate limits; a bug or malicious prompt could corrupt state. The REST API is the trust boundary. |
| **MCP server embedded in the web client** | Browsers cannot spawn stdio processes reliably; the bridge is the right process to own the local API; stdio server is portable across Claude/Cursor/DSH. |
| **LLM calls inside the bridge** | Model/provider churn, API keys, cost and privacy would all enter the daemon; keep inference in the user's client, bridge stays deterministic. |
| **No activity log** (trust the agent) | Transparency is the safety feature; users need to see what changed and revoke; metadata-only log is cheap. |
| **No OpenAPI / hand-written docs** | Already drifted at v0.2; a machine-checkable contract is cheap and catches missing routes. |
| **Chosen: beginner/advanced modes + shared help corpus + local MCP + OpenAPI + activity log** | Smallest set that makes beginners successful, makes agents safe and useful, and keeps one source of truth for behaviour and explanation. |

## 5. Security and privacy summary

| Concern | v0.3.0 posture |
| --- | --- |
| Agent authority | Local process, loopback REST, bearer token from file/env; no identity seed/PSKs; read-only mode; destructive tools need `confirm`; 30 writes/min; 10 s timeouts |
| Token leakage | Token never in MCP stdout/logs; docs/examples use token file; one-time rotate; activity log excludes secrets |
| Prompt injection / confused deputy | MCP is a thin tool server; dangerous ops require explicit confirmation; docs tell users to review activity and use read-only for dashboards |
| E2E encryption | Unchanged: agents can read what the local bridge can read (your node's messages) but do not get other nodes' keys; channel PSKs never returned by reads |
| Privacy | Activity log records write metadata only (no reads, no bodies); location sharing remains explicit; setup preset defaults to private |
| Help content integrity | Embedded corpus + checkers; runtime dir override for development only (`OPENMESH_HELP_DIR`), no remote content fetch |

## 6. Follow-ups

1. **Field-test the beginner funnel** with non-technical users; measure time-to-first-message,
   drop-off at each wizard step and support volume. Adjust presets/copy from data, not intuition.
2. **Help-topic coverage check in the web client** (every settings control has a `helpKey` and the
   target anchor exists) as a test, not a manual review.
3. **Agent permission scopes** — per-tool allowlists or a "confirm every write" mode; possibly
   separate tokens for read-only vs write agents.
4. **Activity retention/export** — user-selectable retention, optional syslog export, and a
   "revoke all agents" button in the Agents page.
5. **MCP protocol currency** — track spec revisions (2025-06-18 baseline), test against multiple
   clients in CI, document client-specific quirks.
6. **OpenAPI generation from code** if route literals become dynamic; until then the checker is the
   guardrail.
7. **Long-form docs consolidation** — if the guides/help/README triangle starts to drift, promote
   help topics to the canonical source and make guides link-only.
8. **External review** of the MCP/agent surface and setup presets alongside the OMP/2 security
   review; keep the "AI-native" claim honest.

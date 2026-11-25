# AGENTS.md — instructions for coding agents

This repository is **AI-built and human-directed**. Coding agents (Claude Code, Cursor, DSH,
Codex-style tools, custom loops) are welcome, but they work under the same rules as humans:
**contracts first, tests always, no silent protocol drift.** This file is the entry point for any
agent that will change code, docs or content. Read it before editing.

* Humans: see [CONTRIBUTING.md](CONTRIBUTING.md) and [README.md](README.md).
* Agents: also read the two agent-facing references — [docs/agents/mcp.md](docs/agents/mcp.md) and
  [docs/agents/prompts.md](docs/agents/prompts.md) — and the help corpus in `content/help/`.

---

## 1. Read order (do this first)

1. [`docs/architecture/PROTOCOL.md`](docs/architecture/PROTOCOL.md) — **normative** OMP/1 wire
   protocol. Code follows the spec, never the other way around.
2. [`docs/architecture/ARCHITECTURE.md`](docs/architecture/ARCHITECTURE.md) — components, boundaries,
   key decisions.
3. The ADRs in [`docs/adr/`](docs/adr/) that touch your area, especially
   [0001](docs/adr/0001-own-protocol-vs-reticulum.md) (protocol choice),
   [0002](docs/adr/0002-managed-flooding.md) (managed flooding),
   [0003](docs/adr/0003-static-ecdh-then-ratchet.md) (key agreement),
   [0005](docs/adr/0005-companion-radio-architecture.md),
   [0009](docs/adr/0009-backbone-mode.md) (backbone),
   [0010](docs/adr/0010-ai-native-and-beginner-ux.md) (AI-native + beginner UX).
4. [`docs/architecture/WEBSOCKET-API.md`](docs/architecture/WEBSOCKET-API.md) — bridge WS/REST
   contract; [`docs/api/openapi.json`](docs/api/openapi.json) — machine-readable REST contract.
5. [`docs/architecture/BACKBONE.md`](docs/architecture/BACKBONE.md) — normative OBB/1 backbone
   contract (if touching backbone).
6. [`docs/architecture/AI-NATIVE.md`](docs/architecture/AI-NATIVE.md) and
   [`docs/design/ux-help-system.md`](docs/design/ux-help-system.md) — normative for the MCP server,
   help/setup APIs, activity log and beginner/advanced UX.
7. [`docs/ROADMAP.md`](docs/ROADMAP.md) — what is in scope for the current milestone.

If a task conflicts with a normative document, **stop and report the conflict** instead of
inventing behaviour. Ask the Lead/human; do not silently reinterpret the spec.

---

## 2. Contract hierarchy and invariants

These are not style preferences; breaking them breaks the network, firmware or clients.

### Protocol invariants (OMP/1)

* **Frame MTU is 240 bytes** of payload; the 22-byte header is fixed. Never grow a frame beyond it.
* **Wire format changes require:** an edit to `docs/architecture/PROTOCOL.md`, regenerated golden
  vectors in `protocol/vectors/`, a `PROTO_VERSION` migration note, and a firmware conformance
  check. A code-only wire change is never acceptable.
* **AEAD AAD** clears the TTL nibble before authenticating (relays may only change TTL). Do not
  change this, do not "fix" the AAD, do not authenticate mutable header fields.
* **Dedup/TTL semantics** (`(src, seq)` dedup, TTL defaults 5/8, announce TTL 8) are protocol
  behaviour covered by vectors and integration tests. Do not tune them in passing.
* **Golden vectors are byte-for-byte contracts**: `protocol/vectors/*.json` are replayed by Rust,
  the firmware host tests and CI. Never hand-edit them and never regenerate them to make a test
  pass. If they drift, find the cause or report it.
* **Crypto primitives and domains** are specified: XChaCha20-Poly1305, Ed25519, X25519,
  HKDF-SHA256, node id = `SHA-256("openmesh/v1/node" || ed_pub || x_pub)[0..8]`. Do not invent keys,
  reuse a nonce, or add a new domain without a spec + ADR.
* **No secrets in logs**: tokens, PSKs, identity seeds, raw public identities beyond the API
  contract, and message bodies must not be logged. The activity log records metadata only.

### API and content invariants

* The bridge API (**WS/REST**) is documented in `WEBSOCKET-API.md` + `docs/api/openapi.json`. Every
  literal `/api/...` route in `crates/openmesh-bridge/src/http/` must be in the OpenAPI spec —
  `scripts/check_openapi.py` enforces this. Add a route → update the spec and checker run.
* The **help corpus** `content/help/*.md` + `index.json` is the single source of truth for the web
  Help Center, the bridge `GET /api/help*` endpoints, MCP `openmesh_help` and the demo-mode copy.
  Slugs/titles/summaries/audience/minutes/related must match `index.json` exactly
  (`scripts/check_help.py`). Never edit generated copies (e.g. `apps/web/public/help/`) — edit
  `content/help/` and let the build copy.
* **Beginner-facing copy** follows `docs/design/ux-help-system.md`: plain language, jargon defined
  on first use, trade-offs stated, no dead ends. Advanced docs may use protocol terms and numbers
  but must be accurate.
* **No fabricated CLI flags, endpoints or numbers.** Verify commands against `--help` and source;
  verify API against `http/mod.rs` + OpenAPI; verify airtime/capacity numbers against PROTOCOL.md.
* **License**: everything is AGPL-3.0-or-later. Third-party code must be compatible and recorded in
  `NOTICE`. Do not copy from restricted licences (notably LXST / rsLXST).

---

## 3. Repository map

| Path | What lives there | Normative source |
| --- | --- | --- |
| `crates/openmesh-core/` | IO-free OMP/1 engine, crypto, resources, store-forward, companion codec | `PROTOCOL.md` |
| `crates/openmesh-bridge/` | bridge daemon: transports, redb store, WS/REST API, backbone, MCP, help API | `WEBSOCKET-API.md`, `BACKBONE.md`, `AI-NATIVE.md` |
| `crates/openmesh-cli/` | terminal client, diagnostics, airtime calculator, agent helpers | `README.md`, `WEBSOCKET-API.md` |
| `crates/openmesh-sim/` | deterministic mesh simulator/scenarios | `PROTOCOL.md` |
| `crates/openmesh-ffi/` | UniFFI-ready mobile surface | `COMPANION-API.md` |
| `apps/web/` | React 19 + TS web/PWA + Tauri shell; Help Center; demo mode | `WEBSOCKET-API.md`, `ux-help-system.md` |
| `apps/android/` | Compose Android scaffold | `COMPANION-API.md` |
| `firmware/` | LoRa firmware (C++17, PlatformIO) | `PROTOCOL.md` |
| `protocol/vectors/` | Golden vectors replayed across Rust/firmware/CI | `PROTOCOL.md` |
| `content/help/` | Help topics + index (embedded in bridge/web) | `ux-help-system.md` |
| `docs/` | architecture, ADRs, guides, operations, API spec | — |
| `packaging/`, `scripts/install.sh` | systemd/Docker/firewall, installer | `INSTALL.md` |
| `scripts/` | dev env, full check, docs/help/openapi checkers | `CONTRIBUTING.md` |

---

## 4. Build and test commands

The repo is developed on a Fedora host **without a system C compiler**; `scripts/dev-env.sh`
configures `zig cc` as `cc`/linker for Rust builds. On a normal gcc/clang machine this is a no-op.

```bash
# environment (once per shell/machine if `cc` is missing)
./scripts/dev-env.sh

# Rust: format, lint, tests
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets

# protocol/simulator
cargo test -p openmesh-core
cargo run -q -p openmesh-sim -- --profile ci

# bridge (demo + API)
cargo run -q -p openmesh-bridge -- --demo            # API at 127.0.0.1:8787
cargo run -q -p openmesh-bridge -- mcp               # MCP server on stdio (needs a bridge)

# CLI
cargo run -q -p openmesh-cli -- --help
cargo run -q -p openmesh-cli -- agent config --client generic

# web (only if apps/web touched)
pnpm --dir apps/web install
pnpm --dir apps/web build
pnpm --dir apps/web test -- --run

# docs/content gates
python3 scripts/check_docs.py       # required docs + all relative links
python3 scripts/check_help.py       # help corpus <-> index.json consistency
python3 scripts/check_openapi.py    # REST routes in http/mod.rs covered by OpenAPI

# full local gate (CI equivalent today; run the three content checkers separately
# until scripts/check.sh includes them)
./scripts/check.sh
```

**Git policy for agents:** do **not** run git commands that change the repository — no commit,
push, branch, rebase, merge, tag, reset or checkout. Leave your changes in the working tree for the
human to review. (The full gate `scripts/check.sh` uses a read-only `git diff` for vector drift; a
human/CI runs it. If your environment forbids git entirely, run the individual checks above and
report that the vector-drift step was skipped.)

`scripts/check.sh` order: fmt → clippy → workspace tests → regenerate vectors + drift check →
simulator → web build/tests (if installed) → docs links. Keep it green; if a step fails for
environmental reasons (no `pnpm`, no `git`), say so explicitly in your report rather than
weakening the gate.

### Golden vectors

* Location: `protocol/vectors/*.json` (`frame`, `seal`, `identity`, `app_message`, `announce`,
  `resource`, `companion`, …). They are generated, never hand-written.
* Regenerate **only when the protocol spec changes**: `cargo run -q -p openmesh-core --example
  gen_vectors`, then let CI (`git diff --exit-code -- protocol/vectors`) confirm the change is
  intentional and reviewed. A drift without a spec change is a bug: investigate, do not commit the
  regenerated files.
* The firmware host tests replay the same vectors byte-for-byte; a Rust-only change that alters
  bytes will fail firmware conformance, which is the point.

### Firmware (only if `firmware/` touched)

```bash
pip install platformio
pio run -d firmware -e t1000e          # or the board from firmware/README.md
pio test -d firmware -e native         # host tests incl. golden-vector replay
```

---

## 5. Write scopes, conflicts and concurrent agents

* Every delegated task declares **write scopes** (files/directories). Do not edit outside your
  scope, even for a "small fix" — report it instead. Shared files (README, ROADMAP, ARCHITECTURE,
  ADRs, generated assets) are owned by other agents or the human; suggest edits in your report.
* Multiple agents may work in parallel on the same working tree. Before editing, re-read the file
  (another agent may have changed it). Prefer targeted edits; do not revert concurrent work.
* Generated or build-output files (`target/`, `apps/web/dist/`, `apps/web/public/help/`, embedded
  tables produced by `build.rs`, `protocol/vectors/` unless protocol-changing) must not be edited
  by hand.
* Never run destructive operations against the user's data dirs (`~/.openmesh`, `~/.config/openmesh`,
  `~/.local/share/openmesh`) or real radios/ports; use temporary dirs and `--sim`/`--demo` for tests.
* No network-dependent tests in the default suite; installer/download tests must use fake local
  release directories or local HTTP servers.

---

## 6. Change checklist for agents

Before you report completion:

1. **Spec impact** — does this change the wire format, API, config schema or user-facing behaviour?
   If yes, update the normative doc and say which one. Wire change ⇒ vectors + `PROTO_VERSION` note.
2. **Tests** — new protocol behaviour needs unit/integration tests; UI features need a test or a
   documented manual verification step; API routes need OpenAPI coverage.
3. **Docs + help** — if the behaviour is user-facing, update the relevant help topic
   (`content/help/`) and/or long-form doc; keep `index.json` metadata in sync. If an agent tool or
   MCP surface changes, update `docs/agents/mcp.md` (tool table must match AI-NATIVE.md §1.1).
4. **Checks** — run the narrowest checks for your area, then the full set when feasible:
   `cargo fmt/clippy/test`, `pnpm build/test` (web), `scripts/check_docs.py`,
   `check_help.py`, `check_openapi.py`; `./scripts/check.sh` for the CI gate.
5. **Report** — state exactly what changed, commands run with results, known gaps/uncertainties,
   and anything you could not verify (e.g. no Docker, no radio hardware, no browser).
   Do not claim a check passed if it was skipped.

---

## 7. Commit / PR rules (humans merge; agents leave the tree clean)

* One concern per change; small reviewable diffs.
* Commit style: `area: imperative summary` — `core:`, `bridge:`, `cli:`, `web:`, `firmware:`.
  `docs:`, `help:`, `api:`; e.g. `help: explain offline delivery ticks`.
* PR checklist: specs updated, vectors regenerated if needed, fmt/clippy/tests green, web
  build/tests green, docs green, no credentials/tokens/seeds/personal data committed, NOTICE updated
  for new dependencies.
* Do not commit generated build output or secrets. Do not force-push shared branches.
* Agents cannot merge or approve PRs; a human reviews and merges.

---

## 8. Where to look when unsure

| Question | Look at |
| --- | --- |
| Wire format, headers, TTL, crypto | `docs/architecture/PROTOCOL.md` + `protocol/vectors/` |
| Why a design exists | `docs/adr/` (decisions) and `docs/research/` (evidence) |
| Bridge events/commands/REST | `docs/architecture/WEBSOCKET-API.md`, `docs/api/openapi.json`, `crates/openmesh-bridge/src/http/` |
| Backbone rules/ports/config | `docs/architecture/BACKBONE.md`, `docs/operations/backbone-deployment.md` |
| Beginner UX, help surfaces, setup presets | `docs/design/ux-help-system.md` |
| MCP tools/resources/prompts, activity log | `docs/architecture/AI-NATIVE.md`, `docs/agents/mcp.md` |
| Install/upgrade/service paths | `docs/INSTALL.md`, `scripts/install.sh`, `packaging/` |
| Current milestone/scope | `docs/ROADMAP.md` |
| User-facing help wording | `content/help/*.md` (and its `index.json`) |

If the answer is not there, **ask the Lead/human** rather than guessing. A precise question with
the conflicting lines is always better than an invented feature.

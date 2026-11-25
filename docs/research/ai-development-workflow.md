# How this repository was built by AI agents

**Status:** process note · **Last updated:** 2026-11-25
**Scope:** describes the build process, not the product. No benchmark numbers are claimed; the
only quantitative claims are repository facts measured with `find`/`wc` and protocol
measurements that can be reproduced with the commands in this note.

Related: [Architecture](../architecture/ARCHITECTURE.md) ·
[Protocol](../architecture/PROTOCOL.md) ·
[ROADMAP](../ROADMAP.md) ·
[ADR 0004](../adr/0004-rust-core-cpp-firmware-golden-vectors.md).

---

## 1. What "100 % AI-built" means here, precisely

OpenMesh v0.1.0 was built by a team of AI agents running in the **DeepSeek Harness (DSH)** web
GUI, under the direction of a human operator who set the goals, provided the machine,
approved the plan, and will own the result. There is no human-written source code in the
repository. The human contribution is:

* the product brief and non-goals (what OpenMesh is, who it is for, what it will not do);
* the environment (Fedora host without a system compiler, no Android SDK, no sudo, network
  with a flaky GitHub mirror);
* the acceptance bar for this milestone ("compiles, tests pass where runnable, docs accurate,
  no fabricated citations");
* review decisions during the run (for example, patching a protocol bug found by an
  independent size check — §6);
* the responsibility for everything that ships.

The agent contribution is everything else: architecture, protocol design, Rust/C++/TypeScript/
Kotlin code, tests, documentation, ADRs, tooling, and the process used to coordinate all of it.
This note is itself agent-written and describes the process honestly, including its limits
(§8).

The "100 %" claim is therefore about authorship of the artefacts, not about removing the
human. It also does not mean "reviewed to production quality": no hardware, no field test, no
external security audit has happened at v0.1.0.

---

## 2. The environment

| Item | Value |
| --- | --- |
| Harness | DeepSeek Harness (DSH), web GUI on `127.0.0.1:3080` |
| Lead model | `deepseek-v4.1-flash:1017` |
| Teammate models | model route is a per-session harness setting. Build sessions for this project have used `kimi-k3.1-pro` for some teammates and Ollama-hosted cloud models for others; the roster visible to the Lead reports each teammate's route. Model assignment is not recorded in the repository, so per-file attribution by model is *not* claimed. |
| Host | Fedora Linux, Node.js and Python available, **no system gcc**, no Android SDK, no PlatformIO initially, no sudo |
| Network | raw.githubusercontent.com flaky; retries and local clones used heavily |
| Workspace | one shared filesystem; every agent sees the same files immediately; no per-agent worktrees |
| VCS | the human's repository; agents are forbidden from running `git` commands. The Lead inspects the tree and diffs, the human commits. |

The "no system gcc" detail matters: the Rust toolchain uses `zig` as the C compiler/linker for
dependencies with C code (see `scripts/dev-env.sh`), and the firmware is cross-compiled with
PlatformIO's toolchains, not the host compiler. This shaped the verification strategy: build
what the host can build, and make the rest CI-ready and honest about being unverified.

---

## 3. Team topology

The Lead is a single agent with its own context. It spawns **teammates** — durable agents with
fresh contexts, a name, a description, and a complete task prompt — and coordinates through a
shared task board and direct messages. For OpenMesh v0.1.0 the topology was:

```
                         ┌──────────────┐
                         │    Lead      │  specs, integration, review, final acceptance
                         │ deepseek-…   │
                         └──────┬───────┘
        ┌───────────────┬───────┼───────────────┬────────────────┐
        ▼               ▼       ▼               ▼                ▼
  core-protocol    mesh-runtime  web-client   firmware-flasher  platform-docs
  openmesh-core    bridge/cli/   apps/web      firmware/,        android, ffi,
  protocol/vectors sim           (+Tauri)      apps/flasher,      docs/research,
                                                tools/flash,      docs/adr, roadmap
                                                docs/hardware
```

| Teammate | Write scope | Primary acceptance |
| --- | --- | --- |
| `core-protocol` | `crates/openmesh-core`, `protocol/vectors` | `cargo test -p openmesh-core` green, ≥40 tests, malformed-input fuzz-ish tests, clippy clean, public API stable |
| `mesh-runtime` | `crates/openmesh-bridge`, `crates/openmesh-cli`, `crates/openmesh-sim` | workspace builds; bridge demo serves `/api/health` + `/api/ws`; simulator scenarios pass with airtime assertions; redb persistence |
| `web-client` | `apps/web` | `pnpm build` + `pnpm test` green; real API when bridge present, demo data when absent; no console errors |
| `firmware-flasher` | `firmware/`, `apps/flasher/`, `tools/flash/`, `docs/hardware/` | PlatformIO envs parse; four boards; native vector tests where runnable; flasher page static build |
| `platform-docs` (this workstream) | `apps/android/`, `crates/openmesh-ffi/`, `docs/research/`, `docs/adr/`, `docs/ROADMAP.md` | Android scaffold coherent + CI-ready (not compiled locally); FFI crate builds; docs 300–800 lines each, accurate, sourced, cross-linked; ADRs 0001–0008; roadmap |

The Lead kept `docs/architecture/**` and the repository root; teammates were told not to edit
outside their scope. Scopes are disjoint by directory, which is the simplest reliable
concurrency control for a shared filesystem.

### 3.1 Why fresh-context teammates

A teammate started with `fresh` context does not inherit the Lead's conversation. That is a
feature: the Lead must write a complete, self-contained task prompt (goals, files to read,
acceptance criteria, environment constraints, scope boundaries). Those prompts are effectively
a second, executable specification. The cost is that teammates can drift on context the Lead
"assumes"; the mitigation is explicit reading lists in every prompt (e.g. "read
`docs/architecture/PROTOCOL.md`, `crates/openmesh-core/src/{lib,engine,companion,util}.rs`, then
the reference repos in `/tmp/research`").

`fork` context exists too (it seeds the child with the Lead's completed turns) and is better
for review/continuation tasks; it was not needed for the main build because fresh-context
prompts kept the Lead's context small.

### 3.2 The task board

Every workstream is a task on a shared board with:

* `id`, `subject`, `description` (the full prompt), `blocked_by`, `write_scopes`, `status`
  (`pending`/`in_progress`/`completed`), `owner`, and a monotonically increasing `revision`;
* a compare-and-set claim: `team_task_get` → `team_task_update {action: "claim",
  expected_revision}`. A stale revision fails, which prevents two agents from claiming the same
  task;
* `complete` when acceptance is met, with a report to the Lead (paths, line counts, tests run,
  uncertainties).

Dependencies (`blocked_by`) model the only true ordering in the build: the specs existed
before code, and `openmesh-ffi`/Android consume the core + companion API. Most workstreams ran
in parallel because their scopes were disjoint.

### 3.3 Messaging and escalation

Teammates can message the Lead and each other with `send_message`. The rules used:

* message the Lead when a discovered fact changes a shared contract (protocol ambiguity, API
  mismatch, license issue) — do not silently work around it;
* message a peer only for a concrete interface question, and prefer file evidence over prose;
* a successful send only means queued; it does not mean processed.

This is how the protocol bugs in §6 reached the Lead while the core workstream was still
writing code, which is the main advantage of the topology.

---

## 4. Contracts before code

The Lead wrote the normative documents first, before any implementation was accepted:

1. `docs/architecture/ARCHITECTURE.md` — goals, component map, decisions summary, hardware,
   security model, testing strategy, roadmap sketch.
2. `docs/architecture/PROTOCOL.md` — byte layouts, crypto, flooding, resources,
   store-and-forward, timing constants, companion summary, transport bindings, golden vectors.
3. `docs/architecture/WEBSOCKET-API.md` — envelope, events, commands, REST, attachments.
4. `docs/architecture/COMPANION-API.md` — SLIP framing, CRC, message types, handshake, config
   keys, standalone mode, security notes.

Then implementation started against those documents, with `protocol/vectors/` as the
cross-language contract between Rust and C++. The ordering is deliberate:

* a protocol decision costs a paragraph in a document but a rewrite in four codebases;
* golden vectors let the Rust core and C++ firmware be written **in parallel** without
  integration weekends;
* clients (web/Android) can be scaffolded against a stable API before the bridge works, using
  demo data;
* the docs are also the prompts: when a teammate is told "implement PROTOCOL.md §9", the
  acceptance is testable.

The failure mode is equally real: a spec error propagates to every implementation. §6 is the
case study.

---

## 5. Reproducible process

This is the recipe the Lead used, written so another team (human or AI) can repeat it.

### 5.1 Phase 0 — bootstrap (human + Lead, ~1 day)

1. Agree the product brief, non-goals, and license (AGPL-3.0-or-later; see
   [ADR 0008](../adr/0008-agpl-3-0-licensing.md)).
2. Create the repository skeleton: workspace `Cargo.toml`, `rust-toolchain.toml`,
   `rustfmt.toml`, `clippy.toml`, crate directories, `docs/architecture/`, `protocol/vectors/`,
   CI workflow stubs, `.gitignore`.
3. Write the normative specs (§4). Do not start implementation before the frame format,
   crypto, and API envelopes are settled.
4. Establish invariants in the skeleton: `#![forbid(unsafe_code)]` in the core,
   `#![warn(missing_docs)]`, clippy `-D warnings`, a deterministic clock (`now_ms` passed in),
   no IO in the core.
5. Provision the environment: `scripts/dev-env.sh` prints the zig/cc hints, document the
   missing-SDK realities in the READMEs.

### 5.2 Phase 1 — fan-out (Lead + N teammates, parallel)

1. Create one task per workstream with a **complete** prompt:
   * context (product, architecture, relevant spec sections, files to read first);
   * deliverables as a checklist;
   * acceptance commands ("`cargo test -p openmesh-core` green with ≥40 tests");
   * environment constraints ("no Android SDK, do not attempt to install it"; "never run git");
   * write scope and the rule "do not edit outside scope";
   * what to report back (exact paths, line counts, verification performed, uncertainties).
2. Spawn teammates (`fresh` context) with a name, description and that prompt; have each claim
   its task with the current revision.
3. Keep the Lead available for contract questions and integration; the Lead runs the workspace
   build, reads diffs and adjudicates overlaps.
4. Use the shared filesystem as the integration medium: each crate/app is a directory, so
   parallel writes rarely collide. When they do, prefer a small API change in the core that
   both sides can adopt over duplicated glue.

### 5.3 Phase 2 — verification (Lead + all)

* Build and test everything the environment allows:
  `cargo build --workspace`, `cargo test`, `cargo clippy --workspace -- -D warnings`,
  `pnpm --dir apps/web build && pnpm test`, `cargo run -p openmesh-sim`,
  `cargo run -p openmesh-bridge -- --demo` plus a WebSocket smoke test.
* Run the checks that CI would run but the host cannot: Android `assembleDebug`,
  `pio run -e <board>`, Tauri bundle. Those are documented as unverified in the READMEs and
  the CI workflows, not claimed as done.
* Cross-check the documents against the code: links, constants, line counts, spec appendix
  numbers. A docs workstream is a good place to catch numeric drift (§6).
* Review scope: the Lead reads the final tree, not every line of every file; that is a real
  limitation (§8).

### 5.4 Phase 3 — freeze and report

* Tag v0.1.0 with a CHANGELOG-style list of what is real vs stubbed.
* Publish the known limitations honestly (no field test, no SDK builds, no audit).
* Keep the task board as the provenance record of who was asked to do what, with what
  acceptance bar.

### 5.5 A task-prompt template

```markdown
You are <name>, a teammate on the <project> project. Working dir: <abs path>
(shared filesystem with Lead and other agents).

FIRST: call team_task_get {"task_id":"<id>"} then team_task_update
{"task_id":"<id>","expected_revision":<rev>,"action":"claim"}.

Read first: <specs and source files>. Then <reference material>.

YOUR SCOPE (only): <paths>. Do not edit other paths. Do not run git commands.

DELIVERABLES:
1. <deliverable> — <acceptance: command + expected result>
...

ACCEPTANCE: <test/build/line-count/link-check criteria>

ENVIRONMENT: <host limits: no SDK, no sudo, no gcc, flaky network>

WHEN DONE: mark <task> complete and send the Lead a report:
paths + line counts, verification performed, any factual uncertainties.
```

The prompt is long on purpose. In practice, the quality of a teammate's output correlates
with the specificity of its acceptance criteria, and the cheapest fix for a bad deliverable is
a better prompt, not a bigger model.

---

## 6. Case study: independent measurement caught two protocol bugs

The `platform-docs` workstream needed exact frame sizes for the capacity analysis. Rather than
trusting the spec's Appendix A, it serialised the actual `services.rs` types with `ciborium` and
ran its own Semtech airtime implementation. Two real bugs fell out:

**Bug 1 — announces could never fit one frame.** PROTOCOL.md (pre-fix) specified a CBOR
`Announce` containing a 72-byte public identity and a 64-byte signature. Measured sizes:

| name length | CBOR size | body = CBOR + 64 B signature | frame = 22 + body | fits 240-B MTU? |
| --- | --- | --- | --- | --- |
| 0 | 157 | 221 | 243 | no |
| 12 | 169 | 233 | 255 | no |
| 32 | 190 | 254 | 276 | no |

The minimum possible announce was 243 bytes, so no announce could be sent at all. The Lead
replaced it with a fixed binary layout: `version/class/caps/hops/name_len/interval/timestamp/
node_id/sign_pk(32)/dh_pk(32)/name/signature(64)` = `152 + name_len` bytes ≤ 184, one frame,
still self-certifying and still signed. The CBOR vector table was updated accordingly.

**Bug 2 — group messages could never fit either.** The CBOR `GroupMessage` embedded a full
72-byte identity *and* a 64-byte signature, so the minimum sealed body (empty text) was 236
bytes against a 218-byte budget; a 100-char channel message was 337 bytes. The Lead changed the
schema to `sender: NodeId(8) + signature(64) + message`, with the signature key resolved from
the peer book (or `IDENTITY_INCLUDED`), and mandated the resource path beyond ~60 characters.

**Bonus — Appendix A drift.** The 100-char text row quoted 610 ms, which the independent
formula identified as the airtime of a full 240-byte frame, not of the actual 201-byte frame
(519 ms). The appendix is now generated from measured sizes; the independent model reproduces
100-char text 519 ms, 240-byte frame 611 ms, 5 KiB image ~18 s and 8 s Opus ~24 s within a few
percent (differences are in ACK counting).

Why this is evidence the process works, not just a story about a mistake:

* two agents (Lead and docs) and four downstream implementations would otherwise have encoded a
  protocol that cannot transmit announces;
* the bug was found by *measurement against the implemented types*, not by more review of the
  prose;
* the fix was cheap because it landed before golden vectors were frozen — the value of
  contracts-before-code is that changes are cheap *before* the vectors, and expensive after;
* the same independent implementation is now a permanent cross-check for Appendix A; the
  recommendation is to make it a CI test that regenerates the table from code and fails on
  drift (§9).

---

## 7. What the workflow produced (measured snapshot)

At the time this note was written (2026-11-25, before final freeze — counts are a snapshot, not
a release claim):

| Artefact | Count (snapshot) |
| --- | --- |
| Rust source files in `crates/` | 36 |
| Rust lines in `crates/` | 21 472 |
| Workspace crates | 5 (core, bridge, cli, sim, ffi) |
| Normative spec docs | 4 (`ARCHITECTURE`, `PROTOCOL`, `COMPANION-API`, `WEBSOCKET-API`) |
| Research/ADR/roadmap docs | this directory + `docs/adr/` + `docs/ROADMAP.md` |
| Active teammates | 5 + Lead |
| Shared tasks | 5, one per workstream, disjoint write scopes |

The snapshot is deliberately not a quality claim. A large line count with a stub-heavy core
would be a bad milestone, which is why each workstream's acceptance is behavioural (tests,
scenarios, builds), not "write N lines".

---

## 8. Honest limitations of this build and this workflow

### 8.1 Verification gaps (the important ones)

1. **Android was not compiled.** There is no Android SDK on the host. The scaffold is written
   to be structurally correct and CI-ready, and `apps/android/README.md` says plainly that
   `assembleDebug` was not run locally. Treat it as "reviewed code", not "verified build".
2. **Firmware was not compiled on hardware or against PlatformIO** in this environment (no
   PlatformIO/toolchains at start; network restrictions). Board definitions and protocol tests
   are CI targets, not local results.
3. **Tauri desktop packaging was not built** (system webkit dependencies absent). The
   configuration is present; the bundle is a CI/human step.
4. **Web build/tests** may pass locally, but they exercise a demo/bridge simulator, not a
   real radio.
5. **No hardware, no field test, no range test, no duty-cycle enforcement measurement.** Every
   capacity number in `docs/research/` is computed from the airtime formula and frame sizes,
   not measured on air.
6. **No security audit.** The crypto uses vetted crates (dalek, chacha20poly1305, hkdf, sha2),
   but the composition (static ECDH, nonce prefixes, group signatures, store-and-forward
   replay) has not been reviewed by an external cryptographer. The docs say so.

### 8.2 Process limitations

* **Review bandwidth.** The Lead cannot read every line produced by five parallel agents in a
  short session. Review is targeted: acceptance commands, public API shape, diffs in shared
  contracts, and the documents. Subtle bugs inside a passing test suite are possible.
* **Shared filesystem, advisory scopes.** Write scopes are directory-level and advisory. Two
  agents can still race on `Cargo.toml` or a shared doc; the filesystem version guard catches
  stale writes but bash/code-generation side effects are not protected. The Lead must review
  the final tree and rerun tests after merges.
* **Spec drift.** The docs are written first; implementations and docs can diverge silently
  (the Appendix A drift above). The fix is generated tables and link/constant checks in CI.
* **Context limits.** Each teammate sees only its prompt plus files it reads. Cross-cutting
  consistency (naming, error types, API shapes) is maintained by the Lead and by shared specs,
  not by a shared memory. Long tasks may need a second prompt or a `fork`-context reviewer.
* **Model variance.** The harness allows several model routes (DeepSeek, Kimi, Ollama-hosted
  cloud models). Different models write different code quality and different prose; the
  acceptance commands are the stabiliser. No claim is made that model X is better than Y
  without a controlled benchmark, and this repository does not contain one.
* **Attribution.** The repository does not record which model wrote which line. Agents are
  accountable through the harness session log, not through commit metadata. For license
  hygiene (especially the Reticulum License's AI-training clause and the rsLXST
  non-commercial/no-derivatives license) the safe rule is: no code copied from those
  repositories, and the research notes are read-only design references (ADR 0008).
* **The "AI-built" framing can mislead.** The human set goals and owns the result; the agents
  generated text and code. "100 % AI-built" is not a warranty of correctness, security, or
  fitness, and should never be used as one.

### 8.3 Things we deliberately did *not* do

* No performance claims (no "10× faster than Reticulum") — only airtime arithmetic with
  stated assumptions.
* No fake citations: every URL in `docs/research/` was fetched, and every source-derived
  constant is attributed to a file path in a checkout with a commit id.
* No claim that the Android app "works" — it is a scaffold with a documented build path.
* No silent protocol edits by the docs workstream: discrepancies are reported upstream (§6).

---

## 9. Recommendations for the next AI-assisted milestone

1. **Make measurement a CI gate.** Generate PROTOCOL.md Appendix A and the golden vectors from
   the Rust types; fail the build if the committed numbers differ. This would have caught both
   §6 bugs automatically.
2. **Add a "spec size model" test** that asserts every frame variant fits `MTU` and every payload
   limit is reachable: announce max, group text 60 chars, DM first contact, identity-included,
   resource chunk, manifest, ack. A 20-line test prevents a class of design bugs.
3. **Add link checking and doc line-count checks** to CI (`markdown-link-check` or a small
   script), and keep the research docs cross-linked (this repository has a convention: every
   doc links its ADRs and the spec).
4. **Run the un-runnable builds in CI early**, before the scaffold is "done": Android
   `assembleDebug`, `pio run` for at least one board, `tauri build` on Linux. The cost of a CI
   runner is small compared to discovering the scaffold never compiled.
5. **Use `fork`-context reviewer agents** for the final pass on security-sensitive code
   (crypto composition, resource ARQ, store-and-forward, bridge identity mapping in M4), with a
   review prompt that includes the threat model and the spec. A reviewer that has read the whole
   conversation is better at spotting cross-file inconsistencies than a fresh implementer.
6. **Keep the task board.** It is the provenance record and the best answer to "how was this
   built?" — better than any retrospective prose.
7. **Budget for field testing.** The next milestone is not more code; it is two handhelds, a
   solar repeater, a duty-cycle logger and a week of real traffic. AI agents cannot do that.

---

## 10. Appendix: task board snapshot

The five v0.1.0 tasks as created (subject shortened; full prompts live in the harness log):

| Task | Owner | Write scopes | Acceptance (short) |
| --- | --- | --- | --- |
| core: implement OMP/1 engine, resources, store-forward + vectors | `core-protocol` | `crates/openmesh-core`, `protocol/vectors` | tests ≥40, clippy clean, API stable |
| runtime: bridge + CLI + simulator | `mesh-runtime` | `crates/openmesh-bridge`, `-cli`, `-sim` | workspace build, demo server, scenarios |
| web: React PWA + Tauri | `web-client` | `apps/web` | `pnpm build`/`test` green, demo mode |
| firmware: 4 boards + flashers | `firmware-flasher` | `firmware/`, `apps/flasher/`, `tools/flash/`, `docs/hardware/` | envs parse, native vector tests, static flasher |
| platform: Android + FFI + research/ADRs/roadmap | `platform-docs` | `apps/android/`, `crates/openmesh-ffi/`, `docs/research/`, `docs/adr/`, `docs/ROADMAP.md` | FFI builds; docs sourced and cross-linked; scaffold CI-ready |

This table is itself the most useful artefact of the process: it states, for each slice of the
repository, who was responsible, where they were allowed to write, and what "done" meant. Any
future contributor can audit the repository against it.

---

## 11. Sources

* The DeepSeek Harness runtime context and task board (session metadata; not a public URL).
* This repository: `docs/architecture/*.md`, `Cargo.toml`, `rust-toolchain.toml`,
  `scripts/dev-env.sh`, `.github/workflows/*.yml`, `crates/*/Cargo.toml`.
* Protocol measurements and the Semtech formula: [lora-capacity-and-media.md](lora-capacity-and-media.md).
* License constraints that shaped the process: [reticulum-lxmf-notes.md](reticulum-lxmf-notes.md) §4.4 and
  [ADR 0008](../adr/0008-agpl-3-0-licensing.md).

# Contributing to OpenMesh

> **Contributions are closed for now.** OpenMesh is developed only by the repository owner and
> their designated AI assistant. Pull requests from anyone else are closed automatically, and
> issues/discussions are disabled until contributions open. When they open, the
> [AI contribution policy](AI-POLICY.md) and the rules below apply. Do not open a PR expecting it
> to be reviewed while this notice is present.

Thanks for your interest in building an open mesh. OpenMesh is **AI-assisted and human-directed**:
AI tools help write code and docs, while humans set the contracts, review every change, run the
tests and hardware QA, and own the releases. The rules are simple and strict: contracts first,
tests always, no silent protocol drift, and full disclosure of AI involvement.

## Before you start

1. **Read [AI-POLICY.md](AI-POLICY.md).** AI involvement must be disclosed, reviewed by a human and
   attributable to a human author; undisclosed or autonomous AI contributions are rejected.
2. **Read the specs.** Protocol changes require an edit to
   [`docs/architecture/PROTOCOL.md`](docs/architecture/PROTOCOL.md), regenerated golden vectors
   and a firmware conformance note. The spec is normative; code follows it.
3. **`main` is protected.** Contributions go through a PR with green CI and a linear history (when
   contributions open).
4. **One concern per PR.** Small, reviewable diffs. AI-generated bulk changes are rejected.
5. **Tests are not optional.** New protocol behaviour needs unit tests; user-facing features need
   a test or a documented manual verification step in the PR description.
6. **No licence surprises.** AGPL-3.0-or-later for everything in this repo. Third-party code must
   be compatible and recorded in `NOTICE`. Do not copy code from restricted licences — notably
   LXST / rsLXST (CC BY-NC-ND). This includes code produced by an AI from those sources.
7. **Be kind.** See `CODE_OF_CONDUCT.md`.

## Development setup

```bash
./scripts/dev-env.sh            # environment hints (zig-as-cc shim on toolchain-less machines)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

pnpm --dir apps/web install
pnpm --dir apps/web dev        # demo mode works with no backend
pnpm --dir apps/web build && pnpm --dir apps/web test
```

Firmware needs PlatformIO (`pip install platformio`); see `firmware/README.md`.
Android needs JDK 21 + Android SDK/NDK + cargo-ndk; see `apps/android/README.md`.

## Commit style

`area: summary` in the imperative, e.g.:

```
core: fix resource window bitmap when chunks arrive out of order
web: add waveform playback to voice bubbles
firmware: budget airtime per region in the scheduler
docs: describe store-and-forward offer cadence
```

Commits containing substantive AI output should carry an `AI-Assisted:` trailer, e.g.
`AI-Assisted: DeepSeek Harness / DeepSeek v4.1-flash (code+tests)`.

## PR checklist

- [ ] AI assistance disclosed (tools/models, scope) and the change is understood by the human author
- [ ] Specs updated (if protocol/API changed) and golden vectors regenerated
- [ ] `cargo fmt`, `clippy -D warnings`, `cargo test --workspace` pass
- [ ] Web build + tests pass (if `apps/web` touched)
- [ ] Firmware builds/tests for affected boards (if `firmware/` touched) — CI covers this
- [ ] Android lint/tests/APK build pass (if `apps/android` touched)
- [ ] Docs updated (README/architecture/ADR/help if behaviour or decisions changed)
- [ ] No credentials, tokens, seeds or personal data committed

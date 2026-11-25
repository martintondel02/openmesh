# Contributing to OpenMesh

Thanks for helping build an open mesh. This project is AI-built and human-directed, so the
rules are simple and strict: contracts first, tests always, no silent protocol drift.

## Ground rules

1. **Read the specs first.** Protocol changes require an edit to
   [`docs/architecture/PROTOCOL.md`](docs/architecture/PROTOCOL.md), regenerated golden vectors
   and a firmware conformance note. The spec is normative; code follows it.
2. **`main` is protected.** Open a PR, keep CI green, squash-merge with a linear history.
3. **One concern per PR.** Small, reviewable diffs.
4. **Tests are not optional.** New protocol behaviour needs unit tests; user-facing features need
   a test or a documented manual verification step in the PR description.
5. **No licence surprises.** AGPL-3.0-or-later for everything in this repo. Third-party code must
   be compatible and recorded in `NOTICE`. Do not copy code from restricted licences (notably
   LXST / rsLXST).
6. **Be kind.** See `CODE_OF_CONDUCT.md`.

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

## Commit style

`area: summary` in the imperative, e.g.:

```
core: fix resource window bitmap when chunks arrive out of order
web: add waveform playback to voice bubbles
firmware: budget airtime per region in the scheduler
docs: describe store-and-forward offer cadence
```

## PR checklist

- [ ] Specs updated (if protocol/API changed) and golden vectors regenerated
- [ ] `cargo fmt`, `clippy -D warnings`, `cargo test --workspace` pass
- [ ] Web build + tests pass (if `apps/web` touched)
- [ ] Firmware builds for affected boards (if `firmware/` touched) — CI covers this
- [ ] Docs updated (README/architecture/ADR if behaviour or decisions changed)
- [ ] No credentials, tokens, seeds or personal data committed

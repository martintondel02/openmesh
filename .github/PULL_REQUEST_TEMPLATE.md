## What & why

<!-- one paragraph -->

## Scope

- [ ] core protocol (`crates/openmesh-core`)
- [ ] runtime (`bridge`/`cli`/`sim`)
- [ ] clients (`apps/web`, `apps/android`)
- [ ] firmware / flashers
- [ ] docs / research / ADR

## Checklist

- [ ] Specs updated and golden vectors regenerated (protocol changes)
- [ ] `cargo fmt` + `clippy -D warnings` + `cargo test --workspace`
- [ ] Web build/tests (if touched)
- [ ] Firmware build for affected boards (CI)
- [ ] No secrets/keys committed

## Verification

<!-- commands run, output summary, manual steps for UI changes -->

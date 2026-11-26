## What & why

<!-- one paragraph -->

## Scope

- [ ] core protocol (`crates/openmesh-core`)
- [ ] runtime (`bridge`/`cli`/`sim`)
- [ ] clients (`apps/web`, `apps/android`)
- [ ] firmware / flashers
- [ ] docs / research / ADR / help content

## AI assistance (required)

- [ ] This change was **not** AI-assisted (skip the rest of this section)
- Tool(s) / model(s) and what they produced:
- [ ] I understand and can explain every change in this PR, and I ran the relevant tests locally or in CI
- [ ] I have read and accept [AI-POLICY.md](../AI-POLICY.md) and [AGENTS.md](../AGENTS.md)
- [ ] No restricted-licence code (including code reproduced by an AI) was used

> Contributions are closed for now: PRs from anyone other than the repository owner are closed
> automatically. See [AI-POLICY.md](../AI-POLICY.md) and [CONTRIBUTING.md](../CONTRIBUTING.md).

## Checklist

- [ ] Specs updated and golden vectors regenerated (protocol changes)
- [ ] `cargo fmt` + `clippy -D warnings` + `cargo test --workspace`
- [ ] Web build/tests (if touched)
- [ ] Firmware build for affected boards (CI)
- [ ] Android lint/tests/APK (if touched)
- [ ] Docs/help updated (if behaviour or decisions changed)
- [ ] No secrets/keys committed

## Verification

<!-- commands run, output summary, manual steps for UI/hardware changes -->

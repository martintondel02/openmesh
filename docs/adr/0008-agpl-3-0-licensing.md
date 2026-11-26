# ADR 0008 — AGPL-3.0 licensing and third-party policy

* **Status:** accepted (v0.1.0)
* **Date:** 2026-11-25
* **Decision record:** summary row 8 in
  [ARCHITECTURE.md §4](../architecture/ARCHITECTURE.md#4-key-decisions)
* **Related:** [ADR 0001](0001-own-protocol-vs-reticulum.md),
  [Reticulum research notes](../research/reticulum-lxmf-notes.md) §4.4,
  [NomadNet bridge design](../research/nomadnet-bridge.md) §7.4

## 1. Context

OpenMesh is a network protocol with client and firmware implementations. Its license determines
who can build on it, what a fork must share, and which existing code we can reuse.

Requirements and constraints:

1. **Network copyleft.** A mesh is infrastructure. If someone runs a modified bridge as a
   service, users should be able to get that source. The GNU AGPL-3.0 achieves this; GPL-3.0
   does not cover the network-service case.
2. **Client and firmware must stay open.** If a vendor ships a modified OpenMesh client or
   radio firmware, users must be able to modify it. AGPL-3.0 plus the hardware/anti-tivoisation
   considerations of GPLv3 §6 covers this better than a permissive license.
3. **Compatibility with what we studied and might use.** The ecosystem around Reticulum uses:
   * **Reticulum License** (Reticulum, LXMF, Ratspeak, rsReticulum, rsLXMF, ratspeak-handheld):
     MIT-like with two extra conditions — no use in systems purposefully harming humans, and no
     use in creating AI/ML training datasets. These are field-of-use restrictions; the license
     is not OSI-open and is incompatible with AGPL-3.0 redistribution.
   * **LXST license** (rsLXST): proprietary, non-commercial, no derivatives, use outside
     Ratspeak prohibited.
   * **GPLv3** (NomadNet, Meshtastic firmware, RNode firmware): copyleft, compatible with
     AGPL-3.0 in the GPLv3 → AGPLv3 direction (GPLv3 §13 permits combining with AGPLv3).
   * **MIT** (MeshCore): permissive, compatible.
4. **The project is AI-assisted (human-directed and human-reviewed).** Some upstream licenses (notably the Reticulum License) contain
   clauses aimed at AI training/use. Even if we never copy code, using their repositories as
   *design references* and their test vectors as fixtures must be considered carefully.
5. **Crates and assets.** Rust dependencies have their own licenses (MIT/Apache-2.0 mostly),
   which are compatible; firmware libraries (RadioLib, LVGL, ArduinoJson, etc.) need a NOTICE
   and license audit; web dependencies are largely MIT.

## 2. Decision

**OpenMesh is licensed AGPL-3.0-or-later (`AGPL-3.0-or-later` in every crate and app), with an
explicit third-party policy: no code from Reticulum-License or LXST-License repositories is
copied, and all third-party dependencies are inventoried and license-checked.**

### 2.1 Repository license rules

* Every Rust crate inherits `license = "AGPL-3.0-or-later"` from `[workspace.package]`.
* Apps (`apps/web`, `apps/android`) and tooling (`apps/flasher`, `tools/flash`) are AGPL-3.0-
  or-later; no permissive sub-licensing of the client code.
* Documentation (`docs/`) is AGPL-3.0-or-later like the code unless a file says otherwise.
* Hardware designs, if published, should be CC-BY-SA-4.0 or CERN-OHL-S (decided per design; not
  in v0.1).
* AGPL §13 source offer: any hosted bridge/gateway deployment must offer the corresponding
  source (a link to the repository is sufficient for unmodified deployments; modified
  deployments must publish their changes).

### 2.2 Third-party code policy

| Source license | May we copy code? | May we depend on it? | May we read it as a reference? | Notes |
| --- | --- | --- | --- | --- |
| MIT / Apache-2.0 / BSD | yes (with notice) | yes | yes | most Rust/web deps |
| GPL-3.0 | yes, combined work becomes AGPL-3.0 (GPLv3 §13 permits AGPLv3 combination) | yes, if the dependency graph stays GPL-compatible | yes | e.g. Meshtastic/NomadNet code; prefer a separate process over linking when it keeps licenses clearer |
| AGPL-3.0 | yes | yes | yes | our own license |
| Reticulum License | **no** | no (incompatible terms) | **read-only design reference** | no copied code, no copied tests/fixtures; cite the manual, not the source |
| LXST license | **no** | no | read-only for protocol awareness | explicit "no derivatives or use outside Ratspeak" |
| Proprietary / unlicensed | no | no | no | do not vendor |

Rules of thumb:

1. **Design ideas are not copyrightable; expression is.** Reading a manual and writing a
   compatible implementation is fine; copying source, comments, tests or fixtures is not.
2. **No Reticulum-License or LXST-License code, tests, or assets in this repository.** The M4
   bridge interacts with Reticulum as a *separate process* over sockets, so the two licenses
   never combine into one work (see [nomadnet-bridge.md](../research/nomadnet-bridge.md) §3.1).
   The gateway itself is AGPL-3.0; users install the Reticulum stack under its own license.
3. **Research notes cite URLs and public manual text**, and constants are attributed to a file
   path/commit in a checkout as *observed behaviour*, not copied expression. Where a note cites
   a license clause, it quotes it briefly and points to the binding text.
4. **Firmware dependencies** (RadioLib, Arduino core, LVGL, etc.) are pinned and listed in
   `firmware/NOTICE` with their licenses; GPL-3.0 RNode firmware is not vendored, only
   referenced as a separate program where applicable.
5. **AI-training clauses are a separate legal question.** We do not copy Reticulum-License code
   into an AI training corpus or into OpenMesh. If a contributor wants to use a
   Reticulum-License-derived code snippet, it must be independently written from the manual and
   reviewed.

### 2.3 Contributor/CLA policy

* Contributions are accepted under AGPL-3.0-or-later (inbound = outbound). No CLA that would
  allow relicensing under a non-copyleft license.
* Contributors must state the source of any code that is not their own, and confirm no
  incompatible-license code was copied.
* Generated code (including AI-generated code) is allowed if the contributor has the right to
  submit it and it is reviewed like any other contribution. The project's own AI-assisted workflow
  is disclosed in [ai-development-workflow.md](../research/ai-development-workflow.md).

## 3. Consequences

### 3.1 Positive

* **Network copyleft protects the commons.** A hosted bridge or a vendor firmware fork must
  publish its source; users can audit the encryption and the airtime policy.
* **One license for the whole tree.** No mixed-license confusion for contributors, packagers or
  F-Droid; `cargo-deny`/`license` checks can enforce the dependency policy.
* **Clear third-party boundary.** The M4 bridge can interoperate with Reticulum without
  combining licenses, which is the only practical way given the Reticulum License's terms.
* **The reference-repo research stays legal and reusable.** Documentation (which is our own
  expression) is not contaminated by code copying; the notes are AGPL like the rest of the
  repository.
* **Ecosystem compatibility.** AGPL-3.0 is compatible with the GPLv3 and MIT components in the
  firmware/web dependency graphs; Rust crate dependencies (MIT/Apache-2.0) are fine.

### 3.2 Negative

* **AGPL is a barrier for some commercial adopters.** Vendors who want to ship a closed
  OpenMesh radio or a proprietary hosted service cannot; that is intentional, but it reduces
  the addressable ecosystem (some companies avoid AGPL).
* **App-store and legal review.** AGPL mobile apps are acceptable on F-Droid but have
  historically been awkward for proprietary app stores; Android distribution should lead with
  F-Droid/APK and document the situation.
* **Bridge deployments must offer source.** Operators of modified gateways must publish
  changes; the README/UI should make this easy (link to the repository, version string).
* **No reuse of Reticulum code means re-implementation.** We have to write our own announce
  validation, resources, store-and-forward (ADR 0001/0004) even where the algorithm is
  well-understood; this is slower and riskier than copying.
* **License audits are ongoing work.** Dependencies and firmware libraries change; the NOTICE
  and `cargo-deny` configuration must be maintained.
* **The AI-build narrative and upstream licenses.** Some communities are hostile to
  AI-generated code (the Reticulum/NomadNet ecosystem explicitly is; see the warning in
  NomadNet's README about LLM-generated "fakes"). OpenMesh should be transparent about how it
  was built and should not claim endorsement from those projects.

### 3.3 Mitigations

* `cargo-deny`/`cargo-about` in CI: deny GPL-incompatible and Reticulum/LXST-licensed
  dependencies; generate a dependency license inventory.
* A short `NOTICE` in every app/firmware directory listing third-party components and their
  licenses.
* README statement: "OpenMesh is not Reticulum, is not affiliated with the Reticulum project,
  and does not include Reticulum-licensed code. The M4 bridge interoperates with a separately
  installed Reticulum stack."
* Source-offer link in the bridge's `/api/health`/`hello` payloads and in the desktop About
  dialog ("OpenMesh <version> — AGPL-3.0-or-later — source: ...").
* Release checklist item: re-run license audit; update NOTICE; check new dependencies.

## 4. Alternatives considered

| Alternative | Why rejected |
| --- | --- |
| MIT / Apache-2.0 | Maximum adoption, but a vendor could ship a closed radio or a hosted bridge with no source obligation; contrary to the "commons" goal. |
| GPL-3.0 | Covers distributed software but not network services; a hosted bridge would not have to share changes. |
| AGPL-3.0 **only** (not or-later) | Simpler, but blocks adoption of a future AGPL-4 if the FSF fixes issues; `or-later` is the standard, compatible choice and still copyleft. |
| Dual license (AGPL + commercial) | Needs a CLA and commercial entity; not appropriate for a community project at v0.1, and contradicts "no relicensing" contributor policy. |
| Public domain / CC0 | No protection of the commons, no patent grant, no warranty disclaimer; not suitable for infrastructure. |
| **AGPL-3.0-or-later + strict third-party policy (chosen)** | Keeps the mesh and every client/fork open, including hosted services; compatible with the GPLv3/MIT dependencies we need; forces us away from Reticulum-licensed code, which is the legally safe path anyway. |

## 5. Follow-ups

1. **M0/M1:** add `deny.toml` + `cargo deny check` and a license inventory job to CI; add
   `NOTICE` files for firmware/web/app dependencies.
2. **M1:** add source-offer link and license string to bridge `hello`/`/api/health`, CLI
   `--version`, and the desktop About dialog.
3. **M4:** before shipping the NomadNet bridge, have a lawyer review the "separate process /
   no linking" argument and the Reticulum License field-of-use terms; record the conclusion in
   this ADR (append a dated note).
4. **Release:** F-Droid metadata + reproducible builds for Android; document why AGPL apps may
   not be on all stores.
5. **Community:** a short CONTRIBUTING section on license provenance and AI-generated code,
   consistent with the project's transparency about its own build process.

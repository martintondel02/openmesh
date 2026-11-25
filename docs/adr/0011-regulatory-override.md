# ADR 0011 — Legal-maximum defaults and a warning-gated regulatory override

* **Status:** accepted (v0.5.0)
* **Date:** 2026-11-26
* **Decision record:** MADR-style; referenced from
  [regulatory-and-regions.md](../design/regulatory-and-regions.md) (implementation contract) and
  [PROTOCOL.md §9.5](../architecture/PROTOCOL.md)
* **Related:** [ADR 0002](0002-managed-flooding.md) (airtime), [ADR 0007](0007-media-pipeline.md)
  (media budgets), [ADR 0008](0008-agpl-3-0-licensing.md) (licensing / liability posture)

## 1. Context

OpenMesh is used both for fun and as a **preparedness / backup** communication system when normal
infrastructure fails. Two problems surfaced in v0.4:

1. **The region defaults were unnecessarily conservative and internally inconsistent.** `EU868`
   was modelled as 14 dBm / 1 % duty at 869.525 MHz — that is a hybrid of the 868.0–868.6 MHz
   sub-band rules (25 mW / 1 %) applied to the 869.4–869.65 MHz sub-band, which commonly allows
   500 mW EIRP and 10 %. The result was roughly 10× less airtime and ~13 dB less link budget than
   the law permits. Users comparing with Meshtastic saw a much weaker link for no legal reason.
2. **Preparedness users asked for a "bypass" for wartime/emergencies** that exceeds the legal
   duty cycle, power or frequency limits. This is a real need in life-threatening situations, but
   it is legally sensitive, easy to abuse, and cannot be made "legal" by software.

## 2. Decision

1. **Defaults are the legal maxima** for each region, sourced from CEPT ERC/REC 70-03 Annex 1
   (EU/Norway first), the Norwegian Nkom/EEA framework, and cross-checked against the Meshtastic
   region table. EIRP ceilings, duty cycles and maximum bandwidths are exposed to every client as
   a `legal` object; antenna gain is called out because it changes effective radiated power.
2. **A regulatory override exists, but is warning-gated.** It can change duty cycle, TX power and
   frequency, is advanced-mode only, off by default, requires "I understand" + typing `OVERRIDE`,
   shows a persistent banner while active, is audited in the activity log, and is one tap to
   disable. Validation lives in the core (`RegulatoryOverride::validate`), so every surface
   (web, Android, CLI, MCP) enforces the same rules. Acknowledgement is required only when the
   override **exceeds** the legal limit; reducing power/duty or staying on the legal frequency is
   always allowed without it.
3. **Hardware safety caps remain absolute.** Effective power is
   `min(override or legal max, board PA cap)` (SX1262 22 dBm, LR1110 15 dBm), and out-of-range
   frequencies are rejected. The override can never damage the radio by design.
4. **No "wartime" framing and no automatic emergency detection.** The UI explains that some laws
   recognise life-safety situations (e.g. US 47 CFR §97.405) but that no blanket waiver exists,
   that certification/interference rules still apply, and that the operator is responsible. The
   override is always a deliberate human action.
5. **A legal emergency path ships alongside it**: SOS-style repeated text with coordinates,
   priority/relay/store-and-forward behaviour, and listen-only operation (`duty-pct 0`).

## 3. Alternatives considered

| Option | Why not |
| --- | --- |
| **Legal maxima only, no override** (what we first proposed as option A) | Leaves preparedness users without a response when law/practice genuinely permits extraordinary measures; pushes them to ad-hoc forks with no warnings or audit. |
| **Override in a separate "lab" build only** | Better for liability, but fragments the ecosystem and hides the feature from exactly the users who need it in an emergency; distribution friction is real when infrastructure is down. |
| **Override with warnings in the mainline** *(chosen)* | Precedent exists in comparable open-source radio software (e.g. Meshtastic's duty-cycle override toggle). Warnings, typed confirmation, audit and hard hardware caps keep the risk visible and bounded. |
| **Arbitrary frequency/power with no caps** | Unnecessary for the need, high risk of hardware damage, interference with life-safety bands and certification violation; rejected. |
| **Automatic "emergency mode" detection** | Software cannot reliably detect an emergency; automatic illegal transmission would be indefensible. |

## 4. Consequences

**Positive**

* Defaults match what the law commonly allows: up to 10× the airtime and ~13 dB more link budget in
  the EU, with no protocol change and no extra user effort.
* Preparedness users get a documented, audited path instead of a hidden fork.
* Every interface shares one validation model and one warning string, so behaviour cannot drift.

**Negative / risks**

* **Liability and reputation:** publishing an override can be read as facilitating illegal
  transmissions; the warning/acknowledgement/audit design and "not legal advice" documentation are
  the mitigation, but the project cannot verify each user's legal situation.
* **App stores / certification:** stores and module certifications will not accept "user may
  exceed limits" features; the override is documented as sideload/self-hosted only and must not be
  enabled by default in any store build.
* **Operational risk:** a careless override can block a channel, drain batteries or interfere.
  Mitigations: hardware caps, activity log, persistent banner, one-tap disable, conservative
  hardware defaults on repeaters.
* **Legal review burden:** region tables need periodic review (Nkom, CEPT updates, national
  deviations). EU/NO is the audited path in v0.5; other regions are marked audit-pending.

## 5. Compliance notes

* Region numbers are EIRP ceilings; antenna gain counts towards them. Docs and UI say so.
* Hardware PA caps are enforced in firmware and cannot be raised by the override.
* The override is never enabled by setup wizards, demo modes, agents without `confirm: true`, or
  read-only clients.
* This ADR is not legal advice; operators must verify their local rules.

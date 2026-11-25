# Legal maxima, regions and the regulatory override — v0.5.0 contract

**Status:** normative for v0.5.0 · **Applies to:** `crates/openmesh-core`, `crates/openmesh-bridge`,
`crates/openmesh-cli`, `crates/openmesh-ffi`, `apps/web`, `apps/android`, `firmware/`,
`content/help`, `docs/`
**Decisions (user, 2026-11-26):** defaults are the **legal maxima**; **EU/Norway first**, then the
other regions; **full regulatory override** (duty cycle, TX power, frequency) ships behind an
explicit warning + typed acknowledgement. Codec2 is approved for the following media release —
not part of this change.

> **Not legal advice.** Region values below are our best reading of CEPT ERC/REC 70-03 Annex 1,
> the Norwegian Nkom/EEA framework and the Meshtastic region table, cross-checked 2026-11. National
> law changes and antenna gain change effective radiated power. Every surface that shows these
> numbers must also say "verify your local rules"; the override must carry the warning below.

---

## 1. Region table v2 (single source: `crates/openmesh-core/src/util.rs::REGIONS`)

`Region` keeps its fields; `bw_khz` now means **maximum legal bandwidth** for that region.
Firmware `om_region.h` mirrors this table exactly (host test asserts the numbers).

| Region | Centre (kHz) | Max power (dBm, EIRP) | Duty cycle | Max BW (kHz) | Notes / source |
| --- | --- | --- | --- | --- | --- |
| `EU868` | 869 525 | **27** | **10 %** | **250** | CEPT 869.4–869.65 MHz, 500 mW ERP, ≤10 %; covers Norway (Nkom/EEA). No 500 kHz presets in this band. |
| `EU433` *(new)* | 433 920 | **10** | **10 %** | 250 | CEPT 433.05–434.79 MHz, 10 mW; short range but good propagation. |
| `US915` | 915 000 | **30** | none (fair use) | **500** | FCC 902–928 MHz, 1 W conducted with FHSS/DTS rules; 500 kHz presets allowed. |
| `AU915` | 915 000 | **30** | none (fair use) | 500 | ACMA 915–928 MHz, 1 W EIRP. |
| `IN865` | 865 000 | **30** | none | 250 | India 865–867 MHz; max-BW audit pending, keep 250 kHz conservative. |
| `KR920` | 920 000 | **23** | none | 250 | Korea 920–923 MHz. |
| `AS923` | 923 200 | **20** | **10 %** | 250 | Conservative floor across the AS923 group (SG/MY/TH/ID/PH/VN/JP…); per-country split is future work. |

Rules:

* **Defaults = legal maxima.** A region selection sets `tx_power_dbm = region.max_power_dbm`
  unless the user configured a value, and duty budget = `region.duty_cycle_pct`.
* **Hardware cap:** effective TX power is `min(region.max_power_dbm, board.max_tx_power_dbm)`.
  Board caps: SX1262 boards **22 dBm**, LR1110 (T1000-E) **15 dBm**. Never exceed the hardware cap,
  even under an override.
* **Profile validation:** a profile whose `bw_khz > region.bw_khz` is rejected by the config API /
  CLI / clients; clients fall back to `MEDIUM_FAST` (or `LONG_FAST` on EU433) with a visible note.
  `SHORT_TURBO` (500 kHz) is therefore not selectable in EU868/EU433/IN865/KR920/AS923.
* **Antenna caveat:** the power values are EIRP ceilings. A high-gain antenna can push you over
  the limit; the docs and override modal must say so.
* Beginner setup keeps the existing role presets; only the underlying region numbers change.
  EU/Norway is the verification target; other rows are marked "audit pending" in docs.

---

## 2. Regulatory override

### 2.1 Type (core, reusable by all layers)

```rust
pub struct RegulatoryOverride {
    pub enabled: bool,
    pub acknowledged: bool,          // must be true before `enabled` can become true
    pub duty_cycle_pct: Option<u8>,  // 0..=100; None = region duty
    pub max_power_dbm: Option<i8>,   // clamped to hardware max; None = region max
    pub frequency_khz: Option<u32>,  // optional custom centre; None = region centre
    pub reason: Option<String>,      // short free text, audited (<= 120 chars)
}
pub const OVERRIDE_WARNING: &str = "Regulatory override active — transmissions may violate local radio regulations and void your radio's certification. Use only in a life-threatening emergency where local law permits. You are responsible for compliance and for any interference or damage.";
```

Helpers (core): `validate(region, override)`, `exceeds_legal(region, override)`,
`effective_duty_pct(region, override) -> Option<u8>`,
`effective_power_dbm(region, override, hardware_max) -> i8`,
`effective_frequency_khz(region, override) -> u32`.

Validation rules:

1. `enabled && !acknowledged` → error `"regulatory override requires acknowledgement"`.
2. `duty_cycle_pct > 100` → error. `duty_cycle_pct == Some(0)` means "no transmissions" (legal
   way to go silent); allowed without acknowledgement.
3. Acknowledgement is required only when the override **exceeds the legal limit**:
   `duty > region.duty`, `power > region.max_power`, or `frequency != region.freq`. Reducing power
   or duty, or staying on the region frequency, is allowed without acknowledgement.
4. `reason` longer than 120 chars → truncated or rejected (bridge rejects with 422).
5. Frequency range is validated by the radio layer (≈ 150–960 MHz for SX126x/LR1110); out-of-range
   must be rejected, never silently ignored.
6. `enabled=false` clears the effect but keeps the fields for convenience; the status surfaces
   `override_active=false`.

### 2.2 Effective policy

```
policy = region defaults
if override.enabled:
    duty      = override.duty_cycle_pct ?? region.duty_cycle_pct
    power     = min(override.max_power_dbm ?? region.max_power_dbm, hardware_max)
    frequency = override.frequency_khz ?? region.freq_khz
```

Enforcement points: firmware AirtimeBudget (duty, token bucket), firmware radio config (power,
frequency; PA clamped), bridge airtime estimates + config/status/setup + activity log, Android/web
UI estimates + banner. The protocol wire format does not change: no new frame fields, no vector
regeneration.

### 2.3 JSON surfaces

**Bridge REST (`GET /api/config`, snake_case, additive):**

```json
{
  "radio": { "profile": "MEDIUM_FAST", "region": "EU868", "tx_power_dbm": 27,
             "frequency_mhz": 869.525, "interface": "udp",
             "max_power_dbm": 27, "duty_cycle_pct": 10, "max_bw_khz": 250 },
  "legal": { "region": "EU868", "max_power_dbm": 27, "duty_cycle_pct": 10,
             "max_bw_khz": 250, "source": "CEPT ERC/REC 70-03 Annex 1 (verify local law)" },
  "regulatory_override": { "enabled": false, "acknowledged": false,
    "duty_cycle_pct": null, "max_power_dbm": null, "frequency_khz": null, "reason": null },
  "override_active": false,
  "override_warning": null
}
```

`PATCH /api/config` accepts partial `regulatory_override` (snake_case) and either
`acknowledged: true` or `confirm: true` (alias, same meaning) when the override exceeds legal
limits. Invalid combinations → **422** `{"error":"regulatory_override_requires_acknowledgement",
"message":"…", "warning":"…"}`. Enabling/altering an override appends an `activity` entry
(`source` = `user` or `agent:<name>`, summary without secrets). Disabling is always allowed.

`GET /api/status` gains `regulatory_override_active` + `regulatory_override_warning`.

**Setup payloads:** `GET /api/setup/recommend?…` gains a `legal` object for the recommended region;
`GET /api/setup/state` gains `regulatory_override` (enabled/acknowledged) + `override_warning`
when active. Setup never enables an override.

**Engine / FFI `setConfig(json)` (camelCase, additive):**

```json
{"regulatoryOverride":{"enabled":true,"acknowledged":true,"dutyCyclePct":100,
  "maxPowerDbm":22,"frequencyKhz":null,"reason":"emergency"}}
```

Same validation; invalid → `FfiException.InvalidArgument`. `config_json()` echoes the block plus
`overrideActive`. Core `EngineConfig` carries `regulatory: RegulatoryOverride` and exposes
`effective_duty_pct()`; the engine does not set radio power.

**Firmware companion `ConfigSet` keys (additive):**
`radio.duty_override` (`-1` = legal region duty, `0..100` override),
`radio.frequency` (kHz; only applied when override enabled; out-of-range rejected),
existing `radio.tx_power` (clamped to `min(region.max, board.max)`).
Firmware persists these in its config and applies the token bucket via
`budget_.configure(region, duty_override)`.

### 2.4 CLI and MCP

* `openmesh-cli config show` — prints config + legal maxima + override state (warning banner).
* `openmesh-cli config override --duty-pct N --power-dbm N --frequency-khz N --reason TEXT
  --i-understand-risk` — PATCH `/api/config`; when the override exceeds legal limits the flag is
  mandatory, otherwise the command exits 2 with the warning and does nothing.
  `--disable` turns it off (no acknowledgement needed).
* `openmesh-cli airtime` prints the legal duty/power for the region and uses the override when
  given (`--duty-override`).
* MCP `openmesh_set_config`: accepts `regulatory_override`; enabling/exceeding requires
  `confirm: true` (existing dangerous-tool rule); read-only mode rejects; result includes the
  warning string and the tool table/docs are updated (`docs/agents/mcp.md`, AI-NATIVE §1.1).

---

## 3. UX requirements (web + Android)

**Beginner:** shows "Legal limit for EU868: 27 dBm · 10 % duty" as quiet info; **never** shows the
override.

**Advanced → “Regulatory override (emergency use)”:**

1. Toggle off by default. Turning it on opens a modal with the full warning, an "I understand"
   checkbox and a text field where the user types **`OVERRIDE`**.
2. Numeric fields: duty cycle % (default = legal value), max TX power dBm (default = legal max,
   hard-capped by the radio), frequency kHz (optional, default legal centre; inline antenna
   warning), reason (optional; shown in the activity log).
3. While active: a persistent banner on **every** screen/top bar:
   "⚠ Regulatory override active — may violate local law; you are responsible. Disable in
   Advanced → Regulatory."
   plus a status line in the radio/status card and a notification-channel warning (Android).
4. Disable is one tap (no typing) and clears the effect immediately (firmware budget back to
   legal duty).
5. Setup wizard region card displays the new legal maxima and never offers the override; help
   links to the new topic.
6. Demo mode: payloads include `legal` + an inactive `regulatory_override`, screenshots render.

---

## 4. Docs, help and tests

* `docs/architecture/PROTOCOL.md`: update §9.4 region/airtime table to the values in §1 here; add
  §9.5 "Regulatory policy and override" summarising §2 (no wire change).
* `content/help/`: new topic **`regulatory-and-emergency`** (beginner-safe explanation, emergency
  framing, what the override does, laws/certification/antenna responsibility, how to disable),
  update `radio-and-region` + `index.json` (17 topics); help parity tests stay green.
* `docs/guides/regulatory-and-emergency.md`: operator guide with sources (CEPT ERC/REC 70-03,
  Nkom, Meshtastic cross-check), emergency checklists, override steps, disable steps, liability.
* `docs/adr/0011-regulatory-override.md`: decision, options considered (legal-only, override,
  lab-only), consequences.
* README / CHANGELOG / ROADMAP / QUICKSTART: legal-maximum defaults, override warning, version
  0.5.0; README feature row "legal maxima + regulatory override (warning-gated)".
* Tests: core region values/validation/effective policy; firmware host tests (region parity,
  power clamp, duty override 10→100 %, frequency rejection); bridge API 422/activity/setup/legal
  block + OpenAPI; CLI flag gating; web vitest (modal, typed confirm, banner, legal display, demo);
  Android JVM tests (settings persistence, config JSON, banner, companion keys); `check_help`,
  `check_docs`, `check_openapi`, `cargo fmt/clippy/test`, `pnpm build/test`, Android
  lint/test/assembleDebug, firmware host tests all green.
* **Version:** 0.5.0 (workspace, web, Android versionName/versionCode 5, firmware `OM_FW_VERSION`).

## 5. Explicit non-goals for v0.5.0

* No Codec2/voice-tier work (approved, scheduled next).
* No new wire/protocol fields; vectors unchanged.
* No per-country region split (EU868 covers Norway; AS923 remains a conservative group).
* No automatic “emergency detection”; override is always a deliberate user action.

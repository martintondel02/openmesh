# Legal limits, maximum defaults and the emergency override

**Audience:** operators and advanced users who need to know exactly what the legal limits are,
what OpenMesh enforces, and how the warning-gated override works.
**Status:** v0.5.0 · **Companions:** [PROTOCOL.md §9.5](../architecture/PROTOCOL.md),
[regulatory-and-regions.md](../design/regulatory-and-regions.md) (contract),
[ADR 0011](../adr/0011-regulatory-override.md),
[help: legal limits and emergency override](../../content/help/regulatory-and-emergency.md).

> **Not legal advice.** These values are our reading of public regulation (CEPT ERC/REC 70-03
> Annex 1, Nkom/EEA and the Meshtastic region table) cross-checked in November 2026. National law
> changes; antenna gain changes effective radiated power; certification conditions vary by module.
> You are responsible for compliance.

## 1. What changed in v0.5.0

* Region profiles now default to the **maximum commonly permitted** power and duty cycle instead
  of conservative values. The old `EU868` defaults (14 dBm, 1 %) were a hybrid of the 868.0–868.6
  rules; the 869.4–869.65 sub-band allows much more.
* A new `EU433` region (433.05–434.79 MHz, 10 dBm, 10 %) is available.
* Profile/bandwidth validation: a profile wider than the region maximum is rejected (no
  500 kHz `SHORT_TURBO` in `EU868`).
* A warning-gated **regulatory override** (duty cycle, TX power, frequency) exists in advanced
  mode only, with typed confirmation, a persistent banner, audit logging and one-tap disable.

## 2. Legal maxima table (defaults)

| Region | Centre | Max power (EIRP) | Duty cycle | Max BW | Source / notes |
| --- | --- | --- | --- | --- | --- |
| `EU868` | 869.525 MHz | 27 dBm | 10 % | 250 kHz | CEPT ERC/REC 70-03 Annex 1, 869.4–869.65 MHz, 500 mW; covers Norway (Nkom/EEA). 500 kHz not permitted. |
| `EU433` | 433.92 MHz | 10 dBm | 10 % | 250 kHz | CEPT 433.05–434.79 MHz, 10 mW. |
| `US915` | 915 MHz | 30 dBm | fair use | 500 kHz | FCC 902–928 MHz; FHSS/DTS rules, 1 W class. |
| `AU915` | 915 MHz | 30 dBm | fair use | 500 kHz | ACMA 915–928 MHz. |
| `IN865` | 865 MHz | 30 dBm | fair use | 250 kHz | India 865–867 MHz; max-BW audit pending. |
| `KR920` | 920 MHz | 23 dBm | fair use | 250 kHz | Korea 920–923 MHz. |
| `AS923` | 923.2 MHz | 20 dBm | 10 % | 250 kHz | Conservative floor across the AS923 group; per-country split pending. |

EIRP ceilings: a 6 dBi antenna on a 27 dBm radio is ~33 dBm EIRP (≈ 2 W), over the EU 500 mW
limit. Use the **total** radiated power when checking compliance, and prefer lower power with a
better antenna or a higher repeater.

Norway specifics: Nkom aligns with the EEA/CEPT framework; 869.4–869.65 MHz is the usual
non-specific SRD sub-band (500 mW, ≤ 10 %), and 433.05–434.79 MHz allows 10 mW. Verify current
national rules before deploying a permanent installation.

## 3. What the software enforces

* **Region → legal policy.** The region sets the legal duty/power/bandwidth; the UI and API show
  it as `legal: {max_power_dbm, duty_cycle_pct, max_bw_khz, source}`.
* **Profile validation.** Profiles wider than the region max are rejected with a clear error and
  allowed-profile list.
* **Hardware caps.** Effective TX power is `min(legal or override power, board PA cap)`:
  SX1262 boards 22 dBm, LR1110 (T1000-E) 15 dBm.
* **Duty budget.** Firmware enforces the effective duty cycle with its airtime token bucket;
  clients estimate and warn before expensive sends.
* **Audit.** Every override change is an activity-log entry (source user/agent, no secrets) and
  is visible in the panel's activity table.

## 4. Enabling the override (emergency use only)

**Web:** Advanced mode → **Regulatory override (emergency use)** → toggle → read the warning,
tick "I understand", type `OVERRIDE`, set duty/power/frequency (defaults are the legal values),
optionally a reason → Save.

**Android:** Settings → Advanced → **Regulatory override** → same warning, checkbox, typed
`OVERRIDE`, fields, Save.

**CLI:**

```bash
openmesh-cli config show                       # config + legal maxima + override state
openmesh-cli config override --duty-pct 100 --power-dbm 22 --reason "emergency: wildfire"
# add --i-understand-risk when exceeding the legal limits; without it the command exits 2
openmesh-cli config override --disable         # always allowed, no typing
```

**AI agent (MCP):** `openmesh_set_config` with `regulatory_override` and `confirm: true`;
read-only agents are refused. The warning is returned in the tool result.

Safe actions need no acknowledgement: lowering duty cycle or power, or keeping the region's
frequency. `duty-pct 0` means "listen only, never transmit" and is a legal way to go silent.

## 5. Emergency checklist

1. **Prefer legal resilience first.** Repeated SOS text with coordinates, a repeater, and
   store-and-forward; tell people it is emergency traffic; listen more than you transmit.
2. **Record the decision.** Use the reason field ("wildfire, no other comms") — it lands in the
   activity log and helps you disable it later.
3. **Use the minimum override that helps.** Going from 10 % to 100 % duty can block the channel for
   everyone; consider 25–50 % and shorter messages first.
4. **Keep transmissions short and structured:** who, what, where, how many, needs, battery.
5. **Stop when the emergency ends.** One tap/web/CLI; the next status read shows
   `override_active: false`.
6. **Remember the law.** Some countries recognise life-safety situations (e.g. US 47 CFR §97.405
   for amateur stations in distress); most do not provide a general waiver for ISM duty/power or
   certification rules, and "wartime" is a government matter, not a software setting. If you judge
   that lives are at risk and local law permits, the override exists — you accept responsibility.

## 6. Disabling and recovering

* Web/Android: one tap in the advanced regulatory card.
* CLI: `openmesh-cli config override --disable`.
* If a client or agent keeps re-applying an override, fix the saved config (or remove the client
  from the activity log), then disable again.
* After disabling, restart the radio link if the frequency was overridden (region change may
  require a retune); check the Radio screen and the status endpoint.

## 7. Troubleshooting

| Symptom | Check |
| --- | --- |
| Profile rejected in my region | `bw_khz` > region max (e.g. `SHORT_TURBO` in `EU868`); choose `MEDIUM_FAST`/`LONG_FAST`. |
| API returns 422 `regulatory_override_requires_acknowledgement` | Send `acknowledged: true` (or `confirm: true`); only needed when actually exceeding legal limits. |
| Override has no visible effect | Firmware clamps to the board PA max; check the Radio screen/status for effective power. |
| Duty cycle still limiting after override | Override not active in firmware; check `radio.duty_override` propagation and the activity log. |
| Range still poor at max power | Antenna, height, terrain, profile; power is rarely the limiting factor. |
| Status shows override after I disabled it | Another client/agent re-applied it; inspect the activity log. |

## 8. See also

* [PROTOCOL.md §9.4–9.5](../architecture/PROTOCOL.md) — normative region/airtime and override policy.
* [regulatory-and-regions.md](../design/regulatory-and-regions.md) — implementation contract.
* [ADR 0011](../adr/0011-regulatory-override.md) — why we ship an override at all.
* [content/help/regulatory-and-emergency.md](../../content/help/regulatory-and-emergency.md) —
  beginner-safe in-app topic (web/Android Help Center).
* [BACKBONE.md §9b](../architecture/BACKBONE.md) — exposing a backbone legally and safely.

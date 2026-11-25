# Legal limits, power and emergency override

Radio is regulated everywhere. This topic explains the limits OpenMesh applies by default, what
they mean in practice, and the warning-gated **regulatory override** for genuine emergencies. It
is written to be readable without radio jargon; the advanced section has the numbers.

> **Not legal advice.** Rules differ by country and change over time. You are responsible for
> operating legally. When in doubt, ask your national regulator (in Norway: Nkom) or a licensed
> radio amateur before transmitting.

## What it is

* **Legal maximum defaults.** OpenMesh now ships region profiles set to the **maximum power and
  duty cycle that is commonly permitted** for that band, instead of conservative values. For most
  of Europe and Norway that means the 869.4–869.65 MHz sub-band: up to **27 dBm EIRP** and a
  **10 % duty cycle**. This is still a ceiling, not a guarantee — antenna gain and local rules can
  lower it.
* **Duty cycle.** The share of each hour your radio may transmit. 10 % = 6 minutes of transmit
  time per hour. Every relay that repeats your message spends its own airtime too.
* **TX power.** How loudly the radio transmits, in dBm. More power is not always better: a better
  antenna, a higher repeater, or a slower radio profile usually helps more.
* **Regulatory override.** An **advanced, off-by-default** setting that lets you go beyond the
  region's legal duty cycle, power or frequency. It exists for life-threatening emergencies where
  the law permits extraordinary measures — never for convenience.

## Why it matters

* Transmitting outside the rules can be **illegal**, interfere with other users or life-safety
  services, and **void your radio's certification**.
* Overpowering a radio can **damage the power amplifier** or the battery, and it drains your node.
* A mesh shares one channel: one node sending a large file at maximum power can effectively block
  everyone nearby.
* In a real emergency, the legally useful features are usually **priority, persistence and
  redundancy** (SOS-style repeated messages, store-and-forward, repeaters) — not raw power.

## Quick start (the legal path)

1. **Pick the region that matches where you are.** In the setup wizard choose your country; in
   advanced mode check **Settings → Radio**. Norway and most of Europe → `EU868`.
2. **Leave the defaults alone.** OpenMesh sets the legal maximum power and duty cycle for the
   region. Beginners never see anything more complicated than "Legal limit: 27 dBm · 10 % duty".
3. **Use the recommended profile** (`MEDIUM_FAST`) unless everyone in your group has agreed on a
   different one. Every node and repeater must use the same region and profile.
4. **Prefer a better antenna or a repeater** to more power.
5. **Read the legal limits in the app** (Settings → Radio, advanced mode) before enabling anything
   unusual. The app shows the region's legal maxima and the source note.

## Advanced notes

### What the defaults are

| Region | Max power (EIRP) | Duty cycle | Max bandwidth | Notes |
| --- | --- | --- | --- | --- |
| `EU868` | 27 dBm | 10 % | 250 kHz | 869.4–869.65 MHz; covers Norway (Nkom/EEA, CEPT ERC/REC 70-03 Annex 1). 500 kHz profiles are not permitted. |
| `EU433` | 10 dBm | 10 % | 250 kHz | 433.05–434.79 MHz; short range, good penetration. |
| `US915` | 30 dBm | fair use | 500 kHz | 902–928 MHz; FCC FHSS/DTS rules apply, 500 kHz profiles allowed. |
| `AU915` | 30 dBm | fair use | 500 kHz | 915–928 MHz. |
| `IN865` | 30 dBm | fair use | 250 kHz | India; max-BW audit pending. |
| `KR920` | 23 dBm | fair use | 250 kHz | Korea. |
| `AS923` | 20 dBm | 10 % | 250 kHz | Conservative floor across South-East Asia; per-country rules vary. |

"Fair use" means no hard duty-cycle cap, but you must still not cause interference and must follow
your national rules. Power figures are **EIRP ceilings**: a high-gain antenna can push you over
the limit even at the default setting.

### The regulatory override

The override is available **only in advanced mode**. It can change:

* **Duty cycle** (0–100 %; 0 % means "listen only, never transmit"),
* **Maximum TX power** (clamped to the radio hardware's safe maximum — e.g. 22 dBm for SX1262
  boards, 15 dBm for LR1110 boards),
* **Frequency** (optional custom centre frequency; wrong frequencies can be illegal *and* damage
  antennas/radios).

Enabling it shows this warning and requires you to tick "I understand" and type `OVERRIDE`:

> Regulatory override active — transmissions may violate local radio regulations and void your
> radio's certification. Use only in a life-threatening emergency where local law permits. You are
> responsible for compliance and for any interference or damage.

While it is active, every screen shows a red banner, the radio/status card shows the override, and
the bridge records who changed it in the activity log. **Disabling it is one tap** and restores the
legal duty cycle immediately. Reducing power or duty below the legal limit never needs the
override at all.

The same rule applies through every interface: the web panel (Advanced → Regulatory override), the
Android app (Settings → Advanced), `openmesh-cli config override` (requires
`--i-understand-risk` when exceeding the legal limit) and AI agents through MCP (requires
`confirm: true`).

### Emergency guidance (before you reach for the override)

1. **Stay legal if you can.** A repeated SOS message, a high repeater, and store-and-forward often
   reach help when a single high-power transmission cannot.
2. **Use a dedicated channel** with your group's pre-agreed key and name; tell people it is
   emergency traffic.
3. **Send structured information:** who, what, where (coordinates if possible), how many people,
   immediate needs, battery/status. Repeat periodically.
4. **Listen more than you transmit** to save battery and airtime; enable emergency monitoring on a
   second node if you have one.
5. **Local law may recognise life-safety situations** (e.g. US 47 CFR §97.405 for amateur stations
   in distress, or national emergency laws), but there is **no blanket waiver** for duty-cycle,
   power or certification rules, and "wartime" is not a legal permission that software can grant.
   If lives are at risk and you judge that the law permits it, the override exists — document why,
   keep it short, and disable it when the emergency ends.
6. **Have non-radio fallbacks**: a backbone/gateway over any surviving internet link, satellite
   messenger, paper contact plan, and pre-shared locations.

### Disabling the override

* Web: **Advanced → Regulatory override → Disable**.
* Android: **Settings → Advanced → Regulatory override → Disable**.
* CLI: `openmesh-cli config override --disable`.
* Any of these restores the region's legal power/duty/frequency immediately; the next status read
  shows `override_active: false`.

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| "Profile not allowed in this region" | The profile's bandwidth exceeds the region maximum (e.g. `SHORT_TURBO` 500 kHz in `EU868`). Pick `MEDIUM_FAST` or `LONG_FAST`. |
| Radio transmits less than expected | Legal duty-cycle budget already used; wait for the budget to refill, or reduce media size. Check the airtime counter. |
| Override "does nothing" | The radio may be clamped to its hardware maximum; some boards cannot reach the requested power. Check the status/radio card. |
| Override keeps re-enabling | A client/agent is applying a saved config. Remove it from the bridge config (or CLI `--disable`) and check the activity log. |
| Range is poor even at max power | Antenna, height, terrain and profile matter more. Try `LONG_FAST` and a repeater. |
| Legal limits look wrong for my country | Region profiles are conservative/audited per band; local law may differ. Verify with your regulator and use the override only if the law permits. |

## Related topics

* [Radios, regions and airtime](radio-and-region.md)
* [Security model](security.md)
* [Troubleshooting](troubleshooting.md)

# Radios, regions and airtime

Every OpenMesh message travels over radio at a speed measured in **seconds of airtime**, not
megabits. This topic explains the two settings you must get right (region and profile), what they
cost, and how to recognise when a message is too expensive.

## What it is

* **Region** — the legal frequency band and power limit for where you are. It is not a preference;
  transmitting outside your region's rules can be illegal and may interfere with other users.
* **Profile (modem preset)** — the spread-spectrum settings: spreading factor (SF), bandwidth
  (BW) and coding rate (CR). Slower profiles hear weaker signals at longer range; faster profiles
  send more data per second. Nodes must use the **same** profile and region to talk.
* **Airtime** — the time the radio spends transmitting. Regulators cap it (for example 10 % duty
  cycle in the EU 869.4–869.65 MHz sub-band; the 868.0–868.6 MHz sub-band allows 1 % at lower
  power), and the mesh shares one channel, so every relay spends airtime too.
* **Duty cycle** — the percentage of each hour a transmitter may be on air. A 10 % limit means
  6 minutes of transmit time per hour, shared by everything the node sends.

## Why it matters

Choose the wrong region and your setup is illegal or deaf. Choose a too-fast profile and you lose
range; choose a too-slow profile and text feels fine but a photo takes minutes. Understanding
airtime explains why OpenMesh shows an estimate before sending media, and why "just send a
picture" is not free on a shared channel.

## Quick start

1. Open **Settings → Radio** (advanced mode) or accept the region from the setup wizard.
   Pick your country; OpenMesh maps it to a region (EU/UK/NO/CH/IS → `EU868`, US/CA/MX → `US915`,
   AU/NZ → `AU915`, IN → `IN865`, KR → `KR920`, SG/MY/TH/ID/PH/VN → `AS923`).
2. Leave the profile on the recommended default (`MEDIUM_FAST` for most people). It balances range
   and speed, and it is what the beginner presets use.
3. Keep `radio.tx_power` at or below your region's limit. Higher power rarely beats a better
   antenna or a higher repeater.
4. Make sure every node and repeater in your group uses the same region and profile.

## Advanced notes

### The profiles, and what they cost

Airtime below is for one full 240-byte frame (the OMP/1 MTU) on each profile, including preamble,
header and CRC. "5 KiB image" includes the manifest, 29 data chunks and ACKs.

| Profile | SF / BW / CR | One 240 B frame | 5 KiB image | Relative range | Use it when |
| --- | --- | --- | --- | --- | --- |
| `LONG_SLOW` | 12 / 125 kHz / 4/8 | ~13.5 s | ~6 min | longest | desperate range, tiny text only |
| `LONG_FAST` | 11 / 250 kHz / 4/5 | ~2.0 s | ~61 s | long | rural areas, few nodes, text-first |
| `MEDIUM_FAST` | 9 / 250 kHz / 4/5 | ~0.61 s | ~18 s | balanced | default for most groups |
| `SHORT_FAST` | 7 / 250 kHz / 4/5 | ~0.19 s | ~6 s | short | dense sites, fast media |
| `SHORT_TURBO` | 7 / 500 kHz / 4/5 | ~0.10 s | ~3 s | shortest | events/campuses with line-of-sight |

These are computed estimates from the LoRa airtime formula, not measured on your hardware. Range
depends on terrain, antennas and noise; speed and range trade off.

### Airtime budgets in practice

* A 10 % duty cycle allows ~6 minutes of transmit time per hour. A 5 KiB image at `MEDIUM_FAST`
  uses ~18 s — about 5 % of the hourly budget, before relays. A 1 % band (such as 868.0–868.6 MHz
  in parts of Europe) allows only ~36 s per hour, so the same image costs half the budget.
* A 100-character text at `MEDIUM_FAST` costs ~0.5 s. That is the reference point the app uses for
  comparisons ("about the same as six text messages").
* Every relay in range also retransmits, so dense meshes spend more aggregate airtime. This is
  why OpenMesh caps media per profile and warns before expensive sends.
* Airtime is why `LONG_SLOW` is a last resort: a single photo can occupy the channel for minutes.

### Region table (legal-maximum defaults, v0.5)

Since v0.5, OpenMesh defaults to the **legal maximum** power and duty cycle for the region instead
of conservative values. The numbers are EIRP ceilings — antenna gain counts, so a high-gain antenna
can put you over the limit even at the default setting.

| Region | Typical band | Max power | Duty cycle | Max bandwidth |
| --- | --- | --- | --- | --- |
| `EU868` | 869.4–869.65 MHz (covers Norway) | 27 dBm | 10 % | 250 kHz |
| `EU433` | 433.05–434.79 MHz | 10 dBm | 10 % | 250 kHz |
| `US915` | 902–928 MHz | 30 dBm | fair use | 500 kHz |
| `AU915` | 915–928 MHz | 30 dBm | fair use | 500 kHz |
| `IN865` | 865–867 MHz | 30 dBm | fair use | 250 kHz |
| `KR920` | 920–923 MHz | 23 dBm | fair use | 250 kHz |
| `AS923` | 923 MHz | 20 dBm | 10 % | 250 kHz |

Exact legal limits vary by sub-band and country, and rules change. Verify with your local
regulator (in Norway: Nkom) and your hardware's certification before deploying a public network.
A profile whose bandwidth exceeds the region maximum (for example `SHORT_TURBO` at 500 kHz in
`EU868`) is rejected by the app.

If you need to go beyond the legal limits in a life-threatening emergency, read
[Legal limits, power and emergency override](regulatory-and-emergency.md) — it is warning-gated,
advanced-only and off by default.

### Changing settings safely

* Region changes can require a radio restart; do it from Settings and re-check the Nodes screen.
* Changing profile disconnects nodes using the old profile — plan it for a quiet moment and tell
  your group.
* `radio.frequency` (like power and duty) is part of the warning-gated regulatory override; leave
  it unset unless you are deliberately testing or responding to a genuine emergency.
* Use the API (`GET/POST /api/config`) or the Settings screen; avoid hand-editing files while the
  bridge is running.

## Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| Nodes list is empty but radios "work" | Profile or region mismatch between nodes | Set the same profile/region everywhere, including repeaters. |
| Messages work at short range only | Profile too fast for terrain, or antenna/power issue | Try `LONG_FAST`; check antenna and placement. |
| Media fails or takes minutes | Profile too slow for the image size, or duty-cycle budget exhausted | Send smaller media, use a faster profile locally, or wait for the next hour budget. |
| "Duty cycle" warning in Settings | Your region caps transmit time and the estimate exceeds it | Reduce media size/frequency; the app will ask for confirmation. |
| Airtime percentages look wrong | Percentages are per-node and reset with uptime | Compare over an hour, not immediately after restart. |

## Related topics

[Images and voice notes](media.md) · [Nodes, roles and telemetry](nodes-telemetry.md) ·
[Legal limits, power and emergency override](regulatory-and-emergency.md) ·
[Build a tiny mesh](first-mesh.md) · [Troubleshooting](troubleshooting.md)

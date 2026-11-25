# Antennas and siting — a short practical guide

**Audience:** anyone deploying an OpenMesh repeater or tracker.
**Scope:** 868 / 915 / 923 MHz LoRa, the four reference boards. Not a replacement for local
regulations.

---

## 1. The 30-second version

1. **Height beats gain.** A 3 dBi whip on a 2 m pole above the roof will out-talk a 12 dBi panel
   in the attic, because radio paths are limited by obstacles and Fresnel-zone clearance, not by
   raw gain.
2. **Line of sight is everything.** LoRa at SF9/250 kHz has ~150 dB of link budget. Each wall,
   hill or tree belt costs 10–30 dB. Get above the clutter if you can.
3. **Keep the antenna away from the enclosure and the solar panel.** Metal and panels detune
   antennas and block the horizon.
4. **Match the band.** 868 MHz for EU/UK, 915 MHz for US/AU, 923 MHz for AS. Wrong-band antennas
   can still work badly — and may be illegal to transmit with at higher power.
5. **Never transmit without an antenna.** The PA is the most expensive part of the radio.

---

## 2. Link budget, quickly

```
received_power_dBm = tx_power + tx_antenna_gain − cable_loss
                    − free_space_path_loss(distance, frequency)
                    + rx_antenna_gain − misc_losses
link_budget_dB     = tx_power − rx_sensitivity
```

* SX1262/LR1110 sensitivity at SF9/250 kHz is roughly **−120 dBm** (SF12/125 kHz: −137 dBm).
* At 14 dBm, 0 dBi antennas and no cable loss: budget ≈ 134 dB.
* Free-space path loss at 868 MHz: **91.5 dB at 1 km**, and it increases 6 dB per doubling of
  distance. So ~14 km free-space is the *best case*; with terrain and trees expect 1–5 km from a
  ridge-top repeater, 300 m–2 km at ground level.
* Every **6 dB** lost = half the range. A 3 m SMA pigtail can cost 1–2 dB (≈25 % range). A metal
  enclosure can cost 10–20 dB.

Practical range rule of thumb for the reference boards:

| Situation | Typical range |
| --- | --- |
| Two trackers at ground level in a city | 300–800 m |
| Tracker to a rooftop repeater | 2–5 km |
| Ridge-top repeater to ridge-top repeater, line of sight | 15–40 km |
| Ridge repeater to valley town | 5–15 km |
| Inside buildings | 1–2 walls, not floors |

---

## 3. Antenna choices

| Antenna | Gain | When to use | Cautions |
| --- | --- | --- | --- |
| Quarter-wave whip (86 mm @ 868, 82 mm @ 915) | 2 dBi | Default on trackers, small enclosures | Needs a ground plane; body/hand detuning |
| Half-wave dipole | 2–2.5 dBi | General purpose, repeaters | 165 mm @ 868 MHz; keep vertical |
| 5/8 λ whip | 3–5 dBi | Repeaters with a small mast | Needs a ground plane/radials |
| Collinear (2× 5/8) | 5–6 dBi | Ridge-top repeaters, omni coverage | Long, needs a mast, 1.6 m @ 868 MHz |
| Yagi / panel | 7–12 dBi | Point-to-point ridge links only | Narrow beam — useless for serving many peers |
| PCB/inverted-F (internal) | 0–2 dBi | T1000-E and compact trackers | Sensitive to enclosure and body placement |

**Polarisation matters.** Keep every antenna vertical (or all horizontal). A 90° mismatch costs
~20 dB.

**Cable loss** (@ 868 MHz, approximate):

| Cable | dB/m | 1 m | 3 m | 5 m |
| --- | --- | --- | --- | --- |
| RG-174 (thin pigtail) | 1.0 | 1.0 | 3.0 | 5.0 |
| RG-58 | 0.6 | 0.6 | 1.8 | 3.0 |
| LMR-200 | 0.3 | 0.3 | 0.9 | 1.5 |
| LMR-400 | 0.13 | 0.13 | 0.4 | 0.65 |

Use the shortest, thickest coax that reaches, or put the radio at the mast head in a sealed box
(with a USB/solar cable) and run DC, not RF.

---

## 4. Siting a solar repeater

A repeater is infrastructure. Treat it like a hilltop radio site, not like a gadget:

1. **Pick for line of sight to the people, not to another repeater.** Draw the coverage you want
   on a map; choose the closest point that sees them.
2. **Height**: 1–2 m above the roof (or the top of the mast) is usually the sweet spot. Doubling
   mast height from 1 m to 2 m can add several km; going from 10 m to 20 m rarely does.
3. **Fresnel zone**: for a 5 km link at 868 MHz, the first Fresnel zone radius at midpoint is
   ≈ 9 m. Keep the path ~60 % clear of obstructions.
4. **Solar panel**: face south (north in the southern hemisphere) at `latitude × 0.87` degrees,
   clear of shadows from the antenna mast. Panels and antennas both like height, but put the
   antenna above the panel and offset it, not in the panel's plane.
5. **Battery**: size for 7–10 dark days. See `boards.md` §4.3 for the mAh/day numbers and panel
   sizing formula.
6. **Enclosure**: IP66/67 with a breathable vent. Desiccant inside; cable glands pointing down.
7. **Grounding**: mast grounded, coax surge arrestor at the enclosure entry. LoRa is low-cost but
   a lightning hit also kills the solar controller and battery.
8. **Serviceability**: you will climb the mast again. Use a waterproof connector for the radio,
   keep the SD/USB port reachable, and label the node id.

### Do not

* Mount the antenna inside a metal box, on a metal roof, or flat against a wall.
* Coil excess coax into a tight loop (choke ok, bundle bad).
* Point a Yagi at a town if you want to serve all directions — use an omni or two sectors.
* Rely on a solar node in deep winter without checking the panel's snow shedding.

---

## 5. Tracker placement

* **T1000-E**: wear it with the printed antenna area up and away from the body; a pocket full of
  keys/phone costs 3–6 dB. IP65 is splash-proof, not swim-proof.
* **Heltec Wireless Tracker**: the external IPEX antenna matters more than the screen. Keep the
  antenna vertical and away from the ESP32-S3's own 2.4 GHz antenna.
* **GNSS**: the LoRa antenna and the GNSS patch are different bands, but a 868/915 MHz antenna
  next to a GNSS patch can desensitise the front end. Keep them >5 cm apart.
* **Motion**: for vehicle tracking, place the node high in the cabin or on the roof (external
  antenna); a node under the seat may never hear a repeater.

---

## 6. Legal and duty-cycle notes

* EU868/AS923 have a **1 % duty cycle** per sub-band. The firmware enforces a token bucket and
  will drop frames rather than exceed it; do not increase power or reduce the budget expecting
  better throughput — the radio is mostly limited by the duty cycle, not the PA.
* US915/AU915 are fair-use with dwell-time and frequency-hopping expectations; the reference
  firmware uses a single centre frequency, so check your local rules for narrowband operation.
* The antenna gain and cable loss affect EIRP; the firmware clamps TX power to the region maximum
  (`region.max_power_dbm`), but **you** are responsible for the installed antenna gain.

---

## 7. Quick checklist before you deploy

- [ ] Antenna attached, vertical, correct band.
- [ ] Enclosure sealed, desiccant, cable glands down.
- [ ] Solar panel angled, clean, unshaded; battery voltage >3.7 V after a day.
- [ ] Node announces (visible to a nearby tracker or the companion app).
- [ ] Region and channel PSK configured consistently across the mesh.
- [ ] Repeater in line of sight to the area it should cover; test with a tracker walk.
- [ ] Mast grounded; coax connections weather-proofed.
- [ ] Record node id, location, and firmware version for the mesh map.

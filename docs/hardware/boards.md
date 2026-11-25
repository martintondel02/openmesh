# OpenMesh hardware support

**Status:** v0.1.0 · normative for the four reference boards
**Firmware:** `firmware/` (PlatformIO, C++17) · **Flashers:** `apps/flasher/` (web), `tools/flash/` (CLI)

OpenMesh v0.1 supports four reference boards. All four run the same firmware codebase; the board
difference is a pin map (`firmware/boards/<board>.cpp`), a radio configuration (SX1262 or
LR1110) and a default role.

---

## 1. Support matrix

| | Seeed SenseCAP T1000-E | RAK WisBlock 4631 (solar enclosure) | Seeed SenseCAP Solar Node P1 | Heltec Wireless Tracker |
| --- | --- | --- | --- | --- |
| **PlatformIO env** | `t1000e` | `rak4631_solar` | `seeed_solar` | `heltec_tracker` |
| **MCU** | nRF52840, Cortex-M4F 64 MHz | nRF52840, Cortex-M4F 64 MHz | nRF52840, Cortex-M4F 64 MHz | ESP32-S3, dual Xtensa LX7 |
| **Radio** | Semtech LR1110 | Semtech SX1262 (RAK4630 module) | Semtech SX1262 | Semtech SX1262 |
| **Radio driver** | RadioLib `LR1110` | RadioLib `SX1262` | RadioLib `SX1262` | RadioLib `SX1262` |
| **TCXO** | 1.6 V (DIO3) | 1.8 V (DIO3) | 1.8 V (DIO3) | 1.8 V (DIO3) |
| **RF switch** | internal (LR1110) | DIO2 | DIO2 + external RXEN | DIO2 |
| **GNSS** | Airoha (115200 baud) | optional RAK1910/RAM12500 (9600) | L76KB (9600) | L76K/UC6580 (115200) |
| **Screen** | none | none | none | 0.96" ST7735S TFT 160×80 |
| **Flash** | 1 MiB internal | 1 MiB internal | 2 MiB QSPI (P25Q16H) | 8 MiB |
| **USB** | USB-C CDC (TinyUSB) | USB-C CDC | USB-C CDC | USB-C CDC + USB-JTAG |
| **BLE** | Bluefruit NUS | Bluefruit NUS | Bluefruit NUS | NimBLE NUS |
| **WiFi** | no | no | no | yes (ESP32-S3) |
| **Solar** | no (USB charge only) | yes (solar enclosure/MPPT) | yes (integrated) | no |
| **Battery** | 700 mAh LiPo, IP65 | external pack (18650/LiPo) | integrated pack, IP66 | USB / small LiPo |
| **Antenna** | internal (soldered) | external SMA/IPEX | external IPX | external IPEX (2.4 GHz + LoRa) |
| **Default role** | tracker/client | repeater | repeater/gateway | tracker/client |
| **Bootloader** | Adafruit UF2 | Adafruit UF2 (or RAK bootloader) | Adafruit UF2 | Espressif (esptool/USB-JTAG) |
| **Flash method** | UF2 copy, CLI, nrfutil | UF2 copy, CLI, nrfutil/J-Link | UF2 copy, CLI, nrfutil | WebSerial, esptool, CLI |

All boards are supported by the same firmware feature set: signed announces, managed flood
relaying, deduplicated forwarding, AEAD direct and channel messages, acknowledgements,
store-and-forward on repeaters, USB/BLE companion link, GNSS NMEA parsing, battery/solar
telemetry and region duty-cycle budgeting. Store-and-forward is enabled by default on repeaters
and gateways only (it needs the larger flash/queue budget and mains/solar power).

---

## 2. Pinouts

Pin numbers are **Arduino/RadioLib pin indices** as used by the vendor Arduino core. On the nRF52
boards these equal the raw nRF GPIO number (`port × 32 + pin`), which is the convention of the
vendor variants. Sources are listed per board; the firmware definition lives in
`firmware/boards/<board>.cpp`.

### 2.1 Seeed SenseCAP T1000-E

Source: vendor nRF52 variant (Seeed open hardware) — see `firmware/NOTICE`.

| Function | Pin | nRF GPIO | Notes |
| --- | --- | --- | --- |
| Radio SCK | 11 | P0.11 | |
| Radio MISO | 40 | P1.08 | |
| Radio MOSI | 41 | P1.09 | |
| Radio NSS | 12 | P0.12 | |
| Radio RESET | 42 | P1.10 | |
| Radio DIO1/IRQ | 33 | P1.01 | |
| Radio BUSY | 7 | P0.07 | DIO2 used as BUSY |
| Radio TCXO | — | DIO3 | 1.6 V, internal RF switch |
| GNSS UART TX (MCU) | 13 | P0.13 | Serial1 |
| GNSS UART RX (MCU) | 14 | P0.14 | Serial1 |
| GNSS enable | 43 | P1.11 | active HIGH |
| GNSS reset | 47 | P1.15 | |
| Battery ADC | 2 | P0.02 / AIN0 | ×2.0 divider, 12-bit |
| Charger detect | 36 | P1.04 | |
| USB detect | 5 | P0.05 | |
| Status LED | 24 | P0.24 | green |
| User button | 6 | P0.06 | |
| Sensor rail enable | 4 | P0.04 | |
| 3V3 enable | 38 | P1.06 | |
| Buzzer enable | 37 | P1.05 | |

### 2.2 RAK WisBlock 4631 (RAK solar enclosure)

Source: RAK4631/RAK4630 datasheet and RAK variant definitions.

| Function | Pin | nRF GPIO | Notes |
| --- | --- | --- | --- |
| Radio SCK | 43 | P1.11 | |
| Radio MISO | 45 | P1.13 | |
| Radio MOSI | 44 | P1.12 | |
| Radio NSS | 42 | P1.10 | |
| Radio DIO1/IRQ | 47 | P1.15 | |
| Radio BUSY | 46 | P1.14 | |
| Radio RESET | 38 | P1.06 | |
| Radio power enable | 37 | P1.05 | must be HIGH before init |
| Radio TCXO | — | DIO3 | 1.8 V, DIO2 = RF switch |
| GNSS UART TX (MCU) | 16 | P0.16 | Serial1 (optional module) |
| GNSS UART RX (MCU) | 15 | P0.15 | Serial1 |
| 3V3_S enable (GNSS) | 34 | P1.02 | HIGH powers peripheral rail |
| GNSS PPS | 17 | P0.17 | optional |
| Battery ADC | 5 | P0.05 / AIN0 | ×1.73 divider, 12-bit |
| Green LED | 35 | P1.03 | |
| Blue LED | 36 | P1.04 | |
| User button | 9 | P0.09 | |
| Buzzer | 21 | P0.21 | optional RAK18001 |

### 2.3 Seeed SenseCAP Solar Node P1

Source: vendor variant definitions and Seeed SenseCAP Solar Node documentation.

| Function | Pin (vendor) | MCU/nRF GPIO | Notes |
| --- | --- | --- | --- |
| Radio SCK | D8 | P1.13 | |
| Radio MISO | D9 | P1.14 | |
| Radio MOSI | D10 | P1.15 | |
| Radio NSS | D4 | 4 | |
| Radio DIO1/IRQ | D1 | 1 | |
| Radio BUSY | D3 | 3 | |
| Radio RESET | D2 | 2 | |
| Radio RXEN | D5 | 5 | external RF switch RX enable |
| Radio TCXO | — | DIO3 | 1.8 V, DIO2 = RF switch |
| GNSS UART TX (MCU) | D7 | 7 | L76KB |
| GNSS UART RX (MCU) | D6 | 6 | L76KB |
| GNSS enable | D18 | 18 | |
| GNSS reset | D17 | 17 | |
| GNSS standby/wake | D0 | 0 | |
| Battery ADC | D16 | P0.31 | ×3.3 divider |
| Battery read enable | D19 | P0.14 | |
| LED | 12 | P0.12 | |
| User button | D13 | 13 | |

The P1 uses a XIAO-nRF52840-class pin map; the D-numbered macros in the vendor variant are the
authoritative Arduino pin numbers.

### 2.4 Heltec Wireless Tracker V1.1

Source: Heltec schematic and vendor variant definitions.

| Function | Pin | Notes |
| --- | --- | --- |
| Radio SCK | 9 | |
| Radio MISO | 11 | |
| Radio MOSI | 10 | |
| Radio NSS | 8 | |
| Radio DIO1/IRQ | 14 | |
| Radio BUSY | 13 | |
| Radio RESET | 12 | |
| Radio TCXO | — | 1.8 V, DIO2 = RF switch |
| GNSS UART TX (MCU) | 34 | UC6580/L76K |
| GNSS UART RX (MCU) | 33 | |
| GNSS reset | 35 | |
| GNSS PPS | 36 | |
| GNSS/Front-end power (VEXT) | 3 | active HIGH, also powers TFT/LNA |
| Battery ADC | 1 | GPIO1, ×4.9·1.045 divider |
| ADC divider enable | 2 | active HIGH |
| TFT CS | 38 | ST7735S |
| TFT DC | 40 | |
| TFT RESET | 39 | |
| TFT backlight | 21 | |
| TFT SCK | 41 | |
| TFT MOSI | 42 | |
| Status LED | 18 | |
| User button | 0 | BOOT button |

---

## 3. Flashing

### 3.1 Web flasher (easiest, ESP32-S3 only)

Open `apps/flasher/index.html` over `http://localhost:8000` (WebSerial needs a secure context;
`file://` works for everything except in-browser flashing). Click **Flash from this browser** on
the Heltec card. Chrome/Edge 89+ required. nRF52 cards link to the UF2 download and show
drag-to-drive instructions.

### 3.2 CLI (`tools/flash/openmesh-flash`)

Works for every board, no root, resolves the latest GitHub release and verifies SHA-256.

```bash
tools/flash/openmesh-flash --list
tools/flash/openmesh-flash --board auto --dry-run
tools/flash/openmesh-flash --board t1000e
tools/flash/openmesh-flash --board heltec_tracker --port /dev/ttyACM0
```

Fedora permissions and udev notes: `tools/flash/README.md`.

### 3.3 nRF52 boards: UF2 drag-and-drop

1. Put the board in bootloader mode:
   * T1000-E: double-tap the side button. Drive name `T1000E`.
   * RAK4631 solar: double-tap reset. Drive name `RAK4631`.
   * Seeed Solar P1: double reset. Drive name `SEEEDSOLAR`.
2. A mass-storage drive appears (the board also reboots into it automatically if you copy while
   running the Adafruit bootloader is not possible — enter bootloader first).
3. Copy `openmesh-<board>.uf2` to the drive. The bootloader verifies and reboots.
4. First boot generates the node identity and starts announcing.

If a RAK4631 has the stock RAK bootloader instead of the Adafruit UF2 bootloader, flash with
`nrfutil` over USB or a J-Link/SWD probe:

```bash
# nrfutil (Nordic) with a .zip DFU package produced from the PlatformIO .hex
nrfutil pkg generate --hw-version 52 --sd-req 0x00 \
  --application firmware/.pio/build/rak4631_solar/firmware.hex package.zip
nrfutil dfu usb-serial -pkg package.zip -p /dev/ttyACM0
# or SWD
openocd -f interface/cmsis-dap.cfg -f target/nrf52.cfg \
  -c "program firmware/.pio/build/rak4631_solar/firmware.elf verify reset exit"
```

### 3.4 ESP32-S3: esptool

```bash
# merged image (offset 0) — what releases and the web flasher use
esptool.py --chip esp32s3 -p /dev/ttyACM0 -b 921600 \
  write_flash 0x0 openmesh-heltec_tracker-full.bin

# or from a PlatformIO build (bootloader + partitions + app)
esptool.py --chip esp32s3 -p /dev/ttyACM0 -b 921600 \
  write_flash --flash_mode dio --flash_freq 80m --flash_size 8MB \
  0x0 firmware/.pio/build/heltec_tracker/bootloader.bin \
  0x8000 firmware/.pio/build/heltec_tracker/partitions.bin \
  0xe000 ~/.platformio/packages/framework-arduinoespressif32/tools/partitions/boot_app0.bin \
  0x10000 firmware/.pio/build/heltec_tracker/firmware.bin
```

Force download mode with BOOT + RESET if the port is busy or the board does not enumerate.

### 3.5 Building from source

```bash
python3 -m pip install -U platformio
cd firmware
pio run -e t1000e           # or rak4631_solar, seeed_solar, heltec_tracker
pio run -e t1000e -t upload # uses nrfutil/esptool as configured
pio test -e native          # host protocol tests (platform-native compiler)
```

The nRF52 environments use the Meshtastic `platform-nordicnrf52` fork for its board definitions
(`tracker-t1000-e`, `wiscore_rak4631`, `seeed_solar_node`) and the Adafruit nRF52 Arduino core.
No Meshtastic application code is used. Details and pin-map citations: `firmware/README.md` and
`firmware/NOTICE`.

---

## 4. Power budget

All numbers are **estimates from datasheets and vendor documentation** for v0.1 firmware. They
are targets to be measured on real hardware (see `firmware/README.md` "Known issues"); treat them
as ±30 %. Measured values replace this table when field data lands.

### 4.1 Current by mode

| Mode | T1000-E (nRF52) | RAK4631 solar | Seeed Solar P1 | Heltec Tracker (ESP32-S3) |
| --- | --- | --- | --- | --- |
| MCU active (radio RX idle) | ~7 mA | ~7 mA | ~7 mA | ~45 mA |
| + SX1262/LR1110 RX | ~13 mA | ~13 mA | ~13 mA | ~52 mA |
| TX burst @ 14 dBm (240 B) | ~55 mA for ~610 ms | ~55 mA | ~55 mA | ~95 mA |
| GNSS tracking | ~25 mA (Airoha) | ~25 mA (optional) | ~20 mA (L76KB) | ~25 mA (UC6580) |
| TFT on | — | — | — | +15–25 mA |
| Light sleep (RTC tick) | ~1.5 mA | ~1.5 mA | ~1.5 mA | ~250 µA |
| Deep sleep (MCU off, radio off) | ~15 µA | ~15 µA | ~15 µA | ~10 µA |

### 4.2 Average consumption by role

Assumptions: MEDIUM_FAST (SF9/250 kHz), 240-byte frame ≈ 610 ms airtime, announce every 30 min
(clients) / 60 min (repeaters), position every 5 min for trackers, telemetry every 15 min,
receiver left on (current firmware — see known issues).

| Role | Dominant cost | Estimated average | Battery life / energy |
| --- | --- | --- | --- |
| T1000-E tracker (RX on, position 5 min) | 13 mA RX + 1.5 s/h TX + 25 mA GNSS 2 min/5 min | **~13–16 mA** | 700 mAh ÷ 14.5 mA ≈ **2 days** (GNSS on); weeks with deep-sleep firmware (roadmap) |
| Heltec tracker (WiFi off, screen dim, GNSS duty) | 52 mA RX + screen + GNSS | **~75–95 mA** | 1000 mAh pack ≈ **11–13 h**; USB-powered typical |
| RAK4631 repeater (solar, RX 24/7) | 13 mA RX | **~13 mA ⇒ 312 mAh/day** (1.15 Wh/day) | 3000 mAh pack ≈ 9 days dark; 5 W panel recovers in ~1.5 peak-sun h |
| Seeed Solar P1 repeater/gateway (GNSS 10 min/h) | 13 mA RX + GNSS duty | **~15 mA ⇒ 360 mAh/day** (1.33 Wh/day) | integrated pack; 5 W panel with 3 peak-sun h ≈ 4× margin |

Airtime is negligible for power: at 610 ms per 240 B and an announce every 30 min, transmit duty
cycle is ~0.03 %. The receiver (and GNSS for trackers) dominates. This is why repeaters are
solar nodes and trackers must deep-sleep between fixes (roadmap M1).

### 4.3 Solar sizing

```
daily_energy_mAh = average_current_mA × 24 h
panel_needed_W   = daily_energy_mAh × 3.7 V ÷ (peak_sun_hours × derate)
```

Use **derate = 0.55** for a fixed, non-tilted solar panel in northern Europe (winter, dirt,
converter losses) and **1.0 h** minimum winter peak-sun equivalent for dimensioning, or add a
battery sized for 7–10 dark days.

Worked examples:

* RAK repeater, 312 mAh/day: `0.31 Ah × 3.7 V = 1.15 Wh/day`. With 1.5 winter peak-sun h and
  0.55 derate: `1.15 ÷ (1.5 × 0.55) ≈ 1.4 W` panel minimum. A **5 W panel** gives ~3.5× winter
  margin and covers a 7-day dark spell with a 3 Ah pack (7 × 0.31 = 2.2 Ah).
* Seeed Solar P1, 360 mAh/day: `1.33 Wh/day` → ~1.6 W minimum; the integrated 5 W panel is
  comfortable.
* Add 20 % headroom if the node also acts as a store-and-forward relay during high traffic.

Keep the solar panel **above the snow line and clear of the antenna**, angle it at
`latitude × 0.87` degrees from horizontal (northern hemisphere, facing south), and use an MPPT
charger. A flat panel on a ridge collects 30–50 % less than an angled one in winter.

---

## 5. Enclosure and antenna guidance

* **Antenna first, radio second.** Never power the radio without an antenna. At 14 dBm the
  SX1262/LR1110 is tolerant for short tests, but a mismatched load can still damage the PA over
  time.
* **nRF52 boards with internal antennas** (T1000-E): keep the device away from metal, and do not
  enclose it in a metal case or place it on a metal surface. The antenna is at the opposite end
  from the USB connector.
* **External antennas** (RAK, Seeed P1, Heltec): use a half-wave dipole or a 5/8 λ whip tuned for
  the region (868 MHz EU, 915 MHz US/AU, 923 MHz AS). A 3 dBi whip on a 1–2 m mast outperforms a
  10 dBi patch aimed at the wrong ridge.
* **Cable loss matters more than antenna gain.** RG-58 at 868 MHz loses ~0.6 dB/m; LMR-200
  ~0.3 dB/m. Keep the radio and antenna close, or accept the loss. Never coil excess coax.
* **IP rating**: T1000-E is IP65 (splash), the RAK solar enclosure and Seeed P1 are IP66/67.
  If you add a cable gland, use a breathable membrane vent to avoid condensation.
* **Thermal**: Li-ion loses capacity below 0 °C and must not charge below 0 °C. Solar nodes in
  freezing climates need the battery insulated or a LiFePO4 pack.
* **Lightning**: a mast on a ridge is a lightning target. Ground the mast, use a coax
  surge arrestor, and keep the radio in a grounded enclosure — or accept the risk for a cheap
  node.
* **Mounting**: ridge-top or above the tree line beats a 10 dBi antenna in a valley. Height
  gains (Fresnel zone clearance) matter more than gain. Aim for line of sight over the expected
  peer set; 1–2 m above the roof is a good compromise.

---

## 6. Sourcing

Prices are indicative and change; verify region/band before ordering.

| Item | Where | Notes |
| --- | --- | --- |
| Seeed SenseCAP T1000-E | Seeed Studio store / Mouser / Digi-Key | "T1000-E for Meshtastic" variant; includes internal antenna and 700 mAh battery |
| RAK WisBlock 4631 starter kit | RAKwireless store / Digi-Key / Mouser | RAK4631 + RAK19007 base board + antenna |
| RAK solar enclosure + MPPT | RAKwireless store | "RAK Solar Enclosure" / "WisBlock Solar Unify" |
| Seeed SenseCAP Solar Node P1 | Seeed Studio store | Complete IP66 solar node with L76KB GNSS |
| Heltec Wireless Tracker V1.1 | Heltec store / AliExpress official store | Check band (868/915) and antenna connector |
| 868/915 MHz antennas | RAKwireless, Taoglas, Pulse, local ham suppliers | Half-wave dipole or 5/8 λ whip, ≤3 dBi |
| Low-loss coax (LMR-200/RG-58) | RF suppliers | N/SMA or IPEX adapters as required |
| Solar panels 5–10 W, 6 V/12 V | Local solar suppliers | Match the MPPT input range |
| 18650 / LiFePO4 packs | Local battery suppliers | Protected cells, temperature-rated |
| IP66/67 enclosures | Hammond, Fibox, local electrical suppliers | Add cable glands + vent |

---

## 7. Firmware roles

| Role | Boards | Behaviour |
| --- | --- | --- |
| `tracker` | T1000-E, Heltec Tracker | Announces every 30 min, shares GNSS position every 5 min (channel + gateway fallback), low-power aware, BLE/USB companion. |
| `repeater` | RAK solar, Seeed Solar P1 | Announces every 60 min, floods all floodable frames with TTL−1, stores encrypted envelopes for offline peers (72 h / capacity-bounded), no position sharing. |
| `gateway` | Seeed Solar P1, RAK (optional) | Repeater + telemetry beacons to a status channel; WiFi/Ethernet bridge support is v0.2+ (ESP32 WiFi/BLE hardware present). |
| `client` | any board via companion `ConfigSet node.class=client` | Companion-attached node; app owns messaging. |

Role is set at build time by the board default and can be changed at runtime through the
companion API (`node.class`, reboot recommended). Repeaters work standalone: no app is required
after flashing.

---

## 8. Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| No USB device | Charge-only cable, missing bootloader mode | Try another cable; put board in bootloader/download mode |
| UF2 drive not visible | Board not in bootloader; Linux automount disabled | Double reset; `udisksctl mount -b /dev/sdX1`; check `--list` |
| `esptool` cannot connect | Port busy, wrong board, no download mode | BOOT+RESET; close serial monitors; `--port` explicitly |
| No mesh traffic after flash | No antenna, wrong region, no channel PSK | Attach antenna; set `radio.region`; join the same default channel or add a custom PSK |
| Node never appears for peers | Announce interval not elapsed, TTL/region mismatch | Wait 30–60 min or press the button (3 s) to force an announce |
| Repeater drops traffic | Duty-cycle budget exhausted (EU868/AS923 1 %) | Lower traffic, raise `radio.tx_power` only if legal, check `Stats` over USB |
| GNSS never fixes | Antenna view, wrong baud, VEXT off | Move outdoors; check GNSS enable pin; T1000-E needs 115200 baud (firmware default) |
| Battery reads 0 % | ADC multiplier/table mismatch | Verify board variant; open an issue with `--list-json` output |

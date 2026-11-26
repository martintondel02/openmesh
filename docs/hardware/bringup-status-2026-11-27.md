# Hardware bring-up status — 2026-11-27

**Boards:** Seeed SenseCAP T1000-E (nRF52840 + LR1110) and RAK WisBlock 4631 (nRF52840 + SX1262,
WisBlock base board, UF2 bootloader 0.4.3 / S140 6.1.1).

**Bottom line:** the T1000-E is verified working on the wired companion path. The RAK builds and
its probe firmware runs, but the full OpenMesh application image is not started by the RAK
bootloader yet. The two-node RF test harness is ready; it has **not** been run against two working
OpenMesh nodes yet.

This file is the hand-off point for the next session. Keep it updated or replace it when the
blocker is resolved.

---

## 1. What is verified on hardware

### T1000-E

Using `env:t1000e_usb` (full application, BLE compiled out, no-op scheduler sleep for this
first-article test) and flashing **UF2 with family `0xADA52840`**:

* Boots and prints `OpenMesh 0.7.1 on t1000e, node 5ea2230d7941aa78, radio ok`.
* LR1110 init reports `radio ok`; frequency `869525 kHz`, profile `MEDIUM_FAST`, region `EU868`.
* USB CDC companion protocol:
  * `Ping {nonce: 0x12345678}` → `Pong {nonce: 0x12345678}`.
  * `Hello` → `HelloAck {proto:1, fw_version:"0.7.1", node_id:5ea2230d7941aa78, max_frame:240,
    caps:69, region:"EU868", profile:"MEDIUM_FAST"}`.
  * `Stats` → entries including `tx_frames`, `rx_frames`, `airtime_ms`, `uptime_s`; uptime advances,
    so the engine scheduler is ticking in this build.
* USB enumeration is stable in this BLE-disabled build.

### RAK4631

* `env:rak4631_probe` (USB CDC + LED only) boots and prints `OPENMESH PROBE alive` over USB serial.
* `env:rak4631_usb` (full application, BLE disabled) and `env:rak4631_early` (early stop at the top of
  `setup()`) **do not start**: no USB CDC/serial, and the green LED only shows the bootloader's
  2-blink pattern on reset (not the firmware's 3-blink pre-setup trace or solid setup marker).
* The RAK UF2 bootloader is visible as a `RAK4631` drive after double-tapping reset and accepts
  probe-sized images; it reports:

  ```
  UF2 Bootloader 0.4.3
  Model: WisBlock RAK4631 Board
  Board-ID: WisBlock-RAK4631-Board
  SoftDevice: S140 6.1.1
  ```

* Build fixes already in the tree: RAK uses its board-provided S140 6.1.1 linker script (not the
  T1000-E v7 script), GCC 10.3 toolchain, `framework-cmsis`, nrfutil/bossac packages, RadioLib 7
  SX1262 API fix, and an attempted main-stack increase from 2 KB to 16 KB.

---

## 2. Critical discovery: UF2 family ID

The T1000-E/RAK Adafruit-style bootloaders require UF2 family **`0xADA52840`**. Generating with
`-f 0x100` (the default many tools suggest) produces files the bootloader silently ignores. This
explained repeated "flashed but did not boot" cycles.

Correct conversion:

```bash
UF2=/tmp/pio-core/packages/framework-arduinoadafruitnrf52/tools/uf2conv/uf2conv.py
python3 "$UF2" firmware/.pio/build/<env>/firmware.hex -c -f 0xADA52840 -o image.uf2
```

Then copy `image.uf2` to the `T1000-E` / `RAK4631` drive and unmount. For a serial/DFU path, the
PlatformIO upload (`pio run -e <env> -t upload --upload-port /dev/ttyACMx`) works when the
application (or probe) is running and supports the 1200 bps touch; it does not work from the UF2
bootloader itself.

---

## 3. Current code state (working tree at hand-off)

### Build environments

| Env | Purpose | State |
| --- | --- | --- |
| `t1000e` | Product T1000-E with BLE | Builds; **BLE init with USB attached re-enumerates CDC**; currently not the verified image |
| `t1000e_usb` | Full app, BLE off, no-op sleep | **Verified working on T1000-E** |
| `t1000e_probe` | USB CDC + LED probe | Boots |
| `rak4631_solar` | Product RAK with BLE | Builds |
| `rak4631_usb` | Full app, BLE off | Builds; **does not start on hardware** |
| `rak4631_early` | Early stop at top of setup | Builds; does not start |
| `rak4631_trace` | Adds pre-setup LED trace | Builds; does not start (green = bootloader pattern) |
| `rak4631_setupmark` | Setup-only marker, no constructor | Built; not conclusively flashed/tested |
| `rak4631_probe` | USB CDC + LED probe | Boots |

### Diagnostic changes currently in the working tree

* `firmware/src/hal/hal_platform.cpp`: nRF52 `sleepMs()` is a no-op for first-article testing.
  The direct RTC1/WFE implementation conflicts with FreeRTOS/SoftDevice (`millis()` freezes /
  engine stops ticking); `delay()` was also implicated in a startup regression. **Power/sleep is
  a follow-up, not production-ready.**
* `firmware/src/main.cpp`: init serial markers and short settle delays, `OM_EARLY_STOP` guard,
  `OM_BOOT_TRACE` pre-setup LED constructor.
* `firmware/src/hal/hal_radio.cpp`: RadioLib 7 removed `SX126x::setDcDc()`; the call is omitted (LDO
  default) until a public DC-DC API/power fix is added.
* `firmware/platformio.ini`: RAK toolchain/linker/stack fixes and diagnostic envs listed above.
* `firmware/tools/two_node_test.py`: two-node companion/RF harness (needs `pyserial` + `cbor2`).

Host checks at hand-off: `51` firmware test cases / `3,639` assertions pass; `t1000e` and
`t1000e_usb` build locally.

---

## 4. Two-node test plan (ready to run once RAK starts)

1. Flash T1000-E with `t1000e_usb` and RAK with `rak4631_usb` using the correct UF2 family.
2. Both default to `EU868` / `MEDIUM_FAST`; place them a few metres apart with antennas attached.
3. Identify the ports (order can change; use `udevadm` serial or the HelloAck node id):

   ```bash
   python3 firmware/tools/two_node_test.py /dev/ttyACM0 /dev/ttyACM1
   ```

4. The harness pings both, prints Hello/Stats, injects a golden OMP frame from node A, then checks
   node B's `rx_frames` delta, and repeats B→A. `RF A->B PASS` / `RF B->A PASS` is the acceptance
   criterion.
5. If RX fails, check antennas, region/profile (`Stats.frequency_khz`, HelloAck profile), and
   whether the transmitter's `tx_frames`/`airtime_ms` advanced.

---

## 5. Next-session debugging steps (RAK application startup)

The key open question: **why does the RAK bootloader start the probe image but not the full
application image?** Recommended order:

1. **Image-size/validation test:** build a probe image with a large flash-resident dummy payload
   (e.g. 250–350 KB) and flash it via UF2. If it boots, the bootloader is not rejecting on size;
   if it fails, find the RAK 0.4.3 bootloader's maximum application size and reduce the image or
   update the bootloader.
2. **Minimal-full-app test:** make `setup()` only call `Serial.begin()` + print + halt while keeping
   all globals (`OM_EARLY_STOP` already does this after Serial). If that does not start, the issue
   is before setup: static constructors, linker/RAM map, or bootloader hand-off.
3. **Static-constructor audit:** compare the RAK `.init_array`/`.data`/`.bss` map against the
   T1000-E; test disabling `InternalFS`/storage globals, then Bluefruit globals, one at a time.
4. **Bootloader/SoftDevice check:** confirm the RAK application is linked for S140 6.1.1 and starts
   at `0x26000`; verify with `arm-none-eabi-nm/readelf` and the linker map. The RAK currently uses
   the board-provided `nrf52840_s140_v6.ld`.
5. **Fallback:** use a J-Link/SWD or RAK's nrfjprog path to flash and inspect the fault, rather than
   guessing through UF2 cycles.
6. Once RAK starts: run the two-node test, then revisit BLE/USB coexistence on both boards and the
   sleep/power implementation before calling the hardware support production-ready.

Hardware at hand-off: RAK4631 UF2 drive visible (`/dev/disk/by-label/RAK4631`); RAK running/in
bootloader; T1000-E enumerated in its bootloader (`239a:8029`, `/dev/ttyACM0`). If the ports move,
re-enumerate with `lsusb` / `ls /dev/ttyACM*`.

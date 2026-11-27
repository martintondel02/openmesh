# Hardware bring-up status — 2026-11-27 (updated end of session)

**Boards:** Seeed SenseCAP T1000-E (nRF52840 + LR1110) and RAK WisBlock 4631 (nRF52840 + SX1262,
WisBlock base board, UF2 bootloader 0.4.3 / S140 6.1.1).

**Bottom line:** both boards now boot the full OpenMesh application over USB, and the two-node RF
test **passes in both directions** (`RF A->B PASS`, `RF B->A PASS`). The verified images are the
BLE-compiled-out product builds `env:t1000e_usb` and `env:rak4631_usb`; both flash over USB without
touching a button once an application is running. A real (light) sleep path is in place. BLE
advertising was verified on the RAK4631, but BLE/USB coexistence is **not** solved: the BLE-on
images have unstable USB endpoints on both boards, so they are not yet usable as wired companion
nodes (see §5). Nothing here is release-ready and no tag was created.

This file is the hand-off point for the next session. Keep it updated or replace it when the
remaining issues are resolved.

---

## 1. What is verified on hardware (final images)

Final verified images (all fixes in this tree):

| Board | Env | Image | Flash path used |
| --- | --- | --- | --- |
| T1000-E | `t1000e_usb` | full app, BLE closed, SoftDevice enabled (for flash), light sleep | UF2 family `0xADA52840` copy **and** `pio run -t upload` (1200 bps touch + adafruit-nrfutil) |
| RAK4631 | `rak4631_usb` | full app, BLE closed, SoftDevice enabled (for flash), light sleep | UF2 family `0xADA52840` copy; adafruit-nrfutil also works when the bootloader is in DFU mode |

* Both images print their boot banner and answer the USB companion protocol: `Ping -> Pong`,
  `Hello -> HelloAck`, `Stats` with advancing `uptime_s`.
* Node identities: T1000-E `5ea2230d7941aa78`, RAK4631 `685a7eda39a2ad9c`.
* Both report `EU868` / `MEDIUM_FAST` / `869525 kHz`, TX power `15 dBm` (T1000-E LR1110 cap) and
  `22 dBm` (RAK4631 SX1262 cap).
* `Stats` deltas move in the expected direction under traffic: `tx_frames`, `airtime_ms`,
  `rx_frames`, `relayed`, `dedup_drops` (see §2).

### Two-node RF acceptance (2026-11-27, final run)

Command (`pyserial` + `cbor2` needed, run with access to the serial group):

```bash
python3 firmware/tools/two_node_test.py /dev/ttyACM0 /dev/ttyACM1
```

Observed A = T1000-E, B = RAK4631:

```
/dev/ttyACM0 ping True
/dev/ttyACM0 hello {... caps: 69  region: EU868 profile: MEDIUM_FAST}
/dev/ttyACM0 stats {rx_frames: 1, tx_frames: 2, relayed: 1, airtime_ms: 989, uptime_s: 246, ...}
/dev/ttyACM1 ping True
/dev/ttyACM1 hello {... caps: 583 region: EU868 profile: MEDIUM_FAST}
/dev/ttyACM1 stats {rx_frames: 1, tx_frames: 1, dedup_drops: 1, airtime_ms: 499, uptime_s: 28, ...}
A->B inject 208064a0f5...00000002
A tx delta 3 -> 2  airtime 989
B rx delta 2 -> 1
RF A->B PASS
RF B->A PASS
```

Interpretation: each node had already received the other's boot announce (`rx_frames` 1 at the
first Stats), then the injected golden OMP frame produced one additional `rx_frames` on the peer
in each direction; `tx_frames` and `airtime_ms` advanced on the sender. This is end-to-end RF
proof, not just "radio init returned ok".

---

## 2. Root causes found and fixed in this session

### 2.1 RAK4631 application did not start — bad `--defsym` stack size (fixed)

`env:rak4631_solar` (and everything extending it) carried
`-Wl,--defsym=__StackSize=0x4000`. The framework linker script `nrf52_common.ld` hard-assigns
`__StackSize = 1024 * 2` and computes `__StackLimit = __StackTop - __StackSize`, so the defsym did
not move the stack limit (the ELF keeps `__StackLimit = 0x2003f800`, `__StackTop = 0x20040000`) but
it **did** change the value the Adafruit `Reset_Handler` stack-fill loop uses
(`ldr r0,=__StackLimit; ldr r1,=__StackSize; str 0xADADADAD`). The fill therefore wrote `0x4000`
bytes starting at `0x2003f800`, i.e. **14,336 bytes past the end of nRF52840 RAM**, and the chip
faulted before `setup()` — no USB, no LED markers, bootloader LED pattern only.

Evidence: a probe/trace image byte-identical to HEAD except for the defsym booted immediately
(enumeration as `WisCore RAK4631 Board`), and the pre-session flash at `0x26000` was byte-identical
to a fresh `env:rak4631_trace` build (so the UF2 write was never the problem). The defsym was
removed and replaced with an explanatory comment in `firmware/platformio.ini`. Do not re-add a
stack defsym: the 2 KiB linker stack is for ISRs/SoftDevice only; the application thread stack
comes from the FreeRTOS heap.

### 2.2 Companion replies hung the node — stack overflow in `companionSend()` (fixed)

The Adafruit nRF52 core runs `setup()`/`loop()` on a FreeRTOS task created with
`LOOP_STACK_SZ = 1024` **words = 4 KiB** (`cores/nRF5/main.cpp`). The old companion path put
`uint8_t chunk[2048]`, `uint8_t slip[4096]` and a `CompanionMsg` (1,608 bytes) on that stack — about
7.7 KiB worst case. The first companion reply (Pong) corrupted the task stack; the node stopped
answering and its USB endpoint stopped being serviced (reproduced on the T1000-E: one successful
Ping, then a hung node). The buffers now live in file-scope statics in `firmware/src/main.cpp`
(the firmware main loop is single-threaded), which fixed the hang and is covered by the final
two-node run above.

### 2.3 No RF reception in either direction — two independent bugs (fixed)

1. **RX polling used the wrong RadioLib 7 API.** `Radio::receive()` polled
   `radio->available()`, which is the *direct-receive* ring-buffer length and is always 0 in LoRa
   packet mode. No packet was ever read, on either chip. It now polls the radio IRQ flags
   (`getIrqFlags()` / `getPacketLength()` / `readData()` / `clearIrqFlags()`), drops and re-arms on
   CRC/header errors, and uses a static discard buffer for oversized frames.
2. **T1000-E LR1110 antenna switch was never configured.** RadioLib's LR11x0 `begin()` does not
   install the RF switch table; on the T1000-E the sub-GHz switch is driven by DIO5-DIO8. Without
   it, `radio.begin()` still succeeds but TX/RX is routed into a disabled RF path. The
   board-specific table from the Meshtastic T1000-E variant was added to
   `firmware/src/hal/hal_radio.cpp` (verified byte-identical to upstream
   `variants/nrf52840/tracker-t1000-e/rfswitch.h`).

The two-node test before these fixes transmitted on both nodes but received 0 frames in both
directions; after the fixes it passes both directions consistently (three consecutive runs).

### 2.4 Sleep / power (partially implemented)

The nRF52 `sleepMs()` was a no-op busy loop. It now calls `delay()`, which in this core flushes the
TinyUSB CDC and then does `vTaskDelay()`; with `configUSE_TICKLESS_IDLE = 1` in the Adafruit
FreeRTOS port this lets the CPU enter the tickless-idle low-power state instead of spinning. The
scheduler loop still polls radio/companion every 4 ms (repeater role) or 20 ms (tracker role).
Verified indirectly on hardware (engine uptime keeps advancing and the companion link stays
responsive in the final two-node run); **current consumption was not measured**, and deep sleep
(radio off between windows) is still not implemented — see §6.

### 2.5 UF2 family (carried over from the previous hand-off)

Both Adafruit-style bootloaders silently ignore UF2 files with the wrong family. Use:

```bash
UF2=/tmp/pio-core/packages/framework-arduinoadafruitnrf52/tools/uf2conv/uf2conv.py
python3 "$UF2" firmware/.pio/build/<env>/firmware.hex -c -f 0xADA52840 -o image.uf2
```

### 2.6 First-boot flash writes — SoftDevice now enabled in the BLE-closed images (fixed)

Adafruit's `InternalFS` routes flash writes through `sd_flash_*` SVCs, which are only valid while
the SoftDevice is enabled. The previous BLE-compiled-out images left the SoftDevice off, so a
first-boot identity/store write on a blank board could fault (the linked `SVC_Handler` is
FreeRTOS's). The verified images now define `OM_ENABLE_SOFTDEVICE=1`: `hal::beginSoftDevice()`
enables the SoftDevice (without starting BLE) at the top of `setup()`, before `g_storage.begin()`
and the identity load, and `randomBytes()` switches to the SoftDevice RNG application API when the
SoftDevice is running instead of touching the SoftDevice-owned `NRF_RNG` peripheral.

Verified on hardware 2026-11-27:

* `env:t1000e_fswrite` (InternalFS write probe, SoftDevice enabled, BLE closed) wrote and
  read back a file: `FSPROBE stage=5 msg=read=19 data=fsprobe-0123456789`.
* Both verified images enumerate USB once, take the TinyUSB ↔ SoftDevice hand-off at boot (one
  extra USB re-enumeration during boot is expected), then stay enumerated: 0 USB disconnects after
  boot while running the two-node test, and the companion link + RF remain healthy.

A fresh board (erased identity/store) was not physically available to test the full first-boot
path end to end, but the write mechanism it depends on is now exercised and working.

---

## 3. Flashing notes (both boards, no buttons once an app runs)

* **T1000-E**: double-clicking the side button exposes the `T1000-E` UF2 drive (~2 min window);
  `pio run -e t1000e_usb -t upload --upload-port /dev/ttyACM0` also works from a running
  application (1200 bps touch → bootloader → adafruit-nrfutil). The bootloader does *not* speak
  adafruit-nrfutil while it sits in UF2/MSD mode, only in DFU mode.
* **RAK4631**: double-tapping reset exposes the `RAK4631` drive. With a running application,
  `pio run -e rak4631_usb -t upload --upload-port /dev/ttyACM1` works (touch + nrfutil). The
  bootloader also accepts adafruit-nrfutil directly in its DFU-mode PID (`239a:002a`). Note the RAK
  application takes ~15–20 s to enumerate USB after boot (`device descriptor read/64` timeout then
  success), so wait before concluding it failed.
* USB product strings: application = `T1000-E-BOOT` (T1000-E board JSON) / `WisCore RAK4631 Board`;
  bootloader = `T1000-E-BOOT` (Seeed PID `2886:0057` UF2 mode) / `WisBlock RAK4631`.

---

## 4. BLE / USB coexistence — OPEN (not solved)

The BLE-first init order was introduced because Adafruit's `InternalFS` uses SoftDevice flash
SVCs (`sd_flash_*`) that are only valid once the SoftDevice is enabled; BLE-on builds now bring
the SoftDevice up at the top of `setup()`. This did **not** fix USB coexistence:

* **RAK4631, `env:rak4631_solar` (BLE on):** BLE advertising *works* — a host BlueZ scan saw
  `RAK WisBlock 4631 solar` — but the USB CDC endpoint does not enumerate reliably (repeated
  re-enumeration / descriptor read timeouts), so the wired companion link is unusable. The
  Adafruit TinyUSB ↔ SoftDevice USB-event hand-off (`usb_softdevice_pre_enable/post_enable`,
  `usb_softdevice_pre_enable()` disables nrfx_power USB event handling) is the prime suspect on
  this older S140 6.1.1 bootloader; the framework/core versions were also checked and the
  per-board SoftDevice API headers do match (RAK 6.1.1, T1000-E 7.3.0).
* **T1000-E, `env:t1000e` (BLE on):** the USB companion is not stable; this unit reset every
  ~2.5 s with `RESETREAS = 0x00000000` (power-on-reset semantics, not a software/watchdog cause),
  which points at supply/brownout on this first-article unit (BLE + LR1110 TX peaks) rather than a
  firmware crash. Needs a charged battery / bench supply and a current measurement to confirm.
* **Resolved in the verified images:** the flash-write hazard that came from running BLE-off images
  with the SoftDevice disabled is fixed by `OM_ENABLE_SOFTDEVICE=1` (see §2.6). What remains open is
  only the BLE-on USB instability above.

Until §4 is closed, the **verified** companion/RF images are `t1000e_usb` and `rak4631_usb`.

---

## 5. Build environments (diagnostics kept in-tree)

| Env | Purpose | State |
| --- | --- | --- |
| `t1000e` | product T1000-E with BLE | builds; BLE advertising path exists; USB instability open (§4) |
| `t1000e_usb` | full app, BLE closed, SoftDevice on, light sleep | **verified: companion + RF, two-node PASS** |
| `t1000e_probe` | USB CDC + LED probe | boots |
| `t1000e_fswrite` | InternalFS write probe (SoftDevice on, BLE closed) | **verified: writes and reads back a file** |
| `t1000e_bigprobe` | probe + 300 KiB flash payload (bootloader size test) | builds; superseded by the §2.1 root cause |
| `rak4631_solar` | product RAK with BLE | builds; BLE advertises, USB unusable (§4) |
| `rak4631_usb` | full app, BLE closed, SoftDevice on, light sleep | **verified: companion + RF, two-node PASS** |
| `rak4631_early` / `rak4631_trace` / `rak4631_setupmark` | startup diagnostics | obsolete after §2.1 but kept |
| `rak4631_probe` | USB CDC + LED probe | boots |
| `rak4631_bigprobe` | probe + 300 KiB flash payload | builds; not needed after §2.1 |
| `native`, `native_vectors` | host unit/integration tests + golden-vector replay | pass |

Host checks after this session's changes: `51` firmware test cases / `3,639` assertions pass,
including golden-vector replay (run with `firmware/ci/host_tests.sh`; the repo docs checkers are
listed in §6).

---

## 6. Remaining work, in order

1. **BLE/USB coexistence (§4).** Decide the target: (a) fix Adafruit TinyUSB + SoftDevice USB
   hand-off on S140 6.1.1 (RAK bootloader 0.4.3) and the power/advertising behaviour on T1000-E; or
   (b) upgrade the RAK bootloader/SoftDevice to a newer S140 and re-test; or (c) ship the verified
   BLE-closed images for wired companion use and treat BLE as a separate transport build. Measure
   current with a charged battery/bench supply. Hint from this session: enabling the SoftDevice
   with BLE closed is stable on both boards (USB stays up after the one-time boot hand-off), so
   the T1000-E BLE-on resets are likely drawn by BLE advertising/radio activity rather than by the
   hand-off itself; starting BLE advertising *after* boot (SoftDevice already enabled) is worth
   testing as the next coexistence experiment.
2. **Deep sleep / power budget.** Keep the receiver on continuously today; implement radio
   RX windows and measure current. The scheduler and `PowerPolicy` already emit low-power hints.
   Battery/ADC calibration and GNSS date/time remain as before (§9 in `firmware/README.md`).
3. **Other boards** (Seeed Solar Node P1, Heltec Wireless Tracker) still need first-article
   build/boot checks.
4. Android BLE companion handshake and the BLE NUS companion path on nRF52.

Do not release or tag until the BLE/USB question is settled and the user has reviewed the
bring-up diff.

---

## 7. Repro / evidence

Environment used:

```bash
export PATH=/tmp/pioenv/bin:$PATH
export PLATFORMIO_CORE_DIR=/tmp/pio-core
pio run -d firmware -e t1000e_usb
pio run -d firmware -e rak4631_usb
pio run -d firmware -e t1000e_fswrite   # InternalFS write probe (diagnostic)
pio run -d firmware -e <env> -t upload --upload-port /dev/ttyACMx   # from a running app
python3 firmware/tools/two_node_test.py /dev/ttyACM0 /dev/ttyACM1   # needs pyserial + cbor2
cd firmware && ./ci/host_tests.sh
python3 scripts/check_docs.py && python3 scripts/check_help.py && python3 scripts/check_openapi.py
```

Serial access needs the `dialout` group (`sg dialout -c '...'`). The two-node harness exits 0 only
when both RF directions pass. Evidence logs from the session (serial captures, RF pass log) are
kept outside the repo under `/tmp/bringup-evidence/`; they are not committed.

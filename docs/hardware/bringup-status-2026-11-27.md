# Hardware bring-up status — 2026-11-27 (updated end of session)

**Boards:** Seeed SenseCAP T1000-E (nRF52840 + LR1110) and RAK WisBlock 4631 (nRF52840 + SX1262,
WisBlock base board, UF2 bootloader 0.4.3 / S140 6.1.1).

**Bottom line:** both boards run the full OpenMesh **product BLE images** (`env:t1000e`,
`env:rak4631_solar`) with **USB and BLE coexisting**: stable USB companion (`Ping/Hello/Stats`),
BLE advertising visible from a host scan, and the two-node RF test **passing in both directions**
(`RF A->B PASS`, `RF B->A PASS`). The coexistence fix is ordering: the SoftDevice is enabled during
`setup()` but BLE advertising starts ~5 s after boot (§4). A real (light) sleep path is in place,
and the blank-board first boot (no identity/store) was verified end-to-end, including identity
creation through the SoftDevice flash path (§2.6). Both boards flash over USB without touching a
button once an application is running. Nothing here is release-ready, no tag was created, and deep
sleep / sleep current remain unmeasured.

This file is the hand-off point for the next session. Keep it updated or replace it when the
remaining issues are resolved.

---

## 1. What is verified on hardware (final images)

Final verified images (all fixes in this tree):

| Board | Env | Image | Flash path used |
| --- | --- | --- | --- |
| T1000-E | `t1000e` | **product image**: full app, SoftDevice on, BLE advertising ~5 s after boot, light sleep | UF2 family `0xADA52840` copy **and** `pio run -t upload` (1200 bps touch + adafruit-nrfutil) |
| RAK4631 | `rak4631_solar` | **product image**: full app, SoftDevice on, BLE advertising ~5 s after boot, light sleep | UF2 family `0xADA52840` copy; adafruit-nrfutil also works when the bootloader is in DFU mode |
| both | `t1000e_usb` / `rak4631_usb` | BLE-closed variants (SoftDevice on for flash, no advertising) — useful for wired-only bring-up | same as above |

* Both images print their boot banner and answer the USB companion protocol: `Ping -> Pong`,
  `Hello -> HelloAck`, `Stats` with advancing `uptime_s`.
* Node identities: T1000-E `7fe558cfc94d9c60` (regenerated when the InternalFS region was erased for
  the blank-board first-boot test in §2.6), RAK4631 `685a7eda39a2ad9c`.
* Both report `EU868` / `MEDIUM_FAST` / `869525 kHz`, TX power `15 dBm` (T1000-E LR1110 cap) and
  `22 dBm` (RAK4631 SX1262 cap).
* `Stats` deltas move in the expected direction under traffic: `tx_frames`, `airtime_ms`,
  `rx_frames`, `relayed`, `dedup_drops` (see §2).
* BLE + USB coexistence (product images): host BlueZ scans see `Seeed SenseCAP T1000-E` and
  `RAK WisBlock 4631 solar`; 0 USB disconnects after boot on both boards while advertising, and the
  USB companion `caps` include the BLE capability (101 / 615); two-node RF still passes with the
  BLE images loaded (§4).

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

The full blank-board path was then exercised end to end on the T1000-E (2026-11-27):

1. `env:t1000e_fsformat` (diagnostic InternalFS format probe) erased and reformatted the InternalFS
   region: `FSPROBE stage=3 msg=formatted`.
2. Flashing `env:t1000e` booted normally, found no seed, generated a **new node identity**
   (`7fe558cfc94d9c60`, replacing `5ea2230d7941aa78`), saved it through the SoftDevice flash path,
   and answered the companion link.
3. A device-initiated reset (companion `Reset` command) rebooted the node; the identity persisted
   and the companion link came back. The reset handler now uses `sd_nvic_SystemReset()` while the
   SoftDevice is enabled (Nordic's supported path) instead of a plain `NVIC_SystemReset()`; device-
   initiated resets can renumber the `/dev/ttyACM*` node, so ports must be re-detected after one.
4. The two-node RF test passed again with the new identity.

Also verified with the same images: `env:t1000e_fswrite` wrote and read back a file
(`FSPROBE stage=5 msg=read=19 data=fsprobe-0123456789`).

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

## 4. BLE / USB coexistence — SOLVED by advertising after boot (verified both boards)

**Symptom (earlier in the session):** BLE-on images were unusable as wired companion nodes. The
T1000-E power-on-reset every ~2.5 s (`RESETREAS = 0x00000000`, i.e. supply/power-on semantics, not
a software/watchdog cause), and the RAK4631's USB CDC re-enumerated or failed descriptor reads while
its BLE advertising worked.

**Root cause / fix:** starting BLE advertising *during* `setup()` overlaps the TinyUSB ↔ SoftDevice
hand-off and the radio bring-up (LR1110/SX1262 init and the first announce transmission), and that
combination destabilises the USB/power path on both first articles. The firmware now:

* enables the SoftDevice at the top of `setup()` (`hal::beginSoftDevice()`, no BLE yet, BLE
  transport configured but not advertising), and
* starts BLE advertising from `loop()` once ~5 s have passed (`g_ble.begin(name)` on the first loop
  iteration after that threshold).

The SoftDevice-first ordering is also required for `InternalFS` flash writes (`sd_flash_*` SVCs),
see §2.6. The BLE-first/`OM_ENABLE_SOFTDEVICE` architecture from earlier in the session is retained.

**Verified on hardware 2026-11-27 with the product images:**

| Check | T1000-E (`t1000e`) | RAK4631 (`rak4631_solar`) |
| --- | --- | --- |
| USB disconnects after boot | 0 | 0 |
| USB companion Ping/Hello/Stats | PASS (`caps` 101, includes BLE) | PASS (`caps` 615, includes BLE) |
| BLE advertising seen by host BlueZ | `Seeed SenseCAP T1000-E` | `RAK WisBlock 4631 solar` |
| Two-node RF test | **PASS both directions** | **PASS both directions** |

**Consequences for the earlier bootloader question:** no RAK bootloader/SoftDevice upgrade is
needed for coexistence, and none was attempted — **no SWD/J-Link probe was connected**, so flashing
a bootloader/SD would have been unrecoverable-if-wrong; the older S140 6.1.1 works with the
advertising-after-boot ordering. What is still not verified is a BLE **companion** session
(NUS write/read from a phone/host) — only advertising is proven; the USB companion is proven.

## 5. Build environments (diagnostics kept in-tree)

| Env | Purpose | State |
| --- | --- | --- |
| `t1000e` | product T1000-E with BLE | **verified: BLE + USB coexist, advertising, companion, RF two-node PASS** |
| `t1000e_usb` | full app, BLE closed, SoftDevice on, light sleep | **verified: companion + RF, two-node PASS** |
| `t1000e_probe` | USB CDC + LED probe | boots |
| `t1000e_fswrite` | InternalFS write probe (SoftDevice on, BLE closed) | **verified: writes and reads back a file** |
| `t1000e_fsformat` | InternalFS erase/reformat probe (blank-board test) | **verified: erased identity/store; blank-board first boot then verified** |
| `t1000e_bigprobe` | probe + 300 KiB flash payload (bootloader size test) | builds; superseded by the §2.1 root cause |
| `rak4631_solar` | product RAK with BLE | **verified: BLE + USB coexist, advertising, companion, RF two-node PASS** |
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

1. **BLE companion transport (NUS).** Advertising is verified on both boards and the USB companion
   is verified end to end, but no BLE **session** has been exercised yet: connect to the NUS, write a
   Ping, receive a Pong. Then run the two-node harness over BLE on one side.
2. **Deep sleep / power budget.** The receiver stays on continuously today; implement radio RX
   windows and measure current (light sleep via `delay()` + FreeRTOS tickless idle is in place).
   Battery/ADC calibration and GNSS date/time remain as before (§9 in `firmware/README.md`).
3. **Other boards** (Seeed Solar Node P1, Heltec Wireless Tracker) still need first-article
   build/boot checks.
4. **Optional:** the coexistence fix should be stress-tested across long runs and repeated
   advertising/connection cycles; also consider whether a shorter post-boot delay than 5 s is still
   stable.

Do not release or tag until the remaining power/current work is done and the user has reviewed the
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

// SPDX-License-Identifier: AGPL-3.0-or-later
// Hardware bring-up probe: minimal USB CDC + LED, no radio/BLE/storage. Used by
// `pio run -e t1000e_probe` / `pio run -e rak4631_probe` to verify the nRF52
// bootloader/SoftDevice/USB path on a first article before flashing the full firmware.
#include <Arduino.h>

#if defined(OM_FS_WRITE_TEST)
#include <cstdio>
#include <InternalFileSystem.h>
#if defined(OM_ENABLE_SOFTDEVICE) && (OM_ENABLE_SOFTDEVICE)
#include <bluefruit.h>
#endif
#endif

#ifndef LED_BUILTIN
#define LED_BUILTIN 24
#endif

// Optional flash-resident payload for bootloader image-size validation. Build with
// -DOM_PROBE_PAYLOAD_KB=<n> to place an n KiB const array in .rodata (FLASH) and touch it at
// runtime. This tests whether a bootloader starts large application images without changing any
// other probe behaviour.
#ifndef OM_PROBE_PAYLOAD_KB
#define OM_PROBE_PAYLOAD_KB 0
#endif

#if OM_PROBE_PAYLOAD_KB > 0
extern "C" const unsigned char om_probe_payload[OM_PROBE_PAYLOAD_KB * 1024]
    __attribute__((used, section(".rodata.probe_payload"))) = {0x5a};
static volatile unsigned g_payload_sink;

// Reading through a runtime-derived index is deliberate: if the payload is only touched with
// constant indices the compiler folds them and the linker's --gc-sections drops the whole array,
// which silently turns this size test into a normal probe build.
static void touchProbePayload() {
    const unsigned size = OM_PROBE_PAYLOAD_KB * 1024;
    const unsigned idx = (unsigned)(millis() & 1u);
    g_payload_sink =
        (unsigned)om_probe_payload[idx] + (unsigned)om_probe_payload[size - 1u - idx];
}
#endif

#if defined(OM_FS_WRITE_TEST)
// InternalFS write probe: proves whether the Adafruit InternalFS flash path (sd_flash_* SVCs) works
// in this image. It writes a new file only; with the SoftDevice enabled this is the same path the
// first-boot identity/store writes use. Result is printed repeatedly in loop() so a host attaching
// after boot still sees it.
static volatile int g_fs_stage = 0;
static char g_fs_msg[80] = "not-run";

static void runFsWriteTest() {
    g_fs_stage = 1;
#if defined(OM_ENABLE_SOFTDEVICE) && (OM_ENABLE_SOFTDEVICE)
    if (!Bluefruit.begin()) {
        std::snprintf(g_fs_msg, sizeof(g_fs_msg), "softdevice-begin-failed");
        return;
    }
    std::snprintf(g_fs_msg, sizeof(g_fs_msg), "softdevice-ok");
#else
    std::snprintf(g_fs_msg, sizeof(g_fs_msg), "softdevice-not-enabled");
#endif
    g_fs_stage = 2;
    if (!InternalFS.begin()) {
        std::snprintf(g_fs_msg, sizeof(g_fs_msg), "fs-begin-failed");
        return;
    }
#if defined(OM_FS_FORMAT)
    // Factory-reset probe: erase + reformat the InternalFS region so the next application boot has
    // no identity/store files and exercises the first-boot write path for real.
    g_fs_stage = 3;
    InternalFS.format();
    std::snprintf(g_fs_msg, sizeof(g_fs_msg), "formatted");
    return;
#else
    g_fs_stage = 3;
    Adafruit_LittleFS_Namespace::File file(InternalFS);
    if (!file.open("om_fsprobe.bin", Adafruit_LittleFS_Namespace::FILE_O_WRITE)) {
        std::snprintf(g_fs_msg, sizeof(g_fs_msg), "open-failed");
        return;
    }
    static const char payload[] = "fsprobe-0123456789";
    const size_t written = file.write(payload, sizeof(payload));
    file.close();
    g_fs_stage = 4;
    std::snprintf(g_fs_msg, sizeof(g_fs_msg), "wrote=%u", (unsigned)written);

    Adafruit_LittleFS_Namespace::File readback(InternalFS);
    if (readback.open("om_fsprobe.bin", Adafruit_LittleFS_Namespace::FILE_O_READ)) {
        char buf[32] = {0};
        const int n = readback.read(buf, sizeof(buf) - 1);
        readback.close();
        g_fs_stage = 5;
        std::snprintf(g_fs_msg, sizeof(g_fs_msg), "read=%d data=%s", n, buf);
    }
#endif  // OM_FS_FORMAT
}
#endif  // OM_FS_WRITE_TEST

void setup() {
    pinMode(LED_BUILTIN, OUTPUT);
    digitalWrite(LED_BUILTIN, HIGH);
    Serial.begin(115200);
    delay(300);
    Serial.println();
#if defined(OM_BOARD_RAK4631_SOLAR)
    Serial.println("OPENMESH PROBE rak4631: USB CDC alive");
#else
    Serial.println("OPENMESH PROBE t1000e: USB CDC alive");
#endif
#if defined(OM_FS_WRITE_TEST)
    runFsWriteTest();
    Serial.printf("FSPROBE stage=%d msg=%s\n", (int)g_fs_stage, g_fs_msg);
    Serial.flush();
#endif
#if OM_PROBE_PAYLOAD_KB > 0
    touchProbePayload();
    Serial.printf("OPENMESH PROBE payload=%u KiB first=%02x last=%02x sum=%u\n",
                  (unsigned)OM_PROBE_PAYLOAD_KB, (unsigned)om_probe_payload[0],
                  (unsigned)om_probe_payload[OM_PROBE_PAYLOAD_KB * 1024 - 1],
                  (unsigned)g_payload_sink);
#endif
}

void loop() {
    static uint32_t n = 0;
    digitalWrite(LED_BUILTIN, (n % 2) ? HIGH : LOW);
    Serial.printf("OPENMESH PROBE alive %lu\n", (unsigned long)n++);
#if defined(OM_FS_WRITE_TEST)
    if (n < 60) {
        Serial.printf("FSPROBE stage=%d msg=%s\n", (int)g_fs_stage, g_fs_msg);
        Serial.flush();
    }
#endif
    delay(1000);
}

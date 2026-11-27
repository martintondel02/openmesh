// SPDX-License-Identifier: AGPL-3.0-or-later
// Hardware bring-up probe: minimal USB CDC + LED, no radio/BLE/storage. Used by
// `pio run -e t1000e_probe` / `pio run -e rak4631_probe` to verify the nRF52
// bootloader/SoftDevice/USB path on a first article before flashing the full firmware.
#include <Arduino.h>

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
    delay(1000);
}

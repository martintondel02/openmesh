// SPDX-License-Identifier: AGPL-3.0-or-later
// Hardware bring-up probe: minimal USB CDC + LED, no radio/BLE/storage. Used by
// `pio run -e t1000e_probe` to verify the nRF52 bootloader/SoftDevice/USB path on a first
// article before flashing the full firmware.
#include <Arduino.h>

#ifndef LED_BUILTIN
#define LED_BUILTIN 24
#endif

void setup() {
    pinMode(LED_BUILTIN, OUTPUT);
    digitalWrite(LED_BUILTIN, HIGH);
    Serial.begin(115200);
    delay(300);
    Serial.println();
    Serial.println("OPENMESH PROBE t1000e: USB CDC alive");
}

void loop() {
    static uint32_t n = 0;
    digitalWrite(LED_BUILTIN, (n % 2) ? HIGH : LOW);
    Serial.printf("OPENMESH PROBE alive %lu\n", (unsigned long)n++);
    delay(1000);
}

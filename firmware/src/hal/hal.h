// SPDX-License-Identifier: AGPL-3.0-or-later
// Board HAL façade: radio, flash storage, companion transports, time, RNG and sleep.
// The protocol core in lib/openmesh is Arduino-free; everything Arduino/RadioLib specific
// lives behind this header.
#ifndef OPENMESH_HAL_H
#define OPENMESH_HAL_H

#include <cstddef>
#include <cstdint>

#include "board.h"
#include "openmesh/om_engine.h"
#include "openmesh/om_region.h"

namespace om {
namespace hal {

// Monotonic milliseconds since boot.
uint64_t nowMs();
// Fills `out` from the hardware CSPRNG (nRF52 NRF_RNG, ESP32 esp_random, fallback XorShift).
void randomBytes(void* ctx, uint8_t* out, size_t len);
// Busy/light sleep for `ms`; deep-sleep friendly on both MCUs.
void sleepMs(uint32_t ms);

// Configures status LED / button.
void initUx(const board::Board& board);
void setLed(bool on);
bool buttonPressed();

// GNSS UART + power control. Returns bytes read into `buf` (0 when none).
bool gpsBegin(const board::Board& board);
int gpsRead(uint8_t* buf, size_t cap);
void gpsSetEnabled(bool enabled);

// Battery / solar / charger measurement using the board divider table.
om::BatteryReading readBattery(const board::Board& board);

// Wall clock (Unix ms) when the platform knows it (GPS time once fixed, else 0).
uint64_t wallTimeMs();

}  // namespace hal
}  // namespace om

#endif  // OPENMESH_HAL_H

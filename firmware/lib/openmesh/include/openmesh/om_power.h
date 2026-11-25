// SPDX-License-Identifier: AGPL-3.0-or-later
// Battery / solar / charger measurements and power policy.
//
// Board HALs report raw millivolts (already multiplied by the board's divider) and an ADC
// status; this module converts to a percentage and decides low-power behaviour. OCV curves
// are taken from the reference board definitions (see firmware/boards/ and NOTICE).
#ifndef OPENMESH_OM_POWER_H
#define OPENMESH_OM_POWER_H

#include <cstddef>
#include <cstdint>

namespace om {

struct BatteryReading {
    uint16_t battery_mv = 0;
    uint8_t percent = 0;
    uint16_t solar_mv = 0;
    bool charging = false;
    bool usb_present = false;
    bool valid = false;
};

// Piecewise-linear interpolation over a millivolt/percent table. Tables are ordered from
// full (100 %) to empty (0 %).
uint8_t batteryPercentFromMv(uint16_t mv, const uint16_t* ocv_mv, size_t count);

// Reference OCV curves (millivolts at 100, 90, ... 0 percent).
extern const uint16_t kOcvT1000E[];        // Seeed T1000-E, 700 mAh LiPo
extern const uint16_t kOcvSeeedSolar[];   // Seeed SenseCAP Solar Node P1 pack
extern const uint16_t kOcvGenericLiIon[]; // RAK4631 / Heltec (linear approximation)

constexpr size_t kOcvT1000ECount = 11;
constexpr size_t kOcvSeeedSolarCount = 11;
constexpr size_t kOcvGenericCount = 11;

// Power policy thresholds, in millivolts.
constexpr uint16_t kLowBatteryMv = 3400;
constexpr uint16_t kCriticalBatteryMv = 3200;
constexpr uint16_t kSolarChargingMvDelta = 150;  // solar_mv - battery_mv when charging

struct PowerPolicy {
    bool low_power = false;       // reduce announce cadence, stop position sharing
    bool critical = false;        // radio TX disabled except announces, prepare shutdown
    bool position_sharing = true;
    // Trackers sleep between fixes; repeaters stay in RX.
    uint32_t tracker_sleep_ms = 300000;
};

// Derives the policy from the latest reading. `solar` is only meaningful on solar boards.
PowerPolicy evaluatePower(const BatteryReading& reading, bool has_solar, bool is_repeater);

// Simple moving average to de-noise ADC readings (integer, no floats).
class MovingAverage {
public:
    explicit MovingAverage(uint16_t initial = 0) : value_(initial) {}
    uint16_t update(uint16_t sample) {
        // Exponential moving average with 1/8 weight; integer only.
        value_ = (uint16_t)((int32_t)value_ + ((int32_t)sample - (int32_t)value_) / 8);
        return value_;
    }
    uint16_t value() const { return value_; }

private:
    uint16_t value_;
};

}  // namespace om

#endif  // OPENMESH_OM_POWER_H

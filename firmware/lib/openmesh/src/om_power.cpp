// SPDX-License-Identifier: AGPL-3.0-or-later
#include "openmesh/om_power.h"

namespace om {

const uint16_t kOcvT1000E[kOcvT1000ECount] = {4190, 4042, 3957, 3885, 3820, 3776,
                                             3746, 3725, 3696, 3644, 3100};
const uint16_t kOcvSeeedSolar[kOcvSeeedSolarCount] = {4200, 3986, 3922, 3812, 3734, 3645,
                                                      3527, 3420, 3281, 3087, 2786};
const uint16_t kOcvGenericLiIon[kOcvGenericCount] = {4200, 4060, 3980, 3920, 3870, 3820,
                                                     3780, 3730, 3670, 3550, 3200};

uint8_t batteryPercentFromMv(uint16_t mv, const uint16_t* ocv_mv, size_t count) {
    if (ocv_mv == nullptr || count < 2) return 0;
    if (mv >= ocv_mv[0]) return 100;
    if (mv <= ocv_mv[count - 1]) return 0;
    // Table is ordered full -> empty; interpolate between adjacent entries.
    for (size_t i = 0; i + 1 < count; ++i) {
        const uint16_t hi = ocv_mv[i];
        const uint16_t lo = ocv_mv[i + 1];
        if (mv <= hi && mv >= lo) {
            const uint32_t span = (uint32_t)(hi - lo);
            const uint32_t offset = (uint32_t)(hi - mv);
            // Percent at index i is 100 - i*(100/(count-1)).
            const uint32_t step_num = 100;
            const uint32_t step_den = (uint32_t)(count - 1);
            const uint32_t base_pct = 100 - (uint32_t)((i * step_num) / step_den);
            const uint32_t delta = span == 0 ? 0 : (uint32_t)((offset * step_num) / (span * step_den));
            const uint32_t pct = base_pct > delta ? base_pct - delta : 0;
            return (uint8_t)(pct > 100 ? 100 : pct);
        }
    }
    return 0;
}

PowerPolicy evaluatePower(const BatteryReading& reading, bool has_solar, bool is_repeater) {
    PowerPolicy p;
    if (!reading.valid) return p;
    p.low_power = reading.battery_mv < kLowBatteryMv;
    p.critical = reading.battery_mv < kCriticalBatteryMv;
    if (has_solar && reading.solar_mv > 0) {
        // Recovering: treat solar input as an escape hatch from low-power mode.
        if (reading.solar_mv > reading.battery_mv + kSolarChargingMvDelta) p.low_power = false;
    }
    if (is_repeater) {
        p.position_sharing = false;
    } else if (p.low_power) {
        p.position_sharing = false;
        p.tracker_sleep_ms = p.critical ? 1800000u : 900000u;
    }
    return p;
}

}  // namespace om

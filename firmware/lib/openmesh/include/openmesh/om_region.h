// SPDX-License-Identifier: AGPL-3.0-or-later
// Region profiles, LoRa modem presets, airtime estimation and the duty-cycle token bucket.
// Mirrors crates/openmesh-core/src/util.rs (PROTOCOL.md §11, §12).
#ifndef OPENMESH_OM_REGION_H
#define OPENMESH_OM_REGION_H

#include <cstddef>
#include <cstdint>

namespace om {

struct LoraProfile {
    const char* name;
    uint8_t sf;
    uint16_t bw_khz;
    uint8_t coding_rate;  // 4/5 = 5 ... 4/8 = 8
    uint16_t preamble_symbols;
    bool explicit_header;
    bool crc;
};

extern const LoraProfile kLongSlow;
extern const LoraProfile kLongFast;
extern const LoraProfile kMediumFast;
extern const LoraProfile kShortFast;
extern const LoraProfile kShortTurbo;

struct Region {
    const char* name;
    uint32_t freq_khz;
    int8_t max_power_dbm;   // EIRP where applicable
    int8_t duty_cycle_pct;  // -1 = no duty-cycle limit (fair use only)
    uint16_t bw_khz;
};

extern const Region kRegionEu868;
extern const Region kRegionUs915;
extern const Region kRegionAu915;
extern const Region kRegionIn865;
extern const Region kRegionKr920;
extern const Region kRegionAs923;

const LoraProfile* profileByName(const char* name);
const Region* regionByName(const char* name);
// Build-time default used when the user has not configured a region.
const Region& defaultRegion();

// On-air time in milliseconds, Semtech AN1200.13 formula (same as util::lora_airtime_ms).
uint32_t loraAirtimeMs(const LoraProfile& profile, size_t payload_len);

// Maximum payload bytes for a preset/region combination, and chunk sizing helpers.
size_t maxAttachmentBytes(const LoraProfile& profile, const Region& region);

// Token bucket limiting transmitted airtime to the region duty cycle. Stored in microsecond
// token units so the math stays integral. Regions without a duty-cycle limit yield an
// always-allowing budget.
class AirtimeBudget {
public:
    void configure(const Region& region, bool enabled = true);
    void reset(uint64_t now_ms);
    // Attempts to spend `airtime_ms`; returns false when the budget would be exceeded and the
    // frame must be dropped/queued. `enabled=false` budgets always allow.
    bool tryConsume(uint32_t airtime_ms, uint64_t now_ms);
    uint32_t remainingMs(uint64_t now_ms);
    uint32_t capacityMs() const { return capacity_ms_; }
    bool enabled() const { return enabled_; }

private:
    void refill(uint64_t now_ms);

    int64_t tokens_us_ = 0;
    uint32_t capacity_ms_ = 0;
    uint32_t refill_ms_per_sec_ = 0;  // airtime ms allowed per wall-clock second
    uint64_t last_ms_ = 0;
    bool enabled_ = false;
    bool initialized_ = false;
};

// Origination rate limiter: at most MAX_ORIGINATE_PER_MIN frames per minute (ACKs excepted).
constexpr uint32_t kMaxOriginatePerMin = 60;

class OriginateLimiter {
public:
    void reset(uint64_t now_ms) {
        window_start_ms_ = now_ms;
        count_ = 0;
    }
    bool allow(uint64_t now_ms, bool is_ack_or_probe);

private:
    uint64_t window_start_ms_ = 0;
    uint32_t count_ = 0;
    bool initialized_ = false;
};

}  // namespace om

#endif  // OPENMESH_OM_REGION_H

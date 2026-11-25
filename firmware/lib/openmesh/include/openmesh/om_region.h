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
extern const Region kRegionEu433;  // NEW in v0.5.0 (CEPT 433.05-434.79 MHz, 10 mW)
extern const Region kRegionUs915;
extern const Region kRegionAu915;
extern const Region kRegionIn865;
extern const Region kRegionKr920;
extern const Region kRegionAs923;

const LoraProfile* profileByName(const char* name);
const Region* regionByName(const char* name);
// Build-time default used when the user has not configured a region.
const Region& defaultRegion();

// --- Regulatory policy and override (docs/design/regulatory-and-regions.md §2) ---------------
// Version 0.5.0: region values are legal maxima (EIRP ceilings). Firmware never exceeds them
// unless the user explicitly enables the advanced override; even then the PA hardware cap is
// absolute. NOT LEGAL ADVICE: verify local rules and antenna gain.
constexpr char kRegulatoryOverrideWarning[] =
    "Regulatory override active \u2014 transmissions may violate local radio regulations and "
    "void your radio's certification. Use only in a life-threatening emergency where local law "
    "permits. You are responsible for compliance and for any interference or damage.";

// SX126x/LR1110 practical tuning range (kHz); frequencies outside are rejected by the radio.
constexpr uint32_t kRadioMinFrequencyKhz = 150000;
constexpr uint32_t kRadioMaxFrequencyKhz = 960000;

// A profile is selectable only when its bandwidth fits the region's maximum legal bandwidth.
bool profileFitsRegion(const LoraProfile& profile, const Region& region);

// Custom (override) centre frequency validation: non-zero and within the radio tuning range.
bool validateFrequencyKhz(uint32_t khz);

// Effective duty-cycle percentage used by the token bucket.
//   override_pct < 0  -> legal region duty (-1 = unlimited for fair-use regions)
//   override_pct 0..100 -> override (0 = block all transmissions)
int16_t effectiveDutyPct(const Region& region, int16_t override_pct);

// True when the requested duty/frequency exceeds the legal region defaults. Power above the
// legal maximum is itself an override request but cannot be judged in isolation from the other
// fields, so callers use effectivePowerDbm() for the clamp.
bool overrideExceedsLegal(const Region& region, int16_t duty_override_pct,
                          uint32_t frequency_override_khz);

// Sentinel for "no explicit power request" in the regulatory config. Values -9..30 are real
// requests; anything below -9 (including this sentinel) means "use the region maximum".
constexpr int8_t kNoTxPowerOverrideDbm = -128;

// Effective TX power: legal-region max by default, override request when the override is active,
// always clamped to the board PA cap and the radio's -9 dBm floor.
int8_t effectivePowerDbm(const Region& region, int8_t hardware_max_dbm, int8_t requested_dbm,
                         bool override_active);

// Effective centre frequency: override when set, otherwise the region centre.
uint32_t effectiveFrequencyKhz(const Region& region, uint32_t frequency_override_khz);

// On-air time in milliseconds, Semtech AN1200.13 formula (same as util::lora_airtime_ms).
uint32_t loraAirtimeMs(const LoraProfile& profile, size_t payload_len);

// Maximum payload bytes for a preset/region combination, and chunk sizing helpers.
size_t maxAttachmentBytes(const LoraProfile& profile, const Region& region);

// Token bucket limiting transmitted airtime to the region duty cycle. Stored in microsecond
// token units so the math stays integral. Regions without a duty-cycle limit yield an
// always-allowing budget.
class AirtimeBudget {
public:
    // `duty_override_pct` follows effectiveDutyPct() semantics. `enabled=false` disables
    // enforcement entirely (tests / unlimited regions); an enabled budget with 0 % blocks all
    // transmissions instead of silently allowing them.
    void configure(const Region& region, int16_t duty_override_pct = -1, bool enabled = true);
    void reset(uint64_t now_ms);
    int16_t dutyPct() const { return duty_pct_; }
    bool blocksAllTx() const { return enabled_ && capacity_ms_ == 0; }
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
    int16_t duty_pct_ = -1;
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

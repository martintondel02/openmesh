// SPDX-License-Identifier: AGPL-3.0-or-later
#include "openmesh/om_region.h"

#include <cmath>
#include <cstring>

namespace om {

const LoraProfile kLongSlow = {"LONG_SLOW", 12, 125, 8, 16, true, true};
const LoraProfile kLongFast = {"LONG_FAST", 11, 250, 5, 16, true, true};
const LoraProfile kMediumFast = {"MEDIUM_FAST", 9, 250, 5, 16, true, true};
const LoraProfile kShortFast = {"SHORT_FAST", 7, 250, 5, 16, true, true};
const LoraProfile kShortTurbo = {"SHORT_TURBO", 7, 500, 5, 16, true, true};

// Region table v2 (v0.5.0): legal maxima, mirrored from crates/openmesh-core util.rs and
// docs/design/regulatory-and-regions.md §1. Do not edit one without the other.
const Region kRegionEu868 = {"EU868", 869525, 27, 10, 250};   // CEPT 869.4-869.65, 500 mW ERP
const Region kRegionEu433 = {"EU433", 433920, 10, 10, 250};   // CEPT 433.05-434.79, 10 mW
const Region kRegionUs915 = {"US915", 915000, 30, -1, 500};   // FCC 902-928
const Region kRegionAu915 = {"AU915", 915000, 30, -1, 500};   // ACMA 915-928
const Region kRegionIn865 = {"IN865", 865000, 30, -1, 250};   // India, max-BW audit pending
const Region kRegionKr920 = {"KR920", 920000, 23, -1, 250};   // Korea
const Region kRegionAs923 = {"AS923", 923200, 20, 10, 250};   // conservative AS923 floor

namespace {
const LoraProfile* const kProfiles[] = {&kLongSlow, &kLongFast, &kMediumFast, &kShortFast,
                                        &kShortTurbo};
const Region* const kRegions[] = {&kRegionEu868, &kRegionEu433, &kRegionUs915, &kRegionAu915,
                                  &kRegionIn865, &kRegionKr920, &kRegionAs923};
}  // namespace

const LoraProfile* profileByName(const char* name) {
    if (name == nullptr) return nullptr;
    for (const LoraProfile* p : kProfiles) {
        if (std::strcmp(p->name, name) == 0) return p;
    }
    return nullptr;
}

const Region* regionByName(const char* name) {
    if (name == nullptr) return nullptr;
    for (const Region* r : kRegions) {
        if (std::strcmp(r->name, name) == 0) return r;
    }
    return nullptr;
}

const Region& defaultRegion() {
#ifdef OM_DEFAULT_REGION
    const Region* r = regionByName(OM_DEFAULT_REGION);
    if (r != nullptr) return *r;
#endif
    return kRegionEu868;
}

uint32_t loraAirtimeMs(const LoraProfile& profile, size_t payload_len) {
    const double sf = (double)profile.sf;
    const double bw = (double)profile.bw_khz * 1000.0;
    const double tsym = std::pow(2.0, (int)profile.sf) / bw;
    const double preamble = ((double)profile.preamble_symbols + 4.25) * tsym;

    const double pl = (double)payload_len + (profile.crc ? 2.0 : 0.0);
    const double ih = profile.explicit_header ? 0.0 : 1.0;
    const double de = (profile.sf >= 11 && profile.bw_khz == 125) ? 1.0 : 0.0;
    const int cr_int = profile.coding_rate < 5 ? 5 : (profile.coding_rate > 8 ? 8 : profile.coding_rate);
    const double cr = (double)(cr_int - 4);
    const double num = 8.0 * pl - 4.0 * sf + 28.0 - 20.0 * ih;
    const double den = 4.0 * (sf - 2.0 * de);
    double symbols = std::ceil(num / den);
    if (symbols < 0.0) symbols = 0.0;
    const double payload_symbols = 8.0 + symbols * (cr + 4.0);
    double total_ms = (preamble + payload_symbols * tsym) * 1000.0;
    if (total_ms < 1.0) total_ms = 1.0;
    return (uint32_t)std::ceil(total_ms);
}

size_t maxAttachmentBytes(const LoraProfile& profile, const Region& region) {
    (void)region;
    const uint32_t per_frame = loraAirtimeMs(profile, 240);
    if (per_frame == 0) return 512;
    const size_t frames = (size_t)(90000u / per_frame);
    return frames * 180 > 512 ? frames * 180 : 512;
}

bool profileFitsRegion(const LoraProfile& profile, const Region& region) {
    return region.bw_khz != 0 && profile.bw_khz <= region.bw_khz;
}

bool validateFrequencyKhz(uint32_t khz) {
    return khz >= kRadioMinFrequencyKhz && khz <= kRadioMaxFrequencyKhz;
}

int16_t effectiveDutyPct(const Region& region, int16_t override_pct) {
    if (override_pct >= 0) {
        return override_pct > 100 ? 100 : override_pct;
    }
    return region.duty_cycle_pct;
}

bool overrideExceedsLegal(const Region& region, int16_t duty_override_pct,
                          uint32_t frequency_override_khz) {
    if (duty_override_pct >= 0) {
        // A legal region duty of -1 means "fair use, no percentage"; any explicit percentage is
        // then a self-imposed restriction, not an excess.
        if (region.duty_cycle_pct >= 0 && duty_override_pct > region.duty_cycle_pct) return true;
    }
    if (frequency_override_khz != 0 && frequency_override_khz != region.freq_khz) return true;
    return false;
}

int8_t effectivePowerDbm(const Region& region, int8_t hardware_max_dbm, int8_t requested_dbm,
                         bool override_active) {
    int8_t target = requested_dbm >= -9 ? requested_dbm : region.max_power_dbm;
    if (!override_active && target > region.max_power_dbm) {
        // Without an active override, a power request is only ever a reduction: clamp to the
        // legal ceiling (the docs' "defaults = legal maxima" rule).
        target = region.max_power_dbm;
    }
    if (target > hardware_max_dbm) target = hardware_max_dbm;
    if (target < -9) target = -9;
    return target;
}

uint32_t effectiveFrequencyKhz(const Region& region, uint32_t frequency_override_khz) {
    return frequency_override_khz != 0 ? frequency_override_khz : region.freq_khz;
}

void AirtimeBudget::configure(const Region& region, int16_t duty_override_pct, bool enabled) {
    const int16_t pct = effectiveDutyPct(region, duty_override_pct);
    duty_pct_ = pct;
    if (!enabled || pct < 0) {
        // Enforcement disabled, or fair-use region with no override: always allow.
        enabled_ = false;
        capacity_ms_ = 0;
        refill_ms_per_sec_ = 0;
        tokens_us_ = 0;
        initialized_ = false;
        return;
    }
    enabled_ = true;
    if (pct == 0) {
        // Legal way to go silent: enabled budget with no tokens at all.
        capacity_ms_ = 0;
        refill_ms_per_sec_ = 0;
        tokens_us_ = 0;
        initialized_ = true;
        last_ms_ = 0;
        return;
    }
    // One hour of budget at the effective duty cycle, refilled continuously.
    capacity_ms_ = (uint32_t)(3600u * 1000u * (uint32_t)pct / 100u);
    refill_ms_per_sec_ = (uint32_t)(1000u * (uint32_t)pct / 100u);
    tokens_us_ = (int64_t)capacity_ms_ * 1000;
    initialized_ = false;
}

void AirtimeBudget::reset(uint64_t now_ms) {
    tokens_us_ = (int64_t)capacity_ms_ * 1000;
    last_ms_ = now_ms;
    initialized_ = true;
}

void AirtimeBudget::refill(uint64_t now_ms) {
    if (!initialized_) {
        reset(now_ms);
        return;
    }
    if (now_ms <= last_ms_) {
        last_ms_ = now_ms;
        return;
    }
    const uint64_t elapsed = now_ms - last_ms_;
    const int64_t cap = (int64_t)capacity_ms_ * 1000;
    tokens_us_ += (int64_t)elapsed * (int64_t)refill_ms_per_sec_;
    if (tokens_us_ > cap) tokens_us_ = cap;
    last_ms_ = now_ms;
}

bool AirtimeBudget::tryConsume(uint32_t airtime_ms, uint64_t now_ms) {
    if (!enabled_) return true;
    refill(now_ms);
    const int64_t need = (int64_t)airtime_ms * 1000;
    if (tokens_us_ < need) return false;
    tokens_us_ -= need;
    return true;
}

uint32_t AirtimeBudget::remainingMs(uint64_t now_ms) {
    if (!enabled_) return 0xFFFFFFFFu;
    refill(now_ms);
    return (uint32_t)(tokens_us_ / 1000);
}

bool OriginateLimiter::allow(uint64_t now_ms, bool is_ack_or_probe) {
    if (is_ack_or_probe) return true;
    if (!initialized_) {
        initialized_ = true;
        window_start_ms_ = now_ms;
        count_ = 0;
    }
    if (now_ms - window_start_ms_ >= 60000ull) {
        window_start_ms_ = now_ms;
        count_ = 0;
    }
    if (count_ >= kMaxOriginatePerMin) return false;
    ++count_;
    return true;
}

}  // namespace om

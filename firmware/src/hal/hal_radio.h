// SPDX-License-Identifier: AGPL-3.0-or-later
// LoRa radio abstraction over RadioLib: SX1262 (RAK4631, Seeed Solar P1, Heltec) and LR1110
// (T1000-E, SX126x-compatible command set). Board pins/TCXO/RF-switch configuration comes
// from firmware/boards/<board>.cpp.
#ifndef OPENMESH_HAL_RADIO_H
#define OPENMESH_HAL_RADIO_H

#include <cstddef>
#include <cstdint>

#include "board.h"
#include "openmesh/om_region.h"

namespace om {
namespace hal {

class Radio {
public:
    bool begin(const board::Board& board, const LoraProfile& profile, const Region& region,
               int8_t tx_power_dbm, uint32_t frequency_override_khz = 0);
    // Retunes for a new region/profile/power/frequency at runtime (returns false if the radio
    // rejects it). `frequency_override_khz` 0 means the region centre frequency.
    bool reconfigure(const LoraProfile& profile, const Region& region, int8_t tx_power_dbm,
                     uint32_t frequency_override_khz = 0);
    bool transmit(const uint8_t* data, size_t len);
    // Polls for a received packet. Returns packet length (>0), 0 when none is available, or -1
    // on radio error.
    int receive(uint8_t* buffer, size_t capacity, int16_t& rssi, int16_t& snr);
    // Re-arms continuous RX. Returns false if the transceiver rejected the command.
    bool startReceive();
    // Radio health watchdog: detects a receiver that was left disarmed (e.g. by a live retune)
    // and re-arms it; hard-resets the transceiver over SPI/NRST if arming keeps failing. Call
    // from the main loop; internally rate-limited (one probe per 5 s).
    void pollHealth(uint32_t now_ms);
    // Pops one pending radio-health event detail ("rearm", "reinit", "reinit_failed"), or nullptr.
    // The main loop maps these onto companion Event messages.
    const char* takeRadioEvent();
    uint32_t rearmCount() const { return rearm_count_; }
    uint32_t reinitCount() const { return reinit_count_; }
    void sleep();
    bool ok() const { return ok_; }
    int16_t lastRssi() const { return last_rssi_; }
    int16_t lastSnr() const { return last_snr_; }

private:
    bool applyConfig(const LoraProfile& profile, const Region& region, int8_t tx_power_dbm,
                     uint32_t frequency_override_khz);
    // Starts continuous RX and records activity. False when RadioLib rejected the command.
    bool armReceiver();
    // Re-runs begin() from the cached configuration (toggles the transceiver NRST) and re-arms.
    bool hardRecover(uint32_t now_ms);
    // Reads the LR1110 GET_STATUS register pair (read-only, allowed in any chip mode).
    // Returns false if the SPI transaction failed (transceiver unresponsive).
    bool readChipStatus(uint8_t out[2]);
    void noteActivity(uint32_t now_ms);
    void markUnhealthy(uint32_t now_ms);
    void postEvent(const char* detail);
    void* radio_ = nullptr;   // RadioLib SX1262* / LR1110*
    void* module_ = nullptr;
    const board::Board* board_ = nullptr;
    bool ok_ = false;
    int16_t last_rssi_ = 0;
    int16_t last_snr_ = 0;
    // Cached last applied configuration so a wedged transceiver can be re-initialised.
    bool have_config_ = false;
    const LoraProfile* cfg_profile_ = nullptr;
    const Region* cfg_region_ = nullptr;
    int8_t cfg_power_dbm_ = 0;
    uint32_t cfg_frequency_khz_ = 0;
    // Health watchdog state (millis() domain).
    uint32_t last_activity_ms_ = 0;    // last IRQ flag / RX / TX / successful arm
    uint32_t last_health_ms_ = 0;     // last pollHealth probe
    uint32_t last_recover_ms_ = 0;    // last hard-recovery attempt
    uint32_t unhealthy_since_ms_ = 0; // 0 = healthy
    uint32_t rearm_count_ = 0;
    uint32_t reinit_count_ = 0;
    uint32_t reinit_fail_count_ = 0;
    const char* pending_event_ = nullptr;
};

}  // namespace hal
}  // namespace om

#endif  // OPENMESH_HAL_RADIO_H

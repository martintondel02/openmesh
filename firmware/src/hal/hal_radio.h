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
               int8_t tx_power_dbm);
    // Retunes for a new region/profile at runtime (returns false if the radio rejects it).
    bool reconfigure(const LoraProfile& profile, const Region& region, int8_t tx_power_dbm);
    bool transmit(const uint8_t* data, size_t len);
    // Polls for a received packet. Returns packet length (>0), 0 when none is available, or -1
    // on radio error.
    int receive(uint8_t* buffer, size_t capacity, int16_t& rssi, int16_t& snr);
    void startReceive();
    void sleep();
    bool ok() const { return ok_; }
    int16_t lastRssi() const { return last_rssi_; }
    int16_t lastSnr() const { return last_snr_; }

private:
    bool applyConfig(const LoraProfile& profile, const Region& region, int8_t tx_power_dbm);
    void* radio_ = nullptr;   // RadioLib SX1262* / LR1110*
    void* module_ = nullptr;
    const board::Board* board_ = nullptr;
    bool ok_ = false;
    int16_t last_rssi_ = 0;
    int16_t last_snr_ = 0;
};

}  // namespace hal
}  // namespace om

#endif  // OPENMESH_HAL_RADIO_H

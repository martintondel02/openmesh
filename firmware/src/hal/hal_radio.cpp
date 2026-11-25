// SPDX-License-Identifier: AGPL-3.0-or-later
#include "hal_radio.h"

#if defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES) || defined(ESP32)
#include <Arduino.h>
#include <RadioLib.h>
#endif

namespace om {
namespace hal {

#if defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES) || defined(ESP32)

#if defined(OM_RADIO_LR1110)
using RadioLibRadio = LR1110;
#else
using RadioLibRadio = SX1262;
#endif

bool Radio::begin(const board::Board& board, const LoraProfile& profile, const Region& region,
                  int8_t tx_power_dbm) {
    board_ = &board;
    if (board.radio.power_en >= 0) {
        pinMode((uint32_t)board.radio.power_en, OUTPUT);
        digitalWrite((uint32_t)board.radio.power_en, HIGH);
        delay(10);
    }
    SPI.begin((int8_t)board.radio.sck, (int8_t)board.radio.miso, (int8_t)board.radio.mosi,
              (int8_t)board.radio.cs);
    auto* module = new Module((uint32_t)board.radio.cs, (uint32_t)board.radio.irq,
                              (uint32_t)board.radio.reset, (uint32_t)board.radio.busy, &SPI);
    auto* radio = new RadioLibRadio(module);
    module_ = module;
    radio_ = radio;
    if (!applyConfig(profile, region, tx_power_dbm)) return false;
    startReceive();
    ok_ = true;
    return true;
}

bool Radio::applyConfig(const LoraProfile& profile, const Region& region, int8_t tx_power_dbm) {
    auto* radio = static_cast<RadioLibRadio*>(radio_);
    if (radio == nullptr) return false;
    const float freq_mhz = (float)region.freq_khz / 1000.0f;
    int8_t power = tx_power_dbm;
    if (power > region.max_power_dbm) power = region.max_power_dbm;
    if (power < -9) power = -9;
    const uint8_t sync_word = 0x12;  // OpenMesh private sync word (same convention as §15)
    const int16_t state =
        radio->begin(freq_mhz, (float)profile.bw_khz, profile.sf, profile.coding_rate, sync_word,
                     power, profile.preamble_symbols, board_->radio.tcxo_voltage);
    if (state != RADIOLIB_ERR_NONE) return false;
#if !defined(OM_RADIO_LR1110)
    if (board_->radio.dio2_as_rf_switch) radio->setDio2AsRfSwitch(true);
    if (board_->radio.rxen >= 0) {
        radio->setRfSwitchPins((uint32_t)board_->radio.rxen,
                               board_->radio.txen >= 0 ? (uint32_t)board_->radio.txen
                                                      : RADIOLIB_NC);
    }
    radio->setDcDc(true);
#else
    // LR1110: the internal switch is configured by begin(); DIO3 supplies the TCXO voltage.
    (void)board_->radio.dio2_as_rf_switch;
#endif
    radio->setOutputPower(power);
    last_rssi_ = 0;
    last_snr_ = 0;
    return true;
}

bool Radio::reconfigure(const LoraProfile& profile, const Region& region, int8_t tx_power_dbm) {
    return applyConfig(profile, region, tx_power_dbm);
}

bool Radio::transmit(const uint8_t* data, size_t len) {
    auto* radio = static_cast<RadioLibRadio*>(radio_);
    if (radio == nullptr || data == nullptr || len > 255) return false;
    const int16_t state = radio->transmit(const_cast<uint8_t*>(data), len);
    // Always return to RX so relays keep hearing frames.
    startReceive();
    return state == RADIOLIB_ERR_NONE;
}

int Radio::receive(uint8_t* buffer, size_t capacity, int16_t& rssi, int16_t& snr) {
    auto* radio = static_cast<RadioLibRadio*>(radio_);
    if (radio == nullptr) return -1;
    const size_t available = radio->available();
    if (available == 0) return 0;
    if (available > capacity) {
        // Oversized packet for the OMP MTU: read and discard.
        uint8_t discard[256];
        radio->readData(discard, sizeof(discard));
        startReceive();
        return -1;
    }
    const int16_t state = radio->readData(buffer, available);
    last_rssi_ = (int16_t)radio->getRSSI();
    last_snr_ = (int16_t)radio->getSNR();
    rssi = last_rssi_;
    snr = last_snr_;
    startReceive();
    if (state != RADIOLIB_ERR_NONE) return -1;
    return (int)available;
}

void Radio::startReceive() {
    auto* radio = static_cast<RadioLibRadio*>(radio_);
    if (radio != nullptr) radio->startReceive();
}

void Radio::sleep() {
    auto* radio = static_cast<RadioLibRadio*>(radio_);
    if (radio != nullptr) radio->sleep();
}

#else
// Host build: no radio hardware.
bool Radio::begin(const board::Board&, const LoraProfile&, const Region&, int8_t) { return false; }
bool Radio::reconfigure(const LoraProfile&, const Region&, int8_t) { return false; }
bool Radio::transmit(const uint8_t*, size_t) { return false; }
int Radio::receive(uint8_t*, size_t, int16_t&, int16_t&) { return 0; }
void Radio::startReceive() {}
void Radio::sleep() {}
#endif

}  // namespace hal
}  // namespace om

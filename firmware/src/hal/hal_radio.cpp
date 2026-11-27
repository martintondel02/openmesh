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

// Oversized-frame discard buffer: receive() runs on the 4 KiB loop task stack, so this must not be
// a local. The firmware main loop is single-threaded.
uint8_t g_radio_discard[255];

bool Radio::begin(const board::Board& board, const LoraProfile& profile, const Region& region,
                  int8_t tx_power_dbm, uint32_t frequency_override_khz) {
    board_ = &board;
    if (board.radio.power_en >= 0) {
        pinMode((uint32_t)board.radio.power_en, OUTPUT);
        digitalWrite((uint32_t)board.radio.power_en, HIGH);
        delay(10);
    }
#if defined(ARDUINO_ARCH_NRF52)
    // Adafruit nRF52 SPI: pins are configured separately from begin(), and RadioLib 7.x
    // takes the SPIClass by reference (not pointer).
    SPI.setPins((uint8_t)board.radio.miso, (uint8_t)board.radio.sck, (uint8_t)board.radio.mosi);
    SPI.begin();
#else
    SPI.begin((int8_t)board.radio.sck, (int8_t)board.radio.miso, (int8_t)board.radio.mosi,
              (int8_t)board.radio.cs);
#endif
    auto* module = new Module((uint32_t)board.radio.cs, (uint32_t)board.radio.irq,
                              (uint32_t)board.radio.reset, (uint32_t)board.radio.busy, SPI);
    auto* radio = new RadioLibRadio(module);
    module_ = module;
    radio_ = radio;
    if (!applyConfig(profile, region, tx_power_dbm, frequency_override_khz)) return false;
    startReceive();
    ok_ = true;
    return true;
}

bool Radio::applyConfig(const LoraProfile& profile, const Region& region, int8_t tx_power_dbm,
                        uint32_t frequency_override_khz) {
    auto* radio = static_cast<RadioLibRadio*>(radio_);
    if (radio == nullptr) return false;
    // Advance/test-network custom centre (already range-validated by the engine); 0 = region
    // default. Never silently ignore a requested frequency.
    const uint32_t freq_khz =
        frequency_override_khz != 0 ? frequency_override_khz : region.freq_khz;
    if (freq_khz < kRadioMinFrequencyKhz || freq_khz > kRadioMaxFrequencyKhz) return false;
    const float freq_mhz = (float)freq_khz / 1000.0f;
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
    // RadioLib 7 made the old SX126x::setDcDc() regulator switch protected; the radio defaults to
    // LDO mode, which is functional for first-article tests. DC-DC mode is a power follow-up.
#else
    // LR1110: on the T1000-E the sub-GHz antenna switch is driven by the radio's own DIO5-DIO8
    // pins. RadioLib's LR11x0 API has setRfSwitchTable()/setDioAsRfSwitch(), but begin() does not
    // configure the switch, so without this call radio.begin() still succeeds while every TX/RX is
    // routed into a disabled RF path. Verified on hardware 2026-11-27: the two-node RF test saw
    // zero received frames in both directions until this table was added. Table source: Meshtastic
    // T1000-E variant (variants/nrf52840/tracker-t1000-e/rfswitch.h), same wiring as the board
    // pin map in boards/t1000e.cpp.
    static const uint32_t rfswitch_pins[Module::RFSWITCH_MAX_PINS] = {
        RADIOLIB_LR11X0_DIO5, RADIOLIB_LR11X0_DIO6, RADIOLIB_LR11X0_DIO7, RADIOLIB_LR11X0_DIO8,
        RADIOLIB_NC};
    static const Module::RfSwitchMode_t rfswitch_table[] = {
        // mode                DIO5  DIO6  DIO7  DIO8
        {LR11x0::MODE_STBY, {LOW, LOW, LOW, LOW}},
        {LR11x0::MODE_RX, {HIGH, LOW, LOW, HIGH}},
        {LR11x0::MODE_TX, {HIGH, HIGH, LOW, HIGH}},
        {LR11x0::MODE_TX_HP, {LOW, HIGH, LOW, HIGH}},
        {LR11x0::MODE_TX_HF, {LOW, LOW, LOW, LOW}},
        {LR11x0::MODE_GNSS, {LOW, LOW, HIGH, LOW}},
        {LR11x0::MODE_WIFI, {LOW, LOW, LOW, LOW}},
        END_OF_MODE_TABLE,
    };
    radio->setRfSwitchTable(rfswitch_pins, rfswitch_table);
    (void)board_->radio.dio2_as_rf_switch;
#endif
    radio->setOutputPower(power);
    last_rssi_ = 0;
    last_snr_ = 0;
    return true;
}

bool Radio::reconfigure(const LoraProfile& profile, const Region& region, int8_t tx_power_dbm,
                        uint32_t frequency_override_khz) {
    return applyConfig(profile, region, tx_power_dbm, frequency_override_khz);
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
    // RadioLib 7's PhysicalLayer::available() reports the direct-receive ring buffer and is always
    // 0 in LoRa packet mode, so polling it meant no frame was ever read (this was the second,
    // independent reason the first two-node RF test saw rx_frames stay at 0). Poll the radio IRQ
    // flags instead; readData() validates CRC, copies the packet and clears the flags.
#if defined(OM_RADIO_LR1110)
    constexpr uint32_t kRxDone = RADIOLIB_LR11X0_IRQ_RX_DONE;
    constexpr uint32_t kRxError =
        RADIOLIB_LR11X0_IRQ_CRC_ERR | RADIOLIB_LR11X0_IRQ_TIMEOUT | RADIOLIB_LR11X0_IRQ_HEADER_ERR;
    constexpr uint32_t kIrqAll = RADIOLIB_LR11X0_IRQ_ALL;
#else
    constexpr uint16_t kRxDone = RADIOLIB_SX126X_IRQ_RX_DONE;
    constexpr uint16_t kRxError =
        RADIOLIB_SX126X_IRQ_CRC_ERR | RADIOLIB_SX126X_IRQ_TIMEOUT | RADIOLIB_SX126X_IRQ_HEADER_ERR;
    constexpr uint16_t kIrqAll = RADIOLIB_SX126X_IRQ_ALL;
#endif
    const uint32_t irq = radio->getIrqFlags();
    if ((irq & kRxDone) == 0) {
        if ((irq & kRxError) != 0) {
            // Drop a corrupted/timed-out reception and re-arm so the receiver cannot wedge.
            radio->clearIrqFlags(kIrqAll);
            startReceive();
            return -1;
        }
        return 0;
    }
    const size_t length = radio->getPacketLength();
    if (length == 0 || length > capacity) {
        // Oversized (or unexpected) packet for the OMP MTU: read and discard.
        const size_t dropped = radio->readData(g_radio_discard, sizeof(g_radio_discard));
        (void)dropped;
        startReceive();
        return -1;
    }
    const int16_t state = radio->readData(buffer, capacity);
    last_rssi_ = (int16_t)radio->getRSSI();
    last_snr_ = (int16_t)radio->getSNR();
    rssi = last_rssi_;
    snr = last_snr_;
    startReceive();
    if (state != RADIOLIB_ERR_NONE) return -1;
    return (int)length;
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
bool Radio::begin(const board::Board&, const LoraProfile&, const Region&, int8_t, uint32_t) {
    return false;
}
bool Radio::reconfigure(const LoraProfile&, const Region&, int8_t, uint32_t) { return false; }
bool Radio::transmit(const uint8_t*, size_t) { return false; }
int Radio::receive(uint8_t*, size_t, int16_t&, int16_t&) { return 0; }
void Radio::startReceive() {}
void Radio::sleep() {}
#endif

}  // namespace hal
}  // namespace om

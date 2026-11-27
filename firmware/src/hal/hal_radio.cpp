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
    // applyConfig leaves the transceiver in STANDBY (RadioLib::begin behavior), so arm RX now.
    // If that fails once, reset the transceiver and retry before giving up; the watchdog would
    // also retry, but initialisation is the one place where a failure is worth handling inline.
    if (!armReceiver()) (void)hardRecover((uint32_t)millis());
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
    cfg_profile_ = &profile;
    cfg_region_ = &region;
    cfg_power_dbm_ = tx_power_dbm;
    cfg_frequency_khz_ = frequency_override_khz;
    have_config_ = true;
    last_rssi_ = 0;
    last_snr_ = 0;
    return true;
}

bool Radio::reconfigure(const LoraProfile& profile, const Region& region, int8_t tx_power_dbm,
                        uint32_t frequency_override_khz) {
    const bool ok = applyConfig(profile, region, tx_power_dbm, frequency_override_khz);
    // RadioLib::begin() inside applyConfig() stops reception and leaves the chip in STANDBY.
    // Without re-arming here every live retune (companion ConfigSet, app settings push) leaves the
    // receiver deaf until some later transmit() happens to re-arm it. This was the reproducible
    // "T1000-E goes deaf while the MCU still answers" bug.
    if (ok && !armReceiver()) {
        if (!hardRecover((uint32_t)millis())) postEvent("reinit_failed");
    }
    return ok;
}

bool Radio::transmit(const uint8_t* data, size_t len) {
    auto* radio = static_cast<RadioLibRadio*>(radio_);
    if (radio == nullptr || data == nullptr || len > 255) return false;
    const int16_t state = radio->transmit(const_cast<uint8_t*>(data), len);
    last_tx_error_ = state;
    // Always return to RX so relays keep hearing frames.
    if (!armReceiver()) markUnhealthy((uint32_t)millis());
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
    if (irq != 0) noteActivity((uint32_t)millis());
    if ((irq & kRxDone) == 0) {
        if ((irq & kRxError) != 0) {
            // Drop a corrupted/timed-out reception and re-arm so the receiver cannot wedge.
            radio->clearIrqFlags(kIrqAll);
            if (!armReceiver()) markUnhealthy((uint32_t)millis());
            return -1;
        }
        return 0;
    }
    const size_t length = radio->getPacketLength();
    if (length == 0 || length > capacity) {
        // Oversized (or unexpected) packet for the OMP MTU: read and discard.
        const size_t dropped = radio->readData(g_radio_discard, sizeof(g_radio_discard));
        (void)dropped;
        if (!armReceiver()) markUnhealthy((uint32_t)millis());
        return -1;
    }
    const int16_t state = radio->readData(buffer, capacity);
    last_rssi_ = (int16_t)radio->getRSSI();
    last_snr_ = (int16_t)radio->getSNR();
    rssi = last_rssi_;
    snr = last_snr_;
    if (!armReceiver()) markUnhealthy((uint32_t)millis());
    if (state != RADIOLIB_ERR_NONE) return -1;
    return (int)length;
}

bool Radio::startReceive() { return armReceiver(); }

bool Radio::armReceiver() {
    auto* radio = static_cast<RadioLibRadio*>(radio_);
    if (radio == nullptr) return false;
    const int16_t state = radio->startReceive();
    if (state != RADIOLIB_ERR_NONE) return false;
    last_activity_ms_ = (uint32_t)millis();
    return true;
}

void Radio::noteActivity(uint32_t now_ms) {
    last_activity_ms_ = now_ms;
    unhealthy_since_ms_ = 0;
}

void Radio::markUnhealthy(uint32_t now_ms) {
    if (unhealthy_since_ms_ == 0) unhealthy_since_ms_ = now_ms;
}

void Radio::postEvent(const char* detail) { pending_event_ = detail; }

const char* Radio::takeRadioEvent() {
    const char* detail = pending_event_;
    pending_event_ = nullptr;
    return detail;
}

#if defined(OM_RADIO_LR1110)
bool Radio::readChipStatus(uint8_t out[2]) {
    auto* mod = static_cast<Module*>(module_);
    if (mod == nullptr) return false;
    // GET_STATUS is read-only and allowed in every chip mode; it does not disturb reception.
    // LR11x0 needs two SPI transactions: command first, then a command-less read (RadioLib's
    // LRxxxx::SPIcommand does the same by temporarily setting the command width to 0).
    if (mod->SPIwriteStream(RADIOLIB_LR11X0_CMD_GET_STATUS, nullptr, 0, true, false) !=
        RADIOLIB_ERR_NONE) {
        return false;
    }
    const Module::BitWidth_t previous = mod->spiConfig.widths[RADIOLIB_MODULE_SPI_WIDTH_CMD];
    mod->spiConfig.widths[RADIOLIB_MODULE_SPI_WIDTH_CMD] = Module::BITS_0;
    const int16_t state = mod->SPIreadStream(RADIOLIB_LRXXXX_CMD_NOP, out, 2, true, false);
    mod->spiConfig.widths[RADIOLIB_MODULE_SPI_WIDTH_CMD] = previous;
    return state == RADIOLIB_ERR_NONE;
}
#else
bool Radio::readChipStatus(uint8_t[2]) { return false; }
#endif

bool Radio::hardRecover(uint32_t now_ms) {
    if (!have_config_ || cfg_profile_ == nullptr || cfg_region_ == nullptr) return false;
    ++reinit_count_;
    // applyConfig() re-runs RadioLib::begin(), which toggles the transceiver NRST pin, so this
    // recovers a wedged/unresponsive LR1110/SX1262 without resetting the whole MCU.
    if (!applyConfig(*cfg_profile_, *cfg_region_, cfg_power_dbm_, cfg_frequency_khz_)) return false;
    if (!armReceiver()) return false;
    noteActivity(now_ms);
    return true;
}

void Radio::pollHealth(uint32_t now_ms) {
    if (!ok_ || radio_ == nullptr) return;
    const uint32_t now = now_ms;
    if ((uint32_t)(now - last_health_ms_) < 5000u) return;
    last_health_ms_ = now;

#if defined(OM_RADIO_LR1110)
    // Precise, non-intrusive check: ask the LR1110 which mode it is actually in. A receiver that
    // was left in STANDBY (or a transceiver whose SPI stopped answering) is the failure class
    // that used to require an MCU reset.
    uint8_t status[2] = {0, 0};
    if (readChipStatus(status)) {
        const uint8_t mode = (uint8_t)((status[1] >> 1) & 0x07u);
        if (mode == 0x04u /*RX*/ || mode == 0x05u /*TX in progress*/) {
            noteActivity(now);
            return;
        }
        // Not listening: re-arm continuous RX first (radio-only recovery).
        if (armReceiver()) {
            ++rearm_count_;
            if (unhealthy_since_ms_ != 0) {
                postEvent("rearm");
                unhealthy_since_ms_ = 0;
                reinit_fail_count_ = 0;
            }
        } else {
            markUnhealthy(now);
        }
    } else {
        markUnhealthy(now);
    }
#else
    // SX126x has no cheap chip-mode read, and the "deaf" failure class seen on the T1000-E is a
    // retune/arm issue, not an idle-RX issue. Do not touch a healthy SX126x receiver here; the
    // hard-recovery block below still runs after a real arm failure (markUnhealthy()).
    (void)last_activity_ms_;
#endif

    if (unhealthy_since_ms_ != 0 && (uint32_t)(now - last_recover_ms_) >= 5000u) {
        last_recover_ms_ = now;
        if (hardRecover(now)) {
            postEvent("reinit");
            unhealthy_since_ms_ = 0;
            reinit_fail_count_ = 0;
        } else if (++reinit_fail_count_ >= 3u) {
            postEvent("reinit_failed");
            reinit_fail_count_ = 0;
        }
    }
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
bool Radio::startReceive() { return false; }
void Radio::pollHealth(uint32_t) {}
const char* Radio::takeRadioEvent() { return nullptr; }
bool Radio::armReceiver() { return false; }
bool Radio::hardRecover(uint32_t) { return false; }
bool Radio::readChipStatus(uint8_t[2]) { return false; }
void Radio::noteActivity(uint32_t) {}
void Radio::markUnhealthy(uint32_t) {}
void Radio::postEvent(const char*) {}
void Radio::sleep() {}
#endif

}  // namespace hal
}  // namespace om

// SPDX-License-Identifier: AGPL-3.0-or-later
// Board description consumed by the PlatformIO HAL and the engine bootstrap.
//
// Pin numbers are Arduino/RadioLib pin indices as used by the vendor Arduino core for each
// board (for the nRF52 boards these equal the raw nRF GPIO number, port*32+pin). Sources are
// cited per board and in /NOTICE; see docs/hardware/boards.md for the human-readable tables.
#ifndef OPENMESH_BOARD_H
#define OPENMESH_BOARD_H

#include <cstddef>
#include <cstdint>

#include "openmesh/om_messages.h"

namespace om {
namespace board {

// Which firmware role the board runs unless the companion app configures otherwise.
enum class DefaultRole : uint8_t { Tracker, Repeater, Gateway, Client };

// Hard PA caps for the reference radios (v0.5.0 regulatory contract): SX1262 boards 22 dBm,
// LR1110 (T1000-E) 15 dBm. These are absolute: neither region maxima nor the regulatory
// override may exceed them. Single source of truth for both the board factories and the host
// parity tests (firmware/test/test_protocol).
constexpr int8_t kMaxTxPowerSx1262Dbm = 22;
constexpr int8_t kMaxTxPowerLr1110Dbm = 15;

struct PaCap {
    const char* board_id;
    int8_t max_tx_power_dbm;
};

constexpr PaCap kPaCaps[] = {
    {"t1000e", kMaxTxPowerLr1110Dbm},
    {"rak4631_solar", kMaxTxPowerSx1262Dbm},
    {"seeed_solar", kMaxTxPowerSx1262Dbm},
    {"heltec_tracker", kMaxTxPowerSx1262Dbm},
};
constexpr size_t kPaCapCount = sizeof(kPaCaps) / sizeof(kPaCaps[0]);

inline const PaCap* paCapForBoard(const char* id) {
    if (id == nullptr) return nullptr;
    for (size_t i = 0; i < kPaCapCount; ++i) {
        const char* a = kPaCaps[i].board_id;
        const char* b = id;
        while (*a != '\0' && *a == *b) {
            ++a;
            ++b;
        }
        if (*a == '\0' && *b == '\0') return &kPaCaps[i];
    }
    return nullptr;
}

enum class GnssModule : uint8_t { None, Airoha, L76K, UC6580 };

struct RadioPins {
    int8_t cs = -1;
    int8_t irq = -1;      // DIO1 / IRQ
    int8_t reset = -1;
    int8_t busy = -1;     // DIO2 used as BUSY on Semtech modules
    int8_t rxen = -1;     // external RX switch enable (RADIOLIB_NC if unused)
    int8_t txen = -1;
    int8_t power_en = -1; // rail enable required before radio init
    int8_t sck = -1;
    int8_t miso = -1;
    int8_t mosi = -1;
    float tcxo_voltage = 0.0f;  // 0 = no TCXO controlled by DIO3
    bool dio2_as_rf_switch = false;
    bool lr1110 = false;  // true for LR1110-class radios (RadioLib LR1110 driver)
};

struct GpsPins {
    int8_t rx = -1;    // MCU RX (from GNSS TX)
    int8_t tx = -1;    // MCU TX (to GNSS RX)
    int8_t en = -1;
    int8_t reset = -1;
    int8_t standby = -1;
    int8_t pps = -1;
    uint32_t baud = 9600;
    GnssModule module = GnssModule::None;
};

struct PowerPins {
    int8_t battery_adc = -1;
    int8_t solar_adc = -1;
    int8_t adc_ctrl = -1;       // divider enable (active HIGH)
    int8_t charge_status = -1;
    int8_t usb_detect = -1;
    float adc_multiplier = 1.0f;
    const uint16_t* ocv_table = nullptr;
    uint8_t ocv_count = 0;
};

struct UxPins {
    int8_t led = -1;
    bool led_active_high = true;
    int8_t button = -1;
    bool has_screen = false;
    int8_t tft_cs = -1;
    int8_t tft_dc = -1;
    int8_t tft_reset = -1;
    int8_t tft_backlight = -1;
};

struct Board {
    const char* id = "";
    const char* display_name = "";
    const char* mcu = "";
    const char* radio_chip = "";
    const char* default_region = "EU868";
    const char* default_profile = "MEDIUM_FAST";
    DefaultRole default_role = DefaultRole::Client;
    RadioPins radio{};
    GpsPins gps{};
    PowerPins power{};
    UxPins ux{};
    uint32_t flash_bytes = 0;
    bool has_wifi = false;
    bool has_ethernet = false;
    bool has_ble = true;
    bool has_usb = true;
    bool has_solar = false;
    // Hardware PA ceiling in dBm; the effective TX power is min(region legal max, this).
    int8_t max_tx_power_dbm = kMaxTxPowerSx1262Dbm;
};

// Implemented by exactly one firmware/boards/<board>.cpp, selected with -DOM_BOARD_*.
const Board& get();

}  // namespace board
}  // namespace om

#endif  // OPENMESH_BOARD_H

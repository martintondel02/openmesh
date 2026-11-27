
#include "t1000e.h"

#if defined(OM_BOARD_T1000E)

#include "openmesh/om_power.h"

namespace om {
namespace board {
namespace {
const Board kBoard = [] {
    Board b;
    b.id = "t1000e";
    b.display_name = "OpenMesh T1000-E";
    b.mcu = "nRF52840";
    b.radio_chip = "LR1110";
    b.default_region = "EU868";
    b.default_profile = "MEDIUM_FAST";
    b.default_role = DefaultRole::Tracker;
    b.radio.cs = 12;      // P0.12
    b.radio.irq = 33;     // P1.01 DIO1
    b.radio.reset = 42;   // P1.10
    b.radio.busy = 7;     // P0.07 DIO2
    b.radio.sck = 11;
    b.radio.miso = 40;
    b.radio.mosi = 41;
    b.radio.tcxo_voltage = 1.6f;
    b.radio.dio2_as_rf_switch = true;
    b.radio.lr1110 = true;
    b.gps.rx = 14;
    b.gps.tx = 13;
    b.gps.en = 43;
    b.gps.reset = 47;
    b.gps.baud = 115200;
    b.gps.module = GnssModule::Airoha;
    b.power.battery_adc = 2;       // P0.02 / AIN0
    b.power.adc_multiplier = 2.0f; // battery divider
    b.power.ocv_table = kOcvT1000E;
    b.power.ocv_count = (uint8_t)kOcvT1000ECount;
    b.power.charge_status = 36;    // P1.04
    b.power.usb_detect = 5;        // P0.05
    b.ux.led = 24;                 // P0.24 (green)
    b.ux.led_active_high = true;
    b.ux.button = 6;               // P0.06
    b.ux.has_screen = false;
    b.flash_bytes = 1024u * 1024u;  // internal nRF52840 flash
    b.has_wifi = false;
    b.has_ethernet = false;
    b.has_ble = true;
    b.has_usb = true;
    b.has_solar = false;
    b.max_tx_power_dbm =
        paCapForBoard(b.id)->max_tx_power_dbm;
    return b;
}();
}  // namespace
const Board& get() { return kBoard; }
}  // namespace board
}  // namespace om

#endif  // OM_BOARD_T1000E

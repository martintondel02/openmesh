
#include "rak4631_solar.h"

#if defined(OM_BOARD_RAK4631_SOLAR)

#include "openmesh/om_power.h"

namespace om {
namespace board {
namespace {
const Board kBoard = [] {
    Board b;
    b.id = "rak4631_solar";
    b.display_name = "RAK WisBlock 4631 solar node";
    b.mcu = "nRF52840";
    b.radio_chip = "SX1262";
    b.default_region = "EU868";
    b.default_profile = "MEDIUM_FAST";
    b.default_role = DefaultRole::Repeater;
    b.radio.cs = 42;     // P1.10
    b.radio.irq = 47;    // P1.15 DIO1
    b.radio.busy = 46;   // P1.14
    b.radio.reset = 38;  // P1.06
    b.radio.power_en = 37;  // P1.05, radio rail enable
    b.radio.sck = 43;
    b.radio.miso = 45;
    b.radio.mosi = 44;
    b.radio.tcxo_voltage = 1.8f;
    b.radio.dio2_as_rf_switch = true;
    b.radio.lr1110 = false;
    // Optional RAK1910/RAK12500 GNSS module on the WisBlock sensor slot (Serial1).
    b.gps.rx = 15;
    b.gps.tx = 16;
    b.gps.en = 34;  // 3V3_S rail enable (also powers the GPS)
    b.gps.baud = 9600;
    b.gps.module = GnssModule::L76K;
    b.power.battery_adc = 5;        // AIN0 / A0
    b.power.adc_multiplier = 1.73f; // 1.5/2.5 divider per RAK4631 schematic
    b.power.ocv_table = kOcvGenericLiIon;
    b.power.ocv_count = (uint8_t)kOcvGenericCount;
    b.power.usb_detect = -1;
    b.ux.led = 35;  // green
    b.ux.led_active_high = true;
    b.ux.button = 9;
    b.ux.has_screen = false;
    b.flash_bytes = 1024u * 1024u;
    b.has_wifi = false;
    b.has_ethernet = false;
    b.has_ble = true;
    b.has_usb = true;
    b.has_solar = true;  // solar enclosure / MPPT board
    b.default_tx_power_dbm = 14;
    return b;
}();
}  // namespace
const Board& get() { return kBoard; }
}  // namespace board
}  // namespace om

#endif  // OM_BOARD_RAK4631_SOLAR

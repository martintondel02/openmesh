
#include "heltec_tracker.h"

#if defined(OM_BOARD_HELTEC_TRACKER)

#include "openmesh/om_power.h"

namespace om {
namespace board {
namespace {
const Board kBoard = [] {
    Board b;
    b.id = "heltec_tracker";
    b.display_name = "Heltec Wireless Tracker";
    b.mcu = "ESP32-S3";
    b.radio_chip = "SX1262";
    b.default_region = "EU868";
    b.default_profile = "MEDIUM_FAST";
    b.default_role = DefaultRole::Tracker;
    b.radio.cs = 8;
    b.radio.irq = 14;    // DIO1
    b.radio.busy = 13;   // DIO2
    b.radio.reset = 12;
    b.radio.sck = 9;
    b.radio.miso = 11;
    b.radio.mosi = 10;
    b.radio.tcxo_voltage = 1.8f;
    b.radio.dio2_as_rf_switch = true;
    b.radio.lr1110 = false;
    b.gps.rx = 33;
    b.gps.tx = 34;
    b.gps.reset = 35;
    b.gps.pps = 36;
    b.gps.baud = 115200;
    b.gps.module = GnssModule::UC6580;
    b.power.battery_adc = 1;
    b.power.adc_multiplier = 4.9f * 1.045f;
    b.power.adc_ctrl = 2;
    b.power.ocv_table = kOcvGenericLiIon;
    b.power.ocv_count = (uint8_t)kOcvGenericCount;
    b.power.usb_detect = -1;
    b.ux.led = 18;
    b.ux.led_active_high = true;
    b.ux.button = 0;
    b.ux.has_screen = true;
    b.ux.tft_cs = 38;
    b.ux.tft_dc = 40;
    b.ux.tft_reset = 39;
    b.ux.tft_backlight = 21;
    b.flash_bytes = 8u * 1024u * 1024u;
    b.has_wifi = true;
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

#endif  // OM_BOARD_HELTEC_TRACKER

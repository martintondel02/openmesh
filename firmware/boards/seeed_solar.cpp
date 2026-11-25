
#include "seeed_solar.h"

#if defined(OM_BOARD_SEEED_SOLAR)

#include "openmesh/om_power.h"

namespace om {
namespace board {
namespace {
const Board kBoard = [] {
    Board b;
    b.id = "seeed_solar";
    b.display_name = "Seeed SenseCAP Solar Node P1";
    b.mcu = "nRF52840";
    b.radio_chip = "SX1262";
    b.default_region = "EU868";
    b.default_profile = "MEDIUM_FAST";
    b.default_role = DefaultRole::Repeater;
    b.radio.cs = 4;     // D4
    b.radio.irq = 1;    // D1, SX126X DIO1
    b.radio.busy = 3;   // D3
    b.radio.reset = 2;  // D2
    b.radio.rxen = 5;   // D5, external RF switch RX enable
    b.radio.txen = -1;
    b.radio.sck = 8;
    b.radio.miso = 9;
    b.radio.mosi = 10;
    b.radio.tcxo_voltage = 1.8f;
    b.radio.dio2_as_rf_switch = true;
    b.radio.lr1110 = false;
    b.gps.rx = 7;   // D7
    b.gps.tx = 6;   // D6
    b.gps.en = 18;  // D18 GNSS enable
    b.gps.reset = 17;
    b.gps.standby = 0;  // D0 GNSS wakeup
    b.gps.baud = 9600;
    b.gps.module = GnssModule::L76K;
    b.power.battery_adc = 16;       // D16 / P0.31 VBAT_ADC
    b.power.adc_multiplier = 3.3f;
    b.power.ocv_table = kOcvSeeedSolar;
    b.power.ocv_count = (uint8_t)kOcvSeeedSolarCount;
    b.power.charge_status = -1;
    b.power.usb_detect = -1;
    b.ux.led = 12;
    b.ux.led_active_high = true;
    b.ux.button = 13;
    b.ux.has_screen = false;
    b.flash_bytes = 2u * 1024u * 1024u;  // onboard QSPI P25Q16H (2 MiB)
    b.has_wifi = false;
    b.has_ethernet = false;
    b.has_ble = true;
    b.has_usb = true;
    b.has_solar = true;
    b.max_tx_power_dbm =
        paCapForBoard(b.id)->max_tx_power_dbm;
    return b;
}();
}  // namespace
const Board& get() { return kBoard; }
}  // namespace board
}  // namespace om

#endif  // OM_BOARD_SEEED_SOLAR

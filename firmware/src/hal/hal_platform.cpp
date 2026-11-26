// SPDX-License-Identifier: AGPL-3.0-or-later
// Platform-specific implementations: nRF52840 (Adafruit nRF52 core) and ESP32-S3 (Arduino).
#include "hal.h"

#if defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES)
#include <Arduino.h>
#include <nrf.h>
#include <nrf_gpio.h>
#include <nrfx_rtc.h>
#elif defined(ESP32)
#include <Arduino.h>
#include <esp_sleep.h>
#include <esp_system.h>
#include <driver/uart.h>
#include <driver/adc.h>
#else
#include <chrono>
#include <thread>
#endif

#include "openmesh/om_power.h"

namespace om {
namespace hal {

namespace {
const board::Board* g_board = nullptr;
uint32_t g_gps_baud = 9600;
uint64_t g_wall_ms = 0;
bool g_wall_valid = false;
}  // namespace

#if defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES)

uint64_t nowMs() { return (uint64_t)millis(); }

void randomBytes(void* ctx, uint8_t* out, size_t len) {
    (void)ctx;
    // NRF_RNG is enabled on demand for each byte; the Adafruit core also exposes
    // random() but that is a PRNG, so use the hardware peripheral directly.
    NRF_RNG->CONFIG = RNG_CONFIG_DERCEN_Enabled << RNG_CONFIG_DERCEN_Pos;
    NRF_RNG->TASKS_START = 1;
    size_t i = 0;
    while (i < len) {
        NRF_RNG->EVENTS_VALRDY = 0;
        while (NRF_RNG->EVENTS_VALRDY == 0) {
        }
        out[i++] = (uint8_t)NRF_RNG->VALUE;
    }
    NRF_RNG->TASKS_STOP = 1;
}

void sleepMs(uint32_t ms) {
    // The Adafruit core runs FreeRTOS and the SoftDevice owns the low-power clock domain; poking
    // RTC1/WFE directly from the scheduler loop can reset or wedge the radio. Yield to the RTOS;
    // the radio keeps its own state and callers only sleep between scheduler ticks.
    if (ms == 0) return;
    if (ms > 3600000u) ms = 3600000u;
    delay(ms);
}

void initUx(const board::Board& board) {
    g_board = &board;
    if (board.ux.led >= 0) {
        pinMode((uint32_t)board.ux.led, OUTPUT);
        setLed(false);
    }
    if (board.ux.button >= 0) pinMode((uint32_t)board.ux.button, INPUT_PULLDOWN);
}

void setLed(bool on) {
    if (g_board == nullptr || g_board->ux.led < 0) return;
    const bool level = g_board->ux.led_active_high ? on : !on;
    digitalWrite((uint32_t)g_board->ux.led, level ? HIGH : LOW);
}

bool buttonPressed() {
    if (g_board == nullptr || g_board->ux.button < 0) return false;
    return digitalRead((uint32_t)g_board->ux.button) == HIGH;
}

bool gpsBegin(const board::Board& board) {
    g_board = &board;
    if (board.gps.module == board::GnssModule::None) return false;
    if (board.gps.en >= 0) {
        pinMode((uint32_t)board.gps.en, OUTPUT);
        digitalWrite((uint32_t)board.gps.en, HIGH);
    }
    if (board.gps.reset >= 0) {
        pinMode((uint32_t)board.gps.reset, OUTPUT);
        digitalWrite((uint32_t)board.gps.reset, HIGH);
        delay(10);
        digitalWrite((uint32_t)board.gps.reset, LOW);
        delay(10);
        digitalWrite((uint32_t)board.gps.reset, HIGH);
        delay(100);
    }
    g_gps_baud = board.gps.baud;
    Serial1.begin(g_gps_baud);
    // Drain the UART so the NMEA parser starts on a sentence boundary.
    while (Serial1.available() > 0) (void)Serial1.read();
    return true;
}

int gpsRead(uint8_t* buf, size_t cap) {
    size_t n = 0;
    while (n < cap && Serial1.available() > 0) buf[n++] = (uint8_t)Serial1.read();
    return (int)n;
}

void gpsSetEnabled(bool enabled) {
    if (g_board == nullptr || g_board->gps.en < 0) return;
    digitalWrite((uint32_t)g_board->gps.en, enabled ? HIGH : LOW);
}

om::BatteryReading readBattery(const board::Board& board) {
    om::BatteryReading reading;
    if (board.power.battery_adc < 0) return reading;
    if (board.power.adc_ctrl >= 0) {
        pinMode((uint32_t)board.power.adc_ctrl, OUTPUT);
        digitalWrite((uint32_t)board.power.adc_ctrl, HIGH);
        delay(2);
    }
    int raw = 0;
    for (int i = 0; i < 8; ++i) raw += analogRead((uint32_t)board.power.battery_adc);
    raw /= 8;
    // 12-bit ADC over 3.0 V reference (AR_INTERNAL_3_0 on the vendor variants).
    const float mv = (float)raw * (3000.0f / 4095.0f) * board.power.adc_multiplier;
    reading.battery_mv = (uint16_t)(mv + 0.5f);
    reading.percent = om::batteryPercentFromMv(reading.battery_mv, board.power.ocv_table,
                                               board.power.ocv_count);
    if (board.power.usb_detect >= 0) {
        pinMode((uint32_t)board.power.usb_detect, INPUT);
        // T1000-E and the reference carriers pull USB-detect high when VBUS is present.
        reading.usb_present = digitalRead((uint32_t)board.power.usb_detect) == HIGH;
    }
    if (board.power.charge_status >= 0) {
        pinMode((uint32_t)board.power.charge_status, INPUT);
        reading.charging = digitalRead((uint32_t)board.power.charge_status) == LOW;
    }
    if (board.power.adc_ctrl >= 0) digitalWrite((uint32_t)board.power.adc_ctrl, LOW);
    reading.valid = reading.battery_mv > 1000;
    return reading;
}

uint64_t wallTimeMs() { return g_wall_valid ? g_wall_ms : 0; }

#elif defined(ESP32)

uint64_t nowMs() { return (uint64_t)millis(); }

void randomBytes(void* ctx, uint8_t* out, size_t len) {
    (void)ctx;
    for (size_t i = 0; i < len; ++i) out[i] = (uint8_t)(esp_random() >> 24);
}

void sleepMs(uint32_t ms) {
    // ESP32-S3 light sleep with a timer wake-up: the radio is re-armed by the caller after the
    // sleep, so only scheduler delays are passed here.
    if (ms == 0) return;
    if (ms > 3600000u) ms = 3600000u;
    esp_sleep_enable_timer_wakeup((uint64_t)ms * 1000ull);
    esp_light_sleep_start();
}

void initUx(const board::Board& board) {
    g_board = &board;
    if (board.ux.led >= 0) {
        pinMode(board.ux.led, OUTPUT);
        setLed(false);
    }
    if (board.ux.button >= 0) pinMode(board.ux.button, INPUT_PULLUP);
}

void setLed(bool on) {
    if (g_board == nullptr || g_board->ux.led < 0) return;
    const bool level = g_board->ux.led_active_high ? on : !on;
    digitalWrite(g_board->ux.led, level ? HIGH : LOW);
}

bool buttonPressed() {
    if (g_board == nullptr || g_board->ux.button < 0) return false;
    return digitalRead(g_board->ux.button) == LOW;
}

bool gpsBegin(const board::Board& board) {
    g_board = &board;
    if (board.gps.module == board::GnssModule::None) return false;
    // VEXT powers the GNSS rail on the Wireless Tracker.
    if (board.gps.en >= 0) {
        pinMode(board.gps.en, OUTPUT);
        digitalWrite(board.gps.en, HIGH);
    }
    g_gps_baud = board.gps.baud;
    Serial1.begin(g_gps_baud, SERIAL_8N1, board.gps.rx, board.gps.tx);
    while (Serial1.available() > 0) (void)Serial1.read();
    return true;
}

int gpsRead(uint8_t* buf, size_t cap) {
    size_t n = 0;
    while (n < cap && Serial1.available() > 0) buf[n++] = (uint8_t)Serial1.read();
    return (int)n;
}

void gpsSetEnabled(bool enabled) {
    if (g_board == nullptr || g_board->gps.en < 0) return;
    digitalWrite(g_board->gps.en, enabled ? HIGH : LOW);
}

om::BatteryReading readBattery(const board::Board& board) {
    om::BatteryReading reading;
    if (board.power.battery_adc < 0) return reading;
    if (board.power.adc_ctrl >= 0) {
        pinMode(board.power.adc_ctrl, OUTPUT);
        digitalWrite(board.power.adc_ctrl, HIGH);
        delay(2);
    }
    int raw = 0;
    for (int i = 0; i < 8; ++i) raw += analogRead(board.power.battery_adc);
    raw /= 8;
    // ESP32-S3 12-bit ADC with 2.5 dB attenuation; the board multiplier includes the divider.
    const float mv = (float)raw * (3.3f / 4095.0f) * board.power.adc_multiplier;
    reading.battery_mv = (uint16_t)(mv + 0.5f);
    reading.percent = om::batteryPercentFromMv(reading.battery_mv, board.power.ocv_table,
                                               board.power.ocv_count);
    if (board.power.adc_ctrl >= 0) digitalWrite(board.power.adc_ctrl, LOW);
    reading.valid = reading.battery_mv > 1000;
    return reading;
}

uint64_t wallTimeMs() { return g_wall_valid ? g_wall_ms : 0; }

#else
// Host fallback so the HAL can be smoke-built outside Arduino.
#include <chrono>
#include <thread>

uint64_t nowMs() {
    using namespace std::chrono;
    return (uint64_t)duration_cast<milliseconds>(steady_clock::now().time_since_epoch()).count();
}
void randomBytes(void* ctx, uint8_t* out, size_t len) {
    (void)ctx;
    static uint64_t state = 0x1234567;
    for (size_t i = 0; i < len; ++i) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out[i] = (uint8_t)(state >> 24);
    }
}
void sleepMs(uint32_t ms) { std::this_thread::sleep_for(std::chrono::milliseconds(ms)); }
void initUx(const board::Board& board) {
    g_board = &board;
    g_wall_valid = false;
    g_wall_ms = 0;
}
void setLed(bool) {}
bool buttonPressed() { return false; }
bool gpsBegin(const board::Board& board) {
    g_board = &board;
    g_gps_baud = board.gps.baud;
    return false;
}
int gpsRead(uint8_t*, size_t) { return 0; }
void gpsSetEnabled(bool) {}
om::BatteryReading readBattery(const board::Board&) { return om::BatteryReading{}; }
uint64_t wallTimeMs() { return g_wall_valid ? g_wall_ms : 0; }
#endif

}  // namespace hal
}  // namespace om

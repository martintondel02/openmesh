// SPDX-License-Identifier: AGPL-3.0-or-later
// Platform-specific implementations: nRF52840 (Adafruit nRF52 core) and ESP32-S3 (Arduino).
#include "hal.h"

#if defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES)
#include <Arduino.h>
#include <nrf.h>
#include <nrf_gpio.h>
#include <nrf_soc.h>
#include <nrfx_rtc.h>
#include <cstring>
#include "openmesh/om_sha256.h"
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
bool g_softdevice_enabled = false;

// SoftDevice RNG application API. Only valid while the SoftDevice is enabled: with it running the
// nRF52 RNG peripheral belongs to the SoftDevice (BLE uses it), so applications must not poke
// NRF_RNG directly. The pool is filled asynchronously, so poll for a bounded time.
bool fillFromSoftDeviceRng(uint8_t* out, size_t len) {
    size_t got = 0;
    for (uint32_t spins = 0; spins < 2000 && got < len; ++spins) {
        uint8_t available = 0;
        if (sd_rand_application_bytes_available_get(&available) != NRF_SUCCESS) return false;
        if (available == 0) {
            delay(1);
            continue;
        }
        size_t want = len - got;
        if (want > available) want = available;
        if (want > 255) want = 255;
        if (sd_rand_application_vector_get(out + got, (uint8_t)want) != NRF_SUCCESS) return false;
        got += want;
    }
    return got == len;
}

// Fallback when the SoftDevice RNG is unavailable: SHA-256 counter chain seeded from the device
// id and boot time. Not a substitute for true entropy, but never touches the SD-owned RNG and
// never returns a zero block (used only if the SD RNG pool does not fill).
void fillFromHashChain(uint8_t* out, size_t len) {
    static uint8_t state[32];
    static uint32_t counter = 0;
    static bool seeded = false;
    if (!seeded) {
        seeded = true;
        uint8_t seed[16];
        const uint32_t id0 = NRF_FICR->DEVICEID[0];
        const uint32_t id1 = NRF_FICR->DEVICEID[1];
        const uint64_t t = (uint64_t)millis();
        std::memcpy(seed, &id0, sizeof(id0));
        std::memcpy(seed + 4, &id1, sizeof(id1));
        std::memcpy(seed + 8, &t, sizeof(t));
        om_sha256(seed, sizeof(seed), state);
    }
    while (len > 0) {
        om_sha256_ctx ctx;
        om_sha256_init(&ctx);
        om_sha256_update(&ctx, state, sizeof(state));
        om_sha256_update(&ctx, &counter, sizeof(counter));
        om_sha256_final(&ctx, state);
        ++counter;
        const size_t n = len < sizeof(state) ? len : sizeof(state);
        std::memcpy(out, state, n);
        out += n;
        len -= n;
    }
}
}  // namespace

void setSoftDeviceEnabled(bool enabled) { g_softdevice_enabled = enabled; }

#if defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES)

uint64_t nowMs() { return (uint64_t)millis(); }

void randomBytes(void* ctx, uint8_t* out, size_t len) {
    (void)ctx;
    if (g_softdevice_enabled) {
        if (fillFromSoftDeviceRng(out, len)) return;
        fillFromHashChain(out, len);
        return;
    }
    // No SoftDevice: NRF_RNG can be driven directly. The Adafruit core also exposes
    // random() but that is a PRNG, so use the hardware peripheral.
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
    // Yield the CPU to FreeRTOS for the scheduler idle window instead of busy-spinning. delay() is
    // the Adafruit core's vTaskDelay wrapper (and flushes TinyUSB CDC first), so it is safe next to
    // the SoftDevice and lets the nRF52 FreeRTOS port enter its tickless-idle low-power state
    // (configUSE_TICKLESS_IDLE = 1). Direct RTC1/WFE register access is still wrong here: it fights
    // the FreeRTOS/SoftDevice clock domain and stops millis(). Deep sleep (radio off between
    // windows) remains a follow-up; this is the light-sleep path the power table assumes.
    if (ms > 0) delay(ms);
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

void setSoftDeviceEnabled(bool) {}

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
void setSoftDeviceEnabled(bool) {}

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

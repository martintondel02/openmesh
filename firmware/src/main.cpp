// SPDX-License-Identifier: AGPL-3.0-or-later
// OpenMesh firmware entry point: one codebase, four boards, standalone mesh node + companion
// radio. The protocol itself lives in lib/openmesh (Arduino-free); this file wires it to the
// board HAL: RadioLib, LittleFS/InternalFileSystem, USB CDC + BLE NUS, GNSS UART and ADC.
#include <Arduino.h>

#include <cstdio>
#include <cstring>

#include "board_select.h"
#include "hal/hal.h"
#include "hal/hal_companion.h"
#include "hal/hal_radio.h"
#include "hal/hal_storage.h"
#include "openmesh/om_companion.h"
#include "openmesh/om_engine.h"
#include "openmesh/om_gps.h"
#include "openmesh/om_identity.h"
#include "openmesh/om_power.h"
#include "openmesh/om_region.h"
#include "openmesh/om_sha256.h"

#ifndef OM_FW_VERSION
#define OM_FW_VERSION "0.5.0"
#endif
#ifndef OM_DEFAULT_CHANNEL_NAME
#define OM_DEFAULT_CHANNEL_NAME "openmesh-public"
#endif
#ifndef OM_ENABLE_BLE
#define OM_ENABLE_BLE 0
#endif
#ifndef OM_ANNOUNCE_TTL_SECONDS
#define OM_ANNOUNCE_TTL_SECONDS 1800
#endif

namespace {

using namespace om;

constexpr char kSeedFile[] = "om_seed.bin";
constexpr char kStoreFile[] = "om_store.bin";
constexpr uint32_t kCompanionTimeoutMs = 90000;
constexpr uint32_t kBatteryIntervalMs = 30000;
constexpr uint32_t kStoreSaveIntervalMs = 300000;

hal::Radio g_radio;
hal::HalStorage g_storage;
hal::UsbTransport g_usb;
hal::BleTransport g_ble;
Engine g_engine;
NmeaParser g_gps;
SlipDecoder g_usb_slip;
SlipDecoder g_ble_slip;
Identity g_identity;
const board::Board* g_board = nullptr;

uint8_t g_store_buffer[StoreQueue::kSerializedSize];

// Stack budget: the Adafruit nRF52 core runs setup()/loop() on a FreeRTOS task created with
// LOOP_STACK_SZ = 1024 words = 4 KiB (framework cores/nRF5/main.cpp; xTaskCreate sizes are words).
// The old companion path put a 2048-byte chunk, a 4096-byte SLIP buffer and a 1608-byte
// CompanionMsg (~7.7 KiB worst case) on that stack, so the first companion reply overflowed it and
// hung the node (reproduced on T1000-E hardware 2026-11-27). The firmware main loop is
// single-threaded; these file-scope scratch areas replace the on-stack copies.
CompanionMsg g_rx_msg;     // decoded inbound companion message
CompanionMsg g_tx_msg;     // outgoing reply, built while g_rx_msg is still being read
CompanionMsg g_event_msg;  // outgoing event; may be built from inside a reply handler
uint8_t g_companion_rx[512];
uint8_t g_companion_chunk[2048];
uint8_t g_companion_slip[4096];

uint64_t g_last_usb_ms = 0;
uint64_t g_last_ble_ms = 0;
uint64_t g_last_battery_ms = 0;
uint64_t g_last_store_save_ms = 0;
uint64_t g_last_led_ms = 0;
uint64_t g_last_gps_ms = 0;
bool g_gps_emitted = false;
bool g_led_on = false;
bool g_button_handled = false;
uint64_t g_button_down_ms = 0;

uint32_t deviceUniqueWord(uint32_t index) {
#if defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES)
    if (index == 0) return NRF_FICR->DEVICEID[0];
    if (index == 1) return NRF_FICR->DEVICEID[1];
    return NRF_FICR->DEVICEADDR[index & 1];
#elif defined(ESP32)
    const uint64_t mac = ESP.getEfuseMac();
    return index == 0 ? (uint32_t)mac : (uint32_t)(mac >> 32);
#else
    return 0x4F4D3031u ^ index;
#endif
}

// Device key for seeding encryption at rest: derived from the MCU unique id. This protects
// against casual flash extraction only; see firmware/README.md "Key storage".
void deviceKey(uint8_t out[32]) {
    uint8_t material[16];
    for (int i = 0; i < 4; ++i) writeU32Be(material + i * 4, deviceUniqueWord((uint32_t)i));
    om_sha256(material, sizeof(material), out);
}

bool loadOrCreateIdentity() {
    uint8_t key[32];
    deviceKey(key);
    uint8_t sealed[kSealedSeedLen];
    size_t sealed_len = 0;
    if (g_storage.load(kSeedFile, sealed, sizeof(sealed), sealed_len) &&
        sealed_len == kSealedSeedLen) {
        uint8_t seed[32];
        if (openSeedAtRest(sealed, sealed_len, key, seed)) {
            const bool ok = Identity::fromSeed(seed, g_identity);
            secureWipe(seed, sizeof(seed));
            if (ok) return true;
        }
    }
    if (!Identity::generate(hal::randomBytes, nullptr, g_identity)) return false;
    if (sealSeedAtRest(g_identity.seed, key, hal::randomBytes, nullptr, sealed)) {
        g_storage.save(kSeedFile, sealed, sizeof(sealed));
    }
    return true;
}

void defaultChannelPsk(const char* name, uint8_t psk[32]) {
    // Well-known default channel for out-of-the-box mesh testing. Documented in firmware/README
    // and docs/hardware/boards.md: it is a public channel, not a secret; users should add their
    // own channel PSK via the companion ConfigSet API.
    char material[96];
    const char domain[] = "openmesh/v1/default-channel";
    const size_t domain_len = sizeof(domain) - 1;
    std::memcpy(material, domain, domain_len);
    const size_t name_len = std::strlen(name);
    std::memcpy(material + domain_len, name, name_len);
    uint8_t digest[32];
    om_sha256(material, domain_len + name_len, digest);
    std::memcpy(psk, digest, 32);
}

void configureEngine() {
    EngineConfig config;
    config.cls = NodeClass::Client;
    switch (g_board->default_role) {
        case board::DefaultRole::Tracker: config.cls = NodeClass::Tracker; break;
        case board::DefaultRole::Repeater: config.cls = NodeClass::Repeater; break;
        case board::DefaultRole::Gateway: config.cls = NodeClass::Gateway; break;
        case board::DefaultRole::Client: config.cls = NodeClass::Client; break;
    }
    config.store_role = g_board->default_role == board::DefaultRole::Repeater ||
                        g_board->default_role == board::DefaultRole::Gateway;
    config.relay_role = true;
    config.accept_channels = true;
    config.position_sharing = g_board->gps.module != board::GnssModule::None;
    config.default_ttl = kDefaultTtl;
    config.profile = profileByName(g_board->default_profile);
    if (config.profile == nullptr) config.profile = &kMediumFast;
    config.region = regionByName(g_board->default_region);
    if (config.region == nullptr) config.region = &defaultRegion();
    // A board default profile may not be legal in its default region (e.g. a 500 kHz preset in
    // EU868); fall back rather than transmit an illegal bandwidth.
    if (!profileFitsRegion(*config.profile, *config.region)) {
        config.profile = (config.region == &kRegionEu433) ? &kLongFast : &kMediumFast;
    }
    // v0.5.0: defaults are the legal maxima, limited by the board PA cap.
    config.hardware_max_power_dbm = g_board->max_tx_power_dbm;
    config.tx_power_dbm = effectivePowerDbm(*config.region, config.hardware_max_power_dbm, -1,
                                            false);
    config.announce_interval_ms =
        config.store_role ? (uint32_t)kAnnounceIntervalRelayMs : (uint32_t)kAnnounceIntervalClientMs;
    config.telemetry_interval_ms = 900000;
    config.caps = 0;
    if (g_board->has_usb) config.caps |= kCapUsb;
    if (g_board->has_ble && OM_ENABLE_BLE) config.caps |= kCapBle;
    if (g_board->gps.module != board::GnssModule::None) config.caps |= kCapGps;
    if (g_board->has_wifi) config.caps |= kCapWifi;
    if (g_board->has_ethernet) config.caps |= kCapEthernet;
    if (g_board->has_solar) config.caps |= kCapSolar;
    if (g_board->ux.has_screen) config.caps |= kCapScreen;
    config.name_len = (uint8_t)std::snprintf(config.name, sizeof(config.name), "%s-%02x%02x",
                                             g_board->id, g_identity.nodeId().b[0],
                                             g_identity.nodeId().b[1]);
    g_engine.begin(g_identity, config, hal::randomBytes, nullptr, &g_storage);
    uint8_t channel_psk[32];
    defaultChannelPsk(OM_DEFAULT_CHANNEL_NAME, channel_psk);
    g_engine.setChannel(OM_DEFAULT_CHANNEL_NAME, channel_psk);
    size_t store_len = 0;
    if (g_storage.load(kStoreFile, g_store_buffer, sizeof(g_store_buffer), store_len)) {
        g_engine.loadStoreBuffer(g_store_buffer, store_len, millis());
    }
    g_engine.start(millis());
}

void companionSend(const CompanionMsg& msg) {
    const size_t chunk_len =
        encodeCompanionChunk(msg, g_companion_chunk, sizeof(g_companion_chunk));
    if (chunk_len == 0) return;
    const size_t slip_len = slipEncode(g_companion_chunk, chunk_len, g_companion_slip,
                                       sizeof(g_companion_slip));
    if (slip_len == 0) return;
    // Only write to links that recently carried a companion Hello; a USB CDC write with no
    // host attached can block.
    if (millis() - g_last_usb_ms < kCompanionTimeoutMs) g_usb.write(g_companion_slip, slip_len);
    if (g_ble.enabled() && g_ble.connected() && millis() - g_last_ble_ms < kCompanionTimeoutMs) {
        g_ble.write(g_companion_slip, slip_len);
    }
}

void sendEvent(const char* kind, const char* detail) {
    CompanionMsg& msg = g_event_msg;
    msg.type = CompanionType::Event;
    std::snprintf(msg.kind, sizeof(msg.kind), "%s", kind);
    std::snprintf(msg.detail, sizeof(msg.detail), "%s", detail);
    companionSend(msg);
}

void drainEvents() {
    while (g_engine.events().size() > 0) {
        const EngineEvent& event = g_engine.events().at(0);
        switch (event.type) {
            case EventType::RxFrame: {
                CompanionMsg& msg = g_tx_msg;
                msg.type = CompanionType::Rx;
                const size_t len = event.frame_len;
                std::memcpy(msg.frame, event.frame, len);
                msg.frame_len = (uint16_t)len;
                msg.rssi = event.rssi;
                msg.snr = event.snr;
                std::snprintf(msg.link, sizeof(msg.link), event.link == Link::Ble ? "ble" : "lora");
                msg.at_ms = millis();
                companionSend(msg);
                break;
            }
            case EventType::LowBattery:
                sendEvent("low_battery", event.detail);
                break;
            case EventType::StoreDelivered:
                sendEvent("store_deliver", "envelope replayed");
                break;
            case EventType::Error:
                sendEvent("error", event.detail);
                break;
            case EventType::Announce:
                sendEvent("announce_rx", event.announce.name);
                break;
            case EventType::AppMessage:
                break;  // the raw Rx frame already carried this message
            case EventType::GpsFix:
                sendEvent("gps_fix", "position acquired");
                break;
        }
        g_engine.events().popFront();
    }
}

void drainOutbox() {
    uint8_t encoded[kMtu];
    while (g_engine.outbox().size() > 0) {
        const OutFrame& out = g_engine.outbox().at(0);
        const size_t len = out.frame.encode(encoded, sizeof(encoded));
        if (len > 0) {
            if (!g_radio.transmit(encoded, len)) {
                ++g_engine.mutableStats().malformed;  // radio TX failure counter reuse
            } else {
                g_radio.startReceive();
            }
        }
        g_engine.outbox().popFront();
    }
}

void handleCompanionMessage(const CompanionMsg& msg) {
    switch (msg.type) {
        case CompanionType::Hello: {
            g_engine.fillHelloAck(g_tx_msg);
            companionSend(g_tx_msg);
            sendEvent("boot", OM_FW_VERSION);
            break;
        }
        case CompanionType::ConfigGet: {
            g_engine.fillConfig(g_tx_msg);
            companionSend(g_tx_msg);
            break;
        }
        case CompanionType::ConfigSet: {
            if (!g_engine.applyConfig(msg.key, msg.value)) {
                const char* detail = "unknown_config_key";
                if (std::strcmp(msg.key, "radio.profile") == 0) {
                    detail = "profile_bw_exceeds_region_bw";
                } else if (std::strcmp(msg.key, "radio.frequency") == 0) {
                    detail = "frequency_out_of_range";
                } else if (std::strcmp(msg.key, "radio.duty_override") == 0) {
                    detail = "duty_override_out_of_range";
                } else if (std::strcmp(msg.key, "radio.region") == 0) {
                    detail = "unknown_region";
                }
                sendEvent("error", detail);
            } else {
                g_engine.fillConfig(g_tx_msg);
                companionSend(g_tx_msg);
            }
            break;
        }
        case CompanionType::Tx:
            if (msg.frame_len > kMtu) {
                sendEvent("error", "frame_too_large");
            } else if (!g_engine.originateFromApp(msg.frame, msg.frame_len, millis())) {
                sendEvent("error", "tx_rejected");
            }
            break;
        case CompanionType::StatsGet: {
            g_engine.fillStats(g_tx_msg);
            companionSend(g_tx_msg);
            break;
        }
        case CompanionType::Ping: {
            g_tx_msg.type = CompanionType::Pong;
            g_tx_msg.nonce = msg.nonce;
            companionSend(g_tx_msg);
            break;
        }
        case CompanionType::Reset:
            sendEvent("boot", "reset requested");
            delay(200);
#if defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES)
            NVIC_SystemReset();
#elif defined(ESP32)
            ESP.restart();
#endif
            break;
        default:
            break;
    }
}

void pollCompanion() {
    int n = g_usb.read(g_companion_rx, sizeof(g_companion_rx));
    for (int i = 0; i < n; ++i) {
        if (g_usb_slip.feed(g_companion_rx[i]) && g_usb_slip.frameValid()) {
            if (decodeCompanionChunk(g_usb_slip.chunk(), g_usb_slip.chunkLen(), g_rx_msg)) {
                g_last_usb_ms = millis();
                handleCompanionMessage(g_rx_msg);
            }
        }
    }
    if (g_ble.enabled()) {
        n = g_ble.read(g_companion_rx, sizeof(g_companion_rx));
        for (int i = 0; i < n; ++i) {
            if (g_ble_slip.feed(g_companion_rx[i]) && g_ble_slip.frameValid()) {
                if (decodeCompanionChunk(g_ble_slip.chunk(), g_ble_slip.chunkLen(), g_rx_msg)) {
                    g_last_ble_ms = millis();
                    handleCompanionMessage(g_rx_msg);
                }
            }
        }
    }
}

void pollRadio() {
    uint8_t buffer[kMtu];
    int16_t rssi = 0;
    int16_t snr = 0;
    const int len = g_radio.receive(buffer, sizeof(buffer), rssi, snr);
    if (len > 0) {
        g_engine.onBytes(buffer, (size_t)len, Link::Lora, rssi, snr, millis());
        if (!g_led_on) {
            g_led_on = true;
            hal::setLed(true);
            g_last_led_ms = millis();
        }
    } else if (len < 0) {
        sendEvent("radio_error", "receive");
    }
}

void pollGps() {
    if (g_board->gps.module == board::GnssModule::None) return;
    uint8_t buffer[128];
    const int n = hal::gpsRead(buffer, sizeof(buffer));
    for (int i = 0; i < n; ++i) {
        g_gps.feed((char)buffer[i], millis());
    }
    if (g_gps.fix().valid) {
        if (!g_gps_emitted) {
            g_gps_emitted = true;
            EngineEvent event;
            event.type = EventType::GpsFix;
            g_engine.events().push(event);
        }
        // Emit at most one position per five minutes; the engine also rate-limits.
        if (millis() - g_last_gps_ms >= 60000u) {
            g_last_gps_ms = millis();
            g_engine.setGpsFix(g_gps.fix(), millis());
        }
    }
}

void pollButton() {
    if (g_board->ux.button < 0) return;
    const bool pressed = hal::buttonPressed();
    if (pressed && g_button_down_ms == 0) g_button_down_ms = millis();
    if (pressed && !g_button_handled && millis() - g_button_down_ms > 3000u) {
        g_button_handled = true;
        Frame announce;
        if (g_engine.buildAnnounce(millis(), announce)) sendEvent("announce_tx", "button");
    }
    if (!pressed) {
        g_button_down_ms = 0;
        g_button_handled = false;
    }
}

// Tracks the last radio configuration handed to the HAL so region/profile/power/frequency
// ConfigSet changes are applied without a reboot, and logs the regulatory warning once per
// activation.
struct AppliedRadioConfig {
    const om::LoraProfile* profile = nullptr;
    const om::Region* region = nullptr;
    int8_t power_dbm = 0;
    uint32_t frequency_khz = 0;
    bool valid = false;
};
AppliedRadioConfig g_applied_radio;
bool g_override_warning_logged = false;

void applyRadioConfigIfChanged() {
    const om::EngineConfig& config = g_engine.config();
    const int8_t power = config.tx_power_dbm;
    const uint32_t freq = config.frequency_override_khz;
    const bool changed = !g_applied_radio.valid ||
                         g_applied_radio.profile != config.profile ||
                         g_applied_radio.region != config.region ||
                         g_applied_radio.power_dbm != power ||
                         g_applied_radio.frequency_khz != freq;
    if (changed) {
        const bool ok = g_radio.reconfigure(*config.profile, *config.region, power, freq);
        if (!ok) sendEvent("radio_error", "reconfigure");
        g_applied_radio.profile = config.profile;
        g_applied_radio.region = config.region;
        g_applied_radio.power_dbm = power;
        g_applied_radio.frequency_khz = freq;
        g_applied_radio.valid = true;
    }
    if (g_engine.overrideActive() && !g_override_warning_logged) {
        g_override_warning_logged = true;
        Serial.println();
        Serial.println("WARNING: regulatory override active - may violate local law.");
        Serial.println(om::kRegulatoryOverrideWarning);
        sendEvent("error", "regulatory_override_active");
    } else if (!g_engine.overrideActive()) {
        g_override_warning_logged = false;
    }
}

}  // namespace

// First-article startup tracing: this runs before setup() (static/global constructors). It uses
// direct GPIO if available so it can report that the application image itself started.
#if defined(OM_BOOT_TRACE)
extern "C" void om_boot_trace_ctor(void) __attribute__((constructor(101)));
extern "C" void om_boot_trace_ctor(void) {
    pinMode(LED_BUILTIN, OUTPUT);
    for (int i = 0; i < 3; ++i) {
        digitalWrite(LED_BUILTIN, HIGH);
        for (volatile int j = 0; j < 400000; ++j) {
        }
        digitalWrite(LED_BUILTIN, LOW);
        for (volatile int j = 0; j < 400000; ++j) {
        }
    }
}
#endif

void setup() {
    // Trace marker: LED solid before USB/serial so a silent crash after setup entry is visible too.
#if defined(OM_BOOT_TRACE)
    pinMode(LED_BUILTIN, OUTPUT);
    digitalWrite(LED_BUILTIN, HIGH);
#endif
    // USB/serial first: the init markers below must be visible even if a later step fails.
    Serial.begin(115200);
    delay(250);
    Serial.println();
    Serial.println("[om] setup: begin"); delay(150);
#if defined(OM_EARLY_STOP)
    Serial.println("[om] EARLY STOP: USB alive"); Serial.flush();
    for (;;) { digitalWrite(LED_BUILTIN, (millis() / 500) % 2 ? HIGH : LOW); delay(1000); }
#endif
    g_board = &board::get();
    Serial.printf("[om] board=%s\n", g_board->id); Serial.flush(); delay(150);
#if defined(OM_ENABLE_SOFTDEVICE) && (OM_ENABLE_SOFTDEVICE)
    // BLE-compiled-out images still enable the SoftDevice: InternalFS flash writes use sd_flash_*
    // SVCs that need it. Must happen before g_storage.begin()/identity load. The TinyUSB <-> SD
    // hand-off this triggers runs once here, before the companion link is used.
    {
        const bool sd_ok = hal::beginSoftDevice();
        Serial.printf("[om] softdevice=%s (BLE closed)\n", sd_ok ? "ok" : "ERROR");
        Serial.flush();
        delay(150);
    }
#endif
#if OM_ENABLE_BLE
    // Bring the SoftDevice up before the rest of the peripherals. Two reasons: Adafruit's
    // InternalFS writes go through SoftDevice flash SVCs and are only valid once the SoftDevice is
    // enabled, and the TinyUSB/SoftDevice hand-off (usb_softdevice_pre_enable/post_enable) is best
    // done before the companion link is used. This does not remove the one-time USB
    // re-enumeration the hand-off causes; BLE-on USB stability is still under investigation.
    {
        char device_name[24];
        std::snprintf(device_name, sizeof(device_name), "%s", g_board->display_name);
        const bool ble_ok = g_ble.begin(device_name);
        Serial.printf("[om] ble=%s\n", ble_ok ? "ok" : "ERROR"); Serial.flush(); delay(150);
    }
#endif
    hal::initUx(*g_board);
    hal::setLed(true);
    Serial.println("[om] ux"); Serial.flush(); delay(150);
    g_storage.begin();
    Serial.println("[om] storage"); Serial.flush(); delay(150);
    if (!loadOrCreateIdentity()) {
        sendEvent("error", "identity");
        Serial.println("[om] identity: ERROR");
    } else {
        Serial.println("[om] identity"); Serial.flush(); delay(150);
    }
    configureEngine();
    Serial.println("[om] engine"); Serial.flush(); delay(150);
    if (g_board->gps.module != board::GnssModule::None) {
        hal::gpsBegin(*g_board);
        Serial.println("[om] gps"); Serial.flush(); delay(150);
    }
    const bool radio_ok = g_radio.begin(*g_board, *g_engine.config().profile,
                                        *g_engine.config().region,
                                        g_engine.config().tx_power_dbm,
                                        g_engine.config().frequency_override_khz);
    Serial.printf("[om] radio=%s\n", radio_ok ? "ok" : "ERROR"); Serial.flush(); delay(150);
    g_usb.begin();
    Serial.println("[om] usb"); Serial.flush(); delay(250);
    hal::setLed(false);
    g_applied_radio.profile = g_engine.config().profile;
    g_applied_radio.region = g_engine.config().region;
    g_applied_radio.power_dbm = g_engine.config().tx_power_dbm;
    g_applied_radio.frequency_khz = g_engine.config().frequency_override_khz;
    g_applied_radio.valid = radio_ok;
    if (g_engine.overrideActive()) {
        g_override_warning_logged = true;
        Serial.println();
        Serial.println("WARNING: regulatory override restored from flash - may violate local law.");
        Serial.println(om::kRegulatoryOverrideWarning);
    }
    g_last_store_save_ms = millis();
    g_last_battery_ms = 0;
    if (!radio_ok) {
        sendEvent("radio_error", "init");
    }
    char node_hex[17];
    g_engine.identity().nodeId().toHex(node_hex);
    Serial.printf("OpenMesh %s on %s, node %s, radio %s\n", OM_FW_VERSION, g_board->id, node_hex,
                  radio_ok ? "ok" : "ERROR");
}

void loop() {
    const uint64_t now = millis();
    pollRadio();
    pollCompanion();
    applyRadioConfigIfChanged();
    g_engine.tick(now);
    drainEvents();
    drainOutbox();

    if (now - g_last_battery_ms >= kBatteryIntervalMs) {
        g_last_battery_ms = now;
        const BatteryReading reading = hal::readBattery(*g_board);
        g_engine.setBattery(reading, now);
        const uint64_t wall = hal::wallTimeMs();
        if (wall != 0) g_engine.setWallTime(wall, now);
    }
    pollGps();
    pollButton();

    if (now - g_last_store_save_ms >= kStoreSaveIntervalMs) {
        g_last_store_save_ms = now;
        if (g_engine.store().dirty()) g_engine.saveStore(now);
    }
    // Heartbeat LED: 20 ms flash after traffic, plus a slow blink while idle.
    if (g_led_on && now - g_last_led_ms > 20) {
        g_led_on = false;
        hal::setLed(false);
    }
    if (!g_led_on && (now % 5000) < 2) hal::setLed(true);

    // Trackers and battery nodes sleep between scheduler ticks; repeaters stay responsive.
    if (g_board->default_role == board::DefaultRole::Tracker) {
        hal::sleepMs(20);
    } else {
        hal::sleepMs(4);
    }
}

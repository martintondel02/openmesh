// SPDX-License-Identifier: AGPL-3.0-or-later
#include "hal_companion.h"

#include "hal.h"

#if defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES) || defined(ESP32)
#include <Arduino.h>
#endif

#if (defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES)) && \
    ((defined(OM_ENABLE_BLE) && (OM_ENABLE_BLE)) || \
     (defined(OM_ENABLE_SOFTDEVICE) && (OM_ENABLE_SOFTDEVICE)))
#include <bluefruit.h>
#elif defined(ESP32) && defined(OM_ENABLE_BLE)
#include <NimBLEDevice.h>
#endif

namespace om {
namespace hal {

// --- USB CDC -------------------------------------------------------------------------------
#if defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES) || defined(ESP32)
bool UsbTransport::begin() {
    // USB CDC is already initialised by the Arduino core: the loop task calls
    // TinyUSB_Device_Init() before setup(), and setup() calls Serial.begin(115200) for the
    // bring-up log markers. Adafruit_USBD_CDC::begin() returns early when it is already started, so
    // the old second call here was a harmless no-op; drop it anyway so the transport state is
    // explicit and does not depend on that implementation detail.
    begun_ = true;
    return true;
}

int UsbTransport::read(uint8_t* buffer, size_t capacity) {
    if (!begun_) return 0;
    size_t n = 0;
    while (n < capacity && Serial.available() > 0) buffer[n++] = (uint8_t)Serial.read();
    return (int)n;
}

size_t UsbTransport::write(const uint8_t* data, size_t len) {
    if (!begun_) return 0;
    // A CDC write can be short when the TX buffer is full; never truncate a companion frame.
    size_t sent = 0;
    const uint32_t start = millis();
    while (sent < len) {
        const size_t n = (size_t)Serial.write(data + sent, len - sent);
        sent += n;
        if (sent >= len) break;
        if (millis() - start > 1000u) break;  // never wedge the main loop
        delay(1);
    }
    return sent;
}

bool UsbTransport::connected() const { return begun_; }
#else
bool UsbTransport::begin() {
    begun_ = true;
    return false;
}
int UsbTransport::read(uint8_t*, size_t) { return 0; }
size_t UsbTransport::write(const uint8_t*, size_t) { return 0; }
bool UsbTransport::connected() const { return false; }
#endif

// --- BLE NUS: nRF52 (Adafruit Bluefruit) ----------------------------------------------------
#if (defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES)) && defined(OM_ENABLE_BLE)
namespace {
// The default BLEUart RX FIFO is 256 bytes: an app that writes a whole companion frame
// (SLIP + OM chunk, up to ~300 B) in a burst could overflow it while the firmware was busy
// queueing a reply, silently losing the frame. 1 KiB keeps a full frame plus slack.
BLEUart g_bleuart(1024);
}
bool BleTransport::begin(const char* device_name) {
    // The SoftDevice may already be enabled (BLE-closed "SD for flash" images, or an early-start
    // bring-up); only initialise it once and then just configure/advertise.
    const bool sd_ok = softDeviceEnabled() ? true : Bluefruit.begin();
    setSoftDeviceEnabled(sd_ok);
    Bluefruit.setName(device_name);
    Bluefruit.setTxPower(4);
    Bluefruit.Periph.setConnectCallback(nullptr);
    Bluefruit.Periph.setConnInterval(9, 12);
    // BLEUart::begin() returns err_t (0 = ERROR_NONE), not bool.
    const err_t uart_err = g_bleuart.begin();
    Bluefruit.Advertising.addFlags(BLE_GAP_ADV_FLAGS_LE_ONLY_GENERAL_DISC_MODE);
    Bluefruit.Advertising.addTxPower();
    Bluefruit.Advertising.addService(g_bleuart);
    // The 128-bit service UUID already fills the advertising packet; put the device name in the
    // scan response so phone/Linux scanners can show "Seeed SenseCAP T1000-E".
    Bluefruit.ScanResponse.addName();
    Bluefruit.Advertising.restartOnDisconnect(true);
    Bluefruit.Advertising.setInterval(32, 244);
    Bluefruit.Advertising.setFastTimeout(30);
    const bool adv_ok = Bluefruit.Advertising.start(0);
    uint8_t mac[6] = {0};
    Bluefruit.getAddr(mac);
    Serial.printf("[om] ble init: sd=%d uart_err=%d adv=%d mac=%02X:%02X:%02X:%02X:%02X:%02X name=%s\n",
                  sd_ok ? 1 : 0, (int)uart_err, adv_ok ? 1 : 0,
                  mac[5], mac[4], mac[3], mac[2], mac[1], mac[0], device_name);
    enabled_ = sd_ok && (uart_err == ERROR_NONE) && adv_ok;
    return enabled_;
}

int BleTransport::read(uint8_t* buffer, size_t capacity) {
    if (!enabled_) return 0;
    size_t n = 0;
    while (n < capacity && g_bleuart.available() > 0) buffer[n++] = (uint8_t)g_bleuart.read();
    return (int)n;
}

size_t BleTransport::write(const uint8_t* data, size_t len) {
    if (!enabled_) return 0;
    // Adafruit's BLEUart defaults to unbuffered TX: BLEUart::write() sends exactly ONE notification
    // (<= ATT MTU - 3 bytes) but reports the full length as written. A single call therefore
    // silently truncated larger companion replies (HelloAck at the default 23-byte MTU), which
    // broke the companion handshake. Send the frame in MTU-sized chunks and retry a chunk that the
    // link-layer TX queue cannot take yet. The timeout keeps the main loop from ever wedging.
    const uint16_t conn_hdl = Bluefruit.connHandle();
    BLEConnection* conn = Bluefruit.Connection(conn_hdl);
    size_t chunk = 20;
    if (conn != nullptr) {
        const uint16_t mtu = conn->getMtu();
        if (mtu > 3) chunk = (size_t)mtu - 3;
    }
    if (chunk > 244u) chunk = 244u;  // largest notification the TX characteristic is configured for

    size_t sent = 0;
    const uint32_t start = millis();
    while (sent < len) {
        size_t n = len - sent;
        if (n > chunk) n = chunk;
        const size_t w = (size_t)g_bleuart.write(data + sent, n);
        if (w == 0) {
            if (millis() - start > 500u) break;  // link stalled: drop the rest, never hang the loop
            delay(2);
            continue;
        }
        sent += n;
    }
    return sent;
}

bool BleTransport::connected() const { return Bluefruit.connected() > 0; }

// --- BLE NUS: ESP32-S3 (NimBLE-Arduino 1.4.x) -----------------------------------------------
#elif defined(ESP32) && defined(OM_ENABLE_BLE)
namespace {
// Nordic UART Service UUIDs.
NimBLEUUID g_nus_service("6E400001-B5A3-F393-E0A9-E50E24DCCA9E");
NimBLEUUID g_nus_rx("6E400002-B5A3-F393-E0A9-E50E24DCCA9E");
NimBLEUUID g_nus_tx("6E400003-B5A3-F393-E0A9-E50E24DCCA9E");
NimBLECharacteristic* g_tx = nullptr;

constexpr size_t kRingSize = 4096;
volatile size_t g_ring_head = 0;
volatile size_t g_ring_tail = 0;
uint8_t g_ring[kRingSize];

void ringPush(const uint8_t* data, size_t len) {
    for (size_t i = 0; i < len; ++i) {
        const size_t next = (g_ring_head + 1) % kRingSize;
        if (next == g_ring_tail) return;  // drop on overflow rather than corrupt framing
        g_ring[g_ring_head] = data[i];
        g_ring_head = next;
    }
}

class RxCallbacks : public NimBLECharacteristicCallbacks {
    void onWrite(NimBLECharacteristic* characteristic) override {
        const std::string value = characteristic->getValue();
        if (!value.empty()) ringPush((const uint8_t*)value.data(), value.size());
    }
};

class ServerCallbacks : public NimBLEServerCallbacks {
    void onConnect(NimBLEServer* server) override {
        server->updateConnParams(12, 24, 0, 400);
    }
};
}  // namespace

bool BleTransport::begin(const char* device_name) {
    NimBLEDevice::init(device_name);
    NimBLEDevice::setMTU(247);
    NimBLEServer* server = NimBLEDevice::createServer();
    server->setCallbacks(new ServerCallbacks());
    NimBLEService* service = server->createService(g_nus_service);
    NimBLECharacteristic* rx = service->createCharacteristic(
        g_nus_rx, NIMBLE_PROPERTY::WRITE | NIMBLE_PROPERTY::WRITE_NR);
    rx->setCallbacks(new RxCallbacks());
    g_tx = service->createCharacteristic(g_nus_tx, NIMBLE_PROPERTY::NOTIFY);
    service->start();
    NimBLEAdvertising* advertising = NimBLEDevice::getAdvertising();
    advertising->addServiceUUID(g_nus_service);
    advertising->setScanResponse(true);
    advertising->start();
    enabled_ = true;
    return true;
}

int BleTransport::read(uint8_t* buffer, size_t capacity) {
    if (!enabled_) return 0;
    size_t n = 0;
    while (n < capacity && g_ring_tail != g_ring_head) {
        buffer[n++] = g_ring[g_ring_tail];
        g_ring_tail = (g_ring_tail + 1) % kRingSize;
    }
    return (int)n;
}

size_t BleTransport::write(const uint8_t* data, size_t len) {
    if (!enabled_ || g_tx == nullptr) return 0;
    // BLE notifications are limited by the negotiated MTU; the transport writes at most 20
    // bytes per notification so SLIP chunks are fragmented by the client's reassembly layer.
    size_t sent = 0;
    while (sent < len) {
        const size_t chunk = (len - sent) > 20 ? 20 : (len - sent);
        g_tx->setValue((uint8_t*)data + sent, chunk);
        g_tx->notify();
        sent += chunk;
    }
    return sent;
}

bool BleTransport::connected() const { return enabled_ && NimBLEDevice::getServer() != nullptr && NimBLEDevice::getServer()->getConnectedCount() > 0; }

#else
bool BleTransport::begin(const char*) { return false; }
int BleTransport::read(uint8_t*, size_t) { return 0; }
size_t BleTransport::write(const uint8_t*, size_t) { return 0; }
bool BleTransport::connected() const { return false; }
#endif

// --- SoftDevice-only bring-up (BLE compiled out) -------------------------------------------
#if defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES)
bool beginSoftDevice() {
    // Enables the SoftDevice without starting BLE. Used by the BLE-closed images (InternalFS flash
    // writes go through sd_flash_* SVCs, valid only with the SoftDevice on) and by the late-BLE
    // bring-up experiment, where advertising starts after boot. The USB/TinyUSB <-> SoftDevice
    // hand-off happens here, once.
    if (softDeviceEnabled()) return true;
    const bool ok = Bluefruit.begin();
    setSoftDeviceEnabled(ok);
    return ok;
}
#else
bool beginSoftDevice() { return false; }
#endif

}  // namespace hal
}  // namespace om

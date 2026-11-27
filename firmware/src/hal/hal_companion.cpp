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

// Burst-safe TX ring for the BLE companion link. A resource payload is forwarded as a burst of
// Rx events; each event is 13 notifications at the default 23-byte ATT MTU. 16 KiB covers a full
// 6 KiB image payload (34 OMP frames) with SLIP overhead plus the envelope/replies.
constexpr size_t kTxRingSize = 16u * 1024u;
uint8_t g_tx_ring[kTxRingSize];
size_t g_tx_head = 0;  // next write position
size_t g_tx_tail = 0;  // next byte to notify
}  // namespace
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
    // Fresh session: reset the burst queue and its diagnostics.
    g_tx_head = 0;
    g_tx_tail = 0;
    queued_bytes_ = 0;
    peak_queued_bytes_ = 0;
    chunks_sent_ = 0;
    notify_failures_ = 0;
    frames_dropped_ = 0;
    bytes_dropped_ = 0;
    mtu_ = 23;
    return enabled_;
}

namespace {
size_t ring_used() { return (g_tx_head + kTxRingSize - g_tx_tail) % kTxRingSize; }
}  // namespace

int BleTransport::read(uint8_t* buffer, size_t capacity) {
    if (!enabled_) return 0;
    size_t n = 0;
    while (n < capacity && g_bleuart.available() > 0) buffer[n++] = (uint8_t)g_bleuart.read();
    return (int)n;
}

size_t BleTransport::write(const uint8_t* data, size_t len) {
    if (!enabled_ || data == nullptr) return 0;
    if (len == 0) return 0;
    // Whole companion frames are queued, never partially delivered and never dropped just because
    // the SoftDevice notification queue is momentarily full. Adafruit's BLEUart::write() sends ONE
    // notification (<= ATT MTU - 3) and returns 0 when the link's TX queue rejects it; a 34-chunk
    // resource payload burst at 20-byte notifications used to outrun that queue, and the old
    // bounded retry (500 ms) then silently dropped the rest of the frame. service() drains this
    // ring at the link's pace from the main loop instead.
    const size_t used = ring_used();
    const size_t free = kTxRingSize - 1u - used;
    if (free < len) {
        ++frames_dropped_;
        bytes_dropped_ += (uint32_t)len;
        return 0;
    }
    size_t tail = g_tx_head;
    for (size_t i = 0; i < len; ++i) {
        g_tx_ring[tail] = data[i];
        tail = (tail + 1u) % kTxRingSize;
    }
    g_tx_head = tail;
    queued_bytes_ = (uint32_t)ring_used();
    if (queued_bytes_ > peak_queued_bytes_) peak_queued_bytes_ = queued_bytes_;
    return len;
}

void BleTransport::service() {
    if (!enabled_) return;
    // A BLE drop invalidates pending notifications for that app session: clear rather than deliver
    // stale Rx events to the next connection.
    if (!connected()) {
        g_tx_tail = g_tx_head;
        queued_bytes_ = 0;
        return;
    }
    if (ring_used() == 0) return;
    size_t chunk = 20;
    BLEConnection* conn = Bluefruit.Connection(Bluefruit.connHandle());
    if (conn != nullptr) {
        const uint16_t mtu = conn->getMtu();
        mtu_ = mtu;
        if (mtu > 3) chunk = (size_t)mtu - 3;
    }
    if (chunk > 244u) chunk = 244u;  // largest notification the TX characteristic is configured for
    // Push at most one connection-event worth per call so the main loop (radio/companion RX) is
    // never starved; the rest stays queued for the next loop iteration.
    uint32_t budget = 16;
    while (ring_used() > 0 && budget-- > 0) {
        const size_t used = ring_used();
        const size_t contiguous = kTxRingSize - g_tx_tail;
        size_t n = used < contiguous ? used : contiguous;
        if (n > chunk) n = chunk;
        const size_t w = (size_t)g_bleuart.write(g_tx_ring + g_tx_tail, n);
        if (w == 0) {
            ++notify_failures_;
            break;
        }
        g_tx_tail = (g_tx_tail + w) % kTxRingSize;
        ++chunks_sent_;
    }
    queued_bytes_ = (uint32_t)ring_used();
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

// ESP32 NimBLE path not migrated to the burst ring yet; notifications are sent synchronously in
// write(), so just reset diagnostics.
void BleTransport::service() {
    queued_bytes_ = 0;
    peak_queued_bytes_ = 0;
    frames_dropped_ = 0;
    bytes_dropped_ = 0;
}

#else
bool BleTransport::begin(const char*) { return false; }
int BleTransport::read(uint8_t*, size_t) { return 0; }
size_t BleTransport::write(const uint8_t*, size_t) { return 0; }
bool BleTransport::connected() const { return false; }
void BleTransport::service() {}
#endif

// --- SoftDevice-only bring-up (BLE compiled out) -------------------------------------------
#if defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES)
bool beginSoftDevice() {
    // Enables the SoftDevice without starting BLE. Used by the BLE-closed images (InternalFS flash
    // writes go through sd_flash_* SVCs, valid only with the SoftDevice on) and by the late-BLE
    // bring-up experiment, where advertising starts after boot. The USB/TinyUSB <-> SoftDevice
    // hand-off happens here, once.
    if (softDeviceEnabled()) return true;
#if defined(OM_ENABLE_BLE) && (OM_ENABLE_BLE)
    // Larger ATT MTU ceiling + deeper HVN TX queue so a companion burst (a resource payload is
    // forwarded as 34 Rx events = hundreds of notifications at the default 23-byte MTU) cannot
    // outrun the SoftDevice notification queue. The MTU ceiling only takes effect once the client
    // requests it; the deeper queue helps immediately. Modest values keep the S140 RAM reservation
    // in the linker's 24 KiB window.
    Bluefruit.configPrphConn(247, BLE_GAP_EVENT_LENGTH_DEFAULT, 4,
                             BLE_GATTC_WRITE_CMD_TX_QUEUE_SIZE_DEFAULT);
#endif
    const bool ok = Bluefruit.begin();
    setSoftDeviceEnabled(ok);
    return ok;
}
#else
bool beginSoftDevice() { return false; }
#endif

}  // namespace hal
}  // namespace om

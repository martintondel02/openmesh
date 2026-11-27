// SPDX-License-Identifier: AGPL-3.0-or-later
// Companion link transports: USB CDC and BLE NUS, both carrying the framed SLIP chunks from
// docs/architecture/COMPANION-API.md. The radio behaves identically with no app attached.
#ifndef OPENMESH_HAL_COMPANION_H
#define OPENMESH_HAL_COMPANION_H

#include <cstddef>
#include <cstdint>

namespace om {
namespace hal {

class UsbTransport {
public:
    bool begin();
    int read(uint8_t* buffer, size_t capacity);
    size_t write(const uint8_t* data, size_t len);
    bool connected() const;

private:
    bool begun_ = false;
};

class BleTransport {
public:
    bool begin(const char* device_name);
    int read(uint8_t* buffer, size_t capacity);
    size_t write(const uint8_t* data, size_t len);
    bool connected() const;
    bool enabled() const { return enabled_; }
    // Drains the burst TX queue toward the BLE link at the link's own pace. Must be called from the
    // main loop: companion replies/events are enqueued whole by write() and never dropped just
    // because the SoftDevice notification queue is momentarily full (resource-payload bursts).
    void service();
    // Diagnostics (used by the main loop log and for validation evidence).
    uint32_t queuedBytes() const { return queued_bytes_; }
    uint32_t peakQueuedBytes() const { return peak_queued_bytes_; }
    uint32_t sentChunks() const { return chunks_sent_; }
    uint32_t notifyFailures() const { return notify_failures_; }
    uint32_t droppedFrames() const { return frames_dropped_; }
    uint16_t mtu() const { return mtu_; }  // negotiated ATT MTU (23 until the client requests more)
    uint32_t droppedBytes() const { return bytes_dropped_; }

private:
    bool enabled_ = false;
    uint32_t queued_bytes_ = 0;
    uint32_t peak_queued_bytes_ = 0;
    uint32_t chunks_sent_ = 0;
    uint32_t notify_failures_ = 0;
    uint32_t frames_dropped_ = 0;
    uint32_t bytes_dropped_ = 0;
    uint16_t mtu_ = 23;
};

}  // namespace hal
}  // namespace om

#endif  // OPENMESH_HAL_COMPANION_H

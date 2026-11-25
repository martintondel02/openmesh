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

private:
    bool enabled_ = false;
};

}  // namespace hal
}  // namespace om

#endif  // OPENMESH_HAL_COMPANION_H

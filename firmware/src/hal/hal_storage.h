// SPDX-License-Identifier: AGPL-3.0-or-later
// Persistent storage backend: Adafruit InternalFileSystem on nRF52, LittleFS on ESP32.
// Only encrypted envelopes and key material are stored (never plaintext messages).
#ifndef OPENMESH_HAL_STORAGE_H
#define OPENMESH_HAL_STORAGE_H

#include <cstddef>
#include <cstdint>

#include "openmesh/om_engine.h"

namespace om {
namespace hal {

class HalStorage : public StorageBackend {
public:
    bool begin();
    bool load(const char* name, uint8_t* out, size_t capacity, size_t& len) override;
    bool save(const char* name, const uint8_t* data, size_t len) override;
    bool remove(const char* name) override;
    bool mounted() const { return mounted_; }

private:
    bool mounted_ = false;
};

}  // namespace hal
}  // namespace om

#endif  // OPENMESH_HAL_STORAGE_H

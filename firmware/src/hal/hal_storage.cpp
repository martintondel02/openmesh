// SPDX-License-Identifier: AGPL-3.0-or-later
#include "hal_storage.h"

#if defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES)
#include <Adafruit_LittleFS.h>
#include <InternalFileSystem.h>
using namespace Adafruit_LittleFS_Namespace;
#elif defined(ESP32)
#include <LittleFS.h>
#endif

namespace om {
namespace hal {

#if defined(ARDUINO_ARCH_NRF52) || defined(NRF52_SERIES)

bool HalStorage::begin() {
    mounted_ = InternalFS.begin();
    return mounted_;
}

bool HalStorage::load(const char* name, uint8_t* out, size_t capacity, size_t& len) {
    len = 0;
    if (!mounted_) return false;
    File file = InternalFS.open(name, FILE_O_READ);
    if (!file) return false;
    const size_t size = (size_t)file.size();
    if (size > capacity) {
        file.close();
        return false;
    }
    len = file.read(out, capacity);
    file.close();
    return len == size || len > 0;
}

bool HalStorage::save(const char* name, const uint8_t* data, size_t len) {
    if (!mounted_) return false;
    InternalFS.remove(name);
    File file = InternalFS.open(name, FILE_O_WRITE);
    if (!file) return false;
    const size_t written = file.write(data, len);
    file.close();
    return written == len;
}

bool HalStorage::remove(const char* name) {
    if (!mounted_) return false;
    return InternalFS.remove(name);
}

#elif defined(ESP32)

bool HalStorage::begin() {
    mounted_ = LittleFS.begin(true);
    return mounted_;
}

bool HalStorage::load(const char* name, uint8_t* out, size_t capacity, size_t& len) {
    len = 0;
    if (!mounted_) return false;
    File file = LittleFS.open(name, "r");
    if (!file) return false;
    const size_t size = (size_t)file.size();
    if (size > capacity) {
        file.close();
        return false;
    }
    len = file.read(out, capacity);
    file.close();
    return len > 0;
}

bool HalStorage::save(const char* name, const uint8_t* data, size_t len) {
    if (!mounted_) return false;
    File file = LittleFS.open(name, "w");
    if (!file) return false;
    const size_t written = file.write(data, len);
    file.close();
    return written == len;
}

bool HalStorage::remove(const char* name) {
    if (!mounted_) return false;
    return LittleFS.remove(name);
}

#else

bool HalStorage::begin() {
    mounted_ = false;
    return false;
}
bool HalStorage::load(const char*, uint8_t*, size_t, size_t& len) {
    len = 0;
    return false;
}
bool HalStorage::save(const char*, const uint8_t*, size_t) { return false; }
bool HalStorage::remove(const char*) { return false; }

#endif

}  // namespace hal
}  // namespace om

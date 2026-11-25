// SPDX-License-Identifier: AGPL-3.0-or-later
#include "openmesh/om_gps.h"

#include <cmath>
#include <cstdlib>
#include <cstring>

namespace om {

namespace {

int hexDigit(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

// Returns the Nth comma-separated field (0-based) and its length. Only reads within [s, s+len).
bool fieldAt(const char* s, size_t len, int index, const char*& out, size_t& out_len) {
    int current = 0;
    size_t start = 0;
    for (size_t i = 0; i <= len; ++i) {
        if (i == len || s[i] == ',' || s[i] == '*') {
            if (current == index) {
                out = s + start;
                out_len = i - start;
                return true;
            }
            ++current;
            start = i + 1;
            if (i < len && s[i] == '*') break;
        }
    }
    return false;
}

}  // namespace

void NmeaParser::reset() {
    len_ = 0;
    in_sentence_ = false;
    fix_ = GpsFix{};
}

bool NmeaParser::feed(char c, uint64_t now_ms) {
    if (c == '$') {
        in_sentence_ = true;
        len_ = 0;
        return false;
    }
    if (!in_sentence_) return false;
    if (c == '\r' || c == '\n') {
        in_sentence_ = false;
        if (len_ == 0) return false;
        return handleSentence(now_ms);
    }
    if (len_ >= kMaxSentence) {
        // Overlong sentence: discard until the next start delimiter.
        in_sentence_ = false;
        len_ = 0;
        return false;
    }
    sentence_[len_++] = c;
    return false;
}

bool NmeaParser::handleSentence(uint64_t now_ms) {
    // sentence_ = "GPGGA,...*HH"; verify the checksum over everything before '*'.
    sentence_[len_] = '\0';
    const char* star = (const char*)std::memchr(sentence_, '*', len_);
    if (star == nullptr) {
        ++fix_.checksum_errors;
        return false;
    }
    const size_t body_len = (size_t)(star - sentence_);
    if (body_len + 2 < len_) {
        const int hi = hexDigit(star[1]);
        const int lo = hexDigit(star[2]);
        if (hi < 0 || lo < 0) {
            ++fix_.checksum_errors;
            return false;
        }
        uint8_t checksum = 0;
        for (size_t i = 0; i < body_len; ++i) checksum ^= (uint8_t)sentence_[i];
        if (checksum != (uint8_t)((hi << 4) | lo)) {
            ++fix_.checksum_errors;
            return false;
        }
    }
    ++fix_.sentences;
    return parseSentence(sentence_, body_len, now_ms);
}

bool NmeaParser::parseSentence(const char* sentence, size_t len, uint64_t now_ms) {
    const char* type = nullptr;
    size_t type_len = 0;
    if (!fieldAt(sentence, len, 0, type, type_len)) return false;
    // Skip the 2-char talker id ("GP", "GN", "GA", ...).
    if (type_len < 5) return false;
    const char* kind = type + type_len - 3;
    const bool is_gga = std::memcmp(kind, "GGA", 3) == 0;
    const bool is_rmc = std::memcmp(kind, "RMC", 3) == 0;
    const bool is_gsa = std::memcmp(kind, "GSA", 3) == 0;
    if (!is_gga && !is_rmc && !is_gsa) return false;

    if (is_gga) {
        const char* f = nullptr;
        size_t n = 0;
        uint32_t quality = 0;
        if (fieldAt(sentence, len, 6, f, n) && n > 0) parseUint(f, n, quality);
        fix_.fix_quality = (uint8_t)quality;
        uint32_t sats = 0;
        if (fieldAt(sentence, len, 7, f, n) && n > 0) parseUint(f, n, sats);
        fix_.satellites = (uint8_t)sats;
        const char* lat = nullptr;
        const char* ns = nullptr;
        size_t lat_len = 0, ns_len = 0;
        if (fieldAt(sentence, len, 2, lat, lat_len) && fieldAt(sentence, len, 3, ns, ns_len) &&
            lat_len > 0 && ns_len == 1) {
            int32_t lat_e7 = 0;
            if (parseCoord(lat, lat_len, ns[0], lat_e7)) {
                fix_.lat_e7 = lat_e7;
                fix_.has_position = true;
            }
        }
        const char* lon = nullptr;
        const char* ew = nullptr;
        size_t lon_len = 0, ew_len = 0;
        if (fieldAt(sentence, len, 4, lon, lon_len) && fieldAt(sentence, len, 5, ew, ew_len) &&
            lon_len > 0 && ew_len == 1) {
            int32_t lon_e7 = 0;
            if (parseCoord(lon, lon_len, ew[0], lon_e7)) fix_.lon_e7 = lon_e7;
        }
        const char* alt = nullptr;
        size_t alt_len = 0;
        if (fieldAt(sentence, len, 9, alt, alt_len) && alt_len > 0) {
            char tmp[16];
            size_t copy = alt_len < sizeof(tmp) - 1 ? alt_len : sizeof(tmp) - 1;
            std::memcpy(tmp, alt, copy);
            tmp[copy] = '\0';
            fix_.alt_m = (int16_t)std::lround(std::strtod(tmp, nullptr));
            fix_.has_alt = true;
        }
        const char* hdop = nullptr;
        size_t hdop_len = 0;
        if (fieldAt(sentence, len, 8, hdop, hdop_len) && hdop_len > 0) {
            uint32_t hdop10 = 0;
            if (parseFixed1(hdop, hdop_len, hdop10)) {
                uint32_t accuracy = hdop10 / 2;  // 5 m per HDOP unit
                fix_.accuracy_m = (uint8_t)(accuracy > 255 ? 255 : accuracy);
                fix_.has_accuracy = true;
            }
        }
        if (quality > 0 && fix_.has_position) {
            fix_.valid = true;
            fix_.last_fix_ms = now_ms;
        }
        return true;
    }

    if (is_rmc) {
        const char* status = nullptr;
        size_t status_len = 0;
        if (fieldAt(sentence, len, 2, status, status_len) && status_len == 1) {
            if (status[0] == 'V') {
                // Void fix: keep the last position but do not report a new one.
                return true;
            }
        }
        const char* f = nullptr;
        size_t n = 0;
        const char* lat = nullptr;
        const char* ns = nullptr;
        const char* lon = nullptr;
        const char* ew = nullptr;
        size_t lat_len = 0, ns_len = 0, lon_len = 0, ew_len = 0;
        if (fieldAt(sentence, len, 3, lat, lat_len) && fieldAt(sentence, len, 4, ns, ns_len) &&
            fieldAt(sentence, len, 5, lon, lon_len) && fieldAt(sentence, len, 6, ew, ew_len) &&
            lat_len > 0 && lon_len > 0) {
            int32_t lat_e7 = 0, lon_e7 = 0;
            if (parseCoord(lat, lat_len, ns_len == 1 ? ns[0] : 'N', lat_e7) &&
                parseCoord(lon, lon_len, ew_len == 1 ? ew[0] : 'E', lon_e7)) {
                fix_.lat_e7 = lat_e7;
                fix_.lon_e7 = lon_e7;
                fix_.has_position = true;
                fix_.valid = true;
                fix_.last_fix_ms = now_ms;
            }
        }
        // Speed in knots -> cm/s.
        if (fieldAt(sentence, len, 7, f, n) && n > 0) {
            char tmp[16];
            size_t copy = n < sizeof(tmp) - 1 ? n : sizeof(tmp) - 1;
            std::memcpy(tmp, f, copy);
            tmp[copy] = '\0';
            const double knots = std::strtod(tmp, nullptr);
            const double cm_s = knots * 51.4444;
            fix_.speed_cm_s = (uint16_t)(cm_s > 65535.0 ? 65535.0 : cm_s);
            fix_.has_speed = cm_s > 0.0;
        }
        // Course over ground -> heading.
        if (fieldAt(sentence, len, 8, f, n) && n > 0) {
            char tmp[16];
            size_t copy = n < sizeof(tmp) - 1 ? n : sizeof(tmp) - 1;
            std::memcpy(tmp, f, copy);
            tmp[copy] = '\0';
            double course = std::strtod(tmp, nullptr);
            if (course >= 0.0) {
                course = std::fmod(course, 360.0);
                fix_.heading_deg = (uint16_t)std::lround(course) % 360;
                fix_.has_heading = true;
            }
        }
        return true;
    }

    // GSA: fix type at field 2 (1 = no fix, 2 = 2D, 3 = 3D).
    const char* f = nullptr;
    size_t n = 0;
    if (fieldAt(sentence, len, 2, f, n) && n == 1) {
        if (f[0] == '2' || f[0] == '3') {
            fix_.valid = fix_.has_position;
        } else if (f[0] == '1') {
            fix_.valid = false;
        }
    }
    return true;
}

bool NmeaParser::parseCoord(const char* field, size_t len, char hemisphere, int32_t& out_e7) {
    // NMEA format: ddmm.mmmm (lat) or dddmm.mmmm (lon), no sign.
    char tmp[24];
    if (len == 0 || len >= sizeof(tmp)) return false;
    std::memcpy(tmp, field, len);
    tmp[len] = '\0';
    const double value = std::strtod(tmp, nullptr);
    if (value <= 0.0) {
        out_e7 = 0;
        return true;
    }
    const int degrees = (int)(value / 100.0);
    const double minutes = value - degrees * 100.0;
    double decimal = degrees + minutes / 60.0;
    if (hemisphere == 'S' || hemisphere == 'W') decimal = -decimal;
    out_e7 = (int32_t)std::llround(decimal * 1e7);
    return true;
}

bool NmeaParser::parseUint(const char* field, size_t len, uint32_t& out) {
    uint32_t v = 0;
    size_t i = 0;
    if (len == 0) return false;
    for (; i < len; ++i) {
        if (field[i] < '0' || field[i] > '9') break;
        v = v * 10 + (uint32_t)(field[i] - '0');
    }
    if (i == 0) return false;
    out = v;
    return true;
}

bool NmeaParser::parseFixed1(const char* field, size_t len, uint32_t& out_x10) {
    uint32_t whole = 0;
    uint32_t frac = 0;
    size_t i = 0;
    for (; i < len && field[i] >= '0' && field[i] <= '9'; ++i) whole = whole * 10 + (field[i] - '0');
    if (i < len && field[i] == '.') {
        ++i;
        uint32_t scale = 1;
        for (int k = 0; k < 2 && i < len && field[i] >= '0' && field[i] <= '9'; ++k, ++i) {
            frac = frac * 10 + (field[i] - '0');
            scale *= 10;
        }
        if (scale == 10) frac *= 10;
    }
    out_x10 = whole * 10 + frac;
    return true;
}

}  // namespace om

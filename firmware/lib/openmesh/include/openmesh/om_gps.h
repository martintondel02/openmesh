// SPDX-License-Identifier: AGPL-3.0-or-later
// GPS / GNSS NMEA 0183 parsing (GGA, RMC, GSA) for trackers and solar nodes.
//
// The parser is deliberately tolerant: it consumes characters from a UART stream, reassembles
// sentences, verifies the NMEA checksum and keeps the last valid fix. Vendor modules:
//   * Seeed T1000-E: Airoha GNSS (115200 baud, proprietary start-up but standard NMEA out)
//   * Seeed Solar Node P1: L76KB (9600 baud)
//   * Heltec Wireless Tracker: L76K via UC6580 (115200 baud, GPS_UC6580 variant)
#ifndef OPENMESH_OM_GPS_H
#define OPENMESH_OM_GPS_H

#include <cstddef>
#include <cstdint>

namespace om {

struct GpsFix {
    bool valid = false;          // at least one valid fix since boot
    bool has_position = false;   // lat/lon available from the latest sentence
    int32_t lat_e7 = 0;
    int32_t lon_e7 = 0;
    bool has_alt = false;
    int16_t alt_m = 0;
    bool has_accuracy = false;
    uint8_t accuracy_m = 0;      // HDOP * 5, capped at 255
    bool has_speed = false;
    uint16_t speed_cm_s = 0;
    bool has_heading = false;
    uint16_t heading_deg = 0;
    uint8_t satellites = 0;
    uint8_t fix_quality = 0;     // 0 none, 1 GPS, 2 DGPS, ...
    uint64_t last_fix_ms = 0;
    uint32_t sentences = 0;
    uint32_t checksum_errors = 0;
};

class NmeaParser {
public:
    static constexpr size_t kMaxSentence = 96;

    // Feed one byte from the GNSS UART. Returns true when a complete sentence was parsed
    // (valid or not); check `fix()` for the resulting state.
    bool feed(char c, uint64_t now_ms);

    const GpsFix& fix() const { return fix_; }
    void reset();

    // Exposed for tests: parse one NUL-terminated sentence without the leading '$' and
    // trailing CR/LF. Returns true when the sentence was well-formed and understood.
    bool parseSentence(const char* sentence, size_t len, uint64_t now_ms);

private:
    bool handleSentence(uint64_t now_ms);
    static bool parseCoord(const char* field, size_t len, char hemisphere, int32_t& out_e7);
    static bool parseUint(const char* field, size_t len, uint32_t& out);
    static bool parseFixed1(const char* field, size_t len, uint32_t& out_x10);

    char sentence_[kMaxSentence]{};
    size_t len_ = 0;
    bool in_sentence_ = false;
    GpsFix fix_{};
};

}  // namespace om

#endif  // OPENMESH_OM_GPS_H

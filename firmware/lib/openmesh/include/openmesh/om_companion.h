// SPDX-License-Identifier: AGPL-3.0-or-later
// Companion link framing (docs/architecture/COMPANION-API.md §1).
//
// Stream transports use SLIP delimiters around a chunk:
//   magic "OM" | type u8 | len u16 LE | payload | crc16 u16 LE
// where crc16 is CRC-16/CCITT-FALSE over type || len || payload. The same decoder can also
// pass through raw OMP frames for the bridge transport: the framing is self-identifying via
// the "OM" magic.
#ifndef OPENMESH_OM_COMPANION_H
#define OPENMESH_OM_COMPANION_H

#include <cstddef>
#include <cstdint>

#include "openmesh/om_types.h"

namespace om {

constexpr size_t kCompanionMaxPayload = 1024;
constexpr size_t kCompanionHeaderLen = 5;   // "OM" | type | len LE
constexpr size_t kCompanionMaxChunk = kCompanionHeaderLen + kCompanionMaxPayload + 2;
constexpr uint8_t kSlipEnd = 0xC0;
constexpr uint8_t kSlipEsc = 0xDB;
constexpr uint8_t kSlipEscEnd = 0xDC;
constexpr uint8_t kSlipEscEsc = 0xDD;

enum class CompanionType : uint8_t {
    Hello = 1,
    HelloAck = 2,
    ConfigGet = 3,
    ConfigSet = 4,
    Config = 5,
    Tx = 6,
    Rx = 7,
    StatsGet = 8,
    Stats = 9,
    Ping = 10,
    Pong = 11,
    Event = 12,
    Reset = 13,
};

uint16_t crc16CcittFalse(const uint8_t* data, size_t len);

// --- Decoded companion message --------------------------------------------------------------
struct CompanionEntry {
    char key[24]{};
    char value[64]{};
};

constexpr size_t kCompanionMaxEntries = 8;

struct CompanionMsg {
    CompanionType type = CompanionType::Ping;

    // Handshake.
    uint8_t proto = kProtoVersion;
    char app_version[32]{};
    char fw_version[24]{};
    NodeId node_id{};
    uint16_t max_frame = (uint16_t)kMtu;
    uint16_t caps = 0;
    char region[12]{};
    char profile[16]{};
    bool wants_events = true;

    // ConfigSet / Config.
    char key[24]{};
    char value[64]{};
    CompanionEntry entries[kCompanionMaxEntries]{};
    uint8_t entry_count = 0;

    // Tx / Rx.
    uint8_t frame[kMtu]{};
    uint16_t frame_len = 0;
    int16_t rssi = 0;
    int16_t snr = 0;
    char link[12]{};
    uint64_t at_ms = 0;

    // Ping / Pong.
    uint32_t nonce = 0;

    // Event.
    char kind[24]{};
    char detail[64]{};
};

// Payload CBOR encoding/decoding per COMPANION-API.md §2.
size_t encodeCompanionPayload(const CompanionMsg& msg, uint8_t* out, size_t capacity);
bool decodeCompanionPayload(CompanionType type, const uint8_t* payload, size_t len,
                             CompanionMsg& out);

// Whole chunk: magic/type/len/payload/crc. Returns bytes written or 0.
size_t encodeCompanionChunk(const CompanionMsg& msg, uint8_t* out, size_t capacity);
bool decodeCompanionChunk(const uint8_t* chunk, size_t len, CompanionMsg& out);

// SLIP stream encoder: appends END + escaped chunk + END to `out`.
size_t slipEncode(const uint8_t* chunk, size_t len, uint8_t* out, size_t capacity);

// Streaming SLIP decoder. Feed one byte at a time from the transport; when a complete,
// CRC-valid chunk is available it is copied into `chunk()`.
class SlipDecoder {
public:
    static constexpr size_t kBufferSize = kCompanionMaxChunk + 16;

    void reset();
    // Returns true when a complete delimiter-terminated frame was decoded (valid or not).
    bool feed(uint8_t byte);
    bool frameValid() const { return last_valid_; }
    const uint8_t* chunk() const { return buffer_; }
    size_t chunkLen() const { return chunk_len_; }
    uint32_t framesDecoded() const { return frames_decoded_; }
    uint32_t framesDropped() const { return frames_dropped_; }

private:
    uint8_t buffer_[kBufferSize]{};
    size_t chunk_len_ = 0;
    bool escaping_ = false;
    bool overflow_ = false;
    bool in_frame_ = false;
    bool last_valid_ = false;
    uint32_t frames_decoded_ = 0;
    uint32_t frames_dropped_ = 0;
};

}  // namespace om

#endif  // OPENMESH_OM_COMPANION_H

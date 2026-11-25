// SPDX-License-Identifier: AGPL-3.0-or-later
#include "openmesh/om_companion.h"

#include <cstdlib>
#include <cstring>

#include "openmesh/om_cbor.h"

namespace om {

uint16_t crc16CcittFalse(const uint8_t* data, size_t len) {
    uint16_t crc = 0xFFFF;
    for (size_t i = 0; i < len; ++i) {
        crc ^= (uint16_t)((uint16_t)data[i] << 8);
        for (int bit = 0; bit < 8; ++bit) {
            if (crc & 0x8000) {
                crc = (uint16_t)((crc << 1) ^ 0x1021);
            } else {
                crc = (uint16_t)(crc << 1);
            }
        }
    }
    return crc;
}

namespace {

uint16_t readU16Le(const uint8_t* p) { return (uint16_t)(p[0] | ((uint16_t)p[1] << 8)); }

void copyStr(char* dst, size_t cap, const char* src) {
    if (src == nullptr) {
        dst[0] = '\0';
        return;
    }
    size_t n = std::strlen(src);
    if (n >= cap) n = cap - 1;
    std::memcpy(dst, src, n);
    dst[n] = '\0';
}

bool keyIs(const uint8_t* key, size_t key_len, const char* expected) {
    const size_t n = std::strlen(expected);
    return key_len == n && std::memcmp(key, expected, n) == 0;
}

bool readTextTo(const uint8_t* p, size_t n, char* dst, size_t cap) {
    if (n >= cap) n = cap - 1;
    std::memcpy(dst, p, n);
    dst[n] = '\0';
    return true;
}

bool encodeEntries(const CompanionEntry* entries, uint8_t count, CborWriter& w, bool numeric) {
    size_t pairs = 0;
    if (!w.mapHeader(1)) return false;
    if (!w.key("entries")) return false;
    if (!w.arrayHeader(count)) return false;
    for (uint8_t i = 0; i < count; ++i) {
        if (!w.arrayHeader(2)) return false;
        if (!w.text(entries[i].key)) return false;
        if (numeric) {
            // Stats values are u64; Config values are strings.
            const uint64_t v = std::strtoull(entries[i].value, nullptr, 10);
            if (!w.uint(v)) return false;
        } else {
            if (!w.text(entries[i].value)) return false;
        }
    }
    (void)pairs;
    return true;
}

bool encodeNodeId(const NodeId& id, CborWriter& w) {
    // COMPANION-API.md §2 pins node_id as bytes(8).
    return w.bytes(id.b, 8);
}

}  // namespace

size_t encodeCompanionPayload(const CompanionMsg& msg, uint8_t* out, size_t capacity) {
    CborWriter w(out, capacity);
    switch (msg.type) {
        case CompanionType::Hello:
            w.mapHeader(3);
            w.key("proto");
            w.uint(msg.proto);
            w.key("app_version");
            w.text(msg.app_version);
            w.key("wants_events");
            w.boolean(msg.wants_events);
            break;
        case CompanionType::HelloAck:
            w.mapHeader(7);
            w.key("proto");
            w.uint(msg.proto);
            w.key("fw_version");
            w.text(msg.fw_version);
            w.key("node_id");
            encodeNodeId(msg.node_id, w);
            w.key("max_frame");
            w.uint(msg.max_frame);
            w.key("caps");
            w.uint(msg.caps);
            w.key("region");
            w.text(msg.region);
            w.key("profile");
            w.text(msg.profile);
            break;
        case CompanionType::ConfigGet:
        case CompanionType::StatsGet:
        case CompanionType::Reset:
            w.mapHeader(0);
            break;
        case CompanionType::ConfigSet:
            w.mapHeader(2);
            w.key("key");
            w.text(msg.key);
            w.key("value");
            w.text(msg.value);
            break;
        case CompanionType::Config:
            return encodeEntries(msg.entries, msg.entry_count, w, false) && w.ok() ? w.size() : 0;
        case CompanionType::Tx:
            w.mapHeader(1);
            w.key("frame");
            w.bytes(msg.frame, msg.frame_len);
            break;
        case CompanionType::Rx:
            w.mapHeader(5);
            w.key("frame");
            w.bytes(msg.frame, msg.frame_len);
            w.key("rssi");
            w.int64Value(msg.rssi);
            w.key("snr");
            w.int64Value(msg.snr);
            w.key("link");
            w.text(msg.link);
            w.key("at_ms");
            w.uint(msg.at_ms);
            break;
        case CompanionType::Stats:
            return encodeEntries(msg.entries, msg.entry_count, w, true) && w.ok() ? w.size() : 0;
        case CompanionType::Ping:
        case CompanionType::Pong:
            w.mapHeader(1);
            w.key("nonce");
            w.uint(msg.nonce);
            break;
        case CompanionType::Event:
            w.mapHeader(2);
            w.key("kind");
            w.text(msg.kind);
            w.key("detail");
            w.text(msg.detail);
            break;
        default:
            return 0;
    }
    return w.ok() ? w.size() : 0;
}

bool decodeCompanionPayload(CompanionType type, const uint8_t* payload, size_t len,
                             CompanionMsg& out) {
    out = CompanionMsg{};
    out.type = type;
    CborReader r(payload, len);
    size_t pairs = 0;
    if (!r.mapHeader(pairs)) return false;
    for (size_t i = 0; i < pairs && r.ok(); ++i) {
        const uint8_t* key = nullptr;
        size_t key_len = 0;
        if (!r.readKeySpan(key, key_len)) return false;
        const uint8_t* text = nullptr;
        size_t text_len = 0;
        const uint8_t* bytes = nullptr;
        size_t bytes_len = 0;
        if (keyIs(key, key_len, "proto")) {
            if (!r.readU8(out.proto)) return false;
        } else if (keyIs(key, key_len, "app_version")) {
            if (!r.readText(text, text_len)) return false;
            readTextTo(text, text_len, out.app_version, sizeof(out.app_version));
        } else if (keyIs(key, key_len, "fw_version")) {
            if (!r.readText(text, text_len)) return false;
            readTextTo(text, text_len, out.fw_version, sizeof(out.fw_version));
        } else if (keyIs(key, key_len, "wants_events")) {
            if (!r.readBool(out.wants_events)) return false;
        } else if (keyIs(key, key_len, "node_id")) {
            // Pin: bytes(8). Tolerate an array of 8 integers for forward compatibility.
            if (r.readBytes(bytes, bytes_len) && bytes_len == 8) {
                std::memcpy(out.node_id.b, bytes, 8);
            } else {
                // Reset reader state is complex after a failed readBytes; require bytes(8).
                return false;
            }
        } else if (keyIs(key, key_len, "max_frame")) {
            if (!r.readU16(out.max_frame)) return false;
        } else if (keyIs(key, key_len, "caps")) {
            if (!r.readU16(out.caps)) return false;
        } else if (keyIs(key, key_len, "region")) {
            if (!r.readText(text, text_len)) return false;
            readTextTo(text, text_len, out.region, sizeof(out.region));
        } else if (keyIs(key, key_len, "profile")) {
            if (!r.readText(text, text_len)) return false;
            readTextTo(text, text_len, out.profile, sizeof(out.profile));
        } else if (keyIs(key, key_len, "key")) {
            if (!r.readText(text, text_len)) return false;
            readTextTo(text, text_len, out.key, sizeof(out.key));
        } else if (keyIs(key, key_len, "value")) {
            if (!r.readText(text, text_len)) return false;
            readTextTo(text, text_len, out.value, sizeof(out.value));
        } else if (keyIs(key, key_len, "frame")) {
            if (!r.readBytes(bytes, bytes_len)) return false;
            if (bytes_len > sizeof(out.frame)) return false;
            std::memcpy(out.frame, bytes, bytes_len);
            out.frame_len = (uint16_t)bytes_len;
        } else if (keyIs(key, key_len, "rssi")) {
            int64_t v = 0;
            if (!r.readInt(v)) return false;
            out.rssi = (int16_t)v;
        } else if (keyIs(key, key_len, "snr")) {
            int64_t v = 0;
            if (!r.readInt(v)) return false;
            out.snr = (int16_t)v;
        } else if (keyIs(key, key_len, "link")) {
            if (!r.readText(text, text_len)) return false;
            readTextTo(text, text_len, out.link, sizeof(out.link));
        } else if (keyIs(key, key_len, "at_ms")) {
            if (!r.readUint(out.at_ms)) return false;
        } else if (keyIs(key, key_len, "nonce")) {
            if (!r.readU32(out.nonce)) return false;
        } else if (keyIs(key, key_len, "kind")) {
            if (!r.readText(text, text_len)) return false;
            readTextTo(text, text_len, out.kind, sizeof(out.kind));
        } else if (keyIs(key, key_len, "detail")) {
            if (!r.readText(text, text_len)) return false;
            readTextTo(text, text_len, out.detail, sizeof(out.detail));
        } else if (keyIs(key, key_len, "entries")) {
            size_t entry_pairs = 0;
            if (!r.arrayHeader(entry_pairs)) return false;
            out.entry_count = 0;
            for (size_t k = 0; k < entry_pairs; ++k) {
                size_t pair_items = 0;
                if (!r.arrayHeader(pair_items) || pair_items != 2) return false;
                const uint8_t* kp = nullptr;
                size_t kn = 0;
                if (!r.readText(kp, kn)) return false;
                if (out.entry_count < kCompanionMaxEntries) {
                    readTextTo(kp, kn, out.entries[out.entry_count].key,
                               sizeof(out.entries[0].key));
                }
                if (type == CompanionType::Stats) {
                    uint64_t v = 0;
                    if (!r.readUint(v)) return false;
                    if (out.entry_count < kCompanionMaxEntries) {
                        // Store numeric values as decimal strings; max 20 digits.
                        char tmp[24];
                        size_t pos = sizeof(tmp) - 1;
                        tmp[pos] = '\0';
                        do {
                            tmp[--pos] = (char)('0' + (v % 10));
                            v /= 10;
                        } while (v > 0 && pos > 1);
                        copyStr(out.entries[out.entry_count].value,
                                sizeof(out.entries[0].value), tmp + pos);
                    }
                } else {
                    const uint8_t* vp = nullptr;
                    size_t vn = 0;
                    if (!r.readText(vp, vn)) return false;
                    if (out.entry_count < kCompanionMaxEntries) {
                        readTextTo(vp, vn, out.entries[out.entry_count].value,
                                   sizeof(out.entries[0].value));
                    }
                }
                if (out.entry_count < kCompanionMaxEntries) ++out.entry_count;
            }
        } else if (!r.skipValue()) {
            return false;
        }
    }
    return r.ok();
}

size_t encodeCompanionChunk(const CompanionMsg& msg, uint8_t* out, size_t capacity) {
    const size_t need = kCompanionHeaderLen + kCompanionMaxPayload + 2;
    if (capacity < need) return 0;
    const size_t payload_len = encodeCompanionPayload(msg, out + kCompanionHeaderLen,
                                                      capacity - kCompanionHeaderLen - 2);
    if (payload_len == 0) return 0;
    out[0] = 'O';
    out[1] = 'M';
    out[2] = (uint8_t)msg.type;
    out[3] = (uint8_t)(payload_len & 0xFF);
    out[4] = (uint8_t)((payload_len >> 8) & 0xFF);
    const uint16_t crc = crc16CcittFalse(out + 2, 3 + payload_len);
    out[kCompanionHeaderLen + payload_len] = (uint8_t)(crc & 0xFF);
    out[kCompanionHeaderLen + payload_len + 1] = (uint8_t)((crc >> 8) & 0xFF);
    return kCompanionHeaderLen + payload_len + 2;
}

bool decodeCompanionChunk(const uint8_t* chunk, size_t len, CompanionMsg& out) {
    if (len < kCompanionHeaderLen + 2) return false;
    if (chunk[0] != 'O' || chunk[1] != 'M') return false;
    const uint8_t type_byte = chunk[2];
    if (type_byte < (uint8_t)CompanionType::Hello || type_byte > (uint8_t)CompanionType::Reset)
        return false;
    const uint16_t payload_len = readU16Le(chunk + 3);
    if (payload_len > kCompanionMaxPayload) return false;
    if (len != (size_t)kCompanionHeaderLen + payload_len + 2) return false;
    const uint16_t expected = crc16CcittFalse(chunk + 2, 3 + payload_len);
    const uint16_t actual = (uint16_t)(chunk[kCompanionHeaderLen + payload_len] |
                                       ((uint16_t)chunk[kCompanionHeaderLen + payload_len + 1] << 8));
    if (expected != actual) return false;
    return decodeCompanionPayload((CompanionType)type_byte, chunk + kCompanionHeaderLen,
                                  payload_len, out);
}

size_t slipEncode(const uint8_t* chunk, size_t len, uint8_t* out, size_t capacity) {
    size_t off = 0;
    auto put = [&](uint8_t b) {
        if (off >= capacity) return false;
        out[off++] = b;
        return true;
    };
    if (!put(kSlipEnd)) return 0;
    for (size_t i = 0; i < len; ++i) {
        const uint8_t b = chunk[i];
        if (b == kSlipEnd) {
            if (!put(kSlipEsc) || !put(kSlipEscEnd)) return 0;
        } else if (b == kSlipEsc) {
            if (!put(kSlipEsc) || !put(kSlipEscEsc)) return 0;
        } else {
            if (!put(b)) return 0;
        }
    }
    if (!put(kSlipEnd)) return 0;
    return off;
}

void SlipDecoder::reset() {
    chunk_len_ = 0;
    escaping_ = false;
    overflow_ = false;
    in_frame_ = false;
    last_valid_ = false;
}

bool SlipDecoder::feed(uint8_t byte) {
    if (byte == kSlipEnd) {
        if (!in_frame_) {
            // Opening delimiter (or back-to-back frames): start a new chunk.
            in_frame_ = true;
            chunk_len_ = 0;
            escaping_ = false;
            overflow_ = false;
            return false;
        }
        in_frame_ = false;
        if (overflow_ || chunk_len_ == 0) {
            ++frames_dropped_;
            last_valid_ = false;
            return true;
        }
        CompanionMsg ignored;
        last_valid_ = decodeCompanionChunk(buffer_, chunk_len_, ignored);
        if (last_valid_) {
            ++frames_decoded_;
        } else {
            ++frames_dropped_;
        }
        return true;
    }
    if (!in_frame_) return false;
    uint8_t b = byte;
    if (escaping_) {
        escaping_ = false;
        if (b == kSlipEscEnd) {
            b = kSlipEnd;
        } else if (b == kSlipEscEsc) {
            b = kSlipEsc;
        } else {
            // Unknown escape: drop the frame rather than guessing.
            overflow_ = true;
            ++frames_dropped_;
            return false;
        }
    } else if (b == kSlipEsc) {
        escaping_ = true;
        return false;
    }
    if (chunk_len_ >= sizeof(buffer_)) {
        overflow_ = true;
        return false;
    }
    buffer_[chunk_len_++] = b;
    return false;
}

}  // namespace om

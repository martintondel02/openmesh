// SPDX-License-Identifier: AGPL-3.0-or-later
#include "openmesh/om_cbor.h"

#include <cstring>

namespace om {

CborWriter::CborWriter(uint8_t* buffer, size_t capacity) : buf_(buffer), cap_(capacity) {}

void CborWriter::reset(uint8_t* buffer, size_t capacity) {
    buf_ = buffer;
    cap_ = capacity;
    len_ = 0;
    ok_ = true;
}

bool CborWriter::raw(const void* data, size_t n) {
    if (!ok_ || len_ + n > cap_) {
        ok_ = false;
        return false;
    }
    std::memcpy(buf_ + len_, data, n);
    len_ += n;
    return true;
}

bool CborWriter::head(uint8_t major, uint64_t value) {
    uint8_t b[9];
    size_t n = 0;
    const uint8_t m = (uint8_t)(major << 5);
    if (value < 24) {
        b[n++] = (uint8_t)(m | (uint8_t)value);
    } else if (value <= 0xFF) {
        b[n++] = (uint8_t)(m | 24);
        b[n++] = (uint8_t)value;
    } else if (value <= 0xFFFF) {
        b[n++] = (uint8_t)(m | 25);
        b[n++] = (uint8_t)(value >> 8);
        b[n++] = (uint8_t)value;
    } else if (value <= 0xFFFFFFFFULL) {
        b[n++] = (uint8_t)(m | 26);
        b[n++] = (uint8_t)(value >> 24);
        b[n++] = (uint8_t)(value >> 16);
        b[n++] = (uint8_t)(value >> 8);
        b[n++] = (uint8_t)value;
    } else {
        b[n++] = (uint8_t)(m | 27);
        for (int i = 7; i >= 0; --i) b[n++] = (uint8_t)(value >> (8 * i));
    }
    return raw(b, n);
}

bool CborWriter::mapHeader(size_t pair_count) { return head(5, pair_count); }
bool CborWriter::arrayHeader(size_t item_count) { return head(4, item_count); }

bool CborWriter::key(const char* k) { return text(k, std::strlen(k)); }

bool CborWriter::text(const char* s) { return text(s, std::strlen(s)); }

bool CborWriter::text(const char* s, size_t n) {
    if (!head(3, n)) return false;
    return raw(s, n);
}

bool CborWriter::bytes(const uint8_t* data, size_t n) {
    if (!head(2, n)) return false;
    return raw(data, n);
}

bool CborWriter::uint(uint64_t v) { return head(0, v); }

bool CborWriter::int64Value(int64_t v) {
    if (v >= 0) return head(0, (uint64_t)v);
    return head(1, (uint64_t)(-1 - v));
}

bool CborWriter::boolean(bool v) { return raw(v ? "\xF5" : "\xF4", 1); }

bool CborWriter::nullValue() { return raw("\xF6", 1); }

bool CborWriter::rawItem(const uint8_t* data, size_t len) { return raw(data, len); }

// --- Reader ---------------------------------------------------------------------------------
CborReader::CborReader(const uint8_t* data, size_t len) : data_(data), len_(len) {}

bool CborReader::readHead(uint8_t& major, uint64_t& value) {
    if (!ok_ || pos_ >= len_) {
        ok_ = false;
        return false;
    }
    const uint8_t initial = data_[pos_++];
    major = (uint8_t)(initial >> 5);
    const uint8_t ai = (uint8_t)(initial & 0x1F);
    value = 0;
    if (ai < 24) {
        value = ai;
        return true;
    }
    size_t extra = 0;
    switch (ai) {
        case 24: extra = 1; break;
        case 25: extra = 2; break;
        case 26: extra = 4; break;
        case 27: extra = 8; break;
        case 31: extra = 0; break;  // indefinite length / break
        default:
            ok_ = false;
            return false;
    }
    if (extra > 0) {
        if (pos_ + extra > len_) {
            ok_ = false;
            return false;
        }
        for (size_t i = 0; i < extra; ++i) value = (value << 8) | data_[pos_ + i];
        pos_ += extra;
    } else {
        value = 31;  // marker for indefinite
    }
    return true;
}

bool CborReader::readLength(uint8_t major, size_t& len) {
    uint8_t m = 0;
    uint64_t v = 0;
    if (!readHead(m, v)) return false;
    if (m != major) {
        ok_ = false;
        return false;
    }
    if (v == 31) {
        // Indefinite length: not produced by ciborium, but tolerated on read.
        ok_ = false;
        return false;
    }
    len = (size_t)v;
    return true;
}

bool CborReader::mapHeader(size_t& count) { return readLength(5, count); }
bool CborReader::arrayHeader(size_t& count) { return readLength(4, count); }

bool CborReader::readText(const uint8_t*& text, size_t& len) {
    uint8_t m = 0;
    uint64_t v = 0;
    const size_t save = pos_;
    if (!readHead(m, v)) return false;
    if (m != 3) {
        pos_ = save;
        return false;
    }
    if (v == 31) {
        ok_ = false;
        return false;
    }
    if (pos_ + (size_t)v > len_) {
        ok_ = false;
        return false;
    }
    text = data_ + pos_;
    len = (size_t)v;
    pos_ += len;
    return true;
}

bool CborReader::readKeySpan(const uint8_t*& text, size_t& len) {
    return readText(text, len);
}

bool CborReader::key(const char* expected) {
    const uint8_t* p = nullptr;
    size_t n = 0;
    if (!readText(p, n)) return false;
    const size_t want = std::strlen(expected);
    return n == want && std::memcmp(p, expected, want) == 0;
}

bool CborReader::readBytes(const uint8_t*& data, size_t& len) {
    uint8_t m = 0;
    uint64_t v = 0;
    const size_t save = pos_;
    if (!readHead(m, v)) return false;
    if (m != 2) {
        pos_ = save;
        return false;
    }
    if (v == 31) {
        ok_ = false;
        return false;
    }
    if (pos_ + (size_t)v > len_) {
        ok_ = false;
        return false;
    }
    data = data_ + pos_;
    len = (size_t)v;
    pos_ += len;
    return true;
}

bool CborReader::readUint(uint64_t& out) {
    uint8_t m = 0;
    uint64_t v = 0;
    const size_t save = pos_;
    if (!readHead(m, v)) return false;
    if (m == 0) {
        out = v;
        return true;
    }
    if (m == 1) {
        out = 0;  // negative: not valid for a uint field
        ok_ = false;
        return false;
    }
    pos_ = save;
    ok_ = false;
    return false;
}

bool CborReader::readInt(int64_t& out) {
    uint8_t m = 0;
    uint64_t v = 0;
    const size_t save = pos_;
    if (!readHead(m, v)) return false;
    if (m == 0 && v <= (uint64_t)INT64_MAX) {
        out = (int64_t)v;
        return true;
    }
    if (m == 1 && v <= (uint64_t)INT64_MAX) {
        out = -1 - (int64_t)v;
        return true;
    }
    pos_ = save;
    ok_ = false;
    return false;
}

bool CborReader::readBool(bool& out) {
    if (pos_ >= len_) {
        ok_ = false;
        return false;
    }
    const uint8_t b = data_[pos_];
    if (b == 0xF4) {
        out = false;
        ++pos_;
        return true;
    }
    if (b == 0xF5) {
        out = true;
        ++pos_;
        return true;
    }
    ok_ = false;
    return false;
}

bool CborReader::readU8(uint8_t& out) {
    uint64_t v = 0;
    if (!readUint(v) || v > 0xFF) {
        ok_ = false;
        return false;
    }
    out = (uint8_t)v;
    return true;
}

bool CborReader::readU16(uint16_t& out) {
    uint64_t v = 0;
    if (!readUint(v) || v > 0xFFFF) {
        ok_ = false;
        return false;
    }
    out = (uint16_t)v;
    return true;
}

bool CborReader::readU32(uint32_t& out) {
    uint64_t v = 0;
    if (!readUint(v) || v > 0xFFFFFFFFULL) {
        ok_ = false;
        return false;
    }
    out = (uint32_t)v;
    return true;
}

bool CborReader::skipValue() {
    uint8_t m = 0;
    uint64_t v = 0;
    const size_t save = pos_;
    if (!readHead(m, v)) return false;
    if (v == 31 && (m == 2 || m == 3 || m == 4 || m == 5)) {
        // Indefinite length: consume until break.
        while (pos_ < len_) {
            if (data_[pos_] == 0xFF) {
                ++pos_;
                return true;
            }
            if (!skipValue()) return false;
            if (m == 5 && pos_ < len_ && data_[pos_] != 0xFF) {
                if (!skipValue()) return false;  // map values follow keys
            }
        }
        ok_ = false;
        return false;
    }
    switch (m) {
        case 0:
        case 1:
        case 7:
            if (m == 7 && v == 31) return true;  // break
            return true;
        case 2:
        case 3: {
            if (v > len_ - pos_) {
                ok_ = false;
                return false;
            }
            pos_ += (size_t)v;
            return true;
        }
        case 4: {
            for (uint64_t i = 0; i < v; ++i) {
                if (!skipValue()) return false;
            }
            return true;
        }
        case 5: {
            for (uint64_t i = 0; i < v; ++i) {
                if (!skipValue()) return false;
                if (!skipValue()) return false;
            }
            return true;
        }
        case 6: {
            return skipValue();  // tagged value
        }
        default:
            break;
    }
    pos_ = save;
    ok_ = false;
    return false;
}

bool CborReader::skipValueSpan(size_t& offset, size_t& length) {
    const size_t start = pos_;
    if (!skipValue()) return false;
    offset = start;
    length = pos_ - start;
    return true;
}

}  // namespace om

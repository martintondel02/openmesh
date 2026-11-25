// SPDX-License-Identifier: AGPL-3.0-or-later
#include "openmesh/om_store.h"

#include <cstring>

namespace om {

namespace {

constexpr uint8_t kMagic[4] = {'O', 'M', 'S', 'Q'};

size_t recordBytes(const StoredEnvelope& e, uint32_t now_ignored, uint8_t* out, size_t capacity) {
    (void)now_ignored;
    if (capacity < StoreQueue::kRecordSize) return 0;
    size_t off = 0;
    out[off++] = e.used ? 1 : 0;
    std::memcpy(out + off, e.dst.b, 8);
    off += 8;
    std::memcpy(out + off, e.src.b, 8);
    off += 8;
    writeU32Be(out + off, e.msg_ref);
    off += 4;
    writeU32Be(out + off, e.seq);
    off += 4;
    writeU32Be(out + off, e.stored_at_ms);
    off += 4;
    writeU32Be(out + off, e.expires_at_ms);
    off += 4;
    out[off++] = e.frame_len;
    std::memcpy(out + off, e.frame, kMtu);
    off += kMtu;
    const uint32_t crc = crc32(out, off);
    writeU32Be(out + off, crc);
    off += 4;
    return off;
}

}  // namespace

uint32_t crc32(const uint8_t* data, size_t len) {
    uint32_t crc = 0xFFFFFFFFu;
    for (size_t i = 0; i < len; ++i) {
        crc ^= data[i];
        for (int bit = 0; bit < 8; ++bit) {
            const uint32_t mask = (uint32_t)(-(int32_t)(crc & 1u));
            crc = (crc >> 1) ^ (0xEDB88320u & mask);
        }
    }
    return ~crc;
}

void StoreQueue::clear() {
    for (size_t i = 0; i < kCapacity; ++i) entries_[i].used = false;
    count_ = 0;
    dirty_ = true;
}

bool StoreQueue::contains(const NodeId& dst, uint32_t msg_ref) const {
    for (size_t i = 0; i < kCapacity; ++i) {
        if (entries_[i].used && entries_[i].dst == dst && entries_[i].msg_ref == msg_ref)
            return true;
    }
    return false;
}

bool StoreQueue::retain(const Frame& frame, const NodeId& me, uint32_t msg_ref,
                        uint64_t now_ms) {
    if (!isStorable(frame.type)) return false;
    if (!isUnicastAddress(frame.dst)) return false;  // never store channel traffic
    if (frame.dst == me) return false;
    const uint32_t now32 = (uint32_t)(now_ms & 0xFFFFFFFFu);
    prune(now_ms);
    if (contains(frame.dst, msg_ref)) return false;
    size_t slot = kCapacity;
    for (size_t i = 0; i < kCapacity; ++i) {
        if (!entries_[i].used) {
            slot = i;
            break;
        }
    }
    if (slot == kCapacity) {
        // Full: evict the entry that expires first (oldest by store time).
        size_t oldest = 0;
        for (size_t i = 1; i < kCapacity; ++i) {
            if ((int32_t)(entries_[i].stored_at_ms - entries_[oldest].stored_at_ms) < 0)
                oldest = i;
        }
        slot = oldest;
        --count_;
    }
    StoredEnvelope& e = entries_[slot];
    e = StoredEnvelope{};
    e.used = true;
    e.dst = frame.dst;
    e.src = frame.src;
    e.msg_ref = msg_ref;
    e.seq = frame.seq;
    e.stored_at_ms = now32;
    e.expires_at_ms = now32 + ttl_ms_;
    const size_t frame_len = frame.encode(e.frame, sizeof(e.frame));
    if (frame_len == 0) return false;
    e.frame_len = (uint8_t)frame_len;
    ++count_;
    dirty_ = true;
    return true;
}

void StoreQueue::prune(uint64_t now_ms) {
    const uint32_t now32 = (uint32_t)(now_ms & 0xFFFFFFFFu);
    for (size_t i = 0; i < kCapacity; ++i) {
        if (entries_[i].used && expired(entries_[i], now32)) {
            entries_[i].used = false;
            if (count_ > 0) --count_;
            dirty_ = true;
        }
    }
}

size_t StoreQueue::idsFor(const NodeId& dst, uint32_t* ids, size_t max_ids) const {
    size_t n = 0;
    for (size_t i = 0; i < kCapacity && n < max_ids; ++i) {
        if (entries_[i].used && entries_[i].dst == dst) ids[n++] = entries_[i].msg_ref;
    }
    return n;
}

bool StoreQueue::take(const NodeId& dst, uint32_t msg_ref, StoredEnvelope& out) {
    for (size_t i = 0; i < kCapacity; ++i) {
        if (entries_[i].used && entries_[i].dst == dst && entries_[i].msg_ref == msg_ref) {
            out = entries_[i];
            entries_[i].used = false;
            if (count_ > 0) --count_;
            dirty_ = true;
            return true;
        }
    }
    return false;
}

size_t StoreQueue::serialize(uint8_t* out, size_t capacity) const {
    if (capacity < kSerializedSize) return 0;
    std::memcpy(out, kMagic, sizeof(kMagic));
    size_t off = sizeof(kMagic);
    for (size_t i = 0; i < kCapacity; ++i) {
        const size_t n = recordBytes(entries_[i], 0, out + off, capacity - off);
        if (n == 0) return 0;
        off += n;
    }
    return off;
}

bool StoreQueue::deserialize(const uint8_t* data, size_t len, uint64_t now_ms) {
    if (len < kSerializedSize) return false;
    if (std::memcmp(data, kMagic, sizeof(kMagic)) != 0) return false;
    count_ = 0;
    size_t off = sizeof(kMagic);
    for (size_t i = 0; i < kCapacity; ++i) {
        if (off + kRecordSize > len) return false;
        const uint8_t* rec = data + off;
        const uint32_t expected_crc = readU32Be(rec + kRecordSize - 4);
        const uint32_t actual_crc = crc32(rec, kRecordSize - 4);
        if (expected_crc != actual_crc) {
            entries_[i].used = false;
            off += kRecordSize;
            continue;
        }
        StoredEnvelope& e = entries_[i];
        e = StoredEnvelope{};
        e.used = rec[0] != 0;
        size_t p = 1;
        std::memcpy(e.dst.b, rec + p, 8);
        p += 8;
        std::memcpy(e.src.b, rec + p, 8);
        p += 8;
        e.msg_ref = readU32Be(rec + p);
        p += 4;
        e.seq = readU32Be(rec + p);
        p += 4;
        e.stored_at_ms = readU32Be(rec + p);
        p += 4;
        e.expires_at_ms = readU32Be(rec + p);
        p += 4;
        e.frame_len = rec[p++];
        std::memcpy(e.frame, rec + p, kMtu);
        p += kMtu;
        if (e.used) {
            if (e.frame_len < kHeaderLen || e.frame_len > kMtu) e.used = false;
            else ++count_;
        }
        off += kRecordSize;
    }
    dirty_ = false;
    prune(now_ms);
    return true;
}

}  // namespace om

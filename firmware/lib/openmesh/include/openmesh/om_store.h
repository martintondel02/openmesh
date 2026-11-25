// SPDX-License-Identifier: AGPL-3.0-or-later
// Store-and-forward queue (PROTOCOL.md §10): fixed-size, encrypted envelopes only, no heap.
//
// Relays retain complete original frame bytes so they can replay them verbatim with a fresh
// TTL (the AEAD AAD excludes the TTL, so the ciphertext survives the replay). Retention is
// keyed by (dst unicast node id, clear-text msg_ref). Announces/ACKs/offers/wants are never
// stored, and channel-addressed frames are never stored (address-class bit, §3).
#ifndef OPENMESH_OM_STORE_H
#define OPENMESH_OM_STORE_H

#include <cstddef>
#include <cstdint>

#include "openmesh/om_types.h"

#ifndef OM_STORE_CAPACITY
#define OM_STORE_CAPACITY 48
#endif

namespace om {

struct StoredEnvelope {
    bool used = false;
    NodeId dst{};
    NodeId src{};
    uint32_t msg_ref = 0;
    uint32_t seq = 0;
    uint32_t stored_at_ms = 0;   // low 32 bits of monotonic ms
    uint32_t expires_at_ms = 0;  // low 32 bits
    uint8_t frame_len = 0;
    uint8_t frame[kMtu]{};  // original frame bytes including header
};

class StoreQueue {
public:
    static constexpr size_t kCapacity = OM_STORE_CAPACITY;
    static constexpr size_t kRecordSize =
        1 + 8 + 8 + 4 + 4 + 4 + 4 + 1 + kMtu + 4;  // includes trailing CRC32
    static constexpr size_t kSerializedSize = 4 + kCapacity * kRecordSize;  // magic + records

    explicit StoreQueue(uint32_t ttl_ms = (uint32_t)kStoreTtlMs) : ttl_ms_(ttl_ms) {}

    void clear();
    // Retains the frame when it is storable, addressed to a unicast node other than `me`, and
    // not already queued. When full, the oldest envelope is evicted first (PROTOCOL.md §10).
    bool retain(const Frame& frame, const NodeId& me, uint32_t msg_ref, uint64_t now_ms);
    void prune(uint64_t now_ms);
    bool contains(const NodeId& dst, uint32_t msg_ref) const;
    size_t idsFor(const NodeId& dst, uint32_t* ids, size_t max_ids) const;
    // Removes and returns the retained envelope for (dst, msg_ref); false when absent.
    bool take(const NodeId& dst, uint32_t msg_ref, StoredEnvelope& out);
    size_t size() const { return count_; }
    size_t capacity() const { return kCapacity; }
    bool dirty() const { return dirty_; }
    void clearDirty() { dirty_ = false; }

    // Fixed-size serialisation for the board flash backend (little-endian fields, CRC32 per
    // record, magic "OMSQ" at the front).
    size_t serialize(uint8_t* out, size_t capacity) const;
    bool deserialize(const uint8_t* data, size_t len, uint64_t now_ms);

private:
    static bool expired(const StoredEnvelope& e, uint32_t now32) {
        return (int32_t)(now32 - e.expires_at_ms) >= 0;
    }
    StoredEnvelope entries_[kCapacity]{};
    size_t count_ = 0;
    uint32_t ttl_ms_ = 0;
    bool dirty_ = false;
};

uint32_t crc32(const uint8_t* data, size_t len);

}  // namespace om

#endif  // OPENMESH_OM_STORE_H

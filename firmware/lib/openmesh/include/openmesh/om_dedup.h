// SPDX-License-Identifier: AGPL-3.0-or-later
// Flood duplicate-suppression cache (PROTOCOL.md §11): fixed-size ring, no heap.
//
// Key: (src, seq). Entries expire after kDedupTtlMs; capacity is at least 2048 per spec and
// configurable with -DOM_DEDUP_CAPACITY for memory-constrained builds.
#ifndef OPENMESH_OM_DEDUP_H
#define OPENMESH_OM_DEDUP_H

#include <cstddef>
#include <cstdint>

#include "openmesh/om_types.h"

#ifndef OM_DEDUP_CAPACITY
#define OM_DEDUP_CAPACITY 2048
#endif

namespace om {

class DedupCache {
public:
    static constexpr size_t kCapacity = OM_DEDUP_CAPACITY;

    struct Entry {
        NodeId src{};
        uint32_t seq = 0;
        uint32_t stamp_ms = 0;  // low 32 bits of monotonic milliseconds
        bool used = false;
    };

    static constexpr size_t kBytes = kCapacity * sizeof(Entry);

    explicit DedupCache(uint64_t ttl_ms = kDedupTtlMs) : ttl_ms_(ttl_ms) {}

    void clear();
    // Returns true when the (src, seq) pair is fresh (and records it); false when it is a
    // duplicate already seen within the TTL window.
    bool checkAndInsert(const NodeId& src, uint32_t seq, uint64_t now_ms);

    size_t size() const { return count_; }
    size_t capacity() const { return kCapacity; }
    uint64_t ttlMs() const { return ttl_ms_; }

private:
    static bool newerThan(uint32_t a, uint32_t b) {
        // Wrap-safe comparison of 32-bit millisecond stamps.
        return (int32_t)(a - b) > 0;
    }
    bool fresh(const Entry& e, uint32_t now32) const {
        const uint32_t age = now32 - e.stamp_ms;
        return (int32_t)age >= 0 && (uint64_t)age <= ttl_ms_;
    }

    Entry entries_[kCapacity]{};
    size_t head_ = 0;
    size_t count_ = 0;
    uint64_t ttl_ms_;
};

}  // namespace om

#endif  // OPENMESH_OM_DEDUP_H

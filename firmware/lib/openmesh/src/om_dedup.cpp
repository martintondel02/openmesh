// SPDX-License-Identifier: AGPL-3.0-or-later
#include "openmesh/om_dedup.h"

namespace om {

void DedupCache::clear() {
    for (size_t i = 0; i < kCapacity; ++i) entries_[i].used = false;
    head_ = 0;
    count_ = 0;
}

bool DedupCache::checkAndInsert(const NodeId& src, uint32_t seq, uint64_t now_ms) {
    const uint32_t now32 = (uint32_t)(now_ms & 0xFFFFFFFFu);
    // Purge the slot we are about to overwrite if it is stale; look for a live duplicate.
    for (size_t i = 0; i < count_; ++i) {
        const Entry& e = entries_[i];
        if (!e.used) continue;
        if (e.src == src && e.seq == seq && fresh(e, now32)) return false;
    }
    Entry& slot = entries_[head_];
    slot.src = src;
    slot.seq = seq;
    slot.stamp_ms = now32;
    slot.used = true;
    head_ = (head_ + 1) % kCapacity;
    if (count_ < kCapacity) ++count_;
    return true;
}

}  // namespace om

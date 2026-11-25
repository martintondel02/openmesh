//! Duplicate suppression, flood control and peer bookkeeping.
//!
//! OMP/1 uses managed flooding (like Meshtastic): every relayed frame is rebroadcast once per
//! node, deduplicated by `(src, seq)`. Directed routing is reserved for OMP/2; the
//! [`RouteTable`] stub documents the intended interface so the firmware and host agree on the
//! future shape.

use std::collections::HashMap;

use crate::wire::NodeId;

/// Fixed-window duplicate suppression cache.
#[derive(Debug)]
pub struct DedupCache {
    capacity: usize,
    ttl_ms: u64,
    entries: HashMap<(NodeId, u32), u64>,
}

impl DedupCache {
    pub fn new(capacity: usize, ttl_ms: u64) -> Self {
        Self { capacity, ttl_ms, entries: HashMap::with_capacity(capacity) }
    }

    /// Insert `(src, seq)`. Returns `true` when the frame is new (i.e. should be processed).
    pub fn check_and_insert(&mut self, src: NodeId, seq: u32, now_ms: u64) -> bool {
        if self.entries.contains_key(&(src, seq)) {
            return false;
        }
        if self.entries.len() >= self.capacity {
            self.prune(now_ms);
            if self.entries.len() >= self.capacity {
                // Still full: drop the oldest quarter rather than allocate unboundedly.
                let mut keys: Vec<(NodeId, u32)> = self.entries.keys().copied().collect();
                keys.sort_unstable();
                for key in keys.iter().take(self.capacity / 4 + 1) {
                    self.entries.remove(key);
                }
            }
        }
        self.entries.insert((src, seq), now_ms);
        true
    }

    pub fn prune(&mut self, now_ms: u64) {
        let ttl = self.ttl_ms;
        self.entries.retain(|_, seen| now_ms.saturating_sub(*seen) <= ttl);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Simple token bucket used to respect airtime budgets (EU 1% duty cycle, fair-use elsewhere).
#[derive(Debug)]
pub struct AirtimeBudget {
    capacity_ms: u32,
    refill_per_min_ms: u32,
    tokens_ms: f64,
    last_refill_ms: u64,
}

impl AirtimeBudget {
    pub fn new(refill_per_min_ms: u32) -> Self {
        Self {
            capacity_ms: refill_per_min_ms,
            refill_per_min_ms,
            tokens_ms: refill_per_min_ms as f64,
            last_refill_ms: 0,
        }
    }

    fn refill(&mut self, now_ms: u64) {
        let elapsed = now_ms.saturating_sub(self.last_refill_ms);
        let refill = (elapsed as f64 / 60_000.0) * self.refill_per_min_ms as f64;
        self.tokens_ms = (self.tokens_ms + refill).min(self.capacity_ms as f64);
        self.last_refill_ms = now_ms;
    }

    /// Try to spend `airtime_ms`. Returns `false` when the budget is exhausted.
    pub fn try_spend(&mut self, airtime_ms: u32, now_ms: u64) -> bool {
        self.refill(now_ms);
        let cost = airtime_ms as f64;
        if self.tokens_ms >= cost {
            self.tokens_ms -= cost;
            true
        } else {
            false
        }
    }

    pub fn available_ms(&mut self, now_ms: u64) -> f64 {
        self.refill(now_ms);
        self.tokens_ms
    }
}

/// Reserved for OMP/2 directed path routing (AODV-style source routes over flooded traces).
#[derive(Debug, Default)]
pub struct RouteTable;

impl RouteTable {
    pub fn is_empty(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_rejects_duplicates() {
        let mut cache = DedupCache::new(64, 60_000);
        let a = NodeId::new([1; 8]);
        assert!(cache.check_and_insert(a, 1, 0));
        assert!(!cache.check_and_insert(a, 1, 10));
        assert!(cache.check_and_insert(a, 2, 10));
        assert!(cache.check_and_insert(NodeId::new([2; 8]), 1, 10));
    }

    #[test]
    fn dedup_expires_entries() {
        let mut cache = DedupCache::new(64, 1_000);
        let a = NodeId::new([1; 8]);
        assert!(cache.check_and_insert(a, 1, 0));
        cache.prune(2_000);
        assert!(cache.check_and_insert(a, 1, 2_000));
    }

    #[test]
    fn airtime_budget_refills() {
        let mut b = AirtimeBudget::new(1_000);
        assert!(b.try_spend(600, 0));
        assert!(b.try_spend(400, 0));
        assert!(!b.try_spend(1, 0));
        assert!(b.try_spend(500, 60_000));
    }
}

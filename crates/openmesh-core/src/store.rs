//! Store-and-forward queue used by repeater and gateway nodes.
//!
//! Relays retain *encrypted* envelopes only: they never learn message contents. A logical
//! message consists of one or more `Data`/`Resource` frames that all carry the same clear-text
//! `msg_ref` (PROTOCOL.md §5, §10); the queue groups frames by `(dst, msg_ref)` so an offer
//! carries one id and delivery replays every frame of that message. See
//! [`crate::engine::Engine`] for the offer/want/replay handshake.

use crate::wire::NodeId;

/// One retained frame of an envelope.
#[derive(Clone, Debug)]
pub struct StoredEnvelope {
    /// Clear-text `msg_ref` shared by all frames of the message.
    pub msg_id: u32,
    /// Addressee.
    pub dst: NodeId,
    /// Original sender (needed to rebuild the relay frame).
    pub src: NodeId,
    /// Frame type (`Data` or `Resource`).
    pub frame_type: u8,
    /// Header flags carried by the original frame.
    pub flags: u8,
    /// Original sequence number.
    pub seq: u32,
    /// Encrypted body, replayed verbatim.
    pub body: Vec<u8>,
    /// When the first frame of this envelope was stored.
    pub stored_at_ms: u64,
    /// Absolute eviction time.
    pub expires_at_ms: u64,
}

/// Bounded queue with per-`(dst, msg_ref)` grouping and TTL eviction.
#[derive(Debug)]
pub struct StoreQueue {
    capacity: usize,
    ttl_ms: u64,
    entries: Vec<StoredEnvelope>,
}

impl StoreQueue {
    /// Create a queue holding at most `capacity` logical envelopes for `ttl_ms` milliseconds.
    pub fn new(capacity: usize, ttl_ms: u64) -> Self {
        Self {
            capacity,
            ttl_ms,
            entries: Vec::with_capacity(capacity.min(1024)),
        }
    }

    /// Number of retained *frames* (for statistics).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no frames are retained.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of distinct logical envelopes (offer ids) in the queue.
    pub fn group_count(&self) -> usize {
        let mut keys: Vec<(NodeId, u32)> = self.group_keys();
        keys.sort_unstable();
        keys.dedup();
        keys.len()
    }

    fn group_keys(&self) -> Vec<(NodeId, u32)> {
        self.entries.iter().map(|e| (e.dst, e.msg_id)).collect()
    }

    /// Retain a frame without evicting: returns `false` when the queue is full or the frame is
    /// already stored. Kept for embedders that manage eviction themselves.
    pub fn push(&mut self, mut envelope: StoredEnvelope, now_ms: u64) -> bool {
        self.prune(now_ms);
        let is_new_group = !self.contains(envelope.dst, envelope.msg_id);
        if is_new_group && self.group_count() >= self.capacity {
            return false;
        }
        if self.entries.iter().any(|e| {
            e.dst == envelope.dst
                && e.msg_id == envelope.msg_id
                && e.src == envelope.src
                && e.seq == envelope.seq
        }) {
            return false;
        }
        envelope.stored_at_ms = now_ms;
        envelope.expires_at_ms = now_ms.saturating_add(self.ttl_ms);
        self.entries.push(envelope);
        true
    }

    /// Retain a frame, evicting the oldest logical envelope when the queue is full
    /// (PROTOCOL.md §10). Returns `false` for duplicate frames or a zero-capacity queue.
    pub fn push_evicting(&mut self, mut envelope: StoredEnvelope, now_ms: u64) -> bool {
        if self.capacity == 0 {
            return false;
        }
        self.prune(now_ms);
        let key = (envelope.dst, envelope.msg_id);
        if self
            .entries
            .iter()
            .any(|e| (e.dst, e.msg_id) == key && e.src == envelope.src && e.seq == envelope.seq)
        {
            return false;
        }
        let group_stored_at = self
            .entries
            .iter()
            .filter(|e| (e.dst, e.msg_id) == key)
            .map(|e| e.stored_at_ms)
            .min();
        let stored_at = group_stored_at.unwrap_or(now_ms);
        if group_stored_at.is_none() && self.group_count() >= self.capacity {
            self.evict_oldest_group();
        }
        envelope.stored_at_ms = stored_at;
        envelope.expires_at_ms = stored_at.saturating_add(self.ttl_ms);
        self.entries.push(envelope);
        true
    }

    /// Remove the logical envelope whose oldest frame was stored first.
    pub fn evict_oldest_group(&mut self) -> Option<(NodeId, u32)> {
        let oldest = self
            .entries
            .iter()
            .min_by_key(|e| e.stored_at_ms)
            .map(|e| (e.dst, e.msg_id))?;
        self.entries.retain(|e| (e.dst, e.msg_id) != oldest);
        Some(oldest)
    }

    /// Distinct ids held for `dst`, in insertion order.
    pub fn ids_for(&self, dst: NodeId) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for e in &self.entries {
            if e.dst == dst && !out.contains(&e.msg_id) {
                out.push(e.msg_id);
            }
        }
        out
    }

    /// Up to `limit` ids held for `dst`, in insertion order.
    pub fn ids_for_limited(&self, dst: NodeId, limit: usize) -> Vec<u32> {
        let mut out = self.ids_for(dst);
        out.truncate(limit);
        out
    }

    /// Number of logical envelopes held for `dst`.
    pub fn count_for(&self, dst: NodeId) -> usize {
        self.ids_for(dst).len()
    }

    /// Remove and return all frames for the given ids addressed to `dst`.
    pub fn take(&mut self, dst: NodeId, ids: &[u32]) -> Vec<StoredEnvelope> {
        let mut taken = Vec::new();
        let mut i = 0;
        while i < self.entries.len() {
            let matches = self.entries[i].dst == dst && ids.contains(&self.entries[i].msg_id);
            if matches {
                taken.push(self.entries.remove(i));
            } else {
                i += 1;
            }
        }
        taken
    }

    /// Clone the frames for the given ids addressed to `dst` without removing them.
    pub fn frames_for(&self, dst: NodeId, ids: &[u32]) -> Vec<StoredEnvelope> {
        self.entries
            .iter()
            .filter(|e| e.dst == dst && ids.contains(&e.msg_id))
            .cloned()
            .collect()
    }

    /// Drop expired frames.
    pub fn prune(&mut self, now_ms: u64) {
        self.entries.retain(|e| e.expires_at_ms > now_ms);
    }

    /// Whether a frame of the logical envelope `(dst, msg_id)` is retained.
    pub fn contains(&self, dst: NodeId, msg_id: u32) -> bool {
        self.entries
            .iter()
            .any(|e| e.dst == dst && e.msg_id == msg_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(dst: NodeId, msg_id: u32, seq: u32) -> StoredEnvelope {
        StoredEnvelope {
            msg_id,
            dst,
            src: NodeId::new([9; 8]),
            frame_type: 1,
            flags: 1,
            seq,
            body: vec![1, 2, 3],
            stored_at_ms: 0,
            expires_at_ms: 0,
        }
    }

    #[test]
    fn push_and_take_per_destination() {
        let dst = NodeId::new([7; 8]);
        let mut q = StoreQueue::new(8, 60_000);
        assert!(q.push(envelope(dst, 1, 1), 0));
        assert!(q.push(envelope(dst, 2, 2), 0));
        // A second frame of the same logical envelope (different seq) is accepted; an exact
        // duplicate frame is rejected.
        assert!(q.push(envelope(dst, 1, 3), 0));
        assert!(!q.push(envelope(dst, 1, 3), 0));
        assert_eq!(q.ids_for(dst), vec![1, 2]);
        let taken = q.take(dst, &[2]);
        assert_eq!(taken.len(), 1);
        assert!(q.contains(dst, 1));
        // Message 1 now has two frames (the original plus the extra frame pushed above).
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn ttl_eviction() {
        let dst = NodeId::new([7; 8]);
        let mut q = StoreQueue::new(8, 1_000);
        assert!(q.push(envelope(dst, 1, 1), 0));
        q.prune(1_001);
        assert!(q.is_empty());
    }

    #[test]
    fn capacity_is_respected_when_pushing() {
        let mut q = StoreQueue::new(2, 60_000);
        assert!(q.push(envelope(NodeId::new([1; 8]), 1, 1), 0));
        assert!(q.push(envelope(NodeId::new([1; 8]), 2, 2), 0));
        assert!(!q.push(envelope(NodeId::new([1; 8]), 3, 3), 0));
    }

    #[test]
    fn capacity_evicts_oldest_group() {
        let dst = NodeId::new([1; 8]);
        let mut q = StoreQueue::new(2, 60_000);
        assert!(q.push_evicting(envelope(dst, 1, 1), 10));
        assert!(q.push_evicting(envelope(dst, 2, 2), 20));
        assert!(q.push_evicting(envelope(dst, 3, 3), 30));
        assert!(!q.contains(dst, 1));
        assert!(q.contains(dst, 2));
        assert!(q.contains(dst, 3));
        assert_eq!(q.group_count(), 2);
    }

    #[test]
    fn frames_of_one_message_share_a_group_and_ttl() {
        let dst = NodeId::new([2; 8]);
        let mut q = StoreQueue::new(4, 1_000);
        assert!(q.push_evicting(envelope(dst, 5, 100), 100));
        assert!(q.push_evicting(envelope(dst, 5, 101), 200));
        assert!(q.push_evicting(envelope(dst, 5, 102), 300));
        assert_eq!(q.group_count(), 1);
        assert_eq!(q.len(), 3);
        assert_eq!(q.frames_for(dst, &[5]).len(), 3);
        // Group expiry is anchored at the first frame (100 ms + 1000 ms).
        q.prune(1_099);
        assert!(!q.is_empty());
        q.prune(1_101);
        assert!(q.is_empty());
        // Duplicate frames are rejected, new frames for an existing group are accepted.
        assert!(q.push_evicting(envelope(dst, 6, 103), 0));
        assert!(!q.push_evicting(envelope(dst, 6, 103), 0));
        assert!(q.push_evicting(envelope(dst, 6, 104), 0));
        assert_eq!(q.frames_for(dst, &[6]).len(), 2);
    }

    #[test]
    fn zero_capacity_stores_nothing() {
        let mut q = StoreQueue::new(0, 60_000);
        assert!(!q.push_evicting(envelope(NodeId::new([1; 8]), 1, 1), 0));
        assert!(!q.push(envelope(NodeId::new([1; 8]), 1, 1), 0));
    }
}

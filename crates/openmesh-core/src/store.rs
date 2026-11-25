//! Store-and-forward queue used by repeater and gateway nodes.
//!
//! Relays retain *encrypted* envelopes only: they never learn message contents or senders.
//! Delivery works as an offer/want handshake (see `PROTOCOL.md` §Store-and-forward) so that
//! reconnections never duplicate or re-transmit the whole queue.

use crate::wire::NodeId;

/// One retained envelope.
#[derive(Clone, Debug)]
pub struct StoredEnvelope {
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
    /// Encrypted body.
    pub body: Vec<u8>,
    pub stored_at_ms: u64,
    pub expires_at_ms: u64,
}

/// Bounded queue with per-addressee indexing and TTL eviction.
#[derive(Debug)]
pub struct StoreQueue {
    capacity: usize,
    ttl_ms: u64,
    entries: Vec<StoredEnvelope>,
}

impl StoreQueue {
    pub fn new(capacity: usize, ttl_ms: u64) -> Self {
        Self { capacity, ttl_ms, entries: Vec::with_capacity(capacity) }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Retain an envelope. Returns `false` when the queue is full or the id is a duplicate.
    pub fn push(&mut self, mut envelope: StoredEnvelope, now_ms: u64) -> bool {
        self.prune(now_ms);
        if self.entries.len() >= self.capacity {
            return false;
        }
        if self.entries.iter().any(|e| e.msg_id == envelope.msg_id && e.dst == envelope.dst) {
            return false;
        }
        envelope.stored_at_ms = now_ms;
        envelope.expires_at_ms = now_ms + self.ttl_ms;
        self.entries.push(envelope);
        true
    }

    /// Ids held for `dst`.
    pub fn ids_for(&self, dst: NodeId) -> Vec<u32> {
        self.entries.iter().filter(|e| e.dst == dst).map(|e| e.msg_id).collect()
    }

    /// Take (remove) the envelopes for `dst` matching `ids`.
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

    pub fn prune(&mut self, now_ms: u64) {
        self.entries.retain(|e| e.expires_at_ms > now_ms);
    }

    pub fn contains(&self, dst: NodeId, msg_id: u32) -> bool {
        self.entries.iter().any(|e| e.dst == dst && e.msg_id == msg_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(dst: NodeId, msg_id: u32) -> StoredEnvelope {
        StoredEnvelope {
            msg_id,
            dst,
            src: NodeId::new([9; 8]),
            frame_type: 1,
            flags: 1,
            seq: msg_id,
            body: vec![1, 2, 3],
            stored_at_ms: 0,
            expires_at_ms: 0,
        }
    }

    #[test]
    fn push_and_take_per_destination() {
        let dst = NodeId::new([7; 8]);
        let mut q = StoreQueue::new(8, 60_000);
        assert!(q.push(envelope(dst, 1), 0));
        assert!(q.push(envelope(dst, 2), 0));
        assert!(!q.push(envelope(dst, 1), 0));
        assert_eq!(q.ids_for(dst), vec![1, 2]);
        let taken = q.take(dst, &[2]);
        assert_eq!(taken.len(), 1);
        assert!(q.contains(dst, 1));
    }

    #[test]
    fn ttl_eviction() {
        let dst = NodeId::new([7; 8]);
        let mut q = StoreQueue::new(8, 1_000);
        assert!(q.push(envelope(dst, 1), 0));
        q.prune(1_001);
        assert!(q.is_empty());
    }

    #[test]
    fn capacity_is_respected() {
        let mut q = StoreQueue::new(2, 60_000);
        assert!(q.push(envelope(NodeId::new([1; 8]), 1), 0));
        assert!(q.push(envelope(NodeId::new([1; 8]), 2), 0));
        assert!(!q.push(envelope(NodeId::new([1; 8]), 3), 0));
    }
}

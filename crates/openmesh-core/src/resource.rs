//! Resource transfer ARQ (selective repeat) used for attachments, long text and history.
//!
//! A resource is a single AEAD-sealed blob (see PROTOCOL.md §9) split into
//! [`DEFAULT_CHUNK_BYTES`]-sized chunks. The sender streams a window of chunks, retransmits
//! holes reported by acknowledgement bitmaps, backs off exponentially when no ack arrives and
//! gives up after [`MAX_RETRIES`]. The receiver tracks a chunk bitmap, surfaces progress and
//! completeness, and hands the verified bytes back to the engine.
//!
//! This module is transport-free and time-free: the engine supplies `now_ms` and turns the
//! returned indices into `Resource` frames.

use crate::error::{Error, Result};
use crate::fragment::Reassembler;

/// Default number of chunks in flight.
pub const DEFAULT_WINDOW: u16 = 8;
/// Hard ceiling on consecutive retransmissions of the same window (safety only; the primary
/// give-up condition is the no-progress time budget in [`ResourceSender::schedule_retry`]).
pub const MAX_RETRIES: u8 = 32;
/// Default chunk payload size in bytes (PROTOCOL.md §9.2).
pub const DEFAULT_CHUNK_BYTES: u16 = 180;
/// Maximum chunk payload that fits in one frame body after the resource header.
pub const MAX_CHUNK_BYTES: u16 = 207;
/// Length of the fixed resource chunk header: flags(1) + msg_ref(4) + res_id(4) + index(2).
pub const CHUNK_HEADER_LEN: usize = 11;
/// Domain separator for resource blob AEAD associated data.
pub const RESOURCE_AAD_PREFIX: &[u8] = b"openmesh/v1/resource";

/// Associated data for a resource's sealed blob (PROTOCOL.md §9.1, 25 bytes).
///
/// `"openmesh/v1/resource" || res_id(4 BE) || msg_id(4 BE) || total_len(4 BE) || chunk_bytes(2
/// BE)`. `sha256` is deliberately excluded: it is computed over the sealed blob, so it cannot
/// exist before sealing (circular). Everything else from the manifest is bound here so a relay
/// cannot re-frame a blob under a different resource/message/length/chunk size.
pub fn resource_aad(res_id: u32, msg_id: u32, total_len: u32, chunk_bytes: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(RESOURCE_AAD_PREFIX.len() + 14);
    out.extend_from_slice(RESOURCE_AAD_PREFIX);
    out.extend_from_slice(&res_id.to_be_bytes());
    out.extend_from_slice(&msg_id.to_be_bytes());
    out.extend_from_slice(&total_len.to_be_bytes());
    out.extend_from_slice(&chunk_bytes.to_be_bytes());
    out
}

/// Number of chunks for a sealed blob, validating the size policy and field ranges.
pub fn chunk_count(total_len: u32, chunk_bytes: u16) -> Result<u16> {
    if chunk_bytes == 0 || chunk_bytes > 1024 {
        return Err(Error::Config("invalid chunk size"));
    }
    let count = (total_len as u64).div_ceil(chunk_bytes as u64);
    if count > u16::MAX as u64 {
        return Err(Error::PayloadTooLarge {
            size: total_len as usize,
            max: u16::MAX as usize * chunk_bytes as usize,
        });
    }
    Ok(count as u16)
}

/// Build the acknowledgement bitmap for `next_expected..next_expected+32`.
///
/// Bit `0` reports chunk `next_expected + 1`, bit `31` reports `next_expected + 32`
/// (PROTOCOL.md §8). Bits beyond the transfer are zero.
pub fn ack_bitmap(total_chunks: u16, next_expected: u16, has: impl Fn(u16) -> bool) -> u32 {
    let mut bitmap = 0u32;
    for bit in 0..32u16 {
        let Some(idx) = next_expected
            .checked_add(1)
            .and_then(|v| v.checked_add(bit))
        else {
            break;
        };
        if idx < total_chunks && has(idx) {
            bitmap |= 1 << bit;
        }
    }
    bitmap
}

/// Sender-side state for one outgoing resource.
#[derive(Debug)]
pub struct ResourceSender {
    /// Resource id being transferred.
    pub res_id: u32,
    /// Total chunks in the sealed blob.
    pub total_chunks: u16,
    /// Bytes per chunk.
    pub chunk_bytes: u16,
    /// Chunks allowed in flight before waiting for an ack.
    pub window: u16,
    /// Lowest chunk index that has never been transmitted.
    pub next_unsent: u16,
    /// Retransmissions since the last ack (reset by [`ResourceSender::apply_ack`]).
    pub retries: u8,
    /// Chunks transmitted at least once.
    pub sent_mask: Vec<bool>,
    /// Chunks acknowledged by the peer.
    pub acked: Vec<bool>,
    /// Absolute time (ms) at which the next retransmission is due.
    pub deadline_ms: u64,
    /// Wall time (ms) of the last ack that advanced the transfer (retry budget anchor).
    pub last_progress_ms: u64,
    /// Wall time (ms) at which the resource was armed.
    pub started_ms: u64,
}

impl ResourceSender {
    /// Create sender state for `total_chunks` chunks of `chunk_storage_bytes` bytes each.
    pub fn new(res_id: u32, total_chunks: u16, chunk_storage_bytes: usize) -> Self {
        Self {
            res_id,
            total_chunks,
            chunk_bytes: (chunk_storage_bytes as u16).min(1024),
            window: DEFAULT_WINDOW,
            next_unsent: 0,
            retries: 0,
            sent_mask: vec![false; total_chunks as usize],
            acked: vec![false; total_chunks as usize],
            deadline_ms: 0,
            last_progress_ms: 0,
            started_ms: 0,
        }
    }

    /// True when every chunk has been acknowledged.
    pub fn is_complete(&self) -> bool {
        self.acked.iter().all(|a| *a)
    }

    /// Number of acknowledged chunks.
    pub fn acked_count(&self) -> u16 {
        self.acked.iter().filter(|a| **a).count() as u16
    }

    /// Chunk indices to transmit or retransmit right now.
    ///
    /// Returns the first [`ResourceSender::window`] unacknowledged chunks, regardless of
    /// whether they were already sent: this is what the retransmission timer emits.
    pub fn pending_window(&self) -> Vec<u16> {
        let mut out = Vec::new();
        for i in 0..self.total_chunks {
            if !self.acked[i as usize] && out.len() < self.window as usize {
                out.push(i);
            }
        }
        out
    }

    /// Chunks that have never been sent and are not yet acked, up to `limit` and the window size.
    ///
    /// Marks them as sent and advances [`ResourceSender::next_unsent`]. The engine passes a small
    /// limit for paced emission so a whole window never leaves in one burst.
    pub fn take_unsent(&mut self, limit: usize) -> Vec<u16> {
        // Never exceed the window of chunks in flight: count those sent but not yet acked.
        let in_flight = self
            .sent_mask
            .iter()
            .zip(self.acked.iter())
            .filter(|(sent, acked)| **sent && !**acked)
            .count();
        let room = (self.window as usize)
            .saturating_sub(in_flight)
            .min(limit.max(1));
        let mut out = Vec::new();
        let mut i = self.next_unsent;
        while i < self.total_chunks && out.len() < room {
            if !self.sent_mask[i as usize] && !self.acked[i as usize] {
                self.sent_mask[i as usize] = true;
                out.push(i);
            }
            i += 1;
        }
        while self.next_unsent < self.total_chunks
            && (self.sent_mask[self.next_unsent as usize] || self.acked[self.next_unsent as usize])
        {
            self.next_unsent += 1;
        }
        out
    }

    /// Apply an acknowledgement window; returns `true` when the transfer is finished.
    pub fn apply_ack(&mut self, next_expected: u16, bitmap: u32) -> Result<bool> {
        if next_expected > self.total_chunks {
            return Err(Error::Malformed("ack window out of range"));
        }
        self.retries = 0;
        for i in 0..next_expected.min(self.total_chunks) {
            self.acked[i as usize] = true;
            self.sent_mask[i as usize] = true;
        }
        for bit in 0..32u16 {
            let Some(idx) = next_expected
                .checked_add(1)
                .and_then(|v| v.checked_add(bit))
            else {
                break;
            };
            if idx < self.total_chunks && bitmap & (1 << bit) != 0 {
                self.acked[idx as usize] = true;
                self.sent_mask[idx as usize] = true;
            }
        }
        if next_expected > self.next_unsent {
            self.next_unsent = next_expected.min(self.total_chunks);
        }
        while self.next_unsent < self.total_chunks
            && (self.sent_mask[self.next_unsent as usize] || self.acked[self.next_unsent as usize])
        {
            self.next_unsent += 1;
        }
        Ok(self.is_complete())
    }

    /// Arm the retransmission timer before the first transmission.
    pub fn arm(&mut self, now_ms: u64, rto_ms: u64) {
        self.started_ms = now_ms;
        self.last_progress_ms = now_ms;
        self.deadline_ms = now_ms.saturating_add(rto_ms);
    }

    /// Apply an ack and reschedule the retransmission timer.
    ///
    /// Returns `true` when complete. `rto_ms` is the base retransmission timeout. An ack that
    /// advances the transfer resets the retry budget anchor, so a slow but progressing link is
    /// never abandoned.
    pub fn on_ack(
        &mut self,
        next_expected: u16,
        bitmap: u32,
        now_ms: u64,
        rto_ms: u64,
    ) -> Result<bool> {
        let before = self.acked_count();
        let complete = self.apply_ack(next_expected, bitmap)?;
        let after = self.acked_count();
        if complete {
            self.deadline_ms = 0;
            self.last_progress_ms = now_ms;
            return Ok(true);
        }
        if after > before {
            self.last_progress_ms = now_ms;
        }
        self.deadline_ms = now_ms.saturating_add(rto_ms);
        Ok(false)
    }

    /// Schedule a retransmission after an RTO fired.
    ///
    /// Returns the new deadline, or `None` when the transfer is abandoned. Give-up is
    /// **time-based**: after `no_progress_budget_ms` wall time without an acknowledged chunk.
    /// This is what lets a duty-cycle-paced transfer survive: its first ack can take longer than
    /// any fixed retry count would allow. Backoff doubles the base timeout for every retry and is
    /// capped at `max_backoff_ms`, so a lost link is still probed occasionally without flooding.
    pub fn schedule_retry(
        &mut self,
        now_ms: u64,
        rto_ms: u64,
        no_progress_budget_ms: u64,
        max_backoff_ms: u64,
    ) -> Option<u64> {
        self.schedule_retry_with_cap(
            now_ms,
            rto_ms,
            no_progress_budget_ms,
            max_backoff_ms,
            MAX_RETRIES,
        )
    }

    /// [`ResourceSender::schedule_retry`] with an explicit no-progress retry cap.
    ///
    /// Companion radios use a much smaller cap (and shorter budget/backoff passed by the engine) so
    /// a failed media transfer cannot keep retrying for minutes and starve channel text.
    pub fn schedule_retry_with_cap(
        &mut self,
        now_ms: u64,
        rto_ms: u64,
        no_progress_budget_ms: u64,
        max_backoff_ms: u64,
        max_retries: u8,
    ) -> Option<u64> {
        if now_ms.saturating_sub(self.last_progress_ms) > no_progress_budget_ms {
            return None;
        }
        if self.retries >= max_retries {
            return None;
        }
        self.retries += 1;
        let shift = self.retries.saturating_sub(1).min(6) as u32;
        let backoff = rto_ms.saturating_mul(1u64 << shift).min(max_backoff_ms);
        let deadline = now_ms.saturating_add(backoff);
        self.deadline_ms = deadline;
        Some(deadline)
    }
}

/// Receiver-side state for one incoming resource.
#[derive(Debug)]
pub struct ResourceReceiver {
    /// Resource id being received.
    pub res_id: u32,
    /// Sealed blob length in bytes.
    pub total_len: u32,
    /// Chunk reassembly buffer.
    pub reassembler: Reassembler,
    /// Bytes per chunk from the manifest.
    pub chunk_bytes: u16,
}

impl ResourceReceiver {
    /// Create receiver state; validates the chunking policy.
    pub fn new(res_id: u32, total_len: u32, chunk_bytes: usize) -> Result<Self> {
        if chunk_bytes == 0 || chunk_bytes > 1024 {
            return Err(Error::Config("invalid chunk size"));
        }
        let reassembler = Reassembler::new(total_len, chunk_bytes)?;
        Ok(Self {
            res_id,
            total_len,
            reassembler,
            chunk_bytes: chunk_bytes as u16,
        })
    }

    /// Number of chunks received so far.
    pub fn received(&self) -> u16 {
        self.reassembler.received_chunks()
    }

    /// Total number of chunks expected.
    pub fn total(&self) -> u16 {
        self.reassembler.total_chunks()
    }

    /// Whether every chunk is present.
    pub fn is_complete(&self) -> bool {
        self.reassembler.is_complete()
    }

    /// Insert a chunk; returns `true` when it was new.
    pub fn insert_chunk(&mut self, index: u16, data: &[u8]) -> Result<bool> {
        self.reassembler.insert(index, data)
    }

    /// Build the acknowledgement window (next expected chunk + 32-bit bitmap).
    pub fn ack_window(&self) -> (u16, u32) {
        let next = self.reassembler.next_missing().unwrap_or(self.total());
        let bitmap = ack_bitmap(self.total(), next, |i| self.reassembler.has(i));
        (next, bitmap)
    }

    /// Consume the receiver and return the reassembled sealed blob.
    pub fn into_blob(self) -> Result<Vec<u8>> {
        self.reassembler.assemble()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sender_streams_a_window_then_acks() {
        let mut s = ResourceSender::new(1, 20, 180);
        let window = s.window as usize;
        assert_eq!(s.take_unsent(window), vec![0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(s.take_unsent(window), Vec::<u16>::new());
        // Ack the first window (next expected = 8, no extra bitmap bits).
        assert!(!s.apply_ack(8, 0).unwrap());
        assert_eq!(s.take_unsent(window), vec![8, 9, 10, 11, 12, 13, 14, 15]);
        assert!(!s.is_complete());
        assert_eq!(s.acked_count(), 8);
    }

    #[test]
    fn bitmap_marks_out_of_order_receipts() {
        let mut s = ResourceSender::new(1, 20, 180);
        let _ = s.take_unsent(8);
        // Receiver has 0,1,2 and 4..8 (chunk 3 missing): next = 3, bits for 4..8.
        let mut bitmap = 0u32;
        for idx in 4..=8u16 {
            bitmap |= 1 << (idx - 4);
        }
        s.apply_ack(3, bitmap).unwrap();
        assert_eq!(s.acked_count(), 8);
        assert!(!s.acked[3]);
        assert_eq!(s.pending_window(), vec![3, 9, 10, 11, 12, 13, 14, 15]);
    }

    #[test]
    fn retries_back_off_and_give_up() {
        let mut s = ResourceSender::new(1, 8, 180);
        const BUDGET: u64 = 600_000;
        const MAX_BACKOFF: u64 = 60_000;
        s.arm(0, 3_000);
        assert!(!s.on_ack(0, 0, 0, 3_000).unwrap());
        assert_eq!(s.deadline_ms, 3_000);
        assert_eq!(s.schedule_retry(0, 3_000, BUDGET, MAX_BACKOFF), Some(3_000));
        assert_eq!(s.retries, 1);
        assert_eq!(s.schedule_retry(0, 3_000, BUDGET, MAX_BACKOFF), Some(6_000));
        assert_eq!(
            s.schedule_retry(0, 3_000, BUDGET, MAX_BACKOFF),
            Some(12_000)
        );
        assert_eq!(
            s.schedule_retry(0, 3_000, BUDGET, MAX_BACKOFF),
            Some(24_000)
        );
        assert_eq!(
            s.schedule_retry(0, 3_000, BUDGET, MAX_BACKOFF),
            Some(48_000)
        );
        // Backoff is capped, so later retries stay at the 60 s ceiling.
        assert_eq!(
            s.schedule_retry(0, 3_000, BUDGET, MAX_BACKOFF),
            Some(60_000)
        );
        // A successful ack resets the retry counter and the budget anchor.
        s.apply_ack(1, 0).unwrap();
        assert_eq!(s.retries, 0);
        assert_eq!(s.last_progress_ms, 0);
        // Without any progress the transfer is abandoned after the budget, not after 6 retries.
        assert_eq!(
            s.schedule_retry(BUDGET + 1, 3_000, BUDGET, MAX_BACKOFF),
            None
        );
    }

    #[test]
    fn reassembler_ack_window_bitmap_semantics() {
        let mut r = ResourceReceiver::new(7, 180 * 10, 180).unwrap();
        assert_eq!(r.total(), 10);
        assert_eq!(r.ack_window(), (0, 0));
        r.insert_chunk(0, &[1u8; 180]).unwrap();
        // next missing is 1; bitmap reports chunks 2..=10 (only present ones set: none yet).
        assert_eq!(r.ack_window(), (1, 0));
        r.insert_chunk(2, &[2u8; 180]).unwrap();
        r.insert_chunk(3, &[3u8; 180]).unwrap();
        // next = 1, bit 0 = chunk 2, bit 1 = chunk 3.
        assert_eq!(r.ack_window(), (1, 0b11));
        r.insert_chunk(1, &[4u8; 180]).unwrap();
        // next = 4 now; nothing beyond.
        assert_eq!(r.ack_window(), (4, 0));
        assert!(!r.is_complete());
        assert_eq!(r.received(), 4);
    }

    #[test]
    fn paced_emission_limits_each_release() {
        let mut s = ResourceSender::new(1, 20, 180);
        s.arm(0, 3_000);
        assert_eq!(s.take_unsent(2), vec![0, 1]);
        assert_eq!(s.take_unsent(2), vec![2, 3]);
    }

    #[test]
    fn ack_bitmap_helper_matches_doc() {
        let present = [0u16, 1, 2, 5, 6];
        let map = ack_bitmap(10, 2, |i| present.contains(&i));
        // next = 2 => bit 0 = 3 (absent), bit 3 = 6 (present) ... only 5 and 6 beyond.
        assert_eq!(map, (1 << (5 - 3)) | (1 << (6 - 3)));
        assert_eq!(ack_bitmap(0, 0, |_| true), 0);
        assert_eq!(ack_bitmap(4, 4, |_| true), 0);
    }

    #[test]
    fn resource_aad_is_25_bytes_and_distinct() {
        let a = resource_aad(1, 2, 3, 180);
        // "openmesh/v1/resource" (20) + res_id(4) + msg_id(4) + total_len(4) + chunk_bytes(2).
        assert_eq!(RESOURCE_AAD_PREFIX.len(), 20);
        assert_eq!(a.len(), RESOURCE_AAD_PREFIX.len() + 14);
        assert_eq!(a.len(), 34);
        assert_ne!(a, resource_aad(1, 2, 3, 181));
        assert_eq!(&a[..RESOURCE_AAD_PREFIX.len()], RESOURCE_AAD_PREFIX);
        assert_eq!(&a[20..24], &1u32.to_be_bytes());
        assert_eq!(&a[24..28], &2u32.to_be_bytes());
        assert_eq!(&a[28..32], &3u32.to_be_bytes());
        assert_eq!(&a[32..34], &180u16.to_be_bytes());
        assert_eq!(chunk_count(0, 180).unwrap(), 0);
        assert_eq!(chunk_count(180, 180).unwrap(), 1);
        assert_eq!(chunk_count(181, 180).unwrap(), 2);
        assert!(chunk_count(1, 0).is_err());
    }

    #[test]
    fn chunk_header_length_is_consistent() {
        // 1 flags + 4 msg_ref + 4 res_id + 2 index = 11; header + chunk must fit MAX_BODY.
        assert_eq!(CHUNK_HEADER_LEN, 11);
        assert!(CHUNK_HEADER_LEN + DEFAULT_CHUNK_BYTES as usize <= crate::wire::MAX_BODY);
        assert_eq!(
            MAX_CHUNK_BYTES as usize,
            crate::wire::MAX_BODY - CHUNK_HEADER_LEN
        );
    }
}

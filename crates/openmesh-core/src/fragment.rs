//! Byte-level fragmentation and reassembly utilities.
//!
//! OMP keeps frames at or under [`crate::wire::MTU`]. Anything larger (long text, images,
//! voice notes) is split into fixed-size chunks and reassembled with a bitmap. Resource
//! transfers use this module; the reassembler is deliberately tolerant of duplicates,
//! out-of-order arrival and overlapping windows.

use std::collections::BTreeMap;

use crate::error::{Error, Result};

/// Split `data` into `chunk_bytes`-sized chunks (last chunk may be shorter).
pub fn split(data: &[u8], chunk_bytes: usize) -> Result<Vec<&[u8]>> {
    if chunk_bytes == 0 {
        return Err(Error::Config("chunk size must be non-zero"));
    }
    Ok(data.chunks(chunk_bytes).collect())
}

/// Tracks which chunks of an expected total have arrived.
#[derive(Debug)]
pub struct Reassembler {
    chunk_bytes: usize,
    total_chunks: u16,
    received: u16,
    chunks: BTreeMap<u16, Vec<u8>>,
    total_len: u32,
}

impl Reassembler {
    /// Create a reassembler for `total_len` bytes split into `chunk_bytes` chunks.
    pub fn new(total_len: u32, chunk_bytes: usize) -> Result<Self> {
        if chunk_bytes == 0 || chunk_bytes > 1024 {
            return Err(Error::Config("invalid chunk size"));
        }
        let total_chunks = total_len.div_ceil(chunk_bytes as u32);
        if total_chunks > u16::MAX as u32 {
            return Err(Error::PayloadTooLarge {
                size: total_len as usize,
                max: u16::MAX as usize * chunk_bytes,
            });
        }
        Ok(Self {
            chunk_bytes,
            total_chunks: total_chunks as u16,
            received: 0,
            chunks: BTreeMap::new(),
            total_len,
        })
    }

    /// Number of chunks the message is split into.
    pub fn total_chunks(&self) -> u16 {
        self.total_chunks
    }

    /// Number of distinct chunks received so far.
    pub fn received_chunks(&self) -> u16 {
        self.received
    }

    /// Whether the chunk at `index` has been received.
    pub fn has(&self, index: u16) -> bool {
        self.chunks.contains_key(&index)
    }

    /// All missing chunk indices in ascending order.
    pub fn missing(&self) -> Vec<u16> {
        (0..self.total_chunks)
            .filter(|i| !self.chunks.contains_key(i))
            .collect()
    }

    /// Configured chunk size in bytes.
    pub fn chunk_bytes(&self) -> usize {
        self.chunk_bytes
    }

    /// Expected on-wire length of one chunk (the last chunk may be shorter).
    pub fn chunk_len_for(&self, index: u16) -> usize {
        self.chunk_len(index)
    }

    /// Whether every chunk is present.
    pub fn is_complete(&self) -> bool {
        self.received == self.total_chunks
    }

    /// Insert a chunk. Returns `true` if it was new. Duplicates are ignored.
    pub fn insert(&mut self, index: u16, data: &[u8]) -> Result<bool> {
        if index >= self.total_chunks {
            return Err(Error::Malformed("chunk index out of range"));
        }
        let expected = self.chunk_len(index);
        if data.len() != expected {
            return Err(Error::Malformed("chunk length mismatch"));
        }
        if self.chunks.contains_key(&index) {
            return Ok(false);
        }
        self.chunks.insert(index, data.to_vec());
        self.received += 1;
        Ok(true)
    }

    fn chunk_len(&self, index: u16) -> usize {
        let start = index as u32 * self.chunk_bytes as u32;
        let remaining = self.total_len.saturating_sub(start);
        (remaining as usize).min(self.chunk_bytes)
    }

    /// Next missing chunk index, or `None` when complete.
    pub fn next_missing(&self) -> Option<u16> {
        (0..self.total_chunks).find(|i| !self.chunks.contains_key(i))
    }

    /// Assemble the payload once all chunks are present.
    pub fn assemble(self) -> Result<Vec<u8>> {
        if !self.is_complete() {
            return Err(Error::Resource("incomplete"));
        }
        let mut out = Vec::with_capacity(self.total_len as usize);
        for (_, chunk) in self.chunks {
            out.extend_from_slice(&chunk);
        }
        if out.len() != self.total_len as usize {
            return Err(Error::Resource("assembled length mismatch"));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_and_reassemble_out_of_order() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let parts: Vec<&[u8]> = split(&data, 180).unwrap();
        let mut r = Reassembler::new(data.len() as u32, 180).unwrap();
        assert_eq!(r.total_chunks(), parts.len() as u16);
        for (i, part) in parts.iter().enumerate().rev() {
            assert!(r.insert(i as u16, part).unwrap());
        }
        assert!(r.is_complete());
        assert_eq!(r.assemble().unwrap(), data);
    }

    #[test]
    fn duplicate_chunks_are_ignored() {
        let mut r = Reassembler::new(360, 180).unwrap();
        assert!(r.insert(0, &[7u8; 180]).unwrap());
        assert!(!r.insert(0, &[7u8; 180]).unwrap());
        assert_eq!(r.received_chunks(), 1);
        assert_eq!(r.next_missing(), Some(1));
    }

    #[test]
    fn rejects_bad_indices_and_lengths() {
        let mut r = Reassembler::new(180, 180).unwrap();
        assert!(r.insert(5, &[0u8; 10]).is_err());
        assert!(r.insert(0, &[0u8; 10]).is_err());
    }
}

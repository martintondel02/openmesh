//! OMP/1 frame format — the byte-level contract shared by all implementations.
//!
//! See `docs/architecture/PROTOCOL.md` for the normative specification. This module is the
//! single Rust implementation of the layer-2 frame format; the C++ firmware mirrors it and
//! `protocol/vectors/` holds golden vectors that both must pass.
//!
//! ```text
//! offset size field
//! 0      1    version(3 bits) | frame_type(5 bits)
//! 1      1    ttl(4 bits)      | flags(4 bits)
//! 2      8    src  node id (big endian)
//! 10     8    dst  node id (big endian) or BROADCAST / channel id
//! 18     4    seq  (big endian, per-sender strictly increasing)
//! 22     ..   body (<= MAX_BODY)
//! ```

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Error, Result};

/// Protocol version carried in every frame header.
pub const PROTO_VERSION: u8 = 1;

/// Maximum on-air frame size. LoRa SX1262/LR1110 payload limit is 255 bytes; 240 leaves room
/// for future transport framing while staying under a single LoRa packet.
pub const MTU: usize = 240;

/// Fixed frame header length in bytes.
pub const HEADER_LEN: usize = 22;

/// Maximum body length that fits in one frame.
pub const MAX_BODY: usize = MTU - HEADER_LEN;

/// Reserved all-ones address meaning "every node".
pub const BROADCAST: NodeId = NodeId([0xFF; 8]);

/// An 8-byte OpenMesh node address, derived from the node's public identity.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct NodeId(pub [u8; 8]);

impl NodeId {
    /// All-zero id used as "unset".
    pub const ZERO: NodeId = NodeId([0; 8]);

    /// Construct from raw bytes.
    pub const fn new(bytes: [u8; 8]) -> Self {
        Self(bytes)
    }

    /// Raw address bytes.
    pub const fn as_bytes(&self) -> &[u8; 8] {
        &self.0
    }

    /// True for the reserved all-ones broadcast address.
    pub fn is_broadcast(&self) -> bool {
        self.0 == [0xFF; 8]
    }

    /// True for the all-zero address used as "unset".
    pub fn is_zero(&self) -> bool {
        self.0 == [0; 8]
    }

    /// True for a channel address (high bit of byte 0 set, PROTOCOL.md §3).
    pub fn is_channel(&self) -> bool {
        !self.is_broadcast() && self.0[0] & 0x80 != 0
    }

    /// True for a unicast node address (high bit of byte 0 clear).
    pub fn is_unicast(&self) -> bool {
        self.0[0] & 0x80 == 0
    }

    /// Lowercase hex representation.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse a hex address (an optional `0x` prefix is accepted).
    pub fn from_hex(s: &str) -> Result<Self> {
        let s = s.trim().trim_start_matches("0x");
        let bytes = hex::decode(s).map_err(|_| Error::Malformed("node id is not valid hex"))?;
        let arr: [u8; 8] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::Malformed("node id must be 8 bytes"))?;
        Ok(Self(arr))
    }

    /// First eight bytes of a SHA-256 digest.
    pub fn from_hash(hash: &[u8; 32]) -> Self {
        let mut out = [0u8; 8];
        out.copy_from_slice(&hash[..8]);
        Self(out)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({})", self.to_hex())
    }
}

impl FromStr for NodeId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::from_hex(s)
    }
}

impl Serialize for NodeId {
    fn serialize<S: Serializer>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for NodeId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> core::result::Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

/// Frame type discriminator, five bits wide.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum FrameType {
    /// Signed, flooded node advertisement: identity keys, role, name, capabilities.
    Announce = 0,
    /// Encrypted application payload (chat message, ACK-bearing message, position, ...).
    Data = 1,
    /// Delivery/read acknowledgement for a message id.
    Ack = 2,
    /// Chunk of a resource transfer (attachment, long text, avatar).
    Resource = 3,
    /// Reserved for directed path discovery.
    RouteRequest = 4,
    /// Reserved for directed path replies.
    RouteResponse = 5,
    /// Link probe / round-trip measurement.
    Probe = 6,
    /// Probe response.
    ProbeResponse = 7,
    /// Store-and-forward: "I hold envelopes for you".
    StoreOffer = 8,
    /// Store-and-forward: "send me these ids".
    StoreWant = 9,
    /// Signed administrative control frame.
    Admin = 10,
}

impl FrameType {
    /// Parse the five-bit wire discriminator.
    pub fn from_u8(v: u8) -> Result<Self> {
        Ok(match v {
            0 => Self::Announce,
            1 => Self::Data,
            2 => Self::Ack,
            3 => Self::Resource,
            4 => Self::RouteRequest,
            5 => Self::RouteResponse,
            6 => Self::Probe,
            7 => Self::ProbeResponse,
            8 => Self::StoreOffer,
            9 => Self::StoreWant,
            10 => Self::Admin,
            other => return Err(Error::Malformed(unknown_type_msg(other))),
        })
    }

    /// Whether the frame should be considered for store-and-forward retention by relays.
    pub fn is_storable(self) -> bool {
        matches!(self, Self::Data | Self::Resource)
    }

    /// Whether the frame carries a payload that may be retransmitted by flooding relays.
    pub fn is_floodable(self) -> bool {
        matches!(
            self,
            Self::Announce
                | Self::Data
                | Self::Ack
                | Self::Resource
                | Self::RouteRequest
                | Self::RouteResponse
                | Self::Probe
                | Self::ProbeResponse
                | Self::StoreOffer
                | Self::StoreWant
        )
    }
}

fn unknown_type_msg(v: u8) -> &'static str {
    // Avoid allocation in the hot path; one static message per unknown value class is not
    // possible, so callers get a generic error. Kept as a helper to keep the match arm tidy.
    match v {
        11..=31 => "unknown reserved frame type",
        _ => "invalid frame type",
    }
}

/// Header flag bits (low nibble of byte 1).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags(u8);

impl Flags {
    /// Body is AEAD sealed. Clear-text frames are only allowed for announces/probes.
    pub const ENCRYPTED: u8 = 0x01;
    /// Sender wants a delivery acknowledgement.
    pub const ACK_REQUESTED: u8 = 0x02;
    /// Sender marks this frame as time-sensitive (voice, acks, path replies).
    pub const PRIORITY: u8 = 0x04;
    /// Reserved. Relays must not set it: flags are authenticated, and store replay is
    /// handled out-of-band by the offer/want handshake (see PROTOCOL.md §10).
    pub const RESERVED_BIT3: u8 = 0x08;

    /// No flags set.
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Build from raw bits; the high nibble is masked off.
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits & 0x0F)
    }

    /// Raw flag bits.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Whether `flag` (one or more bits) is set.
    pub const fn contains(self, flag: u8) -> bool {
        self.0 & flag != 0
    }

    /// Set the given flag bits.
    pub fn insert(&mut self, flag: u8) {
        self.0 |= flag & 0x0F;
    }

    /// Clear the given flag bits.
    pub fn remove(&mut self, flag: u8) {
        self.0 &= !(flag & 0x0F);
    }
}

impl fmt::Debug for Flags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.contains(Self::ENCRYPTED) {
            parts.push("enc");
        }
        if self.contains(Self::ACK_REQUESTED) {
            parts.push("ack");
        }
        if self.contains(Self::PRIORITY) {
            parts.push("prio");
        }
        if self.contains(Self::RESERVED_BIT3) {
            parts.push("reserved");
        }
        write!(f, "Flags({})", parts.join(","))
    }
}

/// A single OMP/1 frame: fixed header plus opaque body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Frame type discriminator.
    pub typ: FrameType,
    /// Header flags (low nibble of byte 1).
    pub flags: Flags,
    /// Remaining hop budget. A relay decrements and re-emits while `ttl > 0`.
    pub ttl: u8,
    /// Originating node address.
    pub src: NodeId,
    /// Destination node, channel or [`BROADCAST`].
    pub dst: NodeId,
    /// Per-sender sequence number; used for duplicate suppression and replay protection.
    pub seq: u32,
    /// Opaque body, at most [`MAX_BODY`] bytes.
    pub body: Vec<u8>,
}

impl Frame {
    /// Maximum body length for one frame.
    pub const MAX_BODY: usize = MAX_BODY;

    /// Create a frame with empty flags and TTL 0.
    pub fn new(typ: FrameType, src: NodeId, dst: NodeId, seq: u32, body: Vec<u8>) -> Result<Self> {
        if body.len() > MAX_BODY {
            return Err(Error::PayloadTooLarge {
                size: body.len(),
                max: MAX_BODY,
            });
        }
        Ok(Self {
            typ,
            flags: Flags::empty(),
            ttl: 0,
            src,
            dst,
            seq,
            body,
        })
    }

    /// True for the reserved all-ones broadcast address.
    pub fn is_broadcast(&self) -> bool {
        self.dst.is_broadcast()
    }

    /// Whether the frame is addressed to `node` (or broadcast).
    pub fn is_for(&self, node: NodeId) -> bool {
        self.dst == node || self.is_broadcast()
    }

    /// Encode to the canonical 22-byte header plus body.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.body.len());
        self.encode_into(&mut out)?;
        Ok(out)
    }

    /// Encode into an existing buffer (appending).
    pub fn encode_into(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.body.len() > MAX_BODY {
            return Err(Error::PayloadTooLarge {
                size: self.body.len(),
                max: MAX_BODY,
            });
        }
        if self.ttl > 0x0F {
            return Err(Error::Malformed("ttl exceeds 4-bit field"));
        }
        out.push((PROTO_VERSION << 5) | (self.typ as u8 & 0x1F));
        out.push((self.ttl << 4) | self.flags.bits());
        out.extend_from_slice(&self.src.0);
        out.extend_from_slice(&self.dst.0);
        out.extend_from_slice(&self.seq.to_be_bytes());
        out.extend_from_slice(&self.body);
        Ok(())
    }

    /// Parse a frame from a full transport payload.
    pub fn decode(buf: &[u8]) -> Result<Self> {
        if buf.len() < HEADER_LEN {
            return Err(Error::BufferTooSmall {
                need: HEADER_LEN,
                have: buf.len(),
            });
        }
        if buf.len() > MTU {
            return Err(Error::PayloadTooLarge {
                size: buf.len(),
                max: MTU,
            });
        }
        let version = buf[0] >> 5;
        if version != PROTO_VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        let typ = FrameType::from_u8(buf[0] & 0x1F)?;
        let ttl = buf[1] >> 4;
        let flags = Flags::from_bits(buf[1] & 0x0F);
        let mut src = [0u8; 8];
        src.copy_from_slice(&buf[2..10]);
        let mut dst = [0u8; 8];
        dst.copy_from_slice(&buf[10..18]);
        let seq = u32::from_be_bytes([buf[18], buf[19], buf[20], buf[21]]);
        let body = buf[HEADER_LEN..].to_vec();
        Ok(Self {
            typ,
            flags,
            ttl,
            src: NodeId(src),
            dst: NodeId(dst),
            seq,
            body,
        })
    }

    /// Header bytes as used in AEAD associated data.
    ///
    /// The TTL nibble is **cleared** because every relay decrements it in flight; including it
    /// would make the ciphertext undecryptable after the first hop. Everything else (version,
    /// type, flags, source, destination, sequence) is authenticated.
    pub fn aad(&self) -> [u8; HEADER_LEN] {
        let mut aad = [0u8; HEADER_LEN];
        aad[0] = (PROTO_VERSION << 5) | (self.typ as u8 & 0x1F);
        aad[1] = self.flags.bits(); // high nibble (TTL) zeroed
        aad[2..10].copy_from_slice(&self.src.0);
        aad[10..18].copy_from_slice(&self.dst.0);
        aad[18..22].copy_from_slice(&self.seq.to_be_bytes());
        aad
    }

    /// Clone the frame with a decremented TTL, ready for relaying. Returns `None` when the
    /// hop budget is exhausted.
    pub fn relay_copy(&self) -> Option<Frame> {
        if self.ttl == 0 {
            return None;
        }
        let mut f = self.clone();
        f.ttl -= 1;
        Some(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Frame {
        Frame {
            typ: FrameType::Data,
            flags: Flags::from_bits(Flags::ENCRYPTED | Flags::ACK_REQUESTED),
            ttl: 5,
            src: NodeId::from_hex("0102030405060708").unwrap(),
            dst: NodeId::from_hex("1112131415161718").unwrap(),
            seq: 0xDEAD_BEEF,
            body: vec![1, 2, 3, 4, 5],
        }
    }

    #[test]
    fn roundtrip() {
        let f = sample();
        let encoded = f.encode().unwrap();
        assert_eq!(encoded.len(), HEADER_LEN + 5);
        let decoded = Frame::decode(&encoded).unwrap();
        assert_eq!(decoded, f);
    }

    #[test]
    fn header_layout_is_stable() {
        let encoded = sample().encode().unwrap();
        assert_eq!(encoded[0], (PROTO_VERSION << 5) | 1);
        assert_eq!(encoded[1], (5 << 4) | 0x03);
        assert_eq!(&encoded[2..10], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            &encoded[10..18],
            &[0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18]
        );
        assert_eq!(&encoded[18..22], &0xDEAD_BEEFu32.to_be_bytes());
    }

    #[test]
    fn rejects_bad_version_and_short_buffers() {
        let mut encoded = sample().encode().unwrap();
        encoded[0] = 2 << 5;
        assert!(matches!(
            Frame::decode(&encoded),
            Err(Error::UnsupportedVersion(2))
        ));
        assert!(matches!(
            Frame::decode(&[0u8; 4]),
            Err(Error::BufferTooSmall { .. })
        ));
    }

    #[test]
    fn relay_decrements_ttl() {
        let f = sample();
        assert_eq!(f.relay_copy().unwrap().ttl, 4);
        let mut zero = f.clone();
        zero.ttl = 0;
        assert!(zero.relay_copy().is_none());
    }

    #[test]
    fn node_id_hex_roundtrip() {
        let id = NodeId::from_hex("0xAABBCCDDEEFF0011").unwrap();
        assert_eq!(id.to_hex(), "aabbccddeeff0011");
        assert_eq!(id.to_string().parse::<NodeId>().unwrap(), id);
    }

    #[test]
    fn aad_ignores_ttl_so_relays_do_not_break_encryption() {
        let f = sample();
        let mut relayed = f.clone();
        relayed.ttl = 1;
        assert_eq!(f.aad(), relayed.aad());
        // ...but flags are authenticated.
        let mut tampered = f.clone();
        tampered.flags.insert(Flags::PRIORITY);
        assert_ne!(f.aad(), tampered.aad());
    }

    #[test]
    fn address_class_bits() {
        let node = NodeId::from_hex("7f01020304050607").unwrap();
        let chan = NodeId::from_hex("8001020304050607").unwrap();
        assert!(node.is_unicast() && !node.is_channel());
        assert!(chan.is_channel() && !chan.is_unicast());
        assert!(!BROADCAST.is_channel() && !BROADCAST.is_unicast());
    }
}

//! Application-layer message schemas carried inside OMP frames.
//!
//! Control payloads are CBOR-encoded with `serde`/`ciborium`; announces use a compact fixed
//! binary layout (PROTOCOL.md §6) because a CBOR identity + signature does not fit in one
//! frame. These types are the cross-language contract between the Rust host stack, the C++
//! firmware and the companion API; changing them requires a protocol version bump and new
//! golden vectors in `protocol/vectors/`.
//!
//! Byte fields are encoded as CBOR byte strings (not integer arrays), matching the `bytes(N)`
//! notation in the specification.

use serde::{Deserialize, Serialize};

use crate::crypto::PublicIdentity;
use crate::error::{Error, Result};
use crate::wire::NodeId;

/// Serde helpers that encode byte fields as CBOR byte strings (the cross-language contract)
/// rather than as CBOR arrays of integers. Decoders also accept integer arrays for tolerance.
mod bytes_wire {
    use serde::{Serialize, Serializer};

    pub struct BytesRef<'a>(pub &'a [u8]);

    impl Serialize for BytesRef<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_bytes(self.0)
        }
    }

    pub struct BytesVisitor;

    impl<'v> serde::de::Visitor<'v> for BytesVisitor {
        type Value = Vec<u8>;
        fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.write_str("a CBOR byte string")
        }
        fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
            Ok(v.to_vec())
        }
        fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
            Ok(v)
        }
        fn visit_seq<A: serde::de::SeqAccess<'v>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1024));
            while let Some(byte) = seq.next_element::<u8>()? {
                out.push(byte);
            }
            Ok(out)
        }
    }

    pub mod vec {
        use serde::{Deserializer, Serializer};

        /// Encode a `Vec<u8>` as a CBOR byte string.
        pub fn serialize<S: Serializer>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_bytes(value)
        }

        /// Decode a CBOR byte string (or legacy integer array) into a `Vec<u8>`.
        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Vec<u8>, D::Error> {
            deserializer.deserialize_bytes(super::BytesVisitor)
        }
    }

    pub mod option_vec {
        use serde::{Deserializer, Serializer};

        /// Encode an optional byte string.
        pub fn serialize<S: Serializer>(
            value: &Option<Vec<u8>>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match value {
                None => serializer.serialize_none(),
                Some(bytes) => serializer.serialize_some(&super::BytesRef(bytes)),
            }
        }

        /// Decode an optional byte string.
        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<Vec<u8>>, D::Error> {
            struct OptVisitor;
            impl<'v> serde::de::Visitor<'v> for OptVisitor {
                type Value = Option<Vec<u8>>;
                fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    f.write_str("an optional CBOR byte string")
                }
                fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_some<D2: Deserializer<'v>>(
                    self,
                    inner: D2,
                ) -> Result<Self::Value, D2::Error> {
                    inner.deserialize_bytes(super::BytesVisitor).map(Some)
                }
            }
            deserializer.deserialize_option(OptVisitor)
        }
    }

    pub mod array32 {
        use serde::{Deserializer, Serializer};

        /// Encode a 32-byte digest as a CBOR byte string.
        pub fn serialize<S: Serializer>(
            value: &[u8; 32],
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            serializer.serialize_bytes(value)
        }

        /// Decode a 32-byte CBOR byte string.
        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<[u8; 32], D::Error> {
            let bytes = deserializer.deserialize_bytes(super::BytesVisitor)?;
            bytes
                .as_slice()
                .try_into()
                .map_err(serde::de::Error::custom)
        }
    }
}

/// What role a node plays in the mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeClass {
    /// Phone/desktop-attached or standalone client.
    Client,
    /// Headless range extender; forwards and optionally stores envelopes.
    Repeater,
    /// Bridges LoRa to IP (WiFi/Ethernet).
    Gateway,
    /// Stores and serves channel history for late joiners.
    RoomServer,
    /// Low-power GPS tracker; mostly beacons position.
    Tracker,
}

impl NodeClass {
    /// Lowercase name used in JSON APIs and configuration.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Repeater => "repeater",
            Self::Gateway => "gateway",
            Self::RoomServer => "room_server",
            Self::Tracker => "tracker",
        }
    }

    /// Wire value used in the binary announce layout (PROTOCOL.md §6).
    pub fn as_u8(self) -> u8 {
        match self {
            Self::Client => 0,
            Self::Repeater => 1,
            Self::Gateway => 2,
            Self::RoomServer => 3,
            Self::Tracker => 4,
        }
    }

    /// Parse the wire value used in the binary announce layout.
    pub fn from_u8(v: u8) -> Result<Self> {
        Ok(match v {
            0 => Self::Client,
            1 => Self::Repeater,
            2 => Self::Gateway,
            3 => Self::RoomServer,
            4 => Self::Tracker,
            _ => return Err(Error::Malformed("unknown node class")),
        })
    }
}

/// Capability bits advertised in an [`Announce`] (PROTOCOL.md §6.1).
pub mod caps {
    /// Forwards frames for other nodes.
    pub const RELAY: u16 = 1 << 0;
    /// Store-and-forward capable.
    pub const STORE: u16 = 1 << 1;
    /// Has a position source.
    pub const GPS: u16 = 1 << 2;
    /// Has WiFi.
    pub const WIFI: u16 = 1 << 3;
    /// Has Ethernet.
    pub const ETHERNET: u16 = 1 << 4;
    /// Provides a BLE companion link.
    pub const BLE: u16 = 1 << 5;
    /// Provides a USB companion link.
    pub const USB: u16 = 1 << 6;
    /// Has a display.
    pub const SCREEN: u16 = 1 << 7;
    /// Can play audio.
    pub const AUDIO_OUT: u16 = 1 << 8;
    /// Solar powered.
    pub const SOLAR: u16 = 1 << 9;
}

/// Maximum announce display name length in bytes.
pub const ANNOUNCE_NAME_MAX: usize = 32;
/// Length of the fixed announce body prefix (everything before the variable name).
pub const ANNOUNCE_PREFIX_LEN: usize = 88;
/// Ed25519 signature length carried at the end of every announce body.
pub const ANNOUNCE_SIG_LEN: usize = 64;
/// Maximum complete announce body (prefix + 32-byte name + 64-byte signature).
pub const ANNOUNCE_MAX_BODY: usize = ANNOUNCE_PREFIX_LEN + ANNOUNCE_NAME_MAX + ANNOUNCE_SIG_LEN;

/// Signed node advertisement (frame type `Announce`).
///
/// The on-air body is the binary layout from PROTOCOL.md §6: `version | class | caps |
/// reserved | name_len | interval_s | timestamp_ms | node_id | sign_pk | dh_pk | name`, with a
/// 64-byte Ed25519 signature appended. The `hops` field is **not** on the wire (relays must not
/// modify signed bytes); it is a local UI value derived from the received TTL by the engine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Announce {
    /// Wire format version; must be 1.
    pub version: u8,
    /// Node role.
    pub class: NodeClass,
    /// Capability bitflags (see [`caps`]).
    pub caps: u16,
    /// Display name, 0..32 bytes UTF-8.
    pub name: String,
    /// Local-only hop count derived from the received TTL (`ANNOUNCE_TTL - frame.ttl`).
    ///
    /// Not transmitted and not signed; kept for API/UI compatibility.
    pub hops: u8,
    /// Sender's Unix time in milliseconds; only used for display.
    pub timestamp_ms: u64,
    /// Intended re-announce period in seconds.
    pub interval_s: u16,
    /// Full public identity of the announcing node.
    pub ident: PublicIdentity,
}

impl Announce {
    /// Encode the signed portion of the body (`88 + name_len` bytes, without the signature).
    ///
    /// Offset 4 is the reserved byte and is always written as zero; `hops` is local-only.
    pub fn encode_wire(&self) -> Result<Vec<u8>> {
        let name = self.name.as_bytes();
        if name.len() > ANNOUNCE_NAME_MAX {
            return Err(Error::Malformed("announce name too long"));
        }
        let mut out = Vec::with_capacity(ANNOUNCE_PREFIX_LEN + name.len());
        out.push(self.version);
        out.push(self.class.as_u8());
        out.extend_from_slice(&self.caps.to_be_bytes());
        out.push(0); // reserved (formerly hops); MUST be 0
        out.push(name.len() as u8);
        out.extend_from_slice(&self.interval_s.to_be_bytes());
        out.extend_from_slice(&self.timestamp_ms.to_be_bytes());
        out.extend_from_slice(&self.ident.node_id.0);
        out.extend_from_slice(&self.ident.sign_pk);
        out.extend_from_slice(&self.ident.dh_pk);
        out.extend_from_slice(name);
        Ok(out)
    }

    /// Decode a signed portion produced by [`Announce::encode_wire`].
    ///
    /// The returned `hops` value is 0; the engine overwrites it from the frame TTL.
    pub fn decode_wire(body: &[u8]) -> Result<Self> {
        if body.len() < ANNOUNCE_PREFIX_LEN {
            return Err(Error::BufferTooSmall {
                need: ANNOUNCE_PREFIX_LEN,
                have: body.len(),
            });
        }
        let version = body[0];
        let class = NodeClass::from_u8(body[1])?;
        let caps = u16::from_be_bytes([body[2], body[3]]);
        if body[4] != 0 {
            return Err(Error::Malformed("announce reserved byte must be zero"));
        }
        let name_len = body[5] as usize;
        if name_len > ANNOUNCE_NAME_MAX {
            return Err(Error::Malformed("announce name too long"));
        }
        if body.len() != ANNOUNCE_PREFIX_LEN + name_len {
            return Err(Error::Malformed("announce body length mismatch"));
        }
        let interval_s = u16::from_be_bytes([body[6], body[7]]);
        let mut ts = [0u8; 8];
        ts.copy_from_slice(&body[8..16]);
        let timestamp_ms = u64::from_be_bytes(ts);
        let mut node_id = [0u8; 8];
        node_id.copy_from_slice(&body[16..24]);
        let mut sign_pk = [0u8; 32];
        sign_pk.copy_from_slice(&body[24..56]);
        let mut dh_pk = [0u8; 32];
        dh_pk.copy_from_slice(&body[56..88]);
        let name = core::str::from_utf8(&body[ANNOUNCE_PREFIX_LEN..])
            .map_err(|_| Error::Malformed("announce name is not UTF-8"))?
            .to_string();
        Ok(Self {
            version,
            class,
            caps,
            name,
            hops: 0,
            timestamp_ms,
            interval_s,
            ident: PublicIdentity {
                node_id: NodeId(node_id),
                sign_pk,
                dh_pk,
            },
        })
    }

    /// Encode the full on-air body: signed portion followed by the Ed25519 signature.
    pub fn encode_signed(&self, signature: &[u8; ANNOUNCE_SIG_LEN]) -> Result<Vec<u8>> {
        let mut out = self.encode_wire()?;
        out.extend_from_slice(signature);
        if out.len() > crate::wire::MAX_BODY {
            return Err(Error::PayloadTooLarge {
                size: out.len(),
                max: crate::wire::MAX_BODY,
            });
        }
        Ok(out)
    }

    /// Split a full on-air body into the decoded announce and its signature.
    ///
    /// Callers MUST verify the signature over the original bytes (see
    /// [`announce_signature_payload`]) and check [`PublicIdentity::is_self_consistent`].
    pub fn decode_signed(body: &[u8]) -> Result<(Self, [u8; ANNOUNCE_SIG_LEN])> {
        if body.len() < ANNOUNCE_PREFIX_LEN + ANNOUNCE_SIG_LEN {
            return Err(Error::BufferTooSmall {
                need: ANNOUNCE_PREFIX_LEN + ANNOUNCE_SIG_LEN,
                have: body.len(),
            });
        }
        let split = body.len() - ANNOUNCE_SIG_LEN;
        let announce = Self::decode_wire(&body[..split])?;
        let mut signature = [0u8; ANNOUNCE_SIG_LEN];
        signature.copy_from_slice(&body[split..]);
        Ok((announce, signature))
    }
}

/// A geographic position fix.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Position {
    /// Latitude in 1e-7 degrees.
    pub lat_e7: i32,
    /// Longitude in 1e-7 degrees.
    pub lon_e7: i32,
    /// Altitude in metres.
    pub alt_m: Option<i16>,
    /// Horizontal accuracy in metres.
    pub accuracy_m: Option<u8>,
    /// Speed in cm/s.
    pub speed_cm_s: Option<u16>,
    /// Heading in degrees (0-359).
    pub heading_deg: Option<u16>,
    /// Fix timestamp in Unix milliseconds.
    pub timestamp_ms: u64,
}

/// Reference to a binary resource transferred separately from the message that announces it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentRef {
    /// Short id used by the resource transfer.
    pub res_id: u32,
    /// MIME type of the payload.
    pub mime: String,
    /// Plaintext payload size in bytes.
    pub size: u32,
    /// SHA-256 of the sealed resource blob (see PROTOCOL.md §9.1).
    #[serde(with = "bytes_wire::array32")]
    pub sha256: [u8; 32],
    /// Bytes per resource chunk.
    pub chunk_bytes: u16,
    /// Number of chunks in the blob.
    pub chunk_count: u16,
    /// Duration for audio attachments.
    pub duration_ms: Option<u32>,
    /// Original filename, when known.
    pub name: Option<String>,
}

/// Kind of application message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// Plain chat text.
    Text,
    /// Position report.
    Position,
    /// Message with an attachment transferred as a resource.
    Attachment,
    /// Channel invitation.
    GroupInvite,
    /// Infrastructure telemetry.
    Telemetry,
    /// Out-of-band control message.
    Control,
}

/// App-level message carried inside an encrypted `Data` frame (direct) or channel frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppMessage {
    /// Random 32-bit id, unique per sender.
    pub id: u32,
    /// What kind of content this message carries.
    pub kind: MessageKind,
    /// Sender's Unix time in milliseconds.
    pub sent_at_ms: u64,
    /// Text body for `kind = text`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub text: Option<String>,
    /// Message id this message replies to.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reply_to: Option<u32>,
    /// Thread root message id.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub thread: Option<u32>,
    /// Attachment metadata for `kind = attachment`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub attachment: Option<AttachmentRef>,
    /// Position fix for `kind = position`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub position: Option<Position>,
    /// Telemetry sample for `kind = telemetry`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub telemetry: Option<Telemetry>,
    /// Present for group messages so receivers can attribute and verify authorship.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sender: Option<PublicIdentity>,
    /// Ed25519 signature over the canonical encoding of this message without `signature`.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        with = "bytes_wire::option_vec"
    )]
    pub signature: Option<Vec<u8>>,
}

impl AppMessage {
    /// Create a text message with only the required fields set.
    pub fn text(id: u32, sent_at_ms: u64, text: impl Into<String>) -> Self {
        Self {
            id,
            kind: MessageKind::Text,
            sent_at_ms,
            text: Some(text.into()),
            reply_to: None,
            thread: None,
            attachment: None,
            position: None,
            telemetry: None,
            sender: None,
            signature: None,
        }
    }
}

/// Resource manifest sent as its own message before chunks are streamed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    /// Binary attachment (image, voice note).
    Attachment,
    /// Long chat text that did not fit in one frame.
    LongText,
    /// Avatar image.
    Avatar,
    /// Channel history for late joiners.
    ChannelHistory,
}

/// Manifest describing one resource transfer (PROTOCOL.md §9.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceManifest {
    /// Random per-sender resource id.
    pub res_id: u32,
    /// Message id this resource belongs to.
    pub msg_id: u32,
    /// Payload category.
    pub kind: ResourceKind,
    /// MIME type of the plaintext payload.
    pub mime: String,
    /// Sealed blob size in bytes.
    pub total_len: u32,
    /// Bytes per chunk (180 by default).
    pub chunk_bytes: u16,
    /// Number of chunks; `ceil(total_len / chunk_bytes)`.
    pub chunk_count: u16,
    /// SHA-256 over the sealed blob.
    #[serde(with = "bytes_wire::array32")]
    pub sha256: [u8; 32],
}

/// Acknowledgement frame payload (PROTOCOL.md §8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AckKind {
    /// Message delivered to the recipient application.
    Delivered,
    /// Message read by the human recipient (read receipt).
    Read,
    /// Resource chunk window acknowledgement.
    ResourceWindow,
    /// Resource fully received and verified.
    ResourceComplete,
}

/// Acknowledgement body carried in a sealed `Ack` frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {
    /// Acknowledgement category.
    pub kind: AckKind,
    /// Message being acked.
    pub msg_id: u32,
    /// Resource being acked (resource kinds only).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub res_id: Option<u32>,
    /// Next chunk the receiver expects (selective repeat).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub next_expected: Option<u16>,
    /// Bitmap of the 32 chunks following `next_expected`; bit 0 = next_expected+1.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bitmap: Option<u32>,
    /// Receiver's Unix time in milliseconds.
    pub timestamp_ms: u64,
}

/// Body of a `StoreOffer` frame: ids of envelopes a relay holds for the addressee.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreOffer {
    /// Clear-text `msg_ref`s of the envelopes that are being offered.
    pub ids: Vec<u32>,
    /// Total number of envelopes held for the addressee (may exceed `ids.len()`).
    pub count: u32,
}

/// Body of a `StoreWant` frame: ids the addressee is missing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreWant {
    /// `msg_ref`s the addressee asks the relay to replay.
    pub ids: Vec<u32>,
}

/// Telemetry beacon from infrastructure nodes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Telemetry {
    /// Sample time in Unix milliseconds.
    pub timestamp_ms: u64,
    /// Battery voltage in millivolts.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub battery_mv: Option<u16>,
    /// Battery state of charge in percent.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub battery_pct: Option<u8>,
    /// Seconds since boot.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub uptime_s: Option<u32>,
    /// Transmit airtime used since boot.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub airtime_used_ms: Option<u32>,
    /// Frames received since boot.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rx_frames: Option<u32>,
    /// Frames transmitted since boot.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tx_frames: Option<u32>,
    /// Ambient temperature in degrees Celsius.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub temperature_c: Option<i8>,
    /// Solar input voltage in millivolts.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub solar_mv: Option<u16>,
}

/// Envelope used when a channel message must be delivered to a group (channel) address.
///
/// The plaintext is CBOR of this struct sealed with the channel key. The sender is only an
/// 8-byte node id; receivers resolve the Ed25519 key from the peer book (announce history) and
/// verify `signature`, which is what proves authorship because the channel key is shared.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupMessage {
    /// Sender's node id (8 bytes on the wire).
    pub sender: NodeId,
    /// Application message.
    pub message: AppMessage,
    /// Ed25519 signature over [`group_signature_payload`].
    #[serde(with = "bytes_wire::vec")]
    pub signature: Vec<u8>,
}

/// Envelope used when a direct message is larger than one frame and streamed as a resource.
///
/// Kept for API compatibility; the engine's long-text path sends `CBOR(AppMessage)` as the
/// resource plaintext (PROTOCOL.md §9.1) instead of this envelope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DirectResource {
    /// Sender identity.
    pub sender: PublicIdentity,
    /// Message that references the resource.
    pub message: AppMessage,
    /// SHA-256 over the sealed resource blob.
    #[serde(with = "bytes_wire::array32")]
    pub sha256: [u8; 32],
}

/// Canonical bytes signed for group authorship: a domain-separated digest.
pub fn group_signature_payload(sender: &NodeId, channel: &NodeId, msg: &AppMessage) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    out.extend_from_slice(b"openmesh/v1/group-msg");
    out.extend_from_slice(&sender.0);
    out.extend_from_slice(&channel.0);
    out.extend_from_slice(&cbor_bytes(msg));
    out
}

/// Canonical bytes covered by an announce signature (PROTOCOL.md §6).
///
/// `body_without_signature` is the exact binary body returned by [`Announce::encode_wire`].
pub fn announce_signature_payload(
    src: &NodeId,
    seq: u32,
    body_without_signature: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + body_without_signature.len());
    out.extend_from_slice(b"openmesh/v1/announce");
    out.extend_from_slice(&src.0);
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(body_without_signature);
    out
}

/// Helper: encode any serializable value as canonical CBOR.
pub fn cbor_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(value, &mut out).expect("CBOR serialization to Vec cannot fail");
    out
}

/// Helper: decode CBOR, mapping errors into the protocol error type.
pub fn cbor_from_bytes<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> crate::error::Result<T> {
    ciborium::from_reader(bytes).map_err(|e| crate::error::Error::Codec(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_message() -> AppMessage {
        AppMessage::text(42, 1_700_000_000_000, "hello mesh")
    }

    #[test]
    fn app_message_cbor_roundtrip() {
        let msg = sample_message();
        let bytes = cbor_bytes(&msg);
        let back: AppMessage = cbor_from_bytes(&bytes).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn byte_fields_encode_as_cbor_byte_strings() {
        // GroupMessage.signature must be a CBOR byte string (0x58 0x40 0x...), not an array.
        let group = GroupMessage {
            sender: NodeId([1; 8]),
            message: sample_message(),
            signature: vec![0xABu8; 64],
        };
        let bytes = cbor_bytes(&group);
        assert!(
            bytes.windows(2).any(|w| w == [0x58, 0x40]),
            "signature is not a 64-byte CBOR byte string"
        );
        let back: GroupMessage = cbor_from_bytes(&bytes).unwrap();
        assert_eq!(back, group);

        // AttachmentRef and ResourceManifest sha256 fields likewise.
        let manifest = ResourceManifest {
            res_id: 1,
            msg_id: 2,
            kind: ResourceKind::Attachment,
            mime: "image/jpeg".into(),
            total_len: 100,
            chunk_bytes: 180,
            chunk_count: 1,
            sha256: [0xCD; 32],
        };
        let bytes = cbor_bytes(&manifest);
        assert!(bytes.windows(2).any(|w| w == [0x58, 0x20]));
        assert_eq!(
            cbor_from_bytes::<ResourceManifest>(&bytes).unwrap(),
            manifest
        );
    }

    #[test]
    fn attachment_metadata_roundtrip() {
        let att = AttachmentRef {
            res_id: 7,
            mime: "image/jpeg".into(),
            size: 5120,
            sha256: [3u8; 32],
            chunk_bytes: 180,
            chunk_count: 29,
            duration_ms: None,
            name: Some("trail.jpg".into()),
        };
        let mut msg = sample_message();
        msg.kind = MessageKind::Attachment;
        msg.text = None;
        msg.attachment = Some(att);
        assert_eq!(
            cbor_from_bytes::<AppMessage>(&cbor_bytes(&msg)).unwrap(),
            msg
        );
    }

    #[test]
    fn position_and_telemetry_roundtrip() {
        let pos = Position {
            lat_e7: 59_913_700,
            lon_e7: 10_752_200,
            alt_m: Some(45),
            accuracy_m: Some(8),
            speed_cm_s: None,
            heading_deg: Some(180),
            timestamp_ms: 12,
        };
        assert_eq!(cbor_from_bytes::<Position>(&cbor_bytes(&pos)).unwrap(), pos);
        let telemetry = Telemetry {
            timestamp_ms: 99,
            battery_mv: Some(3980),
            battery_pct: Some(82),
            uptime_s: Some(86_400),
            airtime_used_ms: Some(42_000),
            rx_frames: Some(240),
            tx_frames: Some(120),
            temperature_c: Some(21),
            solar_mv: Some(5100),
        };
        let mut msg = sample_message();
        msg.kind = MessageKind::Telemetry;
        msg.text = None;
        msg.telemetry = Some(telemetry);
        assert_eq!(
            cbor_from_bytes::<AppMessage>(&cbor_bytes(&msg)).unwrap(),
            msg
        );
    }

    #[test]
    fn announce_wire_layout_offsets() {
        let ident = crate::crypto::Identity::from_seed(&[9u8; 32]).public();
        let announce = Announce {
            version: 1,
            class: NodeClass::Repeater,
            caps: caps::RELAY | caps::STORE,
            name: "ridge".into(),
            hops: 7, // local-only, must not appear on the wire
            timestamp_ms: 0x0102_0304_0506_0708,
            interval_s: 3600,
            ident,
        };
        let wire = announce.encode_wire().unwrap();
        assert_eq!(wire.len(), ANNOUNCE_PREFIX_LEN + 5);
        assert_eq!(wire[0], 1);
        assert_eq!(wire[1], 1); // repeater
        assert_eq!(&wire[2..4], &(caps::RELAY | caps::STORE).to_be_bytes());
        assert_eq!(wire[4], 0); // reserved; hops is not transmitted
        assert_eq!(wire[5], 5);
        assert_eq!(&wire[6..8], &3600u16.to_be_bytes());
        assert_eq!(&wire[8..16], &0x0102_0304_0506_0708u64.to_be_bytes());
        assert_eq!(&wire[16..24], &ident.node_id.0);
        assert_eq!(&wire[24..56], &ident.sign_pk);
        assert_eq!(&wire[56..88], &ident.dh_pk);
        assert_eq!(&wire[88..], b"ridge");

        let back = Announce::decode_wire(&wire).unwrap();
        assert_eq!(back.hops, 0);
        assert_eq!(back.name, announce.name);
        assert_eq!(back.ident, ident);
        assert!(back.ident.is_self_consistent());
        assert_eq!(back.version, 1);
    }

    #[test]
    fn announce_signed_body_roundtrip_and_rejects_tamper() {
        let ident = crate::crypto::Identity::from_seed(&[3u8; 32]);
        let payload = Announce {
            version: 1,
            class: NodeClass::Client,
            caps: caps::RELAY,
            name: "alice".into(),
            hops: 0,
            timestamp_ms: 1_700_000_000_000,
            interval_s: 1800,
            ident: ident.public(),
        };
        let unsigned = payload.encode_wire().unwrap();
        let signing_payload = announce_signature_payload(&ident.node_id(), 7, &unsigned);
        let sig = ident.sign(&signing_payload);
        let body = payload.encode_signed(&sig).unwrap();
        assert_eq!(body.len(), unsigned.len() + 64);
        let (decoded, got_sig) = Announce::decode_signed(&body).unwrap();
        assert_eq!(got_sig, sig);
        assert_eq!(decoded, payload);
        let verify_payload =
            announce_signature_payload(&decoded.ident.node_id, 7, &body[..unsigned.len()]);
        decoded.ident.verify(&verify_payload, &got_sig).unwrap();
        // A one-bit change in the signed body must not verify.
        let mut tampered = body.clone();
        tampered[20] ^= 1;
        let verify_payload =
            announce_signature_payload(&decoded.ident.node_id, 7, &tampered[..unsigned.len()]);
        assert!(decoded.ident.verify(&verify_payload, &got_sig).is_err());
    }

    #[test]
    fn announce_rejects_bad_input() {
        assert!(matches!(
            Announce::decode_wire(&[0u8; 10]),
            Err(Error::BufferTooSmall { .. })
        ));
        let ident = crate::crypto::Identity::from_seed(&[1u8; 32]).public();
        let mut announce = Announce {
            version: 1,
            class: NodeClass::Client,
            caps: 0,
            name: "x".into(),
            hops: 0,
            timestamp_ms: 0,
            interval_s: 1800,
            ident,
        };
        announce.name = "x".repeat(ANNOUNCE_NAME_MAX + 1);
        assert!(announce.encode_wire().is_err());
        // Reserved byte must be zero.
        announce.name = "ok".into();
        let mut wire = announce.encode_wire().unwrap();
        wire[4] = 1;
        assert!(Announce::decode_wire(&wire).is_err());
        // Name length beyond the buffer.
        wire[4] = 0;
        wire[5] = 10;
        assert!(Announce::decode_wire(&wire).is_err());
    }

    #[test]
    fn capability_bits_match_spec() {
        assert_eq!(caps::RELAY, 1 << 0);
        assert_eq!(caps::STORE, 1 << 1);
        assert_eq!(caps::GPS, 1 << 2);
        assert_eq!(caps::WIFI, 1 << 3);
        assert_eq!(caps::ETHERNET, 1 << 4);
        assert_eq!(caps::BLE, 1 << 5);
        assert_eq!(caps::USB, 1 << 6);
        assert_eq!(caps::SCREEN, 1 << 7);
        assert_eq!(caps::AUDIO_OUT, 1 << 8);
        assert_eq!(caps::SOLAR, 1 << 9);
    }

    #[test]
    fn node_class_wire_values() {
        for (value, class) in [
            (0u8, NodeClass::Client),
            (1, NodeClass::Repeater),
            (2, NodeClass::Gateway),
            (3, NodeClass::RoomServer),
            (4, NodeClass::Tracker),
        ] {
            assert_eq!(class.as_u8(), value);
            assert_eq!(NodeClass::from_u8(value).unwrap(), class);
        }
        assert!(NodeClass::from_u8(5).is_err());
    }

    #[test]
    fn group_message_is_compact_and_signed() {
        let alice = crate::crypto::Identity::from_seed(&[4u8; 32]);
        let channel = NodeId([0x80, 1, 2, 3, 4, 5, 6, 7]);
        let mut msg = sample_message();
        msg.text = Some("on my way".into());
        let signed = group_signature_payload(&alice.node_id(), &channel, &msg);
        let group = GroupMessage {
            sender: alice.node_id(),
            message: msg,
            signature: alice.sign(&signed).to_vec(),
        };
        let bytes = cbor_bytes(&group);
        assert!(
            bytes.len() < 190,
            "group message CBOR too large: {}",
            bytes.len()
        );
        let back: GroupMessage = cbor_from_bytes(&bytes).unwrap();
        assert_eq!(back, group);
        let signed = group_signature_payload(&back.sender, &channel, &back.message);
        let signature: [u8; 64] = back.signature.as_slice().try_into().unwrap();
        alice.public().verify(&signed, &signature).unwrap();
    }
}

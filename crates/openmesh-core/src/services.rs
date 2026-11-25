//! Application-layer message schemas carried inside OMP frames.
//!
//! Payloads are CBOR-encoded with `serde`/`ciborium`. These types are the cross-language
//! contract between the Rust host stack and the firmware's companion API; changing them
//! requires a protocol version bump and new golden vectors.

use serde::{Deserialize, Serialize};

use crate::crypto::PublicIdentity;
use crate::wire::NodeId;

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
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Repeater => "repeater",
            Self::Gateway => "gateway",
            Self::RoomServer => "room_server",
            Self::Tracker => "tracker",
        }
    }
}

/// Capability bits advertised in an [`Announce`].
pub mod caps {
    pub const GPS: u16 = 1 << 0;
    pub const WIFI: u16 = 1 << 1;
    pub const ETHERNET: u16 = 1 << 2;
    pub const STORE: u16 = 1 << 3;
    pub const RELAY: u16 = 1 << 4;
    pub const BLE: u16 = 1 << 5;
    pub const USB: u16 = 1 << 6;
    pub const SCREEN: u16 = 1 << 7;
    pub const AUDIO_OUT: u16 = 1 << 8;
    pub const SOLAR: u16 = 1 << 9;
}

/// Signed node advertisement (frame type `Announce`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Announce {
    pub version: u8,
    pub class: NodeClass,
    pub caps: u16,
    pub name: String,
    /// How many relays this announcement has traversed (incremented by relays).
    pub hops: u8,
    pub timestamp_ms: u64,
    /// Refresh period the node intends to use.
    pub interval_s: u16,
    pub ident: PublicIdentity,
}

/// A geographic position fix.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Position {
    /// Latitude in 1e-7 degrees.
    pub lat_e7: i32,
    /// Longitude in 1e-7 degrees.
    pub lon_e7: i32,
    pub alt_m: Option<i16>,
    pub accuracy_m: Option<u8>,
    /// Speed in cm/s.
    pub speed_cm_s: Option<u16>,
    /// Heading in degrees (0-359).
    pub heading_deg: Option<u16>,
    pub timestamp_ms: u64,
}

/// Reference to a binary resource transferred separately from the message that announces it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentRef {
    /// Short id used by the resource transfer.
    pub res_id: u32,
    pub mime: String,
    pub size: u32,
    pub sha256: [u8; 32],
    pub chunk_bytes: u16,
    pub chunk_count: u16,
    /// Duration for audio attachments.
    pub duration_ms: Option<u32>,
    pub name: Option<String>,
}

/// Kind of application message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Text,
    Position,
    Attachment,
    GroupInvite,
    Telemetry,
    Control,
}

/// App-level message carried inside an encrypted `Data` frame (direct) or channel frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppMessage {
    /// Random 32-bit id, unique per sender.
    pub id: u32,
    pub kind: MessageKind,
    pub sent_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reply_to: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub thread: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub attachment: Option<AttachmentRef>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub position: Option<Position>,
    /// Present for group messages so receivers can attribute and verify authorship.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sender: Option<PublicIdentity>,
    /// Ed25519 signature over the canonical encoding of this message without `signature`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub signature: Option<Vec<u8>>,
}

/// Resource manifest sent as its own message before chunks are streamed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Attachment,
    LongText,
    Avatar,
    ChannelHistory,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceManifest {
    pub res_id: u32,
    /// Message id this resource belongs to.
    pub msg_id: u32,
    pub kind: ResourceKind,
    pub mime: String,
    pub total_len: u32,
    pub chunk_bytes: u16,
    pub chunk_count: u16,
    pub sha256: [u8; 32],
}

/// Acknowledgement frame payload.
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {
    pub kind: AckKind,
    pub msg_id: u32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub res_id: Option<u32>,
    /// Next chunk the receiver expects (selective repeat).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub next_expected: Option<u16>,
    /// Bitmap of the 32 chunks following `next_expected`; bit 0 = next_expected+1.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bitmap: Option<u32>,
    pub timestamp_ms: u64,
}

/// Body of a `StoreOffer` frame: ids of envelopes a relay holds for the addressee.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreOffer {
    pub ids: Vec<u32>,
    pub count: u32,
}

/// Body of a `StoreWant` frame: ids the addressee is missing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreWant {
    pub ids: Vec<u32>,
}

/// Telemetry beacon from infrastructure nodes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Telemetry {
    pub timestamp_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub battery_mv: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub battery_pct: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub uptime_s: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub airtime_used_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rx_frames: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tx_frames: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub temperature_c: Option<i8>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub solar_mv: Option<u16>,
}

/// Envelope used when a message must be delivered to a group (channel) address.
///
/// The plaintext is CBOR of this struct, sealed with the channel key. Keeping the sender
/// identity inside the ciphertext keeps membership private from relays.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupMessage {
    pub sender: PublicIdentity,
    pub message: AppMessage,
    pub signature: Vec<u8>,
}

/// Envelope used when a direct message is larger than one frame and streamed as a resource.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DirectResource {
    pub sender: PublicIdentity,
    pub message: AppMessage,
    pub sha256: [u8; 32],
}

/// Canonical bytes signed for group authorship: a domain-separated SHA-256 digest.
pub fn group_signature_payload(sender: &NodeId, channel: &NodeId, msg: &AppMessage) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    out.extend_from_slice(b"openmesh/v1/group-msg");
    out.extend_from_slice(&sender.0);
    out.extend_from_slice(&channel.0);
    out.extend_from_slice(&cbor_bytes(msg));
    out
}

/// Canonical bytes covered by an announce signature.
pub fn announce_signature_payload(src: &NodeId, seq: u32, announce: &Announce) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    out.extend_from_slice(b"openmesh/v1/announce");
    out.extend_from_slice(&src.0);
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(&cbor_bytes(announce));
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

    #[test]
    fn app_message_cbor_roundtrip() {
        let msg = AppMessage {
            id: 42,
            kind: MessageKind::Text,
            sent_at_ms: 1_700_000_000_000,
            text: Some("hello mesh".into()),
            reply_to: None,
            thread: None,
            attachment: None,
            position: None,
            sender: None,
            signature: None,
        };
        let bytes = cbor_bytes(&msg);
        let back: AppMessage = cbor_from_bytes(&bytes).unwrap();
        assert_eq!(msg, back);
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
        let msg = AppMessage {
            id: 1,
            kind: MessageKind::Attachment,
            sent_at_ms: 0,
            text: None,
            reply_to: None,
            thread: None,
            attachment: Some(att),
            position: None,
            sender: None,
            signature: None,
        };
        assert_eq!(cbor_from_bytes::<AppMessage>(&cbor_bytes(&msg)).unwrap(), msg);
    }

    #[test]
    fn position_roundtrip() {
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
    }
}

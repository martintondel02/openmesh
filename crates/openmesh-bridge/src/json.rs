//! JSON schemas for the bridge WebSocket + REST API.
//!
//! These types are the exact wire representation documented in
//! `docs/architecture/WEBSOCKET-API.md`; `crates/openmesh-core` types are converted at the
//! edge so the bridge storage layer stays decoupled from the protocol types.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::json;

use openmesh_core::services::caps;
use openmesh_core::{MessageKind, NodeClass, NodeId, Position as CorePosition, PublicIdentity};

/// API envelope version (`v` field).
pub const API_VERSION: u8 = 1;

/// Serialize one server event into the documented envelope.
pub fn event_json<T: Serialize>(typ: &str, data: Option<T>) -> String {
    let mut map = serde_json::Map::new();
    map.insert("v".into(), json!(API_VERSION));
    map.insert("type".into(), json!(typ));
    if let Some(data) = data {
        map.insert(
            "data".into(),
            serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
        );
    }
    serde_json::Value::Object(map).to_string()
}

/// Serialize a response that echoes the client request `id`.
pub fn response_json<T: Serialize>(typ: &str, id: Option<&str>, data: Option<T>) -> String {
    let mut map = serde_json::Map::new();
    map.insert("v".into(), json!(API_VERSION));
    map.insert("type".into(), json!(typ));
    if let Some(id) = id {
        map.insert("id".into(), json!(id));
    }
    if let Some(data) = data {
        map.insert(
            "data".into(),
            serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
        );
    }
    serde_json::Value::Object(map).to_string()
}

/// Error envelope with the documented error codes.
pub fn error_json(id: Option<&str>, code: &str, message: &str) -> String {
    let data = ErrorData {
        code: code.to_string(),
        message: message.to_string(),
    };
    response_json("error", id, Some(data))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorData {
    pub code: String,
    pub message: String,
}

// ---------------------------------------------------------------------------------------------
// Incoming client frames
// ---------------------------------------------------------------------------------------------

/// One decoded client → server WebSocket message.
#[derive(Clone, Debug, Deserialize)]
pub struct ClientMsg {
    #[serde(default)]
    pub v: u8,
    #[serde(rename = "type")]
    pub typ: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub data: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SyncData {
    #[serde(default)]
    pub since_ms: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SendMessageData {
    pub to: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub reply_to: Option<u32>,
    #[serde(default)]
    pub attachment_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SendGroupMessageData {
    pub channel_id: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub attachment_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SendPositionData {
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub position: Option<PositionJson>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MsgIdData {
    pub msg_id: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AddContactData {
    pub node_id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Base64 of the 72-byte public identity; optional when the node is already known.
    #[serde(default)]
    pub identity: Option<String>,
    #[serde(default)]
    pub verify_key: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ChannelCreateData {
    pub name: String,
    #[serde(default)]
    pub psk: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ChannelIdData {
    pub channel_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GetMessagesData {
    #[serde(default)]
    pub peer: Option<String>,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub before_ms: Option<u64>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

pub fn default_limit() -> usize {
    100
}

// ---------------------------------------------------------------------------------------------
// Shared value objects
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct PositionJson {
    pub lat: f64,
    pub lon: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_m: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accuracy_m: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_cm_s: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading_deg: Option<u16>,
    pub timestamp_ms: u64,
}

impl PositionJson {
    pub fn from_core(p: &CorePosition) -> Self {
        Self {
            lat: p.lat_e7 as f64 / 1e7,
            lon: p.lon_e7 as f64 / 1e7,
            alt_m: p.alt_m,
            accuracy_m: p.accuracy_m,
            speed_cm_s: p.speed_cm_s,
            heading_deg: p.heading_deg,
            timestamp_ms: p.timestamp_ms,
        }
    }

    pub fn to_core(&self) -> CorePosition {
        CorePosition {
            lat_e7: (self.lat * 1e7).round() as i32,
            lon_e7: (self.lon * 1e7).round() as i32,
            alt_m: self.alt_m,
            accuracy_m: self.accuracy_m,
            speed_cm_s: self.speed_cm_s,
            heading_deg: self.heading_deg,
            timestamp_ms: self.timestamp_ms,
        }
    }
}

/// Convert capability bits into the human-readable array used by `node` events.
pub fn caps_to_strings(bits: u16) -> Vec<String> {
    const MAP: &[(u16, &str)] = &[
        (caps::GPS, "gps"),
        (caps::WIFI, "wifi"),
        (caps::ETHERNET, "ethernet"),
        (caps::STORE, "store"),
        (caps::RELAY, "relay"),
        (caps::BLE, "ble"),
        (caps::USB, "usb"),
        (caps::SCREEN, "screen"),
        (caps::AUDIO_OUT, "audio_out"),
        (caps::SOLAR, "solar"),
    ];
    MAP.iter()
        .filter(|(bit, _)| bits & bit != 0)
        .map(|(_, name)| (*name).to_string())
        .collect()
}

pub fn node_class_str(class: NodeClass) -> &'static str {
    class.as_str()
}

pub fn parse_node_class(s: &str) -> Option<NodeClass> {
    Some(match s {
        "client" => NodeClass::Client,
        "repeater" => NodeClass::Repeater,
        "gateway" => NodeClass::Gateway,
        "room_server" | "roomserver" => NodeClass::RoomServer,
        "tracker" => NodeClass::Tracker,
        _ => return None,
    })
}

pub fn message_kind_str(kind: MessageKind) -> &'static str {
    match kind {
        MessageKind::Text => "text",
        MessageKind::Position => "position",
        MessageKind::Attachment => "attachment",
        MessageKind::GroupInvite => "group_invite",
        MessageKind::Telemetry => "telemetry",
        MessageKind::Control => "control",
    }
}

pub fn parse_message_kind(s: &str) -> Option<MessageKind> {
    Some(match s {
        "text" => MessageKind::Text,
        "position" => MessageKind::Position,
        "attachment" => MessageKind::Attachment,
        "group_invite" => MessageKind::GroupInvite,
        "telemetry" => MessageKind::Telemetry,
        "control" => MessageKind::Control,
        _ => return None,
    })
}

pub fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .ok()
}

pub fn identity_to_b64(ident: &PublicIdentity) -> String {
    b64(&ident.to_bytes())
}

pub fn identity_from_b64(s: &str) -> Option<PublicIdentity> {
    let bytes = b64_decode(s)?;
    PublicIdentity::from_bytes(&bytes).ok()
}

// ---------------------------------------------------------------------------------------------
// Server → client event payloads (§3)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize)]
pub struct NodeSelfJson {
    pub node_id: String,
    pub name: String,
    pub class: String,
    pub caps: u16,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RadioJson {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub connected: bool,
    pub profile: String,
    pub region: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct StoreCountsJson {
    pub messages: usize,
    pub contacts: usize,
    pub channels: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct HelloJson {
    pub bridge_version: String,
    pub protocol: String,
    pub node: NodeSelfJson,
    pub radios: Vec<RadioJson>,
    pub features: Vec<String>,
    pub store: StoreCountsJson,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RadioStatusJson {
    pub connected: bool,
    pub tx_queue: u64,
    pub airtime_used_pct: f64,
    pub rssi: Option<i16>,
    pub snr: Option<f32>,
    pub frequency_mhz: f64,
    pub tx_power_dbm: i8,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MeshStatusJson {
    pub tx_frames: u64,
    pub rx_frames: u64,
    pub forwarded: u64,
    pub dropped: u64,
    pub known_nodes: usize,
    pub stored_envelopes: usize,
    pub duty_cycle_pct: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct StorageStatusJson {
    pub used_bytes: u64,
    pub db_path: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct StatusJson {
    pub uptime_s: u64,
    pub radio: RadioStatusJson,
    pub mesh: MeshStatusJson,
    pub storage: StorageStatusJson,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NodeJson {
    pub node_id: String,
    pub name: String,
    pub class: String,
    pub caps: Vec<String>,
    pub hops: u8,
    pub last_seen_ms: u64,
    pub rssi: Option<i16>,
    pub snr: Option<f32>,
    pub position: Option<PositionJson>,
    pub battery_pct: Option<u8>,
    pub is_contact: bool,
    pub public_key: String,
}

impl NodeJson {
    pub fn from_core(info: &openmesh_core::NodeInfo) -> Self {
        Self {
            node_id: info.node_id.to_hex(),
            name: info.name.clone(),
            class: node_class_str(info.class).to_string(),
            caps: caps_to_strings(info.caps),
            hops: info.hops,
            last_seen_ms: info.last_seen_ms,
            rssi: info.rssi,
            snr: info.snr,
            position: info.last_position.as_ref().map(PositionJson::from_core),
            battery_pct: info.battery_pct,
            is_contact: info.is_contact,
            public_key: identity_to_b64(&info.ident),
        }
    }

    /// Minimal constructor for a node we have an identity for but no announce yet.
    pub fn from_identity(ident: &PublicIdentity, name: &str, is_contact: bool) -> Self {
        Self {
            node_id: ident.node_id.to_hex(),
            name: name.to_string(),
            class: "client".to_string(),
            caps: Vec::new(),
            hops: 0,
            last_seen_ms: 0,
            rssi: None,
            snr: None,
            position: None,
            battery_pct: None,
            is_contact,
            public_key: identity_to_b64(ident),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct AttachmentJson {
    pub id: String,
    pub mime: String,
    pub size: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub progress: f64,
}

/// `message` / `message_update` payload (§3.4, §3.5).
#[derive(Clone, Debug, Serialize)]
pub struct MessageJson {
    pub msg_id: u32,
    pub peer: String,
    pub from: String,
    pub to: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub sent_at_ms: u64,
    pub received_at_ms: u64,
    pub outgoing: bool,
    pub delivered: bool,
    pub read: bool,
    pub reply_to: Option<u32>,
    pub thread: Option<u32>,
    pub attachment: Option<AttachmentJson>,
    pub position: Option<PositionJson>,
}

/// `group_message` payload (§3.6).
#[derive(Clone, Debug, Serialize)]
pub struct GroupMessageJson {
    pub msg_id: u32,
    pub channel_id: String,
    pub channel_name: String,
    pub from: String,
    pub from_name: String,
    pub sender_key: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub sent_at_ms: u64,
    pub outgoing: bool,
    pub signature_valid: bool,
    pub attachment: Option<AttachmentJson>,
    pub position: Option<PositionJson>,
}

/// `message_update` payload: same message with only changed fields present (§3.5).
#[derive(Clone, Debug, Serialize)]
pub struct MessageUpdateJson {
    pub msg_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment: Option<AttachmentJson>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AckJson {
    pub msg_id: u32,
    pub peer: String,
    pub kind: String,
    pub at_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PositionEventJson {
    pub node_id: String,
    pub channel_id: Option<String>,
    pub position: PositionJson,
}

#[derive(Clone, Debug, Serialize)]
pub struct TelemetryJson {
    pub node_id: String,
    pub battery_mv: Option<u16>,
    pub battery_pct: Option<u8>,
    pub uptime_s: Option<u32>,
    pub airtime_used_ms: Option<u32>,
    pub temperature_c: Option<i8>,
    pub solar_mv: Option<u16>,
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct StoreOfferJson {
    pub relay: String,
    pub count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResourceProgressJson {
    pub peer: String,
    pub msg_id: u32,
    pub res_id: u32,
    pub received: u16,
    pub total: u16,
    /// `"in"` or `"out"`.
    pub direction: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ConfigNodeJson {
    pub name: String,
    pub class: String,
    pub announce_interval_s: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ConfigRadioJson {
    pub profile: String,
    pub region: String,
    pub tx_power_dbm: i8,
    pub frequency_mhz: f64,
    pub interface: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ConfigFeaturesJson {
    pub auto_announce: bool,
    pub store_forward: bool,
    pub position_sharing: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ConfigChannelJson {
    pub channel_id: String,
    pub name: String,
    pub psk_set: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ConfigJson {
    pub node: ConfigNodeJson,
    pub radio: ConfigRadioJson,
    pub features: ConfigFeaturesJson,
    pub channels: Vec<ConfigChannelJson>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContactJson {
    pub node_id: String,
    pub name: Option<String>,
    pub identity: String,
    pub last_seen_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContactsJson {
    pub contacts: Vec<ContactJson>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChannelJson {
    pub channel_id: String,
    pub name: String,
    pub psk_set: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChannelsJson {
    pub channels: Vec<ChannelJson>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChannelCreatedJson {
    pub channel_id: String,
    pub name: String,
    /// Base64 32-byte PSK; returned exactly once on creation.
    pub psk: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct UrlJson {
    pub url: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SendResultJson {
    pub msg_id: u32,
    pub est_airtime_ms: u32,
    pub queued: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct QueuedResultJson {
    pub est_airtime_ms: u32,
    pub queued: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct SyncDoneJson {
    pub messages: usize,
    pub nodes: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct NodesJson {
    pub nodes: Vec<NodeJson>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MessagesJson {
    pub messages: Vec<MessageJson>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GroupMessagesJson {
    pub messages: Vec<GroupMessageJson>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct LinkJson {
    pub link: u16,
    pub kind: String,
    pub label: String,
    pub rssi: Option<i16>,
    pub snr: Option<f32>,
    pub tx_frames: u64,
    pub rx_frames: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct StatsJson {
    #[serde(flatten)]
    pub mesh: MeshStatusJson,
    pub links: Vec<LinkJson>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AttachmentUploadJson {
    pub id: String,
    pub size: u64,
    pub sha256: String,
}

pub fn json_value<T: Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

/// Encode a node id from user input (hex with optional `0x` prefix).
pub fn parse_node_id(s: &str) -> Option<NodeId> {
    NodeId::from_hex(s).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_has_version_type_and_data() {
        let s = event_json("hello", Some(json!({"bridge_version": "0.1.0"})));
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(v["type"], "hello");
        assert_eq!(v["data"]["bridge_version"], "0.1.0");
        assert!(v.get("id").is_none());
    }

    #[test]
    fn response_echoes_id() {
        let s = response_json("send_result", Some("m1"), Some(json!({"msg_id": 7})));
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["id"], "m1");
        assert_eq!(v["data"]["msg_id"], 7);
    }

    #[test]
    fn errors_use_documented_codes() {
        let s = error_json(Some("x"), "bad_request", "missing field");
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["type"], "error");
        assert_eq!(v["data"]["code"], "bad_request");
        assert_eq!(v["id"], "x");
    }

    #[test]
    fn client_message_parses() {
        let raw = r#"{"v":1,"type":"send_message","id":"m1","data":{"to":"aabbccddeeff0011","text":"hi"}}"#;
        let msg: ClientMsg = serde_json::from_str(raw).unwrap();
        assert_eq!(msg.typ, "send_message");
        assert_eq!(msg.id.as_deref(), Some("m1"));
        let data: SendMessageData = serde_json::from_value(msg.data).unwrap();
        assert_eq!(data.text.as_deref(), Some("hi"));
    }

    #[test]
    fn position_roundtrips_through_json() {
        let p = CorePosition {
            lat_e7: 599_137_000,
            lon_e7: 107_522_000,
            alt_m: Some(45),
            accuracy_m: Some(8),
            speed_cm_s: Some(120),
            heading_deg: Some(180),
            timestamp_ms: 1_700_000_000_000,
        };
        let j = PositionJson::from_core(&p);
        assert!((j.lat - 59.9137).abs() < 1e-9);
        assert_eq!(j.to_core(), p);
        let s = serde_json::to_string(&j).unwrap();
        assert!(s.contains("\"lat\":59.9137"));
    }

    #[test]
    fn caps_render_as_strings() {
        let caps = caps::RELAY | caps::STORE | caps::GPS;
        let names = caps_to_strings(caps);
        assert_eq!(names, vec!["gps", "store", "relay"]);
        assert!(caps_to_strings(0).is_empty());
    }

    #[test]
    fn identity_base64_roundtrip() {
        let ident = openmesh_core::Identity::from_seed(&[5u8; 32]).public();
        let encoded = identity_to_b64(&ident);
        let decoded = identity_from_b64(&encoded).unwrap();
        assert_eq!(decoded, ident);
        assert_eq!(b64_decode(&encoded).unwrap().len(), 72);
    }

    #[test]
    fn node_json_shape() {
        let ident = openmesh_core::Identity::from_seed(&[3u8; 32]).public();
        let node = NodeJson::from_identity(&ident, "alice", true);
        let v = serde_json::to_value(&node).unwrap();
        assert_eq!(v["node_id"], ident.node_id.to_hex());
        assert_eq!(v["class"], "client");
        assert_eq!(v["is_contact"], true);
        assert!(v["caps"].is_array());
    }

    #[test]
    fn class_and_kind_parsing() {
        for class in ["client", "repeater", "gateway", "room_server", "tracker"] {
            assert!(parse_node_class(class).is_some());
        }
        assert!(parse_node_class("nope").is_none());
        assert_eq!(parse_message_kind("text"), Some(MessageKind::Text));
        assert!(parse_message_kind("nope").is_none());
    }
}

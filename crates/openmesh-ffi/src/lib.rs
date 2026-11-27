//! # openmesh-ffi — UniFFI surface for OpenMesh mobile clients
//!
//! Thin, typed wrapper over [`openmesh_core`] for Android (`apps/android`) and, later, iOS. The
//! normative contract is `docs/design/android-ga.md` §1.
//!
//! Design rules enforced here:
//!
//! * one [`OpenMeshNode`] per local identity, backed by `Arc<Mutex<Engine>>` and `Send + Sync`;
//! * every method takes `&self`, no callbacks into platform code and no panics across the
//!   boundary (all failures map to [`FfiError`]);
//! * time is always passed in (`nowMs`) — the engine never reads the wall clock;
//! * the identity seed is copied at construction and never returned;
//! * attachments are encapsulated: [`OpenMeshNode::send_attachment`] creates the message,
//!   manifest and resource transfer and returns the events plus outbound frames;
//! * engine state export/import is opaque JSON (`export_state`/`import_state`), while message
//!   history lives in the app's database.

#![warn(missing_docs)]

uniffi::setup_scaffolding!();

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use base64::Engine as _;
use sha2::{Digest, Sha256};

use openmesh_core::regulatory::{self, RegulatoryOverride};
use openmesh_core::util;
use openmesh_core::{
    Ack, AckKind, AppMessage, AttachmentRef, Engine, EngineConfig, Error, Event, Identity,
    LinkInfo, MessageKind, NodeClass, NodeId, NodeInfo, Position, Processed, PublicIdentity,
    RelayConfig, ResourceKind, Role, SendMode, StateSnapshot, Target,
};
use openmesh_voice::{VoiceError, VoiceTier};

/// Accepted identity seed length (32 bytes).
pub const SEED_LEN: usize = 32;
/// Maximum OMP frame accepted by [`OpenMeshNode::ingest_frame`] (240 bytes).
pub const MAX_FRAME_LEN: usize = openmesh_core::MTU;
/// (filename, duration_ms) remembered from an attachment message until its resource completes.
type IncomingAttachmentMeta = (Option<String>, Option<u64>);
/// Chunk size advertised in outgoing attachment references (matches PROTOCOL.md §9).
const ATTACHMENT_CHUNK_BYTES: u16 = 180;
/// Relay store-and-forward window used when `storeForward` is enabled.
const RELAY_STORE_CAPACITY: usize = 256;

fn to_u64(value: i64, what: &str) -> Result<u64, FfiError> {
    u64::try_from(value).map_err(|_| FfiError::invalid(format!("{what} must not be negative")))
}

fn to_u32(value: i64, what: &str) -> Result<u32, FfiError> {
    u32::try_from(value)
        .map_err(|_| FfiError::invalid(format!("{what} is out of range (expected 0..=4294967295)")))
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn b64_decode(value: &str) -> Option<Vec<u8>> {
    let value = value.trim();
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .ok()
        .or_else(|| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(value)
                .ok()
        })
}

fn b64_url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn url_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Types (docs/design/android-ga.md §1)
// ---------------------------------------------------------------------------------------------

/// Engine configuration supplied at construction time.
#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiConfig {
    /// Display name announced to the mesh (max 32 UTF-8 bytes).
    pub name: String,
    /// `client`, `repeater`, `gateway` or `tracker`.
    pub node_class: String,
    /// LoRa profile (`LONG_SLOW`, `LONG_FAST`, `MEDIUM_FAST`, `SHORT_FAST`, `SHORT_TURBO`).
    pub profile: String,
    /// Regulatory region (`EU868`, `EU433`, `US915`, `AU915`, `IN865`, `KR920`, `AS923`).
    pub region: String,
    /// Enable store-and-forward for other nodes (repeater/gateway behaviour).
    pub store_forward: bool,
    /// Accept and decrypt channel traffic.
    pub accept_channels: bool,
    /// Announce period in seconds (minimum 300).
    pub announce_interval_s: i64,
}

impl Default for FfiConfig {
    fn default() -> Self {
        Self {
            name: "openmesh".to_string(),
            node_class: "client".to_string(),
            profile: "MEDIUM_FAST".to_string(),
            region: "EU868".to_string(),
            store_forward: false,
            accept_channels: true,
            announce_interval_s: 1800,
        }
    }
}

/// Destination of an application message.
#[derive(Clone, Debug, uniffi::Enum)]
pub enum FfiTarget {
    /// Direct, end-to-end encrypted message to one node (hex node id).
    Direct {
        /// 8-byte node id, lowercase hex.
        node_id_hex: String,
    },
    /// Group message to a joined channel (hex channel id).
    Channel {
        /// 8-byte channel id, lowercase hex.
        channel_id_hex: String,
    },
}

/// Preferred transmission mode of one outbound frame.
#[derive(Clone, Debug, uniffi::Enum)]
pub enum FfiSendMode {
    /// Broadcast on every interface.
    Flood,
    /// Point-to-point to the peer if the link exists (hex node id), otherwise flood.
    Direct {
        /// Neighbour node id, lowercase hex.
        peer_hex: String,
    },
}

/// One OMP frame to put on the air, in order.
#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiOutbound {
    /// Encoded OMP/1 frame (at most 240 bytes).
    pub frame: Vec<u8>,
    /// Preferred transmission mode.
    pub mode: FfiSendMode,
}

/// A node the engine has learned about.
#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiNode {
    /// 8-byte node id, lowercase hex.
    pub node_id_hex: String,
    /// Announced display name (may be empty).
    pub name: String,
    /// `client`, `repeater`, `gateway`, `room_server` or `tracker`.
    pub class: String,
    /// Capability strings (`relay`, `store`, `gps`, `wifi`, `ethernet`, `ble`, `usb`, ...).
    pub caps: Vec<String>,
    /// Announce hop count when known.
    pub hops: i32,
    /// Unix milliseconds when the node was last heard (embedder time).
    pub last_seen_ms: i64,
    /// RSSI in dBm when known.
    pub rssi: Option<i32>,
    /// SNR in dB when known.
    pub snr: Option<f64>,
    /// Battery percentage from telemetry when known.
    pub battery_pct: Option<i32>,
    /// Last reported latitude in degrees when known.
    pub lat: Option<f64>,
    /// Last reported longitude in degrees when known.
    pub lon: Option<f64>,
}

/// A channel invitation: returned once by create/join with the PSK.
#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiChannel {
    /// 8-byte channel id, lowercase hex.
    pub channel_id_hex: String,
    /// Human-readable channel name.
    pub name: String,
    /// `openmesh://channel?...` invite URL (contains the PSK; share via QR).
    pub invite_url: String,
    /// Base64 32-byte channel key — returned once, never persisted by the FFI layer.
    pub psk_b64: String,
}

/// A joined channel (no PSK).
#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiChannelInfo {
    /// 8-byte channel id, lowercase hex.
    pub channel_id_hex: String,
    /// Human-readable channel name.
    pub name: String,
    /// `openmesh://channel?...` invite URL for sharing.
    pub invite_url: String,
}

/// Engine counters for the status screen.
#[derive(Clone, Copy, Debug, uniffi::Record)]
pub struct FfiStats {
    /// Frames emitted for transmission.
    pub tx_frames: i64,
    /// Frames received and processed.
    pub rx_frames: i64,
    /// Frames re-broadcast for other nodes.
    pub forwarded: i64,
    /// Frames dropped (malformed, invalid signature, unknown sender, exhausted TTL, ...).
    pub dropped: i64,
    /// Known nodes in the peer book.
    pub nodes: i64,
    /// Joined channels.
    pub channels: i64,
    /// Best-effort transmitted airtime in milliseconds.
    pub airtime_ms: i64,
    /// Effective duty cycle used by the engine (override or region default); `null` for fair-use
    /// regions with no numeric limit.
    pub effective_duty_pct: Option<i32>,
}

/// One event produced by the engine.
#[derive(Clone, Debug, uniffi::Enum)]
pub enum FfiEvent {
    /// A direct message was received and decrypted.
    Message {
        /// Sender node id, lowercase hex.
        from: String,
        /// Message id chosen by the sender.
        msg_id: i64,
        /// Text payload when the message carries text.
        text: Option<String>,
        /// Local (embedder) time when the event was produced, in Unix milliseconds.
        received_at_ms: i64,
        /// Channel id when the message arrived on a channel.
        channel: Option<String>,
    },
    /// A channel/group message was received, decrypted and signature-verified.
    GroupMessage {
        /// Channel id, lowercase hex.
        channel: String,
        /// Sender node id, lowercase hex.
        from: String,
        /// Message id.
        msg_id: i64,
        /// Text payload when present.
        text: Option<String>,
    },
    /// A delivery/read/resource acknowledgement arrived.
    Ack {
        /// Message being acknowledged.
        msg_id: i64,
        /// `delivered`, `read`, `resource_window` or `resource_complete`.
        kind: String,
    },
    /// A node was discovered or refreshed.
    NodeSeen {
        /// The node as currently known.
        node: FfiNode,
    },
    /// A position report was received.
    Position {
        /// Reporting node id, lowercase hex.
        from: String,
        /// Channel id when it arrived on a channel.
        channel: Option<String>,
        /// Latitude in degrees.
        lat: f64,
        /// Longitude in degrees.
        lon: f64,
        /// Altitude in metres when known.
        alt_m: Option<i32>,
        /// Horizontal accuracy in metres when known.
        accuracy_m: Option<i32>,
        /// Speed in cm/s when known.
        speed_cm_s: Option<i32>,
        /// Heading in degrees when known.
        heading_deg: Option<i32>,
        /// Fix timestamp in Unix milliseconds.
        timestamp_ms: i64,
    },
    /// An infrastructure node reported telemetry.
    Telemetry {
        /// Reporting node (with battery/position merged when known).
        node: FfiNode,
    },
    /// Progress of an incoming or outgoing resource transfer.
    ResourceProgress {
        /// Peer the transfer is with, lowercase hex.
        peer: String,
        /// Message that owns the resource (0 when unknown for incoming transfers).
        msg_id: i64,
        /// Resource id.
        res_id: i64,
        /// Chunks received/accepted so far.
        received: i64,
        /// Total chunks expected.
        total: i64,
    },
    /// A resource transfer completed, verified and decrypted.
    ResourceReceived {
        /// Sender node id, lowercase hex.
        peer: String,
        /// Channel id when the resource arrived on a channel.
        channel: Option<String>,
        /// Message that owns the resource.
        msg_id: i64,
        /// MIME type from the manifest.
        mime: String,
        /// Original filename when known.
        filename: Option<String>,
        /// Decrypted payload (attachment bytes; long text arrives as CBOR).
        data: Vec<u8>,
        /// Audio duration in milliseconds when applicable.
        duration_ms: Option<i64>,
    },
    /// One of our outgoing attachments was fully acknowledged.
    AttachmentSent {
        /// Peer that completed the transfer, lowercase hex.
        peer: String,
        /// Message the attachment belongs to.
        msg_id: i64,
    },
    /// A relay holds envelopes for this node (store-and-forward).
    StoreOffer {
        /// Relay node id, lowercase hex.
        relay: String,
        /// Number of envelopes offered.
        count: i64,
    },
    /// A non-fatal protocol problem worth surfacing in logs/UI.
    Error {
        /// Plain-language message (never contains secrets).
        message: String,
    },
}

/// Result of feeding input to the engine or originating a message.
#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiProcessed {
    /// Events to surface to the UI, in order.
    pub events: Vec<FfiEvent>,
    /// Frames to transmit, in order (each at most 240 bytes).
    pub outbound: Vec<FfiOutbound>,
    /// Message id of the originated message, for send methods only.
    ///
    /// This field is additive to the v0.4.0 `FfiProcessed` table in `android-ga.md` §1: the app
    /// needs the id to key its Room rows before any ack/echo event exists.
    pub msg_id: Option<i64>,
}

/// Errors crossing the FFI boundary (mapped to a Kotlin exception hierarchy).
#[derive(Clone, Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    /// A caller-supplied value (node id, seed, config, JSON) is invalid.
    #[error("invalid argument: {msg}")]
    InvalidArgument {
        /// Plain-language reason.
        msg: String,
    },
    /// The peer identity is not in the peer book yet (wait for an announce or add a contact).
    #[error("unknown peer: {msg}")]
    UnknownPeer {
        /// Plain-language reason.
        msg: String,
    },
    /// A frame or payload exceeded the protocol limit.
    #[error("payload too large: {msg}")]
    PayloadTooLarge {
        /// Plain-language reason.
        msg: String,
    },
    /// The core rejected the operation or the FFI layer hit an internal problem.
    #[error("internal error: {msg}")]
    Internal {
        /// Plain-language reason.
        msg: String,
    },
}

impl FfiError {
    fn invalid(msg: impl Into<String>) -> Self {
        Self::InvalidArgument { msg: msg.into() }
    }

    fn internal(msg: impl Into<String>) -> Self {
        Self::Internal { msg: msg.into() }
    }
}

// ---------------------------------------------------------------------------------------------
// Object
// ---------------------------------------------------------------------------------------------

/// A local OpenMesh node: protocol engine plus the small amount of app-level state the FFI
/// layer needs (channel keys for invite URLs, message/resource correlations).
#[derive(uniffi::Object)]
pub struct OpenMeshNode {
    engine: Mutex<Engine>,
    config: Mutex<FfiConfig>,
    node_id_hex: String,
    public_identity: Vec<u8>,
    next_res_id: AtomicU32,
    last_now_ms: AtomicU64,
    /// Outgoing resource id → message id, for `AttachmentSent`.
    outgoing_resources: Mutex<HashMap<u32, u32>>,
    /// Received attachment message id → (filename, duration_ms), for `ResourceReceived`.
    incoming_attachments: Mutex<HashMap<u32, IncomingAttachmentMeta>>,
    /// Direct message id → sender, for read receipts (`markRead`).
    received_from: Mutex<HashMap<u32, NodeId>>,
    /// Channel id → PSK, so channel invite URLs can be regenerated.
    channel_keys: Mutex<HashMap<NodeId, [u8; 32]>>,
}

impl fmt::Debug for OpenMeshNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print key material; the node id and public identity are public.
        f.debug_struct("OpenMeshNode")
            .field("node_id", &self.node_id_hex)
            .finish_non_exhaustive()
    }
}

#[uniffi::export]
impl OpenMeshNode {
    /// Create a node from a 32-byte identity seed and configuration.
    ///
    /// The seed must come from the platform CSPRNG and be stored encrypted by the app; it is
    /// copied into the engine and never returned by any method.
    #[uniffi::constructor]
    pub fn new(seed: Vec<u8>, config: FfiConfig) -> Result<Arc<Self>, FfiError> {
        if seed.len() != SEED_LEN {
            return Err(FfiError::invalid(format!(
                "seed must be {SEED_LEN} bytes, got {}",
                seed.len()
            )));
        }
        validate_config(&config)?;
        let mut seed_bytes = [0u8; SEED_LEN];
        seed_bytes.copy_from_slice(&seed);
        drop(seed);
        let identity = Identity::from_seed(&seed_bytes);
        let public_identity = identity.public();
        let node_id_hex = public_identity.node_id.to_hex();
        let engine_config = engine_config_for(&config)?;
        let engine = Engine::new(identity, engine_config);
        let seed_counter =
            u32::from_be_bytes([seed_bytes[0], seed_bytes[1], seed_bytes[2], seed_bytes[3]]) | 1;
        Ok(Arc::new(Self {
            engine: Mutex::new(engine),
            config: Mutex::new(config),
            node_id_hex,
            public_identity: public_identity.to_bytes().to_vec(),
            next_res_id: AtomicU32::new(seed_counter.wrapping_mul(0x85EB_CA6B)),
            last_now_ms: AtomicU64::new(0),
            outgoing_resources: Mutex::new(HashMap::new()),
            incoming_attachments: Mutex::new(HashMap::new()),
            received_from: Mutex::new(HashMap::new()),
            channel_keys: Mutex::new(HashMap::new()),
        }))
    }

    /// This node's self-certifying id as lowercase hex (16 characters).
    pub fn node_id(&self) -> String {
        self.node_id_hex.clone()
    }

    /// The 72-byte public identity (`node_id || Ed25519 public || X25519 public`), base64.
    ///
    /// Public data: share it via the contact QR/invite.
    pub fn public_identity_b64(&self) -> String {
        b64(&self.public_identity)
    }

    /// The current configuration as JSON (same shape as [`FfiConfig`]).
    pub fn config_json(&self) -> String {
        let config = self
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let region = region_for(&config);
        let (regulatory_override, effective_duty_pct) = {
            let engine = match self.engine.lock() {
                Ok(engine) => engine,
                Err(poisoned) => poisoned.into_inner(),
            };
            (
                engine.config().regulatory.clone(),
                engine.config().effective_duty_pct(region),
            )
        };
        let mut value = config_to_json(&config);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "regulatoryOverride".into(),
                override_to_json(&regulatory_override),
            );
            object.insert(
                "overrideActive".into(),
                serde_json::json!(regulatory_override.enabled),
            );
            object.insert(
                "effectiveDutyPct".into(),
                serde_json::json!(effective_duty_pct.map(i32::from)),
            );
        }
        serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
    }

    /// Apply a partial configuration JSON patch (unknown fields are rejected).
    pub fn set_config(&self, json_patch: String) -> Result<(), FfiError> {
        let patch: serde_json::Value = serde_json::from_str(&json_patch)
            .map_err(|e| FfiError::invalid(format!("config patch is not valid JSON: {e}")))?;
        let object = patch
            .as_object()
            .ok_or_else(|| FfiError::invalid("config patch must be a JSON object"))?;
        let mut config = self
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let mut regulatory_override: Option<RegulatoryOverride> = None;
        for (key, value) in object {
            let key = key.as_str();
            match key {
                "name" => {
                    config.name = string_value(key, value)?;
                }
                "nodeClass" | "node_class" => {
                    config.node_class = string_value(key, value)?;
                }
                "profile" => {
                    config.profile = string_value(key, value)?;
                }
                "region" => {
                    config.region = string_value(key, value)?;
                }
                "storeForward" | "store_forward" => {
                    config.store_forward = bool_value(key, value)?;
                }
                "acceptChannels" | "accept_channels" => {
                    config.accept_channels = bool_value(key, value)?;
                }
                "announceIntervalS" | "announce_interval_s" => {
                    config.announce_interval_s = value
                        .as_i64()
                        .ok_or_else(|| FfiError::invalid(format!("{key} must be an integer")))?;
                }
                "regulatoryOverride" | "regulatory_override" => {
                    let current = match &regulatory_override {
                        Some(current) => current.clone(),
                        None => {
                            let engine = self.lock_engine()?;
                            engine.config().regulatory.clone()
                        }
                    };
                    regulatory_override = Some(parse_override_patch(value, &current)?);
                }
                other => {
                    return Err(FfiError::invalid(format!("unknown config field '{other}'")));
                }
            }
        }
        validate_config(&config)?;
        let region = region_for(&config);
        let mut engine_config = engine_config_for(&config)?;
        {
            let mut engine = self.lock_engine()?;
            engine_config.regulatory = match regulatory_override {
                Some(override_config) => {
                    if let Some(frequency) = override_config.frequency_khz {
                        if !regulatory::frequency_supported(frequency) {
                            return Err(FfiError::invalid(format!(
                                "frequency {frequency} kHz is outside the supported radio range {}..={} kHz",
                                regulatory::RADIO_MIN_FREQUENCY_KHZ,
                                regulatory::RADIO_MAX_FREQUENCY_KHZ
                            )));
                        }
                    }
                    regulatory::validate(region, &override_config)
                        .map_err(|error| FfiError::invalid(error.message()))?;
                    override_config
                }
                None => engine.config().regulatory.clone(),
            };
            engine.set_config(engine_config);
        }
        *self
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = config;
        Ok(())
    }

    /// Apply the duty-aware resource chunk pacing hint, in milliseconds.
    ///
    /// Derive the value with [`openmesh_resource_pace_interval_ms`] from the active radio profile
    /// and region duty cycle, and call this after every config change (especially profile/region
    /// changes). The engine then releases at most its internal chunk batch per interval on both
    /// tick- and ack-driven paths, so the companion radio's FIFO is not overrun in duty-cycle
    /// regions and a fast app tick cannot offer chunks faster than airtime allows.
    pub fn set_resource_pace_interval_ms(&self, ms: u64) -> Result<(), FfiError> {
        let mut engine = self.lock_engine()?;
        engine.set_resource_pace_interval_ms(ms);
        Ok(())
    }

    /// Feed one complete OMP frame received from the radio.
    pub fn ingest_frame(&self, frame: Vec<u8>, now_ms: i64) -> Result<FfiProcessed, FfiError> {
        if frame.is_empty() {
            return Err(FfiError::invalid("frame is empty"));
        }
        if frame.len() > MAX_FRAME_LEN {
            return Err(FfiError::PayloadTooLarge {
                msg: format!("frame is {} bytes; the MTU is {MAX_FRAME_LEN}", frame.len()),
            });
        }
        let now_ms = to_u64(now_ms, "nowMs")?;
        self.note_now(now_ms);
        let processed = {
            let mut engine = self.lock_engine()?;
            engine.ingest(&frame, Some(LinkInfo::default()), now_ms)
        };
        self.map_processed(processed)
    }

    /// Advance engine timers (re-announce, retransmits, resource ARQ, store offers, peer aging).
    pub fn tick(&self, now_ms: i64) -> Result<FfiProcessed, FfiError> {
        let now_ms = to_u64(now_ms, "nowMs")?;
        self.note_now(now_ms);
        let processed = {
            let mut engine = self.lock_engine()?;
            engine.tick(now_ms)
        };
        self.map_processed(processed)
    }

    /// Send a direct or channel text message.
    pub fn send_text(
        &self,
        target: FfiTarget,
        text: String,
        now_ms: i64,
    ) -> Result<FfiProcessed, FfiError> {
        if text.trim().is_empty() {
            return Err(FfiError::invalid("message text is empty"));
        }
        let now_ms = to_u64(now_ms, "nowMs")?;
        self.note_now(now_ms);
        let target = parse_target(&target)?;
        let processed = {
            let mut engine = self.lock_engine()?;
            engine
                .send_text(target, &text, now_ms)
                .map_err(map_core_error)?
        };
        self.map_processed(processed)
    }

    /// Share a position fix once.
    #[allow(clippy::too_many_arguments)]
    pub fn send_position(
        &self,
        target: FfiTarget,
        lat: f64,
        lon: f64,
        alt_m: Option<i32>,
        accuracy_m: Option<i32>,
        now_ms: i64,
    ) -> Result<FfiProcessed, FfiError> {
        if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
            return Err(FfiError::invalid("latitude/longitude out of range"));
        }
        let now_ms = to_u64(now_ms, "nowMs")?;
        self.note_now(now_ms);
        let target = parse_target(&target)?;
        let position = Position {
            lat_e7: (lat * 1e7).round() as i32,
            lon_e7: (lon * 1e7).round() as i32,
            alt_m: alt_m.map(|value| value.clamp(i16::MIN as i32, i16::MAX as i32) as i16),
            accuracy_m: accuracy_m.map(|value| value.clamp(0, u8::MAX as i32) as u8),
            speed_cm_s: None,
            heading_deg: None,
            timestamp_ms: now_ms,
        };
        let processed = {
            let mut engine = self.lock_engine()?;
            engine
                .send_position(target, position, now_ms)
                .map_err(map_core_error)?
        };
        self.map_processed(processed)
    }

    /// Send an attachment: creates the announcing message and the resource transfer in one call.
    ///
    /// The caller may send the first returned frames before the full transfer finishes; further
    /// frames arrive from [`OpenMeshNode::tick`]. `AttachmentSent` is emitted when the peer
    /// acknowledges the whole transfer.
    #[allow(clippy::too_many_arguments)]
    pub fn send_attachment(
        &self,
        target: FfiTarget,
        mime: String,
        filename: Option<String>,
        data: Vec<u8>,
        duration_ms: Option<i64>,
        now_ms: i64,
    ) -> Result<FfiProcessed, FfiError> {
        if mime.trim().is_empty() {
            return Err(FfiError::invalid("attachment MIME type is empty"));
        }
        if data.is_empty() {
            return Err(FfiError::invalid("attachment data is empty"));
        }
        let max = self.attachment_limit()?;
        if data.len() > max {
            return Err(FfiError::PayloadTooLarge {
                msg: format!(
                    "attachment is {} bytes; this profile allows at most {max}",
                    data.len()
                ),
            });
        }
        let now_ms = to_u64(now_ms, "nowMs")?;
        self.note_now(now_ms);
        let target = parse_target(&target)?;
        // Allocate through the engine so the persisted counter/epoch (StateJson) prevents id reuse
        // across app restarts and lets in-flight resources keep their ids reserved.
        let msg_id = {
            let mut engine = self.lock_engine()?;
            engine.allocate_message_id()
        };
        let res_id = self.next_res_id();
        let filename = filename.filter(|name| !name.trim().is_empty());
        let duration_ms = duration_ms.map(|value| value.max(0).min(u32::MAX as i64) as u32);
        let digest: [u8; 32] = Sha256::digest(&data).into();
        let chunk_count = (data.len() as u64)
            .div_ceil(ATTACHMENT_CHUNK_BYTES as u64)
            .min(u16::MAX as u64) as u16;
        let attachment = AttachmentRef {
            res_id,
            mime: mime.clone(),
            size: data.len().min(u32::MAX as usize) as u32,
            sha256: digest,
            chunk_bytes: ATTACHMENT_CHUNK_BYTES,
            chunk_count,
            duration_ms,
            name: filename.clone(),
        };
        let message = AppMessage {
            id: msg_id,
            kind: MessageKind::Attachment,
            sent_at_ms: now_ms,
            text: None,
            reply_to: None,
            thread: None,
            attachment: Some(attachment),
            position: None,
            telemetry: None,
            sender: None,
            signature: None,
        };
        let mut processed = {
            let mut engine = self.lock_engine()?;
            engine
                .send_message(target, message, now_ms)
                .map_err(map_core_error)?
        };
        let resource = {
            let mut engine = self.lock_engine()?;
            engine
                .send_resource(
                    target,
                    msg_id,
                    res_id,
                    ResourceKind::Attachment,
                    &mime,
                    &data,
                    now_ms,
                )
                .map_err(map_core_error)?
        };
        processed.merge(resource);
        self.outgoing_resources
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(res_id, msg_id);
        let mut result = self.map_processed(processed)?;
        result.msg_id = Some(i64::from(msg_id));
        Ok(result)
    }

    /// Build and emit a signed announce.
    pub fn announce(&self, now_ms: i64) -> Result<FfiProcessed, FfiError> {
        let now_ms = to_u64(now_ms, "nowMs")?;
        self.note_now(now_ms);
        let processed = {
            let mut engine = self.lock_engine()?;
            engine.announce(now_ms).map_err(map_core_error)?
        };
        self.map_processed(processed)
    }

    /// Retransmit an unacknowledged outgoing message.
    pub fn retry_message(&self, msg_id: i64, now_ms: i64) -> Result<FfiProcessed, FfiError> {
        let msg_id = to_u32(msg_id, "msgId")?;
        let now_ms = to_u64(now_ms, "nowMs")?;
        self.note_now(now_ms);
        let processed = {
            let mut engine = self.lock_engine()?;
            engine
                .retry_message(msg_id, now_ms)
                .map_err(map_core_error)?
        };
        self.map_processed(processed)
    }

    /// Send a read receipt for a received direct message.
    pub fn mark_read(&self, msg_id: i64, now_ms: i64) -> Result<FfiProcessed, FfiError> {
        let msg_id = to_u32(msg_id, "msgId")?;
        let now_ms = to_u64(now_ms, "nowMs")?;
        self.note_now(now_ms);
        let from = {
            let received = self
                .received_from
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            received.get(&msg_id).copied()
        };
        let Some(from) = from else {
            return Err(FfiError::UnknownPeer {
                msg: format!("no received message with id {msg_id}"),
            });
        };
        let ack = Ack {
            kind: AckKind::Read,
            msg_id,
            res_id: None,
            next_expected: None,
            bitmap: None,
            timestamp_ms: now_ms,
        };
        let processed = {
            let mut engine = self.lock_engine()?;
            engine.send_ack(from, ack, now_ms).map_err(map_core_error)?
        };
        self.map_processed(processed)
    }

    /// Add a contact from a base64 72-byte public identity and a display name.
    pub fn add_contact(&self, identity_b64: String, name: String) -> Result<(), FfiError> {
        let bytes = b64_decode(&identity_b64)
            .ok_or_else(|| FfiError::invalid("identity is not valid base64"))?;
        let identity = PublicIdentity::from_bytes(&bytes)
            .map_err(|_| FfiError::invalid("identity is not a valid public identity"))?;
        if !identity.is_self_consistent() {
            return Err(FfiError::invalid("identity keys do not match the node id"));
        }
        let mut engine = self.lock_engine()?;
        engine.add_contact(
            identity,
            (!name.trim().is_empty()).then(|| name.trim().to_string()),
        );
        Ok(())
    }

    /// Remove a contact by hex node id.
    pub fn remove_contact(&self, node_id_hex: String) -> Result<(), FfiError> {
        let node = parse_node_id(&node_id_hex)?;
        let mut engine = self.lock_engine()?;
        engine.remove_contact(node);
        Ok(())
    }

    /// Create a private channel with a fresh random 32-byte key. The PSK is returned once.
    pub fn create_channel(&self, name: String) -> Result<FfiChannel, FfiError> {
        if name.trim().is_empty() {
            return Err(FfiError::invalid("channel name is empty"));
        }
        // `Identity::generate` uses the core's CSPRNG; the seed is discarded immediately.
        let psk = Identity::generate().seed();
        self.join_with_psk(name.trim(), &psk)
    }

    /// Join a channel from a name and base64 32-byte PSK.
    pub fn join_channel(&self, name: String, psk_b64: String) -> Result<FfiChannel, FfiError> {
        if name.trim().is_empty() {
            return Err(FfiError::invalid("channel name is empty"));
        }
        let raw =
            b64_decode(&psk_b64).ok_or_else(|| FfiError::invalid("PSK is not valid base64"))?;
        let psk: [u8; 32] = raw
            .try_into()
            .map_err(|_| FfiError::invalid("PSK must be 32 bytes"))?;
        self.join_with_psk(name.trim(), &psk)
    }

    /// Joined channels with invite URLs (no separate PSK field).
    pub fn channels(&self) -> Vec<FfiChannelInfo> {
        let keys = self
            .channel_keys
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let channels = {
            let engine = match self.engine.lock() {
                Ok(engine) => engine,
                Err(poisoned) => poisoned.into_inner(),
            };
            engine.channels()
        };
        channels
            .into_iter()
            .map(|(id, name)| {
                let invite_url = keys
                    .get(&id)
                    .map(|psk| channel_invite_url(id, &name, psk))
                    .unwrap_or_else(|| {
                        format!(
                            "openmesh://channel?name={}&id={}",
                            url_encode(&name),
                            id.to_hex()
                        )
                    });
                FfiChannelInfo {
                    channel_id_hex: id.to_hex(),
                    name,
                    invite_url,
                }
            })
            .collect()
    }

    /// Known nodes (contacts included); the UI sorts and filters.
    pub fn nodes(&self) -> Vec<FfiNode> {
        let engine = match self.engine.lock() {
            Ok(engine) => engine,
            Err(poisoned) => poisoned.into_inner(),
        };
        engine.known_nodes().into_iter().map(node_to_ffi).collect()
    }

    /// Engine counters for the status screen.
    pub fn stats(&self) -> FfiStats {
        let engine = match self.engine.lock() {
            Ok(engine) => engine,
            Err(poisoned) => poisoned.into_inner(),
        };
        let config = self
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let effective_duty_pct = engine
            .config()
            .effective_duty_pct(region_for(&config))
            .map(i32::from);
        let stats = engine.stats();
        FfiStats {
            tx_frames: stats.tx_frames as i64,
            rx_frames: stats.rx_frames as i64,
            forwarded: stats.forwarded_frames as i64,
            dropped: stats.dropped_frames as i64,
            nodes: stats.known_nodes as i64,
            channels: stats.channels as i64,
            airtime_ms: stats.airtime_tx_ms as i64,
            effective_duty_pct,
        }
    }

    /// Export contacts, channels and counters as opaque JSON bytes for encrypted storage.
    pub fn export_state(&self) -> Result<Vec<u8>, FfiError> {
        let snapshot = {
            let engine = self.lock_engine()?;
            engine.export_state()
        };
        let state = StateJson::from_core(&snapshot);
        serde_json::to_vec(&state)
            .map_err(|e| FfiError::internal(format!("cannot encode state: {e}")))
    }

    /// Restore state produced by [`OpenMeshNode::export_state`].
    pub fn import_state(&self, state: Vec<u8>) -> Result<(), FfiError> {
        let parsed: StateJson = serde_json::from_slice(&state)
            .map_err(|e| FfiError::invalid(format!("state is not valid JSON: {e}")))?;
        let snapshot = parsed.to_core()?;
        let mut engine = self.lock_engine()?;
        engine.import_state(snapshot);
        drop(engine);
        let mut keys = self
            .channel_keys
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        keys.clear();
        for (id, name, psk) in parsed.channels_to_keys()? {
            let _ = name;
            keys.insert(id, psk);
        }
        Ok(())
    }
}

impl OpenMeshNode {
    fn lock_engine(&self) -> Result<MutexGuard<'_, Engine>, FfiError> {
        self.engine
            .lock()
            .map_err(|_| FfiError::internal("engine lock poisoned"))
    }

    fn note_now(&self, now_ms: u64) {
        self.last_now_ms.store(now_ms, Ordering::Relaxed);
    }

    fn current_now(&self) -> u64 {
        self.last_now_ms.load(Ordering::Relaxed)
    }

    fn next_res_id(&self) -> u32 {
        loop {
            let id = self.next_res_id.fetch_add(1, Ordering::Relaxed);
            if id != 0 {
                return id;
            }
        }
    }

    fn attachment_limit(&self) -> Result<usize, FfiError> {
        let config = self
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let profile = openmesh_core::util::PROFILES
            .iter()
            .find(|profile| profile.name == config.profile)
            .unwrap_or(&openmesh_core::util::PROFILES[2]);
        let region = openmesh_core::util::REGIONS
            .iter()
            .find(|region| region.name == config.region)
            .unwrap_or(&openmesh_core::util::REGIONS[0]);
        Ok(openmesh_core::util::max_attachment_bytes(profile, region))
    }

    fn join_with_psk(&self, name: &str, psk: &[u8; 32]) -> Result<FfiChannel, FfiError> {
        let mut engine = self.lock_engine()?;
        let channel_id = engine.add_channel(name, psk);
        drop(engine);
        self.channel_keys
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(channel_id, *psk);
        Ok(FfiChannel {
            channel_id_hex: channel_id.to_hex(),
            name: name.to_string(),
            invite_url: channel_invite_url(channel_id, name, psk),
            psk_b64: b64(psk),
        })
    }

    fn map_processed(&self, processed: Processed) -> Result<FfiProcessed, FfiError> {
        let now = self.current_now();
        let msg_id = processed.outbound.iter().find_map(|outbound| {
            let frame = &outbound.frame;
            (frame.typ == openmesh_core::FrameType::Data && frame.body.len() >= 5).then(|| {
                u32::from_be_bytes([frame.body[1], frame.body[2], frame.body[3], frame.body[4]])
            })
        });
        let mut outbound = Vec::with_capacity(processed.outbound.len());
        for item in processed.outbound {
            let mode = match item.mode {
                SendMode::Flood => FfiSendMode::Flood,
                SendMode::Direct(peer) => FfiSendMode::Direct {
                    peer_hex: peer.to_hex(),
                },
            };
            outbound.push(FfiOutbound {
                frame: item.frame.encode().map_err(map_core_error)?,
                mode,
            });
        }
        let mut events = Vec::with_capacity(processed.events.len());
        for event in processed.events {
            events.push(self.map_event(event, now));
        }
        Ok(FfiProcessed {
            events,
            outbound,
            msg_id: msg_id.map(i64::from),
        })
    }

    fn map_event(&self, event: Event, now_ms: u64) -> FfiEvent {
        match event {
            Event::Message(received) => {
                if let Some(attachment) = received.message.attachment.as_ref() {
                    self.remember_attachment(
                        received.message.id,
                        attachment.name.clone(),
                        attachment.duration_ms.map(u64::from),
                    );
                }
                self.received_from
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(received.message.id, received.from);
                FfiEvent::Message {
                    from: received.from.to_hex(),
                    msg_id: i64::from(received.message.id),
                    text: received.message.text,
                    received_at_ms: now_ms as i64,
                    channel: None,
                }
            }
            Event::GroupMessage(received) => {
                if let Some(attachment) = received.message.attachment.as_ref() {
                    self.remember_attachment(
                        received.message.id,
                        attachment.name.clone(),
                        attachment.duration_ms.map(u64::from),
                    );
                }
                FfiEvent::GroupMessage {
                    channel: received.channel.to_hex(),
                    from: received.sender.to_hex(),
                    msg_id: i64::from(received.message.id),
                    text: received.message.text,
                }
            }
            Event::Ack { ack, .. } => FfiEvent::Ack {
                msg_id: i64::from(ack.msg_id),
                kind: ack_kind_str(ack.kind).to_string(),
            },
            Event::Position {
                from,
                channel,
                position,
            } => FfiEvent::Position {
                from: from.to_hex(),
                channel: channel.map(|id| id.to_hex()),
                lat: position.lat_e7 as f64 / 1e7,
                lon: position.lon_e7 as f64 / 1e7,
                alt_m: position.alt_m.map(i32::from),
                accuracy_m: position.accuracy_m.map(i32::from),
                speed_cm_s: position.speed_cm_s.map(i32::from),
                heading_deg: position.heading_deg.map(i32::from),
                timestamp_ms: position.timestamp_ms as i64,
            },
            Event::Telemetry { from, telemetry } => {
                let node = {
                    let engine = match self.engine.lock() {
                        Ok(engine) => engine,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    engine.node_info(from)
                };
                let mut node = node
                    .map(node_to_ffi)
                    .unwrap_or_else(|| minimal_node(from, now_ms));
                if node.battery_pct.is_none() {
                    node.battery_pct = telemetry.battery_pct.map(i32::from);
                }
                FfiEvent::Telemetry { node }
            }
            Event::NodeSeen(info) => FfiEvent::NodeSeen {
                node: node_to_ffi(info),
            },
            Event::StoreOffer { from, ids } => FfiEvent::StoreOffer {
                relay: from.to_hex(),
                count: ids.len().min(i64::MAX as usize) as i64,
            },
            Event::StoreAccepted { .. } => FfiEvent::Error {
                message: "store accepted (reserved)".to_string(),
            },
            Event::ResourceProgress {
                peer,
                res_id,
                received,
                total,
            } => {
                let msg_id = self
                    .outgoing_resources
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .get(&res_id)
                    .copied()
                    .unwrap_or(0);
                FfiEvent::ResourceProgress {
                    peer: peer.to_hex(),
                    msg_id: i64::from(msg_id),
                    res_id: i64::from(res_id),
                    received: i64::from(received),
                    total: i64::from(total),
                }
            }
            Event::ResourceReceived {
                peer,
                channel,
                manifest,
                data,
            } => {
                let (filename, duration_ms) = self
                    .incoming_attachments
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(&manifest.msg_id)
                    .unwrap_or((None, None));
                FfiEvent::ResourceReceived {
                    peer: peer.to_hex(),
                    channel: channel.map(|id| id.to_hex()),
                    msg_id: i64::from(manifest.msg_id),
                    mime: manifest.mime,
                    filename,
                    data,
                    duration_ms: duration_ms.map(|value| value as i64),
                }
            }
            Event::ResourceSent { peer, res_id } => {
                let msg_id = self
                    .outgoing_resources
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(&res_id)
                    .unwrap_or(0);
                FfiEvent::AttachmentSent {
                    peer: peer.to_hex(),
                    msg_id: i64::from(msg_id),
                }
            }
            Event::Error(message) => FfiEvent::Error { message },
        }
    }

    fn remember_attachment(&self, msg_id: u32, filename: Option<String>, duration_ms: Option<u64>) {
        self.incoming_attachments
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(msg_id, (filename, duration_ms));
    }
}

// ---------------------------------------------------------------------------------------------
// Configuration helpers
// ---------------------------------------------------------------------------------------------

fn validate_config(config: &FfiConfig) -> Result<(), FfiError> {
    if config.name.trim().is_empty() {
        return Err(FfiError::invalid("node name is empty"));
    }
    if config.name.len() > 32 {
        return Err(FfiError::invalid("node name is longer than 32 bytes"));
    }
    if parse_node_class(&config.node_class).is_none() {
        return Err(FfiError::invalid(format!(
            "unknown node class '{}' (use client, repeater, gateway or tracker)",
            config.node_class
        )));
    }
    if !openmesh_core::util::PROFILES
        .iter()
        .any(|profile| profile.name == config.profile)
    {
        return Err(FfiError::invalid(format!(
            "unknown radio profile '{}'",
            config.profile
        )));
    }
    if !openmesh_core::util::REGIONS
        .iter()
        .any(|region| region.name == config.region)
    {
        return Err(FfiError::invalid(format!(
            "unknown region '{}'",
            config.region
        )));
    }
    if config.announce_interval_s < 300 {
        return Err(FfiError::invalid(
            "announceIntervalS must be at least 300 seconds",
        ));
    }
    if config.announce_interval_s > 24 * 3600 {
        return Err(FfiError::invalid(
            "announceIntervalS must be at most 24 hours",
        ));
    }
    Ok(())
}

fn parse_node_class(value: &str) -> Option<NodeClass> {
    Some(match value.trim().to_ascii_lowercase().as_str() {
        "client" => NodeClass::Client,
        "repeater" => NodeClass::Repeater,
        "gateway" => NodeClass::Gateway,
        "room_server" | "roomserver" => NodeClass::RoomServer,
        "tracker" => NodeClass::Tracker,
        _ => return None,
    })
}

fn engine_config_for(config: &FfiConfig) -> Result<EngineConfig, FfiError> {
    let class = parse_node_class(&config.node_class)
        .ok_or_else(|| FfiError::invalid("unknown node class"))?;
    let mut engine_config = match class {
        NodeClass::Client | NodeClass::Tracker => EngineConfig::client(config.name.trim()),
        NodeClass::Repeater | NodeClass::Gateway | NodeClass::RoomServer => {
            EngineConfig::repeater(config.name.trim())
        }
    };
    engine_config.class = class;
    engine_config.accept_channels = config.accept_channels;
    engine_config.announce_interval_ms = (config.announce_interval_s as u64) * 1000;
    if config.store_forward {
        engine_config.caps |= openmesh_core::services::caps::STORE;
        engine_config.role = Role::Relay(RelayConfig {
            store_capacity: RELAY_STORE_CAPACITY,
            store_ttl_ms: 72 * 3_600_000,
            max_frames_per_min: 240,
        });
    } else {
        engine_config.caps &= !openmesh_core::services::caps::STORE;
        engine_config.role = Role::Client;
    }
    Ok(engine_config)
}

fn region_for(config: &FfiConfig) -> &'static util::Region {
    util::region(&config.region).unwrap_or(&util::REGIONS[0])
}

/// Merge a camelCase (or snake_case) `regulatoryOverride` JSON object onto the current override.
fn parse_override_patch(
    value: &serde_json::Value,
    current: &RegulatoryOverride,
) -> Result<RegulatoryOverride, FfiError> {
    let object = value
        .as_object()
        .ok_or_else(|| FfiError::invalid("regulatoryOverride must be an object"))?;
    let mut merged = current.clone();
    for (key, value) in object {
        match key.as_str() {
            "enabled" => {
                merged.enabled = value.as_bool().ok_or_else(|| {
                    FfiError::invalid("regulatoryOverride.enabled must be a boolean")
                })?;
            }
            "acknowledged" | "confirm" => {
                merged.acknowledged = value.as_bool().ok_or_else(|| {
                    FfiError::invalid(format!("regulatoryOverride.{key} must be a boolean"))
                })?;
            }
            "dutyCyclePct" | "duty_cycle_pct" => {
                merged.duty_cycle_pct = match value {
                    serde_json::Value::Null => None,
                    other => Some(other.as_u64().filter(|duty| *duty <= 100).ok_or_else(|| {
                        FfiError::invalid("regulatoryOverride.dutyCyclePct must be 0..=100")
                    })? as u8),
                };
            }
            "maxPowerDbm" | "max_power_dbm" => {
                merged.max_power_dbm = match value {
                    serde_json::Value::Null => None,
                    other => Some(
                        other
                            .as_i64()
                            .and_then(|power| i8::try_from(power).ok())
                            .ok_or_else(|| {
                                FfiError::invalid(
                                    "regulatoryOverride.maxPowerDbm must be an integer dBm value",
                                )
                            })?,
                    ),
                };
            }
            "frequencyKhz" | "frequency_khz" => {
                merged.frequency_khz = match value {
                    serde_json::Value::Null => None,
                    other => Some(
                        other
                            .as_u64()
                            .and_then(|frequency| u32::try_from(frequency).ok())
                            .ok_or_else(|| {
                                FfiError::invalid(
                                    "regulatoryOverride.frequencyKhz must be a positive integer",
                                )
                            })?,
                    ),
                };
            }
            "reason" => {
                merged.reason = match value {
                    serde_json::Value::Null => None,
                    other => Some(
                        other
                            .as_str()
                            .ok_or_else(|| {
                                FfiError::invalid("regulatoryOverride.reason must be a string")
                            })?
                            .to_string(),
                    ),
                };
            }
            other => {
                return Err(FfiError::invalid(format!(
                    "unknown regulatoryOverride field '{other}'"
                )));
            }
        }
    }
    Ok(merged)
}

fn override_to_json(override_config: &RegulatoryOverride) -> serde_json::Value {
    serde_json::json!({
        "enabled": override_config.enabled,
        "acknowledged": override_config.acknowledged,
        "dutyCyclePct": override_config.duty_cycle_pct,
        "maxPowerDbm": override_config.max_power_dbm,
        "frequencyKhz": override_config.frequency_khz,
        "reason": override_config.reason,
    })
}

fn config_to_json(config: &FfiConfig) -> serde_json::Value {
    serde_json::json!({
        "name": config.name,
        "nodeClass": config.node_class,
        "profile": config.profile,
        "region": config.region,
        "storeForward": config.store_forward,
        "acceptChannels": config.accept_channels,
        "announceIntervalS": config.announce_interval_s,
    })
}

fn string_value(key: &str, value: &serde_json::Value) -> Result<String, FfiError> {
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| FfiError::invalid(format!("{key} must be a string")))
}

fn bool_value(key: &str, value: &serde_json::Value) -> Result<bool, FfiError> {
    value
        .as_bool()
        .ok_or_else(|| FfiError::invalid(format!("{key} must be a boolean")))
}

fn parse_target(target: &FfiTarget) -> Result<Target, FfiError> {
    match target {
        FfiTarget::Direct { node_id_hex } => Ok(Target::Direct(parse_node_id(node_id_hex)?)),
        FfiTarget::Channel { channel_id_hex } => {
            Ok(Target::Channel(parse_node_id(channel_id_hex)?))
        }
    }
}

fn parse_node_id(value: &str) -> Result<NodeId, FfiError> {
    NodeId::from_hex(value.trim()).map_err(|_| {
        FfiError::invalid(format!("invalid node id '{value}' (expected 16 hex chars)"))
    })
}

fn channel_invite_url(channel_id: NodeId, name: &str, psk: &[u8; 32]) -> String {
    format!(
        "openmesh://channel?name={}&id={}&psk={}",
        url_encode(name),
        channel_id.to_hex(),
        b64_url(psk)
    )
}

fn map_core_error(error: Error) -> FfiError {
    match error {
        Error::UnknownNode(id) => FfiError::UnknownPeer {
            msg: format!("unknown node {}", id.to_hex()),
        },
        Error::PayloadTooLarge { size, max } => FfiError::PayloadTooLarge {
            msg: format!("payload is {size} bytes; the limit is {max}"),
        },
        Error::Config(message) | Error::Malformed(message) => {
            FfiError::invalid(message.to_string())
        }
        Error::BufferTooSmall { need, have } => {
            FfiError::invalid(format!("buffer too small: need {need} bytes, have {have}"))
        }
        Error::UnsupportedVersion(version) => {
            FfiError::invalid(format!("unsupported protocol version {version}"))
        }
        other => FfiError::internal(other.to_string()),
    }
}

fn ack_kind_str(kind: AckKind) -> &'static str {
    match kind {
        AckKind::Delivered => "delivered",
        AckKind::Read => "read",
        AckKind::ResourceWindow => "resource_window",
        AckKind::ResourceComplete => "resource_complete",
    }
}

fn node_to_ffi(info: NodeInfo) -> FfiNode {
    let (lat, lon) = info
        .last_position
        .as_ref()
        .map(|position| (position.lat_e7 as f64 / 1e7, position.lon_e7 as f64 / 1e7))
        .unzip();
    FfiNode {
        node_id_hex: info.node_id.to_hex(),
        name: info.name,
        class: info.class.as_str().to_string(),
        caps: caps_to_strings(info.caps),
        hops: i32::from(info.hops),
        last_seen_ms: info.last_seen_ms as i64,
        rssi: info.rssi.map(i32::from),
        snr: info.snr.map(f64::from),
        battery_pct: info.battery_pct.map(i32::from),
        lat,
        lon,
    }
}

fn minimal_node(node_id: NodeId, now_ms: u64) -> FfiNode {
    FfiNode {
        node_id_hex: node_id.to_hex(),
        name: String::new(),
        class: "client".to_string(),
        caps: Vec::new(),
        hops: 0,
        last_seen_ms: now_ms as i64,
        rssi: None,
        snr: None,
        battery_pct: None,
        lat: None,
        lon: None,
    }
}

fn caps_to_strings(caps: u16) -> Vec<String> {
    use openmesh_core::services::caps;
    let table = [
        (caps::RELAY, "relay"),
        (caps::STORE, "store"),
        (caps::GPS, "gps"),
        (caps::WIFI, "wifi"),
        (caps::ETHERNET, "ethernet"),
        (caps::BLE, "ble"),
        (caps::USB, "usb"),
        (caps::SCREEN, "screen"),
        (caps::AUDIO_OUT, "audio_out"),
        (caps::SOLAR, "solar"),
    ];
    table
        .iter()
        .filter(|(bit, _)| caps & bit != 0)
        .map(|(_, name)| name.to_string())
        .collect()
}

// ---------------------------------------------------------------------------------------------
// State export/import (opaque JSON; the platform encrypts it)
// ---------------------------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize)]
struct StateJson {
    version: u32,
    contacts: Vec<StateContact>,
    channels: Vec<StateChannel>,
    last_seq: u32,
    /// Monotonic message-id allocation counter (added in state version 2; defaults to 0 = legacy).
    #[serde(default)]
    msg_counter: u32,
    /// Message-id derivation epoch (added in state version 2; defaults to 0 = legacy).
    #[serde(default)]
    msg_id_epoch: u32,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StateContact {
    identity_b64: String,
    name: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StateChannel {
    channel_id_hex: String,
    name: String,
    psk_b64: String,
}

impl StateJson {
    fn from_core(snapshot: &StateSnapshot) -> Self {
        Self {
            version: 2,
            contacts: snapshot
                .contacts
                .iter()
                .map(|(identity, name)| StateContact {
                    identity_b64: b64(&identity.to_bytes()),
                    name: name.clone(),
                })
                .collect(),
            channels: snapshot
                .channels
                .iter()
                .map(|(channel_id, name, psk)| StateChannel {
                    channel_id_hex: channel_id.to_hex(),
                    name: name.clone(),
                    psk_b64: b64(psk),
                })
                .collect(),
            last_seq: snapshot.last_seq,
            msg_counter: snapshot.msg_counter,
            msg_id_epoch: snapshot.msg_id_epoch,
        }
    }

    fn to_core(&self) -> Result<StateSnapshot, FfiError> {
        let mut contacts = Vec::with_capacity(self.contacts.len());
        for contact in &self.contacts {
            let bytes = b64_decode(&contact.identity_b64)
                .ok_or_else(|| FfiError::invalid("state contact identity is not base64"))?;
            let identity = PublicIdentity::from_bytes(&bytes)
                .map_err(|_| FfiError::invalid("state contact identity is invalid"))?;
            contacts.push((identity, contact.name.clone()));
        }
        let mut channels = Vec::with_capacity(self.channels.len());
        for channel in &self.channels {
            let channel_id = parse_node_id(&channel.channel_id_hex)?;
            let raw = b64_decode(&channel.psk_b64)
                .ok_or_else(|| FfiError::invalid("state channel PSK is not base64"))?;
            let psk: [u8; 32] = raw
                .try_into()
                .map_err(|_| FfiError::invalid("state channel PSK must be 32 bytes"))?;
            channels.push((channel_id, channel.name.clone(), psk));
        }
        Ok(StateSnapshot {
            contacts,
            channels,
            last_seq: self.last_seq,
            msg_counter: self.msg_counter,
            msg_id_epoch: self.msg_id_epoch,
        })
    }

    fn channels_to_keys(&self) -> Result<Vec<(NodeId, String, [u8; 32])>, FfiError> {
        let mut out = Vec::with_capacity(self.channels.len());
        for channel in &self.channels {
            let channel_id = parse_node_id(&channel.channel_id_hex)?;
            let raw = b64_decode(&channel.psk_b64)
                .ok_or_else(|| FfiError::invalid("state channel PSK is not base64"))?;
            let psk: [u8; 32] = raw
                .try_into()
                .map_err(|_| FfiError::invalid("state channel PSK must be 32 bytes"))?;
            out.push((channel_id, channel.name.clone(), psk));
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------------------------
// Voice (Codec2 tiny/medium) — additive free functions (media-tiers.md §4)
// ---------------------------------------------------------------------------------------------

/// Shared media tier definitions (`content/presets/media.json`, single source for web/Android).
const MEDIA_TIERS_JSON: &str = include_str!("../../../content/presets/media.json");

fn parse_voice_tier(tier: &str) -> Result<VoiceTier, FfiError> {
    match tier.trim().to_ascii_lowercase().as_str() {
        "tiny" => Ok(VoiceTier::Tiny),
        "medium" => Ok(VoiceTier::Medium),
        "high" => Ok(VoiceTier::High),
        other => Err(FfiError::invalid(format!(
            "unknown voice tier '{other}' (expected tiny, medium or high)"
        ))),
    }
}

fn map_voice_error(error: VoiceError) -> FfiError {
    FfiError::invalid(error.to_string())
}

/// Encode 8 kHz mono s16le PCM into raw Codec2 frames for `tiny` or `medium`.
///
/// `high` is rejected (clients use the platform Opus/AAC recorder). Empty/short input and a
/// trailing partial sample/frame are handled by the codec crate (whole-frame truncation).
#[uniffi::export]
pub fn openmesh_voice_encode(tier: String, pcm_s16le: Vec<u8>) -> Result<Vec<u8>, FfiError> {
    let tier = parse_voice_tier(&tier)?;
    openmesh_voice::encode(tier, &pcm_s16le).map_err(map_voice_error)
}

/// Decode a raw Codec2 frame stream back to 8 kHz mono s16le PCM (`tiny`/`medium`).
///
/// A trailing partial payload frame is ignored; `high` is rejected.
#[uniffi::export]
pub fn openmesh_voice_decode(tier: String, payload: Vec<u8>) -> Result<Vec<u8>, FfiError> {
    let tier = parse_voice_tier(&tier)?;
    openmesh_voice::decode(tier, &payload).map_err(map_voice_error)
}

/// The `voice` block of `content/presets/media.json` as JSON (tiny/medium/high formats).
#[uniffi::export]
pub fn openmesh_voice_tiers_json() -> String {
    static VOICE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VOICE
        .get_or_init(|| {
            serde_json::from_str::<serde_json::Value>(MEDIA_TIERS_JSON)
                .ok()
                .and_then(|media| media.get("voice").cloned())
                .map(|voice| voice.to_string())
                .unwrap_or_else(|| "{}".to_string())
        })
        .clone()
}

/// Wall-clock milliseconds between resource chunk batches for a radio profile and region.
///
/// Duty-aware pacing hint: `batch * airtime(240 B frame) / (duty_pct / 100)`, clamped to
/// 250 ms..=120 s. Apply the result with [`OpenMeshNode::set_resource_pace_interval_ms`] at
/// startup and after config changes. Unknown profile/region names return an error.
#[uniffi::export]
pub fn openmesh_resource_pace_interval_ms(
    profile: String,
    region: String,
    batch: u16,
) -> Result<u64, FfiError> {
    let profile_name = profile.trim();
    let profile = util::PROFILES
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(profile_name))
        .ok_or_else(|| FfiError::invalid(format!("unknown radio profile: {profile_name}")))?;
    let region_name = region.trim();
    let region = util::region(region_name)
        .ok_or_else(|| FfiError::invalid(format!("unknown region: {region_name}")))?;
    Ok(util::resource_pace_interval_ms_for_region(
        profile,
        region,
        batch.max(1),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, seed_byte: u8) -> Arc<OpenMeshNode> {
        let config = FfiConfig {
            name: name.to_string(),
            ..FfiConfig::default()
        };
        OpenMeshNode::new(vec![seed_byte; SEED_LEN], config).expect("node")
    }

    fn deliver(
        from: &OpenMeshNode,
        to: &OpenMeshNode,
        processed: &FfiProcessed,
        now: i64,
        events: &mut Vec<FfiEvent>,
    ) {
        for outbound in &processed.outbound {
            let frame = match &outbound.mode {
                FfiSendMode::Flood => outbound.frame.clone(),
                FfiSendMode::Direct { .. } => outbound.frame.clone(),
            };
            let delivered = to.ingest_frame(frame, now).expect("ingest");
            events.extend(delivered.events);
            for reply in delivered.outbound {
                let reply_events = from.ingest_frame(reply.frame, now).expect("ingest");
                events.extend(reply_events.events);
            }
        }
    }

    /// Pump the two nodes until `predicate` matches or the frame budget is exhausted.
    fn pump(
        a: &OpenMeshNode,
        b: &OpenMeshNode,
        mut now: i64,
        mut predicate: impl FnMut(&[FfiEvent]) -> bool,
    ) -> Vec<FfiEvent> {
        let mut all = Vec::new();
        for step in 0..64 {
            now += 250;
            let pa = a.tick(now).expect("tick a");
            let pb = b.tick(now).expect("tick b");
            let mut events = Vec::new();
            deliver(a, b, &pa, now, &mut events);
            deliver(b, a, &pb, now, &mut events);
            all.extend(events);
            if predicate(&all) {
                break;
            }
            if step == 63 {
                break;
            }
        }
        all
    }

    #[test]
    fn resource_pace_interval_helper_matches_duty_budget() {
        // MEDIUM_FAST 240 B = 611 ms airtime; batch 2 at EU868 10 % duty = 12 220 ms.
        assert_eq!(
            openmesh_resource_pace_interval_ms("MEDIUM_FAST".into(), "EU868".into(), 2)
                .expect("pace"),
            12_220
        );
        assert!(openmesh_resource_pace_interval_ms("NOPE".into(), "EU868".into(), 2).is_err());
        assert!(
            openmesh_resource_pace_interval_ms("MEDIUM_FAST".into(), "NOPE".into(), 2).is_err()
        );
        let node = node("pacer", 7);
        node.set_resource_pace_interval_ms(12_220)
            .expect("apply pace");
        node.set_resource_pace_interval_ms(0)
            .expect("clamped by engine");
    }

    #[test]
    fn legacy_state_json_without_msg_fields_still_imports() {
        let node = node("legacy-state", 12);
        let exported = node.export_state().expect("export");
        let mut value: serde_json::Value = serde_json::from_slice(&exported).expect("state JSON");
        let object = value.as_object_mut().expect("state object");
        object.remove("msg_counter");
        object.remove("msg_id_epoch");
        object.insert("version".into(), serde_json::Value::from(1));
        let legacy = serde_json::to_vec(&value).expect("legacy bytes");
        node.import_state(legacy).expect("legacy import");

        // Re-export: the imported legacy state must have advanced to the new epoch/counter floor.
        let reexported = node.export_state().expect("re-export");
        let value: serde_json::Value = serde_json::from_slice(&reexported).expect("JSON");
        assert_eq!(value["version"], 2);
        assert_eq!(value["msg_id_epoch"], 1);
        assert!(value["msg_counter"].as_u64().expect("counter") >= 0x8000_0000);
    }

    #[test]
    fn restored_state_never_reuses_attachment_message_ids() {
        let first_node = node("media", 9);
        let channel = first_node
            .join_channel("media-test".into(), b64(&[7u8; 32]))
            .expect("channel");
        let target = FfiTarget::Channel {
            channel_id_hex: channel.channel_id_hex.clone(),
        };
        let first = first_node
            .send_attachment(
                target.clone(),
                "audio/c2-1300".into(),
                None,
                vec![1u8; 308],
                Some(2_000),
                1_000,
            )
            .expect("first attachment");
        let first_id = first.msg_id.expect("msg id");
        // A stalled transfer must not release its id: a second media message gets a fresh one.
        let stalled = first_node
            .send_attachment(
                target.clone(),
                "image/webp".into(),
                None,
                vec![2u8; 400],
                None,
                1_500,
            )
            .expect("second attachment");
        assert_ne!(first_id, stalled.msg_id.expect("msg id"));

        // Export/import simulates an app restart; the persisted counter/epoch must prevent reuse.
        let state = first_node.export_state().expect("export state");
        let text = String::from_utf8_lossy(&state);
        assert!(text.contains("msg_counter"));
        assert!(text.contains("msg_id_epoch"));
        let second_node = node("media", 9);
        second_node.import_state(state).expect("import state");
        let after_restart = second_node
            .send_attachment(
                target,
                "audio/c2-1300".into(),
                None,
                vec![3u8; 308],
                Some(2_000),
                2_000,
            )
            .expect("post-restart attachment");
        assert_ne!(first_id, after_restart.msg_id.expect("msg id"));
    }

    #[test]
    fn config_defaults_and_validation() {
        let config = FfiConfig::default();
        assert_eq!(config.node_class, "client");
        assert!(validate_config(&config).is_ok());
        assert!(
            validate_config(&FfiConfig {
                profile: "NOPE".into(),
                ..config.clone()
            })
            .is_err()
        );
        assert!(
            validate_config(&FfiConfig {
                region: "MARS".into(),
                ..config.clone()
            })
            .is_err()
        );
        assert!(
            validate_config(&FfiConfig {
                node_class: "wizard".into(),
                ..config.clone()
            })
            .is_err()
        );
        assert!(
            validate_config(&FfiConfig {
                announce_interval_s: 10,
                ..config
            })
            .is_err()
        );
    }

    #[test]
    fn identity_is_public_and_deterministic() {
        let a = node("a", 7);
        let b = node("a", 7);
        assert_eq!(a.node_id(), b.node_id());
        assert_eq!(a.node_id().len(), 16);
        let identity = b64_decode(&a.public_identity_b64()).expect("base64 identity");
        assert_eq!(identity.len(), 72);
        assert!(a.config_json().contains("\"nodeClass\":\"client\""));
    }

    #[test]
    fn set_config_applies_live_and_rejects_unknown_fields() {
        let a = node("a", 11);
        a.set_config(
            r#"{"name":"renamed","profile":"LONG_FAST","region":"US915","storeForward":true}"#
                .to_string(),
        )
        .expect("patch");
        assert!(a.config_json().contains("\"name\":\"renamed\""));
        assert!(a.config_json().contains("\"storeForward\":true"));
        assert!(a.set_config(r#"{"bogus":1}"#.to_string()).is_err());
    }

    #[test]
    fn two_nodes_exchange_direct_message_and_ack() {
        let a = node("alice", 1);
        let b = node("bob", 2);
        let bob = b.node_id();
        // Announces first so each peer identity is known.
        let now = 1_000;
        let pa = a.announce(now).expect("announce a");
        let pb = b.announce(now).expect("announce b");
        deliver(&a, &b, &pa, now, &mut Vec::new());
        deliver(&b, &a, &pb, now, &mut Vec::new());

        let sent = a
            .send_text(
                FfiTarget::Direct {
                    node_id_hex: bob.clone(),
                },
                "hello bob".into(),
                2_000,
            )
            .expect("send");
        assert!(sent.msg_id.is_some());
        let msg_id = sent.msg_id.unwrap();
        let mut events = Vec::new();
        deliver(&a, &b, &sent, 2_000, &mut events);
        events.extend(pump(&a, &b, 2_000, |events| {
            events
                .iter()
                .any(|event| matches!(event, FfiEvent::Ack { kind, .. } if kind == "delivered"))
        }));
        assert!(
            events.iter().any(|event| matches!(event, FfiEvent::Message { msg_id: id, text, .. } if *id == msg_id && text.as_deref() == Some("hello bob"))),
            "message not delivered: {events:?}"
        );
        assert!(
            events.iter().any(|event| matches!(event, FfiEvent::Ack { msg_id: id, kind } if *id == msg_id && kind == "delivered")),
            "ack not delivered: {events:?}"
        );
    }

    #[test]
    fn channel_create_join_and_group_message() {
        let a = node("alice", 3);
        let b = node("bob", 4);
        let channel = a.create_channel("family".into()).expect("create");
        assert_eq!(channel.channel_id_hex.len(), 16);
        assert!(channel.invite_url.starts_with("openmesh://channel?"));
        assert_eq!(
            b.join_channel(channel.name.clone(), channel.psk_b64.clone())
                .expect("join")
                .channel_id_hex,
            channel.channel_id_hex
        );
        let now = 500;
        let pa = a.announce(now).expect("announce a");
        let pb = b.announce(now).expect("announce b");
        deliver(&a, &b, &pa, now, &mut Vec::new());
        deliver(&b, &a, &pb, now, &mut Vec::new());
        let sent = a
            .send_text(
                FfiTarget::Channel {
                    channel_id_hex: channel.channel_id_hex.clone(),
                },
                "hi family".into(),
                1_000,
            )
            .expect("send channel");
        let mut events = Vec::new();
        deliver(&a, &b, &sent, 1_000, &mut events);
        events.extend(pump(&a, &b, 1_000, |_| false));
        assert!(
            events.iter().any(|event| matches!(event, FfiEvent::GroupMessage { channel: group_channel, text, .. } if group_channel == &channel.channel_id_hex && text.as_deref() == Some("hi family"))),
            "group message not delivered: {events:?}"
        );
        assert_eq!(b.channels().len(), 1);
        assert!(!b.channels()[0].invite_url.is_empty());
        assert!(!b.config_json().contains(&channel.psk_b64));
    }

    #[test]
    fn attachment_round_trip_delivers_resource_received() {
        let a = node("alice", 5);
        let b = node("bob", 6);
        let bob = b.node_id();
        let now = 1_000;
        let pa = a.announce(now).expect("announce a");
        let pb = b.announce(now).expect("announce b");
        deliver(&a, &b, &pa, now, &mut Vec::new());
        deliver(&b, &a, &pb, now, &mut Vec::new());

        let payload = b"a tiny image payload".to_vec();
        let sent = a
            .send_attachment(
                FfiTarget::Direct {
                    node_id_hex: bob.clone(),
                },
                "image/jpeg".into(),
                Some("photo.jpg".into()),
                payload.clone(),
                None,
                2_000,
            )
            .expect("send attachment");
        assert!(sent.msg_id.is_some());
        let mut events = Vec::new();
        deliver(&a, &b, &sent, 2_000, &mut events);
        events.extend(pump(&a, &b, 2_000, |events| {
            events
                .iter()
                .any(|event| matches!(event, FfiEvent::ResourceReceived { .. }))
        }));
        let received = events
            .iter()
            .find_map(|event| match event {
                FfiEvent::ResourceReceived {
                    mime,
                    filename,
                    data,
                    channel,
                    ..
                } => Some((
                    mime.clone(),
                    filename.clone(),
                    data.clone(),
                    channel.clone(),
                )),
                _ => None,
            })
            .expect("resource received");
        assert_eq!(received.0, "image/jpeg");
        assert_eq!(received.1.as_deref(), Some("photo.jpg"));
        assert_eq!(received.2, payload);
        assert!(received.3.is_none());
        assert!(
            events.iter().any(
                |event| matches!(event, FfiEvent::AttachmentSent { peer, .. } if peer == &bob)
            )
        );
    }

    #[test]
    fn state_export_import_round_trips_contacts_and_channels() {
        let a = node("alice", 8);
        let b = node("bob", 9);
        a.add_contact(b.public_identity_b64(), "Bob".into())
            .expect("contact");
        let channel = a.create_channel("family".into()).expect("create");
        let exported = a.export_state().expect("export");
        assert!(!exported.is_empty());
        // The exported blob contains channel keys, so the app must encrypt it.
        assert!(String::from_utf8_lossy(&exported).contains("psk_b64"));

        let restored = node("alice", 8);
        restored.import_state(exported.clone()).expect("import");
        assert_eq!(restored.node_id(), a.node_id());
        let round_trip = restored.export_state().expect("re-export");
        let round_trip_text = String::from_utf8_lossy(&round_trip);
        assert!(round_trip_text.contains(&b.public_identity_b64()));
        assert!(round_trip_text.contains("Bob"));
        let channels = restored.channels();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].channel_id_hex, channel.channel_id_hex);
        assert!(channels[0].invite_url.contains("psk="));
    }

    fn config_value(node: &OpenMeshNode) -> serde_json::Value {
        serde_json::from_str(&node.config_json()).expect("config_json is JSON")
    }

    #[test]
    fn regulatory_override_defaults_disabled_and_echoes_state() {
        let a = node("a", 30);
        let config = config_value(&a);
        assert_eq!(config["regulatoryOverride"]["enabled"], false);
        assert_eq!(config["regulatoryOverride"]["acknowledged"], false);
        assert!(config["regulatoryOverride"]["dutyCyclePct"].is_null());
        assert_eq!(config["overrideActive"], false);
        // Disabled override = legal region duty for EU868.
        assert_eq!(config["effectiveDutyPct"], 10);
        assert_eq!(a.stats().effective_duty_pct, Some(10));
    }

    #[test]
    fn regulatory_override_requires_ack_only_when_exceeding() {
        let a = node("a", 31);
        let error = a
            .set_config(r#"{"regulatoryOverride":{"enabled":true,"dutyCyclePct":100}}"#.to_string())
            .unwrap_err();
        let FfiError::InvalidArgument { msg } = error else {
            panic!("expected InvalidArgument, got {error:?}");
        };
        assert!(msg.contains("acknowledg"), "message: {msg}");
        // Nothing changed.
        assert_eq!(config_value(&a)["overrideActive"], false);

        // Acknowledged exceeding override is accepted and echoed in camelCase.
        a.set_config(
            r#"{"regulatoryOverride":{"enabled":true,"acknowledged":true,"dutyCyclePct":100,"maxPowerDbm":22,"reason":"emergency"}}"#
                .to_string(),
        )
        .expect("acknowledged override");
        let config = config_value(&a);
        assert_eq!(config["regulatoryOverride"]["enabled"], true);
        assert_eq!(config["regulatoryOverride"]["acknowledged"], true);
        assert_eq!(config["regulatoryOverride"]["dutyCyclePct"], 100);
        assert_eq!(config["regulatoryOverride"]["maxPowerDbm"], 22);
        assert_eq!(config["overrideActive"], true);
        assert_eq!(config["effectiveDutyPct"], 100);
        assert_eq!(a.stats().effective_duty_pct, Some(100));

        // Safe reductions need no acknowledgement (contract §2.1 rule 3).
        let b = node("b", 32);
        b.set_config(
            r#"{"regulatoryOverride":{"enabled":true,"dutyCyclePct":5,"maxPowerDbm":15}}"#
                .to_string(),
        )
        .expect("safe reduction");
        let config = config_value(&b);
        assert_eq!(config["regulatoryOverride"]["acknowledged"], false);
        assert_eq!(config["regulatoryOverride"]["dutyCyclePct"], 5);
        assert_eq!(config["overrideActive"], true);
        assert_eq!(config["effectiveDutyPct"], 5);

        // 0 % means "no transmissions", valid without acknowledgement.
        let c = node("c", 33);
        c.set_config(r#"{"regulatoryOverride":{"enabled":true,"dutyCyclePct":0}}"#.to_string())
            .expect("zero TX override");
        assert_eq!(config_value(&c)["effectiveDutyPct"], 0);
    }

    #[test]
    fn regulatory_override_snake_case_reason_escaping_and_bounds() {
        let a = node("a", 34);
        let reason = "quote \" and backslash \\ and newline\n";
        let patch = serde_json::json!({
            "regulatory_override": {
                "enabled": true,
                "acknowledged": true,
                "duty_cycle_pct": 20,
                "reason": reason,
            }
        });
        a.set_config(patch.to_string())
            .expect("snake_case override with escaped reason");
        let config = config_value(&a);
        assert_eq!(config["regulatoryOverride"]["dutyCyclePct"], 20);
        assert_eq!(config["regulatoryOverride"]["reason"], reason);

        let error = a
            .set_config(
                serde_json::json!({"regulatoryOverride": {"frequencyKhz": 100_000}}).to_string(),
            )
            .unwrap_err();
        assert!(matches!(error, FfiError::InvalidArgument { .. }));

        let error = a
            .set_config(
                serde_json::json!({"regulatoryOverride": {"reason": "x".repeat(140)}}).to_string(),
            )
            .unwrap_err();
        let FfiError::InvalidArgument { msg } = error else {
            panic!("expected InvalidArgument");
        };
        assert!(
            msg.contains("reason") || msg.contains("120"),
            "message: {msg}"
        );

        let error = a
            .set_config(serde_json::json!({"regulatoryOverride": {"bogus": true}}).to_string())
            .unwrap_err();
        assert!(matches!(error, FfiError::InvalidArgument { .. }));
    }

    #[test]
    fn errors_are_typed_and_message_ids_are_returned() {
        let a = node("alice", 10);
        assert!(matches!(
            a.send_text(
                FfiTarget::Direct {
                    node_id_hex: "00112233445566ff".into()
                },
                "hello".into(),
                100
            ),
            Err(FfiError::UnknownPeer { .. })
        ));
        assert!(matches!(
            a.ingest_frame(vec![0u8; MAX_FRAME_LEN + 1], 100),
            Err(FfiError::PayloadTooLarge { .. })
        ));
        assert!(matches!(
            a.ingest_frame(Vec::new(), 100),
            Err(FfiError::InvalidArgument { .. })
        ));
        assert!(matches!(
            a.send_text(
                FfiTarget::Direct {
                    node_id_hex: "not-hex".into()
                },
                "hello".into(),
                100
            ),
            Err(FfiError::InvalidArgument { .. })
        ));
        assert!(matches!(
            a.mark_read(123, 100),
            Err(FfiError::UnknownPeer { .. })
        ));
        let announced = a.announce(100).expect("announce");
        assert!(announced.msg_id.is_none());
        assert!(!announced.outbound.is_empty());
    }
    #[test]
    fn voice_tiers_encode_decode_and_round_trip() {
        fn sine(duration_ms: u64) -> Vec<u8> {
            let samples = (8000 * duration_ms / 1000) as usize;
            let mut pcm = Vec::with_capacity(samples * 2);
            for index in 0..samples {
                let t = index as f64 / 8000.0;
                let value = (2.0 * std::f64::consts::PI * 440.0 * t).sin() * 12_000.0;
                pcm.extend_from_slice(&(value as i16).to_le_bytes());
            }
            pcm
        }
        for (tier, expected_bytes) in [("tiny", 175usize), ("medium", 400)] {
            let pcm = sine(1_000);
            let encoded = openmesh_voice_encode(tier.to_string(), pcm.clone()).expect("encode");
            assert_eq!(encoded.len(), expected_bytes, "{tier} 1 s payload");
            let decoded = openmesh_voice_decode(tier.to_string(), encoded).expect("decode voice");
            assert_eq!(decoded.len(), pcm.len());
            assert!(
                decoded.iter().any(|byte| *byte != 0),
                "{tier} decode silent"
            );
        }
    }

    #[test]
    fn voice_tiers_reject_high_invalid_and_empty() {
        let pcm = vec![0u8; 16_000];
        let high = openmesh_voice_encode("high".to_string(), pcm.clone()).unwrap_err();
        assert!(matches!(high, FfiError::InvalidArgument { .. }));
        let invalid = openmesh_voice_encode("wizard".to_string(), pcm.clone()).unwrap_err();
        let FfiError::InvalidArgument { msg } = invalid else {
            panic!("expected InvalidArgument");
        };
        assert!(msg.contains("unknown voice tier"));
        assert!(matches!(
            openmesh_voice_decode("high".to_string(), vec![0u8; 7]),
            Err(FfiError::InvalidArgument { .. })
        ));
        assert!(matches!(
            openmesh_voice_encode("tiny".to_string(), Vec::new()),
            Err(FfiError::InvalidArgument { .. })
        ));
        assert!(matches!(
            openmesh_voice_decode("tiny".to_string(), Vec::new()),
            Err(FfiError::InvalidArgument { .. })
        ));
    }

    #[test]
    fn voice_tiers_json_contains_all_formats() {
        let json: serde_json::Value =
            serde_json::from_str(&openmesh_voice_tiers_json()).expect("tiers JSON");
        assert_eq!(json["tiny"]["mime"], "audio/c2-1300");
        assert_eq!(json["tiny"]["mode"], "1300");
        assert_eq!(json["tiny"]["frame_ms"], 40);
        assert_eq!(json["medium"]["mime"], "audio/c2-3200");
        assert_eq!(json["medium"]["bits_per_second"], 3200);
        assert_eq!(json["high"]["mime"], "audio/ogg; codecs=opus");
        assert_eq!(json["high"]["codec"], "opus");
        assert_eq!(json["tiny"]["max_duration_s"], 30);
    }
}

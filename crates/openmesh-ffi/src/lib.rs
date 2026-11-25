//! # openmesh-ffi — UniFFI-ready surface for OpenMesh mobile clients
//!
//! This crate is the bridge between [`openmesh_core`] and native apps (Android first, iOS
//! later). It wraps the IO-free protocol engine in a small, thread-safe, panic-free API that
//! UniFFI can generate Kotlin/Swift bindings from. The generated bindings are enabled in M2
//! (see the integration notes at the bottom of this file); the API surface itself is real and
//! delegates to the core today.
//!
//! ## Why the surface looks like this
//!
//! The companion API (`docs/architecture/COMPANION-API.md`) deliberately keeps radios dumb:
//! apps own identity, storage and UI. The FFI mirror of that decision is:
//!
//! * an opaque [`FfiEngine`] handle that wraps `openmesh_core::Engine` behind a `Mutex`;
//! * **no callbacks into platform code** — every method returns a [`FfiProcessed`] value with
//!   the events produced and the exact OMP frames to transmit. The Kotlin wrapper turns those
//!   into a `Flow`/`StateFlow`; re-entering Kotlin from a Rust thread inside JNI is where mobile
//!   FFIs usually break;
//! * **caller-owned identity material**: the seed is passed in as bytes and never returned;
//! * explicit typed errors ([`FfiError`]) rather than panics across the FFI boundary;
//! * `Vec<u8>`/`ByteArray` for OMP frames and attachment bytes, strings for hex node ids,
//!   `u64` milliseconds for time (the engine never reads the clock; the app supplies `now_ms`
//!   so behaviour stays deterministic and testable).
//!
//! ## Planned UniFFI mapping (what the generated Kotlin will look like)
//!
//! | Rust (this crate) | Kotlin (generated) |
//! | --- | --- |
//! | `FfiEngine::create(seed, config)` | `FfiEngine.create(seed, config)` |
//! | `FfiEngine::send_text(peer_id, text, now_ms)` | `engine.sendText(peerId, text, nowMs)` |
//! | `FfiEngine::ingest(frame, rssi, snr, now_ms)` | `engine.ingest(frame, rssi, snr, nowMs)` |
//! | `FfiEngine::tick(now_ms)` / `announce(now_ms)` | `engine.tick(nowMs)` / `engine.announce(nowMs)` |
//! | `FfiProcessed { events, outbound, msg_id }` | `data class FfiProcessed(...)` |
//! | `FfiEngine::nodes()` / `stats()` / `add_contact(...)` | `engine.nodes()` / `engine.stats()` / `engine.addContact(...)` |
//! | `FfiRole` / `FfiEvent` / `FfiError` | `enum class` / `sealed class` |
//!
//! The Android scaffold (`apps/android`) already has the matching Kotlin seam
//! (`data/engine/EngineBridge.kt`); replacing `DemoEngineBridge` with a thin wrapper over the
//! generated bindings is the M2 task, and no UI code needs to change.
//!
//! ## Threading and lifetime
//!
//! One engine per local identity. The handle is reference-counted (`Arc`); engine state is
//! behind a `Mutex`. Methods may be called from any thread but must not re-enter the same
//! handle (no callbacks are invoked, so this cannot happen accidentally). Frames returned in
//! [`FfiProcessed::outbound`] must be handed to the radio/bridge transport in order; the engine
//! performs no IO itself.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

use std::sync::{Arc, Mutex};

use openmesh_core::{
    AckKind, AppMessage, CRATE_VERSION, Engine, EngineConfig, Event, FrameType, Identity, LinkInfo,
    MessageKind, NodeClass, NodeId, NodeInfo, Position, Processed, PublicIdentity, Target,
    Telemetry, VERSION_STR,
};

/// Seed length accepted by [`FfiEngine::create`] (32 bytes).
pub const SEED_LEN: usize = 32;
/// Encoded public identity length (node id + Ed25519 + X25519 public keys).
pub const IDENTITY_LEN: usize = 72;
/// Maximum OMP frame size accepted by [`FfiEngine::ingest`].
pub const MAX_FRAME_LEN: usize = openmesh_core::MTU;

/// What the node does for the mesh. Mirrors `NodeClass` in the core, with a `Role`-derived
/// forwarding/storage behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FfiRole {
    /// End-user node: sends/receives its own traffic, never stores for others.
    Client,
    /// Infrastructure node: floods frames and may store envelopes for offline peers.
    Repeater,
    /// Bridges LoRa to IP (Wi-Fi/Ethernet).
    Gateway,
    /// Stores and serves channel history (future room-server feature).
    RoomServer,
    /// Low-power GPS tracker.
    Tracker,
}

impl FfiRole {
    fn engine_config(self, name: &str) -> EngineConfig {
        let mut config = match self {
            Self::Client | Self::Tracker => EngineConfig::client(name),
            Self::Repeater | Self::Gateway | Self::RoomServer => EngineConfig::repeater(name),
        };
        config.class = match self {
            Self::Client => NodeClass::Client,
            Self::Repeater => NodeClass::Repeater,
            Self::Gateway => NodeClass::Gateway,
            Self::RoomServer => NodeClass::RoomServer,
            Self::Tracker => NodeClass::Tracker,
        };
        config
    }
}

/// Engine configuration supplied at construction time.
///
/// `profile` and `region` are radio-level settings (they select the modem configuration and
/// duty-cycle model); the engine itself is medium-agnostic but uses them for airtime defaults.
/// Both are validated against the tables in `openmesh_core::util`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FfiEngineConfig {
    /// Display name announced to the mesh (0–32 UTF-8 bytes in the announce).
    pub name: String,
    /// Role/class of the node.
    pub role: FfiRole,
    /// Announce period in milliseconds (clients default to 1 800 000; repeaters 3 600 000).
    pub announce_interval_ms: u64,
    /// LoRa profile name (`LONG_SLOW`, `LONG_FAST`, `MEDIUM_FAST`, `SHORT_FAST`, `SHORT_TURBO`).
    pub profile: String,
    /// Region name (`EU868`, `US915`, `AU915`, `IN865`, `KR920`, `AS923`).
    pub region: String,
    /// Advertised capability bits (`openmesh_core::services::caps`). `None` keeps the role's
    /// defaults (RELAY for clients, RELAY|STORE for repeaters).
    pub caps: Option<u16>,
}

impl Default for FfiEngineConfig {
    fn default() -> Self {
        Self {
            name: "openmesh".to_string(),
            role: FfiRole::Client,
            announce_interval_ms: 30 * 60_000,
            profile: "MEDIUM_FAST".to_string(),
            region: "EU868".to_string(),
            caps: None,
        }
    }
}

impl FfiEngineConfig {
    fn validate(&self) -> Result<(), FfiError> {
        if !openmesh_core::util::PROFILES
            .iter()
            .any(|p| p.name == self.profile)
        {
            return Err(FfiError::InvalidConfig("unknown LoRa profile"));
        }
        if !openmesh_core::util::REGIONS
            .iter()
            .any(|r| r.name == self.region)
        {
            return Err(FfiError::InvalidConfig("unknown region"));
        }
        if self.name.len() > 32 {
            return Err(FfiError::InvalidConfig("node name longer than 32 bytes"));
        }
        Ok(())
    }
}

/// A geographic position fix (fixed-point degrees, per PROTOCOL.md §7.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FfiPosition {
    /// Latitude in 1e-7 degrees.
    pub lat_e7: i32,
    /// Longitude in 1e-7 degrees.
    pub lon_e7: i32,
    /// Altitude in metres when known.
    pub alt_m: Option<i16>,
    /// Horizontal accuracy in metres when known.
    pub accuracy_m: Option<u8>,
    /// Sender's Unix milliseconds.
    pub timestamp_ms: u64,
}

impl From<&Position> for FfiPosition {
    fn from(position: &Position) -> Self {
        Self {
            lat_e7: position.lat_e7,
            lon_e7: position.lon_e7,
            alt_m: position.alt_m,
            accuracy_m: position.accuracy_m,
            timestamp_ms: position.timestamp_ms,
        }
    }
}

/// A node the engine has learned about.
#[derive(Clone, Debug, PartialEq)]
pub struct FfiNode {
    /// 8-byte node id, lowercase hex (16 characters).
    pub node_id: String,
    /// Announced display name (may be empty).
    pub name: String,
    /// `client`, `repeater`, `gateway`, `room_server`, `tracker`.
    pub class: String,
    /// Capability strings for readability: `gps`, `wifi`, `ethernet`, `store`, `relay`, ...
    pub caps: Vec<String>,
    /// Announce hop count when known.
    pub hops: u8,
    /// Unix milliseconds when the node was last heard.
    pub last_seen_ms: u64,
    /// RSSI in dBm when the last frame carried it.
    pub rssi: Option<i16>,
    /// SNR in dB when the last frame carried it.
    pub snr: Option<f32>,
    /// Last position fix reported by this node, if any.
    pub last_position: Option<FfiPosition>,
    /// Last telemetry battery percentage, if any.
    pub battery_pct: Option<u8>,
    /// Whether the user marked this node as a contact.
    pub is_contact: bool,
}

/// An application message surfaced to the client.
#[derive(Clone, Debug, PartialEq)]
pub struct FfiMessage {
    /// Random 32-bit message id chosen by the sender.
    pub msg_id: u32,
    /// Conversation partner: peer hex node id for direct messages, channel hex id for groups.
    pub peer_id: String,
    /// Sender node id (hex).
    pub from: String,
    /// Recipient node id or channel id (hex).
    pub to: String,
    /// Channel id when the message arrived on a channel (`None` for direct messages).
    pub channel: Option<String>,
    /// `text`, `position`, `attachment`, `group_invite`, `telemetry`, `control`.
    pub kind: String,
    /// UTF-8 text for text messages.
    pub text: Option<String>,
    /// Sender's Unix milliseconds.
    pub sent_at_ms: u64,
    /// True when this is an echo of a locally originated message.
    pub outgoing: bool,
    /// Message id this message replies to, when present.
    pub reply_to: Option<u32>,
}

/// One event produced by the engine.
#[derive(Clone, Debug, PartialEq)]
pub enum FfiEvent {
    /// A direct message was received and decrypted.
    MessageReceived(FfiMessage),
    /// A channel/group message was received, decrypted and signature-verified.
    GroupMessageReceived(FfiMessage),
    /// A delivery/read/resource acknowledgement arrived.
    Acknowledgement {
        /// Message being acknowledged.
        msg_id: u32,
        /// Peer that acknowledged.
        peer_id: String,
        /// `delivered`, `read`, `resource_window`, `resource_complete`.
        kind: String,
    },
    /// A position report was received.
    Position {
        /// Reporting node (hex).
        from: String,
        /// Channel id when it arrived on a channel (hex), `None` for direct.
        channel: Option<String>,
        /// The fix.
        position: FfiPosition,
    },
    /// An infrastructure node reported telemetry.
    Telemetry {
        /// Reporting node (hex).
        from: String,
        /// Battery percentage when reported.
        battery_pct: Option<u8>,
        /// Battery voltage in millivolts when reported.
        battery_mv: Option<u16>,
        /// Airtime used in milliseconds when reported.
        airtime_used_ms: Option<u32>,
        /// Node uptime in seconds when reported.
        uptime_s: Option<u32>,
    },
    /// A node was discovered or refreshed.
    NodeSeen(FfiNode),
    /// A relay holds envelopes for this node (store-and-forward).
    StoreOffer {
        /// Relay node id (hex).
        relay: String,
        /// Message ids the relay holds.
        ids: Vec<u32>,
    },
    /// A relay accepted an envelope for later delivery.
    StoreAccepted {
        /// Message id.
        msg_id: u32,
        /// Relay node id (hex).
        relay: String,
    },
    /// Progress of an incoming/outgoing resource transfer (images, voice notes, long text).
    ResourceProgress {
        /// Peer the transfer is with (hex node id or channel id).
        peer_id: String,
        /// Resource id.
        res_id: u32,
        /// Chunks received (incoming) or acknowledged (outgoing).
        received: u16,
        /// Total chunks expected.
        total: u16,
    },
    /// A resource transfer completed, verified and decrypted.
    ResourceReceived {
        /// Sender node id (hex).
        peer_id: String,
        /// Channel id when the resource arrived on a channel (hex).
        channel: Option<String>,
        /// Message that owns the resource.
        msg_id: u32,
        /// MIME type from the manifest.
        mime: String,
        /// Total verified plaintext length in bytes.
        total_len: u32,
        /// Decrypted payload: attachment bytes, or CBOR for long text / channel history.
        data: Vec<u8>,
    },
    /// One of our outgoing resource transfers was fully acknowledged.
    ResourceSent {
        /// Peer that completed the transfer (hex node id or channel id).
        peer_id: String,
        /// Resource id.
        res_id: u32,
    },
    /// A non-fatal protocol problem worth surfacing in logs/UI.
    Error(String),
}

/// The result of feeding input to the engine or asking it to originate a message.
///
/// `events` must be surfaced to the UI; `outbound` are complete OMP frames that must be
/// transmitted (via the companion `Tx` message, the bridge transport, or the simulator) in the
/// order given. For `send_text`, `msg_id` is the id of the originated message so the UI can
/// track delivery.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FfiProcessed {
    /// Events to surface.
    pub events: Vec<FfiEvent>,
    /// Encoded OMP frames to transmit (each ≤ [`MAX_FRAME_LEN`]).
    pub outbound: Vec<Vec<u8>>,
    /// Message id of the originated message, for send methods only.
    pub msg_id: Option<u32>,
}

/// Errors crossing the FFI boundary. UniFFI maps these to a Kotlin sealed exception hierarchy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FfiError {
    /// The seed was not exactly [`SEED_LEN`] bytes.
    InvalidSeed,
    /// A node id was not 16 hex characters, or a public identity was not [`IDENTITY_LEN`] bytes.
    InvalidIdentity,
    /// A frame was empty or larger than [`MAX_FRAME_LEN`].
    InvalidFrame,
    /// A configuration value was unknown or out of range.
    InvalidConfig(&'static str),
    /// The core rejected the operation (unknown peer, crypto, resource, codec, ...).
    Core(String),
}

impl core::fmt::Display for FfiError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidSeed => write!(f, "seed must be {SEED_LEN} bytes"),
            Self::InvalidIdentity => write!(f, "invalid node id or public identity"),
            Self::InvalidFrame => write!(f, "invalid frame (empty or > {MAX_FRAME_LEN} bytes)"),
            Self::InvalidConfig(what) => write!(f, "invalid configuration: {what}"),
            Self::Core(message) => write!(f, "core error: {message}"),
        }
    }
}

impl std::error::Error for FfiError {}

/// Engine statistics surfaced to the UI.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FfiStats {
    /// Frames transmitted by this engine.
    pub tx_frames: u64,
    /// Frames received.
    pub rx_frames: u64,
    /// Frames forwarded for others.
    pub forwarded_frames: u64,
    /// Frames dropped (dedup, TTL, malformed, decrypt failures).
    pub dropped_frames: u64,
    /// Duplicate frames suppressed.
    pub dedup_hits: u64,
    /// Known node count.
    pub known_nodes: u32,
    /// Channels joined.
    pub channels: u32,
    /// Encrypted envelopes retained for others (repeaters).
    pub stored_envelopes: u32,
    /// Best-effort transmitted airtime in milliseconds.
    pub airtime_tx_ms: u64,
}

/// Opaque engine handle. One instance per local identity; create it once and keep it for the
/// app's lifetime (UniFFI objects are reference-counted / finalized; do not recreate per call).
pub struct FfiEngine {
    inner: Mutex<Engine>,
    public_identity: Vec<u8>,
    node_id_hex: String,
    config: FfiEngineConfig,
}

impl core::fmt::Debug for FfiEngine {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Never print key material: the public identity and node id are public, the seed is
        // not stored in this struct at all.
        f.debug_struct("FfiEngine")
            .field("node_id", &self.node_id_hex)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl FfiEngine {
    /// Create an engine from a 32-byte seed and configuration.
    ///
    /// UniFFI will expose this as `FfiEngine.create(seed, config)`; the seed must come from the
    /// platform CSPRNG and be stored encrypted (Android Keystore) before the engine exists.
    pub fn create(seed: Vec<u8>, config: FfiEngineConfig) -> Result<Arc<Self>, FfiError> {
        if seed.len() != SEED_LEN {
            return Err(FfiError::InvalidSeed);
        }
        config.validate()?;
        let mut seed_bytes = [0u8; SEED_LEN];
        seed_bytes.copy_from_slice(&seed);
        drop(seed); // the caller-owned bytes are copied and not retained

        let identity = Identity::from_seed(&seed_bytes);
        let public_identity = identity.public();
        let node_id_hex = public_identity.node_id.to_hex();
        let mut engine_config = config.role.engine_config(&config.name);
        engine_config.announce_interval_ms = config.announce_interval_ms;
        if let Some(caps) = config.caps {
            engine_config.caps = caps;
        }
        let engine = Engine::new(identity, engine_config);

        Ok(Arc::new(Self {
            inner: Mutex::new(engine),
            public_identity: public_identity.to_bytes().to_vec(),
            node_id_hex,
            config,
        }))
    }

    /// Generate a fresh identity seed from the OS CSPRNG (32 bytes).
    ///
    /// Keeping this in Rust means the app does not need a second RNG implementation; the
    /// generated seed should be encrypted with the platform keystore immediately.
    pub fn generate_seed() -> Vec<u8> {
        Identity::generate().seed().to_vec()
    }

    /// This engine's self-certifying node id as lowercase hex (16 characters).
    pub fn node_id(&self) -> String {
        self.node_id_hex.clone()
    }

    /// The 72-byte public identity (`node_id || Ed25519 public || X25519 public`).
    ///
    /// Apps share this via QR (`openmesh://contact?identity=…`). It is public data.
    pub fn public_identity(&self) -> Vec<u8> {
        self.public_identity.clone()
    }

    /// The configured protocol version string (`omp/1`).
    pub fn protocol_version(&self) -> String {
        VERSION_STR.to_string()
    }

    /// The linked core crate version.
    pub fn core_version(&self) -> String {
        CRATE_VERSION.to_string()
    }

    /// Current configuration (reflecting runtime changes such as the announce interval).
    pub fn config(&self) -> FfiEngineConfig {
        self.config.clone()
    }

    /// Change the announce interval without recreating the engine.
    pub fn set_announce_interval(&self, interval_ms: u64) -> Result<(), FfiError> {
        if interval_ms < 300_000 {
            return Err(FfiError::InvalidConfig(
                "announce interval below the 5 minute floor (PROTOCOL.md §6.2)",
            ));
        }
        let mut engine = self.lock()?;
        let mut config = engine.config().clone();
        config.announce_interval_ms = interval_ms;
        engine.set_config(config);
        Ok(())
    }

    /// Add a contact from a 72-byte public identity (as scanned from a QR code).
    pub fn add_contact(&self, identity: Vec<u8>, name: Option<String>) -> Result<(), FfiError> {
        if identity.len() != IDENTITY_LEN {
            return Err(FfiError::InvalidIdentity);
        }
        let public =
            PublicIdentity::from_bytes(&identity).map_err(|_| FfiError::InvalidIdentity)?;
        if !public.is_self_consistent() {
            return Err(FfiError::InvalidIdentity);
        }
        let mut engine = self.lock()?;
        engine.add_contact(public, name);
        Ok(())
    }

    /// Remove a contact by hex node id.
    pub fn remove_contact(&self, node_id: String) -> Result<(), FfiError> {
        let id = NodeId::from_hex(&node_id).map_err(|_| FfiError::InvalidIdentity)?;
        let mut engine = self.lock()?;
        engine.remove_contact(id);
        Ok(())
    }

    /// Join a channel from a 32-byte PSK and a name; returns the channel id (hex).
    pub fn add_channel(&self, name: String, psk: Vec<u8>) -> Result<String, FfiError> {
        if psk.len() != 32 {
            return Err(FfiError::InvalidIdentity);
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&psk);
        let mut engine = self.lock()?;
        let id = engine.add_channel(&name, &key);
        Ok(id.to_hex())
    }

    /// Leave a channel by hex channel id.
    pub fn remove_channel(&self, channel_id: String) -> Result<(), FfiError> {
        let id = NodeId::from_hex(&channel_id).map_err(|_| FfiError::InvalidIdentity)?;
        let mut engine = self.lock()?;
        engine.remove_channel(id);
        Ok(())
    }

    /// Known nodes (the UI sorts contacts/hops itself).
    pub fn nodes(&self) -> Result<Vec<FfiNode>, FfiError> {
        let engine = self.lock()?;
        Ok(engine.known_nodes().into_iter().map(node_to_ffi).collect())
    }

    /// Engine counters for the status screen and the simulator.
    pub fn stats(&self) -> Result<FfiStats, FfiError> {
        let engine = self.lock()?;
        let stats = engine.stats();
        Ok(FfiStats {
            tx_frames: stats.tx_frames,
            rx_frames: stats.rx_frames,
            forwarded_frames: stats.forwarded_frames,
            dropped_frames: stats.dropped_frames,
            dedup_hits: stats.dedup_hits,
            known_nodes: stats.known_nodes as u32,
            channels: stats.channels as u32,
            stored_envelopes: stats.stored_envelopes as u32,
            airtime_tx_ms: stats.airtime_tx_ms,
        })
    }

    /// Build and emit a signed announce. The returned frames must be transmitted.
    pub fn announce(&self, now_ms: u64) -> Result<FfiProcessed, FfiError> {
        let mut engine = self.lock()?;
        let processed = engine.announce(now_ms).map_err(map_core_error)?;
        map_processed(processed)
    }

    /// Send a direct text message; the returned [`FfiProcessed::msg_id`] is the message id.
    ///
    /// The peer must be known (a contact, or a node whose announce has been received and
    /// decoded) because the message is sealed with the static ECDH key from its identity.
    pub fn send_text(
        &self,
        peer_id: String,
        text: String,
        now_ms: u64,
    ) -> Result<FfiProcessed, FfiError> {
        if text.is_empty() {
            return Err(FfiError::InvalidConfig("empty message text"));
        }
        let peer = NodeId::from_hex(&peer_id).map_err(|_| FfiError::InvalidIdentity)?;
        let mut engine = self.lock()?;
        let processed = engine
            .send_text(Target::Direct(peer), &text, now_ms)
            .map_err(map_core_error)?;
        map_processed(processed)
    }

    /// Feed one complete OMP frame received from the radio/bridge; returns events and frames
    /// to relay. Malformed frames are reported by the core as dropped; only oversized or empty
    /// input is rejected here before touching the engine.
    pub fn ingest(
        &self,
        frame: Vec<u8>,
        rssi: Option<i16>,
        snr: Option<f32>,
        now_ms: u64,
    ) -> Result<FfiProcessed, FfiError> {
        if frame.is_empty() || frame.len() > MAX_FRAME_LEN {
            return Err(FfiError::InvalidFrame);
        }
        let link = LinkInfo {
            rssi,
            snr,
            ..Default::default()
        };
        let mut engine = self.lock()?;
        let processed = engine.ingest(&frame, Some(link), now_ms);
        map_processed(processed)
    }

    /// Advance timers (re-announces, retransmissions, resource ARQ, store offers, peer aging).
    pub fn tick(&self, now_ms: u64) -> Result<FfiProcessed, FfiError> {
        let mut engine = self.lock()?;
        let processed = engine.tick(now_ms);
        map_processed(processed)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Engine>, FfiError> {
        self.inner
            .lock()
            .map_err(|_| FfiError::Core("engine lock poisoned".to_string()))
    }
}

fn map_core_error(error: openmesh_core::Error) -> FfiError {
    FfiError::Core(error.to_string())
}

fn map_processed(processed: Processed) -> Result<FfiProcessed, FfiError> {
    // The clear-text msg_ref lives at body[1..5] (payload_flags(1) || msg_ref(4 BE)); use it to
    // recover the originated message id from the first Data frame. This keeps the FFI
    // independent of engine internals while matching PROTOCOL.md §5.
    let msg_id = processed.outbound.iter().find_map(|outbound| {
        if outbound.frame.typ == FrameType::Data && outbound.frame.body.len() >= 5 {
            Some(u32::from_be_bytes([
                outbound.frame.body[1],
                outbound.frame.body[2],
                outbound.frame.body[3],
                outbound.frame.body[4],
            ]))
        } else {
            None
        }
    });
    let mut frames = Vec::with_capacity(processed.outbound.len());
    for outbound in processed.outbound {
        frames.push(outbound.frame.encode().map_err(map_core_error)?);
    }
    Ok(FfiProcessed {
        events: processed.events.into_iter().map(map_event).collect(),
        outbound: frames,
        msg_id,
    })
}

fn map_event(event: Event) -> FfiEvent {
    match event {
        Event::Message(received) => FfiEvent::MessageReceived(message_to_ffi(
            received.from,
            received.from,
            None,
            &received.message,
            false,
        )),
        Event::GroupMessage(received) => FfiEvent::GroupMessageReceived(message_to_ffi(
            received.sender,
            received.channel,
            Some(received.channel),
            &received.message,
            false,
        )),
        Event::Ack { from, ack } => FfiEvent::Acknowledgement {
            msg_id: ack.msg_id,
            peer_id: from.to_hex(),
            kind: ack_kind_str(ack.kind).to_string(),
        },
        Event::Position {
            from,
            channel,
            position,
        } => FfiEvent::Position {
            from: from.to_hex(),
            channel: channel.map(|id| id.to_hex()),
            position: FfiPosition::from(&position),
        },
        Event::Telemetry { from, telemetry } => telemetry_to_ffi(from, &telemetry),
        Event::NodeSeen(info) => FfiEvent::NodeSeen(node_to_ffi(info)),
        Event::StoreOffer { from, ids } => FfiEvent::StoreOffer {
            relay: from.to_hex(),
            ids,
        },
        Event::StoreAccepted { msg_id, relay } => FfiEvent::StoreAccepted {
            msg_id,
            relay: relay.to_hex(),
        },
        Event::ResourceProgress {
            peer,
            res_id,
            received,
            total,
        } => FfiEvent::ResourceProgress {
            peer_id: peer.to_hex(),
            res_id,
            received,
            total,
        },
        Event::ResourceReceived {
            peer,
            channel,
            manifest,
            data,
        } => FfiEvent::ResourceReceived {
            peer_id: peer.to_hex(),
            channel: channel.map(|id| id.to_hex()),
            msg_id: manifest.msg_id,
            mime: manifest.mime,
            total_len: manifest.total_len,
            data,
        },
        Event::ResourceSent { peer, res_id } => FfiEvent::ResourceSent {
            peer_id: peer.to_hex(),
            res_id,
        },
        Event::Error(message) => FfiEvent::Error(message),
    }
}

fn message_to_ffi(
    from: NodeId,
    peer: NodeId,
    channel: Option<NodeId>,
    message: &AppMessage,
    outgoing: bool,
) -> FfiMessage {
    FfiMessage {
        msg_id: message.id,
        peer_id: if let Some(channel) = channel {
            channel.to_hex()
        } else {
            peer.to_hex()
        },
        from: from.to_hex(),
        to: if let Some(channel) = channel {
            channel.to_hex()
        } else {
            peer.to_hex()
        },
        channel: channel.map(|id| id.to_hex()),
        kind: message_kind_str(message.kind).to_string(),
        text: message.text.clone(),
        sent_at_ms: message.sent_at_ms,
        outgoing,
        reply_to: message.reply_to,
    }
}

fn telemetry_to_ffi(from: NodeId, telemetry: &Telemetry) -> FfiEvent {
    FfiEvent::Telemetry {
        from: from.to_hex(),
        battery_pct: telemetry.battery_pct,
        battery_mv: telemetry.battery_mv,
        airtime_used_ms: telemetry.airtime_used_ms,
        uptime_s: telemetry.uptime_s,
    }
}

fn node_to_ffi(info: NodeInfo) -> FfiNode {
    FfiNode {
        node_id: info.node_id.to_hex(),
        name: info.name,
        class: info.class.as_str().to_string(),
        caps: caps_to_strings(info.caps),
        hops: info.hops,
        last_seen_ms: info.last_seen_ms,
        rssi: info.rssi,
        snr: info.snr,
        last_position: info.last_position.as_ref().map(FfiPosition::from),
        battery_pct: info.battery_pct,
        is_contact: info.is_contact,
    }
}

fn caps_to_strings(caps: u16) -> Vec<String> {
    use openmesh_core::services::caps;
    let mut out = Vec::new();
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
    for (bit, name) in table {
        if caps & bit != 0 {
            out.push(name.to_string());
        }
    }
    out
}

fn message_kind_str(kind: MessageKind) -> &'static str {
    match kind {
        MessageKind::Text => "text",
        MessageKind::Position => "position",
        MessageKind::Attachment => "attachment",
        MessageKind::GroupInvite => "group_invite",
        MessageKind::Telemetry => "telemetry",
        MessageKind::Control => "control",
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

/// Version string of the linked protocol core (kept for diagnostics/About screens).
pub fn version() -> String {
    format!("openmesh-core {CRATE_VERSION} ({VERSION_STR})")
}

// ---------------------------------------------------------------------------------------------
// UniFFI integration notes (bindings enabled in M2, not part of v0.1.0)
// ---------------------------------------------------------------------------------------------
//
// When the Android client is ready to swap in the real core, add:
//
//   [dependencies]
//   uniffi = { version = "0.29", features = ["cli"] }
//
//   [build-dependencies]
//   uniffi = { version = "0.29", features = ["build"] }
//
// Add `uniffi::setup_scaffolding!();` plus `#[uniffi::export]` on the `impl FfiEngine` block
// (proc-macro style fits this small API better than UDL). Generate bindings in CI:
//
//   cargo build -p openmesh-ffi
//   cargo install uniffi-bindgen --version 0.29.0
//   uniffi-bindgen generate --library target/debug/libopenmesh_ffi.so \
//       --language kotlin --out-dir apps/android/app/src/main/java
//
// Generated Kotlin lands under `apps/android/app/src/main/java/uniffi/openmesh/` and is not
// hand-edited; `apps/android/.../data/engine/EngineBridge.kt` adapts it to the app's coroutines
// and StateFlow. Add `cargo test -p openmesh-ffi` to CI before generating bindings.
//
// Design invariants to preserve when implementing:
// 1. No Rust callbacks that call into Kotlin; return events in FfiProcessed instead.
// 2. No panics across the boundary; map every error to FfiError.
// 3. Secrets never cross back: node_id()/public_identity() are public; the seed is not
//    returned by any method after create.
// 4. Engine state is behind a Mutex; callers may be on any thread but must not assume
//    re-entrancy. UniFFI objects are Send + Sync when their contents are.
// 5. Time is always passed in (now_ms) so tests and the simulator are deterministic.
// 6. MAX_FRAME_LEN (= MTU = 240) is enforced at ingest; attachment bytes travel through the
//    resource layer/bridge, not through this API in one call.

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Arc<FfiEngine> {
        let config = FfiEngineConfig {
            name: "test-node".into(),
            ..Default::default()
        };
        FfiEngine::create(
            vec![
                7u8, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
                7, 7, 7, 7, 7,
            ],
            config,
        )
        .expect("engine")
    }

    #[test]
    fn seed_length_is_enforced() {
        assert_eq!(
            FfiEngine::create(vec![0u8; 31], FfiEngineConfig::default()).unwrap_err(),
            FfiError::InvalidSeed
        );
        assert_eq!(
            FfiEngine::create(vec![0u8; 33], FfiEngineConfig::default()).unwrap_err(),
            FfiError::InvalidSeed
        );
    }

    #[test]
    fn identity_is_deterministic_and_public_only() {
        let a = engine();
        let b = engine();
        assert_eq!(a.node_id(), b.node_id());
        assert_eq!(a.node_id().len(), 16);
        assert_eq!(a.public_identity().len(), IDENTITY_LEN);
        assert_eq!(a.protocol_version(), "omp/1");
        // The node id is the hash prefix of the two public keys, so a contact round-trip works.
        a.add_contact(a.public_identity(), Some("self".into()))
            .unwrap();
        assert!(a.nodes().unwrap().iter().any(|n| n.node_id == a.node_id()));
    }

    #[test]
    fn reject_invalid_identity_and_config() {
        let e = engine();
        assert_eq!(
            e.add_contact(vec![0u8; 71], None).unwrap_err(),
            FfiError::InvalidIdentity
        );
        assert_eq!(
            FfiEngine::create(
                vec![0u8; 32],
                FfiEngineConfig {
                    profile: "NOPE".into(),
                    ..Default::default()
                }
            )
            .unwrap_err(),
            FfiError::InvalidConfig("unknown LoRa profile")
        );
    }

    #[test]
    fn announce_emits_a_valid_omp_frame() {
        let e = engine();
        let processed = e.announce(0).expect("announce");
        assert!(!processed.outbound.is_empty());
        for bytes in &processed.outbound {
            assert!(bytes.len() <= MAX_FRAME_LEN);
            assert!(openmesh_core::Frame::decode(bytes).is_ok());
        }
    }

    #[test]
    fn sending_to_an_unknown_peer_is_a_typed_error() {
        let e = engine();
        let err = e
            .send_text("0101010101010101".into(), "hello".into(), 1_000)
            .unwrap_err();
        assert!(matches!(err, FfiError::Core(_)));
    }

    #[test]
    fn malformed_input_is_rejected_or_dropped_without_panicking() {
        let e = engine();
        assert_eq!(
            e.ingest(Vec::new(), None, None, 0).unwrap_err(),
            FfiError::InvalidFrame
        );
        assert_eq!(
            e.ingest(vec![0u8; MAX_FRAME_LEN + 1], None, None, 0)
                .unwrap_err(),
            FfiError::InvalidFrame
        );
        // A full-size but invalid frame is handled by the core (dropped, no events).
        let processed = e
            .ingest(vec![0u8; MAX_FRAME_LEN], None, None, 0)
            .expect("ingest");
        assert!(processed.events.is_empty());
    }

    #[test]
    fn channels_can_be_joined_and_left() {
        let e = engine();
        let id = e
            .add_channel("family".into(), vec![9u8; 32])
            .expect("channel");
        assert_eq!(id.len(), 16);
        e.remove_channel(id).unwrap();
        assert_eq!(e.stats().unwrap().channels, 0);
    }
}

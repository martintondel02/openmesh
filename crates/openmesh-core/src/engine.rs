//! The host-side mesh engine: a deterministic, IO-free state machine.
//!
//! The engine owns all protocol logic (announces, encryption, deduplication, flooding,
//! acknowledgements, resource transfer ARQ, store-and-forward, node bookkeeping) and is driven
//! by its embedder:
//!
//! 1. Transport bytes arrive → [`Engine::ingest`] → [`Processed`]
//! 2. Application action (send message, share position, ...) → [`Processed`]
//! 3. Timers → [`Engine::tick`] → [`Processed`]
//!
//! The engine never performs IO, blocking, or wall-clock reads; the embedder supplies
//! `now_ms` and delivers [`Outbound`] frames to interfaces. This makes the full protocol
//! trivially testable and lets the simulator run thousands of virtual nodes in one process.

use std::collections::HashMap;

use crate::crypto::{ChannelKey, Identity, PublicIdentity};
use crate::error::Result;
use crate::services::{
    Ack, Announce, AppMessage, NodeClass, Position, ResourceKind, ResourceManifest, StoreWant,
    Telemetry,
};
use crate::wire::{Frame, NodeId};

/// Opaque identifier for a local transport interface (USB, BLE, UDP socket, simulator link, ...).
pub type LinkId = u16;

/// Metadata about the interface a frame arrived on.
#[derive(Clone, Debug, Default)]
pub struct LinkInfo {
    pub link: LinkId,
    /// Peer node id when the link is point-to-point (USB/BLE/TCP). `None` for broadcast media.
    pub peer: Option<NodeId>,
    pub rssi: Option<i16>,
    pub snr: Option<f32>,
}

/// What a node does for the mesh.
#[derive(Clone, Debug, PartialEq)]
pub enum Role {
    /// End-user node: sends and receives its own traffic, never stores for others.
    Client,
    /// Infrastructure node: floods frames and may store envelopes for offline peers.
    Relay(RelayConfig),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RelayConfig {
    /// Maximum number of encrypted envelopes retained for other nodes.
    pub store_capacity: usize,
    /// How long an envelope is retained before eviction.
    pub store_ttl_ms: u64,
    /// Flood budget: maximum frames this relay injects per minute.
    pub max_frames_per_min: u32,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self { store_capacity: 64, store_ttl_ms: 72 * 3_600_000, max_frames_per_min: 120 }
    }
}

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub class: NodeClass,
    pub name: String,
    pub caps: u16,
    /// How often the node re-announces itself.
    pub announce_interval_ms: u64,
    /// Default hop budget for originated frames.
    pub default_ttl: u8,
    pub role: Role,
    /// Whether to accept and decrypt group (channel) traffic.
    pub accept_channels: bool,
}

impl EngineConfig {
    pub fn client(name: impl Into<String>) -> Self {
        Self {
            class: NodeClass::Client,
            name: name.into(),
            caps: crate::services::caps::RELAY,
            announce_interval_ms: 30 * 60_000,
            default_ttl: 5,
            role: Role::Client,
            accept_channels: true,
        }
    }

    pub fn repeater(name: impl Into<String>) -> Self {
        Self {
            class: NodeClass::Repeater,
            name: name.into(),
            caps: crate::services::caps::RELAY | crate::services::caps::STORE,
            announce_interval_ms: 60 * 60_000,
            default_ttl: 5,
            role: Role::Relay(RelayConfig::default()),
            accept_channels: false,
        }
    }
}

/// Destination of an application message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// End-to-end encrypted direct message to one node.
    Direct(NodeId),
    /// Symmetric channel message (group chat, announces to a channel).
    Channel(NodeId),
}

/// How the embedder should transmit an outbound frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendMode {
    /// Broadcast on all interfaces.
    Flood,
    /// Point-to-point if the neighbor link exists, otherwise fall back to flooding.
    Direct(NodeId),
}

#[derive(Clone, Debug)]
pub struct Outbound {
    pub frame: Frame,
    pub mode: SendMode,
    /// Preferred interface when the frame is a link-bound reply.
    pub link: Option<LinkId>,
}

/// Result of processing: events for the application plus frames to transmit.
#[derive(Clone, Debug, Default)]
pub struct Processed {
    pub events: Vec<Event>,
    pub outbound: Vec<Outbound>,
}

impl Processed {
    pub fn is_empty(&self) -> bool {
        self.events.is_empty() && self.outbound.is_empty()
    }

    /// Absorb another processing result into this one (events then outbound).
    pub fn merge(&mut self, other: Processed) {
        self.events.extend(other.events);
        self.outbound.extend(other.outbound);
    }

    pub fn with_event(mut self, event: Event) -> Self {
        self.events.push(event);
        self
    }

    pub fn with_frame(mut self, frame: Frame, mode: SendMode) -> Self {
        self.outbound.push(Outbound { frame, mode, link: None });
        self
    }

    pub fn event(event: Event) -> Self {
        Self { events: vec![event], outbound: Vec::new() }
    }

    pub fn frame(frame: Frame, mode: SendMode) -> Self {
        Self { events: Vec::new(), outbound: vec![Outbound { frame, mode, link: None }] }
    }
}

#[derive(Clone, Debug)]
pub struct ReceivedMessage {
    pub from: NodeId,
    pub message: AppMessage,
    pub link: LinkInfo,
    /// True when the envelope was replayed by a relay from its store.
    pub via_store: bool,
}

#[derive(Clone, Debug)]
pub struct ReceivedGroupMessage {
    pub channel: NodeId,
    pub sender: NodeId,
    pub message: AppMessage,
    pub link: LinkInfo,
}

#[derive(Clone, Debug)]
pub enum Event {
    /// A direct message was received and decrypted.
    Message(ReceivedMessage),
    /// A channel/group message was received and decrypted.
    GroupMessage(ReceivedGroupMessage),
    /// An acknowledgement for one of our messages arrived.
    Ack { from: NodeId, ack: Ack },
    /// A position report was received (direct or channel).
    Position { from: NodeId, channel: Option<NodeId>, position: Position },
    /// An infrastructure node reported telemetry.
    Telemetry { from: NodeId, telemetry: Telemetry },
    /// A node was discovered or refreshed (announce or first traffic).
    NodeSeen(NodeInfo),
    /// A relay holds envelopes addressed to us.
    StoreOffer { from: NodeId, ids: Vec<u32> },
    /// A relay accepted an envelope for later delivery.
    StoreAccepted { msg_id: u32, relay: NodeId },
    /// Progress of an incoming or outgoing resource transfer.
    ResourceProgress { peer: NodeId, res_id: u32, received: u16, total: u16 },
    /// A resource transfer completed and verified.
    ResourceReceived {
        peer: NodeId,
        channel: Option<NodeId>,
        manifest: ResourceManifest,
        data: Vec<u8>,
    },
    /// One of our outgoing resource transfers was fully acknowledged.
    ResourceSent { peer: NodeId, res_id: u32 },
    /// Non-fatal protocol problem worth surfacing in logs/UI.
    Error(String),
}

/// Live view of a known node.
#[derive(Clone, Debug)]
pub struct NodeInfo {
    pub node_id: NodeId,
    pub ident: PublicIdentity,
    pub name: String,
    pub class: NodeClass,
    pub caps: u16,
    /// Announce hop count when known.
    pub hops: u8,
    pub last_seen_ms: u64,
    pub rssi: Option<i16>,
    pub snr: Option<f32>,
    pub last_position: Option<Position>,
    pub battery_pct: Option<u8>,
    pub is_contact: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub tx_frames: u64,
    pub rx_frames: u64,
    pub forwarded_frames: u64,
    pub dropped_frames: u64,
    pub dedup_hits: u64,
    pub stored_envelopes: usize,
    pub known_nodes: usize,
    pub channels: usize,
    /// Best-effort on-air time of frames emitted by this engine.
    pub airtime_tx_ms: u64,
}

/// Serializable subset of engine state, persisted by the embedder.
#[derive(Clone, Debug, Default)]
pub struct StateSnapshot {
    pub contacts: Vec<(PublicIdentity, Option<String>)>,
    pub channels: Vec<(NodeId, String, [u8; 32])>,
    pub last_seq: u32,
}

/// The mesh protocol state machine.
pub struct Engine {
    identity: Identity,
    config: EngineConfig,
    now_ms: u64,
    seq: u32,
    peers: HashMap<NodeId, PeerEntry>,
    contacts: HashMap<NodeId, Option<String>>,
    channels: HashMap<NodeId, (String, ChannelKey)>,
    _reserved: (),
}

#[derive(Clone, Debug)]
pub(crate) struct PeerEntry {
    pub info: NodeInfo,
    pub last_announce_ms: u64,
    pub announce_seen: bool,
}

impl Engine {
    /// Create an engine for the given identity.
    pub fn new(identity: Identity, config: EngineConfig) -> Self {
        Self {
            identity,
            config,
            now_ms: 0,
            seq: 0,
            peers: HashMap::new(),
            contacts: HashMap::new(),
            channels: HashMap::new(),
            _reserved: (),
        }
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn node_id(&self) -> NodeId {
        self.identity.node_id()
    }

    pub fn public_identity(&self) -> PublicIdentity {
        self.identity.public()
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Replace configuration (e.g. role switch from the UI). Identity is preserved.
    pub fn set_config(&mut self, config: EngineConfig) {
        self.config = config;
    }

    // -----------------------------------------------------------------------------------------
    // Input path
    // -----------------------------------------------------------------------------------------

    /// Parse and process a raw transport payload.
    pub fn ingest(&mut self, bytes: &[u8], link: Option<LinkInfo>, now_ms: u64) -> Processed {
        let _ = (bytes, link, now_ms);
        todo!("core: decode frame then delegate to on_frame; record malformed frames as dropped")
    }

    /// Process an already-decoded frame.
    ///
    /// Responsibilities: deduplicate, learn about the sender, decrypt our own traffic, deliver
    /// events, generate acknowledgements, and relay/flood when the TTL allows.
    pub fn on_frame(&mut self, frame: Frame, link: Option<LinkInfo>, now_ms: u64) -> Processed {
        let _ = (frame, link, now_ms);
        todo!("core: see docs/architecture/PROTOCOL.md §Processing rules")
    }

    // -----------------------------------------------------------------------------------------
    // Application actions
    // -----------------------------------------------------------------------------------------

    /// Build and emit a signed announce.
    pub fn announce(&mut self, now_ms: u64) -> Result<Processed> {
        let _ = now_ms;
        todo!("core: build Announce, sign, emit broadcast frame")
    }

    /// Send a plain text message (single frame or streamed as a long-text resource).
    pub fn send_text(&mut self, target: Target, text: &str, now_ms: u64) -> Result<Processed> {
        let _ = (target, text, now_ms);
        todo!("core: build AppMessage, seal for target/channel, fragment if needed")
    }

    /// Send an arbitrary application message.
    pub fn send_message(
        &mut self,
        target: Target,
        message: AppMessage,
        now_ms: u64,
    ) -> Result<Processed> {
        let _ = (target, message, now_ms);
        todo!("core: seal AppMessage, emit frame; long payload becomes resource")
    }

    /// Broadcast/`target` a position report.
    pub fn send_position(
        &mut self,
        target: Target,
        position: Position,
        now_ms: u64,
    ) -> Result<Processed> {
        let _ = (target, position, now_ms);
        todo!("core: wrap position in AppMessage and send")
    }

    /// Emit telemetry about this node.
    pub fn send_telemetry(&mut self, telemetry: Telemetry, now_ms: u64) -> Result<Processed> {
        let _ = (telemetry, now_ms);
        todo!("core: wrap telemetry in an AppMessage addressed to broadcast")
    }

    /// Queue a resource transfer (attachment / long text) to a direct peer or channel.
    pub fn send_resource(
        &mut self,
        target: Target,
        msg_id: u32,
        res_id: u32,
        kind: ResourceKind,
        mime: &str,
        data: &[u8],
        now_ms: u64,
    ) -> Result<Processed> {
        let _ = (target, msg_id, res_id, kind, mime, data, now_ms);
        todo!("core: build manifest, store encrypted blob, emit initial burst")
    }

    /// Send an acknowledgement frame.
    pub fn send_ack(&mut self, to: NodeId, ack: Ack, now_ms: u64) -> Result<Processed> {
        let _ = (to, ack, now_ms);
        todo!("core: seal Ack and emit direct frame")
    }

    // -----------------------------------------------------------------------------------------
    // Bookkeeping
    // -----------------------------------------------------------------------------------------

    pub fn add_contact(&mut self, ident: PublicIdentity, name: Option<String>) {
        self.contacts.insert(ident.node_id, name);
        let entry = self.peers.entry(ident.node_id).or_insert_with(|| PeerEntry {
            info: NodeInfo {
                node_id: ident.node_id,
                ident,
                name: ident.node_id.to_hex(),
                class: NodeClass::Client,
                caps: 0,
                hops: 0,
                last_seen_ms: 0,
                rssi: None,
                snr: None,
                last_position: None,
                battery_pct: None,
                is_contact: true,
            },
            last_announce_ms: 0,
            announce_seen: false,
        });
        entry.info.is_contact = true;
    }

    pub fn remove_contact(&mut self, node: NodeId) {
        self.contacts.remove(&node);
        if let Some(entry) = self.peers.get_mut(&node) {
            entry.info.is_contact = false;
        }
    }

    /// Add a channel from a pre-shared secret; returns the channel address.
    pub fn add_channel(&mut self, name: &str, psk: &[u8; 32]) -> NodeId {
        let key = ChannelKey::from_psk(psk, name);
        let id = key.id();
        self.channels.insert(id, (name.to_string(), key));
        id
    }

    pub fn remove_channel(&mut self, id: NodeId) {
        self.channels.remove(&id);
    }

    pub fn channels(&self) -> Vec<(NodeId, String)> {
        self.channels.iter().map(|(id, (name, _))| (*id, name.clone())).collect()
    }

    pub fn known_nodes(&self) -> Vec<NodeInfo> {
        self.peers.values().map(|p| p.info.clone()).collect()
    }

    pub fn node_info(&self, id: NodeId) -> Option<NodeInfo> {
        self.peers.get(&id).map(|p| p.info.clone())
    }

    pub fn stats(&self) -> Stats {
        Stats {
            stored_envelopes: 0,
            known_nodes: self.peers.len(),
            channels: self.channels.len(),
            ..Default::default()
        }
    }

    // -----------------------------------------------------------------------------------------
    // Timers & persistence
    // -----------------------------------------------------------------------------------------

    /// Drive periodic work: re-announces, retransmissions, store-and-forward offers, pruning.
    pub fn tick(&mut self, now_ms: u64) -> Processed {
        let _ = now_ms;
        todo!("core: timers for announce, resource ARQ, store offers, peer aging")
    }

    /// Export persistable state (contacts, channels, counters).
    pub fn export_state(&self) -> StateSnapshot {
        let _ = ();
        todo!("core: snapshot contacts/channels/last_seq")
    }

    /// Restore state previously produced by [`Engine::export_state`].
    pub fn import_state(&mut self, state: StateSnapshot) {
        let _ = state;
        todo!("core: restore contacts/channels/last_seq")
    }
}

/// Convenience: construct an engine with an identity derived from a seed.
pub fn engine_from_seed(seed: &[u8; 32], config: EngineConfig) -> Engine {
    Engine::new(Identity::from_seed(seed), config)
}

/// Convenience: build a `StoreWant` helper (used by the engine and tests).
pub fn store_want(ids: Vec<u32>) -> StoreWant {
    StoreWant { ids }
}

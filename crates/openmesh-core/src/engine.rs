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
//! The engine never performs IO, blocking or wall-clock reads; the embedder supplies
//! `now_ms` and delivers [`Outbound`] frames to interfaces. This makes the full protocol
//! trivially testable and lets the simulator run thousands of virtual nodes in one process.
//!
//! # Wire invariants implemented here
//!
//! * Frame AEAD associated data is [`Frame::aad`]: the header with the TTL nibble cleared, so
//!   flooding relays can decrement TTL without breaking the tag.
//! * Sealed bodies are `payload_flags(1) || msg_ref(4 BE) || [identity(72)] || nonce_prefix(12) ||
//!   ct || tag` (PROTOCOL.md §5). `msg_ref` is the only clear-text application metadata and is
//!   what store-and-forward relays index envelopes by.
//! * Announces use the compact binary layout of [`Announce::encode_wire`] and are never stored.
//! * `StoreOffer`/`StoreWant` bodies are **clear-text CBOR** (no AEAD, no `msg_ref` header): a
//!   relay may not have the addressee's key material, and `msg_ref` is already clear in the
//!   stored envelopes it refers to.
//! * Store-and-forward relays keep original frame bytes and replay them verbatim with the TTL
//!   reset to [`DEFAULT_TTL`]; the addressee deduplicates by `(src, msg_ref)`.

use std::collections::{HashMap, HashSet};

use sha2::{Digest, Sha256};

use crate::crypto::{ChannelKey, Identity, PublicIdentity, SEAL_OVERHEAD};
use crate::error::{Error, Result};
use crate::resource::{
    DEFAULT_CHUNK_BYTES, MAX_CHUNK_BYTES, MAX_RETRIES as MAX_RESOURCE_RETRIES, ResourceReceiver,
    ResourceSender, ack_bitmap, chunk_count, resource_aad,
};
use crate::routing::DedupCache;
use crate::services::{
    Ack, AckKind, Announce, AppMessage, MessageKind, NodeClass, Position, ResourceKind,
    ResourceManifest, StoreOffer, StoreWant, Telemetry, announce_signature_payload, cbor_bytes,
    cbor_from_bytes, group_signature_payload,
};
use crate::store::{StoreQueue, StoredEnvelope};
use crate::wire::{Flags, Frame, FrameType, MAX_BODY, NodeId};

// ---------------------------------------------------------------------------------------------
// Timing and sizing constants (PROTOCOL.md §12)
// ---------------------------------------------------------------------------------------------

/// Client re-announce interval.
pub const ANNOUNCE_INTERVAL_CLIENT_MS: u64 = 1_800_000;
/// Repeater/gateway re-announce interval.
pub const ANNOUNCE_INTERVAL_RELAY_MS: u64 = 3_600_000;
/// Minimum spacing between announces.
pub const ANNOUNCE_MIN_INTERVAL_MS: u64 = 300_000;
/// Duplicate-suppression window.
pub const DEDUP_TTL_MS: u64 = 1_800_000;
/// Duplicate-suppression cache capacity (entries).
pub const DEDUP_CAPACITY: usize = 4096;
/// Retry delay for unacknowledged direct messages.
pub const ACK_TIMEOUT_MS: u64 = 8_000;
/// Maximum number of retransmissions of an unacknowledged message.
pub const MAX_MSG_RETRIES: u8 = 3;
/// Resource selective-repeat window.
///
/// Must stay <= 32: the receiver's window ack carries one `u32` bitmap. A small window (the
/// original 8) makes progress gated by every ack round-trip; under duty-cycle pacing one
/// round-trip can take tens of seconds, so the sender now streams up to this many chunks before
/// it needs an ack.
pub const RESOURCE_WINDOW: u16 = 24;
/// Base resource retransmission timeout.
pub const RESOURCE_RTO_MS: u64 = 3_000;
/// Give up on an outgoing resource after this long without acknowledged chunk progress.
///
/// Under duty-cycle pacing a single window can take tens of seconds of wall time before its ack
/// arrives; the old fixed "6 retries x 3 s" budget abandoned such transfers before the first ack.
pub const RESOURCE_RETRY_BUDGET_MS: u64 = 900_000;
/// Cap for the exponential retry backoff, so a lost link is probed occasionally without flooding.
pub const RESOURCE_MAX_RETRY_BACKOFF_MS: u64 = 60_000;
/// Chunks released per engine tick / ack. A duty-cycle-paced companion TX path drops a whole
/// window emitted in one burst, so chunks leave the engine a couple at a time instead.
pub const RESOURCE_TX_BATCH: u16 = 2;
/// Minimum wall time between paced chunk releases. Engine tick rates differ per embedder (the
/// bridge ticks at 1 Hz, the Android companion at 4 Hz); pacing by time keeps the offered frame
/// rate at ~2 chunks/s everywhere so a duty-cycle-paced radio's outbox/companion FIFO is not
/// overrun by a fast-ticking sender.
pub const RESOURCE_PACE_INTERVAL_MS: u64 = 1_000;
/// Maximum resource retransmissions between progress (hard safety ceiling; the time budget is the
/// primary give-up condition).
pub const RESOURCE_MAX_RETRIES: u8 = MAX_RESOURCE_RETRIES;
/// How long a completed resource stays cached to answer duplicate acks.
pub const RESOURCE_KEEP_MS: u64 = 600_000;
/// Minimum spacing between store offers to one peer.
pub const STORE_OFFER_INTERVAL_MS: u64 = 300_000;
/// Default store-and-forward retention.
pub const STORE_TTL_MS: u64 = 259_200_000;
/// Node bookkeeping expiry.
pub const PEER_EXPIRY_MS: u64 = 86_400_000;
/// Default hop budget for originated client frames.
pub const DEFAULT_TTL: u8 = 5;
/// Hop budget for announces.
pub const ANNOUNCE_TTL: u8 = 8;
/// Origination rate limit per minute (PROTOCOL.md §11).
pub const MAX_ORIGINATE_PER_MIN: u32 = 60;
/// Maximum number of ids in one store offer.
pub const MAX_STORE_OFFER_IDS: usize = 16;
/// Application-level `(src, msg_id)` dedup memory (24 h, as recommended by §17).
pub const SEEN_MSG_TTL_MS: u64 = 86_400_000;
/// Current message-id derivation epoch.
///
/// Message ids are `SHA-256("openmesh/v1/msg-id" || epoch || seed || counter)[0..4]`; bumping the
/// epoch moves the id space so a snapshot that never persisted its counter (legacy) cannot collide
/// with ids derived by the current engine. This is not a wire change: `msg_id` remains an opaque u32.
pub const MSG_ID_EPOCH: u32 = 1;
/// Counter floor applied when importing a legacy snapshot without a persisted message-id counter.
///
/// Old engines derived ids from counters `1..N`; starting fresh imports at a high counter value
/// (combined with the epoch) guarantees the new ids cannot repeat any previously issued id.
pub const LEGACY_MSG_COUNTER_START: u32 = 0x8000_0000;
/// Capacity of the application-level dedup map.
pub const SEEN_MSG_CAPACITY: usize = 4096;
/// Default resource chunk size.
pub const CHUNK_BYTES: u16 = DEFAULT_CHUNK_BYTES;
/// Hard receiver cap on a resource blob (memory protection; the size policy is enforced by the
/// embedder using [`crate::util::max_attachment_bytes`]).
pub const MAX_RESOURCE_BYTES: u32 = 256 * 1024;
/// How recently a destination must have been heard to be considered reachable (a relay then
/// does not need to store envelopes for it).
pub const STORE_PEER_REACHABLE_MS: u64 = 300_000;
/// Minimum interval between duplicate delivery acks for the same message id.
pub const RE_ACK_MIN_INTERVAL_MS: u64 = 2_000;
/// A receiver acknowledges a partial window after this much quiet time ("end of burst").
pub const RESOURCE_ACK_DELAY_MS: u64 = 5_000;
/// Receiver sends a window ack after this many new chunks.
pub const RESOURCE_ACK_EVERY: u16 = 8;
/// Maximum lifetime of an outgoing resource before it is abandoned.
pub const RESOURCE_TRANSFER_TIMEOUT_MS: u64 = 2 * RESOURCE_KEEP_MS;

/// `payload_flags` bit 0: the body carries the sender's 72-byte public identity.
pub const PAYLOAD_IDENTITY_INCLUDED: u8 = 0x01;
/// `payload_flags` bit 1: the plaintext is a CBOR [`Ack`].
pub const PAYLOAD_IS_ACK: u8 = 0x02;
/// `payload_flags` bit 2: the plaintext is a CBOR [`ResourceManifest`].
pub const PAYLOAD_IS_MANIFEST: u8 = 0x04;
/// Mask of defined `payload_flags` bits; anything else is a protocol error.
pub const PAYLOAD_FLAGS_MASK: u8 = 0x07;

/// Opaque identifier for a local transport interface (USB, BLE, UDP socket, simulator link, ...).
pub type LinkId = u16;

/// Key of the completed-resource cache.
type ResourceKey = (NodeId, u32);
/// Cache entry for a completed incoming resource: `(completed_at_ms, manifest)`.
type CompletedResource = (u64, ResourceManifest);

/// Metadata about the interface a frame arrived on.
#[derive(Clone, Debug, Default)]
pub struct LinkInfo {
    /// Embedder link identifier.
    pub link: LinkId,
    /// Peer node id when the link is point-to-point (USB/BLE/TCP). `None` for broadcast media.
    pub peer: Option<NodeId>,
    /// Received signal strength in dBm, when known.
    pub rssi: Option<i16>,
    /// Signal-to-noise ratio in dB, when known.
    pub snr: Option<f32>,
}

/// What a node does for the mesh.
#[derive(Clone, Debug, PartialEq)]
pub enum Role {
    /// End-user node: sends and receives its own traffic, forwards frames, never stores for
    /// other nodes.
    Client,
    /// Infrastructure node: floods frames and may store envelopes for offline peers.
    Relay(RelayConfig),
}

/// Store-and-forward configuration for [`Role::Relay`].
#[derive(Clone, Debug, PartialEq)]
pub struct RelayConfig {
    /// Maximum number of logical envelopes retained for other nodes.
    pub store_capacity: usize,
    /// How long an envelope is retained before eviction.
    pub store_ttl_ms: u64,
    /// Flood budget: maximum frames this relay injects per minute.
    pub max_frames_per_min: u32,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            store_capacity: 64,
            store_ttl_ms: STORE_TTL_MS,
            max_frames_per_min: 120,
        }
    }
}

/// Engine configuration: the identity is passed separately to [`Engine::new`].
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// Advertised role class.
    pub class: NodeClass,
    /// Display name used in announces (truncated to 32 bytes).
    pub name: String,
    /// Capability bitflags advertised in announces.
    pub caps: u16,
    /// How often the node re-announces itself.
    pub announce_interval_ms: u64,
    /// Default hop budget for originated frames.
    pub default_ttl: u8,
    /// What the node does for the mesh.
    pub role: Role,
    /// Whether to accept and decrypt group (channel) traffic.
    pub accept_channels: bool,
    /// Regulatory override (duty cycle / TX power / frequency). Defaults to disabled; the engine
    /// only exposes the effective duty-cycle policy, radio power and frequency are applied by
    /// the embedder (docs/design/regulatory-and-regions.md §2).
    pub regulatory: crate::regulatory::RegulatoryOverride,
}

impl EngineConfig {
    /// Configuration for an end-user client node.
    pub fn client(name: impl Into<String>) -> Self {
        Self {
            class: NodeClass::Client,
            name: name.into(),
            caps: crate::services::caps::RELAY,
            announce_interval_ms: ANNOUNCE_INTERVAL_CLIENT_MS,
            default_ttl: DEFAULT_TTL,
            role: Role::Client,
            accept_channels: true,
            regulatory: crate::regulatory::RegulatoryOverride::default(),
        }
    }

    /// Configuration for a store-and-forward repeater.
    pub fn repeater(name: impl Into<String>) -> Self {
        Self {
            class: NodeClass::Repeater,
            name: name.into(),
            caps: crate::services::caps::RELAY | crate::services::caps::STORE,
            announce_interval_ms: ANNOUNCE_INTERVAL_RELAY_MS,
            default_ttl: DEFAULT_TTL,
            role: Role::Relay(RelayConfig::default()),
            accept_channels: false,
            regulatory: crate::regulatory::RegulatoryOverride::default(),
        }
    }

    /// Effective duty-cycle budget in percent for `region`, honouring the configured regulatory
    /// override (`None` for regions with only fair-use rules).
    ///
    /// This is the value the embedder's airtime budget should use; it does not affect the wire
    /// protocol.
    pub fn effective_duty_pct(&self, region: &crate::util::Region) -> Option<u8> {
        crate::regulatory::effective_duty_pct(region, &self.regulatory)
    }
}

/// Destination of an application message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// End-to-end encrypted direct message to one node.
    Direct(NodeId),
    /// Symmetric channel message (group chat, position to a status channel, ...).
    Channel(NodeId),
}

impl Target {
    /// The on-air destination address.
    pub fn address(self) -> NodeId {
        match self {
            Self::Direct(id) | Self::Channel(id) => id,
        }
    }
}

/// How the embedder should transmit an outbound frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendMode {
    /// Broadcast on all interfaces.
    Flood,
    /// Point-to-point if the neighbor link exists, otherwise fall back to flooding.
    Direct(NodeId),
}

/// A frame the embedder should transmit.
#[derive(Clone, Debug)]
pub struct Outbound {
    /// Frame to put on the air (encode with [`Frame::encode`]).
    pub frame: Frame,
    /// Preferred transmission mode.
    pub mode: SendMode,
    /// Preferred interface when the frame is a link-bound reply.
    pub link: Option<LinkId>,
}

/// Result of processing: events for the application plus frames to transmit.
#[derive(Clone, Debug, Default)]
pub struct Processed {
    /// Events the application should surface.
    pub events: Vec<Event>,
    /// Frames the embedder should transmit.
    pub outbound: Vec<Outbound>,
}

impl Processed {
    /// Whether there is nothing to do.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty() && self.outbound.is_empty()
    }

    /// Absorb another processing result into this one (events then outbound).
    pub fn merge(&mut self, other: Processed) {
        self.events.extend(other.events);
        self.outbound.extend(other.outbound);
    }

    /// Add an event.
    pub fn with_event(mut self, event: Event) -> Self {
        self.events.push(event);
        self
    }

    /// Add an outbound frame.
    pub fn with_frame(mut self, frame: Frame, mode: SendMode) -> Self {
        self.outbound.push(Outbound {
            frame,
            mode,
            link: None,
        });
        self
    }

    /// A result containing only one event.
    pub fn event(event: Event) -> Self {
        Self {
            events: vec![event],
            outbound: Vec::new(),
        }
    }

    /// A result containing only one frame.
    pub fn frame(frame: Frame, mode: SendMode) -> Self {
        Self {
            events: Vec::new(),
            outbound: vec![Outbound {
                frame,
                mode,
                link: None,
            }],
        }
    }
}

/// A direct message that was received and decrypted.
#[derive(Clone, Debug)]
pub struct ReceivedMessage {
    /// Sender node id (authenticated by the AEAD key and, when present, by the identity hint).
    pub from: NodeId,
    /// Decrypted application message.
    pub message: AppMessage,
    /// Link metadata of the frame that carried it.
    pub link: LinkInfo,
    /// True when the envelope was replayed by a relay store. v1 has no on-wire marker for this
    /// (store replay is signalled out-of-band by the offer/want handshake); always `false`.
    pub via_store: bool,
}

/// A channel/group message that was received, decrypted and signature-verified.
#[derive(Clone, Debug)]
pub struct ReceivedGroupMessage {
    /// Channel address the message was sent to.
    pub channel: NodeId,
    /// Sender node id (verified through the Ed25519 signature over the group payload).
    pub sender: NodeId,
    /// Decrypted application message.
    pub message: AppMessage,
    /// Link metadata of the frame that carried it.
    pub link: LinkInfo,
}

/// Something the application should know about.
#[derive(Clone, Debug)]
pub enum Event {
    /// A direct message was received and decrypted.
    Message(ReceivedMessage),
    /// A channel/group message was received, decrypted and signature-verified.
    GroupMessage(ReceivedGroupMessage),
    /// An acknowledgement for one of our direct messages arrived.
    Ack {
        /// Peer that sent the acknowledgement.
        from: NodeId,
        /// Acknowledgement body.
        ack: Ack,
    },
    /// A position report was received (direct or channel). Also emitted alongside the
    /// corresponding [`Event::Message`]/[`Event::GroupMessage`] for convenience.
    Position {
        /// Reporting node.
        from: NodeId,
        /// Channel address when the report came over a channel.
        channel: Option<NodeId>,
        /// Position fix.
        position: Position,
    },
    /// An infrastructure node reported telemetry.
    Telemetry {
        /// Reporting node.
        from: NodeId,
        /// Telemetry sample.
        telemetry: Telemetry,
    },
    /// A node was discovered or its announce changed.
    NodeSeen(NodeInfo),
    /// A relay holds envelopes addressed to us.
    StoreOffer {
        /// Relay node id.
        from: NodeId,
        /// Clear-text `msg_ref`s offered.
        ids: Vec<u32>,
    },
    /// A relay accepted an envelope for later delivery. v1 has no on-wire confirmation for a
    /// relay's store; reserved for future use.
    StoreAccepted {
        /// Message id.
        msg_id: u32,
        /// Relay that accepted it.
        relay: NodeId,
    },
    /// Progress of an incoming or outgoing resource transfer.
    ResourceProgress {
        /// Peer the transfer is with.
        peer: NodeId,
        /// Resource id.
        res_id: u32,
        /// Chunks received (incoming) or acknowledged (outgoing).
        received: u16,
        /// Total chunks.
        total: u16,
    },
    /// A resource transfer completed, verified and decrypted.
    ResourceReceived {
        /// Sender node id.
        peer: NodeId,
        /// Channel address when the resource was addressed to a channel.
        channel: Option<NodeId>,
        /// Manifest that described the transfer.
        manifest: ResourceManifest,
        /// Plaintext payload (attachment bytes, or CBOR for long text / history).
        data: Vec<u8>,
    },
    /// One of our outgoing resource transfers was fully acknowledged.
    ResourceSent {
        /// Peer that completed the transfer.
        peer: NodeId,
        /// Resource id.
        res_id: u32,
    },
    /// Non-fatal protocol problem worth surfacing in logs/UI.
    Error(String),
}

/// Live view of a known node.
#[derive(Clone, Debug)]
pub struct NodeInfo {
    /// Node address.
    pub node_id: NodeId,
    /// Full public identity (self-certified against `node_id`).
    pub ident: PublicIdentity,
    /// Display name from the last announce (or a hex id before one is heard).
    pub name: String,
    /// Advertised role class.
    pub class: NodeClass,
    /// Advertised capability bitflags.
    pub caps: u16,
    /// Hop count of the last announce (`ANNOUNCE_TTL - received_ttl`).
    pub hops: u8,
    /// Last time this node was heard, in embedder time.
    pub last_seen_ms: u64,
    /// Last observed RSSI in dBm.
    pub rssi: Option<i16>,
    /// Last observed SNR in dB.
    pub snr: Option<f32>,
    /// Last position reported by this node.
    pub last_position: Option<Position>,
    /// Last battery percentage reported via telemetry.
    pub battery_pct: Option<u8>,
    /// Whether the user marked this node as a contact.
    pub is_contact: bool,
}

/// Engine counters for the bridge status panel and the simulator.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    /// Frames emitted for transmission.
    pub tx_frames: u64,
    /// Frames received and processed.
    pub rx_frames: u64,
    /// Frames re-broadcast for other nodes.
    pub forwarded_frames: u64,
    /// Frames dropped (malformed, invalid signature, unknown sender, exhausted TTL, ...).
    pub dropped_frames: u64,
    /// Duplicate frames suppressed by the dedup cache.
    pub dedup_hits: u64,
    /// ACK frames decrypted and accepted for processing.
    pub acks_rx: u64,
    /// Accepted `resource_window` ACKs (the media flow-control acknowledgements).
    pub window_acks_rx: u64,
    /// Logical envelopes currently held in the store queue.
    pub stored_envelopes: usize,
    /// Known nodes in the peer book.
    pub known_nodes: usize,
    /// Joined channels.
    pub channels: usize,
    /// Best-effort on-air time of frames emitted by this engine (at the default profile).
    pub airtime_tx_ms: u64,
}

/// Serializable subset of engine state, persisted by the embedder.
#[derive(Clone, Debug, Default)]
pub struct StateSnapshot {
    /// Contacts with their user-assigned labels.
    pub contacts: Vec<(PublicIdentity, Option<String>)>,
    /// Joined channels as `(channel_id, name, psk)`.
    pub channels: Vec<(NodeId, String, [u8; 32])>,
    /// Last transmitted per-sender sequence number.
    pub last_seq: u32,
    /// Monotonic message-id allocation counter (persisted so a restart never reuses an id).
    ///
    /// `0` means "legacy snapshot written before the counter was persisted"; import applies
    /// [`LEGACY_MSG_COUNTER_START`] so ids from old app versions cannot be re-derived.
    pub msg_counter: u32,
    /// Message-id derivation epoch. Snapshots from older app versions default to `0` (legacy);
    /// current engines use [`MSG_ID_EPOCH`]. Bumping the epoch changes the derived id space without
    /// changing the wire format (`msg_id` stays an opaque u32).
    pub msg_id_epoch: u32,
}

/// The mesh protocol state machine.
pub struct Engine {
    identity: Identity,
    config: EngineConfig,
    now_ms: u64,
    seq: u32,
    msg_counter: u32,
    /// Message-id derivation epoch (see [`MSG_ID_EPOCH`]).
    msg_id_epoch: u32,
    res_counter: u32,
    peers: HashMap<NodeId, PeerEntry>,
    contacts: HashMap<NodeId, Option<String>>,
    channels: HashMap<NodeId, ChannelEntry>,
    dedup: DedupCache,
    stats: Stats,
    heard_at: HashMap<NodeId, u64>,
    peer_knows_us: HashSet<NodeId>,
    ack_sent_at: HashMap<(NodeId, u32), u64>,
    pending_msgs: HashMap<u32, PendingMessage>,
    outgoing: HashMap<u32, OutgoingResource>,
    incoming: HashMap<ResourceKey, IncomingResource>,
    completed: HashMap<ResourceKey, CompletedResource>,
    seen_msgs: HashMap<(NodeId, u32), u64>,
    seen_refs: HashMap<u32, u64>,
    store: StoreQueue,
    store_offer_at: HashMap<NodeId, u64>,
    last_announce_ms: u64,
    orig_window_start_ms: u64,
    orig_count: u32,
    /// Wall-clock interval between paced resource chunk batches (duty-aware hint).
    resource_pace_interval_ms: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct PeerEntry {
    pub(crate) info: NodeInfo,
    pub(crate) last_announce_ms: u64,
    pub(crate) announce_seen: bool,
}

#[derive(Clone, Debug)]
struct ChannelEntry {
    name: String,
    key: ChannelKey,
    psk: [u8; 32],
}

#[derive(Clone, Debug)]
struct PendingMessage {
    peer: PublicIdentity,
    plaintext: Vec<u8>,
    include_identity: bool,
    attempts: u8,
    last_tx_ms: u64,
}

#[derive(Clone, Debug)]
enum TransferKey {
    Peer(PublicIdentity),
    Channel { id: NodeId, key: ChannelKey },
}

impl TransferKey {
    fn address(&self) -> NodeId {
        match self {
            Self::Peer(ident) => ident.node_id,
            Self::Channel { id, .. } => *id,
        }
    }

    fn peer(&self) -> Option<PublicIdentity> {
        match self {
            Self::Peer(ident) => Some(*ident),
            Self::Channel { .. } => None,
        }
    }
}

#[derive(Debug)]
struct OutgoingResource {
    msg_id: u32,
    target: Target,
    key: TransferKey,
    manifest: ResourceManifest,
    manifest_plaintext: Vec<u8>,
    blob: Vec<u8>,
    sender: ResourceSender,
    started_ms: u64,
    /// Wall time of the last paced chunk release (rate-limits `RESOURCE_TX_BATCH` per interval).
    paced_ms: u64,
}

#[derive(Clone, Debug)]
struct OutgoingSnapshot {
    msg_id: u32,
    target: Target,
    key: TransferKey,
    manifest_plaintext: Vec<u8>,
    pending: Vec<u16>,
    include_identity: bool,
    started_ms: u64,
    deadline_ms: u64,
    /// Wall time of the last paced emission (normal or retry), used to keep retries paced too.
    paced_ms: u64,
    /// Acknowledged chunks when the snapshot was taken (manifest repair policy).
    acked_count: u16,
    /// Retries since the last acknowledged progress (manifest repair cadence).
    retries: u8,
}

#[derive(Debug)]
struct IncomingResource {
    peer: NodeId,
    channel: Option<NodeId>,
    manifest: ResourceManifest,
    receiver: ResourceReceiver,
    key: TransferKey,
    dirty: bool,
    last_ack_ms: u64,
    progress_reported: u16,
    chunks_since_ack: u16,
}

// ---------------------------------------------------------------------------------------------
// Body layout helpers (PROTOCOL.md §5, §9.2)
// ---------------------------------------------------------------------------------------------

/// Build a sealed body: `payload_flags || msg_ref || [identity] || nonce_prefix || ct || tag`.
pub fn sealed_body(
    payload_flags: u8,
    msg_ref: u32,
    identity: Option<&PublicIdentity>,
    sealed: &[u8],
) -> Vec<u8> {
    let mut body = Vec::with_capacity(5 + identity.map_or(0, |_| 72) + sealed.len());
    body.push(payload_flags);
    body.extend_from_slice(&msg_ref.to_be_bytes());
    if let Some(ident) = identity {
        body.extend_from_slice(&ident.to_bytes());
    }
    body.extend_from_slice(sealed);
    body
}

/// Split a sealed body into `(payload_flags, msg_ref, identity_hint, sealed)`.
///
/// Validates the payload flag bits and the identity hint length; the sealed remainder still
/// needs AEAD verification by the caller.
pub fn parse_sealed_body(body: &[u8]) -> Result<(u8, u32, Option<PublicIdentity>, &[u8])> {
    if body.len() < 5 {
        return Err(Error::BufferTooSmall {
            need: 5,
            have: body.len(),
        });
    }
    let payload_flags = body[0];
    if payload_flags & !PAYLOAD_FLAGS_MASK != 0 {
        return Err(Error::Malformed("unknown payload flags"));
    }
    let msg_ref = u32::from_be_bytes([body[1], body[2], body[3], body[4]]);
    let mut rest = &body[5..];
    let mut identity = None;
    if payload_flags & PAYLOAD_IDENTITY_INCLUDED != 0 {
        const LEN: usize = crate::crypto::PUBLIC_IDENTITY_LEN;
        if rest.len() < LEN {
            return Err(Error::BufferTooSmall {
                need: 5 + LEN,
                have: body.len(),
            });
        }
        identity = Some(PublicIdentity::from_bytes(&rest[..LEN])?);
        rest = &rest[LEN..];
    }
    Ok((payload_flags, msg_ref, identity, rest))
}

/// Build a `Resource` frame body: `flags(0) || msg_ref || res_id || chunk_index || slice`.
pub fn chunk_body(msg_ref: u32, res_id: u32, index: u16, slice: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(11 + slice.len());
    body.push(0);
    body.extend_from_slice(&msg_ref.to_be_bytes());
    body.extend_from_slice(&res_id.to_be_bytes());
    body.extend_from_slice(&index.to_be_bytes());
    body.extend_from_slice(slice);
    body
}

/// Parse a `Resource` frame body into `(msg_ref, res_id, chunk_index, slice)`.
pub fn parse_chunk_body(body: &[u8]) -> Result<(u32, u32, u16, &[u8])> {
    if body.len() < 11 {
        return Err(Error::BufferTooSmall {
            need: 11,
            have: body.len(),
        });
    }
    if body[0] != 0 {
        return Err(Error::Malformed("resource payload_flags must be zero"));
    }
    let msg_ref = u32::from_be_bytes([body[1], body[2], body[3], body[4]]);
    let res_id = u32::from_be_bytes([body[5], body[6], body[7], body[8]]);
    let index = u16::from_be_bytes([body[9], body[10]]);
    Ok((msg_ref, res_id, index, &body[11..]))
}

/// Origination hop budget for a frame type (announces start at [`ANNOUNCE_TTL`], everything
/// else at [`DEFAULT_TTL`]).
pub fn origin_ttl(typ: FrameType) -> u8 {
    if typ == FrameType::Announce {
        ANNOUNCE_TTL
    } else {
        DEFAULT_TTL
    }
}

/// Truncate a UTF-8 string to at most `max` bytes without splitting a character.
fn truncate_utf8(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

impl Engine {
    /// Create an engine for the given identity.
    pub fn new(identity: Identity, config: EngineConfig) -> Self {
        let (capacity, ttl) = match &config.role {
            Role::Relay(relay) => (relay.store_capacity, relay.store_ttl_ms),
            Role::Client => (0, STORE_TTL_MS),
        };
        Self {
            identity,
            config,
            now_ms: 0,
            seq: 0,
            msg_counter: 0,
            msg_id_epoch: MSG_ID_EPOCH,
            res_counter: 0,
            peers: HashMap::new(),
            contacts: HashMap::new(),
            channels: HashMap::new(),
            dedup: DedupCache::new(DEDUP_CAPACITY, DEDUP_TTL_MS),
            stats: Stats::default(),
            heard_at: HashMap::new(),
            peer_knows_us: HashSet::new(),
            ack_sent_at: HashMap::new(),
            pending_msgs: HashMap::new(),
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
            completed: HashMap::new(),
            seen_msgs: HashMap::new(),
            seen_refs: HashMap::new(),
            store: StoreQueue::new(capacity, ttl),
            store_offer_at: HashMap::new(),
            // `u64::MAX` means "never announced": the first tick emits the startup announce.
            last_announce_ms: u64::MAX,
            orig_window_start_ms: 0,
            orig_count: 0,
            resource_pace_interval_ms: RESOURCE_PACE_INTERVAL_MS,
        }
    }

    /// The engine's secret identity.
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// This node's address.
    pub fn node_id(&self) -> NodeId {
        self.identity.node_id()
    }

    /// This node's full public identity.
    pub fn public_identity(&self) -> PublicIdentity {
        self.identity.public()
    }

    /// Current configuration.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Replace configuration (e.g. role switch from the UI). Identity is preserved.
    pub fn set_config(&mut self, config: EngineConfig) {
        let new_ttl = match &config.role {
            Role::Relay(relay) => relay.store_ttl_ms,
            Role::Client => STORE_TTL_MS,
        };
        let old_ttl = match &self.config.role {
            Role::Relay(relay) => relay.store_ttl_ms,
            Role::Client => STORE_TTL_MS,
        };
        let capacity = Self::store_capacity(&config.role);
        if self.config.role != config.role || old_ttl != new_ttl {
            self.store = StoreQueue::new(capacity, new_ttl);
        }
        self.config = config;
    }

    /// Wall-clock interval between paced resource chunk batches.
    ///
    /// Defaults to [`RESOURCE_PACE_INTERVAL_MS`]; embedders should set a duty-aware hint derived
    /// from the active LoRa profile and region duty cycle (see
    /// [`crate::util::resource_pace_interval_ms`]). The engine releases at most
    /// [`RESOURCE_TX_BATCH`] chunks per interval on both tick- and ack-driven paths.
    pub fn resource_pace_interval_ms(&self) -> u64 {
        self.resource_pace_interval_ms
    }

    /// Apply a duty-aware resource chunk pacing hint (clamped to at least 1 ms).
    pub fn set_resource_pace_interval_ms(&mut self, interval_ms: u64) {
        self.resource_pace_interval_ms = interval_ms.max(1);
    }

    fn store_capacity(role: &Role) -> usize {
        match role {
            Role::Relay(relay) => relay.store_capacity,
            Role::Client => 0,
        }
    }

    // -----------------------------------------------------------------------------------------
    // Input path
    // -----------------------------------------------------------------------------------------

    /// Parse and process a raw transport payload.
    ///
    /// Malformed frames are counted as dropped and never panic.
    pub fn ingest(&mut self, bytes: &[u8], link: Option<LinkInfo>, now_ms: u64) -> Processed {
        self.now_ms = now_ms;
        match Frame::decode(bytes) {
            Ok(frame) => {
                // Timestamped, per-frame debug line: correlates with firmware `txfail ... seq=` /
                // `acktx` diag entries and with the bridge's pre-engine `radio frame received` line.
                tracing::debug!(
                    bytes = bytes.len(),
                    kind = ?frame.typ,
                    src = %frame.src,
                    dst = %frame.dst,
                    seq = frame.seq,
                    ttl = frame.ttl,
                    "radio frame decoded"
                );
                self.on_frame(frame, link, now_ms)
            }
            Err(_) => {
                self.stats.dropped_frames += 1;
                tracing::debug!(
                    bytes = bytes.len(),
                    "dropped malformed radio frame (framing/MTU decode failed)"
                );
                Processed::default()
            }
        }
    }

    /// Process an already-decoded frame.
    ///
    /// Responsibilities: deduplicate, learn about the sender, decrypt our own traffic, deliver
    /// events, generate acknowledgements, store envelopes for other nodes when acting as a
    /// relay, and flood when the TTL allows.
    pub fn on_frame(&mut self, frame: Frame, link: Option<LinkInfo>, now_ms: u64) -> Processed {
        self.now_ms = now_ms;
        let mut out = Processed::default();
        let me = self.node_id();

        // Own frames echoed back by relays are normal in a flood mesh, and frames from the
        // reserved zero address are meaningless: ignore both without counting a protocol drop.
        if frame.src == me || frame.src.is_zero() {
            return out;
        }
        // Header flag bit 3 is reserved and MUST be zero (PROTOCOL.md §4.1, §16).
        if frame.flags.contains(Flags::RESERVED_BIT3) {
            self.stats.dropped_frames += 1;
            return out;
        }

        let to_me = frame.dst == me;
        let to_channel = self.config.accept_channels && self.channels.contains_key(&frame.dst);
        let broadcast = frame.is_broadcast();
        let for_us = to_me || to_channel || broadcast || frame.typ == FrameType::Announce;

        // Frames we can neither deliver nor forward are dropped (PROTOCOL.md §11).
        if !for_us && (frame.ttl == 0 || !frame.typ.is_floodable()) {
            self.stats.dropped_frames += 1;
            return out;
        }
        if frame.typ == FrameType::Admin && !to_me {
            self.stats.dropped_frames += 1;
            return out;
        }
        self.stats.rx_frames += 1;

        // Duplicate suppression by (src, seq).
        if !self.dedup.check_and_insert(frame.src, frame.seq, now_ms) {
            self.stats.dedup_hits += 1;
            tracing::debug!(
                src = %frame.src,
                seq = frame.seq,
                kind = ?frame.typ,
                dst = %frame.dst,
                "duplicate frame suppressed"
            );
            self.maybe_reack_duplicate(&frame, now_ms, &mut out);
            return out;
        }

        let direct_heard = self.note_heard(&frame, link.as_ref(), now_ms);
        self.learn_link_stats(&frame, link.as_ref(), now_ms);

        let mut allow_relay = true;
        match frame.typ {
            FrameType::Announce => {
                allow_relay = self.process_announce(&frame, link.as_ref(), now_ms, &mut out);
            }
            FrameType::Data => {
                if to_me {
                    self.process_direct_data(&frame, link.as_ref(), now_ms, &mut out);
                } else if to_channel {
                    self.process_channel_data(&frame, link.as_ref(), now_ms, &mut out);
                }
            }
            FrameType::Ack => {
                if to_me {
                    self.process_ack(&frame, now_ms, &mut out);
                }
            }
            FrameType::Resource => {
                if to_me || to_channel {
                    self.process_incoming_chunk(&frame, now_ms, &mut out);
                }
            }
            FrameType::StoreOffer => {
                if to_me {
                    self.process_store_offer(&frame, &mut out);
                }
            }
            FrameType::StoreWant => {
                if to_me {
                    self.process_store_want(&frame, &mut out);
                }
            }
            FrameType::Probe => {
                // An explicit probe addressed to us allows an immediate announce
                // (PROTOCOL.md §6.2 exception).
                if to_me {
                    if let Ok(announce) = self.announce(now_ms) {
                        out.merge(announce);
                    }
                }
            }
            FrameType::ProbeResponse
            | FrameType::RouteRequest
            | FrameType::RouteResponse
            | FrameType::Admin => {}
        }

        // Store-and-forward retention for absent addressees (relays only).
        if !to_me && !to_channel && !broadcast && frame.typ.is_storable() {
            self.maybe_store(&frame, now_ms);
        }

        // Managed flooding: rebroadcast with a decremented TTL, body and flags untouched.
        if allow_relay && frame.ttl > 0 && frame.typ.is_floodable() {
            if let Some(relay) = frame.relay_copy() {
                self.stats.forwarded_frames += 1;
                let mode = self.relay_mode(&frame);
                self.emit_frame(&mut out, relay, mode);
            }
        }

        // Store-and-forward: a relay that holds envelopes for a freshly heard addressee offers
        // them (rate-limited per peer).
        if direct_heard {
            self.maybe_offer_store(frame.src, now_ms, &mut out);
        }

        out
    }

    // -----------------------------------------------------------------------------------------
    // Announce
    // -----------------------------------------------------------------------------------------

    /// Build, sign and emit an announce (PROTOCOL.md §6).
    pub fn announce(&mut self, now_ms: u64) -> Result<Processed> {
        self.now_ms = now_ms;
        let announce = Announce {
            version: crate::wire::PROTO_VERSION,
            class: self.config.class,
            caps: self.config.caps,
            name: truncate_utf8(&self.config.name, crate::services::ANNOUNCE_NAME_MAX),
            hops: 0,
            timestamp_ms: now_ms,
            interval_s: (self.config.announce_interval_ms / 1000).min(u16::MAX as u64) as u16,
            ident: self.public_identity(),
        };
        let unsigned = announce.encode_wire()?;
        let seq = self.next_seq();
        let payload = announce_signature_payload(&self.node_id(), seq, &unsigned);
        let signature = self.identity.sign(&payload);
        let body = announce.encode_signed(&signature)?;
        let frame = Frame {
            typ: FrameType::Announce,
            flags: Flags::empty(),
            ttl: ANNOUNCE_TTL,
            src: self.node_id(),
            dst: crate::wire::BROADCAST,
            seq,
            body,
        };
        let mut out = Processed::default();
        self.emit_frame(&mut out, frame, SendMode::Flood);
        self.last_announce_ms = now_ms;
        Ok(out)
    }

    /// Verify and learn from a received announce. Returns whether the frame may be relayed.
    fn process_announce(
        &mut self,
        frame: &Frame,
        link: Option<&LinkInfo>,
        now_ms: u64,
        out: &mut Processed,
    ) -> bool {
        if !frame.src.is_unicast() {
            out.events
                .push(Event::Error("announce from non-unicast address".into()));
            return false;
        }
        let (announce, signature) = match Announce::decode_signed(&frame.body) {
            Ok(value) => value,
            Err(err) => {
                self.stats.dropped_frames += 1;
                out.events
                    .push(Event::Error(format!("bad announce body: {err}")));
                return false;
            }
        };
        if announce.version != crate::wire::PROTO_VERSION
            || !announce.ident.is_self_consistent()
            || announce.ident.node_id != frame.src
        {
            self.stats.dropped_frames += 1;
            out.events
                .push(Event::Error("announce identity mismatch".into()));
            return false;
        }
        let unsigned_len = frame.body.len() - crate::services::ANNOUNCE_SIG_LEN;
        let payload =
            announce_signature_payload(&frame.src, frame.seq, &frame.body[..unsigned_len]);
        if announce.ident.verify(&payload, &signature).is_err() {
            self.stats.dropped_frames += 1;
            out.events
                .push(Event::Error("bad announce signature".into()));
            return false;
        }
        let hops = ANNOUNCE_TTL.saturating_sub(frame.ttl.min(ANNOUNCE_TTL));
        let info = self.upsert_peer_from_announce(&announce, hops, link, now_ms);
        out.events.push(Event::NodeSeen(info));
        true
    }

    fn upsert_peer_from_announce(
        &mut self,
        announce: &Announce,
        hops: u8,
        link: Option<&LinkInfo>,
        now_ms: u64,
    ) -> NodeInfo {
        let ident = announce.ident;
        let node = ident.node_id;
        let is_contact = self.contacts.contains_key(&node);
        let (rssi, snr) = link.map(|l| (l.rssi, l.snr)).unwrap_or((None, None));
        let name = if announce.name.is_empty() {
            self.peers
                .get(&node)
                .map(|p| p.info.name.clone())
                .unwrap_or_else(|| node.to_hex())
        } else {
            announce.name.clone()
        };
        let entry = self.peers.entry(node).or_insert_with(|| PeerEntry {
            info: NodeInfo {
                node_id: node,
                ident,
                name: name.clone(),
                class: announce.class,
                caps: announce.caps,
                hops,
                last_seen_ms: now_ms,
                rssi,
                snr,
                last_position: None,
                battery_pct: None,
                is_contact,
            },
            last_announce_ms: 0,
            announce_seen: false,
        });
        entry.info.ident = ident;
        entry.info.name = name;
        entry.info.class = announce.class;
        entry.info.caps = announce.caps;
        entry.info.hops = hops;
        entry.info.last_seen_ms = now_ms;
        entry.info.is_contact = is_contact;
        if rssi.is_some() {
            entry.info.rssi = rssi;
        }
        if snr.is_some() {
            entry.info.snr = snr;
        }
        entry.last_announce_ms = now_ms;
        entry.announce_seen = true;
        entry.info.clone()
    }

    // -----------------------------------------------------------------------------------------
    // Application actions
    // -----------------------------------------------------------------------------------------

    /// Send a plain text message (single frame, or streamed as a long-text resource).
    pub fn send_text(&mut self, target: Target, text: &str, now_ms: u64) -> Result<Processed> {
        let message = AppMessage {
            id: self.allocate_message_id(),
            kind: MessageKind::Text,
            sent_at_ms: now_ms,
            text: Some(text.to_string()),
            reply_to: None,
            thread: None,
            attachment: None,
            position: None,
            telemetry: None,
            sender: None,
            signature: None,
        };
        self.send_app_message(target, message, now_ms)
    }

    /// Send an arbitrary application message.
    pub fn send_message(
        &mut self,
        target: Target,
        message: AppMessage,
        now_ms: u64,
    ) -> Result<Processed> {
        self.send_app_message(target, message, now_ms)
    }

    /// Broadcast/`target` a position report.
    pub fn send_position(
        &mut self,
        target: Target,
        position: Position,
        now_ms: u64,
    ) -> Result<Processed> {
        let message = AppMessage {
            id: self.allocate_message_id(),
            kind: MessageKind::Position,
            sent_at_ms: now_ms,
            text: None,
            reply_to: None,
            thread: None,
            attachment: None,
            position: Some(position),
            telemetry: None,
            sender: None,
            signature: None,
        };
        self.send_app_message(target, message, now_ms)
    }

    /// Emit telemetry to the mesh: every joined channel, or every contact when no channel is
    /// configured. Use [`Engine::send_telemetry_to`] for an explicit destination.
    pub fn send_telemetry(&mut self, telemetry: Telemetry, now_ms: u64) -> Result<Processed> {
        let mut targets: Vec<Target> = self
            .channels
            .keys()
            .map(|id| Target::Channel(*id))
            .collect();
        targets.sort_by_key(|target| target.address().0);
        if targets.is_empty() {
            let mut contacts: Vec<NodeId> = self
                .contacts
                .keys()
                .filter(|id| self.peer_identity(**id).is_some())
                .copied()
                .collect();
            contacts.sort_unstable();
            targets = contacts.into_iter().map(Target::Direct).collect();
        }
        if targets.is_empty() {
            return Err(Error::Config("no telemetry destination"));
        }
        let mut out = Processed::default();
        for target in targets {
            let message = AppMessage {
                id: self.allocate_message_id(),
                kind: MessageKind::Telemetry,
                sent_at_ms: now_ms,
                text: None,
                reply_to: None,
                thread: None,
                attachment: None,
                position: None,
                telemetry: Some(telemetry.clone()),
                sender: None,
                signature: None,
            };
            out.merge(self.send_app_message(target, message, now_ms)?);
        }
        Ok(out)
    }

    /// Emit telemetry to one explicit target.
    pub fn send_telemetry_to(
        &mut self,
        target: Target,
        telemetry: Telemetry,
        now_ms: u64,
    ) -> Result<Processed> {
        let message = AppMessage {
            id: self.allocate_message_id(),
            kind: MessageKind::Telemetry,
            sent_at_ms: now_ms,
            text: None,
            reply_to: None,
            thread: None,
            attachment: None,
            position: None,
            telemetry: Some(telemetry),
            sender: None,
            signature: None,
        };
        self.send_app_message(target, message, now_ms)
    }

    /// Queue a resource transfer (attachment / long text) to a direct peer or channel.
    #[allow(clippy::too_many_arguments)]
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
        self.now_ms = now_ms;
        let key = self.transfer_key_for(target)?;
        self.start_resource_with_key(target, Some(res_id), msg_id, key, kind, mime, data, now_ms)
    }

    /// Send an acknowledgement frame (PROTOCOL.md §8). ACK frames never request ACKs.
    pub fn send_ack(&mut self, to: NodeId, ack: Ack, now_ms: u64) -> Result<Processed> {
        self.now_ms = now_ms;
        let Some(peer) = self.peer_identity(to) else {
            return Err(Error::UnknownNode(to));
        };
        let include_identity = !self.peer_knows_us.contains(&to);
        let frame = self.build_ack_frame(&peer, &ack, include_identity)?;
        let mut out = Processed::default();
        self.emit_frame(&mut out, frame, SendMode::Direct(to));
        Ok(out)
    }

    /// Retransmit an unacknowledged direct message immediately (UI "retry" action).
    pub fn retry_message(&mut self, msg_id: u32, now_ms: u64) -> Result<Processed> {
        self.now_ms = now_ms;
        let Some((peer, plaintext, include_identity, attempts)) =
            self.pending_msgs.get(&msg_id).map(|pending| {
                (
                    pending.peer,
                    pending.plaintext.clone(),
                    pending.include_identity,
                    pending.attempts,
                )
            })
        else {
            return Err(Error::Malformed("message is not pending"));
        };
        if attempts >= MAX_MSG_RETRIES {
            self.pending_msgs.remove(&msg_id);
            return Err(Error::Malformed("retry budget exhausted"));
        }
        if let Some(pending) = self.pending_msgs.get_mut(&msg_id) {
            pending.attempts += 1;
            pending.last_tx_ms = now_ms;
        }
        let frame = self.seal_frame(
            FrameType::Data,
            peer.node_id,
            Flags::ENCRYPTED | Flags::ACK_REQUESTED,
            0,
            msg_id,
            &plaintext,
            &peer,
            include_identity,
        )?;
        let mut out = Processed::default();
        self.emit_frame(&mut out, frame, SendMode::Direct(peer.node_id));
        Ok(out)
    }

    fn send_app_message(
        &mut self,
        target: Target,
        message: AppMessage,
        now_ms: u64,
    ) -> Result<Processed> {
        self.now_ms = now_ms;
        match target {
            Target::Direct(peer_id) => {
                let Some(peer) = self.peer_identity(peer_id) else {
                    return Err(Error::UnknownNode(peer_id));
                };
                let wants_identity = !self.peer_knows_us.contains(&peer_id);
                let plaintext = cbor_bytes(&message);
                let plain_body_len = 5 + SEAL_OVERHEAD + plaintext.len();
                let body_len = plain_body_len
                    + if wants_identity {
                        crate::crypto::PUBLIC_IDENTITY_LEN
                    } else {
                        0
                    };
                if body_len <= MAX_BODY || plain_body_len <= MAX_BODY {
                    // Include the identity hint when it fits; if the payload only fits without
                    // it, the receiver must already know us (contact or announce).
                    let include_identity = wants_identity && body_len <= MAX_BODY;
                    let frame = self.seal_frame(
                        FrameType::Data,
                        peer_id,
                        Flags::ENCRYPTED | Flags::ACK_REQUESTED,
                        0,
                        message.id,
                        &plaintext,
                        &peer,
                        include_identity,
                    )?;
                    self.pending_msgs.insert(
                        message.id,
                        PendingMessage {
                            peer,
                            plaintext,
                            include_identity,
                            attempts: 0,
                            last_tx_ms: now_ms,
                        },
                    );
                    let mut out = Processed::default();
                    self.emit_frame(&mut out, frame, SendMode::Direct(peer_id));
                    Ok(out)
                } else {
                    // Too large for one frame: long text goes through the resource path with
                    // CBOR(AppMessage) as the plaintext so metadata survives.
                    let msg_id = message.id;
                    self.start_resource_with_key(
                        Target::Direct(peer_id),
                        None,
                        msg_id,
                        TransferKey::Peer(peer),
                        ResourceKind::LongText,
                        "application/cbor",
                        &plaintext,
                        now_ms,
                    )
                }
            }
            Target::Channel(channel_id) => {
                let Some(entry) = self.channels.get(&channel_id) else {
                    return Err(Error::UnknownNode(channel_id));
                };
                let key = entry.key.clone();
                let signature = self.identity.sign(&group_signature_payload(
                    &self.node_id(),
                    &channel_id,
                    &message,
                ));
                let msg_id = message.id;
                let group = crate::services::GroupMessage {
                    sender: self.node_id(),
                    message,
                    signature: signature.to_vec(),
                };
                let plaintext = cbor_bytes(&group);
                let body_len = 5 + SEAL_OVERHEAD + plaintext.len();
                if body_len <= MAX_BODY {
                    let frame = self.seal_frame_channel(
                        FrameType::Data,
                        channel_id,
                        Flags::ENCRYPTED,
                        0,
                        msg_id,
                        &plaintext,
                        &key,
                    )?;
                    let mut out = Processed::default();
                    self.emit_frame(&mut out, frame, SendMode::Flood);
                    Ok(out)
                } else {
                    self.start_resource_with_key(
                        Target::Channel(channel_id),
                        None,
                        msg_id,
                        TransferKey::Channel {
                            id: channel_id,
                            key,
                        },
                        ResourceKind::LongText,
                        "application/cbor",
                        &plaintext,
                        now_ms,
                    )
                }
            }
        }
    }

    fn transfer_key_for(&self, target: Target) -> Result<TransferKey> {
        match target {
            Target::Direct(peer_id) => {
                let Some(peer) = self.peer_identity(peer_id) else {
                    return Err(Error::UnknownNode(peer_id));
                };
                Ok(TransferKey::Peer(peer))
            }
            Target::Channel(channel_id) => {
                let Some(entry) = self.channels.get(&channel_id) else {
                    return Err(Error::UnknownNode(channel_id));
                };
                Ok(TransferKey::Channel {
                    id: channel_id,
                    key: entry.key.clone(),
                })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn start_resource_with_key(
        &mut self,
        target: Target,
        res_id: Option<u32>,
        msg_id: u32,
        key: TransferKey,
        kind: ResourceKind,
        mime: &str,
        data: &[u8],
        now_ms: u64,
    ) -> Result<Processed> {
        self.now_ms = now_ms;
        let res_id = match res_id {
            Some(id) => id,
            None => self.allocate_res_id(),
        };
        if self.outgoing.contains_key(&res_id) {
            return Err(Error::Malformed("resource id already in use"));
        }
        let total_len = (1 + SEAL_OVERHEAD + data.len()) as u32;
        if total_len > MAX_RESOURCE_BYTES {
            return Err(Error::PayloadTooLarge {
                size: total_len as usize,
                max: MAX_RESOURCE_BYTES as usize,
            });
        }
        let chunk_bytes = CHUNK_BYTES.min(MAX_CHUNK_BYTES);
        let count = chunk_count(total_len, chunk_bytes)?;
        let aad = resource_aad(res_id, msg_id, total_len, chunk_bytes);
        let sealed = match &key {
            TransferKey::Peer(peer) => self.identity.seal(peer, &aad, data)?,
            TransferKey::Channel {
                key: channel_key, ..
            } => channel_key.seal(&aad, data)?,
        };
        let mut blob = Vec::with_capacity(1 + sealed.len());
        blob.push(0);
        blob.extend_from_slice(&sealed);
        debug_assert_eq!(blob.len() as u32, total_len);
        let digest: [u8; 32] = Sha256::digest(&blob).into();
        let manifest = ResourceManifest {
            res_id,
            msg_id,
            kind,
            mime: mime.to_string(),
            total_len,
            chunk_bytes,
            chunk_count: count,
            sha256: digest,
        };
        let manifest_plaintext = cbor_bytes(&manifest);
        let mut sender = ResourceSender::new(res_id, count, chunk_bytes as usize);
        sender.window = RESOURCE_WINDOW;
        sender.arm(now_ms, RESOURCE_RTO_MS);

        let include_identity = match &key {
            TransferKey::Peer(peer) => !self.peer_knows_us.contains(&peer.node_id),
            TransferKey::Channel { .. } => false,
        };
        let mut out = Processed::default();
        let mut manifest_frame = match &key {
            TransferKey::Peer(peer) => self.seal_frame(
                FrameType::Data,
                peer.node_id,
                Flags::ENCRYPTED,
                PAYLOAD_IS_MANIFEST,
                msg_id,
                &manifest_plaintext,
                peer,
                include_identity,
            ),
            TransferKey::Channel {
                id,
                key: channel_key,
            } => self.seal_frame_channel(
                FrameType::Data,
                *id,
                Flags::ENCRYPTED,
                PAYLOAD_IS_MANIFEST,
                msg_id,
                &manifest_plaintext,
                channel_key,
            ),
        };
        if matches!(manifest_frame, Err(Error::PayloadTooLarge { .. })) {
            // The manifest with a 72-byte identity hint does not fit; omit the hint and rely
            // on the peer book (the addressee must have announced or be a contact).
            manifest_frame = match &key {
                TransferKey::Peer(peer) => self.seal_frame(
                    FrameType::Data,
                    peer.node_id,
                    Flags::ENCRYPTED,
                    PAYLOAD_IS_MANIFEST,
                    msg_id,
                    &manifest_plaintext,
                    peer,
                    false,
                ),
                TransferKey::Channel {
                    id,
                    key: channel_key,
                } => self.seal_frame_channel(
                    FrameType::Data,
                    *id,
                    Flags::ENCRYPTED,
                    PAYLOAD_IS_MANIFEST,
                    msg_id,
                    &manifest_plaintext,
                    channel_key,
                ),
            };
        }
        let manifest_frame = manifest_frame?;
        self.emit_frame(&mut out, manifest_frame, self.send_mode_for(target));

        let first = {
            let mut resource = OutgoingResource {
                msg_id,
                target,
                key,
                manifest: manifest.clone(),
                manifest_plaintext,
                blob,
                sender,
                started_ms: now_ms,
                paced_ms: now_ms,
            };
            // Release only the first paced batch: the rest of the window follows on later ticks
            // instead of hitting the transport as one burst.
            let first = resource.sender.take_unsent(RESOURCE_TX_BATCH as usize);
            self.outgoing.insert(res_id, resource);
            first
        };
        self.emit_resource_chunks(res_id, &first, &mut out);
        out.events.push(Event::ResourceProgress {
            peer: target.address(),
            res_id,
            received: 0,
            total: count,
        });
        Ok(out)
    }

    fn build_ack_frame(
        &mut self,
        peer: &PublicIdentity,
        ack: &Ack,
        include_identity: bool,
    ) -> Result<Frame> {
        let plaintext = cbor_bytes(ack);
        self.seal_frame(
            FrameType::Ack,
            peer.node_id,
            Flags::ENCRYPTED | Flags::PRIORITY,
            PAYLOAD_IS_ACK,
            ack.msg_id,
            &plaintext,
            peer,
            include_identity,
        )
    }

    /// Send an acknowledgement without surfacing an error when the peer is unknown.
    fn queue_ack(&mut self, to: NodeId, ack: Ack, out: &mut Processed) {
        if let Some(peer) = self.peer_identity(to) {
            let include_identity = !self.peer_knows_us.contains(&to);
            if let Ok(frame) = self.build_ack_frame(&peer, &ack, include_identity) {
                self.emit_frame(out, frame, SendMode::Direct(to));
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Bookkeeping
    // -----------------------------------------------------------------------------------------

    /// Add or update a contact.
    pub fn add_contact(&mut self, ident: PublicIdentity, name: Option<String>) {
        self.contacts.insert(ident.node_id, name);
        let entry = self
            .peers
            .entry(ident.node_id)
            .or_insert_with(|| PeerEntry {
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

    /// Remove a contact (the peer entry is kept until it expires).
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
        self.channels.insert(
            id,
            ChannelEntry {
                name: name.to_string(),
                key,
                psk: *psk,
            },
        );
        id
    }

    /// Leave a channel.
    pub fn remove_channel(&mut self, id: NodeId) {
        self.channels.remove(&id);
    }

    /// Joined channels as `(channel_id, name)`, sorted by id.
    pub fn channels(&self) -> Vec<(NodeId, String)> {
        let mut out: Vec<(NodeId, String)> = self
            .channels
            .iter()
            .map(|(id, entry)| (*id, entry.name.clone()))
            .collect();
        out.sort_by_key(|(id, _)| id.0);
        out
    }

    /// Known nodes, sorted by node id.
    pub fn known_nodes(&self) -> Vec<NodeInfo> {
        let mut out: Vec<NodeInfo> = self.peers.values().map(|p| p.info.clone()).collect();
        out.sort_by_key(|info| info.node_id.0);
        out
    }

    /// Look up a known node.
    pub fn node_info(&self, id: NodeId) -> Option<NodeInfo> {
        self.peers.get(&id).map(|p| p.info.clone())
    }

    /// Engine counters plus current queue/peer sizes.
    pub fn stats(&self) -> Stats {
        Stats {
            stored_envelopes: self.store.group_count(),
            known_nodes: self.peers.len(),
            channels: self.channels.len(),
            ..self.stats.clone()
        }
    }

    // -----------------------------------------------------------------------------------------
    // Timers & persistence
    // -----------------------------------------------------------------------------------------

    /// Drive periodic work: re-announces, retransmissions, store-and-forward offers, pruning.
    pub fn tick(&mut self, now_ms: u64) -> Processed {
        self.now_ms = now_ms;
        let mut out = Processed::default();

        // Re-announce on schedule (rate limited by the announce interval).
        let interval = self
            .config
            .announce_interval_ms
            .max(ANNOUNCE_MIN_INTERVAL_MS);
        let due = self.last_announce_ms == u64::MAX
            || now_ms.saturating_sub(self.last_announce_ms) >= interval;
        if due && self.can_originate(now_ms) {
            if let Ok(announce) = self.announce(now_ms) {
                out.merge(announce);
            }
        }

        self.tick_pending_messages(now_ms, &mut out);
        self.tick_outgoing_resources(now_ms, &mut out);
        self.tick_incoming_resources(now_ms, &mut out);

        self.store.prune(now_ms);
        self.dedup.prune(now_ms);
        self.prune_peers(now_ms);
        self.prune_seen(now_ms);
        out
    }

    /// Export persistable state (contacts, channels, counters).
    pub fn export_state(&self) -> StateSnapshot {
        let mut contacts: Vec<(PublicIdentity, Option<String>)> = self
            .contacts
            .iter()
            .filter_map(|(id, name)| {
                self.peers
                    .get(id)
                    .map(|peer| (peer.info.ident, name.clone()))
            })
            .collect();
        contacts.sort_by_key(|(ident, _)| ident.node_id.0);
        let mut channels: Vec<(NodeId, String, [u8; 32])> = self
            .channels
            .iter()
            .map(|(id, entry)| (*id, entry.name.clone(), entry.psk))
            .collect();
        channels.sort_by_key(|(id, _, _)| id.0);
        StateSnapshot {
            contacts,
            channels,
            last_seq: self.seq,
            msg_counter: self.msg_counter,
            msg_id_epoch: self.msg_id_epoch,
        }
    }

    /// Restore state previously produced by [`Engine::export_state`].
    pub fn import_state(&mut self, state: StateSnapshot) {
        for (ident, name) in state.contacts {
            self.add_contact(ident, name);
        }
        for (_id, name, psk) in state.channels {
            self.add_channel(&name, &psk);
        }
        self.seq = self.seq.max(state.last_seq);
        // Persisted message-id allocation state. Monotonic counters prevent id reuse across
        // restarts; a legacy snapshot (epoch 0 / counter 0) starts at a high floor so ids issued
        // before the counter was persisted can never be re-derived.
        self.msg_counter = self.msg_counter.max(state.msg_counter);
        if state.msg_id_epoch == 0 {
            self.msg_counter = self.msg_counter.max(LEGACY_MSG_COUNTER_START);
            self.msg_id_epoch = self.msg_id_epoch.max(MSG_ID_EPOCH);
        } else {
            self.msg_id_epoch = self.msg_id_epoch.max(state.msg_id_epoch);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Frame construction helpers
    // -----------------------------------------------------------------------------------------

    fn next_seq(&mut self) -> u32 {
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    /// Allocate a fresh outgoing message id for a user-visible message.
    ///
    /// The counter and derivation epoch are part of [`Engine::export_state`], so an embedder that
    /// persists/imports the state never reuses an id after a restart. Within a session the loop
    /// also skips ids that are still pending or carried by an in-flight resource.
    pub fn allocate_message_id(&mut self) -> u32 {
        loop {
            self.msg_counter = self.msg_counter.wrapping_add(1);
            let seed = self.identity.seed();
            let mut hasher = Sha256::new();
            hasher.update(b"openmesh/v1/msg-id");
            hasher.update(self.msg_id_epoch.to_be_bytes());
            hasher.update(seed);
            hasher.update(self.msg_counter.to_be_bytes());
            let digest: [u8; 32] = hasher.finalize().into();
            let id = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
            if id != 0 && !self.msg_id_in_use(id) {
                return id;
            }
        }
    }

    /// Sequence number of the most recently originated frame.
    ///
    /// Senders MUST keep this monotonic across restarts: receivers deduplicate by `(src, seq)` for
    /// [`DEDUP_TTL_MS`], so reusing small sequence numbers after a restart makes a running peer drop
    /// the new frames as duplicates (observed as a bridge-restart media stall).
    pub fn last_seq(&self) -> u32 {
        self.seq
    }

    /// Raise the sequence floor so the next originated frame has `seq > floor`.
    ///
    /// Embedders that restart must restore the persisted [`Engine::last_seq`] through this method
    /// before emitting frames; the engine's own sequence counter is not part of [`EngineConfig`].
    pub fn set_sequence_floor(&mut self, floor: u32) {
        self.seq = self.seq.max(floor);
    }

    /// Whether a message id is still pending (direct ack) or carried by an in-flight resource.
    fn msg_id_in_use(&self, id: u32) -> bool {
        self.pending_msgs.contains_key(&id)
            || self.outgoing.values().any(|resource| resource.msg_id == id)
    }

    fn allocate_res_id(&mut self) -> u32 {
        loop {
            self.res_counter = self.res_counter.wrapping_add(1);
            let seed = self.identity.seed();
            let mut hasher = Sha256::new();
            hasher.update(b"openmesh/v1/res-id");
            hasher.update(seed);
            hasher.update(self.res_counter.to_be_bytes());
            let digest: [u8; 32] = hasher.finalize().into();
            let id = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
            if id != 0 && !self.outgoing.contains_key(&id) {
                return id;
            }
        }
    }

    fn peer_identity(&self, node: NodeId) -> Option<PublicIdentity> {
        self.peers.get(&node).map(|peer| peer.info.ident)
    }

    fn emit_frame(&mut self, out: &mut Processed, frame: Frame, mode: SendMode) {
        self.stats.tx_frames += 1;
        let len = crate::wire::HEADER_LEN + frame.body.len();
        self.stats.airtime_tx_ms +=
            crate::util::lora_airtime_ms(&crate::util::MEDIUM_FAST, len) as u64;
        out.outbound.push(Outbound {
            frame,
            mode,
            link: None,
        });
    }

    fn can_originate(&mut self, now_ms: u64) -> bool {
        if now_ms.saturating_sub(self.orig_window_start_ms) >= 60_000 {
            self.orig_window_start_ms = now_ms;
            self.orig_count = 0;
        }
        if self.orig_count >= MAX_ORIGINATE_PER_MIN {
            return false;
        }
        self.orig_count += 1;
        true
    }

    fn send_mode_for(&self, target: Target) -> SendMode {
        match target {
            Target::Channel(_) => SendMode::Flood,
            Target::Direct(node) => self.reply_mode(node),
        }
    }

    fn reply_mode(&self, node: NodeId) -> SendMode {
        if self.heard_at.contains_key(&node) {
            SendMode::Direct(node)
        } else {
            SendMode::Flood
        }
    }

    fn relay_mode(&self, frame: &Frame) -> SendMode {
        if frame.is_broadcast() {
            SendMode::Flood
        } else {
            self.reply_mode(frame.dst)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn seal_frame(
        &mut self,
        typ: FrameType,
        dst: NodeId,
        header_flags: u8,
        payload_flags: u8,
        msg_ref: u32,
        plaintext: &[u8],
        peer: &PublicIdentity,
        include_identity: bool,
    ) -> Result<Frame> {
        let mut frame = Frame {
            typ,
            flags: Flags::from_bits(header_flags),
            ttl: self.config.default_ttl,
            src: self.node_id(),
            dst,
            seq: self.next_seq(),
            body: Vec::new(),
        };
        let aad = frame.aad();
        let sealed = self.identity.seal(peer, &aad, plaintext)?;
        let identity = if include_identity {
            Some(self.public_identity())
        } else {
            None
        };
        let payload_flags = if include_identity {
            payload_flags | PAYLOAD_IDENTITY_INCLUDED
        } else {
            payload_flags
        };
        frame.body = sealed_body(payload_flags, msg_ref, identity.as_ref(), &sealed);
        if frame.body.len() > MAX_BODY {
            return Err(Error::PayloadTooLarge {
                size: frame.body.len(),
                max: MAX_BODY,
            });
        }
        Ok(frame)
    }

    #[allow(clippy::too_many_arguments)]
    fn seal_frame_channel(
        &mut self,
        typ: FrameType,
        dst: NodeId,
        header_flags: u8,
        payload_flags: u8,
        msg_ref: u32,
        plaintext: &[u8],
        key: &ChannelKey,
    ) -> Result<Frame> {
        let mut frame = Frame {
            typ,
            flags: Flags::from_bits(header_flags),
            ttl: self.config.default_ttl,
            src: self.node_id(),
            dst,
            seq: self.next_seq(),
            body: Vec::new(),
        };
        let aad = frame.aad();
        let sealed = key.seal(&aad, plaintext)?;
        frame.body = sealed_body(payload_flags, msg_ref, None, &sealed);
        if frame.body.len() > MAX_BODY {
            return Err(Error::PayloadTooLarge {
                size: frame.body.len(),
                max: MAX_BODY,
            });
        }
        Ok(frame)
    }

    fn build_chunk_frame(
        &mut self,
        dst: NodeId,
        msg_ref: u32,
        res_id: u32,
        index: u16,
        slice: &[u8],
    ) -> Frame {
        Frame {
            typ: FrameType::Resource,
            flags: Flags::from_bits(Flags::ENCRYPTED),
            ttl: self.config.default_ttl,
            src: self.node_id(),
            dst,
            seq: self.next_seq(),
            body: chunk_body(msg_ref, res_id, index, slice),
        }
    }

    fn emit_resource_chunks(&mut self, res_id: u32, indices: &[u16], out: &mut Processed) {
        if indices.is_empty() {
            return;
        }
        let Some(resource) = self.outgoing.get(&res_id) else {
            return;
        };
        let dst = resource.key.address();
        let msg_ref = resource.msg_id;
        let chunk_bytes = resource.manifest.chunk_bytes as usize;
        let blob = resource.blob.clone();
        let target = resource.target;
        for &index in indices {
            let start = index as usize * chunk_bytes;
            if start >= blob.len() {
                continue;
            }
            let end = (start + chunk_bytes).min(blob.len());
            let frame = self.build_chunk_frame(dst, msg_ref, res_id, index, &blob[start..end]);
            self.emit_frame(out, frame, self.send_mode_for(target));
        }
    }

    // -----------------------------------------------------------------------------------------
    // Receive: peer bookkeeping
    // -----------------------------------------------------------------------------------------

    /// Update peer link statistics and direct-reachability tracking. Returns whether the frame
    /// appears to have been transmitted by its source (full origination TTL).
    fn note_heard(&mut self, frame: &Frame, link: Option<&LinkInfo>, now_ms: u64) -> bool {
        if let Some(link) = link {
            if let Some(peer) = link.peer {
                self.heard_at.insert(peer, now_ms);
                if let Some(entry) = self.peers.get_mut(&peer) {
                    entry.info.last_seen_ms = now_ms;
                    if link.rssi.is_some() {
                        entry.info.rssi = link.rssi;
                    }
                    if link.snr.is_some() {
                        entry.info.snr = link.snr;
                    }
                }
            }
        }
        // A frame that still carries its full origination hop budget came from the source
        // itself, not from a relay. This is the only way to tell a neighbour from a relay on a
        // shared broadcast medium (v1 has no per-hop transmitter id).
        let direct = frame.ttl >= origin_ttl(frame.typ);
        if direct && frame.src.is_unicast() {
            self.heard_at.insert(frame.src, now_ms);
            if let Some(entry) = self.peers.get_mut(&frame.src) {
                entry.info.last_seen_ms = now_ms;
            }
        }
        direct
    }

    fn learn_link_stats(&mut self, frame: &Frame, link: Option<&LinkInfo>, now_ms: u64) {
        let Some(link) = link else { return };
        if let Some(entry) = self.peers.get_mut(&frame.src) {
            entry.info.last_seen_ms = now_ms;
            if let Some(rssi) = link.rssi {
                entry.info.rssi = Some(rssi);
            }
            if let Some(snr) = link.snr {
                entry.info.snr = Some(snr);
            }
        }
    }

    /// Learn a public identity from an `IDENTITY_INCLUDED` hint.
    fn learn_identity(
        &mut self,
        ident: PublicIdentity,
        link: Option<&LinkInfo>,
        now_ms: u64,
    ) -> Option<NodeInfo> {
        if !ident.is_self_consistent() {
            return None;
        }
        if let Some(entry) = self.peers.get_mut(&ident.node_id) {
            entry.info.ident = ident;
            entry.info.last_seen_ms = now_ms;
            if let Some(link) = link {
                if link.rssi.is_some() {
                    entry.info.rssi = link.rssi;
                }
                if link.snr.is_some() {
                    entry.info.snr = link.snr;
                }
            }
            return Some(entry.info.clone());
        }
        let is_contact = self.contacts.contains_key(&ident.node_id);
        let name = self
            .contacts
            .get(&ident.node_id)
            .cloned()
            .flatten()
            .unwrap_or_else(|| ident.node_id.to_hex());
        let info = NodeInfo {
            node_id: ident.node_id,
            ident,
            name,
            class: NodeClass::Client,
            caps: 0,
            hops: 0,
            last_seen_ms: now_ms,
            rssi: link.and_then(|l| l.rssi),
            snr: link.and_then(|l| l.snr),
            last_position: None,
            battery_pct: None,
            is_contact,
        };
        self.peers.insert(
            ident.node_id,
            PeerEntry {
                info: info.clone(),
                last_announce_ms: 0,
                announce_seen: false,
            },
        );
        Some(info)
    }

    fn mark_seen_msg(&mut self, from: NodeId, msg_id: u32, now_ms: u64) -> bool {
        let key = (from, msg_id);
        if self.seen_msgs.contains_key(&key) {
            return false;
        }
        self.seen_msgs.insert(key, now_ms);
        true
    }

    fn mark_seen_ref(&mut self, msg_ref: u32, now_ms: u64) {
        self.seen_refs.insert(msg_ref, now_ms);
    }

    // -----------------------------------------------------------------------------------------
    // Receive: direct data / acks / manifests / chunks
    // -----------------------------------------------------------------------------------------

    fn process_direct_data(
        &mut self,
        frame: &Frame,
        link: Option<&LinkInfo>,
        now_ms: u64,
        out: &mut Processed,
    ) {
        if !frame.flags.contains(Flags::ENCRYPTED) {
            self.stats.dropped_frames += 1;
            out.events
                .push(Event::Error("direct data frame is not encrypted".into()));
            return;
        }
        let Ok((payload_flags, msg_ref, hint, sealed)) = parse_sealed_body(&frame.body) else {
            self.stats.dropped_frames += 1;
            return;
        };
        if let Some(ident) = hint {
            if ident.node_id == frame.src && ident.is_self_consistent() {
                self.learn_identity(ident, link, now_ms);
            }
        }
        let Some(peer) = self.peer_identity(frame.src) else {
            self.stats.dropped_frames += 1;
            out.events
                .push(Event::Error("direct data from unknown sender".into()));
            return;
        };
        // Receiving a sealed frame from a peer proves that peer knows our identity.
        self.peer_knows_us.insert(frame.src);
        let aad = frame.aad();
        let plaintext = match self.identity.open(&peer, &aad, sealed) {
            Ok(plaintext) => plaintext,
            Err(_) => {
                self.stats.dropped_frames += 1;
                out.events
                    .push(Event::Error("direct data decrypt failed".into()));
                return;
            }
        };
        if payload_flags & PAYLOAD_IS_ACK != 0 {
            match cbor_from_bytes::<Ack>(&plaintext) {
                Ok(ack) => self.handle_ack(frame.src, ack, now_ms, out),
                Err(err) => out
                    .events
                    .push(Event::Error(format!("bad ack body: {err}"))),
            }
            return;
        }
        if payload_flags & PAYLOAD_IS_MANIFEST != 0 {
            match cbor_from_bytes::<ResourceManifest>(&plaintext) {
                Ok(manifest) => {
                    let key = TransferKey::Peer(peer);
                    self.begin_incoming_resource(
                        frame.src, None, key, manifest, msg_ref, now_ms, out,
                    );
                }
                Err(err) => out
                    .events
                    .push(Event::Error(format!("bad manifest: {err}"))),
            }
            return;
        }
        match cbor_from_bytes::<AppMessage>(&plaintext) {
            Ok(message) => {
                self.mark_seen_ref(msg_ref, now_ms);
                self.handle_direct_app_message(frame, peer, message, link, now_ms, out);
            }
            Err(err) => {
                self.stats.dropped_frames += 1;
                out.events
                    .push(Event::Error(format!("bad app message: {err}")));
            }
        }
    }

    fn handle_direct_app_message(
        &mut self,
        frame: &Frame,
        peer: PublicIdentity,
        message: AppMessage,
        link: Option<&LinkInfo>,
        now_ms: u64,
        out: &mut Processed,
    ) {
        let first = self.mark_seen_msg(peer.node_id, message.id, now_ms);
        if first {
            if let Some(position) = &message.position {
                if let Some(entry) = self.peers.get_mut(&peer.node_id) {
                    entry.info.last_position = Some(position.clone());
                }
            }
            if let Some(telemetry) = &message.telemetry {
                if let Some(entry) = self.peers.get_mut(&peer.node_id) {
                    if telemetry.battery_pct.is_some() {
                        entry.info.battery_pct = telemetry.battery_pct;
                    }
                }
            }
            out.events.push(Event::Message(ReceivedMessage {
                from: peer.node_id,
                message: message.clone(),
                link: link.cloned().unwrap_or_default(),
                via_store: false,
            }));
            if let Some(position) = &message.position {
                out.events.push(Event::Position {
                    from: peer.node_id,
                    channel: None,
                    position: position.clone(),
                });
            }
            if let Some(telemetry) = &message.telemetry {
                out.events.push(Event::Telemetry {
                    from: peer.node_id,
                    telemetry: telemetry.clone(),
                });
            }
        }
        if frame.flags.contains(Flags::ACK_REQUESTED) {
            self.send_delivered_ack(peer.node_id, message.id, now_ms, out);
        }
    }

    fn send_delivered_ack(&mut self, to: NodeId, msg_id: u32, now_ms: u64, out: &mut Processed) {
        let key = (to, msg_id);
        if let Some(last) = self.ack_sent_at.get(&key) {
            if now_ms.saturating_sub(*last) < RE_ACK_MIN_INTERVAL_MS {
                return;
            }
        }
        self.ack_sent_at.insert(key, now_ms);
        let ack = Ack {
            kind: AckKind::Delivered,
            msg_id,
            res_id: None,
            next_expected: None,
            bitmap: None,
            timestamp_ms: now_ms,
        };
        self.queue_ack(to, ack, out);
    }

    fn process_channel_data(
        &mut self,
        frame: &Frame,
        link: Option<&LinkInfo>,
        now_ms: u64,
        out: &mut Processed,
    ) {
        let Some(entry) = self.channels.get(&frame.dst) else {
            return;
        };
        let key = entry.key.clone();
        if !frame.flags.contains(Flags::ENCRYPTED) {
            self.stats.dropped_frames += 1;
            return;
        }
        let Ok((payload_flags, msg_ref, hint, sealed)) = parse_sealed_body(&frame.body) else {
            self.stats.dropped_frames += 1;
            return;
        };
        if let Some(ident) = hint {
            if ident.node_id == frame.src && ident.is_self_consistent() {
                self.learn_identity(ident, link, now_ms);
            }
        }
        let aad = frame.aad();
        let plaintext = match key.open(&aad, sealed) {
            Ok(plaintext) => plaintext,
            Err(_) => {
                self.stats.dropped_frames += 1;
                out.events
                    .push(Event::Error("channel decrypt failed".into()));
                return;
            }
        };
        if payload_flags & PAYLOAD_IS_MANIFEST != 0 {
            if let Ok(manifest) = cbor_from_bytes::<ResourceManifest>(&plaintext) {
                let key = TransferKey::Channel { id: frame.dst, key };
                self.begin_incoming_resource(
                    frame.src,
                    Some(frame.dst),
                    key,
                    manifest,
                    msg_ref,
                    now_ms,
                    out,
                );
            }
            return;
        }
        if payload_flags & PAYLOAD_IS_ACK != 0 {
            return;
        }
        let Ok(group) = cbor_from_bytes::<crate::services::GroupMessage>(&plaintext) else {
            self.stats.dropped_frames += 1;
            return;
        };
        if group.sender != frame.src {
            self.stats.dropped_frames += 1;
            out.events
                .push(Event::Error("group message sender mismatch".into()));
            return;
        }
        let Some(signer) = self.peer_identity(group.sender) else {
            self.stats.dropped_frames += 1;
            out.events
                .push(Event::Error("group message from unknown signer".into()));
            return;
        };
        let payload = group_signature_payload(&group.sender, &frame.dst, &group.message);
        let Ok(signature) = <[u8; 64]>::try_from(group.signature.as_slice()) else {
            self.stats.dropped_frames += 1;
            out.events
                .push(Event::Error("group message signature length".into()));
            return;
        };
        if signer.verify(&payload, &signature).is_err() {
            self.stats.dropped_frames += 1;
            out.events
                .push(Event::Error("group message signature invalid".into()));
            return;
        }
        self.mark_seen_ref(msg_ref, now_ms);
        if self.mark_seen_msg(group.sender, group.message.id, now_ms) {
            if let Some(position) = &group.message.position {
                if let Some(entry) = self.peers.get_mut(&group.sender) {
                    entry.info.last_position = Some(position.clone());
                }
            }
            out.events.push(Event::GroupMessage(ReceivedGroupMessage {
                channel: frame.dst,
                sender: group.sender,
                message: group.message.clone(),
                link: link.cloned().unwrap_or_default(),
            }));
            if let Some(position) = &group.message.position {
                out.events.push(Event::Position {
                    from: group.sender,
                    channel: Some(frame.dst),
                    position: position.clone(),
                });
            }
            if let Some(telemetry) = &group.message.telemetry {
                out.events.push(Event::Telemetry {
                    from: group.sender,
                    telemetry: telemetry.clone(),
                });
            }
        }
    }

    fn process_ack(&mut self, frame: &Frame, now_ms: u64, out: &mut Processed) {
        let Ok((payload_flags, _msg_ref, hint, sealed)) = parse_sealed_body(&frame.body) else {
            self.stats.dropped_frames += 1;
            return;
        };
        if payload_flags & PAYLOAD_IS_ACK == 0 {
            self.stats.dropped_frames += 1;
            tracing::debug!(
                from = %frame.src,
                dst = %frame.dst,
                "dropped ACK frame: payload lacks the IS_ACK flag"
            );
            out.events
                .push(Event::Error("ack frame without IS_ACK".into()));
            return;
        }
        if let Some(ident) = hint {
            if ident.node_id == frame.src && ident.is_self_consistent() {
                self.learn_identity(ident, None, now_ms);
            }
        }
        let Some(peer) = self.peer_identity(frame.src) else {
            self.stats.dropped_frames += 1;
            tracing::debug!(
                from = %frame.src,
                "dropped ACK frame: unknown peer identity (no announce/contact yet)"
            );
            return;
        };
        let aad = frame.aad();
        let Ok(plaintext) = self.identity.open(&peer, &aad, sealed) else {
            self.stats.dropped_frames += 1;
            tracing::debug!(from = %frame.src, "dropped ACK frame: decryption failed");
            return;
        };
        match cbor_from_bytes::<Ack>(&plaintext) {
            Ok(ack) => {
                self.stats.acks_rx += 1;
                tracing::debug!(
                    from = %frame.src,
                    kind = ?ack.kind,
                    msg_id = ack.msg_id,
                    msg_ref = format!("{:#010x}", ack.msg_id),
                    res_id = ?ack.res_id,
                    next_expected = ?ack.next_expected,
                    bitmap = ?ack.bitmap,
                    "ack accepted"
                );
                self.peer_knows_us.insert(frame.src);
                self.handle_ack(frame.src, ack, now_ms, out);
            }
            Err(err) => {
                tracing::debug!(from = %frame.src, error = %err, "dropped ACK frame: bad body");
                out.events
                    .push(Event::Error(format!("bad ack body: {err}")));
            }
        }
    }

    fn handle_ack(&mut self, from: NodeId, ack: Ack, now_ms: u64, out: &mut Processed) {
        match ack.kind {
            AckKind::Delivered => {
                if self.pending_msgs.remove(&ack.msg_id).is_some() {
                    out.events.push(Event::Ack { from, ack });
                }
            }
            AckKind::Read => {
                // Read receipts are user-triggered and may arrive after the delivery ack has
                // already cleared the pending entry, so surface them unconditionally.
                self.pending_msgs.remove(&ack.msg_id);
                out.events.push(Event::Ack { from, ack });
            }
            AckKind::ResourceWindow => {
                let Some(res_id) = ack.res_id else { return };
                let (Some(next_expected), Some(bitmap)) = (ack.next_expected, ack.bitmap) else {
                    return;
                };
                self.stats.window_acks_rx += 1;
                tracing::debug!(
                    from = %from,
                    res_id,
                    msg_id = ack.msg_id,
                    msg_ref = format!("{:#010x}", ack.msg_id),
                    next_expected,
                    bitmap = format!("{bitmap:#010x}"),
                    "resource window ack received"
                );
                let pace_interval = self.resource_pace_interval_ms;
                let (completed, unsent, progress) = {
                    let Some(resource) = self.outgoing.get_mut(&res_id) else {
                        return;
                    };
                    if let Some(expected) = resource.key.peer() {
                        if expected.node_id != from {
                            return;
                        }
                    }
                    match resource
                        .sender
                        .on_ack(next_expected, bitmap, now_ms, RESOURCE_RTO_MS)
                    {
                        Ok(true) => (true, Vec::new(), None),
                        Ok(false) => {
                            // Release the next paced batch of never-sent chunks, and also repair
                            // any sent-but-unacked chunks the peer reported as missing. This is what
                            // completes small resources: if the last chunk was lost (or emitted in a
                            // burst that the companion radio dropped), the ack for N-1 chunks is the
                            // signal to release it again on a paced link.
                            //
                            // Both tick and ack releases share the same duty-aware interval so a
                            // fast-acking/fast-ticking embedder cannot outpace the radio.
                            let batch = usize::from(RESOURCE_TX_BATCH).max(1);
                            let paced = now_ms.saturating_sub(resource.paced_ms) >= pace_interval;
                            let mut release = if paced {
                                resource.sender.take_unsent(batch)
                            } else {
                                Vec::new()
                            };
                            if paced && release.len() < batch {
                                let pending = resource.sender.pending_window();
                                release.extend(
                                    pending
                                        .into_iter()
                                        .filter(|index| !resource.sender.acked[*index as usize])
                                        .take(batch - release.len()),
                                );
                            }
                            if !release.is_empty() {
                                resource.paced_ms = now_ms;
                            }
                            tracing::debug!(
                                res_id,
                                acked = resource.sender.acked_count(),
                                total = resource.sender.total_chunks,
                                next_unsent = resource.sender.next_unsent,
                                pending = resource.sender.pending_window().len(),
                                released = release.len(),
                                paced,
                                retries = resource.sender.retries,
                                deadline_ms = resource.sender.deadline_ms,
                                "resource window ack applied"
                            );
                            (
                                false,
                                release,
                                Some((resource.sender.acked_count(), resource.sender.total_chunks)),
                            )
                        }
                        Err(_) => return,
                    }
                };
                if completed {
                    self.finish_outgoing_resource(res_id, from, out);
                } else {
                    self.emit_resource_chunks(res_id, &unsent, out);
                    if let Some((received, total)) = progress {
                        out.events.push(Event::ResourceProgress {
                            peer: from,
                            res_id,
                            received,
                            total,
                        });
                    }
                }
            }
            AckKind::ResourceComplete => {
                let Some(res_id) = ack.res_id else { return };
                let known = self
                    .outgoing
                    .get(&res_id)
                    .map(|resource| resource.key.peer().is_none_or(|peer| peer.node_id == from))
                    .unwrap_or(false);
                if known {
                    self.finish_outgoing_resource(res_id, from, out);
                }
            }
        }
    }

    fn finish_outgoing_resource(&mut self, res_id: u32, from: NodeId, out: &mut Processed) {
        if self.outgoing.remove(&res_id).is_some() {
            tracing::debug!(res_id, peer = %from, "outgoing resource complete");
            out.events.push(Event::ResourceSent { peer: from, res_id });
        }
    }

    // --- store and forward ----------------------------------------------------------------

    fn maybe_store(&mut self, frame: &Frame, now_ms: u64) {
        let (capacity, ttl_ms) = match &self.config.role {
            Role::Relay(relay) => (relay.store_capacity, relay.store_ttl_ms),
            Role::Client => return,
        };
        if capacity == 0 || !frame.typ.is_storable() {
            return;
        }
        if !frame.dst.is_unicast() || frame.dst == self.node_id() || frame.is_broadcast() {
            return;
        }
        if !frame.flags.contains(Flags::ENCRYPTED) || frame.body.len() < 5 {
            return;
        }
        if let Some(seen) = self.heard_at.get(&frame.dst) {
            if now_ms.saturating_sub(*seen) < STORE_PEER_REACHABLE_MS {
                return;
            }
        }
        let msg_ref =
            u32::from_be_bytes([frame.body[1], frame.body[2], frame.body[3], frame.body[4]]);
        let envelope = StoredEnvelope {
            msg_id: msg_ref,
            dst: frame.dst,
            src: frame.src,
            frame_type: frame.typ as u8,
            flags: frame.flags.bits(),
            seq: frame.seq,
            body: frame.body.clone(),
            stored_at_ms: now_ms,
            expires_at_ms: now_ms,
        };
        let _ = ttl_ms;
        self.store.push_evicting(envelope, now_ms);
    }

    fn maybe_offer_store(&mut self, peer: NodeId, now_ms: u64, out: &mut Processed) {
        if !matches!(self.config.role, Role::Relay(_)) {
            return;
        }
        if self.config.caps & crate::services::caps::STORE == 0 {
            return;
        }
        let ids = self.store.ids_for_limited(peer, MAX_STORE_OFFER_IDS);
        if ids.is_empty() {
            return;
        }
        if let Some(last) = self.store_offer_at.get(&peer) {
            if now_ms.saturating_sub(*last) < STORE_OFFER_INTERVAL_MS {
                return;
            }
        }
        let count = self.store.count_for(peer) as u32;
        let offer = StoreOffer { ids, count };
        // StoreOffer bodies are CLEAR-TEXT CBOR (PROTOCOL.md §4.1, §10): a relay that has never
        // heard the addressee's announce has no key material to seal with, and `msg_ref` is
        // already clear in Data/Resource bodies, so nothing new is leaked.
        let body = cbor_bytes(&offer);
        if body.len() > MAX_BODY {
            return;
        }
        let frame = Frame {
            typ: FrameType::StoreOffer,
            flags: Flags::from_bits(Flags::PRIORITY),
            ttl: self.config.default_ttl,
            src: self.node_id(),
            dst: peer,
            seq: self.next_seq(),
            body,
        };
        self.store_offer_at.insert(peer, now_ms);
        let mode = self.reply_mode(peer);
        self.emit_frame(out, frame, mode);
    }

    fn process_store_offer(&mut self, frame: &Frame, out: &mut Processed) {
        let Ok(offer) = cbor_from_bytes::<StoreOffer>(&frame.body) else {
            self.stats.dropped_frames += 1;
            return;
        };
        let offered = offer.ids.clone();
        let wanted: Vec<u32> = offered
            .iter()
            .copied()
            .filter(|id| !self.seen_refs.contains_key(id))
            .collect();
        out.events.push(Event::StoreOffer {
            from: frame.src,
            ids: offered,
        });
        if wanted.is_empty() {
            return;
        }
        let want = StoreWant { ids: wanted };
        let body = cbor_bytes(&want);
        if body.len() > MAX_BODY {
            return;
        }
        let reply = Frame {
            typ: FrameType::StoreWant,
            flags: Flags::from_bits(Flags::PRIORITY),
            ttl: self.config.default_ttl,
            src: self.node_id(),
            dst: frame.src,
            seq: self.next_seq(),
            body,
        };
        let mode = self.reply_mode(frame.src);
        self.emit_frame(out, reply, mode);
    }

    fn process_store_want(&mut self, frame: &Frame, out: &mut Processed) {
        if !matches!(self.config.role, Role::Relay(_)) {
            return;
        }
        let Ok(want) = cbor_from_bytes::<StoreWant>(&frame.body) else {
            self.stats.dropped_frames += 1;
            return;
        };
        let envelopes = self.store.frames_for(frame.src, &want.ids);
        for envelope in envelopes {
            let Ok(typ) = FrameType::from_u8(envelope.frame_type) else {
                continue;
            };
            let frame = Frame {
                typ,
                flags: Flags::from_bits(envelope.flags),
                ttl: DEFAULT_TTL,
                src: envelope.src,
                dst: envelope.dst,
                seq: envelope.seq,
                body: envelope.body,
            };
            let mode = self.reply_mode(frame.dst);
            self.emit_frame(out, frame, mode);
        }
    }

    /// Answer duplicate `Data` frames addressed to us with `ACK_REQ` without re-decrypting.
    fn maybe_reack_duplicate(&mut self, frame: &Frame, now_ms: u64, out: &mut Processed) {
        if frame.dst != self.node_id()
            || frame.typ != FrameType::Data
            || !frame.flags.contains(Flags::ACK_REQUESTED)
            || frame.body.len() < 5
        {
            return;
        }
        if frame.body[0] & PAYLOAD_IS_ACK != 0 || frame.body[0] & PAYLOAD_IS_MANIFEST != 0 {
            return;
        }
        let msg_ref =
            u32::from_be_bytes([frame.body[1], frame.body[2], frame.body[3], frame.body[4]]);
        if self.seen_msgs.contains_key(&(frame.src, msg_ref)) {
            self.send_delivered_ack(frame.src, msg_ref, now_ms, out);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Resources: receiver side
    // -----------------------------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn begin_incoming_resource(
        &mut self,
        peer: NodeId,
        channel: Option<NodeId>,
        key: TransferKey,
        manifest: ResourceManifest,
        msg_ref: u32,
        now_ms: u64,
        out: &mut Processed,
    ) {
        let id = (peer, manifest.res_id);
        self.mark_seen_ref(msg_ref, now_ms);
        // Duplicate manifest: answer with the current window / completion ack so a lost ack
        // does not stall the sender.
        if self.incoming.contains_key(&id) {
            tracing::debug!(
                peer = %peer,
                res_id = manifest.res_id,
                msg_id = manifest.msg_id,
                "duplicate resource manifest; re-queuing window ack"
            );
            self.send_window_ack(id, now_ms, out);
            return;
        }
        if let Some((done, previous)) = self.completed.get(&id).cloned() {
            if now_ms.saturating_sub(done) <= RESOURCE_KEEP_MS {
                self.send_resource_complete_ack(peer, &previous, now_ms, out);
            }
            return;
        }
        if manifest.total_len > MAX_RESOURCE_BYTES
            || manifest.chunk_bytes == 0
            || manifest.chunk_bytes > MAX_CHUNK_BYTES
            || manifest.chunk_count
                != chunk_count(manifest.total_len, manifest.chunk_bytes).unwrap_or(u16::MAX)
        {
            self.stats.dropped_frames += 1;
            out.events
                .push(Event::Error("invalid resource manifest".into()));
            return;
        }
        let Ok(receiver) = ResourceReceiver::new(
            manifest.res_id,
            manifest.total_len,
            manifest.chunk_bytes as usize,
        ) else {
            out.events
                .push(Event::Error("resource receiver init failed".into()));
            return;
        };
        let total = receiver.total();
        self.incoming.insert(
            id,
            IncomingResource {
                peer,
                channel,
                manifest: manifest.clone(),
                receiver,
                key,
                dirty: false,
                last_ack_ms: now_ms,
                progress_reported: 0,
                chunks_since_ack: 0,
            },
        );
        out.events.push(Event::ResourceProgress {
            peer,
            res_id: manifest.res_id,
            received: 0,
            total,
        });
        if total == 0 {
            self.complete_incoming_resource(id, now_ms, out);
        }
    }

    fn process_incoming_chunk(&mut self, frame: &Frame, now_ms: u64, out: &mut Processed) {
        let Ok((msg_ref, res_id, index, slice)) = parse_chunk_body(&frame.body) else {
            self.stats.dropped_frames += 1;
            return;
        };
        if slice.len() > MAX_CHUNK_BYTES as usize {
            self.stats.dropped_frames += 1;
            return;
        }
        let id = (frame.src, res_id);
        if !self.incoming.contains_key(&id) {
            // A retransmitted chunk of an already completed resource: repeat the completion ack.
            if let Some((done, manifest)) = self.completed.get(&id).cloned() {
                if now_ms.saturating_sub(done) <= RESOURCE_KEEP_MS {
                    self.send_resource_complete_ack(frame.src, &manifest, now_ms, out);
                }
            }
            return;
        }
        self.mark_seen_ref(msg_ref, now_ms);
        let outcome = {
            let resource = self.incoming.get_mut(&id).expect("checked above");
            let expected_before = resource.receiver.ack_window().0;
            match resource.receiver.insert_chunk(index, slice) {
                Ok(true) => {
                    resource.dirty = true;
                    resource.chunks_since_ack += 1;
                    let complete = resource.receiver.is_complete();
                    let hole = index > expected_before;
                    let ack_now =
                        !complete && (hole || resource.chunks_since_ack >= RESOURCE_ACK_EVERY);
                    ChunkOutcome::Inserted { complete, ack_now }
                }
                Ok(false) => ChunkOutcome::Duplicate,
                Err(_) => ChunkOutcome::Rejected,
            }
        };
        match outcome {
            ChunkOutcome::Inserted { complete: true, .. } => {
                self.send_window_ack(id, now_ms, out);
                self.complete_incoming_resource(id, now_ms, out);
            }
            ChunkOutcome::Inserted { ack_now: true, .. } => {
                self.send_window_ack(id, now_ms, out);
            }
            ChunkOutcome::Inserted { .. } => {}
            ChunkOutcome::Duplicate => {
                // The sender is retransmitting, so our last ack was likely lost.
                self.send_window_ack(id, now_ms, out);
            }
            ChunkOutcome::Rejected => {
                self.stats.dropped_frames += 1;
            }
        }
    }

    fn send_window_ack(&mut self, id: ResourceKey, now_ms: u64, out: &mut Processed) {
        let snapshot = {
            let Some(resource) = self.incoming.get_mut(&id) else {
                return;
            };
            let (next_expected, bitmap) = resource.receiver.ack_window();
            let received = resource.receiver.received();
            let total = resource.receiver.total();
            resource.dirty = false;
            resource.last_ack_ms = now_ms;
            resource.chunks_since_ack = 0;
            let progress = if received != resource.progress_reported {
                resource.progress_reported = received;
                Some((received, total))
            } else {
                None
            };
            AckSnapshot {
                peer: resource.peer,
                res_id: resource.manifest.res_id,
                msg_id: resource.manifest.msg_id,
                progress,
                ack: Ack {
                    kind: AckKind::ResourceWindow,
                    msg_id: resource.manifest.msg_id,
                    res_id: Some(resource.manifest.res_id),
                    next_expected: Some(next_expected),
                    bitmap: Some(bitmap),
                    timestamp_ms: now_ms,
                },
            }
        };
        if let Some((received, total)) = snapshot.progress {
            out.events.push(Event::ResourceProgress {
                peer: snapshot.peer,
                res_id: snapshot.res_id,
                received,
                total,
            });
        }
        tracing::debug!(
            peer = %snapshot.peer,
            res_id = snapshot.res_id,
            next_expected = snapshot.ack.next_expected,
            bitmap = snapshot
                .ack
                .bitmap
                .map(|bitmap| format!("{bitmap:#010x}"))
                .unwrap_or_default(),
            "window ack queued"
        );
        let _ = snapshot.msg_id;
        self.queue_ack(snapshot.peer, snapshot.ack, out);
    }

    fn send_resource_complete_ack(
        &mut self,
        peer: NodeId,
        manifest: &ResourceManifest,
        now_ms: u64,
        out: &mut Processed,
    ) {
        let ack = Ack {
            kind: AckKind::ResourceComplete,
            msg_id: manifest.msg_id,
            res_id: Some(manifest.res_id),
            next_expected: None,
            bitmap: None,
            timestamp_ms: now_ms,
        };
        self.queue_ack(peer, ack, out);
    }

    fn complete_incoming_resource(&mut self, id: ResourceKey, now_ms: u64, out: &mut Processed) {
        let Some(resource) = self.incoming.remove(&id) else {
            return;
        };
        let IncomingResource {
            peer,
            channel,
            manifest,
            receiver,
            key,
            ..
        } = resource;
        let Ok(blob) = receiver.into_blob() else {
            out.events
                .push(Event::Error("resource reassembly failed".into()));
            return;
        };
        let digest: [u8; 32] = Sha256::digest(&blob).into();
        if digest != manifest.sha256 {
            self.stats.dropped_frames += 1;
            out.events
                .push(Event::Error("resource sha256 mismatch".into()));
            return;
        }
        let aad = resource_aad(
            manifest.res_id,
            manifest.msg_id,
            manifest.total_len,
            manifest.chunk_bytes,
        );
        // The sealed blob starts with payload_flags(1) = 0; the AEAD ciphertext follows it.
        let sealed = &blob[1..];
        let data = match &key {
            TransferKey::Peer(ident) => self.identity.open(ident, &aad, sealed),
            TransferKey::Channel {
                key: channel_key, ..
            } => channel_key.open(&aad, sealed),
        };
        let data = match data {
            Ok(data) => data,
            Err(_) => {
                self.stats.dropped_frames += 1;
                out.events
                    .push(Event::Error("resource decrypt failed".into()));
                return;
            }
        };
        self.completed.insert(id, (now_ms, manifest.clone()));
        self.send_resource_complete_ack(peer, &manifest, now_ms, out);
        out.events.push(Event::ResourceReceived {
            peer,
            channel,
            manifest: manifest.clone(),
            data: data.clone(),
        });
        // Long text and channel history carry a CBOR application envelope as their plaintext:
        // surface it as a normal message so clients do not have to decode resources themselves.
        if manifest.kind == ResourceKind::LongText {
            self.surface_long_text(peer, channel, &manifest, &data, now_ms, out);
        }
    }

    fn surface_long_text(
        &mut self,
        peer: NodeId,
        channel: Option<NodeId>,
        manifest: &ResourceManifest,
        data: &[u8],
        now_ms: u64,
        out: &mut Processed,
    ) {
        match channel {
            None => {
                let Ok(message) = cbor_from_bytes::<AppMessage>(data) else {
                    return;
                };
                if message.id != manifest.msg_id {
                    return;
                }
                if self.mark_seen_msg(peer, message.id, now_ms) {
                    out.events.push(Event::Message(ReceivedMessage {
                        from: peer,
                        message: message.clone(),
                        link: LinkInfo::default(),
                        via_store: false,
                    }));
                    if let Some(position) = &message.position {
                        out.events.push(Event::Position {
                            from: peer,
                            channel: None,
                            position: position.clone(),
                        });
                    }
                    if let Some(telemetry) = &message.telemetry {
                        out.events.push(Event::Telemetry {
                            from: peer,
                            telemetry: telemetry.clone(),
                        });
                    }
                }
            }
            Some(channel_id) => {
                let Ok(group) = cbor_from_bytes::<crate::services::GroupMessage>(data) else {
                    return;
                };
                if group.sender != peer || group.message.id != manifest.msg_id {
                    return;
                }
                let Some(signer) = self.peer_identity(group.sender) else {
                    return;
                };
                let payload = group_signature_payload(&group.sender, &channel_id, &group.message);
                let Ok(signature) = <[u8; 64]>::try_from(group.signature.as_slice()) else {
                    return;
                };
                if signer.verify(&payload, &signature).is_err() {
                    out.events
                        .push(Event::Error("long-text group signature invalid".into()));
                    return;
                }
                if self.mark_seen_msg(group.sender, group.message.id, now_ms) {
                    out.events.push(Event::GroupMessage(ReceivedGroupMessage {
                        channel: channel_id,
                        sender: group.sender,
                        message: group.message.clone(),
                        link: LinkInfo::default(),
                    }));
                    if let Some(position) = &group.message.position {
                        out.events.push(Event::Position {
                            from: group.sender,
                            channel: Some(channel_id),
                            position: position.clone(),
                        });
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Resources: sender side timers
    // -----------------------------------------------------------------------------------------

    fn tick_outgoing_resources(&mut self, now_ms: u64, out: &mut Processed) {
        let ids: Vec<u32> = self.outgoing.keys().copied().collect();
        for res_id in ids {
            let Some(snapshot) = self.outgoing_snapshot(res_id, now_ms) else {
                continue;
            };
            if now_ms.saturating_sub(snapshot.started_ms) > RESOURCE_TRANSFER_TIMEOUT_MS {
                self.outgoing.remove(&res_id);
                out.events.push(Event::Error(format!(
                    "resource {res_id} timed out after {} retries",
                    RESOURCE_MAX_RETRIES
                )));
                continue;
            }
            // Paced emission: release at most `RESOURCE_TX_BATCH` never-sent chunks per tick. The
            // bridge ticks at 1 Hz, so a duty-cycle-paced companion TX path is not flooded by the
            // whole window in one burst (the live photo failure).
            let unsent = match self.outgoing.get_mut(&res_id) {
                Some(resource) => {
                    if now_ms.saturating_sub(resource.paced_ms) < self.resource_pace_interval_ms {
                        Vec::new()
                    } else {
                        resource.sender.take_unsent(RESOURCE_TX_BATCH as usize)
                    }
                }
                None => continue,
            };
            self.emit_resource_chunks(res_id, &unsent, out);
            if !unsent.is_empty() {
                // We are still streaming never-sent chunks: push the retransmission deadline out
                // so the RTO does not fire while the paced link is still delivering this batch.
                if let Some(resource) = self.outgoing.get_mut(&res_id) {
                    resource.paced_ms = now_ms;
                    resource.sender.deadline_ms = now_ms.saturating_add(RESOURCE_RTO_MS);
                    tracing::debug!(
                        res_id,
                        released = unsent.len(),
                        next_unsent = resource.sender.next_unsent,
                        total = resource.sender.total_chunks,
                        "paced resource batch released"
                    );
                }
                continue;
            }

            if now_ms < snapshot.deadline_ms {
                continue;
            }
            // Duty-aware retries: a companion radio FIFO holds one paced batch, so an RTO that
            // fires between paced boundaries must wait for the next boundary instead of emitting
            // a burst (this is what dropped the final tail chunks on hardware and stalled 32/34).
            if now_ms.saturating_sub(snapshot.paced_ms) < self.resource_pace_interval_ms {
                if let Some(resource) = self.outgoing.get_mut(&res_id) {
                    resource.sender.deadline_ms = snapshot
                        .paced_ms
                        .saturating_add(self.resource_pace_interval_ms);
                    tracing::debug!(
                        res_id,
                        acked = resource.sender.acked_count(),
                        total = resource.sender.total_chunks,
                        pending = snapshot.pending.len(),
                        paced_ms = snapshot.paced_ms,
                        deferred_deadline_ms = resource.sender.deadline_ms,
                        "resource retry deferred to the paced boundary"
                    );
                }
                continue;
            }
            // RTO: repair the manifest only when the receiver may not have the resource (no
            // acknowledged progress yet) or periodically to recover a restarted receiver state.
            // Once progress is being acknowledged, a retry is just one paced batch of missing
            // chunks - adding the manifest would push the burst past the radio FIFO again.
            let repair_manifest = snapshot.acked_count == 0 || snapshot.retries % 4 == 1;
            let manifest_frame = if repair_manifest {
                match &snapshot.key {
                    TransferKey::Peer(peer) => self.seal_frame(
                        FrameType::Data,
                        peer.node_id,
                        Flags::ENCRYPTED,
                        PAYLOAD_IS_MANIFEST,
                        snapshot.msg_id,
                        &snapshot.manifest_plaintext,
                        peer,
                        snapshot.include_identity,
                    ),
                    TransferKey::Channel { id, key } => self.seal_frame_channel(
                        FrameType::Data,
                        *id,
                        Flags::ENCRYPTED,
                        PAYLOAD_IS_MANIFEST,
                        snapshot.msg_id,
                        &snapshot.manifest_plaintext,
                        key,
                    ),
                }
                .ok()
            } else {
                None
            };
            let retry_ok = match self.outgoing.get_mut(&res_id) {
                Some(resource) => resource
                    .sender
                    .schedule_retry(
                        now_ms,
                        RESOURCE_RTO_MS,
                        RESOURCE_RETRY_BUDGET_MS,
                        RESOURCE_MAX_RETRY_BACKOFF_MS,
                    )
                    .is_some(),
                None => false,
            };
            if !retry_ok {
                self.outgoing.remove(&res_id);
                out.events.push(Event::Error(format!(
                    "resource {res_id} failed after {RESOURCE_MAX_RETRIES} retries without acknowledged progress"
                )));
                continue;
            }
            if let Some(frame) = manifest_frame {
                self.emit_frame(out, frame, self.send_mode_for(snapshot.target));
            }
            // One paced batch per retry, never the whole pending window: the radio transmits this
            // in one FIFO-sized burst and later paced retries repair the remaining chunks. Prefer
            // never-sent chunks (the tail) over resending already-delivered duplicates, otherwise a
            // small resource can wait for an ack round-trip before its final chunk is first sent.
            let batch = usize::from(RESOURCE_TX_BATCH).max(1);
            let mut pending = match self.outgoing.get_mut(&res_id) {
                Some(resource) => resource.sender.take_unsent(batch),
                None => Vec::new(),
            };
            if pending.len() < batch && !snapshot.pending.is_empty() {
                // Rotate the repair window across retries (retry 0 starts at chunk 0, retry 1 at
                // `batch`, ...). Always resending the first `batch` pending chunks can starve a
                // later lost chunk forever when acks lag behind retries (the 32/34 tail case).
                let window = snapshot.pending;
                let start = (usize::from(snapshot.retries).wrapping_mul(batch)) % window.len();
                for offset in 0..window.len() {
                    if pending.len() >= batch {
                        break;
                    }
                    let index = window[(start + offset) % window.len()];
                    if !pending.contains(&index) {
                        pending.push(index);
                    }
                }
            }
            if !pending.is_empty() {
                self.emit_resource_chunks(res_id, &pending, out);
                if let Some(resource) = self.outgoing.get_mut(&res_id) {
                    resource.paced_ms = now_ms;
                    resource.sender.deadline_ms =
                        now_ms.saturating_add(self.resource_pace_interval_ms.max(RESOURCE_RTO_MS));
                    tracing::debug!(
                        res_id,
                        retried = pending.len(),
                        acked = resource.sender.acked_count(),
                        total = resource.sender.total_chunks,
                        retries = resource.sender.retries,
                        manifest = repair_manifest,
                        next_deadline_ms = resource.sender.deadline_ms,
                        "paced resource retry released"
                    );
                }
            }
        }
    }

    fn outgoing_snapshot(&self, res_id: u32, _now_ms: u64) -> Option<OutgoingSnapshot> {
        let resource = self.outgoing.get(&res_id)?;
        let include_identity = match &resource.key {
            TransferKey::Peer(peer) => !self.peer_knows_us.contains(&peer.node_id),
            TransferKey::Channel { .. } => false,
        };
        Some(OutgoingSnapshot {
            msg_id: resource.msg_id,
            target: resource.target,
            key: resource.key.clone(),
            manifest_plaintext: resource.manifest_plaintext.clone(),
            pending: resource.sender.pending_window(),
            include_identity,
            started_ms: resource.started_ms,
            deadline_ms: resource.sender.deadline_ms,
            paced_ms: resource.paced_ms,
            acked_count: resource.sender.acked_count(),
            retries: resource.sender.retries,
        })
    }

    fn tick_incoming_resources(&mut self, now_ms: u64, out: &mut Processed) {
        let ids: Vec<ResourceKey> = self
            .incoming
            .iter()
            .filter(|(_, resource)| {
                resource.dirty
                    && now_ms.saturating_sub(resource.last_ack_ms) >= RESOURCE_ACK_DELAY_MS
            })
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.send_window_ack(id, now_ms, out);
        }
    }

    fn tick_pending_messages(&mut self, now_ms: u64, out: &mut Processed) {
        let due: Vec<u32> = self
            .pending_msgs
            .iter()
            .filter(|(_, pending)| now_ms.saturating_sub(pending.last_tx_ms) >= ACK_TIMEOUT_MS)
            .map(|(msg_id, _)| *msg_id)
            .collect();
        for msg_id in due {
            let Some((peer, plaintext, include_identity, attempts)) =
                self.pending_msgs.get(&msg_id).map(|pending| {
                    (
                        pending.peer,
                        pending.plaintext.clone(),
                        pending.include_identity,
                        pending.attempts,
                    )
                })
            else {
                continue;
            };
            if attempts >= MAX_MSG_RETRIES {
                self.pending_msgs.remove(&msg_id);
                out.events.push(Event::Error(format!(
                    "message {msg_id} unacked after {MAX_MSG_RETRIES} retries"
                )));
                continue;
            }
            if let Some(pending) = self.pending_msgs.get_mut(&msg_id) {
                pending.attempts += 1;
                pending.last_tx_ms = now_ms;
            }
            match self.seal_frame(
                FrameType::Data,
                peer.node_id,
                Flags::ENCRYPTED | Flags::ACK_REQUESTED,
                0,
                msg_id,
                &plaintext,
                &peer,
                include_identity,
            ) {
                Ok(frame) => self.emit_frame(out, frame, SendMode::Direct(peer.node_id)),
                Err(_) => {
                    self.pending_msgs.remove(&msg_id);
                }
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Pruning
    // -----------------------------------------------------------------------------------------

    fn prune_peers(&mut self, now_ms: u64) {
        let me = self.node_id();
        self.heard_at
            .retain(|id, seen| now_ms.saturating_sub(*seen) < PEER_EXPIRY_MS && *id != me);
        let contact_ids: HashSet<NodeId> = self.contacts.keys().copied().collect();
        self.peers.retain(|id, peer| {
            contact_ids.contains(id)
                || peer.info.is_contact
                || now_ms.saturating_sub(peer.info.last_seen_ms) < PEER_EXPIRY_MS
        });
    }

    fn prune_seen(&mut self, now_ms: u64) {
        self.seen_msgs
            .retain(|_, seen| now_ms.saturating_sub(*seen) < SEEN_MSG_TTL_MS);
        self.seen_refs
            .retain(|_, seen| now_ms.saturating_sub(*seen) < SEEN_MSG_TTL_MS);
        self.completed
            .retain(|_, (done, _)| now_ms.saturating_sub(*done) < RESOURCE_KEEP_MS);
        self.ack_sent_at
            .retain(|_, at| now_ms.saturating_sub(*at) < SEEN_MSG_TTL_MS);
        self.store_offer_at
            .retain(|_, at| now_ms.saturating_sub(*at) < STORE_OFFER_INTERVAL_MS.saturating_mul(4));
        if self.seen_msgs.len() > SEEN_MSG_CAPACITY {
            trim_times(&mut self.seen_msgs, SEEN_MSG_CAPACITY);
        }
        if self.seen_refs.len() > SEEN_MSG_CAPACITY {
            trim_times(&mut self.seen_refs, SEEN_MSG_CAPACITY);
        }
    }
}

#[derive(Clone, Debug)]
enum ChunkOutcome {
    Inserted { complete: bool, ack_now: bool },
    Duplicate,
    Rejected,
}

#[derive(Clone, Debug)]
struct AckSnapshot {
    peer: NodeId,
    res_id: u32,
    msg_id: u32,
    progress: Option<(u16, u16)>,
    ack: Ack,
}

/// Keep at most `capacity` entries in a timestamp map by dropping the oldest.
fn trim_times<K: core::hash::Hash + Eq + Copy>(map: &mut HashMap<K, u64>, capacity: usize) {
    if map.len() <= capacity {
        return;
    }
    let mut entries: Vec<(u64, K)> = map.iter().map(|(key, value)| (*value, *key)).collect();
    entries.sort_unstable_by_key(|(value, _)| *value);
    let excess = map.len().saturating_sub(capacity);
    for (_, key) in entries.into_iter().take(excess) {
        map.remove(&key);
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

/// Compute the expected acknowledgement bitmap for a set of received chunk indices
/// (test/vector helper).
pub fn expected_ack_bitmap(total_chunks: u16, next_expected: u16, received: &[u16]) -> u32 {
    ack_bitmap(total_chunks, next_expected, |index| {
        received.contains(&index)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Identity;

    fn engine(seed: u8) -> Engine {
        engine_from_seed(&[seed; 32], EngineConfig::client(format!("node-{seed}")))
    }

    #[test]
    fn sealed_body_layout_offsets() {
        let ident = Identity::from_seed(&[5u8; 32]).public();
        let body = sealed_body(
            PAYLOAD_IDENTITY_INCLUDED,
            0x0102_0304,
            Some(&ident),
            &[9, 9],
        );
        assert_eq!(body[0], PAYLOAD_IDENTITY_INCLUDED);
        assert_eq!(&body[1..5], &0x0102_0304u32.to_be_bytes());
        assert_eq!(&body[5..77], &ident.to_bytes());
        assert_eq!(&body[77..], &[9, 9]);
        let (flags, msg_ref, hint, sealed) = parse_sealed_body(&body).unwrap();
        assert_eq!(flags, PAYLOAD_IDENTITY_INCLUDED);
        assert_eq!(msg_ref, 0x0102_0304);
        assert_eq!(hint, Some(ident));
        assert_eq!(sealed, &[9, 9]);

        let body = sealed_body(0, 7, None, &[1, 2, 3]);
        assert_eq!(body.len(), 8);
        let (flags, msg_ref, hint, sealed) = parse_sealed_body(&body).unwrap();
        assert_eq!((flags, msg_ref, hint), (0, 7, None));
        assert_eq!(sealed, &[1, 2, 3]);

        assert!(parse_sealed_body(&[]).is_err());
        assert!(parse_sealed_body(&[0x08, 0, 0, 0, 0]).is_err());
        assert!(parse_sealed_body(&[PAYLOAD_IDENTITY_INCLUDED, 0, 0, 0, 0]).is_err());
    }

    #[test]
    fn chunk_body_roundtrip() {
        let body = chunk_body(0xAA, 0xBB, 3, &[1, 2, 3, 4]);
        assert_eq!(body.len(), 15);
        let (msg_ref, res_id, index, slice) = parse_chunk_body(&body).unwrap();
        assert_eq!((msg_ref, res_id, index), (0xAA, 0xBB, 3));
        assert_eq!(slice, &[1, 2, 3, 4]);
        assert!(parse_chunk_body(&[0u8; 3]).is_err());
        let mut bad = body.clone();
        bad[0] = 1;
        assert!(parse_chunk_body(&bad).is_err());
    }

    #[test]
    fn aad_ignores_ttl_but_not_dst() {
        let mut frame = Frame {
            typ: FrameType::Data,
            flags: Flags::from_bits(Flags::ENCRYPTED),
            ttl: 5,
            src: NodeId([1; 8]),
            dst: NodeId([2; 8]),
            seq: 9,
            body: vec![1],
        };
        let aad = frame.aad();
        frame.ttl = 3;
        assert_eq!(frame.aad(), aad);
        frame.dst = NodeId([3; 8]);
        assert_ne!(frame.aad(), aad);
    }

    #[test]
    fn originate_rate_limit() {
        let mut e = engine(1);
        let mut emitted = 0;
        for _ in 0..MAX_ORIGINATE_PER_MIN + 5 {
            if e.can_originate(0) {
                emitted += 1;
            }
        }
        assert_eq!(emitted, MAX_ORIGINATE_PER_MIN as usize);
        assert!(e.can_originate(60_000));
    }

    #[test]
    fn announce_is_binary_and_relays_are_deduped() {
        let mut a = engine(1);
        let out = a.announce(0).unwrap();
        assert_eq!(out.outbound.len(), 1);
        let frame = out.outbound[0].frame.clone();
        assert_eq!(frame.typ, FrameType::Announce);
        assert_eq!(frame.ttl, ANNOUNCE_TTL);
        assert!(frame.dst.is_broadcast());
        let (decoded, _sig) = Announce::decode_signed(&frame.body).unwrap();
        assert_eq!(decoded.ident, a.public_identity());

        let mut b = engine(2);
        let processed = b.on_frame(frame.clone(), None, 10);
        assert!(
            matches!(processed.events[0], Event::NodeSeen(ref info) if info.node_id == a.node_id())
        );
        assert_eq!(b.stats().known_nodes, 1);
        // A relayed copy of the same announce is suppressed by dedup.
        let relayed = frame.relay_copy().unwrap();
        let duplicate = b.on_frame(relayed, None, 20);
        assert!(duplicate.events.is_empty());
        assert_eq!(b.stats().dedup_hits, 1);
    }

    #[test]
    fn export_import_roundtrip() {
        let mut a = engine(7);
        let contact = Identity::from_seed(&[8u8; 32]).public();
        a.add_contact(contact, Some("Bob".into()));
        let channel = a.add_channel("family", &[3u8; 32]);
        a.announce(123).unwrap();
        let snapshot = a.export_state();
        assert_eq!(snapshot.contacts.len(), 1);
        assert_eq!(snapshot.channels.len(), 1);
        assert_eq!(snapshot.last_seq, a.seq);

        let mut b = engine_from_seed(&[7u8; 32], EngineConfig::client("node-7"));
        b.import_state(snapshot);
        assert!(b.node_info(contact.node_id).unwrap().is_contact);
        assert_eq!(b.channels(), vec![(channel, "family".to_string())]);
        assert_eq!(b.seq, a.seq);
    }

    #[test]
    fn engine_config_carries_the_regulatory_override() {
        let region = crate::util::region("EU868").expect("EU868");
        let mut config = EngineConfig::client("node");
        assert_eq!(config.effective_duty_pct(region), Some(10));
        assert!(!config.regulatory.is_active());
        config.regulatory = crate::regulatory::RegulatoryOverride {
            enabled: true,
            acknowledged: true,
            duty_cycle_pct: Some(100),
            ..Default::default()
        };
        assert_eq!(config.effective_duty_pct(region), Some(100));
        // An engine built from that config keeps the override (no wire effect).
        let engine = Engine::new(Identity::from_seed(&[1u8; 32]), config);
        assert_eq!(engine.config().effective_duty_pct(region), Some(100));
        assert!(engine.config().regulatory.is_active());
    }

    #[test]
    fn malformed_input_never_panics() {
        let mut e = engine(3);
        for len in 0..300usize {
            let bytes = vec![0xFFu8; len];
            let _ = e.ingest(&bytes, None, len as u64);
        }
        let _ = e.ingest(&[], None, 0);
    }

    #[test]
    fn resource_pace_hint_defaults_and_clamps() {
        let mut e = engine(11);
        assert_eq!(e.resource_pace_interval_ms(), RESOURCE_PACE_INTERVAL_MS);
        e.set_resource_pace_interval_ms(12_220);
        assert_eq!(e.resource_pace_interval_ms(), 12_220);
        e.set_resource_pace_interval_ms(0);
        assert_eq!(e.resource_pace_interval_ms(), 1);
    }

    #[test]
    fn tail_completes_with_two_chunk_acks_and_delayed_final_ack() {
        // The Android app can ack a couple of chunks at a time, and the final ack may be delayed.
        // This variant drives the sender with explicit 2-chunk window acks (instead of the default
        // 8-chunk/5 s cadence), holds the final ack back by a minute, and limits the radio FIFO to
        // one paced batch. The sender must keep releasing/retrying the tail at the paced rate and
        // finish once the delayed ack arrives.
        use crate::services::{Ack, AckKind};
        let mut a = engine(71);
        let mut b = engine(72);
        let a_id = a.node_id();
        let b_id = b.node_id();
        a.add_contact(b.public_identity(), Some("b".into()));
        b.add_contact(a.public_identity(), Some("a".into()));
        a.set_resource_pace_interval_ms(12_220);
        let data = vec![7u8; 6018];
        let res_id = 0x7171_0001u32;
        let msg_id = 0x7171_0002u32;
        let start = a
            .send_resource(
                Target::Direct(b_id),
                msg_id,
                res_id,
                ResourceKind::Attachment,
                "image/webp",
                &data,
                0,
            )
            .expect("send");
        let fifo_capacity = usize::from(RESOURCE_TX_BATCH) + 1;
        let mut a_frames: Vec<Outbound> = start.outbound;
        let mut now = 0u64;
        let mut last_acked = 0u16;
        let mut final_ack_due: Option<u64> = None;
        let mut completed = false;
        for _step in 0..2400u32 {
            now += 1_000;
            let mut next_a: Vec<Outbound> = Vec::new();
            for out in a_frames.drain(..).take(fifo_capacity) {
                let b_out = b.on_frame(out.frame, None, now);
                for event in &b_out.events {
                    if matches!(event, Event::ResourceReceived { .. }) {
                        completed = true;
                    }
                }
                for ack in b_out.outbound {
                    let a_out = a.on_frame(ack.frame, None, now);
                    next_a.extend(a_out.outbound);
                }
            }
            // Explicit 2-chunk window acks, final ack delayed by 60 s after it is first due.
            let received = b
                .incoming
                .values()
                .map(|resource| resource.receiver.reassembler.received_chunks())
                .max()
                .unwrap_or(0);
            if received >= last_acked.saturating_add(2) {
                let (next_expected, bitmap) = b
                    .incoming
                    .values()
                    .map(|resource| resource.receiver.ack_window())
                    .next()
                    .unwrap_or((received, 0));
                let due = *final_ack_due.get_or_insert_with(|| {
                    if next_expected >= 34 {
                        now + 60_000
                    } else {
                        now
                    }
                });
                if now >= due {
                    let ack = Ack {
                        kind: AckKind::ResourceWindow,
                        msg_id,
                        res_id: Some(res_id),
                        next_expected: Some(next_expected),
                        bitmap: Some(bitmap),
                        timestamp_ms: now,
                    };
                    let ack_out = b.send_ack(a_id, ack, now).expect("ack");
                    for frame in ack_out.outbound {
                        let a_out = a.on_frame(frame.frame, None, now);
                        next_a.extend(a_out.outbound);
                    }
                    last_acked = next_expected;
                    if next_expected >= 34 {
                        final_ack_due = None;
                    }
                }
            }
            let a_out = a.tick(now);
            next_a.extend(a_out.outbound);
            // Ignore the receiver's default periodic acks: this test controls the ack cadence.
            let _ = b.tick(now);
            a_frames = next_a;
            if completed {
                break;
            }
        }
        assert!(completed, "tail did not complete after {now} ms");
        assert!(!a.outgoing.contains_key(&res_id));
    }

    #[test]
    fn dropped_tail_is_retried_until_complete() {
        // Hardware-equivalent tail case: 34 chunks, batch 2, duty-aware pace, a radio FIFO that
        // only absorbs one paced batch per engine emission (extra frames in the same tick are
        // dropped). The final three chunks are lost on first transmission, so only the retry path
        // can finish the transfer. Before the paced-retry fix, each RTO emitted manifest + up to
        // four chunks at once, the FIFO dropped the tail of every burst, and the transfer froze at
        // 32/34 - this test would time out.
        let mut a = engine(61);
        let mut b = engine(62);
        a.add_contact(b.public_identity(), Some("b".into()));
        b.add_contact(a.public_identity(), Some("a".into()));
        a.set_resource_pace_interval_ms(12_220);
        let b_id = b.node_id();
        let data = vec![6u8; 6018];
        let res_id = 0x6161_0001u32;
        let start = a
            .send_resource(
                Target::Direct(b_id),
                0x6161_0002,
                res_id,
                ResourceKind::Attachment,
                "image/webp",
                &data,
                0,
            )
            .expect("send");
        let fifo_capacity = usize::from(RESOURCE_TX_BATCH) + 1;
        let mut a_frames: Vec<Outbound> = start.outbound;
        let mut lost: std::collections::HashSet<u16> = [31u16, 32, 33].into_iter().collect();
        let mut now = 0u64;
        let mut b_received = 0u16;
        let mut completed = false;
        for _step in 0..1800u32 {
            now += 1_000;
            let mut next_a: Vec<Outbound> = Vec::new();
            // Radio FIFO: only the first `fifo_capacity` frames of an engine emission survive.
            for out in a_frames.drain(..).take(fifo_capacity) {
                if out.frame.typ == FrameType::Resource && out.frame.body.len() >= 11 {
                    let index = u16::from_be_bytes([out.frame.body[9], out.frame.body[10]]);
                    if lost.remove(&index) {
                        continue;
                    }
                }
                let b_out = b.on_frame(out.frame, None, now);
                for event in &b_out.events {
                    if let Event::ResourceProgress { received, .. } = event {
                        b_received = *received;
                    }
                    if matches!(event, Event::ResourceReceived { .. }) {
                        completed = true;
                    }
                }
                for ack in b_out.outbound {
                    let a_out = a.on_frame(ack.frame, None, now);
                    next_a.extend(a_out.outbound);
                }
            }
            let a_out = a.tick(now);
            next_a.extend(a_out.outbound);
            let b_out = b.tick(now);
            for ack in b_out.outbound {
                let a_out = a.on_frame(ack.frame, None, now);
                next_a.extend(a_out.outbound);
            }
            a_frames = next_a;
            if completed {
                break;
            }
        }
        assert!(
            completed,
            "tail did not recover: receiver has {b_received}/34 chunks after {now} ms"
        );
        assert_eq!(b_received, 34);
        assert!(
            !a.outgoing.contains_key(&res_id),
            "sender still has the resource"
        );
    }

    #[test]
    fn retry_bursts_stay_within_one_paced_batch() {
        // Regression repro for the 32/34 tail stall: the RTO path used to emit a manifest plus up
        // to `RESOURCE_TX_BATCH * 2` pending chunks in one tick, ignoring the duty-aware pace. The
        // companion radio FIFO holds one paced batch, so every tail retransmission overflowed it
        // and the final chunks were never delivered.
        //
        // No receiver here: we only assert the sender's per-tick emission shape. 34 chunks, batch
        // 2, paced interval, no acks, so the window fills and every later emission is a retry.
        let mut e = engine(51);
        e.set_resource_pace_interval_ms(12_220);
        let channel = e.add_channel("retry-burst", &[3u8; 32]);
        let data = vec![5u8; 6018];
        e.send_resource(
            Target::Channel(channel),
            0x7777_0001,
            0x7777_0002,
            ResourceKind::Attachment,
            "image/webp",
            &data,
            0,
        )
        .expect("send");

        let max_burst = usize::from(RESOURCE_TX_BATCH) + 1; // one paced batch plus its manifest
        let mut max_seen = 0usize;
        let mut retry_bursts = Vec::new();
        for step in 0..600u32 {
            let now = u64::from(step) * 1_000;
            let processed = e.tick(now);
            let frames = processed.outbound.len();
            if frames > 0 {
                if frames > max_burst {
                    retry_bursts.push((now, frames));
                }
                max_seen = max_seen.max(frames);
            }
            assert!(
                frames <= max_burst,
                "tick at {now} emitted {frames} frames in one burst (max {max_burst});                  tail retries must stay within one paced batch"
            );
        }
        assert!(max_seen > 0, "sender emitted nothing");
        assert!(
            retry_bursts.is_empty(),
            "bursty retry emissions observed: {retry_bursts:?}"
        );
    }

    #[test]
    fn restart_without_sequence_floor_is_dropped_as_duplicate() {
        // Bridge-restart media stall: a continuously running peer deduplicates frames by
        // `(src, seq)` for DEDUP_TTL_MS. If the restarted sender starts at seq 0 again, every new
        // frame looks like a duplicate and is dropped before it can be acknowledged. Restoring the
        // persisted sequence floor fixes it.
        let mut sender = engine_from_seed(&[9u8; 32], EngineConfig::client("bridge"));
        let mut phone = engine_from_seed(&[10u8; 32], EngineConfig::client("phone"));
        sender.add_contact(phone.public_identity(), Some("phone".into()));
        phone.add_contact(sender.public_identity(), Some("bridge".into()));

        let first = sender
            .send_text(Target::Direct(phone.node_id()), "before restart", 1_000)
            .expect("first message");
        let mut first_message = 0;
        for out in first.outbound {
            let processed = phone.on_frame(out.frame, None, 1_000);
            first_message += processed
                .events
                .iter()
                .filter(|event| matches!(event, Event::Message(_)))
                .count();
        }
        assert_eq!(first_message, 1, "pre-restart message must arrive");
        let old_seq = sender.last_seq();
        assert!(old_seq > 0);

        // Unfixed restart: same identity and a freshly zeroed sequence counter.
        let mut restarted = engine_from_seed(&[9u8; 32], EngineConfig::client("bridge"));
        restarted.add_contact(phone.public_identity(), Some("phone".into()));
        let collided = restarted
            .send_text(Target::Direct(phone.node_id()), "after restart", 2_000)
            .expect("collided message");
        let dedup_before = phone.stats().dedup_hits;
        for out in collided.outbound {
            let processed = phone.on_frame(out.frame, None, 2_000);
            assert!(
                !processed
                    .events
                    .iter()
                    .any(|event| matches!(event, Event::Message(_))),
                "colliding seq frame must not surface as a new message"
            );
        }
        assert!(
            phone.stats().dedup_hits > dedup_before,
            "the restarted sender's reused sequence numbers must be deduplicated"
        );

        // Restoring the persisted floor makes the next frame acceptable again.
        restarted.set_sequence_floor(old_seq);
        let fixed = restarted
            .send_text(
                Target::Direct(phone.node_id()),
                "after sequence floor",
                3_000,
            )
            .expect("post-floor message");
        let mut delivered = false;
        for out in fixed.outbound {
            let processed = phone.on_frame(out.frame, None, 3_000);
            delivered |= processed
                .events
                .iter()
                .any(|event| matches!(event, Event::Message(_)));
        }
        assert!(
            delivered,
            "a restored sequence floor must let frames through"
        );
    }

    #[test]
    fn asymmetric_ack_loss_does_not_deadlock_sender() {
        // Hardware symptom: phone→bridge ACK direction is lossy while bridge→phone data is clean.
        // The sender must keep retrying and the receiver must re-ack duplicate chunks; dropping an
        // entire minute of window ACKs must not deadlock the transfer.
        let mut sender = engine(81);
        let mut phone = engine(82);
        sender.add_contact(phone.public_identity(), Some("phone".into()));
        phone.add_contact(sender.public_identity(), Some("bridge".into()));
        sender.set_resource_pace_interval_ms(2_000);
        let phone_id = phone.node_id();
        let data = vec![11u8; 1_200];
        let res_id = 0x8181_0001u32;
        let start = sender
            .send_resource(
                Target::Direct(phone_id),
                0x8181_0002,
                res_id,
                ResourceKind::Attachment,
                "image/webp",
                &data,
                0,
            )
            .expect("send");
        let drop_acks_until = 60_000u64;
        let mut frames: Vec<Outbound> = start.outbound;
        let mut now = 0u64;
        let mut completed = false;
        let mut phone_received = false;
        for _step in 0..300u32 {
            now += 1_000;
            let mut next: Vec<Outbound> = Vec::new();
            for out in frames.drain(..) {
                let received = phone.on_frame(out.frame, None, now);
                phone_received |= received
                    .events
                    .iter()
                    .any(|event| matches!(event, Event::ResourceReceived { .. }));
                for ack in received.outbound {
                    if now >= drop_acks_until {
                        let applied = sender.on_frame(ack.frame, None, now);
                        next.extend(applied.outbound);
                    }
                }
            }
            next.extend(sender.tick(now).outbound);
            // Do not let the phone's quiet-timer acks through during the lossy window either.
            let phone_tick = phone.tick(now);
            phone_received |= phone_tick
                .events
                .iter()
                .any(|event| matches!(event, Event::ResourceReceived { .. }));
            for ack in phone_tick.outbound {
                if now >= drop_acks_until {
                    let applied = sender.on_frame(ack.frame, None, now);
                    next.extend(applied.outbound);
                }
            }
            frames = next;
            if !sender.outgoing.contains_key(&res_id) {
                completed = true;
                break;
            }
        }
        assert!(
            completed,
            "sender deadlocked after asymmetric ACK loss (resource still outgoing)"
        );
        assert!(
            phone_received,
            "phone must have received the whole resource"
        );
        assert!(
            sender.stats().acks_rx > 0,
            "accepted ACK counter must advance once the ACK direction recovers"
        );
    }

    #[test]
    fn window_ack_counter_reports_accepted_acks() {
        // Dedicated counter for the controlled `--log debug` ACK-uplink diagnosis: a window ACK
        // that is received, decrypted and applied must increment `window_acks_rx` (while
        // `rx_frames` remains the all-frame counter).
        let mut sender = engine(91);
        let mut phone = engine(92);
        sender.add_contact(phone.public_identity(), Some("phone".into()));
        phone.add_contact(sender.public_identity(), Some("bridge".into()));
        let phone_id = phone.node_id();
        let data = vec![13u8; 600];
        let start = sender
            .send_resource(
                Target::Direct(phone_id),
                0x9191_0001,
                0x9191_0002,
                ResourceKind::Attachment,
                "image/webp",
                &data,
                0,
            )
            .expect("send");
        for out in start.outbound {
            let _ = phone.on_frame(out.frame, None, 0);
        }
        // The receiver's quiet-timer tick emits a window ACK for the two chunks it has.
        let ack_out = phone.tick(6_000);
        let mut delivered_ack = false;
        for out in ack_out.outbound {
            let processed = sender.on_frame(out.frame, None, 6_000);
            delivered_ack |= processed
                .events
                .iter()
                .any(|event| matches!(event, Event::ResourceProgress { .. }));
        }
        assert!(delivered_ack);
        assert_eq!(sender.stats().window_acks_rx, 1);
        assert!(sender.stats().acks_rx >= 1);
        assert!(sender.stats().rx_frames >= 1);
    }

    #[test]
    fn persisted_msg_counter_prevents_id_reuse_after_restart() {
        let seed = [7u8; 32];
        let mut a = engine_from_seed(&seed, EngineConfig::client("restart"));
        let first = a.allocate_message_id();
        let snapshot = a.export_state();
        assert_eq!(snapshot.msg_id_epoch, MSG_ID_EPOCH);
        assert_eq!(snapshot.msg_counter, 1);
        assert_eq!(snapshot.last_seq, a.seq);

        // Simulate an app restart: same identity, state restored from the persisted snapshot.
        let mut b = engine_from_seed(&seed, EngineConfig::client("restart"));
        b.import_state(snapshot);
        let second = b.allocate_message_id();
        assert_ne!(
            first, second,
            "a restart must not reuse the previous message id"
        );
        assert!(b.msg_counter > 1);
    }

    #[test]
    fn legacy_snapshot_cannot_rederive_old_message_ids() {
        // Snapshots written before task-16 have no counter/epoch: import must start at the legacy
        // floor with the current epoch so ids issued by old engines (counter 1..N, epoch 0) never
        // reappear.
        let mut e = engine_from_seed(&[8u8; 32], EngineConfig::client("legacy"));
        e.import_state(StateSnapshot {
            contacts: Vec::new(),
            channels: Vec::new(),
            last_seq: 0,
            msg_counter: 0,
            msg_id_epoch: 0,
        });
        let id = e.allocate_message_id();
        assert!(e.msg_counter >= LEGACY_MSG_COUNTER_START);

        let seed = e.identity.seed();
        let legacy_ids: std::collections::HashSet<u32> = (1u32..=32)
            .map(|counter| {
                let mut hasher = Sha256::new();
                hasher.update(b"openmesh/v1/msg-id");
                hasher.update(seed);
                hasher.update(counter.to_be_bytes());
                let digest: [u8; 32] = hasher.finalize().into();
                u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
            })
            .collect();
        assert!(
            !legacy_ids.contains(&id),
            "legacy-derived ids must never be reissued"
        );
    }

    #[test]
    fn stalled_attachment_id_is_not_reused_for_the_next_message() {
        let mut e = engine(13);
        let channel = e.add_channel("media", &[6u8; 32]);
        let image_id = e.allocate_message_id();
        let image = vec![7u8; 400];
        let sent = e
            .send_resource(
                Target::Channel(channel),
                image_id,
                0x1111_2222,
                ResourceKind::Attachment,
                "image/webp",
                &image,
                0,
            )
            .expect("image resource");
        assert!(!sent.outbound.is_empty());
        assert!(
            e.outgoing
                .values()
                .any(|resource| resource.msg_id == image_id)
        );

        let voice_id = e.allocate_message_id();
        assert_ne!(
            image_id, voice_id,
            "the stalled image id must not be reused"
        );
        let voice = vec![8u8; 308];
        e.send_resource(
            Target::Channel(channel),
            voice_id,
            0x3333_4444,
            ResourceKind::Attachment,
            "audio/c2-1300",
            &voice,
            1,
        )
        .expect("voice resource");
    }

    #[test]
    fn resource_pace_hint_limits_tick_release() {
        let mut e = engine(12);
        e.set_resource_pace_interval_ms(10_000);
        let channel = e.add_channel("pace", &[9u8; 32]);
        let data: Vec<u8> = (0..900u32).map(|i| (i % 251) as u8).collect();
        let sent = e
            .send_resource(
                Target::Channel(channel),
                1,
                2,
                ResourceKind::Attachment,
                "image/webp",
                &data,
                0,
            )
            .expect("send");
        let resource_frames = |processed: &Processed| {
            processed
                .outbound
                .iter()
                .filter(|outbound| outbound.frame.typ == FrameType::Resource)
                .count()
        };
        assert_eq!(
            resource_frames(&sent),
            2,
            "only the first paced batch is released"
        );
        assert_eq!(
            resource_frames(&e.tick(1_000)),
            0,
            "interval has not elapsed yet"
        );
        assert_eq!(
            resource_frames(&e.tick(10_000)),
            2,
            "one interval releases one batch"
        );
        assert_eq!(
            resource_frames(&e.tick(20_000)),
            2,
            "the remaining chunks follow"
        );
    }
}

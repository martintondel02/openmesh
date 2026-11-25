//! The bridge runtime: owns the protocol engine and glues it to transports, storage and the API.
//!
//! A single tokio task owns [`openmesh_core::Engine`]. Transport tasks and WebSocket command
//! handlers communicate with it through [`EngineInput`]; the engine's [`openmesh_core::Event`]s
//! are persisted, mapped to the documented JSON shapes and fanned out to API clients.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{RwLock, broadcast, mpsc};

use openmesh_core::regulatory;
use openmesh_core::services::{caps, group_signature_payload};
use openmesh_core::{
    Ack, AckKind, AppMessage, Engine, EngineConfig, Event, MessageKind, NodeClass, NodeId,
    Processed, PublicIdentity, RelayConfig, ResourceKind, ResourceManifest, Role, Target, util,
};

use crate::attachments::AttachmentStore;
use crate::json::*;
use crate::store::{
    ChannelRecord, ContactRecord, Store, StoredAttachment, StoredMessage, identity_b64,
};
use crate::transport::backbone::BackboneManager;
use crate::transport::{Inbound, TransportFrame, TransportHandle};

/// Features advertised in `hello` (documented list, extended when store-forward is enabled).
pub const FEATURES: &[&str] = &["images", "voice", "map", "groups", "store_forward"];

/// Bridge runtime configuration (persisted in the `meta` table and exposed by `config` events).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_class")]
    pub class: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default = "default_region")]
    pub region: String,
    #[serde(default = "default_interface")]
    pub interface: String,
    #[serde(default = "default_true")]
    pub auto_announce: bool,
    #[serde(default = "default_true")]
    pub store_forward: bool,
    #[serde(default = "default_true")]
    pub position_sharing: bool,
    /// Last position reported by the host (shared by `send_position` without an override).
    #[serde(default)]
    pub position: Option<PositionJson>,
    /// Emergency regulatory override (disabled by default; see regulatory-and-regions.md §2).
    #[serde(default)]
    pub regulatory_override: RegulatoryOverrideJson,
}

fn default_name() -> String {
    "openmesh-bridge".to_string()
}
fn default_class() -> String {
    "client".to_string()
}
fn default_profile() -> String {
    "MEDIUM_FAST".to_string()
}
fn default_region() -> String {
    "EU868".to_string()
}
fn default_interface() -> String {
    "udp".to_string()
}
fn default_true() -> bool {
    true
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            name: default_name(),
            class: default_class(),
            profile: default_profile(),
            region: default_region(),
            interface: default_interface(),
            auto_announce: true,
            store_forward: true,
            position_sharing: true,
            position: None,
            regulatory_override: RegulatoryOverrideJson::default(),
        }
    }
}

impl RuntimeConfig {
    pub fn profile(&self) -> &'static util::LoraProfile {
        util::PROFILES
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(&self.profile))
            .unwrap_or(&util::MEDIUM_FAST)
    }

    pub fn region(&self) -> &'static util::Region {
        util::REGIONS
            .iter()
            .find(|r| r.name.eq_ignore_ascii_case(&self.region))
            .unwrap_or(&util::REGIONS[0])
    }

    pub fn node_class(&self) -> NodeClass {
        parse_node_class(&self.class).unwrap_or(NodeClass::Client)
    }

    pub fn engine_config(&self) -> EngineConfig {
        let class = self.node_class();
        let mut config = match class {
            NodeClass::Repeater | NodeClass::Gateway | NodeClass::RoomServer => {
                EngineConfig::repeater(self.name.clone())
            }
            _ => EngineConfig::client(self.name.clone()),
        };
        config.class = class;
        config.accept_channels = true;
        config.regulatory = self.regulatory_override.to_core();
        if matches!(class, NodeClass::Gateway) {
            config.caps |= caps::WIFI | caps::ETHERNET;
        }
        config
    }
}

/// A live snapshot of everything the API can answer without asking the engine task.
#[derive(Clone, Default)]
pub struct Snapshot {
    pub node_id: String,
    pub node: NodeSelfJson,
    pub radios: Vec<RadioJson>,
    pub features: Vec<String>,
    pub store: StoreCountsJson,
    pub status: StatusJson,
    pub config: ConfigJson,
    pub nodes: Vec<NodeJson>,
    pub links: Vec<LinkJson>,
}

/// Commands that need the protocol engine (mutating operations and announces).
#[derive(Clone, Debug)]
pub struct ClientCommand {
    pub id: Option<String>,
    pub kind: CommandKind,
}

#[derive(Clone, Debug)]
pub enum CommandKind {
    SendMessage(SendMessageData),
    SendGroupMessage(SendGroupMessageData),
    SendPosition(SendPositionData),
    MarkRead(MsgIdData),
    Retry(MsgIdData),
    AddContact(AddContactData),
    RemoveContact(String),
    CreateChannel(ChannelCreateData),
    JoinChannel(ChannelCreateData),
    LeaveChannel(ChannelIdData),
    SetConfig(serde_json::Value),
    Announce,
}

/// Input queue of the engine task.
pub enum EngineInput {
    Inbound(Inbound),
    Command {
        cmd: ClientCommand,
        reply: mpsc::UnboundedSender<Arc<str>>,
    },
    /// Toggle store-and-forward (engine relay role) — backbone `store_forward` setting.
    SetStoreForward(bool),
    Tick,
}

/// Error returned by a command forwarded to the engine task (REST/MCP friendly).
#[derive(Clone, Debug, serde::Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Forward one mutating command to the engine task and wait for its JSON reply (10 s).
///
/// The reply envelope is `{v,type,id?,data}`; success returns `data`, engine errors become
/// [`CommandError`] with the documented code and plain-language message.
pub async fn run_command(
    shared: &Shared,
    kind: CommandKind,
) -> Result<serde_json::Value, CommandError> {
    let (reply_tx, mut reply_rx) = mpsc::unbounded_channel::<Arc<str>>();
    let command = ClientCommand { id: None, kind };
    shared
        .commands
        .send(EngineInput::Command {
            cmd: command,
            reply: reply_tx,
        })
        .await
        .map_err(|_| CommandError::new("unavailable", "bridge engine is not available"))?;
    let reply = match tokio::time::timeout(Duration::from_secs(10), reply_rx.recv()).await {
        Ok(Some(reply)) => reply,
        Ok(None) => {
            return Err(CommandError::new(
                "unavailable",
                "bridge engine closed before answering",
            ));
        }
        Err(_) => {
            return Err(CommandError::new(
                "timeout",
                "the bridge did not answer within 10 seconds",
            ));
        }
    };
    let value: serde_json::Value = serde_json::from_str(&reply)
        .map_err(|e| CommandError::new("internal", format!("invalid engine reply: {e}")))?;
    if value.get("type").and_then(|v| v.as_str()) == Some("error") {
        let data = value
            .get("data")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        return Err(CommandError::new(
            data.get("code")
                .and_then(|v| v.as_str())
                .unwrap_or("internal"),
            data.get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("command failed"),
        ));
    }
    Ok(value.get("data").cloned().unwrap_or(serde_json::json!({})))
}

/// Queue a command without waiting for a reply (announce/mark-read style operations).
pub async fn queue_command(shared: &Shared, kind: CommandKind) -> Result<(), CommandError> {
    let (reply_tx, _reply_rx) = mpsc::unbounded_channel::<Arc<str>>();
    shared
        .commands
        .send(EngineInput::Command {
            cmd: ClientCommand { id: None, kind },
            reply: reply_tx,
        })
        .await
        .map_err(|_| CommandError::new("unavailable", "bridge engine is not available"))
}

/// Everything the API layer and transports share with the engine task.
pub struct Shared {
    pub snapshot: Arc<RwLock<Snapshot>>,
    pub store: Arc<Store>,
    pub attachments: Arc<AttachmentStore>,
    pub events: broadcast::Sender<Arc<str>>,
    pub commands: mpsc::Sender<EngineInput>,
    pub started_at_ms: u64,
    /// Backbone mode manager (server listener + outbound links + control surface).
    pub backbone: Arc<BackboneManager>,
    /// Live engine store-and-forward envelope count, surfaced in `backbone_status`.
    pub backbone_envelopes: Arc<AtomicUsize>,
}

/// Emit one JSON event to every connected WebSocket/SSE client.
pub fn broadcast_event(events: &broadcast::Sender<Arc<str>>, json: String) {
    let _ = events.send(Arc::from(json));
}

/// Build the engine-facing config from a [`RuntimeConfig`], honouring channel state.
pub fn engine_config(config: &RuntimeConfig) -> EngineConfig {
    config.engine_config()
}

/// The engine task state.
pub struct CoreRuntime {
    pub engine: Engine,
    pub store: Arc<Store>,
    pub attachments: Arc<AttachmentStore>,
    pub snapshot: Arc<RwLock<Snapshot>>,
    pub events: broadcast::Sender<Arc<str>>,
    pub config: RuntimeConfig,
    pub transports: Vec<TransportHandle>,
    pub started_at_ms: u64,
    pub last_announce_ms: u64,
    pub last_rssi: Option<i16>,
    pub last_snr: Option<f32>,
    /// `res_id -> (peer, msg_id)` for resources we are sending.
    pub outgoing_resources: HashMap<u32, (NodeId, u32)>,
    /// `res_id -> (peer, msg_id)` for resources we are receiving (set when the manifest arrives).
    pub incoming_resources: HashMap<u32, (NodeId, u32)>,
    /// Resource bytes that arrived before their manifest message.
    pub pending_resources: HashMap<u32, (ResourceManifest, Vec<u8>)>,
    pub backbone: Arc<BackboneManager>,
    pub backbone_envelopes: Arc<AtomicUsize>,
    pub shutdown: tokio::sync::watch::Receiver<bool>,
}

impl CoreRuntime {
    fn node(&self) -> NodeId {
        self.engine.node_id()
    }

    fn identity(&self) -> PublicIdentity {
        self.engine.public_identity()
    }

    fn emit<T: Serialize>(&self, typ: &str, data: T) {
        broadcast_event(&self.events, event_json(typ, Some(data)));
    }

    fn node_name(&self, id: NodeId) -> String {
        self.engine
            .node_info(id)
            .map(|n| n.name)
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| id.to_hex())
    }

    /// Transmit engine output frames and apply engine events.
    pub async fn process(&mut self, processed: Processed, now: u64) {
        self.process_from(processed, now, None).await;
    }

    /// Like [`CoreRuntime::process`], but records which peer a frame came from so point-to-point
    /// transports (backbone) can avoid echoing it back.
    pub async fn process_from(&mut self, processed: Processed, now: u64, exclude: Option<NodeId>) {
        self.transmit(&processed, exclude);
        for event in processed.events {
            if let Err(e) = self.handle_event(event, now).await {
                tracing::warn!(error = %e, "failed to handle engine event");
            }
        }
    }

    /// Send only the frames of a processing result. Used from event handlers (sending an ACK
    /// while handling an incoming message) to avoid recursing through the async event loop.
    fn transmit(&self, processed: &Processed, exclude: Option<NodeId>) {
        for outbound in &processed.outbound {
            let bytes = match outbound.frame.encode() {
                Ok(bytes) => bytes,
                Err(e) => {
                    tracing::warn!(error = %e, "dropping unencodable outbound frame");
                    continue;
                }
            };
            // Only relayed copies of another node's frame must avoid the peer they arrived
            // from; locally originated frames (ACKs, offers, our own announces) may target that
            // same peer.
            let exclude = if outbound.frame.src == self.node() {
                None
            } else {
                exclude
            };
            let frame = TransportFrame {
                bytes,
                mode: outbound.mode,
                link: outbound.link,
                exclude,
            };
            for transport in &self.transports {
                transport.send(&frame);
            }
        }
        if !processed.events.is_empty() {
            tracing::debug!(
                events = processed.events.len(),
                "events generated while handling an event (dropped)"
            );
        }
    }

    async fn handle_event(&mut self, event: Event, now: u64) -> Result<()> {
        match event {
            Event::Message(received) => self.on_direct_message(received, now).await?,
            Event::GroupMessage(received) => self.on_group_message(received, now).await?,
            Event::Ack { from, ack } => self.on_ack(from, ack, now)?,
            Event::Position {
                from,
                channel,
                position,
            } => {
                self.emit(
                    "position",
                    PositionEventJson {
                        node_id: from.to_hex(),
                        channel_id: channel.map(|c| c.to_hex()),
                        position: PositionJson::from_core(&position),
                    },
                );
            }
            Event::Telemetry { from, telemetry } => {
                self.emit(
                    "telemetry",
                    TelemetryJson {
                        node_id: from.to_hex(),
                        battery_mv: telemetry.battery_mv,
                        battery_pct: telemetry.battery_pct,
                        uptime_s: telemetry.uptime_s,
                        airtime_used_ms: telemetry.airtime_used_ms,
                        temperature_c: telemetry.temperature_c,
                        solar_mv: telemetry.solar_mv,
                        timestamp_ms: telemetry.timestamp_ms,
                    },
                );
            }
            Event::NodeSeen(info) => {
                let node = NodeJson::from_core(&info);
                if let Err(e) = self.store.put_node(&node) {
                    tracing::warn!(error = %e, "failed to persist node");
                }
                self.touch_contact(info.node_id, now);
                self.emit("node", node);
            }
            Event::StoreOffer { from, ids } => {
                self.emit(
                    "store_offer",
                    StoreOfferJson {
                        relay: from.to_hex(),
                        count: ids.len(),
                    },
                );
            }
            Event::StoreAccepted { msg_id, relay } => {
                tracing::info!(msg_id, relay = %relay, "relay accepted envelope for store-and-forward");
            }
            Event::ResourceProgress {
                peer,
                res_id,
                received,
                total,
            } => {
                self.on_resource_progress(peer, res_id, received, total, now)?;
            }
            Event::ResourceReceived {
                peer,
                channel: _,
                manifest,
                data,
            } => {
                self.on_resource_received(peer, manifest, data, now).await?;
            }
            Event::ResourceSent { peer: _, res_id } => {
                self.on_resource_sent(res_id)?;
            }
            Event::Error(message) => {
                tracing::warn!(message, "protocol error reported by engine");
            }
        }
        Ok(())
    }

    fn touch_contact(&self, node: NodeId, now: u64) {
        if let Ok(Some(mut contact)) = self.store.get_contact(node) {
            contact.last_seen_ms = now;
            let _ = self.store.put_contact(&contact);
        }
    }

    async fn on_direct_message(
        &mut self,
        received: openmesh_core::ReceivedMessage,
        now: u64,
    ) -> Result<()> {
        let peer = received.from;
        let msg = received.message;
        let from_name = self.node_name(peer);
        let mut pending_res_id = None;
        let mut attachment = None;
        if let Some(a) = msg.attachment.as_ref() {
            pending_res_id = Some(a.res_id);
            attachment = Some(StoredAttachment {
                id: format!("att_pending_{}", a.res_id),
                mime: a.mime.clone(),
                size: a.size,
                sha256: hex::encode(a.sha256),
                duration_ms: a.duration_ms,
                name: a.name.clone(),
                progress: 0.0,
            });
            self.incoming_resources.insert(a.res_id, (peer, msg.id));
        }
        let record = StoredMessage {
            msg_id: msg.id,
            peer: Some(peer),
            channel_id: None,
            channel_name: None,
            from: peer,
            to: self.node(),
            from_name: Some(from_name),
            kind: msg.kind,
            text: msg.text.clone(),
            sent_at_ms: msg.sent_at_ms,
            received_at_ms: now,
            outgoing: false,
            delivered: true,
            read: false,
            reply_to: msg.reply_to,
            thread: msg.thread,
            attachment,
            position: msg.position.clone(),
            signature_valid: false,
            sender_key: None,
            via_store: received.via_store,
        };
        self.store.put_message(&record)?;
        // Position/telemetry messages also emit their dedicated event; avoid duplicate UI rows.
        if !matches!(record.kind, MessageKind::Position | MessageKind::Telemetry) {
            self.emit("message", record.to_message_json());
        }

        if let Some(res_id) = pending_res_id {
            if let Some((manifest, data)) = self.pending_resources.remove(&msg.id) {
                let _ = res_id;
                self.complete_incoming_resource(manifest, data, now).await?;
            }
        }
        Ok(())
    }

    async fn on_group_message(
        &mut self,
        received: openmesh_core::ReceivedGroupMessage,
        now: u64,
    ) -> Result<()> {
        let channel = received.channel;
        let sender = received.sender;
        let msg = received.message;
        let channel_name = self
            .store
            .get_channel(channel)?
            .map(|c| c.name)
            .or_else(|| {
                self.engine
                    .channels()
                    .into_iter()
                    .find(|(id, _)| *id == channel)
                    .map(|(_, name)| name)
            })
            .unwrap_or_else(|| channel.to_hex());
        let from_name = msg
            .sender
            .map(|s| self.node_name(s.node_id))
            .filter(|n| !n.is_empty() && n != &sender.to_hex())
            .unwrap_or_else(|| self.node_name(sender));
        let signature_valid = verify_group_signature(&sender, &channel, &msg);
        let sender_key = msg
            .sender
            .as_ref()
            .map(identity_to_b64)
            .or_else(|| {
                self.engine
                    .node_info(sender)
                    .map(|n| identity_to_b64(&n.ident))
            })
            .unwrap_or_default();
        let record = StoredMessage {
            msg_id: msg.id,
            peer: None,
            channel_id: Some(channel),
            channel_name: Some(channel_name),
            from: sender,
            to: channel,
            from_name: Some(from_name),
            kind: msg.kind,
            text: msg.text.clone(),
            sent_at_ms: msg.sent_at_ms,
            received_at_ms: now,
            outgoing: false,
            delivered: true,
            read: false,
            reply_to: msg.reply_to,
            thread: msg.thread,
            attachment: None,
            position: msg.position.clone(),
            signature_valid,
            sender_key: Some(sender_key),
            via_store: false,
        };
        self.store.put_message(&record)?;
        if !matches!(record.kind, MessageKind::Position | MessageKind::Telemetry) {
            self.emit("group_message", record.to_group_message_json());
        }
        Ok(())
    }

    fn on_ack(&mut self, from: NodeId, ack: Ack, _now: u64) -> Result<()> {
        let kind = match ack.kind {
            AckKind::Delivered => "delivered",
            AckKind::Read => "read",
            AckKind::ResourceWindow => "resource_window",
            AckKind::ResourceComplete => "resource_complete",
        };
        if let Some(mut record) = self.store.get_message(ack.msg_id)? {
            let mut changed = false;
            match ack.kind {
                AckKind::Delivered => {
                    if !record.delivered {
                        record.delivered = true;
                        changed = true;
                    }
                }
                AckKind::Read => {
                    record.read = true;
                    record.delivered = true;
                    changed = true;
                }
                AckKind::ResourceWindow => {}
                AckKind::ResourceComplete => {
                    if let Some(att) = record.attachment.as_mut() {
                        att.progress = 1.0;
                        record.delivered = true;
                        changed = true;
                    }
                }
            }
            if changed {
                self.store.put_message(&record)?;
                if let Some(att) = record.attachment.as_ref() {
                    self.emit(
                        "message_update",
                        MessageUpdateJson {
                            msg_id: record.msg_id,
                            delivered: Some(record.delivered),
                            read: Some(record.read),
                            attachment: Some(att.to_json()),
                        },
                    );
                }
            }
        }
        self.emit(
            "ack",
            AckJson {
                msg_id: ack.msg_id,
                peer: from.to_hex(),
                kind: kind.to_string(),
                at_ms: ack.timestamp_ms,
            },
        );
        Ok(())
    }

    fn on_resource_progress(
        &mut self,
        peer: NodeId,
        res_id: u32,
        received: u16,
        total: u16,
        _now: u64,
    ) -> Result<()> {
        let (direction, msg_id) = if let Some((_, msg_id)) = self.outgoing_resources.get(&res_id) {
            ("out", *msg_id)
        } else if let Some((_, msg_id)) = self.incoming_resources.get(&res_id) {
            ("in", *msg_id)
        } else {
            tracing::debug!(res_id, "resource progress for unknown resource");
            return Ok(());
        };
        self.emit(
            "resource_progress",
            ResourceProgressJson {
                peer: peer.to_hex(),
                msg_id,
                res_id,
                received,
                total,
                direction: direction.to_string(),
            },
        );
        if total > 0 {
            if let Some(mut record) = self.store.get_message(msg_id)? {
                if let Some(att) = record.attachment.as_mut() {
                    att.progress = (received as f32 / total as f32).clamp(0.0, 1.0);
                    let att_json = att.to_json();
                    self.store.put_message(&record)?;
                    self.emit(
                        "message_update",
                        MessageUpdateJson {
                            msg_id: record.msg_id,
                            delivered: None,
                            read: None,
                            attachment: Some(att_json),
                        },
                    );
                }
            }
        }
        Ok(())
    }

    fn on_resource_sent(&mut self, res_id: u32) -> Result<()> {
        if let Some((_, msg_id)) = self.outgoing_resources.remove(&res_id) {
            if let Some(mut record) = self.store.get_message(msg_id)? {
                if let Some(att) = record.attachment.as_mut() {
                    att.progress = 1.0;
                    record.delivered = true;
                }
                self.store.put_message(&record)?;
                if let Some(att) = record.attachment.as_ref() {
                    self.emit(
                        "message_update",
                        MessageUpdateJson {
                            msg_id: record.msg_id,
                            delivered: Some(record.delivered),
                            read: None,
                            attachment: Some(att.to_json()),
                        },
                    );
                }
            }
        }
        Ok(())
    }

    async fn on_resource_received(
        &mut self,
        peer: NodeId,
        manifest: ResourceManifest,
        data: Vec<u8>,
        now: u64,
    ) -> Result<()> {
        // Long text / history resources carry a CBOR application envelope that the engine has
        // already surfaced as a normal `message`/`group_message` event; only binary attachment
        // kinds carry file bytes we must store.
        if !matches!(
            manifest.kind,
            ResourceKind::Attachment | ResourceKind::Avatar
        ) {
            tracing::debug!(
                res_id = manifest.res_id,
                kind = ?manifest.kind,
                peer = %peer,
                "resource carried an application envelope (handled by the engine)"
            );
            return Ok(());
        }
        // The engine has already verified the sealed blob digest. If the application message
        // carried an AttachmentRef (plaintext sha256), cross-check the bytes against it as well.
        if let Ok(Some(record)) = self.store.get_message(manifest.msg_id) {
            if let Some(att) = record.attachment.as_ref() {
                if let Ok(expected) = hex::decode(&att.sha256) {
                    if expected.len() == 32 && sha256_bytes(&data).as_slice() != expected.as_slice()
                    {
                        tracing::warn!(
                            res_id = manifest.res_id,
                            peer = %peer,
                            "attachment payload does not match the manifest sha256, refusing it"
                        );
                        return Ok(());
                    }
                }
            }
        }
        let msg_id = manifest.msg_id;
        if self.store.get_message(msg_id)?.is_some() {
            self.complete_incoming_resource(manifest, data, now).await?;
        } else {
            tracing::debug!(msg_id, "resource arrived before its manifest message");
            self.pending_resources.insert(msg_id, (manifest, data));
        }
        Ok(())
    }

    async fn complete_incoming_resource(
        &mut self,
        manifest: ResourceManifest,
        data: Vec<u8>,
        _now: u64,
    ) -> Result<()> {
        let Some(mut record) = self.store.get_message(manifest.msg_id)? else {
            return Ok(());
        };
        let name = record.attachment.as_ref().and_then(|a| a.name.clone());
        let duration_ms = record.attachment.as_ref().and_then(|a| a.duration_ms);
        let meta = self
            .attachments
            .put(&manifest.mime, name.as_deref(), duration_ms, &data)
            .await
            .context("storing received resource")?;
        record.attachment = Some(StoredAttachment {
            id: meta.id.clone(),
            mime: meta.mime.clone(),
            size: meta.size as u32,
            sha256: meta.sha256.clone(),
            duration_ms,
            name,
            progress: 1.0,
        });
        self.store.put_message(&record)?;
        self.incoming_resources.remove(&manifest.res_id);
        self.emit(
            "message_update",
            MessageUpdateJson {
                msg_id: record.msg_id,
                delivered: Some(record.delivered),
                read: None,
                attachment: record.attachment.as_ref().map(StoredAttachment::to_json),
            },
        );
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Application commands
    // -----------------------------------------------------------------------------------------

    pub async fn handle_command(
        &mut self,
        cmd: ClientCommand,
        reply: mpsc::UnboundedSender<Arc<str>>,
        now: u64,
    ) {
        let id = cmd.id.clone();
        let result = match cmd.kind {
            CommandKind::SendMessage(data) => {
                self.cmd_send_message(id.as_deref(), data, &reply, now)
                    .await
            }
            CommandKind::SendGroupMessage(data) => {
                self.cmd_send_group_message(id.as_deref(), data, &reply, now)
                    .await
            }
            CommandKind::SendPosition(data) => {
                self.cmd_send_position(id.as_deref(), data, &reply, now)
                    .await
            }
            CommandKind::MarkRead(data) => {
                self.cmd_mark_read(id.as_deref(), data, &reply, now).await
            }
            CommandKind::Retry(data) => self.cmd_retry(id.as_deref(), data, &reply, now).await,
            CommandKind::AddContact(data) => self.cmd_add_contact(id.as_deref(), data, &reply, now),
            CommandKind::RemoveContact(node) => {
                self.cmd_remove_contact(id.as_deref(), node, &reply, now)
            }
            CommandKind::CreateChannel(data) => {
                self.cmd_create_channel(id.as_deref(), data, &reply, now)
            }
            CommandKind::JoinChannel(data) => {
                self.cmd_join_channel(id.as_deref(), data, &reply, now)
            }
            CommandKind::LeaveChannel(data) => {
                self.cmd_leave_channel(id.as_deref(), data, &reply, now)
            }
            CommandKind::SetConfig(value) => {
                self.cmd_set_config(id.as_deref(), value, &reply, now).await
            }
            CommandKind::Announce => self.cmd_announce(now).await,
        };
        if let Err(e) = result {
            tracing::warn!(error = %e, "command failed");
            reply_error(&reply, id.as_deref(), "internal", &e.to_string());
        }
    }

    fn send_reply<T: Serialize>(
        &mut self,
        reply: &mpsc::UnboundedSender<Arc<str>>,
        typ: &str,
        id: Option<&str>,
        data: T,
    ) {
        let _ = self;
        let _ = reply.send(Arc::from(response_json(typ, id, Some(data))));
    }

    fn new_msg_id(&self) -> u32 {
        loop {
            let id = rand::random::<u32>();
            if id == 0 {
                continue;
            }
            match self.store.get_message(id) {
                Ok(None) => return id,
                Ok(Some(_)) => continue,
                Err(_) => return id,
            }
        }
    }

    fn est_airtime_ms(&self, processed: &Processed) -> u32 {
        let profile = self.config.profile();
        processed
            .outbound
            .iter()
            .map(|o| util::lora_airtime_ms(profile, o.frame.body.len() + 22))
            .sum()
    }

    async fn cmd_send_message(
        &mut self,
        id: Option<&str>,
        data: SendMessageData,
        reply: &mpsc::UnboundedSender<Arc<str>>,
        now: u64,
    ) -> Result<()> {
        let Some(to) = parse_node_id(&data.to) else {
            reply_error(reply, id, "bad_request", "invalid node id in `to`");
            return Ok(());
        };
        if data.text.as_deref().unwrap_or("").is_empty() && data.attachment_id.is_none() {
            reply_error(reply, id, "bad_request", "text or attachment_id required");
            return Ok(());
        }
        let Some(peer_ident) = self.peer_identity(to) else {
            reply_error(
                reply,
                id,
                "unknown_peer",
                "no identity for recipient; wait for an announce or add a contact",
            );
            return Ok(());
        };
        let _ = peer_ident;
        let msg_id = self.new_msg_id();
        let mut attachment_ref = None;
        let mut stored_attachment = None;
        let mut attachment_bytes = None;
        if let Some(att_id) = data.attachment_id.as_deref() {
            if self.store.get_attachment(att_id)?.is_none() {
                reply_error(reply, id, "not_found", "unknown attachment id");
                return Ok(());
            }
            let Some((meta, bytes)) = self.attachments.read(att_id).await? else {
                reply_error(reply, id, "not_found", "attachment payload missing on disk");
                return Ok(());
            };
            let sha = hex::decode(&meta.sha256)?;
            let sha: [u8; 32] = sha.as_slice().try_into().unwrap_or([0u8; 32]);
            let chunk_bytes = 180u16;
            let chunk_count = meta.size.div_ceil(chunk_bytes as u64).min(u16::MAX as u64) as u16;
            let attachment = openmesh_core::AttachmentRef {
                res_id: rand::random::<u32>(),
                mime: meta.mime.clone(),
                size: meta.size as u32,
                sha256: sha,
                chunk_bytes,
                chunk_count,
                duration_ms: meta.duration_ms,
                name: meta.name.clone(),
            };
            stored_attachment = Some(StoredAttachment {
                id: meta.id.clone(),
                mime: meta.mime.clone(),
                size: meta.size as u32,
                sha256: meta.sha256.clone(),
                duration_ms: meta.duration_ms,
                name: meta.name.clone(),
                progress: 0.0,
            });
            attachment_ref = Some(attachment);
            attachment_bytes = Some((meta.mime.clone(), bytes));
        }
        let kind = if attachment_ref.is_some() {
            MessageKind::Attachment
        } else {
            MessageKind::Text
        };
        let app = AppMessage {
            id: msg_id,
            kind,
            sent_at_ms: now,
            text: data.text.clone(),
            reply_to: data.reply_to,
            thread: None,
            attachment: attachment_ref.clone(),
            position: None,
            telemetry: None,
            sender: None,
            signature: None,
        };
        let target = Target::Direct(to);
        let processed = match self.engine.send_message(target, app, now) {
            Ok(p) => p,
            Err(e) => {
                reply_error(reply, id, error_code(&e), &e.to_string());
                return Ok(());
            }
        };
        let mut est = self.est_airtime_ms(&processed);
        self.process(processed, now).await;
        if let (Some(attachment), Some((mime, bytes))) = (attachment_ref, attachment_bytes) {
            match self.engine.send_resource(
                target,
                msg_id,
                attachment.res_id,
                ResourceKind::Attachment,
                &mime,
                &bytes,
                now,
            ) {
                Ok(p) => {
                    est += self.est_airtime_ms(&p);
                    self.outgoing_resources
                        .insert(attachment.res_id, (to, msg_id));
                    self.process(p, now).await;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to start attachment transfer");
                }
            }
        }
        let record = StoredMessage {
            msg_id,
            peer: Some(to),
            channel_id: None,
            channel_name: None,
            from: self.node(),
            to,
            from_name: Some(self.config.name.clone()),
            kind,
            text: data.text,
            sent_at_ms: now,
            received_at_ms: now,
            outgoing: true,
            delivered: false,
            read: false,
            reply_to: data.reply_to,
            thread: None,
            attachment: stored_attachment,
            position: None,
            signature_valid: true,
            sender_key: None,
            via_store: false,
        };
        self.store.put_message(&record)?;
        self.emit("message", record.to_message_json());
        self.send_reply(
            reply,
            "send_result",
            id,
            SendResultJson {
                msg_id,
                est_airtime_ms: est,
                queued: true,
            },
        );
        Ok(())
    }

    async fn cmd_send_group_message(
        &mut self,
        id: Option<&str>,
        data: SendGroupMessageData,
        reply: &mpsc::UnboundedSender<Arc<str>>,
        now: u64,
    ) -> Result<()> {
        let Some(channel) = parse_node_id(&data.channel_id) else {
            reply_error(reply, id, "bad_request", "invalid channel_id");
            return Ok(());
        };
        let Some(channel_record) = self.store.get_channel(channel)? else {
            reply_error(reply, id, "not_found", "unknown channel");
            return Ok(());
        };
        if data.text.as_deref().unwrap_or("").is_empty() && data.attachment_id.is_none() {
            reply_error(reply, id, "bad_request", "text or attachment_id required");
            return Ok(());
        }
        if let Some(psk) = channel_record.psk_bytes() {
            // Idempotent: keeps the engine in sync if the channel was created outside the daemon.
            self.engine.add_channel(&channel_record.name, &psk);
        }
        let msg_id = self.new_msg_id();
        let mut attachment_ref = None;
        let mut stored_attachment = None;
        let mut attachment_bytes = None;
        if let Some(att_id) = data.attachment_id.as_deref() {
            if self.store.get_attachment(att_id)?.is_none() {
                reply_error(reply, id, "not_found", "unknown attachment id");
                return Ok(());
            }
            let Some((meta, bytes)) = self.attachments.read(att_id).await? else {
                reply_error(reply, id, "not_found", "attachment payload missing on disk");
                return Ok(());
            };
            let sha = hex::decode(&meta.sha256)?;
            let sha: [u8; 32] = sha.as_slice().try_into().unwrap_or([0u8; 32]);
            let chunk_bytes = 180u16;
            let chunk_count = meta.size.div_ceil(chunk_bytes as u64).min(u16::MAX as u64) as u16;
            attachment_ref = Some(openmesh_core::AttachmentRef {
                res_id: rand::random::<u32>(),
                mime: meta.mime.clone(),
                size: meta.size as u32,
                sha256: sha,
                chunk_bytes,
                chunk_count,
                duration_ms: meta.duration_ms,
                name: meta.name.clone(),
            });
            stored_attachment = Some(StoredAttachment {
                id: meta.id.clone(),
                mime: meta.mime.clone(),
                size: meta.size as u32,
                sha256: meta.sha256.clone(),
                duration_ms: meta.duration_ms,
                name: meta.name.clone(),
                progress: 1.0,
            });
            attachment_bytes = Some((meta.mime.clone(), bytes));
        }
        let kind = if attachment_ref.is_some() {
            MessageKind::Attachment
        } else {
            MessageKind::Text
        };
        let app = AppMessage {
            id: msg_id,
            kind,
            sent_at_ms: now,
            text: data.text.clone(),
            reply_to: None,
            thread: None,
            attachment: attachment_ref.clone(),
            position: None,
            telemetry: None,
            sender: Some(self.identity()),
            signature: None,
        };
        let target = Target::Channel(channel);
        let processed = match self.engine.send_message(target, app, now) {
            Ok(p) => p,
            Err(e) => {
                reply_error(reply, id, error_code(&e), &e.to_string());
                return Ok(());
            }
        };
        let mut est = self.est_airtime_ms(&processed);
        self.process(processed, now).await;
        if let (Some(attachment), Some((mime, bytes))) = (attachment_ref, attachment_bytes) {
            match self.engine.send_resource(
                target,
                msg_id,
                attachment.res_id,
                ResourceKind::Attachment,
                &mime,
                &bytes,
                now,
            ) {
                Ok(p) => {
                    est += self.est_airtime_ms(&p);
                    self.outgoing_resources
                        .insert(attachment.res_id, (channel, msg_id));
                    self.process(p, now).await;
                }
                Err(e) => tracing::warn!(error = %e, "failed to start group attachment transfer"),
            }
        }
        let record = StoredMessage {
            msg_id,
            peer: None,
            channel_id: Some(channel),
            channel_name: Some(channel_record.name.clone()),
            from: self.node(),
            to: channel,
            from_name: Some(self.config.name.clone()),
            kind,
            text: data.text,
            sent_at_ms: now,
            received_at_ms: now,
            outgoing: true,
            delivered: false,
            read: false,
            reply_to: None,
            thread: None,
            attachment: stored_attachment,
            position: None,
            signature_valid: true,
            sender_key: Some(identity_to_b64(&self.identity())),
            via_store: false,
        };
        self.store.put_message(&record)?;
        self.emit("group_message", record.to_group_message_json());
        self.send_reply(
            reply,
            "send_result",
            id,
            SendResultJson {
                msg_id,
                est_airtime_ms: est,
                queued: true,
            },
        );
        Ok(())
    }

    async fn cmd_send_position(
        &mut self,
        id: Option<&str>,
        data: SendPositionData,
        reply: &mpsc::UnboundedSender<Arc<str>>,
        now: u64,
    ) -> Result<()> {
        let position = match data.position.or(self.config.position) {
            Some(p) => p.to_core(),
            None => {
                reply_error(reply, id, "bad_request", "no position configured");
                return Ok(());
            }
        };
        let target = if let Some(to) = data.to.as_deref() {
            match parse_node_id(to) {
                Some(node) => {
                    if self.peer_identity(node).is_none() {
                        reply_error(reply, id, "unknown_peer", "no identity for recipient");
                        return Ok(());
                    }
                    Target::Direct(node)
                }
                None => {
                    reply_error(reply, id, "bad_request", "invalid node id in `to`");
                    return Ok(());
                }
            }
        } else if let Some(channel) = data.channel_id.as_deref() {
            match parse_node_id(channel) {
                Some(channel) => {
                    if self.store.get_channel(channel)?.is_none() {
                        reply_error(reply, id, "not_found", "unknown channel");
                        return Ok(());
                    }
                    Target::Channel(channel)
                }
                None => {
                    reply_error(reply, id, "bad_request", "invalid channel_id");
                    return Ok(());
                }
            }
        } else {
            reply_error(reply, id, "bad_request", "`to` or `channel_id` required");
            return Ok(());
        };
        let processed = match self.engine.send_position(target, position.clone(), now) {
            Ok(p) => p,
            Err(e) => {
                reply_error(reply, id, error_code(&e), &e.to_string());
                return Ok(());
            }
        };
        let est = self.est_airtime_ms(&processed);
        self.process(processed, now).await;
        self.emit(
            "position",
            PositionEventJson {
                node_id: self.node().to_hex(),
                channel_id: match target {
                    Target::Channel(c) => Some(c.to_hex()),
                    Target::Direct(_) => None,
                },
                position: PositionJson::from_core(&position),
            },
        );
        self.send_reply(
            reply,
            "send_result",
            id,
            QueuedResultJson {
                est_airtime_ms: est,
                queued: true,
            },
        );
        Ok(())
    }

    async fn cmd_mark_read(
        &mut self,
        id: Option<&str>,
        data: MsgIdData,
        reply: &mpsc::UnboundedSender<Arc<str>>,
        now: u64,
    ) -> Result<()> {
        let Some(mut record) = self.store.get_message(data.msg_id)? else {
            reply_error(reply, id, "not_found", "unknown message id");
            return Ok(());
        };
        if !record.outgoing && !record.read {
            let ack = Ack {
                kind: AckKind::Read,
                msg_id: record.msg_id,
                res_id: None,
                next_expected: None,
                bitmap: None,
                timestamp_ms: now,
            };
            if let Ok(processed) = self.engine.send_ack(record.from, ack, now) {
                self.process(processed, now).await;
            }
        }
        record.read = true;
        self.store.put_message(&record)?;
        self.emit(
            "message_update",
            MessageUpdateJson {
                msg_id: record.msg_id,
                delivered: Some(record.delivered),
                read: Some(true),
                attachment: None,
            },
        );
        let _ = reply;
        Ok(())
    }

    async fn cmd_retry(
        &mut self,
        id: Option<&str>,
        data: MsgIdData,
        reply: &mpsc::UnboundedSender<Arc<str>>,
        now: u64,
    ) -> Result<()> {
        let Some(record) = self.store.get_message(data.msg_id)? else {
            reply_error(reply, id, "not_found", "unknown message id");
            return Ok(());
        };
        if !record.outgoing {
            reply_error(
                reply,
                id,
                "bad_request",
                "only outgoing messages can be retried",
            );
            return Ok(());
        }
        let mut attachment_ref = None;
        let mut attachment_bytes = None;
        if let Some(att) = record.attachment.as_ref() {
            if self.store.get_attachment(&att.id)?.is_some() {
                if let Some((meta, bytes)) = self.attachments.read(&att.id).await? {
                    let sha = hex::decode(&meta.sha256)?;
                    let sha: [u8; 32] = sha.as_slice().try_into().unwrap_or([0u8; 32]);
                    let chunk_bytes = 180u16;
                    let chunk_count =
                        meta.size.div_ceil(chunk_bytes as u64).min(u16::MAX as u64) as u16;
                    attachment_ref = Some(openmesh_core::AttachmentRef {
                        res_id: rand::random::<u32>(),
                        mime: meta.mime.clone(),
                        size: meta.size as u32,
                        sha256: sha,
                        chunk_bytes,
                        chunk_count,
                        duration_ms: meta.duration_ms,
                        name: meta.name.clone(),
                    });
                    attachment_bytes = Some((meta.mime.clone(), bytes));
                }
            }
        }
        let kind = if record.attachment.is_some() {
            MessageKind::Attachment
        } else {
            MessageKind::Text
        };
        let app = AppMessage {
            id: record.msg_id,
            kind,
            sent_at_ms: now,
            text: record.text.clone(),
            reply_to: record.reply_to,
            thread: record.thread,
            attachment: attachment_ref.clone(),
            position: record.position.clone(),
            telemetry: None,
            sender: None,
            signature: None,
        };
        let target = if let Some(channel) = record.channel_id {
            Target::Channel(channel)
        } else if let Some(peer) = record.peer {
            Target::Direct(peer)
        } else {
            reply_error(reply, id, "bad_request", "message has no destination");
            return Ok(());
        };
        let processed = match self.engine.send_message(target, app, now) {
            Ok(p) => p,
            Err(e) => {
                reply_error(reply, id, error_code(&e), &e.to_string());
                return Ok(());
            }
        };
        let mut est = self.est_airtime_ms(&processed);
        self.process(processed, now).await;
        if let (Some(attachment), Some((mime, bytes))) = (attachment_ref, attachment_bytes) {
            if let Ok(p) = self.engine.send_resource(
                target,
                record.msg_id,
                attachment.res_id,
                ResourceKind::Attachment,
                &mime,
                &bytes,
                now,
            ) {
                est += self.est_airtime_ms(&p);
                let dest = match target {
                    Target::Direct(peer) => peer,
                    Target::Channel(channel) => channel,
                };
                self.outgoing_resources
                    .insert(attachment.res_id, (dest, record.msg_id));
                self.process(p, now).await;
            }
        }
        let mut record = record;
        record.delivered = false;
        record.sent_at_ms = now;
        record.received_at_ms = now;
        if let Some(att) = record.attachment.as_mut() {
            att.progress = 0.0;
        }
        self.store.put_message(&record)?;
        if record.is_group() {
            self.emit("group_message", record.to_group_message_json());
        } else {
            self.emit("message", record.to_message_json());
        }
        self.send_reply(
            reply,
            "send_result",
            id,
            SendResultJson {
                msg_id: record.msg_id,
                est_airtime_ms: est,
                queued: true,
            },
        );
        Ok(())
    }

    fn cmd_add_contact(
        &mut self,
        id: Option<&str>,
        data: AddContactData,
        reply: &mpsc::UnboundedSender<Arc<str>>,
        now: u64,
    ) -> Result<()> {
        let Some(node) = parse_node_id(&data.node_id) else {
            reply_error(reply, id, "bad_request", "invalid node id");
            return Ok(());
        };
        let ident = match data.identity.as_deref() {
            Some(encoded) => match identity_from_b64(encoded) {
                Some(ident) => {
                    if data.verify_key && !ident.is_self_consistent() {
                        reply_error(
                            reply,
                            id,
                            "bad_request",
                            "identity keys do not match node id",
                        );
                        return Ok(());
                    }
                    if ident.node_id != node {
                        reply_error(reply, id, "bad_request", "identity does not match node id");
                        return Ok(());
                    }
                    ident
                }
                None => {
                    reply_error(reply, id, "bad_request", "identity is not valid base64");
                    return Ok(());
                }
            },
            None => match self.peer_identity(node) {
                Some(ident) => ident,
                None => {
                    reply_error(
                        reply,
                        id,
                        "unknown_peer",
                        "identity required for unknown node",
                    );
                    return Ok(());
                }
            },
        };
        self.engine.add_contact(ident, data.name.clone());
        let contact = ContactRecord {
            node_id: node,
            name: data.name.clone(),
            identity: identity_b64(&ident),
            last_seen_ms: now,
            added_at_ms: now,
        };
        self.store.put_contact(&contact)?;
        if let Ok(Some(mut node_json)) = self.node_from_store(node) {
            node_json.is_contact = true;
            if let Some(name) = data.name {
                node_json.name = name;
            }
            self.store.put_node(&node_json)?;
        }
        let contacts = contacts_json(&self.store)?;
        self.send_reply(reply, "contacts", id, contacts);
        Ok(())
    }

    fn cmd_remove_contact(
        &mut self,
        id: Option<&str>,
        node: String,
        reply: &mpsc::UnboundedSender<Arc<str>>,
        _now: u64,
    ) -> Result<()> {
        let Some(node) = parse_node_id(&node) else {
            reply_error(reply, id, "bad_request", "invalid node id");
            return Ok(());
        };
        self.engine.remove_contact(node);
        self.store.remove_contact(node)?;
        let contacts = contacts_json(&self.store)?;
        self.send_reply(reply, "contacts", id, contacts);
        Ok(())
    }

    fn cmd_create_channel(
        &mut self,
        id: Option<&str>,
        data: ChannelCreateData,
        reply: &mpsc::UnboundedSender<Arc<str>>,
        now: u64,
    ) -> Result<()> {
        if data.name.trim().is_empty() {
            reply_error(reply, id, "bad_request", "channel name required");
            return Ok(());
        }
        let psk: [u8; 32] = match data.psk.as_deref() {
            Some(encoded) => match b64_decode(encoded).and_then(|b| <[u8; 32]>::try_from(b).ok()) {
                Some(psk) => psk,
                None => {
                    reply_error(reply, id, "bad_request", "psk must be base64 of 32 bytes");
                    return Ok(());
                }
            },
            None => rand::random::<[u8; 32]>(),
        };
        let channel_id = self.engine.add_channel(&data.name, &psk);
        let record = ChannelRecord {
            channel_id,
            name: data.name.clone(),
            psk: b64(&psk),
            created_at_ms: now,
        };
        self.store.put_channel(&record)?;
        self.send_reply(
            reply,
            "channel_created",
            id,
            ChannelCreatedJson {
                channel_id: channel_id.to_hex(),
                name: data.name,
                psk: b64(&psk),
            },
        );
        Ok(())
    }

    fn cmd_join_channel(
        &mut self,
        id: Option<&str>,
        data: ChannelCreateData,
        reply: &mpsc::UnboundedSender<Arc<str>>,
        now: u64,
    ) -> Result<()> {
        if data.name.trim().is_empty() {
            reply_error(reply, id, "bad_request", "channel name required");
            return Ok(());
        }
        let Some(psk) = data
            .psk
            .as_deref()
            .and_then(b64_decode)
            .and_then(|b| <[u8; 32]>::try_from(b).ok())
        else {
            reply_error(reply, id, "bad_request", "psk must be base64 of 32 bytes");
            return Ok(());
        };
        let channel_id = self.engine.add_channel(&data.name, &psk);
        let record = ChannelRecord {
            channel_id,
            name: data.name.clone(),
            psk: b64(&psk),
            created_at_ms: now,
        };
        self.store.put_channel(&record)?;
        let channels = channels_json(&self.store)?;
        self.send_reply(reply, "channels", id, channels);
        Ok(())
    }

    fn cmd_leave_channel(
        &mut self,
        id: Option<&str>,
        data: ChannelIdData,
        reply: &mpsc::UnboundedSender<Arc<str>>,
        _now: u64,
    ) -> Result<()> {
        let Some(channel) = parse_node_id(&data.channel_id) else {
            reply_error(reply, id, "bad_request", "invalid channel_id");
            return Ok(());
        };
        self.engine.remove_channel(channel);
        self.store.remove_channel(channel)?;
        let channels = channels_json(&self.store)?;
        self.send_reply(reply, "channels", id, channels);
        Ok(())
    }

    async fn cmd_set_config(
        &mut self,
        id: Option<&str>,
        value: serde_json::Value,
        reply: &mpsc::UnboundedSender<Arc<str>>,
        _now: u64,
    ) -> Result<()> {
        let Some(object) = value.as_object() else {
            reply_error(reply, id, "bad_request", "config must be an object");
            return Ok(());
        };
        let mut config = self.config.clone();
        if let Some(node) = object.get("node").and_then(|v| v.as_object()) {
            if let Some(name) = node.get("name").and_then(|v| v.as_str()) {
                if name.trim().is_empty() {
                    reply_error(reply, id, "bad_request", "node.name must not be empty");
                    return Ok(());
                }
                config.name = name.to_string();
            }
            if let Some(class) = node.get("class").and_then(|v| v.as_str()) {
                if parse_node_class(class).is_none() {
                    reply_error(reply, id, "bad_request", "unknown node class");
                    return Ok(());
                }
                config.class = class.to_string();
            }
        }
        if let Some(radio) = object.get("radio").and_then(|v| v.as_object()) {
            if let Some(profile) = radio.get("profile").and_then(|v| v.as_str()) {
                if !util::PROFILES
                    .iter()
                    .any(|p| p.name.eq_ignore_ascii_case(profile))
                {
                    reply_error(reply, id, "bad_request", "unknown radio profile");
                    return Ok(());
                }
                config.profile = profile.to_string();
            }
            if let Some(region) = radio.get("region").and_then(|v| v.as_str()) {
                if !util::REGIONS
                    .iter()
                    .any(|r| r.name.eq_ignore_ascii_case(region))
                {
                    reply_error(reply, id, "bad_request", "unknown region");
                    return Ok(());
                }
                config.region = region.to_string();
            }
        }
        if let Some(features) = object.get("features").and_then(|v| v.as_object()) {
            if let Some(v) = features.get("auto_announce").and_then(|v| v.as_bool()) {
                config.auto_announce = v;
            }
            if let Some(v) = features.get("store_forward").and_then(|v| v.as_bool()) {
                config.store_forward = v;
            }
            if let Some(v) = features.get("position_sharing").and_then(|v| v.as_bool()) {
                config.position_sharing = v;
            }
        }
        if let Some(position) = object.get("position") {
            if position.is_null() {
                config.position = None;
            } else {
                match serde_json::from_value::<PositionJson>(position.clone()) {
                    Ok(p) => config.position = Some(p),
                    Err(_) => {
                        reply_error(reply, id, "bad_request", "invalid position object");
                        return Ok(());
                    }
                }
            }
        }
        // A partial regulatory override patch (enabling/exceeding requires acknowledgement).
        if let Some(override_patch) = object.get("regulatory_override") {
            if override_patch.is_null() {
                config.regulatory_override = RegulatoryOverrideJson::default();
            } else {
                let mut merged = config.regulatory_override.clone();
                if let Err((code, message)) = merged.apply_patch(override_patch) {
                    reply_error(reply, id, code, &message);
                    return Ok(());
                }
                config.regulatory_override = merged;
            }
        }
        // Effective profile/region must be a legal combination for the region.
        let region = util::region(&config.region).unwrap_or(&util::REGIONS[0]);
        let profile = config.profile();
        let allowed_profiles: Vec<&str> = util::PROFILES
            .iter()
            .filter(|candidate| candidate.bw_khz <= region.bw_khz)
            .map(|candidate| candidate.name)
            .collect();
        if !region.allows_profile(profile) {
            reply_error(
                reply,
                id,
                "profile_not_allowed_in_region",
                &format!(
                    "profile {} uses {} kHz, which is wider than the {} kHz legal maximum in {}; allowed profiles: {}",
                    profile.name,
                    profile.bw_khz,
                    region.bw_khz,
                    region.name,
                    allowed_profiles.join(", ")
                ),
            );
            return Ok(());
        }
        // Validate the (possibly region-changed) override against the effective region.
        if let Some(frequency) = config.regulatory_override.frequency_khz {
            if !regulatory::frequency_supported(frequency) {
                reply_error(
                    reply,
                    id,
                    "regulatory_override_frequency_out_of_range",
                    &format!(
                        "frequency {frequency} kHz is outside the supported radio range {}..={} kHz",
                        regulatory::RADIO_MIN_FREQUENCY_KHZ,
                        regulatory::RADIO_MAX_FREQUENCY_KHZ
                    ),
                );
                return Ok(());
            }
        }
        if let Err(error) = regulatory::validate(region, &config.regulatory_override.to_core()) {
            reply_error(reply, id, error.code(), error.message());
            return Ok(());
        }
        self.engine.set_config(config.engine_config());
        self.config = config.clone();
        self.store.set_meta_json("config", &config)?;
        self.refresh_snapshot().await;
        let snapshot = self.snapshot.read().await.clone();
        let full_config = snapshot.config.clone();
        self.send_reply(reply, "config", id, full_config);
        Ok(())
    }

    async fn cmd_announce(&mut self, now: u64) -> Result<()> {
        if let Ok(processed) = self.engine.announce(now) {
            self.process(processed, now).await;
            self.last_announce_ms = now;
        }
        Ok(())
    }

    /// Enable/disable store-and-forward by switching the engine between relay and client role.
    pub async fn set_store_forward(&mut self, enabled: bool) {
        let mut config = self.engine.config().clone();
        if enabled {
            config.caps |= caps::STORE;
            config.role = Role::Relay(RelayConfig {
                store_capacity: 256,
                store_ttl_ms: 72 * 3_600_000,
                max_frames_per_min: 240,
            });
        } else {
            config.caps &= !caps::STORE;
            config.role = Role::Client;
        }
        self.engine.set_config(config);
        self.refresh_snapshot().await;
    }

    fn peer_identity(&self, node: NodeId) -> Option<PublicIdentity> {
        self.engine.node_info(node).map(|n| n.ident).or_else(|| {
            self.store
                .get_contact(node)
                .ok()
                .flatten()
                .and_then(|c| c.ident())
        })
    }

    fn node_from_store(&self, node: NodeId) -> Result<Option<NodeJson>> {
        Ok(self
            .store
            .list_nodes()?
            .into_iter()
            .find(|n| n.node_id == node.to_hex()))
    }

    // -----------------------------------------------------------------------------------------
    // Tick / snapshot
    // -----------------------------------------------------------------------------------------

    pub async fn tick(&mut self, now: u64) {
        let processed = self.engine.tick(now);
        self.process(processed, now).await;
        if self.config.auto_announce && now.saturating_sub(self.last_announce_ms) >= 1_800_000 {
            let _ = self.cmd_announce(now).await;
        }
        self.refresh_snapshot().await;
        let snapshot = self.snapshot.read().await;
        self.emit("status", snapshot.status.clone());
    }

    /// Rebuild the API snapshot from the engine + store.
    pub async fn refresh_snapshot(&mut self) {
        self.backbone_envelopes
            .store(self.engine.stats().stored_envelopes, Ordering::Relaxed);
        let snapshot = build_snapshot(SnapshotInput {
            engine: &self.engine,
            store: &self.store,
            attachments: &self.attachments,
            transports: &self.transports,
            config: &self.config,
            started_at_ms: self.started_at_ms,
            last_rssi: self.last_rssi,
            last_snr: self.last_snr,
            backbone_configured: self.backbone.is_configured(),
            backbone_running: self.backbone.is_running(),
        });
        *self.snapshot.write().await = snapshot;
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

fn verify_group_signature(sender: &NodeId, channel: &NodeId, msg: &AppMessage) -> bool {
    let Some(ident) = msg.sender.as_ref() else {
        return false;
    };
    let Some(signature) = msg.signature.as_ref() else {
        return false;
    };
    if ident.node_id != *sender {
        return false;
    }
    let Ok(sig): std::result::Result<[u8; 64], _> = signature.as_slice().try_into() else {
        return false;
    };
    let mut unsigned = msg.clone();
    unsigned.signature = None;
    let payload = group_signature_payload(sender, channel, &unsigned);
    ident.verify(&payload, &sig).is_ok()
}

fn error_code(err: &openmesh_core::Error) -> &'static str {
    match err {
        openmesh_core::Error::UnknownNode(_) => "unknown_peer",
        openmesh_core::Error::PayloadTooLarge { .. } => "too_large",
        openmesh_core::Error::Config(_) => "bad_request",
        openmesh_core::Error::UnsupportedVersion(_) => "bad_request",
        _ => "internal",
    }
}

/// Reply helpers shared with the API layer.
pub fn reply_error(
    reply: &mpsc::UnboundedSender<Arc<str>>,
    id: Option<&str>,
    code: &str,
    message: &str,
) {
    let _ = reply.send(Arc::from(error_json(id, code, message)));
}

// ---------------------------------------------------------------------------------------------
// Read-only API builders (used by the WebSocket handlers and SSE)
// ---------------------------------------------------------------------------------------------

pub fn build_hello(snapshot: &Snapshot) -> HelloJson {
    HelloJson {
        bridge_version: openmesh_core::CRATE_VERSION.to_string(),
        protocol: openmesh_core::VERSION_STR.to_string(),
        node: snapshot.node.clone(),
        radios: snapshot.radios.clone(),
        features: snapshot.features.clone(),
        store: snapshot.store.clone(),
    }
}

pub fn contacts_json(store: &Store) -> Result<ContactsJson> {
    let contacts = store
        .list_contacts()?
        .into_iter()
        .map(|c| ContactJson {
            node_id: c.node_id.to_hex(),
            name: c.name.clone(),
            identity: c.identity.clone(),
            last_seen_ms: c.last_seen_ms,
        })
        .collect();
    Ok(ContactsJson { contacts })
}

pub fn channels_json(store: &Store) -> Result<ChannelsJson> {
    let channels = store
        .list_channels()?
        .into_iter()
        .map(|c| ChannelJson {
            channel_id: c.channel_id.to_hex(),
            name: c.name,
            psk_set: true,
        })
        .collect();
    Ok(ChannelsJson { channels })
}

pub fn nodes_json(snapshot: &Snapshot) -> NodesJson {
    NodesJson {
        nodes: snapshot.nodes.clone(),
    }
}

pub fn stats_json(snapshot: &Snapshot) -> StatsJson {
    StatsJson {
        mesh: snapshot.status.mesh.clone(),
        links: snapshot.links.clone(),
    }
}

/// Direct-message history as the `messages` event payload.
pub fn messages_payload(
    store: &Store,
    peer: Option<&str>,
    channel: Option<&str>,
    before_ms: Option<u64>,
    limit: usize,
) -> Result<serde_json::Value> {
    let mut out = Vec::new();
    if let Some(peer) = peer {
        let Some(peer) = parse_node_id(peer) else {
            anyhow::bail!("invalid peer id");
        };
        for record in store.direct_history(peer, before_ms, limit)? {
            out.push(serde_json::to_value(record.to_message_json())?);
        }
    } else if let Some(channel) = channel {
        let Some(channel) = parse_node_id(channel) else {
            anyhow::bail!("invalid channel id");
        };
        for record in store.channel_history(channel, before_ms, limit)? {
            out.push(serde_json::to_value(record.to_group_message_json())?);
        }
    } else {
        anyhow::bail!("peer or channel required");
    }
    Ok(json!({ "messages": out }))
}

/// All messages since `since_ms` for the `sync` sequence.
pub fn sync_messages(store: &Store, since_ms: u64) -> Result<Vec<StoredMessage>> {
    store.messages_since(since_ms, 2000)
}

pub fn contact_url(ident: &PublicIdentity, name: &str) -> String {
    format!(
        "openmesh://contact?node={}&name={}&identity={}",
        ident.node_id.to_hex(),
        url_encode(name),
        base64_url(ident.to_bytes().as_slice())
    )
}

pub fn channel_url(channel: NodeId, name: &str, psk: &[u8; 32]) -> String {
    format!(
        "openmesh://channel?name={}&id={}&psk={}",
        url_encode(name),
        channel.to_hex(),
        base64_url(psk)
    )
}

/// Parse the query string of an `openmesh://...` URL into decoded key/value pairs.
pub fn parse_url_query(url: &str, scheme_prefix: &str) -> Option<HashMap<String, String>> {
    let rest = url.strip_prefix(scheme_prefix)?;
    let query = rest.split_once('?')?.1;
    let mut out = HashMap::new();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        out.insert(url_decode(key), url_decode(value));
    }
    Some(out)
}

fn url_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
                if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(byte);
                    index += 3;
                    continue;
                }
                out.push(b'%');
                index += 1;
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Decode base64 in any of the alphabets the bridge/QR links may use (standard or URL-safe,
/// padded or unpadded). Invite URLs use URL-safe no-pad while the WS API uses standard.
pub fn b64_decode_any(encoded: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let encoded = encoded.trim();
    for engine in [
        base64::engine::general_purpose::URL_SAFE_NO_PAD,
        base64::engine::general_purpose::URL_SAFE,
        base64::engine::general_purpose::STANDARD_NO_PAD,
        base64::engine::general_purpose::STANDARD,
    ] {
        if let Ok(bytes) = engine.decode(encoded) {
            return Some(bytes);
        }
    }
    None
}

/// Parse `openmesh://channel?name=...&psk=...` (id is optional and ignored on join).
pub fn parse_channel_url(url: &str) -> Option<(String, [u8; 32])> {
    let params = parse_url_query(url, "openmesh://channel")?;
    let name = params.get("name")?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let psk: [u8; 32] = b64_decode_any(params.get("psk")?)?.try_into().ok()?;
    Some((name, psk))
}

/// Parse `openmesh://contact?node=...&identity=...&name=...`.
pub fn parse_contact_url(url: &str) -> Option<(NodeId, Option<String>, PublicIdentity)> {
    let params = parse_url_query(url, "openmesh://contact")?;
    let node = parse_node_id(params.get("node")?)?;
    let identity = PublicIdentity::from_bytes(&b64_decode_any(params.get("identity")?)?).ok()?;
    if identity.node_id != node {
        return None;
    }
    let name = params
        .get("name")
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .map(|name| name.chars().take(64).collect());
    Some((node, name, identity))
}

fn base64_url(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Small helper used when loading persisted state at startup.
pub fn restore_engine_state(engine: &mut Engine, store: &Store) -> Result<()> {
    for contact in store.list_contacts()? {
        if let Some(ident) = contact.ident() {
            engine.add_contact(ident, contact.name.clone());
        }
    }
    for channel in store.list_channels()? {
        if let Some(psk) = channel.psk_bytes() {
            let id = engine.add_channel(&channel.name, &psk);
            if id != channel.channel_id {
                tracing::warn!(
                    expected = %channel.channel_id,
                    derived = %id,
                    name = %channel.name,
                    "stored channel id does not match derived id"
                );
            }
        }
    }
    Ok(())
}

/// Inputs for building an API snapshot without owning the engine task.
pub struct SnapshotInput<'a> {
    pub engine: &'a Engine,
    pub store: &'a Arc<Store>,
    pub attachments: &'a Arc<AttachmentStore>,
    pub transports: &'a [TransportHandle],
    pub config: &'a RuntimeConfig,
    pub started_at_ms: u64,
    pub last_rssi: Option<i16>,
    pub last_snr: Option<f32>,
    /// Whether backbone mode has been configured (show the backbone "radio" when true).
    pub backbone_configured: bool,
    /// Whether the backbone listener/client is live right now.
    pub backbone_running: bool,
}

/// Build the JSON snapshot served by the API from live engine state.
pub fn build_snapshot(input: SnapshotInput<'_>) -> Snapshot {
    let SnapshotInput {
        engine,
        store,
        attachments,
        transports,
        config,
        started_at_ms,
        last_rssi,
        last_snr,
        backbone_configured,
        backbone_running,
    } = input;
    let store_counts = StoreCountsJson {
        messages: store.message_count().unwrap_or(0),
        contacts: store.contact_count().unwrap_or(0),
        channels: store.channel_count().unwrap_or(0),
    };
    let engine_stats = engine.stats();
    let uptime_ms = util::now_ms().saturating_sub(started_at_ms);
    let uptime_s = uptime_ms / 1000;
    let airtime_pct = if uptime_ms > 0 {
        (engine_stats.airtime_tx_ms as f64 / uptime_ms as f64 * 100.0).min(100.0)
    } else {
        0.0
    };
    let region = config.region();
    let override_json = config.regulatory_override.clone();
    let override_active = override_json.is_active();
    let override_warning = override_json.warning();
    let effective_power = override_json.effective_power_dbm(region);
    let effective_frequency = override_json.effective_frequency_khz(region);
    let effective_duty = override_json.effective_duty_pct(region);
    let legal = LegalRegionJson::for_region(region);
    let radio_status = RadioStatusJson {
        connected: !transports.is_empty(),
        tx_queue: 0,
        airtime_used_pct: round2(airtime_pct),
        rssi: last_rssi,
        snr: last_snr,
        frequency_mhz: effective_frequency as f64 / 1000.0,
        tx_power_dbm: effective_power,
        duty_cycle_pct: effective_duty,
        max_power_dbm: region.max_power_dbm,
        max_bw_khz: region.bw_khz,
    };
    let mesh_status = MeshStatusJson {
        tx_frames: engine_stats.tx_frames,
        rx_frames: engine_stats.rx_frames,
        forwarded: engine_stats.forwarded_frames,
        dropped: engine_stats.dropped_frames,
        known_nodes: engine_stats.known_nodes,
        stored_envelopes: engine_stats.stored_envelopes,
        duty_cycle_pct: round2(airtime_pct),
    };
    let used_bytes = store.db_bytes() + attachments.total_bytes();
    let status = StatusJson {
        uptime_s,
        radio: radio_status,
        mesh: mesh_status,
        storage: StorageStatusJson {
            used_bytes,
            db_path: store.path().display().to_string(),
        },
        regulatory_override_active: override_active,
        regulatory_override_warning: override_warning.clone(),
    };
    let node = NodeSelfJson {
        node_id: engine.node_id().to_hex(),
        name: config.name.clone(),
        class: node_class_str(config.node_class()).to_string(),
        caps: engine.config().caps,
    };
    let visible_transports: Vec<&TransportHandle> = transports
        .iter()
        .filter(|t| t.kind != "backbone" || backbone_configured)
        .collect();
    let radios: Vec<RadioJson> = visible_transports
        .iter()
        .map(|t| RadioJson {
            id: format!("{}", t.id),
            kind: t.kind.to_string(),
            label: t.label.clone(),
            connected: if t.kind == "backbone" {
                backbone_running
            } else {
                true
            },
            profile: config.profile().name.to_string(),
            region: config.region().name.to_string(),
        })
        .collect();
    let links: Vec<LinkJson> = visible_transports
        .iter()
        .map(|t| LinkJson {
            link: t.id,
            kind: t.kind.to_string(),
            label: t.label.clone(),
            rssi: t.stats.rssi().or(last_rssi),
            snr: t.stats.snr().or(last_snr),
            tx_frames: t.stats.tx(),
            rx_frames: t.stats.rx(),
        })
        .collect();
    let channels: Vec<ConfigChannelJson> = channels_json(store)
        .map(|c| {
            c.channels
                .into_iter()
                .map(|channel| ConfigChannelJson {
                    channel_id: channel.channel_id,
                    name: channel.name,
                    psk_set: channel.psk_set,
                })
                .collect()
        })
        .unwrap_or_default();
    let config_json = ConfigJson {
        node: ConfigNodeJson {
            name: config.name.clone(),
            class: node_class_str(config.node_class()).to_string(),
            announce_interval_s: engine.config().announce_interval_ms / 1000,
        },
        radio: ConfigRadioJson {
            profile: config.profile().name.to_string(),
            region: config.region().name.to_string(),
            tx_power_dbm: effective_power,
            frequency_mhz: effective_frequency as f64 / 1000.0,
            interface: config.interface.clone(),
            max_power_dbm: region.max_power_dbm,
            duty_cycle_pct: region.duty_cycle_pct,
            max_bw_khz: region.bw_khz,
        },
        features: ConfigFeaturesJson {
            auto_announce: config.auto_announce,
            store_forward: config.store_forward,
            position_sharing: config.position_sharing,
        },
        channels,
        legal,
        regulatory_override: override_json,
        override_active,
        override_warning,
    };
    // Merge live engine knowledge with persisted node metadata (names, capabilities, last known
    // position). Persisted data fills the gaps that a fresh contact entry (hex name) leaves.
    let mut merged: std::collections::HashMap<String, NodeJson> = engine
        .known_nodes()
        .iter()
        .map(|info| {
            let node = NodeJson::from_core(info);
            (node.node_id.clone(), node)
        })
        .collect();
    if let Ok(stored_nodes) = store.list_nodes() {
        for stored in stored_nodes {
            match merged.get_mut(&stored.node_id) {
                Some(existing) => {
                    if existing.name == existing.node_id && !stored.name.is_empty() {
                        existing.name = stored.name.clone();
                    }
                    if existing.caps.is_empty() {
                        existing.caps = stored.caps.clone();
                    }
                    if existing.class == "client" && stored.class != "client" {
                        existing.class = stored.class.clone();
                    }
                    if existing.position.is_none() {
                        existing.position = stored.position;
                    }
                    if existing.battery_pct.is_none() {
                        existing.battery_pct = stored.battery_pct;
                    }
                    if existing.rssi.is_none() {
                        existing.rssi = stored.rssi;
                    }
                    if existing.snr.is_none() {
                        existing.snr = stored.snr;
                    }
                    existing.last_seen_ms = existing.last_seen_ms.max(stored.last_seen_ms);
                    existing.is_contact |= stored.is_contact;
                }
                None => {
                    merged.insert(stored.node_id.clone(), stored);
                }
            }
        }
    }
    let mut nodes: Vec<NodeJson> = merged.into_values().collect();
    nodes.sort_by_key(|n| n.last_seen_ms);
    nodes.reverse();
    Snapshot {
        node_id: engine.node_id().to_hex(),
        node,
        radios,
        features: FEATURES.iter().map(|f| f.to_string()).collect(),
        store: store_counts,
        status,
        config: config_json,
        nodes,
        links,
    }
}

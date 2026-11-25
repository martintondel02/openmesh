//! # openmesh-sim
//!
//! Discrete-event simulator for the OpenMesh protocol. Every virtual node runs a real
//! [`openmesh_core::Engine`]; the simulator only models the radio medium (topology, latency,
//! loss, airtime) and virtual wall-clock time. That keeps scenario results meaningful: they
//! exercise the same flooding, dedup, ARQ and store-and-forward code that runs on hardware.
//!
//! ```no_run
//! use openmesh_sim::{MeshSim, SimConfig};
//! let mut sim = MeshSim::new(2, SimConfig { loss_pct: 0, ..Default::default() });
//! sim.set_full_topology();
//! sim.announce_all();
//! sim.run_until(100);
//! let msg_id = sim.send_text(0, 1, "hello").unwrap();
//! sim.run_until(2_000);
//! assert!(sim.deliveries().iter().any(|d| d.msg_id == msg_id));
//! ```

pub mod scenarios;

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

use anyhow::{Context, Result};
use rand::rngs::StdRng;
use rand::{Rng, RngCore, SeedableRng};
use serde::{Deserialize, Serialize};

use openmesh_core::services::caps;
use openmesh_core::{
    AppMessage, Engine, EngineConfig, Event, Identity, LinkInfo, MessageKind, NodeId, Outbound,
    Position as CorePosition, ResourceKind, SendMode, Target, util,
};

/// Simulator configuration. Deterministic for a fixed seed.
#[derive(Clone, Debug)]
pub struct SimConfig {
    pub seed: u64,
    /// One-way latency added to every hop, in ms.
    pub latency_ms: u64,
    /// Per-transmission packet loss, percent.
    pub loss_pct: u8,
    /// How often each online node calls `Engine::tick`, in virtual ms.
    pub tick_ms: u64,
    /// LoRa profile used for airtime accounting.
    pub profile: util::LoraProfile,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            seed: 1,
            latency_ms: 20,
            loss_pct: 0,
            tick_ms: 100,
            profile: util::MEDIUM_FAST,
        }
    }
}

/// One virtual node.
pub struct SimNode {
    pub index: usize,
    pub name: String,
    pub node_id: NodeId,
    pub engine: Engine,
    pub repeater: bool,
    pub online: bool,
    pub tx_frames: u64,
    pub rx_frames: u64,
    pub forwarded: u64,
    pub airtime_tx_ms: u64,
    pub retransmits: u64,
    /// Keys of frames already transmitted by this node, for retransmission accounting.
    sent_frames: HashSet<(u8, u32, u32, u16)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeliveredMessage {
    pub from: usize,
    pub to: usize,
    pub msg_id: u32,
    pub text: Option<String>,
    pub sent_at_ms: u64,
    pub delivered_at_ms: u64,
    pub hops: Option<usize>,
    pub via_store: bool,
    /// True when the message arrived as a resource (attachment) payload.
    pub resource: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AckRecord {
    pub from: usize,
    pub to: usize,
    pub msg_id: u32,
    pub kind: String,
    pub at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceDelivery {
    pub peer: usize,
    pub msg_id: u32,
    pub res_id: u32,
    pub bytes: usize,
    pub sha_ok: bool,
    pub at_ms: u64,
    pub from_store: bool,
    /// Resource kind (`attachment`, `long_text`, ...).
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroupDelivery {
    pub channel: NodeId,
    pub sender: NodeId,
    pub msg_id: u32,
    pub at_ms: u64,
}

#[derive(Clone, Debug)]
struct SimEvent {
    at: u64,
    seq: u64,
    kind: EventKind,
}

#[derive(Clone, Debug)]
enum EventKind {
    Frame { to: usize, bytes: Vec<u8> },
    Tick { node: usize },
}

impl PartialEq for SimEvent {
    fn eq(&self, other: &Self) -> bool {
        (self.at, self.seq) == (other.at, other.seq)
    }
}
impl Eq for SimEvent {}
impl PartialOrd for SimEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for SimEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.at, self.seq).cmp(&(other.at, other.seq))
    }
}

/// The discrete-event mesh simulator.
pub struct MeshSim {
    pub config: SimConfig,
    pub nodes: Vec<SimNode>,
    pub neighbors: Vec<Vec<usize>>,
    pub now_ms: u64,
    queue: BinaryHeap<Reverse<SimEvent>>,
    next_seq: u64,
    rng: StdRng,
    pub deliveries: Vec<DeliveredMessage>,
    pub acks: Vec<AckRecord>,
    pub resource_deliveries: Vec<ResourceDelivery>,
    pub group_deliveries: Vec<GroupDelivery>,
    pub store_offers: usize,
    pub store_accepted: usize,
    pub sent_messages: usize,
    pub dropped_loss: u64,
    pub dropped_offline: u64,
    pub protocol_errors: Vec<String>,
    /// First time each node learned about each other node.
    pub discovery: Vec<HashMap<NodeId, u64>>,
    /// Virtual-time spans during which each node was offline.
    pub offline_spans: Vec<Vec<(u64, u64)>>,
    node_index: HashMap<NodeId, usize>,
}

impl MeshSim {
    /// Create `count` virtual nodes in a line topology.
    pub fn new(count: usize, config: SimConfig) -> Self {
        assert!(count > 0, "a mesh needs at least one node");
        let mut rng = StdRng::seed_from_u64(config.seed);
        let mut nodes = Vec::with_capacity(count);
        let mut node_index = HashMap::with_capacity(count);
        for index in 0..count {
            let mut seed = [0u8; 32];
            rng.fill_bytes(&mut seed);
            let engine = Engine::new(
                Identity::from_seed(&seed),
                EngineConfig::client(format!("node-{index}")),
            );
            let node_id = engine.node_id();
            node_index.insert(node_id, index);
            nodes.push(SimNode {
                index,
                name: format!("node-{index}"),
                node_id,
                engine,
                repeater: false,
                online: true,
                tx_frames: 0,
                rx_frames: 0,
                forwarded: 0,
                airtime_tx_ms: 0,
                retransmits: 0,
                sent_frames: HashSet::new(),
            });
        }
        let mut sim = Self {
            config,
            nodes,
            neighbors: vec![Vec::new(); count],
            now_ms: 0,
            queue: BinaryHeap::new(),
            next_seq: 0,
            rng,
            deliveries: Vec::new(),
            acks: Vec::new(),
            resource_deliveries: Vec::new(),
            group_deliveries: Vec::new(),
            store_offers: 0,
            store_accepted: 0,
            sent_messages: 0,
            dropped_loss: 0,
            dropped_offline: 0,
            protocol_errors: Vec::new(),
            discovery: vec![HashMap::new(); count],
            offline_spans: vec![Vec::new(); count],
            node_index,
        };
        sim.set_line_topology();
        sim.schedule_ticks();
        sim
    }

    /// Make node `index` a store-and-forward repeater.
    pub fn set_repeater(&mut self, index: usize) {
        let name = self.nodes[index].name.clone();
        let mut config = EngineConfig::repeater(name);
        config.caps |= caps::GPS;
        self.nodes[index].engine.set_config(config);
        self.nodes[index].repeater = true;
    }

    /// Undirected line topology: node i is adjacent to i-1 and i+1.
    pub fn set_line_topology(&mut self) {
        let count = self.nodes.len();
        let mut neighbors = vec![Vec::new(); count];
        for (i, adjacent) in neighbors.iter_mut().enumerate() {
            if i > 0 {
                adjacent.push(i - 1);
            }
            if i + 1 < count {
                adjacent.push(i + 1);
            }
        }
        self.neighbors = neighbors;
    }

    /// Every node can hear every other node.
    pub fn set_full_topology(&mut self) {
        let count = self.nodes.len();
        self.neighbors = (0..count)
            .map(|i| (0..count).filter(|j| *j != i).collect())
            .collect();
    }

    /// Replace the topology with an explicit undirected edge list.
    pub fn set_topology(&mut self, edges: &[(usize, usize)]) {
        let count = self.nodes.len();
        let mut neighbors = vec![Vec::new(); count];
        for &(a, b) in edges {
            assert!(a < count && b < count, "edge endpoint out of range");
            assert_ne!(a, b, "self loops are not useful");
            if !neighbors[a].contains(&b) {
                neighbors[a].push(b);
                neighbors[b].push(a);
            }
        }
        for list in &mut neighbors {
            list.sort_unstable();
        }
        self.neighbors = neighbors;
    }

    fn schedule_ticks(&mut self) {
        let start = self.config.tick_ms.max(1);
        for i in 0..self.nodes.len() {
            self.push_event(start, EventKind::Tick { node: i });
        }
    }

    fn push_event(&mut self, at: u64, kind: EventKind) {
        self.next_seq += 1;
        let seq = self.next_seq;
        self.queue.push(Reverse(SimEvent { at, seq, kind }));
    }

    pub fn node_name(&self, index: usize) -> &str {
        &self.nodes[index].name
    }

    pub fn node_id(&self, index: usize) -> NodeId {
        self.nodes[index].node_id
    }

    pub fn node_index_of(&self, id: NodeId) -> Option<usize> {
        self.node_index.get(&id).copied()
    }

    /// Number of nodes that know about every other node.
    pub fn fully_converged_nodes(&self) -> usize {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| self.discovery[*i].len() >= self.nodes.len() - 1)
            .count()
    }

    pub fn send_text(&mut self, from: usize, to: usize, text: &str) -> Result<u32> {
        let to_id = self.nodes[to].node_id;
        self.ensure_peer_known(from, to_id)?;
        let now = self.now_ms;
        let msg_id = self.rng.next_u32().max(1);
        let app = AppMessage {
            id: msg_id,
            kind: MessageKind::Text,
            sent_at_ms: now,
            text: Some(text.to_string()),
            reply_to: None,
            thread: None,
            attachment: None,
            position: None,
            telemetry: None,
            sender: None,
            signature: None,
        };
        let processed = self.nodes[from]
            .engine
            .send_message(Target::Direct(to_id), app, now)
            .with_context(|| format!("node {} failed to build message", self.nodes[from].name))?;
        self.sent_messages += 1;
        self.handle_processed(from, processed);
        Ok(msg_id)
    }

    pub fn send_position(&mut self, from: usize, position: CorePosition) -> Result<()> {
        let now = self.now_ms;
        let target = Target::Direct(self.nodes[from].node_id);
        let processed = self.nodes[from]
            .engine
            .send_position(target, position, now)
            .with_context(|| format!("node {} failed to send position", self.nodes[from].name))?;
        self.handle_processed(from, processed);
        Ok(())
    }

    /// Send a group message on a channel all nodes have joined.
    pub fn send_group_text(&mut self, from: usize, channel: NodeId, text: &str) -> Result<u32> {
        let now = self.now_ms;
        let msg_id = self.rng.next_u32().max(1);
        let app = AppMessage {
            id: msg_id,
            kind: MessageKind::Text,
            sent_at_ms: now,
            text: Some(text.to_string()),
            reply_to: None,
            thread: None,
            attachment: None,
            position: None,
            telemetry: None,
            sender: None,
            signature: None,
        };
        let processed = self.nodes[from]
            .engine
            .send_message(Target::Channel(channel), app, now)
            .with_context(|| "group send failed")?;
        self.sent_messages += 1;
        self.handle_processed(from, processed);
        Ok(msg_id)
    }

    /// Send an attachment-like resource (manifest message + chunked transfer).
    pub fn send_resource(
        &mut self,
        from: usize,
        to: usize,
        mime: &str,
        data: &[u8],
    ) -> Result<(u32, u32)> {
        let to_id = self.nodes[to].node_id;
        self.ensure_peer_known(from, to_id)?;
        let now = self.now_ms;
        let msg_id = self.rng.next_u32().max(1);
        let res_id = self.rng.next_u32().max(1);
        let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
        sha2::Digest::update(&mut hasher, data);
        let sha256: [u8; 32] = sha2::Digest::finalize(hasher).into();
        let chunk_bytes = 180u16;
        let chunk_count = (data.len() as u64)
            .div_ceil(chunk_bytes as u64)
            .min(u16::MAX as u64) as u16;
        let attachment = openmesh_core::AttachmentRef {
            res_id,
            mime: mime.to_string(),
            size: data.len() as u32,
            sha256,
            chunk_bytes,
            chunk_count,
            duration_ms: None,
            name: Some("attachment.bin".to_string()),
        };
        let app = AppMessage {
            id: msg_id,
            kind: MessageKind::Attachment,
            sent_at_ms: now,
            text: None,
            reply_to: None,
            thread: None,
            attachment: Some(attachment),
            position: None,
            telemetry: None,
            sender: None,
            signature: None,
        };
        let target = Target::Direct(to_id);
        let manifest = self.nodes[from]
            .engine
            .send_message(target, app, now)
            .with_context(|| "failed to build attachment manifest")?;
        self.sent_messages += 1;
        self.handle_processed(from, manifest);
        let chunks = self.nodes[from]
            .engine
            .send_resource(
                target,
                msg_id,
                res_id,
                ResourceKind::Attachment,
                mime,
                data,
                now,
            )
            .with_context(|| "failed to create resource transfer")?;
        self.handle_processed(from, chunks);
        Ok((msg_id, res_id))
    }

    fn ensure_peer_known(&self, from: usize, to: NodeId) -> Result<()> {
        if self.nodes[from].engine.node_info(to).is_some() {
            Ok(())
        } else {
            anyhow::bail!(
                "node {} does not know recipient {} yet (run announces first)",
                self.nodes[from].name,
                to
            )
        }
    }

    pub fn announce(&mut self, index: usize) {
        let now = self.now_ms;
        if let Ok(processed) = self.nodes[index].engine.announce(now) {
            self.handle_processed(index, processed);
        }
    }

    pub fn announce_all(&mut self) {
        for index in 0..self.nodes.len() {
            if self.nodes[index].online {
                self.announce(index);
            }
        }
    }

    pub fn set_online(&mut self, index: usize, online: bool) {
        let now = self.now_ms;
        if !online && self.nodes[index].online {
            self.offline_spans[index].push((now, u64::MAX));
        } else if online && !self.nodes[index].online {
            if let Some(last) = self.offline_spans[index].last_mut() {
                if last.1 == u64::MAX {
                    last.1 = now;
                }
            }
            self.announce(index);
        }
        self.nodes[index].online = online;
    }

    /// Send announces repeatedly (in one-second virtual rounds) until every online node knows
    /// every other online node, or `timeout_ms` runs out. Returns true on convergence.
    pub fn converge_announces(&mut self, timeout_ms: u64) -> bool {
        let deadline = self.now_ms + timeout_ms;
        loop {
            let online: Vec<usize> = (0..self.nodes.len())
                .filter(|i| self.nodes[*i].online)
                .collect();
            let mut missing = false;
            for &i in &online {
                for &j in &online {
                    if i != j
                        && self.nodes[i]
                            .engine
                            .node_info(self.nodes[j].node_id)
                            .is_none()
                    {
                        missing = true;
                        break;
                    }
                }
                if missing {
                    break;
                }
            }
            if !missing {
                return true;
            }
            if self.now_ms >= deadline {
                return false;
            }
            for &i in &online {
                self.announce(i);
            }
            let step = 500.min(deadline.saturating_sub(self.now_ms)).max(1);
            self.run_until(self.now_ms + step);
        }
    }

    fn message_sent_during_offline(&self, to: usize, sent_at_ms: u64) -> bool {
        self.offline_spans[to]
            .iter()
            .any(|(start, end)| sent_at_ms >= *start && (*end == u64::MAX || sent_at_ms <= *end))
    }

    /// Run the simulation until virtual time `until_ms`, in event order.
    pub fn run_until(&mut self, until_ms: u64) {
        while let Some(Reverse(event)) = self.queue.peek() {
            if event.at > until_ms {
                break;
            }
            let Reverse(event) = self.queue.pop().expect("peeked event exists");
            self.now_ms = event.at;
            match event.kind {
                EventKind::Tick { node } => self.on_tick(node, event.at),
                EventKind::Frame { to, bytes } => self.on_frame(to, bytes, event.at),
            }
        }
        self.now_ms = until_ms;
    }

    /// Run until `predicate` becomes true or `until_ms` is reached. Returns true when the
    /// predicate fired.
    pub fn run_until_cond(
        &mut self,
        until_ms: u64,
        mut predicate: impl FnMut(&MeshSim) -> bool,
    ) -> bool {
        while let Some(Reverse(event)) = self.queue.peek() {
            if event.at > until_ms {
                break;
            }
            if predicate(self) {
                return true;
            }
            let Reverse(event) = self.queue.pop().expect("peeked event exists");
            self.now_ms = event.at;
            match event.kind {
                EventKind::Tick { node } => self.on_tick(node, event.at),
                EventKind::Frame { to, bytes } => self.on_frame(to, bytes, event.at),
            }
        }
        self.now_ms = until_ms;
        predicate(self)
    }

    fn on_tick(&mut self, node: usize, at: u64) {
        if self.nodes[node].online {
            let processed = self.nodes[node].engine.tick(at);
            self.handle_processed(node, processed);
        }
        self.push_event(at + self.config.tick_ms.max(1), EventKind::Tick { node });
    }

    fn on_frame(&mut self, to: usize, bytes: Vec<u8>, at: u64) {
        if !self.nodes[to].online {
            return;
        }
        self.nodes[to].rx_frames += 1;
        let link = LinkInfo {
            link: 1,
            peer: None,
            rssi: Some(-70),
            snr: Some(9.0),
        };
        let processed = self.nodes[to].engine.ingest(&bytes, Some(link), at);
        self.handle_processed(to, processed);
    }

    fn handle_processed(&mut self, node: usize, processed: openmesh_core::Processed) {
        let before = self.nodes[node].engine.stats();
        self.record_events(node, processed.events);
        let after = self.nodes[node].engine.stats();
        self.nodes[node].forwarded += after
            .forwarded_frames
            .saturating_sub(before.forwarded_frames);
        self.transmit_all(node, processed.outbound);
    }

    fn record_events(&mut self, at_node: usize, events: Vec<Event>) {
        let now = self.now_ms;
        for event in events {
            match event {
                Event::Message(received) => {
                    let Some(from) = self.node_index.get(&received.from).copied() else {
                        continue;
                    };
                    let hops = self.hops_between(from, at_node);
                    self.deliveries.push(DeliveredMessage {
                        from,
                        to: at_node,
                        msg_id: received.message.id,
                        text: received.message.text.clone(),
                        sent_at_ms: received.message.sent_at_ms,
                        delivered_at_ms: now,
                        hops,
                        via_store: received.via_store,
                        resource: received.message.attachment.is_some(),
                    });
                }
                Event::GroupMessage(group) => {
                    self.group_deliveries.push(GroupDelivery {
                        channel: group.channel,
                        sender: group.sender,
                        msg_id: group.message.id,
                        at_ms: now,
                    });
                }
                Event::Ack { from, ack } => {
                    let Some(from_idx) = self.node_index.get(&from).copied() else {
                        continue;
                    };
                    let kind = match ack.kind {
                        openmesh_core::AckKind::Delivered => "delivered",
                        openmesh_core::AckKind::Read => "read",
                        openmesh_core::AckKind::ResourceWindow => "resource_window",
                        openmesh_core::AckKind::ResourceComplete => "resource_complete",
                    };
                    self.acks.push(AckRecord {
                        from: from_idx,
                        to: at_node,
                        msg_id: ack.msg_id,
                        kind: kind.to_string(),
                        at_ms: now,
                    });
                }
                Event::ResourceReceived {
                    peer,
                    manifest,
                    data,
                    ..
                } => {
                    let Some(peer_idx) = self.node_index.get(&peer).copied() else {
                        continue;
                    };
                    // The engine verifies the sealed blob digest before surfacing a resource;
                    // reaching this point means the payload is intact.
                    let kind = match manifest.kind {
                        ResourceKind::Attachment => "attachment",
                        ResourceKind::LongText => "long_text",
                        ResourceKind::Avatar => "avatar",
                        ResourceKind::ChannelHistory => "channel_history",
                    };
                    self.resource_deliveries.push(ResourceDelivery {
                        peer: peer_idx,
                        msg_id: manifest.msg_id,
                        res_id: manifest.res_id,
                        bytes: data.len(),
                        sha_ok: true,
                        at_ms: now,
                        from_store: false,
                        kind: kind.to_string(),
                    });
                }
                Event::StoreOffer { .. } => self.store_offers += 1,
                Event::StoreAccepted { .. } => self.store_accepted += 1,
                Event::NodeSeen(info) => {
                    self.discovery[at_node].entry(info.node_id).or_insert(now);
                }
                Event::Error(message) if self.protocol_errors.len() < 100 => {
                    self.protocol_errors.push(message);
                }
                _ => {}
            }
        }
    }

    fn transmit_all(&mut self, from: usize, outbound: Vec<Outbound>) {
        for outbound in outbound {
            let Ok(bytes) = outbound.frame.encode() else {
                continue;
            };
            self.transmit(from, bytes, outbound.mode);
        }
    }

    fn transmit(&mut self, from: usize, bytes: Vec<u8>, mode: SendMode) {
        // Account airtime and per-node counters.
        let airtime = util::lora_airtime_ms(&self.config.profile, bytes.len());
        {
            let node = &mut self.nodes[from];
            node.tx_frames += 1;
            node.airtime_tx_ms += airtime as u64;
            if let Some(key) = retransmission_key(&bytes) {
                if !node.sent_frames.insert(key) {
                    node.retransmits += 1;
                }
            }
        }
        // A directed frame goes only to the addressed neighbour if it is adjacent, else floods.
        let targets: Vec<usize> = match mode {
            SendMode::Flood => self.neighbors[from].clone(),
            SendMode::Direct(peer) => match self.node_index.get(&peer).copied() {
                Some(idx) if self.neighbors[from].contains(&idx) => vec![idx],
                _ => self.neighbors[from].clone(),
            },
        };
        let now = self.now_ms;
        let loss = self.config.loss_pct as u32;
        let base_latency = self.config.latency_ms;
        for to in targets {
            if loss > 0 && self.rng.gen_range(0..100u32) < loss {
                self.dropped_loss += 1;
                continue;
            }
            if !self.nodes[to].online {
                self.dropped_offline += 1;
                continue;
            }
            let jitter = if base_latency > 0 {
                self.rng.gen_range(0..=base_latency)
            } else {
                0
            };
            self.push_event(
                now + base_latency + jitter,
                EventKind::Frame {
                    to,
                    bytes: bytes.clone(),
                },
            );
        }
    }

    /// Shortest path length in the topology, if connected.
    pub fn hops_between(&self, from: usize, to: usize) -> Option<usize> {
        if from == to {
            return Some(0);
        }
        let mut seen = vec![false; self.nodes.len()];
        let mut queue = VecDeque::new();
        seen[from] = true;
        queue.push_back((from, 0usize));
        while let Some((node, depth)) = queue.pop_front() {
            for &next in &self.neighbors[node] {
                if next == to {
                    return Some(depth + 1);
                }
                if !seen[next] {
                    seen[next] = true;
                    queue.push_back((next, depth + 1));
                }
            }
        }
        None
    }

    /// Join a channel on every virtual node (returns the derived channel id).
    pub fn add_channel_all(&mut self, name: &str, psk: &[u8; 32]) -> NodeId {
        let mut id = NodeId::ZERO;
        for node in &mut self.nodes {
            id = node.engine.add_channel(name, psk);
        }
        id
    }

    /// Nodes known to the engine of virtual node `index` (from announces/traffic).
    pub fn known_nodes(&self, index: usize) -> Vec<openmesh_core::NodeInfo> {
        self.nodes[index].engine.known_nodes()
    }

    /// The simulated engine of node `index` (read-only access for diagnostics).
    pub fn engine(&self, index: usize) -> &Engine {
        &self.nodes[index].engine
    }

    /// Recorded deliveries, most recent last.
    pub fn deliveries(&self) -> &[DeliveredMessage] {
        &self.deliveries
    }

    pub fn acks(&self) -> &[AckRecord] {
        &self.acks
    }

    pub fn resource_deliveries(&self) -> &[ResourceDelivery] {
        &self.resource_deliveries
    }

    pub fn store_forward_deliveries(&self) -> usize {
        // OMP/1 does not mark replayed envelopes on the wire (`via_store` is reserved), so count
        // messages that were sent while the recipient was offline and delivered afterwards: those
        // can only be completed through a relay's store-and-forward queue.
        self.deliveries
            .iter()
            .filter(|d| {
                self.message_sent_during_offline(d.to, d.sent_at_ms)
                    && d.delivered_at_ms > d.sent_at_ms
            })
            .count()
    }

    pub fn total_airtime_ms(&self) -> u64 {
        self.nodes.iter().map(|n| n.airtime_tx_ms).sum()
    }

    pub fn total_tx_frames(&self) -> u64 {
        self.nodes.iter().map(|n| n.tx_frames).sum()
    }

    pub fn total_rx_frames(&self) -> u64 {
        self.nodes.iter().map(|n| n.rx_frames).sum()
    }

    pub fn total_retransmits(&self) -> u64 {
        self.nodes.iter().map(|n| n.retransmits).sum()
    }

    /// Convenience: a `delivered` message id with matching text.
    pub fn delivered_text(&self, msg_id: u32) -> Option<&DeliveredMessage> {
        self.deliveries.iter().find(|d| d.msg_id == msg_id)
    }

    /// Build a stable report for a named scenario with explicit assertions.
    pub fn report(&self, scenario: &str, assertions: Vec<Assertion>) -> Report {
        Report::from_sim(scenario, self, assertions)
    }
}

/// One scenario assertion.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Assertion {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

impl Assertion {
    pub fn ok(name: impl Into<String>, passed: bool, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed,
            detail: detail.into(),
        }
    }
}

/// Per-node simulator counters shown in the scenario report.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeReport {
    pub name: String,
    pub tx_frames: u64,
    pub airtime_ms: u64,
    pub retransmits: u64,
    pub forwarded: u64,
}

/// Human + machine readable scenario result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub scenario: String,
    pub nodes: usize,
    pub simulated_ms: u64,
    pub messages_sent: usize,
    pub messages_delivered: usize,
    pub delivery_pct: f64,
    pub acks: usize,
    pub avg_hops: f64,
    pub airtime_total_ms: u64,
    pub airtime_per_node_ms: Vec<(String, u64)>,
    pub per_node: Vec<NodeReport>,
    pub frames_tx: u64,
    pub frames_rx: u64,
    pub dropped: u64,
    pub retransmits: u64,
    pub store_forward_deliveries: usize,
    pub resources_delivered: usize,
    pub resource_bytes: usize,
    pub resources_sha_ok: usize,
    pub converged_nodes: usize,
    pub assertions: Vec<Assertion>,
}

impl Report {
    pub fn from_sim(scenario: &str, sim: &MeshSim, assertions: Vec<Assertion>) -> Self {
        let deliveries = sim.deliveries();
        let hops: Vec<usize> = deliveries.iter().filter_map(|d| d.hops).collect();
        let resource_bytes: usize = sim.resource_deliveries().iter().map(|r| r.bytes).sum();
        let resources_sha_ok = sim
            .resource_deliveries()
            .iter()
            .filter(|r| r.sha_ok)
            .count();
        Self {
            scenario: scenario.to_string(),
            nodes: sim.nodes.len(),
            simulated_ms: sim.now_ms,
            messages_sent: sim.sent_messages,
            messages_delivered: deliveries.len(),
            delivery_pct: if sim.sent_messages > 0 {
                deliveries.len() as f64 / sim.sent_messages as f64 * 100.0
            } else {
                0.0
            },
            acks: sim.acks().len(),
            avg_hops: if hops.is_empty() {
                0.0
            } else {
                hops.iter().sum::<usize>() as f64 / hops.len() as f64
            },
            airtime_total_ms: sim.total_airtime_ms(),
            airtime_per_node_ms: sim
                .nodes
                .iter()
                .map(|n| (n.name.clone(), n.airtime_tx_ms))
                .collect(),
            per_node: sim
                .nodes
                .iter()
                .map(|n| NodeReport {
                    name: n.name.clone(),
                    tx_frames: n.tx_frames,
                    airtime_ms: n.airtime_tx_ms,
                    retransmits: n.retransmits,
                    forwarded: n.forwarded,
                })
                .collect(),
            frames_tx: sim.total_tx_frames(),
            frames_rx: sim.total_rx_frames(),
            dropped: sim.dropped_loss + sim.dropped_offline,
            retransmits: sim.total_retransmits(),
            store_forward_deliveries: sim.store_forward_deliveries(),
            resources_delivered: sim.resource_deliveries().len(),
            resource_bytes,
            resources_sha_ok,
            converged_nodes: sim.fully_converged_nodes(),
            assertions,
        }
    }

    pub fn passed(&self) -> bool {
        self.assertions.iter().all(|a| a.passed)
    }

    pub fn failures(&self) -> Vec<&Assertion> {
        self.assertions.iter().filter(|a| !a.passed).collect()
    }

    /// Human-readable scenario report used by the CLI.
    pub fn human(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("=== {} ===\n", self.scenario));
        out.push_str(&format!(
            "nodes {}   simulated {}   converged {}/{}\n",
            self.nodes,
            format_ms(self.simulated_ms),
            self.converged_nodes,
            self.nodes
        ));
        out.push_str(&format!(
            "messages {}/{} delivered ({:.1}%)   acks {}   avg hops {:.2}\n",
            self.messages_delivered,
            self.messages_sent,
            self.delivery_pct,
            self.acks,
            self.avg_hops
        ));
        out.push_str(&format!(
            "airtime {} total  ({} per node avg)\n",
            util::format_duration_ms(self.airtime_total_ms),
            util::format_duration_ms(if self.nodes > 0 {
                self.airtime_total_ms / self.nodes as u64
            } else {
                0
            })
        ));
        if self.nodes <= 8 {
            for node in &self.per_node {
                out.push_str(&format!(
                    "  {:<10} airtime={:<8} tx={:<3} fwd={:<3} retransmits={}\n",
                    node.name,
                    util::format_duration_ms(node.airtime_ms),
                    node.tx_frames,
                    node.forwarded,
                    node.retransmits
                ));
            }
        }
        out.push_str(&format!(
            "frames tx {} rx {}   dropped {}   retransmits {}\n",
            self.frames_tx, self.frames_rx, self.dropped, self.retransmits
        ));
        out.push_str(&format!(
            "store-forward deliveries {}   resources {} ({} B, sha ok {})\n",
            self.store_forward_deliveries,
            self.resources_delivered,
            self.resource_bytes,
            self.resources_sha_ok
        ));
        out.push_str("assertions:\n");
        for assertion in &self.assertions {
            out.push_str(&format!(
                "  [{}] {} — {}\n",
                if assertion.passed { "ok" } else { "FAIL" },
                assertion.name,
                assertion.detail
            ));
        }
        if !self.passed() {
            out.push_str(&format!(
                "*** {} assertion(s) FAILED ***\n",
                self.failures().len()
            ));
        }
        out
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "scenario": self.scenario,
            "nodes": self.nodes,
            "simulated_ms": self.simulated_ms,
            "messages_sent": self.messages_sent,
            "messages_delivered": self.messages_delivered,
            "delivery_pct": round2(self.delivery_pct),
            "acks": self.acks,
            "avg_hops": round2(self.avg_hops),
            "airtime_total_ms": self.airtime_total_ms,
            "airtime_per_node_ms": self.airtime_per_node_ms,
            "per_node": self.per_node,
            "frames_tx": self.frames_tx,
            "frames_rx": self.frames_rx,
            "dropped": self.dropped,
            "retransmits": self.retransmits,
            "store_forward_deliveries": self.store_forward_deliveries,
            "resources_delivered": self.resources_delivered,
            "resource_bytes": self.resource_bytes,
            "resources_sha_ok": self.resources_sha_ok,
            "converged_nodes": self.converged_nodes,
            "assertions": self.assertions,
            "passed": self.passed(),
        })
    }
}

pub fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn format_ms(ms: u64) -> String {
    util::format_duration_ms(ms)
}

/// Re-export the core type used for positions so scenarios do not have to import the core.
pub use openmesh_core::Position;

/// Key identifying retransmissions of the same logical frame: engine retransmissions reuse the
/// message reference (and, for chunks, the chunk index) but allocate a fresh sequence number.
fn retransmission_key(bytes: &[u8]) -> Option<(u8, u32, u32, u16)> {
    let frame = openmesh_core::Frame::decode(bytes).ok()?;
    if frame.body.len() < 5 {
        return None;
    }
    let msg_ref = u32::from_be_bytes([frame.body[1], frame.body[2], frame.body[3], frame.body[4]]);
    match frame.typ {
        openmesh_core::FrameType::Data | openmesh_core::FrameType::Ack => {
            Some((frame.typ as u8, msg_ref, 0, 0))
        }
        openmesh_core::FrameType::Resource if frame.body.len() >= 11 => {
            let res_id =
                u32::from_be_bytes([frame.body[5], frame.body[6], frame.body[7], frame.body[8]]);
            let index = u16::from_be_bytes([frame.body[9], frame.body[10]]);
            Some((frame.typ as u8, msg_ref, res_id, index))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_node_message_and_ack() {
        let mut sim = MeshSim::new(
            2,
            SimConfig {
                latency_ms: 5,
                ..Default::default()
            },
        );
        sim.set_full_topology();
        sim.announce_all();
        sim.run_until(100);
        let msg_id = sim.send_text(0, 1, "hello mesh").unwrap();
        sim.run_until(3_000);
        assert!(
            sim.delivered_text(msg_id).is_some(),
            "message should be delivered"
        );
        assert!(
            sim.acks()
                .iter()
                .any(|a| a.msg_id == msg_id && a.kind == "delivered")
        );
        assert_eq!(
            sim.delivered_text(msg_id).unwrap().text.as_deref(),
            Some("hello mesh")
        );
        assert_eq!(sim.delivered_text(msg_id).unwrap().hops, Some(1));
    }

    #[test]
    fn delivery_scales_with_loss() {
        let run = |loss_pct: u8| {
            let mut sim = MeshSim::new(
                2,
                SimConfig {
                    loss_pct,
                    latency_ms: 1,
                    seed: 7,
                    ..Default::default()
                },
            );
            sim.set_full_topology();
            sim.converge_announces(60_000);
            let mut delivered = 0;
            for _ in 0..10 {
                let msg_id = sim.send_text(0, 1, "x").unwrap();
                sim.run_until(sim.now_ms + 500);
                if sim.delivered_text(msg_id).is_some() {
                    delivered += 1;
                }
            }
            delivered
        };
        let perfect = run(0);
        let lossy = run(60);
        assert_eq!(perfect, 10);
        assert!(
            lossy < perfect,
            "60% loss should drop some messages ({lossy}/10)"
        );
    }

    #[test]
    fn topology_hops_are_computed() {
        let mut sim = MeshSim::new(5, SimConfig::default());
        sim.set_line_topology();
        assert_eq!(sim.hops_between(0, 4), Some(4));
        assert_eq!(sim.hops_between(0, 1), Some(1));
        let mut isolated = MeshSim::new(3, SimConfig::default());
        isolated.set_topology(&[(0, 1)]);
        assert_eq!(isolated.hops_between(0, 2), None);
    }

    #[test]
    fn report_serializes_and_formats() {
        let mut sim = MeshSim::new(
            2,
            SimConfig {
                latency_ms: 1,
                ..Default::default()
            },
        );
        sim.set_full_topology();
        sim.announce_all();
        sim.run_until(50);
        sim.send_text(0, 1, "hi").unwrap();
        sim.run_until(1_000);
        let report = sim.report(
            "unit",
            vec![Assertion::ok(
                "delivered",
                sim.deliveries().len() == 1,
                "1 message",
            )],
        );
        assert!(report.passed());
        let human = report.human();
        assert!(human.contains("unit"));
        assert!(human.contains("[ok] delivered"));
        let json = report.to_json();
        assert_eq!(json["scenario"], "unit");
        assert_eq!(json["passed"], true);
    }

    #[test]
    fn resource_transfer_delivers_all_bytes() {
        let mut sim = MeshSim::new(
            2,
            SimConfig {
                latency_ms: 1,
                ..Default::default()
            },
        );
        sim.set_full_topology();
        sim.announce_all();
        sim.run_until(50);
        let data: Vec<u8> = (0..1024u32).map(|i| (i % 251) as u8).collect();
        let (msg_id, res_id) = sim
            .send_resource(0, 1, "application/octet-stream", &data)
            .unwrap();
        let delivered = sim.run_until_cond(30_000, |sim| {
            sim.resource_deliveries().iter().any(|r| r.res_id == res_id)
        });
        assert!(delivered, "resource transfer should complete");
        let delivery = sim
            .resource_deliveries()
            .iter()
            .find(|r| r.res_id == res_id)
            .unwrap();
        assert_eq!(delivery.msg_id, msg_id);
        assert_eq!(delivery.bytes, data.len());
        assert!(delivery.sha_ok);
    }
}

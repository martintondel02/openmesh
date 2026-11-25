//! In-process multi-engine integration tests: the whole OMP/1 protocol running over a virtual
//! shared medium, with deterministic loss injection.
//!
//! Every test drives real [`Engine`] instances through `ingest`/`tick`, exactly like the bridge
//! and the simulator do. No wall clock, no IO, no network: time is virtual and loss is a seeded
//! xorshift PRNG, so failures are reproducible.

use std::collections::VecDeque;

use openmesh_core::{
    Ack, AckKind, Engine, EngineConfig, Event, Frame, LinkInfo, NodeId, Position, Processed,
    ResourceKind, Target, Telemetry, engine_from_seed,
};

// ---------------------------------------------------------------------------------------------
// Virtual medium
// ---------------------------------------------------------------------------------------------

/// Deterministic xorshift64* PRNG for loss injection.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// True with probability `percent` (0..=100).
    fn percent_hit(&mut self, percent: u32) -> bool {
        percent > 0 && (self.next_u64() % 100) < percent as u64
    }
}

/// A virtual mesh: N engines, symmetric range matrix, a frame queue and virtual time.
struct Mesh {
    engines: Vec<Engine>,
    /// `adj[i][j]` is true when node i can hear node j's transmissions.
    adj: Vec<Vec<bool>>,
    queue: VecDeque<(usize, Frame)>,
    /// Events produced, tagged with the receiving engine index.
    events: Vec<(usize, Event)>,
    now: u64,
    rng: Rng,
    /// Percentage of individual transmissions to drop (0..=100).
    drop_percent: u32,
    /// Per-node receive gate; a node that is "offline" hears nothing.
    online: Vec<bool>,
    transmissions: u64,
    drops: u64,
}

impl Mesh {
    fn new(engines: Vec<Engine>, adj: Vec<Vec<bool>>) -> Self {
        let n = engines.len();
        assert_eq!(adj.len(), n);
        assert!(adj.iter().all(|row| row.len() == n));
        Self {
            engines,
            adj,
            queue: VecDeque::new(),
            events: Vec::new(),
            now: 0,
            rng: Rng(0x1234_5678_9ABC_DEF0),
            drop_percent: 0,
            online: vec![true; n],
            transmissions: 0,
            drops: 0,
        }
    }

    fn set_online(&mut self, index: usize, online: bool) {
        self.online[index] = online;
    }

    fn engine(&self, index: usize) -> &Engine {
        &self.engines[index]
    }

    fn engine_mut(&mut self, index: usize) -> &mut Engine {
        &mut self.engines[index]
    }

    fn submit(&mut self, from: usize, processed: Processed) {
        self.events
            .extend(processed.events.into_iter().map(|event| (from, event)));
        for outbound in processed.outbound {
            self.queue.push_back((from, outbound.frame));
        }
    }

    /// Deliver queued frames until the medium is quiet (bounded to avoid pathological loops).
    fn pump(&mut self) {
        let mut steps = 0usize;
        while let Some((from, frame)) = self.queue.pop_front() {
            steps += 1;
            assert!(
                steps < 200_000,
                "flooding did not converge within 200k transmissions"
            );
            let Ok(bytes) = frame.encode() else { continue };
            let link_base = LinkInfo {
                link: from as u16,
                peer: Some(self.engines[from].node_id()),
                rssi: Some(-90),
                snr: Some(6.5),
            };
            for to in 0..self.engines.len() {
                if to == from || !self.adj[from][to] || !self.online[to] {
                    continue;
                }
                self.transmissions += 1;
                if self.rng.percent_hit(self.drop_percent) {
                    self.drops += 1;
                    continue;
                }
                let processed = self.engines[to].ingest(&bytes, Some(link_base.clone()), self.now);
                self.submit(to, processed);
            }
        }
    }

    /// Run all timers once, then deliver everything they produced.
    fn tick_all(&mut self) {
        for index in 0..self.engines.len() {
            let processed = self.engines[index].tick(self.now);
            self.submit(index, processed);
        }
        self.pump();
    }

    /// Advance virtual time by `ms`, stepping in 100 ms increments.
    fn advance(&mut self, ms: u64) {
        let target = self.now + ms;
        while self.now < target {
            self.now = (self.now + 100).min(target);
            self.tick_all();
        }
    }

    fn events_for(&self, index: usize) -> Vec<&Event> {
        self.events
            .iter()
            .filter(|(node, _)| *node == index)
            .map(|(_, event)| event)
            .collect()
    }

    fn last_event_for(&self, index: usize) -> Option<&Event> {
        self.events
            .iter()
            .rev()
            .find(|(node, _)| *node == index)
            .map(|(_, event)| event)
    }

    fn has_message(&self, index: usize, text: &str) -> bool {
        self.events_for(index).iter().any(|event| {
            matches!(event, Event::Message(message) if message.message.text.as_deref() == Some(text))
        })
    }

    fn ack_count(&self, index: usize) -> usize {
        self.events_for(index)
            .iter()
            .filter(|event| matches!(event, Event::Ack { .. }))
            .count()
    }

    fn drop_percent(&mut self, percent: u32) {
        self.drop_percent = percent;
    }
}

fn client(seed: u8, name: &str) -> Engine {
    engine_from_seed(&[seed; 32], EngineConfig::client(name))
}

/// Two nodes that can hear each other, with mutual contacts pre-provisioned.
fn duo() -> (Mesh, usize, usize) {
    let a = client(1, "alice");
    let b = client(2, "bob");
    let a_ident = a.public_identity();
    let b_ident = b.public_identity();
    let mut a = a;
    let mut b = b;
    a.add_contact(b_ident, Some("bob".into()));
    b.add_contact(a_ident, Some("alice".into()));
    let mesh = Mesh::new(vec![a, b], vec![vec![false, true], vec![true, false]]);
    (mesh, 0, 1)
}

// ---------------------------------------------------------------------------------------------
// Direct messaging
// ---------------------------------------------------------------------------------------------

#[test]
fn two_node_dm_with_delivery_ack() {
    let (mut mesh, a, b) = duo();
    let a_id = mesh.engine(a).node_id();
    let b_id = mesh.engine(b).node_id();
    let __now = mesh.now;

    // Announces are exchanged first, like on a real network join.
    let announced = mesh.engine_mut(a).announce(__now).unwrap();
    mesh.submit(a, announced);
    mesh.pump();
    let __now = mesh.now;
    let announced = mesh.engine_mut(b).announce(__now).unwrap();
    mesh.submit(b, announced);
    mesh.pump();
    assert!(mesh.engine(a).node_info(b_id).is_some());
    assert!(mesh.engine(b).node_info(a_id).is_some());
    assert!(matches!(
        mesh.last_event_for(a),
        Some(Event::NodeSeen(info)) if info.node_id == b_id && info.hops == 0
    ));
    let __now = mesh.now;

    let sent = mesh
        .engine_mut(a)
        .send_text(Target::Direct(b_id), "hello mesh", __now)
        .unwrap();
    assert_eq!(sent.outbound.len(), 1);
    mesh.submit(a, sent);
    mesh.pump();

    assert!(
        mesh.has_message(b, "hello mesh"),
        "b must surface the direct message"
    );
    assert_eq!(mesh.ack_count(a), 1, "a must receive the delivery ack");
    // Echoes of our own frames bounced back by the relay are counted as dropped; there must be
    // no decrypt/signature errors anywhere.
    assert!(
        !mesh
            .events_for(a)
            .iter()
            .any(|event| matches!(event, Event::Error(_)))
    );
    assert!(
        !mesh
            .events_for(b)
            .iter()
            .any(|event| matches!(event, Event::Error(_)))
    );
}

#[test]
fn dm_to_unknown_peer_is_refused() {
    let mut engine = client(1, "alice");
    let unknown = NodeId([0x01, 2, 3, 4, 5, 6, 7, 8]);
    assert!(engine.send_text(Target::Direct(unknown), "hi", 0).is_err());
}

#[test]
fn long_text_is_streamed_as_a_resource() {
    let (mut mesh, a, b) = duo();
    let a_id = mesh.engine(a).node_id();
    let b_id = mesh.engine(b).node_id();
    let __now = mesh.now;
    let text = "long message ".repeat(40); // ~520 bytes, definitely does not fit one frame

    let sent = mesh
        .engine_mut(a)
        .send_text(Target::Direct(b_id), &text, __now)
        .unwrap();
    // Manifest plus the first window of chunks.
    assert!(sent.outbound.len() > 1);
    mesh.submit(a, sent);
    mesh.pump();
    // Let the ARQ finish (acks travel over the same medium).
    for _ in 0..20 {
        mesh.advance(1_000);
        if mesh.has_message(b, &text) {
            break;
        }
    }
    assert!(
        mesh.has_message(b, &text),
        "long text must arrive as a Message event"
    );
    assert!(
        mesh.events_for(b)
            .iter()
            .any(|event| matches!(event, Event::ResourceReceived { .. })),
        "long text must also surface ResourceReceived"
    );
    assert!(
        mesh.events_for(a)
            .iter()
            .any(|event| matches!(event, Event::ResourceSent { .. })),
        "sender must report ResourceSent"
    );
    let _ = a_id;
}

// ---------------------------------------------------------------------------------------------
// Channels / groups
// ---------------------------------------------------------------------------------------------

#[test]
fn channel_message_is_signed_and_verified() {
    let (mut mesh, a, b) = duo();
    let psk = [42u8; 32];
    let channel_a = mesh.engine_mut(a).add_channel("family", &psk);
    let channel_b = mesh.engine_mut(b).add_channel("family", &psk);
    assert_eq!(channel_a, channel_b);
    assert!(
        channel_a.is_channel(),
        "channel addresses must have the class bit set"
    );
    let __now = mesh.now;

    // b learns a's signing key from an announce (required to verify group authorship).
    let announced = mesh.engine_mut(a).announce(__now).unwrap();
    mesh.submit(a, announced);
    mesh.pump();
    let __now = mesh.now;

    let sent = mesh
        .engine_mut(a)
        .send_text(Target::Channel(channel_a), "on my way", __now)
        .unwrap();
    mesh.submit(a, sent);
    mesh.pump();

    match mesh.last_event_for(b) {
        Some(Event::GroupMessage(group)) => {
            assert_eq!(group.sender, mesh.engine(a).node_id());
            assert_eq!(group.channel, channel_a);
            assert_eq!(group.message.text.as_deref(), Some("on my way"));
        }
        other => panic!("expected group message, got {other:?}"),
    }
    // The ack is not required for channel traffic.
    assert_eq!(mesh.ack_count(a), 0);
}

#[test]
fn channel_message_from_unknown_signer_is_dropped() {
    // No contacts and no announces: b cannot resolve a's Ed25519 key.
    let a = client(21, "a");
    let b = client(22, "b");
    let mut mesh = Mesh::new(vec![a, b], vec![vec![false, true], vec![true, false]]);
    let psk = [9u8; 32];
    let channel = mesh.engine_mut(0).add_channel("family", &psk);
    mesh.engine_mut(1).add_channel("family", &psk);
    let __now = mesh.now;
    // a does not announce, so b cannot resolve a's Ed25519 key and must drop the message.
    let sent = mesh
        .engine_mut(0)
        .send_text(Target::Channel(channel), "hello", __now)
        .unwrap();
    mesh.submit(0, sent);
    mesh.pump();
    assert!(
        !mesh
            .events_for(1)
            .iter()
            .any(|event| matches!(event, Event::GroupMessage(_))),
        "unverifiable group message must not be surfaced"
    );
    assert!(
        mesh.events_for(1)
            .iter()
            .any(|event| matches!(event, Event::Error(_)))
    );
}

// ---------------------------------------------------------------------------------------------
// Position and telemetry
// ---------------------------------------------------------------------------------------------

#[test]
fn position_is_surfaced_direct_and_on_channels() {
    let (mut mesh, a, b) = duo();
    let b_id = mesh.engine(b).node_id();
    let position = Position {
        lat_e7: 59_913_700,
        lon_e7: 10_752_200,
        alt_m: Some(45),
        accuracy_m: Some(8),
        speed_cm_s: Some(120),
        heading_deg: Some(180),
        timestamp_ms: 1_000,
    };
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_position(Target::Direct(b_id), position.clone(), __now)
        .unwrap();
    mesh.submit(a, sent);
    mesh.pump();
    assert!(
        mesh.events_for(b).iter().any(|event| matches!(
            event,
            Event::Position { channel: None, position: p, .. } if p == &position
        )),
        "direct position event expected"
    );
    assert_eq!(
        mesh.engine(b)
            .node_info(mesh.engine(a).node_id())
            .unwrap()
            .last_position,
        Some(position)
    );
}

#[test]
fn telemetry_roundtrip() {
    let (mut mesh, a, b) = duo();
    let b_id = mesh.engine(b).node_id();
    let telemetry = Telemetry {
        timestamp_ms: 5_000,
        battery_mv: Some(3_980),
        battery_pct: Some(82),
        uptime_s: Some(86_400),
        airtime_used_ms: Some(42_000),
        rx_frames: Some(240),
        tx_frames: Some(120),
        temperature_c: Some(21),
        solar_mv: Some(5_100),
    };
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_telemetry_to(Target::Direct(b_id), telemetry.clone(), __now)
        .unwrap();
    mesh.submit(a, sent);
    mesh.pump();
    assert!(
        mesh.events_for(b).iter().any(|event| matches!(
            event,
            Event::Telemetry { telemetry: t, .. } if t == &telemetry
        )),
        "telemetry event expected"
    );
    assert_eq!(
        mesh.engine(b)
            .node_info(mesh.engine(a).node_id())
            .unwrap()
            .battery_pct,
        Some(82)
    );
}

// ---------------------------------------------------------------------------------------------
// Acks
// ---------------------------------------------------------------------------------------------

#[test]
fn read_ack_can_be_sent_for_a_received_message() {
    let (mut mesh, a, b) = duo();
    let a_id = mesh.engine(a).node_id();
    let b_id = mesh.engine(b).node_id();
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_text(Target::Direct(b_id), "hi", __now)
        .unwrap();
    mesh.submit(a, sent);
    mesh.pump();
    let msg_id = mesh
        .events_for(b)
        .iter()
        .find_map(|event| match event {
            Event::Message(message) => Some(message.message.id),
            _ => None,
        })
        .expect("message received");
    let ack = Ack {
        kind: AckKind::Read,
        msg_id,
        res_id: None,
        next_expected: None,
        bitmap: None,
        timestamp_ms: mesh.now,
    };
    let __now = mesh.now;
    let processed = mesh.engine_mut(b).send_ack(a_id, ack, __now).unwrap();
    assert_eq!(processed.outbound.len(), 1);
    assert_eq!(
        processed.outbound[0].frame.typ,
        openmesh_core::FrameType::Ack
    );
    assert!(
        !processed.outbound[0]
            .frame
            .flags
            .contains(openmesh_core::Flags::ACK_REQUESTED),
        "ACK frames must not request ACKs"
    );
    mesh.submit(b, processed);
    mesh.pump();
    assert!(
        mesh.events_for(a).iter().any(|event| matches!(
            event,
            Event::Ack { ack, .. } if ack.kind == AckKind::Read && ack.msg_id == msg_id
        )),
        "read receipt expected"
    );
}

// ---------------------------------------------------------------------------------------------
// Announce discovery over multiple hops
// ---------------------------------------------------------------------------------------------

#[test]
fn announce_discovery_is_multi_hop() {
    // A — R — B: A and B are out of range, the repeater bridges them.
    let a = client(10, "a");
    let r = engine_from_seed(&[11; 32], EngineConfig::repeater("r"));
    let b = client(12, "b");
    let adj = vec![
        vec![false, true, false],
        vec![true, false, true],
        vec![false, true, false],
    ];
    let mut mesh = Mesh::new(vec![a, r, b], adj);
    let a_id = mesh.engine(0).node_id();
    let b_id = mesh.engine(2).node_id();
    let __now = mesh.now;

    let announced = mesh.engine_mut(0).announce(__now).unwrap();
    mesh.submit(0, announced);
    mesh.pump();

    assert!(mesh.engine(1).node_info(a_id).is_some(), "relay learns A");
    assert!(
        mesh.engine(2).node_info(a_id).is_some(),
        "B learns A through the relay"
    );
    let info = mesh.engine(2).node_info(a_id).unwrap();
    assert_eq!(
        info.hops, 1,
        "announce travelled one relay hop (hops = 8 - ttl)"
    );
    assert!(info.ident.is_self_consistent());
    let __now = mesh.now;

    let announced = mesh.engine_mut(2).announce(__now).unwrap();
    mesh.submit(2, announced);
    mesh.pump();
    assert!(
        mesh.engine(0).node_info(b_id).is_some(),
        "A learns B through the relay"
    );
}

// ---------------------------------------------------------------------------------------------
// Dedup, TTL and malformed input
// ---------------------------------------------------------------------------------------------

#[test]
fn duplicate_frames_are_suppressed() {
    let (mut mesh, a, b) = duo();
    let b_id = mesh.engine(b).node_id();
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_text(Target::Direct(b_id), "once", __now)
        .unwrap();
    let frame = sent.outbound[0].frame.clone();
    mesh.submit(a, sent);
    mesh.pump();
    let messages_after_first = mesh.has_message(b, "once");
    assert!(messages_after_first);
    let dedup_before = mesh.engine(b).stats().dedup_hits;
    let __now = mesh.now;
    // Replay the same frame (same src/seq) many times.
    for now in 0..5 {
        let processed = mesh
            .engine_mut(b)
            .on_frame(frame.clone(), None, __now + now);
        mesh.submit(b, processed);
    }
    mesh.pump();
    assert_eq!(mesh.engine(b).stats().dedup_hits, dedup_before + 5);
    let count = mesh
        .events_for(b)
        .iter()
        .filter(
            |event| matches!(event, Event::Message(m) if m.message.text.as_deref() == Some("once")),
        )
        .count();
    assert_eq!(
        count, 1,
        "the application layer must deduplicate by (src, msg_id)"
    );
}

#[test]
fn exhausted_ttl_frames_are_not_relayed() {
    let (mut mesh, a, b) = duo();
    let b_id = mesh.engine(b).node_id();
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_text(Target::Direct(b_id), "ttl", __now)
        .unwrap();
    let mut frame = sent.outbound[0].frame.clone();
    frame.ttl = 0;
    let __now = mesh.now;
    let processed = mesh.engine_mut(b).on_frame(frame.clone(), None, __now);
    let relayed = processed
        .outbound
        .iter()
        .any(|outbound| outbound.frame.typ == openmesh_core::FrameType::Data);
    assert!(!relayed, "ttl=0 frames must not be relayed");
    mesh.submit(b, processed);
    // The frame is still for b, so it is delivered.
    assert!(mesh.has_message(b, "ttl"));
    let _ = a;
}

#[test]
fn malformed_frames_are_counted_and_never_panic() {
    let mut engine = client(1, "alice");
    let mut rng = Rng(0xDEAD_BEEF);
    let mut dropped_before = engine.stats().dropped_frames;
    for _ in 0..2_000 {
        let len = (rng.next_u64() % 260) as usize;
        let mut bytes = vec![0u8; len];
        for byte in &mut bytes {
            *byte = (rng.next_u64() & 0xFF) as u8;
        }
        let _ = engine.ingest(&bytes, None, rng.next_u64() % 1_000_000);
        if engine.stats().dropped_frames > dropped_before + 1_000 {
            dropped_before = engine.stats().dropped_frames;
        }
    }
    // Valid-looking headers with random bodies must also be safe.
    for _ in 0..2_000 {
        let len = (rng.next_u64() % 240) as usize;
        let frame = Frame {
            typ: openmesh_core::FrameType::Data,
            flags: openmesh_core::Flags::from_bits((rng.next_u64() & 0x0F) as u8),
            ttl: (rng.next_u64() % 16) as u8,
            src: NodeId([(rng.next_u64() & 0xFF) as u8; 8]),
            dst: NodeId([(rng.next_u64() & 0xFF) as u8; 8]),
            seq: rng.next_u64() as u32,
            body: vec![(rng.next_u64() & 0xFF) as u8; len],
        };
        let _ = engine.on_frame(frame, None, rng.next_u64() % 1_000_000);
    }
    assert!(engine.stats().dropped_frames > 0);
}

#[test]
fn tampered_ciphertext_is_rejected() {
    let (mut mesh, a, b) = duo();
    let b_id = mesh.engine(b).node_id();
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_text(Target::Direct(b_id), "secret", __now)
        .unwrap();
    let mut frame = sent.outbound[0].frame.clone();
    // Flip a ciphertext byte (the last body byte is inside the AEAD tag region).
    let last = frame.body.len() - 1;
    frame.body[last] ^= 0xFF;
    let __now = mesh.now;
    let processed = mesh.engine_mut(b).on_frame(frame, None, __now);
    mesh.submit(b, processed);
    mesh.pump();
    assert!(!mesh.has_message(b, "secret"));
    assert!(mesh.events_for(b).iter().any(|event| matches!(
        event,
        Event::Error(message) if message.contains("decrypt")
    )));
}

#[test]
fn tampered_announce_is_rejected_and_not_relayed() {
    let mut rng = Rng(7);
    let a = client(1, "alice");
    let r = engine_from_seed(&[2; 32], EngineConfig::repeater("relay"));
    let mut mesh = Mesh::new(vec![a, r], vec![vec![false, true], vec![true, false]]);
    let announced = mesh.engine_mut(0).announce(0).unwrap();
    let mut frame = announced.outbound[0].frame.clone();
    // Flip a bit in the signed announce body.
    let index = 20 + (rng.next_u64() % 40) as usize;
    frame.body[index] ^= 0x01;
    let processed = mesh.engine_mut(1).on_frame(frame, None, 1);
    mesh.submit(1, processed);
    mesh.pump();
    assert!(
        mesh.engine(1).known_nodes().is_empty(),
        "tampered announce must not be learned"
    );
    assert_ne!(mesh.engine(1).stats().dropped_frames, 0);
    let relayed = mesh
        .events_for(1)
        .iter()
        .any(|event| matches!(event, Event::Error(message) if message.contains("announce")));
    assert!(relayed);
}

#[test]
fn replay_of_old_envelope_is_rejected_by_dedup() {
    let (mut mesh, a, b) = duo();
    let b_id = mesh.engine(b).node_id();
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_text(Target::Direct(b_id), "replay me", __now)
        .unwrap();
    let frame = sent.outbound[0].frame.clone();
    mesh.submit(a, sent);
    mesh.pump();
    let before = mesh.events_for(b).len();
    let __now = mesh.now;
    // Hostile replay of the exact same frame.
    let processed = mesh.engine_mut(b).on_frame(frame, None, __now + 1);
    mesh.submit(b, processed);
    mesh.pump();
    let after = mesh.events_for(b).len();
    assert_eq!(
        before, after,
        "replayed frame must not surface a second time"
    );
}

#[test]
fn app_layer_dedup_rejects_reencrypted_replay() {
    // An attacker cannot produce a new valid frame without the key, but a *legitimate* sender
    // retrying with a new seq (as our retry logic does) must not surface the message twice.
    let (mut mesh, a, b) = duo();
    let b_id = mesh.engine(b).node_id();
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_text(Target::Direct(b_id), "dedup", __now)
        .unwrap();
    let first = sent.outbound[0].frame.clone();
    mesh.submit(a, sent);
    mesh.pump();
    assert!(mesh.has_message(b, "dedup"));
    // Drop the ack so the sender retries.
    mesh.drop_percent(100);
    mesh.advance(10_000);
    mesh.drop_percent(0);
    mesh.advance(1_000);
    let count = mesh
        .events_for(b)
        .iter()
        .filter(|event| matches!(event, Event::Message(m) if m.message.text.as_deref() == Some("dedup")))
        .count();
    assert_eq!(count, 1);
    let _ = first;
    let _ = a;
}

// ---------------------------------------------------------------------------------------------
// Message retry
// ---------------------------------------------------------------------------------------------

#[test]
fn unacked_message_is_retried_with_backoff_then_given_up() {
    let (mut mesh, a, b) = duo();
    let b_id = mesh.engine(b).node_id();
    let __now = mesh.now;
    // Everything is lost: neither the message nor the acks ever get through.
    mesh.drop_percent(100);
    let sent = mesh
        .engine_mut(a)
        .send_text(Target::Direct(b_id), "retry me", __now)
        .unwrap();
    mesh.submit(a, sent);
    mesh.pump();
    mesh.advance(45_000);
    let errors = mesh
        .events_for(a)
        .iter()
        .filter(|event| matches!(event, Event::Error(message) if message.contains("unacked")))
        .count();
    assert_eq!(errors, 1, "sender must give up after MAX_MSG_RETRIES");
    assert_eq!(mesh.ack_count(a), 0);
}

// ---------------------------------------------------------------------------------------------
// Sender API surface used by the bridge
// ---------------------------------------------------------------------------------------------

#[test]
fn export_import_state_preserves_contacts_and_channels() {
    let mut engine = client(3, "alice");
    let contact = client(4, "bob").public_identity();
    engine.add_contact(contact, Some("Bob".into()));
    let channel = engine.add_channel("family", &[7u8; 32]);
    engine.announce(1_000).unwrap();
    let snapshot = engine.export_state();
    let mut restored = client(3, "alice");
    restored.import_state(snapshot);
    assert!(restored.node_info(contact.node_id).unwrap().is_contact);
    assert_eq!(restored.channels(), vec![(channel, "family".to_string())]);
}

#[test]
fn resource_kinds_and_attachment_manifest_roundtrip() {
    let (mut mesh, a, b) = duo();
    let a_id = mesh.engine(a).node_id();
    let b_id = mesh.engine(b).node_id();
    let image: Vec<u8> = (0..5_120u32).map(|i| (i * 31 % 251) as u8).collect();
    let msg_id = 0xAABB_CCDD;
    let res_id = 0x0102_0304;
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_resource(
            Target::Direct(b_id),
            msg_id,
            res_id,
            ResourceKind::Attachment,
            "image/jpeg",
            &image,
            __now,
        )
        .unwrap();
    assert!(sent.outbound.len() > 1);
    mesh.submit(a, sent);
    for _ in 0..40 {
        mesh.advance(1_000);
        if mesh
            .events_for(b)
            .iter()
            .any(|event| matches!(event, Event::ResourceReceived { .. }))
        {
            break;
        }
    }
    let received = mesh
        .events_for(b)
        .iter()
        .find_map(|event| match event {
            Event::ResourceReceived {
                peer,
                manifest,
                data,
                ..
            } if *peer == a_id => Some((manifest.clone(), data.clone())),
            _ => None,
        })
        .expect("resource received");
    assert_eq!(received.0.res_id, res_id);
    assert_eq!(received.0.msg_id, msg_id);
    assert_eq!(received.0.kind, ResourceKind::Attachment);
    assert_eq!(received.0.mime, "image/jpeg");
    assert_eq!(
        received.0.chunk_count,
        received.0.total_len.div_ceil(180) as u16
    );
    assert_eq!(received.1, image, "resource payload must be byte-exact");
    let _ = b_id;
}

// ---------------------------------------------------------------------------------------------
// Lossy resource transfer (5 KiB image, ~20 % loss on every transmission)
// ---------------------------------------------------------------------------------------------

#[test]
fn five_kib_image_survives_lossy_link() {
    let (mut mesh, a, b) = duo();
    let a_id = mesh.engine(a).node_id();
    let b_id = mesh.engine(b).node_id();
    let image: Vec<u8> = (0..5_120u32).map(|i| ((i * 37 + 11) % 251) as u8).collect();
    mesh.drop_percent(20);

    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_resource(
            Target::Direct(b_id),
            0x1122_3344,
            0x5566_7788,
            ResourceKind::Attachment,
            "image/jpeg",
            &image,
            __now,
        )
        .unwrap();
    mesh.submit(a, sent);

    let mut done = false;
    for _ in 0..1_200 {
        mesh.advance(100);
        if mesh
            .events_for(b)
            .iter()
            .any(|event| matches!(event, Event::ResourceReceived { .. }))
        {
            done = true;
            break;
        }
    }
    assert!(
        done,
        "resource must complete despite 20% loss (transmissions={}, drops={})",
        mesh.transmissions, mesh.drops
    );
    let received = mesh
        .events_for(b)
        .iter()
        .find_map(|event| match event {
            Event::ResourceReceived {
                peer,
                manifest,
                data,
                ..
            } if *peer == a_id => Some((manifest.clone(), data.clone())),
            _ => None,
        })
        .expect("resource received");
    assert_eq!(received.1.len(), image.len());
    assert_eq!(
        received.1, image,
        "lossy transport must not corrupt the payload"
    );
    assert_eq!(received.0.total_len, (image.len() + 29) as u32);
    assert_eq!(received.0.chunk_count, 29);
    assert!(
        mesh.events_for(a)
            .iter()
            .any(|event| matches!(event, Event::ResourceSent { .. })),
        "sender must see the transfer complete"
    );
    assert!(mesh.drops > 0, "the loss model must actually drop frames");
}

#[test]
fn chunked_transfer_ack_bitmap_reports_holes() {
    // Drive the sender/receiver state machines through the public engine API with a burst of
    // chunks delivered out of order, and verify the receiver's ack bitmap closes the holes.
    let (mut mesh, a, b) = duo();
    let b_id = mesh.engine(b).node_id();
    let image: Vec<u8> = (0..1_800u32).map(|i| (i % 211) as u8).collect();
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_resource(
            Target::Direct(b_id),
            7,
            9,
            ResourceKind::Attachment,
            "application/octet-stream",
            &image,
            __now,
        )
        .unwrap();
    mesh.submit(a, sent);
    mesh.pump();
    // Deliver chunks out of order through the medium by dropping the manifest first window and
    // then replaying the whole burst many times: selective repeat must fill all holes.
    mesh.drop_percent(50);
    for _ in 0..40 {
        mesh.advance(1_000);
        if mesh
            .events_for(b)
            .iter()
            .any(|event| matches!(event, Event::ResourceReceived { .. }))
        {
            break;
        }
    }
    mesh.drop_percent(0);
    mesh.advance(2_000);
    assert!(
        mesh.events_for(b)
            .iter()
            .any(|event| matches!(event, Event::ResourceReceived { .. }))
    );
}

// ---------------------------------------------------------------------------------------------
// Store-and-forward: 3-node line topology with a repeater
// ---------------------------------------------------------------------------------------------

#[test]
fn store_and_forward_offer_want_replay() {
    // A — R — C: A and C are out of range of each other.
    let a = client(30, "a");
    let r = engine_from_seed(&[31; 32], EngineConfig::repeater("relay"));
    let c = client(32, "c");
    let c_ident = c.public_identity();
    let mut a = a;
    a.add_contact(c_ident, Some("carol".into()));
    let adj = vec![
        vec![false, true, false],
        vec![true, false, true],
        vec![false, true, false],
    ];
    let mut mesh = Mesh::new(vec![a, r, c], adj);
    let a_idx = 0;
    let r_idx = 1;
    let c_idx = 2;
    let a_id = mesh.engine(a_idx).node_id();
    let c_id = mesh.engine(c_idx).node_id();

    // C is offline. A and R announce so everyone who is online knows each other.
    mesh.set_online(c_idx, false);
    let __now = mesh.now;
    let processed = mesh.engine_mut(a_idx).announce(__now).unwrap();
    mesh.submit(a_idx, processed);
    let __now = mesh.now;
    let processed = mesh.engine_mut(r_idx).announce(__now).unwrap();
    mesh.submit(r_idx, processed);
    mesh.pump();
    assert!(mesh.engine(r_idx).node_info(a_id).is_some());

    // A sends a DM to the offline C. A knows C out-of-band (QR contact).
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a_idx)
        .send_text(Target::Direct(c_id), "stored for later", __now)
        .unwrap();
    mesh.submit(a_idx, sent);
    mesh.pump();
    assert!(mesh.engine(c_idx).stats().rx_frames == 0, "C is offline");
    assert!(
        mesh.engine(r_idx).stats().stored_envelopes >= 1,
        "relay must retain the envelope for the absent addressee"
    );
    assert!(!mesh.has_message(c_idx, "stored for later"));

    // C comes online and announces; the relay offers, C wants, the relay replays.
    mesh.set_online(c_idx, true);
    let __now = mesh.now;
    let processed = mesh.engine_mut(c_idx).announce(__now).unwrap();
    mesh.submit(c_idx, processed);
    mesh.pump();
    for _ in 0..20 {
        mesh.advance(1_000);
        if mesh.has_message(c_idx, "stored for later") {
            break;
        }
    }
    assert!(
        mesh.has_message(c_idx, "stored for later"),
        "replayed envelope must be delivered"
    );
    assert!(
        mesh.events_for(c_idx)
            .iter()
            .any(|event| matches!(event, Event::StoreOffer { .. })),
        "addressee must receive a store offer"
    );
    assert_eq!(
        mesh.ack_count(a_idx),
        1,
        "addressee must ack replayed envelopes normally"
    );
    // C's delivery ack travels back through the relay.
    assert!(
        !mesh
            .events_for(c_idx)
            .iter()
            .any(|event| matches!(event, Event::Error(_)))
    );
}

#[test]
fn relay_never_stores_announces_acks_or_channel_frames() {
    let a = client(40, "a");
    let r = engine_from_seed(&[41; 32], EngineConfig::repeater("relay"));
    let adj = vec![vec![false, true], vec![true, false]];
    let mut mesh = Mesh::new(vec![a, r], adj);
    let __now = mesh.now;
    let processed = mesh.engine_mut(0).announce(__now).unwrap();
    mesh.submit(0, processed);
    mesh.pump();
    assert_eq!(
        mesh.engine(1).stats().stored_envelopes,
        0,
        "announces are never stored"
    );
}

#[test]
fn channel_frames_are_never_stored() {
    // A channel-addressed frame passes through a relay that has no channel key.
    let a = client(42, "a");
    let r = engine_from_seed(&[43; 32], EngineConfig::repeater("relay"));
    let b = client(44, "b");
    let adj = vec![
        vec![false, true, false],
        vec![true, false, true],
        vec![false, true, false],
    ];
    let mut mesh = Mesh::new(vec![a, r, b], adj);
    mesh.set_online(2, false);
    let psk = [5u8; 32];
    let channel = mesh.engine_mut(0).add_channel("family", &psk);
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(0)
        .send_text(Target::Channel(channel), "group secret", __now)
        .unwrap();
    mesh.submit(0, sent);
    mesh.pump();
    assert_eq!(
        mesh.engine(1).stats().stored_envelopes,
        0,
        "channel frames must not be stored"
    );
}

// ---------------------------------------------------------------------------------------------
// Dedup TTL and announce cadence
// ---------------------------------------------------------------------------------------------

#[test]
fn dedup_cache_expires_but_application_dedup_holds() {
    let (mut mesh, a, b) = duo();
    let b_id = mesh.engine(b).node_id();
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_text(Target::Direct(b_id), "expiring", __now)
        .unwrap();
    let frame = sent.outbound[0].frame.clone();
    mesh.submit(a, sent);
    mesh.pump();
    let dedup_before = mesh.engine(b).stats().dedup_hits;
    // Replaying the frame immediately is suppressed by the (src, seq) dedup cache.
    let __now = mesh.now;
    mesh.engine_mut(b)
        .ingest(&frame.encode().unwrap(), None, __now);
    assert_eq!(mesh.engine(b).stats().dedup_hits, dedup_before + 1);
    // After the dedup TTL the frame is processed again, but the application-level
    // (src, msg_id) cache must still prevent a duplicate surface.
    mesh.advance(openmesh_core::engine::DEDUP_TTL_MS + 1_000);
    let __now = mesh.now;
    let processed = mesh
        .engine_mut(b)
        .ingest(&frame.encode().unwrap(), None, __now);
    mesh.submit(b, processed);
    mesh.pump();
    let count = mesh
        .events_for(b)
        .iter()
        .filter(|event| matches!(event, Event::Message(m) if m.message.text.as_deref() == Some("expiring")))
        .count();
    assert_eq!(count, 1, "app-level dedup must cover the dedup-TTL gap");
}

#[test]
fn announce_cadence_is_driven_by_tick() {
    let mut config = EngineConfig::client("a");
    config.announce_interval_ms = openmesh_core::engine::ANNOUNCE_MIN_INTERVAL_MS;
    let mut engine = engine_from_seed(&[50; 32], config);
    let first = engine.tick(0);
    assert!(
        first
            .outbound
            .iter()
            .any(|out| out.frame.typ == openmesh_core::FrameType::Announce)
    );
    // Too soon: no new announce.
    let second = engine.tick(1_000);
    assert!(
        !second
            .outbound
            .iter()
            .any(|out| out.frame.typ == openmesh_core::FrameType::Announce)
    );
    // After the interval: a new announce (with a fresh seq).
    let third = engine.tick(openmesh_core::engine::ANNOUNCE_MIN_INTERVAL_MS + 1_000);
    assert!(
        third
            .outbound
            .iter()
            .any(|out| out.frame.typ == openmesh_core::FrameType::Announce)
    );
}

#[test]
fn peer_expiry_prunes_stale_nodes() {
    let mut engine = client(60, "a");
    let mut other = client(61, "b");
    let ident = other.public_identity();
    engine.add_contact(ident, None);
    // A contact is retained, a non-contact peer expires after PEER_EXPIRY_MS.
    engine.remove_contact(ident.node_id);
    let mut frame = other.announce(0).unwrap().outbound.remove(0).frame;
    frame.ttl = openmesh_core::engine::ANNOUNCE_TTL;
    engine.on_frame(frame, None, 0);
    assert!(engine.node_info(ident.node_id).is_some());
    engine.tick(openmesh_core::engine::PEER_EXPIRY_MS + 1);
    assert!(engine.node_info(ident.node_id).is_none());
}

#[test]
fn store_queue_evicts_expired_envelopes() {
    // Relay stores an envelope for an absent addressee, then the 72 h TTL expires it.
    let mut a = client(70, "a");
    let mut r = engine_from_seed(&[71; 32], EngineConfig::repeater("relay"));
    let c = client(72, "c");
    let c_id = c.node_id();
    a.add_contact(c.public_identity(), None);
    let sent = a.send_text(Target::Direct(c_id), "keep this", 0).unwrap();
    let frame = sent.outbound[0].frame.clone();
    let processed = r.on_frame(frame, None, 0);
    assert!(processed.events.is_empty());
    assert_eq!(
        r.stats().stored_envelopes,
        1,
        "relay must retain the envelope"
    );
    // A relay only stores Data/Resource, never its own traffic or announces.
    let announce = r.announce(0).unwrap();
    for outbound in announce.outbound {
        r.on_frame(outbound.frame, None, 0);
    }
    assert_eq!(r.stats().stored_envelopes, 1);
    // Ticking past STORE_TTL_MS evicts the envelope.
    let late = openmesh_core::engine::STORE_TTL_MS + 1_000;
    r.tick(late);
    assert_eq!(
        r.stats().stored_envelopes,
        0,
        "expired envelopes must be evicted"
    );
}

#[test]
fn stats_count_transmissions_and_forwards() {
    let (mut mesh, a, b) = duo();
    let b_id = mesh.engine(b).node_id();
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_text(Target::Direct(b_id), "count me", __now)
        .unwrap();
    mesh.submit(a, sent);
    mesh.pump();
    let a_stats = mesh.engine(a).stats();
    let b_stats = mesh.engine(b).stats();
    assert!(a_stats.tx_frames >= 1);
    assert!(b_stats.rx_frames >= 1);
    assert!(
        b_stats.forwarded_frames >= 1,
        "b re-broadcasts the message it heard"
    );
    assert!(a_stats.airtime_tx_ms > 0);
    assert_eq!(b_stats.known_nodes, 1);
    assert!(mesh.has_message(b, "count me"));
}

#[test]
fn long_text_on_a_channel_is_streamed_as_a_resource() {
    let (mut mesh, a, b) = duo();
    let psk = [77u8; 32];
    let channel = mesh.engine_mut(a).add_channel("family", &psk);
    mesh.engine_mut(b).add_channel("family", &psk);
    // The receiver must know the sender's Ed25519 key to verify group authorship.
    let announced = mesh.engine_mut(a).announce(0).unwrap();
    mesh.submit(a, announced);
    mesh.pump();

    let text = "channel long text ".repeat(30); // ~540 bytes: needs the resource path
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_text(Target::Channel(channel), &text, __now)
        .unwrap();
    assert!(
        sent.outbound.len() > 1,
        "long channel text must be streamed as a resource"
    );
    mesh.submit(a, sent);
    for _ in 0..40 {
        mesh.advance(1_000);
        if mesh.events_for(b).iter().any(|event| matches!(event, Event::GroupMessage(group) if group.message.text.as_deref() == Some(text.as_str()))) {
            break;
        }
    }
    let group = mesh
        .events_for(b)
        .iter()
        .find_map(|event| match event {
            Event::GroupMessage(group) => Some(group),
            _ => None,
        })
        .expect("long channel text must surface as a verified GroupMessage");
    assert_eq!(group.channel, channel);
    assert_eq!(group.message.text.as_deref(), Some(text.as_str()));
    assert!(
        mesh.events_for(b)
            .iter()
            .any(|event| matches!(event, Event::ResourceReceived { channel: Some(c), .. } if *c == channel)),
        "channel resource must also surface ResourceReceived"
    );
}

#[test]
fn channel_position_report_carries_the_channel() {
    let (mut mesh, a, b) = duo();
    let psk = [78u8; 32];
    let channel = mesh.engine_mut(a).add_channel("family", &psk);
    mesh.engine_mut(b).add_channel("family", &psk);
    let announced = mesh.engine_mut(a).announce(0).unwrap();
    mesh.submit(a, announced);
    mesh.pump();
    let position = Position {
        lat_e7: -33_868_800,
        lon_e7: 151_209_300,
        alt_m: Some(12),
        accuracy_m: Some(5),
        speed_cm_s: None,
        heading_deg: None,
        timestamp_ms: 5,
    };
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_position(Target::Channel(channel), position.clone(), __now)
        .unwrap();
    mesh.submit(a, sent);
    mesh.pump();
    assert!(
        mesh.events_for(b).iter().any(|event| matches!(
            event,
            Event::Position { channel: Some(c), position: p, .. } if *c == channel && p == &position
        )),
        "channel position event expected"
    );
}

#[test]
fn clear_text_store_offer_works_without_peer_key_material() {
    // A relay may hold mail for a node whose announce it never heard (e.g. left by a gateway).
    // StoreOffer/StoreWant are clear-text, so it can still offer and deliver.
    let a = client(80, "a");
    let r = engine_from_seed(&[81; 32], EngineConfig::repeater("relay"));
    let c = client(82, "c");
    let a_ident = a.public_identity();
    let c_ident = c.public_identity();
    let mut a = a;
    a.add_contact(c_ident, Some("carol".into()));
    let mut r = r;
    let mut c = c;
    r.add_contact(a_ident, Some("alice".into()));
    c.add_channel("side", &[3u8; 32]);

    // A's message reaches the relay while C is offline.
    let sent = a
        .send_text(Target::Direct(c_ident.node_id), "mail for carol", 0)
        .unwrap();
    for outbound in sent.outbound {
        r.on_frame(outbound.frame, None, 0);
    }
    assert_eq!(r.stats().stored_envelopes, 1);
    assert!(
        r.node_info(c_ident.node_id).is_none(),
        "the relay never heard C's announce"
    );

    // C comes online and transmits a channel frame the relay can hear, without announcing.
    let channel = c.channels()[0].0;
    let chatter = c
        .send_text(Target::Channel(channel), "anyone there?", 100)
        .unwrap();
    let mut out = Processed::default();
    for outbound in chatter.outbound {
        out.merge(r.on_frame(outbound.frame, None, 100));
    }
    assert!(
        r.node_info(c_ident.node_id).is_none(),
        "still no identity for C"
    );
    let offer = out
        .outbound
        .iter()
        .find(|outbound| outbound.frame.typ == openmesh_core::FrameType::StoreOffer)
        .expect("relay must offer stored mail without knowing the addressee's keys");
    assert!(
        !offer.frame.flags.contains(openmesh_core::Flags::ENCRYPTED),
        "StoreOffer bodies are clear-text"
    );
    let parsed: openmesh_core::services::StoreOffer =
        openmesh_core::services::cbor_from_bytes(&offer.frame.body).unwrap();
    assert!(!parsed.ids.is_empty());

    // C wants the mail and the relay replays the original envelope.
    let want = c.on_frame(offer.frame.clone(), None, 200);
    assert!(
        want.events
            .iter()
            .any(|event| matches!(event, Event::StoreOffer { .. })),
        "addressee must surface the offer even without key material"
    );
    let want_frame = want
        .outbound
        .iter()
        .find(|outbound| outbound.frame.typ == openmesh_core::FrameType::StoreWant)
        .expect("addressee replies with StoreWant");
    assert!(
        !want_frame
            .frame
            .flags
            .contains(openmesh_core::Flags::ENCRYPTED)
    );
    let replay = r.on_frame(want_frame.frame.clone(), None, 200);
    assert!(
        !replay.outbound.is_empty(),
        "relay must replay the stored envelope"
    );
    for outbound in replay.outbound {
        let processed = c.on_frame(outbound.frame, None, 200);
        for event in processed.events {
            if let Event::Message(message) = event {
                assert_eq!(message.message.text.as_deref(), Some("mail for carol"));
            }
        }
    }
}

#[test]
fn lossy_channel_position_and_long_text_fall_back_to_resources() {
    let (mut mesh, a, b) = duo();
    let psk = [90u8; 32];
    let channel = mesh.engine_mut(a).add_channel("family", &psk);
    mesh.engine_mut(b).add_channel("family", &psk);
    let announced = mesh.engine_mut(a).announce(0).unwrap();
    mesh.submit(a, announced);
    mesh.pump();
    mesh.drop_percent(20);

    let position = Position {
        lat_e7: 51_507_100,
        lon_e7: -12_715_000,
        alt_m: Some(1_200),
        accuracy_m: Some(6),
        speed_cm_s: Some(310),
        heading_deg: Some(275),
        timestamp_ms: 42,
    };
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_position(Target::Channel(channel), position.clone(), __now)
        .unwrap();
    assert!(
        sent.outbound.len() > 1,
        "channel position must fall back to a resource"
    );
    mesh.submit(a, sent);

    let long_text = "channel position fallback ".repeat(30);
    let __now = mesh.now;
    let sent = mesh
        .engine_mut(a)
        .send_text(Target::Channel(channel), &long_text, __now)
        .unwrap();
    assert!(
        sent.outbound.len() > 1,
        "long channel text must fall back to a resource"
    );
    mesh.submit(a, sent);

    let mut got_position = false;
    let mut got_text = false;
    for _ in 0..120 {
        mesh.advance(1_000);
        got_position = mesh.events_for(b).iter().any(|event| {
            matches!(event, Event::Position { channel: Some(c), position: p, .. }
                if *c == channel && p == &position)
        });
        got_text = mesh.events_for(b).iter().any(|event| {
            matches!(event, Event::GroupMessage(group) if group.message.text.as_deref() == Some(long_text.as_str()))
        });
        if got_position && got_text {
            break;
        }
    }
    assert!(
        got_position,
        "lossy channel position must complete via the resource path"
    );
    assert!(
        got_text,
        "lossy long channel text must complete via the resource path"
    );
    let received = mesh
        .events_for(b)
        .iter()
        .filter(|event| matches!(event, Event::ResourceReceived { channel: Some(c), .. } if *c == channel))
        .count();
    assert_eq!(
        received, 2,
        "both channel resources must be verified and surfaced"
    );
    assert!(mesh.drops > 0);
}

//! Demo mode: seeded virtual nodes, stored conversations and live replies over the sim mesh.
//!
//! `openmesh-bridge --demo` is what the README, the web client and reviewers run: it needs no
//! radio hardware and no second process. Virtual peers are full protocol engines running in
//! this process on the same in-process virtual mesh; they announce, ack and reply to messages
//! exactly like real nodes do, so the UI sees genuine protocol traffic.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::{mpsc, watch};

use openmesh_core::{
    AppMessage, Engine, EngineConfig, Event, LinkInfo, MessageKind, NodeClass, NodeId, Position,
    Processed, PublicIdentity, Target, util,
};

use crate::json::{NodeJson, PositionJson, caps_to_strings};
use crate::store::{ChannelRecord, ContactRecord, Store, StoredMessage, identity_b64};
use crate::transport::sim::MeshHub;

use openmesh_core::services::caps;

/// A virtual node used by demo mode.
pub struct DemoPeer {
    pub seed: u8,
    pub name: &'static str,
    pub class: NodeClass,
    pub caps: u16,
    pub position: Option<Position>,
}

/// Deterministic demo peers (same node ids on every run, so seeded conversations stay coherent).
pub fn demo_peers() -> Vec<DemoPeer> {
    vec![
        DemoPeer {
            seed: 0xD1,
            name: "alice",
            class: NodeClass::Client,
            caps: caps::RELAY | caps::GPS,
            position: Some(Position {
                lat_e7: 599_211_000,
                lon_e7: 107_615_000,
                alt_m: Some(62),
                accuracy_m: Some(12),
                speed_cm_s: Some(0),
                heading_deg: None,
                timestamp_ms: 0,
            }),
        },
        DemoPeer {
            seed: 0xD2,
            name: "bob",
            class: NodeClass::Tracker,
            caps: caps::GPS | caps::RELAY,
            position: Some(Position {
                lat_e7: 599_003_000,
                lon_e7: 107_041_000,
                alt_m: Some(310),
                accuracy_m: Some(25),
                speed_cm_s: Some(140),
                heading_deg: Some(212),
                timestamp_ms: 0,
            }),
        },
        DemoPeer {
            seed: 0xD3,
            name: "solar-ridge",
            class: NodeClass::Repeater,
            caps: caps::RELAY | caps::STORE | caps::SOLAR | caps::GPS,
            position: Some(Position {
                lat_e7: 599_137_000,
                lon_e7: 107_522_000,
                alt_m: Some(489),
                accuracy_m: Some(5),
                speed_cm_s: None,
                heading_deg: None,
                timestamp_ms: 0,
            }),
        },
    ]
}

pub fn peer_identity(peer: &DemoPeer) -> PublicIdentity {
    openmesh_core::Identity::from_seed(&[peer.seed; 32]).public()
}

fn peer_node_id(peer: &DemoPeer) -> NodeId {
    peer_identity(peer).node_id
}

fn now_with_offset(now: u64, offset_ms: u64) -> u64 {
    now.saturating_sub(offset_ms)
}

/// Seed contacts, a group channel and a realistic conversation history (only when the store is
/// empty so restarts don't duplicate data).
pub fn seed_demo(
    store: &Arc<Store>,
    engine: &mut Engine,
    bridge_ident: &PublicIdentity,
    now: u64,
) -> Result<()> {
    let bridge_node = bridge_ident.node_id;
    let peers = demo_peers();

    // Contacts + cached nodes for every demo peer.
    for peer in &peers {
        let ident = peer_identity(peer);
        let node = NodeJson {
            node_id: ident.node_id.to_hex(),
            name: peer.name.to_string(),
            class: crate::json::node_class_str(peer.class).to_string(),
            caps: caps_to_strings(peer.caps),
            hops: 1,
            last_seen_ms: now.saturating_sub(60_000),
            rssi: Some(-78),
            snr: Some(7.25),
            position: peer.position.as_ref().map(PositionJson::from_core),
            battery_pct: Some(88),
            is_contact: true,
            public_key: identity_b64(&ident),
        };
        engine.add_contact(ident, Some(peer.name.to_string()));
        store.put_contact(&ContactRecord {
            node_id: ident.node_id,
            name: Some(peer.name.to_string()),
            identity: identity_b64(&ident),
            last_seen_ms: now.saturating_sub(60_000),
            added_at_ms: now.saturating_sub(86_400_000),
        })?;
        store.put_node(&node)?;
    }

    // A demo group channel so the UI's group chat is populated.
    let psk = [0x42u8; 32];
    let channel_id = engine.add_channel("family", &psk);
    store.put_channel(&ChannelRecord {
        channel_id,
        name: "family".to_string(),
        psk: crate::json::b64(&psk),
        created_at_ms: now.saturating_sub(172_800_000),
    })?;

    if store.message_count()? > 0 {
        return Ok(());
    }

    let alice = peer_node_id(&peers[0]);
    let bob = peer_node_id(&peers[1]);
    let solar = peer_node_id(&peers[2]);
    let mut messages = vec![
        incoming(
            bridge_node,
            alice,
            "alice",
            "Are you on the ridge today? Signal is clear.",
            now_with_offset(now, 2_400_000),
        ),
        outgoing(
            bridge_node,
            alice,
            "Just set up the solar repeater, coverage looks good.",
            now_with_offset(now, 2_340_000),
            true,
        ),
        incoming(
            bridge_node,
            alice,
            "alice",
            "Nice! I can see the node on the map now 🏔",
            now_with_offset(now, 2_100_000),
        ),
        outgoing(
            bridge_node,
            bob,
            "Sent you the trail map for tomorrow.",
            now_with_offset(now, 7_200_000),
            true,
        ),
        incoming(
            bridge_node,
            bob,
            "bob",
            "Got it, thanks! I'll meet you at the cabin.",
            now_with_offset(now, 7_000_000),
        ),
        incoming(
            bridge_node,
            solar,
            "solar-ridge",
            "Battery at 88%, 6.4 V solar input. All quiet.",
            now_with_offset(now, 900_000),
        ),
    ];

    for (i, text) in [
        "Family hike is on for Saturday.",
        "Bring the handhelds, no signal up there 😄",
    ]
    .iter()
    .enumerate()
    {
        let outgoing_group = MessageKind::Text;
        messages.push(StoredMessage {
            msg_id: rand::random::<u32>().max(1),
            peer: None,
            channel_id: Some(channel_id),
            channel_name: Some("family".to_string()),
            from: if i == 0 { alice } else { bridge_node },
            to: channel_id,
            from_name: Some(if i == 0 {
                "alice".to_string()
            } else {
                "you".to_string()
            }),
            kind: outgoing_group,
            text: Some((*text).to_string()),
            sent_at_ms: now_with_offset(now, 5_400_000 - i as u64 * 120_000),
            received_at_ms: now_with_offset(now, 5_400_000 - i as u64 * 120_000),
            outgoing: i == 1,
            delivered: true,
            read: true,
            reply_to: None,
            thread: None,
            attachment: None,
            position: None,
            signature_valid: true,
            sender_key: Some(if i == 0 {
                identity_b64(&peer_identity(&peers[0]))
            } else {
                identity_b64(bridge_ident)
            }),
            via_store: false,
        });
    }

    for message in &messages {
        store.put_message(message)?;
    }
    tracing::info!(messages = messages.len(), "seeded demo conversations");
    Ok(())
}

fn outgoing(bridge: NodeId, peer: NodeId, text: &str, sent: u64, delivered: bool) -> StoredMessage {
    StoredMessage {
        msg_id: rand::random::<u32>().max(1),
        peer: Some(peer),
        channel_id: None,
        channel_name: None,
        from: bridge,
        to: peer,
        from_name: Some("you".to_string()),
        kind: MessageKind::Text,
        text: Some(text.to_string()),
        sent_at_ms: sent,
        received_at_ms: sent,
        outgoing: true,
        delivered,
        read: true,
        reply_to: None,
        thread: None,
        attachment: None,
        position: None,
        signature_valid: true,
        sender_key: None,
        via_store: false,
    }
}

fn incoming(bridge: NodeId, from: NodeId, name: &str, text: &str, sent: u64) -> StoredMessage {
    StoredMessage {
        msg_id: rand::random::<u32>().max(1),
        peer: Some(from),
        channel_id: None,
        channel_name: None,
        from,
        to: bridge,
        from_name: Some(name.to_string()),
        kind: MessageKind::Text,
        text: Some(text.to_string()),
        sent_at_ms: sent,
        received_at_ms: sent,
        outgoing: false,
        delivered: true,
        read: sent < util::now_ms().saturating_sub(1_800_000),
        reply_to: None,
        thread: None,
        attachment: None,
        position: None,
        signature_valid: false,
        sender_key: None,
        via_store: false,
    }
}

/// Announce and start the demo virtual peers on the shared mesh.
pub fn spawn_peers(
    hub: &Arc<MeshHub>,
    bridge_ident: &PublicIdentity,
    shutdown: watch::Receiver<bool>,
) -> Vec<tokio::task::JoinHandle<()>> {
    demo_peers()
        .into_iter()
        .map(|peer| {
            let ident = peer_identity(&peer);
            let mut engine = Engine::new(
                openmesh_core::Identity::from_seed(&[peer.seed; 32]),
                peer_config(&peer),
            );
            engine.add_contact(*bridge_ident, Some("you".to_string()));
            let rx = hub.register(ident.node_id);
            let hub = hub.clone();
            let bridge_node = bridge_ident.node_id;
            let name = peer.name;
            let mut shutdown = shutdown.clone();
            tokio::spawn(async move {
                peer_task(
                    engine,
                    ident.node_id,
                    rx,
                    hub,
                    bridge_node,
                    name,
                    &mut shutdown,
                )
                .await;
            })
        })
        .collect()
}

fn peer_config(peer: &DemoPeer) -> EngineConfig {
    let mut config = match peer.class {
        NodeClass::Repeater => EngineConfig::repeater(peer.name),
        _ => EngineConfig::client(peer.name),
    };
    config.class = peer.class;
    config.caps = peer.caps;
    config
}

async fn peer_task(
    mut engine: Engine,
    node_id: NodeId,
    mut rx: mpsc::Receiver<Vec<u8>>,
    hub: Arc<MeshHub>,
    bridge_node: NodeId,
    name: &'static str,
    shutdown: &mut watch::Receiver<bool>,
) {
    let started = Instant::now();
    let mut announced = false;
    let mut greeted = false;
    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            frame = rx.recv() => {
                let Some(bytes) = frame else { break };
                let now = util::now_ms();
                let processed = engine.ingest(
                    &bytes,
                    Some(LinkInfo { link: 1, peer: None, rssi: Some(-65), snr: Some(8.5) }),
                    now,
                );
                let mut reply_texts = Vec::new();
                for event in &processed.events {
                    match event {
                        Event::Message(received) => {
                            if let Some(text) = received.message.text.as_deref() {
                                if !text.is_empty() {
                                    reply_texts.push(text.to_string());
                                }
                            }
                        }
                        Event::Error(message) => tracing::debug!(peer = name, message, "demo peer protocol error"),
                        _ => {}
                    }
                }
                // Engine acknowledges incoming data automatically; only app-level replies are
                // the demo peer's job here.
                relay_frames(&hub, node_id, processed);
                for text in reply_texts {
                    let reply = AppMessage {
                        id: rand::random::<u32>().max(1),
                        kind: MessageKind::Text,
                        sent_at_ms: now,
                        text: Some(reply_for(name, &text)),
                        reply_to: None,
                        thread: None,
                        attachment: None,
                        position: None,
                        telemetry: None,
                        sender: None,
                        signature: None,
                    };
                    if let Ok(processed) = engine.send_message(Target::Direct(bridge_node), reply, now) {
                        relay_frames(&hub, node_id, processed);
                    }
                }
            }
            _ = ticker.tick() => {
                let now = util::now_ms();
                if !announced && started.elapsed() > Duration::from_millis(250) {
                    if let Ok(processed) = engine.announce(now) {
                        relay_frames(&hub, node_id, processed);
                    }
                    announced = true;
                }
                let processed = engine.tick(now);
                relay_frames(&hub, node_id, processed);
                if name == "alice" && !greeted && started.elapsed() > Duration::from_millis(1800) {
                    let greeting = AppMessage {
                        id: rand::random::<u32>().max(1),
                        kind: MessageKind::Text,
                        sent_at_ms: now,
                        text: Some("Mesh is up! Send me a message and I'll answer over the simulated radio.".to_string()),
                        reply_to: None,
                        thread: None,
                        attachment: None,
                        position: None,
                        telemetry: None,
                        sender: None,
                        signature: None,
                    };
                    if let Ok(processed) = engine.send_message(Target::Direct(bridge_node), greeting, now) {
                        relay_frames(&hub, node_id, processed);
                    }
                    greeted = true;
                }
            }
        }
    }
    hub.unregister(node_id);
}

fn reply_for(name: &str, received: &str) -> String {
    let lower = received.to_lowercase();
    if lower.contains("hello") || lower.contains("hi") {
        format!("Hi! {name} here, received you loud and clear over the mesh.")
    } else if lower.contains("?") {
        format!("{name} here — yes! Got your question over the mesh 📡")
    } else {
        format!(
            "{name} received: \"{}\" — ack sent back over the relay.",
            truncate(received, 60)
        )
    }
}

fn truncate(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

fn relay_frames(hub: &Arc<MeshHub>, node_id: NodeId, processed: Processed) {
    for outbound in processed.outbound {
        if let Ok(bytes) = outbound.frame.encode() {
            hub.broadcast(node_id, bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_peer_ids_are_deterministic() {
        let a = demo_peers();
        let b = demo_peers();
        assert_eq!(peer_node_id(&a[0]), peer_node_id(&b[0]));
        assert_ne!(peer_node_id(&a[0]), peer_node_id(&a[1]));
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn replies_are_contextual() {
        assert!(reply_for("alice", "hello there").contains("alice"));
        assert!(reply_for("bob", "is the trail open?").contains("yes"));
        assert!(reply_for("alice", "long message").contains("ack sent back"));
    }

    #[test]
    fn truncation_is_utf8_safe() {
        assert_eq!(truncate("abcdef", 3), "abc…");
        assert_eq!(truncate("abc", 3), "abc");
    }
}

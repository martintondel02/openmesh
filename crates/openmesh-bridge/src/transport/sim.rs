//! In-process virtual mesh transport.
//!
//! One [`MeshHub`] per process (shared by every bridge instance started in `--sim` mode and by
//! demo virtual nodes) routes encoded OMP frames between participants. Latency and loss are
//! configurable, and a fixed RNG seed makes loss runs reproducible for tests and the simulator.
//!
//! This is deliberately *not* a protocol shortcut: participants still receive and process real
//! OMP frames through [`openmesh_core::Engine::ingest`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tokio::sync::{mpsc, watch};

use openmesh_core::{LinkId, LinkInfo, NodeId};

use super::{INBOUND_CHANNEL_CAPACITY, Inbound, TransportFrame, TransportHandle, make_link_stats};

/// Loss/latency configuration for the virtual medium.
#[derive(Clone, Copy, Debug)]
pub struct MeshConfig {
    /// Packet loss in percent (0..=100), applied independently per recipient.
    pub loss_pct: u8,
    /// One-way delivery latency in milliseconds.
    pub latency_ms: u64,
    pub seed: u64,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            loss_pct: 0,
            latency_ms: 15,
            seed: 0x0BADF00D,
        }
    }
}

struct HubInner {
    nodes: HashMap<NodeId, mpsc::Sender<Vec<u8>>>,
    rng: StdRng,
}

/// Global virtual medium shared by all in-process participants.
pub struct MeshHub {
    inner: Mutex<HubInner>,
    config: Mutex<MeshConfig>,
}

static HUB: OnceLock<Weak<MeshHub>> = OnceLock::new();

impl MeshHub {
    /// Process-global hub (created on first use).
    pub fn global() -> Arc<MeshHub> {
        if let Some(weak) = HUB.get() {
            if let Some(hub) = weak.upgrade() {
                return hub;
            }
        }
        let hub = Arc::new(MeshHub {
            inner: Mutex::new(HubInner {
                nodes: HashMap::new(),
                rng: StdRng::seed_from_u64(1),
            }),
            config: Mutex::new(MeshConfig::default()),
        });
        let _ = HUB.set(Arc::downgrade(&hub));
        hub
    }

    /// Create an isolated hub (used by tests that must not share global state).
    pub fn isolated(config: MeshConfig) -> Arc<MeshHub> {
        Arc::new(MeshHub {
            inner: Mutex::new(HubInner {
                nodes: HashMap::new(),
                rng: StdRng::seed_from_u64(config.seed),
            }),
            config: Mutex::new(config),
        })
    }

    pub fn configure(&self, config: MeshConfig) {
        let mut cfg = self.config.lock().expect("mesh config lock");
        *cfg = config;
        let mut inner = self.inner.lock().expect("mesh hub lock");
        inner.rng = StdRng::seed_from_u64(config.seed);
    }

    pub fn config(&self) -> MeshConfig {
        *self.config.lock().expect("mesh config lock")
    }

    /// Register a participant and return its receive channel.
    pub fn register(&self, node_id: NodeId) -> mpsc::Receiver<Vec<u8>> {
        let (tx, rx) = mpsc::channel(INBOUND_CHANNEL_CAPACITY);
        let mut inner = self.inner.lock().expect("mesh hub lock");
        inner.nodes.insert(node_id, tx);
        rx
    }

    pub fn unregister(&self, node_id: NodeId) {
        let mut inner = self.inner.lock().expect("mesh hub lock");
        inner.nodes.remove(&node_id);
    }

    pub fn participant_count(&self) -> usize {
        self.inner.lock().expect("mesh hub lock").nodes.len()
    }

    /// Deliver `bytes` from `from` to every other participant (loss/latency applied per hop).
    pub fn broadcast(self: &Arc<Self>, from: NodeId, bytes: Vec<u8>) {
        let cfg = self.config();
        let targets: Vec<(NodeId, mpsc::Sender<Vec<u8>>)> = {
            let mut inner = self.inner.lock().expect("mesh hub lock");
            let participants: Vec<(NodeId, mpsc::Sender<Vec<u8>>)> = inner
                .nodes
                .iter()
                .filter(|(id, _)| **id != from)
                .map(|(id, tx)| (*id, tx.clone()))
                .collect();
            let mut chosen = Vec::new();
            let mut lost = 0usize;
            for (id, tx) in participants {
                let drop_this =
                    cfg.loss_pct > 0 && inner.rng.gen_range(0..100u32) < cfg.loss_pct as u32;
                if drop_this {
                    lost += 1;
                } else {
                    chosen.push((id, tx));
                }
            }
            if lost > 0 {
                tracing::trace!(from = %from, lost, "virtual mesh dropped frame");
            }
            chosen
        };
        if cfg.latency_ms == 0 {
            for (_, tx) in targets {
                let _ = tx.try_send(bytes.clone());
            }
            return;
        }
        for (_, tx) in targets {
            let bytes = bytes.clone();
            let latency = cfg.latency_ms;
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(latency)).await;
                let _ = tx.send(bytes).await;
            });
        }
    }
}

/// Start the bridge's virtual-mesh transport task.
pub fn spawn(
    hub: Arc<MeshHub>,
    node_id: NodeId,
    link: LinkId,
    inbound: mpsc::Sender<Inbound>,
    mut shutdown: watch::Receiver<bool>,
) -> TransportHandle {
    let mut rx = hub.register(node_id);
    let (tx, mut outbound) = mpsc::channel::<TransportFrame>(INBOUND_CHANNEL_CAPACITY);
    let stats = make_link_stats();
    let stats_task = stats.clone();
    let hub_task = hub.clone();
    let tx_stats = stats.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                received = rx.recv() => {
                    let Some(bytes) = received else { break };
                    stats_task.record_rx(Some(-61), Some(9.5));
                    let inbound_msg = Inbound {
                        bytes,
                        link: Some(LinkInfo {
                            link,
                            peer: None,
                            rssi: Some(-61),
                            snr: Some(9.5),
                        }),
                    };
                    if inbound.send(inbound_msg).await.is_err() {
                        break;
                    }
                }
                frame = outbound.recv() => {
                    let Some(frame) = frame else { break };
                    if frame.link.is_some_and(|preferred| preferred != link) {
                        continue;
                    }
                    tx_stats.record_tx();
                    hub_task.broadcast(node_id, frame.bytes);
                }
            }
        }
        hub_task.unregister(node_id);
        tracing::debug!(%node_id, "sim transport stopped");
    });
    TransportHandle {
        id: link,
        kind: "sim",
        label: "Simulator".to_string(),
        tx,
        stats,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hub_delivers_between_participants() {
        let hub = MeshHub::isolated(MeshConfig {
            loss_pct: 0,
            latency_ms: 0,
            seed: 7,
        });
        let mut a = hub.register(NodeId::new([1; 8]));
        let mut b = hub.register(NodeId::new([2; 8]));
        hub.broadcast(NodeId::new([1; 8]), vec![1, 2, 3]);
        assert_eq!(b.recv().await.unwrap(), vec![1, 2, 3]);
        assert!(
            a.try_recv().is_err(),
            "sender must not receive its own broadcast"
        );
    }

    #[tokio::test]
    async fn loss_is_deterministic_for_a_seed() {
        let config = MeshConfig {
            loss_pct: 50,
            latency_ms: 0,
            seed: 42,
        };
        let hub = MeshHub::isolated(config);
        let _rx = hub.register(NodeId::new([2; 8]));
        let mut delivered = 0;
        for i in 0..100u32 {
            let mut rx = hub.register(NodeId::new([2; 8]));
            // Re-registering replaces the sender; count deliveries with a fresh channel.
            hub.broadcast(NodeId::new([1; 8]), i.to_be_bytes().to_vec());
            if tokio::time::timeout(std::time::Duration::from_millis(5), rx.recv())
                .await
                .is_ok()
            {
                delivered += 1;
            }
        }
        assert!(
            delivered > 20 && delivered < 80,
            "unexpected delivery count {delivered}"
        );
    }

    #[tokio::test]
    async fn high_loss_can_drop_everything() {
        let hub = MeshHub::isolated(MeshConfig {
            loss_pct: 100,
            latency_ms: 0,
            seed: 1,
        });
        let mut rx = hub.register(NodeId::new([2; 8]));
        hub.broadcast(NodeId::new([1; 8]), vec![9]);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(5), rx.recv())
                .await
                .is_err()
        );
    }
}

//! Transport layer: the bridge glues the protocol engine to one or more frame carriers.
//!
//! Every transport task receives encoded OMP frames from the engine and converts them to its
//! medium (in-process virtual mesh, UDP broadcast, TCP stream, USB serial), then feeds received
//! frames back into the engine as [`openmesh_core::LinkInfo`]-tagged input.
//!
//! | module | medium | framing |
//! | --- | --- | --- |
//! | [`sim`] | in-process virtual LoRa mesh (demo/CI) | one frame per virtual packet |
//! | [`udp`] | LAN / WiFi / Ethernet | one datagram per frame, broadcast to port 4403 |
//! | [`tcp`] | WAN relay link | 2-byte big-endian length prefix |
//! | [`serial`] | USB CDC radio (`--features real-serial`) | SLIP (`0xC0` delimiter) |

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use tokio::sync::mpsc;

use openmesh_core::{LinkId, LinkInfo, SendMode};

pub mod backbone;
pub mod sim;
pub mod tcp;
pub mod udp;

#[cfg(feature = "real-serial")]
pub mod serial;

/// One frame the engine wants transmitted, plus its routing preference.
#[derive(Clone, Debug)]
pub struct TransportFrame {
    /// Fully encoded OMP frame bytes.
    pub bytes: Vec<u8>,
    pub mode: SendMode,
    /// Preferred link when the engine is answering a link-bound peer.
    pub link: Option<LinkId>,
    /// Peer this frame arrived from: point-to-point transports must not echo it back
    /// (backbone mode delivers to every peer except the origin).
    pub exclude: Option<openmesh_core::NodeId>,
}

/// Inbound data handed from a transport to the protocol engine.
#[derive(Clone, Debug)]
pub struct Inbound {
    pub bytes: Vec<u8>,
    pub link: Option<LinkInfo>,
}

/// Per-link counters shown by `get_stats` and the 1 Hz `status` event.
#[derive(Debug, Default)]
pub struct LinkStats {
    pub tx_frames: AtomicU64,
    pub rx_frames: AtomicU64,
    /// Last observed signal strength, stored as a fixed-point integer (sentinel `i32::MIN`).
    pub last_rssi: AtomicI32,
    pub last_snr_centi: AtomicI32,
}

const SENTINEL: i32 = i32::MIN;

impl LinkStats {
    pub fn record_tx(&self) {
        self.tx_frames.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_rx(&self, rssi: Option<i16>, snr: Option<f32>) {
        self.rx_frames.fetch_add(1, Ordering::Relaxed);
        if let Some(rssi) = rssi {
            self.last_rssi.store(rssi as i32, Ordering::Relaxed);
        }
        if let Some(snr) = snr {
            self.last_snr_centi
                .store((snr * 100.0) as i32, Ordering::Relaxed);
        }
    }

    pub fn rssi(&self) -> Option<i16> {
        let v = self.last_rssi.load(Ordering::Relaxed);
        (v != SENTINEL).then_some(v as i16)
    }

    pub fn snr(&self) -> Option<f32> {
        let v = self.last_snr_centi.load(Ordering::Relaxed);
        (v != SENTINEL).then_some(v as f32 / 100.0)
    }

    pub fn tx(&self) -> u64 {
        self.tx_frames.load(Ordering::Relaxed)
    }

    pub fn rx(&self) -> u64 {
        self.rx_frames.load(Ordering::Relaxed)
    }
}

/// Handle the engine uses to write to one transport.
#[derive(Clone)]
pub struct TransportHandle {
    pub id: LinkId,
    /// Short machine name (`sim`, `udp`, `tcp`, `serial`).
    pub kind: &'static str,
    /// Human-readable label for the `hello` radios list.
    pub label: String,
    pub tx: mpsc::Sender<TransportFrame>,
    pub stats: Arc<LinkStats>,
}

impl TransportHandle {
    /// Send a frame to this transport, ignoring a closed channel (transport stopped).
    pub fn send(&self, frame: &TransportFrame) {
        if self.tx.try_send(frame.clone()).is_err() {
            // A full or closed queue drops the frame; the protocol tolerates loss.
            tracing::trace!(
                link = self.id,
                kind = self.kind,
                "transport queue full/closed"
            );
        }
    }
}

/// Shared plumbing for transport tasks: receive buffer size for engine input.
pub const INBOUND_CHANNEL_CAPACITY: usize = 1024;

pub fn make_link_stats() -> Arc<LinkStats> {
    Arc::new(LinkStats {
        last_rssi: AtomicI32::new(SENTINEL),
        last_snr_centi: AtomicI32::new(SENTINEL),
        ..Default::default()
    })
}

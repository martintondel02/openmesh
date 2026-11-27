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
use std::sync::atomic::{AtomicI32, AtomicU8, AtomicU32, AtomicU64, Ordering};

use tokio::sync::mpsc;

use openmesh_core::{LinkId, LinkInfo, SendMode, util};

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

/// Link state of a radio-backed transport (`status.radio.state` / `hello.radios[].state`).
///
/// `Stopped` is reported to clients as `"disconnected"`: the transport thread has exited and the
/// radio will not come back without a restart. Non-radio transports (sim/UDP/TCP/backbone) keep
/// the default `Connected` and are reported as `"virtual"` by the API snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RadioState {
    /// Port opened, protocol handshake not finished yet.
    Connecting,
    /// Handshake finished (companion or raw mode); frames can flow.
    Connected,
    /// Link lost (EOF/read error); reopening with backoff.
    Reconnecting,
    /// Transport thread stopped (shutdown or fatal error).
    Stopped,
}

impl RadioState {
    /// Parse the atomic representation (unknown values fall back to `Stopped`).
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Connecting,
            1 => Self::Connected,
            2 => Self::Reconnecting,
            _ => Self::Stopped,
        }
    }

    /// Stable API string used by `status.radio.state` and `hello.radios[].state`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Reconnecting => "reconnecting",
            Self::Stopped => "disconnected",
        }
    }
}

/// Per-link counters and radio link state shown by `get_stats` and the 1 Hz `status` event.
#[derive(Debug)]
pub struct LinkStats {
    pub tx_frames: AtomicU64,
    pub rx_frames: AtomicU64,
    /// Last observed signal strength, stored as a fixed-point integer (sentinel `i32::MIN`).
    pub last_rssi: AtomicI32,
    pub last_snr_centi: AtomicI32,
    /// Epoch milliseconds of the last received frame (`0` = no frame yet).
    pub last_rx_ms: AtomicU64,
    /// Radio link state, see [`RadioState`]; defaults to `Connected` for non-radio transports.
    pub link_state: AtomicU8,
    /// Reconnect attempts since the last successful attach.
    pub reconnect_attempts: AtomicU32,
}

impl Default for LinkStats {
    fn default() -> Self {
        Self {
            tx_frames: AtomicU64::new(0),
            rx_frames: AtomicU64::new(0),
            last_rssi: AtomicI32::new(SENTINEL),
            last_snr_centi: AtomicI32::new(SENTINEL),
            last_rx_ms: AtomicU64::new(0),
            link_state: AtomicU8::new(RadioState::Connected as u8),
            reconnect_attempts: AtomicU32::new(0),
        }
    }
}

const SENTINEL: i32 = i32::MIN;

impl LinkStats {
    pub fn record_tx(&self) {
        self.tx_frames.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_rx(&self, rssi: Option<i16>, snr: Option<f32>) {
        self.rx_frames.fetch_add(1, Ordering::Relaxed);
        self.last_rx_ms.store(util::now_ms(), Ordering::Relaxed);
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

    /// Epoch milliseconds of the last received frame, or `None` when nothing has arrived yet.
    pub fn last_rx_ms(&self) -> Option<u64> {
        let value = self.last_rx_ms.load(Ordering::Relaxed);
        (value != 0).then_some(value)
    }

    /// Current radio link state.
    pub fn state(&self) -> RadioState {
        RadioState::from_u8(self.link_state.load(Ordering::Relaxed))
    }

    /// Update the radio link state (serial worker only; other transports stay `Connected`).
    pub fn set_state(&self, state: RadioState) {
        self.link_state.store(state as u8, Ordering::Relaxed);
    }

    /// Reconnect attempts since the last successful attach.
    pub fn reconnect_attempts(&self) -> u32 {
        self.reconnect_attempts.load(Ordering::Relaxed)
    }

    /// Reset the reconnect counter after a successful attach.
    pub fn set_reconnect_attempts(&self, attempts: u32) {
        self.reconnect_attempts.store(attempts, Ordering::Relaxed);
    }

    /// Count one lost link / reconnect cycle.
    pub fn note_reconnect(&self) -> u32 {
        self.reconnect_attempts.fetch_add(1, Ordering::Relaxed) + 1
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_radio_transports_default_to_connected() {
        let stats = make_link_stats();
        assert_eq!(stats.state(), RadioState::Connected);
        assert_eq!(stats.last_rx_ms(), None);
        assert_eq!(stats.reconnect_attempts(), 0);
    }

    #[test]
    fn radio_state_strings_are_stable_api_values() {
        assert_eq!(RadioState::Connecting.as_str(), "connecting");
        assert_eq!(RadioState::Connected.as_str(), "connected");
        assert_eq!(RadioState::Reconnecting.as_str(), "reconnecting");
        assert_eq!(RadioState::Stopped.as_str(), "disconnected");
        assert_eq!(RadioState::from_u8(2), RadioState::Reconnecting);
        assert_eq!(RadioState::from_u8(9), RadioState::Stopped);
    }

    #[test]
    fn record_rx_stamps_last_rx_ms() {
        let stats = make_link_stats();
        let before = util::now_ms();
        stats.record_rx(Some(-71), Some(4.25));
        let after = util::now_ms();
        let seen = stats.last_rx_ms().expect("last_rx_ms set");
        assert!(seen >= before && seen <= after);
        assert_eq!(stats.rx(), 1);
        assert_eq!(stats.rssi(), Some(-71));
        assert_eq!(stats.snr(), Some(4.25));
    }

    #[test]
    fn state_and_reconnect_attempts_roundtrip() {
        let stats = make_link_stats();
        stats.set_state(RadioState::Connecting);
        assert_eq!(stats.state(), RadioState::Connecting);
        assert_eq!(stats.note_reconnect(), 1);
        assert_eq!(stats.note_reconnect(), 2);
        stats.set_state(RadioState::Connected);
        stats.set_reconnect_attempts(0);
        assert_eq!(stats.state(), RadioState::Connected);
        assert_eq!(stats.reconnect_attempts(), 0);
        stats.set_state(RadioState::Stopped);
        assert_eq!(stats.state(), RadioState::Stopped);
    }
}

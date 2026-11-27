//! USB serial radio transport (opt-in via `--features real-serial`).
//!
//! On connect the transport auto-detects which link protocol the radio speaks
//! (`PROTOCOL.md` §13–14, `COMPANION-API.md`):
//!
//! * **Companion mode** — the transport sends a companion `Hello` and waits for `HelloAck`. This is
//!   what OpenMesh product firmware speaks on USB CDC: SLIP-framed `OM | type | len | CBOR | crc16`
//!   chunks, with OMP frames travelling inside companion `Tx`/`Rx` messages.
//! * **Raw mode fallback** — if no `HelloAck` arrives within [`HELLO_TIMEOUT`], the historical raw
//!   framing is kept (SLIP-delimited OMP frames) for dumb-modem firmware without the companion API.
//!
//! A companion radio only pushes `Rx`/`Event` messages to the host while it has seen a valid chunk
//! within the last 90 s (firmware `kCompanionTimeoutMs`), so companion mode sends a `Ping` every
//! [`PING_INTERVAL`]. The blocking serial IO runs on a dedicated OS thread and exchanges frames
//! with the async side over channels.

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serialport::SerialPort;
use tokio::sync::{mpsc, watch};

use openmesh_core::companion::{self, CompanionMsg};
use openmesh_core::{HEADER_LEN, LinkId, LinkInfo, MTU};

use super::{Inbound, LinkStats, TransportFrame, TransportHandle, make_link_stats};

/// SLIP end-of-frame delimiter.
pub const SLIP_END: u8 = 0xC0;
/// SLIP escape byte.
pub const SLIP_ESC: u8 = 0xDB;
/// Escaped `0xC0`.
pub const SLIP_ESC_END: u8 = 0xDC;
/// Escaped `0xDB`.
pub const SLIP_ESC_ESC: u8 = 0xDD;

/// How long the handshake waits for a companion `HelloAck` before falling back to raw framing.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(2);
/// Companion keepalive interval; must stay well below the firmware's 90 s companion timeout.
pub const PING_INTERVAL: Duration = Duration::from_secs(20);
/// Companion protocol version understood by the bridge (`COMPANION-API.md` §3).
const COMPANION_PROTO: u8 = 1;
/// Blocking read timeout: bounds shutdown latency and keeps the IO loop responsive.
const READ_TIMEOUT: Duration = Duration::from_millis(50);
/// Largest partial SLIP stream buffered before resynchronising (a companion chunk is ≤ 1 KiB).
const MAX_SLIP_BUFFER: usize = 64 * 1024;
/// Outbound frame queue per transport.
const OUTBOUND_QUEUE: usize = 64;

/// Frame a payload with SLIP delimiters.
pub fn slip_encode(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 2);
    out.push(SLIP_END);
    for &byte in payload {
        match byte {
            SLIP_END => out.extend_from_slice(&[SLIP_ESC, SLIP_ESC_END]),
            SLIP_ESC => out.extend_from_slice(&[SLIP_ESC, SLIP_ESC_ESC]),
            other => out.push(other),
        }
    }
    out.push(SLIP_END);
    out
}

/// Streaming SLIP decoder tolerant of noise and split reads.
///
/// Bytes before the first `0xC0` delimiter are treated as line noise and discarded, exactly like
/// the firmware's companion link parser. Used by the raw fallback mode; companion mode uses
/// [`CompanionDecoder`] because its chunks carry their own length and CRC.
#[derive(Debug, Default, Clone)]
pub struct SlipDecoder {
    buf: Vec<u8>,
    pending_escape: bool,
    synced: bool,
}

impl SlipDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push raw bytes, returning any completed payloads.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        for &byte in bytes {
            if self.pending_escape {
                self.pending_escape = false;
                match byte {
                    SLIP_ESC_END => self.buf.push(SLIP_END),
                    SLIP_ESC_ESC => self.buf.push(SLIP_ESC),
                    _ => {
                        // Malformed escape: drop the partial frame rather than propagate junk.
                        self.buf.clear();
                        self.synced = false;
                    }
                }
                continue;
            }
            match byte {
                SLIP_END => {
                    if !self.buf.is_empty() {
                        frames.push(std::mem::take(&mut self.buf));
                    }
                    self.buf.clear();
                    self.synced = true;
                }
                SLIP_ESC => {
                    if self.synced {
                        self.pending_escape = true;
                    }
                }
                other => {
                    if !self.synced {
                        continue;
                    }
                    self.buf.push(other);
                }
            }
        }
        frames
    }
}

/// Serial transport configuration.
#[derive(Clone, Debug)]
pub struct SerialConfig {
    pub port: String,
    pub baud: u32,
    /// LoRa region configured for the bridge, compared against the radio's `HelloAck` (informational
    /// only: the firmware owns the PHY configuration and is never reconfigured by the transport).
    pub region: String,
    /// LoRa profile configured for the bridge, compared against the radio's `HelloAck`.
    pub profile: String,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port: "/dev/ttyACM0".to_string(),
            baud: 115_200,
            region: "EU868".to_string(),
            profile: "MEDIUM_FAST".to_string(),
        }
    }
}

/// Facts reported by the radio in a companion `HelloAck` (`COMPANION-API.md` §3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanionInfo {
    /// Companion protocol version implemented by the radio.
    pub proto: u8,
    /// Radio firmware version string.
    pub fw_version: String,
    /// Radio node id (its own standalone identity, not the bridge's).
    pub node_id: [u8; 8],
    /// Largest OMP frame the radio accepts in a companion `Tx` message.
    pub max_frame: u16,
    /// Radio capability bitflags.
    pub caps: u16,
    /// Regulatory region the radio is tuned to.
    pub region: String,
    /// LoRa profile the radio is tuned to.
    pub profile: String,
}

/// Streaming decoder for SLIP-framed companion chunks.
///
/// Handles split reads, noise before/around frames and bad frames: a malformed chunk is dropped and
/// the stream resynchronises on the next delimiter instead of stalling.
#[derive(Debug, Default)]
pub struct CompanionDecoder {
    buf: Vec<u8>,
}

impl CompanionDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append `bytes` and return every complete companion message they finished.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<CompanionMsg> {
        self.buf.extend_from_slice(bytes);
        let mut messages = Vec::new();
        loop {
            match companion::decode_slip(&self.buf) {
                Ok((msg, consumed)) => {
                    self.buf.drain(..consumed);
                    messages.push(msg);
                }
                Err(openmesh_core::Error::BufferTooSmall { .. }) => {
                    // A malformed frame (e.g. a dangling escape before the closing delimiter) can
                    // also report BufferTooSmall even though a complete frame is buffered.
                    if self.has_complete_frame() {
                        tracing::debug!("dropping malformed companion frame");
                        self.resync();
                        continue;
                    }
                    break;
                }
                Err(err) => {
                    tracing::debug!(error = %err, "dropping malformed companion frame");
                    self.resync();
                }
            }
        }
        if self.buf.len() > MAX_SLIP_BUFFER {
            tracing::warn!(
                len = self.buf.len(),
                "companion stream buffer overflow; resynchronising"
            );
            self.buf.clear();
        }
        messages
    }

    /// Take the bytes that are buffered but not yet decoded (used when falling back to raw mode).
    pub fn take_buffered(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buf)
    }

    fn has_complete_frame(&self) -> bool {
        self.buf
            .iter()
            .filter(|byte| **byte == companion::SLIP_END)
            .count()
            >= 2
    }

    /// Drop through the closing delimiter of the malformed frame that starts the buffer.
    fn resync(&mut self) {
        let Some(first) = self
            .buf
            .iter()
            .position(|byte| *byte == companion::SLIP_END)
        else {
            self.buf.clear();
            return;
        };
        match self.buf[first + 1..]
            .iter()
            .position(|byte| *byte == companion::SLIP_END)
        {
            Some(relative) => {
                let end = first + 1 + relative;
                self.buf.drain(..=end);
            }
            None => {
                // Incomplete frame: keep it, discard noise that precedes the opening delimiter.
                self.buf.drain(..first);
            }
        }
    }
}

/// `Hello` sent when probing for a companion radio.
fn hello_message() -> CompanionMsg {
    CompanionMsg::Hello {
        proto: COMPANION_PROTO,
        app_version: format!("openmesh-bridge/{}", openmesh_core::CRATE_VERSION),
        wants_events: true,
    }
}

/// Encode one outgoing OMP frame as a SLIP-framed companion `Tx` message.
fn encode_companion_tx(frame: &[u8]) -> openmesh_core::Result<Vec<u8>> {
    companion::encode_slip(&CompanionMsg::Tx {
        frame: frame.to_vec(),
    })
}

/// Encode a companion keepalive `Ping`.
fn encode_companion_ping(nonce: u32) -> openmesh_core::Result<Vec<u8>> {
    companion::encode_slip(&CompanionMsg::Ping { nonce })
}

/// Extract the handshake facts from a radio → app `HelloAck`; `None` for other messages.
fn companion_info(msg: &CompanionMsg) -> Option<CompanionInfo> {
    match msg {
        CompanionMsg::HelloAck {
            proto,
            fw_version,
            node_id,
            max_frame,
            caps,
            region,
            profile,
        } => Some(CompanionInfo {
            proto: *proto,
            fw_version: fw_version.clone(),
            node_id: *node_id,
            max_frame: *max_frame,
            caps: *caps,
            region: region.clone(),
            profile: profile.clone(),
        }),
        _ => None,
    }
}

/// Map a radio → app `Rx` message to engine input; `None` for other messages and invalid frames.
fn rx_to_inbound(msg: &CompanionMsg, link: LinkId) -> Option<Inbound> {
    let CompanionMsg::Rx {
        frame, rssi, snr, ..
    } = msg
    else {
        return None;
    };
    if frame.len() < HEADER_LEN || frame.len() > MTU {
        return None;
    }
    Some(Inbound {
        bytes: frame.clone(),
        link: Some(LinkInfo {
            link,
            peer: None,
            rssi: Some(*rssi),
            // The companion wire format carries SNR in quarter-dB units (PROTOCOL.md §13).
            snr: Some(*snr as f32 / 4.0),
        }),
    })
}

/// Log the detected radio and warn when its PHY settings diverge from the bridge configuration.
fn log_companion_ready(info: &CompanionInfo, config: &SerialConfig) {
    tracing::info!(
        port = %config.port,
        proto = info.proto,
        fw_version = %info.fw_version,
        node_id = %hex::encode(info.node_id),
        max_frame = info.max_frame,
        caps = info.caps,
        region = %info.region,
        profile = %info.profile,
        "serial radio companion handshake complete (HelloAck)"
    );
    if info.proto != COMPANION_PROTO {
        tracing::warn!(
            radio_proto = info.proto,
            bridge_proto = COMPANION_PROTO,
            "companion protocol version mismatch; continuing best-effort"
        );
    }
    if !info.region.eq_ignore_ascii_case(&config.region) {
        tracing::warn!(
            bridge_region = %config.region,
            radio_region = %info.region,
            "radio region differs from the bridge configuration; not reconfiguring the radio"
        );
    }
    if !info.profile.eq_ignore_ascii_case(&config.profile) {
        tracing::warn!(
            bridge_profile = %config.profile,
            radio_profile = %info.profile,
            "radio profile differs from the bridge configuration; not reconfiguring the radio"
        );
    }
    if usize::from(info.max_frame) < MTU {
        tracing::warn!(
            max_frame = info.max_frame,
            mtu = MTU,
            "radio accepts smaller frames than the OMP MTU; oversized frames will be dropped"
        );
    }
}

/// Send a companion `Hello` and wait up to [`HELLO_TIMEOUT`] for `HelloAck`.
///
/// Returns the radio facts plus any messages that arrived after the ack in the same read, or
/// `None` when the radio did not answer (raw-frame firmware, wrong port or a dead link).
fn detect_companion(
    port: &mut dyn SerialPort,
    decoder: &mut CompanionDecoder,
    config: &SerialConfig,
) -> Option<(CompanionInfo, Vec<CompanionMsg>)> {
    let hello = match companion::encode_slip(&hello_message()) {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::warn!(error = %err, "failed to encode companion Hello");
            return None;
        }
    };
    if let Err(err) = port.write_all(&hello) {
        tracing::warn!(error = %err, port = %config.port, "failed to send companion Hello");
        return None;
    }
    if let Err(err) = port.flush() {
        tracing::debug!(error = %err, "serial flush after companion Hello failed");
    }
    let deadline = Instant::now() + HELLO_TIMEOUT;
    let mut read_buf = [0u8; 256];
    loop {
        if Instant::now() >= deadline {
            tracing::warn!(
                port = %config.port,
                "radio did not answer companion Hello; raw serial mode may be unsupported by this firmware"
            );
            return None;
        }
        match port.read(&mut read_buf) {
            Ok(n) if n > 0 => {
                let mut messages = decoder.push(&read_buf[..n]);
                if let Some((index, info)) = messages
                    .iter()
                    .enumerate()
                    .find_map(|(index, msg)| companion_info(msg).map(|info| (index, info)))
                {
                    let remaining = messages.split_off(index + 1);
                    messages.truncate(index);
                    for msg in &messages {
                        tracing::debug!(message = ?msg, "companion message before HelloAck");
                    }
                    return Some((info, remaining));
                }
                for msg in &messages {
                    tracing::debug!(message = ?msg, "companion message before HelloAck");
                }
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {}
            Err(err) => {
                tracing::warn!(error = %err, "serial read failed during companion handshake");
                return None;
            }
        }
    }
}

/// One open serial link plus the channels it exchanges frames over.
struct SerialLink {
    port: Box<dyn SerialPort>,
    config: SerialConfig,
    link: LinkId,
    inbound: mpsc::Sender<Inbound>,
    outbound: mpsc::Receiver<TransportFrame>,
    shutdown: watch::Receiver<bool>,
    stats: Arc<LinkStats>,
}

impl SerialLink {
    /// Detect the radio protocol, then pump frames until shutdown.
    fn run(mut self) {
        let _ = self.port.set_timeout(READ_TIMEOUT);
        let mut decoder = CompanionDecoder::new();
        match detect_companion(&mut *self.port, &mut decoder, &self.config) {
            Some((info, pending)) => {
                log_companion_ready(&info, &self.config);
                self.run_companion(&info, pending, decoder);
            }
            None => {
                let buffered = decoder.take_buffered();
                self.run_raw(buffered);
            }
        }
    }

    /// Companion mode: `Tx` out, `Rx`/`Event` in, `Ping` keepalive.
    fn run_companion(
        mut self,
        info: &CompanionInfo,
        pending: Vec<CompanionMsg>,
        mut decoder: CompanionDecoder,
    ) {
        let max_frame = usize::from(info.max_frame).min(MTU);
        for msg in pending {
            if !self.handle_companion_message(&msg) {
                return;
            }
        }
        let mut read_buf = [0u8; 512];
        let mut last_ping = Instant::now();
        let mut nonce: u32 = 0;
        loop {
            if *self.shutdown.borrow() {
                break;
            }
            while let Ok(frame) = self.outbound.try_recv() {
                if frame.link.is_some_and(|preferred| preferred != self.link) {
                    continue;
                }
                if frame.bytes.len() > max_frame {
                    tracing::warn!(
                        len = frame.bytes.len(),
                        max_frame = info.max_frame,
                        "dropping OMP frame larger than the radio max_frame"
                    );
                    continue;
                }
                match encode_companion_tx(&frame.bytes) {
                    Ok(bytes) => {
                        self.stats.record_tx();
                        if let Err(err) = self.port.write_all(&bytes) {
                            tracing::warn!(error = %err, "serial write failed");
                            return;
                        }
                    }
                    Err(err) => tracing::warn!(error = %err, "failed to encode companion Tx"),
                }
            }
            if last_ping.elapsed() >= PING_INTERVAL {
                last_ping = Instant::now();
                nonce = nonce.wrapping_add(1);
                match encode_companion_ping(nonce) {
                    Ok(bytes) => {
                        if let Err(err) = self.port.write_all(&bytes) {
                            tracing::warn!(error = %err, "serial keepalive write failed");
                            return;
                        }
                    }
                    Err(err) => tracing::warn!(error = %err, "failed to encode companion Ping"),
                }
            }
            match self.port.read(&mut read_buf) {
                Ok(n) if n > 0 => {
                    for msg in decoder.push(&read_buf[..n]) {
                        if !self.handle_companion_message(&msg) {
                            return;
                        }
                    }
                }
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {}
                Err(err) => {
                    tracing::warn!(error = %err, "serial read failed");
                    return;
                }
            }
        }
    }

    /// Handle one decoded companion message. Returns `false` when the engine side is gone.
    fn handle_companion_message(&mut self, msg: &CompanionMsg) -> bool {
        match msg {
            CompanionMsg::Rx { .. } => {
                let Some(inbound) = rx_to_inbound(msg, self.link) else {
                    tracing::debug!("dropping companion Rx with an invalid OMP frame size");
                    return true;
                };
                self.stats.record_rx(
                    inbound.link.as_ref().and_then(|info| info.rssi),
                    inbound.link.as_ref().and_then(|info| info.snr),
                );
                self.inbound.blocking_send(inbound).is_ok()
            }
            CompanionMsg::Event { kind, detail } => {
                tracing::info!(kind = %kind, detail = %detail, "serial radio event");
                true
            }
            CompanionMsg::HelloAck { fw_version, .. } => {
                tracing::info!(fw_version = %fw_version, "serial radio sent another companion HelloAck");
                true
            }
            CompanionMsg::Pong { nonce } => {
                tracing::debug!(nonce, "companion keepalive answered");
                true
            }
            other => {
                tracing::debug!(message = ?other, "ignoring companion message");
                true
            }
        }
    }

    /// Raw fallback mode: SLIP-delimited OMP frames in both directions.
    fn run_raw(mut self, buffered: Vec<u8>) {
        tracing::info!(
            port = %self.config.port,
            "serial radio using raw OMP frame mode (no companion HelloAck)"
        );
        let mut decoder = SlipDecoder::new();
        let mut read_buf = [0u8; 512];
        for frame in decoder.push(&buffered) {
            if !self.accept_raw_frame(frame) {
                return;
            }
        }
        loop {
            if *self.shutdown.borrow() {
                break;
            }
            while let Ok(frame) = self.outbound.try_recv() {
                if frame.link.is_some_and(|preferred| preferred != self.link) {
                    continue;
                }
                self.stats.record_tx();
                if let Err(err) = self.port.write_all(&slip_encode(&frame.bytes)) {
                    tracing::warn!(error = %err, "serial write failed");
                    return;
                }
            }
            match self.port.read(&mut read_buf) {
                Ok(n) if n > 0 => {
                    for frame in decoder.push(&read_buf[..n]) {
                        if !self.accept_raw_frame(frame) {
                            return;
                        }
                    }
                }
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {}
                Err(err) => {
                    tracing::warn!(error = %err, "serial read failed");
                    return;
                }
            }
        }
    }

    /// Feed one raw-mode frame to the engine. Returns `false` when the engine side is gone.
    fn accept_raw_frame(&mut self, frame: Vec<u8>) -> bool {
        if frame.len() < HEADER_LEN || frame.len() > MTU {
            return true;
        }
        self.stats.record_rx(None, None);
        let msg = Inbound {
            bytes: frame,
            link: Some(LinkInfo {
                link: self.link,
                peer: None,
                rssi: None,
                snr: None,
            }),
        };
        self.inbound.blocking_send(msg).is_ok()
    }
}

/// Open `path` and start the serial transport thread.
pub fn spawn(
    config: SerialConfig,
    link: LinkId,
    inbound: mpsc::Sender<Inbound>,
    shutdown: watch::Receiver<bool>,
) -> Result<TransportHandle> {
    let port = serialport::new(&config.port, config.baud)
        .timeout(READ_TIMEOUT)
        .open()
        .with_context(|| format!("opening serial port {}", config.port))?;
    let (tx, outbound) = mpsc::channel::<TransportFrame>(OUTBOUND_QUEUE);
    let stats = make_link_stats();
    let stats_task = stats.clone();
    let label = format!("Serial {} @{}", config.port, config.baud);
    std::thread::Builder::new()
        .name("openmesh-serial".into())
        .spawn(move || {
            let serial = SerialLink {
                port,
                config,
                link,
                inbound,
                outbound,
                shutdown,
                stats: stats_task,
            };
            serial.run();
            tracing::debug!("serial transport stopped");
        })
        .context("spawning serial reader thread")?;
    Ok(TransportHandle {
        id: link,
        kind: "serial",
        label,
        tx,
        stats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello_ack() -> CompanionMsg {
        CompanionMsg::HelloAck {
            proto: COMPANION_PROTO,
            fw_version: "0.7.1".to_string(),
            node_id: [0x68, 0x5a, 0x7e, 0xda, 0x39, 0xa2, 0xad, 0x9c],
            max_frame: 240,
            caps: 615,
            region: "EU868".to_string(),
            profile: "MEDIUM_FAST".to_string(),
        }
    }

    fn rx_message(frame: Vec<u8>) -> CompanionMsg {
        CompanionMsg::Rx {
            frame,
            rssi: -71,
            snr: 16,
            link: "lora".to_string(),
            at_ms: 42,
        }
    }

    fn encoded(msg: &CompanionMsg) -> Vec<u8> {
        companion::encode_slip(msg).expect("encode companion message")
    }

    // --- existing raw-frame SLIP tests -------------------------------------------------------

    #[test]
    fn slip_roundtrip_escapes_special_bytes() {
        let payload = vec![0x01, SLIP_END, 0x02, SLIP_ESC, 0x03];
        let encoded = slip_encode(&payload);
        assert_eq!(encoded.first(), Some(&SLIP_END));
        assert_eq!(encoded.last(), Some(&SLIP_END));
        let mut decoder = SlipDecoder::new();
        let frames = decoder.push(&encoded);
        assert_eq!(frames, vec![payload]);
    }

    #[test]
    fn slip_decoder_handles_split_and_noise() {
        let payload = vec![9u8; 40];
        let encoded = slip_encode(&payload);
        let mut decoder = SlipDecoder::new();
        let mut frames = Vec::new();
        // Leading noise before the first delimiter is ignored by the frame sync.
        frames.extend(decoder.push(&[0, 7, 9]));
        for chunk in encoded.chunks(3) {
            frames.extend(decoder.push(chunk));
        }
        assert_eq!(frames, vec![payload]);
    }

    #[test]
    fn slip_decoder_skips_empty_frames() {
        let mut decoder = SlipDecoder::new();
        assert!(decoder.push(&[SLIP_END, SLIP_END, SLIP_END]).is_empty());
    }

    #[test]
    fn malformed_escape_drops_frame() {
        let mut decoder = SlipDecoder::new();
        decoder.push(&[0x11, SLIP_ESC, 0x42]);
        let frames = decoder.push(&[SLIP_END]);
        assert!(frames.is_empty());
        assert_eq!(decoder.push(&slip_encode(b"ok")), vec![b"ok".to_vec()]);
    }

    // --- companion-mode codec + mapping tests -----------------------------------------------

    #[test]
    fn companion_hello_is_wire_compatible() {
        let wire = encoded(&hello_message());
        let (msg, consumed) = companion::decode_slip(&wire).expect("Hello decodes");
        assert_eq!(consumed, wire.len());
        match msg {
            CompanionMsg::Hello {
                proto,
                app_version,
                wants_events,
            } => {
                assert_eq!(proto, COMPANION_PROTO);
                assert!(
                    app_version.starts_with("openmesh-bridge/"),
                    "unexpected app_version {app_version}"
                );
                assert!(wants_events);
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[test]
    fn companion_decoder_reassembles_split_frames_and_noise() {
        let frame = vec![0x42u8; 40];
        let wire = encoded(&rx_message(frame.clone()));
        let mut decoder = CompanionDecoder::new();
        let mut messages = Vec::new();
        // Line noise before the frame plus deliberately split reads.
        messages.extend(decoder.push(&[0x00, 0x11, 0x22]));
        for chunk in wire.chunks(3) {
            messages.extend(decoder.push(chunk));
        }
        assert_eq!(messages.len(), 1);
        let inbound = rx_to_inbound(&messages[0], 7).expect("Rx maps to engine input");
        assert_eq!(inbound.bytes, frame);
        let info = inbound.link.expect("link info");
        assert_eq!(info.link, 7);
        assert_eq!(info.rssi, Some(-71));
        assert_eq!(info.snr, Some(4.0));
    }

    #[test]
    fn companion_hello_ack_is_classified() {
        let wire = encoded(&hello_ack());
        let mut decoder = CompanionDecoder::new();
        let messages = decoder.push(&wire);
        let info = companion_info(&messages[0]).expect("HelloAck classified");
        assert_eq!(
            info,
            CompanionInfo {
                proto: 1,
                fw_version: "0.7.1".to_string(),
                node_id: [0x68, 0x5a, 0x7e, 0xda, 0x39, 0xa2, 0xad, 0x9c],
                max_frame: 240,
                caps: 615,
                region: "EU868".to_string(),
                profile: "MEDIUM_FAST".to_string(),
            }
        );
        assert_eq!(hex::encode(info.node_id), "685a7eda39a2ad9c");
    }

    #[test]
    fn companion_tx_roundtrips_omp_frame() {
        // 0xC0/0xDB bytes must survive the SLIP escaping.
        let frame = vec![0xAA, SLIP_END, SLIP_ESC, 0x00, 0x7F];
        let wire = encode_companion_tx(&frame).expect("Tx encodes");
        let (msg, consumed) = companion::decode_slip(&wire).expect("Tx decodes");
        assert_eq!(consumed, wire.len());
        match msg {
            CompanionMsg::Tx { frame: decoded } => assert_eq!(decoded, frame),
            other => panic!("expected Tx, got {other:?}"),
        }
    }

    #[test]
    fn companion_ping_roundtrips() {
        let wire = encode_companion_ping(0xDEAD_BEEF).expect("Ping encodes");
        let (msg, consumed) = companion::decode_slip(&wire).expect("Ping decodes");
        assert_eq!(consumed, wire.len());
        assert_eq!(msg, CompanionMsg::Ping { nonce: 0xDEAD_BEEF });
    }

    #[test]
    fn companion_decoder_resyncs_after_bad_frame() {
        // A complete frame with a flipped CRC is dropped; the next good frame still decodes.
        let mut bad_chunk = companion::encode(&hello_ack()).expect("chunk encodes");
        let last = bad_chunk.len() - 1;
        bad_chunk[last] ^= 0xFF;
        let mut stream = slip_encode(&bad_chunk);
        stream.extend_from_slice(&encoded(&hello_ack()));
        let mut decoder = CompanionDecoder::new();
        let messages = decoder.push(&stream);
        assert_eq!(messages.len(), 1, "bad frame dropped, good frame decoded");
        assert!(companion_info(&messages[0]).is_some());
    }

    #[test]
    fn companion_rx_rejects_invalid_frame_size() {
        // A frame shorter than the fixed OMP header is malformed and is not forwarded.
        let msg = rx_message(vec![0x01, 0x02]);
        assert!(rx_to_inbound(&msg, 1).is_none());
        // Non-Rx messages never become engine input.
        assert!(rx_to_inbound(&hello_ack(), 1).is_none());
    }

    #[test]
    fn companion_info_ignores_other_messages() {
        assert!(companion_info(&CompanionMsg::Ping { nonce: 1 }).is_none());
        assert!(companion_info(&rx_message(vec![0u8; HEADER_LEN])).is_none());
    }

    // --- PTY fake-radio integration test (unix only, no hardware) ----------------------------

    /// Drain whatever the radio end has and decode complete companion frames.
    #[cfg(unix)]
    fn pump(
        radio: &mut dyn SerialPort,
        buf: &mut Vec<u8>,
        read_buf: &mut [u8; 512],
    ) -> Vec<CompanionMsg> {
        if let Ok(n) = radio.read(read_buf) {
            if n > 0 {
                buf.extend_from_slice(&read_buf[..n]);
            }
        }
        let mut messages = Vec::new();
        loop {
            match companion::decode_slip(buf) {
                Ok((msg, consumed)) => {
                    buf.drain(..consumed);
                    messages.push(msg);
                }
                Err(openmesh_core::Error::BufferTooSmall { .. }) => break,
                Err(_) => {
                    // Keep the fake radio simple: drop unparsable bytes and resync.
                    buf.clear();
                    break;
                }
            }
        }
        messages
    }

    #[cfg(unix)]
    #[test]
    fn companion_mode_round_trip_over_pty() {
        let (mut radio, slave) = serialport::TTYPort::pair().expect("create pty pair");
        radio.set_timeout(READ_TIMEOUT).expect("radio timeout");

        let (outbound_tx, outbound_rx) = mpsc::channel::<TransportFrame>(OUTBOUND_QUEUE);
        let (inbound_tx, mut inbound_rx) = mpsc::channel::<Inbound>(8);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let stats = make_link_stats();
        let stats_thread = stats.clone();
        let config = SerialConfig {
            port: "pty-fake-radio".to_string(),
            baud: 115_200,
            region: "EU868".to_string(),
            profile: "MEDIUM_FAST".to_string(),
        };
        let link_thread = std::thread::spawn(move || {
            let port: Box<dyn SerialPort> = Box::new(slave);
            let serial = SerialLink {
                port,
                config,
                link: 9,
                inbound: inbound_tx,
                outbound: outbound_rx,
                shutdown: shutdown_rx,
                stats: stats_thread,
            };
            serial.run();
        });

        let mut buf = Vec::new();
        let mut read_buf = [0u8; 512];

        // 1. The bridge must send a companion Hello.
        let mut saw_hello = false;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && !saw_hello {
            for msg in pump(&mut radio, &mut buf, &mut read_buf) {
                if matches!(msg, CompanionMsg::Hello { proto: 1, .. }) {
                    saw_hello = true;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(saw_hello, "bridge sent companion Hello");

        // 2. Answer with HelloAck: the bridge switches to companion mode.
        radio
            .write_all(&encoded(&hello_ack()))
            .expect("write HelloAck");
        std::thread::sleep(Duration::from_millis(100));

        // 3. Engine egress must arrive as a companion Tx.
        let frame = vec![0x42u8; 40];
        outbound_tx
            .try_send(TransportFrame {
                bytes: frame.clone(),
                mode: openmesh_core::SendMode::Flood,
                link: None,
                exclude: None,
            })
            .expect("queue outbound frame");
        let mut saw_tx = false;
        let mut sent_frame = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && !saw_tx {
            for msg in pump(&mut radio, &mut buf, &mut read_buf) {
                if let CompanionMsg::Tx { frame } = msg {
                    saw_tx = true;
                    sent_frame = frame;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(saw_tx, "bridge sent companion Tx for an engine frame");
        assert_eq!(sent_frame, frame);

        // 4. Radio ingress must reach the engine as Inbound with signal metadata.
        let rx_frame = vec![0x33u8; 40];
        radio
            .write_all(&encoded(&rx_message(rx_frame.clone())))
            .expect("write Rx");
        let mut got_inbound = None;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && got_inbound.is_none() {
            if let Ok(inbound) = inbound_rx.try_recv() {
                got_inbound = Some(inbound);
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let inbound = got_inbound.expect("bridge forwarded companion Rx to the engine");
        assert_eq!(inbound.bytes, rx_frame);
        let info = inbound.link.expect("link info");
        assert_eq!(info.link, 9);
        assert_eq!(info.rssi, Some(-71));
        assert_eq!(info.snr, Some(4.0));
        assert_eq!(stats.rx(), 1);

        // 5. Shutdown stops the link thread cleanly.
        shutdown_tx.send(true).expect("send shutdown");
        link_thread.join().expect("serial thread stops");
    }
}

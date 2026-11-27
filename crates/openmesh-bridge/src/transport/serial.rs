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
//!
//! If the link drops (radio reset/reflash, USB re-enumeration, cable pull), the worker logs a
//! warning, reopens the port with bounded exponential backoff ([`RECONNECT_MIN`] →
//! [`RECONNECT_MAX`]) and re-runs the handshake. Shutdown stays responsive while waiting.
//!
//! **Writes are not link health.** A USB CDC write can block for the duration of the radio's
//! LoRa transmission (and the EU868 duty cycle): that is normal backpressure, not a dead link.
//! Frames are written with a generous [`WRITE_TIMEOUT`]; a timeout is retried a bounded number
//! of times and then the frame is dropped (OMP tolerates loss). Only read errors, EOF and a
//! failed reopen trigger the reconnect path.

use std::io::Read;
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
/// First delay before reopening a lost serial link; doubles on every consecutive failure.
pub const RECONNECT_MIN: Duration = Duration::from_millis(250);
/// Upper bound for the reconnect backoff.
pub const RECONNECT_MAX: Duration = Duration::from_secs(5);
/// How often the reconnect wait checks for shutdown.
const SHUTDOWN_POLL: Duration = Duration::from_millis(25);
/// Serial write timeout: a write may block while the radio transmits a frame over LoRa, so this
/// must absorb radio-busy backpressure (EU868 airtime for one frame is ~1 s). Reads keep the
/// short [`READ_TIMEOUT`] because it is restored after every write.
const WRITE_TIMEOUT: Duration = Duration::from_secs(3);
/// Maximum write attempts for one frame when the radio is busy (initial attempt + one retry).
const WRITE_MAX_ATTEMPTS: u32 = 2;
/// Delay between write retries while the radio drains its transmit queue.
const WRITE_RETRY_DELAY: Duration = Duration::from_millis(250);
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
/// `Ok(Some(..))` — companion radio detected (facts + any messages that arrived after the ack in
/// the same read). `Ok(None)` — the radio did not answer, so raw-frame mode is the fallback.
/// `Err(reason)` — the port itself failed (EOF/read/write error); the caller must reconnect.
fn detect_companion(
    port: &mut dyn SerialPort,
    decoder: &mut CompanionDecoder,
    config: &SerialConfig,
) -> Result<Option<(CompanionInfo, Vec<CompanionMsg>)>, &'static str> {
    let hello = match companion::encode_slip(&hello_message()) {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::warn!(error = %err, "failed to encode companion Hello");
            return Err("failed to encode companion Hello");
        }
    };
    let (hello_result, _) = write_frame_bytes(port, &hello, WRITE_TIMEOUT);
    if let Err(err) = hello_result {
        tracing::warn!(error = %err, port = %config.port, "failed to send companion Hello");
        return Err("serial write failed during companion handshake");
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
            return Ok(None);
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
                    return Ok(Some((info, remaining)));
                }
                for msg in &messages {
                    tracing::debug!(message = ?msg, "companion message before HelloAck");
                }
            }
            Ok(_) => {
                tracing::warn!(port = %config.port, "serial link closed (EOF) during companion handshake");
                return Err("serial link closed (EOF)");
            }
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {}
            Err(err) => {
                tracing::warn!(error = %err, "serial read failed during companion handshake");
                return Err("serial read failed during companion handshake");
            }
        }
    }
}

/// Outcome of one connected session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkOutcome {
    /// Shutdown was requested; stop without reconnecting.
    Shutdown,
    /// The link failed (EOF, read/write error); reopen the port and re-run detection.
    Disconnected(&'static str),
}

/// Double the reconnect backoff, capped at [`RECONNECT_MAX`].
fn grow_backoff(current: Duration) -> Duration {
    (current.saturating_mul(2)).min(RECONNECT_MAX)
}

/// Open the configured serial port (USB CDC ignores the baud rate).
fn open_serial(config: &SerialConfig) -> Result<Box<dyn SerialPort>> {
    serialport::new(&config.port, config.baud)
        .timeout(READ_TIMEOUT)
        .open()
        .with_context(|| format!("opening serial port {}", config.port))
}

/// Write all of `bytes` with `timeout`, restoring the port's previous (read) timeout afterwards.
///
/// Returns the I/O result plus how many bytes were written, so the caller can distinguish a
/// zero-progress timeout (safe to retry) from a partial frame (must be dropped: retrying would
/// duplicate the prefix on the wire).
fn write_frame_bytes(
    port: &mut dyn SerialPort,
    bytes: &[u8],
    timeout: Duration,
) -> (std::io::Result<()>, usize) {
    let previous = port.timeout();
    let _ = port.set_timeout(timeout);
    let mut written = 0usize;
    let result = loop {
        if written >= bytes.len() {
            break Ok(());
        }
        match port.write(&bytes[written..]) {
            Ok(0) => {
                break Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "serial write returned 0 bytes",
                ));
            }
            Ok(n) => written += n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => break Err(err),
        }
    };
    let _ = port.set_timeout(previous);
    (result, written)
}

/// Owns the `--serial` link: opens the port, detects the protocol and reconnects when it drops.
struct SerialWorker {
    config: SerialConfig,
    link: LinkId,
    inbound: mpsc::Sender<Inbound>,
    outbound: mpsc::Receiver<TransportFrame>,
    shutdown: watch::Receiver<bool>,
    stats: Arc<LinkStats>,
    /// Current reconnect delay; reset to [`RECONNECT_MIN`] whenever a session attaches.
    backoff: Duration,
}

impl SerialWorker {
    /// Run until shutdown, reconnecting with bounded backoff whenever the radio link fails.
    fn run(mut self, first_port: Box<dyn SerialPort>) {
        let mut next_port = Some(first_port);
        loop {
            if *self.shutdown.borrow() {
                break;
            }
            let port = match next_port.take() {
                Some(port) => port,
                None => match open_serial(&self.config) {
                    Ok(port) => port,
                    Err(err) => {
                        tracing::warn!(
                            error = %err,
                            port = %self.config.port,
                            retry_in_ms = self.backoff.as_millis(),
                            "serial radio unavailable; retrying"
                        );
                        if !self.wait_for_shutdown(self.backoff) {
                            break;
                        }
                        self.backoff = grow_backoff(self.backoff);
                        continue;
                    }
                },
            };
            match self.run_session(port) {
                LinkOutcome::Shutdown => break,
                LinkOutcome::Disconnected(reason) => {
                    tracing::warn!(
                        port = %self.config.port,
                        reason,
                        retry_in_ms = self.backoff.as_millis(),
                        "serial radio link lost; reconnecting"
                    );
                    if !self.wait_for_shutdown(self.backoff) {
                        break;
                    }
                    self.backoff = grow_backoff(self.backoff);
                }
            }
        }
        tracing::info!(port = %self.config.port, "serial transport stopped");
    }

    /// Detect the radio protocol on a freshly opened port, then pump frames until it fails.
    fn run_session(&mut self, mut port: Box<dyn SerialPort>) -> LinkOutcome {
        let _ = port.set_timeout(READ_TIMEOUT);
        let mut decoder = CompanionDecoder::new();
        match detect_companion(&mut *port, &mut decoder, &self.config) {
            Ok(Some((info, pending))) => {
                log_companion_ready(&info, &self.config);
                self.backoff = RECONNECT_MIN;
                self.run_companion(port, &info, pending, decoder)
            }
            Ok(None) => {
                self.backoff = RECONNECT_MIN;
                let buffered = decoder.take_buffered();
                self.run_raw(port, buffered)
            }
            Err(reason) => LinkOutcome::Disconnected(reason),
        }
    }

    /// Wait for `delay`, returning `false` when shutdown was requested first.
    fn wait_for_shutdown(&self, delay: Duration) -> bool {
        let deadline = Instant::now() + delay;
        loop {
            if *self.shutdown.borrow() {
                return false;
            }
            let now = Instant::now();
            if now >= deadline {
                return true;
            }
            std::thread::sleep(SHUTDOWN_POLL.min(deadline - now));
        }
    }

    /// Write one SLIP-framed chunk, tolerating radio-busy backpressure.
    ///
    /// Returns `false` when the frame was dropped. Write failures never tear the link down: real
    /// link loss is detected by the read path. A zero-progress timeout (the radio is draining its
    /// transmit queue) is retried once within [`WRITE_MAX_ATTEMPTS`]; a partially written frame is
    /// dropped instead of retried so the wire never carries a duplicated prefix.
    fn write_frame(&mut self, port: &mut dyn SerialPort, bytes: &[u8]) -> bool {
        for attempt in 1..=WRITE_MAX_ATTEMPTS {
            let (result, written) = write_frame_bytes(port, bytes, WRITE_TIMEOUT);
            match result {
                Ok(()) => return true,
                Err(err) if written > 0 => {
                    tracing::warn!(
                        error = %err,
                        written,
                        len = bytes.len(),
                        "serial write stopped partway; dropping frame (radio backpressure)"
                    );
                    return false;
                }
                Err(err) => {
                    let transient = matches!(
                        err.kind(),
                        std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::Interrupted
                    );
                    if !transient {
                        tracing::warn!(
                            error = %err,
                            "serial write failed; dropping frame (the read path detects real link loss)"
                        );
                        return false;
                    }
                    tracing::warn!(
                        error = %err,
                        attempt,
                        max_attempts = WRITE_MAX_ATTEMPTS,
                        "serial write timed out (radio busy); retrying frame"
                    );
                    if attempt < WRITE_MAX_ATTEMPTS && !self.wait_for_shutdown(WRITE_RETRY_DELAY) {
                        return false;
                    }
                }
            }
        }
        tracing::warn!(
            len = bytes.len(),
            "dropping frame after repeated write backpressure; link stays up"
        );
        false
    }

    /// Companion mode: `Tx` out, `Rx`/`Event` in, `Ping` keepalive.
    fn run_companion(
        &mut self,
        mut port: Box<dyn SerialPort>,
        info: &CompanionInfo,
        pending: Vec<CompanionMsg>,
        mut decoder: CompanionDecoder,
    ) -> LinkOutcome {
        let max_frame = usize::from(info.max_frame).min(MTU);
        for msg in pending {
            if !self.handle_companion_message(&msg) {
                return LinkOutcome::Shutdown;
            }
        }
        let mut read_buf = [0u8; 512];
        let mut last_ping = Instant::now();
        let mut nonce: u32 = 0;
        loop {
            if *self.shutdown.borrow() {
                return LinkOutcome::Shutdown;
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
                        // A write failure is radio backpressure, not link loss: drop the frame
                        // (OMP tolerates loss) and keep the session up.
                        let _ = self.write_frame(&mut *port, &bytes);
                    }
                    Err(err) => tracing::warn!(error = %err, "failed to encode companion Tx"),
                }
            }
            if last_ping.elapsed() >= PING_INTERVAL {
                last_ping = Instant::now();
                nonce = nonce.wrapping_add(1);
                match encode_companion_ping(nonce) {
                    Ok(bytes) => {
                        // A lost keepalive is harmless: the next one is 20 s away.
                        let _ = self.write_frame(&mut *port, &bytes);
                    }
                    Err(err) => tracing::warn!(error = %err, "failed to encode companion Ping"),
                }
            }
            match port.read(&mut read_buf) {
                Ok(n) if n > 0 => {
                    for msg in decoder.push(&read_buf[..n]) {
                        if !self.handle_companion_message(&msg) {
                            return LinkOutcome::Shutdown;
                        }
                    }
                }
                Ok(_) => {
                    tracing::warn!(port = %self.config.port, "serial link closed (EOF)");
                    return LinkOutcome::Disconnected("serial link closed (EOF)");
                }
                Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {}
                Err(err) => {
                    tracing::warn!(error = %err, "serial read failed");
                    return LinkOutcome::Disconnected("serial read failed");
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
    fn run_raw(&mut self, mut port: Box<dyn SerialPort>, buffered: Vec<u8>) -> LinkOutcome {
        tracing::info!(
            port = %self.config.port,
            "serial radio using raw OMP frame mode (no companion HelloAck)"
        );
        let mut decoder = SlipDecoder::new();
        let mut read_buf = [0u8; 512];
        for frame in decoder.push(&buffered) {
            if !self.accept_raw_frame(frame) {
                return LinkOutcome::Shutdown;
            }
        }
        loop {
            if *self.shutdown.borrow() {
                return LinkOutcome::Shutdown;
            }
            while let Ok(frame) = self.outbound.try_recv() {
                if frame.link.is_some_and(|preferred| preferred != self.link) {
                    continue;
                }
                self.stats.record_tx();
                // A write failure is radio backpressure, not link loss: drop the frame and keep
                // the session (read errors/EOF handle real disconnects).
                let _ = self.write_frame(&mut *port, &slip_encode(&frame.bytes));
            }
            match port.read(&mut read_buf) {
                Ok(n) if n > 0 => {
                    for frame in decoder.push(&read_buf[..n]) {
                        if !self.accept_raw_frame(frame) {
                            return LinkOutcome::Shutdown;
                        }
                    }
                }
                Ok(_) => {
                    tracing::warn!(port = %self.config.port, "serial link closed (EOF)");
                    return LinkOutcome::Disconnected("serial link closed (EOF)");
                }
                Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {}
                Err(err) => {
                    tracing::warn!(error = %err, "serial read failed");
                    return LinkOutcome::Disconnected("serial read failed");
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
    let port = open_serial(&config)?;
    let (tx, outbound) = mpsc::channel::<TransportFrame>(OUTBOUND_QUEUE);
    let stats = make_link_stats();
    let stats_task = stats.clone();
    let label = format!("Serial {} @{}", config.port, config.baud);
    std::thread::Builder::new()
        .name("openmesh-serial".into())
        .spawn(move || {
            let worker = SerialWorker {
                config,
                link,
                inbound,
                outbound,
                shutdown,
                stats: stats_task,
                backoff: RECONNECT_MIN,
            };
            worker.run(port);
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

    // --- reconnect backoff unit tests ------------------------------------------------------

    #[test]
    fn reconnect_backoff_doubles_and_caps() {
        let mut delay = RECONNECT_MIN;
        assert_eq!(delay, Duration::from_millis(250));
        for expected in [
            Duration::from_millis(500),
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(4),
            RECONNECT_MAX,
            RECONNECT_MAX,
        ] {
            delay = grow_backoff(delay);
            assert_eq!(delay, expected);
        }
    }

    // --- PTY fake-radio integration tests (unix only, no hardware) -------------------------

    /// A fake OpenMesh radio on the master side of a PTY pair.
    #[cfg(unix)]
    struct FakeRadio {
        port: serialport::TTYPort,
        buf: Vec<u8>,
        read_buf: [u8; 512],
    }

    #[cfg(unix)]
    impl FakeRadio {
        fn new(mut port: serialport::TTYPort) -> Self {
            port.set_timeout(READ_TIMEOUT).expect("radio timeout");
            Self {
                port,
                buf: Vec::new(),
                read_buf: [0u8; 512],
            }
        }

        /// Drain the radio end and decode every complete companion frame.
        fn pump(&mut self) -> Vec<CompanionMsg> {
            if let Ok(n) = self.port.read(&mut self.read_buf) {
                if n > 0 {
                    self.buf.extend_from_slice(&self.read_buf[..n]);
                }
            }
            let mut messages = Vec::new();
            loop {
                match companion::decode_slip(&self.buf) {
                    Ok((msg, consumed)) => {
                        self.buf.drain(..consumed);
                        messages.push(msg);
                    }
                    Err(openmesh_core::Error::BufferTooSmall { .. }) => break,
                    Err(_) => {
                        // Keep the fake radio simple: drop unparsable bytes and resync.
                        self.buf.clear();
                        break;
                    }
                }
            }
            messages
        }

        fn wait_for(
            &mut self,
            timeout: Duration,
            predicate: impl Fn(&CompanionMsg) -> bool,
        ) -> Option<CompanionMsg> {
            let deadline = Instant::now() + timeout;
            loop {
                for msg in self.pump() {
                    if predicate(&msg) {
                        return Some(msg);
                    }
                }
                if Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        fn write_msg(&mut self, msg: &CompanionMsg) {
            std::io::Write::write_all(&mut self.port, &encoded(msg)).expect("write to bridge");
        }
    }

    /// Create (or replace) a symlink that points at a fake radio's PTY slave path.
    #[cfg(unix)]
    fn point_symlink_at(dir: &tempfile::TempDir, target: &str) -> std::path::PathBuf {
        let link = dir.path().join("radio");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(target, &link).expect("create pty symlink");
        link
    }

    #[cfg(unix)]
    fn pty_config(link_path: &std::path::Path) -> SerialConfig {
        SerialConfig {
            port: link_path.display().to_string(),
            baud: 115_200,
            region: "EU868".to_string(),
            profile: "MEDIUM_FAST".to_string(),
        }
    }

    #[cfg(unix)]
    fn wait_for_inbound(rx: &mut mpsc::Receiver<Inbound>, timeout: Duration) -> Option<Inbound> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(inbound) = rx.try_recv() {
                return Some(inbound);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Block until the serial worker thread has exited (it holds the only extra stats `Arc`).
    #[cfg(unix)]
    fn wait_for_worker_exit(handle: &TransportHandle) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Arc::strong_count(&handle.stats) > 1 {
            assert!(Instant::now() < deadline, "serial worker did not stop");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Drive one complete companion session: Hello -> HelloAck -> Tx -> Rx.
    #[cfg(unix)]
    fn run_session_round_trip(
        radio: &mut FakeRadio,
        handle: &TransportHandle,
        inbound_rx: &mut mpsc::Receiver<Inbound>,
        label: &str,
    ) {
        radio
            .wait_for(Duration::from_secs(5), |msg| {
                matches!(msg, CompanionMsg::Hello { proto: 1, .. })
            })
            .unwrap_or_else(|| panic!("{label}: bridge sent companion Hello"));
        radio.write_msg(&hello_ack());
        // Give the worker a moment to leave the handshake loop and enter companion mode.
        std::thread::sleep(Duration::from_millis(100));

        let frame = vec![0x42u8; 40];
        handle.send(&TransportFrame {
            bytes: frame.clone(),
            mode: openmesh_core::SendMode::Flood,
            link: None,
            exclude: None,
        });
        let sent = radio
            .wait_for(Duration::from_secs(5), |msg| {
                matches!(msg, CompanionMsg::Tx { .. })
            })
            .unwrap_or_else(|| panic!("{label}: bridge sent companion Tx"));
        match sent {
            CompanionMsg::Tx { frame: decoded } => assert_eq!(decoded, frame, "{label}"),
            other => panic!("{label}: expected Tx, got {other:?}"),
        }

        let rx_frame = vec![0x33u8; 40];
        radio.write_msg(&rx_message(rx_frame.clone()));
        let inbound = wait_for_inbound(inbound_rx, Duration::from_secs(5))
            .unwrap_or_else(|| panic!("{label}: bridge forwarded companion Rx"));
        assert_eq!(inbound.bytes, rx_frame, "{label}");
        let info = inbound.link.expect("link info");
        assert_eq!(info.link, 9);
        assert_eq!(info.rssi, Some(-71));
        assert_eq!(info.snr, Some(4.0));
    }

    #[cfg(unix)]
    #[test]
    fn companion_mode_round_trip_over_pty() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let (radio_port, slave) = serialport::TTYPort::pair().expect("create pty pair");
        let link_path = point_symlink_at(&dir, &slave.name().expect("slave pty path"));
        drop(slave);

        let (inbound_tx, mut inbound_rx) = mpsc::channel::<Inbound>(8);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = spawn(pty_config(&link_path), 9, inbound_tx, shutdown_rx)
            .expect("spawn serial transport");
        let mut radio = FakeRadio::new(radio_port);

        run_session_round_trip(&mut radio, &handle, &mut inbound_rx, "initial");
        // The 20 s keepalive does not fire in this short test; Tx/Rx mapping is already covered.
        assert_eq!(handle.stats.rx(), 1);
        assert_eq!(handle.stats.tx(), 1);

        shutdown_tx.send(true).expect("request shutdown");
        wait_for_worker_exit(&handle);
    }

    #[cfg(unix)]
    #[test]
    fn serial_transport_reconnects_after_disconnect() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let (radio1_port, slave1) = serialport::TTYPort::pair().expect("create pty pair 1");
        let link_path = point_symlink_at(&dir, &slave1.name().expect("slave 1 path"));
        drop(slave1);

        let (inbound_tx, mut inbound_rx) = mpsc::channel::<Inbound>(8);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = spawn(pty_config(&link_path), 9, inbound_tx, shutdown_rx)
            .expect("spawn serial transport");
        let mut radio1 = FakeRadio::new(radio1_port);

        run_session_round_trip(&mut radio1, &handle, &mut inbound_rx, "before reconnect");
        assert_eq!(handle.stats.rx(), 1);

        // "Unplug" the radio: point the path at a second fake radio, then close the first PTY.
        // The worker sees EOF/error, reopens the path and re-runs the companion handshake.
        let (radio2_port, slave2) = serialport::TTYPort::pair().expect("create pty pair 2");
        point_symlink_at(&dir, &slave2.name().expect("slave 2 path"));
        drop(slave2);
        drop(radio1);

        let mut radio2 = FakeRadio::new(radio2_port);
        run_session_round_trip(&mut radio2, &handle, &mut inbound_rx, "after reconnect");
        assert_eq!(
            handle.stats.rx(),
            2,
            "both sessions delivered one Rx frame each"
        );

        shutdown_tx.send(true).expect("request shutdown");
        wait_for_worker_exit(&handle);
    }

    /// A write that blocks because the radio is busy must not be treated as a dead link: the
    /// session must survive (no reconnect handshake) and keep forwarding inbound frames.
    #[cfg(unix)]
    #[test]
    fn write_backpressure_keeps_the_link_up() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let (radio_port, slave) = serialport::TTYPort::pair().expect("create pty pair");
        let link_path = point_symlink_at(&dir, &slave.name().expect("slave pty path"));
        drop(slave);

        let (inbound_tx, mut inbound_rx) = mpsc::channel::<Inbound>(8);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = spawn(pty_config(&link_path), 9, inbound_tx, shutdown_rx)
            .expect("spawn serial transport");
        let mut radio = FakeRadio::new(radio_port);

        radio
            .wait_for(Duration::from_secs(5), |msg| {
                matches!(msg, CompanionMsg::Hello { proto: 1, .. })
            })
            .expect("bridge sent companion Hello");
        radio.write_msg(&hello_ack());
        std::thread::sleep(Duration::from_millis(100));

        // Queue enough max-size frames to fill the ~12 KiB PTY buffer, then let the fake radio
        // "transmit" (not read) for longer than the write timeout + retry delay.
        for _ in 0..OUTBOUND_QUEUE {
            handle.send(&TransportFrame {
                bytes: vec![0x5A; MTU],
                mode: openmesh_core::SendMode::Flood,
                link: None,
                exclude: None,
            });
        }
        std::thread::sleep(WRITE_TIMEOUT + WRITE_RETRY_DELAY + Duration::from_millis(500));

        // Once the radio drains its buffer every frame arrives and there is no second Hello.
        let mut frames = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            frames.extend(radio.pump());
            if frames
                .iter()
                .filter(|msg| matches!(msg, CompanionMsg::Tx { .. }))
                .count()
                >= OUTBOUND_QUEUE
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let tx_count = frames
            .iter()
            .filter(|msg| matches!(msg, CompanionMsg::Tx { .. }))
            .count();
        assert!(
            tx_count > OUTBOUND_QUEUE / 2,
            "expected most queued frames after backpressure, got {tx_count}"
        );
        assert!(
            !frames
                .iter()
                .any(|msg| matches!(msg, CompanionMsg::Hello { .. })),
            "write backpressure must not trigger a reconnect"
        );

        // The session is still alive: inbound frames still reach the engine.
        let rx_frame = vec![0x44u8; 40];
        radio.write_msg(&rx_message(rx_frame.clone()));
        let inbound = wait_for_inbound(&mut inbound_rx, Duration::from_secs(5))
            .expect("session alive after write backpressure");
        assert_eq!(inbound.bytes, rx_frame);
        assert_eq!(handle.stats.rx(), 1);

        shutdown_tx.send(true).expect("request shutdown");
        wait_for_worker_exit(&handle);
    }
}

//! USB serial radio transport (opt-in via `--features real-serial`).
//!
//! Frames are SLIP-framed exactly like the firmware's companion link:
//! `0xC0` frame delimiter, `0xDB 0xDC` escapes `0xC0`, `0xDB 0xDD` escapes `0xDB`
//! (`PROTOCOL.md` §14). The serial read/write loop runs on a dedicated OS thread because
//! `serialport` is blocking; it exchanges frames with the async side over channels.

use std::io::{Read, Write};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::{mpsc, watch};

use openmesh_core::{LinkId, LinkInfo};

use super::{Inbound, TransportFrame, TransportHandle, make_link_stats};

/// SLIP end-of-frame delimiter.
pub const SLIP_END: u8 = 0xC0;
/// SLIP escape byte.
pub const SLIP_ESC: u8 = 0xDB;
/// Escaped `0xC0`.
pub const SLIP_ESC_END: u8 = 0xDC;
/// Escaped `0xDB`.
pub const SLIP_ESC_ESC: u8 = 0xDD;

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
/// the firmware's companion link parser.
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
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port: "/dev/ttyACM0".to_string(),
            baud: 115_200,
        }
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
        .timeout(Duration::from_millis(50))
        .open()
        .with_context(|| format!("opening serial port {}", config.port))?;
    let (tx, mut outbound) = mpsc::channel::<TransportFrame>(64);
    let stats = make_link_stats();
    let stats_task = stats.clone();
    let label = format!("Serial {} @{}", config.port, config.baud);
    std::thread::Builder::new()
        .name("openmesh-serial".into())
        .spawn(move || {
            let mut port = port;
            let mut decoder = SlipDecoder::new();
            let mut read_buf = [0u8; 512];
            loop {
                if *shutdown.borrow() {
                    break;
                }
                while let Ok(frame) = outbound.try_recv() {
                    if frame.link.is_some_and(|preferred| preferred != link) {
                        continue;
                    }
                    stats_task.record_tx();
                    if let Err(e) = port.write_all(&slip_encode(&frame.bytes)) {
                        tracing::warn!(error = %e, "serial write failed");
                        break;
                    }
                }
                match port.read(&mut read_buf) {
                    Ok(n) if n > 0 => {
                        for frame in decoder.push(&read_buf[..n]) {
                            if frame.len() < openmesh_core::HEADER_LEN
                                || frame.len() > openmesh_core::MTU
                            {
                                continue;
                            }
                            stats_task.record_rx(None, None);
                            let msg = Inbound {
                                bytes: frame,
                                link: Some(LinkInfo {
                                    link,
                                    peer: None,
                                    rssi: None,
                                    snr: None,
                                }),
                            };
                            if inbound.blocking_send(msg).is_err() {
                                return;
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "serial read failed");
                        break;
                    }
                }
            }
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
}

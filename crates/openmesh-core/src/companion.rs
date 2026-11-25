//! Companion link protocol: how apps talk to a radio node over USB CDC, BLE or TCP.
//!
//! Framing: `b"OM" | type: u8 | len: u16 (little endian) | payload`.
//! The companion API is intentionally small: apps do the heavy lifting (storage, UI) and the
//! radio provides frames, link stats and configuration.
//!
//! Normative reference: `docs/architecture/COMPANION-API.md`.

use crate::error::{Error, Result};

pub const COMPANION_MAGIC: [u8; 2] = *b"OM";
pub const COMPANION_HEADER_LEN: usize = 5;
/// Largest companion payload (a full OMP frame plus metadata, with headroom).
pub const COMPANION_MAX_PAYLOAD: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CompanionType {
    Hello = 1,
    HelloAck = 2,
    ConfigGet = 3,
    ConfigSet = 4,
    Config = 5,
    Tx = 6,
    Rx = 7,
    StatsGet = 8,
    Stats = 9,
    Ping = 10,
    Pong = 11,
    Event = 12,
    Reset = 13,
}

impl CompanionType {
    pub fn from_u8(v: u8) -> Result<Self> {
        Ok(match v {
            1 => Self::Hello,
            2 => Self::HelloAck,
            3 => Self::ConfigGet,
            4 => Self::ConfigSet,
            5 => Self::Config,
            6 => Self::Tx,
            7 => Self::Rx,
            8 => Self::StatsGet,
            9 => Self::Stats,
            10 => Self::Ping,
            11 => Self::Pong,
            12 => Self::Event,
            13 => Self::Reset,
            _ => return Err(Error::Malformed("unknown companion message type")),
        })
    }
}

/// Decoded companion message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompanionMsg {
    Hello { proto: u8, fw_version: String, node_id: [u8; 8] },
    HelloAck { proto: u8, fw_version: String, node_id: [u8; 8], max_frame: u16 },
    ConfigGet,
    ConfigSet { key: String, value: String },
    Config { entries: Vec<(String, String)> },
    Tx { frame: Vec<u8> },
    Rx { frame: Vec<u8>, rssi: i16, snr: i16 },
    StatsGet,
    Stats { entries: Vec<(String, u64)> },
    Ping { nonce: u32 },
    Pong { nonce: u32 },
    Event { kind: String, detail: String },
    Reset,
}

/// Encode a companion frame.
pub fn encode(msg: &CompanionMsg) -> Result<Vec<u8>> {
    let _ = msg;
    todo!("core: companion framing + CBOR payloads")
}

/// Decode a companion frame; also reports how many bytes were consumed so stream parsers can
/// handle partial reads.
pub fn decode(buf: &[u8]) -> Result<(CompanionMsg, usize)> {
    let _ = buf;
    todo!("core: companion framing + CBOR payloads")
}

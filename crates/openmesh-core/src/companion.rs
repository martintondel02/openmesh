//! Companion link protocol: how apps talk to a radio node over USB CDC, BLE or TCP.
//!
//! Normative reference: `docs/architecture/COMPANION-API.md`.
//!
//! Two framings live here, both deterministic and transport-agnostic:
//!
//! * **Chunk framing** (this is what [`encode`]/[`decode`] produce):
//!   `b"OM" | type(u8) | len(u16 LE) | payload | crc16(u16 LE)` where the CRC-16/CCITT-FALSE is
//!   computed over `type || len || payload` and the payload is CBOR.
//! * **SLIP stream framing** ([`slip_encode`]/[`slip_unescape`], and the convenience
//!   [`encode_slip`]/[`decode_slip`]): a `0xC0` delimiter before and after every chunk, with
//!   `0xC0` escaped as `0xDB 0xDC` and `0xDB` as `0xDB 0xDD`.
//!
//! [`decode`] is stream-safe: it returns the number of bytes consumed so a parser can keep the
//! remainder in its buffer, and reports [`Error::BufferTooSmall`] for a partial frame.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Magic bytes at the start of every companion chunk.
pub const COMPANION_MAGIC: [u8; 2] = *b"OM";
/// Length of the fixed chunk header (`OM` + type + u16 length).
pub const COMPANION_HEADER_LEN: usize = 5;
/// Length of the trailing CRC-16.
pub const COMPANION_CRC_LEN: usize = 2;
/// Largest companion payload (a full OMP frame plus metadata, with headroom).
pub const COMPANION_MAX_PAYLOAD: usize = 1024;
/// SLIP frame delimiter.
pub const SLIP_END: u8 = 0xC0;
/// SLIP escape byte.
pub const SLIP_ESC: u8 = 0xDB;
/// Escaped `0xC0`.
pub const SLIP_ESC_END: u8 = 0xDC;
/// Escaped `0xDB`.
pub const SLIP_ESC_ESC: u8 = 0xDD;

/// Companion message type discriminator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CompanionType {
    /// App → radio handshake.
    Hello = 1,
    /// Radio → app handshake response.
    HelloAck = 2,
    /// App → radio configuration query.
    ConfigGet = 3,
    /// App → radio configuration write (one key).
    ConfigSet = 4,
    /// Radio → app configuration snapshot.
    Config = 5,
    /// App → radio: transmit one whole OMP frame.
    Tx = 6,
    /// Radio → app: one received OMP frame plus link metadata.
    Rx = 7,
    /// App → radio statistics query.
    StatsGet = 8,
    /// Radio → app statistics snapshot.
    Stats = 9,
    /// App → radio keepalive.
    Ping = 10,
    /// Radio → app keepalive reply.
    Pong = 11,
    /// Radio → app asynchronous event.
    Event = 12,
    /// App → radio reset request (after OTA).
    Reset = 13,
}

impl CompanionType {
    /// Parse the wire type byte.
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

    /// Wire type byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Decoded companion message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompanionMsg {
    /// App → radio handshake.
    Hello {
        /// Companion protocol version requested by the app.
        proto: u8,
        /// App version string, e.g. `openmesh-android/0.1.0`.
        app_version: String,
        /// Whether the app wants asynchronous `Event` messages.
        wants_events: bool,
    },
    /// Radio → app handshake response.
    HelloAck {
        /// Companion protocol version implemented by the radio.
        proto: u8,
        /// Firmware version string.
        fw_version: String,
        /// Radio's node id.
        node_id: [u8; 8],
        /// Largest OMP frame the radio accepts in a `Tx` message.
        max_frame: u16,
        /// Advertised capability bitflags.
        caps: u16,
        /// Regulatory region name, e.g. `EU868`.
        region: String,
        /// Radio profile name, e.g. `MEDIUM_FAST`.
        profile: String,
    },
    /// App → radio configuration query.
    ConfigGet,
    /// App → radio configuration write (one key).
    ConfigSet {
        /// Configuration key, e.g. `node.name`.
        key: String,
        /// New value as a string.
        value: String,
    },
    /// Radio → app configuration snapshot.
    Config {
        /// Key/value pairs.
        entries: Vec<(String, String)>,
    },
    /// App → radio: transmit one whole OMP frame.
    Tx {
        /// Raw OMP frame bytes (header plus body).
        frame: Vec<u8>,
    },
    /// Radio → app: one received OMP frame plus link metadata.
    Rx {
        /// Raw OMP frame bytes.
        frame: Vec<u8>,
        /// RSSI in dBm.
        rssi: i16,
        /// SNR in quarter-dB units (value / 4 = dB).
        snr: i16,
        /// Transport label, e.g. `lora` or `udp`.
        link: String,
        /// Receive time in Unix milliseconds.
        at_ms: u64,
    },
    /// App → radio statistics query.
    StatsGet,
    /// Radio → app statistics snapshot.
    Stats {
        /// Named counters.
        entries: Vec<(String, u64)>,
    },
    /// App → radio keepalive.
    Ping {
        /// Opaque nonce echoed in the `Pong`.
        nonce: u32,
    },
    /// Radio → app keepalive reply.
    Pong {
        /// Nonce from the corresponding `Ping`.
        nonce: u32,
    },
    /// Radio → app asynchronous event.
    Event {
        /// Event kind, e.g. `boot`, `store_deliver`.
        kind: String,
        /// Human-readable detail.
        detail: String,
    },
    /// App → radio reset request (after OTA).
    Reset,
}

impl CompanionMsg {
    /// Message type discriminator for this message.
    pub fn message_type(&self) -> CompanionType {
        match self {
            Self::Hello { .. } => CompanionType::Hello,
            Self::HelloAck { .. } => CompanionType::HelloAck,
            Self::ConfigGet => CompanionType::ConfigGet,
            Self::ConfigSet { .. } => CompanionType::ConfigSet,
            Self::Config { .. } => CompanionType::Config,
            Self::Tx { .. } => CompanionType::Tx,
            Self::Rx { .. } => CompanionType::Rx,
            Self::StatsGet => CompanionType::StatsGet,
            Self::Stats { .. } => CompanionType::Stats,
            Self::Ping { .. } => CompanionType::Ping,
            Self::Pong { .. } => CompanionType::Pong,
            Self::Event { .. } => CompanionType::Event,
            Self::Reset => CompanionType::Reset,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// CBOR payload schemas (field names are normative; see docs/architecture/COMPANION-API.md).
// ---------------------------------------------------------------------------------------------

/// CBOR byte string helper (`[u8; N]` would serialize as a CBOR array, which is not what the
/// firmware contract specifies).
#[derive(Clone, Debug, PartialEq, Eq)]
struct WireBytes(Vec<u8>);

impl Serialize for WireBytes {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> core::result::Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for WireBytes {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> core::result::Result<Self, D::Error> {
        struct VisitorImpl;
        impl<'v> serde::de::Visitor<'v> for VisitorImpl {
            type Value = WireBytes;
            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str("a CBOR byte string")
            }
            fn visit_bytes<E: serde::de::Error>(
                self,
                v: &[u8],
            ) -> core::result::Result<Self::Value, E> {
                Ok(WireBytes(v.to_vec()))
            }
            fn visit_byte_buf<E: serde::de::Error>(
                self,
                v: Vec<u8>,
            ) -> core::result::Result<Self::Value, E> {
                Ok(WireBytes(v))
            }
            fn visit_seq<A: serde::de::SeqAccess<'v>>(
                self,
                mut seq: A,
            ) -> core::result::Result<Self::Value, A::Error> {
                let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1024));
                while let Some(b) = seq.next_element::<u8>()? {
                    out.push(b);
                }
                Ok(WireBytes(out))
            }
        }
        deserializer.deserialize_bytes(VisitorImpl)
    }
}

#[derive(Serialize, Deserialize)]
struct HelloWire {
    proto: u8,
    app_version: String,
    wants_events: bool,
}

#[derive(Serialize, Deserialize)]
struct HelloAckWire {
    proto: u8,
    fw_version: String,
    node_id: WireBytes,
    max_frame: u16,
    caps: u16,
    region: String,
    profile: String,
}

#[derive(Serialize, Deserialize)]
struct ConfigSetWire {
    key: String,
    value: String,
}

#[derive(Serialize, Deserialize)]
struct ConfigWire {
    entries: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize)]
struct TxWire {
    frame: WireBytes,
}

#[derive(Serialize, Deserialize)]
struct RxWire {
    frame: WireBytes,
    rssi: i16,
    snr: i16,
    link: String,
    at_ms: u64,
}

#[derive(Serialize, Deserialize)]
struct StatsWire {
    entries: Vec<(String, u64)>,
}

#[derive(Serialize, Deserialize)]
struct NonceWire {
    nonce: u32,
}

#[derive(Serialize, Deserialize)]
struct EventWire {
    kind: String,
    detail: String,
}

#[derive(Serialize, Deserialize, Default)]
struct EmptyWire {}

fn encode_cbor<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    ciborium::into_writer(value, &mut out)
        .map_err(|_| Error::Malformed("companion CBOR encode failed"))?;
    Ok(out)
}

fn decode_cbor<T: for<'de> Deserialize<'de>>(payload: &[u8]) -> Result<T> {
    ciborium::from_reader(payload).map_err(|e| Error::Codec(e.to_string()))
}

fn encode_payload(msg: &CompanionMsg) -> Result<Vec<u8>> {
    match msg {
        CompanionMsg::Hello {
            proto,
            app_version,
            wants_events,
        } => encode_cbor(&HelloWire {
            proto: *proto,
            app_version: app_version.clone(),
            wants_events: *wants_events,
        }),
        CompanionMsg::HelloAck {
            proto,
            fw_version,
            node_id,
            max_frame,
            caps,
            region,
            profile,
        } => encode_cbor(&HelloAckWire {
            proto: *proto,
            fw_version: fw_version.clone(),
            node_id: WireBytes(node_id.to_vec()),
            max_frame: *max_frame,
            caps: *caps,
            region: region.clone(),
            profile: profile.clone(),
        }),
        CompanionMsg::ConfigGet | CompanionMsg::StatsGet | CompanionMsg::Reset => {
            encode_cbor(&EmptyWire {})
        }
        CompanionMsg::ConfigSet { key, value } => encode_cbor(&ConfigSetWire {
            key: key.clone(),
            value: value.clone(),
        }),
        CompanionMsg::Config { entries } => encode_cbor(&ConfigWire {
            entries: entries.clone(),
        }),
        CompanionMsg::Tx { frame } => encode_cbor(&TxWire {
            frame: WireBytes(frame.clone()),
        }),
        CompanionMsg::Rx {
            frame,
            rssi,
            snr,
            link,
            at_ms,
        } => encode_cbor(&RxWire {
            frame: WireBytes(frame.clone()),
            rssi: *rssi,
            snr: *snr,
            link: link.clone(),
            at_ms: *at_ms,
        }),
        CompanionMsg::Stats { entries } => encode_cbor(&StatsWire {
            entries: entries.clone(),
        }),
        CompanionMsg::Ping { nonce } | CompanionMsg::Pong { nonce } => {
            encode_cbor(&NonceWire { nonce: *nonce })
        }
        CompanionMsg::Event { kind, detail } => encode_cbor(&EventWire {
            kind: kind.clone(),
            detail: detail.clone(),
        }),
    }
}

fn decode_payload(typ: CompanionType, payload: &[u8]) -> Result<CompanionMsg> {
    match typ {
        CompanionType::Hello => {
            let w: HelloWire = decode_cbor(payload)?;
            Ok(CompanionMsg::Hello {
                proto: w.proto,
                app_version: w.app_version,
                wants_events: w.wants_events,
            })
        }
        CompanionType::HelloAck => {
            let w: HelloAckWire = decode_cbor(payload)?;
            let node_id: [u8; 8] = w
                .node_id
                .0
                .as_slice()
                .try_into()
                .map_err(|_| Error::Malformed("hello_ack node_id must be 8 bytes"))?;
            Ok(CompanionMsg::HelloAck {
                proto: w.proto,
                fw_version: w.fw_version,
                node_id,
                max_frame: w.max_frame,
                caps: w.caps,
                region: w.region,
                profile: w.profile,
            })
        }
        CompanionType::ConfigGet => {
            let _: EmptyWire = decode_cbor(payload)?;
            Ok(CompanionMsg::ConfigGet)
        }
        CompanionType::ConfigSet => {
            let w: ConfigSetWire = decode_cbor(payload)?;
            Ok(CompanionMsg::ConfigSet {
                key: w.key,
                value: w.value,
            })
        }
        CompanionType::Config => {
            let w: ConfigWire = decode_cbor(payload)?;
            Ok(CompanionMsg::Config { entries: w.entries })
        }
        CompanionType::Tx => {
            let w: TxWire = decode_cbor(payload)?;
            Ok(CompanionMsg::Tx { frame: w.frame.0 })
        }
        CompanionType::Rx => {
            let w: RxWire = decode_cbor(payload)?;
            Ok(CompanionMsg::Rx {
                frame: w.frame.0,
                rssi: w.rssi,
                snr: w.snr,
                link: w.link,
                at_ms: w.at_ms,
            })
        }
        CompanionType::StatsGet => {
            let _: EmptyWire = decode_cbor(payload)?;
            Ok(CompanionMsg::StatsGet)
        }
        CompanionType::Stats => {
            let w: StatsWire = decode_cbor(payload)?;
            Ok(CompanionMsg::Stats { entries: w.entries })
        }
        CompanionType::Ping => {
            let w: NonceWire = decode_cbor(payload)?;
            Ok(CompanionMsg::Ping { nonce: w.nonce })
        }
        CompanionType::Pong => {
            let w: NonceWire = decode_cbor(payload)?;
            Ok(CompanionMsg::Pong { nonce: w.nonce })
        }
        CompanionType::Event => {
            let w: EventWire = decode_cbor(payload)?;
            Ok(CompanionMsg::Event {
                kind: w.kind,
                detail: w.detail,
            })
        }
        CompanionType::Reset => {
            let _: EmptyWire = decode_cbor(payload)?;
            Ok(CompanionMsg::Reset)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Framing
// ---------------------------------------------------------------------------------------------

/// CRC-16/CCITT-FALSE: polynomial 0x1021, init 0xFFFF, no reflection, xorout 0.
pub fn crc16_ccitt_false(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Encode one companion chunk: `OM | type | len LE | payload | crc16 LE`.
pub fn encode(msg: &CompanionMsg) -> Result<Vec<u8>> {
    let payload = encode_payload(msg)?;
    if payload.len() > COMPANION_MAX_PAYLOAD {
        return Err(Error::PayloadTooLarge {
            size: payload.len(),
            max: COMPANION_MAX_PAYLOAD,
        });
    }
    let typ = msg.message_type();
    let len = payload.len() as u16;
    let mut out = Vec::with_capacity(COMPANION_HEADER_LEN + payload.len() + COMPANION_CRC_LEN);
    out.extend_from_slice(&COMPANION_MAGIC);
    out.push(typ.as_u8());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&payload);
    let crc = crc16_ccitt_false(&out[2..]);
    out.extend_from_slice(&crc.to_le_bytes());
    Ok(out)
}

/// Decode one companion chunk, returning the message and the number of bytes consumed.
///
/// Partial input yields [`Error::BufferTooSmall`] so stream parsers can wait for more bytes;
/// trailing bytes after the chunk are left to the caller.
pub fn decode(buf: &[u8]) -> Result<(CompanionMsg, usize)> {
    if buf.len() < COMPANION_HEADER_LEN {
        return Err(Error::BufferTooSmall {
            need: COMPANION_HEADER_LEN,
            have: buf.len(),
        });
    }
    if buf[0..2] != COMPANION_MAGIC {
        return Err(Error::Malformed("bad companion magic"));
    }
    let typ = CompanionType::from_u8(buf[2])?;
    let len = u16::from_le_bytes([buf[3], buf[4]]) as usize;
    if len > COMPANION_MAX_PAYLOAD {
        return Err(Error::PayloadTooLarge {
            size: len,
            max: COMPANION_MAX_PAYLOAD,
        });
    }
    let total = COMPANION_HEADER_LEN + len + COMPANION_CRC_LEN;
    if buf.len() < total {
        return Err(Error::BufferTooSmall {
            need: total,
            have: buf.len(),
        });
    }
    let payload = &buf[COMPANION_HEADER_LEN..COMPANION_HEADER_LEN + len];
    let expected_crc = u16::from_le_bytes([
        buf[COMPANION_HEADER_LEN + len],
        buf[COMPANION_HEADER_LEN + len + 1],
    ]);
    let actual_crc = crc16_ccitt_false(&buf[2..COMPANION_HEADER_LEN + len]);
    if expected_crc != actual_crc {
        return Err(Error::Malformed("companion CRC mismatch"));
    }
    let msg = decode_payload(typ, payload)?;
    Ok((msg, total))
}

/// SLIP-escape `data` and wrap it in `0xC0` delimiters.
pub fn slip_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 2);
    out.push(SLIP_END);
    for &byte in data {
        match byte {
            SLIP_END => out.extend_from_slice(&[SLIP_ESC, SLIP_ESC_END]),
            SLIP_ESC => out.extend_from_slice(&[SLIP_ESC, SLIP_ESC_ESC]),
            other => out.push(other),
        }
    }
    out.push(SLIP_END);
    out
}

/// Unescape the contents of one SLIP frame (the bytes between the delimiters).
pub fn slip_unescape(escaped: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(escaped.len());
    let mut i = 0;
    while i < escaped.len() {
        let byte = escaped[i];
        if byte == SLIP_ESC {
            let Some(&next) = escaped.get(i + 1) else {
                return Err(Error::BufferTooSmall {
                    need: i + 2,
                    have: escaped.len(),
                });
            };
            match next {
                SLIP_ESC_END => out.push(SLIP_END),
                SLIP_ESC_ESC => out.push(SLIP_ESC),
                _ => return Err(Error::Malformed("invalid SLIP escape sequence")),
            }
            i += 2;
        } else {
            out.push(byte);
            i += 1;
        }
    }
    Ok(out)
}

/// Encode a companion message as a complete SLIP-delimited stream frame.
pub fn encode_slip(msg: &CompanionMsg) -> Result<Vec<u8>> {
    Ok(slip_encode(&encode(msg)?))
}

/// Decode one SLIP-delimited companion frame from a stream buffer.
///
/// Scans for the first delimiter (resynchronising over noise), decodes the enclosed chunk and
/// returns the number of bytes consumed, including the closing delimiter. Iterative, so a
/// hostile run of thousands of empty frames cannot recurse the stack.
pub fn decode_slip(buf: &[u8]) -> Result<(CompanionMsg, usize)> {
    let mut search_from = 0usize;
    loop {
        let Some(relative) = buf[search_from..].iter().position(|byte| *byte == SLIP_END) else {
            return Err(Error::BufferTooSmall {
                need: search_from + 1,
                have: buf.len(),
            });
        };
        let start = search_from + relative;
        let mut i = start + 1;
        loop {
            match buf.get(i) {
                None => {
                    return Err(Error::BufferTooSmall {
                        need: i + 1,
                        have: buf.len(),
                    });
                }
                Some(&SLIP_END) => break,
                Some(_) => i += 1,
            }
        }
        let escaped = &buf[start + 1..i];
        let consumed = i + 1;
        if escaped.is_empty() {
            // Empty frame (back-to-back delimiters): skip it and try the following frame.
            search_from = consumed;
            if search_from >= buf.len() {
                return Err(Error::BufferTooSmall {
                    need: consumed + 1,
                    have: buf.len(),
                });
            }
            continue;
        }
        let chunk = slip_unescape(escaped)?;
        let (msg, _) = decode(&chunk)?;
        return Ok((msg, consumed));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_messages() -> Vec<CompanionMsg> {
        vec![
            CompanionMsg::Hello {
                proto: 1,
                app_version: "openmesh-android/0.1.0".into(),
                wants_events: true,
            },
            CompanionMsg::HelloAck {
                proto: 1,
                fw_version: "0.1.0".into(),
                node_id: [0x11; 8],
                max_frame: 240,
                caps: 0b1001,
                region: "EU868".into(),
                profile: "MEDIUM_FAST".into(),
            },
            CompanionMsg::ConfigGet,
            CompanionMsg::ConfigSet {
                key: "node.name".into(),
                value: "ridge".into(),
            },
            CompanionMsg::Config {
                entries: vec![
                    ("node.name".into(), "ridge".into()),
                    ("mesh.ttl".into(), "5".into()),
                ],
            },
            CompanionMsg::Tx {
                frame: vec![1u8, 2, 3, 4],
            },
            CompanionMsg::Rx {
                frame: vec![0xC0u8, 0xDB, 0xAA],
                rssi: -87,
                snr: 26,
                link: "lora".into(),
                at_ms: 1_760_000_000_000,
            },
            CompanionMsg::StatsGet,
            CompanionMsg::Stats {
                entries: vec![("tx_frames".into(), 120), ("rx_frames".into(), 240)],
            },
            CompanionMsg::Ping { nonce: 0xDEAD_BEEF },
            CompanionMsg::Pong { nonce: 0xDEAD_BEEF },
            CompanionMsg::Event {
                kind: "boot".into(),
                detail: "ok".into(),
            },
            CompanionMsg::Reset,
        ]
    }

    #[test]
    fn every_message_roundtrips() {
        for msg in all_messages() {
            let bytes = encode(&msg).unwrap();
            assert_eq!(&bytes[0..2], b"OM");
            assert_eq!(bytes[2], msg.message_type().as_u8());
            let (decoded, consumed) = decode(&bytes).unwrap();
            assert_eq!(decoded, msg);
            assert_eq!(consumed, bytes.len());
        }
    }

    #[test]
    fn cbor_payloads_use_expected_keys() {
        // Hello payload is a definite-length CBOR map with proto/app_version/wants_events.
        let hello = CompanionMsg::Hello {
            proto: 1,
            app_version: "app/1".into(),
            wants_events: true,
        };
        let bytes = encode(&hello).unwrap();
        let payload = &bytes[5..bytes.len() - 2];
        let value: ciborium::value::Value = ciborium::from_reader(payload).unwrap();
        let map = value.as_map().expect("hello payload must be a CBOR map");
        let keys: Vec<&str> = map.iter().filter_map(|(k, _)| k.as_text()).collect();
        assert!(keys.contains(&"proto"));
        assert!(keys.contains(&"app_version"));
        assert!(keys.contains(&"wants_events"));
        // No-payload commands encode as an empty CBOR map (0xa0).
        let cfg = encode(&CompanionMsg::ConfigGet).unwrap();
        assert_eq!(&cfg[5..cfg.len() - 2], &[0xa0]);
    }

    #[test]
    fn crc_catches_corruption() {
        let mut bytes = encode(&CompanionMsg::Ping { nonce: 7 }).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        assert!(matches!(decode(&bytes), Err(Error::Malformed(_))));
        // Corrupting the payload is also caught.
        let mut bytes = encode(&CompanionMsg::Ping { nonce: 7 }).unwrap();
        bytes[6] ^= 0xFF;
        assert!(matches!(decode(&bytes), Err(Error::Malformed(_))));
    }

    #[test]
    fn crc16_known_answer() {
        // CRC-16/CCITT-FALSE("123456789") = 0x29B1.
        assert_eq!(crc16_ccitt_false(b"123456789"), 0x29B1);
        assert_eq!(crc16_ccitt_false(b""), 0xFFFF);
    }

    #[test]
    fn decode_is_partial_stream_safe() {
        let bytes = encode(&CompanionMsg::Tx {
            frame: vec![9u8; 40],
        })
        .unwrap();
        for cut in 0..bytes.len() {
            match decode(&bytes[..cut]) {
                Err(Error::BufferTooSmall { .. }) => {}
                other => panic!("cut {cut} should be incomplete, got {other:?}"),
            }
        }
        // Trailing bytes are not consumed.
        let mut with_trailing = bytes.clone();
        with_trailing.extend_from_slice(&[0xAA; 5]);
        let (msg, consumed) = decode(&with_trailing).unwrap();
        assert_eq!(
            msg,
            CompanionMsg::Tx {
                frame: vec![9u8; 40]
            }
        );
        assert_eq!(consumed, bytes.len());
    }

    #[test]
    fn rejects_bad_magic_type_and_oversize() {
        let mut bytes = encode(&CompanionMsg::Reset).unwrap();
        bytes[0] = b'X';
        assert!(matches!(decode(&bytes), Err(Error::Malformed(_))));
        let mut bytes = encode(&CompanionMsg::Reset).unwrap();
        bytes[2] = 99;
        assert!(decode(&bytes).is_err());
        // A frame declaring more than the maximum payload is rejected before allocation.
        let mut bytes = encode(&CompanionMsg::Reset).unwrap();
        bytes[3..5].copy_from_slice(&(COMPANION_MAX_PAYLOAD as u16 + 1).to_le_bytes());
        assert!(matches!(decode(&bytes), Err(Error::PayloadTooLarge { .. })));
    }

    #[test]
    fn slip_roundtrip_and_escaping() {
        let msg = CompanionMsg::Rx {
            frame: vec![0xC0u8, 0xDB, 0xC0, 0xDB, 0x00],
            rssi: -100,
            snr: 8,
            link: "ble".into(),
            at_ms: 1,
        };
        let stream = encode_slip(&msg).unwrap();
        assert_eq!(stream.first(), Some(&SLIP_END));
        assert_eq!(stream.last(), Some(&SLIP_END));
        // Delimiters inside the payload must be escaped.
        assert!(!stream[1..stream.len() - 1].contains(&SLIP_END));
        let (decoded, consumed) = decode_slip(&stream).unwrap();
        assert_eq!(decoded, msg);
        assert_eq!(consumed, stream.len());
    }

    #[test]
    fn slip_decode_resyncs_and_handles_partial() {
        let a = encode_slip(&CompanionMsg::Ping { nonce: 1 }).unwrap();
        let b = encode_slip(&CompanionMsg::Pong { nonce: 2 }).unwrap();
        // Noise before the first delimiter is skipped (scan resync).
        let mut buf = vec![0x00u8, 0x11, 0x22];
        buf.extend_from_slice(&a);
        buf.extend_from_slice(&b);
        let (msg, consumed) = decode_slip(&buf).unwrap();
        assert_eq!(msg, CompanionMsg::Ping { nonce: 1 });
        assert_eq!(consumed, 3 + a.len());
        let (msg2, _) = decode_slip(&buf[consumed..]).unwrap();
        assert_eq!(msg2, CompanionMsg::Pong { nonce: 2 });
        // Partial frames report BufferTooSmall.
        assert!(matches!(
            decode_slip(&buf[consumed..consumed + 2]),
            Err(Error::BufferTooSmall { .. })
        ));
        // An invalid escape inside a frame is rejected.
        let bad = slip_encode(&[b'O', b'M', 1, 0, 0, SLIP_ESC, 0x00]);
        assert!(matches!(decode_slip(&bad), Err(Error::Malformed(_))));
    }
}

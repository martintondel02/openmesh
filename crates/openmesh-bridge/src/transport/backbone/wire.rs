//! OBB/1 wire layer: TCP framing, control messages (CBOR), authentication and connection strings.
//!
//! Normative reference: `docs/architecture/BACKBONE.md` §2, §3, §6.
//!
//! ```text
//! message := u16 BE length L (1..=4200) || payload[L]
//! payload[0] := kind: 0x00 control (CBOR map) | 0x01 OMP/1 frame
//! ```

use std::io;
use std::time::Duration;

use base64::Engine as _;
use rand::RngCore;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use openmesh_core::NodeId;

/// Protocol string in the `hello` control message.
pub const OBB_PROTO: &str = "obb/1";
/// Protocol version in the `hello` control message.
pub const OBB_VERSION: u16 = 1;
/// Maximum OBB message size (kind byte + payload).
pub const MAX_OBB_MESSAGE: usize = 4200;
/// Control message kind.
pub const KIND_CONTROL: u8 = 0;
/// One complete OMP/1 frame.
pub const KIND_FRAME: u8 = 1;
/// Domain separation prefix for the handshake HMAC.
pub const AUTH_DOMAIN: &[u8] = b"openmesh/backbone/v1/auth";
/// Features advertised in `welcome`.
pub const FEATURES: &[&str] = &["frames", "stats", "store_forward", "panel"];

/// Errors on a backbone wire/session path.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("auth failed: {0}")]
    AuthFailed(String),
    #[error("timeout")]
    Timeout,
}

pub type WireResult<T> = Result<T, WireError>;

fn unexpected_eof() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed by peer")
}

// ---------------------------------------------------------------------------------------------
// CBOR byte-string helpers (CBOR `bytes`, not arrays)
// ---------------------------------------------------------------------------------------------

struct BytesRef<'a>(&'a [u8]);

impl Serialize for BytesRef<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self.0)
    }
}

struct BytesOwned(Vec<u8>);

struct BytesVisitor;

impl<'de> Visitor<'de> for BytesVisitor {
    type Value = BytesOwned;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a CBOR byte string")
    }

    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
        Ok(BytesOwned(v.to_vec()))
    }

    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
        Ok(BytesOwned(v))
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut out = Vec::new();
        while let Some(byte) = seq.next_element::<u8>()? {
            out.push(byte);
        }
        Ok(BytesOwned(out))
    }
}

impl<'de> Deserialize<'de> for BytesOwned {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_byte_buf(BytesVisitor)
    }
}

mod wire_bytes {
    use super::*;

    #[allow(clippy::ptr_arg)] // serde `with` passes `&Vec<u8>` for `Vec<u8>` fields
    pub fn serialize<S: Serializer>(value: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        BytesOwned::deserialize(deserializer).map(|b| b.0)
    }
}

mod opt_wire_bytes {
    use super::*;

    pub fn serialize<S: Serializer>(
        value: &Option<Vec<u8>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(bytes) => serializer.serialize_some(&BytesRef(bytes)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<u8>>, D::Error> {
        struct OptionalVisitor;
        impl<'de> Visitor<'de> for OptionalVisitor {
            type Value = Option<Vec<u8>>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a CBOR byte string or null")
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(None)
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(None)
            }
            fn visit_some<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                BytesOwned::deserialize(deserializer).map(|b| Some(b.0))
            }
        }
        deserializer.deserialize_option(OptionalVisitor)
    }
}

// ---------------------------------------------------------------------------------------------
// Control messages
// ---------------------------------------------------------------------------------------------

/// Auth mode advertised in `challenge`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    /// HMAC-SHA256 proof over a shared token.
    #[serde(rename = "hmac-sha256")]
    Token,
    /// No authentication (trusted LAN only).
    Open,
}

impl AuthMode {
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Token => "hmac-sha256",
            Self::Open => "open",
        }
    }

    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "hmac-sha256" => Some(Self::Token),
            "open" => Some(Self::Open),
            _ => None,
        }
    }
}

/// One OBB/1 control message (`kind 0x00`, canonical CBOR map with a `type` key).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlMsg {
    Hello {
        proto: String,
        v: u16,
        #[serde(with = "wire_bytes")]
        node_id: Vec<u8>,
        app: String,
        #[serde(with = "wire_bytes")]
        nonce: Vec<u8>,
        caps: u16,
    },
    Challenge {
        #[serde(with = "wire_bytes")]
        node_id: Vec<u8>,
        #[serde(with = "wire_bytes")]
        nonce: Vec<u8>,
        auth: String,
        server: String,
    },
    Auth {
        #[serde(with = "opt_wire_bytes")]
        mac: Option<Vec<u8>>,
    },
    Welcome {
        session: u32,
        #[serde(with = "wire_bytes")]
        node_id: Vec<u8>,
        name: String,
        features: Vec<String>,
        ping_interval_s: u16,
        max_frame: u16,
        peers_connected: u16,
    },
    AuthFailed {
        reason: String,
    },
    Ping {
        nonce: u32,
    },
    Pong {
        nonce: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rtt_ms: Option<u16>,
    },
    Stats {
        peers: u16,
        frames_in: u64,
        frames_out: u64,
        bytes_in: u64,
        bytes_out: u64,
        uptime_s: u64,
    },
    Close {
        reason: String,
    },
    Error {
        code: String,
        message: String,
    },
}

/// A decoded OBB/1 envelope.
#[derive(Clone, Debug)]
pub enum WireMessage {
    Control(ControlMsg),
    Frame(Vec<u8>),
    /// Unknown kind values are ignored; the length prefix keeps the stream in sync.
    Unknown(u8),
}

// ---------------------------------------------------------------------------------------------
// Framing
// ---------------------------------------------------------------------------------------------

/// Encode and write one control message.
pub async fn write_control<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &ControlMsg,
) -> WireResult<()> {
    let mut cbor = Vec::new();
    ciborium::into_writer(msg, &mut cbor)
        .map_err(|e| WireError::Protocol(format!("cbor encode: {e}")))?;
    write_message(writer, KIND_CONTROL, &cbor).await
}

/// Encode and write one OMP/1 frame as a `kind 0x01` message.
pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, frame: &[u8]) -> WireResult<()> {
    write_message(writer, KIND_FRAME, frame).await
}

async fn write_message<W: AsyncWrite + Unpin>(
    writer: &mut W,
    kind: u8,
    payload: &[u8],
) -> WireResult<()> {
    let len = payload.len() + 1;
    if len == 0 || len > MAX_OBB_MESSAGE {
        return Err(WireError::Protocol(format!("message too large: {len}")));
    }
    let mut buf = Vec::with_capacity(2 + len);
    buf.extend_from_slice(&(len as u16).to_be_bytes());
    buf.push(kind);
    buf.extend_from_slice(payload);
    writer.write_all(&buf).await?;
    writer.flush().await?;
    Ok(())
}

/// Read one OBB/1 message. Returns `None` on a clean EOF.
pub async fn read_message<R: AsyncRead + Unpin>(reader: &mut R) -> WireResult<Option<WireMessage>> {
    let mut header = [0u8; 2];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(WireError::Io(e)),
    }
    let len = u16::from_be_bytes(header) as usize;
    if len == 0 || len > MAX_OBB_MESSAGE {
        return Err(WireError::Protocol(format!(
            "invalid OBB message length {len}"
        )));
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await.map_err(|e| {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            WireError::Io(unexpected_eof())
        } else {
            WireError::Io(e)
        }
    })?;
    match payload[0] {
        KIND_CONTROL => {
            let msg: ControlMsg = ciborium::from_reader(&payload[1..])
                .map_err(|e| WireError::Protocol(format!("control cbor: {e}")))?;
            Ok(Some(WireMessage::Control(msg)))
        }
        KIND_FRAME => Ok(Some(WireMessage::Frame(payload[1..].to_vec()))),
        other => Ok(Some(WireMessage::Unknown(other))),
    }
}

async fn read_control_step<R: AsyncRead + Unpin>(
    reader: &mut R,
    timeout: Duration,
) -> WireResult<ControlMsg> {
    match tokio::time::timeout(timeout, read_message(reader)).await {
        Err(_) => Err(WireError::Timeout),
        Ok(Err(e)) => Err(e),
        Ok(Ok(None)) => Err(WireError::Io(unexpected_eof())),
        Ok(Ok(Some(WireMessage::Control(msg)))) => Ok(msg),
        Ok(Ok(Some(_))) => Err(WireError::Protocol(
            "expected a control message during handshake".into(),
        )),
    }
}

// ---------------------------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------------------------

/// Decode a base64url (no padding) token into its 32 raw key bytes.
pub fn token_key(token: &str) -> WireResult<[u8; 32]> {
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token.trim())
        .map_err(|_| WireError::Protocol("token is not valid base64url".into()))?;
    <[u8; 32]>::try_from(raw.as_slice())
        .map_err(|_| WireError::Protocol("token must decode to 32 bytes".into()))
}

/// Generate a fresh base64url (no padding) 32-byte token.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// HMAC-SHA256 over the handshake transcript (`BACKBONE.md` §3.2).
pub fn auth_mac(
    token: &[u8; 32],
    client_nonce: &[u8; 16],
    server_nonce: &[u8; 16],
    client_id: NodeId,
    server_id: NodeId,
) -> [u8; 32] {
    let mut data = Vec::with_capacity(AUTH_DOMAIN.len() + 16 + 16 + 8 + 8);
    data.extend_from_slice(AUTH_DOMAIN);
    data.extend_from_slice(client_nonce);
    data.extend_from_slice(server_nonce);
    data.extend_from_slice(&client_id.0);
    data.extend_from_slice(&server_id.0);
    hmac_sha256(token, &data)
}

/// Constant-time byte comparison.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// HMAC-SHA256 (RFC 2104), hand-rolled over `sha2` so the bridge needs no extra crypto crate.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut key_block = [0u8; BLOCK];
    if key.len() > BLOCK {
        let digest: [u8; 32] = Sha256::digest(key).into();
        key_block[..32].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0x36u8; BLOCK];
    let mut outer_pad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        inner_pad[i] ^= key_block[i];
        outer_pad[i] ^= key_block[i];
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(data);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_hash);
    outer.finalize().into()
}

// ---------------------------------------------------------------------------------------------
// Handshake
// ---------------------------------------------------------------------------------------------

fn node_id_from_bytes(bytes: &[u8]) -> WireResult<NodeId> {
    let arr: [u8; 8] = bytes
        .try_into()
        .map_err(|_| WireError::Protocol("node_id must be 8 bytes".into()))?;
    Ok(NodeId(arr))
}

fn nonce_from_bytes(bytes: &[u8]) -> WireResult<[u8; 16]> {
    bytes
        .try_into()
        .map_err(|_| WireError::Protocol("nonce must be 16 bytes".into()))
}

/// Client-side handshake parameters.
pub struct ClientHandshakeParams<'a> {
    pub node_id: NodeId,
    pub app: &'a str,
    pub caps: u16,
    /// Token key bytes; `None` is only valid against `auth:"open"` servers.
    pub token: Option<[u8; 32]>,
    pub timeout: Duration,
}

/// Handshake result from the server's point of view.
pub struct ServerHandshakeInfo {
    pub node_id: NodeId,
    pub app: String,
    pub caps: u16,
    pub auth: &'static str,
}

/// `welcome` contents seen by a client.
#[derive(Clone, Debug)]
pub struct WelcomeInfo {
    pub session: u32,
    pub node_id: NodeId,
    pub name: String,
    pub features: Vec<String>,
    pub ping_interval_s: u16,
    pub max_frame: u16,
    pub peers_connected: u16,
}

/// Server-side handshake parameters.
pub struct ServerHandshakeParams<'a> {
    pub node_id: NodeId,
    pub server_name: &'a str,
    pub auth: AuthMode,
    pub token: Option<[u8; 32]>,
    pub allowlist: &'a [NodeId],
    pub max_peers: usize,
    pub peers_connected: usize,
    pub session: u32,
    pub timeout: Duration,
}

/// Run the server half of the OBB/1 handshake. On auth rejection the peer receives
/// `auth_failed` with the documented reason before this returns [`WireError::AuthFailed`].
pub async fn server_handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    params: &ServerHandshakeParams<'_>,
) -> WireResult<ServerHandshakeInfo> {
    let hello = read_control_step(stream, params.timeout).await?;
    let (client_id, client_nonce, app, caps) = match hello {
        ControlMsg::Hello {
            proto,
            v,
            node_id,
            app,
            nonce,
            caps,
        } => {
            if proto != OBB_PROTO || v != OBB_VERSION {
                send_auth_failed(stream, "server_full").await;
                return Err(WireError::AuthFailed("server_full".into()));
            }
            let node_id = match node_id_from_bytes(&node_id) {
                Ok(node_id) => node_id,
                Err(_) => {
                    send_auth_failed(stream, "server_full").await;
                    return Err(WireError::AuthFailed("server_full".into()));
                }
            };
            let nonce = match nonce_from_bytes(&nonce) {
                Ok(nonce) => nonce,
                Err(_) => {
                    send_auth_failed(stream, "server_full").await;
                    return Err(WireError::AuthFailed("server_full".into()));
                }
            };
            (node_id, nonce, app, caps)
        }
        other => {
            return Err(WireError::Protocol(format!(
                "expected hello, got {other:?}"
            )));
        }
    };

    // Capacity and allowlist checks happen before the challenge so the client can report a
    // precise `auth_failed` reason.
    if params.peers_connected >= params.max_peers {
        send_auth_failed(stream, "max_peers").await;
        return Err(WireError::AuthFailed("max_peers".into()));
    }
    if !params.allowlist.is_empty() && !params.allowlist.contains(&client_id) {
        send_auth_failed(stream, "not_allowlisted").await;
        return Err(WireError::AuthFailed("not_allowlisted".into()));
    }

    let mut server_nonce = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut server_nonce);
    write_control(
        stream,
        &ControlMsg::Challenge {
            node_id: params.node_id.0.to_vec(),
            nonce: server_nonce.to_vec(),
            auth: params.auth.as_wire().to_string(),
            server: params.server_name.to_string(),
        },
    )
    .await?;

    let auth = read_control_step(stream, params.timeout).await?;
    let mac = match auth {
        ControlMsg::Auth { mac } => mac,
        other => return Err(WireError::Protocol(format!("expected auth, got {other:?}"))),
    };
    let auth_ok = match params.auth {
        AuthMode::Open => true,
        AuthMode::Token => {
            let Some(token) = params.token else {
                send_auth_failed(stream, "bad_token").await;
                return Err(WireError::AuthFailed("bad_token".into()));
            };
            let expected = auth_mac(
                &token,
                &client_nonce,
                &server_nonce,
                client_id,
                params.node_id,
            );
            match mac {
                Some(mac) => constant_time_eq(&mac, &expected),
                None => false,
            }
        }
    };
    if !auth_ok {
        send_auth_failed(stream, "bad_token").await;
        return Err(WireError::AuthFailed("bad_token".into()));
    }

    write_control(
        stream,
        &ControlMsg::Welcome {
            session: params.session,
            node_id: params.node_id.0.to_vec(),
            name: params.server_name.to_string(),
            features: FEATURES.iter().map(|f| f.to_string()).collect(),
            ping_interval_s: 30,
            max_frame: MAX_OBB_MESSAGE as u16,
            peers_connected: (params.peers_connected + 1).min(u16::MAX as usize) as u16,
        },
    )
    .await?;
    Ok(ServerHandshakeInfo {
        node_id: client_id,
        app,
        caps,
        auth: if params.auth == AuthMode::Open {
            "open"
        } else {
            "token"
        },
    })
}

async fn send_auth_failed<S: AsyncWrite + Unpin>(stream: &mut S, reason: &str) {
    let _ = write_control(
        stream,
        &ControlMsg::AuthFailed {
            reason: reason.to_string(),
        },
    )
    .await;
}

/// Run the client half of the OBB/1 handshake.
pub async fn client_handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    params: &ClientHandshakeParams<'_>,
) -> WireResult<WelcomeInfo> {
    let mut client_nonce = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut client_nonce);
    write_control(
        stream,
        &ControlMsg::Hello {
            proto: OBB_PROTO.to_string(),
            v: OBB_VERSION,
            node_id: params.node_id.0.to_vec(),
            app: params.app.to_string(),
            nonce: client_nonce.to_vec(),
            caps: params.caps,
        },
    )
    .await?;

    let challenge = read_control_step(stream, params.timeout).await?;
    let (server_id, server_nonce, auth_mode) = match challenge {
        ControlMsg::Challenge {
            node_id,
            nonce,
            auth,
            ..
        } => {
            let server_id = node_id_from_bytes(&node_id)?;
            let server_nonce = nonce_from_bytes(&nonce)?;
            let mode = AuthMode::from_wire(&auth)
                .ok_or_else(|| WireError::Protocol(format!("unknown auth mode {auth:?}")))?;
            (server_id, server_nonce, mode)
        }
        ControlMsg::AuthFailed { reason } => return Err(WireError::AuthFailed(reason)),
        ControlMsg::Error { code, message } => {
            return Err(WireError::Protocol(format!(
                "server error {code}: {message}"
            )));
        }
        other => {
            return Err(WireError::Protocol(format!(
                "expected challenge, got {other:?}"
            )));
        }
    };

    match auth_mode {
        AuthMode::Open => {
            write_control(stream, &ControlMsg::Auth { mac: None }).await?;
        }
        AuthMode::Token => {
            let mac = params.token.map(|token| {
                auth_mac(
                    &token,
                    &client_nonce,
                    &server_nonce,
                    params.node_id,
                    server_id,
                )
                .to_vec()
            });
            write_control(stream, &ControlMsg::Auth { mac }).await?;
        }
    }

    match read_control_step(stream, params.timeout).await? {
        ControlMsg::Welcome {
            session,
            node_id,
            name,
            features,
            ping_interval_s,
            max_frame,
            peers_connected,
        } => Ok(WelcomeInfo {
            session,
            node_id: node_id_from_bytes(&node_id)?,
            name,
            features,
            ping_interval_s,
            max_frame,
            peers_connected,
        }),
        ControlMsg::AuthFailed { reason } => Err(WireError::AuthFailed(reason)),
        ControlMsg::Error { code, message } => Err(WireError::Protocol(format!(
            "server error {code}: {message}"
        ))),
        other => Err(WireError::Protocol(format!(
            "expected welcome, got {other:?}"
        ))),
    }
}

// ---------------------------------------------------------------------------------------------
// Connection strings
// ---------------------------------------------------------------------------------------------

/// Parsed `openmesh://backbone?...` connection string (`BACKBONE.md` §6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionString {
    pub host: String,
    pub port: u16,
    pub token: Option<String>,
    pub node: Option<NodeId>,
    pub name: Option<String>,
}

impl ConnectionString {
    /// `host:port` dial target.
    pub fn authority(&self) -> String {
        if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Build a shareable `openmesh://backbone?...` connection string.
pub fn format_connection_string(connection: &ConnectionString) -> String {
    let mut out = format!(
        "openmesh://backbone?host={}&port={}",
        percent_encode(&connection.host),
        connection.port
    );
    if let Some(token) = &connection.token {
        out.push_str(&format!("&token={token}"));
    }
    if let Some(node) = connection.node {
        out.push_str(&format!("&node={}", node.to_hex()));
    }
    if let Some(name) = &connection.name {
        out.push_str(&format!("&name={}", percent_encode(name)));
    }
    out.push_str("&v=1");
    out
}

/// Parse a connection string. Accepts the `openmesh://backbone?...` form only.
pub fn parse_connection_string(input: &str) -> Result<ConnectionString, String> {
    let trimmed = input.trim();
    let rest = trimmed
        .strip_prefix("openmesh://backbone?")
        .or_else(|| trimmed.strip_prefix("openmesh://backbone/?"))
        .ok_or_else(|| "expected openmesh://backbone?host=... connection string".to_string())?;
    let mut host = None;
    let mut port = None;
    let mut token = None;
    let mut node = None;
    let mut name = None;
    for pair in rest.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = percent_decode(value);
        match key {
            "host" => host = Some(value),
            "port" => {
                port = Some(
                    value
                        .parse::<u16>()
                        .map_err(|_| format!("invalid port {value:?}"))?,
                )
            }
            "token" => token = Some(value),
            "node" => {
                node = Some(
                    NodeId::from_hex(&value).map_err(|_| format!("invalid node id {value:?}"))?,
                )
            }
            "name" => name = Some(value),
            _ => {}
        }
    }
    let host = host.ok_or_else(|| "connection string is missing host".to_string())?;
    let port = port.ok_or_else(|| "connection string is missing port".to_string())?;
    Ok(ConnectionString {
        host,
        port,
        token,
        node,
        name,
    })
}

/// Render a QR code of `data` as an SVG document.
pub fn qr_svg(data: &str) -> Result<String, String> {
    use qrcodegen::{QrCode, QrCodeEcc};
    let qr = QrCode::encode_text(data, QrCodeEcc::Medium).map_err(|e| e.to_string())?;
    let size = qr.size();
    let border = 2i32;
    let total = size + border * 2;
    let mut svg = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" viewBox=\"0 0 {total} {total}\" width=\"{px}\" height=\"{px}\" shape-rendering=\"crispEdges\">\n<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n<path fill=\"#000000\" d=\"",
        px = total * 8
    );
    for y in 0..size {
        for x in 0..size {
            if qr.get_module(x, y) {
                svg.push_str(&format!("M{},{}h1v1h-1z", x + border, y + border));
            }
        }
    }
    svg.push_str("\"/>\n</svg>\n");
    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_rfc4231_vector() {
        // RFC 4231 test case 2.
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex::encode(mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn auth_mac_binds_nonce_and_ids() {
        let token = [7u8; 32];
        let cn = [1u8; 16];
        let sn = [2u8; 16];
        let a = NodeId::new([3; 8]);
        let b = NodeId::new([4; 8]);
        let mac = auth_mac(&token, &cn, &sn, a, b);
        assert_eq!(mac, auth_mac(&token, &cn, &sn, a, b));
        assert_ne!(mac, auth_mac(&token, &cn, &sn, b, a));
        assert_ne!(mac, auth_mac(&token, &[9u8; 16], &sn, a, b));
        assert!(constant_time_eq(&mac, &mac));
        assert!(!constant_time_eq(&mac, &mac[..31]));
    }

    #[test]
    fn control_messages_roundtrip_cbor() {
        let messages = [
            ControlMsg::Hello {
                proto: OBB_PROTO.into(),
                v: 1,
                node_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
                app: "openmesh-bridge/0.2.0".into(),
                nonce: vec![9u8; 16],
                caps: 0x0021,
            },
            ControlMsg::Challenge {
                node_id: vec![1; 8],
                nonce: vec![2; 16],
                auth: "hmac-sha256".into(),
                server: "oslo-backbone".into(),
            },
            ControlMsg::Auth {
                mac: Some(vec![3; 32]),
            },
            ControlMsg::Auth { mac: None },
            ControlMsg::Welcome {
                session: 42,
                node_id: vec![4; 8],
                name: "oslo-backbone".into(),
                features: FEATURES.iter().map(|f| f.to_string()).collect(),
                ping_interval_s: 30,
                max_frame: 4200,
                peers_connected: 2,
            },
            ControlMsg::AuthFailed {
                reason: "bad_token".into(),
            },
            ControlMsg::Ping { nonce: 7 },
            ControlMsg::Pong {
                nonce: 7,
                rtt_ms: Some(12),
            },
            ControlMsg::Stats {
                peers: 1,
                frames_in: 2,
                frames_out: 3,
                bytes_in: 4,
                bytes_out: 5,
                uptime_s: 6,
            },
            ControlMsg::Close {
                reason: "kicked".into(),
            },
            ControlMsg::Error {
                code: "x".into(),
                message: "y".into(),
            },
        ];
        for message in messages {
            let mut encoded = Vec::new();
            ciborium::into_writer(&message, &mut encoded).unwrap();
            let decoded: ControlMsg = ciborium::from_reader(encoded.as_slice()).unwrap();
            let reencoded = {
                let mut buf = Vec::new();
                ciborium::into_writer(&decoded, &mut buf).unwrap();
                buf
            };
            assert_eq!(encoded, reencoded, "control round-trip must be canonical");
        }
    }

    #[test]
    fn control_node_ids_serialize_as_cbor_bytes() {
        let message = ControlMsg::Hello {
            proto: OBB_PROTO.into(),
            v: 1,
            node_id: vec![0xAA; 8],
            app: "x".into(),
            nonce: vec![0xBB; 16],
            caps: 0,
        };
        let mut encoded = Vec::new();
        ciborium::into_writer(&message, &mut encoded).unwrap();
        // CBOR byte strings start with major type 2: 0x48 for 8 bytes, 0x50 for 16 bytes.
        assert!(encoded.windows(2).any(|w| w == [0x48, 0xAA]));
        assert!(encoded.windows(2).any(|w| w == [0x50, 0xBB]));
    }

    #[tokio::test]
    async fn framing_roundtrip_and_limits() {
        let mut buf = Vec::new();
        let frame = vec![9u8; 100];
        write_frame(&mut buf, &frame).await.unwrap();
        let mut slice = buf.as_slice();
        match read_message(&mut slice).await.unwrap().unwrap() {
            WireMessage::Frame(decoded) => assert_eq!(decoded, frame),
            other => panic!("unexpected {other:?}"),
        }

        let mut empty = &[0u8, 0][..];
        assert!(matches!(
            read_message(&mut empty).await,
            Err(WireError::Protocol(_))
        ));

        let mut too_large = &[0x10, 0x69][..]; // 4201
        assert!(matches!(
            read_message(&mut too_large).await,
            Err(WireError::Protocol(_))
        ));

        let mut eof = &[][..];
        assert!(read_message(&mut eof).await.unwrap().is_none());
    }

    #[test]
    fn connection_string_roundtrip() {
        let connection = ConnectionString {
            host: "mesh.example.org".into(),
            port: 9443,
            token: Some(generate_token()),
            node: Some(NodeId::new([1, 2, 3, 4, 5, 6, 7, 8])),
            name: Some("oslo backbone".into()),
        };
        let text = format_connection_string(&connection);
        assert!(text.starts_with("openmesh://backbone?host=mesh.example.org&port=9443"));
        assert!(text.contains("name=oslo%20backbone"));
        assert_eq!(parse_connection_string(&text).unwrap(), connection);
        assert!(parse_connection_string("http://example.com").is_err());
    }

    #[test]
    fn token_generation_and_key_decoding() {
        let token = generate_token();
        assert_eq!(token.len(), 43);
        assert!(token_key(&token).is_ok());
        assert!(token_key("short").is_err());
    }

    #[test]
    fn qr_svg_is_generated() {
        let svg = qr_svg("openmesh://backbone?host=x&port=1&token=abc").unwrap();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("<path fill=\"#000000\""));
        assert!(svg.len() > 500);
    }
}

//! OpenMesh Backbone mode (OBB/1): authenticated TCP relay server + outbound join clients.
//!
//! Normative contract: `docs/architecture/BACKBONE.md`. The manager owns:
//! * the persisted server configuration (`<data-dir>/backbone.json`, mode 0600),
//! * the TCP listener lifecycle (live restart on bind/port/enabled changes),
//! * connected peer sessions and their accounting,
//! * the outbound `join` client with reconnect/backoff,
//! * the JSON shapes used by REST (`/api/backbone*`) and WebSocket (`backbone_status`,
//!   `backbone_peer`),
//! * the onboarding setup payload and its QR code.
//!
//! Frames received from peers are injected into the normal protocol engine exactly like frames
//! from any other transport; engine output is fanned out to peers (except the origin) plus the
//! other local transports by the core runtime.

pub mod client;
pub mod server;
pub mod wire;

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::{Mutex, broadcast, mpsc, watch};

use openmesh_core::{LinkId, LinkInfo, NodeId, SendMode, util};

use crate::json::event_json;
use crate::runtime::{ClientCommand, CommandKind, EngineInput, broadcast_event};
use crate::transport::{Inbound, LinkStats, TransportFrame, TransportHandle, make_link_stats};

pub use wire::{
    AuthMode, ConnectionString, ControlMsg, WireError, WireMessage, format_connection_string,
    parse_connection_string, qr_svg,
};

// ---------------------------------------------------------------------------------------------
// Configuration (`BACKBONE.md` §5)
// ---------------------------------------------------------------------------------------------

fn default_bind() -> String {
    "0.0.0.0".to_string()
}
fn default_port() -> u16 {
    9443
}
fn default_auth() -> String {
    "token".to_string()
}
fn default_max_peers() -> usize {
    32
}
fn default_backbone_name() -> String {
    "openmesh-backbone".to_string()
}
fn default_ping_interval() -> u16 {
    30
}
fn default_stats_push() -> u16 {
    30
}
fn default_true() -> bool {
    true
}

/// Persisted backbone server configuration. The token is a base64url (no padding) encoding of
/// 32 random bytes; it is never returned by read APIs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackboneConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_auth")]
    pub auth: String,
    #[serde(default)]
    pub token: String,
    #[serde(default = "default_max_peers")]
    pub max_peers: usize,
    /// Hex node ids allowed to complete the handshake; empty means "anyone with the token".
    #[serde(default)]
    pub allowlist: Vec<String>,
    #[serde(default = "default_true")]
    pub store_forward: bool,
    #[serde(default = "default_backbone_name")]
    pub name: String,
    #[serde(default)]
    pub public_host: Option<String>,
    #[serde(default = "default_ping_interval")]
    pub ping_interval_s: u16,
    #[serde(default = "default_stats_push")]
    pub stats_push_s: u16,
}

impl Default for BackboneConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: default_bind(),
            port: default_port(),
            auth: default_auth(),
            token: String::new(),
            max_peers: default_max_peers(),
            allowlist: Vec::new(),
            store_forward: true,
            name: default_backbone_name(),
            public_host: None,
            ping_interval_s: default_ping_interval(),
            stats_push_s: default_stats_push(),
        }
    }
}

impl BackboneConfig {
    pub fn auth_mode(&self) -> AuthMode {
        if self.auth.eq_ignore_ascii_case("open") {
            AuthMode::Open
        } else {
            AuthMode::Token
        }
    }

    pub fn token_key(&self) -> Option<[u8; 32]> {
        wire::token_key(&self.token).ok()
    }

    pub fn allowlist_ids(&self) -> Vec<NodeId> {
        self.allowlist
            .iter()
            .filter_map(|id| NodeId::from_hex(id).ok())
            .collect()
    }

    /// Clamp/normalize user-provided values per `BACKBONE.md` §5.
    fn clamp(&mut self) {
        if self.bind.trim().is_empty() {
            self.bind = default_bind();
        }
        // `port == 0` asks the OS for an ephemeral port (tests/automation); the bound port is
        // reported by the control API. Operator-facing values are u16 already.
        self.max_peers = self.max_peers.clamp(1, 256);
        self.name = clamp_chars(&self.name, 32);
        if self.name.is_empty() {
            self.name = default_backbone_name();
        }
        if let Some(host) = &self.public_host {
            let trimmed = clamp_chars(host, 255);
            self.public_host = (!trimmed.is_empty()).then_some(trimmed);
        }
        if !self.auth.eq_ignore_ascii_case("open") {
            self.auth = "token".to_string();
        } else {
            self.auth = "open".to_string();
        }
        self.ping_interval_s = self.ping_interval_s.clamp(5, 3600);
        self.stats_push_s = self.stats_push_s.clamp(1, 3600);
        self.allowlist = self
            .allowlist
            .iter()
            .filter_map(|entry| NodeId::from_hex(entry).ok().map(|id| id.to_hex()))
            .collect();
    }

    fn view(&self, running: bool, bound_port: u16, peers: usize, mode: &str) -> Value {
        let port = if bound_port != 0 {
            bound_port
        } else {
            self.port
        };
        json!({
            "enabled": self.enabled,
            "running": running,
            "bind": self.bind,
            "port": port,
            "auth": self.auth,
            "token_set": !self.token.is_empty(),
            "max_peers": self.max_peers,
            "allowlist": self.allowlist,
            "store_forward": self.store_forward,
            "name": self.name,
            "public_host": self.public_host,
            "ping_interval_s": self.ping_interval_s,
            "stats_push_s": self.stats_push_s,
            "mode": mode,
            "peers": peers,
        })
    }
}

fn clamp_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

/// CLI / startup overrides applied on top of the persisted configuration.
#[derive(Clone, Debug, Default)]
pub struct ConfigOverrides {
    pub enabled: Option<bool>,
    pub bind: Option<String>,
    pub port: Option<u16>,
    pub auth: Option<String>,
    pub token: Option<String>,
    pub max_peers: Option<usize>,
    pub allowlist: Option<Vec<String>>,
    pub store_forward: Option<bool>,
    pub name: Option<String>,
    pub public_host: Option<String>,
    pub ping_interval_s: Option<u16>,
}

impl ConfigOverrides {
    pub fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.bind.is_none()
            && self.port.is_none()
            && self.auth.is_none()
            && self.token.is_none()
            && self.max_peers.is_none()
            && self.allowlist.is_none()
            && self.store_forward.is_none()
            && self.name.is_none()
            && self.public_host.is_none()
            && self.ping_interval_s.is_none()
    }

    fn apply_to(&self, config: &mut BackboneConfig) {
        if let Some(enabled) = self.enabled {
            config.enabled = enabled;
        }
        if let Some(bind) = &self.bind {
            config.bind = bind.clone();
        }
        if let Some(port) = self.port {
            config.port = port;
        }
        if let Some(auth) = &self.auth {
            config.auth = auth.clone();
        }
        if let Some(token) = &self.token {
            config.token = token.clone();
        }
        if let Some(max_peers) = self.max_peers {
            config.max_peers = max_peers;
        }
        if let Some(allowlist) = &self.allowlist {
            config.allowlist = allowlist.clone();
        }
        if let Some(store_forward) = self.store_forward {
            config.store_forward = store_forward;
        }
        if let Some(name) = &self.name {
            config.name = name.clone();
        }
        if let Some(public_host) = &self.public_host {
            config.public_host = Some(public_host.clone());
        }
        if let Some(ping) = self.ping_interval_s {
            config.ping_interval_s = ping;
        }
    }
}

/// Startup intent for backbone mode.
#[derive(Clone, Debug, Default)]
pub struct BackboneStartup {
    /// Start a backbone listener for this bridge run (`openmesh-bridge backbone`).
    pub enabled: bool,
    pub overrides: ConfigOverrides,
    /// Bootstrap token from `--token-file` (written to `backbone.json`).
    pub token_from_file: Option<String>,
    /// Outbound join target (`openmesh-bridge join ...`).
    pub join: Option<JoinTarget>,
    /// Explicitly requested backbone run: bind failures are fatal instead of best-effort.
    pub explicit: bool,
}

// ---------------------------------------------------------------------------------------------
// Join target (`openmesh-bridge join`)
// ---------------------------------------------------------------------------------------------

/// Parsed `openmesh://backbone?...` join target, including reconnect policy.
#[derive(Clone, Debug)]
pub struct JoinTarget {
    pub connection: ConnectionString,
    pub token_key: Option<[u8; 32]>,
    pub app: String,
    pub reconnect_min_ms: u64,
    pub reconnect_max_ms: u64,
}

impl JoinTarget {
    /// Parse a connection string into a join target.
    pub fn from_url(url: &str) -> Result<Self, String> {
        let connection = parse_connection_string(url)?;
        let token_key = match &connection.token {
            Some(token) => Some(wire::token_key(token).map_err(|e| e.to_string())?),
            None => None,
        };
        Ok(Self {
            connection,
            token_key,
            app: format!("openmesh-bridge/{}", openmesh_core::CRATE_VERSION),
            reconnect_min_ms: 1_000,
            reconnect_max_ms: 30_000,
        })
    }

    /// Override reconnect policy (used by tests and impatient operators).
    pub fn with_backoff(mut self, min_ms: u64, max_ms: u64) -> Self {
        self.reconnect_min_ms = min_ms.max(10);
        self.reconnect_max_ms = max_ms.max(self.reconnect_min_ms);
        self
    }
}

// ---------------------------------------------------------------------------------------------
// Peers and accounting
// ---------------------------------------------------------------------------------------------

/// Direction of one backbone link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerDirection {
    Inbound,
    Outbound,
}

impl PeerDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::Outbound => "outbound",
        }
    }
}

/// Messages queued to a peer's writer task.
#[derive(Debug)]
pub enum PeerOut {
    /// One complete OMP/1 frame.
    Frame(Vec<u8>),
    /// A control message (ping/pong/stats/close).
    Control(ControlMsg),
    /// Graceful close with a reason.
    Close(String),
}

/// One connected backbone peer (server-side session or outbound join link).
pub struct PeerSession {
    /// Monotonic session id; guards against replacing a peer while an old task exits.
    pub session: u32,
    pub node_id: NodeId,
    pub name: String,
    pub direction: PeerDirection,
    pub remote_addr: String,
    pub connected_at_ms: u64,
    pub auth: String,
    pub app_version: String,
    pub tx: mpsc::Sender<PeerOut>,
    pub frames_in: AtomicU64,
    pub frames_out: AtomicU64,
    pub bytes_in: AtomicU64,
    pub bytes_out: AtomicU64,
    pub last_seen_ms: AtomicU64,
    /// Round-trip time in milliseconds; 0 until a ping/pong completed.
    pub rtt_ms: AtomicU64,
    /// Pings we sent, keyed by nonce, for RTT measurement.
    pending_pings: std::sync::Mutex<HashMap<u32, Instant>>,
}

impl PeerSession {
    fn to_json(&self) -> Value {
        let rtt = self.rtt_ms.load(Ordering::Relaxed);
        json!({
            "node_id": self.node_id.to_hex(),
            "name": self.name,
            "direction": self.direction.as_str(),
            "remote_addr": self.remote_addr,
            "connected_at_ms": self.connected_at_ms,
            "last_seen_ms": self.last_seen_ms.load(Ordering::Relaxed),
            "frames_in": self.frames_in.load(Ordering::Relaxed),
            "frames_out": self.frames_out.load(Ordering::Relaxed),
            "bytes_in": self.bytes_in.load(Ordering::Relaxed),
            "bytes_out": self.bytes_out.load(Ordering::Relaxed),
            "rtt_ms": (rtt > 0).then_some(rtt.min(u16::MAX as u64) as u16),
            "app_version": self.app_version,
            "auth": self.auth,
        })
    }

    fn touch(&self) {
        self.last_seen_ms.store(util::now_ms(), Ordering::Relaxed);
    }

    /// Queue one OMP frame for the peer (drops when a slow peer's queue is full).
    pub fn send_frame(&self, bytes: Vec<u8>) -> bool {
        self.tx.try_send(PeerOut::Frame(bytes)).is_ok()
    }

    /// Queue a control message for the peer.
    pub fn send_control(&self, msg: ControlMsg) {
        let _ = self.tx.try_send(PeerOut::Control(msg));
    }

    /// Ask the session to close gracefully.
    pub fn close(&self, reason: &str) {
        let _ = self.tx.try_send(PeerOut::Close(reason.to_string()));
    }
}

/// Cumulative backbone counters.
#[derive(Debug, Default)]
pub struct Totals {
    pub frames_in: AtomicU64,
    pub frames_out: AtomicU64,
    pub bytes_in: AtomicU64,
    pub bytes_out: AtomicU64,
    pub auth_failures: AtomicU64,
}

impl Totals {
    fn add_rx(&self, bytes: u64, peer: &PeerSession) {
        self.frames_in.fetch_add(1, Ordering::Relaxed);
        self.bytes_in.fetch_add(bytes, Ordering::Relaxed);
        peer.frames_in.fetch_add(1, Ordering::Relaxed);
        peer.bytes_in.fetch_add(bytes, Ordering::Relaxed);
    }

    fn add_tx(&self, bytes: u64, peer: &PeerSession) {
        self.frames_out.fetch_add(1, Ordering::Relaxed);
        self.bytes_out.fetch_add(bytes, Ordering::Relaxed);
        peer.frames_out.fetch_add(1, Ordering::Relaxed);
        peer.bytes_out.fetch_add(bytes, Ordering::Relaxed);
    }
}

struct Inner {
    config: BackboneConfig,
    configured: bool,
    server_running: bool,
    bound_port: u16,
    listener_stop: Option<watch::Sender<bool>>,
    client_active: bool,
    client_stop: Option<Arc<AtomicBool>>,
    client_join: Option<JoinTarget>,
    peers: HashMap<NodeId, Arc<PeerSession>>,
    last_error: Option<String>,
    started_at_ms: Option<u64>,
    session_counter: u32,
    prev_tick: Option<(Instant, u64, u64, u64, u64)>,
    rate: HashMap<IpAddr, VecDeque<Instant>>,
}

impl Inner {
    fn mode(&self) -> &'static str {
        match (self.server_running, self.client_active) {
            (true, true) => "server+client",
            (true, false) => "server",
            (false, true) => "client",
            (false, false) => "disabled",
        }
    }

    fn running(&self) -> bool {
        self.server_running || self.client_active
    }
}

// ---------------------------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------------------------

/// Backbone mode manager: owns the listener, peers, outbound client and control-surface data.
pub struct BackboneManager {
    data_dir: PathBuf,
    config_path: PathBuf,
    own_node_id: NodeId,
    api_listen: SocketAddr,
    executable: PathBuf,
    serve_web: Option<PathBuf>,
    token_file: Option<PathBuf>,
    events: broadcast::Sender<Arc<str>>,
    commands: mpsc::Sender<EngineInput>,
    shutdown: watch::Receiver<bool>,
    envelopes: Arc<AtomicUsize>,
    totals: Arc<Totals>,
    inner: Mutex<Inner>,
    tasks: std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>,
    link: AtomicU16,
    link_stats: std::sync::Mutex<Option<Arc<LinkStats>>>,
    pending_handshakes: AtomicUsize,
    running_flag: AtomicBool,
    configured_flag: AtomicBool,
}

impl BackboneManager {
    /// Create the manager for a bridge run. Does not start a listener until [`Self::startup`].
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        data_dir: PathBuf,
        own_node_id: NodeId,
        api_listen: SocketAddr,
        executable: PathBuf,
        serve_web: Option<PathBuf>,
        token_file: Option<PathBuf>,
        events: broadcast::Sender<Arc<str>>,
        commands: mpsc::Sender<EngineInput>,
        shutdown: watch::Receiver<bool>,
        envelopes: Arc<AtomicUsize>,
    ) -> Arc<Self> {
        let manager = Arc::new(Self {
            config_path: data_dir.join("backbone.json"),
            data_dir,
            own_node_id,
            api_listen,
            executable,
            serve_web,
            token_file,
            events,
            commands,
            shutdown,
            envelopes,
            totals: Arc::new(Totals::default()),
            inner: Mutex::new(Inner {
                config: BackboneConfig::default(),
                configured: false,
                server_running: false,
                bound_port: 0,
                listener_stop: None,
                client_active: false,
                client_stop: None,
                client_join: None,
                peers: HashMap::new(),
                last_error: None,
                started_at_ms: None,
                session_counter: 0,
                prev_tick: None,
                rate: HashMap::new(),
            }),
            tasks: std::sync::Mutex::new(Vec::new()),
            link: AtomicU16::new(0),
            link_stats: std::sync::Mutex::new(None),
            pending_handshakes: AtomicUsize::new(0),
            running_flag: AtomicBool::new(false),
            configured_flag: AtomicBool::new(false),
        });
        let ticker = manager.clone();
        let handle = tokio::spawn(async move { ticker.ticker_loop().await });
        manager.tasks.lock().expect("backbone tasks").push(handle);
        manager
    }

    pub fn own_node_id(&self) -> NodeId {
        self.own_node_id
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn is_running(&self) -> bool {
        self.running_flag.load(Ordering::Relaxed)
    }

    pub fn is_configured(&self) -> bool {
        self.configured_flag.load(Ordering::Relaxed)
    }

    fn link_id(&self) -> LinkId {
        self.link.load(Ordering::Relaxed)
    }

    fn link_stats(&self) -> Option<Arc<LinkStats>> {
        self.link_stats.lock().expect("backbone link stats").clone()
    }

    // --- startup / shutdown -------------------------------------------------------------

    /// Load/create the configuration, apply startup intent and start the listener or client.
    pub async fn startup(self: &Arc<Self>, startup: &BackboneStartup) -> Result<(), String> {
        let loaded = self
            .load_config()
            .map_err(|e| format!("reading backbone config: {e}"))?;
        let mut config = loaded.clone().unwrap_or_default();
        let mut configured = loaded.is_some();
        if configured || startup.enabled || !startup.overrides.is_empty() || startup.join.is_some()
        {
            configured = true;
            startup.overrides.apply_to(&mut config);
            if let Some(token) = &startup.token_from_file {
                config.token = token.trim().to_string();
            }
            if startup.enabled || startup.overrides.enabled == Some(true) {
                config.enabled = true;
            }
            if config.token.is_empty() && config.auth_mode() == AuthMode::Token {
                config.token = wire::generate_token();
            }
            config.clamp();
            self.persist_config(&config)?;
        }
        {
            let mut inner = self.inner.lock().await;
            inner.config = config.clone();
            inner.configured = configured;
        }
        self.configured_flag.store(configured, Ordering::Relaxed);
        if let Some(join) = startup.join.clone() {
            self.start_client(join).await?;
        }
        if startup.enabled || (configured && config.enabled) {
            match self.start_server().await {
                Ok(()) => {}
                Err(e) => {
                    if startup.explicit {
                        return Err(e);
                    }
                    tracing::warn!(error = %e, "could not start backbone listener from persisted config");
                }
            }
        }
        if configured {
            self.push_store_forward(config.store_forward).await;
        }
        Ok(())
    }

    /// Stop all backbone tasks (listener, client, sessions, ticker).
    pub async fn shutdown(&self) {
        {
            let mut inner = self.inner.lock().await;
            if let Some(stop) = inner.listener_stop.take() {
                let _ = stop.send(true);
            }
            if let Some(stop) = inner.client_stop.take() {
                stop.store(true, Ordering::Relaxed);
            }
        }
        let handles: Vec<tokio::task::JoinHandle<()>> = self
            .tasks
            .lock()
            .expect("backbone tasks")
            .drain(..)
            .collect();
        for handle in handles {
            handle.abort();
        }
    }

    fn load_config(&self) -> std::io::Result<Option<BackboneConfig>> {
        match std::fs::read_to_string(&self.config_path) {
            Ok(text) => match serde_json::from_str::<BackboneConfig>(&text) {
                Ok(mut config) => {
                    config.clamp();
                    Ok(Some(config))
                }
                Err(e) => {
                    tracing::warn!(path = %self.config_path.display(), error = %e, "ignoring invalid backbone.json");
                    Ok(None)
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn persist_config(&self, config: &BackboneConfig) -> Result<(), String> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("creating data dir: {e}"))?;
        }
        let body =
            serde_json::to_vec_pretty(config).map_err(|e| format!("serializing config: {e}"))?;
        std::fs::write(&self.config_path, body)
            .map_err(|e| format!("writing {}: {e}", self.config_path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                std::fs::set_permissions(&self.config_path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    // --- engine plumbing ------------------------------------------------------------------

    /// Register the backbone transport with the engine and return its handle.
    pub fn transport_handle(self: &Arc<Self>, link: LinkId) -> TransportHandle {
        self.link.store(link, Ordering::Relaxed);
        let stats = make_link_stats();
        *self.link_stats.lock().expect("backbone link stats") = Some(stats.clone());
        let (tx, mut rx) = mpsc::channel::<TransportFrame>(1024);
        let manager = self.clone();
        let handle = tokio::spawn(async move {
            while let Some(frame) = rx.recv().await {
                manager.dispatch_outbound(link, frame).await;
            }
        });
        self.tasks.lock().expect("backbone tasks").push(handle);
        TransportHandle {
            id: link,
            kind: "backbone",
            label: "Backbone".to_string(),
            tx,
            stats,
        }
    }

    async fn dispatch_outbound(self: &Arc<Self>, link: LinkId, frame: TransportFrame) {
        if frame.link.is_some_and(|preferred| preferred != link) {
            return;
        }
        let peers: Vec<Arc<PeerSession>> = {
            let inner = self.inner.lock().await;
            inner.peers.values().cloned().collect()
        };
        let direct = match frame.mode {
            SendMode::Direct(node) => peers.iter().find(|p| p.node_id == node).cloned(),
            SendMode::Flood => None,
        };
        let targets: Vec<Arc<PeerSession>> = match direct {
            Some(peer) => vec![peer],
            None => peers,
        };
        let bytes = frame.bytes;
        for peer in targets {
            if frame.exclude == Some(peer.node_id) {
                continue;
            }
            if peer.send_frame(bytes.clone()) {
                self.totals.add_tx(bytes.len() as u64, &peer);
                if let Some(stats) = self.link_stats() {
                    stats.record_tx();
                }
            } else {
                tracing::debug!(peer = %peer.node_id, "backbone peer queue full, dropping frame");
            }
        }
    }

    async fn forward_to_engine(&self, peer: &Arc<PeerSession>, bytes: Vec<u8>) -> bool {
        self.totals.add_rx(bytes.len() as u64, peer);
        if let Some(stats) = self.link_stats() {
            stats.record_rx(None, None);
        }
        let link = self.link_id();
        let inbound = Inbound {
            bytes,
            link: Some(LinkInfo {
                link,
                peer: Some(peer.node_id),
                rssi: None,
                snr: None,
            }),
        };
        self.commands
            .send(EngineInput::Inbound(inbound))
            .await
            .is_ok()
    }

    async fn push_store_forward(&self, enabled: bool) {
        let _ = self
            .commands
            .send(EngineInput::SetStoreForward(enabled))
            .await;
    }

    async fn request_announce(&self) {
        let (reply, _rx) = mpsc::unbounded_channel();
        let cmd = ClientCommand {
            id: None,
            kind: CommandKind::Announce,
        };
        let _ = self
            .commands
            .send(EngineInput::Command { cmd, reply })
            .await;
    }

    // --- peer registry --------------------------------------------------------------------

    async fn next_session_id(&self) -> u32 {
        let mut inner = self.inner.lock().await;
        inner.session_counter = inner.session_counter.wrapping_add(1).max(1);
        inner.session_counter
    }

    async fn peer_count(&self) -> usize {
        self.inner.lock().await.peers.len()
    }

    async fn server_handshake_config(&self) -> ServerHandshakeConfig {
        let inner = self.inner.lock().await;
        ServerHandshakeConfig {
            node_id: self.own_node_id,
            name: inner.config.name.clone(),
            auth: inner.config.auth_mode(),
            token: inner.config.token_key(),
            allowlist: inner.config.allowlist_ids(),
            max_peers: inner.config.max_peers,
            ping_interval_s: inner.config.ping_interval_s,
        }
    }

    async fn register_peer(&self, peer: Arc<PeerSession>) -> Option<Arc<PeerSession>> {
        let old = {
            let mut inner = self.inner.lock().await;
            inner.peers.insert(peer.node_id, peer.clone())
        };
        if let Some(old) = &old {
            if old.session != peer.session {
                old.close("replaced");
            }
        }
        self.broadcast_peer("joined", &peer);
        self.request_announce().await;
        old.filter(|old| old.session != peer.session)
    }

    async fn unregister_peer(&self, node_id: NodeId, session: u32) {
        let removed = {
            let mut inner = self.inner.lock().await;
            match inner.peers.get(&node_id) {
                Some(existing) if existing.session == session => inner.peers.remove(&node_id),
                _ => None,
            }
        };
        if let Some(peer) = removed {
            self.broadcast_peer("left", &peer);
        }
    }

    async fn set_last_error(&self, error: Option<String>) {
        self.inner.lock().await.last_error = error;
    }

    fn broadcast_peer(&self, action: &str, peer: &PeerSession) {
        broadcast_event(
            &self.events,
            event_json(
                "backbone_peer",
                Some(json!({ "action": action, "peer": peer.to_json() })),
            ),
        );
    }

    // --- listener lifecycle ---------------------------------------------------------------

    async fn start_server(self: &Arc<Self>) -> Result<(), String> {
        {
            let inner = self.inner.lock().await;
            if inner.server_running {
                return Ok(());
            }
        }
        let config = {
            let mut inner = self.inner.lock().await;
            let mut config = inner.config.clone();
            if config.auth_mode() == AuthMode::Token && config.token.is_empty() {
                config.token = wire::generate_token();
                inner.config.token = config.token.clone();
                self.persist_config(&config)?;
            }
            config
        };
        let address = format!("{}:{}", config.bind, config.port);
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .map_err(|e| format!("binding backbone listener {address}: {e}"))?;
        let bound_port = listener
            .local_addr()
            .map(|a| a.port())
            .unwrap_or(config.port);
        let (stop_tx, stop_rx) = watch::channel(false);
        let manager = self.clone();
        let shutdown = self.shutdown.clone();
        let handle = tokio::spawn(async move {
            server::accept_loop(manager, listener, stop_rx, shutdown).await;
        });
        self.tasks.lock().expect("backbone tasks").push(handle);
        {
            let mut inner = self.inner.lock().await;
            inner.server_running = true;
            inner.bound_port = bound_port;
            inner.listener_stop = Some(stop_tx);
            inner.last_error = None;
            inner.started_at_ms.get_or_insert_with(util::now_ms);
        }
        self.running_flag.store(true, Ordering::Relaxed);
        tracing::info!(listen = %address, port = bound_port, "backbone listener started");
        self.push_store_forward(config.store_forward).await;
        self.broadcast_status().await;
        Ok(())
    }

    async fn stop_server(&self) {
        let peers: Vec<Arc<PeerSession>> = {
            let mut inner = self.inner.lock().await;
            if let Some(stop) = inner.listener_stop.take() {
                let _ = stop.send(true);
            }
            inner.server_running = false;
            inner.bound_port = 0;
            inner.peers.values().cloned().collect()
        };
        for peer in peers {
            if peer.direction == PeerDirection::Inbound {
                peer.close("server stopping");
            }
        }
        self.refresh_running_flag().await;
        self.broadcast_status().await;
    }

    async fn refresh_running_flag(&self) {
        let running = self.inner.lock().await.running();
        self.running_flag.store(running, Ordering::Relaxed);
    }

    // --- client lifecycle -----------------------------------------------------------------

    async fn start_client(self: &Arc<Self>, join: JoinTarget) -> Result<(), String> {
        self.stop_client().await;
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut inner = self.inner.lock().await;
            inner.client_join = Some(join.clone());
            inner.client_stop = Some(stop.clone());
            inner.client_active = true;
            inner.last_error = None;
            inner.started_at_ms.get_or_insert_with(util::now_ms);
        }
        let manager = self.clone();
        let shutdown = self.shutdown.clone();
        let handle = tokio::spawn(async move {
            client::client_loop(manager, join, stop, shutdown).await;
        });
        self.tasks.lock().expect("backbone tasks").push(handle);
        self.refresh_running_flag().await;
        self.broadcast_status().await;
        Ok(())
    }

    async fn stop_client(&self) {
        let (stop, join) = {
            let mut inner = self.inner.lock().await;
            let stop = inner.client_stop.take();
            let join = inner.client_join.take();
            if let Some(stop) = &stop {
                stop.store(true, Ordering::Relaxed);
            }
            inner.client_active = false;
            (stop, join)
        };
        let _ = stop;
        let outbound: Vec<Arc<PeerSession>> = {
            let inner = self.inner.lock().await;
            inner
                .peers
                .values()
                .filter(|p| p.direction == PeerDirection::Outbound)
                .cloned()
                .collect()
        };
        for peer in outbound {
            peer.close("disconnected");
        }
        if join.is_some() {
            tracing::info!("backbone outbound link stopped");
        }
        self.refresh_running_flag().await;
    }

    // --- status / control surface ---------------------------------------------------------

    /// Compute the live `backbone_status` payload (`WEBSOCKET-API.md` §8).
    async fn status_value(&self) -> Value {
        let (
            running,
            server_running,
            peers,
            uptime_s,
            frames_in,
            frames_out,
            bytes_in,
            bytes_out,
            auth_failures,
            last_error,
            throughput_in,
            throughput_out,
        ) = {
            let inner = self.inner.lock().await;
            let now = Instant::now();
            let frames_in = self.totals.frames_in.load(Ordering::Relaxed);
            let frames_out = self.totals.frames_out.load(Ordering::Relaxed);
            let bytes_in = self.totals.bytes_in.load(Ordering::Relaxed);
            let bytes_out = self.totals.bytes_out.load(Ordering::Relaxed);
            let (tin, tout) = match inner.prev_tick {
                Some((at, pf_in, pf_out, pb_in, pb_out)) => {
                    let elapsed = now.duration_since(at).as_secs_f64().max(0.001);
                    let _ = (pb_in, pb_out);
                    (
                        ((frames_in.saturating_sub(pf_in) as f64 * 8.0) / elapsed) as u64,
                        ((frames_out.saturating_sub(pf_out) as f64 * 8.0) / elapsed) as u64,
                    )
                }
                None => (0, 0),
            };
            let _ = (frames_in, frames_out, bytes_in, bytes_out);
            let uptime_s = inner
                .started_at_ms
                .map(|start| util::now_ms().saturating_sub(start) / 1000)
                .unwrap_or(0);
            (
                inner.running(),
                inner.server_running,
                inner.peers.len(),
                uptime_s,
                frames_in,
                frames_out,
                bytes_in,
                bytes_out,
                self.totals.auth_failures.load(Ordering::Relaxed),
                inner.last_error.clone(),
                tin,
                tout,
            )
        };
        let _ = server_running;
        json!({
            "running": running,
            "peers": peers,
            "uptime_s": uptime_s,
            "frames_in": frames_in,
            "frames_out": frames_out,
            "bytes_in": bytes_in,
            "bytes_out": bytes_out,
            "throughput_in_bps": throughput_in,
            "throughput_out_bps": throughput_out,
            "store_forward_envelopes": self.envelopes.load(Ordering::Relaxed),
            "auth_failures": auth_failures,
            "last_error": last_error,
        })
    }

    async fn broadcast_status(&self) {
        let status = self.status_value().await;
        broadcast_event(&self.events, event_json("backbone_status", Some(status)));
    }

    async fn ticker_loop(self: Arc<Self>) {
        let mut shutdown = self.shutdown.clone();
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(2));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = ticker.tick() => {
                    let active = {
                        let mut inner = self.inner.lock().await;
                        if !inner.running() {
                            continue;
                        }
                        let frames_in = self.totals.frames_in.load(Ordering::Relaxed);
                        let frames_out = self.totals.frames_out.load(Ordering::Relaxed);
                        let bytes_in = self.totals.bytes_in.load(Ordering::Relaxed);
                        let bytes_out = self.totals.bytes_out.load(Ordering::Relaxed);
                        let (tin, tout) = if let Some((at, pf_in, pf_out, pb_in, pb_out)) = inner.prev_tick {
                            let elapsed = at.elapsed().as_secs_f64().max(0.001);
                            let _ = (pb_in, pb_out);
                            (((frames_in.saturating_sub(pf_in)) as f64 * 8.0 / elapsed) as u64,
                             ((frames_out.saturating_sub(pf_out)) as f64 * 8.0 / elapsed) as u64)
                        } else { (0, 0) };
                        inner.prev_tick = Some((Instant::now(), frames_in, frames_out, bytes_in, bytes_out));
                        let _ = (tin, tout);
                        true
                    };
                    if active {
                        self.broadcast_status().await;
                    }
                }
            }
        }
        tracing::debug!("backbone manager stopped");
    }

    /// `GET /api/backbone` payload.
    pub async fn describe(&self) -> Value {
        let configured = self.is_configured();
        if !configured {
            return json!({ "enabled": false });
        }
        let (config, running, bound_port, peers, mode) = {
            let inner = self.inner.lock().await;
            (
                inner.config.clone(),
                inner.running(),
                inner.bound_port,
                inner.peers.len(),
                inner.mode(),
            )
        };
        let uptime_s = self
            .inner
            .lock()
            .await
            .started_at_ms
            .map(|start| util::now_ms().saturating_sub(start) / 1000)
            .unwrap_or(0);
        let mut view = config.view(running, bound_port, peers, mode);
        if let Some(object) = view.as_object_mut() {
            object.insert(
                "totals".into(),
                json!({
                    "frames_in": self.totals.frames_in.load(Ordering::Relaxed),
                    "frames_out": self.totals.frames_out.load(Ordering::Relaxed),
                    "bytes_in": self.totals.bytes_in.load(Ordering::Relaxed),
                    "bytes_out": self.totals.bytes_out.load(Ordering::Relaxed),
                    "auth_failures": self.totals.auth_failures.load(Ordering::Relaxed),
                    "uptime_s": uptime_s,
                    "peers": peers,
                }),
            );
        }
        view
    }

    /// Live `backbone_status` payload (also used by the `get_backbone` WS command).
    pub async fn status_view(&self) -> Value {
        self.status_value().await
    }

    /// `GET /api/backbone/peers` payload.
    pub async fn peers_view(&self) -> Value {
        if !self.is_configured() {
            return json!({ "enabled": false, "peers": [] });
        }
        let peers: Vec<Value> = {
            let inner = self.inner.lock().await;
            let mut peers: Vec<Arc<PeerSession>> = inner.peers.values().cloned().collect();
            peers.sort_by_key(|peer| peer.connected_at_ms);
            peers.iter().map(|peer| peer.to_json()).collect()
        };
        Value::Array(peers)
    }

    /// `POST /api/backbone/config`: partial update; returns `(config view, restarted)`.
    pub async fn update_config(self: &Arc<Self>, patch: &Value) -> Result<(Value, bool), String> {
        let Some(object) = patch.as_object() else {
            return Err("config must be a JSON object".to_string());
        };
        if !self.is_configured() && object.get("enabled").and_then(|v| v.as_bool()) != Some(true) {
            return Err(
                "backbone mode is not configured; set {\"enabled\": true} to enable it".to_string(),
            );
        }
        let mut join_request: Option<String> = None;
        let mut disconnect_request: Option<String> = None;
        let (mut config, was_running, old_bind, old_port) = {
            let mut inner = self.inner.lock().await;
            inner.configured = true;
            (
                inner.config.clone(),
                inner.server_running,
                inner.config.bind.clone(),
                inner.config.port,
            )
        };
        let mut changed_store_forward = false;
        if let Some(value) = object.get("enabled").and_then(|v| v.as_bool()) {
            config.enabled = value;
        }
        if let Some(value) = object.get("bind").and_then(|v| v.as_str()) {
            config.bind = value.to_string();
        }
        if let Some(value) = object.get("port").and_then(|v| v.as_u64()) {
            config.port = u16::try_from(value).map_err(|_| "port out of range".to_string())?;
        }
        if let Some(value) = object.get("auth").and_then(|v| v.as_str()) {
            if !value.eq_ignore_ascii_case("token") && !value.eq_ignore_ascii_case("open") {
                return Err("auth must be \"token\" or \"open\"".to_string());
            }
            config.auth = value.to_string();
        }
        if let Some(value) = object.get("max_peers").and_then(|v| v.as_u64()) {
            config.max_peers =
                usize::try_from(value).map_err(|_| "max_peers out of range".to_string())?;
        }
        if let Some(value) = object.get("allowlist").and_then(|v| v.as_array()) {
            let mut allowlist = Vec::new();
            for entry in value {
                let Some(text) = entry.as_str() else {
                    return Err("allowlist entries must be node id strings".to_string());
                };
                let node = NodeId::from_hex(text)
                    .map_err(|_| format!("invalid allowlist node id {text:?}"))?;
                allowlist.push(node.to_hex());
            }
            config.allowlist = allowlist;
        }
        if let Some(value) = object.get("store_forward").and_then(|v| v.as_bool()) {
            changed_store_forward = value != config.store_forward;
            config.store_forward = value;
        }
        if let Some(value) = object.get("name").and_then(|v| v.as_str()) {
            config.name = value.to_string();
        }
        if let Some(value) = object.get("public_host") {
            config.public_host = value.as_str().map(|v| v.to_string());
        }
        if let Some(value) = object.get("ping_interval_s").and_then(|v| v.as_u64()) {
            config.ping_interval_s =
                u16::try_from(value).map_err(|_| "ping_interval_s out of range".to_string())?;
        }
        if let Some(value) = object.get("stats_push_s").and_then(|v| v.as_u64()) {
            config.stats_push_s =
                u16::try_from(value).map_err(|_| "stats_push_s out of range".to_string())?;
        }
        if let Some(value) = object.get("join_url").and_then(|v| v.as_str()) {
            join_request = Some(value.to_string());
        }
        if let Some(value) = object.get("disconnect").and_then(|v| v.as_str()) {
            disconnect_request = Some(value.to_string());
        }
        config.clamp();
        if config.auth_mode() == AuthMode::Token && config.token.is_empty() {
            config.token = wire::generate_token();
        }
        self.persist_config(&config)?;
        {
            let mut inner = self.inner.lock().await;
            inner.config = config.clone();
            inner.configured = true;
        }
        self.configured_flag.store(true, Ordering::Relaxed);

        let listener_changed = config.bind != old_bind || config.port != old_port;
        let mut restarted = false;
        if was_running && (!config.enabled || listener_changed) {
            self.stop_server().await;
            restarted = true;
        }
        if config.enabled {
            let server_running = {
                let inner = self.inner.lock().await;
                inner.server_running
            };
            if !server_running {
                self.start_server().await?;
                restarted = true;
            }
        }
        if changed_store_forward {
            self.push_store_forward(config.store_forward).await;
        }
        if let Some(url) = join_request {
            self.start_client(JoinTarget::from_url(&url)?).await?;
            restarted = true;
        }
        if let Some(node) = disconnect_request {
            if let Ok(node) = NodeId::from_hex(&node) {
                self.kick(node).await;
            }
        }
        self.broadcast_status().await;
        let view = self.describe().await;
        Ok((view, restarted))
    }

    /// `POST /api/backbone/token/rotate`: returns the new token once.
    pub async fn rotate_token(&self) -> Result<Option<String>, String> {
        if !self.is_configured() {
            return Ok(None);
        }
        let token = wire::generate_token();
        let mut config = {
            let mut inner = self.inner.lock().await;
            inner.config.token = token.clone();
            inner.config.clone()
        };
        config.token = token.clone();
        self.persist_config(&config)?;
        tracing::info!("backbone token rotated (old token invalid for new handshakes)");
        Ok(Some(token))
    }

    /// Disconnect one peer (`DELETE /api/backbone/peers/:node_id`).
    pub async fn kick(&self, node_id: NodeId) -> Option<bool> {
        if !self.is_configured() {
            return None;
        }
        let peer = {
            let mut inner = self.inner.lock().await;
            inner.peers.remove(&node_id)
        };
        let Some(peer) = peer else {
            return Some(false);
        };
        if peer.direction == PeerDirection::Outbound {
            let mut inner = self.inner.lock().await;
            if let Some(stop) = inner.client_stop.take() {
                stop.store(true, Ordering::Relaxed);
            }
            inner.client_active = false;
        }
        peer.close("kicked");
        self.broadcast_peer("left", &peer);
        self.refresh_running_flag().await;
        Some(true)
    }

    /// Record a non-auth handshake failure (protocol/IO) for the panel.
    pub(crate) async fn note_handshake_error(&self, remote: SocketAddr, message: &str) {
        self.set_last_error(Some(format!("handshake from {remote} failed: {message}")))
            .await;
    }

    /// Record an outbound client failure for the panel.
    pub(crate) async fn note_outbound_failure(&self, message: String) {
        self.set_last_error(Some(message)).await;
    }

    pub(crate) async fn clear_last_error(&self) {
        self.set_last_error(None).await;
    }

    pub(crate) async fn on_client_loop_exit(&self) {
        {
            let mut inner = self.inner.lock().await;
            inner.client_active = false;
            inner.client_stop = None;
        }
        self.refresh_running_flag().await;
        self.broadcast_status().await;
    }

    /// A clone of the bridge shutdown receiver for session tasks.
    pub(crate) fn shutdown_receiver(&self) -> watch::Receiver<bool> {
        self.shutdown.clone()
    }

    /// `GET /api/backbone/setup` payload (`BACKBONE.md` §8).
    pub async fn setup(&self) -> Value {
        if !self.is_configured() {
            return json!({ "enabled": false });
        }
        let (config, bound_port) = {
            let inner = self.inner.lock().await;
            (inner.config.clone(), inner.bound_port)
        };
        let port = if bound_port != 0 {
            bound_port
        } else {
            config.port
        };
        let host = config
            .public_host
            .clone()
            .or_else(detect_local_host)
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let connection = ConnectionString {
            host: host.clone(),
            port,
            token: (config.auth_mode() == AuthMode::Token && !config.token.is_empty())
                .then(|| config.token.clone()),
            node: Some(self.own_node_id),
            name: Some(config.name.clone()),
        };
        let connection_string = format_connection_string(&connection);
        let listen = format!("{}:{}", config.bind, port);
        let firewall_add = format!("sudo firewall-cmd --add-port={port}/tcp --permanent");
        let firewall_reload = "sudo firewall-cmd --reload".to_string();
        let systemd_unit = systemd_unit(
            true,
            &self.executable,
            &self.data_dir,
            self.api_listen,
            port,
            self.serve_web.as_deref(),
            self.token_file.as_deref(),
            false,
        );
        json!({
            "enabled": true,
            "listen": listen,
            "public_host": host,
            "connection_string": connection_string,
            "token_set": !config.token.is_empty(),
            "auth": config.auth,
            "steps": [
                {
                    "title": "Install OpenMesh",
                    "commands": ["curl -fsSL https://github.com/martintondel02/openmesh/releases/latest/download/install.sh | sh"],
                    "note": "Installs the openmesh-bridge daemon and the web panel bundle."
                },
                {
                    "title": "Start the backbone",
                    "commands": [format!("openmesh-bridge backbone --port {port} --data-dir {}", self.data_dir.display())],
                    "note": "First run generates the token and prints this same setup guide."
                },
                {
                    "title": "Open the firewall",
                    "commands": [firewall_add.clone(), firewall_reload.clone()],
                    "note": "Only the OBB/1 port needs to be reachable from the internet."
                },
                crate::setup::exposure_step(port),
                {
                    "title": "Keep it running",
                    "commands": ["openmesh-bridge install-service --backbone"],
                    "note": "Installs a hardened systemd user service that survives reboots."
                },
                {
                    "title": "Connect a gateway",
                    "commands": [format!("openmesh-bridge join '{connection_string}'")],
                    "note": "Run this on any bridge that should link its local mesh to the backbone."
                }
            ],
            "exposure": crate::setup::exposure_payload(port),
            "systemd_unit": systemd_unit,
            "firewall": {
                "firewalld": [firewall_add, firewall_reload],
                "ufw": [format!("sudo ufw allow {port}/tcp"), "sudo ufw reload".to_string()]
            },
            "notes": [
                "OpenMesh payloads are end-to-end encrypted; the backbone relays ciphertext only.",
                "The token in the connection string is a bearer secret: share it over a trusted channel and rotate it if leaked.",
                "For hostile networks terminate TLS in front of the OBB/1 port or use WireGuard/Tailscale."
            ],
            "panel_url": format!("http://{}/backbone", self.api_listen),
        })
    }

    /// `GET /api/backbone/qr.svg`.
    pub async fn setup_qr_svg(&self) -> Result<Option<String>, String> {
        if !self.is_configured() {
            return Ok(None);
        }
        let setup = self.setup().await;
        let Some(connection) = setup.get("connection_string").and_then(|v| v.as_str()) else {
            return Err("connection string is not available".to_string());
        };
        qr_svg(connection).map(Some)
    }
}

fn detect_local_host() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
}

/// Server-side handshake parameters resolved from the live configuration.
pub(crate) struct ServerHandshakeConfig {
    pub node_id: NodeId,
    pub name: String,
    pub auth: AuthMode,
    pub token: Option<[u8; 32]>,
    pub allowlist: Vec<NodeId>,
    pub max_peers: usize,
    pub ping_interval_s: u16,
}

impl BackboneManager {
    /// Resolve the parameters for a new inbound handshake.
    pub(crate) async fn handshake_config(&self) -> ServerHandshakeConfig {
        self.server_handshake_config().await
    }

    /// Increment the pending-handshake counter; returns `None` when over the concurrency limit.
    pub(crate) fn begin_handshake(self: &Arc<Self>) -> Option<PendingHandshakeGuard> {
        if self.pending_handshakes.load(Ordering::Relaxed) >= 30 {
            return None;
        }
        self.pending_handshakes.fetch_add(1, Ordering::Relaxed);
        Some(PendingHandshakeGuard {
            manager: self.clone(),
        })
    }

    /// Per-remote-IP handshake rate limit (`BACKBONE.md` §3: 10/minute).
    pub(crate) async fn allow_handshake_from(&self, ip: IpAddr) -> bool {
        let mut inner = self.inner.lock().await;
        let now = Instant::now();
        let window = std::time::Duration::from_secs(60);
        let entry = inner.rate.entry(ip).or_default();
        while entry
            .front()
            .is_some_and(|at| now.duration_since(*at) > window)
        {
            entry.pop_front();
        }
        if entry.len() >= 10 {
            return false;
        }
        entry.push_back(now);
        true
    }

    /// Count an auth failure and remember a token-free error string.
    pub(crate) async fn note_auth_failure(&self, remote: SocketAddr, reason: &str) {
        self.totals.auth_failures.fetch_add(1, Ordering::Relaxed);
        self.set_last_error(Some(format!("handshake from {remote} rejected: {reason}")))
            .await;
    }

    pub(crate) async fn peer_total(&self) -> usize {
        self.peer_count().await
    }

    pub(crate) async fn new_session_id(&self) -> u32 {
        self.next_session_id().await
    }

    /// Build a peer session record for a freshly authenticated link.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn make_peer(
        &self,
        node_id: NodeId,
        name: String,
        direction: PeerDirection,
        remote_addr: String,
        auth: String,
        app_version: String,
    ) -> (Arc<PeerSession>, mpsc::Receiver<PeerOut>) {
        let session = self.next_session_id().await;
        let (tx, rx) = mpsc::channel(256);
        let peer = Arc::new(PeerSession {
            session,
            node_id,
            name,
            direction,
            remote_addr,
            connected_at_ms: util::now_ms(),
            auth,
            app_version,
            tx,
            frames_in: AtomicU64::new(0),
            frames_out: AtomicU64::new(0),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            last_seen_ms: AtomicU64::new(util::now_ms()),
            rtt_ms: AtomicU64::new(0),
            pending_pings: std::sync::Mutex::new(HashMap::new()),
        });
        (peer, rx)
    }

    pub(crate) async fn publish_peer(&self, peer: Arc<PeerSession>) {
        let _ = self.register_peer(peer).await;
    }

    pub(crate) async fn remove_peer(&self, node_id: NodeId, session: u32) {
        self.unregister_peer(node_id, session).await;
    }

    pub(crate) async fn inject_frame(&self, peer: &Arc<PeerSession>, bytes: Vec<u8>) -> bool {
        self.forward_to_engine(peer, bytes).await
    }
}

/// RAII guard for the pending-handshake budget.
pub(crate) struct PendingHandshakeGuard {
    manager: Arc<BackboneManager>,
}

impl Drop for PendingHandshakeGuard {
    fn drop(&mut self) {
        self.manager
            .pending_handshakes
            .fetch_sub(1, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------------------------
// Sessions (shared by the server listener and the outbound join client)
// ---------------------------------------------------------------------------------------------

/// Run the post-handshake read/write loop for one peer until it closes.
pub(crate) async fn run_session(
    manager: Arc<BackboneManager>,
    peer: Arc<PeerSession>,
    read_half: tokio::net::tcp::OwnedReadHalf,
    mut write_half: OwnedWriteHalf,
    outbound_rx: mpsc::Receiver<PeerOut>,
    ping_interval_s: u16,
    mut shutdown: watch::Receiver<bool>,
) {
    let last_seen = Arc::new(AtomicU64::new(util::now_ms()));
    let writer = tokio::spawn(async move {
        let mut outbound_rx = outbound_rx;
        while let Some(message) = outbound_rx.recv().await {
            match message {
                PeerOut::Frame(bytes) => {
                    if wire::write_frame(&mut write_half, &bytes).await.is_err() {
                        break;
                    }
                }
                PeerOut::Control(msg) => {
                    if wire::write_control(&mut write_half, &msg).await.is_err() {
                        break;
                    }
                }
                PeerOut::Close(reason) => {
                    let _ =
                        wire::write_control(&mut write_half, &ControlMsg::Close { reason }).await;
                    let _ = write_half.shutdown().await;
                    break;
                }
            }
        }
        let _ = write_half.shutdown().await;
    });

    // Keepalive / idle enforcement task.
    let ping_peer = peer.clone();
    let ping_last_seen = last_seen.clone();
    let ping_shutdown = shutdown.clone();
    let ping = tokio::spawn(async move {
        let mut ping_shutdown = ping_shutdown;
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(
            ping_interval_s.clamp(5, 3600) as u64,
        ));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut counter = rand::random::<u32>();
        loop {
            tokio::select! {
                _ = ping_shutdown.changed() => break,
                _ = ticker.tick() => {
                    if util::now_ms().saturating_sub(ping_last_seen.load(Ordering::Relaxed)) > 90_000 {
                        ping_peer.close("idle timeout");
                        break;
                    }
                    counter = counter.wrapping_add(1);
                    if let Ok(mut pending) = ping_peer.pending_pings.lock() {
                        if pending.len() > 32 {
                            pending.clear();
                        }
                        pending.insert(counter, Instant::now());
                    }
                    ping_peer.send_control(ControlMsg::Ping { nonce: counter });
                }
            }
        }
    });

    let mut reader = read_half;
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                peer.close("bridge shutting down");
                break;
            }
            message = wire::read_message(&mut reader) => {
                match message {
                    Ok(Some(WireMessage::Frame(bytes))) => {
                        last_seen.store(util::now_ms(), Ordering::Relaxed);
                        peer.touch();
                        if !manager.inject_frame(&peer, bytes).await {
                            break;
                        }
                    }
                    Ok(Some(WireMessage::Control(control))) => {
                        last_seen.store(util::now_ms(), Ordering::Relaxed);
                        match control {
                            ControlMsg::Ping { nonce } => {
                                peer.send_control(ControlMsg::Pong { nonce, rtt_ms: None });
                            }
                            ControlMsg::Pong { nonce, .. } => {
                                let sent = peer
                                    .pending_pings
                                    .lock()
                                    .ok()
                                    .and_then(|mut pending| pending.remove(&nonce));
                                if let Some(sent) = sent {
                                    let rtt = sent.elapsed().as_millis().min(u16::MAX as u128) as u64;
                                    peer.rtt_ms.store(rtt, Ordering::Relaxed);
                                }
                            }
                            ControlMsg::Stats { .. } => {}
                            ControlMsg::Close { .. } | ControlMsg::Error { .. } => break,
                            _ => {}
                        }
                    }
                    Ok(Some(WireMessage::Unknown(_))) => {}
                    Ok(None) => break,
                    Err(WireError::Io(_)) | Err(WireError::AuthFailed(_)) => break,
                    Err(e) => {
                        tracing::debug!(peer = %peer.node_id, error = %e, "backbone session error");
                        break;
                    }
                }
            }
        }
    }
    manager.remove_peer(peer.node_id, peer.session).await;
    peer.close("session closed");
    ping.abort();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), writer).await;
    tracing::debug!(peer = %peer.node_id, direction = peer.direction.as_str(), "backbone session closed");
}

// ---------------------------------------------------------------------------------------------
// Setup / service helpers
// ---------------------------------------------------------------------------------------------

/// Render a hardened systemd unit for the bridge / backbone daemon.
///
/// `backbone_mode` selects the `backbone` subcommand; `serve_web` and `token_file` are embedded
/// in `ExecStart` when supplied. The caller decides between the user and system unit paths.
#[allow(clippy::too_many_arguments)]
pub fn systemd_unit(
    backbone_mode: bool,
    executable: impl AsRef<Path>,
    data_dir: &Path,
    api_listen: SocketAddr,
    backbone_port: u16,
    serve_web: Option<&Path>,
    token_file: Option<&Path>,
    system_unit: bool,
) -> String {
    let executable = executable.as_ref().display();
    let mut args = if backbone_mode {
        format!(
            "backbone --data-dir {} --listen {api_listen}",
            data_dir.display()
        )
    } else {
        format!("--data-dir {} --listen {api_listen}", data_dir.display())
    };
    if backbone_mode {
        args.push_str(&format!(" --port {backbone_port}"));
    }
    if let Some(path) = serve_web {
        args.push_str(&format!(" --serve-web {}", path.display()));
    }
    if let Some(path) = token_file {
        args.push_str(&format!(" --token-file {}", path.display()));
    }
    let wanted_by = if system_unit {
        "multi-user.target"
    } else {
        "default.target"
    };
    let description = if backbone_mode {
        "OpenMesh backbone bridge"
    } else {
        "OpenMesh bridge"
    };
    // The API token defaults to the user config dir (outside the data dir); keep it writable so
    // the service can create/read it under ProtectHome=read-only.
    let token_dir = match token_file {
        Some(path) => path.parent().map(Path::to_path_buf),
        None => crate::token::default_token_path()
            .parent()
            .map(Path::to_path_buf),
    };
    let mut read_write_paths = format!("ReadWritePaths={}", data_dir.display());
    if let Some(token_dir) = token_dir {
        let token_dir = token_dir.display().to_string();
        if !token_dir.is_empty() && token_dir != data_dir.display().to_string() {
            read_write_paths.push(' ');
            read_write_paths.push_str(&token_dir);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    format!(
        "[Unit]\n\
         Description={description}\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={executable} {args}\n\
         WorkingDirectory={home}\n\
         Restart=on-failure\n\
         RestartSec=3\n\
         NoNewPrivileges=true\n\
         ProtectSystem=strict\n\
         ProtectHome=read-only\n\
         PrivateTmp=true\n\
         {read_write_paths}\n\
         Environment=RUST_LOG=info\n\
         \n\
         [Install]\n\
         WantedBy={wanted_by}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_and_clamping() {
        let mut config = BackboneConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.port, 9443);
        assert_eq!(config.max_peers, 32);
        config.port = 0;
        config.max_peers = 1000;
        config.name = "x".repeat(80);
        config.public_host = Some("h".repeat(400));
        config.allowlist = vec!["zzzz".into(), "0102030405060708".into()];
        config.auth = "weird".into();
        config.clamp();
        assert_eq!(config.port, 0);
        assert_eq!(config.max_peers, 256);
        assert_eq!(config.name.chars().count(), 32);
        assert_eq!(config.public_host.as_ref().unwrap().chars().count(), 255);
        assert_eq!(config.allowlist, vec!["0102030405060708".to_string()]);
        assert_eq!(config.auth, "token");
    }

    #[test]
    fn config_roundtrip_json() {
        let config = BackboneConfig {
            enabled: true,
            bind: "127.0.0.1".into(),
            port: 1234,
            auth: "open".into(),
            token: wire::generate_token(),
            max_peers: 3,
            allowlist: vec!["0102030405060708".into()],
            store_forward: false,
            name: "test-backbone".into(),
            public_host: Some("mesh.example.org".into()),
            ping_interval_s: 20,
            stats_push_s: 10,
        };
        let text = serde_json::to_string(&config).unwrap();
        let back: BackboneConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(back.token, config.token);
        assert_eq!(back.auth_mode(), AuthMode::Open);
        assert_eq!(back.port, 1234);
    }

    #[test]
    fn systemd_unit_is_hardened() {
        let unit = systemd_unit(
            true,
            "/usr/local/bin/openmesh-bridge",
            Path::new("/home/u/.local/share/openmesh"),
            "127.0.0.1:8787".parse().unwrap(),
            9443,
            Some(Path::new("/usr/share/openmesh/web")),
            Some(Path::new("/etc/openmesh/bridge-token")),
            false,
        );
        assert!(unit.contains("ExecStart=/usr/local/bin/openmesh-bridge backbone"));
        assert!(unit.contains("--port 9443"));
        assert!(unit.contains("NoNewPrivileges=true"));
        assert!(unit.contains("ProtectSystem=strict"));
        assert!(unit.contains("PrivateTmp=true"));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(unit.contains("--serve-web /usr/share/openmesh/web"));
        assert!(unit.contains("--token-file /etc/openmesh/bridge-token"));
        assert!(
            systemd_unit(
                true,
                "/bin/x",
                Path::new("/d"),
                "127.0.0.1:1".parse().unwrap(),
                9443,
                None,
                None,
                true
            )
            .contains("WantedBy=multi-user.target")
        );
    }

    #[test]
    fn join_target_parsing() {
        let token = wire::generate_token();
        let url = format!(
            "openmesh://backbone?host=mesh.example.org&port=9443&token={token}&node=0102030405060708&name=oslo"
        );
        let target = JoinTarget::from_url(&url).unwrap();
        assert_eq!(target.connection.host, "mesh.example.org");
        assert_eq!(target.connection.port, 9443);
        assert!(target.token_key.is_some());
        assert!(JoinTarget::from_url("nope").is_err());
    }
}

//! Authenticated webhook gateways (v0.7.0, `docs/design/remote-operations.md` §2).
//!
//! A gateway connector bridges one mesh channel (and optionally direct messages) to any external
//! service through an adapter that speaks the signed webhook contract:
//!
//! * **Outbound** — on a new channel/direct message the bridge POSTs a JSON body with
//!   `X-OpenMesh-Signature: sha256=<hex HMAC-SHA256(secret, raw body)>`, bounded retries and
//!   activity-log entries.
//! * **Inbound** — `POST /api/gateways/{id}/inbound` verifies the same signature over the raw
//!   body and injects a channel message attributed to the connector (display-only `from`).
//!
//! Configuration lives in `<data-dir>/gateways.json` (mode 0600). **Secret values are never
//! stored, logged or returned** — the config only holds environment variable *names* that are
//! resolved when a request is signed.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use openmesh_core::{NodeId, util};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::activity::ActivityLog;
use crate::runtime::{self, CommandKind, GatewayInboundData, Shared};

/// File name inside the data dir.
pub const GATEWAYS_FILE: &str = "gateways.json";
/// Outbound text is truncated to this many characters (contract §2).
pub const MAX_TEXT_CHARS: usize = 4000;
/// Inbound requests are rate limited to this many per connector per minute.
pub const INBOUND_RATE_LIMIT: usize = 30;
/// Maximum inbound JSON body size.
pub const MAX_INBOUND_BODY: usize = 64 * 1024;
/// Signature header carrying `sha256=<hex>`.
pub const SIGNATURE_HEADER: &str = "X-OpenMesh-Signature";

const DEFAULT_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_MAX_RETRIES: u32 = 3;
const DEFAULT_MAX_CHARS: usize = 400;
const MAX_TIMEOUT_MS: u64 = 120_000;
const MAX_RETRIES: u32 = 10;
const MAX_CHARS: usize = 4000;
const MAX_CHANNEL_IDS: usize = 64;
const RETRY_BASE_MS: u64 = 120;
const RETRY_CAP_MS: u64 = 2_000;
const OUTBOUND_QUEUE: usize = 256;

fn version_one() -> u32 {
    1
}
fn default_true() -> bool {
    true
}
fn default_kind() -> String {
    "webhook".to_string()
}
fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}
fn default_max_retries() -> u32 {
    DEFAULT_MAX_RETRIES
}
fn default_max_chars() -> usize {
    DEFAULT_MAX_CHARS
}

// ---------------------------------------------------------------------------------------------
// Configuration (`remote-operations.md` §2)
// ---------------------------------------------------------------------------------------------

/// Persisted gateway configuration (`<data-dir>/gateways.json`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewaysFile {
    #[serde(default = "version_one")]
    pub version: u32,
    #[serde(default)]
    pub connectors: Vec<Connector>,
}

impl Default for GatewaysFile {
    fn default() -> Self {
        Self {
            version: version_one(),
            connectors: Vec::new(),
        }
    }
}

/// One gateway connector. `secret_env` values are environment variable names, never secrets.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connector {
    pub id: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub outbound: Option<OutboundConfig>,
    #[serde(default)]
    pub inbound: Option<InboundConfig>,
}

/// Outbound webhook delivery settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutboundConfig {
    pub url: String,
    pub secret_env: String,
    #[serde(default)]
    pub channel_ids: Vec<String>,
    #[serde(default)]
    pub include_direct: bool,
    #[serde(default)]
    pub include_attachments: bool,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

/// Inbound webhook settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboundConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub secret_env: String,
    pub channel_id: String,
    #[serde(default = "default_max_chars")]
    pub max_chars: usize,
}

/// One message handed to the outbound worker.
#[derive(Clone, Debug)]
pub struct GatewayMessage {
    /// `None` for direct messages.
    pub channel_id: Option<String>,
    pub direct: bool,
    pub message_id: u32,
    pub from: String,
    pub from_name: String,
    pub text: Option<String>,
    pub timestamp_ms: u64,
    /// Attachment metadata only (bytes are never pushed through webhooks in v0.7).
    pub attachment: Option<AttachmentMeta>,
}

/// Attachment metadata included in outbound webhooks.
#[derive(Clone, Debug, Serialize)]
pub struct AttachmentMeta {
    pub mime: String,
    pub size: u32,
}

/// Result of an inbound webhook request, mapped to an HTTP response by the router.
pub struct InboundOutcome {
    pub status: u16,
    pub body: Value,
}

#[derive(Clone, Debug, Default)]
struct ConnectorStatus {
    deliveries: u64,
    failures: u64,
    retries: u64,
    last_success_ms: Option<u64>,
    last_failure_ms: Option<u64>,
    last_status: Option<u16>,
    last_error: Option<String>,
    inbound_accepted: u64,
    inbound_rejected: u64,
    inbound_last_ms: Option<u64>,
}

struct Inner {
    config: GatewaysFile,
    status: HashMap<String, ConnectorStatus>,
    /// Rolling 60 s window of authenticated inbound requests per connector.
    inbound_rate: HashMap<String, VecDeque<Instant>>,
}

struct DeliveryOutcome {
    status: Option<u16>,
    attempts: u32,
    error: Option<String>,
}

impl DeliveryOutcome {
    fn ok(&self) -> bool {
        self.error.is_none() && self.status.is_some_and(|s| (200..300).contains(&s))
    }
}

// ---------------------------------------------------------------------------------------------
// Signatures (HMAC-SHA256, constant-time verification)
// ---------------------------------------------------------------------------------------------

/// HMAC-SHA256 over the raw request body.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    crate::transport::backbone::wire::hmac_sha256(key, data)
}

/// `sha256=<hex HMAC-SHA256(secret, body)>` for the contract header.
pub fn sign_body(secret: &[u8], body: &[u8]) -> String {
    format!("sha256={}", hex::encode(hmac_sha256(secret, body)))
}

/// Constant-time verification of an `X-OpenMesh-Signature` header value.
pub fn verify_signature(secret: &[u8], body: &[u8], header: &str) -> bool {
    let Some(encoded) = header.trim().strip_prefix("sha256=") else {
        return false;
    };
    let Ok(provided) = hex::decode(encoded.trim()) else {
        return false;
    };
    if provided.len() != 32 {
        return false;
    }
    constant_time_eq(&hmac_sha256(secret, body), &provided)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= (x ^ y) as usize;
    }
    diff == 0
}

// ---------------------------------------------------------------------------------------------
// Validation and redaction
// ---------------------------------------------------------------------------------------------

fn valid_connector_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

fn valid_env_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn env_secret(name: &str) -> Option<Vec<u8>> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .map(String::into_bytes)
}

fn env_secret_present(name: &str) -> bool {
    env_secret(name).is_some()
}

fn reject_inline_secrets(value: &Value) -> Result<(), String> {
    let Some(object) = value.as_object() else {
        return Err("connector must be a JSON object".to_string());
    };
    let forbidden = |key: &str| {
        let key = key.to_ascii_lowercase();
        key != "secret_env"
            && (key.contains("secret")
                || key.contains("password")
                || key.contains("token")
                || key == "psk")
    };
    for (key, value) in object {
        if forbidden(key) {
            return Err(format!(
                "inline secret field '{key}' is not accepted; reference an environment \
                 variable with secret_env"
            ));
        }
        if let Some(nested) = value.as_object() {
            for nested_key in nested.keys() {
                if forbidden(nested_key) {
                    return Err(format!(
                        "inline secret field '{nested_key}' is not accepted; reference an \
                         environment variable with secret_env"
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Validate and clamp one connector in place. `check_env` rejects `secret_env` names whose
/// environment variable is not currently set (API upsert); config files loaded at startup only
/// get syntax validation so operators can stage them before the secret exists.
fn validate_connector(connector: &mut Connector, check_env: bool) -> Result<(), String> {
    if !valid_connector_id(&connector.id) {
        return Err(
            "connector id must be 1-64 lowercase letters, digits, '-', '_' or '.'".to_string(),
        );
    }
    if !connector.kind.eq_ignore_ascii_case("webhook") {
        return Err("kind must be \"webhook\" in v0.7".to_string());
    }
    connector.kind = "webhook".to_string();
    if let Some(outbound) = connector.outbound.as_mut() {
        let parsed = reqwest::Url::parse(outbound.url.trim())
            .map_err(|_| "outbound.url must be a valid http(s) URL".to_string())?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("outbound.url must use http or https".to_string());
        }
        if parsed.host_str().is_none() {
            return Err("outbound.url must include a host".to_string());
        }
        outbound.url = outbound.url.trim().to_string();
        if !valid_env_name(&outbound.secret_env) {
            return Err(
                "outbound.secret_env must be an uppercase environment variable name".to_string(),
            );
        }
        if check_env && !env_secret_present(&outbound.secret_env) {
            return Err(format!(
                "environment variable '{}' referenced by outbound.secret_env is not set",
                outbound.secret_env
            ));
        }
        let mut channels = Vec::with_capacity(outbound.channel_ids.len().min(MAX_CHANNEL_IDS));
        for entry in &outbound.channel_ids {
            let node = NodeId::from_hex(entry.trim())
                .map_err(|_| format!("outbound.channel_ids entry {entry:?} is not a channel id"))?;
            if !channels.contains(&node.to_hex()) {
                channels.push(node.to_hex());
            }
            if channels.len() >= MAX_CHANNEL_IDS {
                break;
            }
        }
        outbound.channel_ids = channels;
        outbound.timeout_ms = outbound.timeout_ms.clamp(100, MAX_TIMEOUT_MS);
        outbound.max_retries = outbound.max_retries.min(MAX_RETRIES);
    }
    if let Some(inbound) = connector.inbound.as_mut() {
        if !valid_env_name(&inbound.secret_env) {
            return Err(
                "inbound.secret_env must be an uppercase environment variable name".to_string(),
            );
        }
        if check_env && !env_secret_present(&inbound.secret_env) {
            return Err(format!(
                "environment variable '{}' referenced by inbound.secret_env is not set",
                inbound.secret_env
            ));
        }
        NodeId::from_hex(inbound.channel_id.trim())
            .map_err(|_| "inbound.channel_id must be a channel id".to_string())?;
        inbound.channel_id = inbound.channel_id.trim().to_string();
        inbound.max_chars = inbound.max_chars.clamp(1, MAX_CHARS);
    }
    if connector.outbound.is_none() && connector.inbound.is_none() {
        return Err("connector must configure outbound, inbound or both".to_string());
    }
    Ok(())
}

fn connector_accepts(outbound: &OutboundConfig, message: &GatewayMessage) -> bool {
    if message.direct {
        return outbound.include_direct;
    }
    if outbound.channel_ids.is_empty() {
        return true;
    }
    message
        .channel_id
        .as_ref()
        .is_some_and(|channel| outbound.channel_ids.iter().any(|id| id == channel))
}

fn message_payload(connector: &str, message: &GatewayMessage, include_attachments: bool) -> Value {
    let text: String = message
        .text
        .as_deref()
        .unwrap_or("")
        .chars()
        .take(MAX_TEXT_CHARS)
        .collect();
    let mut payload = json!({
        "event": "message",
        "connector": connector,
        "channel_id": message.channel_id,
        "message_id": message.message_id,
        "from": message.from,
        "from_name": message.from_name,
        "text": text,
        "timestamp_ms": message.timestamp_ms,
    });
    if include_attachments {
        if let Some(attachment) = &message.attachment {
            payload["attachment"] = json!({
                "mime": attachment.mime,
                "size": attachment.size,
            });
        }
    }
    payload
}

/// Display-safe webhook URL: keep scheme/host/path, drop query/fragment (they may carry
/// provider credentials). The full URL stays in `gateways.json` (mode 0600) only.
pub fn redact_webhook_url(url: &str) -> String {
    let without_fragment = url.split('#').next().unwrap_or(url);
    match without_fragment.split_once('?') {
        Some((base, _)) => format!("{base}?<redacted>"),
        None => without_fragment.to_string(),
    }
}

fn redacted_connector_view(connector: &Connector, status: &ConnectorStatus) -> Value {
    let outbound = connector.outbound.as_ref().map(|outbound| {
        json!({
            "url": redact_webhook_url(&outbound.url),
            "secret_env": outbound.secret_env,
            "secret_set": env_secret_present(&outbound.secret_env),
            "channel_ids": outbound.channel_ids,
            "include_direct": outbound.include_direct,
            "include_attachments": outbound.include_attachments,
            "timeout_ms": outbound.timeout_ms,
            "max_retries": outbound.max_retries,
        })
    });
    let inbound = connector.inbound.as_ref().map(|inbound| {
        json!({
            "enabled": inbound.enabled,
            "secret_env": inbound.secret_env,
            "secret_set": env_secret_present(&inbound.secret_env),
            "channel_id": inbound.channel_id,
            "max_chars": inbound.max_chars,
            "inbound_path": format!("/api/gateways/{}/inbound", connector.id),
        })
    });
    json!({
        "id": connector.id,
        "kind": connector.kind,
        "enabled": connector.enabled,
        "outbound": outbound,
        "inbound": inbound,
        "status": {
            "deliveries": status.deliveries,
            "failures": status.failures,
            "retries": status.retries,
            "last_success_ms": status.last_success_ms,
            "last_failure_ms": status.last_failure_ms,
            "last_status": status.last_status,
            "last_error": status.last_error,
            "inbound_accepted": status.inbound_accepted,
            "inbound_rejected": status.inbound_rejected,
            "inbound_last_ms": status.inbound_last_ms,
        },
    })
}

// ---------------------------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------------------------

/// Owns the gateway configuration, the outbound delivery worker and inbound authentication.
pub struct GatewayManager {
    data_dir: PathBuf,
    config_path: PathBuf,
    activity: Arc<ActivityLog>,
    client: reqwest::Client,
    inner: Mutex<Inner>,
    tx: tokio::sync::mpsc::Sender<GatewayMessage>,
    worker: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    enqueued: AtomicU64,
    dropped: AtomicU64,
}

impl GatewayManager {
    /// Create the manager for the bridge's data dir, spawning the outbound worker.
    pub fn new(data_dir: PathBuf, activity: Arc<ActivityLog>) -> Arc<Self> {
        let config_path = data_dir.join(GATEWAYS_FILE);
        let config = load_config(&config_path);
        Self::start(data_dir, config_path, config, activity)
    }

    /// Construct a manager from an in-memory config (tests / callers that manage persistence).
    pub fn with_config(
        data_dir: PathBuf,
        activity: Arc<ActivityLog>,
        config: GatewaysFile,
    ) -> Arc<Self> {
        let config_path = data_dir.join(GATEWAYS_FILE);
        Self::start(data_dir, config_path, config, activity)
    }

    fn start(
        data_dir: PathBuf,
        config_path: PathBuf,
        config: GatewaysFile,
        activity: Arc<ActivityLog>,
    ) -> Arc<Self> {
        let (tx, rx) = tokio::sync::mpsc::channel::<GatewayMessage>(OUTBOUND_QUEUE);
        let manager = Arc::new(Self {
            data_dir,
            config_path,
            activity,
            client: reqwest::Client::builder()
                .user_agent(concat!("openmesh-bridge/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_default(),
            inner: Mutex::new(Inner {
                config,
                status: HashMap::new(),
                inbound_rate: HashMap::new(),
            }),
            tx,
            worker: std::sync::Mutex::new(None),
            enqueued: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
        });
        let worker_manager = manager.clone();
        let handle = tokio::spawn(async move { worker_manager.worker_loop(rx).await });
        if let Ok(mut slot) = manager.worker.lock() {
            *slot = Some(handle);
        }
        manager
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// Queue one message for webhook delivery (drops when the bounded queue is full).
    pub fn enqueue(&self, message: GatewayMessage) {
        match self.tx.try_send(message) {
            Ok(()) => {
                self.enqueued.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                tracing::debug!("gateway outbound queue full; dropping message");
            }
        }
    }

    /// Stop the outbound worker (called from [`crate::Bridge::shutdown`]).
    pub async fn shutdown(&self) {
        if let Ok(mut slot) = self.worker.lock() {
            if let Some(handle) = slot.take() {
                handle.abort();
            }
        }
    }

    async fn worker_loop(self: Arc<Self>, mut rx: tokio::sync::mpsc::Receiver<GatewayMessage>) {
        while let Some(message) = rx.recv().await {
            self.deliver_message(message).await;
        }
    }

    /// `GET /api/gateways` payload: redacted configuration + live status.
    pub async fn list_view(&self) -> Value {
        let inner = self.inner.lock().await;
        let connectors: Vec<Value> = inner
            .config
            .connectors
            .iter()
            .map(|connector| {
                let status = inner.status.get(&connector.id).cloned().unwrap_or_default();
                redacted_connector_view(connector, &status)
            })
            .collect();
        json!({
            "version": inner.config.version,
            "connectors": connectors,
            "enqueued": self.enqueued.load(Ordering::Relaxed),
            "dropped": self.dropped.load(Ordering::Relaxed),
        })
    }

    /// `PUT /api/gateways/{id}` — upsert one connector.
    pub async fn upsert(self: &Arc<Self>, id: &str, body: &Value) -> Result<Value, String> {
        if !valid_connector_id(id) {
            return Err(
                "connector id must be 1-64 lowercase letters, digits, '-', '_' or '.'".to_string(),
            );
        }
        reject_inline_secrets(body)?;
        let mut connector: Connector =
            serde_json::from_value(body.clone()).map_err(|e| format!("invalid connector: {e}"))?;
        if connector.id.is_empty() {
            connector.id = id.to_string();
        }
        if connector.id != id {
            return Err("connector id in the body must match the URL".to_string());
        }
        validate_connector(&mut connector, true)?;
        let mut file = {
            let inner = self.inner.lock().await;
            inner.config.clone()
        };
        file.version = version_one();
        match file
            .connectors
            .iter_mut()
            .find(|existing| existing.id == connector.id)
        {
            Some(existing) => *existing = connector.clone(),
            None => file.connectors.push(connector.clone()),
        }
        persist_config(&self.config_path, &file)?;
        let status = {
            let mut inner = self.inner.lock().await;
            inner.config = file;
            inner.status.get(&connector.id).cloned().unwrap_or_default()
        };
        Ok(redacted_connector_view(&connector, &status))
    }

    /// `DELETE /api/gateways/{id}` — remove one connector; `false` when it did not exist.
    pub async fn delete(&self, id: &str) -> Result<bool, String> {
        let mut file = {
            let inner = self.inner.lock().await;
            inner.config.clone()
        };
        let before = file.connectors.len();
        file.connectors.retain(|connector| connector.id != id);
        if file.connectors.len() == before {
            return Ok(false);
        }
        persist_config(&self.config_path, &file)?;
        let mut inner = self.inner.lock().await;
        inner.config = file;
        inner.status.remove(id);
        inner.inbound_rate.remove(id);
        Ok(true)
    }

    /// `POST /api/gateways/{id}/test` — send one signed test payload (single attempt).
    pub async fn test(&self, id: &str) -> Result<Value, String> {
        let connector = {
            let inner = self.inner.lock().await;
            inner
                .config
                .connectors
                .iter()
                .find(|connector| connector.id == id)
                .cloned()
        };
        let Some(connector) = connector else {
            return Err("gateway not found".to_string());
        };
        let Some(outbound) = connector.outbound.clone() else {
            return Err("gateway has no outbound configuration".to_string());
        };
        let payload = json!({
            "event": "test",
            "connector": connector.id,
            "text": "OpenMesh gateway test",
            "timestamp_ms": util::now_ms(),
        });
        let body = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
        let mut single_attempt = outbound.clone();
        single_attempt.max_retries = 0;
        let outcome = self.post_signed(&single_attempt, &body).await;
        Ok(json!({
            "ok": outcome.ok(),
            "connector": connector.id,
            "status": outcome.status,
            "attempts": outcome.attempts,
            "error": outcome.error,
        }))
    }

    /// Inject one inbound webhook message into its configured channel.
    ///
    /// Authentication (`sha256=` HMAC over the raw body), enabled/channel/length/rate-limit
    /// checks all run here and map to 401/404/400/429 without injecting anything.
    pub async fn handle_inbound(
        self: &Arc<Self>,
        shared: &Arc<Shared>,
        id: &str,
        signature: Option<&str>,
        body: &[u8],
    ) -> InboundOutcome {
        if body.len() > MAX_INBOUND_BODY {
            return InboundOutcome {
                status: 413,
                body: json!({ "error": "too_large" }),
            };
        }
        let connector = {
            let inner = self.inner.lock().await;
            inner
                .config
                .connectors
                .iter()
                .find(|connector| connector.id == id)
                .cloned()
        };
        let Some(connector) = connector else {
            return InboundOutcome {
                status: 404,
                body: json!({ "error": "not_found" }),
            };
        };
        let Some(inbound) = connector.inbound.clone().filter(|inbound| inbound.enabled) else {
            return InboundOutcome {
                status: 404,
                body: json!({ "error": "inbound_disabled" }),
            };
        };
        let Some(secret) = env_secret(&inbound.secret_env) else {
            self.note_inbound_for(&connector.id, false).await;
            return InboundOutcome {
                status: 401,
                body: json!({ "error": "unauthorized" }),
            };
        };
        let Some(signature) = signature else {
            self.note_inbound_for(&connector.id, false).await;
            return InboundOutcome {
                status: 401,
                body: json!({ "error": "unauthorized" }),
            };
        };
        if !verify_signature(&secret, body, signature) {
            self.note_inbound_for(&connector.id, false).await;
            tracing::info!(connector = %connector.id, "gateway inbound rejected: bad signature");
            return InboundOutcome {
                status: 401,
                body: json!({ "error": "unauthorized" }),
            };
        }
        if !self
            .allow_inbound(&connector.id, INBOUND_RATE_LIMIT, Duration::from_secs(60))
            .await
        {
            self.note_inbound_for(&connector.id, false).await;
            return InboundOutcome {
                status: 429,
                body: json!({ "error": "rate_limited", "message": "inbound rate limit exceeded" }),
            };
        }
        let payload: Value = match serde_json::from_slice(body) {
            Ok(value) => value,
            Err(e) => {
                self.note_inbound_for(&connector.id, false).await;
                return InboundOutcome {
                    status: 400,
                    body: json!({ "error": "bad_request", "message": e.to_string() }),
                };
            }
        };
        let Some(text) = payload
            .get("text")
            .and_then(|value| value.as_str())
            .map(str::to_string)
        else {
            self.note_inbound_for(&connector.id, false).await;
            return InboundOutcome {
                status: 400,
                body: json!({ "error": "bad_request", "message": "text is required" }),
            };
        };
        let text = text.trim().to_string();
        if text.is_empty() {
            self.note_inbound_for(&connector.id, false).await;
            return InboundOutcome {
                status: 400,
                body: json!({ "error": "bad_request", "message": "text is required" }),
            };
        }
        let chars = text.chars().count();
        if chars > inbound.max_chars {
            self.note_inbound_for(&connector.id, false).await;
            return InboundOutcome {
                status: 400,
                body: json!({
                    "error": "too_long",
                    "message": format!("text exceeds {} characters", inbound.max_chars),
                }),
            };
        }
        let from = payload
            .get("from")
            .and_then(|value| value.as_str())
            .map(|value| value.chars().take(120).collect::<String>())
            .filter(|value| !value.is_empty());
        let timestamp_ms = payload.get("timestamp_ms").and_then(|value| value.as_u64());
        let data = GatewayInboundData {
            connector_id: connector.id.clone(),
            channel_id: inbound.channel_id.clone(),
            text,
            from,
            timestamp_ms,
        };
        match runtime::run_command(shared, CommandKind::GatewayInbound(data)).await {
            Ok(value) => {
                self.note_inbound_for(&connector.id, true).await;
                self.activity.record(
                    &format!("gateway:{}", connector.id),
                    "POST",
                    &format!("/api/gateways/{}/inbound", connector.id),
                    format!("inject channel message ({chars} chars)"),
                    true,
                    None,
                );
                tracing::info!(
                    connector = %connector.id,
                    channel = %inbound.channel_id,
                    "gateway inbound injected a channel message"
                );
                InboundOutcome {
                    status: 200,
                    body: json!({
                        "ok": true,
                        "connector": connector.id,
                        "channel_id": value.get("channel_id"),
                        "message_id": value.get("message_id"),
                    }),
                }
            }
            Err(error) => {
                self.note_inbound_for(&connector.id, false).await;
                let status = match error.code.as_str() {
                    "not_found" => 404,
                    "unavailable" | "timeout" => 503,
                    _ => 400,
                };
                self.activity.record(
                    &format!("gateway:{}", connector.id),
                    "POST",
                    &format!("/api/gateways/{}/inbound", connector.id),
                    format!("inject channel message ({chars} chars)"),
                    false,
                    Some(error.message.clone()),
                );
                InboundOutcome {
                    status,
                    body: json!({ "error": error.code, "message": error.message }),
                }
            }
        }
    }

    async fn note_inbound_for(&self, id: &str, accepted: bool) {
        let mut inner = self.inner.lock().await;
        let status = inner.status.entry(id.to_string()).or_default();
        status.inbound_last_ms = Some(util::now_ms());
        if accepted {
            status.inbound_accepted += 1;
        } else {
            status.inbound_rejected += 1;
        }
    }

    async fn allow_inbound(&self, id: &str, limit: usize, window: Duration) -> bool {
        let mut inner = self.inner.lock().await;
        let now = Instant::now();
        let entry = inner.inbound_rate.entry(id.to_string()).or_default();
        while entry
            .front()
            .is_some_and(|at| now.duration_since(*at) > window)
        {
            entry.pop_front();
        }
        if entry.len() >= limit {
            return false;
        }
        entry.push_back(now);
        true
    }

    // --- outbound delivery ----------------------------------------------------------------

    async fn deliver_message(&self, message: GatewayMessage) {
        let connectors: Vec<Connector> = {
            let inner = self.inner.lock().await;
            inner.config.connectors.clone()
        };
        for connector in connectors {
            if !connector.enabled {
                continue;
            }
            let Some(outbound) = connector.outbound.as_ref() else {
                continue;
            };
            if !connector_accepts(outbound, &message) {
                continue;
            }
            let payload = message_payload(&connector.id, &message, outbound.include_attachments);
            let Ok(body) = serde_json::to_vec(&payload) else {
                continue;
            };
            self.deliver_payload(&connector.id, outbound, &body, "message")
                .await;
        }
    }

    async fn deliver_payload(
        &self,
        connector_id: &str,
        outbound: &OutboundConfig,
        body: &[u8],
        kind: &str,
    ) {
        let outcome = self.post_signed(outbound, body).await;
        let ok = outcome.ok();
        let now = util::now_ms();
        let status = self
            .inner
            .lock()
            .await
            .status
            .entry(connector_id.to_string())
            .or_default()
            .clone();
        let mut status = status;
        status.retries += u64::from(outcome.attempts.saturating_sub(1));
        status.last_status = outcome.status;
        let error = if ok {
            status.deliveries += 1;
            status.last_success_ms = Some(now);
            status.last_error = None;
            None
        } else {
            status.failures += 1;
            status.last_failure_ms = Some(now);
            let error = outcome
                .error
                .clone()
                .unwrap_or_else(|| "delivery failed".to_string());
            status.last_error = Some(error.clone());
            Some(error)
        };
        {
            let mut inner = self.inner.lock().await;
            inner
                .status
                .insert(connector_id.to_string(), status.clone());
        }
        let summary = if ok {
            format!(
                "{kind} webhook delivered (status {}, {} attempt(s))",
                outcome.status.unwrap_or(0),
                outcome.attempts
            )
        } else {
            format!(
                "{kind} webhook failed after {} attempt(s): {}",
                outcome.attempts,
                error.as_deref().unwrap_or("delivery failed")
            )
        };
        self.activity.record(
            &format!("gateway:{connector_id}"),
            "POST",
            &format!("/api/gateways/{connector_id}/outbound"),
            summary,
            ok,
            error.clone(),
        );
        if ok {
            tracing::info!(
                connector = connector_id,
                status = outcome.status.unwrap_or(0),
                attempts = outcome.attempts,
                "gateway webhook delivered"
            );
        } else {
            tracing::warn!(
                connector = connector_id,
                attempts = outcome.attempts,
                error = error.as_deref().unwrap_or("delivery failed"),
                "gateway webhook failed"
            );
        }
    }

    async fn post_signed(&self, outbound: &OutboundConfig, body: &[u8]) -> DeliveryOutcome {
        let Some(secret) = env_secret(&outbound.secret_env) else {
            return DeliveryOutcome {
                status: None,
                attempts: 0,
                error: Some(format!(
                    "environment variable '{}' is not set",
                    outbound.secret_env
                )),
            };
        };
        let signature = sign_body(&secret, body);
        let timeout = Duration::from_millis(outbound.timeout_ms.clamp(100, MAX_TIMEOUT_MS));
        let total = outbound.max_retries.saturating_add(1).min(MAX_RETRIES + 1);
        let mut attempts = 0;
        loop {
            attempts += 1;
            let response = self
                .client
                .post(&outbound.url)
                .header("Content-Type", "application/json")
                .header(SIGNATURE_HEADER, &signature)
                .timeout(timeout)
                .body(body.to_vec())
                .send()
                .await;
            match response {
                Ok(response) => {
                    let status = response.status().as_u16();
                    if (200..300).contains(&status) {
                        return DeliveryOutcome {
                            status: Some(status),
                            attempts,
                            error: None,
                        };
                    }
                    let error = format!("HTTP {status}");
                    if attempts >= total {
                        return DeliveryOutcome {
                            status: Some(status),
                            attempts,
                            error: Some(error),
                        };
                    }
                }
                Err(error) => {
                    let description = describe_request_error(&error);
                    if attempts >= total {
                        return DeliveryOutcome {
                            status: None,
                            attempts,
                            error: Some(description),
                        };
                    }
                }
            }
            let delay = RETRY_BASE_MS
                .saturating_mul(1u64 << (attempts.saturating_sub(1)).min(4))
                .min(RETRY_CAP_MS);
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
    }
}

/// Plain-language error without URLs (which may carry tokens) or response bodies.
fn describe_request_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "request timed out".to_string()
    } else if error.is_connect() {
        "connection failed".to_string()
    } else if error.is_redirect() {
        "too many redirects".to_string()
    } else if error.is_body() {
        "request body error".to_string()
    } else {
        "request failed".to_string()
    }
}

fn load_config(path: &Path) -> GatewaysFile {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<GatewaysFile>(&text) {
            Ok(mut file) => {
                file.version = version_one();
                let mut connectors = Vec::with_capacity(file.connectors.len());
                for mut connector in file.connectors {
                    match validate_connector(&mut connector, false) {
                        Ok(()) => connectors.push(connector),
                        Err(error) => tracing::warn!(
                            path = %path.display(),
                            connector = %connector.id,
                            error,
                            "ignoring invalid gateway connector in gateways.json"
                        ),
                    }
                }
                file.connectors = connectors;
                file
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "ignoring invalid gateways.json"
                );
                GatewaysFile::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => GatewaysFile::default(),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "cannot read gateways.json");
            GatewaysFile::default()
        }
    }
}

fn persist_config(path: &Path, file: &GatewaysFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("creating data dir: {e}"))?;
    }
    let body = serde_json::to_vec_pretty(file).map_err(|e| format!("serializing gateways: {e}"))?;
    std::fs::write(path, body).map_err(|e| format!("writing {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn set_env(name: &str, value: &str) {
        // Safety: each test uses a unique variable name, so tests never race on the same key;
        // the bridge only reads the variable (values are not aliased).
        unsafe { std::env::set_var(name, value) };
    }

    fn remove_env(name: &str) {
        // Safety: see `set_env`.
        unsafe { std::env::remove_var(name) };
    }

    fn event(channel: Option<&str>, direct: bool, text: &str) -> GatewayMessage {
        GatewayMessage {
            channel_id: channel.map(str::to_string),
            direct,
            message_id: 7,
            from: "0102030405060708".to_string(),
            from_name: "alice".to_string(),
            text: Some(text.to_string()),
            timestamp_ms: 1_700_000_000_000,
            attachment: None,
        }
    }

    fn manager(dir: &TempDir, file: GatewaysFile) -> Arc<GatewayManager> {
        GatewayManager::with_config(dir.path().to_path_buf(), Arc::new(ActivityLog::new()), file)
    }

    fn webhook_connector(id: &str, url: &str, secret_env: &str) -> Connector {
        Connector {
            id: id.to_string(),
            kind: "webhook".to_string(),
            enabled: true,
            outbound: Some(OutboundConfig {
                url: url.to_string(),
                secret_env: secret_env.to_string(),
                channel_ids: Vec::new(),
                include_direct: false,
                include_attachments: false,
                timeout_ms: 2_000,
                max_retries: 1,
            }),
            inbound: None,
        }
    }

    #[test]
    fn hmac_matches_rfc4231_vector() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex::encode(mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn signature_header_roundtrip_and_constant_time_reject() {
        let secret = b"top-secret";
        let body = br#"{"text":"hello"}"#;
        let header = sign_body(secret, body);
        assert!(header.starts_with("sha256="));
        assert!(verify_signature(secret, body, &header));
        // Case/whitespace tolerance in the prefix.
        assert!(verify_signature(
            secret,
            body,
            &header.replace("sha256=", "sha256= ")
        ));
        assert!(!verify_signature(secret, body, ""));
        assert!(!verify_signature(secret, body, "sha1=abcd"));
        assert!(!verify_signature(secret, body, "sha256=zz"));
        // A valid signature for a *different* secret must not verify.
        assert!(!verify_signature(
            secret,
            body,
            &sign_body(b"other-secret", body)
        ));
        assert!(!verify_signature(secret, br#"{"text":"bye"}"#, &header));
    }

    #[test]
    fn outbound_payload_truncates_text_and_includes_attachment_metadata_only() {
        let long = "x".repeat(MAX_TEXT_CHARS + 500);
        let mut message = event(Some("0102030405060708"), false, &long);
        message.attachment = Some(AttachmentMeta {
            mime: "image/webp".to_string(),
            size: 1234,
        });
        let payload = message_payload("sms-ops", &message, true);
        assert_eq!(
            payload["text"].as_str().unwrap().chars().count(),
            MAX_TEXT_CHARS
        );
        assert_eq!(payload["event"], "message");
        assert_eq!(payload["connector"], "sms-ops");
        assert_eq!(payload["attachment"]["mime"], "image/webp");
        assert_eq!(payload["attachment"]["size"], 1234);
        // Bytes are never included: the only attachment fields are mime/size.
        assert_eq!(payload["attachment"].as_object().unwrap().len(), 2);
        let without = message_payload("sms-ops", &message, false);
        assert!(without.get("attachment").is_none());
    }

    #[test]
    fn channel_filter_and_include_direct() {
        let mut outbound = webhook_connector("c", "http://127.0.0.1:1/x", "SECRET")
            .outbound
            .unwrap();
        // Empty channel_ids means "all channels".
        assert!(connector_accepts(
            &outbound,
            &event(Some("0102030405060708"), false, "a")
        ));
        assert!(!connector_accepts(&outbound, &event(None, true, "a")));
        outbound.include_direct = true;
        assert!(connector_accepts(&outbound, &event(None, true, "a")));
        outbound.channel_ids = vec!["0102030405060708".to_string()];
        assert!(connector_accepts(
            &outbound,
            &event(Some("0102030405060708"), false, "a")
        ));
        assert!(!connector_accepts(
            &outbound,
            &event(Some("1112131415161718"), false, "a")
        ));
    }

    #[tokio::test]
    async fn config_roundtrip_redacts_secrets_and_sets_mode_0600() {
        set_env("OPENMESH_TEST_GW_SECRET", "super-secret-value");
        let dir = TempDir::new().unwrap();
        let manager = manager(&dir, GatewaysFile::default());
        let body = json!({
            "id": "sms-ops",
            "kind": "webhook",
            "enabled": true,
            "outbound": {
                "url": "https://gateway.example/openmesh?api_key=provider-key",
                "secret_env": "OPENMESH_TEST_GW_SECRET",
                "channel_ids": [],
                "include_direct": true,
                "include_attachments": false,
                "timeout_ms": 10000,
                "max_retries": 3
            },
            "inbound": {
                "enabled": true,
                "secret_env": "OPENMESH_TEST_GW_SECRET",
                "channel_id": "0102030405060708",
                "max_chars": 400
            }
        });
        let view = manager.upsert("sms-ops", &body).await.unwrap();
        assert_eq!(view["id"], "sms-ops");
        assert_eq!(view["outbound"]["secret_set"], true);
        assert_eq!(view["inbound"]["secret_set"], true);
        let listed = manager.list_view().await.to_string();
        assert!(listed.contains("OPENMESH_TEST_GW_SECRET"));
        assert!(!listed.contains("super-secret-value"));
        assert!(
            !listed.contains("provider-key"),
            "URL query leaked: {listed}"
        );
        // Outbound URL query strings (which may hold provider keys) are redacted in API output.
        let view = manager.list_view().await;
        assert!(
            view["connectors"][0]["outbound"]["url"]
                .as_str()
                .unwrap()
                .ends_with("?<redacted>")
        );

        // Rendered config is persisted with mode 0600.
        let path = dir.path().join(GATEWAYS_FILE);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let stored = std::fs::read_to_string(&path).unwrap();
        assert!(stored.contains("OPENMESH_TEST_GW_SECRET"));
        assert!(!stored.contains("super-secret-value"));
        // The configured URL (including any query credentials) stays in the 0600 file only.
        assert!(stored.contains("provider-key"));
        remove_env("OPENMESH_TEST_GW_SECRET");
    }

    #[tokio::test]
    async fn upsert_rejects_inline_secrets_and_unknown_env_names() {
        remove_env("OPENMESH_TEST_GW_UNSET");
        let dir = TempDir::new().unwrap();
        let manager = manager(&dir, GatewaysFile::default());
        let inline = json!({
            "id": "bad",
            "outbound": {
                "url": "https://gateway.example/hook",
                "secret": "inline-value"
            }
        });
        let error = manager.upsert("bad", &inline).await.unwrap_err();
        assert!(error.contains("inline secret"), "{error}");

        let unknown_env = json!({
            "id": "bad",
            "outbound": {
                "url": "https://gateway.example/hook",
                "secret_env": "OPENMESH_TEST_GW_UNSET"
            }
        });
        let error = manager.upsert("bad", &unknown_env).await.unwrap_err();
        assert!(error.contains("is not set"), "{error}");

        set_env("OPENMESH_TEST_GW_UNSET", "now-set");
        let view = manager.upsert("bad", &unknown_env).await.unwrap();
        assert_eq!(view["outbound"]["secret_set"], true);
        remove_env("OPENMESH_TEST_GW_UNSET");
    }

    /// Minimal mock webhook server: reads one request per connection and answers with `status`.
    async fn mock_server(status: u16) -> (String, tokio::task::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let mut requests = 0usize;
            while let Ok((mut stream, _)) = listener.accept().await {
                requests += 1;
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 4096];
                // Read headers + body (Content-Length is small in these tests).
                loop {
                    let read = stream.read(&mut chunk).await.unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                    if let Some(split) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buffer[..split]).to_ascii_lowercase();
                        let length = head
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .and_then(|value| value.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if buffer.len() >= split + 4 + length {
                            break;
                        }
                    }
                }
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
            requests
        });
        (format!("http://{addr}/hook"), handle)
    }

    #[tokio::test]
    async fn outbound_success_posts_signed_json() {
        set_env("OPENMESH_TEST_GW_OUT", "hmac-key");
        let dir = TempDir::new().unwrap();
        let (url, server) = mock_server(200).await;
        let manager = manager(
            &dir,
            GatewaysFile {
                version: 1,
                connectors: vec![webhook_connector("hook", &url, "OPENMESH_TEST_GW_OUT")],
            },
        );
        manager.enqueue(event(Some("0102030405060708"), false, "hello mesh"));
        for _ in 0..100 {
            let status = manager.list_view().await;
            if status["connectors"][0]["status"]["deliveries"].as_u64() == Some(1) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let view = manager.list_view().await;
        assert_eq!(view["connectors"][0]["status"]["deliveries"], 1);
        assert_eq!(view["connectors"][0]["status"]["last_status"], 200);
        server.abort();
        remove_env("OPENMESH_TEST_GW_OUT");
    }

    #[tokio::test]
    async fn outbound_retries_on_500_then_gives_up() {
        set_env("OPENMESH_TEST_GW_RETRY", "hmac-key");
        let dir = TempDir::new().unwrap();
        let (url, server) = mock_server(500).await;
        let mut connector = webhook_connector("hook", &url, "OPENMESH_TEST_GW_RETRY");
        connector.outbound.as_mut().unwrap().max_retries = 1;
        let manager = manager(
            &dir,
            GatewaysFile {
                version: 1,
                connectors: vec![connector],
            },
        );
        manager.enqueue(event(Some("0102030405060708"), false, "hello mesh"));
        for _ in 0..200 {
            let status = manager.list_view().await;
            if status["connectors"][0]["status"]["failures"].as_u64() == Some(1) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let view = manager.list_view().await;
        assert_eq!(view["connectors"][0]["status"]["failures"], 1);
        assert_eq!(view["connectors"][0]["status"]["retries"], 1);
        assert!(
            view["connectors"][0]["status"]["last_error"]
                .as_str()
                .unwrap()
                .contains("HTTP 500")
        );
        // Both the initial attempt and the retry reached the server.
        tokio::time::sleep(Duration::from_millis(50)).await;
        server.abort();
        remove_env("OPENMESH_TEST_GW_RETRY");
    }

    #[tokio::test]
    async fn secret_never_leaks_into_activity_or_status() {
        set_env("OPENMESH_TEST_GW_LOG", "log-secret-value");
        let dir = TempDir::new().unwrap();
        let (url, server) = mock_server(500).await;
        let activity = Arc::new(ActivityLog::new());
        let mut connector = webhook_connector("hook", &url, "OPENMESH_TEST_GW_LOG");
        connector.outbound.as_mut().unwrap().max_retries = 0;
        let manager = GatewayManager::with_config(
            dir.path().to_path_buf(),
            activity.clone(),
            GatewaysFile {
                version: 1,
                connectors: vec![connector],
            },
        );
        manager.enqueue(event(Some("0102030405060708"), false, "hello"));
        for _ in 0..100 {
            if !activity.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let entries = serde_json::to_string(&activity.recent(10)).expect("activity json");
        assert!(entries.contains("gateway:hook"));
        assert!(!entries.contains("log-secret-value"));
        let listed = manager.list_view().await.to_string();
        assert!(!listed.contains("log-secret-value"));
        server.abort();
        remove_env("OPENMESH_TEST_GW_LOG");
    }

    #[tokio::test]
    async fn inbound_rate_limit_is_30_per_minute() {
        let dir = TempDir::new().unwrap();
        let manager = manager(&dir, GatewaysFile::default());
        for index in 0..INBOUND_RATE_LIMIT {
            assert!(
                manager
                    .allow_inbound("hook", INBOUND_RATE_LIMIT, Duration::from_secs(60))
                    .await,
                "request {index} should be allowed"
            );
        }
        assert!(
            !manager
                .allow_inbound("hook", INBOUND_RATE_LIMIT, Duration::from_secs(60))
                .await
        );
    }

    #[test]
    fn outbound_urls_are_redacted_for_display() {
        assert_eq!(
            redact_webhook_url("https://adapter.example/hook?api_key=secret"),
            "https://adapter.example/hook?<redacted>"
        );
        assert_eq!(
            redact_webhook_url("https://adapter.example/hook"),
            "https://adapter.example/hook"
        );
        assert!(!redact_webhook_url("https://a.example/x?token=s3cret").contains("s3cret"));
    }

    #[test]
    fn connector_ids_and_env_names_are_validated() {
        assert!(valid_connector_id("sms-ops"));
        assert!(valid_connector_id("a.b_c-1"));
        assert!(!valid_connector_id(""));
        assert!(!valid_connector_id("SMS Ops"));
        assert!(!valid_connector_id(&"x".repeat(65)));
        assert!(valid_env_name("OPENMESH_SMS_OUT_SECRET"));
        assert!(!valid_env_name("lowercase"));
        assert!(!valid_env_name("1STARTS_WITH_DIGIT"));
        assert!(!valid_env_name(""));
    }

    #[test]
    fn inbound_config_channel_id_must_be_a_node_id() {
        let mut connector = Connector {
            id: "ops".to_string(),
            kind: "webhook".to_string(),
            enabled: true,
            outbound: None,
            inbound: Some(InboundConfig {
                enabled: true,
                secret_env: "OPENMESH_TEST_IN".to_string(),
                channel_id: "not-a-node-id".to_string(),
                max_chars: 400,
            }),
        };
        assert!(validate_connector(&mut connector, false).is_err());
        connector.inbound.as_mut().unwrap().channel_id = "0102030405060708".to_string();
        assert!(validate_connector(&mut connector, false).is_ok());
    }
}

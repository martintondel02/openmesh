//! Remote MCP over HTTP (`docs/design/remote-operations.md` §1).
//!
//! Opt-in transport on the bridge's existing HTTP listener, reusing the stdio MCP dispatcher and
//! tool catalog. Loopback by default, bearer-token authenticated, read-only by default, with
//! session ids, bounded sessions, SSE responses/keep-alive and an activity-log audit of every
//! write tool call. Configuration lives in `<data-dir>/mcp-http.json` (0600).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::http::{HttpRequest, HttpState, authorized};
use crate::mcp;

/// Maximum JSON-RPC request body accepted by the MCP HTTP transport (256 KiB).
pub const MAX_MCP_BODY: usize = 256 * 1024;
/// Sessions unused for this long are pruned when a new one is created.
const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// SSE keep-alive interval (`GET /mcp`).
const PING_INTERVAL: Duration = Duration::from_secs(20);

fn default_path() -> String {
    "/mcp".to_string()
}

fn default_true() -> bool {
    true
}

fn default_rate_limit() -> u32 {
    60
}

fn default_max_sessions() -> usize {
    8
}

/// `<data-dir>/mcp-http.json` (mode 0600, disabled by default).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpHttpConfig {
    /// Expose the MCP HTTP endpoint on the bridge listener.
    #[serde(default)]
    pub enabled: bool,
    /// URL path of the endpoint (default `/mcp`).
    #[serde(default = "default_path")]
    pub path: String,
    /// Allow write tools (read-only scope by default).
    #[serde(default)]
    pub allow_write: bool,
    /// Keep the MCP `confirm:true` requirement for dangerous tools.
    #[serde(default = "default_true")]
    pub require_confirm: bool,
    /// Requests per rolling minute per session/token.
    #[serde(default = "default_rate_limit")]
    pub rate_limit_per_min: u32,
    /// Maximum concurrently tracked sessions.
    #[serde(default = "default_max_sessions")]
    pub max_sessions: usize,
}

impl Default for McpHttpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: default_path(),
            allow_write: false,
            require_confirm: true,
            rate_limit_per_min: default_rate_limit(),
            max_sessions: default_max_sessions(),
        }
    }
}

impl McpHttpConfig {
    /// Load (or create) `<data-dir>/mcp-http.json`. A corrupt file is tolerated: warn and return
    /// the disabled default.
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join("mcp-http.json");
        if !path.exists() {
            let config = Self::default();
            if let Err(e) = config.persist(&path) {
                tracing::warn!(error = %e, path = %path.display(), "could not write default mcp-http.json");
            }
            return config;
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Self>(&text) {
                Ok(config) => config.sanitized(),
                Err(e) => {
                    tracing::warn!(error = %e, path = %path.display(), "invalid mcp-http.json; remote MCP stays disabled");
                    Self::default()
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, path = %path.display(), "cannot read mcp-http.json; remote MCP stays disabled");
                Self::default()
            }
        }
    }

    fn sanitized(mut self) -> Self {
        if !self.path.starts_with('/') || self.path.contains('?') || self.path.contains('#') {
            tracing::warn!(path = %self.path, "invalid MCP HTTP path; using /mcp");
            self.path = default_path();
        }
        if self.path.len() > 200 {
            self.path = self.path.chars().take(200).collect();
        }
        self.rate_limit_per_min = self.rate_limit_per_min.clamp(1, 600);
        self.max_sessions = self.max_sessions.clamp(1, 64);
        self
    }

    fn persist(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        std::fs::write(path, format!("{text}\n"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Session {
    last_seen: Instant,
    window_start: Instant,
    requests_in_window: u32,
}

/// Runtime state for the remote MCP transport (config + bounded session table).
#[derive(Debug)]
pub struct McpHttp {
    pub config: McpHttpConfig,
    sessions: Mutex<HashMap<String, Session>>,
    next_id: AtomicU64,
}

#[derive(Debug, PartialEq, Eq)]
enum SessionError {
    TooManySessions,
    RateLimited,
}

impl McpHttp {
    /// Load the config and build the runtime state.
    pub fn load(data_dir: &Path) -> Self {
        let config = McpHttpConfig::load(data_dir);
        if config.enabled {
            tracing::info!(
                path = %config.path,
                allow_write = config.allow_write,
                "remote MCP HTTP enabled (loopback by default; bearer token required)"
            );
        }
        Self::new(config)
    }

    pub fn new(config: McpHttpConfig) -> Self {
        Self {
            config,
            sessions: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Whether this request path is the configured MCP endpoint.
    pub fn matches(&self, path: &str) -> bool {
        self.config.enabled && path == self.config.path
    }

    pub fn path(&self) -> &str {
        &self.config.path
    }

    fn generate_id(&self) -> String {
        let counter = self.next_id.fetch_add(1, Ordering::Relaxed);
        let random = rand::random::<u64>();
        format!("mcp-{counter:08x}-{random:016x}")
    }

    /// Resolve/generate a session id, enforce `max_sessions` and the per-session rate limit.
    fn touch_session(&self, requested: Option<&str>) -> Result<String, SessionError> {
        let now = Instant::now();
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Prune idle sessions so long-lived bridges do not leak session slots.
        sessions.retain(|_, session| now.duration_since(session.last_seen) < SESSION_IDLE_TIMEOUT);
        let id = match requested.filter(|id| !id.trim().is_empty()) {
            Some(id) => id.trim().to_string(),
            None => self.generate_id(),
        };
        if !sessions.contains_key(&id) {
            if sessions.len() >= self.config.max_sessions {
                return Err(SessionError::TooManySessions);
            }
            sessions.insert(
                id.clone(),
                Session {
                    last_seen: now,
                    window_start: now,
                    requests_in_window: 0,
                },
            );
        }
        let session = sessions.get_mut(&id).expect("session inserted above");
        session.last_seen = now;
        if now.duration_since(session.window_start) >= Duration::from_secs(60) {
            session.window_start = now;
            session.requests_in_window = 0;
        }
        session.requests_in_window += 1;
        if session.requests_in_window > self.config.rate_limit_per_min {
            return Err(SessionError::RateLimited);
        }
        Ok(id)
    }

    fn end_session(&self, id: &str) -> bool {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(id)
            .is_some()
    }
}

// ---------------------------------------------------------------------------------------------
// HTTP handling
// ---------------------------------------------------------------------------------------------

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        _ => "Unknown",
    }
}

fn jsonrpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn jsonrpc_error_data(id: &Value, code: i64, message: &str, data: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message, "data": data }
    })
}

async fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    extra_headers: &[(String, String)],
    body: &[u8],
) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n",
        status_text(status),
        body.len()
    );
    for (name, value) in extra_headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await
}

fn agent_source(request: &HttpRequest) -> String {
    match request
        .header("x-openmesh-agent")
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        Some(name) => {
            let name: String = name
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
                .take(48)
                .collect();
            if name.is_empty() {
                "agent:mcp-http".to_string()
            } else {
                format!("agent:{name}")
            }
        }
        None => "agent:mcp-http".to_string(),
    }
}

fn audit_write(
    state: &HttpState,
    source: &str,
    path: &str,
    tool: &str,
    ok: bool,
    error: Option<String>,
) {
    state
        .activity
        .record(source, "MCP", path, format!("mcp tool {tool}"), ok, error);
}

fn response_is_error(response: &Option<String>) -> (bool, Option<String>) {
    let Some(text) = response else {
        return (true, None);
    };
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return (false, None);
    };
    if value.get("result").is_some() {
        let result = &value["result"];
        if result.get("isError").and_then(|v| v.as_bool()) == Some(true) {
            let message = result
                .pointer("/structuredContent/error/message")
                .and_then(|v| v.as_str())
                .unwrap_or("tool failed")
                .chars()
                .take(200)
                .collect();
            return (false, Some(message));
        }
        return (true, None);
    }
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("JSON-RPC error")
            .chars()
            .take(200)
            .collect();
        return (false, Some(message));
    }
    (false, None)
}

/// Handle one request to the configured MCP HTTP endpoint.
pub async fn handle(
    stream: &mut TcpStream,
    request: &HttpRequest,
    state: &Arc<HttpState>,
) -> std::io::Result<()> {
    let path = state.mcp_http.path().to_string();
    if !state.mcp_http.config.enabled {
        return write_response(
            stream,
            404,
            "application/json",
            &[],
            br#"{"error":"not_found","message":"remote MCP is disabled (enable it in mcp-http.json)"}"#,
        )
        .await;
    }
    // Bearer auth reuses the bridge token; wrong/missing never executes anything.
    if !authorized(request, &state.token) {
        let body = jsonrpc_error(
            &Value::Null,
            -32001,
            "unauthorized: missing or invalid bridge API token",
        );
        return write_response(
            stream,
            401,
            "application/json",
            &[],
            body.to_string().as_bytes(),
        )
        .await;
    }
    match request.method.as_str() {
        "POST" => handle_post(stream, request, state, &path).await,
        "GET" => handle_get(stream, request, state, &path).await,
        "DELETE" => handle_delete(stream, request, state).await,
        _ => {
            let body = jsonrpc_error(&Value::Null, -32600, "method not allowed");
            write_response(
                stream,
                405,
                "application/json",
                &[],
                body.to_string().as_bytes(),
            )
            .await
        }
    }
}

fn session_headers(session: Option<&str>) -> Vec<(String, String)> {
    session
        .map(|id| vec![("Mcp-Session-Id".to_string(), id.to_string())])
        .unwrap_or_default()
}

async fn handle_post(
    stream: &mut TcpStream,
    request: &HttpRequest,
    state: &Arc<HttpState>,
    path: &str,
) -> std::io::Result<()> {
    if request.body.len() > MAX_MCP_BODY {
        let body = jsonrpc_error(
            &Value::Null,
            -32600,
            "request body exceeds the 256 KiB MCP HTTP limit",
        );
        return write_response(
            stream,
            413,
            "application/json",
            &[],
            body.to_string().as_bytes(),
        )
        .await;
    }
    let requested = request.header("mcp-session-id").map(str::to_string);
    let session = match state.mcp_http.touch_session(requested.as_deref()) {
        Ok(session) => session,
        Err(SessionError::TooManySessions) => {
            let body = jsonrpc_error(
                &Value::Null,
                -32000,
                "too many MCP sessions; retry later or reuse an existing Mcp-Session-Id",
            );
            return write_response(
                stream,
                429,
                "application/json",
                &[],
                body.to_string().as_bytes(),
            )
            .await;
        }
        Err(SessionError::RateLimited) => {
            let body = jsonrpc_error(
                &Value::Null,
                -32000,
                "MCP HTTP rate limit exceeded for this session; retry later",
            );
            return write_response(
                stream,
                429,
                "application/json",
                &session_headers(requested.as_deref()),
                body.to_string().as_bytes(),
            )
            .await;
        }
    };

    let text = String::from_utf8_lossy(&request.body).to_string();
    let parsed: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(e) => {
            let body = jsonrpc_error(&Value::Null, -32700, &format!("parse error: {e}"));
            return write_response(
                stream,
                400,
                "application/json",
                &session_headers(Some(&session)),
                body.to_string().as_bytes(),
            )
            .await;
        }
    };
    let method = parsed.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let request_id = parsed.get("id").cloned().unwrap_or(Value::Null);
    let source = agent_source(request);
    let mut line = parsed.clone();

    // Scope gate: write tools are refused unless `allow_write`; keep confirm rules otherwise.
    let mut audited_tool: Option<(String, bool, Option<String>)> = None;
    if method == "tools/call" {
        let tool_name = parsed
            .pointer("/params/name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if let Some((is_write, _requires_confirm)) = mcp::tool_flags(tool_name) {
            if is_write {
                if !state.mcp_http.config.allow_write {
                    audit_write(
                        state,
                        &source,
                        path,
                        tool_name,
                        false,
                        Some("http_scope_read_only".to_string()),
                    );
                    let body = jsonrpc_error_data(
                        &request_id,
                        -32000,
                        "write tools are disabled on this remote MCP endpoint (read-only scope)",
                        json!({
                            "code": "http_scope_read_only",
                            "help_topic": "agents",
                            "hint": "set allow_write:true in mcp-http.json and restart the bridge",
                        }),
                    );
                    return write_response(
                        stream,
                        200,
                        "application/json",
                        &session_headers(Some(&session)),
                        body.to_string().as_bytes(),
                    )
                    .await;
                }
                // `require_confirm:false` opts out of the confirm:true gate for this trusted
                // endpoint (dangerous tools and conditional confirmations such as a regulatory
                // override on openmesh_set_config).
                if !state.mcp_http.config.require_confirm {
                    if let Some(arguments) = line
                        .pointer_mut("/params/arguments")
                        .and_then(|value| value.as_object_mut())
                    {
                        arguments.insert("confirm".into(), Value::Bool(true));
                    }
                }
                audited_tool = Some((tool_name.to_string(), false, None));
            }
        }
    }

    let mut server = mcp::McpServer::embedded(
        format!("http://{}", state.local_addr),
        state.token.to_string(),
        source
            .strip_prefix("agent:")
            .unwrap_or("mcp-http")
            .to_string(),
        false, // HTTP scope is enforced above; the shared dispatcher stays writable
        30,
    );
    let response = server.handle_line(&line.to_string()).await;
    if let Some((tool, _, _)) = audited_tool {
        let (ok, error) = response_is_error(&response);
        audit_write(state, &source, path, &tool, ok, error);
    }

    let Some(body) = response else {
        // JSON-RPC notification: accepted with no body.
        return write_response(
            stream,
            202,
            "application/json",
            &session_headers(Some(&session)),
            &[],
        )
        .await;
    };

    let wants_sse = request
        .header("accept")
        .map(|value| value.to_ascii_lowercase().contains("text/event-stream"))
        .unwrap_or(false);
    if wants_sse {
        let frame = format!("data: {body}\n\n");
        write_response(
            stream,
            200,
            "text/event-stream",
            &session_headers(Some(&session)),
            frame.as_bytes(),
        )
        .await
    } else {
        write_response(
            stream,
            200,
            "application/json",
            &session_headers(Some(&session)),
            body.as_bytes(),
        )
        .await
    }
}

async fn handle_get(
    stream: &mut TcpStream,
    request: &HttpRequest,
    state: &Arc<HttpState>,
    _path: &str,
) -> std::io::Result<()> {
    let session = match state
        .mcp_http
        .touch_session(request.header("mcp-session-id"))
    {
        Ok(session) => session,
        Err(SessionError::TooManySessions) | Err(SessionError::RateLimited) => {
            let body = jsonrpc_error(
                &Value::Null,
                -32000,
                "MCP session limit reached; reuse an Mcp-Session-Id or retry later",
            );
            return write_response(
                stream,
                429,
                "application/json",
                &[],
                body.to_string().as_bytes(),
            )
            .await;
        }
    };
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nX-Accel-Buffering: no\r\nMcp-Session-Id: {session}\r\n\r\n"
    );
    stream.write_all(head.as_bytes()).await?;
    stream.flush().await?;
    let mut shutdown = state.shutdown.clone();
    loop {
        let ping = "event: ping\ndata: {}\n\n";
        if stream.write_all(ping.as_bytes()).await.is_err() {
            break;
        }
        if stream.flush().await.is_err() {
            break;
        }
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tokio::time::sleep(PING_INTERVAL) => continue,
        }
    }
    Ok(())
}

async fn handle_delete(
    stream: &mut TcpStream,
    request: &HttpRequest,
    state: &Arc<HttpState>,
) -> std::io::Result<()> {
    let session = request.header("mcp-session-id").map(str::to_string);
    let ended = session
        .as_deref()
        .map(|id| state.mcp_http.end_session(id))
        .unwrap_or(false);
    let body = json!({
        "jsonrpc": "2.0",
        "id": Value::Null,
        "result": { "ended": ended }
    });
    write_response(
        stream,
        200,
        "application/json",
        &session_headers(session.as_deref()),
        body.to_string().as_bytes(),
    )
    .await
}

/// Path of the default config file (for docs/tests).
pub fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("mcp-http.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_disabled_and_sanitizes() {
        let config = McpHttpConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.path, "/mcp");
        assert!(!config.allow_write);
        assert!(config.require_confirm);
        assert_eq!(config.rate_limit_per_min, 60);
        assert_eq!(config.max_sessions, 8);

        let sanitized = McpHttpConfig {
            path: "mcp".into(),
            rate_limit_per_min: 0,
            max_sessions: 0,
            ..McpHttpConfig::default()
        }
        .sanitized();
        assert_eq!(sanitized.path, "/mcp");
        assert_eq!(sanitized.rate_limit_per_min, 1);
        assert_eq!(sanitized.max_sessions, 1);
    }

    #[test]
    fn sessions_are_bounded_and_rate_limited() {
        let http = McpHttp::new(McpHttpConfig {
            enabled: true,
            rate_limit_per_min: 2,
            max_sessions: 1,
            ..McpHttpConfig::default()
        });
        let first = http.touch_session(None).expect("first session");
        assert!(first.starts_with("mcp-"));
        // Same session: rate limit after 2 requests.
        http.touch_session(Some(&first)).expect("second request");
        assert_eq!(
            http.touch_session(Some(&first)).unwrap_err(),
            SessionError::RateLimited
        );
        // A different session id over the active-session cap is rejected.
        assert_eq!(
            http.touch_session(Some("other")).unwrap_err(),
            SessionError::TooManySessions
        );
        // Ending the session frees the slot.
        assert!(http.end_session(&first));
        assert!(http.touch_session(Some("other")).is_ok());
    }
}

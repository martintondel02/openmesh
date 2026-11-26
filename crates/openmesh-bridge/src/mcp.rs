//! `openmesh-bridge mcp` — Model Context Protocol server on stdio.
//!
//! A thin, honest client of the local bridge REST API (`docs/architecture/AI-NATIVE.md` §1): it
//! holds no state, never touches identity keys, and writes only JSON-RPC lines to stdout (all
//! diagnostics go to stderr). Mutating tools are attributed through the `X-OpenMesh-Agent`
//! header so the bridge activity log shows exactly what an agent did.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::Instant;

use openmesh_core::OVERRIDE_WARNING;

use crate::help;

/// MCP protocol revision used when the client does not send one.
const DEFAULT_PROTOCOL_VERSION: &str = "2024-11-05";
/// Per-HTTP-call timeout required by the contract.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// Default write budget: 30 mutating tool calls per rolling minute.
const WRITE_LIMIT: usize = 30;

// ---------------------------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct McpConfig {
    pub api: String,
    pub token: Option<String>,
    pub read_only: bool,
    pub agent_name: String,
    pub write_limit: usize,
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

impl McpConfig {
    pub fn from_env() -> Result<Self> {
        let api = std::env::var("OPENMESH_API")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "http://127.0.0.1:8787".to_string())
            .trim_end_matches('/')
            .to_string();
        let token = resolve_token()?;
        let read_only = env_truthy("OPENMESH_MCP_READ_ONLY") || env_truthy("MCP_READ_ONLY");
        let agent_name = std::env::var("OPENMESH_AGENT_NAME")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| std::env::var("HOSTNAME").ok())
            .unwrap_or_else(|| "agent".to_string());
        let agent_name: String = agent_name
            .trim()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
            .take(48)
            .collect();
        let agent_name = if agent_name.is_empty() {
            "agent".to_string()
        } else {
            agent_name
        };
        let write_limit = std::env::var("OPENMESH_MCP_WRITE_LIMIT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|limit| *limit > 0)
            .unwrap_or(WRITE_LIMIT);
        Ok(Self {
            api,
            token,
            read_only,
            agent_name,
            write_limit,
        })
    }
}

fn resolve_token() -> Result<Option<String>> {
    if let Some(token) = std::env::var("OPENMESH_TOKEN")
        .ok()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
    {
        return Ok(Some(token));
    }
    let path = std::env::var("OPENMESH_TOKEN_FILE")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(crate::token::default_token_path);
    match std::fs::read_to_string(&path) {
        Ok(token) if !token.trim().is_empty() => Ok(Some(token.trim().to_string())),
        Ok(_) => Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading token file {}", path.display())),
    }
}

// ---------------------------------------------------------------------------------------------
// Minimal HTTP client
// ---------------------------------------------------------------------------------------------

#[derive(Debug)]
struct HttpOutcome {
    value: Value,
}

#[derive(Debug, Clone)]
struct ToolError {
    code: String,
    message: String,
    help_topic: String,
}

impl ToolError {
    fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        help_topic: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            help_topic: help_topic.into(),
        }
    }
}

struct HttpClient {
    api: String,
    token: Option<String>,
    agent_name: String,
}

impl HttpClient {
    fn new(config: &McpConfig) -> Self {
        Self {
            api: config.api.clone(),
            token: config.token.clone(),
            agent_name: config.agent_name.clone(),
        }
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<HttpOutcome, ToolError> {
        if !self.api.starts_with("http://") {
            return Err(ToolError::new(
                "unreachable",
                format!(
                    "only http:// is supported for the local bridge API, got {}",
                    self.api
                ),
                "troubleshooting",
            ));
        }
        let authority = self.api.trim_start_matches("http://");
        let (authority, prefix) = match authority.split_once('/') {
            Some((authority, prefix)) => (authority, format!("/{prefix}")),
            None => (authority, String::new()),
        };
        let Some(token) = self.token.as_deref() else {
            return Err(ToolError::new(
                "unauthorized",
                "no bridge API token configured: set OPENMESH_TOKEN or OPENMESH_TOKEN_FILE",
                "agents",
            ));
        };
        let payload = body.map(|body| body.to_string()).unwrap_or_default();
        let mut request = format!(
            "{method} {prefix}{path} HTTP/1.1\r\nHost: {authority}\r\nAuthorization: Bearer {token}\r\nX-OpenMesh-Agent: {agent}\r\nX-OpenMesh-Client: mcp\r\nConnection: close\r\n",
            agent = self.agent_name,
        );
        if !payload.is_empty() {
            request.push_str("Content-Type: application/json\r\n");
            request.push_str(&format!("Content-Length: {}\r\n", payload.len()));
        }
        request.push_str("\r\n");
        request.push_str(&payload);

        let connect = TcpStream::connect(authority);
        let mut stream = match tokio::time::timeout(HTTP_TIMEOUT, connect).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(e)) => {
                return Err(ToolError::new(
                    "unreachable",
                    format!(
                        "cannot reach the bridge API at {} ({e}); is `openmesh-bridge` running?",
                        self.api
                    ),
                    "troubleshooting",
                ));
            }
            Err(_) => {
                return Err(ToolError::new(
                    "timeout",
                    format!("connecting to the bridge API at {} timed out", self.api),
                    "troubleshooting",
                ));
            }
        };
        let exchange = async {
            stream.write_all(request.as_bytes()).await?;
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await?;
            Ok::<Vec<u8>, std::io::Error>(response)
        };
        let response = match tokio::time::timeout(HTTP_TIMEOUT, exchange).await {
            Ok(Ok(response)) => response,
            Ok(Err(e)) => {
                return Err(ToolError::new(
                    "unreachable",
                    format!("bridge API request failed: {e}"),
                    "troubleshooting",
                ));
            }
            Err(_) => {
                return Err(ToolError::new(
                    "timeout",
                    "the bridge API did not answer within 10 seconds",
                    "troubleshooting",
                ));
            }
        };
        let split = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap_or(response.len());
        let head = String::from_utf8_lossy(&response[..split.min(response.len())]).to_string();
        let body = if split + 4 <= response.len() {
            &response[split + 4..]
        } else {
            &[]
        };
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .unwrap_or(0);
        let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
        if status != 0 && !(200..300).contains(&status) {
            return Err(http_error(status, &value));
        }
        Ok(HttpOutcome { value })
    }
}

fn http_error(status: u16, value: &Value) -> ToolError {
    let body_code = value.get("code").and_then(|v| v.as_str());
    let message = value
        .get("message")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("bridge API returned HTTP {status}"));
    let (code, help_topic) = match (status, body_code) {
        (401 | 403, _) => ("unauthorized", "agents"),
        (404, Some(code)) => (code, "troubleshooting"),
        (404, None) => ("not_found", "troubleshooting"),
        (400, Some(code)) => (code, "getting-started"),
        (409, Some(code)) => (code, "radio-and-region"),
        (429, _) => ("rate_limited", "agents"),
        (503, _) => ("unavailable", "troubleshooting"),
        (_, Some(code)) => (code, "troubleshooting"),
        _ => ("http_error", "troubleshooting"),
    };
    ToolError::new(code, message, help_topic)
}

/// Remove bearer secrets from strings before they reach a model.
fn redact_secrets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for token in text.split_inclusive('&') {
        if let Some((key, _value)) = token.split_once('=') {
            if matches!(key, "token" | "psk" | "identity" | "seed") {
                out.push_str(key);
                out.push_str("=<redacted-by-mcp>");
                if token.ends_with('&') {
                    out.push('&');
                }
                continue;
            }
        }
        out.push_str(token);
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Tool catalog (AI-NATIVE.md §1.1)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct ToolDef {
    name: &'static str,
    description: &'static str,
    write: bool,
    confirm: bool,
    input_schema: Value,
}

fn schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn tool_catalog() -> Vec<ToolDef> {
    let node_id = || json!({ "type": "string", "description": "8-byte node id as hex" });
    let msg_id = || json!({ "type": "integer", "description": "message id" });
    vec![
        ToolDef {
            name: "openmesh_health",
            description: "Bridge and API version, uptime and whether a radio is connected. Start here if something seems broken.",
            write: false,
            confirm: false,
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "openmesh_status",
            description: "Node identity summary, radio region/profile, airtime usage and queues.",
            write: false,
            confirm: false,
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "openmesh_nodes",
            description: "Known nodes with class, capabilities, hop count, signal, battery and last-seen time.",
            write: false,
            confirm: false,
            input_schema: schema(
                json!({
                    "role": { "type": "string", "enum": ["client", "repeater", "gateway", "tracker", "room_server"] },
                    "has_position": { "type": "boolean" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500 }
                }),
                &[],
            ),
        },
        ToolDef {
            name: "openmesh_node",
            description: "Details for one node plus whether it is a saved contact.",
            write: false,
            confirm: false,
            input_schema: schema(json!({ "node_id": node_id() }), &["node_id"]),
        },
        ToolDef {
            name: "openmesh_messages",
            description: "Recent direct messages with a peer, or channel messages.",
            write: false,
            confirm: false,
            input_schema: schema(
                json!({
                    "peer": node_id(),
                    "channel": { "type": "string", "description": "channel id as hex" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500 },
                    "before_ms": { "type": "integer" }
                }),
                &[],
            ),
        },
        ToolDef {
            name: "openmesh_channels",
            description: "Joined channels. PSKs are never returned.",
            write: false,
            confirm: false,
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "openmesh_contacts",
            description: "Saved contacts with identity verification state.",
            write: false,
            confirm: false,
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "openmesh_backbone_status",
            description: "Backbone configuration (token redacted), mode and totals.",
            write: false,
            confirm: false,
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "openmesh_backbone_peers",
            description: "Connected backbone peers with frame/byte counters.",
            write: false,
            confirm: false,
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "openmesh_backbone_setup",
            description: "Backbone onboarding payload: listen address, steps, firewall commands and systemd unit. The connection-string token is redacted; copy the full string from the web panel.",
            write: false,
            confirm: false,
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "openmesh_setup_state",
            description: "First-run/checklist state, secure-setup recommendations and counts.",
            write: false,
            confirm: false,
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "openmesh_activity",
            description: "Recent mutating actions with their source (user or agent). No message bodies.",
            write: false,
            confirm: false,
            input_schema: schema(
                json!({ "limit": { "type": "integer", "minimum": 1, "maximum": 256 } }),
                &[],
            ),
        },
        ToolDef {
            name: "openmesh_help",
            description: "Help index, or one topic's markdown (use the slugs from the index).",
            write: false,
            confirm: false,
            input_schema: schema(
                json!({
                    "topic": { "type": "string" },
                    "audience": { "type": "string", "enum": ["beginner", "advanced", "both"] }
                }),
                &[],
            ),
        },
        ToolDef {
            name: "openmesh_send_message",
            description: "Send an end-to-end encrypted direct message. Requires the recipient to be known (announce / contact).",
            write: true,
            confirm: false,
            input_schema: schema(
                json!({
                    "to": node_id(),
                    "text": { "type": "string" },
                    "reply_to": msg_id(),
                    "attachment_id": { "type": "string" }
                }),
                &["to"],
            ),
        },
        ToolDef {
            name: "openmesh_send_channel_message",
            description: "Send a message to a joined channel.",
            write: true,
            confirm: false,
            input_schema: schema(
                json!({
                    "channel": { "type": "string", "description": "channel id as hex" },
                    "text": { "type": "string" },
                    "attachment_id": { "type": "string" }
                }),
                &["channel"],
            ),
        },
        ToolDef {
            name: "openmesh_share_position",
            description: "Share the node's configured position once, with a peer or a channel.",
            write: true,
            confirm: false,
            input_schema: schema(
                json!({
                    "to": node_id(),
                    "channel": { "type": "string" }
                }),
                &[],
            ),
        },
        ToolDef {
            name: "openmesh_announce",
            description: "Force a mesh announce now (idempotent).",
            write: true,
            confirm: false,
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "openmesh_mark_read",
            description: "Send a read receipt for a message.",
            write: true,
            confirm: false,
            input_schema: schema(json!({ "msg_id": msg_id() }), &["msg_id"]),
        },
        ToolDef {
            name: "openmesh_retry_message",
            description: "Retry delivery of a message you sent.",
            write: true,
            confirm: false,
            input_schema: schema(json!({ "msg_id": msg_id() }), &["msg_id"]),
        },
        ToolDef {
            name: "openmesh_set_config",
            description: "Apply a config patch (node name/class, radio region/profile, feature toggles) live. Emergency regulatory overrides are accepted as `regulatory_override` and require confirm:true when they enable or exceed the legal maxima; the result includes the warning string.",
            write: true,
            confirm: false,
            input_schema: schema(
                json!({
                    "patch": { "type": "object", "description": "partial config: {node,radio,features,position}" },
                    "regulatory_override": {
                        "type": "object",
                        "description": "Emergency override: {enabled, acknowledged/confirm, duty_cycle_pct, max_power_dbm, frequency_khz, reason}. Enabling/exceeding requires confirm:true; disabling is always allowed.",
                        "properties": {
                            "enabled": { "type": "boolean" },
                            "acknowledged": { "type": "boolean" },
                            "confirm": { "type": "boolean" },
                            "duty_cycle_pct": { "type": "integer", "minimum": 0, "maximum": 100 },
                            "max_power_dbm": { "type": "integer" },
                            "frequency_khz": { "type": "integer" },
                            "reason": { "type": "string", "maxLength": 120 }
                        }
                    },
                    "confirm": { "type": "boolean", "description": "Required to enable the override or exceed legal maxima" }
                }),
                &[],
            ),
        },
        ToolDef {
            name: "openmesh_setup_quick",
            description: "Apply the secure beginner preset for a role and country: region, profile, privacy, announce, and (optionally) a private channel invite. Idempotent.",
            write: true,
            confirm: false,
            input_schema: schema(
                json!({
                    "role": { "type": "string", "enum": ["client", "tracker", "repeater", "gateway"] },
                    "country": { "type": "string", "description": "ISO country code, e.g. NO" },
                    "node_name": { "type": "string" },
                    "privacy": { "type": "string", "enum": ["private", "shared"] },
                    "create_channel": { "type": "boolean" }
                }),
                &["role", "country"],
            ),
        },
        ToolDef {
            name: "openmesh_create_channel",
            description: "Create a private channel. The PSK and invite URL are returned once — store them, do not paste secrets into chat logs.",
            write: true,
            confirm: false,
            input_schema: schema(
                json!({
                    "name": { "type": "string" },
                    "psk": { "type": "string", "description": "optional base64 32-byte key (otherwise generated)" }
                }),
                &["name"],
            ),
        },
        ToolDef {
            name: "openmesh_join_channel",
            description: "Join a channel from an invite URL or a name + base64 PSK.",
            write: true,
            confirm: false,
            input_schema: schema(
                json!({
                    "url": { "type": "string" },
                    "name": { "type": "string" },
                    "psk": { "type": "string" }
                }),
                &[],
            ),
        },
        ToolDef {
            name: "openmesh_add_contact",
            description: "Add a contact from a contact invite URL or an explicit identity.",
            write: true,
            confirm: false,
            input_schema: schema(
                json!({
                    "invite_url": { "type": "string" },
                    "identity": { "type": "string" },
                    "node_id": node_id(),
                    "name": { "type": "string" }
                }),
                &[],
            ),
        },
        ToolDef {
            name: "openmesh_backbone_config",
            description: "Change backbone mode configuration live (enable/disable, port, auth, allowlist, store-forward).",
            write: true,
            confirm: false,
            input_schema: schema(
                json!({ "patch": { "type": "object", "description": "partial backbone config" } }),
                &["patch"],
            ),
        },
        ToolDef {
            name: "openmesh_rotate_backbone_token",
            description: "⚠ Destructive: rotate the backbone token. Requires confirm:true; the new token is returned once.",
            write: true,
            confirm: true,
            input_schema: schema(
                json!({
                    "confirm": { "type": "boolean" },
                    "kick_peers": { "type": "boolean" }
                }),
                &["confirm"],
            ),
        },
        ToolDef {
            name: "openmesh_kick_backbone_peer",
            description: "⚠ Destructive: disconnect a backbone peer. Requires confirm:true.",
            write: true,
            confirm: true,
            input_schema: schema(
                json!({
                    "node_id": node_id(),
                    "confirm": { "type": "boolean" }
                }),
                &["node_id", "confirm"],
            ),
        },
    ]
}

/// `(is_write, requires_confirm)` for a tool in the shared catalog (`None` if unknown).
pub(crate) fn tool_flags(name: &str) -> Option<(bool, bool)> {
    tool_catalog()
        .into_iter()
        .find(|tool| tool.name == name)
        .map(|tool| (tool.write, tool.confirm))
}

fn tools_list_json(read_only: bool) -> Value {
    let tools: Vec<Value> = tool_catalog()
        .into_iter()
        .filter(|tool| !(read_only && tool.write))
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": tool.input_schema,
                "annotations": {
                    "readOnlyHint": !tool.write,
                    "destructiveHint": tool.confirm,
                },
            })
        })
        .collect();
    json!({ "tools": tools })
}

// ---------------------------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------------------------

pub async fn run_stdio() -> Result<()> {
    let config = McpConfig::from_env()?;
    let mut server = McpServer {
        http: HttpClient::new(&config),
        read_only: config.read_only,
        write_limit: config.write_limit,
        write_times: VecDeque::new(),
    };
    eprintln!(
        "openmesh-bridge mcp: serving MCP on stdio (api={}, read_only={}, agent={})",
        redact_api(&config.api),
        config.read_only,
        config.agent_name
    );
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(e) => {
                eprintln!("openmesh-bridge mcp: stdin error: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let Some(response) = server.handle_line(&line).await else {
            continue;
        };
        stdout.write_all(response.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }
    Ok(())
}

fn redact_api(api: &str) -> String {
    api.to_string()
}

pub(crate) struct McpServer {
    http: HttpClient,
    read_only: bool,
    write_limit: usize,
    write_times: VecDeque<Instant>,
}

impl McpServer {
    /// Build an in-process-dispatcher handle for the HTTP transport. `api`/`token` point at the
    /// local bridge so tool calls reuse the same REST path as stdio MCP.
    pub(crate) fn embedded(
        api: String,
        token: String,
        agent_name: String,
        read_only: bool,
        write_limit: usize,
    ) -> Self {
        Self {
            http: HttpClient {
                api,
                token: Some(token),
                agent_name,
            },
            read_only,
            write_limit,
            write_times: VecDeque::new(),
        }
    }

    pub(crate) async fn handle_line(&mut self, line: &str) -> Option<String> {
        let request: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(e) => {
                return Some(error_response(
                    &Value::Null,
                    -32700,
                    &format!("parse error: {e}"),
                ));
            }
        };
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");
        if method.is_empty() {
            let id = id?;
            return Some(error_response(
                &id,
                -32600,
                "invalid request: missing method",
            ));
        }
        // Notifications (no id) never get a response.
        if method.starts_with("notifications/") {
            return None;
        }
        let id = id?;
        let params = request.get("params").cloned().unwrap_or(Value::Null);
        match method {
            "initialize" => Some(result_response(&id, self.initialize(&params))),
            "ping" => Some(result_response(&id, json!({}))),
            "tools/list" => Some(result_response(&id, tools_list_json(self.read_only))),
            "tools/call" => Some(self.tools_call(&id, &params).await),
            "resources/list" => Some(result_response(&id, resources_list_json())),
            "resources/read" => Some(self.resources_read(&id, &params).await),
            "prompts/list" => Some(result_response(&id, prompts_list_json())),
            "prompts/get" => Some(self.prompts_get(&id, &params).await),
            _ => Some(error_response(
                &id,
                -32601,
                &format!("method not found: {method}"),
            )),
        }
    }

    fn initialize(&self, params: &Value) -> Value {
        let protocol_version = params
            .get("protocolVersion")
            .and_then(|v| v.as_str())
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_PROTOCOL_VERSION);
        json!({
            "protocolVersion": protocol_version,
            "capabilities": {
                "tools": { "listChanged": false },
                "resources": { "subscribe": false, "listChanged": false },
                "prompts": { "listChanged": false },
            },
            "serverInfo": {
                "name": "openmesh",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": "Operate a local OpenMesh bridge. Start with openmesh_status, openmesh_setup_state and openmesh_help. Destructive tools require confirm:true; never paste tokens or PSKs into chat logs.",
        })
    }

    async fn tools_call(&mut self, id: &Value, params: &Value) -> String {
        let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
        let Some(tool) = tool_catalog().into_iter().find(|tool| tool.name == name) else {
            return error_response(id, -32602, &format!("unknown tool: {name}"));
        };
        if self.read_only && tool.write {
            return tool_result_response(
                id,
                Err(ToolError::new(
                    "read_only",
                    format!(
                        "the MCP server is running in read-only mode, so {name} is unavailable; ask the human to restart it without OPENMESH_MCP_READ_ONLY"
                    ),
                    "agents",
                )),
            );
        }
        if tool.confirm && arguments.get("confirm").and_then(|v| v.as_bool()) != Some(true) {
            return tool_result_response(
                id,
                Err(ToolError::new(
                    "confirm_required",
                    format!(
                        "{name} is destructive and requires confirm:true. Ask the human first."
                    ),
                    "agents",
                )),
            );
        }
        if tool.write && !self.allow_write() {
            return tool_result_response(
                id,
                Err(ToolError::new(
                    "rate_limited",
                    format!(
                        "write limit reached ({} mutating calls per minute); wait a moment and try again",
                        self.write_limit
                    ),
                    "agents",
                )),
            );
        }
        let outcome = self.dispatch(name, &arguments).await;
        if tool.write {
            self.note_write();
        }
        tool_result_response(id, outcome)
    }

    fn allow_write(&mut self) -> bool {
        let now = Instant::now();
        while let Some(front) = self.write_times.front() {
            if now.duration_since(*front) >= Duration::from_secs(60) {
                self.write_times.pop_front();
            } else {
                break;
            }
        }
        self.write_times.len() < self.write_limit
    }

    fn note_write(&mut self) {
        self.write_times.push_back(Instant::now());
    }

    async fn get(&self, path: &str) -> Result<Value, ToolError> {
        self.http
            .request("GET", path, None)
            .await
            .map(|out| out.value)
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value, ToolError> {
        self.http
            .request("POST", path, Some(&body))
            .await
            .map(|out| out.value)
    }

    async fn patch(&self, path: &str, body: Value) -> Result<Value, ToolError> {
        self.http
            .request("PATCH", path, Some(&body))
            .await
            .map(|out| out.value)
    }

    async fn delete(&self, path: &str) -> Result<Value, ToolError> {
        self.http
            .request("DELETE", path, None)
            .await
            .map(|out| out.value)
    }

    async fn dispatch(&self, name: &str, args: &Value) -> Result<ToolOutcome, ToolError> {
        match name {
            "openmesh_health" => {
                let health = self.get("/api/health").await?;
                let status = self.get("/api/status").await.unwrap_or(Value::Null);
                let uptime = status.get("uptime_s").and_then(|v| v.as_u64()).unwrap_or(0);
                let connected = status
                    .pointer("/radio/connected")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let text = format!(
                    "Bridge v{} is up (uptime {}s); radio connected: {}.",
                    health
                        .get("version")
                        .and_then(|v| v.as_str())
                        .unwrap_or(env!("CARGO_PKG_VERSION")),
                    uptime,
                    if connected { "yes" } else { "no" }
                );
                Ok(ToolOutcome::new(
                    text,
                    json!({ "health": health, "status": status }),
                ))
            }
            "openmesh_status" => {
                let status = self.get("/api/status").await?;
                let config = self.get("/api/config").await?;
                let name = config
                    .pointer("/node/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let region = config
                    .pointer("/radio/region")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let profile = config
                    .pointer("/radio/profile")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let text = format!(
                    "Node {name}: region {region}, profile {profile}, uptime {}s, airtime {:.1}%, tx queue {}.",
                    status.get("uptime_s").and_then(|v| v.as_u64()).unwrap_or(0),
                    status
                        .pointer("/radio/airtime_used_pct")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    status
                        .pointer("/radio/tx_queue")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                );
                Ok(ToolOutcome::new(
                    text,
                    json!({ "status": status, "config": config }),
                ))
            }
            "openmesh_nodes" => {
                let nodes = self.get("/api/nodes").await?;
                let mut list: Vec<Value> = nodes
                    .as_array()
                    .cloned()
                    .or_else(|| nodes.get("nodes").and_then(|v| v.as_array()).cloned())
                    .unwrap_or_default();
                if let Some(role) = args.get("role").and_then(|v| v.as_str()) {
                    list.retain(|node| node.get("class").and_then(|v| v.as_str()) == Some(role));
                }
                if args
                    .get("has_position")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    list.retain(|node| node.get("position").map(|v| !v.is_null()).unwrap_or(false));
                }
                if let Some(limit) = args.get("limit").and_then(|v| v.as_u64()) {
                    list.truncate(limit as usize);
                }
                let mut text = format!("{} node(s) known:", list.len());
                for node in &list {
                    text.push_str(&format!(
                        "\n- {} {} ({}) hops={} seen={}",
                        node.get("node_id").and_then(|v| v.as_str()).unwrap_or("?"),
                        node.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        node.get("class").and_then(|v| v.as_str()).unwrap_or("?"),
                        node.get("hops").and_then(|v| v.as_u64()).unwrap_or(0),
                        node.get("last_seen_ms")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                    ));
                }
                if list.is_empty() {
                    text.push_str("\n(no nodes yet — they appear after an announce)");
                }
                Ok(ToolOutcome::new(text, json!({ "nodes": list })))
            }
            "openmesh_node" => {
                let node_id = required_str(args, "node_id")?;
                let node = self.get(&format!("/api/nodes/{node_id}")).await?;
                let contacts = self.get("/api/contacts").await.unwrap_or(Value::Null);
                let is_contact = contacts
                    .get("contacts")
                    .and_then(|v| v.as_array())
                    .map(|contacts| {
                        contacts.iter().any(|contact| {
                            contact.get("node_id").and_then(|v| v.as_str()) == Some(node_id)
                        })
                    })
                    .unwrap_or(false);
                let text = format!(
                    "Node {} ({}) class={} hops={} last seen {} ms; saved contact: {}.",
                    node.get("node_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or(node_id),
                    node.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    node.get("class").and_then(|v| v.as_str()).unwrap_or("?"),
                    node.get("hops").and_then(|v| v.as_u64()).unwrap_or(0),
                    node.get("last_seen_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    if is_contact { "yes" } else { "no" },
                );
                Ok(ToolOutcome::new(
                    text,
                    json!({ "node": node, "is_contact": is_contact }),
                ))
            }
            "openmesh_messages" => {
                let peer = args.get("peer").and_then(|v| v.as_str());
                let channel = args.get("channel").and_then(|v| v.as_str());
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50);
                let before = args.get("before_ms").and_then(|v| v.as_u64());
                let path = match (peer, channel) {
                    (Some(peer), _) => format!("/api/messages?peer={peer}&limit={limit}"),
                    (None, Some(channel)) => {
                        format!("/api/messages/channel/{channel}?limit={limit}")
                    }
                    _ => {
                        return Err(ToolError::new(
                            "bad_request",
                            "pass either peer or channel",
                            "messaging",
                        ));
                    }
                };
                let path = match before {
                    Some(before) => format!("{path}&before_ms={before}"),
                    None => path,
                };
                let payload = self.get(&path).await?;
                let messages: Vec<Value> = payload
                    .get("messages")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut text = format!("{} message(s):", messages.len());
                for message in messages.iter().take(20) {
                    text.push_str(&format!(
                        "\n- id={} from={} at={} text={:?}",
                        message.get("msg_id").and_then(|v| v.as_u64()).unwrap_or(0),
                        message.get("from").and_then(|v| v.as_str()).unwrap_or("?"),
                        message
                            .get("sent_at_ms")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        message.get("text").and_then(|v| v.as_str()).unwrap_or(""),
                    ));
                }
                Ok(ToolOutcome::new(text, payload))
            }
            "openmesh_channels" => {
                let channels = self.get("/api/channels").await?;
                let list = channels
                    .get("channels")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut text = format!("{} channel(s):", list.len());
                for channel in &list {
                    text.push_str(&format!(
                        "\n- {} {}",
                        channel
                            .get("channel_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("?"),
                        channel.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    ));
                }
                Ok(ToolOutcome::new(text, channels))
            }
            "openmesh_contacts" => {
                let contacts = self.get("/api/contacts").await?;
                let text = format!(
                    "{} saved contact(s).",
                    contacts
                        .get("contacts")
                        .and_then(|v| v.as_array())
                        .map(Vec::len)
                        .unwrap_or(0)
                );
                Ok(ToolOutcome::new(text, contacts))
            }
            "openmesh_backbone_status" => {
                let status = self.get("/api/backbone").await?;
                if status.get("enabled").and_then(|v| v.as_bool()) == Some(false)
                    && status.get("mode").is_none()
                {
                    return Ok(ToolOutcome::new(
                        "Backbone mode is not configured on this bridge.",
                        status,
                    ));
                }
                let text = format!(
                    "Backbone mode={} running={} listen={}:{} peers={} store_forward={}.",
                    status.get("mode").and_then(|v| v.as_str()).unwrap_or("?"),
                    status
                        .get("running")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    status.get("bind").and_then(|v| v.as_str()).unwrap_or(""),
                    status.get("port").and_then(|v| v.as_u64()).unwrap_or(0),
                    status.get("peers").and_then(|v| v.as_u64()).unwrap_or(0),
                    status
                        .get("store_forward")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                );
                Ok(ToolOutcome::new(text, status))
            }
            "openmesh_backbone_peers" => {
                let peers = self.get("/api/backbone/peers").await?;
                let list = peers.as_array().cloned().unwrap_or_default();
                let mut text = format!("{} backbone peer(s):", list.len());
                for peer in &list {
                    text.push_str(&format!(
                        "\n- {} {} {} frames_in={} frames_out={} rtt={:?}",
                        peer.get("node_id").and_then(|v| v.as_str()).unwrap_or("?"),
                        peer.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        peer.get("direction").and_then(|v| v.as_str()).unwrap_or(""),
                        peer.get("frames_in").and_then(|v| v.as_u64()).unwrap_or(0),
                        peer.get("frames_out").and_then(|v| v.as_u64()).unwrap_or(0),
                        peer.get("rtt_ms"),
                    ));
                }
                Ok(ToolOutcome::new(text, peers))
            }
            "openmesh_backbone_setup" => {
                let setup = self.get("/api/backbone/setup").await?;
                let mut safe = setup.clone();
                if let Some(connection) = safe.get("connection_string").and_then(|v| v.as_str()) {
                    safe["connection_string"] = json!(redact_secrets(connection));
                }
                let text = format!(
                    "Backbone setup: listen {} (auth {}). The connection-string token is redacted here — copy the full string from the web panel (panel_url: {}).",
                    safe.get("listen").and_then(|v| v.as_str()).unwrap_or("?"),
                    safe.get("auth").and_then(|v| v.as_str()).unwrap_or("?"),
                    safe.get("panel_url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?"),
                );
                Ok(ToolOutcome::new(text, safe))
            }
            "openmesh_setup_state" => {
                let state = self.get("/api/setup/state").await?;
                let first_run = state
                    .get("first_run")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let text = if first_run {
                    "This node is not set up yet. Ask the human for role, country and a node name, then call openmesh_setup_quick.".to_string()
                } else {
                    "Setup is complete. openmesh_setup_state has the checklist and recommendations."
                        .to_string()
                };
                Ok(ToolOutcome::new(text, state))
            }
            "openmesh_activity" => {
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50);
                let payload = self.get(&format!("/api/activity?limit={limit}")).await?;
                let entries = payload
                    .get("entries")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut text = format!("{} recent action(s):", entries.len());
                for entry in entries.iter().take(50) {
                    text.push_str(&format!(
                        "\n- [{}] {} {}: {} ({})",
                        entry.get("source").and_then(|v| v.as_str()).unwrap_or("?"),
                        entry.get("method").and_then(|v| v.as_str()).unwrap_or(""),
                        entry.get("path").and_then(|v| v.as_str()).unwrap_or(""),
                        entry.get("summary").and_then(|v| v.as_str()).unwrap_or(""),
                        if entry.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                            "ok"
                        } else {
                            "failed"
                        },
                    ));
                }
                Ok(ToolOutcome::new(text, payload))
            }
            "openmesh_help" => {
                let topic = args.get("topic").and_then(|v| v.as_str());
                if let Some(topic) = topic {
                    let value = self.get(&format!("/api/help/{topic}")).await?;
                    let markdown = value.get("markdown").and_then(|v| v.as_str()).unwrap_or("");
                    Ok(ToolOutcome::new(markdown.to_string(), value))
                } else {
                    let payload = self.get("/api/help").await?;
                    let mut text = String::from("OpenMesh help topics:\n");
                    if let Some(topics) = payload.get("topics").and_then(|v| v.as_array()) {
                        for topic in topics {
                            text.push_str(&format!(
                                "- {}: {} ({})\n",
                                topic.get("slug").and_then(|v| v.as_str()).unwrap_or("?"),
                                topic.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                                topic.get("summary").and_then(|v| v.as_str()).unwrap_or(""),
                            ));
                        }
                    }
                    Ok(ToolOutcome::new(text, payload))
                }
            }
            "openmesh_send_message" => {
                let mut body = json!({
                    "to": required_str(args, "to")?,
                });
                copy_opt(args, "text", &mut body);
                copy_opt(args, "reply_to", &mut body);
                copy_opt(args, "attachment_id", &mut body);
                let result = self.post("/api/messages", body).await?;
                let text = format!(
                    "Message queued as id {} (estimated airtime {} ms).",
                    result.get("msg_id").and_then(|v| v.as_u64()).unwrap_or(0),
                    result
                        .get("est_airtime_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                );
                Ok(ToolOutcome::new(text, result))
            }
            "openmesh_send_channel_message" => {
                let channel = required_str(args, "channel")?;
                let mut body = json!({});
                copy_opt(args, "text", &mut body);
                copy_opt(args, "attachment_id", &mut body);
                let result = self
                    .post(&format!("/api/channels/{channel}/messages"), body)
                    .await?;
                let text = format!(
                    "Channel message queued as id {} (estimated airtime {} ms).",
                    result.get("msg_id").and_then(|v| v.as_u64()).unwrap_or(0),
                    result
                        .get("est_airtime_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                );
                Ok(ToolOutcome::new(text, result))
            }
            "openmesh_share_position" => {
                let mut body = json!({});
                copy_opt(args, "to", &mut body);
                if let Some(channel) = args.get("channel").and_then(|v| v.as_str()) {
                    body["channel_id"] = json!(channel);
                }
                if !body.get("to").is_some() && !body.get("channel_id").is_some() {
                    return Err(ToolError::new(
                        "bad_request",
                        "pass either to or channel",
                        "map-and-position",
                    ));
                }
                let result = self.post("/api/position", body).await?;
                Ok(ToolOutcome::new("Position shared.", result))
            }
            "openmesh_announce" => {
                let result = self.post("/api/announce", json!({})).await?;
                Ok(ToolOutcome::new("Announce queued.", result))
            }
            "openmesh_mark_read" => {
                let msg_id = required_u64(args, "msg_id")?;
                let result = self
                    .post(&format!("/api/messages/{msg_id}/read"), json!({}))
                    .await?;
                Ok(ToolOutcome::new(
                    format!("Read receipt queued for message {msg_id}."),
                    result,
                ))
            }
            "openmesh_retry_message" => {
                let msg_id = required_u64(args, "msg_id")?;
                let result = self
                    .post(&format!("/api/messages/{msg_id}/retry"), json!({}))
                    .await?;
                Ok(ToolOutcome::new(
                    format!("Retry queued for message {msg_id}."),
                    result,
                ))
            }
            "openmesh_set_config" => {
                let patch = args.get("patch").cloned();
                let override_patch = args
                    .get("regulatory_override")
                    .filter(|value| value.is_object())
                    .cloned();
                if patch.is_none() && override_patch.is_none() {
                    return Err(ToolError::new(
                        "bad_request",
                        "pass patch and/or regulatory_override",
                        "troubleshooting",
                    ));
                }
                let mut body = patch.unwrap_or_else(|| json!({}));
                let mut warning = None;
                if let Some(override_patch) = override_patch {
                    let config = self.get("/api/config").await?;
                    let current = config
                        .get("regulatory_override")
                        .cloned()
                        .unwrap_or(json!({}));
                    let desired_enabled = override_patch
                        .get("enabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or_else(|| {
                            current
                                .get("enabled")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false)
                        });
                    let current_enabled = current
                        .get("enabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let exceeds = override_merged_exceeds_legal(&config, &current, &override_patch);
                    let requires_confirm = desired_enabled && (!current_enabled || exceeds);
                    if requires_confirm
                        && args.get("confirm").and_then(|v| v.as_bool()) != Some(true)
                    {
                        return Err(ToolError::new(
                            "confirm_required",
                            format!(
                                "openmesh_set_config wants to enable a regulatory override (exceeds legal maxima: {exceeds}). Ask the human first and retry with confirm:true. {OVERRIDE_WARNING}"
                            ),
                            "regulatory-and-emergency",
                        ));
                    }
                    let mut merged = override_patch.clone();
                    if let Some(object) = merged.as_object_mut() {
                        object.insert("enabled".into(), json!(desired_enabled));
                        if desired_enabled && requires_confirm {
                            object.insert("acknowledged".into(), json!(true));
                            object.remove("confirm");
                        } else {
                            object.remove("confirm");
                        }
                    }
                    if desired_enabled {
                        warning = Some(OVERRIDE_WARNING.to_string());
                    }
                    if let Some(object) = body.as_object_mut() {
                        object.insert("regulatory_override".into(), merged);
                    } else {
                        body = json!({ "regulatory_override": merged });
                    }
                }
                let result = self.patch("/api/config", body).await?;
                let mut text = String::from("Configuration applied.");
                if let Some(warning) = result
                    .get("override_warning")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .or(warning)
                {
                    text.push_str(&format!(" Warning: {warning}"));
                }
                Ok(ToolOutcome::new(text, result))
            }
            "openmesh_setup_quick" => {
                let role = required_str(args, "role")?;
                let country = required_str(args, "country")?;
                let mut body = json!({ "role": role, "country": country });
                copy_opt(args, "node_name", &mut body);
                copy_opt(args, "privacy", &mut body);
                copy_opt(args, "create_channel", &mut body);
                let result = self.post("/api/setup/quick", body).await?;
                let mut text = String::from("Secure setup applied.");
                if let Some(channel) = result.get("channel").filter(|v| !v.is_null()) {
                    if let Some(invite) = channel.get("invite_url").and_then(|v| v.as_str()) {
                        text.push_str(&format!(
                            " Share this channel invite with the people you trust: {}",
                            redact_secrets(invite)
                        ));
                    }
                    if channel.get("psk_once").and_then(|v| v.as_str()).is_some() {
                        text.push_str(" The PSK is returned once in structuredContent for the human to store — do not paste it into chat logs.");
                    }
                }
                text.push_str(" Next steps: send a message, invite someone, add a contact, run a backbone or connect an agent.");
                Ok(ToolOutcome::new(text, result))
            }
            "openmesh_create_channel" => {
                let name = required_str(args, "name")?;
                let mut body = json!({ "name": name });
                copy_opt(args, "psk", &mut body);
                let result = self.post("/api/channels", body).await?;
                let text = format!(
                    "Channel created: {}. The PSK and invite URL are returned once in structuredContent — write them to a file, not into chat logs.",
                    result.get("name").and_then(|v| v.as_str()).unwrap_or(name)
                );
                Ok(ToolOutcome::new(text, result))
            }
            "openmesh_join_channel" => {
                let mut body = json!({});
                copy_opt(args, "url", &mut body);
                copy_opt(args, "name", &mut body);
                copy_opt(args, "psk", &mut body);
                if body.get("url").is_none()
                    && (body.get("name").is_none() || body.get("psk").is_none())
                {
                    return Err(ToolError::new(
                        "bad_request",
                        "pass url, or name and psk",
                        "channels",
                    ));
                }
                let result = self.post("/api/channels/join", body).await?;
                Ok(ToolOutcome::new("Channel joined.", result))
            }
            "openmesh_add_contact" => {
                let mut body = json!({});
                copy_opt(args, "invite_url", &mut body);
                copy_opt(args, "identity", &mut body);
                copy_opt(args, "node_id", &mut body);
                copy_opt(args, "name", &mut body);
                if body.get("invite_url").is_none()
                    && (body.get("identity").is_none() || body.get("node_id").is_none())
                {
                    return Err(ToolError::new(
                        "bad_request",
                        "pass invite_url, or identity and node_id",
                        "contacts-identity",
                    ));
                }
                let result = self.post("/api/contacts", body).await?;
                Ok(ToolOutcome::new("Contact saved.", result))
            }
            "openmesh_backbone_config" => {
                let patch = args.get("patch").cloned().ok_or_else(|| {
                    ToolError::new("bad_request", "patch object required", "backbone")
                })?;
                let result = self.post("/api/backbone/config", patch).await?;
                Ok(ToolOutcome::new("Backbone configuration applied.", result))
            }
            "openmesh_rotate_backbone_token" => {
                let result = self.post("/api/backbone/token/rotate", json!({})).await?;
                let text = if let Some(token) = result.get("token").and_then(|v| v.as_str()) {
                    format!(
                        "Backbone token rotated. New token (shown once — write it to a file, never into chat logs): {token}"
                    )
                } else {
                    "Backbone mode is not configured; nothing to rotate.".to_string()
                };
                let mut structured = result;
                structured["shown_once"] = json!(true);
                Ok(ToolOutcome::new(text, structured))
            }
            "openmesh_kick_backbone_peer" => {
                let node_id = required_str(args, "node_id")?;
                let result = self
                    .delete(&format!("/api/backbone/peers/{node_id}"))
                    .await?;
                Ok(ToolOutcome::new(
                    format!("Backbone peer {node_id} disconnected."),
                    result,
                ))
            }
            _ => Err(ToolError::new(
                "unknown_tool",
                format!("unknown tool: {name}"),
                "agents",
            )),
        }
    }

    async fn resources_read(&self, id: &Value, params: &Value) -> String {
        let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
        let contents = match uri {
            "openmesh://help/index" => {
                let text = help::index_text();
                json!({ "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": text }] })
            }
            _ if uri.starts_with("openmesh://help/") => {
                let slug = &uri["openmesh://help/".len()..];
                match help::markdown(slug) {
                    Some(markdown) => json!({
                        "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": markdown }]
                    }),
                    None => {
                        return error_response(
                            id,
                            -32602,
                            &format!("unknown help topic '{slug}' (see openmesh://help/index)"),
                        );
                    }
                }
            }
            "openmesh://status" => match self.get("/api/status").await {
                Ok(value) => {
                    json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": value.to_string() }] })
                }
                Err(error) => return error_response(id, -32603, &error.message),
            },
            "openmesh://nodes" => match self.get("/api/nodes").await {
                Ok(value) => {
                    json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": value.to_string() }] })
                }
                Err(error) => return error_response(id, -32603, &error.message),
            },
            "openmesh://config" => match self.get("/api/config").await {
                Ok(value) => {
                    json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": value.to_string() }] })
                }
                Err(error) => return error_response(id, -32603, &error.message),
            },
            _ => {
                return error_response(
                    id,
                    -32602,
                    &format!("unknown resource '{uri}'; resources/list shows the available URIs"),
                );
            }
        };
        result_response(id, contents)
    }

    async fn prompts_get(&self, id: &Value, params: &Value) -> String {
        let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let status = self
            .get("/api/status")
            .await
            .map(|value| value.to_string())
            .unwrap_or_else(|_| "(bridge API unreachable)".to_string());
        let nodes = self
            .get("/api/nodes")
            .await
            .map(|value| value.to_string())
            .unwrap_or_else(|_| "(bridge API unreachable)".to_string());
        let help_index = help::index_text();
        let safety = "Safety rules: ask before anything destructive (token rotation, kicking peers, changing radio region). Never paste API tokens, PSKs or identity seeds into chat logs. Destructive tools require confirm:true. Mutations are visible in the activity log.";
        let text = match name {
            "manage_my_mesh" => format!(
                "You are operating a local OpenMesh bridge through MCP. Here is the live state.\n\nStatus: {status}\n\nNodes: {nodes}\n\nHelp topics:\n{help_index}\n{safety}\n\nAsk the human what they want to change, prefer read-only tools for diagnosis, and call openmesh_help for the relevant topic before changing radio settings."
            ),
            "set_up_my_node" => format!(
                "You are a beginner-friendly OpenMesh onboarding assistant. Ask the human: (1) what the box is (client, tracker, repeater or gateway), (2) their country, (3) a node name, and (4) whether position may be shared with their channel. Then call openmesh_setup_quick with role/country/node_name/privacy/create_channel=true. Read the returned checklist and invite back to them, and explain any trade-offs with help topics.\n\nCurrent state: {status}\n\nHelp topics:\n{help_index}\n{safety}"
            ),
            "troubleshoot_mesh" => format!(
                "Diagnose this OpenMesh node: (1) openmesh_health, (2) openmesh_status for radio/airtime, (3) openmesh_nodes for peers, (4) openmesh_messages for failed sends, (5) openmesh_activity for recent mutations, then openmesh_help topic troubleshooting. Report what happened, what to try next and a help link, never blame the user.\n\nStatus: {status}\n\nNodes: {nodes}\n\nHelp topics:\n{help_index}\n{safety}"
            ),
            "set_up_backbone" => format!(
                "Walk the human through running or joining an OpenMesh backbone: call openmesh_backbone_setup for the connection string (token redacted here — the human copies it from the panel), explain the firewall command and the systemd unit from the payload, and for a gateway use `openmesh-bridge join '<connection string>'`. If the human wants to rotate the backbone token, ask for explicit confirmation first.\n\nStatus: {status}\n\nHelp topics:\n{help_index}\n{safety}"
            ),
            "invite_someone" => format!(
                "Help the human invite someone. Check openmesh_channels for an existing channel; if there is none, offer openmesh_create_channel (returns a one-time PSK + invite URL) or openmesh_setup_quick. Share the invite URL and QR-friendly link, and tell them the PSK is shown once and must be stored safely.\n\nStatus: {status}\n\nHelp topics:\n{help_index}\n{safety}"
            ),
            other => {
                return error_response(
                    id,
                    -32602,
                    &format!("unknown prompt '{other}'; prompts/list shows the available prompts"),
                );
            }
        };
        let description = match name {
            "manage_my_mesh" => "Operate this mesh safely with live state and security rules",
            "set_up_my_node" => "Beginner onboarding assistant for a secure node",
            "troubleshoot_mesh" => "Diagnostic playbook: health → radio → nodes → activity → help",
            "set_up_backbone" => "Run or join a backbone, firewall and systemd guidance",
            "invite_someone" => "Create or read a channel invite and draft a shareable message",
            _ => "",
        };
        result_response(
            id,
            json!({
                "description": description,
                "messages": [{
                    "role": "user",
                    "content": { "type": "text", "text": text },
                }],
            }),
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Resources / prompts catalog
// ---------------------------------------------------------------------------------------------

fn resources_list_json() -> Value {
    let mut resources = vec![
        json!({
            "uri": "openmesh://help/index",
            "name": "Help index",
            "description": "All help topics with titles, audience and summaries.",
            "mimeType": "text/markdown",
        }),
        json!({
            "uri": "openmesh://status",
            "name": "Bridge status",
            "description": "Live bridge/radio/mesh status JSON.",
            "mimeType": "application/json",
        }),
        json!({
            "uri": "openmesh://nodes",
            "name": "Known nodes",
            "description": "Node list JSON with identity metadata.",
            "mimeType": "application/json",
        }),
        json!({
            "uri": "openmesh://config",
            "name": "Configuration",
            "description": "Config JSON with secrets redacted.",
            "mimeType": "application/json",
        }),
    ];
    for slug in help::slugs() {
        let topic = help::topic_value(&slug).unwrap_or(Value::Null);
        resources.push(json!({
            "uri": format!("openmesh://help/{slug}"),
            "name": topic.get("title").and_then(|v| v.as_str()).unwrap_or(&slug),
            "description": topic.get("summary").and_then(|v| v.as_str()).unwrap_or(""),
            "mimeType": "text/markdown",
        }));
    }
    json!({ "resources": resources })
}

fn prompts_list_json() -> Value {
    json!({
        "prompts": [
            { "name": "manage_my_mesh", "description": "Here is my mesh state; help me operate it safely", "arguments": [] },
            { "name": "set_up_my_node", "description": "Help me set up a secure OpenMesh node", "arguments": [] },
            { "name": "troubleshoot_mesh", "description": "Something is wrong with my mesh — diagnose it", "arguments": [] },
            { "name": "set_up_backbone", "description": "Help me run or join a backbone", "arguments": [] },
            { "name": "invite_someone", "description": "Help me invite someone to my mesh", "arguments": [] },
        ]
    })
}

// ---------------------------------------------------------------------------------------------
// Result plumbing
// ---------------------------------------------------------------------------------------------

struct ToolOutcome {
    text: String,
    structured: Value,
}

impl ToolOutcome {
    fn new(text: impl Into<String>, structured: Value) -> Self {
        Self {
            text: text.into(),
            structured,
        }
    }
}

/// Whether merging a partial override onto the current one exceeds the legal maxima.
fn override_merged_exceeds_legal(config: &Value, current: &Value, patch: &Value) -> bool {
    let region_name = config
        .pointer("/legal/region")
        .and_then(|v| v.as_str())
        .or_else(|| config.pointer("/radio/region").and_then(|v| v.as_str()))
        .unwrap_or("EU868");
    let Some(region) = openmesh_core::util::REGIONS
        .iter()
        .find(|region| region.name.eq_ignore_ascii_case(region_name))
    else {
        return true;
    };
    let merged = |key: &str| patch.get(key).or_else(|| current.get(key));
    if let Some(duty) = merged("duty_cycle_pct").and_then(|v| v.as_u64()) {
        if region
            .duty_cycle_pct
            .is_some_and(|legal| duty > u64::from(legal))
        {
            return true;
        }
    }
    if let Some(power) = merged("max_power_dbm").and_then(|v| v.as_i64()) {
        if power > i64::from(region.max_power_dbm) {
            return true;
        }
    }
    if let Some(frequency) = merged("frequency_khz").and_then(|v| v.as_u64()) {
        if frequency != u64::from(region.freq_khz) {
            return true;
        }
    }
    false
}

fn tool_result_response(id: &Value, outcome: Result<ToolOutcome, ToolError>) -> String {
    match outcome {
        Ok(outcome) => result_response(
            id,
            json!({
                "content": [{ "type": "text", "text": outcome.text }],
                "structuredContent": outcome.structured,
            }),
        ),
        Err(error) => result_response(
            id,
            json!({
                "content": [{ "type": "text", "text": error.message }],
                "structuredContent": {
                    "error": {
                        "code": error.code,
                        "message": error.message,
                        "help_topic": error.help_topic,
                    }
                },
                "isError": true,
            }),
        ),
    }
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    args.get(key)
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ToolError::new(
                "bad_request",
                format!("missing required argument: {key}"),
                "getting-started",
            )
        })
}

fn required_u64(args: &Value, key: &str) -> Result<u64, ToolError> {
    args.get(key)
        .and_then(|value| value.as_u64())
        .ok_or_else(|| {
            ToolError::new(
                "bad_request",
                format!("missing required integer argument: {key}"),
                "getting-started",
            )
        })
}

fn copy_opt(from: &Value, key: &str, to: &mut Value) {
    if let Some(value) = from.get(key) {
        if !value.is_null() {
            to[key] = value.clone();
        }
    }
}

fn result_response(id: &Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error_response(id: &Value, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_catalog_matches_contract() {
        let tools = tool_catalog();
        let names: Vec<&str> = tools.iter().map(|tool| tool.name).collect();
        for required in [
            "openmesh_health",
            "openmesh_status",
            "openmesh_nodes",
            "openmesh_node",
            "openmesh_messages",
            "openmesh_channels",
            "openmesh_contacts",
            "openmesh_backbone_status",
            "openmesh_backbone_peers",
            "openmesh_backbone_setup",
            "openmesh_setup_state",
            "openmesh_activity",
            "openmesh_help",
            "openmesh_send_message",
            "openmesh_send_channel_message",
            "openmesh_share_position",
            "openmesh_announce",
            "openmesh_mark_read",
            "openmesh_retry_message",
            "openmesh_set_config",
            "openmesh_setup_quick",
            "openmesh_create_channel",
            "openmesh_join_channel",
            "openmesh_add_contact",
            "openmesh_backbone_config",
            "openmesh_rotate_backbone_token",
            "openmesh_kick_backbone_peer",
        ] {
            assert!(names.contains(&required), "missing tool {required}");
        }
        assert!(
            tools
                .iter()
                .all(|tool| tool.input_schema["type"] == "object")
        );
    }

    #[test]
    fn read_only_hides_writes() {
        let full = tools_list_json(false);
        let read_only = tools_list_json(true);
        let count = |value: &Value| value["tools"].as_array().unwrap().len();
        assert!(count(&read_only) < count(&full));
        assert!(
            read_only["tools"]
                .as_array()
                .unwrap()
                .iter()
                .all(|tool| tool["annotations"]["readOnlyHint"] == true)
        );
    }

    #[test]
    fn destructive_tools_are_marked() {
        let tools = tool_catalog();
        let destructive: Vec<&str> = tools
            .iter()
            .filter(|tool| tool.confirm)
            .map(|tool| tool.name)
            .collect();
        assert_eq!(
            destructive,
            vec![
                "openmesh_rotate_backbone_token",
                "openmesh_kick_backbone_peer"
            ]
        );
    }

    #[test]
    fn secrets_are_redacted() {
        let text = "openmesh://backbone?host=x&port=1&token=abc123&name=y";
        let redacted = redact_secrets(text);
        assert!(!redacted.contains("abc123"));
        assert!(redacted.contains("token=<redacted-by-mcp>"));
        assert!(redacted.contains("name=y"));
    }
}

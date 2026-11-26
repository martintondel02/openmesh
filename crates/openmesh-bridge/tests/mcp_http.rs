//! Remote MCP over HTTP (`docs/design/remote-operations.md` §1).
//!
//! Uses the real bridge HTTP listener and a tiny local HTTP client; no external network.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use openmesh_bridge::transport::backbone::ConfigOverrides;
use openmesh_bridge::{Bridge, BridgeOptions};

const API_TOKEN: &str = "mcp00112233445566778899aabbccddeeff00112233445566778899aabbcc";

async fn start_bridge(dir: &Path, mcp_config: Option<Value>) -> Bridge {
    if let Some(config) = mcp_config {
        std::fs::write(
            dir.join("mcp-http.json"),
            serde_json::to_string_pretty(&config).expect("config json"),
        )
        .expect("write mcp-http.json");
    }
    let options = BridgeOptions {
        listen: SocketAddr::from(([127, 0, 0, 1], 0)),
        token: Some(API_TOKEN.to_string()),
        token_file: dir.join("bridge-token"),
        db_path: dir.join("openmesh.redb"),
        data_dir: dir.to_path_buf(),
        identity_seed_path: dir.join("identity.key"),
        name: None,
        class: None,
        profile: None,
        region: None,
        sim: true,
        demo: false,
        sim_config: Default::default(),
        mesh_hub: None,
        udp_port: None,
        tcp_listen: None,
        serial_port: None,
        serve_web: None,
        api_only: false,
        backbone_enabled: false,
        backbone_overrides: ConfigOverrides::default(),
        backbone_join: None,
    };
    Bridge::start(options).await.expect("bridge starts")
}

fn enabled_config() -> Value {
    json!({
        "enabled": true,
        "path": "/mcp",
        "allow_write": false,
        "require_confirm": true,
        "rate_limit_per_min": 60,
        "max_sessions": 8
    })
}

struct HttpResult {
    status: u16,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl HttpResult {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    fn session_id(&self) -> Option<&str> {
        self.headers.get("mcp-session-id").map(String::as_str)
    }
}

async fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&Value>,
    headers: &[(&str, &str)],
) -> HttpResult {
    let mut stream = TcpStream::connect(addr).await.expect("connect bridge");
    let payload = body.map(|value| value.to_string()).unwrap_or_default();
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if !payload.is_empty() {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", payload.len()));
    }
    request.push_str("\r\n");
    request.push_str(&payload);
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(20), async {
        stream.read_to_end(&mut response).await
    })
    .await
    .expect("http response timeout")
    .expect("http response");
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or(response.len());
    let head = String::from_utf8_lossy(&response[..split]).to_string();
    let body = if split + 4 <= response.len() {
        response[split + 4..].to_vec()
    } else {
        Vec::new()
    };
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let headers = head
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    HttpResult {
        status,
        headers,
        body,
    }
}

async fn post(addr: SocketAddr, body: Value, token: &str, headers: &[(&str, &str)]) -> HttpResult {
    http(addr, "POST", "/mcp", Some(token), Some(&body), headers).await
}

fn rpc(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

fn tool_call(id: u64, name: &str, arguments: Value) -> Value {
    rpc(
        id,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    )
}

async fn activity(addr: SocketAddr) -> Value {
    http(
        addr,
        "GET",
        "/api/activity?limit=50",
        Some(API_TOKEN),
        None,
        &[],
    )
    .await
    .json()
}

fn mcp_entry(activity: &Value) -> Option<&Value> {
    activity["entries"]
        .as_array()?
        .iter()
        .find(|entry| entry["method"] == "MCP")
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn disabled_by_default_returns_404_and_writes_default_config() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path(), None).await;
    let addr = bridge.addr;
    let response = post(addr, rpc(1, "initialize", json!({})), API_TOKEN, &[]).await;
    assert_eq!(response.status, 404);
    let get = http(addr, "GET", "/mcp", Some(API_TOKEN), None, &[]).await;
    assert_eq!(get.status, 404);
    // The bridge creates a disabled 0600 config file on first start.
    let path = dir.path().join("mcp-http.json");
    assert!(path.exists());
    let config: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(config["enabled"], false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    bridge.shutdown().await;
}

#[tokio::test]
async fn corrupt_config_is_tolerated_as_disabled() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("mcp-http.json"), "{not valid json").unwrap();
    let bridge = start_bridge(dir.path(), None).await;
    let addr = bridge.addr;
    let response = post(addr, rpc(1, "initialize", json!({})), API_TOKEN, &[]).await;
    assert_eq!(response.status, 404);
    bridge.shutdown().await;
}

#[tokio::test]
async fn initialize_tools_list_session_and_read_tool() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path(), Some(enabled_config())).await;
    let addr = bridge.addr;

    let init = post(
        addr,
        rpc(
            1,
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}}),
        ),
        API_TOKEN,
        &[],
    )
    .await;
    assert_eq!(init.status, 200, "initialize: {}", init.json());
    assert_eq!(init.json()["result"]["serverInfo"]["name"], "openmesh");
    let session = init.session_id().expect("generated session id").to_string();
    assert!(session.len() >= 16);

    let list = post(
        addr,
        rpc(2, "tools/list", json!({})),
        API_TOKEN,
        &[("Mcp-Session-Id", &session)],
    )
    .await;
    assert_eq!(list.status, 200);
    assert_eq!(list.session_id(), Some(session.as_str()));
    let tools = list.json()["result"]["tools"].as_array().unwrap().clone();
    assert_eq!(tools.len(), 27, "full MCP tool catalog stays visible");
    assert!(tools.iter().any(|tool| tool["name"] == "openmesh_status"));

    let status = post(
        addr,
        tool_call(3, "openmesh_status", json!({})),
        API_TOKEN,
        &[("Mcp-Session-Id", &session)],
    )
    .await;
    assert_eq!(status.status, 200);
    let result = &status.json()["result"];
    assert_ne!(result["isError"], true);
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Node")
    );
    bridge.shutdown().await;
}

#[tokio::test]
async fn bad_token_is_401_and_never_executes() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path(), Some(enabled_config())).await;
    let addr = bridge.addr;
    let before = activity(addr).await["entries"].as_array().unwrap().len();

    let init = http(
        addr,
        "POST",
        "/mcp",
        Some("wrong-token"),
        Some(&rpc(1, "initialize", json!({}))),
        &[],
    )
    .await;
    assert_eq!(init.status, 401);
    let message = init.json()["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        message.contains("unauthorized") || message.contains("invalid"),
        "unexpected 401 body: {message}"
    );

    // A write call with a bad token must not run either (and must not be audited as an action).
    let announce = http(
        addr,
        "POST",
        "/mcp",
        Some("wrong-token"),
        Some(&tool_call(2, "openmesh_announce", json!({}))),
        &[],
    )
    .await;
    assert_eq!(announce.status, 401);
    let after = activity(addr).await["entries"].as_array().unwrap().len();
    assert_eq!(
        before, after,
        "unauthorized calls are not executed or audited"
    );
    bridge.shutdown().await;
}

#[tokio::test]
async fn write_tools_refused_read_only_and_audited() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path(), Some(enabled_config())).await;
    let addr = bridge.addr;
    let response = post(
        addr,
        tool_call(1, "openmesh_announce", json!({})),
        API_TOKEN,
        &[("X-OpenMesh-Agent", "remote-agent")],
    )
    .await;
    assert_eq!(response.status, 200);
    let body = response.json();
    assert_eq!(body["error"]["data"]["code"], "http_scope_read_only");
    assert_eq!(body["error"]["data"]["help_topic"], "agents");

    let activity = activity(addr).await;
    let entry = mcp_entry(&activity).expect("refusal audited");
    assert_eq!(entry["source"], "agent:remote-agent");
    assert_eq!(entry["ok"], false);
    assert!(
        entry["summary"]
            .as_str()
            .unwrap()
            .contains("openmesh_announce")
    );
    assert_eq!(entry["error"], "http_scope_read_only");
    bridge.shutdown().await;
}

#[tokio::test]
async fn allow_write_runs_tools_and_keeps_confirm_gating() {
    let dir = TempDir::new().unwrap();
    let mut config = enabled_config();
    config["allow_write"] = json!(true);
    let bridge = start_bridge(dir.path(), Some(config)).await;
    let addr = bridge.addr;

    let announce = post(
        addr,
        tool_call(1, "openmesh_announce", json!({})),
        API_TOKEN,
        &[("X-OpenMesh-Agent", "claude-remote")],
    )
    .await;
    assert_eq!(announce.status, 200);
    assert_ne!(
        announce.json()["result"]["isError"],
        true,
        "{:?}",
        announce.json()
    );
    let activity = activity(addr).await;
    let entries: Vec<&Value> = activity["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["method"] == "MCP")
        .collect();
    assert!(entries.iter().any(|entry| {
        entry["source"] == "agent:claude-remote"
            && entry["ok"] == true
            && entry["summary"]
                .as_str()
                .unwrap()
                .contains("openmesh_announce")
    }));

    // Dangerous tool still requires confirm:true.
    let refused = post(
        addr,
        tool_call(2, "openmesh_rotate_backbone_token", json!({})),
        API_TOKEN,
        &[],
    )
    .await;
    assert_eq!(refused.json()["result"]["isError"], true);
    assert_eq!(
        refused.json()["result"]["structuredContent"]["error"]["code"],
        "confirm_required"
    );

    let confirmed = post(
        addr,
        tool_call(
            3,
            "openmesh_rotate_backbone_token",
            json!({"confirm": true}),
        ),
        API_TOKEN,
        &[],
    )
    .await;
    assert_ne!(confirmed.json()["result"]["isError"], true);
    bridge.shutdown().await;
}

#[tokio::test]
async fn require_confirm_false_injects_confirm_for_dangerous_tools() {
    let dir = TempDir::new().unwrap();
    let mut config = enabled_config();
    config["allow_write"] = json!(true);
    config["require_confirm"] = json!(false);
    let bridge = start_bridge(dir.path(), Some(config)).await;
    let addr = bridge.addr;
    let rotated = post(
        addr,
        tool_call(1, "openmesh_rotate_backbone_token", json!({})),
        API_TOKEN,
        &[],
    )
    .await;
    assert_eq!(rotated.status, 200);
    assert_ne!(
        rotated.json()["result"]["isError"],
        true,
        "{:?}",
        rotated.json()
    );
    bridge.shutdown().await;
}

#[tokio::test]
async fn sse_post_returns_one_data_frame_and_get_pings() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path(), Some(enabled_config())).await;
    let addr = bridge.addr;

    let response = post(
        addr,
        rpc(1, "initialize", json!({"protocolVersion": "2025-03-26"})),
        API_TOKEN,
        &[("Accept", "text/event-stream")],
    )
    .await;
    assert_eq!(response.status, 200);
    assert_eq!(
        response.headers.get("content-type").map(String::as_str),
        Some("text/event-stream")
    );
    let body = String::from_utf8_lossy(&response.body).to_string();
    assert!(body.starts_with("data: "), "SSE frame: {body}");
    assert_eq!(body.matches("data:").count(), 1);
    assert!(body.contains("\"jsonrpc\""));
    assert!(response.session_id().is_some());

    // GET /mcp streams an SSE keep-alive; the first ping arrives promptly.
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let request = format!(
        "GET /mcp HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {API_TOKEN}\r\nAccept: text/event-stream\r\nConnection: keep-alive\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut chunk = Vec::new();
    let mut buffer = [0u8; 1024];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let read = tokio::time::timeout(Duration::from_secs(3), stream.read(&mut buffer)).await;
        match read {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                chunk.extend_from_slice(&buffer[..n]);
                if chunk.windows(11).any(|w| w == b"event: ping") {
                    break;
                }
            }
            _ => break,
        }
    }
    let text = String::from_utf8_lossy(&chunk);
    assert!(text.starts_with("HTTP/1.1 200"), "GET SSE response: {text}");
    assert!(text.to_ascii_lowercase().contains("text/event-stream"));
    assert!(text.contains("event: ping"), "SSE ping missing: {text}");
    assert!(text.to_ascii_lowercase().contains("mcp-session-id"));
    bridge.shutdown().await;
}

#[tokio::test]
async fn sessions_are_bounded_rate_limited_and_deletable() {
    let dir = TempDir::new().unwrap();
    let mut config = enabled_config();
    config["rate_limit_per_min"] = json!(2);
    config["max_sessions"] = json!(1);
    let bridge = start_bridge(dir.path(), Some(config)).await;
    let addr = bridge.addr;

    let init = post(addr, rpc(1, "initialize", json!({})), API_TOKEN, &[]).await;
    assert_eq!(init.status, 200);
    let session = init.session_id().unwrap().to_string();
    let second = post(
        addr,
        rpc(2, "ping", json!({})),
        API_TOKEN,
        &[("Mcp-Session-Id", &session)],
    )
    .await;
    assert_eq!(second.status, 200);
    let limited = post(
        addr,
        rpc(3, "ping", json!({})),
        API_TOKEN,
        &[("Mcp-Session-Id", &session)],
    )
    .await;
    assert_eq!(limited.status, 429);
    assert!(
        limited.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("rate limit")
    );
    // A second session would exceed max_sessions=1.
    let extra = post(addr, rpc(4, "initialize", json!({})), API_TOKEN, &[]).await;
    assert_eq!(extra.status, 429);
    assert!(
        extra.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("too many MCP sessions")
    );

    // DELETE ends the session and frees the slot.
    let delete = http(
        addr,
        "DELETE",
        "/mcp",
        Some(API_TOKEN),
        None,
        &[("Mcp-Session-Id", &session)],
    )
    .await;
    assert_eq!(delete.status, 200);
    assert_eq!(delete.json()["result"]["ended"], true);
    let again = http(
        addr,
        "DELETE",
        "/mcp",
        Some(API_TOKEN),
        None,
        &[("Mcp-Session-Id", &session)],
    )
    .await;
    assert_eq!(again.json()["result"]["ended"], false);
    // A new session is accepted after the old one ended.
    let fresh = post(addr, rpc(5, "initialize", json!({})), API_TOKEN, &[]).await;
    assert_eq!(fresh.status, 200);
    bridge.shutdown().await;
}

#[tokio::test]
async fn oversized_body_is_rejected() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path(), Some(enabled_config())).await;
    let addr = bridge.addr;
    let huge = "x".repeat(256 * 1024 + 1);
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"pad": huge}});
    let response = post(addr, body, API_TOKEN, &[]).await;
    assert_eq!(response.status, 413);
    assert!(
        response.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("256 KiB")
    );
    bridge.shutdown().await;
}

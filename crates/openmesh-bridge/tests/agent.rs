//! v0.3.0 AI-native surface: help + setup + activity + OpenAPI REST APIs and the scripted MCP
//! stdio server (AI-NATIVE.md §5 acceptance, ux-help-system.md §5).

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::net::TcpStream;
use tokio::process::{Child, ChildStdin, ChildStdout, Command as ProcessCommand};

use openmesh_bridge::transport::backbone::ConfigOverrides;
use openmesh_bridge::{Bridge, BridgeOptions};

const API_TOKEN: &str = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

async fn start_bridge(dir: &Path) -> Bridge {
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

struct HttpResult {
    status: u16,
    content_type: String,
    body: Vec<u8>,
}

impl HttpResult {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

async fn http(
    addr: SocketAddr,
    token: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> HttpResult {
    let mut stream = TcpStream::connect(addr).await.expect("connect API");
    let payload = body.map(|value| value.to_string()).unwrap_or_default();
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n"
    );
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
    tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut response)
        .await
        .unwrap();
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
    let content_type = head
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-type")
                .then(|| value.trim().to_string())
        })
        .unwrap_or_default();
    HttpResult {
        status,
        content_type,
        body,
    }
}

async fn get(addr: SocketAddr, path: &str) -> Value {
    http(addr, API_TOKEN, "GET", path, None, &[]).await.json()
}

async fn post(addr: SocketAddr, path: &str, body: Value) -> HttpResult {
    http(addr, API_TOKEN, "POST", path, Some(body), &[]).await
}

async fn patch_with_agent(addr: SocketAddr, path: &str, body: Value, agent: &str) -> HttpResult {
    http(
        addr,
        API_TOKEN,
        "PATCH",
        path,
        Some(body),
        &[("X-OpenMesh-Agent", agent)],
    )
    .await
}

// --- MCP child process driver ---------------------------------------------------------------

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
}

async fn spawn_mcp(
    addr: SocketAddr,
    read_only: bool,
    agent: &str,
    write_limit: Option<usize>,
) -> Mcp {
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_openmesh-bridge"));
    command
        .arg("mcp")
        .env("OPENMESH_API", format!("http://{addr}"))
        .env("OPENMESH_TOKEN", API_TOKEN)
        .env("OPENMESH_AGENT_NAME", agent)
        .env("OPENMESH_MCP_READ_ONLY", if read_only { "1" } else { "0" })
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    if let Some(limit) = write_limit {
        command.env("OPENMESH_MCP_WRITE_LIMIT", limit.to_string());
    }
    let mut child = command.spawn().expect("spawn openmesh-bridge mcp");
    let stdin = child.stdin.take().expect("mcp stdin");
    let stdout = child.stdout.take().expect("mcp stdout");
    Mcp {
        child,
        stdin,
        lines: BufReader::new(stdout).lines(),
    }
}

impl Mcp {
    async fn send(&mut self, request: Value) {
        let line = request.to_string();
        self.stdin.write_all(line.as_bytes()).await.unwrap();
        self.stdin.write_all(b"\n").await.unwrap();
        self.stdin.flush().await.unwrap();
    }

    async fn request(&mut self, request: Value) -> Value {
        self.send(request).await;
        self.read().await
    }

    async fn read(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(20), self.lines.next_line())
            .await
            .expect("MCP response timeout")
            .expect("MCP stdout read")
            .expect("MCP process exited unexpectedly");
        serde_json::from_str(&line).expect("MCP response is JSON")
    }

    async fn shutdown(mut self) {
        drop(self.stdin);
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
        let _ = self.child.kill().await;
    }
}

// ---------------------------------------------------------------------------------------------
// REST: help / openapi / activity / setup
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn help_api_lists_embedded_topics_and_serves_markdown() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path()).await;
    let addr = bridge.addr;

    let index = get(addr, "/api/help").await;
    let topics = index["topics"].as_array().expect("topics array");
    assert!(
        topics.len() >= 10,
        "embedded content/help index: {topics:?}"
    );
    let slugs: Vec<&str> = topics
        .iter()
        .filter_map(|topic| topic["slug"].as_str())
        .collect();
    assert!(slugs.contains(&"getting-started"));
    assert!(slugs.contains(&"agents"));
    assert!(topics.iter().all(|topic| topic["title"].is_string()));
    assert!(topics.iter().all(|topic| topic["audience"].is_string()));
    assert!(topics.iter().all(|topic| topic["minutes"].is_number()));
    assert!(topics.iter().all(|topic| topic["related"].is_array()));

    let topic = http(
        addr,
        API_TOKEN,
        "GET",
        "/api/help/getting-started",
        None,
        &[],
    )
    .await;
    assert_eq!(topic.status, 200);
    assert!(topic.content_type.contains("application/json"));
    let topic = topic.json();
    assert_eq!(topic["slug"], "getting-started");
    assert!(topic["title"].as_str().unwrap_or("").len() > 3);
    assert_eq!(topic["audience"], "beginner");
    let markdown = topic["markdown"].as_str().expect("markdown");
    assert!(markdown.starts_with("# "), "raw markdown: {markdown:?}");

    let missing = http(addr, API_TOKEN, "GET", "/api/help/no-such-topic", None, &[]).await;
    assert_eq!(missing.status, 404);
    let body = missing.json();
    assert_eq!(body["error"], "not_found");
    assert!(body["code"].is_string());
    assert!(body["help_topic"].is_string());
    assert!(body["hint"].as_str().unwrap_or("").contains("/api/help"));

    bridge.shutdown().await;
}

#[tokio::test]
async fn openapi_is_served_and_activity_attributes_writes() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path()).await;
    let addr = bridge.addr;

    let openapi = http(addr, API_TOKEN, "GET", "/api/openapi.json", None, &[]).await;
    assert_eq!(openapi.status, 200);
    assert!(openapi.content_type.contains("application/json"));
    let document = openapi.json();
    assert_eq!(document["openapi"], "3.1.0");
    assert_eq!(document["info"]["title"], "OpenMesh Bridge API");
    for path in [
        "/api/health",
        "/api/status",
        "/api/help",
        "/api/help/{slug}",
        "/api/setup/state",
        "/api/setup/quick",
        "/api/activity",
        "/api/openapi.json",
    ] {
        assert!(
            document["paths"].get(path).is_some(),
            "openapi must document {path}"
        );
    }
    assert!(document["components"]["securitySchemes"]["bearerAuth"].is_object());

    // Read calls are not logged.
    let _ = get(addr, "/api/status").await;
    assert_eq!(
        get(addr, "/api/activity").await["entries"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // Agent-attributed mutation.
    let patched = patch_with_agent(
        addr,
        "/api/config",
        json!({ "node": { "name": "Agent Node" } }),
        "claude-test",
    )
    .await;
    assert_eq!(patched.status, 200, "patch: {:?}", patched.json());
    assert_eq!(patched.json()["node"]["name"], "Agent Node");
    let activity = get(addr, "/api/activity?limit=10").await;
    let entries = activity["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "one write logged: {activity}");
    assert_eq!(entries[0]["source"], "agent:claude-test");
    assert_eq!(entries[0]["method"], "PATCH");
    assert_eq!(entries[0]["path"], "/api/config");
    assert_eq!(entries[0]["summary"], "update config");
    assert_eq!(entries[0]["ok"], true);
    assert!(entries[0]["error"].is_null());
    assert_eq!(activity["agent_connected"], true);
    assert!(entries[0]["id"].as_u64().unwrap() > 0);
    assert!(entries[0]["at_ms"].as_u64().unwrap() > 0);
    // Never bodies or secrets.
    let raw = activity.to_string();
    assert!(!raw.contains("Agent Node"), "no request bodies in activity");

    // Client attribution and failure tracking.
    let announce = http(
        addr,
        API_TOKEN,
        "POST",
        "/api/announce",
        Some(json!({})),
        &[("X-OpenMesh-Client", "web")],
    )
    .await;
    assert_eq!(announce.status, 200);
    let bad = patch_with_agent(
        addr,
        "/api/config",
        json!({ "node": { "name": "" } }),
        "agent-2",
    )
    .await;
    assert_eq!(bad.status, 400);
    let activity = get(addr, "/api/activity?limit=10").await;
    let entries = activity["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0]["source"], "agent:agent-2");
    assert_eq!(entries[0]["ok"], false);
    assert!(
        entries[0]["error"]
            .as_str()
            .unwrap_or("")
            .contains("must not be empty")
    );
    assert_eq!(entries[1]["source"], "web");
    assert_eq!(entries[1]["path"], "/api/announce");

    // setup/state now reports an agent connection.
    let state = get(addr, "/api/setup/state").await;
    assert_eq!(state["checks"]["agent"]["ok"], true);

    bridge.shutdown().await;
}

#[tokio::test]
async fn setup_state_recommend_and_idempotent_quick_setup() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path()).await;
    let addr = bridge.addr;

    let state = get(addr, "/api/setup/state").await;
    assert_eq!(state["first_run"], true);
    assert_eq!(state["setup_completed"], false);
    for check in [
        "identity",
        "radio",
        "api_token",
        "channel",
        "backbone",
        "agent",
    ] {
        assert!(
            state["checks"].get(check).is_some(),
            "missing check {check}"
        );
        assert!(state["checks"][check]["ok"].is_boolean());
    }
    assert_eq!(state["checks"]["identity"]["ok"], true);
    assert_eq!(state["checks"]["api_token"]["ok"], true);
    assert!(state["recommended"]["region"].is_string());
    assert!(state["recommended"]["profile"].is_string());
    assert!(state["recommended"]["role"].is_string());
    assert!(state["counts"]["channels"].is_number());

    let recommend = get(addr, "/api/setup/recommend?country=NO&role=repeater").await;
    assert_eq!(recommend["region"], "EU868");
    assert_eq!(recommend["profile"], "LONG_FAST");
    assert_eq!(recommend["store_forward"], true);
    assert_eq!(recommend["role"], "repeater");
    assert_eq!(recommend["duty_cycle_pct"], 10);
    let unknown = get(addr, "/api/setup/recommend?country=ZZ&role=client").await;
    assert_eq!(unknown["region"], "EU868");
    assert!(
        unknown["warning"]
            .as_str()
            .unwrap_or("")
            .contains("Unknown country")
    );

    let quick = post(
        addr,
        "/api/setup/quick",
        json!({
            "role": "client",
            "country": "NO",
            "node_name": "Beginner Node",
            "privacy": "private",
            "create_channel": true,
        }),
    )
    .await;
    assert_eq!(quick.status, 200, "quick: {:?}", quick.json());
    let quick = quick.json();
    assert_eq!(quick["applied"]["node_name"], "Beginner Node");
    assert_eq!(quick["applied"]["store_forward"], false);
    assert_eq!(quick["applied"]["position_sharing"], false);
    assert_eq!(quick["applied"]["channel_created"], true);
    assert_eq!(quick["applied"]["announced"], true);
    assert_eq!(quick["setup_completed"], true);
    let channel = &quick["channel"];
    assert_eq!(channel["name"], "general");
    assert_eq!(channel["channel_id"].as_str().unwrap().len(), 16);
    let invite = channel["invite_url"].as_str().expect("invite url");
    assert!(invite.starts_with("openmesh://channel?"), "{invite}");
    assert!(invite.contains("psk="));
    let psk_once = channel["psk_once"].as_str().expect("one-time psk");
    assert!(psk_once.len() >= 40);
    let checklist = quick["checklist"].as_array().unwrap();
    assert!(checklist.len() >= 5);
    assert!(
        checklist
            .iter()
            .any(|item| item["id"] == "channel" && item["ok"] == true)
    );
    let next_steps = quick["next_steps"].as_array().unwrap();
    assert_eq!(next_steps.len(), 4);
    assert!(next_steps.iter().all(|step| step["help_topic"].is_string()));

    // Channel list never exposes the PSK. It contains the built-in Public Mesh channel plus the
    // channel created by quick setup.
    let channels = get(addr, "/api/channels").await;
    let channel_id = channel["channel_id"].as_str().unwrap();
    let listed = channels["channels"].as_array().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(
        listed
            .iter()
            .filter(|c| c["channel_id"] == channel_id)
            .count(),
        1
    );
    assert!(
        listed
            .iter()
            .any(|c| c["name"] == "Public Mesh" && c["public"] == true)
    );
    assert!(listed.iter().all(|c| c["psk_set"] == true));
    assert!(!channels.to_string().contains(psk_once));
    assert!(listed.iter().all(|c| c.get("psk").is_none()));

    // State resumes as completed.
    let state = get(addr, "/api/setup/state").await;
    assert_eq!(state["first_run"], false);
    assert_eq!(state["setup_completed"], true);
    assert_eq!(state["checks"]["channel"]["ok"], true);
    assert_eq!(
        state["counts"]["channels"], 2,
        "Public Mesh + created channel"
    );

    // Idempotent: nothing changes, no duplicate channel, PSK not re-revealed.
    let again = post(
        addr,
        "/api/setup/quick",
        json!({
            "role": "client",
            "country": "NO",
            "node_name": "Beginner Node",
            "privacy": "private",
            "create_channel": true,
        }),
    )
    .await;
    assert_eq!(again.status, 200, "second quick: {:?}", again.json());
    let again = again.json();
    assert!(
        again["applied"].get("channel_created").is_none(),
        "second run must not create a channel: {again}"
    );
    assert_eq!(again["channel"]["psk_once"], Value::Null);
    assert!(
        again["channel"]["invite_url"]
            .as_str()
            .unwrap()
            .contains("psk=")
    );
    assert_eq!(
        get(addr, "/api/channels").await["channels"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "Public Mesh + the quick-setup channel"
    );

    // Bad input is a 400 with code/help_topic.
    let bad = post(
        addr,
        "/api/setup/quick",
        json!({ "role": "wizard", "country": "NO" }),
    )
    .await;
    assert_eq!(bad.status, 400);
    assert_eq!(bad.json()["code"], "bad_request");
    let bad = post(addr, "/api/setup/quick", json!({ "role": "client" })).await;
    assert_eq!(bad.status, 400);
    assert!(bad.json()["message"].as_str().unwrap().contains("country"));

    bridge.shutdown().await;
}

#[tokio::test]
async fn rest_write_endpoints_used_by_mcp_work() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path()).await;
    let addr = bridge.addr;

    // Channel create returns the invite + PSK exactly once.
    let created = post(addr, "/api/channels", json!({ "name": "family" })).await;
    assert_eq!(created.status, 200);
    let created = created.json();
    assert_eq!(created["shown_once"], true);
    assert!(created["psk_once"].as_str().unwrap().len() >= 40);
    assert!(
        created["invite_url"]
            .as_str()
            .unwrap()
            .starts_with("openmesh://channel?")
    );
    let channel_id = created["channel_id"].as_str().unwrap().to_string();

    // Channel message queues and appears in history.
    let sent = post(
        addr,
        &format!("/api/channels/{channel_id}/messages"),
        json!({ "text": "hello channel" }),
    )
    .await;
    assert_eq!(sent.status, 200, "send channel: {:?}", sent.json());
    assert!(sent.json()["msg_id"].as_u64().unwrap() > 0);
    let history = get(addr, &format!("/api/messages/channel/{channel_id}")).await;
    assert!(
        history["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["text"] == "hello channel")
    );

    // Config patch, mark-read and retry validation paths.
    let patched = http(
        addr,
        API_TOKEN,
        "PATCH",
        "/api/config",
        Some(json!({ "features": { "auto_announce": false } })),
        &[],
    )
    .await;
    assert_eq!(patched.status, 200);
    assert_eq!(patched.json()["features"]["auto_announce"], false);
    let missing = post(addr, "/api/messages/999999/read", json!({})).await;
    assert_eq!(missing.status, 200);
    let bad = post(addr, "/api/messages/not-a-number/read", json!({})).await;
    assert_eq!(bad.status, 400);

    // Join by invite URL works and is idempotent.
    let joined = post(
        addr,
        "/api/channels/join",
        json!({ "url": created["invite_url"] }),
    )
    .await;
    assert_eq!(joined.status, 200, "join: {:?}", joined.json());
    assert_eq!(
        joined.json()["channels"].as_array().unwrap().len(),
        2,
        "Public Mesh + the invited channel"
    );

    // Unauthenticated writes are rejected and not logged.
    let unauth = http(
        addr,
        "wrong-token",
        "POST",
        "/api/announce",
        Some(json!({})),
        &[],
    )
    .await;
    assert_eq!(unauth.status, 401);
    let activity = get(addr, "/api/activity?limit=100").await;
    assert!(
        activity["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["source"] != "user" || entry["path"] != "/api/announce"),
        "unauthorised write must not be logged: {activity}"
    );

    bridge.shutdown().await;
}

// ---------------------------------------------------------------------------------------------
// MCP protocol
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn mcp_initialize_tools_resources_prompts_and_confirm_flow() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path()).await;
    let addr = bridge.addr;
    let mut mcp = spawn_mcp(addr, false, "test-mcp", None).await;

    // initialize echoes the client protocol version and advertises the capabilities.
    let init = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": { "name": "test" } }
        }))
        .await;
    assert_eq!(init["jsonrpc"], "2.0");
    assert_eq!(init["id"], 1);
    let result = &init["result"];
    assert_eq!(result["protocolVersion"], "2025-03-26");
    assert_eq!(result["serverInfo"]["name"], "openmesh");
    assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(result["capabilities"]["tools"].is_object());
    assert!(result["capabilities"]["resources"].is_object());
    assert!(result["capabilities"]["prompts"].is_object());
    assert!(result["instructions"].as_str().unwrap().contains("confirm"));

    // Notifications are silent; the next response must still be the ping.
    mcp.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;
    let pong = mcp
        .request(json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }))
        .await;
    assert_eq!(pong["id"], 2);
    assert!(pong["result"].is_object());

    // Full tool catalog.
    let tools = mcp
        .request(json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": {} }))
        .await;
    let tools = tools["result"]["tools"].as_array().expect("tools").clone();
    assert_eq!(tools.len(), 27, "full MCP tool catalog");
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    for expected in [
        "openmesh_health",
        "openmesh_status",
        "openmesh_nodes",
        "openmesh_setup_state",
        "openmesh_activity",
        "openmesh_help",
        "openmesh_send_message",
        "openmesh_rotate_backbone_token",
        "openmesh_kick_backbone_peer",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }
    let rotate = tools
        .iter()
        .find(|tool| tool["name"] == "openmesh_rotate_backbone_token")
        .unwrap();
    assert_eq!(rotate["annotations"]["destructiveHint"], true);
    assert_eq!(rotate["inputSchema"]["required"], json!(["confirm"]));

    // Read tool: human text + structuredContent.
    let health = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "openmesh_health", "arguments": {} }
        }))
        .await;
    let result = &health["result"];
    assert_ne!(result["isError"], true, "health failed: {result}");
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains(&format!("Bridge v{}", env!("CARGO_PKG_VERSION")))
    );
    assert_eq!(
        result["structuredContent"]["health"]["version"],
        env!("CARGO_PKG_VERSION")
    );

    let state = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": { "name": "openmesh_setup_state", "arguments": {} }
        }))
        .await;
    assert_eq!(state["result"]["structuredContent"]["first_run"], true);

    // Destructive tool without confirm: isError + code + help_topic.
    let refused = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 6, "method": "tools/call",
            "params": { "name": "openmesh_rotate_backbone_token", "arguments": {} }
        }))
        .await;
    assert_eq!(refused["result"]["isError"], true);
    assert_eq!(
        refused["result"]["structuredContent"]["error"]["code"],
        "confirm_required"
    );
    assert_eq!(
        refused["result"]["structuredContent"]["error"]["help_topic"],
        "agents"
    );

    // Write tool attributed as an agent in the activity log.
    let announce = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 7, "method": "tools/call",
            "params": { "name": "openmesh_announce", "arguments": {} }
        }))
        .await;
    assert_ne!(announce["result"]["isError"], true);
    let activity = get(addr, "/api/activity?limit=10").await;
    assert_eq!(activity["entries"][0]["source"], "agent:test-mcp");
    assert_eq!(activity["entries"][0]["path"], "/api/announce");

    // HTTP error -> isError with code + help_topic (never a JSON-RPC transport error).
    let missing = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 8, "method": "tools/call",
            "params": { "name": "openmesh_node", "arguments": { "node_id": "00112233445566ff" } }
        }))
        .await;
    assert_ne!(missing["result"]["isError"], false);
    assert!(missing["result"]["structuredContent"]["error"]["code"].is_string());

    // Resources.
    let resources = mcp
        .request(json!({ "jsonrpc": "2.0", "id": 9, "method": "resources/list" }))
        .await;
    let uris: Vec<&str> = resources["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|resource| resource["uri"].as_str())
        .collect();
    assert!(uris.contains(&"openmesh://help/index"));
    assert!(uris.contains(&"openmesh://status"));
    assert!(uris.contains(&"openmesh://nodes"));
    assert!(uris.iter().any(|uri| uri.starts_with("openmesh://help/")));
    let read = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 10, "method": "resources/read",
            "params": { "uri": "openmesh://help/getting-started" }
        }))
        .await;
    assert_eq!(read["result"]["contents"][0]["mimeType"], "text/markdown");
    assert!(
        read["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("# ")
    );
    let unknown_resource = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 11, "method": "resources/read",
            "params": { "uri": "openmesh://nope" }
        }))
        .await;
    assert_eq!(unknown_resource["error"]["code"], -32602);

    // Prompts.
    let prompts = mcp
        .request(json!({ "jsonrpc": "2.0", "id": 12, "method": "prompts/list" }))
        .await;
    let prompt_names: Vec<&str> = prompts["result"]["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|prompt| prompt["name"].as_str())
        .collect();
    assert_eq!(
        prompt_names,
        vec![
            "manage_my_mesh",
            "set_up_my_node",
            "troubleshoot_mesh",
            "set_up_backbone",
            "invite_someone"
        ]
    );
    let prompt = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 13, "method": "prompts/get",
            "params": { "name": "manage_my_mesh", "arguments": {} }
        }))
        .await;
    let text = prompt["result"]["messages"][0]["content"]["text"]
        .as_str()
        .expect("prompt text");
    assert!(text.contains("Safety rules"));
    assert!(text.contains("openmesh_help"));
    let unknown_prompt = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 14, "method": "prompts/get",
            "params": { "name": "nope", "arguments": {} }
        }))
        .await;
    assert_eq!(unknown_prompt["error"]["code"], -32602);

    // Unknown method -> -32601 per the MCP contract.
    let unknown = mcp
        .request(json!({ "jsonrpc": "2.0", "id": 15, "method": "nope/nothing" }))
        .await;
    assert_eq!(unknown["error"]["code"], -32601);

    // Confirm-required flow with real rotation: backend token is shown once via structuredContent
    // while the read-only setup tool redacts connection strings.
    let configured = post(
        addr,
        "/api/backbone/config",
        json!({ "enabled": true, "bind": "127.0.0.1", "port": 0, "auth": "token" }),
    )
    .await;
    assert_eq!(
        configured.status,
        200,
        "backbone enable: {:?}",
        configured.json()
    );
    let rotated = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 16, "method": "tools/call",
            "params": { "name": "openmesh_rotate_backbone_token", "arguments": { "confirm": true } }
        }))
        .await;
    assert_ne!(rotated["result"]["isError"], true, "rotate: {rotated}");
    let token = rotated["result"]["structuredContent"]["token"]
        .as_str()
        .expect("one-time rotated token");
    assert!(token.len() >= 40);
    let setup = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 17, "method": "tools/call",
            "params": { "name": "openmesh_backbone_setup", "arguments": {} }
        }))
        .await;
    let setup_text = setup["result"]["content"][0]["text"].as_str().unwrap();
    assert!(setup_text.contains("redacted"));
    let connection = setup["result"]["structuredContent"]["connection_string"]
        .as_str()
        .unwrap();
    assert!(
        !connection.contains(token),
        "MCP must not leak the OBB token"
    );
    assert!(connection.contains("redacted-by-mcp"));

    mcp.shutdown().await;
    bridge.shutdown().await;
}

#[tokio::test]
async fn mcp_read_only_hides_and_refuses_writes() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path()).await;
    let addr = bridge.addr;
    let mut mcp = spawn_mcp(addr, true, "read-only-agent", None).await;

    let init = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2024-11-05" }
        }))
        .await;
    assert_eq!(init["result"]["serverInfo"]["name"], "openmesh");

    let tools = mcp
        .request(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
        .await;
    let tools = tools["result"]["tools"].as_array().unwrap();
    assert!(!tools.is_empty());
    assert!(
        tools
            .iter()
            .all(|tool| tool["annotations"]["readOnlyHint"] == true)
    );
    assert!(
        !tools
            .iter()
            .any(|tool| tool["name"] == "openmesh_announce"
                || tool["name"] == "openmesh_send_message")
    );

    // Defense in depth: calling a hidden write anyway is refused.
    let refused = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "openmesh_announce", "arguments": {} }
        }))
        .await;
    assert_eq!(refused["result"]["isError"], true);
    assert_eq!(
        refused["result"]["structuredContent"]["error"]["code"],
        "read_only"
    );
    // No mutation reached the bridge.
    assert!(
        get(addr, "/api/activity").await["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // Reads still work in read-only mode.
    let nodes = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "openmesh_nodes", "arguments": {} }
        }))
        .await;
    assert_ne!(nodes["result"]["isError"], true);

    mcp.shutdown().await;
    bridge.shutdown().await;
}

#[tokio::test]
async fn mcp_write_rate_limit_is_enforced() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path()).await;
    let addr = bridge.addr;
    // Small limit keeps the test fast while proving the 30/min default is configurable.
    let mut mcp = spawn_mcp(addr, false, "rate-agent", Some(2)).await;
    let _ = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2024-11-05" }
        }))
        .await;
    for id in 2..=3 {
        let ok = mcp
            .request(json!({
                "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": { "name": "openmesh_announce", "arguments": {} }
            }))
            .await;
        assert_ne!(ok["result"]["isError"], true, "write {id} should pass");
    }
    let limited = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "openmesh_announce", "arguments": {} }
        }))
        .await;
    assert_eq!(limited["result"]["isError"], true);
    assert_eq!(
        limited["result"]["structuredContent"]["error"]["code"],
        "rate_limited"
    );
    mcp.shutdown().await;
    bridge.shutdown().await;
}

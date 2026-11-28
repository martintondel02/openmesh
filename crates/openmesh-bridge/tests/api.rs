//! End-to-end bridge integration test: starts the real daemon (demo mode, ephemeral port),
//! exercises the documented REST + WebSocket API and verifies a full protocol round-trip
//! (message -> transport -> virtual peer engine -> ack -> API event).

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use openmesh_bridge::transport::sim::{MeshConfig, MeshHub};
use openmesh_bridge::{Bridge, BridgeOptions};

const TEST_TOKEN: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

struct TestBridge {
    bridge: Bridge,
    _dir: TempDir,
    addr: SocketAddr,
}

async fn start_bridge() -> TestBridge {
    let dir = TempDir::new().expect("temp dir");
    let options = BridgeOptions {
        listen: SocketAddr::from(([127, 0, 0, 1], 0)),
        token: Some(TEST_TOKEN.to_string()),
        token_file: dir.path().join("bridge-token"),
        db_path: dir.path().join("openmesh.redb"),
        data_dir: dir.path().to_path_buf(),
        identity_seed_path: dir.path().join("identity.key"),
        sim: true,
        demo: true,
        sim_config: MeshConfig {
            loss_pct: 0,
            latency_ms: 5,
            seed: 5,
        },
        mesh_hub: Some(MeshHub::isolated(MeshConfig {
            loss_pct: 0,
            latency_ms: 5,
            seed: 5,
        })),
        udp_port: None,
        api_only: false,
        ..Default::default()
    };
    let bridge = Bridge::start(options).await.expect("bridge starts");
    let addr = bridge.addr;
    TestBridge {
        bridge,
        _dir: dir,
        addr,
    }
}

struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

async fn raw_http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    content_type: Option<&str>,
    body: &[u8],
) -> HttpResponse {
    let mut stream = TcpStream::connect(addr).await.expect("connect to bridge");
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if let Some(content_type) = content_type {
        request.push_str(&format!("Content-Type: {content_type}\r\n"));
    }
    if !body.is_empty() {
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    if !body.is_empty() {
        stream.write_all(body).await.unwrap();
    }
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let text = response;
    let split = text
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response headers");
    let head = String::from_utf8_lossy(&text[..split]).to_string();
    let body = text[split + 4..].to_vec();
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    HttpResponse {
        status,
        headers,
        body,
    }
}

async fn ws_connect(
    addr: SocketAddr,
    token: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>> {
    let url = format!("ws://{addr}/api/ws?token={token}");
    let (stream, _response) = connect_async(&url).await.expect("ws connects");
    stream
}

async fn next_json(
    ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
    timeout: Duration,
) -> Option<Value> {
    let frame = tokio::time::timeout(timeout, ws.next()).await.ok()??.ok()?;
    let text = frame.to_text().ok()?;
    serde_json::from_str(text).ok()
}

/// Read frames until a predicate matches; returns all frames seen.
async fn read_until(
    ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
    timeout: Duration,
    mut predicate: impl FnMut(&Value) -> bool,
) -> Vec<Value> {
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return seen;
        }
        let Some(value) = next_json(ws, remaining).await else {
            return seen;
        };
        let matched = predicate(&value);
        seen.push(value);
        if matched {
            return seen;
        }
    }
}

#[tokio::test]
async fn health_is_public_and_rest_requires_token() {
    let test = start_bridge().await;
    let health = raw_http(test.addr, "GET", "/api/health", None, None, b"").await;
    assert_eq!(health.status, 200);
    let body = health.json();
    assert_eq!(body["ok"], true);
    assert_eq!(body["version"], openmesh_core::CRATE_VERSION);

    let unauthorized = raw_http(test.addr, "GET", "/api/status", None, None, b"").await;
    assert_eq!(unauthorized.status, 401);

    let status = raw_http(test.addr, "GET", "/api/status", Some(TEST_TOKEN), None, b"").await;
    assert_eq!(status.status, 200);
    let body = status.json();
    assert!(body.get("mesh").is_some());
    assert!(body.get("radio").is_some());
    assert!(body.get("storage").is_some());
    assert!(body["uptime_s"].is_number());

    let config = raw_http(test.addr, "GET", "/api/config", Some(TEST_TOKEN), None, b"").await;
    assert_eq!(config.status, 200);
    assert!(config.json().get("node").is_some());

    let nodes = raw_http(test.addr, "GET", "/api/nodes", Some(TEST_TOKEN), None, b"").await;
    assert_eq!(nodes.status, 200);
    let nodes_json = nodes.json();
    assert!(nodes_json.is_array(), "GET /api/nodes returns an array");
    assert!(
        nodes_json
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["name"] == "alice"),
        "seeded demo nodes are listed"
    );

    let unknown = raw_http(test.addr, "GET", "/api/nope", Some(TEST_TOKEN), None, b"").await;
    assert_eq!(unknown.status, 404);

    test.bridge.shutdown().await;
}

#[tokio::test]
async fn attachments_upload_download_roundtrip() {
    let test = start_bridge().await;
    let payload = b"\x89PNG fake image bytes for the bridge test".to_vec();
    let upload = raw_http(
        test.addr,
        "POST",
        "/api/attachments",
        Some(TEST_TOKEN),
        Some("image/png"),
        &payload,
    )
    .await;
    assert_eq!(upload.status, 200, "upload status: {:?}", upload.json());
    let meta = upload.json();
    let id = meta["id"].as_str().expect("attachment id").to_string();
    assert!(id.starts_with("att_"));
    assert_eq!(meta["size"].as_u64(), Some(payload.len() as u64));
    assert_eq!(meta["sha256"].as_str().map(|s| s.len()), Some(64));

    let download = raw_http(
        test.addr,
        "GET",
        &format!("/api/attachments/{id}"),
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(download.status, 200);
    assert_eq!(download.header("content-type"), Some("image/png"));
    assert_eq!(download.body, payload);

    let missing = raw_http(
        test.addr,
        "GET",
        "/api/attachments/att_ffffffffffff",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(missing.status, 404);

    let no_content_type = raw_http(
        test.addr,
        "POST",
        "/api/attachments",
        Some(TEST_TOKEN),
        None,
        b"abc",
    )
    .await;
    assert_eq!(no_content_type.status, 400);

    test.bridge.shutdown().await;
}

#[tokio::test]
async fn websocket_rejects_bad_token_with_4401() {
    let test = start_bridge().await;
    let (mut ws, _response) = connect_async(format!("ws://{}/api/ws?token=wrong-token", test.addr))
        .await
        .expect("handshake completes");
    let frame = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("close frame arrives")
        .expect("some frame")
        .expect("valid frame");
    match frame {
        Message::Close(Some(close)) => {
            assert_eq!(
                close.code,
                tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Library(4401)
            );
        }
        other => panic!("expected close 4401, got {other:?}"),
    }
    test.bridge.shutdown().await;
}

#[tokio::test]
async fn websocket_sync_create_channel_and_send_group_message() {
    let test = start_bridge().await;
    let mut ws = ws_connect(test.addr, TEST_TOKEN).await;

    // 3.1 hello is sent immediately.
    let hello = next_json(&mut ws, Duration::from_secs(5))
        .await
        .expect("hello frame");
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["v"], 1);
    assert_eq!(hello["data"]["protocol"], "omp/1");
    assert!(hello["data"]["node"]["node_id"].is_string());
    // The bridge advertises its own public identity so clients can add it as a contact.
    assert_eq!(
        hello["data"]["node"]["public_key"]
            .as_str()
            .map(str::len)
            .unwrap_or(0),
        96
    );
    assert!(hello["data"]["radios"].is_array());
    assert!(
        hello["data"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "images")
    );

    // 4.1 initial sync ends with sync_done, includes seeded history.
    ws.send(Message::Text(
        json!({"v":1,"type":"sync","id":"s1","data":{"since_ms":0}}).to_string(),
    ))
    .await
    .unwrap();
    let frames = read_until(&mut ws, Duration::from_secs(5), |v| {
        v["type"] == "sync_done"
    })
    .await;
    let sync_done = frames.last().expect("sync_done");
    assert_eq!(sync_done["id"], "s1");
    assert!(
        sync_done["data"]["messages"].as_u64().unwrap() >= 5,
        "seeded messages sync: {sync_done}"
    );
    assert!(sync_done["data"]["nodes"].as_u64().unwrap() >= 3);

    // 4.4 create a channel: psk returned exactly once.
    ws.send(Message::Text(
        json!({"v":1,"type":"create_channel","id":"c1","data":{"name":"testers"}}).to_string(),
    ))
    .await
    .unwrap();
    let frames = read_until(&mut ws, Duration::from_secs(5), |v| {
        v["type"] == "channel_created"
    })
    .await;
    let created = frames.last().expect("channel_created");
    let channel_id = created["data"]["channel_id"]
        .as_str()
        .expect("channel id")
        .to_string();
    assert_eq!(created["id"], "c1");
    assert!(created["data"]["psk"].as_str().unwrap().len() > 20);

    // 4.2 send a group message; the reply and the broadcast echo arrive in either order, so
    // wait for both regardless of scheduling.
    ws.send(
        Message::Text(
            json!({"v":1,"type":"send_group_message","id":"g1","data":{"channel_id":channel_id,"text":"hello group"}})
                .to_string(),
        ),
    )
    .await
    .unwrap();
    let mut send_result: Option<Value> = None;
    let mut echo: Option<Value> = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while send_result.is_none() || echo.is_none() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Some(value) = next_json(&mut ws, remaining).await else {
            panic!("timed out waiting for send_result/group echo");
        };
        if value["type"] == "send_result" && value["id"] == "g1" {
            send_result = Some(value);
        } else if value["type"] == "group_message"
            && value["data"]["text"] == "hello group"
            && value["data"]["outgoing"] == true
        {
            echo = Some(value);
        }
    }
    let send_result = send_result.expect("send_result");
    assert_eq!(send_result["id"], "g1");
    assert!(send_result["data"]["msg_id"].is_number());
    let echo = echo.expect("group_message echo");
    assert_eq!(echo["data"]["channel_id"], channel_id);
    assert_eq!(echo["data"]["outgoing"], true);
    assert_eq!(echo["data"]["channel_name"], "testers");

    // get_channels now includes the new channel.
    ws.send(Message::Text(
        json!({"v":1,"type":"get_channels","id":"ch1"}).to_string(),
    ))
    .await
    .unwrap();
    let frames = read_until(&mut ws, Duration::from_secs(5), |v| v["type"] == "channels").await;
    let channels = frames.last().unwrap();
    assert!(
        channels["data"]["channels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["channel_id"] == channel_id),
        "created channel listed"
    );

    // Unknown command types are ignored, not errors (§7).
    ws.send(Message::Text(
        json!({"v":1,"type":"future_command","id":"x1"}).to_string(),
    ))
    .await
    .unwrap();
    ws.send(Message::Text(
        json!({"v":1,"type":"get_config","id":"cfg1"}).to_string(),
    ))
    .await
    .unwrap();
    let frames = read_until(&mut ws, Duration::from_secs(5), |v| v["type"] == "config").await;
    assert_eq!(
        frames.last().unwrap()["id"],
        "cfg1",
        "unknown types are ignored, config still answers"
    );

    test.bridge.shutdown().await;
}

#[tokio::test]
async fn websocket_direct_message_roundtrip_with_ack() {
    let test = start_bridge().await;
    let mut ws = ws_connect(test.addr, TEST_TOKEN).await;
    let hello = next_json(&mut ws, Duration::from_secs(5))
        .await
        .expect("hello");
    assert_eq!(hello["type"], "hello");

    // Find alice's node id (seeded demo contact).
    let contacts = raw_http(
        test.addr,
        "GET",
        "/api/contacts",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(contacts.status, 200);
    let contacts_json = contacts.json();
    let alice = contacts_json["contacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "alice")
        .expect("alice seeded");
    let alice_id = alice["node_id"].as_str().unwrap().to_string();

    // Give the demo peers a moment to announce on the virtual mesh.
    tokio::time::sleep(Duration::from_millis(600)).await;

    // Send a direct message; expect send_result, outgoing message echo, then a delivery ack.
    ws.send(
        Message::Text(
            json!({"v":1,"type":"send_message","id":"m1","data":{"to":alice_id,"text":"integration hello"}})
                .to_string(),
        ),
    )
    .await
    .unwrap();

    // The request/response (`send_result`) and the broadcast echo race on the wire, so wait for
    // both regardless of which ordering the scheduler picks.
    let mut frames: Vec<Value> = Vec::new();
    let mut send_result: Option<Value> = None;
    let mut echo: Option<Value> = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while send_result.is_none() || echo.is_none() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!("timed out waiting for send_result/echo, frames: {frames:?}");
        }
        let Some(value) = next_json(&mut ws, remaining).await else {
            panic!("bridge closed while waiting for send_result/echo");
        };
        if value["type"] == "send_result" && value["id"] == "m1" {
            send_result = Some(value.clone());
        }
        if value["type"] == "message"
            && value["data"]["outgoing"] == true
            && value["data"]["text"] == "integration hello"
        {
            echo = Some(value.clone());
        }
        frames.push(value);
    }
    let send_result = send_result.expect("send_result");
    let msg_id = send_result["data"]["msg_id"].as_u64().expect("msg id");
    let echo = echo.expect("outgoing message echo");
    assert_eq!(echo["data"]["peer"], alice_id);
    assert_eq!(echo["data"]["text"], "integration hello");

    // The real end-to-end proof: the virtual peer engine decrypted the message and acked it.
    let frames = read_until(&mut ws, Duration::from_secs(20), |v| {
        v["type"] == "message_update"
            && v["data"]["msg_id"].as_u64() == Some(msg_id)
            && v["data"]["delivered"] == true
            || v["type"] == "ack" && v["data"]["msg_id"].as_u64() == Some(msg_id)
    })
    .await;
    assert!(
        !frames.is_empty(),
        "expected a delivery ack or delivered message_update for msg {msg_id}"
    );
    let delivered = frames.iter().any(|v| {
        (v["type"] == "ack" && v["data"]["kind"] == "delivered")
            || (v["type"] == "message_update" && v["data"]["delivered"] == true)
    });
    assert!(delivered, "delivery confirmation frames: {frames:?}");

    // REST history now contains the message.
    let history = raw_http(
        test.addr,
        "GET",
        &format!("/api/messages?peer={alice_id}&limit=50"),
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(history.status, 200);
    let history_json = history.json();
    let found = history_json["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["msg_id"].as_u64() == Some(msg_id) && m["text"] == "integration hello");
    assert!(found, "sent message persisted");

    test.bridge.shutdown().await;
}

#[tokio::test]
async fn rest_endpoints_cover_documented_surface() {
    let test = start_bridge().await;

    // Channels and channel history.
    let channels = raw_http(
        test.addr,
        "GET",
        "/api/channels",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(channels.status, 200);
    let channel_id = channels.json()["channels"][0]["channel_id"]
        .as_str()
        .unwrap()
        .to_string();
    let group_history = raw_http(
        test.addr,
        "GET",
        &format!("/api/messages/channel/{channel_id}?limit=10"),
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(group_history.status, 200);
    assert!(
        group_history.json()["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["channel_id"] == channel_id),
        "seeded group messages are returned"
    );

    // Single node lookup and unknown node.
    let nodes = raw_http(test.addr, "GET", "/api/nodes", Some(TEST_TOKEN), None, b"")
        .await
        .json();
    let node_id = nodes.as_array().unwrap()[0]["node_id"]
        .as_str()
        .unwrap()
        .to_string();
    let one = raw_http(
        test.addr,
        "GET",
        &format!("/api/nodes/{node_id}"),
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(one.status, 200);
    assert_eq!(one.json()["node_id"], node_id);
    let missing_node = raw_http(
        test.addr,
        "GET",
        "/api/nodes/0000000000000000",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(missing_node.status, 404);

    // Contacts.
    let contacts = raw_http(
        test.addr,
        "GET",
        "/api/contacts",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(contacts.status, 200);
    assert!(contacts.json()["contacts"].as_array().unwrap().len() >= 3);

    // Static route: the built web client when present, otherwise the informational page.
    let root = raw_http(test.addr, "GET", "/", None, None, b"").await;
    assert_eq!(root.status, 200);
    let content_type = root.header("content-type").unwrap_or("");
    assert!(
        content_type.contains("text/html")
            || String::from_utf8_lossy(&root.body).contains("OpenMesh bridge"),
        "expected static index.html or the informational page, got {content_type}"
    );

    // CORS preflight for the dev web origin.
    let preflight = raw_http(test.addr, "OPTIONS", "/api/messages", None, None, b"").await;
    assert_eq!(preflight.status, 204);
    assert_eq!(preflight.header("access-control-allow-origin"), Some("*"));

    test.bridge.shutdown().await;
}

/// Helper: start a bare (non-demo) bridge in an existing data directory, reusing files.
async fn start_bridge_in(dir: &std::path::Path, token: &str) -> Bridge {
    let options = BridgeOptions {
        listen: SocketAddr::from(([127, 0, 0, 1], 0)),
        token: Some(token.to_string()),
        token_file: dir.join("bridge-token"),
        db_path: dir.join("openmesh.redb"),
        data_dir: dir.to_path_buf(),
        identity_seed_path: dir.join("identity.key"),
        sim: true,
        demo: true,
        sim_config: MeshConfig {
            loss_pct: 0,
            latency_ms: 5,
            seed: 9,
        },
        mesh_hub: None,
        udp_port: None,
        api_only: false,
        ..Default::default()
    };
    Bridge::start(options).await.expect("bridge starts")
}

#[tokio::test]
async fn attachment_message_end_to_end() {
    let test = start_bridge().await;
    let mut ws = ws_connect(test.addr, TEST_TOKEN).await;
    assert_eq!(
        next_json(&mut ws, Duration::from_secs(5))
            .await
            .expect("hello")["type"],
        "hello"
    );

    // Upload raw bytes via the documented REST endpoint.
    let payload: Vec<u8> = (0..1500u32).map(|i| (i * 7 % 253) as u8).collect();
    let upload = raw_http(
        test.addr,
        "POST",
        "/api/attachments",
        Some(TEST_TOKEN),
        Some("image/jpeg"),
        &payload,
    )
    .await;
    assert_eq!(upload.status, 200);
    let attachment_id = upload.json()["id"].as_str().unwrap().to_string();

    // Find alice and send the attachment message.
    let contacts = raw_http(
        test.addr,
        "GET",
        "/api/contacts",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await
    .json();
    let alice_id = contacts["contacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "alice")
        .unwrap()["node_id"]
        .as_str()
        .unwrap()
        .to_string();
    tokio::time::sleep(Duration::from_millis(500)).await;
    ws.send(Message::Text(
        json!({"v":1,"type":"send_message","id":"att1","data":{"to":alice_id,"attachment_id":attachment_id}})
            .to_string(),
    ))
    .await
    .unwrap();

    // Wait for the outgoing attachment message echo and the transfer to complete (progress 1.0,
    // delivered through the virtual mesh).
    let mut saw_echo = false;
    let mut completed = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while (!saw_echo || !completed) && tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Some(value) = next_json(&mut ws, remaining).await else {
            break;
        };
        if value["type"] == "message"
            && value["data"]["text"].is_null()
            && value["data"]["attachment"]["id"] == attachment_id
        {
            saw_echo = true;
        }
        if value["type"] == "message_update"
            && value["data"]["attachment"]["id"] == attachment_id
            && value["data"]["attachment"]["progress"].as_f64() == Some(1.0)
        {
            completed = true;
        }
        if value["type"] == "message_update" && value["data"]["delivered"] == true {
            completed = true;
        }
    }
    assert!(
        saw_echo,
        "attachment message echo must be pushed to clients"
    );
    assert!(completed, "attachment transfer must complete end to end");

    // The stored message exposes the metadata and the bytes are downloadable.
    let history = raw_http(
        test.addr,
        "GET",
        &format!("/api/messages?peer={alice_id}&limit=20"),
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await
    .json();
    let message = history["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["attachment"]["id"] == attachment_id)
        .expect("attachment message stored");
    assert_eq!(message["attachment"]["mime"], "image/jpeg");
    assert_eq!(message["attachment"]["size"], 1500);
    let download = raw_http(
        test.addr,
        "GET",
        &format!("/api/attachments/{attachment_id}"),
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(download.status, 200);
    assert_eq!(download.header("content-type"), Some("image/jpeg"));
    assert_eq!(download.body, payload);

    test.bridge.shutdown().await;
}

#[tokio::test]
async fn state_persists_across_bridge_restarts() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge_in(dir.path(), TEST_TOKEN).await;
    let addr = bridge.addr;

    // Create a channel and send a group message through the API.
    let mut ws = ws_connect(addr, TEST_TOKEN).await;
    assert_eq!(
        next_json(&mut ws, Duration::from_secs(5))
            .await
            .expect("hello")["type"],
        "hello"
    );
    ws.send(Message::Text(
        json!({"v":1,"type":"create_channel","id":"c1","data":{"name":"persisted"}}).to_string(),
    ))
    .await
    .unwrap();
    let frames = read_until(&mut ws, Duration::from_secs(5), |v| {
        v["type"] == "channel_created"
    })
    .await;
    let channel_id = frames.last().unwrap()["data"]["channel_id"]
        .as_str()
        .unwrap()
        .to_string();
    ws.send(Message::Text(
        json!({"v":1,"type":"send_group_message","id":"g1","data":{"channel_id":channel_id,"text":"survives restart"}})
            .to_string(),
    ))
    .await
    .unwrap();
    read_until(&mut ws, Duration::from_secs(5), |v| {
        v["type"] == "group_message"
    })
    .await;
    // Ensure the write transaction has committed before the daemon stops.
    tokio::time::sleep(Duration::from_millis(200)).await;
    bridge.shutdown().await;

    // Restart on the same data directory with a fresh token value; state must survive.
    let bridge2 = start_bridge_in(dir.path(), TEST_TOKEN).await;
    let channels = raw_http(
        bridge2.addr,
        "GET",
        "/api/channels",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(channels.status, 200);
    assert!(
        channels.json()["channels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["channel_id"] == channel_id
                && c["name"] == "persisted"
                && c["psk_set"] == true),
        "channel (including its key) must survive a restart"
    );
    let history = raw_http(
        bridge2.addr,
        "GET",
        &format!("/api/messages/channel/{channel_id}?limit=20"),
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await
    .json();
    assert!(
        history["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["text"] == "survives restart"),
        "group message history must survive a restart"
    );
    let contacts = raw_http(
        bridge2.addr,
        "GET",
        "/api/contacts",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await
    .json();
    assert!(contacts["contacts"].as_array().unwrap().len() >= 3);
    bridge2.shutdown().await;
}

/// One round-trip helper: send a command and wait for a given response type (echoes id).
async fn ws_roundtrip(
    ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
    typ: &str,
    data: Value,
    id: &str,
    expect: &str,
) -> Value {
    ws.send(Message::Text(
        json!({"v":1,"type":typ,"id":id,"data":data}).to_string(),
    ))
    .await
    .unwrap();
    let frames = read_until(ws, Duration::from_secs(5), |v| {
        v["type"] == expect && v["id"] == id
    })
    .await;
    frames
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("no {expect} for {typ}, frames: {frames:?}"))
}

#[tokio::test]
async fn websocket_commands_cover_documented_surface() {
    let test = start_bridge().await;
    let mut ws = ws_connect(test.addr, TEST_TOKEN).await;
    let hello = next_json(&mut ws, Duration::from_secs(5))
        .await
        .expect("hello");
    assert_eq!(hello["type"], "hello");
    // The bridge advertises its own 72-byte public identity (base64) so clients can add it as a
    // contact and keep verifying its signed messages across client restarts.
    let self_id = hello["data"]["node"]["node_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        hello["data"]["node"]["public_key"]
            .as_str()
            .map(str::len)
            .unwrap_or(0),
        96
    );

    // Seed the sync history so mark_read/retry have real message ids.
    ws.send(Message::Text(
        json!({"v":1,"type":"sync","id":"s1","data":{"since_ms":0}}).to_string(),
    ))
    .await
    .unwrap();
    let frames = read_until(&mut ws, Duration::from_secs(5), |v| {
        v["type"] == "sync_done"
    })
    .await;
    let messages: Vec<Value> = frames
        .iter()
        .filter(|v| v["type"] == "message" || v["type"] == "group_message")
        .cloned()
        .collect();
    let incoming = messages
        .iter()
        .find(|m| m["type"] == "message" && m["data"]["outgoing"] == false)
        .cloned()
        .expect("seeded incoming message");
    let outgoing = messages
        .iter()
        .find(|m| m["type"] == "message" && m["data"]["outgoing"] == true)
        .cloned()
        .expect("seeded outgoing message");
    let incoming_id = incoming["data"]["msg_id"].as_u64().unwrap();
    let outgoing_id = outgoing["data"]["msg_id"].as_u64().unwrap();

    // 4.2 mark_read -> read ack is sent and a message_update is pushed.
    ws.send(Message::Text(
        json!({"v":1,"type":"mark_read","id":"mr1","data":{"msg_id":incoming_id}}).to_string(),
    ))
    .await
    .unwrap();
    let frames = read_until(&mut ws, Duration::from_secs(5), |v| {
        v["type"] == "message_update"
            && v["data"]["msg_id"].as_u64() == Some(incoming_id)
            && v["data"]["read"] == true
    })
    .await;
    assert!(!frames.is_empty(), "mark_read must push an updated message");

    // 4.2 retry an outgoing message -> new send_result for the same msg id.
    let retried = ws_roundtrip(
        &mut ws,
        "retry",
        json!({"msg_id": outgoing_id}),
        "r1",
        "send_result",
    )
    .await;
    assert_eq!(retried["data"]["msg_id"].as_u64(), Some(outgoing_id));

    // 4.2 delete_message -> local delete notification.
    ws.send(Message::Text(
        json!({"v":1,"type":"delete_message","id":"d1","data":{"msg_id":outgoing_id}}).to_string(),
    ))
    .await
    .unwrap();
    let frames = read_until(&mut ws, Duration::from_secs(5), |v| {
        v["type"] == "message_update"
            && v["data"]["msg_id"].as_u64() == Some(outgoing_id)
            && v["data"]["deleted"] == true
    })
    .await;
    assert!(!frames.is_empty(), "delete_message must notify clients");

    // 4.5 get_stats -> stats event with per-link counters.
    let stats = ws_roundtrip(&mut ws, "get_stats", json!({}), "st1", "stats").await;
    assert!(stats["data"]["links"].is_array());
    assert!(stats["data"]["tx_frames"].is_number());

    // 4.5 get_messages -> messages event.
    let peer = incoming["data"]["peer"].as_str().unwrap().to_string();
    let history = ws_roundtrip(
        &mut ws,
        "get_messages",
        json!({"peer": peer, "limit": 10}),
        "gm1",
        "messages",
    )
    .await;
    assert!(history["data"]["messages"].is_array());

    // 4.5 get_nodes_map -> nodes event.
    let nodes = ws_roundtrip(&mut ws, "get_nodes_map", json!({}), "nm1", "nodes").await;
    assert!(nodes["data"]["nodes"].as_array().unwrap().len() >= 3);

    // 4.5 radio_scan -> documented radio_busy error (error envelopes carry the request id).
    ws.send(Message::Text(
        json!({"v":1,"type":"radio_scan","id":"rs1"}).to_string(),
    ))
    .await
    .unwrap();
    let frames = read_until(&mut ws, Duration::from_secs(5), |v| {
        v["type"] == "error" && v["id"] == "rs1"
    })
    .await;
    assert_eq!(frames.last().unwrap()["data"]["code"], "radio_busy");

    // 4.3 contact_qr -> openmesh:// contact URL.
    let contacts = raw_http(
        test.addr,
        "GET",
        "/api/contacts",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await
    .json();
    let alice = contacts["contacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "alice")
        .cloned()
        .expect("alice contact");
    let alice_id = alice["node_id"].as_str().unwrap().to_string();
    let qr = ws_roundtrip(
        &mut ws,
        "contact_qr",
        json!({"node_id": alice_id}),
        "qr1",
        "contact_url",
    )
    .await;
    assert!(
        qr["data"]["url"]
            .as_str()
            .unwrap()
            .starts_with("openmesh://contact?")
    );

    // 4.3b contact_qr for the bridge's own node returns its self invite.
    let self_qr = ws_roundtrip(
        &mut ws,
        "contact_qr",
        json!({"node_id": self_id}),
        "qr2",
        "contact_url",
    )
    .await;
    let self_url = self_qr["data"]["url"].as_str().unwrap_or_default();
    assert!(self_url.starts_with("openmesh://contact?"), "{self_url}");
    assert!(self_url.contains(&self_id), "{self_url}");

    // 4.4 channel_invite -> openmesh:// channel URL.
    let channels = raw_http(
        test.addr,
        "GET",
        "/api/channels",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await
    .json();
    let channel_id = channels["channels"][0]["channel_id"]
        .as_str()
        .unwrap()
        .to_string();
    let invite = ws_roundtrip(
        &mut ws,
        "channel_invite",
        json!({"channel_id": channel_id}),
        "ci1",
        "channel_url",
    )
    .await;
    assert!(
        invite["data"]["url"]
            .as_str()
            .unwrap()
            .starts_with("openmesh://channel?")
    );

    // 4.3 add_contact / remove_contact -> contacts snapshot.
    let bob = contacts["contacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "bob")
        .cloned()
        .expect("bob contact");
    let bob_id = bob["node_id"].as_str().unwrap().to_string();
    let bob_identity = bob["identity"].as_str().unwrap().to_string();
    let contacts_after = ws_roundtrip(
        &mut ws,
        "add_contact",
        json!({"node_id": bob_id, "name": "Bobby", "identity": bob_identity, "verify_key": true}),
        "ac1",
        "contacts",
    )
    .await;
    assert!(
        contacts_after["data"]["contacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["node_id"] == bob_id && c["name"] == "Bobby")
    );
    let contacts_after = ws_roundtrip(
        &mut ws,
        "remove_contact",
        json!({"node_id": bob_id}),
        "rc1",
        "contacts",
    )
    .await;
    assert!(
        !contacts_after["data"]["contacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["node_id"] == bob_id)
    );

    // 4.5 set_config applies node/radio/features changes and pushes a full config snapshot.
    let config = ws_roundtrip(
        &mut ws,
        "set_config",
        json!({"node": {"name": "cli-test-node", "class": "tracker"},
               "radio": {"profile": "LONG_FAST", "region": "US915"},
               "features": {"auto_announce": false},
               "position": {"lat": 59.9, "lon": 10.7, "timestamp_ms": 1}}),
        "sc1",
        "config",
    )
    .await;
    assert_eq!(config["data"]["node"]["name"], "cli-test-node");
    assert_eq!(config["data"]["node"]["class"], "tracker");
    assert_eq!(config["data"]["radio"]["profile"], "LONG_FAST");
    assert_eq!(config["data"]["radio"]["region"], "US915");
    assert_eq!(config["data"]["features"]["auto_announce"], false);

    // 4.2 send_position (explicit override) -> position event addressed to the channel.
    let _ = ws_roundtrip(
        &mut ws,
        "send_position",
        json!({"channel_id": channel_id, "position": {"lat": 59.91, "lon": 10.75, "timestamp_ms": 2}}),
        "sp1",
        "send_result",
    )
    .await;
    let frames = read_until(&mut ws, Duration::from_secs(5), |v| v["type"] == "position").await;
    assert!(
        !frames.is_empty(),
        "send_position must push a position event"
    );

    // invalid payloads produce bad_request errors instead of dropping the connection.
    ws.send(Message::Text(
        json!({"v":1,"type":"send_message","id":"bad1","data":{}}).to_string(),
    ))
    .await
    .unwrap();
    let frames = read_until(&mut ws, Duration::from_secs(5), |v| {
        v["type"] == "error" && v["id"] == "bad1"
    })
    .await;
    assert_eq!(frames.last().unwrap()["data"]["code"], "bad_request");

    test.bridge.shutdown().await;
}

#[tokio::test]
async fn sse_stream_pushes_events() {
    let test = start_bridge().await;
    let mut stream = TcpStream::connect(test.addr).await.unwrap();
    let request = format!(
        "GET /api/events HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TEST_TOKEN}\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 8192];
    let mut received = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !received.contains("data:") && tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Ok(read) = tokio::time::timeout(remaining, stream.read(&mut buf)).await else {
            break;
        };
        let Ok(n) = read else { break };
        if n == 0 {
            break;
        }
        received.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    assert!(
        received.contains("text/event-stream"),
        "SSE headers: {received}"
    );
    assert!(
        received.contains("data:"),
        "SSE must stream events: {received}"
    );
    let event = received.split("data:").nth(1).unwrap_or("");
    assert!(
        event.contains("\"type\""),
        "SSE data must be a JSON envelope: {event}"
    );

    let unauthorized = raw_http(test.addr, "GET", "/api/events", None, None, b"").await;
    assert_eq!(unauthorized.status, 401);
    test.bridge.shutdown().await;
}

#[tokio::test]
async fn cancel_message_rest_and_ws_set_terminal_cancelled_state() {
    let test = start_bridge().await;
    let mut ws = ws_connect(test.addr, TEST_TOKEN).await;
    let _hello = next_json(&mut ws, Duration::from_secs(5))
        .await
        .expect("hello");

    // Fetch the seeded history so we have real direct message ids.
    ws.send(Message::Text(
        json!({"v":1,"type":"sync","id":"s1","data":{"since_ms":0}}).to_string(),
    ))
    .await
    .unwrap();
    let frames = read_until(&mut ws, Duration::from_secs(5), |v| {
        v["type"] == "sync_done"
    })
    .await;
    let messages: Vec<Value> = frames
        .iter()
        .filter(|v| v["type"] == "message")
        .cloned()
        .collect();
    let incoming_id = messages
        .iter()
        .find(|m| m["data"]["outgoing"] == false)
        .map(|m| m["data"]["msg_id"].as_u64().unwrap())
        .expect("incoming message");
    let outgoing: Vec<Value> = messages
        .iter()
        .filter(|m| m["data"]["outgoing"] == true)
        .cloned()
        .collect();
    assert!(outgoing.len() >= 2, "demo seeds at least two outgoing DMs");
    let rest_id = outgoing[0]["data"]["msg_id"].as_u64().unwrap() as u32;
    let ws_id = outgoing[1]["data"]["msg_id"].as_u64().unwrap() as u32;
    let rest_peer = outgoing[0]["data"]["peer"].as_str().unwrap().to_string();
    let ws_peer = outgoing[1]["data"]["peer"].as_str().unwrap().to_string();

    // Simulate in-flight/queued sends (demo seeds are delivered): mark both as not delivered so
    // the bridge's terminal-state guard allows cancellation.
    for id in [rest_id, ws_id] {
        let mut record = test
            .bridge
            .shared
            .store
            .get_message(id)
            .expect("store read")
            .expect("seeded message");
        record.delivered = false;
        record.cancelled = false;
        test.bridge
            .shared
            .store
            .put_message(&record)
            .expect("store");
    }

    // Unknown id -> 404; inbound message -> 400.
    let unknown = raw_http(
        test.addr,
        "POST",
        "/api/messages/4294967290/cancel",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(unknown.status, 404, "{}", unknown.json());
    let inbound = raw_http(
        test.addr,
        "POST",
        &format!("/api/messages/{incoming_id}/cancel"),
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(inbound.status, 400, "{}", inbound.json());

    // REST cancel -> 200 with cancelled=true; second call is idempotent (already_cancelled).
    let cancelled = raw_http(
        test.addr,
        "POST",
        &format!("/api/messages/{rest_id}/cancel"),
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(cancelled.status, 200, "{}", cancelled.json());
    let body = cancelled.json();
    assert_eq!(body["ok"], true);
    assert_eq!(body["cancelled"], true);
    assert_eq!(body["msg_id"].as_u64(), Some(u64::from(rest_id)));
    let again = raw_http(
        test.addr,
        "POST",
        &format!("/api/messages/{rest_id}/cancel"),
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(again.status, 200);
    assert_eq!(again.json()["already_cancelled"], true);
    // Drain the REST cancel's broadcast update before driving the same WS stream.
    let rest_updates = read_until(&mut ws, Duration::from_secs(5), |v| {
        v["type"] == "message_update"
            && v["data"]["msg_id"].as_u64() == Some(u64::from(rest_id))
            && v["data"]["cancelled"] == true
    })
    .await;
    assert!(
        !rest_updates.is_empty(),
        "REST cancel must push a cancelled message_update"
    );

    // WS cancel_message -> cancel_result reply and message_update cancelled=true.
    ws.send(Message::Text(
        json!({"v":1,"type":"cancel_message","id":"c1","data":{"msg_id":ws_id}}).to_string(),
    ))
    .await
    .unwrap();
    let frames = read_until(&mut ws, Duration::from_secs(5), |v| {
        v["type"] == "cancel_result" && v["id"] == "c1"
    })
    .await;
    let reply = frames
        .iter()
        .find(|v| v["type"] == "cancel_result" && v["id"] == "c1")
        .expect("cancel_result reply");
    assert_eq!(reply["data"]["cancelled"], true);
    assert_eq!(reply["data"]["msg_id"].as_u64(), Some(u64::from(ws_id)));
    let ws_updates = read_until(&mut ws, Duration::from_secs(5), |v| {
        v["type"] == "message_update"
            && v["data"]["msg_id"].as_u64() == Some(u64::from(ws_id))
            && v["data"]["cancelled"] == true
    })
    .await;
    assert!(
        !ws_updates.is_empty(),
        "WS cancel must push message_update with cancelled=true"
    );

    // The persisted rows expose the terminal cancelled flag in history (one peer each).
    for (peer, id) in [(rest_peer, rest_id), (ws_peer, ws_id)] {
        let history = ws_roundtrip(
            &mut ws,
            "get_messages",
            json!({"peer": peer, "limit": 50}),
            "h1",
            "messages",
        )
        .await;
        let rows = history["data"]["messages"].as_array().expect("messages");
        let row = rows
            .iter()
            .find(|row| row["msg_id"].as_u64() == Some(u64::from(id)))
            .unwrap_or_else(|| panic!("cancelled row {id} for peer {peer} missing: {history}"));
        assert_eq!(row["cancelled"], true, "row must be cancelled: {row}");
    }

    test.bridge.shutdown().await;
}

#[tokio::test]
async fn public_mesh_channel_is_builtin_join_is_idempotent_and_leave_is_forbidden() {
    let test = start_bridge().await;
    let public = openmesh_core::public_channel();

    let channels = raw_http(
        test.addr,
        "GET",
        "/api/channels",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await;
    assert_eq!(channels.status, 200);
    let listed = channels.json();
    let public_entry = listed["channels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|channel| channel["channel_id"] == public.id_hex)
        .expect("Public Mesh channel exists on startup");
    assert_eq!(public_entry["name"], "Public Mesh");
    assert_eq!(public_entry["public"], true);
    assert_eq!(public_entry["psk_set"], true);

    // Joining via the canonical invite URL is idempotent and returns the same public channel.
    let joined = raw_http(
        test.addr,
        "POST",
        "/api/channels/join",
        Some(TEST_TOKEN),
        Some("application/json"),
        serde_json::to_vec(&json!({ "url": public.join_url }))
            .unwrap()
            .as_slice(),
    )
    .await;
    assert_eq!(joined.status, 200, "join response: {:?}", joined.json());
    let after_join = raw_http(
        test.addr,
        "GET",
        "/api/channels",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await
    .json();
    let public_count = after_join["channels"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|channel| channel["channel_id"] == public.id_hex)
        .count();
    assert_eq!(public_count, 1, "public channel is not duplicated by join");

    // The server refuses to leave the built-in channel.
    let mut ws = ws_connect(test.addr, TEST_TOKEN).await;
    let _hello = next_json(&mut ws, Duration::from_secs(5))
        .await
        .expect("hello");
    ws.send(Message::Text(
        json!({"v":1,"type":"leave_channel","id":"leave-public","data":{"channel_id":public.id_hex}})
            .to_string(),
    ))
    .await
    .unwrap();
    let frames = read_until(&mut ws, Duration::from_secs(5), |value| {
        value["type"] == "error"
    })
    .await;
    let error = frames.last().expect("leave error");
    assert_eq!(error["id"], "leave-public");
    assert_eq!(error["data"]["code"], "forbidden");

    let still_there = raw_http(
        test.addr,
        "GET",
        "/api/channels",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await
    .json();
    assert!(
        still_there["channels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|channel| channel["channel_id"] == public.id_hex),
        "public channel survives a refused leave attempt"
    );
    test.bridge.shutdown().await;
}

#[tokio::test]
async fn admin_reset_requires_auth_and_explicit_wipe_confirmation() {
    let test = start_bridge().await;
    let public = openmesh_core::public_channel();

    let unauthorized = raw_http(
        test.addr,
        "POST",
        "/api/admin/reset",
        None,
        Some("application/json"),
        br#"{"confirm":"WIPE","new_identity":true}"#,
    )
    .await;
    assert_eq!(unauthorized.status, 401);

    let wrong_confirm = raw_http(
        test.addr,
        "POST",
        "/api/admin/reset",
        Some(TEST_TOKEN),
        Some("application/json"),
        br#"{"confirm":"nope","new_identity":true}"#,
    )
    .await;
    assert_eq!(wrong_confirm.status, 400);
    assert_eq!(wrong_confirm.json()["error"], "confirmation_required");

    // A rejected reset must not touch data or the channel list.
    let channels = raw_http(
        test.addr,
        "GET",
        "/api/channels",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await
    .json();
    assert!(
        channels["channels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|channel| channel["channel_id"] == public.id_hex),
        "rejected reset leaves the public channel intact"
    );
    test.bridge.shutdown().await;
}

#[tokio::test]
async fn admin_reset_data_only_keeps_identity_and_recreates_public_mesh() {
    let test = start_bridge().await;
    let public = openmesh_core::public_channel();
    let before = raw_http(
        test.addr,
        "GET",
        "/api/channels",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await
    .json();
    let seed_before = std::fs::read(test._dir.path().join("identity.key")).unwrap();

    let reset = raw_http(
        test.addr,
        "POST",
        "/api/admin/reset",
        Some(TEST_TOKEN),
        Some("application/json"),
        br#"{"confirm":"WIPE","new_identity":false}"#,
    )
    .await;
    assert_eq!(reset.status, 200, "reset response: {:?}", reset.json());
    let body = reset.json();
    assert_eq!(body["reset"], true);
    assert_eq!(body["identity_changed"], false);
    assert!(body["node_id"].is_string());
    assert_eq!(
        body["counts"]["channels"],
        before["channels"].as_array().unwrap().len() as u64
    );
    assert!(body["counts"]["activity"].is_number());
    assert_eq!(body["public_channel"]["channel_id"], public.id_hex);
    assert_eq!(body["public_channel"]["public"], true);
    assert_eq!(
        std::fs::read(test._dir.path().join("identity.key")).unwrap(),
        seed_before,
        "data-only reset keeps the persisted identity"
    );

    let channels = raw_http(
        test.addr,
        "GET",
        "/api/channels",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await
    .json();
    assert_eq!(channels["channels"].as_array().unwrap().len(), 1);
    assert_eq!(channels["channels"][0]["channel_id"], public.id_hex);
    assert_eq!(channels["channels"][0]["public"], true);
    let messages = raw_http(
        test.addr,
        "GET",
        &format!("/api/messages/channel/{}?limit=10", public.id_hex),
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await
    .json();
    assert_eq!(
        messages["messages"].as_array().unwrap().len(),
        0,
        "messages are cleared"
    );
    let activity = raw_http(
        test.addr,
        "GET",
        "/api/activity?limit=5",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await
    .json();
    let entries = activity["entries"].as_array().unwrap();
    assert_eq!(
        entries.len(),
        1,
        "old activity is cleared; the reset is logged"
    );
    assert_eq!(entries[0]["path"], "/api/admin/reset");
    assert_eq!(entries[0]["ok"], true);
    assert!(
        !activity.to_string().contains("WIPE") && !activity.to_string().contains("psk"),
        "activity entries never contain request bodies/PSKs"
    );
    test.bridge.shutdown().await;
}

#[tokio::test]
async fn admin_reset_with_new_identity_rotates_seed_and_pushes_fresh_hello() {
    let test = start_bridge().await;
    let public = openmesh_core::public_channel();
    let mut ws = ws_connect(test.addr, TEST_TOKEN).await;
    let hello = next_json(&mut ws, Duration::from_secs(5))
        .await
        .expect("hello");
    let old_node_id = hello["data"]["node"]["node_id"]
        .as_str()
        .expect("old node id")
        .to_string();
    let seed_before = std::fs::read(test._dir.path().join("identity.key")).unwrap();

    let reset = raw_http(
        test.addr,
        "POST",
        "/api/admin/reset",
        Some(TEST_TOKEN),
        Some("application/json"),
        br#"{"confirm":"WIPE","new_identity":true}"#,
    )
    .await;
    assert_eq!(reset.status, 200, "reset response: {:?}", reset.json());
    let body = reset.json();
    assert_eq!(body["reset"], true);
    assert_eq!(body["identity_changed"], true);
    assert_eq!(body["old_node_id"], old_node_id);
    let new_node_id = body["node_id"].as_str().expect("new node id").to_string();
    assert_ne!(new_node_id, old_node_id);
    assert!(body["counts"]["messages"].is_number());
    assert_eq!(body["public_channel"]["channel_id"], public.id_hex);
    assert_eq!(body["public_channel"]["public"], true);
    let seed_after = std::fs::read(test._dir.path().join("identity.key")).unwrap();
    assert_ne!(seed_before, seed_after, "new identity seed is persisted");

    // The old token still authenticates and the token/config state is preserved.
    let config = raw_http(test.addr, "GET", "/api/config", Some(TEST_TOKEN), None, b"").await;
    assert_eq!(config.status, 200);

    // The reset pushes a fresh hello with the new node id to existing WS clients.
    let frames = read_until(&mut ws, Duration::from_secs(5), |value| {
        value["type"] == "hello" && value["data"]["node"]["node_id"] == new_node_id
    })
    .await;
    let fresh = frames.last().expect("fresh hello after reset");
    assert_eq!(fresh["data"]["node"]["node_id"], new_node_id);
    let channel_frames = read_until(&mut ws, Duration::from_secs(5), |value| {
        value["type"] == "channels"
    })
    .await;
    assert!(
        !channel_frames.is_empty(),
        "reset pushes a fresh channels event"
    );

    let channels = raw_http(
        test.addr,
        "GET",
        "/api/channels",
        Some(TEST_TOKEN),
        None,
        b"",
    )
    .await
    .json();
    assert_eq!(channels["channels"].as_array().unwrap().len(), 1);
    assert_eq!(channels["channels"][0]["channel_id"], public.id_hex);
    test.bridge.shutdown().await;
}

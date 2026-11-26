//! Integration tests for Backbone mode (`docs/architecture/BACKBONE.md`):
//! OBB/1 handshake/auth, peer accounting, kick, live config restart, token rotation, setup
//! payload, and end-to-end frame relay between two in-process bridges over real TCP.

use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use openmesh_bridge::transport::backbone::wire::{self, ClientHandshakeParams};
use openmesh_bridge::transport::backbone::{ConfigOverrides, JoinTarget};
use openmesh_bridge::{Bridge, BridgeOptions};

const API_TOKEN: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

async fn start_bridge(dir: &Path, configure: impl FnOnce(&mut BridgeOptions)) -> Bridge {
    let mut options = BridgeOptions {
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
        sim: false,
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
    configure(&mut options);
    Bridge::start(options).await.expect("bridge starts")
}

fn server_overrides(token: &str, port: u16, auth: &str, enabled: bool) -> ConfigOverrides {
    ConfigOverrides {
        enabled: Some(enabled),
        bind: Some("127.0.0.1".to_string()),
        port: Some(port),
        auth: Some(auth.to_string()),
        token: (!token.is_empty()).then(|| token.to_string()),
        name: Some("test-backbone".to_string()),
        store_forward: Some(true),
        ..ConfigOverrides::default()
    }
}

async fn start_backbone_server(dir: &Path, token: &str, port: u16) -> Bridge {
    let token = token.to_string();
    start_bridge(dir, move |options| {
        options.backbone_enabled = true;
        options.backbone_overrides = server_overrides(&token, port, "token", true);
    })
    .await
}

struct HttpResult {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpResult {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

async fn http(
    addr: SocketAddr,
    token: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> HttpResult {
    let mut stream = TcpStream::connect(addr)
        .await
        .expect("connect to bridge API");
    let payload = body.map(|value| value.to_string()).unwrap_or_default();
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n"
    );
    if !payload.is_empty() {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", payload.len()));
    }
    request.push_str("\r\n");
    request.push_str(&payload);
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("http response headers");
    let head = String::from_utf8_lossy(&response[..split]).to_string();
    let body = response[split + 4..].to_vec();
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect();
    HttpResult {
        status,
        headers,
        body,
    }
}

async fn get(addr: SocketAddr, path: &str) -> Value {
    http(addr, API_TOKEN, "GET", path, None).await.json()
}

async fn post(addr: SocketAddr, path: &str, body: Value) -> (u16, Value) {
    let result = http(addr, API_TOKEN, "POST", path, Some(body)).await;
    (result.status, result.json())
}

async fn delete(addr: SocketAddr, path: &str) -> (u16, Value) {
    let result = http(addr, API_TOKEN, "DELETE", path, None).await;
    (result.status, result.json())
}

/// Poll a GET endpoint until `predicate` matches (or panic on timeout).
async fn poll<F>(addr: SocketAddr, path: &str, timeout: Duration, predicate: F) -> Value
where
    F: Fn(&Value) -> bool,
{
    let deadline = Instant::now() + timeout;
    loop {
        let value = get(addr, path).await;
        if predicate(&value) {
            return value;
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for {path}; last response: {value}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn backbone_port(addr: SocketAddr) -> u16 {
    let config = get(addr, "/api/backbone").await;
    config["port"].as_u64().expect("backbone port") as u16
}

fn token_key(token: &str) -> [u8; 32] {
    wire::token_key(token).expect("valid token")
}

/// Client-side OBB/1 handshake; returns the connected stream on success.
async fn dial(port: u16, node_id: [u8; 8], token: Option<&str>) -> Result<TcpStream, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|e| format!("connect: {e}"))?;
    let params = ClientHandshakeParams {
        node_id: openmesh_core::NodeId::new(node_id),
        app: "test-app",
        caps: 0,
        token: token.map(token_key),
        timeout: Duration::from_secs(5),
    };
    wire::client_handshake(&mut stream, &params)
        .await
        .map_err(|e| e.to_string())?;
    Ok(stream)
}

async fn dial_error(port: u16, node_id: [u8; 8], token: Option<&str>) -> String {
    match dial(port, node_id, token).await {
        Ok(_) => panic!("handshake unexpectedly succeeded"),
        Err(error) => error,
    }
}

fn sample_frame(seq: u32, payload_len: usize) -> Vec<u8> {
    use openmesh_core::{BROADCAST, Flags, Frame, FrameType, NodeId};
    let mut frame = Frame::new(
        FrameType::Data,
        NodeId::new([0x11; 8]),
        BROADCAST,
        seq,
        vec![0x7A; payload_len],
    )
    .expect("frame");
    frame.ttl = 5;
    frame.flags.insert(Flags::ENCRYPTED);
    frame.encode().expect("encode frame")
}

fn connection_url(port: u16, token: &str) -> String {
    format!("openmesh://backbone?host=127.0.0.1&port={port}&token={token}&name=test-backbone")
}

// --- WebSocket helpers -----------------------------------------------------------------------

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

async fn ws_connect(addr: SocketAddr) -> Ws {
    let (stream, _response) = connect_async(format!("ws://{addr}/api/ws?token={API_TOKEN}"))
        .await
        .expect("ws connects");
    stream
}

async fn ws_next(ws: &mut Ws, timeout: Duration) -> Option<Value> {
    let frame = tokio::time::timeout(timeout, ws.next()).await.ok()??.ok()?;
    serde_json::from_str(frame.to_text().ok()?).ok()
}

async fn ws_send(ws: &mut Ws, value: Value) {
    ws.send(Message::Text(value.to_string())).await.unwrap();
}

async fn ws_wait_for(
    ws: &mut Ws,
    timeout: Duration,
    mut predicate: impl FnMut(&Value) -> bool,
) -> Vec<Value> {
    let mut seen = Vec::new();
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Some(value) = ws_next(ws, remaining).await else {
            break;
        };
        let matched = predicate(&value);
        seen.push(value);
        if matched {
            break;
        }
    }
    seen
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn disabled_by_default_then_enabled_via_rest_and_setup_payload() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path(), |_| {}).await;
    let addr = bridge.addr;
    let node_id = bridge.node_id.clone();

    // Not in backbone mode: the documented 200 {"enabled": false} behaviour.
    assert_eq!(
        get(addr, "/api/backbone").await,
        json!({ "enabled": false })
    );
    assert_eq!(
        get(addr, "/api/backbone/peers").await,
        json!({ "enabled": false, "peers": [] })
    );
    assert_eq!(
        get(addr, "/api/backbone/setup").await,
        json!({ "enabled": false })
    );
    let rotate = http(
        addr,
        API_TOKEN,
        "POST",
        "/api/backbone/token/rotate",
        Some(json!({})),
    )
    .await;
    assert_eq!(rotate.status, 200);
    assert_eq!(rotate.json(), json!({ "enabled": false }));
    let kick = delete(addr, "/api/backbone/peers/0102030405060708").await;
    assert_eq!(kick.0, 200);
    assert_eq!(kick.1, json!({ "enabled": false }));

    // Enable backbone mode live with one POST (panel "enable" click).
    let (status, enabled) = post(
        addr,
        "/api/backbone/config",
        json!({"enabled": true, "bind": "127.0.0.1", "port": 0, "auth": "token", "name": "panel-backbone"}),
    )
    .await;
    assert_eq!(status, 200, "enable response: {enabled}");
    assert_eq!(enabled["enabled"], true);
    assert_eq!(enabled["running"], true);
    assert_eq!(enabled["token_set"], true);
    assert_eq!(enabled["restarted"], true);
    assert_eq!(enabled["name"], "panel-backbone");
    let port = enabled["port"].as_u64().expect("bound port") as u16;
    assert!(port > 0);

    // GET /api/backbone never returns the token, but reports that one is set.
    let config = get(addr, "/api/backbone").await;
    assert_eq!(config["enabled"], true);
    assert_eq!(config["running"], true);
    assert_eq!(config["bind"], "127.0.0.1");
    assert_eq!(config["port"], port);
    assert_eq!(config["auth"], "token");
    assert_eq!(config["token_set"], true);
    assert!(config["totals"]["frames_in"].is_number());
    assert!(config["totals"]["auth_failures"].is_number());
    assert!(
        !config.to_string().contains("token\":\""),
        "token value leaked"
    );

    // Setup payload §8.
    let setup = get(addr, "/api/backbone/setup").await;
    assert_eq!(setup["enabled"], true);
    assert_eq!(setup["listen"], format!("127.0.0.1:{port}"));
    assert_eq!(setup["auth"], "token");
    assert_eq!(setup["token_set"], true);
    let connection = setup["connection_string"]
        .as_str()
        .expect("connection string");
    assert!(
        connection.starts_with("openmesh://backbone?host="),
        "{connection}"
    );
    assert!(connection.contains(&format!("port={port}")));
    assert!(connection.contains("token="));
    assert!(connection.contains(&format!("node={node_id}")));
    let steps = setup["steps"].as_array().expect("steps");
    assert_eq!(steps.len(), 6);
    assert_eq!(steps[0]["title"], "Install OpenMesh");
    let titles: Vec<&str> = steps
        .iter()
        .filter_map(|step| step["title"].as_str())
        .collect();
    assert_eq!(
        titles,
        vec![
            "Install OpenMesh",
            "Start the backbone",
            "Open the firewall",
            "Expose it publicly (optional)",
            "Keep it running",
            "Connect a gateway",
        ]
    );
    assert!(steps.iter().all(|step| step["commands"].is_array()));
    let exposure_step = &steps[3];
    assert!(
        exposure_step["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| command.as_str().unwrap_or("").contains("--bind 127.0.0.1"))
    );
    assert!(
        exposure_step["note"]
            .as_str()
            .unwrap_or("")
            .contains("loopback")
    );

    // BACKBONE.md §8/§9b exposure block (pinned shape; no secrets).
    let exposure = &setup["exposure"];
    assert_eq!(exposure["recommended"], "openflare");
    assert_eq!(exposure["provider"]["name"], "Openflare");
    assert_eq!(
        exposure["provider"]["url"],
        "https://github.com/martintondel02/openflare"
    );
    assert!(
        exposure["provider"]["description"]
            .as_str()
            .unwrap_or("")
            .contains("outbound-only")
    );
    assert!(
        exposure["when_to_use"]
            .as_str()
            .unwrap_or("")
            .contains("NAT")
    );
    assert_eq!(exposure["steps"].as_array().unwrap().len(), 4);
    assert!(
        exposure["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| step.is_string())
    );
    let alternatives: Vec<&str> = exposure["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|value| value.as_str())
        .collect();
    assert!(alternatives.contains(&"WireGuard"));
    assert!(alternatives.contains(&"Tailscale"));
    assert!(alternatives.contains(&"Cloudflare Tunnel"));
    assert_eq!(
        exposure["bind_command"],
        format!("openmesh-bridge backbone --bind 127.0.0.1 --port {port} --public-host <host>")
    );
    assert!(
        exposure["guide_url"]
            .as_str()
            .unwrap_or("")
            .ends_with("docs/guides/backbone-exposure.md")
    );
    // The exposure object contains no secrets (the connection string with the token is a
    // sibling field, not part of exposure).
    let exposure_text = exposure.to_string();
    let connection = setup["connection_string"].as_str().unwrap_or("");
    let connection_token = connection
        .split("token=")
        .nth(1)
        .and_then(|rest| rest.split('&').next())
        .unwrap_or("");
    assert!(
        !connection_token.is_empty(),
        "test expects a token connection string"
    );
    assert!(!exposure_text.contains(connection_token));
    assert!(!exposure_text.contains("token="));
    assert!(!exposure_text.contains("psk"));
    assert!(
        setup["systemd_unit"]
            .as_str()
            .unwrap()
            .contains("ExecStart=")
    );
    assert!(
        setup["systemd_unit"]
            .as_str()
            .unwrap()
            .contains("NoNewPrivileges=true")
    );
    assert!(setup["firewall"]["firewalld"].is_array());
    assert!(setup["firewall"]["ufw"].is_array());
    assert_eq!(setup["panel_url"], format!("http://{addr}/backbone"));
    assert!(!setup["notes"].as_array().unwrap().is_empty());

    // QR endpoint renders the connection string as SVG.
    let qr = http(addr, API_TOKEN, "GET", "/api/backbone/qr.svg", None).await;
    assert_eq!(qr.status, 200);
    assert_eq!(qr.header("content-type"), Some("image/svg+xml"));
    let svg = String::from_utf8_lossy(&qr.body).to_string();
    assert!(svg.contains("<svg"), "QR is SVG");
    assert!(svg.contains("<path"));

    // Live disable keeps the configuration for one-click re-enable.
    let (status, disabled) = post(addr, "/api/backbone/config", json!({"enabled": false})).await;
    assert_eq!(status, 200);
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["running"], false);
    assert_eq!(disabled["name"], "panel-backbone");

    let (status, reenabled) = post(addr, "/api/backbone/config", json!({"enabled": true})).await;
    assert_eq!(status, 200);
    assert_eq!(reenabled["enabled"], true);
    assert_eq!(reenabled["running"], true);

    bridge.shutdown().await;
}

#[tokio::test]
async fn handshake_success_bad_token_and_auth_failure_count() {
    let dir = TempDir::new().unwrap();
    let token = wire::generate_token();
    let bridge = start_backbone_server(dir.path(), &token, 0).await;
    let addr = bridge.addr;
    let port = backbone_port(addr).await;
    let server_node = bridge.node_id.clone();

    // Correct token: welcome, features, server identity.
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let params = ClientHandshakeParams {
        node_id: openmesh_core::NodeId::new([0x21; 8]),
        app: "test-app",
        caps: 0,
        token: Some(token_key(&token)),
        timeout: Duration::from_secs(5),
    };
    let welcome = wire::client_handshake(&mut stream, &params)
        .await
        .expect("handshake succeeds");
    assert_eq!(welcome.node_id.to_hex(), server_node);
    assert!(welcome.features.iter().any(|feature| feature == "frames"));
    assert!(welcome.features.iter().any(|feature| feature == "stats"));
    assert_eq!(welcome.max_frame, 4200);
    assert!(welcome.peers_connected >= 1);
    drop(stream);

    // Wrong token is rejected with the documented reason and counted.
    let error = dial_error(port, [0x22; 8], Some(&wire::generate_token())).await;
    assert!(error.contains("bad_token"), "unexpected error: {error}");

    // Missing token in token mode is also rejected.
    let error = dial_error(port, [0x23; 8], None).await;
    assert!(error.contains("bad_token"), "unexpected error: {error}");

    poll(addr, "/api/backbone", Duration::from_secs(5), |config| {
        config["totals"]["auth_failures"].as_u64().unwrap_or(0) >= 2
    })
    .await;
    bridge.shutdown().await;
}

#[tokio::test]
async fn open_mode_allowlist_and_max_peers() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path(), |options| {
        options.backbone_enabled = true;
        options.backbone_overrides = server_overrides("", 0, "open", true);
    })
    .await;
    let addr = bridge.addr;
    let port = backbone_port(addr).await;
    assert_eq!(get(addr, "/api/backbone").await["auth"], "open");
    assert_eq!(get(addr, "/api/backbone").await["token_set"], false);

    // Open mode accepts auth {mac: null}.
    let open_peer = dial(port, [0x31; 8], None)
        .await
        .expect("open mode accepts without token");
    assert_eq!(
        get(addr, "/api/backbone/peers")
            .await
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // Switch to token auth; the existing session stays connected but new handshakes need a token.
    let (status, config) = post(addr, "/api/backbone/config", json!({"auth": "token"})).await;
    assert_eq!(status, 200, "config: {config}");
    assert_eq!(config["token_set"], true);
    let rotate = http(
        addr,
        API_TOKEN,
        "POST",
        "/api/backbone/token/rotate",
        Some(json!({})),
    )
    .await;
    let new_token = rotate.json()["token"]
        .as_str()
        .expect("rotated token")
        .to_string();
    assert!(
        dial_error(port, [0x32; 8], None)
            .await
            .contains("bad_token")
    );

    // Allowlist: only the listed transport node id may connect.
    let allowlisted = openmesh_core::NodeId::new([0x33; 8]).to_hex();
    let (status, config) = post(
        addr,
        "/api/backbone/config",
        json!({"allowlist": [allowlisted], "max_peers": 32}),
    )
    .await;
    assert_eq!(status, 200, "config: {config}");
    let _allowed = dial(port, [0x33; 8], Some(&new_token))
        .await
        .expect("allowlisted peer");
    let error = dial_error(port, [0x34; 8], Some(&new_token)).await;
    assert!(
        error.contains("not_allowlisted"),
        "unexpected error: {error}"
    );

    // Kick everyone, then enforce max_peers.
    let peers = get(addr, "/api/backbone/peers").await;
    for peer in peers.as_array().unwrap() {
        let node = peer["node_id"].as_str().unwrap();
        let (status, body) = delete(addr, &format!("/api/backbone/peers/{node}")).await;
        assert_eq!(status, 200, "kick: {body}");
    }
    poll(
        addr,
        "/api/backbone/peers",
        Duration::from_secs(5),
        |peers| {
            peers
                .as_array()
                .map(|list| list.is_empty())
                .unwrap_or(false)
        },
    )
    .await;
    let (status, config) = post(
        addr,
        "/api/backbone/config",
        json!({"allowlist": [], "max_peers": 1}),
    )
    .await;
    assert_eq!(status, 200, "config: {config}");
    let _first = dial(port, [0x35; 8], Some(&new_token))
        .await
        .expect("first peer");
    let error = dial_error(port, [0x36; 8], Some(&new_token)).await;
    assert!(error.contains("max_peers"), "unexpected error: {error}");

    drop(open_peer);
    bridge.shutdown().await;
}

#[tokio::test]
async fn peer_accounting_events_and_kick() {
    let dir = TempDir::new().unwrap();
    let token = wire::generate_token();
    let bridge = start_backbone_server(dir.path(), &token, 0).await;
    let addr = bridge.addr;
    let port = backbone_port(addr).await;

    // Subscribe before the peer joins so the pushed events are observable.
    let mut ws = ws_connect(addr).await;
    let hello = ws_next(&mut ws, Duration::from_secs(5))
        .await
        .expect("hello");
    assert_eq!(hello["type"], "hello");

    let peer_node = openmesh_core::NodeId::new([0x41; 8]);
    let mut stream = dial(port, [0x41; 8], Some(&token))
        .await
        .expect("peer connects");
    let peers = poll(
        addr,
        "/api/backbone/peers",
        Duration::from_secs(5),
        |peers| {
            peers
                .as_array()
                .map(|list| list.len() == 1)
                .unwrap_or(false)
        },
    )
    .await;
    let peer = &peers[0];
    assert_eq!(peer["node_id"], peer_node.to_hex());
    assert_eq!(peer["direction"], "inbound");
    assert_eq!(peer["auth"], "token");
    assert_eq!(peer["app_version"], "test-app");
    assert!(
        peer["remote_addr"]
            .as_str()
            .unwrap_or("")
            .starts_with("127.0.0.1:")
    );
    assert_eq!(peer["frames_in"], 0);

    // Three OMP frames -> per-peer and global accounting.
    let mut expected_bytes = 0usize;
    for seq in 1..=3u32 {
        let frame = sample_frame(seq, 32);
        expected_bytes += frame.len();
        wire::write_frame(&mut stream, &frame)
            .await
            .expect("send frame");
    }
    let peers = poll(
        addr,
        "/api/backbone/peers",
        Duration::from_secs(5),
        |peers| {
            peers
                .as_array()
                .map(|list| list.len() == 1)
                .unwrap_or(false)
                && peers[0]["frames_in"].as_u64().unwrap_or(0) == 3
        },
    )
    .await;
    assert_eq!(peers[0]["frames_in"], 3);
    assert_eq!(peers[0]["bytes_in"], expected_bytes);
    let config = get(addr, "/api/backbone").await;
    assert_eq!(config["totals"]["frames_in"], 3);
    assert_eq!(config["totals"]["bytes_in"], expected_bytes);

    // Kick via the documented DELETE endpoint.
    let (status, body) = delete(addr, &format!("/api/backbone/peers/{}", peer_node.to_hex())).await;
    assert_eq!(status, 200, "kick response: {body}");
    assert_eq!(body["disconnected"], true);
    poll(
        addr,
        "/api/backbone/peers",
        Duration::from_secs(5),
        |peers| {
            peers
                .as_array()
                .map(|list| list.is_empty())
                .unwrap_or(false)
        },
    )
    .await;

    // Events: joined, left and periodic status pushes.
    let seen = ws_wait_for(&mut ws, Duration::from_secs(6), |value| {
        value["type"] == "backbone_peer" && value["data"]["action"] == "left"
    })
    .await;
    let joined = seen.iter().any(|value| {
        value["type"] == "backbone_peer"
            && value["data"]["action"] == "joined"
            && value["data"]["peer"]["node_id"] == peer_node.to_hex()
    });
    let left = seen
        .iter()
        .any(|value| value["type"] == "backbone_peer" && value["data"]["action"] == "left");
    assert!(joined, "joined event missing: {seen:?}");
    assert!(left, "left event missing: {seen:?}");
    // The 2 Hz status push may arrive after the `left` event.
    let status_seen = ws_wait_for(&mut ws, Duration::from_secs(5), |value| {
        value["type"] == "backbone_status" && value["data"]["running"] == true
    })
    .await;
    assert!(
        status_seen
            .iter()
            .any(|value| value["type"] == "backbone_status" && value["data"]["running"] == true),
        "2s backbone_status push missing"
    );
    drop(stream);
    bridge.shutdown().await;
}

#[tokio::test]
async fn websocket_backbone_commands() {
    let dir = TempDir::new().unwrap();
    let token = wire::generate_token();
    let bridge = start_backbone_server(dir.path(), &token, 0).await;
    let addr = bridge.addr;
    let port = backbone_port(addr).await;
    let mut ws = ws_connect(addr).await;
    assert_eq!(
        ws_next(&mut ws, Duration::from_secs(5))
            .await
            .expect("hello")["type"],
        "hello"
    );

    // get_backbone -> backbone_status with the redacted config attached.
    ws_send(&mut ws, json!({"v":1,"type":"get_backbone","id":"gb1"})).await;
    let seen = ws_wait_for(&mut ws, Duration::from_secs(5), |value| {
        value["type"] == "backbone_status" && value["id"] == "gb1"
    })
    .await;
    let response = seen.last().expect("get_backbone response");
    assert_eq!(response["data"]["config"]["enabled"], true);
    assert_eq!(response["data"]["config"]["token_set"], true);
    assert!(!response["data"]["config"].to_string().contains(&token));

    // set_backbone_config -> broadcast status/config (panel inline edit).
    ws_send(
        &mut ws,
        json!({"v":1,"type":"set_backbone_config","id":"sb1","data":{"name":"ws-renamed","max_peers":7}}),
    )
    .await;
    let seen = ws_wait_for(&mut ws, Duration::from_secs(5), |value| {
        value["type"] == "backbone_status" && value["id"] == "sb1"
    })
    .await;
    let response = seen.last().expect("set_backbone_config response");
    assert_eq!(response["data"]["config"]["name"], "ws-renamed");
    assert_eq!(response["data"]["config"]["max_peers"], 7);
    assert_eq!(get(addr, "/api/backbone").await["name"], "ws-renamed");

    // backbone_rotate_token -> one-time token response; old token invalid immediately.
    ws_send(
        &mut ws,
        json!({"v":1,"type":"backbone_rotate_token","id":"rt1"}),
    )
    .await;
    let seen = ws_wait_for(&mut ws, Duration::from_secs(5), |value| {
        value["type"] == "backbone_token" && value["id"] == "rt1"
    })
    .await;
    let rotated = seen.last().expect("rotate response")["data"]["token"]
        .as_str()
        .expect("new token")
        .to_string();
    assert!(
        dial_error(port, [0x51; 8], Some(&token))
            .await
            .contains("bad_token")
    );
    let _new_peer = dial(port, [0x52; 8], Some(&rotated))
        .await
        .expect("new token works");

    // backbone_kick -> peer disconnected.
    let node = openmesh_core::NodeId::new([0x52; 8]).to_hex();
    poll(
        addr,
        "/api/backbone/peers",
        Duration::from_secs(5),
        |peers| {
            peers
                .as_array()
                .map(|list| !list.is_empty())
                .unwrap_or(false)
        },
    )
    .await;
    ws_send(
        &mut ws,
        json!({"v":1,"type":"backbone_kick","id":"k1","data":{"node_id": node}}),
    )
    .await;
    let seen = ws_wait_for(&mut ws, Duration::from_secs(5), |value| {
        value["type"] == "backbone_status" && value["id"] == "k1"
    })
    .await;
    let response = seen.last().expect("kick response");
    assert_eq!(response["data"]["disconnected"], true);
    poll(
        addr,
        "/api/backbone/peers",
        Duration::from_secs(5),
        |peers| {
            peers
                .as_array()
                .map(|list| list.is_empty())
                .unwrap_or(false)
        },
    )
    .await;

    bridge.shutdown().await;
}

#[tokio::test]
async fn token_rotation_invalidates_old_token() {
    let dir = TempDir::new().unwrap();
    let old_token = wire::generate_token();
    let bridge = start_backbone_server(dir.path(), &old_token, 0).await;
    let addr = bridge.addr;
    let port = backbone_port(addr).await;
    assert!(dial(port, [0x61; 8], Some(&old_token)).await.is_ok());

    let result = http(
        addr,
        API_TOKEN,
        "POST",
        "/api/backbone/token/rotate",
        Some(json!({})),
    )
    .await;
    assert_eq!(result.status, 200);
    let new_token = result.json()["token"]
        .as_str()
        .expect("new token")
        .to_string();
    assert_ne!(new_token, old_token);
    assert!(
        dial_error(port, [0x62; 8], Some(&old_token))
            .await
            .contains("bad_token")
    );
    assert!(dial(port, [0x63; 8], Some(&new_token)).await.is_ok());

    // Neither the API config nor the setup payload leaks the bridge API token; the backbone
    // token only appears where documented (setup connection string, one-time rotate response).
    let config = get(addr, "/api/backbone").await;
    assert!(!config.to_string().contains(&new_token));
    let config_file = std::fs::read_to_string(dir.path().join("backbone.json")).unwrap();
    assert!(
        config_file.contains(&new_token),
        "rotated token must be persisted"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join("backbone.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "backbone.json must be 0600");
    }
    bridge.shutdown().await;
}

#[tokio::test]
async fn config_persistence_and_live_listener_restart() {
    let dir = TempDir::new().unwrap();
    let token = wire::generate_token();
    let bridge = start_backbone_server(dir.path(), &token, 0).await;
    let addr = bridge.addr;
    let first_port = backbone_port(addr).await;
    assert!(dial(first_port, [0x71; 8], Some(&token)).await.is_ok());

    // Choose a new free port and move the listener live. Port probing races with other test
    // binaries and the OS, so retry with a fresh port instead of failing the whole suite.
    let mut second_port = 0u16;
    let mut moved = false;
    for _ in 0..5 {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let candidate = probe.local_addr().unwrap().port();
        drop(probe);
        let (status, config) = post(
            addr,
            "/api/backbone/config",
            json!({"bind": "127.0.0.1", "port": candidate}),
        )
        .await;
        assert_eq!(status, 200, "restart response: {config}");
        assert_eq!(config["restarted"], true);
        assert_eq!(config["port"], candidate);
        // Under parallel test load the listener may need a moment to accept; if the port was
        // stolen between probe and bind, try another one.
        let mut ok = false;
        for _ in 0..30 {
            if dial(candidate, [0x72; 8], Some(&token)).await.is_ok() {
                ok = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if ok {
            second_port = candidate;
            moved = true;
            break;
        }
    }
    assert!(moved, "listener did not restart on any probed port");
    bridge.shutdown().await;

    // Restart the process from persisted state only: config (incl. secret) survives.
    let bridge2 = start_bridge(dir.path(), |_| {}).await;
    let addr2 = bridge2.addr;
    let config = poll(addr2, "/api/backbone", Duration::from_secs(15), |config| {
        config["running"].as_bool() == Some(true)
    })
    .await;
    assert_eq!(config["enabled"], true);
    assert_eq!(config["bind"], "127.0.0.1");
    assert_eq!(config["port"], second_port);
    assert_eq!(config["token_set"], true);
    assert!(dial(second_port, [0x73; 8], Some(&token)).await.is_ok());
    bridge2.shutdown().await;
}

#[tokio::test]
async fn two_bridges_relay_frames_end_to_end_over_real_tcp() {
    let dir_a = TempDir::new().unwrap();
    let dir_b = TempDir::new().unwrap();
    let token = wire::generate_token();

    let server = start_backbone_server(dir_a.path(), &token, 0).await;
    let server_addr = server.addr;
    let server_node = server.node_id.clone();
    let server_port = backbone_port(server_addr).await;

    let join = JoinTarget::from_url(&connection_url(server_port, &token))
        .expect("connection string")
        .with_backoff(100, 1_000);
    let client = start_bridge(dir_b.path(), move |options| {
        options.backbone_join = Some(join);
    })
    .await;
    let client_addr = client.addr;
    let client_node = client.node_id.clone();

    // Both sides see the TCP link.
    poll(
        server_addr,
        "/api/backbone/peers",
        Duration::from_secs(10),
        |peers| {
            peers
                .as_array()
                .map(|list| {
                    list.iter().any(|peer| {
                        peer["node_id"] == client_node && peer["direction"] == "inbound"
                    })
                })
                .unwrap_or(false)
        },
    )
    .await;
    poll(
        client_addr,
        "/api/backbone/peers",
        Duration::from_secs(10),
        |peers| {
            peers
                .as_array()
                .map(|list| {
                    list.iter().any(|peer| {
                        peer["node_id"] == server_node && peer["direction"] == "outbound"
                    })
                })
                .unwrap_or(false)
        },
    )
    .await;

    // Announces cross the backbone so each engine learns the other identity.
    poll(
        server_addr,
        "/api/nodes",
        Duration::from_secs(10),
        |nodes| {
            nodes
                .as_array()
                .map(|list| list.iter().any(|node| node["node_id"] == client_node))
                .unwrap_or(false)
        },
    )
    .await;
    poll(
        client_addr,
        "/api/nodes",
        Duration::from_secs(10),
        |nodes| {
            nodes
                .as_array()
                .map(|list| list.iter().any(|node| node["node_id"] == server_node))
                .unwrap_or(false)
        },
    )
    .await;

    // Send a direct message from the joining bridge to the backbone bridge; the OMP frame is
    // relayed over real TCP, decrypted, acked, and the ack travels back the same way.
    let mut client_ws = ws_connect(client_addr).await;
    assert_eq!(
        ws_next(&mut client_ws, Duration::from_secs(5))
            .await
            .expect("hello")["type"],
        "hello"
    );
    let mut server_ws = ws_connect(server_addr).await;
    assert_eq!(
        ws_next(&mut server_ws, Duration::from_secs(5))
            .await
            .expect("hello")["type"],
        "hello"
    );
    ws_send(
        &mut client_ws,
        json!({"v":1,"type":"send_message","id":"backbone-1",
               "data":{"to": server_node, "text": "hello over the backbone"}}),
    )
    .await;
    let seen = ws_wait_for(&mut client_ws, Duration::from_secs(15), |value| {
        value["type"] == "ack"
            && value["data"]["kind"] == "delivered"
            && value["data"]["peer"] == server_node
    })
    .await;
    assert!(
        seen.iter().any(|value| value["type"] == "send_result"),
        "send_result missing: {seen:?}"
    );
    assert!(
        seen.iter()
            .any(|value| value["type"] == "message" && value["data"]["outgoing"] == true),
        "outgoing message echo missing: {seen:?}"
    );
    assert!(
        seen.iter()
            .any(|value| value["type"] == "ack" && value["data"]["kind"] == "delivered"),
        "delivery ack over backbone missing: {seen:?}"
    );

    let seen = ws_wait_for(&mut server_ws, Duration::from_secs(15), |value| {
        value["type"] == "message"
            && value["data"]["text"] == "hello over the backbone"
            && value["data"]["outgoing"] == false
    })
    .await;
    let received = seen
        .last()
        .unwrap_or_else(|| panic!("backbone did not deliver the message: {seen:?}"));
    assert_eq!(received["data"]["from"], client_node);
    assert_eq!(received["data"]["peer"], client_node);

    // The server panel shows the relayed traffic.
    let config = get(server_addr, "/api/backbone").await;
    assert!(config["totals"]["frames_in"].as_u64().unwrap_or(0) > 0);
    assert!(config["totals"]["frames_out"].as_u64().unwrap_or(0) > 0);

    client.shutdown().await;
    server.shutdown().await;
}

#[tokio::test]
async fn joining_client_reconnects_after_server_restart() {
    let dir_a = TempDir::new().unwrap();
    let dir_b = TempDir::new().unwrap();
    let token = wire::generate_token();
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let server = start_backbone_server(dir_a.path(), &token, port).await;
    let server_addr = server.addr;
    let server_node = server.node_id.clone();
    let join = JoinTarget::from_url(&connection_url(port, &token))
        .expect("connection string")
        .with_backoff(100, 800);
    let client = start_bridge(dir_b.path(), move |options| {
        options.backbone_join = Some(join);
    })
    .await;
    let client_addr = client.addr;
    let client_node = client.node_id.clone();

    poll(
        server_addr,
        "/api/backbone/peers",
        Duration::from_secs(10),
        |peers| {
            peers
                .as_array()
                .map(|list| list.len() == 1)
                .unwrap_or(false)
        },
    )
    .await;
    poll(
        client_addr,
        "/api/backbone/peers",
        Duration::from_secs(10),
        |peers| {
            peers
                .as_array()
                .map(|list| list.iter().any(|peer| peer["node_id"] == server_node))
                .unwrap_or(false)
        },
    )
    .await;

    // Restart the backbone server on the same port/data dir; the client reconnects with backoff.
    server.shutdown().await;
    let server2 = start_backbone_server(dir_a.path(), &token, port).await;
    let server2_addr = server2.addr;
    poll(
        server2_addr,
        "/api/backbone/peers",
        Duration::from_secs(20),
        |peers| {
            peers
                .as_array()
                .map(|list| {
                    list.iter().any(|peer| {
                        peer["node_id"] == client_node && peer["direction"] == "inbound"
                    })
                })
                .unwrap_or(false)
        },
    )
    .await;
    poll(
        client_addr,
        "/api/backbone/peers",
        Duration::from_secs(20),
        |peers| {
            peers
                .as_array()
                .map(|list| {
                    list.iter().any(|peer| {
                        peer["node_id"] == server_node && peer["direction"] == "outbound"
                    })
                })
                .unwrap_or(false)
        },
    )
    .await;

    // Prove the reconnected link still carries frames both ways. Wait for the server engine to
    // have ingested the client's fresh announce (engine known_nodes, not the persisted node cache).
    poll(
        server2_addr,
        "/api/status",
        Duration::from_secs(10),
        |status| {
            status
                .pointer("/mesh/known_nodes")
                .and_then(|value| value.as_u64())
                .unwrap_or(0)
                >= 1
        },
    )
    .await;
    let mut client_ws = ws_connect(client_addr).await;
    assert_eq!(
        ws_next(&mut client_ws, Duration::from_secs(5))
            .await
            .expect("hello")["type"],
        "hello"
    );
    ws_send(
        &mut client_ws,
        json!({"v":1,"type":"send_message","id":"reconnect-1",
               "data":{"to": server_node, "text": "still here after restart"}}),
    )
    .await;
    let seen = ws_wait_for(&mut client_ws, Duration::from_secs(15), |value| {
        value["type"] == "ack" && value["data"]["kind"] == "delivered"
    })
    .await;
    assert!(
        seen.iter()
            .any(|value| value["type"] == "ack" && value["data"]["kind"] == "delivered"),
        "no delivery ack after reconnect: {seen:?}"
    );

    client.shutdown().await;
    server2.shutdown().await;
}

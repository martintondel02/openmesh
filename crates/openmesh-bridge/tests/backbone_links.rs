//! Integration tests for multiple outbound backbone links (`remote-operations.md` §3):
//! two links connected at once, one link down while the other stays up, restart restoration,
//! legacy `join` import as `default`, API CRUD and token redaction. Backbone servers are real
//! in-process bridges on ephemeral loopback ports — no external network.

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

use openmesh_bridge::transport::backbone::{ConfigOverrides, JoinTarget, wire};
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
        sim: false,
        demo: false,
        udp_port: None,
        ..Default::default()
    };
    configure(&mut options);
    Bridge::start(options).await.expect("bridge starts")
}

fn server_overrides(token: &str, port: u16) -> ConfigOverrides {
    ConfigOverrides {
        enabled: Some(true),
        bind: Some("127.0.0.1".to_string()),
        port: Some(port),
        auth: Some("token".to_string()),
        token: Some(token.to_string()),
        name: Some("test-backbone".to_string()),
        store_forward: Some(true),
        ..ConfigOverrides::default()
    }
}

/// Start a backbone server on an ephemeral port; returns the bridge and its OBB token.
async fn start_backbone_server(dir: &Path) -> (Bridge, String) {
    let token = wire::generate_token();
    let token_for_options = token.clone();
    let bridge = start_bridge(dir, move |options| {
        options.backbone_enabled = true;
        options.backbone_overrides = server_overrides(&token_for_options, 0);
    })
    .await;
    (bridge, token)
}

fn connection_url(port: u16, token: &str, name: &str) -> String {
    format!("openmesh://backbone?host=127.0.0.1&port={port}&token={token}&name={name}&v=1")
}

struct HttpResponse {
    status: u16,
    body: Value,
}

impl HttpResponse {
    fn text(&self) -> String {
        self.body.to_string()
    }
}

async fn http(addr: SocketAddr, method: &str, path: &str, body: Option<Value>) -> HttpResponse {
    let mut stream = TcpStream::connect(addr).await.expect("connect bridge");
    let body = body.map(|value| serde_json::to_vec(&value).expect("json body"));
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
         Authorization: Bearer {API_TOKEN}\r\n"
    );
    if let Some(body) = &body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.expect("write head");
    if let Some(body) = body {
        stream.write_all(&body).await.expect("write body");
    }
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("read response");
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("response headers");
    let head = String::from_utf8_lossy(&response[..split]).to_string();
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    HttpResponse {
        status,
        body: serde_json::from_slice(&response[split + 4..]).unwrap_or(Value::Null),
    }
}

async fn get(addr: SocketAddr, path: &str) -> Value {
    http(addr, "GET", path, None).await.body
}

async fn put(addr: SocketAddr, path: &str, body: Value) -> (u16, Value) {
    let response = http(addr, "PUT", path, Some(body)).await;
    (response.status, response.body)
}

async fn delete(addr: SocketAddr, path: &str) -> (u16, Value) {
    let response = http(addr, "DELETE", path, None).await;
    (response.status, response.body)
}

async fn backbone_port(addr: SocketAddr) -> u16 {
    get(addr, "/api/backbone").await["port"]
        .as_u64()
        .expect("backbone port") as u16
}

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
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {path}; last response: {value}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn link_connected(value: &Value, name: &str) -> bool {
    value
        .as_array()
        .map(|links| {
            links
                .iter()
                .any(|link| link["name"] == name && link["connected"] == true)
        })
        .unwrap_or(false)
}

fn write_links(dir: &Path, links: Value) {
    std::fs::write(
        dir.join("backbone-links.json"),
        serde_json::to_string_pretty(&json!({ "version": 1, "links": links })).unwrap(),
    )
    .expect("write backbone-links.json");
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
    tokio::time::timeout(timeout, async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(value) = serde_json::from_str::<Value>(&text) {
                        return Some(value);
                    }
                }
                Some(Ok(_)) => continue,
                _ => return None,
            }
        }
    })
    .await
    .ok()
    .flatten()
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn two_links_connect_and_tokens_are_redacted() {
    let server_dir_a = TempDir::new().unwrap();
    let server_dir_b = TempDir::new().unwrap();
    let (server_a, token_a) = start_backbone_server(server_dir_a.path()).await;
    let (server_b, token_b) = start_backbone_server(server_dir_b.path()).await;
    let port_a = backbone_port(server_a.addr).await;
    let port_b = backbone_port(server_b.addr).await;

    let client_dir = TempDir::new().unwrap();
    write_links(
        client_dir.path(),
        json!([
            { "name": "oslo", "url": connection_url(port_a, &token_a, "oslo"), "enabled": true },
            { "name": "cabin", "url": connection_url(port_b, &token_b, "cabin"), "enabled": true }
        ]),
    );
    let client = start_bridge(client_dir.path(), |_| {}).await;
    let client_addr = client.addr;
    let client_node = client.node_id.clone();

    // Both backbones accept the bridge as an inbound peer.
    for server in [&server_a, &server_b] {
        poll(
            server.addr,
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
    }

    let links = poll(
        client_addr,
        "/api/backbone/links",
        Duration::from_secs(10),
        |links| link_connected(links, "oslo") && link_connected(links, "cabin"),
    )
    .await;
    assert_eq!(links.as_array().unwrap().len(), 2);
    let rendered = links.to_string();
    assert!(
        !rendered.contains(&token_a),
        "link token leaked: {rendered}"
    );
    assert!(
        !rendered.contains(&token_b),
        "link token leaked: {rendered}"
    );
    for link in links.as_array().unwrap() {
        assert_eq!(link["mode"], "client");
        assert_eq!(link["enabled"], true);
        assert!(
            link["url"].as_str().unwrap().contains("token=redacted"),
            "URL must be redacted: {link}"
        );
        assert!(link["stats"]["frames_in"].as_u64().is_some());
    }

    // `GET /api/backbone` gains a links summary and reports client mode.
    let info = get(client_addr, "/api/backbone").await;
    assert_eq!(info["mode"], "client");
    assert_eq!(info["links"].as_array().unwrap().len(), 2);
    assert!(!info.to_string().contains(&token_a));

    // WebSocket `backbone_status` includes the link list.
    let mut ws = ws_connect(client_addr).await;
    ws.send(Message::Text(
        json!({ "v": 1, "type": "get_backbone" }).to_string(),
    ))
    .await
    .expect("ws send");
    let mut status = None;
    for _ in 0..32 {
        let Some(event) = ws_next(&mut ws, Duration::from_secs(5)).await else {
            break;
        };
        if event["type"] == "backbone_status" {
            status = Some(event);
            break;
        }
    }
    let status = status.expect("backbone_status event");
    assert_eq!(status["data"]["links"].as_array().unwrap().len(), 2);

    client.shutdown().await;
    server_a.shutdown().await;
    server_b.shutdown().await;
}

#[tokio::test]
async fn one_link_down_keeps_the_other_connected() {
    let server_dir_a = TempDir::new().unwrap();
    let server_dir_b = TempDir::new().unwrap();
    let (server_a, token_a) = start_backbone_server(server_dir_a.path()).await;
    let (server_b, token_b) = start_backbone_server(server_dir_b.path()).await;
    let port_a = backbone_port(server_a.addr).await;
    let port_b = backbone_port(server_b.addr).await;

    let client_dir = TempDir::new().unwrap();
    write_links(
        client_dir.path(),
        json!([
            { "name": "oslo", "url": connection_url(port_a, &token_a, "oslo"), "enabled": true },
            { "name": "cabin", "url": connection_url(port_b, &token_b, "cabin"), "enabled": true }
        ]),
    );
    let client = start_bridge(client_dir.path(), |_| {}).await;
    let client_addr = client.addr;
    poll(
        client_addr,
        "/api/backbone/links",
        Duration::from_secs(10),
        |links| link_connected(links, "oslo") && link_connected(links, "cabin"),
    )
    .await;

    // Take one backbone offline: only that link reports the failure.
    server_a.shutdown().await;
    let links = poll(
        client_addr,
        "/api/backbone/links",
        Duration::from_secs(10),
        |links| {
            links
                .as_array()
                .map(|links| {
                    links.iter().any(|link| {
                        link["name"] == "oslo"
                            && link["connected"] == false
                            && link["last_error"].as_str().is_some()
                    })
                })
                .unwrap_or(false)
        },
    )
    .await;
    let cabin = links
        .as_array()
        .unwrap()
        .iter()
        .find(|link| link["name"] == "cabin")
        .expect("cabin link");
    assert_eq!(cabin["connected"], true);
    // The surviving link stays up while the other one is retrying.
    for _ in 0..4 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let links = get(client_addr, "/api/backbone/links").await;
        let cabin_connected = links
            .as_array()
            .unwrap()
            .iter()
            .find(|link| link["name"] == "cabin")
            .is_some_and(|link| link["connected"] == true);
        assert!(
            cabin_connected,
            "cabin link dropped when oslo went down: {links}"
        );
    }

    client.shutdown().await;
    server_b.shutdown().await;
}

#[tokio::test]
async fn restart_restores_enabled_links() {
    let server_dir_a = TempDir::new().unwrap();
    let server_dir_b = TempDir::new().unwrap();
    let (server_a, token_a) = start_backbone_server(server_dir_a.path()).await;
    let (server_b, token_b) = start_backbone_server(server_dir_b.path()).await;
    let port_a = backbone_port(server_a.addr).await;
    let port_b = backbone_port(server_b.addr).await;

    let client_dir = TempDir::new().unwrap();
    write_links(
        client_dir.path(),
        json!([
            { "name": "oslo", "url": connection_url(port_a, &token_a, "oslo"), "enabled": true },
            { "name": "cabin", "url": connection_url(port_b, &token_b, "cabin"), "enabled": true },
            { "name": "parked", "url": connection_url(port_b, &token_b, "parked"), "enabled": false }
        ]),
    );
    let first = start_bridge(client_dir.path(), |_| {}).await;
    poll(
        first.addr,
        "/api/backbone/links",
        Duration::from_secs(10),
        |links| link_connected(links, "oslo") && link_connected(links, "cabin"),
    )
    .await;
    first.shutdown().await;

    // Same data dir, no CLI join: the saved links are restored on restart.
    let second = start_bridge(client_dir.path(), |_| {}).await;
    let links = poll(
        second.addr,
        "/api/backbone/links",
        Duration::from_secs(10),
        |links| link_connected(links, "oslo") && link_connected(links, "cabin"),
    )
    .await;
    let parked = links
        .as_array()
        .unwrap()
        .iter()
        .find(|link| link["name"] == "parked")
        .expect("parked link kept in config");
    assert_eq!(parked["enabled"], false);
    assert_eq!(parked["connected"], false);
    assert_eq!(parked["attempts"], 0, "disabled links are not dialed");

    second.shutdown().await;
    server_a.shutdown().await;
    server_b.shutdown().await;
}

#[tokio::test]
async fn legacy_join_imports_and_restores_the_default_link() {
    let server_dir = TempDir::new().unwrap();
    let (server, token) = start_backbone_server(server_dir.path()).await;
    let port = backbone_port(server.addr).await;
    let url = connection_url(port, &token, "default");

    let client_dir = TempDir::new().unwrap();
    let join = JoinTarget::from_url(&url)
        .expect("join target")
        .with_backoff(100, 1_000);
    let first = start_bridge(client_dir.path(), move |options| {
        options.backbone_join = Some(join);
    })
    .await;
    let links = poll(
        first.addr,
        "/api/backbone/links",
        Duration::from_secs(10),
        |links| link_connected(links, "default"),
    )
    .await;
    assert_eq!(links.as_array().unwrap().len(), 1);
    let stored = std::fs::read_to_string(client_dir.path().join("backbone-links.json"))
        .expect("legacy join persisted backbone-links.json");
    assert!(stored.contains("\"default\""));
    assert!(stored.contains("token="));
    first.shutdown().await;

    // A restart without `join` still dials the imported `default` link.
    let second = start_bridge(client_dir.path(), |_| {}).await;
    poll(
        second.addr,
        "/api/backbone/links",
        Duration::from_secs(10),
        |links| link_connected(links, "default"),
    )
    .await;
    second.shutdown().await;
    server.shutdown().await;
}

#[tokio::test]
async fn api_crud_adds_lists_and_removes_links() {
    let server_dir = TempDir::new().unwrap();
    let (server, token) = start_backbone_server(server_dir.path()).await;
    let port = backbone_port(server.addr).await;
    let url = connection_url(port, &token, "oslo");

    let client_dir = TempDir::new().unwrap();
    let client = start_bridge(client_dir.path(), |_| {}).await;
    let client_addr = client.addr;

    let (status, body) = put(
        client_addr,
        "/api/backbone/links/oslo",
        json!({ "url": url, "enabled": true }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["name"], "oslo");
    assert_eq!(body["mode"], "client");
    assert!(
        !body.to_string().contains(&token),
        "PUT response leaked the token"
    );
    poll(
        client_addr,
        "/api/backbone/links",
        Duration::from_secs(10),
        |links| link_connected(links, "oslo"),
    )
    .await;

    let listed = get(client_addr, "/api/backbone/links").await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert!(!listed.to_string().contains(&token));

    // An invalid connection string is rejected (no loop starts).
    let (status, body) = put(
        client_addr,
        "/api/backbone/links/bad",
        json!({ "url": "not-a-connection-string" }),
    )
    .await;
    assert_eq!(status, 400, "{body}");

    // Activity-log entries are recorded for link management.
    let activity = get(client_addr, "/api/activity?limit=20").await.to_string();
    assert!(activity.contains("upsert backbone link oslo"));

    // A second link (disabled) can be stored; remove both by name.
    let (status, _) = put(
        client_addr,
        "/api/backbone/links/cabin",
        json!({ "url": connection_url(port, &token, "cabin"), "enabled": false }),
    )
    .await;
    assert_eq!(status, 200);
    let listed = get(client_addr, "/api/backbone/links").await;
    assert_eq!(listed.as_array().unwrap().len(), 2);

    let (status, body) = delete(client_addr, "/api/backbone/links/oslo").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["deleted"], true);
    // Removing the link also tears down its OBB/1 session on the server side.
    poll(
        server.addr,
        "/api/backbone/peers",
        Duration::from_secs(10),
        |peers| {
            peers
                .as_array()
                .map(|list| list.is_empty())
                .unwrap_or(false)
        },
    )
    .await;
    let listed = get(client_addr, "/api/backbone/links").await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed.as_array().unwrap()[0]["name"], "cabin");

    let (status, _) = delete(client_addr, "/api/backbone/links/unknown").await;
    assert_eq!(status, 404);

    client.shutdown().await;
    server.shutdown().await;
}

#[tokio::test]
async fn legacy_join_url_config_manages_the_default_link() {
    let server_dir = TempDir::new().unwrap();
    let (server, token) = start_backbone_server(server_dir.path()).await;
    let port = backbone_port(server.addr).await;
    let url = connection_url(port, &token, "default");

    let client_dir = TempDir::new().unwrap();
    let client = start_bridge(client_dir.path(), |_| {}).await;
    let response = http(
        client.addr,
        "POST",
        "/api/backbone/config",
        Some(json!({ "join_url": url })),
    )
    .await;
    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.body["restarted"], true);
    poll(
        client.addr,
        "/api/backbone/links",
        Duration::from_secs(10),
        |links| link_connected(links, "default"),
    )
    .await;

    client.shutdown().await;
    server.shutdown().await;
}

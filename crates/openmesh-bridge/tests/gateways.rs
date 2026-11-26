//! End-to-end tests for authenticated webhook gateways (`remote-operations.md` §2):
//! signed outbound delivery, retries, activity log, inbound signature/limits/rate limiting and
//! secret redaction. Everything runs against a local mock webhook server — no external network.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use openmesh_bridge::gateway::{GatewaysFile, sign_body};
use openmesh_bridge::transport::sim::{MeshConfig, MeshHub};
use openmesh_bridge::{Bridge, BridgeOptions};

const API_TOKEN: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

// ---------------------------------------------------------------------------------------------
// Mock webhook server
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct CapturedRequest {
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl CapturedRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

struct MockWebhook {
    url: String,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for MockWebhook {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Start a mock webhook that answers with the next status in `statuses` (last one repeats).
async fn start_webhook(statuses: Vec<u16>) -> MockWebhook {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind mock");
    let addr = listener.local_addr().expect("mock addr");
    let requests: Arc<Mutex<Vec<CapturedRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let handle = tokio::spawn(async move {
        let mut index = 0usize;
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let status = statuses
                .get(index)
                .or_else(|| statuses.last())
                .copied()
                .unwrap_or(200);
            index += 1;
            let captured = captured.clone();
            tokio::spawn(async move {
                if let Some(request) = read_http_request(&mut stream).await {
                    captured.lock().await.push(request);
                }
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    MockWebhook {
        url: format!("http://{addr}/hook"),
        requests,
        handle,
    }
}

async fn read_http_request(stream: &mut TcpStream) -> Option<CapturedRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(split) = find_header_end(&buffer) {
            let head = String::from_utf8_lossy(&buffer[..split]).to_string();
            let content_length = head
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if buffer.len() >= split + 4 + content_length {
                let headers = head
                    .lines()
                    .skip(1)
                    .filter_map(|line| line.split_once(':'))
                    .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
                    .collect();
                return Some(CapturedRequest {
                    headers,
                    body: buffer[split + 4..split + 4 + content_length].to_vec(),
                });
            }
        }
    }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

// ---------------------------------------------------------------------------------------------
// Bridge + HTTP helpers
// ---------------------------------------------------------------------------------------------

async fn start_bridge(dir: &Path) -> Bridge {
    Bridge::start(BridgeOptions {
        listen: SocketAddr::from(([127, 0, 0, 1], 0)),
        token: Some(API_TOKEN.to_string()),
        token_file: dir.join("bridge-token"),
        db_path: dir.join("openmesh.redb"),
        data_dir: dir.to_path_buf(),
        identity_seed_path: dir.join("identity.key"),
        sim: true,
        mesh_hub: Some(MeshHub::isolated(MeshConfig::default())),
        ..Default::default()
    })
    .await
    .expect("bridge starts")
}

struct HttpResponse {
    status: u16,
    body: Value,
}

async fn request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    extra_headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> HttpResponse {
    let mut stream = TcpStream::connect(addr).await.expect("connect bridge");
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
    if let Some(token) = token {
        head.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    for (name, value) in extra_headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = body {
        if !extra_headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        {
            head.push_str("Content-Type: application/json\r\n");
        }
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.expect("write head");
    if let Some(body) = body {
        stream.write_all(body).await.expect("write body");
    }
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("read response");
    let split = find_header_end(&response).expect("response headers");
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

async fn api_get(bridge: &Bridge, path: &str) -> Value {
    request(bridge.addr, "GET", path, Some(API_TOKEN), &[], None)
        .await
        .body
}

async fn api_send(bridge: &Bridge, method: &str, path: &str, body: Value) -> (u16, Value) {
    let body = serde_json::to_vec(&body).expect("json body");
    let response = request(
        bridge.addr,
        method,
        path,
        Some(API_TOKEN),
        &[("X-OpenMesh-Client", "cli")],
        Some(&body),
    )
    .await;
    (response.status, response.body)
}

async fn create_channel(bridge: &Bridge) -> String {
    let (status, body) = api_send(
        bridge,
        "POST",
        "/api/channels",
        json!({ "name": "ops-gateway" }),
    )
    .await;
    assert_eq!(status, 200, "create channel: {body}");
    body["channel_id"].as_str().expect("channel id").to_string()
}

async fn send_channel_text(bridge: &Bridge, channel: &str, text: &str) {
    let (status, body) = api_send(
        bridge,
        "POST",
        &format!("/api/channels/{channel}/messages"),
        json!({ "text": text }),
    )
    .await;
    assert_eq!(status, 200, "send channel message: {body}");
}

async fn wait_gateway_status(bridge: &Bridge, id: &str, deliveries: u64, failures: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let list = api_get(bridge, "/api/gateways").await;
        let connector = list
            .get("connectors")
            .and_then(|v| v.as_array())
            .and_then(|connectors| {
                connectors
                    .iter()
                    .find(|connector| connector.get("id").and_then(|v| v.as_str()) == Some(id))
            });
        if let Some(connector) = connector {
            let status = connector.get("status").cloned().unwrap_or(Value::Null);
            if status.get("deliveries").and_then(|v| v.as_u64()) == Some(deliveries)
                && status.get("failures").and_then(|v| v.as_u64()) == Some(failures)
            {
                return connector.clone();
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for gateway status; last: {list}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn set_env(name: &str, value: &str) {
    // Safety: each test uses a unique variable name and the bridge only reads it.
    unsafe { std::env::set_var(name, value) };
}

fn gateway_config(connector: Value) -> GatewaysFile {
    serde_json::from_value(json!({ "version": 1, "connectors": [connector] }))
        .expect("valid gateway config")
}

// ---------------------------------------------------------------------------------------------
// Outbound
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn outbound_webhook_is_signed_with_headers_and_body() {
    const ENV: &str = "OPENMESH_TEST_GW_INT_OUT_A";
    set_env(ENV, "integration-secret");
    let dir = TempDir::new().expect("temp dir");
    let bridge = start_bridge(dir.path()).await;
    let channel = create_channel(&bridge).await;
    let webhook = start_webhook(vec![200]).await;
    let connector = json!({
        "id": "hook",
        "kind": "webhook",
        "enabled": true,
        "outbound": {
            "url": webhook.url,
            "secret_env": ENV,
            "channel_ids": [],
            "include_attachments": false,
            "timeout_ms": 5000,
            "max_retries": 2
        }
    });
    let (status, body) = api_send(&bridge, "PUT", "/api/gateways/hook", connector).await;
    assert_eq!(status, 200, "upsert gateway: {body}");
    assert_eq!(body["outbound"]["secret_set"], true);

    send_channel_text(&bridge, &channel, "hello from the mesh").await;
    // Wait for the mock server to receive the signed POST.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if !webhook.requests.lock().await.is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "no outbound webhook received");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let requests = webhook.requests.lock().await.clone();
    let request = &requests[0];
    let signature = request
        .header("x-openmesh-signature")
        .expect("signature header");
    assert_eq!(
        signature,
        sign_body(b"integration-secret", &request.body),
        "signature must be HMAC-SHA256 over the raw body"
    );
    assert_eq!(request.header("content-type"), Some("application/json"));
    let payload = request.json();
    assert_eq!(payload["event"], "message");
    assert_eq!(payload["connector"], "hook");
    assert_eq!(payload["channel_id"], channel);
    assert!(payload["message_id"].as_u64().is_some());
    assert_eq!(payload["from"], bridge.node_id);
    assert_eq!(payload["from_name"], "openmesh-bridge");
    assert_eq!(payload["text"], "hello from the mesh");
    assert!(payload["timestamp_ms"].as_u64().unwrap() > 0);
    assert!(payload.get("attachment").is_none());

    let view = wait_gateway_status(&bridge, "hook", 1, 0).await;
    assert_eq!(view["status"]["last_status"], 200);
    assert_eq!(view["outbound"]["secret_set"], true);
    let listed = view.to_string();
    assert!(
        !listed.contains("integration-secret"),
        "API leaked the secret"
    );

    let activity = api_get(&bridge, "/api/activity?limit=20").await.to_string();
    assert!(activity.contains("gateway:hook"));
    assert!(activity.contains("/api/gateways/hook/outbound"));
    assert!(!activity.contains("integration-secret"));
    bridge.shutdown().await;
}

#[tokio::test]
async fn outbound_retries_on_500_then_gives_up() {
    const ENV: &str = "OPENMESH_TEST_GW_INT_RETRY";
    set_env(ENV, "retry-secret");
    let dir = TempDir::new().expect("temp dir");
    let bridge = start_bridge(dir.path()).await;
    let channel = create_channel(&bridge).await;
    let webhook = start_webhook(vec![500]).await;
    let connector = json!({
        "id": "flaky",
        "outbound": {
            "url": webhook.url,
            "secret_env": ENV,
            "max_retries": 1,
            "timeout_ms": 2000
        }
    });
    let (status, _) = api_send(&bridge, "PUT", "/api/gateways/flaky", connector).await;
    assert_eq!(status, 200);
    send_channel_text(&bridge, &channel, "retry me").await;
    let view = wait_gateway_status(&bridge, "flaky", 0, 1).await;
    assert_eq!(
        view["status"]["retries"], 1,
        "one retry after the first attempt"
    );
    assert_eq!(view["status"]["last_status"], 500);
    assert!(
        view["status"]["last_error"]
            .as_str()
            .unwrap()
            .contains("HTTP 500")
    );
    // Two connections reached the mock: the initial attempt and the retry.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if webhook.requests.lock().await.len() >= 2 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "retry never reached the mock server"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let activity = api_get(&bridge, "/api/activity?limit=20").await;
    let entries = activity["entries"].as_array().cloned().unwrap_or_default();
    assert!(
        entries
            .iter()
            .any(|entry| { entry["source"] == "gateway:flaky" && entry["ok"] == false })
    );
    bridge.shutdown().await;
}

#[tokio::test]
async fn outbound_channel_filter_can_be_narrowed_and_widened() {
    const ENV: &str = "OPENMESH_TEST_GW_INT_FILTER";
    set_env(ENV, "filter-secret");
    let dir = TempDir::new().expect("temp dir");
    let bridge = start_bridge(dir.path()).await;
    let channel = create_channel(&bridge).await;
    let webhook = start_webhook(vec![200]).await;
    let connector = json!({
        "id": "filtered",
        "outbound": {
            "url": webhook.url,
            "secret_env": ENV,
            "channel_ids": ["1112131415161718"],
            "timeout_ms": 2000,
            "max_retries": 0
        }
    });
    let (status, _) = api_send(&bridge, "PUT", "/api/gateways/filtered", connector.clone()).await;
    assert_eq!(status, 200);
    send_channel_text(&bridge, &channel, "not mirrored").await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        webhook.requests.lock().await.is_empty(),
        "filtered channel must not deliver"
    );

    let mut widened = connector;
    widened["outbound"]["channel_ids"] = json!([]);
    let (status, _) = api_send(&bridge, "PUT", "/api/gateways/filtered", widened).await;
    assert_eq!(status, 200);
    send_channel_text(&bridge, &channel, "now mirrored").await;
    wait_gateway_status(&bridge, "filtered", 1, 0).await;
    assert_eq!(
        webhook.requests.lock().await[0].json()["text"],
        "now mirrored"
    );
    bridge.shutdown().await;
}

#[tokio::test]
async fn outbound_attachment_is_metadata_only() {
    const ENV: &str = "OPENMESH_TEST_GW_INT_ATT";
    set_env(ENV, "attachment-secret");
    let dir = TempDir::new().expect("temp dir");
    let bridge = start_bridge(dir.path()).await;
    let channel = create_channel(&bridge).await;
    let webhook = start_webhook(vec![200]).await;
    let connector = json!({
        "id": "files",
        "outbound": {
            "url": webhook.url,
            "secret_env": ENV,
            "include_attachments": true,
            "max_retries": 0
        }
    });
    let (status, _) = api_send(&bridge, "PUT", "/api/gateways/files", connector).await;
    assert_eq!(status, 200);

    let bytes = b"RIFF....WEBP-fake-bytes";
    let upload = request(
        bridge.addr,
        "POST",
        "/api/attachments",
        Some(API_TOKEN),
        &[("Content-Type", "image/webp")],
        Some(bytes),
    )
    .await;
    assert_eq!(upload.status, 200, "upload attachment");
    let attachment_id = upload.body["id"]
        .as_str()
        .expect("attachment id")
        .to_string();
    let (status, body) = api_send(
        &bridge,
        "POST",
        &format!("/api/channels/{channel}/messages"),
        json!({ "attachment_id": attachment_id }),
    )
    .await;
    assert_eq!(status, 200, "send attachment message: {body}");

    wait_gateway_status(&bridge, "files", 1, 0).await;
    let requests = webhook.requests.lock().await.clone();
    let payload = requests[0].json();
    let attachment = payload["attachment"]
        .as_object()
        .expect("attachment metadata");
    assert_eq!(attachment.len(), 2, "only mime + size are pushed");
    assert_eq!(attachment["mime"], "image/webp");
    assert_eq!(attachment["size"], bytes.len());
    assert!(
        !String::from_utf8_lossy(&requests[0].body).contains(&attachment_id),
        "attachment id/bytes must not leak into webhook payload"
    );
    bridge.shutdown().await;
}

#[tokio::test]
async fn outbound_missing_secret_env_records_failure_without_secret() {
    let dir = TempDir::new().expect("temp dir");
    // Stage the config directly (PUT would reject an unset env var on purpose).
    std::fs::write(
        dir.path().join("gateways.json"),
        serde_json::to_string_pretty(&gateway_config(json!({
            "id": "unset",
            "outbound": {
                "url": "http://127.0.0.1:9/hook",
                "secret_env": "OPENMESH_TEST_GW_INT_DEFINITELY_UNSET",
                "max_retries": 0
            }
        })))
        .unwrap(),
    )
    .expect("write gateways.json");
    let bridge = start_bridge(dir.path()).await;
    let channel = create_channel(&bridge).await;
    send_channel_text(&bridge, &channel, "no secret configured").await;
    let view = wait_gateway_status(&bridge, "unset", 0, 1).await;
    assert!(
        view["status"]["last_error"]
            .as_str()
            .unwrap()
            .contains("is not set")
    );
    let activity = api_get(&bridge, "/api/activity?limit=20").await.to_string();
    assert!(activity.contains("gateway:unset"));
    assert!(!activity.contains("OPENMESH_TEST_GW_INT_DEFINITELY_UNSET="));
    bridge.shutdown().await;
}

// ---------------------------------------------------------------------------------------------
// Inbound
// ---------------------------------------------------------------------------------------------

async fn setup_inbound_bridge(
    dir: &Path,
    env: &str,
    secret: &str,
    max_chars: usize,
) -> (Bridge, String) {
    set_env(env, secret);
    let bridge = start_bridge(dir).await;
    let channel = create_channel(&bridge).await;
    let connector = json!({
        "id": "sms",
        "inbound": {
            "enabled": true,
            "secret_env": env,
            "channel_id": channel,
            "max_chars": max_chars
        }
    });
    let (status, body) = api_send(&bridge, "PUT", "/api/gateways/sms", connector).await;
    assert_eq!(status, 200, "configure inbound: {body}");
    (bridge, channel)
}

async fn post_inbound(bridge: &Bridge, connector: &str, secret: &str, body: Value) -> HttpResponse {
    let raw = serde_json::to_vec(&body).expect("json body");
    let signature = sign_body(secret.as_bytes(), &raw);
    request(
        bridge.addr,
        "POST",
        &format!("/api/gateways/{connector}/inbound"),
        None,
        &[("X-OpenMesh-Signature", &signature)],
        Some(&raw),
    )
    .await
}

async fn channel_messages(bridge: &Bridge, channel: &str) -> Vec<Value> {
    let value = api_get(
        bridge,
        &format!("/api/messages/channel/{channel}?limit=200"),
    )
    .await;
    value["messages"].as_array().cloned().unwrap_or_default()
}

#[tokio::test]
async fn inbound_webhook_injects_a_channel_message() {
    const ENV: &str = "OPENMESH_TEST_GW_INT_IN_A";
    let dir = TempDir::new().expect("temp dir");
    let (bridge, channel) = setup_inbound_bridge(dir.path(), ENV, "inbound-secret", 400).await;
    let response = post_inbound(
        &bridge,
        "sms",
        "inbound-secret",
        json!({
            "text": "reply from SMS",
            "from": "+4712345678",
            "timestamp_ms": 1_760_000_000_000u64
        }),
    )
    .await;
    assert_eq!(response.status, 200, "inbound inject: {:?}", response.body);
    assert_eq!(response.body["ok"], true);
    assert_eq!(response.body["channel_id"], channel);
    assert!(response.body["message_id"].as_u64().is_some());

    let messages = channel_messages(&bridge, &channel).await;
    let injected = messages
        .iter()
        .find(|message| message["text"] == "reply from SMS")
        .expect("injected message in channel");
    assert_eq!(injected["channel_id"], channel);
    let from_name = injected["from_name"].as_str().unwrap_or("");
    assert!(
        from_name.contains("sms"),
        "display attribution: {from_name}"
    );
    assert!(
        from_name.contains("+4712345678"),
        "display attribution: {from_name}"
    );
    // The injected message is signed by this bridge, never by the adapter.
    assert_eq!(injected["from"], bridge.node_id);

    let activity = api_get(&bridge, "/api/activity?limit=20").await.to_string();
    assert!(activity.contains("gateway:sms"));
    assert!(activity.contains("/api/gateways/sms/inbound"));
    assert!(!activity.contains("inbound-secret"));
    bridge.shutdown().await;
}

#[tokio::test]
async fn inbound_bad_signature_is_rejected_without_injection() {
    const ENV: &str = "OPENMESH_TEST_GW_INT_IN_B";
    let dir = TempDir::new().expect("temp dir");
    let (bridge, channel) = setup_inbound_bridge(dir.path(), ENV, "right-secret", 400).await;
    let before = channel_messages(&bridge, &channel).await.len();
    let response = post_inbound(
        &bridge,
        "sms",
        "wrong-secret",
        json!({ "text": "do not inject" }),
    )
    .await;
    assert_eq!(response.status, 401, "bad signature: {:?}", response.body);
    assert_eq!(response.body["error"], "unauthorized");
    assert_eq!(channel_messages(&bridge, &channel).await.len(), before);
    // The same body with the right signature still works afterwards (no lockout).
    let good = post_inbound(&bridge, "sms", "right-secret", json!({ "text": "ok" })).await;
    assert_eq!(good.status, 200);
    bridge.shutdown().await;
}

#[tokio::test]
async fn inbound_disabled_unknown_and_over_length_statuses_do_not_inject() {
    const ENV: &str = "OPENMESH_TEST_GW_INT_IN_C";
    let dir = TempDir::new().expect("temp dir");
    set_env(ENV, "small-secret");
    let bridge = start_bridge(dir.path()).await;
    let channel = create_channel(&bridge).await;
    let connector = json!({
        "id": "sms",
        "enabled": true,
        "inbound": {
            "enabled": false,
            "secret_env": ENV,
            "channel_id": channel,
            "max_chars": 5
        }
    });
    let (status, _) = api_send(&bridge, "PUT", "/api/gateways/sms", connector.clone()).await;
    assert_eq!(status, 200);
    let disabled = post_inbound(&bridge, "sms", "small-secret", json!({ "text": "hi" })).await;
    assert_eq!(disabled.status, 404, "{:?}", disabled.body);
    let unknown = post_inbound(&bridge, "nope", "small-secret", json!({ "text": "hi" })).await;
    assert_eq!(unknown.status, 404);
    assert!(channel_messages(&bridge, &channel).await.is_empty());

    let mut enabled = connector;
    enabled["inbound"]["enabled"] = json!(true);
    let (status, _) = api_send(&bridge, "PUT", "/api/gateways/sms", enabled).await;
    assert_eq!(status, 200);
    let too_long = post_inbound(
        &bridge,
        "sms",
        "small-secret",
        json!({ "text": "this text is definitely too long" }),
    )
    .await;
    assert_eq!(too_long.status, 400, "{:?}", too_long.body);
    assert_eq!(too_long.body["error"], "too_long");
    assert!(channel_messages(&bridge, &channel).await.is_empty());
    bridge.shutdown().await;
}

#[tokio::test]
async fn inbound_rate_limit_is_30_per_minute() {
    const ENV: &str = "OPENMESH_TEST_GW_INT_IN_RATE";
    let dir = TempDir::new().expect("temp dir");
    let (bridge, channel) = setup_inbound_bridge(dir.path(), ENV, "rate-secret", 100).await;
    let mut accepted = 0;
    for index in 0..30 {
        let response = post_inbound(
            &bridge,
            "sms",
            "rate-secret",
            json!({ "text": format!("message {index}") }),
        )
        .await;
        assert_eq!(response.status, 200, "request {index}: {:?}", response.body);
        accepted += 1;
    }
    let limited = post_inbound(
        &bridge,
        "sms",
        "rate-secret",
        json!({ "text": "one too many" }),
    )
    .await;
    assert_eq!(limited.status, 429, "{:?}", limited.body);
    assert_eq!(limited.body["error"], "rate_limited");
    assert_eq!(channel_messages(&bridge, &channel).await.len(), accepted);
    let view = api_get(&bridge, "/api/gateways").await;
    assert_eq!(view["connectors"][0]["status"]["inbound_accepted"], 30);
    assert_eq!(view["connectors"][0]["status"]["inbound_rejected"], 1);
    bridge.shutdown().await;
}

// ---------------------------------------------------------------------------------------------
// Configuration, redaction and persistence
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn gateway_api_rejects_inline_secrets_and_redacts() {
    const ENV: &str = "OPENMESH_TEST_GW_INT_CFG";
    let dir = TempDir::new().expect("temp dir");
    let bridge = start_bridge(dir.path()).await;

    let inline = json!({
        "id": "bad",
        "outbound": {
            "url": "https://adapter.example/hook",
            "secret": "inline-value"
        }
    });
    let (status, body) = api_send(&bridge, "PUT", "/api/gateways/bad", inline).await;
    assert_eq!(status, 400, "{body}");
    assert!(body.to_string().contains("inline secret"));

    let unset = json!({
        "id": "bad",
        "outbound": {
            "url": "https://adapter.example/hook",
            "secret_env": "OPENMESH_TEST_GW_INT_NEVER_SET"
        }
    });
    let (status, body) = api_send(&bridge, "PUT", "/api/gateways/bad", unset).await;
    assert_eq!(status, 400, "{body}");
    assert!(body.to_string().contains("is not set"));

    set_env(ENV, "config-secret-value");
    let connector = json!({
        "id": "bad",
        "outbound": {
            "url": "https://adapter.example/hook?api_key=url-secret",
            "secret_env": ENV,
            "max_retries": 0
        }
    });
    let (status, body) = api_send(&bridge, "PUT", "/api/gateways/bad", connector).await;
    assert_eq!(status, 200, "{body}");
    let rendered = body.to_string();
    assert!(!rendered.contains("config-secret-value"));
    assert!(
        !rendered.contains("url-secret"),
        "URL query must be redacted"
    );
    assert_eq!(body["outbound"]["secret_env"], ENV);
    assert_eq!(body["outbound"]["secret_set"], true);

    let listed = api_get(&bridge, "/api/gateways").await.to_string();
    assert!(!listed.contains("config-secret-value"));
    assert!(!listed.contains("url-secret"));

    // The persisted file holds the env-var name but never the value.
    let config_path = dir.path().join("gateways.json");
    let stored = std::fs::read_to_string(&config_path).expect("gateways.json exists");
    assert!(stored.contains(ENV));
    assert!(!stored.contains("config-secret-value"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&config_path)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let (status, body) = api_send(&bridge, "DELETE", "/api/gateways/bad", json!({})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["deleted"], true);
    let empty = api_get(&bridge, "/api/gateways").await;
    assert_eq!(empty["connectors"].as_array().unwrap().len(), 0);
    let (status, _) = api_send(&bridge, "DELETE", "/api/gateways/bad", json!({})).await;
    assert_eq!(status, 404);
    bridge.shutdown().await;
}

#[tokio::test]
async fn gateway_config_persists_across_bridge_restarts() {
    const ENV: &str = "OPENMESH_TEST_GW_INT_PERSIST";
    set_env(ENV, "persist-secret");
    let dir = TempDir::new().expect("temp dir");
    let bridge = start_bridge(dir.path()).await;
    let (status, body) = api_send(
        &bridge,
        "PUT",
        "/api/gateways/persisted",
        json!({
            "id": "persisted",
            "outbound": {
                "url": "http://127.0.0.1:9/hook",
                "secret_env": ENV,
                "max_retries": 0
            }
        }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    bridge.shutdown().await;

    let restarted = start_bridge(dir.path()).await;
    let listed = api_get(&restarted, "/api/gateways").await;
    let connectors = listed["connectors"].as_array().cloned().unwrap_or_default();
    assert_eq!(connectors.len(), 1, "{listed}");
    assert_eq!(connectors[0]["id"], "persisted");
    assert_eq!(connectors[0]["outbound"]["secret_set"], true);
    assert!(!listed.to_string().contains("persist-secret"));
    restarted.shutdown().await;
}

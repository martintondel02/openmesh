//! Regulatory legal maxima + override API (docs/design/regulatory-and-regions.md §2.3–§2.4).

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

const API_TOKEN: &str = "reg00112233445566778899aabbccddeeff00112233445566778899aabbcc";

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
    body: Vec<u8>,
}

impl HttpResult {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

async fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> HttpResult {
    let mut stream = TcpStream::connect(addr).await.expect("connect API");
    let payload = body.map(|value| value.to_string()).unwrap_or_default();
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {API_TOKEN}\r\nConnection: close\r\n"
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
    HttpResult { status, body }
}

async fn get(addr: SocketAddr, path: &str) -> Value {
    http(addr, "GET", path, None, &[]).await.json()
}

async fn patch(addr: SocketAddr, body: Value) -> HttpResult {
    http(addr, "PATCH", "/api/config", Some(body), &[]).await
}

async fn patch_agent(addr: SocketAddr, body: Value, agent: &str) -> HttpResult {
    http(
        addr,
        "PATCH",
        "/api/config",
        Some(body),
        &[("X-OpenMesh-Agent", agent)],
    )
    .await
}

// --- tiny MCP child helper ---------------------------------------------------------------

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
}

async fn spawn_mcp(addr: SocketAddr, read_only: bool) -> Mcp {
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_openmesh-bridge"));
    command
        .arg("mcp")
        .env("OPENMESH_API", format!("http://{addr}"))
        .env("OPENMESH_TOKEN", API_TOKEN)
        .env("OPENMESH_AGENT_NAME", "reg-agent")
        .env("OPENMESH_MCP_READ_ONLY", if read_only { "1" } else { "0" })
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = command.spawn().expect("spawn mcp");
    let stdin = child.stdin.take().expect("mcp stdin");
    let stdout = child.stdout.take().expect("mcp stdout");
    Mcp {
        child,
        stdin,
        lines: BufReader::new(stdout).lines(),
    }
}

impl Mcp {
    async fn request(&mut self, request: Value) -> Value {
        self.stdin
            .write_all(request.to_string().as_bytes())
            .await
            .unwrap();
        self.stdin.write_all(b"\n").await.unwrap();
        self.stdin.flush().await.unwrap();
        let line = tokio::time::timeout(Duration::from_secs(20), self.lines.next_line())
            .await
            .expect("MCP response timeout")
            .expect("MCP stdout")
            .expect("MCP exited");
        serde_json::from_str(&line).expect("MCP JSON")
    }

    async fn shutdown(mut self) {
        drop(self.stdin);
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
        let _ = self.child.kill().await;
    }
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn config_exposes_legal_maxima_and_inactive_override() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path()).await;
    let addr = bridge.addr;

    let config = get(addr, "/api/config").await;
    assert_eq!(config["radio"]["region"], "EU868");
    assert_eq!(config["radio"]["max_power_dbm"], 27);
    assert_eq!(config["radio"]["duty_cycle_pct"], 10);
    assert_eq!(config["radio"]["max_bw_khz"], 250);
    assert_eq!(config["radio"]["tx_power_dbm"], 27);
    assert!((config["radio"]["frequency_mhz"].as_f64().unwrap() - 869.525).abs() < 1e-6);

    let legal = &config["legal"];
    assert_eq!(legal["region"], "EU868");
    assert_eq!(legal["max_power_dbm"], 27);
    assert_eq!(legal["duty_cycle_pct"], 10);
    assert_eq!(legal["max_bw_khz"], 250);
    assert!(
        legal["source"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("verify local law")
    );

    let over = &config["regulatory_override"];
    assert_eq!(over["enabled"], false);
    assert_eq!(over["acknowledged"], false);
    assert!(over["duty_cycle_pct"].is_null());
    assert!(over["max_power_dbm"].is_null());
    assert!(over["frequency_khz"].is_null());
    assert!(over["reason"].is_null());
    assert_eq!(config["override_active"], false);
    assert!(config["override_warning"].is_null());

    let status = get(addr, "/api/status").await;
    assert_eq!(status["regulatory_override_active"], false);
    assert!(status["regulatory_override_warning"].is_null());
    assert_eq!(status["radio"]["duty_cycle_pct"], 10);
    assert_eq!(status["radio"]["max_power_dbm"], 27);
    assert_eq!(status["radio"]["max_bw_khz"], 250);

    bridge.shutdown().await;
}

#[tokio::test]
async fn override_requires_ack_then_applies_and_can_be_disabled() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path()).await;
    let addr = bridge.addr;

    // Enabling/exceeding without acknowledgement is rejected with 422 + warning.
    let rejected = patch(
        addr,
        json!({ "regulatory_override": { "enabled": true, "duty_cycle_pct": 50 } }),
    )
    .await;
    assert_eq!(rejected.status, 422, "body: {}", rejected.json());
    let body = rejected.json();
    assert_eq!(body["code"], "regulatory_override_requires_acknowledgement");
    assert!(body["message"].as_str().unwrap().contains("acknowledg"));
    assert!(openmesh_core::OVERRIDE_WARNING.contains(body["warning"].as_str().unwrap()));
    assert_eq!(
        get(addr, "/api/config").await["regulatory_override"]["enabled"],
        false
    );

    // Acknowledged patch applies: effective power/frequency move, legal block survives.
    let applied = patch(
        addr,
        json!({
            "regulatory_override": {
                "enabled": true,
                "acknowledged": true,
                "duty_cycle_pct": 50,
                "max_power_dbm": 22,
                "reason": "test emergency"
            }
        }),
    )
    .await;
    assert_eq!(applied.status, 200, "body: {}", applied.json());
    let config = applied.json();
    assert_eq!(config["regulatory_override"]["enabled"], true);
    assert_eq!(config["regulatory_override"]["acknowledged"], true);
    assert_eq!(config["regulatory_override"]["duty_cycle_pct"], 50);
    assert_eq!(config["override_active"], true);
    assert_eq!(config["override_warning"], openmesh_core::OVERRIDE_WARNING);
    assert_eq!(config["radio"]["tx_power_dbm"], 22);
    // The legal maxima in the radio block stay the region values.
    assert_eq!(config["radio"]["max_power_dbm"], 27);
    assert_eq!(config["radio"]["duty_cycle_pct"], 10);

    let status = get(addr, "/api/status").await;
    assert_eq!(status["regulatory_override_active"], true);
    assert_eq!(
        status["regulatory_override_warning"],
        openmesh_core::OVERRIDE_WARNING
    );
    assert_eq!(status["radio"]["tx_power_dbm"], 22);
    assert_eq!(status["radio"]["duty_cycle_pct"], 50);

    // `confirm: true` is an accepted alias for `acknowledged`.
    let disabled = patch(addr, json!({ "regulatory_override": { "enabled": false } })).await;
    assert_eq!(disabled.status, 200);
    assert_eq!(disabled.json()["override_active"], false);
    let confirmed = patch(
        addr,
        json!({
            "regulatory_override": { "enabled": true, "confirm": true, "duty_cycle_pct": 100 }
        }),
    )
    .await;
    assert_eq!(confirmed.status, 200, "body: {}", confirmed.json());
    assert_eq!(
        confirmed.json()["regulatory_override"]["acknowledged"],
        true
    );
    assert_eq!(
        confirmed.json()["regulatory_override"]["duty_cycle_pct"],
        100
    );

    // Frequency override changes the effective frequency in config + status.
    let frequency = patch(
        addr,
        json!({
            "regulatory_override": { "enabled": true, "acknowledged": true, "frequency_khz": 868_100 }
        }),
    )
    .await;
    assert_eq!(frequency.status, 200, "body: {}", frequency.json());
    assert_eq!(frequency.json()["radio"]["frequency_mhz"], 868.1);
    assert_eq!(
        get(addr, "/api/status").await["radio"]["frequency_mhz"],
        868.1
    );

    // Disabling is always allowed (no acknowledgement required).
    let disabled = patch(addr, json!({ "regulatory_override": { "enabled": false } })).await;
    assert_eq!(disabled.status, 200);
    assert_eq!(disabled.json()["override_active"], false);
    assert!(disabled.json()["override_warning"].is_null());

    // Safe reduction on an enabled override does not need a fresh acknowledgement.
    let safe = patch(
        addr,
        json!({ "regulatory_override": { "enabled": true, "duty_cycle_pct": 5 } }),
    )
    .await;
    assert_eq!(safe.status, 200, "body: {}", safe.json());
    assert_eq!(safe.json()["regulatory_override"]["duty_cycle_pct"], 5);
    assert_eq!(safe.json()["override_active"], true);

    bridge.shutdown().await;
}

#[tokio::test]
async fn override_validation_matrix_and_profile_bandwidth_rules() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path()).await;
    let addr = bridge.addr;

    for (body, expected_code) in [
        (
            json!({"regulatory_override": {"enabled": true, "acknowledged": true, "duty_cycle_pct": 101}}),
            "regulatory_override_duty_out_of_range",
        ),
        (
            json!({"regulatory_override": {"enabled": true, "acknowledged": true, "reason": "x".repeat(200)}}),
            "regulatory_override_reason_too_long",
        ),
        (
            json!({"regulatory_override": {"enabled": true, "acknowledged": true, "frequency_khz": 100_000}}),
            "regulatory_override_frequency_out_of_range",
        ),
        (
            json!({"regulatory_override": {"bogus_field": true}}),
            "regulatory_override_invalid",
        ),
    ] {
        let response = patch(addr, body).await;
        assert_eq!(response.status, 422, "body: {}", response.json());
        assert_eq!(response.json()["code"], expected_code);
        assert!(response.json()["override_active"].is_null());
    }
    assert_eq!(
        get(addr, "/api/config").await["regulatory_override"]["enabled"],
        false
    );

    // A profile wider than the region's legal maximum is rejected with allowed profiles listed.
    let rejected = patch(addr, json!({"radio": {"profile": "SHORT_TURBO"}})).await;
    assert_eq!(rejected.status, 422, "body: {}", rejected.json());
    let body = rejected.json();
    assert_eq!(body["code"], "profile_not_allowed_in_region");
    let message = body["message"].as_str().unwrap();
    assert!(message.contains("SHORT_TURBO"));
    assert!(message.contains("250"));
    assert!(message.contains("MEDIUM_FAST"));
    assert_eq!(
        get(addr, "/api/config").await["radio"]["profile"],
        "MEDIUM_FAST"
    );

    // The same profile is legal in a 500 kHz region; updating region+profile together works.
    let allowed = patch(
        addr,
        json!({"radio": {"region": "US915", "profile": "SHORT_TURBO"}}),
    )
    .await;
    assert_eq!(allowed.status, 200, "body: {}", allowed.json());
    let config = allowed.json();
    assert_eq!(config["radio"]["region"], "US915");
    assert_eq!(config["radio"]["profile"], "SHORT_TURBO");
    assert_eq!(config["radio"]["max_power_dbm"], 30);
    assert_eq!(config["radio"]["max_bw_khz"], 500);
    assert_eq!(config["radio"]["duty_cycle_pct"], Value::Null);
    assert!(config["legal"]["source"].as_str().unwrap().contains("FCC"));

    bridge.shutdown().await;
}

#[tokio::test]
async fn override_changes_are_attributed_and_setup_reports_legal_and_state() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path()).await;
    let addr = bridge.addr;

    let recommend = get(addr, "/api/setup/recommend?country=NO&role=repeater").await;
    let legal = &recommend["legal"];
    assert_eq!(legal["region"], "EU868");
    assert_eq!(legal["max_power_dbm"], 27);
    assert_eq!(legal["duty_cycle_pct"], 10);
    assert_eq!(legal["max_bw_khz"], 250);
    assert!(
        legal["source"]
            .as_str()
            .unwrap()
            .contains("verify local law")
    );
    assert_eq!(recommend["warning"], Value::Null);

    let applied = patch_agent(
        addr,
        json!({"regulatory_override": {"enabled": true, "acknowledged": true, "duty_cycle_pct": 42, "reason": "storm"}}),
        "claude-test",
    )
    .await;
    assert_eq!(applied.status, 200, "body: {}", applied.json());

    let activity = get(addr, "/api/activity?limit=10").await;
    let entry = &activity["entries"][0];
    assert_eq!(entry["source"], "agent:claude-test");
    assert_eq!(entry["method"], "PATCH");
    assert_eq!(entry["path"], "/api/config");
    assert_eq!(entry["ok"], true);
    let summary = entry["summary"].as_str().unwrap();
    assert!(summary.contains("regulatory override"));
    assert!(summary.contains("duty=42%"));
    assert!(summary.contains("acknowledged=true"));
    // The activity log records no secrets from the override patch.
    assert!(!activity.to_string().contains(API_TOKEN));
    assert!(!activity.to_string().contains("token="));

    let state = get(addr, "/api/setup/state").await;
    assert_eq!(state["regulatory_override"]["enabled"], true);
    assert_eq!(state["regulatory_override"]["acknowledged"], true);
    assert_eq!(state["override_active"], true);
    assert_eq!(state["override_warning"], openmesh_core::OVERRIDE_WARNING);
    // Setup never enables the override by itself.
    assert_eq!(state["setup_completed"], false);

    bridge.shutdown().await;
}

#[tokio::test]
async fn override_persists_across_restart_and_setup_never_enables_it() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path()).await;
    let addr = bridge.addr;

    // A fresh bridge (and setup state) never enables the override.
    assert_eq!(
        get(addr, "/api/setup/state").await["regulatory_override"]["enabled"],
        false
    );
    let applied = patch(
        addr,
        json!({
            "regulatory_override": {"enabled": true, "acknowledged": true,
                                     "duty_cycle_pct": 25, "max_power_dbm": 20,
                                     "reason": "persisted"}
        }),
    )
    .await;
    assert_eq!(applied.status, 200, "body: {}", applied.json());
    bridge.shutdown().await;

    let reopened = start_bridge(dir.path()).await;
    let addr = reopened.addr;
    let config = get(addr, "/api/config").await;
    assert_eq!(config["regulatory_override"]["enabled"], true);
    assert_eq!(config["regulatory_override"]["acknowledged"], true);
    assert_eq!(config["regulatory_override"]["duty_cycle_pct"], 25);
    assert_eq!(config["regulatory_override"]["max_power_dbm"], 20);
    assert_eq!(config["regulatory_override"]["reason"], "persisted");
    assert_eq!(config["override_active"], true);
    assert_eq!(config["radio"]["tx_power_dbm"], 20);
    assert_eq!(
        get(addr, "/api/status").await["radio"]["duty_cycle_pct"],
        25
    );
    reopened.shutdown().await;
}

#[tokio::test]
async fn mcp_override_requires_confirm_and_read_only_refuses() {
    let dir = TempDir::new().unwrap();
    let bridge = start_bridge(dir.path()).await;
    let addr = bridge.addr;
    let mut mcp = spawn_mcp(addr, false).await;
    let init = mcp
        .request(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}))
        .await;
    assert_eq!(init["result"]["serverInfo"]["name"], "openmesh");

    let refused = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "openmesh_set_config", "arguments": {
                "regulatory_override": {"enabled": true, "duty_cycle_pct": 50}
            }}
        }))
        .await;
    assert_eq!(refused["result"]["isError"], true);
    let error = &refused["result"]["structuredContent"]["error"];
    assert_eq!(error["code"], "confirm_required");
    assert_eq!(error["help_topic"], "regulatory-and-emergency");
    assert!(
        refused["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Regulatory override")
    );
    assert_eq!(
        get(addr, "/api/config").await["regulatory_override"]["enabled"],
        false
    );

    let confirmed = mcp
        .request(json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "openmesh_set_config", "arguments": {
                "regulatory_override": {"enabled": true, "duty_cycle_pct": 50},
                "confirm": true
            }}
        }))
        .await;
    assert_ne!(confirmed["result"]["isError"], true, "{confirmed}");
    assert!(
        confirmed["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Warning")
    );
    assert_eq!(
        confirmed["result"]["structuredContent"]["override_warning"],
        openmesh_core::OVERRIDE_WARNING
    );
    let config = get(addr, "/api/config").await;
    assert_eq!(config["regulatory_override"]["enabled"], true);
    assert_eq!(config["regulatory_override"]["acknowledged"], true);
    let activity = get(addr, "/api/activity?limit=5").await;
    assert_eq!(activity["entries"][0]["source"], "agent:reg-agent");
    mcp.shutdown().await;

    // Read-only MCP refuses the write outright.
    let mut read_only = spawn_mcp(addr, true).await;
    let refused = read_only
        .request(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "openmesh_set_config", "arguments": {
                "regulatory_override": {"enabled": false}, "confirm": true
            }}
        }))
        .await;
    assert_eq!(refused["result"]["isError"], true);
    assert_eq!(
        refused["result"]["structuredContent"]["error"]["code"],
        "read_only"
    );
    read_only.shutdown().await;

    bridge.shutdown().await;
}
